//! Runtime function imports for AOT compilation.
//!
//! Declares external runtime functions that will be linked at load time.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam};
use cranelift_codegen::isa::CallConv;
use cranelift_module::{FuncId, Linkage, Module};

use crate::types::PTR_TYPE;
use crate::AotError;

/// Imported runtime functions.
#[derive(Clone, Copy)]
pub struct RuntimeImports {
    /// `dtlv_rti_init() -> LocalRtHandle`
    pub init: FuncId,
    /// `dtlv_rti_shutdown(rt: LocalRtHandle) -> RtStatus`
    pub shutdown: FuncId,
    /// `dtlv_rti_set_debug_mode(rt: LocalRtHandle, mode: DebugOutputMode) -> RtStatus`
    pub set_debug_mode: FuncId,
    /// `dtlv_rti_debuglog_local(rt: LocalRtHandle, value_ref: *const u8, tydesc: *const TyDesc) -> RtStatus`
    pub debuglog_local: FuncId,
    /// `dtlv_rti_any_destroy_local(rt: LocalRtHandle, value: *mut u8, tydesc: *const TyDesc) -> RtStatus`
    pub destroy_local: FuncId,
    /// `dtlv_rti_mem_alloc_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: u32) -> *mut u8`
    pub mem_alloc_raw: FuncId,
    /// `dtlv_rti_string_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const TyDesc) -> RtStatus`
    pub string_create: FuncId,
    /// `dtlv_rti_string_push_bytes_local(rt: LocalRtHandle, value_mut: *mut u8, tydesc: *const TyDesc, bytes: *const u8, len: u32) -> RtStatus`
    pub string_push_bytes: FuncId,

    // Collection functions.
    /// `dtlv_rti_list_create_local(rt, value_out, tydesc) -> RtStatus`
    pub list_create: FuncId,
    /// `dtlv_rti_list_push_local(rt, list_value_mut, list_tydesc, element_in, element_tydesc) -> RtStatus`
    pub list_push: FuncId,
    /// `dtlv_rti_btreeset_create_local(rt, value_out, tydesc) -> RtStatus`
    pub set_create: FuncId,
    /// `dtlv_rti_btreeset_insert_local(rt, set_value_mut, set_tydesc, element_in, element_tydesc, bool_out) -> RtStatus`
    pub set_insert: FuncId,
    /// `dtlv_rti_btreemap_create_local(rt, value_out, tydesc) -> RtStatus`
    pub map_create: FuncId,
    /// `dtlv_rti_btreemap_insert_local(rt, map_value_mut, map_tydesc, key_in, key_tydesc, value_in, value_tydesc) -> RtStatus`
    pub map_insert: FuncId,
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

        // dtlv_rti_any_destroy_local(ptr, ptr, ptr) -> u8
        let destroy_local = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value ptr
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_any_destroy_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_any_destroy_local: {}", e)))?
        };

        // dtlv_rti_mem_alloc_raw_local(ptr, u32, u32, u32) -> ptr
        let mem_alloc_raw = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(cl_types::I32)); // size
            sig.params.push(AbiParam::new(cl_types::I32)); // align
            sig.params.push(AbiParam::new(cl_types::I32)); // count
            sig.returns.push(AbiParam::new(PTR_TYPE));     // allocated ptr
            module
                .declare_function("dtlv_rti_mem_alloc_raw_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_mem_alloc_raw_local: {}", e)))?
        };

        // dtlv_rti_string_create_local(ptr, ptr, ptr) -> u8
        let string_create = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_string_create_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_string_create_local: {}", e)))?
        };

        // dtlv_rti_string_push_bytes_local(ptr, ptr, ptr, ptr, u32) -> u8
        let string_push_bytes = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));      // value_mut
            sig.params.push(AbiParam::new(PTR_TYPE));      // tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));      // bytes ptr
            sig.params.push(AbiParam::new(cl_types::I32)); // len
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_string_push_bytes_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_string_push_bytes_local: {}", e)))?
        };

        // dtlv_rti_list_create_local(rt, value_out, tydesc) -> u8
        let list_create = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_list_create_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_list_create_local: {}", e)))?
        };

        // dtlv_rti_list_push_local(rt, list_value_mut, list_tydesc, element_in, element_tydesc) -> u8
        let list_push = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // list_value_mut
            sig.params.push(AbiParam::new(PTR_TYPE)); // list_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // element_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // element_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_list_push_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_list_push_local: {}", e)))?
        };

        // dtlv_rti_btreeset_create_local(rt, value_out, tydesc) -> u8
        let set_create = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreeset_create_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_btreeset_create_local: {}", e)))?
        };

        // dtlv_rti_btreeset_insert_local(rt, set_value_mut, set_tydesc, element_in, element_tydesc, bool_out) -> u8
        let set_insert = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // set_value_mut
            sig.params.push(AbiParam::new(PTR_TYPE)); // set_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // element_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // element_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // bool_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreeset_insert_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_btreeset_insert_local: {}", e)))?
        };

        // dtlv_rti_btreemap_create_local(rt, value_out, tydesc) -> u8
        let map_create = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_create_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_btreemap_create_local: {}", e)))?
        };

        // dtlv_rti_btreemap_insert_local(rt, map_value_mut, map_tydesc, key_in, key_tydesc, value_in, value_tydesc) -> u8
        let map_insert = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_value_mut
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_insert_local", Linkage::Import, &sig)
                .map_err(|e| AotError::Module(format!("declare dtlv_rti_btreemap_insert_local: {}", e)))?
        };

        Ok(Self {
            init,
            shutdown,
            set_debug_mode,
            debuglog_local,
            destroy_local,
            mem_alloc_raw,
            string_create,
            string_push_bytes,
            list_create,
            list_push,
            set_create,
            set_insert,
            map_create,
            map_insert,
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
