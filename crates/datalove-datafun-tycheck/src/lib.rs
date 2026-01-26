//! Datafun typechecker.
//!
//! Provides type checking for datafun scripts and expressions.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use bct::text::InternedText;
use datalove_datafun_ast::ast::*;
use datalove_datalit;

use salsa::Database as Db;
use bct::module_graph::ModuleId;
use datalove_datafun_ast::spans::DatafunSpans;

// Re-export shared types from ast crate.
pub use datalove_datafun_ast::types::{
    Type,
    TypeFunction,
    TypeError,
    DbClone,
    ParallelMode,
    parallel_mode_from_env,
    ParsedModuleGraph,
    ModuleNameResolution,
    AllModuleNameResolutions,
    CollectedNames,
    AllModuleExports,
    AllModuleFunctionAsts,
};

// Implementation modules.
mod api;
mod check;
mod context;
mod emit;
mod statement;
pub mod synthesize;
pub mod types;

// Re-export emit infrastructure.
pub use emit::{SpanLookup, LocalSpanLookup, ModuleGraphSpanLookup, emit_pending_diagnostics, format_pending_diagnostics};

/// Pending diagnostic for unified diagnostic emission.
///
/// All type errors are collected as pending diagnostics during typechecking,
/// then emitted at the end with span information. For module graph typechecking,
/// module_id is Some and spans are looked up from ParsedModuleGraph. For
/// non-module-graph paths, module_id is None and spans are looked up from
/// TypeContext.spans.
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum PendingDiagnostic<'db> {
    /// F001: Undefined variable.
    UndefinedVariable {
        expr_id: u32,
        module_id: Option<ModuleId>,
        name: InternedText<'db>,
    },
    /// F002: Undefined function.
    UndefinedFunction {
        expr_id: u32,
        module_id: Option<ModuleId>,
        name: InternedText<'db>,
    },
    /// F011: Cannot synthesize type.
    CannotSynthesize {
        expr_id: u32,
        module_id: Option<ModuleId>,
        message: InternedText<'db>,
    },
    /// F016: Type mismatch.
    TypeMismatch {
        expr_id: u32,
        module_id: Option<ModuleId>,
        expected: InternedText<'db>,
        actual: InternedText<'db>,
        label: InternedText<'db>,
    },
    /// F026: Invalid operand type.
    InvalidOperandType {
        expr_id: u32,
        module_id: Option<ModuleId>,
        op: InternedText<'db>,
        ty: InternedText<'db>,
    },
    /// F045: Function arity mismatch.
    ArityMismatch {
        /// Salsa ID of the call expression (for primary span lookup).
        call_expr_id: u32,
        /// Module where the call occurs (None for non-module-graph paths).
        call_module_id: Option<ModuleId>,
        /// Name of the function being called.
        func_name: InternedText<'db>,
        /// local_index of the function definition (for secondary span lookup).
        func_local_index: u32,
        /// Module where the function is defined (None for script-local functions).
        func_module_id: Option<ModuleId>,
        /// Expected number of arguments.
        expected: usize,
        /// Actual number of arguments supplied.
        actual: usize,
    },
    /// F046: Result destructuring requires error binding.
    ResultRequiresBinding {
        expr_id: u32,
        module_id: Option<ModuleId>,
    },
    /// F048: Try operator type mismatch.
    TryTypeMismatch {
        expr_id: u32,
        module_id: Option<ModuleId>,
        operator: InternedText<'db>,
        expected: InternedText<'db>,
        actual: InternedText<'db>,
    },
    /// F049: Try operator return type mismatch.
    TryReturnTypeMismatch {
        expr_id: u32,
        module_id: Option<ModuleId>,
        operator: InternedText<'db>,
        expected: InternedText<'db>,
        actual: InternedText<'db>,
    },
    /// F050: Break outside loop.
    BreakOutsideLoop {
        local_index: u32,
        module_id: Option<ModuleId>,
    },
    /// F051: Continue outside loop.
    ContinueOutsideLoop {
        local_index: u32,
        module_id: Option<ModuleId>,
    },
    /// F052: Void function returns value.
    VoidFunctionReturnsValue {
        local_index: u32,
        module_id: Option<ModuleId>,
    },
    /// F053: Non-void function requires return value.
    FunctionRequiresReturnValue {
        local_index: u32,
        module_id: Option<ModuleId>,
    },
    /// F054: Undefined variable in set statement.
    UndefinedVariableSet {
        local_index: u32,
        module_id: Option<ModuleId>,
        name: InternedText<'db>,
    },
    /// F055: Cannot assign to immutable variable.
    VariableNotMutable {
        local_index: u32,
        module_id: Option<ModuleId>,
        name: InternedText<'db>,
    },
}

/// Check if a datalit type is a copy type (can be safely copied without cloning).
///
/// Copy types are:
/// - All fixed-size numeric types (bool, u8, i8, u16, i16, u32, i32, u64, i64, f32, f64)
/// - Tuples/structs where all fields are copy types
///
/// Move types (require ownership transfer):
/// - Int (bigint with heap-allocated limbs)
/// - String, List, Map, Set, Tensor (heap-allocated collections)
/// - Option/Result containing move types
/// - Data, Error
pub fn is_copy_type<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::tycheck::Type<'db>) -> bool {
    use datalove_datalit::tycheck::Type as DatalitType;
    match ty {
        // Fixed-size scalars are copy.
        DatalitType::Bool
        | DatalitType::U8 | DatalitType::I8
        | DatalitType::U16 | DatalitType::I16
        | DatalitType::U32 | DatalitType::I32
        | DatalitType::U64 | DatalitType::I64
        | DatalitType::Usize | DatalitType::Isize
        | DatalitType::F32 | DatalitType::F64 => true,

        // Bigint is move (heap-allocated).
        DatalitType::Int => false,

        // Collections are move (heap-allocated).
        DatalitType::String
        | DatalitType::List(_)
        | DatalitType::Map(_)
        | DatalitType::Set(_)
        | DatalitType::Tensor(_) => false,

        // Data and Error are move.
        DatalitType::Data | DatalitType::Error => false,

        // Aggregates are copy if all fields are copy.
        DatalitType::AnonTuple(tuple) => {
            tuple.fields.iter().all(|f| is_copy_type(db, f))
        }
        DatalitType::AnonStruct(struct_ty) => {
            struct_ty.fields.iter().all(|f| is_copy_type(db, &f.ty))
        }
        DatalitType::AnonEnum(enum_ty) => {
            enum_ty.variants.iter().all(|v| {
                v.payload.as_ref().map_or(true, |p| is_copy_type(db, p))
            })
        }

        // Option/Result are copy if inner type is copy.
        DatalitType::Option(opt) => is_copy_type(db, &opt.inner_type),
        DatalitType::Result(res) => is_copy_type(db, &res.inner_type),

        // Tables are not copy (they contain heap-allocated data).
        DatalitType::Table(_) => false,
    }
}

/// Type error entry with location info.
#[salsa::tracked]
pub struct TypeErrorEntry<'db> {
    pub error: TypeError,
}

/// Resolved call target from typechecking.
///
/// Stores the resolved function AST and source module for a function call,
/// eliminating the need for runtime name lookup.
#[salsa::tracked]
pub struct ResolvedCallTarget<'db> {
    /// The resolved function AST.
    pub func: StmtFun<'db>,
    /// The source module (None for script-local functions).
    pub module_id: Option<ModuleId>,
}

/// Result of typechecking a script.
#[salsa::tracked]
pub struct TypecheckResult<'db> {
    /// The root parsed statements.
    pub root_parsed: ParsedStatements<'db>,

    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,

    /// Expression types, indexed by ExprFun ID.
    #[returns(ref)]
    pub expr_types: Vec<Option<Type<'db>>>,

    /// Resolved call targets, indexed by ExprFunctionCall ID.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,

    /// Function signatures defined in this script.
    ///
    /// Contains resolved function types needed for lowering when type aliases
    /// are used in function parameters.
    #[returns(ref)]
    pub function_types: Vec<(InternedText<'db>, TypeFunction<'db>)>,
}

/// Result of typechecking a single expression.
#[salsa::tracked]
pub struct ExprTypecheckResult<'db> {
    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,
    /// Expression types, indexed by ExprFun ID.
    #[returns(ref)]
    pub expr_types: Vec<Option<Type<'db>>>,
}

/// Kind of script unit for batch typechecking (with parsed content).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum ScriptUnitKind<'db> {
    /// A fragment containing statements and pre-computed name resolution.
    Fragment(ParsedStatements<'db>, CollectedNames<'db>),
    /// A single expression.
    Expr(ExprFun<'db>),
}

/// Spec for a single script unit (with pre-parsed content).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ScriptUnitSpec<'db> {
    pub source: bct::input::Source,
    pub spans: DatafunSpans,
    pub kind: ScriptUnitKind<'db>,
}

impl<'db> ScriptUnitSpec<'db> {
    /// Create new ScriptUnitSpec.
    pub fn new(source: bct::input::Source, spans: DatafunSpans, kind: ScriptUnitKind<'db>) -> Self {
        Self { source, spans, kind }
    }
}

/// Spec for a module (path + pre-parsed statements + module ID + name resolution).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct ModuleSpec<'db> {
    pub path: String,
    pub source: bct::input::Source,
    pub spans: DatafunSpans,
    pub parsed: ParsedStatements<'db>,
    pub module_id: ModuleId,
    /// Pre-computed name resolution for this module.
    pub name_resolution: CollectedNames<'db>,
}

impl<'db> ModuleSpec<'db> {
    /// Create new ModuleSpec.
    pub fn new(
        path: String,
        source: bct::input::Source,
        spans: DatafunSpans,
        parsed: ParsedStatements<'db>,
        module_id: ModuleId,
        name_resolution: CollectedNames<'db>,
    ) -> Self {
        Self { path, source, spans, parsed, module_id, name_resolution }
    }
}

/// Spec for a batch of script units.
///
/// Tracked type - must be created inside a tracked function.
#[salsa::tracked]
pub struct ScriptBatchSpec<'db> {
    #[returns(ref)]
    pub units: Vec<ScriptUnitSpec<'db>>,
    #[returns(ref)]
    pub modules: Vec<ModuleSpec<'db>>,
}

/// A script unit with parsed content (tracked - created inside tracked fn).
#[salsa::tracked]
pub struct ScriptUnitInput<'db> {
    pub source: bct::input::Source,
    #[returns(ref)]
    pub kind: ScriptUnitKind<'db>,
}

/// Module info with parsed content (tracked).
#[salsa::tracked]
pub struct ModuleInfo<'db> {
    #[returns(ref)]
    pub path: String,
    pub parsed: ParsedStatements<'db>,
    pub source: bct::input::Source,
    pub module_id: ModuleId,
}

/// Batch of script units (tracked).
#[salsa::tracked]
pub struct ScriptUnitBatch<'db> {
    #[returns(ref)]
    pub units: Vec<ScriptUnitInput<'db>>,
    #[returns(ref)]
    pub modules: Vec<ModuleInfo<'db>>,
}

/// Result of typechecking one script unit.
#[salsa::tracked]
pub struct UnitTypecheckResultTracked<'db> {
    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,
    /// Expression types, indexed by ExprFun ID.
    #[returns(ref)]
    pub expr_types: Vec<Option<Type<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
    /// Function signatures defined in this script unit.
    ///
    /// Contains resolved function types needed for lowering when type aliases
    /// are used in function parameters.
    #[returns(ref)]
    pub function_types: Vec<(InternedText<'db>, TypeFunction<'db>)>,
}

/// Result of typechecking multiple script units together.
#[salsa::tracked]
pub struct ScriptUnitsTypecheckResultTracked<'db> {
    /// Per-unit results.
    pub results: Vec<UnitTypecheckResultTracked<'db>>,
}

// ============================================================================
// Module Graph Typecheck Result Types
// ============================================================================

/// A resolved import for a module.
///
/// Contains all information needed to add an imported function to the type context.
#[salsa::tracked]
pub struct ResolvedImport<'db> {
    /// Local name for the imported function.
    pub local_name: InternedText<'db>,
    /// Type of the imported function.
    pub func_type: TypeFunction<'db>,
    /// AST of the imported function (for inlining).
    pub func_ast: Option<StmtFun<'db>>,
    /// Module the function was imported from.
    pub source_module: ModuleId,
    /// Original name in the source module.
    pub source_name: InternedText<'db>,
}

/// Result of resolving imports for a single module (memoized per module).
#[salsa::tracked]
pub struct ModuleImportResolution<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Resolved imports: (local_name, func_type, func_ast, source_module_id).
    #[returns(ref)]
    pub imports: Vec<(InternedText<'db>, TypeFunction<'db>, Option<StmtFun<'db>>, ModuleId)>,

    /// Import resolution errors.
    #[returns(ref)]
    pub errors: Vec<TypeError>,
}

/// Result of typechecking a single module.
#[salsa::tracked]
pub struct SingleModuleTypecheckResult<'db> {
    /// Module that was typechecked.
    pub module_id: ModuleId,

    /// Type errors encountered.
    #[returns(ref)]
    pub errors: Vec<TypeError>,

    /// Pending diagnostics for post-hoc span enrichment.
    ///
    /// These are collected when spans are not available during typechecking
    /// (module graph path) and can be emitted later with full span information.
    #[returns(ref)]
    pub pending_diagnostics: Vec<PendingDiagnostic<'db>>,

    /// Exported function signatures.
    #[returns(ref)]
    pub exports: Vec<(InternedText<'db>, TypeFunction<'db>)>,

    /// Exported type aliases.
    #[returns(ref)]
    pub exported_type_aliases: Vec<(InternedText<'db>, Type<'db>)>,

    /// Imported functions: (local_name, source_module_id, source_name).
    #[returns(ref)]
    pub imports: Vec<(InternedText<'db>, ModuleId, InternedText<'db>)>,

    /// Expression types for this module.
    #[returns(ref)]
    pub expr_types: Vec<Option<Type<'db>>>,

    /// Resolved call targets for this module.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
}

/// Exported function signatures from a module.
#[salsa::tracked]
pub struct ModuleExports<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Function signatures as a vector of (name, type) pairs.
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,

    /// Type aliases as a vector of (name, type) pairs.
    #[returns(ref)]
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
}

/// Imported functions for a module.
#[salsa::tracked]
pub struct ModuleImports<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Imported functions: (local_name, source_module_id, source_name).
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, ModuleId, InternedText<'db>)>,
}

/// Result of typechecking a module graph.
#[salsa::tracked]
pub struct ModuleGraphTypecheckResult<'db> {
    /// The module graph that was typechecked.
    pub graph: bct::module_graph::ModuleGraph,

    /// Type errors encountered, per module.
    #[returns(ref)]
    pub module_errors: BTreeMap<ModuleId, Vec<TypeError>>,

    /// Module exports, per module.
    #[returns(ref)]
    pub module_exports: BTreeMap<ModuleId, ModuleExports<'db>>,

    /// Module imports, per module.
    #[returns(ref)]
    pub module_imports: BTreeMap<ModuleId, ModuleImports<'db>>,

    /// Expression types from all modules, combined.
    ///
    /// Indexed by ExprFun salsa ID, contains types for all expressions
    /// across all modules in the graph.
    #[returns(ref)]
    pub expr_types: Vec<Option<Type<'db>>>,

    /// Resolved call targets from all modules, combined.
    ///
    /// Indexed by ExprFunctionCall salsa ID, contains resolved function ASTs
    /// for all function calls across all modules in the graph.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,

    /// Per-module typecheck results (tracked structs with stable salsa IDs).
    ///
    /// Used by downstream phases (e.g., IR lowering) to get per-module type
    /// information while preserving memoization.
    #[returns(ref)]
    pub module_results: BTreeMap<ModuleId, SingleModuleTypecheckResult<'db>>,
}

impl<'db> ModuleGraphTypecheckResult<'db> {
    /// Check if typechecking succeeded (no errors).
    pub fn is_ok(&self, db: &'db dyn Db) -> bool {
        self.module_errors(db).values().all(|errors| errors.is_empty())
    }

    /// Get all errors across all modules.
    pub fn all_errors(&self, db: &'db dyn Db) -> Vec<&TypeError> {
        self.module_errors(db).values().flatten().collect()
    }
}

// ============================================================================
// Public API Re-exports
// ============================================================================

// Re-export public API functions.
pub use api::{
    create_batch_spec,
    type_check_script_units,
    type_check_single_script,
    typecheck_module_graph,
    typecheck_module_graph_parallel,
    typecheck_module_graph_with_mode,
    resolve_module_imports,
    // Per-unit memoization types.
    AccumulatedBindings,
    ScriptUnitTypecheckOutput,
    typecheck_script_unit,
};

// Re-export context types.
pub use context::TypeContext;

// Re-export statement functions.
pub use statement::{collect_function_signature, collect_type_aliases};

// Re-export type utilities.
pub use types::{
    convert_type_hint,
    convert_type_hint_with_aliases,
    is_primitive_name,
    type_to_string,
    unit_type,
};
