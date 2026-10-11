//! Local runtime state.
//!
//! Contains the runtime state for single-threaded local execution,
//! including the allocator and any other runtime-specific state.

use datalove_rti::RtHeader;

use crate::c::impls::alloc::AllocLocal;

// Crosses the ABI as an argument to `dtlv_rti_set_debug_mode`, so it is
// declared with the rest of the interface.
pub use datalove_rti::DebugOutputMode;

/// Local runtime state.
///
/// Contains the allocator and other runtime-specific state needed
/// for single-threaded local execution.
/// `repr(C)` for the first field's sake: a handle points here, and what
/// outside this crate may assume about it is the `RtHeader` it starts with.
#[repr(C)]
pub struct RtLocal {
    /// The table riders call this runtime through, and what compiled code
    /// reads and writes in place. First, and it has to stay first.
    pub header: RtHeader,
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
            header: RtHeader { table: &crate::abi_table::TABLE, abandoning: 0, stack_limit: 0 },
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
                AllocLocal::new_raw_with_leak_check_mode(crate::c::impls::alloc::LeakCheckMode::Ignore)
            );
            alloc.shutdown();
        }
    }
}*/

/// The calling thread's stack limit; see `dtlv_rti_native_stack_limit`.
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
