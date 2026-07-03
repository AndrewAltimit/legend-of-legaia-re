//! Checked little-endian byte readers shared across the Legaia parser crates.
//!
//! Every format parser in the workspace reads unaligned little-endian scalars
//! out of raw disc buffers. Historically each crate (and often each module)
//! carried its own private `read_u32_le` / `read_u16_le` helper of the shape
//! `buf.get(at..at + N)?.try_into().unwrap()`. This crate hoists that one
//! idiom into a single leaf dependency so the bounds check travels with the
//! read and the `.try_into().unwrap()` array-copy lives in exactly one place.
//!
//! # Semantics
//!
//! All readers are **checked**: they return [`None`] when the requested window
//! falls outside `buf`, matching the dominant `.get(range)?` convention the
//! callers already relied on. None of them panic on a short or malformed
//! buffer, so they are safe to point at untrusted disc bytes.
//!
//! Signed and 24-bit variants are provided because the on-disc formats use
//! them (signed vertex deltas, packed 24-bit RGB / offsets). The 24-bit
//! readers widen into the next larger native type (`u32` / `i32`).
//!
//! ```
//! use legaia_bytes::{u16_le, u32_le};
//! let buf = [0x01, 0x02, 0x03, 0x04];
//! assert_eq!(u16_le(&buf, 0), Some(0x0201));
//! assert_eq!(u32_le(&buf, 0), Some(0x0403_0201));
//! assert_eq!(u32_le(&buf, 1), None); // out of range
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Reads the single byte at `at`, or [`None`] if `at` is past the end.
#[inline]
pub fn u8_at(buf: &[u8], at: usize) -> Option<u8> {
    buf.get(at).copied()
}

/// Reads a little-endian `u16` at `at`, or [`None`] if the 2-byte window
/// falls outside `buf`.
#[inline]
pub fn u16_le(buf: &[u8], at: usize) -> Option<u16> {
    let b = buf.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// Reads a little-endian `i16` at `at`, or [`None`] if the 2-byte window
/// falls outside `buf`.
#[inline]
pub fn i16_le(buf: &[u8], at: usize) -> Option<i16> {
    let b = buf.get(at..at + 2)?;
    Some(i16::from_le_bytes([b[0], b[1]]))
}

/// Reads a little-endian unsigned 24-bit value at `at` (widened to `u32`), or
/// [`None`] if the 3-byte window falls outside `buf`.
#[inline]
pub fn u24_le(buf: &[u8], at: usize) -> Option<u32> {
    let b = buf.get(at..at + 3)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], 0]))
}

/// Reads a little-endian `u32` at `at`, or [`None`] if the 4-byte window
/// falls outside `buf`.
#[inline]
pub fn u32_le(buf: &[u8], at: usize) -> Option<u32> {
    let b = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Reads a little-endian `i32` at `at`, or [`None`] if the 4-byte window
/// falls outside `buf`.
#[inline]
pub fn i32_le(buf: &[u8], at: usize) -> Option<i32> {
    let b = buf.get(at..at + 4)?;
    Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUF: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

    #[test]
    fn reads_match_manual_from_le_bytes() {
        assert_eq!(u8_at(&BUF, 2), Some(0x33));
        assert_eq!(u16_le(&BUF, 0), Some(u16::from_le_bytes([0x11, 0x22])));
        assert_eq!(i16_le(&BUF, 0), Some(i16::from_le_bytes([0x11, 0x22])));
        assert_eq!(u24_le(&BUF, 0), Some(0x0033_2211));
        assert_eq!(
            u32_le(&BUF, 0),
            Some(u32::from_le_bytes([0x11, 0x22, 0x33, 0x44]))
        );
        assert_eq!(
            i32_le(&BUF, 0),
            Some(i32::from_le_bytes([0x11, 0x22, 0x33, 0x44]))
        );
    }

    #[test]
    fn out_of_range_returns_none() {
        assert_eq!(u8_at(&BUF, 6), None);
        assert_eq!(u16_le(&BUF, 5), None);
        assert_eq!(i16_le(&BUF, 5), None);
        assert_eq!(u24_le(&BUF, 4), None);
        assert_eq!(u32_le(&BUF, 3), None);
        assert_eq!(i32_le(&BUF, 3), None);
    }

    #[test]
    fn signed_values_sign_extend() {
        let b = [0x00u8, 0x80, 0xff, 0xff];
        assert_eq!(i16_le(&b, 0), Some(-32768));
        assert_eq!(
            i32_le(&b, 0),
            Some(i32::from_le_bytes([0x00, 0x80, 0xff, 0xff]))
        );
    }
}
