//! The field pause menu: command rows, the party info panel and its icon sprites.

use crate::*;

/// Plain-data row for the field-menu draw. Engines build these from
/// `legaia_engine_menus::field_menu::FieldMenuView::rows` so this crate doesn't
/// depend on engine-core.
pub struct FieldMenuRowView<'a> {
    pub label: &'a str,
    pub enabled: bool,
}

/// Build [`TextDraw`]s for the field (pause) menu command list. `cursor` is
/// the row index; greyed-out rows render dim.
///
/// Layout follows the retail top-level pause menu's window-descriptor
/// geometry (menu overlay window table, `legaia_game_tables::menu_windows`):
/// the command rows fill the id-50 list window (`list_pen` = its content
/// origin, 7 rows at the retail 13-px list pitch), and the money +
/// play-time lines fill the id-49 corner box (`money_pen`). The row
/// *content* layout (cursor glyph, label inset) is engine-styled - the
/// retail list renderer `FUN_801CFD68` is untraced.
#[allow(clippy::too_many_arguments)]
pub fn field_menu_draws_for(
    font: &legaia_font::Font,
    rows: &[FieldMenuRowView<'_>],
    cursor: u8,
    money: u32,
    play_time_seconds: u32,
    coins: Option<u32>,
    list_pen: (i32, i32),
    money_pen: (i32, i32),
) -> Vec<TextDraw> {
    /// Retail command-list row pitch (`FUN_801CFD68` steps each row's Y
    /// by `0x0e`).
    const LIST_PITCH: i32 = 14;
    let white: [f32; 4] = MENU_TEXT_WHITE;
    // Disabled rows stage string CLUT 0 (the grayed row ink retail uses
    // for a blocked Save / Load) - 5-bit `(16, 16, 16)` on a retail frame.
    let dim: [f32; 4] = MENU_TEXT_GREY;

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
    // The casino coin row (`FUN_8003CE64(8)`, raised at menu open when the
    // bank is non-zero): the bank `0x800845A4` as a second 8-digit field one
    // row down (`0x801D01C0..0x801D01E8`), pushing the time row with it.
    let mut time_y = money_pen.1 + 0x0e;
    if let Some(coins) = coins {
        out.extend(num_field_draws(
            font,
            coins as u64,
            money_pen.0 + 0x28,
            time_y,
            8,
            white,
        ));
        time_y += 0x0e;
    }
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
    // The colons are separator glyph 9 (`FUN_8003C1F8(9, ..)` at
    // `0x801D029C` / `0x801D02C0`).
    let colon = |out: &mut Vec<TextDraw>, x: i32| {
        out.extend(crate::menu::window_painters::separator_glyph_draws(
            font,
            crate::menu::window_painters::SEPARATOR_GLYPH_COLON,
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
        out.extend(crate::numeral_cell_draws(font, ch, (cell_x, y), color));
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
    /// The member's packed battle-status halfword (char record `+0x12E`,
    /// the mirror of battle actor `+0x16E`). Non-zero re-inks the HP
    /// number gold - see [`menu_hp_ink_with_status`], whose ailment arm
    /// this field is the input of.
    pub status: u16,
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
                menu_hp_ink_with_status(m.hp, m.hp_max, m.status as i16),
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
    coin_row: bool,
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
        let mut time_y = money_pen.1 + 0x10;
        if coin_row {
            // ICO `0x66` one row down (`0x801D01CC`).
            push(rects.label_coin, money_pen.0, time_y);
            time_y += 0x0e;
        }
        push(rects.label_time, money_pen.0, time_y);
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
