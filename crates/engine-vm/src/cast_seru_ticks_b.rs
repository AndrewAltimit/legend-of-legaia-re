//! The player Seru-magic band's **tick bodies** for action ids `0x87..=0x8B`
//! (PROT 0909..0913) - the second half of the eleven `0x801CF4EC` arms
//! `docs/subsystems/cast-module.md` grades "code, not data".
//!
//! Each of these five modules carries **two** routines: the `0x801F6734` row
//! the move VM's opcode `0x20` calls (the spawn stager, mostly the pool's
//! data half) and the `0x801CF4EC` arm the battle phase re-enters every frame
//! (the tick body, the choreography). Only PROT 0909's stager touches state;
//! the other four are pure spawn dispatchers. So for this band the tick body
//! *is* the module, and it is what this file ports.
//!
//! | Id | PROT | tick body | shape |
//! |---|---|---|---|
//! | `0x87` | 0909 `summon_viguro` | [`VIGURO_TICK`] | enemy-row damage sweep, two rendezvous phases |
//! | `0x88` | 0910 `summon_swordie` | [`SWORDIE_TICK`] | four-slash damage in a **callee**, [`SWORDIE_SLASH`] |
//! | `0x89` | 0911 `summon_orb` | [`ORB_TICK`] | whole-row **heal** plus a status cleanse |
//! | `0x8A` | 0912 `summon_freed` | [`FREED_TICK`] | enemy-row damage sweep, 20 arms |
//! | `0x8B` | 0913 `summon_nova` | [`NOVA_TICK`] | single-victim damage, 21 arms |
//!
//! Types and idioms come from [`crate::cast_module_ticks`]; this module adds
//! only what the five bodies need that the band did not already have.
//!
//! ## What is ported
//!
//! The dispatch chain's arm map (every one of these five heads is a
//! `beq`/`slti` chain on `ctx[+0x279]`, not a word table), every simulation
//! write that has a mirror on [`CastActorState`] / [`CastModuleCtx`], the
//! damage and heal steps with their baked constants and clamp shapes, the
//! phase walk including its two rendezvous holds, the `ctx+0x278` discipline
//! and the terminal arm's return.
//!
//! Not ported, and disclosed per body below: the packet arms (`FUN_80021B04`
//! / `FUN_80050ED4` spawn sites - those records are
//! `legaia_asset::cast_effect_pool`'s), the camera arms (`FUN_801D829C` /
//! `FUN_801DCEAC`), the seat **pose** fields (`+0x34` / `+0x38` / `+0x46` /
//! `+0x36` / `+0x1BA` / `+0x176` / `+0x21B` / `+0x225` / `+0x21F`), the
//! `+0x04` blend word, the per-arm frame gating (every body times its arms on
//! the scratchpad frame-delta pair `0x1F80037D` / `0x1F800393` against a
//! module-local countdown word, which is capture-pinned timing and not static
//! shape), and the text draws (`FUN_8003541C`).
//!
//! ## Three clamp shapes, not two
//!
//! [`crate::cast_module_ticks`] records two apply shapes for the band - the
//! kill-capable unsigned clamp to HP ([`apply_hit_floor_zero`]) and the
//! signed clamp to `HP - 1`
//! ([`crate::cast_module_ticks::apply_hit_floor_one`]). PROT 0910's per-slash
//! applier is a **third**: an *unsigned* clamp to `HP - 1`
//! ([`apply_hit_unsigned_floor_one`]), and PROT 0910 picks between it and the
//! kill-capable one **per hit**, on the slash counter - so kill-capability is
//! a property of the hit index, not of the module and not even of the
//! routine.
//!
//! ## The phase is driven from two call sites, so the arm map has holes
//!
//! PROT 0909's chain names phases `0,1,2,4,5,6,7,9,0xA..0xE,0xFF` and **not**
//! `3` or `8`: those two fall through to the epilogue with the busy register
//! still `1`, so the tick parks. What releases them is the module's own
//! stager, whose arms `0` and `1` each carry `lbu v0,0x279(v1); addiu v0,v0,1;
//! sb v0,0x279(v1)` (`0x801F7BB8` and `0x801F7C48`). The choreography is
//! therefore a rendezvous between the per-frame tick and the move-VM effect
//! script, and a port that advanced through `3` and `8` would run the cast
//! ahead of its own script. [`viguro_tick`] holds there
//! ([`CastTickStep::Busy`], no advance).
//!
//! PROT 0911's chain has the same *shape* of hole at `6`, `7` and `8` and a
//! different cause: its arm `5` writes `sb s7,0x279(s5)` with `s7 = 9`
//! (`0x801F7780`, the constant materialised eleven hundred instructions
//! earlier at `0x801F6A74` and held in a saved register), so the body
//! **jumps** the phase to `9` and those three arms are unreachable.
//!
//! Provenance: disassembly of each owning image at slot-B base `0x801F69D8`
//! (`scripts/ghidra-analysis/disasm-overlay-fn.py
//! extracted/overlays/overlay_summon_<name>_<entry>.bin --base 0x801F69D8
//! --addr <va>`), plus `crates/asset/data/static-overlays.toml` for the
//! per-entry anchor VA that names each tick.

use crate::cast_module_ticks::{
    ANIM_RATE_NORMAL, CastActorState, CastArmStep, CastDamageShape, CastModuleCtx, CastTickStep,
    CastWrapper, FIRST_MONSTER_SEAT, FLAG_NON_TARGETABLE, SUMMON_SEAT, SweepHit, advance_phase,
    apply_hit_floor_zero,
};

// ---------------------------------------------------------------------------
// The five tick-body addresses
// ---------------------------------------------------------------------------

/// PROT 0909 (`summon_viguro`) tick body - `0x801CF4EC` row `6`, reached from
/// PROT 0898 at `0x801F1F9C`.
pub const VIGURO_TICK: u32 = 0x801F_69F4;
/// PROT 0910 (`summon_swordie`) tick body - `0x801CF4EC` row `7`
/// (`0x801F1FAC`).
pub const SWORDIE_TICK: u32 = 0x801F_69EC;
/// PROT 0910's per-slash applier, reached by three `jal` sites inside
/// [`SWORDIE_TICK`] (`0x801F78E8`, `0x801F7928`, `0x801F7A08`). It is the only
/// routine in this band that writes HP from outside a tick body, which is why
/// a sweep bounded by the tick's own extent reports PROT 0910 as writing no
/// HP at all.
pub const SWORDIE_SLASH: u32 = 0x801F_81DC;
/// PROT 0911 (`summon_orb`) tick body - `0x801CF4EC` row `8` (`0x801F1FBC`).
/// The module's image opens with code, so the body sits at the load base.
pub const ORB_TICK: u32 = 0x801F_69D8;
/// PROT 0912 (`summon_freed`) tick body - `0x801CF4EC` row `9`
/// (`0x801F1FCC`). Same VA as [`ORB_TICK`] in a different image, which is why
/// the band's dispatch has to be keyed on `(entry, body)`.
pub const FREED_TICK: u32 = 0x801F_69D8;
/// PROT 0913 (`summon_nova`) tick body - `0x801CF4EC` row `10`
/// (`0x801F1FDC`). The largest body in the band at 7260 B / 1815
/// instructions.
pub const NOVA_TICK: u32 = 0x801F_69F0;

// ---------------------------------------------------------------------------
// Damage shapes
// ---------------------------------------------------------------------------

/// Every damage site in these five modules goes through `FUN_801DD0AC` with
/// `a1 = 7` - the shared battle kernel's **summon** branch, the same wrapper
/// PROT 0927's AoE stager uses.
///
/// The rest of the band's shared callees, named throughout this module and
/// ported (or owned) elsewhere: the two spawn stagers the pool drives, the
/// two camera helpers, the text draw, the flash spawner, the summon-creature
/// installer and the damage-number ring push.
///
/// REF: FUN_801DD0AC, FUN_80021B04, FUN_80050ED4, FUN_801D829C, FUN_801DCEAC, FUN_8003541C, FUN_80024E80, FUN_801F19EC, FUN_801F44A0
const SUMMON_WRAPPER: CastWrapper = CastWrapper::SharedSummon;

/// The baked per-hit powers, in `(PROT entry, routine, power)` form.
///
/// Each is the `a0` an `addiu a0,zero,<imm>` sets immediately before the
/// `jal 0x801DD0AC`, read off the disassembly rather than off any table:
/// PROT 0909 `0x801F75E8`, PROT 0910 `0x801F8874`, PROT 0912 `0x801F8018`,
/// PROT 0913 `0x801F8438`. Three of the four sit in a `bne` **delay slot**
/// (every one except PROT 0910's), so a backward scan from the `jal` that
/// stops at the branch misses them.
pub const SERU_B_DAMAGE_SHAPES: [CastDamageShape; 4] = [
    CastDamageShape {
        prot_entry: 909,
        routine: VIGURO_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
    CastDamageShape {
        prot_entry: 910,
        routine: SWORDIE_SLASH,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
    CastDamageShape {
        prot_entry: 912,
        routine: FREED_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x10],
    },
    CastDamageShape {
        prot_entry: 913,
        routine: NOVA_TICK,
        wrapper: SUMMON_WRAPPER,
        never_kills: false,
        powers: &[0x12],
    },
];

/// The damage shape of one of the five modules, or `None` for PROT 0911,
/// whose only `+0x14C` write is a **heal**.
///
/// Kept apart from [`crate::cast_module_ticks::CAST_DAMAGE_SHAPES`] on
/// purpose: that table answers for a module's *stager*, and these four sites
/// are all in tick bodies (PROT 0910's in a tick body's callee). Nothing in
/// the engine reads this into the generic cast fold yet - see the module
/// note on the fold seam.
pub fn seru_b_damage_shape(prot_entry: u32) -> Option<&'static CastDamageShape> {
    SERU_B_DAMAGE_SHAPES
        .iter()
        .find(|s| s.prot_entry == prot_entry)
}

// ---------------------------------------------------------------------------
// The idioms these five bodies add
// ---------------------------------------------------------------------------

/// The latched-busy return every body in this file takes, spelled out because
/// `cast_module_ticks`' own copy is private to that module.
///
/// Each of the five seeds a register (or, for PROT 0911 and 0913, a stack
/// word) with `1` in its prologue and returns it; only a terminal arm clears
/// it. An arm that runs and does not reach a `ctx[+0x279] += 1` site
/// therefore reports **busy**, which is what a rendezvous hold is.
fn run_latched(
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

/// Apply shape **C** - an *unsigned* clamp to `HP - 1`.
///
/// ```text
/// v0 = victim[+0x14C]
/// v1 = v0 - 1                  ; the cap
/// sltu v0, v1, dmg             ; UNSIGNED
/// if v0 { dmg = v1 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// PROT 0910's `0x801F88D8..0x801F8910`. It differs from both shapes the band
/// already carries: [`apply_hit_floor_zero`] caps at HP (so it can kill) and
/// `apply_hit_floor_one` compares **signed** (so a negative roll heals). Here
/// the comparison is unsigned against `HP - 1`, so the hit can neither kill
/// nor heal - a negative roll reads as a huge unsigned value and clamps to
/// the cap, leaving the victim on 1 HP.
///
/// Returns the damage actually applied.
pub fn apply_hit_unsigned_floor_one(victim: &mut CastActorState, roll: i32) -> u32 {
    let cap = u32::from(victim.hp).saturating_sub(1);
    let mut dmg = roll as u32;
    if cap < dmg {
        dmg = cap;
    }
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = u32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// The band's three-leg reaction pick, in the form these five bodies spell it
/// (`0x801F7678..0x801F76D8` in PROT 0909 and the same block in 0910 / 0912 /
/// 0913):
///
/// ```text
/// if victim[+0x14C] == 0 || victim[+0x1F2] != 0 { victim[+0x1DA] = victim[+0x1F1] }
/// else { victim[+0x1DA] = victim[+0x1EF] != 0 ? victim[+0x1EF] : victim[+0x1F0] ; alt }
/// ```
///
/// `crate::cast_module_ticks::stage_reaction` carries only the two-leg form
/// (`+0x1F2` picks `+0x1F1` or `+0x1EF`). The three-leg one is what the band
/// actually runs: a **dead** victim takes the knockdown clip regardless of the
/// gate, and a zero `+0x1EF` falls on to `+0x1F0`.
///
/// Returns `true` when the knockdown leg was taken, because the restage write
/// that follows differs between the two legs and between modules.
fn pick_reaction_clip(victim: &mut CastActorState) -> bool {
    if victim.hp == 0 || victim.reaction_gate != 0 {
        victim.staged_anim = victim.knockdown_anim;
        true
    } else {
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        false
    }
}

/// PROT 0909's restage form: the knockdown leg **sets** `+0x1DC = 1`
/// (`sb s5,0x1dc(v0)` at `0x801F76AC` with `s5` seeded `1` at `0x801F75B0`)
/// and the alternate leg sets `5` (`0x801F76D4`). Neither is a bump - the
/// counter is assigned.
fn stage_reaction_set(victim: &mut CastActorState) {
    let knockdown = pick_reaction_clip(victim);
    victim.restage = if knockdown { 1 } else { 5 };
}

/// PROT 0910 / 0912 / 0913's restage form: the alternate leg ORs in `4`
/// (`0x801F80E8`, `0x801F84E0`, `0x801F896C`) and **both** legs then OR in `1`
/// (`0x801F8100`, `0x801F84F0`, `0x801F8978`). So `+0x1DC` is a bit set here
/// and a counter in PROT 0909 - one field, two readings, in one band.
fn stage_reaction_or(victim: &mut CastActorState) {
    if !pick_reaction_clip(victim) {
        victim.restage |= 0x04;
    }
    victim.restage |= 0x01;
}

/// Is this seat in the enemy row's sweep? Both PROT 0909's and PROT 0912's
/// loops open with `lhu +0x14C; beqz skip` then `lhu +0x16E; andi 4; bnez
/// skip`, over `actor_table[3 .. 7]` (`addiu a0,s6,0xc` then `sltiu s3,7`).
fn row_seat_is_hittable(actor: &CastActorState) -> bool {
    actor.hp != 0 && (actor.flags & FLAG_NON_TARGETABLE) == 0
}

/// The seat range PROT 0909's and PROT 0912's sweeps walk: `3..7`, fixed in
/// the bytes as `addiu s3,zero,3` / `sltiu v0,s3,0x7` - not `ctx[+1]`.
pub const ROW_SWEEP_SEAT_END: u8 = 7;

/// "Every enemy seat has finished reacting", the gate PROT 0909 arm `0x0E`,
/// PROT 0912 arm `0x13` and PROT 0913 arm `0x14` all hold on before they
/// write the terminal phase:
///
/// ```text
/// live seat -> require +0x1D9 == 0   (no clip playing)
/// dead seat -> require +0x04  == 0   (the fade word has run out)
/// ```
///
/// The engine has no mirror for `+0x04`, so a dead seat is treated as
/// settled: the fade is presentation, and holding the whole band on it would
/// park the phase on state no host writes.
fn row_is_idle(seats: &[CastActorState], first: u8, end: u8) -> bool {
    (first..end).all(|s| {
        seats
            .get(s as usize)
            .map(|a| a.hp == 0 || a.playing_anim == 0)
            .unwrap_or(true)
    })
}

/// The terminal phase every one of the five chains names (`0xFF`).
pub const SERU_B_DONE_PHASE: u8 = 0xFF;

// ---------------------------------------------------------------------------
// PROT 0909 - Viguro
// ---------------------------------------------------------------------------

/// The two phases PROT 0909's chain does **not** name. The tick parks on each
/// until the module's stager bumps `ctx[+0x279]` from the effect script.
pub const VIGURO_RENDEZVOUS_PHASES: [u8; 2] = [3, 8];
/// PROT 0909's damage arm.
pub const VIGURO_DAMAGE_PHASE: u8 = 0x0D;
/// PROT 0909's settle arm - the one that writes [`SERU_B_DONE_PHASE`].
pub const VIGURO_SETTLE_PHASE: u8 = 0x0E;
/// `ctx[+0x278]` arm `0` writes (`addiu v0,zero,3; sb v0,0x278(v1)` at
/// `0x801F6BA0`), paired with `ctx[+0x27A] = 0x80` (`0x801F6B94`).
pub const VIGURO_ARM0_CTX_278: u8 = 3;
/// `ctx[+0x27A]` arm `0` writes.
pub const VIGURO_ARM0_CTX_27A: u8 = 0x80;
/// The summon seat's render flag in arm `7` (`addiu v0,zero,4; sb
/// v0,0x21c(s4)` at `0x801F6F54`).
pub const VIGURO_ARM7_RENDER_FLAG: u8 = 4;
/// The render flag arm `0x0B` writes on every live enemy seat
/// (`addiu a1,zero,4` at `0x801F7240`, stored at `0x801F726C`).
pub const VIGURO_ARM11_ROW_RENDER_FLAG: u8 = 4;
/// The summon seat's render flag in the terminal arm (`addiu v0,zero,2; sb
/// v0,0x21c(s4)` at `0x801F78F8`).
pub const VIGURO_DONE_RENDER_FLAG: u8 = 2;
/// The `+0x0C` root speed the damage arm writes on each seat it hit
/// (`0x801F76EC`).
pub const VIGURO_HIT_ROOT_SPEED: i32 = 0x1000;

/// PROT 0909 (Viguro, action id `0x87`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A6C..0x801F6B24` over phases
/// `0,1,2,4,5,6,7,9,0x0A,0x0B,0x0C,0x0D,0x0E,0xFF`, driven through
/// `s7 = ctx + 0x279` (materialised at `0x801F6A70`). `s4 = actor_table[7]`
/// is the summon seat, `s2` the context and `s6` the actor table; the
/// **caster** is only reached in arm `9`, as `actor_table[ctx[+0x13]]`.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 3`, `ctx[+0x27A] = 0x80` (the band of
///   `ctx[+0x27B..0x283]` bytes written beside them is shadow / camera setup);
/// * `3` - **rendezvous hold**, see the module note;
/// * `4` - the summon seat's `+0x1DA` and `+0x1DC` both `+= 1`
///   (`0x801F6D38`), after the arm draws the spell name out of
///   `0x800754C8[caster[+0x1DF]] + 8`;
/// * `7` - summon `+0x21C = 4`, then the same `+0x1DA` / `+0x1DC` bump;
/// * `8` - **rendezvous hold**;
/// * `9` - summon `+0x21C = 0xFF` (the fade-out), `ctx[+0x278] = 0`, and
///   `caster[+0x1DD] = 9`. That `9` is not an immediate: the store is
///   `sb s0,0x1dd(v0)` at `0x801F7058` and `s0` is still the **phase** the
///   prologue loaded at `0x801F6A64`. It happens to equal the enemy-row group
///   code the stager writes, which is what made the constant legible at all;
/// * `0x0B` - `+0x21C = 4` on every live enemy seat;
/// * `0x0D` - the damage sweep (below);
/// * `0x0E` - hold until the enemy row is idle, then write phase `0xFF`;
/// * `0xFF` - `ctx[+0x27A] = 0`, summon `+0x21C = 2`, return zero.
///
/// The damage sweep walks `actor_table[3..7]`, skips a dead or
/// `+0x16E & 4` seat, calls `FUN_801DD0AC(0x12, 7, seat)`, clamps **shape A**
/// (`sltu` against HP at `0x801F7638`, so it can kill), accumulates `+0x10`
/// with a **word** store, writes `+0x14C`, stages the three-leg reaction with
/// the *assigning* restage form, and finishes each seat with `+0x0C = 0x1000`
/// and `+0x21C = 0`. When exactly one seat was hittable it also writes the
/// roll to `0x8007BD14` and `0x78` to `0x8007B64C` - two battle globals the
/// engine has no mirror for, reported back as
/// [`ViguroSweep::single_target_roll`] rather than dropped.
///
/// `rolls` is called once per hittable seat, in seat order, so a host's RNG
/// cursor advances the way retail's does.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F4 (PROT 0909 tick; phase chain + the enemy-row damage sweep, packet/camera arms unported)
pub fn viguro_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    caster_seat: u8,
    summon_seat: u8,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, ViguroSweep) {
    let mut sweep = ViguroSweep::default();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = VIGURO_ARM0_CTX_278;
            c.ctx_27a = VIGURO_ARM0_CTX_27A;
            CastArmStep::Advance
        }
        4 => {
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        7 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = VIGURO_ARM7_RENDER_FLAG;
            }
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        9 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            if let Some(s) = seats.get_mut(caster_seat as usize) {
                // The reused-register `9`; see the doc comment.
                s.target_code = crate::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
            }
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        0x0B => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                if let Some(s) = seats.get_mut(seat as usize)
                    && s.hp != 0
                {
                    s.render_flag = VIGURO_ARM11_ROW_RENDER_FLAG;
                }
            }
            CastArmStep::Advance
        }
        VIGURO_DAMAGE_PHASE => {
            let live = (FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END)
                .filter(|s| {
                    seats
                        .get(*s as usize)
                        .map(row_seat_is_hittable)
                        .unwrap_or(false)
                })
                .count();
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if !row_seat_is_hittable(v) {
                    continue;
                }
                let roll = rolls(seat);
                if live == 1 {
                    sweep.single_target_roll = Some(roll);
                }
                let applied = apply_hit_floor_zero(v, roll);
                stage_reaction_set(v);
                v.root_speed = VIGURO_HIT_ROOT_SPEED;
                v.render_flag = 0;
                sweep.hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        VIGURO_SETTLE_PHASE => {
            if row_is_idle(seats, FIRST_MONSTER_SEAT, ROW_SWEEP_SEAT_END) {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => {
            c.ctx_27a = 0;
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = VIGURO_DONE_RENDER_FLAG;
            }
            CastArmStep::Finish
        }
        p if VIGURO_RENDEZVOUS_PHASES.contains(&p) => CastArmStep::Hold,
        // Named-but-presentation arms (1, 2, 5, 6, 0x0A, 0x0C) advance.
        1 | 2 | 5 | 6 | 0x0A | 0x0C => CastArmStep::Advance,
        // Retail returns busy for a phase the chain does not name; the port
        // reports Done so a host that holds on `busy` cannot park on a phase
        // nothing in the module can leave.
        _ => CastArmStep::Finish,
    });
    (step, sweep)
}

/// What PROT 0909's damage arm did this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViguroSweep {
    /// Per-seat outcome, in the sweep's own seat order.
    pub hits: Vec<SweepHit>,
    /// The roll the arm published to `0x8007BD14` because exactly one enemy
    /// seat was hittable (`bne s4,s5` at `0x801F75FC`, with `s5 = 1`). Retail
    /// also sets `0x8007B64C = 0x78` on that path.
    pub single_target_roll: Option<i32>,
}

/// The `+0x1DA` / `+0x1DC` pair bump both PROT 0909 arms `4` and `7` and PROT
/// 0912 arm `0x0A` perform on the summon seat.
fn bump_stage(seats: &mut [CastActorState], seat: u8) {
    if let Some(s) = seats.get_mut(seat as usize) {
        s.staged_anim = s.staged_anim.wrapping_add(1);
        s.restage = s.restage.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// PROT 0910 - Swordie
// ---------------------------------------------------------------------------

/// PROT 0910's settle arm.
pub const SWORDIE_SETTLE_PHASE: u8 = 0x0A;
/// The clip arm `2` stages on the summon seat, with `+0x1DC` **set** to the
/// same value (`sb s8,0x1da(s2)` / `sb s8,0x1dc(s2)` at `0x801F6DB0`, `s8 = 1`).
pub const SWORDIE_ARM2_CLIP: u8 = 1;
/// The clip arm `5` stages, this time with a `+0x1DC` **bump**
/// (`0x801F714C`).
pub const SWORDIE_ARM5_CLIP: u8 = 2;
/// The summon seat's render flag at the end of arm `5` (`li v0,0x7` at
/// `0x801F762C`, stored at `0x801F7634`).
pub const SWORDIE_ARM5_RENDER_FLAG: u8 = 7;
/// How many slashes [`swordie_slash`] runs per cast - the `sltiu s0,0x4`
/// bound on the three `FUN_801F81DC` loops (`0x801F77E8`, `0x801F7934`,
/// `0x801F7A14`).
pub const SWORDIE_SLASHES: u8 = 4;
/// The slash index whose reaction is the knockdown (`li v0,3; bne s7,v0` at
/// `0x801F8930`): only the last one knocks the victim down.
pub const SWORDIE_KNOCKDOWN_SLASH: u8 = 3;
/// The hit counter value at which the clamp becomes kill-capable (`li v0,0x4;
/// beq v1,v0` at `0x801F88CC`, against the module counter at `0x801F8DAC`
/// that the same block just incremented).
pub const SWORDIE_LETHAL_HIT: u32 = 4;

/// PROT 0910 (Swordie, action id `0x88`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A8C..0x801F6B30` over phases `0..=0x0A`
/// and `0xFF` - one of the three gapless runs in this band (with PROT 0912
/// and 0913; PROT 0909 and 0911 both have holes). `s2 = actor_table[7]` is
/// the summon seat and `s3 = actor_table[caster[+0x1DD]]` the victim, resolved
/// in the prologue behind `sltiu v1,0x8`; a target byte of `8` or more leaves
/// `s3` uninitialised, which is retail's own hole, not the port's.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `2` - summon `+0x1DA = 1`, `+0x1DC = 1`, `+0x21C = 0`, `+0x225 = 0`;
/// * `3` - summon `+0x1DA = 0` when its ramp crosses `0x40`, plus `+0x21C = 0`;
/// * `5` - summon `+0x1DA = 2` (a plain store), `+0x1DC += 1`, `+0x21C = 7`;
/// * `7` - summon `+0x21D = scratch[0x37D]`, i.e. the animation rate is set to
///   the **frame delta** for the strike, then restored by the next clip;
/// * `8` - victim `+0x21C = 0`;
/// * `0x0A` - hold until the victim is idle, then write phase `0xFF`;
/// * `0xFF` - return zero.
///
/// **The tick body itself writes no HP and calls no damage wrapper.** That is
/// what `docs/subsystems/cast-module.md`'s "`0` wrappers, `0` HP" cell
/// measures, and it is true only of the tick's own extent: the cast's damage
/// is [`swordie_slash`], a separate framed routine the tick calls four times.
///
/// `frame_delta` is the scratchpad byte at `0x1F80037D` - the per-frame tick
/// count every body in the band paces on. It has no engine mirror, so arm `7`
/// takes it as an argument rather than inventing one.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69EC (PROT 0910 tick; phase chain + staging, packet/camera arms unported)
pub fn swordie_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    victim_seat: u8,
    frame_delta: u8,
) -> CastTickStep {
    run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        2 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = SWORDIE_ARM2_CLIP;
                s.restage = SWORDIE_ARM2_CLIP;
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        3 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
                s.staged_anim = 0;
            }
            CastArmStep::Advance
        }
        5 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = SWORDIE_ARM5_CLIP;
                s.restage = s.restage.wrapping_add(1);
                s.render_flag = SWORDIE_ARM5_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        7 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.anim_rate = frame_delta;
            }
            CastArmStep::Advance
        }
        8 => {
            if let Some(s) = seats.get_mut(victim_seat as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        SWORDIE_SETTLE_PHASE => {
            let settled = seats
                .get(victim_seat as usize)
                .map(|v| v.hp == 0 || v.playing_anim == 0)
                .unwrap_or(true);
            if settled {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        1 | 4 | 6 | 9 => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    })
}

/// PROT 0910's per-slash applier - three `jal` sites inside [`SWORDIE_TICK`],
/// `slash` being the loop index `0..4` retail passes in `a0`.
///
/// ```text
/// counter = ++module[0x801F8DAC]
/// roll    = FUN_801DD0AC(0x12, 7, caster[+0x1DD])
/// 0x8007BD14 -= roll - (roll >> 2)                  ; the running total
/// cap = counter == 4 ? victim[+0x14C] : victim[+0x14C] - 1
/// if cap < roll { roll = cap }                       ; UNSIGNED
/// victim[+0x10]  += roll
/// victim[+0x14C] -= roll
/// ```
///
/// The cap switch is the finding this routine carries: the first three
/// slashes clamp to `HP - 1` and **cannot kill**, the fourth clamps to `HP`
/// and can. Kill-capability in this band is therefore per **hit**, not per
/// module - `crate::cast_module_ticks`' entry-keyed
/// `crate::cast_module_ticks::damage_shape_for` cannot express it and does
/// not claim to.
///
/// The reaction is staged with the OR restage form, and the knockdown clip
/// only on `slash == 3`; on every other slash the victim takes the alternate
/// reaction even when the gate `+0x1F2` is set.
///
/// Returns the damage applied.
///
/// PORT: FUN_801F81DC NOT WIRED: the host that should call it is
/// [`swordie_tick`]'s arms `9` and `0x0A`, which is where retail's three
/// `jal` sites live. They stay silent here for two reasons: the per-slash
/// firing order is the module's own frame gate (`slot[i] != 0` at
/// `0x801F78D8`, then `(i + 2) * scratch[0x37D] * 16 < ctx[+0x6D8]` at
/// `0x801F7900..0x801F7920`), which is capture-pinned timing this port does
/// not carry; and the cast-band seam folds a cast's HP outcome once at
/// `World::cast_spell_on_slots_prepaid`, so a second application here would
/// double it. The kernel is exercised by this module's own tests.
/// REF: FUN_801F69EC
pub fn swordie_slash(victim: &mut CastActorState, slash: u8, hit_counter: u32, roll: i32) -> u32 {
    let applied = if hit_counter == SWORDIE_LETHAL_HIT {
        apply_hit_floor_zero(victim, roll)
    } else {
        apply_hit_unsigned_floor_one(victim, roll)
    };
    if slash == SWORDIE_KNOCKDOWN_SLASH {
        stage_reaction_or(victim);
    } else {
        // The `bne s7,3` leg skips the knockdown pick entirely and always
        // takes the alternate clip.
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        victim.restage |= 0x04;
        victim.restage |= 0x01;
    }
    applied
}

// ---------------------------------------------------------------------------
// PROT 0911 - Orb
// ---------------------------------------------------------------------------

/// The phase PROT 0911's arm `5` jumps straight to, skipping `6`, `7` and `8`
/// (`sb s7,0x279(s5)` at `0x801F7780` with `s7 = 9`).
pub const ORB_ARM5_TARGET_PHASE: u8 = 9;
/// `ctx[+0x278]` arm `5` writes before the jump (`0x801F775C`).
pub const ORB_ARM5_CTX_278: u8 = 2;
/// PROT 0911's settle arm.
pub const ORB_SETTLE_PHASE: u8 = 0x0A;
/// The lowest magic level at which arm `9`'s status cleanse runs at all
/// (`sltiu v0,v0,0x3` at `0x801F7BD8` - below `3` the branch skips the whole
/// ladder).
pub const ORB_CLEANSE_MIN_LEVEL: u8 = 3;
/// The party seats arm `9` un-hides before it heals - `actor_table[0..3]`,
/// each gated on `0x8007BD10[seat] != 0` (the per-seat character index).
pub const ORB_PARTY_SEATS: u8 = 3;

/// PROT 0911's inline heal: `(magic_level << 6) + 0x1C0`, i.e.
/// `level * 64 + 448`.
///
/// `magic_level` is **not** a spell-table field. Arm `9` walks the caster's
/// own character record - `0x80084140 + (0x8007BD10[ctx[+0x13]] - 1) * 0x414`,
/// the `0x414`-stride live record block `docs/formats/save-record.md`
/// describes - scanning the 32-entry learned-id list at record `+0x13D` for
/// `caster[+0x1DF]` and reading the parallel byte at record `+0x161`
/// (`0x801F7AB4` / `0x801F7AD8` under a `0x80084140` base, so `+0x705` and
/// `+0x729` from there).
///
/// `docs/formats/spell-table.md` records this family's heal as
/// `(power_byte << 5) + 0xE0`. PROT 0911's shift is `0x6` and its addend
/// `0x1C0` (`sll v0,v0,0x6` at `0x801F7AE0`, `addiu a3,v0,0x1c0` at
/// `0x801F7AEC`) - exactly twice the documented amount - so the formula is
/// per module, not band-wide.
pub fn orb_heal_amount(magic_level: u8) -> u32 {
    (u32::from(magic_level) << 6) + 0x1C0
}

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
/// victim's `+0x170` with a ceiling of `0x64`, which has no mirror on
/// [`CastActorState`] and is reported rather than applied.
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
    /// `true` when this seat's `+0x170` was doubled (selector `4` only).
    pub doubled_resist: bool,
}

/// PROT 0911 (Orb, action id `0x89`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A68..0x801F6AE8` over phases
/// `0,1,2,3,4,5,9,0x0A,0xFF`. `s4 = actor_table[7]` is the summon seat; the
/// busy flag lives on the **stack** (`sw t6,0x38(sp)` with `t6 = 1` at
/// `0x801F6A34`), not in a saved register, which is the same latched-busy
/// convention spelled differently.
///
/// This module is the band's **heal**, and its `1` HP store is an `addu`:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `3` - summon `+0x21C = 0`, `+0x04 = 0`, `+0x21B = 8`, `+0x176 = 0x80`;
/// * `4` - summon `+0x1DA += 1`;
/// * `5` - `ctx[+0x278] = 2`, then `ctx[+0x279] = 9` - a jump, not an
///   advance, which is why `6`, `7` and `8` are unreachable;
/// * `9` - un-hide the party seats (`+0x21C = 0`, `+0x04 = 0x20080200`),
///   `ctx[+0x278] = 0`, summon `+0x21C = 0xFF`, then the heal sweep over
///   `actor_table[0 .. ctx[+0]]`: skip a dead seat, skip `+0x16E & 4`,
///   `restored = min(amount, maxHP - HP)`, `+0x14C += restored`,
///   `+0x10 = -restored` (a **store**, not an accumulate - the bar delta is
///   overwritten with the negative of the heal), then the cleanse ladder when
///   the caster's magic level is at least [`ORB_CLEANSE_MIN_LEVEL`];
/// * `0x0A` - hold until the module's countdown expires, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// `amount` is [`orb_heal_amount`] of the caster's per-magic level, and
/// `max_hp` answers `+0x14E` for a seat, neither of which
/// [`CastActorState`] carries. `cleanse` is the `0x801F6960` selector, or
/// `None` when the level gate fails.
///
/// Not ported: the `record[+0x5D0]` counter the arm bumps by `2` or `4` per
/// healed seat (a save-record write, outside the actor mirror) and the
/// `+0x220..0x223` render-byte reset the cleanse ladder performs.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0911 tick; phase chain + the whole-row heal and cleanse, packet/camera arms unported)
pub fn orb_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    amount: u32,
    cleanse: Option<u8>,
    max_hp: impl Fn(u8) -> u16,
) -> (CastTickStep, Vec<SeruHeal>) {
    let mut healed = Vec::new();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        3 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        4 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        5 => {
            c.ctx_278 = ORB_ARM5_CTX_278;
            c.phase = ORB_ARM5_TARGET_PHASE;
            CastArmStep::Hold
        }
        9 => {
            for seat in 0..ORB_PARTY_SEATS {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
            }
            c.ctx_278 = 0;
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            let mask = cleanse.and_then(orb_cleanse_mask);
            for seat in 0..c.party_count {
                let cap = max_hp(seat);
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if v.hp == 0 || (v.flags & FLAG_NON_TARGETABLE) != 0 {
                    continue;
                }
                let restored = u32::from(cap).saturating_sub(u32::from(v.hp)).min(amount) as u16;
                v.hp = v.hp.wrapping_add(restored);
                v.hp_bar_delta = -i32::from(restored);
                if let Some(m) = mask {
                    v.flags &= m;
                }
                healed.push(SeruHeal {
                    seat,
                    restored,
                    doubled_resist: cleanse == Some(4),
                });
            }
            CastArmStep::Advance
        }
        ORB_SETTLE_PHASE => {
            c.phase = SERU_B_DONE_PHASE;
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        1 | 2 => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, healed)
}

// ---------------------------------------------------------------------------
// PROT 0912 - Freed
// ---------------------------------------------------------------------------

/// PROT 0912's damage arm.
pub const FREED_DAMAGE_PHASE: u8 = 0x11;
/// PROT 0912's settle arm.
pub const FREED_SETTLE_PHASE: u8 = 0x13;
/// `ctx[+0x278]` arm `0x0E` writes (`0x801F7D5C`).
pub const FREED_ARM14_CTX_278: u8 = 2;
/// The `+0x0C` root speed arm `0x10` and arm `7` write (`0x801F7F0C`,
/// `0x801F74D4`).
pub const FREED_ROOT_SPEED: i32 = 0x1000;
/// The animation rate the damage arm leaves on each seat it hit
/// (`0x801F8140`).
pub const FREED_HIT_ANIM_RATE: u8 = 2;

/// PROT 0912 (Freed, action id `0x8A`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A50..0x801F6B6C` over phases `0..=0x13`
/// and `0xFF` - twenty consecutive arms, the widest in this band, through
/// `s4 = ctx + 0x279`. `s2 = actor_table[7]` is the summon seat.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `2` - summon `+0x21C = 0xFF`, `+0x04 = 0`;
/// * `7` - summon `+0x0C = 0x1000`, `+0x21C = 0xFF`;
/// * `9` - summon `+0x21C = 0`;
/// * `0x0A` - summon `+0x1DA += 1`, `+0x1DC += 1`;
/// * `0x0E` - summon `+0x21C = 0xFF` (out of `s0`, loaded `0xFF` at
///   `0x801F7D04` for a colour store nine instructions earlier - another
///   reused-register constant), `+0x04 = 0`, `ctx[+0x278] = 2`;
/// * `0x0F` - every live enemy seat `+0x21C = 0`;
/// * `0x10` - every live enemy seat `+0x0C = 0x1000`, `+0x21C = 0xFF`,
///   `+0x21D = 0`;
/// * `0x11` - the damage sweep: `actor_table[3..7]`, skip dead / `+0x16E & 4`,
///   `FUN_801DD0AC(0x10, 7, seat)`, clamp **shape A** (`sltu` at
///   `0x801F8050`), `+0x10 +=`, `+0x14C -=`, the OR restage form,
///   `+0x21C = 0`, `+0x21D = 2`;
/// * `0x13` - hold until the enemy row is idle, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// This body's three module-local words (`0x801F92A8`, `0x801F92AC`,
/// `0x801F92B0`) sit in the image's own zero-filled scratch, which begins at
/// file `+0x28D0`: the run above the spawn records is partly this module's
/// BSS, not donor residue.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0912 tick; phase chain + the enemy-row damage sweep, packet/camera arms unported)
pub fn freed_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        2 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            CastArmStep::Advance
        }
        7 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.root_speed = FREED_ROOT_SPEED;
                s.render_flag = 0xFF;
            }
            CastArmStep::Advance
        }
        9 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        0x0A => {
            bump_stage(seats, summon_seat);
            CastArmStep::Advance
        }
        0x0E => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            c.ctx_278 = FREED_ARM14_CTX_278;
            CastArmStep::Advance
        }
        0x0F => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                if let Some(s) = seats.get_mut(seat as usize)
                    && s.hp != 0
                {
                    s.render_flag = 0;
                }
            }
            CastArmStep::Advance
        }
        0x10 => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                if let Some(s) = seats.get_mut(seat as usize)
                    && s.hp != 0
                {
                    s.root_speed = FREED_ROOT_SPEED;
                    s.render_flag = 0xFF;
                    s.anim_rate = 0;
                }
            }
            CastArmStep::Advance
        }
        FREED_DAMAGE_PHASE => {
            for seat in FIRST_MONSTER_SEAT..ROW_SWEEP_SEAT_END {
                let Some(v) = seats.get_mut(seat as usize) else {
                    continue;
                };
                if !row_seat_is_hittable(v) {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, rolls(seat));
                stage_reaction_or(v);
                v.render_flag = 0;
                v.anim_rate = FREED_HIT_ANIM_RATE;
                hits.push(SweepHit { seat, applied });
            }
            CastArmStep::Advance
        }
        FREED_SETTLE_PHASE => {
            if row_is_idle(seats, FIRST_MONSTER_SEAT, ROW_SWEEP_SEAT_END) {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        1 | 3 | 4 | 5 | 6 | 8 | 0x0B | 0x0C | 0x0D | 0x12 => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

// ---------------------------------------------------------------------------
// PROT 0913 - Nova
// ---------------------------------------------------------------------------

/// PROT 0913's damage arm.
pub const NOVA_DAMAGE_PHASE: u8 = 0x13;
/// PROT 0913's settle arm.
pub const NOVA_SETTLE_PHASE: u8 = 0x14;
/// The clip arm `9` stages on the summon seat, and the same value it writes
/// to `ctx[+0x278]` in the same arm out of the same register (`li s0,0x2` at
/// `0x801F7788`, stored at `0x801F778C` and `0x801F77F0`).
pub const NOVA_ARM9_CLIP: u8 = 2;
/// The victim's render flag in arm `0x0F` (`li v1,0x7` at `0x801F7E4C`).
pub const NOVA_ARM15_VICTIM_RENDER_FLAG: u8 = 7;

/// PROT 0913 (Nova, action id `0x8B`) tick body - the band's largest.
///
/// A `beq`/`slti` chain at `0x801F6A8C..0x801F6BC4` over phases `0..=0x14`
/// and `0xFF`, twenty-one consecutive arms. `s8 = actor_table[7]` is the
/// summon seat and `s2 = actor_table[caster[+0x1DD]]` the victim; the busy
/// flag is the stack word `0x38(sp)`.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`;
/// * `3` - summon `+0x1DA += 1`;
/// * `9` - summon `+0x1DA = 2`, `+0x1DC += 1`, `ctx[+0x278] = 2`;
/// * `0x0D` - victim `+0x21C = 0`, summon `+0x04 = 0`, `+0x21C = 0xFF`;
/// * `0x0F` - victim `+0x21C = 7`;
/// * `0x13` - the damage step: **one** victim, and the only guard is
///   `+0x16E & 4` - there is no dead-seat skip, so retail will roll against a
///   corpse. `FUN_801DD0AC(0x12, 7, caster[+0x1DD])`, clamp **shape A**
///   (`sltu` at `0x801F8470`), `+0x10 +=`, `+0x14C -=`, the OR restage form,
///   then `+0x04 = 0` and `+0x21C = 0` over `actor_table[0 .. ctx[+0]]`;
/// * `0x14` - hold until the victim is idle, then phase `0xFF`;
/// * `0xFF` - return zero.
///
/// `docs/reference/functions/battle.md` groups PROT 0913 with the band's
/// *heal* modules. It is not one: the store at `0x801F8490` is `subu` under a
/// `FUN_801DD0AC` roll, the same damage shape PROT 0909 and 0912 take.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69F0 (PROT 0913 tick; phase chain + the single-victim damage step, packet/camera arms unported)
pub fn nova_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    victim_seat: u8,
    roll: i32,
) -> (CastTickStep, Option<SweepHit>) {
    let mut hit = None;
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            CastArmStep::Advance
        }
        3 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = s.staged_anim.wrapping_add(1);
            }
            CastArmStep::Advance
        }
        9 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = NOVA_ARM9_CLIP;
                s.restage = s.restage.wrapping_add(1);
            }
            c.ctx_278 = NOVA_ARM9_CLIP;
            CastArmStep::Advance
        }
        0x0D => {
            if let Some(s) = seats.get_mut(victim_seat as usize) {
                s.render_flag = 0;
            }
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0xFF;
            }
            CastArmStep::Advance
        }
        0x0F => {
            if let Some(s) = seats.get_mut(victim_seat as usize) {
                s.render_flag = NOVA_ARM15_VICTIM_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        NOVA_DAMAGE_PHASE => {
            if let Some(v) = seats.get_mut(victim_seat as usize)
                && (v.flags & FLAG_NON_TARGETABLE) == 0
            {
                let applied = apply_hit_floor_zero(v, roll);
                stage_reaction_or(v);
                hit = Some(SweepHit {
                    seat: victim_seat,
                    applied,
                });
            }
            for seat in 0..c.party_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
            }
            CastArmStep::Advance
        }
        NOVA_SETTLE_PHASE => {
            let settled = seats
                .get(victim_seat as usize)
                .map(|v| v.hp == 0 || v.playing_anim == 0)
                .unwrap_or(true);
            if settled {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.anim_rate = ANIM_RATE_NORMAL;
            }
            CastArmStep::Finish
        }
        1 | 2 | 4 | 5 | 6 | 7 | 8 | 0x0A | 0x0B | 0x0C | 0x0E | 0x10 | 0x11 | 0x12 => {
            CastArmStep::Advance
        }
        _ => CastArmStep::Finish,
    });
    (step, hit)
}

/// The five modules' seat conventions in one place, so a host does not have
/// to re-derive them: every body takes `actor_table[7]` as the summon seat
/// ([`SUMMON_SEAT`]), the enemy row as `3..7` ([`FIRST_MONSTER_SEAT`] ..
/// [`ROW_SWEEP_SEAT_END`]), and the single-victim bodies resolve their target
/// as `actor_table[caster[+0x1DD]]` behind a `sltiu ...,8` bound.
pub const SERU_B_SUMMON_SEAT: u8 = SUMMON_SEAT;

#[cfg(test)]
mod tests {
    use super::*;

    fn seats(n: usize) -> Vec<CastActorState> {
        (0..n)
            .map(|_| CastActorState {
                hp: 400,
                anim_rate: ANIM_RATE_NORMAL,
                knockdown_anim: 0x20,
                reaction_alt: 0x21,
                reaction_alt2: 0x22,
                ..Default::default()
            })
            .collect()
    }

    fn ctx_at(phase: u8) -> CastModuleCtx {
        CastModuleCtx {
            party_count: 8,
            monster_count: 4,
            caster_seat: 0,
            phase,
            ..Default::default()
        }
    }

    #[test]
    fn viguro_arm_zero_writes_both_context_bytes() {
        let mut ctx = ctx_at(0);
        let mut s = seats(8);
        let (step, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 0);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.ctx_278, VIGURO_ARM0_CTX_278);
        assert_eq!(ctx.ctx_27a, VIGURO_ARM0_CTX_27A);
        assert_eq!(ctx.phase, 1, "arm 0 advances");
        assert!(sweep.hits.is_empty());
    }

    /// The rendezvous: phases 3 and 8 report busy and do **not** advance, so
    /// the choreography waits for the stager's own `ctx[+0x279] += 1`.
    #[test]
    fn viguro_parks_on_the_two_rendezvous_phases() {
        for p in VIGURO_RENDEZVOUS_PHASES {
            let mut ctx = ctx_at(p);
            let mut s = seats(8);
            let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 0);
            assert_eq!(step, CastTickStep::Busy, "phase {p} holds");
            assert_eq!(ctx.phase, p, "phase {p} does not advance");
        }
    }

    /// The sweep's bound is the fixed `3..7` seat range, not `ctx[+1]`, and
    /// its clamp can kill.
    #[test]
    fn viguro_sweep_hits_the_enemy_row_only_and_can_kill() {
        let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
        let mut s = seats(8);
        s[4].flags |= FLAG_NON_TARGETABLE;
        s[5].hp = 0;
        let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 10_000);
        let seats_hit: Vec<u8> = sweep.hits.iter().map(|h| h.seat).collect();
        assert_eq!(seats_hit, vec![3, 6], "dead and untargetable seats skipped");
        assert_eq!(s[3].hp, 0, "shape A clamps to HP, so the hit kills");
        assert_eq!(s[0].hp, 400, "the party row is untouched");
        assert_eq!(s[3].hp_bar_delta, 400);
        assert_eq!(
            s[3].staged_anim, 0x20,
            "a killed seat takes the knockdown clip"
        );
        assert_eq!(s[3].restage, 1, "PROT 0909 assigns the restage counter");
        assert_eq!(s[3].root_speed, VIGURO_HIT_ROOT_SPEED);
    }

    /// The one-live-target path publishes the roll retail writes to
    /// `0x8007BD14`.
    #[test]
    fn viguro_reports_the_single_target_roll() {
        let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
        let mut s = seats(8);
        for a in s[4..7].iter_mut() {
            a.hp = 0;
        }
        let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 25);
        assert_eq!(sweep.single_target_roll, Some(25));
        assert_eq!(sweep.hits.len(), 1);
    }

    /// Two live targets: the arm does not publish.
    #[test]
    fn viguro_does_not_publish_with_two_targets() {
        let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
        let mut s = seats(8);
        s[5].hp = 0;
        s[6].hp = 0;
        let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 25);
        assert_eq!(sweep.single_target_roll, None);
        assert_eq!(sweep.hits.len(), 2);
    }

    /// A live victim with `+0x1F2 == 0` takes the alternate clip and the
    /// assigning restage `5`.
    #[test]
    fn viguro_alternate_reaction_sets_restage_five() {
        let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
        let mut s = seats(8);
        for a in s[4..7].iter_mut() {
            a.hp = 0;
        }
        viguro_tick(&mut ctx, &mut s, 0, 7, |_| 1);
        assert_eq!(s[3].staged_anim, 0x21);
        assert_eq!(s[3].restage, 5);
    }

    #[test]
    fn viguro_settles_only_when_the_row_is_idle() {
        let mut ctx = ctx_at(VIGURO_SETTLE_PHASE);
        let mut s = seats(8);
        s[3].playing_anim = 4;
        let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 0);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, VIGURO_SETTLE_PHASE);

        s[3].playing_anim = 0;
        let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 0);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, SERU_B_DONE_PHASE);

        let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, |_| 0);
        assert_eq!(step, CastTickStep::Done);
        assert_eq!(s[7].render_flag, VIGURO_DONE_RENDER_FLAG);
        assert_eq!(ctx.ctx_27a, 0);
    }

    /// PROT 0910's tick writes the summon seat and never HP.
    #[test]
    fn swordie_tick_stages_and_never_touches_hp() {
        let mut ctx = ctx_at(2);
        let mut s = seats(8);
        assert_eq!(swordie_tick(&mut ctx, &mut s, 7, 3, 1), CastTickStep::Busy);
        assert_eq!(s[7].staged_anim, SWORDIE_ARM2_CLIP);
        assert_eq!(s[7].restage, SWORDIE_ARM2_CLIP);
        assert!(s.iter().all(|a| a.hp == 400));
    }

    /// Arm 7 sets the animation rate to the frame delta, which is the strike
    /// slow-down the move is known for.
    #[test]
    fn swordie_arm_seven_writes_the_frame_delta_as_anim_rate() {
        let mut ctx = ctx_at(7);
        let mut s = seats(8);
        swordie_tick(&mut ctx, &mut s, 7, 3, 3);
        assert_eq!(s[7].anim_rate, 3);
    }

    /// The finding: the first three slashes cannot kill, the fourth can.
    #[test]
    fn swordie_slashes_switch_clamp_shape_on_the_last_hit() {
        for counter in 1..SWORDIE_LETHAL_HIT {
            let mut v = seats(1)[0];
            let applied = swordie_slash(&mut v, 0, counter, 10_000);
            assert_eq!(applied, 399, "hit {counter} clamps to HP - 1");
            assert_eq!(v.hp, 1, "hit {counter} cannot kill");
        }
        let mut v = seats(1)[0];
        let applied = swordie_slash(&mut v, SWORDIE_KNOCKDOWN_SLASH, SWORDIE_LETHAL_HIT, 10_000);
        assert_eq!(applied, 400);
        assert_eq!(v.hp, 0, "the fourth hit clamps to HP and kills");
        assert_eq!(v.staged_anim, 0x20, "and stages the knockdown clip");
        assert_eq!(v.restage & 0x01, 0x01);
        assert_eq!(v.restage & 0x04, 0, "the knockdown leg does not OR in 4");
    }

    /// A non-final slash takes the alternate clip even when the gate is set.
    #[test]
    fn swordie_non_final_slash_ignores_the_knockdown_gate() {
        let mut v = seats(1)[0];
        v.reaction_gate = 1;
        swordie_slash(&mut v, 1, 2, 10);
        assert_eq!(v.staged_anim, 0x21);
        assert_eq!(v.restage, 0x05);
    }

    /// A negative roll is a huge unsigned value, so shape C clamps it to the
    /// cap rather than healing.
    #[test]
    fn unsigned_floor_one_never_heals() {
        let mut v = seats(1)[0];
        let applied = apply_hit_unsigned_floor_one(&mut v, -50);
        assert_eq!(applied, 399);
        assert_eq!(v.hp, 1);
    }

    #[test]
    fn orb_heal_amount_is_level_times_sixty_four_plus_four_hundred_forty_eight() {
        assert_eq!(orb_heal_amount(0), 0x1C0);
        assert_eq!(orb_heal_amount(1), 0x200);
        assert_eq!(orb_heal_amount(5), 0x1C0 + 5 * 64);
        // Twice the band-wide formula `docs/formats/spell-table.md` records.
        assert_eq!(orb_heal_amount(5), 2 * ((5u32 << 5) + 0xE0));
    }

    #[test]
    fn orb_cleanse_masks_match_the_four_selector_arms() {
        assert_eq!(orb_cleanse_mask(0), None);
        assert_eq!(orb_cleanse_mask(1), Some(0xFFFC));
        assert_eq!(orb_cleanse_mask(2), Some(0xFF84));
        assert_eq!(orb_cleanse_mask(3), Some(0xFB84));
        assert_eq!(orb_cleanse_mask(4), Some(0xFB84));
        assert_eq!(orb_cleanse_mask(5), None);
        assert_eq!(
            orb_cleanse_mask(4).unwrap() & crate::cast_module_ticks::FLAG_KISS_OF_DEATH_MARK,
            0,
            "the top selector clears the Kiss of Death mark"
        );
    }

    /// Arm 5 jumps to phase 9 rather than advancing, which is what makes 6,
    /// 7 and 8 unreachable.
    #[test]
    fn orb_arm_five_jumps_to_phase_nine() {
        let mut ctx = ctx_at(5);
        let mut s = seats(8);
        let (step, healed) = orb_tick(&mut ctx, &mut s, 7, 0, None, |_| 400);
        assert_eq!(step, CastTickStep::Busy);
        assert_eq!(ctx.phase, ORB_ARM5_TARGET_PHASE);
        assert_eq!(ctx.ctx_278, ORB_ARM5_CTX_278);
        assert!(healed.is_empty());
    }

    /// The heal clamps to the missing HP, skips a dead seat, and writes the
    /// bar delta as the negative of the amount restored.
    #[test]
    fn orb_heals_the_whole_row_and_clamps_to_missing_hp() {
        let mut ctx = ctx_at(9);
        let mut s = seats(8);
        s[0].hp = 100;
        s[1].hp = 390;
        s[2].hp = 0;
        s[3].flags |= FLAG_NON_TARGETABLE;
        // Seat 0 has room for the whole amount, seat 1 only for ten points.
        let (_, healed) = orb_tick(&mut ctx, &mut s, 7, orb_heal_amount(0), None, |seat| {
            if seat == 0 { 2000 } else { 400 }
        });
        assert_eq!(s[0].hp, 100 + 0x1C0, "a full 448 fits under the cap");
        assert_eq!(s[0].hp_bar_delta, -0x1C0);
        assert_eq!(s[1].hp, 400, "clamped to maxHP - HP");
        assert_eq!(s[1].hp_bar_delta, -10);
        assert_eq!(s[2].hp, 0, "a dead seat is skipped");
        assert_eq!(s[3].hp, 400, "and so is an untargetable one");
        assert_eq!(s[4].hp, 400, "a seat already at maxHP gains nothing");
        assert_eq!(s[4].hp_bar_delta, 0, "and its bar delta is overwritten");
        let seats_healed: Vec<u8> = healed.iter().map(|h| h.seat).collect();
        assert_eq!(seats_healed, vec![0, 1, 4, 5, 6, 7]);
    }

    #[test]
    fn orb_cleanse_clears_the_status_bits_only_at_level_three() {
        let mut ctx = ctx_at(9);
        let mut s = seats(8);
        for a in s.iter_mut() {
            a.flags = 0x0403;
        }
        orb_tick(&mut ctx, &mut s, 7, 0, None, |_| 400);
        assert_eq!(s[0].flags, 0x0403, "no selector: nothing is cleared");

        let mut ctx = ctx_at(9);
        let mut s = seats(8);
        for a in s.iter_mut() {
            a.flags = 0x0403;
        }
        orb_tick(&mut ctx, &mut s, 7, 0, Some(4), |_| 400);
        assert_eq!(s[0].flags, 0, "selector 4 clears Venom, Toxic and the mark");
    }

    #[test]
    fn freed_sweep_uses_the_or_restage_form() {
        let mut ctx = ctx_at(FREED_DAMAGE_PHASE);
        let mut s = seats(8);
        let (_, hits) = freed_tick(&mut ctx, &mut s, 7, |_| 30);
        assert_eq!(hits.len(), 4, "seats 3..7");
        assert_eq!(s[3].hp, 370);
        assert_eq!(s[3].staged_anim, 0x21, "alive, gate clear: alternate clip");
        assert_eq!(s[3].restage, 0x05, "OR of 4 and 1, not an assignment");
        assert_eq!(s[3].anim_rate, FREED_HIT_ANIM_RATE);
        assert_eq!(s[0].hp, 400, "the party row is untouched");
    }

    #[test]
    fn freed_walks_twenty_arms_then_settles() {
        let mut ctx = ctx_at(0);
        let mut s = seats(8);
        let mut steps = 0;
        // Bounded: the chain names 0..=0x13 and then 0xFF.
        while steps < 64 {
            let (step, _) = freed_tick(&mut ctx, &mut s, 7, |_| 0);
            steps += 1;
            if step == CastTickStep::Done {
                break;
            }
        }
        assert_eq!(ctx.phase, SERU_B_DONE_PHASE);
        assert_eq!(
            steps, 0x15,
            "twenty arms, the settle arm, then the terminal"
        );
    }

    /// PROT 0913 rolls against a corpse: only `+0x16E & 4` guards the step.
    #[test]
    fn nova_has_no_dead_seat_guard() {
        let mut ctx = ctx_at(NOVA_DAMAGE_PHASE);
        let mut s = seats(8);
        s[3].hp = 0;
        let (_, hit) = nova_tick(&mut ctx, &mut s, 7, 3, 50);
        assert_eq!(hit.map(|h| h.applied), Some(0), "clamped to a zero HP bar");
        assert_eq!(s[3].staged_anim, 0x20, "and still staged the knockdown");

        let mut ctx = ctx_at(NOVA_DAMAGE_PHASE);
        let mut s = seats(8);
        s[3].flags |= FLAG_NON_TARGETABLE;
        let (_, hit) = nova_tick(&mut ctx, &mut s, 7, 3, 50);
        assert_eq!(hit, None, "the flag guard is the one that skips");
        assert_eq!(s[3].hp, 400);
    }

    #[test]
    fn nova_arm_nine_writes_the_same_two_from_one_register() {
        let mut ctx = ctx_at(9);
        let mut s = seats(8);
        nova_tick(&mut ctx, &mut s, 7, 3, 0);
        assert_eq!(s[7].staged_anim, NOVA_ARM9_CLIP);
        assert_eq!(ctx.ctx_278, NOVA_ARM9_CLIP);
        assert_eq!(s[7].restage, 1);
    }

    #[test]
    fn every_damage_shape_goes_through_the_summon_wrapper() {
        for entry in [909u32, 910, 912, 913] {
            let shape = seru_b_damage_shape(entry).expect("a shape per damage module");
            assert_eq!(shape.wrapper, CastWrapper::SharedSummon);
            assert!(!shape.never_kills);
            assert_eq!(shape.powers.len(), 1);
        }
        assert_eq!(seru_b_damage_shape(909).unwrap().powers[0], 0x12);
        assert_eq!(seru_b_damage_shape(912).unwrap().powers[0], 0x10);
        assert!(
            seru_b_damage_shape(911).is_none(),
            "PROT 0911 is a heal, not a damage module"
        );
    }
}
