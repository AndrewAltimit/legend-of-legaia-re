//! The in-game settings page: the window's `F12` panel, also `Select` +
//! `Start` for a controller, which no toggle key reaches.
//!
//! It lists the launcher's rows the game itself reads ([`SETTINGS_ROWS`]),
//! with the launcher's words and the same green for a value the original
//! game does not have, and writes the same `legaia-options.toml`, so a change
//! made in either place is the one the other shows. Up / Down pick a row,
//! Left / Right change it, Cross steps it; `L1` / `R1` (or `Tab`) flip to the
//! key list, Circle, Start, `F12` or `Esc` close the panel. The game holds
//! still under the panel, except in a multiplayer session, whose peers would
//! stall: there `F12` shows the key list alone and takes no pad input. The
//! pages run Settings, Cheats (`window::cheats_panel`), then the key list.
//!
//! The browser play page has no twin of this page and needs none: its
//! *Visual enhancements* panel and sound sliders sit beside the canvas, in
//! reach of the mouse without leaving the game.

use super::*;
use crate::launcher::{LauncherSettings, OptionRow};
use legaia_app_theme::chrome;
use legaia_engine_system::input::PadButton;

/// The rows on the page: what the game reads live. The launch switches and
/// the window size need a restart, so they stay on the launcher's list.
pub(super) const SETTINGS_ROWS: [OptionRow; 15] = [
    OptionRow::Fullscreen,
    OptionRow::CameraDistance,
    OptionRow::ReduceFlashing,
    OptionRow::EnhancedLighting,
    OptionRow::TimeOfDay,
    OptionRow::LampShadows,
    OptionRow::Bloom,
    OptionRow::AmbientOcclusion,
    OptionRow::GroundFog,
    OptionRow::SeeThroughWalls,
    OptionRow::PreciseMovement,
    OptionRow::MasterVolume,
    OptionRow::MusicVolume,
    OptionRow::SfxVolume,
    OptionRow::MonoDownmix,
];

/// Which page of the `F12` panel is up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum PanelPage {
    #[default]
    Settings,
    Cheats,
    Keys,
}

/// What a frame's pad edges ask of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelInput {
    /// Move the row cursor by this many rows.
    Move(i32),
    /// Step the row's value: forward for a positive direction.
    Change(i8),
    /// Cross: step a setting forward, apply a cheat.
    Confirm,
    Flip,
    Close,
}

/// A frame's pad edges as a panel action. Up and Down come first, so a
/// diagonal on a stick moves the cursor and changes nothing by accident.
pub(super) fn panel_input(edge: u16) -> Option<PanelInput> {
    let hit = |b: PadButton| edge & b.mask() != 0;
    if hit(PadButton::Circle) || hit(PadButton::Start) {
        Some(PanelInput::Close)
    } else if hit(PadButton::L1) || hit(PadButton::R1) {
        Some(PanelInput::Flip)
    } else if hit(PadButton::Up) {
        Some(PanelInput::Move(-1))
    } else if hit(PadButton::Down) {
        Some(PanelInput::Move(1))
    } else if hit(PadButton::Left) {
        Some(PanelInput::Change(-1))
    } else if hit(PadButton::Right) {
        Some(PanelInput::Change(1))
    } else if hit(PadButton::Cross) {
        Some(PanelInput::Confirm)
    } else {
        None
    }
}

/// The controller's way in: `Start` pressed with `Select` held. `L1` / `R1`
/// held leave the press alone: with both of those it is the original game's
/// soft reset.
pub(super) fn settings_chord(held: u16, edge: u16) -> bool {
    held & PadButton::Select.mask() != 0
        && edge & PadButton::Start.mask() != 0
        && held & (PadButton::L1.mask() | PadButton::R1.mask()) == 0
}

/// Break `text` into lines no wider than `max_w` by the `width` metric.
pub(super) fn wrap_lines(text: &str, max_w: i32, width: &dyn Fn(&str) -> i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if width(&next) > max_w && !cur.is_empty() {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Stage geometry of the settings page. Its plate is translucent and narrower
/// than the stage, so a toggled light or fog shows around it.
const X0: i32 = 10;
const Y0: i32 = 5;
const WIDTH: i32 = 300;
const ROW_H: i32 = 11;
/// The row help's line budget: the longest row help wraps to this many.
const HELP_LINES: usize = 3;
const HELP_H: i32 = 12;
const CONTROLS: &str = "Up/Down pick  Left/Right change  Esc close";
const FLIP_HINT: &str = "L1/R1 or Tab: cheats";

impl PlayWindowApp {
    /// Whether the panel owns the frame: it is up, in a window a player is at
    /// (a scripted capture counts ticks to its frame), and no multiplayer
    /// peer is waiting on this machine's ticks.
    pub(super) fn panel_holds_game(&self) -> bool {
        self.hotkey_help
            && !self.scripted_run()
            && self.win.renderer.is_some()
            && self.netplay.is_none()
            && self.netplay_menu.is_none()
    }

    /// The page to draw: a session that cannot hold still gets the key list.
    pub(super) fn panel_page(&self) -> PanelPage {
        if self.netplay.is_some() || self.netplay_menu.is_some() {
            PanelPage::Keys
        } else {
            self.panel_page
        }
    }

    /// Open the panel on its first page; a movie's frame has no chrome to
    /// draw it in, so a press under one does nothing.
    pub(super) fn open_panel(&mut self) {
        if self.cutscene.is_some() {
            return;
        }
        self.hotkey_help = true;
        self.panel_page = PanelPage::Settings;
        self.startup_hint.dismiss();
    }

    /// One frame's pad edges while the panel owns the frame.
    pub(super) fn panel_pad_edge(&mut self, edge: u16) {
        let Some(input) = panel_input(edge) else {
            return;
        };
        let page = self.panel_page();
        match input {
            PanelInput::Close => self.hotkey_help = false,
            PanelInput::Flip => self.flip_panel(),
            PanelInput::Move(d) if page == PanelPage::Settings => {
                let n = SETTINGS_ROWS.len() as i32;
                self.settings_cursor = (self.settings_cursor as i32 + d).rem_euclid(n) as usize;
            }
            PanelInput::Change(dir) if page == PanelPage::Settings => self.change_setting(dir),
            PanelInput::Confirm if page == PanelPage::Settings => self.change_setting(1),
            PanelInput::Move(d) if page == PanelPage::Cheats => self.cheats_page.move_cursor(d),
            PanelInput::Change(dir) if page == PanelPage::Cheats => self.cheats_change(dir),
            PanelInput::Confirm if page == PanelPage::Cheats => self.cheats_apply(),
            PanelInput::Move(_) | PanelInput::Change(_) | PanelInput::Confirm => {}
        }
    }

    /// `Tab`, `L1`, `R1`: the other page.
    pub(super) fn flip_panel(&mut self) {
        self.panel_page = match self.panel_page {
            PanelPage::Settings => PanelPage::Cheats,
            PanelPage::Cheats => PanelPage::Keys,
            PanelPage::Keys => PanelPage::Settings,
        };
    }

    /// Step the row under the cursor and push it to everything that reads it,
    /// the same way the row's toggle key does.
    pub(super) fn change_setting(&mut self, dir: i8) {
        let row = SETTINGS_ROWS[self.settings_cursor];
        if row == OptionRow::EnhancedLighting {
            // A launch switch may hold the renderer off the stored option:
            // toggle what is on screen, as the `I` key does.
            self.options_state.enhanced_lighting = self.dynamic_lighting;
        }
        // Every row on the page is a game option: the launch switches this
        // scratch value would carry are not on it.
        let mut switches = LauncherSettings::default();
        row.adjust(&mut self.options_state, &mut switches, dir);
        match row {
            OptionRow::Fullscreen => self.win.set_fullscreen(self.options_state.fullscreen),
            OptionRow::EnhancedLighting => {
                self.dynamic_lighting = self.options_state.enhanced_lighting;
                if let Some(r) = self.win.renderer.as_ref() {
                    r.set_dynamic_lighting(self.dynamic_lighting);
                }
            }
            _ => {}
        }
        self.session.apply_option_knobs(&self.options_state, None);
        self.persist_and_apply_options();
    }

    /// The settings page's draws, in stage pixels.
    pub(super) fn settings_stage_draws(&self) -> Vec<TextDraw> {
        let heading = chrome::HEADING.to_f32();
        let key = chrome::KEY.to_f32();
        let text = chrome::TEXT.to_f32();
        let changed = chrome::CHANGED.to_f32();
        let rows_h = SETTINGS_ROWS.len() as i32 * ROW_H;
        let height = 14 + rows_h + 3 + HELP_LINES as i32 * HELP_H + HELP_H + 2;
        let mut out = Vec::new();
        self.push_scrim(&mut out, (X0, Y0), (WIDTH as u32, height as u32), 0.82);
        let width = |s: &str| self.font.layout_ascii(s).advance_x as i32;
        let put = |out: &mut Vec<TextDraw>, s: &str, at: (i32, i32), color: [f32; 4]| {
            out.extend(legaia_engine_ui::text_draws_for(
                &self.font.layout_ascii(s),
                at,
                color,
            ));
        };
        let left = X0 + 6;
        let right = X0 + WIDTH - 6;
        put(&mut out, "Settings", (left, Y0 + 1), heading);
        put(
            &mut out,
            FLIP_HINT,
            (right - width(FLIP_HINT), Y0 + 1),
            text,
        );
        let switches = LauncherSettings::default();
        let mut y = Y0 + 15;
        for (i, row) in SETTINGS_ROWS.iter().enumerate() {
            let focused = i == self.settings_cursor;
            if focused && let Some(src) = self.battle_hud_solid_src() {
                out.push(TextDraw {
                    dst: (X0 + 2, y, (WIDTH - 4) as u32, ROW_H as u32),
                    src,
                    color: legaia_app_theme::DARK.hover.to_f32(),
                });
            }
            let label_at = (left, y - 2);
            put(&mut out, row.label(), label_at, text);
            let value = if *row == OptionRow::EnhancedLighting {
                if self.dynamic_lighting { "On" } else { "Off" }.to_string()
            } else {
                row.value(&self.options_state, &switches)
            };
            let ink = match row.original_value() {
                Some(o) if o != value => changed,
                _ if focused => key,
                _ => text,
            };
            let shown = if focused {
                format!("< {value} >")
            } else {
                value
            };
            put(&mut out, &shown, (right - width(&shown), y - 2), ink);
            y += ROW_H;
        }
        y += 3;
        // The row's help, then the controls or what went wrong saving.
        let row = SETTINGS_ROWS[self.settings_cursor];
        let help = wrap_lines(row.help(), WIDTH - 12, &width);
        for line in help.iter().take(HELP_LINES) {
            put(&mut out, line, (left, y), text);
            y += HELP_H;
        }
        y = Y0 + height - HELP_H - 2;
        match self.options_save_error.borrow().as_deref() {
            Some(e) => put(&mut out, e, (left, y), heading),
            None => put(&mut out, CONTROLS, (left, y), heading),
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::app_state::STARTUP_HINT_TEXT as STARTUP_LINE_FOR_TEST;

    #[test]
    fn page_rows_are_game_options_and_unique() {
        for (i, row) in SETTINGS_ROWS.iter().enumerate() {
            assert!(row.is_game_option(), "{row:?} needs a restart");
            assert!(!SETTINGS_ROWS[..i].contains(row), "{row:?} twice");
        }
    }

    #[test]
    fn chord_needs_select_held_and_leaves_the_soft_reset_alone() {
        let (sel, start) = (PadButton::Select.mask(), PadButton::Start.mask());
        assert!(settings_chord(sel | start, start));
        assert!(
            !settings_chord(start, start),
            "Start alone is the pause menu"
        );
        assert!(!settings_chord(sel | start, 0), "only on Start's press");
        let reset = sel | start | PadButton::L1.mask() | PadButton::R1.mask();
        assert!(!settings_chord(reset, start));
    }

    #[test]
    fn pad_edges_map_to_panel_actions() {
        use PanelInput::*;
        let cases = [
            (PadButton::Up, Move(-1)),
            (PadButton::Down, Move(1)),
            (PadButton::Left, Change(-1)),
            (PadButton::Right, Change(1)),
            (PadButton::Cross, Confirm),
            (PadButton::L1, Flip),
            (PadButton::R1, Flip),
            (PadButton::Circle, Close),
            (PadButton::Start, Close),
        ];
        for (b, want) in cases {
            assert_eq!(panel_input(b.mask()), Some(want), "{b:?}");
        }
        assert_eq!(panel_input(0), None);
        assert_eq!(panel_input(PadButton::Square.mask()), None);
    }

    /// Every row's help fits the page's budget in the disc's dialog font, the
    /// one the game's window always draws in. Disc-gated: the built-in font's
    /// fixed pitch is nothing a player's page is ever lettered in.
    #[test]
    fn every_help_fits_its_lines_and_the_controls_fit_the_plate() {
        let font = std::env::var_os("LEGAIA_DISC_BIN")
            .and_then(|p| crate::launcher::disc_font(std::path::Path::new(&p)));
        if let Some(font) = font {
            let width = |s: &str| font.layout_ascii(s).advance_x as i32;
            for row in SETTINGS_ROWS {
                let lines = wrap_lines(row.help(), WIDTH - 12, &width);
                assert!(lines.len() <= HELP_LINES, "{row:?}: {lines:?}");
            }
            assert!(width(CONTROLS) <= WIDTH - 12, "{}", width(CONTROLS));
            assert!(width(STARTUP_LINE_FOR_TEST) <= 310);
        }
    }

    #[test]
    fn wrap_keeps_every_word_in_order() {
        let width = |s: &str| s.len() as i32;
        let lines = wrap_lines("aa bb cc dd", 5, &width);
        assert_eq!(lines, vec!["aa bb", "cc dd"]);
    }
}
