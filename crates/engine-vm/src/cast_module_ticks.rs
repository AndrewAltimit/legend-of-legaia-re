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
//! Shape B is a property of those two **stagers**, not of their images. Both
//! images also hold a tick body whose own damage site takes shape A: PROT
//! 0927's at `0x801F7E0C` / clamp `0x801F7E38` (same baked `0x12`), and PROT
//! 0966's at `0x801F8610` / clamp `0x801F863C` with a **different** baked
//! power, `0x327` rather than the stager's `0x100`. The band has exactly two
//! shape-B sites - `0x801F8758` (0927) and `0x801F8F08` (0966) - and every
//! other damage-wrapper call in PROT 0903..0966 clamps with `sltu`. So an
//! entry-keyed lookup ([`damage_shape_for`]) answers for the module's
//! **stager**; a tick's magnitude and kill-capability must come from the tick.
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
    /// `+0x158` / `+0x15A` - ATK working / base. Raised as a pair by PROT
    /// 0955's Power Charge and lowered as a pair by its Melt Spray.
    pub atk: u16,
    /// See [`CastActorState::atk`].
    pub atk_base: u16,
    /// `+0x15C` / `+0x15E` - UDF (upper defence) working / base.
    pub udf: u16,
    /// See [`CastActorState::udf`].
    pub udf_base: u16,
    /// `+0x160` / `+0x162` - LDF (lower defence) working / base.
    pub ldf: u16,
    /// See [`CastActorState::ldf`].
    pub ldf_base: u16,
    /// `+0x164` / `+0x166` - SPD working / base.
    pub spd: u16,
    /// See [`CastActorState::spd`].
    pub spd_base: u16,
    /// `+0x168` / `+0x16A` - INT working / base.
    pub intel: u16,
    /// See [`CastActorState::intel`].
    pub intel_base: u16,
    /// `+0x16C` - the per-round **initiative key**, doubling as "has not
    /// acted yet". The turn-steal idiom clears it.
    pub init_key: u16,
    /// `+0x1DE` - action category. `1` is Item, which is what the refund
    /// arm gates on.
    pub action_category: u8,
    /// `+0x1DF` - the queued action byte. For an Item action it is the item
    /// id the refund hands back.
    pub queued_action: u8,
    /// `+0x1EF` / `+0x1F0` - the two alternative reaction clips the band
    /// stages when `+0x1F2` is zero.
    pub reaction_alt: u8,
    /// See [`CastActorState::reaction_alt`].
    pub reaction_alt2: u8,
    /// `+0x1F2` - the gate that picks [`CastActorState::knockdown_anim`]
    /// over [`CastActorState::reaction_alt`].
    pub reaction_gate: u8,
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
    /// `ctx+0x0D` - the band's own "this cast is finished" byte, which every
    /// PROT 0955 body clears in its terminal arm (`sb zero,0xd(ctx)`).
    pub ctx_0d: u8,
    /// `ctx+0x1A` - the **turn cursor**. The turn-steal arms bump it, which
    /// is how they consume the victim's turn.
    pub turn_cursor: u8,
    /// `ctx+0x27A` - a second scratch byte, cleared beside `ctx+0x278` by
    /// PROT 0965's damage arm (`0x801F789C`).
    pub ctx_27a: u8,
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

/// The three **whole-row sweeps'** damage shapes, keyed by tick body rather
/// than by PROT entry.
///
/// [`CAST_DAMAGE_SHAPES`] cannot hold them: PROT 0938 carries **two** bodies
/// with different baked powers and different skip guards behind one
/// trampoline, so an entry-keyed lookup would answer for whichever came
/// first. Each of the three writes `sh <net>, 0x14C(victim)` itself, over
/// `actor_table[0 .. ctx[+0]]`, after its own `jal 0x801DD4B0` - the same
/// shape PROT 0927's and 0966's stagers take, so these bodies **replace**
/// the generic fold rather than joining it (`World::fold_pending_cast`).
///
/// The one difference from those two stagers, and it matters: the clamp here
/// is `sltu` against the live HP (shape A, [`apply_hit_floor_zero`]), not
/// `slt` against `HP - 1`. **These sweeps kill**; the stagers cannot.
///
/// REF: FUN_801F726C (`0x801F77B0` the baked `0x274`, `0x801F77EC` the
/// unsigned clamp, `0x801F7820` the HP store)
/// REF: FUN_801F69EC (`0x801F70DC` / `0x801F7118` / `0x801F714C`)
/// REF: FUN_801F69D8 (`0x801F77A8` / `0x801F77E0` / `0x801F7814`)
pub const SWEEP_DAMAGE_SHAPES: [(u32, CastDamageShape); 3] = [
    (
        CHAOS_BREATH_TICK,
        CastDamageShape {
            prot_entry: 938,
            routine: 0x801F_726C,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x274],
        },
    ),
    (
        MYSTIC_CIRCLE_TICK,
        CastDamageShape {
            prot_entry: 938,
            routine: 0x801F_69EC,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x309],
        },
    ),
    (
        DOOMSDAY_TICK,
        CastDamageShape {
            prot_entry: 965,
            routine: 0x801F_69D8,
            wrapper: CastWrapper::Respect,
            never_kills: false,
            powers: &[0x600],
        },
    ),
];

/// The damage shape of one whole-row sweep body, keyed by the body constant
/// [`capture_tick_body`] resolves - see [`SWEEP_DAMAGE_SHAPES`] for why this
/// cannot be keyed by PROT entry.
pub fn sweep_damage_shape_for(body: u32) -> Option<&'static CastDamageShape> {
    SWEEP_DAMAGE_SHAPES
        .iter()
        .find(|(b, _)| *b == body)
        .map(|(_, s)| s)
}

/// Does this tick body own its cast's HP outcome outright - i.e. does the
/// module write `actor+0x14C` itself, so the band's generic fold must not
/// also run? True for exactly the three whole-row sweeps.
pub fn tick_body_owns_the_fold(body: u32) -> bool {
    sweep_damage_shape_for(body).is_some()
}

/// The module phase a whole-row sweep body applies its damage on - the one
/// arm that reaches the wrapper. A caller deciding whether the module has
/// already folded compares the live phase against this.
pub fn sweep_arm_for(body: u32) -> Option<u8> {
    match body {
        CHAOS_BREATH_TICK => Some(CHAOS_BREATH_SWEEP_ARM),
        MYSTIC_CIRCLE_TICK => Some(MYSTIC_CIRCLE_SWEEP_ARM),
        DOOMSDAY_TICK => Some(DOOMSDAY_SWEEP_ARM),
        _ => None,
    }
}

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
/// Wired: `World::run_cast_module_code`, at the cast band's staging seam.
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
/// Wired: `World::run_cast_module_code`.
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
/// Wired: `World::run_cast_module_code`.
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
/// Wired: `World::run_cast_module_code`.
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
/// Wired: `World::run_cast_module_code`.
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
/// Wired: `World::run_cast_module_code`.
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

// ---------------------------------------------------------------------------
// The capture-class trampolines (`0x801CF56C` arm -> tick body)
// ---------------------------------------------------------------------------

/// One capture-class module's trampoline: the routine PROT 0898's
/// `0x801CF56C` arm `jal`s, and the `(queued action id -> tick body)` map it
/// dispatches on.
///
/// The shape is uniform across the band and is 22 to 51 instructions long:
/// materialise the battle ctx `*0x8007BD24`, load the caster
/// `actor_table[ctx+0x13]` out of `0x801C9370`, read its queued action byte
/// `caster[+0x1DF]`, and either compare it against the module's own spell ids
/// in a `beq` chain or index a jump table. An id the map does not name falls
/// straight through to the epilogue with `a0 = 0`, so the module ticks
/// nothing and the drive loop's "tick returned zero" gate lets the battle
/// proceed.
#[derive(Debug, Clone, Copy)]
pub struct CaptureTrampoline {
    /// Extraction PROT entry of the owning module.
    pub prot_entry: u32,
    /// Retail VA of the trampoline itself - the `0x801CF56C` arm's `jal`
    /// target.
    pub trampoline: u32,
    /// `(action id, tick body VA)`, in `beq`-chain / table order.
    pub arms: &'static [(u8, u32)],
}

/// PROT 0955's twenty-word head table, read at file `+0x00..+0x50`: the
/// trampoline bounds `id - 0x60` with `sltiu 0x14` and jumps through it, so
/// the table's index space is action ids `0x60..=0x73`. Fourteen of the
/// twenty words point at the shared epilogue `0x801F9360` and tick nothing;
/// the six below are the module's real bodies.
///
/// This is a **third** owner for a band head table.
/// `docs/subsystems/cast-module.md` resolves head tables as the tick's or the
/// stager's by reading the `sltiu` immediate; 0955's belongs to neither - it
/// is the trampoline's.
pub const WHITE_SHIELD_TRAMPOLINE_ARMS: [(u8, u32); 6] = [
    (0x60, 0x801F_8F0C),
    (0x6E, 0x801F_86A4),
    (0x6F, 0x801F_7FA4),
    (0x70, 0x801F_767C),
    (0x72, 0x801F_7158),
    (0x73, 0x801F_6A28),
];

/// Every capture-class trampoline the port catalog lists, read off its
/// **owning** image's bytes at slot-B base `0x801F69D8`.
///
/// PROT 0957's trampoline (`0x801F9BA8`, ids `0x76` / `0x77`) is deliberately
/// absent: that VA carries distinct code in several band images and is filed
/// under `[worklist_va_aliased]`, so naming it here would put a `// PORT:`
/// claim on an address that is not one port site. Its two ids are constants
/// of their own ([`SUMMON_EFFECT_TICK_B_ID`] / [`SUMMON_EFFECT_TICK_A_ID`]).
///
/// The tag is one line on purpose: `port-catalog.py` scrapes a `PORT:` tag's
/// tail from the line it opens on, so a wrapped continuation drops every
/// address after the break.
///
/// PORT: FUN_801F7A40, FUN_801F7B1C, FUN_801F7B28, FUN_801F816C, FUN_801F8E60, FUN_801F92A4
pub const CAPTURE_TRAMPOLINES: [CaptureTrampoline; 6] = [
    // `beq v1, 0x4e -> 0x801F726C` / `beq v1, 0xb7 -> 0x801F69EC`.
    CaptureTrampoline {
        prot_entry: 938,
        trampoline: 0x801F_7A40,
        arms: &[(0x4E, 0x801F_726C), (0xB7, 0x801F_69EC)],
    },
    // Single arm, spelled as `bne v1, 0xb6 -> epilogue`.
    CaptureTrampoline {
        prot_entry: 951,
        trampoline: 0x801F_816C,
        arms: &[(0x36, 0x801F_6A20), (0x5B, 0x801F_77E8)],
    },
    CaptureTrampoline {
        prot_entry: 952,
        trampoline: 0x801F_7B28,
        arms: &[(0x5C, 0x801F_7118), (0xB8, 0x801F_6A0C)],
    },
    CaptureTrampoline {
        prot_entry: 955,
        trampoline: 0x801F_92A4,
        arms: &WHITE_SHIELD_TRAMPOLINE_ARMS,
    },
    CaptureTrampoline {
        prot_entry: 958,
        trampoline: 0x801F_8E60,
        arms: &[(0x79, 0x801F_6DD8)],
    },
    CaptureTrampoline {
        prot_entry: 965,
        trampoline: 0x801F_7B1C,
        arms: &[(0xB6, 0x801F_69D8)],
    },
];

/// PROT 0958's tick body, the arm its trampoline reaches for action `0x79`.
pub const BLAZING_SLASH_TICK: u32 = 0x801F_6DD8;
/// PROT 0952's tick body, the arm its trampoline reaches for action `0xB8`.
pub const ASTRAL_SLASH_TICK: u32 = 0x801F_6A0C;
/// PROT 0938's `0x4E` arm - [`chaos_breath_tick`].
pub const CHAOS_BREATH_TICK: u32 = 0x801F_726C;
/// PROT 0938's `0xB7` arm - [`mystic_circle_tick`].
pub const MYSTIC_CIRCLE_TICK: u32 = 0x801F_69EC;
/// PROT 0951's `0x36` arm - [`chaos_flare_tick`].
pub const CHAOS_FLARE_TICK: u32 = 0x801F_6A20;
/// PROT 0951's `0x5B` arm - [`scythe_wind_tick`].
pub const SCYTHE_WIND_TICK: u32 = 0x801F_77E8;
/// PROT 0952's `0x5C` arm - [`bloody_horns_tick`].
pub const BLOODY_HORNS_TICK: u32 = 0x801F_7118;
/// PROT 0965's `0xB6` arm - [`doomsday_tick`].
pub const DOOMSDAY_TICK: u32 = 0x801F_69D8;
/// PROT 0955's `0x60` arm - [`white_shield_tick`].
pub const WHITE_SHIELD_TICK: u32 = 0x801F_8F0C;
/// PROT 0955's `0x6E` arm - [`kiss_of_death_tick`].
pub const KISS_OF_DEATH_TICK: u32 = 0x801F_86A4;
/// PROT 0955's `0x6F` arm - [`melt_spray_tick`].
pub const MELT_SPRAY_TICK: u32 = 0x801F_7FA4;
/// PROT 0955's `0x70` arm - [`terror_scream_tick`].
pub const TERROR_SCREAM_TICK: u32 = 0x801F_767C;
/// PROT 0955's `0x72` arm - [`power_charge_tick`].
pub const POWER_CHARGE_TICK: u32 = 0x801F_7158;
/// PROT 0955's `0x73` arm - [`void_accessories_tick`].
pub const VOID_ACCESSORIES_TICK: u32 = 0x801F_6A28;
/// The phase PROT 0955's Melt Spray runs its five-stat debuff on
/// (`0x801F828C`, the `slt v1, 4` arm of its chain).
pub const MELT_SPRAY_DEBUFF_ARM: u8 = 3;
/// The phase PROT 0938's Chaos Breath runs its sweep on (`0x801F7750`).
pub const CHAOS_BREATH_SWEEP_ARM: u8 = 2;
/// The phase PROT 0938's Mystic Circle runs its sweep on: table word 3
/// (`0x801F69E4` -> `0x801F6F3C`), the arm the loop at `0x801F70B0` sits in.
pub const MYSTIC_CIRCLE_SWEEP_ARM: u8 = 3;
/// The phase PROT 0965's Doomsday runs its sweep on: the `beq v1, 0x0C` /
/// `slt` pair at `0x801F6AE8` sends `0x0B` to `0x801F7648`, and the loop at
/// `0x801F76E4` sits inside that arm.
pub const DOOMSDAY_SWEEP_ARM: u8 = 0x0B;

/// The trampoline of the module PROT `prot_entry` pages, if it has one.
pub fn capture_trampoline_for(prot_entry: u32) -> Option<&'static CaptureTrampoline> {
    CAPTURE_TRAMPOLINES
        .iter()
        .find(|t| t.prot_entry == prot_entry)
}

/// Resolve one capture-class cast through its module's trampoline: which tick
/// body VA the caster's queued action id `caster[+0x1DF]` reaches.
///
/// `None` is retail's fall-through - the trampoline returns `a0 = 0`, the
/// module ticks nothing, and the drive loop proceeds. That is why a
/// "multi-spell cell" is several whole choreographies in one image rather
/// than one body branching internally: PROT 0955 holds **six**, the widest in
/// the band.
pub fn capture_tick_body(prot_entry: u32, action_id: u8) -> Option<u32> {
    capture_trampoline_for(prot_entry).and_then(|t| {
        t.arms
            .iter()
            .find(|(id, _)| *id == action_id)
            .map(|(_, body)| *body)
    })
}

// ---------------------------------------------------------------------------
// Six more tick bodies (`0x801CF4EC` / `0x801CF56C` arm, per-module)
// ---------------------------------------------------------------------------

/// PROT 0925 (Spikefish) tick body.
///
/// Ten phase arms behind `sltiu a0, 0xa` (`0x801F6A68`) through the head
/// table filling file `0x0..0x28`; `$s3 = ctx + 0x279` is materialised in the
/// same breath at `0x801F6A64`. No damage-wrapper call anywhere in its 1082
/// instructions - the module's whole simulation footprint is one staged clip
/// with its restage bump (`0x801F7448`), two `ctx+0x278` writes
/// (`0x801F6AC4` seeds it, `0x801F7AB0` clears it in the terminal arm), and
/// two animation-rate stores.
///
/// Ported: the bound, the phase walk, the stage/restage pair and the
/// `ctx+0x278` discipline. Not ported: the packet arms, which are most of the
/// body.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A00 (phase machine + staging; packet arms unported)
pub fn spikefish_tick(ctx: &mut CastModuleCtx, caster: &mut CastActorState) -> CastTickStep {
    run_tick(ctx, 10, |c| {
        if c.phase == 0 {
            c.ctx_278 = 0;
        }
        if c.phase == SPIKEFISH_STAGE_ARM {
            stage_clip(caster, caster.staged_anim);
        }
        false
    })
}

/// The arm PROT 0925 stages its clip from (`sb t0, 0x1da(v0)` at
/// `0x801F7448`, inside the arm the head table's word 5 reaches).
pub const SPIKEFISH_STAGE_ARM: u8 = 5;

/// PROT 0924 (Ultimate Rave) tick body.
///
/// Twelve phase arms behind `sltiu a1, 0xc` (`0x801F6AA8`) through the table
/// at `0x801F69E8` - base `+0x10`, i.e. the head table starts four words in,
/// which is why a reader that assumes file `+0` misses it. Five `+0x1DA`
/// stages against four restage bumps (the unpaired one is `0x801F7350`,
/// which writes the staged clip and the animation rate in the same pair of
/// instructions), ten `+0x21D` animation-rate stores, three `ctx+0x278`
/// writes and no damage wrapper.
///
/// Its one HP write is not a hit: `sh zero, 0x14c(s3)` at `0x801F76A0` zeroes
/// the victim's HP outright in the finale arm, next to `+0x225` / `+0x21C`
/// render flags - the seat-0 "declare the victim dead" shape the module docs
/// name, not a roll through a wrapper.
///
/// Ported: the bound, the phase walk, the stage/restage pairs and the finale
/// HP zero. Not ported: the packet and camera arms.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A18 (phase machine + staging + the finale HP zero; packet
/// arms unported)
pub fn ultimate_rave_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
) -> CastTickStep {
    run_tick(ctx, 12, |c| {
        if c.phase == ULTIMATE_RAVE_FINALE_ARM {
            victim.hp = 0;
            victim.render_flag = 2;
            victim.anim_rate = 2;
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        } else {
            stage_clip(caster, caster.staged_anim);
        }
        false
    })
}

/// The arm PROT 0924 zeroes the victim's HP in (`sh zero, 0x14c(s3)` at
/// `0x801F76A0`).
pub const ULTIMATE_RAVE_FINALE_ARM: u8 = 9;

/// PROT 0922 (Puera) tick body - 2474 instructions, the band's longest.
///
/// Twenty-five phase arms behind `sltiu a0, 0x19` (`0x801F6AB4`) through the
/// head table at file `+0`. One `FUN_801DD0AC` site at `0x801F8E1C` with the
/// baked power `0x12` and `a1 = 7` (the shared kernel's summon branch), under
/// the same two guards the band's AoE stagers use - skip a dead seat
/// (`+0x14C == 0`) and skip a non-targetable one (`+0x16E & 4`) - followed by
/// the **shape-A** clamp at `0x801F8E48` (`sltu a0, s1`, unsigned).
///
/// That corrects a reading `docs/subsystems/cast-module.md` could be taken to
/// imply: PROT 0927's *stager* clamps to `HP - 1` and cannot kill, but the
/// summon-branch wrapper is not itself a never-kill shape - this module calls
/// it with the kill-capable clamp.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A3C (phase machine + damage/staging; packet arms unported)
pub fn puera_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 25, |_| {
        if let Some(roll) = hit
            && aoe_seat_is_hittable(victim)
        {
            apply_hit_floor_zero(victim, roll);
        }
        false
    })
}

/// PROT 0927 (Juggernaut) tick body - the sibling of the AoE stager
/// [`juggernaut_stager`] and a **second, differently clamped** damage site in
/// the same module.
///
/// Twenty-nine phase arms behind `sltiu a1, 0x1d` (`0x801F6B04`) through the
/// table at `0x801F69E8`. Its `FUN_801DD0AC` site is `0x801F7E0C`, same baked
/// `0x12` and `a1 = 7` as the stager, but the clamp at `0x801F7E38` is
/// `sltu a0, s1` - **shape A**, which can kill - where the stager's
/// `0x801F85C4` clamp is the signed `HP - 1` shape that cannot.
///
/// So "PROT 0927 never kills" is true of its move-VM sweep only. The tick's
/// hit is kill-capable, and a negative wrapper return there kills outright
/// through the unsigned compare.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A84 (phase machine + damage/staging; packet arms unported)
pub fn juggernaut_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 29, |_| {
        if let Some(roll) = hit
            && aoe_seat_is_hittable(victim)
        {
            apply_hit_floor_zero(victim, roll);
        }
        false
    })
}

/// PROT 0918 (Kemaro) tick body.
///
/// The one body in this set whose head is a `beq`/`slti` chain rather than a
/// word table (`0x801F6D08` onward); its phase literals run `1 ..= 0x14` plus
/// the `0xFF` done marker. One `FUN_801DD0AC` site at `0x801F87A4`, and its
/// power is **not** loaded as its own literal - the arm gate
/// `addiu v0, zero, 0x12; bne v1, v0` compares the phase byte against `0x12`
/// and then reuses the same register as `a0` (`move a0, v0` at `0x801F8798`),
/// so the phase number and the baked power are one constant. A reader that
/// takes `move a0, v0` at face value loses the power entirely.
///
/// Shape-A clamp at `0x801F87C8`, spelled the other way round
/// (`sltu s1, a1` - damage below HP branches *past* the clamp). The arm that
/// does clamp also credits a kill: it increments the word at `+0x664` of the
/// caster's per-character record in the `0x80084140 + n * 0x414` block.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6C70 (phase chain + damage/staging + the kill credit; packet
/// arms unported)
pub fn kemaro_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> Option<CastTickStep> {
    if ctx.phase == KEMARO_DONE_PHASE {
        return Some(CastTickStep::Done);
    }
    if ctx.phase > KEMARO_LAST_PHASE {
        return None;
    }
    if ctx.phase == KEMARO_DAMAGE_PHASE
        && let Some(roll) = hit
    {
        apply_hit_floor_zero(victim, roll);
    }
    advance_phase(ctx);
    Some(CastTickStep::Busy)
}

/// PROT 0918's damage arm, and - the same constant - the `a0` its
/// `FUN_801DD0AC` site bakes.
pub const KEMARO_DAMAGE_PHASE: u8 = 0x12;
/// The widest phase literal PROT 0918's `beq`/`slti` chain compares.
pub const KEMARO_LAST_PHASE: u8 = 0x14;
/// The band's "choreography done" phase marker.
pub const KEMARO_DONE_PHASE: u8 = 0xFF;

/// PROT 0949 (Water Crystals) tick body - the sibling of the freeze-ramp
/// stager [`water_crystals_stager`].
///
/// Six phase arms behind `sltiu v1, 6` (`0x801F6AA0`) through the head table
/// at file `+0`, and the victim is derived the band's way in the prologue
/// (`caster[+0x1DD]` at `0x801F6A5C`, then `actor_table[that]`). One
/// `FUN_801DD4B0` site at `0x801F7318` with the baked power `0xC0`
/// (`0x801F72F8`), then the shape-A clamp at `0x801F733C`, the `+0x10`
/// accumulate, the HP write, a `+0x1DC = 1` restage store and the victim's
/// own `+0x1F1` reaction stage.
///
/// `0xC0` is a power `docs/subsystems/cast-module.md`'s baked-constant table
/// does not carry: that table was read off the routines already on the
/// verdict table, and this tick was not one of them.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A10 (phase machine + damage/staging; packet arms unported)
pub fn water_crystals_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 6, |_| {
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
            victim.restage = 1;
            let knockdown = victim.knockdown_anim;
            victim.staged_anim = knockdown;
        }
        false
    })
}

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
fn run_tick_latched(
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
/// RNG cursor. Not ported: the packet and camera arms, and arms `1` / `3`,
/// which are frame-gated presentation.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F726C (phase chain + the whole-row damage/status sweep; packet arms unported)
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
            for seat in 0..c.actor_count {
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
/// PORT: FUN_801F69EC (phase table + the whole-row damage sweep; packet arms unported)
pub fn mystic_circle_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    damage_arm: bool,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_tick_latched(ctx, |c| {
        if damage_arm {
            for seat in 0..c.actor_count {
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
/// PORT: FUN_801F6A20 (phase table + damage/staging; packet arms unported)
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
/// `FUN_801DD6B4` is the physical wrapper - the one that folds the defender's
/// two defence stats - which is what makes this body's `0x1D0` a physical
/// figure rather than a spell one.
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
/// PORT: FUN_801F69D8 (phase chain + the whole-row damage sweep; packet arms unported)
pub fn doomsday_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    damage_arm: bool,
    mut rolls: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_tick_latched(ctx, |c| {
        if damage_arm {
            for seat in 0..c.actor_count {
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
/// `bnez ...; addiu v0,v0,1` pair each store is followed by, and it tests the
/// full **32-bit** difference while the store is a 16-bit `sh` - so a stat of
/// `2` lands on zero and is corrected to `1`, but a stat of `0` or `1` goes
/// to `-1`, misses the `bnez`, and is written back as `0xFFFF`. Retail
/// underflows a one-point stat into 65535; that is the behaviour, not a port
/// artefact, and it is why the floor cannot be modelled as `max(1, ..)`.
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
