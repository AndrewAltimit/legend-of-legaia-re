//! The field overlay's **attached-sprite tick** - the per-frame body of an
//! actor spawned by the arc-hop family
//! ([`crate::field_ledge_hop_arc`]), which rides a parent actor and draws a
//! screen-space billboard at the parent's position.
//!
//! REF: FUN_800172c0, FUN_800195a8, FUN_801e3984, FUN_801e3e00
//!
//! The port of `FUN_801e4470` is [`attached_sprite_tick`], which carries the
//! `PORT` tag; it is not repeated at module level.
//!
//! # Provenance
//!
//! `FUN_801e4470` lives in the field overlay (PROT entry `0897_xxx_dat`,
//! slot-A base `0x801CE818`, file offset `0x15C58`): 83 instructions opening
//! `addiu sp, sp, -0x48` and closing `jr ra / addiu sp, sp, 0x48` at
//! `0x801E45B4`. Its own `overlay_0897_801e4470.txt` dump reports `size=1
//! bytes, 0 instructions` - the catalogued "no disassembly" artifact - so the
//! read here is off the extracted image, corroborated by
//! `ghidra/scripts/funcs/overlay_cutscene_dialogue_801e4470.txt`.
//!
//! Every VA-alias sibling that covers this address (`baka_fighter`, `dance`,
//! `debug_menu`, `fishing`, `slot_machine`) turns out not to: an image scan
//! finds a stack-frame prologue at `0x801E4470` in the **field** overlay
//! alone, and no other image holds a `jal` to it.
//!
//! # Shape
//!
//! ```text
//! parent = actor[+0x90]                     ; null -> nothing to ride
//! if parent[+0x10] & 8   -> actor[+0x10] |= 8 ; parent torn down, follow it
//! if parent[+0x10] & 2   -> return             ; parent hidden, draw nothing
//! pos = parent[+0x14..+0x1A] + actor[+0x14..+0x1A]
//! if actor[+0x94] -> FUN_801e3e00(actor)    ; the per-tick hook
//! FUN_800172c0()                            ; scene camera for this frame
//! FUN_800195a8(&pos, actor[+0x3C], actor[+0x3E], 0, &p0, &p1, &p2, &p3)
//! FUN_801e3984(&rect, actor[+0x74], actor[+0x88], actor[+0x5A])
//! ```
//!
//! The two flag tests are **not** the same shape. `8` propagates - the sprite
//! tears itself down with its parent - while `2` is a plain early return that
//! leaves the sprite alive and simply skips a frame's draw. A port that folds
//! them into one "parent gone" branch loses the difference between a hidden
//! parent and a dead one.

/// Parent-actor flag bits `FUN_801e4470` tests in `+0x10`.
pub mod parent_flag {
    /// Tear-down. Propagates into the sprite's own `+0x10`.
    pub const TEARDOWN: u32 = 8;
    /// Hidden. Skips this frame's draw and nothing else.
    pub const HIDDEN: u32 = 2;
}

/// The four screen points `FUN_800195a8` writes back, in the order retail
/// passes their addresses (`sp+0x30`, `sp+0x34`, `sp+0x38`, `sp+0x3C`).
///
/// Only the first and the last are read afterwards; the middle pair is
/// written and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectedQuad {
    /// `sp+0x30` - the first corner.
    pub p0: (i16, i16),
    /// `sp+0x34`.
    pub p1: (i16, i16),
    /// `sp+0x38`.
    pub p2: (i16, i16),
    /// `sp+0x3C` - the opposite corner.
    pub p3: (i16, i16),
}

/// The centre-plus-span rect retail assembles at `sp+0x28..sp+0x30` and hands
/// to the sprite emitter `FUN_801e3984`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteRect {
    /// `sp+0x28` - `(p0.x + p3.x) >> 1`.
    pub centre_x: i16,
    /// `sp+0x2A` - `(p0.y + p3.y) >> 1`.
    pub centre_y: i16,
    /// `sp+0x2C` - `p3.x - p0.x`.
    pub width: i16,
    /// `sp+0x2E` - `p3.y - p0.y`.
    pub height: i16,
}

/// What one tick decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteTick {
    /// `+0x90` was null - retail falls straight through to the epilogue.
    NoParent,
    /// The parent carries `+0x10 & 8`; the sprite sets the same bit on itself
    /// and draws nothing.
    TearDown,
    /// The parent carries `+0x10 & 2`; nothing at all happens this frame.
    Hidden,
    /// The draw ran.
    Draw {
        /// The world point the billboard is projected from.
        world: (i16, i16, i16),
        /// `true` when `+0x94` was non-null and retail called the per-tick
        /// hook `FUN_801e3e00` before projecting.
        ran_hook: bool,
    },
}

/// Fold the parent's transform into the sprite's own offset, exactly as
/// retail does with three `lhu`/`lhu`/`addu`/`sh` triples - i.e. modulo
/// `2^16`, never saturating.
fn offset_world(parent: (i16, i16, i16), local: (i16, i16, i16)) -> (i16, i16, i16) {
    let add = |a: i16, b: i16| (a as u16).wrapping_add(b as u16) as i16;
    (
        add(parent.0, local.0),
        add(parent.1, local.1),
        add(parent.2, local.2),
    )
}

/// Reduce the projected quad to the emitter's rect.
///
/// Retail mixes signed and unsigned half-word reads here - `lh` for the two
/// values that go into a `sra`-halved sum, `lhu` for the two that go into a
/// `subu`. The results are stored back through `sh`, so the span pair is the
/// same 16 bits either way; the port uses wrapping arithmetic rather than
/// picking one signedness and pretending the other is not there.
///
/// PORT: FUN_801e4470 (`0x801E4550..0x801E4594`)
// NOT WIRED: same blocker as [`attached_sprite_tick`], which is its only
// caller - there is no attached-sprite actor class in `engine-core` and no
// `engine-vm`-visible billboard projection to feed it.
pub fn sprite_rect(quad: &ProjectedQuad) -> SpriteRect {
    let (x0, y0) = quad.p0;
    let (x3, y3) = quad.p3;
    SpriteRect {
        centre_x: ((x0 as i32 + x3 as i32) >> 1) as i16,
        centre_y: ((y0 as i32 + y3 as i32) >> 1) as i16,
        width: (x3 as u16).wrapping_sub(x0 as u16) as i16,
        height: (y3 as u16).wrapping_sub(y0 as u16) as i16,
    }
}

/// One tick of an attached sprite.
///
/// `parent` is `None` when `+0x90` is null. `project` stands in for the
/// `FUN_800172c0` + `FUN_800195a8` pair: given the folded world point and the
/// sprite's `+0x3C` / `+0x3E` extents it yields the four screen corners.
/// `emit` is `FUN_801e3984`.
///
/// PORT: FUN_801e4470
// NOT WIRED: the emitter `FUN_801e3984` and the billboard projection
// `FUN_800195a8` live behind `engine-render` (`billboard::project_billboard`),
// which `engine-vm` cannot depend on, and the field host has no attached-sprite
// actor class to tick. The missing host is an `engine-core` actor kind whose
// `+0x90` back-link the arc-hop spawners would fill in.
pub fn attached_sprite_tick<P, E>(
    parent: Option<(u32, (i16, i16, i16))>,
    local_offset: (i16, i16, i16),
    extents: (i16, i16),
    has_tick_hook: bool,
    project: P,
    emit: E,
) -> SpriteTick
where
    P: FnOnce((i16, i16, i16), (i16, i16)) -> ProjectedQuad,
    E: FnOnce(SpriteRect),
{
    let Some((parent_flags, parent_pos)) = parent else {
        return SpriteTick::NoParent;
    };
    if parent_flags & parent_flag::TEARDOWN != 0 {
        return SpriteTick::TearDown;
    }
    if parent_flags & parent_flag::HIDDEN != 0 {
        return SpriteTick::Hidden;
    }
    let world = offset_world(parent_pos, local_offset);
    // Retail folds the position *before* the hook and the camera call, so the
    // hook sees the frame's world point already committed to the stack.
    let quad = project(world, extents);
    emit(sprite_rect(&quad));
    SpriteTick::Draw {
        world,
        ran_hook: has_tick_hook,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn quad(p0: (i16, i16), p3: (i16, i16)) -> ProjectedQuad {
        ProjectedQuad {
            p0,
            p1: (0, 0),
            p2: (0, 0),
            p3,
        }
    }

    #[test]
    fn null_parent_is_a_plain_return() {
        let r = attached_sprite_tick(
            None,
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(r, SpriteTick::NoParent);
    }

    #[test]
    fn teardown_and_hidden_are_different_branches() {
        let td = attached_sprite_tick(
            Some((parent_flag::TEARDOWN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(td, SpriteTick::TearDown);
        // `8` wins over `2`: retail tests it first.
        let both = attached_sprite_tick(
            Some((parent_flag::TEARDOWN | parent_flag::HIDDEN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(both, SpriteTick::TearDown);
        let hidden = attached_sprite_tick(
            Some((parent_flag::HIDDEN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(hidden, SpriteTick::Hidden);
    }

    #[test]
    fn draw_folds_the_parent_transform_and_emits_once() {
        let seen = RefCell::new(Vec::new());
        let r = attached_sprite_tick(
            Some((0, (100, 200, 300))),
            (-10, 5, 20),
            (16, 32),
            true,
            |world, ext| {
                assert_eq!(world, (90, 205, 320));
                assert_eq!(ext, (16, 32));
                quad((10, 20), (50, 80))
            },
            |rect| seen.borrow_mut().push(rect),
        );
        assert_eq!(
            r,
            SpriteTick::Draw {
                world: (90, 205, 320),
                ran_hook: true
            }
        );
        assert_eq!(
            seen.into_inner(),
            vec![SpriteRect {
                centre_x: 30,
                centre_y: 50,
                width: 40,
                height: 60,
            }]
        );
    }

    #[test]
    fn centres_floor_rather_than_truncate() {
        // `addu` then `sra 1`: -3 halves to -2, not -1.
        let r = sprite_rect(&quad((-3, -3), (0, 0)));
        assert_eq!((r.centre_x, r.centre_y), (-2, -2));
    }

    #[test]
    fn spans_wrap_like_the_halfword_store() {
        let r = sprite_rect(&quad((0x4000, 0), (-0x4000, 0)));
        assert_eq!(r.width, -0x8000);
    }
}
