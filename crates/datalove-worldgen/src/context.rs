//! Generation context for tracking scope during worldfile generation.

use datalove_datalit::ast::TypeHintAndHeap;
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
        }
    }

    /// Get all callable functions (local + imported, with shadowing).
    ///
    /// Local functions shadow imported functions with the same name.
    pub fn callable_functions(&self) -> impl Iterator<Item = &FunctionSig<'db>> {
        // Collect local function names for shadowing check.
        let local_names: std::collections::HashSet<_> =
            self.functions.iter().map(|f| f.name.as_str()).collect();

        // Return local functions + non-shadowed imports.
        self.functions.iter().chain(
            self.imported_functions
                .iter()
                .filter(move |f| !local_names.contains(f.name.as_str())),
        )
    }

    /// Find variables of a specific type.
    pub fn variables_of_type(
        &self,
        db: &'db dyn salsa::Database,
        type_hint: TypeHintAndHeap<'db>,
    ) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| types_match(db, v.type_hint, type_hint))
            .collect()
    }

    /// Find mutable variables.
    pub fn mutable_variables(&self) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| v.is_mutable)
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

        let muts = ctx.mutable_variables();
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
