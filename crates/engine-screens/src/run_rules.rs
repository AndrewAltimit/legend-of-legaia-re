//! The run rules' screens, projected once for both play hosts: the New Game
//! rules screen ([`setup_menu`]), the Boss Rush intermission and result
//! ([`boss_rush_screen`]), and the pause menu's run line
//! ([`run_summary_draws`]).
//!
//! The rules screen is a menu-style screen ([`SetupMenu`]): a host draws its
//! black backdrop prim under the text layer, its chrome sprites with the
//! pause menu's (the system-UI atlas) and its labels with its stage text.
//! The Boss Rush screens are one flat panel ([`RulesScreen`]): its prims
//! under the text layer, its labels with the stage text. [`active_screen`]
//! answers which owns the frame.

use legaia_engine_core::world::World;
use legaia_engine_run_rules::boss_rush::{Intermission, clock_label};
use legaia_engine_run_rules::setup::{RulesSetup, SetupTab, seed_grid};
use legaia_engine_ui::run_rules_screen::{
    RulesMenuRow, RulesMenuView, RulesPanelView, SeedGridView, rules_menu_prims,
    rules_menu_sprites, rules_menu_strings, rules_menu_texts, rules_panel_prims,
    rules_panel_strings, rules_panel_texts,
};
use legaia_engine_ui::{SaveMenuAtlasRects, SpriteDraw, TextDraw};
use legaia_render_kernels::screen_prim::ScreenPrim;

/// One rules screen's content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RulesScreen {
    pub title: String,
    pub subtitle: Option<String>,
    pub rows: Vec<(String, String)>,
    pub cursor: Option<usize>,
    pub footer: Option<String>,
    pub hint: String,
}

impl RulesScreen {
    fn view(&self) -> RulesPanelView<'_> {
        RulesPanelView {
            title: &self.title,
            subtitle: self.subtitle.as_deref(),
            rows: &self.rows,
            cursor: self.cursor,
            footer: self.footer.as_deref(),
            hint: &self.hint,
        }
    }

    /// The panel's rects, in stage pixels.
    pub fn prims(&self) -> Vec<ScreenPrim> {
        rules_panel_prims(&self.view())
    }

    /// The panel's labels, in stage pixels.
    pub fn texts(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        rules_panel_texts(font, &self.view())
    }

    /// The labels as strings (tests, a text host).
    pub fn strings(&self) -> Vec<String> {
        rules_panel_strings(&self.view())
    }
}

/// What a key hint calls the pad buttons the screens read: the host's
/// bound key, or the controller's button while one was pressed last (each
/// host's `prompt_label` rule, the minigame captions' too).
/// [`PromptKeys::default`] names the pad buttons themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptKeys {
    /// Cross.
    pub confirm: String,
    /// Circle.
    pub back: String,
    /// Start.
    pub start: String,
    /// L1 (the tab to the left).
    pub tab_prev: String,
    /// R1 (the tab to the right).
    pub tab_next: String,
    /// The keyboard was pressed last: the seed editor types from it (its
    /// hint names the typing keys, not the grid's buttons).
    pub keyboard: bool,
}

impl Default for PromptKeys {
    fn default() -> Self {
        Self::from_labels(|_| None)
    }
}

impl PromptKeys {
    /// The names `label` gives `"Cross"`, `"Circle"` and `"Start"`, each
    /// falling back to the button's own name.
    pub fn from_labels(label: impl Fn(&str) -> Option<String>) -> Self {
        let name = |b: &str| label(b).unwrap_or_else(|| b.to_string());
        Self {
            confirm: name("Cross"),
            back: name("Circle"),
            start: name("Start"),
            tab_prev: name("L1"),
            tab_next: name("R1"),
            keyboard: false,
        }
    }
}

/// The New Game rules screen's content, owned (the view borrows it).
#[derive(Debug, Clone, PartialEq)]
pub struct SetupMenu {
    pub tab: usize,
    pub rows: Vec<RulesMenuRow>,
    pub cursor: usize,
    pub info: Vec<(String, String)>,
    pub help: String,
    pub summary: String,
    pub hint: String,
    pub seed_grid: Option<SeedGrid>,
}

/// The seed editor on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedGrid {
    /// The seed as typed so far.
    pub text: String,
    /// The grid's cell labels, row by row.
    pub cells: Vec<Vec<String>>,
    /// The selected cell, `(row, column)`.
    pub cell: (usize, usize),
}

/// The tab headings, in [`SetupTab::ALL`] order.
const TAB_LABELS: [&str; 4] = [
    SetupTab::ALL[0].label(),
    SetupTab::ALL[1].label(),
    SetupTab::ALL[2].label(),
    SetupTab::ALL[3].label(),
];

impl SetupMenu {
    fn view(&self) -> RulesMenuView<'_> {
        RulesMenuView {
            tabs: &TAB_LABELS,
            tab: self.tab,
            rows: &self.rows,
            cursor: self.cursor,
            info: &self.info,
            help: &self.help,
            summary: &self.summary,
            hint: &self.hint,
            seed_grid: self.seed_grid.as_ref().map(|g| SeedGridView {
                text: &g.text,
                cells: &g.cells,
                cell: g.cell,
            }),
        }
    }

    /// The black backdrop, in stage pixels.
    pub fn prims(&self) -> Vec<ScreenPrim> {
        rules_menu_prims()
    }

    /// The windows, tab plaques and hand cursors, in surface pixels.
    pub fn sprites(
        &self,
        rects: &SaveMenuAtlasRects,
        stage_origin: (i32, i32),
        stage_scale: u32,
    ) -> Vec<SpriteDraw> {
        rules_menu_sprites(&self.view(), rects, stage_origin, stage_scale)
    }

    /// The labels, in stage pixels.
    pub fn texts(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        rules_menu_texts(font, &self.view())
    }

    /// The labels as strings (tests, a text host).
    pub fn strings(&self) -> Vec<String> {
        rules_menu_strings(&self.view())
    }
}

/// The New Game rules screen.
pub fn setup_menu(s: &RulesSetup, keys: &PromptKeys) -> SetupMenu {
    let rows = s
        .row_list()
        .into_iter()
        .map(|r| {
            let (label, value) = s.row_text(r);
            RulesMenuRow {
                label,
                value,
                enabled: s.row_enabled(r),
            }
        })
        .collect();
    // The difficulty tab lists what its preset scales.
    let info = if s.tab == SetupTab::Difficulty {
        s.rules
            .knobs()
            .rows()
            .into_iter()
            .map(|(l, v)| (l.to_string(), v))
            .collect()
    } else {
        Vec::new()
    };
    let seed_grid = s.seed_editor.as_ref().map(|e| {
        let cells = seed_grid()
            .into_iter()
            .map(|row| row.into_iter().map(|c| c.label()).collect())
            .collect();
        SeedGrid {
            text: e.text.clone(),
            cells,
            cell: e.cell,
        }
    });
    let hint = if seed_grid.is_some() && keys.keyboard {
        "Type  Backspace erase  Enter OK  Esc cancel".to_string()
    } else if seed_grid.is_some() {
        format!(
            "{} pick  {} erase  {} done",
            keys.confirm, keys.back, keys.start
        )
    } else {
        format!(
            "{}/{} tab  {} OK  {} begin  {} back",
            keys.tab_prev, keys.tab_next, keys.confirm, keys.start, keys.back
        )
    };
    SetupMenu {
        tab: s.tab.index(),
        rows,
        cursor: s.cursor(),
        info,
        help: s.help(),
        summary: s
            .rules
            .summary()
            .unwrap_or_else(|| "Original rules".to_string()),
        hint,
        seed_grid,
    }
}

/// The Boss Rush screen this frame: the intermission while it is open, the
/// result once the run is over; `None` otherwise.
pub fn boss_rush_screen(world: &World, keys: &PromptKeys) -> Option<RulesScreen> {
    let run = world.run_rules.boss_rush.as_ref()?;
    if let Some((cleared, won, frames)) = world.boss_rush_result() {
        let rec = &world.run_rules.record;
        let best = rec
            .best_clear_frames
            .map_or_else(|| "-".to_string(), clock_label);
        return Some(RulesScreen {
            title: if cleared {
                "BOSS RUSH CLEAR"
            } else {
                "BOSS RUSH OVER"
            }
            .into(),
            subtitle: Some(format!("Time {}", clock_label(frames))),
            rows: vec![
                (
                    "Bosses beaten".into(),
                    format!("{won} / {}", run.stages.len()),
                ),
                ("Best clear".into(), best),
                (
                    "Clears / runs".into(),
                    format!("{} / {}", rec.clears, rec.runs),
                ),
                ("Most bosses in one run".into(), rec.best_stages.to_string()),
            ],
            cursor: None,
            footer: None,
            hint: format!("{} return to title", keys.confirm),
        });
    }
    let m = world.run_rules.intermission.as_ref()?;
    let next = run.current().map(|s| s.name()).unwrap_or_default();
    let rows = Intermission::rows()
        .into_iter()
        .map(|r| (Intermission::row_label(r), String::new()))
        .collect();
    Some(RulesScreen {
        title: format!("BOSS RUSH  {} / {}", run.stage, run.stages.len()),
        subtitle: Some(format!(
            "Next: {next}   Time {}   Gold {}",
            clock_label(run.frames),
            world.party.money
        )),
        rows,
        cursor: Some(m.cursor),
        footer: m.message.clone(),
        hint: format!("Up/Down choose  {} OK", keys.confirm),
    })
}

/// A run-rules screen.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveScreen {
    /// The New Game rules screen.
    Setup(SetupMenu),
    /// The Boss Rush intermission or result.
    Panel(RulesScreen),
}

impl ActiveScreen {
    /// The prims under the text layer, in stage pixels.
    pub fn prims(&self) -> Vec<ScreenPrim> {
        match self {
            Self::Setup(m) => m.prims(),
            Self::Panel(p) => p.prims(),
        }
    }

    /// The chrome sprites (the rules screen's; a panel has none), in
    /// surface pixels.
    pub fn sprites(
        &self,
        rects: &SaveMenuAtlasRects,
        stage_origin: (i32, i32),
        stage_scale: u32,
    ) -> Vec<SpriteDraw> {
        match self {
            Self::Setup(m) => m.sprites(rects, stage_origin, stage_scale),
            Self::Panel(_) => Vec::new(),
        }
    }

    /// The labels, in stage pixels.
    pub fn texts(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        match self {
            Self::Setup(m) => m.texts(font),
            Self::Panel(p) => p.texts(font),
        }
    }

    /// The labels as strings (tests, a text host).
    pub fn strings(&self) -> Vec<String> {
        match self {
            Self::Setup(m) => m.strings(),
            Self::Panel(p) => p.strings(),
        }
    }
}

/// The run-rules screen that owns the frame, if one does: the New Game
/// rules screen while it is up, else the Boss Rush intermission or result.
/// `keys` name the buttons in its hint; no rect or sprite depends on them.
pub fn active_screen(world: &World, keys: &PromptKeys) -> Option<ActiveScreen> {
    if let Some(seat) = world.run_rules.setup.as_deref() {
        return Some(ActiveScreen::Setup(setup_menu(&seat.setup, keys)));
    }
    boss_rush_screen(world, keys).map(ActiveScreen::Panel)
}

/// The owning screen's labels, in stage pixels (each host's text layer
/// scales and appends them, as it does the expansion screens').
pub fn run_rules_stage_texts(
    world: &World,
    font: &legaia_font::Font,
    keys: &PromptKeys,
) -> Vec<TextDraw> {
    active_screen(world, keys).map_or_else(Vec::new, |s| s.texts(font))
}

/// The owning screen's chrome sprites, in surface pixels (the rules
/// screen's windows; none for a Boss Rush panel), for the host's system-UI
/// atlas pass.
pub fn run_rules_chrome_sprites(
    world: &World,
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    active_screen(world, &PromptKeys::default())
        .map_or_else(Vec::new, |s| s.sprites(rects, stage_origin, stage_scale))
}

/// The pause menu's run line (difficulty, New Game+, seed ...), `None`
/// under retail rules. Drawn under the journal hint's line.
pub fn run_summary_draws(
    font: &legaia_font::Font,
    world: &World,
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let Some(line) = world.run_rules_summary() else {
        return Vec::new();
    };
    let mut texts = legaia_engine_ui::text_draws_for(
        &font.layout_ascii(&line),
        RUN_SUMMARY_PEN,
        [0.93, 0.74, 0.26, 1.0],
    );
    legaia_engine_ui::scale_stage_text_draws(&mut texts, origin, scale);
    texts
}

/// Where [`run_summary_draws`] writes, in stage pixels.
pub const RUN_SUMMARY_PEN: (i32, i32) = (24, 224);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setup_screen_lists_the_rows_and_the_run() {
        let s = RulesSetup::new(vec![], 3);
        let screen = setup_menu(&s, &PromptKeys::default());
        let strings = screen.strings();
        for t in [
            "Run",
            "Difficulty",
            "New Game+",
            "Randomizer",
            "Mode",
            "Original rules",
        ] {
            assert!(strings.iter().any(|x| x == t), "{t} missing");
        }
        assert!(!screen.prims().is_empty());
        let w = World::new();
        assert_eq!(boss_rush_screen(&w, &PromptKeys::default()), None);
        assert_eq!(active_screen(&w, &PromptKeys::default()), None);
    }

    #[test]
    fn the_difficulty_tab_lists_its_scales() {
        use legaia_engine_run_rules::setup::SetupInput;
        let mut s = RulesSetup::new(vec![], 3);
        s.step(SetupInput {
            tab_next: true,
            ..Default::default()
        });
        s.step(SetupInput {
            right: true,
            ..Default::default()
        });
        let m = setup_menu(&s, &PromptKeys::default());
        assert_eq!(m.info.len(), 10);
        assert_eq!(m.info[0], ("Foe stats".to_string(), "x1.35".to_string()));
        assert_eq!(m.summary, "Hard");
    }

    #[test]
    fn the_hint_names_the_hosts_keys() {
        let s = RulesSetup::new(vec![], 3);
        let keys = PromptKeys::from_labels(|b| match b {
            "Cross" => Some("Z".into()),
            "Circle" => Some("X".into()),
            "L1" => Some("Q".into()),
            "R1" => Some("E".into()),
            "Start" => Some("Enter".into()),
            _ => None,
        });
        assert_eq!(
            setup_menu(&s, &keys).hint,
            "Q/E tab  Z OK  Enter begin  X back"
        );
        // No rect depends on the key names.
        assert_eq!(
            setup_menu(&s, &keys).prims(),
            setup_menu(&s, &PromptKeys::default()).prims()
        );
    }
}
