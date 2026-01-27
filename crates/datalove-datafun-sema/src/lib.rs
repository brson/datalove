//! Semantic analysis types shared between ownership analysis and lowering.
//!
//! This crate provides core types used by both:
//! - `datalove-datafun-ownership`: Ownership and liveness analysis
//! - Lowering (in `datalove-datafun-compiler`): IR generation
//!
//! By extracting these types, lowering can eventually be moved to its own crate
//! without depending on the full ownership analysis module.

use rmx::std::collections::BTreeMap;
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast::{Statement, StmtFun, ParamMode};
use datalove_datafun_ir::IrType;

// ============================================================================
// Resolved call target
// ============================================================================

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

// ============================================================================
// Statement identity for debug verification
// ============================================================================

/// Key identifying a statement for debug verification.
///
/// Used to verify that lowering processes statements in the same order as
/// ownership analysis. This catches bugs where statement IDs get out of sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StmtKey {
    /// Discriminant of the Statement enum.
    pub kind: u8,
    /// local_index if the statement type has one, for additional discrimination.
    pub local_index: Option<u32>,
}

impl StmtKey {
    /// Extract a key from a statement.
    pub fn from_stmt(db: &dyn salsa::Database, stmt: &Statement<'_>) -> Self {
        let (kind, local_index) = match stmt {
            Statement::Let(_) => (0, None),
            Statement::Var(_) => (1, None),
            Statement::Set(s) => (2, Some(s.local_index)),
            Statement::Fun(s) => (3, Some(s.local_index(db))),
            Statement::Ret(s) => (4, Some(s.local_index)),
            Statement::Require(_) => (5, None),
            Statement::Import(_) => (6, None),
            Statement::If(_) => (7, None),
            Statement::Loop(_) => (8, None),
            Statement::Break(s) => (9, Some(s.local_index)),
            Statement::Continue(s) => (10, Some(s.local_index)),
            Statement::DebugLog(_) => (11, None),
            Statement::TypeAlias(s) => (12, Some(s.local_index)),
            Statement::ParseError(_) => (13, None),
            Statement::Const(_) => (14, None),
        };
        Self { kind, local_index }
    }

    /// Get a human-readable name for the statement kind.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "Let",
            1 => "Var",
            2 => "Set",
            3 => "Fun",
            4 => "Ret",
            5 => "Require",
            6 => "Import",
            7 => "If",
            8 => "Loop",
            9 => "Break",
            10 => "Continue",
            11 => "DebugLog",
            12 => "TypeAlias",
            13 => "ParseError",
            14 => "Const",
            _ => "Unknown",
        }
    }
}

// ============================================================================
// Core types
// ============================================================================

/// Identifies a binding (parameter or let/var).
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct BindingId(pub u32);

/// Tracking category for a binding.
///
/// Determines whether the binding needs runtime tracking for moves/drops.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum TrackingCategory {
    /// Copy type - no tracking needed, no drops.
    Copy,
    /// Precise ownership - state is statically known at every program point.
    /// Uses precise move/drop instructions (no runtime checks).
    Precise,
    /// Tracked ownership - state may vary at runtime.
    /// Uses tracked move/drop instructions (with runtime checks).
    /// Examples: exports, out params, conditional moves, mutable slots.
    Tracked,
}

/// Information about a binding.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct BindingInfo {
    /// Name of the binding (for debugging).
    pub name: String,
    /// Type of the binding.
    pub ty: IrType,
    /// Whether this binding is a slot (var) vs value (let/param).
    pub is_slot: bool,
    /// Whether this binding is a ScriptUnit top-level binding (exported, never dropped).
    pub is_script_unit: bool,
    /// Parameter mode if this binding is a param (None for let/var).
    pub param_mode: Option<ParamMode>,
}

impl BindingInfo {
    /// True if this binding is borrowed (Ref, Mut, or Out param) and cannot transfer ownership.
    ///
    /// Out params are borrowed because the caller owns the slot and retains ownership
    /// after the call returns.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.param_mode, Some(ParamMode::Ref) | Some(ParamMode::Mut) | Some(ParamMode::Out))
    }
}

// ============================================================================
// Error types
// ============================================================================

/// Error detected during ownership analysis.
///
/// Each variant includes a `local_index` for span lookup during diagnostic
/// emission. The local_index is the expression's sequential index within its
/// function, stable regardless of parallel vs sequential compilation order.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum AnalysisError {
    /// Using a value after it was moved.
    /// D001
    UseAfterMove {
        local_index: u32,
        name: String,
    },
    /// Moving a value multiple times.
    /// D002
    DoubleMove {
        local_index: u32,
        name: String,
    },
    /// Attempting to move a borrowed value (ref/mut/out parameter).
    /// D003
    CannotMoveBorrowed {
        local_index: u32,
        name: String,
    },
    /// Attempting to pass a ref param to a mut param.
    /// D004
    CannotMutFromRef {
        local_index: u32,
        name: String,
    },
    /// Reading binding before it was initialized.
    /// D005. Applies to Out params and uninitialized var bindings.
    ReadUninitialized {
        local_index: u32,
        name: String,
    },
    /// Function returns without initializing Out param.
    /// D006
    OutParamNotInitialized {
        /// Statement index of return, or None for implicit return.
        ret_stmt_idx: Option<usize>,
        name: String,
    },
    /// Moving an outer-scoped value inside a loop body.
    /// D007
    MoveInLoop {
        /// Expression where the move occurred.
        local_index: u32,
        name: String,
    },
    /// Value moved in one branch but not another.
    /// D008
    InconsistentBranchMove {
        /// Statement index of the if statement.
        stmt_idx: usize,
        name: String,
        /// Which branch has the move (for error message).
        moved_in: &'static str,
    },
    /// Partial field write to out param (must write whole value).
    /// D009
    OutParamPartialWrite {
        local_index: u32,
        name: String,
    },
}

/// Format analysis errors for display.
pub fn format_analysis_errors(errors: &[AnalysisError]) -> String {
    errors.iter()
        .map(|e| format_single_error(e))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_single_error(error: &AnalysisError) -> String {
    match error {
        AnalysisError::UseAfterMove { local_index: _, name } => {
            format!("error[D001]: use of moved value: `{}`", name)
        }
        AnalysisError::DoubleMove { local_index: _, name } => {
            format!("error[D002]: value moved twice: `{}`", name)
        }
        AnalysisError::CannotMoveBorrowed { local_index: _, name } => {
            format!("error[D003]: cannot move borrowed value: `{}`", name)
        }
        AnalysisError::CannotMutFromRef { local_index: _, name } => {
            format!("error[D004]: cannot get mutable reference from immutable: `{}`", name)
        }
        AnalysisError::ReadUninitialized { local_index: _, name } => {
            format!("error[D005]: read of uninitialized binding: `{}`", name)
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            format!("error[D006]: out parameter not initialized: `{}`", name)
        }
        AnalysisError::MoveInLoop { local_index: _, name } => {
            format!("error[D007]: cannot move `{}` in loop", name)
        }
        AnalysisError::InconsistentBranchMove { stmt_idx: _, name, moved_in } => {
            format!("error[D008]: `{}` moved in {} branch but not the other", name, moved_in)
        }
        AnalysisError::OutParamPartialWrite { local_index: _, name } => {
            format!("error[D009]: cannot partially write to out parameter: `{}`", name)
        }
    }
}

// ============================================================================
// Analysis results
// ============================================================================

/// Drop schedule computed by analysis.
///
/// Maps statement indices to bindings that need to be dropped at various
/// control flow points (branch exits, returns, loops, etc.).
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct DropSchedule {
    /// Drops to emit at the end of a then-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub then_branch_exit: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of an else-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub else_branch_exit: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before a return statement.
    /// Key is the return statement index in the body.
    pub before_return: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before TryReturn in checked/optional operators.
    /// Key is the statement index containing the expression with try.
    pub before_try_return: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at end of loop body before looping back.
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before break.
    pub before_break: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before continue.
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,

    /// Statement keys in allocation order, for verifying lowering traversal.
    /// Only present in debug builds.
    #[cfg(debug_assertions)]
    pub stmt_order: Vec<StmtKey>,
}

/// Result of analyzing a function.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct FunctionAnalysis {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    /// Determines whether precise or tracked move/drop instructions are used.
    pub tracking: Vec<TrackingCategory>,
}

/// Analysis result for script-level statements.
///
/// Similar to FunctionAnalysis but includes script-specific data like unit_end
/// drops for bindings that should be dropped when the script unit ends.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ScriptAnalysisData {
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    pub tracking: Vec<TrackingCategory>,
    /// Bindings to drop at unit end (for AOT cleanup).
    pub unit_end: Vec<BindingId>,
}
