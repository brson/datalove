//! Core codegen driver for translating IR to Cranelift.
//!
//! Translates [`IrCodeUnit`] to Cranelift IR using FunctionBuilder. The main type is
//! [`FunctionCompiler`], which handles the translation of a single function.
//!
//! # Submodules
//!
//! The driver -- bodies, blocks, operands, tracking bytes, signatures, static
//! consts and OSR entries -- and the codegen for each kind of instruction are
//! submodules, each said below what it holds.
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
/// Entering a function at a loop header (on-stack replacement).
mod osr;
/// Operands and values.
mod operands;
/// Function, native and local callee signatures.
mod signature;
/// Static consts and the passes that build and destroy them.
mod statics;
/// Tracking bytes.
mod tracking;
/// Compiling a body, its blocks and its instructions.
mod translate;
#[cfg(test)]
mod tests;

pub use osr::OsrSpec;

/// A function compiled into the module: its id, and how many bytes of machine
/// code it came to.
pub struct Defined {
    pub id: FuncId,
    pub code_size: u32,
}
pub use signature::{build_native_signature, build_signature_for_func, uses_sret, LocalCallee};
pub use statics::{empty_function_unit, static_const_key, StaticConstLoc, StaticConsts};
use osr::{reachable_from, OsrState};
use signature::return_slot_values;

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

/// Compiles a single IR function to Cranelift IR.
pub struct FunctionCompiler<'a, M: Module> {
    /// The IR code unit being compiled.
    func: &'a IrCodeUnit,
    /// Where the runtime says whether calls are being abandoned, if this code
    /// is the JIT's and has to look after each call; see
    /// `return_if_abandoning`.
    abandoning_offset: Option<i32>,
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
    registry: Option<&'a FunctionRegistry>,
    /// Where the frame is: values, slots and tracking bytes, as `layout` says.
    frame_slot: Option<FrameBase>,
    /// The runtime's functions, as this module imports them.
    runtime: RuntimeImports,
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

    /// Create a function compiler for `func`, whose code calls the runtime
    /// through `runtime`.
    ///
    /// Panics if the code unit is not a function.
    pub fn new(
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
            abandoning_offset: None,
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
            frame_slot: None,
            runtime,
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

    /// Have compiled code look after each call it makes for whether the call
    /// is being abandoned, and return if so: what the JIT's code has to do,
    /// since what it calls may be interpreted.
    pub fn abandon_on_interpreter_errors(&mut self) {
        self.abandoning_offset = Some(datalove_rt::rust::abandoning_offset());
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

}
