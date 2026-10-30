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
mod impls;

use impls::rt_local;

// The ABI's own types come from `datalove-rti`, which declares this interface
// for callers that link a runtime rather than containing one. They are defined
// there rather than here so that a rider and a runtime cannot hold different
// ideas of what an `RtStatus` is.
pub use datalove_rti::{LocalRtHandle, RtEq, RtOrdering, RtStatus};
pub use datalove_rtdt::{AnyMut, AnyOut, AnyOwned, AnyRef};
pub use datalove_rti::{RtHeader, ABANDONED_FOR_ERROR, ABANDONED_FOR_STACK};

/// Whether two descriptors describe the same type.
///
/// Compared structurally rather than by address. Descriptors are not
/// deduplicated: the compiler emits one wherever a module names a type, and
/// the runtime owns constants for the types that pack into a `data`'s own
/// words and so have none of their own to lend. Two descriptions of one type
/// sit at different addresses and still describe it.
fn same_tydesc(a: &rtdt::TyDesc, b: &rtdt::TyDesc) -> bool {
    crate::c::impls::cmp::eq_tydesc(rtdt::TyDescRef::from_ref(a), rtdt::TyDescRef::from_ref(b))
}

/// Hold the values an export is handed to what `rtdt::any` asks of a call: no
/// value it changes, takes over or writes overlaps another it is handed.
///
/// In a debug build the bytes are compared, and an overlap panics. Then each
/// value the export changes, takes or writes is rebound to a pointer derived
/// from a `&mut` of its bytes, which is nothing at run time and under Miri
/// makes reaching those bytes through any other argument a reported error.
///
/// Every export with such a value and another beside it begins with this,
/// naming each value and its size; `gen-rti` refuses one that doesn't.
macro_rules! lend {
    ($function:literal; $($value:ident: $size:expr),+ $(,)?) => {
        if cfg!(debug_assertions) {
            check_disjoint($function, &[$(Lend::lent(&$value, stringify!($value), $size)),+]);
        }
        $(let $value = unsafe { Lend::hold($value, $size) };)+
    };
}

/// One value an export is handed, as `lend!` sees it.
struct Lent {
    name: &'static str,
    at: usize,
    size: usize,
    exclusive: bool,
}

/// What `lend!` does with each kind of value.
trait Lend: Sized {
    fn lent(&self, name: &'static str, size: usize) -> Lent;

    /// The value, held to its bytes if the export changes, takes or writes
    /// it.
    ///
    /// # Safety
    ///
    /// As for `AnyMut::exclusive`.
    unsafe fn hold(self, size: usize) -> Self;
}

impl Lend for rtdt::AnyRef<'_> {
    fn lent(&self, name: &'static str, size: usize) -> Lent {
        Lent { name, at: self.as_ptr().addr(), size, exclusive: false }
    }

    unsafe fn hold(self, _size: usize) -> Self {
        self
    }
}

macro_rules! lend_exclusive {
    ($($ty:ident),+) => {$(
        impl Lend for rtdt::$ty<'_> {
            fn lent(&self, name: &'static str, size: usize) -> Lent {
                Lent { name, at: self.as_ptr().addr(), size, exclusive: true }
            }

            unsafe fn hold(self, size: usize) -> Self {
                unsafe { self.exclusive(size) }
            }
        }
    )+};
}

lend_exclusive!(AnyMut, AnyOwned, AnyOut);

/// Panic if a value `function` changes, takes or writes overlaps another.
fn check_disjoint(function: &str, values: &[Lent]) {
    for (i, a) in values.iter().enumerate() {
        for b in &values[i + 1..] {
            let overlap = a.size > 0 && b.size > 0 && a.at < b.at + b.size && b.at < a.at + a.size;
            assert!(
                !(overlap && (a.exclusive || b.exclusive)),
                "{function}: `{}` overlaps `{}`, and one of them is the function's to change",
                a.name, b.name,
            );
        }
    }
}

/// Debug assertion to verify pointer alignment matches tydesc requirements.
#[inline]
fn debug_assert_aligned(ptr: *const u8, tydesc: &rtdt::TyDesc, name: &str) {
    if cfg!(debug_assertions) {
        let align = tydesc.align as usize;
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

/// Free everything the runtime has allocated, live or not, at once, without
/// reporting it as leaked, and carry on with the same handle as if new.
///
/// For after an error: datalove has no side effects, so what an error unwinds
/// through need not be destroyed in any order, only freed, and freeing all of
/// it is simplest. Every value the runtime made is gone, those held by
/// whoever called it included. The debug mode and buffered output are kept.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_reset(rt: LocalRtHandle) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe { rt_local::from_handle(rt).reset() };
    RtStatus::Ok
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
    tydesc: &rtdt::TyDesc,
    count: rtdt::IndexRepr,
) -> *mut u8 {
    debug_assert!(!rt.is_null(), "rt is null");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        rt_ref.alloc.alloc(tydesc.size, tydesc.align, count)
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
        let rt_ref = rt_local::from_handle(rt);
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
        let rt_ref = rt_local::from_handle(rt);
        rt_ref.alloc.free(size, align, count, ptr);
    }

    RtStatus::Ok
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_mem_free_local(
    rt: LocalRtHandle,
    tydesc: &rtdt::TyDesc,
    count: rtdt::IndexRepr,
    ptr: *mut u8
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!ptr.is_null(), "ptr is null");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        rt_ref.alloc.free(tydesc.size, tydesc.align, count, ptr);
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
    src_ref: rtdt::AnyOwned<'_>,
    tydesc: &rtdt::TyDesc,
    dst_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("move_value_local";
        src_ref: tydesc.size as usize,
        dst_out: tydesc.size as usize,
    );
    debug_assert_aligned(src_ref.as_ptr(), tydesc, "move_value_local:src");
    debug_assert_aligned(dst_out.as_ptr(), tydesc, "move_value_local:dst");
    unsafe {
        let size = tydesc.size as usize;
        rtdt::bytes::copy_value(src_ref.as_ptr(), dst_out.as_ptr(), size);
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
    value_in: rtdt::AnyRef<'_>,
    tydesc_in: &rtdt::TyDesc,
    value_out: rtdt::AnyOut<'_>,
    tydesc_out: &rtdt::TyDesc,
) -> RtStatus {
    lend!("clone_local";
        value_in: tydesc_in.size as usize,
        value_out: tydesc_out.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc_in, "clone_local:value_in.as_ptr()");
    debug_assert_aligned(value_out.as_ptr(), tydesc_out, "clone_local:value_out.as_ptr()");
    debug_assert!(
        same_tydesc(tydesc_in, tydesc_out),
        "clone: tydesc_in and tydesc_out must describe the same type"
    );
    unsafe {
        crate::c::impls::clone::clone_value(rt_local::from_handle(rt), value_in.as_ptr(), tydesc_in, value_out.as_ptr())
    }
}

/// Standard equality.
///
/// Floats have weird cases.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_eq_local(
    _rt: LocalRtHandle,
    value_a_ref: rtdt::AnyRef<'_>,
    value_a_tydesc: &rtdt::TyDesc,
    value_b_ref: rtdt::AnyRef<'_>,
    value_b_tydesc: &rtdt::TyDesc,
) -> RtEq {
    debug_assert_aligned(value_a_ref.as_ptr(), value_a_tydesc, "eq_local:value_a");
    debug_assert_aligned(value_b_ref.as_ptr(), value_b_tydesc, "eq_local:value_b");
    // Note that the runtime handle isn't needed
    // because we don't allocate - it's just part
    // of the ABI.
    unsafe {
        crate::c::impls::cmp::eq(value_a_ref.as_ptr(), value_a_tydesc, value_b_ref.as_ptr(), value_b_tydesc)
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
    value_a_ref: rtdt::AnyRef<'_>,
    value_a_tydesc: &rtdt::TyDesc,
    value_b_ref: rtdt::AnyRef<'_>,
    value_b_tydesc: &rtdt::TyDesc,
) -> RtEq {
    debug_assert_aligned(value_a_ref.as_ptr(), value_a_tydesc, "eq_unique_local:value_a");
    debug_assert_aligned(value_b_ref.as_ptr(), value_b_tydesc, "eq_unique_local:value_b");
    unsafe {
        crate::c::impls::cmp::eq_unique(value_a_ref.as_ptr(), value_a_tydesc, value_b_ref.as_ptr(), value_b_tydesc)
    }
}

/// Establish ordering.
///
/// The same relation as `dtlv_rti_cmp_total_local`, which is the only one this
/// language has. Kept as its own symbol because the backends declare it for
/// the bigint comparisons, where no float is in reach and the two never
/// differed anyway.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_cmp_local(
    _rt: LocalRtHandle,
    value_a_ref: rtdt::AnyRef<'_>,
    value_a_tydesc: &rtdt::TyDesc,
    value_b_ref: rtdt::AnyRef<'_>,
    value_b_tydesc: &rtdt::TyDesc,
) -> RtOrdering {
    debug_assert_aligned(value_a_ref.as_ptr(), value_a_tydesc, "cmp_local:value_a");
    debug_assert_aligned(value_b_ref.as_ptr(), value_b_tydesc, "cmp_local:value_b");
    unsafe {
        crate::c::impls::cmp::cmp_total(value_a_ref.as_ptr(), value_a_tydesc, value_b_ref.as_ptr(), value_b_tydesc)
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
    value_a_ref: rtdt::AnyRef<'_>,
    value_a_tydesc: &rtdt::TyDesc,
    value_b_ref: rtdt::AnyRef<'_>,
    value_b_tydesc: &rtdt::TyDesc,
) -> RtOrdering {
    debug_assert_aligned(value_a_ref.as_ptr(), value_a_tydesc, "cmp_total_local:value_a");
    debug_assert_aligned(value_b_ref.as_ptr(), value_b_tydesc, "cmp_total_local:value_b");
    unsafe {
        crate::c::impls::cmp::cmp_total(value_a_ref.as_ptr(), value_a_tydesc, value_b_ref.as_ptr(), value_b_tydesc)
    }
}

// ============================================================================
// Bigint arithmetic operations
// ============================================================================

/// Add two bigints: a + b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_add(
    rt: LocalRtHandle,
    a_in: rtdt::AnyRef<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_add";
        a_in: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
        result_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_in.as_ptr(), a_tydesc, "int_add:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_add:b");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "int_add:result");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_add_impl(rt_ref, a_in.as_ptr(), b_in.as_ptr(), result_out.as_ptr())
    }
}

/// Subtract two bigints: a - b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_sub(
    rt: LocalRtHandle,
    a_in: rtdt::AnyRef<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_sub";
        a_in: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
        result_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_in.as_ptr(), a_tydesc, "int_sub:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_sub:b");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "int_sub:result");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_sub_impl(rt_ref, a_in.as_ptr(), b_in.as_ptr(), result_out.as_ptr())
    }
}

/// Multiply two bigints: a * b.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_mul(
    rt: LocalRtHandle,
    a_in: rtdt::AnyRef<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_mul";
        a_in: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
        result_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_in.as_ptr(), a_tydesc, "int_mul:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_mul:b");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "int_mul:result");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_mul_impl(rt_ref, a_in.as_ptr(), b_in.as_ptr(), result_out.as_ptr())
    }
}

/// Negate a bigint: -a.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_neg(
    rt: LocalRtHandle,
    a_in: rtdt::AnyRef<'_>,
    a_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_neg";
        a_in: a_tydesc.size as usize,
        result_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_in.as_ptr(), a_tydesc, "int_neg:a");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "int_neg:result");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_neg_impl(rt_ref, a_in.as_ptr(), result_out.as_ptr())
    }
}

/// Divide two bigints: a / b.
/// Returns RtStatus::Error if b is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_div_checked(
    rt: LocalRtHandle,
    a_in: rtdt::AnyRef<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_div_checked";
        a_in: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
        result_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_in.as_ptr(), a_tydesc, "int_div_checked:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_div_checked:b");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "int_div_checked:result");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_div_checked_impl(rt_ref, a_in.as_ptr(), b_in.as_ptr(), result_out.as_ptr())
    }
}

/// Add a bigint into another in place: a = a + b.
///
/// `b` must not overlap `a`, as no other value argument may overlap one that is
/// changed (see `rtdt::any`).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_add_assign(
    rt: LocalRtHandle,
    a_mut: rtdt::AnyMut<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_add_assign";
        a_mut: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_mut.as_ptr(), a_tydesc, "int_add_assign:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_add_assign:b");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_add_assign_impl(rt_ref, a_mut.as_ptr(), b_in.as_ptr())
    }
}

/// Subtract a bigint from another in place: a = a - b.
///
/// `b` must not overlap `a`, as no other value argument may overlap one that is
/// changed (see `rtdt::any`).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_sub_assign(
    rt: LocalRtHandle,
    a_mut: rtdt::AnyMut<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_sub_assign";
        a_mut: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_mut.as_ptr(), a_tydesc, "int_sub_assign:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_sub_assign:b");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_sub_assign_impl(rt_ref, a_mut.as_ptr(), b_in.as_ptr())
    }
}

/// Multiply a bigint by another in place: a = a * b.
///
/// `b` must not overlap `a`, as no other value argument may overlap one that is
/// changed (see `rtdt::any`).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_mul_assign(
    rt: LocalRtHandle,
    a_mut: rtdt::AnyMut<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_mul_assign";
        a_mut: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_mut.as_ptr(), a_tydesc, "int_mul_assign:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_mul_assign:b");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_mul_assign_impl(rt_ref, a_mut.as_ptr(), b_in.as_ptr())
    }
}

/// Divide a bigint by another in place: a = a / b.
///
/// Returns RtStatus::Error, leaving `a` as it was, if b is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_div_assign_checked(
    rt: LocalRtHandle,
    a_mut: rtdt::AnyMut<'_>,
    a_tydesc: &rtdt::TyDesc,
    b_in: rtdt::AnyRef<'_>,
    b_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_div_assign_checked";
        a_mut: a_tydesc.size as usize,
        b_in: b_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(a_mut.as_ptr(), a_tydesc, "int_div_assign_checked:a");
    debug_assert_aligned(b_in.as_ptr(), b_tydesc, "int_div_assign_checked:b");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_div_assign_checked_impl(rt_ref, a_mut.as_ptr(), b_in.as_ptr())
    }
}

/// Widen a fixed-width integer to Int (bigint).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_int_from_fixed(
    rt: LocalRtHandle,
    src_in: rtdt::AnyRef<'_>,
    src_tydesc: &rtdt::TyDesc,
    result_out: rtdt::AnyOut<'_>,
    _result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("int_from_fixed";
        src_in: src_tydesc.size as usize,
        result_out: _result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_from_fixed_impl(rt_ref, src_in.as_ptr(), src_tydesc, result_out.as_ptr())
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
    result_out: rtdt::AnyOut<'_>,
    _result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(limb_count == 0 || !limbs_ptr.is_null(), "limbs_ptr is null with non-zero count");

    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::int_math::int_from_limbs_impl(rt_ref, limbs_ptr, limb_count, negative, result_out.as_ptr())
    }
}

/// Destroys any type of value, freeing allocations recursively.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_any_destroy_local(
    rt: LocalRtHandle,
    value_in: rtdt::AnyOwned<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc, "any_destroy_local:value");
    unsafe {
        crate::c::impls::destroy::any_destroy_local(rt_local::from_handle(rt), value_in.as_ptr(), tydesc)
    }
}

/// Creates an Error from any value (moves inner to heap).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_error_from_local(
    rt: LocalRtHandle,
    inner_in: rtdt::AnyOwned<'_>,
    inner_tydesc: &rtdt::TyDesc,
    dest_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("error_from_local";
        inner_in: inner_tydesc.size as usize,
        dest_out: size_of::<rtdt::Error>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(inner_in.as_ptr(), inner_tydesc, "error_from_local:inner");
    unsafe {
        crate::c::impls::boxing::error_from_local(rt_local::from_handle(rt), inner_in.as_ptr(), inner_tydesc, dest_out.as_ptr())
    }
}

/// Creates a Data from any value (moves inner to heap).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_data_from_local(
    rt: LocalRtHandle,
    inner_in: rtdt::AnyOwned<'_>,
    inner_tydesc: &rtdt::TyDesc,
    dest_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("data_from_local";
        inner_in: inner_tydesc.size as usize,
        dest_out: size_of::<rtdt::Data>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(inner_in.as_ptr(), inner_tydesc, "data_from_local:inner");
    unsafe {
        crate::c::impls::boxing::data_from_local(rt_local::from_handle(rt), inner_in.as_ptr(), inner_tydesc, dest_out.as_ptr())
    }
}

/// Apply an operator to two numbers whose type only their descriptors say.
///
/// `op` is a `rtdt::DynOp`. See `dyn_ops`: this is how a generic bounded to
/// `float` or `fixedint` operates on its type parameter, since one compiled
/// body cannot hold the instruction for every type the bound admits.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_binop(
    _rt: LocalRtHandle,
    op: u8,
    lhs: rtdt::AnyRef<'_>,
    lhs_tydesc: &rtdt::TyDesc,
    rhs: rtdt::AnyRef<'_>,
    rhs_tydesc: &rtdt::TyDesc,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("dyn_binop";
        lhs: lhs_tydesc.size as usize,
        rhs: rhs_tydesc.size as usize,
        out: out_tydesc.size as usize,
    );
    let Some(op) = rtdt::DynOp::from_code(op) else {
        return RtStatus::Error;
    };
    unsafe {
        crate::c::impls::dyn_ops::dyn_binop(
            op, lhs.as_ptr(), lhs_tydesc, rhs.as_ptr(), rhs_tydesc, out.as_ptr(), out_tydesc)
    }
}

/// Apply a checked operator to two integers whose type only their descriptors
/// say, writing to `overflow_out` whether the answer fit.
///
/// A fixed-width integer has no bare arithmetic, so this rather than
/// `dtlv_rti_dyn_binop` is where a `T is fixedint` adds.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_binop_checked(
    _rt: LocalRtHandle,
    op: u8,
    lhs: rtdt::AnyRef<'_>,
    lhs_tydesc: &rtdt::TyDesc,
    rhs: rtdt::AnyRef<'_>,
    rhs_tydesc: &rtdt::TyDesc,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
    overflow_out: *mut bool,
) -> RtStatus {
    lend!("dyn_binop_checked";
        lhs: lhs_tydesc.size as usize,
        rhs: rhs_tydesc.size as usize,
        out: out_tydesc.size as usize,
    );
    debug_assert!(!overflow_out.is_null(), "overflow_out is null");
    let Some(op) = rtdt::DynOp::from_code(op) else {
        return RtStatus::Error;
    };
    unsafe {
        crate::c::impls::dyn_ops::dyn_binop_checked(
            op, lhs.as_ptr(), lhs_tydesc, rhs.as_ptr(), rhs_tydesc, out.as_ptr(), out_tydesc, overflow_out)
    }
}

/// Apply a one-operand operation to a number whose type only its descriptor
/// says. `op` is a `rtdt::DynUnOp`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_unop(
    _rt: LocalRtHandle,
    op: u8,
    value: rtdt::AnyRef<'_>,
    value_tydesc: &rtdt::TyDesc,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("dyn_unop";
        value: value_tydesc.size as usize,
        out: out_tydesc.size as usize,
    );
    let Some(op) = rtdt::DynUnOp::from_code(op) else {
        return RtStatus::Error;
    };
    unsafe { crate::c::impls::dyn_ops::dyn_unop(op, value.as_ptr(), value_tydesc, out.as_ptr(), out_tydesc) }
}

/// Make a constant at the type `value_tydesc` names.
///
/// `which` is a `rtdt::DynConst`. The descriptor is the one the call site had
/// to hand over, because a type parameter appearing only in a native's return
/// type has no argument to bring it along.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_const(
    _rt: LocalRtHandle,
    which: u8,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    let Some(which) = rtdt::DynConst::from_code(which) else {
        return RtStatus::Error;
    };
    unsafe { crate::c::impls::dyn_ops::dyn_const(which, out.as_ptr(), out_tydesc, value_tydesc) }
}

/// Make a float constant at the width `value_tydesc` names.
///
/// `which` is a `rtdt::DynFloatConst`. As `dtlv_rti_dyn_const` is for the
/// integers, and for the same reason.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_float_const(
    _rt: LocalRtHandle,
    which: u8,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    let Some(which) = rtdt::DynFloatConst::from_code(which) else {
        return RtStatus::Error;
    };
    unsafe { crate::c::impls::dyn_ops::dyn_float_const(which, out.as_ptr(), out_tydesc, value_tydesc) }
}

/// Negate an integer whose type only its descriptor says, writing to
/// `overflow_out` whether the answer fit.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_dyn_neg_checked(
    _rt: LocalRtHandle,
    value: rtdt::AnyRef<'_>,
    value_tydesc: &rtdt::TyDesc,
    out: rtdt::AnyOut<'_>,
    out_tydesc: &rtdt::TyDesc,
    overflow_out: *mut bool,
) -> RtStatus {
    lend!("dyn_neg_checked";
        value: value_tydesc.size as usize,
        out: out_tydesc.size as usize,
    );
    debug_assert!(!overflow_out.is_null(), "overflow_out is null");
    unsafe {
        crate::c::impls::dyn_ops::dyn_neg_checked(
            value.as_ptr(), value_tydesc, out.as_ptr(), out_tydesc, overflow_out)
    }
}

/// Borrow what a Data holds, unpacking into `scratch` when it has to.
///
/// A `data` pointing to its value lends that value's address; one keeping it
/// in its own words unpacks it into `scratch`, which must be eight writable
/// bytes, aligned to eight, that outlive the borrow: a `rust::DataScratch`, to
/// a Rust caller. See `boxing::data_borrow`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_data_borrow(
    data_in: *const u8,
    scratch: *mut u8,
    value_out: *mut *const u8,
    tydesc_out: *mut *const rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!data_in.is_null(), "data_in is null");
    debug_assert!(!scratch.is_null(), "scratch is null");
    debug_assert!(!value_out.is_null(), "value_out is null");
    debug_assert!(!tydesc_out.is_null(), "tydesc_out is null");
    // Made a reference here, at the boundary, where the doc above holds the
    // caller to the alignment.
    unsafe {
        let (value, tydesc) = crate::c::impls::boxing::data_borrow(
            &*(data_in as *const rtdt::Data), &mut *(scratch as *mut crate::rust::DataScratch));
        std::ptr::write(value_out, value);
        std::ptr::write(tydesc_out, tydesc.as_ptr());
    }
    RtStatus::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: rtdt::AnyOut<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_out.as_ptr(), tydesc, "string_create_local:value_out.as_ptr()");
    unsafe {
        crate::c::impls::string::string_create_local(value_out.as_ptr(), tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_destroy_local(
    rt: LocalRtHandle,
    // Pointer will be freed.
    value_in: rtdt::AnyOwned<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc, "string_destroy_local:value");
    unsafe {
        crate::c::impls::string::string_destroy_local(rt_local::from_handle(rt), value_in.as_ptr(), tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_push_bytes_local(
    rt: LocalRtHandle,
    string_value_mut: rtdt::AnyMut<'_>,
    string_tydesc: &rtdt::TyDesc,
    bytes_ref: *const u8,
    bytes_len: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(bytes_len == 0 || !bytes_ref.is_null(), "bytes_ref is null");
    debug_assert_aligned(string_value_mut.as_ptr(), string_tydesc, "string_push_bytes_local:string");
    unsafe {
        crate::c::impls::string::string_push_bytes_local(rt_local::from_handle(rt), string_value_mut.as_ptr(), string_tydesc, bytes_ref, bytes_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_string_clear_local(
    rt: LocalRtHandle,
    string_value_mut: rtdt::AnyMut<'_>,
    string_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(string_value_mut.as_ptr(), string_tydesc, "string_clear_local:string");
    unsafe {
        crate::c::impls::string::string_clear_local(string_value_mut.as_ptr(), string_tydesc)
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
    result_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(bytes_len == 0 || !bytes_ptr.is_null(), "bytes_ptr is null with non-zero len");
    debug_assert_aligned(result_out.as_ptr(), result_tydesc, "string_from_bytes:result_out.as_ptr()");
    unsafe {
        crate::c::impls::string::string_from_bytes(rt_local::from_handle(rt), bytes_ptr, bytes_len, result_out.as_ptr(), result_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_pretty_print_local(
    rt: LocalRtHandle,
    arg_value_ref: rtdt::AnyRef<'_>,
    arg_tydesc_ref: &rtdt::TyDesc,
    // String previously allocated by string_create_local
    string_value_mut: rtdt::AnyMut<'_>,
    string_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("pretty_print_local";
        arg_value_ref: arg_tydesc_ref.size as usize,
        string_value_mut: string_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(arg_value_ref.as_ptr(), arg_tydesc_ref, "pretty_print_local:arg");
    debug_assert_aligned(string_value_mut.as_ptr(), string_tydesc, "pretty_print_local:string");
    if rt.is_null() {
        return RtStatus::Error;
    }
    unsafe {
        crate::c::impls::pretty::pretty_print_local(rt_local::from_handle(rt), arg_value_ref.as_ptr(), arg_tydesc_ref, string_value_mut.as_ptr(), string_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: rtdt::AnyOut<'_>,
    // BTreeMap type.
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_out.as_ptr(), tydesc, "btreemap_create_local:value_out.as_ptr()");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::btreemap::btreemap_create_impl(rt_ref, value_out.as_ptr(), tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_clone_from_slice_local(
    rt: LocalRtHandle,
    // Values will be cloned.
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    // Should be a tuple of key/value I guess.
    slice_element_tydesc: &rtdt::TyDesc,
    // Destination will be overwritten.
    btreemap_value_out: rtdt::AnyOut<'_>,
    // BTreeMap type.
    btreemap_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(slice_len == 0 || !slice_ref.is_null(), "slice_ref is null");
    debug_assert_aligned(slice_ref, slice_element_tydesc, "btreemap_clone_from_slice:slice");
    debug_assert_aligned(btreemap_value_out.as_ptr(), btreemap_tydesc, "btreemap_clone_from_slice:btreemap");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        crate::c::impls::btreemap::btreemap_clone_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            slice_element_tydesc,
            btreemap_value_out.as_ptr(),
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
    map_out: rtdt::AnyOut<'_>,
    key_tydesc: &rtdt::TyDesc,
    value_tydesc: &rtdt::TyDesc,
    keys_ptr: *mut u8,
    values_ptr: *mut u8,
    num_entries: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(num_entries == 0 || !keys_ptr.is_null(), "keys_ptr is null");
    debug_assert!(num_entries == 0 || !values_ptr.is_null(), "values_ptr is null");
    debug_assert_aligned(keys_ptr, key_tydesc, "btreemap_build_from_sorted_slices:keys");
    debug_assert_aligned(values_ptr, value_tydesc, "btreemap_build_from_sorted_slices:values");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ref(value_tydesc);
        crate::c::impls::btreemap::btreemap_build_from_sorted_slices(
            rt_ref,
            map_out.as_ptr() as *mut rtdt::Map,
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
    value_in: rtdt::AnyOwned<'_>,
    // BTreMap type.
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc, "btreemap_destroy_local:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::btreemap::btreemap_destroy_impl(rt_ref, value_in.as_ptr(), tydesc_ref)
    }
}

/// Add an amount to the value under a key, counting from zero if it's absent.
///
/// The key is borrowed, and cloned only if it's new. The amount is consumed,
/// as itself or boxed in a `data`. Writes whether the sum fit to `fit_out`;
/// one that didn't leaves the map as it was.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_add_local(
    rt: LocalRtHandle,
    btreemap_value_mut: rtdt::AnyMut<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
    amount_in: rtdt::AnyOwned<'_>,
    amount_tydesc: &rtdt::TyDesc,
    fit_out: *mut bool,
) -> RtStatus {
    lend!("btreemap_add_local";
        btreemap_value_mut: btreemap_tydesc.size as usize,
        key_ref: key_tydesc.size as usize,
        amount_in: amount_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!fit_out.is_null(), "fit_out is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::btreemap::btreemap_add_impl(
            rt_ref,
            btreemap_value_mut.as_ptr(),
            rtdt::TyDescRef::from_ref(btreemap_tydesc),
            key_ref.as_ptr(),
            rtdt::TyDescRef::from_ref(key_tydesc),
            amount_in.as_ptr(),
            rtdt::TyDescRef::from_ref(amount_tydesc),
            fit_out,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_insert_local(
    rt: LocalRtHandle,
    btreemap_value_mut: rtdt::AnyMut<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    // Value is moved.
    key_in: rtdt::AnyOwned<'_>,
    key_tydesc: &rtdt::TyDesc,
    // Value is moved.
    value_in: rtdt::AnyOwned<'_>,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_insert_local";
        btreemap_value_mut: btreemap_tydesc.size as usize,
        key_in: key_tydesc.size as usize,
        value_in: value_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreemap_value_mut.as_ptr(), btreemap_tydesc, "btreemap_insert:map");
    debug_assert_aligned(key_in.as_ptr(), key_tydesc, "btreemap_insert:key");
    debug_assert_aligned(value_in.as_ptr(), value_tydesc, "btreemap_insert:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_key_ty().as_ref(), key_tydesc),
            "btreemap_insert: key_tydesc doesn't match map's key type"
        );
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_value_ty().as_ref(), value_tydesc),
            "btreemap_insert: value_tydesc doesn't match map's value type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ref(value_tydesc);
        crate::c::impls::btreemap::btreemap_insert_impl(
            rt_ref,
            btreemap_value_mut.as_ptr(),
            btreemap_tydesc_ref,
            key_in.as_ptr(),
            key_tydesc_ref,
            value_in.as_ptr(),
            value_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_remove_local(
    rt: LocalRtHandle,
    btreemap_value_mut: rtdt::AnyMut<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_remove_local";
        btreemap_value_mut: btreemap_tydesc.size as usize,
        key_ref: key_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreemap_value_mut.as_ptr(), btreemap_tydesc, "btreemap_remove:map");
    debug_assert_aligned(key_ref.as_ptr(), key_tydesc, "btreemap_remove:key");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_key_ty().as_ref(), key_tydesc),
            "btreemap_remove: key_tydesc doesn't match map's key type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        crate::c::impls::btreemap::btreemap_remove_impl(
            rt_ref,
            btreemap_value_mut.as_ptr(),
            btreemap_tydesc_ref,
            key_ref.as_ptr(),
            key_tydesc_ref,
        )
    }
}

/// Every key of a map, in sort order, onto the end of a list.
///
/// Appended to the list, which is the caller's: a generic one makes it with a
/// descriptor for the element type it was handed, so the list has a real one
/// here where a result slot would only say `data`. See `btreemap_collect_impl`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_keys_into_local(
    rt: LocalRtHandle,
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_keys_into_local";
        btreemap_value_ref: btreemap_tydesc.size as usize,
        list_value_mut: list_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::btreemap::btreemap_collect_impl(
            rt_ref, btreemap_value_ref.as_ptr(), rtdt::TyDescRef::from_ref(btreemap_tydesc),
            crate::c::impls::btreemap::MapPart::Keys,
            list_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc),
        )
    }
}

/// Every value of a map, in the order of its key, onto the end of a list.
///
/// Appended to the list, which is the caller's: a generic one makes it with a
/// descriptor for the element type it was handed, so the list has a real one
/// here where a result slot would only say `data`. See `btreemap_collect_impl`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_values_into_local(
    rt: LocalRtHandle,
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_values_into_local";
        btreemap_value_ref: btreemap_tydesc.size as usize,
        list_value_mut: list_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::btreemap::btreemap_collect_impl(
            rt_ref, btreemap_value_ref.as_ptr(), rtdt::TyDescRef::from_ref(btreemap_tydesc),
            crate::c::impls::btreemap::MapPart::Values,
            list_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc),
        )
    }
}

/// Every entry of a map as a key and its value, in sort order, onto the end of
/// a list.
///
/// Appended to the list, which is the caller's: a generic one makes it with a
/// descriptor for the element type it was handed, so the list has a real one
/// here where a result slot would only say `data`. See `btreemap_collect_impl`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_entries_into_local(
    rt: LocalRtHandle,
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_entries_into_local";
        btreemap_value_ref: btreemap_tydesc.size as usize,
        list_value_mut: list_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::btreemap::btreemap_collect_impl(
            rt_ref, btreemap_value_ref.as_ptr(), rtdt::TyDescRef::from_ref(btreemap_tydesc),
            crate::c::impls::btreemap::MapPart::Entries,
            list_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc),
        )
    }
}

/// Every element of a set, in order, onto the end of a list. See
/// `dtlv_rti_btreemap_keys_into_local`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_into_list_local(
    rt: LocalRtHandle,
    set_value_ref: rtdt::AnyRef<'_>,
    set_tydesc: &rtdt::TyDesc,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreeset_into_list_local";
        set_value_ref: set_tydesc.size as usize,
        list_value_mut: list_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_to_list_impl(
            rt_ref, set_value_ref.as_ptr(), rtdt::TyDescRef::from_ref(set_tydesc),
            list_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_get_local(
    rt: LocalRtHandle,
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
    option_value_out: rtdt::AnyOut<'_>,
    option_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_get_local";
        btreemap_value_ref: btreemap_tydesc.size as usize,
        key_ref: key_tydesc.size as usize,
        option_value_out: option_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreemap_value_ref.as_ptr(), btreemap_tydesc, "btreemap_get:map");
    debug_assert_aligned(key_ref.as_ptr(), key_tydesc, "btreemap_get:key");
    debug_assert_aligned(option_value_out.as_ptr(), option_tydesc, "btreemap_get:option");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_key_ty().as_ref(), key_tydesc),
            "btreemap_get: key_tydesc doesn't match map's key type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ref(option_tydesc);
        crate::c::impls::btreemap::btreemap_get_impl(
            rt_ref,
            btreemap_value_ref.as_ptr(),
            btreemap_tydesc_ref,
            key_ref.as_ptr(),
            key_tydesc_ref,
            option_value_out.as_ptr(),
            option_tydesc_ref,
        )
    }
}


#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_len_local(
    _rt: LocalRtHandle,
    map_value_ref: rtdt::AnyRef<'_>,
    map_tydesc: &rtdt::TyDesc,
    len_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("btreemap_len_local";
        map_value_ref: map_tydesc.size as usize,
        len_out: size_of::<rtdt::Index>(),
    );
    debug_assert_aligned(map_value_ref.as_ptr(), map_tydesc, "btreemap_len:map");
    unsafe { crate::c::impls::btreemap::btreemap_len_impl(map_value_ref.as_ptr(), len_out.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_len_local(
    _rt: LocalRtHandle,
    set_value_ref: rtdt::AnyRef<'_>,
    set_tydesc: &rtdt::TyDesc,
    len_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("btreeset_len_local";
        set_value_ref: set_tydesc.size as usize,
        len_out: size_of::<rtdt::Index>(),
    );
    debug_assert_aligned(set_value_ref.as_ptr(), set_tydesc, "btreeset_len:set");
    unsafe { crate::c::impls::set::btreeset_len_impl(set_value_ref.as_ptr(), len_out.as_ptr()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_clear_local(
    rt: LocalRtHandle,
    btreemap_value_mut: rtdt::AnyMut<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreemap_value_mut.as_ptr(), btreemap_tydesc, "btreemap_clear:map");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::btreemap::btreemap_clear_impl(rt_ref, btreemap_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(btreemap_tydesc))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_contains_key_local(
    _rt: LocalRtHandle,
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
    result_out: *mut bool,
) -> RtStatus {
    debug_assert!(!result_out.is_null(), "result_out is null");
    debug_assert_aligned(btreemap_value_ref.as_ptr(), btreemap_tydesc, "btreemap_contains_key:map");
    debug_assert_aligned(key_ref.as_ptr(), key_tydesc, "btreemap_contains_key:key");
    unsafe {
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        crate::c::impls::btreemap::btreemap_contains_key_impl(
            btreemap_value_ref.as_ptr(),
            btreemap_tydesc_ref,
            key_ref.as_ptr(),
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
    btreemap_value_ref: rtdt::AnyRef<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
    value_ptr_out: *mut *mut u8,
) -> RtStatus {
    debug_assert!(!value_ptr_out.is_null(), "value_ptr_out is null");
    debug_assert_aligned(btreemap_value_ref.as_ptr(), btreemap_tydesc, "btreemap_get_value_ref:map");
    debug_assert_aligned(key_ref.as_ptr(), key_tydesc, "btreemap_get_value_ref:key");
    unsafe {
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        crate::c::impls::btreemap::btreemap_get_value_ref_impl(
            btreemap_value_ref.as_ptr(),
            btreemap_tydesc_ref,
            key_ref.as_ptr(),
            key_tydesc_ref,
            value_ptr_out,
        )
    }
}

/// Replace the value at a key the map already has, moving the new one in.
///
/// A key the map does not have is `Error`. The value is taken either way, as
/// every value moved into the runtime is: one that does not land is destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_set_value_local(
    rt: LocalRtHandle,
    btreemap_value_mut: rtdt::AnyMut<'_>,
    btreemap_tydesc: &rtdt::TyDesc,
    key_ref: rtdt::AnyRef<'_>,
    key_tydesc: &rtdt::TyDesc,
    value_in: rtdt::AnyOwned<'_>,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("btreemap_set_value_local";
        btreemap_value_mut: btreemap_tydesc.size as usize,
        key_ref: key_tydesc.size as usize,
        value_in: value_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreemap_value_mut.as_ptr(), btreemap_tydesc, "btreemap_set_value:map");
    debug_assert_aligned(key_ref.as_ptr(), key_tydesc, "btreemap_set_value:key");
    debug_assert_aligned(value_in.as_ptr(), value_tydesc, "btreemap_set_value:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let btreemap_tydesc_ref = rtdt::TyDescRef::from_ref(btreemap_tydesc);
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_key_ty().as_ref(), key_tydesc),
            "btreemap_set_value: key_tydesc doesn't match map's key type"
        );
        debug_assert!(
            same_tydesc(btreemap_tydesc_ref.map_value_ty().as_ref(), value_tydesc),
            "btreemap_set_value: value_tydesc doesn't match map's value type"
        );
        let key_tydesc_ref = rtdt::TyDescRef::from_ref(key_tydesc);
        let value_tydesc_ref = rtdt::TyDescRef::from_ref(value_tydesc);
        crate::c::impls::btreemap::btreemap_set_value_impl(
            rt_ref,
            btreemap_value_mut.as_ptr(),
            btreemap_tydesc_ref,
            key_ref.as_ptr(),
            key_tydesc_ref,
            value_in.as_ptr(),
            value_tydesc_ref,
        )
    }
}

// BTreeSet operations.

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: rtdt::AnyOut<'_>,
    // BTreeSet type.
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_out.as_ptr(), tydesc, "btreeset_create:value_out.as_ptr()");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_create_impl(rt_ref, value_out.as_ptr(), tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_destroy_local(
    rt: LocalRtHandle,
    btreeset_value_in: rtdt::AnyOwned<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreeset_value_in.as_ptr(), btreeset_tydesc, "btreeset_destroy:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::set_destroy_impl(rt_ref, btreeset_value_in.as_ptr(), btreeset_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_insert_local(
    rt: LocalRtHandle,
    btreeset_value_mut: rtdt::AnyMut<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
    // Element is moved.
    element_in: rtdt::AnyOwned<'_>,
    element_tydesc: &rtdt::TyDesc,
    // Output: 1 if newly inserted, 0 if already existed.
    bool_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("btreeset_insert_local";
        btreeset_value_mut: btreeset_tydesc.size as usize,
        element_in: element_tydesc.size as usize,
        bool_out: size_of::<u8>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreeset_value_mut.as_ptr(), btreeset_tydesc, "btreeset_insert:set");
    debug_assert_aligned(element_in.as_ptr(), element_tydesc, "btreeset_insert:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ref(btreeset_tydesc);
        debug_assert!(
            same_tydesc(btreeset_tydesc_ref.set_element_ty().as_ref(), element_tydesc),
            "btreeset_insert: element_tydesc doesn't match set's element type"
        );

        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_insert_impl(
            rt_ref,
            btreeset_value_mut.as_ptr(),
            btreeset_tydesc,
            element_in.as_ptr(),
            element_tydesc,
            bool_out.as_ptr(),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_remove_local(
    rt: LocalRtHandle,
    btreeset_value_mut: rtdt::AnyMut<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
    element_ref: rtdt::AnyRef<'_>,
    element_tydesc: &rtdt::TyDesc,
    // Output: 1 if removed, 0 if not found.
    bool_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("btreeset_remove_local";
        btreeset_value_mut: btreeset_tydesc.size as usize,
        element_ref: element_tydesc.size as usize,
        bool_out: size_of::<u8>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreeset_value_mut.as_ptr(), btreeset_tydesc, "btreeset_remove:set");
    debug_assert_aligned(element_ref.as_ptr(), element_tydesc, "btreeset_remove:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ref(btreeset_tydesc);
        debug_assert!(
            same_tydesc(btreeset_tydesc_ref.set_element_ty().as_ref(), element_tydesc),
            "btreeset_remove: element_tydesc doesn't match set's element type"
        );

        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_remove_impl(
            rt_ref,
            btreeset_value_mut.as_ptr(),
            btreeset_tydesc,
            element_ref.as_ptr(),
            element_tydesc,
            bool_out.as_ptr(),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_contains_local(
    rt: LocalRtHandle,
    btreeset_value_ref: rtdt::AnyRef<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
    element_ref: rtdt::AnyRef<'_>,
    element_tydesc: &rtdt::TyDesc,
    // Output: 1 if contains, 0 if not.
    bool_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("btreeset_contains_local";
        btreeset_value_ref: btreeset_tydesc.size as usize,
        element_ref: element_tydesc.size as usize,
        bool_out: size_of::<u8>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreeset_value_ref.as_ptr(), btreeset_tydesc, "btreeset_contains:set");
    debug_assert_aligned(element_ref.as_ptr(), element_tydesc, "btreeset_contains:element");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ref(btreeset_tydesc);
        debug_assert!(
            same_tydesc(btreeset_tydesc_ref.set_element_ty().as_ref(), element_tydesc),
            "btreeset_contains: element_tydesc doesn't match set's element type"
        );

        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_contains_impl(
            rt_ref,
            btreeset_value_ref.as_ptr(),
            btreeset_tydesc,
            element_ref.as_ptr(),
            element_tydesc,
            bool_out.as_ptr(),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_clear_local(
    rt: LocalRtHandle,
    btreeset_value_mut: rtdt::AnyMut<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(btreeset_value_mut.as_ptr(), btreeset_tydesc, "btreeset_clear:set");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_clear_impl(rt_ref, btreeset_value_mut.as_ptr(), btreeset_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_clone_from_slice_local(
    rt: LocalRtHandle,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    slice_element_tydesc: &rtdt::TyDesc,
    btreeset_value_out: rtdt::AnyOut<'_>,
    btreeset_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(slice_len == 0 || !slice_ref.is_null(), "slice_ref is null");
    debug_assert_aligned(slice_ref, slice_element_tydesc, "btreeset_clone_from_slice:slice");
    debug_assert_aligned(btreeset_value_out.as_ptr(), btreeset_tydesc, "btreeset_clone_from_slice:set");
    unsafe {
        let btreeset_tydesc_ref = rtdt::TyDescRef::from_ref(btreeset_tydesc);
        debug_assert!(
            same_tydesc(btreeset_tydesc_ref.set_element_ty().as_ref(), slice_element_tydesc),
            "btreeset_clone_from_slice: element_tydesc doesn't match set's element type"
        );

        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_clone_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            slice_element_tydesc,
            btreeset_value_out.as_ptr(),
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
    set_out: rtdt::AnyOut<'_>,
    element_tydesc: &rtdt::TyDesc,
    elements_ptr: *mut u8,
    num_elements: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(num_elements == 0 || !elements_ptr.is_null(), "elements_ptr is null");
    debug_assert_aligned(elements_ptr, element_tydesc, "btreeset_build_from_sorted_slice:elements");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        crate::c::impls::set::btreeset_build_from_sorted_slice(
            rt_ref,
            set_out.as_ptr() as *mut rtdt::Set,
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
    value_out: rtdt::AnyOut<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_out.as_ptr(), tydesc, "list_create:value_out.as_ptr()");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::list::list_create_impl(rt_ref, value_out.as_ptr(), tydesc_ref)
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
    element_tydesc: &rtdt::TyDesc,
    list_value_out: rtdt::AnyOut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert_aligned(slice_ref, element_tydesc, "list_create_from_slice:slice");
    debug_assert_aligned(list_value_out.as_ptr(), list_tydesc, "list_create_from_slice:list");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        debug_assert!(
            same_tydesc(list_tydesc_ref.list_element_ty().as_ref(), element_tydesc),
            "list_create_from_slice: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_create_from_slice_impl(
            rt_ref,
            slice_ref,
            slice_len,
            element_tydesc_ref,
            list_value_out.as_ptr(),
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
    list_out: rtdt::AnyOut<'_>,
    element_tydesc: &rtdt::TyDesc,
    elements_ptr: *mut u8,
    num_elements: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(num_elements == 0 || !elements_ptr.is_null(), "elements_ptr is null");
    debug_assert_aligned(elements_ptr, element_tydesc, "list_build_from_slice:elements");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_build_from_slice_impl(
            rt_ref,
            list_out.as_ptr(),
            element_tydesc_ref,
            elements_ptr,
            num_elements,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_destroy_local(
    rt: LocalRtHandle,
    value_in: rtdt::AnyOwned<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc, "list_destroy:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::list::list_destroy_impl(rt_ref, value_in.as_ptr(), tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_clear_local(
    rt: LocalRtHandle,
    value_mut: rtdt::AnyMut<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_mut.as_ptr(), tydesc, "list_clear:value");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::list::list_clear_impl(rt_ref, value_mut.as_ptr(), tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_len_local(
    rt: LocalRtHandle,
    list_value_ref: rtdt::AnyRef<'_>,
    list_tydesc: &rtdt::TyDesc,
    len_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("list_len_local";
        list_value_ref: list_tydesc.size as usize,
        len_out: size_of::<rtdt::Index>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_ref.as_ptr(), list_tydesc, "list_len:list");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        crate::c::impls::list::list_len_impl(rt_ref, list_value_ref.as_ptr(), list_tydesc_ref, len_out.as_ptr())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_get_local(
    rt: LocalRtHandle,
    list_value_ref: rtdt::AnyRef<'_>,
    list_tydesc: &rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: rtdt::AnyOut<'_>,
    option_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_get_local";
        list_value_ref: list_tydesc.size as usize,
        option_value_out: option_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_ref.as_ptr(), list_tydesc, "list_get:list");
    debug_assert_aligned(option_value_out.as_ptr(), option_tydesc, "list_get:option");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ref(option_tydesc);
        crate::c::impls::list::list_get_impl(
            rt_ref,
            list_value_ref.as_ptr(),
            list_tydesc_ref,
            index,
            option_value_out.as_ptr(),
            option_tydesc_ref,
        )
    }
}

/// Replace the element at `index`, moving the new one in.
///
/// An index past the end is `Error`. The element is taken either way, as every
/// value moved into the runtime is: one that does not land is destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_set_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    index: rtdt::IndexRepr,
    element_in: rtdt::AnyOwned<'_>,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_set_local";
        list_value_mut: list_tydesc.size as usize,
        element_in: element_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_set:list");
    debug_assert_aligned(element_in.as_ptr(), element_tydesc, "list_set:element");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        debug_assert!(
            same_tydesc(list_tydesc_ref.list_element_ty().as_ref(), element_tydesc),
            "list_set: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_set_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            index,
            element_in.as_ptr(),
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_push_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    element_in: rtdt::AnyOwned<'_>,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_push_local";
        list_value_mut: list_tydesc.size as usize,
        element_in: element_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_push:list");
    debug_assert_aligned(element_in.as_ptr(), element_tydesc, "list_push:element");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        debug_assert!(
            same_tydesc(list_tydesc_ref.list_element_ty().as_ref(), element_tydesc),
            "list_push: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_push_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            element_in.as_ptr(),
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_pop_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    option_value_out: rtdt::AnyOut<'_>,
    option_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_pop_local";
        list_value_mut: list_tydesc.size as usize,
        option_value_out: option_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_pop:list");
    debug_assert_aligned(option_value_out.as_ptr(), option_tydesc, "list_pop:option");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ref(option_tydesc);
        crate::c::impls::list::list_pop_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            option_value_out.as_ptr(),
            option_tydesc_ref,
        )
    }
}

/// Insert an element before `index`, or at the end for an index equal to the
/// length, moving it in.
///
/// An index past the end is `Error`. The element is taken either way, as every
/// value moved into the runtime is: one that does not land is destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_insert_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    index: rtdt::IndexRepr,
    element_in: rtdt::AnyOwned<'_>,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_insert_local";
        list_value_mut: list_tydesc.size as usize,
        element_in: element_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_insert:list");
    debug_assert_aligned(element_in.as_ptr(), element_tydesc, "list_insert:element");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        debug_assert!(
            same_tydesc(list_tydesc_ref.list_element_ty().as_ref(), element_tydesc),
            "list_insert: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_insert_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            index,
            element_in.as_ptr(),
            element_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_remove_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    index: rtdt::IndexRepr,
    option_value_out: rtdt::AnyOut<'_>,
    option_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("list_remove_local";
        list_value_mut: list_tydesc.size as usize,
        option_value_out: option_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_remove:list");
    debug_assert_aligned(option_value_out.as_ptr(), option_tydesc, "list_remove:option");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        let option_tydesc_ref = rtdt::TyDescRef::from_ref(option_tydesc);
        crate::c::impls::list::list_remove_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            index,
            option_value_out.as_ptr(),
            option_tydesc_ref,
        )
    }
}

/// Exchange the elements at `i` and `j` of a list where they are, which
/// nothing written in datalove can do without taking one out and shifting the
/// rest. `Error` if either is past the end, which leaves the list as it was.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_swap_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    i: rtdt::IndexRepr,
    j: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_swap:list");
    unsafe {
        crate::c::impls::list::list_swap_impl(list_value_mut.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc), i, j)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_reserve_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    additional: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_reserve:list");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        crate::c::impls::list::list_reserve_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
            additional,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_shrink_to_fit_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_shrink_to_fit:list");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        crate::c::impls::list::list_shrink_to_fit_impl(
            rt_ref,
            list_value_mut.as_ptr(),
            list_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_extend_from_slice_local(
    rt: LocalRtHandle,
    list_value_mut: rtdt::AnyMut<'_>,
    list_tydesc: &rtdt::TyDesc,
    slice_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert_aligned(list_value_mut.as_ptr(), list_tydesc, "list_extend_from_slice:list");
    debug_assert_aligned(slice_ref, element_tydesc, "list_extend_from_slice:slice");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let list_tydesc_ref = rtdt::TyDescRef::from_ref(list_tydesc);
        debug_assert!(
            same_tydesc(list_tydesc_ref.list_element_ty().as_ref(), element_tydesc),
            "list_extend_from_slice: element_tydesc doesn't match list's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        crate::c::impls::list::list_extend_from_slice_impl(
            rt_ref,
            list_value_mut.as_ptr(),
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
    element_tydesc: &rtdt::TyDesc,
    shape_in: rtdt::AnyOwned<'_>,
    shape_tydesc: &rtdt::TyDesc,
    layout: u8,
    tensor_value_out: rtdt::AnyOut<'_>,
    tensor_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_create_from_slice_local";
        shape_in: shape_tydesc.size as usize,
        tensor_value_out: tensor_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!slice_ref.is_null(), "slice_ref is null");
    debug_assert_aligned(slice_ref, element_tydesc, "tensor_create_from_slice:slice");
    debug_assert_aligned(shape_in.as_ptr(), shape_tydesc, "tensor_create_from_slice:shape");
    debug_assert_aligned(tensor_value_out.as_ptr(), tensor_tydesc, "tensor_create_from_slice:tensor");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        debug_assert!(
            same_tydesc(tensor_tydesc_ref.tensor_element_ty().as_ref(), element_tydesc),
            "tensor_create_from_slice: element_tydesc doesn't match tensor's element type"
        );
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        let shape_tydesc_ref = rtdt::TyDescRef::from_ref(shape_tydesc);
        crate::c::impls::tensor::tensor_create_from_slice_impl(
            rt_ref,
            slice_ref,
            rtdt::Index(slice_len),
            element_tydesc_ref,
            shape_in.as_ptr(),
            shape_tydesc_ref,
            layout,
            tensor_value_out.as_ptr(),
            tensor_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_destroy_local(
    rt: LocalRtHandle,
    tensor_value_in: rtdt::AnyOwned<'_>,
    tensor_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(tensor_value_in.as_ptr(), tensor_tydesc, "tensor_destroy:tensor");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        crate::c::impls::tensor::tensor_destroy_impl(rt_ref, tensor_value_in.as_ptr(), tensor_tydesc_ref)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_get_local(
    rt: LocalRtHandle,
    tensor_value_ref: rtdt::AnyRef<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    indices_ptr: *const rtdt::IndexRepr,
    element_value_out: rtdt::AnyOut<'_>,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_get_local";
        tensor_value_ref: tensor_tydesc.size as usize,
        element_value_out: element_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!indices_ptr.is_null(), "indices_ptr is null");
    debug_assert_aligned(tensor_value_ref.as_ptr(), tensor_tydesc, "tensor_get:tensor");
    debug_assert_aligned(element_value_out.as_ptr(), element_tydesc, "tensor_get:element");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        debug_assert!(
            same_tydesc(tensor_tydesc_ref.tensor_element_ty().as_ref(), element_tydesc),
            "tensor_get: element_tydesc doesn't match tensor's element type"
        );
        crate::c::impls::tensor::tensor_get_impl(
            rt_ref,
            tensor_value_ref.as_ptr(),
            tensor_tydesc_ref,
            indices_ptr,
            element_value_out.as_ptr(),
        )
    }
}

/// Replace the element at `indices`, one per axis, moving the new one in.
///
/// An index past the end of its axis is `Error`, with the tensor as it was.
/// The element is taken either way, as every value moved into the runtime is:
/// one that does not land is destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_set_local(
    rt: LocalRtHandle,
    tensor_value_mut: rtdt::AnyMut<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    indices_ptr: *const rtdt::IndexRepr,
    element_in: rtdt::AnyOwned<'_>,
    element_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_set_local";
        tensor_value_mut: tensor_tydesc.size as usize,
        element_in: element_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!indices_ptr.is_null(), "indices_ptr is null");
    debug_assert_aligned(tensor_value_mut.as_ptr(), tensor_tydesc, "tensor_set:tensor");
    debug_assert_aligned(element_in.as_ptr(), element_tydesc, "tensor_set:element");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        debug_assert!(
            same_tydesc(tensor_tydesc_ref.tensor_element_ty().as_ref(), element_tydesc),
            "tensor_set: element_tydesc doesn't match tensor's element type"
        );
        crate::c::impls::tensor::tensor_set_impl(
            rt_ref,
            tensor_value_mut.as_ptr(),
            tensor_tydesc_ref,
            indices_ptr,
            element_in.as_ptr(),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_transpose_local(
    rt: LocalRtHandle,
    tensor_ref: rtdt::AnyRef<'_>,
    tensor_tydesc_ref: &rtdt::TyDesc,
    perm_ptr: *const u32,
    tensor_value_out: rtdt::AnyOut<'_>,
    tensor_tydesc_out: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_transpose_local";
        tensor_ref: tensor_tydesc_ref.size as usize,
        tensor_value_out: tensor_tydesc_out.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!perm_ptr.is_null(), "perm_ptr is null");
    debug_assert_aligned(tensor_ref.as_ptr(), tensor_tydesc_ref, "tensor_transpose:tensor_in");
    debug_assert_aligned(tensor_value_out.as_ptr(), tensor_tydesc_out, "tensor_transpose:tensor_out");
    unsafe {
        debug_assert!(
            std::ptr::eq(tensor_tydesc_ref, tensor_tydesc_out),
            "tensor_transpose: input and output tensor tydescs must be identical"
        );

        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc = rtdt::TyDescRef::from_ref(tensor_tydesc_ref);
        crate::c::impls::tensor::tensor_transpose_impl(
            rt_ref,
            tensor_ref.as_ptr() as *mut u8,
            tensor_tydesc,
            perm_ptr,
            tensor_value_out.as_ptr(),
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_slice_local(
    rt: LocalRtHandle,
    tensor_value_in: rtdt::AnyOwned<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    ranges_ptr: *const rtdt::SliceRange,
    result_value_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_slice_local";
        tensor_value_in: tensor_tydesc.size as usize,
        result_value_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!ranges_ptr.is_null(), "ranges_ptr is null");
    debug_assert_aligned(tensor_value_in.as_ptr(), tensor_tydesc, "tensor_slice:tensor");
    debug_assert_aligned(result_value_out.as_ptr(), result_tydesc, "tensor_slice:result");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        let result_tydesc_ref = rtdt::TyDescRef::from_ref(result_tydesc);
        crate::c::impls::tensor::tensor_slice_impl(
            rt_ref,
            tensor_value_in.as_ptr(),
            tensor_tydesc_ref,
            ranges_ptr,
            result_value_out.as_ptr(),
            result_tydesc_ref,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_reshape_local(
    rt: LocalRtHandle,
    tensor_value_in: rtdt::AnyOwned<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    new_shape_in: rtdt::AnyOwned<'_>,
    new_shape_tydesc: &rtdt::TyDesc,
    result_value_out: rtdt::AnyOut<'_>,
    result_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("tensor_reshape_local";
        tensor_value_in: tensor_tydesc.size as usize,
        new_shape_in: new_shape_tydesc.size as usize,
        result_value_out: result_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(tensor_value_in.as_ptr(), tensor_tydesc, "tensor_reshape:tensor");
    debug_assert_aligned(new_shape_in.as_ptr(), new_shape_tydesc, "tensor_reshape:shape");
    debug_assert_aligned(result_value_out.as_ptr(), result_tydesc, "tensor_reshape:result");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        let new_shape_tydesc_ref = rtdt::TyDescRef::from_ref(new_shape_tydesc);
        let result_tydesc_ref = rtdt::TyDescRef::from_ref(result_tydesc);
        crate::c::impls::tensor::tensor_reshape_impl(
            rt_ref,
            tensor_value_in.as_ptr(),
            tensor_tydesc_ref,
            new_shape_in.as_ptr(),
            new_shape_tydesc_ref,
            result_value_out.as_ptr(),
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
    element_tydesc: &rtdt::TyDesc,
    shape_ptr: *const u32,
    rank: u32,
    tensor_value_out: rtdt::AnyOut<'_>,
    tensor_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!element_data_in.is_null() || element_count == 0, "element_data_in is null");
    debug_assert!(!shape_ptr.is_null() || rank == 0, "shape_ptr is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let element_tydesc_ref = rtdt::TyDescRef::from_ref(element_tydesc);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        crate::c::impls::tensor::tensor_init_impl(
            rt_ref,
            element_data_in,
            rtdt::Index(element_count),
            element_tydesc_ref,
            shape_ptr,
            rank,
            tensor_value_out.as_ptr(),
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
    tensor_value_ref: rtdt::AnyRef<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    axis0_index: rtdt::IndexRepr,
    sub_tensor_out: rtdt::AnyOut<'_>,
) -> RtStatus {
    lend!("tensor_hyperplane_clone_local";
        tensor_value_ref: tensor_tydesc.size as usize,
        sub_tensor_out: size_of::<rtdt::Tensor>(),
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(tensor_value_ref.as_ptr(), tensor_tydesc, "tensor_hyperplane_clone:tensor");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let tensor_tydesc_ref = rtdt::TyDescRef::from_ref(tensor_tydesc);
        crate::c::impls::tensor::tensor_hyperplane_clone_impl(
            rt_ref,
            tensor_value_ref.as_ptr(),
            tensor_tydesc_ref,
            axis0_index,
            sub_tensor_out.as_ptr(),
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
    value_out: rtdt::AnyOut<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_out.as_ptr(), tydesc, "table_create:value_out.as_ptr()");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::table::table_create_impl(value_out.as_ptr(), tydesc_ref)
    }
}

/// Destroy a table and all its elements.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_destroy_local(
    rt: LocalRtHandle,
    value_in: rtdt::AnyOwned<'_>,
    tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_in.as_ptr(), tydesc, "table_destroy:value");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ref(tydesc);
        crate::c::impls::table::table_destroy_impl(rt_local::from_handle(rt), value_in.as_ptr(), tydesc_ref)
    }
}

/// Push a row to a table.
///
/// The row is passed as a tuple with one field per column.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_push_row_local(
    rt: LocalRtHandle,
    table_mut: rtdt::AnyMut<'_>,
    table_tydesc: &rtdt::TyDesc,
    row_ref: rtdt::AnyRef<'_>,
    row_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("table_push_row_local";
        table_mut: table_tydesc.size as usize,
        row_ref: row_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(table_mut.as_ptr(), table_tydesc, "table_push_row:table");
    debug_assert_aligned(row_ref.as_ptr(), row_tydesc, "table_push_row:row");
    unsafe {
        let table_tydesc_ref = rtdt::TyDescRef::from_ref(table_tydesc);
        let row_tydesc_ref = rtdt::TyDescRef::from_ref(row_tydesc);
        crate::c::impls::table::table_push_row_impl(
            rt_local::from_handle(rt),
            table_mut.as_ptr(),
            table_tydesc_ref,
            row_ref.as_ptr(),
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
    table_out: rtdt::AnyOut<'_>,
    table_tydesc: &rtdt::TyDesc,
    rows_ptr: *mut u8,
    row_tydesc: &rtdt::TyDesc,
    num_rows: rtdt::IndexRepr,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(num_rows == 0 || !rows_ptr.is_null(), "rows_ptr is null");
    debug_assert_aligned(rows_ptr, row_tydesc, "table_build_from_rows:rows");
    unsafe {
        let table_tydesc_ref = rtdt::TyDescRef::from_ref(table_tydesc);
        let row_tydesc_ref = rtdt::TyDescRef::from_ref(row_tydesc);
        crate::c::impls::table::table_build_from_rows_impl(
            rt_local::from_handle(rt),
            table_out.as_ptr(),
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
    table_ref: rtdt::AnyRef<'_>,
    table_tydesc: &rtdt::TyDesc,
    row: rtdt::IndexRepr,
    col: u32,
) -> *const u8 {
    debug_assert_aligned(table_ref.as_ptr(), table_tydesc, "table_get:table");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ref(table_tydesc);
        crate::c::impls::table::table_get_element_ptr(table_ref.as_ptr(), tydesc_ref, row, col)
    }
}

/// Set an element at (row, col).
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_set_local(
    rt: LocalRtHandle,
    table_mut: rtdt::AnyMut<'_>,
    table_tydesc: &rtdt::TyDesc,
    row: rtdt::IndexRepr,
    col: u32,
    value_ref: rtdt::AnyRef<'_>,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    lend!("table_set_local";
        table_mut: table_tydesc.size as usize,
        value_ref: value_tydesc.size as usize,
    );
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(table_mut.as_ptr(), table_tydesc, "table_set:table");
    debug_assert_aligned(value_ref.as_ptr(), value_tydesc, "table_set:value");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ref(table_tydesc);
        crate::c::impls::table::table_set_element(
            rt_local::from_handle(rt),
            table_mut.as_ptr(),
            tydesc_ref,
            row,
            col,
            value_ref.as_ptr(),
            value_tydesc,
        )
    }
}

/// Clear a table, destroying all elements but keeping capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_clear_local(
    rt: LocalRtHandle,
    table_mut: rtdt::AnyMut<'_>,
    table_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(table_mut.as_ptr(), table_tydesc, "table_clear:table");
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ref(table_tydesc);
        crate::c::impls::table::table_clear_impl(rt_local::from_handle(rt), table_mut.as_ptr(), tydesc_ref)
    }
}

/// Get the length (number of rows) of a table.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_table_len(
    _rt: LocalRtHandle,
    table_ref: rtdt::AnyRef<'_>,
    _table_tydesc: &rtdt::TyDesc,
) -> rtdt::IndexRepr {
    unsafe {
        crate::c::impls::table::table_len(table_ref.as_ptr()).0
    }
}

// ============================================================================
// Abandoning compiled code
// ============================================================================

/// Have compiled code abandon the calls it is in, for `reason`, one of the
/// `ABANDONED_FOR_*` reasons; see [`RtHeader::abandoning`].
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_abandon(rt: LocalRtHandle, reason: u8) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(reason == ABANDONED_FOR_ERROR || reason == ABANDONED_FOR_STACK, "unknown reason {reason}");
    unsafe { (*datalove_rti::header(rt)).abandoning = reason };
    RtStatus::Ok
}

/// Why compiled code abandoned its calls, or 0 if it did not, clearing it.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_take_abandoned(rt: LocalRtHandle) -> u8 {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe { std::mem::take(&mut (*datalove_rti::header(rt)).abandoning) }
}

/// Set the lowest address compiled code lets the native stack grow to; see
/// [`RtHeader::stack_limit`].
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_set_stack_limit(rt: LocalRtHandle, limit: usize) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe { (*datalove_rti::header(rt)).stack_limit = limit };
    RtStatus::Ok
}

/// Stop the process for a stack overflow, which compiled code that cannot
/// return to an interpreter found on entering a function.
///
/// The frames under it are not unwound and what they own is not destroyed:
/// there is no way back up through compiled code, and the process is ending.
/// Reported the way the CLI reports one, on stderr with status 1.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_stack_overflow(rt: LocalRtHandle) -> ! {
    debug_assert!(!rt.is_null(), "rt is null");
    eprintln!("Error: stack overflow");
    std::process::exit(1)
}

/// The lowest address code running datalove on the calling thread should let
/// the native stack grow to, or 0 for no limit where the thread's stack cannot
/// be found.
///
/// A margin above the bottom of the thread's stack, room for the work between
/// a check and the error's way back. Found once per thread.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_native_stack_limit(rt: LocalRtHandle) -> usize {
    debug_assert!(!rt.is_null(), "rt is null");
    rt_local::native_stack_limit()
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
        let rt_ref = rt_local::from_handle(rt);
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
    value_ref: rtdt::AnyRef<'_>,
    value_tydesc: &rtdt::TyDesc,
) -> RtStatus {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert_aligned(value_ref.as_ptr(), value_tydesc, "debuglog_local:value");
    unsafe {
        crate::c::impls::debuglog::debuglog_local(rt_local::from_handle(rt), value_ref.as_ptr(), value_tydesc)
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
        let rt_ref = &*rt_local::from_handle(rt);
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
        let rt_ref = rt_local::from_handle(rt);
        rt_ref.debug_buffer.clear();
    }
    RtStatus::Ok
}


/// The offset of a field of an aggregate, taken from its descriptor.
///
/// Inside a generic an aggregate holding a type parameter has a layout only
/// its descriptor knows, so a field offset has to be read from there. The
/// offset is already in there: building a descriptor is what settled the
/// alignment.
///
/// Here rather than in each backend because a tuple and a struct do not agree
/// on the order of their two fields -- `TyInfoTuple` is count then pointer and
/// `TyInfoStruct` is pointer then count -- and four hand-written walks of that
/// would get it wrong at least once.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_field_offset(
    tydesc: &rtdt::TyDesc,
    index: u32,
) -> u32 {
    unsafe { crate::c::impls::dyn_ops::field_offset(tydesc, index) }
}

/// The descriptor of a field of an aggregate, taken from its descriptor.
///
/// The other half of `dtlv_rti_field_offset`: a projection narrows the pointer
/// and the descriptor together, so that the next step of a chain, and whatever
/// finally reads through the reference, each have the type of what is really
/// there.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_field_tydesc(
    tydesc: &rtdt::TyDesc,
    index: u32,
) -> *const rtdt::TyDesc {
    unsafe { crate::c::impls::dyn_ops::field_tydesc(tydesc, index) }
}

/// The offset of the payload of an option, a result, a term, or variant
/// `index` of an enum, taken from its descriptor.
///
/// The counterpart of `dtlv_rti_field_offset` for the types with a tag. They
/// do not share one rule -- a result's payload is aligned for the error it
/// might hold as well as for its value, and each variant of an enum has its
/// own offset -- so the rules are kept here, beside the layouts that decide
/// them. `index` is ignored but for an enum.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_payload_offset(
    tydesc: &rtdt::TyDesc,
    index: u32,
) -> u32 {
    unsafe { crate::c::impls::dyn_ops::payload_offset(tydesc, index) }
}

/// The descriptor of the payload of an option, a result's value, a term, or
/// variant `index` of an enum; null for a variant with none.
///
/// The other half of `dtlv_rti_payload_offset`, as `dtlv_rti_field_tydesc` is
/// of `dtlv_rti_field_offset`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_payload_tydesc(
    tydesc: &rtdt::TyDesc,
    index: u32,
) -> *const rtdt::TyDesc {
    unsafe { crate::c::impls::dyn_ops::payload_tydesc(tydesc, index) }
}

/// The descriptor a template describes, with its type parameters bound to
/// `params`.
///
/// `params` points to `num_params` descriptors, and is not null even when
/// there are none, as a Rust slice's pointer is not.
///
/// A generic is handed one descriptor per type parameter; the descriptor of a
/// type it writes in terms of them is built from those, here, from the
/// template the compiler emitted for it. Built descriptors are cached on the
/// runtime handle and live until it shuts down, so asking again at the same
/// binding is a lookup. See `impls::instantiate`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tydesc_instantiate(
    rt: LocalRtHandle,
    template: &rtdt::TyDesc,
    params: *const *const rtdt::TyDesc,
    num_params: u32,
) -> *const rtdt::TyDesc {
    debug_assert!(!rt.is_null(), "rt is null");
    debug_assert!(!params.is_null(), "params is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        let params = std::slice::from_raw_parts(params, num_params as usize);
        rt_ref.tydesc_cache.instantiate(template, params)
    }
}

/// Take `size` bytes aligned to `align` from the shadow stack.
///
/// Compiled code bumps `RtHeader::shadow_top` itself when what it wants fits
/// below `RtHeader::shadow_limit`, and calls this when it does not; this does
/// either. The space is given back by writing the top and limit saved on entry
/// back to the header. See `impls::shadow`.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_shadow_alloc(
    rt: LocalRtHandle,
    size: u32,
    align: u32,
) -> *mut u8 {
    debug_assert!(!rt.is_null(), "rt is null");
    unsafe {
        let rt_ref = rt_local::from_handle(rt);
        rt_ref.shadow.alloc(&mut rt_ref.header, size as usize, align as usize)
    }
}


// ============================================================================
// Cursors
// ============================================================================
//
// A `for` loop walks a collection with a cursor: begin, then advance until
// done. These functions hold the walk's logic; whether it is done and the
// current element, with a map's value beside its key, are the cursor's own
// fields, laid out in `datalove-rtdt`, which the caller reads. Compiled code
// may open-code these functions too, but only with these meanings. A cursor
// points into a collection the language keeps frozen for the walk, and owns
// nothing.
//
// Each takes the handle and returns a status, with what it answers written
// through an out parameter, as everything here does. Begin, and the trees'
// advance, read the collection and take its descriptor; the rest touch only
// the cursor, and cannot fail. A cursor is not a value of the language and has
// no descriptor, so a cursor argument is not followed by one.

/// Starts a walk over a list at its first element.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_list_cursor_begin_local(
    _rt: LocalRtHandle,
    list_ref: rtdt::AnyRef<'_>,
    list_tydesc: &rtdt::TyDesc,
    cursor_out: *mut rtdt::StrideCursor,
) -> RtStatus {
    debug_assert!(!cursor_out.is_null(), "cursor_out is null");
    debug_assert_aligned(list_ref.as_ptr(), list_tydesc, "list_cursor_begin:list");
    unsafe {
        crate::c::impls::cursor::list_cursor_begin_impl(list_ref.as_ptr(), rtdt::TyDescRef::from_ref(list_tydesc), cursor_out);
    }
    RtStatus::Ok
}

/// Starts a walk over a rank-1 tensor at its first element.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_cursor_begin_local(
    _rt: LocalRtHandle,
    tensor_ref: rtdt::AnyRef<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    cursor_out: *mut rtdt::StrideCursor,
) -> RtStatus {
    debug_assert!(!cursor_out.is_null(), "cursor_out is null");
    debug_assert_aligned(tensor_ref.as_ptr(), tensor_tydesc, "tensor_cursor_begin:tensor");
    unsafe {
        crate::c::impls::cursor::tensor_cursor_begin_impl(tensor_ref.as_ptr(), rtdt::TyDescRef::from_ref(tensor_tydesc), cursor_out);
    }
    RtStatus::Ok
}

/// Moves a walk over a list or a rank-1 tensor to the next element.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_stride_cursor_advance_local(
    _rt: LocalRtHandle,
    cursor_mut: *mut rtdt::StrideCursor,
) -> RtStatus {
    debug_assert!(!cursor_mut.is_null(), "cursor_mut is null");
    unsafe { crate::c::impls::cursor::stride_cursor_advance_impl(cursor_mut) };
    RtStatus::Ok
}

/// Starts a walk over the rows of a tensor of rank above 1.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tensor_row_cursor_begin_local(
    _rt: LocalRtHandle,
    tensor_ref: rtdt::AnyRef<'_>,
    tensor_tydesc: &rtdt::TyDesc,
    cursor_out: *mut rtdt::RowCursor,
) -> RtStatus {
    debug_assert!(!cursor_out.is_null(), "cursor_out is null");
    debug_assert_aligned(tensor_ref.as_ptr(), tensor_tydesc, "tensor_row_cursor_begin:tensor");
    unsafe {
        crate::c::impls::cursor::tensor_row_cursor_begin_impl(tensor_ref.as_ptr(), rtdt::TyDescRef::from_ref(tensor_tydesc), cursor_out);
    }
    RtStatus::Ok
}

/// Moves a walk over the rows of a tensor to the next row.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_row_cursor_advance_local(
    _rt: LocalRtHandle,
    cursor_mut: *mut rtdt::RowCursor,
) -> RtStatus {
    debug_assert!(!cursor_mut.is_null(), "cursor_mut is null");
    unsafe { crate::c::impls::cursor::row_cursor_advance_impl(cursor_mut) };
    RtStatus::Ok
}

/// Starts a walk over a set at its least element.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreeset_cursor_begin_local(
    _rt: LocalRtHandle,
    set_ref: rtdt::AnyRef<'_>,
    set_tydesc: &rtdt::TyDesc,
    cursor_out: *mut rtdt::TreeCursor,
) -> RtStatus {
    debug_assert!(!cursor_out.is_null(), "cursor_out is null");
    debug_assert_aligned(set_ref.as_ptr(), set_tydesc, "btreeset_cursor_begin:set");
    unsafe { crate::c::impls::set::btreeset_cursor_begin_impl(set_ref.as_ptr(), set_tydesc, cursor_out) };
    RtStatus::Ok
}

/// Starts a walk over a map at its least key.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_btreemap_cursor_begin_local(
    _rt: LocalRtHandle,
    map_ref: rtdt::AnyRef<'_>,
    map_tydesc: &rtdt::TyDesc,
    cursor_out: *mut rtdt::TreeCursor,
) -> RtStatus {
    debug_assert!(!cursor_out.is_null(), "cursor_out is null");
    debug_assert_aligned(map_ref.as_ptr(), map_tydesc, "btreemap_cursor_begin:map");
    unsafe {
        crate::c::impls::btreemap::btreemap_cursor_begin_impl(map_ref.as_ptr(), rtdt::TyDescRef::from_ref(map_tydesc), cursor_out);
    }
    RtStatus::Ok
}

/// Moves a walk over a set or a map to the next element or entry.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn dtlv_rti_tree_cursor_advance_local(
    _rt: LocalRtHandle,
    cursor_mut: *mut rtdt::TreeCursor,
) -> RtStatus {
    debug_assert!(!cursor_mut.is_null(), "cursor_mut is null");
    unsafe { crate::c::impls::cursor::tree_cursor_advance_impl(cursor_mut) };
    RtStatus::Ok
}
