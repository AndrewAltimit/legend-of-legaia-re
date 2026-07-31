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

/// With the resident system-UI atlas the strip's chrome and its gold `HP` /
/// green `MP` label cells come out as **sprites**, not glyphs - the second
/// list the builder returns. Without an atlas the same information degrades
/// into the text list, which is what keeps a chrome-less host readable.
#[test]
fn chrome_atlas_moves_the_strip_skin_and_labels_into_sprites() {
    let font = legaia_font::synthetic_for_tests();
    let rects = SaveMenuAtlasRects {
        label_hp: (208, 86, 16, 10),
        label_mp: (224, 86, 16, 10),
        dialog_fill: (128, 0, 32, 29),
        // The 9-slice needs real tile extents - it tiles the edges.
        panel_tl: (160, 0, 4, 4),
        panel_tr: (188, 0, 4, 4),
        panel_bl: (160, 28, 4, 4),
        panel_br: (188, 28, 4, 4),
        panel_top: (164, 0, 24, 4),
        panel_bot: (164, 28, 24, 4),
        panel_left: (160, 4, 4, 24),
        panel_right: (188, 4, 4, 24),
        ..Default::default()
    };
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            popups: &[],
            log: &[],
            solid_src: Some(SOLID),
            surface: SURFACE,
            chrome: Some(&rects),
            ..Default::default()
        },
        PEN,
    );
    assert!(
        draws.sprites.iter().any(|s| s.src == rects.label_hp
            && s.dst.0 == (SOLO_PANEL_X + 4) * STAGE_SCALE
            && s.dst.1 == (PANEL_Y + 21) * STAGE_SCALE),
        "the HP label cell is not at the panel's pinned HP-row seat"
    );
    assert!(
        draws.sprites.iter().any(|s| s.src == rects.label_mp),
        "the green MP label cell never drew"
    );
    assert!(
        draws.sprites.iter().any(|s| s.src == rects.dialog_fill),
        "the strip interior never drew from the atlas"
    );
    // With chrome the plate body is a sprite, so the text list keeps no
    // solid rect for it.
    assert!(
        !draws.text.iter().any(|d| d.src == SOLID),
        "the solid-texel fallback body drew alongside the atlas chrome"
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

#[test]
fn battle_hud_draws_for_party_popup_rides_its_panel_anchor() {
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
    let n_no_popup = hud_draws(&font, &[slot], &[], &[]).len();
    let draws = hud_draws(&font, &[slot], &[popup], &[]);
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
    assert!(
        with.iter().any(|d| d.src != SOLID
            && d.dst.0 == (SOLO_PANEL_X + 88) * STAGE_SCALE
            && d.dst.1 == (PANEL_Y + 4) * STAGE_SCALE),
        "the level digits are not at the panel's pinned LV seat"
    );
}

/// An ailment adds its own badge inside the member's panel - the selection is
/// the ported part, the badge art and placement are approximations.
#[test]
fn ailment_badge_draws_above_the_members_strip() {
    let font = legaia_font::synthetic_for_tests();
    let clean = slot_view("Vahn", true, true, 100, 100, 30, 30);
    let mut sick = slot_view("Vahn", true, true, 100, 100, 30, 30);
    sick.status_sprite = 0x1F;
    let with = hud_draws(&font, &[sick], &[], &[]);
    let without = hud_draws(&font, &[clean], &[], &[]);
    assert!(with.len() > without.len(), "the ailment badge never drew");
    // The badge rides the member's own panel, under its MP row - retail's
    // element art and its caller-supplied pen are both unpinned, so what is
    // pinned here is only that it stays inside the member's own panel.
    assert!(
        with.iter().any(|d| {
            let x = d.dst.0 / STAGE_SCALE;
            let y = d.dst.1 / STAGE_SCALE;
            (SOLO_PANEL_X..SOLO_PANEL_X + PANEL_W).contains(&x)
                && (PANEL_Y..PANEL_Y + PANEL_H + 12).contains(&y)
        }),
        "the badge did not draw inside the member's own panel"
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
    let with_popup = hud_draws(&font, &[slot], &[popup], &[]);
    let no_popup = hud_draws(&font, &[slot], &[], &[]);
    assert_eq!(with_popup.len(), no_popup.len());
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
    let draws = hud_draws(&font, &[slot], &[popup], &[]);
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

/// The measured strip columns have to clear the retail dialog font's actual
/// advances, or the fields collide on screen. Skips and passes when
/// `extracted/font/` is absent (same gating as every other artifact-dependent
/// test), so CI does not need redistributed Sony bytes.
///
/// Three column sets ride this: the active-actor bar, the roster panel and
/// the diagnostic monster row. In both retail surfaces a *current* value is
/// right-aligned against a fixed edge while its *maximum* runs forward from
/// its own, so each side has to clear a different neighbour.
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

    // Active-actor bar: the name must end before the HP label's seat, each
    // right-aligned current must start after the label before it, and each
    // forward-running maximum must end before the next field.
    assert!(
        16 + width("Songi") <= 80,
        "the longest party name overruns the HP label seat at 80"
    );
    assert!(
        134 - width("9999") >= 80 + 16,
        "a four-digit HP current overruns the HP label cell"
    );
    assert!(
        154 + width("9999") <= 192,
        "a four-digit HP maximum overruns the MP label seat"
    );
    assert!(
        238 - width("999") >= 192 + 16,
        "a three-digit MP current overruns the MP label cell"
    );
    assert!(
        258 + width("999") <= 312,
        "a three-digit MP maximum overruns the bar's right cap"
    );
    // Roster panel: the same two fields inside a 102-px panel.
    assert!(
        5 + width("Songi") <= 64,
        "the longest party name overruns the panel's LV label at +64"
    );
    assert!(
        57 - width("9999") >= 4 + 16,
        "a four-digit HP current overruns the panel's HP label cell"
    );
    assert!(
        73 + width("9999") <= 102,
        "a four-digit maximum overruns the panel's right edge"
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
