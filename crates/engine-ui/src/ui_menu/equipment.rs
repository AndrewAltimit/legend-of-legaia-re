//! Equip-screen draw builders - the retail multi-window layout.
//!
//! Retail composes the pause menu's Equip screen from four descriptor-table
//! windows (`legaia_asset::menu_windows`, draw order): the "Equip" title tab
//! (id 2, `FUN_801DCA94`), the party window (id 21, `FUN_801D2094`), the
//! renderer-less item-list container (id 23; its lower span is occluded by
//! the main window drawn after it) and the main window (id 22,
//! `FUN_801D21C0`) with the "Best Equipment" header, the 7-row pictogram
//! slot column and the stat-compare block. Frames come from the caller
//! (`menu_window_chrome_draws_for`); this module builds the window
//! *content* at the byte-pinned offsets, in 320x240 stage pixels off each
//! window's content-origin pen.

use super::field_panels::num_field_draws;
use crate::*;

/// Retail row pitch inside the equip party + main windows (the `+0xe`
/// row step of `FUN_801D2094` / `FUN_801D21C0`).
pub const EQUIP_ROW_PITCH: i32 = 0x0e;
/// Retail list pitch used for the item-list window rows (the shared
/// `0x0d` step of the menu-overlay list pages).
const LIST_PITCH: i32 = 0x0d;

/// Label drawn on the candidate list's Remove row (retail's class-`0x4000`
/// payload-`0` entry, whose string comes from the row-name resolver's
/// verb table rather than from an item record).
pub const EQUIP_REMOVE_ROW_LABEL: &str = "Remove";

/// One slot row in the equip main window.
pub struct EquipSlotRow<'a> {
    /// Slot label (engine hint only - retail identifies slots purely by
    /// the pictogram column; the label is not drawn).
    pub label: &'a str,
    /// Currently-equipped item display name. Empty / "(empty)" slots draw
    /// nothing, matching retail (icon-only row).
    pub current_name: &'a str,
}

/// One candidate row in the item-list window (id 23).
pub struct EquipCandidateRow<'a> {
    pub name: &'a str,
    pub count: u8,
}

/// One row of the main window's stat-compare block: current value plus
/// the preview under the candidate item.
pub struct EquipStatRow<'a> {
    /// 3-char stat label (retail strings at `0x801CE9A0/A4/A8`).
    pub label: &'a str,
    pub current: u16,
    pub preview: u16,
}

/// Phase tag for [`equip_screen_draws_for`]. Mirrors
/// `engine-core::equip_session::EquipState` without naming the enum so
/// the renderer doesn't pull engine-core in as a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipDrawPhase {
    /// Cursor on the slot rows of the main window.
    SlotPicker,
    /// Cursor on the candidate-item list for the active slot.
    ItemPicker,
    /// Yes/No confirmation prompt (`cursor` == 0 for Yes, 1 for No).
    Confirm,
}

/// Plain-data view of the whole equip screen for
/// [`equip_screen_draws_for`].
pub struct EquipScreenView<'a> {
    /// Party-member names for the id-21 party window rows.
    pub party_names: &'a [&'a str],
    /// Row of the character being equipped (hand-cursor row).
    pub party_cursor: usize,
    /// Slot rows for the main window (engine order; retail draws 7).
    pub slots: &'a [EquipSlotRow<'a>],
    /// Candidate items for the active slot. Empty in `SlotPicker` phase.
    pub candidates: &'a [EquipCandidateRow<'a>],
    /// Stat-compare rows (current vs candidate preview). Empty in
    /// `SlotPicker` phase; up to 3 rows drawn (the retail block height).
    pub stat_compare: &'a [EquipStatRow<'a>],
    pub phase: EquipDrawPhase,
    /// Cursor row inside the active phase column.
    ///
    /// In [`EquipDrawPhase::SlotPicker`] this is the **slot-browse row**,
    /// not a slot index: row `0` is the "Best Equipment" header line and
    /// row `n` is `slots[n - 1]`. That is retail's own row space - the
    /// header is cursor row 0 of `DAT_801E46C0` - and it is what
    /// `engine-core::equip_session::EquipState::SlotPicker` carries.
    pub cursor: u16,
    /// Active slot index when in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Optional pending swap label rendered above the Yes/No prompt.
    pub confirm_label: Option<&'a str>,
    /// Emit ASCII `>` cursors. `false` when the caller draws the retail
    /// pointing-hand sprite instead ([`equip_screen_sprites_for`]).
    pub text_cursor: bool,
}

/// Build [`TextDraw`]s for the retail equip screen's window contents.
///
/// `party_pen` / `list_pen` / `main_pen` are the content origins of the
/// id-21 / id-23 / id-22 descriptor windows (`legaia_asset::menu_windows`).
/// Offsets inside each window are the traced retail placements
/// (docs/subsystems/field-menu.md "Equip screen"):
///
/// - party window: member name at `X+6`, rows every `0xE` px
///   (PORT: FUN_801d2094);
/// - main window: "Best Equipment" at `(X+0x10, Y)` - cursor row 0 of the
///   window's cursor space, not a static header - slot rows at
///   `Y + 0xE*(i+1)` with the item name at `X+0x20` (the pictogram at
///   `X+0x10` is a sprite - [`equip_screen_sprites_for`]), and the
///   stat-compare block at rows `Y+0x48/+0x55/+0x62`: label `X+0xA0`,
///   current 3-digit value at `X+0xC8`, change arrow at `X+0xE4`, preview
///   value at `X+0xF0`, drawn only when the preview differs
///   (PORT: FUN_801d21c0);
/// - item-list window: engine-styled candidate rows at the retail `0xD`
///   list pitch (the retail picker's content renderer is untraced; only
///   the window rect is pinned).
pub fn equip_screen_draws_for(
    font: &legaia_font::Font,
    view: &EquipScreenView<'_>,
    party_pen: (i32, i32),
    list_pen: (i32, i32),
    main_pen: (i32, i32),
) -> Vec<TextDraw> {
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Party window (PORT: FUN_801d2094): names at X+6, 0xE pitch, CLUT 7
    // (white) - retail marks the active row with the hand cursor at
    // X-0xC (sprite; text fallback below), not a tint.
    let (px, py) = party_pen;
    for (i, name) in view.party_names.iter().enumerate() {
        let y = py + i as i32 * EQUIP_ROW_PITCH;
        if view.text_cursor && i == view.party_cursor {
            str_at(&mut out, ">", px - 8, y, gold);
        }
        str_at(&mut out, name, px + 6, y, white);
    }

    // Main window (PORT: FUN_801d21c0): header + slot rows. "Best
    // Equipment" is cursor row 0 of the window's own cursor space
    // (`DAT_801E46C0`), so it takes the hand cursor like any slot row.
    let (mx, my) = main_pen;
    let best_row_active = view.phase == EquipDrawPhase::SlotPicker && view.cursor == 0;
    if view.text_cursor && best_row_active {
        str_at(&mut out, ">", mx + 4, my, gold);
    }
    str_at(
        &mut out,
        "Best Equipment",
        mx + 0x10,
        my,
        if best_row_active { gold } else { white },
    );
    for (i, slot) in view.slots.iter().enumerate() {
        let y = my + (i as i32 + 1) * EQUIP_ROW_PITCH;
        let cursor_here = view.phase == EquipDrawPhase::SlotPicker && i as u16 + 1 == view.cursor;
        let row_active = view.phase != EquipDrawPhase::SlotPicker && view.active_slot as usize == i;
        // Retail keeps row text CLUT 7 (white); the hand cursor marks the
        // hovered row. The gold tint only backs the text-cursor fallback
        // and the active-slot reminder in the picker phases.
        let color = if row_active || (view.text_cursor && cursor_here) {
            gold
        } else {
            white
        };
        if view.text_cursor && cursor_here {
            str_at(&mut out, ">", mx + 4, y, color);
        }
        // Retail draws the equipped item's name-table string at X+0x20;
        // an empty slot stays icon-only.
        if !slot.current_name.is_empty() && slot.current_name != "(empty)" {
            str_at(&mut out, slot.current_name, mx + 0x20, y, color);
        }
    }

    // Stat-compare block (PORT: FUN_801d21c0, Best-Equipment pass):
    // rows Y+0x48/+0x55/+0x62.
    for (i, sr) in view.stat_compare.iter().take(3).enumerate() {
        let y = my + 0x48 + i as i32 * LIST_PITCH;
        str_at(&mut out, sr.label, mx + 0xa0, y, white);
        out.extend(num_field_draws(
            font,
            sr.current.min(999) as u64,
            mx + 0xc8,
            y,
            3,
            white,
        ));
        if sr.preview != sr.current {
            // Retail: up/down arrow glyph FUN_8003C1F8(4|5), CLUT 6
            // (raised) / 1 (lowered). ASCII stand-in until the arrow
            // glyphs are ported from the UI-icon atlas.
            let (glyph, c) = if sr.preview > sr.current {
                ("+", green)
            } else {
                ("-", red)
            };
            str_at(&mut out, glyph, mx + 0xe4, y, c);
            out.extend(num_field_draws(
                font,
                sr.preview.min(999) as u64,
                mx + 0xf0,
                y,
                3,
                c,
            ));
        }
    }

    // Item-list window (id 23): candidate rows. Engine-styled content in
    // the pinned retail rect.
    if view.phase != EquipDrawPhase::SlotPicker {
        let (lx, ly) = list_pen;
        if view.candidates.is_empty() {
            str_at(&mut out, "(no items)", lx + 10, ly, dim);
        }
        // Row 0 is retail's **Remove** row (class `0x4000`, payload `0`)
        // exactly when the active slot holds something -
        // `engine-core::equip_session::EquipSession::items_for_slot` gates
        // it on the same condition. It carries no item, so the host's
        // per-id name and owned count are both meaningless there and the
        // label is drawn from here instead.
        let remove_row = view
            .slots
            .get(view.active_slot as usize)
            .is_some_and(|s| !s.current_name.is_empty() && s.current_name != "(empty)");
        for (i, c) in view.candidates.iter().enumerate() {
            let y = ly + i as i32 * LIST_PITCH;
            let selected = view.phase == EquipDrawPhase::ItemPicker && i as u16 == view.cursor;
            let color = if selected { gold } else { white };
            if selected {
                str_at(&mut out, ">", lx, y, color);
            }
            if remove_row && i == 0 {
                str_at(&mut out, EQUIP_REMOVE_ROW_LABEL, lx + 10, y, color);
                continue;
            }
            str_at(&mut out, c.name, lx + 10, y, color);
            let count = format!("x{:>2}", c.count);
            str_at(&mut out, &count, lx + 104, y, color);
        }

        // Confirm prompt at the bottom of the list window.
        if view.phase == EquipDrawPhase::Confirm {
            let prompt_y = ly + 150;
            if let Some(label) = view.confirm_label {
                str_at(&mut out, label, lx, prompt_y, white);
            }
            for (i, opt) in ["Yes", "No"].iter().enumerate() {
                let x = lx + 10 + i as i32 * 50;
                let selected = i as u16 == view.cursor;
                let color = if selected { gold } else { white };
                if selected {
                    str_at(&mut out, ">", x - 10, prompt_y + LIST_PITCH, color);
                }
                str_at(&mut out, opt, x, prompt_y + LIST_PITCH, color);
            }
        }
    }

    out
}

/// Build the equip screen's UI-icon [`SpriteDraw`]s from the system-UI
/// atlas: the main window's slot pictogram column plus the pointing-hand
/// cursors, all at the traced `FUN_801D21C0` / `FUN_801D2094` offsets and
/// mapped from the 320x240 menu stage into surface pixels (`stage_origin`
/// + `stage_scale`, matching [`crate::menu_window_chrome_draws_for`]).
///
/// Pictograms sit at `main_pen + (0x10, 0xE*(row+1))`: the fixed icon-code
/// array `DAT_801E43F4` (weapon fist / helmet / armor / boot / 3x Goods
/// ring - the same 12x12 row-8 ICO records the status screen's equipment
/// grid uses, drawn via `FUN_8002C488`). Retail shows 7 rows; the engine's
/// 8-slot model adds a hand-guard row that reuses the fist pictogram and
/// an extra Goods row.
///
/// The hand cursor (the load-screen pointing-finger record) marks the
/// active party row at `party_pen + (-0xC, 0xE*row)` (FUN_801d2094's
/// cursor offset) and, in the slot-picker phase, the main-window
/// slot-browse row at `main_pen + (0, 0xE*row)` - row `0` being the "Best
/// Equipment" line, so `slot_cursor` is the row, not the slot index.
///
/// PORT: FUN_801d21c0 / FUN_801d2094 (icon + cursor placement).
/// REF: FUN_8002c488 / FUN_8002b994 - the UI-icon + cursor primitives.
#[allow(clippy::too_many_arguments)]
pub fn equip_screen_sprites_for(
    rects: &SaveMenuAtlasRects,
    n_slot_rows: usize,
    main_pen: (i32, i32),
    party_pen: (i32, i32),
    party_cursor: usize,
    slot_cursor: Option<u16>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    let mut push = |src: (u32, u32, u32, u32), sx: i32, sy: i32| {
        let (_, _, w, h) = src;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                w * stage_scale,
                h * stage_scale,
            ),
            src,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    };

    // Slot pictogram column. Engine slot order (Weapon / Helmet / Body
    // Armor / Hand Guard / Boots / Ring / Ring / Accessory) mapped onto
    // the retail pictogram set.
    let icons = [
        rects.icon_weapon,
        rects.icon_helmet,
        rects.icon_armor,
        rects.icon_weapon,
        rects.icon_boot,
        rects.icon_goods,
        rects.icon_goods,
        rects.icon_goods,
    ];
    let (mx, my) = main_pen;
    for (i, src) in icons.into_iter().take(n_slot_rows).enumerate() {
        push(src, mx + 0x10, my + EQUIP_ROW_PITCH * (i as i32 + 1));
    }

    // Party-window hand cursor at X-0xC on the active member's row.
    let (px, py) = party_pen;
    push(
        rects.cursor,
        px - 0x0c,
        py + EQUIP_ROW_PITCH * party_cursor as i32,
    );

    // Main-window hand cursor on the hovered slot-browse row. Row 0 is the
    // "Best Equipment" line at the window origin (retail's `hand at (X, Y)`),
    // row `n` the slot row `n - 1` at `Y + 0xE*n`.
    if let Some(row) = slot_cursor {
        push(rects.cursor, mx, my + EQUIP_ROW_PITCH * row as i32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: (i32, i32) = (14, 96);
    const LIST: (i32, i32) = (174, 22);
    const PARTY: (i32, i32) = (14, 42);

    fn view<'a>(
        slots: &'a [EquipSlotRow<'a>],
        candidates: &'a [EquipCandidateRow<'a>],
        phase: EquipDrawPhase,
        cursor: u16,
        active_slot: u8,
    ) -> EquipScreenView<'a> {
        EquipScreenView {
            party_names: &["Vahn"][..1],
            party_cursor: 0,
            slots,
            candidates,
            stat_compare: &[],
            phase,
            cursor,
            active_slot,
            confirm_label: None,
            text_cursor: true,
        }
    }

    fn cursor_rows(draws: &[TextDraw], x: i32) -> Vec<i32> {
        let mut ys: Vec<i32> = draws
            .iter()
            .filter(|d| d.dst.0 == x)
            .map(|d| d.dst.1)
            .collect();
        ys.sort_unstable();
        ys.dedup();
        ys
    }

    /// Browse row 0 is "Best Equipment" - the cursor belongs on the header
    /// line, and row `n` marks slot `n - 1`. Getting this off by one would
    /// silently point the hand at the wrong slot on every row.
    #[test]
    fn the_browse_cursor_counts_best_equipment_as_row_zero() {
        let font = legaia_font::synthetic_for_tests();
        let slots = [
            EquipSlotRow {
                label: "Weapon",
                current_name: "Iron Sword",
            },
            EquipSlotRow {
                label: "Helmet",
                current_name: "Cap",
            },
        ];
        let (mx, my) = MAIN;

        let row0 = equip_screen_draws_for(
            &font,
            &view(&slots, &[], EquipDrawPhase::SlotPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        assert_eq!(cursor_rows(&row0, mx + 4), vec![my]);

        let row2 = equip_screen_draws_for(
            &font,
            &view(&slots, &[], EquipDrawPhase::SlotPicker, 2, 0),
            PARTY,
            LIST,
            MAIN,
        );
        assert_eq!(
            cursor_rows(&row2, mx + 4),
            vec![my + EQUIP_ROW_PITCH * 2],
            "browse row 2 marks the second slot row"
        );
    }

    /// The candidate list's row 0 is retail's Remove entry whenever the
    /// active slot holds something. It carries no item, so its label comes
    /// from here rather than from the host's per-id name.
    #[test]
    fn candidate_row_zero_is_the_remove_row_only_on_an_occupied_slot() {
        let font = legaia_font::synthetic_for_tests();
        let candidates = [
            EquipCandidateRow {
                name: "REMOVE-PLACEHOLDER",
                count: 0,
            },
            EquipCandidateRow {
                name: "Iron Sword",
                count: 1,
            },
        ];
        let occupied = [EquipSlotRow {
            label: "Weapon",
            current_name: "Wood Sword",
        }];
        let empty = [EquipSlotRow {
            label: "Weapon",
            current_name: "",
        }];

        let with_remove = equip_screen_draws_for(
            &font,
            &view(&occupied, &candidates, EquipDrawPhase::ItemPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        let without = equip_screen_draws_for(
            &font,
            &view(&empty, &candidates, EquipDrawPhase::ItemPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        // The Remove row drops the count field the ordinary rows carry, so
        // an occupied slot emits strictly fewer glyphs on the same list.
        assert!(
            with_remove.len() < without.len(),
            "the Remove row must replace the host's name + count draws"
        );
        let (lx, ly) = LIST;
        let remove_glyphs = font.layout_ascii(EQUIP_REMOVE_ROW_LABEL).glyphs.len();
        assert_eq!(
            with_remove
                .iter()
                .filter(|d| d.dst.0 >= lx + 10 && d.dst.1 == ly)
                .count(),
            remove_glyphs,
            "row 0 draws exactly the Remove label"
        );
    }
}
