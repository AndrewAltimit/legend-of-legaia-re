//! Camera-relative **glide** actor tick - the per-frame partner of the
//! spawner [`legaia_engine_vm::camera_rel_actor`].
//!
//! (The `PORT:` tag sits on [`CameraRelGlide::tick`], the item that
//! implements the body. A module-level tag would attribute every `pub` item
//! in this file to that one address.)
//!
//! ## What pinned the role
//!
//! `FUN_8002149C` is a 172-instruction leaf with no stack frame, which is
//! why Ghidra never carved it out of the surrounding bytes and why
//! `docs/reference/functions/game-modes.md` used to record its role as *not
//! established*. Three independent facts settle it, and none of them needs
//! the decompiled C:
//!
//! 1. **It is a spawn-descriptor handler.** The 24-byte descriptor family at
//!    `0x800705FC..0x80070763` carries its handler at `+0x8`
//!    (`docs/subsystems/asset-loader.md`), and descriptor `0x8007071C`'s `+8`
//!    word reads `0x8002149C` straight off `extracted/SCUS_942.54`.
//!    `0x8007071C` is exactly the descriptor `FUN_80021248` allocates from
//!    (`0x800212AC`), so this leaf is *that family's* `actor+0x0C` tick.
//! 2. **It consumes precisely what the spawner produces.** `FUN_80021248`
//!    writes a normalized 20-halfword record to `actor+0x80` and the folded
//!    angle deltas to `actor+0x24/26/28`. This tick reads ten `(step, target)`
//!    halfword pairs from `actor+0x80` and three `u16` budgets from
//!    `actor+0x24`, and drives the ten camera globals in the same order and
//!    with the same widths the spawner compared them against: the three `u16`
//!    angles `0x8007B790/92/94`, the `i32` eye-space trio `0x800840B8/BC/C0`,
//!    the `i32` focus trio `0x80089118/1C/20`, and the `u16` GTE `H`
//!    `0x8007B6F4`. Those ten in that order are
//!    [`crate::camera::RetailCamGlobals`].
//! 3. **Its terminal handshake is the spawner's, inverted.** The spawner runs
//!    `scratch &= !0x80; ... scratch |= 0x100` on `_DAT_1F800394`
//!    (`legaia_engine_vm::camera_rel_actor::spawn_handshake`). When all ten
//!    channels have arrived this tick runs `scratch &= !0x100; scratch |=
//!    0x80` (`0x80021720..0x80021730`) and raises its own kill bit
//!    `actor[+0x10] |= 8`. So bit `0x100` means "a camera-relative glide is
//!    live" and bit `0x80` means "the last one finished".
//!
//! So the family is a **one-shot camera glide**: `FUN_80021248` stages a
//! shot as ten signed per-frame velocities plus their endpoints, and this
//! tick walks the live camera onto them and retires.
//!
//! ## The per-frame law (`0x8002149C..0x80021744`)
//!
//! Every channel is skipped - and *counted as arrived* - when its step is
//! `0`, which is how a beat leaves an axis untouched. Otherwise the frame
//! step is `step * DAT_1F800393` (the adaptive frame-skip factor), so travel
//! is denominated in display frames like every other duration in the engine.
//! The three channel classes differ in how arrival is detected:
//!
//! | channels | global width | arrival test |
//! |---|---|---|
//! | 0..3 angles | `u16`, 16-bit wrap | a per-channel **distance budget** at `actor+0x24+2i` drops by `\|step*dt\|`; underflow (tested as `i16`) snaps the angle to the target |
//! | 3..9 eye / focus | `i32` | overshoot: `step > 0` arrives at `cur >= target`, `step <= 0` at `cur <= target` |
//! | 9 GTE `H` | `u16`, 16-bit wrap | overshoot, on the sign-extended halfword |
//!
//! The budget is what makes the angle channels correct across the 12-bit
//! wrap: the spawner already folded the shortest arc into `actor+0x24`, so
//! the tick counts *distance travelled* rather than comparing angles, and
//! then writes the exact endpoint. Retail stores the accumulated value
//! before the arrival test in all three classes and overwrites it with the
//! target on arrival, so an arriving channel is exact - never one frame
//! past.
//!
//! ## NOT WIRED
//!
//! Same gap as the spawner's, and for the same reason: nothing in the engine
//! produces the 20-halfword spawn record. The camera half is fully available
//! ([`crate::camera::Camera::globals`] is the ten axes this tick drives, and
//! `RetailCamGlobals::camera_snapshot` is what the spawner normalizes
//! against), but the record itself comes from the battle overlay's callers
//! of `FUN_80021248`, and the engine has no port of that family's spawn
//! sites. Standing this on a synthetic record would only re-run the unit
//! tests below under a different name.

use crate::camera::RetailCamGlobals;
use legaia_engine_vm::camera_mover::AXIS_COUNT;
use legaia_engine_vm::camera_rel_actor::NormalizedParams;

/// The three angle channels at the head of the record - the ones driven by a
/// distance budget rather than an overshoot test.
pub const ANGLE_AXES: usize = 3;

/// Channels that must report arrival before the actor retires. Ten, the same
/// [`AXIS_COUNT`] the op-`0x45` camera mover uses.
pub const ARRIVED_ALL: u32 = AXIS_COUNT as u32;

/// `_DAT_1F800394` bit the spawner raises for "a glide of this family is
/// live" and this tick clears on completion.
pub const SCRATCH_GLIDE_LIVE: u32 = 0x100;

/// `_DAT_1F800394` bit this tick raises on completion.
pub const SCRATCH_GLIDE_DONE: u32 = 0x80;

/// `actor[+0x10]` bit the tick sets to retire itself.
pub const ACTOR_KILL_BIT: u32 = 8;

/// One camera-relative glide actor: the ten `(step, target)` pairs at
/// `actor+0x80` plus the three angle budgets at `actor+0x24`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraRelGlide {
    /// Per-frame velocity per channel, signed. `0` parks the channel.
    pub step: [i16; AXIS_COUNT],
    /// Endpoint per channel. For the `i32` channels retail reads this as a
    /// **halfword** (`lh` at `0x800215A8` / `0x8002163C`), so an eye or focus
    /// endpoint is confined to `i16` range even though the global is 32-bit.
    pub target: [i16; AXIS_COUNT],
    /// `actor+0x24/26/28` - remaining folded angular distance per angle
    /// channel, in the same 12-bit units as the globals.
    pub angle_budget: [u16; ANGLE_AXES],
}

/// What one tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlideTick {
    /// How many of the ten channels reported arrival **this frame** - the
    /// `a2` counter. A parked (`step == 0`) channel counts every frame, which
    /// is why the total, not a latch, is the completion test.
    pub arrived: u32,
    /// `arrived == ARRIVED_ALL` - the frame the actor retires.
    pub finished: bool,
}

impl CameraRelGlide {
    /// Seat a glide from the spawner's normalized record. `params[i*2]` is
    /// channel `i`'s signed magnitude and `params[i*2+1]` its reference (the
    /// endpoint); `rotation` is the folded angle distance the spawner wrote
    /// to `actor+0x24`.
    ///
    /// REF: FUN_80021248
    pub fn from_normalized(n: &NormalizedParams) -> Self {
        let mut g = Self::default();
        for i in 0..AXIS_COUNT {
            g.step[i] = n.params[i * 2];
            g.target[i] = n.params[i * 2 + 1];
        }
        for i in 0..ANGLE_AXES {
            g.angle_budget[i] = n.rotation[i] as u16;
        }
        g
    }

    /// One frame of the glide. `dt` is `DAT_1F800393`, the adaptive
    /// frame-skip factor.
    ///
    /// PORT: FUN_8002149C
    ///
    /// NOT WIRED: the engine has no producer for this family's 20-halfword
    /// spawn record. `FUN_80021248`'s callers are battle-overlay spawn sites
    /// that have no engine counterpart, so there is nothing to seat a
    /// [`CameraRelGlide`] from in a live session; see the module docs.
    pub fn tick(&mut self, cam: &mut RetailCamGlobals, dt: u8) -> GlideTick {
        let dt = i32::from(dt);
        let mut arrived = 0u32;

        // Channels 0..3 - the u16 angle globals, budget-driven
        // (`0x800214C0..0x80021550`).
        for i in 0..ANGLE_AXES {
            let step = i32::from(self.step[i]);
            if step == 0 {
                arrived += 1;
                continue;
            }
            let frame = step * dt;
            // `lhu` + `addu` + `sh`: pure 16-bit wrap. The engine mirror
            // keeps the halfword sign-extended (the form every other reader
            // of these globals uses), so wrap in `i16`.
            let cur = (cam.0[i] as i16).wrapping_add(frame as i16);
            cam.0[i] = i32::from(cur);
            // `subu` of |frame| off the budget, stored as a halfword and
            // then sign-tested (`sll 16` + `bgez`).
            let left = self.angle_budget[i].wrapping_sub(frame.unsigned_abs() as u16);
            self.angle_budget[i] = left;
            if (left as i16) < 0 {
                arrived += 1;
                cam.0[i] = i32::from(self.target[i]);
                self.angle_budget[i] = 0;
            }
        }

        // Channels 3..9 - the i32 eye-space and focus globals, overshoot
        // clamped (`0x8002156C..0x800215E4` and `0x80021600..0x80021678`).
        for i in ANGLE_AXES..AXIS_COUNT - 1 {
            let step = i32::from(self.step[i]);
            if step == 0 {
                arrived += 1;
                continue;
            }
            let cur = cam.0[i].wrapping_add(step * dt);
            cam.0[i] = cur;
            let target = i32::from(self.target[i]);
            let travelling = if step > 0 { cur < target } else { target < cur };
            if !travelling {
                cam.0[i] = target;
                arrived += 1;
            }
        }

        // Channel 9 - GTE `H`, a u16 global with the same overshoot test on
        // the sign-extended halfword (`0x8002167C..0x80021710`).
        let step = i32::from(self.step[AXIS_COUNT - 1]);
        if step == 0 {
            arrived += 1;
        } else {
            let cur = (cam.0[AXIS_COUNT - 1] as i16).wrapping_add((step * dt) as i16);
            cam.0[AXIS_COUNT - 1] = i32::from(cur);
            let target = self.target[AXIS_COUNT - 1];
            let travelling = if step > 0 { cur < target } else { target < cur };
            if !travelling {
                cam.0[AXIS_COUNT - 1] = i32::from(target);
                arrived += 1;
            }
        }

        GlideTick {
            arrived,
            finished: arrived == ARRIVED_ALL,
        }
    }
}

/// The completion handshake on `_DAT_1F800394` - the exact inverse of
/// `legaia_engine_vm::camera_rel_actor::spawn_handshake`.
///
/// Retail: `scratch = (scratch & ~0x100) | 0x80` (`0x80021720..0x80021730`).
pub fn finish_handshake(scratch: u32) -> u32 {
    (scratch & !SCRATCH_GLIDE_LIVE) | SCRATCH_GLIDE_DONE
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::camera_rel_actor::{normalize_camera_relative_params, spawn_handshake};

    fn glide(pairs: [(i16, i16); AXIS_COUNT], budget: [u16; ANGLE_AXES]) -> CameraRelGlide {
        let mut g = CameraRelGlide {
            angle_budget: budget,
            ..Default::default()
        };
        for (i, (s, t)) in pairs.into_iter().enumerate() {
            g.step[i] = s;
            g.target[i] = t;
        }
        g
    }

    /// A record that parks every channel finishes on its first frame without
    /// touching the camera. This is the "leave this axis alone" encoding, and
    /// it is also why the terminal test counts to ten instead of latching.
    #[test]
    fn an_all_parked_record_retires_immediately_and_moves_nothing() {
        let mut cam = RetailCamGlobals::FIELD_RESET;
        let before = cam;
        let mut g = CameraRelGlide::default();
        let t = g.tick(&mut cam, 3);
        assert_eq!(t.arrived, ARRIVED_ALL);
        assert!(t.finished);
        assert_eq!(cam, before);
    }

    /// The completion handshake must undo the spawn handshake bit-for-bit,
    /// otherwise a finished glide would still read as live to the next
    /// spawner and it would kill an actor that is already gone.
    #[test]
    fn finish_handshake_inverts_the_spawn_handshake() {
        let spawned = spawn_handshake(0).scratch;
        assert_eq!(spawned & SCRATCH_GLIDE_LIVE, SCRATCH_GLIDE_LIVE);
        assert_eq!(spawned & SCRATCH_GLIDE_DONE, 0);
        let done = finish_handshake(spawned);
        assert_eq!(done & SCRATCH_GLIDE_LIVE, 0);
        assert_eq!(done & SCRATCH_GLIDE_DONE, SCRATCH_GLIDE_DONE);
        // Unrelated story-flag bits in the same scratch word survive both.
        let busy = spawn_handshake(1 << 26).scratch;
        assert_eq!(finish_handshake(busy) & (1 << 26), 1 << 26);
    }

    /// What the budget is *for*: an angle channel must land on its endpoint
    /// exactly, never a frame's worth past it, for any step size / cadence
    /// pair that does not divide the distance evenly.
    #[test]
    fn an_angle_channel_lands_exactly_on_its_target() {
        for step in [7i16, 13, -11, -64] {
            for dt in [1u8, 2, 3, 5] {
                let distance = 0x137u16;
                let mut g = glide(
                    [
                        (step, 0x321),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                    ],
                    [distance, 0, 0],
                );
                let mut cam = RetailCamGlobals([0; AXIS_COUNT]);
                let mut frames = 0;
                while !g.tick(&mut cam, dt).finished {
                    frames += 1;
                    assert!(frames < 4096, "glide never converged");
                }
                assert_eq!(
                    cam.0[0], 0x321,
                    "step {step} dt {dt} must snap to the endpoint"
                );
                assert_eq!(g.angle_budget[0], 0);
                // The budget is a distance, so the frame count is the
                // distance divided by the per-frame travel, rounded up.
                let per_frame = i32::from(step).unsigned_abs() * u32::from(dt);
                let want = u32::from(distance).div_ceil(per_frame);
                assert_eq!(frames + 1, want, "step {step} dt {dt}");
            }
        }
    }

    /// The eye / focus channels have no budget - only an overshoot test - so
    /// the property that matters is that neither sign of travel can end up
    /// past its endpoint.
    #[test]
    fn an_i32_channel_never_finishes_past_its_target() {
        for (start, step, target) in [
            (0i32, 40i16, 1000i16),
            (0, 37, 1000),
            (1000, -37, 0),
            (-500, 9, 3),
            (500, -9, -3),
        ] {
            for dt in [1u8, 3] {
                let mut g = glide(
                    [
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (step, target),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                    ],
                    [0; ANGLE_AXES],
                );
                let mut cam = RetailCamGlobals([0; AXIS_COUNT]);
                cam.0[3] = start;
                let mut frames = 0;
                while !g.tick(&mut cam, dt).finished {
                    frames += 1;
                    assert!(frames < 4096, "glide never converged");
                }
                assert_eq!(cam.0[3], i32::from(target));
            }
        }
    }

    /// The whole point of the family, end to end: `FUN_80021248` normalizes a
    /// spawn record against the live camera, and ticking the actor it seats
    /// walks that same camera onto the record's reference values. If the
    /// normalizer's sign convention and this tick's arrival tests disagreed
    /// on even one channel class, that channel would travel away from its
    /// reference forever instead of converging.
    #[test]
    fn a_record_normalized_by_the_spawner_glides_the_camera_onto_its_references() {
        let mut cam =
            RetailCamGlobals([0x100, 0xF00, 0x800, 900, -400, 20000, -8640, 0, -10304, 500]);
        let snapshot = cam.camera_snapshot();

        // Ten (magnitude, reference) pairs: a magnitude for every channel and
        // an endpoint on the far side of the camera's current value in both
        // directions.
        let refs: [i16; AXIS_COUNT] =
            [0x200, 0x100, 0x700, 400, 300, 19000, -9000, 250, -9800, 700];
        let mut record = [0i16; 20];
        for i in 0..AXIS_COUNT {
            record[i * 2] = 17;
            record[i * 2 + 1] = refs[i];
        }

        let normalized = normalize_camera_relative_params(&record, &snapshot);
        let mut g = CameraRelGlide::from_normalized(&normalized);

        let mut frames = 0;
        while !g.tick(&mut cam, 2).finished {
            frames += 1;
            assert!(frames < 100_000, "glide never converged");
        }
        for (i, want) in refs.iter().enumerate() {
            assert_eq!(
                cam.0[i],
                i32::from(*want),
                "channel {i} did not converge on its reference"
            );
        }
    }

    /// A parked camera plus a live one differ only in the arrival count, so a
    /// half-parked record still retires the frame its live channels land.
    #[test]
    fn a_partially_parked_record_retires_with_its_live_channels() {
        let mut g = glide(
            [
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (25, 600),
            ],
            [0; ANGLE_AXES],
        );
        let mut cam = RetailCamGlobals([0; AXIS_COUNT]);
        let first = g.tick(&mut cam, 1);
        assert_eq!(first.arrived, ARRIVED_ALL - 1);
        assert!(!first.finished);
        let mut frames = 0;
        while !g.tick(&mut cam, 1).finished {
            frames += 1;
            assert!(frames < 1024);
        }
        assert_eq!(cam.0[AXIS_COUNT - 1], 600);
    }

    /// `dt` scales travel, not the endpoint: the same record at two cadences
    /// converges on the same camera, only in fewer frames.
    #[test]
    fn cadence_changes_the_frame_count_not_the_endpoint() {
        let seat = || {
            glide(
                [
                    (9, 0x400),
                    (0, 0),
                    (0, 0),
                    (11, 5000),
                    (0, 0),
                    (0, 0),
                    (0, 0),
                    (0, 0),
                    (0, 0),
                    (0, 0),
                ],
                [0x400, 0, 0],
            )
        };
        let run = |dt: u8| {
            let mut g = seat();
            let mut cam = RetailCamGlobals([0; AXIS_COUNT]);
            let mut frames = 0u32;
            while !g.tick(&mut cam, dt).finished {
                frames += 1;
                assert!(frames < 4096);
            }
            (cam, frames)
        };
        let (cam1, f1) = run(1);
        let (cam3, f3) = run(3);
        assert_eq!(cam1.0[0], cam3.0[0]);
        assert_eq!(cam1.0[3], cam3.0[3]);
        assert!(f3 < f1);
    }
}
