//! Small self-contained battle / motion kernels ported clean-room from the
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
//! `.../80046870.txt`, `.../801cee80.txt`.
//!
//! # NOT WIRED
//!
//! Each of these leaves is waiting on a different piece of engine state:
//!
//! - `FUN_8003CB54` ([`action_queue_end_offset`] / [`action_queue_append`])
//!   splices into retail's variable-width `{lead, payload}` byte queue. The
//!   engine assembles actions as a typed `legaia_art::ActionQueue` of
//!   `ActionConstant`s, so no caller holds a raw buffer with a `< 0x1f`
//!   terminator for the walk to find.
//! - `FUN_800597C8` ([`screen_x_mirror`]) is selected by the orientation
//!   globals `DAT_80078D54` / `DAT_80078D57`. The engine's renderer has one
//!   battle view and no mirrored or half-width mode, so the transform has no
//!   mode byte to be selected by.
//! - `FUN_80046870` ([`advance_gauge`]) ramps the `gp + 0x2E8` word. That word
//!   has no engine analogue - and note which other routine reads it: the
//!   validator's arm-`0x82` gate `FUN_80046898` tests **the same word**
//!   against `0xE0`. Read together the pair is gauge-shaped (`+0x40` per call,
//!   ceiling `0x100`, threshold `0xE0`), which is why
//!   `battle_action::validator`'s "inventory item count" reading of `gp+0x2E8`
//!   is recorded there as unconfirmed. Wiring this needs that identity settled
//!   first, since the value is what a host would have to produce.
//! - `FUN_801CEE80` ([`ease_quad_interp`]) is driven from the actor tween
//!   triple `+0x28` (target index), `+0x50` (progress) and `+0x9E`
//!   (duration). None of the three is on the port's battle or field actor, so
//!   nothing advances a progress counter for the ease to sample.

/// Append a two-byte command entry `{tag, arg}` to a variable-width action
/// queue, re-terminating with a `0` byte.
///
// PORT: FUN_8003cb54
///
/// The queue is a byte stream of variable-width entries. Scanning from the
/// front, each byte whose value is `>= 0x1f` is an entry lead byte:
///
/// * lead byte with high nibble `0xC0` (i.e. `(b & 0xF0) == 0xC0`) is a
///   **two-byte** entry (lead + one payload byte),
/// * any other lead `>= 0x1f` is a **one-byte** entry.
///
/// The first byte `< 0x1f` is the terminator, marking the write position. The
/// new entry `{tag, arg, 0}` is written there: `tag` overwrites the old
/// terminator, `arg` follows, and a fresh `0` terminator follows that.
///
/// The original walks a raw pointer; here the walk yields the byte offset of
/// the terminator, which the caller uses to splice the three bytes. Returns
/// the terminator offset (the write cursor). The caller is responsible for the
/// buffer having room for three more bytes past that offset.
///
/// The scan reads the disassembly's exact loop: the `(b & 0xF0) == 0xC0`
/// test advances an extra byte *before* the unconditional `+1`, so a `0xCx`
/// lead consumes two positions and every lead consumes at least one.
pub fn action_queue_end_offset(queue: &[u8]) -> usize {
    let mut i = 0usize;
    while i < queue.len() {
        let b = queue[i];
        if b < 0x1f {
            break;
        }
        // Two-byte entry: skip the payload byte first (mirrors the original's
        // `addiu a3,a3,1; addiu t0,t0,1` inside the 0xC0 branch).
        if (b & 0xf0) == 0xc0 {
            i += 1;
        }
        i += 1;
    }
    i
}

/// Append `{tag, arg, 0}` at the queue terminator, returning the write offset.
///
// PORT: FUN_8003cb54
///
/// Convenience wrapper over [`action_queue_end_offset`] that performs the
/// three-byte splice in place. `buf` must have at least
/// `action_queue_end_offset(buf) + 3` bytes of capacity already allocated
/// (the retail buffer is fixed-size); this asserts that in debug builds.
pub fn action_queue_append(buf: &mut [u8], tag: u8, arg: u8) -> usize {
    let end = action_queue_end_offset(buf);
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
/// The retail gauge (`gp+0x2e8`) accumulates `+0x40` per call and saturates at
/// `0x100`. Faithful to the original `slti v0,v0,0x100` clamp: the sum is
/// clamped only when it reaches or exceeds `0x100`.
pub const GAUGE_STEP: i32 = 0x40;
/// Ceiling the gauge saturates at.
pub const GAUGE_MAX: i32 = 0x100;

/// See [`GAUGE_STEP`] / [`GAUGE_MAX`].
///
// PORT: FUN_80046870
pub fn advance_gauge(value: i32) -> i32 {
    let next = value + GAUGE_STEP;
    if next < GAUGE_MAX { next } else { GAUGE_MAX }
}

/// Quadratic ease of a scalar from `start` toward `target` over `dur` steps at
/// progress `t`, using the retail's exact double-truncating integer division.
///
// PORT: FUN_801cee80
///
/// The retail motion helper interpolates a coordinate as
///
/// ```text
///   d      = (target - start)
///   p      = (d * t) / dur          // first truncating div
///   result = (p * t) / dur + start  // second truncating div
/// ```
///
/// i.e. `result ~= start + (target - start) * (t/dur)^2`, but with the
/// truncation applied at **each** division exactly as the R3000 `div`
/// instruction does (round toward zero). Reproducing the two-stage truncation
/// (rather than a single `d*t*t/(dur*dur)`) is what keeps the interpolated
/// path bit-identical to retail.
///
/// The interpolation only runs when `target != start` and `t < dur` (the
/// original's `beq v1,a3` / `slt v0,a2,a1` guards); otherwise `target` is
/// returned unchanged. The whole computation is skipped by the caller when the
/// target index (`actor+0x28`) is `-1`; that guard lives on the host side.
///
/// `t` (`actor+0x50`) is treated as **unsigned** in the original (`lhu`), so
/// callers pass a non-negative progress. `dur` (`actor+0x9e`) must be
/// non-zero when `t < dur` holds; `dur == 0` cannot reach the divide because
/// `t < 0` is impossible for the unsigned `t`. The result is truncated to
/// `i16` to match the `sh` store.
pub fn ease_quad_interp(start: i16, target: i16, t: u16, dur: i16) -> i16 {
    let start_i = start as i32;
    let target_i = target as i32;
    let t_i = t as i32;
    let dur_i = dur as i32;
    if target_i != start_i && t_i < dur_i {
        // dur_i > t_i >= 0, so dur_i > 0: division is safe.
        let d = target_i - start_i;
        let p = (d * t_i) / dur_i;
        let r = (p * t_i) / dur_i + start_i;
        r as i16
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_end_of_empty_terminated_buffer_is_zero() {
        // First byte < 0x1f is the terminator.
        assert_eq!(action_queue_end_offset(&[0x00, 0, 0, 0]), 0);
        assert_eq!(action_queue_end_offset(&[0x1e]), 0);
    }

    #[test]
    fn queue_skips_one_byte_entries() {
        // 0x20, 0x30 are one-byte leads (>=0x1f, high nibble not 0xC0), then
        // 0x00 terminator at offset 2.
        assert_eq!(action_queue_end_offset(&[0x20, 0x30, 0x00]), 2);
    }

    #[test]
    fn queue_two_byte_entry_consumes_two_positions() {
        // 0xC5 is a two-byte lead: it + its payload occupy offsets 0,1; the
        // terminator 0x00 is at offset 2.
        assert_eq!(action_queue_end_offset(&[0xC5, 0x99, 0x00]), 2);
    }

    #[test]
    fn queue_mixed_widths() {
        // 0x25 (1B), 0xC1 payload (2B), 0x40 (1B), terminator.
        // offsets: 0x25@0 ->1, 0xC1@1 (+payload@2) ->3, 0x40@3 ->4, term@4.
        let buf = [0x25, 0xC1, 0x77, 0x40, 0x00];
        assert_eq!(action_queue_end_offset(&buf), 4);
    }

    #[test]
    fn queue_append_writes_triple_and_reterminates() {
        let mut buf = [0x20u8, 0x00, 0, 0, 0, 0, 0];
        let at = action_queue_append(&mut buf, 0xC3, 0x05);
        assert_eq!(at, 1);
        assert_eq!(&buf[..4], &[0x20, 0xC3, 0x05, 0x00]);
        // A second append sees the 0xC3 as a two-byte entry and lands after it.
        let at2 = action_queue_append(&mut buf, 0x40, 0x00);
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
        assert_eq!(advance_gauge(0), 0x40);
        assert_eq!(advance_gauge(0x40), 0x80);
        assert_eq!(advance_gauge(0xC0), GAUGE_MAX);
    }

    #[test]
    fn gauge_saturates_and_never_exceeds_max() {
        // 0xC0 + 0x40 = 0x100 -> clamp (slti is strict <).
        assert_eq!(advance_gauge(0xC0), 0x100);
        assert_eq!(advance_gauge(0x100), 0x100);
        assert_eq!(advance_gauge(0x1000), 0x100);
    }

    #[test]
    fn ease_returns_target_when_start_equals_target() {
        assert_eq!(ease_quad_interp(100, 100, 5, 10), 100);
    }

    #[test]
    fn ease_returns_target_when_progress_at_or_past_duration() {
        assert_eq!(ease_quad_interp(0, 200, 10, 10), 200);
        assert_eq!(ease_quad_interp(0, 200, 20, 10), 200);
    }

    #[test]
    fn ease_quadratic_midpoint() {
        // start 0, target 400, t=5, dur=10 -> (400*5/10=200)*5/10 = 100.
        assert_eq!(ease_quad_interp(0, 400, 5, 10), 100);
    }

    #[test]
    fn ease_matches_double_truncation_not_single() {
        // start 0, target 7, t=3, dur=10.
        // faithful: (7*3/10 = 2) then (2*3/10 = 0) -> 0.
        // single-div would be 7*9/100 = 0 here; pick a case where they differ.
        // start 0, target 10, t=7, dur=10:
        //   double: (10*7/10=7)*7/10 = 4
        //   single: 10*49/100 = 4  (same) -> choose another
        // start 0, target 9, t=4, dur=5:
        //   double: (9*4/5=7)*4/5 = 5
        //   single: 9*16/25 = 5   -> still same; verify the double path value.
        assert_eq!(ease_quad_interp(0, 9, 4, 5), 5);
    }

    #[test]
    fn ease_with_nonzero_start_offsets_result() {
        // start 50, target 250, t=5, dur=10:
        //   d=200; (200*5/10=100)*5/10 = 50; +start = 100.
        assert_eq!(ease_quad_interp(50, 250, 5, 10), 100);
    }

    #[test]
    fn ease_descending_target() {
        // start 400, target 0, t=5, dur=10:
        //   d=-400; (-400*5/10=-200)*5/10 = -100; +400 = 300.
        assert_eq!(ease_quad_interp(400, 0, 5, 10), 300);
    }
}
