//! `FUN_801D71B8` itself: the context / actor mirrors and the per-frame framing fold.

use super::*;

/// The battle-context counters the arms read - and, through the
/// `ctx[+0x26F]` latch, write.
///
/// Every field is a retail context byte or word. The engine's battle camera
/// owns one of these for the life of a battle, exactly as retail's context
/// does, so the arms' ramps are continuous across an action instead of being
/// recomputed from nothing each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackCamCtx {
    /// `ctx[+0x26D]` - the per-action coin flip that picks a track column.
    /// See the module doc; retail rolls it in `FUN_8004E13C`.
    pub phase_cursor: u8,
    /// `ctx[+0x26E]` - the `0..=`[`AttackCamCtx::RAMP_CAP`] ramp.
    pub ramp: u8,
    /// `ctx[+0x270]` - the second byte ramp the same prologue advances
    /// (`0x801D5960..0x801D59B8`), on the same law and the same cap. Read
    /// only by the case-8 death re-frame
    /// (`battle_cam_script::apply_death_reframe`).
    pub death_ramp: u8,
    /// `ctx[+0x87C]` - the free-running 32-bit accumulator advanced beside
    /// the ramp. Unlike the ramp it does not saturate; the arms read it whole
    /// (`lw` + shift) or truncated (`lhu`).
    pub accum: u32,
    /// `ctx[+0x26F]` - the latch several arms bump when they re-zero the
    /// ramp at an animation-frame threshold, so the reset fires once per
    /// crossing instead of every frame.
    pub latch: u8,
}

impl AttackCamCtx {
    /// Ceiling on [`Self::ramp`] (`sltiu v0,a1,0xc8` / `li v0,0xc8`).
    pub const RAMP_CAP: u8 = 0xC8;
    /// Ramp increment per display frame (`sll v0,v0,0x3` on the frame step).
    pub const RAMP_SCALE: u32 = 8;

    /// `FUN_801D5854`'s prologue ramp advance (`0x801D58F8..0x801D5960`),
    /// which runs on **every** call to that function - so once per display
    /// frame for as long as any framing case is being re-armed.
    ///
    /// The accumulator takes `frame_step * 8` unconditionally; the ramp takes
    /// the same increment only while it is still below the cap, and is then
    /// clamped to it.
    pub fn advance(&mut self, frame_step: u32) {
        let d = frame_step.wrapping_mul(Self::RAMP_SCALE);
        self.accum = self.accum.wrapping_add(d);
        self.ramp = Self::ramp_step(self.ramp, d);
        self.death_ramp = Self::ramp_step(self.death_ramp, d);
    }

    /// One byte ramp's step, shared by `ctx[+0x26E]` and `ctx[+0x270]`
    /// because retail spells the two out as byte-identical code
    /// (`0x801D5910..0x801D595C` and `0x801D5968..0x801D59B8`).
    ///
    /// The order matters: retail tests `ramp < 0xC8` *before* adding, stores
    /// the sum with `sb` - which truncates to 8 bits - and only then compares
    /// the stored byte against `0xC9`. So the clamp sees the truncated value,
    /// not the true sum, and a step of `>= 0x20` display frames wraps the ramp
    /// low instead of pinning it at the cap. Retail's own frame step is `1..4`
    /// so the wrap never fires in play; the port models it anyway because the
    /// alternative is an arithmetic that only agrees on the inputs it was
    /// tested with.
    fn ramp_step(cur: u8, d: u32) -> u8 {
        if cur >= Self::RAMP_CAP {
            return cur;
        }
        let stored = u32::from(cur).wrapping_add(d) as u8;
        if stored > Self::RAMP_CAP {
            Self::RAMP_CAP
        } else {
            stored
        }
    }

    /// The threshold-crossing reset several arms perform: when the latch
    /// still reads `expect`, re-zero the ramp (and, in the two Gala arms that
    /// pass `clear_accum`, the accumulator too) and bump the latch.
    ///
    /// Retail spells it `bne ctx[+0x26F], expect, skip` - the store to
    /// `ctx[+0x26E]` precedes the re-read of the latch, which is why the
    /// order here is reset-then-bump.
    pub fn latch_reset(&mut self, expect: u8, clear_accum: bool) {
        if self.latch != expect {
            return;
        }
        self.ramp = 0;
        if clear_accum {
            self.accum = 0;
        }
        self.latch = self.latch.wrapping_add(1);
    }

    /// Clear the per-action state and roll a fresh [`Self::phase_cursor`].
    /// `coin` is the caller's `rand() % 2` - retail's `FUN_8004E13C` rolls it
    /// at action seed, and the port's battle camera rolls it on entry into
    /// the Action phase.
    ///
    /// [`Self::death_ramp`] is **carried across** the seed. `FUN_8004E13C`
    /// writes exactly one byte of this quartet - `sb v0,0x26d(a0)` at
    /// `0x8004E2DC` - and nothing else in the action seed touches
    /// `ctx[+0x270]`; its only zeroing site is the death re-frame's own gate
    /// (`sb zero,0x270(a0)` at `0x801D6B4C`). Since the prologue ramp saturates
    /// in ~25 display frames, a fight's second death therefore reads a ramp
    /// already at the cap, which is what makes the re-frame a *recovery* from
    /// the corpse-on-the-floor pose rather than a pull-in from scratch.
    pub fn begin_action(&mut self, coin: bool) {
        *self = AttackCamCtx {
            phase_cursor: u8::from(coin),
            death_ramp: self.death_ramp,
            ..AttackCamCtx::default()
        };
    }
}

/// The acting actor's per-art camera channels - the three inputs the shared
/// battle camera did not have, plus the position and facing the seed needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackCamActor {
    /// `DAT_8007BD10[ctx[+0x13]]` resolved through [`character_arm`].
    pub character: CharacterArm,
    /// `actor[+0x1DB]` - the latched battle-animation id (see the module doc).
    pub art_id: u8,
    /// `actor[+0x21B]` - the arm sub-selector two arms branch on.
    pub arm_select: u8,
    /// `actor[+0x22C][+0x68]` - the live animation cursor, in **sixteenths**
    /// of a keyframe (retail's loop bounds are `clip[+0x85] << 4` /
    /// `clip[+0x86] << 4`, `FUN_80047430` `0x800477D4` / `0x8004781C`).
    pub anim_frame: i16,
    /// `actor[+0x34/+0x36/+0x38]`.
    pub pos: [u16; 3],
    /// `actor[+0x46]`.
    pub facing: u16,
}

/// What one frame of the per-art camera produces: the pose it hands the tween
/// builder and the duration (retail's `a3`, in **display frames**) it hands it
/// alongside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackCamFraming {
    /// The pose, with `dist[2]` still in **raw world units** - the tween
    /// builder is what converts it to projection units.
    pub pose: CameraPose,
    /// `FUN_801D829C`'s fourth argument.
    pub duration_frames: u16,
}

/// Run the per-art attack camera for one frame.
///
/// Returns `None` for every path that reaches the epilogue `0x801D828C`
/// without calling the tween builder - an art id with no arm, a character id
/// outside `1..=3`, a monster slot (`0x801D8280` writes the local frame and
/// returns), or Gala's `0x1E` arm past its animation cut-off. In every such
/// case retail leaves whatever `FUN_801D5854` armed standing, which is what
/// the caller must do too.
///
/// `ctx` is `&mut` because seven arms re-zero the ramps through the
/// `ctx[+0x26F]` latch.
///
/// PORT: FUN_801D71B8
pub fn attack_camera_framing(
    actor: AttackCamActor,
    ctx: &mut AttackCamCtx,
    table: &legaia_game_tables::battle_attack_camera_table::AttackCameraTracks,
) -> Option<AttackCamFraming> {
    let entry = art_arm(actor.character, actor.art_id)?;
    let mut p = CameraPose::seed(actor.pos, actor.facing);
    // Character 3's branch overwrites the seeded TR.y before it dispatches
    // (`li v0,0x600` in the delay slot at `0x801D72C8`, stored at
    // `0x801D7B14`).
    if actor.character == CharacterArm::Three {
        p.dist[1] = 0x600;
    }
    let cursor = ctx.phase_cursor;
    let phase = ArmPhase { cursor };
    let track = |t: usize| phase.track_value(table, t).unwrap_or(0);
    let f = actor.anim_frame;
    let s = actor.arm_select;

    // The ramp reads the arms share. `ramp` is a byte, so every scaling of it
    // fits an i16 without care; the accumulator is a 32-bit word whose low
    // halfword is what lands in the pose.
    let r = i16::from(ctx.ramp);
    let acc = ctx.accum;
    let acc_lo = acc as u16 as i16;
    let acc_shr = |k: u32| (acc >> k) as u16 as i16;
    let acc_shl = |k: u32| (acc << k) as u16 as i16;

    let dur: u16 = match entry {
        // -- character 1 -------------------------------------------------
        // 0x801D7308 (art 0x1A).
        0x801D_7308 => {
            if s == 5 {
                p.dist[2] = p.dist[2].wrapping_sub(0x100);
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(1));
                if let Some(push) = anim_push(f) {
                    apply_anim_push(&mut p, cursor, push);
                }
                6
            } else if s < 2 {
                // `0x801D7418` falls into character 3's late block.
                gala_late_arm(&mut p, ctx, track(4));
                1
            } else {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            }
        }
        // 0x801D74A8 (art 0x1E / 0x2A).
        0x801D_74A8 => {
            if f < 0xE0 {
                p.rot[0] = p.rot[0].wrapping_sub(r >> 2);
                p.rot[1] = p.rot[1].wrapping_add(track(5).wrapping_add(acc_lo));
                p.dist[1] = p.dist[1].wrapping_add(r);
                p.dist[2] = p.dist[2].wrapping_add(r >> 2);
                6
            } else {
                p.rot[1] = p.rot[1].wrapping_add(0x600);
                p.dist[2] = p.dist[2].wrapping_add(0x400);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                1
            }
        }
        // 0x801D7568 (art 0x1D).
        0x801D_7568 => {
            if f < 0xF0 {
                p.rot[0] = p.rot[0].wrapping_sub(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(6).wrapping_add(acc_lo));
                p.dist[1] = p.dist[1].wrapping_add(r << 1);
                p.dist[2] = p.dist[2].wrapping_add(acc_shr(2));
                6
            } else {
                p.rot[0] = p.rot[0].wrapping_add(0x100);
                p.dist[0] = p.dist[0].wrapping_sub(0x400);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
                p.dist[2] = p.dist[2].wrapping_add(0x400);
                p.dist[1] = p.dist[1].wrapping_add(0x200);
                3
            }
        }
        // 0x801D7650 (art 0x1C).
        0x801D_7650 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 2);
            p.rot[1] = p.rot[1].wrapping_add(track(7).wrapping_add(acc_lo));
            p.dist[2] = p.dist[2].wrapping_add(acc_shr(1));
            6
        }
        // -- character 2 -------------------------------------------------
        // 0x801D76EC (art 0x1A).
        0x801D_76EC => {
            if s == 2 {
                p.dist[2] = p.dist[2].wrapping_sub(0x100);
                p.dist[0] = p.dist[0].wrapping_sub(0x100);
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(13));
                6
            } else if f < 0x90 {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            } else {
                // Rejoins the shared block at `0x801D7D4C` with row 14.
                gala_late_arm(&mut p, ctx, track(14));
                1
            }
        }
        // 0x801D7870 (art 0x20).
        0x801D_7870 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 2);
            p.rot[1] = p.rot[1].wrapping_add(track(8));
            p.dist[1] = p.dist[1].wrapping_add(r);
            p.dist[2] = p.dist[2].wrapping_sub(acc_shr(1).wrapping_sub(0x400));
            6
        }
        // 0x801D78F0 (art 0x1D / 0x2C).
        0x801D_78F0 => {
            p.rot[0] = p.rot[0].wrapping_add(r >> 1);
            p.dist[0] = p.dist[0].wrapping_add(0x200);
            p.rot[1] = p.rot[1].wrapping_sub(track(9).wrapping_add(acc_shr(1)));
            p.dist[1] = p.dist[1].wrapping_add(r >> 1);
            p.dist[2] = p.dist[2].wrapping_add(acc_shl(1));
            6
        }
        // 0x801D797C (art 0x1E / 0x2D). The pitch fold is `srl` then
        // `ori 0xff80`, i.e. a small ramp OR-ed into a negative constant.
        0x801D_797C => {
            p.rot[0] = p.rot[0].wrapping_add(((r >> 2) as u16 | 0xFF80) as i16);
            p.dist[0] = p.dist[0].wrapping_sub(0x300);
            p.rot[1] = p.rot[1].wrapping_add(track(10).wrapping_add(acc_shr(1)));
            p.dist[2] = p.dist[2].wrapping_add(acc_shr(1));
            1
        }
        // 0x801D79F8 (art 0x1F).
        0x801D_79F8 => {
            if f < 0xE0 {
                p.rot[0] = p.rot[0].wrapping_add(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(11).wrapping_add(acc_shr(1)));
                p.dist[0] = p.dist[0].wrapping_add(track(15));
                p.dist[2] = p.dist[2].wrapping_add(r >> 1);
                6
            } else {
                p.rot[0] = p.rot[0].wrapping_add(r >> 1);
                p.rot[1] = p.rot[1].wrapping_add(track(12));
                p.dist[0] = p.dist[0].wrapping_add(track(16));
                p.dist[1] = p.dist[1].wrapping_sub(r >> 1);
                p.dist[2] = p.dist[2].wrapping_add(r.wrapping_mul(3));
                1
            }
        }
        // -- character 3 -------------------------------------------------
        // 0x801D7B4C (art 0x1A).
        0x801D_7B4C => {
            if f < 0x70 {
                p.dist[2] = p.dist[2].wrapping_sub(track(19).wrapping_add(r));
                p.rot[0] = p.rot[0].wrapping_add(track(0));
                p.rot[1] = p.rot[1].wrapping_add(track(17));
                6
            } else if f < 0xA0 {
                threshold_arm(&mut p, ctx, 0, track(2), track(3));
                1
            } else {
                gala_late_arm(&mut p, ctx, track(4));
                1
            }
        }
        // 0x801D7D7C (art 0x1C) - four literal-only animation windows.
        0x801D_7D7C => {
            if f < 0x40 {
                p.rot[0] = p.rot[0].wrapping_add(0x80);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
            } else if f < 0x70 {
                p.rot[0] = p.rot[0].wrapping_sub(0x100);
                p.rot[1] = p.rot[1].wrapping_sub(0x200);
                p.dist[0] = p.dist[0].wrapping_add(0x280);
                p.dist[1] = p.dist[1].wrapping_sub(0x200);
            } else if f < 0xA0 {
                p.rot[0] = p.rot[0].wrapping_sub(0x100);
                p.rot[1] = p.rot[1].wrapping_add(0x200);
                p.dist[0] = p.dist[0].wrapping_sub(0x200);
                p.dist[1] = p.dist[1].wrapping_sub(0x200);
            } else {
                p.rot[0] = p.rot[0].wrapping_add(0x80);
                p.rot[1] = p.rot[1].wrapping_sub(0x200);
                p.dist[0] = p.dist[0].wrapping_add(0x300);
            }
            1
        }
        // 0x801D7EA0 (art 0x1D) - the one arm that drives the roll slot, and
        // the one that reads a track row at a FIXED cursor.
        0x801D_7EA0 => {
            if f < 0xB0 {
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_sub(r >> 1);
                    p.rot[1] = p.rot[1].wrapping_add(track(18).wrapping_add(acc_shl(1)));
                    p.rot[2] = p.rot[2].wrapping_sub(r >> 1);
                    p.dist[0] = p.dist[0].wrapping_add(acc_lo);
                    p.dist[1] = p.dist[1].wrapping_sub(acc_shr(1));
                    // `multu` by 0xAAAAAAAB then `srl 1` - a divide by three.
                    let third = (((u64::from(acc) * 0xAAAA_AAABu64) >> 32) >> 1) as u16 as i16;
                    p.dist[2] = p.dist[2].wrapping_add(third.wrapping_add(0xFF00u16 as i16));
                } else {
                    p.rot[0] = p.rot[0].wrapping_add(r);
                    // `lhu v1,0x4e58(v0)` - row 18 at cursor 0, hard-coded.
                    let fixed = table.track(18, 0).unwrap_or(0);
                    p.rot[1] = p.rot[1].wrapping_add(fixed.wrapping_add(acc_lo));
                }
                6
            } else if f < 0x110 {
                ctx.latch_reset(0, true);
                let acc = ctx.accum;
                let lo = acc as u16 as i16;
                let shl1 = (acc << 1) as u16 as i16;
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_add(lo.wrapping_add(0xFFA0u16 as i16));
                    p.rot[1] = p.rot[1]
                        .wrapping_sub((acc.wrapping_mul(3) as u16 as i16).wrapping_sub(0x400));
                    p.dist[0] = p.dist[0].wrapping_add(shl1.wrapping_add(0xFD00u16 as i16));
                    p.dist[1] = p.dist[1].wrapping_add(shl1.wrapping_add(0xFE00u16 as i16));
                    p.dist[2] = p.dist[2].wrapping_sub(shl1.wrapping_sub(0x200));
                } else {
                    p.dist[0] = p.dist[0].wrapping_sub(0x400);
                    p.rot[1] = p.rot[1].wrapping_add(0x280);
                    p.dist[2] = p.dist[2].wrapping_sub(0x100);
                    p.dist[1] = p.dist[1].wrapping_sub(0x300);
                    p.rot[0] = p.rot[0].wrapping_sub((i16::from(ctx.ramp) >> 2) + 0x80);
                }
                1
            } else {
                ctx.latch_reset(1, true);
                let acc = ctx.accum;
                let r = i16::from(ctx.ramp);
                let shl1 = (acc << 1) as u16 as i16;
                p.dist[0] = p.dist[0].wrapping_sub(shl1.wrapping_sub(0x400));
                if cursor != 0 {
                    p.rot[0] = p.rot[0].wrapping_sub(r);
                    p.rot[1] = p.rot[1].wrapping_add(r.wrapping_mul(6).wrapping_sub(0x600));
                    p.dist[1] = p.dist[1].wrapping_sub(r);
                    p.dist[2] = p.dist[2].wrapping_sub(r);
                } else {
                    p.rot[0] = p.rot[0].wrapping_sub(0x80);
                    p.dist[2] = p.dist[2].wrapping_sub(0x100);
                    p.dist[1] = p.dist[1].wrapping_sub(0x100);
                    p.rot[1] =
                        p.rot[1].wrapping_add((acc as u16 as i16).wrapping_add(0xFE00u16 as i16));
                }
                1
            }
        }
        // 0x801D81FC (art 0x1E / 0x2A) - the one arm with a hard cut-off:
        // past `0xC0` it returns without arming anything.
        0x801D_81FC => {
            if f >= 0xC0 {
                return None;
            }
            p.rot[1] = p.rot[1].wrapping_add(r.wrapping_mul(5).wrapping_sub(0x600));
            p.dist[0] = p.dist[0].wrapping_add(r << 2);
            p.dist[2] = p.dist[2].wrapping_add(0x40i16.wrapping_sub(r << 1));
            1
        }
        _ => return None,
    };
    Some(AttackCamFraming {
        pose: p,
        duration_frames: dur,
    })
}

/// The "mid-swing" block three arms share (`0x801D7420`, `0x801D7788`,
/// `0x801D7C6C`): latch-reset the ramp at the threshold crossing, tilt the
/// pitch a fixed `0xC0`, fold one row into the yaw and another into the depth,
/// and drop the height half a unit.
///
/// The ramp read at the tail (`0x801D749C` and siblings) is **after** the
/// reset, so the frame the threshold is crossed on subtracts nothing - which
/// is why this is a helper rather than an inline `r << 1`.
fn threshold_arm(
    pose: &mut CameraPose,
    ctx: &mut AttackCamCtx,
    expect_latch: u8,
    yaw_row: i16,
    depth_row: i16,
) {
    ctx.latch_reset(expect_latch, false);
    pose.rot[0] = pose.rot[0].wrapping_add(0xC0);
    pose.rot[1] = pose.rot[1].wrapping_add(yaw_row);
    pose.dist[1] = pose.dist[1].wrapping_sub(0x200);
    pose.dist[2] = pose.dist[2]
        .wrapping_add(depth_row)
        .wrapping_sub(i16::from(ctx.ramp) << 1);
}

/// The block at `0x801D7CF4` + `0x801D7D4C`: the late window of Gala's `0x1A`
/// arm, which Vahn's `0x1A` arm branches into when `actor[+0x21B] < 2`
/// (`0x801D7418`) and Noa's `0x1A` arm reaches from `0x801D7810` with its own
/// row. Like [`threshold_arm`], the ramp is read after the latch reset.
fn gala_late_arm(pose: &mut CameraPose, ctx: &mut AttackCamCtx, row: i16) {
    ctx.latch_reset(1, false);
    pose.dist[1] = pose.dist[1].wrapping_sub(0x100);
    pose.dist[2] = pose.dist[2].wrapping_add(0x200);
    pose.rot[1] = pose.rot[1].wrapping_add(row);
    pose.dist[2] = pose.dist[2].wrapping_sub(i16::from(ctx.ramp) << 1);
}
