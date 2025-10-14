//! Simple segregated free list allocator for the Datalove runtime.
//!
//! Single-threaded allocator using mmap for page allocation.

use rmx::prelude::*;
use std::ptr;

/// Size of a memory page (4KB).
const PAGE_SIZE: usize = 4096;

/// Size classes for small allocations.
/// Allocations > MAX_SMALL_SIZE use direct mmap.
const SIZE_CLASSES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
const MAX_SMALL_SIZE: usize = 4096;
const NUM_SIZE_CLASSES: usize = SIZE_CLASSES.len();

/// A node in the intrusive free list.
///
/// When a block is free, the first bytes store a pointer to the next free block.
#[repr(C)]
struct FreeListNode {
    next: *mut FreeListNode,
}

/// Tracks a page allocated via mmap.
struct Page {
    ptr: *mut u8,
    size: usize,
}

/// Runtime state for the allocator.
pub struct LocalRt {
    /// Free lists for each size class.
    free_lists: [*mut FreeListNode; NUM_SIZE_CLASSES],
    /// Pages allocated for small allocations.
    small_pages: Vec<Page>,
    /// Large allocations (each is its own mmap).
    large_pages: Vec<Page>,
}

impl LocalRt {
    /// Create a new runtime with empty free lists.
    pub fn new() -> Box<LocalRt> {
        Box::new(LocalRt {
            free_lists: [ptr::null_mut(); NUM_SIZE_CLASSES],
            small_pages: Vec::new(),
            large_pages: Vec::new(),
        })
    }

    /// Allocate memory for `count` elements of the given type.
    pub unsafe fn alloc(&mut self, size: u32, align: u32, count: u32) -> *mut u8 {
        let total_size = (size as usize)
            .checked_mul(count as usize)
            .expect("allocation size overflow");

        // Ensure alignment is at least pointer-aligned for free list nodes.
        let align = align.max(std::mem::align_of::<*mut u8>() as u32) as usize;

        unsafe {
            if total_size > MAX_SMALL_SIZE {
                self.alloc_large(total_size, align)
            } else {
                self.alloc_small(total_size, align)
            }
        }
    }

    /// Free memory for `count` elements of the given type.
    pub unsafe fn free(&mut self, size: u32, _align: u32, count: u32, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        let total_size = (size as usize)
            .checked_mul(count as usize)
            .expect("deallocation size overflow");

        unsafe {
            if total_size > MAX_SMALL_SIZE {
                self.free_large(ptr);
            } else {
                self.free_small(total_size, ptr);
            }
        }
    }

    /// Allocate a small block from a size class.
    unsafe fn alloc_small(&mut self, size: usize, align: usize) -> *mut u8 {
        let size_class_idx = size_to_class_index(size.max(align));

        unsafe {
            // Try to pop from the free list.
            if let Some(ptr) = self.pop_free_list(size_class_idx) {
                return ptr;
            }

            // No free blocks - allocate a new page and carve it up.
            self.allocate_page_for_size_class(size_class_idx);

            // Now there should be free blocks.
            self.pop_free_list(size_class_idx)
                .expect("page allocation should have created free blocks")
        }
    }

    /// Free a small block to its size class free list.
    unsafe fn free_small(&mut self, size: usize, ptr: *mut u8) {
        let size_class_idx = size_to_class_index(size);
        unsafe {
            self.push_free_list(size_class_idx, ptr);
        }
    }

    /// Allocate a large block using direct mmap.
    unsafe fn alloc_large(&mut self, size: usize, align: usize) -> *mut u8 {
        // Allocate extra space for alignment padding.
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

            // Align the pointer.
            let aligned_ptr = align_up_ptr(ptr, align);

            // Track this allocation.
            // Note: we store the original ptr for munmap, not the aligned one.
            self.large_pages.push(Page {
                ptr,
                size: alloc_size,
            });

            aligned_ptr
        }
    }

    /// Free a large block.
    unsafe fn free_large(&mut self, ptr: *mut u8) {
        // Find and remove the page from large_pages.
        // This is O(n) but large allocations should be rare.
        if let Some(idx) = self.large_pages.iter().position(|page| {
            // Check if ptr falls within this allocation.
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

    /// Allocate a page and carve it into blocks for the given size class.
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

            // Track this page.
            self.small_pages.push(Page {
                ptr,
                size: PAGE_SIZE,
            });

            // Carve the page into blocks and add them to the free list.
            let num_blocks = PAGE_SIZE / block_size;
            for i in 0..num_blocks {
                let block_ptr = ptr.add(i * block_size);
                self.push_free_list(size_class_idx, block_ptr);
            }
        }
    }

    /// Pop a block from the free list for the given size class.
    unsafe fn pop_free_list(&mut self, size_class_idx: usize) -> Option<*mut u8> {
        let head = self.free_lists[size_class_idx];
        if head.is_null() {
            return None;
        }

        unsafe {
            // Remove head from the list.
            let node = &*head;
            self.free_lists[size_class_idx] = node.next;

            Some(head as *mut u8)
        }
    }

    /// Push a block onto the free list for the given size class.
    unsafe fn push_free_list(&mut self, size_class_idx: usize, ptr: *mut u8) {
        let node = ptr as *mut FreeListNode;
        unsafe {
            (*node).next = self.free_lists[size_class_idx];
            self.free_lists[size_class_idx] = node;
        }
    }

    /// Clean up all allocated pages.
    pub unsafe fn shutdown(mut self: Box<Self>) {
        unsafe {
            // Free small pages.
            for page in self.small_pages.drain(..) {
                let result = libc::munmap(page.ptr as *mut libc::c_void, page.size);
                if result != 0 {
                    // Don't panic during shutdown, just continue.
                    eprintln!("Warning: munmap failed during shutdown");
                }
            }

            // Free large pages.
            for page in self.large_pages.drain(..) {
                let result = libc::munmap(page.ptr as *mut libc::c_void, page.size);
                if result != 0 {
                    eprintln!("Warning: munmap failed during shutdown");
                }
            }
        }
    }
}

/// Round a size up to the appropriate size class index.
fn size_to_class_index(size: usize) -> usize {
    for (i, &class_size) in SIZE_CLASSES.iter().enumerate() {
        if size <= class_size {
            return i;
        }
    }
    // Should not reach here if size <= MAX_SMALL_SIZE.
    panic!("size exceeds maximum small allocation size");
}

/// Align a pointer up to the given alignment.
unsafe fn align_up_ptr(ptr: *mut u8, align: usize) -> *mut u8 {
    let addr = ptr as usize;
    let aligned_addr = (addr + align - 1) & !(align - 1);
    aligned_addr as *mut u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Helper function to check if a pointer is aligned.
    fn is_aligned(ptr: *mut u8, align: usize) -> bool {
        (ptr as usize) % align == 0
    }

    // Helper function to write a pattern to memory and verify it.
    unsafe fn test_write_read(ptr: *mut u8, size: usize) {
        unsafe {
            // Write a pattern.
            for i in 0..size {
                *ptr.add(i) = (i % 256) as u8;
            }

            // Read and verify.
            for i in 0..size {
                assert_eq!(*ptr.add(i), (i % 256) as u8);
            }
        }
    }

    #[test]
    fn test_small_alloc_each_size_class() {
        for &size in SIZE_CLASSES.iter() {
            let mut rt = LocalRt::new();
            unsafe {
                let ptr = rt.alloc(size as u32, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));

                // Write and read to verify memory is usable.
                test_write_read(ptr, size);

                rt.free(size as u32, 8, 1, ptr);
                rt.shutdown();
            }
        }
    }

    #[test]
    fn test_large_alloc() {
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
        unsafe {
            // Should not panic.
            rt.free(16, 8, 1, std::ptr::null_mut());
            rt.shutdown();
        }
    }

    #[test]
    fn test_free_list_reuse() {
        let mut rt = LocalRt::new();
        unsafe {
            // Allocate and free from same size class.
            let ptr1 = rt.alloc(32, 8, 1);
            assert!(!ptr1.is_null());

            rt.free(32, 8, 1, ptr1);

            // Next allocation should reuse the freed block.
            let ptr2 = rt.alloc(32, 8, 1);
            assert_eq!(ptr1, ptr2);

            rt.free(32, 8, 1, ptr2);
            rt.shutdown();
        }
    }

    #[test]
    fn test_multiple_small_allocs() {
        let mut rt = LocalRt::new();
        unsafe {
            let mut ptrs = Vec::new();
            for _ in 0..100 {
                let ptr = rt.alloc(64, 8, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 8));
                ptrs.push(ptr);
            }

            // Verify all pointers are unique.
            for i in 0..ptrs.len() {
                for j in (i + 1)..ptrs.len() {
                    assert_ne!(ptrs[i], ptrs[j]);
                }
            }

            // Free all.
            for ptr in ptrs {
                rt.free(64, 8, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_multiple_large_allocs() {
        let mut rt = LocalRt::new();
        unsafe {
            let mut ptrs = Vec::new();
            for _ in 0..10 {
                let ptr = rt.alloc(8192, 16, 1);
                assert!(!ptr.is_null());
                assert!(is_aligned(ptr, 16));
                ptrs.push(ptr);
            }

            // Free all.
            for ptr in ptrs {
                rt.free(8192, 16, 1, ptr);
            }

            rt.shutdown();
        }
    }

    #[test]
    fn test_mixed_small_large() {
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
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
        let mut rt = LocalRt::new();
        unsafe {
            // Test maximum reasonable alignment.
            let ptr = rt.alloc(1024, 256, 1);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 256));

            rt.free(1024, 256, 1, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_page_exhaustion() {
        let mut rt = LocalRt::new();
        unsafe {
            // Allocate enough 64-byte blocks to exhaust multiple pages.
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
        let mut rt = LocalRt::new();
        unsafe {
            // Allocate various sizes.
            let _small1 = rt.alloc(128, 8, 1);
            let _small2 = rt.alloc(256, 8, 1);
            let _large1 = rt.alloc(8192, 16, 1);
            let _large2 = rt.alloc(16384, 32, 1);

            // Shutdown should clean up all pages without panic.
            rt.shutdown();
        }
    }

    #[test]
    fn test_count_parameter() {
        let mut rt = LocalRt::new();
        unsafe {
            // Allocate array of 10 u32s.
            let ptr = rt.alloc(4, 4, 10);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 4));

            // Should have allocated 40 bytes.
            test_write_read(ptr, 40);

            rt.free(4, 4, 10, ptr);
            rt.shutdown();
        }
    }

    #[test]
    fn test_alignment_larger_than_size() {
        let mut rt = LocalRt::new();
        unsafe {
            // Alignment larger than size.
            let ptr = rt.alloc(4, 64, 1);
            assert!(!ptr.is_null());
            assert!(is_aligned(ptr, 64));

            rt.free(4, 64, 1, ptr);
            rt.shutdown();
        }
    }

    // Property-based tests using proptest.

    proptest! {
        #[test]
        fn proptest_random_alloc_free(size in 1u32..10000, align in prop::sample::select(vec![1u32, 2, 4, 8, 16, 32, 64]), count in 1u32..10) {
            let mut rt = LocalRt::new();
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
            let mut rt = LocalRt::new();
            unsafe {
                let ptr = rt.alloc(size as u32, 8, 1);
                prop_assert!(!ptr.is_null());

                // Write data.
                for i in 0..size {
                    *ptr.add(i) = data[i];
                }

                // Read and verify.
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
            align_pow in 0usize..7, // 2^0 to 2^6 = 1 to 64
        ) {
            let align = 1u32 << align_pow;
            let mut rt = LocalRt::new();
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
            let mut rt = LocalRt::new();
            let mut allocations = Vec::new();

            unsafe {
                // Allocate all.
                for (size, count) in &ops {
                    let ptr = rt.alloc(*size, 8, *count);
                    prop_assert!(!ptr.is_null());
                    allocations.push((*size, *count, ptr));
                }

                // Free in reverse order (LIFO).
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
            let mut rt = LocalRt::new();

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
            // Test around size class boundaries.
            let boundaries = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
            let mut rt = LocalRt::new();

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
            let mut rt = LocalRt::new();
            unsafe {
                let ptr = rt.alloc(size, 8, count);
                prop_assert!(!ptr.is_null());

                let total_size = (size as usize) * (count as usize);
                if total_size <= 8192 {
                    // Test writing to the entire allocation.
                    test_write_read(ptr, total_size);
                }

                rt.free(size, 8, count, ptr);
                rt.shutdown();
            }
        }
    }

    proptest! {
        #[test]
        fn proptest_reuse_freed_blocks(size in prop::sample::select(SIZE_CLASSES.to_vec())) {
            let mut rt = LocalRt::new();
            unsafe {
                // Allocate and free multiple times.
                let mut prev_ptr = std::ptr::null_mut();
                for i in 0..10 {
                    let ptr = rt.alloc(size as u32, 8, 1);
                    prop_assert!(!ptr.is_null());

                    if i > 0 {
                        // After first free, subsequent allocations should reuse.
                        prop_assert_eq!(ptr, prev_ptr);
                    }

                    prev_ptr = ptr;
                    rt.free(size as u32, 8, 1, ptr);
                }

                rt.shutdown();
            }
        }
    }
}
