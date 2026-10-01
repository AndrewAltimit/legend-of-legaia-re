//! The six tick bodies, plus PROT 0942 Power Up and PROT 0964 Element Change.
//! Split out of `cast_module_ticks.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The six tick bodies (`0x801CF4EC` / `0x801CF56C` arm, via the trampoline)
// ---------------------------------------------------------------------------

/// One tick body's phase-machine shape, recovered from its dispatch head.
#[derive(Debug, Clone, Copy)]
pub struct CastTickShape {
    /// Extraction PROT entry of the owning image.
    pub prot_entry: u32,
    /// Retail VA of the tick body.
    pub routine: u32,
    /// Phase values the dispatch accepts. A table head bounds with `sltiu`;
    /// a `beq`/`slti` chain head bounds with its widest `slti`.
    pub phase_arms: u16,
    /// `true` when the head is a `jr`-through-table (`sltiu` + word table),
    /// `false` when it is a `beq`/`slti` chain.
    pub table_head: bool,
}

/// The six tick bodies' dispatch shapes, read off each routine's own head.
pub const CAST_TICK_SHAPES: [CastTickShape; 6] = [
    // `sltiu v1, 5` at 0x801F6A98, table at 0x801F69D8.
    CastTickShape {
        prot_entry: 952,
        routine: 0x801F_6A0C,
        phase_arms: 5,
        table_head: true,
    },
    // `beq`/`slti` chain from 0x801F6A98; widest compare `slti v1, 4` plus
    // the `beq v1, 7` head test.
    CastTickShape {
        prot_entry: 957,
        routine: 0x801F_6A14,
        phase_arms: 8,
        table_head: false,
    },
    // `beq`/`slti` chain from 0x801F7A20; widest compare `slti v1, 8`.
    CastTickShape {
        prot_entry: 957,
        routine: 0x801F_798C,
        phase_arms: 8,
        table_head: false,
    },
    // `sltiu v1, 0x100` at 0x801F6E70, 256-word table at 0x801F69D8,
    // default arm 0x801F8CFC.
    CastTickShape {
        prot_entry: 958,
        routine: 0x801F_6DD8,
        phase_arms: 0x100,
        table_head: true,
    },
    // `sltiu v1, 8` at 0x801F6F68, table at 0x801F69D8.
    CastTickShape {
        prot_entry: 945,
        routine: 0x801F_6EDC,
        phase_arms: 8,
        table_head: true,
    },
    // `beq`/`slti` chain from 0x801F7578; widest compare `slti v1, 9`.
    CastTickShape {
        prot_entry: 960,
        routine: 0x801F_74E4,
        phase_arms: 9,
        table_head: false,
    },
];

/// The tick shape of the routine at `routine`, if it is one of the six.
pub fn tick_shape_for(routine: u32) -> Option<&'static CastTickShape> {
    CAST_TICK_SHAPES.iter().find(|s| s.routine == routine)
}

/// The shared body every tick in this band runs once per frame: bound the
/// module phase against the routine's own dispatch, run the arm, and advance
/// the phase exactly once unless the arm held.
///
/// `arm` returns `true` when the taken arm holds the phase (retail's
/// confirm-gated arms fall out of the dispatch without reaching an advance).
///
/// **The out-of-bound answer here is not retail's, deliberately.** Every one
/// of the bodies below opens by seeding a saved register with `1`, returns
/// that register, and lets **only** a terminal arm zero it - and the
/// out-of-range branch is aimed one instruction *past* that zeroing store, at
/// the return-value materialisation. So a phase past the `sltiu` bound
/// returns **Busy** in retail, and retail's drive loop (which advances only
/// on a zero return) would park there. The eleven bodies on this helper are
/// uniform in that shape; the seed / bound / latch / landing sites are, in
/// image order:
///
/// | PROT | routine | seed | bound | Done latch | out-of-range lands at |
/// |---|---|---|---|---|---|
/// | 0952 | `0x801F6A0C` | `0x801F6A58` | `0x801F6A98` `sltiu 5` | `0x801F70DC` | `0x801F70EC` |
/// | 0957 | `0x801F6A14` | `0x801F6A54` | chain from `0x801F6A98` | `0x801F7954` | `0x801F7958` |
/// | 0957 | `0x801F798C` | `0x801F79E0` | chain from `0x801F7A18` | `0x801F99BC` | `0x801F99C0` |
/// | 0958 | `0x801F6DD8` | `0x801F6E2C` | `0x801F6E70` `sltiu 0x100` | `0x801F8CF4` | `0x801F8CFC` |
/// | 0945 | `0x801F6EDC` | `0x801F6F38` | `0x801F6F68` `sltiu 8` | `0x801F769C` | `0x801F76C8` |
/// | 0960 | `0x801F74E4` | `0x801F7530` | chain from `0x801F7570` | `0x801F85F4` | `0x801F75B4` |
/// | 0925 | `0x801F6A00` | `0x801F6A70` | `0x801F6A68` `sltiu 0xA` | `0x801F7AA0` | `0x801F7ABC` |
/// | 0924 | `0x801F6A18` | `0x801F6A64` | `0x801F6AA8` `sltiu 0xC` | `0x801F77D0` | `0x801F77EC` |
/// | 0922 | `0x801F6A3C` | `0x801F6AA4` | `0x801F6AB4` `sltiu 0x19` | `0x801F90AC` | `0x801F90B0` |
/// | 0927 | `0x801F6A84` | `0x801F6A9C` | `0x801F6B04` `sltiu 0x1D` | `0x801F82D0` | `0x801F82D4` |
/// | 0949 | `0x801F6A10` | `0x801F6A70` | `0x801F6AA0` `sltiu 6` | `0x801F7588` | `0x801F758C` |
///
/// The port answers [`CastTickStep::Done`] instead, because the phase here is
/// the *engine's* (`World::run_cast_module_code` advances it) and a body that
/// walks past its own arms would otherwise hold the band's phase forever -
/// a softlock, not a fidelity gain. [`run_tick_latched`] is the helper whose
/// reported step **is** the register's, for the bodies whose terminal arms
/// are named.
///
/// REF: FUN_801F6A10 (`0x801F6AA4` the branch, `0x801F7588` the latch it
/// skips - the exemplar for all eleven)
pub(super) fn run_tick(
    ctx: &mut CastModuleCtx,
    arms: u16,
    arm: impl FnOnce(&mut CastModuleCtx) -> bool,
) -> CastTickStep {
    if u16::from(ctx.phase) >= arms {
        return CastTickStep::Done;
    }
    if !arm(ctx) {
        advance_phase(ctx);
    }
    CastTickStep::Busy
}

/// PROT 0952's terminal phase arm - the one arm that does not advance.
pub const ASTRAL_SLASH_TERMINAL_ARM: u8 = 4;
/// The clip PROT 0952's arm 2 stages (`addiu v0,zero,0xa; sb v0,0x1da(s0)`
/// at `0x801F6D88`).
pub const ASTRAL_SLASH_ARM2_CLIP: u8 = 0x0A;

/// PROT 0952 (Astral Slash) tick body.
///
/// Five phase arms behind `sltiu v1, 5` through the head table at
/// `0x801F69D8` (arm targets `0x801F6AC4`, `6C28`, `6D08`, `6E98`, `6FF4`).
/// It carries **no** damage-wrapper call at all; its whole simulation
/// footprint is staging and the phase walk, all through `$s4 = ctx + 0x279`
/// materialised at `0x801F6AA0`:
///
/// * arms `0..=3` each advance the phase exactly once (arm 0 has two
///   alternative advance sites on branches of the same arm);
/// * arm `0` stages a computed clip with a `+0x1DC` bump (`0x801F6BBC`);
/// * arm `2` stages clip `0x0A` with a bump, and writes `+0x21D = 1` on both
///   the caster and the victim - the slow-motion the move is known for;
/// * arm `3` writes `+0x1DA = 0` **without** a `+0x1DC` bump (`0x801F6EA4`),
///   restores the caster to `+0x21D = 8`, and drops the victim to `2` when
///   `0x8007BD10[victim] == 2`;
/// * arm `4` is terminal - it advances nothing, so the module parks there.
///
/// `docs/subsystems/cast-module.md`'s verdict cell lists only "writes staged
/// `+0x1DA`, restage `+0x1DC`"; the routine also advances the module phase
/// (five sites) and writes the animation rate `+0x21D` (four sites).
///
/// Not ported: the packet arms and the `FUN_80050BB8` / `FUN_801D5854` /
/// `FUN_80058490` calls.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A0C (phase machine + staging; packet arms unported)
pub fn astral_slash_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
) -> CastTickStep {
    run_tick(ctx, 5, |c| {
        match c.phase {
            0 => stage_clip(caster, caster.staged_anim.wrapping_add(1)),
            2 => {
                stage_clip(caster, ASTRAL_SLASH_ARM2_CLIP);
                caster.anim_rate = 1;
                victim.anim_rate = 1;
            }
            3 => {
                // The unpaired `+0x1DA` store: no restage bump here.
                caster.staged_anim = 0;
                caster.anim_rate = ANIM_RATE_NORMAL;
                victim.anim_rate = 2;
            }
            _ => {}
        }
        // Arm 4 is the terminal hold.
        c.phase == ASTRAL_SLASH_TERMINAL_ARM
    })
}

/// The action id PROT 0957's trampoline `0x801F9BA8` sends to
/// [`summon_effect_tick_b`] (`beq v1, 0x76` at `0x801F9BDC`).
pub const SUMMON_EFFECT_TICK_B_ID: u8 = 0x76;
/// The action id the same trampoline sends to [`summon_effect_tick_a`]
/// (`beq v1, 0x77` at `0x801F9BE4`). Any other id falls through to the
/// epilogue and ticks nothing.
pub const SUMMON_EFFECT_TICK_A_ID: u8 = 0x77;

/// PROT 0957 tick body A, reached from the module's own trampoline.
///
/// One `FUN_801DD4B0` site at `0x801F7514` with a baked power of `0x100`, the
/// shape-A clamp at `0x801F7538`, three `+0x1DA` / `+0x1DC` pairs, three
/// `ctx+0x278` writes and one phase store. Its head is a `beq`/`slti` chain,
/// not a table, so the per-arm map is not recovered statically; what is
/// ported is the bound, the damage step and the phase discipline.
///
/// `hit` is `Some(roll)` on the frame the damage arm fires; the caller owns
/// the wrapper call so the RNG cursor stays retail's.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A14 (phase machine + damage; packet arms and the per-arm
/// gating unported)
pub fn summon_effect_tick_a(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 8, |_| {
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
        }
        false
    })
}

/// PROT 0957 tick body B - the long one (8296 B, 2074 instructions).
///
/// No damage-wrapper call: every HP write here is computed in line, and there
/// are **four** shape-A clamp sites, not two, in three kinds - which is the
/// module's head string table `['Dies','Puera','Both','Damage','Recover']`
/// spelled out over the two seats:
///
/// * **Dies** - `sh zero, 0x14c(...)` at `0x801F92DC` (seat `s2`) and
///   `0x801F9364` (seat `s0`), each accumulating the whole bar into `+0x10`.
/// * **Damage** - a quarter of current HP (`(hp + 3) >> 2`) clamped against
///   the bar itself at `0x801F93D0` (`s2`) and `0x801F957C` (`s0`), then
///   `+0x10 += d`, `+0x14C -= d`.
/// * **Recover** - a quarter of **max** HP (`+0x14E`) clamped against the
///   missing HP (`subu v1, a0, v0; sltu v0, v1, a3`) at `0x801F9740` (`s0`)
///   and `0x801F978C` (`s2`), then `+0x10 -= d`, `+0x14C += d`. The HP goes
///   **up** here: this pair is a heal, not the drain an earlier reading of the
///   same two instructions took it for.
///
/// Seven phase stores through `$fp = ctx+0x279`, seven `+0x1DA` stages, five
/// restage bumps.
///
/// Ported: the phase walk and the staging discipline. Not ported: the drain's
/// per-arm rate, which is a frame-gated packet arm.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F798C (phase machine + staging; the drain arms unported)
pub fn summon_effect_tick_b(ctx: &mut CastModuleCtx, victim: &mut CastActorState) -> CastTickStep {
    run_tick(ctx, 8, |_| {
        let knockdown = victim.knockdown_anim;
        stage_clip(victim, knockdown);
        false
    })
}

/// PROT 0958 (Blazing Slash, Gi Delilas) tick body.
///
/// 256 phase arms behind `sltiu v1, 0x100` through the table filling file
/// `0x0..0x400` (default arm `0x801F8CFC`). Six `FUN_801DD6B4` sites, powers
/// [`BLAZING_SLASH_POWERS`], each followed by the shape-A clamp against
/// `actor_table[0]` - the seat-0 hardcode the module docs name.
///
/// `hit` names which of the six sites fires this frame; the caller supplies
/// the roll so the RNG cursor stays retail's.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6DD8 (phase machine + damage/staging; packet + camera arms
/// unported)
pub fn blazing_slash_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<(usize, i32)>,
) -> CastTickStep {
    run_tick(ctx, 0x100, |_| {
        if let Some((site, roll)) = hit
            && site < BLAZING_SLASH_POWERS.len()
        {
            apply_hit_floor_zero(victim, roll);
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        }
        false
    })
}

/// PROT 0945 (Water Column) tick body.
///
/// Eight phase arms behind `sltiu v1, 8` through the head table at
/// `0x801F69D8` (arm targets `0x801F6F94`, `7084`, `7140`, `7200`, `72A0`,
/// `7418`, `75AC`, `7604`). One `FUN_801DD4B0` site at `0x801F74A4`, power
/// `0x30`, shape-A clamp at `0x801F74C0`. It is the one tick body that writes
/// the flag word `+0x16E` (one store paired with one load).
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6EDC (phase machine + damage/staging; packet arms unported)
pub fn water_column_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
    flags_set: u16,
) -> CastTickStep {
    run_tick(ctx, 8, |_| {
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
            victim.flags |= flags_set;
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        }
        false
    })
}

/// PROT 0960's confirm-gated phase (`ctx+0x279 == 5`).
pub const PLASMA_STRIKE_CONFIRM_PHASE: u8 = 5;
/// The clip id that arm stages and then waits for (`0x0D`, compared against
/// `caster[+0x1D9]` at `0x801F7B64`).
pub const PLASMA_STRIKE_CONFIRM_CLIP: u8 = 0x0D;

/// PROT 0960's arm that lands the flurry (`ctx+0x279 == 0x0C`,
/// `0x801F801C..0x801F80C8`).
pub const PLASMA_STRIKE_LAND_ARM: u8 = 0x0C;
/// PROT 0960's burst arm (`ctx+0x279 == 0x0D`, `0x801F80CC`): the baked
/// `0x1C0` roll, then the caster's closing clip and the victim's knockdown.
pub const PLASMA_STRIKE_BURST_ARM: u8 = 0x0D;
/// The caster's closing clip the burst arm stages (`li v0,0xF` /
/// `sb v0,0x1da(s2)` at `0x801F8214`).
pub const PLASMA_STRIKE_CLOSE_CLIP: u8 = 0x0F;
/// Phase arms the head dispatches on: `0..=0x10`, then the terminal `0xFF`
/// (`0x801F85E4`, the only arm that returns `0`). Arm `0x10` stores `0xFF`
/// (`0x801F8584` / `0x801F85C0`), so the band runs `0x11` arms before it
/// reports done.
pub const PLASMA_STRIKE_ARMS: u16 = 0x11;

/// PROT 0960 (Plasma Strike, Lu Delilas) tick body.
///
/// The head is a `beq`/`slti` tree over `ctx+0x279` (`0x801F7570..
/// 0x801F7648`) whose arms run `0..=0x10` plus the terminal `0xFF`; every arm
/// but `0xFF` returns busy. Lu's chain stages `0x0E` (arm 0), `0x0C` (arm 4),
/// `0x0D` (arm 5) and closes on `0x0F` (arm `0x0D`).
///
/// * Arm 5 is the documented paired stage/confirm gate: it stages id `0x0D`
///   every tick and holds until `caster[+0x1D9]` equals the same literal,
///   ANDed with a progress check - so that arm does **not** advance the phase
///   until the confirm passes. An edit that remaps the stage without the
///   compare stalls phase 5 forever, which is the softlock the module docs
///   record.
/// * Arm `0x0C` lands the flurry. The flurry clips' hit events only
///   accumulate (the action's strike cursor is not parked during a cast), so
///   the victim's combo total `+0x00` is clamped to live HP, subtracted from
///   `+0x14C` and zeroed here (`lw a0,0x0(s3)` .. `sh v0,0x14c(s3)` at
///   `0x801F8098..0x801F80C8`). Without it the bar drains the flurry while HP
///   keeps it, and the action SM's `0x51` settle gate (`FUN_801E7250`,
///   `+0x14C != +0x172`) never opens.
/// * Arm `0x0D` is the `FUN_801DD6B4(0x1C0)` burst at `0x801F8168` with the
///   shape-A clamp at `0x801F818C`, then the caster's close
///   ([`PLASMA_STRIKE_CLOSE_CLIP`]) and the victim's knockdown from its
///   reaction map. `hit` is that roll; the engine folds the cast's outcome at
///   the band seam instead and passes `None`.
///
/// The per-arm countdowns (`0x801F8E50` minus the frame scalar), arm 6's
/// loop-window hold and the camera / packet / sound arms are not ported: an
/// arm with no modelled write advances on its first tick. In retail those
/// waits put arm `0x0C` past the flurry's last hit; the port instead holds
/// arm `0x0C` while the caster's clip still has hits to fire
/// ([`CastActorState::hits_pending`]), which keeps the same order.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F74E4 (phase machine + damage/staging + the phase-5 confirm
/// gate + the arm-`0x0C` combo landing; packet, camera and countdown arms
/// unported)
pub fn plasma_strike_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, PLASMA_STRIKE_ARMS, |c| match c.phase {
        PLASMA_STRIKE_CONFIRM_PHASE => {
            stage_clip(caster, PLASMA_STRIKE_CONFIRM_CLIP);
            // Hold the phase until the commit mirrors the id into `+0x1D9`.
            caster.playing_anim != PLASMA_STRIKE_CONFIRM_CLIP
        }
        // Held while the flurry clip still has hits to fire (see above):
        // landing mid-flurry would strand the later hits on the bar.
        PLASMA_STRIKE_LAND_ARM if caster.hits_pending => true,
        PLASMA_STRIKE_LAND_ARM => {
            let total = victim.combo_total.min(u32::from(victim.hp));
            victim.combo_total = 0;
            victim.hp -= total as u16;
            false
        }
        PLASMA_STRIKE_BURST_ARM => {
            if let Some(roll) = hit {
                apply_hit_floor_zero(victim, roll);
            }
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
            stage_clip(caster, PLASMA_STRIKE_CLOSE_CLIP);
            false
        }
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// PROT 0942 Power Up and PROT 0964 Element Change
// ---------------------------------------------------------------------------

/// The arm PROT 0942's Power Up commits its buff on (`ctx+0x279 == 3`, the
/// `beq v1, 3` at `0x801F7DC4`).
pub const POWER_UP_COMMIT_ARM: u8 = 3;

/// The render flag PROT 0942's arm 1 puts on the caster (`addiu v0, zero, 7`
/// then `sb v0, 0x21c(s0)` at `0x801F7F4C`/`0x801F7F58`); arm 2 clears it
/// again (`sb zero, 0x21c(s0)` at `0x801F7FF8`).
pub const POWER_UP_CHARGE_RENDER_FLAG: u8 = 7;

/// PROT 0942 (Power Up) tick body - the buff `battle-formulas.md` names as
/// the one that prints *"agility increased!"*.
///
/// Four `ctx+0x279` arms, reached through the `beq v1,1` / `slti v1,2` /
/// `beq v1,2` / `beq v1,3` chain at `0x801F7D94..0x801F7DC8`; anything else
/// falls to the epilogue with the seeded `1`. The module is reached from its
/// own trampoline `0x801F80A0`, action id `0x52`.
///
/// What it writes:
///
/// * arm `0` (`0x801F7DD4`) - spawns the charge effect, seeds the module's
///   own countdown at `0x801F88B4`, cue `0x5B` into `ctx+0x18`, advances;
/// * arm `1` (`0x801F7EB8`) - caster `+0x21C` =
///   [`POWER_UP_CHARGE_RENDER_FLAG`], advances;
/// * arm `2` (`0x801F7F5C`) - caster `+0x21C` = `0`, advances;
/// * arm `3` (`0x801F8010`) - the buff: caster `+0x156` (**AGL base**) =
///   `record[+0x0E] * 3 / 2`, read through the monster-record table
///   `0x801C9348[ctx[+0x13] - 3]` (`lhu v1,0xe(v0)` then `sll`/`addu`/`sra 1`
///   at `0x801F8060..0x801F8074`), then `ctx+0x0D = 0` and `s4 = 0`, so this
///   arm and only this arm reports done.
///
/// Only the **base** half moves: there is no `+0x154` store anywhere in the
/// routine, so the working gauge picks the buff up at the next round reset
/// (`battle_formulas::round_reset_agility`), not mid-round.
///
/// `agl_record` is the record's `+0x0E`; the caller supplies it because the
/// record table is the host's, not the kernel's. The per-arm countdown gate
/// (`0x801F88B4` minus `scratch[0x1F80037D] * scratch[0x1F800393]` each
/// frame) is **not** ported - it is per-frame timing, the same class this
/// module's header disclaims for every body here.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7D34 (phase machine + the AGL-base buff; packet arms and the countdown gate unported)
pub fn power_up_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    agl_record: u16,
) -> CastTickStep {
    if u16::from(ctx.phase) >= 4 {
        return CastTickStep::Done;
    }
    run_tick_latched(ctx, |c| match c.phase {
        1 => {
            caster.render_flag = POWER_UP_CHARGE_RENDER_FLAG;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0;
            CastArmStep::Advance
        }
        POWER_UP_COMMIT_ARM => {
            caster.agl_base = power_up_agl(agl_record);
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// The Power Up arithmetic on its own: `(x * 3) >> 1`, an arithmetic shift on
/// a value the routine loaded with `lhu`, so it never sees a negative.
pub fn power_up_agl(agl_record: u16) -> u16 {
    let x = u32::from(agl_record);
    ((x * 3) >> 1) as u16
}

/// The arm PROT 0945's `0xBA` body runs its buff on (`ctx+0x279 == 2`, the
/// `beq v1, 2` at `0x801F6A90`).
pub const ALL_STATS_SURGE_ARM: u8 = 2;

/// PROT 0945's **second** choreography - action id `0xBA` off the trampoline
/// `0x801F76F4`, in the same image as Water Column.
///
/// Four `ctx+0x279` arms (`0x801F6AA8`, `0x801F6BCC`, `0x801F6D24`,
/// `0x801F6E6C`) on the same chain shape PROT 0942 uses. Arm `2` is the buff
/// and it is the widest stat write in the band: **all ten** halfwords of the
/// five `(working, base)` pairs get `x + (x >> 2)` - a `+25%`, the same shape
/// as PROT 0955's Power Charge but over the whole block instead of the ATK
/// pair - and then `+0x156` (**AGL base**) takes the same
/// `record[+0x0E] * 3 / 2` PROT 0942's Power Up writes, off the same
/// `0x801C9348` record table. The stores run `0x801F6DA8..0x801F6E44`.
///
/// The shift is `srl`, and every operand is an `lhu`, so nothing here is
/// signed: a stat at `0xFFFF` wraps rather than saturating, which is retail's
/// behaviour and the port's.
///
/// Arm `3` (`0x801F6E6C`) is terminal: `ctx+0x0D = 0`, `ctx[+0x6DA] = 0x780`
/// and the return register is zeroed, so only that arm reports done.
///
/// PROT 0955's four bodies were once read as the band's only writers of the
/// actor stat block. They are not: this body, PROT 0942's `0x801F7D34`, PROT
/// 0954's `0x801F6A58` and PROT 0940's `0x801F78B8` write it too, and an
/// exhaustive `sh`-immediate sweep of all 64 band images finds stores in
/// eight of them (`docs/subsystems/cast-module.md`).
///
/// Not ported: the packet arms and the per-arm countdown gate at
/// `0x801F8834`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F8 (phase machine + the ten-halfword surge and the AGL-base write; packet arms unported)
pub fn all_stats_surge_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    agl_record: u16,
) -> CastTickStep {
    if u16::from(ctx.phase) >= 4 {
        return CastTickStep::Done;
    }
    run_tick_latched(ctx, |c| match c.phase {
        ALL_STATS_SURGE_ARM => {
            for stat in [
                &mut caster.atk,
                &mut caster.atk_base,
                &mut caster.udf,
                &mut caster.udf_base,
                &mut caster.ldf,
                &mut caster.ldf_base,
                &mut caster.spd,
                &mut caster.spd_base,
                &mut caster.intel,
                &mut caster.intel_base,
            ] {
                *stat = stat.wrapping_add(*stat >> 2);
            }
            caster.agl_base = power_up_agl(agl_record);
            CastArmStep::Advance
        }
        3 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// The three element ids PROT 0964's Element Change rolls between, read out
/// of its own image at `0x801F9B70` (file `+0x3198`): `03 04 02` - the
/// monster record's `+0x1D` id space.
pub const ELEMENT_CHANGE_ELEMENTS: [u8; 3] = [0x03, 0x04, 0x02];

/// The bias the same roll gets before it lands in the record's `+0x1C` group
/// byte (`addiu v0, v0, 0x13` at `0x801F8AAC`).
pub const ELEMENT_CHANGE_GROUP_BASE: u8 = 0x13;

/// The render flag PROT 0964's arm 1 puts on **every** seat
/// (`addiu a1, zero, 0xff` at `0x801F8B14`, stored `sb a1, 0x21c(v0)`).
pub const ELEMENT_CHANGE_HIDE_RENDER_FLAG: u8 = 0xFF;

/// How many rerolls the port allows before it accepts a repeat. Retail's loop
/// at `0x801F8A3C..0x801F8A64` is unbounded (`beq a3, a0` back to the draw);
/// an engine that inherits that spins forever on a degenerate RNG, so the
/// port bounds it. Any real RNG clears it on the first or second draw.
pub const ELEMENT_CHANGE_MAX_REROLLS: usize = 32;

/// What one Element Change commit resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementChangeOutcome {
    /// The accepted `rand() % 3`, which retail keeps at `0x801C8FE4`.
    pub roll: u8,
    /// The monster record's new `+0x1D` element id.
    pub element: u8,
    /// The monster record's new `+0x1C` group byte.
    pub group: u8,
}

/// PROT 0964 (Element Change) tick body, action id `0xAF` off its trampoline
/// `0x801F8E3C`.
///
/// Three `ctx+0x279` arms (`beq v1,1` / `slti v1,2` / `beq v1,2` at
/// `0x801F8954..0x801F8988`):
///
/// * arm `0` (`0x801F8990`) - cue `0x1C7`, the effect spawn, then the roll:
///   `rand() % 3` (the `0x55555556` multiply-high divide at
///   `0x801F89F8..0x801F8A28`) **re-drawn while it equals** the word at
///   `0x801C8FE4`, which is then rewritten with the accepted draw. The draw
///   indexes the module's own three-byte table at `0x801F9B70` into the
///   monster record's element `+0x1D` (`sb v0,0x1d(t0)` at `0x801F8AA0`) and
///   the raw draw `+ 0x13` into the record's group byte `+0x1C`
///   (`sb v0,0x1c(v1)` at `0x801F8AB4`). The record is `0x801C9348[0]` - the
///   **first monster seat**, not the caster and not the target.
/// * arm `1` (`0x801F8AC0`) - hides every seat in `ctx[+0]`: `+0x21C = 0xFF`
///   and the actor word `+4 = 0` (`0x801F8B20..0x801F8B50`).
/// * arm `2` (`0x801F8B94`) - `ctx+0x0D = 0`, `ctx[+0x6DA] = 0x780` and
///   `s4 = 0`: the only arm that reports done.
///
/// `last_roll` stands in for `0x801C8FE4`. The engine derives it from the
/// enemy's current element rather than carrying a second global - the word's
/// only job is "do not repeat what the last commit set", and the element the
/// last commit set is exactly what the record now holds. A value outside
/// `0..3` (a monster whose element is none of the three) matches nothing and
/// the first draw is accepted, which is what a fresh `0x801C8FE4` does too.
///
/// Not ported: the packet / camera arms, the per-arm countdown gate at
/// `0x801F9B74`, the `ctx[+0x6DA]` write and the actor word `+4`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F88EC (phase machine + the element re-roll and the whole-row hide; packet arms unported)
pub fn element_change_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    last_roll: u8,
    mut rand: impl FnMut() -> u32,
) -> (CastTickStep, Option<ElementChangeOutcome>) {
    if u16::from(ctx.phase) >= 3 {
        return (CastTickStep::Done, None);
    }
    let mut outcome = None;
    let step = run_tick_latched(ctx, |c| match c.phase {
        0 => {
            let mut roll = (rand() % 3) as u8;
            for _ in 0..ELEMENT_CHANGE_MAX_REROLLS {
                if roll != last_roll {
                    break;
                }
                roll = (rand() % 3) as u8;
            }
            outcome = Some(ElementChangeOutcome {
                roll,
                element: ELEMENT_CHANGE_ELEMENTS[roll as usize % 3],
                group: roll.wrapping_add(ELEMENT_CHANGE_GROUP_BASE),
            });
            CastArmStep::Advance
        }
        1 => {
            let count = usize::from(c.party_count).min(seats.len());
            for seat in seats.iter_mut().take(count) {
                seat.render_flag = ELEMENT_CHANGE_HIDE_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        _ => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
    });
    (step, outcome)
}
