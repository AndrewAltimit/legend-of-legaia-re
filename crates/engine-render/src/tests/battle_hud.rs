use super::*;
use legaia_engine_ui::BattleChromeRects;
use legaia_engine_ui::BattleHudDraws;
use legaia_engine_ui::BattleHudFrame;
use legaia_engine_ui::HudLogView;
use legaia_engine_ui::HudPopupView;
use legaia_engine_ui::HudSlotView;
use legaia_engine_ui::SaveMenuAtlasRects;
use legaia_engine_ui::battle_hud_draws_for;

mod diagnostic_rows;
mod inks;
mod numeral_cells;
mod party_surfaces;
mod top_left_seat;

/// A recognisable 1x1 solid src for the filled-rect draws.
const SOLID: (u32, u32, u32, u32) = (5, 9, 1, 1);
/// 640x480 is an exact 2x of the 320x240 stage with a zero origin, so a stage
/// column `c` lands at surface `2 * c` and the measured retail columns read
/// straight off `dst.0`.
const SURFACE: (u32, u32) = (640, 480);
const STAGE_SCALE: i32 = 2;
const PEN: (i32, i32) = (8, 100);
/// The packet-pinned active-actor bar (`battle_chrome::BAR_*`).
const BAR_Y: i32 = 188;
const BAR_X: i32 = 8;
const BAR_H: i32 = 20;
const BAR_W: i32 = 304;
const BAR_NAME_Y: i32 = 192;
/// The packet-pinned roster panels (`battle_chrome::PANEL_*`).
const PANEL_Y: i32 = 164;
const PANEL_W: i32 = 102;
const PANEL_H: i32 = 48;
const SOLO_PANEL_X: i32 = 109;
const TRIO_PANEL_X: [i32; 3] = [7, 109, 211];

fn slot_view<'a>(
    name: &'a str,
    is_party: bool,
    alive: bool,
    hp: u16,
    hp_max: u16,
    mp: u16,
    mp_max: u16,
) -> HudSlotView<'a> {
    // Fill indices as engine-core's `BattleSlotHud::gauge_fill_indices`
    // derives them (FUN_80046A20 precedence, sans status arm).
    let band = |cur: u16, max: u16| -> u8 {
        if (max >> 1) < cur {
            7
        } else if (max >> 2) < cur {
            6
        } else {
            9
        }
    };
    let (hp_fill, mp_fill) = if hp == 0 {
        (2, 2)
    } else {
        (band(hp, hp_max), band(mp, mp_max))
    };
    HudSlotView {
        name,
        is_party,
        alive,
        hp,
        hp_max,
        mp,
        mp_max,
        ap_filled: 0,
        ap_max: 0,
        hp_fill,
        mp_fill,
        status_sprite: 0,
        level: 0,
    }
}

fn hud_frame(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    popups: &[HudPopupView],
    log: &[HudLogView<'_>],
    diag: bool,
) -> BattleHudDraws {
    battle_hud_draws_for(
        font,
        &BattleHudFrame {
            slots,
            popups,
            log,
            solid_src: Some(SOLID),
            surface: SURFACE,
            diag,
            ..Default::default()
        },
        PEN,
    )
}

fn hud_draws(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    popups: &[HudPopupView],
    log: &[HudLogView<'_>],
) -> Vec<TextDraw> {
    hud_frame(font, slots, popups, log, false).text
}

/// The same frame with slot 0 acting, which is what raises the active-actor
/// bar over that member's resting panel.
fn hud_draws_acting(font: &legaia_font::Font, slots: &[HudSlotView<'_>]) -> Vec<TextDraw> {
    battle_hud_draws_for(
        font,
        &BattleHudFrame {
            slots,
            solid_src: Some(SOLID),
            surface: SURFACE,
            active_slot: Some(0),
            ..Default::default()
        },
        PEN,
    )
    .text
}

/// Retail's numeral-cell pitch - the unit every HP / MP / level field on the
/// battle screen is measured in (`battle_chrome::DIGIT_W`).
const DIGIT_W: i32 = 8;
const DIGIT_H: i32 = 12;
/// The battle cells' seats in the baked atlas
/// (`save_menu_atlas::ATLAS_RECT_BATTLE_*` / `_HUD_DIGITS`).
const DIGIT_STRIP: (u32, u32, u32, u32) = (0, 244, 80, DIGIT_H as u32);
const BATTLE_PANEL_BG: (u32, u32, u32, u32) = (0, 0, 102, 48);
const PLATE_CAP_L: (u32, u32, u32, u32) = (208, 0, 8, 20);
const PLATE_BODY: (u32, u32, u32, u32) = (192, 0, 16, 20);
const PLATE_CAP_R: (u32, u32, u32, u32) = (216, 0, 8, 20);
const SEPARATOR_SRC: (u32, u32, u32, u32) = (96, 64, 8, 16);
/// The actor-name plaque's fixed plate seat (`battle_chrome::PLAQUE_*`).
const PLAQUE_X: i32 = 8;
const PLAQUE_Y: i32 = 8;

/// The atlas rects a resident system-UI + menu-glyph bake produces, at the
/// seats `save_menu_atlas` bakes them to.
fn chrome_rects() -> SaveMenuAtlasRects {
    SaveMenuAtlasRects {
        label_hp: (208, 86, 16, 10),
        label_mp: (224, 86, 16, 10),
        label_lv: (192, 86, 16, 10),
        dialog_fill: (128, 0, 32, 29),
        panel_interior: (128, 0, 32, 29),
        panel_tl: (160, 0, 4, 4),
        panel_tr: (188, 0, 4, 4),
        panel_bl: (160, 28, 4, 4),
        panel_br: (188, 28, 4, 4),
        panel_top: (164, 0, 24, 4),
        panel_bot: (164, 28, 24, 4),
        panel_left: (160, 4, 4, 24),
        panel_right: (188, 4, 4, 24),
        tab_cap_l: (144, 232, 8, 20),
        tab_body: (154, 232, 16, 20),
        tab_cap_r: (172, 232, 8, 20),
        battle: Some(BattleChromeRects {
            panel_bg: BATTLE_PANEL_BG,
            plate_cap_l: PLATE_CAP_L,
            plate_body: PLATE_BODY,
            plate_cap_r: PLATE_CAP_R,
            separator: SEPARATOR_SRC,
            digits: Some(DIGIT_STRIP),
            cross_out: None,
            rot_stamp: None,
            curse_plate: None,
        }),
        ..Default::default()
    }
}

/// A frame drawn with the resident chrome atlas, so every piece of the
/// surface comes out as a disc-sourced sprite rather than a font stand-in.
fn hud_frame_with_chrome(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    active_slot: Option<u8>,
) -> BattleHudDraws {
    let rects = chrome_rects();
    battle_hud_draws_for(
        font,
        &BattleHudFrame {
            slots,
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            active_slot,
            ..Default::default()
        },
        PEN,
    )
}

/// Every numeral the frame drew, as `(stage x, stage y, digit)`. Reads the
/// digit back out of the sprite's `src`, so the test sees the number the HUD
/// actually put on screen and not just where it put something.
fn numeral_cells(draws: &BattleHudDraws) -> Vec<(i32, i32, u32)> {
    let mut out: Vec<(i32, i32, u32)> = draws
        .sprites
        .iter()
        .filter(|s| s.src.1 == DIGIT_STRIP.1 && s.src.2 == DIGIT_W as u32)
        .map(|s| {
            (
                s.dst.0 / STAGE_SCALE,
                s.dst.1 / STAGE_SCALE,
                (s.src.0 - DIGIT_STRIP.0) / DIGIT_W as u32,
            )
        })
        .collect();
    out.sort_by_key(|(x, y, _)| (*y, *x));
    out
}

/// The numerals on one row, as the number they spell plus its left edge.
fn numerals_on_row(cells: &[(i32, i32, u32)], y: i32) -> Vec<(i32, String)> {
    let mut runs: Vec<(i32, String)> = Vec::new();
    for &(x, _, d) in cells.iter().filter(|(_, cy, _)| *cy == y) {
        match runs.last_mut() {
            Some((start, s)) if *start + (s.len() as i32) * DIGIT_W == x => {
                s.push(char::from_digit(d, 10).unwrap())
            }
            _ => runs.push((x, char::from_digit(d, 10).unwrap().to_string())),
        }
    }
    runs
}

/// Solid rects of exactly `(w, h)` stage pixels, by stage `(x, y)`.
fn boxes_of(draws: &[TextDraw], w: i32, h: i32) -> Vec<(i32, i32)> {
    draws
        .iter()
        .filter(|d| {
            d.src == SOLID
                && d.dst.2 == (w * STAGE_SCALE) as u32
                && d.dst.3 == (h * STAGE_SCALE) as u32
        })
        .map(|d| (d.dst.0 / STAGE_SCALE, d.dst.1 / STAGE_SCALE))
        .collect()
}
