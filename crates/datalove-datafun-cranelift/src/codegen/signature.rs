//! Signatures: what a datalove function, a native and a local callee are
//! called with.

use super::*;

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

    // Then a descriptor for each type parameter, last.
    for _ in 0..func_ctx.type_param_count {
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
    // `NativeContext::undetermined_type_params`; almost every native has none.
    for _ in &ctx.undetermined_type_params {
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
    /// How the callee takes each parameter, which the call site needs because
    /// an `out` one has its old value dropped here rather than there.
    pub param_modes: Vec<ParamMode>,
    /// How many type parameters the callee takes a descriptor for, last.
    pub type_param_count: u32,
    /// Whether the callee's signature takes a pointer to return through,
    /// which is its own return type's to say: a generic returning `T` does,
    /// whatever a caller bound `T` to.
    pub uses_sret: bool,
}

impl LocalCallee {
    /// What a call to `unit` has to pass, given the id it was declared under.
    pub fn of(unit: &IrCodeUnit, func_id: FuncId) -> Self {
        let ctx = unit.function_context();
        LocalCallee {
            func_id,
            param_modes: ctx.map(|c| c.param_modes.clone()).unwrap_or_default(),
            type_param_count: ctx.map(|c| c.type_param_count).unwrap_or(0),
            uses_sret: ctx.is_some_and(|c| uses_sret(&c.return_type)),
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
pub(super) fn return_slot_values(func: &IrCodeUnit) -> std::collections::HashSet<ValueId> {
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
