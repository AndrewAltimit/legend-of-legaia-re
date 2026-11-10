//! The status screen, its satellite panels, icon sprites and the AP gauge.

use crate::*;

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
    /// The character's packed battle-status halfword (char record
    /// `+0x12E`); non-zero re-inks the HP number gold through
    /// [`menu_hp_ink_with_status`].
    pub status: u16,
    /// The worn Ra-Seru's name and level.
    pub seru: Option<(&'a str, u8)>,
    /// The pager's page (`DAT_801E46C0`, folded): `0` Condition, `1` Skills,
    /// `2` Magic, `3` Moves.
    pub page: u8,
    pub skills: &'a [StatusSkillRowView<'a>],
    pub spells: &'a [StatusSpellRowView<'a>],
    pub moves: &'a [StatusMoveRowView<'a>],
    /// The Magic / Moves list's window while it holds the pad.
    pub list: Option<StatusListView>,
    /// The list's window start (`_DAT_8007BB90`) when no list holds the pad.
    pub scroll: usize,
}

/// One row of the Skills page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusSkillRowView<'a> {
    pub item_name: &'a str,
    pub effect_name: &'a str,
    /// `|` is the retail line break.
    pub description: &'a str,
}

/// One row of the Magic page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusSpellRowView<'a> {
    pub name: &'a str,
    /// The `0xCE` element-plate escape the raw name opens with.
    pub icon: Option<u8>,
    pub level: u8,
    pub mp_cost: u16,
    /// `|` is the retail line break.
    pub description: &'a str,
}

/// One row of the Moves page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMoveRowView<'a> {
    /// The name string as stored, `0xCF` ink and `0xC1` name escapes kept.
    pub name_bytes: &'a [u8],
    pub ap: u8,
    /// The command string's entries as stored, `0xFF XX` ink markers kept.
    pub glyphs: &'a [[u8; 2]],
    /// `|` is the retail line break.
    pub description: &'a [u8],
}

/// A Status page's list while it holds the pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusListView {
    pub cursor: usize,
    pub scroll: usize,
    /// The row a Magic list's confirm latched (`DAT_801E46D0`).
    pub latched: Option<usize>,
}

/// Rows a Magic or Moves page shows (`slti v0,t1,0x7`, `0x801D436C` / `0x801D472C`).
const STATUS_PAGE_ROWS: usize = 7;
/// The pitch between a string's `|` lines on the Status pages, measured on the
/// `menu_status_*_conc_retail` frames. `FUN_8003CD00` itself steps `0x0D`
/// (`0x8003CD84`); what adds the last pixel is not traced.
const LINE_BREAK_PITCH: i32 = 0x0E;
/// The list pages' preview box, off the window's bottom (`0x801D4830`).
const PREVIEW_H: i32 = 0x28;

/// `FUN_8003CD00`: a string's `|` lines, one pitch apart, from `pen`.
fn lines_draws(
    font: &legaia_font::Font,
    text: &[u8],
    pen: (i32, i32),
    ink: [f32; 4],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (i, line) in text.split(|b| *b == b'|' || *b == b'\n').enumerate() {
        out.extend(text_draws_for(
            &font.layout(line),
            (pen.0, pen.1 + i as i32 * LINE_BREAK_PITCH),
            ink,
        ));
    }
    out
}

/// The Skills page (`0x801D3E64..0x801D4098`): per Goods slot with a passive,
/// the item's name, the effect's name and its description; every slot takes
/// the same pitch.
fn status_skills_draws(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    panel: &StatusPanelView<'_>,
    (wx, wy): (i32, i32),
) -> Vec<TextDraw> {
    use crate::menu::window_painters::pool_label;
    use legaia_game_tables::menu_pool::MenuLabel;
    let x = wx + 0x10;
    let mut out = Vec::new();
    if panel.skills.is_empty() {
        out.extend(text_draws_for(
            &font.layout_ascii(pool_label(
                labels,
                MenuLabel::NoSkills,
                "You do not have any skills.",
            )),
            (x, wy + 0x50),
            crate::menu_ink_color(7),
        ));
        return out;
    }
    let mut y = wy + 0x13;
    for row in panel.skills {
        out.extend(text_draws_for(
            &font.layout_ascii(row.item_name),
            (x + 0x10, y),
            crate::menu_ink_color(6),
        ));
        y += 0x0E;
        out.extend(lines_draws(
            font,
            row.effect_name.as_bytes(),
            (x + 0x20, y),
            crate::menu_ink_color(4),
        ));
        y += 0x0E;
        out.extend(lines_draws(
            font,
            row.description.as_bytes(),
            (x + 0x28, y),
            crate::menu_ink_color(7),
        ));
        y += 0x1F;
    }
    out
}

/// The Magic page (`0x801D40A8..0x801D43BC`): header, up to seven rows from the
/// window start, and with the hand in the list the row's name, level and info
/// lines in the preview box.
fn status_magic_draws(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    panel: &StatusPanelView<'_>,
    (wx, wy, h): (i32, i32, i32),
) -> Vec<TextDraw> {
    use crate::menu::window_painters::pool_label;
    use legaia_game_tables::menu_pool::MenuLabel;
    let x = wx + 0x10;
    let gold = crate::menu_ink_color(6);
    let white = crate::menu_ink_color(7);
    let mut out = Vec::new();
    out.extend(text_draws_for(
        &font.layout_ascii(pool_label(labels, MenuLabel::Magic, "Magic")),
        (x, wy),
        gold,
    ));
    out.extend(text_draws_for(
        &font.layout_ascii(pool_label(labels, MenuLabel::MpUsed, "MP Used")),
        (wx + 0x70, wy),
        gold,
    ));
    let rows_y = wy + 0x15;
    let scroll = panel.list.map_or(panel.scroll, |l| l.scroll);
    if panel.seru.is_none() || panel.spells.is_empty() {
        out.extend(text_draws_for(
            &font.layout_ascii(pool_label(labels, MenuLabel::NoMagic, "-No magic skills-")),
            (x, wy + 0x50),
            white,
        ));
        return out;
    }
    let spell_name = |row: &StatusSpellRowView<'_>, pen: (i32, i32), ink| {
        let mut bytes = Vec::new();
        if let Some(op) = row.icon.filter(|op| font.escape_cell(*op).is_some()) {
            bytes.extend([0xCE, op, b' ']);
        }
        bytes.extend(row.name.bytes());
        text_draws_for(&font.layout(&bytes), pen, ink)
    };
    let level_text = crate::spell_level_text;
    for (i, row) in panel
        .spells
        .iter()
        .skip(scroll)
        .take(STATUS_PAGE_ROWS)
        .enumerate()
    {
        let y = rows_y + i as i32 * 0x0D;
        out.extend(spell_name(row, (x, y), white));
        out.extend(text_draws_for(
            &font.layout_ascii(&level_text(row.level)),
            (x + 0x68, y),
            white,
        ));
        out.extend(num_field_draws(
            font,
            u64::from(row.mp_cost),
            x + 0x98,
            y,
            3,
            white,
        ));
    }
    if let Some(row) = panel.list.and_then(|l| panel.spells.get(l.cursor)) {
        let py = wy + h - PREVIEW_H;
        out.extend(spell_name(row, (wx, py), gold));
        out.extend(text_draws_for(
            &font.layout_ascii(&level_text(row.level)),
            (wx + 0x68, py),
            gold,
        ));
        out.extend(lines_draws(
            font,
            row.description.as_bytes(),
            (wx + 0x0C, py + 0x0E),
            white,
        ));
    }
    out
}

/// The Moves page (`0x801D43CC..0x801D477C`): header, up to seven rows, and
/// with the hand in the list the row's command arrows and description.
fn status_moves_draws(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    panel: &StatusPanelView<'_>,
    (wx, wy, h): (i32, i32, i32),
) -> Vec<TextDraw> {
    use crate::menu::window_painters::pool_label;
    use legaia_font::sprite_cell_ids as ids;
    use legaia_game_tables::menu_pool::MenuLabel;
    let x = wx + 0x10;
    let head = crate::menu_ink_color(9);
    let white = crate::menu_ink_color(7);
    let mut out = Vec::new();
    out.extend(text_draws_for(
        &font.layout_ascii(pool_label(labels, MenuLabel::Moves, "Moves")),
        (x, wy),
        head,
    ));
    out.extend(text_draws_for(
        &font.layout_ascii(pool_label(labels, MenuLabel::ApUsed, "AP Used")),
        (wx + 0x70, wy),
        head,
    ));
    if panel.moves.is_empty() {
        out.extend(text_draws_for(
            &font.layout_ascii(pool_label(
                labels,
                MenuLabel::NoMoves,
                "You have not learned any moves.",
            )),
            (x, wy + 0x50),
            white,
        ));
        return out;
    }
    let rows_y = wy + 0x15;
    let scroll = panel.list.map_or(panel.scroll, |l| l.scroll);
    for (i, row) in panel
        .moves
        .iter()
        .skip(scroll)
        .take(STATUS_PAGE_ROWS)
        .enumerate()
    {
        let y = rows_y + i as i32 * 0x0D;
        // The name's `0xCF XX` restages the ink the AP then prints in.
        let mut ink = 7u8;
        let mut pen = x;
        let mut run = Vec::new();
        let mut bytes = row.name_bytes.iter().copied();
        while let Some(b) = bytes.next() {
            match b {
                0xCF => {
                    pen += crate::arts_input::list_name_run(
                        font,
                        &run,
                        ink,
                        (pen, y),
                        (0, 0),
                        1,
                        &mut out,
                    );
                    run.clear();
                    ink = bytes.next().unwrap_or(ink);
                }
                // `0xC1 XX`: the character's display name.
                0xC1 => {
                    let _ = bytes.next();
                    run.extend(panel.name.bytes());
                }
                _ => run.push(b),
            }
        }
        crate::arts_input::list_name_run(font, &run, ink, (pen, y), (0, 0), 1, &mut out);
        let digits = [row.ap / 100, row.ap / 10 % 10, row.ap % 10];
        let lead = digits.iter().position(|&d| d != 0).unwrap_or(2);
        for (k, &d) in digits.iter().enumerate().skip(lead) {
            out.extend(crate::arts_input::list_cell_draw(
                font,
                ids::ink_numeral(ink, u16::from(d)),
                (x + 0x72 + k as i32 * 8, y),
                (0, 0),
                1,
            ));
        }
    }
    if let Some(row) = panel.list.and_then(|l| panel.moves.get(l.cursor)) {
        let py = wy + h - PREVIEW_H;
        out.extend(text_draws_for(
            &font.layout_ascii(pool_label(labels, MenuLabel::Command, "Command:")),
            (wx, py),
            crate::menu_ink_color(1),
        ));
        // `0x801D45E8..0x801D46CC`: a `0xFF XX` marker restages the ink and
        // takes back the cell the loop's step gives it.
        let mut ax = wx + 0x3C;
        let mut ink = 1u8;
        for &g in row.glyphs {
            if g[0] == 0xFF {
                ink = g[1];
                continue;
            }
            if let Some(dir) = crate::arts_input::arts_list_arrow_dir(g) {
                out.extend(crate::arts_input::list_cell_draw(
                    font,
                    ids::ink_arrow(ink, u16::from(dir)),
                    (ax, py),
                    (0, 0),
                    1,
                ));
            }
            ax += 0x0C;
        }
        out.extend(lines_draws(
            font,
            row.description,
            (wx + 0x0C, wy + h - 0x1A),
            white,
        ));
    }
    out
}

/// The list pages' more-rows arrows (`0x801D47A0..0x801D4808`, icons `0x67` /
/// `0x68`), centred on the window.
fn status_scroll_arrow_draws(
    font: &legaia_font::Font,
    panel: &StatusPanelView<'_>,
    (wx, wy, w): (i32, i32, i32),
) -> Vec<TextDraw> {
    use legaia_font::sprite_cell_ids as ids;
    let count = match panel.page {
        2 => panel.spells.len(),
        3 => panel.moves.len(),
        _ => return Vec::new(),
    };
    let scroll = panel.list.map_or(panel.scroll, |l| l.scroll);
    let mut out = Vec::new();
    let x = wx + (w >> 1) - 4;
    let rows_y = wy + 0x15;
    if scroll > 0 {
        out.extend(crate::arts_input::list_cell_draw(
            font,
            ids::LIST_UP,
            (x, rows_y - 8),
            (0, 0),
            1,
        ));
    }
    if scroll + STATUS_PAGE_ROWS < count {
        out.extend(crate::arts_input::list_cell_draw(
            font,
            ids::LIST_DOWN,
            (x, rows_y + 0x5B),
            (0, 0),
            1,
        ));
    }
    out
}

/// Lay a decimal value into a `digits`-wide fixed-cell field starting at
/// `x` - the shape of the retail decimal primitive
/// `FUN_80034b78(value, digits, x, y)`: digit `i` (of the value's
/// decimal form, right-aligned in the field) draws at its own 8-px cell
/// origin, leading cells stay blank.
pub(crate) fn num_field_draws(
    font: &legaia_font::Font,
    value: u64,
    x: i32,
    y: i32,
    digits: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    crate::menu::decimal_field::decimal_field_draws(
        font,
        i64::try_from(value).unwrap_or(i64::MAX),
        (x, y),
        digits,
        false,
        color,
    )
}

/// Window 28's content size, the Status main panel's descriptor rect.
const fn panel_window_w() -> i32 {
    218
}
const fn panel_window_h() -> i32 {
    188
}

/// Build [`TextDraw`]s for the status panel of one character, at the
/// byte-pinned `FUN_801D33D8` status-page offsets
/// (docs/subsystems/field-menu.md). `pen` is the caller window's content
/// origin `(WX, WY)` - the id-28 descriptor rect origin `(90, 16)` in the
/// retail table. `nav_hint` renders below the panel (an engine addition;
/// pass `None` to omit).
///
/// The retail UI-icon primitives on this page - the "LV"/"HP"/"MP" label
/// tags, the AP gauge bar and the 7-slot equipment pictograms - are ported
/// UI-icon-atlas sprites: pass `label_icons = true` so this function
/// suppresses their ASCII stand-ins and the caller emits the sprites from
/// the `0x800732a4` icon table via [`status_icon_sprites_for`] (which draws
/// the labels + equipment pictograms and folds in [`ap_gauge_sprites`]).
/// `false` keeps the text stand-ins for callers with no atlas.
///
/// The derived-stat abbreviations (ATK/UDF/LDF/SPD/INT/AGL) are always
/// drawn here as proportional text - retail renders them through the string
/// primitive `FUN_80036888`. The number-field separators (`/`, `(`, `)`) are
/// the separator primitive `FUN_8003C1F8`'s 8x12 cells, glyphs `6` / `7` /
/// `8` (`li a0,0x6` / `0x7` / `0x8` before each `jal 0x8003c1f8`, from
/// `0x801D363C` on), drawn off the menu-glyph page when the font carries the
/// cells and as font characters otherwise.
///
/// PORT: FUN_801d33d8 (the status / party panel content renderer - every
/// pen below is its instruction-pinned offset; the submenu list pages it
/// also serves draw through the sibling builders in this module)
pub fn status_screen_draws_for_with(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    panel: &StatusPanelView<'_>,
    nav_hint: Option<&str>,
    pen: (i32, i32),
    // When `true`, the LV / HP / MP labels are omitted here because the
    // caller draws them as sprites from the UI-icon atlas
    // ([`status_icon_sprites_for`]). `false` keeps the ASCII text stand-ins.
    label_icons: bool,
) -> Vec<TextDraw> {
    let (wx, wy) = pen;
    let white: [f32; 4] = MENU_TEXT_WHITE;
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let teal: [f32; 4] = MENU_TEXT_TEAL;
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };
    use crate::menu::window_painters::{
        SEPARATOR_GLYPH_LPAREN, SEPARATOR_GLYPH_RPAREN, SEPARATOR_GLYPH_SLASH,
        separator_glyph_draws,
    };
    let sep =
        |glyph: u8, x: i32, y: i32, c: [f32; 4]| separator_glyph_draws(font, glyph, (x, y), c);

    // Header: name at +8, "LV" icon at +0x50, level (2-digit) at +0x60.
    str_at(&mut out, panel.name, wx + 8, wy, white);
    if !label_icons {
        str_at(&mut out, "LV", wx + 0x50, wy + 2, gold);
    }
    // The worn Ra-Seru (`0x801D3524..0x801D35C8`): tag at +0x8A, name at
    // +0x96, "LV" icon at +0xBE and the one-digit level at +0xCE.
    if let Some((name, level)) = panel.seru {
        out.extend(crate::arts_input::list_cell_draw(
            font,
            legaia_font::sprite_cell_ids::SERU_TAG,
            (wx + 0x8A, wy),
            (0, 0),
            1,
        ));
        str_at(&mut out, name, wx + 0x96, wy, white);
        out.extend(num_field_draws(
            font,
            u64::from(level),
            wx + 0xCE,
            wy,
            1,
            white,
        ));
    }
    let (win_w, win_h) = (panel_window_w(), panel_window_h());
    match panel.page {
        1 => {
            out.extend(num_field_draws(
                font,
                panel.level as u64,
                wx + 0x60,
                wy,
                2,
                white,
            ));
            out.extend(status_skills_draws(font, labels, panel, (wx, wy)));
            return out;
        }
        2 | 3 => {
            out.extend(num_field_draws(
                font,
                panel.level as u64,
                wx + 0x60,
                wy,
                2,
                white,
            ));
            let body = (wx, wy + 0x13, win_h - 0x13);
            out.extend(if panel.page == 2 {
                status_magic_draws(font, labels, panel, body)
            } else {
                status_moves_draws(font, labels, panel, body)
            });
            out.extend(status_scroll_arrow_draws(
                font,
                panel,
                (wx, wy + 0x13, win_w),
            ));
            return out;
        }
        _ => {}
    }
    out.extend(num_field_draws(
        font,
        panel.level as u64,
        wx + 0x60,
        wy,
        2,
        white,
    ));

    // HP / MP rows: label tag at +0x20, current at +0x30, "/" at +0x50
    // (white - the field's last cell ends flush against it, retail's
    // "180/ 180" spacing), max at +0x58, then the teal parenthesised
    // base group: "(" at +0x7c, base at +0x84, ")" at +0xa4 (all 4-digit
    // fields; parens + base value share the teal separator ink).
    // Both number fields draw in the health-tier ink retail stages
    // before them (FUN_800349EC / FUN_80035EA8); the "/" stays white
    // (re-staged 7) and the paren group teal (staged 5).
    for (row_y, tag, cur, max, base, tier) in [
        (
            wy + 0x13,
            "HP",
            panel.hp as u64,
            panel.hp_max as u64,
            panel.hp_max as u64,
            menu_hp_ink_with_status(panel.hp, panel.hp_max, panel.status as i16),
        ),
        (
            wy + 0x20,
            "MP",
            panel.mp as u64,
            panel.mp_max as u64,
            panel.mp_max as u64,
            menu_mp_ink(panel.mp, panel.mp_max),
        ),
    ] {
        if !label_icons {
            str_at(&mut out, tag, wx + 0x20, row_y, gold);
        }
        out.extend(num_field_draws(font, cur, wx + 0x30, row_y, 4, tier));
        out.extend(sep(SEPARATOR_GLYPH_SLASH, wx + 0x50, row_y, white));
        out.extend(num_field_draws(font, max, wx + 0x58, row_y, 4, tier));
        out.extend(sep(SEPARATOR_GLYPH_LPAREN, wx + 0x7c, row_y, teal));
        out.extend(num_field_draws(font, base, wx + 0x84, row_y, 4, teal));
        out.extend(sep(SEPARATOR_GLYPH_RPAREN, wx + 0xa4, row_y, teal));
    }

    // AP gauge line. With `label_icons` the caller draws the retail bar
    // (four gauge sprites + red value digits from the UI-icon atlas via
    // [`status_icon_sprites_for`]); otherwise fall back to the numeric
    // text readout.
    if !label_icons {
        let ap = format!("AP {:>2}/{:<2}", panel.ap, panel.ap_max);
        str_at(&mut out, &ap, wx + 0x40, wy + 0x2d, dim);
    }

    // Derived-stat 3x2 grid: rows at WY+0x42/+0x4f/+0x5c. Left column
    // label +0 / live value (3-digit field) +0x28 / "(" +0x40 / growth
    // (3-digit field) +0x48 / ")" +0x60; right column shifts the same
    // shape to +0x74 / +0x9c / +0xb4 / +0xbc / +0xd4. Parens + growth
    // value in the teal separator ink (golden-capture pinned).
    // `stat_rows` order: left column rows 0..3, right column rows 3..6
    // (ATK/UDF/LDF | SPD/INT/AGL).
    for (i, sr) in panel.stat_rows.iter().take(6).enumerate() {
        let row_y = wy + 0x42 + (i % 3) as i32 * 0x0d;
        let (lx, vx, px) = if i < 3 {
            (wx, wx + 0x28, wx + 0x40)
        } else {
            (wx + 0x74, wx + 0x9c, wx + 0xb4)
        };
        str_at(&mut out, sr.label, lx, row_y, white);
        out.extend(num_field_draws(font, sr.value as u64, vx, row_y, 3, white));
        out.extend(sep(SEPARATOR_GLYPH_LPAREN, px, row_y, teal));
        out.extend(num_field_draws(
            font,
            sr.growth as u64,
            px + 8,
            row_y,
            3,
            teal,
        ));
        out.extend(sep(SEPARATOR_GLYPH_RPAREN, px + 0x20, row_y, teal));
    }

    // Equipment grid: slots 0..3 stack at +0/+0x10 on rows WY+0x6d/+0x7a/
    // +0x87/+0x94; slots 4..6 in a right column at +0x6a/+0x7a on rows
    // WY+0x7a/+0x87/+0x94. With `label_icons` the caller draws the slot
    // pictograms as sprites and the item name lands at the retail name
    // offset (+0x10 past the icon); empty slots stay icon-only, matching
    // retail. Without icons, the item tag text stands in at the icon
    // position.
    for (slot, (label, item)) in panel.equip_rows.iter().take(7).enumerate() {
        let (icon_x, y) = if slot < 4 {
            (wx, wy + 0x6d + slot as i32 * 0x0d)
        } else {
            (wx + 0x6a, wy + 0x7a + (slot as i32 - 4) * 0x0d)
        };
        let _ = label;
        if label_icons {
            if !item.is_empty() {
                str_at(&mut out, item, icon_x + 0x10, y, white);
            }
        } else {
            str_at(&mut out, item, icon_x, y, dim);
        }
    }

    // Experience / Next Level rows.
    use legaia_game_tables::menu_pool::MenuLabel;
    str_at(
        &mut out,
        crate::menu::window_painters::pool_label(labels, MenuLabel::Experience, "Experience"),
        wx + 0x18,
        wy + 0xa5,
        white,
    );
    out.extend(num_field_draws(
        font,
        panel.xp as u64,
        wx + 0x78,
        wy + 0xa5,
        8,
        white,
    ));
    str_at(
        &mut out,
        crate::menu::window_painters::pool_label(labels, MenuLabel::NextLevel, "Next Level"),
        wx + 0x18,
        wy + 0xb2,
        white,
    );
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

/// Build the status-page UI-icon [`SpriteDraw`]s from the system-UI atlas:
/// the LV / HP / MP labels, the AP gauge (four 1:1 pieces + meter fill +
/// value) and the 7-slot equipment pictogram grid. Everything is positioned
/// at the byte-pinned `FUN_801D33D8` offsets and mapped from the 320x240
/// menu stage into surface pixels (`stage_origin` + `stage_scale`, matching
/// [`crate::menu_window_chrome_draws_for`]). Pair with
/// `status_screen_draws_for_with(.., None, label_icons = true)` so the labels / AP
/// value aren't double-drawn as text. `pen` is the id-28 window's content
/// origin in stage pixels (the same value passed to
/// `status_screen_draws_for`); `ap` is the character's current AP (meter
/// width + numeric readout).
///
/// PORT: FUN_8002c0b0 - the gauge content (gouraud meter fill + tens/ones
/// digit / "100"-glyph layout).
/// REF: FUN_8002c488 / FUN_8002c69c - the UI-icon + bar-widget primitives
/// whose `0x800732a4` records supply every source rect and placement.
pub fn status_icon_sprites_for(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    ap: u16,
    page_view: StatusIconPage,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (wx, wy) = pen;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(17);
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
    // LV label in the header row; HP / MP labels at the left of the HP and
    // MP rows, each 2 px below its row's text baseline - the pixel-exact
    // retail placements (golden menu_status_town capture: LV at
    // `(+0x50, +2)`, HP at `(+0x20, +0x15)`, MP at `(+0x20, +0x22)`).
    push(rects.label_lv, wx + 0x50, wy + 2);
    if page_view.seru {
        push(rects.label_lv, wx + 0xBE, wy + 2);
    }
    if page_view.page != 0 {
        drop(push);
        out.extend(status_page_sprites(
            rects,
            (wx, wy),
            page_view,
            stage_origin,
            stage_scale,
        ));
        return out;
    }
    push(rects.label_hp, wx + 0x20, wy + 0x15);
    push(rects.label_mp, wx + 0x20, wy + 0x22);

    // Equipment pictograms: slots 0..3 stack at +0 on rows +0x6d/+0x7a/
    // +0x87/+0x94; slots 4..6 (the Goods ring) sit in a right column at
    // +0x6a on the last three rows. Icon-per-slot is fixed (the menu
    // overlay's DAT_801e43f4 code array), independent of what's equipped.
    for (i, src) in [
        rects.icon_weapon,
        rects.icon_helmet,
        rects.icon_armor,
        rects.icon_boot,
    ]
    .into_iter()
    .enumerate()
    {
        push(src, wx, wy + 0x6d + 0x0d * i as i32);
    }
    for k in 0..3 {
        push(rects.icon_goods, wx + 0x6a, wy + 0x7a + 0x0d * k);
    }

    // AP gauge at the retail bar anchor (+0x40, +0x2d): frame pieces,
    // value digits and meter fill (shared widget - see
    // [`ap_gauge_sprites`]).
    out.extend(ap_gauge_sprites(
        rects,
        (wx + 0x40, wy + 0x2d),
        ap,
        stage_origin,
        stage_scale,
    ));
    out
}

/// What of the panel [`status_icon_sprites_for`] draws around the page text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusIconPage {
    pub page: u8,
    pub seru: bool,
    /// Rows the Skills page lists, one Goods icon each.
    pub skills: usize,
    /// The list hand's row within the window (`0x801D4190` / `0x801D4564`).
    pub hand_row: Option<usize>,
    /// The Magic list's latched row within the window, marked by a second
    /// hand three pixels up (`0x801D42BC`).
    pub latched_row: Option<usize>,
}

/// The page sprites past the header: the Skills page's Goods icons, and on
/// the list pages the preview box's frame and the list hands.
fn status_page_sprites(
    rects: &SaveMenuAtlasRects,
    (wx, wy): (i32, i32),
    view: StatusIconPage,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    let x = wx + 0x10;
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
    match view.page {
        1 => {
            for i in 0..view.skills {
                push(
                    rects.icon_goods,
                    x,
                    wy + 0x13 + i as i32 * (0x0E + 0x0E + 0x1F),
                );
            }
            out
        }
        2 | 3 => {
            let rows_y = wy + 0x13 + 0x15;
            if let Some(r) = view.hand_row {
                push(rects.cursor, x - 0x14, rows_y + r as i32 * 0x0D);
            }
            if let Some(r) = view.latched_row {
                push(rects.cursor, x - 0x14, rows_y + r as i32 * 0x0D - 3);
            }
            // `FUN_8002C69C(WX, WY + h - 0x28, w, 0x28)`, skin kind 3.
            out.extend(crate::menu_window_chrome_draws_for(
                rects,
                (
                    wx - 8,
                    wy + panel_window_h() - PREVIEW_H - 8,
                    panel_window_w() + 16,
                    PREVIEW_H + 16,
                ),
                stage_origin,
                stage_scale,
            ));
            out
        }
        _ => out,
    }
}

/// The retail **AP gauge widget** (staged kind `0x31` of the bar-widget
/// dispatcher `FUN_8002c69c`) as atlas [`SpriteDraw`]s at an arbitrary
/// bar anchor: cap, trough, value box, right tip - four 1:1 sprites laid
/// edge-to-edge (cap 24 wide, trough 56 wide; box = ICO record 0x69 at
/// anchor+0x50, tip = ICO 0x6A at anchor+0x60) - plus the value digits
/// and the gouraud meter fill. The status main panel draws it at
/// `(WX+0x40, WY+0x2d)` (`FUN_801D33D8`) and the top-level party info
/// panel per member at `(WX+0x28, row+0x29)` (`FUN_801D030C`); both are
/// the same widget instance in retail.
///
/// PORT: FUN_8002c0b0 - the gauge content (gouraud meter fill +
/// tens/ones digit / "100"-glyph layout).
pub fn ap_gauge_sprites(
    rects: &SaveMenuAtlasRects,
    anchor: (i32, i32),
    ap: u16,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (gauge_x, gauge_y) = anchor;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(8);
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
    push(rects.gauge_cap, gauge_x, gauge_y);
    push(rects.gauge_trough, gauge_x + 0x18, gauge_y);
    push(rects.gauge_box, gauge_x + 0x50, gauge_y);
    push(rects.gauge_tip, gauge_x + 0x60, gauge_y);

    // Value (FUN_8002c0b0): a full 100 draws the dedicated "100" glyph
    // at anchor+0x50; below 100, the tens digit (when non-zero) lands
    // at anchor+0x50 and the ones digit at anchor+0x56 - 6x6 cells from
    // the digit strip (ICO codes 0x6C..=0x75), 5 px below the gauge top.
    let ap = ap.min(100);
    let (dgx, dgy, _, _) = rects.gauge_digits;
    let digit_y = gauge_y + 5;
    if ap == 100 {
        push(rects.gauge_100, gauge_x + 0x50, digit_y);
    } else {
        let (tens, ones) = (u32::from(ap) / 10, u32::from(ap) % 10);
        if tens > 0 {
            push((dgx + tens * 6, dgy, 6, 6), gauge_x + 0x50, digit_y);
        }
        push((dgx + ones * 6, dgy, 6, 6), gauge_x + 0x56, digit_y);
    }

    // Meter fill (FUN_8002c0b0): `value/2` px wide (max 50 at AP 100)
    // from anchor+0x1B, 6 rows starting 5 below the gauge top - the
    // baked gradient column stretched horizontally. Retail prepends the
    // fill quads into the frame's OT bucket so they render on top of
    // the trough; appending here (list order = draw order) stacks the
    // same way.
    let fill_w = u32::from(ap) / 2;
    if fill_w > 0 {
        let (fx, fy, fw, fh) = rects.gauge_fill;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + (gauge_x + 0x1b) * scale,
                stage_origin.1 + (gauge_y + 5) * scale,
                fill_w * stage_scale,
                fh * stage_scale,
            ),
            src: (fx, fy, fw, fh),
            color: [1.0, 1.0, 1.0, 1.0],
        });
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
    /// The pager's page, which names its label.
    pub page: u8,
    /// The worn Ra-Seru's name and level (summary window).
    pub seru: Option<(&'a str, u8)>,
}

/// Build [`TextDraw`]s for the status screen's three satellite windows
/// (party list id 26, "Condition" pager id 27, character summary id 30),
/// at their pinned descriptor-rect content origins.
///
/// With `label_icons` the sprite stand-ins (hand cursor, pager
/// triangles, LV label, ATR element icon) are omitted here because the
/// caller draws them from the UI-icon atlas
/// ([`status_satellite_icon_sprites_for`]); `false` keeps ASCII text
/// stand-ins.
///
/// PORT: FUN_801D2094 - party list (name at `WX+6`, row pitch `0x0e`,
/// hand cursor at `WX-0xc`).
/// PORT: FUN_801D30A4 - "Condition" pager (label at `WX+6`, arrow
/// sprites at `WX-0x10` / `WX+0x3A`, `WY-2`).
/// PORT: FUN_801D31EC - summary window (name at `+0`, LV icon at
/// `(+0x1c, +0xf)`, 2-digit level field at `(+0x2c, +0xd)`, "ATR:" at
/// `(+0, +0x1a)` with the element icon at `+0x20`).
pub fn status_satellite_draws_for_with(
    font: &legaia_font::Font,
    labels: Option<&legaia_game_tables::menu_pool::MenuPoolLabels>,
    view: &StatusSatelliteView<'_>,
    list_pen: (i32, i32),
    condition_pen: (i32, i32),
    summary_pen: (i32, i32),
    label_icons: bool,
) -> Vec<TextDraw> {
    /// Retail party-list row pitch (`FUN_801D2094` steps Y by `0x0e`).
    const LIST_PITCH: i32 = 14;
    let white: [f32; 4] = MENU_TEXT_WHITE;
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    let str_at = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        out.extend(text_draws_for(&font.layout_ascii(s), (x, y), c));
    };

    // Party list: one name per row at WX+6, every row plain white (the
    // selection is the pointing-hand sprite overhanging the window's
    // left frame at WX-0xc, not an ink change).
    for (i, name) in view.party_names.iter().enumerate() {
        let y = list_pen.1 + i as i32 * LIST_PITCH;
        if !label_icons && i == view.cursor {
            str_at(&mut out, ">", list_pen.0 - 8, y, white);
        }
        str_at(&mut out, name, list_pen.0 + 6, y, white);
    }

    // The pager's label names the page (`FUN_801D30A4`).
    // "Condition" pager: label at WX+6; the flanking solid-triangle
    // sprites are atlas draws (text fallbacks when un-iconed).
    if !label_icons {
        str_at(&mut out, "<", condition_pen.0 - 8, condition_pen.1, white);
        str_at(&mut out, ">", condition_pen.0 + 58, condition_pen.1, white);
    }
    str_at(
        &mut out,
        {
            use legaia_game_tables::menu_pool::MenuLabel;
            let (label, fallback) = match view.page {
                1 => (MenuLabel::Skills, "Skills"),
                2 => (MenuLabel::Magic, "Magic"),
                3 => (MenuLabel::Moves, "Moves"),
                _ => (MenuLabel::Condition, "Condition"),
            };
            crate::menu::window_painters::pool_label(labels, label, fallback)
        },
        condition_pen.0 + 6,
        condition_pen.1,
        white,
    );

    // Character summary: name at the content origin, the LV icon +
    // 2-digit level field on the next line, "ATR:" + element icon below.
    str_at(&mut out, view.name, summary_pen.0, summary_pen.1, white);
    if !label_icons {
        str_at(
            &mut out,
            "LV",
            summary_pen.0 + 0x1c,
            summary_pen.1 + 0x0f,
            gold,
        );
    }
    out.extend(num_field_draws(
        font,
        view.level as u64,
        summary_pen.0 + 0x2c,
        summary_pen.1 + 0x0d,
        2,
        white,
    ));
    str_at(
        &mut out,
        crate::menu::window_painters::pool_label(
            labels,
            legaia_game_tables::menu_pool::MenuLabel::Atr,
            "ATR:",
        ),
        summary_pen.0,
        summary_pen.1 + 0x1a,
        white,
    );
    // A character carrying a Seru (`FUN_801D31EC`): tag + name at `WY + 0x2F`,
    // the "LV" icon and level at `WY + 0x3C`.
    if let Some((name, level)) = view.seru {
        out.extend(crate::arts_input::list_cell_draw(
            font,
            legaia_font::sprite_cell_ids::SERU_TAG,
            (summary_pen.0, summary_pen.1 + 0x2F),
            (0, 0),
            1,
        ));
        str_at(
            &mut out,
            name,
            summary_pen.0 + 0x0C,
            summary_pen.1 + 0x2F,
            white,
        );
        out.extend(num_field_draws(
            font,
            u64::from(level),
            summary_pen.0 + 0x34,
            summary_pen.1 + 0x3C,
            1,
            white,
        ));
    }

    out
}

/// Build the status-screen satellite-window UI-icon [`SpriteDraw`]s from
/// the system-UI atlas: the party-list pointing-hand cursor, the
/// "Condition" pager triangles, the summary window's LV label and the
/// per-character ATR element icon. Positions are the traced renderer
/// offsets (see [`status_satellite_draws_for_with`]); the stage mapping
/// matches [`crate::menu_window_chrome_draws_for`]. Pair with
/// `status_satellite_draws_for_with(.., None, label_icons = true)`.
///
/// `cursor` is the highlighted party-list row; `atr_char` indexes
/// [`SaveMenuAtlasRects::atr_icons`] (roster character id 0=Vahn,
/// 1=Noa, 2=Gala; out-of-range draws no element icon).
///
/// REF: FUN_8002b994 - the animated-cursor sprite primitive behind the
/// hand + pager triangles (frame-0 statics here; the 2-px idle bob is
/// not reproduced).
#[allow(clippy::too_many_arguments)]
pub fn status_satellite_icon_sprites_for(
    rects: &SaveMenuAtlasRects,
    cursor: usize,
    atr_char: usize,
    seru: bool,
    list_pen: (i32, i32),
    condition_pen: (i32, i32),
    summary_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    const LIST_PITCH: i32 = 14;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(5);
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
    // Party-list hand cursor (FUN_801D2094: sprite-table kind 0 at
    // (WX-0xc, row_y)).
    push(
        rects.cursor,
        list_pen.0 - 0x0c,
        list_pen.1 + cursor as i32 * LIST_PITCH,
    );
    // Pager triangles (FUN_801D30A4: kinds 2/3 at WX-0x10 / WX+0x3A,
    // both at WY-2).
    push(
        rects.pager_left,
        condition_pen.0 - 0x10,
        condition_pen.1 - 2,
    );
    push(
        rects.pager_right,
        condition_pen.0 + 0x3a,
        condition_pen.1 - 2,
    );
    // Summary LV label icon (ICO code 0x0a at (+0x1c, +0xf)).
    push(rects.label_lv, summary_pen.0 + 0x1c, summary_pen.1 + 0x0f);
    if seru {
        push(rects.label_lv, summary_pen.0 + 0x24, summary_pen.1 + 0x3E);
    }
    // Summary ATR element icon (the 0xCE-token ICO at (+0x20, +0x1a)).
    if let Some(src) = rects.atr_icons.get(atr_char) {
        push(*src, summary_pen.0 + 0x20, summary_pen.1 + 0x1a);
    }
    out
}
