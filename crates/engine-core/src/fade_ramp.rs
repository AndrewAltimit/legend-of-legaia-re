//! The SCUS fade actor's per-frame half - clean-room port of the ramp step
//! `FUN_80020C14` and the tick `FUN_80025000` that pushes its result to the
//! full-screen quad emitter.
//!
//! [`crate::fade`] ports the other half: the spawn `FUN_80024E80` and the
//! template loader `FUN_80020B00`, which fill the actor's `+0x7C` block. This
//! module is what runs on that block every frame. The two are deliberately
//! separate types - [`crate::fade::FadeState`] is the host-driven model the
//! engine's own fades use, with a linear per-frame lerp and a latch at the end;
//! [`FadeRamp`] is the retail arithmetic, byte for byte.
//!
//! ## The block
//!
//! Reading `FUN_80020B00`'s stores against `FUN_80020C14`'s loads pins every
//! field of the `+0x7C` block, including the three template words the loader
//! copies verbatim:
//!
//! ```text
//! +0x00/02/04  current RGB, 10.6 fixed      (template [3..=5] << 6)
//! +0x08/0A/0C  target  RGB, 10.6 fixed      (template [7..=9] << 6)
//! +0x10/12/14  per-frame delta, 10.6 fixed  ((end - start) << 6) / duration
//! +0x18        fade kind, word              (template [0])
//! +0x1C        start delay, vsyncs          (template [10])
//! +0x1E        hold after the ramp          (template [11]); -1 = no hold
//! +0x20        duration, vsyncs             (template [1])
//! +0x22        id stamped by FUN_80024E80   (template [12])
//! ```
//!
//! ## The step
//!
//! Every countdown decrements by `DAT_1F800393`, the scratchpad vsync delta, so
//! the ramp is cadence-invariant. Unlike the overlay sibling `FUN_801DDC20`
//! (ported as `crate::field_actor_kernels::step_colour_tween`), which lerps off
//! the install-time endpoints each frame, this one **accumulates** and clamps:
//!
//! * while the start delay is still positive nothing is drawn;
//! * the duration counts down, and going negative raises [`RampFlags::ramped`];
//! * the hold then counts down, and expiring raises [`RampFlags::finished`],
//!   which is the actor-list "done" bit `actor[+0x10] |= 8`;
//! * each channel does `current += delta * dt`, clamps onto the target when the
//!   delta's sign says it overshot, and clamps into `[0, 0x3FC0]` either way;
//! * the packed result is `R | G << 8 | B << 16` with each channel `>> 6`.
//!
//! The clamp arm is chosen by the **delta's sign**, not by the direction the
//! channel actually has to travel: a non-negative delta clamps when `current`
//! rises past the target, a negative one clamps when `current` falls below it.
//! So a delta whose sign disagrees with `target - current` snaps onto the
//! target on its very first step rather than ramping. That is retail
//! behaviour, not a port simplification.
//!
//! See `docs/reference/functions/renderer.md` and
//! `ghidra/scripts/funcs/80020c14.txt`, `80025000.txt`, `80020b00.txt`.
//!
//! NOT WIRED - and **not** for want of the actor pool. That is the reason this
//! tag used to give ("wiring it means the same thing wiring
//! [`crate::fade::spawn_fade`] does"), and it names a prerequisite this module
//! does not have. [`FadeRamp`] *is* the `+0x7C` block; one world field can hold
//! it exactly as well as a pool entry can, and its per-frame input is a vsync
//! delta, which [`crate::world::World::frame_step`] already carries live. The
//! pool is what `spawn_fade` needs in order to have *several* fades at once,
//! which is a different question.
//!
//! What blocks it is that the engine's one live fade already has a model.
//! [`crate::world::World::screen_fade`] is an `Option<`[`crate::fade::FadeState`]`>`,
//! staged by the battle-escape teardown and stepped once per frame by the world
//! tick, which **drops it when `step()` reports the ramp complete**. The retail
//! ramp has no such report on this template: the escape template's hold word is
//! `-1`, so [`RampFlags::finished`] never rises and the white-out holds white
//! until the battle unloads. Substituting one for the other therefore moves the
//! fade's lifetime out of the world tick and onto the teardown, which is a
//! change to when the screen clears rather than a call insertion - so the
//! substitution lands with the battle-teardown owner, not here.
//!
//! Until then this module is the retail reference the host model is checked
//! against, not the thing driving the screen.

// REF: FUN_80020B00 - the template loader whose stores name this block's
// fields; ported in `crate::fade` as `FadeState::load`.
// REF: FUN_80024E80 - the spawn that stamps the `+0x22` id.
// REF: FUN_80024EE4 - the GP0 quad emitter the tick's result is handed to; the
// engine draws the overlay from its own draw list, so the port stops at the
// descriptor.
// REF: FUN_801DDC20 - the overlay-resident sibling ramp (a lerp, not an
// accumulator), ported as `crate::field_actor_kernels::step_colour_tween`.

/// The two latched outcomes the step raises on the owning actor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RampFlags {
    /// Duration expired - retail sets `actor[+0x62] |= 0x100` and keeps
    /// drawing.
    pub ramped: bool,
    /// Hold expired - retail sets `actor[+0x10] |= 8`, the actor-list
    /// "finished" bit, and stops drawing.
    pub finished: bool,
}

/// Live `+0x7C` fade block, in the retail units.
///
/// Field names follow the block layout in the module docs; every value is the
/// halfword retail stores, so `current` / `target` / `delta` are 10.6 fixed
/// point and the three counters are in vsyncs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeRamp {
    /// `+0x00/02/04` - current RGB, 10.6 fixed.
    pub current_q6: [i16; 3],
    /// `+0x08/0A/0C` - target RGB, 10.6 fixed.
    pub target_q6: [i16; 3],
    /// `+0x10/12/14` - per-frame delta, 10.6 fixed, signed.
    pub delta_q6: [i16; 3],
    /// `+0x18` - fade kind; `FUN_80025000` passes it as the emitter's second
    /// argument.
    pub kind: i32,
    /// `+0x1C` - vsyncs to wait before the ramp starts.
    pub delay: i16,
    /// `+0x1E` - vsyncs to hold after the ramp; `-1` disables the hold, so the
    /// actor never raises [`RampFlags::finished`].
    pub hold: i16,
    /// `+0x20` - ramp duration in vsyncs.
    pub duration: i16,
    /// `+0x22` - the id `FUN_80024E80` stamps into the template's last word;
    /// `FUN_80025000` passes it as the emitter's first argument.
    pub id: i16,
    /// Sticky copy of the flags the steps have raised.
    pub flags: RampFlags,
}

/// The ceiling every channel is clamped to: `0xFF << 6`.
pub const CHANNEL_MAX_Q6: i16 = 0x3FC0;

impl FadeRamp {
    /// Build a ramp from the block fields, with both flags clear.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        current_q6: [i16; 3],
        target_q6: [i16; 3],
        delta_q6: [i16; 3],
        kind: i32,
        delay: i16,
        hold: i16,
        duration: i16,
        id: i16,
    ) -> Self {
        FadeRamp {
            current_q6,
            target_q6,
            delta_q6,
            kind,
            delay,
            hold,
            duration,
            id,
            flags: RampFlags::default(),
        }
    }

    /// Advance the ramp by `dt` vsyncs (retail reads the scratchpad byte at
    /// `0x1F800393`), returning the packed `R | G << 8 | B << 16` colour to
    /// draw, or `None` on the two retail `-1` returns: still inside the start
    /// delay, and hold expired.
    ///
    /// PORT: FUN_80020C14
    pub fn advance_fade_vsyncs(&mut self, dt: u8) -> Option<u32> {
        let dt = dt as i16;

        // 0x80020C20..0x80020C54 - start delay. Only counted down while it is
        // still positive; a still-positive result aborts the frame.
        if self.delay > 0 {
            self.delay = self.delay.wrapping_sub(dt);
            if self.delay > 0 {
                return None;
            }
        }

        // 0x80020C5C..0x80020CD0 - duration, then the hold behind it. The
        // duration is counted down unconditionally and wraps like the retail
        // halfword store does.
        self.duration = self.duration.wrapping_sub(dt);
        if self.duration < 0 {
            self.flags.ramped = true;
            if self.hold >= 0 {
                self.hold = self.hold.wrapping_sub(dt);
                if self.hold <= 0 {
                    self.flags.finished = true;
                    return None;
                }
            }
        }

        // 0x80020CE8..0x80020D90 - the three channels.
        for c in 0..3 {
            let mut v = self.current_q6[c].wrapping_add(self.delta_q6[c].wrapping_mul(dt));
            if self.delta_q6[c] >= 0 {
                if v > self.target_q6[c] {
                    v = self.target_q6[c];
                }
            } else if v < self.target_q6[c] {
                v = self.target_q6[c];
            }
            // Retail runs these as two independent tests in this order
            // (0x80020D54..0x80020D7C); the combined form is equivalent here
            // because 0 < CHANNEL_MAX_Q6.
            self.current_q6[c] = v.clamp(0, CHANNEL_MAX_Q6);
        }

        Some(self.packed_fade_rgb())
    }

    /// `R | G << 8 | B << 16`, each channel `>> 6` with the retail
    /// round-toward-zero fixup (`+0x3F` before an arithmetic shift when the
    /// value is negative). The clamp in [`Self::advance_fade_vsyncs`] means the fixup cannot
    /// fire in practice; it is kept because the instruction stream has it.
    ///
    /// PORT: FUN_80020C14 (`0x80020d94..0x80020ddc`)
    pub fn packed_fade_rgb(&self) -> u32 {
        let ch = |v: i16| -> u32 {
            let v = if v < 0 { v.wrapping_add(0x3F) } else { v };
            ((v >> 6) as u32) & 0xFF
        };
        ch(self.current_q6[0]) | (ch(self.current_q6[1]) << 8) | (ch(self.current_q6[2]) << 16)
    }
}

/// What one tick of the fade actor asks its host to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeQuad {
    /// `FUN_80024EE4`'s first argument - block `+0x22`, the id the spawn
    /// stamped.
    pub id: i16,
    /// `FUN_80024EE4`'s second argument - block `+0x18`, the fade kind.
    pub kind: i32,
    /// `FUN_80024EE4`'s third argument - the packed colour.
    pub rgb: u32,
}

/// The fade actor's per-frame tick: step the ramp and, unless it aborted, hand
/// the host a full-screen quad to draw.
///
/// Retail body is three calls deep - `FUN_80020C14(actor)`, then, when the
/// result is not `-1`, `FUN_80024EE4(block[+0x22], block[+0x18], rgb)`. The
/// emitter itself is the GP0 packet builder documented in
/// `docs/reference/functions/renderer.md`; the engine draws the overlay from
/// its own draw list, so the port stops at the descriptor.
///
/// PORT: FUN_80025000
pub fn tick_fade_ramp(ramp: &mut FadeRamp, dt: u8) -> Option<FadeQuad> {
    let rgb = ramp.advance_fade_vsyncs(dt)?;
    Some(FadeQuad {
        id: ramp.id,
        kind: ramp.kind,
        rgb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_black_to_white(duration: i16) -> FadeRamp {
        // The shape FUN_80020B00 produces for a 0 -> 0xFF ramp: delta =
        // ((0xFF - 0) << 6) / duration.
        let delta = ((0xFF << 6) / duration as i32) as i16;
        FadeRamp::new(
            [0; 3],
            [CHANNEL_MAX_Q6; 3],
            [delta; 3],
            2,
            0,
            -1,
            duration,
            0,
        )
    }

    #[test]
    fn delay_suppresses_the_draw_then_releases() {
        let mut r = ramp_black_to_white(0x40);
        r.delay = 3;
        assert_eq!(r.advance_fade_vsyncs(1), None);
        assert_eq!(r.advance_fade_vsyncs(1), None);
        // Third step drops the delay to 0, so the frame draws.
        assert!(r.advance_fade_vsyncs(1).is_some());
    }

    #[test]
    fn ramp_lands_exactly_on_the_target() {
        let mut r = ramp_black_to_white(0x40);
        for _ in 0..0x40 {
            r.advance_fade_vsyncs(1);
        }
        assert_eq!(r.current_q6, [CHANNEL_MAX_Q6; 3]);
        assert_eq!(r.packed_fade_rgb(), 0x00FF_FFFF);
    }

    #[test]
    fn overshoot_clamps_onto_the_target_not_past_it() {
        let mut r = ramp_black_to_white(0x40);
        // A whole ramp's worth of vsyncs in one step.
        r.advance_fade_vsyncs(0x40);
        assert_eq!(r.current_q6, [CHANNEL_MAX_Q6; 3]);
    }

    #[test]
    fn negative_hold_never_finishes() {
        let mut r = ramp_black_to_white(4);
        for _ in 0..64 {
            r.advance_fade_vsyncs(1);
        }
        assert!(r.flags.ramped);
        assert!(!r.flags.finished, "hold == -1 disables the finish latch");
    }

    #[test]
    fn hold_expiry_finishes_and_stops_drawing() {
        let mut r = ramp_black_to_white(4);
        r.hold = 2;
        let mut drew = 0;
        for _ in 0..16 {
            if r.advance_fade_vsyncs(1).is_some() {
                drew += 1;
            }
        }
        assert!(r.flags.ramped);
        assert!(r.flags.finished);
        assert!(drew > 0 && drew < 16, "draws stop once the hold expires");
    }

    #[test]
    fn a_delta_pointing_away_from_the_target_snaps_on_the_first_step() {
        // delta negative but the target sits ABOVE current: the sign test
        // picks the `current < target` arm, which is true immediately, so the
        // channel jumps straight onto the target instead of ramping.
        let mut r = FadeRamp::new(
            [0x1000; 3],
            [CHANNEL_MAX_Q6; 3],
            [-0x400; 3],
            0,
            0,
            -1,
            0x40,
            0,
        );
        r.advance_fade_vsyncs(1);
        assert_eq!(r.current_q6, [CHANNEL_MAX_Q6; 3]);
    }

    #[test]
    fn tick_carries_the_block_ids_through() {
        let mut r = ramp_black_to_white(0x40);
        r.id = 7;
        r.kind = 2;
        let q = tick_fade_ramp(&mut r, 1).expect("first frame draws");
        assert_eq!(q.id, 7);
        assert_eq!(q.kind, 2);
    }
}
