//! The **code** half of the slot-B cast-module band (PROT 0903..0966): the
//! thirteen routines `docs/subsystems/cast-module.md`'s worklist grades
//! **PORT** because they read or write simulation state rather than only
//! handing spawn records to the pool spawner.
//!
//! The band's DATA half - the arm switches whose arms do nothing but call
//! `FUN_80021B04` / `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a
//! module-resident record pointer - is already the engine's
//! (`legaia_asset::cast_effect_pool`, staged by
//! `World::spawn_cast_module_fx`). What no record can express, and what this
//! module carries, is the state those routines touch:
//!
//! | retail field | what it is | mirror here |
//! |---|---|---|
//! | `+0x0C` | root-speed / magnitude word | [`CastActorState::root_speed`] |
//! | `+0x10` | pending HP-bar delta | [`CastActorState::hp_bar_delta`] |
//! | `+0x14C` | live HP | [`CastActorState::hp`] |
//! | `+0x16E` | flag bank (bit `0x4` = non-targetable) | [`CastActorState::flags`] |
//! | `+0x1D9` / `+0x1DA` / `+0x1DC` | playing clip / staged clip / restage bump | [`CastActorState::playing_anim`] / [`CastActorState::staged_anim`] / [`CastActorState::restage`] |
//! | `+0x1DD` | active-target byte | [`CastActorState::target_code`] |
//! | `+0x1F1` | the victim's own knockdown-reaction id | [`CastActorState::knockdown_anim`] |
//! | `+0x21C` / `+0x21D` | render flag / animation-rate scalar | [`CastActorState::render_flag`] / [`CastActorState::anim_rate`] |
//! | ctx `+0`, `+1` | actor count, monster count | [`CastModuleCtx::actor_count`] / [`CastModuleCtx::monster_count`] |
//! | ctx `+0x13` | caster seat | [`CastModuleCtx::caster_seat`] |
//! | ctx `+0x278` | module scratch byte | [`CastModuleCtx::ctx_278`] |
//! | ctx `+0x279` | the module phase | [`CastModuleCtx::phase`] |
//!
//! Every routine here is read off its **owning** image, not off whichever
//! dump prints at the VA: a band image ends in a byte-identical copy of a
//! sibling's tail, so seven of these addresses were catalogued against an
//! image that only holds the residue
//! (`docs/subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes`).
//!
//! ## What is ported, and what is not
//!
//! Ported per routine, byte-exactly: the dispatch bound (the `sltiu`
//! immediate, or the `beq`/`slti` chain's span), the arm map where the head
//! is a word table, the simulation-state writes, the damage step where the
//! routine has one - its **baked** power constant, the wrapper it calls, its
//! clamp shape, the `+0x10` accumulate, the HP write, the reaction stage and
//! the anim-rate write - and the phase advance.
//!
//! Not ported, and disclosed per item: the GPU-packet arms, the camera arms,
//! and, for the five tick bodies whose arm map is a `beq` chain or a
//! 256-entry table, the per-arm frame gating that decides *when* each step
//! fires. That is per-phase timing and it is pinned by capture, not by the
//! static window. A tick body is 451 to 2074 instructions and most of that is
//! packet emission.
//!
//! ## Two clamp shapes, not one
//!
//! `docs/subsystems/cast-module.md` describes a single apply shape for the
//! band ("clamp the roll against HP `+0x14C`, accumulate into `+0x10`, write
//! HP back"). The bytes carry **two**, and they differ in whether the hit can
//! kill:
//!
//! * [`apply_hit_floor_zero`] - `sltu hp, dmg` then `dmg = hp`: an
//!   **unsigned** clamp to HP, so the victim can be left at 0. PROT 0945,
//!   0957 (both tick bodies), 0958, 0960.
//! * [`apply_hit_floor_one`] - `addiu cap, hp, -1; slt cap, dmg` then
//!   `dmg = cap`: a **signed** clamp to `HP - 1`, so the hit can never kill
//!   and a *negative* roll heals. PROT 0927 (Juggernaut) and PROT 0966 (Evil
//!   Seru Magic) - the two band-wide AoE stagers.
//!
//! The unsigned comparison in the first shape is load-bearing: the wrappers
//! return `attacker_roll - defender_roll` as a signed word, and a negative
//! result reads as a huge unsigned value, so `sltu` fires and the clamp
//! rewrites the damage to the victim's whole HP. A negative roll on those
//! modules therefore **kills outright** rather than healing.
//!
//! Provenance: disassembly of each owning image at slot-B base
//! `0x801F69D8` (`scripts/ghidra-analysis/disasm-overlay-fn.py
//! extracted/overlays/overlay_<label>_<entry>.bin --base 0x801F69D8 --addr
//! <va>`), plus PROT 0898's three entry tables for the owner attribution
//! (`docs/subsystems/cast-module.md#the-entry-tables-and-where-the-addresses-live`).

use crate::battle_damage_wrappers::{
    WrapperAttacker, WrapperDefender, physical_wrapper_predamage, spell_wrapper_predamage,
    wrapper_net_damage,
};

/// Link base every band image is loaded at (`0x801F69D8`), and therefore the
/// base every VA in this module is printed under.
pub const CAST_MODULE_LINK_BASE: u32 = 0x801F_69D8;

/// The summon seat the stagers pose: `actor_table[7]`, reached in the bytes as
/// `lw rX, 0x1C(0x801C9370)`.
pub const SUMMON_SEAT: u8 = 7;

/// First monster seat - where the Juggernaut sweep starts
/// (`addiu s4, zero, 0xc; addu s2, s4, actor_table` = `&table[3]`).
pub const FIRST_MONSTER_SEAT: u8 = 3;

/// `+0x16E` bit `0x4` - "non-targetable". Both AoE stagers skip a victim
/// carrying it (`lhu v0,0x16e(v); andi v0,v0,4; bnez`).
pub const FLAG_NON_TARGETABLE: u16 = 0x0004;

/// The `+0x1DD` group code the Viguro stager writes onto the summon seat:
/// `9`, the enemy row (`addiu v0,zero,9; sb v0,0x1dd(a1)` at `0x801F7B98`).
pub const TARGET_CODE_ENEMY_ROW: u8 = 9;

/// The animation-rate scalar a seat runs at normally.
pub const ANIM_RATE_NORMAL: u8 = 8;

// ---------------------------------------------------------------------------
// State views
// ---------------------------------------------------------------------------

/// The slice of one battle actor's record a slot-B routine reads or writes.
///
/// Field names carry their retail offsets in the doc comments; a host bridges
/// this to its own actor record rather than the kernels reaching into a
/// global pool, so every routine here is directly testable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastActorState {
    /// `+0x0C` - the root-speed / magnitude word the stagers ramp.
    pub root_speed: i32,
    /// `+0x10` - pending HP-bar delta, accumulated by every apply site.
    pub hp_bar_delta: i32,
    /// `+0x14C` - live HP. Also the liveness flag: `0` means dead, and both
    /// AoE stagers skip such a seat.
    pub hp: u16,
    /// `+0x16E` - per-actor flag bank. Only [`FLAG_NON_TARGETABLE`] is read
    /// in this band.
    pub flags: u16,
    /// `+0x1D9` - the clip currently playing.
    pub playing_anim: u8,
    /// `+0x1DA` - the staged (queued) clip id.
    pub staged_anim: u8,
    /// `+0x1DC` - the restage counter, bumped with most `+0x1DA` writes.
    pub restage: u8,
    /// `+0x1DD` - active-target byte.
    pub target_code: u8,
    /// `+0x1F1` - this actor's own knockdown-reaction clip id, which the AoE
    /// stagers stage back onto it.
    pub knockdown_anim: u8,
    /// `+0x21C` - render flag (`0` visible, `0xFF` hidden by a summon fade).
    pub render_flag: u8,
    /// `+0x21D` - animation-rate scalar, normal [`ANIM_RATE_NORMAL`].
    pub anim_rate: u8,
}

/// The battle-context bytes a slot-B routine drives (`ctx` is
/// `*0x8007BD24`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastModuleCtx {
    /// `ctx+0` - actor count. The Evil Seru Magic loop's bound.
    pub actor_count: u8,
    /// `ctx+1` - monster count. The Juggernaut loop's bound.
    pub monster_count: u8,
    /// `ctx+0x13` - the caster's seat, passed to every wrapper as `a1`.
    pub caster_seat: u8,
    /// `ctx+0x278` - a module scratch byte; three of these routines write it.
    pub ctx_278: u8,
    /// `ctx+0x279` - the **module phase**, the second phase space riding
    /// under battle phase `0x70`.
    pub phase: u8,
}

/// What a tick reported to the drive loop. Retail's `0x801E4CA8` /
/// `0x801E50C8` sites hold battle phase `0x70` while the tick returns
/// non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastTickStep {
    /// The choreography is still running.
    Busy,
    /// The phase walked past the module's last arm, so the dispatch falls
    /// through to the epilogue and the routine writes nothing.
    Done,
}

// ---------------------------------------------------------------------------
// The idioms every routine in the band is built out of
// ---------------------------------------------------------------------------

/// The phase advance: `lbu v0, 0x279(ctx); addiu v0, v0, 1; sb v0, 0x279(ctx)`.
///
/// At most one taken arm per tick performs it, which is what makes the module
/// phase walk one step per frame. Wrapping is retail's: the byte is a `u8`
/// and the store is an `sb`.
pub fn advance_phase(ctx: &mut CastModuleCtx) {
    ctx.phase = ctx.phase.wrapping_add(1);
}

/// The paired stage/restage idiom: store the clip id to `+0x1DA` and bump the
/// restage counter `+0x1DC`.
///
/// Most `+0x1DA` writes in the band are paired this way, but not all - PROT
/// 0952's arm 3 writes `+0x1DA = 0` at `0x801F6EA4` with no `+0x1DC` bump
/// anywhere near it, so a plain assignment is a distinct operation and is
/// spelled out where it occurs.
pub fn stage_clip(actor: &mut CastActorState, clip: u8) {
    actor.staged_anim = clip;
    actor.restage = actor.restage.wrapping_add(1);
}

/// Apply shape **A** - the kill-capable clamp (PROT 0945 / 0957 / 0958 /
/// 0960).
///
/// ```text
/// a0 = victim[+0x14C]
/// sltu v0, a0, dmg        ; UNSIGNED
/// if v0 { dmg = a0 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// `roll` is the wrapper's raw signed return, and the comparison is unsigned,
/// so a negative roll compares greater than any HP and the clamp rewrites it
/// to the victim's whole HP - the victim dies. That is the retail behaviour,
/// not a port artefact; it is why this is kept apart from
/// [`apply_hit_floor_one`].
///
/// Returns the damage actually applied.
pub fn apply_hit_floor_zero(victim: &mut CastActorState, roll: i32) -> u32 {
    let hp = u32::from(victim.hp);
    let mut dmg = roll as u32;
    if hp < dmg {
        dmg = hp;
    }
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = u32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// Apply shape **B** - the never-kill clamp (PROT 0927 / 0966).
///
/// ```text
/// v0 = victim[+0x14C]
/// v1 = v0 - 1
/// slt v0, v1, dmg         ; SIGNED
/// if v0 { dmg = v1 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// The comparison is signed, so a negative roll passes through unclamped and
/// the subtract *raises* HP. The cap is `HP - 1`, so a live victim is left at
/// 1 HP at worst - neither of the two band-wide AoE stagers can kill.
///
/// Returns the (signed) amount applied.
pub fn apply_hit_floor_one(victim: &mut CastActorState, roll: i32) -> i32 {
    let cap = i32::from(victim.hp).wrapping_sub(1);
    let dmg = if cap < roll { cap } else { roll };
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg);
    victim.hp = i32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// Should an AoE stager's loop body run for this seat? Both `FUN_801F85A8`
/// and `FUN_801F8D64` open with the same two guards: skip a dead seat
/// (`+0x14C == 0`) and skip a non-targetable one (`+0x16E & 4`).
pub fn aoe_seat_is_hittable(actor: &CastActorState) -> bool {
    actor.hp != 0 && (actor.flags & FLAG_NON_TARGETABLE) == 0
}

// ---------------------------------------------------------------------------
// Baked per-hit power constants
// ---------------------------------------------------------------------------

/// Which SCUS wrapper a module's damage call goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastWrapper {
    /// `FUN_801DD0AC` with `a1 = 7` - the shared kernel's **summon** branch.
    /// Only PROT 0927 in this band.
    SharedSummon,
    /// `FUN_801DD4B0` - the resist ladder runs.
    Respect,
    /// `FUN_801DD6B4` - the party-defender resist block is skipped.
    Bypass,
}

/// One module's damage shape: its wrapper, its clamp, and the `a0` constants
/// its call sites bake in.
#[derive(Debug, Clone, Copy)]
pub struct CastDamageShape {
    /// Extraction PROT entry of the owning module.
    pub prot_entry: u32,
    /// Retail VA of the routine the sites live in.
    pub routine: u32,
    /// The wrapper every one of its sites calls.
    pub wrapper: CastWrapper,
    /// `true` when the module uses the `HP - 1` signed clamp
    /// ([`apply_hit_floor_one`]).
    pub never_kills: bool,
    /// The baked `a0` power constants, in call-site order.
    pub powers: &'static [u16],
}

/// PROT 0958's six baked per-hit powers, in call-site order
/// (`0x801F74EC`, `0x801F7854`, `0x801F7CA4`, `0x801F80E4`, `0x801F8754`,
/// `0x801F88D8`).
///
/// `docs/subsystems/cast-module.md` quotes the run as
/// "`0x30, 0x38, 0x38, 0x38, 0x40, ..`"; the sixth site is `0x30`, so the
/// escalation does **not** continue.
pub const BLAZING_SLASH_POWERS: [u16; 6] = [0x30, 0x38, 0x38, 0x38, 0x40, 0x30];

/// Every damage shape the band's PORT rows carry, read off the `a0` set
/// before each `jal` into `0x801DD0AC` / `0x801DD4B0` / `0x801DD6B4`.
pub const CAST_DAMAGE_SHAPES: [CastDamageShape; 6] = [
    CastDamageShape {
        prot_entry: 927,
        routine: 0x801F_85A8,
        wrapper: CastWrapper::SharedSummon,
        never_kills: true,
        powers: &[0x12],
    },
    CastDamageShape {
        prot_entry: 945,
        routine: 0x801F_6EDC,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[0x30],
    },
    CastDamageShape {
        prot_entry: 957,
        routine: 0x801F_6A14,
        wrapper: CastWrapper::Respect,
        never_kills: false,
        powers: &[0x100],
    },
    CastDamageShape {
        prot_entry: 958,
        routine: 0x801F_6DD8,
        wrapper: CastWrapper::Bypass,
        never_kills: false,
        powers: &BLAZING_SLASH_POWERS,
    },
    CastDamageShape {
        prot_entry: 960,
        routine: 0x801F_74E4,
        wrapper: CastWrapper::Bypass,
        never_kills: false,
        powers: &[0x1C0],
    },
    CastDamageShape {
        prot_entry: 966,
        routine: 0x801F_8D64,
        wrapper: CastWrapper::Respect,
        never_kills: true,
        powers: &[0x100],
    },
];

/// The damage shape of the module PROT `prot_entry` pages, if it has one.
///
/// This is the figure a capture cast's damage is *actually* built from in
/// retail: the module bakes `a0` into the call site, so the move-power
/// table's scalar is not what the wrapper sees.
pub fn damage_shape_for(prot_entry: u32) -> Option<&'static CastDamageShape> {
    CAST_DAMAGE_SHAPES
        .iter()
        .find(|s| s.prot_entry == prot_entry)
}

/// The module's first baked power - the seed a single-hit cast uses.
pub fn baked_power_for(prot_entry: u32) -> Option<u16> {
    damage_shape_for(prot_entry).and_then(|s| s.powers.first().copied())
}

/// Roll one hit through the wrapper the module's shape names, seeded with the
/// module's own baked power rather than the move-power table's scalar.
///
/// Returns the wrapper's raw signed net damage; feed it to
/// [`apply_hit_floor_zero`] or [`apply_hit_floor_one`] according to
/// [`CastDamageShape::never_kills`].
pub fn roll_module_hit(
    shape: &CastDamageShape,
    site: usize,
    attacker: &WrapperAttacker,
    defender: &WrapperDefender,
    element_affinity_pct: u8,
    rng: [u16; 3],
    bonus_rng: impl FnOnce() -> u16,
) -> i32 {
    let power = u32::from(shape.powers.get(site).copied().unwrap_or(0));
    let (atk, def) = match shape.wrapper {
        CastWrapper::Bypass => spell_wrapper_predamage(
            power,
            attacker,
            defender,
            element_affinity_pct,
            [rng[0], rng[1]],
            bonus_rng,
        ),
        // The respecting wrapper `FUN_801DD4B0`. PROT 0927's shared-kernel
        // summon branch shares its roll shape and differs only in the bonus
        // arm, which `battle_formulas` owns; routing it here keeps the baked
        // power and the clamp right even where the bonus arm is the other
        // kernel's.
        CastWrapper::Respect | CastWrapper::SharedSummon => physical_wrapper_predamage(
            power,
            attacker,
            defender,
            element_affinity_pct,
            rng,
            bonus_rng,
        ),
    };
    wrapper_net_damage(atk, def)
}

// ---------------------------------------------------------------------------
// The seven state-touching spawn stagers (`0x801F6734` row, move-VM op 0x20)
// ---------------------------------------------------------------------------

/// PROT 0949 (Water Crystals) spawn stager - the freeze ramp.
///
/// Eight arms behind `sltiu a1, 8` through the table at `0x801F69F0`, and
/// every one is two stores on the **victim** (`actor_table[caster[+0x1DD]]`,
/// derived in the prologue at `0x801F75BC..0x801F75F4`). The routine holds no
/// `jal` at all:
///
/// ```text
/// arm n:  sw (n + 1) * 0x200, 0x0C(victim)
///         sb 7 - n,           0x21D(victim)
/// ```
///
/// so `+0x0C` climbs `0x200 .. 0x1000` while the animation rate falls `7 .. 0`:
/// the victim freezes as the magnitude peaks. An `a1` of 8 or more falls
/// through to the epilogue and writes nothing.
///
/// `docs/subsystems/cast-module.md` grades this **PORT** for the `+0x0C`
/// write and does not mention `+0x21D`; both stores are here.
///
/// Wired: `World::run_cast_module_stager`, at the cast band's staging seam.
///
/// PORT: FUN_801F75BC
pub fn water_crystals_stager(victim: &mut CastActorState, arm: u8) {
    if arm >= 8 {
        return;
    }
    victim.root_speed = (i32::from(arm) + 1) * 0x200;
    victim.anim_rate = 7 - arm;
}

/// PROT 0922 (Puera) spawn stager - eight instructions and one byte.
///
/// `bnez a1` skips the whole body, so only arm `0` does anything, and what it
/// does is `ctx[+0x278] = 3`. The frame (`addiu sp,sp,-8`) opens in the
/// branch's delay slot, which is why a prologue scan puts the entry four
/// bytes late - the routine really starts at `0x801F90E4`.
///
/// Wired: `World::run_cast_module_stager`.
///
/// PORT: FUN_801F90E4
pub fn puera_stager(ctx: &mut CastModuleCtx, arm: u8) {
    if arm != 0 {
        return;
    }
    ctx.ctx_278 = 3;
}

/// PROT 0923 (Gilium) spawn stager.
///
/// Three arms reached by a `beq`/`slti` chain rather than a table:
///
/// * arm `0` - `FUN_80021B04(actor+0x14, actor+0x24, 0x801F9450, 0x1000)`,
///   stash the handle at `0x801FA4D0`, then `ctx[+0x278] = 3`;
/// * arm `1` - OR bit `8` into the `+0x10` word of three live handles and
///   allocate a screen prim through `FUN_80024E80`;
/// * arm `2` - `FUN_80021B04(actor+0x14, actor+0x24, 0x801FA414, actor[+0x72])`.
///
/// The spawn calls are the DATA layer the pool already stages; the only
/// simulation write in the routine is arm 0's `ctx[+0x278]`.
///
/// Wired: `World::run_cast_module_stager`.
///
/// PORT: FUN_801F8B90 (state half; the three spawn sites are
/// `legaia_asset::cast_effect_pool`'s records)
pub fn gilium_stager(ctx: &mut CastModuleCtx, arm: u8) {
    if arm == 0 {
        ctx.ctx_278 = 3;
    }
}

/// PROT 0906 (Gizam) spawn stager.
///
/// Seven arms behind `sltiu a1, 7` through the head table at `0x801F69D8`
/// (arm targets `0x801F7788`, `780C`, `783C`, `785C`, `7920`, `7964`,
/// `79A8`). Two arms carry state:
///
/// * arm `1` (`0x801F780C..0x801F783C`) poses the summon seat
///   `actor_table[7]`: `+0x21C = 0` (visible), `+0x04 = 0x3FF00000` (the
///   render blend), `+0x0C = 0x1000`;
/// * arm `2` (`0x801F783C..0x801F785C`) is nothing but the phase advance.
///
/// The rest are `FUN_80024E80` prim fills, two `FUN_80056798` rand draws and
/// one `FUN_80021B04` spawn - the DATA layer.
///
/// Wired: `World::run_cast_module_stager`.
///
/// PORT: FUN_801F7740 (state half)
pub fn gizam_stager(ctx: &mut CastModuleCtx, summon_seat: &mut CastActorState, arm: u8) {
    match arm {
        1 => {
            summon_seat.render_flag = 0;
            summon_seat.root_speed = 0x1000;
        }
        2 => advance_phase(ctx),
        _ => {}
    }
}

/// PROT 0909 (Viguro) spawn stager.
///
/// Seven arms behind `sltiu a1, 7` through the head table at `0x801F69D8`
/// (arm targets `0x801F7B2C`, `7BCC`, `7C64`, `7C78`, `7C8C`, `7CB8`,
/// `7CA0`; arm 5 is the epilogue itself, i.e. a no-op). Arm `0` is the seat
/// pose, and it is the one that touches state:
///
/// ```text
/// jal FUN_801F19EC                        ; module init
/// summon = actor_table[7]
/// summon[+0x21C] = 0                      ; visible
/// summon[+0x0C]  = 0x1000
/// saved          = summon[+0x1DD]         ; stashed at 0x8019835C
/// summon[+0x1DD] = 9                      ; the enemy-row group code
/// ctx[+0x279]   += 1
/// ```
///
/// The `+0x34` / `+0x38` / `+0x46` / `+0x04` / `+0x21F` stores in the same arm
/// are pose and render fields, left to the host's own seat placement.
///
/// Returns the `+0x1DD` value the arm displaced, which retail stashes for a
/// later arm to restore.
///
/// Wired: `World::run_cast_module_stager`.
///
/// PORT: FUN_801F7AF4 (state half)
pub fn viguro_stager(
    ctx: &mut CastModuleCtx,
    summon_seat: &mut CastActorState,
    arm: u8,
) -> Option<u8> {
    if arm != 0 {
        return None;
    }
    let saved = summon_seat.target_code;
    summon_seat.render_flag = 0;
    summon_seat.root_speed = 0x1000;
    summon_seat.target_code = TARGET_CODE_ENEMY_ROW;
    advance_phase(ctx);
    Some(saved)
}

/// One seat's outcome inside an AoE stager's loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AoeHit {
    /// Absolute actor slot the hit landed on.
    pub seat: u8,
    /// Amount applied after the module's clamp (signed: shape B can heal).
    pub applied: i32,
}

/// PROT 0927 (Juggernaut) spawn stager - the enemy-row sweep.
///
/// Nine arms behind `sltiu a1, 9` through the table at `0x801F6A60`. The
/// working arm walks the actor table from `+0xC` (seat
/// [`FIRST_MONSTER_SEAT`]) for `ctx[+1]` seats:
///
/// ```text
/// for i in 0 .. ctx[+1]:
///     victim = actor_table[3 + i]
///     if victim[+0x14C] == 0    { continue }        ; dead
///     if victim[+0x16E] & 4     { continue }        ; non-targetable
///     dmg = FUN_801DD0AC(0x12, 7, 3 + i)            ; SHARED kernel, summon branch
///     cap = victim[+0x14C] - 1
///     if (i32)cap < (i32)dmg { dmg = cap }          ; never kills
///     victim[+0x10]  += dmg
///     victim[+0x14C] -= dmg
/// ```
///
/// The `a1 = 7` is the shared kernel's summon-branch selector, so this module
/// is the band's one user of `FUN_801DD0AC` - and it is a *summon* module
/// (PROT 0903..0934), which is why it does not contradict
/// `docs/subsystems/battle-formulas.md`'s "a capture-class cast never reaches
/// `FUN_801DD0AC` at all". Unlike Evil Seru Magic it stages no reaction clip
/// and leaves the animation rate alone.
///
/// `roll` supplies one wrapper result per hittable seat, in seat order; the
/// caller owns the RNG cursor so a host can keep retail's draw order.
///
/// Wired: `World::run_cast_module_aoe`.
///
/// PORT: FUN_801F85A8 (state half; the seven spawn sites are the pool's)
pub fn juggernaut_stager(
    ctx: &CastModuleCtx,
    seats: &mut [CastActorState],
    arm: u8,
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<AoeHit> {
    let mut hits = Vec::new();
    if arm >= 9 {
        return hits;
    }
    for i in 0..ctx.monster_count {
        let seat = FIRST_MONSTER_SEAT.saturating_add(i);
        let Some(victim) = seats.get_mut(seat as usize) else {
            break;
        };
        if !aoe_seat_is_hittable(victim) {
            continue;
        }
        let applied = apply_hit_floor_one(victim, roll(seat));
        hits.push(AoeHit { seat, applied });
    }
    hits
}

/// PROT 0966 (Evil Seru Magic) spawn stager - the whole-table sweep.
///
/// Nine arms behind `sltiu a1, 9` through the table at `0x801F6A50`. Its
/// working arm (arm `4`, `0x801F8E4C`) walks the actor table from seat `0`
/// for `ctx[+0]` seats, and unlike Juggernaut it also stages the reaction:
///
/// ```text
/// for i in 0 .. ctx[+0]:
///     victim = actor_table[i]
///     if victim[+0x14C] == 0 || victim[+0x16E] & 4 { continue }
///     dmg = FUN_801DD4B0(0x100, ctx[+0x13], i)      ; the RESPECTING wrapper
///     cap = victim[+0x14C] - 1
///     if (i32)cap < (i32)dmg { dmg = cap }          ; never kills
///     victim[+0x10]  += dmg
///     victim[+0x14C] -= dmg
///     victim[+0x1DA]  = victim[+0x1F1]              ; its own knockdown clip
///     victim[+0x1DC] += 1
///     victim[+0x21D]  = 2                           ; slow-motion
/// ```
///
/// So Cort's ESM hits **every seat in the table**, party and monsters alike,
/// and leaves each at 1 HP at worst.
///
/// Wired: `World::run_cast_module_aoe`.
///
/// PORT: FUN_801F8D64 (state half; the seven spawn sites are the pool's)
pub fn evil_seru_magic_stager(
    ctx: &CastModuleCtx,
    seats: &mut [CastActorState],
    arm: u8,
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<AoeHit> {
    let mut hits = Vec::new();
    if arm >= 9 {
        return hits;
    }
    for seat in 0..ctx.actor_count {
        let Some(victim) = seats.get_mut(seat as usize) else {
            break;
        };
        if !aoe_seat_is_hittable(victim) {
            continue;
        }
        let applied = apply_hit_floor_one(victim, roll(seat));
        let knockdown = victim.knockdown_anim;
        stage_clip(victim, knockdown);
        victim.anim_rate = 2;
        hits.push(AoeHit { seat, applied });
    }
    hits
}

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
fn run_tick(
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
/// Wired: `World::run_cast_module_tick`.
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
/// Wired: `World::run_cast_module_tick`.
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
/// No damage-wrapper call: its two shape-A clamp sites at `0x801F9734` /
/// `0x801F9780` cap a value computed in line (`subu v1, a0, v0; sltu v0, v1,
/// a3`) rather than a wrapper return, so the HP writes here are a drain, not
/// a roll. Seven phase stores through `$fp = ctx+0x279`, seven `+0x1DA`
/// stages, five restage bumps.
///
/// Ported: the phase walk and the staging discipline. Not ported: the drain's
/// per-arm rate, which is a frame-gated packet arm.
///
/// Wired: `World::run_cast_module_tick`.
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
/// Wired: `World::run_cast_module_tick`.
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
/// Wired: `World::run_cast_module_tick`.
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

/// PROT 0960 (Plasma Strike, Lu Delilas) tick body.
///
/// A `beq`/`slti` chain head (widest compare `slti v1, 9`), one
/// `FUN_801DD6B4` site at `0x801F8168` with the baked `0x1C0` burst, the
/// shape-A clamp at `0x801F818C`, six `+0x1DA` stages, five restage bumps and
/// three phase stores through `$s4 = ctx+0x279`.
///
/// Its phase-5 arm is the documented paired stage/confirm gate: it stages id
/// `0x0D` every tick and holds until `caster[+0x1D9]` equals the same
/// literal, ANDed with a progress check - so that arm does **not** advance
/// the phase until the confirm passes. An edit that remaps the stage without
/// the compare stalls phase 5 forever, which is the softlock the module docs
/// record.
///
/// Wired: `World::run_cast_module_tick`.
///
/// PORT: FUN_801F74E4 (phase machine + damage/staging + the phase-5 confirm
/// gate; packet + camera arms unported)
pub fn plasma_strike_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 9, |c| {
        if c.phase == PLASMA_STRIKE_CONFIRM_PHASE {
            stage_clip(caster, PLASMA_STRIKE_CONFIRM_CLIP);
            // Hold the phase until the commit mirrors the id into `+0x1D9`.
            return caster.playing_anim != PLASMA_STRIKE_CONFIRM_CLIP;
        }
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(hp: u16) -> CastActorState {
        CastActorState {
            hp,
            anim_rate: ANIM_RATE_NORMAL,
            knockdown_anim: 0x11,
            ..Default::default()
        }
    }

    // --- the two clamp shapes -------------------------------------------

    #[test]
    fn shape_a_clamps_to_hp_and_floors_at_zero() {
        let mut v = actor(100);
        assert_eq!(apply_hit_floor_zero(&mut v, 40), 40);
        assert_eq!(v.hp, 60);
        assert_eq!(v.hp_bar_delta, 40);
        // Over-damage is clamped to the remaining HP, so HP lands exactly 0.
        assert_eq!(apply_hit_floor_zero(&mut v, 1000), 60);
        assert_eq!(v.hp, 0);
        assert_eq!(v.hp_bar_delta, 100);
    }

    #[test]
    fn shape_a_treats_a_negative_roll_as_a_kill() {
        // `sltu` is unsigned, so a negative wrapper return compares above any
        // HP and the clamp rewrites it to the whole bar. This is retail.
        let mut v = actor(250);
        assert_eq!(apply_hit_floor_zero(&mut v, -5), 250);
        assert_eq!(v.hp, 0);
    }

    #[test]
    fn shape_b_never_kills() {
        let mut v = actor(100);
        assert_eq!(apply_hit_floor_one(&mut v, 1000), 99);
        assert_eq!(v.hp, 1, "the HP-1 cap leaves the victim alive");
        // ... and at 1 HP the cap is 0, so a further hit does nothing.
        assert_eq!(apply_hit_floor_one(&mut v, 1000), 0);
        assert_eq!(v.hp, 1);
    }

    #[test]
    fn shape_b_heals_on_a_negative_roll() {
        // `slt` is signed, so a negative roll passes the cap unclamped and
        // the subtract raises HP - the opposite of shape A.
        let mut v = actor(100);
        assert_eq!(apply_hit_floor_one(&mut v, -20), -20);
        assert_eq!(v.hp, 120);
    }

    // --- the phase discipline -------------------------------------------

    #[test]
    fn a_tick_advances_the_phase_exactly_once_and_parks_on_the_terminal_arm() {
        let mut ctx = CastModuleCtx::default();
        let mut caster = actor(100);
        let mut victim = actor(500);
        for expect in 1..=ASTRAL_SLASH_TERMINAL_ARM {
            assert_eq!(
                astral_slash_tick(&mut ctx, &mut caster, &mut victim),
                CastTickStep::Busy
            );
            assert_eq!(ctx.phase, expect);
        }
        // Arm 4 holds: the tick still reports Busy, but the phase stays.
        for _ in 0..3 {
            assert_eq!(
                astral_slash_tick(&mut ctx, &mut caster, &mut victim),
                CastTickStep::Busy
            );
            assert_eq!(ctx.phase, ASTRAL_SLASH_TERMINAL_ARM);
        }
    }

    #[test]
    fn a_phase_past_the_bound_falls_through_and_writes_nothing() {
        let mut ctx = CastModuleCtx {
            phase: 5,
            ..Default::default()
        };
        let mut caster = actor(100);
        let mut victim = actor(500);
        let before = (caster, victim, ctx);
        assert_eq!(
            astral_slash_tick(&mut ctx, &mut caster, &mut victim),
            CastTickStep::Done
        );
        assert_eq!((caster, victim, ctx), before);
    }

    #[test]
    fn astral_slash_drops_and_restores_the_animation_rate() {
        let mut ctx = CastModuleCtx {
            phase: 2,
            ..Default::default()
        };
        let mut caster = actor(100);
        let mut victim = actor(500);
        astral_slash_tick(&mut ctx, &mut caster, &mut victim);
        assert_eq!(caster.staged_anim, ASTRAL_SLASH_ARM2_CLIP);
        assert_eq!(caster.restage, 1);
        assert_eq!((caster.anim_rate, victim.anim_rate), (1, 1));
        // Arm 3 restores the caster and leaves the victim slowed.
        astral_slash_tick(&mut ctx, &mut caster, &mut victim);
        assert_eq!(caster.staged_anim, 0);
        assert_eq!(caster.restage, 1, "arm 3's `+0x1DA` store is unpaired");
        assert_eq!((caster.anim_rate, victim.anim_rate), (ANIM_RATE_NORMAL, 2));
    }

    #[test]
    fn the_confirm_gate_holds_the_phase_until_the_clip_plays() {
        let mut ctx = CastModuleCtx {
            phase: PLASMA_STRIKE_CONFIRM_PHASE,
            ..Default::default()
        };
        let mut caster = actor(100);
        let mut victim = actor(500);
        for _ in 0..4 {
            assert_eq!(
                plasma_strike_tick(&mut ctx, &mut caster, &mut victim, None),
                CastTickStep::Busy
            );
            assert_eq!(ctx.phase, PLASMA_STRIKE_CONFIRM_PHASE);
            assert_eq!(caster.staged_anim, PLASMA_STRIKE_CONFIRM_CLIP);
        }
        // Once the commit mirrors the id into `+0x1D9` the gate passes.
        caster.playing_anim = PLASMA_STRIKE_CONFIRM_CLIP;
        assert_eq!(
            plasma_strike_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Busy
        );
        assert_eq!(ctx.phase, PLASMA_STRIKE_CONFIRM_PHASE + 1);
    }

    #[test]
    fn a_bypass_hit_lands_through_the_tick_and_clamps_at_zero() {
        let mut ctx = CastModuleCtx::default();
        let mut victim = actor(200);
        blazing_slash_tick(&mut ctx, &mut victim, Some((0, 90)));
        assert_eq!(victim.hp, 110);
        assert_eq!(victim.staged_anim, 0x11, "the victim's own +0x1F1 reaction");
        // A site index past the six baked powers stages nothing.
        blazing_slash_tick(&mut ctx, &mut victim, Some((9, 90)));
        assert_eq!(victim.hp, 110);
    }

    // --- the seven stagers ----------------------------------------------

    #[test]
    fn the_water_crystals_ramp_pairs_speed_with_rate() {
        for arm in 0..8u8 {
            let mut v = actor(100);
            water_crystals_stager(&mut v, arm);
            assert_eq!(v.root_speed, (i32::from(arm) + 1) * 0x200);
            assert_eq!(v.anim_rate, 7 - arm);
        }
        // Arm 8 is past `sltiu a1, 8` and writes nothing.
        let mut v = actor(100);
        let before = v;
        water_crystals_stager(&mut v, 8);
        assert_eq!(v, before);
    }

    #[test]
    fn the_puera_stager_is_arm_zero_only() {
        let mut ctx = CastModuleCtx::default();
        puera_stager(&mut ctx, 1);
        assert_eq!(ctx.ctx_278, 0, "`bnez a1` skips the whole body");
        puera_stager(&mut ctx, 0);
        assert_eq!(ctx.ctx_278, 3);
    }

    #[test]
    fn the_gilium_stager_writes_ctx_278_on_arm_zero() {
        let mut ctx = CastModuleCtx::default();
        gilium_stager(&mut ctx, 2);
        assert_eq!(ctx.ctx_278, 0);
        gilium_stager(&mut ctx, 0);
        assert_eq!(ctx.ctx_278, 3);
    }

    #[test]
    fn the_gizam_stager_splits_the_pose_and_the_advance() {
        let mut ctx = CastModuleCtx::default();
        let mut seat = CastActorState {
            render_flag: 0xFF,
            ..actor(1)
        };
        gizam_stager(&mut ctx, &mut seat, 1);
        assert_eq!((seat.render_flag, seat.root_speed), (0, 0x1000));
        assert_eq!(ctx.phase, 0, "arm 1 poses only");
        gizam_stager(&mut ctx, &mut seat, 2);
        assert_eq!(ctx.phase, 1, "arm 2 is the advance");
    }

    #[test]
    fn the_viguro_stager_retargets_the_summon_seat() {
        let mut ctx = CastModuleCtx::default();
        let mut seat = CastActorState {
            target_code: 2,
            render_flag: 0xFF,
            ..actor(1)
        };
        assert_eq!(viguro_stager(&mut ctx, &mut seat, 0), Some(2));
        assert_eq!(seat.target_code, TARGET_CODE_ENEMY_ROW);
        assert_eq!(seat.render_flag, 0);
        assert_eq!(seat.root_speed, 0x1000);
        assert_eq!(ctx.phase, 1);
        // Any other arm leaves the seat alone.
        assert_eq!(viguro_stager(&mut ctx, &mut seat, 3), None);
    }

    #[test]
    fn the_esm_sweep_covers_the_whole_table_and_leaves_everyone_alive() {
        let ctx = CastModuleCtx {
            actor_count: 5,
            caster_seat: 3,
            ..Default::default()
        };
        let mut seats = vec![actor(100), actor(100), actor(0), actor(100), actor(100)];
        // Seat 3 is non-targetable, seat 2 is dead: both are skipped.
        seats[3].flags = FLAG_NON_TARGETABLE;
        let hits = evil_seru_magic_stager(&ctx, &mut seats, 4, |_| 9999);
        assert_eq!(
            hits.iter().map(|h| h.seat).collect::<Vec<_>>(),
            vec![0, 1, 4]
        );
        for seat in [0usize, 1, 4] {
            assert_eq!(seats[seat].hp, 1, "the HP-1 cap");
            assert_eq!(seats[seat].staged_anim, 0x11, "its own +0x1F1 reaction");
            assert_eq!(seats[seat].restage, 1);
            assert_eq!(seats[seat].anim_rate, 2);
        }
        assert_eq!(seats[2].hp, 0, "a dead seat is untouched");
        assert_eq!(seats[3].hp, 100, "a non-targetable seat is untouched");
        // Arm 9 is past `sltiu a1, 9`.
        let mut fresh = vec![actor(100); 5];
        assert!(evil_seru_magic_stager(&ctx, &mut fresh, 9, |_| 50).is_empty());
        assert_eq!(fresh[0].hp, 100);
    }

    #[test]
    fn the_juggernaut_sweep_starts_at_seat_three() {
        let ctx = CastModuleCtx {
            monster_count: 2,
            ..Default::default()
        };
        let mut seats = vec![actor(100); 6];
        let hits = juggernaut_stager(&ctx, &mut seats, 0, |_| 50);
        assert_eq!(hits.iter().map(|h| h.seat).collect::<Vec<_>>(), vec![3, 4]);
        for seat in seats.iter().take(3) {
            assert_eq!(seat.hp, 100, "the party row is not swept");
        }
        assert_eq!(seats[3].hp, 50);
        assert_eq!(seats[4].hp, 50);
        assert_eq!(seats[5].hp, 100, "bounded by ctx[+1]");
        // Unlike ESM, Juggernaut stages no reaction clip.
        assert_eq!(seats[3].staged_anim, 0);
        assert_eq!(seats[3].anim_rate, ANIM_RATE_NORMAL);
    }

    // --- the tables ------------------------------------------------------

    #[test]
    fn every_damage_shape_names_a_band_entry_and_at_least_one_power() {
        for s in CAST_DAMAGE_SHAPES {
            assert!((903..=966).contains(&s.prot_entry), "{s:?}");
            assert!(!s.powers.is_empty(), "{s:?}");
            assert!(s.routine >= CAST_MODULE_LINK_BASE, "{s:?}");
        }
        assert_eq!(baked_power_for(960), Some(0x1C0), "Plasma Strike's burst");
        assert_eq!(baked_power_for(958), Some(0x30));
        assert_eq!(damage_shape_for(958).unwrap().powers, &BLAZING_SLASH_POWERS);
        assert_eq!(baked_power_for(903), None, "not a PORT row");
        // Only the two AoE stagers use the never-kill clamp.
        let never: Vec<u32> = CAST_DAMAGE_SHAPES
            .iter()
            .filter(|s| s.never_kills)
            .map(|s| s.prot_entry)
            .collect();
        assert_eq!(never, vec![927, 966]);
    }

    #[test]
    fn every_tick_shape_names_a_band_entry() {
        for s in CAST_TICK_SHAPES {
            assert!((903..=966).contains(&s.prot_entry), "{s:?}");
            assert!(s.phase_arms > 0, "{s:?}");
        }
        assert_eq!(tick_shape_for(0x801F_6DD8).unwrap().phase_arms, 0x100);
        assert!(tick_shape_for(0x801F_6DD8).unwrap().table_head);
        assert!(!tick_shape_for(0x801F_74E4).unwrap().table_head);
        assert!(tick_shape_for(0x801F_9999).is_none());
    }
}
