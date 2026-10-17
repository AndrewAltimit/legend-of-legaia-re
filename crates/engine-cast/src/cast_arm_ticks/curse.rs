//! PROT 0943 Curse: the single-victim tick and the MP-drain tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0943 - cast_curse
// ---------------------------------------------------------------------------

/// PROT 0943 (Curse) tick body for action `0x40` - the **single-target**
/// curse.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6F7C`) through the head
/// table at **`0x801F69F0`** - not the load base, because the module's other
/// body's five-word table occupies `0x801F69D8..0x801F69EC` first. Arm targets
/// `0x801F6FA8`, `7164`, `7284`, `73C0`, `74C0`.
///
/// * arm `0` - sound cue `0x142`, `caster[+0x1DA] = 0x0B` with `+0x1DC += 1`,
///   hide every party seat flagged in `0x8007BD10` and every live monster
///   seat, hide the victim (`+0x21C = 0xFF`) and put the caster at
///   `+0x21C = 6`;
/// * arm `1` - `caster[+0x1DA] = 0`;
/// * arm `2` - `caster[+0x21C] = 0xFF`, `victim[+0x21C] = 0`;
/// * arm `3` - **`victim[+0x16E] |= 0x1000`** (`0x801F7408`), the curse mark;
/// * arm `4` - terminal: `ctx[+0x0D] = 0`, re-show every seat, busy cleared.
///
/// Not ported: the `FUN_80021B04` / `FUN_80024E80` spawns, the `FUN_801D829C`
/// framings and the countdown at `0x801F7A04`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6EF4 (PROT 0943 action 0x40; phase machine + the 0x1000
/// curse mark; packet / camera arms unported)
pub fn curse_single_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
) -> CastTickStep {
    run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            stage_clip(caster, 0x0B);
            victim.render_flag = 0xFF;
            caster.render_flag = 6;
            CastArmStep::Advance
        }
        1 => {
            caster.staged_anim = 0;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0xFF;
            victim.render_flag = 0;
            CastArmStep::Advance
        }
        3 => {
            victim.flags |= FLAG_CURSED;
            CastArmStep::Advance
        }
        _ => {
            c.ctx_0d = 0;
            caster.render_flag = 0;
            victim.render_flag = 0;
            CastArmStep::Finish
        }
    })
}

/// PROT 0943 (Curse) tick body for action `0xB5` - the **MP drain**.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6A70`) through the head
/// table at `0x801F69D8` (arm targets `0x801F6A9C`, `6B78`, `6C20`, `6DE0`,
/// `6E58`).
///
/// Arm `2` is the whole point: over `0 .. ctx[+0]`, with **no** liveness or
/// `+0x16E & 4` guard at all,
///
/// ```text
/// seat[+0x178] = seat[+0x150]      ; 0x801F6CF4 / 0x801F6CFC
/// seat[+0x150] = 0                 ; 0x801F6D08
/// seat[+0x152] = 0                 ; 0x801F6D1C
/// ```
///
/// so every actor on the field loses its MP and the module keeps the old
/// working value at `+0x178`. Arms `0` and `2` also drive `+0x21C` (hide, then
/// show) across the same range, and arm `4` is terminal (`ctx[+0x0D] = 0`,
/// `ctx[+0x6DA] = 0x780`).
///
/// `docs/subsystems/cast-module.md`'s stat-writer table attributes these two
/// stores to a PROT 0943 routine at `0x801F69D8`. There is no routine there:
/// `0x801F69D8` is this body's own five-word head table, and `0x801F6D08` /
/// `0x801F6D1C` both sit inside this body's frame-matched extent
/// (`0x801F6A04 + 0x4F0 = 0x801F6EF4`).
///
/// Not ported: the `FUN_80050ED4` / `FUN_80024E80` spawns and the countdown.
///
/// Returns the drained MP per seat, in visit order.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A04 (PROT 0943 action 0xB5; phase machine + the field-wide
/// MP drain; packet arms unported)
pub fn curse_mp_drain_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    exts: &mut [CastArmExtState],
) -> (CastTickStep, Vec<(u8, u16)>) {
    let mut drained = Vec::new();
    let step = run_arm_tick(ctx, 5, |c| match c.phase {
        0 => {
            for seat in 0..c.party_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0xFF;
                }
            }
            CastArmStep::Advance
        }
        2 => {
            for seat in 0..c.party_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
                let Some(e) = exts.get_mut(seat as usize) else {
                    continue;
                };
                e.mp_stash = e.mp;
                drained.push((seat, e.mp));
                e.mp = 0;
                e.mp_base = 0;
            }
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, drained)
}
