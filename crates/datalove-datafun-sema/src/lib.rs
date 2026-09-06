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
use datalove_datafun_ast::ast::{Statement, StmtFun, ParamMode, ExprKey};
use datalove_datafun_ir::IrType;

/// Types of expressions, keyed by the expression's identity.
///
/// This used to be a `Vec` indexed by the raw salsa id, which sizes it by
/// however many ids the database had handed out rather than by how many
/// expressions there are: measured at 1416 slots for 8 of them, and growing
/// with every module added. Keying it also lets two tables with the same
/// contents compare equal, which they could not when their length depended on
/// where the id counter happened to stand.
pub type ExprTypes<'db> = BTreeMap<ExprKey<'db>, datalove_datafun_common::Type<'db>>;

/// The same table after conversion to IR types.
pub type ExprIrTypes<'db> = BTreeMap<ExprKey<'db>, IrType>;

/// Resolved call targets, keyed by the call's identity.
///
/// Calls are numbered separately from other expressions, so a key from one
/// of these tables means nothing against the other.
pub type CallTargets<'db> = BTreeMap<ExprKey<'db>, ResolvedCallTarget<'db>>;

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
    #[returns(copy)]
    pub func: StmtFun<'db>,
    /// The source module (None for script-local functions).
    #[returns(copy)]
    pub module_id: Option<ModuleId<'db>>,
    /// What this call site bound the callee's type parameters to, in the
    /// callee's declaration order.
    ///
    /// Empty for a callee with no type parameters. A generic that builds a
    /// collection of one of its type parameters needs a descriptor for it, and
    /// this is the only place the answer exists: the call site worked it out
    /// from the arguments and what it expected back, and nothing downstream
    /// can work it out again.
    #[returns(ref)]
    pub type_args: Vec<datalove_datalit::tycheck::Type<'db>>,
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
            Statement::Match(_) => (15, None),
            Statement::NativeFun(_) => (16, None),
            Statement::ExprStatement(_) => (17, None),
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
            15 => "Match",
            16 => "NativeFun",
            17 => "ExprStatement",
            _ => "Unknown",
        }
    }
}

// ============================================================================
// Core types
// ============================================================================

/// Identifies a binding (parameter or let/var).
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, PartialOrd, Ord)]
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
    /// Whether this binding is a const.
    ///
    /// A const names a value the compiler computed, not a place holding the
    /// only copy of one, so reading it does not consume it however many times
    /// it is read. Lowering makes each read its own value.
    pub is_const: bool,
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

/// Hint for how to recover from an @-recoverable ownership error.
///
/// When an ownership error can be fixed by inserting the `@` (adapt) operator
/// to clone the value, a recovery hint is attached to the diagnostic.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum OwnershipRecoveryHint {
    /// Insert @ before the expression to clone the value.
    InsertAdapt {
        /// Human-readable description of what @ does here.
        description: String,
    },
    /// No automatic recovery available.
    #[default]
    None,
}

/// Error detected during ownership analysis.
///
/// Each variant includes an `expr_key` for span lookup during diagnostic
/// emission, the expression's stable key within its function, used to look
/// up spans in DatafunSpans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum AnalysisError<'db> {
    /// Using a binding whose own unit gave its value away.
    /// D013 - Recoverable with @ (clone at the use that gave it away)
    UseAfterMoveInEarlierUnit {
        /// Location of the use in this unit.
        expr_key: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Using a value after it was moved.
    /// D001 - Recoverable with @ (clone before first use)
    UseAfterMove {
        /// Location of the second use (where error is detected).
        expr_key: ExprKey<'db>,
        /// Location of the first move (where @ should be inserted).
        moved_at: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Moving a value multiple times.
    /// D002 - Recoverable with @ (clone before second move)
    DoubleMove {
        /// Location of the second move (where error is detected).
        expr_key: ExprKey<'db>,
        /// Location of the first move (where @ should be inserted).
        moved_at: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
    },
    /// Attempting to move a borrowed value (ref/mut/out parameter).
    /// D003
    CannotMoveBorrowed {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Attempting to pass a ref param to a mut param.
    /// D004
    CannotMutFromRef {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Reading binding before it was initialized.
    /// D005. Applies to Out params and uninitialized var bindings.
    ReadUninitialized {
        expr_key: ExprKey<'db>,
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
    /// D007 - Recoverable with @ (clone before entering loop)
    MoveInLoop {
        /// Expression where the move occurred.
        expr_key: ExprKey<'db>,
        name: String,
        recovery_hint: OwnershipRecoveryHint,
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
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Two arguments of one call name the same binding, and at least one of
    /// them is passed to a `mut` or `out` parameter.
    /// D010
    AliasedMutableArgument {
        /// Location of the later of the two arguments.
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Passing an immutable binding to a `mut` or `out` parameter.
    /// D011. Applies to `let` bindings and `in` parameters; a `ref` parameter
    /// reports D004 instead.
    CannotMutateImmutable {
        expr_key: ExprKey<'db>,
        name: String,
    },
    /// Passing a value that is not a place to a `mut` or `out` parameter.
    /// D012. The callee's writes would land in a temporary and be discarded.
    CannotMutateTemporary {
        expr_key: ExprKey<'db>,
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
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key: _, name, recovery_hint } => {
            let base = format!("error[D013]: `{}` was given away by an earlier input", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::UseAfterMove { expr_key: _, moved_at: _, name, recovery_hint } => {
            let base = format!("error[D001]: use of moved value: `{}`", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::DoubleMove { expr_key: _, moved_at: _, name, recovery_hint } => {
            let base = format!("error[D002]: value moved twice: `{}`", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::CannotMoveBorrowed { expr_key: _, name } => {
            format!("error[D003]: cannot move borrowed value: `{}`", name)
        }
        AnalysisError::CannotMutFromRef { expr_key: _, name } => {
            format!("error[D004]: cannot get mutable reference from immutable: `{}`", name)
        }
        AnalysisError::ReadUninitialized { expr_key: _, name } => {
            format!("error[D005]: read of uninitialized binding: `{}`", name)
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            format!("error[D006]: out parameter not initialized: `{}`", name)
        }
        AnalysisError::MoveInLoop { expr_key: _, name, recovery_hint } => {
            let base = format!("error[D007]: cannot move `{}` in loop", name);
            format_with_hint(base, recovery_hint)
        }
        AnalysisError::InconsistentBranchMove { stmt_idx: _, name, moved_in } => {
            format!("error[D008]: `{}` moved in {} branch but not the other", name, moved_in)
        }
        AnalysisError::OutParamPartialWrite { expr_key: _, name } => {
            format!("error[D009]: cannot partially write to out parameter: `{}`", name)
        }
        AnalysisError::AliasedMutableArgument { expr_key: _, name } => {
            format!("error[D010]: aliased mutable argument: `{}`", name)
        }
        AnalysisError::CannotMutateImmutable { expr_key: _, name } => {
            format!("error[D011]: cannot pass immutable binding as mutable: `{}`", name)
        }
        AnalysisError::CannotMutateTemporary { expr_key: _ } => {
            "error[D012]: cannot pass a temporary as mutable".to_string()
        }
    }
}

fn format_with_hint(base: String, hint: &OwnershipRecoveryHint) -> String {
    match hint {
        OwnershipRecoveryHint::InsertAdapt { description } => {
            format!("{}\n  help: use `@` to {}", base, description)
        }
        OwnershipRecoveryHint::None => base,
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

    /// Drops to emit on early return from a set-index bounds check.
    ///
    /// Computed before RHS moves are analyzed, so includes the RHS binding
    /// (which is still live when the bounds check fails).
    pub before_set_target_early_return: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at end of loop body before looping back.
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before break.
    pub before_break: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit before continue.
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of a match arm before jumping to join.
    /// Key is (match_stmt_idx, arm_idx).
    pub match_arm_exit: BTreeMap<(usize, usize), Vec<BindingId>>,

    /// Statement keys in allocation order, for verifying lowering traversal.
    /// Only present in debug builds.
    #[cfg(debug_assertions)]
    pub stmt_order: Vec<StmtKey>,
}

/// Result of analyzing a function.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct FunctionAnalysis<'db> {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError<'db>>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    /// Determines whether precise or tracked move/drop instructions are used.
    pub tracking: Vec<TrackingCategory>,
    /// Uses auto-adapt turned into clones.
    pub adapt_sites: AdaptSites<'db>,
}

/// Expressions where auto-adapt supplies the `@` the source left out.
///
/// Keyed by the expression's stable key, the same key ownership analysis uses
/// for spans, so lowering can recognize the expression it is looking at.
/// Lowering clones such a use instead of moving it.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct AdaptSites<'db> {
    sites: Vec<ExprKey<'db>>,
}

impl<'db> AdaptSites<'db> {
    /// Record that an expression needs an implicit `@`.
    pub fn insert(&mut self, expr_key: ExprKey<'db>) {
        if !self.sites.contains(&expr_key) {
            self.sites.push(expr_key);
        }
    }

    /// Whether an expression needs an implicit `@`.
    pub fn contains(&self, expr_key: ExprKey<'db>) -> bool {
        self.sites.contains(&expr_key)
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    /// Take on another set's sites.
    ///
    /// A key names a function and an index within it, so a set that names
    /// expressions belonging to some other body is harmless: nothing matches.
    pub fn extend(&mut self, other: &AdaptSites<'db>) {
        for site in &other.sites {
            self.insert(*site);
        }
    }
}

/// Analysis result for script-level statements.
///
/// Similar to FunctionAnalysis but includes script-specific data like unit_end
/// drops for bindings that should be dropped when the script unit ends.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ScriptAnalysisData<'db> {
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    pub tracking: Vec<TrackingCategory>,
    /// Bindings to drop at unit end (for AOT cleanup).
    pub unit_end: Vec<BindingId>,
    /// Uses auto-adapt turned into clones.
    pub adapt_sites: AdaptSites<'db>,
    /// Names this unit exports whose value it already gave away.
    ///
    /// Later units can still see the name, but there is nothing behind it, so
    /// using one is an error rather than a copy.
    pub dead_exports: Vec<String>,
    /// Names from earlier units this unit assigned to, which have a value again.
    pub revived_exports: Vec<String>,
}
