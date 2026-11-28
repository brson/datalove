//! Reading the order-preserving byte encodings, and why a read fails.
//!
//! The writing half is `datalove_rtdt::ordered`, which the runtime's type
//! encoder needs; reading is only for decoding, so it is here.

use datalove_rtdt::ordered::VARINT_LONG;

/// Why a run of bytes is not a canonical encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// Where in the input the problem was found.
    pub at: usize,
    pub kind: DecodeErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeErrorKind {
    /// The input ended inside an encoding.
    Truncated,
    /// A byte that no encoding starts with or continues with here.
    BadByte(u8),
    /// A number written with more bytes than it needs.
    NotMinimal,
    /// A string that is not UTF-8.
    NotUtf8,
    /// A set or map whose elements are not strictly ascending.
    NotAscending,
    /// A number too large for where it goes.
    OutOfRange,
    /// A name that is not a datalit identifier.
    BadName,
    /// Two fields or columns with one name.
    DuplicateName,
    /// An enum value naming a variant its type does not have.
    NoSuchVariant,
    /// A type that recurs other than through a collection.
    Unguarded,
    /// A type written in a form other than its canonical one.
    NotCanonical,
    /// Values nested deeper than the decoder allows.
    TooDeep,
    /// Bytes left over after the value.
    Trailing,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} at byte {}", self.kind, self.at)
    }
}

impl std::error::Error for DecodeError {}

/// A cursor over input bytes that reports where it failed.
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// How many bytes are left.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }

    /// The bytes between `start` and where the reader is.
    pub fn since(&self, start: usize) -> &'a [u8] {
        &self.bytes[start..self.pos]
    }

    pub fn error(&self, kind: DecodeErrorKind) -> DecodeError {
        DecodeError { at: self.pos, kind }
    }

    pub fn peek(&self) -> Result<u8, DecodeError> {
        self.bytes.get(self.pos).copied().ok_or(self.error(DecodeErrorKind::Truncated))
    }

    pub fn byte(&mut self) -> Result<u8, DecodeError> {
        let b = self.peek()?;
        self.pos += 1;
        Ok(b)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.bytes.len() - self.pos < n {
            return Err(self.error(DecodeErrorKind::Truncated));
        }
        let taken = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(taken)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        Ok(self.take(N)?.try_into().expect("took N bytes"))
    }

    /// Read the end of the input, failing if anything is left.
    pub fn finish(&self) -> Result<(), DecodeError> {
        if self.is_empty() { Ok(()) } else { Err(self.error(DecodeErrorKind::Trailing)) }
    }

    /// Read one byte that must be `0` or `1`.
    pub fn flag(&mut self) -> Result<bool, DecodeError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            b => {
                self.pos -= 1;
                Err(self.error(DecodeErrorKind::BadByte(b)))
            }
        }
    }
}


pub fn read_varint(r: &mut Reader<'_>) -> Result<u64, DecodeError> {
    let first = r.byte()?;
    if first < VARINT_LONG {
        return Ok(first as u64);
    }
    let n = (first - (VARINT_LONG - 1)) as usize;
    let start = r.pos();
    let bytes = r.take(n)?;
    let mut buf = [0u8; 8];
    buf[8 - n..].copy_from_slice(bytes);
    let value = u64::from_be_bytes(buf);
    // Minimal: one byte fewer would not hold it, and one byte holds only what
    // the short form cannot.
    let minimal = if n == 1 { value >= VARINT_LONG as u64 } else { bytes[0] != 0 };
    if !minimal {
        return Err(DecodeError { at: start, kind: DecodeErrorKind::NotMinimal });
    }
    Ok(value)
}

/// Read a varint that must fit in `usize`.
pub fn read_varint_usize(r: &mut Reader<'_>) -> Result<usize, DecodeError> {
    let at = r.pos();
    let value = read_varint(r)?;
    usize::try_from(value).map_err(|_| DecodeError { at, kind: DecodeErrorKind::OutOfRange })
}


/// Read bytes written by [`write_escaped`], unescaped into `into`.
pub fn read_escaped(r: &mut Reader<'_>, into: &mut Vec<u8>) -> Result<(), DecodeError> {
    loop {
        let b = r.byte()?;
        if b != 0 {
            into.push(b);
            continue;
        }
        match r.byte()? {
            0xFF => into.push(0),
            0x01 => return Ok(()),
            other => {
                return Err(DecodeError { at: r.pos() - 1, kind: DecodeErrorKind::BadByte(other) });
            }
        }
    }
}


pub fn read_string(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let start = r.pos();
    let mut bytes = Vec::new();
    read_escaped(r, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| DecodeError { at: start, kind: DecodeErrorKind::NotUtf8 })
}

/// Whether a name is a datalit identifier: ASCII letters, digits and `_`, not
/// starting with a digit.
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.bytes();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == b'_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Read a name, which must be an identifier.
pub fn read_name(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let start = r.pos();
    let name = read_string(r)?;
    if !is_identifier(&name) {
        return Err(DecodeError { at: start, kind: DecodeErrorKind::BadName });
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_rtdt::ordered::{write_escaped, write_varint};

    #[test]
    fn varints_round_trip() {
        for v in [0, 1, 0xF7, 0xF8, 0xFF, 0x100, 0xFFFF, 0x1_0000, u32::MAX as u64, u64::MAX] {
            let mut bytes = Vec::new();
            write_varint(&mut bytes, v);
            let mut r = Reader::new(&bytes);
            assert_eq!(read_varint(&mut r).unwrap(), v);
            r.finish().unwrap();
        }
    }

    #[test]
    fn non_minimal_varints_are_rejected() {
        for bytes in [&[0xF8, 0x05][..], &[0xF9, 0x00, 0xFF], &[0xFF, 0, 0, 0, 0, 0, 0, 1, 0]] {
            let mut r = Reader::new(bytes);
            assert_eq!(read_varint(&mut r).unwrap_err().kind, DecodeErrorKind::NotMinimal);
        }
    }

    #[test]
    fn escaped_bytes_round_trip() {
        for run in [&b""[..], b"\0", b"\0\0", b"\0a", b"a", b"a\0"] {
            let mut bytes = Vec::new();
            write_escaped(&mut bytes, run);
            let mut r = Reader::new(&bytes);
            let mut back = Vec::new();
            read_escaped(&mut r, &mut back).unwrap();
            assert_eq!(back, run);
            r.finish().unwrap();
        }
    }
}
