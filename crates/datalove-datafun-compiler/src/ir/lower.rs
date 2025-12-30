//! Lower AST to SSA IR.
//!
//! This module transforms typechecked AST into flat SSA IR suitable for
//! interpretation and codegen.

use rmx::prelude::*;
use std::collections::{HashMap, HashSet};
use salsa::plumbing::AsId;
use bct::text::InternedText;
use crate::ast::{self, Statement, ExprFun, ExprFunKind};
use crate::tycheck::TypecheckResult;
use crate::Db;
use super::*;

/// Check if an IR type has copy semantics.
///
/// Copy types can be bitwise copied without ownership tracking.
/// Non-copy types require explicit drops.
fn is_copy_type(ty: &IrType) -> bool {
    match ty {
        // Scalar primitives are always copy.
        IrType::Unit | IrType::Bool => true,
        IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 => true,
        IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 => true,
        IrType::F32 => true,
        // Heap-allocated types are never copy.
        IrType::Int | IrType::String | IrType::Data | IrType::Error => false,
        IrType::List(_) | IrType::Set(_) | IrType::Map(_, _) => false,
        // Option is copy only if inner is copy.
        IrType::Option(inner) => is_copy_type(inner),
        // Result is never copy (conservative).
        IrType::Result(_) => false,
        // Tuple is copy only if all fields are copy.
        IrType::Tuple(fields) => fields.iter().all(is_copy_type),
        // Struct is copy only if all fields are copy.
        IrType::Struct(fields) => fields.iter().all(|(_, ty)| is_copy_type(ty)),
    }
}

/// What kind of scope we're tracking.
#[derive(Clone, Debug)]
enum ScopeKind {
    /// Function body scope.
    Function,
    /// Script unit top-level scope (values are exported, not dropped at unit end).
    ScriptUnit,
    /// Loop body scope.
    Loop { header: BlockId, exit: BlockId },
    /// If-then branch scope.
    IfThen { merge: BlockId },
    /// If-else branch scope.
    IfElse { merge: BlockId },
}

/// A binding tracked for drop purposes.
#[derive(Clone, Debug)]
struct TrackedBinding {
    /// The operand (Value or Slot).
    operand: Operand,
    /// The type (for determining if drop is needed).
    ty: IrType,
    /// Whether this binding has been moved/consumed.
    moved: bool,
}

/// A scope for tracking drops.
#[derive(Clone, Debug)]
struct Scope {
    kind: ScopeKind,
    /// Bindings created in this scope that may need dropping.
    bindings: Vec<TrackedBinding>,
}

impl Scope {
    fn new(kind: ScopeKind) -> Self {
        Self {
            kind,
            bindings: Vec::new(),
        }
    }
}

/// Scope tracker for emitting drops at scope exits.
#[derive(Clone, Debug, Default)]
struct ScopeTracker {
    scopes: Vec<Scope>,
}

impl ScopeTracker {
    fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// Enter a new scope.
    fn enter_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope::new(kind));
    }

    /// Record a binding in the current scope.
    fn record_binding(&mut self, operand: Operand, ty: IrType) {
        if let Some(scope) = self.scopes.last_mut() {
            // Only track non-copy types.
            if !is_copy_type(&ty) {
                scope.bindings.push(TrackedBinding {
                    operand,
                    ty,
                    moved: false,
                });
            }
        }
    }

    /// Mark an operand as moved (won't be dropped).
    fn mark_moved(&mut self, operand: &Operand) {
        // Search all scopes from innermost to outermost.
        for scope in self.scopes.iter_mut().rev() {
            for binding in &mut scope.bindings {
                if &binding.operand == operand {
                    binding.moved = true;
                    return;
                }
            }
        }
    }

    /// Get the bindings that need dropping when exiting the current scope.
    fn bindings_to_drop(&self) -> Vec<Operand> {
        if let Some(scope) = self.scopes.last() {
            // Don't drop script unit top-level bindings (they're exported).
            if matches!(scope.kind, ScopeKind::ScriptUnit) {
                return Vec::new();
            }
            scope.bindings.iter()
                .filter(|b| !b.moved)
                .map(|b| b.operand)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Exit the current scope, returning bindings that need dropping.
    fn exit_scope(&mut self) -> Vec<Operand> {
        let drops = self.bindings_to_drop();
        self.scopes.pop();
        drops
    }

    /// Get bindings to drop for break (all scopes up to and including the loop).
    fn bindings_to_drop_for_break(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a loop scope.
            if matches!(scope.kind, ScopeKind::Loop { .. }) {
                break;
            }
        }
        drops
    }

    /// Get bindings to drop for continue (only the current loop iteration).
    fn bindings_to_drop_for_continue(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a loop scope (include it, then stop).
            if matches!(scope.kind, ScopeKind::Loop { .. }) {
                break;
            }
        }
        drops
    }

    /// Check if we're inside a function scope.
    fn in_function(&self) -> bool {
        self.scopes.iter().any(|s| matches!(s.kind, ScopeKind::Function))
    }

    /// Get all bindings to drop for a function return.
    ///
    /// Returns bindings from all scopes up to and including the function scope.
    fn bindings_to_drop_for_return(&self) -> Vec<Operand> {
        let mut drops = Vec::new();
        for scope in self.scopes.iter().rev() {
            // Collect bindings from this scope.
            for binding in &scope.bindings {
                if !binding.moved && !is_copy_type(&binding.ty) {
                    drops.push(binding.operand);
                }
            }
            // Stop when we hit a function scope.
            if matches!(scope.kind, ScopeKind::Function) {
                break;
            }
        }
        drops
    }
}

/// Convert a decimal string to bigint limbs (little-endian base 2^32).
fn parse_decimal_to_limbs(text: &str) -> Result<(Vec<u32>, bool), ()> {
    let (negative, text) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };

    // Parse digit by digit, multiply by 10 and add.
    let mut limbs: Vec<u32> = vec![0];

    for c in text.chars() {
        let digit = c.to_digit(10).ok_or(())?;

        // Multiply all limbs by 10.
        let mut carry: u64 = 0;
        for limb in &mut limbs {
            let product = (*limb as u64) * 10 + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }

        // Add the digit.
        let mut add_carry: u64 = digit as u64;
        for limb in &mut limbs {
            let sum = (*limb as u64) + add_carry;
            *limb = sum as u32;
            add_carry = sum >> 32;
            if add_carry == 0 {
                break;
            }
        }
        if add_carry > 0 {
            limbs.push(add_carry as u32);
        }
    }

    // Remove trailing zeros from the end (high-order limbs).
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }

    // Handle zero case.
    if limbs.len() == 1 && limbs[0] == 0 {
        limbs.clear();
    }

    let is_nonzero = !limbs.is_empty();
    Ok((limbs, negative && is_nonzero))
}

/// Convert a hex string to bigint limbs (little-endian base 2^32).
fn parse_hex_to_limbs(hex_str: &str) -> Result<(Vec<u32>, bool), ()> {
    // Parse from right to left, 8 hex digits at a time = 1 u32 limb.
    let mut limbs = Vec::new();
    let len = hex_str.len();

    let mut i = len;
    while i > 0 {
        let start = if i >= 8 { i - 8 } else { 0 };
        let chunk = &hex_str[start..i];
        let limb = u32::from_str_radix(chunk, 16).map_err(|_| ())?;
        limbs.push(limb);
        i = start;
    }

    // Remove trailing zeros.
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }

    // Handle zero case.
    if limbs.len() == 1 && limbs[0] == 0 {
        limbs.clear();
    }

    // Hex is always non-negative for now.
    Ok((limbs, false))
}

/// Parse an integer literal into a ConstValue based on the target type.
fn parse_int_const(text: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => text.parse::<u8>().map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => text.parse::<u16>().map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => text.parse::<u32>().map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => text.parse::<u64>().map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => text.parse::<i8>().map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => text.parse::<i16>().map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => text.parse::<i32>().map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => text.parse::<i64>().map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            let (limbs, negative) = parse_decimal_to_limbs(text)?;
            Ok(ConstValue::Int { limbs, negative })
        }
        _ => Err(()),
    }
}

/// Parse a hex literal into a ConstValue based on the target type.
fn parse_hex_const(hex_str: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => u8::from_str_radix(hex_str, 16).map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => u16::from_str_radix(hex_str, 16).map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => u32::from_str_radix(hex_str, 16).map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => u64::from_str_radix(hex_str, 16).map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => i8::from_str_radix(hex_str, 16).map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => i16::from_str_radix(hex_str, 16).map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => i32::from_str_radix(hex_str, 16).map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => i64::from_str_radix(hex_str, 16).map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            let (limbs, negative) = parse_hex_to_limbs(hex_str)?;
            Ok(ConstValue::Int { limbs, negative })
        }
        _ => Err(()),
    }
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
    db: &'db dyn Db,
    /// Expression types from typechecker.
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    /// Next ValueId to allocate.
    next_value: u32,
    /// Next SlotId to allocate.
    next_slot: u32,
    /// Next BlockId to allocate.
    next_block: u32,
    /// Blocks being built.
    blocks: Vec<IrBlock>,
    /// Current block being built.
    current_block: BlockId,
    /// Instructions for current block.
    current_instructions: Vec<Instruction>,
    /// Mapping from variable names to their operands.
    variables: HashMap<String, Operand>,
    /// Script context for external lookups (None for functions).
    script_ctx: Option<ScriptLowerContext>,
    /// Exports from this unit (only used for script units).
    exports: Vec<(String, ExportBinding)>,
    /// Functions defined in this script unit.
    functions: Vec<IrFunction>,
    /// Symbol table for function resolution.
    symbols: SymbolTable,
    /// Available functions: name -> FuncRef (for resolving calls).
    func_scope: HashMap<String, FuncRef>,
    /// Current unit index (for script units).
    current_unit: u32,
    /// Type for each ValueId.
    value_types: Vec<IrType>,
    /// Type for each SlotId.
    slot_types: Vec<IrType>,
    /// Loop context stack: (continue_target, break_target) for each nested loop.
    loop_stack: Vec<(BlockId, BlockId)>,
    /// Scope tracker for emitting drops at scope exits.
    scope_tracker: ScopeTracker,
    /// Return type for current function/script (for try operators).
    return_type: Option<IrType>,
    /// Whether we're in a script unit (vs function).
    is_script_unit: bool,
    /// Temporary values to drop after the current expression is evaluated.
    /// These are created during operand lowering for compound expressions.
    expr_temps: Vec<(ValueId, IrType)>,
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
            script_ctx: None,
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope: HashMap::new(),
            current_unit: 0,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            scope_tracker: ScopeTracker::new(),
            return_type: None,
            is_script_unit: false,
            expr_temps: Vec::new(),
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
            script_ctx: None,
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            current_unit: 0,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            scope_tracker: ScopeTracker::new(),
            return_type: None,
            is_script_unit: false,
            expr_temps: Vec::new(),
        }
    }

    /// Get the IrType for an expression from the typechecker.
    fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
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

        let current_unit = script_ctx.current_unit;

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
            script_ctx: Some(script_ctx),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            current_unit,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            loop_stack: Vec::new(),
            scope_tracker: ScopeTracker::new(),
            // Script units have Result<()> return type for ! operator.
            return_type: Some(IrType::Result(Box::new(IrType::Unit))),
            is_script_unit: true,
            expr_temps: Vec::new(),
        }
    }

    /// Define a function in the current scope.
    fn define_func(&mut self, name: &str, param_count: usize) -> FuncId {
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), FuncRef::Local(func_id));
        func_id
    }

    /// Look up a function by name.
    fn lookup_func(&self, name: &str) -> Option<FuncRef> {
        self.func_scope.get(name).cloned()
    }

    /// Allocate a fresh SSA value with known type.
    fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ty);
        id
    }

    /// Allocate a fresh mutable slot with known type.
    fn fresh_slot(&mut self, ty: IrType) -> SlotId {
        let id = SlotId(self.next_slot);
        self.next_slot += 1;
        self.slot_types.push(ty);
        id
    }

    /// Allocate a fresh block.
    fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    /// Emit an instruction to the current block.
    fn emit(&mut self, instr: Instruction) {
        self.current_instructions.push(instr);
    }

    /// Finish current block with a terminator, start a new block.
    fn finish_block(&mut self, terminator: Terminator) -> BlockId {
        let block = IrBlock {
            id: self.current_block,
            instructions: std::mem::take(&mut self.current_instructions),
            terminator,
        };
        self.blocks.push(block);
        self.current_block
    }

    /// Start building a new block.
    fn start_block(&mut self, id: BlockId) {
        self.current_block = id;
        self.current_instructions.clear();
    }

    /// Bind a variable name to an operand.
    fn bind_var(&mut self, name: &str, operand: Operand) {
        self.variables.insert(name.to_string(), operand);
    }

    /// Look up a variable.
    fn lookup_var(&self, name: &str) -> Option<Operand> {
        self.variables.get(name).copied()
    }

    /// Emit Drop instructions for the given operands.
    fn emit_drops(&mut self, operands: Vec<Operand>) {
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Record an expression temporary that needs dropping after the operation.
    fn record_expr_temp(&mut self, value: ValueId, ty: IrType) {
        if !is_copy_type(&ty) {
            self.expr_temps.push((value, ty));
        }
    }

    /// Emit Drop instructions for all expression temporaries and clear the list.
    fn emit_expr_temp_drops(&mut self) {
        let temps = std::mem::take(&mut self.expr_temps);
        for (value, _ty) in temps {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Get the type for a value ID.
    fn value_type(&self, id: ValueId) -> Option<&IrType> {
        self.value_types.get(id.0 as usize)
    }

    /// Get the type for a slot ID.
    fn slot_type(&self, id: SlotId) -> Option<&IrType> {
        self.slot_types.get(id.0 as usize)
    }
}

/// Lower a function to IR.
///
/// For standalone function lowering (not in a script context).
pub fn lower_function<'db>(
    db: &'db dyn Db,
    tycheck_result: TypecheckResult<'db>,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    lower_function_with_expr_types(db, tycheck_result.expr_types(db), func)
}

/// Lower a function to IR using pre-computed expr_types.
///
/// This variant is useful when lowering functions from a module graph
/// where expr_types are combined across all modules.
pub fn lower_function_with_expr_types<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    lower_function_for_module(db, expr_types, &[], func)
}

/// Lower a function to IR with available module functions in scope.
///
/// This variant is used when lowering module functions that may call
/// other module functions (imported from other modules).
pub fn lower_function_for_module<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    available_functions: &[String],
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new_for_module(db, expr_types, available_functions);
    let name = func.name(db).text(db).to_string();
    let param_count = func.params(db).len();

    // Define the function in the symbol table.
    let func_id = ctx.define_func(&name, param_count);

    lower_function_body(&mut ctx, func_id, func)
}

/// Lower a function body given an already-allocated FuncId.
fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let name = func.name(ctx.db).text(ctx.db).to_string();

    // Save and set function context for try operators.
    let saved_return_type = ctx.return_type.take();
    let saved_is_script_unit = ctx.is_script_unit;
    ctx.is_script_unit = false;

    // Set return type from function signature.
    ctx.return_type = func.return_type(ctx.db).map(|ty| IrType::from_type_hint(ctx.db, &ty));

    // Enter function scope for drop tracking.
    ctx.scope_tracker.enter_scope(ScopeKind::Function);

    // Allocate ValueIds for parameters with correct types.
    let params: Vec<ValueId> = func.params(ctx.db)
        .iter()
        .map(|p| {
            let param_name = p.name(ctx.db).text(ctx.db).to_string();
            let param_type = IrType::from_type_hint(ctx.db, &p.type_hint(ctx.db));
            let id = ctx.fresh_value(param_type.clone());
            ctx.bind_var(&param_name, Operand::Value(id));
            // Record parameter for drop tracking.
            ctx.scope_tracker.record_binding(Operand::Value(id), param_type);
            id
        })
        .collect();

    // Lower the function body.
    for stmt in func.body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // If no explicit return, add implicit return unit.
    // Emit drops before the implicit return.
    if ctx.current_instructions.is_empty()
        || !matches!(ctx.blocks.last().map(|b| &b.terminator), Some(Terminator::Return { .. }))
    {
        // Check if we already have a return as the last instruction.
        let needs_return = ctx.blocks.is_empty()
            || !matches!(ctx.blocks.last().unwrap().terminator, Terminator::Return { .. });
        if needs_return {
            // Emit drops before implicit return.
            let drops = ctx.scope_tracker.exit_scope();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Return { value: None });
        }
    } else {
        // Scope already exited by explicit return, just pop it.
        ctx.scope_tracker.scopes.pop();
    }

    // Restore saved context.
    ctx.return_type = saved_return_type;
    ctx.is_script_unit = saved_is_script_unit;

    Ok(IrFunction {
        id: func_id,
        name,
        params,
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
    })
}

/// Lower a statement.
fn lower_statement<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let init_expr = let_stmt.value(ctx.db);
            let value_type = ctx.expr_type(init_expr);
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.bind_var(&name, Operand::Value(value_id));
            // Record binding for drop tracking.
            ctx.scope_tracker.record_binding(Operand::Value(value_id), value_type);
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            // Get type from the initialization expression.
            let init_expr = var_stmt.value(ctx.db);
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type.clone());
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            ctx.bind_var(&name, Operand::Slot(slot));
            // Record slot for drop tracking.
            ctx.scope_tracker.record_binding(Operand::Slot(slot), slot_type);
            Ok(())
        }
        Statement::Set(set_stmt) => {
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value(ctx.db))?;
            if let Some(Operand::Slot(slot)) = ctx.lookup_var(&name) {
                // Drop old value before storing new one.
                if let Some(slot_type) = ctx.slot_type(slot).cloned() {
                    if !is_copy_type(&slot_type) {
                        ctx.emit(Instruction::Drop { operand: Operand::Slot(slot) });
                    }
                }
                ctx.emit(Instruction::SlotStore {
                    dest: SlotDest::Local(slot),
                    value: Operand::Value(value_id),
                });
                Ok(())
            } else {
                Err(LowerError::VariableNotMutable(name))
            }
        }
        Statement::Ret(ret_stmt) => {
            let value = if let Some(expr) = ret_stmt.value(ctx.db) {
                let value_id = lower_expression(ctx, expr)?;
                let operand = Operand::Value(value_id);
                // Mark return value as moved (not dropped).
                ctx.scope_tracker.mark_moved(&operand);
                Some(operand)
            } else {
                None
            };
            // Emit drops for all values in all scopes before return.
            let drops = ctx.scope_tracker.bindings_to_drop_for_return();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Return { value });
            // Start a new unreachable block (code after return).
            let new_block = ctx.fresh_block();
            ctx.start_block(new_block);
            Ok(())
        }
        Statement::If(if_stmt) => {
            lower_if(ctx, *if_stmt)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, *loop_stmt)
        }
        Statement::Break(_) => {
            let (_, break_target) = ctx.loop_stack.last()
                .ok_or(LowerError::BreakOutsideLoop)?;
            let break_target = *break_target;
            // Emit drops for all scopes up to the loop.
            let drops = ctx.scope_tracker.bindings_to_drop_for_break();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Goto(break_target));
            // Start unreachable block for code after break.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Continue(_) => {
            let (continue_target, _) = ctx.loop_stack.last()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            let continue_target = *continue_target;
            // Emit drops for current loop iteration.
            let drops = ctx.scope_tracker.bindings_to_drop_for_continue();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Goto(continue_target));
            // Start unreachable block for code after continue.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Fun(_) => {
            // Nested functions not supported in IR yet.
            Err(LowerError::NotImplemented("nested functions".to_string()))
        }
        Statement::Require(_) | Statement::Import(_) => {
            // These are module-level, not in function bodies.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}

/// Lower an if statement.
///
/// Handles three cases based on condition type:
/// - Bool: regular if/else
/// - Option with then_binding: destructure Some value
/// - Result with then_binding and else_binding: destructure Ok/Err values
fn lower_if<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
) -> Result<(), LowerError> {
    let condition = if_stmt.condition(ctx.db);
    let then_binding = if_stmt.then_binding(ctx.db);
    let else_binding = if_stmt.else_binding(ctx.db);
    let cond_type = ctx.expr_type(condition);

    match (&cond_type, then_binding) {
        // Option destructuring: if opt_value |x| ... end if
        (IrType::Option(inner_type), Some(binding_name)) => {
            lower_if_option(ctx, if_stmt, condition, inner_type, binding_name)
        }

        // Result destructuring: if result_value |ok_val| else |err_val| ... end if
        (IrType::Result(ok_type), Some(binding_name)) => {
            // Typechecker enforces else_binding for Result (F046).
            let err_binding = else_binding
                .ok_or_else(|| LowerError::NotImplemented(
                    "Result if-binding without else binding".to_string()
                ))?;
            lower_if_result(ctx, if_stmt, condition, ok_type, binding_name, err_binding)
        }

        // Boolean condition (no binding).
        (IrType::Bool, None) => {
            lower_if_bool(ctx, if_stmt, condition)
        }

        // Invalid combinations.
        (_, Some(_)) => {
            // Binding on non-Option/non-Result type.
            Err(LowerError::NotImplemented(format!(
                "if-binding requires Option or Result type, got {:?}",
                cond_type
            )))
        }
        (_, None) => {
            // Non-bool without binding - typechecker should catch this.
            Err(LowerError::NotImplemented(format!(
                "if condition must be Bool without binding, got {:?}",
                cond_type
            )))
        }
    }
}

/// Lower a boolean if statement (no binding).
fn lower_if_bool<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
) -> Result<(), LowerError> {
    let cond_id = lower_expression(ctx, condition)?;

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    // Finish current block with branch.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(cond_id),
        then_block,
        else_block,
    });

    // Lower then branch.
    ctx.start_block(then_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfThen { merge: merge_block });
    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }
    // Exit scope and emit drops before Goto.
    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    // Lower else branch.
    ctx.start_block(else_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfElse { merge: merge_block });
    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }
    // Exit scope and emit drops before Goto.
    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    // Continue in merge block.
    ctx.start_block(merge_block);
    Ok(())
}

/// Lower an Option if-binding: `if opt_value |x| ... end if`
///
/// Moves the inner value out of Some to the binding. If None, takes else branch.
fn lower_if_option<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
    inner_type: &IrType,
    binding_name: InternedText<'db>,
) -> Result<(), LowerError> {
    // Lower the Option expression.
    let opt_id = lower_expression(ctx, condition)?;

    // Emit UnwrapOption instruction.
    // dest: receives inner value (only valid when is_some=true).
    // is_some: boolean flag for branching.
    let inner_dest = ctx.fresh_value(inner_type.clone());
    let is_some = ctx.fresh_value(IrType::Bool);

    ctx.emit(Instruction::UnwrapOption {
        dest: inner_dest,
        is_some,
        src: Operand::Value(opt_id),
    });

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_some),
        then_block,
        else_block,
    });

    // === Then branch: Some case ===
    ctx.start_block(then_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfThen { merge: merge_block });

    // Bind the inner value to the binding name.
    let binding_str = binding_name.text(ctx.db);
    let old_binding = ctx.lookup_var(binding_str);
    ctx.bind_var(binding_str, Operand::Value(inner_dest));

    // Track the binding for drops at scope exit.
    ctx.scope_tracker.record_binding(Operand::Value(inner_dest), inner_type.clone());

    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // Restore old binding if we shadowed something.
    if let Some(old) = old_binding {
        ctx.bind_var(binding_str, old);
    } else {
        ctx.variables.remove(binding_str);
    }

    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    // === Else branch: None case ===
    ctx.start_block(else_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfElse { merge: merge_block });
    // No binding in else branch for Option.
    // inner_dest is NOT valid here - do NOT access or drop it.

    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }

    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    ctx.start_block(merge_block);
    Ok(())
}

/// Lower a Result if-binding: `if result_value |ok_val| else |err_val| ... end if`
///
/// Moves the Ok payload to then_binding, or Error to else_binding.
fn lower_if_result<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
    ok_type: &IrType,
    ok_binding: InternedText<'db>,
    err_binding: InternedText<'db>,
) -> Result<(), LowerError> {
    // Lower the Result expression.
    let result_id = lower_expression(ctx, condition)?;

    // Emit UnwrapResult instruction.
    // ok_dest: receives Ok payload (only valid when is_ok=true).
    // err_dest: receives Error (only valid when is_ok=false).
    // is_ok: boolean flag for branching.
    let ok_dest = ctx.fresh_value(ok_type.clone());
    let err_dest = ctx.fresh_value(IrType::Error);
    let is_ok = ctx.fresh_value(IrType::Bool);

    ctx.emit(Instruction::UnwrapResult {
        ok_dest,
        err_dest,
        is_ok,
        src: Operand::Value(result_id),
    });

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_ok),
        then_block,
        else_block,
    });

    // === Then branch: Ok case ===
    ctx.start_block(then_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfThen { merge: merge_block });

    // Bind ok_dest to the ok_binding name.
    let ok_binding_str = ok_binding.text(ctx.db);
    let old_ok_binding = ctx.lookup_var(ok_binding_str);
    ctx.bind_var(ok_binding_str, Operand::Value(ok_dest));

    // Track for drops.
    ctx.scope_tracker.record_binding(Operand::Value(ok_dest), ok_type.clone());

    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // Restore old binding.
    if let Some(old) = old_ok_binding {
        ctx.bind_var(ok_binding_str, old);
    } else {
        ctx.variables.remove(ok_binding_str);
    }

    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    // === Else branch: Error case ===
    ctx.start_block(else_block);
    ctx.scope_tracker.enter_scope(ScopeKind::IfElse { merge: merge_block });

    // Bind err_dest to the err_binding name.
    let err_binding_str = err_binding.text(ctx.db);
    let old_err_binding = ctx.lookup_var(err_binding_str);
    ctx.bind_var(err_binding_str, Operand::Value(err_dest));

    // Track for drops (Error type is always non-copy).
    ctx.scope_tracker.record_binding(Operand::Value(err_dest), IrType::Error);

    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }

    // Restore old binding.
    if let Some(old) = old_err_binding {
        ctx.bind_var(err_binding_str, old);
    } else {
        ctx.variables.remove(err_binding_str);
    }

    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);
    ctx.finish_block(Terminator::Goto(merge_block));

    ctx.start_block(merge_block);
    Ok(())
}

/// Lower a loop statement.
fn lower_loop<'db>(
    ctx: &mut LowerCtx<'db>,
    loop_stmt: ast::StmtLoop<'db>,
) -> Result<(), LowerError> {
    let loop_header = ctx.fresh_block();
    let loop_exit = ctx.fresh_block();

    // Jump to loop header.
    ctx.finish_block(Terminator::Goto(loop_header));

    // Push loop context for break/continue.
    ctx.loop_stack.push((loop_header, loop_exit));

    // Enter loop scope for drop tracking.
    ctx.scope_tracker.enter_scope(ScopeKind::Loop {
        header: loop_header,
        exit: loop_exit,
    });

    // Lower loop body.
    ctx.start_block(loop_header);
    for stmt in loop_stmt.body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // Exit loop scope and emit drops before looping back.
    let drops = ctx.scope_tracker.exit_scope();
    ctx.emit_drops(drops);

    // Loop back to header.
    ctx.finish_block(Terminator::Goto(loop_header));

    // Pop loop context.
    ctx.loop_stack.pop();

    // Continue after loop.
    ctx.start_block(loop_exit);
    Ok(())
}

/// Lower an operand for borrowing contexts (binop, unaryop).
///
/// Returns an Operand directly:
/// - For Names bound to slots: returns Operand::Slot (no load, just borrow)
/// - For Names bound to values: returns Operand::Value
/// - For compound expressions: evaluates and returns Operand::Value(result)
fn lower_operand<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<Operand, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            if let Some(operand) = ctx.lookup_var(name_str) {
                // Return the operand directly - no load needed for borrowing.
                Ok(operand)
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        _ => {
            // Compound expression: lower to a value.
            let expr_type = ctx.expr_type(expr);
            let value_id = lower_expression(ctx, expr)?;
            // Record as temp for dropping after the borrowing operation completes.
            ctx.record_expr_temp(value_id, expr_type);
            Ok(Operand::Value(value_id))
        }
    }
}

/// Lower an expression, returning the ValueId holding the result.
fn lower_expression<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ValueId, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            if let Some(operand) = ctx.lookup_var(name_str) {
                match operand {
                    Operand::Value(v) => {
                        // For SSA values, just return the existing ValueId.
                        Ok(v)
                    }
                    Operand::Slot(s) => {
                        // For slots, emit a load.
                        let slot_type = ctx.expr_type(expr);
                        let dest = ctx.fresh_value(slot_type);
                        ctx.emit(Instruction::SlotLoad { dest, slot: s });
                        Ok(dest)
                    }
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                        // External operands from previous script units.
                        // Copy types use Copy, non-copy types use Move.
                        let ext_type = ctx.expr_type(expr);
                        let dest = ctx.fresh_value(ext_type.clone());
                        if ext_type.is_copy() {
                            ctx.emit(Instruction::Copy { dest, src: operand });
                        } else {
                            ctx.emit(Instruction::Move { dest, src: operand });
                        }
                        Ok(dest)
                    }
                }
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        ExprFunKind::True(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(true),
            });
            Ok(dest)
        }
        ExprFunKind::False(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(false),
            });
            Ok(dest)
        }
        ExprFunKind::None(_) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapNone { dest });
            Ok(dest)
        }
        ExprFunKind::Int(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value(ctx.db).text(ctx.db);
            let const_value = parse_int_const(text, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value(ctx.db).text(ctx.db);
            let hex_str = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
            let const_value = parse_hex_const(hex_str, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::BinOp(binop) => {
            // Use lower_operand for borrowing semantics - operands are read by
            // reference, not consumed.
            let lhs = lower_operand(ctx, binop.lhs(ctx.db))?;
            let rhs = lower_operand(ctx, binop.rhs(ctx.db))?;

            let ast_op = binop.op(ctx.db);
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            let op = match ast_op {
                ast::BinOp::Add => BinOp::Add,
                ast::BinOp::Sub => BinOp::Sub,
                ast::BinOp::Mul => BinOp::Mul,
                ast::BinOp::Div => BinOp::Div,
                ast::BinOp::Eq => BinOp::Eq,
                ast::BinOp::Ne => BinOp::Ne,
                ast::BinOp::Lt => BinOp::Lt,
                ast::BinOp::Le => BinOp::Le,
                ast::BinOp::Gt => BinOp::Gt,
                ast::BinOp::Ge => BinOp::Ge,
                // Checked/optional ops - emit as checked for now.
                ast::BinOp::AddChecked | ast::BinOp::AddOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Add,
                        lhs,
                        rhs,
                    });
                    ctx.emit_expr_temp_drops();
                    return Ok(dest);
                }
                ast::BinOp::SubChecked | ast::BinOp::SubOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Sub,
                        lhs,
                        rhs,
                    });
                    ctx.emit_expr_temp_drops();
                    return Ok(dest);
                }
                ast::BinOp::MulChecked | ast::BinOp::MulOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Mul,
                        lhs,
                        rhs,
                    });
                    ctx.emit_expr_temp_drops();
                    return Ok(dest);
                }
                ast::BinOp::DivChecked | ast::BinOp::DivOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Div,
                        lhs,
                        rhs,
                    });
                    ctx.emit_expr_temp_drops();
                    return Ok(dest);
                }
            };

            ctx.emit(Instruction::BinOp {
                dest,
                op,
                lhs,
                rhs,
            });
            // Drop expression temporaries after borrowing operation completes.
            ctx.emit_expr_temp_drops();
            Ok(dest)
        }
        ExprFunKind::UnaryOp(unary) => {
            // Use lower_operand for borrowing semantics.
            let operand = lower_operand(ctx, unary.operand(ctx.db))?;

            let op = match unary.op(ctx.db) {
                ast::UnaryOp::Neg => UnaryOp::Neg,
                ast::UnaryOp::NegOptional | ast::UnaryOp::NegResult => {
                    // TODO: Handle checked unary ops.
                    UnaryOp::Neg
                }
            };

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::UnaryOp {
                dest,
                op,
                operand,
            });
            // Drop expression temporaries after borrowing operation completes.
            ctx.emit_expr_temp_drops();
            Ok(dest)
        }
        ExprFunKind::FunctionCall(call) => {
            let func_name = call.name(ctx.db).text(ctx.db).to_string();
            let args: Result<Vec<_>, _> = call.args(ctx.db)
                .iter()
                .map(|arg| lower_expression(ctx, *arg).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            // Resolve function reference.
            let func_ref = ctx.lookup_func(&func_name)
                .ok_or_else(|| LowerError::FunctionNotFound(func_name))?;

            ctx.emit(Instruction::Call {
                dest,
                func: func_ref,
                args: args?,
            });
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapSome {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapOk {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapErr {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::TryOption(try_expr) => {
            let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let is_some = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::UnwrapOption {
                dest,
                is_some,
                src: Operand::Value(src_id),
            });

            // Create early return and continue blocks.
            let early_return_block = ctx.fresh_block();
            let continue_block = ctx.fresh_block();

            // Branch: if is_some, continue; else early return.
            ctx.finish_block(Terminator::Branch {
                cond: Operand::Value(is_some),
                then_block: continue_block,
                else_block: early_return_block,
            });

            // Early return block: wrap None and return.
            ctx.start_block(early_return_block);
            let return_type = ctx.return_type.clone()
                .expect("try operator requires return type");
            let none_value = ctx.fresh_value(return_type);
            ctx.emit(Instruction::WrapNone { dest: none_value });
            if ctx.is_script_unit {
                ctx.finish_block(Terminator::UnitEarlyReturn {
                    value: Operand::Value(none_value),
                });
            } else {
                ctx.finish_block(Terminator::TryReturn {
                    value: Some(Operand::Value(none_value)),
                });
            }

            // Continue block: dest already has the unwrapped value.
            ctx.start_block(continue_block);
            Ok(dest)
        }
        ExprFunKind::TryResult(try_expr) => {
            let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let ok_dest = ctx.fresh_value(result_type.clone());
            let err_dest = ctx.fresh_value(IrType::Error);
            let is_ok = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::UnwrapResult {
                ok_dest,
                err_dest,
                is_ok,
                src: Operand::Value(src_id),
            });

            // Create early return and continue blocks.
            let early_return_block = ctx.fresh_block();
            let continue_block = ctx.fresh_block();

            // Branch: if is_ok, continue; else early return.
            ctx.finish_block(Terminator::Branch {
                cond: Operand::Value(is_ok),
                then_block: continue_block,
                else_block: early_return_block,
            });

            // Early return block: wrap error and return.
            ctx.start_block(early_return_block);
            let return_type = ctx.return_type.clone()
                .expect("try operator requires return type");
            let wrapped_err = ctx.fresh_value(return_type);
            ctx.emit(Instruction::WrapErr {
                dest: wrapped_err,
                inner: Operand::Value(err_dest),
            });
            if ctx.is_script_unit {
                ctx.finish_block(Terminator::UnitEarlyReturn {
                    value: Operand::Value(wrapped_err),
                });
            } else {
                ctx.finish_block(Terminator::TryReturn {
                    value: Some(Operand::Value(wrapped_err)),
                });
            }

            // Continue block: ok_dest has the unwrapped Ok value.
            ctx.start_block(continue_block);
            Ok(ok_dest)
        }
        ExprFunKind::Tuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::AnonTuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::List(list) => {
            let elements: Result<Vec<_>, _> = list.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ListNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Set(set) => {
            let elements: Result<Vec<_>, _> = set.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::SetNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Map(map) => {
            let entries: Result<Vec<_>, _> = map.entries(ctx.db)
                .iter()
                .map(|e| {
                    let k = lower_expression(ctx, e.key(ctx.db))?;
                    let v = lower_expression(ctx, e.value(ctx.db))?;
                    Ok((Operand::Value(k), Operand::Value(v)))
                })
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::MapNew {
                dest,
                entries: entries?,
            });
            Ok(dest)
        }
        ExprFunKind::Err(err_expr) => {
            let inner_id = lower_expression(ctx, err_expr.value(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ErrorFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Data(data_expr) => {
            let inner_id = lower_expression(ctx, data_expr.value(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::DataFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Float(_) => {
            Err(LowerError::NotImplemented("Float".to_string()))
        }
        ExprFunKind::String(_) => {
            Err(LowerError::NotImplemented("String".to_string()))
        }
        ExprFunKind::AnonStruct(_) => {
            Err(LowerError::NotImplemented("AnonStruct".to_string()))
        }
        ExprFunKind::AnonEnum(_) => {
            Err(LowerError::NotImplemented("AnonEnum".to_string()))
        }
        ExprFunKind::Tensor(_) => {
            Err(LowerError::NotImplemented("Tensor".to_string()))
        }
        ExprFunKind::ParseError(_) => {
            Err(LowerError::NotImplemented("ParseError".to_string()))
        }
    }
}

/// Lower a script unit.
///
/// Script units are sequences of statements (fragment) or a single expression (expr).
/// They can reference values from previous units and export bindings to subsequent units.
pub fn lower_script_unit<'db>(
    db: &'db dyn Db,
    tycheck_result: TypecheckResult<'db>,
    script_ctx: ScriptLowerContext,
    kind: ScriptUnitKind<'db>,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, tycheck_result.expr_types(db), script_ctx);

    // Enter script unit scope. Top-level bindings are exported, not dropped.
    ctx.scope_tracker.enter_scope(ScopeKind::ScriptUnit);

    let result = match kind {
        ScriptUnitKind::Fragment(stmts) => {
            // Lower all statements.
            for stmt in &stmts {
                lower_statement_for_script(&mut ctx, stmt)?;
            }
            // Fragment units have no result value.
            None
        }
        ScriptUnitKind::Expr(expr) => {
            // Lower the expression and capture the result.
            let value_id = lower_expression(&mut ctx, expr)?;
            Some(value_id)
        }
    };

    // Exit scope (no drops for ScriptUnit - bindings are exported).
    ctx.scope_tracker.exit_scope();

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: result.map(Operand::Value),
    });

    Ok(IrScriptUnit {
        blocks: ctx.blocks,
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result,
        exports: ctx.exports,
    })
}

/// Lower a script fragment unit with raw expr_types.
///
/// Like `lower_script_unit` but takes expr_types directly instead of TypecheckResult.
/// Used when typechecking with context (non-salsa version).
pub fn lower_script_fragment_raw<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    script_ctx: ScriptLowerContext,
    stmts: Vec<crate::ast::Statement<'db>>,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, script_ctx);

    // Enter script unit scope. Top-level bindings are exported, not dropped.
    ctx.scope_tracker.enter_scope(ScopeKind::ScriptUnit);

    // Lower all statements.
    for stmt in &stmts {
        lower_statement_for_script(&mut ctx, stmt)?;
    }

    // Exit scope (no drops for ScriptUnit - bindings are exported).
    ctx.scope_tracker.exit_scope();

    // Fragment units have no result value.
    ctx.finish_block(Terminator::UnitEnd { result: None });

    Ok(IrScriptUnit {
        blocks: ctx.blocks,
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: None,
        exports: ctx.exports,
    })
}

/// Lower a script expression unit.
///
/// Like `lower_script_unit` but takes expr_types directly and an expression.
pub fn lower_script_expr<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    script_ctx: ScriptLowerContext,
    expr: crate::ast::ExprFun<'db>,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, script_ctx);

    // Lower the expression and capture the result.
    let value_id = lower_expression(&mut ctx, expr)?;

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: Some(Operand::Value(value_id)),
    });

    Ok(IrScriptUnit {
        blocks: ctx.blocks,
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: Some(value_id),
        exports: ctx.exports,
    })
}

/// Lower a statement in script unit context.
///
/// This handles function definitions by lowering them and adding to the unit's functions.
fn lower_statement_for_script<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let init_expr = let_stmt.value(ctx.db);
            let value_type = ctx.expr_type(init_expr);
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.bind_var(&name, Operand::Value(value_id));
            // Record binding for drop tracking.
            // Note: ScriptUnit scope bindings are exported, so they won't be dropped.
            ctx.scope_tracker.record_binding(Operand::Value(value_id), value_type);
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Value(value_id)));
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            let init_expr = var_stmt.value(ctx.db);
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type.clone());
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            ctx.bind_var(&name, Operand::Slot(slot));
            // Record slot for drop tracking.
            ctx.scope_tracker.record_binding(Operand::Slot(slot), slot_type);
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Slot(slot)));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            // Same as function lowering - no export needed for assignment.
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value(ctx.db))?;
            if let Some(operand) = ctx.lookup_var(&name) {
                match operand {
                    Operand::Slot(slot) => {
                        // Drop old value before storing new one.
                        if let Some(slot_type) = ctx.slot_type(slot).cloned() {
                            if !is_copy_type(&slot_type) {
                                ctx.emit(Instruction::Drop { operand: Operand::Slot(slot) });
                            }
                        }
                        ctx.emit(Instruction::SlotStore {
                            dest: SlotDest::Local(slot),
                            value: Operand::Value(value_id),
                        });
                        Ok(())
                    }
                    Operand::ExternalSlot { unit, slot } => {
                        // TODO: External slot drops need special handling.
                        // For now, skip drop since we can't easily get the type.
                        ctx.emit(Instruction::SlotStore {
                            dest: SlotDest::External { unit, slot },
                            value: Operand::Value(value_id),
                        });
                        Ok(())
                    }
                    _ => Err(LowerError::VariableNotMutable(name)),
                }
            } else {
                Err(LowerError::VariableNotFound(name))
            }
        }
        Statement::Ret(ret_stmt) => {
            // In scripts, return means early return from the unit.
            let value = if let Some(expr) = ret_stmt.value(ctx.db) {
                Operand::Value(lower_expression(ctx, expr)?)
            } else {
                // Return unit value for bare `ret`.
                let unit_val = ctx.fresh_value(IrType::Unit);
                ctx.emit(Instruction::Const {
                    dest: unit_val,
                    value: ConstValue::Unit,
                });
                Operand::Value(unit_val)
            };
            // Note: Script unit early returns don't drop top-level bindings.
            // Those are exported and cleaned up at script finalize.
            ctx.finish_block(Terminator::UnitEarlyReturn { value });
            // Start a new unreachable block.
            let new_block = ctx.fresh_block();
            ctx.start_block(new_block);
            Ok(())
        }
        Statement::Fun(fun_stmt) => {
            // Define the function in the symbol table first (allows recursion).
            let func_name = fun_stmt.name(ctx.db).text(ctx.db).to_string();
            let param_count = fun_stmt.params(ctx.db).len();
            let func_id = ctx.define_func(&func_name, param_count);

            // Save current lowering state.
            let saved_blocks = std::mem::take(&mut ctx.blocks);
            let saved_instructions = std::mem::take(&mut ctx.current_instructions);
            let saved_current_block = ctx.current_block;
            let saved_next_block = ctx.next_block;
            let saved_next_value = ctx.next_value;
            let saved_next_slot = ctx.next_slot;
            let saved_variables = std::mem::take(&mut ctx.variables);
            let saved_scope_tracker = std::mem::take(&mut ctx.scope_tracker);

            // Reset for function body.
            ctx.current_block = BlockId(0);
            ctx.next_block = 1;
            ctx.next_value = 0;
            ctx.next_slot = 0;
            ctx.scope_tracker = ScopeTracker::new();

            // Lower the function body.
            let func = lower_function_body(ctx, func_id, *fun_stmt)?;

            // Restore parent state.
            ctx.blocks = saved_blocks;
            ctx.current_instructions = saved_instructions;
            ctx.current_block = saved_current_block;
            ctx.next_block = saved_next_block;
            ctx.next_value = saved_next_value;
            ctx.next_slot = saved_next_slot;
            ctx.variables = saved_variables;
            ctx.scope_tracker = saved_scope_tracker;

            // Add the function to the unit's functions.
            ctx.functions.push(func);

            // Export the function.
            ctx.exports.push((func_name, ExportBinding::Function(func_id)));
            Ok(())
        }
        Statement::If(if_stmt) => {
            lower_if(ctx, *if_stmt)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, *loop_stmt)
        }
        Statement::Break(_) => {
            let (_, break_target) = ctx.loop_stack.last()
                .ok_or(LowerError::BreakOutsideLoop)?;
            let break_target = *break_target;
            // Emit drops for all scopes up to the loop.
            let drops = ctx.scope_tracker.bindings_to_drop_for_break();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Goto(break_target));
            // Start unreachable block for code after break.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Continue(_) => {
            let (continue_target, _) = ctx.loop_stack.last()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            let continue_target = *continue_target;
            // Emit drops for current loop iteration.
            let drops = ctx.scope_tracker.bindings_to_drop_for_continue();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Goto(continue_target));
            // Start unreachable block for code after continue.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Require(_) | Statement::Import(_) => {
            // Module-level, handled elsewhere.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}

/// Errors that can occur during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    VariableNotFound(String),
    VariableNotMutable(String),
    FunctionNotFound(String),
    InvalidLiteral(String),
    NotImplemented(String),
    ParseError,
    BreakOutsideLoop,
    ContinueOutsideLoop,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::VariableNotFound(name) => write!(f, "variable not found: {}", name),
            LowerError::VariableNotMutable(name) => write!(f, "variable not mutable: {}", name),
            LowerError::FunctionNotFound(name) => write!(f, "function not found: {}", name),
            LowerError::InvalidLiteral(lit) => write!(f, "invalid literal: {}", lit),
            LowerError::NotImplemented(what) => write!(f, "not implemented: {}", what),
            LowerError::ParseError => write!(f, "parse error in source"),
            LowerError::BreakOutsideLoop => write!(f, "break outside of loop"),
            LowerError::ContinueOutsideLoop => write!(f, "continue outside of loop"),
        }
    }
}

impl std::error::Error for LowerError {}
