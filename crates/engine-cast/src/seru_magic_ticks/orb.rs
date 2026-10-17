//! PROT 0911 Orb (`0x89`): whole-row heal and status cleanse tick.

use super::*;

/// PROT 0911 (`summon_orb`) tick body - `0x801CF4EC` row `8` (`0x801F1FBC`).
/// The module's image opens with code, so the body sits at the load base.
pub const ORB_TICK: u32 = 0x801F_69D8;

// ---------------------------------------------------------------------------
// PROT 0911 - Orb
// ---------------------------------------------------------------------------

/// The phase PROT 0911's arm `5` jumps straight to, skipping `6`, `7` and `8`
/// (`sb s7,0x279(s5)` at `0x801F7780` with `s7 = 9`).
pub const ORB_ARM5_TARGET_PHASE: u8 = 9;

/// `ctx[+0x278]` arm `5` writes before the jump (`0x801F775C`).
pub const ORB_ARM5_CTX_278: u8 = 2;

/// PROT 0911's heal arm: its countdown runs out, then the whole-row heal
/// and cleanse land in the same pass (`0x801F77D0..`).
pub const ORB_HEAL_ARM: u8 = 9;

/// PROT 0911's settle arm.
pub const ORB_SETTLE_PHASE: u8 = 0x0A;

/// The lowest magic level at which arm `9`'s status cleanse runs at all
/// (`sltiu v0,v0,0x3` at `0x801F7BD8` - below `3` the branch skips the whole
/// ladder).
pub const ORB_CLEANSE_MIN_LEVEL: u8 = 3;

/// The party seats arm `9` un-hides before it heals - `actor_table[0..3]`,
/// each gated on `0x8007BD10[seat] != 0` (the per-seat character index).
pub const ORB_PARTY_SEATS: u8 = 3;

/// The `+0x16E` keep-mask arm `9`'s cleanse ladder applies, selected by the
/// battle-overlay word at `0x801F6960` (read at `0x801F7BE4`, below this
/// module's own load base, so it belongs to PROT 0898 rather than to the
/// module).
///
/// * `1` -> `0xFFFC` - clears Venom and Toxic only;
/// * `2` -> `0xFF84` - also clears `0x08`..`0x40`;
/// * `3` and `4` -> `0xFB84` - additionally clears bit `0x400`, the Kiss of
///   Death mark `crate::cast_module_ticks::FLAG_KISS_OF_DEATH_MARK` names.
///
/// Any other selector clears nothing. Selector `4` additionally doubles the
/// seat's AP gauge `+0x170` with a ceiling of `0x64`
/// (`0x801F7E10..0x801F7E3C`, [`crate::seru_magic_ticks::cure_tier4_ap`]).
pub fn orb_cleanse_mask(selector: u8) -> Option<u16> {
    match selector {
        1 => Some(0xFFFC),
        2 => Some(0xFF84),
        3 | 4 => Some(0xFB84),
        _ => None,
    }
}

/// One seat's outcome inside PROT 0911's heal sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruHeal {
    /// The seat healed.
    pub seat: u8,
    /// HP actually restored, after the clamp to `maxHP - HP`.
    pub restored: u16,
    /// `true` when this seat's AP gauge `+0x170` was doubled (selector `4`
    /// only).
    pub doubled_ap: bool,
}

/// The summon seat's placement words PROT 0911's arms move: `+0x34` /
/// `+0x36` / `+0x38` (x, y, z) and the facing `+0x46` the arm-5 step reads.
/// [`CastActorState`] carries none of them; the host passes the creature's
/// live seat in and writes it back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrbSeatPos {
    /// `+0x34`.
    pub x: i16,
    /// `+0x36` - negative is up.
    pub y: i16,
    /// `+0x38`.
    pub z: i16,
    /// `+0x46`, a 12-bit heading.
    pub facing: u16,
}

/// The colour word arm `9` lights each un-hidden party seat with (`lui a0,
/// 0x2008; ori a0,a0,0x200` / `sw a0,0x4(v0)` at `0x801F77E0..0x801F7814`) -
/// the target cursor's bright word.
pub const ORB_PARTY_LIT_WORD: u32 = 0x2008_0200;

/// The height arm `3` lifts the creature to as it seats it (`li v1,-0x320`
/// / `sh v1,0x36(s4)` at `0x801F70EC..0x801F70F0`); arm `4` lowers it back
/// to the ground.
pub const ORB_DROP_HEIGHT: i16 = -0x320;

/// The height arm `5` lifts the creature to as it steps it forward
/// (`li v0,-0x400` / `sh v0,0x36(s4)` at `0x801F75E4..0x801F75EC`).
pub const ORB_LIFT_HEIGHT: i16 = -0x400;

/// Arm `4`'s re-arm of the module countdown: `scalar * 180`
/// (`0x801F7420..0x801F7440`, `((s * 3) << 4) - s * 3`, `<< 2`).
pub const ORB_ARM4_REARM_MUL: i32 = 180;

/// PROT 0911 (Orb, action id `0x89`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A68..0x801F6AE8` over phases
/// `0,1,2,3,4,5,9,0x0A,0xFF`. `s4 = actor_table[7]` is the summon seat; the
/// busy flag lives on the **stack** (`sw t6,0x38(sp)` with `t6 = 1` at
/// `0x801F6A34`), not in a saved register, which is the same latched-busy
/// convention spelled differently.
///
/// Every hold is the one module countdown `0x801F8558` (`settle` here),
/// drained `rate * speed` a pass, except arm `4`'s, which is the creature's
/// own height:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `2` - once the stream poll passes, countdown `= scalar << 7`
///   (`0x801F6CC8..0x801F6CD8`) and the particle ring is seeded;
/// * `3` - drain, **hold** while positive (`bgtz` at `0x801F70A0`); then
///   seat the creature (`jal 0x801F19EC`), summon `+0x21C = 0`, `+0x04 = 0`,
///   `+0x21B = 8`, `+0x176 = 0x80`, and lift it to `y = -0x320`
///   ([`ORB_DROP_HEIGHT`]);
/// * `4` - summon `+0x36 += rate * speed`, **hold** while it reads negative
///   as a halfword (`bltz` at `0x801F7408`): the creature drops to the
///   ground. Then `+0x36 = 0`, `+0x1DA += 1`, countdown `= scalar * 180`;
/// * `5` - drain, **hold** while positive (`bgtz` at `0x801F7510`); then step
///   the creature an eighth of a unit along its facing (`+0x34 +=
///   sin >> 3`, `+0x38 += cos >> 3`), lift it to `y = -0x400`, countdown
///   `+= scalar << 8`, `ctx[+0x278] = 2`, then `ctx[+0x279] = 9` - a jump,
///   not an advance, which is why `6`, `7` and `8` are unreachable;
/// * `9` - drain, **hold** while positive (`bgtz` at `0x801F77C8`); then
///   un-hide the party seats (`+0x21C = 0`, `+0x04 = 0x20080200`),
///   `ctx[+0x278] = 0`, countdown `+= scalar << 8`, summon `+0x21C = 0xFF`
///   with its colour word `+0x04 = 0`,
///   then the heal sweep over
///   `actor_table[0 .. ctx[+0]]`: skip a dead seat, and unless `+0x16E & 4`
///   `restored = min(amount, maxHP - HP)`, `+0x14C += restored`,
///   `+0x10 = -restored` (a **store**, not an accumulate - the bar delta is
///   overwritten with the negative of the heal), then the cleanse ladder when
///   the caster's magic level is at least [`ORB_CLEANSE_MIN_LEVEL`] - whose
///   selector-`4` arm also doubles the seat's AP gauge `+0x170`, capped at
///   100;
/// * `0x0A` - drain, hold while positive, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// With the engine's one-vsync tick those gates give arm `3` 128 ticks, arm
/// `4` 100, arm `5` 180, arm `9` 256 and arm `0x0A` 256 - retail's measured
/// 128 / 103 / 180 / 256 vsyncs.
///
/// `amount` is [`legaia_engine_battle_vm::battle_formulas::orb_heal_amount`] of the caster's per-magic level, and
/// `max_hp` answers `+0x14E` for a seat, neither of which
/// [`CastActorState`] carries. `cleanse` is the `0x801F6960` selector, or
/// `None` when the level gate fails.
///
/// Not ported: the `record[+0x5D0]` counter the arm bumps by `2` or `4` per
/// healed seat (a save-record write, outside the actor mirror), the
/// `+0x220..0x223` render-byte reset the cleanse ladder performs, and the
/// particle and camera writes.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0911 tick; phase chain, the arm 2..0x0A countdown and drop holds, creature placement, the whole-row heal and cleanse; packet/camera arms unported)
#[allow(clippy::too_many_arguments)]
pub fn orb_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    amount: u32,
    cleanse: Option<u8>,
    settle: &mut i32,
    pos: &mut OrbSeatPos,
    max_hp: impl Fn(u8) -> u16,
) -> (CastTickStep, Vec<SeruHeal>) {
    let mut healed = Vec::new();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        1 => CastArmStep::Advance,
        2 => {
            *settle = SETTLE_SCALAR << 7;
            CastArmStep::Advance
        }
        3 => {
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle > 0 {
                return CastArmStep::Hold;
            }
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
            }
            pos.y = ORB_DROP_HEIGHT;
            CastArmStep::Advance
        }
        4 => {
            pos.y = pos.y.wrapping_add(SETTLE_DRAIN_PER_TICK as i16);
            if pos.y < 0 {
                return CastArmStep::Hold;
            }
            pos.y = 0;
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            *settle = SETTLE_SCALAR * ORB_ARM4_REARM_MUL;
            CastArmStep::Advance
        }
        5 => {
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle > 0 {
                return CastArmStep::Hold;
            }
            let (sin, cos) = legaia_engine_system::trig::trig12(pos.facing);
            pos.x = pos.x.wrapping_add((i32::from(sin) >> 3) as i16);
            pos.z = pos.z.wrapping_add((i32::from(cos) >> 3) as i16);
            pos.y = ORB_LIFT_HEIGHT;
            *settle += SETTLE_SCALAR << 8;
            c.ctx_278 = ORB_ARM5_CTX_278;
            c.phase = ORB_ARM5_TARGET_PHASE;
            CastArmStep::Hold
        }
        ORB_HEAL_ARM => {
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle > 0 {
                return CastArmStep::Hold;
            }
            // `0x801F77D8..0x801F7824`: every occupied party seat (the
            // per-seat character byte `0x8007BD10[seat]`) back to render
            // flag `0` and lit with the bright word, which the tint pass
            // then eases back to neutral.
            for seat in 0..ORB_PARTY_SEATS.min(c.party_count.max(1)) {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                    s.present_04 = ORB_PARTY_LIT_WORD;
                }
            }
            c.ctx_278 = 0;
            // `countdown += scalar << 8` (`0x801F78A4..0x801F78B8`).
            *settle += SETTLE_SCALAR << 8;
            // `sb 0xff,0x21c(s4)` / `sw zero,0x4(s4)` (`0x801F78BC..
            // 0x801F78CC`): the creature's hide is the flag and the zeroed
            // colour word together.
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
                s.present_04 = 0;
            }
            let mask = cleanse.and_then(orb_cleanse_mask);
            for seat in 0..c.party_count {
                let cap = max_hp(seat);
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                // A dead seat is skipped whole (`beq v0,zero,0x801f7e40` at
                // `0x801F7A2C`); `+0x16E & 4` skips only the HP store
                // (`0x801F7B7C`) and the cleanse ladder still runs.
                if v.hp == 0 {
                    continue;
                }
                let stores_hp = (v.flags & FLAG_NON_TARGETABLE) == 0;
                let restored = if stores_hp {
                    let r = u32::from(cap).saturating_sub(u32::from(v.hp)).min(amount) as u16;
                    v.hp = v.hp.wrapping_add(r);
                    v.hp_bar_delta = -i32::from(r);
                    r
                } else {
                    0
                };
                if let Some(m) = mask {
                    v.flags &= m;
                }
                let doubled_ap = cleanse == Some(4);
                if doubled_ap {
                    v.spirit_gauge = crate::seru_magic_ticks::cure_tier4_ap(v.spirit_gauge);
                }
                if stores_hp {
                    healed.push(SeruHeal {
                        seat,
                        restored,
                        doubled_ap,
                    });
                }
            }
            CastArmStep::Advance
        }
        ORB_SETTLE_PHASE => {
            // An ungated drain (`subu; bgtz` at `0x801F7F74..0x801F7F94`):
            // `0xFF` once the word arm 9 re-armed is spent.
            *settle -= SETTLE_DRAIN_PER_TICK;
            if *settle <= 0 {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        _ => CastArmStep::Finish,
    });
    (step, healed)
}
