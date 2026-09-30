//! Declarations of the runtime functions a rider calls.
//!
//! Generated from `datalove-rt`'s `c.rs` by `scripts/gen-rti.py`. Do not edit;
//! `datalove-rt`'s `abi_check` module makes a declaration that disagrees with
//! its definition a compile error, and the count below makes an undeclared
//! export one.
//!
//! Nothing here is defined. A caller that links these resolves them against
//! whichever runtime it is loaded into.

use datalove_rtdt as rtdt;

use crate::{DebugOutputMode, LocalRtHandle, RtEq, RtOrdering, RtStatus};

/// How many functions the runtime exports.
pub const EXPORTED: usize = 125;

// `TyDesc` holds a union whose `nothing` member is a zero-sized struct, for
// the scalar types that need no further description, and a zero-sized type
// has no C counterpart for the lint to check against. Nothing here takes a
// `TyDesc` by value -- it is always behind a pointer -- so the member's size
// never reaches the ABI. `datalove-rt` gets the same warning on the matching
// definitions.
#[allow(improper_ctypes)]
unsafe extern "C-unwind" {
    pub safe fn dtlv_rti_init() -> LocalRtHandle;
    pub fn dtlv_rti_shutdown(rt: LocalRtHandle) -> RtStatus;
    pub fn dtlv_rti_mem_alloc_local(rt: LocalRtHandle, tydesc: *const rtdt::TyDesc, count: rtdt::IndexRepr) -> *mut u8;
    pub fn dtlv_rti_mem_alloc_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: rtdt::IndexRepr) -> *mut u8;
    pub fn dtlv_rti_mem_free_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: rtdt::IndexRepr, ptr: *mut u8) -> RtStatus;
    pub fn dtlv_rti_mem_free_local(rt: LocalRtHandle, tydesc: *const rtdt::TyDesc, count: rtdt::IndexRepr, ptr: *mut u8) -> RtStatus;
    pub fn dtlv_rti_move_value_local(rt: LocalRtHandle, src_ref: *const u8, tydesc: *const rtdt::TyDesc, dst_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_clone_local(rt: LocalRtHandle, value_in: *const u8, tydesc_in: *const rtdt::TyDesc, value_out: *mut u8, tydesc_out: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_eq_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtEq;
    pub fn dtlv_rti_eq_unique_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtEq;
    pub fn dtlv_rti_cmp_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtOrdering;
    pub fn dtlv_rti_cmp_total_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtOrdering;
    pub fn dtlv_rti_int_add(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_sub(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_mul(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_neg(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_div_checked(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_from_fixed(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_int_from_limbs(rt: LocalRtHandle, limbs_ptr: *const u32, limb_count: u32, negative: bool, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_any_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_error_from_local(rt: LocalRtHandle, inner_in: *const u8, inner_tydesc: *const rtdt::TyDesc, dest_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_data_from_local(rt: LocalRtHandle, inner_in: *const u8, inner_tydesc: *const rtdt::TyDesc, dest_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_clone_erased_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_dyn_binop(rt: LocalRtHandle, op: u8, lhs: *const u8, lhs_tydesc: *const rtdt::TyDesc, rhs: *const u8, rhs_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_dyn_binop_checked(rt: LocalRtHandle, op: u8, lhs: *const u8, lhs_tydesc: *const rtdt::TyDesc, rhs: *const u8, rhs_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc, overflow_out: *mut bool) -> RtStatus;
    pub fn dtlv_rti_dyn_unop(rt: LocalRtHandle, op: u8, value: *const u8, value_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_dyn_const(rt: LocalRtHandle, which: u8, out: *mut u8, out_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_dyn_float_const(rt: LocalRtHandle, which: u8, out: *mut u8, out_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_dyn_neg_checked(rt: LocalRtHandle, value: *const u8, value_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc, overflow_out: *mut bool) -> RtStatus;
    pub fn dtlv_rti_data_parts(data_in: *const u8, value_out: *mut *const u8, tydesc_out: *mut *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_data_borrow(data_in: *const u8, scratch: *mut u8, value_out: *mut *const u8, tydesc_out: *mut *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_data_into_local(rt: LocalRtHandle, data_in: *const u8, dest_out: *mut u8, dest_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_erase_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_reify_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_string_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_string_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_string_push_bytes_local(rt: LocalRtHandle, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc, bytes_ref: *const u8, bytes_len: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_string_clear_local(rt: LocalRtHandle, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_string_from_bytes(rt: LocalRtHandle, bytes_ptr: *const u8, bytes_len: rtdt::IndexRepr, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_pretty_print_local(rt: LocalRtHandle, arg_value_ref: *const u8, arg_tydesc_ref: *const rtdt::TyDesc, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_clone_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, slice_element_tydesc: *const rtdt::TyDesc, btreemap_value_out: *mut u8, btreemap_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_build_from_sorted_slices_local(rt: LocalRtHandle, map_out: *mut u8, key_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc, keys_ptr: *mut u8, values_ptr: *mut u8, num_entries: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_btreemap_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_insert_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_in: *mut u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_remove_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_key_at_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus;
    pub fn dtlv_rti_btreemap_value_at_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus;
    pub fn dtlv_rti_btreeset_get_at_local(rt: LocalRtHandle, set_value_ref: *const u8, set_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus;
    pub fn dtlv_rti_btreemap_get_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_get_as_data_local(rt: LocalRtHandle, map_value_ref: *const u8, map_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_push_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_element_write_local(rt: LocalRtHandle, slot_out: *mut u8, slot_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_set_value_erased_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_remove_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_pop_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_set_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_insert_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_insert_erased_local(rt: LocalRtHandle, set_value_mut: *mut u8, set_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreemap_insert_erased_local(rt: LocalRtHandle, map_value_mut: *mut u8, map_tydesc: *const rtdt::TyDesc, key_in: *mut u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_insert_data_local(rt: LocalRtHandle, map_value_mut: *mut u8, map_tydesc: *const rtdt::TyDesc, key_data_in: *const u8, value_data_in: *const u8) -> RtStatus;
    pub fn dtlv_rti_btreeset_insert_data_local(rt: LocalRtHandle, set_value_mut: *mut u8, set_tydesc: *const rtdt::TyDesc, data_in: *const u8, bool_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreemap_len_local(rt: LocalRtHandle, map_value_ref: *const u8, map_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreeset_len_local(rt: LocalRtHandle, set_value_ref: *const u8, set_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreemap_clear_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreemap_contains_key_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, result_out: *mut bool) -> RtStatus;
    pub fn dtlv_rti_btreemap_get_value_ref_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_ptr_out: *mut *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreemap_set_value_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_in: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_destroy_local(rt: LocalRtHandle, btreeset_value_in: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_insert_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreeset_remove_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreeset_contains_local(rt: LocalRtHandle, btreeset_value_ref: *const u8, btreeset_tydesc: *const rtdt::TyDesc, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_btreeset_clear_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_clone_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, slice_element_tydesc: *const rtdt::TyDesc, btreeset_value_out: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_btreeset_build_from_sorted_slice_local(rt: LocalRtHandle, set_out: *mut u8, element_tydesc: *const rtdt::TyDesc, elements_ptr: *mut u8, num_elements: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_list_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_create_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, list_value_out: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_build_from_slice_local(rt: LocalRtHandle, list_out: *mut u8, element_tydesc: *const rtdt::TyDesc, elements_ptr: *mut u8, num_elements: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_list_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_clear_local(rt: LocalRtHandle, value_mut: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_set_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, data_in: *const u8) -> RtStatus;
    pub fn dtlv_rti_list_insert_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, data_in: *const u8) -> RtStatus;
    pub fn dtlv_rti_list_remove_as_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_len_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_list_get_as_data_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_pop_as_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_push_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, data_in: *const u8) -> RtStatus;
    pub fn dtlv_rti_list_get_erased_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_get_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_set_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_push_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_pop_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_insert_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_remove_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_reserve_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, additional: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_list_shrink_to_fit_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_list_extend_from_slice_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_create_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, shape_in: *mut u8, shape_tydesc: *const rtdt::TyDesc, layout: u8, tensor_value_out: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_destroy_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_get_local(rt: LocalRtHandle, tensor_value_ref: *const u8, tensor_tydesc: *const rtdt::TyDesc, indices_ptr: *const u32, element_value_out: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_set_local(rt: LocalRtHandle, tensor_value_ref: *mut u8, tensor_tydesc: *const rtdt::TyDesc, indices_ptr: *const u32, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_transpose_local(rt: LocalRtHandle, tensor_ref: *const u8, tensor_tydesc_ref: *const rtdt::TyDesc, perm_ptr: *const u32, tensor_value_out: *mut u8, tensor_tydesc_out: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_slice_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc, ranges_ptr: *const rtdt::SliceRange, result_value_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_reshape_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc, new_shape_in: *mut u8, new_shape_tydesc: *const rtdt::TyDesc, result_value_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_init_local(rt: LocalRtHandle, element_data_in: *mut u8, element_count: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, shape_ptr: *const u32, rank: u32, tensor_value_out: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_tensor_hyperplane_clone_local(rt: LocalRtHandle, tensor_value_ref: *const u8, tensor_tydesc: *const rtdt::TyDesc, axis0_index: rtdt::IndexRepr, sub_tensor_out: *mut u8) -> RtStatus;
    pub fn dtlv_rti_table_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_table_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_table_push_row_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc, row_ref: *const u8, row_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_table_build_from_rows_local(rt: LocalRtHandle, table_out: *mut u8, table_tydesc: *const rtdt::TyDesc, rows_ptr: *mut u8, row_tydesc: *const rtdt::TyDesc, num_rows: rtdt::IndexRepr) -> RtStatus;
    pub fn dtlv_rti_table_get_local(rt: LocalRtHandle, table_ref: *const u8, table_tydesc: *const rtdt::TyDesc, row: rtdt::IndexRepr, col: u32) -> *const u8;
    pub fn dtlv_rti_table_set_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc, row: rtdt::IndexRepr, col: u32, value_ref: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_table_clear_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_table_len(rt: LocalRtHandle, table_ref: *const u8, table_tydesc: *const rtdt::TyDesc) -> rtdt::IndexRepr;
    pub fn dtlv_rti_set_debug_mode(rt: LocalRtHandle, mode: DebugOutputMode) -> RtStatus;
    pub fn dtlv_rti_debuglog_local(rt: LocalRtHandle, value_ref: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus;
    pub fn dtlv_rti_get_debug_buffer(rt: LocalRtHandle, out_ptr: *mut *const u8, out_len: *mut usize) -> RtStatus;
    pub fn dtlv_rti_clear_debug_buffer(rt: LocalRtHandle) -> RtStatus;
    pub fn dtlv_rti_field_offset(tydesc: *const rtdt::TyDesc, index: u32) -> u32;
    pub fn dtlv_rti_field_tydesc(tydesc: *const rtdt::TyDesc, index: u32) -> *const rtdt::TyDesc;
    pub fn dtlv_rti_field_read_local(rt: LocalRtHandle, dest_out: *mut u8, dest_tydesc: *const rtdt::TyDesc, base_in: *const u8, base_tydesc: *const rtdt::TyDesc, index: u32) -> RtStatus;
    pub fn dtlv_rti_element_tydesc(tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc;
    pub fn dtlv_rti_btreemap_contains_key_erased_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, result_out: *mut bool) -> RtStatus;
    pub fn dtlv_rti_btreemap_get_value_ref_erased_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_ptr_out: *mut *mut u8) -> RtStatus;
}
