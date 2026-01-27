//! Ownership and liveness analysis.
//!
//! This module performs static analysis of value ownership, detecting errors
//! and computing drop schedules. It runs on the AST after typechecking and
//! before IR lowering.
//!
//! # Analysis performed
//!
//! **Ownership tracking**: Values are either Live (owned) or Moved (ownership
//! transferred). Moving a value transfers ownership; using it again is an error.
//!
//! **Borrow checking**: Borrowed parameters (ref, mut, out) cannot be moved
//! since the caller retains ownership. Additionally, immutable refs cannot be
//! passed where mutable refs are required.
//!
//! **Initialization tracking**: Out parameters must be initialized (via `set`)
//! before the function returns, and cannot be read before initialization.
//!
//! **Drop scheduling**: As a byproduct of liveness analysis, computes precise
//! drop points for the IR lowering phase.
//!
//! # Errors detected
//!
//! - D001 UseAfterMove: using a value after ownership was transferred
//! - D002 DoubleMove: transferring ownership twice
//! - D003 CannotMoveBorrowed: attempting to move a ref/mut/out parameter
//! - D004 CannotMutFromRef: passing immutable ref where mutable is required
//! - D005 ReadUninitialized: reading binding before initialization
//! - D006 OutParamNotInitialized: returning without initializing out param
//! - D007 MoveInLoop: moving outer-scoped value inside loop body
//!
//! # Drop scheduling
//!
//! The `DropSchedule` tells IR lowering when to emit Drop instructions:
//! - After statements when bindings go out of scope
//! - At branch exits for convergence (value moved in one branch, not other)
//! - Before return/break/continue for cleanup
//! - At loop body end for iteration-scoped bindings

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use std::collections::HashMap;
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::{
    Statement, StmtFun, StmtLet, StmtVar, StmtSet, StmtRet, StmtIf, StmtLoop, StmtConst,
    ExprFun, ExprFunKind, BinOp, UnaryOp, ParamMode, SetTarget,
};
use datalove_datafun_ir::IrType;

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
// Call site information
// ============================================================================

/// Pre-resolved call site info for ownership analysis.
///
/// This provides the parameter modes of the callee function, which ownership
/// analysis needs to determine whether arguments are consumed (In) or borrowed
/// (Ref, Mut, Out).
#[derive(Clone, Debug)]
pub struct CallInfo {
    /// Parameter modes of the called function.
    pub param_modes: Vec<ParamMode>,
}

// ============================================================================
// Core types
// ============================================================================

/// Pre-computed drop analyses for functions in a script unit.
pub type ScriptFunctionAnalyses<'db> = HashMap<StmtFun<'db>, FunctionAnalysis>;

/// Identifies a binding (parameter or let/var).
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct BindingId(pub u32);

/// State of a binding during analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingState {
    /// In scope, not yet moved.
    Live,
    /// Ownership transferred.
    Moved,
}

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

/// Initialization state for Out params.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutParamInitState {
    /// Out param has not yet been written.
    Uninitialized,
    /// Out param has been written (via `set`).
    Initialized,
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

// ============================================================================
// Analysis context
// ============================================================================

/// Context for ownership and liveness analysis.
struct AnalysisCtx<'db> {
    db: &'db dyn salsa::Database,
    /// Pre-converted expression types (IrType).
    expr_types: &'db [Option<IrType>],
    /// Pre-resolved call info for looking up callee parameter modes.
    call_info: &'db [Option<CallInfo>],
    /// Next binding ID to allocate.
    next_binding: u32,
    /// Next global statement ID for drop schedule keys.
    next_stmt_id: usize,
    /// All bindings (indexed by BindingId).
    bindings: Vec<BindingInfo>,
    /// Name to binding ID mapping (current scope).
    name_to_binding: HashMap<String, BindingId>,
    /// Stack of scopes. Each scope records bindings created in it.
    scope_stack: Vec<ScopeFrame>,
    /// Detected errors.
    errors: Vec<AnalysisError>,
    /// Computed drop schedule.
    schedule: DropSchedule,
}

/// A scope frame for tracking bindings.
#[derive(Clone, Debug)]
struct ScopeFrame {
    /// Bindings created in this scope.
    bindings: Vec<BindingId>,
    /// Kind of scope (for handling break/continue).
    kind: ScopeKind,
    /// Current state of bindings.
    current_state: HashMap<BindingId, BindingState>,
    /// Initialization state for Out params.
    out_param_init: HashMap<BindingId, OutParamInitState>,
    /// Where each binding was moved (local_index), for error reporting.
    moved_at: HashMap<BindingId, u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScopeKind {
    Function,
    /// Script unit top-level scope. Bindings are exported, not dropped.
    ScriptUnit,
    Loop,
    IfThen,
    IfElse,
}

impl<'db> AnalysisCtx<'db> {
    fn new(
        db: &'db dyn salsa::Database,
        expr_types: &'db [Option<IrType>],
        call_info: &'db [Option<CallInfo>],
    ) -> Self {
        Self {
            db,
            expr_types,
            call_info,
            next_binding: 0,
            next_stmt_id: 0,
            bindings: Vec::new(),
            name_to_binding: HashMap::new(),
            scope_stack: Vec::new(),
            errors: Vec::new(),
            schedule: DropSchedule::default(),
        }
    }

    /// Compute tracking category for each binding.
    ///
    /// Categories:
    /// - Copy: type is copy (no tracking needed)
    /// - Tracked: var slots, Out params
    /// - Precise: let bindings, In/Ref/Mut params
    ///
    /// Precise bindings have deterministic lifecycle:
    /// - In params: owned, always initialized
    /// - Ref/Mut params: borrowed, always initialized, cannot be moved/dropped
    /// - Let bindings: single assignment, drop point statically known
    fn compute_tracking(&self) -> Vec<TrackingCategory> {
        self.bindings.iter().map(|info| {
            if info.ty.is_copy() {
                TrackingCategory::Copy
            } else if info.is_slot {
                // Var bindings: reassignable, state varies.
                TrackingCategory::Tracked
            } else if info.param_mode == Some(ParamMode::Out) {
                // Out params: may be uninitialized, dynamic state.
                TrackingCategory::Tracked
            } else {
                // In/Ref/Mut params and let bindings: deterministic state.
                TrackingCategory::Precise
            }
        }).collect()
    }

    /// Allocate a new binding ID.
    fn alloc_binding(&mut self, name: String, ty: IrType, is_slot: bool, param_mode: Option<ParamMode>) -> BindingId {
        let id = BindingId(self.next_binding);
        self.next_binding += 1;

        // Check if we're directly in ScriptUnit scope.
        let is_script_unit = self.scope_stack.last()
            .map(|f| f.kind == ScopeKind::ScriptUnit)
            .unwrap_or(false);

        self.bindings.push(BindingInfo { name: name.C(), ty, is_slot, is_script_unit, param_mode });

        // Record in current scope.
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.bindings.push(id);
            frame.current_state.insert(id, BindingState::Live);

            // Out params start uninitialized.
            if param_mode == Some(ParamMode::Out) {
                frame.out_param_init.insert(id, OutParamInitState::Uninitialized);
            }
        }

        // Add to name mapping.
        self.name_to_binding.insert(name, id);

        id
    }

    /// Enter a new scope.
    fn enter_scope(&mut self, kind: ScopeKind) {
        // Copy current state from parent scope.
        let current_state = self.scope_stack.last()
            .map(|f| f.current_state.C())
            .unwrap_or_default();
        let out_param_init = self.scope_stack.last()
            .map(|f| f.out_param_init.C())
            .unwrap_or_default();
        let moved_at = self.scope_stack.last()
            .map(|f| f.moved_at.C())
            .unwrap_or_default();

        self.scope_stack.push(ScopeFrame {
            bindings: Vec::new(),
            kind,
            current_state,
            out_param_init,
            moved_at,
        });
    }

    /// Exit scope and return bindings that need dropping.
    fn exit_scope(&mut self) -> Vec<BindingId> {
        let frame = self.scope_stack.pop().expect("unbalanced scope");

        // ScriptUnit bindings are exported, not dropped.
        let is_script_unit = frame.kind == ScopeKind::ScriptUnit;

        // Collect bindings that are still live and need dropping.
        let mut to_drop = Vec::new();
        if !is_script_unit {
            for &id in &frame.bindings {
                if frame.current_state.get(&id) == Some(&BindingState::Live) {
                    let info = &self.bindings[id.0 as usize];
                    // Skip Copy types (no drop needed).
                    // Skip borrowed params (caller retains ownership).
                    if !info.ty.is_copy() && !info.is_borrowed() {
                        to_drop.push(id);
                    }
                }
            }
        }

        // Remove bindings from name mapping.
        for &id in &frame.bindings {
            let name = &self.bindings[id.0 as usize].name;
            self.name_to_binding.remove(name);
        }

        // Propagate state changes to parent scope.
        if let Some(parent) = self.scope_stack.last_mut() {
            for (id, state) in frame.current_state {
                // Only propagate state for bindings that existed before this scope.
                if parent.current_state.contains_key(&id) {
                    parent.current_state.insert(id, state);
                }
            }
            // Propagate Out param init state.
            for (id, state) in frame.out_param_init {
                if parent.out_param_init.contains_key(&id) {
                    parent.out_param_init.insert(id, state);
                }
            }
            // Propagate moved_at tracking.
            for (id, expr_id) in frame.moved_at {
                if parent.current_state.contains_key(&id) && !parent.moved_at.contains_key(&id) {
                    parent.moved_at.insert(id, expr_id);
                }
            }
        }

        to_drop
    }

    /// Get current state of a binding.
    fn get_state(&self, id: BindingId) -> Option<BindingState> {
        self.scope_stack.last()?.current_state.get(&id).copied()
    }

    /// Set state of a binding.
    fn set_state(&mut self, id: BindingId, state: BindingState) {
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.current_state.insert(id, state);
        }
    }

    /// Get Out param init state.
    fn get_out_param_init(&self, id: BindingId) -> Option<OutParamInitState> {
        self.scope_stack.last()?.out_param_init.get(&id).copied()
    }

    /// Set Out param init state.
    fn set_out_param_init(&mut self, id: BindingId, state: OutParamInitState) {
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.out_param_init.insert(id, state);
        }
    }

    /// Get where a binding was moved (local_index).
    fn get_moved_at(&self, id: BindingId) -> Option<u32> {
        self.scope_stack.last()?.moved_at.get(&id).copied()
    }

    /// Record where a binding was moved.
    fn set_moved_at(&mut self, id: BindingId, local_index: u32) {
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.moved_at.insert(id, local_index);
        }
    }

    /// Look up a binding by name.
    fn lookup(&self, name: &str) -> Option<BindingId> {
        self.name_to_binding.get(name).copied()
    }

    /// Mark a binding as moved at the given expression.
    fn mark_moved(&mut self, id: BindingId, local_index: u32) {
        // Borrowed params (Ref/Mut) cannot be moved - caller retains ownership.
        if self.bindings[id.0 as usize].is_borrowed() {
            let name = self.bindings[id.0 as usize].name.C();
            self.errors.push(AnalysisError::CannotMoveBorrowed { local_index, name });
            return;
        }

        if self.get_state(id) == Some(BindingState::Moved) {
            // Double move error.
            let name = self.bindings[id.0 as usize].name.C();
            self.errors.push(AnalysisError::DoubleMove { local_index, name });
        } else {
            // Track the move for error detection.
            // Note: ScriptUnit bindings are tracked for error detection (e.g., move in loop)
            // but are NOT scheduled for drops since they're exported.
            self.set_state(id, BindingState::Moved);
            self.set_moved_at(id, local_index);
        }
    }

    /// Allocate and return the next global statement ID.
    ///
    /// Records the statement key for debug verification that lowering
    /// processes statements in the same order.
    fn alloc_stmt_id(&mut self, stmt: &Statement<'db>) -> usize {
        let id = self.next_stmt_id;
        self.next_stmt_id += 1;
        #[cfg(debug_assertions)]
        self.schedule.stmt_order.push(StmtKey::from_stmt(self.db, stmt));
        id
    }

    /// Get all live bindings defined in scopes we're exiting (for break/continue).
    ///
    /// Only includes bindings that were created within the scopes being traversed,
    /// not bindings from outer scopes that happen to be live.
    fn live_bindings_in_scopes(&self, stop_at: ScopeKind) -> Vec<BindingId> {
        let mut result = Vec::new();
        for frame in self.scope_stack.iter().rev() {
            // Only include bindings defined in this frame.
            for &id in &frame.bindings {
                if frame.current_state.get(&id) == Some(&BindingState::Live) {
                    let info = &self.bindings[id.0 as usize];
                    // Skip Copy types.
                    if !info.ty.is_copy() {
                        result.push(id);
                    }
                }
            }
            // Stop at the requested scope or at script unit boundary.
            if frame.kind == stop_at || frame.kind == ScopeKind::ScriptUnit {
                break;
            }
        }
        result
    }

    /// Get all live bindings for return (all live bindings in all scopes).
    ///
    /// For functions, stops at Function scope. For scripts, stops at ScriptUnit scope.
    /// Script top-level bindings are NOT included (they're exported, not dropped).
    fn live_bindings_for_return(&self) -> Vec<BindingId> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for frame in self.scope_stack.iter().rev() {
            // For ScriptUnit, don't include its bindings (they're exported).
            // Just stop here without adding them.
            if frame.kind == ScopeKind::ScriptUnit {
                break;
            }

            // Include bindings defined in this frame.
            for &id in &frame.bindings {
                // Mark as seen first - inner scopes have the most up-to-date state.
                if !seen.insert(id) {
                    continue;
                }
                if frame.current_state.get(&id) == Some(&BindingState::Live) {
                    let info = &self.bindings[id.0 as usize];
                    // Skip Copy types and borrowed params (caller retains ownership).
                    if !info.ty.is_copy() && !info.is_borrowed() {
                        result.push(id);
                    }
                }
            }
            // Also include bindings from parent scopes that are tracked here.
            for (&id, &state) in &frame.current_state {
                // Mark as seen first - inner scopes have the most up-to-date state.
                if !seen.insert(id) {
                    continue;
                }
                if state == BindingState::Live {
                    let info = &self.bindings[id.0 as usize];
                    // Skip Copy types and borrowed params (caller retains ownership).
                    if !info.ty.is_copy() && !info.is_borrowed() {
                        result.push(id);
                    }
                }
            }
            if frame.kind == ScopeKind::Function {
                break;
            }
        }
        result
    }

    /// Get the type of an expression.
    fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
        let expr_id = expr.as_id();
        let index = expr_id.index() as usize;
        self.expr_types.get(index).cloned().flatten().unwrap_or(IrType::Unit)
    }

    /// If the expression is a simple name, return its binding ID.
    fn expr_to_binding(&self, expr: ExprFun<'db>) -> Option<BindingId> {
        if let ExprFunKind::Name(name_text) = expr.expr(self.db) {
            let name: &str = name_text.text(self.db).as_ref();
            self.name_to_binding.get(name).copied()
        } else {
            None
        }
    }

    /// Check if an expression contains early-return operators.
    fn expr_may_early_return(&self, expr: ExprFun<'db>) -> bool {
        match expr.expr(self.db) {
            ExprFunKind::TryOption(_) | ExprFunKind::TryResult(_) => true,
            ExprFunKind::BinOp(binop) => {
                let op_may_return = matches!(
                    binop.op,
                    BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional |
                    BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked
                );
                op_may_return
                    || self.expr_may_early_return(binop.lhs)
                    || self.expr_may_early_return(binop.rhs)
            }
            ExprFunKind::UnaryOp(unary) => {
                let op_may_return = matches!(
                    unary.op,
                    UnaryOp::NegOptional | UnaryOp::NegResult
                );
                op_may_return || self.expr_may_early_return(unary.operand)
            }
            ExprFunKind::FunctionCall(call) => {
                call.args(self.db).iter().any(|arg| self.expr_may_early_return(*arg))
            }
            ExprFunKind::Tuple(tuple) => {
                tuple.elements.iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::List(list) => {
                list.elements.iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::Set(set) => {
                set.elements.iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::Map(map) => {
                map.entries.iter().any(|entry| {
                    self.expr_may_early_return(entry.key)
                        || self.expr_may_early_return(entry.value)
                })
            }
            ExprFunKind::AnonTuple(tuple) => {
                tuple.elements.iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::AnonStruct(s) => {
                s.fields.iter().any(|f| self.expr_may_early_return(f.value))
            }
            ExprFunKind::Some(s) => self.expr_may_early_return(s.payload),
            ExprFunKind::Ok(o) => self.expr_may_early_return(o.payload),
            ExprFunKind::Er(e) => self.expr_may_early_return(e.payload),
            ExprFunKind::Data(d) => self.expr_may_early_return(d.value),
            ExprFunKind::Error(e) => self.expr_may_early_return(e.value),
            _ => false,
        }
    }

    /// Analyze moves in an expression.
    ///
    /// If `is_consumed` is true, the expression result is consumed (bound to a
    /// variable, passed to a function, returned). Otherwise, it's just read
    /// (e.g., operand of a binary operation).
    ///
    /// Returns the binding ID if the expression is a simple move of a binding.
    fn analyze_expr_moves(&mut self, expr: ExprFun<'db>, is_consumed: bool) -> Option<BindingId> {
        let local_index = expr.local_index(self.db);
        match expr.expr(self.db) {
            ExprFunKind::Name(name) => {
                let name_str = name.text(self.db);
                if let Some(id) = self.lookup(name_str) {
                    // Check for reading uninitialized binding (Out param or uninitialized var).
                    if self.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
                        let name = self.bindings[id.0 as usize].name.C();
                        self.errors.push(AnalysisError::ReadUninitialized { local_index, name });
                        return None;
                    }
                    // Check for use after move.
                    if self.get_state(id) == Some(BindingState::Moved) {
                        let name = self.bindings[id.0 as usize].name.C();
                        self.errors.push(AnalysisError::UseAfterMove { local_index, name });
                        return None;
                    }
                    if is_consumed && !self.bindings[id.0 as usize].ty.is_copy() {
                        // This is a move.
                        self.mark_moved(id, local_index);
                        return Some(id);
                    }
                }
                None
            }
            ExprFunKind::BinOp(binop) => {
                // Binary ops read their operands, not consume them.
                self.analyze_expr_moves(binop.lhs, false);
                self.analyze_expr_moves(binop.rhs, false);
                None
            }
            ExprFunKind::UnaryOp(unary) => {
                // Unary ops read their operand, not consume it.
                self.analyze_expr_moves(unary.operand, false);
                None
            }
            ExprFunKind::FunctionCall(call) => {
                // Look up callee's parameter modes if available.
                let call_index = call.as_id().index() as usize;
                let callee_modes: Vec<ParamMode> = self.call_info
                    .get(call_index)
                    .and_then(|opt| opt.as_ref())
                    .map(|info| info.param_modes.clone())
                    .unwrap_or_default();

                // Analyze args with appropriate consumption based on param mode.
                let args = call.args(self.db);
                for (i, arg) in args.iter().enumerate() {
                    let callee_mode = callee_modes.get(i).copied();

                    // Check for invalid ref -> mut passing.
                    // Can't get mutable reference from immutable ref param.
                    // Mut -> Mut is allowed since the source already has mutable access.
                    if callee_mode == Some(ParamMode::Mut) {
                        if let Some(binding_id) = self.expr_to_binding(*arg) {
                            if self.bindings[binding_id.0 as usize].param_mode == Some(ParamMode::Ref) {
                                let name = self.bindings[binding_id.0 as usize].name.C();
                                let local_index = arg.local_index(self.db);
                                self.errors.push(AnalysisError::CannotMutFromRef {
                                    local_index,
                                    name,
                                });
                            }
                        }
                    }

                    // For Out params: the callee writes to the arg, so this is NOT a read.
                    // Skip the uninitialized Out param check and mark as initialized after.
                    if callee_mode == Some(ParamMode::Out) {
                        // For Out args, we only need to check use-after-move, not uninitialized.
                        if let Some(binding_id) = self.expr_to_binding(*arg) {
                            if self.get_state(binding_id) == Some(BindingState::Moved) {
                                let name = self.bindings[binding_id.0 as usize].name.C();
                                let local_index = arg.local_index(self.db);
                                self.errors.push(AnalysisError::UseAfterMove { local_index, name });
                            }
                            // Mark the binding as initialized after the call writes to it.
                            if self.bindings[binding_id.0 as usize].param_mode == Some(ParamMode::Out) {
                                self.set_out_param_init(binding_id, OutParamInitState::Initialized);
                            }
                        }
                        continue;
                    }

                    // Ref, Mut, and Out params don't consume (caller retains ownership).
                    let is_consumed = callee_mode
                        .map(|mode| !matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out))
                        .unwrap_or(true);
                    self.analyze_expr_moves(*arg, is_consumed);
                }
                None
            }
            ExprFunKind::Tuple(tuple) => {
                // Tuple elements are consumed.
                for elem in &tuple.elements {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::TryOption(try_opt) => {
                // Try operand is consumed.
                self.analyze_expr_moves(try_opt.operand, true);
                None
            }
            ExprFunKind::TryResult(try_res) => {
                // Try operand is consumed.
                self.analyze_expr_moves(try_res.operand, true);
                None
            }
            ExprFunKind::List(list) => {
                // List elements are consumed.
                for elem in &list.elements {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::Set(set) => {
                // Set elements are consumed.
                for elem in &set.elements {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::Map(map) => {
                // Map entries are consumed.
                for entry in &map.entries {
                    self.analyze_expr_moves(entry.key, true);
                    self.analyze_expr_moves(entry.value, true);
                }
                None
            }
            ExprFunKind::AnonTuple(tuple) => {
                // Tuple elements are consumed.
                for elem in &tuple.elements {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::AnonStruct(s) => {
                // Struct fields are consumed.
                for field in &s.fields {
                    self.analyze_expr_moves(field.value, true);
                }
                None
            }
            ExprFunKind::Some(s) => {
                // Payload is consumed.
                self.analyze_expr_moves(s.payload, true);
                None
            }
            ExprFunKind::Ok(o) => {
                // Payload is consumed.
                self.analyze_expr_moves(o.payload, true);
                None
            }
            ExprFunKind::Er(e) => {
                // Payload is consumed.
                self.analyze_expr_moves(e.payload, true);
                None
            }
            ExprFunKind::Data(d) => {
                // Value is consumed.
                self.analyze_expr_moves(d.value, true);
                None
            }
            ExprFunKind::Error(e) => {
                // Value is consumed.
                self.analyze_expr_moves(e.value, true);
                None
            }
            ExprFunKind::AnonEnum(e) => {
                // Payload is consumed.
                if let Some(payload) = e.payload {
                    self.analyze_expr_moves(payload, true);
                }
                None
            }
            ExprFunKind::FieldProj(proj) => {
                // Field projection reads from the base, doesn't consume it.
                // The base expression may itself consume values (e.g., try operator).
                self.analyze_expr_moves(proj.base, false);
                None
            }
            // Literals don't move anything.
            _ => None,
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Analyze a function for ownership errors and compute drop schedule.
///
/// If `resolved_param_types` is provided, use those types for parameters instead of
/// deriving from AST type hints. This is necessary when type aliases are used in
/// function parameters, since the AST type hint contains an alias reference rather
/// than the resolved type.
pub fn analyze_function<'db>(
    db: &'db dyn salsa::Database,
    func: StmtFun<'db>,
    expr_types: &'db [Option<IrType>],
    call_info: &'db [Option<CallInfo>],
    resolved_param_types: Option<&[IrType]>,
) -> FunctionAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_info);

    // Enter function scope.
    ctx.enter_scope(ScopeKind::Function);

    // Register parameters as bindings.
    let params = func.params(db);
    for (i, param) in params.iter().enumerate() {
        let name = param.name.text(db).S();
        // Use resolved type if available, otherwise fall back to AST type hint.
        let ty = match resolved_param_types {
            Some(types) => types[i].clone(),
            None => IrType::from_type_hint(db, &param.type_hint),
        };
        ctx.alloc_binding(name, ty, false, Some(param.mode));
    }

    // Analyze function body.
    analyze_statements(&mut ctx, func.body(db));

    // Exit function scope - remaining live bindings need dropping at implicit return.
    let _final_drops = ctx.exit_scope();
    // Note: Final drops are handled by lowering's implicit return path.

    // Compute tracking categories.
    let tracking = ctx.compute_tracking();

    FunctionAnalysis {
        errors: ctx.errors,
        schedule: ctx.schedule,
        bindings: ctx.bindings,
        tracking,
    }
}

/// Analyze all functions in a list of statements.
///
/// Returns a map of function analyses, or an error if any function has analysis errors.
/// Call this before lowering to ensure all functions are valid.
///
/// If `func_param_types` is provided, use those resolved param types for the given functions.
/// This is needed when type aliases are used in function parameters.
pub fn analyze_script_functions<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<IrType>],
    call_info: &'db [Option<CallInfo>],
    stmts: &[Statement<'db>],
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
) -> Result<ScriptFunctionAnalyses<'db>, Vec<(String, Vec<AnalysisError>)>> {
    let mut analyses = HashMap::new();
    let mut errors = Vec::new();

    for stmt in stmts {
        if let Statement::Fun(func) = stmt {
            // Look up resolved param types if available.
            let func_name = func.name(db).text(db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name))
                .map(|v| v.as_slice());

            let analysis = analyze_function(db, *func, expr_types, call_info, resolved_params);
            if !analysis.errors.is_empty() {
                errors.push((func_name.S(), analysis.errors.C()));
            }
            analyses.insert(*func, analysis);
        }
    }

    if errors.is_empty() {
        Ok(analyses)
    } else {
        Err(errors)
    }
}

/// Result of analyzing script-level statements.
#[derive(Clone, Debug)]
pub struct ScriptAnalysis {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// Tracking category for each binding (indexed by BindingId).
    /// Determines whether precise or tracked move/drop instructions are used.
    pub tracking: Vec<TrackingCategory>,
    /// Bindings to emit UnitEndDrop for.
    ///
    /// These are script-level bindings that are still live at unit end.
    /// The IR lowering emits UnitEndDrop for each; backend semantics differ:
    /// - Interpreter: no-op (bindings persist for REPL)
    /// - AOT: conditional drop (checks tracking byte)
    pub unit_end: Vec<BindingId>,
}

/// Analyze script-level statements and compute drop schedule.
///
/// Similar to `analyze_function` but for script units. Key differences:
/// - Enters `ScriptUnit` scope, so bindings are marked as Tracked
/// - Top-level bindings are NOT scheduled for regular drops (they're exported)
/// - Nested scopes (if, loop) get normal drop analysis
/// - Returns live bindings in `unit_end` for UnitEndDrop emission
///
/// The `unit_end` field contains bindings that should have UnitEndDrop emitted.
/// The backend determines the semantics:
/// - Interpreter: no-op (bindings persist for REPL)
/// - AOT: conditional drop (checks tracking byte)
pub fn analyze_script_statements<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<IrType>],
    call_info: &'db [Option<CallInfo>],
    stmts: &[Statement<'db>],
) -> ScriptAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_info);

    // Enter ScriptUnit scope so bindings are Tracked.
    ctx.enter_scope(ScopeKind::ScriptUnit);

    // Analyze statements.
    analyze_statements(&mut ctx, stmts);

    // Collect bindings for UnitEndDrop before exiting scope.
    // These are script-level bindings that are still live and non-Copy.
    let unit_end_bindings: Vec<BindingId> = ctx.scope_stack.last()
        .map(|frame| {
            frame.bindings.iter()
                .filter(|&id| {
                    let info = &ctx.bindings[id.0 as usize];
                    let state = frame.current_state.get(id).copied();
                    // Include if: live, non-Copy, not borrowed.
                    state == Some(BindingState::Live)
                        && !info.ty.is_copy()
                        && !info.is_borrowed()
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();

    // Exit scope (returns empty for ScriptUnit, but we collected bindings above).
    let _ = ctx.exit_scope();

    // Compute tracking categories.
    let tracking = ctx.compute_tracking();

    ScriptAnalysis {
        errors: ctx.errors,
        schedule: ctx.schedule,
        bindings: ctx.bindings,
        tracking,
        unit_end: unit_end_bindings,
    }
}

/// Result of analyzing a standalone expression.
#[derive(Clone, Debug)]
pub struct ExprAnalysis {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError>,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
}

/// Analyze a standalone expression for ownership errors.
///
/// Used for script expression units where the unit is just an expression (no statements).
/// Detects use-after-move within the expression itself, e.g. `(x, x)` where x is non-Copy.
///
/// Note: Expression units don't need a drop schedule because:
/// - The expression result is returned/consumed by the caller
/// - Temporaries created during evaluation are handled by lowering's temp tracking
pub fn analyze_expr<'db>(
    db: &'db dyn salsa::Database,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
    expr_types: &'db [Option<IrType>],
    call_info: &'db [Option<CallInfo>],
) -> ExprAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_info);

    // Enter a scope for the expression analysis.
    ctx.enter_scope(ScopeKind::Function);

    // Expression result is consumed (it's the unit result).
    ctx.analyze_expr_moves(expr, true);

    // Exit scope. No drops needed since expression result is returned.
    let _ = ctx.exit_scope();

    ExprAnalysis {
        errors: ctx.errors,
        bindings: ctx.bindings,
    }
}

// ============================================================================
// Statement analysis
// ============================================================================

/// Analyze statements for ownership tracking.
///
/// Uses globally-unique statement IDs for drop schedule keys to avoid
/// collisions between nested scopes (e.g., breaks in different loops).
fn analyze_statements<'db>(ctx: &mut AnalysisCtx<'db>, stmts: &[Statement<'db>]) {
    for stmt in stmts.iter() {
        // Allocate a globally-unique statement ID.
        let stmt_id = ctx.alloc_stmt_id(stmt);
        match stmt {
            Statement::Let(let_stmt) => {
                analyze_let(ctx, let_stmt, stmt_id);
            }
            Statement::Var(var_stmt) => {
                analyze_var(ctx, var_stmt, stmt_id);
            }
            Statement::Set(set_stmt) => {
                analyze_set(ctx, set_stmt, stmt_id);
            }
            Statement::Ret(ret_stmt) => {
                analyze_return(ctx, ret_stmt, stmt_id);
            }
            Statement::If(if_stmt) => {
                analyze_if(ctx, if_stmt, stmt_id);
            }
            Statement::Loop(loop_stmt) => {
                analyze_loop(ctx, loop_stmt, stmt_id);
            }
            Statement::Break(_) => {
                // Drops before break: all live bindings in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_break.insert(stmt_id, drops);
                }
            }
            Statement::Continue(_) => {
                // Drops before continue: all live bindings in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_continue.insert(stmt_id, drops);
                }
            }
            Statement::Fun(_) => {
                // Nested functions handled separately.
            }
            Statement::DebugLog(stmt) => {
                // Debuglog borrows its value, so analyze the expression but don't consume it.
                ctx.analyze_expr_moves(stmt.value, false);
            }
            Statement::TypeAlias(_) => {
                // Type aliases are resolved at typecheck time; nothing to analyze.
            }
            Statement::Const(const_stmt) => {
                // In const_as_let mode (now always true), const bindings are lowered as
                // let bindings and need runtime ownership tracking just like let.
                analyze_const(ctx, const_stmt, stmt_id);
            }
            Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No drops.
            }
        }
    }
}

fn analyze_let<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtLet<'db>, stmt_idx: usize) {
    let expr = stmt.value;
    let may_early_return = ctx.expr_may_early_return(expr);

    // Analyze moves in the expression. The expression result is consumed by the binding.
    ctx.analyze_expr_moves(expr, true);

    // Check for early return operators AFTER analyzing moves.
    // This ensures bindings consumed by the expression itself aren't dropped.
    if may_early_return {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Create binding for the let.
    let name = stmt.name.text(ctx.db).S();
    let ty = ctx.expr_type(expr);
    ctx.alloc_binding(name, ty, false, None);
}

/// Analyze a const statement.
///
/// In const_as_let mode (now always true), const bindings are lowered as
/// let bindings and need the same ownership tracking.
fn analyze_const<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtConst<'db>, stmt_idx: usize) {
    let expr = stmt.value;
    let may_early_return = ctx.expr_may_early_return(expr);

    // Analyze moves in the expression. The expression result is consumed by the binding.
    ctx.analyze_expr_moves(expr, true);

    // Check for early return operators AFTER analyzing moves.
    if may_early_return {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Create binding for the const (same as let in const_as_let mode).
    let name = stmt.name.text(ctx.db).S();
    let ty = ctx.expr_type(expr);
    ctx.alloc_binding(name, ty, false, None);
}

fn analyze_var<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtVar<'db>, stmt_idx: usize) {
    let name = stmt.name.text(ctx.db).S();

    if let Some(expr) = stmt.value {
        // Initialized var - analyze the initializer expression.
        let may_early_return = ctx.expr_may_early_return(expr);

        // Analyze moves in the expression. The expression result is consumed by the binding.
        ctx.analyze_expr_moves(expr, true);

        // Check for early return operators AFTER analyzing moves.
        if may_early_return {
            let drops = ctx.live_bindings_for_return();
            if !drops.is_empty() {
                ctx.schedule.before_try_return.insert(stmt_idx, drops);
            }
        }

        // Create binding for the var (as a slot), initialized.
        let ty = ctx.expr_type(expr);
        ctx.alloc_binding(name, ty, true, None);
    } else {
        // Uninitialized var - get type from type hint.
        let ty = stmt.type_hint.as_ref()
            .map(|th| IrType::from_type_hint(ctx.db, th))
            .unwrap_or(IrType::Unit);

        // Create binding, then mark as uninitialized.
        let id = ctx.alloc_binding(name, ty, true, None);

        // Mark as uninitialized (reuse Out param init tracking).
        ctx.set_out_param_init(id, OutParamInitState::Uninitialized);
    }
}

fn analyze_set<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtSet<'db>, stmt_idx: usize) {
    let expr = stmt.value;
    let may_early_return = ctx.expr_may_early_return(expr);

    // Analyze moves in the expression. The value is moved into the slot.
    ctx.analyze_expr_moves(expr, true);

    // Check for early return operators AFTER analyzing moves.
    // This ensures bindings consumed by the expression itself aren't dropped.
    if may_early_return {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Set doesn't create a new binding, but the slot is now live again.
    let name = match &stmt.target {
        SetTarget::Name(n) => n.text(ctx.db),
        SetTarget::Proj(_) => {
            // Find root name of projection chain.
            let root_name = set_target_root_name(ctx.db, &stmt.target);
            if let Some(root) = root_name {
                if let Some(id) = ctx.lookup(root) {
                    // Disallow partial field writes to uninitialized bindings (Out params or uninitialized vars).
                    if ctx.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
                        ctx.errors.push(AnalysisError::OutParamPartialWrite {
                            local_index: stmt.local_index,
                            name: root.to_string(),
                        });
                    }
                }
            }
            return;
        }
    };
    if let Some(id) = ctx.lookup(name) {
        ctx.set_state(id, BindingState::Live);

        // Mark binding as initialized (Out param or uninitialized var).
        // Only set if the binding has init tracking (is in out_param_init map).
        if ctx.get_out_param_init(id).is_some() {
            ctx.set_out_param_init(id, OutParamInitState::Initialized);
        }
    }
}

/// Extract the root name from a set target projection chain.
fn set_target_root_name<'a, 'db>(db: &'db dyn salsa::Database, target: &'a SetTarget<'db>) -> Option<&'a str> {
    match target {
        SetTarget::Name(n) => Some(n.text(db)),
        SetTarget::Proj(proj) => set_target_root_name(db, &proj.base),
    }
}

fn analyze_return<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtRet<'db>, stmt_idx: usize) {
    // Check that all Out params are initialized before return.
    for (idx, info) in ctx.bindings.iter().enumerate() {
        if info.param_mode == Some(ParamMode::Out) {
            let id = BindingId(idx as u32);
            if ctx.get_out_param_init(id) != Some(OutParamInitState::Initialized) {
                let name = info.name.C();
                ctx.errors.push(AnalysisError::OutParamNotInitialized {
                    ret_stmt_idx: Some(stmt_idx),
                    name,
                });
            }
        }
    }

    let may_early_return = stmt.value
        .map(|expr| ctx.expr_may_early_return(expr))
        .unwrap_or(false);

    // Analyze moves in return value if any. The return value is consumed.
    if let Some(expr) = stmt.value {
        ctx.analyze_expr_moves(expr, true);
    }

    // Check for early return operators AFTER analyzing moves.
    // This ensures bindings consumed by the expression itself aren't dropped.
    if may_early_return {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // All live bindings need dropping before return.
    let drops = ctx.live_bindings_for_return();
    if !drops.is_empty() {
        ctx.schedule.before_return.insert(stmt_idx, drops.C());
    }

    // Mark dropped bindings as Moved so they're not included in scope exit drops.
    for id in drops {
        ctx.set_state(id, BindingState::Moved);
    }
}

fn analyze_if<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtIf<'db>, stmt_idx: usize) {
    // Analyze condition. For regular bool conditions, it's just read.
    // For Option/Result conditions with bindings, it's consumed by the destructure.
    let has_binding = stmt.then_binding.is_some();
    ctx.analyze_expr_moves(stmt.condition, has_binding);

    // Save state before branches.
    let state_before = ctx.scope_stack.last()
        .map(|f| f.current_state.C())
        .unwrap_or_default();
    let out_param_init_before = ctx.scope_stack.last()
        .map(|f| f.out_param_init.C())
        .unwrap_or_default();

    // Analyze then branch.
    ctx.enter_scope(ScopeKind::IfThen);

    // If there's a then-binding (if-let), create it.
    if let Some(binding_name) = stmt.then_binding {
        let name = binding_name.text(ctx.db).S();
        let ty = ctx.expr_type(stmt.condition);
        // The binding type depends on the condition type (unwrap Option/Result).
        let inner_ty = match &ty {
            IrType::Option(inner) => (**inner).C(),
            IrType::Result(inner) => (**inner).C(),
            other => other.C(),
        };
        ctx.alloc_binding(name, inner_ty, false, None);
    }

    analyze_statements(ctx, &stmt.then_body);
    let then_drops = ctx.exit_scope();
    let state_after_then = ctx.scope_stack.last()
        .map(|f| f.current_state.C())
        .unwrap_or_default();
    let out_param_init_after_then = ctx.scope_stack.last()
        .map(|f| f.out_param_init.C())
        .unwrap_or_default();

    // Reset state for else branch.
    if let Some(frame) = ctx.scope_stack.last_mut() {
        frame.current_state = state_before.C();
        frame.out_param_init = out_param_init_before.C();
    }

    // Analyze else branch.
    let (state_after_else, out_param_init_after_else) = if let Some(else_body) = &stmt.else_body {
        ctx.enter_scope(ScopeKind::IfElse);

        // If there's an else-binding (if-let with else), create it.
        if let Some(binding_name) = stmt.else_binding {
            let name = binding_name.text(ctx.db).S();
            // Else binding gets the error for Result types.
            let ty = IrType::Error;
            ctx.alloc_binding(name, ty, false, None);
        }

        analyze_statements(ctx, else_body);
        let else_drops = ctx.exit_scope();

        if !else_drops.is_empty() {
            ctx.schedule.else_branch_exit.insert(stmt_idx, else_drops);
        }

        let state = ctx.scope_stack.last()
            .map(|f| f.current_state.C())
            .unwrap_or_default();
        let out_init = ctx.scope_stack.last()
            .map(|f| f.out_param_init.C())
            .unwrap_or_default();
        (state, out_init)
    } else {
        // No else branch - state unchanged.
        (state_before.C(), out_param_init_before.C())
    };

    // Check for inconsistent moves between branches.
    // If a binding is moved in one branch but not the other, that's an error.
    // This ensures drop points are precise - no runtime tracking needed.
    for (&id, &then_state) in &state_after_then {
        let else_state = state_after_else.get(&id).copied().unwrap_or(BindingState::Live);

        if then_state != else_state && !ctx.bindings[id.0 as usize].ty.is_copy() {
            let name = ctx.bindings[id.0 as usize].name.C();
            let moved_in = if then_state == BindingState::Moved { "then" } else { "else" };
            ctx.errors.push(AnalysisError::InconsistentBranchMove {
                stmt_idx,
                name,
                moved_in,
            });
        }
    }

    // Schedule scope exit drops for bindings created in the branches.
    if !then_drops.is_empty() {
        ctx.schedule.then_branch_exit.insert(stmt_idx, then_drops);
    }

    // Update state after convergence.
    // Both branches must have the same state for each binding (or error was reported).
    if let Some(frame) = ctx.scope_stack.last_mut() {
        for (&id, &then_state) in &state_after_then {
            frame.current_state.insert(id, then_state);
        }

        // Out param convergence: must be initialized in both branches or neither.
        // If initialized in only one branch, report error.
        for (&id, &then_init) in &out_param_init_after_then {
            let else_init = out_param_init_after_else.get(&id).copied()
                .unwrap_or(OutParamInitState::Uninitialized);
            if then_init != else_init {
                // Initialized in one branch but not the other.
                let name = ctx.bindings[id.0 as usize].name.C();
                // No specific return statement - this is a branch convergence issue.
                ctx.errors.push(AnalysisError::OutParamNotInitialized {
                    ret_stmt_idx: None,
                    name,
                });
            }
            // After convergence, use the "most restrictive" state: if either is
            // Uninitialized, the converged state is Uninitialized.
            let converged = if then_init == OutParamInitState::Initialized
                && else_init == OutParamInitState::Initialized {
                OutParamInitState::Initialized
            } else {
                OutParamInitState::Uninitialized
            };
            frame.out_param_init.insert(id, converged);
        }
    }
}

fn analyze_loop<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtLoop<'db>, stmt_idx: usize) {
    // Capture outer-scope non-copy bindings that are Live before entering the loop.
    // If any of these become Moved during loop body analysis, that's an error
    // because the loop could iterate multiple times.
    let outer_live_bindings: Vec<BindingId> = ctx.scope_stack.last()
        .map(|frame| {
            frame.current_state.iter()
                .filter(|(id, state)| {
                    **state == BindingState::Live && !ctx.bindings[id.0 as usize].ty.is_copy()
                })
                .map(|(id, _)| *id)
                .collect()
        })
        .unwrap_or_default();

    ctx.enter_scope(ScopeKind::Loop);

    analyze_statements(ctx, &stmt.body);

    // Check for outer-scope bindings that were moved inside the loop body.
    // This is an error because the loop could iterate multiple times.
    for id in &outer_live_bindings {
        if ctx.get_state(*id) == Some(BindingState::Moved) {
            let name = ctx.bindings[id.0 as usize].name.C();
            // Use the recorded move location for the error span.
            let local_index = ctx.get_moved_at(*id).unwrap_or(0);
            ctx.errors.push(AnalysisError::MoveInLoop { local_index, name });
        }
    }

    // Drops at end of loop iteration.
    let loop_drops = ctx.exit_scope();
    if !loop_drops.is_empty() {
        ctx.schedule.loop_body_end.insert(stmt_idx, loop_drops);
    }
}
