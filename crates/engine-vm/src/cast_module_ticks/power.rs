//! Baked per-hit power constants and the per-module damage shapes.
//! Split out of `cast_module_ticks.rs`.

use super::*;

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

/// PROT 0959's three baked powers, in call-site order (`0x801F71E8`,
/// `0x801F7A90`, `0x801F7EBC`).
///
/// All three are `FUN_801DD6B4` sites inside the single arm body
/// `FUN_801F69F0` that the trampoline's `0x7A` arm reaches, each followed by
/// the shape-A clamp (`sltu` against the live `+0x14C` at `0x801F7214` /
/// `0x801F7ABC` / `0x801F7EE8`), so every hit can kill. The third site is the
/// one the module fires repeatedly.
pub const MEGATON_PRESS_POWERS: [u16; 3] = [0x80, 0x80, 0x30];

/// Every damage shape the band's PORT rows carry, read off the `a0` set
/// before each `jal` into `0x801DD0AC` / `0x801DD4B0` / `0x801DD6B4`.
pub const CAST_DAMAGE_SHAPES: [CastDamageShape; 7] = [
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
        prot_entry: 959,
        routine: 0x801F_69F0,
        wrapper: CastWrapper::Bypass,
        never_kills: false,
        powers: &MEGATON_PRESS_POWERS,
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

/// The baked `a0` of the capture-class bodies whose tick port carries no
/// damage shape of its own, as `(PROT entry, action ids, power, site)`;
/// empty ids = the module's only body. Each is the immediate loaded into
/// `a0` ahead of the body's `jal 0x801DD4B0` (or `0x801DD6B4` - the
/// wrapper choice is the caller's, keyed on the action id) in the module
/// image at slot-B base `0x801F69D8`. Where a body has two sites they carry
/// the same constant (PROT 0935's `0x801F7AFC` / `0x801F7C50`).
///
/// Without these the cast band's fold had no magnitude for the cast: a
/// capture-class special of one of these modules dealt nothing at all.
pub const CAPTURE_SITE_POWERS: &[(u32, &[u8], u16, u32)] = &[
    (935, &[], 0x1AE, 0x801F_7AFC),
    (936, &[], 0x1E4, 0x801F_797C),
    (937, &[], 0x26A, 0x801F_76AC),
    (939, &[], 0x60, 0x801F_70E0),
    (942, &[0xAA], 0x280, 0x801F_774C),
    (946, &[], 0x120, 0x801F_73DC),
    (947, &[], 0xEA, 0x801F_7414),
    (948, &[], 0x78, 0x801F_6FB0),
    (949, &[], 0xC0, 0x801F_7318),
    (951, &[0x36], 0x3A0, 0x801F_7414),
    (951, &[0x5B], 0x80, 0x801F_7F88),
    (952, &[0x5C], 0x1D0, 0x801F_7948),
    (953, &[], 0x274, 0x801F_7410),
    (956, &[0x75], 0x100, 0x801F_6EF8),
    (961, &[], 0x880, 0x801F_73AC),
    (963, &[], 0x540, 0x801F_7C54),
    (964, &[0xB0, 0xB1, 0xB2], 0x580, 0x801F_74CC),
];

/// The [`CAPTURE_SITE_POWERS`] constant for a cast of `action_id` through
/// PROT `prot_entry`.
pub fn capture_site_power(prot_entry: u32, action_id: u8) -> Option<u16> {
    CAPTURE_SITE_POWERS
        .iter()
        .find(|(e, ids, _, _)| *e == prot_entry && (ids.is_empty() || ids.contains(&action_id)))
        .map(|&(_, _, power, _)| power)
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
        CastWrapper::Bypass => atk_wrapper_predamage(
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
        CastWrapper::Respect | CastWrapper::SharedSummon => int_wrapper_predamage(
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
