//! JIT compiler wrapping Cranelift's JITModule.

use std::sync::Arc;

use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use target_lexicon::Triple;

use datalove_datafun_ir::IrFunction;
use datalove_datafun_aot_cranelift::codegen::{self, uses_sret};
use datalove_datafun_aot_cranelift::runtime::RuntimeImports;
use datalove_datafun_aot_cranelift::tydesc_emit::TyDescEmitter;

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
}

impl JitCompiler {
    /// Create a new JIT compiler for the host target.
    pub fn new() -> Result<Self, JitError> {
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

        // Build JIT module.
        let jit_builder = JITBuilder::with_isa(isa.clone(), cranelift_module::default_libcall_names());
        let mut jit_module = JITModule::new(jit_builder);

        // Declare runtime imports.
        let call_conv = isa.default_call_conv();
        let runtime = RuntimeImports::declare(&mut jit_module, call_conv)
            .map_err(|e| JitError::CompilationFailed(format!("runtime imports: {}", e)))?;

        let tydesc_emitter = TyDescEmitter::new();

        Ok(Self {
            jit_module,
            isa,
            runtime,
            tydesc_emitter,
        })
    }

    /// Compile a function to native code.
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
}
