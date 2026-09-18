//! Runtime function imports for Cranelift compilation.
//!
//! Declares external runtime functions that will be linked at load time.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam};
use cranelift_codegen::isa::CallConv;
use cranelift_module::{FuncId, Linkage, Module};

use crate::types::PTR_TYPE;
use crate::CraneliftError;

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
    /// `dtlv_rti_field_offset(tydesc: *const TyDesc, index: u32) -> u32`
    pub field_offset: FuncId,
    /// `dtlv_rti_field_tydesc(tydesc: *const TyDesc, index: u32) -> *const TyDesc`
    pub field_tydesc: FuncId,
    /// `dtlv_rti_field_read_local(rt, dest, dest_td, base, base_td, index) -> RtStatus`
    pub field_read: FuncId,
    /// `dtlv_rti_element_tydesc(tydesc: *const TyDesc) -> *const TyDesc`
    pub element_tydesc: FuncId,
    /// `dtlv_rti_btreemap_contains_key_erased_local(rt, map, map_td, key, key_td, out) -> RtStatus`
    pub map_contains_key_erased: FuncId,
    /// `dtlv_rti_btreemap_get_value_ref_erased_local(rt, map, map_td, key, key_td, out) -> RtStatus`
    pub map_get_value_ref_erased: FuncId,
    /// `dtlv_rti_element_write_local(rt, slot, slot_td, value, value_td) -> RtStatus`
    pub element_write: FuncId,
    /// `dtlv_rti_btreemap_set_value_erased_local(rt, map, map_td, key, key_td, value, value_td)`
    pub map_set_value_erased: FuncId,
    /// `dtlv_rti_mem_alloc_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: u32) -> *mut u8`
    pub mem_alloc_raw: FuncId,
    /// `dtlv_rti_string_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const TyDesc) -> RtStatus`
    pub string_create: FuncId,
    /// `dtlv_rti_string_push_bytes_local(rt: LocalRtHandle, value_mut: *mut u8, tydesc: *const TyDesc, bytes: *const u8, len: u32) -> RtStatus`
    pub string_push_bytes: FuncId,
    /// `dtlv_rti_string_from_bytes(rt: LocalRtHandle, bytes: *const u8, len: u32, result_out: *mut u8, tydesc: *const TyDesc) -> RtStatus`
    pub string_from_bytes: FuncId,

    // Collection functions.
    /// `dtlv_rti_list_create_local(rt, value_out, tydesc) -> RtStatus`
    pub list_create: FuncId,
    /// `dtlv_rti_list_push_local(rt, list_value_mut, list_tydesc, element_in, element_tydesc) -> RtStatus`
    pub list_push: FuncId,
    /// `dtlv_rti_list_build_from_slice_local(rt, list_out, element_tydesc, elements_ptr, num_elements) -> RtStatus`
    pub list_build_from_slice: FuncId,
    /// `dtlv_rti_btreeset_create_local(rt, value_out, tydesc) -> RtStatus`
    pub set_create: FuncId,
    /// `dtlv_rti_btreeset_insert_local(rt, set_value_mut, set_tydesc, element_in, element_tydesc, bool_out) -> RtStatus`
    pub set_insert: FuncId,
    /// `dtlv_rti_btreeset_build_from_sorted_slice_local(rt, set_out, element_tydesc, elements_ptr, num_elements) -> RtStatus`
    pub set_build_from_sorted: FuncId,
    /// `dtlv_rti_btreemap_create_local(rt, value_out, tydesc) -> RtStatus`
    pub map_create: FuncId,
    /// `dtlv_rti_list_push_erased_local(rt, list, list_td, element_in, element_td) -> RtStatus`
    pub list_push_erased: FuncId,
    /// `dtlv_rti_btreeset_insert_erased_local(rt, set, set_td, element_in, element_td, bool_out) -> RtStatus`
    pub set_insert_erased: FuncId,
    /// `dtlv_rti_btreemap_insert_erased_local(rt, map, map_td, key_in, key_td, value_in, value_td) -> RtStatus`
    pub map_insert_erased: FuncId,
    /// `dtlv_rti_btreemap_insert_local(rt, map_value_mut, map_tydesc, key_in, key_tydesc, value_in, value_tydesc) -> RtStatus`
    pub map_insert: FuncId,
    /// `dtlv_rti_btreemap_build_from_sorted_slices_local(rt, map_out, key_tydesc, value_tydesc, keys_ptr, values_ptr, num_entries) -> RtStatus`
    pub map_build_from_sorted: FuncId,
    /// `dtlv_rti_tensor_init_local(rt, element_data_in, element_count, element_tydesc, shape_ptr, rank, tensor_out, tensor_tydesc) -> RtStatus`
    pub tensor_init: FuncId,
    /// `dtlv_rti_tensor_hyperplane_clone_local(rt, tensor_ref, tensor_tydesc, axis0_index, sub_tensor_out) -> RtStatus`
    pub tensor_hyperplane_clone: FuncId,
    /// `dtlv_rti_table_create_local(rt, value_out, tydesc) -> RtStatus`
    pub table_create: FuncId,
    /// `dtlv_rti_table_push_row_local(rt, table_mut, table_tydesc, row_ref, row_tydesc) -> RtStatus`
    pub table_push_row: FuncId,
    /// `dtlv_rti_table_build_from_rows_local(rt, table_out, table_tydesc, rows_ptr, row_tydesc, num_rows) -> RtStatus`
    pub table_build_from_rows: FuncId,

    // Int (bigint) arithmetic functions.
    /// `dtlv_rti_int_add(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_add: FuncId,
    /// `dtlv_rti_int_sub(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_sub: FuncId,
    /// `dtlv_rti_int_mul(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_mul: FuncId,
    /// `dtlv_rti_int_div_checked(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_div: FuncId,
    /// `dtlv_rti_int_neg(rt, a_in, a_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_neg: FuncId,
    /// `dtlv_rti_int_from_fixed(rt, src_in, src_tydesc, result_out, result_tydesc) -> RtStatus`
    pub int_from_fixed: FuncId,
    /// `dtlv_rti_int_from_limbs(rt, limbs_ptr, limb_count, negative, result_out, result_tydesc) -> RtStatus`
    pub int_from_limbs: FuncId,
    /// `dtlv_rti_cmp_local(rt, a_ref, a_tydesc, b_ref, b_tydesc) -> RtOrdering`
    pub int_cmp: FuncId,

    // Value move function.
    /// `dtlv_rti_move_value_local(rt, src_ref, tydesc, dst_out) -> RtStatus`
    pub move_value: FuncId,

    // Clone function.
    /// `dtlv_rti_clone_local(rt, src_ref, src_tydesc, dst_out, dst_tydesc) -> RtStatus`
    pub clone_local: FuncId,

    // Map indexing functions.
    /// `dtlv_rti_btreemap_contains_key_local(rt, map_ref, map_tydesc, key_ref, key_tydesc, result_out) -> RtStatus`
    pub map_contains_key: FuncId,
    /// `dtlv_rti_btreemap_get_value_ref_local(rt, map_ref, map_tydesc, key_ref, key_tydesc, value_ptr_out) -> RtStatus`
    pub map_get_value_ref: FuncId,
    /// `dtlv_rti_btreemap_set_value_local(rt, map_mut, map_tydesc, key_ref, key_tydesc, value_in, value_tydesc) -> RtStatus`
    pub map_set_value: FuncId,

    // Boxing functions.
    /// `dtlv_rti_error_from_local(rt, inner_in, inner_tydesc, dest_out) -> RtStatus`
    pub error_from: FuncId,
    /// `dtlv_rti_data_from_local(rt, inner_in, inner_tydesc, dest_out) -> RtStatus`
    pub data_from: FuncId,
    /// `dtlv_rti_erase_local(rt, src_in, src_tydesc, dst_out, dst_tydesc) -> RtStatus`
    pub erase: FuncId,
    /// `dtlv_rti_reify_local(rt, src_in, src_tydesc, dst_out, dst_tydesc) -> RtStatus`
    pub reify: FuncId,
    /// `dtlv_rti_data_parts(data_in, value_out, tydesc_out) -> RtStatus`
    pub data_parts: FuncId,
    /// `dtlv_rti_data_borrow(data_in, scratch, value_out, tydesc_out) -> RtStatus`
    pub data_borrow: FuncId,
    /// `dtlv_rti_data_from_local(rt, inner, inner_tydesc, dest) -> RtStatus`
    pub data_from_local: FuncId,
    /// `dtlv_rti_dyn_binop(rt, op, lhs, lhs_td, rhs, rhs_td, out, out_td)`
    pub dyn_binop: FuncId,
    /// `dtlv_rti_dyn_binop_checked(rt, op, lhs, lhs_td, rhs, rhs_td, out, out_td, overflow)`
    pub dyn_binop_checked: FuncId,
    /// `dtlv_rti_dyn_neg_checked(rt, value, value_td, out, out_td, overflow)`
    pub dyn_neg_checked: FuncId,
    /// `dtlv_rti_list_get_erased_local(rt, list, list_td, index, opt_out, opt_td) -> RtStatus`
    pub list_get_erased: FuncId,
    /// `dtlv_rti_clone_erased_local(rt, src, src_td, dst, dst_td) -> RtStatus`
    pub clone_erased: FuncId,
}

impl RuntimeImports {
    /// Declare all runtime function imports in the module.
    pub fn declare<M: Module>(module: &mut M, call_conv: CallConv) -> Result<Self, CraneliftError> {
        // dtlv_rti_init() -> ptr
        let init = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.returns.push(AbiParam::new(PTR_TYPE));
            module
                .declare_function("dtlv_rti_init", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_init: {}", e)))?
        };

        // dtlv_rti_shutdown(ptr) -> u8
        let shutdown = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_shutdown", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_shutdown: {}", e)))?
        };

        // dtlv_rti_set_debug_mode(ptr, u8) -> u8
        let set_debug_mode = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));
            sig.params.push(AbiParam::new(cl_types::I8));
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_set_debug_mode", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_set_debug_mode: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_debuglog_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_any_destroy_local: {}", e)))?
        };

        // dtlv_rti_mem_alloc_raw_local(ptr, u32, u32, IndexRepr) -> ptr
        // Count type is IndexRepr which depends on index-64 feature.
        use crate::index_types::INDEX_TYPE;

        let mem_alloc_raw = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(cl_types::I32)); // size
            sig.params.push(AbiParam::new(cl_types::I32)); // align
            sig.params.push(AbiParam::new(INDEX_TYPE));    // count (IndexRepr)
            sig.returns.push(AbiParam::new(PTR_TYPE));     // allocated ptr
            module
                .declare_function("dtlv_rti_mem_alloc_raw_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_mem_alloc_raw_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_string_create_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_string_push_bytes_local: {}", e)))?
        };

        // dtlv_rti_string_from_bytes(ptr, ptr, u32, ptr, ptr) -> u8
        let string_from_bytes = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));      // bytes ptr
            sig.params.push(AbiParam::new(cl_types::I32)); // len
            sig.params.push(AbiParam::new(PTR_TYPE));      // result_out
            sig.params.push(AbiParam::new(PTR_TYPE));      // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_string_from_bytes", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_string_from_bytes: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_list_create_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_list_push_local: {}", e)))?
        };

        // The same three again, taking an element that may have arrived in the
        // erased shape. See `dtlv_rti_list_push_erased_local`.
        let list_push_erased = {
            let mut sig = cl_ir::Signature::new(call_conv);
            for _ in 0..5 {
                sig.params.push(AbiParam::new(PTR_TYPE));
            }
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_list_push_erased_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_list_push_erased_local: {}", e)))?
        };
        let set_insert_erased = {
            let mut sig = cl_ir::Signature::new(call_conv);
            for _ in 0..6 {
                sig.params.push(AbiParam::new(PTR_TYPE));
            }
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreeset_insert_erased_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_btreeset_insert_erased_local: {}", e)))?
        };
        let map_insert_erased = {
            let mut sig = cl_ir::Signature::new(call_conv);
            for _ in 0..7 {
                sig.params.push(AbiParam::new(PTR_TYPE));
            }
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_insert_erased_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_btreemap_insert_erased_local: {}", e)))?
        };

        // dtlv_rti_list_build_from_slice_local(rt, list_out, element_tydesc, elements_ptr, num_elements) -> u8
        let list_build_from_slice = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));    // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));    // list_out
            sig.params.push(AbiParam::new(PTR_TYPE));    // element_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));    // elements_ptr
            sig.params.push(AbiParam::new(INDEX_TYPE));  // num_elements (IndexRepr)
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_list_build_from_slice_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_list_build_from_slice_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreeset_create_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreeset_insert_local: {}", e)))?
        };

        // dtlv_rti_btreeset_build_from_sorted_slice_local(rt, set_out, element_tydesc, elements_ptr, num_elements) -> u8
        let set_build_from_sorted = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));    // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));    // set_out
            sig.params.push(AbiParam::new(PTR_TYPE));    // element_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));    // elements_ptr
            sig.params.push(AbiParam::new(INDEX_TYPE));  // num_elements (IndexRepr)
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreeset_build_from_sorted_slice_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreeset_build_from_sorted_slice_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_create_local: {}", e)))?
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
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_insert_local: {}", e)))?
        };

        // dtlv_rti_btreemap_build_from_sorted_slices_local(rt, map_out, key_tydesc, value_tydesc, keys_ptr, values_ptr, num_entries) -> u8
        let map_build_from_sorted = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));    // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));    // map_out
            sig.params.push(AbiParam::new(PTR_TYPE));    // key_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));    // value_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));    // keys_ptr
            sig.params.push(AbiParam::new(PTR_TYPE));    // values_ptr
            sig.params.push(AbiParam::new(INDEX_TYPE));  // num_entries (IndexRepr)
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_build_from_sorted_slices_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_build_from_sorted_slices_local: {}", e)))?
        };

        // dtlv_rti_btreemap_contains_key_local(rt, map_ref, map_tydesc, key_ref, key_tydesc, result_out) -> u8
        let map_contains_key = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // result_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_contains_key_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_contains_key_local: {}", e)))?
        };

        // dtlv_rti_btreemap_get_value_ref_local(rt, map_ref, map_tydesc, key_ref, key_tydesc, value_ptr_out) -> u8
        let map_get_value_ref = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_ptr_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_get_value_ref_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_get_value_ref_local: {}", e)))?
        };

        // dtlv_rti_btreemap_set_value_local(rt, map_mut, map_tydesc, key_ref, key_tydesc, value_in, value_tydesc) -> u8
        let map_set_value = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_mut
            sig.params.push(AbiParam::new(PTR_TYPE)); // map_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // key_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_set_value_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_btreemap_set_value_local: {}", e)))?
        };

        // dtlv_rti_tensor_init_local(rt, element_data_in, element_count, element_tydesc, shape_ptr, rank, tensor_out, tensor_tydesc) -> u8
        let tensor_init = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));      // element_data_in
            sig.params.push(AbiParam::new(INDEX_TYPE));    // element_count (IndexRepr)
            sig.params.push(AbiParam::new(PTR_TYPE));      // element_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));      // shape_ptr
            sig.params.push(AbiParam::new(cl_types::I32)); // rank
            sig.params.push(AbiParam::new(PTR_TYPE));      // tensor_out
            sig.params.push(AbiParam::new(PTR_TYPE));      // tensor_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_tensor_init_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_tensor_init_local: {}", e)))?
        };

        // dtlv_rti_tensor_hyperplane_clone_local(rt, tensor_ref, tensor_tydesc, axis0_index, sub_tensor_out) -> u8
        let tensor_hyperplane_clone = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));   // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));   // tensor_ref
            sig.params.push(AbiParam::new(PTR_TYPE));   // tensor_tydesc
            sig.params.push(AbiParam::new(INDEX_TYPE)); // axis0_index
            sig.params.push(AbiParam::new(PTR_TYPE));   // sub_tensor_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_tensor_hyperplane_clone_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_tensor_hyperplane_clone_local: {}", e)))?
        };

        // dtlv_rti_table_create_local(rt, value_out, tydesc) -> u8
        let table_create = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_table_create_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_table_create_local: {}", e)))?
        };

        // dtlv_rti_table_push_row_local(rt, table_mut, table_tydesc, row_ref, row_tydesc) -> u8
        let table_push_row = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // table_mut
            sig.params.push(AbiParam::new(PTR_TYPE)); // table_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // row_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // row_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_table_push_row_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_table_push_row_local: {}", e)))?
        };

        // dtlv_rti_table_build_from_rows_local(rt, table_out, table_tydesc, rows_ptr, row_tydesc, num_rows) -> u8
        let table_build_from_rows = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));    // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));    // table_out
            sig.params.push(AbiParam::new(PTR_TYPE));    // table_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));    // rows_ptr
            sig.params.push(AbiParam::new(PTR_TYPE));    // row_tydesc
            sig.params.push(AbiParam::new(INDEX_TYPE));  // num_rows (IndexRepr)
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_table_build_from_rows_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_table_build_from_rows_local: {}", e)))?
        };

        // Int (bigint) binary operations: (rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> u8
        let int_binop_sig = || {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // b_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // b_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // result_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // result_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        };

        let int_add = module
            .declare_function("dtlv_rti_int_add", Linkage::Import, &int_binop_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_add: {}", e)))?;

        let int_sub = module
            .declare_function("dtlv_rti_int_sub", Linkage::Import, &int_binop_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_sub: {}", e)))?;

        let int_mul = module
            .declare_function("dtlv_rti_int_mul", Linkage::Import, &int_binop_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_mul: {}", e)))?;

        let int_div = module
            .declare_function("dtlv_rti_int_div_checked", Linkage::Import, &int_binop_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_div_checked: {}", e)))?;

        // Int (bigint) unary operation: (rt, a_in, a_tydesc, result_out, result_tydesc) -> u8
        let int_unary_sig = || {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // result_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // result_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        };

        let int_neg = module
            .declare_function("dtlv_rti_int_neg", Linkage::Import, &int_unary_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_neg: {}", e)))?;

        let int_from_fixed = module
            .declare_function("dtlv_rti_int_from_fixed", Linkage::Import, &int_unary_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_from_fixed: {}", e)))?;

        // dtlv_rti_int_from_limbs(rt, limbs_ptr, limb_count, negative, result_out, result_tydesc) -> u8
        let int_from_limbs = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE));      // limbs_ptr
            sig.params.push(AbiParam::new(cl_types::I32)); // limb_count
            sig.params.push(AbiParam::new(cl_types::I8));  // negative (bool)
            sig.params.push(AbiParam::new(PTR_TYPE));      // result_out
            sig.params.push(AbiParam::new(PTR_TYPE));      // result_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_int_from_limbs", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_int_from_limbs: {}", e)))?
        };

        // Int (bigint) comparison: (rt, a_ref, a_tydesc, b_ref, b_tydesc) -> RtOrdering (u8)
        let int_cmp = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // a_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // b_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // b_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8)); // RtOrdering
            module
                .declare_function("dtlv_rti_cmp_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_cmp_local: {}", e)))?
        };

        // dtlv_rti_move_value_local(ptr, ptr, ptr, ptr) -> u8
        let move_value = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // src_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // dst_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_move_value_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_move_value_local: {}", e)))?
        };

        // dtlv_rti_clone_local(ptr, ptr, ptr, ptr, ptr) -> u8
        let clone_local = {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // src_ref
            sig.params.push(AbiParam::new(PTR_TYPE)); // src_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // dst_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // dst_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_clone_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_clone_local: {}", e)))?
        };

        // Boxing functions: error_from, data_from
        // Signature: (rt, inner_in, inner_tydesc, dest_out) -> u8
        let boxing_sig = || {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // inner_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // inner_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // dest_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        };

        let error_from = module
            .declare_function("dtlv_rti_error_from_local", Linkage::Import, &boxing_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_error_from_local: {}", e)))?;

        let data_from = module
            .declare_function("dtlv_rti_data_from_local", Linkage::Import, &boxing_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_data_from_local: {}", e)))?;

        // Erasure conversions: five pointers in, a status out. Both tydescs
        // are needed because the two shapes differ only where one has `data`.
        let erasure_sig = || {
            let mut sig = cl_ir::Signature::new(call_conv);
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt handle
            sig.params.push(AbiParam::new(PTR_TYPE)); // src_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // src_tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // dst_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // dst_tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        };

        let erase = module
            .declare_function("dtlv_rti_erase_local", Linkage::Import, &erasure_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_erase_local: {}", e)))?;

        let reify = module
            .declare_function("dtlv_rti_reify_local", Linkage::Import, &erasure_sig())
            .map_err(|e| CraneliftError::Module(format!("declare dtlv_rti_reify_local: {}", e)))?;

        // Borrowing what a wrapper holds takes no runtime handle: it reads two
        // words out of the wrapper and lends them, allocating nothing.
        let data_parts = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE)); // data_in
            sig.params.push(AbiParam::new(PTR_TYPE)); // value_out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc_out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_data_parts", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_data_parts: {}", e)))?
        };

        // Moving a freshly built collection into the wrapper that carries its
        // descriptor with it.
        let data_borrow = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE)); // data in
            sig.params.push(AbiParam::new(PTR_TYPE)); // scratch
            sig.params.push(AbiParam::new(PTR_TYPE)); // value out
            sig.params.push(AbiParam::new(PTR_TYPE)); // tydesc out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module.declare_function("dtlv_rti_data_borrow", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_data_borrow: {}", e)))?
        };

        let data_from_local = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE)); // rt
            sig.params.push(AbiParam::new(PTR_TYPE)); // inner
            sig.params.push(AbiParam::new(PTR_TYPE)); // inner tydesc
            sig.params.push(AbiParam::new(PTR_TYPE)); // dest
            sig.returns.push(AbiParam::new(cl_types::I8));
            module.declare_function("dtlv_rti_data_from_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_data_from_local: {}", e)))?
        };

        // An operator on a bounded type parameter, whose type only the
        // descriptor says.
        fn dyn_binop_sig(mut sig: cl_ir::Signature, checked: bool) -> cl_ir::Signature {
            sig.params.push(AbiParam::new(PTR_TYPE));       // rt
            sig.params.push(AbiParam::new(cl_types::I8));   // op
            sig.params.push(AbiParam::new(PTR_TYPE));       // lhs
            sig.params.push(AbiParam::new(PTR_TYPE));       // lhs tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));       // rhs
            sig.params.push(AbiParam::new(PTR_TYPE));       // rhs tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));       // out
            sig.params.push(AbiParam::new(PTR_TYPE));       // out tydesc
            if checked {
                sig.params.push(AbiParam::new(PTR_TYPE));   // overflow out
            }
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        }
        let dyn_binop = {
            let sig = dyn_binop_sig(module.make_signature(), false);
            module.declare_function("dtlv_rti_dyn_binop", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_dyn_binop: {}", e)))?
        };
        let dyn_binop_checked = {
            let sig = dyn_binop_sig(module.make_signature(), true);
            module.declare_function("dtlv_rti_dyn_binop_checked", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_dyn_binop_checked: {}", e)))?
        };
        let dyn_neg_checked = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));       // rt
            sig.params.push(AbiParam::new(PTR_TYPE));       // value
            sig.params.push(AbiParam::new(PTR_TYPE));       // value tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));       // out
            sig.params.push(AbiParam::new(PTR_TYPE));       // out tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));       // overflow out
            sig.returns.push(AbiParam::new(cl_types::I8));
            module.declare_function("dtlv_rti_dyn_neg_checked", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_dyn_neg_checked: {}", e)))?
        };

        // Indexing a list whose elements a generic cannot name. The runtime
        // decides whether the element wants packing, since a list of `data`
        // does not.
        let list_get_erased = {
            use crate::index_types::INDEX_TYPE;
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt
            sig.params.push(AbiParam::new(PTR_TYPE));      // list
            sig.params.push(AbiParam::new(PTR_TYPE));      // list tydesc
            sig.params.push(AbiParam::new(INDEX_TYPE));    // index
            sig.params.push(AbiParam::new(PTR_TYPE));      // option out
            sig.params.push(AbiParam::new(PTR_TYPE));      // option tydesc
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_list_get_erased_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_list_get_erased_local: {}", e)))?
        };

        // Cloning into a destination that may be the erased shape. The
        // runtime decides whether the clone wants wrapping, since only the
        // descriptors say and a call site has one of them statically.
        let clone_erased = module
            .declare_function("dtlv_rti_clone_erased_local", Linkage::Import, &erasure_sig())
            .map_err(|e| CraneliftError::Module(
                format!("declare dtlv_rti_clone_erased_local: {}", e)))?;

        // Where a field of a borrowed generic aggregate is, and what it is.
        // The offsets in a descriptor are the only truthful ones inside a
        // generic, where the static type says `data` at a type parameter and a
        // `data` is a different width from what stands behind it.
        let field_offset = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // tydesc
            sig.params.push(AbiParam::new(cl_types::I32)); // index
            sig.returns.push(AbiParam::new(cl_types::I32));
            module
                .declare_function("dtlv_rti_field_offset", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_field_offset: {}", e)))?
        };
        let field_tydesc = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // tydesc
            sig.params.push(AbiParam::new(cl_types::I32)); // index
            sig.returns.push(AbiParam::new(PTR_TYPE));
            module
                .declare_function("dtlv_rti_field_tydesc", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_field_tydesc: {}", e)))?
        };

        // Reading such a field out into a value. Whether what is found wants
        // packing on the way is the runtime's to decide, for the reason
        // `list_get_erased` gives.
        // What a container holds, for a reference reaching into one inside a
        // generic, where the static element type is a `data` of the wrong width.
        let element_tydesc = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // tydesc
            sig.returns.push(AbiParam::new(PTR_TYPE));
            module
                .declare_function("dtlv_rti_element_tydesc", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_element_tydesc: {}", e)))?
        };

        // The lookup half of a map inside a generic: the map's descriptor is
        // the truthful one and the key may have arrived packed into a `data`,
        // which the runtime unpacks so that the four backends do not each
        // decide whether it is packed.
        let map_lookup_sig = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt
            sig.params.push(AbiParam::new(PTR_TYPE));      // map
            sig.params.push(AbiParam::new(PTR_TYPE));      // map tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));      // key
            sig.params.push(AbiParam::new(PTR_TYPE));      // key tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));      // out
            sig.returns.push(AbiParam::new(cl_types::I8));
            sig
        };
        let map_contains_key_erased = module
            .declare_function("dtlv_rti_btreemap_contains_key_erased_local",
                Linkage::Import, &map_lookup_sig)
            .map_err(|e| CraneliftError::Module(
                format!("declare dtlv_rti_btreemap_contains_key_erased_local: {}", e)))?;
        let map_get_value_ref_erased = module
            .declare_function("dtlv_rti_btreemap_get_value_ref_erased_local",
                Linkage::Import, &map_lookup_sig)
            .map_err(|e| CraneliftError::Module(
                format!("declare dtlv_rti_btreemap_get_value_ref_erased_local: {}", e)))?;

        // Writing a value into a slot whose type the caller names, whatever
        // shape the value arrived in.
        let element_write = module
            .declare_function("dtlv_rti_element_write_local", Linkage::Import, &erasure_sig())
            .map_err(|e| CraneliftError::Module(
                format!("declare dtlv_rti_element_write_local: {}", e)))?;

        let map_set_value_erased = {
            let mut sig = module.make_signature();
            for _ in 0..7 {
                sig.params.push(AbiParam::new(PTR_TYPE));
            }
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_btreemap_set_value_erased_local",
                    Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_btreemap_set_value_erased_local: {}", e)))?
        };

        let field_read = {
            let mut sig = module.make_signature();
            sig.params.push(AbiParam::new(PTR_TYPE));      // rt
            sig.params.push(AbiParam::new(PTR_TYPE));      // dest
            sig.params.push(AbiParam::new(PTR_TYPE));      // dest tydesc
            sig.params.push(AbiParam::new(PTR_TYPE));      // base
            sig.params.push(AbiParam::new(PTR_TYPE));      // base tydesc
            sig.params.push(AbiParam::new(cl_types::I32)); // index
            sig.returns.push(AbiParam::new(cl_types::I8));
            module
                .declare_function("dtlv_rti_field_read_local", Linkage::Import, &sig)
                .map_err(|e| CraneliftError::Module(
                    format!("declare dtlv_rti_field_read_local: {}", e)))?
        };

        Ok(Self {
            init,
            shutdown,
            set_debug_mode,
            debuglog_local,
            destroy_local,
            field_offset,
            field_tydesc,
            field_read,
            element_tydesc,
            map_contains_key_erased,
            map_get_value_ref_erased,
            element_write,
            map_set_value_erased,
            mem_alloc_raw,
            string_create,
            string_push_bytes,
            string_from_bytes,
            list_create,
            list_push,
            list_push_erased,
            set_insert_erased,
            map_insert_erased,
            list_build_from_slice,
            set_create,
            set_insert,
            set_build_from_sorted,
            map_create,
            map_insert,
            map_build_from_sorted,
            map_contains_key,
            map_get_value_ref,
            map_set_value,
            tensor_init,
            tensor_hyperplane_clone,
            table_create,
            table_push_row,
            table_build_from_rows,
            int_add,
            int_sub,
            int_mul,
            int_div,
            int_neg,
            int_from_fixed,
            int_from_limbs,
            int_cmp,
            move_value,
            clone_local,
            error_from,
            data_from,
            erase,
            data_parts,
            data_borrow,
            data_from_local,
            dyn_binop,
            dyn_binop_checked,
            dyn_neg_checked,
            list_get_erased,
            clone_erased,
            reify,
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
