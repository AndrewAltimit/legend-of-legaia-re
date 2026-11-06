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
/// the window size need a restart, so they stay on the launcher's list -
/// except the PlayStation pixel look and *Original game only*
/// ([`LIVE_SWITCH_ROWS`]), which take effect between two frames, as the
/// play page's two boxes do.
pub(super) const SETTINGS_ROWS: [OptionRow; 17] = [
    OptionRow::Fullscreen,
    OptionRow::CameraDistance,
    OptionRow::PsxRender,
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
    OptionRow::Retail,
];

/// The launch switches the page changes live. Each is remembered in the
/// launcher's settings file, where its row there reads it back.
#[cfg(test)]
pub(super) const LIVE_SWITCH_ROWS: [OptionRow; 2] = [OptionRow::PsxRender, OptionRow::Retail];

/// How many rows the plate shows at once; the list scrolls under it.
pub(super) const SHOWN_ROWS: usize = 15;

/// The first shown row that keeps `cursor` in view from `scroll`.
pub(super) fn scroll_to_cursor(scroll: usize, cursor: usize) -> usize {
    let max = SETTINGS_ROWS.len().saturating_sub(SHOWN_ROWS);
    if cursor < scroll {
        cursor
    } else if cursor >= scroll + SHOWN_ROWS {
        cursor + 1 - SHOWN_ROWS
    } else {
        scroll
    }
    .min(max)
}

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
const Y0: i32 = 2;
const WIDTH: i32 = 300;
const ROW_H: i32 = 11;
/// The row help's line budget: the longest row help wraps to this many.
const HELP_LINES: usize = 3;
const HELP_H: i32 = 12;
const CONTROLS: &str = "Arrows pick, change  Del default  Esc close";
const FLIP_HINT: &str = "L1/R1 or Tab: cheats";

/// The scrim under every page of the `F12` panel. Near-opaque: the panel
/// is read while the game holds still, and over the party readout's bright
/// digits a lighter scrim (0.82, 0.85) let them show through between the
/// rows. One value, so the pages do not change darkness as they flip.
pub(super) const PANEL_SCRIM: f32 = 0.94;
/// The space between the help lines and the controls line at the plate's
/// foot, so the controls never read as one more line of help.
const FOOTER_GAP: i32 = legaia_app_theme::spacing::CHROME_LINE_GAP;

/// The height of a Settings / Cheats plate: the heading, `rows` rows, a
/// `lines`-line help budget, the gap, and the controls line. The controls
/// line is the plate's last `HELP_H`.
pub(super) const fn plate_height(rows: usize, lines: usize) -> i32 {
    15 + rows as i32 * ROW_H + 3 + lines as i32 * HELP_H + FOOTER_GAP + HELP_H
}

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
                self.settings_scroll = scroll_to_cursor(self.settings_scroll, self.settings_cursor);
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
        self.edit_setting(|row, opts, s| row.adjust(opts, s, dir));
    }

    /// `Delete` / `Backspace`: the row under the cursor back on its default,
    /// as the launcher's `Delete` puts it.
    pub(super) fn reset_setting(&mut self) {
        self.edit_setting(|row, opts, s| row.reset(opts, s));
    }

    fn edit_setting(
        &mut self,
        edit: impl FnOnce(
            OptionRow,
            &mut legaia_engine_menus::options::OptionsState,
            &mut LauncherSettings,
        ),
    ) {
        let row = SETTINGS_ROWS[self.settings_cursor];
        if self.retail_session() && row.overridden_by_retail() {
            return;
        }
        if row == OptionRow::EnhancedLighting {
            // A launch switch may hold the renderer off the stored option:
            // toggle what is on screen, as the `I` key does.
            self.options_state.enhanced_lighting = self.dynamic_lighting;
        }
        // Every other row on the page is a game option: the launch switches
        // this scratch value carries are the live ones, seeded from the
        // window's own state.
        let mut switches = self.live_switches();
        edit(row, &mut self.options_state, &mut switches);
        match row {
            OptionRow::Fullscreen => self.win.set_fullscreen(self.options_state.fullscreen),
            OptionRow::PsxRender => {
                self.set_psx_render(switches.switches.psx_render);
                return;
            }
            OptionRow::Retail => {
                self.set_retail(switches.switches.retail);
                return;
            }
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

    /// The launch switches the page shows and changes, as they stand.
    fn live_switches(&self) -> LauncherSettings {
        let mut s = LauncherSettings::default();
        s.switches.psx_render = self.psx_render;
        s.switches.retail = self.retail_session();
        s
    }

    /// Whether this session holds to the original game (`--retail`, or the
    /// page's *Original game only* row).
    pub(super) fn retail_session(&self) -> bool {
        self.launch_switches.is_some_and(|s| s.retail)
    }

    /// Hold the session to the original game, or let it go: the retail
    /// option pins (`OptionsPins::pin_retail`, what `--retail` sets at
    /// launch) and the shared launch switch set re-applied with retail
    /// flipped, which holds every port-only world switch off or gives each
    /// back its launch value - the play page's Retail-faithful box, through
    /// the same two calls. Remembered for the launcher's next Play.
    pub(super) fn set_retail(&mut self, on: bool) {
        if on == self.retail_session() || self.launch_switches.is_none() {
            return;
        }
        if on {
            self.options_pins.pin_retail(&mut self.options_state);
        } else {
            self.options_pins.release_all(&mut self.options_state);
        }
        if let Some(sw) = self.launch_switches.as_mut() {
            sw.retail = on;
            sw.apply(&mut self.session.host.world.toggles);
        }
        self.dynamic_lighting = self.options_state.enhanced_lighting;
        if let Some(r) = self.win.renderer.as_ref() {
            r.set_dynamic_lighting(self.dynamic_lighting);
        }
        self.session.apply_option_knobs(&self.options_state, None);
        self.persist_and_apply_options();
        self.remember_switch(|s| s.retail = on);
    }

    /// Turn the PlayStation pixel look on or off between two frames: PSX
    /// rasterisation on the native 320x240 target, or clean edges at the
    /// window's size - the play page's *PSX rasterisation* box. The launcher
    /// remembers it ([`crate::launcher::LauncherSettings`]), so the next
    /// Play starts the way this one ends.
    pub(super) fn set_psx_render(&mut self, on: bool) {
        self.psx_render = on;
        if let Some(r) = self.win.renderer.as_mut() {
            r.set_psx_mode(on);
            r.set_native_resolution(on.then_some(legaia_engine_render::PSX_DISPLAY_SIZE));
        }
        self.remember_switch(|s| s.psx_render = on);
    }

    /// Write one launch switch into the launcher's settings file, where its
    /// launcher row reads it and the next Play passes it.
    fn remember_switch(
        &self,
        edit: impl FnOnce(&mut legaia_engine_session::session_switches::PersistedSwitches),
    ) {
        if self.scripted_run() {
            return;
        }
        let Some(path) = crate::launcher::settings_path() else {
            return;
        };
        // A file that does not parse is the launcher's to report; it is left
        // alone rather than replaced by one holding this switch alone.
        let mut stored = match LauncherSettings::load(&path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("settings: {} left alone: {e:#}", path.display());
                return;
            }
        };
        edit(&mut stored.switches);
        *self.options_save_error.borrow_mut() = match stored.save(&path) {
            Ok(()) => None,
            Err(e) => {
                log::warn!("settings: save to {} failed: {e:#}", path.display());
                Some("Could not save these settings (see the log)".to_string())
            }
        };
    }

    /// The settings page's draws, in stage pixels.
    pub(super) fn settings_stage_draws(&self) -> Vec<TextDraw> {
        let heading = chrome::HEADING.to_f32();
        let key = chrome::KEY.to_f32();
        let text = chrome::TEXT.to_f32();
        let changed = chrome::CHANGED.to_f32();
        let faint = legaia_app_theme::DARK.ink_faint.to_f32();
        let height = plate_height(SHOWN_ROWS.min(SETTINGS_ROWS.len()), HELP_LINES);
        let mut out = Vec::new();
        self.push_scrim(
            &mut out,
            (X0, Y0),
            (WIDTH as u32, height as u32),
            PANEL_SCRIM,
        );
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
        let switches = self.live_switches();
        let retail = switches.switches.retail;
        let mut y = Y0 + 15;
        let scroll = scroll_to_cursor(self.settings_scroll, self.settings_cursor);
        if SETTINGS_ROWS.len() > SHOWN_ROWS
            && let Some(src) = self.battle_hud_solid_src()
        {
            // The scroll bar: the whole list is the track, the view the thumb.
            let track_h = SHOWN_ROWS as i32 * ROW_H;
            let n = SETTINGS_ROWS.len() as i32;
            let thumb_h = track_h * SHOWN_ROWS as i32 / n;
            let max = n - SHOWN_ROWS as i32;
            let thumb_y = y + (track_h - thumb_h) * scroll as i32 / max.max(1);
            let x = X0 + WIDTH - 3;
            out.push(TextDraw {
                dst: (x, y, 1, track_h as u32),
                src,
                color: legaia_app_theme::DARK.rule.to_f32(),
            });
            out.push(TextDraw {
                dst: (x, thumb_y, 1, thumb_h as u32),
                src,
                color: text,
            });
        }
        for (i, row) in SETTINGS_ROWS
            .iter()
            .enumerate()
            .skip(scroll)
            .take(SHOWN_ROWS)
        {
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
            // Under *Original game only* a row it holds reads "Original",
            // faint, as on the launcher, and does not step.
            let muted = retail && row.overridden_by_retail();
            let value = if muted {
                "Original".to_string()
            } else if *row == OptionRow::EnhancedLighting {
                if self.dynamic_lighting { "On" } else { "Off" }.to_string()
            } else {
                row.value(&self.options_state, &switches)
            };
            let ink = match row.original_value() {
                _ if muted => faint,
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
        y = Y0 + height - HELP_H;
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
            assert!(
                row.is_game_option() || LIVE_SWITCH_ROWS.contains(row),
                "{row:?} needs a restart"
            );
            assert!(!SETTINGS_ROWS[..i].contains(row), "{row:?} twice");
        }
    }

    #[test]
    fn the_row_list_scrolls_to_keep_the_cursor_shown() {
        let n = SETTINGS_ROWS.len();
        assert_eq!(scroll_to_cursor(0, 0), 0);
        assert_eq!(scroll_to_cursor(0, SHOWN_ROWS - 1), 0);
        let last = scroll_to_cursor(0, n - 1);
        assert_eq!(last, n.saturating_sub(SHOWN_ROWS));
        assert!(n - 1 < last + SHOWN_ROWS);
        // Back up the list: the view follows only once the cursor leaves it.
        assert_eq!(scroll_to_cursor(last, last), last);
        assert_eq!(scroll_to_cursor(last, 0), 0);
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

    /// The Settings plate, the taller of the two, stays on the stage with
    /// its controls line clear of the help budget.
    #[test]
    fn the_plate_fits_the_stage_with_a_gap_above_the_controls() {
        let h = plate_height(SHOWN_ROWS, HELP_LINES);
        // The font's descenders reach 14 px below a line's origin.
        let controls_y = Y0 + h - HELP_H;
        assert!(
            controls_y + 14 <= legaia_engine_ui::BOOT_UI_STAGE_H as i32,
            "{h}"
        );
        let help_end = Y0 + 15 + SHOWN_ROWS as i32 * ROW_H + 3 + HELP_LINES as i32 * HELP_H;
        assert!(Y0 + h - HELP_H - help_end >= FOOTER_GAP);
    }

    #[test]
    fn wrap_keeps_every_word_in_order() {
        let width = |s: &str| s.len() as i32;
        let lines = wrap_lines("aa bb cc dd", 5, &width);
        assert_eq!(lines, vec!["aa bb", "cc dd"]);
    }
}
