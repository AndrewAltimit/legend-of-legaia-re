//! The `F12` panel's Cheats page: the one cheat list both play hosts expose
//! ([`PLAYER_CHEAT_LIST`]), applied through `WorldCheats::apply_cheat` while
//! the game runs. The browser play page's *Cheats* panel is its twin; before
//! this page a desktop player reached the list only as `--cheat-*` launch
//! switches (and `F6` / `F7`).
//!
//! Each row is worked as [`CheatControl`] says: a press applies it, a switch
//! flips on every change, an amount is picked with Left / Right and applied
//! with Cross or Enter, the item row picks a name and adds one per press.
//! The outcome line `apply_cheat` returns is shown under the rows, the text
//! the page shows on its status line.

use super::*;
use legaia_engine_cheats::cheats::{
    CheatControl, PLAYER_CHEAT_LIST, PlayerCheat, WorldCheats, cheat_help, cheat_switch_on,
    cheat_switch_to,
};

/// The page's rows: every listed cheat, in the list's order.
pub(super) const CHEAT_ROWS: usize = PLAYER_CHEAT_LIST.len();

/// The page's own state: the cursor, each amount row's ladder step, the
/// picked item and the last outcome line.
#[derive(Debug, Clone, Default)]
pub(super) struct CheatsPage {
    pub(super) cursor: usize,
    /// Per row, the step of an [`CheatControl::Amount`] ladder (`None`: the
    /// ladder's default).
    steps: [Option<usize>; CHEAT_ROWS],
    /// Index into the build's item names (`WorldCheats::item_name_pairs`);
    /// `None` starts on [`FIRST_ITEM`].
    item: Option<usize>,
    /// What the last applied cheat did.
    pub(super) outcome: Option<String>,
}

impl CheatsPage {
    /// The ladder step row `row` is on.
    fn step(&self, row: usize, values: &[i64], default: usize) -> usize {
        self.steps[row].unwrap_or(default).min(values.len() - 1)
    }

    /// The picked item's index into `items`.
    fn item_index(&self, items: &[(u8, String)]) -> Option<usize> {
        let last = items.len().checked_sub(1)?;
        Some(
            self.item
                .unwrap_or_else(|| {
                    items
                        .iter()
                        .position(|(_, n)| n.eq_ignore_ascii_case(FIRST_ITEM))
                        .unwrap_or(0)
                })
                .min(last),
        )
    }

    /// Move the cursor by `d` rows, wrapping.
    pub(super) fn move_cursor(&mut self, d: i32) {
        self.cursor = (self.cursor as i32 + d).rem_euclid(CHEAT_ROWS as i32) as usize;
    }

    /// Left / Right on an amount or the item row; `false` when the row has
    /// nothing to pick (a press or a switch).
    fn pick(&mut self, dir: i8, items: &[(u8, String)]) -> bool {
        let (key, _) = PLAYER_CHEAT_LIST[self.cursor];
        match CheatControl::for_key(key) {
            Some(CheatControl::Amount { values, default }) => {
                let s = self.step(self.cursor, values, default) as i32 + i32::from(dir);
                self.steps[self.cursor] = Some(s.clamp(0, values.len() as i32 - 1) as usize);
                true
            }
            Some(CheatControl::Item) => match self.item_index(items) {
                Some(i) => {
                    let n = items.len() as i32;
                    self.item = Some((i as i32 + i32::from(dir)).rem_euclid(n) as usize);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    /// The cheat a press on the focused row applies. `switch_on` is where a
    /// switch row stands now, `items` the build's item names.
    fn cheat(&self, switch_on: Option<bool>, items: &[(u8, String)]) -> Option<PlayerCheat> {
        let (key, _) = PLAYER_CHEAT_LIST[self.cursor];
        match CheatControl::for_key(key)? {
            CheatControl::Press => PlayerCheat::from_key(key, 0.0, ""),
            CheatControl::Switch => cheat_switch_to(key, !switch_on?),
            CheatControl::Amount { values, default } => PlayerCheat::from_key(
                key,
                values[self.step(self.cursor, values, default)] as f64,
                "",
            ),
            CheatControl::Item => {
                let (_, name) = items.get(self.item_index(items)?)?;
                PlayerCheat::from_key(key, 1.0, name)
            }
        }
    }

    /// The value column of row `row`.
    fn value(&self, row: usize, switch_on: Option<bool>, items: &[(u8, String)]) -> String {
        let (key, _) = PLAYER_CHEAT_LIST[row];
        match CheatControl::for_key(key) {
            Some(CheatControl::Press) | None => String::new(),
            Some(CheatControl::Switch) => if switch_on == Some(true) { "On" } else { "Off" }.into(),
            Some(CheatControl::Amount { values, default }) => {
                values[self.step(row, values, default)].to_string()
            }
            Some(CheatControl::Item) => match self.item_index(items).and_then(|i| items.get(i)) {
                Some((_, name)) => format!("{name} +1"),
                None => "no item names".into(),
            },
        }
    }
}

/// The item row's first pick: the commonest consumable, not item 1 (a
/// Ra-Seru key item).
const FIRST_ITEM: &str = "Healing Leaf";

/// Stage geometry, matching the settings page's plate.
const X0: i32 = 10;
const Y0: i32 = 2;
const WIDTH: i32 = 300;
const ROW_H: i32 = 11;
const HELP_H: i32 = 12;
/// The help / outcome line budget under the rows.
const NOTE_LINES: usize = 2;
const CONTROLS: &str = "Up/Down pick  Left/Right choose  Cross apply";
/// [`CONTROLS`] on a controller (`settings_panel::pad_controls`).
const PAD_CONTROLS: &str = "D-pad pick, choose  {cross} apply";
const FLIP_HINT: &str = "L1/R1 or Tab: scenes";

impl PlayWindowApp {
    /// Left / Right on the Cheats page: pick a value; a switch flips.
    pub(super) fn cheats_change(&mut self, dir: i8) {
        let items = self.session.host.world.item_name_pairs();
        if !self.cheats_page.pick(dir, &items) {
            let (key, _) = PLAYER_CHEAT_LIST[self.cheats_page.cursor];
            if CheatControl::for_key(key) == Some(CheatControl::Switch) {
                self.cheats_apply();
            }
        }
    }

    /// Cross / Enter on the Cheats page: apply the focused row.
    pub(super) fn cheats_apply(&mut self) {
        let (key, _) = PLAYER_CHEAT_LIST[self.cheats_page.cursor];
        let world = &self.session.host.world;
        let items = world.item_name_pairs();
        let Some(cheat) = self.cheats_page.cheat(cheat_switch_on(world, key), &items) else {
            return;
        };
        let templates = self
            .session
            .host
            .new_game_defaults
            .as_ref()
            .map(|d| d.party.clone());
        let line = self
            .session
            .host
            .world
            .apply_cheat(&cheat, templates.as_ref());
        if let PlayerCheat::RandomEncounters(on) = cheat {
            // The session flag every entry re-arms from, as `F7` writes it.
            self.field_live_opts.live_loop = on;
        }
        log::info!("cheat: {line}");
        self.cheats_page.outcome = Some(line);
    }

    /// The Cheats page's draws, in stage pixels.
    pub(super) fn cheats_stage_draws(&self) -> Vec<TextDraw> {
        let heading = self.chrome().heading;
        let key_ink = self.chrome().key;
        let text = self.chrome().text;
        let changed = self.chrome().changed;
        let world = &self.session.host.world;
        let items = world.item_name_pairs();
        let height = super::settings_panel::plate_height(CHEAT_ROWS, NOTE_LINES);
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
        put(&mut out, "Cheats", (left, Y0 + 1), heading);
        put(
            &mut out,
            FLIP_HINT,
            (right - width(FLIP_HINT), Y0 + 1),
            text,
        );
        let mut y = Y0 + 15;
        for (i, (key, label)) in PLAYER_CHEAT_LIST.iter().enumerate() {
            let focused = i == self.cheats_page.cursor;
            if focused {
                self.push_focus_row(&mut out, (X0, y), (WIDTH, ROW_H));
            }
            put(&mut out, label, (left, y - 2), text);
            let on = cheat_switch_on(world, key);
            let value = self.cheats_page.value(i, on, &items);
            let ink = match (on, focused) {
                (Some(true), _) => changed,
                (_, true) => key_ink,
                _ => text,
            };
            let shown = match CheatControl::for_key(key) {
                Some(CheatControl::Press) if focused => "Cross: apply".to_string(),
                _ if focused && !value.is_empty() => format!("< {value} >"),
                _ => value,
            };
            put(&mut out, &shown, (right - width(&shown), y - 2), ink);
            y += ROW_H;
        }
        y += 3;
        let (key, _) = PLAYER_CHEAT_LIST[self.cheats_page.cursor];
        let note = self
            .cheats_page
            .outcome
            .as_deref()
            .unwrap_or_else(|| cheat_help(key));
        for line in super::settings_panel::wrap_lines(note, WIDTH - 12, &width)
            .iter()
            .take(NOTE_LINES)
        {
            put(&mut out, line, (left, y), text);
            y += HELP_H;
        }
        put(
            &mut out,
            &self.panel_controls(CONTROLS, PAD_CONTROLS),
            (left, super::settings_panel::controls_y(Y0, height)),
            heading,
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str) -> usize {
        PLAYER_CHEAT_LIST
            .iter()
            .position(|(k, _)| *k == key)
            .unwrap()
    }

    #[test]
    fn amount_rows_step_their_ladder_and_hold_at_the_ends() {
        let mut p = CheatsPage {
            cursor: row("seru"),
            ..Default::default()
        };
        assert_eq!(p.cheat(None, &[]), Some(PlayerCheat::GrantSeru(9)));
        assert!(p.pick(1, &[]), "held at the top");
        assert_eq!(p.cheat(None, &[]), Some(PlayerCheat::GrantSeru(9)));
        assert!(p.pick(-1, &[]));
        assert_eq!(p.value(p.cursor, None, &[]), "8");
        p.cursor = row("level");
        for _ in 0..40 {
            p.pick(-1, &[]);
        }
        assert_eq!(p.cheat(None, &[]), Some(PlayerCheat::PartyLevel(1)));
    }

    #[test]
    fn switches_flip_from_where_they_stand_and_presses_have_no_value() {
        let mut p = CheatsPage {
            cursor: row("noclip"),
            ..Default::default()
        };
        assert!(!p.pick(1, &[]));
        assert_eq!(
            p.cheat(Some(false), &[]),
            Some(PlayerCheat::WalkThroughWalls(true))
        );
        p.cursor = row("encounters");
        // "No encounters" on: random encounters back on.
        assert_eq!(
            p.cheat(Some(true), &[]),
            Some(PlayerCheat::RandomEncounters(true))
        );
        p.cursor = row("restore");
        assert_eq!(p.value(p.cursor, None, &[]), "");
        assert_eq!(p.cheat(None, &[]), Some(PlayerCheat::Restore));
    }

    #[test]
    fn the_item_row_wraps_through_the_names_and_adds_one() {
        let items = vec![
            (1, "Ra-Seru Meta".to_string()),
            (2, "Healing Leaf".to_string()),
            (3, "Magic Leaf".to_string()),
        ];
        let mut p = CheatsPage {
            cursor: row("item"),
            ..Default::default()
        };
        // Starts on the Healing Leaf, not item 1.
        assert_eq!(p.value(p.cursor, None, &items), "Healing Leaf +1");
        assert!(p.pick(1, &items));
        assert_eq!(
            p.cheat(None, &items),
            Some(PlayerCheat::GiveItem {
                query: "Magic Leaf".into(),
                qty: 1
            })
        );
        assert!(p.pick(1, &items));
        assert_eq!(p.value(p.cursor, None, &items), "Ra-Seru Meta +1", "wraps");
        assert_eq!(p.cheat(None, &[]), None, "no item table");
        assert!(!p.pick(1, &[]));
    }

    /// Every row's help fits the page's note lines, and the controls line
    /// the plate, in the disc's dialog font the window letters it in.
    #[test]
    fn every_help_and_the_controls_fit_the_plate() {
        let font = std::env::var_os("LEGAIA_DISC_BIN")
            .and_then(|p| crate::launcher::disc_font(std::path::Path::new(&p)));
        if let Some(font) = font {
            let width = |s: &str| font.layout_ascii(s).advance_x as i32;
            for (key, label) in PLAYER_CHEAT_LIST {
                let lines =
                    super::super::settings_panel::wrap_lines(cheat_help(key), WIDTH - 12, &width);
                assert!(lines.len() <= NOTE_LINES, "{key}: {lines:?}");
                assert!(width(label) + width("< 9999999 >") <= WIDTH - 12, "{key}");
            }
            assert!(width(CONTROLS) <= WIDTH - 12, "{}", width(CONTROLS));
            let [a, b] = super::super::settings_panel::WIDEST_PAD_LABELS;
            let pad = super::super::settings_panel::pad_controls(PAD_CONTROLS, a, b);
            assert!(width(&pad) <= WIDTH - 12, "{pad}: {}", width(&pad));
        }
    }

    #[test]
    fn cursor_wraps_both_ways() {
        let mut p = CheatsPage::default();
        p.move_cursor(-1);
        assert_eq!(p.cursor, CHEAT_ROWS - 1);
        p.move_cursor(1);
        assert_eq!(p.cursor, 0);
    }
}
