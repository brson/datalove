//! Drop analysis for IR lowering.
//!
//! This module computes precise drop points for values at compile time.
//! It runs on the AST before lowering and produces a `DropSchedule` that
//! lowering consumes to emit Drop instructions at the right places.
//!
//! The analysis handles:
//! - Scope exits (let bindings going out of scope)
//! - Branch convergence (values moved in some paths but not others)
//! - Early returns (return statements, try operators)
//! - Loops (values from previous iterations)

use std::collections::HashMap;
use crate::ast::{
    Statement, StmtFun, StmtLet, StmtVar, StmtSet, StmtRet, StmtIf, StmtLoop,
    ExprFun, ExprFunKind, BinOp, UnaryOp,
};
use crate::Db;
use super::IrType;

/// Pre-computed drop analyses for functions in a script unit.
pub type ScriptFunctionAnalyses<'db> = HashMap<StmtFun<'db>, FunctionDropAnalysis>;

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

/// Information about a binding.
#[derive(Clone, Debug)]
pub struct BindingInfo {
    /// Name of the binding (for debugging).
    pub name: String,
    /// Type of the binding.
    pub ty: IrType,
    /// Whether this binding is a slot (var) vs value (let/param).
    pub is_slot: bool,
    /// Whether this binding is a ScriptUnit top-level binding (exported, never dropped).
    pub is_script_unit: bool,
}

/// Error detected during drop analysis.
#[derive(Clone, Debug)]
pub enum AnalysisError {
    /// Using a value after it was moved.
    UseAfterMove {
        binding: BindingId,
        name: String,
    },
    /// Moving a value multiple times.
    DoubleMove {
        binding: BindingId,
        name: String,
    },
}

/// Drop schedule computed by analysis.
///
/// Keyed by statement/expression identity. During lowering, after processing
/// each AST node, check if there are drops scheduled for it.
#[derive(Clone, Debug, Default)]
pub struct DropSchedule {
    /// Drops to emit after processing a statement.
    /// Key is statement index in the body.
    pub after_stmt: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of a then-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub then_branch_exit: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit at the end of an else-branch before jumping to join.
    /// Key is the StmtIf index in the body.
    pub else_branch_exit: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit before a return statement.
    /// Key is the return statement index in the body.
    pub before_return: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit before TryReturn in checked/optional operators.
    /// Key is the statement index containing the expression with try.
    pub before_try_return: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit at end of loop body before looping back.
    pub loop_body_end: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit before break.
    pub before_break: HashMap<usize, Vec<BindingId>>,

    /// Drops to emit before continue.
    pub before_continue: HashMap<usize, Vec<BindingId>>,
}

/// Result of analyzing a function.
#[derive(Clone, Debug)]
pub struct FunctionDropAnalysis {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
}

/// Context for drop analysis.
struct AnalysisCtx<'db> {
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    /// Next binding ID to allocate.
    next_binding: u32,
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
        db: &'db dyn Db,
        expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    ) -> Self {
        Self {
            db,
            expr_types,
            next_binding: 0,
            bindings: Vec::new(),
            name_to_binding: HashMap::new(),
            scope_stack: Vec::new(),
            errors: Vec::new(),
            schedule: DropSchedule::default(),
        }
    }

    /// Allocate a new binding ID.
    fn alloc_binding(&mut self, name: String, ty: IrType, is_slot: bool) -> BindingId {
        let id = BindingId(self.next_binding);
        self.next_binding += 1;

        // Check if we're directly in ScriptUnit scope.
        let is_script_unit = self.scope_stack.last()
            .map(|f| f.kind == ScopeKind::ScriptUnit)
            .unwrap_or(false);

        self.bindings.push(BindingInfo { name: name.clone(), ty, is_slot, is_script_unit });

        // Record in current scope.
        if let Some(frame) = self.scope_stack.last_mut() {
            frame.bindings.push(id);
            frame.current_state.insert(id, BindingState::Live);
        }

        // Add to name mapping.
        self.name_to_binding.insert(name, id);

        id
    }

    /// Enter a new scope.
    fn enter_scope(&mut self, kind: ScopeKind) {
        // Copy current state from parent scope.
        let current_state = self.scope_stack.last()
            .map(|f| f.current_state.clone())
            .unwrap_or_default();

        self.scope_stack.push(ScopeFrame {
            bindings: Vec::new(),
            kind,
            current_state,
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
                    if !self.bindings[id.0 as usize].ty.is_copy() {
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

    /// Look up a binding by name.
    fn lookup(&self, name: &str) -> Option<BindingId> {
        self.name_to_binding.get(name).copied()
    }

    /// Mark a binding as moved.
    fn mark_moved(&mut self, id: BindingId) {
        // ScriptUnit bindings are never moved - they're exported.
        if self.bindings[id.0 as usize].is_script_unit {
            return;
        }

        if self.get_state(id) == Some(BindingState::Moved) {
            // Double move error.
            let name = self.bindings[id.0 as usize].name.clone();
            self.errors.push(AnalysisError::DoubleMove { binding: id, name });
        } else {
            self.set_state(id, BindingState::Moved);
        }
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
                    if !self.bindings[id.0 as usize].ty.is_copy() {
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
                if frame.current_state.get(&id) == Some(&BindingState::Live) {
                    if !self.bindings[id.0 as usize].ty.is_copy() && seen.insert(id) {
                        result.push(id);
                    }
                }
            }
            // Also include bindings from parent scopes that are tracked here.
            for (&id, &state) in &frame.current_state {
                if state == BindingState::Live {
                    if !self.bindings[id.0 as usize].ty.is_copy() && seen.insert(id) {
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
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        let index = expr_id.index() as usize;
        match self.expr_types.get(index).copied().flatten() {
            Some(ty) => IrType::from_tycheck(self.db, &ty),
            None => IrType::Unit,
        }
    }

    /// Check if an expression contains early-return operators.
    fn expr_may_early_return(&self, expr: ExprFun<'db>) -> bool {
        match expr.expr(self.db) {
            ExprFunKind::TryOption(_) | ExprFunKind::TryResult(_) => true,
            ExprFunKind::BinOp(binop) => {
                let op_may_return = matches!(
                    binop.op(self.db),
                    BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional |
                    BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked
                );
                op_may_return
                    || self.expr_may_early_return(binop.lhs(self.db))
                    || self.expr_may_early_return(binop.rhs(self.db))
            }
            ExprFunKind::UnaryOp(unary) => {
                let op_may_return = matches!(
                    unary.op(self.db),
                    UnaryOp::NegOptional | UnaryOp::NegResult
                );
                op_may_return || self.expr_may_early_return(unary.operand(self.db))
            }
            ExprFunKind::FunctionCall(call) => {
                call.args(self.db).iter().any(|arg| self.expr_may_early_return(*arg))
            }
            ExprFunKind::Tuple(tuple) => {
                tuple.elements(self.db).iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::List(list) => {
                list.elements(self.db).iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::Set(set) => {
                set.elements(self.db).iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::Map(map) => {
                map.entries(self.db).iter().any(|entry| {
                    self.expr_may_early_return(entry.key(self.db))
                        || self.expr_may_early_return(entry.value(self.db))
                })
            }
            ExprFunKind::AnonTuple(tuple) => {
                tuple.elements(self.db).iter().any(|elem| self.expr_may_early_return(*elem))
            }
            ExprFunKind::AnonStruct(s) => {
                s.fields(self.db).iter().any(|f| self.expr_may_early_return(f.value(self.db)))
            }
            ExprFunKind::Some(s) => self.expr_may_early_return(s.payload(self.db)),
            ExprFunKind::Ok(o) => self.expr_may_early_return(o.payload(self.db)),
            ExprFunKind::Er(e) => self.expr_may_early_return(e.payload(self.db)),
            ExprFunKind::Data(d) => self.expr_may_early_return(d.value(self.db)),
            ExprFunKind::Err(e) => self.expr_may_early_return(e.value(self.db)),
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
        match expr.expr(self.db) {
            ExprFunKind::Name(name) => {
                let name_str = name.text(self.db);
                if let Some(id) = self.lookup(name_str) {
                    // Check for use after move.
                    if self.get_state(id) == Some(BindingState::Moved) {
                        let name = self.bindings[id.0 as usize].name.clone();
                        self.errors.push(AnalysisError::UseAfterMove { binding: id, name });
                        return None;
                    }
                    if is_consumed && !self.bindings[id.0 as usize].ty.is_copy() {
                        // This is a move.
                        self.mark_moved(id);
                        return Some(id);
                    }
                }
                None
            }
            ExprFunKind::BinOp(binop) => {
                // Binary ops read their operands, not consume them.
                self.analyze_expr_moves(binop.lhs(self.db), false);
                self.analyze_expr_moves(binop.rhs(self.db), false);
                None
            }
            ExprFunKind::UnaryOp(unary) => {
                // Unary ops read their operand, not consume it.
                self.analyze_expr_moves(unary.operand(self.db), false);
                None
            }
            ExprFunKind::FunctionCall(call) => {
                // Function args are consumed.
                for arg in call.args(self.db) {
                    self.analyze_expr_moves(*arg, true);
                }
                None
            }
            ExprFunKind::Tuple(tuple) => {
                // Tuple elements are consumed.
                for elem in tuple.elements(self.db) {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::TryOption(try_opt) => {
                // Try operand is consumed.
                self.analyze_expr_moves(try_opt.operand(self.db), true);
                None
            }
            ExprFunKind::TryResult(try_res) => {
                // Try operand is consumed.
                self.analyze_expr_moves(try_res.operand(self.db), true);
                None
            }
            ExprFunKind::List(list) => {
                // List elements are consumed.
                for elem in list.elements(self.db) {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::Set(set) => {
                // Set elements are consumed.
                for elem in set.elements(self.db) {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::Map(map) => {
                // Map entries are consumed.
                for entry in map.entries(self.db) {
                    self.analyze_expr_moves(entry.key(self.db), true);
                    self.analyze_expr_moves(entry.value(self.db), true);
                }
                None
            }
            ExprFunKind::AnonTuple(tuple) => {
                // Tuple elements are consumed.
                for elem in tuple.elements(self.db) {
                    self.analyze_expr_moves(*elem, true);
                }
                None
            }
            ExprFunKind::AnonStruct(s) => {
                // Struct fields are consumed.
                for field in s.fields(self.db) {
                    self.analyze_expr_moves(field.value(self.db), true);
                }
                None
            }
            ExprFunKind::Some(s) => {
                // Payload is consumed.
                self.analyze_expr_moves(s.payload(self.db), true);
                None
            }
            ExprFunKind::Ok(o) => {
                // Payload is consumed.
                self.analyze_expr_moves(o.payload(self.db), true);
                None
            }
            ExprFunKind::Er(e) => {
                // Payload is consumed.
                self.analyze_expr_moves(e.payload(self.db), true);
                None
            }
            ExprFunKind::Data(d) => {
                // Value is consumed.
                self.analyze_expr_moves(d.value(self.db), true);
                None
            }
            ExprFunKind::Err(e) => {
                // Value is consumed.
                self.analyze_expr_moves(e.value(self.db), true);
                None
            }
            ExprFunKind::AnonEnum(e) => {
                // Payload is consumed.
                if let Some(payload) = e.payload(self.db) {
                    self.analyze_expr_moves(payload, true);
                }
                None
            }
            // Literals don't move anything.
            _ => None,
        }
    }
}

/// Analyze a function and compute drop schedule.
pub fn analyze_function<'db>(
    db: &'db dyn Db,
    func: StmtFun<'db>,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
) -> FunctionDropAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types);

    // Enter function scope.
    ctx.enter_scope(ScopeKind::Function);

    // Register parameters as bindings.
    for param in func.params(db) {
        let name = param.name(db).text(db).to_string();
        let ty = IrType::from_type_hint(db, &param.type_hint(db));
        ctx.alloc_binding(name, ty, false);
    }

    // Analyze function body.
    analyze_statements(&mut ctx, func.body(db), &[]);

    // Exit function scope - remaining live bindings need dropping at implicit return.
    let _final_drops = ctx.exit_scope();
    // Note: Final drops are handled by lowering's implicit return path.

    FunctionDropAnalysis {
        errors: ctx.errors,
        schedule: ctx.schedule,
        bindings: ctx.bindings,
    }
}

/// Analyze all functions in a list of statements.
///
/// Returns a map of function analyses, or an error if any function has analysis errors.
/// Call this before lowering to ensure all functions are valid.
pub fn analyze_script_functions<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    stmts: &[Statement<'db>],
) -> Result<ScriptFunctionAnalyses<'db>, Vec<(String, Vec<AnalysisError>)>> {
    let mut analyses = HashMap::new();
    let mut errors = Vec::new();

    for stmt in stmts {
        if let Statement::Fun(func) = stmt {
            let analysis = analyze_function(db, *func, expr_types);
            if !analysis.errors.is_empty() {
                let func_name = func.name(db).text(db).to_string();
                errors.push((func_name, analysis.errors.clone()));
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
pub struct ScriptDropAnalysis {
    /// Errors detected during analysis.
    pub errors: Vec<AnalysisError>,
    /// Computed drop schedule.
    pub schedule: DropSchedule,
    /// Information about each binding (indexed by BindingId).
    pub bindings: Vec<BindingInfo>,
}

/// Analyze script-level statements and compute drop schedule.
///
/// Similar to `analyze_function` but for script units. Key differences:
/// - Enters `ScriptUnit` scope instead of `Function` scope
/// - Top-level bindings are NOT scheduled for drops (they're exported)
/// - Nested scopes (if, loop) get normal drop analysis
pub fn analyze_script_statements<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    stmts: &[Statement<'db>],
) -> ScriptDropAnalysis {
    let mut ctx = AnalysisCtx::new(db, expr_types);

    // Enter script unit scope.
    ctx.enter_scope(ScopeKind::ScriptUnit);

    // Analyze statements.
    analyze_statements(&mut ctx, stmts, &[]);

    // Exit script unit scope. Top-level bindings are NOT dropped (they're exported).
    // The exit_scope call still cleans up the scope frame.
    let _final_drops = ctx.exit_scope();
    // Note: _final_drops will be empty for ScriptUnit scope because top-level
    // bindings are exported. Nested scope drops are scheduled during analysis.

    ScriptDropAnalysis {
        errors: ctx.errors,
        schedule: ctx.schedule,
        bindings: ctx.bindings,
    }
}

/// Analyze a list of statements.
///
/// `stmt_path` is the index path from the root to the current statement list.
fn analyze_statements<'db>(
    ctx: &mut AnalysisCtx<'db>,
    stmts: &[Statement<'db>],
    stmt_path: &[usize],
) {
    for (i, stmt) in stmts.iter().enumerate() {
        let mut current_path = stmt_path.to_vec();
        current_path.push(i);

        match stmt {
            Statement::Let(let_stmt) => {
                analyze_let(ctx, *let_stmt, i);
            }
            Statement::Var(var_stmt) => {
                analyze_var(ctx, *var_stmt, i);
            }
            Statement::Set(set_stmt) => {
                analyze_set(ctx, *set_stmt, i);
            }
            Statement::Ret(ret_stmt) => {
                analyze_return(ctx, *ret_stmt, i);
            }
            Statement::If(if_stmt) => {
                analyze_if(ctx, *if_stmt, i);
            }
            Statement::Loop(loop_stmt) => {
                analyze_loop(ctx, *loop_stmt, i);
            }
            Statement::Break(_) => {
                // Drops before break - only bindings defined in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_break.insert(i, drops);
                }
            }
            Statement::Continue(_) => {
                // Drops before continue - only bindings defined in loop body.
                let drops = ctx.live_bindings_in_scopes(ScopeKind::Loop);
                if !drops.is_empty() {
                    ctx.schedule.before_continue.insert(i, drops);
                }
            }
            Statement::Fun(_) => {
                // Nested functions handled separately.
            }
            Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No drops.
            }
        }
    }
}

fn analyze_let<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtLet<'db>, stmt_idx: usize) {
    let expr = stmt.value(ctx.db);

    // Check for early return operators.
    if ctx.expr_may_early_return(expr) {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Analyze moves in the expression. The expression result is consumed by the binding.
    ctx.analyze_expr_moves(expr, true);

    // Create binding for the let.
    let name = stmt.name(ctx.db).text(ctx.db).to_string();
    let ty = ctx.expr_type(expr);
    ctx.alloc_binding(name, ty, false);
}

fn analyze_var<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtVar<'db>, stmt_idx: usize) {
    let expr = stmt.value(ctx.db);

    // Check for early return operators.
    if ctx.expr_may_early_return(expr) {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Analyze moves in the expression. The expression result is consumed by the binding.
    ctx.analyze_expr_moves(expr, true);

    // Create binding for the var (as a slot).
    let name = stmt.name(ctx.db).text(ctx.db).to_string();
    let ty = ctx.expr_type(expr);
    ctx.alloc_binding(name, ty, true);
}

fn analyze_set<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtSet<'db>, stmt_idx: usize) {
    let expr = stmt.value(ctx.db);

    // Check for early return operators.
    if ctx.expr_may_early_return(expr) {
        let drops = ctx.live_bindings_for_return();
        if !drops.is_empty() {
            ctx.schedule.before_try_return.insert(stmt_idx, drops);
        }
    }

    // Analyze moves in the expression. The value is moved into the slot.
    ctx.analyze_expr_moves(expr, true);

    // Set doesn't create a new binding, but the slot is now live again.
    let name = stmt.name(ctx.db).text(ctx.db);
    if let Some(id) = ctx.lookup(name) {
        ctx.set_state(id, BindingState::Live);
    }
}

fn analyze_return<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtRet<'db>, stmt_idx: usize) {
    // Analyze moves in return value if any. The return value is consumed.
    if let Some(expr) = stmt.value(ctx.db) {
        ctx.analyze_expr_moves(expr, true);
    }

    // All live bindings need dropping before return.
    let drops = ctx.live_bindings_for_return();
    if !drops.is_empty() {
        ctx.schedule.before_return.insert(stmt_idx, drops.clone());
    }

    // Mark dropped bindings as Moved so they're not included in scope exit drops.
    for id in drops {
        ctx.set_state(id, BindingState::Moved);
    }
}

fn analyze_if<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtIf<'db>, stmt_idx: usize) {
    // Analyze condition. For regular bool conditions, it's just read.
    // For Option/Result conditions with bindings, it's consumed by the destructure.
    let has_binding = stmt.then_binding(ctx.db).is_some();
    ctx.analyze_expr_moves(stmt.condition(ctx.db), has_binding);

    // Save state before branches.
    let state_before = ctx.scope_stack.last()
        .map(|f| f.current_state.clone())
        .unwrap_or_default();

    // Analyze then branch.
    ctx.enter_scope(ScopeKind::IfThen);

    // If there's a then-binding (if-let), create it.
    if let Some(binding_name) = stmt.then_binding(ctx.db) {
        let name = binding_name.text(ctx.db).to_string();
        let ty = ctx.expr_type(stmt.condition(ctx.db));
        // The binding type depends on the condition type (unwrap Option/Result).
        let inner_ty = match &ty {
            IrType::Option(inner) => (**inner).clone(),
            IrType::Result(inner) => (**inner).clone(),
            other => other.clone(),
        };
        ctx.alloc_binding(name, inner_ty, false);
    }

    analyze_statements(ctx, stmt.then_body(ctx.db), &[]);
    let then_drops = ctx.exit_scope();
    let state_after_then = ctx.scope_stack.last()
        .map(|f| f.current_state.clone())
        .unwrap_or_default();

    // Reset state for else branch.
    if let Some(frame) = ctx.scope_stack.last_mut() {
        frame.current_state = state_before.clone();
    }

    // Analyze else branch.
    let state_after_else = if let Some(else_body) = stmt.else_body(ctx.db) {
        ctx.enter_scope(ScopeKind::IfElse);

        // If there's an else-binding (if-let with else), create it.
        if let Some(binding_name) = stmt.else_binding(ctx.db) {
            let name = binding_name.text(ctx.db).to_string();
            // Else binding gets the error for Result types.
            let ty = IrType::Error;
            ctx.alloc_binding(name, ty, false);
        }

        analyze_statements(ctx, else_body, &[]);
        let else_drops = ctx.exit_scope();

        if !else_drops.is_empty() {
            ctx.schedule.else_branch_exit.insert(stmt_idx, else_drops);
        }

        ctx.scope_stack.last()
            .map(|f| f.current_state.clone())
            .unwrap_or_default()
    } else {
        // No else branch - state unchanged.
        state_before.clone()
    };

    // Compute convergence drops.
    // For each binding that is Live in one branch but Moved in another,
    // schedule a drop on the Live branch.
    let mut then_extra_drops = Vec::new();
    let mut else_extra_drops = Vec::new();

    for (&id, &then_state) in &state_after_then {
        let else_state = state_after_else.get(&id).copied().unwrap_or(BindingState::Live);

        if then_state != else_state {
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
    }
}

fn analyze_loop<'db>(ctx: &mut AnalysisCtx<'db>, stmt: StmtLoop<'db>, stmt_idx: usize) {
    ctx.enter_scope(ScopeKind::Loop);

    analyze_statements(ctx, stmt.body(ctx.db), &[]);

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

    fn parse_function<'db>(db: &'db dyn crate::Db, source_code: &str) -> StmtFun<'db> {
        let source = Source::new(db, source_code.to_string());
        let script = crate::parser::parse_for_test(db, source);
        let statements = script.statements(db);

        for stmt in statements {
            if let Statement::Fun(fun) = stmt {
                return *fun;
            }
        }
        panic!("No function found in source code");
    }

    #[test]
    fn test_simple_function() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(): @u32
    let x = @42
    ret x
end fun
        "#;

        let func = parse_function(db, source);
        let expr_types = &[];
        let analysis = analyze_function(db, func, expr_types);

        assert!(analysis.errors.is_empty());
    }

    #[test]
    fn test_conditional_move() {
        // Full convergence testing is done in interp3 test 120_conditional_move_convergence.
        // This test just verifies the analysis runs without panicking.
        let ref db = crate::Database::default();
        let source = r#"
fun test(cond: @bool): @u32
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
        let analysis = analyze_function(db, func, &[]);

        // No errors expected even without type info.
        assert!(analysis.errors.is_empty());
    }
}
