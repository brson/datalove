//! Order-preserving byte encodings, the writing half.
//!
//! Each encoding here is canonical and prefix-free, and two encodings compare
//! with `memcmp` as their values do. The type encoding in [`crate::tyenc`] is
//! built from these, and so is the value encoding in `datalove-datalit-bin`,
//! which also reads them back. See `botdocs/datalit-binary.md`.

/// The first byte of a varint that is followed by more bytes.
pub const VARINT_LONG: u8 = 0xF8;

/// Write an unsigned integer as an ordered varint.
///
/// Below `F8` it is the one byte. Otherwise it is `F7 + n` and then `n`
/// bytes big-endian, `n` from 1 to 8, as few as hold it.
pub fn write_varint(out: &mut Vec<u8>, value: u64) {
    if value < VARINT_LONG as u64 {
        out.push(value as u8);
        return;
    }
    let n = 8 - (value.leading_zeros() / 8) as usize;
    out.push(VARINT_LONG - 1 + n as u8);
    out.extend_from_slice(&value.to_be_bytes()[8 - n..]);
}

/// Write bytes escaped and terminated, so that they sort lexicographically
/// with a prefix first: `00` is written `00 FF`, and the end is `00 01`.
pub fn write_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        out.push(b);
        if b == 0 {
            out.push(0xFF);
        }
    }
    out.push(0);
    out.push(1);
}

/// Write a string: its UTF-8 bytes escaped and terminated.
pub fn write_str(out: &mut Vec<u8>, s: &str) {
    write_escaped(out, s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, v);
        out
    }

    #[test]
    fn varints_sort() {
        let values = [0, 1, 0xF7, 0xF8, 0xFF, 0x100, 0xFFFF, 0x1_0000, u32::MAX as u64, u64::MAX - 1, u64::MAX];
        for pair in values.windows(2) {
            assert!(varint(pair[0]) < varint(pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn escaped_bytes_sort_with_prefixes_first() {
        let runs: [&[u8]; 6] = [b"", b"\0", b"\0\0", b"\0a", b"a", b"a\0"];
        let enc = |b: &[u8]| {
            let mut out = Vec::new();
            write_escaped(&mut out, b);
            out
        };
        for pair in runs.windows(2) {
            assert!(enc(pair[0]) < enc(pair[1]), "{pair:?}");
        }
    }
}
