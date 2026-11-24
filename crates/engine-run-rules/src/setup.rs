//! The New Game rules screen's session: the rows a player sets a run's rules
//! on between the title's NEW GAME and the opening scene.
//!
//! The session is pad-driven and host-free: both play hosts feed it the same
//! edges and draw it through the shared draw builder, so the screen is one
//! screen on both. Its rows sit on four tabs ([`SetupTab`]): the run (mode,
//! ironman, *Start*), the difficulty, New Game+ and the randomizer. Its first
//! state is the retail rules on the run tab with the cursor on *Start*, so
//! one confirm starts the retail game.
//!
//! The randomizer's seed is typed on a letter grid ([`SeedEditor`]), opened
//! by confirming the seed row; a host with a keyboard types into the same
//! editor ([`RulesSetup::seed_editor_type`] and its siblings).

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
    /// L1: the tab to the left.
    pub tab_prev: bool,
    /// R1: the tab to the right.
    pub tab_next: bool,
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

/// One tab of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetupTab {
    #[default]
    Run,
    Difficulty,
    NgPlus,
    Randomizer,
}

impl SetupTab {
    /// Every tab, left to right.
    pub const ALL: [SetupTab; 4] = [
        SetupTab::Run,
        SetupTab::Difficulty,
        SetupTab::NgPlus,
        SetupTab::Randomizer,
    ];

    /// The tab's heading.
    pub const fn label(self) -> &'static str {
        match self {
            SetupTab::Run => "Run",
            SetupTab::Difficulty => "Difficulty",
            SetupTab::NgPlus => "New Game+",
            SetupTab::Randomizer => "Randomizer",
        }
    }

    /// Where the tab sits in [`Self::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn step(self, dir: i8) -> Self {
        let n = Self::ALL.len();
        let i = self.index();
        Self::ALL[if dir < 0 {
            (i + n - 1) % n
        } else {
            (i + 1) % n
        }]
    }
}

/// One row of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRow {
    Mode,
    Ironman,
    Start,
    Difficulty,
    NgPlus,
    Carry(usize),
    Randomizer,
    Seed,
    /// One of the randomizer's parts ([`Randomizer::parts`]).
    Part(usize),
}

/// One cell of the seed editor's letter grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedCell {
    /// Types the character.
    Char(char),
    /// Erases the last character.
    Delete,
    /// Takes the typed seed.
    Done,
}

impl SeedCell {
    /// The cell as the grid shows it.
    pub fn label(self) -> String {
        match self {
            SeedCell::Char(c) => c.to_string(),
            SeedCell::Delete => "Del".into(),
            SeedCell::Done => "Done".into(),
        }
    }
}

/// The longest seed the editor takes.
pub const SEED_MAX: usize = 20;

/// The seed editor's letter grid, row by row: the capitals, the dash the
/// rolled seeds use, the digits, then the two actions.
pub fn seed_grid() -> Vec<Vec<SeedCell>> {
    let chars = |s: &str| s.chars().map(SeedCell::Char).collect::<Vec<_>>();
    vec![
        chars("ABCDEFGHIJ"),
        chars("KLMNOPQRST"),
        chars("UVWXYZ-"),
        chars("0123456789"),
        vec![SeedCell::Delete, SeedCell::Done],
    ]
}

/// Whether a seed may hold `c`: printable ASCII, which is what the screen
/// font draws.
pub fn seed_char_ok(c: char) -> bool {
    c.is_ascii_graphic() || c == ' '
}

/// The letter grid a seed is typed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedEditor {
    /// The seed as typed so far.
    pub text: String,
    /// The selected cell, `(row, column)` of [`seed_grid`].
    pub cell: (usize, usize),
    /// The seed the editor opened on, put back on a cancel.
    before: String,
}

impl SeedEditor {
    fn open(seed: &str) -> Self {
        Self {
            text: seed.to_string(),
            cell: (0, 0),
            before: seed.to_string(),
        }
    }

    fn type_char(&mut self, c: char) {
        if seed_char_ok(c) && self.text.chars().count() < SEED_MAX {
            self.text.push(c);
        }
    }

    fn selected(&self) -> SeedCell {
        let grid = seed_grid();
        let row = &grid[self.cell.0.min(grid.len() - 1)];
        row[self.cell.1.min(row.len() - 1)]
    }

    /// One frame on the grid: `Some(true)` takes the seed, `Some(false)`
    /// drops the edit, `None` keeps the grid up.
    fn step(&mut self, input: SetupInput) -> Option<bool> {
        let grid = seed_grid();
        let (mut r, mut c) = self.cell;
        if input.up || input.down {
            r = if input.up {
                (r + grid.len() - 1) % grid.len()
            } else {
                (r + 1) % grid.len()
            };
            // A column past a short row lands on its last cell.
            c = c.min(grid[r].len() - 1);
        } else if input.left || input.right {
            let n = grid[r].len();
            c = if input.left {
                (c + n - 1) % n
            } else {
                (c + 1) % n
            };
        }
        self.cell = (r, c);
        if input.start {
            return Some(true);
        }
        if input.cancel {
            // Circle erases; on an empty field it leaves the grid.
            if self.text.pop().is_none() {
                return Some(false);
            }
            return None;
        }
        if input.confirm {
            match self.selected() {
                SeedCell::Char(ch) => self.type_char(ch),
                SeedCell::Delete => {
                    self.text.pop();
                }
                SeedCell::Done => return Some(true),
            }
        }
        None
    }
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
    /// The tab on screen.
    pub tab: SetupTab,
    /// The cursor row on each tab, by [`SetupTab::index`].
    pub cursors: [usize; 4],
    /// Entropy for the next seed the *Seed* row rolls.
    pub entropy: u64,
    /// The seed's letter grid, while it is up.
    pub seed_editor: Option<SeedEditor>,
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
    /// retail rules on the run tab with the cursor on *Start*.
    pub fn new(ng_sources: Vec<(String, u8)>, entropy: u64) -> Self {
        let (labels, cycles) = ng_sources.into_iter().unzip();
        let mut s = Self {
            rules: RunRules::default(),
            ng_sources: labels,
            ng_source_cycles: cycles,
            ng_source: None,
            tab: SetupTab::Run,
            cursors: [0; 4],
            entropy,
            seed_editor: None,
        };
        s.cursors[0] = s
            .row_list()
            .iter()
            .position(|&r| r == SetupRow::Start)
            .unwrap_or(0);
        s
    }

    /// The cursor row on the tab on screen.
    pub fn cursor(&self) -> usize {
        self.cursors[self.tab.index()]
    }

    fn set_cursor(&mut self, c: usize) {
        self.cursors[self.tab.index()] = c;
    }

    /// The rows of `tab`: the carry rows show while New Game+ is on, the
    /// seed and the parts while the randomizer is.
    pub fn tab_rows(&self, tab: SetupTab) -> Vec<SetupRow> {
        match tab {
            SetupTab::Run => vec![SetupRow::Mode, SetupRow::Ironman, SetupRow::Start],
            SetupTab::Difficulty => vec![SetupRow::Difficulty],
            SetupTab::NgPlus => {
                let mut rows = vec![SetupRow::NgPlus];
                if self.rules.ng_plus.is_some() {
                    rows.extend((0..7).map(SetupRow::Carry));
                }
                rows
            }
            SetupTab::Randomizer => {
                let mut rows = vec![SetupRow::Randomizer];
                if self.rules.randomizer.is_some() {
                    rows.push(SetupRow::Seed);
                    rows.extend((0..7).map(SetupRow::Part));
                }
                rows
            }
        }
    }

    /// The rows on screen now (the tab on screen's).
    pub fn row_list(&self) -> Vec<SetupRow> {
        self.tab_rows(self.tab)
    }

    /// Whether `row` does anything when changed (a New Game+ row with no
    /// save to carry from does not).
    pub fn row_enabled(&self, row: SetupRow) -> bool {
        !(row == SetupRow::NgPlus && self.ng_sources.is_empty())
    }

    /// One row as `(label, value)` text.
    pub fn row_text(&self, row: SetupRow) -> (String, String) {
        let on_off = |on: bool| if on { "On" } else { "Off" }.to_string();
        match row {
            SetupRow::Mode => (
                "Mode".into(),
                match self.rules.mode {
                    RunMode::Story => "Story",
                    RunMode::BossRush => "Boss Rush",
                }
                .into(),
            ),
            SetupRow::Difficulty => ("Preset".into(), self.rules.difficulty.label().into()),
            SetupRow::NgPlus => (
                "Carry from".into(),
                match self.ng_source {
                    Some(i) if self.rules.ng_plus.is_some() => self
                        .ng_sources
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| "?".into()),
                    _ if self.ng_sources.is_empty() => "No save".into(),
                    _ => "Off".into(),
                },
            ),
            SetupRow::Carry(i) => {
                let (label, on) = self
                    .rules
                    .ng_plus
                    .map(|n| n.carry)
                    .unwrap_or_default()
                    .rows()[i];
                (
                    format!("Keep {label}"),
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
            SetupRow::Randomizer => ("Randomizer".into(), on_off(self.rules.randomizer.is_some())),
            SetupRow::Seed => (
                "Seed".into(),
                self.rules
                    .randomizer
                    .as_ref()
                    .map_or(String::new(), |r| r.seed.clone()),
            ),
            SetupRow::Part(i) => {
                let (label, on) = self
                    .rules
                    .randomizer
                    .as_ref()
                    .map_or(("", false), |r| r.parts()[i]);
                (label.into(), on_off(on))
            }
            SetupRow::Start => ("Start".into(), String::new()),
        }
    }

    /// Each row on screen as `(label, value)` text, in [`Self::row_list`]
    /// order.
    pub fn rows(&self) -> Vec<(String, String)> {
        self.row_list()
            .into_iter()
            .map(|r| self.row_text(r))
            .collect()
    }

    /// The row under the cursor.
    pub fn selected(&self) -> SetupRow {
        let rows = self.row_list();
        rows[self.cursor().min(rows.len() - 1)]
    }

    /// A line describing the selected row at its current value, for the
    /// screen's help window: what the row does, and for a preset what it
    /// changes. The seed editor, while up, says how to type.
    pub fn help(&self) -> String {
        if self.seed_editor.is_some() {
            return "Pick letters on the grid, or type them. Done takes the seed; \
                    an empty seed keeps the old one."
                .into();
        }
        match self.selected() {
            SetupRow::Mode => match self.rules.mode {
                RunMode::Story => "The story, from the opening.",
                RunMode::BossRush => "Every story boss back to back.",
            }
            .into(),
            SetupRow::Difficulty => self.rules.difficulty.blurb().into(),
            SetupRow::NgPlus => match self.rules.ng_plus {
                _ if self.ng_sources.is_empty() => "No save on a card to carry from.".into(),
                None => "Replay a save's party; foes scale up.".into(),
                Some(n) => format!(
                    "Cycle {}: foes grow with the cycle and your levels.",
                    n.cycle
                ),
            },
            SetupRow::Carry(_) => "What the new run keeps from the save.".into(),
            SetupRow::Ironman => match self.rules.ironman {
                Some(Ironman { hardcore: true }) => "Ironman, and no items in battle.",
                _ => "One save per run, erased if the party falls.",
            }
            .into(),
            SetupRow::Randomizer => "Fights, loot, shops and stats drawn from a seed, \
                                     with no disc patch."
                .into(),
            SetupRow::Seed => "Confirm types a seed; Left/Right rolls one. The disc \
                               patcher reads seeds its own way."
                .into(),
            SetupRow::Part(i) => match i {
                0 => "Random fights draw from the area's own monsters.",
                1 => "Each monster drops a different sellable item.",
                2 => "Each monster holds a different item to steal.",
                3 => "Each shop sells a different stock.",
                4 => "Chests and gifts hold different items; key items stay.",
                5 => "Monster stats move within 0.8x to 1.25x; bosses keep theirs.",
                _ => "Vahn starts in different gear he can wear.",
            }
            .into(),
            SetupRow::Start => "Begin the run.".into(),
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
            SetupRow::Part(i) => {
                if let Some(r) = self.rules.randomizer.as_mut() {
                    r.toggle_part(i);
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

    /// Open the seed's letter grid over the current seed (the seed row's
    /// confirm). `false` while the randomizer is off.
    pub fn open_seed_editor(&mut self) -> bool {
        let Some(seed) = self.rules.randomizer.as_ref().map(|r| r.seed.clone()) else {
            return false;
        };
        self.seed_editor = Some(SeedEditor::open(&seed));
        true
    }

    /// Type `c` into the open seed editor (a host's keyboard).
    pub fn seed_editor_type(&mut self, c: char) {
        if let Some(e) = self.seed_editor.as_mut() {
            e.type_char(c);
        }
    }

    /// Erase the editor's last character.
    pub fn seed_editor_backspace(&mut self) {
        if let Some(e) = self.seed_editor.as_mut() {
            e.text.pop();
        }
    }

    /// Replace the editor's text whole (the page's text box), held to the
    /// characters and length the grid takes.
    pub fn seed_editor_set_text(&mut self, text: &str) {
        if let Some(e) = self.seed_editor.as_mut() {
            e.text = text
                .chars()
                .filter(|&c| seed_char_ok(c))
                .take(SEED_MAX)
                .collect();
        }
    }

    /// Close the editor, taking its seed (`take`) or dropping the edit. An
    /// empty seed keeps the old one.
    pub fn close_seed_editor(&mut self, take: bool) {
        let Some(e) = self.seed_editor.take() else {
            return;
        };
        let seed = if take { e.text } else { e.before };
        self.set_seed(&seed);
    }

    /// One frame of input.
    pub fn step(&mut self, input: SetupInput) -> SetupOutcome {
        if let Some(e) = self.seed_editor.as_mut() {
            if let Some(take) = e.step(input) {
                self.close_seed_editor(take);
            }
            return SetupOutcome::Open;
        }
        if input.cancel {
            return SetupOutcome::Cancelled;
        }
        if input.start {
            return self.start();
        }
        if input.tab_prev || input.tab_next {
            self.tab = self.tab.step(if input.tab_prev { -1 } else { 1 });
            let n = self.row_list().len();
            self.set_cursor(self.cursor().min(n - 1));
            return SetupOutcome::Open;
        }
        let n = self.row_list().len();
        if input.up {
            self.set_cursor((self.cursor() + n - 1) % n);
        } else if input.down {
            self.set_cursor((self.cursor() + 1) % n);
        }
        let row = self.selected();
        if input.left || input.right {
            self.change(row, if input.left { -1 } else { 1 });
        } else if input.confirm {
            match row {
                SetupRow::Start => return self.start(),
                SetupRow::Seed => {
                    self.open_seed_editor();
                }
                _ => self.change(row, 1),
            }
        }
        // Rows come and go with New Game+ and the randomizer; keep the
        // cursor on the row it was on.
        let rows = self.row_list();
        let c = rows
            .iter()
            .position(|&r| r == row)
            .unwrap_or(self.cursor().min(rows.len() - 1));
        self.set_cursor(c);
        SetupOutcome::Open
    }

    fn start(&self) -> SetupOutcome {
        SetupOutcome::Start {
            rules: self.rules.clone(),
            ng_source: self.ng_source.filter(|_| self.rules.ng_plus.is_some()),
        }
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

    fn go_to(s: &mut RulesSetup, tab: SetupTab, row: SetupRow) {
        while s.tab != tab {
            press(s, |i| i.tab_next = true);
        }
        for _ in 0..s.row_list().len() {
            if s.selected() == row {
                return;
            }
            press(s, |i| i.down = true);
        }
        panic!("no row {row:?} on {tab:?}");
    }

    #[test]
    fn one_confirm_starts_the_retail_game() {
        let mut s = RulesSetup::new(vec![], 1);
        assert_eq!(s.tab, SetupTab::Run);
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
    fn start_begins_the_run_from_any_row_and_tab() {
        let mut s = RulesSetup::new(vec![], 1);
        press(&mut s, |i| i.tab_next = true);
        assert_eq!(s.tab, SetupTab::Difficulty);
        assert_eq!(
            press(&mut s, |i| i.start = true),
            SetupOutcome::Start {
                rules: RunRules::default(),
                ng_source: None
            }
        );
    }

    #[test]
    fn tabs_wrap_and_keep_their_cursors() {
        let mut s = RulesSetup::new(vec![], 1);
        press(&mut s, |i| i.up = true);
        assert_eq!(s.selected(), SetupRow::Ironman);
        press(&mut s, |i| i.tab_prev = true);
        assert_eq!(s.tab, SetupTab::Randomizer);
        press(&mut s, |i| i.tab_next = true);
        assert_eq!(s.tab, SetupTab::Run);
        assert_eq!(s.selected(), SetupRow::Ironman, "the run tab kept its row");
    }

    #[test]
    fn rows_set_every_rule() {
        let mut s = RulesSetup::new(vec![("Save 1  Vahn Lv 40".into(), 0)], 5);
        go_to(&mut s, SetupTab::Randomizer, SetupRow::Randomizer);
        press(&mut s, |i| i.right = true);
        assert!(s.rules.randomizer.is_some());
        assert!(s.row_list().contains(&SetupRow::Seed));
        let seed = s.rules.randomizer.as_ref().unwrap().seed.clone();
        press(&mut s, |i| i.down = true);
        assert_eq!(s.selected(), SetupRow::Seed);
        press(&mut s, |i| i.right = true);
        assert_ne!(s.rules.randomizer.as_ref().unwrap().seed, seed, "rerolled");
        s.set_seed("custom");
        assert_eq!(s.rules.randomizer.as_ref().unwrap().seed, "custom");
        go_to(&mut s, SetupTab::Randomizer, SetupRow::Part(3));
        press(&mut s, |i| i.confirm = true);
        assert!(!s.rules.randomizer.as_ref().unwrap().shops);
        assert_eq!(
            s.row_text(SetupRow::Part(3)),
            ("Shops".into(), "Off".into())
        );

        go_to(&mut s, SetupTab::Run, SetupRow::Ironman);
        press(&mut s, |i| i.right = true);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.ironman, Some(Ironman { hardcore: true }));
        go_to(&mut s, SetupTab::Run, SetupRow::Mode);
        press(&mut s, |i| i.confirm = true);
        assert_eq!(s.rules.mode, RunMode::BossRush);

        go_to(&mut s, SetupTab::NgPlus, SetupRow::NgPlus);
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

        go_to(&mut s, SetupTab::Difficulty, SetupRow::Difficulty);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.difficulty, Difficulty::Hard);

        let SetupOutcome::Start { rules, ng_source } = press(&mut s, |i| i.start = true) else {
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

    #[test]
    fn the_seed_is_typed_on_the_grid_or_the_keyboard() {
        let mut s = RulesSetup::new(vec![], 9);
        go_to(&mut s, SetupTab::Randomizer, SetupRow::Randomizer);
        press(&mut s, |i| i.confirm = true);
        go_to(&mut s, SetupTab::Randomizer, SetupRow::Seed);
        let rolled = s.rules.randomizer.as_ref().unwrap().seed.clone();
        press(&mut s, |i| i.confirm = true);
        assert!(s.seed_editor.is_some(), "confirm opens the grid");
        // Circle erases the rolled seed down to nothing.
        for _ in 0..rolled.len() {
            press(&mut s, |i| i.cancel = true);
        }
        assert_eq!(s.seed_editor.as_ref().unwrap().text, "");
        // Cross on the first cell types `A`; Right then Cross types `B`.
        press(&mut s, |i| i.confirm = true);
        press(&mut s, |i| i.right = true);
        press(&mut s, |i| i.confirm = true);
        // Down three rows lands on the digits, column 1: `1`.
        for _ in 0..3 {
            press(&mut s, |i| i.down = true);
        }
        press(&mut s, |i| i.confirm = true);
        s.seed_editor_type('-');
        s.seed_editor_type('z');
        s.seed_editor_backspace();
        // Down again lands on the actions, the column held to the row: Done.
        press(&mut s, |i| i.down = true);
        assert_eq!(s.seed_editor.as_ref().unwrap().cell, (4, 1));
        press(&mut s, |i| i.confirm = true);
        assert!(s.seed_editor.is_none());
        assert_eq!(s.rules.randomizer.as_ref().unwrap().seed, "AB1-");
        // A cancel puts the old seed back; an empty take keeps it too.
        press(&mut s, |i| i.confirm = true);
        s.seed_editor_set_text("dropped");
        s.close_seed_editor(false);
        assert_eq!(s.rules.randomizer.as_ref().unwrap().seed, "AB1-");
        press(&mut s, |i| i.confirm = true);
        s.seed_editor_set_text("");
        press(&mut s, |i| i.start = true);
        assert_eq!(s.rules.randomizer.as_ref().unwrap().seed, "AB1-");
        // The page's text box is held to the grid's length and characters.
        press(&mut s, |i| i.confirm = true);
        s.seed_editor_set_text(&format!("{}\u{7}", "x".repeat(40)));
        assert_eq!(s.seed_editor.as_ref().unwrap().text.len(), SEED_MAX);
    }
}
