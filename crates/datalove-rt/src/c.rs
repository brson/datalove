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
//! Functions return `RtStatus::Error` on null required params.
//! Argument pointers should never be null in correct generated code.

use crate::rtdt;
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
    if rt.is_null() {
        return RtStatus::Error;
    }

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
    count: u32,
) -> *mut u8 {
    if rt.is_null() || tydesc.is_null() {
        return std::ptr::null_mut();
    }

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
    count: u32,
) -> *mut u8 {
    if rt.is_null() {
        return std::ptr::null_mut();
    }

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
    count: u32,
    ptr: *mut u8,
) -> RtStatus {
    if rt.is_null() {
        return RtStatus::Error;
    }

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
    count: u32,
    ptr: *mut u8
) -> RtStatus {
    if rt.is_null() || tydesc.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let ty = &*tydesc;
        rt_ref.alloc.free(ty.size, ty.align, count, ptr);
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
    _a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    _b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

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
    _a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    _b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

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
    _a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    _b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

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
    _a_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

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
    _a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    _b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    _result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::int_math::int_div_checked_impl(rt_ref, a_in, b_in, result_out)
    }
}

/// Compare two bigints.
///
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
/// Does not require runtime handle since no allocation is performed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_cmp(
    a_in: *const u8,
    b_in: *const u8,
) -> i32 {
    if a_in.is_null() || b_in.is_null() {
        return 0; // Treat null as zero for comparison.
    }

    unsafe {
        crate::impls::int_math::int_cmp_impl(a_in, b_in)
    }
}

/// Destroys any type of value, freeing allocations recursively.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_any_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        crate::impls::destroy::any_destroy_local(rt, value_in, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
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
    bytes_len: u32,
) -> RtStatus {
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
    unsafe {
        crate::impls::string::string_clear_local(rt, string_value_mut, string_tydesc)
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
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

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
    slice_len: u32,
    // Should be a tuple of key/value I guess.
    slice_element_tydesc: *const rtdt::TyDesc,
    // Destination will be overwritten.
    btreemap_value_out: *mut u8,
    // BTreeMap type.
    btreemap_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || btreemap_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    num_entries: u32,
) -> RtStatus {
    unsafe {
        if rt.is_null() || key_tydesc.is_null() || value_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreemap_tydesc.is_null()
            || key_tydesc.is_null() || value_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreemap_tydesc.is_null() || key_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreemap_value_ref.is_null() || btreemap_tydesc.is_null()
            || key_ref.is_null() || key_tydesc.is_null()
            || option_value_out.is_null() || option_tydesc.is_null() {
            return RtStatus::Error;
        }

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
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_clear_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || btreemap_tydesc.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::btreemap::btreemap_clear_impl(rt_ref, btreemap_value_mut, rtdt::TyDescRef::from_ptr(btreemap_tydesc))
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
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        crate::impls::set::btreeset_clear_impl(rt_ref, btreeset_value_mut, btreeset_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_clone_from_slice_local(
    rt: LocalRtHandle,
    slice_ref: *const u8,
    slice_len: u32,
    slice_element_tydesc: *const rtdt::TyDesc,
    btreeset_value_out: *mut u8,
    btreeset_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || btreeset_tydesc.is_null() || slice_element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    num_elements: u32,
) -> RtStatus {
    unsafe {
        if rt.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

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
    slice_len: u32,
    element_tydesc: *const rtdt::TyDesc,
    list_value_out: *mut u8,
    list_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_out.is_null() || list_tydesc.is_null()
            || slice_ref.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut rt_local::RtLocal);
        let tydesc_ref = rtdt::TyDescRef::from_ptr(tydesc);
        crate::impls::list::list_clear_impl(rt_ref, value_mut, tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_get_local(
    rt: LocalRtHandle,
    list_value_ref: *const u8,
    list_tydesc: *const rtdt::TyDesc,
    index: u32,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_ref.is_null() || list_tydesc.is_null()
            || option_value_out.is_null() || option_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    index: u32,
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || element_in.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || element_in.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || option_value_out.is_null() || option_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    index: u32,
    element_in: *mut u8,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || element_in.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    index: u32,
    option_value_out: *mut u8,
    option_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || option_value_out.is_null() || option_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    additional: u32,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    slice_len: u32,
    element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || list_value_mut.is_null() || list_tydesc.is_null()
            || slice_ref.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    slice_len: u32,
    element_tydesc: *const rtdt::TyDesc,
    shape_in: *mut u8,
    shape_tydesc: *const rtdt::TyDesc,
    layout: u8,
    tensor_value_out: *mut u8,
    tensor_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() || tensor_value_out.is_null() || tensor_tydesc.is_null()
            || slice_ref.is_null() || element_tydesc.is_null()
            || shape_in.is_null() || shape_tydesc.is_null() {
            return RtStatus::Error;
        }

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
            slice_len,
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
    unsafe {
        if rt.is_null() || tensor_value_in.is_null() || tensor_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tensor_value_ref.is_null() || tensor_tydesc.is_null()
            || indices_ptr.is_null() || element_value_out.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tensor_value_ref.is_null() || tensor_tydesc.is_null()
            || indices_ptr.is_null() || element_ref.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tensor_ref.is_null() || tensor_tydesc_ref.is_null()
            || perm_ptr.is_null() || tensor_value_out.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tensor_value_in.is_null() || tensor_tydesc.is_null()
            || ranges_ptr.is_null() || result_value_out.is_null() || result_tydesc.is_null() {
            return RtStatus::Error;
        }

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
    unsafe {
        if rt.is_null() || tensor_value_in.is_null() || tensor_tydesc.is_null()
            || new_shape_in.is_null() || new_shape_tydesc.is_null()
            || result_value_out.is_null() || result_tydesc.is_null() {
            return RtStatus::Error;
        }

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

