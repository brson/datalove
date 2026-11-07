//! Signatures: what a datalove function, a native and a local callee are
//! called with.

use std::ops::Range;

use super::*;

/// How a compiled function is handed one argument by another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgPass {
    /// The address of the argument.
    Pointer,
    /// The argument itself: a scalar, or the words of a small aggregate.
    Value(Vec<cl_ir::Type>),
}

/// How a compiled function gives its result back to another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetPass {
    /// It has none.
    Unit,
    /// Through a pointer the caller passes, second after the runtime handle.
    Sret,
    /// In registers: a scalar, or the words of a small aggregate.
    Value(Vec<cl_ir::Type>),
}

/// The Cranelift types a value of `ty` is passed as by value, if it can be: a
/// scalar's, or a small static aggregate's words.
fn by_value(ty: &IrType) -> Option<Vec<cl_ir::Type>> {
    super::words::reg_pieces(ty).map(|cut| cut.into_iter().map(|(_, t)| t).collect())
}

/// How an argument for a parameter of `mode` and type `ty` is passed between
/// compiled functions: an `in` one that can be, by value; the rest, which the
/// callee may write through or keep, by pointer.
pub fn arg_pass(mode: ParamMode, ty: &IrType) -> ArgPass {
    match (mode, by_value(ty)) {
        (ParamMode::In, Some(tys)) => ArgPass::Value(tys),
        _ => ArgPass::Pointer,
    }
}

/// How a result of type `ty` is given back between compiled functions.
pub fn ret_pass(ty: &IrType) -> RetPass {
    if !has_result(ty) {
        return RetPass::Unit;
    }
    match by_value(ty) {
        Some(tys) => RetPass::Value(tys),
        None => RetPass::Sret,
    }
}

/// Where each part of a call between compiled functions goes among the
/// signature's parameters, and how its result comes back.
///
/// The runtime handle comes first, then the `sret` pointer if the result goes
/// back through one, then each argument as `arg_pass` says, and a descriptor
/// for each type parameter last. This is the one account of that order: the
/// signature, a call, the function's own entry block, the adapter into it and
/// the JIT's stub for it all place their values by it.
#[derive(Clone, Debug)]
pub struct ParamLayout {
    /// Where the `sret` pointer is, if the result goes back through one.
    pub sret: Option<usize>,
    /// How each argument is passed, and where.
    pub args: Vec<(ArgPass, Range<usize>)>,
    /// Where the type parameters' descriptors are.
    pub descriptors: Range<usize>,
    pub ret: RetPass,
}

impl ParamLayout {
    /// Where the runtime handle is.
    pub const RT: usize = 0;

    /// The layout of a call to a function of context `ctx`.
    pub fn of(ctx: &FunctionContext) -> Self {
        let ret = ret_pass(&ctx.return_type);
        let mut next = Self::RT + 1;
        let mut take = |n: usize| {
            next += n;
            next - n..next
        };
        let sret = (ret == RetPass::Sret).then(|| take(1).start);
        let args = ctx.param_modes.iter().zip(&ctx.param_types)
            .map(|(mode, ty)| {
                let pass = arg_pass(*mode, ty);
                let n = match &pass {
                    ArgPass::Pointer => 1,
                    ArgPass::Value(tys) => tys.len(),
                };
                (pass, take(n))
            })
            .collect();
        let descriptors = take(ctx.type_param_count as usize);
        ParamLayout { sret, args, descriptors, ret }
    }

    /// How many parameters there are.
    pub fn len(&self) -> usize {
        self.descriptors.end
    }

    /// The type of each parameter, in order.
    fn param_types(&self) -> Vec<cl_ir::Type> {
        let mut tys = vec![PTR_TYPE; self.len()];
        for (pass, at) in &self.args {
            if let ArgPass::Value(arg_tys) = pass {
                tys[at.clone()].copy_from_slice(arg_tys);
            }
        }
        tys
    }
}

/// Build the signature compiled functions call `func` by, as `ParamLayout`
/// lays it out. A result `ret_pass` gives back by value comes back in
/// registers. The calling convention is the tail one, which has registers for
/// up to eight words of results where the system's has two.
///
/// Panics if the code unit is not a function.
pub fn build_signature_for_func(
    func: &IrCodeUnit,
    _isa: &dyn TargetIsa,
) -> cl_ir::Signature {
    let func_ctx = func.function_context()
        .expect("build_signature_for_func requires a function code unit");
    let layout = ParamLayout::of(func_ctx);
    let mut sig = cl_ir::Signature::new(cranelift_codegen::isa::CallConv::Tail);
    sig.params.extend(layout.param_types().into_iter().map(cl_ir::AbiParam::new));
    if let RetPass::Value(tys) = layout.ret {
        sig.returns.extend(tys.into_iter().map(cl_ir::AbiParam::new));
    }
    sig
}

/// Build the signature `func` is entered by from outside compiled code: the
/// interpreter's, through an adapter that calls the function itself.
///
/// The runtime handle, then an `sret` pointer for every result but unit, then
/// a pointer per argument and a descriptor per type parameter, in the system
/// calling convention.
///
/// Panics if the code unit is not a function.
pub fn build_entry_signature_for_func(
    func: &IrCodeUnit,
    isa: &dyn TargetIsa,
) -> cl_ir::Signature {
    let func_ctx = func.function_context()
        .expect("build_entry_signature_for_func requires a function code unit");

    let call_conv = isa.default_call_conv();
    let mut sig = cl_ir::Signature::new(call_conv);

    // Implicit rt_handle as first param (pointer to runtime).
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));

    // Every result but unit goes back through a pointer the caller passes.
    if has_result(&func_ctx.return_type) {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // User parameters are passed by pointer.
    for _ in &func_ctx.param_types {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // Then a descriptor for each type parameter, last.
    for _ in 0..func_ctx.type_param_count {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

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
    // `NativeContext::undetermined_type_params`; almost every native has none.
    for _ in &ctx.undetermined_type_params {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // RtStatus return (i8).
    sig.returns.push(cl_ir::AbiParam::new(cl_ir::types::I8));

    sig
}

/// Whether a function returning `ret_ty` gives anything back: whether its
/// result is not unit.
///
/// An entry from outside compiled code takes an `sret` pointer for every such
/// result; a call between compiled functions does as `ret_pass` says.
pub fn has_result(ret_ty: &IrType) -> bool {
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
    /// How the callee takes each parameter, which the call site needs because
    /// an `out` one has its old value dropped here rather than there.
    pub param_modes: Vec<ParamMode>,
    /// How many type parameters the callee takes a descriptor for, last.
    pub type_param_count: u32,
    /// The types the callee declares its parameters as.
    pub param_types: Vec<IrType>,
    /// Where each argument goes, and how the result comes back, which is the
    /// callee's own return type's to say: a generic returning `T` returns
    /// through `sret`, whatever a caller bound `T` to.
    pub layout: ParamLayout,
}

impl LocalCallee {
    /// What a call to `unit` has to pass, given the id it was declared under.
    ///
    /// Panics if the code unit is not a function.
    pub fn of(unit: &IrCodeUnit, func_id: FuncId) -> Self {
        let ctx = unit.function_context().expect("a local callee is a function");
        LocalCallee {
            func_id,
            param_modes: ctx.param_modes.clone(),
            type_param_count: ctx.type_param_count,
            param_types: ctx.param_types.clone(),
            layout: ParamLayout::of(ctx),
        }
    }
}

/// The values a function builds straight into its caller's result slot: each
/// an option or result its block wraps and then returns, and uses for nothing
/// else.
///
/// Built in this frame instead, such a value is written field by field and then
/// copied out by the return, and the copy's wide loads read back the narrow
/// stores that just wrote it, which the processor cannot forward from: on
/// recursive `fib`, which returns `!u32`, most of a call's time went there.
pub(super) fn return_slot_values(func: &IrCodeUnit) -> bct::lookup::LookupSet<ValueId> {
    let mut uses: LookupMap<ValueId, u32> = LookupMap::default();
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

/// Define the entry of `func` from outside compiled code, under `symbol`: a
/// function of `build_entry_signature_for_func`'s signature, which loads each
/// argument passed by value from its pointer, calls `internal`, the function
/// itself, and stores a result that comes back in registers through `sret`.
pub fn define_entry_adapter<M: Module>(
    module: &mut M,
    isa: &dyn TargetIsa,
    func: &IrCodeUnit,
    internal: FuncId,
    symbol: &str,
) -> Result<Defined, CraneliftError> {
    let func_ctx = func.function_context().expect("an entry is into a function");
    let sig = build_entry_signature_for_func(func, isa);
    let id = module.declare_function(symbol, Linkage::Local, &sig)
        .map_err(|e| CraneliftError::Module(format!("declare entry: {e}")))?;
    let mut cl_func = cl_ir::Function::with_name_signature(cl_ir::UserFuncName::user(0, id.as_u32()), sig);
    let mut fb_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();

    // The entry's own parameters: the runtime handle, an `sret` for any
    // result, a pointer per argument, and the descriptors.
    let sret = has_result(&func_ctx.return_type).then(|| params[1]);
    let mut entry_args = params[1 + sret.iter().count()..].iter().copied();

    let layout = ParamLayout::of(func_ctx);
    let mut args = vec![params[0]; layout.len()];
    if let Some(at) = layout.sret {
        args[at] = sret.expect("a result through sret has one");
    }
    for ((pass, at), ty) in layout.args.iter().zip(&func_ctx.param_types) {
        let ptr = entry_args.next().expect("an entry pointer per argument");
        match pass {
            ArgPass::Pointer => args[at.start] = ptr,
            ArgPass::Value(_) => args[at.clone()].copy_from_slice(&super::words::load_regs(&mut builder, ptr, ty)),
        }
    }
    for (at, descriptor) in layout.descriptors.clone().zip(entry_args) {
        args[at] = descriptor;
    }

    let callee = module.declare_func_in_func(internal, builder.func);
    let call = builder.ins().call(callee, &args);
    let results = builder.inst_results(call).to_vec();
    if let RetPass::Value(_) = layout.ret {
        let sret = sret.expect("a result in registers has an sret to go through here");
        super::words::store_regs(&mut builder, sret, &func_ctx.return_type, &results);
    }
    builder.ins().return_(&[]);
    builder.finalize(isa.frontend_config());

    let mut ctx = cranelift_codegen::Context::new();
    ctx.func = cl_func;
    module.define_function(id, &mut ctx).map_err(define_error)?;
    let code_size = ctx.compiled_code().expect("a function just defined").code_info().total_size;
    Ok(Defined { id, code_size })
}
