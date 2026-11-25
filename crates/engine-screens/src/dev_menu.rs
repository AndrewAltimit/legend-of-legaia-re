//! The developer menu's draw half: the row list and the Records page, the
//! pens they draw from, the page's heading strings and the records model.
//!
//! Both play hosts drive the same `DevMenuSession` (`DevMenuWorldExt`) and
//! each used to carry its own copy of everything below - pinned together by
//! three paired-constant rows and a paired-kernel row of the host-drift gate.
//! One copy needs no pairing.

use legaia_engine_dev_menu::dev_menu_leaves::{CharRecordStats, RecordsScreen, records_screen};
use legaia_engine_menus::dev_menu_host::{DevMenuRow, DevMenuSession};
use legaia_engine_ui::{self as ui, RecordsLabels, TextDraw};

/// Pen the developer row list draws from.
pub const DEV_MENU_PEN: (i32, i32) = (16, 24);

/// Pen the Records page draws from. Its widest field lands at `+0xF0` and its
/// lowest row (the treasure line) at `+0xB7`, so this origin keeps the whole
/// page on the 320x240 stage while clearing the field HUD's own top rows.
pub const DEV_RECORDS_PEN: (i32, i32) = (24, 40);

/// Pad bit (retail packed layout) that swaps the row list for the Records
/// page - unused by every [`DevMenuSession`] row, so it steals nothing.
pub const RECORDS_TOGGLE: u16 = legaia_engine_system::pad::PACK_SQUARE;

/// The Records page's heading strings. Retail keeps them in the world-map
/// overlay's data segment; the builder takes them from its caller so no game
/// text lives in `engine-ui`.
pub const RECORDS_LABELS: RecordsLabels<'static> = RecordsLabels {
    battles: "No. of Battles",
    escapes: "No. of Escapes",
    max_hits: "Maximum Hits",
    max_damage: "Maximum Damage",
    knockouts: "Knockouts",
    monsters_defeated: "Monsters",
    hyper_arts: "Hyper Arts",
    magic: "Magic",
    treasure: "Treasure",
    percent: "%",
};

/// The row pitch the developer list draws at. Retail steps its rows 8 px
/// (`FUN_801EAD98`, `dev_menu_row_y`) under its 8 x 8 debug font; the port
/// draws the list in the dialogue font, whose ink is 11 px tall, and at
/// retail's pitch each row's lower half sat under the next row's upper half.
/// One pixel more than the ink keeps a descender off the row below.
pub const DEV_MENU_FONT_PITCH: i32 = 12;

/// The developer row list: the shared list geometry (the `+8` label column,
/// the `0x17` row clamp, the cursor column) over the session's own
/// `(label, value)` row model, at [`DEV_MENU_PEN`], a row every
/// [`DEV_MENU_FONT_PITCH`]. Each row goes through the ported row renderer as
/// a one-row window at its own pen, so the columns stay the kernel's.
pub fn dev_menu_list_draws(session: &DevMenuSession, font: &legaia_font::Font) -> Vec<TextDraw> {
    use ui::{DevMenuListRow, dev_menu_cursor_xy, dev_menu_list_draws_for, text_draws_for};
    let values: Vec<String> = DevMenuRow::ALL
        .iter()
        .map(|r| session.row_value(*r).unwrap_or_default())
        .collect();
    // The cursor in retail's row-index space (`+0x9E`), which the readout's
    // colour is decided in.
    let cursor_index = DevMenuRow::ALL
        .get(session.row)
        .map_or(-1, |r| r.retail_index() as i32);
    let rows: Vec<DevMenuListRow<'_>> = DevMenuRow::ALL
        .iter()
        .zip(values.iter())
        .map(|(r, v)| DevMenuListRow {
            label: session.row_label(*r),
            value: Some((v.as_str(), 0x68)),
            value_tint: r.retail_row().map_or(ui::DEV_MENU_NEUTRAL_TINT, |rr| {
                rr.readout_print_word(cursor_index)
            }),
        })
        .collect();
    let pen_of = |row: i32| (DEV_MENU_PEN.0, DEV_MENU_PEN.1 + row * DEV_MENU_FONT_PITCH);
    let mut out = Vec::new();
    for row in 0..rows.len() as i32 {
        out.extend(dev_menu_list_draws_for(font, &rows, row, row, pen_of(row)));
    }
    let cursor = session.row as i32;
    if let Some(xy) = dev_menu_cursor_xy(pen_of(cursor), cursor, cursor, cursor) {
        out.extend(text_draws_for(
            &font.layout_ascii(">"),
            xy,
            ui::MENU_TEXT_WHITE,
        ));
    }
    out
}

/// The Records model for up to three character records, the save window's
/// battle / escape tallies, the world's play clock (whole seconds) and the
/// script-counter slots 0 / 1 the treasure line divides: the record-relative
/// counter reads (`record_counters`) through the retail clamp / decompose
/// model (`records_screen`).
pub fn dev_records_model(
    records: &[&[u8]],
    (battles, escapes): (u32, u32),
    play_time_seconds: u32,
    treasure: (i16, i16),
) -> RecordsScreen {
    let mut chars = [CharRecordStats::default(); 3];
    for (slot, out) in chars.iter_mut().enumerate() {
        let Some(c) = records.get(slot).and_then(|r| ui::record_counters(r)) else {
            continue;
        };
        *out = CharRecordStats {
            max_hits: c.max_hits,
            max_damage: c.max_damage,
            knockouts: c.knockouts,
            monsters_defeated: c.monsters_defeated,
            hyper_arts: c.hyper_arts,
            magic: c.magic,
        };
    }
    // Retail's play counter ticks at 60 Hz; the world keeps whole seconds.
    let play_frames = play_time_seconds.saturating_mul(60);
    records_screen(
        battles,
        escapes,
        play_frames,
        &chars,
        i32::from(treasure.0),
        i32::from(treasure.1),
    )
}

/// The records screen (`FUN_801ED710`) for the live `world` at `pen`, retail
/// stage pixels: the dev Records page and the end-of-game soft reset
/// (`World::soft_reset_records_pen`) draw it at different origins.
///
/// Per-character counters come off the first three roster records - the
/// three columns retail's own loops walk (`s2 < 3` from record slot 0) - and
/// the play clock is the world's own second counter. Both play hosts call
/// this; each used to spell the roster walk itself.
pub fn records_draws_for_world(
    world: &legaia_engine_core::world::World,
    font: &legaia_font::Font,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let records: Vec<&[u8]> = world
        .party
        .roster
        .members
        .iter()
        .take(3)
        .map(|m| m.raw.as_slice())
        .collect();
    // The treasure line divides the field VM's script-counter slots 0 / 1
    // (`0x801C6460` / `0x801C6462`), which `4C CA/CB/CC` and the dance
    // results write.
    let slots = &world.field_vm.slot_table;
    let model = dev_records_model(
        &records,
        (
            world.party.record_tally.battles,
            world.party.record_tally.escapes,
        ),
        world.clock.records_play_seconds,
        (slots[0], slots[1]),
    );
    ui::records_screen_draws_for(font, &records_view(&model), pen)
}

/// Project the records model onto the draw builder's view, with
/// [`RECORDS_LABELS`] attached.
pub fn records_view(model: &RecordsScreen) -> ui::RecordsScreenView<'static> {
    ui::RecordsScreenView {
        battles: model.battles,
        escapes: model.escapes,
        play_hours: model.play_hours,
        play_minutes: model.play_minutes,
        play_seconds: model.play_seconds,
        max_hits: model.max_hits,
        max_damage: model.max_damage,
        knockouts: model.knockouts,
        monsters_defeated: model.monsters_defeated,
        hyper_arts: model.hyper_arts,
        magic: model.magic,
        treasure_found: model.treasure_found,
        treasure_total: model.treasure_total,
        treasure_percent: model.treasure_percent,
        treasure_fraction: model.treasure_fraction,
        treasure_percent_shown: model.treasure_percent_shown,
        labels: RECORDS_LABELS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_ui::{RecordsField, record_offset, records_screen_fields};

    const NO_TALLY: (u32, u32) = (0, 0);

    /// A record with distinguishable values in each of the six fields.
    fn record(scale: u32) -> Vec<u8> {
        let mut r = vec![0u8; 0x414];
        let put = |r: &mut Vec<u8>, off: usize, v: u32| {
            r[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut r, record_offset::MAX_HITS, 11 * scale);
        put(&mut r, record_offset::MAX_DAMAGE, 1000 * scale);
        put(&mut r, record_offset::KNOCKOUTS, 3 * scale);
        put(&mut r, record_offset::MONSTERS_DEFEATED, 500 * scale);
        r[record_offset::HYPER_ARTS] = 4 * scale as u8;
        r[record_offset::MAGIC] = 5 * scale as u8;
        r
    }

    fn assert_on_stage(draws: &[TextDraw]) {
        assert!(!draws.is_empty());
        for d in draws {
            assert!(d.dst.0 >= 0 && d.dst.1 >= 0, "draw off the top/left: {d:?}");
            assert!(d.dst.0 < 320 && d.dst.1 < 240, "draw off the stage: {d:?}");
        }
    }

    /// The three columns must carry the three roster records, in slot order.
    #[test]
    fn model_reads_one_column_per_roster_slot() {
        let (a, b, c) = (record(1), record(2), record(3));
        let m = dev_records_model(&[&a, &b, &c], NO_TALLY, 0, (0, 0));
        assert_eq!(m.max_hits, [11, 22, 33]);
        assert_eq!(m.max_damage, [1000, 2000, 3000]);
        assert_eq!(m.knockouts, [3, 6, 9]);
        assert_eq!(m.monsters_defeated, [500, 1000, 1500]);
        assert_eq!(m.hyper_arts, [4, 8, 12]);
        assert_eq!(m.magic, [5, 10, 15]);
    }

    /// A short roster leaves the missing columns zeroed rather than shifting
    /// the ones that are present, and a truncated record is skipped.
    #[test]
    fn model_tolerates_a_short_roster() {
        let a = record(1);
        let m = dev_records_model(&[&a], NO_TALLY, 0, (0, 0));
        assert_eq!(m.max_hits, [11, 0, 0]);
        let short = vec![0u8; 8];
        let m = dev_records_model(&[&short], NO_TALLY, 0, (0, 0));
        assert_eq!(m.max_hits, [0, 0, 0]);
    }

    /// The clock arrives as **seconds** and the model wants retail's `1/60 s`
    /// ticks - the conversion has to happen exactly once.
    #[test]
    fn play_clock_converts_seconds_to_retail_ticks_once() {
        let m = dev_records_model(&[], NO_TALLY, 3661, (0, 0));
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (1, 1, 1));
        let m = dev_records_model(&[], NO_TALLY, 59, (0, 0));
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (0, 0, 59));
        let m = dev_records_model(&[], NO_TALLY, 100 * 3600, (0, 0));
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (99, 59, 59));
    }

    /// With a zero slot 1 the line reads `0 / 0` with no percentage, as
    /// retail draws it (`blez` at `0x801EDDC8` skips the percentage only).
    #[test]
    fn treasure_line_without_a_total_drops_its_percentage() {
        let m = dev_records_model(&[], NO_TALLY, 0, (0, 0));
        assert!(!m.treasure_percent_shown);
        let fields = records_screen_fields(&records_view(&m), DEV_RECORDS_PEN);
        assert!(fields.iter().any(
            |f| matches!(f, RecordsField::Label { text, .. } if text == RECORDS_LABELS.treasure)
        ));
        assert!(!fields.iter().any(
            |f| matches!(f, RecordsField::Label { text, .. } if text == RECORDS_LABELS.percent)
        ));
    }

    /// The save window's tallies reach the two top rows, and slots 0 / 1
    /// of the script-counter table reach the treasure line as found / total
    /// with retail's percentage split (`25 / 64` = `39.06`).
    #[test]
    fn tally_and_slot_table_reach_the_page() {
        let m = dev_records_model(&[], (708, 37), 0, (25, 64));
        assert_eq!((m.battles, m.escapes), (708, 37));
        assert!(m.treasure_percent_shown);
        assert_eq!((m.treasure_percent, m.treasure_fraction), (39, 6));
        let v = records_view(&m);
        assert_eq!((v.treasure_found, v.treasure_total), (25, 64));
        let fields = records_screen_fields(&v, DEV_RECORDS_PEN);
        for want in [708, 37, 25, 64, 39, 6] {
            assert!(
                fields
                    .iter()
                    .any(|f| matches!(f, RecordsField::Number { value, .. } if *value == want)),
                "no field carries {want}"
            );
        }
    }

    /// A counter read out of a record reaches the emitted fields at the
    /// retail cell, the clock reaches the page as seconds, and the page fits
    /// the 320x240 stage from its pen.
    #[test]
    fn record_values_reach_the_emitted_fields() {
        let a = record(1);
        let m = dev_records_model(&[&a], NO_TALLY, 12, (0, 0));
        let fields = records_screen_fields(&records_view(&m), DEV_RECORDS_PEN);
        let (px, py) = DEV_RECORDS_PEN;
        assert!(fields.contains(&RecordsField::Number {
            x: px + 0x18,
            y: py + 0x31,
            value: 11,
            digits: 3,
            zero_pad: false,
            ink: 6,
        }));
        assert!(fields.iter().any(|f| matches!(
            f,
            RecordsField::Number { value, digits: 2, zero_pad: true, .. } if *value == 12
        )));
        let font = legaia_font::Font::placeholder();
        assert_on_stage(&ui::records_screen_draws_for(
            &font,
            &records_view(&m),
            DEV_RECORDS_PEN,
        ));
    }

    /// The row list draws every row and its cursor on the stage.
    #[test]
    fn dev_list_draws_fit_the_stage() {
        let session = DevMenuSession::new();
        let font = legaia_font::Font::placeholder();
        assert_on_stage(&dev_menu_list_draws(&session, &font));
    }

    /// The `EVENT_FLAG` readout takes retail's cursor tint (`0x10A080`,
    /// green over red, little blue) while the cursor sits on it; with the
    /// cursor elsewhere every quad keeps the list ink.
    #[test]
    fn the_event_flag_readout_lights_under_the_cursor() {
        let font = legaia_font::Font::placeholder();
        let mut session = DevMenuSession::new();
        let plain = dev_menu_list_draws(&session, &font);
        assert!(plain.iter().all(|d| d.color == ui::MENU_TEXT_WHITE));
        session.row = DevMenuRow::ALL
            .iter()
            .position(|r| *r == DevMenuRow::EventFlag)
            .unwrap();
        let lit = dev_menu_list_draws(&session, &font);
        let tinted: Vec<_> = lit
            .iter()
            .filter(|d| d.color != ui::MENU_TEXT_WHITE)
            .collect();
        assert!(!tinted.is_empty());
        for d in tinted {
            assert!(
                d.color[1] > d.color[0] && d.color[2] < d.color[0],
                "{:?}",
                d.color
            );
        }
    }

    /// The rows sit a font pitch apart, not retail's 8 px: at 8 a row's ink
    /// (11 px in the dialogue font) ran into the row under it.
    #[test]
    fn dev_list_rows_clear_each_other_in_the_dialogue_font() {
        let session = DevMenuSession::new();
        let font = legaia_font::Font::placeholder();
        let draws = dev_menu_list_draws(&session, &font);
        let mut tops: Vec<i32> = draws.iter().map(|d| d.dst.1).collect();
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), DevMenuRow::ALL.len(), "{tops:?}");
        for pair in tops.windows(2) {
            assert_eq!(pair[1] - pair[0], DEV_MENU_FONT_PITCH, "{tops:?}");
        }
        const { assert!(DEV_MENU_FONT_PITCH > ui::DEV_MENU_ROW_STEP) };
    }
}
