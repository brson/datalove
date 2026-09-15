//! Generation context for tracking scope during worldfile generation.

use std::collections::HashSet;
use datalove_datalit::ast::TypeHint;
use crate::config::WorldGenConfig;

/// A variable in scope.
#[derive(Clone)]
pub struct Variable<'db> {
    pub name: String,
    pub type_hint: TypeHint<'db>,
    pub is_mutable: bool,
}

/// How a parameter is passed, which decides what may be done with it.
///
/// Every argument repeats its parameter's mode at the call, so a call can only
/// be written where the caller has something the mode will take: a `mut` or an
/// `out` wants a `var`, and neither will take an expression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamMode {
    /// `x: T`. The caller hands the value over and the callee consumes it.
    In,
    /// `ref x: T`. The callee reads it and the caller keeps it.
    Ref,
    /// `mut x: T`. The callee may write it and the caller keeps it.
    Mut,
    /// `out x: T`. The callee must write it before it returns.
    Out,
}

impl ParamMode {
    /// How the mode is written, in a signature or before an argument.
    pub fn marker(&self) -> &'static str {
        match self {
            ParamMode::In => "",
            ParamMode::Ref => "ref ",
            ParamMode::Mut => "mut ",
            ParamMode::Out => "out ",
        }
    }

    /// Whether the caller has to hand over a `var` rather than a value.
    pub fn wants_a_mutable_binding(&self) -> bool {
        matches!(self, ParamMode::Mut | ParamMode::Out)
    }
}

/// One parameter of a generated function.
#[derive(Clone)]
pub struct Param<'db> {
    pub name: String,
    pub type_hint: TypeHint<'db>,
    pub mode: ParamMode,
}

/// A function signature.
#[derive(Clone)]
pub struct FunctionSig<'db> {
    pub name: String,
    pub params: Vec<Param<'db>>,
    pub return_type: Option<TypeHint<'db>>,
}

/// One variant of a generated enum: an atom, or a term over a payload.
#[derive(Clone)]
pub struct EnumVariant<'db> {
    pub name: String,
    pub payload: Option<TypeHint<'db>>,
}

/// An enum a module defines, declared as a type alias so it has a name.
///
/// A `match` names the variants of the type it takes apart, so the type has to
/// be one the generator can name, which a structural enum written inline is
/// not.
#[derive(Clone)]
pub struct EnumDef<'db> {
    pub name: String,
    pub variants: Vec<EnumVariant<'db>>,
}

/// A const a module or a body defines.
///
/// Kept apart from the variables because reading one does not consume it: a
/// const names a value rather than a place, so a linear one can be named as
/// often as it is wanted. Nothing else the generator writes can be.
#[derive(Clone)]
pub struct ConstDef<'db> {
    pub name: String,
    pub type_hint: TypeHint<'db>,
}

/// A type alias.
#[derive(Clone)]
pub struct TypeAlias<'db> {
    pub name: String,
    pub type_hint: TypeHint<'db>,
}

/// A module in the worldfile.
#[derive(Clone)]
pub struct ModuleInfo<'db> {
    pub library: String,
    pub package: String,
    pub module: String,
    pub functions: Vec<FunctionSig<'db>>,
    pub type_aliases: Vec<TypeAlias<'db>>,
    /// The enums it defines, each declared as a named type alias.
    pub enums: Vec<EnumDef<'db>>,
    /// The consts it defines, in scope for every function in it.
    pub consts: Vec<ConstDef<'db>>,
    /// The generic functions it defines, which are called by picking types
    /// rather than by matching a return type. See `gen_generic`.
    pub generics: Vec<crate::gen_generic::GenericSig>,
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

    /// Enums in reach, which are what a `match` can be written over.
    pub enums: Vec<EnumDef<'db>>,

    /// Consts in reach, which may be named as often as wanted.
    pub consts: Vec<ConstDef<'db>>,

    /// Imported functions from other modules.
    pub imported_functions: Vec<FunctionSig<'db>>,

    /// Expected return type for current function.
    pub return_type: Option<TypeHint<'db>>,

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

    /// The generic functions in reach, which a call has to pick concrete
    /// types for. Kept apart from `functions` because they are not called the
    /// same way: a generic's answer depends on what the call binds its type
    /// parameters to, so the caller chooses first and names the result.
    pub generic_functions: Vec<crate::gen_generic::GenericSig>,

    /// Variables that have been consumed (moved) and can't be used again.
    /// This is used for ownership tracking - global heap types get moved on first use.
    pub consumed_variables: HashSet<String>,

    /// Variables that are protected from moving inside a loop.
    /// These can still be borrowed (used in operators) but not moved.
    pub loop_protected_variables: HashSet<String>,
}

impl<'db> GenContext<'db> {
    pub fn new() -> Self {
        GenContext {
            variables: Vec::new(),
            functions: Vec::new(),
            type_aliases: Vec::new(),
            enums: Vec::new(),
            consts: Vec::new(),
            imported_functions: Vec::new(),
            return_type: None,
            loop_depth: 0,
            control_flow_depth: 0,
            current_function_name: None,
            max_callable_function_index: None,
            generic_functions: Vec::new(),
            consumed_variables: HashSet::new(),
            loop_protected_variables: HashSet::new(),
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

    /// Mark a variable as loop-protected (can be borrowed but not moved).
    pub fn loop_protect_variable(&mut self, name: &str) {
        self.loop_protected_variables.insert(name.to_string());
    }

    /// Check if a variable is loop-protected.
    pub fn is_loop_protected(&self, name: &str) -> bool {
        self.loop_protected_variables.contains(name)
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

        // Collect local function names for the shadowing check. Every local
        // name, not just the callable ones: a local function further down the
        // module is not callable from here, but it still stands in front of an
        // import of the same name, and writing that name would reach it.
        let local_names: std::collections::HashSet<_> =
            self.functions.iter().map(|f| f.name.as_str()).collect();

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
    /// Find variables of a specific type that can be moved.
    ///
    /// Excludes consumed variables (already moved) and loop-protected variables
    /// (can only be borrowed inside a loop, not moved).
    pub fn variables_of_type(
        &self,
        db: &'db dyn salsa::Database,
        type_hint: TypeHint<'db>,
    ) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| {
                if !types_match(db, v.type_hint.clone(), type_hint.clone()) {
                    return false;
                }
                // Filter out consumed variables.
                if self.is_consumed(&v.name) {
                    return false;
                }
                // Filter out loop-protected variables (can only borrow, not move).
                if self.is_loop_protected(&v.name) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Find variables of a specific type that can be borrowed (used in operators).
    ///
    /// Excludes consumed variables but allows loop-protected variables since
    /// operators use ref semantics (borrow, not move).
    pub fn variables_of_type_for_borrow(
        &self,
        db: &'db dyn salsa::Database,
        type_hint: TypeHint<'db>,
    ) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| {
                if !types_match(db, v.type_hint.clone(), type_hint.clone()) {
                    return false;
                }
                // Filter out consumed variables.
                if self.is_consumed(&v.name) {
                    return false;
                }
                // Loop-protected is OK for borrowing.
                true
            })
            .collect()
    }

    /// Find mutable variables that are not consumed.
    pub fn mutable_variables(&self) -> Vec<&Variable<'db>> {
        self.variables
            .iter()
            .filter(|v| {
                if !v.is_mutable {
                    return false;
                }
                // Filter out consumed variables.
                if self.is_consumed(&v.name) {
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

/// Check if a type is linear (has move semantics).
///
/// Copy types: bool, u8-u64, i8-i64, usize, isize, f32, f64
/// Linear types: int, string, list, map, set, tensor, table, data, error, etc.
pub fn is_linear_type(type_hint: &TypeHint<'_>) -> bool {
    match type_hint {
        // Copy types.
        TypeHint::Bool
        | TypeHint::U8
        | TypeHint::U16
        | TypeHint::U32
        | TypeHint::U64
        | TypeHint::I8
        | TypeHint::I16
        | TypeHint::I32
        | TypeHint::I64
        | TypeHint::Index
        | TypeHint::Offset
        | TypeHint::F32
        | TypeHint::F64 => false,

        // Linear types.
        TypeHint::Int
        | TypeHint::String
        | TypeHint::Data
        | TypeHint::Error
        | TypeHint::List(_)
        | TypeHint::Map(_)
        | TypeHint::Set(_)
        | TypeHint::Tensor(_)
        | TypeHint::Table(_)
        | TypeHint::Option(_)
        | TypeHint::Result(_)
        | TypeHint::AnonTuple(_)
        | TypeHint::AnonStruct(_)

        | TypeHint::Alias(_)
        | TypeHint::ParseError(_)
        | TypeHint::Atom(_)
        | TypeHint::Term(_)
        | TypeHint::Enum(_) => true,
    }
}

/// Check if two TypeHint values match (same type structure).
pub fn types_match<'db>(
    db: &'db dyn salsa::Database,
    a: TypeHint<'db>,
    b: TypeHint<'db>,
) -> bool {
    match (&a, &b) {
        (TypeHint::Bool, TypeHint::Bool) => true,
        (TypeHint::U8, TypeHint::U8) => true,
        (TypeHint::I8, TypeHint::I8) => true,
        (TypeHint::U16, TypeHint::U16) => true,
        (TypeHint::I16, TypeHint::I16) => true,
        (TypeHint::U32, TypeHint::U32) => true,
        (TypeHint::I32, TypeHint::I32) => true,
        (TypeHint::U64, TypeHint::U64) => true,
        (TypeHint::I64, TypeHint::I64) => true,
        (TypeHint::Index, TypeHint::Index) => true,
        (TypeHint::Offset, TypeHint::Offset) => true,
        (TypeHint::F32, TypeHint::F32) => true,
        (TypeHint::F64, TypeHint::F64) => true,
        (TypeHint::Int, TypeHint::Int) => true,
        (TypeHint::String, TypeHint::String) => true,
        (TypeHint::Data, TypeHint::Data) => true,
        (TypeHint::Error, TypeHint::Error) => true,
        (TypeHint::List(la), TypeHint::List(lb)) => {
            types_match(db, (*la.element_type).clone(), (*lb.element_type).clone())
        }
        (TypeHint::Option(oa), TypeHint::Option(ob)) => {
            types_match(db, (*oa.inner_type).clone(), (*ob.inner_type).clone())
        }
        (TypeHint::Result(ra), TypeHint::Result(rb)) => {
            types_match(db, (*ra.inner_type).clone(), (*rb.inner_type).clone())
        }
        (TypeHint::Map(ma), TypeHint::Map(mb)) => {
            types_match(db, (*ma.key_type).clone(), (*mb.key_type).clone())
                && types_match(db, (*ma.value_type).clone(), (*mb.value_type).clone())
        }
        (TypeHint::Set(sa), TypeHint::Set(sb)) => {
            types_match(db, (*sa.element_type).clone(), (*sb.element_type).clone())
        }
        (TypeHint::AnonTuple(ta), TypeHint::AnonTuple(tb)) => {
            ta.fields.len() == tb.fields.len()
                && ta.fields.iter().zip(tb.fields.iter())
                    .all(|(a, b)| types_match(db, a.clone(), b.clone()))
        }
        (TypeHint::AnonStruct(sa), TypeHint::AnonStruct(sb)) => {
            sa.fields.len() == sb.fields.len()
                && sa.fields.iter().zip(sb.fields.iter())
                    .all(|(fa, fb)| {
                        fa.name.as_str(db) == fb.name.as_str(db)
                            && types_match(db, (*fa.type_hint).clone(), (*fb.type_hint).clone())
                    })
        }
        (TypeHint::Tensor(ta), TypeHint::Tensor(tb)) => {
            ta.rank == tb.rank && types_match(db, (*ta.element_type).clone(), (*tb.element_type).clone())
        }
        (TypeHint::Alias(na), TypeHint::Alias(nb)) => {
            na.name.as_str(db) == nb.name.as_str(db)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::Database;

    #[test]
    fn test_types_match_same() {
        let db = Database::default();
        test_types_match_same_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_types_match_same_inner<'db>(db: &'db dyn salsa::Database) {
        assert!(types_match(db, TypeHint::U32, TypeHint::U32), "Same types should match");
    }

    #[test]
    fn test_types_match_different_type() {
        let db = Database::default();
        test_types_match_different_type_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_types_match_different_type_inner<'db>(db: &'db dyn salsa::Database) {
        assert!(!types_match(db, TypeHint::U32, TypeHint::I32), "Different types should not match");
    }

    #[test]
    fn test_variables_of_type() {
        let db = Database::default();
        test_variables_of_type_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_variables_of_type_inner<'db>(db: &'db dyn salsa::Database) {
        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "x".to_string(),
            type_hint: TypeHint::U32,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "y".to_string(),
            type_hint: TypeHint::I32,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "z".to_string(),
            type_hint: TypeHint::U32,
            is_mutable: true,
        });

        // Should find both u32 variables.
        let u32_vars = ctx.variables_of_type(db, TypeHint::U32);
        assert_eq!(u32_vars.len(), 2);
        assert!(u32_vars.iter().any(|v| v.name == "x"));
        assert!(u32_vars.iter().any(|v| v.name == "z"));

        // Should find one i32 variable.
        let i32_vars = ctx.variables_of_type(db, TypeHint::I32);
        assert_eq!(i32_vars.len(), 1);
        assert_eq!(i32_vars[0].name, "y");

        // Should find no bool variables.
        let bool_vars = ctx.variables_of_type(db, TypeHint::Bool);
        assert!(bool_vars.is_empty());
    }

    #[test]
    fn test_mutable_variables() {
        let db = Database::default();
        test_mutable_variables_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_mutable_variables_inner<'db>(db: &'db dyn salsa::Database) {
        let _ = db;
        let mut ctx = GenContext::new();
        ctx.variables.push(Variable {
            name: "immut".to_string(),
            type_hint: TypeHint::U32,
            is_mutable: false,
        });
        ctx.variables.push(Variable {
            name: "mut1".to_string(),
            type_hint: TypeHint::U32,
            is_mutable: true,
        });
        ctx.variables.push(Variable {
            name: "mut2".to_string(),
            type_hint: TypeHint::U32,
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

    #[salsa::tracked(returns(copy))]
    fn test_callable_functions_shadowing_inner<'db>(db: &'db dyn salsa::Database) {
        let _ = db;
        let mut ctx = GenContext::new();

        // Local function named "compute".
        ctx.functions.push(FunctionSig {
            name: "compute".to_string(),
            params: vec![],
            return_type: Some(TypeHint::U32),
        });

        // Imported function also named "compute" (should be shadowed).
        ctx.imported_functions.push(FunctionSig {
            name: "compute".to_string(),
            params: vec![Param { name: "x".to_string(), type_hint: TypeHint::U32, mode: ParamMode::In }],
            return_type: Some(TypeHint::U32),
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
            enums: vec![],
            consts: vec![],
            generics: vec![],
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
