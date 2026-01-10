//! JIT compiler wrapping Cranelift's JITModule.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use target_lexicon::Triple;

use datalove_datafun_ir::{FuncRef, Instruction, IrFunction, IrModuleId};
use datalove_datafun_interp::{ExecutionContext, FunctionRegistry};
use datalove_datafun_aot_cranelift::codegen::{self, build_signature_for_func, uses_sret};
use datalove_datafun_aot_cranelift::runtime::RuntimeImports;
use datalove_datafun_aot_cranelift::tydesc_emit::TyDescEmitter;
use datalove_datafun_aot_cranelift::types::PTR_TYPE;

use crate::trampoline::{self, EncodedFuncKey};
use crate::JitError;

/// JIT compiler using Cranelift.
pub struct JitCompiler {
    /// JIT module for in-memory code generation.
    jit_module: JITModule,
    /// Target ISA.
    isa: Arc<dyn TargetIsa>,
    /// Runtime function imports.
    runtime: RuntimeImports,
    /// Type descriptor emitter.
    tydesc_emitter: TyDescEmitter,
    /// Dispatch function FuncId (for JIT->interpreter calls).
    #[allow(dead_code)]
    dispatch_func_id: FuncId,
}

impl JitCompiler {
    /// Create a new JIT compiler for the host target.
    pub fn new() -> Result<Self, JitError> {
        use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam};

        let triple = Triple::host();

        // Build target ISA.
        let builder = cranelift_codegen::isa::lookup(triple.clone())
            .map_err(|e| JitError::CompilationFailed(format!("unsupported target: {}", e)))?;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed")
            .map_err(|e| JitError::CompilationFailed(format!("settings error: {}", e)))?;

        let flags = settings::Flags::new(settings_builder);
        let isa = builder.finish(flags)
            .map_err(|e| JitError::CompilationFailed(format!("isa error: {}", e)))?;

        // Build JIT module with dispatch symbol registered.
        let mut jit_builder = JITBuilder::with_isa(isa.clone(), cranelift_module::default_libcall_names());

        // Register the dispatch function so JIT code can call it.
        jit_builder.symbol("__jit_dispatch_call", trampoline::dispatch_fn_ptr());

        let mut jit_module = JITModule::new(jit_builder);

        // Declare runtime imports.
        let call_conv = isa.default_call_conv();
        let runtime = RuntimeImports::declare(&mut jit_module, call_conv)
            .map_err(|e| JitError::CompilationFailed(format!("runtime imports: {}", e)))?;

        // Declare the dispatch function signature.
        // __jit_dispatch_call(rt_handle, encoded_key, ret_dest, ret_is_sret, arg_count, args) -> usize
        let mut dispatch_sig = cl_ir::Signature::new(call_conv);
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // rt_handle
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // encoded_key
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // ret_dest
        dispatch_sig.params.push(AbiParam::new(cl_types::I8));  // ret_is_sret
        dispatch_sig.params.push(AbiParam::new(cl_types::I32)); // arg_count
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // args
        dispatch_sig.returns.push(AbiParam::new(cl_types::I64)); // return value

        let dispatch_func_id = jit_module
            .declare_function("__jit_dispatch_call", Linkage::Import, &dispatch_sig)
            .map_err(|e| JitError::CompilationFailed(format!("declare dispatch: {}", e)))?;

        let tydesc_emitter = TyDescEmitter::new();

        Ok(Self {
            jit_module,
            isa,
            runtime,
            tydesc_emitter,
            dispatch_func_id,
        })
    }

    /// Compile a function to native code (no calls to other functions).
    ///
    /// Returns (code_ptr, uses_sret).
    pub fn compile_function(&mut self, func: &IrFunction) -> Result<(*const u8, bool), JitError> {
        // Build a FunctionCompiler for this function.
        let compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
            func,
            self.isa.as_ref(),
            &mut self.jit_module,
            self.runtime.clone(),
            self.tydesc_emitter.clone(),
            None, // No registry for now - single function compilation.
        );

        // Compile and get the Cranelift FuncId.
        let cl_func_id = compiler.compile()
            .map_err(|e| JitError::CompilationFailed(format!("compile: {}", e)))?;

        // Finalize to get executable code.
        self.jit_module.finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(format!("finalize: {}", e)))?;

        // Get the code pointer.
        let code_ptr = self.jit_module.get_finalized_function(cl_func_id);

        // Determine if function uses sret.
        let sret = uses_sret(&func.return_type);

        Ok((code_ptr, sret))
    }

    /// Compile a function that may call other functions.
    ///
    /// Creates stub functions for all callees that dispatch through the trampoline.
    /// This enables mixed-mode execution where JIT code can call interpreted functions.
    ///
    /// Returns (code_ptr, uses_sret).
    pub fn compile_function_with_context<'a>(
        &mut self,
        func: &IrFunction,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
    ) -> Result<(*const u8, bool), JitError> {
        // Collect all Call targets in this function.
        let callees = self.collect_call_targets(func);

        // Create stubs for each callee.
        let mut local_funcs: HashMap<datalove_datafun_ir::FuncId, FuncId> = HashMap::new();
        let mut module_funcs: HashMap<(IrModuleId, datalove_datafun_ir::FuncId), FuncId> = HashMap::new();

        for func_ref in callees {
            // Look up the callee's IR to get its signature.
            let callee_ir = ctx.get_function(&func_ref, registry)
                .map_err(|e| JitError::CompilationFailed(format!("callee lookup: {:?}", e)))?;

            // Create a stub for this callee.
            let stub_id = self.create_stub_for_callee(&func_ref, callee_ir)?;

            // Register in appropriate map.
            match &func_ref {
                FuncRef::Local(id) => {
                    local_funcs.insert(*id, stub_id);
                }
                FuncRef::Module { module, func: fid } => {
                    module_funcs.insert((*module, *fid), stub_id);
                }
                FuncRef::External { unit, func: fid } => {
                    // External functions go in local_funcs for now.
                    // The stub handles the dispatch correctly.
                    local_funcs.insert(*fid, stub_id);
                    let _ = unit; // Silence warning; actual unit is encoded in stub.
                }
            }
        }

        // Build a FunctionCompiler with stub mappings.
        let mut compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
            func,
            self.isa.as_ref(),
            &mut self.jit_module,
            self.runtime.clone(),
            self.tydesc_emitter.clone(),
            Some(registry),
        );
        compiler.set_local_funcs(local_funcs);
        compiler.set_module_funcs(module_funcs);

        // Compile and get the Cranelift FuncId.
        let cl_func_id = compiler.compile()
            .map_err(|e| JitError::CompilationFailed(format!("compile: {}", e)))?;

        // Finalize to get executable code.
        self.jit_module.finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(format!("finalize: {}", e)))?;

        // Get the code pointer.
        let code_ptr = self.jit_module.get_finalized_function(cl_func_id);

        // Determine if function uses sret.
        let sret = uses_sret(&func.return_type);

        Ok((code_ptr, sret))
    }

    /// Collect all unique Call targets in a function.
    fn collect_call_targets(&self, func: &IrFunction) -> HashSet<FuncRef> {
        let mut targets = HashSet::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Call { func: func_ref, .. } = inst {
                    targets.insert(func_ref.clone());
                }
            }
        }

        targets
    }

    /// Create a stub function for a callee that dispatches through the trampoline.
    ///
    /// The stub has the same signature as the callee and internally calls
    /// __jit_dispatch_call with the encoded function key.
    fn create_stub_for_callee(
        &mut self,
        func_ref: &FuncRef,
        callee: &IrFunction,
    ) -> Result<FuncId, JitError> {
        // Generate unique stub name.
        static STUB_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let stub_num = STUB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stub_name = format!("__jit_stub_{}_{}", callee.name, stub_num);

        // Build signature matching the callee.
        let sig = build_signature_for_func(callee, self.isa.as_ref());

        // Declare the stub.
        let stub_id = self.jit_module
            .declare_function(&stub_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(format!("declare stub: {}", e)))?;

        // Define the stub.
        self.define_stub(stub_id, func_ref, callee, &sig)?;

        Ok(stub_id)
    }

    /// Define a stub function body that calls __jit_dispatch_call.
    fn define_stub(
        &mut self,
        stub_id: FuncId,
        func_ref: &FuncRef,
        callee: &IrFunction,
        sig: &cl_ir::Signature,
    ) -> Result<(), JitError> {
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, stub_id.as_u32()),
            sig.clone(),
        );

        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Get function parameters.
        let params: Vec<_> = builder.block_params(entry_block).to_vec();

        // rt_handle is always first param.
        let rt_handle = params[0];

        // Determine if callee uses sret.
        let callee_uses_sret = uses_sret(&callee.return_type);

        // Get sret ptr if applicable.
        let (sret_ptr, user_args_start) = if callee_uses_sret {
            (Some(params[1]), 2)
        } else {
            (None, 1)
        };

        // User arguments.
        let user_args = &params[user_args_start..];

        // Build args array on stack.
        // Allocate stack slot for args array.
        let arg_count = user_args.len();
        let args_slot = if arg_count > 0 {
            let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                (arg_count * 8) as u32,
                8,
            ));
            // Store each arg pointer in the array.
            for (i, &arg) in user_args.iter().enumerate() {
                let offset = (i * 8) as i32;
                builder.ins().stack_store(arg, slot, offset);
            }
            Some(slot)
        } else {
            None
        };

        // Get pointer to args array (or null if no args).
        let args_ptr = if let Some(slot) = args_slot {
            builder.ins().stack_addr(PTR_TYPE, slot, 0)
        } else {
            builder.ins().iconst(PTR_TYPE, 0)
        };

        // Encode function key.
        let encoded_key = EncodedFuncKey::from_func_ref(func_ref);
        let encoded_key_val = builder.ins().iconst(cl_types::I64, encoded_key.as_u64() as i64);

        // Get return destination pointer.
        // For sret: use the sret ptr.
        // For scalar: allocate stack space.
        let (ret_dest, ret_slot) = if let Some(sret) = sret_ptr {
            (sret, None)
        } else if !matches!(callee.return_type, datalove_datafun_ir::IrType::Unit) {
            // Allocate space for scalar return (8 bytes).
            let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                8,
                8,
            ));
            let ptr = builder.ins().stack_addr(PTR_TYPE, slot, 0);
            (ptr, Some(slot))
        } else {
            // Unit return - null pointer.
            (builder.ins().iconst(PTR_TYPE, 0), None)
        };

        // ret_is_sret flag.
        let ret_is_sret_val = builder.ins().iconst(cl_types::I8, if callee_uses_sret { 1 } else { 0 });

        // arg_count.
        let arg_count_val = builder.ins().iconst(cl_types::I32, arg_count as i64);

        // Call __jit_dispatch_call.
        let dispatch_ref = self.jit_module.declare_func_in_func(self.dispatch_func_id, builder.func);
        let call_args = [rt_handle, encoded_key_val, ret_dest, ret_is_sret_val, arg_count_val, args_ptr];
        let _call_inst = builder.ins().call(dispatch_ref, &call_args);

        // Return value.
        if callee_uses_sret {
            // Sret: no return value.
            builder.ins().return_(&[]);
        } else if let Some(slot) = ret_slot {
            // Scalar: load from stack and return.
            // Determine the scalar return type.
            let ret_ty = sig.returns.get(0).map(|r| r.value_type).unwrap_or(cl_types::I64);
            let ret_val = builder.ins().stack_load(ret_ty, slot, 0);
            builder.ins().return_(&[ret_val]);
        } else {
            // Unit return.
            builder.ins().return_(&[]);
        }

        builder.finalize();

        // Define the function in the module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.jit_module
            .define_function(stub_id, &mut ctx)
            .map_err(|e| JitError::CompilationFailed(format!("define stub: {}", e)))?;

        Ok(())
    }
}
