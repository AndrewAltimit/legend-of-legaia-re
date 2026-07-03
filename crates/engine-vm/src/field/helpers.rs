//! Field VM standalone bytecode helpers (coordinate decode, relative jumps,
//! MES-shape walker, extended-opcode peek). Split out of `field.rs`.

/// Inspect the byte at `pc`. If the opcode there has the extended bit (0x80)
/// set, returns the target script ID byte that follows. Otherwise returns
/// `None`. Use this before calling [`step`] to know which `FieldCtx` to pass.
///
/// The system channel script ID is `0xFB`. Unknown IDs match the original's
/// `"UNFIND INDICATION %d"` diagnostic - the caller decides how to recover
/// (the original returns `pc + 1`).
pub fn peek_extended(bytecode: &[u8], pc: usize) -> Option<u8> {
    let op = *bytecode.get(pc)?;
    if op & 0x80 != 0 {
        bytecode.get(pc + 1).copied()
    } else {
        None
    }
}

/// Decode a grid-coordinate byte to a world coordinate.
///
/// Formula: `(b & 0x7F) * 0x80 + 0x40`, plus `0x40` if the high bit is set.
/// Used by ops 0x23 (`MOVE_TO`) and 0x3F (`DIALOG`) for the position bytes.
pub(super) fn grid_to_world(b: u8) -> u16 {
    let base = u16::from(b & 0x7F) * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// Relative-jump target, faithful to retail's 16-bit `short` PC.
///
/// The original stores each script's PC as a signed 16-bit value at
/// `ctx[+0x9e]`, so every relative branch target wraps mod `0x10000`. A delta
/// with the high bit set is therefore a *backward* jump (e.g. `0xFFFE` = -2),
/// not a `+65534` forward one. Computing `base + delta` in `usize` (no wrap)
/// sends every backward jump off the end of the buffer (the classic
/// "PC runs away to 0x10102" symptom). `base` is the post-operand address the
/// delta is measured from.
pub(super) fn rel_jump(base: usize, lo: u8, hi: u8) -> usize {
    let delta = u16::from_le_bytes([lo, hi]);
    usize::from((base as u16).wrapping_add(delta))
}

/// MES-shape bytecode walker. Mirrors `FUN_8003ca38`: counts payload bytes
/// starting at `buf[0]` until a terminator (`≤ 0x1E`), with a one-byte
/// peek-extension for `0xCx` prefix bytes (each consumes its trailing pair
/// byte). Used by op 0x49 sub-0 in the `Done` arm to advance past an inline
/// MES payload.
///
/// Returns the number of bytes walked. The walker stops at the first
/// terminator or at end-of-slice (defensive: the original reads past EOF
/// without bounds checks).
pub(super) fn walk_mes_bytecode(buf: &[u8]) -> usize {
    let mut i = 0;
    while let Some(&b) = buf.get(i) {
        if b <= 0x1E {
            break;
        }
        if b & 0xF0 == 0xC0 {
            if buf.get(i + 1).is_none() {
                i += 1;
                break;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}
