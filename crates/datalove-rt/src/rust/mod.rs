//! Safe Rust wrapper API for the Datalove runtime.
//!
//! This module provides an idiomatic Rust interface around the C-ABI functions.

mod aligned_buffer;

pub use aligned_buffer::{AlignedBuffer, POISON};

use crate::c::{LocalRtHandle, RtStatus};
use datalove_rtdt as rtdt;

/// Where in the runtime JIT-compiled code finds whether it is abandoning the
/// calls it is in, and why: a byte, 0 if it is not.
///
/// Compiled code has no way to return an interpreter's error: its results are
/// values. So when an interpreted function it called fails, the JIT notes the
/// error and sets this to `ABANDONED_FOR_ERROR`, and when compiled code finds
/// the stack too deep it sets `ABANDONED_FOR_STACK`; either way it returns.
/// Compiled code reads this after each call it makes and returns at once if it
/// is set, and the interpreter, where it entered the compiled code, takes the
/// error back. Read through the handle every compiled function is given, at
/// this offset into what it points at.
pub fn abandoning_offset() -> i32 {
    std::mem::offset_of!(crate::impls::rt_local::RtLocal, abandoning) as i32
}

/// An interpreted function under compiled code failed, and the JIT has the
/// error.
pub const ABANDONED_FOR_ERROR: u8 = 1;

/// Compiled code found the stack past `stack_limit_offset`'s limit.
pub const ABANDONED_FOR_STACK: u8 = 2;

/// Where in the runtime JIT-compiled code finds the lowest address it lets the
/// stack grow to, checked as each compiled function is entered: a word, 0 for
/// no limit.
pub fn stack_limit_offset() -> i32 {
    std::mem::offset_of!(crate::impls::rt_local::RtLocal, stack_limit) as i32
}

/// Set why compiled code is abandoning its calls, or 0 to clear it; see
/// `abandoning_offset`.
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out.
pub unsafe fn set_abandoning(rt: LocalRtHandle, reason: u8) {
    unsafe { (*(rt as *mut crate::impls::rt_local::RtLocal)).abandoning = reason };
}

/// Why compiled code is abandoning its calls, or 0; see `abandoning_offset`.
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out.
pub unsafe fn abandoning(rt: LocalRtHandle) -> u8 {
    unsafe { (*(rt as *const crate::impls::rt_local::RtLocal)).abandoning }
}

/// Set the lowest address compiled code lets the stack grow to; see
/// `stack_limit_offset`.
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out.
pub unsafe fn set_stack_limit(rt: LocalRtHandle, limit: usize) {
    unsafe { (*(rt as *mut crate::impls::rt_local::RtLocal)).stack_limit = limit };
}

/// The lowest address code running datalove on this thread lets the native
/// stack grow to, or 0 for no limit where the thread's stack cannot be found.
///
/// What reaches it reports a stack overflow rather than overflowing: the IR
/// walker, which recurses on the native stack, and JIT-compiled code (see
/// `stack_limit_offset`). It is a margin above the stack's bottom, room for
/// the work between the check and the error's way back.
pub fn native_stack_limit() -> usize {
    const MARGIN: usize = 1 << 20;
    thread_local! {
        static LIMIT: usize = thread_stack_bottom().map_or(0, |bottom| bottom + MARGIN);
    }
    LIMIT.with(|limit| *limit)
}

/// The lowest address of this thread's stack. Asking is slow for the main
/// thread, whose bounds glibc reads out of `/proc`, so `native_stack_limit`
/// asks once a thread.
#[cfg(target_os = "linux")]
fn thread_stack_bottom() -> Option<usize> {
    use rmx::libc;
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let got = libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
        libc::pthread_attr_destroy(&mut attr);
        (got == 0).then_some(addr as usize)
    }
}

#[cfg(not(target_os = "linux"))]
fn thread_stack_bottom() -> Option<usize> {
    None
}

/// Safe wrapper around a Datalove runtime instance.
///
/// This struct provides RAII management of the runtime handle.
pub struct Runtime {
    handle: LocalRtHandle,
}

impl Runtime {
    /// Create a new runtime instance with default settings (debug output disabled).
    pub fn new() -> Self {
        Self::new_with_debug_mode(crate::c::DebugOutputMode::Disabled)
    }

    /// Create a new runtime instance with the specified debug output mode.
    pub fn new_with_debug_mode(debug_mode: crate::c::DebugOutputMode) -> Self {
        let handle = crate::c::dtlv_rti_init();
        assert!(!handle.is_null(), "Failed to initialize runtime");
        unsafe {
            crate::c::dtlv_rti_set_debug_mode(handle, debug_mode);
        }
        Runtime { handle }
    }

    /// Get the raw handle for use with C-ABI functions.
    pub fn handle(&self) -> LocalRtHandle {
        self.handle
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        assert!(!self.handle.is_null());
        unsafe {
            let status = crate::c::dtlv_rti_shutdown(self.handle);
            debug_assert_eq!(status, RtStatus::Ok, "Runtime shutdown failed");
        }
        self.handle = std::ptr::null_mut();
    }
}

/// Guard for typed local memory allocation.
///
/// Automatically frees memory on drop unless `leak()` is called.
/// Does NOT call destroy - use this for uninitialized memory.
pub struct MemGuard {
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    count: u32,
    ptr: *mut u8,
}

impl MemGuard {
    /// Allocate typed memory. Returns None if allocation fails.
    pub fn new(rt: LocalRtHandle, tydesc: *const rtdt::TyDesc, count: u32) -> Option<Self> {
        let ptr = unsafe { crate::c::dtlv_rti_mem_alloc_local(rt, tydesc, count.into()) };
        if ptr.is_null() {
            None
        } else {
            Some(Self { rt, tydesc, count, ptr })
        }
    }

    /// Get the raw pointer.
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// Get the type descriptor.
    pub fn tydesc(&self) -> *const rtdt::TyDesc {
        self.tydesc
    }

    /// Consume the guard without freeing. Returns the pointer.
    pub fn leak(self) -> *mut u8 {
        let ptr = self.ptr;
        std::mem::forget(self);
        ptr
    }

    /// Convert to a ValueGuard after the memory has been initialized.
    ///
    /// Only valid for single-element allocations (count=1).
    ///
    /// # Safety
    /// The memory must be properly initialized before calling this.
    pub unsafe fn into_value(self) -> ValueGuard {
        debug_assert_eq!(self.count, 1, "into_value only valid for count=1");
        let guard = ValueGuard {
            rt: self.rt,
            tydesc: self.tydesc,
            ptr: self.ptr,
        };
        std::mem::forget(self);
        guard
    }
}

impl Drop for MemGuard {
    fn drop(&mut self) {
        unsafe {
            crate::c::dtlv_rti_mem_free_local(self.rt, self.tydesc, self.count.into(), self.ptr);
        }
    }
}

/// Guard for an initialized typed value.
///
/// Automatically destroys and frees on drop unless `leak()` is called.
/// Use this when you have ownership of an initialized value.
pub struct ValueGuard {
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    ptr: *mut u8,
}

impl ValueGuard {
    /// Take ownership of an existing initialized value.
    ///
    /// # Safety
    /// The pointer must be a valid initialized value of the given type,
    /// allocated with the given runtime.
    pub unsafe fn from_raw(
        rt: LocalRtHandle,
        tydesc: *const rtdt::TyDesc,
        ptr: *mut u8,
    ) -> Self {
        debug_assert!(!ptr.is_null());
        Self { rt, tydesc, ptr }
    }

    /// Get the raw pointer.
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// Get the type descriptor.
    pub fn tydesc(&self) -> *const rtdt::TyDesc {
        self.tydesc
    }

    /// Consume the guard without destroying/freeing. Returns the pointer.
    pub fn leak(self) -> *mut u8 {
        let ptr = self.ptr;
        std::mem::forget(self);
        ptr
    }
}

impl Drop for ValueGuard {
    fn drop(&mut self) {
        unsafe {
            crate::c::dtlv_rti_any_destroy_local(self.rt, self.ptr, self.tydesc);
            crate::c::dtlv_rti_mem_free_local(self.rt, self.tydesc, 1, self.ptr);
        }
    }
}
