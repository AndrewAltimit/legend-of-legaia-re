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

/// Plain-data row for the field-menu draw. Engines build these from
/// `engine_core::field_menu::FieldMenuView::rows` so this crate doesn't
/// depend on engine-core.
pub struct FieldMenuRowView<'a> {
    pub label: &'a str,
    pub enabled: bool,
}

/// Build [`TextDraw`]s for the field (pause) menu command list. `cursor` is
/// the row index; greyed-out rows render dim.
///
/// Layout follows the retail top-level pause menu's window-descriptor
/// geometry (menu overlay window table, `legaia_asset::menu_windows`):
/// the command rows fill the id-50 list window (`list_pen` = its content
/// origin, 7 rows at the retail 13-px list pitch), and the money +
/// play-time lines fill the id-49 corner box (`money_pen`). The row
/// *content* layout (cursor glyph, label inset) is engine-styled - the
/// retail list renderer `FUN_801CFD68` is untraced.
pub fn field_menu_draws_for(
    font: &legaia_font::Font,
    rows: &[FieldMenuRowView<'_>],
    cursor: u8,
    money: u32,
    play_time_seconds: u32,
    list_pen: (i32, i32),
    money_pen: (i32, i32),
) -> Vec<TextDraw> {
    /// Retail list pitch (the `0x0d` row step of the menu-overlay list pages).
    const LIST_PITCH: i32 = 13;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let y = list_pen.1 + i as i32 * LIST_PITCH;
        let selected = i as u8 == cursor;
        let color = if !row.enabled {
            dim
        } else if selected {
            gold
        } else {
            white
        };
        if selected && row.enabled {
            let cur = font.layout_ascii(">");
            out.extend(text_draws_for(&cur, (list_pen.0, y), color));
        }
        let l = font.layout_ascii(row.label);
        out.extend(text_draws_for(&l, (list_pen.0 + 14, y), color));
    }

    // Money + play-time corner box (two 12-px lines in the 104x24 window).
    let g = format!("{}G", money);
    let g_l = font.layout_ascii(&g);
    out.extend(text_draws_for(&g_l, money_pen, white));
    let h = play_time_seconds / 3600;
    let m = (play_time_seconds % 3600) / 60;
    let s = play_time_seconds % 60;
    let t = format!("{h:02}:{m:02}:{s:02}");
    let t_l = font.layout_ascii(&t);
    out.extend(text_draws_for(&t_l, (money_pen.0, money_pen.1 + 12), white));

    out
}

/// One party member's line block for the top-level menu's right info panel.
pub struct FieldMenuPartyView<'a> {
    pub name: &'a str,
    pub level: u8,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// Build [`TextDraw`]s for the top-level pause menu's right party-overview
/// panel (window id 51, content origin `pen`). Content layout is
/// engine-styled (the retail renderer `FUN_801D030C` is untraced); the
/// window geometry is the pinned descriptor rect.
pub fn field_menu_info_draws_for(
    font: &legaia_font::Font,
    party: &[FieldMenuPartyView<'_>],
    pen: (i32, i32),
) -> Vec<TextDraw> {
    /// Per-member block pitch: 152x180 window / up to 4 members.
    const BLOCK_PITCH: i32 = 44;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    for (i, m) in party.iter().take(4).enumerate() {
        let y = pen.1 + i as i32 * BLOCK_PITCH;
        let head = format!("{}  LV {}", m.name, m.level);
        out.extend(text_draws_for(&font.layout_ascii(&head), (pen.0, y), gold));
        let hp = format!("HP {:>4}/{:<4}", m.hp, m.hp_max);
        out.extend(text_draws_for(
            &font.layout_ascii(&hp),
            (pen.0 + 8, y + 13),
            white,
        ));
        let mp = format!("MP {:>3}/{:<3}", m.mp, m.mp_max);
        out.extend(text_draws_for(
            &font.layout_ascii(&mp),
            (pen.0 + 8, y + 26),
            white,
        ));
    }
    out
}

/// One stat row for the status screen: retail draws a live value plus the
/// parenthesised growth value beside it.
pub struct StatusStatRow<'a> {
    pub label: &'a str,
    pub value: u32,
    pub growth: u32,
}

/// Plain-data view of a single character's status panel.
pub struct StatusPanelView<'a> {
    pub name: &'a str,
    pub level: u8,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap: u8,
    pub ap_max: u8,
    pub stat_rows: &'a [StatusStatRow<'a>],
    pub equip_rows: &'a [(&'a str, &'a str)],
}

/// Right-align a numeric string so its last glyph ends at
/// `x + digits * 6` - the shape of the retail decimal primitive
/// `FUN_80034b78(value, x, y, digits)`, which fills a `digits`-wide cell
/// field from the left edge `x`.
fn num_field_draws(
    font: &legaia_font::Font,
    value: u64,
    x: i32,
    y: i32,
    digits: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.to_string();
    let l = font.layout_ascii(&s);
    let w = l.advance_x as i32;
    text_draws_for(&l, (x + digits * 6 - w, y), color)
}

/// Build [`TextDraw`]s for the status panel of one character, at the
/// byte-pinned `FUN_801D33D8` status-page offsets
/// (docs/subsystems/field-menu.md). `pen` is the caller window's content
/// origin `(WX, WY)` - the id-28 descriptor rect origin `(90, 16)` in the
/// retail table. `nav_hint` renders below the panel (an engine addition;
/// pass `None` to omit).
///
/// Icon primitives (the "HP"/"MP" tags, LV label, equipment icons and the
/// HP gauge bar) are approximated with text glyphs at the icon positions -
/// the retail UI-icon atlas (`0x800732a4` table) is not yet ported.
pub fn status_screen_draws_for(
    font: &legaia_font::Font,
    panel: &StatusPanelView<'_>,
    nav_hint: Option<&str>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let (wx, wy) = pen;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let teal: [f32; 4] = [0.5, 1.0, 0.85, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Header: name at +8, "LV" icon at +0x50, level (2-digit) at +0x60.
    str_at(&mut out, panel.name, wx + 8, wy, white);
    str_at(&mut out, "LV", wx + 0x50, wy + 2, gold);
    out.extend(num_field_draws(
        font,
        panel.level as u64,
        wx + 0x60,
        wy,
        2,
        white,
    ));

    // HP / MP rows: label tag at +0x20, current at +0x30, "/" at +0x50,
    // max at +0x58, "(" at +0x7c, base at +0x84, ")" at +0xa4 (all
    // 4-digit fields).
    for (row_y, tag, cur, max, base) in [
        (
            wy + 0x13,
            "HP",
            panel.hp as u64,
            panel.hp_max as u64,
            panel.hp_max as u64,
        ),
        (
            wy + 0x20,
            "MP",
            panel.mp as u64,
            panel.mp_max as u64,
            panel.mp_max as u64,
        ),
    ] {
        str_at(&mut out, tag, wx + 0x20, row_y, gold);
        out.extend(num_field_draws(font, cur, wx + 0x30, row_y, 4, white));
        str_at(&mut out, "/", wx + 0x50, row_y, dim);
        out.extend(num_field_draws(font, max, wx + 0x58, row_y, 4, white));
        str_at(&mut out, "(", wx + 0x7c, row_y, dim);
        out.extend(num_field_draws(font, base, wx + 0x84, row_y, 4, teal));
        str_at(&mut out, ")", wx + 0xa4, row_y, dim);
    }

    // AP gauge line (retail draws a bar primitive at (+0x40, WY+0x2d);
    // approximated as a numeric readout until the bar prim is ported).
    let ap = format!("AP {:>2}/{:<2}", panel.ap, panel.ap_max);
    str_at(&mut out, &ap, wx + 0x40, wy + 0x2d, dim);

    // Derived-stat 3x2 grid: rows at WY+0x42/+0x4f/+0x5c. Left column
    // label +0 / live +0x28 / growth +0x48; right column label +0x74 /
    // live +0x9c / growth +0xbc. `stat_rows` order: left column rows
    // 0..3, right column rows 3..6 (ATK/UDF/LDF | SPD/INT/AGL).
    for (i, sr) in panel.stat_rows.iter().take(6).enumerate() {
        let row_y = wy + 0x42 + (i % 3) as i32 * 0x0d;
        let (lx, vx, gx) = if i < 3 {
            (wx, wx + 0x28, wx + 0x48)
        } else {
            (wx + 0x74, wx + 0x9c, wx + 0xbc)
        };
        str_at(&mut out, sr.label, lx, row_y, white);
        out.extend(num_field_draws(font, sr.value as u64, vx, row_y, 3, white));
        str_at(&mut out, "(", gx, row_y, dim);
        out.extend(num_field_draws(
            font,
            sr.growth as u64,
            gx + 4,
            row_y,
            3,
            teal,
        ));
        str_at(&mut out, ")", gx + 4 + 20, row_y, dim);
    }

    // Equipment grid: slots 0..3 stack at +0/+0x10 on rows WY+0x6d/+0x7a/
    // +0x87/+0x94; slots 4..6 in a right column at +0x6a/+0x7a on rows
    // WY+0x7a/+0x87/+0x94. Retail draws item ICONS here; until the icon
    // atlas is ported, draw the item tag text at the icon position.
    for (slot, (label, item)) in panel.equip_rows.iter().take(7).enumerate() {
        let (x, y) = if slot < 4 {
            (wx, wy + 0x6d + slot as i32 * 0x0d)
        } else {
            (wx + 0x6a, wy + 0x7a + (slot as i32 - 4) * 0x0d)
        };
        let _ = label;
        str_at(&mut out, item, x, y, dim);
    }

    // Experience / Next Level rows.
    str_at(&mut out, "Experience", wx + 0x18, wy + 0xa5, white);
    out.extend(num_field_draws(
        font,
        panel.xp as u64,
        wx + 0x78,
        wy + 0xa5,
        8,
        white,
    ));
    str_at(&mut out, "Next Level", wx + 0x18, wy + 0xb2, white);
    out.extend(num_field_draws(
        font,
        panel.xp_to_next as u64,
        wx + 0x78,
        wy + 0xb2,
        8,
        white,
    ));

    if let Some(hint) = nav_hint {
        str_at(&mut out, hint, wx - 40, wy + 0xd0, dim);
    }
    out
}

/// Party names for the status screen's satellite windows.
pub struct StatusSatelliteView<'a> {
    /// Party member names (the left party-list window rows).
    pub party_names: &'a [&'a str],
    /// Highlighted member index.
    pub cursor: usize,
    /// Highlighted member's name (summary window).
    pub name: &'a str,
    /// Highlighted member's level (summary window).
    pub level: u8,
}

/// Build [`TextDraw`]s for the status screen's three satellite windows
/// (party list id 26, "Condition" pager id 27, character summary id 30),
/// at their pinned descriptor-rect content origins.
pub fn status_satellite_draws_for(
    font: &legaia_font::Font,
    view: &StatusSatelliteView<'_>,
    list_pen: (i32, i32),
    condition_pen: (i32, i32),
    summary_pen: (i32, i32),
) -> Vec<TextDraw> {
    const LIST_PITCH: i32 = 13;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Party list: one name per row, the highlighted row cursor-marked
    // (retail draws a pointing-hand icon overhanging the window's left
    // edge).
    for (i, name) in view.party_names.iter().enumerate() {
        let y = list_pen.1 + i as i32 * LIST_PITCH;
        let color = if i == view.cursor { gold } else { white };
        if i == view.cursor {
            str_at(&mut out, ">", list_pen.0 - 8, y, color);
        }
        str_at(&mut out, name, list_pen.0, y, color);
    }

    // "Condition" pager (retail: left/right arrow icons flank the label).
    str_at(&mut out, "<", condition_pen.0 - 8, condition_pen.1, white);
    str_at(
        &mut out,
        "Condition",
        condition_pen.0 + 2,
        condition_pen.1,
        white,
    );
    str_at(&mut out, ">", condition_pen.0 + 56, condition_pen.1, white);

    // Character summary: name, LV, ATR rows (the ATR element icon is
    // unported; label only).
    str_at(&mut out, view.name, summary_pen.0, summary_pen.1, white);
    str_at(&mut out, "LV", summary_pen.0 + 12, summary_pen.1 + 14, gold);
    out.extend(num_field_draws(
        font,
        view.level as u64,
        summary_pen.0 + 28,
        summary_pen.1 + 14,
        2,
        white,
    ));
    str_at(&mut out, "ATR:", summary_pen.0, summary_pen.1 + 28, white);

    out
}
