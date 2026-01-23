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
//! - D005 ReadUninitializedOutParam: reading out param before initialization
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
    Statement, StmtFun, StmtLet, StmtVar, StmtSet, StmtRet, StmtIf, StmtLoop,
    ExprFun, ExprFunKind, BinOp, UnaryOp, ParamMode, SetTarget,
};
use datalove_datafun_ir::IrType;
use crate::ir_ext::IrTypeExt;

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
    /// Reading Out param before it was written.
    /// D005
    ReadUninitializedOutParam {
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
        AnalysisError::ReadUninitializedOutParam { local_index: _, name } => {
            format!("error[D005]: read of uninitialized out parameter: `{}`", name)
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            format!("error[D006]: out parameter not initialized: `{}`", name)
        }
        AnalysisError::MoveInLoop { local_index: _, name } => {
            format!("error[D007]: cannot move `{}` in loop", name)
        }
    }
}

// ============================================================================
// Analysis results
// ============================================================================

/// Drop schedule computed by analysis.
///
/// Keyed by statement index. During lowering, after processing each AST node,
/// check if there are drops scheduled for it.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct DropSchedule {
    /// Drops to emit after processing a statement.
    /// Key is statement index in the body.
    pub after_stmt: BTreeMap<usize, Vec<BindingId>>,

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
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    /// Resolved call targets for looking up callee parameter modes.
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
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
    /// Bindings that were conditionally moved (moved in one branch, not the other).
    /// These need tracked semantics since their state varies at runtime.
    conditionally_moved: std::collections::HashSet<BindingId>,
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
        expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
        call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
    ) -> Self {
        Self {
            db,
            expr_types,
            call_targets,
            next_binding: 0,
            next_stmt_id: 0,
            bindings: Vec::new(),
            name_to_binding: HashMap::new(),
            scope_stack: Vec::new(),
            errors: Vec::new(),
            schedule: DropSchedule::default(),
            conditionally_moved: std::collections::HashSet::new(),
        }
    }

    /// Compute tracking category for each binding.
    ///
    /// Categories:
    /// - Copy: type is copy (no tracking needed)
    /// - Tracked: exported, all params, slot (var), or conditionally moved
    /// - Precise: let bindings with statically-known state
    ///
    /// In params inside functions are precise because:
    /// - Always initialized at function entry
    /// - Cannot be reassigned (read-only)
    /// - Not exported (function-local)
    /// - Lifecycle fully determined by function scope
    ///
    /// Other bindings remain tracked for now (conservative).
    fn compute_tracking(&self) -> Vec<TrackingCategory> {
        self.bindings.iter().enumerate().map(|(idx, info)| {
            let _id = BindingId(idx as u32);
            if info.ty.is_copy() {
                TrackingCategory::Copy
            } else if info.param_mode == Some(ParamMode::In) && !info.is_script_unit {
                // In params in functions are precise.
                TrackingCategory::Precise
            } else {
                // Conservative: track everything else.
                TrackingCategory::Tracked
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
    fn alloc_stmt_id(&mut self) -> usize {
        let id = self.next_stmt_id;
        self.next_stmt_id += 1;
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
        match self.expr_types.get(index).cloned().flatten() {
            Some(ty) => IrType::from_tycheck(self.db, &ty),
            None => IrType::Unit,
        }
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
                    // Check for reading uninitialized Out param.
                    if self.bindings[id.0 as usize].param_mode == Some(ParamMode::Out) {
                        if self.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
                            let name = self.bindings[id.0 as usize].name.C();
                            self.errors.push(AnalysisError::ReadUninitializedOutParam { local_index, name });
                            return None;
                        }
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
                let callee_modes: Vec<ParamMode> = self.call_targets
                    .get(call_index)
                    .and_then(|opt| opt.as_ref())
                    .map(|target| {
                        target.func(self.db).params(self.db)
                            .iter()
                            .map(|p| p.mode)
                            .collect()
                    })
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
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
    resolved_param_types: Option<&[IrType]>,
) -> FunctionAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_targets);

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
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
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

            let analysis = analyze_function(db, *func, expr_types, call_targets, resolved_params);
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
    /// Bindings to drop at unit end (only populated when for_aot=true).
    pub unit_end: Vec<BindingId>,
}

/// Analyze script-level statements and compute drop schedule.
///
/// Similar to `analyze_function` but for script units. Key differences:
/// - Enters `ScriptUnit` scope instead of `Function` scope (unless for_aot=true)
/// - Top-level bindings are NOT scheduled for drops (they're exported) unless for_aot=true
/// - Nested scopes (if, loop) get normal drop analysis
///
/// When `for_aot` is true:
/// - Uses `Function` scope so top-level bindings ARE scheduled for drops
/// - Returns final drops in `unit_end` field for emission before UnitEnd
pub fn analyze_script_statements<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
    stmts: &[Statement<'db>],
    for_aot: bool,
) -> ScriptAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_targets);

    // Enter scope. For AOT, use Function scope so bindings get dropped.
    // For REPL, use ScriptUnit scope so bindings are exported.
    let scope_kind = if for_aot { ScopeKind::Function } else { ScopeKind::ScriptUnit };
    ctx.enter_scope(scope_kind);

    // Analyze statements.
    analyze_statements(&mut ctx, stmts);

    // Exit scope. For AOT, capture final drops. For REPL, they're empty.
    let final_drops = ctx.exit_scope();

    // Compute tracking categories.
    let tracking = ctx.compute_tracking();

    ScriptAnalysis {
        errors: ctx.errors,
        schedule: ctx.schedule,
        bindings: ctx.bindings,
        tracking,
        unit_end: final_drops,
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
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
) -> ExprAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types, call_targets);

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
        let stmt_id = ctx.alloc_stmt_id();
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

fn analyze_var<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &StmtVar<'db>, stmt_idx: usize) {
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

    // Create binding for the var (as a slot).
    let name = stmt.name.text(ctx.db).S();
    let ty = ctx.expr_type(expr);
    ctx.alloc_binding(name, ty, true, None);
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
            // TODO: Handle field projection in drop analysis.
            return;
        }
    };
    if let Some(id) = ctx.lookup(name) {
        ctx.set_state(id, BindingState::Live);

        // Mark Out param as initialized.
        if ctx.bindings[id.0 as usize].param_mode == Some(ParamMode::Out) {
            ctx.set_out_param_init(id, OutParamInitState::Initialized);
        }
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

    // Compute convergence drops.
    // For each binding that is Live in one branch but Moved in another,
    // schedule a drop on the Live branch and mark as conditionally moved.
    let mut then_extra_drops = Vec::new();
    let mut else_extra_drops = Vec::new();

    for (&id, &then_state) in &state_after_then {
        let else_state = state_after_else.get(&id).copied().unwrap_or(BindingState::Live);

        if then_state != else_state {
            // Mark as conditionally moved - needs tracked semantics.
            if !ctx.bindings[id.0 as usize].ty.is_copy() {
                ctx.conditionally_moved.insert(id);
            }

            if then_state == BindingState::Live {
                // Live in then, moved in else -> drop in then.
                if !ctx.bindings[id.0 as usize].ty.is_copy() {
                    then_extra_drops.push(id);
                }
            } else {
                // Moved in then, live in else -> drop in else.
                if !ctx.bindings[id.0 as usize].ty.is_copy() {
                    else_extra_drops.push(id);
                }
            }
        }
    }

    // Combine with scope exit drops.
    let mut all_then_drops = then_drops;
    all_then_drops.extend(then_extra_drops);
    if !all_then_drops.is_empty() {
        ctx.schedule.then_branch_exit.insert(stmt_idx, all_then_drops);
    }

    if !else_extra_drops.is_empty() {
        // Add to existing else drops if any.
        let existing = ctx.schedule.else_branch_exit.entry(stmt_idx).or_default();
        existing.extend(else_extra_drops);
    }

    // After convergence, all bindings that were live in either branch but moved
    // in one should now be considered moved.
    if let Some(frame) = ctx.scope_stack.last_mut() {
        for (&id, &then_state) in &state_after_then {
            let else_state = state_after_else.get(&id).copied().unwrap_or(BindingState::Live);
            // If moved in either branch, it's now moved.
            if then_state == BindingState::Moved || else_state == BindingState::Moved {
                frame.current_state.insert(id, BindingState::Moved);
            } else {
                frame.current_state.insert(id, BindingState::Live);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;

    fn parse_function<'db>(db: &'db dyn salsa::Database, source_code: &str) -> StmtFun<'db> {
        let source = Source::new(db, source_code.to_string());
        let script = datalove_datafun_parser::parse_for_test(db, source);
        let statements = script.statements;

        for stmt in statements {
            if let Statement::Fun(fun) = stmt {
                return fun;
            }
        }
        panic!("No function found in source code");
    }

    #[test]
    fn test_simple_function() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(): u32
    let x = @42
    ret x
end fun
        "#;

        let func = parse_function(db, source);
        let expr_types = &[];
        let call_targets = &[];
        let analysis = analyze_function(db, func, expr_types, call_targets, None);

        assert!(analysis.errors.is_empty());
    }

    #[test]
    fn test_conditional_move() {
        // Full convergence testing is done in interp3 test 120_conditional_move_convergence.
        // This test just verifies the analysis runs without panicking.
        let ref db = crate::Database::default();
        let source = r#"
fun test(cond: bool): u32
    let x = [@1, @2]
    if cond
        let _sink = x
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        // Without full expr_types, types default to Unit (Copy), so no drops scheduled.
        // This just tests that analysis completes without panicking.
        let analysis = analyze_function(db, func, &[], &[], None);

        // No errors expected even without type info.
        assert!(analysis.errors.is_empty());
    }

    #[test]
    fn test_ref_param_registered_correctly() {
        // Verify that ref params have param_mode = Some(Ref).
        let ref db = crate::Database::default();
        let source = r#"
fun test(ref x: u32): u32
    ret x
end fun
        "#;

        let func = parse_function(db, source);
        let analysis = analyze_function(db, func, &[], &[], None);

        // Should have one binding (the ref param).
        assert_eq!(analysis.bindings.len(), 1);
        assert_eq!(analysis.bindings[0].param_mode, Some(ParamMode::Ref), "ref param should have param_mode = Ref");
        assert!(analysis.bindings[0].is_borrowed(), "ref param should be borrowed");
        assert_eq!(analysis.bindings[0].name, "x");
        assert!(analysis.errors.is_empty());
    }

    #[test]
    fn test_in_param_not_ref() {
        // Verify that regular in params have param_mode = Some(In).
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32): u32
    ret x
end fun
        "#;

        let func = parse_function(db, source);
        let analysis = analyze_function(db, func, &[], &[], None);

        // Should have one binding (the in param).
        assert_eq!(analysis.bindings.len(), 1);
        assert_eq!(analysis.bindings[0].param_mode, Some(ParamMode::In), "in param should have param_mode = In");
        assert!(!analysis.bindings[0].is_borrowed(), "in param should NOT be borrowed");
        assert_eq!(analysis.bindings[0].name, "x");
        assert!(analysis.errors.is_empty());
    }

    #[test]
    fn test_ref_param_cannot_be_moved() {
        // Verify that trying to move a ref param produces an error.
        // Use @int (non-Copy type) since Copy types don't trigger move tracking.
        let ref db = crate::Database::default();
        let source = r#"
fun test(ref x: int): int
    let sink = x
    ret sink
end fun
        "#;

        let func = parse_function(db, source);
        let analysis = analyze_function(db, func, &[], &[], None);

        // Should have an error for moving the ref param.
        assert!(!analysis.errors.is_empty(), "should have error for moving ref param");

        // Check that it's specifically a CannotMoveBorrowed error.
        let has_cannot_move_error = analysis.errors.iter().any(|e| {
            matches!(e, AnalysisError::CannotMoveBorrowed { name, .. } if name == "x")
        });
        assert!(has_cannot_move_error, "error should be CannotMoveBorrowed for 'x'");
    }

    #[test]
    fn test_mixed_ref_and_in_params() {
        // Verify mixed parameter modes are tracked correctly.
        let ref db = crate::Database::default();
        let source = r#"
fun test(a: u32, ref b: u32, c: u32): u32
    ret a
end fun
        "#;

        let func = parse_function(db, source);
        let analysis = analyze_function(db, func, &[], &[], None);

        // Should have three bindings.
        assert_eq!(analysis.bindings.len(), 3);
        assert_eq!(analysis.bindings[0].param_mode, Some(ParamMode::In), "a should be In mode");
        assert_eq!(analysis.bindings[1].param_mode, Some(ParamMode::Ref), "b should be Ref mode");
        assert_eq!(analysis.bindings[2].param_mode, Some(ParamMode::In), "c should be In mode");
        assert!(analysis.errors.is_empty());
    }
}
