//! Pure-Rust ports of the small leaf helpers the field VM dispatcher
//! ([`super::field`]) calls into, lifted out of the dispatcher arms so they
//! can be unit-tested in isolation.
//!
//! These three helpers are referenced from many of the still-Pending sub-ops
//! in `FUN_801DE840`'s `case 0x4C` cluster. They are pure arithmetic — no
//! globals, no overlay calls — so a clean-room port can match the original
//! byte-for-byte and the dispatcher arms can call into them directly without
//! a `FieldHost` round-trip.
//!
//! | Helper                  | Original          | Used by                      |
//! | ----------------------- | ----------------- | ---------------------------- |
//! | [`packet_length`]       | `FUN_8003CA38`    | `0x4C nE sub-1`, `0x49`      |
//! | [`party_flag_test`]     | `FUN_8003CE64`    | `0x4C nC sub-1/5/6`          |
//! | [`small_table_search`]  | `FUN_80042EE0`    | `0x4C nD sub-C/E`            |
//! | [`load_u16_le`]         | `FUN_8003CE9C`    | `0x4C nC sub-5/6`, `nD sub-1`|
//! | [`load_u24_le`]         | `FUN_8003CEB8`    | `0x4C nE sub-5` (XP add)     |
//! | [`load_u32_le`]         | `FUN_8003CED8`    | 32-bit immediates            |
//! | [`tile_center`]         | inline (multi-arm) | `0x4C nE sub-3/4`, MOVE_TO  |
//!
//! Provenance for each port lives next to the function in the doc comment.
//!
//! No bytes from `SCUS_942.54` live here; the algorithms are described by
//! their decompilation.

/// Length of one variable-length text/data packet, in bytes.
///
/// Ported from `FUN_8003CA38` (see `ghidra/scripts/funcs/8003ca38.txt`). The
/// in-game text encoding terminates a packet with any byte `<= 0x1E`. Bytes
/// `>= 0x1F` are normal payload; bytes whose top nibble is `0xC` are 2-byte
/// escape sequences (the second byte is consumed unconditionally).
///
/// The returned count does **not** include the terminator byte itself. So if
/// the input is `[0x40, 0x40, 0x00, ...]`, the result is `2`. The dispatcher
/// adds the terminator and the opcode prefix bytes separately when computing
/// PC delta — see `0x4C nE sub-1`.
///
/// `buf` is read until either:
/// - a terminator byte (`<= 0x1E`) is seen, or
/// - the slice is exhausted.
///
/// On exhaustion the function returns the consumed length, matching the
/// original's address-walks-off-end behaviour (which would read uninitialised
/// memory in retail). Callers that need bounded scanning should pre-validate
/// the buffer.
pub fn packet_length(buf: &[u8]) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        let b = buf[i];
        if b <= 0x1E {
            break;
        }
        if (b & 0xF0) == 0xC0 {
            // Escape pair — consume one extra byte and credit it to the count.
            i += 1;
            count += 1;
            if i >= buf.len() {
                break;
            }
        }
        count += 1;
        i += 1;
    }
    count
}

/// Test whether the `idx`-th bit of a packed bit array is set.
///
/// Ported from `FUN_8003CE64` (see `ghidra/scripts/funcs/8003ce64.txt`). The
/// original indexes a global byte array at `0x80085758` (256 bits = 32 bytes
/// = the field-VM's per-scene "trigger flag" bank). It returns `0xFF` when
/// the bit is set and `0` otherwise, mirroring the original's saturation
/// pattern — callers compare for equality, not bit-by-bit.
///
/// Bit ordering matches the original: bit 7 of `flags[idx >> 3]` is index 0;
/// bit 0 is index 7. (The mask is `0x80 >> (idx & 7)`.)
///
/// Out-of-range indices return `0` (as if the bit were unset). This differs
/// from the retail behaviour, which would read off the end of the byte array
/// and pull whatever happens to be at `0x80085758 + (idx >> 3)`. The
/// out-of-range guard is safe for engine purposes because callers have
/// already validated the upper bound by the time they reach this helper.
pub fn party_flag_test(idx: u32, flags: &[u8]) -> u8 {
    let byte_idx = (idx >> 3) as usize;
    let Some(&byte) = flags.get(byte_idx) else {
        return 0;
    };
    let mask = 0x80u8 >> (idx & 7);
    if byte & mask != 0 { 0xFF } else { 0 }
}

/// Sentinel value returned by [`small_table_search`] when the needle is not
/// in the table. Matches the original's `return 0x100` for "not found".
pub const SEARCH_NOT_FOUND: u32 = 0x100;

/// Search a stride-2 byte table for the first index containing `needle`.
///
/// Ported from `FUN_80042EE0` (see `ghidra/scripts/funcs/80042ee0.txt`). The
/// original searches a `short[]` table (treated as bytes via the low byte of
/// each short) at `0x80085958`, scanning indices `[lo, hi)` where `lo` is
/// `*(short *)(gp + 0x2d2)` and `hi` is `*(short *)(gp + 0x2d4)`.
///
/// The function returns the matching `i`-index (zero-based offset into the
/// scanned range, i.e., the original's `(short)iVar2 - 1`-after-increment).
/// On miss it returns [`SEARCH_NOT_FOUND`] (`0x100`).
///
/// `table` is the raw byte slice; the helper indexes `table[i * 2]` to
/// extract each entry's low byte, matching the original's `*(byte *)(idx *
/// 2 + base)` access pattern.
///
/// `lo` and `hi` are `i16` to match the gp-relative globals; negative values
/// or `lo >= hi` produce [`SEARCH_NOT_FOUND`] without scanning.
pub fn small_table_search(needle: u8, table: &[u8], lo: i16, hi: i16) -> u32 {
    if lo < 0 || hi < 0 || lo >= hi {
        return SEARCH_NOT_FOUND;
    }
    let lo_u = lo as u16 as usize;
    let hi_u = hi as u16 as usize;
    for i in lo_u..hi_u {
        let byte_idx = i * 2;
        match table.get(byte_idx) {
            Some(&b) if b == needle => return i as u32,
            _ => {}
        }
    }
    SEARCH_NOT_FOUND
}

/// Load a little-endian unsigned 16-bit value from the head of a byte buffer.
///
/// Ported from `FUN_8003CE9C` (see `ghidra/scripts/funcs/8003ce9c.txt`). The
/// original is a 2-instruction `lbu / lbu / sll / or / jr` sequence that the
/// PSX MIPS toolchain emits for unaligned 16-bit loads — the field VM stores
/// 16-bit operand fields as raw byte pairs, so most call sites pass a pointer
/// somewhere into the bytecode stream.
///
/// Returns the byte at `buf[0]` as the low 8 bits and `buf[1] << 8` as the
/// high 8 bits, exactly matching the original's `(b0 | (b1 << 8))` formula.
///
/// On a buffer shorter than 2 bytes this returns the partial value extending
/// missing bytes as zero — matching the `try_get`-style guard the dispatcher
/// arms wrap their operand reads in. Callers that need the strict 2-byte
/// invariant should pre-validate.
pub fn load_u16_le(buf: &[u8]) -> u16 {
    let lo = buf.first().copied().unwrap_or(0);
    let hi = buf.get(1).copied().unwrap_or(0);
    u16::from(lo) | (u16::from(hi) << 8)
}

/// Load a little-endian unsigned 24-bit value, returned in the low 24 bits of
/// a `u32`.
///
/// Ported from `FUN_8003CEB8` (see `ghidra/scripts/funcs/8003ceb8.txt`). The
/// original assembles `b0 | (b1 << 8) | (b2 << 16)`. Used by the field VM's
/// XP-add opcode (`0x4C nE sub-5`), where the 24-bit immediate is clamped to
/// `[0, 9999999]` after this helper extracts it.
///
/// The high byte of the returned `u32` is always zero — to sign-extend the
/// 24-bit value into an `i32`, callers should compute
/// `(value << 8) as i32 >> 8`.
pub fn load_u24_le(buf: &[u8]) -> u32 {
    let b0 = u32::from(buf.first().copied().unwrap_or(0));
    let b1 = u32::from(buf.get(1).copied().unwrap_or(0));
    let b2 = u32::from(buf.get(2).copied().unwrap_or(0));
    b0 | (b1 << 8) | (b2 << 16)
}

/// Load a little-endian unsigned 32-bit value from the head of a byte buffer.
///
/// Ported from `FUN_8003CED8` (see `ghidra/scripts/funcs/8003ced8.txt`).
/// Companion to [`load_u16_le`] / [`load_u24_le`]; the original assembles four
/// 8-bit reads into a single 32-bit immediate. Used rarely by the field VM
/// (most operands are 16-bit) but the helper is small enough to bundle here
/// alongside the rest of the LE-load family.
pub fn load_u32_le(buf: &[u8]) -> u32 {
    let b0 = u32::from(buf.first().copied().unwrap_or(0));
    let b1 = u32::from(buf.get(1).copied().unwrap_or(0));
    let b2 = u32::from(buf.get(2).copied().unwrap_or(0));
    let b3 = u32::from(buf.get(3).copied().unwrap_or(0));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

/// Sign-extend a 24-bit value (in the low 24 bits of a `u32`) to an `i32`.
///
/// Helper for callers of [`load_u24_le`] that need a signed value (e.g. the
/// XP-add opcode, where the 24-bit immediate can be negative). The high byte
/// of `value` is ignored.
pub fn sign_extend_24(value: u32) -> i32 {
    ((value << 8) as i32) >> 8
}

/// Decode a grid-coordinate byte to a tile-center world coordinate (signed).
///
/// The field VM stores per-tile positions as packed bytes: the low 7 bits are
/// the tile index along one axis, and the high bit is a "+0x40 fine offset"
/// half-tile flag. The original dispatcher inlines this conversion in nine
/// distinct sub-ops (most prominently `0x4C nE sub-3/4` for camera-anchored
/// teleport / bbox queries, and the `MOVE_TO` family at op 0x23 / 0x3F).
///
/// Formula:
/// - `b == 0` returns `0` (the origin tile is treated as not-yet-positioned;
///   the original's `lb / beq / move zero` chain at `0x801E2810` short-circuits
///   the multiplication).
/// - Otherwise `(b & 0x7F) << 7 | 0x40` (= `(b & 0x7F) * 0x80 + 0x40` =
///   tile-grid origin + half-tile center).
/// - If `b & 0x80` is set, add another `0x40` (= the fine-offset bit pushes
///   the position to the next half-tile boundary).
///
/// The signed return matches the dispatcher's local-variable type — bbox /
/// camera coordinates are stored as `short` and can be compared with negative
/// reference values produced by tween / scroll operations. Callers that need an
/// unsigned value (e.g. world-space `ctx.world_x` writes for MOVE_TO) cast back
/// via `value as u16`.
///
/// The output range is `[0, 0x4000]` (max input `0xFF` = (`0x7F << 7`) +
/// `0x40` + `0x40` = `0x4000`). The signed return type never goes negative
/// for valid input.
pub fn tile_center(b: u8) -> i16 {
    if b == 0 {
        return 0;
    }
    let base = (i16::from(b & 0x7F)) << 7 | 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_length_empty_buffer_is_zero() {
        assert_eq!(packet_length(&[]), 0);
    }

    #[test]
    fn packet_length_immediate_terminator_is_zero() {
        // Any byte <= 0x1E ends the packet without contributing.
        for b in 0..=0x1Eu8 {
            assert_eq!(packet_length(&[b]), 0, "terminator byte {b:#x}");
        }
    }

    #[test]
    fn packet_length_pure_printable_run() {
        // Bytes 0x1F..=0xBF (excluding the 0xC0 escape range) are 1-byte each.
        let buf = [0x20, 0x40, 0x80, 0xBF, 0x00];
        assert_eq!(packet_length(&buf), 4);
    }

    #[test]
    fn packet_length_escape_sequence_counts_two() {
        // 0xC1 is an escape lead — the next byte is consumed unconditionally,
        // and BOTH count toward the length.
        let buf = [0xC1, 0xAB, 0x00];
        assert_eq!(packet_length(&buf), 2);
    }

    #[test]
    fn packet_length_multiple_escapes_with_runs() {
        // 0x40 (1) + 0xC1 0xAB (2) + 0x40 (1) + 0xCD 0x05 (2) + terminator.
        // Note: the 0x05 inside the escape pair is consumed unconditionally
        // even though it's <= 0x1E — it's already past the terminator check
        // when the lead byte's escape semantics fire.
        let buf = [0x40, 0xC1, 0xAB, 0x40, 0xCD, 0x05, 0x00];
        assert_eq!(packet_length(&buf), 6);
    }

    #[test]
    fn packet_length_escape_at_buffer_boundary() {
        // 0xC2 with no following byte. The original loops would read off
        // the end; we guard and stop. The lead byte still counts.
        let buf = [0xC2];
        assert_eq!(packet_length(&buf), 1);
    }

    #[test]
    fn packet_length_no_terminator_runs_to_end() {
        let buf = [0x20, 0x21, 0x22, 0x23];
        assert_eq!(packet_length(&buf), 4);
    }

    #[test]
    fn packet_length_high_nibble_matters_for_escape() {
        // 0xCx is escape; 0xDx is plain.
        assert_eq!(packet_length(&[0xC0, 0xFF, 0x00]), 2);
        assert_eq!(packet_length(&[0xD0, 0xFF, 0x00]), 2);
        // 0xC0..=0xCF all consume the next byte:
        for lead in 0xC0..=0xCFu8 {
            assert_eq!(
                packet_length(&[lead, 0xAB, 0x00]),
                2,
                "escape lead {lead:#x}"
            );
        }
        // 0xB0..=0xBF and 0xD0..=0xDF do NOT (only 0xC range escapes):
        for lead in [0xB0u8, 0xBFu8, 0xD0u8, 0xDFu8] {
            assert_eq!(packet_length(&[lead, 0x00]), 1, "non-escape lead {lead:#x}");
        }
    }

    #[test]
    fn party_flag_test_returns_ff_when_bit_set() {
        // Bit 7 of flags[0] is index 0.
        let flags = [0x80, 0x00, 0x00, 0x00];
        assert_eq!(party_flag_test(0, &flags), 0xFF);
        // Bit 0 of flags[0] is index 7.
        let flags = [0x01, 0x00, 0x00, 0x00];
        assert_eq!(party_flag_test(7, &flags), 0xFF);
        // Bit 7 of flags[1] is index 8.
        let flags = [0x00, 0x80, 0x00, 0x00];
        assert_eq!(party_flag_test(8, &flags), 0xFF);
    }

    #[test]
    fn party_flag_test_returns_zero_when_bit_clear() {
        let flags = [0xFE, 0x00, 0x00, 0x00];
        assert_eq!(party_flag_test(7, &flags), 0); // only bit 7 (idx 0) is set
        // Verify all the other bits in byte 0 ARE set:
        for idx in 0..7 {
            assert_eq!(party_flag_test(idx, &flags), 0xFF);
        }
    }

    #[test]
    fn party_flag_test_out_of_range_returns_zero() {
        let flags = [0xFF, 0xFF];
        // Bit 16 is at flags[2] which is out of range.
        assert_eq!(party_flag_test(16, &flags), 0);
        assert_eq!(party_flag_test(255, &flags), 0);
    }

    #[test]
    fn party_flag_test_full_byte_round_trip() {
        // For each bit position 0..8, set only that bit and verify exactly
        // one index returns 0xFF.
        for bit in 0..8u32 {
            let mut flags = [0u8; 1];
            flags[0] = 0x80u8 >> bit;
            for idx in 0..8u32 {
                let expected = if idx == bit { 0xFF } else { 0 };
                assert_eq!(
                    party_flag_test(idx, &flags),
                    expected,
                    "set bit {bit}, query {idx}"
                );
            }
        }
    }

    #[test]
    fn small_table_search_finds_first_match() {
        // table[0..6] = [0xAA, 0, 0xBB, 0, 0xCC, 0] (stride 2, low bytes only)
        let table = [0xAA, 0, 0xBB, 0, 0xCC, 0];
        assert_eq!(small_table_search(0xAA, &table, 0, 3), 0);
        assert_eq!(small_table_search(0xBB, &table, 0, 3), 1);
        assert_eq!(small_table_search(0xCC, &table, 0, 3), 2);
    }

    #[test]
    fn small_table_search_miss_returns_sentinel() {
        let table = [0xAA, 0, 0xBB, 0];
        assert_eq!(small_table_search(0xFF, &table, 0, 2), SEARCH_NOT_FOUND);
    }

    #[test]
    fn small_table_search_respects_lo_hi_window() {
        let table = [0xAA, 0, 0xBB, 0, 0xCC, 0, 0xDD, 0];
        // Searching only [2..4) hides indices 0 and 1.
        assert_eq!(small_table_search(0xAA, &table, 2, 4), SEARCH_NOT_FOUND);
        assert_eq!(small_table_search(0xCC, &table, 2, 4), 2);
        // [0..2) hides indices 2 and 3.
        assert_eq!(small_table_search(0xCC, &table, 0, 2), SEARCH_NOT_FOUND);
    }

    #[test]
    fn small_table_search_empty_window_returns_sentinel() {
        let table = [0xAA, 0];
        assert_eq!(small_table_search(0xAA, &table, 0, 0), SEARCH_NOT_FOUND);
        assert_eq!(small_table_search(0xAA, &table, 1, 1), SEARCH_NOT_FOUND);
    }

    #[test]
    fn small_table_search_negative_bounds_return_sentinel() {
        let table = [0xAA, 0];
        assert_eq!(small_table_search(0xAA, &table, -1, 1), SEARCH_NOT_FOUND);
        assert_eq!(small_table_search(0xAA, &table, 0, -1), SEARCH_NOT_FOUND);
    }

    #[test]
    fn small_table_search_out_of_range_index_skipped() {
        // Window [0..4) but table only has 2 entries — the missing reads
        // are skipped (treated as misses) without panicking.
        let table = [0xAA, 0];
        assert_eq!(small_table_search(0xAA, &table, 0, 4), 0);
        assert_eq!(small_table_search(0xBB, &table, 0, 4), SEARCH_NOT_FOUND);
    }

    #[test]
    fn small_table_search_lo_inclusive_hi_exclusive() {
        // Verify the range semantics — lo is included, hi is excluded.
        let table = [0xAA, 0, 0xBB, 0, 0xCC, 0];
        assert_eq!(small_table_search(0xAA, &table, 0, 1), 0);
        assert_eq!(small_table_search(0xBB, &table, 0, 1), SEARCH_NOT_FOUND);
        assert_eq!(small_table_search(0xBB, &table, 0, 2), 1);
        assert_eq!(small_table_search(0xCC, &table, 2, 3), 2);
    }

    #[test]
    fn load_u16_le_assembles_low_then_high_byte() {
        assert_eq!(load_u16_le(&[0x34, 0x12]), 0x1234);
        assert_eq!(load_u16_le(&[0xFF, 0xFF]), 0xFFFF);
        assert_eq!(load_u16_le(&[0x00, 0x80]), 0x8000);
    }

    #[test]
    fn load_u16_le_short_buffer_zero_extends() {
        assert_eq!(load_u16_le(&[]), 0);
        assert_eq!(load_u16_le(&[0xAB]), 0x00AB);
    }

    #[test]
    fn load_u16_le_ignores_trailing_bytes() {
        // Helper reads exactly 2 bytes; trailing bytes never contribute.
        assert_eq!(load_u16_le(&[0x34, 0x12, 0xFF, 0xFF]), 0x1234);
    }

    #[test]
    fn load_u24_le_assembles_three_bytes() {
        assert_eq!(load_u24_le(&[0x56, 0x34, 0x12]), 0x123456);
        assert_eq!(load_u24_le(&[0xFF, 0xFF, 0xFF]), 0x00FF_FFFF);
        assert_eq!(load_u24_le(&[0x00, 0x00, 0x80]), 0x0080_0000);
    }

    #[test]
    fn load_u24_le_short_buffer_zero_extends() {
        assert_eq!(load_u24_le(&[]), 0);
        assert_eq!(load_u24_le(&[0xAA]), 0x0000_00AA);
        assert_eq!(load_u24_le(&[0xAA, 0xBB]), 0x0000_BBAA);
    }

    #[test]
    fn load_u24_le_high_byte_is_always_zero() {
        // 24-bit value never sets bits 24..32.
        assert_eq!(load_u24_le(&[0xFF, 0xFF, 0xFF]) & 0xFF00_0000, 0);
    }

    #[test]
    fn load_u32_le_assembles_four_bytes() {
        assert_eq!(load_u32_le(&[0x78, 0x56, 0x34, 0x12]), 0x1234_5678);
        assert_eq!(load_u32_le(&[0xFF, 0xFF, 0xFF, 0xFF]), 0xFFFF_FFFF);
        assert_eq!(load_u32_le(&[0x00, 0x00, 0x00, 0x80]), 0x8000_0000);
    }

    #[test]
    fn load_u32_le_short_buffer_zero_extends() {
        assert_eq!(load_u32_le(&[]), 0);
        assert_eq!(load_u32_le(&[0x11]), 0x0000_0011);
        assert_eq!(load_u32_le(&[0x11, 0x22]), 0x0000_2211);
        assert_eq!(load_u32_le(&[0x11, 0x22, 0x33]), 0x0033_2211);
    }

    #[test]
    fn sign_extend_24_preserves_positive_values() {
        assert_eq!(sign_extend_24(0), 0);
        assert_eq!(sign_extend_24(1), 1);
        assert_eq!(sign_extend_24(0x7F_FFFF), 0x7F_FFFF);
    }

    #[test]
    fn sign_extend_24_extends_negative_values() {
        // 0x80_0000 is the most-negative 24-bit value: -8388608.
        assert_eq!(sign_extend_24(0x80_0000), -0x80_0000);
        // 0xFF_FFFF is -1 in 24-bit two's complement.
        assert_eq!(sign_extend_24(0xFF_FFFF), -1);
        // 0xFF_FFFE is -2.
        assert_eq!(sign_extend_24(0xFF_FFFE), -2);
    }

    #[test]
    fn sign_extend_24_ignores_high_byte() {
        // The function only looks at the low 24 bits.
        assert_eq!(sign_extend_24(0xABFF_FFFF), -1);
        assert_eq!(sign_extend_24(0xCD00_0000), 0);
    }

    #[test]
    fn tile_center_zero_is_zero() {
        // The dispatcher short-circuits b == 0 to 0 — verifies the special
        // case isn't lost in arithmetic.
        assert_eq!(tile_center(0), 0);
    }

    #[test]
    fn tile_center_low_byte_examples() {
        // Verified against the original's `(b & 0x7F) << 7 | 0x40` formula.
        assert_eq!(tile_center(0x01), 0xC0); // 1 * 0x80 + 0x40
        assert_eq!(tile_center(0x02), 0x140); // 2 * 0x80 + 0x40
        assert_eq!(tile_center(0x10), 0x840); // 16 * 0x80 + 0x40
        assert_eq!(tile_center(0x7F), 0x3FC0); // 127 * 0x80 + 0x40
    }

    #[test]
    fn tile_center_high_bit_adds_fine_offset() {
        // b & 0x80 set adds another 0x40.
        assert_eq!(tile_center(0x90), 0x880); // 16 * 0x80 + 0x40 + 0x40
        assert_eq!(tile_center(0xFF), 0x4000); // 127 * 0x80 + 0x40 + 0x40 = 0x3FC0 + 0x40
        assert_eq!(tile_center(0x81), 0x100); // 1 * 0x80 + 0x40 + 0x40
    }

    #[test]
    fn tile_center_high_bit_only_does_not_zero_out() {
        // 0x80 has the fine-offset bit set but tile index 0. The original
        // does NOT short-circuit for this — only b == 0 zeroes out.
        assert_eq!(tile_center(0x80), 0x80); // 0 * 0x80 + 0x40 + 0x40
    }

    #[test]
    fn tile_center_output_is_non_negative() {
        // For all valid inputs the output is non-negative (max ~0x40C0).
        for b in 0..=u8::MAX {
            assert!(tile_center(b) >= 0, "byte {b:#x} produced negative output");
        }
    }

    #[test]
    fn load_helpers_form_consistent_family() {
        // u16 of "abc" prefix matches u24 of "abc" masked to 16 bits, etc.
        let buf = [0x11, 0x22, 0x33, 0x44];
        let u16v = u32::from(load_u16_le(&buf));
        let u24v = load_u24_le(&buf);
        let u32v = load_u32_le(&buf);
        assert_eq!(u24v & 0xFFFF, u16v);
        assert_eq!(u32v & 0xFF_FFFF, u24v);
        assert_eq!(u32v & 0xFFFF, u16v);
    }
}
