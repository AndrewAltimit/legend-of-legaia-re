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
/// below. [`Difficulty::Normal`] is retail; [`Difficulty::Custom`] takes its
/// knobs from [`RunRules::custom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
    Nightmare,
    /// The player's own knobs ([`RunRules::custom`]).
    Custom,
}

impl Difficulty {
    /// Every preset, in the order a host cycles them.
    pub const ALL: [Difficulty; 5] = [
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Nightmare,
        Difficulty::Custom,
    ];

    /// The preset as a player reads it.
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Nightmare => "Nightmare",
            Difficulty::Custom => "Custom",
        }
    }

    /// One line saying what the preset changes, for a rules screen's foot.
    pub fn blurb(self) -> &'static str {
        match self {
            Difficulty::Easy => "Weaker foes, fewer fights, more EXP and gold.",
            Difficulty::Normal => "The original game's balance.",
            Difficulty::Hard => "Tougher, smarter foes; dear shops, weak heals.",
            Difficulty::Nightmare => "Brutal foes, no escape, half-strength heals.",
            Difficulty::Custom => "Your own scales: Left/Right sets each one below.",
        }
    }

    /// The knob bundle the preset stands for ([`Difficulty::Custom`]'s own
    /// is [`RunRules::custom`]; here it reads as retail).
    pub fn knobs(self) -> Knobs {
        match self {
            Difficulty::Custom => Knobs::RETAIL,
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
/// at retail, every gate `false`. A custom run saves its own (a missing
/// field reads as retail's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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

/// A permille as a multiplier a player reads: `x1`, `x0.75`, `x1.35`.
pub fn permille_label(p: u16) -> String {
    let whole = p / 1000;
    let frac = p % 1000;
    if frac == 0 {
        return format!("x{whole}");
    }
    let mut s = format!("x{whole}.{frac:03}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

impl Default for Knobs {
    fn default() -> Self {
        Self::RETAIL
    }
}

/// How far one Left / Right press moves a permille knob: `x0.05` up to
/// `x2`, `x0.25` past it.
fn permille_step(v: u16, dir: i8) -> u16 {
    let up = dir > 0;
    let step = if v > 2000 || (up && v >= 2000) {
        250
    } else {
        50
    };
    if up {
        v.saturating_add(step)
    } else {
        v.saturating_sub(step)
    }
}

impl Knobs {
    /// How many knobs [`Self::rows`] lists.
    pub const COUNT: usize = 10;

    /// Each permille knob's `(least, most)`. Encounters may go to `x0` (no
    /// random fights); every other scale keeps at least `x0.1`.
    pub const BOUNDS: [(u16, u16); 8] = [
        (250, 5000),
        (250, 3000),
        (100, 5000),
        (100, 5000),
        (0, 3000),
        (100, 3000),
        (100, 3000),
        (100, 3000),
    ];

    /// Step knob `i` of [`Self::rows`] one notch (`dir` `< 0` down): a
    /// scale by [`permille_step`] inside [`Self::BOUNDS`], a gate flipped.
    pub fn step(&mut self, i: usize, dir: i8) {
        let field = match i {
            0 => &mut self.enemy_stat_permille,
            1 => &mut self.enemy_attack_permille,
            2 => &mut self.exp_permille,
            3 => &mut self.gold_permille,
            4 => &mut self.encounter_rate_permille,
            5 => &mut self.price_permille,
            6 => &mut self.heal_permille,
            7 => &mut self.ap_gain_permille,
            8 => {
                self.smart_monsters = !self.smart_monsters;
                return;
            }
            _ => {
                self.no_escape = !self.no_escape;
                return;
            }
        };
        let (lo, hi) = Self::BOUNDS[i];
        *field = permille_step(*field, dir).clamp(lo, hi);
    }

    /// What knob `i` of [`Self::rows`] scales, for a help line.
    pub fn help(i: usize) -> &'static str {
        match i {
            0 => "Each monster's HP, MP and battle stats.",
            1 => "How many swings a monster's agility buys.",
            2 => "The EXP each monster pays.",
            3 => "The gold a victory pays.",
            4 => "How fast walking fills the random-encounter gauge.",
            5 => "What shops charge.",
            6 => "What a healing item restores.",
            7 => "The AP a Spirit turn and a hit taken charge.",
            8 => "A monster's single-target attack picks the weakest member.",
            _ => "The party's Run command never works.",
        }
    }

    /// Each knob as `(label, value)` text, in the order a rules screen
    /// lists them.
    pub fn rows(&self) -> [(&'static str, String); 10] {
        let gate = |on: bool| if on { "Yes" } else { "No" }.to_string();
        [
            ("Foe stats", permille_label(self.enemy_stat_permille)),
            ("Foe attacks", permille_label(self.enemy_attack_permille)),
            ("EXP", permille_label(self.exp_permille)),
            ("Gold", permille_label(self.gold_permille)),
            ("Encounters", permille_label(self.encounter_rate_permille)),
            ("Shop prices", permille_label(self.price_permille)),
            ("Healing", permille_label(self.heal_permille)),
            ("AP gain", permille_label(self.ap_gain_permille)),
            ("Smart foes", gate(self.smart_monsters)),
            ("No escape", gate(self.no_escape)),
        ]
    }

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

    /// Each part as `(label, on)`, in the order a host lists them.
    pub fn parts(&self) -> [(&'static str, bool); 7] {
        [
            ("Encounters", self.encounters),
            ("Drops", self.drops),
            ("Steals", self.steals),
            ("Shops", self.shops),
            ("Chests and gifts", self.chests),
            ("Monster stats", self.monster_stats),
            ("Starting gear", self.starting_equipment),
        ]
    }

    /// Flip part `i` of [`Self::parts`].
    pub fn toggle_part(&mut self, i: usize) {
        let f = match i {
            0 => &mut self.encounters,
            1 => &mut self.drops,
            2 => &mut self.steals,
            3 => &mut self.shops,
            4 => &mut self.chests,
            5 => &mut self.monster_stats,
            _ => &mut self.starting_equipment,
        };
        *f = !*f;
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
    /// [`Difficulty::Custom`]'s knobs; `None` under every preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<Knobs>,
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
        match self.difficulty {
            Difficulty::Custom => self.custom.unwrap_or(Knobs::RETAIL),
            d => d.knobs(),
        }
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
        assert_eq!(Difficulty::Easy.step(-1), Difficulty::Custom);
        assert_eq!(Difficulty::Custom.step(1), Difficulty::Easy);
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
            custom: None,
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
        let mut r = Randomizer::all("x");
        r.toggle_part(4);
        assert!(!r.chests && r.parts()[4] == ("Chests and gifts", false));
    }

    #[test]
    fn custom_knobs_step_inside_their_bounds_and_save() {
        let mut k = Knobs::RETAIL;
        k.step(0, 1);
        assert_eq!(k.enemy_stat_permille, 1050);
        for _ in 0..200 {
            k.step(0, 1);
        }
        assert_eq!(k.enemy_stat_permille, 5000, "held to the bound");
        for _ in 0..200 {
            k.step(4, -1);
        }
        assert_eq!(k.encounter_rate_permille, 0, "encounters may stop");
        k.step(9, 1);
        assert!(k.no_escape);
        // Past x2 a press moves x0.25; under it, x0.05.
        let mut j = Knobs::RETAIL;
        j.exp_permille = 2000;
        j.step(2, 1);
        assert_eq!(j.exp_permille, 2250);
        j.step(2, -1);
        assert_eq!(j.exp_permille, 2000);
        j.step(2, -1);
        assert_eq!(j.exp_permille, 1950);
        let r = RunRules {
            difficulty: Difficulty::Custom,
            custom: Some(k),
            ..Default::default()
        };
        assert_eq!(r.knobs(), k);
        assert_eq!(r.summary().unwrap(), "Custom");
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<RunRules>(&json).unwrap(), r);
        // A custom block missing a knob reads it as retail's.
        let partial: Knobs = serde_json::from_str(r#"{"exp_permille":1500}"#).unwrap();
        assert_eq!(partial.exp_permille, 1500);
        assert_eq!(partial.enemy_stat_permille, 1000);
        // A preset's rules never carry custom knobs.
        assert_eq!(
            serde_json::to_string(&RunRules::default()).unwrap(),
            r#"{"mode":"story","difficulty":"normal"}"#
        );
    }

    #[test]
    fn knob_rows_read_as_multipliers() {
        assert_eq!(permille_label(1000), "x1");
        assert_eq!(permille_label(750), "x0.75");
        assert_eq!(permille_label(1350), "x1.35");
        assert_eq!(permille_label(1500), "x1.5");
        let rows = Difficulty::Nightmare.knobs().rows();
        assert_eq!(rows[0], ("Foe stats", "x1.8".to_string()));
        assert_eq!(rows[9], ("No escape", "Yes".to_string()));
    }

    /// A save written before the randomizer's parts or the carry set
    /// existed (a block with only a seed, or no New Game+ carry) still
    /// decodes, every missing part on.
    #[test]
    fn an_older_rules_block_decodes_with_every_part_on() {
        let old = r#"{"mode":"story","difficulty":"hard","ng_plus":{"cycle":1},"randomizer":{"seed":"RIM-0042"}}"#;
        let r: RunRules = serde_json::from_str(old).unwrap();
        assert_eq!(r.randomizer, Some(Randomizer::all("RIM-0042")));
        assert_eq!(r.ng_plus.unwrap().carry, CarrySet::default());
        assert_eq!(r.difficulty, Difficulty::Hard);
        // And the block a save carries round-trips through the save body.
        let save = crate::save::RunSave {
            rules: r.clone(),
            ..Default::default()
        };
        let back = crate::save::RunSave::decode(&save.encode().unwrap()).unwrap();
        assert_eq!(back.rules, r);
    }
}
