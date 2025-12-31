//! Lowering context types.
//!
//! The main `LowerCtx` struct provides the state for lowering AST to IR,
//! and `ScriptLowerContext` tracks bindings across script units.

use std::collections::{HashMap, HashSet};
use salsa::plumbing::AsId;
use crate::ast::{Statement, ExprFun};
use crate::Db;
use super::super::{
    IrType, IrBlock, IrFunction, Operand, ValueId, SlotId, BlockId, FuncId,
    FuncRef, Terminator, Instruction, SymbolTable, ExportBinding,
};
use super::super::drop_analysis::{BindingId, DropSchedule, BindingInfo};

/// Context for lowering script units.
///
/// Tracks bindings available from previous units.
#[derive(Clone, Debug, Default)]
pub struct ScriptLowerContext {
    /// Available let bindings: name -> (unit_index, value_id).
    pub values: HashMap<String, (u32, ValueId)>,
    /// Available var bindings: name -> (unit_index, slot_id).
    pub slots: HashMap<String, (u32, SlotId)>,
    /// Available functions: name -> (unit_index, func_id).
    pub functions: HashMap<String, (u32, FuncId)>,
    /// Module functions: name -> lowered IR function.
    /// These are all functions available from modules.
    pub module_functions: HashMap<String, IrFunction>,
    /// Imported module function names.
    /// Only functions in this set are accessible to the current unit.
    pub imported_module_functions: HashSet<String>,
    /// Module aliases: alias -> full path.
    /// Built from require statements.
    pub module_aliases: HashMap<String, String>,
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
    pub fn add_exports(&mut self, unit_index: u32, exports: &[(String, ExportBinding)]) {
        for (name, binding) in exports {
            match binding {
                ExportBinding::Value(v) => {
                    // Remove any slot with the same name to properly shadow.
                    self.slots.remove(name);
                    self.values.insert(name.clone(), (unit_index, *v));
                }
                ExportBinding::Slot(s) => {
                    // Remove any value with the same name to properly shadow.
                    self.values.remove(name);
                    self.slots.insert(name.clone(), (unit_index, *s));
                }
                ExportBinding::Function(func_id) => {
                    self.functions.insert(name.clone(), (unit_index, *func_id));
                }
            }
        }
    }

    /// Add a lowered module function to the context.
    pub fn add_module_function(&mut self, name: String, func: IrFunction) {
        self.module_functions.insert(name, func);
    }

    /// Add a module alias from a require statement.
    pub fn add_module_alias(&mut self, alias: String, full_path: String) {
        self.module_aliases.insert(alias, full_path);
    }

    /// Mark a module function as imported.
    pub fn import_module_function(&mut self, name: String) {
        self.imported_module_functions.insert(name);
    }
}

/// What kind of script unit we're lowering.
pub enum ScriptUnitKind<'db> {
    /// A sequence of statements.
    Fragment(Vec<Statement<'db>>),
    /// A single expression.
    Expr(ExprFun<'db>),
}

/// Context for lowering a single function or script unit.
pub struct LowerCtx<'db> {
    pub(super) db: &'db dyn Db,
    /// Expression types from typechecker.
    pub(super) expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    /// Next ValueId to allocate.
    pub(super) next_value: u32,
    /// Next SlotId to allocate.
    pub(super) next_slot: u32,
    /// Next BlockId to allocate.
    pub(super) next_block: u32,
    /// Blocks being built.
    pub(super) blocks: Vec<IrBlock>,
    /// Current block being built.
    pub(super) current_block: BlockId,
    /// Instructions for current block.
    pub(super) current_instructions: Vec<Instruction>,
    /// Mapping from variable names to their operands.
    pub(super) variables: HashMap<String, Operand>,
    /// Exports from this unit (only used for script units).
    pub(super) exports: Vec<(String, ExportBinding)>,
    /// Functions defined in this script unit.
    pub(super) functions: Vec<IrFunction>,
    /// Symbol table for function resolution.
    pub(super) symbols: SymbolTable,
    /// Available functions: name -> FuncRef (for resolving calls).
    pub(super) func_scope: HashMap<String, FuncRef>,
    /// Type for each ValueId.
    pub(super) value_types: Vec<IrType>,
    /// Type for each SlotId.
    pub(super) slot_types: Vec<IrType>,
    /// Loop context stack: (continue_target, break_target) for each nested loop.
    pub(super) loop_stack: Vec<(BlockId, BlockId)>,
    /// Return type for current function/script (for try operators).
    pub(super) return_type: Option<IrType>,
    /// Whether we're in a script unit (vs function).
    pub(super) is_script_unit: bool,
    /// Temporary values to drop after the current expression is evaluated.
    /// These are created during operand lowering for compound expressions.
    pub(super) expr_temps: Vec<(ValueId, IrType)>,
    /// Drop schedule from analysis.
    pub(super) drop_schedule: DropSchedule,
    /// Binding info from analysis (for looking up names).
    pub(super) binding_info: Vec<BindingInfo>,
    /// Mapping from BindingId to Operand (built during lowering).
    pub(super) binding_to_operand: HashMap<BindingId, Operand>,
    /// Next BindingId to allocate (must match analysis traversal order).
    pub(super) next_binding_id: u32,
    /// Current statement index in the parent body (for drop schedule lookup).
    pub(super) current_stmt_idx: Option<usize>,
}

impl<'db> LowerCtx<'db> {
    pub fn new(db: &'db dyn Db, expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>]) -> Self {
        Self {
            db,
            expr_types,
            next_value: 0,
            next_slot: 0,
            next_block: 1, // Block 0 is entry
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables: HashMap::new(),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope: HashMap::new(),
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            return_type: None,
            is_script_unit: false,
            expr_temps: Vec::new(),
            drop_schedule: DropSchedule::default(),
            binding_info: Vec::new(),
            binding_to_operand: HashMap::new(),
            next_binding_id: 0,
            current_stmt_idx: None,
        }
    }

    /// Create a context for lowering module functions with imported functions available.
    pub fn new_for_module(
        db: &'db dyn Db,
        expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
        available_functions: &[String],
    ) -> Self {
        // Seed func_scope with available module functions.
        let mut func_scope = HashMap::new();
        for name in available_functions {
            func_scope.insert(name.clone(), FuncRef::Module { name: name.clone() });
        }

        Self {
            db,
            expr_types,
            next_value: 0,
            next_slot: 0,
            next_block: 1,
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables: HashMap::new(),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            return_type: None,
            is_script_unit: false,
            expr_temps: Vec::new(),
            drop_schedule: DropSchedule::default(),
            binding_info: Vec::new(),
            binding_to_operand: HashMap::new(),
            next_binding_id: 0,
            current_stmt_idx: None,
        }
    }

    /// Get the IrType for an expression from the typechecker.
    pub fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
        let expr_id = expr.as_id();
        let index = expr_id.index() as usize;
        match self.expr_types.get(index).copied().flatten() {
            Some(ty) => IrType::from_tycheck(self.db, &ty),
            None => panic!(
                "Expression must have type from typechecker. Expression ID {} but expr_types.len() = {}",
                index, self.expr_types.len()
            ),
        }
    }

    /// Create a context for lowering a script unit.
    pub fn new_for_script(
        db: &'db dyn Db,
        expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
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

        // Add module functions - only those that have been imported.
        for (name, _func) in &script_ctx.module_functions {
            if script_ctx.imported_module_functions.contains(name) {
                func_scope.insert(name.clone(), FuncRef::Module { name: name.clone() });
            }
        }

        Self {
            db,
            expr_types,
            next_value: 0,
            next_slot: 0,
            next_block: 1,
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables,
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            // Script units have Result<()> return type for ! operator.
            return_type: Some(IrType::Result(Box::new(IrType::Unit))),
            is_script_unit: true,
            expr_temps: Vec::new(),
            drop_schedule: DropSchedule::default(),
            binding_info: Vec::new(),
            binding_to_operand: HashMap::new(),
            next_binding_id: 0,
            current_stmt_idx: None,
        }
    }

    /// Define a function in the current scope.
    pub fn define_func(&mut self, name: &str, param_count: usize) -> FuncId {
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), FuncRef::Local(func_id));
        func_id
    }

    /// Look up a function by name.
    pub fn lookup_func(&self, name: &str) -> Option<FuncRef> {
        self.func_scope.get(name).cloned()
    }

    /// Allocate a fresh SSA value with known type.
    pub fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ty);
        id
    }

    /// Allocate a fresh mutable slot with known type.
    pub fn fresh_slot(&mut self, ty: IrType) -> SlotId {
        let id = SlotId(self.next_slot);
        self.next_slot += 1;
        self.slot_types.push(ty);
        id
    }

    /// Allocate a fresh block.
    pub fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    /// Emit an instruction to the current block.
    pub fn emit(&mut self, instr: Instruction) {
        self.current_instructions.push(instr);
    }

    /// Finish current block with a terminator, start a new block.
    pub fn finish_block(&mut self, terminator: Terminator) -> BlockId {
        let block = IrBlock {
            id: self.current_block,
            instructions: std::mem::take(&mut self.current_instructions),
            terminator,
        };
        self.blocks.push(block);
        self.current_block
    }

    /// Start building a new block.
    pub fn start_block(&mut self, id: BlockId) {
        self.current_block = id;
        self.current_instructions.clear();
    }

    /// Bind a variable name to an operand.
    pub fn bind_var(&mut self, name: &str, operand: Operand) {
        self.variables.insert(name.to_string(), operand);
    }

    /// Look up a variable.
    pub fn lookup_var(&self, name: &str) -> Option<Operand> {
        self.variables.get(name).copied()
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
            self.expr_temps.push((value, ty));
        }
    }

    /// Emit Drop instructions for all expression temporaries and clear the list.
    pub fn emit_expr_temp_drops(&mut self) {
        let temps = std::mem::take(&mut self.expr_temps);
        for (value, _ty) in temps {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Get the type for a slot ID.
    pub fn slot_type(&self, id: SlotId) -> Option<&IrType> {
        self.slot_types.get(id.0 as usize)
    }

    /// Set the drop schedule for this context.
    pub fn set_drop_schedule(&mut self, schedule: DropSchedule, bindings: Vec<BindingInfo>) {
        self.drop_schedule = schedule;
        self.binding_info = bindings;
    }

    /// Record a binding to operand mapping.
    ///
    /// Called when creating bindings during lowering.
    /// The binding ID must match the order from drop analysis.
    pub fn record_binding_operand(&mut self, operand: Operand) -> BindingId {
        let id = BindingId(self.next_binding_id);
        self.next_binding_id += 1;
        self.binding_to_operand.insert(id, operand);
        id
    }

    /// Emit drops scheduled for a then-branch exit.
    pub fn emit_then_branch_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_then(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit drops scheduled for an else-branch exit.
    pub fn emit_else_branch_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_else(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit drops scheduled before a return statement.
    pub fn emit_before_return_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_return(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit drops scheduled before a TryReturn (checked/optional operators).
    ///
    /// Uses current_stmt_idx since TryReturn happens within expression lowering.
    pub fn emit_before_try_return_drops(&mut self) {
        if let Some(stmt_idx) = self.current_stmt_idx {
            let operands = self.get_scheduled_drops_try(stmt_idx);
            for operand in operands {
                self.emit(Instruction::Drop { operand });
            }
        }
    }

    /// Get operands to drop for then-branch exit.
    fn get_scheduled_drops_then(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.then_branch_exit.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Get operands to drop for else-branch exit.
    fn get_scheduled_drops_else(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.else_branch_exit.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Get operands to drop before return.
    fn get_scheduled_drops_return(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.before_return.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Get operands to drop before TryReturn.
    fn get_scheduled_drops_try(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.before_try_return.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Emit drops scheduled for loop body end.
    pub fn emit_loop_body_end_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_loop(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit drops scheduled before a break statement.
    pub fn emit_before_break_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_break(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit drops scheduled before a continue statement.
    pub fn emit_before_continue_drops(&mut self, stmt_idx: usize) {
        let operands = self.get_scheduled_drops_continue(stmt_idx);
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Get operands to drop at loop body end.
    fn get_scheduled_drops_loop(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.loop_body_end.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Get operands to drop before break.
    fn get_scheduled_drops_break(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.before_break.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }

    /// Get operands to drop before continue.
    fn get_scheduled_drops_continue(&self, stmt_idx: usize) -> Vec<Operand> {
        let mut result = Vec::new();
        if let Some(binding_ids) = self.drop_schedule.before_continue.get(&stmt_idx) {
            for &id in binding_ids {
                if let Some(&operand) = self.binding_to_operand.get(&id) {
                    result.push(operand);
                }
            }
        }
        result
    }
}
