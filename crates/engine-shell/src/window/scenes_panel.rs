//! The `F12` panel's Scenes page: go to any town, field area or overworld
//! while the game runs - the native twin of the browser play page's *Scenes*
//! list. Before this page a desktop player reached another scene only as the
//! `--scene` launch switch.
//!
//! The list is the one scene-name table (`legaia_gamedata`'s `scenes.toml`,
//! which also feeds the page's list) cut to the categories the page offers -
//! towns, field areas and dungeons, the three kingdom overworlds - in the
//! page's group order. Cross or Enter enters the focused scene through
//! `BootSession::enter_scene_live`, the entry the page's `enter_field` and a
//! door transition take, then rebuilds the window's render state the way a
//! door does, after the two picker rules the page and the `--scene` entry
//! share: the scene staged at its free-roam visit and the party seated where
//! the disc's doors land it. The party and the bag come along.
//!
//! The page refuses where a jump would tear something down mid-way: a
//! front-end screen (logos, title, save select, pause menu, game over), a
//! fight, a movie or a minigame. The refusal is the note line, so the press
//! never looks dead.

use super::*;

/// The scene-table categories the page offers, in its group order, with the
/// heading each group shows. The play page's `GROUPS`.
const GROUPS: [(&str, &str); 3] = [
    ("town", "Towns"),
    ("field", "Field areas"),
    ("world_map", "World map"),
];

/// One row: the CDNAME label the entry takes, the display name, and the
/// index of its group in [`GROUPS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SceneRow {
    pub(super) label: String,
    pub(super) name: String,
    group: usize,
}

/// The page's rows, built once from the scene table.
pub(super) fn scene_rows() -> &'static [SceneRow] {
    static ROWS: std::sync::OnceLock<Vec<SceneRow>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        let db = legaia_gamedata::Database::load();
        let mut rows = Vec::new();
        for (group, (category, _)) in GROUPS.iter().enumerate() {
            for s in db.scene_names().iter().filter(|s| s.category == *category) {
                if rows.iter().any(|r: &SceneRow| r.label == s.id) {
                    continue;
                }
                rows.push(SceneRow {
                    label: s.id.clone(),
                    name: s.name.clone(),
                    group,
                });
            }
        }
        rows
    })
}

/// How many rows the plate shows at once; the list scrolls under it.
const SHOWN: usize = 15;
/// Left / Right move the cursor a page of rows.
pub(super) const PAGE_STEP: i32 = SHOWN as i32;

/// The page's own state: the cursor and the first row shown.
#[derive(Debug, Clone, Default)]
pub(super) struct ScenesPage {
    pub(super) cursor: usize,
    scroll: usize,
    /// Put on the scene the player stands in at the first open.
    placed: bool,
    /// Why the last press did nothing, until the cursor moves.
    pub(super) refused: Option<&'static str>,
}

impl ScenesPage {
    /// Move the cursor by `d` rows over `n`; a single step wraps, a page
    /// step holds at the ends.
    pub(super) fn move_cursor(&mut self, d: i32, n: usize) {
        if n == 0 {
            return;
        }
        let last = n as i32 - 1;
        let to = self.cursor as i32 + d;
        self.cursor = if d.abs() == 1 {
            to.rem_euclid(n as i32)
        } else {
            to.clamp(0, last)
        } as usize;
        self.refused = None;
        self.keep_in_view(n);
    }

    /// Put the cursor on `label` (the scene the player is in), once.
    fn place(&mut self, label: Option<&str>, rows: &[SceneRow]) {
        if self.placed {
            return;
        }
        self.placed = true;
        if let Some(i) = label.and_then(|l| rows.iter().position(|r| r.label == l)) {
            self.cursor = i;
            self.scroll = i.saturating_sub(SHOWN / 2);
            self.keep_in_view(rows.len());
        }
    }

    fn keep_in_view(&mut self, n: usize) {
        let max = n.saturating_sub(SHOWN);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + SHOWN {
            self.scroll = self.cursor + 1 - SHOWN;
        }
        self.scroll = self.scroll.min(max);
    }
}

/// Stage geometry, matching the settings and cheats pages' plate.
const X0: i32 = 10;
const Y0: i32 = 2;
const WIDTH: i32 = 300;
const ROW_H: i32 = 11;
const HELP_H: i32 = 12;
/// The note line budget under the rows.
const NOTE_LINES: usize = 2;
const CONTROLS: &str = "Up/Down pick  Left/Right page  Cross go";
/// [`CONTROLS`] on a controller (`settings_panel::pad_controls`).
const PAD_CONTROLS: &str = "D-pad pick, page  {cross} go";
const FLIP_HINT: &str = "L1/R1 or Tab: tools";

impl PlayWindowApp {
    /// Why a jump cannot be made now, if it cannot.
    fn scene_jump_refusal(&self) -> Option<&'static str> {
        if self.boot_ui.is_active() {
            return Some("Start or continue a game first, then pick a scene.");
        }
        if self.cutscene.is_some() {
            return Some("Not during a movie.");
        }
        match self.session.host.world.mode {
            SceneMode::Field | SceneMode::WorldMap => None,
            SceneMode::Battle => Some("Not during a fight: finish it first."),
            _ => Some("Not here: walk back into the field first."),
        }
    }

    /// The scene the player stands in; none under a front-end screen, whose
    /// backdrop scene is nobody's position.
    fn player_scene(&self) -> Option<String> {
        if self.boot_ui.is_active() {
            return None;
        }
        self.session.host.scene.as_ref().map(|s| s.name.clone())
    }

    /// Up / Down on the Scenes page (`d` = 1 row) and Left / Right (a page).
    pub(super) fn scenes_move(&mut self, d: i32) {
        self.scenes_page.move_cursor(d, scene_rows().len());
    }

    /// Cross / Enter on the Scenes page: go to the focused scene.
    pub(super) fn scenes_go(&mut self) {
        let Some(row) = scene_rows().get(self.scenes_page.cursor) else {
            return;
        };
        if let Some(why) = self.scene_jump_refusal() {
            self.scenes_page.refused = Some(why);
            return;
        }
        // The picker rules every host's picker asks: a field scene is staged
        // at its free-roam visit (`World::stage_picker_entry`), and the party
        // is seated where the disc's doors land it
        // (`SceneHost::arm_picker_entry_seat`) - the page's `enter_field` and
        // the `--scene` entry make the same two calls.
        let world_map = legaia_engine_scene::scene::is_world_map_scene(&row.label);
        if !world_map {
            self.session
                .host
                .world
                .stage_picker_entry(&row.label, false);
        }
        self.session.host.arm_picker_entry_seat(&row.label);
        let opts = self.field_live_opts.clone();
        match self.session.enter_scene_live(&row.label, &opts) {
            Ok(mode) => {
                if world_map {
                    self.session.host.world.arm_picker_world_map_debug();
                }
                log::info!("scenes page: entered '{}' (mode={mode:?})", row.label);
                self.rebuild_scene_render_state();
                self.hotkey_help = false;
                self.scenes_page.refused = None;
            }
            Err(e) => {
                log::warn!("scenes page: enter '{}' failed ({e:#})", row.label);
                self.show_notice(format!("{} did not load", row.name));
            }
        }
    }

    /// The page comes up: the first time, on the scene the player is in.
    pub(super) fn scenes_page_shown(&mut self) {
        let here = self.player_scene();
        self.scenes_page.place(here.as_deref(), scene_rows());
    }

    /// The Scenes page's draws, in stage pixels.
    pub(super) fn scenes_stage_draws(&self) -> Vec<TextDraw> {
        let rows = scene_rows();
        let here = self.player_scene();
        let heading = self.chrome().heading;
        let key_ink = self.chrome().key;
        let text = self.chrome().text;
        let changed = self.chrome().changed;
        let height = super::settings_panel::plate_height(SHOWN, NOTE_LINES);
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
        let page = &self.scenes_page;
        let group = rows.get(page.cursor).map_or("", |r| GROUPS[r.group].1);
        put(
            &mut out,
            &format!("Scenes: {group}"),
            (left, Y0 + 1),
            heading,
        );
        put(
            &mut out,
            FLIP_HINT,
            (right - width(FLIP_HINT), Y0 + 1),
            text,
        );
        let mut y = Y0 + 15;
        for (i, row) in rows.iter().enumerate().skip(page.scroll).take(SHOWN) {
            let focused = i == page.cursor;
            if focused && let Some(src) = self.battle_hud_solid_src() {
                out.push(TextDraw {
                    dst: (X0 + 2, y, (WIDTH - 4) as u32, ROW_H as u32),
                    src,
                    color: self.chrome().hover,
                });
            }
            let current = here.as_deref() == Some(row.label.as_str());
            let label = if current {
                format!("{} (here)", row.label)
            } else {
                row.label.clone()
            };
            let label_w = width(&label);
            // A long name gives way to the label column, cut at a word.
            let mut name = row.name.clone();
            while width(&name) > WIDTH - 22 - label_w {
                match name.rfind(' ') {
                    Some(cut) => name.truncate(cut),
                    None => break,
                }
            }
            put(
                &mut out,
                &name,
                (left, y - 2),
                if current { changed } else { text },
            );
            put(
                &mut out,
                &label,
                (right - label_w, y - 2),
                if focused { key_ink } else { text },
            );
            y += ROW_H;
        }
        // The scroll bar on the plate's edge, as the settings page draws it.
        if rows.len() > SHOWN
            && let Some(src) = self.battle_hud_solid_src()
        {
            let track_h = SHOWN as i32 * ROW_H;
            let thumb_h = (track_h * SHOWN as i32 / rows.len() as i32).max(6);
            let max = (rows.len() - SHOWN) as i32;
            let thumb_y = Y0 + 15 + (track_h - thumb_h) * page.scroll as i32 / max.max(1);
            out.push(TextDraw {
                dst: (X0 + WIDTH - 3, thumb_y, 2, thumb_h as u32),
                src,
                color: text,
            });
        }
        y = Y0 + 15 + SHOWN as i32 * ROW_H + 3;
        let note = match (page.refused, rows.get(page.cursor)) {
            (Some(why), _) => why.to_string(),
            (None, Some(row)) => format!(
                "Cross: go to {}. Your party and items come along.",
                row.name
            ),
            (None, None) => String::new(),
        };
        for line in super::settings_panel::wrap_lines(&note, WIDTH - 12, &width)
            .iter()
            .take(NOTE_LINES)
        {
            put(&mut out, line, (left, y), text);
            y += HELP_H;
        }
        put(
            &mut out,
            &self.panel_controls(CONTROLS, PAD_CONTROLS),
            (left, Y0 + height - HELP_H),
            heading,
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_the_pages_groups_in_order() {
        let rows = scene_rows();
        assert!(rows.iter().any(|r| r.label == "town01"));
        assert!(rows.iter().any(|r| r.label == "map01"));
        // No battle, audio, cutscene or system block is offered.
        assert!(!rows.iter().any(|r| r.label == "battle_data"));
        // Grouped in the page's order and never repeated.
        assert!(rows.windows(2).all(|w| w[0].group <= w[1].group));
        let mut labels: Vec<_> = rows.iter().map(|r| &r.label).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), rows.len());
    }

    #[test]
    fn a_step_wraps_and_a_page_holds_at_the_ends() {
        let mut p = ScenesPage::default();
        p.move_cursor(-1, 40);
        assert_eq!(p.cursor, 39);
        assert!(p.scroll + SHOWN > 39 && p.scroll <= 39);
        p.move_cursor(1, 40);
        assert_eq!((p.cursor, p.scroll), (0, 0));
        p.move_cursor(-(SHOWN as i32), 40);
        assert_eq!(p.cursor, 0, "a page up from the top holds");
        p.move_cursor(SHOWN as i32, 40);
        assert_eq!(p.cursor, SHOWN);
        assert_eq!(p.scroll, 1);
    }

    #[test]
    fn the_cursor_starts_on_the_players_scene() {
        let rows = scene_rows();
        let mut p = ScenesPage::default();
        let i = rows.iter().position(|r| r.label == "map01").unwrap();
        p.place(Some("map01"), rows);
        assert_eq!(p.cursor, i);
        assert!(p.scroll <= i && i < p.scroll + SHOWN);
        // Only the first open places it.
        p.move_cursor(1, rows.len());
        p.place(Some("map01"), rows);
        assert_eq!(p.cursor, (i + 1) % rows.len());
    }

    /// Every note and the controls line fit the plate, in the disc's dialog
    /// font the window letters them in.
    #[test]
    fn the_notes_and_the_controls_fit_the_plate() {
        let font = std::env::var_os("LEGAIA_DISC_BIN")
            .and_then(|p| crate::launcher::disc_font(std::path::Path::new(&p)));
        if let Some(font) = font {
            let width = |s: &str| font.layout_ascii(s).advance_x as i32;
            assert!(width(CONTROLS) <= WIDTH - 12);
            let [a, b] = super::super::settings_panel::WIDEST_PAD_LABELS;
            let pad = super::super::settings_panel::pad_controls(PAD_CONTROLS, a, b);
            assert!(width(&pad) <= WIDTH - 12, "{pad}: {}", width(&pad));
            for row in scene_rows() {
                let note = format!(
                    "Cross: go to {}. Your party and items come along.",
                    row.name
                );
                let lines = super::super::settings_panel::wrap_lines(&note, WIDTH - 12, &width);
                assert!(lines.len() <= NOTE_LINES, "{}: {lines:?}", row.label);
            }
        }
    }
}
