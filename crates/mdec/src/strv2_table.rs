//! STRv2/v3 VLC lookup-table unpacker - `FUN_801F1A00` in the STR/MDEC overlay
//! (PROT 0970, base `0x801CE818`).
//!
//! The play loop `FUN_801CF098` calls this once per FMV, unconditionally, right
//! after the ring/stream setup (`801cf210`: `FUN_801F1A00(0x801E0A00)`). It
//! expands a compressed byte stream that lives immediately after the function
//! itself (VA `0x801F1AE8`) into the direct-lookup VLC table the STRv2/v3
//! bitstream decoder `FUN_801D070C` indexes.
//!
//! The unpacked table is **exactly** [`STRV2_TABLE_U16S`] `u16` entries
//! (`0x8800` = 34816, `0x11000` bytes) written to `0x801E0A00`, so it ends at
//! `0x801F1A00` - flush against this function's own entry point. That exact
//! abutment is the geometric proof of the size: the second pass runs its `u16`
//! index from `4` to `0x87FF` inclusive (`ori a2,zero,0x87ff` bound at
//! `0x801F1AB8`, `slt`/`beqz` at `0x801F1AD4`), and the last byte it touches is
//! `0x801F19FE..0x801F19FF`.
//!
//! ## Pass 1 - mode-switched LZ77
//!
//! A byte-oriented LZ with a *sticky* match distance rather than a per-match
//! one:
//!
//! | Control byte | Meaning |
//! |---|---|
//! | `0x00..=0xEF` | emit `n + 1` bytes: literals in literal mode, back-references at the current distance in match mode |
//! | `0xF0` | switch to literal mode (distance := 0) |
//! | `0xF1..=0xFF` | read one more byte; distance := `((b << 8) \| next) - 0xF0FF`, switching to match mode |
//! | `0xFF 0xFF` | end of stream (the distance `0xF00` sentinel at `0x801F1AB0`) |
//!
//! The distance persists across control bytes, so a run of `< 0xF0` bytes after
//! a distance escape keeps copying from that same distance. Copies are
//! byte-at-a-time and may overlap (`801f1a48`: load, store, advance both).
//!
//! ## Pass 2 - XOR de-delta at a 4-entry stride
//!
//! `out[i] ^= out[i - 4]` for every `u16` index `i` in `4..=0x87FF`
//! (`lhu 0(a0)` / `lhu -8(a0)` at `0x801F1AC0`). The eight-byte lag is the
//! table's own record width, so the packed form stores each record as a delta
//! against its predecessor - which is what makes it compress at all.
//!
//! ## The decoder it feeds
//!
//! Every retail movie uses the **Iki** bitstream instead ([`crate::MdecDecoder`]),
//! because `FUN_801CEA3C` presets `DAT_801E09FC = 1` and clears it for dev slots
//! 9/10 alone, whose files are not on the released disc. The table this unpacker
//! produces is nonetheless the exact input of the STRv2/v3 decoder
//! [`crate::strv2_decode::decode_frame`] (`FUN_801D070C`), which carves it into
//! the DC and AC lookup regions described there.

use anyhow::{Result, bail};

/// VA the play loop unpacks the table to (`0x801E0A00`).
pub const STRV2_TABLE_VA: u32 = 0x801E_0A00;

/// VA of the packed source bytes - the word immediately after `FUN_801F1A00`
/// (`lui a3,0x801f` / `addiu a3,a3,0x1ae8` at `0x801F1A04`).
pub const STRV2_PACKED_VA: u32 = 0x801F_1AE8;

/// `u16` entries in the unpacked table (`0x8800`).
pub const STRV2_TABLE_U16S: usize = 0x8800;

/// Byte length of the unpacked table (`0x11000`).
pub const STRV2_TABLE_BYTES: usize = STRV2_TABLE_U16S * 2;

/// De-delta stride, in `u16` entries - the `-8($a0)` lag of pass 2.
pub const STRV2_DELTA_STRIDE: usize = 4;

/// Control byte that switches pass 1 back to literal mode.
const MODE_LITERAL: u8 = 0xF0;

/// Bias subtracted from a two-byte distance escape (`addu v1,v0,t0` with
/// `t0 = 0xFFFF0F01`, i.e. `-0xF0FF`).
const DISTANCE_BIAS: u32 = 0xF0FF;

/// Distance value that terminates pass 1 (`bne v1,t1` against `t1 = 0xF00`).
const DISTANCE_END: u32 = 0xF00;

/// Pass 1 only: expand the mode-switched LZ77 stream, without the pass-2 XOR
/// de-delta.
///
/// Stops on the `0xFF 0xFF` end escape. A truncated stream is an error rather
/// than a silent short read - a partial VLC table decodes to garbage that looks
/// like a bitstream bug rather than a table bug.
// PORT: FUN_801f1a00
pub fn unpack_lz(packed: &[u8]) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(STRV2_TABLE_BYTES);
    let mut src = packed.iter().copied();
    // `v1` - the sticky match distance. Zero selects literal mode.
    let mut distance: usize = 0;
    loop {
        let Some(ctl) = src.next() else {
            bail!("STRv2 table stream ended without the 0xFF 0xFF terminator");
        };
        if ctl < MODE_LITERAL {
            // `a1` counts down from the control byte through zero inclusive, so
            // the run length is `ctl + 1`.
            let run = ctl as usize + 1;
            if distance == 0 {
                for _ in 0..run {
                    let Some(b) = src.next() else {
                        bail!("STRv2 table stream ended mid literal run");
                    };
                    out.push(b);
                }
            } else {
                for _ in 0..run {
                    let Some(from) = out.len().checked_sub(distance) else {
                        bail!("STRv2 table back-reference reaches before the output start");
                    };
                    out.push(out[from]);
                }
            }
        } else if ctl == MODE_LITERAL {
            distance = 0;
        } else {
            let Some(lo) = src.next() else {
                bail!("STRv2 table stream ended mid distance escape");
            };
            let word = ((ctl as u32) << 8) | lo as u32;
            let d = word.wrapping_sub(DISTANCE_BIAS);
            if d == DISTANCE_END {
                return Ok(out);
            }
            distance = d as usize;
        }
    }
}

/// Apply pass 2 in place: `out[i] ^= out[i - 4]` over `u16` index `4..=0x87FF`.
///
/// Operates on the `u16` view of `bytes`, which must therefore hold at least
/// [`STRV2_TABLE_BYTES`].
// PORT: FUN_801f1a00
pub fn de_delta(bytes: &mut [u8]) -> Result<()> {
    if bytes.len() < STRV2_TABLE_BYTES {
        bail!(
            "STRv2 table is {} bytes, need at least {}",
            bytes.len(),
            STRV2_TABLE_BYTES
        );
    }
    for i in STRV2_DELTA_STRIDE..STRV2_TABLE_U16S {
        let cur = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        let prev_i = i - STRV2_DELTA_STRIDE;
        let prev = u16::from_le_bytes([bytes[prev_i * 2], bytes[prev_i * 2 + 1]]);
        bytes[i * 2..i * 2 + 2].copy_from_slice(&(cur ^ prev).to_le_bytes());
    }
    Ok(())
}

/// Both passes: expand `packed` and return the `0x8800`-entry `u16` table the
/// STRv2/v3 decoder indexes.
///
/// `packed` is the overlay bytes at [`STRV2_PACKED_VA`] (the tail of the
/// overlay image; everything from that VA onward is fine - the stream carries
/// its own terminator).
// PORT: FUN_801f1a00
pub fn unpack_strv2_vlc_table(packed: &[u8]) -> Result<Vec<u16>> {
    let mut bytes = unpack_lz(packed)?;
    if bytes.len() < STRV2_TABLE_BYTES {
        bail!(
            "STRv2 table unpacked to {} bytes, expected at least {}",
            bytes.len(),
            STRV2_TABLE_BYTES
        );
    }
    de_delta(&mut bytes)?;
    Ok(bytes[..STRV2_TABLE_BYTES]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect())
}

/// Slice the packed stream out of a raw STR/MDEC overlay image loaded at
/// `base_va`, then unpack it.
pub fn unpack_from_overlay(overlay: &[u8], base_va: u32) -> Result<Vec<u16>> {
    let Some(off) = STRV2_PACKED_VA.checked_sub(base_va).map(|o| o as usize) else {
        bail!("overlay base {base_va:#x} is above the packed-table VA");
    };
    let Some(packed) = overlay.get(off..) else {
        bail!(
            "overlay is {} bytes, packed table starts at {off:#x}",
            overlay.len()
        );
    };
    unpack_strv2_vlc_table(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a packed stream from an explicit op list, so the pass-1 grammar is
    /// exercised without any disc bytes.
    fn packed(ops: &[&[u8]]) -> Vec<u8> {
        let mut v: Vec<u8> = ops.concat();
        v.extend_from_slice(&[0xFF, 0xFF]);
        v
    }

    #[test]
    fn literal_runs_emit_control_plus_one_bytes() {
        // Control 0x02 in literal mode emits three literals.
        let out = unpack_lz(&packed(&[&[0x02, b'a', b'b', b'c']])).unwrap();
        assert_eq!(out, b"abc");
        // Control 0x00 emits exactly one.
        let out = unpack_lz(&packed(&[&[0x00, b'z']])).unwrap();
        assert_eq!(out, b"z");
    }

    #[test]
    fn distance_escape_switches_to_match_mode_and_is_sticky() {
        // Four literals, then distance 1, then two separate copy runs. The
        // distance persists across the second control byte - that stickiness is
        // the whole point of the mode design.
        let out = unpack_lz(&packed(&[
            &[0x03, b'w', b'x', b'y', b'z'],
            &[0xF1, 0x00], // distance = 0xF100 - 0xF0FF = 1
            &[0x01],       // copy 2 bytes at distance 1
            &[0x01],       // copy 2 more, same distance, no new escape
        ]))
        .unwrap();
        assert_eq!(out, b"wxyzzzzz");
    }

    #[test]
    fn overlapping_back_references_are_byte_at_a_time() {
        // Distance 2 over a 4-byte run reads bytes this same run just wrote.
        let out = unpack_lz(&packed(&[
            &[0x01, b'A', b'B'],
            &[0xF1, 0x01], // distance = 2
            &[0x03],       // 4 bytes
        ]))
        .unwrap();
        assert_eq!(out, b"ABABAB");
    }

    #[test]
    fn f0_returns_to_literal_mode() {
        let out = unpack_lz(&packed(&[
            &[0x00, b'Q'],
            &[0xF1, 0x00], // distance 1
            &[0x01],       // "QQQ"
            &[0xF0],       // back to literals
            &[0x01, b'r', b's'],
        ]))
        .unwrap();
        assert_eq!(out, b"QQQrs");
    }

    #[test]
    fn distance_bias_matches_the_escape_arithmetic() {
        // `0xF1 0x00` is the smallest distance (1) and `0xFE 0xFF` the largest
        // before the terminator.
        for (hi, lo, want) in [
            (0xF1u8, 0x00u8, 1u32),
            (0xF1, 0x0A, 11),
            (0xFE, 0xFF, 0xE00),
        ] {
            let word = ((hi as u32) << 8) | lo as u32;
            assert_eq!(word - DISTANCE_BIAS, want);
        }
        // The terminator is the one escape whose distance lands on 0xF00.
        assert_eq!(0xFFFFu32 - DISTANCE_BIAS, DISTANCE_END);
    }

    #[test]
    fn truncated_streams_are_errors_not_short_tables() {
        // No terminator at all.
        assert!(unpack_lz(&[0x00, b'a']).is_err());
        // Literal run runs off the end.
        assert!(unpack_lz(&[0x05, b'a']).is_err());
        // Distance escape missing its second byte.
        assert!(unpack_lz(&[0xF3]).is_err());
        // Back-reference before the start of the output.
        assert!(unpack_lz(&packed(&[&[0xF1, 0x00], &[0x00]])).is_err());
    }

    #[test]
    fn de_delta_is_self_inverse_at_the_four_entry_stride() {
        let mut bytes = vec![0u8; STRV2_TABLE_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let original = bytes.clone();
        de_delta(&mut bytes).unwrap();
        // XOR-delta chains forward, so undo it walking backwards.
        for i in (STRV2_DELTA_STRIDE..STRV2_TABLE_U16S).rev() {
            let cur = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
            let p = i - STRV2_DELTA_STRIDE;
            let prev = u16::from_le_bytes([bytes[p * 2], bytes[p * 2 + 1]]);
            bytes[i * 2..i * 2 + 2].copy_from_slice(&(cur ^ prev).to_le_bytes());
        }
        assert_eq!(bytes, original);
        // The first four entries are never touched.
        de_delta(&mut bytes).unwrap();
        assert_eq!(
            bytes[..STRV2_DELTA_STRIDE * 2],
            original[..STRV2_DELTA_STRIDE * 2]
        );
    }

    #[test]
    fn short_tables_are_rejected_by_both_stages() {
        let mut short = vec![0u8; STRV2_TABLE_BYTES - 2];
        assert!(de_delta(&mut short).is_err());
        // A well-formed but too-short LZ stream fails the length check.
        assert!(unpack_strv2_vlc_table(&packed(&[&[0x00, 0x00]])).is_err());
    }

    #[test]
    fn table_geometry_abuts_the_unpacker_entry_point() {
        // The table's last byte sits immediately below FUN_801F1A00 - the
        // layout fact that pins the 0x8800-entry size.
        assert_eq!(STRV2_TABLE_VA as usize + STRV2_TABLE_BYTES, 0x801F_1A00);
        // ...and the packed source starts just past the function's 232 bytes.
        assert_eq!(STRV2_PACKED_VA, 0x801F_1A00 + 0xE8);
    }
}
