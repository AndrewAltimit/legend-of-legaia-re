//! The run rules' screens, projected once for both play hosts: the New Game
//! rules screen ([`setup_menu`]), the Boss Rush intermission and result
//! ([`boss_rush_screen`]), and the pause menu's run line
//! ([`run_summary_draws`]).
//!
//! Each is a menu-style screen ([`RulesMenu`]): a host draws its black
//! backdrop prim under the text layer, its chrome sprites with the pause
//! menu's (the system-UI atlas) and its labels with its stage text.
//! [`active_screen`] answers which owns the frame; [`setup_tap`] resolves a
//! tap on the rules screen from the geometry it draws.

use legaia_engine_core::world::World;
use legaia_engine_run_rules::boss_rush::{BossRushRecord, Intermission, clock_label};
use legaia_engine_run_rules::rules::{Difficulty, RunMode};
use legaia_engine_run_rules::setup::{RulesSetup, SetupOverlay, SetupTab, SetupTap, seed_grid};
use legaia_engine_ui::run_rules_screen::{
    InfoLine, ListWindowView, MenuHit, RulesMenuRow, RulesMenuView, SeedGridView, rules_menu_hit,
    rules_menu_prims, rules_menu_sprites, rules_menu_strings, rules_menu_texts,
};
use legaia_engine_ui::{SaveMenuAtlasRects, SpriteDraw, TextDraw};
use legaia_render_kernels::screen_prim::ScreenPrim;

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
    /// Square (the tab's reset).
    pub reset: String,
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
    /// The names `label` gives the pad buttons, each falling back to the
    /// button's own name.
    pub fn from_labels(label: impl Fn(&str) -> Option<String>) -> Self {
        let name = |b: &str| label(b).unwrap_or_else(|| b.to_string());
        Self {
            confirm: name("Cross"),
            back: name("Circle"),
            start: name("Start"),
            reset: name("Square"),
            tab_prev: name("L1"),
            tab_next: name("R1"),
            keyboard: false,
        }
    }
}

/// The list window over a rules screen's rows, owned.
#[derive(Debug, Clone, PartialEq)]
pub struct ListWindow {
    pub title: String,
    pub facts: Vec<(String, String)>,
    pub rows: Vec<RulesMenuRow>,
    pub cursor: usize,
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

/// One rules screen's content, owned (the view borrows it).
#[derive(Debug, Clone, PartialEq)]
pub struct RulesMenu {
    /// The tab headings, or the screen's one title.
    pub tabs: Vec<String>,
    pub tab: usize,
    pub rows: Vec<RulesMenuRow>,
    pub cursor: Option<usize>,
    pub info: Vec<InfoLine>,
    pub help: String,
    pub summary: String,
    pub hint: String,
    pub seed_grid: Option<SeedGrid>,
    pub list: Option<ListWindow>,
}

impl RulesMenu {
    /// Draw `f` over this screen's view.
    fn with_view<T>(&self, f: impl FnOnce(&RulesMenuView<'_>) -> T) -> T {
        let tabs: Vec<&str> = self.tabs.iter().map(String::as_str).collect();
        let view = RulesMenuView {
            tabs: &tabs,
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
            list: self.list.as_ref().map(|l| ListWindowView {
                title: &l.title,
                facts: &l.facts,
                rows: &l.rows,
                cursor: l.cursor,
            }),
        };
        f(&view)
    }

    /// The black backdrop, in stage pixels.
    pub fn prims(&self) -> Vec<ScreenPrim> {
        rules_menu_prims()
    }

    /// The windows, plaques and hand cursors, in surface pixels.
    pub fn sprites(
        &self,
        rects: &SaveMenuAtlasRects,
        stage_origin: (i32, i32),
        stage_scale: u32,
    ) -> Vec<SpriteDraw> {
        self.with_view(|v| rules_menu_sprites(v, rects, stage_origin, stage_scale))
    }

    /// The labels and font marks, in stage pixels.
    pub fn texts(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        self.with_view(|v| rules_menu_texts(font, v))
    }

    /// The labels as strings (tests, a text host).
    pub fn strings(&self) -> Vec<String> {
        self.with_view(rules_menu_strings)
    }

    /// What a tap at stage pixel `at` lands on.
    pub fn hit(&self, at: (i32, i32)) -> Option<MenuHit> {
        self.with_view(|v| rules_menu_hit(v, at))
    }
}

/// The session gameplay tweaks that multiply a difficulty preset's knobs
/// (`WorldToggles`, set from the launcher's or the page's tweak rows), for
/// the difficulty tab's help: `None` when every one is at retail's value.
pub fn stacking_tweaks_note(world: &World) -> Option<String> {
    use legaia_engine_run_rules::rules::permille_label;
    let t = &world.toggles;
    let base = legaia_engine_core::world::WorldToggles::default();
    let mut on = Vec::new();
    if t.enemy_stat_permille != base.enemy_stat_permille {
        on.push(format!(
            "foe stats {}",
            permille_label(t.enemy_stat_permille)
        ));
    }
    if t.enemy_attack_permille != base.enemy_attack_permille {
        on.push(format!(
            "foe attacks {}",
            permille_label(t.enemy_attack_permille)
        ));
    }
    if t.exp_rate_permille != base.exp_rate_permille {
        on.push(format!("EXP {}", permille_label(t.exp_rate_permille)));
    }
    if t.spirit_ap != base.spirit_ap || t.damage_ap != base.damage_ap {
        on.push("AP".to_string());
    }
    if t.smarter_monster_targeting {
        on.push("smart foes".to_string());
    }
    (!on.is_empty()).then(|| format!("Tweaks stack on top: {}.", on.join(", ")))
}

/// The tab headings, in [`SetupTab::ALL`] order.
fn tab_labels() -> Vec<String> {
    SetupTab::ALL
        .iter()
        .map(|t| t.label().to_string())
        .collect()
}

/// `(label, value)` cells laid `per` to a line.
fn lines_of(cells: Vec<(String, String)>, per: usize) -> Vec<InfoLine> {
    cells.chunks(per).map(<[_]>::to_vec).collect()
}

/// A Boss Rush's record as info lines.
fn record_lines(rec: &BossRushRecord) -> Vec<InfoLine> {
    let best = rec
        .best_clear_frames
        .map_or_else(|| "-".to_string(), clock_label);
    lines_of(
        vec![
            ("Runs".into(), rec.runs.to_string()),
            ("Clears".into(), rec.clears.to_string()),
            ("Most bosses".into(), rec.best_stages.to_string()),
            ("Best clear".into(), best),
        ],
        2,
    )
}

/// The Boss Rush panel the run tab shows under its rows: the record, then
/// the stages three to a line, the last line saying how many more the
/// window cannot hold.
fn boss_rush_info(s: &RulesSetup) -> Vec<InfoLine> {
    const STAGE_LINES: usize = 3;
    let mut lines = record_lines(&s.boss_rush_record);
    let mut cells: Vec<(String, String)> = s
        .boss_rush_stages
        .iter()
        .map(|(name, level)| (name.clone(), format!("Lv{level}")))
        .collect();
    if cells.len() > STAGE_LINES * 3 {
        let more = cells.len() - (STAGE_LINES * 3 - 1);
        cells.truncate(STAGE_LINES * 3 - 1);
        cells.push((format!("+{more} more"), String::new()));
    }
    // Every stage line has three cells, so a short last line stays on the
    // grid of the ones above it.
    while !cells.len().is_multiple_of(3) {
        cells.push((String::new(), String::new()));
    }
    lines.extend(lines_of(cells, 3));
    lines
}

/// The New Game rules screen. `tweaks` is [`stacking_tweaks_note`] of the
/// world, said on the difficulty tab.
pub fn setup_menu(s: &RulesSetup, keys: &PromptKeys, tweaks: Option<&str>) -> RulesMenu {
    let rows = s
        .row_list()
        .into_iter()
        .map(|r| {
            let (label, value) = s.row_text(r);
            RulesMenuRow {
                label,
                value,
                enabled: s.row_enabled(r),
                changes: s.row_changes(r),
            }
        })
        .collect();
    let info = match s.tab {
        // A preset lists what it scales (Custom lists them as rows).
        SetupTab::Difficulty if s.rules.difficulty != Difficulty::Custom => lines_of(
            s.rules
                .knobs()
                .rows()
                .into_iter()
                .map(|(l, v)| (l.to_string(), v))
                .collect(),
            2,
        ),
        SetupTab::Run if s.rules.mode == RunMode::BossRush => boss_rush_info(s),
        _ => Vec::new(),
    };
    let seed_grid = s.seed_editor.as_ref().map(|e| SeedGrid {
        text: e.text.clone(),
        cells: seed_grid()
            .into_iter()
            .map(|row| row.into_iter().map(|c| c.label()).collect())
            .collect(),
        cell: e.cell,
    });
    let list = s.overlay.map(|o| match o {
        SetupOverlay::Sources { cursor } => ListWindow {
            title: "Carry from which save?".into(),
            facts: Vec::new(),
            rows: s
                .source_rows()
                .into_iter()
                .map(|(label, value)| RulesMenuRow {
                    label,
                    value,
                    enabled: true,
                    changes: false,
                })
                .collect(),
            cursor,
        },
        SetupOverlay::Confirm { cursor } => ListWindow {
            title: "Begin with these rules?".into(),
            facts: s.confirm_lines(),
            rows: vec![RulesMenuRow::action("Begin"), RulesMenuRow::action("Back")],
            cursor,
        },
    });
    let hint = if seed_grid.is_some() && keys.keyboard {
        "Type  Backspace erase  Enter OK  Esc cancel".to_string()
    } else if seed_grid.is_some() {
        format!(
            "{} pick  {} erase  {} done",
            keys.confirm, keys.back, keys.start
        )
    } else if list.is_some() {
        format!("{} pick  {} close", keys.confirm, keys.back)
    } else {
        format!(
            "{}/{} tab  {} OK  {} begin  {} back  {} reset",
            keys.tab_prev, keys.tab_next, keys.confirm, keys.start, keys.back, keys.reset
        )
    };
    let mut help = s.help();
    if s.tab == SetupTab::Difficulty
        && s.overlay.is_none()
        && let Some(note) = tweaks
    {
        help = format!("{help} {note}");
    }
    RulesMenu {
        tabs: tab_labels(),
        tab: s.tab.index(),
        rows,
        cursor: Some(s.cursor()),
        info,
        help,
        summary: s
            .rules
            .summary()
            .unwrap_or_else(|| "Original rules".to_string()),
        hint,
        seed_grid,
        list,
    }
}

/// The Boss Rush screen this frame: the intermission while it is open, the
/// result once the run is over; `None` otherwise.
pub fn boss_rush_screen(world: &World, keys: &PromptKeys) -> Option<RulesMenu> {
    let run = world.run_rules.boss_rush.as_ref()?;
    if let Some((cleared, won, frames)) = world.boss_rush_result() {
        let rec = &world.run_rules.record;
        let mut info = vec![vec![
            (
                "Bosses beaten".to_string(),
                format!("{won} / {}", run.stages.len()),
            ),
            ("Time".to_string(), clock_label(frames)),
        ]];
        info.extend(record_lines(rec));
        return Some(RulesMenu {
            tabs: vec![
                if cleared {
                    "BOSS RUSH CLEAR"
                } else {
                    "BOSS RUSH OVER"
                }
                .into(),
            ],
            tab: 0,
            rows: Vec::new(),
            cursor: None,
            info,
            help: if cleared {
                "Every boss beaten. The record rides this session's saves."
            } else {
                "The run is over. The record rides this session's saves."
            }
            .into(),
            summary: world.run_rules_summary().unwrap_or_default(),
            hint: format!("{} return to title", keys.confirm),
            seed_grid: None,
            list: None,
        });
    }
    let m = world.run_rules.intermission.as_ref()?;
    let next = run.current().map(|s| s.name()).unwrap_or_default();
    let rows = Intermission::rows()
        .into_iter()
        .map(|r| RulesMenuRow::action(Intermission::row_label(r)))
        .collect();
    Some(RulesMenu {
        tabs: vec![format!("BOSS RUSH  {} / {}", run.stage, run.stages.len())],
        tab: 0,
        rows,
        cursor: Some(m.cursor),
        info: Vec::new(),
        help: m
            .message
            .clone()
            .unwrap_or_else(|| "Rest, buy supplies, or face the next boss.".to_string()),
        summary: format!(
            "Next: {next}   Time {}   Gold {}",
            clock_label(run.frames),
            world.party.money
        ),
        hint: format!("Up/Down choose  {} OK", keys.confirm),
        seed_grid: None,
        list: None,
    })
}

/// The run-rules screen that owns the frame, if one does: the New Game
/// rules screen while it is up, else the Boss Rush intermission or result.
/// `keys` name the buttons in its hint; no rect or sprite depends on them.
pub fn active_screen(world: &World, keys: &PromptKeys) -> Option<RulesMenu> {
    if let Some(seat) = world.run_rules.setup.as_deref() {
        let note = stacking_tweaks_note(world);
        return Some(setup_menu(&seat.setup, keys, note.as_deref()));
    }
    boss_rush_screen(world, keys)
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

/// The owning screen's chrome sprites, in surface pixels, for the host's
/// system-UI atlas pass.
pub fn run_rules_chrome_sprites(
    world: &World,
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    active_screen(world, &PromptKeys::default())
        .map_or_else(Vec::new, |s| s.sprites(rects, stage_origin, stage_scale))
}

/// What a tap at stage pixel `at` does on the New Game rules screen, `None`
/// when the screen is not up or the tap lands on nothing.
pub fn setup_tap(world: &World, at: (i32, i32)) -> Option<SetupTap> {
    let seat = world.run_rules.setup.as_deref()?;
    let menu = setup_menu(&seat.setup, &PromptKeys::default(), None);
    Some(match menu.hit(at)? {
        MenuHit::Tab(i) => SetupTap::Tab(i),
        MenuHit::Row(i) => SetupTap::Row(i),
        MenuHit::Prev(i) => SetupTap::Prev(i),
        MenuHit::Next(i) => SetupTap::Next(i),
        MenuHit::List(i) => SetupTap::Overlay(i),
        MenuHit::Cell(r, c) => SetupTap::Cell(r, c),
    })
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
    use legaia_engine_run_rules::setup::{NgSource, SetupInput};

    fn press(s: &mut RulesSetup, f: impl Fn(&mut SetupInput)) {
        let mut i = SetupInput::default();
        f(&mut i);
        s.step(i);
    }

    #[test]
    fn the_setup_screen_lists_the_rows_and_the_run() {
        let s = RulesSetup::new(vec![], 3);
        let screen = setup_menu(&s, &PromptKeys::default(), None);
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
        assert_eq!(setup_tap(&w, (10, 10)), None);
        assert_eq!(stacking_tweaks_note(&w), None);
    }

    #[test]
    fn the_difficulty_tab_lists_its_scales_and_the_tweaks_on_top() {
        let mut s = RulesSetup::new(vec![], 3);
        press(&mut s, |i| i.tab_next = true);
        press(&mut s, |i| i.right = true);
        let mut w = World::new();
        w.toggles.exp_rate_permille = 2000;
        let note = stacking_tweaks_note(&w);
        assert_eq!(note.as_deref(), Some("Tweaks stack on top: EXP x2."));
        let m = setup_menu(&s, &PromptKeys::default(), note.as_deref());
        assert_eq!(m.info.len(), 5);
        assert_eq!(m.info[0][0], ("Foe stats".to_string(), "x1.35".to_string()));
        assert_eq!(m.summary, "Hard");
        assert!(m.help.ends_with("EXP x2."), "{}", m.help);
        // Custom lists them as rows instead, each with its arrows.
        press(&mut s, |i| i.right = true);
        press(&mut s, |i| i.right = true);
        assert_eq!(s.rules.difficulty, Difficulty::Custom);
        let m = setup_menu(&s, &PromptKeys::default(), None);
        assert!(m.info.is_empty());
        assert_eq!(m.rows.len(), 11);
        assert!(m.rows.iter().all(|r| r.changes));
    }

    #[test]
    fn a_boss_rush_fills_the_run_tab_with_its_stages_and_record() {
        let mut s = RulesSetup::new(vec![], 3);
        s.boss_rush_stages = (0..12).map(|i| (format!("Boss{i}"), i as u8)).collect();
        s.boss_rush_record.runs = 4;
        press(&mut s, |i| i.up = true);
        press(&mut s, |i| i.up = true);
        press(&mut s, |i| i.right = true);
        let m = setup_menu(&s, &PromptKeys::default(), None);
        assert_eq!(m.info.len(), 5, "two record lines, three stage lines");
        assert_eq!(m.info[0][0], ("Runs".to_string(), "4".to_string()));
        assert_eq!(m.info[4][2].0, "+4 more");
        // Eight retail stages: the last line is padded onto the grid.
        s.boss_rush_stages.truncate(8);
        let m = setup_menu(&s, &PromptKeys::default(), None);
        assert!(m.info[2..].iter().all(|l| l.len() == 3));
    }

    #[test]
    fn the_confirm_and_the_save_list_are_windows_over_the_rows() {
        let src = NgSource {
            label: "Card 1 #1".into(),
            lead: "Vahn Lv 40".into(),
            detail: "Rim Elm  1:02:03".into(),
            cycle: 0,
        };
        let mut s = RulesSetup::new(vec![src], 3);
        press(&mut s, |i| i.tab_next = true);
        press(&mut s, |i| i.tab_next = true);
        press(&mut s, |i| i.confirm = true);
        let m = setup_menu(&s, &PromptKeys::default(), None);
        let l = m.list.as_ref().unwrap();
        assert_eq!(l.rows[1].value, "Rim Elm  1:02:03");
        press(&mut s, |i| i.down = true);
        press(&mut s, |i| i.confirm = true);
        press(&mut s, |i| i.start = true);
        let m = setup_menu(&s, &PromptKeys::default(), None);
        let l = m.list.as_ref().unwrap();
        assert_eq!(l.title, "Begin with these rules?");
        assert_eq!(l.facts[0].0, "New Game+ 1");
        assert!(m.strings().iter().any(|t| t == "Begin"));
    }

    #[test]
    fn the_hint_names_the_hosts_keys() {
        let s = RulesSetup::new(vec![], 3);
        let keys = PromptKeys::from_labels(|b| match b {
            "Cross" => Some("Z".into()),
            "Circle" => Some("X".into()),
            "Square" => Some("V".into()),
            "L1" => Some("Q".into()),
            "R1" => Some("E".into()),
            "Start" => Some("Enter".into()),
            _ => None,
        });
        assert_eq!(
            setup_menu(&s, &keys, None).hint,
            "Q/E tab  Z OK  Enter begin  X back  V reset"
        );
        // No rect depends on the key names.
        assert_eq!(
            setup_menu(&s, &keys, None).prims(),
            setup_menu(&s, &PromptKeys::default(), None).prims()
        );
    }
}
