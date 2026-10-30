//! The New Game rules screen's session: the rows a player sets a run's rules
//! on between the title's NEW GAME and the opening scene.
//!
//! The session is pad-driven and host-free: both play hosts feed it the same
//! edges and draw its [`RulesSetup::rows`] through the shared draw builder,
//! so the screen is one screen on both. Its first state is the retail rules
//! with the cursor on *Start*, so one confirm starts the retail game.

use crate::rules::{Ironman, NgPlus, Randomizer, RunMode, RunRules};

/// The pad edges the screen reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetupInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub cancel: bool,
    /// Start: begin the run with the rules as they stand, from any row.
    pub start: bool,
}

/// What a step of the screen came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupOutcome {
    /// Still open.
    Open,
    /// Start a run under these rules; `ng_source` is the New Game+ source
    /// (index into the sources the screen was given) when New Game+ is on.
    Start {
        rules: RunRules,
        ng_source: Option<usize>,
    },
    /// Back to the title.
    Cancelled,
}

/// One row of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRow {
    Mode,
    Difficulty,
    NgPlus,
    Carry(usize),
    Ironman,
    Randomizer,
    Seed,
    Start,
}

/// The New Game rules screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesSetup {
    pub rules: RunRules,
    /// Labels of the saves a New Game+ can start from (`"Save 1  Vahn Lv 40"`).
    pub ng_sources: Vec<String>,
    /// The cycle each source is on (its own New Game+ cycle).
    pub ng_source_cycles: Vec<u8>,
    /// The chosen source, while New Game+ is on.
    pub ng_source: Option<usize>,
    pub cursor: usize,
    /// Entropy for the next seed the *Seed* row rolls.
    pub entropy: u64,
}

const SEED_WORDS: &[&str] = &[
    "RIM",
    "ELM",
    "SERU",
    "MIST",
    "GENESIS",
    "VAHN",
    "NOA",
    "GALA",
    "DRAKE",
    "SEBUCUS",
    "KARISTO",
    "JUGGERNAUT",
    "MEIA",
    "TERRA",
    "ZORA",
    "SONGI",
    "GAZA",
    "CARA",
];

/// A fresh, readable seed string from `entropy`: `WORD-NNNN`.
pub fn seed_string(entropy: u64) -> String {
    let mut r = legaia_randomizer::rng::SplitMix64::new(entropy);
    let w = SEED_WORDS[r.below(SEED_WORDS.len())];
    format!("{w}-{:04}", r.below(10_000))
}

impl RulesSetup {
    /// The screen over the given New Game+ sources (label, cycle), at the
    /// retail rules with the cursor on *Start*.
    pub fn new(ng_sources: Vec<(String, u8)>, entropy: u64) -> Self {
        let (labels, cycles) = ng_sources.into_iter().unzip();
        let mut s = Self {
            rules: RunRules::default(),
            ng_sources: labels,
            ng_source_cycles: cycles,
            ng_source: None,
            cursor: 0,
            entropy,
        };
        s.cursor = s
            .row_list()
            .iter()
            .position(|&r| r == SetupRow::Start)
            .unwrap_or(0);
        s
    }

    /// The rows on screen now: the carry rows show while New Game+ is on, the
    /// seed row while the randomizer is.
    pub fn row_list(&self) -> Vec<SetupRow> {
        let mut rows = vec![SetupRow::Mode, SetupRow::Difficulty, SetupRow::NgPlus];
        if self.rules.ng_plus.is_some() {
            rows.extend((0..7).map(SetupRow::Carry));
        }
        rows.extend([SetupRow::Ironman, SetupRow::Randomizer]);
        if self.rules.randomizer.is_some() {
            rows.push(SetupRow::Seed);
        }
        rows.push(SetupRow::Start);
        rows
    }

    /// Each row as `(label, value)` text, in [`Self::row_list`] order.
    pub fn rows(&self) -> Vec<(String, String)> {
        self.row_list()
            .into_iter()
            .map(|row| match row {
                SetupRow::Mode => (
                    "Mode".into(),
                    match self.rules.mode {
                        RunMode::Story => "Story",
                        RunMode::BossRush => "Boss Rush",
                    }
                    .into(),
                ),
                SetupRow::Difficulty => ("Difficulty".into(), self.rules.difficulty.label().into()),
                SetupRow::NgPlus => (
                    "New Game+".into(),
                    match self.ng_source {
                        Some(i) if self.rules.ng_plus.is_some() => self
                            .ng_sources
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| "?".into()),
                        _ if self.ng_sources.is_empty() => "Off (no save)".into(),
                        _ => "Off".into(),
                    },
                ),
                SetupRow::Carry(i) => {
                    let (label, on) = self
                        .rules
                        .ng_plus
                        .unwrap_or_else(|| NgPlus {
                            cycle: 1,
                            carry: Default::default(),
                        })
                        .carry
                        .rows()[i];
                    (
                        format!("  Keep {label}"),
                        if on { "Yes" } else { "No" }.into(),
                    )
                }
                SetupRow::Ironman => (
                    "Ironman".into(),
                    match self.rules.ironman {
                        None => "Off",
                        Some(Ironman { hardcore: false }) => "On",
                        Some(Ironman { hardcore: true }) => "Hardcore",
                    }
                    .into(),
                ),
                SetupRow::Randomizer => (
                    "Randomizer".into(),
                    if self.rules.randomizer.is_some() {
                        "On"
                    } else {
                        "Off"
                    }
                    .into(),
                ),
                SetupRow::Seed => (
                    "  Seed".into(),
                    self.rules
                        .randomizer
                        .as_ref()
                        .map_or(String::new(), |r| r.seed.clone()),
                ),
                SetupRow::Start => ("Start".into(), String::new()),
            })
            .collect()
    }

    /// The row under the cursor.
    pub fn selected(&self) -> SetupRow {
        let rows = self.row_list();
        rows[self.cursor.min(rows.len() - 1)]
    }

    /// A line describing the selected row, for the screen's foot.
    pub fn help(&self) -> &'static str {
        match self.selected() {
            SetupRow::Mode => "Story, or every boss back to back.",
            SetupRow::Difficulty => "Normal is the original game.",
            SetupRow::NgPlus => "Replay a save's party; foes scale up.",
            SetupRow::Carry(_) => "What the new run keeps.",
            SetupRow::Ironman => "One save, lost on a wipe. Hardcore: no items.",
            SetupRow::Randomizer => "Fights, loot, shops, stats from a seed.",
            SetupRow::Seed => "Confirm rolls a new seed.",
            SetupRow::Start => "Begin the run.",
        }
    }

    fn roll_seed(&mut self) -> String {
        self.entropy = self.entropy.wrapping_add(0x9E37_79B9_7F4A_7C15);
        seed_string(self.entropy)
    }

    fn change(&mut self, row: SetupRow, dir: i8) {
        match row {
            SetupRow::Mode => {
                self.rules.mode = match self.rules.mode {
                    RunMode::Story => RunMode::BossRush,
                    RunMode::BossRush => RunMode::Story,
                }
            }
            SetupRow::Difficulty => self.rules.difficulty = self.rules.difficulty.step(dir),
            SetupRow::NgPlus => {
                let n = self.ng_sources.len();
                if n == 0 {
                    return;
                }
                // Off, then each source, cycling.
                let cur = self.ng_source.map_or(0, |i| i + 1);
                let next = if dir < 0 {
                    (cur + n) % (n + 1)
                } else {
                    (cur + 1) % (n + 1)
                };
                self.ng_source = next.checked_sub(1);
                self.rules.ng_plus = self.ng_source.map(|i| NgPlus {
                    cycle: self
                        .ng_source_cycles
                        .get(i)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(1),
                    carry: self.rules.ng_plus.map(|n| n.carry).unwrap_or_default(),
                });
            }
            SetupRow::Carry(i) => {
                if let Some(n) = self.rules.ng_plus.as_mut() {
                    n.carry.toggle(i);
                }
            }
            SetupRow::Ironman => {
                self.rules.ironman = match (self.rules.ironman, dir < 0) {
                    (None, false) => Some(Ironman { hardcore: false }),
                    (Some(Ironman { hardcore: false }), false) => Some(Ironman { hardcore: true }),
                    (Some(Ironman { hardcore: true }), false) => None,
                    (None, true) => Some(Ironman { hardcore: true }),
                    (Some(Ironman { hardcore: true }), true) => Some(Ironman { hardcore: false }),
                    (Some(Ironman { hardcore: false }), true) => None,
                }
            }
            SetupRow::Randomizer => {
                self.rules.randomizer = match self.rules.randomizer.take() {
                    Some(_) => None,
                    None => Some(Randomizer::all(self.roll_seed())),
                }
            }
            SetupRow::Seed => {
                let seed = self.roll_seed();
                if let Some(r) = self.rules.randomizer.as_mut() {
                    r.seed = seed;
                }
            }
            SetupRow::Start => {}
        }
    }

    /// Set the randomizer seed to `seed` (a host with a keyboard types one),
    /// turning the randomizer on.
    pub fn set_seed(&mut self, seed: &str) {
        let seed = seed.trim();
        if seed.is_empty() {
            return;
        }
        match self.rules.randomizer.as_mut() {
            Some(r) => r.seed = seed.to_string(),
            None => self.rules.randomizer = Some(Randomizer::all(seed)),
        }
    }

    /// One frame of input.
    pub fn step(&mut self, input: SetupInput) -> SetupOutcome {
        if input.cancel {
            return SetupOutcome::Cancelled;
        }
        if input.start {
            return SetupOutcome::Start {
                rules: self.rules.clone(),
                ng_source: self.ng_source.filter(|_| self.rules.ng_plus.is_some()),
            };
        }
        let n = self.row_list().len();
        if input.up {
            self.cursor = (self.cursor + n - 1) % n;
        } else if input.down {
            self.cursor = (self.cursor + 1) % n;
        }
        let row = self.selected();
        if input.left || input.right {
            self.change(row, if input.left { -1 } else { 1 });
        } else if input.confirm {
            if row == SetupRow::Start {
                return SetupOutcome::Start {
                    rules: self.rules.clone(),
                    ng_source: self.ng_source.filter(|_| self.rules.ng_plus.is_some()),
                };
            }
            self.change(row, 1);
        }
        // Rows come and go with New Game+ and the randomizer; keep the
        // cursor on the row it was on.
        let rows = self.row_list();
        self.cursor = rows
            .iter()
            .position(|&r| r == row)
            .unwrap_or(self.cursor.min(rows.len() - 1));
        SetupOutcome::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Difficulty;

    fn press(s: &mut RulesSetup, f: impl Fn(&mut SetupInput)) -> SetupOutcome {
        let mut i = SetupInput::default();
        f(&mut i);
        s.step(i)
    }

    #[test]
    fn one_confirm_starts_the_retail_game() {
        let mut s = RulesSetup::new(vec![], 1);
        assert_eq!(s.selected(), SetupRow::Start);
        assert_eq!(
            press(&mut s, |i| i.confirm = true),
            SetupOutcome::Start {
                rules: RunRules::default(),
                ng_source: None
            }
        );
        assert_eq!(press(&mut s, |i| i.cancel = true), SetupOutcome::Cancelled);
    }

    #[test]
    fn start_begins_the_run_from_any_row() {
        let mut s = RulesSetup::new(vec![], 1);
        press(&mut s, |i| i.up = true);
        assert_ne!(s.selected(), SetupRow::Start);
        assert_eq!(
            press(&mut s, |i| i.start = true),
            SetupOutcome::Start {
                rules: RunRules::default(),
                ng_source: None
            }
        );
    }

    #[test]
    fn rows_set_every_rule() {
        let mut s = RulesSetup::new(vec![("Save 1  Vahn Lv 40".into(), 0)], 5);
        // Up from Start walks to Randomizer, Ironman, NG+, Difficulty, Mode.
        press(&mut s, |i| i.up = true);
        assert_eq!(s.selected(), SetupRow::Randomizer);
        press(&mut s, |i| i.right = true);
        assert!(s.rules.randomizer.is_some());
        assert!(s.row_list().contains(&SetupRow::Seed));
        let seed = s.rules.randomizer.as_ref().unwrap().seed.clone();
        press(&mut s, |i| i.down = true);
        assert_eq!(s.selected(), SetupRow::Seed);
        press(&mut s, |i| i.confirm = true);
        assert_ne!(s.rules.randomizer.as_ref().unwrap().seed, seed, "rerolled");
        s.set_seed("custom");
        assert_eq!(s.rules.randomizer.as_ref().unwrap().seed, "custom");
        for _ in 0..2 {
            press(&mut s, |i| i.up = true);
        }
        assert_eq!(s.selected(), SetupRow::Ironman);
        press(&mut s, |i| i.right = true);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.ironman, Some(Ironman { hardcore: true }));
        press(&mut s, |i| i.up = true);
        assert_eq!(s.selected(), SetupRow::NgPlus);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.ng_plus.unwrap().cycle, 1);
        assert_eq!(
            s.selected(),
            SetupRow::NgPlus,
            "cursor stays as rows appear"
        );
        press(&mut s, |i| i.down = true);
        assert_eq!(s.selected(), SetupRow::Carry(0));
        press(&mut s, |i| i.confirm = true);
        assert!(!s.rules.ng_plus.unwrap().carry.levels);
        press(&mut s, |i| i.up = true);
        press(&mut s, |i| i.up = true);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.difficulty, Difficulty::Hard);
        press(&mut s, |i| i.up = true);
        press(&mut s, |i| i.confirm = true);
        assert_eq!(s.rules.mode, RunMode::BossRush);
        // Back down to Start.
        let n = s.row_list().len();
        for _ in 0..n - 1 {
            press(&mut s, |i| i.down = true);
        }
        assert_eq!(s.selected(), SetupRow::Start);
        let SetupOutcome::Start { rules, ng_source } = press(&mut s, |i| i.confirm = true) else {
            panic!()
        };
        assert_eq!(ng_source, Some(0));
        assert_eq!(
            rules.summary().unwrap(),
            "Boss Rush / Hard / NG+1 / Hardcore / Seed custom"
        );
        assert_eq!(s.rows().len(), s.row_list().len());
        assert!(!s.help().is_empty());
    }
}
