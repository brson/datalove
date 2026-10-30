//! Moving one value's bytes when its size is known only at run time.
//!
//! A value of a type parameter is copied by the size its descriptor gives, and
//! a copy of a size the compiler cannot see is a call to libc's `memcpy`,
//! which for the few words most values are costs more than the copy. These
//! copy the common small sizes in fixed-size loads and stores, and leave the
//! rest to libc. They are for one value; a run of elements is better left to
//! `memcpy`, which is fast at sizes worth calling it for.

use std::ptr;
use std::sync::atomic::{compiler_fence, Ordering};

/// Copy `size` bytes from `src` to `dst`, which may overlap.
///
/// # Safety
///
/// As for `ptr::copy::<u8>(src, dst, size)`.
#[inline(always)]
pub unsafe fn copy_value(src: *const u8, dst: *mut u8, size: usize) {
    match size {
        0 => {}
        1 => unsafe { dst.write(src.read()) },
        2 => unsafe { (dst as *mut u16).write_unaligned((src as *const u16).read_unaligned()) },
        4 => unsafe { (dst as *mut u32).write_unaligned((src as *const u32).read_unaligned()) },
        8 => unsafe { copy_words::<1>(src, dst) },
        16 => unsafe { copy_words::<2>(src, dst) },
        24 => unsafe { copy_words::<3>(src, dst) },
        32 => unsafe { copy_words::<4>(src, dst) },
        _ => unsafe { ptr::copy(src, dst, size) },
    }
}

/// Copy `N` eight-byte words, reading all of them before writing any.
///
/// A word at a time, never a wider load. The value was most likely just
/// written, by compiled code that stores a word or less at a time, and a load
/// wider than the store that wrote its bytes cannot take them from that store
/// while it is still in flight: it waits for the store to reach the cache.
/// A 16-byte load of a value written as two words measured as a stall on
/// every push. Left alone, LLVM merges adjacent word copies into exactly that
/// load, which the fences between them prevent; they emit no instructions.
#[inline(always)]
unsafe fn copy_words<const N: usize>(src: *const u8, dst: *mut u8) {
    let mut words = [0u64; N];
    for (i, w) in words.iter_mut().enumerate() {
        *w = unsafe { (src as *const u64).add(i).read_unaligned() };
        compiler_fence(Ordering::SeqCst);
    }
    for (i, w) in words.iter().enumerate() {
        unsafe { (dst as *mut u64).add(i).write_unaligned(*w) };
        compiler_fence(Ordering::SeqCst);
    }
}

/// Exchange `size` bytes at `a` and `b`, which do not overlap.
///
/// # Safety
///
/// As for `ptr::swap_nonoverlapping::<u8>(a, b, size)`.
#[inline(always)]
pub unsafe fn swap_value(a: *mut u8, b: *mut u8, size: usize) {
    macro_rules! fixed {
        ($n:literal) => {
            unsafe { ptr::swap_nonoverlapping(a as *mut [u8; $n], b as *mut [u8; $n], 1) }
        };
    }
    match size {
        0 => {}
        1 => fixed!(1),
        2 => fixed!(2),
        4 => fixed!(4),
        8 => fixed!(8),
        12 => fixed!(12),
        16 => fixed!(16),
        24 => fixed!(24),
        32 => fixed!(32),
        _ => unsafe { ptr::swap_nonoverlapping(a, b, size) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_swap_every_size() {
        for size in 0..=40 {
            let a: Vec<u8> = (0..size as u8).collect();
            let b: Vec<u8> = (0..size as u8).map(|x| x ^ 0xA5).collect();
            let mut out = vec![0u8; size];
            unsafe { copy_value(a.as_ptr(), out.as_mut_ptr(), size) };
            assert_eq!(out, a);

            let (mut x, mut y) = (a.clone(), b.clone());
            unsafe { swap_value(x.as_mut_ptr(), y.as_mut_ptr(), size) };
            assert_eq!((x, y), (b, a));
        }
    }
}
