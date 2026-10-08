//! The trampoline-reached tick bodies of PROT 0938 / 0951 / 0952 / 0965.
//! Split out of `cast_module_ticks.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The twelve trampoline-reached tick bodies (PROT 0938 / 0951 / 0952 / 0955 /
// 0965)
// ---------------------------------------------------------------------------

/// What one phase arm did, in the three shapes the band's arms come in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastArmStep {
    /// The arm ran and performed the `ctx[+0x279] += 1` store.
    Advance,
    /// The arm ran (or the phase named none) and left the phase alone; the
    /// busy register is still `1`.
    Hold,
    /// The terminal arm: it zeroed the busy register, so the tick reports
    /// [`CastTickStep::Done`].
    Finish,
}

/// Return convention of every body in this section, read off the bytes rather
/// than assumed: each opens by seeding a saved register with `1` and returns
/// it, and **only** a terminal arm zeroes it. So a phase the dispatch does not
/// name - including one past a `sltiu` bound - returns **Busy**, not Done.
///
/// `0x801F6AA4` in PROT 0949's tick is the exemplar: its out-of-bound `beqz`
/// jumps to `0x801F758C`, one instruction *past* the `move s7, zero` at
/// `0x801F7588`, so `s7` is still `1` there. [`run_tick`] models an
/// out-of-bound phase as [`CastTickStep::Done`] instead; the bodies below use
/// this helper so their reported step is the register's.
pub(super) fn run_tick_latched(
    ctx: &mut CastModuleCtx,
    arm: impl FnOnce(&mut CastModuleCtx) -> CastArmStep,
) -> CastTickStep {
    match arm(ctx) {
        CastArmStep::Advance => {
            advance_phase(ctx);
            CastTickStep::Busy
        }
        CastArmStep::Hold => CastTickStep::Busy,
        CastArmStep::Finish => CastTickStep::Done,
    }
}

/// `+0x1DE == 1` - the Item action category, the only one the turn-steal
/// refunds.
pub const ACTION_CATEGORY_ITEM: u8 = 1;

/// The band's **turn-steal** idiom, shared by PROT 0955's Kiss of Death miss
/// arm (`0x801F8CF4..0x801F8D54`) and its Terror Scream arm 3
/// (`0x801F7E18..0x801F7E4C`).
///
/// ```text
/// if victim[+0x1DE] == 1 && victim[+0x16C] != 0 { FUN_800421D4(victim[+0x1DF], 1) }
/// victim[+0x1DE] = 0
/// if victim[+0x16C] != 0 { ctx[+0x1A] += 1 ; victim[+0x16C] = 0 }
/// ```
///
/// `FUN_800421D4` is the inventory find-or-insert
/// (`docs/subsystems/inventory.md`), so the refund only makes sense for an
/// Item action - which is exactly what `+0x1DE == 1` names. `+0x16C` is the
/// per-round initiative key, and clearing it is what "the victim has already
/// acted" means to the next-actor selector; `ctx[+0x1A]` is the turn cursor.
///
/// Returns `Some(item_id)` when retail refunds an item, so a host can hand it
/// back through its own bag.
pub fn steal_turn(ctx: &mut CastModuleCtx, victim: &mut CastActorState) -> Option<u8> {
    let had_turn = victim.init_key != 0;
    let refund = (victim.action_category == ACTION_CATEGORY_ITEM && had_turn)
        .then_some(victim.queued_action);
    victim.action_category = 0;
    if had_turn {
        ctx.turn_cursor = ctx.turn_cursor.wrapping_add(1);
        victim.init_key = 0;
    }
    refund
}

/// The band's reaction-stage pick: `+0x1F2` decides between the knockdown
/// clip `+0x1F1` and the alternate `+0x1EF`.
///
/// PROT 0955's Kiss of Death (`0x801F8D90`) and Melt Spray (`0x801F856C`)
/// both spell it as `if +0x1F2 != 0 { +0x1DA = +0x1F1 } else { +0x1DA = +0x1EF }`;
/// the battle overlay's own effect-child arm (`0x801E196C`) adds the third
/// leg - a dead victim takes `+0x1F1` regardless, and a zero `+0x1EF` falls
/// on to `+0x1F0`.
pub fn stage_reaction(victim: &mut CastActorState) {
    victim.staged_anim = if victim.reaction_gate != 0 {
        victim.knockdown_anim
    } else {
        victim.reaction_alt
    };
}

/// The `0xFF` phase every `beq`/`slti` body in this section ends on.
pub const CHOREOGRAPHY_DONE_PHASE: u8 = 0xFF;

/// One seat's outcome inside a tick body's whole-row sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepHit {
    /// The seat the wrapper was called for (the wrapper's `a2`).
    pub seat: u8,
    /// The damage actually applied after the clamp.
    pub applied: u32,
}

// --- PROT 0938 -------------------------------------------------------------

/// The baked power PROT 0938's `0x4E` body hands `FUN_801DD4B0`
/// (`addiu a0,zero,0x274` at `0x801F77B0`).
pub const CHAOS_BREATH_POWER: u16 = 0x274;
/// The clip PROT 0938's arm 0 stages on the caster
/// (`addiu v1,zero,9; sb v1,0x1da(s3)` at `0x801F73C8`).
pub const CHAOS_BREATH_ARM0_CLIP: u8 = 9;
/// The gauge budget PROT 0938's `0x4E` sweep arms for arm 3 to drain out of
/// the caster's `+0x170` (`li v0,0x32` at `0x801F7880`) - the same `0x32`
/// the monster `0x8A` pick clamps the gauge to when it fires the breath, so
/// a cast leaves the gauge at zero.
pub const CHAOS_BREATH_GAUGE_DRAIN: u16 = 0x32;
/// `+0x16E` bit `0x1` - **Venom** (`docs/subsystems/battle-formulas.md`).
pub const FLAG_VENOM: u16 = 0x0001;
/// `+0x16E` bit `0x2` - **Toxic**.
pub const FLAG_TOXIC: u16 = 0x0002;

/// PROT 0938 (Chaos Breath) tick body - action id `0x4E`.
///
/// A `beq`/`slti` chain over phases `0`, `1`, `2`, `3` and `0xFF`
/// (`0x801F72EC..0x801F7334`), caster `s3 = actor_table[ctx+0x13]`.
///
/// * arm `0` (`0x801F733C`) - cue `0x156`, face the caster
///   (`+0x46 = angle + 0x800`), stage clip [`CHAOS_BREATH_ARM0_CLIP`] with a
///   `+0x1DC` bump, and **halve** the caster's animation rate
///   (`+0x21D >>= 1` at `0x801F73E4`);
/// * arm `2` (`0x801F7750`) - the **sweep**, below;
/// * arm `0xFF` (`0x801F7A04`) - `+0x21D <<= 1` restores the rate and zeroes
///   the busy register, so this is the only arm that reports
///   [`CastTickStep::Done`].
///
/// The sweep walks `actor_table[0 .. ctx[+0]]`, skips a dead seat and a
/// `+0x16E & 4` one, then per hittable seat:
/// `FUN_801DD4B0(0x274, ctx[+0x13], seat)` then the **shape-A** clamp
/// (`sltu a0,s1` at `0x801F77EC`, kill-capable), `+0x10 +=`, `+0x14C -=`,
/// `+0x1DA = +0x1F1`, `+0x1DC = 1`, face away from the caster,
/// `+0x04 = 0x3FF04040`, then two 1-in-8 rolls: the first sets
/// [`FLAG_VENOM`], and only if that one misses does a second roll set
/// [`FLAG_TOXIC`] (`0x801F7888..0x801F78D8`).
///
/// **This is a whole-row applier that kills.** `docs/subsystems/cast-module.md`
/// called `0x801F85A8` / `0x801F8D64` "the band's only whole-row appliers" and
/// paired whole-row with the never-kill `HP - 1` clamp; this body, PROT 0938's
/// `0xB7` body and PROT 0965's are three more, and all three take shape A.
///
/// `rolls` supplies one wrapper result per hittable seat in seat order, and
/// `status` one `rand()` pair per hittable seat, so the caller keeps retail's
/// RNG cursor. Arm `3` is frame-gated, but it is not only presentation: it
/// drains [`CHAOS_BREATH_GAUGE_DRAIN`] out of the caster's `+0x170` gauge,
/// which the port folds at the sweep. Not ported: the packet and camera
/// arms, and arm `1`'s wait.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F726C (PROT 0938; phase chain + the whole-row damage/status sweep; packet arms unported)
pub fn chaos_breath_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    seats: &mut [CastActorState],
    mut rolls: impl FnMut(u8) -> i32,
    mut status: impl FnMut(u8) -> (u32, u32),
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_tick_latched(ctx, |c| match c.phase {
        0 => {
            caster.staged_anim = CHAOS_BREATH_ARM0_CLIP;
            caster.restage = caster.restage.wrapping_add(1);
            caster.anim_rate >>= 1;
            CastArmStep::Advance
        }
        2 => {
            for seat in 0..c.party_count {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if !aoe_seat_is_hittable(v) {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, rolls(seat));
                let knockdown = v.knockdown_anim;
                v.staged_anim = knockdown;
                v.restage = 1;
                let (a, b) = status(seat);
                if a & 7 == 0 {
                    v.flags |= FLAG_VENOM;
                } else if b & 7 == 0 {
                    v.flags |= FLAG_TOXIC;
                }
                hits.push(SweepHit { seat, applied });
            }
            // Every hit seat re-arms the module timer to the drain budget
            // (`li v0,0x32; sw v0,-0x7FC0(0x8020)` at `0x801F7880`), and arm 3
            // spends that budget out of the caster's own gauge, a frame step
            // at a time (`0x801F793C..0x801F7978`). Nothing reads the gauge
            // before arm 3 has drained it, so the port takes the whole budget
            // here. Without it the gauge the `0x8A` pick clamped to `0x32`
            // stays above its `0x31` gate and the breath fires every turn.
            if !hits.is_empty() {
                caster.spirit_gauge = caster.spirit_gauge.wrapping_sub(CHAOS_BREATH_GAUGE_DRAIN);
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => {
            caster.anim_rate = caster.anim_rate.wrapping_shl(1);
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, hits)
}

/// The baked power PROT 0938's `0xB7` body hands `FUN_801DD4B0`
/// (`addiu a0,zero,0x309` at `0x801F70DC`, in the `beqz` delay slot).
pub const MYSTIC_CIRCLE_POWER: u16 = 0x309;
/// PROT 0938's `0xB7` terminal arm: table word 4, the only one that zeroes
/// the busy register (`clear s6` at `0x801F7230`).
pub const MYSTIC_CIRCLE_DONE_ARM: u8 = 4;
/// The animation rate PROT 0938's `0xB7` sweep drops each hit seat to
/// (`addiu v0,zero,4; sb v0,0x21d(v1)` at `0x801F7180`).
pub const MYSTIC_CIRCLE_HIT_ANIM_RATE: u8 = 4;

/// PROT 0938 (Mystic Circle) tick body - action id `0xB7`.
///
/// Five phase arms behind `sltiu v1, 5` (`0x801F6A78`) through a word table at
/// `0x801F69D8` - the image head, which ends exactly where this function
/// opens. The damage arm sweeps `actor_table[0 .. ctx[+0]]` and skips **only**
/// a dead seat: unlike every other sweep in the band it does *not* test
/// `+0x16E & 4`, so a Stoned seat is still hit (`0x801F70D0`).
///
/// Per hittable seat: `FUN_801DD4B0(0x309, ctx[+0x13], seat)`, the shape-A
/// clamp at `0x801F7118`, `+0x10 +=`, `+0x14C -=`, `+0x1DA = +0x1F1`,
/// `+0x1DC += 1`, `+0x21D = 4`, face away, `+0x04 = 0x3FF80300`.
///
/// This body is on no `--missing-ports` row only because no dump prints at
/// its VA; it is named by PROT 0938's trampoline `0x801F7A40`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69EC (PROT 0938; phase table + the whole-row damage sweep; packet arms unported)
pub fn mystic_circle_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    damage_arm: bool,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_tick_latched(ctx, |c| {
        // Table word 4 is the terminal arm: it waits out the module timer,
        // clears `ctx[+0x0D]` and returns `0` with the phase left at 4
        // (`0x801F71F8..0x801F7230`). Advancing past it walks off the
        // `sltiu 5` table into the busy default, which parks the band.
        if c.phase == MYSTIC_CIRCLE_DONE_ARM {
            c.ctx_0d = 0;
            return CastArmStep::Finish;
        }
        if damage_arm {
            for seat in 0..c.party_count {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                // The one sweep in the band with no `+0x16E & 4` guard.
                if v.hp == 0 {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, rolls(seat));
                let knockdown = v.knockdown_anim;
                stage_clip(v, knockdown);
                v.anim_rate = MYSTIC_CIRCLE_HIT_ANIM_RATE;
                hits.push(SweepHit { seat, applied });
            }
        }
        CastArmStep::Advance
    });
    (step, hits)
}

// --- PROT 0951 -------------------------------------------------------------

/// PROT 0951's twelve-arm phase table, at `0x801F69D8` (file `0x00..0x30`).
pub const CHAOS_FLARE_ARMS: u16 = 0x0C;
/// The baked power PROT 0951's `0x36` body hands `FUN_801DD4B0`
/// (`addiu a0,zero,0x3a0` at `0x801F7404`).
pub const CHAOS_FLARE_POWER: u16 = 0x3A0;

/// PROT 0951 (Chaos Flare) tick body - action id `0x36`.
///
/// Twelve phase arms behind `sltiu v1, 0x0C` (`0x801F6AB4`) through the table
/// at `0x801F69D8`; caster `s4`, victim `s1 = actor_table[caster[+0x1DD]]`.
/// One damage site - `FUN_801DD4B0(0x3A0, ctx[+0x13], victim_seat)` at
/// `0x801F7414` with the shape-A clamp at `0x801F7438` - then `+0x10 +=`,
/// `+0x14C -=`, face the victim away and stage its `+0x1F1` with a `+0x1DC`
/// bump. The routine also writes `ctx[+0x278]` (`0x801F71F0` seeds it,
/// `0x801F76D4` clears it), `ctx[+0x0D]` (`0x801F6B08` and the terminal arm's
/// `0x801F77A4`) and eight `+0x21D` stores across the two seats.
///
/// Not ported: the packet and camera arms, and the per-arm frame gating.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A20 (PROT 0951; phase table + damage/staging; packet arms unported)
pub fn chaos_flare_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick_latched(ctx, |c| {
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        }
        if u16::from(c.phase) >= CHAOS_FLARE_ARMS {
            // Out of the table's range: retail's `beqz` lands past the busy
            // register's only clearing store, so the tick still reports busy.
            CastArmStep::Hold
        } else if u16::from(c.phase) == CHAOS_FLARE_ARMS - 1 {
            // Arm 11 (`0x801F76F4`) is terminal: it never advances the phase,
            // and once the module countdown runs out it clears the busy
            // register and `ctx[+0x0D]` (`0x801F77A0` / `0x801F77A4`).
            // Advancing past it walked off the table into the hold above,
            // which parked the band - and the battle - for good.
            c.ctx_0d = 0;
            CastArmStep::Finish
        } else {
            CastArmStep::Advance
        }
    })
}

/// PROT 0951's second phase table - six arms at `0x801F6A08`, ending exactly
/// where the `0x5B` body opens.
pub const SCYTHE_WIND_ARMS: u16 = 6;
/// The baked power PROT 0951's `0x5B` body hands `FUN_801DD4B0`.
///
/// It is set in the call's **delay slot** (`jal 0x801DD4B0` at `0x801F7F88`,
/// `addiu a0,zero,0x80` at `0x801F7F8C`), so a scan that only looks backwards
/// from a `jal` reports no constant for this site.
pub const SCYTHE_WIND_POWER: u16 = 0x80;

/// PROT 0951 (Scythe Wind) tick body - action id `0x5B`.
///
/// Six phase arms behind `sltiu v1, 6` (`0x801F7874`) through the table at
/// `0x801F6A08`. The damage arm writes the victim's animation rate back to
/// [`ANIM_RATE_NORMAL`] (`0x801F7F74`) before the call, then
/// `FUN_801DD4B0(0x80, ctx[+0x13], victim_seat)`, the shape-A clamp at
/// `0x801F7FAC`, `+0x1DC = 1`, `+0x1DA = +0x1F1`, `+0x10 +=`, `+0x14C -=` and
/// the face-away store.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F77E8 (phase table + damage/staging; packet arms unported)
pub fn scythe_wind_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick_latched(ctx, |c| {
        if let Some(roll) = hit {
            victim.anim_rate = ANIM_RATE_NORMAL;
            apply_hit_floor_zero(victim, roll);
            victim.restage = 1;
            let knockdown = victim.knockdown_anim;
            victim.staged_anim = knockdown;
        }
        if u16::from(c.phase) >= SCYTHE_WIND_ARMS {
            CastArmStep::Hold
        } else if u16::from(c.phase) == SCYTHE_WIND_ARMS - 1 {
            // Arm 5 (`0x801F801C`) is terminal: no phase store, and once the
            // countdown runs out and the victim has settled it clears the
            // busy register and `ctx[+0x0D]` (`0x801F8138` / `0x801F813C`).
            c.ctx_0d = 0;
            CastArmStep::Finish
        } else {
            CastArmStep::Advance
        }
    })
}

// --- PROT 0952 -------------------------------------------------------------

/// PROT 0952's seven-arm phase table for the `0x5C` body, at `0x801F69F0` -
/// it ends where the module's *other* tick body (`0x801F6A0C`, Astral Slash)
/// opens.
pub const BLOODY_HORNS_ARMS: u16 = 7;
/// The baked power PROT 0952's `0x5C` body hands `FUN_801DD6B4`
/// (`addiu a0,zero,0x1d0` at `0x801F792C`, six instructions ahead of the
/// `jal` and past two intervening stores).
pub const BLOODY_HORNS_POWER: u16 = 0x1D0;

/// PROT 0952 (Bloody Horns) tick body - action id `0x5C`.
///
/// Seven phase arms behind `sltiu v1, 7` (`0x801F71A4`) through the table at
/// `0x801F69F0`. The damage arm clears three presentation fields on the
/// **caster** first - `+0x21B = 0`, `+0x1DA = 0`, `+0x176 = 0`
/// (`0x801F7930..0x801F7940`) - and the victim's `+0x36 = 0` /
/// `+0x21D = 8`, then calls `FUN_801DD6B4(0x1D0, ctx[+0x13], victim_seat)`,
/// clamps shape A at `0x801F796C`, accumulates `+0x10`, writes `+0x14C`,
/// stages `+0x1F1` with `+0x1DC = 1` and faces the victim away.
///
/// `FUN_801DD6B4` is the ATK wrapper - it mixes the caster's `+0x158` and
/// folds the defender's two defence stats - which is what makes this body's
/// `0x1D0` a physical figure rather than a spell one.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7118 (phase table + damage/staging; packet arms unported)
pub fn bloody_horns_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick_latched(ctx, |c| {
        if let Some(roll) = hit {
            caster.staged_anim = 0;
            victim.anim_rate = ANIM_RATE_NORMAL;
            apply_hit_floor_zero(victim, roll);
            victim.restage = 1;
            let knockdown = victim.knockdown_anim;
            victim.staged_anim = knockdown;
        }
        if u16::from(c.phase) >= BLOODY_HORNS_ARMS {
            CastArmStep::Hold
        } else if u16::from(c.phase) == BLOODY_HORNS_ARMS - 1 {
            // Arm 6 (`0x801F79DC`) is terminal: no phase store, and once the
            // countdown runs out and the victim has settled it clears the
            // busy register and `ctx[+0x0D]` (`0x801F7AF4` / `0x801F7AF8`).
            c.ctx_0d = 0;
            CastArmStep::Finish
        } else {
            CastArmStep::Advance
        }
    })
}

// --- PROT 0965 -------------------------------------------------------------

/// The baked power PROT 0965's `0xB6` body hands `FUN_801DD4B0`
/// (`addiu a0,zero,0x600` at `0x801F77A8`, in the `beqz` delay slot).
pub const DOOMSDAY_POWER: u16 = 0x600;

/// PROT 0965 (Doomsday) tick body - action id `0xB6`, and the whole image:
/// the function opens at the module's own load base `0x801F69D8` and runs
/// `0x1144` bytes to `0x801F7B1C`, where the trampoline begins.
///
/// A `beq`/`slti` chain (`0x801F6A58..0x801F6AB4`) over phases `0`, `1`, `2`,
/// `4`, `5`, `6`, `7` and more. Its damage arm is the band's **third**
/// whole-row applier: it walks `actor_table[0 .. ctx[+0]]`, poses each seat
/// (`+0x34`/`+0x38` copied to a scratch strip, `+0x46 = 0x800`), skips a dead
/// one, then `FUN_801DD4B0(0x600, ctx[+0x13], seat)`, the shape-A clamp at
/// `0x801F77E0`, `+0x10 +=`, `+0x14C -=`, `+0x1DA = +0x1F1`, `+0x1DC += 1`.
/// Past the loop it clears `ctx[+0x27A]` and `ctx[+0x278]`
/// (`0x801F789C` / `0x801F78A8`).
///
/// Like Mystic Circle this body is on no `--missing-ports` row only because
/// no dump prints at its VA.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0965; phase chain + the whole-row damage sweep; packet arms unported)
pub fn doomsday_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    damage_arm: bool,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_tick_latched(ctx, |c| {
        // The `0xFF` arm (`0x801F7A08`) restores the seats' poses, clears
        // `ctx[+0x0D]` and returns `0` (`clear s8` at `0x801F7A84`). Without
        // it the phase wrapped to `0` and the choreography - damage arm
        // included - ran again.
        if c.phase == CHOREOGRAPHY_DONE_PHASE {
            c.ctx_0d = 0;
            return CastArmStep::Finish;
        }
        if damage_arm {
            for seat in 0..c.party_count {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, rolls(seat));
                let knockdown = v.knockdown_anim;
                stage_clip(v, knockdown);
                hits.push(SweepHit { seat, applied });
            }
            c.ctx_27a = 0;
            c.ctx_278 = 0;
        }
        CastArmStep::Advance
    });
    (step, hits)
}
