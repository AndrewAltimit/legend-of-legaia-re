//! The field overlay's **actor-reflection** callback: a leaf that mirrors one
//! actor's pose onto a second actor across an axis-aligned mirror line, and
//! flips the facing angle to match.
//!
//! The port of `FUN_801e5154` is [`tick_reflection`], which carries the `PORT`
//! tag and its own wiring disclosure. It is deliberately not repeated at module
//! level - a `//!  PORT:` line makes the whole file a second, coarser anchor for
//! the same address, with no disclosure of its own.
//!
//! # Provenance and why the old readings were wrong
//!
//! The VA `0x801E5154` has had two contradictory readings in this repo, and
//! neither survives a read of the disc bytes:
//!
//! * The `overlay_0897_xxx_dat_801e5154.txt` dump resolves `entry=801E5134`
//!   and disassembles a run of `lb sp, imm(zero)` - a pointer table decoded as
//!   code. That dump is a wrong-base import: the words it prints decode as
//!   `0x801D5xxx` addresses, which is the save-UI band, not the field overlay.
//! * The extracted field overlay itself (PROT entry `0897_xxx_dat`, slot-A base
//!   `0x801CE818`, file offset `0x1693C`) holds **code** at this VA: 120
//!   instructions from `0x801E5154` to the `jr ra` at `0x801E5330`, with the
//!   next function's `addiu sp, sp, -0x38` prologue at `0x801E5338`.
//!
//! It is a leaf - no stack frame, no calls - which is why a frame scan reports
//! "not an entry". It is reached through a function pointer: the word
//! `0x801E5154` appears in the same image at VA `0x801F2950`, inside a
//! descriptor record, which is what `docs/subsystems/script-vm-menuctrl.md`
//! registers as `&LAB_801E5154`. There is no `jal` to it anywhere on the disc,
//! so a call-site scan cannot see it either.
//!
//! # Shape
//!
//! Called with the *controller* actor in `a0`. The controller carries:
//!
//! | Field | Meaning |
//! |---|---|
//! | `+0x90` | destination actor - the reflection |
//! | `+0x94` | source actor - the thing being reflected |
//! | `+0x80` | mirror line X, or `0` for "no X mirror" |
//! | `+0x82` | mirror line Z, or `0` for "no Z mirror" |
//! | `+0x84 / +0x86` | active tile rect, minimum corner |
//! | `+0x88 / +0x8A` | active tile rect, maximum corner |
//!
//! and the routine, in order:
//!
//! 1. Tears itself down (`+0x10 |= 8`) if **either** end of the pair already
//!    carries the tear-down bit `8` in its own `+0x10`. This is the same
//!    `+0x10` bit `8` the ledge-hop setup sets on a failed second allocation.
//! 2. Returns without doing anything when the source's `+0x10` carries bit
//!    `0x100`.
//! 3. Converts the source's world position to tile space - `(v + 0x40) >> 7`,
//!    the standard 128-unit field tile with a half-tile bias - and returns
//!    when it lies outside the controller's rect. This is what makes a mirror
//!    a *place*: the reflection only tracks while the source stands in it.
//! 4. When both actors' `+0x64` (the animation-set selector) agree, copies the
//!    source's animation state across - `+0x5C`, `+0x68`, `+0x6A` - and moves
//!    the source's `+0x10` bit `0x01000000` into the destination, leaving the
//!    destination's other flags alone.
//! 5. Reflects the position and the facing angle, on whichever of the three
//!    arms the `+0x80` / `+0x82` pair selects.
//!
//! # The reflection algebra
//!
//! Facing is the usual `0x1000`-per-turn field angle, so `0x800` is a
//! half-turn. Retail's three arms are exactly the three axis-aligned planar
//! reflections:
//!
//! | `+0x80` | `+0x82` | X | Z | facing |
//! |---|---|---|---|---|
//! | `0` | `zz` | copied | `2*zz - z` | `-0x800 - a` |
//! | `xx` | `0` | `2*xx - x` | copied | `-a` |
//! | `xx` | `zz` | `2*xx - x` | `2*zz - z` | `a + 0x800` |
//!
//! Y (`+0x16`) is copied unchanged on every arm - the mirror is a ground-plane
//! mirror, never a vertical one. The third arm is a point reflection through
//! `(xx, zz)`, and a point reflection of a heading is a half-turn, which is
//! precisely the `+0x800`. Modulo `0x1000` the first arm's `-0x800 - a` and
//! the more familiar `0x800 - a` are the same value; retail writes the former
//! because it materialises `-0x800` as a single `addiu v0, zero, -0x800`.
//!
//! # Not wired
//!
//! The engine has no actor-callback table to hang this off - the descriptor
//! record at `0x801F2950` that holds the pointer is not parsed yet, and the
//! actor fields it reads (`+0x64`, `+0x68`, `+0x6A`, `+0x80..+0x8A`, `+0x90`,
//! `+0x94`) have no counterpart on `engine_core::world`'s actor. Wiring it
//! means editing `engine-core/src/world/**`, owned by another change.

/// Actor `+0x10` flag bits this routine tests or moves.
pub mod flags {
    /// Tear-down request: the actor is being released this frame.
    pub const TEARDOWN: u32 = 0x0000_0008;
    /// Suppress bit on the source - the reflection freezes while it is set.
    pub const SUPPRESS: u32 = 0x0000_0100;
    /// The one flag copied from source to destination when their `+0x64`
    /// animation selectors agree.
    pub const ANIM_MIRROR: u32 = 0x0100_0000;
}

/// Full turn of the field facing angle; a half-turn is `HALF_TURN`.
pub const FULL_TURN: i32 = 0x1000;
/// Half turn - the point-reflection facing offset.
pub const HALF_TURN: i16 = 0x0800;

/// The subset of an actor this routine reads or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReflectActor {
    /// `+0x10` flag word.
    pub flags: u32,
    /// `+0x14` world X.
    pub x: i16,
    /// `+0x16` world Y.
    pub y: i16,
    /// `+0x18` world Z.
    pub z: i16,
    /// `+0x26` facing angle.
    pub facing: i16,
    /// `+0x5C` animation cursor.
    pub anim_cursor: u16,
    /// `+0x64` animation-set selector - the equality gate on the state copy.
    pub anim_set: i16,
    /// `+0x68` animation frame.
    pub anim_frame: u16,
    /// `+0x6A` animation timer.
    pub anim_timer: u16,
}

/// The controller actor's own fields (`a0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReflectController {
    /// `+0x10` flag word - gains [`flags::TEARDOWN`] when either end dies.
    pub flags: u32,
    /// `+0x80` mirror line X (`0` = no X mirror).
    pub mirror_x: i16,
    /// `+0x82` mirror line Z (`0` = no Z mirror).
    pub mirror_z: i16,
    /// `+0x84` active rect minimum tile X.
    pub min_tile_x: i16,
    /// `+0x86` active rect minimum tile Z.
    pub min_tile_z: i16,
    /// `+0x88` active rect maximum tile X.
    pub max_tile_x: i16,
    /// `+0x8A` active rect maximum tile Z.
    pub max_tile_z: i16,
}

/// What one call did, so a caller (or a test) can tell the arms apart without
/// diffing whole actors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectOutcome {
    /// Either end carried [`flags::TEARDOWN`]; the controller set it on
    /// itself and returned.
    TornDown,
    /// The source carried [`flags::SUPPRESS`]; nothing was written.
    Suppressed,
    /// The source stood outside the controller's tile rect.
    OutOfRect,
    /// The destination was updated.
    Reflected,
}

/// World-to-tile conversion, exactly as retail: `(v + 0x40) >> 7`. The `+0x40`
/// is a half-tile bias, and the shift is arithmetic, so negative coordinates
/// floor rather than truncate.
pub fn tile_of(v: i16) -> i16 {
    ((v as i32 + 0x40) >> 7) as i16
}

/// Run one tick of the reflection callback.
///
/// PORT: FUN_801e5154
// NOT WIRED: the actor-callback descriptor at 0x801F2950 is not parsed and
// `engine-core`'s actor carries none of the fields this reads; wiring it
// would edit `engine-core/src/world/**`, owned elsewhere.
pub fn tick_reflection(
    ctrl: &mut ReflectController,
    dst: &mut ReflectActor,
    src: &ReflectActor,
) -> ReflectOutcome {
    if dst.flags & flags::TEARDOWN != 0 || src.flags & flags::TEARDOWN != 0 {
        ctrl.flags |= flags::TEARDOWN;
        return ReflectOutcome::TornDown;
    }
    if src.flags & flags::SUPPRESS != 0 {
        return ReflectOutcome::Suppressed;
    }

    let tx = tile_of(src.x);
    let tz = tile_of(src.z);
    if tx < ctrl.min_tile_x || tz < ctrl.min_tile_z || ctrl.max_tile_x < tx || ctrl.max_tile_z < tz
    {
        return ReflectOutcome::OutOfRect;
    }

    if dst.anim_set == src.anim_set {
        dst.flags = (dst.flags & !flags::ANIM_MIRROR) | (src.flags & flags::ANIM_MIRROR);
        dst.anim_cursor = src.anim_cursor;
        dst.anim_timer = src.anim_timer;
        dst.anim_frame = src.anim_frame;
    }

    let wrap = |v: i32| v as i16;
    match (ctrl.mirror_x, ctrl.mirror_z) {
        (0, zz) => {
            dst.x = src.x;
            dst.y = src.y;
            dst.z = wrap(2 * zz as i32 - src.z as i32);
            dst.facing = wrap(-(HALF_TURN as i32) - src.facing as i32);
        }
        (xx, 0) => {
            dst.x = wrap(2 * xx as i32 - src.x as i32);
            dst.y = src.y;
            dst.z = src.z;
            dst.facing = wrap(-(src.facing as i32));
        }
        (xx, zz) => {
            dst.x = wrap(2 * xx as i32 - src.x as i32);
            dst.y = src.y;
            dst.z = wrap(2 * zz as i32 - src.z as i32);
            dst.facing = wrap(src.facing as i32 + HALF_TURN as i32);
        }
    }
    ReflectOutcome::Reflected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> ReflectController {
        ReflectController {
            flags: 0,
            mirror_x: 0,
            mirror_z: 0,
            min_tile_x: -100,
            min_tile_z: -100,
            max_tile_x: 100,
            max_tile_z: 100,
        }
    }

    fn src() -> ReflectActor {
        ReflectActor {
            x: 300,
            y: -40,
            z: 500,
            facing: 0x200,
            anim_cursor: 7,
            anim_set: 3,
            anim_frame: 11,
            anim_timer: 13,
            ..Default::default()
        }
    }

    #[test]
    fn teardown_propagates_from_either_end() {
        for which in 0..2 {
            let mut c = ctrl();
            let mut d = ReflectActor::default();
            let mut s = src();
            if which == 0 {
                d.flags |= flags::TEARDOWN;
            } else {
                s.flags |= flags::TEARDOWN;
            }
            assert_eq!(
                tick_reflection(&mut c, &mut d, &s),
                ReflectOutcome::TornDown
            );
            assert_ne!(c.flags & flags::TEARDOWN, 0);
        }
    }

    #[test]
    fn suppress_bit_freezes_the_reflection() {
        let mut c = ctrl();
        let mut d = ReflectActor::default();
        let mut s = src();
        s.flags |= flags::SUPPRESS;
        assert_eq!(
            tick_reflection(&mut c, &mut d, &s),
            ReflectOutcome::Suppressed
        );
        assert_eq!(d, ReflectActor::default());
    }

    #[test]
    fn rect_is_in_tile_space_with_the_half_tile_bias() {
        let mut c = ctrl();
        c.min_tile_x = 2;
        c.max_tile_x = 2;
        c.min_tile_z = 0;
        c.max_tile_z = 0;
        let mut d = ReflectActor::default();
        let mut s = ReflectActor { z: 0, ..src() };
        // 0xC0 -> (0xC0 + 0x40) >> 7 == 2, inside; 0x140 -> 3, outside.
        s.x = 0xC0;
        assert_eq!(
            tick_reflection(&mut c, &mut d, &s),
            ReflectOutcome::Reflected
        );
        s.x = 0x140;
        assert_eq!(
            tick_reflection(&mut c, &mut d, &s),
            ReflectOutcome::OutOfRect
        );
    }

    #[test]
    fn z_mirror_arm_flips_z_and_the_facing() {
        let mut c = ctrl();
        c.mirror_z = 64;
        let mut d = ReflectActor::default();
        let s = ReflectActor {
            x: 10,
            y: -5,
            z: 100,
            facing: 0x200,
            ..src()
        };
        tick_reflection(&mut c, &mut d, &s);
        assert_eq!(d.x, 10);
        assert_eq!(d.y, -5);
        assert_eq!(d.z, 2 * 64 - 100);
        assert_eq!(d.facing as i32 & 0xFFF, (0x800 - 0x200) & 0xFFF);
    }

    #[test]
    fn x_mirror_arm_flips_x_and_negates_the_facing() {
        let mut c = ctrl();
        c.mirror_x = 64;
        let mut d = ReflectActor::default();
        let s = ReflectActor {
            x: 100,
            z: 10,
            facing: 0x200,
            ..src()
        };
        tick_reflection(&mut c, &mut d, &s);
        assert_eq!(d.x, 2 * 64 - 100);
        assert_eq!(d.z, 10);
        assert_eq!(d.facing, -0x200);
    }

    #[test]
    fn point_reflection_arm_is_a_half_turn() {
        let mut c = ctrl();
        c.mirror_x = 64;
        c.mirror_z = 32;
        let mut d = ReflectActor::default();
        let s = ReflectActor {
            x: 100,
            z: 100,
            facing: 0x200,
            ..src()
        };
        tick_reflection(&mut c, &mut d, &s);
        assert_eq!(d.x, 2 * 64 - 100);
        assert_eq!(d.z, 2 * 32 - 100);
        assert_eq!(d.facing, 0x200 + HALF_TURN);
    }

    #[test]
    fn every_arm_is_an_involution_on_position() {
        // Reflecting a reflection returns the original point, which is the
        // property that makes the three arms genuine mirrors.
        for (mx, mz) in [(0i16, 64i16), (64, 0), (64, 32)] {
            let mut c = ctrl();
            c.mirror_x = mx;
            c.mirror_z = mz;
            let s = ReflectActor {
                x: 111,
                z: 222,
                facing: 0x123,
                ..src()
            };
            let mut once = ReflectActor::default();
            tick_reflection(&mut c, &mut once, &s);
            let mut twice = ReflectActor::default();
            let relay = ReflectActor {
                anim_set: s.anim_set,
                ..once
            };
            tick_reflection(&mut c, &mut twice, &relay);
            assert_eq!((twice.x, twice.z), (s.x, s.z), "arm ({mx}, {mz})");
            assert_eq!(
                (twice.facing as i32).rem_euclid(FULL_TURN),
                (s.facing as i32).rem_euclid(FULL_TURN),
                "arm ({mx}, {mz})"
            );
        }
    }

    #[test]
    fn anim_state_copies_only_when_the_selectors_agree() {
        let mut c = ctrl();
        c.mirror_x = 64;
        let s = src();
        let mut same = ReflectActor {
            anim_set: s.anim_set,
            ..Default::default()
        };
        tick_reflection(&mut c, &mut same, &s);
        assert_eq!(
            (same.anim_cursor, same.anim_frame, same.anim_timer),
            (s.anim_cursor, s.anim_frame, s.anim_timer)
        );

        let mut differ = ReflectActor {
            anim_set: s.anim_set + 1,
            ..Default::default()
        };
        tick_reflection(&mut c, &mut differ, &s);
        assert_eq!(
            (differ.anim_cursor, differ.anim_frame, differ.anim_timer),
            (0, 0, 0)
        );
    }
}
