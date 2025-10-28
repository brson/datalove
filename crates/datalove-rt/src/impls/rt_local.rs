//! Local runtime state.
//!
//! Contains the runtime state for single-threaded local execution,
//! including the allocator and any other runtime-specific state.

use rmx::prelude::*;
use crate::impls::alloc::AllocLocal;

/// Local runtime state.
///
/// Contains the allocator and other runtime-specific state needed
/// for single-threaded local execution.
pub struct RtLocal {
    /// Local allocator.
    pub alloc: AllocLocal,
}

impl RtLocal {
    /// Create a new local runtime.
    pub fn new() -> Box<RtLocal> {
        Box::new(RtLocal {
            alloc: AllocLocal::new_raw(),
        })
    }

    /// Create a new local runtime with a specific leak check mode.
    pub fn with_leak_check_mode(mode: crate::impls::alloc::LeakCheckMode) -> Box<RtLocal> {
        Box::new(RtLocal {
            alloc: AllocLocal::new_raw_with_leak_check_mode(mode),
        })
    }
}

impl Drop for RtLocal {
    fn drop(&mut self) {
        // Move alloc out and consume it in shutdown.
        // We need to replace it with a dummy value since shutdown consumes self.
        unsafe {
            let alloc = std::mem::replace(
                &mut self.alloc,
                AllocLocal::new_raw_with_leak_check_mode(crate::impls::alloc::LeakCheckMode::Ignore)
            );
            alloc.shutdown();
        }
    }
}
