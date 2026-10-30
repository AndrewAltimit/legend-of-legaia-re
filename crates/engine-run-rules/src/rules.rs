//! The rules one run plays under, chosen at New Game and carried in its
//! saves.
//!
//! [`RunRules::default`] is the retail game: every knob at its identity, no
//! New Game+, no ironman, no randomizer, the story mode. Each reader in the
//! engine folds the rules in as a multiplier or a gate that is the identity
//! at the default, so a run with no rules plays exactly as retail does.

use serde::{Deserialize, Serialize};

use crate::scaling;

/// A difficulty preset: a bundle of existing scales plus the rule knobs
/// below. [`Difficulty::Normal`] is retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
    Nightmare,
}

impl Difficulty {
    /// Every preset, in the order a host cycles them.
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Nightmare,
    ];

    /// The preset as a player reads it.
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Nightmare => "Nightmare",
        }
    }

    /// The knob bundle the preset stands for.
    pub fn knobs(self) -> Knobs {
        match self {
            Difficulty::Easy => Knobs {
                enemy_stat_permille: 750,
                enemy_attack_permille: 750,
                exp_permille: 1500,
                gold_permille: 1500,
                encounter_rate_permille: 600,
                price_permille: 800,
                heal_permille: 1500,
                ap_gain_permille: 1250,
                smart_monsters: false,
                no_escape: false,
            },
            Difficulty::Normal => Knobs::RETAIL,
            Difficulty::Hard => Knobs {
                enemy_stat_permille: 1350,
                enemy_attack_permille: 1250,
                exp_permille: 1000,
                gold_permille: 1000,
                encounter_rate_permille: 1250,
                price_permille: 1250,
                heal_permille: 750,
                ap_gain_permille: 1000,
                smart_monsters: true,
                no_escape: false,
            },
            Difficulty::Nightmare => Knobs {
                enemy_stat_permille: 1800,
                enemy_attack_permille: 1500,
                exp_permille: 900,
                gold_permille: 800,
                encounter_rate_permille: 1500,
                price_permille: 1500,
                heal_permille: 500,
                ap_gain_permille: 750,
                smart_monsters: true,
                no_escape: true,
            },
        }
    }

    /// The next (or, `dir < 0`, previous) preset.
    pub fn step(self, dir: i8) -> Self {
        let i = Self::ALL.iter().position(|&d| d == self).unwrap_or(1);
        let n = Self::ALL.len();
        Self::ALL[if dir < 0 {
            (i + n - 1) % n
        } else {
            (i + 1) % n
        }]
    }
}

/// The scales and gates a difficulty preset sets. Every permille is `1000`
/// at retail, every gate `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Knobs {
    /// Each monster's HP / MP / ATK / UDF / LDF / INT / SPD.
    pub enemy_stat_permille: u16,
    /// How many swings a monster's AGL buys (the attack-count scale).
    pub enemy_attack_permille: u16,
    /// Each monster's EXP reward.
    pub exp_permille: u16,
    /// A victory's gold.
    pub gold_permille: u16,
    /// The field encounter rate: each step's region increment.
    pub encounter_rate_permille: u16,
    /// Shop buy prices.
    pub price_permille: u16,
    /// What a healing item restores.
    pub heal_permille: u16,
    /// The AP a Spirit turn and a hit taken charge.
    pub ap_gain_permille: u16,
    /// A monster's single-target attack picks the weakest living member.
    pub smart_monsters: bool,
    /// The party's Run command never succeeds.
    pub no_escape: bool,
}

impl Knobs {
    /// Retail: every scale `1000`, every gate off.
    pub const RETAIL: Knobs = Knobs {
        enemy_stat_permille: 1000,
        enemy_attack_permille: 1000,
        exp_permille: 1000,
        gold_permille: 1000,
        encounter_rate_permille: 1000,
        price_permille: 1000,
        heal_permille: 1000,
        ap_gain_permille: 1000,
        smart_monsters: false,
        no_escape: false,
    };
}

/// What a New Game+ keeps from the save it starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrySet {
    /// Levels, EXP and the stats the levels grew.
    pub levels: bool,
    /// Every member's equipment.
    pub equipment: bool,
    /// The bag.
    pub items: bool,
    /// Learned Seru magic and its levels.
    pub seru: bool,
    /// Learned arts.
    pub arts: bool,
    /// The purse.
    pub gold: bool,
    /// The trading-card binder.
    pub cards: bool,
}

impl Default for CarrySet {
    /// Everything carries.
    fn default() -> Self {
        Self {
            levels: true,
            equipment: true,
            items: true,
            seru: true,
            arts: true,
            gold: true,
            cards: true,
        }
    }
}

impl CarrySet {
    /// Each part as `(label, value)`, in the order a host lists them.
    pub fn rows(&self) -> [(&'static str, bool); 7] {
        [
            ("Levels", self.levels),
            ("Equipment", self.equipment),
            ("Items", self.items),
            ("Seru magic", self.seru),
            ("Arts", self.arts),
            ("Gold", self.gold),
            ("Cards", self.cards),
        ]
    }

    /// Flip part `i` of [`Self::rows`].
    pub fn toggle(&mut self, i: usize) {
        let f = match i {
            0 => &mut self.levels,
            1 => &mut self.equipment,
            2 => &mut self.items,
            3 => &mut self.seru,
            4 => &mut self.arts,
            5 => &mut self.gold,
            _ => &mut self.cards,
        };
        *f = !*f;
    }
}

/// New Game+: the cycle the run is on (`1` for the first replay) and what
/// it kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NgPlus {
    pub cycle: u8,
    #[serde(default)]
    pub carry: CarrySet,
}

/// Ironman: one save per run, the run's save goes when the party falls, an
/// autosave at each scene entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ironman {
    /// No items in battle.
    #[serde(default)]
    pub hardcore: bool,
}

/// What the in-engine randomizer reassigns, and the seed it does it from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Randomizer {
    /// The seed as the player typed it; [`Self::seed_value`] is its number.
    pub seed: String,
    #[serde(default = "yes")]
    pub encounters: bool,
    #[serde(default = "yes")]
    pub drops: bool,
    #[serde(default = "yes")]
    pub steals: bool,
    #[serde(default = "yes")]
    pub shops: bool,
    #[serde(default = "yes")]
    pub chests: bool,
    #[serde(default = "yes")]
    pub monster_stats: bool,
    #[serde(default = "yes")]
    pub starting_equipment: bool,
}

fn yes() -> bool {
    true
}

impl Randomizer {
    /// Every part on, from `seed`.
    pub fn all(seed: impl Into<String>) -> Self {
        Self {
            seed: seed.into(),
            encounters: true,
            drops: true,
            steals: true,
            shops: true,
            chests: true,
            monster_stats: true,
            starting_equipment: true,
        }
    }

    /// The seed's number: the patcher's own string-to-seed rule, so a seed
    /// string names the same stream in both.
    pub fn seed_value(&self) -> u64 {
        legaia_randomizer::rng::seed_from_str(&self.seed)
    }
}

/// The game mode a run plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// The story, from the opening.
    #[default]
    Story,
    /// Every story boss back to back ([`crate::boss_rush`]).
    BossRush,
}

/// The rules of one run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunRules {
    #[serde(default)]
    pub mode: RunMode,
    #[serde(default)]
    pub difficulty: Difficulty,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ng_plus: Option<NgPlus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ironman: Option<Ironman>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomizer: Option<Randomizer>,
}

/// Round-half-up permille product, the identity when `b == 1000`.
pub fn mul_permille(a: u32, b: u16) -> u32 {
    if b == 1000 {
        return a;
    }
    ((u64::from(a) * u64::from(b) + 500) / 1000).min(u64::from(u32::MAX)) as u32
}

impl RunRules {
    /// Whether these are the retail rules (nothing to carry, nothing to
    /// save).
    pub fn is_retail(&self) -> bool {
        *self == Self::default()
    }

    /// The knob bundle in force.
    pub fn knobs(&self) -> Knobs {
        self.difficulty.knobs()
    }

    /// The New Game+ cycle, `0` outside New Game+.
    pub fn ng_plus_cycle(&self) -> u8 {
        self.ng_plus.map_or(0, |n| n.cycle)
    }

    /// The enemy stat scale for monster `monster_id` with the party's
    /// strongest member at `party_level`: the preset's scale times New
    /// Game+'s curve ([`scaling::ng_plus_permille`]).
    pub fn enemy_stat_permille(&self, monster_id: u16, party_level: u8) -> u16 {
        let base = u32::from(self.knobs().enemy_stat_permille);
        let ng = scaling::ng_plus_permille(self.ng_plus_cycle(), party_level, monster_id);
        mul_permille(base, ng).clamp(100, 10_000) as u16
    }

    /// One line a status screen shows for the run, `None` for retail rules.
    pub fn summary(&self) -> Option<String> {
        if self.is_retail() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        if self.mode == RunMode::BossRush {
            parts.push("Boss Rush".into());
        }
        parts.push(self.difficulty.label().into());
        if let Some(n) = self.ng_plus {
            parts.push(format!("NG+{}", n.cycle));
        }
        if let Some(i) = self.ironman {
            parts.push(if i.hardcore { "Hardcore" } else { "Ironman" }.into());
        }
        if let Some(r) = &self.randomizer {
            parts.push(format!("Seed {}", r.seed));
        }
        Some(parts.join(" / "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_rules_are_the_identity() {
        let r = RunRules::default();
        assert!(r.is_retail());
        assert_eq!(r.knobs(), Knobs::RETAIL);
        assert_eq!(Difficulty::Normal.knobs(), Knobs::RETAIL);
        assert_eq!(r.summary(), None);
        for id in [1u16, 73, 79, 180] {
            for lvl in [1u8, 50, 99] {
                assert_eq!(r.enemy_stat_permille(id, lvl), 1000);
            }
        }
        assert_eq!(mul_permille(12345, 1000), 12345);
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"mode":"story","difficulty":"normal"}"#
        );
    }

    #[test]
    fn presets_order_and_step() {
        assert!(Difficulty::Easy.knobs().enemy_stat_permille < 1000);
        assert!(Difficulty::Hard.knobs().enemy_stat_permille > 1000);
        let n = Difficulty::Nightmare.knobs();
        assert!(n.no_escape && n.smart_monsters && n.ap_gain_permille < 1000);
        assert_eq!(Difficulty::Normal.step(1), Difficulty::Hard);
        assert_eq!(Difficulty::Easy.step(-1), Difficulty::Nightmare);
    }

    #[test]
    fn rules_round_trip_and_summarise() {
        let r = RunRules {
            mode: RunMode::Story,
            difficulty: Difficulty::Hard,
            ng_plus: Some(NgPlus {
                cycle: 2,
                carry: CarrySet {
                    gold: false,
                    ..Default::default()
                },
            }),
            ironman: Some(Ironman { hardcore: true }),
            randomizer: Some(Randomizer::all("legaia")),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<RunRules>(&json).unwrap(), r);
        assert_eq!(r.summary().unwrap(), "Hard / NG+2 / Hardcore / Seed legaia");
        assert_eq!(
            Randomizer::all("42").seed_value(),
            legaia_randomizer::rng::seed_from_str("42")
        );
        let mut c = CarrySet::default();
        c.toggle(5);
        assert!(!c.gold && c.rows()[5] == ("Gold", false));
    }
}
