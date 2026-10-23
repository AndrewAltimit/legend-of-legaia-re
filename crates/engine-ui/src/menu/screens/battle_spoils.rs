//! The encounter banner and the post-battle spoils / defeat windows.

use crate::*;

/// Build [`TextDraw`]s for the encounter transition banner.
///
/// Drawn during [`crate::EncounterPhase::Transition`] (where the engine
/// type is `legaia_engine_battle::encounter::EncounterPhase`). Renders a
/// large centered "ENCOUNTER!" line plus the formation label below.
/// Engines fade the surface independently - this just produces the
/// glyph draws.
pub fn encounter_banner_draws_for(
    font: &legaia_font::Font,
    formation_label: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.3, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let mut out = Vec::new();
    let head = font.layout_ascii("ENCOUNTER!");
    out.extend(text_draws_for(&head, pen, yellow));
    if !formation_label.is_empty() {
        let body = font.layout_ascii(formation_label);
        out.extend(text_draws_for(&body, (pen.0, pen.1 + LINE_H), white));
    }
    out
}

/// Plain-data projection of `legaia_engine_core::world::BattleRewards` for
/// [`battle_spoils_draws_for`]. The shell builds one of these after a victory;
/// this crate stays free of an engine-core dependency.
pub struct BattleSpoilsView<'a> {
    pub xp: u32,
    pub gold: u32,
    /// The level-up window's line(s), already composed by the shell (it
    /// owns the name table and the executable's strings).
    pub level_ups: &'a [String],
    /// The report window's drop line(s), already composed by the shell
    /// against the item catalog and the executable's template.
    pub drops: &'a [String],
    /// The party leader, whose name opens the spoils line.
    pub leader: &'a str,
    /// Who the victory line names - the lead alone, or the lead's team -
    /// off the result-message build arms
    /// ([`legaia_engine_battle_vm::battle_party_panel::result_subject`]).
    pub subject: legaia_engine_battle_vm::battle_party_panel::ResultSubject,
    /// `(level_up, report)` rows each window sits below its rest seat this
    /// frame - the raise glide the results frame starts (`0` once landed).
    pub slide: (i32, i32),
    /// The victory sentence off the battle overlay
    /// ([`legaia_engine_battle_vm::battle_party_panel::VictoryText`]), so a
    /// language pack's wording reaches the report. `None` on a disc-free
    /// host, which draws the port's English.
    pub victory: Option<&'a legaia_engine_battle_vm::battle_party_panel::VictoryText>,
}

/// Row pitch of the post-battle report - the dialog font's own line height,
/// which is what the two-line spoils window measures at.
pub const SPOILS_LINE_H: i32 = 14;

/// The **level-up** window's stage rect, and the **spoils** window's.
///
/// The measurement is the gold frame band in the `noa_levelup_banner`
/// framebuffer at 320x240 native: the level-up window's band lies on rows
/// `6` and `30` and columns `10..=302`, the spoils window's on `154` and
/// `208` and columns `10..=310`. Retail draws them over the **live battle
/// scene**, one above the party and one below.
///
/// The constants are those bands **outset by 2**, because
/// [`crate::menu_window_chrome_draws_for`] seats its corner tiles two pixels
/// inside the rect it is handed - so a rect equal to the band draws the band
/// two pixels small on every side.
///
/// The two widths are the placement records': screen elements
/// `0x45..=0x4B` (the level-up window) carry a `280`-wide content box, the
/// report window's `0x41` a `288`-wide one.
pub const SPOILS_LEVELUP_RECT: (i32, i32, i32, i32) = (8, 4, 296, 28);
pub const SPOILS_REPORT_RECT: (i32, i32, i32, i32) = (8, 152, 304, 58);

/// Text pen inside either window, from its frame origin. The capture puts
/// the first glyph column at `16` in both windows and the first ink row at
/// `14` / `162`; the glyph cell's own ink starts two rows down, so the pen
/// is `(+8, +8)` off the rects above.
pub const SPOILS_TEXT_INSET: (i32, i32) = (8, 8);

/// The two figures' number records on the report window's text actor
/// (`0x27`), registered by the results frame `FUN_8004E568` through the
/// record-queue append `FUN_8003563C`: the EXP share at x `0x2E`
/// (`li a3,0x2e` at `0x8004F674`) and the gold at `0xB6` (`0x8004F6A8`),
/// both at y `0x0E` in a `5`-cell field (`li s1,0x5`, the stack word
/// `0x14(sp)`) under CLUT `7`. The actor-list draw `FUN_80031D00` renders a
/// record in the menu numeral cells (`8 x 12` at `v = 0xD0`, one 8-px cell
/// per digit, blank cells for the leading zeros - the cell kernel of
/// `FUN_80034B78`), so each figure right-aligns in its field, in the
/// numeral face rather than the dialog font: the capture's `12` ends at
/// column `100` and its `13` at `236`. Offsets are from the text pen, the
/// actor's content origin.
pub const SPOILS_XP_FIELD_X: i32 = 0x2E;
pub const SPOILS_GOLD_FIELD_X: i32 = 0xB6;
/// The records' y (`0x0E`), which is the sentence's second row.
pub const SPOILS_FIGURE_Y: i32 = 0x0E;
/// The records' field width in 8-px cells.
pub const SPOILS_FIGURE_CELLS: i32 = 5;
/// Pitch of the numeral cells.
const SPOILS_FIGURE_CELL: i32 = 8;
pub const SPOILS_TAIL_X: i32 = 91;
pub const SPOILS_GOLD_TAIL_X: i32 = 228;

/// A number record's draws: `value` right-aligned in a `cells`-wide field
/// of 8-px numeral cells at `(x, y)`, leading zeros blank and a zero value
/// drawn in the last cell (`FUN_80031D00`, `0x80031F00..0x80032150`).
fn number_record_draws(
    font: &legaia_font::Font,
    value: u32,
    (x, y): (i32, i32),
    cells: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (cells - len + i as i32).max(0);
        out.extend(crate::numeral_cell_draws(
            font,
            ch,
            (x + cell * SPOILS_FIGURE_CELL, y),
            color,
        ));
    }
    out
}

/// One window of the post-battle report: its stage rect and the lines in it.
pub struct SpoilsWindow {
    pub rect: (i32, i32, i32, i32),
    pub lines: Vec<String>,
}

/// The windows retail's post-battle report puts on screen, top first.
///
/// Split out of the draw so a host can emit the window **chrome** (the gold
/// nine-slice over the blue fill, off the system-UI atlas) and the text from
/// one description. A window with no lines is omitted: a victory with no
/// level-up draws the spoils window alone, which is what retail's capture of
/// a level-less win shows.
///
/// Heights: the two measured rects hold one and two lines respectively, and
/// a longer report grows downward at [`SPOILS_LINE_H`]. The **growth** rule
/// is the port's - one capture cannot pin it - but the two rects are not.
pub fn battle_spoils_windows(view: &BattleSpoilsView<'_>) -> Vec<SpoilsWindow> {
    let mut out = Vec::new();
    if !view.level_ups.is_empty() {
        let (x, y, w, h) = SPOILS_LEVELUP_RECT;
        let extra = (view.level_ups.len() as i32 - 1).max(0) * SPOILS_LINE_H;
        out.push(SpoilsWindow {
            rect: (x, y + view.slide.0, w, h + extra),
            lines: view.level_ups.to_vec(),
        });
    }
    // Retail's two build arms word this line differently: a lone lead is
    // named, a party is the lead's team (`FUN_801D84C0`).
    let opening = match (view.victory, view.subject) {
        (Some(v), subject) => v.opening(subject, view.leader),
        (None, legaia_engine_battle_vm::battle_party_panel::ResultSubject::Lead(_)) => {
            format!("{} won the battle!", view.leader)
        }
        (None, legaia_engine_battle_vm::battle_party_panel::ResultSubject::LeadsTeam { .. }) => {
            format!("{}'s team won the battle!", view.leader)
        }
    };
    let mut lines = vec![
        opening,
        // The two figures are placed by column, not by this string - the
        // builder lays the sentence out in three runs. Kept whole here so a
        // caller that only wants the text reads a sentence.
        match view.victory.map(|v| v.report_runs()).as_deref() {
            Some([a, b, c]) => format!("{a} {} {b} {} {c}", view.xp, view.gold),
            Some(_) => view.victory.map(|v| v.report.clone()).unwrap_or_default(),
            None => format!("Gained {} Experience and {} G.", view.xp, view.gold),
        },
    ];
    // The drop line arrives composed (`World::battle_spoils_banner`): the
    // executable's template, appended to the victory buffer as its third row.
    lines.extend(view.drops.iter().cloned());
    // Retail's box is a fixed `288 x 42` (element `0x41`): three rows - the
    // victory line, the `Gained` row and the drop line - fit without growing
    // it. Past three the port grows the box (retail surfaces one drop).
    let (x, y, w, h) = SPOILS_REPORT_RECT;
    let extra = (lines.len() as i32 - 3).max(0) * SPOILS_LINE_H;
    out.push(SpoilsWindow {
        rect: (x, y + view.slide.1, w, h + extra),
        lines,
    });
    out
}

/// Build the post-battle report's text, in **stage** pixels.
///
/// Retail's report is two framed windows over the live battle scene, not a
/// caption on the field: `<name>'s level increased!` above the party and
/// `<leader>'s team won the battle!` / `Gained N Experience and M G.` below
/// it. The frames come from [`battle_spoils_windows`] through the host's
/// window-chrome emitter; this is the text that goes in them.
///
/// The arithmetic behind every number is `World::apply_battle_loot` /
/// `apply_battle_xp` (see `docs/subsystems/level-up.md`). The report window
/// is screen element `0x41` (`FUN_801D8DE8(0x41, 0)` in the results frame
/// `FUN_8004E568`), whose placement record agrees with the framebuffer rect.
/// The two figures are its number records (`FUN_8003563C` at `0x8004F690` /
/// `0x8004F6B4`, [`SPOILS_XP_FIELD_X`]); the sentence's text columns are read
/// off the framebuffer.
pub fn battle_spoils_draws_for(
    font: &legaia_font::Font,
    view: &BattleSpoilsView<'_>,
    windows: &[SpoilsWindow],
) -> Vec<TextDraw> {
    // Retail's default pen (ink 7), not full-scale white - see
    // [`MENU_TEXT_WHITE`].
    let white = MENU_TEXT_WHITE;
    let mut out = Vec::new();
    for (i, win) in windows.iter().enumerate() {
        let pen = (
            win.rect.0 + SPOILS_TEXT_INSET.0,
            win.rect.1 + SPOILS_TEXT_INSET.1,
        );
        // The report window's second line is the only one laid out by
        // column: retail right-aligns both figures inside the sentence.
        let is_report = i + 1 == windows.len();
        for (row, line) in win.lines.iter().enumerate() {
            let y = pen.1 + row as i32 * SPOILS_LINE_H;
            if is_report && row == 1 {
                // The disc's report line, split at the blank runs the
                // figures sit in; the port's English without a disc. A
                // translation that keeps three word groups draws them at
                // retail's three columns, any other shape draws the line
                // whole from the pen with the figures over it, the way
                // retail draws the string.
                let runs = view.victory.map(|v| v.report_runs());
                let words: [&str; 3] = match runs.as_deref() {
                    Some([a, b, c]) => [a.as_str(), b.as_str(), c.as_str()],
                    Some(_) => ["", "", ""],
                    None => ["Gained", "Experience and", "G."],
                };
                if let (Some(v), Some(r)) = (view.victory, runs.as_ref())
                    && r.len() != 3
                {
                    out.extend(text_draws_for(
                        &font.layout_ascii(&v.report),
                        (pen.0, y),
                        white,
                    ));
                }
                let head = font.layout_ascii(words[0]);
                out.extend(text_draws_for(&head, (pen.0, y), white));
                out.extend(number_record_draws(
                    font,
                    view.xp,
                    (pen.0 + SPOILS_XP_FIELD_X, pen.1 + SPOILS_FIGURE_Y),
                    SPOILS_FIGURE_CELLS,
                    white,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii(words[1]),
                    (pen.0 + SPOILS_TAIL_X, y),
                    white,
                ));
                out.extend(number_record_draws(
                    font,
                    view.gold,
                    (pen.0 + SPOILS_GOLD_FIELD_X, pen.1 + SPOILS_FIGURE_Y),
                    SPOILS_FIGURE_CELLS,
                    white,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii(words[2]),
                    (pen.0 + SPOILS_GOLD_TAIL_X, y),
                    white,
                ));
                continue;
            }
            out.extend(text_draws_for(&font.layout_ascii(line), (pen.0, y), white));
        }
    }
    out
}

/// The **loss** window retail's results frame opens on a party wipe - screen
/// element `0x42`, one line in the report window's frame.
///
/// Its placement record is byte-identical to the win window's (`0x41`):
/// content box `288 x 42` at `(16, 160)`, which outset by the frame's six
/// pixels is exactly the band [`SPOILS_REPORT_RECT`] measures, so the loss
/// window reuses that rect. `line` is the defeat message
/// (`legaia_engine_battle_vm::battle_party_panel::DefeatText::compose`); `None` - a
/// disc-free host - opens the frame empty.
pub fn battle_defeat_windows(line: Option<&str>, slide_y: i32) -> Vec<SpoilsWindow> {
    let (x, y, w, h) = SPOILS_REPORT_RECT;
    vec![SpoilsWindow {
        rect: (x, y + slide_y, w, h),
        lines: line.map(|l| vec![l.to_string()]).unwrap_or_default(),
    }]
}

/// Text for windows whose lines are plain sentences - the loss window's -
/// in **stage** pixels, at the report windows' pen and pitch.
pub fn battle_result_line_draws_for(
    font: &legaia_font::Font,
    windows: &[SpoilsWindow],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for win in windows {
        let pen = (
            win.rect.0 + SPOILS_TEXT_INSET.0,
            win.rect.1 + SPOILS_TEXT_INSET.1,
        );
        for (row, line) in win.lines.iter().enumerate() {
            let y = pen.1 + row as i32 * SPOILS_LINE_H;
            out.extend(text_draws_for(
                &font.layout_ascii(line),
                (pen.0, y),
                MENU_TEXT_WHITE,
            ));
        }
    }
    out
}

#[cfg(test)]
mod spoils_subject_tests {
    use super::*;
    use legaia_engine_battle_vm::battle_party_panel::{ResultSubject, result_subject};

    fn opening(subject: ResultSubject) -> String {
        let view = BattleSpoilsView {
            xp: 12,
            gold: 13,
            level_ups: &[],
            drops: &[],
            leader: "Vahn",
            subject,
            slide: (0, 0),
            victory: None,
        };
        battle_spoils_windows(&view)
            .last()
            .and_then(|w| w.lines.first().cloned())
            .unwrap_or_default()
    }

    /// `FUN_801D84C0`'s two arms: an empty second seat names the lead, a
    /// filled one names the lead's team.
    #[test]
    fn the_victory_line_follows_the_build_arm() {
        let solo = opening(result_subject([1, 0, 0]));
        let party = opening(result_subject([1, 2, 0]));
        assert!(solo.starts_with("Vahn won"), "{solo}");
        assert!(party.starts_with("Vahn's team won"), "{party}");
    }

    /// The raise glide moves each window by its own row offset: the
    /// level-up window down from above, the report up from below.
    #[test]
    fn the_windows_ride_their_glide_offsets() {
        let ups = ["A".to_string()];
        let view = BattleSpoilsView {
            xp: 1,
            gold: 1,
            level_ups: &ups,
            drops: &[],
            leader: "Vahn",
            subject: result_subject([1, 0, 0]),
            slide: (-38, 76),
            victory: None,
        };
        let w = battle_spoils_windows(&view);
        assert_eq!(w[0].rect.1, SPOILS_LEVELUP_RECT.1 - 38);
        assert_eq!(w[1].rect.1, SPOILS_REPORT_RECT.1 + 76);
    }
}

#[cfg(test)]
mod victory_text_tests {
    use super::*;
    use legaia_engine_battle_vm::battle_party_panel::{VictoryText, result_subject};

    #[allow(clippy::type_complexity)]
    fn draws(
        victory: Option<&VictoryText>,
    ) -> (
        Vec<String>,
        Vec<((i32, i32, u32, u32), (u32, u32, u32, u32))>,
    ) {
        let font = legaia_font::synthetic_for_tests();
        let view = BattleSpoilsView {
            xp: 120,
            gold: 34,
            level_ups: &[],
            drops: &[],
            leader: "Vahn",
            subject: result_subject([1, 2, 0]),
            slide: (0, 0),
            victory,
        };
        let w = battle_spoils_windows(&view);
        let d = battle_spoils_draws_for(&font, &view, &w);
        (
            w.last().unwrap().lines.clone(),
            d.iter().map(|t| (t.dst, t.src)).collect(),
        )
    }

    /// The retail disc's pieces draw exactly what the port's English drew:
    /// the same lines, glyph for glyph at the same pens.
    #[test]
    fn the_retail_sentence_draws_like_the_english_fallback() {
        let retail = VictoryText {
            team_tail: "'s team".into(),
            opening_tail: " won the battle!".into(),
            report: "Gained           Experience and           G.".into(),
        };
        assert_eq!(draws(Some(&retail)), draws(None));
    }

    /// A translated sentence reaches both lines.
    #[test]
    fn a_translated_sentence_reaches_the_report() {
        let fr = VictoryText {
            team_tail: " et son groupe".into(),
            opening_tail: " gagnent !".into(),
            report: "Obtenu      points et      Or.".into(),
        };
        let (lines, _) = draws(Some(&fr));
        assert_eq!(lines[0], "Vahn et son groupe gagnent !");
        assert_eq!(lines[1], "Obtenu 120 points et 34 Or.");
    }
}

#[cfg(test)]
mod defeat_window_tests {
    use super::*;

    /// Screen element `0x42` is the report window's twin: one window at the
    /// report rect, one line, or an empty frame without the disc text.
    #[test]
    fn the_loss_window_takes_the_report_frame() {
        let w = battle_defeat_windows(Some("A line"), 0);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].rect, SPOILS_REPORT_RECT);
        assert_eq!(w[0].lines, vec!["A line".to_string()]);
        let empty = battle_defeat_windows(None, 0);
        assert_eq!(empty[0].rect, SPOILS_REPORT_RECT);
        assert!(empty[0].lines.is_empty());
    }
}
