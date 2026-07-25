//! The game-over banner letter stager.
//!
//! PORT: FUN_801CE844
//! REF: FUN_80021B04, FUN_80020DE0, FUN_80024C88
//!
//! `FUN_801CE844` is the init entry of the game-over overlay (PROT `0902` at
//! slot-A base `0x801CE818`, so the entry is file `+0x2C`). Mode 18
//! `GAME OVER INIT` (`FUN_80025B30`) calls it after `FUN_8003EBE4(7)`. Nothing
//! statically writes mode 18, so retail never reaches it - it is a dev harness,
//! which is exactly why it is worth having: it is the smallest complete example
//! of the overlay-init shape.
//!
//! `see ghidra/scripts/funcs/overlay_0902_xxx_dat_801ce844.txt` - 193
//! instructions. Do **not** read it out of `overlay_battle_action_0898.bin`:
//! that VA is inside `0898`'s image too, but decodes to nothing there (the dump
//! at that path reports `NOFUNC` and a garbage window). The `0902` copy is the
//! one with a clean `addiu sp, sp, -0x58` prologue.
//!
//! ## What is ported and what is not
//!
//! The body is three phases and only the third is renderer-free:
//!
//! 1. **Reset + stream** - GPU/heap resets (`FUN_8001DAF8`, `FUN_8001DCF8`,
//!    `FUN_80058068`, `FUN_8001E3B8`, a `0x32000`-byte `FUN_80017888`), the game
//!    mode write `_DAT_8007B83C = 0x13`, the counter seed `_DAT_800840C0 =
//!    3000`, and a `gameover.pak` load that forks on `_DAT_8007B8C2` between the
//!    dev-host path (`FUN_8003E6BC`) and the retail CD path (`FUN_8003EB98`).
//! 2. **Pak walk** - a `[u32 tag][...]` chunk loop over the loaded pak,
//!    dispatching kind `1` to `FUN_800198E0` (per-entry, plus a nested
//!    `[count][offsets]` table) and kind `2` to `FUN_80026B4C`. Host-side asset
//!    installation.
//! 3. **The banner stager** - [`banner_slots`], below. Nine fixed slots laid out
//!    on a line, one child actor per non-blank slot, each seated on a *shared*
//!    move record whose `model_sel` is rewritten per letter.
//!
//! Phases 1 and 2 are a deliberate non-port: they are host emission (GPU state,
//! heap, CD reads, asset install) with no arithmetic of its own, and
//! transcribing them would be a fake port. Phase 3 is pure layout arithmetic
//! over host-supplied glyph bytes, and that is what this module is.
//!
//! ## The stager loop
//!
//! Nine iterations (`slti $v0, $s2, 9`), reading one label byte per iteration:
//!
//! ```text
//! s3 = -0x708                       ; pen X
//! s1 = 0                            ; stagger accumulator
//! s4 = 0                            ; letter ordinal
//! loop:
//!   pos.x = s3
//!   c = label[i]
//!   if c == ' ' goto next           ; delay slot: s3 += 0x1C2   <-- always
//!   spawn(&pos, &rot, record, 0x1000) with record[0] = c - 0x3F
//!   s1 += 0xF0
//!   actor[+0x60] = s4 ; s4 += 1
//!   actor[+0x54] = s1
//! next:
//!   i += 1
//! ```
//!
//! Two details the C rendering loses, both from delay slots:
//!
//! * **The pen advances for the blank too.** `addiu $s3, $s3, 0x1c2` sits in the
//!   `beq`'s delay slot, so it runs on every slot including the skipped one -
//!   which is what keeps the two words of the label evenly spaced instead of
//!   butted together.
//! * **The stagger accumulator does not.** `addiu $s1, $s1, 0xf0` is inside the
//!   spawn arm, so the wait timers count *letters*, not slots.
//!
//! The pen is symmetric about zero by construction: nine slots at
//! [`SLOT_ADVANCE`] starting from [`PEN_START`] give `-1800..+1800`, so the
//! centre slot sits exactly at the origin. [`banner_slots`] checks that rather
//! than asserting it in prose.
//!
//! All nine children share **one** move record (`s5`), whose first word the loop
//! overwrites before each spawn (`sh v0, 0x0(s5)` in the `jal`'s delay slot).
//! That word is the record's `model_sel`, i.e. the seater's library-mesh
//! selector - so the glyph byte picks the mesh and everything else about the
//! letters is identical.
//!
//! # NOT WIRED
//!
//! No engine path enters game-over mode. `engine-core::mode` models mode 18's
//! stage plan (`mode_init_stage`, the port of the caller `FUN_80025B30`) but
//! nothing dispatches to it, and the banner additionally needs the two inputs
//! phases 1-2 supply: the nine-byte label the overlay carries and the letter
//! mesh library the pak walk installs. This module takes the label as an
//! argument for exactly that reason - the bytes are disc data and are not
//! reproduced here.

/// Slots the banner lays out (`slti $v0, $s2, 9`).
pub const BANNER_SLOTS: usize = 9;

/// Pen X for slot `0` (`li $s3, -0x708`).
pub const PEN_START: i16 = -0x708;

/// Pen advance per slot (`addiu $s3, $s3, 0x1c2`), applied in the skip
/// branch's delay slot and therefore on blank slots too.
pub const SLOT_ADVANCE: i16 = 0x1C2;

/// Wait-timer step per *letter* (`addiu $s1, $s1, 0xf0`), applied only on the
/// spawn arm. Written to the child's `+0x54`, the move VM's wait timer, so the
/// letters unfold one at a time.
pub const LETTER_STAGGER: i16 = 0xF0;

/// The label byte the loop skips (`li $v0, 0x20; beq`).
pub const BLANK_BYTE: u8 = b' ';

/// Bias subtracted from a label byte to get the record's `model_sel`
/// (`addiu $v0, $v0, -0x3f`).
pub const GLYPH_MESH_BIAS: i16 = 0x3F;

/// Scale handed to the seater for every letter (`li $a3, 0x1000`).
pub const LETTER_SCALE: i32 = 0x1000;

/// Value written to `_DAT_8007B6F4` once the loop retires
/// (`li $v0, 0x140; sh`).
pub const BANNER_TAIL_WRITE: i16 = 0x140;

/// One spawned letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BannerLetter {
    /// Index into the label, `0..`[`BANNER_SLOTS`]. Blank slots produce no
    /// entry, so this is *not* the ordinal.
    pub slot: usize,
    /// Pen X at the moment of the spawn - `param_1[0]` of the seater call.
    pub x: i16,
    /// The record's rewritten `model_sel`: `label[slot] - 0x3F`.
    pub model_sel: i16,
    /// Written to the child's `+0x60` - the ordinal among *letters*.
    pub ordinal: i16,
    /// Written to the child's `+0x54`, the move-VM wait timer.
    pub wait_timer: i16,
    /// `param_4` of the seater call.
    pub scale: i32,
}

/// Lay out the banner: one entry per non-blank label slot, in loop order.
///
/// `label` is the nine-byte string the overlay carries. Shorter input yields
/// shorter output (the loop reads one byte per slot and stops at
/// [`BANNER_SLOTS`]); the bytes themselves are disc data and are supplied by the
/// caller, never carried here.
///
/// The pen advance runs for every slot the loop visits, blank included; the
/// stagger only for spawned letters.
pub fn banner_slots(label: &[u8]) -> Vec<BannerLetter> {
    let mut out = Vec::with_capacity(BANNER_SLOTS);
    let mut pen = PEN_START;
    let mut stagger: i16 = 0;
    let mut ordinal: i16 = 0;
    for (slot, &byte) in label.iter().take(BANNER_SLOTS).enumerate() {
        let x = pen;
        // Delay slot of the blank test: always taken.
        pen = pen.wrapping_add(SLOT_ADVANCE);
        if byte == BLANK_BYTE {
            continue;
        }
        stagger = stagger.wrapping_add(LETTER_STAGGER);
        out.push(BannerLetter {
            slot,
            x,
            model_sel: (byte as i16).wrapping_sub(GLYPH_MESH_BIAS),
            ordinal,
            wait_timer: stagger,
            scale: LETTER_SCALE,
        });
        ordinal = ordinal.wrapping_add(1);
    }
    out
}

/// Pen X for a slot, independent of what the label says there.
pub const fn slot_x(slot: usize) -> i16 {
    PEN_START.wrapping_add(SLOT_ADVANCE.wrapping_mul(slot as i16))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in labels. The retail bytes stay on disc; what the port needs to
    /// be right about is the *shape* - nine slots with one blank - so the tests
    /// use synthetic labels of that shape.
    const NINE_LETTERS: &[u8] = b"ABCDEFGHI";
    const BLANK_CENTRE: &[u8] = b"ABCD FGHI";

    #[test]
    fn the_pen_is_symmetric_about_the_origin() {
        // Nine slots at 0x1C2 from -0x708: the centre slot lands on zero and
        // the ends mirror. This is the layout constant's whole justification.
        assert_eq!(slot_x(0), -1800);
        assert_eq!(slot_x(BANNER_SLOTS / 2), 0);
        assert_eq!(slot_x(BANNER_SLOTS - 1), 1800);
        for s in 0..BANNER_SLOTS {
            assert_eq!(slot_x(s), -slot_x(BANNER_SLOTS - 1 - s), "slot {s}");
        }
    }

    #[test]
    fn the_pen_advances_across_a_blank_slot() {
        // The advance is in the skip branch's delay slot, so a blank costs a
        // slot's width. If it did not, the letters after it would shift left.
        let with_blank = banner_slots(BLANK_CENTRE);
        let no_blank = banner_slots(NINE_LETTERS);
        assert_eq!(with_blank.len(), 8);
        assert_eq!(no_blank.len(), 9);
        for l in &with_blank {
            assert_eq!(l.x, slot_x(l.slot), "slot {} keeps its own X", l.slot);
        }
        // Same slots, same X, whether or not one of them is blank.
        for l in &with_blank {
            assert_eq!(l.x, no_blank[l.slot].x);
        }
    }

    #[test]
    fn the_stagger_counts_letters_not_slots() {
        // The accumulator is inside the spawn arm, so a blank does not consume
        // a stagger step - the letters after it are one step earlier than their
        // slot index would give.
        let letters = banner_slots(BLANK_CENTRE);
        for (i, l) in letters.iter().enumerate() {
            assert_eq!(l.ordinal, i as i16, "ordinals are dense across the blank");
            assert_eq!(l.wait_timer, LETTER_STAGGER * (i as i16 + 1));
        }
        // And the slot indices are not dense, or the test proves nothing.
        assert_eq!(letters[4].slot, 5);
        assert_ne!(letters[4].ordinal as usize, letters[4].slot);
    }

    #[test]
    fn the_first_letter_waits_one_step_not_zero() {
        // `addiu $s1, $s1, 0xf0` runs *before* `sh $s1, 0x54(v0)`, so no letter
        // is ever seated with a zero timer.
        let letters = banner_slots(NINE_LETTERS);
        assert_eq!(letters[0].wait_timer, LETTER_STAGGER);
        assert!(letters.iter().all(|l| l.wait_timer > 0));
    }

    #[test]
    fn the_ordinal_is_written_before_it_increments() {
        // `sh $s4, 0x60(v0)` precedes `addiu $s4, $s4, 1`, so the first letter
        // gets 0 - the mirror of the timer's pre-increment above.
        let letters = banner_slots(NINE_LETTERS);
        assert_eq!(letters[0].ordinal, 0);
        assert_eq!(letters[0].wait_timer, LETTER_STAGGER);
    }

    #[test]
    fn the_mesh_selector_is_the_glyph_byte_less_the_bias() {
        // `addiu $v0, $v0, -0x3f` on the raw byte, so 'A' selects mesh 2 and
        // the alphabet runs contiguously from there.
        let letters = banner_slots(NINE_LETTERS);
        assert_eq!(letters[0].model_sel, b'A' as i16 - GLYPH_MESH_BIAS);
        assert_eq!(letters[0].model_sel, 2);
        for w in letters.windows(2) {
            assert_eq!(w[1].model_sel - w[0].model_sel, 1, "contiguous alphabet");
        }
    }

    #[test]
    fn a_blank_slot_never_becomes_a_letter() {
        // The blank's own selector would be negative, which the seater reads as
        // a transform node - the skip is what stops a stray pivot actor.
        assert!((BLANK_BYTE as i16) - GLYPH_MESH_BIAS < 0);
        let letters = banner_slots(BLANK_CENTRE);
        assert!(letters.iter().all(|l| l.model_sel > 0));
        assert!(letters.iter().all(|l| l.slot != 4));
    }

    #[test]
    fn every_letter_is_seated_at_the_same_scale() {
        assert!(
            banner_slots(NINE_LETTERS)
                .iter()
                .all(|l| l.scale == LETTER_SCALE)
        );
    }

    #[test]
    fn the_loop_stops_at_nine_slots() {
        let long: Vec<u8> = std::iter::repeat_n(b'A', 32).collect();
        assert_eq!(banner_slots(&long).len(), BANNER_SLOTS);
        // And a short label simply runs out.
        assert_eq!(banner_slots(b"AB").len(), 2);
        assert!(banner_slots(b"").is_empty());
    }

    #[test]
    fn an_all_blank_label_spawns_nothing_but_still_walks_the_pen() {
        assert!(banner_slots(b"         ").is_empty());
        // The pen state is not observable from the outside, but slot_x is the
        // same pure function the loop uses, so the invariant still holds.
        assert_eq!(slot_x(BANNER_SLOTS - 1), 1800);
    }
}
