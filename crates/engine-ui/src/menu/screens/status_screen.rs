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
}

/// Fixed decimal-cell pitch of the retail number primitive
/// `FUN_80034b78`: one glyph cell per digit, 8 px apart (pinned against
/// the golden capture - the HP row's current-value field at `+0x30` puts
/// "180" in cells `+0x38/+0x40/+0x48`, ending flush at the `/` at
/// `+0x50`).
const NUM_CELL_W: i32 = 8;

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
    let s = value.to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (digits - len + i as i32).max(0);
        out.extend(crate::numeral_cell_draws(
            font,
            ch,
            (x + cell * NUM_CELL_W, y),
            color,
        ));
    }
    out
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
/// The derived-stat abbreviations (ATK/UDF/LDF/SPD/INT/AGL) and the
/// number-field separators (`/`, `(`, `)`) are always drawn here as
/// proportional text - retail renders them through the string primitive
/// `FUN_80036888`, not the icon atlas (no `0x800732a4` record exists for
/// them), so there is no icon sprite to stand in for.
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

    // Header: name at +8, "LV" icon at +0x50, level (2-digit) at +0x60.
    str_at(&mut out, panel.name, wx + 8, wy, white);
    if !label_icons {
        str_at(&mut out, "LV", wx + 0x50, wy + 2, gold);
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
        str_at(&mut out, "/", wx + 0x50, row_y, white);
        out.extend(num_field_draws(font, max, wx + 0x58, row_y, 4, tier));
        str_at(&mut out, "(", wx + 0x7c, row_y, teal);
        out.extend(num_field_draws(font, base, wx + 0x84, row_y, 4, teal));
        str_at(&mut out, ")", wx + 0xa4, row_y, teal);
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
        str_at(&mut out, "(", px, row_y, teal);
        out.extend(num_field_draws(
            font,
            sr.growth as u64,
            px + 8,
            row_y,
            3,
            teal,
        ));
        str_at(&mut out, ")", px + 0x20, row_y, teal);
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

    // "Condition" pager: label at WX+6; the flanking solid-triangle
    // sprites are atlas draws (text fallbacks when un-iconed).
    if !label_icons {
        str_at(&mut out, "<", condition_pen.0 - 8, condition_pen.1, white);
        str_at(&mut out, ">", condition_pen.0 + 58, condition_pen.1, white);
    }
    str_at(
        &mut out,
        crate::menu::window_painters::pool_label(
            labels,
            legaia_game_tables::menu_pool::MenuLabel::Condition,
            "Condition",
        ),
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
    // Summary ATR element icon (the 0xCE-token ICO at (+0x20, +0x1a)).
    if let Some(src) = rects.atr_icons.get(atr_char) {
        push(*src, summary_pen.0 + 0x20, summary_pen.1 + 0x1a);
    }
    out
}
