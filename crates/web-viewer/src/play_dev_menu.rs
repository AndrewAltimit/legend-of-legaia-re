//! Browser host surface for the retail developer menu - the play-page twin
//! of the native window's `window/dev_menu.rs`.
//!
//! Retail opens its dev tools from debug branches in the world-map and field
//! controllers - branches a retail player cannot reach. The native window's
//! equivalent is the `LEGAIA_DEV_MENU` environment variable: an explicit
//! opt-in taken by the person who launches the binary. This host mirrors
//! that opt-in in the only form a browser tab has: an explicit page control
//! ([`LegaiaRuntime::play_dev_menu_set_enabled`]) that the visitor - who
//! *is* the person running the program in a client-side page - must flip
//! each session. The page deliberately neither persists the flag nor reads
//! it from the URL, so a shared link cannot carry it and nothing but a
//! fresh, deliberate click enables it. Everything the menu edits is the
//! visitor's own single-player world, in their own tab.
//!
//! With the flag off (the default), nothing here runs and no draw is
//! produced - the shipped page is unchanged.
//!
//! The model is the same one the native window drives: `engine-core`'s
//! [`DevMenuSession`] ticked off the retail **packed** pad words
//! (`retail_packed` over the world's own pad pump - see that function in
//! `legaia_engine_core::dev_menu` for the two layouts), the row list drawn
//! through the ported list-body renderer (`dev_menu_list_draws_for`,
//! `FUN_801EAD98`'s geometry), and Square swapping the list for the
//! battle-records page (`records_screen_draws_for`, `FUN_801ED710`) while
//! the list has the pad. The equip row's confirm commits against the
//! engine's own bag through the same `WorldEquipHost`.
//!
//! `check-ui-host-drift.py` pins this host to the native one three ways: the
//! two pens and the records heading strings are `CONSTANT_PAIRS` rows, and
//! the tick + records-model injection sites are `SIM_PAIRS` rows, so neither
//! host can drift on the packed-pad conversion, the equip commit or the
//! record-counter reads without the gate failing.

use legaia_engine_core::dev_menu::retail_packed;
use legaia_engine_core::dev_menu_host::{DevMenuRow, DevMenuSession, DevPage, WorldEquipHost};
use legaia_engine_ui::{self as ui, RecordsLabels, TextDraw};
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// Pen the dev-menu list draws from - clear of the field HUD's own rows.
/// Byte-identical to the native window's (`CONSTANT_PAIRS`).
const DEV_MENU_PEN: (i32, i32) = (16, 24);

/// Pen the Records page draws from. Its widest field lands at `+0xF0` and its
/// lowest row (the treasure line) at `+0xB7`, so this origin keeps the whole
/// page on the 320x240 stage. Byte-identical to the native window's.
const DEV_RECORDS_PEN: (i32, i32) = (24, 40);

/// Pad bit that swaps the row list for the Records page (unused by every
/// [`DevMenuSession`] row, so it steals nothing).
const RECORDS_TOGGLE: u16 = legaia_engine_core::dev_menu::PACK_SQUARE;

/// The records page's heading strings. Retail keeps them in the world-map
/// overlay's data segment; the builder takes them from the caller so no game
/// text lives in `engine-ui`. Byte-identical to the native window's copy
/// (`CONSTANT_PAIRS`).
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

impl LegaiaRuntime {
    /// Advance the developer menu one frame off the world's own pad pump.
    ///
    /// Called from `tick_frame` so the session sees the same 60 Hz pad
    /// transitions the simulation does - no page-side key handling exists
    /// (host-drift tier 5): the words come out of `World::set_pad`, converted
    /// to the retail packed layout the dev kernels key on.
    pub(crate) fn tick_dev_menu(&mut self) {
        if !self.dev_menu_enabled {
            return;
        }
        let Some(scene) = self.scene_host.as_mut() else {
            return;
        };
        let session = self.dev_menu.get_or_insert_with(DevMenuSession::new);
        let world = &mut scene.world;
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

        // The EQUIP row's confirm commits against the engine's own bag,
        // exactly as the native window's arm does.
        if session.current_row() == DevMenuRow::Equip
            && edge & legaia_engine_core::dev_menu::PACK_CROSS != 0
        {
            let character = session.chars.character as usize;
            let weapon_slots: Vec<i16> = vec![2; world.roster.members.len().max(4)];
            if let Some(member) = world.roster.members.get_mut(character) {
                let mut raw = std::mem::take(&mut member.raw);
                let mut equip_host = WorldEquipHost {
                    inventory: &mut world.inventory,
                    sfx: Vec::new(),
                };
                let _ = session.commit_equip_row(&mut equip_host, &mut raw, &weapon_slots);
                let cues = std::mem::take(&mut equip_host.sfx);
                world.roster.members[character].raw = raw;
                session.pending_sfx.extend(cues);
            }
        }

        // The native window only logs these cues; there is no dev-menu SFX
        // mapping in the page's cue bank either, so drain them the same way.
        session.drain_sfx();

        // Square swaps the row list for the Records page while the list has
        // the pad; the sub-editors keep their own key map.
        if session.page == DevPage::List && edge & RECORDS_TOGGLE != 0 {
            self.dev_menu_records = !self.dev_menu_records;
        }
    }

    /// Build the records page for the live world - the browser twin of the
    /// native `build_dev_records_draws`.
    fn dev_records_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(scene) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let world = &scene.world;
        let records: Vec<&[u8]> = world
            .roster
            .members
            .iter()
            .take(3)
            .map(|m| m.raw.as_slice())
            .collect();
        let model = dev_records_model(&records, world.play_time_seconds);
        ui::records_screen_draws_for(font, &records_view(&model), DEV_RECORDS_PEN)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The visitor's explicit dev-menu opt-in - the browser twin of the
    /// native window's `LEGAIA_DEV_MENU` environment variable. Session-only
    /// by design: the page neither persists this nor reads it from the URL,
    /// so only a deliberate click by the person at the keyboard enables it.
    /// Turning it off drops the session and its staged edits.
    pub fn play_dev_menu_set_enabled(&mut self, on: bool) {
        self.dev_menu_enabled = on;
        if !on {
            self.dev_menu = None;
            self.dev_menu_records = false;
        }
    }

    /// Whether the dev menu is currently enabled for this session.
    pub fn play_dev_menu_enabled(&self) -> bool {
        self.dev_menu_enabled
    }

    /// Draw list for the developer-menu overlay: `{ "open", "texts" }` in
    /// surface pixels, font-atlas quads only (the dev overlay is text-only on
    /// the native window too). `open` is `false` while the opt-in is off,
    /// before a scene is staged, or while the pause menu owns the screen -
    /// the dev overlay rides the live field, exactly as the native window
    /// folds it into the field HUD's draw list.
    pub fn play_dev_menu_draws_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        const CLOSED: &str = r#"{"open":false,"texts":[]}"#;
        if !self.dev_menu_enabled || self.play_menu_is_open() {
            return CLOSED.to_string();
        }
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let Some(session) = self.dev_menu.as_ref() else {
            return CLOSED.to_string();
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let mut texts = if session.page == DevPage::List && self.dev_menu_records {
            self.dev_records_draws(font)
        } else {
            build_dev_menu_draws(session, font)
        };
        if texts.is_empty() {
            return CLOSED.to_string();
        }
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        serde_json::json!({
            "open": true,
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

/// Build the row list's draws through the ported list-body renderer - the
/// browser twin of the native `build_dev_menu_draws`. The renderer owns the
/// geometry (the `+8` label column, the 8-px row pitch, the `0x17` row clamp
/// and the cursor column); each row's `(label, value)` pair is the ported row
/// model's own (`DevMenuSession::row_label` / `row_value`).
fn build_dev_menu_draws(session: &DevMenuSession, font: &legaia_font::Font) -> Vec<TextDraw> {
    use ui::{DevMenuListRow, dev_menu_cursor_xy, dev_menu_list_draws_for, text_draws_for};
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
            ui::MENU_TEXT_WHITE,
        ));
    }
    out
}

/// Build the records model for up to three character records and the world's
/// play clock - the browser twin of the native `dev_records_model`, pinned to
/// it by a `SIM_PAIRS` row (both must read through `record_counters` and
/// clamp through `records_screen`).
fn dev_records_model(
    records: &[&[u8]],
    play_time_seconds: u32,
) -> legaia_engine_vm::world_map_overlay::RecordsScreen {
    use legaia_engine_vm::world_map_overlay::{CharRecordStats, records_screen};
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
    // No lifetime battle / escape tally and no treasure census exist on the
    // world yet, so those read zero and the model's `total <= 0` guard hides
    // the treasure line entirely.
    records_screen(0, 0, play_frames, &chars, 0, 0)
}

/// Project the ported model onto the draw builder's view, with the host-owned
/// heading strings attached.
fn records_view(
    model: &legaia_engine_vm::world_map_overlay::RecordsScreen,
) -> ui::RecordsScreenView<'static> {
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

    /// A record with distinguishable values in each of the six fields.
    fn record(scale: u32) -> Vec<u8> {
        use ui::record_offset;
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

    /// The three columns carry the three roster records in slot order, and
    /// the clock arrives as seconds - the two conversions the native host's
    /// own tests pin from its side of the `SIM_PAIRS` row.
    #[test]
    fn records_model_reads_slots_and_converts_the_clock_once() {
        let (a, b, c) = (record(1), record(2), record(3));
        let m = dev_records_model(&[&a, &b, &c], 3661);
        assert_eq!(m.max_hits, [11, 22, 33]);
        assert_eq!(m.magic, [5, 10, 15]);
        assert_eq!((m.play_hours, m.play_minutes, m.play_seconds), (1, 1, 1));
        assert!(!m.treasure_shown, "no census -> the line is hidden");
    }

    /// The list body draws every row's label + readout and the cursor lands
    /// inside the 320x240 stage from the shared pen.
    #[test]
    fn dev_list_draws_fit_the_stage() {
        let session = DevMenuSession::new();
        let font = legaia_font::Font::placeholder();
        let draws = build_dev_menu_draws(&session, &font);
        assert!(!draws.is_empty());
        for d in &draws {
            assert!(d.dst.0 >= 0 && d.dst.1 >= 0, "draw off the top/left: {d:?}");
            assert!(d.dst.0 < 320 && d.dst.1 < 240, "draw off the stage: {d:?}");
        }
    }

    /// The records page fits the stage from its shared pen too.
    #[test]
    fn records_draws_fit_the_stage() {
        let a = record(1);
        let m = dev_records_model(&[&a], 12);
        let font = legaia_font::Font::placeholder();
        let draws = ui::records_screen_draws_for(&font, &records_view(&m), DEV_RECORDS_PEN);
        assert!(!draws.is_empty());
        for d in &draws {
            assert!(d.dst.0 >= 0 && d.dst.1 >= 0, "draw off the top/left: {d:?}");
            assert!(d.dst.0 < 320 && d.dst.1 < 240, "draw off the stage: {d:?}");
        }
    }
}
