//! Calling the runtime through the table on the handle.
//!
//! Generated from `datalove-rt`'s `c.rs` by `scripts/gen-rti.py`. Do not edit.
//!
//! Each of these finds the table on the handle it is given and calls through
//! it, so a rider writes `call::dtlv_rti_string_from_bytes(rt, ..)` and does
//! not have to hold the indirection in mind. The handle is passed on as well
//! as being read, the runtime needing its own state to do the work.
//!
//! These functions take no handle, so there is nothing here to find a table
//! on. They ask nothing of the runtime's state, reading a descriptor or
//! unpacking a value, and a caller holding a handle for other reasons can
//! reach them through [`table`](crate::table) directly:
//!
//! - `dtlv_rti_init`
//! - `dtlv_rti_data_parts`
//! - `dtlv_rti_data_borrow`
//! - `dtlv_rti_field_offset`
//! - `dtlv_rti_field_tydesc`
//! - `dtlv_rti_element_tydesc`

use datalove_rtdt as rtdt;

use crate::{DebugOutputMode, LocalRtHandle, RtEq, RtOrdering, RtStatus};

/// Calls [`RtiTable::dtlv_rti_shutdown`](crate::table::RtiTable::dtlv_rti_shutdown).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_shutdown(rt: LocalRtHandle) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_shutdown)(rt) }
}

/// Calls [`RtiTable::dtlv_rti_mem_alloc_local`](crate::table::RtiTable::dtlv_rti_mem_alloc_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_mem_alloc_local(rt: LocalRtHandle, tydesc: *const rtdt::TyDesc, count: rtdt::IndexRepr) -> *mut u8 {
    unsafe { (crate::table(rt).dtlv_rti_mem_alloc_local)(rt, tydesc, count) }
}

/// Calls [`RtiTable::dtlv_rti_mem_alloc_raw_local`](crate::table::RtiTable::dtlv_rti_mem_alloc_raw_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_mem_alloc_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: rtdt::IndexRepr) -> *mut u8 {
    unsafe { (crate::table(rt).dtlv_rti_mem_alloc_raw_local)(rt, size, align, count) }
}

/// Calls [`RtiTable::dtlv_rti_mem_free_raw_local`](crate::table::RtiTable::dtlv_rti_mem_free_raw_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_mem_free_raw_local(rt: LocalRtHandle, size: u32, align: u32, count: rtdt::IndexRepr, ptr: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_mem_free_raw_local)(rt, size, align, count, ptr) }
}

/// Calls [`RtiTable::dtlv_rti_mem_free_local`](crate::table::RtiTable::dtlv_rti_mem_free_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_mem_free_local(rt: LocalRtHandle, tydesc: *const rtdt::TyDesc, count: rtdt::IndexRepr, ptr: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_mem_free_local)(rt, tydesc, count, ptr) }
}

/// Calls [`RtiTable::dtlv_rti_move_value_local`](crate::table::RtiTable::dtlv_rti_move_value_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_move_value_local(rt: LocalRtHandle, src_ref: *const u8, tydesc: *const rtdt::TyDesc, dst_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_move_value_local)(rt, src_ref, tydesc, dst_out) }
}

/// Calls [`RtiTable::dtlv_rti_clone_local`](crate::table::RtiTable::dtlv_rti_clone_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_clone_local(rt: LocalRtHandle, value_in: *const u8, tydesc_in: *const rtdt::TyDesc, value_out: *mut u8, tydesc_out: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_clone_local)(rt, value_in, tydesc_in, value_out, tydesc_out) }
}

/// Calls [`RtiTable::dtlv_rti_eq_local`](crate::table::RtiTable::dtlv_rti_eq_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_eq_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtEq {
    unsafe { (crate::table(rt).dtlv_rti_eq_local)(rt, value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_eq_unique_local`](crate::table::RtiTable::dtlv_rti_eq_unique_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_eq_unique_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtEq {
    unsafe { (crate::table(rt).dtlv_rti_eq_unique_local)(rt, value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_cmp_local`](crate::table::RtiTable::dtlv_rti_cmp_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_cmp_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtOrdering {
    unsafe { (crate::table(rt).dtlv_rti_cmp_local)(rt, value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_cmp_total_local`](crate::table::RtiTable::dtlv_rti_cmp_total_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_cmp_total_local(rt: LocalRtHandle, value_a_ref: *const u8, value_a_tydesc: *const rtdt::TyDesc, value_b_ref: *const u8, value_b_tydesc: *const rtdt::TyDesc) -> RtOrdering {
    unsafe { (crate::table(rt).dtlv_rti_cmp_total_local)(rt, value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_add`](crate::table::RtiTable::dtlv_rti_int_add).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_add(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_add)(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_sub`](crate::table::RtiTable::dtlv_rti_int_sub).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_sub(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_sub)(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_mul`](crate::table::RtiTable::dtlv_rti_int_mul).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_mul(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_mul)(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_neg`](crate::table::RtiTable::dtlv_rti_int_neg).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_neg(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_neg)(rt, a_in, a_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_div_checked`](crate::table::RtiTable::dtlv_rti_int_div_checked).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_div_checked(rt: LocalRtHandle, a_in: *const u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_div_checked)(rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_add_assign`](crate::table::RtiTable::dtlv_rti_int_add_assign).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_add_assign(rt: LocalRtHandle, a_mut: *mut u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_add_assign)(rt, a_mut, a_tydesc, b_in, b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_sub_assign`](crate::table::RtiTable::dtlv_rti_int_sub_assign).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_sub_assign(rt: LocalRtHandle, a_mut: *mut u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_sub_assign)(rt, a_mut, a_tydesc, b_in, b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_mul_assign`](crate::table::RtiTable::dtlv_rti_int_mul_assign).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_mul_assign(rt: LocalRtHandle, a_mut: *mut u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_mul_assign)(rt, a_mut, a_tydesc, b_in, b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_div_assign_checked`](crate::table::RtiTable::dtlv_rti_int_div_assign_checked).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_div_assign_checked(rt: LocalRtHandle, a_mut: *mut u8, a_tydesc: *const rtdt::TyDesc, b_in: *const u8, b_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_div_assign_checked)(rt, a_mut, a_tydesc, b_in, b_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_from_fixed`](crate::table::RtiTable::dtlv_rti_int_from_fixed).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_from_fixed(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_from_fixed)(rt, src_in, src_tydesc, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_int_from_limbs`](crate::table::RtiTable::dtlv_rti_int_from_limbs).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_int_from_limbs(rt: LocalRtHandle, limbs_ptr: *const u32, limb_count: u32, negative: bool, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_int_from_limbs)(rt, limbs_ptr, limb_count, negative, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_any_destroy_local`](crate::table::RtiTable::dtlv_rti_any_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_any_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_any_destroy_local)(rt, value_in, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_error_from_local`](crate::table::RtiTable::dtlv_rti_error_from_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_error_from_local(rt: LocalRtHandle, inner_in: *const u8, inner_tydesc: *const rtdt::TyDesc, dest_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_error_from_local)(rt, inner_in, inner_tydesc, dest_out) }
}

/// Calls [`RtiTable::dtlv_rti_data_from_local`](crate::table::RtiTable::dtlv_rti_data_from_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_data_from_local(rt: LocalRtHandle, inner_in: *const u8, inner_tydesc: *const rtdt::TyDesc, dest_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_data_from_local)(rt, inner_in, inner_tydesc, dest_out) }
}

/// Calls [`RtiTable::dtlv_rti_clone_erased_local`](crate::table::RtiTable::dtlv_rti_clone_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_clone_erased_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_clone_erased_local)(rt, src_in, src_tydesc, dst_out, dst_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_binop`](crate::table::RtiTable::dtlv_rti_dyn_binop).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_binop(rt: LocalRtHandle, op: u8, lhs: *const u8, lhs_tydesc: *const rtdt::TyDesc, rhs: *const u8, rhs_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_binop)(rt, op, lhs, lhs_tydesc, rhs, rhs_tydesc, out, out_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_binop_checked`](crate::table::RtiTable::dtlv_rti_dyn_binop_checked).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_binop_checked(rt: LocalRtHandle, op: u8, lhs: *const u8, lhs_tydesc: *const rtdt::TyDesc, rhs: *const u8, rhs_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc, overflow_out: *mut bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_binop_checked)(rt, op, lhs, lhs_tydesc, rhs, rhs_tydesc, out, out_tydesc, overflow_out) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_unop`](crate::table::RtiTable::dtlv_rti_dyn_unop).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_unop(rt: LocalRtHandle, op: u8, value: *const u8, value_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_unop)(rt, op, value, value_tydesc, out, out_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_const`](crate::table::RtiTable::dtlv_rti_dyn_const).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_const(rt: LocalRtHandle, which: u8, out: *mut u8, out_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_const)(rt, which, out, out_tydesc, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_float_const`](crate::table::RtiTable::dtlv_rti_dyn_float_const).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_float_const(rt: LocalRtHandle, which: u8, out: *mut u8, out_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_float_const)(rt, which, out, out_tydesc, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_dyn_neg_checked`](crate::table::RtiTable::dtlv_rti_dyn_neg_checked).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_dyn_neg_checked(rt: LocalRtHandle, value: *const u8, value_tydesc: *const rtdt::TyDesc, out: *mut u8, out_tydesc: *const rtdt::TyDesc, overflow_out: *mut bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_dyn_neg_checked)(rt, value, value_tydesc, out, out_tydesc, overflow_out) }
}

/// Calls [`RtiTable::dtlv_rti_data_into_local`](crate::table::RtiTable::dtlv_rti_data_into_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_data_into_local(rt: LocalRtHandle, data_in: *const u8, dest_out: *mut u8, dest_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_data_into_local)(rt, data_in, dest_out, dest_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_erase_local`](crate::table::RtiTable::dtlv_rti_erase_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_erase_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_erase_local)(rt, src_in, src_tydesc, dst_out, dst_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_reify_local`](crate::table::RtiTable::dtlv_rti_reify_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_reify_local(rt: LocalRtHandle, src_in: *const u8, src_tydesc: *const rtdt::TyDesc, dst_out: *mut u8, dst_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_reify_local)(rt, src_in, src_tydesc, dst_out, dst_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_string_create_local`](crate::table::RtiTable::dtlv_rti_string_create_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_string_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_string_create_local)(rt, value_out, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_string_destroy_local`](crate::table::RtiTable::dtlv_rti_string_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_string_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_string_destroy_local)(rt, value_in, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_string_push_bytes_local`](crate::table::RtiTable::dtlv_rti_string_push_bytes_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_string_push_bytes_local(rt: LocalRtHandle, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc, bytes_ref: *const u8, bytes_len: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_string_push_bytes_local)(rt, string_value_mut, string_tydesc, bytes_ref, bytes_len) }
}

/// Calls [`RtiTable::dtlv_rti_string_clear_local`](crate::table::RtiTable::dtlv_rti_string_clear_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_string_clear_local(rt: LocalRtHandle, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_string_clear_local)(rt, string_value_mut, string_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_string_from_bytes`](crate::table::RtiTable::dtlv_rti_string_from_bytes).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_string_from_bytes(rt: LocalRtHandle, bytes_ptr: *const u8, bytes_len: rtdt::IndexRepr, result_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_string_from_bytes)(rt, bytes_ptr, bytes_len, result_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_pretty_print_local`](crate::table::RtiTable::dtlv_rti_pretty_print_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_pretty_print_local(rt: LocalRtHandle, arg_value_ref: *const u8, arg_tydesc_ref: *const rtdt::TyDesc, string_value_mut: *mut u8, string_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_pretty_print_local)(rt, arg_value_ref, arg_tydesc_ref, string_value_mut, string_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_create_local`](crate::table::RtiTable::dtlv_rti_btreemap_create_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_create_local)(rt, value_out, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_clone_from_slice_local`](crate::table::RtiTable::dtlv_rti_btreemap_clone_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_clone_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, slice_element_tydesc: *const rtdt::TyDesc, btreemap_value_out: *mut u8, btreemap_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_clone_from_slice_local)(rt, slice_ref, slice_len, slice_element_tydesc, btreemap_value_out, btreemap_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_build_from_sorted_slices_local`](crate::table::RtiTable::dtlv_rti_btreemap_build_from_sorted_slices_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_build_from_sorted_slices_local(rt: LocalRtHandle, map_out: *mut u8, key_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc, keys_ptr: *mut u8, values_ptr: *mut u8, num_entries: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_build_from_sorted_slices_local)(rt, map_out, key_tydesc, value_tydesc, keys_ptr, values_ptr, num_entries) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_destroy_local`](crate::table::RtiTable::dtlv_rti_btreemap_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_destroy_local)(rt, value_in, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_insert_local`](crate::table::RtiTable::dtlv_rti_btreemap_insert_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_insert_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_in: *mut u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_insert_local)(rt, btreemap_value_mut, btreemap_tydesc, key_in, key_tydesc, value_in, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_remove_local`](crate::table::RtiTable::dtlv_rti_btreemap_remove_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_remove_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_remove_local)(rt, btreemap_value_mut, btreemap_tydesc, key_ref, key_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_key_at_local`](crate::table::RtiTable::dtlv_rti_btreemap_key_at_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_key_at_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_key_at_local)(rt, btreemap_value_ref, btreemap_tydesc, index, option_value_out, option_tydesc, as_data) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_value_at_local`](crate::table::RtiTable::dtlv_rti_btreemap_value_at_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_value_at_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_value_at_local)(rt, btreemap_value_ref, btreemap_tydesc, index, option_value_out, option_tydesc, as_data) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_keys_into_local`](crate::table::RtiTable::dtlv_rti_btreemap_keys_into_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_keys_into_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_keys_into_local)(rt, btreemap_value_ref, btreemap_tydesc, list_value_mut, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_values_into_local`](crate::table::RtiTable::dtlv_rti_btreemap_values_into_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_values_into_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_values_into_local)(rt, btreemap_value_ref, btreemap_tydesc, list_value_mut, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_entries_into_local`](crate::table::RtiTable::dtlv_rti_btreemap_entries_into_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_entries_into_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_entries_into_local)(rt, btreemap_value_ref, btreemap_tydesc, list_value_mut, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_into_list_local`](crate::table::RtiTable::dtlv_rti_btreeset_into_list_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_into_list_local(rt: LocalRtHandle, set_value_ref: *const u8, set_tydesc: *const rtdt::TyDesc, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_into_list_local)(rt, set_value_ref, set_tydesc, list_value_mut, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_get_at_local`](crate::table::RtiTable::dtlv_rti_btreeset_get_at_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_get_at_local(rt: LocalRtHandle, set_value_ref: *const u8, set_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc, as_data: bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_get_at_local)(rt, set_value_ref, set_tydesc, index, option_value_out, option_tydesc, as_data) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_get_local`](crate::table::RtiTable::dtlv_rti_btreemap_get_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_get_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_get_local)(rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_get_as_data_local`](crate::table::RtiTable::dtlv_rti_btreemap_get_as_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_get_as_data_local(rt: LocalRtHandle, map_value_ref: *const u8, map_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_get_as_data_local)(rt, map_value_ref, map_tydesc, key_ref, key_tydesc, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_push_erased_local`](crate::table::RtiTable::dtlv_rti_list_push_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_push_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_push_erased_local)(rt, list_value_mut, list_tydesc, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_element_write_local`](crate::table::RtiTable::dtlv_rti_element_write_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_element_write_local(rt: LocalRtHandle, slot_out: *mut u8, slot_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_element_write_local)(rt, slot_out, slot_tydesc, value_in, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_set_value_erased_local`](crate::table::RtiTable::dtlv_rti_btreemap_set_value_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_set_value_erased_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_set_value_erased_local)(rt, btreemap_value_mut, btreemap_tydesc, key_ref, key_tydesc, value_in, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_remove_erased_local`](crate::table::RtiTable::dtlv_rti_list_remove_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_remove_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_remove_erased_local)(rt, list_value_mut, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_pop_erased_local`](crate::table::RtiTable::dtlv_rti_list_pop_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_pop_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_pop_erased_local)(rt, list_value_mut, list_tydesc, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_set_erased_local`](crate::table::RtiTable::dtlv_rti_list_set_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_set_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_set_erased_local)(rt, list_value_mut, list_tydesc, index, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_insert_erased_local`](crate::table::RtiTable::dtlv_rti_list_insert_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_insert_erased_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_insert_erased_local)(rt, list_value_mut, list_tydesc, index, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_insert_erased_local`](crate::table::RtiTable::dtlv_rti_btreeset_insert_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_insert_erased_local(rt: LocalRtHandle, set_value_mut: *mut u8, set_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_insert_erased_local)(rt, set_value_mut, set_tydesc, element_in, element_tydesc, bool_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_insert_erased_local`](crate::table::RtiTable::dtlv_rti_btreemap_insert_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_insert_erased_local(rt: LocalRtHandle, map_value_mut: *mut u8, map_tydesc: *const rtdt::TyDesc, key_in: *mut u8, key_tydesc: *const rtdt::TyDesc, value_in: *mut u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_insert_erased_local)(rt, map_value_mut, map_tydesc, key_in, key_tydesc, value_in, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_insert_data_local`](crate::table::RtiTable::dtlv_rti_btreemap_insert_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_insert_data_local(rt: LocalRtHandle, map_value_mut: *mut u8, map_tydesc: *const rtdt::TyDesc, key_data_in: *const u8, value_data_in: *const u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_insert_data_local)(rt, map_value_mut, map_tydesc, key_data_in, value_data_in) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_insert_data_local`](crate::table::RtiTable::dtlv_rti_btreeset_insert_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_insert_data_local(rt: LocalRtHandle, set_value_mut: *mut u8, set_tydesc: *const rtdt::TyDesc, data_in: *const u8, bool_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_insert_data_local)(rt, set_value_mut, set_tydesc, data_in, bool_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_len_local`](crate::table::RtiTable::dtlv_rti_btreemap_len_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_len_local(rt: LocalRtHandle, map_value_ref: *const u8, map_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_len_local)(rt, map_value_ref, map_tydesc, len_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_len_local`](crate::table::RtiTable::dtlv_rti_btreeset_len_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_len_local(rt: LocalRtHandle, set_value_ref: *const u8, set_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_len_local)(rt, set_value_ref, set_tydesc, len_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_clear_local`](crate::table::RtiTable::dtlv_rti_btreemap_clear_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_clear_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_clear_local)(rt, btreemap_value_mut, btreemap_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_contains_key_local`](crate::table::RtiTable::dtlv_rti_btreemap_contains_key_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_contains_key_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, result_out: *mut bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_contains_key_local)(rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc, result_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_get_value_ref_local`](crate::table::RtiTable::dtlv_rti_btreemap_get_value_ref_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_get_value_ref_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_ptr_out: *mut *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_get_value_ref_local)(rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc, value_ptr_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_set_value_local`](crate::table::RtiTable::dtlv_rti_btreemap_set_value_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_set_value_local(rt: LocalRtHandle, btreemap_value_mut: *mut u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_in: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_set_value_local)(rt, btreemap_value_mut, btreemap_tydesc, key_ref, key_tydesc, value_in, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_create_local`](crate::table::RtiTable::dtlv_rti_btreeset_create_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_create_local)(rt, value_out, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_destroy_local`](crate::table::RtiTable::dtlv_rti_btreeset_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_destroy_local(rt: LocalRtHandle, btreeset_value_in: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_destroy_local)(rt, btreeset_value_in, btreeset_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_insert_local`](crate::table::RtiTable::dtlv_rti_btreeset_insert_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_insert_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_insert_local)(rt, btreeset_value_mut, btreeset_tydesc, element_in, element_tydesc, bool_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_remove_local`](crate::table::RtiTable::dtlv_rti_btreeset_remove_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_remove_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_remove_local)(rt, btreeset_value_mut, btreeset_tydesc, element_ref, element_tydesc, bool_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_contains_local`](crate::table::RtiTable::dtlv_rti_btreeset_contains_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_contains_local(rt: LocalRtHandle, btreeset_value_ref: *const u8, btreeset_tydesc: *const rtdt::TyDesc, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc, bool_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_contains_local)(rt, btreeset_value_ref, btreeset_tydesc, element_ref, element_tydesc, bool_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_clear_local`](crate::table::RtiTable::dtlv_rti_btreeset_clear_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_clear_local(rt: LocalRtHandle, btreeset_value_mut: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_clear_local)(rt, btreeset_value_mut, btreeset_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_clone_from_slice_local`](crate::table::RtiTable::dtlv_rti_btreeset_clone_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_clone_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, slice_element_tydesc: *const rtdt::TyDesc, btreeset_value_out: *mut u8, btreeset_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_clone_from_slice_local)(rt, slice_ref, slice_len, slice_element_tydesc, btreeset_value_out, btreeset_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_btreeset_build_from_sorted_slice_local`](crate::table::RtiTable::dtlv_rti_btreeset_build_from_sorted_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreeset_build_from_sorted_slice_local(rt: LocalRtHandle, set_out: *mut u8, element_tydesc: *const rtdt::TyDesc, elements_ptr: *mut u8, num_elements: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreeset_build_from_sorted_slice_local)(rt, set_out, element_tydesc, elements_ptr, num_elements) }
}

/// Calls [`RtiTable::dtlv_rti_list_create_local`](crate::table::RtiTable::dtlv_rti_list_create_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_create_local)(rt, value_out, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_create_from_slice_local`](crate::table::RtiTable::dtlv_rti_list_create_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_create_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, list_value_out: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_create_from_slice_local)(rt, slice_ref, slice_len, element_tydesc, list_value_out, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_build_from_slice_local`](crate::table::RtiTable::dtlv_rti_list_build_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_build_from_slice_local(rt: LocalRtHandle, list_out: *mut u8, element_tydesc: *const rtdt::TyDesc, elements_ptr: *mut u8, num_elements: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_build_from_slice_local)(rt, list_out, element_tydesc, elements_ptr, num_elements) }
}

/// Calls [`RtiTable::dtlv_rti_list_destroy_local`](crate::table::RtiTable::dtlv_rti_list_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_destroy_local)(rt, value_in, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_clear_local`](crate::table::RtiTable::dtlv_rti_list_clear_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_clear_local(rt: LocalRtHandle, value_mut: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_clear_local)(rt, value_mut, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_set_data_local`](crate::table::RtiTable::dtlv_rti_list_set_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_set_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, data_in: *const u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_set_data_local)(rt, list_value_mut, list_tydesc, index, data_in) }
}

/// Calls [`RtiTable::dtlv_rti_list_insert_data_local`](crate::table::RtiTable::dtlv_rti_list_insert_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_insert_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, data_in: *const u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_insert_data_local)(rt, list_value_mut, list_tydesc, index, data_in) }
}

/// Calls [`RtiTable::dtlv_rti_list_remove_as_data_local`](crate::table::RtiTable::dtlv_rti_list_remove_as_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_remove_as_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_remove_as_data_local)(rt, list_value_mut, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_len_local`](crate::table::RtiTable::dtlv_rti_list_len_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_len_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, len_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_len_local)(rt, list_value_ref, list_tydesc, len_out) }
}

/// Calls [`RtiTable::dtlv_rti_list_get_as_data_local`](crate::table::RtiTable::dtlv_rti_list_get_as_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_get_as_data_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_get_as_data_local)(rt, list_value_ref, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_pop_as_data_local`](crate::table::RtiTable::dtlv_rti_list_pop_as_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_pop_as_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_pop_as_data_local)(rt, list_value_mut, list_tydesc, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_push_data_local`](crate::table::RtiTable::dtlv_rti_list_push_data_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_push_data_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, data_in: *const u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_push_data_local)(rt, list_value_mut, list_tydesc, data_in) }
}

/// Calls [`RtiTable::dtlv_rti_list_get_erased_local`](crate::table::RtiTable::dtlv_rti_list_get_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_get_erased_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_get_erased_local)(rt, list_value_ref, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_get_local`](crate::table::RtiTable::dtlv_rti_list_get_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_get_local(rt: LocalRtHandle, list_value_ref: *const u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_get_local)(rt, list_value_ref, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_set_local`](crate::table::RtiTable::dtlv_rti_list_set_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_set_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_set_local)(rt, list_value_mut, list_tydesc, index, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_push_local`](crate::table::RtiTable::dtlv_rti_list_push_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_push_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_push_local)(rt, list_value_mut, list_tydesc, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_pop_local`](crate::table::RtiTable::dtlv_rti_list_pop_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_pop_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_pop_local)(rt, list_value_mut, list_tydesc, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_insert_local`](crate::table::RtiTable::dtlv_rti_list_insert_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_insert_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, element_in: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_insert_local)(rt, list_value_mut, list_tydesc, index, element_in, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_remove_local`](crate::table::RtiTable::dtlv_rti_list_remove_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_remove_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, index: rtdt::IndexRepr, option_value_out: *mut u8, option_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_remove_local)(rt, list_value_mut, list_tydesc, index, option_value_out, option_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_reserve_local`](crate::table::RtiTable::dtlv_rti_list_reserve_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_reserve_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, additional: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_reserve_local)(rt, list_value_mut, list_tydesc, additional) }
}

/// Calls [`RtiTable::dtlv_rti_list_shrink_to_fit_local`](crate::table::RtiTable::dtlv_rti_list_shrink_to_fit_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_shrink_to_fit_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_shrink_to_fit_local)(rt, list_value_mut, list_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_list_extend_from_slice_local`](crate::table::RtiTable::dtlv_rti_list_extend_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_list_extend_from_slice_local(rt: LocalRtHandle, list_value_mut: *mut u8, list_tydesc: *const rtdt::TyDesc, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_list_extend_from_slice_local)(rt, list_value_mut, list_tydesc, slice_ref, slice_len, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_create_from_slice_local`](crate::table::RtiTable::dtlv_rti_tensor_create_from_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_create_from_slice_local(rt: LocalRtHandle, slice_ref: *const u8, slice_len: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, shape_in: *mut u8, shape_tydesc: *const rtdt::TyDesc, layout: u8, tensor_value_out: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_create_from_slice_local)(rt, slice_ref, slice_len, element_tydesc, shape_in, shape_tydesc, layout, tensor_value_out, tensor_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_destroy_local`](crate::table::RtiTable::dtlv_rti_tensor_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_destroy_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_destroy_local)(rt, tensor_value_in, tensor_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_get_local`](crate::table::RtiTable::dtlv_rti_tensor_get_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_get_local(rt: LocalRtHandle, tensor_value_ref: *const u8, tensor_tydesc: *const rtdt::TyDesc, indices_ptr: *const u32, element_value_out: *mut u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_get_local)(rt, tensor_value_ref, tensor_tydesc, indices_ptr, element_value_out, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_set_local`](crate::table::RtiTable::dtlv_rti_tensor_set_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_set_local(rt: LocalRtHandle, tensor_value_ref: *mut u8, tensor_tydesc: *const rtdt::TyDesc, indices_ptr: *const u32, element_ref: *const u8, element_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_set_local)(rt, tensor_value_ref, tensor_tydesc, indices_ptr, element_ref, element_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_transpose_local`](crate::table::RtiTable::dtlv_rti_tensor_transpose_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_transpose_local(rt: LocalRtHandle, tensor_ref: *const u8, tensor_tydesc_ref: *const rtdt::TyDesc, perm_ptr: *const u32, tensor_value_out: *mut u8, tensor_tydesc_out: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_transpose_local)(rt, tensor_ref, tensor_tydesc_ref, perm_ptr, tensor_value_out, tensor_tydesc_out) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_slice_local`](crate::table::RtiTable::dtlv_rti_tensor_slice_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_slice_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc, ranges_ptr: *const rtdt::SliceRange, result_value_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_slice_local)(rt, tensor_value_in, tensor_tydesc, ranges_ptr, result_value_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_reshape_local`](crate::table::RtiTable::dtlv_rti_tensor_reshape_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_reshape_local(rt: LocalRtHandle, tensor_value_in: *mut u8, tensor_tydesc: *const rtdt::TyDesc, new_shape_in: *mut u8, new_shape_tydesc: *const rtdt::TyDesc, result_value_out: *mut u8, result_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_reshape_local)(rt, tensor_value_in, tensor_tydesc, new_shape_in, new_shape_tydesc, result_value_out, result_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_init_local`](crate::table::RtiTable::dtlv_rti_tensor_init_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_init_local(rt: LocalRtHandle, element_data_in: *mut u8, element_count: rtdt::IndexRepr, element_tydesc: *const rtdt::TyDesc, shape_ptr: *const u32, rank: u32, tensor_value_out: *mut u8, tensor_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_init_local)(rt, element_data_in, element_count, element_tydesc, shape_ptr, rank, tensor_value_out, tensor_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_tensor_hyperplane_clone_local`](crate::table::RtiTable::dtlv_rti_tensor_hyperplane_clone_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_tensor_hyperplane_clone_local(rt: LocalRtHandle, tensor_value_ref: *const u8, tensor_tydesc: *const rtdt::TyDesc, axis0_index: rtdt::IndexRepr, sub_tensor_out: *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_tensor_hyperplane_clone_local)(rt, tensor_value_ref, tensor_tydesc, axis0_index, sub_tensor_out) }
}

/// Calls [`RtiTable::dtlv_rti_table_create_local`](crate::table::RtiTable::dtlv_rti_table_create_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_create_local(rt: LocalRtHandle, value_out: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_create_local)(rt, value_out, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_table_destroy_local`](crate::table::RtiTable::dtlv_rti_table_destroy_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_destroy_local(rt: LocalRtHandle, value_in: *mut u8, tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_destroy_local)(rt, value_in, tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_table_push_row_local`](crate::table::RtiTable::dtlv_rti_table_push_row_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_push_row_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc, row_ref: *const u8, row_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_push_row_local)(rt, table_mut, table_tydesc, row_ref, row_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_table_build_from_rows_local`](crate::table::RtiTable::dtlv_rti_table_build_from_rows_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_build_from_rows_local(rt: LocalRtHandle, table_out: *mut u8, table_tydesc: *const rtdt::TyDesc, rows_ptr: *mut u8, row_tydesc: *const rtdt::TyDesc, num_rows: rtdt::IndexRepr) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_build_from_rows_local)(rt, table_out, table_tydesc, rows_ptr, row_tydesc, num_rows) }
}

/// Calls [`RtiTable::dtlv_rti_table_get_local`](crate::table::RtiTable::dtlv_rti_table_get_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_get_local(rt: LocalRtHandle, table_ref: *const u8, table_tydesc: *const rtdt::TyDesc, row: rtdt::IndexRepr, col: u32) -> *const u8 {
    unsafe { (crate::table(rt).dtlv_rti_table_get_local)(rt, table_ref, table_tydesc, row, col) }
}

/// Calls [`RtiTable::dtlv_rti_table_set_local`](crate::table::RtiTable::dtlv_rti_table_set_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_set_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc, row: rtdt::IndexRepr, col: u32, value_ref: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_set_local)(rt, table_mut, table_tydesc, row, col, value_ref, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_table_clear_local`](crate::table::RtiTable::dtlv_rti_table_clear_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_clear_local(rt: LocalRtHandle, table_mut: *mut u8, table_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_table_clear_local)(rt, table_mut, table_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_table_len`](crate::table::RtiTable::dtlv_rti_table_len).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_table_len(rt: LocalRtHandle, table_ref: *const u8, table_tydesc: *const rtdt::TyDesc) -> rtdt::IndexRepr {
    unsafe { (crate::table(rt).dtlv_rti_table_len)(rt, table_ref, table_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_set_debug_mode`](crate::table::RtiTable::dtlv_rti_set_debug_mode).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_set_debug_mode(rt: LocalRtHandle, mode: DebugOutputMode) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_set_debug_mode)(rt, mode) }
}

/// Calls [`RtiTable::dtlv_rti_debuglog_local`](crate::table::RtiTable::dtlv_rti_debuglog_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_debuglog_local(rt: LocalRtHandle, value_ref: *const u8, value_tydesc: *const rtdt::TyDesc) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_debuglog_local)(rt, value_ref, value_tydesc) }
}

/// Calls [`RtiTable::dtlv_rti_get_debug_buffer`](crate::table::RtiTable::dtlv_rti_get_debug_buffer).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_get_debug_buffer(rt: LocalRtHandle, out_ptr: *mut *const u8, out_len: *mut usize) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_get_debug_buffer)(rt, out_ptr, out_len) }
}

/// Calls [`RtiTable::dtlv_rti_clear_debug_buffer`](crate::table::RtiTable::dtlv_rti_clear_debug_buffer).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_clear_debug_buffer(rt: LocalRtHandle) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_clear_debug_buffer)(rt) }
}

/// Calls [`RtiTable::dtlv_rti_field_read_local`](crate::table::RtiTable::dtlv_rti_field_read_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_field_read_local(rt: LocalRtHandle, dest_out: *mut u8, dest_tydesc: *const rtdt::TyDesc, base_in: *const u8, base_tydesc: *const rtdt::TyDesc, index: u32) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_field_read_local)(rt, dest_out, dest_tydesc, base_in, base_tydesc, index) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_contains_key_erased_local`](crate::table::RtiTable::dtlv_rti_btreemap_contains_key_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_contains_key_erased_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, result_out: *mut bool) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_contains_key_erased_local)(rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc, result_out) }
}

/// Calls [`RtiTable::dtlv_rti_btreemap_get_value_ref_erased_local`](crate::table::RtiTable::dtlv_rti_btreemap_get_value_ref_erased_local).
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out, and the remaining
/// arguments must be what that function requires.
#[inline]
pub unsafe fn dtlv_rti_btreemap_get_value_ref_erased_local(rt: LocalRtHandle, btreemap_value_ref: *const u8, btreemap_tydesc: *const rtdt::TyDesc, key_ref: *const u8, key_tydesc: *const rtdt::TyDesc, value_ptr_out: *mut *mut u8) -> RtStatus {
    unsafe { (crate::table(rt).dtlv_rti_btreemap_get_value_ref_erased_local)(rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc, value_ptr_out) }
}
