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
        // Also validates that align is a non-zero power of two, which the
        // zero-size branch below relies on.
        let layout = Layout::from_size_align(size, align)
            .expect("invalid layout");

        if size == 0 {
            // Nothing to allocate, but callers still offset from this pointer
            // and assert its alignment, so it has to meet the request.
            // NonNull::dangling would give address 1, being u8's alignment
            // rather than the caller's. The address is never dereferenced or
            // freed, so it carries no provenance.
            let ptr = NonNull::new(std::ptr::without_provenance_mut::<u8>(align))
                .expect("alignment is non-zero");
            return Self { ptr, layout };
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_size_buffer_is_aligned() {
        for align in [1usize, 2, 4, 8, 16, 32, 64] {
            let buf = AlignedBuffer::with_align(0, align);
            assert_eq!(
                buf.as_ptr() as usize % align, 0,
                "zero-size buffer for align {} is at {:p}", align, buf.as_ptr()
            );
            assert!(buf.is_empty());
            assert_eq!(buf.as_slice(), &[] as &[u8]);
        }
    }

    #[test]
    fn allocated_buffer_is_aligned_and_zeroed() {
        for align in [1usize, 2, 4, 8, 16, 32, 64] {
            let buf = AlignedBuffer::with_align(128, align);
            assert_eq!(buf.as_ptr() as usize % align, 0);
            assert_eq!(buf.len(), 128);
            assert!(buf.as_slice().iter().all(|b| *b == 0));
        }
    }
}
