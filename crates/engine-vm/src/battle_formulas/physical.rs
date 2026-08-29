//! Melee (physical / direction-command) pre-damage - the roll pair and the
//! underdog rewrite of `FUN_801EC3E4`. Split out of `battle_formulas.rs`.
//!
//! This is the kernel every **melee** hit runs, and it is a different routine
//! from the summon / arts roll [`super::arts_physical_predamage`]
//! (`FUN_801DD0AC`): that one is seeded by the static move-power table and
//! finishes through `FUN_801DDB30`, while `FUN_801EC3E4` rolls attacker ATK
//! against defender UDF/LDF and applies the HP loss itself.
//!
//! The structural point - and the reason a port that skips it looks broken -
//! is the **underdog rewrite**. `raw` and `guard` are two independent rolls,
//! and when the attack roll does not clear the guard roll the routine does not
//! floor the hit at 1: it *rewrites* `raw` to `guard` plus a fraction of
//! itself, and then floors that at a further few points. A weak attacker into
//! a heavily-armoured defender therefore still lands a real, scaling hit.

/// Per-direction-command power scalars - `0x801F64EC[(move_id - 0x0C) % 5]`
/// in the battle overlay (PROT 0898, link base `0x801CE818`).
///
/// The same five values `legaia_art::power` already carries as its power-tier
/// multiplier scale; `FUN_801EC3E4` indexes them by the staged command byte
/// rather than by an art record's power tier.
pub const COMMAND_POWER_SCALARS: [u8; 5] = [12, 18, 20, 22, 28];

/// The scalar for a staged battle command / art action id, `(id - 0x0C) % 5`
/// into [`COMMAND_POWER_SCALARS`]. Ids below `0x0C` (there are none in the
/// command band) clamp to the first entry.
pub fn command_power_scalar(staged_id: u8) -> u8 {
    let idx = staged_id.wrapping_sub(0x0C) as usize % COMMAND_POWER_SCALARS.len();
    COMMAND_POWER_SCALARS[idx]
}

/// The lowest staged-anim id that counts as an **art** rather than a plain
/// direction swing (`0x10 < actor[+0x1D9]`, `0x801ED0AC`). The art arms scale
/// `raw` by 13/10 (14/10 with ability bit `0x1000`) before the affinity pass,
/// and 11/10 (12/10) inside the underdog rewrite.
pub const ART_ANIM_THRESHOLD: u8 = 0x10;

/// Inputs to [`physical_predamage`]. Every field maps to one live battle read
/// in `FUN_801EC3E4`; the [`Default`] is the neutral melee hit (no element
/// affinity, no combo scalars, no status), which is what a port with no
/// element table installed resolves to.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalHit {
    /// Attacker ATK, actor `+0x158` **after** the resolver's execution-time
    /// equipment fold: the actor's working ATK (seeded from the character
    /// record with no equipment) plus half of the one equipment slot the
    /// command reads - [`super::arms_weapon_atk_fold`]. The caller does the
    /// fold; this kernel takes the sum.
    pub attacker_atk: u16,
    /// Attacker live HP, actor `+0x14C` - contributes `hp >> 8`.
    pub attacker_hp: u16,
    /// Defender defence: UDF (`+0x15C`) when the staged command satisfies
    /// `(id - 0x0C) % 10 < 5`, else LDF (`+0x160`). The caller does the
    /// selection; see [`physical_defense_is_udf`].
    pub defender_def: u16,
    /// `0x801F64EC[(id - 0x0C) % 5]` - see [`command_power_scalar`].
    pub command_scalar: u8,
    /// The attacker's staged anim id (`+0x1D9`). `> `[`ART_ANIM_THRESHOLD`]
    /// selects the art arms.
    pub staged_anim: u8,
    /// Character-record ability word `+0xF8` bit `0x1000` on a party
    /// attacker (accessory passive `0x2C` **Arts Power** - War Soul; read at
    /// `0x801ED0F8..0x801ED104`) - picks 14/10 over 13/10 (and 12/10 over
    /// 11/10 in the rewrite).
    pub art_power_bit: bool,
    /// `matrix[attacker_element][defender_element]` from the element-affinity
    /// table (`0x801F53E8`), as a percent. `100` = neutral, which is what an
    /// engine with no affinity table installed passes.
    pub affinity_pct: u8,
    /// Attacker `+0x16E`: bit `0x1` scales `raw` by 9/10, bit `0x2` by 7/10.
    pub attacker_status: u16,
    /// Defender `+0x16E`: same two bits, applied to `guard`.
    pub defender_status: u16,
    /// Defender `+0x1DE == 4` (the Spirit guard stance) - **triples** the
    /// guard roll (`0x801ED210..0x801ED230`). This is the melee path's whole
    /// guard model; it does not also take `FUN_801DDB30`'s halve, which
    /// belongs to the summon/arts kernel. Retail's same `beq` also fires the
    /// triple when the defender is fleeing (`+0x1DE == 5`) and any living
    /// party member carries `+0xF8` bit `0x200000` (passive `0x35` Safe
    /// Escape); the caller folds that case into this flag.
    pub defender_guarding: bool,
    /// `ctx[+0x0A]` - the **juggle** counter: `1` on a chain's first hit,
    /// `+1` per further hit while the defender's hit-reaction timer
    /// (`+0x1F7`) is still running, back to `1` otherwise
    /// (`0x801ECA20..0x801ECA80`).
    pub combo_scale: u8,
    /// `ctx[+0x6D2]` (i16) - the **attack angle** term, written by the
    /// approach state of `FUN_801E295C` (`0x801E3068..0x801E30C8`) as the
    /// folded facing difference minus `0x800`: `0` for a face-on strike.
    pub attack_ramp: i16,
    /// `ctx[+0x6D4]` (i16) - the **approach distance** accumulated while the
    /// attacker walks in (`0x801E35DC..0x801E35EC`); the kernel zeroes it once
    /// the first hit lands, so only a chain's opening hit pays it.
    pub guard_ramp: i16,
}

impl Default for PhysicalHit {
    fn default() -> Self {
        Self {
            attacker_atk: 0,
            attacker_hp: 0,
            defender_def: 0,
            command_scalar: COMMAND_POWER_SCALARS[0],
            staged_anim: 0x0C,
            art_power_bit: false,
            affinity_pct: 100,
            attacker_status: 0,
            defender_status: 0,
            defender_guarding: false,
            combo_scale: 0,
            attack_ramp: 0,
            guard_ramp: 0,
        }
    }
}

/// Which defence half a staged command reads: UDF when
/// `(id - 0x0C) % 10 < 5` (`0x801ECE14..0x801ECE40`), LDF otherwise.
pub fn physical_defense_is_udf(staged_id: u8) -> bool {
    (staged_id.wrapping_sub(0x0C) as u32 % 10) < 5
}

/// The retail melee damage cap, applied as `raw <= guard + 9999`
/// (`0x801EDA00`) - i.e. a cap on the *damage*, not on `raw`.
pub const PHYSICAL_DAMAGE_CAP: u32 = 9999;

fn scale_10(v: u32, num: u32) -> u32 {
    v.saturating_mul(num) / 10
}

fn scale_pct(v: u32, pct: u8) -> u32 {
    v.saturating_mul(u32::from(pct)) / 100
}

/// One melee hit's damage - `FUN_801EC3E4`'s roll pair, art scale, status
/// scales, underdog rewrite and chip floor, returning `raw - guard` capped at
/// [`PHYSICAL_DAMAGE_CAP`].
///
/// `rand` is the shared PsyQ `rand()` (`FUN_80056798`), drawn in retail order:
/// the attacker roll first (`0x801ECE78`), the guard roll second
/// (`0x801ED1B0`), then - only when the underdog arm fires - the rewrite draw
/// (`0x801ED360`) and, only when the chip floor fires, one more. A hit that
/// clears the guard therefore consumes exactly two draws, the same as retail.
///
/// The stages, each cited to `overlay_battle_action_801ec3e4.txt`:
///
/// 1. **Attacker roll** (`0x801ECE78..0x801ECF18`):
///    `raw = ((atk + rand%((atk>>3)+1)) * scalar >> 4) + (hp>>8)
///          + ((combo*atk)>>6) + ((attack_ramp*atk)>>16)`.
/// 2. **Art scale + affinity** (`0x801ED0AC..0x801ED174`): a staged id above
///    [`ART_ANIM_THRESHOLD`] takes `raw = raw*13/10` (`*14/10` with the
///    ability bit) then `raw = raw*affinity/100`; every path then takes a
///    second `raw = raw*affinity/100` (`0x801ED178`), so an art scales by
///    affinity twice.
/// 3. **Guard roll** (`0x801ED1B0..0x801ED234`):
///    `guard = def + rand%((def>>3)+1) + ((def*guard_ramp)>>10)`, tripled when
///    the defender holds the Spirit stance.
/// 4. **Status scales** (`0x801ED25C..0x801ED304`): `9/10` on bit `0x1`,
///    `7/10` on bit `0x2`, applied to `raw` from the attacker's word and to
///    `guard` from the defender's.
/// 5. **Underdog rewrite** (`0x801ED308..0x801ED3E0`): when
///    `raw <= guard + ((raw*scalar)>>6) + ((combo*raw)>>6) + combo`,
///    `raw` is *replaced* by
///    `guard + ((((raw*3)>>2) + rand%((raw>>2)+1)) * scalar >> 6)
///           + ((combo*raw)>>6) + combo`
///    (the `raw` inside the two right-hand terms is the pre-rewrite value),
///    followed by the same art (`11/10` / `12/10`) + affinity pass.
/// 6. **Chip floor** (`0x801ED4A0..0x801ED5BC`), inside the rewrite arm only:
///    a plain swing whose `raw` is still within `guard + combo + 3` becomes
///    `guard + rand%3 + 3 + combo`; an art within `guard + combo + 5` becomes
///    `guard + rand%4 + 5 + combo`.
/// 7. **Cap** (`0x801EDA00`) and `damage = raw - guard` (`0x801EDA6C`).
///
/// Not reproduced here (they are separate, already-ported or live-state
/// stages): the party-defender elemental-guard / All-Guard ladder
/// (`0x801ED844..0x801EDA00`, whose arithmetic is
/// [`super::damage_finish`]'s resist stage), the `0x4000` quarter-damage
/// ability arm, the HP-bar accumulator write, and the spirit-gauge accrual
/// ([`super::spirit_gauge_fill`]).
///
/// PORT: FUN_801EC3E4 (the melee roll pair + underdog rewrite + chip floor)
pub fn physical_predamage(hit: &PhysicalHit, rand: &mut impl FnMut() -> u16) -> u16 {
    let atk = u32::from(hit.attacker_atk);
    let scalar = u32::from(hit.command_scalar);
    let combo = u32::from(hit.combo_scale);

    // 1 - attacker roll.
    let r = u32::from(rand()) % ((atk >> 3) + 1);
    let ramp_term = ((i64::from(hit.attack_ramp) * i64::from(atk)) as u32) >> 16;
    let mut raw = (atk.saturating_add(r).saturating_mul(scalar) >> 4)
        .saturating_add(u32::from(hit.attacker_hp) >> 8)
        .saturating_add((combo.saturating_mul(atk)) >> 6)
        .saturating_add(ramp_term);

    // 2 - art scale + affinity.
    let is_art = hit.staged_anim > ART_ANIM_THRESHOLD;
    if is_art {
        raw = scale_10(raw, if hit.art_power_bit { 14 } else { 13 });
        raw = scale_pct(raw, hit.affinity_pct);
    }
    raw = scale_pct(raw, hit.affinity_pct);

    // 3 - guard roll.
    let def = u32::from(hit.defender_def);
    let rg = u32::from(rand()) % ((def >> 3) + 1);
    let guard_ramp_term = ((i64::from(hit.guard_ramp) * i64::from(def)) as u32) >> 10;
    let mut guard = def
        .saturating_add(rg)
        .saturating_add(guard_ramp_term)
        .saturating_mul(if hit.defender_guarding { 3 } else { 1 });

    // 4 - status scales.
    if hit.attacker_status & 0x1 != 0 {
        raw = scale_10(raw, 9);
    }
    if hit.attacker_status & 0x2 != 0 {
        raw = scale_10(raw, 7);
    }
    if hit.defender_status & 0x1 != 0 {
        guard = scale_10(guard, 9);
    }
    if hit.defender_status & 0x2 != 0 {
        guard = scale_10(guard, 7);
    }

    // 5 - underdog rewrite.
    let clears = guard
        .saturating_add((raw.saturating_mul(scalar)) >> 6)
        .saturating_add((combo.saturating_mul(raw)) >> 6)
        .saturating_add(combo)
        < raw;
    if !clears {
        let prev = raw;
        let r2 = u32::from(rand()) % ((prev >> 2) + 1);
        raw = guard
            .saturating_add(
                (((prev.saturating_mul(3) >> 2).saturating_add(r2)).saturating_mul(scalar)) >> 6,
            )
            .saturating_add((combo.saturating_mul(prev)) >> 6)
            .saturating_add(combo);
        if is_art {
            raw = scale_10(raw, if hit.art_power_bit { 12 } else { 11 });
            raw = scale_pct(raw, hit.affinity_pct);
        }
        raw = scale_pct(raw, hit.affinity_pct);
        // 6 - chip floor.
        if !is_art {
            if raw <= guard.saturating_add(combo).saturating_add(3) {
                raw = guard
                    .saturating_add(u32::from(rand()) % 3 + 3)
                    .saturating_add(combo);
            }
        } else if raw <= guard.saturating_add(combo).saturating_add(5) {
            raw = guard
                .saturating_add(u32::from(rand()) % 4 + 5)
                .saturating_add(combo);
        }
    }

    // 7 - cap + damage.
    let damage = raw.saturating_sub(guard).min(PHYSICAL_DAMAGE_CAP);
    damage.min(u32::from(u16::MAX)) as u16
}
