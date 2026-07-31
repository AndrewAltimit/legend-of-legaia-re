use super::*;

/// A recognisable 1x1 solid src for the filled-rect draws.
const SOLID: (u32, u32, u32, u32) = (5, 9, 1, 1);
const SURFACE: (u32, u32) = (640, 480);
const PEN: (i32, i32) = (8, 100);

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

fn hud_draws(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    popups: &[HudPopupView],
    log: &[HudLogView<'_>],
) -> Vec<TextDraw> {
    battle_hud_draws_for(
        font,
        &BattleHudFrame {
            slots,
            popups,
            log,
            solid_src: Some(SOLID),
            surface: SURFACE,
        },
        PEN,
    )
}

#[test]
fn battle_hud_draws_for_party_row_includes_glyphs_and_bars() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Vahn", true, true, 250, 300, 12, 30);
    slot.ap_filled = 2;
    slot.ap_max = 5;
    let draws = hud_draws(&font, &[slot], &[], &[]);
    assert!(!draws.is_empty());
    // Panel chrome + HP/MP bar rects sample the solid texel.
    assert!(draws.iter().filter(|d| d.src == SOLID).count() >= 4);
    // The HP fill takes the HIGH gauge colour (250 > 300/2 -> index 7).
    assert!(
        draws
            .iter()
            .any(|d| d.src == SOLID && d.color == gauge_fill_color(7))
    );
}

#[test]
fn battle_hud_draws_for_skips_empty_slot_name() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("", true, true, 0, 0, 0, 0);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    assert!(draws.is_empty());
}

#[test]
fn battle_hud_draws_for_dead_slot_shows_ko_overlay() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, false, 0, 300, 0, 30);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // The K.O. label draws in red over the panel.
    let red = [1.0, 0.4, 0.4, 1.0];
    assert!(draws.iter().any(|d| d.src != SOLID && d.color == red));
    // Dead gauge fill (index 2) reaches the bar surface.
    assert!(
        draws
            .iter()
            .any(|d| d.src == SOLID && d.color == gauge_fill_color(2))
    );
}

#[test]
fn battle_hud_draws_for_low_hp_uses_red_color() {
    let font = legaia_font::synthetic_for_tests();
    let slot = slot_view("Vahn", true, true, 10, 100, 0, 0);
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // Numerals take the danger tint (red-dominant) and the fill the LOW band.
    let any_red = draws
        .iter()
        .any(|d| d.src != SOLID && d.color[0] > d.color[1]);
    assert!(any_red, "low HP should produce a red-tinted glyph");
    assert!(
        draws
            .iter()
            .any(|d| d.src == SOLID && d.color == gauge_fill_color(9))
    );
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
        },
        PEN,
    );
    assert!(!draws.is_empty(), "text-only fallback still draws numerals");
    assert!(
        !draws.iter().any(|d| d.src == SOLID),
        "no rect may be emitted without a solid src"
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
    // Damage popups draw in the cyan tint - and above the panel band's top
    // edge (a stage-space anchor, unlike the monster rows' pen anchor).
    let cyan = [0.5, 0.85, 1.0, 1.0];
    assert!(
        draws.iter().any(|d| d.src != SOLID && d.color == cyan),
        "no cyan popup glyph"
    );
}

#[test]
fn battle_hud_draws_for_monster_status_element_renders_in_the_column() {
    let font = legaia_font::synthetic_for_tests();
    let mut slot = slot_view("Gimard", false, true, 100, 100, 0, 0);
    // Sprite 0x19 = Toxic, the element retail's ladder picks for `0x0002`.
    slot.status_sprite = 0x19;
    let draws = hud_draws(&font, &[slot], &[], &[]);
    // The element renders past the column origin at pen.x + 190.
    let icons = draws.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count();
    assert!(icons > 0, "expected a status element in the monster column");

    // And it is exactly ONE element regardless of how many ailments the
    // slot carries - the whole point of the retail ladder.
    let mut clean = slot_view("Gimard", false, true, 100, 100, 0, 0);
    clean.status_sprite = 0;
    let none = hud_draws(&font, &[clean], &[], &[]);
    assert_eq!(
        none.iter().filter(|d| d.dst.0 >= PEN.0 + 190).count(),
        0,
        "an unafflicted monster row draws no status element"
    );
}

#[test]
fn party_panel_draws_the_level_when_no_ailment_is_selected() {
    let font = legaia_font::synthetic_for_tests();
    // Retail's no-ailment arm is the base marker plus the `+0x130` level.
    let mut lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    lv.level = 27;
    let with_level = hud_draws(&font, &[lv], &[], &[]);

    let mut no_lv = slot_view("Vahn", true, true, 100, 100, 30, 30);
    no_lv.level = 0;
    let without = hud_draws(&font, &[no_lv], &[], &[]);
    assert!(
        with_level.len() > without.len(),
        "the level readout produced no glyphs"
    );

    // An ailment replaces it: the count is not drawn beside a sprite.
    let mut sick = slot_view("Vahn", true, true, 100, 100, 30, 30);
    sick.level = 27;
    sick.status_sprite = 0x1F;
    let ailing = hud_draws(&font, &[sick], &[], &[]);
    assert_ne!(ailing.len(), with_level.len());
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

/// The monster-row column offsets have to be wider than the retail dialog
/// font's actual advances, or fields overlap on screen. Skips and passes when
/// `extracted/font/` is absent (same gating as every other artifact-dependent
/// test), so CI does not need redistributed Sony bytes.
///
/// This is the regression guard for the first draft of the old table layout,
/// which was narrower than the font in four of five columns - the K.O. label
/// landed on top of the HP digits. It went unnoticed because the builder had
/// no caller.
#[test]
fn monster_row_offsets_clear_the_retail_font_or_skips() {
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

    // Column origins the builder documents (monster rows): name at 0, HP
    // numerals at 78, K.O. at 150, status strip at 190. Each field's widest
    // realistic string must end before the next column starts.
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
    // And the party-panel numerals must fit the pinned 0x40-px panel width.
    assert!(
        4 + width("999/999") <= 0x40,
        "HP numerals overflow the 64-px panel"
    );

    // Non-vacuous: a full monster row really draws its status strip.
    let mut slot = slot_view("Juggernaut", false, false, 250, 300, 0, 0);
    slot.status_sprite = 0x1B;
    let draws = hud_draws(&font, &[slot], &[], &[]);
    assert!(
        draws.iter().any(|d| d.dst.0 >= 190),
        "status column produced no glyph - the fixture is not exercising a full row"
    );
}
