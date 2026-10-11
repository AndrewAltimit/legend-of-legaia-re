//! Append an escape token to a MES-markup string: the terminator walk and
//! the `{tag, arg}` splice the death-spoils captions are built with.
//!
//! Port provenance (disassembly, not the decompiled C):
//! `see ghidra/scripts/funcs/8003cb54.txt`.
//!
//! REF: FUN_8004AD80 (the one dumped caller of `FUN_8003CB54`)
//! REF: FUN_8003CA78 (its sibling: the marked-up string copy that seeds the
//! buffer `mes_append_escape` appends to)
//!
//! `FUN_8003CB54` ([`mes_string_end_offset`] / [`mes_append_escape`]) is
//! **wired**, and so off this list. Its buffer is a **MES-markup text
//! string**: the `< 0x1f` stop is the terminator/control range and the
//! `(b & 0xF0) == 0xC0` two-byte stride is the escape-token range, both
//! exactly as [`docs/formats/mes.md`](../../../docs/formats/mes.md)
//! tabulates them. Its one dumped caller, the battle anim commit
//! `FUN_8004AD80`, appends `{0xC2, id}` - the item-name token - to the
//! death-spoils captions (steal attack / thief's loot, HUD element `0x5B`),
//! and `legaia_engine_battle::battle_steal::compose_caption` makes the same
//! call when a slain monster's knockdown ends.

/// Byte offset of a MES-markup string's terminator - the write cursor
/// [`mes_append_escape`] splices at.
///
// PORT: FUN_8003cb54
///
/// WIRED: `legaia_engine_battle::battle_steal::compose_caption`, from the
/// death-spoils arm `World::resolve_monster_death_spoils` both hosts run.
///
/// The buffer is dialog bytecode, so the walk is the standard glyph-stride
/// walk of [`docs/formats/mes.md`](../../../docs/formats/mes.md):
///
/// * a byte `>= 0x1f` is a glyph or an escape lead,
/// * an escape lead (high nibble `0xC0`, i.e. `(b & 0xF0) == 0xC0`) carries one
///   argument byte and therefore consumes **two** positions,
/// * any other byte `>= 0x1f` is a single-byte glyph,
/// * the first byte `< 0x1f` is the terminator / control range and stops it.
///
/// The original walks a raw pointer; here the walk yields the byte offset of
/// the terminator. The scan reads the disassembly's exact loop: the
/// `(b & 0xF0) == 0xC0` test advances an extra byte *before* the unconditional
/// `+1`, so an argument byte in the `0x00..=0x1E` range - a `0xC1 0x00`
/// character-name substitution, say - cannot end the string early.
pub fn mes_string_end_offset(s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        if b < 0x1f {
            break;
        }
        // Escape token: skip the argument byte first (mirrors the original's
        // `addiu a3,a3,1; addiu t0,t0,1` inside the 0xC0 branch).
        if (b & 0xf0) == 0xc0 {
            i += 1;
        }
        i += 1;
    }
    i
}

/// Append the two-byte escape token `{tag, arg}` to a MES-markup string and
/// re-terminate it, returning the write offset.
///
// PORT: FUN_8003cb54
///
/// WIRED: `legaia_engine_battle::battle_steal::compose_caption`, from the
/// death-spoils arm `World::resolve_monster_death_spoils` both hosts run.
///
/// `tag` is a `0xC0..=0xCF` escape opcode and `arg` its argument - retail's one
/// dumped caller (`FUN_8004AD80`, `0x8004B2F8`) appends `{0xC2, item_id}`, the
/// item-name substitution. `buf` must have at least
/// `mes_string_end_offset(buf) + 3` bytes of capacity already allocated (the
/// retail buffer is fixed-size); a short buffer panics here where retail would
/// write past its end.
pub fn mes_append_escape(buf: &mut [u8], tag: u8, arg: u8) -> usize {
    let end = mes_string_end_offset(buf);
    buf[end] = tag;
    buf[end + 1] = arg;
    buf[end + 2] = 0;
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mes_end_of_empty_terminated_buffer_is_zero() {
        // First byte < 0x1f is the terminator.
        assert_eq!(mes_string_end_offset(&[0x00, 0, 0, 0]), 0);
        assert_eq!(mes_string_end_offset(&[0x1e]), 0);
    }

    #[test]
    fn mes_walk_skips_single_byte_glyphs() {
        // 0x20, 0x30 are single-byte glyphs (>=0x1f, high nibble not 0xC0),
        // then 0x00 terminator at offset 2.
        assert_eq!(mes_string_end_offset(&[0x20, 0x30, 0x00]), 2);
    }

    #[test]
    fn mes_escape_token_consumes_two_positions() {
        // 0xC5 is an escape lead: it + its argument occupy offsets 0,1; the
        // terminator 0x00 is at offset 2.
        assert_eq!(mes_string_end_offset(&[0xC5, 0x99, 0x00]), 2);
    }

    #[test]
    fn mes_escape_argument_below_0x1f_does_not_terminate_the_string() {
        // `0xC1 0x00` is the character-name substitution with argument 0 - the
        // argument is inside the terminator range and must be strided past,
        // which is the trap docs/formats/mes.md records for this walk.
        let buf = [0x25, 0xC1, 0x00, 0x40, 0x00];
        assert_eq!(mes_string_end_offset(&buf), 4);
    }

    #[test]
    fn mes_append_writes_token_and_reterminates() {
        let mut buf = [0x20u8, 0x00, 0, 0, 0, 0, 0];
        let at = mes_append_escape(&mut buf, 0xC3, 0x05);
        assert_eq!(at, 1);
        assert_eq!(&buf[..4], &[0x20, 0xC3, 0x05, 0x00]);
        // A second append strides the 0xC3 token and lands after its argument.
        let at2 = mes_append_escape(&mut buf, 0x40, 0x00);
        assert_eq!(at2, 3);
        assert_eq!(&buf[..6], &[0x20, 0xC3, 0x05, 0x40, 0x00, 0x00]);
    }
}
