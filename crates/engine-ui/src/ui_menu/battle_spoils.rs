//! The encounter banner and the post-battle spoils / defeat windows.

use crate::*;

/// Build [`TextDraw`]s for the encounter transition banner.
///
/// Drawn during [`crate::EncounterPhase::Transition`] (where the engine
/// type is `legaia_engine_core::encounter::EncounterPhase`). Renders a
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
    /// ([`legaia_engine_vm::battle_party_panel::result_subject`]).
    pub subject: legaia_engine_vm::battle_party_panel::ResultSubject,
    /// `(level_up, report)` rows each window sits below its rest seat this
    /// frame - the raise glide the results frame starts (`0` once landed).
    pub slide: (i32, i32),
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

/// Column the experience figure's last digit ends on, and the column the
/// gold figure's does, both relative to the text pen. Retail right-aligns
/// both numbers inside the sentence rather than letting it reflow - the
/// capture's `12` ends at `100` and its `13` at `236`.
pub const SPOILS_XP_RIGHT: i32 = 84;
pub const SPOILS_TAIL_X: i32 = 91;
pub const SPOILS_GOLD_RIGHT: i32 = 220;
pub const SPOILS_GOLD_TAIL_X: i32 = 228;

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
    let opening = match view.subject {
        legaia_engine_vm::battle_party_panel::ResultSubject::Lead(_) => {
            format!("{} won the battle!", view.leader)
        }
        legaia_engine_vm::battle_party_panel::ResultSubject::LeadsTeam { .. } => {
            format!("{}'s team won the battle!", view.leader)
        }
    };
    let mut lines = vec![
        opening,
        // The two figures are placed by column, not by this string - the
        // builder lays the sentence out in three runs. Kept whole here so a
        // caller that only wants the text reads a sentence.
        format!("Gained {} Experience and {} G.", view.xp, view.gold),
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
/// `FUN_8004E568`), whose placement record agrees with the framebuffer rect;
/// the sentence columns are still read off the framebuffer rather than off
/// the two digit registrations (`FUN_8003563C` at `0x8004F690` /
/// `0x8004F6B4`).
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
                let head = font.layout_ascii("Gained");
                out.extend(text_draws_for(&head, (pen.0, y), white));
                let xp = view.xp.to_string();
                let xp_w = font.layout_ascii(&xp).advance_x as i32;
                out.extend(text_draws_for(
                    &font.layout_ascii(&xp),
                    (pen.0 + SPOILS_XP_RIGHT - xp_w, y),
                    white,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii("Experience and"),
                    (pen.0 + SPOILS_TAIL_X, y),
                    white,
                ));
                let gold = view.gold.to_string();
                let gold_w = font.layout_ascii(&gold).advance_x as i32;
                out.extend(text_draws_for(
                    &font.layout_ascii(&gold),
                    (pen.0 + SPOILS_GOLD_RIGHT - gold_w, y),
                    white,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii("G."),
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
/// (`legaia_engine_vm::battle_party_panel::DefeatText::compose`); `None` - a
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
    use legaia_engine_vm::battle_party_panel::{ResultSubject, result_subject};

    fn opening(subject: ResultSubject) -> String {
        let view = BattleSpoilsView {
            xp: 12,
            gold: 13,
            level_ups: &[],
            drops: &[],
            leader: "Vahn",
            subject,
            slide: (0, 0),
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
        };
        let w = battle_spoils_windows(&view);
        assert_eq!(w[0].rect.1, SPOILS_LEVELUP_RECT.1 - 38);
        assert_eq!(w[1].rect.1, SPOILS_REPORT_RECT.1 + 76);
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
