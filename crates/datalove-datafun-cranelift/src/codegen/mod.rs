//! Core codegen driver for translating IR to Cranelift.
//!
//! Translates [`IrCodeUnit`] to Cranelift IR using FunctionBuilder. The main type is
//! [`FunctionCompiler`], which handles the translation of a single function.
//!
//! # Submodules
//!
//! Instruction compilation is split across submodules by category:
//! - [`ops`]: Binary and unary arithmetic/logic operations.
//! - [`constants`]: Constant value materialization.
//! - [`collections`]: List, Set, Map construction.
//! - [`aggregates`]: Tuple/struct Pack and Unpack.
//! - [`calls`]: Function call compilation.
//! - [`slots`]: Mutable slot load/store.
//! - [`runtime`]: Runtime calls (DebugLog, Drop).
//! - [`terminators`]: Block terminators (Return, Branch, Goto).
//!
//! # Value representation
//!
//! IR values are represented in Cranelift as either:
//! - **Scalar**: Fits in a register (bools, integers, floats).
//! - **Aggregate**: Stored in the stack frame, tracked by pointer.
//!
//! All function parameters are passed by pointer. The implicit `rt_handle`
//! is threaded as the first parameter to all functions.
//!
//! # Block parameters (loop carries/brings)
//!
//! Block parameters implement loop carry/bring values. The representation differs
//! by value type:
//!
//! - **Scalars**: Cranelift block param IS the value. Pure SSA semantics - the
//!   value flows directly through Goto/Branch instructions.
//!
//! - **Aggregates**: Cranelift block param is a pointer to the source data.
//!   On block entry, we memcpy to the value's fixed frame location. This is
//!   necessary to prevent aliasing when the same frame location is both source
//!   and destination (common in loop carry scenarios).

/// Tuple/struct packing and unpacking.
mod aggregates;
/// Function call compilation.
mod calls;
/// Collection type construction.
mod collections;
/// Constant value materialization.
mod constants;
/// Intrinsic function codegen.
mod intrinsics;
/// List indexing operations.
mod lists;
/// Map indexing operations.
mod maps;
/// Tensor indexing operations.
mod tensors;
/// Binary and unary operations.
mod ops;
/// Option and Result operations.
mod options;
/// Boxing operations (ErrorFrom, DataFrom).
mod boxing;
/// Runtime calls (DebugLog, Drop).
mod runtime;
/// Mutable slot operations.
mod slots;
/// Block terminators.
mod terminators;

use std::collections::HashMap;

use cranelift_codegen::ir::{
    self as cl_ir,
    types as cl_types,
    InstBuilder,
    MemFlagsData,
};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};

use datalove_datafun_ir::{
    BlockId, ConstValue, FunctionContext, FunctionRegistry, IrBlock, IrCodeUnit, ParamMode,
    IrModuleId, IrType, Instruction, NativeContext, Operand, ParamId, SlotDest, SlotId,
    Terminator, ValueId,
};

use datalove_datafun_ir::frame_layout::FrameLayout;
use crate::runtime::RuntimeImports;
use crate::tydesc_emit::TyDescEmitter;
use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

/// Where a const a `StaticRef` names lives.
///
/// Under the JIT the interpreter has already built it, and this is its
/// address. In an object file it is a data object the program fills in before
/// the script runs; see [`FunctionCompiler::compile_static_init`].
#[derive(Clone, Copy, Debug)]
pub enum StaticConstLoc {
    Address(usize),
    Data(cranelift_module::DataId),
}

/// Where each static const lives, keyed by the address of the `Arc` a
/// `StaticRef` holds its value in.
pub type StaticConsts = HashMap<usize, StaticConstLoc>;

/// The key a `StaticRef`'s value is found under in [`StaticConsts`].
pub fn static_const_key(value: &std::sync::Arc<ConstValue>) -> usize {
    std::sync::Arc::as_ptr(value) as usize
}

/// A function unit with no parameters and no body, returning nothing.
///
/// What [`FunctionCompiler::compile_static_init`] and
/// [`FunctionCompiler::compile_static_fini`] are compiled over: they need a
/// compiler's runtime and descriptors, and no IR of their own.
pub fn empty_function_unit(name: &str) -> IrCodeUnit {
    IrCodeUnit {
        id: datalove_datafun_ir::CodeUnitId(0),
        name: name.to_string(),
        blocks: Vec::new(),
        value_count: 0,
        slot_count: 0,
        value_types: Vec::new(),
        slot_types: Vec::new(),
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: Default::default(),
        context: datalove_datafun_ir::CodeUnitContext::Function(
            datalove_datafun_ir::FunctionContext {
                descriptor_params: Vec::new(),
                params: Vec::new(),
                param_modes: Vec::new(),
                param_types: Vec::new(),
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                descriptor_shapes: Vec::new(),
            },
        ),
        nested_units: Vec::new(),
    }
}

/// Build a Cranelift function signature for an IR code unit.
///
/// All functions have an implicit rt_handle as first parameter.
/// For aggregate returns, an sret (structure return) pointer is the second parameter.
/// User-visible parameters follow, all passed by pointer.
///
/// Panics if the code unit is not a function.
pub fn build_signature_for_func(
    func: &IrCodeUnit,
    isa: &dyn TargetIsa,
) -> cl_ir::Signature {
    let func_ctx = func.function_context()
        .expect("build_signature_for_func requires a function code unit");

    let call_conv = isa.default_call_conv();
    let mut sig = cl_ir::Signature::new(call_conv);

    // Implicit rt_handle as first param (pointer to runtime).
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));

    // For non-Unit returns, add sret pointer as second param.
    // Caller allocates space and passes pointer; callee writes result there.
    // All returns use sret to avoid ABI complexity around register types.
    let ret_ty = &func_ctx.return_type;
    let has_sret = uses_sret(ret_ty);
    if has_sret {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // User parameters are passed by pointer.
    for _ in &func_ctx.param_types {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // Then a descriptor for each parameter whose type does not describe what
    // arrives, which the call site knows and this function does not.
    for _ in &func_ctx.descriptor_params {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // Then one for each shape this function builds a collection of. Those have
    // no value to carry a descriptor with, so they come on their own, after the
    // ones that describe a parameter.
    for _ in &func_ctx.descriptor_shapes {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // No register return values. All returns use sret.

    sig
}

/// Build a Cranelift signature for a native rider function.
///
/// Matches the runtime C ABI: each parameter is a `(ptr, tydesc)` pair,
/// the return value is passed via an out-param `(ptr, tydesc)` pair,
/// and the function returns `RtStatus` (i8).
///
/// `fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> i8`
pub fn build_native_signature(
    ctx: &NativeContext,
    isa: &dyn TargetIsa,
) -> cl_ir::Signature {
    let call_conv = isa.default_call_conv();
    let mut sig = cl_ir::Signature::new(call_conv);

    // Runtime handle as first parameter.
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));

    // Each arg is a (ptr, tydesc) pair.
    for _ in &ctx.param_types {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE)); // value ptr
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE)); // tydesc ptr
    }

    // Return value as out-param (ptr, tydesc) pair.
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE)); // result out ptr
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE)); // result tydesc ptr

    // Then a descriptor for each type parameter no argument determines. See
    // `NativeContext::descriptor_shapes`; almost every native has none.
    for _ in &ctx.descriptor_shapes {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // RtStatus return (i8).
    sig.returns.push(cl_ir::AbiParam::new(cl_ir::types::I8));

    sig
}

/// Check if a return type uses sret (structure return) convention.
///
/// All non-Unit returns use sret. The caller allocates space and passes a
/// pointer; the callee writes the result there. This avoids ABI complexity
/// around integer vs float register returns.
pub fn uses_sret(ret_ty: &IrType) -> bool {
    !matches!(ret_ty, IrType::Unit)
}

/// A function compiled beside this one, and what its signature asks for.
///
/// The two travel together because a call has to supply exactly what the
/// callee's signature declares. Recording only the `FuncId` is what let a
/// script-local generic be declared with descriptor parameters and called
/// without them, which cranelift reports as a verifier error at best and
/// which loses the descriptor silently at worst.
#[derive(Clone)]
pub struct LocalCallee {
    pub func_id: FuncId,
    /// Parameters whose descriptor the call site supplies, in signature order.
    pub descriptor_params: Vec<ParamId>,
    /// Shapes whose descriptor the call site supplies, after those.
    pub descriptor_shapes: Vec<datalove_datafun_ir::DescriptorShape>,
    /// How the callee takes each parameter, which the call site needs because
    /// an `out` one has its old value dropped here rather than there.
    pub param_modes: Vec<ParamMode>,
}

impl LocalCallee {
    /// What a call to `unit` has to pass, given the id it was declared under.
    pub fn of(unit: &IrCodeUnit, func_id: FuncId) -> Self {
        let ctx = unit.function_context();
        LocalCallee {
            func_id,
            descriptor_params: ctx.map(|c| c.descriptor_params.clone()).unwrap_or_default(),
            descriptor_shapes: ctx.map(|c| c.descriptor_shapes.clone()).unwrap_or_default(),
            param_modes: ctx.map(|c| c.param_modes.clone()).unwrap_or_default(),
        }
    }
}

/// Compiles a single IR function to Cranelift IR.
pub struct FunctionCompiler<'a, M: Module> {
    /// The IR code unit being compiled.
    func: &'a IrCodeUnit,
    /// Function-specific context (always present for compiled code units).
    func_ctx: &'a FunctionContext,
    /// Frame layout for values and slots.
    layout: FrameLayout,
    /// Target ISA for pointer size etc.
    isa: &'a dyn TargetIsa,
    /// Module for declaring functions.
    module: &'a mut M,
    /// Mapping from IR ValueId to Cranelift Value.
    values: HashMap<ValueId, cl_ir::Value>,
    /// Mapping from IR BlockId to Cranelift Block.
    blocks: HashMap<BlockId, cl_ir::Block>,
    /// Mapping from IR ParamId to Cranelift Value (user params, not rt_handle).
    param_values: HashMap<ParamId, cl_ir::Value>,
    /// Descriptors the caller supplied, for parameters whose own type does not
    /// describe what arrives. See `FunctionContext::descriptor_params`.
    descriptor_values: HashMap<ParamId, cl_ir::Value>,
    /// Descriptors handed over for this function's declared shapes, in order.
    shape_descriptor_values: Vec<cl_ir::Value>,
    /// What each reference points at, where its static type does not say.
    ///
    /// Derived from the unit rather than stored on it, so that specialization
    /// and inlining renumbering values cannot leave it stale. See
    /// `datalove_datafun_ir::RefDesc`.
    ref_descs: std::collections::BTreeMap<ValueId, datalove_datafun_ir::RefDesc>,
    /// The descriptor materialized for each such reference, once the projection
    /// that made it has run.
    ref_desc_values: HashMap<ValueId, cl_ir::Value>,
    /// Functions compiled beside this one, by their IR id.
    local_funcs: HashMap<datalove_datafun_ir::CodeUnitId, LocalCallee>,
    /// Functions in a script unit that finished before this one.
    ///
    /// Keyed by the unit as well as the id, because a `CodeUnitId` numbers a
    /// unit's own functions from zero: unit 1's function 0 and unit 2's function
    /// 0 are two functions with one id, and a body calling both would otherwise
    /// put two callees under one key and reach whichever was declared second.
    external_funcs: HashMap<(u32, datalove_datafun_ir::CodeUnitId), LocalCallee>,
    /// Mapping from module function (IrModuleId, FuncId) to Cranelift FuncId.
    module_funcs: HashMap<(IrModuleId, datalove_datafun_ir::CodeUnitId), FuncId>,
    /// Function registry for looking up module functions.
    #[allow(dead_code)]
    registry: Option<&'a FunctionRegistry>,
    /// Cranelift variables for mutable slots (SlotId).
    #[allow(dead_code)]
    slot_vars: HashMap<SlotId, Variable>,
    /// Where the frame is: values, slots and tracking bytes, as `layout` says.
    frame_slot: Option<FrameBase>,
    /// Next variable index for Cranelift.
    #[allow(dead_code)]
    next_var: u32,
    /// Runtime function imports (optional, for functions that need runtime calls).
    runtime: Option<RuntimeImports>,
    /// The module's type descriptors, shared by every function compiled into
    /// it. A descriptor is emitted the first time codegen asks for it.
    tydesc_emitter: &'a mut TyDescEmitter,
    /// Runtime handle (implicit first parameter to all functions).
    rt_handle_param: Option<cl_ir::Value>,
    /// Sret pointer (implicit second parameter for aggregate returns).
    sret_param: Option<cl_ir::Value>,
    /// Values built straight into the caller's result slot rather than in
    /// this frame; see `return_slot_values`.
    return_slot: std::collections::HashSet<ValueId>,
    /// Where the consts `StaticRef` names live.
    static_consts: StaticConsts,
    /// The stack memory constants are built in; see `constants::ConstScratch`.
    const_scratch: constants::ConstScratchArea,
    /// Where to enter, if this is an OSR entry rather than the function.
    osr_spec: Option<OsrSpec>,
    /// What compiling the OSR entry keeps, while it compiles.
    osr: Option<OsrState>,
}

/// The values a function builds straight into its caller's result slot: each
/// an option or result its block wraps and then returns, and uses for nothing
/// else.
///
/// Built in this frame instead, such a value is written field by field and then
/// copied out by the return, and the copy's wide loads read back the narrow
/// stores that just wrote it, which the processor cannot forward from: on
/// recursive `fib`, which returns `!u32`, most of a call's time went there.
fn return_slot_values(func: &IrCodeUnit) -> std::collections::HashSet<ValueId> {
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    let mut count = |op: &Operand| {
        if let Operand::Value(v) | Operand::ValueRef(v) = op {
            *uses.entry(*v).or_default() += 1;
        }
    };
    for block in &func.blocks {
        for instr in &block.instructions {
            instr.for_each_operand(&mut count);
        }
        block.terminator.for_each_operand(&mut count);
    }
    func.blocks.iter()
        .filter_map(|block| {
            let Terminator::Return { value: Some(Operand::Value(v)) } = &block.terminator else { return None };
            let wrapped_here = block.instructions.iter().any(|instr| matches!(instr,
                Instruction::WrapOk { dest, .. } | Instruction::WrapErr { dest, .. }
                | Instruction::WrapSome { dest, .. } | Instruction::WrapNone { dest } if dest == v));
            (wrapped_here && uses.get(v) == Some(&1)).then_some(*v)
        })
        .collect()
}

/// The blocks reachable from `from`, itself included.
fn reachable_from(func: &IrCodeUnit, from: BlockId) -> std::collections::HashSet<BlockId> {
    let mut seen = std::collections::HashSet::new();
    let mut work = vec![from];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let block = func.blocks.iter().find(|b| b.id == id).expect("a block of the function");
        work.extend(block.terminator.successors());
    }
    seen
}

/// Where a function's frame is.
///
/// Its own stack slot, or, for code entered partway through from the
/// interpreter (on-stack replacement), the interpreter's frame for the call,
/// which has the same layout and goes on being used in place.
#[derive(Clone, Copy)]
pub(crate) enum FrameBase {
    Slot(cl_ir::StackSlot),
    Ptr(cl_ir::Value),
}

/// An entry into a function at one of its loop headers rather than at its
/// start, from the interpreter's frame for a call already running it
/// (on-stack replacement).
///
/// The code takes the runtime handle, the `sret` pointer if the function has
/// one, and the frame, and carries on in the frame in place: values, slots
/// and tracking bytes are where `FrameLayout` puts them in both. The rest is
/// where the interpreter keeps it, which this says.
pub struct OsrSpec {
    /// The block entered, a loop header.
    pub header: BlockId,
    /// Where each parameter's `Value` is: its pointer, then its descriptor.
    pub param_offsets: Vec<u32>,
    /// Where the shape descriptors are, one word each.
    pub shape_offset: u32,
}

/// What compiling an OSR entry keeps; see `OsrSpec`.
struct OsrState {
    /// The blocks reachable from the header, which are all that is compiled.
    reachable: std::collections::HashSet<BlockId>,
    /// The interpreter's frame.
    frame: cl_ir::Value,
    /// The entry block's jump to the header, before which a value defined
    /// before the loop is read in, the first time something asks for it.
    entry_jump: cl_ir::Inst,
    /// Those values, read in.
    materialized: std::cell::RefCell<HashMap<ValueId, cl_ir::Value>>,
    /// The constants, a scalar one of which is made again rather than read:
    /// the bytecode folds a constant into what reads it and may never write
    /// it to the frame.
    consts: HashMap<ValueId, ConstValue>,
    /// What compiling each block defined; see `osr_check`.
    defined: HashMap<BlockId, std::collections::HashSet<ValueId>>,
}

impl FrameBase {
    /// The address `offset` bytes into the frame.
    pub(crate) fn addr(self, builder: &mut FunctionBuilder, offset: i32) -> cl_ir::Value {
        match self {
            FrameBase::Slot(slot) => builder.ins().stack_addr(PTR_TYPE, slot, offset),
            FrameBase::Ptr(base) => builder.ins().iadd_imm_s(base, offset as i64),
        }
    }
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Where an option or result is built: in the caller's result slot if it
    /// is one `return_slot_values` found, otherwise in this frame.
    fn wrap_dest_addr(
        &self,
        builder: &mut FunctionBuilder,
        frame_slot: FrameBase,
        dest: ValueId,
        dest_offset: u32,
    ) -> cl_ir::Value {
        match self.sret_param {
            Some(sret) if self.return_slot.contains(&dest) => sret,
            _ => frame_slot.addr(builder, dest_offset as i32),
        }
    }

    /// Create a new function compiler.
    ///
    /// Panics if the code unit is not a function.
    pub fn new(
        func: &'a IrCodeUnit,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        tydesc_emitter: &'a mut TyDescEmitter,
    ) -> Self {
        let func_ctx = func.function_context()
            .expect("FunctionCompiler requires a function code unit");

        let layout = FrameLayout::compute(
            &func_ctx.param_types,
            &func.value_types,
            &func.slot_types,
            &func.tracked_slots,
            &func_ctx.tracked_params,
        );

        Self {
            func,
            func_ctx,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            descriptor_values: HashMap::new(),
            shape_descriptor_values: Vec::new(),
            ref_descs: datalove_datafun_ir::resolve_ref_descriptors(func),
            ref_desc_values: HashMap::new(),
            local_funcs: HashMap::new(),
            external_funcs: HashMap::new(),
            module_funcs: HashMap::new(),
            registry: None,
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: None,
            tydesc_emitter,
            rt_handle_param: None,
            sret_param: None,
            return_slot: return_slot_values(func),
            static_consts: StaticConsts::new(),
            const_scratch: constants::ConstScratchArea::default(),
            osr_spec: None,
            osr: None,
        }
    }

    /// Create a new function compiler with runtime imports.
    ///
    /// Panics if the code unit is not a function.
    pub fn new_with_runtime(
        func: &'a IrCodeUnit,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        runtime: RuntimeImports,
        tydesc_emitter: &'a mut TyDescEmitter,
        registry: Option<&'a FunctionRegistry>,
    ) -> Self {
        let func_ctx = func.function_context()
            .expect("FunctionCompiler requires a function code unit");

        let layout = FrameLayout::compute(
            &func_ctx.param_types,
            &func.value_types,
            &func.slot_types,
            &func.tracked_slots,
            &func_ctx.tracked_params,
        );

        Self {
            func,
            func_ctx,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            descriptor_values: HashMap::new(),
            shape_descriptor_values: Vec::new(),
            ref_descs: datalove_datafun_ir::resolve_ref_descriptors(func),
            ref_desc_values: HashMap::new(),
            local_funcs: HashMap::new(),
            external_funcs: HashMap::new(),
            module_funcs: HashMap::new(),
            registry,
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: Some(runtime),
            tydesc_emitter,
            rt_handle_param: None,
            sret_param: None,
            return_slot: return_slot_values(func),
            static_consts: StaticConsts::new(),
            const_scratch: constants::ConstScratchArea::default(),
            osr_spec: None,
            osr: None,
        }
    }

    /// The descriptor for a type, emitted the first time anything asks.
    ///
    /// On demand rather than collected beforehand: a list made up front had to
    /// foresee every type codegen would reach for, and each one it missed --
    /// a payload inside a constant, the row of an empty table -- was a
    /// function that would not compile.
    pub(crate) fn tydesc(&mut self, ty: &IrType) -> Result<cranelift_module::DataId, CraneliftError> {
        self.tydesc_emitter.emit(self.module, ty)
    }

    /// Compile the function and return the Cranelift FuncId.
    ///
    /// This declares the function under its own name with Export linkage and
    /// then defines it. Use `compile_predeclared` for functions that have
    /// already been declared, and `compile_as` where the name has to be one the
    /// caller chooses.
    pub fn compile(self) -> Result<FuncId, CraneliftError> {
        let symbol = self.func.name.clone();
        self.compile_as(&symbol)
    }

    /// Compile the function under a symbol of the caller's choosing.
    ///
    /// A module's function is named by its own name alone, and two modules can
    /// share one: `list.get` and `map.get` both compile to `get`. Declaring
    /// both in one module is a collision, and it goes one of two ways. Where
    /// the arities differ -- they do here, because `map.get` takes descriptors
    /// for two type parameters and `set.get` for one -- the second declaration
    /// is refused outright. Where they agree it is accepted, and every call
    /// meant for one of them reaches the other.
    ///
    /// So anything compiling functions from more than one module into a single
    /// module has to name them apart itself.
    pub fn compile_as(mut self, symbol: &str) -> Result<FuncId, CraneliftError> {
        // Build function signature.
        let sig = self.build_signature();

        // Declare function in module.
        let func_id = self.module
            .declare_function(symbol, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::Module(format!("declare function: {}", e)))?;

        self.compile_body(func_id, sig)
    }

    /// Compile an entry into the function at a loop header, from an
    /// interpreter frame; see `OsrSpec`.
    ///
    /// Only what can be reached from the header is compiled. Refused, as
    /// `CraneliftError::Unsupported`, where a value the loop needs is made
    /// inside the code reached -- by an enclosing loop's body, for an inner
    /// loop's header -- since there would be two places it comes from and
    /// nothing to choose between them; and where one is a reference whose
    /// descriptor only its making works out.
    pub fn compile_osr_as(mut self, symbol: &str, spec: OsrSpec) -> Result<FuncId, CraneliftError> {
        let mut sig = cl_ir::Signature::new(self.isa.default_call_conv());
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        if uses_sret(&self.func_ctx.return_type) {
            sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        }
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        let func_id = self.module
            .declare_function(symbol, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::Module(format!("declare function: {}", e)))?;
        self.osr_spec = Some(spec);
        self.compile_body(func_id, sig)
    }

    /// Compile a function that has already been declared.
    ///
    /// Use this for two-pass compilation where functions are declared first.
    pub fn compile_predeclared(mut self, func_id: FuncId) -> Result<FuncId, CraneliftError> {
        let sig = self.build_signature();
        self.compile_body(func_id, sig)
    }

    /// Compile the function body using the given FuncId and signature.
    fn compile_body(&mut self, func_id: FuncId, sig: cl_ir::Signature) -> Result<FuncId, CraneliftError> {
        // Create Cranelift function.
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // Create function builder context.
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        // The blocks to compile: every one, or for an OSR entry, those the
        // header reaches.
        let reachable = self.osr_spec.as_ref().map(|spec| reachable_from(self.func, spec.header));
        let compiled = |id: BlockId| reachable.as_ref().is_none_or(|r| r.contains(&id));

        // Create frame stack slot if needed.
        if self.layout.frame_size > 0 && self.osr_spec.is_none() {
            let slot_data = cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                self.layout.frame_size,
                types::align_shift(self.layout.frame_align),
            );
            self.frame_slot = Some(FrameBase::Slot(builder.create_sized_stack_slot(slot_data)));
        }

        // Create blocks with their parameters.
        for block in &self.func.blocks {
            if !compiled(block.id) {
                continue;
            }
            let cl_block = builder.create_block();

            // Add block params for IR block params.
            for &param_value_id in &block.params {
                let param_ty = self.func.value_types.get(param_value_id.0 as usize)
                    .cloned()
                    .unwrap_or(IrType::Unit);
                let cl_ty = match types::ir_type_to_cranelift(&param_ty) {
                    CraneliftRepr::Scalar(t) => t,
                    CraneliftRepr::Aggregate(_) => PTR_TYPE,
                };
                builder.append_block_param(cl_block, cl_ty);
            }

            self.blocks.insert(block.id, cl_block);
        }

        if self.osr_spec.is_some() {
            self.enter_osr(&mut builder, reachable.clone().expect("an OSR entry has its reachable blocks"));
        } else {
            // Set up entry block with parameters.
            let entry_block = self.blocks[&BlockId(0)];
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            // Don't seal yet - wait until all blocks are compiled for loop back-edges.

            // Extract block parameters.
            // Layout: [rt_handle, sret? (if aggregate return), user_param_0, user_param_1, ...]
            let param_values: Vec<_> = builder.block_params(entry_block).to_vec();

            // First param is always rt_handle (implicit).
            self.rt_handle_param = Some(param_values[0]);

            // Check if this function uses sret.
            let has_sret = uses_sret(&self.func_ctx.return_type);
            let user_param_start = if has_sret {
                // Second param is sret pointer.
                self.sret_param = Some(param_values[1]);
                2
            } else {
                1
            };

            // User params start after implicit params, and the descriptors the
            // caller supplied follow them.
            let user_param_count = self.func_ctx.param_types.len();
            for (i, &val) in param_values[user_param_start..].iter().take(user_param_count).enumerate() {
                let param_id = ParamId(i as u32);
                // Track param values for get_operand_value.
                self.param_values.insert(param_id, val);
            }
            let descriptor_start = user_param_start + user_param_count;
            for (&param_id, &val) in self.func_ctx.descriptor_params.iter()
                .zip(param_values[descriptor_start..].iter())
            {
                self.descriptor_values.insert(param_id, val);
            }
            let shape_start = descriptor_start + self.func_ctx.descriptor_params.len();
            self.shape_descriptor_values = param_values[shape_start..].iter().copied()
                .take(self.func_ctx.descriptor_shapes.len())
                .collect();

            // Initialize aggregate slots and tracking bytes region.
            // - Aggregate slots: 0xFF poison makes uninitialized reads obvious
            // - Tracking bytes: 0x00 (UNINIT), so DropTracked skips them
            if let Some(frame_slot) = self.frame_slot {
                // Fill aggregate slots with 0xFF poison pattern.
                for (slot_idx, slot_ty) in self.func.slot_types.iter().enumerate() {
                    let repr = types::ir_type_to_cranelift(slot_ty);
                    if let CraneliftRepr::Aggregate(layout) = repr {
                        let slot_offset = self.layout.slot_offset(slot_idx as u32);
                        let addr = frame_slot.addr(&mut builder, slot_offset as i32);
                        let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                        let poison = builder.ins().iconst(cl_types::I8, 0xFF_u8 as i64);
                        builder.call_memset(self.isa.frontend_config(), addr, poison, size);
                    }
                }

                // Zero-init tracking bytes.
                if self.layout.tracking_count > 0 {
                    let track_addr = frame_slot.addr(&mut builder, self.layout.tracking_offset as i32);
                    let size = builder.ins().iconst(PTR_TYPE, self.layout.tracking_count as i64);
                    let zero = builder.ins().iconst(cl_types::I8, 0);
                    builder.call_memset(self.isa.frontend_config(), track_addr, zero, size);
                }
            }
        }

        // Compile each block.
        let func = self.func;
        for ir_block in func.blocks.iter().filter(|b| compiled(b.id)) {
            if self.osr.is_some() {
                self.compile_osr_block(&mut builder, ir_block)?;
            } else {
                self.compile_block(&mut builder, ir_block)?;
            }
        }
        if self.osr.is_some() {
            self.osr_check()?;
        }

        // Seal all blocks now that all predecessors are known (required for loops).
        builder.seal_all_blocks();

        // Finalize function.
        builder.finalize(self.isa.frontend_config());

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CraneliftError::Codegen(format!("define function: {}", e)))?;

        Ok(func_id)
    }

    /// Compile one block: its parameters, instructions and terminator.
    fn compile_block(&mut self, builder: &mut FunctionBuilder, ir_block: &IrBlock) -> Result<(), CraneliftError> {
        let cl_block = self.blocks[&ir_block.id];
        // Switch to block (entry already switched, unless this is an OSR
        // entry, whose entry is its own).
        if ir_block.id != BlockId(0) || self.osr.is_some() {
            builder.switch_to_block(cl_block);

            // Map block params to IR ValueIds.
            //
            // Block params implement loop carries/brings. The IR semantics specify
            // that Goto/Branch MOVE their args INTO the target block's param locations.
            // Each block param conceptually gets a "fresh" value each time the block
            // is entered.
            //
            // Scalars: Cranelift block param IS the value - pure SSA semantics.
            //
            // Aggregates: Cranelift block param is a POINTER to the source data.
            // We must memcpy to a local frame location to:
            // 1. Ensure value semantics (each iteration sees independent data)
            // 2. Prevent aliasing when source and dest overlap (loop carry case)
            let cl_params = builder.block_params(cl_block).to_vec();
            for (ir_value_id, &cl_param) in ir_block.params.iter().zip(cl_params.iter()) {
                let param_ty = self.func.value_types.get(ir_value_id.0 as usize)
                    .cloned()
                    .unwrap_or(IrType::Unit);
                let repr = types::ir_type_to_cranelift(&param_ty);

                match repr {
                    CraneliftRepr::Scalar(_) => {
                        // Scalar: block param IS the value (pure SSA).
                        self.values.insert(*ir_value_id, cl_param);
                    }
                    CraneliftRepr::Aggregate(layout) => {
                        // Aggregate: block param is PTR to source. Copy to local frame.
                        let frame_slot = self.frame_slot.expect("aggregate block param requires frame slot");
                        let dest_offset = self.layout.value_offset(ir_value_id.0);
                        let dest_addr = frame_slot.addr(builder, dest_offset as i32);

                        // memcpy from incoming pointer to local frame location.
                        let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                        builder.call_memcpy(self.isa.frontend_config(), dest_addr, cl_param, size);

                        // Use local address for this value.
                        self.values.insert(*ir_value_id, dest_addr);

                        // Mark as live if tracked.
                        self.mark_value_live(builder, *ir_value_id);
                    }
                }
            }

            // Don't seal yet - wait until all blocks are compiled for loop back-edges.
        }

        // Compile instructions.
        for inst in &ir_block.instructions {
            self.compile_instruction(builder, inst)?;
        }

        // Compile terminator.
        self.compile_terminator(builder, &ir_block.terminator)?;
        Ok(())
    }

    /// Build the Cranelift function signature.
    ///
    /// All functions have an implicit rt_handle as first parameter.
    /// For aggregate returns, an sret pointer is the second parameter.
    /// User-visible parameters follow.
    fn build_signature(&self) -> cl_ir::Signature {
        // Use the public function to keep consistency.
        build_signature_for_func(self.func, self.isa)
    }

    /// Compile a single instruction.
    fn compile_instruction(
        &mut self,
        builder: &mut FunctionBuilder,
        inst: &Instruction,
    ) -> Result<(), CraneliftError> {
        match inst {
            Instruction::Const { dest, value } => {
                self.compile_const(builder, *dest, value)?;
            }
            Instruction::StaticRef { dest, value } => {
                let addr = match self.static_consts.get(&static_const_key(value)) {
                    Some(StaticConstLoc::Address(addr)) => builder.ins().iconst(PTR_TYPE, *addr as i64),
                    Some(StaticConstLoc::Data(data_id)) => {
                        let gv = self.module.declare_data_in_func(*data_id, builder.func);
                        builder.ins().symbol_value(PTR_TYPE, gv)
                    }
                    None => return Err(CraneliftError::Unsupported(format!(
                        "a static const with nowhere to live: {}", value.0))),
                };
                // The reference is the address.
                self.values.insert(*dest, addr);
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                self.compile_binop(builder, *dest, *op, lhs, rhs)?;
            }
            Instruction::UnaryOp { dest, op, operand } => {
                self.compile_unaryop(builder, *dest, *op, operand)?;
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                self.compile_binop_checked(builder, *dest, *overflow, *op, lhs, rhs)?;
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                self.compile_unaryop_checked(builder, *dest, *overflow, *op, operand)?;
            }
            Instruction::OpAssign { place, op, rhs } => {
                self.compile_op_assign(builder, place, *op, rhs)?;
            }
            Instruction::OpAssignChecked { overflow, place, op, rhs } => {
                self.compile_op_assign_checked(builder, *overflow, place, *op, rhs)?;
            }
            Instruction::Widen { dest, src } => {
                self.compile_widen(builder, *dest, src)?;
            }
            Instruction::WidenFixed { dest, src } => {
                self.compile_widen_fixed(builder, *dest, src)?;
            }
            Instruction::Clone { dest, src } => {
                self.compile_clone(builder, *dest, src)?;
            }
            Instruction::Copy { dest, src } => {
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Move { dest, src } => {
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Pack { dest, ty: _, fields } => {
                self.compile_pack(builder, *dest, fields)?;
            }
            Instruction::Unpack { dests, src } => {
                self.compile_unpack(builder, dests, src)?;
            }
            Instruction::GetField { dest, src, field_index } => {
                self.compile_get_field(builder, *dest, src, *field_index)?;
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                self.compile_get_field_ref(builder, *dest, src, *field_index)?;
            }
            Instruction::DataBorrow { dest, src } => {
                self.compile_data_borrow(builder, *dest, src)?;
            }
            Instruction::SetField { slot, field_path, value } => {
                self.compile_set_field(builder, slot, field_path, value)?;
            }
            Instruction::Nop => {}
            Instruction::ListGet { dest, is_valid, list, index } => {
                self.compile_list_get(builder, *dest, *is_valid, list, index)?;
            }
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                self.compile_list_bounds_check(builder, *is_valid, list, index)?;
            }
            Instruction::ListSet { list, index, value } => {
                self.compile_list_set(builder, list, index, value)?;
            }
            Instruction::ListElementRef { dest, list, index } => {
                self.compile_list_element_ref(builder, *dest, list, index)?;
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                self.compile_map_get(builder, *dest, *is_valid, map, key)?;
            }
            Instruction::MapContainsKey { is_valid, map, key } => {
                self.compile_map_contains_key(builder, *is_valid, map, key)?;
            }
            Instruction::MapSetValue { map, key, value } => {
                self.compile_map_set_value(builder, map, key, value)?;
            }
            Instruction::MapValueRef { dest, map, key } => {
                self.compile_map_value_ref(builder, *dest, map, key)?;
            }
            Instruction::MapUpsert { map, key, value } => {
                self.compile_map_upsert(builder, map, key, value)?;
            }
            Instruction::TensorGet { dest, is_valid, tensor, index } => {
                self.compile_tensor_get(builder, *dest, *is_valid, tensor, index)?;
            }
            Instruction::TensorBoundsCheck { is_valid, tensor, index } => {
                self.compile_tensor_bounds_check(builder, *is_valid, tensor, index)?;
            }
            Instruction::TensorSet { tensor, index, value } => {
                self.compile_tensor_set(builder, tensor, index, value)?;
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                self.compile_tensor_index_ref(builder, *dest, tensor, index)?;
            }
            Instruction::DebugLog { operand } => {
                self.compile_debuglog(builder, operand)?;
            }
            Instruction::Call { dest, func, args, shape_descriptors, .. } => {
                self.compile_call(builder, *dest, func, args, shape_descriptors)?;
            }
            // ComptimeCall behaves exactly like Call - the specialization metadata is
            // only used by the specialization pass. Without specialization, this calls
            // the original function with original args.
            Instruction::ComptimeCall { dest, func, args, shape_descriptors, .. } => {
                self.compile_call(builder, *dest, func, args, shape_descriptors)?;
            }
            Instruction::SlotStoreCopy { dest, value } => {
                self.compile_slot_store(builder, dest, value, true)?;
            }
            Instruction::SlotStoreMove { dest, value } => {
                self.compile_slot_store(builder, dest, value, false)?;
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                self.compile_slot_load(builder, *dest, *slot, true)?;
            }
            Instruction::SlotLoadMove { dest, slot } => {
                // Precise slot load: compiler guarantees slot is occupied.
                self.compile_slot_load(builder, *dest, *slot, false)?;
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                // Tracked slot load: copy then mark slot as moved.
                self.compile_slot_load(builder, *dest, *slot, false)?;
                self.mark_tracking_moved(builder, &Operand::Slot(*slot));
            }
            Instruction::ParamStore { param, value } => {
                self.compile_param_store(builder, *param, value)?;
            }
            Instruction::ParamStoreTracked { param, value } => {
                self.compile_param_store_tracked(builder, *param, value)?;
            }
            Instruction::ParamSetField { param, field_path, value } => {
                self.compile_param_set_field(builder, param, field_path, value)?;
            }
            Instruction::ParamSetFieldTracked { param, field_path, value } => {
                self.compile_param_set_field_tracked(builder, param, field_path, value)?;
            }
            Instruction::RefStore { dest, value } => {
                self.compile_ref_store(builder, dest, value)?;
            }
            Instruction::RefStoreTracked { dest, value } => {
                self.compile_ref_store_tracked(builder, dest, value)?;
            }
            Instruction::RefSetField { dest, field_path, value } => {
                self.compile_ref_set_field(builder, dest, field_path, value)?;
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                self.compile_ref_set_field_tracked(builder, dest, field_path, value)?;
            }
            Instruction::Drop { operand } => {
                self.compile_drop(builder, operand)?;
            }
            Instruction::DropTracked { operand } => {
                self.compile_drop_tracked(builder, operand)?;
            }
            Instruction::DropViaRef { ref_value } => {
                self.compile_drop_via_ref(builder, *ref_value)?;
            }
            Instruction::DropField { base, field_path } => {
                self.compile_drop_field(builder, base, field_path)?;
            }
            Instruction::UnitEndDrop { operand } => {
                // Precise binding: unconditional drop.
                self.compile_drop(builder, operand)?;
            }
            Instruction::UnitEndDropTracked { operand } => {
                // Tracked binding: conditional drop (checks tracking byte).
                self.compile_drop_tracked(builder, operand)?;
            }
            Instruction::ListNew { dest, elements, descriptor } => {
                match descriptor {
                    Some(i) => self.compile_list_new_erased(builder, *dest, elements, *i)?,
                    None => self.compile_list_new(builder, *dest, elements)?,
                }
            }
            Instruction::SetNew { dest, elements, descriptor } => {
                match descriptor {
                    Some(i) => self.compile_set_new_erased(builder, *dest, elements, *i)?,
                    None => self.compile_set_new(builder, *dest, elements)?,
                }
            }
            Instruction::MapNew { dest, entries, descriptor } => {
                match descriptor {
                    Some(i) => self.compile_map_new_erased(builder, *dest, entries, *i)?,
                    None => self.compile_map_new(builder, *dest, entries)?,
                }
            }
            Instruction::TensorNew { dest, shape, elements } => {
                self.compile_tensor_new(builder, *dest, shape, elements)?;
            }
            Instruction::TableNew { dest, rows } => {
                self.compile_table_new(builder, *dest, rows)?;
            }

            // Option/Result instructions.
            Instruction::WrapSome { dest, inner } => {
                self.compile_wrap_some(builder, *dest, inner)?;
            }
            Instruction::WrapNone { dest } => {
                self.compile_wrap_none(builder, *dest)?;
            }
            Instruction::WrapOk { dest, inner } => {
                self.compile_wrap_ok(builder, *dest, inner)?;
            }
            Instruction::WrapErr { dest, inner } => {
                self.compile_wrap_err(builder, *dest, inner)?;
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                self.compile_unwrap_option(builder, *dest, *is_some, src)?;
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                self.compile_unwrap_result(builder, *ok_dest, *err_dest, *is_ok, src)?;
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                self.compile_enum_variant(builder, *dest, *variant_index, payload.as_ref())?;
            }
            Instruction::EnumDiscriminant { dest, src } => {
                self.compile_enum_discriminant(builder, *dest, src)?;
            }
            Instruction::EnumPayload { dest, src, variant_index } => {
                self.compile_enum_payload(builder, *dest, src, *variant_index)?;
            }
            Instruction::ErrorFrom { dest, inner } => {
                self.compile_error_from(builder, *dest, inner)?;
            }
            Instruction::DataFrom { dest, inner } => {
                self.compile_data_from(builder, *dest, inner)?;
            }
            Instruction::Erase { dest, src } => {
                self.compile_erasure(builder, *dest, src, true)?;
            }
            Instruction::EraseTracked { dest, src } => {
                self.compile_erasure_tracked(builder, *dest, src)?;
            }
            Instruction::Reify { dest, src } => {
                self.compile_erasure(builder, *dest, src, false)?;
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                self.compile_intrinsic(builder, *dest, *intrinsic, args)?;
            }

            // ================================================================
            // Slot tracking variants - write slot tracking bytes
            // ================================================================
            Instruction::SlotStoreCopyTracked { dest, value } => {
                self.compile_slot_store(builder, dest, value, true)?;
                if let SlotDest::Local(sid) = dest {
                    self.mark_slot_live(builder, *sid);
                }
            }
            Instruction::SlotStoreMoveTracked { dest, value } => {
                self.compile_slot_store(builder, dest, value, false)?;
                if let SlotDest::Local(sid) = dest {
                    self.mark_slot_live(builder, *sid);
                }
            }
            Instruction::SetFieldTracked { slot, field_path, value } => {
                self.compile_set_field(builder, slot, field_path, value)?;
                if let SlotDest::Local(sid) = slot {
                    self.mark_slot_live(builder, *sid);
                }
            }
        }
        Ok(())
    }

    /// Set all local function mappings at once.
    ///
    /// Use this for two-pass compilation where all functions are declared first.
    pub fn set_local_funcs(
        &mut self,
        local_funcs: HashMap<datalove_datafun_ir::CodeUnitId, LocalCallee>,
    ) {
        self.local_funcs = local_funcs;
    }

    /// Set the functions reached by a `CodeRef::External`, which a script unit
    /// after the first has and nothing else does.
    pub fn set_external_funcs(
        &mut self,
        external_funcs: HashMap<(u32, datalove_datafun_ir::CodeUnitId), LocalCallee>,
    ) {
        self.external_funcs = external_funcs;
    }

    /// Set all module function mappings at once.
    ///
    /// Use this for three-pass compilation where all module functions are declared first.
    pub fn set_module_funcs(&mut self, module_funcs: HashMap<(IrModuleId, datalove_datafun_ir::CodeUnitId), FuncId>) {
        self.module_funcs = module_funcs;
    }

    /// Say where the consts `StaticRef` names live.
    ///
    /// A body naming one this does not list is refused as unsupported.
    pub fn set_static_consts(&mut self, static_consts: StaticConsts) {
        self.static_consts = static_consts;
    }

    /// Compile a function, taking only the runtime handle, that builds each
    /// static const into its data object.
    ///
    /// The compiler is made over [`empty_function_unit`]. The program calls
    /// this before the script runs.
    pub fn compile_static_init(
        mut self,
        func_id: FuncId,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        self.compile_static_pass(func_id, |this, builder, addr, ty, value| {
            this.write_const_value_to_addr(builder, addr, ty, value)
        }, statics)
    }

    /// Compile a function, taking only the runtime handle, that destroys each
    /// static const [`Self::compile_static_init`] built.
    ///
    /// The program calls this before the runtime shuts down, which is when the
    /// runtime looks for leaks.
    pub fn compile_static_fini(
        mut self,
        func_id: FuncId,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        self.compile_static_pass(func_id, |this, builder, addr, ty, _value| {
            let runtime = this.runtime.expect("static const functions are compiled with runtime imports");
            let tydesc_id = this.tydesc(ty)?;
            let tydesc_gv = this.module.declare_data_in_func(tydesc_id, builder.func);
            let tydesc = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);
            let destroy = this.module.declare_func_in_func(runtime.destroy_local, builder.func);
            let rt = this.rt_handle_param.expect("set at entry");
            builder.ins().call(destroy, &[rt, addr, tydesc]);
            Ok(())
        }, statics)
    }

    /// Compile a function over each static const's address in turn.
    fn compile_static_pass(
        &mut self,
        func_id: FuncId,
        mut each: impl FnMut(&mut Self, &mut FunctionBuilder, cl_ir::Value, &IrType, &ConstValue)
            -> Result<(), CraneliftError>,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        let sig = self.build_signature();
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        self.rt_handle_param = Some(builder.block_params(entry)[0]);

        for (data_id, ty, value) in statics {
            let gv = self.module.declare_data_in_func(*data_id, builder.func);
            let addr = builder.ins().symbol_value(PTR_TYPE, gv);
            each(self, &mut builder, addr, ty, value)?;
        }
        builder.ins().return_(&[]);
        builder.finalize(self.isa.frontend_config());

        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CraneliftError::Codegen(format!("define function: {}", e)))?;
        Ok(())
    }

    /// Record a result the runtime wrote into `result_ptr` as `dest`.
    ///
    /// `values` holds an address for an aggregate and the value itself for a
    /// scalar, and every read of an operand takes what is there as one or the
    /// other by the type. A runtime call writes through a pointer whatever
    /// the type, so a scalar destination has to be loaded back out of it --
    /// left as the address, the next read of it takes the address for the
    /// value. That is how `(term Note : u32 / 42)@` came out as a stack
    /// address widened into the enum: a term is transparent over its payload,
    /// so cloning one has a scalar destination.
    fn record_runtime_result(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        result_ptr: cl_ir::Value,
    ) {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        match types::ir_type_to_cranelift(dest_ty) {
            CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), result_ptr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                self.values.insert(dest, result_ptr);
            }
        }
    }

    /// Get a pointer to an operand's value.
    ///
    /// For aggregates already in memory, returns the pointer directly.
    /// For scalars in registers, spills to a temporary stack location.
    fn get_operand_ptr(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        // Params are already passed by pointer - just return the pointer.
        if let Operand::Param(pid) = operand {
            return self.param_values.get(pid).copied().ok_or_else(|| {
                CraneliftError::Codegen(format!("undefined param: {:?}", pid))
            });
        }

        // Slots are in the frame - return the address directly.
        // This is important for mut params: we pass the slot address so writes
        // go to the original slot, not a copy.
        if let Operand::Slot(slot_id) = operand {
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for slot operand".into())
            })?;
            let slot_offset = self.layout.slot_offset(slot_id.0);
            let addr = frame_slot.addr(builder, slot_offset as i32);
            return Ok(addr);
        }

        // ValueRef: the stored value is a pointer - return it directly.
        // This is used for ref/mut/out params to get the dereferenced location.
        if let Operand::ValueRef(vid) = operand {
            return self.value(builder, *vid);
        }

        let ty = self.get_operand_type(operand)?;

        // Ref types store a pointer value - return it directly without spilling.
        if matches!(&ty, IrType::Ref(_)) {
            return self.get_operand_value(builder, operand);
        }

        let repr = types::ir_type_to_cranelift(&ty);

        match repr {
            CraneliftRepr::Aggregate(_) => {
                // Already a pointer.
                self.get_operand_value(builder, operand)
            }
            CraneliftRepr::Scalar(cl_ty) => {
                // Need to spill to memory.
                let val = self.get_operand_value(builder, operand)?;

                // Use the value's frame offset if available.
                if let Operand::Value(vid) = operand {
                    let offset = self.layout.value_offset(vid.0);
                    let frame_slot = self.frame_slot.ok_or_else(|| {
                        CraneliftError::Codegen("no frame slot for value spill".into())
                    })?;
                    let addr = frame_slot.addr(builder, offset as i32);
                    builder.ins().store(MemFlagsData::new(), val, addr, 0);
                    return Ok(addr);
                }

                // For other operand types, create a temporary slot.
                // This is a simple approach - we create a new stack slot for each spill.
                let size = cl_ty.bytes();
                let slot_data = cl_ir::StackSlotData::new(
                    cl_ir::StackSlotKind::ExplicitSlot,
                    size,
                    types::align_shift(size.max(1)),
                );
                let temp_slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);
                builder.ins().store(MemFlagsData::new(), val, addr, 0);
                Ok(addr)
            }
        }
    }

    /// Get a Cranelift value for an operand.
    fn get_operand_value(
        &self,
        builder: &mut FunctionBuilder,
        op: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        match op {
            Operand::Value(vid) => self.value(builder, *vid),
            Operand::ValueRef(vid) => {
                // ValueRef: the stored value is a pointer. Dereference it.
                let ptr = self.value(builder, *vid)?;

                // Get the inner type (the type being pointed to).
                let ref_ty = &self.func.value_types[vid.0 as usize];
                let inner_ty = match ref_ty {
                    IrType::Ref(inner) => inner.as_ref(),
                    _ => return Err(CraneliftError::Codegen(format!(
                        "ValueRef on non-Ref type: {:?}", ref_ty
                    ))),
                };
                let repr = types::ir_type_to_cranelift(inner_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        // Load scalar value from the pointer.
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), ptr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // For aggregates, return the pointer itself.
                        Ok(ptr)
                    }
                }
            }
            Operand::Param(pid) => {
                // Params are passed by pointer. Load the value from the pointer.
                let param_ptr = self.param_values.get(pid).copied().ok_or_else(|| {
                    CraneliftError::Codegen(format!("undefined param: {:?}", pid))
                })?;

                let param_ty = &self.func_ctx.param_types[pid.0 as usize];
                let repr = types::ir_type_to_cranelift(param_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        // Load scalar value from param pointer.
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), param_ptr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // For aggregates, return the pointer itself.
                        Ok(param_ptr)
                    }
                }
            }
            Operand::Slot(slot_id) => {
                // Load value from slot in frame.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for slot operand".into())
                })?;

                let slot_offset = self.layout.slot_offset(slot_id.0);
                let slot_ty = &self.func.slot_types[slot_id.0 as usize];
                let repr = types::ir_type_to_cranelift(slot_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        let addr = frame_slot.addr(builder, slot_offset as i32);
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), addr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // Aggregate: return pointer to slot location.
                        Ok(frame_slot.addr(builder, slot_offset as i32))
                    }
                }
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                Err(CraneliftError::Unsupported(format!(
                    "reading {:?}, a binding of an earlier script unit",
                    op
                )))
            }
        }
    }

    /// Get the type of an operand.
    fn get_operand_type(&self, op: &Operand) -> Result<IrType, CraneliftError> {
        match op {
            Operand::Value(vid) => {
                Ok(self.func.value_types[vid.0 as usize].clone())
            }
            Operand::ValueRef(vid) => {
                // ValueRef dereferences, so return the inner type.
                let ref_ty = &self.func.value_types[vid.0 as usize];
                match ref_ty {
                    IrType::Ref(inner) => Ok(inner.as_ref().clone()),
                    _ => Err(CraneliftError::Codegen(format!(
                        "ValueRef on non-Ref type: {:?}", ref_ty
                    ))),
                }
            }
            Operand::Param(pid) => {
                Ok(self.func_ctx.param_types[pid.0 as usize].clone())
            }
            Operand::Slot(sid) => {
                Ok(self.func.slot_types[sid.0 as usize].clone())
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                Err(CraneliftError::Unsupported(format!(
                    "the type of {:?}, a binding of an earlier script unit",
                    op
                )))
            }
        }
    }

    /// Allocate a new Cranelift variable.
    #[allow(dead_code)]
    fn alloc_var(&mut self) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        var
    }

    /// Get the tracking byte offset for an operand, if it's tracked.
    fn tracking_byte_offset(&self, operand: &Operand) -> Option<u32> {
        match operand {
            Operand::Value(vid) => self.layout.values[vid.0 as usize].tracking_byte,
            Operand::Slot(sid) => self.layout.slots[sid.0 as usize].tracking_byte,
            Operand::Param(pid) => self.param_tracking_byte_offset(*pid),
            _ => None,
        }
    }

    /// Mark a tracking byte as LIVE.
    fn mark_tracking_live(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use datalove_datafun_ir::frame_layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = frame_slot.addr(builder, track_offset as i32);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            builder.ins().store(MemFlagsData::new(), live_val, track_addr, 0);
        }
    }

    /// Mark a tracking byte as MOVED.
    fn mark_tracking_moved(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use datalove_datafun_ir::frame_layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = frame_slot.addr(builder, track_offset as i32);
            let moved_val = builder.ins().iconst(cl_types::I8, tracking::MOVED as i64);
            builder.ins().store(MemFlagsData::new(), moved_val, track_addr, 0);
        }
    }

    /// Mark a tracked value as LIVE after initialization.
    fn mark_value_live(&self, builder: &mut FunctionBuilder, vid: ValueId) {
        self.mark_tracking_live(builder, &Operand::Value(vid));
    }

    /// Mark a tracked slot as LIVE after store.
    fn mark_slot_live(&self, builder: &mut FunctionBuilder, sid: SlotId) {
        self.mark_tracking_live(builder, &Operand::Slot(sid));
    }

    /// Get tracking byte offset for a param, if tracked.
    fn param_tracking_byte_offset(&self, pid: ParamId) -> Option<u32> {
        self.layout.param_tracking[pid.0 as usize]
    }

    /// Mark a tracked param as LIVE after store.
    fn mark_param_live(&self, builder: &mut FunctionBuilder, pid: ParamId) {
        use datalove_datafun_ir::frame_layout::tracking;

        if let Some(track_offset) = self.param_tracking_byte_offset(pid) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = frame_slot.addr(builder, track_offset as i32);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            builder.ins().store(MemFlagsData::new(), live_val, track_addr, 0);
        }
    }

    /// Get the pointer stored in a ref value (from GetFieldRef).
    ///
    /// For ref values, the stored cranelift value IS the pointer.
    fn get_value_as_ref_ptr(
        &self,
        builder: &mut FunctionBuilder,
        vid: ValueId,
    ) -> Result<cl_ir::Value, CraneliftError> {
        // GetFieldRef stores the pointer directly in self.values.
        self.value(builder, vid)
    }

    /// The Cranelift value for `vid`: what defined it, or in an OSR entry, for
    /// one defined before the loop, what the interpreter's frame holds.
    fn value(&self, builder: &mut FunctionBuilder, vid: ValueId) -> Result<cl_ir::Value, CraneliftError> {
        if let Some(v) = self.values.get(&vid) {
            return Ok(*v);
        }
        let Some(osr) = &self.osr else {
            return Err(CraneliftError::Codegen(format!("undefined value: {:?}", vid)));
        };
        if let Some(v) = osr.materialized.borrow().get(&vid) {
            return Ok(*v);
        }
        if self.ref_descs.contains_key(&vid) {
            return Err(CraneliftError::Unsupported(format!(
                "entering a loop that needs reference {:?}, whose descriptor is worked out where it is made", vid)));
        }
        use cranelift_codegen::cursor::{Cursor, FuncCursor};
        let ty = &self.func.value_types[vid.0 as usize];
        let offset = self.layout.value_offset(vid.0) as i32;
        let mut pos = FuncCursor::new(builder.func).at_inst(osr.entry_jump);
        let remade = osr.consts.get(&vid).and_then(|c| constants::scalar_const(pos.ins(), c));
        let v = match (remade, types::ir_type_to_cranelift(ty)) {
            (Some(v), _) => v,
            (None, CraneliftRepr::Scalar(cl_ty)) => pos.ins().load(cl_ty, MemFlagsData::trusted(), osr.frame, offset),
            (None, CraneliftRepr::Aggregate(_)) => pos.ins().iadd_imm_s(osr.frame, offset as i64),
        };
        osr.materialized.borrow_mut().insert(vid, v);
        Ok(v)
    }

    /// Set up an OSR entry's own entry block: the parameters and descriptors
    /// from where the interpreter keeps them, and a jump to the header with
    /// its parameters as the frame holds them.
    fn enter_osr(&mut self, builder: &mut FunctionBuilder, reachable: std::collections::HashSet<BlockId>) {
        let spec = self.osr_spec.as_ref().expect("an OSR entry");
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let params = builder.block_params(entry).to_vec();
        self.rt_handle_param = Some(params[0]);
        let has_sret = uses_sret(&self.func_ctx.return_type);
        if has_sret {
            self.sret_param = Some(params[1]);
        }
        let frame = params[if has_sret { 2 } else { 1 }];
        self.frame_slot = Some(FrameBase::Ptr(frame));

        let flags = MemFlagsData::trusted();
        let word = types::PTR_SIZE as i32;
        for (i, &offset) in spec.param_offsets.iter().enumerate() {
            let ptr = builder.ins().load(PTR_TYPE, flags, frame, offset as i32);
            self.param_values.insert(ParamId(i as u32), ptr);
        }
        for &param in &self.func_ctx.descriptor_params {
            let offset = spec.param_offsets[param.0 as usize] as i32 + word;
            let tydesc = builder.ins().load(PTR_TYPE, flags, frame, offset);
            self.descriptor_values.insert(param, tydesc);
        }
        self.shape_descriptor_values = (0..self.func_ctx.descriptor_shapes.len())
            .map(|k| builder.ins().load(PTR_TYPE, flags, frame, spec.shape_offset as i32 + k as i32 * word))
            .collect();

        // The header's parameters as an edge into it would pass them: a
        // scalar itself, an aggregate by its address.
        let header = self.func.blocks.iter().find(|b| b.id == spec.header)
            .expect("the header is a block of the function");
        let args: Vec<cranelift_codegen::ir::BlockArg> = header.params.iter().map(|&v| {
            let offset = self.layout.value_offset(v.0) as i32;
            let arg = match types::ir_type_to_cranelift(&self.func.value_types[v.0 as usize]) {
                CraneliftRepr::Scalar(cl_ty) => builder.ins().load(cl_ty, flags, frame, offset),
                CraneliftRepr::Aggregate(_) => builder.ins().iadd_imm_s(frame, offset as i64),
            };
            cranelift_codegen::ir::BlockArg::from(arg)
        }).collect();
        let entry_jump = builder.ins().jump(self.blocks[&spec.header], &args);

        let consts = self.func.blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter_map(|instr| match instr {
                Instruction::Const { dest, value } => Some((*dest, value.clone())),
                _ => None,
            })
            .collect();
        self.osr = Some(OsrState {
            reachable,
            frame,
            entry_jump,
            materialized: std::cell::RefCell::new(HashMap::new()),
            consts,
            defined: HashMap::new(),
        });
    }

    /// Compile a block of an OSR entry, noting what compiling it defined.
    fn compile_osr_block(&mut self, builder: &mut FunctionBuilder, ir_block: &IrBlock) -> Result<(), CraneliftError> {
        let before: std::collections::HashSet<ValueId> = self.values.keys().copied().collect();
        self.compile_block(builder, ir_block)?;
        let new = self.values.keys().filter(|v| !before.contains(v)).copied().collect();
        self.osr.as_mut().expect("an OSR entry").defined.insert(ir_block.id, new);
        Ok(())
    }

    /// Refuse an OSR entry whose header needs a value that the code it
    /// reaches defines: an enclosing loop's, or one asked for before the block
    /// defining it was compiled, either way read from the frame where the
    /// code also makes it.
    ///
    fn osr_check(&self) -> Result<(), CraneliftError> {
        let osr = self.osr.as_ref().expect("an OSR entry");
        let defined = &osr.defined;
        let spec = self.osr_spec.as_ref().expect("an OSR entry");
        let all: std::collections::HashSet<ValueId> = defined.values().flatten().copied().collect();
        if let Some(v) = osr.materialized.borrow().keys().find(|v| all.contains(v)) {
            return Err(CraneliftError::Unsupported(format!(
                "entering a loop at {:?} that uses {:?} before the code it reaches defines it", spec.header, v)));
        }

        // What each block uses that it does not itself define, then live-in
        // sets by the usual backward iteration over the reached blocks.
        let blocks: Vec<&IrBlock> = self.func.blocks.iter().filter(|b| osr.reachable.contains(&b.id)).collect();
        let mut uses: HashMap<BlockId, std::collections::HashSet<ValueId>> = HashMap::new();
        for block in &blocks {
            let mut used = std::collections::HashSet::new();
            let mut add = |op: &Operand| {
                if let Operand::Value(v) | Operand::ValueRef(v) = op {
                    used.insert(*v);
                }
            };
            block.instructions.iter().for_each(|i| i.for_each_operand(&mut add));
            block.terminator.for_each_operand(&mut add);
            let own = &defined[&block.id];
            used.retain(|v| !own.contains(v));
            uses.insert(block.id, used);
        }
        let mut live_in: HashMap<BlockId, std::collections::HashSet<ValueId>> = uses.clone();
        loop {
            let mut changed = false;
            for block in blocks.iter().rev() {
                let mut live: std::collections::HashSet<ValueId> = block.terminator.successors().into_iter()
                    .filter_map(|s| live_in.get(&s))
                    .flatten()
                    .copied()
                    .collect();
                let own = &defined[&block.id];
                live.retain(|v| !own.contains(v));
                live.extend(uses[&block.id].iter().copied());
                let entry = live_in.get_mut(&block.id).expect("every reached block");
                if live.len() != entry.len() {
                    *entry = live;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        match live_in[&spec.header].iter().find(|v| all.contains(v)) {
            Some(v) => Err(CraneliftError::Unsupported(format!(
                "entering a loop at {:?} that needs {:?}, which an enclosing loop defines", spec.header, v))),
            None => Ok(()),
        }
    }

    /// Emit a zero constant for a scalar Cranelift type.
    ///
    /// Handles float types (f32const/f64const) and integer types (iconst).
    pub(super) fn emit_scalar_zero(
        builder: &mut FunctionBuilder,
        cl_ty: cl_ir::Type,
    ) -> cl_ir::Value {
        if cl_ty == cl_types::F32 {
            builder.ins().f32const(0.0f32)
        } else if cl_ty == cl_types::F64 {
            builder.ins().f64const(0.0f64)
        } else {
            builder.ins().iconst(cl_ty, 0)
        }
    }

    /// Emit a conditional tracking byte write based on an is_valid flag.
    ///
    /// Writes LIVE if is_valid is true, UNINIT if false.
    pub(super) fn emit_conditional_tracking(
        &self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid_val: cl_ir::Value,
    ) {
        if let Some(track_offset) = self.layout.values[dest.0 as usize].tracking_byte {
            use datalove_datafun_ir::frame_layout::tracking;
            let frame_slot = self.frame_slot.expect("tracking requires frame slot");
            let frame_addr = frame_slot.addr(builder, 0);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);
            let track_addr = builder.ins().iadd_imm_s(frame_addr, track_offset as i64);
            let track_val = builder.ins().select(is_valid_val, live_val, uninit_val);
            builder.ins().store(MemFlagsData::new(), track_val, track_addr, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_object::{ObjectBuilder, ObjectModule};
    use datalove_datafun_ir::{BinOp, IrBlock, IrCodeUnit, CodeUnitId, CodeUnitContext, FunctionContext, ConstValue, Terminator, SymbolTable};

    fn create_test_isa() -> std::sync::Arc<dyn TargetIsa> {
        use cranelift_codegen::isa;
        use cranelift_codegen::settings::{self, Configurable};
        use target_lexicon::Triple;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed").unwrap();
        let flags = settings::Flags::new(settings_builder);

        isa::lookup(Triple::host())
            .unwrap()
            .finish(flags)
            .unwrap()
    }

    fn create_test_module(isa: std::sync::Arc<dyn TargetIsa>) -> ObjectModule {
        let obj_builder = ObjectBuilder::new(
            isa,
            "test",
            cranelift_module::default_libcall_names(),
        ).unwrap();
        ObjectModule::new(obj_builder)
    }

    /// Helper to create a function code unit for tests.
    fn make_func_unit(
        name: &str,
        return_type: IrType,
        blocks: Vec<IrBlock>,
        value_types: Vec<IrType>,
    ) -> IrCodeUnit {
        IrCodeUnit {
            id: CodeUnitId(0),
            name: name.into(),
            blocks,
            value_count: value_types.len() as u32,
            slot_count: 0,
            value_types,
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![],
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Function(FunctionContext {
            descriptor_params: Vec::new(),
                params: vec![],
                param_modes: vec![],
                param_types: vec![],
                return_type,
                tracked_params: vec![],
            descriptor_shapes: Vec::new(),
            }),
            nested_units: vec![],
        }
    }

    #[test]
    fn test_compile_const_i32() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { 42 }
        let code_unit = make_func_unit(
            "test_const",
            IrType::I32,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(42),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(0))),
                    },
                },
            ],
            vec![IrType::I32],
        );

        let mut tydescs = TyDescEmitter::new();
        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, &mut tydescs);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    // test_compile_binop_add removed: i32 + i32 -> i32 is invalid IR.
    // Fixed-width integer arithmetic uses widening (-> Int) or checked ops (BinOpChecked).

    #[test]
    fn test_compile_comparison() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> bool { 10 < 32 }
        let code_unit = make_func_unit(
            "test_cmp",
            IrType::Bool,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(10),
                        },
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I32(32),
                        },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Lt,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            vec![IrType::I32, IrType::I32, IrType::Bool],
        );

        let mut tydescs = TyDescEmitter::new();
        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, &mut tydescs);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_unary_neg() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { -42 }
        let code_unit = make_func_unit(
            "test_neg",
            IrType::I32,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(42),
                        },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: datalove_datafun_ir::UnaryOp::Neg,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
            ],
            vec![IrType::I32, IrType::I32],
        );

        let mut tydescs = TyDescEmitter::new();
        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, &mut tydescs);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_branch() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function with a branch:
        // fn foo() -> i32 {
        //     if true { 1 } else { 2 }
        // }
        let code_unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: "test_branch".into(),
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::Bool(true),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(0)),
                        then_block: BlockId(1),
                        then_args: vec![],
                        else_block: BlockId(2),
                        else_args: vec![],
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I32(1),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
                IrBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(2),
                            value: ConstValue::I32(2),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::Bool, IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![],
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Function(FunctionContext {
            descriptor_params: Vec::new(),
                params: vec![],
                param_modes: vec![],
                param_types: vec![],
                return_type: IrType::I32,
                tracked_params: vec![],
            descriptor_shapes: Vec::new(),
            }),
            nested_units: vec![],
        };

        let mut tydescs = TyDescEmitter::new();
        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, &mut tydescs);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }
}
