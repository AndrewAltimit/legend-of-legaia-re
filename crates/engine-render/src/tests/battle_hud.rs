use super::*;

#[test]
fn battle_hud_draws_for_party_row_includes_name_hp_mp_ap() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 250,
        hp_max: 300,
        mp: 12,
        mp_max: 30,
        ap_filled: 2,
        ap_max: 5,
        status_letters: &[],
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    // Row produces glyphs for name, HP, MP, AP - at minimum one draw.
    assert!(!draws.is_empty());
}

#[test]
fn battle_hud_draws_for_skips_empty_slot_name() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "",
        is_party: true,
        alive: true,
        hp: 0,
        hp_max: 0,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    assert!(draws.is_empty());
}

#[test]
fn battle_hud_draws_for_dead_slot_shows_ko_overlay() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: false,
        hp: 0,
        hp_max: 300,
        mp: 0,
        mp_max: 30,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    // Should include the K.O. label glyphs.
    assert!(!draws.is_empty());
}

#[test]
fn battle_hud_draws_for_low_hp_uses_red_color() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 10,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    // Find any draw with the dim/red HP coloring - red has more red than green.
    let any_red = draws.iter().any(|d| d.color[0] > d.color[1]);
    assert!(any_red, "low HP should produce a red-tinted glyph");
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
fn battle_hud_caution_mp_uses_yellow_not_row_color() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Noa",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 15,
        mp_max: 40, // 15 is in (10, 20] -> caution -> yellow
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    // Yellow = [1.0, 0.95, 0.4]: high R+G, low B. Row color (white) has B==1.
    let any_yellow = draws.iter().any(|d| d.color[1] > 0.9 && d.color[2] < 0.5);
    assert!(
        any_yellow,
        "caution MP should produce a yellow-tinted glyph"
    );
}

#[test]
fn battle_hud_draws_for_includes_log_lines_below_slots() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let log = [HudLogView {
        text: "Vahn attacks.",
        color: [1.0, 1.0, 1.0, 1.0],
    }];
    let draws_no_log = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    let n_no_log = draws_no_log.len();
    let draws_with_log = battle_hud_draws_for(&font, &[slot], &[], &log, (8, 100));
    assert!(draws_with_log.len() > n_no_log);
}

#[test]
fn battle_hud_draws_for_popup_anchored_above_slot_row() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let popup = HudPopupView {
        slot: 0,
        amount: 250,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    let pen = (8, 100);
    let draws = battle_hud_draws_for(&font, &[slot], &[popup], &[], pen);
    // Find a draw whose y is above pen.1 (popup is at pen.1 - 16).
    let any_above = draws.iter().any(|d| d.dst.1 < pen.1);
    assert!(any_above, "popup should sit above the slot row");
}

#[test]
fn battle_hud_draws_for_status_letters_render_above_row() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: b"BP",
    };
    let pen = (8, 100);
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], pen);
    // Status icons render at y - 12.
    let icons = draws.iter().filter(|d| d.dst.1 == pen.1 - 12).count();
    assert!(icons > 0, "expected status icons rendered above the row");
}

#[test]
fn battle_hud_draws_for_popup_for_invalid_slot_is_dropped() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let popup = HudPopupView {
        slot: 99,
        amount: 50,
        is_heal: false,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    let with_popup = battle_hud_draws_for(&font, &[slot], &[popup], &[], (8, 100));
    let no_popup = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
    assert_eq!(with_popup.len(), no_popup.len());
}

#[test]
fn battle_hud_draws_for_heal_popup_uses_green_tint() {
    let font = legaia_font::synthetic_for_tests();
    let slot = HudSlotView {
        name: "Vahn",
        is_party: true,
        alive: true,
        hp: 100,
        hp_max: 100,
        mp: 0,
        mp_max: 0,
        ap_filled: 0,
        ap_max: 0,
        status_letters: &[],
    };
    let popup = HudPopupView {
        slot: 0,
        amount: 60,
        is_heal: true,
        is_crit: false,
        status_letter: None,
        alpha: 1.0,
    };
    let draws = battle_hud_draws_for(&font, &[slot], &[popup], &[], (8, 100));
    // Heal color is green: [0.5, 1.0, 0.5, 1.0]; any glyph with that profile.
    let any_green = draws
        .iter()
        .any(|d| d.color[1] >= 0.95 && d.color[0] < d.color[1]);
    assert!(any_green);
}

#[test]
fn apply_alpha_scales_only_alpha_channel() {
    let c = [0.5, 0.6, 0.7, 1.0];
    let scaled = apply_alpha(c, 0.5);
    assert_eq!(scaled, [0.5, 0.6, 0.7, 0.5]);
}

/// The HUD's column offsets have to be wider than the retail dialog font's
/// actual advances, or fields overlap on screen. Skips and passes when
/// `extracted/font/` is absent (same gating as every other artifact-dependent
/// test), so CI does not need redistributed Sony bytes.
///
/// This is the regression guard for the first draft of the offsets, which was
/// narrower than the font in four of five columns - the K.O. label landed on
/// top of the HP digits. It went unnoticed because the builder had no caller.
#[test]
fn column_offsets_clear_the_retail_font_or_skips() {
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

    let slot = HudSlotView {
        name: "Juggernaut",
        is_party: true,
        alive: false,
        hp: 250,
        hp_max: 300,
        mp: 10,
        mp_max: 30,
        ap_filled: 4,
        ap_max: 8,
        status_letters: b"TC",
    };
    let pen = (0, 0);
    let draws = battle_hud_draws_for(&font, &[slot], &[], &[], pen);

    // Column origins the builder documents, paired with the widest string it
    // can place there. Each field must end before the next one starts.
    let columns: [(i32, i32, &str); 5] = [
        (0, width("Juggernaut"), "name"),
        (78, width("HP 250/300"), "HP"),
        (161, width("MP  10/ 30"), "MP"),
        (240, width("APoooo----"), "AP"),
        (319, width("K.O."), "K.O."),
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
    // And the drawn row must not run past the status strip's first cell.
    let last_field_end = columns[4].0 + columns[4].1;
    assert!(
        last_field_end <= 359,
        "row fields end at {last_field_end}, past the status strip at 359"
    );
    // Non-vacuous: the row really did draw every field.
    assert!(
        draws.iter().any(|d| d.dst.0 >= 359),
        "status strip produced no glyph - the fixture is not exercising a full row"
    );
}
