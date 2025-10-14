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
