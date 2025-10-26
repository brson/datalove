//! Safe Rust wrapper API for the Datalove runtime.
//!
//! This module provides an idiomatic Rust interface around the C-ABI functions.

use crate::c::{LocalRtHandle, RtStatus};
use crate::impls::rt_local::RtLocal;

/// Safe wrapper around a Datalove runtime instance.
///
/// This struct provides RAII management of the runtime handle.
pub struct Runtime {
    handle: LocalRtHandle,
}

impl Runtime {
    /// Create a new runtime instance.
    pub fn new() -> Self {
        let handle = crate::c::dtlv_rti_init();
        assert!(!handle.is_null(), "Failed to initialize runtime");
        Runtime { handle }
    }

    /// Get the raw handle for use with C-ABI functions.
    pub fn handle(&self) -> LocalRtHandle {
        self.handle
    }

    /// Get a mutable reference to the underlying RtLocal.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the handle is valid and that no other references exist.
    pub unsafe fn rt_local_mut(&mut self) -> &mut RtLocal {
        unsafe { &mut *(self.handle as *mut RtLocal) }
    }

    /// Get a shared reference to the underlying RtLocal.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the handle is valid.
    pub unsafe fn rt_local(&self) -> &RtLocal {
        unsafe { &*(self.handle as *const RtLocal) }
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
