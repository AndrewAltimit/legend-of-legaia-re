use super::*;

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

/// At rest the party draws as **roster panels**, not a bar: one 102x48 panel
/// per live member at the packet-pinned seat, name pen `+5` inside it.
#[test]
fn resting_party_draws_roster_panels_at_the_pinned_seats() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    assert_eq!(
        boxes_of(&draws, PANEL_W, PANEL_H),
        vec![(SOLO_PANEL_X, PANEL_Y)],
        "the solo roster panel is not at its pinned seat"
    );
    // `panel_anchors`' solo entry is the panel's *name pen*, +5 inside it.
    assert!(
        draws.iter().any(|d| d.src != SOLID
            && d.dst.0 == (SOLO_PANEL_X + 5) * STAGE_SCALE
            && d.dst.1 == (PANEL_Y + 4) * STAGE_SCALE),
        "no name glyph at the panel's pinned name pen"
    );
    // And no active-actor bar, because nobody is acting.
    assert!(
        boxes_of(&draws, BAR_W, BAR_H).is_empty(),
        "the active-actor bar drew with no acting actor"
    );
}

/// The acting member's readout moves to the full-width bar at the pinned
/// seats: plate `(8, 188)` 304x20, name pen `(16, 192)`.
#[test]
fn acting_member_raises_the_active_actor_bar() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = hud_draws_acting(&font, &[slot]);
    assert_eq!(
        boxes_of(&draws, BAR_W, BAR_H),
        vec![(BAR_X, BAR_Y)],
        "the active-actor bar is not at its pinned seat"
    );
    assert!(
        draws.iter().any(|d| d.src != SOLID
            && d.dst.0 == 16 * STAGE_SCALE
            && d.dst.1 == BAR_NAME_Y * STAGE_SCALE),
        "no name glyph at the bar's pinned (16, 192) pen"
    );
}

/// The two party surfaces are **mutually exclusive**. While the acting
/// member's bar owns the screen retail parks the whole roster cluster at
/// `y = 230`, under its 228-line display window; the port's stage is 240
/// lines so `y = 230` would still show, and it omits the panel draws
/// instead. Either way a frame carries one surface, never both stacked.
#[test]
fn the_bar_and_the_roster_panels_never_share_a_frame() {
    let font = legaia_font::synthetic_for_tests();
    let slots = [
        slot_view("Vahn", true, true, 100, 100, 10, 20),
        slot_view("Noa", true, true, 90, 100, 8, 20),
        slot_view("Gala", true, true, 80, 100, 6, 20),
    ];
    let acting = hud_draws_acting(&font, &slots);
    assert_eq!(
        boxes_of(&acting, BAR_W, BAR_H),
        vec![(BAR_X, BAR_Y)],
        "the acting member's bar is missing"
    );
    assert!(
        boxes_of(&acting, PANEL_W, PANEL_H).is_empty(),
        "the roster panels drew under the active-actor bar"
    );
    // And with the atlas resident, so does the sprite half.
    let sprites = hud_frame_with_chrome(&font, &slots, Some(0));
    assert!(
        !sprites.sprites.iter().any(|s| s.src == BATTLE_PANEL_BG),
        "the roster plate sprite drew under the active-actor bar"
    );
    // Non-vacuous: the same party at rest really does draw all three.
    assert_eq!(
        boxes_of(&hud_draws(&font, &slots, &[], &[]), PANEL_W, PANEL_H).len(),
        3
    );
}

/// Retail's party strip carries **no gauge bar of any kind** - the native
/// 320x228 capture holds only glyphs and the two label sprites between the
/// lozenge caps. This is the assertion the old invented green HP bar fails.
#[test]
fn party_strip_draws_no_gauge_bar() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 150, 300, 12, 30);
    for draws in [
        hud_draws(&font, &[slot], &[], &[]),
        hud_draws_acting(&font, &[slot]),
    ] {
        for d in draws.iter().filter(|d| d.src == SOLID) {
            // Every solid rect on the retail surface is a plate: a panel
            // body, a bar body, a plaque body, or one of the four 1-px rims
            // the chrome-less fallback draws around them. Anything else with
            // interior extents is a gauge bar.
            let w = d.dst.2 as i32 / STAGE_SCALE;
            let h = d.dst.3 as i32 / STAGE_SCALE;
            let rim = w == 1 || h == 1;
            let plate_body = (w, h) == (PANEL_W, PANEL_H) || (w, h) == (BAR_W, BAR_H) || h == 20;
            assert!(
                rim || plate_body,
                "a gauge-bar-shaped rect survives on the retail surface: {:?}",
                d.dst
            );
        }
        // Non-vacuous: the surface really did draw.
        assert!(draws.iter().any(|d| d.src == SOLID));
    }
}

/// A trio seats three roster panels side by side at the pinned x anchors, all
/// on the one panel row.
#[test]
fn trio_seats_three_roster_panels_across_the_pinned_anchors() {
    let font = legaia_font::synthetic_for_tests();
    let slots = [
        slot_view("Vahn", true, true, 100, 100, 10, 20),
        slot_view("Noa", true, true, 90, 100, 8, 20),
        slot_view("Gala", true, true, 80, 100, 6, 20),
    ];
    let draws = hud_draws(&font, &slots, &[], &[]);
    assert_eq!(
        boxes_of(&draws, PANEL_W, PANEL_H),
        TRIO_PANEL_X.map(|x| (x, PANEL_Y)).to_vec(),
        "the trio panels are not at their pinned seats"
    );
}

#[test]
fn battle_hud_draws_for_skips_empty_slot_name() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("", true, true, 0, 0, 0, 0);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    assert!(draws.is_empty());
}

/// A K.O.'d member keeps its row and dims - retail's strip has no "K.O."
/// legend, and the readout law's own death index is what greys the numerals.
#[test]
fn dead_member_dims_instead_of_drawing_a_ko_legend() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, false, 0, 300, 0, 30);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    let dim = [0.5f32, 0.5, 0.5, 1.0];
    assert!(
        draws.iter().any(|d| d.src != SOLID && d.color == dim),
        "dead member's glyphs did not take the dim tint"
    );
    let red = [1.0f32, 0.4, 0.4, 1.0];
    assert!(
        !draws.iter().any(|d| d.src != SOLID && d.color == red),
        "a K.O. legend still draws on the retail strip"
    );
}

#[test]
fn battle_hud_draws_for_low_hp_uses_red_color() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 10, 100, 0, 0);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // Numerals take the danger tint (red-dominant).
    let any_red = draws
        .iter()
        .any(|d| d.src != SOLID && d.color[0] > d.color[1]);
    assert!(any_red, "low HP should produce a red-tinted glyph");
}

#[test]
fn battle_hud_without_solid_src_degrades_to_text_only() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            popups: &[],
            log: &[],
            solid_src: None,
            surface: SURFACE,
            ..Default::default()
        },
        PEN,
    );
    assert!(
        !draws.text.is_empty(),
        "text-only fallback still draws numerals"
    );
    assert!(
        !draws.text.iter().any(|d| d.src == SOLID),
        "no rect may be emitted without a solid src"
    );
}

/// With the resident system-UI atlas every piece of the battle surface is a
/// **sprite off the disc's own cells** - the roster plate, the `HP` / `MP`
/// label cells, the `/` separator and the numerals - not a stand-in drawn
/// out of the font atlas. Without an atlas the same information degrades
/// into the text list, which is what keeps a chrome-less host readable.
#[test]
fn chrome_atlas_moves_the_strip_skin_and_labels_into_sprites() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = hud_frame_with_chrome(&font, &[slot], None);
    assert!(
        draws.sprites.iter().any(|s| s.src == (208, 86, 16, 10)
            && s.dst.0 == (SOLO_PANEL_X + 4) * STAGE_SCALE
            && s.dst.1 == (PANEL_Y + 21) * STAGE_SCALE),
        "the HP label cell is not at the panel's pinned HP-row seat"
    );
    assert!(
        draws.sprites.iter().any(|s| s.src == (224, 86, 16, 10)),
        "the green MP label cell never drew"
    );
    // The roster plate: retail's 102x48 marbled sprite, whole, at the seat.
    assert!(
        draws.sprites.iter().any(|s| s.src == BATTLE_PANEL_BG
            && s.dst.0 == SOLO_PANEL_X * STAGE_SCALE
            && s.dst.1 == PANEL_Y * STAGE_SCALE
            && s.dst.2 == (PANEL_W * STAGE_SCALE) as u32
            && s.dst.3 == (PANEL_H * STAGE_SCALE) as u32),
        "the roster panel did not draw the disc's own 102x48 plate"
    );
    // The `/` is a sheet sprite four rows above its numerals, not a glyph.
    assert_eq!(
        draws
            .sprites
            .iter()
            .filter(|s| s.src == SEPARATOR_SRC)
            .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE))
            .collect::<Vec<_>>(),
        vec![
            (SOLO_PANEL_X + 57, PANEL_Y + 15),
            (SOLO_PANEL_X + 57, PANEL_Y + 30)
        ],
        "the `/` separators are not the pinned sheet sprite at their seats"
    );
    // With chrome nothing rides the solid-texel fallback.
    assert!(
        !draws.text.iter().any(|d| d.src == SOLID),
        "the solid-texel fallback body drew alongside the atlas chrome"
    );
}

/// The active-actor bar is retail's plate **3-slice**, not a 9-slice frame:
/// an 8-px cap, 16-px body tiles with the last one clipped to the
/// remainder, and a closing cap. A 288-px interior is exactly eighteen full
/// tiles, so the run is twenty sprites spanning `8..=312`.
#[test]
fn active_actor_bar_draws_the_retail_plate_three_slice() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = hud_frame_with_chrome(&font, &[slot], Some(0));
    let run: Vec<(i32, (u32, u32, u32, u32))> = draws
        .sprites
        .iter()
        .filter(|s| {
            [PLATE_CAP_L.0, PLATE_BODY.0, PLATE_CAP_R.0].contains(&s.src.0)
                && s.src.1 == 0
                && s.dst.1 / STAGE_SCALE == BAR_Y
        })
        .map(|s| (s.dst.0 / STAGE_SCALE, s.src))
        .collect();
    assert_eq!(run.len(), 20, "the bar is not a 20-sprite plate run");
    assert_eq!(run[0], (BAR_X, PLATE_CAP_L));
    assert!(run[1..19].iter().all(|(_, src)| *src == PLATE_BODY));
    assert_eq!(run[19], (BAR_X + BAR_W - 8, PLATE_CAP_R));
}

/// The plaque takes the **carved-gold** plate row rather than the blue one -
/// the same three tiles the field menu's tab banner samples, which is why
/// the two look alike.
#[test]
fn the_plaque_draws_the_gold_plate_row() {
    let font = legaia_font::synthetic_for_tests();
    let rects = chrome_rects();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            plaque: Some("Gimard"),
            ..Default::default()
        },
        PEN,
    );
    let gold: Vec<i32> = draws
        .sprites
        .iter()
        .filter(|s| s.src.1 == 232 && s.dst.1 / STAGE_SCALE == PLAQUE_Y)
        .map(|s| s.dst.0 / STAGE_SCALE)
        .collect();
    assert!(
        gold.first() == Some(&PLAQUE_X),
        "the plaque did not draw from the gold plate row: {gold:?}"
    );
    assert!(
        !draws
            .sprites
            .iter()
            .any(|s| s.src == PLATE_CAP_L && s.dst.1 / STAGE_SCALE == PLAQUE_Y),
        "the plaque drew the blue plate row"
    );
}

/// Retail parks the roster-panel cluster off-screen (`y = 230`) while a
/// command-entry session owns the frame; the port emits nothing instead,
/// because the engine stage is 240 lines and `y = 230` would still show.
#[test]
fn parked_input_session_suppresses_the_roster_panels() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            popups: &[],
            log: &[],
            solid_src: Some(SOLID),
            surface: SURFACE,
            input_session_parked: true,
            ..Default::default()
        },
        PEN,
    );
    assert!(
        boxes_of(&draws.text, PANEL_W, PANEL_H).is_empty(),
        "the roster panels still drew under a parked input session"
    );
}

/// The top-left plaque names the acting actor at the pinned `(16, 12)`
/// content seat, on a plate whose interior is the measured name.
#[test]
fn plaque_draws_at_the_measured_inset() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            popups: &[],
            log: &[],
            solid_src: Some(SOLID),
            surface: SURFACE,
            plaque: Some("Tetsu"),
            ..Default::default()
        },
        PEN,
    );
    assert!(
        draws
            .text
            .iter()
            .any(|d| d.src != SOLID && d.dst.0 == 16 * STAGE_SCALE && d.dst.1 == 12 * STAGE_SCALE),
        "no plaque glyph at the pinned (16, 12) content seat"
    );
    // Plate at (8, 8), 20 tall, interior sized to the measured name.
    let name_w = font.layout_ascii("Tetsu").advance_x as i32;
    assert_eq!(
        boxes_of(&draws.text, name_w + 16, 20),
        vec![(8, 8)],
        "the plaque plate is not sized to its name at the pinned seat"
    );
}

#[test]
fn hp_bar_color_index_tiers_match_retail() {
    // K.O. -> 2 regardless of max.
    assert_eq!(hp_bar_color_index(0, 100, false), 2);
    // cur <= max/4 -> 9 (danger). max>>2 = 25, so 25 is still danger.
    assert_eq!(hp_bar_color_index(25, 100, false), 9);
    assert_eq!(hp_bar_color_index(1, 100, false), 9);
    // max/4 < cur <= max/2 -> 6 (caution). 26..=50.
    assert_eq!(hp_bar_color_index(26, 100, false), 6);
    assert_eq!(hp_bar_color_index(50, 100, false), 6);
    // cur > max/2 -> 7 (normal).
    assert_eq!(hp_bar_color_index(51, 100, false), 7);
    assert_eq!(hp_bar_color_index(100, 100, false), 7);
    // The status flag forces caution (6) even at full HP.
    assert_eq!(hp_bar_color_index(100, 100, true), 6);
    // ...but never overrides K.O. or danger.
    assert_eq!(hp_bar_color_index(0, 100, true), 2);
    assert_eq!(hp_bar_color_index(10, 100, true), 9);
}

#[test]
fn mp_bar_color_index_tiers_match_retail() {
    // No K.O. tier: empty MP reads as danger (9), not 2.
    assert_eq!(mp_bar_color_index(0, 40), 9);
    assert_eq!(mp_bar_color_index(10, 40), 9); // cur <= max/4
    assert_eq!(mp_bar_color_index(11, 40), 6); // max/4 < cur <= max/2
    assert_eq!(mp_bar_color_index(20, 40), 6);
    assert_eq!(mp_bar_color_index(21, 40), 7); // cur > max/2
    assert_eq!(mp_bar_color_index(40, 40), 7);
}

#[test]
fn gauge_fill_color_distinguishes_every_retail_index() {
    let idxs = [2u8, 3, 6, 7, 9];
    for (i, a) in idxs.iter().enumerate() {
        for b in idxs.iter().skip(i + 1) {
            assert_ne!(
                gauge_fill_color(*a),
                gauge_fill_color(*b),
                "indices {a} and {b} map to one colour"
            );
        }
    }
}

#[test]
fn battle_hud_caution_mp_uses_yellow_not_row_color() {
    let font = legaia_font::synthetic_for_tests();
    // 15 MP of 40 is in (10, 20] -> caution -> yellow numerals.
    let slot = slot_view("Noa", true, true, 100, 100, 15, 40);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // Yellow = [1.0, 0.95, 0.4]: high R+G, low B. Row color (white) has B==1.
    let any_yellow = draws
        .iter()
        .any(|d| d.src != SOLID && d.color[1] > 0.9 && d.color[2] < 0.5);
    assert!(
        any_yellow,
        "caution MP should produce a yellow-tinted glyph"
    );
}

#[test]
fn battle_hud_draws_for_includes_log_lines_below_slots() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let log = [HudLogView {
        text: "Vahn attacks.",
        color: [1.0, 1.0, 1.0, 1.0],
    }];
    let n_no_log = hud_draws(&font, &[slot], &[], &[]).len();
    let draws_with_log = hud_draws(&font, &[slot], &[], &log);
    assert!(draws_with_log.len() > n_no_log);
}

/// The HUD's text popup is the **diagnostic** readout of a hit, not the retail
/// one: retail throws its numeral off the effect atlas
/// ([`legaia_engine_vm::battle_value_readout`]) over the actor that was struck,
/// so nothing about a landed hit reaches the default surface here. The anchor
/// this asserts is therefore the diagnostic column's, and the retail seat is
/// covered by the readout module's own tests.
#[test]
fn battle_hud_popup_is_a_diagnostic_row_riding_its_panel_anchor() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 0,
        amount: 250,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    assert_eq!(
        hud_draws(&font, &[slot], &[popup], &[]).len(),
        hud_draws(&font, &[slot], &[], &[]).len(),
        "a popup drew on the default retail surface"
    );

    let n_no_popup = hud_frame(&font, &[slot], &[], &[], true).text.len();
    let draws = hud_frame(&font, &[slot], &[popup], &[], true).text;
    assert!(draws.len() > n_no_popup, "popup produced no glyphs");
    // Damage popups draw in the cyan tint, above the member's panel (a
    // stage-space anchor, unlike the diagnostic rows' pen anchor).
    let cyan = [0.5, 0.85, 1.0, 1.0];
    assert!(
        draws.iter().any(|d| d.src != SOLID && d.color == cyan),
        "no cyan popup glyph"
    );
}

/// Monster rows are the **diagnostic** surface now: retail's HUD draws no
/// monster gauge at all, so nothing about a monster reaches the default list.
#[test]
fn monster_rows_only_draw_under_the_diagnostic_toggle() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Gimard", false, true, 100, 100, 0, 0);
    // Sprite 0x19 = Toxic, the element retail's ladder picks for `0x0002`.
    slot.status_sprite = 0x19;
    assert!(
        hud_draws(&font, &[slot], &[], &[]).is_empty(),
        "a monster row drew on the default retail surface"
    );

    let diag = hud_frame(&font, &[slot], &[], &[], true).text;
    // The element renders past the column origin at pen.x + 190.
    assert!(
        diag.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count() > 0,
        "expected a status element in the diagnostic monster column"
    );

    // And it is exactly ONE element regardless of how many ailments the slot
    // carries - the whole point of the retail ladder.
    let clean = slot_view("Gimard", false, true, 100, 100, 0, 0);
    let none = hud_frame(&font, &[clean], &[], &[], true).text;
    assert_eq!(
        none.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count(),
        0,
        "an unafflicted monster row draws no status element"
    );
}

/// `FUN_8002C2E4`'s no-ailment arm is the level, and retail draws it as a
/// **panel row** - LV label plus digits on the panel's top cell - not as the
/// floating marker the port used to put over the party.
///
/// The digits are right-aligned to `+96` like every other number on the
/// screen, so a one-digit level occupies `+88` and a two-digit one `+80` and
/// `+88`. Both cases are packet-pinned: a solo capture draws level `1` at
/// panel `+88`, a three-party frame draws level `99` at `+80` / `+88`.
#[test]
fn level_draws_on_the_panels_top_cell() {
    let font = legaia_font::synthetic_for_tests();
    let mut lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    lv.level = 27;
    let mut no_lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    no_lv.level = 0;

    let with = hud_draws(&font, &[lv], &[], &[]);
    assert!(
        with.len() > hud_draws(&font, &[no_lv], &[], &[]).len(),
        "the level readout never drew"
    );
    let cells = numeral_cells(&hud_frame_with_chrome(&font, &[lv], None));
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 4),
        vec![(SOLO_PANEL_X + 80, "27".into())],
        "a two-digit level is not right-aligned to the panel's +96"
    );
    let mut single = lv;
    single.level = 1;
    assert_eq!(
        numerals_on_row(
            &numeral_cells(&hud_frame_with_chrome(&font, &[single], None)),
            PANEL_Y + 4
        ),
        vec![(SOLO_PANEL_X + 88, "1".into())],
        "a one-digit level is not at the solo capture's +88"
    );
}

/// An ailment **replaces** the level on the member's panel - `FUN_8002C2E4`
/// draws exactly one element per slot. The selection is the ported part; the
/// tag's art is an approximation of an unresolved sprite sheet.
#[test]
fn ailment_replaces_the_level_on_the_members_panel() {
    let font = legaia_font::synthetic_for_tests();
    let mut clean = slot_view("Vahn", true, true, 100, 100, 30, 30);
    clean.level = 27;
    let mut sick = slot_view("Vahn", true, true, 100, 100, 30, 30);
    sick.level = 27;
    sick.status_sprite = 0x1F;
    let with = hud_draws(&font, &[sick], &[], &[]);
    // `FUN_8002C2E4`'s ladder is exclusive: the ailment REPLACES the level
    // element rather than joining it, so the tag lands on the panel's LV seat
    // and the LV label sprite stops drawing.
    assert!(
        with.iter().any(|d| d.src != SOLID
            && d.dst.0 == (SOLO_PANEL_X + 64) * STAGE_SCALE
            && d.dst.1 == (PANEL_Y + 6) * STAGE_SCALE),
        "the ailment tag is not on the panel's element seat"
    );
    let lv_row = |slot: HudSlotView<'_>| -> Vec<(i32, String)> {
        numerals_on_row(
            &numeral_cells(&hud_frame_with_chrome(&font, &[slot], None)),
            PANEL_Y + 4,
        )
    };
    assert_eq!(
        lv_row(clean),
        vec![(SOLO_PANEL_X + 80, "27".into())],
        "the unafflicted slot lost its level readout"
    );
    assert!(
        lv_row(sick).is_empty(),
        "the level drew alongside an ailment - the retail ladder is exclusive"
    );
}

#[test]
fn battle_hud_draws_for_popup_for_invalid_slot_is_dropped() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 99,
        amount: 50,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    // Measured on the diagnostic frame: popups no longer reach the default
    // surface at all, so comparing there would pass for the wrong reason - the
    // out-of-range guard would be untested rather than proven.
    let with_popup = hud_frame(&font, &[slot], &[popup], &[], true).text;
    let no_popup = hud_frame(&font, &[slot], &[], &[], true).text;
    assert_eq!(with_popup.len(), no_popup.len());
    // Non-vacuity: an in-range popup on the same frame does add glyphs.
    let valid = HudPopupView { slot: 0, ..popup };
    assert!(hud_frame(&font, &[slot], &[valid], &[], true).text.len() > no_popup.len());
}

#[test]
fn battle_hud_draws_for_heal_popup_uses_green_tint() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 100, 100, 0, 0);
    let popup = HudPopupView {
        slot: 0,
        amount: 60,
        is_heal: true,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    let draws = hud_frame(&font, &[slot], &[popup], &[], true).text;
    // Heal color is green: [0.5, 1.0, 0.5, 1.0]; any glyph with that profile.
    let any_green = draws
        .iter()
        .any(|d| d.src != SOLID && d.color[1] >= 0.95 && d.color[0] < d.color[1]);
    assert!(any_green);
}

#[test]
fn apply_alpha_scales_only_alpha_channel() {
    let c = [0.5, 0.6, 0.7, 1.0];
    let scaled = apply_alpha(c, 0.5);
    assert_eq!(scaled, [0.5, 0.6, 0.7, 0.5]);
}

#[test]
fn font_solid_src_finds_a_white_texel_in_the_placeholder_font() {
    let font = legaia_font::Font::placeholder();
    let src = font_solid_src(&font).expect("placeholder font has white strokes");
    assert_eq!((src.2, src.3), (1, 1), "solid src must be a 1x1 rect");
    let (w, _) = font.atlas_dimensions();
    let off = ((src.1 * w + src.0) * 4) as usize;
    let rgba = font.atlas_rgba();
    assert_eq!(&rgba[off..off + 4], &[255, 255, 255, 255]);
}

/// Retail's numeral fields, measured in retail's numeral cells.
///
/// Every number on the battle screen is a run of fixed 8-px cells off the
/// menu-glyph strip, right-aligned against a pinned edge - **both** halves of
/// a `cur / max` pair, not just the current. Proportional dialog-font digits
/// are not what retail draws and are wider than the field allows: a
/// four-digit maximum seated as a forward-running font string runs past the
/// roster panel's 102-px plate and into its neighbour.
///
/// Font-independent by construction, so it runs on CI too. Reads the seats
/// out of the packet-pinned `engine-vm::battle_chrome` rather than out of
/// literals, so a re-seated pin fails here rather than passing against a
/// stale copy. The companion
/// [`bar_panel_and_diag_columns_clear_the_retail_font_or_skips`] covers the
/// fields that really are proportional - the names and the diagnostic rows.
#[test]
fn numeral_fields_fit_their_widest_value_in_retail_cells() {
    use legaia_engine_vm::battle_chrome as bc;
    let left_of = |right: i16, digits: u16| bc::digits_left_of(right, digits);
    // The label cells are 16 wide, the `/` separator 8.
    let label_w = 16;
    let sep_w = bc::SEPARATOR.2 as i16;

    // Active-actor bar. Four cells per HP field, three per MP field - the
    // width retail actually gave each one.
    assert!(
        left_of(bc::BAR_HP_CUR_RIGHT, 4) >= bc::BAR_HP_LABEL.0 + label_w,
        "a four-digit HP current overruns the bar's HP label cell"
    );
    assert!(
        left_of(bc::BAR_HP_MAX_RIGHT, 4) >= bc::BAR_HP_SEPARATOR.0 + sep_w,
        "a four-digit HP maximum overruns the bar's `/` separator"
    );
    assert!(
        bc::BAR_HP_MAX_RIGHT <= bc::BAR_MP_LABEL.0,
        "the bar's HP maximum overruns the MP label seat"
    );
    assert!(
        left_of(bc::BAR_MP_CUR_RIGHT, 3) >= bc::BAR_MP_LABEL.0 + label_w,
        "a three-digit MP current overruns the bar's MP label cell"
    );
    assert!(
        left_of(bc::BAR_MP_MAX_RIGHT, 3) >= bc::BAR_MP_SEPARATOR.0 + sep_w,
        "a three-digit MP maximum overruns the bar's `/` separator"
    );
    assert!(
        bc::BAR_MP_MAX_RIGHT <= bc::BAR_X + bc::PLATE_CAP_W as i16 + bc::BAR_INTERIOR_W as i16,
        "the bar's MP maximum overruns the right cap"
    );

    // Roster panel: the same two pairs inside a 102-px plate whose pitch is
    // also 102, so an overrun lands on the next member's panel.
    let plate_w = bc::PANEL_BG.2 as i16;
    assert!(
        left_of(bc::panel::CUR_RIGHT, 4) >= bc::panel::HP_LABEL.0 + label_w,
        "a four-digit HP current overruns the panel's HP label cell"
    );
    assert!(
        left_of(bc::panel::MAX_RIGHT, 4) >= bc::panel::HP_SEPARATOR.0 + sep_w,
        "a four-digit maximum overruns the panel's `/` separator"
    );
    assert!(
        bc::panel::MAX_RIGHT <= plate_w,
        "a four-digit maximum overruns the panel's right edge"
    );
    // Level: two cells, clearing the LV label and the plate.
    assert!(
        left_of(bc::panel::LV_DIGITS_RIGHT, 2) >= bc::panel::LV_LABEL.0 + label_w,
        "a two-digit level overruns the panel's LV label cell"
    );
    assert!(
        bc::panel::LV_DIGITS_RIGHT <= plate_w,
        "the level overruns the panel's right edge"
    );

    // And the `engine-ui` mirror really is drawing at those pins: the
    // builder's own four-digit output, cell for cell.
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 9999, 9999, 999, 999);
    let cells = numeral_cells(&hud_frame_with_chrome(&font, &[slot], None));
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 19),
        vec![
            (
                SOLO_PANEL_X + left_of(bc::panel::CUR_RIGHT, 4) as i32,
                "9999".into()
            ),
            (
                SOLO_PANEL_X + left_of(bc::panel::MAX_RIGHT, 4) as i32,
                "9999".into()
            ),
        ],
        "the engine-ui mirror drifted from the battle_chrome pins"
    );
}

/// The same fields, driven through the builder with a four-digit party, so
/// the seats above are checked against what actually gets drawn rather than
/// against arithmetic alone.
#[test]
fn four_digit_party_numerals_land_on_the_pinned_cells() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Vahn", true, true, 4955, 4984, 856, 856);
    slot.level = 99;
    let trio = [slot, slot, slot];

    // Resting: three roster panels, each carrying two four-digit HP runs and
    // two three-digit MP runs at the pinned right edges.
    let panels = hud_frame_with_chrome(&font, &trio, None);
    let cells = numeral_cells(&panels);
    for (i, px) in TRIO_PANEL_X.iter().enumerate() {
        assert_eq!(
            numerals_on_row(&cells, PANEL_Y + 19),
            vec![
                (TRIO_PANEL_X[0] + 25, "4955".into()),
                (TRIO_PANEL_X[0] + 65, "4984".into()),
                (TRIO_PANEL_X[1] + 25, "4955".into()),
                (TRIO_PANEL_X[1] + 65, "4984".into()),
                (TRIO_PANEL_X[2] + 25, "4955".into()),
                (TRIO_PANEL_X[2] + 65, "4984".into()),
            ],
            "panel {i} HP row"
        );
        // Level 99 right-aligns to +96, so its two cells are +80 / +88.
        assert!(
            numerals_on_row(&cells, PANEL_Y + 4).contains(&(px + 80, "99".into())),
            "panel {i} level"
        );
    }
    assert_eq!(
        numerals_on_row(&cells, PANEL_Y + 34),
        vec![
            (TRIO_PANEL_X[0] + 33, "856".into()),
            (TRIO_PANEL_X[0] + 73, "856".into()),
            (TRIO_PANEL_X[1] + 33, "856".into()),
            (TRIO_PANEL_X[1] + 73, "856".into()),
            (TRIO_PANEL_X[2] + 33, "856".into()),
            (TRIO_PANEL_X[2] + 73, "856".into()),
        ],
    );
    // Nothing may leave its own panel: cell right edges stay inside the
    // plate the member owns.
    for &(x, _, _) in &cells {
        let px = TRIO_PANEL_X
            .iter()
            .rev()
            .find(|p| x >= **p)
            .expect("a numeral left of every panel");
        assert!(
            x + DIGIT_W <= px + PANEL_W,
            "a numeral at {x} runs past the panel at {px}"
        );
    }

    // Acting: the same member on the full-width bar, at the bar's own edges.
    // These are the seats the retail reference frame shows for `Vahn HP
    // 2318/2318 MP 316/316` - current back from 134 / 238, maximum back
    // from 178 / 274.
    let bar = hud_frame_with_chrome(&font, &trio, Some(0));
    assert_eq!(
        numerals_on_row(&numeral_cells(&bar), BAR_NAME_Y),
        vec![
            (102, "4955".into()),
            (146, "4984".into()),
            (214, "856".into()),
            (250, "856".into()),
        ],
    );
}

/// Without the numeral strip the digits fall back to font glyphs, and they
/// have to sit on the **same** cell grid - the fallback may change
/// letterforms, never layout. Each glyph is centred in its cell, so its
/// centre is what has to land inside; a font whose digits are wider than
/// eight pixels overhangs symmetrically rather than sliding the run.
#[test]
fn font_fallback_numerals_keep_the_retail_cell_grid() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 4955, 4984, 856, 856);
    let bare = hud_frame(&font, &[slot], &[], &[], false);
    assert!(
        numeral_cells(&bare).is_empty(),
        "no chrome atlas, so no digit sprites"
    );
    // The HP row's eight cells: current 25..57 and maximum 65..97, both
    // right-aligned inside the solo panel.
    let cells: Vec<i32> = (0..4)
        .map(|i| SOLO_PANEL_X + 25 + i * DIGIT_W)
        .chain((0..4).map(|i| SOLO_PANEL_X + 65 + i * DIGIT_W))
        .collect();
    let row_y = PANEL_Y + 19;
    let glyphs: Vec<(i32, i32)> = bare
        .text
        .iter()
        .filter(|d| d.src != SOLID && d.dst.1 / STAGE_SCALE == row_y)
        .map(|d| {
            (
                d.dst.0 / STAGE_SCALE,
                (d.dst.0 + d.dst.2 as i32) / STAGE_SCALE,
            )
        })
        .collect();
    // The row also carries the `/` separator at the panel's +57 seat; every
    // other glyph on it is a numeral and has to centre in one cell.
    let (numerals, rest): (Vec<_>, Vec<_>) = glyphs.iter().partition(|(l, r)| {
        cells
            .iter()
            .any(|c| (l + r) / 2 >= *c && (l + r) / 2 < *c + DIGIT_W)
    });
    assert_eq!(
        numerals.len(),
        8,
        "the fallback seated {} of 8 digits on the cell grid; strays: {rest:?}",
        numerals.len()
    );
    for (l, r) in rest {
        let mid = (l + r) / 2;
        assert!(
            (SOLO_PANEL_X + 57..SOLO_PANEL_X + 65).contains(&mid),
            "an extra glyph at {l}..{r} is neither a numeral cell nor the `/`"
        );
    }
}

/// The proportional fields - names and the diagnostic rows - have to clear
/// the retail dialog font's actual advances, or they collide on screen.
/// Skips and passes when `extracted/font/` is absent (same gating as every
/// other artifact-dependent test), so CI does not need redistributed Sony
/// bytes.
///
/// The numeral fields are **not** here: retail draws those in fixed cells,
/// not in this font, and they are covered font-independently by
/// [`numeral_fields_fit_their_widest_value_in_retail_cells`].
#[test]
fn bar_panel_and_diag_columns_clear_the_retail_font_or_skips() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(root) = manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|w| w.join("extracted"))
        .filter(|c| c.join("font").is_dir())
    else {
        eprintln!("extracted/font not present - skipping");
        return;
    };
    let font = legaia_font::Font::load_from_extracted(&root).expect("load extracted font");
    let width = |s: &str| -> i32 {
        font.layout_ascii(s)
            .glyphs
            .last()
            .map(|g| g.dst_x + g.width as i32)
            .unwrap_or(0)
    };

    // The name is the one proportional field on either surface: it must end
    // before the label cell that follows it.
    assert!(
        16 + width("Songi") <= 80,
        "the longest party name overruns the HP label seat at 80"
    );
    assert!(
        5 + width("Songi") <= 64,
        "the longest party name overruns the panel's LV label at +64"
    );

    // Diagnostic monster row: name at 0, HP numerals at 78, K.O. at 150,
    // status strip at 190.
    let columns: [(i32, i32, &str); 3] = [
        (0, width("Juggernaut"), "name"),
        (78, width("250/300"), "HP"),
        (150, width("K.O."), "K.O."),
    ];
    for pair in columns.windows(2) {
        let (x, w, label) = pair[0];
        let (next_x, _, next_label) = pair[1];
        assert!(
            x + w <= next_x,
            "{label} field ends at {} but {next_label} starts at {next_x}",
            x + w
        );
    }
    let last_field_end = columns[2].0 + columns[2].1;
    assert!(
        last_field_end <= 190,
        "row fields end at {last_field_end}, past the status column at 190"
    );

    // Non-vacuous: a full diagnostic monster row really draws its status strip.
    let mut slot = slot_view("Juggernaut", false, false, 250, 300, 0, 0);
    slot.status_sprite = 0x1B;
    let draws = hud_frame(&font, &[slot], &[], &[], true).text;
    assert!(
        draws.iter().any(|d| d.dst.0 >= 190),
        "status column produced no glyph - the fixture is not exercising a full row"
    );
}

// ---------------------------------------------------------------------------
// The top-left seat: banner, plaque, and the badges that ride them
// ---------------------------------------------------------------------------

/// Atlas cell of status badge `i`, mirroring
/// `legaia_engine_core::save_menu_atlas::status_badge_atlas_rect` - this
/// crate sits below `engine-core`, so the layout is repeated here and
/// `engine-shell`'s `badge_atlas_seats_match_the_bake` pins the two equal.
const fn status_badge_cell(i: usize) -> (u32, u32, u32, u32) {
    (48 * (i as u32 % 4), 128 + 16 * (i as u32 / 4), 48, 16)
}
/// Sibling of the above for the element strip
/// (`save_menu_atlas::element_badge_atlas_rect`).
const fn element_badge_cell(i: usize) -> (u32, u32, u32, u32) {
    (20 * i as u32, 176, 20, 12)
}

/// The badge cells at the seats `save_menu_atlas` bakes them to.
fn badge_rects() -> legaia_engine_ui::battle_hud_chrome::BattleBadgeRects {
    legaia_engine_ui::battle_hud_chrome::BattleBadgeRects {
        status: std::array::from_fn(|i| Some(status_badge_cell(i))),
        element: std::array::from_fn(|i| Some(element_badge_cell(i))),
    }
}

/// Retail's message banner and the actor-name plaque share content pen
/// `(16, 12)`. They are alternatives on one seat, so a frame carrying a
/// message draws the banner and **no** plaque - which is the
/// two-text-runs-on-one-pen artifact this rule exists to stop.
#[test]
fn the_message_banner_takes_the_plaques_seat() {
    use legaia_engine_ui::battle_hud_chrome as bhc;
    let font = legaia_font::synthetic_for_tests();
    let rects = chrome_rects();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            plaque: Some("Gimard"),
            banner: Some("Noa gained a level!"),
            ..Default::default()
        },
        PEN,
    );
    assert!(
        !draws
            .sprites
            .iter()
            .any(|s| s.src.1 == 232 && s.dst.1 / STAGE_SCALE == PLAQUE_Y),
        "the plaque drew under the banner"
    );
    let content = bhc::message_banner_content(&font, "Noa gained a level!");
    let (fx, fy, fw, fh) = bhc::banner_frame(content.0, content.1);
    assert_eq!((fx, fy), (8, 4), "banner frame origin");
    assert_eq!(fh, 28, "single-line banner is 28 tall");
    let corners: Vec<(i32, i32)> = draws
        .sprites
        .iter()
        .filter(|s| s.src.2 == 4 && s.src.3 == 4)
        .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE))
        .collect();
    assert!(
        corners.contains(&(fx, fy)),
        "no top-left corner: {corners:?}"
    );
    assert!(corners.contains(&(fx + fw - 4, fy + fh - 4)));
    // Retail draws no interior fill under the banner - the scene shows
    // through - so the gradient column must not appear.
    assert!(
        !draws.sprites.iter().any(|s| s.src == rects.dialog_fill),
        "the banner painted an interior fill"
    );
}

/// A host that draws its own box on the plaque's pen (the sparring-tutorial
/// prompt, whose rect starts at `(0x10, 0x0E)`) claims the seat outright.
#[test]
fn a_host_owned_box_suppresses_the_plaque() {
    let font = legaia_font::synthetic_for_tests();
    let rects = chrome_rects();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let frame = |taken: bool| {
        battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                slots: std::slice::from_ref(&slot),
                solid_src: Some(SOLID),
                surface: SURFACE,
                chrome: Some(&rects),
                plaque: Some("Vahn"),
                plaque_seat_taken: taken,
                ..Default::default()
            },
            PEN,
        )
    };
    let plate = |d: &BattleHudDraws| {
        d.sprites
            .iter()
            .filter(|s| s.src.1 == 232 && s.dst.1 / STAGE_SCALE == PLAQUE_Y)
            .count()
    };
    assert!(plate(&frame(false)) > 0, "baseline draws the plaque");
    assert_eq!(
        plate(&frame(true)),
        0,
        "the claimed seat still drew a plaque"
    );
}

/// `FUN_8002C2E4`'s matched-ailment arm blits a 48x16 word cell at the panel
/// pen's `+ (0x33, -4)`, which is `(56, 0)` off the panel corner. With the
/// cell present the HUD draws it; without one it keeps the labelled tag, so
/// an atlas that could not reach a badge's palette still reads.
#[test]
fn the_status_badge_blits_at_the_ladder_seat() {
    use legaia_engine_ui::battle_hud_chrome::STATUS_BADGE_PANEL_SEAT;
    let font = legaia_font::synthetic_for_tests();
    let rects = chrome_rects();
    let badges = badge_rects();
    let mut slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    slot.level = 12;
    slot.status_sprite = 0x1D; // Numb - ladder index 5.
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: std::slice::from_ref(&slot),
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            badges: Some(&badges),
            ..Default::default()
        },
        PEN,
    );
    let want = status_badge_cell(5);
    let seat = draws
        .sprites
        .iter()
        .find(|s| s.src == want)
        .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE));
    assert_eq!(
        seat,
        Some((
            SOLO_PANEL_X + STATUS_BADGE_PANEL_SEAT.0,
            PANEL_Y + STATUS_BADGE_PANEL_SEAT.1
        )),
        "the Numb badge is not on the ladder seat"
    );
    // The ladder is exclusive: the badge replaces the level, it does not
    // join it, so retail's LV label sprite is gone from this panel.
    assert!(
        !draws.sprites.iter().any(|s| s.src == rects.label_lv),
        "the badge and the LV label both drew"
    );

    // Without the cell the same slot falls back to the engine's tag.
    let bare = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: std::slice::from_ref(&slot),
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            ..Default::default()
        },
        PEN,
    );
    assert!(
        !bare.sprites.iter().any(|s| s.src == want),
        "a badge blitted with no cell"
    );
    assert!(
        bare.text.len() > draws.text.len(),
        "the fallback drew no tag glyphs"
    );
}

/// An actor that carries an element wears its badge in front of the name and
/// the plaque interior grows by `20 + 5` - the second half of
/// `battle_chrome::name_plaque`'s packet-pinned law.
#[test]
fn the_plaque_badge_widens_the_plate_and_shifts_the_name() {
    let font = legaia_font::synthetic_for_tests();
    let rects = chrome_rects();
    let badges = badge_rects();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let frame = |badge: Option<u8>| {
        battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                slots: std::slice::from_ref(&slot),
                solid_src: Some(SOLID),
                surface: SURFACE,
                chrome: Some(&rects),
                plaque: Some("Gimard"),
                plaque_badge: badge,
                badges: Some(&badges),
                ..Default::default()
            },
            PEN,
        )
    };
    // Right cap of the gold run = the rightmost tile on the plaque row.
    let right_cap = |d: &BattleHudDraws| {
        d.sprites
            .iter()
            .filter(|s| s.src.1 == 232 && s.dst.1 / STAGE_SCALE == PLAQUE_Y)
            .map(|s| s.dst.0 / STAGE_SCALE)
            .max()
            .unwrap_or(0)
    };
    let bare = frame(None);
    let with = frame(Some(3));
    assert_eq!(
        right_cap(&with) - right_cap(&bare),
        20 + 5,
        "the badge did not widen the plaque interior by badge + gap"
    );
    let badge_seat = with
        .sprites
        .iter()
        .find(|s| s.src == element_badge_cell(3))
        .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE));
    assert_eq!(
        badge_seat,
        Some((PLAQUE_X + 8, PLAQUE_Y + 4)),
        "the element badge is not on the plaque's content pen"
    );
}
