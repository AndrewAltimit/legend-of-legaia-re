//! Equip-screen draw builders - the retail multi-window layout.
//!
//! Retail composes the pause menu's Equip screen from four descriptor-table
//! windows (`legaia_game_tables::menu_windows`, draw order): the "Equip" title tab
//! (id 2, `FUN_801DCA94`), the party window (id 21, `FUN_801D2094`), the
//! renderer-less item-list container (id 23; its lower span is occluded by
//! the main window drawn after it) and the main window (id 22,
//! `FUN_801D21C0`) with the "Best Equipment" header, the 7-row pictogram
//! slot column and the stat-compare block. Frames come from the caller
//! (`menu_window_chrome_draws_for`); this module builds the window
//! *content* at the byte-pinned offsets, in 320x240 stage pixels off each
//! window's content-origin pen.

use super::num_field_draws;
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

/// A verb row's name column and a pictogram row's, from the list pen.
const EQUIP_ROW_NAME_X: i32 = 0x0c;
const EQUIP_ROW_ICON_NAME_X: i32 = 0x1c;

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
    /// ICO code of the row's pictogram; `0` is a row with none, whose name
    /// starts where the pictogram would (the Remove verb, class `0x4000`).
    pub icon: u8,
}

/// One row of the main window's stat-compare block: current value plus
/// the preview under the candidate item.
pub struct EquipStatRow<'a> {
    /// 3-char stat label (retail strings at `0x801CE9A0/A4/A8`).
    pub label: &'a str,
    pub current: u16,
    pub preview: u16,
}

/// Phase tag for [`equip_screen_draws_for_with`]. Mirrors
/// `engine-menus::equip_session::EquipState` without naming the enum so
/// the renderer doesn't pull engine-core in as a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipDrawPhase {
    /// Cursor on the slot rows of the main window.
    SlotPicker,
    /// Cursor on the candidate-item list for the active slot.
    ItemPicker,
}

/// Plain-data view of the whole equip screen for
/// [`equip_screen_draws_for_with`].
pub struct EquipScreenView<'a> {
    /// Party-member names for the id-21 party window rows.
    pub party_names: &'a [&'a str],
    /// Row of the character being equipped (hand-cursor row).
    pub party_cursor: usize,
    /// Slot rows for the main window, in retail's browse order (7 rows).
    pub slots: &'a [EquipSlotRow<'a>],
    /// Candidate items for the active slot. Empty in `SlotPicker` phase.
    pub candidates: &'a [EquipCandidateRow<'a>],
    /// Stat-compare rows (current vs candidate preview). Empty in
    /// `SlotPicker` phase; up to 3 rows drawn (the retail block height).
    pub stat_compare: &'a [EquipStatRow<'a>],
    /// The Best Equipment row's change list, `(armament row 0..=3, pick
    /// name)` - one entry per armament whose best pick differs from what it
    /// holds. Empty off the Best Equipment row.
    pub best_changes: &'a [(u8, &'a str)],
    pub phase: EquipDrawPhase,
    /// Cursor row inside the active phase column.
    ///
    /// In [`EquipDrawPhase::SlotPicker`] this is the **slot-browse row**,
    /// not a slot index: row `0` is the "Best Equipment" header line and
    /// row `n` is `slots[n - 1]`. That is retail's own row space - the
    /// header is cursor row 0 of `DAT_801E46C0` - and it is what
    /// `engine-menus::equip_session::EquipState::SlotPicker` carries.
    pub cursor: u16,
    /// Active slot index when in `ItemPicker`.
    pub active_slot: u8,
    /// Emit ASCII `>` cursors. `false` when the caller draws the retail
    /// pointing-hand sprite instead ([`equip_screen_sprites_for`]).
    pub text_cursor: bool,
}

/// Build [`TextDraw`]s for the retail equip screen's window contents.
///
/// `party_pen` / `list_pen` / `main_pen` are the content origins of the
/// id-21 / id-23 / id-22 descriptor windows (`legaia_game_tables::menu_windows`).
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
pub fn equip_screen_draws_for_with(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    view: &EquipScreenView<'_>,
    party_pen: (i32, i32),
    list_pen: (i32, i32),
    main_pen: (i32, i32),
) -> Vec<TextDraw> {
    // CLUT 7, the ink every row below stages: `(206, 206, 206)` on retail's
    // screen (`menu_equipment_field`), not white.
    let white: [f32; 4] = MENU_TEXT_WHITE;
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // The party (21) and main (22) windows are the **browse** step's. The
    // candidate step's open script `0x801E4DC8` begins with `[05 00]`
    // (close-all, `FUN_80035A4C`) and reopens only the tab (2), the list (23,
    // snapped in place) and windows 24 / 25 - which sit on the rects 21 and
    // 22 vacate, so drawing those two on would overprint window 25.
    if view.phase == EquipDrawPhase::SlotPicker {
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
        let best = crate::menu::window_painters::pool_label(
            labels,
            legaia_game_tables::menu_pool::MenuLabel::BestEquipment,
            "Best Equipment",
        );
        if view.text_cursor && best_row_active {
            str_at(&mut out, ">", mx + 4, my, gold);
        }
        str_at(
            &mut out,
            best,
            // The pool string is `"@Best Equipment"`: drawn with its blank
            // lead, which costs one pad; the stand-in adds the pad itself.
            mx + 0x10 + crate::menu::window_painters::pool_lead_pad(font, best),
            my,
            // CLUT 7 like the slot rows: the hand marks the row, the tint only
            // backs the text-cursor fallback.
            if view.text_cursor && best_row_active {
                gold
            } else {
                white
            },
        );
        for (i, slot) in view.slots.iter().enumerate() {
            let y = my + (i as i32 + 1) * EQUIP_ROW_PITCH;
            let cursor_here =
                view.phase == EquipDrawPhase::SlotPicker && i as u16 + 1 == view.cursor;
            let row_active =
                view.phase != EquipDrawPhase::SlotPicker && view.active_slot as usize == i;
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

        // Best-Equipment change list (PORT: FUN_801d21c0, `0x801D2404..0x801D2578`):
        // on armament row `i`'s slot line, the change arrow `FUN_8003C310(2)` at
        // X+0x8E in CLUT 0, then (a sprite - [`equip_best_change_sprites_for`])
        // the armament pictogram at X+0xA8, then the pick's name at X+0xB8 in
        // CLUT 7. ASCII stand-in for the arrow glyph, like the rise/fall arrows.
        let grey: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
        for &(row, name) in view.best_changes.iter().take(4) {
            let y = my + (i32::from(row) + 1) * EQUIP_ROW_PITCH;
            // The right arrow under ink 0's CLUT (`li a0,0x2` / `sw zero`
            // ahead of `jal 0x8003C310`, `0x801D248C..0x801D249C`); the
            // ASCII pair stands in for a font without the arrow cells.
            match crate::arts_input::list_cell_draw(
                font,
                legaia_font::sprite_cell_ids::ink_arrow(0, 2),
                (mx + 0x8e, y),
                (0, 0),
                1,
            ) {
                Some(cell) => out.push(cell),
                None => str_at(&mut out, "->", mx + 0x8e, y, grey),
            }
            str_at(&mut out, name, mx + 0xb8, y, white);
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
                // (raised) / 1 (lowered), off the menu-glyph page.
                let (glyph, ink) = if sr.preview > sr.current {
                    (crate::ARROW_RISE, crate::INK_RISE)
                } else {
                    (crate::ARROW_FALL, crate::INK_FALL)
                };
                out.extend(crate::menu::equip_stat_compare::compare_arrow_draws(
                    font,
                    glyph,
                    (mx + 0xe4, y),
                    ink,
                ));
                out.extend(num_field_draws(
                    font,
                    sr.preview.min(999) as u64,
                    mx + 0xf0,
                    y,
                    3,
                    // Retail re-stages ink 7 before the value (`0x801D262C`).
                    white,
                ));
            }
        }
    }

    // Item-list window (id 23): the kind-4 kernel's rows. Row `Y` from the
    // window top at the shared `0xE` pitch, one 12-row page at a time. A
    // pictogram row (class `0x6000` / `0x7000` / `0x9000`) has its name at
    // `WX + 0x1C`, past the verb column a row without one uses.
    if view.phase != EquipDrawPhase::SlotPicker {
        let (lx, ly) = list_pen;
        if view.candidates.is_empty() {
            str_at(&mut out, "(no items)", lx + 10, ly, dim);
        }
        let hovered = usize::from(view.cursor);
        let top = hovered - hovered % PAUSE_LIST_VISIBLE_ROWS;
        out.extend(list_page_header_draws(
            font,
            (top / PAUSE_LIST_VISIBLE_ROWS) as u16 + 1,
            view.candidates.len().div_ceil(PAUSE_LIST_VISIBLE_ROWS) as u16,
            list_pen,
        ));
        for (i, c) in view
            .candidates
            .iter()
            .enumerate()
            .skip(top)
            .take(PAUSE_LIST_VISIBLE_ROWS)
        {
            let y = ly + PAUSE_LIST_ROWS_TOP + (i - top) as i32 * PAUSE_LIST_ROW_PITCH;
            let selected = view.phase == EquipDrawPhase::ItemPicker && i == hovered;
            if selected && view.text_cursor {
                str_at(&mut out, ">", lx, y, gold);
            }
            let x = if c.icon == 0 {
                lx + EQUIP_ROW_NAME_X
            } else {
                lx + EQUIP_ROW_ICON_NAME_X
            };
            let name = if c.icon == 0 {
                EQUIP_REMOVE_ROW_LABEL
            } else {
                c.name
            };
            str_at(&mut out, name, x, y, white);
        }
    }

    out
}

/// The slot browse panel's two shade boxes, content rects `(x, y, w, h)`.
pub type EquipBrowseBoxes = [(i32, i32, i32, i32); 2];

/// Where the slot browse's item panel sits in window 22, off its pen.
pub const EQUIP_BROWSE_INFO_PEN: (i32, i32) = (0x94, 0x0C);

/// The slot browse's item panel: with the hand on a slot row that holds
/// something, window 22's renderer moves its own rect by `(+0x94, +0xC)`,
/// runs the shared item panel `FUN_801D0F1C` there on the worn id and frames
/// two `0x90 x 0x28` boxes under skin `3`, at the panel's origin and `0x38`
/// below it. Returns the panel's text and the two boxes' content rects.
///
/// PORT: FUN_801d21c0 (`0x801D2800..0x801D28DC`)
pub fn equip_browse_info_draws(
    font: &legaia_font::Font,
    main_pen: (i32, i32),
    info: &crate::PauseItemInfo<'_>,
) -> (Vec<TextDraw>, EquipBrowseBoxes) {
    let (x, y) = (
        main_pen.0 + EQUIP_BROWSE_INFO_PEN.0,
        main_pen.1 + EQUIP_BROWSE_INFO_PEN.1,
    );
    (
        crate::item_info_body_draws(font, (x, y), info),
        [(x, y, 0x90, 0x28), (x, y + 0x38, 0x90, 0x28)],
    )
}

/// The candidate list's sprites: each row's pictogram at the verb column
/// (`(WX + 0xC, row_y)`, drawn by the kernel's class arm through `FUN_8002C488`),
/// the hand on the selected row and the blink-gated page arrows, all off the
/// shared kind-4 list placements.
///
/// PORT: FUN_80032A44 (class `0x6000` / `0x7000` / `0x9000` row pictograms)
pub fn equip_candidate_sprites_for(
    rects: &SaveMenuAtlasRects,
    icons: &[u8],
    cursor: u16,
    arrow_fallback: bool,
    list_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::new();
    let hovered = usize::from(cursor);
    let top = hovered - hovered % PAUSE_LIST_VISIBLE_ROWS;
    for (i, &code) in icons
        .iter()
        .enumerate()
        .skip(top)
        .take(PAUSE_LIST_VISIBLE_ROWS)
    {
        let src = match code {
            0x21 => rects.icon_worn,
            0x22 => rects.icon_helmet,
            0x23 => rects.icon_armor,
            0x24 => rects.icon_weapon,
            0x25 => rects.icon_boot,
            0x46 => rects.icon_goods,
            _ => continue,
        };
        push_stage_sprite(
            &mut out,
            src,
            (
                list_pen.0 + EQUIP_ROW_NAME_X,
                list_pen.1 + PAUSE_LIST_ROWS_TOP + (i - top) as i32 * PAUSE_LIST_ROW_PITCH,
            ),
            stage_origin,
            stage_scale,
        );
    }
    push_stage_sprite(
        &mut out,
        rects.cursor,
        list_hand_pos(list_pen, (hovered - top) as u8),
        stage_origin,
        stage_scale,
    );
    out.extend(page_arrow_sprites(
        rects,
        (
            (top / PAUSE_LIST_VISIBLE_ROWS) as u16 + 1,
            icons.len().div_ceil(PAUSE_LIST_VISIBLE_ROWS) as u16,
        ),
        arrow_fallback,
        list_pen,
        stage_origin,
        stage_scale,
    ));
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
/// grid uses, drawn via `FUN_8002C488`). Retail shows 7 rows, and so does
/// the port: the engine's Hand Guard slot (the Ra-Seru byte) is not browsed.
///
/// The hand cursor (the load-screen pointing-finger record) marks the
/// active party row at `party_pen + (-0xC, 0xE*row)` (FUN_801d2094's
/// cursor offset) and, in the slot-picker phase, the main-window
/// slot-browse row at `main_pen + (0, 0xE*row)` - row `0` being the "Best
/// Equipment" line, so `slot_cursor` is the row, not the slot index.
///
/// Each hand draws in the mode its cursor word names. `party_cursor` is the
/// character word `DAT_801E46C4` as the party window reads it
/// (`0x801D2134..0x801D2180`), over `party_rows` rows, the present party's
/// count: the character picker masks it to the row
/// (`0x801D9948..0x801D9958`), so its hand bobs, and the slot browse raises
/// bit `0x1000` on it (`0x801D9A4C`), so the hand stands still while the
/// main window's reads the pad.
///
/// PORT: FUN_801d21c0 / FUN_801d2094 (icon + cursor placement).
/// REF: FUN_8002c488 / FUN_8002b994 - the UI-icon + cursor primitives.
#[allow(clippy::too_many_arguments)]
pub fn equip_screen_sprites_for(
    rects: &SaveMenuAtlasRects,
    n_slot_rows: usize,
    main_pen: (i32, i32),
    party_pen: (i32, i32),
    party_rows: usize,
    party_cursor: crate::menu::window_painters::ChoiceFlags,
    slot_cursor: Option<crate::menu::window_painters::ChoiceFlags>,
    bob: crate::menu::window_painters::HandBob,
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

    // Slot pictogram column, in the browse order the rows are drawn in:
    // retail's `DAT_801E43F4` (weapon / helmet / armor / boot / Goods x3).
    let icons = [
        rects.icon_weapon,
        rects.icon_helmet,
        rects.icon_armor,
        rects.icon_boot,
        rects.icon_goods,
        rects.icon_goods,
        rects.icon_goods,
    ];
    let (mx, my) = main_pen;
    for (i, src) in icons.into_iter().take(n_slot_rows).enumerate() {
        push(src, mx + 0x10, my + EQUIP_ROW_PITCH * (i as i32 + 1));
    }

    // Party-window hand cursor at X-0xC on the active member's row, in the
    // mode the character word names, over the present party's rows
    // (`0x801D2188..0x801D2194`).
    let (px, py) = party_pen;
    for (x, y, variant) in crate::menu::window_painters::list_hand_marks(
        party_cursor,
        party_rows as u32,
        (px - 0x0c, py),
        EQUIP_ROW_PITCH,
    ) {
        let (dx, dy) = bob.offset(variant);
        push(rects.cursor, x + dx, y + dy);
    }

    // Main-window hand cursor on the hovered slot-browse row. Row 0 is the
    // "Best Equipment" line at the window origin (retail's `hand at (X, Y)`),
    // row `n` the slot row `n - 1` at `Y + 0xE*n`.
    // The cursor word `DAT_801E46C0` picks the rows and the hand's mode:
    // static under bit `0x1000`, every row under `0x2000`
    // (`0x801D2230..0x801D2278`, `0x801D22C0..0x801D2318`).
    if let Some(flags) = slot_cursor {
        for (x, y, variant) in
            crate::menu::window_painters::list_hand_marks(flags, 8, (mx, my), EQUIP_ROW_PITCH)
        {
            let (dx, dy) = bob.offset(variant);
            push(rects.cursor, x + dx, y + dy);
        }
    }
    out
}

/// The Best Equipment row's armament pictograms: one per changed armament
/// row, at `main_pen + (0xA8, 0xE*(row+1))` - the icon `FUN_801D21C0` picks
/// from the pick's equipment-record `+7` class (`0x801D24D8..0x801D253C`:
/// class 2 weapon, 1 helmet, 0 armour, 3 boot, each an index into the same
/// `DAT_801E43F4` pictogram array the slot column draws). A Best Equipment
/// pick for armament `i` carries armament `i`'s class by construction, so
/// the row picks the icon.
///
/// PORT: FUN_801d21c0 (the change list's pictogram column)
pub fn equip_best_change_sprites_for(
    rects: &SaveMenuAtlasRects,
    best_rows: &[u8],
    main_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let icons = [
        rects.icon_weapon,
        rects.icon_helmet,
        rects.icon_armor,
        rects.icon_boot,
    ];
    let (mx, my) = main_pen;
    best_rows
        .iter()
        .filter_map(|&row| {
            let src = *icons.get(usize::from(row))?;
            let (_, _, w, h) = src;
            let sx = mx + 0xa8;
            let sy = my + EQUIP_ROW_PITCH * (i32::from(row) + 1);
            Some(SpriteDraw {
                dst: (
                    stage_origin.0 + sx * scale,
                    stage_origin.1 + sy * scale,
                    w * stage_scale,
                    h * stage_scale,
                ),
                src,
                color: [1.0, 1.0, 1.0, 1.0],
            })
        })
        .collect()
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
            best_changes: &[],
            phase,
            cursor,
            active_slot,
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

        let row0 = equip_screen_draws_for_with(
            &font,
            None,
            &view(&slots, &[], EquipDrawPhase::SlotPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        assert_eq!(cursor_rows(&row0, mx + 4), vec![my]);

        let row2 = equip_screen_draws_for_with(
            &font,
            None,
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

    /// The candidate step closes the party (21) and main (22) windows - its
    /// open script `0x801E4DC8` leads with close-all - so neither draws a
    /// glyph there; only the list (23) does.
    #[test]
    fn the_candidate_step_draws_no_party_or_main_rows() {
        let font = legaia_font::synthetic_for_tests();
        let slots = [EquipSlotRow {
            label: "Weapon",
            current_name: "Iron Sword",
        }];
        let cands = [EquipCandidateRow {
            name: "Iron Sword",
            icon: 0x24,
        }];
        let browse = equip_screen_draws_for_with(
            &font,
            None,
            &view(&slots, &cands, EquipDrawPhase::SlotPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        assert!(
            browse
                .iter()
                .any(|d| d.dst.1 < LIST.1 + 200 && d.dst.0 < LIST.0)
        );
        let pick = equip_screen_draws_for_with(
            &font,
            None,
            &view(&slots, &cands, EquipDrawPhase::ItemPicker, 0, 0),
            PARTY,
            LIST,
            MAIN,
        );
        assert!(!pick.is_empty(), "the list window still draws");
        assert!(
            pick.iter().all(|d| d.dst.0 >= LIST.0),
            "nothing left of the list window (the party / main columns)"
        );
    }

    /// The Best Equipment row's change list lands on the changed armament's
    /// own slot line: the arrow at X+0x8E, the pick's name at X+0xB8
    /// (`FUN_801D21C0`, `0x801D2490` / `0x801D2564`).
    #[test]
    fn best_equipment_changes_draw_on_the_armament_row() {
        let font = legaia_font::synthetic_for_tests();
        let slots = [EquipSlotRow {
            label: "Weapon",
            current_name: "Iron Sword",
        }];
        let changes = [(2u8, "Chain Mail")];
        let mut v = view(&slots, &[], EquipDrawPhase::SlotPicker, 0, 0);
        v.best_changes = &changes;
        let draws = equip_screen_draws_for_with(&font, None, &v, PARTY, LIST, MAIN);
        let (mx, my) = MAIN;
        let row_y = my + EQUIP_ROW_PITCH * 3;
        assert_eq!(cursor_rows(&draws, mx + 0xb8), vec![row_y]);
        assert_eq!(cursor_rows(&draws, mx + 0x8e), vec![row_y]);
    }

    /// The candidate list's row 0 is retail's Remove entry when the session
    /// put one there - the one row with no bag count. It carries no item, so
    /// its label comes from here rather than from the host's per-id name.
    #[test]
    fn a_row_with_no_pictogram_is_the_remove_verb_at_the_verb_column() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            EquipCandidateRow {
                name: "REMOVE-PLACEHOLDER",
                icon: 0,
            },
            EquipCandidateRow {
                name: "Iron Sword",
                icon: 0x24,
            },
        ];
        let slots = [EquipSlotRow {
            label: "Weapon",
            current_name: "",
        }];
        let draws = equip_screen_draws_for_with(
            &font,
            None,
            &view(&slots, &rows, EquipDrawPhase::ItemPicker, 1, 0),
            PARTY,
            LIST,
            MAIN,
        );
        let (lx, ly) = LIST;
        let row_y = |row: i32| ly + PAUSE_LIST_ROWS_TOP + row * PAUSE_LIST_ROW_PITCH;
        let on = |row: i32| {
            let y = row_y(row);
            draws
                .iter()
                .filter(move |d| d.dst.0 >= lx && (y..y + PAUSE_LIST_ROW_PITCH).contains(&d.dst.1))
        };
        assert_eq!(
            on(0).count(),
            font.layout_ascii(EQUIP_REMOVE_ROW_LABEL).glyphs.len(),
            "row 0 draws exactly the Remove label"
        );
        assert_eq!(on(0).map(|d| d.dst.0).min(), Some(lx + EQUIP_ROW_NAME_X));
        assert_eq!(
            // Past the text cursor, which marks the hovered row at the pen.
            on(1).map(|d| d.dst.0).filter(|&x| x > lx).min(),
            Some(lx + EQUIP_ROW_ICON_NAME_X)
        );
    }
}
