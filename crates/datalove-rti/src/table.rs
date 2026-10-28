//! The table of runtime functions a rider calls.
//!
//! Generated from `datalove-rt`'s `c/mod.rs` by `scripts/gen-rti.py`. Do not edit.
//!
//! A rider does not link the runtime, so it cannot call these by name. It
//! gets them from the handle it is passed, which carries a pointer to one of
//! these as its first word. [`call`](crate::call) is the readable way to do
//! that; this is the shape being pointed at.
//!
//! The field order is the ABI. A runtime and a rider built from different
//! revisions of this file would disagree about which function is which, so
//! `EXPORTED` is checked where the table is filled in.

use datalove_rtdt as rtdt;

use crate::{DebugOutputMode, LocalRtHandle, RtEq, RtOrdering, RtStatus};

/// How many functions the runtime exports.
pub const EXPORTED: usize = 122;

/// A hash of every field's name and type, in order.
///
/// Part of [`ABI_VERSION`](crate::ABI_VERSION), which is what a rider and a
/// runtime are checked against each other on. Anything that changes what a
/// caller must pass changes this, order included: a field is reached by
/// offset, so moving one breaks a caller as surely as changing its arguments.
pub const TABLE_SHAPE: u64 = 0x4d3a6a8c3651bc18;

/// Every function the runtime exports, by pointer.
#[repr(C)]
pub struct RtiTable {
    pub dtlv_rti_init: extern "C-unwind" fn() -> LocalRtHandle,
    pub dtlv_rti_reset: unsafe extern "C-unwind" fn(LocalRtHandle) -> RtStatus,
    pub dtlv_rti_shutdown: unsafe extern "C-unwind" fn(LocalRtHandle) -> RtStatus,
    pub dtlv_rti_mem_alloc_local: unsafe extern "C-unwind" fn(LocalRtHandle, &rtdt::TyDesc, rtdt::IndexRepr) -> *mut u8,
    pub dtlv_rti_mem_alloc_raw_local: unsafe extern "C-unwind" fn(LocalRtHandle, u32, u32, rtdt::IndexRepr) -> *mut u8,
    pub dtlv_rti_mem_free_raw_local: unsafe extern "C-unwind" fn(LocalRtHandle, u32, u32, rtdt::IndexRepr, *mut u8) -> RtStatus,
    pub dtlv_rti_mem_free_local: unsafe extern "C-unwind" fn(LocalRtHandle, &rtdt::TyDesc, rtdt::IndexRepr, *mut u8) -> RtStatus,
    pub dtlv_rti_move_value_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_clone_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_eq_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtEq,
    pub dtlv_rti_eq_unique_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtEq,
    pub dtlv_rti_cmp_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtOrdering,
    pub dtlv_rti_cmp_total_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtOrdering,
    pub dtlv_rti_int_add: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_sub: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_mul: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_neg: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_div_checked: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_add_assign: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_sub_assign: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_mul_assign: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_div_assign_checked: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_from_fixed: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_int_from_limbs: unsafe extern "C-unwind" fn(LocalRtHandle, *const u32, u32, bool, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_any_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_error_from_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_data_from_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_dyn_binop: unsafe extern "C-unwind" fn(LocalRtHandle, u8, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_dyn_binop_checked: unsafe extern "C-unwind" fn(LocalRtHandle, u8, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc, *mut bool) -> RtStatus,
    pub dtlv_rti_dyn_unop: unsafe extern "C-unwind" fn(LocalRtHandle, u8, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_dyn_const: unsafe extern "C-unwind" fn(LocalRtHandle, u8, rtdt::AnyOut<'_>, &rtdt::TyDesc, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_dyn_float_const: unsafe extern "C-unwind" fn(LocalRtHandle, u8, rtdt::AnyOut<'_>, &rtdt::TyDesc, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_dyn_neg_checked: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc, *mut bool) -> RtStatus,
    pub dtlv_rti_data_borrow: unsafe extern "C-unwind" fn(*const u8, *mut u8, *mut *const u8, *mut *const rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_string_create_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_string_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_string_push_bytes_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, *const u8, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_string_clear_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_string_from_bytes: unsafe extern "C-unwind" fn(LocalRtHandle, *const u8, rtdt::IndexRepr, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_pretty_print_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_create_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_clone_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, *const u8, rtdt::IndexRepr, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_build_from_sorted_slices_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc, &rtdt::TyDesc, *mut u8, *mut u8, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_btreemap_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_add_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc, *mut bool) -> RtStatus,
    pub dtlv_rti_btreemap_insert_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_remove_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_keys_into_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_values_into_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_entries_into_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_into_list_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_get_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_len_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_btreeset_len_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_btreemap_clear_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreemap_contains_key_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut bool) -> RtStatus,
    pub dtlv_rti_btreemap_get_value_ref_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut *mut u8) -> RtStatus,
    pub dtlv_rti_btreemap_set_value_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_create_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_insert_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_btreeset_remove_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_btreeset_contains_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_btreeset_clear_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_clone_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, *const u8, rtdt::IndexRepr, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_btreeset_build_from_sorted_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc, *mut u8, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_list_create_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_create_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, *const u8, rtdt::IndexRepr, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_build_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc, *mut u8, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_list_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_clear_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_len_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_list_get_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_set_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_push_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_pop_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_insert_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_remove_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_swap_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_list_reserve_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_list_shrink_to_fit_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_list_extend_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, *const u8, rtdt::IndexRepr, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_create_from_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, *const u8, rtdt::IndexRepr, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc, u8, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_get_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *const rtdt::IndexRepr, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_set_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, *const rtdt::IndexRepr, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_transpose_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *const u32, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_slice_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc, *const rtdt::SliceRange, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_reshape_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOwned<'_>, &rtdt::TyDesc, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_init_local: unsafe extern "C-unwind" fn(LocalRtHandle, *mut u8, rtdt::IndexRepr, &rtdt::TyDesc, *const u32, u32, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_tensor_hyperplane_clone_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::IndexRepr, rtdt::AnyOut<'_>) -> RtStatus,
    pub dtlv_rti_table_create_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_table_destroy_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOwned<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_table_push_row_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_table_build_from_rows_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyOut<'_>, &rtdt::TyDesc, *mut u8, &rtdt::TyDesc, rtdt::IndexRepr) -> RtStatus,
    pub dtlv_rti_table_get_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, rtdt::IndexRepr, u32) -> *const u8,
    pub dtlv_rti_table_set_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc, rtdt::IndexRepr, u32, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_table_clear_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyMut<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_table_len: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> rtdt::IndexRepr,
    pub dtlv_rti_abandon: unsafe extern "C-unwind" fn(LocalRtHandle, u8) -> RtStatus,
    pub dtlv_rti_take_abandoned: unsafe extern "C-unwind" fn(LocalRtHandle) -> u8,
    pub dtlv_rti_set_stack_limit: unsafe extern "C-unwind" fn(LocalRtHandle, usize) -> RtStatus,
    pub dtlv_rti_stack_overflow: unsafe extern "C-unwind" fn(LocalRtHandle) -> !,
    pub dtlv_rti_native_stack_limit: unsafe extern "C-unwind" fn(LocalRtHandle) -> usize,
    pub dtlv_rti_set_debug_mode: unsafe extern "C-unwind" fn(LocalRtHandle, DebugOutputMode) -> RtStatus,
    pub dtlv_rti_debuglog_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc) -> RtStatus,
    pub dtlv_rti_get_debug_buffer: unsafe extern "C-unwind" fn(LocalRtHandle, *mut *const u8, *mut usize) -> RtStatus,
    pub dtlv_rti_clear_debug_buffer: unsafe extern "C-unwind" fn(LocalRtHandle) -> RtStatus,
    pub dtlv_rti_field_offset: unsafe extern "C-unwind" fn(&rtdt::TyDesc, u32) -> u32,
    pub dtlv_rti_field_tydesc: unsafe extern "C-unwind" fn(&rtdt::TyDesc, u32) -> *const rtdt::TyDesc,
    pub dtlv_rti_payload_offset: unsafe extern "C-unwind" fn(&rtdt::TyDesc, u32) -> u32,
    pub dtlv_rti_payload_tydesc: unsafe extern "C-unwind" fn(&rtdt::TyDesc, u32) -> *const rtdt::TyDesc,
    pub dtlv_rti_tydesc_instantiate: unsafe extern "C-unwind" fn(LocalRtHandle, &rtdt::TyDesc, *const *const rtdt::TyDesc, u32) -> *const rtdt::TyDesc,
    pub dtlv_rti_shadow_alloc: unsafe extern "C-unwind" fn(LocalRtHandle, u32, u32) -> *mut u8,
    pub dtlv_rti_list_cursor_begin_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut rtdt::StrideCursor) -> RtStatus,
    pub dtlv_rti_tensor_cursor_begin_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut rtdt::StrideCursor) -> RtStatus,
    pub dtlv_rti_stride_cursor_advance_local: unsafe extern "C-unwind" fn(LocalRtHandle, *mut rtdt::StrideCursor) -> RtStatus,
    pub dtlv_rti_tensor_row_cursor_begin_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut rtdt::RowCursor) -> RtStatus,
    pub dtlv_rti_row_cursor_advance_local: unsafe extern "C-unwind" fn(LocalRtHandle, *mut rtdt::RowCursor) -> RtStatus,
    pub dtlv_rti_btreeset_cursor_begin_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut rtdt::TreeCursor) -> RtStatus,
    pub dtlv_rti_btreemap_cursor_begin_local: unsafe extern "C-unwind" fn(LocalRtHandle, rtdt::AnyRef<'_>, &rtdt::TyDesc, *mut rtdt::TreeCursor) -> RtStatus,
    pub dtlv_rti_tree_cursor_advance_local: unsafe extern "C-unwind" fn(LocalRtHandle, *mut rtdt::TreeCursor) -> RtStatus,
}
