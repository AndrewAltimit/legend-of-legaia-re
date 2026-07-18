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
    /// Retail command-list row pitch (`FUN_801CFD68` steps each row's Y
    /// by `0x0e`).
    const LIST_PITCH: i32 = 14;
    let white: [f32; 4] = MENU_TEXT_WHITE;
    // Disabled rows stage string CLUT 0 (the grayed row ink retail uses
    // for a blocked Save / Load).
    let dim: [f32; 4] = [0.28, 0.28, 0.28, 1.0];

    let mut out = Vec::new();
    // Command rows at (WX+0x14, WY + n*0xe), all CLUT-7 white - the
    // selection is the pointing-hand sprite at WX (FUN_8002b994, drawn
    // by the sprite chrome pass), not an ink change.
    let _ = cursor;
    for (i, row) in rows.iter().enumerate() {
        let y = list_pen.1 + i as i32 * LIST_PITCH;
        let l = font.layout_ascii(row.label);
        out.extend(text_draws_for(
            &l,
            (list_pen.0 + 0x14, y),
            if row.enabled { white } else { dim },
        ));
    }

    // Money / play-time corner box (FUN_801D0148, id-49 window): the
    // money amount as an 8-digit field at (WX+0x28, WY) beside the money
    // pictogram, and the play time as H:MM:SS on the row below the time
    // tag - hours 3-wide (clamped 99, minutes/seconds then pin 59),
    // colons at +0x38/+0x50, zero-padded minutes at +0x40, seconds at
    // +0x58. The icons are sprite-pass draws
    // ([`field_menu_icon_sprites_for`]).
    out.extend(num_field_draws(
        font,
        money as u64,
        money_pen.0 + 0x28,
        money_pen.1,
        8,
        white,
    ));
    let time_y = money_pen.1 + 0x0e;
    let mut h = play_time_seconds / 3600;
    let mut m = (play_time_seconds % 3600) / 60;
    let mut s = play_time_seconds % 60;
    if h > 99 {
        h = 99;
        m = 59;
        s = 59;
    }
    out.extend(num_field_draws(
        font,
        h as u64,
        money_pen.0 + 0x20,
        time_y,
        3,
        white,
    ));
    let colon = |out: &mut Vec<TextDraw>, x: i32| {
        out.extend(text_draws_for(
            &font.layout_ascii(":"),
            (money_pen.0 + x, time_y),
            white,
        ));
    };
    colon(&mut out, 0x38);
    out.extend(zero_padded_field_draws(
        font,
        m as u64,
        money_pen.0 + 0x40,
        time_y,
        2,
        white,
    ));
    colon(&mut out, 0x50);
    out.extend(zero_padded_field_draws(
        font,
        s as u64,
        money_pen.0 + 0x58,
        time_y,
        2,
        white,
    ));

    out
}

/// Lay a decimal value into a `digits`-wide fixed-cell field with
/// leading zeros - the retail zero-padded number primitive
/// `FUN_80034e4c` (minutes / seconds of the play-time clock). Same 8-px
/// cell pitch as [`num_field_draws`].
fn zero_padded_field_draws(
    font: &legaia_font::Font,
    value: u64,
    x: i32,
    y: i32,
    digits: usize,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = format!("{value:0digits$}");
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell_x = x + i as i32 * 8;
        let l = font.layout_ascii(&ch.to_string());
        out.extend(text_draws_for(&l, (cell_x, y), color));
    }
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
    /// Persistent AP (char record `+0x10E`) for the per-member gauge.
    pub ap: u16,
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
    /// Retail per-member block pitch (`FUN_801D030C` steps each roster
    /// row's Y by `0x3e`).
    const BLOCK_PITCH: i32 = 0x3e;
    let white: [f32; 4] = MENU_TEXT_WHITE;

    let mut out = Vec::new();
    for (i, m) in party.iter().take(3).enumerate() {
        let y = pen.1 + i as i32 * BLOCK_PITCH;
        // Name at +0x10; level as a 2-digit field at +0x80 beside the
        // LV label sprite; HP / MP current/max as 4-digit fields at
        // +0x38 / +0x60 with the slash at +0x58, on rows +0xf / +0x1c
        // (the label sprites at +0x28 sit 2 px lower - sprite pass).
        // The HP / MP number fields ink through the per-member
        // health-tier fns ([`menu_hp_ink`] / [`menu_mp_ink`], the
        // FUN_800349EC / FUN_80035EA8 ports); the slash stays white.
        out.extend(text_draws_for(
            &font.layout_ascii(m.name),
            (pen.0 + 0x10, y),
            white,
        ));
        out.extend(num_field_draws(
            font,
            m.level as u64,
            pen.0 + 0x80,
            y,
            2,
            white,
        ));
        for (row_y, cur, max, tier) in [
            (
                y + 0x0f,
                m.hp as u64,
                m.hp_max as u64,
                menu_hp_ink(m.hp, m.hp_max),
            ),
            (
                y + 0x1c,
                m.mp as u64,
                m.mp_max as u64,
                menu_mp_ink(m.mp, m.mp_max),
            ),
        ] {
            out.extend(num_field_draws(font, cur, pen.0 + 0x38, row_y, 4, tier));
            out.extend(text_draws_for(
                &font.layout_ascii("/"),
                (pen.0 + 0x58, row_y),
                white,
            ));
            out.extend(num_field_draws(font, max, pen.0 + 0x60, row_y, 4, tier));
        }
    }
    out
}

/// UI-icon sprites for the top-level pause menu (the sprite-pass
/// complement of [`field_menu_draws_for`] + [`field_menu_info_draws_for`]):
/// the command-list pointing-hand cursor, the money / play-time box
/// pictograms, and per party member the LV / HP / MP label sprites plus
/// the AP gauge.
///
/// PORT: FUN_801CFD68 - command list (rows at `WY + n*0xe`, hand cursor
/// at `WX` on the selected row).
/// PORT: FUN_801D0148 - money / play-time box (money icon `0x62` at
/// `(WX, WY+2)`, time tag `0x63` at `(WX, WY+0x10)` when no coin row is
/// shown).
/// PORT: FUN_801D030C - party info panel (LV `0x0a` at `(+0x70, +2)`,
/// HP `0x3f` at `(+0x28, +0x11)`, MP `0x40` at `(+0x28, +0x1e)`, AP
/// gauge at `(+0x28, +0x29)`, member stride `0x3e`).
#[allow(clippy::too_many_arguments)]
pub fn field_menu_icon_sprites_for(
    rects: &SaveMenuAtlasRects,
    cursor: u8,
    party_ap: &[u16],
    list_pen: (i32, i32),
    money_pen: (i32, i32),
    info_pen: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    const LIST_PITCH: i32 = 14;
    const BLOCK_PITCH: i32 = 0x3e;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::new();
    {
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
        // Command-list hand cursor at (WX, row_y).
        push(
            rects.cursor,
            list_pen.0,
            list_pen.1 + cursor as i32 * LIST_PITCH,
        );
        // Money icon + play-time tag in the corner box.
        push(rects.icon_money, money_pen.0, money_pen.1 + 2);
        push(rects.label_time, money_pen.0, money_pen.1 + 0x10);
        // Party rows: LV / HP / MP label sprites per member.
        for i in 0..party_ap.len().min(3) {
            let y = info_pen.1 + i as i32 * BLOCK_PITCH;
            push(rects.label_lv, info_pen.0 + 0x70, y + 2);
            push(rects.label_hp, info_pen.0 + 0x28, y + 0x11);
            push(rects.label_mp, info_pen.0 + 0x28, y + 0x1e);
        }
    }
    for (i, &ap) in party_ap.iter().take(3).enumerate() {
        let y = info_pen.1 + i as i32 * BLOCK_PITCH;
        out.extend(ap_gauge_sprites(
            rects,
            (info_pen.0 + 0x28, y + 0x29),
            ap,
            stage_origin,
            stage_scale,
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

/// Retail body-text white (menu ink 7): every CLUT-7 staged glyph reads
/// back as RGB `(206, 206, 206)` in the golden `menu_status_town`
/// capture. Stored as that byte value straight - `206/255` - because
/// nothing on the path converts colour spaces: [`TextDraw`] colours
/// multiply the whitewashed font atlas and land in a UNORM attachment,
/// so the presented pixel is the value written (see the renderer's
/// `choose_surface_format`). Same ink as [`OPTIONS_INK_WHITE`].
///
/// [`OPTIONS_INK_WHITE`]: crate::OPTIONS_INK_WHITE
pub const MENU_TEXT_WHITE: [f32; 4] = [0.807_843_1, 0.807_843_1, 0.807_843_1, 1.0];
/// Retail teal ink for the parenthesised base/growth values on the
/// status page (the HP/MP `( base)` group and the stat grid's
/// `( growth)` group, parens included): RGB `(66, 222, 222)` in the
/// golden capture - the CLUT row the separator-staging value 5 selects.
/// Byte values, like [`MENU_TEXT_WHITE`].
pub const MENU_TEXT_TEAL: [f32; 4] = [0.258_823_5, 0.870_588_2, 0.870_588_2, 1.0];
/// Retail gold/yellow warning ink - staging id 6. The string-CLUT rows
/// live at VRAM row 510, cell `x = 16*(6+id)`; the main ink is palette
/// entry 15, and id 6 decodes to RGB `(231, 173, 0)` (read from the
/// golden `menu_status_town` VRAM).
pub const MENU_TEXT_GOLD: [f32; 4] = [0.905_882_4, 0.678_431_4, 0.0, 1.0];
/// Retail orange "critical" ink - staging id 9, RGB `(222, 90, 0)`
/// (same VRAM CLUT-row decode as [`MENU_TEXT_GOLD`]).
pub const MENU_TEXT_ORANGE: [f32; 4] = [0.870_588_2, 0.352_941_2, 0.0, 1.0];
/// Retail red "downed" ink - staging id 2, RGB `(231, 33, 0)`
/// (same VRAM CLUT-row decode as [`MENU_TEXT_GOLD`]).
pub const MENU_TEXT_RED: [f32; 4] = [0.905_882_4, 0.129_411_8, 0.0, 1.0];

/// Menu HP health-tier ink - the retail per-character colour fn
/// `FUN_800349EC` the status / party panels stage before drawing the
/// HP current+max number fields:
///
/// - `hp == 0` -> staging 2 (red, [`MENU_TEXT_RED`])
/// - `hp <= max/4` -> staging 9 (orange, [`MENU_TEXT_ORANGE`])
/// - `hp <= max/2` -> staging 6 (gold, [`MENU_TEXT_GOLD`])
/// - else -> staging 7 (white, [`MENU_TEXT_WHITE`])
///
/// (Retail also forces the gold tier at any HP when the char-record
/// `+0x12E` status halfword is non-zero - an ailment latch the engine
/// roster doesn't model yet.)
///
/// PORT: FUN_800349EC - menu HP ink tier (record `+0x104`/`+0x106`
/// thresholds at max/4 and max/2).
pub fn menu_hp_ink(hp: u16, hp_max: u16) -> [f32; 4] {
    if hp == 0 {
        MENU_TEXT_RED
    } else if hp <= hp_max / 4 {
        MENU_TEXT_ORANGE
    } else if hp <= hp_max / 2 {
        MENU_TEXT_GOLD
    } else {
        MENU_TEXT_WHITE
    }
}

/// Menu MP tier ink - retail `FUN_80035EA8`: `mp <= max/4` -> orange
/// (staging 9), `mp <= max/2` -> gold (staging 6), else white
/// (staging 7). Unlike HP there is no zero special-case.
///
/// PORT: FUN_80035EA8 - menu MP ink tier (record `+0x108`/`+0x10A`).
pub fn menu_mp_ink(mp: u16, mp_max: u16) -> [f32; 4] {
    if mp > mp_max / 4 {
        if mp <= mp_max / 2 {
            MENU_TEXT_GOLD
        } else {
            MENU_TEXT_WHITE
        }
    } else {
        MENU_TEXT_ORANGE
    }
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
        let l = font.layout_ascii(&ch.to_string());
        out.extend(text_draws_for(&l, (x + cell * NUM_CELL_W, y), color));
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
pub fn status_screen_draws_for(
    font: &legaia_font::Font,
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
            menu_hp_ink(panel.hp, panel.hp_max),
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

/// Build the status-page UI-icon [`SpriteDraw`]s from the system-UI atlas:
/// the LV / HP / MP labels, the AP gauge (four 1:1 pieces + meter fill +
/// value) and the 7-slot equipment pictogram grid. Everything is positioned
/// at the byte-pinned `FUN_801D33D8` offsets and mapped from the 320x240
/// menu stage into surface pixels (`stage_origin` + `stage_scale`, matching
/// [`crate::menu_window_chrome_draws_for`]). Pair with
/// `status_screen_draws_for(.., label_icons = true)` so the labels / AP
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
pub fn status_satellite_draws_for(
    font: &legaia_font::Font,
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
        "Condition",
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
    str_at(&mut out, "ATR:", summary_pen.0, summary_pen.1 + 0x1a, white);

    out
}

/// Build the status-screen satellite-window UI-icon [`SpriteDraw`]s from
/// the system-UI atlas: the party-list pointing-hand cursor, the
/// "Condition" pager triangles, the summary window's LV label and the
/// per-character ATR element icon. Positions are the traced renderer
/// offsets (see [`status_satellite_draws_for`]); the stage mapping
/// matches [`crate::menu_window_chrome_draws_for`]. Pair with
/// `status_satellite_draws_for(.., label_icons = true)`.
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

/// Compose the retail **tab-banner plaque** for a field-menu title tab
/// (the carved brown plaque behind "Status" / "Equip" / "Options") as
/// atlas [`SpriteDraw`]s.
///
/// Retail draws the class-2 tab window's entire chrome as six sprites
/// (RAM prim scan over the `menu_status_town` capture): a left cap at
/// `(WX-8, WY-4)`, the 16x20 body tile repeated across the tab's content
/// width `w` (with a partial remainder), and a right cap at `(WX+w,
/// WY-4)`. No gold 9-slice frame or filigree interior is drawn for tab
/// windows - the label text lands directly on the plaque at the content
/// origin. `pen` is the tab window's content origin and `content_w` its
/// descriptor width (60 in the retail table).
///
/// REF: FUN_801DCAD8 / FUN_801DCA94 / FUN_801DCB1C - the tab content
/// renderers (label string only; the plaque is caller-drawn chrome).
pub fn tab_banner_draws(
    rects: &SaveMenuAtlasRects,
    pen: (i32, i32),
    content_w: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let (wx, wy) = pen;
    let y = wy - 4;
    let scale = stage_scale.max(1) as i32;
    let mut out = Vec::with_capacity(6);
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
    push(rects.tab_cap_l, wx - 8, y);
    let (bx, by, bw, bh) = rects.tab_body;
    let mut x = wx;
    while x < wx + content_w {
        let this_w = (wx + content_w - x).min(bw as i32);
        push((bx, by, this_w as u32, bh), x, y);
        x += this_w;
    }
    push(rects.tab_cap_r, wx + content_w, y);
    out
}

#[cfg(test)]
mod health_tier_ink_tests {
    use super::*;

    /// The retail HP ink thresholds (`FUN_800349EC`): red at 0, orange
    /// at `<= max/4`, gold at `<= max/2`, white above. Boundaries are
    /// inclusive on the low side (the decompile tests `max>>2 < hp`).
    #[test]
    fn hp_tiers_match_the_retail_thresholds() {
        let max = 180;
        assert_eq!(menu_hp_ink(0, max), MENU_TEXT_RED);
        assert_eq!(menu_hp_ink(1, max), MENU_TEXT_ORANGE);
        assert_eq!(menu_hp_ink(45, max), MENU_TEXT_ORANGE); // == max/4
        assert_eq!(menu_hp_ink(46, max), MENU_TEXT_GOLD);
        assert_eq!(menu_hp_ink(90, max), MENU_TEXT_GOLD); // == max/2
        assert_eq!(menu_hp_ink(91, max), MENU_TEXT_WHITE);
        assert_eq!(menu_hp_ink(180, max), MENU_TEXT_WHITE);
    }

    /// MP tiers (`FUN_80035EA8`): same quarter/half thresholds but no
    /// zero special-case - 0 MP sits in the orange tier.
    #[test]
    fn mp_tiers_match_the_retail_thresholds() {
        let max = 20;
        assert_eq!(menu_mp_ink(0, max), MENU_TEXT_ORANGE);
        assert_eq!(menu_mp_ink(5, max), MENU_TEXT_ORANGE); // == max/4
        assert_eq!(menu_mp_ink(6, max), MENU_TEXT_GOLD);
        assert_eq!(menu_mp_ink(10, max), MENU_TEXT_GOLD); // == max/2
        assert_eq!(menu_mp_ink(11, max), MENU_TEXT_WHITE);
        assert_eq!(menu_mp_ink(20, max), MENU_TEXT_WHITE);
    }
}
