//! C-ABI surface API for FFI.
//!
//! All runtime functions with the `dtlv_rti_*` prefix are exposed through this module.
//!
//! ## Naming Conventions
//!
//! Function suffixes:
//! - `_local` - operates on local heap (all current functions)
//! - (future) `_global` - operates on global heap
//!
//! Parameter suffixes (ownership semantics):
//! - `_in` - move in, callee owns, `*mut`
//! - `_out` - move out, caller owns, `*mut`
//! - `_ref` - shared borrow, `*const`
//! - `_mut` - unique borrow, `*mut`
//!
//! Every value pointer is followed by its tydesc.
//!
//! ## Error Handling
//!
//! Argument pointers should never be null in correct generated code.
//! In debug builds, null pointers will trigger a `debug_assert!` panic.
//! Allocation failures return `RtStatus::Error`.

use datalove_rtdt as rtdt;
use crate::impls::rt_local;

/// A runtime handle. Needed for all calls.
///
/// This is the only native type used in the ABI directly;
/// everything else is an rtdt argument type.
pub type LocalRtHandle = *mut u8;

/// A simple status code.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RtStatus {
    Ok = 1,
    Error = 2,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtEq {
    Equals = 1,
    NotEquals = 2,
    /// Type mismatch.
    Error = 3,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtOrdering {
    Less = 1,
    Equal = 2,
    Greater = 3,
    /// Type mismatch.
    Error = 4,
}

/// Debug assertion to verify pointer alignment matches tydesc requirements.
#[inline]
fn debug_assert_aligned(ptr: *const u8, tydesc: *const rtdt::TyDesc, name: &str) {
    if cfg!(debug_assertions) {
        debug_assert!(!tydesc.is_null(), "{}: tydesc is null", name);
        let align = unsafe { (*tydesc).align } as usize;
        debug_assert!(
            (ptr as usize) % align == 0,
            "{}: pointer {:p} not aligned to {} bytes",
            name, ptr, align
        );
    }
}

/// May return null.
#[unsafe(no_mangle)]
pub extern "C-unwind" fn dtlv_rti_init() -> LocalRtHandle {
    let rt = rt_local::RtLocal::new();
    Box::into_raw(rt) as *mut u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_shutdown(
    rt: LocalRtHandle,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");

    unsafe {
        let rt = Box::from_raw(rt as *mut rt_local::RtLocal);
        rt.shutdown();
    }

    RtStatus::Ok
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_mem_alloc_local(
    rt: LocalRtHandle,
    // The type of the element being allocated (not the container).
    tydesc: *const rtdt::TyDesc,
    count: rtdt::IndexRepr,
) -> *mut u8 {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let ty = &*tydesc;
        rt_ref.alloc.alloc(ty.size, ty.align, count)
    }
}

/// Raw memory allocation without requiring a type descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_mem_alloc_raw_local(
    rt: LocalRtHandle,
    size: u32,
    align: u32,
    count: rtdt::IndexRepr,
) -> *mut u8 {
    debug_assert!(!rt.is_null(), "rt is null");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        rt_ref.alloc.alloc(size, align, count)
    }
}

/// Raw memory deallocation without requiring a type descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_mem_free_raw_local(
    rt: LocalRtHandle,
    size: u32,
    align: u32,
    count: rtdt::IndexRepr,
    ptr: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!ptr.is_null(), "ptr is null");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        rt_ref.alloc.free(size, align, count, ptr);
    }

    RtStatus::Ok
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_mem_free_local(
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    count: rtdt::IndexRepr,
    ptr: *mut u8
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert!(!ptr.is_null(), "ptr is null");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let ty = &*tydesc;
        rt_ref.alloc.free(ty.size, ty.align, count, ptr);
    }

    RtStatus::Ok
}

/// Move a value from src to dst (shallow byte copy).
///
/// This performs a simple memcpy of `tydesc.size` bytes from src to dst.
/// No deep cloning or allocation occurs. After the move, the caller should
/// treat src as invalidated (ownership transferred to dst).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_move_value_local(
    _rt: LocalRtHandle,
    src_ref: *const u8,
    tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
) -> RtStatus {
    debug_assert!(!src_ref.is_null(), "src_ref is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert!(!dst_out.is_null(), "dst_out is null");
    debug_assert_aligned(src_ref, tydesc, "move_value_local:src");
    debug_assert_aligned(dst_out, tydesc, "move_value_local:dst");
    unsafe {
        let size = (*tydesc).size as usize;
        std::ptr::copy_nonoverlapping(src_ref, dst_out, size);
    }
    RtStatus::Ok
}

/// Clone any type into the local heap.
///
/// Space is already allocated for the proximate type
/// at the `value_out` location - we just need to allocate
/// any needed buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_clone_local(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc_in: *const rtdt::TyDesc,
    value_out: *mut u8,
    tydesc_out: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc_in.is_null(), "tydesc_in is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc_out.is_null(), "tydesc_out is null");
    debug_assert_aligned(value_in, tydesc_in, "clone_local:value_in");
    debug_assert_aligned(value_out, tydesc_out, "clone_local:value_out");
    debug_assert_eq!(
        tydesc_in, tydesc_out,
        "clone: tydesc_in and tydesc_out must be identical"
    );
    unsafe {
        crate::impls::clone::clone_value(rt, value_in, tydesc_in, value_out)
    }
}

/// Standard equality.
///
/// Floats have weird cases.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_eq_local(
    _rt: LocalRtHandle,
    value_a_ref: *const u8,
    value_a_tydesc: *const rtdt::TyDesc,
    value_b_ref: *const u8,
    value_b_tydesc: *const rtdt::TyDesc,
) -> RtEq {
    debug_assert!(!value_a_ref.is_null(), "value_a_ref is null");
    debug_assert!(!value_a_tydesc.is_null(), "value_a_tydesc is null");
    debug_assert!(!value_b_ref.is_null(), "value_b_ref is null");
    debug_assert!(!value_b_tydesc.is_null(), "value_b_tydesc is null");
    debug_assert_aligned(value_a_ref, value_a_tydesc, "eq_local:value_a");
    debug_assert_aligned(value_b_ref, value_b_tydesc, "eq_local:value_b");
    // Note that the runtime handle isn't needed
    // because we don't allocate - it's just part
    // of the ABI.
    unsafe {
        crate::impls::cmp::eq(value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc)
    }
}

/// Equality where each value has a single representation.
///
/// Floats are compared bitwise.
/// This is primarily useful for keying hash tables.
/// Not yet clear whether Datalove wants this.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_eq_unique_local(
    _rt: LocalRtHandle,
    value_a_ref: *const u8,
    value_a_tydesc: *const rtdt::TyDesc,
    value_b_ref: *const u8,
    value_b_tydesc: *const rtdt::TyDesc,
) -> RtEq {
    debug_assert!(!value_a_ref.is_null(), "value_a_ref is null");
    debug_assert!(!value_a_tydesc.is_null(), "value_a_tydesc is null");
    debug_assert!(!value_b_ref.is_null(), "value_b_ref is null");
    debug_assert!(!value_b_tydesc.is_null(), "value_b_tydesc is null");
    debug_assert_aligned(value_a_ref, value_a_tydesc, "eq_unique_local:value_a");
    debug_assert_aligned(value_b_ref, value_b_tydesc, "eq_unique_local:value_b");
    unsafe {
        crate::impls::cmp::eq_unique(value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc)
    }
}

/// Establish ordering using Datalove ordering.
///
/// This is probably not actually useful. Just experimenting.
/// NaN's have total order; float zeros are equal.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_cmp_local(
    _rt: LocalRtHandle,
    value_a_ref: *const u8,
    value_a_tydesc: *const rtdt::TyDesc,
    value_b_ref: *const u8,
    value_b_tydesc: *const rtdt::TyDesc,
) -> RtOrdering {
    debug_assert!(!value_a_ref.is_null(), "value_a_ref is null");
    debug_assert!(!value_a_tydesc.is_null(), "value_a_tydesc is null");
    debug_assert!(!value_b_ref.is_null(), "value_b_ref is null");
    debug_assert!(!value_b_tydesc.is_null(), "value_b_tydesc is null");
    debug_assert_aligned(value_a_ref, value_a_tydesc, "cmp_local:value_a");
    debug_assert_aligned(value_b_ref, value_b_tydesc, "cmp_local:value_b");
    unsafe {
        crate::impls::cmp::cmp(value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc)
    }
}

/// Establish total ordering.
///
/// Floats use the typical ordering, like Rust's `total_cmp`:
///
/// > -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_cmp_total_local(
    _rt: LocalRtHandle,
    value_a_ref: *const u8,
    value_a_tydesc: *const rtdt::TyDesc,
    value_b_ref: *const u8,
    value_b_tydesc: *const rtdt::TyDesc,
) -> RtOrdering {
    debug_assert!(!value_a_ref.is_null(), "value_a_ref is null");
    debug_assert!(!value_a_tydesc.is_null(), "value_a_tydesc is null");
    debug_assert!(!value_b_ref.is_null(), "value_b_ref is null");
    debug_assert!(!value_b_tydesc.is_null(), "value_b_tydesc is null");
    debug_assert_aligned(value_a_ref, value_a_tydesc, "cmp_total_local:value_a");
    debug_assert_aligned(value_b_ref, value_b_tydesc, "cmp_total_local:value_b");
    unsafe {
        crate::impls::cmp::cmp_total(value_a_ref, value_a_tydesc, value_b_ref, value_b_tydesc)
    }
}

// ============================================================================
// Bigint arithmetic operations
// ============================================================================

/// Add two bigints: a + b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_add(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!a_in.is_null(), "a_in is null");
    debug_assert!(!b_in.is_null(), "b_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(a_in, a_tydesc, "int_add:a");
    debug_assert_aligned(b_in, b_tydesc, "int_add:b");
    debug_assert_aligned(result_out, result_tydesc, "int_add:result");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_add_impl(rt_ref, a_in, b_in, result_out)
    }
}

/// Subtract two bigints: a - b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_sub(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!a_in.is_null(), "a_in is null");
    debug_assert!(!b_in.is_null(), "b_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(a_in, a_tydesc, "int_sub:a");
    debug_assert_aligned(b_in, b_tydesc, "int_sub:b");
    debug_assert_aligned(result_out, result_tydesc, "int_sub:result");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_sub_impl(rt_ref, a_in, b_in, result_out)
    }
}

/// Multiply two bigints: a * b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_mul(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!a_in.is_null(), "a_in is null");
    debug_assert!(!b_in.is_null(), "b_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(a_in, a_tydesc, "int_mul:a");
    debug_assert_aligned(b_in, b_tydesc, "int_mul:b");
    debug_assert_aligned(result_out, result_tydesc, "int_mul:result");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_mul_impl(rt_ref, a_in, b_in, result_out)
    }
}

/// Negate a bigint: -a.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_neg(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!a_in.is_null(), "a_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(a_in, a_tydesc, "int_neg:a");
    debug_assert_aligned(result_out, result_tydesc, "int_neg:result");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_neg_impl(rt_ref, a_in, result_out)
    }
}

/// Divide two bigints: a / b.
/// Returns RtStatus::Error if b is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_div_checked(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!a_in.is_null(), "a_in is null");
    debug_assert!(!b_in.is_null(), "b_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(a_in, a_tydesc, "int_div_checked:a");
    debug_assert_aligned(b_in, b_tydesc, "int_div_checked:b");
    debug_assert_aligned(result_out, result_tydesc, "int_div_checked:result");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_div_checked_impl(rt_ref, a_in, b_in, result_out)
    }
}

/// Widen a fixed-width integer to Int (bigint).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_from_fixed(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!src_in.is_null(), "src_in is null");
    debug_assert!(!result_out.is_null(), "result_out is null");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_from_fixed_impl(rt_ref, src_in, src_tydesc, result_out)
    }
}

/// Construct an Int (bigint) from a limbs array.
///
/// Takes a pointer to u32 limbs (little-endian, least significant first),
/// the count of limbs, and a sign flag. Allocates new limb memory and
/// initializes the Int struct at the destination.
///
/// For zero values, pass limb_count=0 (limbs_ptr may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_from_limbs(
    rt: LocalRtHandle,
    limbs_ptr: *const u32,
    limb_count: u32,
    negative: bool,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert!(limb_count == 0 || !limbs_ptr.is_null(), "limbs_ptr is null with non-zero count");

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_from_limbs_impl(rt_ref, limbs_ptr, limb_count, negative, result_out)
    }
}

/// Destroys any type of value, freeing allocations recursively.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_any_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_in, tydesc, "any_destroy_local:value");
    unsafe {
        crate::impls::destroy::any_destroy_local(rt, value_in, tydesc)
    }
}

/// Creates an Error from any value (moves inner to heap).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_error_from_local(
    rt: LocalRtHandle,
    inner_in: *const u8,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!inner_in.is_null(), "inner_in is null");
    debug_assert!(!inner_tydesc.is_null(), "inner_tydesc is null");
    debug_assert!(!dest_out.is_null(), "dest_out is null");
    debug_assert_aligned(inner_in, inner_tydesc, "error_from_local:inner");
    unsafe {
        crate::impls::boxing::error_from_local(rt, inner_in, inner_tydesc, dest_out)
    }
}

/// Creates a Data from any value (moves inner to heap).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_data_from_local(
    rt: LocalRtHandle,
    inner_in: *const u8,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!inner_in.is_null(), "inner_in is null");
    debug_assert!(!inner_tydesc.is_null(), "inner_tydesc is null");
    debug_assert!(!dest_out.is_null(), "dest_out is null");
    debug_assert_aligned(inner_in, inner_tydesc, "data_from_local:inner");
    unsafe {
        crate::impls::boxing::data_from_local(rt, inner_in, inner_tydesc, dest_out)
    }
}

/// Borrow what a Data holds, as a value pointer and its descriptor.
///
/// The Data keeps ownership; both outputs point into it. See
/// `boxing::data_parts`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_data_parts(
    data_in: *const u8,
    value_out: *mut *const u8,
    tydesc_out: *mut *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc_out.is_null(), "tydesc_out is null");
    unsafe { crate::impls::boxing::data_parts(data_in, value_out, tydesc_out) }
}

/// Move the value back out of a Data, given the type that went in.
///
/// The caller supplies the tydesc, so this is a move rather than a checked
/// downcast; the Data is dead afterwards. See `boxing::data_into_local`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_data_into_local(
    rt: LocalRtHandle,
    data_in: *const u8,
    dest_out: *mut u8,
    dest_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert!(!dest_out.is_null(), "dest_out is null");
    debug_assert!(!dest_tydesc.is_null(), "dest_tydesc is null");
    debug_assert_aligned(dest_out, dest_tydesc, "data_into_local:dest_out");
    unsafe {
        crate::impls::boxing::data_into_local(rt, data_in, dest_out, dest_tydesc)
    }
}

/// Move a value into the erased shape a generic callee was compiled for.
///
/// The two tydescs have the same shape except where the callee's has `data`.
/// See `boxing::erase_local`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_erase_local(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!src_in.is_null(), "src_in is null");
    debug_assert!(!src_tydesc.is_null(), "src_tydesc is null");
    debug_assert!(!dst_out.is_null(), "dst_out is null");
    debug_assert!(!dst_tydesc.is_null(), "dst_tydesc is null");
    unsafe {
        crate::impls::boxing::erase_local(rt, src_in, src_tydesc, dst_out, dst_tydesc)
    }
}

/// Move a value back out of its erased shape. The inverse of `dtlv_rti_erase_local`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_reify_local(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!src_in.is_null(), "src_in is null");
    debug_assert!(!src_tydesc.is_null(), "src_tydesc is null");
    debug_assert!(!dst_out.is_null(), "dst_out is null");
    debug_assert!(!dst_tydesc.is_null(), "dst_tydesc is null");
    unsafe {
        crate::impls::boxing::reify_local(rt, src_in, src_tydesc, dst_out, dst_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_out, tydesc, "string_create_local:value_out");
    unsafe {
        crate::impls::string::string_create_local(rt, value_out, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_destroy_local(
    rt: LocalRtHandle,
    // Pointer will be freed.
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_in, tydesc, "string_destroy_local:value");
    unsafe {
        crate::impls::string::string_destroy_local(rt, value_in, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_push_bytes_local(
    rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
    bytes_ref: *const u8,
    bytes_len: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!string_value_mut.is_null(), "string_value_mut is null");
    debug_assert!(!string_tydesc.is_null(), "string_tydesc is null");
    debug_assert!(bytes_len == 0 || !bytes_ref.is_null(), "bytes_ref is null");
    debug_assert_aligned(string_value_mut, string_tydesc, "string_push_bytes_local:string");
    unsafe {
        crate::impls::string::string_push_bytes_local(rt, string_value_mut, string_tydesc, bytes_ref, bytes_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_clear_local(
    rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!string_value_mut.is_null(), "string_value_mut is null");
    debug_assert!(!string_tydesc.is_null(), "string_tydesc is null");
    debug_assert_aligned(string_value_mut, string_tydesc, "string_clear_local:string");
    unsafe {
        crate::impls::string::string_clear_local(rt, string_value_mut, string_tydesc)
    }
}

/// Creates a string from UTF-8 bytes in a single call.
///
/// If bytes_len is 0, bytes_ptr may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_from_bytes(
    rt: LocalRtHandle,
    bytes_ptr: *const u8,
    bytes_len: rtdt::IndexRepr,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert!(!result_tydesc.is_null(), "result_tydesc is null");
    debug_assert!(bytes_len == 0 || !bytes_ptr.is_null(), "bytes_ptr is null with non-zero len");
    debug_assert_aligned(result_out, result_tydesc, "string_from_bytes:result_out");
    unsafe {
        crate::impls::string::string_from_bytes(rt, bytes_ptr, bytes_len, result_out, result_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_pretty_print_local(
    rt: LocalRtHandle,
    arg_value_ref: *const u8,
    arg_tydesc_ref: *const rtdt::TyDesc,
    // String previously allocated by string_create_local
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!arg_value_ref.is_null(), "arg_value_ref is null");
    debug_assert!(!arg_tydesc_ref.is_null(), "arg_tydesc_ref is null");
    debug_assert!(!string_value_mut.is_null(), "string_value_mut is null");
    debug_assert!(!string_tydesc.is_null(), "string_tydesc is null");
    debug_assert_aligned(arg_value_ref, arg_tydesc_ref, "pretty_print_local:arg");
    debug_assert_aligned(string_value_mut, string_tydesc, "pretty_print_local:string");
    unsafe {
        crate::impls::pretty::pretty_print_local(rt, arg_value_ref, arg_tydesc_ref, string_value_mut, string_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    // BTreeMap type.
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_out, tydesc, "btreemap_create_local:value_out");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::btreemap::btreemap_create_impl(rt_ref, value_out, tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_clone_from_slice_local(
    rt: LocalRtHandle,
    // Values will be cloned.
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    // Should be a tuple of key/value I guess.
    slice_element_tydesc: *const rtdt::TyDesc,
    // Destination will be overwritten.
    btreemap_value_out: *mut u8,
    // BTreeMap type.
    btreemap_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(slice_len == 0 || !slice_ref.is_null(), "slice_ref is null");
    debug_assert!(!slice_element_tydesc.is_null(), "slice_element_tydesc is null");
    debug_assert!(!btreemap_value_out.is_null(), "btreemap_value_out is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert_aligned(slice_ref, slice_element_tydesc, "btreemap_clone_from_slice:slice");
    debug_assert_aligned(btreemap_value_out, btreemap_tydesc, "btreemap_clone_from_slice:btreemap");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        crate::impls::btreemap::btreemap_clone_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            slice_element_tydesc,
            btreemap_value_out,
            btreemap_tydesc_ref,
        )
    }
}

/// Build a BTreeMap from sorted slices of already-instantiated keys and values.
///
/// Takes ownership of the keys and values by moving them from the input buffers
/// into the tree structure. The input buffers should not be used after this call.
/// Keys must already be sorted.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_build_from_sorted_slices_local(
    rt: LocalRtHandle,
    map_out: *mut u8,
    key_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
    keys_ptr: *mut u8,
    values_ptr: *mut u8,
    num_entries: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!map_out.is_null(), "map_out is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!value_tydesc.is_null(), "value_tydesc is null");
    debug_assert!(num_entries == 0 || !keys_ptr.is_null(), "keys_ptr is null");
    debug_assert!(num_entries == 0 || !values_ptr.is_null(), "values_ptr is null");
    debug_assert_aligned(keys_ptr, key_tydesc, "btreemap_build_from_sorted_slices:keys");
    debug_assert_aligned(values_ptr, value_tydesc, "btreemap_build_from_sorted_slices:values");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ptr(value_tydesc);
        crate::impls::btreemap::btreemap_build_from_sorted_slices(
            rt_ref,
            map_out as *mut rtdt::Map,
            key_tydesc_ref,
            value_tydesc_ref,
            keys_ptr,
            values_ptr,
            num_entries,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_destroy_local(
    rt: LocalRtHandle,
    // Pointer will be freed.
    value_in: *mut u8,
    // BTreMap type.
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_in, tydesc, "btreemap_destroy_local:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::btreemap::btreemap_destroy_impl(rt_ref, value_in, tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_insert_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    // Value is moved.
    key_in: *mut u8,
    key_tydesc: *const rtdt::TyDesc,
    // Value is moved.
    value_in: *mut u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreemap_value_mut.is_null(), "btreemap_value_mut is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_in.is_null(), "key_in is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!value_tydesc.is_null(), "value_tydesc is null");
    debug_assert_aligned(btreemap_value_mut, btreemap_tydesc, "btreemap_insert:map");
    debug_assert_aligned(key_in, key_tydesc, "btreemap_insert:key");
    debug_assert_aligned(value_in, value_tydesc, "btreemap_insert:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        debug_assert_eq!(
            btreemap_tydesc_ref.map_key_ty().as_ptr(), key_tydesc,
            "btreemap_insert: key_tydesc doesn't match map's key type"
        );
        debug_assert_eq!(
            btreemap_tydesc_ref.map_value_ty().as_ptr(), value_tydesc,
            "btreemap_insert: value_tydesc doesn't match map's value type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ptr(value_tydesc);
        crate::impls::btreemap::btreemap_insert_impl(
            rt_ref,
            btreemap_value_mut,
            btreemap_tydesc_ref,
            key_in,
            key_tydesc_ref,
            value_in,
            value_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_remove_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreemap_value_mut.is_null(), "btreemap_value_mut is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert_aligned(btreemap_value_mut, btreemap_tydesc, "btreemap_remove:map");
    debug_assert_aligned(key_ref, key_tydesc, "btreemap_remove:key");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        debug_assert_eq!(
            btreemap_tydesc_ref.map_key_ty().as_ptr(), key_tydesc,
            "btreemap_remove: key_tydesc doesn't match map's key type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        crate::impls::btreemap::btreemap_remove_impl(
            rt_ref,
            btreemap_value_mut,
            btreemap_tydesc_ref,
            key_ref,
            key_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_get_local(
    rt: LocalRtHandle,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreemap_value_ref.is_null(), "btreemap_value_ref is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(btreemap_value_ref, btreemap_tydesc, "btreemap_get:map");
    debug_assert_aligned(key_ref, key_tydesc, "btreemap_get:key");
    debug_assert_aligned(option_value_out, option_tydesc, "btreemap_get:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        debug_assert_eq!(
            btreemap_tydesc_ref.map_key_ty().as_ptr(), key_tydesc,
            "btreemap_get: key_tydesc doesn't match map's key type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::btreemap::btreemap_get_impl(
            rt_ref,
            btreemap_value_ref,
            btreemap_tydesc_ref,
            key_ref,
            key_tydesc_ref,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_get_as_data_local(
    rt: LocalRtHandle,
    map_value_ref: *const u8,
    map_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!map_value_ref.is_null(), "map_value_ref is null");
    debug_assert!(!map_tydesc.is_null(), "map_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::btreemap::btreemap_get_as_data_impl(
            rt_ref,
            map_value_ref,
            rtdt::TyDescRef::from_ptr(map_tydesc),
            key_ref,
            rtdt::TyDescRef::from_ptr(key_tydesc),
            option_value_out,
            rtdt::TyDescRef::from_ptr(option_tydesc),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_insert_data_local(
    rt: LocalRtHandle,
    map_value_mut: *mut u8,
    map_tydesc: *const rtdt::TyDesc,
    key_data_in: *const u8,
    value_data_in: *const u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!map_value_mut.is_null(), "map_value_mut is null");
    debug_assert!(!map_tydesc.is_null(), "map_tydesc is null");
    debug_assert!(!key_data_in.is_null(), "key_data_in is null");
    debug_assert!(!value_data_in.is_null(), "value_data_in is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::btreemap::btreemap_insert_data_impl(
            rt_ref,
            map_value_mut,
            rtdt::TyDescRef::from_ptr(map_tydesc),
            key_data_in,
            value_data_in,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_insert_data_local(
    rt: LocalRtHandle,
    set_value_mut: *mut u8,
    set_tydesc: *const rtdt::TyDesc,
    data_in: *const u8,
    bool_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!set_value_mut.is_null(), "set_value_mut is null");
    debug_assert!(!set_tydesc.is_null(), "set_tydesc is null");
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert!(!bool_out.is_null(), "bool_out is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_insert_data_impl(
            rt_ref,
            set_value_mut,
            rtdt::TyDescRef::from_ptr(set_tydesc),
            data_in,
            bool_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_len_local(
    _rt: LocalRtHandle,
    map_value_ref: *const u8,
    map_tydesc: *const rtdt::TyDesc,
    len_out: *mut u8,
) -> RtStatus {
    debug_assert!(!map_value_ref.is_null(), "map_value_ref is null");
    debug_assert!(!len_out.is_null(), "len_out is null");
    debug_assert_aligned(map_value_ref, map_tydesc, "btreemap_len:map");
    unsafe { crate::impls::btreemap::btreemap_len_impl(map_value_ref, len_out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_len_local(
    _rt: LocalRtHandle,
    set_value_ref: *const u8,
    set_tydesc: *const rtdt::TyDesc,
    len_out: *mut u8,
) -> RtStatus {
    debug_assert!(!set_value_ref.is_null(), "set_value_ref is null");
    debug_assert!(!len_out.is_null(), "len_out is null");
    debug_assert_aligned(set_value_ref, set_tydesc, "btreeset_len:set");
    unsafe { crate::impls::set::btreeset_len_impl(set_value_ref, len_out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_clear_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreemap_value_mut.is_null(), "btreemap_value_mut is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert_aligned(btreemap_value_mut, btreemap_tydesc, "btreemap_clear:map");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::btreemap::btreemap_clear_impl(rt_ref, btreemap_value_mut, rtdt::TyDescRef::from_ptr(btreemap_tydesc))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_contains_key_local(
    _rt: LocalRtHandle,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
    result_out: *mut bool,
) -> RtStatus {
    debug_assert!(!btreemap_value_ref.is_null(), "btreemap_value_ref is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(btreemap_value_ref, btreemap_tydesc, "btreemap_contains_key:map");
    debug_assert_aligned(key_ref, key_tydesc, "btreemap_contains_key:key");
    unsafe {
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        crate::impls::btreemap::btreemap_contains_key_impl(
            btreemap_value_ref,
            btreemap_tydesc_ref,
            key_ref,
            key_tydesc_ref,
            result_out,
        )
    }
}

/// Returns a mutable pointer to a map value for in-place update.
///
/// # Safety
///
/// The returned pointer is an interior pointer into the BTreeMap. It is
/// invalidated by any insert, remove, or clear on the map. The caller must
/// ensure no such mutations occur between obtaining and using the pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_get_value_ref_local(
    _rt: LocalRtHandle,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
    value_ptr_out: *mut *mut u8,
) -> RtStatus {
    debug_assert!(!btreemap_value_ref.is_null(), "btreemap_value_ref is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!value_ptr_out.is_null(), "value_ptr_out is null");
    debug_assert_aligned(btreemap_value_ref, btreemap_tydesc, "btreemap_get_value_ref:map");
    debug_assert_aligned(key_ref, key_tydesc, "btreemap_get_value_ref:key");
    unsafe {
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        crate::impls::btreemap::btreemap_get_value_ref_impl(
            btreemap_value_ref,
            btreemap_tydesc_ref,
            key_ref,
            key_tydesc_ref,
            value_ptr_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_set_value_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    key_ref: *const u8,
    key_tydesc: *const rtdt::TyDesc,
    value_in: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreemap_value_mut.is_null(), "btreemap_value_mut is null");
    debug_assert!(!btreemap_tydesc.is_null(), "btreemap_tydesc is null");
    debug_assert!(!key_ref.is_null(), "key_ref is null");
    debug_assert!(!key_tydesc.is_null(), "key_tydesc is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!value_tydesc.is_null(), "value_tydesc is null");
    debug_assert_aligned(btreemap_value_mut, btreemap_tydesc, "btreemap_set_value:map");
    debug_assert_aligned(key_ref, key_tydesc, "btreemap_set_value:key");
    debug_assert_aligned(value_in, value_tydesc, "btreemap_set_value:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ptr(btreemap_tydesc);
        debug_assert_eq!(
            btreemap_tydesc_ref.map_key_ty().as_ptr(), key_tydesc,
            "btreemap_set_value: key_tydesc doesn't match map's key type"
        );
        debug_assert_eq!(
            btreemap_tydesc_ref.map_value_ty().as_ptr(), value_tydesc,
            "btreemap_set_value: value_tydesc doesn't match map's value type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ptr(value_tydesc);
        crate::impls::btreemap::btreemap_set_value_impl(
            rt_ref,
            btreemap_value_mut,
            btreemap_tydesc_ref,
            key_ref,
            key_tydesc_ref,
            value_in,
            value_tydesc_ref,
        )
    }
}

// BTreeSet operations.

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    // BTreeSet type.
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_out, tydesc, "btreeset_create:value_out");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_create_impl(rt_ref, value_out, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_destroy_local(
    rt: LocalRtHandle,
    btreeset_value_in: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreeset_value_in.is_null(), "btreeset_value_in is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert_aligned(btreeset_value_in, btreeset_tydesc, "btreeset_destroy:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::set_destroy_impl(rt_ref, btreeset_value_in, btreeset_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_insert_local(
    rt: LocalRtHandle,
    btreeset_value_mut: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
    // Element is moved.
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
    // Output: 1 if newly inserted, 0 if already existed.
    bool_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreeset_value_mut.is_null(), "btreeset_value_mut is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert!(!element_in.is_null(), "element_in is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(!bool_out.is_null(), "bool_out is null");
    debug_assert_aligned(btreeset_value_mut, btreeset_tydesc, "btreeset_insert:set");
    debug_assert_aligned(element_in, element_tydesc, "btreeset_insert:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        debug_assert_eq!(
            btreeset_tydesc_ref.set_element_ty().as_ptr(), element_tydesc,
            "btreeset_insert: element_tydesc doesn't match set's element type"
        );

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_insert_impl(
            rt_ref,
            btreeset_value_mut,
            btreeset_tydesc,
            element_in,
            element_tydesc,
            bool_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_remove_local(
    rt: LocalRtHandle,
    btreeset_value_mut: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
    element_ref: *const u8,
    element_tydesc: *const rtdt::TyDesc,
    // Output: 1 if removed, 0 if not found.
    bool_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreeset_value_mut.is_null(), "btreeset_value_mut is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert!(!element_ref.is_null(), "element_ref is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(!bool_out.is_null(), "bool_out is null");
    debug_assert_aligned(btreeset_value_mut, btreeset_tydesc, "btreeset_remove:set");
    debug_assert_aligned(element_ref, element_tydesc, "btreeset_remove:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        debug_assert_eq!(
            btreeset_tydesc_ref.set_element_ty().as_ptr(), element_tydesc,
            "btreeset_remove: element_tydesc doesn't match set's element type"
        );

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_remove_impl(
            rt_ref,
            btreeset_value_mut,
            btreeset_tydesc,
            element_ref,
            element_tydesc,
            bool_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_contains_local(
    rt: LocalRtHandle,
    btreeset_value_ref: *const u8,
    btreeset_tydesc: *const rtdt::TyDesc,
    element_ref: *const u8,
    element_tydesc: *const rtdt::TyDesc,
    // Output: 1 if contains, 0 if not.
    bool_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreeset_value_ref.is_null(), "btreeset_value_ref is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert!(!element_ref.is_null(), "element_ref is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(!bool_out.is_null(), "bool_out is null");
    debug_assert_aligned(btreeset_value_ref, btreeset_tydesc, "btreeset_contains:set");
    debug_assert_aligned(element_ref, element_tydesc, "btreeset_contains:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        debug_assert_eq!(
            btreeset_tydesc_ref.set_element_ty().as_ptr(), element_tydesc,
            "btreeset_contains: element_tydesc doesn't match set's element type"
        );

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_contains_impl(
            rt_ref,
            btreeset_value_ref,
            btreeset_tydesc,
            element_ref,
            element_tydesc,
            bool_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_clear_local(
    rt: LocalRtHandle,
    btreeset_value_mut: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!btreeset_value_mut.is_null(), "btreeset_value_mut is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert_aligned(btreeset_value_mut, btreeset_tydesc, "btreeset_clear:set");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_clear_impl(rt_ref, btreeset_value_mut, btreeset_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_clone_from_slice_local(
    rt: LocalRtHandle,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    slice_element_tydesc: *const rtdt::TyDesc,
    btreeset_value_out: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(slice_len == 0 || !slice_ref.is_null(), "slice_ref is null");
    debug_assert!(!slice_element_tydesc.is_null(), "slice_element_tydesc is null");
    debug_assert!(!btreeset_value_out.is_null(), "btreeset_value_out is null");
    debug_assert!(!btreeset_tydesc.is_null(), "btreeset_tydesc is null");
    debug_assert_aligned(slice_ref, slice_element_tydesc, "btreeset_clone_from_slice:slice");
    debug_assert_aligned(btreeset_value_out, btreeset_tydesc, "btreeset_clone_from_slice:set");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        debug_assert_eq!(
            btreeset_tydesc_ref.set_element_ty().as_ptr(), slice_element_tydesc,
            "btreeset_clone_from_slice: element_tydesc doesn't match set's element type"
        );

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_clone_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            slice_element_tydesc,
            btreeset_value_out,
            btreeset_tydesc,
        )
    }
}

/// Build a BTreeSet from a sorted slice of already-instantiated elements.
///
/// Takes ownership of the elements by moving them from the input buffer into
/// the tree structure. The input buffer should not be used after this call.
/// Elements must already be sorted.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_build_from_sorted_slice_local(
    rt: LocalRtHandle,
    set_out: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
    elements_ptr: *mut u8,
    num_elements: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!set_out.is_null(), "set_out is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(num_elements == 0 || !elements_ptr.is_null(), "elements_ptr is null");
    debug_assert_aligned(elements_ptr, element_tydesc, "btreeset_build_from_sorted_slice:elements");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_build_from_sorted_slice(
            rt_ref,
            set_out as *mut rtdt::Set,
            element_tydesc,
            elements_ptr,
            num_elements,
        )
    }
}

// ============================================================================
// List Operations
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_create_local(
    rt: LocalRtHandle,
    value_out: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_out, tydesc, "list_create:value_out");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::list::list_create_impl(rt_ref, value_out, tydesc_ref)
    }
}

/// Creates a list by cloning elements from a contiguous slice.
///
/// The `slice_ref` is a read-only reference to source elements. Each element
/// is cloned into the new list. Caller retains ownership of the source slice and
/// must destroy those elements separately after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_create_from_slice_local(
    rt: LocalRtHandle,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: *const rtdt::TyDesc,
    list_value_out: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_out.is_null(), "list_value_out is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert_aligned(slice_ref, element_tydesc, "list_create_from_slice:slice");
    debug_assert_aligned(list_value_out, list_tydesc, "list_create_from_slice:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        debug_assert_eq!(
            list_tydesc_ref.list_element_ty().as_ptr(), element_tydesc,
            "list_create_from_slice: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_create_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            element_tydesc_ref,
            list_value_out,
            list_tydesc_ref,
        )
    }
}

/// Builds a list by moving elements from a contiguous slice.
///
/// The `elements_ptr` is a mutable pointer to source elements. Elements are moved
/// (not cloned) into the new list. After this call, the source buffer is consumed
/// and should not be destroyed separately.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_build_from_slice_local(
    rt: LocalRtHandle,
    list_out: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
    elements_ptr: *mut u8,
    num_elements: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_out.is_null(), "list_out is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(num_elements == 0 || !elements_ptr.is_null(), "elements_ptr is null");
    debug_assert_aligned(elements_ptr, element_tydesc, "list_build_from_slice:elements");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_build_from_slice_impl(
            rt_ref,
            list_out,
            element_tydesc_ref,
            elements_ptr,
            num_elements,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_in, tydesc, "list_destroy:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::list::list_destroy_impl(rt_ref, value_in, tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_clear_local(
    rt: LocalRtHandle,
    value_mut: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_mut.is_null(), "value_mut is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_mut, tydesc, "list_clear:value");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::list::list_clear_impl(rt_ref, value_mut, tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_set_data_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    data_in: *const u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_set_data:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_set_data_impl(rt_ref, list_value_mut, list_tydesc_ref, index, data_in)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_insert_data_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    data_in: *const u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_insert_data:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_insert_data_impl(rt_ref, list_value_mut, list_tydesc_ref, index, data_in)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_remove_as_data_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_remove_as_data:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_remove_as_data:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_remove_as_data_impl(
            rt_ref, list_value_mut, list_tydesc_ref, index, option_value_out, option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_len_local(
    rt: LocalRtHandle,
    list_value_ref: *const u8,
    list_tydesc: *const rtdt::TyDesc,
    len_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_ref.is_null(), "list_value_ref is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!len_out.is_null(), "len_out is null");
    debug_assert_aligned(list_value_ref, list_tydesc, "list_len:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_len_impl(rt_ref, list_value_ref, list_tydesc_ref, len_out)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_get_as_data_local(
    rt: LocalRtHandle,
    list_value_ref: *const u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_ref.is_null(), "list_value_ref is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_ref, list_tydesc, "list_get_as_data:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_get_as_data:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_get_as_data_impl(
            rt_ref,
            list_value_ref,
            list_tydesc_ref,
            index,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_pop_as_data_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_pop_as_data:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_pop_as_data:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_pop_as_data_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_push_data_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    data_in: *const u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_push_data:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_push_data_impl(rt_ref, list_value_mut, list_tydesc_ref, data_in)
    }
}

/// Read an element, packing it into a `data` only if it is not one already.
///
/// The decision is here rather than at each call site so that the four
/// backends cannot each have their own answer. See `list_get_erased_impl`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_get_erased_local(
    rt: LocalRtHandle,
    list_value_ref: *const u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_ref.is_null(), "list_value_ref is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::list::list_get_erased_impl(
            rt_ref,
            list_value_ref,
            rtdt::TyDescRef::from_ptr(list_tydesc),
            index,
            option_value_out,
            rtdt::TyDescRef::from_ptr(option_tydesc),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_get_local(
    rt: LocalRtHandle,
    list_value_ref: *const u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_ref.is_null(), "list_value_ref is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_ref, list_tydesc, "list_get:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_get:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_get_impl(
            rt_ref,
            list_value_ref,
            list_tydesc_ref,
            index,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_set_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!element_in.is_null(), "element_in is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_set:list");
    debug_assert_aligned(element_in, element_tydesc, "list_set:element");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        debug_assert_eq!(
            list_tydesc_ref.list_element_ty().as_ptr(), element_tydesc,
            "list_set: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_set_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            index,
            element_in,
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_push_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!element_in.is_null(), "element_in is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_push:list");
    debug_assert_aligned(element_in, element_tydesc, "list_push:element");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        debug_assert_eq!(
            list_tydesc_ref.list_element_ty().as_ptr(), element_tydesc,
            "list_push: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_push_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            element_in,
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_pop_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_pop:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_pop:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_pop_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_insert_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!element_in.is_null(), "element_in is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_insert:list");
    debug_assert_aligned(element_in, element_tydesc, "list_insert:element");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        debug_assert_eq!(
            list_tydesc_ref.list_element_ty().as_ptr(), element_tydesc,
            "list_insert: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_insert_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            index,
            element_in,
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_remove_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!option_value_out.is_null(), "option_value_out is null");
    debug_assert!(!option_tydesc.is_null(), "option_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_remove:list");
    debug_assert_aligned(option_value_out, option_tydesc, "list_remove:option");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ptr(option_tydesc);
        crate::impls::list::list_remove_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            index,
            option_value_out,
            option_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_reserve_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    additional: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_reserve:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_reserve_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            additional,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_shrink_to_fit_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_shrink_to_fit:list");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        crate::impls::list::list_shrink_to_fit_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_extend_from_slice_local(
    rt: LocalRtHandle,
    list_value_mut: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!list_value_mut.is_null(), "list_value_mut is null");
    debug_assert!(!list_tydesc.is_null(), "list_tydesc is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert_aligned(list_value_mut, list_tydesc, "list_extend_from_slice:list");
    debug_assert_aligned(slice_ref, element_tydesc, "list_extend_from_slice:slice");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let list_tydesc_ref = rtdt::TyDescRef::from_ptr(list_tydesc);
        debug_assert_eq!(
            list_tydesc_ref.list_element_ty().as_ptr(), element_tydesc,
            "list_extend_from_slice: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        crate::impls::list::list_extend_from_slice_impl(
            rt_ref,
            list_value_mut,
            list_tydesc_ref,
            slice_ref,
            slice_len,
            element_tydesc_ref,
        )
    }
}

// ============================================================================
// Tensor Operations
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_create_from_slice_local(
    rt: LocalRtHandle,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: *const rtdt::TyDesc,
    shape_in: *mut u8,
    shape_tydesc: *const rtdt::TyDesc,
    layout: u8,
    tensor_value_out: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_out.is_null(), "tensor_value_out is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(!shape_in.is_null(), "shape_in is null");
    debug_assert!(!shape_tydesc.is_null(), "shape_tydesc is null");
    debug_assert_aligned(slice_ref, element_tydesc, "tensor_create_from_slice:slice");
    debug_assert_aligned(shape_in, shape_tydesc, "tensor_create_from_slice:shape");
    debug_assert_aligned(tensor_value_out, tensor_tydesc, "tensor_create_from_slice:tensor");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        debug_assert_eq!(
            tensor_tydesc_ref.tensor_element_ty().as_ptr(), element_tydesc,
            "tensor_create_from_slice: element_tydesc doesn't match tensor's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let shape_tydesc_ref = rtdt::TyDescRef::from_ptr(shape_tydesc);
        crate::impls::tensor::tensor_create_from_slice_impl(
            rt_ref,
            slice_ref,
            rtdt::Index(slice_len),
            element_tydesc_ref,
            shape_in,
            shape_tydesc_ref,
            layout,
            tensor_value_out,
            tensor_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_destroy_local(
    rt: LocalRtHandle,
    tensor_value_in: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_in.is_null(), "tensor_value_in is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert_aligned(tensor_value_in, tensor_tydesc, "tensor_destroy:tensor");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        crate::impls::tensor::tensor_destroy_impl(rt_ref, tensor_value_in, tensor_tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_get_local(
    rt: LocalRtHandle,
    tensor_value_ref: *const u8,
    tensor_tydesc: *const rtdt::TyDesc,
    indices_ptr: *const u32,
    element_value_out: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_ref.is_null(), "tensor_value_ref is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!indices_ptr.is_null(), "indices_ptr is null");
    debug_assert!(!element_value_out.is_null(), "element_value_out is null");
    debug_assert_aligned(tensor_value_ref, tensor_tydesc, "tensor_get:tensor");
    debug_assert_aligned(element_value_out, element_tydesc, "tensor_get:element");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        debug_assert_eq!(
            tensor_tydesc_ref.tensor_element_ty().as_ptr(), element_tydesc,
            "tensor_get: element_tydesc doesn't match tensor's element type"
        );
        crate::impls::tensor::tensor_get_impl(
            rt_ref,
            tensor_value_ref,
            tensor_tydesc_ref,
            indices_ptr,
            element_value_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_set_local(
    rt: LocalRtHandle,
    tensor_value_ref: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
    indices_ptr: *const u32,
    element_ref: *const u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_ref.is_null(), "tensor_value_ref is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!indices_ptr.is_null(), "indices_ptr is null");
    debug_assert!(!element_ref.is_null(), "element_ref is null");
    debug_assert_aligned(tensor_value_ref, tensor_tydesc, "tensor_set:tensor");
    debug_assert_aligned(element_ref, element_tydesc, "tensor_set:element");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        debug_assert_eq!(
            tensor_tydesc_ref.tensor_element_ty().as_ptr(), element_tydesc,
            "tensor_set: element_tydesc doesn't match tensor's element type"
        );
        crate::impls::tensor::tensor_set_impl(
            rt_ref,
            tensor_value_ref,
            tensor_tydesc_ref,
            indices_ptr,
            element_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_transpose_local(
    rt: LocalRtHandle,
    tensor_ref: *const u8,
    tensor_tydesc_ref: *const rtdt::TyDesc,
    perm_ptr: *const u32,
    tensor_value_out: *mut u8,
    tensor_tydesc_out: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_ref.is_null(), "tensor_ref is null");
    debug_assert!(!tensor_tydesc_ref.is_null(), "tensor_tydesc_ref is null");
    debug_assert!(!perm_ptr.is_null(), "perm_ptr is null");
    debug_assert!(!tensor_value_out.is_null(), "tensor_value_out is null");
    debug_assert_aligned(tensor_ref, tensor_tydesc_ref, "tensor_transpose:tensor_in");
    debug_assert_aligned(tensor_value_out, tensor_tydesc_out, "tensor_transpose:tensor_out");
    unsafe {
        debug_assert_eq!(
            tensor_tydesc_ref, tensor_tydesc_out,
            "tensor_transpose: input and output tensor tydescs must be identical"
        );

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc = rtdt::TyDescRef::from_ptr(tensor_tydesc_ref);
        crate::impls::tensor::tensor_transpose_impl(
            rt_ref,
            tensor_ref as *mut u8,
            tensor_tydesc,
            perm_ptr,
            tensor_value_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_slice_local(
    rt: LocalRtHandle,
    tensor_value_in: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
    ranges_ptr: *const rtdt::SliceRange,
    result_value_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_in.is_null(), "tensor_value_in is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!ranges_ptr.is_null(), "ranges_ptr is null");
    debug_assert!(!result_value_out.is_null(), "result_value_out is null");
    debug_assert!(!result_tydesc.is_null(), "result_tydesc is null");
    debug_assert_aligned(tensor_value_in, tensor_tydesc, "tensor_slice:tensor");
    debug_assert_aligned(result_value_out, result_tydesc, "tensor_slice:result");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        let result_tydesc_ref = rtdt::TyDescRef::from_ptr(result_tydesc);
        crate::impls::tensor::tensor_slice_impl(
            rt_ref,
            tensor_value_in,
            tensor_tydesc_ref,
            ranges_ptr,
            result_value_out,
            result_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_reshape_local(
    rt: LocalRtHandle,
    tensor_value_in: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
    new_shape_in: *mut u8,
    new_shape_tydesc: *const rtdt::TyDesc,
    result_value_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_in.is_null(), "tensor_value_in is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!new_shape_in.is_null(), "new_shape_in is null");
    debug_assert!(!new_shape_tydesc.is_null(), "new_shape_tydesc is null");
    debug_assert!(!result_value_out.is_null(), "result_value_out is null");
    debug_assert!(!result_tydesc.is_null(), "result_tydesc is null");
    debug_assert_aligned(tensor_value_in, tensor_tydesc, "tensor_reshape:tensor");
    debug_assert_aligned(new_shape_in, new_shape_tydesc, "tensor_reshape:shape");
    debug_assert_aligned(result_value_out, result_tydesc, "tensor_reshape:result");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        let new_shape_tydesc_ref = rtdt::TyDescRef::from_ptr(new_shape_tydesc);
        let result_tydesc_ref = rtdt::TyDescRef::from_ptr(result_tydesc);
        crate::impls::tensor::tensor_reshape_impl(
            rt_ref,
            tensor_value_in,
            tensor_tydesc_ref,
            new_shape_in,
            new_shape_tydesc_ref,
            result_value_out,
            result_tydesc_ref,
        )
    }
}

/// Initialize a tensor from contiguous element data and raw shape array.
///
/// This is a simpler interface for AOT compilation where the shape is known
/// at compile time. The element data must be contiguous and contain exactly
/// `product(shape[0..rank])` elements.
///
/// # Arguments
/// * `rt` - Runtime handle
/// * `element_data_in` - Pointer to contiguous element data (will be moved/consumed)
/// * `element_count` - Number of elements in the data array
/// * `element_tydesc` - Type descriptor for elements
/// * `shape_ptr` - Pointer to array of u32 dimension sizes
/// * `rank` - Number of dimensions
/// * `tensor_value_out` - Output tensor location
/// * `tensor_tydesc` - Type descriptor for the tensor type
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_init_local(
    rt: LocalRtHandle,
    element_data_in: *mut u8,
    element_count: rtdt::IndexRepr,
    element_tydesc: *const rtdt::TyDesc,
    shape_ptr: *const u32,
    rank: u32,
    tensor_value_out: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!element_data_in.is_null() || element_count == 0, "element_data_in is null");
    debug_assert!(!element_tydesc.is_null(), "element_tydesc is null");
    debug_assert!(!shape_ptr.is_null() || rank == 0, "shape_ptr is null");
    debug_assert!(!tensor_value_out.is_null(), "tensor_value_out is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        crate::impls::tensor::tensor_init_impl(
            rt_ref,
            element_data_in,
            rtdt::Index(element_count),
            element_tydesc_ref,
            shape_ptr,
            rank,
            tensor_value_out,
            tensor_tydesc_ref,
        )
    }
}

/// Deep-clone a hyperplane (axis-0 slice) of a tensor into a new owned tensor.
///
/// Given a tensor reference and an axis-0 index, produces an owned sub-tensor
/// with rank-1 dimensions, contiguous row-major layout.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_hyperplane_clone_local(
    rt: LocalRtHandle,
    tensor_value_ref: *const u8,
    tensor_tydesc: *const rtdt::TyDesc,
    axis0_index: rtdt::IndexRepr,
    sub_tensor_out: *mut u8,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!tensor_value_ref.is_null(), "tensor_value_ref is null");
    debug_assert!(!tensor_tydesc.is_null(), "tensor_tydesc is null");
    debug_assert!(!sub_tensor_out.is_null(), "sub_tensor_out is null");
    debug_assert_aligned(tensor_value_ref, tensor_tydesc, "tensor_hyperplane_clone:tensor");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ptr(tensor_tydesc);
        crate::impls::tensor::tensor_hyperplane_clone_impl(
            rt_ref,
            tensor_value_ref,
            tensor_tydesc_ref,
            axis0_index,
            sub_tensor_out,
        )
    }
}

// ============================================================================
// Table Operations
// ============================================================================

/// Create an empty table.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_create_local(
    rt: LocalRtHandle,
    value_out: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_out, tydesc, "table_create:value_out");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::table::table_create_impl(rt, value_out, tydesc_ref)
    }
}

/// Destroy a table and all its elements.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_in.is_null(), "value_in is null");
    debug_assert!(!tydesc.is_null(), "tydesc is null");
    debug_assert_aligned(value_in, tydesc, "table_destroy:value");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::table::table_destroy_impl(rt, value_in, tydesc_ref)
    }
}

/// Push a row to a table.
///
/// The row is passed as a tuple with one field per column.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_push_row_local(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    table_tydesc: *const rtdt::TyDesc,
    row_ref: *const u8,
    row_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!table_mut.is_null(), "table_mut is null");
    debug_assert!(!table_tydesc.is_null(), "table_tydesc is null");
    debug_assert!(!row_ref.is_null(), "row_ref is null");
    debug_assert!(!row_tydesc.is_null(), "row_tydesc is null");
    debug_assert_aligned(table_mut, table_tydesc, "table_push_row:table");
    debug_assert_aligned(row_ref, row_tydesc, "table_push_row:row");
    unsafe {
        let table_tydesc_ref = rtdt::TyDescRef::from_ptr(table_tydesc);
        let row_tydesc_ref = rtdt::TyDescRef::from_ptr(row_tydesc);
        crate::impls::table::table_push_row_impl(
            rt,
            table_mut,
            table_tydesc_ref,
            row_ref,
            row_tydesc_ref,
        )
    }
}

/// Builds a table by moving rows from a contiguous array of row tuples.
///
/// The `rows_ptr` points to an array of row tuples (row-major layout).
/// Rows are moved (not cloned) into the table's columnar storage.
/// After this call, the source buffer is consumed and should not be destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_build_from_rows_local(
    rt: LocalRtHandle,
    table_out: *mut u8,
    table_tydesc: *const rtdt::TyDesc,
    rows_ptr: *mut u8,
    row_tydesc: *const rtdt::TyDesc,
    num_rows: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!table_out.is_null(), "table_out is null");
    debug_assert!(!table_tydesc.is_null(), "table_tydesc is null");
    debug_assert!(num_rows == 0 || !rows_ptr.is_null(), "rows_ptr is null");
    debug_assert!(!row_tydesc.is_null(), "row_tydesc is null");
    debug_assert_aligned(rows_ptr, row_tydesc, "table_build_from_rows:rows");
    unsafe {
        let table_tydesc_ref = rtdt::TyDescRef::from_ptr(table_tydesc);
        let row_tydesc_ref = rtdt::TyDescRef::from_ptr(row_tydesc);
        crate::impls::table::table_build_from_rows_impl(
            rt,
            table_out,
            table_tydesc_ref,
            rows_ptr,
            row_tydesc_ref,
            num_rows,
        )
    }
}

/// Get a pointer to an element at (row, col).
///
/// Returns null if row or col is out of bounds.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_get_local(
    _rt: LocalRtHandle,
    table_ref: *const u8,
    table_tydesc: *const rtdt::TyDesc,
    row: rtdt::IndexRepr,
    col: u32,
) -> *const u8 {
    debug_assert!(!table_ref.is_null(), "table_ref is null");
    debug_assert!(!table_tydesc.is_null(), "table_tydesc is null");
    debug_assert_aligned(table_ref, table_tydesc, "table_get:table");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(table_tydesc);
        crate::impls::table::table_get_element_ptr(table_ref, tydesc_ref, row, col)
    }
}

/// Set an element at (row, col).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_set_local(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    table_tydesc: *const rtdt::TyDesc,
    row: rtdt::IndexRepr,
    col: u32,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!table_mut.is_null(), "table_mut is null");
    debug_assert!(!table_tydesc.is_null(), "table_tydesc is null");
    debug_assert!(!value_ref.is_null(), "value_ref is null");
    debug_assert!(!value_tydesc.is_null(), "value_tydesc is null");
    debug_assert_aligned(table_mut, table_tydesc, "table_set:table");
    debug_assert_aligned(value_ref, value_tydesc, "table_set:value");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(table_tydesc);
        crate::impls::table::table_set_element(
            rt,
            table_mut,
            tydesc_ref,
            row,
            col,
            value_ref,
            value_tydesc,
        )
    }
}

/// Clear a table, destroying all elements but keeping capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_clear_local(
    rt: LocalRtHandle,
    table_mut: *mut u8,
    table_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!table_mut.is_null(), "table_mut is null");
    debug_assert!(!table_tydesc.is_null(), "table_tydesc is null");
    debug_assert_aligned(table_mut, table_tydesc, "table_clear:table");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(table_tydesc);
        crate::impls::table::table_clear_impl(rt, table_mut, tydesc_ref)
    }
}

/// Get the length (number of rows) of a table.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_len(
    _rt: LocalRtHandle,
    table_ref: *const u8,
    _table_tydesc: *const rtdt::TyDesc,
) -> rtdt::IndexRepr {
    debug_assert!(!table_ref.is_null(), "table_ref is null");
    unsafe {
        crate::impls::table::table_len(table_ref).0
    }
}

// ============================================================================
// Debug Log Operations
// ============================================================================

pub use rt_local::DebugOutputMode;

/// Set the debug output mode.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_set_debug_mode(
    rt: LocalRtHandle,
    mode: DebugOutputMode,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        rt_ref.debug_output_mode = mode;
    }
    RtStatus::Ok
}

/// Debug log a value (borrows, does not consume).
///
/// Pretty-prints the value and outputs according to the current debug mode.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_debuglog_local(
    rt: LocalRtHandle,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!value_ref.is_null(), "value_ref is null");
    debug_assert!(!value_tydesc.is_null(), "value_tydesc is null");
    debug_assert_aligned(value_ref, value_tydesc, "debuglog_local:value");
    unsafe {
        crate::impls::debuglog::debuglog_local(rt, value_ref, value_tydesc)
    }
}

/// Get a pointer to the debug buffer contents.
///
/// Returns a pointer to the UTF-8 bytes and the length.
/// The pointer is valid until the next debuglog or clear operation.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_get_debug_buffer(
    rt: LocalRtHandle,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!out_ptr.is_null(), "out_ptr is null");
    debug_assert!(!out_len.is_null(), "out_len is null");
    unsafe {
        let rt_ref = &*(rt as *const rt_local::RtLocal);
        *out_ptr = rt_ref.debug_buffer.as_ptr();
        *out_len = rt_ref.debug_buffer.len();
    }
    RtStatus::Ok
}

/// Clear the debug buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_clear_debug_buffer(
    rt: LocalRtHandle,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        rt_ref.debug_buffer.clear();
    }
    RtStatus::Ok
}

