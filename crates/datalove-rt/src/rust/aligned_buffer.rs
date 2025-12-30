//! Aligned memory buffer for runtime data.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr::NonNull;

/// A byte buffer with 64-byte alignment.
///
/// 64 bytes covers AVX-512, cache lines, and all standard scalar types.
/// This alignment is sufficient for any practical runtime data.
pub struct AlignedBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl AlignedBuffer {
    /// Alignment for all buffers.
    pub const ALIGN: usize = 64;

    /// Create a new buffer of the given size, zero-initialized.
    pub fn new(size: usize) -> Self {
        if size == 0 {
            // Return a dangling but aligned pointer for zero-size.
            return Self {
                ptr: NonNull::dangling(),
                layout: Layout::from_size_align(0, Self::ALIGN).unwrap(),
            };
        }

        let layout = Layout::from_size_align(size, Self::ALIGN)
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
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        if self.layout.size() > 0 {
            unsafe { dealloc(self.ptr.as_ptr(), self.layout) }
        }
    }
}
