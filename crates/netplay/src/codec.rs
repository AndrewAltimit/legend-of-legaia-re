//! The byte-level reader and writer under the message codec.
//!
//! Integers that are usually small (frame numbers, ids, lengths) are LEB128
//! varints; hashes and pad words are fixed-width little-endian. Every
//! variable-length field carries a cap, checked on decode before anything is
//! allocated, so a hostile length prefix cannot make a reader reserve memory.

use std::fmt;

/// Why a message failed to decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer ended inside a field.
    Truncated,
    /// A message tag this build does not know.
    UnknownTag(u8),
    /// A length field over its cap.
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    /// A varint longer than its type allows.
    BadVarint,
    /// A string field that is not UTF-8.
    BadUtf8,
    /// An enum discriminant or value outside its range.
    BadValue(&'static str),
    /// Bytes left over after a complete message.
    TrailingBytes(usize),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "message truncated"),
            Self::UnknownTag(t) => write!(f, "unknown message tag 0x{t:02X}"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field}: length {len} over the cap of {max}")
            }
            Self::BadVarint => write!(f, "malformed varint"),
            Self::BadUtf8 => write!(f, "string is not UTF-8"),
            Self::BadValue(what) => write!(f, "{what}: value out of range"),
            Self::TrailingBytes(n) => write!(f, "{n} trailing bytes after the message"),
        }
    }
}

impl std::error::Error for CodecError {}

/// Appends fields to a growing buffer.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64_fixed(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    pub fn bytes(&mut self, v: &[u8]) {
        self.varint(v.len() as u64);
        self.buf.extend_from_slice(v);
    }

    pub fn raw(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    pub fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
}

/// Reads fields off a borrowed buffer.
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Fails unless every byte has been consumed.
    pub fn finish(&self) -> Result<(), CodecError> {
        match self.buf.len() - self.pos {
            0 => Ok(()),
            n => Err(CodecError::TrailingBytes(n)),
        }
    }

    pub fn raw(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.pos.checked_add(n).ok_or(CodecError::Truncated)?;
        let out = self.buf.get(self.pos..end).ok_or(CodecError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.raw(1)?[0])
    }

    pub fn bool(&mut self) -> Result<bool, CodecError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::BadValue("bool")),
        }
    }

    pub fn u16(&mut self) -> Result<u16, CodecError> {
        let b = self.raw(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u64_fixed(&mut self) -> Result<u64, CodecError> {
        let b = self.raw(8)?;
        Ok(u64::from_le_bytes(b.try_into().expect("8 bytes")))
    }

    pub fn i32(&mut self) -> Result<i32, CodecError> {
        let b = self.raw(4)?;
        Ok(i32::from_le_bytes(b.try_into().expect("4 bytes")))
    }

    pub fn varint(&mut self) -> Result<u64, CodecError> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            let bits = u64::from(byte & 0x7F);
            if shift == 63 && bits > 1 {
                return Err(CodecError::BadVarint);
            }
            out |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(out);
            }
        }
        Err(CodecError::BadVarint)
    }

    pub fn varint_u32(&mut self) -> Result<u32, CodecError> {
        u32::try_from(self.varint()?).map_err(|_| CodecError::BadVarint)
    }

    /// A length prefix, checked against `max` before any allocation.
    pub fn len(&mut self, field: &'static str, max: usize) -> Result<usize, CodecError> {
        let len = self.varint()?;
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if len > max {
            return Err(CodecError::TooLong { field, len, max });
        }
        Ok(len)
    }

    pub fn bytes(&mut self, field: &'static str, max: usize) -> Result<Vec<u8>, CodecError> {
        let len = self.len(field, max)?;
        Ok(self.raw(len)?.to_vec())
    }

    pub fn string(&mut self, field: &'static str, max: usize) -> Result<String, CodecError> {
        String::from_utf8(self.bytes(field, max)?).map_err(|_| CodecError::BadUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips_edges() {
        for v in [0, 1, 127, 128, 16_383, 16_384, u32::MAX as u64, u64::MAX] {
            let mut w = Writer::new();
            w.varint(v);
            let bytes = w.into_bytes();
            let mut r = Reader::new(&bytes);
            assert_eq!(r.varint().unwrap(), v);
            r.finish().unwrap();
        }
    }

    #[test]
    fn overlong_varint_is_rejected() {
        let bytes = [0xFF; 11];
        assert_eq!(Reader::new(&bytes).varint(), Err(CodecError::BadVarint));
    }

    #[test]
    fn length_cap_is_checked_before_reading() {
        let mut w = Writer::new();
        w.varint(1 << 40);
        let bytes = w.into_bytes();
        let err = Reader::new(&bytes).bytes("payload", 16).unwrap_err();
        assert!(matches!(
            err,
            CodecError::TooLong {
                field: "payload",
                ..
            }
        ));
    }
}
