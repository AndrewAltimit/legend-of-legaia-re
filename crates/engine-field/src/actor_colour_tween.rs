//! The field overlay's **actor colour tween** (`FUN_801DDC20`, built by
//! `FUN_801DE2B0`): a delayed, ramped, held colour that an actor pushes to the
//! screen tint each frame.
//!
//! PORT: FUN_801DDC20, FUN_801DE2B0
//!
//! The scene-transition sweep in [`crate::scene_transition_sweep`] retires
//! these actors: its second retire test compares the per-frame handler
//! against `0x801DDC20`, this tween's own entry.
//!
//! Provenance: `overlay_cutscene_dialogue_801ddc20.txt`, cross-checked against
//! the `overlay_cutscene_mapview_*` and `overlay_world_map_*` field captures
//! (identical sizes). The `overlay_0897_801ddc20.txt` dump is a corpus gap - it
//! reports **zero instructions** - so it cannot be used here.
//!
//! [`step_colour_tween`] is **dispatched but not produced** - the frame loop
//! calls it for every [`crate::actor_handler::ActorHandler::ColourTween`]
//! actor, and nothing spawns one yet. Its own note says exactly what is
//! missing.
//!
//! REF: FUN_80024D78, FUN_80024EE4 (the tween's draw)

use crate::scene_transition_sweep::ACTOR_FLAG_YIELD;

/// A colour-tween actor's state, as the tick reads it.
///
/// The three `u16` triples are **colours**, not positions: the tick packs the
/// interpolated result as `r | (g << 8) | (b << 16)` before handing it to the
/// draw leaf, which is what identifies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTween {
    /// `+0xB8`/`+0xBA`/`+0xBC` - the start colour.
    pub from: (u16, u16, u16),
    /// `+0xBE`/`+0xC0`/`+0xC2` - the target colour.
    pub to: (u16, u16, u16),
    /// `+0xC4` - frames of delay before the ramp starts.
    pub delay: i16,
    /// `+0xD4` - ramp length in frames.
    pub duration: i16,
    /// `+0xC6` - frames to hold the target after the ramp. `-1` holds forever.
    pub hold: i16,
    /// `+0xC8` - the tween clock.
    pub clock: u16,
    /// `+0x10` - actor flag word.
    pub flags: u32,
    /// `+0xD6` - the draw's first argument (`FUN_80024EE4`'s `a0`), a
    /// screen-effect **kind** selector.
    pub push_kind: i16,
    /// `+0xD2` - the draw's second argument (`a1`), the blend mode.
    pub push_blend: i16,
}

/// One `FUN_80024EE4(kind, blend, packed_rgb)` full-screen colour push - the
/// exact three-argument triple the tween's draw call carries.
///
/// PORT: FUN_801DDC20 (`0x801dde14..0x801dde1c`, the argument set-up)
///
/// The engine keeps the triple rather than resolving it to a tint factor,
/// because the three arguments are a draw, not a tint: one full-display
/// `POLY_F4` under a `GP0(0xE1)` draw-mode packet. What each selects is read
/// off `FUN_80024EE4` itself, and **`kind` is not a quad selector** - the
/// emitter has exactly one quad, whose corners are the scratchpad display
/// rect (`0x80024F68..0x80024F98`):
///
/// - `a0` clamps to `0` when negative and to `OT_len - 1` above, then names
///   the **ordering-table bucket** the packet links at (`OT + a0*4`, twice,
///   at `0x80024FA4` / `0x80024FDC`).
/// - `a1` becomes `(a1 << 5) | 0xE`, the draw-mode word whose bits 5..6 are
///   the **ABR equation** (`0x80024FB0`).
/// - `a2` is a **GP0 colour word** (red low), masked to 24 bits under the
///   `0x2B` command byte at `0x80024F54`.
///
/// Hosts draw the frame's pushes through
/// `legaia_engine_core::world::World::screen_tint_push_args` and the shared emitter
/// `legaia_render_kernels::screen_prim::screen_effect_push_prims`, which is where
/// the clamp, the mask and the channel swap live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenTintPush {
    /// `a0` - the ordering-table bucket (actor `+0xD6`). Named `kind` for
    /// the spawner argument it comes from; what the emitter does with it is
    /// depth, not style.
    pub kind: i16,
    /// `a1` - the ABR blend equation (actor `+0xD2`).
    pub blend: i16,
    /// `a2` - the packed colour from [`pack_colour`], in GP0 channel order
    /// (red in bits 0..7).
    pub packed: u32,
}

/// Sentinel in [`ColourTween::hold`] meaning "hold indefinitely" - the tween
/// never retires itself.
pub const TWEEN_HOLD_FOREVER: i16 = -1;

/// One frame of the colour tween.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTweenStep {
    /// The clock after this frame.
    pub clock: u16,
    /// Colour to draw, packed `r | (g << 8) | (b << 16)`.
    pub packed: u32,
    /// Actor flags after this frame - gains [`ACTOR_FLAG_YIELD`] when the hold
    /// expires.
    pub flags: u32,
    /// `false` once the yield bit is set: retail skips the draw entirely on
    /// that frame and every frame after.
    pub draws: bool,
    /// The frame's `FUN_80024EE4` push - `Some` exactly when [`Self::draws`].
    pub push: Option<ScreenTintPush>,
}

/// Build a colour tween from the 13-`i16` fade template.
///
/// PORT: FUN_801DE2B0 (`0x801de2b0..0x801de378`)
///
/// The spawner: allocate from descriptor `0x801F2888` (whose `+0x8` handler
/// word is `0x801DDC20`, i.e. [`step_colour_tween`] - field `0x024078` of the
/// extracted `overlay_field_0897.bin` at base `0x801CE818`), clear the clock,
/// then copy ten halfwords out of the caller's block.
///
/// **The block is [`legaia_engine_system::fade::FadeTemplate`]**, not a private layout, and
/// that is the useful finding: the field VM builds one block on its stack and
/// hands it to *either* `FUN_80024E80` (the fade-actor spawn, taken when
/// `_DAT_1F800394 & 0x800000` is set) or this function (the default arm). Both
/// readings of the same 13 halfwords agree field for field - kind `[0]`,
/// duration `[1]`, start RGB `[3..=5]`, end RGB `[7..=9]` - which is two
/// independent decodes corroborating one layout. It also names two words
/// `fade.rs` records as unpinned: template `[10]` is the tween's **delay** and
/// `[11]` its **hold**. `[12]` is the one word this arm does not read.
///
/// `kind` is the spawner's second argument (`a1`, `_DAT_8007BCCC` at both call
/// sites), landing at `+0xD6` as the draw's screen-effect selector - it is
/// *not* template `[0]`, which lands at `+0xD2` as the blend.
///
/// Live on both hosts. The field VM's op `0x34` sub-0 arm - retail's
/// `FUN_801DE840` at `0x801DFD68` and `0x801DFEE8`, the
/// `_DAT_1F800394 & 0x800000` default side of the fork - reaches
/// `World::set_effect_colour`, which builds the two
/// [`legaia_engine_system::fade::FadeTemplate`]s that arm reads off its stack and spawns
/// each through `World::spawn_colour_tween`. The per-frame push is then the
/// only representation of the effect; the float ramp that used to stand
/// beside it, with nothing reading either, is gone.
///
/// The arm is a **pair**, and the port builds both halves: a walk-out from
/// the previous target down to black with a one-frame hold, then a walk-in
/// from black up to the operand RGB with a `-1` hold. Its two selectors come
/// from the sub-op byte - blend `(op0 & 1) != 0 ? 2 : 1`, and kind `8` when
/// `op0 & 2`, else `0` when `op0 & 4`, else `2`.
///
/// [`ActorHandler::ColourTween`]: crate::actor_handler::ActorHandler::ColourTween
/// [`World::screen_tint_pushes`]: legaia_engine_core::world::World::screen_tint_pushes
pub fn tween_from_fade_template(
    t: &legaia_engine_system::fade::FadeTemplate,
    kind: i16,
) -> ColourTween {
    let c = |v: [i16; 3]| (v[0] as u16, v[1] as u16, v[2] as u16);
    ColourTween {
        from: c(t.start_rgb),
        to: c(t.end_rgb),
        delay: t.mode[0],
        duration: t.duration,
        hold: t.mode[1],
        // Retail's `sh zero,0xc8(v1)` - a fresh tween always starts at 0.
        clock: 0,
        flags: 0,
        push_kind: kind,
        push_blend: t.kind,
    }
}

/// Advance a colour tween by one frame.
///
/// PORT: FUN_801DDC20 (`0x801ddc20..0x801dde30`).
///
/// `delta` is the per-frame tick `_DAT_1F800393` (the same byte the ribbon
/// emitter and the dev warp applier step by).
///
/// Three phases, selected on the clock:
///
/// - **before `delay`**: the colour stays at `from` and only the clock advances.
/// - **inside the ramp**: each channel is `from + (to - from) * (clock - delay)
///   / duration`, computed per channel with a signed divide. The stored `from`
///   is never written back, so the interpolation is recomputed from the
///   original endpoints every frame rather than accumulating drift.
/// - **after `delay + duration`**: the colour snaps to `to`. If `hold` is
///   [`TWEEN_HOLD_FOREVER`] the tween sits there; otherwise the clock keeps
///   running and once it passes `delay + duration + hold` the actor takes
///   [`ACTOR_FLAG_YIELD`].
///
/// The draw is skipped whenever the yield bit is already set, so the frame that
/// retires the tween is also the first frame it does not draw.
///
/// Live and entered. It deliberately carries no inert-port disclosure: the
/// call chain is real and production-only, so that token would be false and
/// the audit would read it as a stale disclosure. `World::tick` →
/// [`World::tick_handler_actors`] dispatches this once per game tick for every
/// pool actor carrying [`crate::actor_handler::ActorHandler::ColourTween`],
/// with the same `frame_delta` (retail `DAT_1F800393`) the rest of the pool
/// advances on; the frame's [`ScreenTintPush`] is stored back on the actor for
/// [`World::screen_tint_pushes`], and an expired hold takes the yield bit and
/// is dropped by the same retire pass the transition sweep's victims go
/// through.
///
/// The producer is the field VM's screen-effect arm
/// (`FUN_801DE840` at `0x801DFD68`/`0x801DFEE8` → `FUN_801DE2B0`, ported as
/// [`tween_from_fade_template`]), which reaches
/// `World::set_effect_colour` on both hosts and seats its
/// tweens through `World::spawn_colour_tween`.
///
/// [`World::tick_handler_actors`]: legaia_engine_core::world::World::tick_handler_actors
/// [`World::screen_tint_pushes`]: legaia_engine_core::world::World::screen_tint_pushes
pub fn step_colour_tween(t: ColourTween, delta: u8) -> ColourTweenStep {
    let clock = i32::from(t.clock as i16);
    let delay = i32::from(t.delay);
    let dur = i32::from(t.duration);
    let ramp_end = delay + dur;
    let advance = |c: u16| c.wrapping_add(u16::from(delta));

    let (colour, clock_out, mut flags) = if clock < ramp_end {
        if delay >= clock {
            // Not started: hold the start colour, run the clock.
            (t.from, advance(t.clock), t.flags)
        } else {
            let progress = i32::from(t.clock) - i32::from(t.delay);
            let lerp = |from: u16, to: u16| -> u16 {
                if dur == 0 {
                    return from;
                }
                let d = i32::from(to as i16) - i32::from(from as i16);
                from.wrapping_add((d * progress / dur) as u16)
            };
            (
                (
                    lerp(t.from.0, t.to.0),
                    lerp(t.from.1, t.to.1),
                    lerp(t.from.2, t.to.2),
                ),
                advance(t.clock),
                t.flags,
            )
        }
    } else {
        // Past the ramp: snap to the target.
        if t.hold == TWEEN_HOLD_FOREVER {
            (t.to, t.clock, t.flags)
        } else {
            let next = advance(t.clock);
            let done = i32::from(next as i16) >= ramp_end + i32::from(t.hold);
            (
                t.to,
                next,
                if done {
                    t.flags | ACTOR_FLAG_YIELD
                } else {
                    t.flags
                },
            )
        }
    };
    let draws = flags & ACTOR_FLAG_YIELD == 0;
    if !draws {
        flags |= ACTOR_FLAG_YIELD;
    }
    let packed = pack_colour(colour);
    ColourTweenStep {
        clock: clock_out,
        packed,
        flags,
        draws,
        push: draws.then_some(ScreenTintPush {
            kind: t.push_kind,
            blend: t.push_blend,
            packed,
        }),
    }
}

/// Pack the tween's three channels the way the tick hands them to the draw.
///
/// PORT: FUN_801DDC20 (`0x801dde00..0x801dde20`).
///
/// Retail **adds** the three terms rather than OR-ing them, and sign-extends
/// the low two: `(i16)r + ((i16)g << 8) + ((b & 0xFFFF) << 16)`. The green term
/// is built as `(g << 16) >> 8` with an *arithmetic* shift, which is what
/// sign-extends it.
///
/// For in-range channels (`0..=0xFF`) this is identical to the obvious
/// `r | (g << 8) | (b << 16)`. It stops being identical the moment a channel
/// runs out of range - which the interpolation can do, because the per-channel
/// lerp is a signed divide with no clamp. An overshooting green then **carries
/// into blue** rather than being masked off. Keeping the add reproduces that;
/// masking would quietly diverge exactly where the tween is most extreme.
///
/// Live through [`step_colour_tween`]'s own chain: the packed word becomes the
/// frame's [`ScreenTintPush::packed`].
pub fn pack_colour((r, g, b): (u16, u16, u16)) -> u32 {
    i32::from(r as i16)
        .wrapping_add(i32::from(g as i16) << 8)
        .wrapping_add((u32::from(b) << 16) as i32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tween() -> ColourTween {
        ColourTween {
            from: (0, 0, 0),
            to: (0x80, 0x40, 0x20),
            delay: 10,
            duration: 100,
            hold: 30,
            clock: 0,
            flags: 0,
            push_kind: 1,
            push_blend: 2,
        }
    }

    #[test]
    fn the_frames_push_carries_the_actors_own_kind_and_blend() {
        let mut t = tween();
        t.push_kind = 3;
        t.push_blend = 1;
        // Inside the hold window (ramp ends at 110, hold expires at 140), so
        // the frame still draws. Past 140 it would retire and push nothing -
        // which is the second half of this test.
        t.clock = 120;
        let s = step_colour_tween(t, 1);
        assert_eq!(
            s.push,
            Some(ScreenTintPush {
                kind: 3,
                blend: 1,
                packed: s.packed,
            })
        );
        // A retiring frame does not draw, so it pushes nothing.
        let mut t = tween();
        t.clock = 139;
        assert_eq!(step_colour_tween(t, 4).push, None);
    }

    #[test]
    fn before_the_delay_the_colour_holds_and_the_clock_runs() {
        let s = step_colour_tween(tween(), 4);
        assert_eq!(s.packed, 0);
        assert_eq!(s.clock, 4);
        assert!(s.draws);
    }

    #[test]
    fn midway_through_the_ramp_each_channel_is_half_interpolated() {
        let mut t = tween();
        t.clock = 60; // delay 10 + half of duration 100
        let s = step_colour_tween(t, 1);
        // progress = 50/100, so each channel is halfway.
        assert_eq!(s.packed & 0xFF, 0x40);
        assert_eq!((s.packed >> 8) & 0xFF, 0x20);
        assert_eq!((s.packed >> 16) & 0xFF, 0x10);
    }

    #[test]
    fn past_the_ramp_the_colour_snaps_to_the_target() {
        let mut t = tween();
        t.clock = 200;
        let s = step_colour_tween(t, 1);
        assert_eq!(s.packed, 0x80 | (0x40 << 8) | (0x20 << 16));
    }

    #[test]
    fn the_hold_expiry_sets_the_yield_bit_and_stops_the_draw() {
        let mut t = tween();
        // delay 10 + duration 100 + hold 30 = 140.
        t.clock = 139;
        let s = step_colour_tween(t, 4);
        assert_eq!(s.flags & ACTOR_FLAG_YIELD, ACTOR_FLAG_YIELD);
        assert!(!s.draws, "the retiring frame does not draw");
    }

    #[test]
    fn hold_forever_never_retires_and_freezes_the_clock() {
        let mut t = tween();
        t.hold = TWEEN_HOLD_FOREVER;
        t.clock = 500;
        let s = step_colour_tween(t, 4);
        assert_eq!(s.flags & ACTOR_FLAG_YIELD, 0);
        assert_eq!(s.clock, 500, "the clock stops once the target is reached");
        assert!(s.draws);
    }

    #[test]
    fn packing_matches_the_obvious_form_only_while_the_channels_are_in_range() {
        // In range: the add and the mask-or agree.
        for c in [(0u16, 0u16, 0u16), (0xFF, 0x7F, 0x01), (0x12, 0x34, 0x56)] {
            let ored = u32::from(c.0) | (u32::from(c.1) << 8) | (u32::from(c.2) << 16);
            assert_eq!(pack_colour(c), ored, "{c:?}");
        }
        // Out of range: retail's add carries between channels where a mask
        // would not. Green 0x100 lands entirely in the blue byte.
        assert_eq!(pack_colour((0, 0x100, 0)), 0x0001_0000);
        // And a negative low channel borrows downward.
        assert_eq!(pack_colour((0xFFFF, 0, 0)), 0xFFFF_FFFF);
    }

    #[test]
    fn a_zero_duration_ramp_does_not_divide_by_zero() {
        let mut t = tween();
        t.duration = 0;
        t.clock = 20;
        // delay 10 + duration 0 = 10, and clock 20 is past it, so this takes
        // the snap arm rather than the divide.
        let s = step_colour_tween(t, 1);
        assert_eq!(s.packed, 0x80 | (0x40 << 8) | (0x20 << 16));
    }
}
