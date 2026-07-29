//! Host surface for the retail developer menu (`LEGAIA_DEV_MENU=1`).
//!
//! Retail opens its dev tools from debug branches in the world-map and field
//! controllers - branches a retail player cannot reach. The engine's
//! equivalent is this opt-in: with `LEGAIA_DEV_MENU` set, `play-window`
//! drives [`DevMenuSession`] once a frame off the same **packed** pad words
//! retail's own dev code reads (`_DAT_8007BB84` newly-pressed,
//! `_DAT_8007B850` held), and draws its row list through the ported list-body
//! renderer. The packed words are built here by [`retail_packed`] rather than
//! taken from `InputState::retail_pad()`, because the native host feeds
//! `World::set_pad` a raw PSX pad word and the pump republishes it unchanged -
//! see that function for the two layouts and why they are not the same word.
//!
//! Without the variable nothing here runs and no draw is produced, so the
//! default build is unchanged.
//!
//! # The Records page
//!
//! Retail's battle-records readout (`FUN_801ED710`) is a page of that same
//! world-map dev menu, so this host is where it belongs. Square swaps the row
//! list for it while the list has the pad. The six per-character counters come
//! straight out of the live `0x414`-byte character records through
//! [`legaia_engine_render::record_counters`], which rebases the retail
//! save-block displacements onto a bare record; the clamping and the H:MM:SS
//! decomposition are the ported model
//! (`legaia_engine_vm::world_map_overlay::records_screen`).
//!
//! The lifetime battle / escape counters and the treasure census are state the
//! engine does not keep, so those read zero and the treasure line stays hidden
//! - the same page retail draws off a save that never incremented them.

use super::*;
use legaia_engine_core::dev_menu::retail_packed;
use legaia_engine_core::dev_menu_host::{DevMenuRow, DevMenuSession, DevPage, WorldEquipHost};
use legaia_engine_render::RecordsLabels;

/// Pen the dev-menu list draws from - clear of the field HUD's own rows.
const DEV_MENU_PEN: (i32, i32) = (16, 24);

/// Pen the Records page draws from. Its widest field lands at `+0xF0` and its
/// lowest row (the treasure line) at `+0xB7`, so this origin keeps the whole
/// page on the 320x240 stage while clearing the field HUD's own top rows.
const DEV_RECORDS_PEN: (i32, i32) = (24, 40);

/// Pad bit that swaps the row list for the Records page (unused by every
/// [`DevMenuSession`] row, so it steals nothing).
const RECORDS_TOGGLE: u16 = legaia_engine_core::dev_menu::PACK_SQUARE;

/// The page's heading strings. Retail keeps them in the world-map overlay's
/// data segment; the builder takes them from the caller so no game text
/// lives in `engine-ui`. Paired with the browser play page's copy by
/// `check-ui-host-drift.py`'s `CONSTANT_PAIRS`, so the two hosts cannot
/// drift on the headings without the gate failing.
const RECORDS_LABELS: RecordsLabels<'static> = RecordsLabels {
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

impl PlayWindowApp {
    /// Whether the developer menu is enabled for this run.
    pub(super) fn dev_menu_enabled() -> bool {
        std::env::var_os("LEGAIA_DEV_MENU").is_some()
    }

    /// Advance the developer menu one frame and rebuild its draw list.
    ///
    /// The pad words are in the retail packed layout the dev kernels key on,
    /// so they see exactly the bits they were written against rather than a
    /// re-mapped approximation - [`retail_packed`] does the conversion and
    /// records why the host has to.
    pub(super) fn tick_dev_menu(&mut self) {
        if !Self::dev_menu_enabled() {
            return;
        }
        let session = self.dev_menu.get_or_insert_with(DevMenuSession::new);
        let world = &mut self.session.host.world;
        let (edge, held) = {
            let (now, prev) = (
                retail_packed(world.input.pad()),
                retail_packed(world.input.pad_prev()),
            );
            (now & !prev, now)
        };

        {
            let mut records: Vec<&mut [u8]> = world
                .roster
                .members
                .iter_mut()
                .map(|m| m.raw.as_mut_slice())
                .collect();
            session.tick(edge, held, &mut records);
        }

        // The EQUIP row's confirm commits against the engine's own bag.
        if session.current_row() == DevMenuRow::Equip
            && edge & legaia_engine_core::dev_menu::PACK_CROSS != 0
        {
            let character = session.chars.character as usize;
            let weapon_slots: Vec<i16> = vec![2; world.roster.members.len().max(4)];
            if let Some(member) = world.roster.members.get_mut(character) {
                let mut raw = std::mem::take(&mut member.raw);
                let mut host = WorldEquipHost {
                    inventory: &mut world.inventory,
                    sfx: Vec::new(),
                };
                let committed = session.commit_equip_row(&mut host, &mut raw, &weapon_slots);
                let cues = std::mem::take(&mut host.sfx);
                world.roster.members[character].raw = raw;
                session.pending_sfx.extend(cues);
                match committed {
                    Some(c) => log::info!(
                        "dev-menu: equipped item {} into slot {} on character {character} \
                         (refunded {:?})",
                        session.equip_item,
                        c.slot,
                        c.refunded
                    ),
                    None => log::info!(
                        "dev-menu: item {} is not in the bag - nothing committed",
                        session.equip_item
                    ),
                }
            }
        }

        for cue in session.drain_sfx() {
            log::debug!("dev-menu: sfx cue {cue:#04x}");
        }

        // Square swaps the row list for the Records page while the list has
        // the pad; the sub-editors keep their own key map.
        let on_list = session.page == DevPage::List;
        if on_list && edge & RECORDS_TOGGLE != 0 {
            self.dev_menu_records = !self.dev_menu_records;
        }
        self.dev_menu_draws = if on_list && self.dev_menu_records {
            self.build_dev_records_draws()
        } else {
            Self::build_dev_menu_draws(self.dev_menu.as_ref().expect("just inserted"), &self.font)
        };
    }

    /// Build the battle-records page for the live world.
    ///
    /// Per-character counters come off the first three roster records - the
    /// three columns retail's own loops walk (`s2 < 3` from record slot 0) -
    /// and the play clock is the world's own second counter scaled back to
    /// retail's 1/60 s tick. Everything past that (the display caps, the
    /// H:MM:SS split, the treasure percentage) is the ported model.
    fn build_dev_records_draws(&self) -> Vec<legaia_engine_render::TextDraw> {
        let world = &self.session.host.world;
        let records: Vec<&[u8]> = world
            .roster
            .members
            .iter()
            .take(3)
            .map(|m| m.raw.as_slice())
            .collect();
        let model = dev_records_model(&records, world.play_time_seconds);
        legaia_engine_render::records_screen_draws_for(
            &self.font,
            &records_view(&model),
            DEV_RECORDS_PEN,
        )
    }

    /// Build the row list's draws through the ported list-body renderer.
    ///
    /// The renderer owns the geometry - the `+8` label column, the 8-px row
    /// pitch, the `0x17` row clamp and the cursor column - so the only thing
    /// assembled here is each row's `(label, value)` pair.
    ///
    /// Both halves of that pair are the ported row model's, not this host's:
    /// the value through `DevMenuSession::row_value` and the label through
    /// `DevMenuSession::row_label`, which asks retail's own row kind whether
    /// the `_DAT_8007B868` gate closes it and substitutes `CLOSED` if so.
    fn build_dev_menu_draws(
        session: &DevMenuSession,
        font: &legaia_font::Font,
    ) -> Vec<legaia_engine_render::TextDraw> {
        use legaia_engine_render::{
            DevMenuListRow, dev_menu_cursor_xy, dev_menu_list_draws_for, text_draws_for,
        };
        let values: Vec<String> = DevMenuRow::ALL
            .iter()
            .map(|r| session.row_value(*r).unwrap_or_default())
            .collect();
        let rows: Vec<DevMenuListRow<'_>> = DevMenuRow::ALL
            .iter()
            .zip(values.iter())
            .map(|(r, v)| DevMenuListRow {
                label: session.row_label(*r),
                value: Some((v.as_str(), 0x68)),
            })
            .collect();
        let last = (rows.len() - 1) as i32;
        let mut out = dev_menu_list_draws_for(font, &rows, 0, last, DEV_MENU_PEN);
        if let Some(xy) = dev_menu_cursor_xy(DEV_MENU_PEN, session.row as i32, 0, last) {
            out.extend(text_draws_for(
                &font.layout_ascii(">"),
                xy,
                legaia_engine_render::MENU_TEXT_WHITE,
            ));
        }
        out
    }
}

/// Build the records model for up to three character records and the world's
/// play clock.
///
/// Free rather than a method so the two conversions that are easy to get
/// wrong - the seconds-to-`1/60 s` play clock and the record-relative field
/// reads - are checkable without a window.
fn dev_records_model(
    records: &[&[u8]],
    play_time_seconds: u32,
) -> legaia_engine_vm::world_map_overlay::RecordsScreen {
    use legaia_engine_vm::world_map_overlay::{CharRecordStats, records_screen};
    let mut chars = [CharRecordStats::default(); 3];
    for (slot, out) in chars.iter_mut().enumerate() {
        let Some(c) = records
            .get(slot)
            .and_then(|r| legaia_engine_render::record_counters(r))
        else {
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
    // No lifetime battle / escape tally and no treasure census exist on the
    // world yet, so those read zero and the model's `total <= 0` guard hides
    // the treasure line entirely.
    records_screen(0, 0, play_frames, &chars, 0, 0)
}

/// Project the ported model onto the draw builder's view, with the caller-owned
/// heading strings attached.
fn records_view(
    model: &legaia_engine_vm::world_map_overlay::RecordsScreen,
) -> legaia_engine_render::RecordsScreenView<'static> {
    legaia_engine_render::RecordsScreenView {
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
        treasure_found: 0,
        treasure_total: 0,
        treasure_percent: model.treasure_percent,
        treasure_fraction: model.treasure_fraction,
        treasure_shown: model.treasure_shown,
        labels: RECORDS_LABELS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_render::{RecordsField, record_offset, records_screen_fields};

    /// The raw-to-packed pad conversion, checked on every button whose two
    /// encodings are documented independently (`retail_pad`'s module table
    /// and the `dev_menu` `PACK_*` constants).
    #[test]
    fn pad_repack_matches_both_documented_layouts() {
        use legaia_engine_core::dev_menu::{
            PACK_CIRCLE, PACK_CROSS, PACK_DOWN, PACK_LEFT, PACK_RIGHT, PACK_SQUARE, PACK_TRIANGLE,
            PACK_UP,
        };
        use legaia_engine_core::input::PadButton as B;
        let cases = [
            (B::Up, PACK_UP),
            (B::Down, PACK_DOWN),
            (B::Left, PACK_LEFT),
            (B::Right, PACK_RIGHT),
            (B::Triangle, PACK_TRIANGLE),
            (B::Circle, PACK_CIRCLE),
            (B::Cross, PACK_CROSS),
            (B::Square, PACK_SQUARE),
        ];
        for (btn, packed) in cases {
            assert_eq!(
                retail_packed(btn.mask()),
                packed,
                "{} repacks wrong",
                btn.name()
            );
        }
        // Start's packed bit is pinned by `retail_pad`'s own decode test.
        assert_eq!(retail_packed(B::Start.mask()), 0x0800);
        // The conversion is an involution, so no caller can double-apply it
        // silently - a second pass returns the raw word.
        assert_eq!(retail_packed(retail_packed(B::Square.mask())), 0x8000);
    }

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

    /// The three columns must carry the three roster records, in slot order.
    #[test]
    fn model_reads_one_column_per_roster_slot() {
        let (a, b, c) = (record(1), record(2), record(3));
        let m = dev_records_model(&[&a, &b, &c], 0);
        assert_eq!(m.max_hits, [11, 22, 33]);
        assert_eq!(m.max_damage, [1000, 2000, 3000]);
        assert_eq!(m.knockouts, [3, 6, 9]);
        assert_eq!(m.monsters_defeated, [500, 1000, 1500]);
        assert_eq!(m.hyper_arts, [4, 8, 12]);
        assert_eq!(m.magic, [5, 10, 15]);
    }

    /// A short roster leaves the missing columns zeroed rather than shifting
    /// the ones that are present.
    #[test]
    fn model_tolerates_a_short_roster() {
        let a = record(1);
        let m = dev_records_model(&[&a], 0);
        assert_eq!(m.max_hits, [11, 0, 0]);
        // A truncated record is skipped, not read as garbage.
        let short = vec![0u8; 8];
        let m = dev_records_model(&[&short], 0);
        assert_eq!(m.max_hits, [0, 0, 0]);
    }

    /// The clock the host feeds is **seconds**, and the model wants retail's
    /// `1/60 s` ticks - the conversion has to happen exactly once. A missing
    /// or doubled `*60` shows up here as a wrong hour.
    #[test]
    fn play_clock_converts_seconds_to_retail_ticks_once() {
        // 1 h 1 min 1 s.
        let m = dev_records_model(&[], 3661);
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (1, 1, 1));
        // 59 s must not round up into a minute.
        let m = dev_records_model(&[], 59);
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (0, 0, 59));
        // Retail's 99h clamp still applies through the same path.
        let m = dev_records_model(&[], 100 * 3600);
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (99, 59, 59));
    }

    /// With no treasure census on the world the line must be *absent*, not
    /// drawn as `0 / 0`.
    #[test]
    fn treasure_line_is_hidden_not_zeroed() {
        let m = dev_records_model(&[], 0);
        assert!(!m.treasure_shown);
        let fields = records_screen_fields(&records_view(&m), DEV_RECORDS_PEN);
        assert!(!fields.iter().any(
            |f| matches!(f, RecordsField::Label { text, .. } if text == RECORDS_LABELS.treasure)
        ));
    }

    /// End-to-end at the draw list: a per-character counter the host read out
    /// of a record has to reach the emitted fields at the retail cell, and a
    /// different record has to move it.
    #[test]
    fn record_values_reach_the_emitted_fields() {
        let a = record(1);
        let m = dev_records_model(&[&a], 12);
        let fields = records_screen_fields(&records_view(&m), DEV_RECORDS_PEN);
        let (px, py) = DEV_RECORDS_PEN;
        // Column 0's Maximum Hits cell: x + 0x18, y + 0x31.
        assert!(fields.contains(&RecordsField::Number {
            x: px + 0x18,
            y: py + 0x31,
            value: 11,
            digits: 3,
            zero_pad: false,
            ink: 6,
        }));
        // The play clock reached the page as seconds, not as raw ticks.
        assert!(fields.iter().any(|f| matches!(
            f,
            RecordsField::Number { value, digits: 2, zero_pad: true, .. } if *value == 12
        )));
        // The dev overlay draws in unscaled window pixels, so the page's own
        // footprint has to fit the retail 320x240 stage from this pen or it
        // would spill past the framebuffer on a 1x window.
        let font = legaia_font::Font::placeholder();
        let draws = legaia_engine_render::records_screen_draws_for(
            &font,
            &records_view(&m),
            DEV_RECORDS_PEN,
        );
        assert!(!draws.is_empty());
        for d in &draws {
            assert!(d.dst.0 >= 0 && d.dst.1 >= 0, "draw off the top/left: {d:?}");
            assert!(d.dst.0 < 320 && d.dst.1 < 240, "draw off the stage: {d:?}");
        }
    }
}
