//! The PSX display environment's screen-X mirror / halve transform.
//!
//! Port provenance (disassembly, not the decompiled C):
//! `see ghidra/scripts/funcs/800597c8.txt`.
//!
//! REF: FUN_800589D0 (`PutDispEnv` - the caller of `FUN_800597C8`, declined
//! rather than pending; see [`screen_x_mirror`])
//!
//! [`screen_x_mirror`] is a **replaced**
//! anchor and carries its own `REPLACED-BY:` marker. Its retail caller
//! `FUN_800589D0` is `PutDispEnv` - PsyQ libgpu, carried on the port-catalog
//! ignore list (`scripts/ci/port-catalog-ignore.toml`) precisely because a
//! port replaces the display-environment layer rather than reproducing it, and
//! recorded alongside this kernel in `docs/reference/functions/renderer.md`.
//! `DAT_80078D54` / `DAT_80078D57` select a **mirrored or half-width PSX
//! display environment**, and the engine programs no display environment at
//! all - one wgpu surface, one orientation - so there is no state for the
//! `< 2` gate at `80058A2C` to read and no host is owed a call.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
