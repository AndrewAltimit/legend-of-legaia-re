//! The party surface: resting roster panels, the active-actor bar, the message bar, plates and host-box parking.

use super::*;
use legaia_engine_ui::BattleHudFrame;
use legaia_engine_ui::battle_hud_draws_for;
use legaia_engine_ui::gauge_fill_color;

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

/// The death-spoils caption (placement record 91) takes the active-actor
/// bar's own seat: its plate lands where the bar's would, its glyphs start on
/// the bar's name pen, and the acting member's readout does not draw under
/// it (`player_steal_skeleton_banner` shows the caption alone on the row).
#[test]
fn the_message_bar_takes_the_active_actor_bars_seat() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    let draws = battle_hud_draws_for(
        &font,
        &BattleHudFrame {
            slots: &[slot],
            solid_src: Some(SOLID),
            surface: SURFACE,
            active_slot: Some(0),
            message_bar: Some("Line"),
            ..Default::default()
        },
        PEN,
    )
    .text;
    assert_eq!(
        boxes_of(&draws, BAR_W, BAR_H),
        vec![(BAR_X, BAR_Y)],
        "one plate, on the bar's seat"
    );
    let on_pen: Vec<_> = draws
        .iter()
        .filter(|d| d.src != SOLID && d.dst.1 == BAR_NAME_Y * STAGE_SCALE)
        .collect();
    assert_eq!(
        on_pen.len(),
        4,
        "the caption's four glyphs, and nothing else"
    );
    assert_eq!(on_pen[0].dst.0, 16 * STAGE_SCALE);
}

/// A message bar still gliding up (`FUN_801D8DE8(0x5B, 0)` spawns it at
/// seat A) draws its caption `message_bar_dy` below the pen, and leaves the
/// target plaque on the row above it in view until its frame reaches the
/// plaque (`zeto_call_wave_mid_cast`: the plaque on its row, the bar's top
/// edge under it).
#[test]
fn a_gliding_message_bar_rides_below_its_seat_and_spares_the_target_plaque() {
    let font = legaia_font::synthetic_for_tests();
    let frame = |dy: i32| BattleHudFrame {
        solid_src: Some(SOLID),
        surface: SURFACE,
        message_bar: Some("Line"),
        message_bar_dy: dy,
        target_plaque: Some(("Zeto", None)),
        ..Default::default()
    };
    let rising = battle_hud_draws_for(&font, &frame(27), PEN).text;
    assert!(
        rising.iter().any(|d| d.src != SOLID
            && d.dst.0 == 16 * STAGE_SCALE
            && d.dst.1 == (BAR_NAME_Y + 27) * STAGE_SCALE),
        "the caption rides the glide"
    );
    let plaque_glyphs = |d: &[TextDraw]| {
        d.iter()
            .filter(|t| t.src != SOLID && t.dst.0 > 200 * STAGE_SCALE)
            .count()
    };
    assert_eq!(
        plaque_glyphs(&rising),
        4,
        "the plaque stays up under a rising bar"
    );
    let landed = battle_hud_draws_for(&font, &frame(0), PEN).text;
    assert_eq!(plaque_glyphs(&landed), 0, "a landed bar hides the plaque");
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

/// A K.O.'d member keeps its row and its readout goes **red** - retail's
/// strip has no loose "K.O." legend, and its death tier is the brightest
/// colour on the panel rather than a grey-out.
///
/// Read off a retail battle frame whose third party member is at 0 HP: the
/// downed panel's HP *and* MP numerals take font CLUT `(128, 510)`, entry 15
/// `(230, 32, 0)`, while its **name** keeps the same `(208, 510)` the two
/// living members' names use. The port used to grey all three, which is the
/// shape this test previously asserted.
#[test]
fn dead_member_reads_red_and_keeps_its_name_colour() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, false, 0, 300, 20, 30);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    let dead = gauge_fill_color(2);
    assert_eq!(
        dead.map(|c| (c * 31.0).round() as u8),
        [28, 4, 0, 31],
        "retail's dead tier is red"
    );
    let glyphs: Vec<_> = draws.iter().filter(|d| d.src != SOLID).collect();
    assert!(
        !glyphs.is_empty(),
        "the downed member drew no glyphs at all"
    );
    assert!(
        glyphs.iter().any(|d| d.color == dead),
        "dead member's numerals did not take the retail death tint"
    );
    assert!(
        glyphs
            .iter()
            .any(|d| d.color == legaia_engine_ui::READOUT_NORMAL),
        "dead member's NAME was tinted - retail leaves it in the normal readout colour"
    );
    let old_dim = [0.5f32, 0.5, 0.5, 1.0];
    assert!(
        !glyphs.iter().any(|d| d.color == old_dim),
        "the old grey-out tint is still on the panel"
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
            panels_parked: true,
            ..Default::default()
        },
        PEN,
    );
    assert!(
        boxes_of(&draws.text, PANEL_W, PANEL_H).is_empty(),
        "the roster panels still drew under a parked input session"
    );
}

/// A host box on the party surface's own rows parks that surface, the same
/// exclusive-seat rule the plaque already follows one band up.
///
/// Retail's bottom-anchored sparring-prompt styles anchor at `0xCC` / `0xB0`
/// / `0x9A`, so a one-line style-3 box is `(0x10, 194, w, 10)` - skin
/// `186..212`, straight through the active-actor bar at `188..208` and inside
/// the roster panels' `164..212`. Both surfaces drew under it, which is what
/// `Vahn  HP 151/ 180` reading through the prompt text was.
#[test]
fn a_bottom_anchored_host_box_parks_the_party_surface_it_covers() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 151, 180, 12, 30);
    let frame = |host_box, active| {
        battle_hud_draws_for(
            &font,
            &BattleHudFrame {
                slots: &[slot],
                solid_src: Some(SOLID),
                surface: SURFACE,
                host_box,
                active_slot: active,
                ..Default::default()
            },
            PEN,
        )
        .text
    };
    // A one-line style-3 prompt: `BoxStyle { bottom_anchor: 0xCC }`, h = 10.
    let bottom = Some((0x10, 0xCC - 10, 120, 10));
    // A style-0 prompt on the top seat leaves both surfaces alone.
    let top = Some((0x10, 0x0E, 120, 24));

    assert!(
        !boxes_of(&frame(None, None), PANEL_W, PANEL_H).is_empty(),
        "non-vacuous: the roster panel draws with no box up"
    );
    assert!(
        boxes_of(&frame(bottom, None), PANEL_W, PANEL_H).is_empty(),
        "the roster panels drew under a bottom-anchored prompt"
    );
    assert!(
        !boxes_of(&frame(top, None), PANEL_W, PANEL_H).is_empty(),
        "a top-seat prompt must not park the roster panels"
    );

    // The bar is the surface that is actually up during command entry.
    let bar_glyphs = |draws: &[TextDraw]| {
        draws
            .iter()
            .filter(|d| d.src != SOLID && d.dst.1 >= BAR_Y * STAGE_SCALE)
            .count()
    };
    assert!(
        bar_glyphs(&frame(None, Some(0))) > 0,
        "non-vacuous: the active-actor bar draws with no box up"
    );
    assert_eq!(
        bar_glyphs(&frame(bottom, Some(0))),
        0,
        "the active-actor bar drew under a bottom-anchored prompt"
    );
}

/// The enemy target strip steps clear of a host box that shares its row.
///
/// Retail's target-select hint is style `5` - centred, bottom-anchored at
/// `0xB0` - so a three-line hint's last row lands on stage `166`, which is
/// exactly [`legaia_engine_ui::ENEMY_MENU_STAGE_Y`]. Left alone the two draw
/// on the same pixels ("Tetsu" through `...only one target.").
#[test]
fn the_enemy_strip_steps_off_a_host_boxs_row() {
    use legaia_engine_ui::{ENEMY_MENU_STAGE_Y, enemy_target_menu_rows_y};
    assert_eq!(
        enemy_target_menu_rows_y(None),
        ENEMY_MENU_STAGE_Y,
        "the strip must not move when no box is up"
    );
    // Three-line style-5 hint: h = 3 * 14 - 4 = 38, y = 0xB0 - 38 = 138.
    let hint = (0x30, 0xB0 - 38, 200, 38);
    let y = enemy_target_menu_rows_y(Some(hint));
    let (box_top, box_bot) = (hint.1 - 8, hint.1 + hint.3 + 8);
    assert!(
        y + 14 <= box_top || y >= box_bot,
        "the strip row {y} still overlaps the box band {box_top}..{box_bot}"
    );
    assert!(y < ENEMY_MENU_STAGE_Y, "the strip should step UP, not down");
    assert_eq!(
        (ENEMY_MENU_STAGE_Y - y) % 14,
        0,
        "the strip must step in whole text rows"
    );
    // A box that does not reach the strip leaves it where it was.
    assert_eq!(
        enemy_target_menu_rows_y(Some((0x10, 0x0E, 120, 24))),
        ENEMY_MENU_STAGE_Y
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
