//! PROT 0962 Blade Breath: one body shape, three constant sets.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0962 - cast_blade_breath
// ---------------------------------------------------------------------------

/// One of PROT 0962's three near-identical charge bodies, as a set of the
/// four constants that differ between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BladeBreathShape {
    /// The clip arm `2` stages on the caster (`+0x1DA`).
    pub windup_clip: u8,
    /// The animation rate arm `2` puts the caster on, `None` where the arm
    /// writes none.
    pub windup_rate: Option<u8>,
    /// The `+0x21F` render sub-flag arm `3` writes on the victim, `None`
    /// where it writes none.
    pub strike_sub_flag: Option<u8>,
    /// Whose `+0x1D9` clip arm `4` waits on before latching `0xFF`: `true`
    /// the victim's, `false` the caster's.
    pub tail_waits_on_victim: bool,
}

/// `0xA2` at `0x801F7AE4`: windup clip `5`, `victim[+0x21F] = 1`, tail on the
/// victim.
pub const BLADE_BREATH_A: BladeBreathShape = BladeBreathShape {
    windup_clip: 5,
    windup_rate: None,
    strike_sub_flag: Some(1),
    tail_waits_on_victim: true,
};

/// `0xA3` at `0x801F74A0`: windup clip `6` at rate `3`,
/// `victim[+0x21F] = 2`, tail on the caster (`caster[+0x21D] = 2` then the
/// `+0x1DC` bump).
pub const BLADE_BREATH_B: BladeBreathShape = BladeBreathShape {
    windup_clip: 6,
    windup_rate: Some(3),
    strike_sub_flag: Some(2),
    tail_waits_on_victim: false,
};

/// `0xA4` at `0x801F6D54`: windup clip `7` at rate `4`, no `+0x21F`, tail on
/// the victim.
pub const BLADE_BREATH_C: BladeBreathShape = BladeBreathShape {
    windup_clip: 7,
    windup_rate: Some(4),
    strike_sub_flag: None,
    tail_waits_on_victim: true,
};

/// The baked power all three PROT 0962 bodies hand `FUN_801DD4B0`
/// (`li a0, 0x200` at `0x801F7F08`, `0x801F787C` and `0x801F72A0`).
pub const BLADE_BREATH_POWER: u16 = 0x0200;

/// The shared body of PROT 0962's three charge arms.
///
/// All three dispatch through a `beq`/`slti` chain over
/// `{0, 1, 2, 3, 4, 0xFF}` and run the same five steps:
///
/// * arm `0` - **stores** `ctx[+0x279] = 1` outright (not an increment), then
///   tests arrival with `FUN_8004E2F0(ctx[+0x13], caster[+0x1DD])`. Arrival is
///   the metric reading **zero**: `sll v0,v0,0x10; bne v0,zero,<hold>` at
///   `0x801F7C0C` sends a non-zero metric to the hold arm, which stages the
///   run clip `1` with `+0x1DC += 1` and stays on phase `1`. Arrived, it runs
///   `FUN_80050BB8` - the pairwise **separation nudge**
///   ([`legaia_engine_battle_vm::battle_separation`]), not the approach - 0x20 times and
///   advances the phase again;
/// * arm `1` - the same test, advancing once the metric reads zero;
/// * arm `2` - the sound cue (`0x18B` / `0x18C` / `0x18D`), the facing, the
///   windup clip with `+0x1DC += 1`, and the windup animation rate;
/// * arm `3` - gated on the caster object's `+0x68` reaching `0x300`, then
///   `FUN_801DD4B0(0x200, ctx[+0x13], caster[+0x1DD])`, the popup, the
///   **unsigned** clamp, the `+0x21F` sub-flag, `victim[+0x21D] = 4`, `+0x10`
///   accumulate, HP write, `+0x1DA = +0x1F1` with `+0x1DC += 1`, and
///   `caster[+0x1DA] = 0`;
/// * arm `4` - `victim[+0x21D] = 4`, hold until the watched actor's `+0x1D9`
///   is `0` or `8`, then latch `ctx[+0x279] = 0xFF`;
/// * arm `0xFF` - clears the busy register.
///
/// Not ported: the walk itself, the facing, the cue, the `FUN_80050ED4`
/// spawns, the screen-prim wash and the countdown at `0x801F89AC`.
///
/// PORT: FUN_801F7AE4, FUN_801F74A0, FUN_801F6D54 (PROT 0962 actions 0xA2 /
/// 0xA3 / 0xA4; chain phase machine + the 0x200 hit; walk / packet arms
/// unported)
pub fn blade_breath_tick(
    ctx: &mut CastModuleCtx,
    shape: &BladeBreathShape,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    run_chain_tick(ctx, |c| match c.phase {
        0 => {
            c.phase = 1;
            if arrived {
                CastArmStep::Advance
            } else {
                stage_clip(caster, 1);
                CastArmStep::Hold
            }
        }
        1 => {
            if arrived {
                CastArmStep::Advance
            } else {
                CastArmStep::Hold
            }
        }
        2 => {
            stage_clip(caster, shape.windup_clip);
            if let Some(rate) = shape.windup_rate {
                caster.anim_rate = rate;
            }
            CastArmStep::Advance
        }
        3 => {
            if let Some(roll) = hit {
                apply_arm_hit(victim, roll, victim.restage.wrapping_add(1), 4);
            }
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        4 => {
            victim.anim_rate = 4;
            let watched = if shape.tail_waits_on_victim {
                victim.playing_anim
            } else {
                caster.playing_anim
            };
            if watched != 0 && watched != 8 {
                return CastArmStep::Hold;
            }
            if !shape.tail_waits_on_victim {
                caster.anim_rate = 2;
                caster.restage = caster.restage.wrapping_add(1);
            }
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => CastArmStep::Finish,
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    })
}

/// PROT 0962's `0xA2` arm - [`blade_breath_tick`] with [`BLADE_BREATH_A`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_a_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_A, caster, victim, arrived, hit)
}

/// PROT 0962's `0xA3` arm - [`blade_breath_tick`] with [`BLADE_BREATH_B`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_b_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_B, caster, victim, arrived, hit)
}

/// PROT 0962's `0xA4` arm - [`blade_breath_tick`] with [`BLADE_BREATH_C`].
///
/// Wired: `World::run_cast_module_code`.
pub fn blade_breath_c_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    arrived: bool,
    hit: Option<i32>,
) -> CastTickStep {
    blade_breath_tick(ctx, &BLADE_BREATH_C, caster, victim, arrived, hit)
}
