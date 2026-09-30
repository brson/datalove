//! Local runtime state.
//!
//! Contains the runtime state for single-threaded local execution,
//! including the allocator and any other runtime-specific state.

use datalove_rti::table::RtiTable;

use crate::impls::alloc::AllocLocal;

// Crosses the ABI as an argument to `dtlv_rti_set_debug_mode`, so it is
// declared with the rest of the interface.
pub use datalove_rti::DebugOutputMode;

/// Local runtime state.
///
/// Contains the allocator and other runtime-specific state needed
/// for single-threaded local execution.
/// `repr(C)` for the first field's sake. A rider does not link the runtime
/// and cannot call it by name, so it calls through the table, which it finds
/// by reading the first word of what its handle points at. That one offset is
/// the only thing outside this crate may assume about this type.
#[repr(C)]
pub struct RtLocal {
    /// The functions a rider calls this runtime through.
    ///
    /// First, and it has to stay first: `datalove_rti::table` reads it at
    /// offset zero.
    pub table: &'static RtiTable,
    /// Local allocator.
    pub alloc: AllocLocal,
    /// Debug output mode.
    pub debug_output_mode: DebugOutputMode,
    /// Debug output buffer (when mode is Buffer).
    ///
    /// Each debuglog entry is appended with a newline.
    pub debug_buffer: String,
}

impl RtLocal {
    /// Create a new local runtime.
    pub fn new() -> Box<RtLocal> {
        Box::new(RtLocal {
            table: &crate::abi_table::TABLE,
            alloc: AllocLocal::new_raw(),
            debug_output_mode: DebugOutputMode::default(),
            debug_buffer: String::new(),
        })
    }

    /// Shutdown the runtime.
    pub unsafe fn shutdown(self: Box<Self>) {
        unsafe {
            self.alloc.shutdown();
        }
    }
}

// fixme leaks
/*impl Drop for RtLocal {
    fn drop(&mut self) {
        unsafe {
            let alloc = std::mem::replace(
                &mut self.alloc,
                AllocLocal::new_raw_with_leak_check_mode(crate::impls::alloc::LeakCheckMode::Ignore)
            );
            alloc.shutdown();
        }
    }
}*/
