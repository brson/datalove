//! Aligned memory buffer for runtime data.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr::NonNull;

/// A byte buffer with configurable alignment.
pub struct AlignedBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl AlignedBuffer {
    /// Maximum alignment, sufficient for AVX-512, cache lines, and all scalar types.
    pub const MAX_ALIGN: usize = 64;

    /// Create a new buffer with maximum (64-byte) alignment, zero-initialized.
    pub fn new(size: usize) -> Self {
        Self::with_align(size, Self::MAX_ALIGN)
    }

    /// Create a new buffer with specified alignment, zero-initialized.
    pub fn with_align(size: usize, align: usize) -> Self {
        if size == 0 {
            // Return a dangling but aligned pointer for zero-size.
            return Self {
                ptr: NonNull::dangling(),
                layout: Layout::from_size_align(0, align).unwrap(),
            };
        }

        let layout = Layout::from_size_align(size, align)
            .expect("invalid layout");
        let ptr = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| {
            std::alloc::handle_alloc_error(layout)
        });

        Self { ptr, layout }
    }

    /// Get a const pointer to the buffer.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Get a mutable pointer to the buffer.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Get the buffer size in bytes.
    pub fn len(&self) -> usize {
        self.layout.size()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.layout.size() == 0
    }

    /// Get the buffer as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        if self.layout.size() == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.layout.size()) }
        }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        if self.layout.size() > 0 {
            unsafe { dealloc(self.ptr.as_ptr(), self.layout) }
        }
    }
}
