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
//! - D015 WalkedCollectionChanged: changing a collection a `for` walks
//!
//! # Drop scheduling
//!
//! The `DropSchedule` tells IR lowering when to emit Drop instructions:
//! - After statements when bindings go out of scope
//! - At branch exits for convergence (value moved in one branch, not other)
//! - Before return/break/continue for cleanup
//! - At loop body end for iteration-scoped bindings

use rmx::prelude::*;
use std::collections::HashMap;
use datalove_datafun_ast::ast::{
    Statement, StmtFun, FunKey, StmtLet, StmtVar, StmtSet, StmtRet, StmtIf, StmtLoop, StmtFor, StmtConst,
    StmtMatch, MatchCaseKind,
    ExprFun, ExprFunKind, BinOp, UnaryOp, ParamMode, ExprKey, ExprId, ExprArena, Body,
};
use datalove_datafun_ast::reachable::body_completes;
use datalove_datafun_ast::ast::Binding;
use datalove_datafun_ir::{CursorKind, IrType};
use datalove_datafun_common::Type;
use datalove_datafun_sema::{BindingMap, Destructure};

// Re-export types from sema for backward compatibility.
pub use datalove_datafun_sema::{
    StmtKey, BindingId, TrackingCategory, BindingInfo, AnalysisError,
    OwnershipRecoveryHint, format_analysis_errors, DropSchedule, FunctionAnalysis,
    ExprTypes, CallTargets, ExternalBinding,
};

// ============================================================================
// Call site information
// ============================================================================

// ============================================================================
// Core types
// ============================================================================

/// Pre-computed drop analyses for functions in a script unit.
pub type ScriptFunctionAnalyses<'db> = bct::lookup::LookupMap<FunKey<'db>, FunctionAnalysis<'db>>;

/// State of a binding during analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BindingState {
    /// In scope, not yet moved.
    Live,
    /// Ownership transferred.
    Moved,
}

/// Initialization state for Out params.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutParamInitState {
    /// Out param has not yet been written.
    Uninitialized,
    /// Out param has been written (via `set`).
    Initialized,
}

// ============================================================================
// Analysis context
// ============================================================================

/// Context for ownership and liveness analysis.
struct AnalysisCtx<'a, 'db> {
    db: &'db dyn salsa::Database,
    /// The typechecker's expression types, converted to IR types as they
    /// are asked for.
    expr_types: &'a ExprTypes<'db>,
    /// The type parameters of the function being analyzed, which its
    /// expression types are written over.
    type_params: &'db [bct::text::InternedText<'db>],
    /// Next binding ID to allocate.
    next_binding: u32,
    /// Next global statement ID for drop schedule keys.
    next_stmt_id: usize,
    /// All bindings (indexed by BindingId).
    bindings: Vec<BindingInfo>,
    /// Name to binding ID mapping (current scope).
    name_to_binding: rustc_hash::FxHashMap<String, BindingId>,
    /// Stack of scopes. Each scope records bindings created in it.
    scope_stack: Vec<ScopeFrame<'db>>,
    /// The earlier units' bindings this unit names, which hold the first
    /// binding ids in order.
    externals: Vec<ExternalBinding>,
    /// The earlier units' bindings this unit writes to, by `set` or by
    /// passing them to a `mut` or `out` parameter.
    written_externals: std::collections::BTreeSet<String>,
    /// Which names this analysis cannot see the binding of are consts.
    outer_consts: OuterConsts,
    /// What each call resolved to, for which of its arguments are const.
    call_targets: &'a CallTargets<'db>,
    /// Detected errors.
    errors: Vec<AnalysisError<'db>>,
    /// Computed drop schedule.
    schedule: DropSchedule,
    /// The ways out of each loop currently being analyzed, innermost last.
    ///
    /// `break` and `continue` carry no label, so both always belong to the
    /// innermost loop and this is a plain stack.
    loop_exits: Vec<LoopExits>,
    /// The collections the `for` loops being analyzed walk, innermost last.
    /// Each is frozen for its loop's body: its cursor and its element bindings
    /// point into it.
    walked: Vec<Walked>,
    /// The expressions of the body being analyzed.
    arena: Option<&'db ExprArena<'db>>,
}

/// The root of a collection a `for` walks.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Walked {
    /// A binding this analysis has.
    Binding(BindingId),
    /// A binding an earlier script unit made, which this one names but has
    /// no binding for.
    Earlier(String),
}

/// The states a loop's `break`s and `continue`s left behind.
///
/// Kept apart because they answer different questions. A `continue` goes back to
/// the loop head, so a binding it moved can be moved again on the next
/// iteration: those states belong to the repeat check. A `break` goes past the
/// loop and never returns to the head, so it cannot repeat, but it decides what
/// is true afterwards along with every other exit.
#[derive(Default)]
struct LoopExits {
    breaks: Vec<BindingMap<BindingState>>,
    /// Which out parameters each `break` left with a value, beside `breaks`.
    break_inits: Vec<BindingMap<OutParamInitState>>,
    continues: Vec<BindingMap<BindingState>>,
}

/// A scope frame for tracking bindings.
#[derive(Clone, Debug)]
struct ScopeFrame<'db> {
    /// Bindings created in this scope.
    bindings: Vec<BindingId>,
    /// What each name in `bindings` meant before this scope took it over.
    ///
    /// A binding here may shadow one from an enclosing scope -- a match arm's
    /// payload taking a name the function body already used, say. Leaving the
    /// scope has to put the outer one back, not merely forget the name: an
    /// unbound name is looked up as nothing, so a later `ret text` recorded no
    /// move of the outer `text`, which then looked live at the return and had
    /// a drop scheduled before the value it returned.
    shadowed: Vec<(String, Option<BindingId>)>,
    /// Kind of scope (for handling break/continue).
    kind: ScopeKind,
    /// Current state of bindings.
    ///
    /// Ordered, because `live_bindings_for_return` walks it to build the drop
    /// schedule, and the order drops are emitted in reaches the emitted code.
    current_state: BindingMap<BindingState>,
    /// Initialization state for Out params.
    out_param_init: BindingMap<OutParamInitState>,
    /// Where each binding was moved, for error reporting.
    moved_at: BindingMap<ExprKey<'db>>,
    /// Where each binding was last given a value by `set`, for error reporting.
    ///
    /// Keyed by the assigned expression, which is what there is a span for.
    assigned_at: BindingMap<ExprKey<'db>>,
}

/// How one branch of an `if` or `match` left the bindings that outlive it.
struct BranchEnd<'db> {
    /// Where the branch is, in a diagnostic, e.g. "in the then branch".
    describe: String,
    /// Where to make it agree with the others, which is somewhere else when
    /// the branch is an `else` that was not written.
    fix_in: String,
    state: BindingMap<BindingState>,
    moved_at: BindingMap<ExprKey<'db>>,
    assigned_at: BindingMap<ExprKey<'db>>,
}

/// Which names declared outside the analyzed body are consts.
///
/// A const is borrowed wherever it is named, so moving out of one is an
/// error. A const this analysis declared is a binding it can see; one declared
/// outside it is only a name, and this says which names those are.
#[derive(Clone, Debug)]
enum OuterConsts {
    /// Every name not bound here. A function body sees only its parameters,
    /// what it binds, consts and functions, and a function cannot be named as
    /// a value, so a name it cannot resolve is a const of the module or script.
    AllUnresolved,
    /// These names. A script unit also sees the `let` and `var` bindings of
    /// earlier units, which it has bindings for and may move out of.
    Named(Vec<String>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScopeKind {
    Function,
    /// Script unit top-level scope. Bindings are exported, not dropped.
    ScriptUnit,
    Loop,
    IfThen,
    IfElse,
    MatchArm,
}

impl<'a, 'db> AnalysisCtx<'a, 'db> {
    fn new(
        db: &'db dyn salsa::Database,
        expr_types: &'a ExprTypes<'db>,
        outer_consts: OuterConsts,
        call_targets: &'a CallTargets<'db>,
    ) -> Self {
        Self {
            db,
            expr_types,
            type_params: &[],
            next_binding: 0,
            next_stmt_id: 0,
            bindings: Vec::new(),
            name_to_binding: Default::default(),
            scope_stack: Vec::new(),
            externals: Vec::new(),
            written_externals: Default::default(),
            outer_consts,
            call_targets,
            errors: Vec::new(),
            schedule: DropSchedule::default(),
            loop_exits: Vec::new(),
            walked: Vec::new(),
            arena: None,
        }
    }

    /// The expression at `id` in the body being analyzed.
    fn expr(&self, id: ExprId) -> ExprFun<'db> {
        self.arena.expect("an expression is analyzed inside a body").get(id)
    }

    /// Bind the earlier units' bindings this unit names, in the scope just
    /// entered.
    ///
    /// They take the first binding ids, which lowering skips: it makes no
    /// binding for a name an earlier unit bound. One an earlier unit gave away
    /// starts out moved.
    fn bind_externals(&mut self, externals: Vec<ExternalBinding>) {
        assert!(self.bindings.is_empty(), "externals are bound before anything else");
        for external in &externals {
            let id = self.alloc_binding(external.name.C(), external.ty.C(), external.is_slot, None);
            if external.dead {
                self.set_state(id, BindingState::Moved);
            }
        }
        self.externals = externals;
    }

    /// Whether `id` is the binding of an earlier unit's binding.
    fn is_external(&self, id: BindingId) -> bool {
        (id.0 as usize) < self.externals.len()
    }

    /// Note a write to the binding `id`, if it is an earlier unit's.
    fn note_write(&mut self, id: BindingId) {
        if self.is_external(id) {
            self.written_externals.insert(self.bindings[id.0 as usize].name.C());
        }
    }

    /// Whether `id` is an earlier unit's binding that an earlier unit gave away.
    fn is_dead_external(&self, id: BindingId) -> bool {
        self.externals.get(id.0 as usize).is_some_and(|external| external.dead)
    }

    /// Report using a binding that holds nothing, `moved_at` aside.
    fn report_use_after_move(&mut self, id: BindingId, expr_key: ExprKey<'db>) {
        let name = self.bindings[id.0 as usize].name.C();
        if self.is_dead_external(id) && self.get_moved_at(id).is_none() {
            let recovery_hint = OwnershipRecoveryHint::InsertAdapt {
                description: format!("clone `{}` where it was given away", name),
            };
            self.errors.push(AnalysisError::UseAfterMoveInEarlierUnit { expr_key, name, recovery_hint });
            return;
        }
        let moved_at = self.get_moved_at(id).unwrap_or(expr_key);
        let recovery_hint = OwnershipRecoveryHint::InsertAdapt {
            description: format!("clone `{}` before the earlier use", name),
        };
        self.errors.push(AnalysisError::UseAfterMove { expr_key, moved_at, name, recovery_hint });
    }

    /// What this unit did to the earlier units' bindings it names, as of the
    /// scope it is still in: the names it gave away, and the names that held
    /// nothing and that it gave a value again.
    fn external_outcome(&self) -> (Vec<String>, Vec<String>) {
        let mut killed = Vec::new();
        let mut revived = Vec::new();
        for (index, external) in self.externals.iter().enumerate() {
            let moved = self.get_state(BindingId(index as u32)) == Some(BindingState::Moved);
            match (external.dead, moved) {
                (false, true) => killed.push(external.name.C()),
                (true, false) => revived.push(external.name.C()),
                _ => {}
            }
        }
        (killed, revived)
    }

    /// Whether a name this analysis has no binding for is a const.
    fn is_outer_const(&self, name: &str) -> bool {
        match &self.outer_consts {
            OuterConsts::AllUnresolved => true,
            OuterConsts::Named(names) => names.iter().any(|n| n == name),
        }
    }

    /// Report a move out of a const.
    fn move_out_of_const(&mut self, name: &str, expr_key: ExprKey<'db>) {
        let recovery_hint = OwnershipRecoveryHint::InsertAdapt {
            description: format!("clone `{}`", name),
        };
        self.errors.push(AnalysisError::CannotMoveConst {
            expr_key,
            name: name.to_string(),
            recovery_hint,
        });
    }

    /// The names this unit exports that hold no value.
    fn dead_exports(&self) -> Vec<String> {
        let mut dead = Vec::new();
        for (id, info) in self.bindings.iter().enumerate().skip(self.externals.len()) {
            if !info.is_script_unit || info.ty.is_copy() || info.is_borrowed() {
                continue;
            }
            if self.get_state(BindingId(id as u32)) == Some(BindingState::Moved) {
                dead.push(info.name.clone());
            }
        }
        dead
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
        self.alloc_binding_inner(name, ty, is_slot, param_mode, false)
    }

    /// Allocate a binding for a const, which is borrowed wherever it is named.
    fn alloc_const_binding(&mut self, name: String, ty: IrType) -> BindingId {
        self.alloc_binding_inner(name, ty, false, None, true)
    }

    /// Allocate a binding for a const parameter, which is passed by reference,
    /// so the body does not own it and does not drop it.
    fn alloc_const_param(&mut self, name: String, ty: IrType) -> BindingId {
        self.alloc_binding_inner(name, ty, false, Some(ParamMode::Ref), true)
    }

    fn alloc_binding_inner(&mut self, name: String, ty: IrType, is_slot: bool, param_mode: Option<ParamMode>, is_const: bool) -> BindingId {
        let id = BindingId(self.next_binding);
        self.next_binding += 1;

        // Check if we're directly in ScriptUnit scope.
        let is_script_unit = self.scope_stack.last()
            .map(|f| f.kind == ScopeKind::ScriptUnit)
            .unwrap_or(false);

        self.bindings.push(BindingInfo { name: name.C(), ty, is_slot, is_script_unit, param_mode, is_const });

        // Record in current scope.
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.bindings.push(id);
            frame.current_state.insert(id, BindingState::Live);

            // Out params start uninitialized.
            if param_mode == Some(ParamMode::Out) {
                frame.out_param_init.insert(id, OutParamInitState::Uninitialized);
            }
        }

        // Add to name mapping, remembering what the name meant before so
        // that leaving the scope can put it back.
        let previous = self.name_to_binding.insert(name.C(), id);
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.shadowed.push((name, previous));
        }

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
        let assigned_at = self.scope_stack.last()
            .map(|f| f.assigned_at.C())
            .unwrap_or_default();

        self.scope_stack.push(ScopeFrame {
            bindings: Vec::new(),
            shadowed: Vec::new(),
            kind,
            current_state,
            out_param_init,
            moved_at,
            assigned_at,
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

        // Put each name back to what it meant before this scope, in reverse
        // so that a name bound more than once here ends at the outer one.
        for (name, previous) in frame.shadowed.iter().rev() {
            match previous {
                Some(id) => { self.name_to_binding.insert(name.C(), *id); }
                None => { self.name_to_binding.remove(name); }
            }
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
            for (id, expr_key) in frame.assigned_at {
                if parent.current_state.contains_key(&id) {
                    parent.assigned_at.insert(id, expr_key);
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

    /// Record the state a `break` or `continue` leaves the innermost loop with.
    ///
    /// The drops this exit needs are already scheduled against it, so what is
    /// kept here is only what the loop has to know afterwards: which bindings it
    /// took with it. Bindings the loop body declared are left out -- they do not
    /// exist past the loop, and the exit dropped them on its way.
    fn record_loop_exit(&mut self, is_continue: bool) {
        let state = self.scope_stack.last()
            .map(|frame| frame.current_state.C())
            .unwrap_or_default();
        let inits = self.scope_stack.last()
            .map(|frame| frame.out_param_init.C())
            .unwrap_or_default();
        let exits = self.loop_exits.last_mut()
            .expect("a `break` or `continue` outside a loop is F050/F051, which typecheck refuses");
        if is_continue {
            exits.continues.push(state);
        } else {
            exits.breaks.push(state);
            exits.break_inits.push(inits);
        }
    }

    /// How the innermost scope has the bindings, for comparing branches.
    fn branch_end(&self, describe: impl Into<String>) -> BranchEnd<'db> {
        let frame = self.scope_stack.last().expect("a branch is analyzed inside a scope");
        let describe = describe.into();
        BranchEnd {
            fix_in: describe.C(),
            describe,
            state: frame.current_state.C(),
            moved_at: frame.moved_at.C(),
            assigned_at: frame.assigned_at.C(),
        }
    }

    /// Get where a binding was moved.
    fn get_moved_at(&self, id: BindingId) -> Option<ExprKey<'db>> {
        self.scope_stack.last()?.moved_at.get(&id).copied()
    }

    /// Record where a binding was moved.
    fn set_moved_at(&mut self, id: BindingId, expr_key: ExprKey<'db>) {
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.moved_at.insert(id, expr_key);
        }
    }

    /// Look up a binding by name.
    fn lookup(&self, name: &str) -> Option<BindingId> {
        self.name_to_binding.get(name).copied()
    }

    /// What a place is rooted in: the binding of the name it starts from,
    /// or that name, where an earlier script unit made the binding.
    fn place_root(&self, expr: ExprFun<'db>) -> Option<Walked> {
        match expr.kind() {
            ExprFunKind::Place(place) => Some(self.walked_root(place.root.text(self.db))),
            ExprFunKind::FieldProj(proj) => self.place_root(self.expr(proj.base)),
            _ => None,
        }
    }

    /// What a name a place starts from is, as a `for` would walk it.
    fn walked_root(&self, name: &str) -> Walked {
        match self.lookup(name) {
            Some(id) => Walked::Binding(id),
            None => Walked::Earlier(name.S()),
        }
    }

    /// Refuse a change to a collection a `for` is walking, saying whether it
    /// was one.
    fn check_not_walked(&mut self, root: Walked, expr_key: ExprKey<'db>) -> bool {
        let Some(name) = self.walked_name(root) else {
            return false;
        };
        self.errors.push(AnalysisError::WalkedCollectionChanged { expr_key, name });
        true
    }

    /// The name of a collection a `for` is walking, or none if it is not one.
    fn walked_name(&self, root: Walked) -> Option<String> {
        if !self.walked.contains(&root) {
            return None;
        }
        Some(match root {
            Walked::Binding(id) => self.bindings[id.0 as usize].name.C(),
            Walked::Earlier(name) => name,
        })
    }

    /// Mark a binding as moved at the given expression.
    fn mark_moved(&mut self, id: BindingId, expr_key: ExprKey<'db>) {
        if self.check_not_walked(Walked::Binding(id), expr_key) {
            return;
        }
        // Borrowed params (Ref/Mut) cannot be moved - caller retains ownership.
        if self.bindings[id.0 as usize].is_borrowed() {
            let name = self.bindings[id.0 as usize].name.C();
            self.errors.push(AnalysisError::CannotMoveBorrowed { expr_key, name });
            return;
        }

        if self.get_state(id) == Some(BindingState::Moved) {
            // Double move error - can be recovered by cloning before the second move.
            let name = self.bindings[id.0 as usize].name.C();
            let moved_at = self.get_moved_at(id).unwrap_or(expr_key);
            let recovery_hint = OwnershipRecoveryHint::InsertAdapt {
                description: format!("clone `{}` before the second use", name),
            };
            self.errors.push(AnalysisError::DoubleMove { expr_key, moved_at, name, recovery_hint });
        } else {
            // Track the move for error detection.
            // Note: ScriptUnit bindings are tracked for error detection (e.g., move in loop)
            // but are NOT scheduled for drops since they're exported.
            self.set_state(id, BindingState::Moved);
            self.set_moved_at(id, expr_key);
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
        self.schedule.stmt_order.push(StmtKey::from_stmt(stmt));
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
                    // A copy owns nothing, and a borrowed binding -- a `ref`
                    // parameter, a `for`'s element -- is someone else's to drop.
                    if !info.ty.is_copy() && !info.is_borrowed() {
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
        let mut seen = vec![false; self.bindings.len()];
        for frame in self.scope_stack.iter().rev() {
            // For ScriptUnit, don't include its bindings (they're exported).
            // Just stop here without adding them.
            if frame.kind == ScopeKind::ScriptUnit {
                break;
            }

            // Include bindings defined in this frame.
            for &id in &frame.bindings {
                // Mark as seen first - inner scopes have the most up-to-date state.
                if std::mem::replace(&mut seen[id.0 as usize], true) {
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
            // Also include bindings from parent scopes that are tracked here,
            // but not an earlier unit's: that unit owns it.
            for (id, &state) in &frame.current_state {
                // Mark as seen first - inner scopes have the most up-to-date state.
                if std::mem::replace(&mut seen[id.0 as usize], true) || self.is_external(id) {
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
        self.expr_types.get(expr)
            .map(|ty| self.ir_type(ty))
            .unwrap_or(IrType::Unit)
    }

    /// The IR type of a typechecker type in the function being analyzed.
    fn ir_type(&self, ty: &Type<'db>) -> IrType {
        match ty {
            Type::Datalit(ty) => IrType::from_datalit_generic(self.db, ty, self.type_params),
            Type::Function(_) => todo!("function types in IR"),
        }
    }

    /// If the expression is a simple name, return its binding ID.
    fn expr_to_binding(&self, expr: ExprFun<'db>) -> Option<BindingId> {
        if let ExprFunKind::Place(place) = expr.kind() {
            if place.steps.is_empty() {
                let name: &str = place.root.text(self.db).as_ref();
                return self.name_to_binding.get(name).copied();
            }
        }
        None
    }

    /// The binding an expression's place is rooted at, ignoring any steps.
    ///
    /// Unlike `expr_to_binding` this answers for projections and indexes too:
    /// `a.x` and `a[i]?` are both rooted at `a`.
    fn expr_to_place_root(&self, expr: ExprFun<'db>) -> Option<BindingId> {
        if let ExprFunKind::Place(place) = expr.kind() {
            let name: &str = place.root.text(self.db).as_ref();
            return self.name_to_binding.get(name).copied();
        }
        None
    }

    /// True if a binding can be assigned through: a `var` slot, or a `mut` or
    /// `out` parameter. `let` bindings and `in`/`ref` parameters cannot.
    fn binding_is_mutable(&self, id: BindingId) -> bool {
        let info = &self.bindings[id.0 as usize];
        info.is_slot || matches!(info.param_mode, Some(ParamMode::Mut) | Some(ParamMode::Out))
    }

    /// Reject an argument to a `mut` or `out` parameter that the caller cannot
    /// mutate.
    ///
    /// Parameters are passed by reference, so the callee writes into whatever
    /// the argument denotes. That must be a place the caller may assign to,
    /// otherwise `let` bindings are mutable in fact while immutable by
    /// declaration, and writes through a temporary are silently discarded.
    fn check_mutable_argument(&mut self, arg: ExprFun<'db>) {
        let expr_key = arg.key();

        let ExprFunKind::Place(place) = arg.kind() else {
            self.errors.push(AnalysisError::CannotMutateTemporary { expr_key });
            return;
        };
        if self.check_not_walked(self.walked_root(place.root.text(self.db)), expr_key) {
            return;
        }
        let Some(root) = self.expr_to_place_root(arg) else {
            return;
        };
        if self.binding_is_mutable(root) {
            self.note_write(root);
            return;
        }

        let name = self.bindings[root.0 as usize].name.C();
        // A ref parameter gets the more specific message.
        if self.bindings[root.0 as usize].param_mode == Some(ParamMode::Ref) {
            self.errors.push(AnalysisError::CannotMutFromRef { expr_key, name });
        } else {
            self.errors.push(AnalysisError::CannotMutateImmutable { expr_key, name });
        }
    }

    /// Reject calls where two arguments share a place root and at least one of
    /// them is passed to a `mut` or `out` parameter.
    ///
    /// Every parameter is passed by reference, so two such arguments hand the
    /// callee two references to one object with at least one of them mutable.
    /// `string_push_str(mut self, ref other)` called as `push_str(s, s)`
    /// reallocates `self` and then reads `other` through the freed pointer.
    ///
    /// Comparison is on the root binding alone. Distinct fields of one value
    /// (`f(mut p.x, ref p.y)`) do not overlap in fact, and distinct indexes
    /// (`f(mut a[i]?, ref a[j]?)`) may or may not, but neither is admitted
    /// yet: rejecting is sound, and nothing in the standard library or the
    /// fixtures relies on either.
    ///
    /// Arguments that are not places cannot alias: a nested call is evaluated
    /// to a value before this call runs.
    fn check_argument_aliasing(&mut self, args: &[ExprFun<'db>], callee_modes: &[ParamMode]) {
        let is_borrow_conflict = |mode: Option<ParamMode>| {
            matches!(mode, Some(ParamMode::Mut) | Some(ParamMode::Out))
        };

        for (j, later) in args.iter().enumerate() {
            let Some(later_root) = self.expr_to_place_root(*later) else {
                continue;
            };
            for (i, earlier) in args.iter().enumerate().take(j) {
                if self.expr_to_place_root(*earlier) != Some(later_root) {
                    continue;
                }
                let modes = (callee_modes.get(i).copied(), callee_modes.get(j).copied());
                if !is_borrow_conflict(modes.0) && !is_borrow_conflict(modes.1) {
                    continue;
                }
                let name = self.bindings[later_root.0 as usize].name.C();
                let expr_key = later.key();
                self.errors.push(AnalysisError::AliasedMutableArgument { expr_key, name });
                break;
            }
        }
    }

    /// Check if an expression contains early-return operators.
    fn expr_may_early_return(&self, expr: ExprFun<'db>) -> bool {
        match expr.kind() {
            ExprFunKind::TryOption(_) | ExprFunKind::TryResult(_) => true,
            ExprFunKind::BinOp(binop) => {
                let op_may_return = matches!(
                    binop.op,
                    BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional |
                    BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked
                );
                op_may_return
                    || self.expr_may_early_return(self.expr(binop.lhs))
                    || self.expr_may_early_return(self.expr(binop.rhs))
            }
            ExprFunKind::UnaryOp(unary) => {
                let op_may_return = matches!(
                    unary.op,
                    UnaryOp::NegOptional | UnaryOp::NegResult
                );
                op_may_return || self.expr_may_early_return(self.expr(unary.operand))
            }
            ExprFunKind::FunctionCall(call) => {
                call.args.iter().any(|&arg| self.expr_may_early_return(expr.at(arg)))
            }
            ExprFunKind::IntrinsicCall(icall) => {
                icall.args.iter().any(|&arg| self.expr_may_early_return(self.expr(arg)))
            }
            ExprFunKind::Tuple(tuple) => {
                tuple.elements.iter().any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::List(list) => {
                list.elements.iter().any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::Set(set) => {
                set.elements.iter().any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::Map(map) => {
                map.entries.iter().any(|entry| {
                    self.expr_may_early_return(self.expr(entry.key))
                        || self.expr_may_early_return(self.expr(entry.value))
                })
            }
            ExprFunKind::Tensor(tensor) => {
                tensor.elements.iter().any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::Table(table) => {
                table.rows.iter()
                    .flat_map(|row| &row.elements)
                    .any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::AnonTuple(tuple) => {
                tuple.elements.iter().any(|elem| self.expr_may_early_return(self.expr(*elem)))
            }
            ExprFunKind::AnonStruct(s) => {
                s.fields.iter().any(|f| self.expr_may_early_return(self.expr(f.value)))
            }
            ExprFunKind::Some(s) => self.expr_may_early_return(self.expr(s.payload)),
            ExprFunKind::Ok(o) => self.expr_may_early_return(self.expr(o.payload)),
            ExprFunKind::Er(e) => self.expr_may_early_return(self.expr(e.payload)),
            ExprFunKind::Data(d) => self.expr_may_early_return(self.expr(d.value)),
            ExprFunKind::Error(e) => self.expr_may_early_return(self.expr(e.value)),
            ExprFunKind::Term(t) => self.expr_may_early_return(self.expr(t.payload)),
            ExprFunKind::EnumLiteral(lit) => self.expr_may_early_return(self.expr(lit.variant)),
            ExprFunKind::FieldProj(proj) => self.expr_may_early_return(self.expr(proj.base)),
            ExprFunKind::Index(idx) => {
                self.expr_may_early_return(self.expr(idx.base))
                    || self.expr_may_early_return(self.expr(idx.index))
            }
            ExprFunKind::Place(place) => {
                // Place expressions with index steps always may early-return.
                place.steps.iter().any(|s| matches!(s, datalove_datafun_ast::ast::PlaceStep::Index(_)))
            }
            ExprFunKind::CloneCoerce(cc) => self.expr_may_early_return(self.expr(cc.operand)),
            ExprFunKind::Hinted(h) => self.expr_may_early_return(self.expr(h.inner)),
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
    /// Check that a binding read at `expr_key` has not been moved out of.
    ///
    /// Reports a use after move and gives false if it has.
    fn check_not_moved(&mut self, id: BindingId, expr_key: ExprKey<'db>) -> bool {
        if self.get_state(id) != Some(BindingState::Moved) {
            return true;
        }
        self.report_use_after_move(id, expr_key);
        false
    }

    fn analyze_expr_moves(&mut self, expr: ExprFun<'db>, is_consumed: bool) -> Option<BindingId> {
        let expr_key = expr.key();
        match expr.kind() {
            ExprFunKind::BinOp(binop) => {
                // Binary ops read their operands, not consume them.
                self.analyze_expr_moves(self.expr(binop.lhs), false);
                self.analyze_expr_moves(self.expr(binop.rhs), false);
                None
            }
            ExprFunKind::UnaryOp(unary) => {
                // Unary ops read their operand, not consume it.
                self.analyze_expr_moves(self.expr(unary.operand), false);
                None
            }
            ExprFunKind::FunctionCall(call) => {
                // Every argument carries its mode, and typechecking has already
                // established that each marker matches the callee's declared
                // mode, so the call site alone says how each argument is passed.
                // An absent marker denotes `in`.
                let callee_modes: Vec<ParamMode> = call.arg_modes.iter()
                    .map(|m| m.unwrap_or(ParamMode::In))
                    .collect();

                let args: Vec<ExprFun<'db>> = call.args.iter().map(|&arg| expr.at(arg)).collect();
                self.check_argument_aliasing(&args, &callee_modes);

                // A const argument is not passed: specialization writes its
                // value into the callee. So it is not a move, and the argument
                // is the bare const name the typechecker required.
                let target = self.call_targets.get_call(expr, call)
                    .expect("ownership analysis runs only on a typechecked body, where every call is resolved");
                let callee_comptime: Vec<bool> = target.func().params().iter()
                    .map(|p| p.is_comptime)
                    .collect();

                // Analyze args with appropriate consumption based on param mode.
                for (i, arg) in args.iter().enumerate() {
                    if callee_comptime[i] {
                        continue;
                    }
                    let callee_mode = callee_modes.get(i).copied();

                    // The callee writes through a mut or out parameter, so the
                    // argument has to be something the caller can mutate.
                    if matches!(callee_mode, Some(ParamMode::Mut) | Some(ParamMode::Out)) {
                        self.check_mutable_argument(*arg);
                    }

                    // For Out params: the callee writes to the arg, so this is NOT a read.
                    // Skip the uninitialized Out param check and mark as initialized after.
                    if callee_mode == Some(ParamMode::Out) {
                        // For Out args, we only need to check use-after-move, not uninitialized.
                        if let Some(binding_id) = self.expr_to_binding(*arg) {
                            if self.get_state(binding_id) == Some(BindingState::Moved) {
                                self.report_use_after_move(binding_id, arg.key());
                            }
                            // The call is the write, so whatever was waiting
                            // for one has had it. Asking whether the argument
                            // is itself an `out` parameter only counted the
                            // case of forwarding one into another; the usual
                            // argument is an uninitialized `var`, which is
                            // tracked the same way and was left unwritten.
                            if self.get_out_param_init(binding_id).is_some() {
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
            ExprFunKind::IntrinsicCall(icall) => {
                // Every intrinsic takes copy scalars and reads them, but an
                // argument can still move something, as a call that takes a
                // binding does. Reaching the fallthrough below left such a
                // move unseen, and the binding was dropped a second time.
                for &arg in &icall.args {
                    self.analyze_expr_moves(self.expr(arg), false);
                }
                None
            }
            ExprFunKind::Tuple(tuple) => {
                // Tuple elements are consumed.
                for elem in &tuple.elements {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::TryOption(try_opt) => {
                // Try operand is consumed.
                self.analyze_expr_moves(self.expr(try_opt.operand), true);
                None
            }
            ExprFunKind::TryResult(try_res) => {
                // Try operand is consumed.
                self.analyze_expr_moves(self.expr(try_res.operand), true);
                None
            }
            ExprFunKind::CloneCoerce(cc) => {
                // Clone/coerce reads its operand (creates a clone), doesn't consume it.
                // The original value remains valid after @.
                self.analyze_expr_moves(self.expr(cc.operand), false);
                None
            }
            ExprFunKind::List(list) => {
                // List elements are consumed.
                for elem in &list.elements {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::Set(set) => {
                // Set elements are consumed.
                for elem in &set.elements {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::Map(map) => {
                // Map entries are consumed.
                for entry in &map.entries {
                    self.analyze_expr_moves(self.expr(entry.key), true);
                    self.analyze_expr_moves(self.expr(entry.value), true);
                }
                None
            }
            ExprFunKind::Tensor(tensor) => {
                // Tensor elements are consumed.
                for elem in &tensor.elements {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::Table(table) => {
                // Table cells are consumed, row by row.
                for elem in table.rows.iter().flat_map(|row| &row.elements) {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::AnonTuple(tuple) => {
                // Tuple elements are consumed.
                for elem in &tuple.elements {
                    self.analyze_expr_moves(self.expr(*elem), true);
                }
                None
            }
            ExprFunKind::AnonStruct(s) => {
                // Struct fields are consumed.
                for field in &s.fields {
                    self.analyze_expr_moves(self.expr(field.value), true);
                }
                None
            }
            ExprFunKind::Some(s) => {
                // Payload is consumed.
                self.analyze_expr_moves(self.expr(s.payload), true);
                None
            }
            ExprFunKind::Ok(o) => {
                // Payload is consumed.
                self.analyze_expr_moves(self.expr(o.payload), true);
                None
            }
            ExprFunKind::Er(e) => {
                // Payload is consumed.
                self.analyze_expr_moves(self.expr(e.payload), true);
                None
            }
            ExprFunKind::Data(d) => {
                // Value is consumed.
                self.analyze_expr_moves(self.expr(d.value), true);
                None
            }
            ExprFunKind::Error(e) => {
                // Value is consumed.
                self.analyze_expr_moves(self.expr(e.value), true);
                None
            }
            ExprFunKind::Term(t) => {
                // A term is its payload under a name, and takes it: `term Foo
                // a` moves `a` in. Reaching the fallthrough below instead left
                // the payload unanalyzed, so nothing was marked moved and both
                // the term and what it was built from were dropped.
                self.analyze_expr_moves(self.expr(t.payload), true);
                None
            }
            ExprFunKind::EnumLiteral(lit) => {
                // An enum literal is the variant it wraps, checked against the
                // enum type, and consumes what that consumes.
                self.analyze_expr_moves(self.expr(lit.variant), is_consumed);
                None
            }
            ExprFunKind::Hinted(h) => {
                // A hint says what the expression under it must be and takes
                // nothing of its own, so the expression moves what it would
                // have moved written without one.
                self.analyze_expr_moves(self.expr(h.inner), is_consumed)
            }

            ExprFunKind::FieldProj(proj) => {
                // Field projection reads from the base, doesn't consume it.
                // The base expression may itself consume values (e.g., try operator).
                self.analyze_expr_moves(self.expr(proj.base), false);
                None
            }
            ExprFunKind::Index(idx) => {
                // Index borrows base (like field projection), borrows index.
                self.analyze_expr_moves(self.expr(idx.base), false);
                self.analyze_expr_moves(self.expr(idx.index), false);
                None
            }
            ExprFunKind::Place(place) => {
                if place.steps.is_empty() {
                    // Zero-step Place: same as old Name — full move tracking.
                    let root_name = place.root.text(self.db);
                    if let Some(id) = self.lookup(root_name) {
                        // Check for reading uninitialized binding (Out param or uninitialized var).
                        if self.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
                            let name = self.bindings[id.0 as usize].name.C();
                            self.errors.push(AnalysisError::ReadUninitialized { expr_key, name });
                            return None;
                        }
                        if !self.check_not_moved(id, expr_key) {
                            return None;
                        }
                        let binding = &self.bindings[id.0 as usize];
                        if is_consumed && !binding.ty.is_copy() {
                            // A const is borrowed wherever it is named, so a
                            // move out of it needs a clone.
                            if binding.is_const {
                                self.move_out_of_const(root_name, expr_key);
                                return None;
                            }
                            self.mark_moved(id, expr_key);
                            return Some(id);
                        }
                    } else {
                        if is_consumed
                            && self.is_outer_const(root_name)
                            && !self.expr_type(expr).is_copy()
                        {
                            self.move_out_of_const(root_name, expr_key);
                        }
                    }
                    None
                } else {
                    // Non-zero steps: root is borrowed, index sub-expressions are borrowed.
                    let root_name = place.root.text(self.db);
                    if let Some(id) = self.lookup(root_name) {
                        self.check_not_moved(id, expr_key);
                    }
                    for step in &place.steps {
                        if let datalove_datafun_ast::ast::PlaceStep::Index(idx) = step {
                            self.analyze_expr_moves(self.expr(idx.index), false);
                        }
                    }
                    None
                }
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
    func: &'db StmtFun<'db>,
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
    resolved_param_types: Option<&[Type<'db>]>,
) -> FunctionAnalysis<'db> {
    // A function body cannot name script-level bindings.
    let mut ctx = AnalysisCtx::new(
        db, expr_types, OuterConsts::AllUnresolved, call_targets,
    );
    ctx.type_params = func.type_params();

    // Enter function scope.
    ctx.enter_scope(ScopeKind::Function);

    // Register parameters as bindings.
    let params = func.params();
    for (i, param) in params.iter().enumerate() {
        let name = param.name.text(db).S();
        // Use resolved type if available, otherwise fall back to AST type hint.
        let ty = match resolved_param_types {
            Some(types) => ctx.ir_type(&types[i]),
            None => IrType::from_type_hint_generic(db, &param.type_hint, ctx.type_params),
        };
        // A const parameter is a constant, and like any other const is
        // borrowed wherever it is named. It arrives by reference.
        if param.is_comptime {
            ctx.alloc_const_param(name, ty);
        } else {
            ctx.alloc_binding(name, ty, false, Some(param.mode));
        }
    }

    // Analyze function body.
    let body = func.body();
    ctx.arena = Some(&body.arena);
    analyze_statements(&mut ctx, &body.statements);

    // The end of the body is a return too, and an out parameter has to have
    // been written by every return there is. Only a written `ret` checked, so
    // a body that simply ended -- which for a function returning nothing is
    // the usual way to write one -- promised an out parameter and delivered
    // nothing, and the caller read whatever was in the slot.
    if datalove_datafun_ast::reachable::body_completes(&body.statements) {
        check_out_params_initialized(&mut ctx, None);
    }

    // Whatever is still owned where the body ends has to be let go there.
    // A `ret` schedules its own against its statement index; a body that runs
    // off the end has no statement to hang them on, so they go in their own
    // place and lowering emits them before the return it adds.
    ctx.schedule.at_function_exit = ctx.exit_scope();

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
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
    stmts: &'db [Statement<'db>],
    func_param_types: Option<&HashMap<String, Vec<Type<'db>>>>,
) -> Result<ScriptFunctionAnalyses<'db>, Vec<(String, Vec<AnalysisError<'db>>)>> {
    let mut analyses = bct::lookup::LookupMap::new();
    let mut errors = Vec::new();

    for stmt in stmts {
        if let Statement::Fun(func) = stmt {
            // Look up resolved param types if available.
            let func_name = func.name().text(db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name))
                .map(|v| v.as_slice());

            let analysis = analyze_function(db, func, expr_types, call_targets, resolved_params);
            if !analysis.errors.is_empty() {
                errors.push((func_name.S(), analysis.errors.C()));
            }
            analyses.insert(func.key(), analysis);
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
pub struct ScriptAnalysis<'db> {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError<'db>>,
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
    /// Names this unit exports that hold no value.
    pub dead_exports: Vec<String>,
    /// Names an earlier unit gave away that this unit assigned to.
    pub revived_exports: Vec<String>,
    /// The earlier units' bindings this unit gave away.
    pub taken_externals: Vec<String>,
    /// The earlier units' bindings this unit wrote to.
    pub written_externals: Vec<String>,
    /// How many of the first bindings are earlier units' bindings, which
    /// lowering makes none of.
    pub externals: u32,
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
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
    body: &'db Body<'db>,
) -> ScriptAnalysis<'db> {
    analyze_script_statements_with_externals(
        db, expr_types, call_targets, body, Vec::new(), Vec::new(),
    )
}

/// Analyze script statements for ownership, given what earlier units left.
///
/// `externals` are the earlier units' bindings the unit names, and
/// `external_consts` the names from earlier units that are consts.
pub fn analyze_script_statements_with_externals<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
    body: &'db Body<'db>,
    externals: Vec<ExternalBinding>,
    external_consts: Vec<String>,
) -> ScriptAnalysis<'db> {
    let mut ctx = AnalysisCtx::new(
        db, expr_types, OuterConsts::Named(external_consts), call_targets,
    );

    // Enter ScriptUnit scope so bindings are Tracked.
    ctx.enter_scope(ScopeKind::ScriptUnit);
    ctx.bind_externals(externals);

    // Analyze statements.
    ctx.arena = Some(&body.arena);
    analyze_statements(&mut ctx, &body.statements);

    // Collect bindings for UnitEndDrop before exiting scope.
    //
    // Live non-Copy bindings, plus every non-Copy slot whatever its state: a
    // later unit can assign to a slot this one gave away, and the cleanup list
    // is fixed now. Slots are Tracked, so both backends check whether the slot
    // holds anything before dropping it.
    let unit_end_bindings: Vec<BindingId> = ctx.scope_stack.last()
        .map(|frame| {
            frame.bindings.iter()
                .filter(|&id| !ctx.is_external(*id))
                .filter(|&id| {
                    let info = &ctx.bindings[id.0 as usize];
                    let state = frame.current_state.get(id).copied();
                    let live_or_assignable = state == Some(BindingState::Live) || info.is_slot;
                    live_or_assignable
                        && !info.ty.is_copy()
                        && !info.is_borrowed()
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();

    // Collect the exports that hold no value while the scope still has the
    // final state of each binding.
    let dead_exports = ctx.dead_exports();
    let (taken_externals, revived_exports) = ctx.external_outcome();
    let externals = ctx.externals.len() as u32;

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
        dead_exports,
        revived_exports,
        taken_externals,
        written_externals: ctx.written_externals.into_iter().collect(),
        externals,
    }
}

/// Result of analyzing a standalone expression.
#[derive(Clone, Debug)]
pub struct ExprAnalysis<'db> {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError<'db>>,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
    /// The earlier units' bindings the expression gave away.
    pub taken_externals: Vec<String>,
    /// The earlier units' bindings the expression wrote to.
    pub written_externals: Vec<String>,
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
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
) -> ExprAnalysis<'db> {
    analyze_expr_with_externals(db, expr, expr_types, call_targets, Vec::new(), Vec::new())
}

/// Analyze an expression for ownership, given what earlier units left.
///
/// See [`analyze_script_statements_with_externals`].
pub fn analyze_expr_with_externals<'db>(
    db: &'db dyn salsa::Database,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
    expr_types: &ExprTypes<'db>,
    call_targets: &CallTargets<'db>,
    externals: Vec<ExternalBinding>,
    external_consts: Vec<String>,
) -> ExprAnalysis<'db> {
    let mut ctx = AnalysisCtx::new(
        db, expr_types, OuterConsts::Named(external_consts), call_targets,
    );

    // Enter a scope for the expression analysis.
    ctx.enter_scope(ScopeKind::Function);
    ctx.bind_externals(externals);

    // The expression's result is the unit's, so it is consumed -- unless it
    // is a bare name of an earlier unit's binding, which the caller reads
    // where it lives.
    let shows_external = match expr.kind() {
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            ctx.lookup(place.root.text(db)).is_some_and(|id| ctx.is_external(id))
        }
        _ => false,
    };
    ctx.arena = Some(expr.arena());
    ctx.analyze_expr_moves(expr, !shows_external);
    let (taken_externals, _) = ctx.external_outcome();

    // Exit scope. No drops needed since expression result is returned.
    let _ = ctx.exit_scope();

    ExprAnalysis {
        errors: ctx.errors,
        bindings: ctx.bindings,
        taken_externals,
        written_externals: ctx.written_externals.into_iter().collect(),
    }
}

// ============================================================================
// Statement analysis
// ============================================================================

/// Analyze statements for ownership tracking.
///
/// Uses globally-unique statement IDs for drop schedule keys to avoid
/// collisions between nested scopes (e.g., breaks in different loops).
fn analyze_statements<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmts: &[Statement<'db>]) {
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
            Statement::For(for_stmt) => {
                analyze_for(ctx, for_stmt, stmt_id);
            }
            Statement::Break(_) => {
                // Drops before break: all live bindings in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_break.insert(stmt_id, drops);
                }
                ctx.record_loop_exit(false);
            }
            Statement::Continue(_) => {
                // Drops before continue: all live bindings in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_continue.insert(stmt_id, drops);
                }
                ctx.record_loop_exit(true);
            }
            Statement::Fun(_) => {
                // Nested functions handled separately.
            }
            Statement::DebugLog(stmt) => {
                // Debuglog borrows its value, so analyze the expression but don't consume it.
                ctx.analyze_expr_moves(ctx.expr(stmt.value), false);
            }
            Statement::TypeAlias(_) => {
                // Type aliases are resolved at typecheck time; nothing to analyze.
            }
            Statement::Const(const_stmt) => {
                // Const bindings are lowered as let bindings and need runtime
                // ownership tracking just like let.
                analyze_const(ctx, const_stmt, stmt_id);
            }
            Statement::Match(match_stmt) => {
                analyze_match(ctx, match_stmt, stmt_id);
            }
            Statement::ExprStatement(stmt) => {
                // Expression statement consumes its args via the function call.
                analyze_leaving_expr(ctx, ctx.expr(stmt.expr), true, stmt_id);
            }
            Statement::Require(_) | Statement::Import(_) | Statement::NativeFun(_) | Statement::ParseError(_) => {
                // No drops.
            }
        }
    }
}

fn analyze_let<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtLet<'db>, stmt_idx: usize) {
    let expr = stmt.value;

    // The expression result is consumed by the binding.
    analyze_leaving_expr(ctx, ctx.expr(expr), true, stmt_idx);

    let ty = ctx.expr_type(ctx.expr(expr));
    alloc_pattern_bindings(ctx, &stmt.binding, ty, false);
}

/// Allocate the bindings a `let` or `var` takes out of a value of type `ty`.
///
/// The value is consumed whole, and each name owns its part from here on.
fn alloc_pattern_bindings<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    binding: &Binding<'db>,
    ty: IrType,
    is_slot: bool,
) {
    let db = ctx.db;
    match Destructure::of(db, binding, &ty) {
        Destructure::Whole(name) => {
            ctx.alloc_binding(name.text(db).S(), ty, is_slot, None);
        }
        Destructure::Fields { names, .. } => {
            for (name, _, field_ty) in names {
                ctx.alloc_binding(name.text(db).S(), field_ty, is_slot, None);
            }
        }
        Destructure::Payload(name, payload_ty) => {
            ctx.alloc_binding(name.text(db).S(), payload_ty, is_slot, None);
        }
        Destructure::Nothing => {}
    }
}

/// Analyze a const statement.
///
/// Const bindings are lowered as let bindings and need the same ownership tracking.
fn analyze_const<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtConst<'db>, stmt_idx: usize) {
    let expr = stmt.value;

    // The expression result is consumed by the binding.
    analyze_leaving_expr(ctx, ctx.expr(expr), true, stmt_idx);

    // Create binding for the const. It is dropped at the end of the scope like
    // a let, but is borrowed wherever it is named.
    let name = stmt.name.text(ctx.db).S();
    let ty = ctx.expr_type(ctx.expr(expr));
    ctx.alloc_const_binding(name, ty);
}

fn analyze_var<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtVar<'db>, stmt_idx: usize) {
    if let Some(expr) = stmt.value {
        // Initialized var. The expression result is consumed by the binding.
        analyze_leaving_expr(ctx, ctx.expr(expr), true, stmt_idx);

        // Create a binding for each name the var binds, each its own slot.
        let ty = ctx.expr_type(ctx.expr(expr));
        alloc_pattern_bindings(ctx, &stmt.binding, ty, true);
    } else {
        let name = stmt.binding.as_name()
            .expect("the parser requires a plain name without a value")
            .text(ctx.db).S();

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

fn analyze_set<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtSet<'db>, stmt_idx: usize) {
    let place = &stmt.target;
    let expr = ctx.expr(stmt.value);
    let compound = stmt.op.is_some();

    // Assigning to a collection a `for` walks, or to any part of it, would
    // free or move what the walk points at.
    if let Some(name) = ctx.walked_name(ctx.walked_root(place.root.text(ctx.db))) {
        ctx.errors.push(AnalysisError::WalkedCollectionAssigned { set_index: stmt.local_index, name });
    }

    // The value comes first. A plain `set` moves it into the place; a compound
    // one reads it as an operand, which borrows.
    let live_before_value = ctx.live_bindings_for_return();
    ctx.analyze_expr_moves(expr, !compound);

    // Then the keys, left to right. A bare index (upsert) consumes its key,
    // which goes into the map if it is absent; with `?` or `!` the key is
    // only looked up, and borrowed.
    let mut keys_may_early_return = false;
    for step in &place.steps {
        if let datalove_datafun_ast::ast::PlaceStep::Index(idx) = step {
            keys_may_early_return |= ctx.expr_may_early_return(ctx.expr(idx.index));
        }
    }

    // An early return from inside the value or a key, or from a checked or
    // optional compound operator, drops what is live once the value has been
    // evaluated: a key that a bare index will consume hasn't been yet. For the
    // operator, which runs after the keys, it's the same, since a compound
    // assignment has no bare index to consume one.
    let op_may_early_return = stmt.op.is_some_and(|op| !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div));
    if ctx.expr_may_early_return(expr) || keys_may_early_return || op_may_early_return {
        let drops = early_return_drops(ctx, stmt_idx);
        schedule_not_yet_moved(ctx, stmt_idx, &live_before_value, &drops);
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    for step in &place.steps {
        if let datalove_datafun_ast::ast::PlaceStep::Index(idx) = step {
            ctx.analyze_expr_moves(ctx.expr(idx.index), idx.error_mode.is_none());
        }
    }

    // Only then is the place reached, so only now does its root have to be
    // there: the value and the keys may have moved it. A compound assignment
    // reads the place, so it has to hold a value too.
    if compound {
        check_compound_target_readable(ctx, stmt);
    } else if !place.steps.is_empty() {
        let expr_key = expr.key();
        if let Some(id) = ctx.lookup(place.root.text(ctx.db)) {
            ctx.check_not_moved(id, expr_key);
        }
    }

    // A failed `?` or `!` on the way to the place returns early with the
    // value and the keys evaluated, and drops what is live then.
    let target_may_early_return = place.steps.iter().any(|s| {
        matches!(s, datalove_datafun_ast::ast::PlaceStep::Index(idx) if idx.error_mode.is_some())
    });
    if target_may_early_return {
        let drops = early_return_drops(ctx, stmt_idx);
        if !drops.is_empty() {
            ctx.schedule.before_set_target_early_return.insert(stmt_idx, drops);
        }
    }

    if let Some(id) = ctx.lookup(place.root.text(ctx.db)) {
        ctx.note_write(id);
    }

    // Set doesn't create a new binding, but the slot is now live again.
    if place.steps.is_empty() {
        // Simple name assignment.
        let name = place.root.text(ctx.db);
        if let Some(id) = ctx.lookup(name) {
            ctx.set_state(id, BindingState::Live);
            if let Some(frame) = ctx.scope_stack.last_mut() {
                frame.assigned_at.insert(id, expr.key());
            }
            if ctx.get_out_param_init(id).is_some() {
                ctx.set_out_param_init(id, OutParamInitState::Initialized);
            }
        }
    } else {
        // Projection/index chain — check root for partial write to uninit.
        let root_name = place.root.text(ctx.db);
        if let Some(id) = ctx.lookup(root_name) {
            if ctx.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
                ctx.errors.push(AnalysisError::OutParamPartialWrite {
                    expr_key: ctx.expr(stmt.value).key(),
                    name: root_name.to_string(),
                });
            }
        }
    }
}

/// Check that the place a compound assignment updates holds a value to read.
///
/// The errors point at the value, the nearest thing the statement has to an
/// expression of its own.
fn check_compound_target_readable<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtSet<'db>) {
    let expr_key = ctx.expr(stmt.value).key();
    let root_name = stmt.target.root.text(ctx.db);
    let Some(id) = ctx.lookup(root_name) else {
        return;
    };
    if ctx.get_out_param_init(id) == Some(OutParamInitState::Uninitialized) {
        let name = ctx.bindings[id.0 as usize].name.C();
        ctx.errors.push(AnalysisError::ReadUninitialized { expr_key, name });
        return;
    }
    ctx.check_not_moved(id, expr_key);
}

/// Report every out parameter that has not been written by this exit.
///
/// `ret_stmt_idx` is the `ret` the check is for, or `None` for the end of the
/// body, which is a return the function did not write down.
fn check_out_params_initialized<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    ret_stmt_idx: Option<usize>,
) {
    for (idx, info) in ctx.bindings.iter().enumerate() {
        if info.param_mode == Some(ParamMode::Out) {
            let id = BindingId(idx as u32);
            if ctx.get_out_param_init(id) != Some(OutParamInitState::Initialized) {
                // The diagnostic carries no location, so a second exit missing
                // the same parameter would only repeat it.
                let reported = ctx.errors.iter().any(|e| matches!(
                    e,
                    AnalysisError::OutParamNotInitialized { name, .. } if *name == info.name
                ));
                if reported {
                    continue;
                }
                let name = info.name.C();
                ctx.errors.push(AnalysisError::OutParamNotInitialized {
                    ret_stmt_idx,
                    name,
                });
            }
        }
    }
}

/// Settle an early return from a `?`, a `!` or a checked operator in statement
/// `stmt_idx`, returning what the function still owns there.
///
/// It leaves the function like a `ret`, so every out parameter must have been
/// written by then: the caller treats its argument as initialized either way.
fn early_return_drops<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt_idx: usize) -> Vec<BindingId> {
    check_out_params_initialized(ctx, Some(stmt_idx));
    ctx.live_bindings_for_return()
}

/// Analyze the moves of `expr`, an operand of statement `stmt_idx`, and if it
/// can return early, schedule what such a return drops.
fn analyze_leaving_expr<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    expr: ExprFun<'db>,
    is_consumed: bool,
    stmt_idx: usize,
) {
    if !ctx.expr_may_early_return(expr) {
        ctx.analyze_expr_moves(expr, is_consumed);
        return;
    }
    let live_before = ctx.live_bindings_for_return();
    ctx.analyze_expr_moves(expr, is_consumed);
    let drops = early_return_drops(ctx, stmt_idx);
    schedule_not_yet_moved(ctx, stmt_idx, &live_before, &drops);
    if !drops.is_empty() {
        ctx.schedule.before_try_return.insert(stmt_idx, drops);
    }
}

/// Schedule the bindings an expression moves, for an early return from inside
/// it to drop if it comes before the move.
///
/// `drops` is what is live once the expression is done, so it leaves out what
/// the expression moved; but a `?` ahead of a move returns while the binding
/// is still owned. Which moves come first depends on where in the expression
/// the return is, so each is scheduled with the place that makes it.
fn schedule_not_yet_moved<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    stmt_idx: usize,
    live_before: &[BindingId],
    drops: &[BindingId],
) {
    let moved: Vec<(u32, BindingId)> = live_before.iter()
        .filter(|id| !drops.contains(id))
        .map(|&id| {
            let moved_at = ctx.get_moved_at(id)
                .expect("a binding live before an expression and not after was moved by it");
            (moved_at.local_index, id)
        })
        .collect();
    if !moved.is_empty() {
        ctx.schedule.try_return_not_yet_moved.insert(stmt_idx, moved);
    }
}

fn analyze_return<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtRet, stmt_idx: usize) {
    // Check that all Out params are initialized before return.
    check_out_params_initialized(ctx, Some(stmt_idx));

    let may_early_return = stmt.value
        .map(|expr| ctx.expr_may_early_return(ctx.expr(expr)))
        .unwrap_or(false);
    let live_before_value = ctx.live_bindings_for_return();

    // Analyze moves in return value if any. The return value is consumed.
    if let Some(expr) = stmt.value {
        ctx.analyze_expr_moves(ctx.expr(expr), true);
    }

    // Check for early return operators AFTER analyzing moves.
    // This ensures bindings consumed by the expression itself aren't dropped.
    if may_early_return {
        let drops = ctx.live_bindings_for_return();
        schedule_not_yet_moved(ctx, stmt_idx, &live_before_value, &drops);
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

fn analyze_if<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtIf<'db>, stmt_idx: usize) {
    // Analyze condition. For regular bool conditions, it's just read.
    // For Option/Result conditions with bindings, it's consumed by the destructure.
    // A condition can leave the function before either branch is reached -- a
    // `?` in it, or a checked overflow -- and what the function still owns has
    // to go with it.
    let has_binding = stmt.then_binding.is_some();
    analyze_leaving_expr(ctx, ctx.expr(stmt.condition), has_binding, stmt_idx);

    // Save state before branches.
    let state_before = ctx.scope_stack.last()
        .map(|f| f.current_state.C())
        .unwrap_or_default();
    let out_param_init_before = ctx.scope_stack.last()
        .map(|f| f.out_param_init.C())
        .unwrap_or_default();
    let before = BranchEnd {
        fix_in: "in an else branch".S(),
        ..ctx.branch_end("when the `if` is skipped")
    };

    // Analyze then branch.
    ctx.enter_scope(ScopeKind::IfThen);

    // If there's a then-binding (if-let), create it.
    if let Some(binding_name) = stmt.then_binding {
        let name = binding_name.text(ctx.db).S();
        let ty = ctx.expr_type(ctx.expr(stmt.condition));
        // The binding type depends on the condition type (unwrap Option/Result).
        let inner_ty = match &ty {
            IrType::Option(inner) => (**inner).C(),
            IrType::Result(inner) => (**inner).C(),
            other => other.C(),
        };
        ctx.alloc_binding(name, inner_ty, false, None);
    }

    analyze_statements(ctx, &stmt.then_body);
    let then_end = ctx.branch_end("in the then branch");
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
    let mut else_end = None;
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
        else_end = Some(ctx.branch_end("in the else branch"));
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

    // Which branches reach the code after the `if` at all. A branch that leaves
    // by any means -- `ret`, `break`, `continue` -- does not, and each of those
    // has somewhere else its state is accounted for: a `ret` at its own drops, a
    // `break` or `continue` in the loop's exit records.
    let then_leaves = !body_completes(&stmt.then_body);
    let else_leaves = stmt.else_body.as_ref().is_some_and(|body| !body_completes(body));

    // Check for inconsistent moves between branches.
    // If a binding is moved in one branch but not the other, that's an error.
    // This ensures drop points are precise - no runtime tracking needed.
    //
    // Only between branches that merge. A branch that left never arrives here,
    // so the two states describe different points in the program and
    // disagreeing about a binding is what they are supposed to do. Comparing
    // them anyway refused
    //
    //     if n == 0
    //       ret 0
    //     end if
    //     debuglog s
    //
    // for every owned `s` live at the `ret`, which is to say most guard
    // clauses anyone writes.
    if !then_leaves && !else_leaves {
        let else_end = else_end.as_ref().unwrap_or(&before);
        let at = ctx.expr(stmt.condition).key();
        check_branches_agree(ctx, stmt_idx, at, &before, &[&then_end, else_end]);
    }

    // Schedule scope exit drops for bindings created in the branches.
    if !then_drops.is_empty() {
        ctx.schedule.then_branch_exit.insert(stmt_idx, then_drops);
    }

    // Update state after convergence.
    //
    // Whichever branch falls through decides what is known afterwards. Both
    // branches must otherwise have the same state for each binding (or an error
    // was reported above), so taking the then branch's is the same as taking
    // the else branch's; it is only when the then branch left that they differ
    // and the else branch's -- or, with no else, the state before the `if` -- is
    // the one that describes the merge.
    let state_after = if then_leaves && !else_leaves {
        &state_after_else
    } else {
        &state_after_then
    };

    if let Some(frame) = ctx.scope_stack.last_mut() {
        for (id, &state) in state_after {
            frame.current_state.insert(id, state);
        }

        // Writing an out parameter in one branch and not the other is not an
        // error in itself: what the parameter has to be is written by the time
        // the function returns, and an `if` is not a return. This used to
        // report here, which refused
        //
        //     if c
        //       set n = 1
        //     end if
        //     set n = 2
        //
        // where the write after the branch is the one that counts. Every exit
        // checks for itself, so all this has to do is say what is known after.
        //
        // Unlike the move state above, this does not need to know which branch
        // returned. Every `ret` checks the out parameters it leaves through, so
        // a branch that returned had them all written, and converging against
        // it already gives the other branch's answer.
        for (id, &then_init) in &out_param_init_after_then {
            let else_init = out_param_init_after_else.get(&id).copied()
                .unwrap_or(OutParamInitState::Uninitialized);
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

/// Report each binding the branches meeting after an `if` or `match` disagree about.
///
/// Whether such a binding is still held after them depends on which branch
/// ran, and its drop has to be placed without knowing. `before` is how things
/// stood on entering the branches; `at` is the condition or input, pointed at
/// when nothing in the branch itself can be.
fn check_branches_agree<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    stmt_idx: usize,
    at: ExprKey<'db>,
    before: &BranchEnd<'db>,
    merging: &[&BranchEnd<'db>],
) {
    for (id, &state_before) in &before.state {
        if ctx.bindings[id.0 as usize].ty.is_copy() {
            continue;
        }
        let state_of = |end: &BranchEnd<'db>| end.state.get(&id).copied().unwrap_or(BindingState::Live);
        let Some(changed) = merging.iter().find(|end| state_of(end) != state_before) else {
            continue;
        };
        let Some(unchanged) = merging.iter().find(|end| state_of(end) == state_before) else {
            continue;
        };

        // Only one thing changes a binding's state in each direction: a move
        // gives it away, a `set` gives it a value again. A site the branch
        // inherited from before it is not the one that did it.
        let gave_away = state_before == BindingState::Live;
        let (sites, sites_before) = match gave_away {
            true => (&changed.moved_at, &before.moved_at),
            false => (&changed.assigned_at, &before.assigned_at),
        };
        let changed_at = sites.get(&id).copied()
            .filter(|site| sites_before.get(&id) != Some(site));
        let moved_before = match gave_away {
            true => None,
            false => before.moved_at.get(&id).copied(),
        };

        ctx.errors.push(AnalysisError::InconsistentBranchMove {
            stmt_idx,
            at,
            name: ctx.bindings[id.0 as usize].name.C(),
            gave_away,
            changed_in: changed.describe.C(),
            unchanged_in: unchanged.describe.C(),
            fix_in: unchanged.fix_in.C(),
            changed_at,
            moved_before,
        });
    }
}

fn analyze_match<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtMatch<'db>, stmt_idx: usize) {
    // Match consumes its input, which can leave the function before any arm
    // is reached.
    analyze_leaving_expr(ctx, ctx.expr(stmt.input), true, stmt_idx);

    // Save state before match arms.
    let state_before = ctx.scope_stack.last()
        .map(|f| f.current_state.C())
        .unwrap_or_default();
    let before = ctx.branch_end("before the `match`");

    let mut arm_ends: Vec<BranchEnd<'db>> = Vec::new();
    // Whether each arm, in the same order, leaves rather than merging after the
    // `match`.
    let mut arm_leaves: Vec<bool> = Vec::new();

    // Analyze each case arm.
    for (arm_idx, case) in stmt.cases.iter().enumerate() {
        // Reset to pre-match state.
        if let Some(frame) = ctx.scope_stack.last_mut() {
            frame.current_state = state_before.C();
        }

        ctx.enter_scope(ScopeKind::MatchArm);

        // For term cases, create the payload binding.
        if let MatchCaseKind::Term { binding, .. } = &case.kind {
            let name = binding.text(ctx.db).S();
            // Get payload type from the input expr type's enum variant.
            // For now, use a placeholder - the typechecker has validated it.
            let input_ty = ctx.expr_type(ctx.expr(stmt.input));
            let variant_name = match &case.kind {
                MatchCaseKind::Term { name, .. } => name.text(ctx.db).to_string(),
                _ => unreachable!(),
            };
            let payload_ty = match &input_ty {
                IrType::Enum(variants) => {
                    variants.iter()
                        .find(|(n, _)| n == &variant_name)
                        .and_then(|(_, payload)| payload.clone())
                        .unwrap_or(IrType::Unit)
                }
                IrType::Term(_, payload) => *payload.clone(),
                _ => IrType::Unit,
            };
            ctx.alloc_binding(name, payload_ty, false, None);
        }

        analyze_statements(ctx, &case.body);
        let case_name = match &case.kind {
            MatchCaseKind::Atom { name } | MatchCaseKind::Term { name, .. } => name.text(ctx.db),
        };
        arm_ends.push(ctx.branch_end(format!("in the `{}` arm", case_name)));
        let arm_drops = ctx.exit_scope();

        if !arm_drops.is_empty() {
            ctx.schedule.match_arm_exit.insert((stmt_idx, arm_idx), arm_drops);
        }

        arm_leaves.push(!body_completes(&case.body));
    }

    // Analyze default arm if present.
    if let Some(default_body) = &stmt.default_body {
        // Reset to pre-match state.
        if let Some(frame) = ctx.scope_stack.last_mut() {
            frame.current_state = state_before.C();
        }

        let default_arm_idx = stmt.cases.len();
        ctx.enter_scope(ScopeKind::MatchArm);
        analyze_statements(ctx, default_body);
        arm_ends.push(ctx.branch_end("in the default arm"));
        let arm_drops = ctx.exit_scope();

        if !arm_drops.is_empty() {
            ctx.schedule.match_arm_exit.insert((stmt_idx, default_arm_idx), arm_drops);
        }

        arm_leaves.push(!body_completes(default_body));
    }

    // Only the arms that fall through meet after the `match`, so only they can
    // disagree, and only their state describes what follows. An arm that left
    // has its state accounted for where it went.
    let merging: Vec<&BranchEnd<'db>> = arm_ends.iter()
        .zip(&arm_leaves)
        .filter(|(_, leaves)| !**leaves)
        .map(|(end, _)| end)
        .collect();

    let at = ctx.expr(stmt.input).key();
    check_branches_agree(ctx, stmt_idx, at, &before, &merging);

    // Update state after match convergence. With every arm leaving, nothing
    // after the `match` runs and any of them will do.
    match merging.first().copied().or_else(|| arm_ends.first()) {
        Some(arm_end) => {
            if let Some(frame) = ctx.scope_stack.last_mut() {
                // The arm's own bindings ended with it.
                for (id, &state) in &arm_end.state {
                    if !state_before.contains_key(&id) {
                        continue;
                    }
                    frame.current_state.insert(id, state);
                }
            }
        }
        None => {
            // No arms - restore pre-match state.
            if let Some(frame) = ctx.scope_stack.last_mut() {
                frame.current_state = state_before;
            }
        }
    }
}

fn analyze_loop<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtLoop<'db>, stmt_idx: usize) {
    // The condition, which was not looked at here at all.
    //
    // It is read rather than consumed, like an `if`'s. And it can leave the
    // function before the body is reached -- `loop while a -? b .< c` leaves
    // where the subtraction goes under -- so what the function still owns has
    // to go with it. Nothing was dropped on that way out.
    if let Some(condition) = stmt.condition {
        analyze_read_before_loop(ctx, ctx.expr(condition), stmt_idx);
    }

    let entry = LoopEntry::of(ctx);
    ctx.loop_exits.push(LoopExits::default());
    ctx.enter_scope(ScopeKind::Loop);

    analyze_statements(ctx, &stmt.body);

    let exits = ctx.loop_exits.pop().expect("unbalanced loop exit records");
    check_loop_repeats(ctx, &stmt.body, &exits, &entry);

    // Drops at end of loop iteration.
    let loop_drops = ctx.exit_scope();
    if !loop_drops.is_empty() {
        ctx.schedule.loop_body_end.insert(stmt_idx, loop_drops);
    }

    merge_loop_exits(ctx, stmt.condition.is_some(), stmt_idx, &exits, &entry);
}

/// Analyze a `for`: a loop that binds references into a collection, and so
/// freezes the collection while it runs.
///
/// The bindings are allocated as a `ref` parameter is, in the order written,
/// which is the order lowering records them in: borrowed, so never moved and
/// never dropped. An index binding is a plain copied value.
fn analyze_for<'db>(ctx: &mut AnalysisCtx<'_, 'db>, stmt: &StmtFor<'db>, stmt_idx: usize) {
    // The collection is read, as a `ref` argument is, and an element step on
    // the way to it -- `for x in ref xss[i]?` -- can leave the function before
    // the walk starts.
    analyze_read_before_loop(ctx, ctx.expr(stmt.iteratee), stmt_idx);

    // What the walk freezes, found before the loop's own names can shadow
    // it: in `for xs in ref xs`, it is the outer `xs`.
    let walked = ctx.place_root(ctx.expr(stmt.iteratee));

    let entry = LoopEntry::of(ctx);
    ctx.loop_exits.push(LoopExits::default());
    ctx.enter_scope(ScopeKind::Loop);

    let coll_ty = ctx.expr_type(ctx.expr(stmt.iteratee));
    let elem_ty = CursorKind::elem_type(&coll_ty);
    let binding_types = match (&coll_ty, stmt.bindings.len()) {
        (IrType::Map(_, value), 2) => vec![(elem_ty, true), ((**value).C(), true)],
        (_, 2) => vec![(IrType::Index, false), (elem_ty, true)],
        _ => vec![(elem_ty, true)],
    };
    for (name, (ty, by_ref)) in stmt.bindings.iter().zip(binding_types) {
        ctx.alloc_binding(name.text(ctx.db).S(), ty, false, by_ref.then_some(ParamMode::Ref));
    }

    ctx.walked.extend(walked.clone());
    analyze_statements(ctx, &stmt.body);
    if walked.is_some() {
        ctx.walked.pop();
    }

    let exits = ctx.loop_exits.pop().expect("unbalanced loop exit records");
    check_loop_repeats(ctx, &stmt.body, &exits, &entry);

    let loop_drops = ctx.exit_scope();
    if !loop_drops.is_empty() {
        ctx.schedule.loop_body_end.insert(stmt_idx, loop_drops);
    }

    // A walk can always run out of elements, on the first look included.
    merge_loop_exits(ctx, true, stmt_idx, &exits, &entry);
}

/// Analyze what a loop reads before its body: a `loop while` condition, or
/// the collection a `for` walks. It is read rather than consumed, and if it
/// can leave the function, what the function still owns goes with it.
fn analyze_read_before_loop<'db>(ctx: &mut AnalysisCtx<'_, 'db>, expr: ExprFun<'db>, stmt_idx: usize) {
    analyze_leaving_expr(ctx, expr, false, stmt_idx);
}

/// What is true as a loop is entered.
struct LoopEntry {
    /// The state the loop is entered with, which is also the state it is left
    /// with when it ends before its body runs, since nothing that changed a
    /// binding here survives the repeat check.
    state_before: BindingMap<BindingState>,
    /// Outer-scope non-copy bindings live on entry. If one is moved in the
    /// body, that's an error, because the body can run more than once.
    outer_live: Vec<BindingId>,
    /// Which out parameters have a value on entry, which is also which have
    /// one when the loop ends before its body runs.
    inits_before: BindingMap<OutParamInitState>,
}

impl LoopEntry {
    fn of(ctx: &AnalysisCtx<'_, '_>) -> LoopEntry {
        let state_before = ctx.scope_stack.last()
            .map(|frame| frame.current_state.C())
            .unwrap_or_default();
        let outer_live = state_before.iter()
            .filter(|(id, state)| {
                **state == BindingState::Live && !ctx.bindings[id.0 as usize].ty.is_copy()
            })
            .map(|(id, _)| id)
            .collect();
        let inits_before = ctx.scope_stack.last()
            .map(|frame| frame.out_param_init.C())
            .unwrap_or_default();
        LoopEntry { state_before, outer_live, inits_before }
    }
}

/// Check for outer-scope bindings moved somewhere that gets back to the loop
/// head, which is the only way a move repeats. The end of the body gets
/// there, and so does every `continue`.
///
/// The `continue` states have to be asked separately, because a branch that
/// leaves is kept out of the merge after an `if`: a move followed by
/// `continue` no longer shows in the state at the end of the body, and
/// missing it would let one value be given away on every iteration.
///
/// A `break` is deliberately not asked. It does not reach the head, so what
/// it moved cannot be moved twice; it is settled at the loop's exit instead.
///
/// The end of the body only counts when control can get there. A body that
/// ends in `break` -- an inner loop followed by one, say -- leaves a state
/// behind that nothing reads, and reading it anyway reported a repeat for a
/// move that happens at most once.
fn check_loop_repeats<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    body: &[Statement<'db>],
    exits: &LoopExits,
    entry: &LoopEntry,
) {
    let body_reaches_head = body_completes(body);
    for id in &entry.outer_live {
        let moved_at_body_end = body_reaches_head
            && ctx.get_state(*id) == Some(BindingState::Moved);
        let moved_at_continue = exits.continues.iter()
            .any(|state| state.get(id) == Some(&BindingState::Moved));
        if moved_at_body_end || moved_at_continue {
            let name = ctx.bindings[id.0 as usize].name.C();
            // The binding is Moved, so mark_moved recorded where.
            let expr_key = ctx.get_moved_at(*id).X();
            let recovery_hint = OwnershipRecoveryHint::InsertAdapt {
                description: format!("clone `{}` inside the loop", name),
            };
            ctx.errors.push(AnalysisError::MoveInLoop { expr_key, name, recovery_hint });
        }
    }
}

/// Settle what is true after a loop from the ways out of it.
///
/// Every `break` is a way out, and a conditional loop can leave by its condition
/// failing as well -- on the first read, so with the state it was entered with.
/// A `ret` is not: it leaves the function and has already dropped what it owned.
///
/// Where the exits agree, that is the answer, and a binding a `break` took with
/// it reads moved afterwards so nothing drops it twice. Where they disagree, the
/// move is still fine -- it cannot have happened more than once -- but there is
/// no state to write down, since whether the binding is still held depends on
/// which exit ran. Drop points here are static, so that is refused.
fn merge_loop_exits<'db>(
    ctx: &mut AnalysisCtx<'_, 'db>,
    can_run_out: bool,
    stmt_idx: usize,
    exits: &LoopExits,
    entry: &LoopEntry,
) {
    let state_before = &entry.state_before;
    let mut exit_states: Vec<&BindingMap<BindingState>> = exits.breaks.iter().collect();
    if can_run_out {
        exit_states.push(state_before);
    }

    // A bare `loop` nothing breaks out of is left by returning or not at all, so
    // there is nothing after it for this to describe.
    if exit_states.is_empty() {
        return;
    }

    // An out parameter has a value after the loop only if every way out left
    // it one: a body that may not run, or a `break` before the `set`, does not.
    let mut exit_inits: Vec<&BindingMap<OutParamInitState>> = exits.break_inits.iter().collect();
    if can_run_out {
        exit_inits.push(&entry.inits_before);
    }
    for id in entry.inits_before.keys() {
        let initialized = exit_inits.iter()
            .all(|inits| inits.get(&id) == Some(&OutParamInitState::Initialized));
        let state = if initialized { OutParamInitState::Initialized } else { OutParamInitState::Uninitialized };
        ctx.set_out_param_init(id, state);
    }

    for id in state_before.keys() {
        if ctx.bindings[id.0 as usize].ty.is_copy() {
            continue;
        }

        let mut states = exit_states.iter()
            .map(|state| state.get(&id).copied().unwrap_or(BindingState::Live));
        let first = states.next().X();

        if states.all(|state| state == first) {
            if let Some(frame) = ctx.scope_stack.last_mut() {
                frame.current_state.insert(id, first);
            }
        } else {
            let name = ctx.bindings[id.0 as usize].name.C();
            ctx.errors.push(AnalysisError::InconsistentLoopExit { stmt_idx, name });
        }
    }
}
