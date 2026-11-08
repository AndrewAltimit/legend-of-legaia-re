//! The `F12` panel's Tools page: the debug switches that otherwise take a
//! launch flag or an environment variable, and the minigame and game-mode
//! entries that otherwise take a keyboard key, so a controller player and a
//! player with no terminal reach them.
//!
//! Every switch is for this session only, as the browser play page's *Dev
//! menu* box and its battle tools are: nothing here is written to a settings
//! file, so a developer tool cannot come back on by itself on a later start.
//! The browser play page's twins are its dev-menu box, Auto-battle box,
//! *Overworld markers* box, *Retail battle cadence* box and Minigame list.

use super::*;
use legaia_app_theme::chrome;

/// One row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolRow {
    /// The original game's developer menu (`LEGAIA_DEV_MENU`).
    DevMenu,
    /// The window's diagnostic rows (`F1`).
    Diagnostics,
    /// The wide orbit vantage (`F3`).
    DebugCamera,
    /// Party turns resolve themselves (`--no-player-battle`).
    AutoBattle,
    /// The battle pass on retail's cadence (`LEGAIA_BATTLE_FRAME_CADENCE`).
    BattleCadence,
    /// The port's overworld markers (`--world-map-markers`).
    OverworldMarkers,
    /// A press of the window key that enters a minigame or a mode.
    Enter(KeyCode),
}

/// The rows: control, label, one line of help.
pub(super) const TOOL_ROWS: [(ToolRow, &str, &str); 13] = [
    (
        ToolRow::DevMenu,
        "Developer menu",
        "The original game's debug screens over the scene. Square on its list: records.",
    ),
    (
        ToolRow::Diagnostics,
        "Diagnostics",
        "Scene, frame, mesh and camera readouts on the screen. Key: F1.",
    ),
    (
        ToolRow::DebugCamera,
        "Debug camera",
        "A wide orbit view of the field instead of the follow camera. Key: F3.",
    ),
    (
        ToolRow::AutoBattle,
        "Auto-battle",
        "Each party turn attacks the first living monster by itself.",
    ),
    (
        ToolRow::BattleCadence,
        "Battle cadence",
        "On: battles advance once a battle frame, as the original does. Off: once a display frame.",
    ),
    (
        ToolRow::OverworldMarkers,
        "Overworld markers",
        "A marker over each place on the world map. Not in the original game.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyL),
        "Fishing",
        "Start fishing from this scene. Key: L.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyO),
        "Slot machine",
        "Start the casino slots from this scene. Key: O.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyB),
        "Baka Fighter",
        "Start a Baka Fighter duel from this scene. Key: B.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyM),
        "Muscle Dome",
        "Start the Muscle Dome ladder from this scene. Key: M.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyK),
        "Dance",
        "Start the dance minigame from this scene. Key: K.",
    ),
    (
        ToolRow::Enter(KeyCode::Backquote),
        "Game modes",
        "The port's own modes: the expansion pack's, or the built-in practice set. Key: `.",
    ),
    (
        ToolRow::Enter(KeyCode::Backslash),
        "Trading cards",
        "The expansion pack's card menu: binder, boosters, deck. Needs a pack with cards. Key: \\.",
    ),
];

/// The page's rows.
pub(super) const TOOL_COUNT: usize = TOOL_ROWS.len();

/// The cursor and the last row's refusal.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ToolsPage {
    pub(super) cursor: usize,
    /// Why the last press did nothing.
    note: Option<&'static str>,
}

impl ToolsPage {
    /// Move the cursor by `d` rows, wrapping, and drop the last note.
    pub(super) fn move_cursor(&mut self, d: i32) {
        self.cursor = (self.cursor as i32 + d).rem_euclid(TOOL_COUNT as i32) as usize;
        self.note = None;
    }
}

const X0: i32 = 10;
const Y0: i32 = 2;
const WIDTH: i32 = 300;
const ROW_H: i32 = 11;
const HELP_H: i32 = 12;
const NOTE_LINES: usize = 2;
const CONTROLS: &str = "Up/Down pick  Cross use  Esc close";
const FLIP_HINT: &str = "L1/R1 or Tab: keys";
/// What a refused minigame press says.
const REFUSED: &str = "Not from here: stand in a town, a field or on the world map.";

impl PlayWindowApp {
    /// Where the row's switch stands; `None` for an entry row.
    fn tool_on(&self, row: ToolRow) -> Option<bool> {
        let world = &self.session.host.world;
        Some(match row {
            ToolRow::DevMenu => self.dev_menu_on,
            ToolRow::Diagnostics => self.diag_rows,
            ToolRow::DebugCamera => self.field_debug_camera,
            ToolRow::AutoBattle => !world.battle.player_driven,
            ToolRow::BattleCadence => world.toggles.battle_sm_frame_cadence,
            ToolRow::OverworldMarkers => world.toggles.overworld_marker_overlay,
            ToolRow::Enter(_) => return None,
        })
    }

    /// Left / Right on the page: a switch flips, an entry row does nothing.
    pub(super) fn tools_change(&mut self) {
        let (row, _, _) = TOOL_ROWS[self.tools_page.cursor];
        if !matches!(row, ToolRow::Enter(_)) {
            self.tools_apply();
        }
    }

    /// Cross / Enter on the focused row.
    pub(super) fn tools_apply(&mut self) {
        let (row, _, _) = TOOL_ROWS[self.tools_page.cursor];
        self.tools_page.note = None;
        match row {
            ToolRow::DevMenu => self.set_dev_menu(!self.dev_menu_on),
            ToolRow::Diagnostics => self.diag_rows ^= true,
            ToolRow::DebugCamera => self.field_debug_camera ^= true,
            ToolRow::AutoBattle => {
                let driven = !self.session.host.world.battle.player_driven;
                self.session.host.world.battle.player_driven = driven;
                self.field_live_opts.player_battle = driven;
            }
            ToolRow::BattleCadence => {
                let toggles = &mut self.session.host.world.toggles;
                let on = !toggles.battle_sm_frame_cadence;
                toggles.battle_sm_frame_cadence = on;
                if let Some(sw) = self.launch_switches.as_mut() {
                    sw.battle_frame_cadence = on;
                }
            }
            ToolRow::OverworldMarkers => {
                if self.retail_session() {
                    self.tools_page.note = Some("Off while Original game only is on.");
                    return;
                }
                let toggles = &mut self.session.host.world.toggles;
                let on = !toggles.overworld_marker_overlay;
                toggles.overworld_marker_overlay = on;
                if let Some(sw) = self.launch_switches.as_mut() {
                    sw.world_map_markers = on;
                }
            }
            ToolRow::Enter(key) => {
                if self.minigame_key(key, ElementState::Pressed) {
                    self.hotkey_help = false;
                } else {
                    self.tools_page.note = Some(REFUSED);
                }
            }
        }
    }

    /// The Tools page's draws, in stage pixels.
    pub(super) fn tools_stage_draws(&self) -> Vec<TextDraw> {
        let heading = chrome::HEADING.to_f32();
        let key_ink = chrome::KEY.to_f32();
        let text = chrome::TEXT.to_f32();
        let changed = chrome::CHANGED.to_f32();
        let height = super::settings_panel::plate_height(TOOL_COUNT, NOTE_LINES);
        let mut out = Vec::new();
        self.push_scrim(
            &mut out,
            (X0, Y0),
            (WIDTH as u32, height as u32),
            super::settings_panel::PANEL_SCRIM,
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
        put(&mut out, "Tools", (left, Y0 + 1), heading);
        put(
            &mut out,
            FLIP_HINT,
            (right - width(FLIP_HINT), Y0 + 1),
            text,
        );
        let mut y = Y0 + 15;
        for (i, (row, label, _)) in TOOL_ROWS.iter().enumerate() {
            let focused = i == self.tools_page.cursor;
            if focused && let Some(src) = self.battle_hud_solid_src() {
                out.push(TextDraw {
                    dst: (X0 + 2, y, (WIDTH - 4) as u32, ROW_H as u32),
                    src,
                    color: legaia_app_theme::DARK.hover.to_f32(),
                });
            }
            put(&mut out, label, (left, y - 2), text);
            let on = self.tool_on(*row);
            let shown = match (on, focused) {
                (Some(on), true) => format!("< {} >", if on { "On" } else { "Off" }),
                (Some(on), false) => if on { "On" } else { "Off" }.to_string(),
                (None, true) => "Cross: start".to_string(),
                (None, false) => String::new(),
            };
            let ink = match (on, focused) {
                (Some(true), _) => changed,
                (_, true) => key_ink,
                _ => text,
            };
            put(&mut out, &shown, (right - width(&shown), y - 2), ink);
            y += ROW_H;
        }
        y += 3;
        let note = self
            .tools_page
            .note
            .unwrap_or(TOOL_ROWS[self.tools_page.cursor].2);
        for line in super::settings_panel::wrap_lines(note, WIDTH - 12, &width)
            .iter()
            .take(NOTE_LINES)
        {
            put(&mut out, line, (left, y), text);
            y += HELP_H;
        }
        put(&mut out, CONTROLS, (left, Y0 + height - HELP_H), heading);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_wraps_and_a_move_clears_the_refusal() {
        let mut p = ToolsPage {
            note: Some(REFUSED),
            ..Default::default()
        };
        p.move_cursor(-1);
        assert_eq!(p.cursor, TOOL_COUNT - 1);
        assert_eq!(p.note, None);
        p.move_cursor(1);
        assert_eq!(p.cursor, 0);
    }

    /// The page stays on the stage with the row count it carries.
    #[test]
    fn the_plate_fits_the_stage() {
        let h = super::super::settings_panel::plate_height(TOOL_COUNT, NOTE_LINES);
        assert!(Y0 + h <= legaia_engine_ui::BOOT_UI_STAGE_H as i32, "{h}");
    }

    /// Every help line and the controls line fit the plate, in the disc's
    /// dialog font the window letters them in.
    #[test]
    fn every_note_and_the_controls_fit_the_plate() {
        let font = std::env::var_os("LEGAIA_DISC_BIN")
            .and_then(|p| crate::launcher::disc_font(std::path::Path::new(&p)));
        if let Some(font) = font {
            let width = |s: &str| font.layout_ascii(s).advance_x as i32;
            for (_, label, help) in TOOL_ROWS {
                let lines = super::super::settings_panel::wrap_lines(help, WIDTH - 12, &width);
                assert!(lines.len() <= NOTE_LINES, "{label}: {lines:?}");
                assert!(width(label) + width("< Off >") <= WIDTH - 12, "{label}");
            }
            let lines = super::super::settings_panel::wrap_lines(REFUSED, WIDTH - 12, &width);
            assert!(lines.len() <= NOTE_LINES, "{lines:?}");
            assert!(width(CONTROLS) <= WIDTH - 12, "{}", width(CONTROLS));
        }
    }
}
