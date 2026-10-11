//! The top-left seat: banner, plaque, and the badges that ride them.

use super::*;
use legaia_engine_ui::BattleHudDraws;
use legaia_engine_ui::BattleHudFrame;
use legaia_engine_ui::battle_hud_draws_for;

// ---------------------------------------------------------------------------
// The top-left seat: banner, plaque, and the badges that ride them
// ---------------------------------------------------------------------------

/// Atlas cell of status badge `i`, mirroring
/// `legaia_engine_menus::save_menu_atlas::status_badge_atlas_rect` - this
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
    // Retail fills the frame with the marbled patch (`FUN_8002BDC4`) under
    // the border: full-height columns of the baked interior tile from the
    // frame origin across the record's 296-wide frame.
    assert_eq!((fw, fh), (296, 28), "the record's fixed 280-wide box");
    let fill: Vec<(i32, i32)> = draws
        .sprites
        .iter()
        .filter(|s| s.src.0 == 128 && s.src.1 == 0 && s.dst.1 / STAGE_SCALE == fy)
        .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.3 as i32 / STAGE_SCALE))
        .collect();
    assert_eq!(fill.len(), 10, "ten fill columns: {fill:?}");
    assert!(fill.iter().all(|&(_, h)| h == 28));
    assert_eq!(fill[0].0, fx);
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
