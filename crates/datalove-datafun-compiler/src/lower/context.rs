//! Lowering context types.
//!
//! - [`FrameState`]: Per-function IR state (blocks, values, slots). Swapped when
//!   entering nested functions to isolate their IR from the parent.
//! - [`LowerCtx`]: Main context combining shared state (db, types) with `FrameState`.
//! - [`ScriptLowerContext`]: Tracks bindings exported from previous script units.

use std::collections::HashMap;
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::{Statement, ExprFun, ExprFunctionCall};
use crate::module_graph::ModuleId;
use datalove_datafun_tycheck::ResolvedCallTarget;
use datalove_datafun_ir::{
    IrType, IrBlock, IrFunction, Operand, ValueId, SlotId, ParamId, BlockId, FuncId,
    FuncRef, Terminator, Instruction, SymbolTable, ExportBinding, IrModuleId, ParamMode,
    ConstValue, TypeRef, SlotDest,
};
use crate::ir_ext::IrTypeExt;
use crate::ownership_analysis::{BindingId, DropSchedule, BindingInfo, TrackingCategory};

/// Compile-time state for building a function's IR.
///
/// Analogous to `Frame` in the interpreter, which holds runtime state.
/// When lowering a nested function, this state is swapped for a fresh
/// instance, then restored after.
pub struct FrameState {
    /// Blocks being built.
    pub blocks: Vec<IrBlock>,
    /// Instructions for current block.
    pub current_instructions: Vec<Instruction>,
    /// Current block being built.
    pub current_block: BlockId,
    /// Block parameters for current block.
    pub current_block_params: Vec<ValueId>,
    /// Next BlockId to allocate.
    pub next_block: u32,
    /// Next ValueId to allocate.
    pub next_value: u32,
    /// Next SlotId to allocate.
    pub next_slot: u32,
    /// Next ParamId to allocate.
    pub next_param: u32,
    /// Mapping from variable names to their operands.
    pub variables: HashMap<String, Operand>,
    /// Mapping from BindingId to Operand (built during lowering).
    pub binding_to_operand: HashMap<BindingId, Operand>,
    /// Reverse mapping from Operand to BindingId (for tracking lookups).
    pub operand_to_binding: HashMap<Operand, BindingId>,
    /// Next BindingId to allocate (must match analysis traversal order).
    pub next_binding_id: u32,
    /// Type for each ParamId.
    pub param_types: Vec<IrType>,
    /// Mode for each ParamId.
    pub param_modes: Vec<ParamMode>,
    /// Type for each ValueId.
    pub value_types: Vec<IrType>,
    /// Type for each SlotId.
    pub slot_types: Vec<IrType>,
    /// Tracking category for each binding (indexed by BindingId).
    pub tracking: Vec<TrackingCategory>,
    /// Binding info from analysis (for looking up names).
    pub binding_info: Vec<BindingInfo>,
    /// Drop schedule from analysis.
    pub drop_schedule: DropSchedule,
    /// Loop context stack for each nested loop.
    pub loop_stack: Vec<LoopLowerContext>,
    /// Whether current block is unreachable (after return/break/continue).
    pub in_unreachable: bool,
    /// Temporary values to drop after the current expression is evaluated.
    pub expr_temps: Vec<(ValueId, IrType)>,
    /// Stack of pending intermediate scopes for nested compound expressions.
    ///
    /// When lowering compound expressions (struct, tuple, list, etc.), we track
    /// intermediate non-Copy values that have been created but not yet consumed.
    /// If an early return happens (via try operator), these must be dropped.
    ///
    /// Each scope corresponds to one compound expression. Scopes are pushed on
    /// entry and popped on completion. On early return, ALL scopes are dropped
    /// from innermost to outermost.
    pub pending_intermediate_scopes: Vec<Vec<ValueId>>,
    /// Next global statement ID (must match analysis traversal order).
    pub next_stmt_id: usize,
    /// Current statement index in the parent body (for drop schedule lookup).
    pub current_stmt_idx: Option<usize>,
}

impl FrameState {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            current_instructions: Vec::new(),
            current_block: BlockId(0),
            current_block_params: Vec::new(),
            next_block: 1, // Block 0 is entry.
            next_value: 0,
            next_slot: 0,
            next_param: 0,
            variables: HashMap::new(),
            binding_to_operand: HashMap::new(),
            operand_to_binding: HashMap::new(),
            next_binding_id: 0,
            param_types: Vec::new(),
            param_modes: Vec::new(),
            value_types: Vec::new(),
            slot_types: Vec::new(),
            tracking: Vec::new(),
            binding_info: Vec::new(),
            drop_schedule: DropSchedule::default(),
            loop_stack: Vec::new(),
            in_unreachable: false,
            expr_temps: Vec::new(),
            pending_intermediate_scopes: Vec::new(),
            next_stmt_id: 0,
            current_stmt_idx: None,
        }
    }
}

impl Default for FrameState {
    fn default() -> Self {
        Self::new()
    }
}

/// Context for a single loop during lowering.
#[derive(Clone, Debug)]
pub struct LoopLowerContext {
    /// Header block (target for continue).
    pub header: BlockId,
    /// Exit block (target for break).
    pub exit: BlockId,
}

/// Context for lowering script units.
///
/// Tracks bindings available from previous units.
#[derive(Clone, Debug, Default)]
pub struct ScriptLowerContext {
    /// Available let bindings: name -> (unit_index, value_id).
    pub values: HashMap<String, (u32, ValueId)>,
    /// Available var bindings: name -> (unit_index, slot_id).
    pub slots: HashMap<String, (u32, SlotId)>,
    /// Types of exported values: name -> type.
    pub value_types: HashMap<String, IrType>,
    /// Types of exported slots: name -> type.
    pub slot_types: HashMap<String, IrType>,
    /// Available functions: name -> (unit_index, func_id).
    pub functions: HashMap<String, (u32, FuncId)>,
    /// Current unit index.
    pub current_unit: u32,
}

impl ScriptLowerContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add exports from a unit to the context.
    ///
    /// When a name is exported, it shadows any previous binding with the same name,
    /// regardless of whether the previous binding was a value or slot.
    pub fn add_exports(
        &mut self,
        unit_index: u32,
        exports: &[(String, ExportBinding)],
        unit_value_types: &[IrType],
        unit_slot_types: &[IrType],
    ) {
        for (name, binding) in exports {
            match binding {
                ExportBinding::Value(v) => {
                    // Remove any slot with the same name to properly shadow.
                    self.slots.remove(name);
                    self.slot_types.remove(name);
                    self.values.insert(name.clone(), (unit_index, *v));
                    // Store the value's type.
                    if let Some(ty) = unit_value_types.get(v.0 as usize) {
                        self.value_types.insert(name.clone(), ty.clone());
                    }
                }
                ExportBinding::Slot(s) => {
                    // Remove any value with the same name to properly shadow.
                    self.values.remove(name);
                    self.value_types.remove(name);
                    self.slots.insert(name.clone(), (unit_index, *s));
                    // Store the slot's type for drop emission in later units.
                    if let Some(ty) = unit_slot_types.get(s.0 as usize) {
                        self.slot_types.insert(name.clone(), ty.clone());
                    }
                }
                ExportBinding::Function(func_id) => {
                    self.functions.insert(name.clone(), (unit_index, *func_id));
                }
            }
        }
    }

}

/// What kind of script unit we're lowering.
pub enum ScriptUnitKind<'db> {
    /// A sequence of statements.
    Fragment(Vec<Statement<'db>>),
    /// A single expression.
    Expr(ExprFun<'db>),
}

/// Main lowering context.
///
/// Contains shared state (database, type info) plus the current [`FrameState`].
/// For script lowering, also tracks unit-level state like exports and functions.
///
/// When lowering nested functions in scripts, `body` is swapped via
/// [`swap_body_state`](Self::swap_body_state) to isolate each function's IR.
pub struct LowerCtx<'db> {
    // Shared/immutable context (from typechecker).
    pub(super) db: &'db dyn salsa::Database,
    /// Expression types from typechecker.
    pub(super) expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    /// Resolved call targets from typechecker, indexed by ExprFunctionCall salsa ID.
    pub(super) call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    /// Map from (salsa ModuleId, func_name) -> (IrModuleId, FuncId).
    pub(super) func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,

    /// Function-local state (swapped when entering nested function).
    pub(super) body: FrameState,

    // Unit-level state (persists across nested functions).
    /// Exports from this unit (only used for script units).
    pub(super) exports: Vec<(String, ExportBinding)>,
    /// Functions defined in this script unit.
    pub(super) functions: Vec<IrFunction>,
    /// Symbol table for function resolution.
    pub(super) symbols: SymbolTable,
    /// Available functions: name -> FuncRef (for resolving calls).
    /// Used for script-local and external unit functions (not module functions).
    pub(super) func_scope: HashMap<String, FuncRef>,
    /// Types of external slots from previous script units, keyed by name.
    pub(super) external_slot_types: HashMap<String, IrType>,
    /// Bindings to drop at unit end (for AOT compilation).
    pub(super) unit_end_drops: Vec<BindingId>,

    // Control flow (saved/restored manually, not part of body swap).
    /// Return type for current function/script (for try operators).
    pub(super) return_type: Option<IrType>,
    /// Whether we're in a script unit (vs function).
    pub(super) is_script_unit: bool,
    /// Const bindings evaluated at compile time: name -> (type, value).
    pub(super) const_bindings: HashMap<String, (IrType, ConstValue)>,
}

/// Empty func_id_map for contexts that don't need module function resolution.
static EMPTY_FUNC_ID_MAP: std::sync::LazyLock<HashMap<(ModuleId, String), (IrModuleId, FuncId)>> =
    std::sync::LazyLock::new(HashMap::new);

impl<'db> LowerCtx<'db> {
    pub fn new(
        db: &'db dyn salsa::Database,
        expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
        call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    ) -> Self {
        Self {
            db,
            expr_types,
            call_targets,
            func_id_map: &EMPTY_FUNC_ID_MAP,
            body: FrameState::new(),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope: HashMap::new(),
            external_slot_types: HashMap::new(),
            unit_end_drops: Vec::new(),
            return_type: None,
            is_script_unit: false,
            const_bindings: HashMap::new(),
        }
    }

    /// Create a context for lowering module functions with call resolution support.
    pub fn new_for_module(
        db: &'db dyn salsa::Database,
        expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
        call_targets: &'db [Option<ResolvedCallTarget<'db>>],
        func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    ) -> Self {
        Self {
            db,
            expr_types,
            call_targets,
            func_id_map,
            body: FrameState::new(),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope: HashMap::new(),
            external_slot_types: HashMap::new(),
            unit_end_drops: Vec::new(),
            return_type: None,
            is_script_unit: false,
            const_bindings: HashMap::new(),
        }
    }

    /// Get the IrType for an expression from the typechecker.
    pub fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
        let expr_id = expr.as_id();
        let index = expr_id.index() as usize;
        match self.expr_types.get(index).cloned().flatten() {
            Some(ty) => IrType::from_tycheck(self.db, &ty),
            None => panic!(
                "Expression must have type from typechecker. Expression ID {} but expr_types.len() = {}",
                index, self.expr_types.len()
            ),
        }
    }

    /// Create a context for lowering a script unit.
    pub fn new_for_script(
        db: &'db dyn salsa::Database,
        expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
        call_targets: &'db [Option<ResolvedCallTarget<'db>>],
        func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
        script_ctx: ScriptLowerContext,
    ) -> Self {
        // Seed variables with external bindings from previous units.
        let mut variables = HashMap::new();
        for (name, (unit, value)) in &script_ctx.values {
            variables.insert(name.clone(), Operand::ExternalValue {
                unit: *unit,
                value: *value,
            });
        }
        for (name, (unit, slot)) in &script_ctx.slots {
            variables.insert(name.clone(), Operand::ExternalSlot {
                unit: *unit,
                slot: *slot,
            });
        }

        // Seed function scope with external functions from previous units.
        let mut func_scope = HashMap::new();
        for (name, (unit, func_id)) in &script_ctx.functions {
            func_scope.insert(name.clone(), FuncRef::External {
                unit: *unit,
                func: *func_id,
            });
        }

        let mut body = FrameState::new();
        body.variables = variables;

        Self {
            db,
            expr_types,
            call_targets,
            func_id_map,
            body,
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            external_slot_types: script_ctx.slot_types,
            unit_end_drops: Vec::new(),
            // Script units have Result<()> return type for ! operator.
            return_type: Some(IrType::Result(Box::new(IrType::Unit))),
            is_script_unit: true,
            const_bindings: HashMap::new(),
        }
    }

    /// Swap function body state for a new function, returning the old state.
    pub fn swap_body_state(&mut self, new_state: FrameState) -> FrameState {
        std::mem::replace(&mut self.body, new_state)
    }

    /// Define a function in the current scope.
    pub fn define_func(&mut self, name: &str, param_count: usize) -> FuncId {
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), FuncRef::Local(func_id));
        func_id
    }

    /// Look up a function by name (for local/external functions only).
    pub fn lookup_func(&self, name: &str) -> Option<FuncRef> {
        self.func_scope.get(name).cloned()
    }

    /// Resolve a function call using typechecker's resolved call target.
    ///
    /// This is the preferred way to resolve function calls. It uses the
    /// `call_targets` from typechecking to get the exact function being called,
    /// then maps it to an IR function reference.
    ///
    /// For module functions (module_id = Some), looks up in `func_id_map`.
    /// For local functions (module_id = None), uses `func_scope` lookup.
    ///
    /// Returns `Err(LowerError::FunctionNotFound)` for local functions that aren't
    /// yet in scope (e.g., mutual recursion in script units where functions are
    /// lowered sequentially).
    pub fn resolve_call(&self, call: ExprFunctionCall<'db>) -> Result<FuncRef, super::LowerError> {
        let id = call.as_id().index() as usize;
        let func_name = call.name(self.db).text(self.db).to_string();

        // Get the resolved call target from typechecking.
        let target = self.call_targets.get(id)
            .and_then(|t| t.as_ref())
            .unwrap_or_else(|| panic!(
                "function call not resolved by typechecker: {} (id={})",
                func_name, id
            ));

        match target.module_id(self.db) {
            Some(module_id) => {
                // Module function - look up by (ModuleId, func_name).
                let resolved_func_name = target.func(self.db).name(self.db).text(self.db).to_string();
                let (ir_mod, func_id) = self.func_id_map
                    .get(&(module_id, resolved_func_name.clone()))
                    .unwrap_or_else(|| panic!(
                        "module function not in func_id_map: {}",
                        resolved_func_name
                    ));
                Ok(FuncRef::Module { module: *ir_mod, func: *func_id })
            }
            None => {
                // Local function resolved by typechecker - use func_scope lookup.
                // Can fail for mutual recursion in script units where the called
                // function hasn't been lowered yet.
                let resolved_func_name = target.func(self.db).name(self.db).text(self.db).to_string();
                self.lookup_func(&resolved_func_name)
                    .ok_or_else(|| super::LowerError::FunctionNotFound(resolved_func_name))
            }
        }
    }

    /// Allocate a fresh parameter with known type and mode.
    pub fn fresh_param(&mut self, ty: IrType, mode: ParamMode) -> ParamId {
        let id = ParamId(self.body.next_param);
        self.body.next_param += 1;
        self.body.param_types.push(ty);
        self.body.param_modes.push(mode);
        id
    }

    /// Get the mode of a parameter.
    pub fn param_mode(&self, id: ParamId) -> Option<ParamMode> {
        self.body.param_modes.get(id.0 as usize).copied()
    }

    /// Get the type of a parameter.
    pub fn param_type(&self, id: ParamId) -> Option<&IrType> {
        self.body.param_types.get(id.0 as usize)
    }

    /// Allocate a fresh SSA value with known type.
    pub fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.body.next_value);
        self.body.next_value += 1;
        self.body.value_types.push(ty);
        id
    }

    /// Allocate a fresh mutable slot with known type.
    pub fn fresh_slot(&mut self, ty: IrType) -> SlotId {
        let id = SlotId(self.body.next_slot);
        self.body.next_slot += 1;
        self.body.slot_types.push(ty);
        id
    }

    /// Allocate a fresh block.
    pub fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.body.next_block);
        self.body.next_block += 1;
        id
    }

    /// Emit an instruction to the current block.
    pub fn emit(&mut self, instr: Instruction) {
        self.body.current_instructions.push(instr);
    }

    /// Finish current block with a terminator, start a new block.
    pub fn finish_block(&mut self, terminator: Terminator) -> BlockId {
        let block = IrBlock {
            id: self.body.current_block,
            params: std::mem::take(&mut self.body.current_block_params),
            instructions: std::mem::take(&mut self.body.current_instructions),
            terminator,
        };
        self.body.blocks.push(block);
        self.body.current_block
    }

    /// Start building a new block.
    pub fn start_block(&mut self, id: BlockId) {
        self.body.current_block = id;
        self.body.current_instructions.clear();
        self.body.in_unreachable = false;
    }

    /// Start building an unreachable block (after return/break/continue).
    pub fn start_unreachable_block(&mut self, id: BlockId) {
        self.body.current_block = id;
        self.body.current_instructions.clear();
        self.body.in_unreachable = true;
    }

    /// Check if current block is unreachable.
    pub fn is_unreachable(&self) -> bool {
        self.body.in_unreachable
    }

    /// Bind a variable name to an operand.
    pub fn bind_var(&mut self, name: &str, operand: Operand) {
        self.body.variables.insert(name.to_string(), operand);
    }

    /// Look up a variable.
    pub fn lookup_var(&self, name: &str) -> Option<Operand> {
        self.body.variables.get(name).copied()
    }

    /// Look up a const binding by name.
    ///
    /// Returns the type and value if found.
    pub fn lookup_const(&self, name: &str) -> Option<&(IrType, ConstValue)> {
        self.const_bindings.get(name)
    }

    /// Add a const binding.
    ///
    /// Const bindings are evaluated at compile time and inlined at use sites.
    pub fn add_const(&mut self, name: String, ir_type: IrType, value: ConstValue) {
        self.const_bindings.insert(name, (ir_type, value));
    }

    /// Emit Drop instructions for the given operands.
    pub fn emit_drops(&mut self, operands: Vec<Operand>) {
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Record an expression temporary that needs dropping after the operation.
    pub fn record_expr_temp(&mut self, value: ValueId, ty: IrType) {
        if !ty.is_copy() {
            self.body.expr_temps.push((value, ty));
        }
    }

    /// Emit Drop instructions for all expression temporaries and clear the list.
    pub fn emit_expr_temp_drops(&mut self) {
        let temps = std::mem::take(&mut self.body.expr_temps);
        for (value, _ty) in temps {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Push a new pending intermediate scope for a compound expression.
    ///
    /// Call this at the start of compound expression lowering (struct, tuple, etc.).
    /// Each scope isolates intermediates created during that expression.
    /// On early return, all scopes are dropped from innermost to outermost.
    pub fn push_pending_scope(&mut self) {
        self.body.pending_intermediate_scopes.push(Vec::new());
    }

    /// Pop the current pending intermediate scope after compound expression completes.
    ///
    /// Call this at the end of compound expression lowering.
    /// The scope's intermediates should have been cleared (consumed by Pack, etc.).
    pub fn pop_pending_scope(&mut self) {
        let scope = self.body.pending_intermediate_scopes.pop();
        debug_assert!(scope.map_or(true, |s| s.is_empty()),
            "pending scope should be empty when popped");
    }

    /// Push a non-Copy intermediate value to the current scope.
    ///
    /// Call this after evaluating each sub-expression of a compound expression.
    /// Copy types are ignored since they don't need dropping.
    pub fn push_pending_intermediate(&mut self, value: ValueId, ty: &IrType) {
        if !ty.is_copy() {
            if let Some(scope) = self.body.pending_intermediate_scopes.last_mut() {
                scope.push(value);
            }
        }
    }

    /// Emit Drop instructions for all pending intermediates in all scopes.
    ///
    /// Call this on early return paths (try operators) before emitting binding drops.
    /// Drops innermost scope first, then outer scopes.
    pub fn emit_pending_intermediate_drops(&mut self) {
        // Collect all values to drop (innermost scope first, newest values first).
        let values: Vec<ValueId> = self.body.pending_intermediate_scopes.iter()
            .rev()
            .flat_map(|scope| scope.iter().rev().copied())
            .collect();
        for value in values {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Clear the current scope's pending intermediates without emitting drops.
    ///
    /// Call this after values are consumed (e.g., after Pack instruction).
    pub fn clear_pending_intermediates(&mut self) {
        if let Some(scope) = self.body.pending_intermediate_scopes.last_mut() {
            scope.clear();
        }
    }

    /// Get the type for a slot ID.
    pub fn slot_type(&self, id: SlotId) -> Option<&IrType> {
        self.body.slot_types.get(id.0 as usize)
    }

    /// Get the type for an external slot by variable name.
    pub fn external_slot_type(&self, name: &str) -> Option<&IrType> {
        self.external_slot_types.get(name)
    }

    /// Get the type for a slot by variable name (checks local and external slots).
    pub fn slot_type_by_name(&self, name: &str) -> Option<&IrType> {
        // First check if it's a local slot.
        if let Some(operand) = self.body.variables.get(name) {
            if let Operand::Slot(slot_id) = operand {
                return self.body.slot_types.get(slot_id.0 as usize);
            }
        }
        // Then check external slots.
        self.external_slot_types.get(name)
    }

    /// Get the type for any variable by name (checks slots, params, and external slots).
    pub fn var_type_by_name(&self, name: &str) -> Option<&IrType> {
        // Check local slots and params.
        if let Some(operand) = self.body.variables.get(name) {
            match operand {
                Operand::Slot(slot_id) => return self.body.slot_types.get(slot_id.0 as usize),
                Operand::Param(param_id) => return self.body.param_types.get(param_id.0 as usize),
                _ => {}
            }
        }
        // Then check external slots.
        self.external_slot_types.get(name)
    }

    /// Record a binding to operand mapping.
    ///
    /// Called when creating bindings during lowering.
    /// The binding ID must match the order from drop analysis.
    pub fn record_binding_operand(&mut self, operand: Operand) -> BindingId {
        let id = BindingId(self.body.next_binding_id);
        self.body.next_binding_id += 1;
        self.body.binding_to_operand.insert(id, operand);
        self.body.operand_to_binding.insert(operand, id);
        id
    }

    // ========================================================================
    // Tracking Helpers
    // ========================================================================
    //
    // IR instructions have precise and tracked variants. The lowering uses these
    // helpers to decide which variant to emit:
    //
    // ## Source Tracking (is_operand_tracked)
    //
    // Used for instructions that CONSUME a source operand and need to mark it
    // as MOVED. Examples:
    //
    // - `Move` vs `MoveTracked` - marks source as MOVED
    // - `SlotLoadMove` vs `SlotLoadMoveTracked` - marks slot as MOVED
    // - `Drop` vs `DropTracked` - checks if source is LIVE before dropping
    //
    // Question: "Does the source need its tracking byte updated to MOVED?"
    //
    // ## Destination Tracking (is_dest_tracked)
    //
    // Used for instructions that PRODUCE a value and need to mark the destination
    // as LIVE. Examples:
    //
    // - `Const` vs `ConstTracked` - marks dest as LIVE
    // - `Call` vs `CallTracked` - marks dest as LIVE
    // - `Pack` vs `PackTracked` - marks dest as LIVE
    // - `WrapSome` vs `WrapSomeTracked` - marks dest as LIVE
    // - etc.
    //
    // Question: "Does the dest need its tracking byte set to LIVE?"
    //
    // ## Slot Tracking (is_slot_tracked)
    //
    // Used for instructions that WRITE to a slot and need to mark it as LIVE:
    //
    // - `SlotStoreCopy` vs `SlotStoreCopyTracked` - marks slot as LIVE
    // - `SlotStoreMove` vs `SlotStoreMoveTracked` - marks slot as LIVE
    // - `SetField` vs `SetFieldTracked` - marks slot as LIVE
    //
    // Question: "Does the slot need its tracking byte set to LIVE?"
    // ========================================================================

    /// Check if a binding is tracked (needs runtime tracking state).
    ///
    /// Bindings are tracked when ownership analysis cannot statically prove their
    /// state at all use points (e.g., exports, values in conditional branches).
    pub fn is_binding_tracked(&self, id: BindingId) -> bool {
        self.body.tracking.get(id.0 as usize)
            .map(|cat| *cat == TrackingCategory::Tracked)
            .unwrap_or(true) // Default to tracked if not found (safe fallback).
    }

    /// Check if a source operand needs tracking when consumed.
    ///
    /// Use this to decide between precise vs tracked variants for instructions
    /// that consume their source:
    /// - `Move` vs `MoveTracked`
    /// - `SlotLoadMove` vs `SlotLoadMoveTracked`
    /// - `Drop` vs `DropTracked`
    ///
    /// Returns true if the source's tracking byte should be written to MOVED.
    pub fn is_operand_tracked(&self, operand: Operand) -> bool {
        self.body.operand_to_binding.get(&operand)
            .map(|id| self.is_binding_tracked(*id))
            .unwrap_or(true) // Default to tracked if not found (safe fallback).
    }

    /// Check if a destination value needs tracking when produced.
    ///
    /// Use this to decide between precise vs tracked variants for instructions
    /// that produce values:
    /// - `Const` vs `ConstTracked`
    /// - `Call` vs `CallTracked`
    /// - `Pack` vs `PackTracked`
    /// - `WrapSome` vs `WrapSomeTracked`
    /// - etc.
    ///
    /// Returns true if the dest's tracking byte should be written to LIVE.
    ///
    /// A destination needs tracking if:
    /// - It corresponds to a tracked binding (exports, conditional paths)
    /// - It's a non-Copy intermediate that will be in tracked_values
    pub fn is_dest_tracked(&self, dest: ValueId) -> bool {
        // Check if this value corresponds to a tracked binding.
        let operand = Operand::Value(dest);
        if let Some(&id) = self.body.operand_to_binding.get(&operand) {
            return self.is_binding_tracked(id);
        }

        // For non-binding values (intermediates), check if the type is non-Copy.
        // Non-Copy intermediates end up in tracked_values and need tracking.
        if let Some(ty) = self.body.value_types.get(dest.0 as usize) {
            return !ty.is_copy();
        }

        // Default to tracked for safety.
        true
    }

    /// Check if a slot needs tracking when written.
    ///
    /// Use this to decide between precise vs tracked variants for instructions
    /// that write to slots:
    /// - `SlotStoreCopy` vs `SlotStoreCopyTracked`
    /// - `SlotStoreMove` vs `SlotStoreMoveTracked`
    /// - `SetField` vs `SetFieldTracked`
    ///
    /// Returns true if the slot's tracking byte should be written to LIVE.
    pub fn is_slot_tracked(&self, slot: SlotId) -> bool {
        let operand = Operand::Slot(slot);
        if let Some(&id) = self.body.operand_to_binding.get(&operand) {
            return self.is_binding_tracked(id);
        }
        // Default to tracked for safety.
        true
    }

    /// Emit a drop for a binding, using DropTracked if tracked, Drop if precise.
    fn emit_binding_drop(&mut self, id: BindingId) {
        if let Some(&operand) = self.body.binding_to_operand.get(&id) {
            if self.is_binding_tracked(id) {
                self.emit(Instruction::DropTracked { operand });
            } else {
                self.emit(Instruction::Drop { operand });
            }
        }
    }

    // ========================================================================
    // Instruction Emission Helpers
    // ========================================================================
    //
    // These helpers emit the correct instruction variant (precise or tracked)
    // based on whether the destination needs tracking.

    /// Emit Const or ConstTracked based on destination tracking.
    pub fn emit_const(&mut self, dest: ValueId, value: ConstValue) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::ConstTracked { dest, value });
        } else {
            self.emit(Instruction::Const { dest, value });
        }
    }

    /// Emit Widen or WidenTracked based on destination tracking.
    pub fn emit_widen(&mut self, dest: ValueId, src: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::WidenTracked { dest, src });
        } else {
            self.emit(Instruction::Widen { dest, src });
        }
    }

    /// Emit Call or CallTracked based on destination tracking.
    pub fn emit_call(&mut self, dest: ValueId, func: FuncRef, args: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::CallTracked { dest, func, args });
        } else {
            self.emit(Instruction::Call { dest, func, args });
        }
    }

    /// Emit Pack or PackTracked based on destination tracking.
    pub fn emit_pack(&mut self, dest: ValueId, ty: TypeRef, fields: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::PackTracked { dest, ty, fields });
        } else {
            self.emit(Instruction::Pack { dest, ty, fields });
        }
    }

    /// Emit Unpack or UnpackTracked based on destination tracking.
    ///
    /// Uses the first dest to determine tracking (all dests should have same tracking).
    pub fn emit_unpack(&mut self, dests: Vec<ValueId>, src: Operand) {
        let tracked = dests.first().map(|d| self.is_dest_tracked(*d)).unwrap_or(false);
        if tracked {
            self.emit(Instruction::UnpackTracked { dests, src });
        } else {
            self.emit(Instruction::Unpack { dests, src });
        }
    }

    /// Emit GetField or GetFieldTracked based on destination tracking.
    pub fn emit_get_field(&mut self, dest: ValueId, src: Operand, field_index: u32) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::GetFieldTracked { dest, src, field_index });
        } else {
            self.emit(Instruction::GetField { dest, src, field_index });
        }
    }

    /// Emit WrapSome or WrapSomeTracked based on destination tracking.
    pub fn emit_wrap_some(&mut self, dest: ValueId, inner: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::WrapSomeTracked { dest, inner });
        } else {
            self.emit(Instruction::WrapSome { dest, inner });
        }
    }

    /// Emit WrapNone or WrapNoneTracked based on destination tracking.
    pub fn emit_wrap_none(&mut self, dest: ValueId) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::WrapNoneTracked { dest });
        } else {
            self.emit(Instruction::WrapNone { dest });
        }
    }

    /// Emit WrapOk or WrapOkTracked based on destination tracking.
    pub fn emit_wrap_ok(&mut self, dest: ValueId, inner: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::WrapOkTracked { dest, inner });
        } else {
            self.emit(Instruction::WrapOk { dest, inner });
        }
    }

    /// Emit WrapErr or WrapErrTracked based on destination tracking.
    pub fn emit_wrap_err(&mut self, dest: ValueId, inner: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::WrapErrTracked { dest, inner });
        } else {
            self.emit(Instruction::WrapErr { dest, inner });
        }
    }

    /// Emit EnumVariant or EnumVariantTracked based on destination tracking.
    pub fn emit_enum_variant(&mut self, dest: ValueId, variant_index: u32, payload: Option<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::EnumVariantTracked { dest, variant_index, payload });
        } else {
            self.emit(Instruction::EnumVariant { dest, variant_index, payload });
        }
    }

    /// Emit ErrorFrom or ErrorFromTracked based on destination tracking.
    pub fn emit_error_from(&mut self, dest: ValueId, inner: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::ErrorFromTracked { dest, inner });
        } else {
            self.emit(Instruction::ErrorFrom { dest, inner });
        }
    }

    /// Emit DataFrom or DataFromTracked based on destination tracking.
    pub fn emit_data_from(&mut self, dest: ValueId, inner: Operand) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::DataFromTracked { dest, inner });
        } else {
            self.emit(Instruction::DataFrom { dest, inner });
        }
    }

    /// Emit ListNew or ListNewTracked based on destination tracking.
    pub fn emit_list_new(&mut self, dest: ValueId, elements: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::ListNewTracked { dest, elements });
        } else {
            self.emit(Instruction::ListNew { dest, elements });
        }
    }

    /// Emit SetNew or SetNewTracked based on destination tracking.
    pub fn emit_set_new(&mut self, dest: ValueId, elements: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::SetNewTracked { dest, elements });
        } else {
            self.emit(Instruction::SetNew { dest, elements });
        }
    }

    /// Emit MapNew or MapNewTracked based on destination tracking.
    pub fn emit_map_new(&mut self, dest: ValueId, entries: Vec<(Operand, Operand)>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::MapNewTracked { dest, entries });
        } else {
            self.emit(Instruction::MapNew { dest, entries });
        }
    }

    /// Emit TensorNew or TensorNewTracked based on destination tracking.
    pub fn emit_tensor_new(&mut self, dest: ValueId, shape: Vec<u32>, elements: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::TensorNewTracked { dest, shape, elements });
        } else {
            self.emit(Instruction::TensorNew { dest, shape, elements });
        }
    }

    /// Emit TableNew or TableNewTracked based on destination tracking.
    pub fn emit_table_new(&mut self, dest: ValueId, rows: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::TableNewTracked { dest, rows });
        } else {
            self.emit(Instruction::TableNew { dest, rows });
        }
    }

    /// Emit Intrinsic or IntrinsicTracked based on destination tracking.
    pub fn emit_intrinsic(&mut self, dest: ValueId, intrinsic: datalove_datafun_intrinsics::IntrinsicId, args: Vec<Operand>) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::IntrinsicTracked { dest, intrinsic, args });
        } else {
            self.emit(Instruction::Intrinsic { dest, intrinsic, args });
        }
    }

    /// Emit SlotStoreCopy or SlotStoreCopyTracked based on slot tracking.
    pub fn emit_slot_store_copy(&mut self, dest: SlotDest, value: Operand) {
        let tracked = match &dest {
            SlotDest::Local(sid) => self.is_slot_tracked(*sid),
            SlotDest::External { .. } => true, // External slots always tracked.
        };
        if tracked {
            self.emit(Instruction::SlotStoreCopyTracked { dest, value });
        } else {
            self.emit(Instruction::SlotStoreCopy { dest, value });
        }
    }

    /// Emit SlotStoreMove or SlotStoreMoveTracked based on slot tracking.
    pub fn emit_slot_store_move(&mut self, dest: SlotDest, value: Operand) {
        let tracked = match &dest {
            SlotDest::Local(sid) => self.is_slot_tracked(*sid),
            SlotDest::External { .. } => true, // External slots always tracked.
        };
        if tracked {
            self.emit(Instruction::SlotStoreMoveTracked { dest, value });
        } else {
            self.emit(Instruction::SlotStoreMove { dest, value });
        }
    }

    /// Emit SetField or SetFieldTracked based on slot tracking.
    pub fn emit_set_field(&mut self, slot: SlotDest, field_path: Vec<u32>, value: Operand) {
        let tracked = match &slot {
            SlotDest::Local(sid) => self.is_slot_tracked(*sid),
            SlotDest::External { .. } => true, // External slots always tracked.
        };
        if tracked {
            self.emit(Instruction::SetFieldTracked { slot, field_path, value });
        } else {
            self.emit(Instruction::SetField { slot, field_path, value });
        }
    }

    /// Emit SlotLoadCopy or SlotLoadCopyTracked based on destination tracking.
    pub fn emit_slot_load_copy(&mut self, dest: ValueId, slot: SlotId) {
        if self.is_dest_tracked(dest) {
            self.emit(Instruction::SlotLoadCopyTracked { dest, slot });
        } else {
            self.emit(Instruction::SlotLoadCopy { dest, slot });
        }
    }

    /// Compute tracked_values for the current unit.
    ///
    /// Returns all non-Copy ValueIds that need runtime tracking for destroy_all.
    /// This includes:
    /// - Tracked bindings stored as values
    /// - All non-binding intermediate values (non-Copy)
    ///
    /// Note: Unit-end bindings are NOT included here - they're handled separately
    /// via unit_end_values/unit_end_slots fields.
    pub fn compute_tracked_values(&self) -> Vec<ValueId> {
        let mut result = Vec::new();
        let mut added: std::collections::HashSet<ValueId> = std::collections::HashSet::new();

        // Collect tracked bindings.
        let binding_values: std::collections::HashSet<ValueId> = self.body.binding_to_operand
            .values()
            .filter_map(|op| match op {
                Operand::Value(v) => Some(*v),
                _ => None,
            })
            .collect();

        for (id, &operand) in &self.body.binding_to_operand {
            if self.is_binding_tracked(*id) {
                if let Operand::Value(value_id) = operand {
                    if added.insert(value_id) {
                        result.push(value_id);
                    }
                }
            }
        }

        // Add all non-Copy non-binding values (intermediates).
        // These don't have explicit Drop instructions, so destroy_all must handle them.
        for (idx, ty) in self.body.value_types.iter().enumerate() {
            let value_id = ValueId(idx as u32);
            if !ty.is_copy() && !binding_values.contains(&value_id) {
                if added.insert(value_id) {
                    result.push(value_id);
                }
            }
        }

        result
    }

    /// Compute tracked_slots for the current unit.
    ///
    /// Returns SlotIds for bindings that are tracked and stored in slots.
    ///
    /// Note: Unit-end bindings are NOT included here - they're handled separately
    /// via unit_end_values/unit_end_slots fields.
    pub fn compute_tracked_slots(&self) -> Vec<SlotId> {
        let mut result = Vec::new();
        let mut added: std::collections::HashSet<SlotId> = std::collections::HashSet::new();

        for (id, &operand) in &self.body.binding_to_operand {
            if self.is_binding_tracked(*id) {
                if let Operand::Slot(slot_id) = operand {
                    if added.insert(slot_id) {
                        result.push(slot_id);
                    }
                }
            }
        }

        result
    }

    /// Compute unit_end_values for the current unit.
    ///
    /// Returns ValueIds for script-level bindings that need cleanup by destroy_all.
    /// These have UnitEndDrop which is a no-op in the interpreter.
    pub fn compute_unit_end_values(&self) -> Vec<ValueId> {
        self.unit_end_drops.iter()
            .filter_map(|id| self.body.binding_to_operand.get(id))
            .filter_map(|op| match op {
                Operand::Value(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    /// Compute unit_end_slots for the current unit.
    ///
    /// Returns SlotIds for script-level bindings that need cleanup by destroy_all.
    /// These have UnitEndDrop which is a no-op in the interpreter.
    pub fn compute_unit_end_slots(&self) -> Vec<SlotId> {
        self.unit_end_drops.iter()
            .filter_map(|id| self.body.binding_to_operand.get(id))
            .filter_map(|op| match op {
                Operand::Slot(s) => Some(*s),
                _ => None,
            })
            .collect()
    }

    /// Allocate and return the next global statement ID.
    ///
    /// Must be called in the same order as during ownership analysis.
    pub fn alloc_stmt_id(&mut self) -> usize {
        let id = self.body.next_stmt_id;
        self.body.next_stmt_id += 1;
        id
    }

    /// Emit drops scheduled for a then-branch exit.
    pub fn emit_then_branch_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_then(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled for an else-branch exit.
    pub fn emit_else_branch_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_else(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a return statement.
    pub fn emit_before_return_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_return(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a TryReturn (checked/optional operators).
    ///
    /// Uses current_stmt_idx since TryReturn happens within expression lowering.
    pub fn emit_before_try_return_drops(&mut self) {
        if let Some(stmt_idx) = self.body.current_stmt_idx {
            let binding_ids = self.get_scheduled_binding_ids_try(stmt_idx);
            for id in binding_ids {
                self.emit_binding_drop(id);
            }
        }
    }

    /// Get binding IDs to drop for then-branch exit.
    fn get_scheduled_binding_ids_then(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.then_branch_exit.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Get binding IDs to drop for else-branch exit.
    fn get_scheduled_binding_ids_else(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.else_branch_exit.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Get binding IDs to drop before return.
    fn get_scheduled_binding_ids_return(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.before_return.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Get binding IDs to drop before TryReturn.
    fn get_scheduled_binding_ids_try(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.before_try_return.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Emit drops scheduled for loop body end.
    pub fn emit_loop_body_end_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_loop(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a break statement.
    pub fn emit_before_break_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_break(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a continue statement.
    pub fn emit_before_continue_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.get_scheduled_binding_ids_continue(stmt_idx);
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Get binding IDs to drop at loop body end.
    fn get_scheduled_binding_ids_loop(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.loop_body_end.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Get binding IDs to drop before break.
    fn get_scheduled_binding_ids_break(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.before_break.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Get binding IDs to drop before continue.
    fn get_scheduled_binding_ids_continue(&self, stmt_idx: usize) -> Vec<BindingId> {
        self.body.drop_schedule.before_continue.get(&stmt_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Emit UnitEndDrop/UnitEndDropTracked instructions for script-level bindings.
    ///
    /// Uses UnitEndDrop for Precise bindings, UnitEndDropTracked for Tracked.
    /// Backend semantics:
    /// - Interpreter: both are no-op (bindings persist for REPL)
    /// - AOT: UnitEndDrop is unconditional, UnitEndDropTracked checks tracking byte
    pub fn emit_unit_end_drops(&mut self) {
        for id in std::mem::take(&mut self.unit_end_drops) {
            if let Some(&operand) = self.body.binding_to_operand.get(&id) {
                if self.is_binding_tracked(id) {
                    self.emit(Instruction::UnitEndDropTracked { operand });
                } else {
                    self.emit(Instruction::UnitEndDrop { operand });
                }
            }
        }
    }

    /// Renumber blocks to be sequential starting from 0.
    ///
    /// After lowering, blocks may have gaps in their IDs due to control flow
    /// structure (blocks are allocated breadth-first but finished depth-first).
    /// This renumbers them so `blocks[i].id.0 == i`, enabling O(1) block lookup
    /// in the interpreter.
    pub fn renumber_blocks(&mut self) {
        if self.body.blocks.is_empty() {
            return;
        }

        // Build mapping from old ID to new ID using Vec for O(1) lookup.
        // Old IDs are sparse but bounded by next_block.
        let mut id_map = vec![0u32; self.body.next_block as usize];
        for (new_id, block) in self.body.blocks.iter().enumerate() {
            id_map[block.id.0 as usize] = new_id as u32;
        }

        // Update block IDs and terminator references.
        for (new_id, block) in self.body.blocks.iter_mut().enumerate() {
            block.id = BlockId(new_id as u32);

            // Update terminator targets.
            match &mut block.terminator {
                Terminator::Goto { target, .. } => {
                    *target = BlockId(id_map[target.0 as usize]);
                }
                Terminator::Branch { then_block, else_block, .. } => {
                    *then_block = BlockId(id_map[then_block.0 as usize]);
                    *else_block = BlockId(id_map[else_block.0 as usize]);
                }
                Terminator::Return { .. }
                | Terminator::UnitEnd { .. }
                | Terminator::UnitEarlyReturn { .. } => {}
            }
        }
    }
}
