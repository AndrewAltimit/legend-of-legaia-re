//! PROT 0955, the six-spell cell, and the per-body damage-shape lookup.
//! Split out of `cast_module_ticks.rs`.

use super::*;

// --- PROT 0955, the six-spell cell -----------------------------------------

/// PROT 0955's White Shield (`0x60`) multiplies the caster's **record** base
/// defence by `3/2` (`sll 1; addu; sra 1` at `0x801F9230..0x801F9238`).
///
/// The source is the monster **record** through `0x801C9348[seat - 3]`, not
/// the live actor, so the buff is idempotent: recasting rewrites the same
/// product rather than compounding it.
pub fn white_shield_defence(record_udf: u16, record_ldf: u16) -> (u16, u16) {
    let scale = |v: u16| ((i32::from(v) * 3) >> 1) as u16;
    (scale(record_udf), scale(record_ldf))
}

/// The `+0x21C` value PROT 0955's White Shield arm 1 writes on the caster
/// (`addiu v0,zero,9; sb v0,0x21c(s0)` at `0x801F9114`). Neither `0` nor the
/// summon fade's `0xFF`, so the field is a small enum rather than a flag.
pub const WHITE_SHIELD_ARM1_RENDER_FLAG: u8 = 9;

/// PROT 0955 (White Shield) tick body - action id `0x60`, 920 B.
///
/// Four phase arms in a `beq`/`slti` chain (`0x801F8F6C..0x801F8FA8`), caster
/// `s0 = actor_table[ctx+0x13]`; every arm past `0` gates on the module's own
/// countdown word at `0x801F9D28`, and the busy register defaults to `1`, so
/// an unnamed phase reports busy.
///
/// * arm `0` - cue `0x196`, one `FUN_80021B04` spawn, seed the countdown from
///   `scratch[0x37D] * 12` and write `ctx[+0x18] = 0x5B`;
/// * arm `1` - `caster[+0x21C] = 9`;
/// * arm `2` - `caster[+0x21C] = 0`;
/// * arm `3` - the buff: read the caster's own monster record through
///   `0x801C9348[ctx[+0x13] - 3]`, take `+0x14` (UDF) and `+0x16` (LDF), and
///   write `base * 3 / 2` into **both** halves of each live pair -
///   `+0x15C`/`+0x15E` and `+0x160`/`+0x162` - then clear `ctx[+0x0D]` and
///   return zero.
///
/// `docs/subsystems/cast-module.md` grades this row's damage "none". It is a
/// **defence buff**, and the pair-at-a-time write is why: the working
/// halfword is what the damage kernel reads and the base halfword is what a
/// round reset restores to, so writing only the working half would evaporate
/// at the next `FUN_80053CB8` pass.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F8F0C (phase chain + the defence buff; packet arms unported)
pub fn white_shield_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    record_defence: (u16, u16),
) -> CastTickStep {
    run_tick_latched(ctx, |c| match c.phase {
        1 => {
            caster.render_flag = WHITE_SHIELD_ARM1_RENDER_FLAG;
            CastArmStep::Advance
        }
        2 => {
            caster.render_flag = 0;
            CastArmStep::Advance
        }
        3 => {
            let (udf, ldf) = white_shield_defence(record_defence.0, record_defence.1);
            caster.udf = udf;
            caster.udf_base = udf;
            caster.ldf = ldf;
            caster.ldf_base = ldf;
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// `+0x16E` bit `0x400` - the flag PROT 0955's Kiss of Death sets on its
/// victim when the instant-death roll **misses** (`ori v0,v0,0x400` at
/// `0x801F8CFC`).
///
/// `docs/subsystems/battle-formulas.md` records the setter for this bit as
/// the one remaining status-applier gap; this is it.
pub const FLAG_KISS_OF_DEATH_MARK: u16 = 0x0400;

/// The `+0x16E` mask Kiss of Death's **hit** arm keeps (`andi v0,v0,0xf07f`
/// at `0x801F8D7C`) - it clears bits `0x0F80`, the status block above Venom /
/// Toxic / Stone.
pub const KISS_OF_DEATH_KEEP_MASK: u16 = 0xF07F;

/// The `+0x1DC` value Kiss of Death's `+0x1EF` leg writes (`sb s2,0x1dc(s0)`
/// at `0x801F8DB0`, `s2` still holding the dispatch chain's `5`).
pub const KISS_OF_DEATH_ALT_RESTAGE: u8 = 5;

/// PROT 0955 (Kiss of Death) tick body - action id `0x6E`, 2152 B.
///
/// Arms `0`, `1`, `2`, `3`, `4`, `5` and `0xFF` in a `beq`/`slti` chain
/// (`0x801F8728..0x801F8788`); caster `s2 = actor_table[ctx+0x13]`, victim
/// `s0 = actor_table[caster[+0x1DD]]`.
///
/// Arm `4` (`0x801F8C68`) is the spell: past the countdown gate it draws
/// `FUN_80056798()` and tests bit `0`.
///
/// * **odd - the miss.** Set [`FLAG_KISS_OF_DEATH_MARK`] on the victim and run
///   the [`steal_turn`] idiom, so a missed Kiss of Death still costs the
///   victim its turn.
/// * **even - the hit.** Skip a `+0x16E & 4` victim; else clear the status
///   block ([`KISS_OF_DEATH_KEEP_MASK`]), then `+0x10 += 1` and
///   `+0x14C -= 1` - the arm applies exactly **one** point of damage - and
///   stage the reaction ([`stage_reaction`], with `+0x1DC` set to `1` on the
///   `+0x1F1` leg and [`KISS_OF_DEATH_ALT_RESTAGE`] on the `+0x1EF` leg).
///
/// Arm `5` waits for the victim to settle and jumps the phase to `0xFF`; arm
/// `0xFF` clears `ctx[+0x0D]`, restores every living seat's
/// `+0x04 = 0x20080200` / `+0x21C = 0` across the seven combat slots, and
/// zeroes the busy register.
///
/// `docs/subsystems/cast-module.md` grades the row's damage "none", which is
/// true of the damage *wrapper*: this body calls none. The HP write is a
/// literal decrement, and the real effect is the status mark plus the stolen
/// turn.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F86A4 (phase chain + the roll, status mark, turn steal and one-point hit; packet arms unported)
pub fn kiss_of_death_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    roll: Option<u32>,
) -> (CastTickStep, Option<u8>) {
    let mut refund = None;
    let step = run_tick_latched(ctx, |c| match c.phase {
        4 => {
            match roll {
                Some(r) if r & 1 != 0 => {
                    victim.flags |= FLAG_KISS_OF_DEATH_MARK;
                    refund = steal_turn(c, victim);
                }
                // The even leg, and only for a victim the `+0x16E & 4`
                // guard at `0x801F8D60` lets through.
                Some(_) if victim.flags & FLAG_NON_TARGETABLE == 0 => {
                    victim.flags &= KISS_OF_DEATH_KEEP_MASK;
                    victim.hp_bar_delta += 1;
                    victim.hp = victim.hp.wrapping_sub(1);
                    stage_reaction(victim);
                    victim.restage = if victim.reaction_gate != 0 {
                        1
                    } else {
                        KISS_OF_DEATH_ALT_RESTAGE
                    };
                }
                Some(_) | None => {}
            }
            CastArmStep::Advance
        }
        5 => {
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, refund)
}

/// PROT 0955's Melt Spray (`0x6F`) stat step: `x - (x + 9) / 5` - a `-20%`
/// with a `+9` rounding bias - with a floor that fires only on an exact
/// zero.
///
/// The division is the signed magic `0x66666667` (`mult`, `mfhi`, `sra 2`,
/// minus the sign) at `0x801F8314..0x801F8378`. The floor is the
/// `bnez ...; addiu v0,v0,1` pair each store is followed by, and the
/// arithmetic that reaches the store is 32-bit `subu` while the store itself
/// is a 16-bit `sh` - so a stat of `2` lands on zero and is corrected to `1`,
/// but a stat of `0` or `1` goes to `-1` and is written back as `0xFFFF`,
/// which the floor's `bnez` then sees as non-zero and leaves alone. Retail
/// underflows a one-point stat into 65535; that is the behaviour, not a port
/// artefact, and it is why the floor cannot be modelled as `max(1, ..)`.
///
/// Only the first floor of each five-stat block tests a live 32-bit register
/// (`move v1,a1` at `0x801F83E0` / `0x801F8504`, then `bnez v1`); the other
/// four re-`lhu` the halfword they just stored and test that
/// (`0x801F83F8`, `0x801F840C`, `0x801F8420`, `0x801F8434`, and the same
/// four in the base block from `0x801F851C`). The two shapes agree on every
/// reachable value - `-1` truncates to `0xFFFF`, which is non-zero either
/// way - so the underflow is a property of the `subu`/`sh` width mismatch,
/// not of which register the `bnez` reads.
pub fn melt_spray_step(stat: u16) -> u16 {
    let x = i32::from(stat);
    let reduced = x - (x + 9) / 5;
    if reduced == 0 { 1 } else { reduced as u16 }
}

/// PROT 0955 (Melt Spray) tick body - action id `0x6F`, 1792 B.
///
/// Arms `0`, `1`, `2`, `3`, `4` and a terminal `0x801F8670`
/// (`0x801F8024..0x801F8074`); victim `s0 = actor_table[caster[+0x1DD]]`.
///
/// The debuff arm walks **five** stats and both halfwords of each -
/// `+0x158`/`+0x15A` ATK, `+0x15C`/`+0x15E` UDF, `+0x160`/`+0x162` LDF,
/// `+0x164`/`+0x166` SPD, `+0x168`/`+0x16A` INT - applying
/// [`melt_spray_step`] to each and then staging the victim's reaction
/// ([`stage_reaction`], `+0x1DC = 1`).
///
/// It is the widest single stat write in the band and the row
/// `docs/subsystems/cast-module.md` grades "none": the module calls no damage
/// wrapper, and what it does instead is a five-stat, ten-halfword `-20%`.
/// Note the shape differs from the item buffs' `x * 6/5` clamped to `0xFFFF`
/// (`battle-formulas.md`): this is `x - (x + 9)/5` floored at `1`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7FA4 (phase chain + the five-stat debuff; packet arms unported)
pub fn melt_spray_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    debuff_arm: bool,
) -> CastTickStep {
    run_tick_latched(ctx, |c| {
        if debuff_arm {
            for stat in [
                &mut victim.udf,
                &mut victim.ldf,
                &mut victim.spd,
                &mut victim.atk,
                &mut victim.intel,
                &mut victim.udf_base,
                &mut victim.ldf_base,
                &mut victim.spd_base,
                &mut victim.atk_base,
                &mut victim.intel_base,
            ] {
                *stat = melt_spray_step(*stat);
            }
            stage_reaction(victim);
            victim.restage = 1;
        }
        if c.phase == CHOREOGRAPHY_DONE_PHASE {
            c.ctx_0d = 0;
            CastArmStep::Finish
        } else {
            CastArmStep::Advance
        }
    })
}

/// PROT 0955 (Terror Scream) tick body - action id `0x70`, 2344 B.
///
/// Arms `0`, `1`, `2`, `3`, `4` and `0xFF` (`0x801F7700..0x801F7750`); caster
/// `s3`, victim `s2 = actor_table[caster[+0x1DD]]`.
///
/// Arm `3` (`0x801F7D98`) is the whole spell, and it writes **no** stat and
/// **no** status bit: it runs the [`steal_turn`] idiom unconditionally on the
/// victim (`0x801F7E18..0x801F7E4C`). Arm `4` polls the victim for a settled
/// pose and jumps the phase to `0xFF`; the `0xFF` arm clears `ctx[+0x0D]`,
/// restores the seven combat seats and reports done.
///
/// So the row `docs/subsystems/cast-module.md` grades "none" is a **turn
/// thief**: the victim's queued item is refunded, its initiative key is
/// consumed and the turn cursor advances past it.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F767C (phase chain + the turn steal; packet arms unported)
pub fn terror_scream_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
) -> (CastTickStep, Option<u8>) {
    let mut refund = None;
    let step = run_tick_latched(ctx, |c| match c.phase {
        3 => {
            refund = steal_turn(c, victim);
            CastArmStep::Advance
        }
        4 => {
            c.phase = CHOREOGRAPHY_DONE_PHASE;
            CastArmStep::Hold
        }
        CHOREOGRAPHY_DONE_PHASE => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, refund)
}

/// The exclusive cap Power Charge tests against - a stat that reaches it is
/// written back as `0x3E7` (999).
pub const POWER_CHARGE_CAP: u16 = 0x3E8;

/// PROT 0955's Power Charge (`0x72`) stat step: `x + (x >> 2)` - a `+25%` -
/// capped at [`POWER_CHARGE_CAP`] (`sltiu v0,v0,0x3e8` at `0x801F74D4`).
pub fn power_charge_step(stat: u16) -> u16 {
    let raised = u32::from(stat) + (u32::from(stat) >> 2);
    let raised = (raised & 0xFFFF) as u16;
    if raised < POWER_CHARGE_CAP {
        raised
    } else {
        POWER_CHARGE_CAP - 1
    }
}

/// PROT 0955 (Power Charge) tick body - action id `0x72`, 1316 B.
///
/// Arms `0`, `1`, `2`, `3` and `4` (`0x801F71D4..0x801F721C`); the target is
/// the **caster** `s1 = actor_table[ctx+0x13]`, not the `+0x1DD` victim the
/// prologue also resolves.
///
/// Arm `3` (`0x801F743C`) raises both halves of the ATK pair -
/// `+0x158` at `0x801F74BC` and `+0x15A` at `0x801F74E4` - by
/// [`power_charge_step`], each with its own cap test, then writes
/// `caster[+0x21C] = 0`.
///
/// The row `docs/subsystems/cast-module.md` grades "none" is an **attack
/// buff**. Its ceiling is `999`, not the `0xFFFF` the item buffs clamp to.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7158 (phase chain + the ATK buff; packet arms unported)
pub fn power_charge_tick(ctx: &mut CastModuleCtx, caster: &mut CastActorState) -> CastTickStep {
    run_tick_latched(ctx, |c| match c.phase {
        3 => {
            caster.atk = power_charge_step(caster.atk);
            caster.atk_base = power_charge_step(caster.atk_base);
            caster.render_flag = 0;
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    })
}

/// The three accessory slots PROT 0955's Void Accessories rolls between -
/// `rand() % 3` at `0x801F6EBC` (magic `0x55555556`).
pub const ACCESSORY_SLOTS: u8 = 3;

/// Offset of the first accessory id inside a per-character record
/// (`0x800848A3` for character 1, stride `0x414`) - `save-record.md`'s
/// `accessory_1_id`. The module forms it as
/// `0x80084140 + (char - 1) * 0x414 + 0x75E + 5 + slot`
/// (`0x801F6EF8..0x801F6F24`).
pub const ACCESSORY_SLOT_0: usize = 0x19B;

/// What PROT 0955's Void Accessories arm decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoidAccessoriesOutcome {
    /// The slot the `rand() % 3` picked, `0..=2`.
    pub slot: u8,
    /// The accessory id lifted out of the record - `None` when the slot was
    /// empty or the second coin flip refused.
    pub voided: Option<u8>,
}

/// PROT 0955 (Void Accessories) tick body - action id `0x73`, 1840 B.
///
/// Arms `0`, `1`, `2`, `3` and `4` (`0x801F6AB0..0x801F6B04`); victim
/// `s1 = actor_table[caster[+0x1DD]]`, and its **character** id comes from
/// `0x8007BD10[victim_seat]`.
///
/// Arm `3` (`0x801F6E60`) is the spell:
///
/// 1. `slot = FUN_80056798() % 3`, stashed at the module word `0x801F9D2C`;
/// 2. read `record[+0x19B + slot]` out of the live game-state block
///    `0x80084140 + (char - 1) * 0x414 + 0x763 + slot`; a zero byte takes the
///    "nothing to void" branch;
/// 3. a second `FUN_80056798() & 1` must be **even** or the arm gives up;
/// 4. `FUN_800421D4(accessory_id, 1)` puts the accessory back in the bag,
///    `FUN_8003CBF8` opens the announcement with the id patched into the
///    module's own string at `0x801F9AD5`, the record byte is cleared, and
///    `FUN_80042558` rebuilds the character's ability bitfield;
/// 5. the victim's `+0x1F1` is staged with a `+0x1DC` bump.
///
/// The row `docs/subsystems/cast-module.md` grades "none" **strips a party
/// member's equipped accessory** and is the only routine in the band that
/// writes the persistent character record rather than the battle actor.
/// `FUN_80042558` is the same ability aggregator the pause menu's equip
/// commit runs, which is why the passive the accessory granted disappears
/// with it.
///
/// `rolls` carries the two `FUN_80056798` draws in order, so the caller keeps
/// retail's RNG cursor. Returns the outcome for the host to apply to its own
/// save record.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A28 (phase chain + the accessory strip; packet arms unported)
pub fn void_accessories_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    accessories: [u8; ACCESSORY_SLOTS as usize],
    rolls: Option<(u32, u32)>,
) -> (CastTickStep, Option<VoidAccessoriesOutcome>) {
    let mut outcome = None;
    let step = run_tick_latched(ctx, |c| match c.phase {
        3 => {
            if let Some((slot_roll, keep_roll)) = rolls {
                let slot = (slot_roll % u32::from(ACCESSORY_SLOTS)) as u8;
                let id = accessories[slot as usize];
                let voided = (id != 0 && keep_roll & 1 == 0).then_some(id);
                if voided.is_some() {
                    let knockdown = victim.knockdown_anim;
                    stage_clip(victim, knockdown);
                }
                outcome = Some(VoidAccessoriesOutcome { slot, voided });
            }
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, outcome)
}

/// The damage shapes of the six trampoline-reached tick **bodies** that call
/// a wrapper, keyed by the body's own VA rather than by its PROT entry.
///
/// [`CAST_DAMAGE_SHAPES`] cannot express these: four of the six live in cells
/// whose trampoline dispatches to *two* bodies with different baked powers
/// (PROT 0938 bakes `0x274` on one arm and `0x309` on the other, PROT 0951
/// `0x3A0` and `0x80`), so an entry-keyed lookup would have to pick one. The
/// entry-keyed table stays what `docs/subsystems/cast-module.md` says it is -
/// the module's **stager** - and a tick's magnitude comes from here.
///
/// Every one clamps shape A: `sltu` against live HP at `0x801F77EC`,
/// `0x801F7118`, `0x801F7438`, `0x801F7FAC`, `0x801F796C` and `0x801F77E0`
/// respectively, so none of them is a never-kill site.
pub const TRAMPOLINE_BODY_SHAPES: [CastDamageShape; 6] = [
    CastDamageShape {
        prot_entry: 938,
        routine: CHAOS_BREATH_TICK,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[CHAOS_BREATH_POWER],
    },
    CastDamageShape {
        prot_entry: 938,
        routine: MYSTIC_CIRCLE_TICK,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[MYSTIC_CIRCLE_POWER],
    },
    CastDamageShape {
        prot_entry: 951,
        routine: CHAOS_FLARE_TICK,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[CHAOS_FLARE_POWER],
    },
    CastDamageShape {
        prot_entry: 951,
        routine: SCYTHE_WIND_TICK,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[SCYTHE_WIND_POWER],
    },
    CastDamageShape {
        prot_entry: 952,
        routine: BLOODY_HORNS_TICK,
        wrapper: CastWrapper::Bypass,
        never_kills: false,
        powers: &[BLOODY_HORNS_POWER],
    },
    CastDamageShape {
        prot_entry: 965,
        routine: DOOMSDAY_TICK,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[DOOMSDAY_POWER],
    },
];

/// The damage shape of one trampoline-reached tick body, by its VA.
///
/// `None` for the six PROT 0955 bodies and for PROT 0952's `0xB8` arm, which
/// call no wrapper at all.
pub fn body_damage_shape(body: u32) -> Option<&'static CastDamageShape> {
    TRAMPOLINE_BODY_SHAPES.iter().find(|s| s.routine == body)
}
