//! The `OTHER3` dev module's selection roster (PROT `0974`).
//!
//! PROT `0974` is the slot-A dev module the mode-24 door-warp reaches as
//! sub-id 2 (`other3_dev` in
//! [`static-overlays.toml`](../../data/static-overlays.toml)). Its head string
//! pool frames a list screen - a module banner, a `SELECT NO %d DEPTH %d`
//! cursor readout, a `vol %d` readout and a `read ret %d` CD-return readout -
//! and nearly three quarters of the image is the list it pages through.
//!
//! ## What the code states
//!
//! The roster walk is one loop inside `FUN_801CED68`
//! (`see ghidra/scripts/funcs/overlay_other3_dev_0974_801ced68.txt`), and every
//! constant below is one of its operands rather than a shape read off the
//! bytes:
//!
//! ```text
//! 801cedfc  lui   v0, 0x801d
//! 801cee00  addiu s3, v0, -0x10c0     <- table base 0x801CEF40
//! 801cee08  sll   a0, s0, 5
//! 801cee0c  addu  a0, a0, s0
//! 801cee10  sll   a0, a0, 2           <- index * (32 + 1) * 4 = index * 0x84
//! 801cee14  addu  a0, a0, s3
//! 801cee20  jal   0x80036888          <- the record IS the string argument
//! 801cee2c  lui   v0, 0xca45          }
//! 801cee30  ori   v0, v0, 0x87e7      } reciprocal divide, shift 6
//! 801cee4c  sll   v0, v1, 4           }
//! 801cee50  addu  v0, v0, v1          } v1 * 81
//! 801cee54  subu  s0, s0, v0          <- index mod 81: the entry count
//! 801cee58  slti  v0, s2, 0xa         <- ten rows drawn per page
//! ```
//!
//! So a record is `0x84` bytes, there are `81` of them, and the record's own
//! first byte is where the text starts: the loop hands `table + i * 0x84`
//! straight to the SCUS text actor with no field offset. Each label is padded
//! with NULs to the stride, which is why the whole region reads as one
//! `ascii_text` residue run to a shape test while being a fixed-stride table.
//!
//! The labels are Japanese - Shift-JIS code units (retail uses the fullwidth
//! block `0x81` / `0x82`, katakana `0x83` and kanji at `0x88` / `0x8F`) stored
//! **low byte first**, i.e. as little-endian `u16`s rather than in Shift-JIS
//! byte order. The dev
//! modules were never localised; the rest of `0974`'s SCUS-range `jal` targets
//! land on `SCUS_942.54` function heads, so this is a USA-build image carrying
//! Japanese dev text, not a foreign-build image like PROT `0896`.
//!
//! This module exists so byte accounting can claim the table structurally. It
//! hands back raw record slices and decodes nothing: the encoding claim above
//! is about byte order, and a decoder would put game text in this workspace's
//! output for no consumer that wants it.

/// Load base of the `OTHER3` dev module.
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// PROT extraction index of the module.
pub const OVERLAY_PROT_INDEX: u32 = 974;
/// Roster base, from `801cee00 addiu s3, v0, -0x10c0`.
pub const ROSTER_VA: u32 = 0x801C_EF40;
/// Record stride, from the `(i << 5) + i` then `<< 2` index arithmetic.
pub const RECORD_STRIDE: usize = 0x84;
/// Record count, from the `mod 81` reciprocal divide that wraps the cursor.
pub const RECORD_COUNT: usize = 81;
/// Rows the roster draws per page (`slti v0, s2, 0xa`).
pub const ROWS_PER_PAGE: usize = 10;

/// File offset of the roster inside PROT `0974`.
pub const ROSTER_FILE_OFFSET: usize = (ROSTER_VA - OVERLAY_BASE_VA) as usize;
/// Byte length of the whole roster.
pub const ROSTER_BYTES: usize = RECORD_COUNT * RECORD_STRIDE;

/// The roster's records as raw `RECORD_STRIDE`-byte slices, or `None` when
/// `entry` is not a whole PROT `0974` image.
///
/// The guard is structural rather than a magic check: every record must start
/// either with a NUL (an empty slot) or with a code unit whose high byte is a
/// Shift-JIS lead (`0x81..=0x9F` or `0xE0..=0xEF`). Retail uses four of those
/// leads - the fullwidth block `0x81` / `0x82`, katakana `0x83`, and kanji at
/// `0x88` / `0x8F` - so a test narrowed to the fullwidth pair refuses two real
/// records. A buffer that is not this image fails on the first one.
pub fn records(entry: &[u8]) -> Option<Vec<&[u8]>> {
    let table = entry.get(ROSTER_FILE_OFFSET..ROSTER_FILE_OFFSET + ROSTER_BYTES)?;
    let mut out = Vec::with_capacity(RECORD_COUNT);
    for i in 0..RECORD_COUNT {
        let rec = &table[i * RECORD_STRIDE..(i + 1) * RECORD_STRIDE];
        let lead_ok = rec[0] == 0 || matches!(rec[1], 0x81..=0x9F | 0xE0..=0xEF);
        if !lead_ok {
            return None;
        }
        out.push(rec);
    }
    Some(out)
}

/// `(file_offset, len)` of record `i`.
pub fn record_extent(i: usize) -> Option<(usize, usize)> {
    (i < RECORD_COUNT).then(|| (ROSTER_FILE_OFFSET + i * RECORD_STRIDE, RECORD_STRIDE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_bounded_by_its_own_count_and_stride() {
        assert_eq!(ROSTER_BYTES, 81 * 132);
        assert_eq!(record_extent(0), Some((ROSTER_FILE_OFFSET, RECORD_STRIDE)));
        assert_eq!(record_extent(RECORD_COUNT), None);
    }

    #[test]
    fn a_buffer_that_is_not_the_image_is_refused() {
        assert!(records(&[0u8; 16]).is_none());
        // Right size, wrong content: a record whose second byte is not a
        // Shift-JIS lead byte and whose first is not NUL.
        let mut buf = vec![0u8; ROSTER_FILE_OFFSET + ROSTER_BYTES];
        buf[ROSTER_FILE_OFFSET] = 0x41;
        buf[ROSTER_FILE_OFFSET + 1] = 0x41;
        assert!(records(&buf).is_none());
    }
}
