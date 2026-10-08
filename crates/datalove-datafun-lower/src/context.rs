//! Lowering context types.
//!
//! - [`FrameState`]: Per-function IR state (blocks, values, slots). Swapped when
//!   entering nested functions to isolate their IR from the parent.
//! - [`LowerCtx`]: Main context combining shared state (db, types) with `FrameState`.
//! - [`ScriptLowerContext`]: Tracks bindings exported from previous script units.

use std::sync::Arc;
use std::collections::{BTreeMap, HashMap};
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast::{Statement, ExprFun, ExprFunctionCall, ExprKey};
use datalove_datafun_ir::{
    IrType, IrBlock, IrCodeUnit, Operand, ValueId, SlotId, ParamId, BlockId, FuncId,
    CodeRef, CodeUnitId, Terminator, Instruction, SymbolTable, ExportBinding, IrModuleId, ParamMode,
    ConstValue, TypeRef, SlotDest,
};
use crate::ir_ext::IrTypeExt;

/// The data files of a world, by path.
pub type DataFiles = BTreeMap<String, bct::input::Source>;
use datalove_datafun_sema::{BindingId, DropSchedule, BindingInfo, TrackingCategory, StmtKey, AdaptSites, ExprTypes, CallTargets};

/// Compile-time state for building a function's IR.
///
/// Analogous to `Frame` in the interpreter, which holds runtime state.
/// When lowering a nested function, this state is swapped for a fresh
/// instance, then restored after.
pub struct FrameState<'db> {
    /// The type parameters this function declared, in order, so that a name in
    /// a type can be turned into the position a descriptor arrives at.
    pub type_params: Vec<bct::text::InternedText<'db>>,
    /// Shapes this function's own body builds, in the order first seen.
    ///
    /// What the call sites add on top is worked out afterwards, once every
    /// function's own shapes are known; see the shape closure pass.
    pub built_shapes: Vec<datalove_datafun_ir::DescriptorShape>,
    /// The container shape behind a binding whose IR type is the wrapper.
    ///
    /// `from_datalit` collapses a container of a type parameter to `data`
    /// everywhere, so a `[T]` parameter or local says nothing about being a
    /// list. Indexing one has to know which container it is, and a place walk
    /// has only the operand to go on -- there is no expression node for a
    /// place's root. This is where the shape it was bound with is kept.
    pub wrapped_shapes: HashMap<Operand, IrType>,
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
    /// Stack of open name scopes, innermost last.
    ///
    /// A block body -- an `if` branch, a loop body, a match arm -- opens one,
    /// and every binding made while it is open records here what the name
    /// stood for before. Closing the scope puts those back, so a name bound
    /// inside a block stops meaning that once the block ends.
    ///
    /// Without this a binding that shadowed an outer one replaced it for the
    /// rest of the function. The typechecker and the ownership analysis both
    /// scope properly, so such a program compiled, and a use of the outer name
    /// after the block read the inner binding's value -- which on a path where
    /// the block did not run is a value that was never written.
    pub variable_scopes: Vec<Vec<ShadowedBinding>>,
    /// Mapping from BindingId to Operand (built during lowering).
    ///
    /// Ordered, because `compute_tracked_slots` walks it to decide the order
    /// tracking bytes are assigned in, and that reaches the emitted code.
    pub binding_to_operand: BTreeMap<BindingId, Operand>,
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
    /// Uses auto-adapt turned into clones.
    pub adapt_sites: AdaptSites<'db>,
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
    /// Const bindings in this frame: (name, value_id).
    /// Used for const inlining pass.
    pub const_values: Vec<(String, ValueId)>,
}

/// What a name meant before a binding inside a scope took it over.
pub struct ShadowedBinding {
    name: String,
    /// The operand the name stood for, or `None` if it was not bound at all.
    operand: Option<Operand>,
}

impl<'db> FrameState<'db> {
    pub fn new() -> Self {
        Self {
            type_params: Vec::new(),
            built_shapes: Vec::new(),
            wrapped_shapes: HashMap::new(),
            blocks: Vec::new(),
            current_instructions: Vec::new(),
            current_block: BlockId(0),
            current_block_params: Vec::new(),
            next_block: 1, // Block 0 is entry.
            next_value: 0,
            next_slot: 0,
            next_param: 0,
            variables: HashMap::new(),
            variable_scopes: Vec::new(),
            binding_to_operand: BTreeMap::new(),
            operand_to_binding: HashMap::new(),
            next_binding_id: 0,
            param_types: Vec::new(),
            param_modes: Vec::new(),
            value_types: Vec::new(),
            slot_types: Vec::new(),
            tracking: Vec::new(),
            binding_info: Vec::new(),
            drop_schedule: DropSchedule::default(),
            adapt_sites: AdaptSites::default(),
            loop_stack: Vec::new(),
            in_unreachable: false,
            expr_temps: Vec::new(),
            pending_intermediate_scopes: Vec::new(),
            next_stmt_id: 0,
            current_stmt_idx: None,
            const_values: Vec::new(),
        }
    }
}

impl<'db> Default for FrameState<'db> {
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
                ExportBinding::Function(unit_id) => {
                    self.functions.insert(name.clone(), (unit_index, FuncId(unit_id.0)));
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
    pub(super) expr_types: &'db ExprTypes<'db>,
    /// Resolved call targets from typechecker, indexed by ExprFunctionCall salsa ID.
    /// None where there is nothing to resolve against, as when lowering a
    /// const expression in isolation.
    pub(super) call_targets: Option<&'db CallTargets<'db>>,
    /// Map from (salsa ModuleId, func_name) -> (IrModuleId, FuncId).
    /// None where there are no module functions to resolve against.
    pub(super) func_id_map: Option<&'db HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>>,
    /// The data files a `require data` const may name, by path.
    pub(super) data_files: &'db DataFiles,

    /// Function-local state (swapped when entering nested function).
    pub(super) body: FrameState<'db>,

    // Unit-level state (persists across nested functions).
    /// Exports from this unit (only used for script units).
    pub(super) exports: Vec<(String, ExportBinding)>,
    /// Functions defined in this script unit.
    pub(super) functions: Vec<IrCodeUnit>,
    /// Symbol table for function resolution.
    pub(super) symbols: SymbolTable,
    /// Available functions: name -> CodeRef (for resolving calls).
    /// Used for script-local and external unit functions (not module functions).
    pub(super) func_scope: HashMap<String, CodeRef>,
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
    pub(super) const_bindings: HashMap<String, (IrType, Arc<ConstValue>)>,
}

impl<'db> LowerCtx<'db> {
    /// Create a context for lowering module functions with call resolution support.
    pub fn new_for_module(
        db: &'db dyn salsa::Database,
        expr_types: &'db ExprTypes<'db>,
        call_targets: Option<&'db CallTargets<'db>>,
        func_id_map: Option<&'db HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>>,
        data_files: &'db DataFiles,
    ) -> Self {
        Self {
            db,
            expr_types,
            call_targets,
            func_id_map,
            data_files,
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
    /// The shape a type stands for, over this function's type parameters.
    ///
    /// `None` where the type has no parameter in it, which describes itself.
    pub fn shape_of(
        &self,
        ty: &datalove_datalit::tycheck::Type<'db>,
    ) -> Option<datalove_datafun_ir::DescriptorShape> {
        use datalove_datafun_ir::DescriptorShape as S;
        use datalove_datalit::tycheck::Type as DT;
        if !datalove_datafun_common::generics::contains_type_param(ty) {
            return Some(S::Concrete(IrType::from_datalit(self.db, ty)));
        }
        Some(match ty {
            DT::Var(name) => {
                S::Param(self.body.type_params.iter().position(|p| p == name)? as u32)
            }
            DT::List(t) => S::List(Box::new(self.shape_of(&t.element_type)?)),
            DT::Set(t) => S::Set(Box::new(self.shape_of(&t.element_type)?)),
            DT::Option(t) => S::Option(Box::new(self.shape_of(&t.inner_type)?)),
            DT::Result(t) => S::Result(Box::new(self.shape_of(&t.inner_type)?)),
            DT::Map(t) => S::Map(
                Box::new(self.shape_of(&t.key_type)?),
                Box::new(self.shape_of(&t.value_type)?),
            ),
            DT::AnonTuple(t) => S::Tuple(
                t.fields.iter().map(|f| self.shape_of(f)).collect::<Option<Vec<_>>>()?,
            ),
            // Any other shape holding a type parameter is one no collection
            // literal can be written over here.
            _ => return None,
        })
    }

    /// Record that this body builds a collection of `ty`, and say where its
    /// descriptor will arrive.
    pub fn build_shape(
        &mut self,
        ty: &datalove_datalit::tycheck::Type<'db>,
    ) -> Option<u32> {
        let shape = self.shape_of(ty)?;
        if !shape.mentions_param() {
            return None;
        }
        if let Some(i) = self.body.built_shapes.iter().position(|s| *s == shape) {
            return Some(i as u32);
        }
        self.body.built_shapes.push(shape);
        Some((self.body.built_shapes.len() - 1) as u32)
    }

    /// The shape an element of a built collection is carried in.
    ///
    /// A bare type parameter is a `data`, but a tuple of them is a tuple of
    /// `data` and an option of one is an option of `data`: erasure reaches
    /// inside. The descriptor the call site hands over describes the same
    /// shape, so the two have to agree. Assuming `data` here made a
    /// `[(A, B)]` push thirty-two byte elements as sixteen.
    pub fn erased_element(&self, ty: &datalove_datalit::tycheck::Type<'db>) -> IrType {
        IrType::from_datalit(self.db, ty)
    }

    /// The unerased type the typechecker gave an expression.
    ///
    /// `expr_type` erases, which is what the rest of lowering wants. A
    /// collection built over a type parameter is the exception: `[T]` and
    /// `[data]` are the same `IrType`, and only this says which parameter.
    pub fn expr_source_type(
        &self,
        expr: ExprFun<'db>,
    ) -> Option<datalove_datafun_common::Type<'db>> {
        self.expr_types.get(&ExprKey::of(self.db, expr)).cloned()
    }

    pub fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
        let key = ExprKey::of(self.db, expr);
        match self.expr_types.get(&key) {
            Some(ty) => IrType::from_tycheck(self.db, ty),
            None => panic!(
                "Expression must have type from typechecker. {:?} but the table holds {} entries",
                key, self.expr_types.len()
            ),
        }
    }

    /// Create a context for lowering a script unit.
    pub fn new_for_script(
        db: &'db dyn salsa::Database,
        expr_types: &'db ExprTypes<'db>,
        call_targets: Option<&'db CallTargets<'db>>,
        func_id_map: Option<&'db HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>>,
        data_files: &'db DataFiles,
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
            func_scope.insert(name.clone(), CodeRef::External {
                unit: *unit,
                id: CodeUnitId(func_id.0),
            });
        }

        let mut body = FrameState::new();
        body.variables = variables;

        Self {
            db,
            expr_types,
            call_targets,
            func_id_map,
            data_files,
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
    pub fn swap_body_state(&mut self, new_state: FrameState<'db>) -> FrameState<'db> {
        std::mem::replace(&mut self.body, new_state)
    }

    /// Pre-register a function for forward reference support.
    ///
    /// Call this for all functions before lowering any of them to enable
    /// forward references (mutual recursion). The function can then be
    /// looked up via `lookup_func` when lowering calls to it.
    pub fn pre_register_func(&mut self, name: &str, param_count: usize) {
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), CodeRef::Local(CodeUnitId(func_id.0)));
    }

    /// Register a function with a specific FuncId.
    ///
    /// Used for CTFE to register functions with the same IDs as pre-lowered functions,
    /// so that call resolution produces matching CodeRefs.
    pub fn register_func_with_id(&mut self, name: &str, _param_count: usize, func_id: FuncId) {
        self.func_scope.insert(name.to_string(), CodeRef::Local(CodeUnitId(func_id.0)));
    }

    /// Define a function in the current scope.
    ///
    /// If the function was pre-registered via `pre_register_func`, returns the
    /// existing FuncId. Otherwise allocates a new one.
    pub fn define_func(&mut self, name: &str, param_count: usize) -> FuncId {
        // Check if already pre-registered.
        if let Some(CodeRef::Local(id)) = self.func_scope.get(name) {
            return FuncId(id.0);
        }
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), CodeRef::Local(CodeUnitId(func_id.0)));
        func_id
    }

    /// Look up a function by name (for local/external functions only).
    pub fn lookup_func(&self, name: &str) -> Option<CodeRef> {
        self.func_scope.get(name).cloned()
    }

    /// Resolve a function call using typechecker's resolved call target.
    ///
    /// This is the preferred way to resolve function calls. It uses the
    /// `call_targets` from typechecking to get the exact function being called,
    /// then maps it to an IR code reference.
    ///
    /// For module functions (module_id = Some), looks up in `func_id_map`.
    /// For local functions (module_id = None), uses `func_scope` lookup.
    pub fn resolve_call(&self, call: ExprFunctionCall<'db>) -> CodeRef {
        let key = ExprKey::of_call(self.db, call);
        let func_name = call.name(self.db).text(self.db).to_string();

        // Get the resolved call target from typechecking.
        let target = self.call_targets.and_then(|t| t.get(&key))
            .unwrap_or_else(|| panic!(
                "function call not resolved by typechecker: {} ({:?})",
                func_name, key
            ));

        match target.module_id(self.db) {
            Some(module_id) => {
                // Module function - look up by (ModuleId<'db>, func_name).
                let resolved_func_name = target.func(self.db).name(self.db).text(self.db).to_string();
                let (ir_mod, func_id) = self.func_id_map
                    .and_then(|m| m.get(&(module_id, resolved_func_name.clone())))
                    .unwrap_or_else(|| panic!(
                        "module function not in func_id_map: {}",
                        resolved_func_name
                    ));
                CodeRef::Module { module: *ir_mod, id: CodeUnitId(func_id.0) }
            }
            None => {
                // Local function resolved by typechecker - use func_scope lookup.
                let resolved_func_name = target.func(self.db).name(self.db).text(self.db).to_string();
                self.lookup_func(&resolved_func_name)
                    .unwrap_or_else(|| panic!(
                        "local function '{}' not found - typechecker should catch this",
                        resolved_func_name
                    ))
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

    /// Get the type of a value.
    pub fn value_type(&self, id: ValueId) -> Option<&IrType> {
        self.body.value_types.get(id.0 as usize)
    }

    /// Convert an operand to ValueRef if it's a Value with Ref type.
    ///
    /// Used when the operand will be read (dereferenced) rather than passed as a ref.
    pub fn deref_if_ref(&self, operand: Operand) -> Operand {
        if let Operand::Value(vid) = operand {
            if let Some(IrType::Ref(_)) = self.value_type(vid) {
                return Operand::ValueRef(vid);
            }
        }
        operand
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
    /// Remember the container shape a binding's wrapper holds.
    ///
    /// Only for a binding whose IR type is `data` where the source type was a
    /// container; everything else describes itself.
    pub fn record_wrapped_shape(&mut self, operand: Operand, shape: IrType) {
        self.body.wrapped_shapes.insert(operand, shape);
    }

    /// The container shape behind an operand, if it is a wrapper.
    pub fn wrapped_shape(&self, operand: &Operand) -> Option<IrType> {
        self.body.wrapped_shapes.get(operand).cloned()
    }

    pub fn bind_var(&mut self, name: &str, operand: Operand) {
        if let Some(scope) = self.body.variable_scopes.last_mut() {
            scope.push(ShadowedBinding {
                name: name.to_string(),
                operand: self.body.variables.get(name).copied(),
            });
        }
        self.body.variables.insert(name.to_string(), operand);
    }

    /// Open a name scope for a block body.
    ///
    /// Bindings made while it is open last only until [`Self::exit_var_scope`].
    pub fn enter_var_scope(&mut self) {
        self.body.variable_scopes.push(Vec::new());
    }

    /// Close the innermost name scope, putting back what its bindings shadowed.
    ///
    /// Restores in reverse, so that a name bound more than once in the scope
    /// ends up at what it meant before the first of them.
    pub fn exit_var_scope(&mut self) {
        let scope = self.body.variable_scopes.pop()
            .expect("exit_var_scope without a matching enter_var_scope");
        for shadowed in scope.into_iter().rev() {
            match shadowed.operand {
                Some(operand) => {
                    self.body.variables.insert(shadowed.name.clone(), operand);
                }
                None => {
                    self.body.variables.remove(&shadowed.name);
                }
            }
        }
    }

    /// The type an operand currently holds.
    ///
    /// This is the type of the storage the operand names, which is not always
    /// the expression's type: an adapted use reads a `u8` where the expression
    /// says `i16`.
    pub fn operand_type(&self, operand: Operand) -> Option<IrType> {
        match operand {
            Operand::Value(id) | Operand::ValueRef(id) => self.value_type(id).cloned(),
            Operand::Slot(id) => self.slot_type(id).cloned(),
            Operand::Param(id) => self.body.param_types.get(id.0 as usize).cloned(),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => None,
        }
    }

    /// Whether auto-adapt supplies an implicit `@` at this expression.
    ///
    /// Such a use clones instead of moving, which is what leaves the binding
    /// available to the use that would otherwise have been an error.
    pub fn is_adapt_site(&self, expr: ExprFun<'db>) -> bool {
        self.body.adapt_sites.contains(ExprKey::of(self.db, expr))
    }

    /// Look up a variable.
    pub fn lookup_var(&self, name: &str) -> Option<Operand> {
        self.body.variables.get(name).copied()
    }

    /// Look up a const binding by name.
    ///
    /// Returns the type and value if found.
    pub fn lookup_const(&self, name: &str) -> Option<&(IrType, Arc<ConstValue>)> {
        self.const_bindings.get(name)
    }

    /// Add a const binding.
    ///
    /// Const bindings are evaluated at compile time and inlined at use sites.
    pub fn add_const(&mut self, name: String, ir_type: IrType, value: Arc<ConstValue>) {
        self.const_bindings.insert(name, (ir_type, value));
    }

    /// The value of the data file a `require data` const names.
    pub fn data_file_value(&self, file: &datalove_datafun_ast::ast::ExprDataFile<'db>, ty: &IrType) -> Arc<ConstValue> {
        let source = *self.data_files.get(&file.path(self.db))
            .expect("the typechecker found the data file");
        crate::datafile::data_file_value(self.db, source, ty)
    }

    /// Pre-populate const bindings from Phase 2 resolved values.
    ///
    /// When using the 3-phase CTFE pipeline, call this before lowering
    /// to provide pre-evaluated const values. Lowering will then skip
    /// inline evaluation for these consts.
    ///
    /// Takes the ConstBindingGraph (for type info) and ResolvedConsts (for values).
    pub fn add_resolved_consts(
        &mut self,
        graph: &datalove_datafun_ir::ConstBindingGraph,
        resolved: &datalove_datafun_ir::ResolvedConsts,
    ) {
        for binding in &graph.bindings {
            if let Some(value) = resolved.get(binding.stmt_id) {
                self.const_bindings.insert(
                    binding.name.clone(),
                    (binding.ir_type.clone(), value.clone()),
                );
            }
        }
    }

    /// Emit Drop instructions for the given operands.
    pub fn emit_drops(&mut self, operands: Vec<Operand>) {
        for operand in operands {
            self.emit(Instruction::Drop { operand });
        }
    }

    /// Emit a Drop instruction for a single operand if its type requires it.
    pub fn emit_drop_for_type(&mut self, operand: &Operand, ty: &IrType) {
        if !ty.is_copy() {
            self.emit(Instruction::Drop { operand: operand.clone() });
        }
    }

    /// Emit drop for an operand, choosing precise or tracked based on binding.
    ///
    /// Uses Drop for precise bindings (values, temps) and DropTracked for tracked
    /// bindings (slots in control flow, Out params).
    pub fn emit_drop_for_operand(&mut self, operand: &Operand, ty: &IrType) {
        if ty.is_copy() {
            return;
        }
        if self.is_operand_tracked(operand.clone()) {
            self.emit(Instruction::DropTracked { operand: operand.clone() });
        } else {
            self.emit(Instruction::Drop { operand: operand.clone() });
        }
    }

    /// Record an expression temporary that needs dropping after the operation.
    pub fn record_expr_temp(&mut self, value: ValueId, ty: IrType) {
        if !ty.is_copy() {
            self.body.expr_temps.push((value, ty));
        }
    }

    /// Stop treating `value` as a temporary, now that something has taken it.
    pub fn forget_expr_temp(&mut self, value: ValueId) {
        self.body.expr_temps.retain(|(v, _)| *v != value);
    }

    /// Return the current number of expression temporaries.
    ///
    /// Used to snapshot the level before lowering call arguments so that
    /// only temps created by those arguments are dropped afterwards.
    pub fn expr_temps_mark(&self) -> usize {
        self.body.expr_temps.len()
    }

    /// Emit Drop instructions for every expression temporary, on a path that
    /// leaves the function.
    ///
    /// They stay listed. The path that does not leave still holds them, and
    /// each is dropped there by the operation that made it, once that
    /// operation is done with it. An early return from inside an operand --
    /// a `?`, a checked operator, a fallible index -- leaves the enclosing
    /// operations unfinished, so what they had made is let go of here or not
    /// at all.
    fn emit_exit_expr_temp_drops(&mut self) {
        for (value, _ty) in self.body.expr_temps.clone() {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Emit Drop instructions for all expression temporaries and clear the list.
    ///
    /// For the end of a statement. Inside an expression, an operation drops
    /// only the temporaries its own operands made, with
    /// [`emit_expr_temp_drops_since`](Self::emit_expr_temp_drops_since): any
    /// before them belong to an enclosing operation that has not yet run.
    pub fn emit_expr_temp_drops(&mut self) {
        let temps = std::mem::take(&mut self.body.expr_temps);
        for (value, _ty) in temps {
            self.emit(Instruction::Drop { operand: Operand::Value(value) });
        }
    }

    /// Emit Drop instructions for expression temporaries added since `mark`.
    ///
    /// Temps before `mark` are left in place for the enclosing expression.
    pub fn emit_expr_temp_drops_since(&mut self, mark: usize) {
        let tail = self.body.expr_temps.split_off(mark);
        for (value, _ty) in tail {
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
        let category = self.body.tracking.get(id.0 as usize)
            .expect("ownership analysis categorizes every binding");
        *category == TrackingCategory::Tracked
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
        match operand {
            // Values are let bindings and temporaries, which ownership analysis
            // always categorizes as precise.
            Operand::Value(_) | Operand::ValueRef(_) | Operand::ExternalValue { .. } => false,
            // A var from an earlier unit, whose state the frame store tracks.
            Operand::ExternalSlot { .. } => true,
            Operand::Slot(_) | Operand::Param(_) => {
                let id = self.body.operand_to_binding.get(&operand)
                    .expect("slots and params are recorded as bindings when created");
                self.is_binding_tracked(*id)
            }
        }
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
        self.is_operand_tracked(Operand::Slot(slot))
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
    // All values are precise - they're always dropped at known points.
    // Only slots (var bindings) need runtime tracking.

    /// Emit Const.
    pub fn emit_const(&mut self, dest: ValueId, value: ConstValue) {
        self.emit(Instruction::Const { dest, value });
    }

    /// Emit Widen.
    pub fn emit_widen(&mut self, dest: ValueId, src: Operand) {
        self.emit(Instruction::Widen { dest, src });
    }

    /// Emit Call.
    pub fn emit_call(&mut self, dest: ValueId, func: CodeRef, args: Vec<Operand>) {
        self.emit_call_with_type_args(dest, func, args, Vec::new());
    }

    /// Emit Call, handing the callee a descriptor for each shape it declared.
    pub fn emit_call_with_type_args(
        &mut self,
        dest: ValueId,
        func: CodeRef,
        args: Vec<Operand>,
        type_args: Vec<datalove_datafun_ir::DescriptorShape>,
    ) {
        self.emit(Instruction::Call {
            dest, func, args, type_args,
            // Filled in once the shape sets have settled.
            shape_descriptors: Vec::new(),
        });
    }

    /// Emit ComptimeCall (for calls to functions with const parameters).
    pub fn emit_comptime_call(
        &mut self,
        dest: ValueId,
        func: CodeRef,
        args: Vec<Operand>,
        comptime_param_indices: Vec<usize>,
        type_args: Vec<datalove_datafun_ir::DescriptorShape>,
    ) {
        self.emit(Instruction::ComptimeCall {
            dest, func, args, comptime_param_indices, type_args,
            // Filled in once the shape sets have settled, as for `Call`.
            shape_descriptors: Vec::new(),
        });
    }

    /// Emit Pack.
    pub fn emit_pack(&mut self, dest: ValueId, ty: TypeRef, fields: Vec<Operand>) {
        self.emit(Instruction::Pack { dest, ty, fields });
    }

    /// Emit Unpack.
    pub fn emit_unpack(&mut self, dests: Vec<ValueId>, src: Operand) {
        self.emit(Instruction::Unpack { dests, src });
    }

    /// Emit GetField.
    pub fn emit_get_field(&mut self, dest: ValueId, src: Operand, field_index: u32) {
        self.emit(Instruction::GetField { dest, src, field_index });
    }

    /// Emit WrapSome.
    pub fn emit_wrap_some(&mut self, dest: ValueId, inner: Operand) {
        self.emit(Instruction::WrapSome { dest, inner });
    }

    /// Emit WrapNone.
    pub fn emit_wrap_none(&mut self, dest: ValueId) {
        self.emit(Instruction::WrapNone { dest });
    }

    /// Emit WrapOk.
    pub fn emit_wrap_ok(&mut self, dest: ValueId, inner: Operand) {
        self.emit(Instruction::WrapOk { dest, inner });
    }

    /// Emit WrapErr.
    pub fn emit_wrap_err(&mut self, dest: ValueId, inner: Operand) {
        self.emit(Instruction::WrapErr { dest, inner });
    }

    /// Emit EnumVariant.
    pub fn emit_enum_variant(&mut self, dest: ValueId, variant_index: u32, payload: Option<Operand>) {
        self.emit(Instruction::EnumVariant { dest, variant_index, payload });
    }

    /// Emit ErrorFrom.
    pub fn emit_error_from(&mut self, dest: ValueId, inner: Operand) {
        self.emit(Instruction::ErrorFrom { dest, inner });
    }

    /// Emit DataFrom.
    pub fn emit_data_from(&mut self, dest: ValueId, inner: Operand) {
        self.emit(Instruction::DataFrom { dest, inner });
    }

    /// Emit ListNew.
    pub fn emit_list_new(&mut self, dest: ValueId, elements: Vec<Operand>) {
        self.emit(Instruction::ListNew { dest, elements, descriptor: None });
    }

    /// Emit ListNew for a collection whose element type only a declared shape
    /// says, naming which of this function's shapes describes it.
    pub fn emit_list_new_erased(&mut self, dest: ValueId, elements: Vec<Operand>, descriptor: u32) {
        self.emit(Instruction::ListNew { dest, elements, descriptor: Some(descriptor) });
    }

    /// Emit SetNew.
    pub fn emit_set_new(&mut self, dest: ValueId, elements: Vec<Operand>) {
        self.emit(Instruction::SetNew { dest, elements, descriptor: None });
    }

    /// Emit SetNew for a collection whose element type only a declared shape
    /// says, naming which of this function's shapes describes it.
    pub fn emit_set_new_erased(&mut self, dest: ValueId, elements: Vec<Operand>, descriptor: u32) {
        self.emit(Instruction::SetNew { dest, elements, descriptor: Some(descriptor) });
    }

    /// Emit MapNew.
    pub fn emit_map_new(&mut self, dest: ValueId, entries: Vec<(Operand, Operand)>) {
        self.emit(Instruction::MapNew { dest, entries, descriptor: None });
    }

    /// Emit MapNew for a collection whose element type only a declared shape
    /// says, naming which of this function's shapes describes it.
    pub fn emit_map_new_erased(&mut self, dest: ValueId, entries: Vec<(Operand, Operand)>, descriptor: u32) {
        self.emit(Instruction::MapNew { dest, entries, descriptor: Some(descriptor) });
    }

    /// Emit TensorNew.
    pub fn emit_tensor_new(&mut self, dest: ValueId, shape: Vec<u32>, elements: Vec<Operand>) {
        self.emit(Instruction::TensorNew { dest, shape, elements });
    }

    /// Emit TableNew.
    pub fn emit_table_new(&mut self, dest: ValueId, rows: Vec<Operand>) {
        self.emit(Instruction::TableNew { dest, rows });
    }

    /// Emit Intrinsic.
    pub fn emit_intrinsic(&mut self, dest: ValueId, intrinsic: datalove_datafun_intrinsics::IntrinsicId, args: Vec<Operand>) {
        self.emit(Instruction::Intrinsic { dest, intrinsic, args });
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

    /// Emit ParamStore to write a value to a mutable parameter.
    ///
    /// - Mut params: precise, always destroys old value (ParamStore)
    /// - Out params: tracked, checks init state before destroying (ParamStoreTracked)
    pub fn emit_param_store(&mut self, param: ParamId, value: Operand) {
        if self.param_mode(param) == Some(ParamMode::Out) {
            self.emit(Instruction::ParamStoreTracked { param, value });
        } else {
            self.emit(Instruction::ParamStore { param, value });
        }
    }

    /// Emit ParamSetField to write a value to a field within a mutable parameter.
    ///
    /// - Mut params: precise, always destroys old field value (ParamSetField)
    /// - Out params: tracked, checks init state before destroying (ParamSetFieldTracked)
    pub fn emit_param_set_field(&mut self, param: ParamId, field_path: Vec<u32>, value: Operand) {
        if self.param_mode(param) == Some(ParamMode::Out) {
            self.emit(Instruction::ParamSetFieldTracked { param, field_path, value });
        } else {
            self.emit(Instruction::ParamSetField { param, field_path, value });
        }
    }

    /// Emit SlotLoadCopy.
    pub fn emit_slot_load_copy(&mut self, dest: ValueId, slot: SlotId) {
        self.emit(Instruction::SlotLoadCopy { dest, slot });
    }

    // Note: compute_tracked_values was removed because all values are now precise.
    // Values don't need runtime tracking - they're always dropped at known points.

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

    /// Compute tracked_params for the current function.
    ///
    /// Returns ParamIds for Out params that need runtime tracking.
    pub fn compute_tracked_params(&self) -> Vec<ParamId> {
        self.body.param_modes.iter().enumerate()
            .filter(|(_, mode)| **mode == ParamMode::Out)
            .map(|(i, _)| ParamId(i as u32))
            .collect()
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
    /// In debug builds, verifies that the statement matches what ownership
    /// analysis recorded at this index.
    pub fn alloc_stmt_id(&mut self, stmt: &Statement<'_>) -> usize {
        let id = self.body.next_stmt_id;
        #[cfg(debug_assertions)]
        {
            let actual = StmtKey::from_stmt(self.db, stmt);
            if let Some(expected) = self.body.drop_schedule.stmt_order.get(id) {
                assert_eq!(
                    *expected, actual,
                    "stmt_id mismatch at index {}: ownership analysis saw {} (local_index={:?}), \
                     lowering saw {} (local_index={:?})",
                    id,
                    expected.kind_name(),
                    expected.local_index,
                    actual.kind_name(),
                    actual.local_index,
                );
            }
        }
        self.body.next_stmt_id += 1;
        id
    }

    /// Emit drops scheduled for a then-branch exit.
    pub fn emit_then_branch_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.then_branch_exit
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled for an else-branch exit.
    pub fn emit_else_branch_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.else_branch_exit
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a return statement.
    pub fn emit_before_return_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.before_return
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops for what is still owned where the body ends.
    ///
    /// The counterpart of `emit_before_return_drops` for a function that runs
    /// off the end rather than returning. Empty when every path returned.
    pub fn emit_function_exit_drops(&mut self) {
        let binding_ids = self.body.drop_schedule.at_function_exit.clone();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a TryReturn (checked/optional operators).
    ///
    /// Uses current_stmt_idx since TryReturn happens within expression lowering.
    pub fn emit_before_try_return_drops(&mut self) {
        if let Some(stmt_idx) = self.body.current_stmt_idx {
            let binding_ids = self.body.drop_schedule.before_try_return
                .get(&stmt_idx).cloned().unwrap_or_default();
            for id in binding_ids {
                self.emit_binding_drop(id);
            }
        }
    }

    /// Emit drops scheduled before a set-target early return (index OOB).
    ///
    /// The value and the keys have been evaluated by then, so what they moved
    /// is not among these, and a key a bare index consumes is a temporary.
    pub fn emit_before_set_target_early_return_drops(&mut self) {
        if let Some(stmt_idx) = self.body.current_stmt_idx {
            let binding_ids = self.body.drop_schedule.before_set_target_early_return
                .get(&stmt_idx)
                .cloned()
                .unwrap_or_default();
            for id in binding_ids {
                self.emit_binding_drop(id);
            }
        }
    }

    /// Emit a None early-return block body.
    ///
    /// Caller must have already called `start_block` on the early-return block.
    /// `set_target_drops` says the return is a `set` failing to reach its
    /// place, which drops by that schedule rather than an expression's.
    pub fn emit_early_return_none(&mut self, set_target_drops: bool) {
        let return_type = self.return_type.clone()
            .expect("early return requires function context");
        let none_value = self.fresh_value(return_type);
        self.emit_wrap_none(none_value);
        self.emit_exit_expr_temp_drops();
        self.emit_pending_intermediate_drops();
        if set_target_drops {
            self.emit_before_set_target_early_return_drops();
        } else {
            self.emit_before_try_return_drops();
        }
        let terminator = if self.is_script_unit {
            Terminator::UnitEarlyReturn { value: Operand::Value(none_value) }
        } else {
            Terminator::Return { value: Some(Operand::Value(none_value)) }
        };
        self.finish_block(terminator);
    }

    /// Emit an Err early-return block body with a string error message.
    ///
    /// Caller must have already called `start_block` on the early-return block.
    pub fn emit_early_return_err_message(&mut self, message: &str, set_target_drops: bool) {
        let err_msg = self.fresh_value(IrType::String);
        self.emit_const(err_msg, ConstValue::String(message.to_string()));
        let err_value = self.fresh_value(IrType::Error);
        self.emit_error_from(err_value, Operand::Value(err_msg));
        self.emit_early_return_err(Operand::Value(err_value), set_target_drops);
    }

    /// Emit an Err early-return block body with an existing error operand.
    ///
    /// Caller must have already called `start_block` on the early-return block.
    pub fn emit_early_return_err(&mut self, err_operand: Operand, set_target_drops: bool) {
        let return_type = self.return_type.clone()
            .expect("early return requires function context");
        let wrapped = self.fresh_value(return_type);
        self.emit_wrap_err(wrapped, err_operand);
        self.emit_exit_expr_temp_drops();
        self.emit_pending_intermediate_drops();
        if set_target_drops {
            self.emit_before_set_target_early_return_drops();
        } else {
            self.emit_before_try_return_drops();
        }
        let terminator = if self.is_script_unit {
            Terminator::UnitEarlyReturn { value: Operand::Value(wrapped) }
        } else {
            Terminator::Return { value: Some(Operand::Value(wrapped)) }
        };
        self.finish_block(terminator);
    }

    /// Emit drops scheduled for loop body end.
    pub fn emit_loop_body_end_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.loop_body_end
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a break statement.
    pub fn emit_before_break_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.before_break
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled before a continue statement.
    pub fn emit_before_continue_drops(&mut self, stmt_idx: usize) {
        let binding_ids = self.body.drop_schedule.before_continue
            .get(&stmt_idx).cloned().unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
    }

    /// Emit drops scheduled for a match arm exit.
    pub fn emit_match_arm_drops(&mut self, stmt_idx: usize, arm_idx: usize) {
        let binding_ids = self.body.drop_schedule.match_arm_exit
            .get(&(stmt_idx, arm_idx))
            .cloned()
            .unwrap_or_default();
        for id in binding_ids {
            self.emit_binding_drop(id);
        }
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
                Terminator::Switch { cases, default, .. } => {
                    for (_, block) in cases.iter_mut() {
                        *block = BlockId(id_map[block.0 as usize]);
                    }
                    *default = BlockId(id_map[default.0 as usize]);
                }
                Terminator::Return { .. }
                | Terminator::UnitEnd { .. }
                | Terminator::UnitEarlyReturn { .. } => {}
            }
        }
    }
}
