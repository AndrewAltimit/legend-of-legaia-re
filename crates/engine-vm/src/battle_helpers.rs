//! Small self-contained battle / motion kernels ported from the
//! `SCUS_942.54` battle code.
//!
//! These are the leaf arithmetic / byte-buffer helpers underneath the larger
//! battle-action state machine - each is a fixed-point computation with no
//! GTE / GPU / driver dependency, so each ports 1:1 to Rust and is unit-tested
//! against the exact integer behaviour of the R3000 (truncating `div`, `i16`
//! wraparound, `slti` clamp).
//!
//! Port provenance (disassembly, not the decompiled C):
//! `see ghidra/scripts/funcs/8003cb54.txt`, `.../800597c8.txt`,
//! `.../80046870.txt`.
//!
//! REF: FUN_8004AD80 (the one dumped caller of `FUN_8003CB54`)
//! REF: FUN_8003CA78 (its sibling: the marked-up string copy that seeds the
//! buffer `mes_append_escape` appends to)
//! REF: FUN_800589D0 (`PutDispEnv` - the caller of `FUN_800597C8`, declined
//! rather than pending; see [`screen_x_mirror`])
//!
//! # NOT WIRED
//!
//! [`top_up_cooldown`] is the one leaf still waiting, and what it waits on is
//! outside the battle code despite the module it lives in.
//!
//! [`screen_x_mirror`] is not on that list: it is the file's one **replaced**
//! anchor and carries its own `REPLACED-BY:` marker. Its retail caller
//! `FUN_800589D0` is `PutDispEnv` - PsyQ libgpu, carried on the port-catalog
//! ignore list (`scripts/ci/port-catalog-ignore.toml`) precisely because a
//! port replaces the display-environment layer rather than reproducing it, and
//! recorded alongside this kernel in `docs/reference/functions/renderer.md`.
//! `DAT_80078D54` / `DAT_80078D57` select a **mirrored or half-width PSX
//! display environment**, and the engine programs no display environment at
//! all - one wgpu surface, one orientation - so there is no state for the
//! `< 2` gate at `80058A2C` to read and no host is owed a call.
//!
//! - `FUN_8003CB54` ([`mes_string_end_offset`] / [`mes_append_escape`]) is
//!   **wired**, and so off this list. Its buffer is a **MES-markup text
//!   string**: the `< 0x1f` stop is the terminator/control range and the
//!   `(b & 0xF0) == 0xC0` two-byte stride is the escape-token range, both
//!   exactly as [`docs/formats/mes.md`](../../../docs/formats/mes.md)
//!   tabulates them. Its one dumped caller, the battle anim commit
//!   `FUN_8004AD80`, appends `{0xC2, id}` - the item-name token - to the
//!   death-spoils captions (steal attack / thief's loot, HUD element `0x5B`),
//!   and `legaia_engine_core::battle_steal::compose_caption` makes the same
//!   call when a slain monster's knockdown ends.
//! - `FUN_80046870` ([`top_up_cooldown`]) is the whole of the item applier's
//!   selector-`0x82` arm (`0x800421A0`, the jump table's slot `0x82`), and
//!   `0x82` is the effect class of **Incense** (item `0x8A`, "Decrease
//!   encounter rate for a period of time"). The word it ramps, `gp + 0x2E8` =
//!   `_DAT_8007B600` (`gp` is `0x8007B318`: `80026ca8` `lui gp,0x8008` +
//!   `80026cac` `addiu gp,gp,-0x4ce8`), is the **Incense window**, counted in
//!   walk-regen ticks rather than frames:
//!
//!   - the field walk tick `FUN_801D0B90` (PROT 0897) decrements it once per
//!     running tick (`0x801D0CD4..0x801D0CE8`) and, on the transition to zero,
//!     installs `0x801F2278` as the field event pointer `_DAT_8007B450`, sets
//!     `0x80000` in the player actor's flag word and registers it with
//!     `FUN_80020DE0` - the "the Incense wore off" hand-off;
//!   - the region encounter roll `FUN_801D9E1C` skips the whole roll while it
//!     is non-zero (`lw v0,-0x4a00(v0)` / `bne v0,zero` at `0x801DA174`), so
//!     an Incense suppresses encounters outright for its window rather than
//!     scaling the rate;
//!   - the validator's arm-`0x82` gate `FUN_80046898` admits another use only
//!     while fewer than `0xE0` ticks remain.
//!
//!   Every host of that is field or pause-menu code. The engine already
//!   carries the counter (`engine-core`'s `FieldLocomotion::walk_regen_window`,
//!   decremented by `walk_regen::tick_walk_regen`), but nothing arms it: the
//!   pause Items Incense confirm ends in `SpecialUseOutcome::EncounterSuppress`
//!   and drops it, and the encounter roll does not read the window. Wiring is
//!   those two edits - top the window up through this kernel on the
//!   Incense outcome, and gate the region roll on it being zero.

/// Byte offset of a MES-markup string's terminator - the write cursor
/// [`mes_append_escape`] splices at.
///
// PORT: FUN_8003cb54
///
/// WIRED: `legaia_engine_core::battle_steal::compose_caption`, from the
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
/// WIRED: `legaia_engine_core::battle_steal::compose_caption`, from the
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

/// Screen-orientation mode consumed by [`screen_x_mirror`].
///
/// The retail global `DAT_80078d54` selects one of these; other values leave
/// the coordinate untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenOrient {
    /// `DAT_80078d54 == 1`.
    Mode1,
    /// `DAT_80078d54 == 2`.
    Mode2,
    /// Any other value.
    Other,
}

impl ScreenOrient {
    /// Decode the raw orientation byte.
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => ScreenOrient::Mode1,
            2 => ScreenOrient::Mode2,
            _ => ScreenOrient::Other,
        }
    }
}

/// Map an on-screen X coordinate through the retail screen mirror / halve
/// transform used when the battle view is flipped or split.
///
// PORT: FUN_800597c8
///
/// REPLACED-BY: `legaia_engine_render::renderer`'s single fixed-orientation
/// wgpu surface. The port programs no PSX display environment - the retail
/// caller `FUN_800589D0` is ignore-listed libgpu - so `DAT_80078D54` /
/// `DAT_80078D57` have no counterpart, the identity arm is the only one
/// reachable, and screen X reaches the surface unchanged. Nothing is owed a
/// call.
///
/// `x` is the entry's X (`param_1[0]`) and `width` is its box width
/// (`param_1[2]`, i.e. the `u16` at byte offset 4). `mirror` corresponds to
/// the retail flag `DAT_80078d57` (mirror when non-zero). The pivot constant
/// is `0x400` (1024).
///
/// | orient | mirror | result                         |
/// | ------ | ------ | ------------------------------ |
/// | Mode1  | false  | `x`                            |
/// | Mode1  | true   | `(0x400 - width) - x`          |
/// | Mode2  | false  | `x / 2` (toward zero)          |
/// | Mode2  | true   | `(0x400 - width/2) - x`        |
/// | Other  | any    | `x`                            |
///
/// The `/2` matches the original's `(v - (v >> 31)) >> 1` idiom, which is
/// integer division rounding toward zero (differs from arithmetic `>> 1` for
/// negative `width`). All arithmetic is `i32`; inputs are sign-extended `i16`.
pub fn screen_x_mirror(orient: ScreenOrient, mirror: bool, x: i16, width: i16) -> i32 {
    let x = x as i32;
    let width = width as i32;
    // Division toward zero, matching `(w - (w >> 31)) >> 1`.
    let half = |v: i32| (v - (v >> 31)) >> 1;
    match orient {
        ScreenOrient::Mode1 => {
            if mirror {
                (0x400 - width) - x
            } else {
                x
            }
        }
        ScreenOrient::Mode2 => {
            if mirror {
                (0x400 - half(width)) - x
            } else {
                half(x)
            }
        }
        ScreenOrient::Other => x,
    }
}

/// Advance a per-frame charge gauge by one step and clamp at the ceiling.
///
// PORT: FUN_80046870
///
/// The retail word `gp+0x2e8` (`_DAT_8007B600`) is a **frame cooldown**, not a
/// charge gauge: each call tops it up by `+0x40` frames and saturates it at
/// `0x100`, and two overlay sites count it down by one per frame and gate on
/// zero (see the module notes). Faithful to the original `slti v0,v0,0x100`
/// clamp: the sum is clamped only when it reaches or exceeds `0x100`.
pub const COOLDOWN_STEP: i32 = 0x40;
/// Ceiling the cooldown saturates at.
pub const COOLDOWN_MAX: i32 = 0x100;

/// See [`COOLDOWN_STEP`] / [`COOLDOWN_MAX`].
///
// PORT: FUN_80046870 NOT WIRED: its host is the pause Items Incense confirm
// (`SpecialUseOutcome::EncounterSuppress`, which ends the flow without arming
// anything) topping up `FieldLocomotion::walk_regen_window`, with the region
// encounter roll gating on that window - field and menu code, see the module
// notes.
pub fn top_up_cooldown(value: i32) -> i32 {
    let next = value + COOLDOWN_STEP;
    if next < COOLDOWN_MAX {
        next
    } else {
        COOLDOWN_MAX
    }
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

    #[test]
    fn screen_mode1_passthrough_and_mirror() {
        assert_eq!(screen_x_mirror(ScreenOrient::Mode1, false, 300, 64), 300);
        assert_eq!(
            screen_x_mirror(ScreenOrient::Mode1, true, 300, 64),
            (0x400 - 64) - 300
        );
    }

    #[test]
    fn screen_mode2_halves_and_mirror() {
        assert_eq!(screen_x_mirror(ScreenOrient::Mode2, false, 300, 64), 150);
        assert_eq!(
            screen_x_mirror(ScreenOrient::Mode2, true, 300, 64),
            (0x400 - 32) - 300
        );
    }

    #[test]
    fn screen_mode2_half_rounds_toward_zero_for_negatives() {
        // -3 / 2 toward zero = -1 (not -2 as arithmetic >>1 would give).
        assert_eq!(screen_x_mirror(ScreenOrient::Mode2, false, -3, 0), -1);
        // mirror path halves width the same way.
        // formula `(0x400 - half(width)) - x` with x=0, half(-3)=-1.
        assert_eq!(
            screen_x_mirror(ScreenOrient::Mode2, true, 0, -3),
            0x400 - (-1)
        );
    }

    #[test]
    fn screen_other_orient_is_passthrough() {
        assert_eq!(screen_x_mirror(ScreenOrient::Other, true, 42, 64), 42);
        assert_eq!(ScreenOrient::from_byte(0), ScreenOrient::Other);
        assert_eq!(ScreenOrient::from_byte(1), ScreenOrient::Mode1);
        assert_eq!(ScreenOrient::from_byte(2), ScreenOrient::Mode2);
        assert_eq!(ScreenOrient::from_byte(9), ScreenOrient::Other);
    }

    #[test]
    fn gauge_accumulates_by_step() {
        assert_eq!(top_up_cooldown(0), 0x40);
        assert_eq!(top_up_cooldown(0x40), 0x80);
        assert_eq!(top_up_cooldown(0xC0), COOLDOWN_MAX);
    }

    #[test]
    fn gauge_saturates_and_never_exceeds_max() {
        // 0xC0 + 0x40 = 0x100 -> clamp (slti is strict <).
        assert_eq!(top_up_cooldown(0xC0), 0x100);
        assert_eq!(top_up_cooldown(0x100), 0x100);
        assert_eq!(top_up_cooldown(0x1000), 0x100);
    }
}
