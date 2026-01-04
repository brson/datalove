//! Runtime function imports for AOT compilation.
//!
//! Declares external runtime functions that will be linked at load time.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam};
use cranelift_codegen::isa::CallConv;
use cranelift_module::{FuncId, Linkage, Module};

use crate::types::PTR_TYPE;
use crate::AotError;

/// Imported runtime functions.
pub struct RuntimeImports {
    /// `dtlv_rti_init() -> LocalRtHandle`
    pub init: FuncId,
    /// `dtlv_rti_shutdown(rt: LocalRtHandle) -> RtStatus`
    pub shutdown: FuncId,
    /// `dtlv_rti_set_debug_mode(rt: LocalRtHandle, mode: DebugOutputMode) -> RtStatus`
    pub set_debug_mode: FuncId,
    /// `dtlv_rti_debuglog_local(rt: LocalRtHandle, value_ref: *const u8, tydesc: *const TyDesc) -> RtStatus`
    pub debuglog_local: FuncId,
}

impl RuntimeImports {
    /// Declare all runtime function imports in the module.
    pub fn declare<M: Module>(module: &mut M, call_conv: CallConv) -> Result<Self, AotError> {
        // dtlv_rti_init() -> ptr
        let init = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.returns.push(AbiParam::new(PTR_TYPE));
            module
                .declare_function("dtlv_rti_init", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_init: {}", e)))?
        };

        // dtlv_rti_shutdown(ptr) -> u8
        let shutdown = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_shutdown", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_shutdown: {}", e)))?
        };

        // dtlv_rti_set_debug_mode(ptr, u8) -> u8
        let set_debug_mode = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));
            sig.params.push(AbiParam::new(cl_types::I8));
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_set_debug_mode", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_set_debug_mode: {}", e)))?
        };

        // dtlv_rti_debuglog_local(ptr, ptr, ptr) -> u8
        let debuglog_local = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_debuglog_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_debuglog_local: {}", e)))?
        };

        Ok(Self {
            init,
            shutdown,
            set_debug_mode,
            debuglog_local,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::isa;
    use cranelift_codegen::settings::{self, Configurable};
    use cranelift_object::{ObjectBuilder, ObjectModule};
    use target_lexicon::Triple;

    fn create_test_module() -> (ObjectModule, CallConv) {
        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed").unwrap();
        let flags = settings::Flags::new(settings_builder);

        let isa = isa::lookup(Triple::host())
            .unwrap()
            .finish(flags)
            .unwrap();

        let call_conv = isa.default_call_conv();

        let obj_builder = ObjectBuilder::new(
            isa,
            "test",
            cranelift_module::default_libcall_names(),
        ).unwrap();

        (ObjectModule::new(obj_builder), call_conv)
    }

    #[test]
    fn test_declare_runtime_imports() {
        let (mut module, call_conv) = create_test_module();
        let result = RuntimeImports::declare(&mut module, call_conv);
        assert!(result.is_ok(), "failed to declare runtime imports: {:?}", result.err());
    }
}
