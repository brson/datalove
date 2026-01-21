//! Generation context for tracking scope during worldfile generation.

use std::collections::HashSet;
use datalove_datalit::ast::{Heap, TypeHintAndHeap};
use crate::config::WorldGenConfig;

/// A variable in scope.
#[derive(Clone)]
pub struct Variable<'db> {
    pub name: String,
    pub type_hint: TypeHintAndHeap<'db>,
    pub is_mutable: bool,
}

/// A function signature.
#[derive(Clone)]
pub struct FunctionSig<'db> {
    pub name: String,
    pub params: Vec<(String, TypeHintAndHeap<'db>)>,
    pub return_type: Option<TypeHintAndHeap<'db>>,
}

/// A type alias.
#[derive(Clone)]
pub struct TypeAlias<'db> {
    pub name: String,
    pub type_hint: TypeHintAndHeap<'db>,
}

/// A module in the worldfile.
#[derive(Clone)]
pub struct ModuleInfo<'db> {
    pub library: String,
    pub package: String,
    pub module: String,
    pub functions: Vec<FunctionSig<'db>>,
    pub type_aliases: Vec<TypeAlias<'db>>,
}

impl<'db> ModuleInfo<'db> {
    /// Get the full module path (e.g., "sys/std/u32").
    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.library, self.package, self.module)
    }

    /// Get the module alias for imports (e.g., "u32").
    pub fn alias(&self) -> &str {
        &self.module
    }
}

/// Generation context tracking what's in scope.
#[derive(Clone)]
pub struct GenContext<'db> {
    /// Variables in scope.
    pub variables: Vec<Variable<'db>>,

    /// Functions available in this module.
    pub functions: Vec<FunctionSig<'db>>,

    /// Type aliases defined in this module.
    pub type_aliases: Vec<TypeAlias<'db>>,

    /// Imported functions from other modules.
    pub imported_functions: Vec<FunctionSig<'db>>,

    /// Expected return type for current function.
    pub return_type: Option<TypeHintAndHeap<'db>>,

    /// Current loop nesting depth.
    pub loop_depth: usize,

    /// Current control flow nesting depth.
    pub control_flow_depth: usize,

    /// Name of the current function being generated (to prevent self-recursion).
    pub current_function_name: Option<String>,

    /// Maximum index of local functions that can be called (to prevent mutual recursion).
    /// When set, only functions with index < this value can be called.
    /// Functions are named fn0, fn1, fn2, etc.
    pub max_callable_function_index: Option<usize>,

    /// Variables that have been consumed (moved) and can't be used again.
    /// This is used for ownership tracking - global heap types get moved on first use.
    pub consumed_variables: HashSet<String>,
}

impl<'db> GenContext<'db> {
    pub fn new() -> Self {
        GenContext {
            variables: Vec::new(),
            functions: Vec::new(),
            type_aliases: Vec::new(),
            imported_functions: Vec::new(),
            return_type: None,
            loop_depth: 0,
            control_flow_depth: 0,
            current_function_name: None,
            max_callable_function_index: None,
            consumed_variables: HashSet::new(),
        }
    }

    /// Mark a variable as consumed (moved). It can't be used again.
    pub fn consume_variable(&mut self, name: &str) {
        self.consumed_variables.insert(name.to_string());
    }

    /// Check if a variable has been consumed.
    pub fn is_consumed(&self, name: &str) -> bool {
        self.consumed_variables.contains(name)
    }

    /// Get all callable functions (local + imported, with shadowing).
    ///
    /// Local functions shadow imported functions with the same name.
    /// Later imports shadow earlier imports with the same name.
    /// The current function (if set) is excluded to prevent self-recursion.
    /// Local functions with index >= max_callable_function_index are excluded to prevent
    /// mutual recursion (only call functions whose bodies have already been generated).
    pub fn callable_functions(&self) -> impl Iterator<Item = &FunctionSig<'db>> {
        // Get current function name to exclude.
        let current_fn = self.current_function_name.as_deref();
        let max_index = self.max_callable_function_index;

        // Filter local functions by index to prevent mutual recursion.
        // Functions are named fn0, fn1, fn2, etc.
        let callable_local: Vec<_> = self.functions.iter()
            .filter(move |f| {
                // Exclude current function (self-recursion).
                if current_fn == Some(f.name.as_str()) {
                    return false;
                }
                // If max_callable_function_index is set, only allow functions with lower index.
                if let Some(max_idx) = max_index {
                    if let Some(idx) = extract_function_index(&f.name) {
                        return idx < max_idx;
                    }
                }
                true
            })
            .collect();

        // Collect callable local function names for shadowing check.
        let local_names: std::collections::HashSet<_> =
            callable_local.iter().map(|f| f.name.as_str()).collect();

        // For imported functions, later imports shadow earlier ones.
        // Keep track of which names we've seen (iterating in reverse).
        let mut seen_import_names = std::collections::HashSet::new();
        let non_shadowed_imports: Vec<_> = self.imported_functions
            .iter()
            .rev()
            .filter(|f| {
                // Exclude current function.
                if current_fn == Some(f.name.as_str()) {
                    return false;
                }
                if local_names.contains(f.name.as_str()) {
                    false // Shadowed by local
                } else if seen_import_names.contains(f.name.as_str()) {
                    false // Shadowed by later import
                } else {
                    seen_import_names.insert(f.name.as_str());
                    true
                }
            })
            .collect();

        // Return callable local functions + non-shadowed imports.
        callable_local.into_iter()
            .chain(non_shadowed_imports.into_iter().rev())
    }
}

/// Extract the numeric index from a function name like "fn0", "fn1", etc.
fn extract_function_index(name: &str) -> Option<usize> {
    if name.starts_with("fn") {
        name[2..].parse().ok()
    } else {
        None
    }
}

impl<'db> GenContext<'db> {
    /// Find variables of a specific type that are not consumed.
    ///
    /// For global heap types, consumed variables are filtered out because
    /// they've already been moved and can't be used again.
    pub fn variables_of_type(
        &self,
        db: &'db dyn salsa::Database,
        type_hint: TypeHintAndHeap<'db>,
    ) -> Vec<&Variable<'db>> {
        let is_global = type_hint.heap(db) == Heap::Global;
        self.variables
            .iter()
            .filter(|v| {
                if !types_match(db, v.type_hint, type_hint) {
                    return false;
                }
                // Filter out consumed variables for global heap types.
                if is_global && self.is_consumed(&v.name) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Find mutable variables that are not consumed.
    pub fn mutable_variables(&self, db: &'db dyn salsa::Database) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| {
                if !v.is_mutable {
                    return false;
                }
                // Filter out consumed variables for global heap types.
                if v.type_hint.heap(db) == Heap::Global && self.is_consumed(&v.name) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Check if we've reached max control flow depth.
    pub fn at_max_depth(&self, config: &WorldGenConfig) -> bool {
        self.control_flow_depth >= config.max_control_flow_depth
    }
}

impl<'db> Default for GenContext<'db> {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if two TypeHintAndHeap values match (same heap and type structure).
pub fn types_match<'db>(
    db: &'db dyn salsa::Database,
    a: TypeHintAndHeap<'db>,
    b: TypeHintAndHeap<'db>,
) -> bool {
    // Salsa tracked structs have identity semantics, so we compare the underlying values.
    a.heap(db) == b.heap(db) && type_hints_equal(db, a.type_hint(db), b.type_hint(db))
}

/// Compare two TypeHint values for structural equality.
fn type_hints_equal<'db>(
    db: &'db dyn salsa::Database,
    a: datalove_datalit::ast::TypeHint<'db>,
    b: datalove_datalit::ast::TypeHint<'db>,
) -> bool {
    use datalove_datalit::ast::TypeHint;
    match (a, b) {
        (TypeHint::Bool, TypeHint::Bool) => true,
        (TypeHint::U8, TypeHint::U8) => true,
        (TypeHint::I8, TypeHint::I8) => true,
        (TypeHint::U16, TypeHint::U16) => true,
        (TypeHint::I16, TypeHint::I16) => true,
        (TypeHint::U32, TypeHint::U32) => true,
        (TypeHint::I32, TypeHint::I32) => true,
        (TypeHint::U64, TypeHint::U64) => true,
        (TypeHint::I64, TypeHint::I64) => true,
        (TypeHint::Usize, TypeHint::Usize) => true,
        (TypeHint::Isize, TypeHint::Isize) => true,
        (TypeHint::F32, TypeHint::F32) => true,
        (TypeHint::F64, TypeHint::F64) => true,
        (TypeHint::Int, TypeHint::Int) => true,
        (TypeHint::String, TypeHint::String) => true,
        (TypeHint::Data, TypeHint::Data) => true,
        (TypeHint::Error, TypeHint::Error) => true,
        (TypeHint::List(la), TypeHint::List(lb)) => {
            types_match(db, la.element_type, lb.element_type)
        }
        (TypeHint::Option(oa), TypeHint::Option(ob)) => {
            types_match(db, oa.inner_type, ob.inner_type)
        }
        (TypeHint::Result(ra), TypeHint::Result(rb)) => {
            types_match(db, ra.inner_type, rb.inner_type)
        }
        (TypeHint::Map(ma), TypeHint::Map(mb)) => {
            types_match(db, ma.key_type, mb.key_type)
                && types_match(db, ma.value_type, mb.value_type)
        }
        (TypeHint::Set(sa), TypeHint::Set(sb)) => {
            types_match(db, sa.element_type, sb.element_type)
        }
        (TypeHint::AnonTuple(ta), TypeHint::AnonTuple(tb)) => {
            ta.fields.len() == tb.fields.len()
                && ta.fields.iter().zip(tb.fields.iter())
                    .all(|(a, b)| types_match(db, *a, *b))
        }
        (TypeHint::AnonStruct(sa), TypeHint::AnonStruct(sb)) => {
            sa.fields.len() == sb.fields.len()
                && sa.fields.iter().zip(sb.fields.iter())
                    .all(|(fa, fb)| {
                        fa.name.as_str(db) == fb.name.as_str(db)
                            && types_match(db, fa.type_hint, fb.type_hint)
                    })
        }
        (TypeHint::Tensor(ta), TypeHint::Tensor(tb)) => {
            ta.rank == tb.rank && types_match(db, ta.element_type, tb.element_type)
        }
        (TypeHint::Alias(na), TypeHint::Alias(nb)) => {
            na.as_str(db) == nb.as_str(db)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::ast::{Heap, TypeHint, TypeHintAndHeap};
    use datalove_datalit::Database;

    #[test]
    fn test_types_match_same() {
        let db = Database::default();
        test_types_match_same_inner(&db);
    }

    #[salsa::tracked]
    fn test_types_match_same_inner<'db>(db: &'db dyn salsa::Database) {
        let ty1 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let ty2 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        assert!(types_match(db, ty1, ty2), "Same types should match");
    }

    #[test]
    fn test_types_match_different_type() {
        let db = Database::default();
        test_types_match_different_type_inner(&db);
    }

    #[salsa::tracked]
    fn test_types_match_different_type_inner<'db>(db: &'db dyn salsa::Database) {
        let ty1 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let ty2 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        assert!(!types_match(db, ty1, ty2), "Different types should not match");
    }

    #[test]
    fn test_types_match_different_heap() {
        let db = Database::default();
        test_types_match_different_heap_inner(&db);
    }

    #[salsa::tracked]
    fn test_types_match_different_heap_inner<'db>(db: &'db dyn salsa::Database) {
        let ty1 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let ty2 = TypeHintAndHeap::new(db, Heap::Global, TypeHint::U32);
        assert!(!types_match(db, ty1, ty2), "Different heaps should not match");
    }

    #[test]
    fn test_variables_of_type() {
        let db = Database::default();
        test_variables_of_type_inner(&db);
    }

    #[salsa::tracked]
    fn test_variables_of_type_inner<'db>(db: &'db dyn salsa::Database) {
        let ty_u32 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let ty_i32 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        let ty_bool = TypeHintAndHeap::new(db, Heap::Local, TypeHint::Bool);

        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "x".to_string(),
            type_hint: ty_u32,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "y".to_string(),
            type_hint: ty_i32,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "z".to_string(),
            type_hint: ty_u32,
            is_mutable: true,
        });

        // Should find both u32 variables.
        let u32_vars = ctx.variables_of_type(db, ty_u32);
        assert_eq!(u32_vars.len(), 2);
        assert!(u32_vars.iter().any(|v| v.name == "x"));
        assert!(u32_vars.iter().any(|v| v.name == "z"));

        // Should find one i32 variable.
        let i32_vars = ctx.variables_of_type(db, ty_i32);
        assert_eq!(i32_vars.len(), 1);
        assert_eq!(i32_vars[0].name, "y");

        // Should find no bool variables.
        let bool_vars = ctx.variables_of_type(db, ty_bool);
        assert!(bool_vars.is_empty());
    }

    #[test]
    fn test_mutable_variables() {
        let db = Database::default();
        test_mutable_variables_inner(&db);
    }

    #[salsa::tracked]
    fn test_mutable_variables_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);

        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "immut".to_string(),
            type_hint: ty,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "mut1".to_string(),
            type_hint: ty,
            is_mutable: true,
        });
        ctx.variables.push(Variable {
            name: "mut2".to_string(),
            type_hint: ty,
            is_mutable: true,
        });

        let muts = ctx.mutable_variables(db);
        assert_eq!(muts.len(), 2);
        assert!(muts.iter().any(|v| v.name == "mut1"));
        assert!(muts.iter().any(|v| v.name == "mut2"));
    }

    #[test]
    fn test_callable_functions_shadowing() {
        let db = Database::default();
        test_callable_functions_shadowing_inner(&db);
    }

    #[salsa::tracked]
    fn test_callable_functions_shadowing_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);

        let mut ctx = GenContext::new();

        // Local function named "compute".
        ctx.functions.push(FunctionSig {
            name: "compute".to_string(),
            params: vec![],
            return_type: Some(ty),
        });

        // Imported function also named "compute" (should be shadowed).
        ctx.imported_functions.push(FunctionSig {
            name: "compute".to_string(),
            params: vec![("x".to_string(), ty)],
            return_type: Some(ty),
        });

        // Another imported function with different name.
        ctx.imported_functions.push(FunctionSig {
            name: "helper".to_string(),
            params: vec![],
            return_type: None,
        });

        let callables: Vec<_> = ctx.callable_functions().collect();

        // Should have 2 functions: local "compute" and imported "helper".
        assert_eq!(callables.len(), 2);

        // Local "compute" should be included (no params).
        let compute = callables.iter().find(|f| f.name == "compute").unwrap();
        assert!(compute.params.is_empty(), "Should be local compute with no params");

        // Helper should be included.
        assert!(callables.iter().any(|f| f.name == "helper"));
    }

    #[test]
    fn test_module_info_path() {
        let info: ModuleInfo<'_> = ModuleInfo {
            library: "local".to_string(),
            package: "gen".to_string(),
            module: "utils".to_string(),
            functions: vec![],
            type_aliases: vec![],
        };

        assert_eq!(info.path(), "local/gen/utils");
        assert_eq!(info.alias(), "utils");
    }

    #[test]
    fn test_at_max_depth() {
        let config = WorldGenConfig::default();
        let mut ctx = GenContext::new();

        // Initially not at max depth.
        assert!(!ctx.at_max_depth(&config));

        // Go to max depth.
        ctx.control_flow_depth = config.max_control_flow_depth;
        assert!(ctx.at_max_depth(&config));

        // Go past max depth.
        ctx.control_flow_depth = config.max_control_flow_depth + 1;
        assert!(ctx.at_max_depth(&config));
    }
}
