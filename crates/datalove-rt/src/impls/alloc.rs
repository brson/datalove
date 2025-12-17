//! Simple segregated free list allocator for the Datalove runtime.
//!
//! On Unix platforms: Single-threaded allocator using mmap for page allocation.
//! On wasm32: Uses Rust's global allocator.

use rmx::prelude::*;

// Unix-only imports and constants (used by unix_impl and tests).
#[cfg(not(target_arch = "wasm32"))]
use std::ptr;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

#[cfg(not(target_arch = "wasm32"))]
const PAGE_SIZE: usize = 4096;
#[cfg(not(target_arch = "wasm32"))]
const SIZE_CLASSES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
#[cfg(not(target_arch = "wasm32"))]
const MAX_SMALL_SIZE: usize = 4096;
#[cfg(not(target_arch = "wasm32"))]
const NUM_SIZE_CLASSES: usize = SIZE_CLASSES.len();

/// Leak detection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakCheckMode {
    /// Silent cleanup, no leak detection.
    Ignore,
    /// Print leak warnings to stderr.
    Warn,
    /// Panic on detected leaks.
    Panic,
    /// Panic on detected leaks with backtraces.
    PanicWithBacktrace,
}

impl LeakCheckMode {
    /// Read leak check mode from environment variable.
    fn from_env() -> Self {
        match std::env::var("DATALOVE_LEAK_CHECK").as_deref() {
            Ok("warn") => LeakCheckMode::Warn,
            Ok("panic") => LeakCheckMode::Panic,
            Ok("panic-backtrace") => LeakCheckMode::PanicWithBacktrace,
            Ok("ignore") => LeakCheckMode::Ignore,
            _ => LeakCheckMode::Panic,
        }
    }
}

// ============================================================================
// Unix (non-wasm32) implementation using mmap
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod unix_impl {
    use super::*;

    /// A node in the intrusive free list.
    #[repr(C)]
    struct FreeListNode {
        next: *mut FreeListNode,
    }

    /// Tracks a page allocated via mmap.
    struct Page {
        ptr: *mut u8,
        size: usize,
    }

    /// Information about an allocation for leak detection.
    struct AllocationInfo {
        size: u32,
        align: u32,
        count: u32,
        backtrace: Option<backtrace::Backtrace>,
    }

    /// Local allocator state for the Unix platform.
    pub struct AllocLocal {
        /// Free lists for each size class.
        free_lists: [*mut FreeListNode; NUM_SIZE_CLASSES],
        /// Pages allocated for small allocations.
        small_pages: Vec<Page>,
        /// Large allocations (each is its own mmap).
        large_pages: Vec<Page>,
        /// Active allocations for leak detection.
        active_allocations: HashMap<*mut u8, AllocationInfo>,
        /// Leak detection mode.
        leak_check_mode: LeakCheckMode,
    }

    impl AllocLocal {
        /// Create a new allocator (not boxed).
        pub fn new_raw() -> AllocLocal {
            AllocLocal {
                free_lists: [ptr::null_mut(); NUM_SIZE_CLASSES],
                small_pages: Vec::new(),
                large_pages: Vec::new(),
                active_allocations: HashMap::new(),
                leak_check_mode: LeakCheckMode::from_env(),
            }
        }

        /// Create a new allocator with a specific leak check mode (not boxed).
        #[cfg(test)]
        pub fn new_raw_with_leak_check_mode(mode: LeakCheckMode) -> AllocLocal {
            AllocLocal {
                free_lists: [ptr::null_mut(); NUM_SIZE_CLASSES],
                small_pages: Vec::new(),
                large_pages: Vec::new(),
                active_allocations: HashMap::new(),
                leak_check_mode: mode,
            }
        }

        pub unsafe fn alloc(&mut self, size: u32, align: u32, count: u32) -> *mut u8 {
            let total_size = (size as usize)
                .checked_mul(count as usize)
                .expect("allocation size overflow");

            let align = align.max(std::mem::align_of::<*mut u8>() as u32) as usize;

            unsafe {
                let ptr = if total_size > MAX_SMALL_SIZE {
                    self.alloc_large(total_size, align)
                } else {
                    self.alloc_small(total_size, align)
                };

                // Track allocation for leak detection.
                if !ptr.is_null() {
                    let backtrace = if self.leak_check_mode == LeakCheckMode::PanicWithBacktrace {
                        Some(backtrace::Backtrace::new())
                    } else {
                        None
                    };

                    self.active_allocations.insert(ptr, AllocationInfo {
                        size,
                        align: align as u32,
                        count,
                        backtrace,
                    });
                }

                ptr
            }
        }

        pub unsafe fn free(&mut self, size: u32, align: u32, count: u32, ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }

            let total_size = (size as usize)
                .checked_mul(count as usize)
                .expect("deallocation size overflow");

            let align = align.max(std::mem::align_of::<*mut u8>() as u32) as usize;

            // Remove from tracking and optionally validate.
            if let Some(info) = self.active_allocations.remove(&ptr) {
                // Validate parameters match (in warn/panic modes).
                if self.leak_check_mode != LeakCheckMode::Ignore {
                    if info.size != size || info.align != align as u32 || info.count != count {
                        let msg = format!(
                            "free() parameter mismatch: ptr={:p}, expected (size={}, align={}, count={}), got (size={}, align={}, count={})",
                            ptr, info.size, info.align, info.count, size, align, count
                        );
                        match self.leak_check_mode {
                            LeakCheckMode::Warn => eprintln!("WARNING: {}", msg),
                            LeakCheckMode::Panic | LeakCheckMode::PanicWithBacktrace => panic!("{}", msg),
                            LeakCheckMode::Ignore => {}
                        }
                    }
                }
            } else if self.leak_check_mode != LeakCheckMode::Ignore {
                // Pointer not found in tracking - possible double-free or invalid pointer.
                let msg = format!("free() called on untracked pointer: {:p}", ptr);
                match self.leak_check_mode {
                    LeakCheckMode::Warn => eprintln!("WARNING: {}", msg),
                    LeakCheckMode::Panic | LeakCheckMode::PanicWithBacktrace => panic!("{}", msg),
                    LeakCheckMode::Ignore => {}
                }
            }

            unsafe {
                if total_size > MAX_SMALL_SIZE {
                    self.free_large(ptr);
                } else {
                    self.free_small(total_size, align, ptr);
                }
            }
        }

        unsafe fn alloc_small(&mut self, size: usize, align: usize) -> *mut u8 {
            let size_class_idx = size_to_class_index(size.max(align));

            unsafe {
                if let Some(ptr) = self.pop_free_list(size_class_idx) {
                    return ptr;
                }

                self.allocate_page_for_size_class(size_class_idx);

                self.pop_free_list(size_class_idx)
                    .expect("page allocation should have created free blocks")
            }
        }

        unsafe fn free_small(&mut self, size: usize, align: usize, ptr: *mut u8) {
            let size_class_idx = size_to_class_index(size.max(align));
            unsafe {
                self.push_free_list(size_class_idx, ptr);
            }
        }

        unsafe fn alloc_large(&mut self, size: usize, align: usize) -> *mut u8 {
            let alloc_size = size + align;

            unsafe {
                let ptr = libc::mmap(
                    ptr::null_mut(),
                    alloc_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                );

                if ptr == libc::MAP_FAILED {
                    panic!("mmap failed for large allocation");
                }

                let ptr = ptr as *mut u8;
                let aligned_ptr = align_up_ptr(ptr, align);

                self.large_pages.push(Page {
                    ptr,
                    size: alloc_size,
                });

                aligned_ptr
            }
        }

        unsafe fn free_large(&mut self, ptr: *mut u8) {
            if let Some(idx) = self.large_pages.iter().position(|page| {
                let page_start = page.ptr as usize;
                let page_end = page_start + page.size;
                let ptr_addr = ptr as usize;
                ptr_addr >= page_start && ptr_addr < page_end
            }) {
                let page = self.large_pages.swap_remove(idx);
                unsafe {
                    let result = libc::munmap(page.ptr as *mut libc::c_void, page.size);
                    if result != 0 {
                        panic!("munmap failed for large allocation");
                    }
                }
            } else {
                panic!("attempted to free invalid large allocation");
            }
        }

        unsafe fn allocate_page_for_size_class(&mut self, size_class_idx: usize) {
            let block_size = SIZE_CLASSES[size_class_idx];

            unsafe {
                let ptr = libc::mmap(
                    ptr::null_mut(),
                    PAGE_SIZE,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                );

                if ptr == libc::MAP_FAILED {
                    panic!("mmap failed for small allocation page");
                }

                let ptr = ptr as *mut u8;

                self.small_pages.push(Page {
                    ptr,
                    size: PAGE_SIZE,
                });

                let num_blocks = PAGE_SIZE / block_size;
                for i in 0..num_blocks {
                    let block_ptr = ptr.add(i * block_size);
                    self.push_free_list(size_class_idx, block_ptr);
                }
            }
        }

        unsafe fn pop_free_list(&mut self, size_class_idx: usize) -> Option<*mut u8> {
            let head = self.free_lists[size_class_idx];
            if head.is_null() {
                return None;
            }

            unsafe {
                let node = &*head;
                self.free_lists[size_class_idx] = node.next;
                Some(head as *mut u8)
            }
        }

        unsafe fn push_free_list(&mut self, size_class_idx: usize, ptr: *mut u8) {
            let node = ptr as *mut FreeListNode;
            unsafe {
                (*node).next = self.free_lists[size_class_idx];
                self.free_lists[size_class_idx] = node;
            }
        }

        pub unsafe fn shutdown(mut self) {
            // Capture leak information before cleanup.
            let leak_report = if !self.active_allocations.is_empty() && self.leak_check_mode != LeakCheckMode::Ignore {
                let leaked_count = self.active_allocations.len();
                let leaked_bytes: usize = self.active_allocations
                    .values()
                    .map(|info| (info.size as usize) * (info.count as usize))
                    .sum();

                let report = format!(
                    "\nDATALOVE RUNTIME LEAK DETECTED\n\
                     ==============================\n\
                     Leaked allocations: {}\n\
                     Total leaked bytes: {}\n\n\
                     Details:",
                    leaked_count, leaked_bytes
                );

                let mut details = String::new();
                for (ptr, info) in self.active_allocations.iter().take(10) {
                    let bytes = (info.size as usize) * (info.count as usize);
                    details.push_str(&format!(
                        "\n  {:p} : size={}, align={}, count={} ({} bytes)",
                        ptr, info.size, info.align, info.count, bytes
                    ));

                    if let Some(ref bt) = info.backtrace {
                        details.push_str(&format!("\n    Backtrace:\n{:?}", bt));
                    }
                }

                if self.active_allocations.len() > 10 {
                    details.push_str(&format!("\n  ... and {} more", self.active_allocations.len() - 10));
                }

                Some(format!("{}{}\n", report, details))
            } else {
                None
            };

            // Clean up resources first.
            unsafe {
                for page in self.small_pages.drain(..) {
                    let result = libc::munmap(page.ptr as *mut libc::c_void, page.size);
                    if result != 0 {
                        eprintln!("Warning: munmap failed during shutdown");
                    }
                }

                for page in self.large_pages.drain(..) {
                    let result = libc::munmap(page.ptr as *mut libc::c_void, page.size);
                    if result != 0 {
                        eprintln!("Warning: munmap failed during shutdown");
                    }
                }
            }

            // Report leaks after cleanup.
            if let Some(full_report) = leak_report {
                match self.leak_check_mode {
                    LeakCheckMode::Warn => {
                        eprintln!("{}", full_report);
                    }
                    LeakCheckMode::Panic | LeakCheckMode::PanicWithBacktrace => {
                        panic!("{}", full_report);
                    }
                    LeakCheckMode::Ignore => {}
                }
            }
        }
    }

    fn size_to_class_index(size: usize) -> usize {
        for (i, &class_size) in SIZE_CLASSES.iter().enumerate() {
            if size <= class_size {
                return i;
            }
        }
        panic!("size exceeds maximum small allocation size");
    }

    unsafe fn align_up_ptr(ptr: *mut u8, align: usize) -> *mut u8 {
        let addr = ptr as usize;
        let aligned_addr = (addr + align - 1) & !(align - 1);
        aligned_addr as *mut u8
    }
}

// ============================================================================
// wasm32 implementation using Rust's global allocator
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use super::*;
    use std::alloc::{alloc, dealloc, Layout};

    /// Tracks an allocation for cleanup.
    struct Allocation {
        ptr: *mut u8,
        layout: Layout,
    }

    /// Local allocator state for the wasm32 platform.
    pub struct AllocLocal {
        allocations: Vec<Allocation>,
        leak_check_mode: LeakCheckMode,
    }

    impl AllocLocal {
        /// Create a new allocator (not boxed).
        pub fn new_raw() -> AllocLocal {
            AllocLocal {
                allocations: Vec::new(),
                leak_check_mode: LeakCheckMode::from_env(),
            }
        }

        /// Create a new allocator with a specific leak check mode (not boxed).
        #[cfg(test)]
        pub fn new_raw_with_leak_check_mode(mode: LeakCheckMode) -> AllocLocal {
            AllocLocal {
                allocations: Vec::new(),
                leak_check_mode: mode,
            }
        }

        pub unsafe fn alloc(&mut self, size: u32, align: u32, count: u32) -> *mut u8 {
            let total_size = (size as usize)
                .checked_mul(count as usize)
                .expect("allocation size overflow");

            let layout = Layout::from_size_align(total_size, align as usize)
                .expect("invalid layout");

            unsafe {
                let ptr = alloc(layout);
                if ptr.is_null() {
                    panic!("allocation failed");
                }

                self.allocations.push(Allocation { ptr, layout });
                ptr
            }
        }

        pub unsafe fn free(&mut self, size: u32, align: u32, count: u32, ptr: *mut u8) {
            if ptr.is_null() {
                return;
            }

            let total_size = (size as usize)
                .checked_mul(count as usize)
                .expect("deallocation size overflow");

            let layout = Layout::from_size_align(total_size, align as usize)
                .expect("invalid layout");

            unsafe {
                dealloc(ptr, layout);
            }

            // Remove from tracking.
            if let Some(idx) = self.allocations.iter().position(|a| a.ptr == ptr) {
                self.allocations.swap_remove(idx);
            }
        }

        pub unsafe fn shutdown(mut self) {
            // Capture leak information before cleanup.
            let leak_report = if !self.allocations.is_empty() && self.leak_check_mode != LeakCheckMode::Ignore {
                let leaked_count = self.allocations.len();
                let leaked_bytes: usize = self.allocations
                    .iter()
                    .map(|a| a.layout.size())
                    .sum();

                let report = format!(
                    "\nDATALOVE RUNTIME LEAK DETECTED\n\
                     ==============================\n\
                     Leaked allocations: {}\n\
                     Total leaked bytes: {}\n\n\
                     Details:",
                    leaked_count, leaked_bytes
                );

                let mut details = String::new();
                for alloc in self.allocations.iter().take(10) {
                    details.push_str(&format!(
                        "\n  {:p} : size={}, align={}",
                        alloc.ptr, alloc.layout.size(), alloc.layout.align()
                    ));
                }

                if self.allocations.len() > 10 {
                    details.push_str(&format!("\n  ... and {} more", self.allocations.len() - 10));
                }

                Some(format!("{}{}\n", report, details))
            } else {
                None
            };

            // Clean up resources first.
            for alloc in self.allocations.drain(..) {
                unsafe {
                    dealloc(alloc.ptr, alloc.layout);
                }
            }

            // Report leaks after cleanup.
            if let Some(full_report) = leak_report {
                match self.leak_check_mode {
                    LeakCheckMode::Warn => {
                        eprintln!("{}", full_report);
                    }
                    LeakCheckMode::Panic | LeakCheckMode::PanicWithBacktrace => {
                        panic!("{}", full_report);
                    }
                    LeakCheckMode::Ignore => {}
                }
            }
        }
    }
}

// ============================================================================
// Public API (platform-independent)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
pub use unix_impl::AllocLocal;

#[cfg(target_arch = "wasm32")]
pub use wasm_impl::AllocLocal;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn is_aligned(ptr: *mut u8, align: usize) -> bool {
        (ptr as usize) % align == 0
    }

    unsafe fn test_write_read(ptr: *mut u8, size: usize) {
        unsafe {
            for i in 0..size {
                *ptr.add(i) = (i % 256) as u8;
            }

            for i in 0..size {
                assert_eq!(*ptr.add(i), (i % 256) as u8);
            }
        }
    }

    #[test]
    fn test_small_alloc_each_size_class() {
        for &size in SIZE_CLASSES.iter() {
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let ptr = rt.alloc(size as u32, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));

                test_write_read(ptr, size);

                rt.free(size as u32, 8, 1, ptr);
                rt.shutdown();
            }
        }
    }

    #[test]
    fn test_large_alloc() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let sizes = [4097, 8192, 16384, 65536];
            for size in sizes {
                let ptr = rt.alloc(size, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));

                test_write_read(ptr, size as usize);

                rt.free(size, 8, 1, ptr);
            }
            rt.shutdown();
        }
    }

    #[test]
    fn test_free_null_pointer() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            rt.free(16, 8, 1, std::ptr::null_mut());
            rt.shutdown();
        }
    }

    #[test]
    fn test_free_list_reuse() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr1 = rt.alloc(32, 8, 1);
            assert!(!ptr1.is_null());

            rt.free(32, 8, 1, ptr1);

            let ptr2 = rt.alloc(32, 8, 1);
            #[cfg(not(target_arch = "wasm32"))]
            assert_eq!(ptr1, ptr2);

            rt.free(32, 8, 1, ptr2);
            rt.shutdown();
        }
    }

    #[test]
    fn test_multiple_small_allocs() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let mut ptrs = Vec::new();
            for _ in 0..100 {
                let ptr = rt.alloc(64, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));
                ptrs.push(ptr);
            }

            for i in 0..ptrs.len() {
                for j in (i + 1)..ptrs.len() {
                    assert_ne!(ptrs[i], ptrs[j]);
                }
            }

            for ptr in ptrs {
                rt.free(64, 8, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_multiple_large_allocs() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let mut ptrs = Vec::new();
            for _ in 0..10 {
                let ptr = rt.alloc(8192, 16, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 16));
                ptrs.push(ptr);
            }

            for ptr in ptrs {
                rt.free(8192, 16, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_mixed_small_large() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let small1 = rt.alloc(128, 8, 1);
            let large1 = rt.alloc(16384, 16, 1);
            let small2 = rt.alloc(256, 8, 1);
            let large2 = rt.alloc(32768, 32, 1);

            assert!(!small1.is_null());
            assert!(!large1.is_null());
            assert!(!small2.is_null());
            assert!(!large2.is_null());

            assert!(is_aligned(small1, 8));
            assert!(is_aligned(large1, 16));
            assert!(is_aligned(small2, 8));
            assert!(is_aligned(large2, 32));

            rt.free(128, 8, 1, small1);
            rt.free(16384, 16, 1, large1);
            rt.free(256, 8, 1, small2);
            rt.free(32768, 32, 1, large2);

            rt.shutdown();
        }
    }

    #[test]
    fn test_alignment_requirements() {
        let mut rt = AllocLocal::new_raw();
        let alignments = [1, 2, 4, 8, 16, 32, 64, 128];

        unsafe {
            for &align in &alignments {
                let ptr = rt.alloc(256, align, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, align as usize));
                rt.free(256, align, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_write_read_small() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr = rt.alloc(512, 8, 1);
            assert!(!ptr.is_null());

            test_write_read(ptr, 512);

            rt.free(512, 8, 1, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_write_read_large() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr = rt.alloc(65536, 16, 1);
            assert!(!ptr.is_null());

            test_write_read(ptr, 65536);

            rt.free(65536, 16, 1, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_size_class_boundaries() {
        let mut rt = AllocLocal::new_raw();
        let test_sizes = [7, 8, 9, 15, 16, 17, 31, 32, 33, 4095, 4096, 4097];

        unsafe {
            for size in test_sizes {
                let ptr = rt.alloc(size, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));
                rt.free(size, 8, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_max_alignment() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr = rt.alloc(1024, 256, 1);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 256));

            rt.free(1024, 256, 1, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_page_exhaustion() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let mut ptrs = Vec::new();
            for _ in 0..200 {
                let ptr = rt.alloc(64, 8, 1);
                assert!(!ptr.is_null());
                ptrs.push(ptr);
            }

            for ptr in ptrs {
                rt.free(64, 8, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_shutdown_cleanup() {
        // This test intentionally leaks to verify shutdown cleanup.
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Ignore);
        unsafe {
            let _small1 = rt.alloc(128, 8, 1);
            let _small2 = rt.alloc(256, 8, 1);
            let _large1 = rt.alloc(8192, 16, 1);
            let _large2 = rt.alloc(16384, 32, 1);

            rt.shutdown();
        }
    }

    #[test]
    fn test_count_parameter() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr = rt.alloc(4, 4, 10);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 4));

            test_write_read(ptr, 40);

            rt.free(4, 4, 10, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_alignment_larger_than_size() {
        let mut rt = AllocLocal::new_raw();
        unsafe {
            let ptr = rt.alloc(4, 64, 1);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 64));

            rt.free(4, 64, 1, ptr);
            rt.shutdown();
        }
    }

    proptest! {
        #[test]
        fn proptest_random_alloc_free(size in 1u32..10000, align in prop::sample::select(vec![1u32, 2, 4, 8, 16, 32, 64]), count in 1u32..10) {
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let ptr = rt.alloc(size, align, count);
                prop_assert!(!ptr.is_null());
                prop_assert!(is_aligned(ptr, align as usize));

                rt.free(size, align, count, ptr);
                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_write_read_random_data(size in 1usize..8192, data in prop::collection::vec(any::<u8>(), 1..8192)) {
            let size = size.min(data.len());
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let ptr = rt.alloc(size as u32, 8, 1);
                prop_assert!(!ptr.is_null());

                for i in 0..size {
                    *ptr.add(i) = data[i];
                }

                for i in 0..size {
                    prop_assert_eq!(*ptr.add(i), data[i]);
                }

                rt.free(size as u32, 8, 1, ptr);
                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_alignment_correctness(
            size in 1u32..4096,
            align_pow in 0usize..7,
        ) {
            let align = 1u32 << align_pow;
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let ptr = rt.alloc(size, align, 1);
                prop_assert!(!ptr.is_null());
                prop_assert!(is_aligned(ptr, align as usize));

                rt.free(size, align, 1, ptr);
                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_alloc_free_patterns(ops in prop::collection::vec((1u32..256, 1u32..5), 1..50)) {
            let mut rt = AllocLocal::new_raw();
            let mut allocations = Vec::new();

            unsafe {
                for (size, count) in &ops {
                    let ptr = rt.alloc(*size, 8, *count);
                    prop_assert!(!ptr.is_null());
                    allocations.push((*size, *count, ptr));
                }

                for (size, count, ptr) in allocations.iter().rev() {
                    rt.free(*size, 8, *count, *ptr);
                }

                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_stress_test(
            ops in prop::collection::vec((1u32..1024, prop::sample::select(vec![1u32, 2, 4, 8])), 1..100)
        ) {
            let mut rt = AllocLocal::new_raw();

            unsafe {
                for (size, align) in ops {
                    let ptr = rt.alloc(size, align, 1);
                    prop_assert!(!ptr.is_null());
                    prop_assert!(is_aligned(ptr, align as usize));
                    rt.free(size, align, 1, ptr);
                }

                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_size_boundaries(offset in 0i32..10) {
            let boundaries = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
            let mut rt = AllocLocal::new_raw();

            unsafe {
                for base in boundaries {
                    let size = (base as i32 + offset).max(1) as u32;
                    let ptr = rt.alloc(size, 8, 1);
                    prop_assert!(!ptr.is_null());
                    rt.free(size, 8, 1, ptr);
                }

                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_count_variations(size in 1u32..256, count in 1u32..100) {
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let ptr = rt.alloc(size, 8, count);
                prop_assert!(!ptr.is_null());

                let total_size = (size as usize) * (count as usize);
                if total_size <= 8192 {
                    test_write_read(ptr, total_size);
                }

                rt.free(size, 8, count, ptr);
                rt.shutdown();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    proptest! {
        #[test]
        fn proptest_reuse_freed_blocks(size in prop::sample::select(SIZE_CLASSES.to_vec())) {
            let mut rt = AllocLocal::new_raw();
            unsafe {
                let mut prev_ptr = std::ptr::null_mut();
                for i in 0..10 {
                    let ptr = rt.alloc(size as u32, 8, 1);
                    prop_assert!(!ptr.is_null());

                    if i > 0 {
                        prop_assert_eq!(ptr, prev_ptr);
                    }

                    prev_ptr = ptr;
                    rt.free(size as u32, 8, 1, ptr);
                }

                rt.shutdown();
            }
        }
    }

    // Leak detection tests.

    #[test]
    fn test_leak_detection_ignore_mode() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Ignore);
        unsafe {
            let _leak1 = rt.alloc(128, 8, 1);
            let _leak2 = rt.alloc(256, 8, 1);

            // Should not panic in Ignore mode.
            rt.shutdown();
        }
    }

    #[test]
    #[should_panic(expected = "DATALOVE RUNTIME LEAK DETECTED")]
    fn test_leak_detection_panic_mode_small() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Panic);
        unsafe {
            let _leak = rt.alloc(128, 8, 1);
            rt.shutdown(); // Should panic.
        }
    }

    #[test]
    #[should_panic(expected = "DATALOVE RUNTIME LEAK DETECTED")]
    fn test_leak_detection_panic_mode_large() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Panic);
        unsafe {
            let _leak = rt.alloc(8192, 16, 1);
            rt.shutdown(); // Should panic.
        }
    }

    #[test]
    #[should_panic(expected = "DATALOVE RUNTIME LEAK DETECTED")]
    fn test_leak_detection_panic_mode_multiple() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Panic);
        unsafe {
            let _leak1 = rt.alloc(128, 8, 1);
            let _leak2 = rt.alloc(256, 8, 1);
            let _leak3 = rt.alloc(8192, 16, 1);
            rt.shutdown(); // Should panic with 3 leaks.
        }
    }

    #[test]
    fn test_no_leak_no_panic() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Panic);
        unsafe {
            let ptr1 = rt.alloc(128, 8, 1);
            let ptr2 = rt.alloc(256, 8, 1);
            let ptr3 = rt.alloc(8192, 16, 1);

            rt.free(128, 8, 1, ptr1);
            rt.free(256, 8, 1, ptr2);
            rt.free(8192, 16, 1, ptr3);

            // Should not panic - all allocations freed.
            rt.shutdown();
        }
    }

    #[test]
    fn test_leak_detection_counts_bytes_correctly() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::Panic);
        unsafe {
            let _leak1 = rt.alloc(100, 8, 1);  // 100 bytes.
            let _leak2 = rt.alloc(50, 8, 2);   // 100 bytes.
            let _leak3 = rt.alloc(25, 8, 4);   // 100 bytes.

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt.shutdown();
            }));

            assert!(result.is_err());
            if let Err(e) = result {
                let msg = e.downcast_ref::<String>().unwrap();
                assert!(msg.contains("Total leaked bytes: 300"));
                assert!(msg.contains("Leaked allocations: 3"));
            }
        }
    }

    #[test]
    #[should_panic(expected = "DATALOVE RUNTIME LEAK DETECTED")]
    fn test_leak_detection_panic_with_backtrace_mode() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::PanicWithBacktrace);
        unsafe {
            let _leak = rt.alloc(128, 8, 1);
            rt.shutdown(); // Should panic with backtrace.
        }
    }

    #[test]
    fn test_leak_detection_panic_with_backtrace_includes_backtrace() {
        let mut rt = AllocLocal::new_raw_with_leak_check_mode(LeakCheckMode::PanicWithBacktrace);
        unsafe {
            let _leak = rt.alloc(128, 8, 1);

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt.shutdown();
            }));

            assert!(result.is_err());
            if let Err(e) = result {
                let msg = e.downcast_ref::<String>().unwrap();
                assert!(msg.contains("DATALOVE RUNTIME LEAK DETECTED"));
                assert!(msg.contains("Backtrace:"));
            }
        }
    }
}
