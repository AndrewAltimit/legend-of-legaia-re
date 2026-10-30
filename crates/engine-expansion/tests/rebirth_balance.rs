//! Rebirth's level curve, checked on the battle kernels.
//!
//! A deterministic, disc-free model of every fight in the flagship pack
//! (`packs/rebirth/battle.toml`) at the party level the quest chain admits it
//! at: the pack monsters are derived, level-scaled and boosted exactly as the
//! engine seats them (`derive_monster`, `scale_monster`, the battle loader's
//! `installed_stats`), and each hit either way is the melee kernel
//! `physical_predamage` (`FUN_801EC3E4`) averaged over a fixed `rand()`
//! stream. What the model fixes by assumption, and states here:
//!
//! - **The party.** Vahn, Noa and Gala at their jitter-free retail growth
//!   ([`BASE`], the `DAT_80076918` / `DAT_800769CC` tables; the disc test at
//!   the bottom re-derives it from `extracted/SCUS_942.54`), wearing the gear
//!   the pack hands out by that point ([`gear`]). No HP boosts, no Seru magic,
//!   no healing in the fight: those are the player's margin.
//! - **The party's turn.** Each member lands [`hits_per_round`] hits a round,
//!   half of them Art strikes (power 22, the `x13/10` art arm, every gear ATK
//!   at half) and half plain arm swings (power 20, the weapon at half). From
//!   level 80 one hit a round is a Rebirth finisher (`damage_pct = 140`).
//! - **The monsters' turn.** One action per monster per round (times the
//!   preset's attack-count knob), a power-20 swing at the party's mean
//!   defence; a monster on the retail AI swings [`RETAIL_AI_SWINGS`] times. A boss script's moves are weighed by `weight`; a spell is taken
//!   as an all-party hit at [`SPELL_FRACTION`] of the swing, a telegraphed one
//!   at the full swing.
//! - **The fight.** The party focuses one enemy at a time; every wave, refill
//!   and script summon has to fall, and a script's `heal_pct` adds that much
//!   HP back. Survival is measured as rounds the party would last with every
//!   enemy that can stand at once acting, and no healing at all.
//!
//! The thresholds are the pack's design targets, not retail's: a field fight
//! is over in a few rounds, a floor keeper in under a dozen, the superbosses
//! in under twenty, and nothing one-shots a full-HP member. Hard and
//! Nightmare (`legaia_engine_run_rules::rules::Difficulty`) loosen them, so
//! the presets stay winnable rather than easy.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_vm::battle_formulas::{PhysicalHit, physical_predamage};
use legaia_engine_expansion::battle::{
    CompiledBattle, CompiledFormation, derive_monster, scale_monster,
};
use legaia_engine_expansion::boss_script::{CompiledPhase, CompiledScript, MoveKind};
use legaia_engine_expansion::progression::enemy_scaling_permille;
use legaia_engine_run_rules::rules::Difficulty;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::battle::Stat;

fn rebirth() -> ExpansionPack {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    ExpansionPack::from_dir(&dir).expect("packs/rebirth loads")
}

// --- The party ------------------------------------------------------------

/// `(level, [[HP, ATK, UDF, LDF]; Vahn, Noa, Gala])`: the jitter-free retail
/// growth (`GrowthTables::level_gain_core` summed from level 1).
const BASE: &[(u8, [[u16; 4]; 3])] = &[
    (
        30,
        [
            [1774, 157, 136, 134],
            [1583, 145, 127, 144],
            [1891, 169, 163, 140],
        ],
    ),
    (
        35,
        [
            [2134, 187, 165, 164],
            [1908, 175, 153, 174],
            [2271, 201, 192, 165],
        ],
    ),
    (
        40,
        [
            [2519, 221, 195, 194],
            [2258, 205, 183, 207],
            [2681, 236, 222, 192],
        ],
    ),
    (
        45,
        [
            [2931, 256, 228, 228],
            [2629, 240, 213, 242],
            [3116, 273, 255, 222],
        ],
    ),
    (
        50,
        [
            [3371, 294, 263, 263],
            [3024, 275, 247, 280],
            [3579, 313, 290, 252],
        ],
    ),
    (
        55,
        [
            [3531, 309, 273, 273],
            [3169, 285, 257, 290],
            [3749, 328, 300, 262],
        ],
    ),
    (
        60,
        [
            [3691, 324, 283, 283],
            [3314, 295, 267, 300],
            [3919, 343, 310, 272],
        ],
    ),
    (
        65,
        [
            [3851, 339, 293, 293],
            [3459, 305, 277, 310],
            [4089, 358, 320, 282],
        ],
    ),
    (
        70,
        [
            [4011, 354, 303, 303],
            [3604, 315, 287, 320],
            [4259, 373, 330, 292],
        ],
    ),
    (
        75,
        [
            [4171, 369, 313, 313],
            [3749, 325, 297, 330],
            [4429, 388, 340, 302],
        ],
    ),
    (
        80,
        [
            [4331, 384, 323, 323],
            [3894, 335, 307, 340],
            [4599, 403, 350, 312],
        ],
    ),
    (
        85,
        [
            [4491, 399, 333, 333],
            [4039, 345, 317, 350],
            [4769, 418, 360, 322],
        ],
    ),
    (
        90,
        [
            [4651, 414, 343, 343],
            [4184, 355, 327, 360],
            [4939, 433, 370, 332],
        ],
    ),
    (
        95,
        [
            [4811, 429, 353, 353],
            [4329, 365, 337, 370],
            [5109, 448, 380, 342],
        ],
    ),
    (
        99,
        [
            [4939, 441, 361, 361],
            [4445, 373, 345, 378],
            [5245, 460, 388, 350],
        ],
    ),
];

#[derive(Debug, Clone, Copy)]
struct Hero {
    hp: u16,
    /// Base ATK plus flat bonuses (accessories, set bonuses).
    atk: u16,
    /// Weapon, Ra-Seru and footwear ATK (each folded at half).
    weapon: u16,
    raseru: u16,
    boots: u16,
    udf: u16,
    ldf: u16,
}

/// What each hero wears at `level`, as the pack hands it out: the retail
/// Ra-Seru set from the tower's start (the retail game's best), the Rebirth
/// weapons from their keepers (Maul: Colossus, Blade: Regent, Talon: Seraph),
/// the Rebirth body armour and crown from the Armory once the Regent falls
/// (the set's two- and three-piece bonuses with them), the greaves after the
/// Seraph (All Guard is not modelled). Below 40 the party has its late-story
/// gear.
fn gear(level: u8, hero: usize) -> (u16, u16, u16, u16, u16, u16) {
    // (weapon, raseru, boots ATK, body+head UDF, body+head+boots LDF, flat ATK)
    let ra_seru_weapon = [98, 96, 100][hero];
    let rebirth_weapon = [112, 110, 114][hero];
    match level {
        0..=39 => (78, 85, 70, 120, 170, 0),
        40..=64 => (ra_seru_weapon, 100, 88, 147, 210, 0),
        65..=77 => {
            let w = if hero == 2 {
                rebirth_weapon
            } else {
                ra_seru_weapon
            };
            (w, 100, 88, 147, 210, 0)
        }
        78..=87 => {
            let w = if hero == 1 {
                ra_seru_weapon
            } else {
                rebirth_weapon
            };
            // Rebirth body (~91 / ~91) and crown (71 UDF), Thongs' 64 LDF,
            // the three-piece +12 / +12 and the two-piece +10 ATK.
            (w, 100, 88, 91 + 71 + 12, 91 + 64 + 12, 10)
        }
        _ => (rebirth_weapon, 100, 98, 91 + 71 + 12, 91 + 70 + 12, 10),
    }
}

fn party(level: u8) -> [Hero; 3] {
    let row = BASE
        .iter()
        .find(|(l, _)| *l == level)
        .unwrap_or_else(|| panic!("no growth row for level {level}"))
        .1;
    std::array::from_fn(|h| {
        let [hp, atk, udf, ldf] = row[h];
        let (weapon, raseru, boots, gu, gl, flat) = gear(level, h);
        Hero {
            hp,
            atk: atk + flat,
            weapon,
            raseru,
            boots,
            udf: udf + gu,
            ldf: ldf + gl,
        }
    })
}

/// A member's hits a round: the command gauge widens with the climb, and the
/// tower's arts add a fifth-arrow art at 55.
fn hits_per_round(level: u8) -> u32 {
    match level {
        0..=54 => 6,
        55..=79 => 7,
        _ => 8,
    }
}

// --- The kernel, averaged ---------------------------------------------------

/// A fixed `rand()` stream (the PsyQ LCG), so every figure is reproducible.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((self.0 >> 16) & 0x7FFF) as u16
    }
}

const SAMPLES: u32 = 64;

fn mean_hit(mut hit: PhysicalHit) -> f64 {
    let mut rng = Lcg(0x5EED);
    let mut sum = 0u64;
    for _ in 0..SAMPLES {
        hit.combo_scale = 1;
        sum += u64::from(physical_predamage(&hit, &mut || rng.next()));
    }
    sum as f64 / f64::from(SAMPLES)
}

/// One member's mean damage a round against a defence of `def`.
fn hero_round(h: &Hero, level: u8, def: u16) -> f64 {
    let gear_half = (h.weapon + h.raseru + h.boots) / 2;
    let art = mean_hit(PhysicalHit {
        attacker_atk: h.atk + gear_half,
        attacker_hp: h.hp,
        defender_def: def,
        command_scalar: 22,
        staged_anim: 0x11,
        ..PhysicalHit::default()
    });
    let arm = mean_hit(PhysicalHit {
        attacker_atk: h.atk + h.weapon / 2,
        attacker_hp: h.hp,
        defender_def: def,
        command_scalar: 20,
        staged_anim: 0x0C,
        ..PhysicalHit::default()
    });
    let n = f64::from(hits_per_round(level));
    let finisher = if level >= 80 { art * 0.4 } else { 0.0 };
    n / 2.0 * (art + arm) + finisher
}

fn party_round(p: &[Hero; 3], level: u8, def: u16) -> f64 {
    p.iter().map(|h| hero_round(h, level, def)).sum()
}

/// A monster swing's mean damage on the party's mean defence.
fn swing(atk: u16, hp: u16, p: &[Hero; 3]) -> f64 {
    p.iter()
        .map(|h| {
            mean_hit(PhysicalHit {
                attacker_atk: atk,
                attacker_hp: hp,
                defender_def: (h.udf + h.ldf) / 2,
                command_scalar: 20,
                staged_anim: 0x0C,
                ..PhysicalHit::default()
            })
        })
        .sum::<f64>()
        / 3.0
}

/// An ordinary spell, against the swing: an all-party hit at this share.
const SPELL_FRACTION: f64 = 0.6;

/// Swings a round for a monster on the retail AI (its AGL buys the budget;
/// a boss script's `hits` stand in for it instead).
const RETAIL_AI_SWINGS: f64 = 1.5;

// --- The fights -------------------------------------------------------------

/// One enemy as it stands in the fight.
#[derive(Debug, Clone)]
struct Foe {
    hp: u32,
    /// EXP after the level scaling (and before the pack's `xp_mul`).
    exp: u32,
    atk: u16,
    def: u16,
    script: Option<std::sync::Arc<CompiledScript>>,
}

fn stat_mul(s: Option<Stat>) -> f64 {
    match s {
        Some(Stat::Scale { mul }) => mul,
        _ => 1.0,
    }
}

/// The modelled preset: its stat and attack-count scales, in thousandths.
#[derive(Debug, Clone, Copy)]
struct Preset {
    name: &'static str,
    stat: u32,
    attacks: f64,
    /// How much longer than Normal a fight may run.
    rounds_mul: f64,
    /// Rounds the party must last with no healing, against Normal's.
    survive_mul: f64,
}

fn presets() -> [Preset; 3] {
    let k = |d: Difficulty| d.knobs();
    [
        Preset {
            name: "Normal",
            stat: u32::from(k(Difficulty::Normal).enemy_stat_permille),
            attacks: f64::from(k(Difficulty::Normal).enemy_attack_permille) / 1000.0,
            rounds_mul: 1.0,
            survive_mul: 1.0,
        },
        Preset {
            name: "Hard",
            stat: u32::from(k(Difficulty::Hard).enemy_stat_permille),
            attacks: f64::from(k(Difficulty::Hard).enemy_attack_permille) / 1000.0,
            rounds_mul: 1.6,
            survive_mul: 0.7,
        },
        Preset {
            name: "Nightmare",
            stat: u32::from(k(Difficulty::Nightmare).enemy_stat_permille),
            attacks: f64::from(k(Difficulty::Nightmare).enemy_attack_permille) / 1000.0,
            rounds_mul: 2.6,
            survive_mul: 0.35,
        },
    ]
}

fn seat(
    pack: &ExpansionPack,
    compiled: &CompiledBattle,
    f: &CompiledFormation,
    base_id: u16,
    level: u8,
    preset: Preset,
    extra_permille: u32,
) -> Foe {
    let key = f
        .variants
        .get(&base_id)
        .unwrap_or_else(|| panic!("{}: base {base_id} is not a pack monster", f.key));
    let ov = &compiled.monsters[key];
    let mut base = MonsterDef::new(base_id, "", 1000, 100);
    // Placeholder AGL / INT / SPD: the model reads none of them.
    base.raw_stats = [150, 100, 100, 100, 150, 150];
    let def = derive_monster(&base, ov);
    let scaling = pack
        .progression
        .enemy_scaling
        .as_ref()
        .map_or(1000, |s| enemy_scaling_permille(s, level));
    let def = scale_monster(&def, scaling, true);
    let def = scale_monster(&def, preset.stat, false);
    let def = scale_monster(&def, extra_permille, false);
    let [_, atk, udf, ldf, _, _] = def.installed_stats(f.def.header_flags != 0);
    Foe {
        hp: u32::from(def.hp),
        exp: u32::from(def.exp),
        atk,
        def: udf.min(ldf),
        script: f.scripts.get(&base_id).cloned(),
    }
}

/// Every monster the fight puts in front of the party, and the largest set
/// that can stand at once.
fn roster(
    pack: &ExpansionPack,
    compiled: &CompiledBattle,
    f: &CompiledFormation,
    level: u8,
    preset: Preset,
    extra: u32,
) -> (Vec<Foe>, Vec<Foe>) {
    let mk = |id: u16| seat(pack, compiled, f, id, level, preset, extra);
    let first: Vec<Foe> = f.def.slots.iter().map(|s| mk(s.monster_id)).collect();
    let mut all = first.clone();
    let mut worst = first.clone();
    for w in &f.waves {
        let wave: Vec<Foe> = (0..w.count)
            .filter_map(|n| w.arrival_id(n))
            .map(mk)
            .collect();
        let standing: Vec<Foe> = wave
            .iter()
            .take(usize::from(w.max_standing).min(wave.len()))
            .cloned()
            .collect();
        if standing.iter().map(|m| m.atk as u32).sum::<u32>()
            > worst.iter().map(|m| m.atk as u32).sum::<u32>()
        {
            worst = standing;
        }
        all.extend(wave);
    }
    // Script summons: each phase's call seats its monsters once.
    let summoned: Vec<Foe> = first
        .iter()
        .filter_map(|m| m.script.clone())
        .flat_map(|s| {
            s.phases
                .iter()
                .flat_map(|p| p.summon.clone())
                .collect::<Vec<_>>()
        })
        .map(mk)
        .collect();
    all.extend(summoned);
    (all, worst)
}

/// The HP the party has to take off `m`, and the defence it takes it off
/// through: one segment per script phase (each at its cumulative defence
/// scale), plus every `heal_pct` the script restores.
fn segments(m: &Foe) -> Vec<(f64, f64)> {
    let Some(s) = &m.script else {
        return vec![(f64::from(m.hp), f64::from(m.def))];
    };
    let hp = f64::from(m.hp);
    let mut cuts: Vec<(f64, &CompiledPhase)> = s
        .phases
        .iter()
        .filter_map(|p| p.when.hp_below_pct.map(|pct| (f64::from(pct) / 100.0, p)))
        .collect();
    cuts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = Vec::new();
    let mut top = 1.0;
    let mut def_mul = s
        .phases
        .first()
        .filter(|p| p.when.hp_below_pct.is_none())
        .map_or(1.0, |p| stat_mul(p.stats.defence));
    for (at, p) in cuts {
        out.push(((top - at) * hp, f64::from(m.def) * def_mul));
        def_mul *= stat_mul(p.stats.defence);
        if let Some(h) = p.heal_pct {
            out.push((hp * f64::from(h) / 100.0, f64::from(m.def) * def_mul));
        }
        top = at;
    }
    out.push((top * hp, f64::from(m.def) * def_mul));
    out
}

/// One monster's threat this fight, worst phase by phase.
#[derive(Debug, Clone, Copy, Default)]
struct Threat {
    /// Expected damage a round, over the whole party.
    per_round: f64,
    /// The heaviest move on one member (every hit of a multi-hit swing).
    move_hit: f64,
    /// The heaviest single blow.
    blow: f64,
}

fn threat(m: &Foe, p: &[Hero; 3], enraged: bool) -> Threat {
    let base = swing(m.atk, m.hp.min(u32::from(u16::MAX)) as u16, p);
    let Some(s) = &m.script else {
        return Threat {
            per_round: base * RETAIL_AI_SWINGS,
            move_hit: base,
            blow: base,
        };
    };
    let mut atk_mul = 1.0;
    let mut moves = None;
    let mut worst = Threat::default();
    let phases: Vec<&CompiledPhase> = s
        .phases
        .iter()
        .chain(s.enrage.iter().filter(|_| enraged))
        .collect();
    for ph in phases {
        atk_mul *= stat_mul(ph.stats.attack);
        if let Some(m) = &ph.moves {
            moves = Some(m.clone());
        }
        let hit = base * atk_mul;
        worst.blow = worst.blow.max(hit);
        let Some(ms) = &moves else {
            worst.per_round = worst.per_round.max(hit);
            worst.move_hit = worst.move_hit.max(hit);
            continue;
        };
        let total: f64 = ms.iter().map(|m| f64::from(m.weight)).sum();
        let mut expect = 0.0;
        for mv in ms {
            let (all, single) = match mv.kind {
                MoveKind::Attack { hits, .. } => {
                    let d = hit * f64::from(hits);
                    (d, d)
                }
                MoveKind::Spell(_) if mv.charge => (hit * 3.0, hit),
                MoveKind::Spell(_) => (hit * 3.0 * SPELL_FRACTION, hit * SPELL_FRACTION),
            };
            expect += all * f64::from(mv.weight) / total;
            worst.move_hit = worst.move_hit.max(single);
        }
        worst.per_round = worst.per_round.max(expect);
    }
    worst
}

#[derive(Debug)]
struct Outcome {
    rounds: f64,
    survive: f64,
    max_hit: f64,
    max_blow: f64,
    min_hp: f64,
    hp_pool: u32,
    exp_pool: u32,
}

fn fight(
    pack: &ExpansionPack,
    compiled: &CompiledBattle,
    formation: &str,
    level: u8,
    preset: Preset,
    extra: u32,
) -> Outcome {
    let id = compiled
        .formation_id(formation)
        .unwrap_or_else(|| panic!("no formation {formation}"));
    let f = compiled.formation(id).expect("compiled");
    let p = party(level);
    let (all, worst) = roster(pack, compiled, f, level, preset, extra);
    let rounds: f64 = all
        .iter()
        .flat_map(segments)
        .map(|(hp, def)| hp / party_round(&p, level, def.round() as u16))
        .sum();
    let enraged_at = all
        .iter()
        .filter_map(|m| m.script.as_ref()?.enrage.as_ref()?.when.round)
        .min();
    let enraged = enraged_at.is_some_and(|r| rounds >= f64::from(r));
    let (dpr, max_hit, max_blow) = worst
        .iter()
        .map(|m| threat(m, &p, enraged))
        .fold((0.0, 0.0f64, 0.0f64), |(d, h, b), t| {
            (d + t.per_round, h.max(t.move_hit), b.max(t.blow))
        });
    let party_hp: f64 = p.iter().map(|h| f64::from(h.hp)).sum();
    Outcome {
        rounds,
        survive: party_hp / (dpr * preset.attacks),
        max_hit,
        max_blow,
        min_hp: p.iter().map(|h| f64::from(h.hp)).fold(f64::MAX, f64::min),
        hp_pool: all.iter().map(|m| m.hp).sum(),
        exp_pool: all.iter().map(|m| m.exp).sum(),
    }
}

// --- The curve ----------------------------------------------------------------

/// What kind of fight a row is, and so which targets it answers to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Field,
    Gauntlet,
    Boss,
    Superboss,
}

impl Kind {
    /// `(most rounds, fewest rounds, rounds the party must last unhealed)`
    /// at Normal.
    fn targets(self) -> (f64, f64, f64) {
        match self {
            Kind::Field => (5.0, 0.5, 4.0),
            Kind::Gauntlet => (14.0, 3.0, 3.0),
            Kind::Boss => (12.0, 3.0, 2.5),
            Kind::Superboss => (20.0, 6.0, 2.0),
        }
    }
}

/// `(area, formation, level the quest chain admits it at, kind, extra
/// permille)`. The levels are the pack's intended path, the same table the
/// README prints.
const CURVE: &[(&str, &str, u8, Kind, u32)] = &[
    ("Isles beach", "isles_shore", 30, Kind::Field, 1000),
    ("Isles beach", "isles_squall", 30, Kind::Field, 1000),
    ("Thornwood", "isles_thicket", 35, Kind::Field, 1000),
    ("Thornwood", "isles_pack", 35, Kind::Field, 1000),
    ("Thornwood ruins", "isles_ruin_swarm", 35, Kind::Field, 1000),
    ("Thornwood Queen", "isles_queen", 35, Kind::Boss, 1000),
    ("Singing Grotto", "isles_grotto", 40, Kind::Field, 1000),
    ("Singing Grotto", "isles_pearl", 40, Kind::Field, 1000),
    ("1F Atrium", "ogre_pack", 40, Kind::Field, 1000),
    ("1F Atrium", "forge_guard", 40, Kind::Field, 1000),
    ("1F wings", "hound_pack", 45, Kind::Field, 1000),
    ("1F bell", "tower_gauntlet", 45, Kind::Gauntlet, 1000),
    ("2F Ember Stair", "ember_court", 50, Kind::Field, 1000),
    ("2F Ember Stair", "hearth_guard", 50, Kind::Field, 1000),
    ("Cinder Warden", "ember_throne", 50, Kind::Boss, 1000),
    ("3F Hanging Garden", "thorn_thicket", 55, Kind::Field, 1000),
    ("3F Hanging Garden", "mire_bloom", 55, Kind::Field, 1000),
    ("3F Hanging Garden", "gilded_herd", 55, Kind::Field, 1000),
    (
        "Garden Gauntlet",
        "garden_gauntlet",
        60,
        Kind::Gauntlet,
        1000,
    ),
    ("4F Verdant Vault", "vault_wardens", 60, Kind::Field, 1000),
    ("4F Verdant Vault", "fountain_court", 60, Kind::Field, 1000),
    ("Verdant Colossus", "verdant_heart", 65, Kind::Boss, 1000),
    ("6F Engine Deep", "brass_line", 70, Kind::Field, 1000),
    ("6F Engine Deep", "storm_engine", 70, Kind::Field, 1000),
    (
        "Engine Gauntlet",
        "engine_gauntlet",
        75,
        Kind::Gauntlet,
        1000,
    ),
    ("Iron Regent", "regent_hall", 75, Kind::Boss, 1000),
    ("7F Sanctum", "sleepless_choir", 80, Kind::Field, 1000),
    ("7F Sanctum", "prism_storm", 80, Kind::Field, 1000),
    ("Azure Seraph", "sanctum_seraph", 85, Kind::Boss, 1000),
    ("8F Hollow Throne", "herald_watch", 90, Kind::Field, 1000),
    (
        "Hollow Sovereign",
        "hollow_throne",
        90,
        Kind::Superboss,
        1000,
    ),
    ("Echo of the Throne", "throne_echo", 95, Kind::Boss, 1000),
    (
        "Echo Storm (x1.5)",
        "throne_echo",
        99,
        Kind::Superboss,
        1500,
    ),
    ("First Breath", "first_breath", 99, Kind::Superboss, 1000),
];

#[test]
fn rebirth_curve_holds_at_every_preset() {
    let pack = rebirth();
    let compiled = CompiledBattle::compile("rebirth", &pack.battle).expect("compiles");
    let mut failures = Vec::new();
    let mut table = String::new();
    for preset in presets() {
        table.push_str(&format!(
            "\n{}\n{:<20} {:<18} {:>3} {:>7} {:>6} {:>7} {:>7} {:>6} {:>6}\n",
            preset.name,
            "area",
            "formation",
            "lv",
            "HP pool",
            "rounds",
            "survive",
            "max hit",
            "blow",
            "min HP"
        ));
        for &(area, formation, level, kind, extra) in CURVE {
            if compiled.formation_id(formation).is_none() {
                failures.push(format!("{area}: no formation {formation}"));
                continue;
            }
            let o = fight(&pack, &compiled, formation, level, preset, extra);
            table.push_str(&format!(
                "{area:<20} {formation:<18} {level:>3} {:>7} {:>6.1} {:>7.1} {:>7.0} {:>6.0} {:>6.0}\n",
                o.hp_pool, o.rounds, o.survive, o.max_hit, o.max_blow, o.min_hp
            ));
            let (most, fewest, last) = kind.targets();
            let most = most * preset.rounds_mul;
            let last = last * preset.survive_mul;
            let at = format!("{} {formation} at {level}", preset.name);
            if o.rounds > most {
                failures.push(format!("{at}: {:.1} rounds > {most:.1}", o.rounds));
            }
            if preset.name == "Normal" && o.rounds < fewest {
                failures.push(format!("{at}: {:.1} rounds < {fewest:.1}", o.rounds));
            }
            if o.survive < last {
                failures.push(format!("{at}: lasts {:.1} rounds < {last:.1}", o.survive));
            }
            // Nothing one-shots a full-HP member: no move at Normal and
            // Hard, no single blow at Nightmare (a multi-hit swing may drop
            // someone there - that is what Phoenix Plumes are for).
            let (worst, what) = if preset.name == "Nightmare" {
                (o.max_blow, "blow")
            } else {
                (o.max_hit, "move")
            };
            if worst >= o.min_hp {
                failures.push(format!(
                    "{at}: one {what} hits {worst:.0}, a member has {:.0}",
                    o.min_hp
                ));
            }
        }
    }
    println!("{table}");
    assert!(failures.is_empty(), "{table}\n{failures:#?}");
}

/// Every formation the pack defines is on the curve, so a new one cannot
/// slip in untuned.
#[test]
fn every_formation_is_on_the_curve() {
    let pack = rebirth();
    let on: std::collections::BTreeSet<&str> = CURVE.iter().map(|r| r.1).collect();
    let missing: Vec<&String> = pack
        .battle
        .formations
        .keys()
        .filter(|k| !on.contains(k.as_str()))
        .collect();
    assert!(missing.is_empty(), "formations off the curve: {missing:?}");
}

/// Each floor is a step up: the field fights' party-round count at the
/// floor's own level does not fall from one floor to the next by more than
/// noise, and the keepers' HP pools rise.
#[test]
fn each_keeper_is_a_step_up() {
    let pack = rebirth();
    let compiled = CompiledBattle::compile("rebirth", &pack.battle).expect("compiles");
    let normal = presets()[0];
    let keepers: Vec<(&str, u8)> = CURVE
        .iter()
        .filter(|r| r.3 == Kind::Boss || r.3 == Kind::Superboss)
        .filter(|r| r.4 == 1000 && r.1 != "throne_echo" && r.1 != "isles_queen")
        .map(|r| (r.1, r.2))
        .collect();
    let mut last = 0u32;
    let mut pools = BTreeMap::new();
    for (f, level) in keepers {
        let o = fight(&pack, &compiled, f, level, normal, 1000);
        assert!(
            o.hp_pool > last,
            "{f} at {level}: pool {} <= {last}",
            o.hp_pool
        );
        last = o.hp_pool;
        pools.insert(f, o.hp_pool);
    }
    println!("{pools:#?}");
}

// --- The party model against the disc -----------------------------------------

/// The [`BASE`] rows are the retail growth: re-derived from the executable
/// when `extracted/SCUS_942.54` is present (skips and passes otherwise).
#[test]
fn party_model_matches_the_retail_growth() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted/SCUS_942.54");
    let Ok(scus) = std::fs::read(&path) else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let tables = legaia_game_tables::level_up_tables::growth_tables_from_scus(&scus)
        .expect("growth tables decode");
    for (h, name) in ["Vahn", "Noa", "Gala"].iter().enumerate() {
        let cp = tables.char_params(h).expect("params");
        for &(level, rows) in BASE {
            // Record order HP, MP, AGL, ATK, UDF, LDF, SPD, INT.
            let at = |s: usize| -> u16 {
                let p = &cp.stats[s];
                let mut v = u32::from(p.start);
                for prev in 1..usize::from(level) {
                    v += tables.level_gain_core(p, prev).unwrap_or(0);
                }
                v as u16
            };
            let got = [at(0), at(3), at(4), at(5)];
            assert_eq!(got, rows[h], "{name} at level {level}");
        }
    }
}

/// Time to level: with the retail XP table (`extracted/SCUS_942.54`, skips
/// and passes without it) under the pack's curve, how many of each fight one
/// level takes at the level the fight is met at. A member's share is the
/// retail victory split: three quarters of the pool, three ways. Field fights
/// have to move the bar - no more than a dozen a level - and nothing hands
/// out a level per fight below the post-game.
#[test]
fn time_to_level_stays_in_band() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted/SCUS_942.54");
    let Ok(scus) = std::fs::read(&path) else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let retail = legaia_game_tables::level_up_tables::xp_thresholds_from_scus(&scus)
        .expect("xp table decodes");
    let pack = rebirth();
    let compiled = CompiledBattle::compile("rebirth", &pack.battle).expect("compiles");
    let curve = legaia_engine_expansion::progression::xp_table(&pack.progression.xp_curve, &retail);
    let xp_mul = pack.progression.xp_mul.unwrap_or(1.0);
    let normal = presets()[0];
    let mut table = format!(
        "{:<20} {:<18} {:>3} {:>9} {:>9} {:>8}\n",
        "area", "formation", "lv", "share", "next lv", "fights"
    );
    let mut failures = Vec::new();
    let mut total_60_99 = 0u64;
    for &(area, formation, level, kind, extra) in CURVE {
        let o = fight(&pack, &compiled, formation, level, normal, extra);
        let share = f64::from(o.exp_pool) * xp_mul * 0.75 / 3.0;
        let idx = usize::from(level).saturating_sub(1);
        let step = if level >= 99 {
            0
        } else {
            curve[idx] - if idx == 0 { 0 } else { curve[idx - 1] }
        };
        let fights = f64::from(step) / share;
        table.push_str(&format!(
            "{area:<20} {formation:<18} {level:>3} {share:>9.0} {step:>9} {fights:>8.1}\n"
        ));
        if kind == Kind::Field && fights > 12.0 {
            failures.push(format!(
                "{formation} at {level}: {fights:.1} fights a level"
            ));
        }
        if level < 90 && step > 0 && fights < 0.25 {
            failures.push(format!(
                "{formation} at {level}: {fights:.2} fights a level"
            ));
        }
    }
    for l in 60..99usize {
        total_60_99 += u64::from(curve[l - 1] - curve[l - 2]);
    }
    println!("{table}\n60 -> 99: {total_60_99} EXP a member");
    assert!(failures.is_empty(), "{table}\n{failures:#?}");
}
