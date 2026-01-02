//! Datafun typechecker.
//!
//! Provides type checking for datafun scripts and expressions.
//! Types are re-exported from the datalove-datafun-tycheck crate.

mod api;
mod check;
mod context;
mod statement;
pub mod synthesize;
pub mod types;

// Re-export all public types from the tycheck crate.
pub use datalove_datafun_tycheck::{
    DatafunSpans,
    Type,
    TypeAndHeap,
    TypeFunction,
    TypeError,
    TypeErrorEntry,
    ResolvedCallTarget,
    TypecheckResult,
    ExprTypecheckResult,
    ScriptUnitKind,
    ScriptUnitSpec,
    ModuleSpec,
    ScriptBatchSpec,
    ScriptUnitInput,
    ModuleInfo,
    ScriptUnitBatch,
    UnitTypecheckResultTracked,
    ScriptUnitsTypecheckResultTracked,
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    ModuleId,
    ParsedModuleGraph,
};

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
