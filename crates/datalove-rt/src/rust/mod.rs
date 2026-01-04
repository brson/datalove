//! Safe Rust wrapper API for the Datalove runtime.
//!
//! This module provides an idiomatic Rust interface around the C-ABI functions.

mod aligned_buffer;

pub use aligned_buffer::AlignedBuffer;

use crate::c::{LocalRtHandle, RtStatus};

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
    tydesc: *const crate::rtdt::TyDesc,
    count: u32,
    ptr: *mut u8,
}

impl MemGuard {
    /// Allocate typed memory. Returns None if allocation fails.
    pub fn new(rt: LocalRtHandle, tydesc: *const crate::rtdt::TyDesc, count: u32) -> Option<Self> {
        let ptr = unsafe { crate::c::dtlv_rti_mem_alloc_local(rt, tydesc, count) };
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
    pub fn tydesc(&self) -> *const crate::rtdt::TyDesc {
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
            crate::c::dtlv_rti_mem_free_local(self.rt, self.tydesc, self.count, self.ptr);
        }
    }
}

/// Guard for an initialized typed value.
///
/// Automatically destroys and frees on drop unless `leak()` is called.
/// Use this when you have ownership of an initialized value.
pub struct ValueGuard {
    rt: LocalRtHandle,
    tydesc: *const crate::rtdt::TyDesc,
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
        tydesc: *const crate::rtdt::TyDesc,
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
    pub fn tydesc(&self) -> *const crate::rtdt::TyDesc {
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
