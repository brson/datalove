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
