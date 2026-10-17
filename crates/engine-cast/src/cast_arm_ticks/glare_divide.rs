//! PROT 0940 Glare / Divide: the blind tick and the split (clone) tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0940 - cast_glare_divide
// ---------------------------------------------------------------------------

/// PROT 0940 (Glare Divide) tick body for action `0xAC`.
///
/// Eight phase arms behind `sltiu v0, v1, 8` (`0x801F72B4`) through the head
/// table at `0x801F69D8` (arm targets `0x801F72E0`, `735C`, `73C4`, `74CC`,
/// `7584`, `76E8`, `77BC`, `7840`). Its whole simulation footprint is two
/// arms:
///
/// * arm `4` blanks the **first monster seat**'s five-byte reaction-clip run
///   `+0x1EF..+0x1F3` (`0x801F7690`..`0x801F76C0`). The seat is
///   `actor_table[3]`, reached as `lw v0, 0xc(s0)` - and `s0` is **not** the
///   caster there: `lui s0, 0x801d; addiu s0, s0, -0x6c90` at
///   `0x801F7648`/`0x801F764C` reassigns it to the actor table base
///   `0x801C9370` four instructions earlier, so the displacement `0xC` is the
///   table index, not a record field. A backward scan that stopped at the
///   prologue's `lw s0, 0(v0)` would read this as `caster[+0x0C]`;
/// * arm `4` also raises Cort's **Mystic Shield**: it stores the handle of
///   the effect it just spawned into `_DAT_8007BD84` (`sw v0,-0x427C(v1)` at
///   `0x801F7678`), the word the damage finisher halves an enemy defender's
///   hit on and the `0xB4` pick gates Evil Seru Magic on. The kernel has no
///   global to write, so the host raises it when this arm runs
///   ([`MYSTIC_SHIELD_ARM`]);
/// * arm `7` is terminal - it clears `ctx[+0x0D]`, writes the framing
///   halfword `ctx[+0x6DA] = 0x780`, and clears the busy register, so the
///   body reports Done and the phase parks at `7`.
///
/// The camera arms (the `FUN_801D829C` shot and the `0x800840BC` /
/// `0x800840C0` / `0x8007B790` walks) and the module-resident countdown at
/// `0x801F864C` that gates every arm are ported beside this body, in
/// `crate::cast_module_camera::mystic_shield_camera`. Not ported: the sound
/// cue `FUN_8004FCC8(0x1AA)` and the `FUN_80050ED4` / `FUN_80021B04` /
/// `FUN_801DFDF0` pool spawns.
///
/// Wired: `World::run_cast_module_code`, the `(940, GLARE_DIVIDE_BLIND_TICK)`
/// arm of the trampoline dispatch.
///
/// PORT: FUN_801F7240 (PROT 0940 action 0xAC; phase machine + reaction-clip
/// blank; packet arms unported)
pub fn glare_divide_blind_tick(
    ctx: &mut CastModuleCtx,
    first_monster: &mut CastActorState,
    first_monster_ext: &mut CastArmExtState,
) -> CastTickStep {
    run_arm_tick(ctx, 8, |c| match c.phase {
        4 => {
            first_monster.reaction_alt = 0;
            first_monster.reaction_alt2 = 0;
            first_monster.knockdown_anim = 0;
            first_monster.reaction_gate = 0;
            first_monster_ext.reaction_extra = 0;
            CastArmStep::Advance
        }
        7 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// Which half of a Divide the weakening branch hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitWeakened {
    /// `rand() & 1 == 0` - the new seat takes the 1 HP.
    Clone,
    /// `rand() & 1 != 0` - the original caster does.
    Caster,
}

/// What PROT 0940's split arm asked the host to materialise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlareDivideSplit {
    /// The seat the arm allocated: `3 + ctx[+1]` read **before** the
    /// increment (`lbu v0, 1(v1); andi s3, v0, 0xff; addiu v0, v0, 1;
    /// sb v0, 1(v1)` at `0x801F7E08`..`0x801F7E1C`, then
    /// `piVar15 = &actor_table[s3 + 3]`).
    pub clone_seat: u8,
    /// `+0x14C` and `+0x172` of the new seat, both the caster's live HP.
    pub clone_hp: u16,
    /// The `+0x1DD` value the arm displaced off the caster, which the `0xFF`
    /// arm puts back.
    pub saved_caster_target: u8,
    /// `None` for action `0x50`; `Some(..)` for `0xAE`, whose extra branch
    /// (`li v0, 0xae; bne v1, v0` at `0x801F80F0`) renames both halves and
    /// pins one of them to 1 HP.
    pub weakened: Option<SplitWeakened>,
}

/// PROT 0940 (Glare Divide) tick body for actions `0x50` and `0xAE` - the
/// **split**, the one arm in the band that allocates a battle seat.
///
/// A `beq`/`slti` chain over `{0, 1, 2, 3, 0xFF}` (`0x801F7920`..`0x801F7968`);
/// any other phase falls to `move v0, s5` and reports Busy.
///
/// * arm `0` - sound cue `0x141` when the caster's queued id is `0x50`, else
///   `0x1A9`, and the framing call. Advances;
/// * arm `1` - `caster[+0x21C] = 10` (`0x801F7B5C`). Advances;
/// * arm `2` - the split, below. Advances;
/// * arm `3` - latches `ctx[+0x279] = 0xFF` (`0x801F81EC`) and stays Busy;
/// * arm `0xFF` - restores `caster[+0x1DD]` from the module word `0x801F8658`
///   and clears the busy register.
///
/// The split itself, in store order:
///
/// ```text
/// seat            = ctx[+1] ; ctx[+1] += 1            ; clone = actor_table[seat + 3]
/// 0x801C9348[seat]= 0x801C9348[ctx[+0x13] - 3]        ; the clone shares the monster record
/// ctx[+0x1A]     += 1                                 ; the turn cursor
/// clone[+0x16C]   = 0                                 ; it has already acted
/// clone[+0x1DE]   = 2 ; clone[+0x1DF] = 0x50 ; clone[+0x1DD] = 9
/// clone[+0x172]   = clone[+0x14C] = caster[+0x14C]
/// saved           = caster[+0x1DD] ; caster[+0x21C] = 0 ; caster[+0x1DD] = 9
/// if caster[+0x1DF] == 0xAE {
///     rand() & 1 == 0 ? clone : caster
///         [+0x14C] = 1 ; [+0x150] = 0 ; [+0x158] = 1 ; [+0x156] = [+0x154] = 0x20
/// }
/// ```
///
/// So `0x50` splits at full HP and `0xAE` leaves one of the two halves on 1 HP
/// with ATK 1, no MP and AGL `0x20` - a coin flip that can land on the caster.
///
/// Not ported: the actor-object allocation (`FUN_80054CB0` / `FUN_80024C88`),
/// the unaligned `swl`/`swr` pose copies into the clone's `+0x34..+0x4B`, the
/// `FUN_8003CA38` / `FUN_8003CA78` name edit that gives the halves their
/// suffix, the `0x80076768`-table copy and the camera arms.
///
/// `rand` is the `FUN_80056798` draw; `None` suppresses the weakening branch,
/// which is what an id of `0x50` does anyway.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F78B8 (PROT 0940 actions 0x50 / 0xAE; split + phase latch;
/// object allocation and name edit unported)
#[allow(clippy::too_many_arguments)]
pub fn glare_divide_split_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    caster_ext: &CastArmExtState,
    action_id: u8,
    saved_target: u8,
    rand: Option<u32>,
) -> (CastTickStep, Option<GlareDivideSplit>) {
    let _ = caster_ext;
    let mut split = None;
    let step = run_chain_tick(ctx, |c| match c.phase {
        0 => CastArmStep::Advance,
        1 => {
            caster.render_flag = 10;
            CastArmStep::Advance
        }
        2 => {
            let seat = FIRST_MONSTER_SEAT.wrapping_add(c.monster_count);
            c.monster_count = c.monster_count.wrapping_add(1);
            c.turn_cursor = c.turn_cursor.wrapping_add(1);
            let saved = caster.target_code;
            caster.render_flag = 0;
            caster.target_code = crate::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
            let weakened = (action_id == 0xAE).then(|| match rand.unwrap_or(0) & 1 {
                0 => SplitWeakened::Clone,
                _ => SplitWeakened::Caster,
            });
            if weakened == Some(SplitWeakened::Caster) {
                caster.hp = 1;
                caster.atk = 1;
                caster.agl_base = SPLIT_WEAK_AGL;
                caster.agl = SPLIT_WEAK_AGL;
            }
            split = Some(GlareDivideSplit {
                clone_seat: seat,
                clone_hp: caster.hp,
                saved_caster_target: saved,
                weakened,
            });
            CastArmStep::Advance
        }
        3 => {
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => {
            caster.target_code = saved_target;
            CastArmStep::Finish
        }
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    });
    (step, split)
}
