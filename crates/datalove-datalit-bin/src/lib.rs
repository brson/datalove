//! An ordered binary encoding of datalit values.
//!
//! A value is encoded at its type, and two values of one type encode so that
//! `memcmp` orders their bytes as datalit orders the values. The encoding is
//! canonical, so equal bytes are equal values, and it is prefix-free, so a
//! tuple's encoding begins with its first element's. That makes encoded
//! values usable as the keys of a byte-ordered store, with range and prefix
//! scans meaning what they say. A value at type `data` carries its type's
//! encoding before its own, so a `data` is the self-describing form.
//!
//! The format is described in `botdocs/datalit-binary.md`. The type encoder
//! is `datalove_rtdt::tyenc`, as the runtime orders types by it; reading types
//! back into descriptors is [`TyArena`], here.
//!
//! Values are the runtime's own: a pointer and a descriptor. Encoding reads
//! one; decoding writes one into space the caller gives, allocating what it
//! points to through the runtime. Both are `unsafe`, as the runtime is, and
//! memory is the caller's to manage: a decoded value is destroyed through the
//! runtime of the thread that decoded it, and the [`TyArena`] that made the
//! descriptors of any `data` in it must outlive it.
//!
//! `index` and `offset` are written at the width the build has, so an index-32
//! encoding and an index-64 one differ in those types and are not read by a
//! build of the other width.

mod decode;
mod encode;
pub mod read;
pub mod tybuild;

pub use read::{DecodeError, DecodeErrorKind};
pub use tybuild::{TyArena, TyTree, layouts_agree};
pub use datalove_rtdt::tyenc::{cmp_types, encode_type, type_bytes};
pub use decode::{decode, MAX_VALUE_DEPTH};
pub use encode::{encode, encode_data};

use datalove_rtdt::ordered::write_varint;
use read::{Reader, read_varint};

/// The header byte of the `int` zero.
const INT_ZERO: u8 = 0x80;
/// The longest magnitude, in bytes, whose length the header byte holds.
const INT_SHORT_MAX: usize = 126;
/// The header of a positive `int` whose length follows it.
const INT_POSITIVE_LONG: u8 = 0xFF;
/// The header of a negative `int` whose complemented length follows it.
const INT_NEGATIVE_LONG: u8 = 0x01;

/// Write an `int` from its sign and its 32-bit limbs, least significant
/// first.
///
/// A header byte says the sign and the magnitude's length in bytes, then the
/// magnitude follows big-endian. A negative number complements its length and
/// magnitude, so that a larger magnitude sorts first.
fn write_int(out: &mut Vec<u8>, negative: bool, limbs: &[u32]) {
    let mut magnitude: Vec<u8> = limbs.iter().rev().flat_map(|l| l.to_be_bytes()).collect();
    let leading = magnitude.iter().take_while(|&&b| b == 0).count();
    magnitude.drain(..leading);
    let len = magnitude.len();
    if len == 0 {
        out.push(INT_ZERO);
        return;
    }
    if !negative {
        if len <= INT_SHORT_MAX {
            out.push(INT_ZERO + len as u8);
        } else {
            out.push(INT_POSITIVE_LONG);
            write_varint(out, len as u64);
        }
        out.extend_from_slice(&magnitude);
    } else {
        if len <= INT_SHORT_MAX {
            out.push(INT_ZERO - len as u8);
        } else {
            out.push(INT_NEGATIVE_LONG);
            let start = out.len();
            write_varint(out, len as u64);
            out[start..].iter_mut().for_each(|b| *b = !*b);
        }
        out.extend(magnitude.iter().map(|b| !b));
    }
}

/// Read an `int`: whether it is negative, and its limbs, least significant
/// first, with no high zero limb.
fn read_int(r: &mut Reader<'_>) -> Result<(bool, Vec<u32>), DecodeError> {
    let at = r.pos();
    let header = r.byte()?;
    let (negative, len) = match header {
        INT_ZERO => return Ok((false, Vec::new())),
        0x81..=0xFE => (false, (header - INT_ZERO) as usize),
        0x02..=0x7F => (true, (INT_ZERO - header) as usize),
        INT_POSITIVE_LONG => (false, read_long_len(r, false)?),
        INT_NEGATIVE_LONG => (true, read_long_len(r, true)?),
        _ => return Err(DecodeError { at, kind: DecodeErrorKind::BadByte(header) }),
    };
    let mag_at = r.pos();
    let mut magnitude = r.take(len)?.to_vec();
    if negative {
        magnitude.iter_mut().for_each(|b| *b = !*b);
    }
    if magnitude[0] == 0 {
        return Err(DecodeError { at: mag_at, kind: DecodeErrorKind::NotMinimal });
    }
    let mut padded = vec![0u8; (4 - len % 4) % 4];
    padded.extend_from_slice(&magnitude);
    let limbs = padded.chunks(4).rev().map(|c| u32::from_be_bytes(c.try_into().expect("four bytes"))).collect();
    Ok((negative, limbs))
}

/// The length after a long `int` header, which must be too long for a short
/// one.
fn read_long_len(r: &mut Reader<'_>, complemented: bool) -> Result<usize, DecodeError> {
    let at = r.pos();
    let len = if complemented {
        // The complement of a varint: complement each byte back as it is
        // read, the first saying how many follow.
        let first = !r.peek()?;
        let n = if first < 0xF8 { 1 } else { 1 + (first - 0xF7) as usize };
        let bytes: Vec<u8> = r.take(n)?.iter().map(|b| !b).collect();
        let mut inner = Reader::new(&bytes);
        read_varint(&mut inner).map_err(|e| DecodeError { at: at + e.at, kind: e.kind })?
    } else {
        read_varint(r)?
    };
    if len <= INT_SHORT_MAX as u64 {
        return Err(DecodeError { at, kind: DecodeErrorKind::NotMinimal });
    }
    usize::try_from(len).map_err(|_| DecodeError { at, kind: DecodeErrorKind::OutOfRange })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(negative: bool, limbs: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        write_int(&mut out, negative, limbs);
        out
    }

    /// Limbs of a number of `n` bytes, all `0xFF`.
    fn ones(n: usize) -> Vec<u32> {
        let mut bytes = vec![0xFFu8; n];
        bytes.reverse();
        let mut limbs = Vec::new();
        for chunk in bytes.chunks(4) {
            let mut b = [0u8; 4];
            b[..chunk.len()].copy_from_slice(chunk);
            limbs.push(u32::from_le_bytes(b));
        }
        limbs
    }

    #[test]
    fn ints_sort_numerically_and_round_trip() {
        let values: Vec<(bool, Vec<u32>)> = vec![
            (true, ones(200)),
            (true, ones(127)),
            (true, ones(126)),
            (true, vec![0, 1]),
            (true, vec![256]),
            (true, vec![2]),
            (true, vec![1]),
            (false, vec![]),
            (false, vec![1]),
            (false, vec![2]),
            (false, vec![256]),
            (false, vec![0, 1]),
            (false, ones(126)),
            (false, ones(127)),
            (false, ones(200)),
        ];
        for pair in values.windows(2) {
            assert!(int(pair[0].0, &pair[0].1) < int(pair[1].0, &pair[1].1), "{pair:?}");
        }
        for (negative, limbs) in values {
            let bytes = int(negative, &limbs);
            let mut r = Reader::new(&bytes);
            assert_eq!(read_int(&mut r).unwrap(), (negative && !limbs.is_empty(), limbs));
            r.finish().unwrap();
        }
    }

    #[test]
    fn non_minimal_ints_are_rejected() {
        for bytes in [&[0x82, 0x00, 0x01][..], &[0x7E, 0xFF, 0xFE], &[0xFF, 0x05, 1, 2, 3, 4, 5]] {
            let mut r = Reader::new(bytes);
            assert_eq!(read_int(&mut r).unwrap_err().kind, DecodeErrorKind::NotMinimal, "{bytes:?}");
        }
    }
}
