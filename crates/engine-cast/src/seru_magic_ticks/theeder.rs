//! PROT 0904 Theeder (`0x82`): monster-row cone sweep tick, its ray geometry and trail packets.

use super::*;
use legaia_render_kernels::cast_theeder_packet::TheederPacket;

// ---------------------------------------------------------------------------
// PROT 0904 - Theeder (spell id 0x82)
// ---------------------------------------------------------------------------

/// PROT 0904's tick entry.
pub const THEEDER_TICK: u32 = 0x801F_69D8;

/// The arm PROT 0904 raises the summon seat in (`li v0,0x4` at `0x801F6ECC`).
pub const THEEDER_RISE_ARM: u8 = 4;

/// The arm PROT 0904 clears the summon seat's render flag in
/// (`0x801F70A4`).
pub const THEEDER_SHOW_ARM: u8 = 5;

/// The arm PROT 0904 bumps the summon seat's clip in (`0x801F71A8`).
pub const THEEDER_STAGE_ARM: u8 = 6;

/// The arm PROT 0904 retargets the caster and the summon seat onto the enemy
/// row in (`li v0,0x9; sb v0,0x1dd(..)` at `0x801F79A4`).
pub const THEEDER_RETARGET_ARM: u8 = 11;

/// The arm PROT 0904's expanding-ring sweep fires in.
pub const THEEDER_SWEEP_ARM: u8 = 12;

/// The arm PROT 0904 settles from.
pub const THEEDER_SETTLE_ARM: u8 = 14;

/// The arm PROT 0904 zeroes `ctx+0x6D8` in (`sh zero,0x6d8(s1)` at
/// `0x801F7768`) ahead of the retarget arm's ramp.
pub const THEEDER_RAMP_RESET_ARM: u8 = 10;

/// Where the retarget arm's ramp holds until (`slti v0,v0,0x100` at
/// `0x801F77B8`).
pub const THEEDER_RETARGET_RAMP_END: i16 = 0x100;

/// Where the sweep arm's ray stops (`slti v0,v0,0x1000` at `0x801F7EF0`) -
/// one full turn.
pub const THEEDER_SWEEP_END: i16 = 0x1000;

/// What one engine tick adds to `ctx+0x6D8` in PROT 0904's retarget arm
/// (`+= (step * scalar) >> 1` a pass, `0x801F778C..0x801F77AC`) and in its
/// sweep arm (`+= (step * scalar) << 3`, `0x801F7ACC..0x801F7AF0`). A pass
/// spans `step` vsyncs and the engine ticks once a vsync, so the per-tick
/// product is the speed scalar alone
/// ([`crate::cast_module_camera::MODULE_DRAIN_PER_TICK`]): the retarget arm
/// holds 64 ticks and the sweep turns its ray once in 64 more - the 16 / 16
/// passes the retail dwell measures at a frame step of `4`.
pub const fn theeder_ramp_per_tick(phase: u8) -> u16 {
    let d = crate::cast_module_camera::MODULE_DRAIN_PER_TICK as u16;
    match phase {
        THEEDER_RETARGET_ARM => d >> 1,
        THEEDER_SWEEP_ARM => d << 3,
        _ => 0,
    }
}

/// The sweep word PROT 0904's arm 12 builds its ray from on the tick it is
/// about to run: `ctx+0x6D8` after the arm's own ramp
/// ([`theeder_ramp_per_tick`]), masked to a turn (`andi v0,v0,0xfff` at
/// `0x801F7B98`). It is a **phase**, not a bearing: the ray's bearing is the
/// summon's facing plus `sin[phase] / 16` ([`theeder_ray_tip`]). The host
/// resolves cone membership with the tip before the tick.
pub const fn theeder_sweep_phase(ctx_6d8: u16) -> u16 {
    ctx_6d8.wrapping_add(theeder_ramp_per_tick(THEEDER_SWEEP_ARM)) & 0x0FFF
}

/// The render flag PROT 0904 raises the summon seat to in arm 4.
pub const THEEDER_RISE_RENDER_FLAG: u8 = 4;

/// PROT 0904 (Theeder, spell id `0x82`) tick body - 6020 bytes, 1505
/// instructions.
///
/// Fifteen phase arms `0..=14` plus `0xFF`, through the chain at
/// `0x801F6A7C..0x801F6B2C`. Its prologue guards the victim lookup
/// (`sltiu v0, s6, 0x8` at `0x801F6A30`): a `+0x1DD` of `8` or more leaves the
/// victim register untouched, which is why the ring sweep addresses seats
/// through the actor table rather than through that register.
///
/// Arm 12 is a **swinging-ray sweep**, not a single hit. `ctx+0x6D8` grows
/// by the frame delta times `8` each tick (`0x801F7AE4`) and swings a ray
/// from the beam root `+-0x100` about the summon's facing
/// ([`theeder_ray_tip`]); the arm walks
/// `actor_table[3 ..= 6]` - a hard-coded `sltiu s3, 0x7`, not `ctx[+1]` -
/// hitting each seat that is alive, is not already reacting (`+0x1D9 == 0`),
/// is inside a `+-0x30` cone of the ray seen from its root (`0x801F7D0C`), and is
/// not [`FLAG_NON_TARGETABLE`]. Per hit: `FUN_801DD0AC(0x11, 7, seat)` -
/// the `0x11` is in the `bne` delay slot at `0x801F7D2C` - the unsigned clamp
/// at `0x801F7D6C`, `+0x10 +=`, `+0x14C -=`, `+0x04 = 0x3FF0000`, render flag
/// `= 0`, `+0x21F = 2` and [`stage_reaction_bits`]. The arm advances only
/// once `ctx+0x6D8` has passed `0x1000`, so the sweep repeats for as long as
/// the ray is swinging and the `+0x1D9` guard is what stops a seat being hit
/// twice. A seat outside the swing's arc is never hit.
///
/// Other simulation state: `ctx+0x278 = 0` in arm 0 (`0x801F6C10`), the
/// summon seat's render flag `= 4` in arm 4 and `= 0` in arm 5, its clip pair
/// `+= 1` in arm 6, and in arm 11 both the caster's and the summon seat's
/// `+0x1DD = ` [`TARGET_CODE_ENEMY_ROW`] with the original victim index
/// latched into the module word `0x801F9200`. Arm `0xFF` restores the caster's
/// `+0x1DD` from that word (`0x801F8124`).
///
/// The packet arms run too, against the summon seat's live position and
/// facing (`geom`): arm 8 seeds and arm 9 drains the prong countdown
/// (`scalar * 0x78`, drawing two lightning prongs a pass while it is
/// non-negative), arm 10 empties the trail, arm 11 draws the charge beam
/// while its ramp holds and the first trail sample on its exit, arm 12 draws
/// and samples the sweeping beam, and arm 13 retracts the trail and holds
/// until it is gone. What each tick drew is left in [`TheederFx::packet`]
/// for the hosts ([`TheederPacket`]).
///
/// Arm 7 holds until the clip arm 6 staged has committed and taken its
/// loop count off the `0xFF` sentinel ([`THEEDER_ATTACK_ARM`]); arm 14 polls
/// the monster row once the director's settle word has run out and stores
/// `0xFF`. The countdown gates of arms 2..6 and the camera arms are the
/// director's (`cast_module_camera::theeder_direct`); the cone membership is
/// the host's (it supplies which seats are in it).
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0904; phase chain, arm 7's clip wait, the swinging-ray sweep, the packet arms and arm 14's row poll; countdown and camera arms in theeder_direct)
pub fn theeder_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    geom: TheederGeom,
    fx: &mut TheederFx,
    mut rolls: impl FnMut(u8) -> Option<i32>,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let site = SERU_HIT_SITES[1];
    fx.packet = None;
    let mouth = theeder_mouth(geom.x, geom.z, geom.facing);
    let step = run_chain(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        THEEDER_RISE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = THEEDER_RISE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        THEEDER_SHOW_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        THEEDER_STAGE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        // `lbu v1,0x21b(s4); li v0,0xff; beq v1,v0,<epilogue>` at
        // `0x801F71C4`: the arm waits for the clip arm 6 staged to commit
        // and reach its window - the commit copies the clip's loop count
        // `0xFF` into `+0x21B`, and the window arm of `FUN_80047430` takes
        // it off the sentinel the first time the cursor reaches the park
        // frame (clip 1 parks on frame 17). Before the commit the seat still
        // reads the previous clip's count, which the bump of arm 6 replaces
        // on the anim pass after it; the port waits for the commit
        // explicitly so its own pass order cannot let the idle's `0` through.
        THEEDER_ATTACK_ARM => {
            let committed = seats
                .get(who.summon as usize)
                .is_none_or(|s| s.playing_anim == s.staged_anim);
            if !committed || geom.clip_loops == THEEDER_LOOP_SENTINEL {
                return CastArmStep::Hold;
            }
            CastArmStep::Advance
        }
        THEEDER_PRONG_SEED_ARM => {
            c.ctx_6d8 = (THEEDER_PRONG_COUNT * crate::cast_module_camera::SPEED_SCALAR) as u16;
            CastArmStep::Advance
        }
        THEEDER_PRONG_ARM => {
            c.ctx_6d8 = c
                .ctx_6d8
                .wrapping_sub(crate::cast_module_camera::MODULE_DRAIN_PER_TICK as u16);
            if (c.ctx_6d8 as i16) < 0 {
                return CastArmStep::Advance;
            }
            fx.cell_seed = fx.cell_seed.wrapping_add(1);
            fx.packet = Some(TheederPacket::Prongs {
                mouth,
                tips: theeder_prong_tips(geom.x, geom.z, geom.facing),
                cells: fx.prong_cells(),
            });
            CastArmStep::Hold
        }
        THEEDER_RAMP_RESET_ARM => {
            c.ctx_6d8 = 0;
            // `sw zero,..` to the trail count `0x801F91FC` beside it.
            fx.trail.reset();
            CastArmStep::Advance
        }
        THEEDER_RETARGET_ARM => {
            // The ramp holds the arm until `ctx+0x6D8` reaches `0x100`; the
            // exit retargets both seats and zeroes the word for the sweep
            // (`sh zero,0x0(s8)` in the `j 0x801F7F04` delay slot).
            c.ctx_6d8 = c
                .ctx_6d8
                .wrapping_add(theeder_ramp_per_tick(THEEDER_RETARGET_ARM));
            if (c.ctx_6d8 as i16) < THEEDER_RETARGET_RAMP_END {
                fx.packet = Some(TheederPacket::Charge {
                    mouth,
                    tip: theeder_charge_tip(mouth, geom.facing),
                    level: c.ctx_6d8,
                });
                return CastArmStep::Hold;
            }
            // The exit frame draws the full beam once (`FUN_801F8634`).
            let tip = theeder_ray_rest_tip(mouth, geom.facing);
            let drawn = fx.trail.sample(tip);
            fx.packet = Some(TheederPacket::Sweep { mouth, tip, drawn });
            for slot in [who.caster, who.summon] {
                if let Some(s) = seats.get_mut(slot as usize) {
                    s.target_code = TARGET_CODE_ENEMY_ROW;
                }
            }
            c.ctx_6d8 = 0;
            CastArmStep::Advance
        }
        THEEDER_SWEEP_ARM => {
            c.ctx_6d8 = c
                .ctx_6d8
                .wrapping_add(theeder_ramp_per_tick(THEEDER_SWEEP_ARM));
            let tip = theeder_ray_tip(mouth, geom.facing, c.ctx_6d8);
            let drawn = fx.trail.sample(tip);
            fx.packet = Some(TheederPacket::Sweep { mouth, tip, drawn });
            for seat in FIRST_MONSTER_SEAT..MONSTER_ROW_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || v.playing_anim != 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let Some(roll) = rolls(seat) else {
                    continue;
                };
                let applied = site.apply(v, roll);
                v.render_flag = 0;
                // The two mirrors the arm stamps beside the HP write
                // (`0x801F7D9C` / `0x801F7DB4`): the presentation word `+0x04`
                // and the render byte `+0x21F`.
                v.present_04 = THEEDER_HIT_PRESENT_WORD;
                v.render_21f = THEEDER_HIT_RENDER_21F;
                stage_reaction_bits(v);
                hits.push(SweepHit { seat, applied });
            }
            // The ray turns a full circle, one pass of hits per tick; a seat
            // already reacting (`+0x1D9 != 0`) is passed over, which is what
            // keeps a seat from being struck twice.
            if (c.ctx_6d8 as i16) < THEEDER_SWEEP_END {
                CastArmStep::Hold
            } else {
                CastArmStep::Advance
            }
        }
        THEEDER_RETRACT_ARM => {
            let drawn = fx.trail.drawn();
            fx.packet = Some(TheederPacket::Retract { mouth, drawn });
            if fx.trail.retract() != 0 {
                CastArmStep::Hold
            } else {
                CastArmStep::Advance
            }
        }
        // The row poll at `0x801F8030..0x801F80DC`, behind the settle
        // word's drain (the director's, `theeder_direct`): every monster
        // seat `3 .. 3 + ctx[+0x01]` alive and back on idle (`+0x1D9 == 0`)
        // or dead with its node's `+0x74` colour faded out, then `0xFF`.
        THEEDER_SETTLE_ARM => {
            let end = (FIRST_MONSTER_SEAT as usize + c.monster_count as usize)
                .min(MONSTER_ROW_END as usize);
            let settled = seats
                .get(FIRST_MONSTER_SEAT as usize..end.min(seats.len()))
                .unwrap_or(&[])
                .iter()
                .all(|v| {
                    if v.hp != 0 {
                        v.playing_anim == 0
                    } else {
                        v.present_04 & 0x00FF_FFFF == 0
                    }
                });
            if settled {
                c.phase = CHOREOGRAPHY_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => CastArmStep::Finish,
        p if p <= THEEDER_SETTLE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

/// The summon seat's live `(x, z)` and facing - slot 7's `+0x34` / `+0x38`
/// / `+0x46`, which every PROT 0904 packet arm builds its points from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TheederGeom {
    pub x: i16,
    pub z: i16,
    pub facing: u16,
    /// The summon seat's clip loop count `+0x21B` (the playing clip's
    /// remaining loop-window cycles) - what arm 7 waits on.
    pub clip_loops: u8,
}

/// The arm PROT 0904 waits in for the creature's attack clip
/// (`0x801F71C4`).
pub const THEEDER_ATTACK_ARM: u8 = 7;

/// The `+0x21B` value arm 7 holds on.
pub const THEEDER_LOOP_SENTINEL: u8 = 0xFF;

/// The engine ticks a module once a vsync, and retail runs it once a battle
/// frame of `step` vsyncs; the trail samples (and retracts) once per retail
/// pass at the measured frame step of `4`, so its fan spans the same arc of
/// the swing as retail's.
pub const THEEDER_TRAIL_PASS_TICKS: u8 = 4;

/// PROT 0904's beam trail: the ring of past ray tips at `0x801F90FC` and its
/// count `0x801F91FC`.
///
/// `FUN_801F8634` shifts the ring one place, writes the new tip at `[0]`,
/// draws a fan quad between each consecutive pair (using the count from
/// before the shift) and only then bumps the count, capped at `0xF`.
/// `FUN_801F8B84` draws the same fan without sampling and drops the count by
/// one, returning what is left; arm 13 holds while that is non-zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TheederTrail {
    /// `hist[0]` is the newest tip.
    pub hist: [[i16; 3]; THEEDER_TRAIL_CAP + 1],
    /// The count word.
    pub n: usize,
    /// Engine ticks into the current retail pass.
    pass: u8,
}

impl TheederTrail {
    /// Arm 10's reset of the count word.
    pub fn reset(&mut self) {
        self.n = 0;
        self.pass = 0;
    }

    /// `FUN_801F8634`'s ring half. On a pass boundary the ring shifts and
    /// the count grows; between boundaries the newest slot tracks the live
    /// tip. Returns the fan entries this call draws: pairs `hist[i]`,
    /// `hist[i + 1]` for `i < drawn`.
    pub fn sample(&mut self, tip: [i16; 3]) -> usize {
        let on_pass = self.pass == 0;
        self.pass = (self.pass + 1) % THEEDER_TRAIL_PASS_TICKS;
        if !on_pass {
            self.hist[0] = tip;
            return self.n.saturating_sub(1);
        }
        let drawn = self.n;
        for i in (1..=self.n.min(THEEDER_TRAIL_CAP)).rev() {
            self.hist[i] = self.hist[i - 1];
        }
        self.hist[0] = tip;
        self.n = (self.n + 1).min(THEEDER_TRAIL_CAP);
        drawn
    }

    /// The fan entries a retract draws (the whole count).
    pub fn drawn(&self) -> usize {
        self.n
    }

    /// `FUN_801F8B84`'s count drop, once a pass. Returns what is left.
    pub fn retract(&mut self) -> usize {
        let on_pass = self.pass == 0;
        self.pass = (self.pass + 1) % THEEDER_TRAIL_PASS_TICKS;
        if on_pass {
            self.n = self.n.saturating_sub(1);
        }
        self.n
    }
}

/// PROT 0904's per-cast draw state: the trail, the last tick's packet, and
/// the counter the prong texture cells are picked from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TheederFx {
    pub trail: TheederTrail,
    /// What the last tick drew, `None` on a tick that drew nothing.
    pub packet: Option<TheederPacket>,
    /// Arm 4's seat placement `(x, z, facing)`
    /// ([`theeder_seat_placement`]); retail never moves the seat again.
    pub seat: Option<(i16, i16, u16)>,
    cell_seed: u32,
}

impl TheederFx {
    /// The two prongs' 32-texel texture cells. Retail picks each from
    /// `rand() & 3` (`0x801F830C`, a second draw discarded); the port takes
    /// them from a per-cast counter so a cast's visuals never move the
    /// battle's RNG stream.
    fn prong_cells(&self) -> [u8; 2] {
        let h = self.cell_seed.wrapping_mul(0x9E37_79B9);
        [((h >> 13) & 3) as u8, ((h >> 21) & 3) as u8]
    }
}

/// The `+0x04` mesh-tint word PROT 0904's ring sweep stamps on every seat it
/// hits.
pub const THEEDER_HIT_PRESENT_WORD: u32 = 0x03FF_0000;

/// The `+0x21F` render byte the same hit writes.
pub const THEEDER_HIT_RENDER_21F: u8 = 2;

/// Half-width of PROT 0904's ring-sweep **cone**, in 12-bit angle units
/// (`addiu v0, v0, -0x30` at `0x801F7D0C`).
///
/// A seat is hit when the bearing difference between it and the ring's rim is
/// within this of zero **modulo a turn**: retail subtracts `0x30` from the
/// absolute difference and compares the result **unsigned** against `0xFB1`,
/// so a difference under `0x30` underflows past that bound and a difference at
/// or above `0xFE1` exceeds it. Both ends of the cone are in.
///
/// The host supplies the membership ([`crate::cast_module_ticks`] consumers
/// call `World::seats_in_cone`); this constant is here so the two sides quote
/// one number.
pub const THEEDER_CONE_HALF_WIDTH: u16 = 0x30;

// --- PROT 0904 geometry: the points the arms build and the packets draw ---

/// How far ahead of the summon seat the beam's root sits (`* 0x5C` against
/// the facing's sine / cosine, e.g. `0x801F7B0C..0x801F7B30`).
pub const THEEDER_MOUTH_REACH: i32 = 0x5C;

/// The beam root's height (`li v0,-0x11d` at `0x801F7B7C`; retail Y is down).
pub const THEEDER_MOUTH_Y: i16 = -0x11D;

/// Arm 9's prong tips: `0xA0` along the facing (`* 0xA0`) ...
pub const THEEDER_PRONG_REACH: i32 = 0xA0;

/// ... and their height (`0xFEB2`).
pub const THEEDER_PRONG_Y: i16 = -0x14E;

/// What arm 8 seeds arm 9's countdown with, per unit of speed scalar
/// (`*puVar14 = scalar * 0x78`).
pub const THEEDER_PRONG_COUNT: i32 = 0x78;

/// The arm the two lightning prongs draw in (`FUN_801F815C` twice a pass)
/// while its countdown drains.
pub const THEEDER_PRONG_ARM: u8 = 9;

/// The arm that seeds [`THEEDER_PRONG_ARM`]'s countdown.
pub const THEEDER_PRONG_SEED_ARM: u8 = 8;

/// The arm the trail retracts in (`FUN_801F8B84`, held while it returns
/// non-zero).
pub const THEEDER_RETRACT_ARM: u8 = 13;

/// Capacity of the sweep's trail ring at `0x801F90FC` (the count word
/// `0x801F91FC` is capped at `0xF`, `sltiu v0,v0,0x10` at `0x801F8974`, and
/// the shift writes one past it).
pub const THEEDER_TRAIL_CAP: usize = 15;

/// `(t * k) / 4096` truncated toward zero - the `bgez` / `addiu 0xfff` /
/// `sra 0xc` idiom every point in the module is built with.
fn scale_q12(t: i16, k: i32) -> i16 {
    ((i32::from(t) * k) / 4096) as i16
}

/// The beam root, `[x, y, z]` in retail battle space: `THEEDER_MOUTH_REACH`
/// ahead of the summon seat `(x, z)` along `facing`, at
/// [`THEEDER_MOUTH_Y`].
pub fn theeder_mouth(x: i16, z: i16, facing: u16) -> [i16; 3] {
    let (s, c) = legaia_engine_system::trig::trig12(facing);
    [
        x.wrapping_add(scale_q12(s, THEEDER_MOUTH_REACH)),
        THEEDER_MOUTH_Y,
        z.wrapping_add(scale_q12(c, THEEDER_MOUTH_REACH)),
    ]
}

/// Arm 12's ray tip at sweep word `c` (`ctx+0x6D8` after the arm's ramp),
/// read off `0x801F7B90..0x801F7C80`.
///
/// The ray does **not** turn a full circle: its bearing is the summon's
/// facing plus `sin[c & 0xFFF] / 16`, a swing of `+-0x100` (22.5 degrees)
/// about the facing over one turn of `c`. Its length is `2/3` of the unit
/// circle (`* 0x55555556` on the doubled sample) and its height bobs by
/// `sin[(c * 2) & 0xFFE] / 16` about [`THEEDER_MOUTH_Y`].
pub fn theeder_ray_tip(mouth: [i16; 3], facing: u16, c: u16) -> [i16; 3] {
    let swing = legaia_engine_system::trig::sin12(c & 0x0FFF) / 16;
    let bearing = facing.wrapping_add(swing as u16) & 0x0FFF;
    let (s, co) = legaia_engine_system::trig::trig12(bearing);
    let bob = legaia_engine_system::trig::sin12(c.wrapping_mul(2) & 0x0FFE) / 16;
    [
        mouth[0].wrapping_add(((i32::from(s) * 2) / 3) as i16),
        bob.wrapping_add(THEEDER_MOUTH_Y),
        mouth[2].wrapping_add(((i32::from(co) * 2) / 3) as i16),
    ]
}

/// Arm 11's short charge beam: the root plus `sin / 8` along the facing
/// (`0x801F77C0..`, `addiu 7` / `sra 3`), both at [`THEEDER_MOUTH_Y`].
pub fn theeder_charge_tip(mouth: [i16; 3], facing: u16) -> [i16; 3] {
    let (s, c) = legaia_engine_system::trig::trig12(facing);
    [
        mouth[0].wrapping_add(s / 8),
        THEEDER_MOUTH_Y,
        mouth[2].wrapping_add(c / 8),
    ]
}

/// Arm 11's exit frame: the full-length ray straight down the facing
/// (`2/3` of the unit circle), the first sample the trail takes.
pub fn theeder_ray_rest_tip(mouth: [i16; 3], facing: u16) -> [i16; 3] {
    let (s, c) = legaia_engine_system::trig::trig12(facing);
    [
        mouth[0].wrapping_add(((i32::from(s) * 2) / 3) as i16),
        THEEDER_MOUTH_Y,
        mouth[2].wrapping_add(((i32::from(c) * 2) / 3) as i16),
    ]
}

/// Where arm 4 seats Theeder once `FUN_801F19EC` has installed it
/// (`0x801F6EEC..0x801F6F8C`): facing `atan2(victim -> caster) + 0x800`, i.e.
/// from the caster toward the victim, and standing half a unit
/// (`sin / 2`, `cos / 2`, truncated toward zero) short of the victim along
/// that line - between the two, facing the victim. Returns `(x, z, facing)`.
/// No later arm moves the seat; the arm-12 beam (`2/3` of a unit from a root
/// `0x5C` ahead) is what reaches the row.
pub fn theeder_seat_placement(victim: (i16, i16), caster: (i16, i16)) -> (i16, i16, u16) {
    let facing = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
        victim.1, victim.0, caster.1, caster.0,
    )
    .wrapping_add(0x800)
        & 0x0FFF;
    let (s, c) = legaia_engine_system::trig::trig12(facing);
    (
        victim.0.wrapping_sub(s / 2),
        victim.1.wrapping_sub(c / 2),
        facing,
    )
}

/// Arm 9's two prong tips about the summon seat `(x, z)`: `0xA0` along the
/// facing plus `0x5C` a quarter turn to either side, at
/// [`THEEDER_PRONG_Y`] (`0x801F7600..0x801F7700`, two `FUN_801F815C` calls).
pub fn theeder_prong_tips(x: i16, z: i16, facing: u16) -> [[i16; 3]; 2] {
    let (s, c) = legaia_engine_system::trig::trig12(facing);
    let side = |turn: u16| {
        let (ss, sc) = legaia_engine_system::trig::trig12(turn & 0x0FFF);
        [
            x.wrapping_add(scale_q12(s, THEEDER_PRONG_REACH))
                .wrapping_add(scale_q12(ss, THEEDER_MOUTH_REACH)),
            THEEDER_PRONG_Y,
            z.wrapping_add(scale_q12(c, THEEDER_PRONG_REACH))
                .wrapping_add(scale_q12(sc, THEEDER_MOUTH_REACH)),
        ]
    };
    [
        side(facing.wrapping_add(0x400)),
        side(facing.wrapping_sub(0x400)),
    ]
}
