//! The run rules' screens, projected once for both play hosts: the New Game
//! rules screen ([`setup_screen`]), the Boss Rush intermission and result
//! ([`boss_rush_screen`]), and the pause menu's run line
//! ([`run_summary_draws`]).
//!
//! Each returns the `legaia_engine_ui::run_rules_screen` panel's view
//! inputs; a host draws [`RulesScreen::prims`] under its text layer and
//! [`RulesScreen::texts`] with the rest of its stage group.

use legaia_engine_core::world::World;
use legaia_engine_run_rules::boss_rush::{Intermission, clock_label};
use legaia_engine_run_rules::setup::RulesSetup;
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::run_rules_screen::{
    RulesPanelView, rules_panel_prims, rules_panel_strings, rules_panel_texts,
};
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

/// The New Game rules screen.
pub fn setup_screen(s: &RulesSetup) -> RulesScreen {
    RulesScreen {
        title: "NEW GAME".into(),
        subtitle: Some(
            s.rules
                .summary()
                .unwrap_or_else(|| "Original rules".to_string()),
        ),
        rows: s.rows(),
        cursor: Some(s.cursor),
        footer: Some(s.help().to_string()),
        hint: "Arrows change  X confirm  Start play  O back".into(),
    }
}

/// The Boss Rush screen this frame: the intermission while it is open, the
/// result once the run is over; `None` otherwise.
pub fn boss_rush_screen(world: &World) -> Option<RulesScreen> {
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
            hint: "X return to title".into(),
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
        hint: "Up/Down choose  X confirm".into(),
    })
}

/// The run-rules screen that owns the frame, if one does: the New Game
/// rules screen while it is up, else the Boss Rush intermission or result.
pub fn active_screen(world: &World) -> Option<RulesScreen> {
    if let Some(seat) = world.run_rules.setup.as_deref() {
        return Some(setup_screen(&seat.setup));
    }
    boss_rush_screen(world)
}

/// The owning screen's labels, in stage pixels (each host's text layer
/// scales and appends them, as it does the expansion screens').
pub fn run_rules_stage_texts(world: &World, font: &legaia_font::Font) -> Vec<TextDraw> {
    active_screen(world).map_or_else(Vec::new, |s| s.texts(font))
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
        let screen = setup_screen(&s);
        let strings = screen.strings();
        assert!(strings.iter().any(|t| t == "Difficulty"));
        assert!(strings.iter().any(|t| t == "Original rules"));
        assert!(!screen.prims().is_empty());
        let w = World::new();
        assert_eq!(boss_rush_screen(&w), None);
    }
}
