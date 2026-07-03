/// Length of one variable-length text/data packet, in bytes.
///
/// Ported from `FUN_8003CA38` (see `ghidra/scripts/funcs/8003ca38.txt`). The
/// in-game text encoding terminates a packet with any byte `<= 0x1E`. Bytes
/// `>= 0x1F` are normal payload; bytes whose top nibble is `0xC` are 2-byte
/// escape sequences (the second byte is consumed unconditionally).
///
/// The returned count does **not** include the terminator byte itself, so the
/// input `[0x40, 0x40, 0x00, ...]` yields `2`. On exhaustion it returns the
/// consumed length (matching the original's walk-off-end behaviour). It is the
/// dialog/text packet-width helper the disassembler uses for the `0x4C nE`
/// text-balloon op; the executing field VM re-exports it from here.
pub fn packet_length(buf: &[u8]) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        let b = buf[i];
        if b <= 0x1E {
            break;
        }
        if (b & 0xF0) == 0xC0 {
            // Escape pair - consume one extra byte and credit it to the count.
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

/// Locate the `nth` occurrence of `delimiter` in a glyph string, returning
/// the byte offset reached when it is found.
///
/// Ported from `FUN_8003CBF8` (see `ghidra/scripts/funcs/8003cbf8.txt`), the
/// delimiter-counting sibling of [`packet_length`]: the walk uses the same
/// two-byte `0xC0`-nibble escape stride (the escape's second byte is skipped
/// without being tested as a delimiter, but still counts toward the offset),
/// and the string terminates on a NUL. `nth` is 1-based, matching the retail
/// down-counter. Returns `None` when fewer than `nth` delimiters occur (the
/// original returns `0` and latches a debug error code when the dev flag is
/// set; the error path has no engine meaning).
// PORT: FUN_8003CBF8
pub fn delimited_field_offset(buf: &[u8], delimiter: u8, nth: u32) -> Option<usize> {
    if nth == 0 {
        // Retail would underflow its down-counter and walk to the
        // terminator; never a match.
        return None;
    }
    let mut remaining = nth;
    let mut off = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        let b = buf[i];
        if b == 0 {
            break;
        }
        if b == delimiter {
            remaining -= 1;
            if remaining == 0 {
                return Some(off);
            }
        }
        if (b & 0xF0) == 0xC0 {
            // Escape pair - skip the operand byte (never delimiter-tested).
            i += 1;
            off += 1;
        }
        i += 1;
        off += 1;
    }
    None
}
