//! Datafun typechecker.
//!
//! Provides type checking for datafun scripts and expressions.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use bct::text::InternedText;
use datalove_datafun_ast::ast::*;
use datalove_datalit;

/// Re-export Db trait for convenience.
pub use salsa::Database as Db;

/// Re-export ModuleId and ModuleGraph from bct for convenience.
pub use bct::module_graph::{ModuleId, ModuleGraph};

/// Re-export DatafunSpans from AST crate.
pub use datalove_datafun_ast::spans::DatafunSpans;

// Implementation modules.
mod api;
mod check;
mod context;
mod statement;
pub mod synthesize;
pub mod types;

/// Type representation for datafun (extends datalit types with function types).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum Type<'db> {
    /// Datalit type (primitives, collections, etc.).
    Datalit(datalove_datalit::tycheck::Type<'db>),
    /// Function type: (param_types) -> return_type.
    Function(TypeFunction<'db>),
}

#[salsa::tracked]
pub struct TypeAndHeap<'db> {
    pub heap: datalove_datalit::ast::Heap,
    #[returns(ref)]
    pub ty: Type<'db>,
}

#[salsa::tracked]
pub struct TypeFunction<'db> {
    pub param_types: Vec<TypeAndHeap<'db>>,
    pub param_modes: Vec<ParamMode>,
    pub return_type: TypeAndHeap<'db>,
}

/// Type error representation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    UnresolvedName(String),
    CannotSynthesize,
    InvalidOperandType { op: String, ty: String },
    InvalidTupleElement { ty: String },
    ArityMismatch { expected: usize, actual: usize },
    NotAFunction(String),
    DatalitError(String),
    ResultRequiresErrorBinding,
    TryOutsideFunction { operator: String },
    TryTypeMismatch { operator: String, actual_type: String },
    TryReturnTypeMismatch { operator: String, return_type: String },
    IntOutOfRange,
    HeapMismatch { expected_heap: String, actual_heap: String },
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    VariantNotFound(String),
    BreakOutsideLoop,
    ContinueOutsideLoop,
}

impl From<datalove_datalit::tycheck::TypeError> for TypeError {
    fn from(err: datalove_datalit::tycheck::TypeError) -> Self {
        match err {
            datalove_datalit::tycheck::TypeError::TypeMismatch { expected, actual } => {
                TypeError::TypeMismatch { expected, actual }
            }
            datalove_datalit::tycheck::TypeError::HeapMismatch { expected_heap, actual_heap } => {
                TypeError::HeapMismatch { expected_heap, actual_heap }
            }
            datalove_datalit::tycheck::TypeError::CannotSynthesize => TypeError::CannotSynthesize,
            datalove_datalit::tycheck::TypeError::MissingField(name) => TypeError::MissingField(name),
            datalove_datalit::tycheck::TypeError::ExtraField(name) => TypeError::ExtraField(name),
            datalove_datalit::tycheck::TypeError::FieldOrderMismatch => TypeError::FieldOrderMismatch,
            datalove_datalit::tycheck::TypeError::IntOutOfRange => TypeError::IntOutOfRange,
            datalove_datalit::tycheck::TypeError::VariantNotFound(name) => TypeError::VariantNotFound(name),
            datalove_datalit::tycheck::TypeError::ArityMismatch { expected, actual } => {
                TypeError::ArityMismatch { expected, actual }
            }
        }
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
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,

    /// Resolved call targets, indexed by ExprFunctionCall ID.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
}

/// Result of typechecking a single expression.
#[salsa::tracked]
pub struct ExprTypecheckResult<'db> {
    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,
    /// Expression types, indexed by ExprFun ID.
    #[returns(ref)]
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
}

/// Kind of script unit for batch typechecking (with parsed content).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum ScriptUnitKind<'db> {
    /// A fragment containing statements.
    Fragment(ParsedStatements<'db>),
    /// A single expression.
    Expr(ExprFun<'db>),
}

/// Spec for a single script unit (with pre-parsed content).
#[salsa::interned]
pub struct ScriptUnitSpec<'db> {
    pub source: bct::input::Source,
    pub spans: DatafunSpans<'db>,
    #[returns(ref)]
    pub kind: ScriptUnitKind<'db>,
}

/// Spec for a module (path + pre-parsed statements + module ID).
#[salsa::interned]
pub struct ModuleSpec<'db> {
    #[returns(ref)]
    pub path: String,
    pub source: bct::input::Source,
    pub spans: DatafunSpans<'db>,
    pub parsed: ParsedStatements<'db>,
    pub module_id: ModuleId,
}

/// Spec for a batch of script units (the "input" to typechecking).
#[salsa::interned]
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
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
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

/// Exported function signatures from a module.
#[salsa::tracked]
pub struct ModuleExports<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Function signatures as a vector of (name, type) pairs.
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,
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
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,

    /// Resolved call targets from all modules, combined.
    ///
    /// Indexed by ExprFunctionCall salsa ID, contains resolved function ASTs
    /// for all function calls across all modules in the graph.
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
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
// Parsed Module Graph
// ============================================================================

/// A module graph paired with pre-parsed statements and spans for each module.
#[salsa::tracked]
pub struct ParsedModuleGraph<'db> {
    /// The underlying module graph.
    pub graph: ModuleGraph,

    /// Pre-parsed statements with spans, as (ModuleId, ParsedStatements, DatafunSpans) tuples.
    /// Order matches graph.iter_modules() order.
    #[returns(ref)]
    pub parsed_statements: Vec<(ModuleId, ParsedStatements<'db>, DatafunSpans<'db>)>,
}

impl<'db> ParsedModuleGraph<'db> {
    /// Get the parsed statements for a module by its ID.
    pub fn get_parsed(&self, db: &'db dyn Db, module_id: ModuleId) -> Option<ParsedStatements<'db>> {
        self.parsed_statements(db).iter()
            .find(|(id, _, _)| *id == module_id)
            .map(|(_, parsed, _)| *parsed)
    }

    /// Get the spans for a module by its ID.
    pub fn get_spans(&self, db: &'db dyn Db, module_id: ModuleId) -> Option<DatafunSpans<'db>> {
        self.parsed_statements(db).iter()
            .find(|(id, _, _)| *id == module_id)
            .map(|(_, _, spans)| *spans)
    }
}

// ============================================================================
// Public API Re-exports
// ============================================================================

// Re-export public API functions.
pub use api::{
    type_check_script_units,
    type_check_single_script,
    type_check_script_with_context,
    type_check_expr_with_context,
    type_check_with_module_graph,
    typecheck_module_graph,
};

// Re-export context types.
pub use context::{
    TypeContext,
    ScriptTypeContext,
    ScriptTypecheckResultRaw,
    ExprTypecheckResultRaw,
    build_function_type_from_stmt,
};

// Re-export statement functions.
pub use statement::collect_function_signature;

// Re-export type utilities.
pub use types::{
    convert_type_hint,
    type_to_string,
    unit_type,
    is_unit_type,
};
