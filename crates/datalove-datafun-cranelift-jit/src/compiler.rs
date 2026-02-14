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

use datalove_datafun_ir::{CodeRef, CodeUnitId, Instruction, IrCodeUnit, IrModuleId};
use datalove_datafun_interp::{ExecutionContext, FunctionRegistry};
use datalove_datafun_cranelift::codegen::{self, build_signature_for_func, uses_sret};
use datalove_datafun_cranelift::runtime::RuntimeImports;
use datalove_datafun_cranelift::tydesc_emit::{self, TyDescEmitter};
use datalove_datafun_cranelift::types::PTR_TYPE;

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

        // Build JIT module with all required symbols registered.
        let mut jit_builder = JITBuilder::with_isa(isa.clone(), cranelift_module::default_libcall_names());

        // Register the dispatch function so JIT code can call it.
        jit_builder.symbol("__jit_dispatch_call", trampoline::dispatch_fn_ptr());

        // Register all runtime symbols so JIT code can call them.
        register_runtime_symbols(&mut jit_builder);

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
    /// Returns (code_ptr, uses_sret, code_size).
    pub fn compile_function(&mut self, func: &IrCodeUnit) -> Result<(*const u8, bool, usize), JitError> {
        // Emit TyDescs for all types in this function.
        let mut types = HashSet::new();
        tydesc_emit::collect_types_from_code_unit(func, &mut types);

        self.tydesc_emitter.emit_all(&mut self.jit_module, types)
            .map_err(|e| JitError::CompilationFailed(format!("tydesc emit: {}", e)))?;

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

        // Get the code pointer and size.
        let code_ptr = self.jit_module.get_finalized_function(cl_func_id);

        // Estimate code size from IR function (Cranelift doesn't expose compiled size directly).
        // Use a heuristic: ~10 bytes per instruction + 20 per block for control flow.
        let code_size = estimate_code_size(func);

        // Determine if function uses sret.
        let func_ctx = func.function_context()
            .expect("function must be a function code unit");
        let sret = uses_sret(&func_ctx.return_type);

        Ok((code_ptr, sret, code_size))
    }

    /// Compile a function that may call other functions.
    ///
    /// Creates stub functions for all callees that dispatch through the trampoline.
    /// This enables mixed-mode execution where JIT code can call interpreted functions.
    ///
    /// Returns (code_ptr, uses_sret, code_size).
    pub fn compile_function_with_context<'a>(
        &mut self,
        func: &IrCodeUnit,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
    ) -> Result<(*const u8, bool, usize), JitError> {
        // Collect all Call targets in this function.
        let callees = self.collect_call_targets(func);

        // Collect types from main function and all callees for TyDesc emission.
        let mut types = HashSet::new();
        tydesc_emit::collect_types_from_code_unit(func, &mut types);

        // Create stubs for each callee.
        let mut local_funcs: HashMap<CodeUnitId, FuncId> = HashMap::new();
        let mut module_funcs: HashMap<(IrModuleId, CodeUnitId), FuncId> = HashMap::new();

        for code_ref in callees {
            // Look up the callee's IR to get its signature.
            let callee_ir = ctx.get_unit(&code_ref, registry);

            // Collect types from callee for TyDesc emission.
            tydesc_emit::collect_types_from_code_unit(&callee_ir, &mut types);

            // Create a stub for this callee.
            let stub_id = self.create_stub_for_callee(&code_ref, &callee_ir)?;

            // Register in appropriate map.
            match &code_ref {
                CodeRef::Local(id) => {
                    local_funcs.insert(CodeUnitId(id.0), stub_id);
                }
                CodeRef::Module { module, id } => {
                    module_funcs.insert((*module, CodeUnitId(id.0)), stub_id);
                }
                CodeRef::External { unit, id } => {
                    // External functions go in local_funcs for now.
                    // The stub handles the dispatch correctly.
                    local_funcs.insert(CodeUnitId(id.0), stub_id);
                    let _ = unit; // Silence warning; actual unit is encoded in stub.
                }
            }
        }

        // Emit TyDescs for all collected types.
        self.tydesc_emitter.emit_all(&mut self.jit_module, types)
            .map_err(|e| JitError::CompilationFailed(format!("tydesc emit: {}", e)))?;

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

        // Get the code pointer and size.
        let code_ptr = self.jit_module.get_finalized_function(cl_func_id);

        // Estimate code size from IR function.
        let code_size = estimate_code_size(func);

        // Determine if function uses sret.
        let func_ctx = func.function_context()
            .expect("function must be a function code unit");
        let sret = uses_sret(&func_ctx.return_type);

        Ok((code_ptr, sret, code_size))
    }

    /// Collect all unique Call targets in a code unit.
    fn collect_call_targets(&self, func: &IrCodeUnit) -> HashSet<CodeRef> {
        let mut targets = HashSet::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Call { func: code_ref, .. } = inst {
                    targets.insert(code_ref.clone());
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
        code_ref: &CodeRef,
        callee: &IrCodeUnit,
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
        self.define_stub(stub_id, code_ref, callee, &sig)?;

        Ok(stub_id)
    }

    /// Define a stub function body that calls __jit_dispatch_call.
    fn define_stub(
        &mut self,
        stub_id: FuncId,
        code_ref: &CodeRef,
        callee: &IrCodeUnit,
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
        let callee_ctx = callee.function_context()
            .expect("callee must be a function code unit");
        let callee_uses_sret = uses_sret(&callee_ctx.return_type);

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
        let encoded_key = EncodedFuncKey::from_code_ref(code_ref);
        let encoded_key_val = builder.ins().iconst(cl_types::I64, encoded_key.as_u64() as i64);

        // Get return destination pointer.
        // For sret: use the sret ptr.
        // For scalar: allocate stack space.
        let (ret_dest, ret_slot) = if let Some(sret) = sret_ptr {
            (sret, None)
        } else if !matches!(callee_ctx.return_type, datalove_datafun_ir::IrType::Unit) {
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

/// Estimate code size for a code unit based on IR complexity.
///
/// Uses heuristics since Cranelift doesn't expose compiled code size directly.
/// Estimates ~10 bytes per instruction + 20 bytes per block for control flow.
fn estimate_code_size(func: &IrCodeUnit) -> usize {
    let mut instruction_count = 0;
    for block in &func.blocks {
        instruction_count += block.instructions.len();
    }
    let block_count = func.blocks.len();

    // Base estimate: 10 bytes per instruction, 20 bytes per block.
    let base = instruction_count * 10 + block_count * 20;

    // Add overhead for function prologue/epilogue (~32 bytes).
    let overhead = 32;

    // Add overhead for parameters (~8 bytes each for stack setup).
    let param_overhead = func.function_context()
        .map(|ctx| ctx.params.len() * 8)
        .unwrap_or(0);

    base + overhead + param_overhead
}

/// Register all runtime symbols with the JIT builder.
///
/// These symbols are declared as imports by RuntimeImports::declare and must be
/// registered before creating the JITModule so they can be resolved at runtime.
fn register_runtime_symbols(jit_builder: &mut JITBuilder) {
    use datalove_rt::c;

    // Core runtime functions.
    jit_builder.symbol("dtlv_rti_init", c::dtlv_rti_init as *const u8);
    jit_builder.symbol("dtlv_rti_shutdown", c::dtlv_rti_shutdown as *const u8);
    jit_builder.symbol("dtlv_rti_set_debug_mode", c::dtlv_rti_set_debug_mode as *const u8);
    jit_builder.symbol("dtlv_rti_debuglog_local", c::dtlv_rti_debuglog_local as *const u8);
    jit_builder.symbol("dtlv_rti_any_destroy_local", c::dtlv_rti_any_destroy_local as *const u8);
    jit_builder.symbol("dtlv_rti_mem_alloc_raw_local", c::dtlv_rti_mem_alloc_raw_local as *const u8);

    // String functions.
    jit_builder.symbol("dtlv_rti_string_create_local", c::dtlv_rti_string_create_local as *const u8);
    jit_builder.symbol("dtlv_rti_string_push_bytes_local", c::dtlv_rti_string_push_bytes_local as *const u8);
    jit_builder.symbol("dtlv_rti_string_from_bytes", c::dtlv_rti_string_from_bytes as *const u8);

    // Collection functions.
    jit_builder.symbol("dtlv_rti_list_create_local", c::dtlv_rti_list_create_local as *const u8);
    jit_builder.symbol("dtlv_rti_list_push_local", c::dtlv_rti_list_push_local as *const u8);
    jit_builder.symbol("dtlv_rti_list_build_from_slice_local", c::dtlv_rti_list_build_from_slice_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreeset_create_local", c::dtlv_rti_btreeset_create_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreeset_insert_local", c::dtlv_rti_btreeset_insert_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreeset_build_from_sorted_slice_local", c::dtlv_rti_btreeset_build_from_sorted_slice_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreemap_create_local", c::dtlv_rti_btreemap_create_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreemap_insert_local", c::dtlv_rti_btreemap_insert_local as *const u8);
    jit_builder.symbol("dtlv_rti_btreemap_build_from_sorted_slices_local", c::dtlv_rti_btreemap_build_from_sorted_slices_local as *const u8);
    jit_builder.symbol("dtlv_rti_tensor_init_local", c::dtlv_rti_tensor_init_local as *const u8);
    jit_builder.symbol("dtlv_rti_table_create_local", c::dtlv_rti_table_create_local as *const u8);
    jit_builder.symbol("dtlv_rti_table_push_row_local", c::dtlv_rti_table_push_row_local as *const u8);
    jit_builder.symbol("dtlv_rti_table_build_from_rows_local", c::dtlv_rti_table_build_from_rows_local as *const u8);

    // Int (bigint) arithmetic functions.
    jit_builder.symbol("dtlv_rti_int_add", c::dtlv_rti_int_add as *const u8);
    jit_builder.symbol("dtlv_rti_int_sub", c::dtlv_rti_int_sub as *const u8);
    jit_builder.symbol("dtlv_rti_int_mul", c::dtlv_rti_int_mul as *const u8);
    jit_builder.symbol("dtlv_rti_int_div_checked", c::dtlv_rti_int_div_checked as *const u8);
    jit_builder.symbol("dtlv_rti_int_neg", c::dtlv_rti_int_neg as *const u8);
    jit_builder.symbol("dtlv_rti_int_from_fixed", c::dtlv_rti_int_from_fixed as *const u8);
    jit_builder.symbol("dtlv_rti_int_from_limbs", c::dtlv_rti_int_from_limbs as *const u8);
    jit_builder.symbol("dtlv_rti_cmp_local", c::dtlv_rti_cmp_local as *const u8);

    // Value move and clone functions.
    jit_builder.symbol("dtlv_rti_move_value_local", c::dtlv_rti_move_value_local as *const u8);
    jit_builder.symbol("dtlv_rti_clone_local", c::dtlv_rti_clone_local as *const u8);

    // Boxing functions.
    jit_builder.symbol("dtlv_rti_error_from_local", c::dtlv_rti_error_from_local as *const u8);
    jit_builder.symbol("dtlv_rti_data_from_local", c::dtlv_rti_data_from_local as *const u8);
}
