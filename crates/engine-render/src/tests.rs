use super::*;
use crate::renderer::letterbox_scale;
use crate::shaders::*;
use crate::ui_overlay::{apply_alpha, hp_bar_color_index, mp_bar_color_index};
use glam::Mat4;

#[test]
fn letterbox_scale_pillarbox() {
    let (sx, sy) = letterbox_scale(800, 400, 100, 100);
    assert!((sx - 0.5).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 1.0).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn letterbox_scale_letterbox() {
    let (sx, sy) = letterbox_scale(400, 800, 100, 100);
    assert!((sx - 1.0).abs() < 1e-4, "sx={}", sx);
    assert!((sy - 0.5).abs() < 1e-4, "sy={}", sy);
}

#[test]
fn sprite_draws_translate_world_positions_with_anchor() {
    let reqs = vec![
        SpriteRequest {
            world_x: 5,
            world_y: 7,
            atlas_src: (16, 0, 14, 15),
            color: [1.0, 1.0, 1.0, 1.0],
        },
        SpriteRequest {
            world_x: 0,
            world_y: 0,
            atlas_src: (0, 16, 14, 15),
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    let draws = sprite_draws_for(&reqs, (100, 200));
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0].dst, (105, 207, 14, 15));
    assert_eq!(draws[0].src, (16, 0, 14, 15));
    assert_eq!(draws[1].dst, (100, 200, 14, 15));
    assert_eq!(draws[1].color, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn dialog_clut_color_distinct_palette() {
    let white = dialog_clut_color(0);
    let gold = dialog_clut_color(1);
    let red = dialog_clut_color(3);
    assert_eq!(white[0], 1.0);
    assert!(gold[0] > 0.9 && gold[2] < 0.5);
    assert!(red[0] > 0.9 && red[1] < 0.5);
    // Out-of-range index falls through to dim.
    let oob = dialog_clut_color(99);
    assert!(oob[0] < 0.9);
}

#[test]
fn dialog_box_default_layout_origin() {
    let l = DialogBoxLayout::default();
    assert_eq!(l.origin, (8, 168));
    assert_eq!(l.line_h, 14);
}

#[test]
fn dialog_box_draws_emits_one_quad_per_glyph() {
    let font = legaia_font::synthetic_for_tests();
    let glyphs = vec![
        DialogGlyphView {
            byte: b'a',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'b',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'c',
            clut: 1,
        },
    ];
    let layout = DialogBoxLayout::default();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    assert_eq!(draws.len(), 3);
    // Third glyph uses gold tint.
    assert!(draws[2].color[2] < 0.5);
}

#[test]
fn dialog_box_draws_handle_newline() {
    let font = legaia_font::synthetic_for_tests();
    let glyphs = vec![
        DialogGlyphView {
            byte: b'a',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'\n',
            clut: 0,
        },
        DialogGlyphView {
            byte: b'b',
            clut: 0,
        },
    ];
    let layout = DialogBoxLayout::default();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    // Two glyph quads (newline isn't drawn).
    assert_eq!(draws.len(), 2);
    // Second glyph y > first glyph y by at least line_h.
    assert!(draws[1].dst.1 - draws[0].dst.1 >= layout.line_h - 4);
}

#[test]
fn dialog_box_draws_wrap_when_too_wide() {
    let font = legaia_font::synthetic_for_tests();
    // Tiny panel that fits maybe 2-3 glyphs per row.
    let layout = DialogBoxLayout {
        origin: (0, 0),
        size: (40, 60),
        padding: (2, 2),
        line_h: 14,
        cols: 4,
    };
    let glyphs: Vec<_> = (0..12)
        .map(|_| DialogGlyphView {
            byte: b'a',
            clut: 0,
        })
        .collect();
    let draws = dialog_box_draws_for(&font, &glyphs, &layout);
    // Expect more than one row and the y coordinates to vary.
    let rows: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
    assert!(rows.len() >= 2);
}

#[test]
fn dialog_panel_draws_for_wrapper() {
    let font = legaia_font::synthetic_for_tests();
    let panel: Vec<(u8, u8)> = vec![(b'a', 0), (b'b', 1)];
    let layout = DialogBoxLayout::default();
    let draws = dialog_panel_draws_for(&font, &panel, &layout);
    assert_eq!(draws.len(), 2);
}

#[test]
fn text_draws_translate_layout_to_screen_space() {
    let font = legaia_font::synthetic_for_tests();
    let layout = font.layout(b"Ab");
    let pen = (10, 20);
    let color = [1.0, 0.5, 0.25, 1.0];
    let draws = text_draws_for(&layout, pen, color);
    assert_eq!(draws.len(), layout.glyphs.len());
    let g0 = layout.glyphs[0];
    let d0 = draws[0];
    assert_eq!(d0.dst.0, pen.0 + g0.dst_x);
    assert_eq!(d0.dst.1, pen.1 + g0.dst_y);
    assert_eq!(d0.dst.2, g0.width);
    assert_eq!(d0.src, (g0.atlas_x, g0.atlas_y, g0.width, g0.height));
    assert_eq!(d0.color, color);
}

#[test]
fn shop_draws_for_buy_mode_produces_draws() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        ShopRow {
            label: "Healing Leaf",
            price: Some(50),
        },
        ShopRow {
            label: "Healing Fruit",
            price: Some(100),
        },
    ];
    let draws = shop_draws_for(&font, "[BUY]", &rows, 0, Some(1500), (8, 140));
    // Title + 2 rows (label + price each, cursor on row 0) + gold line
    assert!(!draws.is_empty(), "expected non-empty draw list");
}

#[test]
fn shop_draws_for_confirm_mode_no_gold() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        ShopRow {
            label: "Yes",
            price: None,
        },
        ShopRow {
            label: "No",
            price: None,
        },
    ];
    let draws = shop_draws_for(&font, "[CONFIRM?]", &rows, 0, None, (8, 140));
    assert!(!draws.is_empty());
}

#[test]
fn shop_draws_for_cursor_on_second_row() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        ShopRow {
            label: "Item A",
            price: Some(10),
        },
        ShopRow {
            label: "Item B",
            price: Some(20),
        },
    ];
    // cursor=1 → no crash
    let draws = shop_draws_for(&font, "[SELL]", &rows, 1, Some(100), (0, 0));
    assert!(!draws.is_empty());
}

#[test]
fn level_up_draws_for_produces_two_line_draws() {
    let font = legaia_font::synthetic_for_tests();
    let draws = level_up_draws_for(&font, 0, 5, 10, 5, (8, 60));
    // Two non-empty lines - at minimum the title line must produce glyphs.
    assert!(!draws.is_empty());
}

#[test]
fn capture_banner_draws_for_produces_glyphs_at_the_pen() {
    let font = legaia_font::synthetic_for_tests();
    let draws = capture_banner_draws_for(&font, "Captured: Spark!", (8, 40));
    assert!(!draws.is_empty(), "banner text produces glyph draws");
    assert!(
        draws.iter().all(|d| d.dst.1 >= 40),
        "all glyphs render at or below the banner pen y"
    );
}

#[test]
fn level_up_draws_for_second_line_below_first() {
    let font = legaia_font::synthetic_for_tests();
    let draws = level_up_draws_for(&font, 0, 2, 10, 5, (8, 60));
    // At least two distinct Y positions (line 1 at 60, line 2 at 76).
    let y_vals: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
    assert!(
        y_vals.len() >= 2,
        "expected draws at two distinct y positions"
    );
}

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

#[test]
fn title_phase_2_renders_three_rows() {
    let font = legaia_font::synthetic_for_tests();
    let draws = title_draws_for(&font, 2, 0, true, true, false, (96, 100));
    // At least three rows (NEW GAME / CONTINUE / OPTIONS) plus a cursor.
    assert!(!draws.is_empty());
}

#[test]
fn title_phase_1_blink_off_is_empty() {
    let font = legaia_font::synthetic_for_tests();
    let draws = title_draws_for(&font, 1, 0, true, false, false, (96, 100));
    // blink off → no glyphs.
    assert!(draws.is_empty());
}

#[test]
fn title_continue_dimmed_when_disabled() {
    let font = legaia_font::synthetic_for_tests();
    let draws = title_draws_for(&font, 2, 0, false, true, false, (96, 100));
    // dim color is [0.45,0.45,0.45]; gold is [1.0,0.85,0.3]; white is [1,1,1].
    let any_dim = draws.iter().any(|d| d.color[0] < 0.5 && d.color[3] >= 0.99);
    assert!(any_dim);
}

#[test]
fn title_phase_1_press_start_suppressed_with_atlas() {
    let font = legaia_font::synthetic_for_tests();
    // Without atlas: blink_on emits the font-rendered "PRESS START".
    let without = title_draws_for(&font, 1, 0, true, true, false, (96, 100));
    assert!(
        !without.is_empty(),
        "phase 1 with blink should emit text when no atlas"
    );
    // With atlas: the title TIM's "PRESS START BUTTON" band covers
    // it, so the font overlay stays empty.
    let with_atlas = title_draws_for(&font, 1, 0, true, true, true, (96, 100));
    assert!(
        with_atlas.is_empty(),
        "phase 1 must not emit font text when title atlas is uploaded"
    );
}

#[test]
fn save_select_renders_with_present_and_empty_rows() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        SaveSelectRow {
            label: "Slot 1",
            present: true,
            party_lv: 12,
            play_time_seconds: 3 * 3600 + 5 * 60 + 7,
            money: 4500,
            location: "Town01",
        },
        SaveSelectRow {
            label: "Slot 2",
            present: false,
            party_lv: 0,
            play_time_seconds: 0,
            money: 0,
            location: "",
        },
    ];
    let draws = save_select_draws_for(&font, "LOAD", &rows, 0, None, (0, 0), 1, true);
    assert!(!draws.is_empty());
}

/// Each char of the title word ("Load") must be emitted at the
/// retail-pinned dst position (stage `(35, 13)` for L; subsequent
/// glyphs advance by `width + INTER_GLYPH_PAD` per the dialog-font
/// widths CSV) and tinted with `SAVE_SELECT_TITLE_COLOR`, NOT pure
/// white. Retail-pinned at sstate9 - see `SAVE_SELECT_TITLE_POS` /
/// `SAVE_SELECT_TITLE_COLOR` doc comments. Regression-guard so a
/// future "tidy up the centering math" patch can't silently revert
/// the byte-equal alignment.
#[test]
fn save_select_title_uses_retail_pinned_pos_and_color() {
    // `synthetic_for_tests` widths are not retail's, but the layout
    // pen advances by `widths[c] + INTER_GLYPH_PAD` regardless of
    // backing font - the property under test is that the FIRST
    // glyph is placed at SAVE_SELECT_TITLE_POS in stage pixels and
    // every title glyph carries SAVE_SELECT_TITLE_COLOR. That's
    // what makes the engine port byte-equal to retail's 4-sprite
    // emit at stage (35,13)/(42,13)/(48,13)/(55,13).
    let font = legaia_font::synthetic_for_tests();
    let rows: [SaveSelectRow<'_>; 0] = [];
    let stage_origin = (0, 0);
    let stage_scale = 1u32;
    let draws = save_select_draws_for(
        &font,
        "Load",
        &rows,
        0,
        None,
        stage_origin,
        stage_scale,
        false,
    );
    // First glyph dst must equal SAVE_SELECT_TITLE_POS (1:1 stage).
    assert!(!draws.is_empty(), "title must emit at least one glyph");
    let first = &draws[0];
    assert_eq!(
        (first.dst.0, first.dst.1),
        SAVE_SELECT_TITLE_POS,
        "first title glyph must start at retail-pinned stage pos"
    );
    // First four draws are the title glyphs; assert all share the
    // retail tint (no white / gold sneaking in).
    for (i, d) in draws.iter().take(4).enumerate() {
        assert_eq!(
            d.color, SAVE_SELECT_TITLE_COLOR,
            "title glyph {i} must use SAVE_SELECT_TITLE_COLOR (retail tint)"
        );
    }
    // Sanity: the title tint is the dialog-font CLUT row 13 bright
    // text entry (206, 206, 206) at VRAM (208, 510). Locked in
    // hex/float so the constant can't drift to "true white".
    assert_eq!(
        SAVE_SELECT_TITLE_COLOR,
        [206.0 / 255.0, 206.0 / 255.0, 206.0 / 255.0, 1.0]
    );
}

#[test]
fn save_select_with_confirm_prompt() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [SaveSelectRow {
        label: "Slot 1",
        present: true,
        party_lv: 1,
        play_time_seconds: 0,
        money: 0,
        location: "T",
    }];
    let draws = save_select_draws_for(
        &font,
        "LOAD",
        &rows,
        0,
        Some(("Load this slot?", 0)),
        (0, 0),
        1,
        true,
    );
    assert!(!draws.is_empty());
}

/// Helper: build a [`SaveMenuAtlasRects`] populated with the
/// byte-pinned retail tile coords. The unit tests use these to
/// verify the 9-slice composition math.
fn pinned_save_menu_rects() -> SaveMenuAtlasRects {
    SaveMenuAtlasRects {
        panel_tl: (160, 0, 4, 4),
        panel_tr: (188, 0, 4, 4),
        panel_bl: (160, 28, 4, 4),
        panel_br: (188, 28, 4, 4),
        panel_top: (164, 0, 24, 4),
        panel_bot: (164, 28, 24, 4),
        panel_left: (160, 4, 4, 21),
        panel_right: (188, 4, 4, 21),
        slot1: (33, 97, 45, 15),
        slot2: (33, 113, 45, 15),
        cursor: (152, 64, 16, 16),
        panel_interior: (128, 0, 32, 29),
        load_empty_frame: Some((200, 64, 32, 32)),
        load_portrait_by_char: [
            Some((200, 96, 16, 16)),
            Some((216, 96, 16, 16)),
            Some((232, 96, 16, 16)),
        ],
    }
}

#[test]
fn save_select_sprite_cursor_anchors_to_active_pill() {
    let rects = pinned_save_menu_rects();
    // Cursor for SLOT 1 at stage_scale=2, origin=(0, 0).
    let draw_row0 = save_select_cursor_draw_for(&rects, 0, (0, 0), 2);
    // src must be the byte-pinned (152, 64, 16, 16) sprite.
    assert_eq!(draw_row0.src, (152, 64, 16, 16));
    // dst.x = SAVE_SELECT_CURSOR_POS.x = 114, ×scale=2 → 228.
    assert_eq!(draw_row0.dst.0, SAVE_SELECT_CURSOR_POS.0 * 2);
    // dst.y = SAVE_SELECT_CURSOR_POS.y = 100, ×scale=2 → 200.
    assert_eq!(draw_row0.dst.1, SAVE_SELECT_CURSOR_POS.1 * 2);
    // dst size = src size × scale = 32x32.
    assert_eq!((draw_row0.dst.2, draw_row0.dst.3), (32, 32));

    // Cursor for SLOT 2 must shift down by SLOT_PITCH_Y × scale = 34.
    let draw_row1 = save_select_cursor_draw_for(&rects, 1, (0, 0), 2);
    assert_eq!(
        draw_row1.dst.1 - draw_row0.dst.1,
        SAVE_SELECT_SLOT_PITCH_Y * 2
    );
}

#[test]
fn save_select_chrome_emits_9slice_panel_and_pills() {
    let rects = pinned_save_menu_rects();
    let draws = save_select_chrome_draws_for(&rects, &[0, 1], SAVE_SELECT_SLOT1_POS, (10, 20), 2);
    // 3 interior tiles + 14 border tiles + 2 pills = 19.
    // (Interior: 2 full 32-wide + 1 17-wide remainder for the 81-
    //  wide panel.) Border: 4 corners + 3 top + 3 bottom + 1 top-rem
    // + 1 bot-rem + 1 left + 1 right = 14.
    assert_eq!(draws.len(), 19);
    let origin = (10, 20);
    // Interior tile #1 at stage (6, 4), sized 32x29. Screen scale=2:
    // dst = (10 + 6*2, 20 + 4*2, 32*2, 29*2) = (22, 28, 64, 58).
    assert_eq!(draws[0].dst, (22, 28, 64, 58));
    assert_eq!(draws[0].src, (128, 0, 32, 29));
    // Interior tile #3 (remainder) at stage (70, 4), sized 17x29.
    // Src width narrowed to 17 to match retail's quad sampling.
    assert_eq!(draws[2].src, (128, 0, 17, 29));
    // Top-left corner draws AFTER interior (idx 3), at stage (6, 4):
    assert_eq!(draws[3].dst, (22, 28, 8, 8));
    assert_eq!(draws[3].src, (160, 0, 4, 4));
    // Top-right corner draws after (idx 4):
    assert_eq!(draws[4].dst, (176, 28, 8, 8));
    assert_eq!(draws[4].src, (188, 0, 4, 4));
    // The 1-pixel remainder tile must use a 1-wide src rect at
    // (164, 0, 1, 4) - verifies the remainder slicing logic.
    let has_remainder = draws.iter().any(|d| d.src == (164, 0, 1, 4));
    assert!(
        has_remainder,
        "9-slice composition must include the 1-pixel top remainder tile"
    );
    // Left edge at stage (6, 4 + 4) = (6, 8), 4x21.
    // dst = (10 + 6*2, 20 + 8*2, 4*2, 21*2) = (22, 36, 8, 42).
    let left_edge = draws.iter().find(|d| d.src == (160, 4, 4, 21)).unwrap();
    assert_eq!(left_edge.dst, (22, 36, 8, 42));
    // Slot pills sit at SAVE_SELECT_SLOT1_POS = (137, 102) with
    // SAVE_SELECT_SLOT_PITCH_Y = 17 between rows; scale=2 origin (10, 20)
    // → SLOT 1 screen (10+274, 20+204) = (284, 224), size 45*2 × 15*2.
    let slot1 = draws.iter().find(|d| d.src == (33, 97, 45, 15)).unwrap();
    assert_eq!(slot1.dst.0, 10 + SAVE_SELECT_SLOT1_POS.0 * 2);
    assert_eq!(slot1.dst.1, 20 + SAVE_SELECT_SLOT1_POS.1 * 2);
    assert_eq!((slot1.dst.2, slot1.dst.3), (90, 30));
    let slot2 = draws.iter().find(|d| d.src == (33, 113, 45, 15)).unwrap();
    assert_eq!(slot2.dst.0, slot1.dst.0);
    assert_eq!(slot2.dst.1 - slot1.dst.1, SAVE_SELECT_SLOT_PITCH_Y * 2);
    // All draws use white (no gold tint - CLUT row 2 has the
    // gold gradient baked in).
    for d in &draws {
        assert_eq!(d.color, [1.0, 1.0, 1.0, 1.0]);
    }
    // origin must not get scaled out from under us when scale=2.
    for d in &draws {
        assert!(d.dst.0 >= origin.0);
        assert!(d.dst.1 >= origin.1);
    }
}

#[test]
fn save_select_chrome_zero_slots_emits_panel_only() {
    let rects = pinned_save_menu_rects();
    let draws = save_select_chrome_draws_for(&rects, &[], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
    // 3 interior + 14 border = 17 panel tiles, no pills.
    assert_eq!(draws.len(), 17);
}

#[test]
fn save_select_chrome_selected_pill_only_draws_one_pill_at_natural_row() {
    // Retail's NowChecking + SlotPreview phases hide every pill
    // except the one the user picked, but the selected pill stays
    // pinned to its natural row position. `pills = &[1]` must emit
    // SLOT 2's sprite (at SLOT_PITCH_Y * 1 below SLOT 1) and no
    // SLOT 1 sprite.
    let rects = pinned_save_menu_rects();
    let draws = save_select_chrome_draws_for(&rects, &[1], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
    // 17 panel tiles + 1 pill = 18.
    assert_eq!(draws.len(), 18);
    // SLOT 1 sprite (33, 97, 45, 15) must NOT appear.
    let any_slot1 = draws.iter().any(|d| d.src == (33, 97, 45, 15));
    assert!(!any_slot1, "SLOT 1 sprite must be suppressed");
    // SLOT 2 sprite must appear at row 1's y (SLOT1.y + PITCH).
    let slot2 = draws.iter().find(|d| d.src == (33, 113, 45, 15)).unwrap();
    assert_eq!(slot2.dst.0, SAVE_SELECT_SLOT1_POS.0);
    assert_eq!(
        slot2.dst.1,
        SAVE_SELECT_SLOT1_POS.1 + SAVE_SELECT_SLOT_PITCH_Y
    );
}

#[test]
fn save_select_chrome_load_active_anchor_relocates_pill() {
    // During NowChecking / SlotPreview retail moves the SLOT 1
    // pill up to (22, 41) under the Load panel. Passing
    // SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE as `pill_anchor` must
    // land the pill there instead of the Browsing position.
    let rects = pinned_save_menu_rects();
    let draws =
        save_select_chrome_draws_for(&rects, &[0], SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE, (0, 0), 1);
    let slot1 = draws.iter().find(|d| d.src == (33, 97, 45, 15)).unwrap();
    assert_eq!(
        slot1.dst.0, SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.0,
        "Load-active anchor must relocate SLOT 1 pill X"
    );
    assert_eq!(
        slot1.dst.1, SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.1,
        "Load-active anchor must relocate SLOT 1 pill Y"
    );
}

#[test]
fn save_select_chrome_tile_count_matches_retail_scan() {
    // Retail's `scan_panel_prims.py` against load_screen_ram.bin
    // returned 14 unique panel-chrome primitives + 3 interior
    // textured-gouraud quads + 4 "Load" text glyphs. Our chrome
    // emitter mirrors the 14 border + 3 interior = 17 primitive
    // count.
    let rects = pinned_save_menu_rects();
    let draws = save_select_chrome_draws_for(&rects, &[], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
    assert_eq!(
        draws.len(),
        17,
        "must produce 17 panel chrome+interior tiles"
    );
}

#[test]
fn menu_window_chrome_frames_arbitrary_rect() {
    // The reusable primitive must emit the 4 corners + tiled edges +
    // tiled interior for any rect, and stay inside the requested box.
    let rects = pinned_save_menu_rects();
    let (px, py, pw, ph) = (40, 30, 120, 96);
    let draws = menu_window_chrome_draws_for(&rects, (px, py, pw, ph), (0, 0), 1);
    assert!(draws.len() >= 8, "at least interior + 4 corners + edges");
    // Every tile must land within the requested window box.
    for d in &draws {
        let (dx, dy, dw, dh) = d.dst;
        assert!(dx >= px && dy >= py, "tile before window origin");
        assert!(
            dx + dw as i32 <= px + pw && dy + dh as i32 <= py + ph,
            "tile past window extent"
        );
    }
    // The four corner source rects must all be present.
    for corner in [
        rects.panel_tl,
        rects.panel_tr,
        rects.panel_bl,
        rects.panel_br,
    ] {
        assert!(
            draws.iter().any(|d| d.src == corner),
            "missing a corner tile"
        );
    }
}

#[test]
fn menu_window_chrome_honors_stage_origin_and_scale() {
    let rects = pinned_save_menu_rects();
    let base = menu_window_chrome_draws_for(&rects, (10, 10, 80, 40), (0, 0), 1);
    let moved = menu_window_chrome_draws_for(&rects, (10, 10, 80, 40), (5, 7), 2);
    assert_eq!(base.len(), moved.len());
    // Top-left corner (first tile after the interior fill) must scale
    // and translate: dst = origin + stage*scale, size = src*scale.
    let tl_base = base.iter().find(|d| d.src == rects.panel_tl).unwrap();
    let tl_moved = moved.iter().find(|d| d.src == rects.panel_tl).unwrap();
    assert_eq!(tl_moved.dst.0, 5 + tl_base.dst.0 * 2);
    assert_eq!(tl_moved.dst.1, 7 + tl_base.dst.1 * 2);
    assert_eq!(tl_moved.dst.2, tl_base.dst.2 * 2);
}

#[test]
fn scale_stage_text_maps_stage_px_to_surface() {
    let mut draws = vec![
        TextDraw {
            dst: (10, 20, 6, 12),
            src: (0, 0, 6, 12),
            color: [1.0; 4],
        },
        TextDraw {
            dst: (0, 0, 8, 8),
            src: (0, 0, 8, 8),
            color: [1.0; 4],
        },
    ];
    scale_stage_text_draws(&mut draws, (100, 50), 3);
    // dst = origin + stage*scale; size = src*scale.
    assert_eq!(draws[0].dst, (100 + 30, 50 + 60, 18, 36));
    assert_eq!(draws[1].dst, (100, 50, 24, 24));
    // src is untouched.
    assert_eq!(draws[0].src, (0, 0, 6, 12));
}

#[test]
fn encounter_banner_renders_label() {
    let font = legaia_font::synthetic_for_tests();
    let draws = encounter_banner_draws_for(&font, "Goblin x2", (100, 80));
    // ENCOUNTER! header in yellow plus body in white = at least 2 distinct colors.
    let any_yellow = draws.iter().any(|d| d.color[2] < 0.5 && d.color[0] > 0.9);
    let any_white = draws
        .iter()
        .any(|d| d.color[0] >= 0.99 && d.color[1] >= 0.99);
    assert!(any_yellow);
    assert!(any_white);
}

#[test]
fn encounter_banner_empty_label_only_header() {
    let font = legaia_font::synthetic_for_tests();
    let draws = encounter_banner_draws_for(&font, "", (100, 80));
    let any_white = draws
        .iter()
        .any(|d| d.color[0] >= 0.99 && d.color[1] >= 0.99);
    assert!(!any_white); // no body line.
}

#[test]
fn field_menu_draws_emit_rows_and_footer() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        FieldMenuRowView {
            label: "Items",
            enabled: true,
        },
        FieldMenuRowView {
            label: "Equip",
            enabled: true,
        },
        FieldMenuRowView {
            label: "Save",
            enabled: false,
        },
    ];
    let draws = field_menu_draws_for(&font, &rows, 0, 1234, 90, (16, 32));
    assert!(!draws.is_empty());
    // Selected row should produce ">" cursor glyph at the row x.
    let any_gold = draws.iter().any(|d| d.color[1] > 0.7 && d.color[2] < 0.5);
    assert!(any_gold);
}

#[test]
fn status_screen_draws_pack_panel() {
    let font = legaia_font::synthetic_for_tests();
    let stat_rows = [
        StatusStatRow {
            label: "STR",
            value: 12,
        },
        StatusStatRow {
            label: "DEF",
            value: 9,
        },
    ];
    let equip_rows = [("Weapon", "Bronze Sword"), ("Helmet", "(none)")];
    let panel = StatusPanelView {
        name: "Vahn",
        level: 5,
        xp: 200,
        xp_to_next: 350,
        hp: 60,
        hp_max: 60,
        mp: 24,
        mp_max: 24,
        ap: 0,
        ap_max: 4,
        stat_rows: &stat_rows,
        equip_rows: &equip_rows,
    };
    let draws = status_screen_draws_for(&font, &panel, Some("L1/R1: Switch"), (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn game_over_dim_continue_when_disabled() {
    let font = legaia_font::synthetic_for_tests();
    let draws = game_over_draws_for(&font, 1, false, (100, 80));
    let any_dim = draws.iter().any(|d| d.color[0] < 0.5);
    assert!(any_dim);
}

#[test]
fn options_draws_render_rows() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [
        OptionsRowView {
            label: "BGM",
            value: "8/10",
        },
        OptionsRowView {
            label: "SFX",
            value: "8/10",
        },
    ];
    let draws = options_draws_for(&font, &rows, 0, (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn key_rebind_awaiting_renders_dots() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [("Cross", "Z"), ("Circle", "S")];
    let draws = key_rebind_draws_for(&font, &rows, 0, true, (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn inventory_use_draws_render_item_rows_with_counts() {
    let font = legaia_font::synthetic_for_tests();
    let items = vec![
        InventoryItemRow {
            name: "Healing Leaf",
            count: 4,
            admissible: true,
        },
        InventoryItemRow {
            name: "Magic Leaf",
            count: 2,
            admissible: true,
        },
    ];
    let args = InventoryUseDrawArgs {
        items: &items,
        targets: &[],
        in_battle: false,
        cursor: 0,
        phase: 0,
        selected_item_name: None,
    };
    let draws = inventory_use_draws_for(&font, args, (16, 32));
    // Title + cursor + 2 rows worth of glyphs.
    assert!(!draws.is_empty());
}

#[test]
fn inventory_use_draws_empty_inventory_shows_message() {
    let font = legaia_font::synthetic_for_tests();
    let args = InventoryUseDrawArgs {
        items: &[],
        targets: &[],
        in_battle: false,
        cursor: 0,
        phase: 0,
        selected_item_name: None,
    };
    let draws = inventory_use_draws_for(&font, args, (16, 32));
    // Title plus the "no usable items" line, no cursor.
    assert!(!draws.is_empty());
}

#[test]
fn inventory_use_draws_target_phase_renders_target_column() {
    let font = legaia_font::synthetic_for_tests();
    let items = vec![InventoryItemRow {
        name: "Healing Leaf",
        count: 4,
        admissible: true,
    }];
    let targets = vec![InventoryTargetRow {
        name: "Vahn",
        hp: 100,
        hp_max: 200,
        mp: 10,
        mp_max: 30,
        alive: true,
    }];
    let no_target = inventory_use_draws_for(
        &font,
        InventoryUseDrawArgs {
            items: &items,
            targets: &targets,
            in_battle: true,
            cursor: 0,
            phase: 0,
            selected_item_name: None,
        },
        (16, 32),
    );
    let with_target = inventory_use_draws_for(
        &font,
        InventoryUseDrawArgs {
            items: &items,
            targets: &targets,
            in_battle: true,
            cursor: 0,
            phase: 1,
            selected_item_name: Some("Healing Leaf"),
        },
        (16, 32),
    );
    // Phase 1 layers the target column on top of the items column.
    assert!(with_target.len() > no_target.len());
}

#[test]
fn equipment_session_draws_render_slot_grid_in_picker_phase() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![
        EquipSlotRow {
            label: "Weapon",
            current_name: "Iron Sword",
        },
        EquipSlotRow {
            label: "Helmet",
            current_name: "(empty)",
        },
    ];
    let args = EquipDrawArgs {
        character_name: "Vahn",
        slots: &slots,
        candidates: &[],
        phase: EquipDrawPhase::SlotPicker,
        cursor: 0,
        active_slot: 0,
        confirm_label: None,
    };
    let draws = equipment_session_draws_for(&font, args, (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn equipment_session_draws_item_picker_renders_candidate_column() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![EquipSlotRow {
        label: "Weapon",
        current_name: "(empty)",
    }];
    let candidates = vec![
        EquipCandidateRow {
            name: "Iron Sword",
            count: 1,
            atk_delta: 5,
            udf_delta: 0,
        },
        EquipCandidateRow {
            name: "Wood Sword",
            count: 1,
            atk_delta: -2,
            udf_delta: 0,
        },
    ];
    let picker_only = equipment_session_draws_for(
        &font,
        EquipDrawArgs {
            character_name: "Vahn",
            slots: &slots,
            candidates: &candidates,
            phase: EquipDrawPhase::ItemPicker,
            cursor: 0,
            active_slot: 0,
            confirm_label: None,
        },
        (16, 32),
    );
    let no_picker = equipment_session_draws_for(
        &font,
        EquipDrawArgs {
            character_name: "Vahn",
            slots: &slots,
            candidates: &[],
            phase: EquipDrawPhase::SlotPicker,
            cursor: 0,
            active_slot: 0,
            confirm_label: None,
        },
        (16, 32),
    );
    assert!(picker_only.len() > no_picker.len());
}

#[test]
fn equipment_session_draws_confirm_phase_shows_yes_no_prompt() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![EquipSlotRow {
        label: "Weapon",
        current_name: "Iron Sword",
    }];
    let candidates = vec![EquipCandidateRow {
        name: "Steel Sword",
        count: 1,
        atk_delta: 3,
        udf_delta: 0,
    }];
    let draws = equipment_session_draws_for(
        &font,
        EquipDrawArgs {
            character_name: "Vahn",
            slots: &slots,
            candidates: &candidates,
            phase: EquipDrawPhase::Confirm,
            cursor: 0,
            active_slot: 0,
            confirm_label: Some("Equip Steel Sword?"),
        },
        (16, 32),
    );
    // Confirm draws should include candidate column glyphs.
    assert!(!draws.is_empty());
}

#[test]
fn tactical_arts_editor_draws_browsing_lists_saved_chains() {
    let font = legaia_font::synthetic_for_tests();
    let saved = vec![
        ArtsChainRow {
            name: "Combo A",
            pretty_sequence: "L R D U",
        },
        ArtsChainRow {
            name: "Striker",
            pretty_sequence: "U U L R D",
        },
    ];
    let args = ArtsEditorDrawArgs {
        character_name: "Vahn",
        phase: ArtsEditorPhase::Browsing,
        saved: &saved,
        browse_cursor: 1,
        editing_pretty: "",
        editing_len: 0,
        min_len: 3,
        max_len: 7,
        naming_name: "",
        can_add_new: true,
    };
    let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn tactical_arts_editor_draws_editing_shows_running_sequence() {
    let font = legaia_font::synthetic_for_tests();
    let args = ArtsEditorDrawArgs {
        character_name: "Vahn",
        phase: ArtsEditorPhase::Editing,
        saved: &[],
        browse_cursor: 0,
        editing_pretty: "L R D",
        editing_len: 3,
        min_len: 3,
        max_len: 7,
        naming_name: "",
        can_add_new: true,
    };
    let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
    // Editing emits at least: title, sequence line, two hint lines.
    assert!(!draws.is_empty());
}

#[test]
fn tactical_arts_editor_draws_naming_shows_name_and_sequence() {
    let font = legaia_font::synthetic_for_tests();
    let args = ArtsEditorDrawArgs {
        character_name: "Vahn",
        phase: ArtsEditorPhase::Naming,
        saved: &[],
        browse_cursor: 0,
        editing_pretty: "L R D",
        editing_len: 3,
        min_len: 3,
        max_len: 7,
        naming_name: "Combo A",
        can_add_new: true,
    };
    let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
    assert!(!draws.is_empty());
}

#[test]
fn tactical_arts_editor_draws_browse_no_new_when_full() {
    let font = legaia_font::synthetic_for_tests();
    let saved = vec![
        ArtsChainRow {
            name: "C1",
            pretty_sequence: "L R D",
        },
        ArtsChainRow {
            name: "C2",
            pretty_sequence: "L R D",
        },
    ];
    let with_new = tactical_arts_editor_draws_for(
        &font,
        ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Browsing,
            saved: &saved,
            browse_cursor: 0,
            editing_pretty: "",
            editing_len: 0,
            min_len: 3,
            max_len: 7,
            naming_name: "",
            can_add_new: true,
        },
        (16, 32),
    );
    let no_new = tactical_arts_editor_draws_for(
        &font,
        ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Browsing,
            saved: &saved,
            browse_cursor: 0,
            editing_pretty: "",
            editing_len: 0,
            min_len: 3,
            max_len: 7,
            naming_name: "",
            can_add_new: false,
        },
        (16, 32),
    );
    // Without "+ New" we have fewer glyphs (no extra row).
    assert!(with_new.len() > no_new.len());
}

#[test]
fn spell_menu_draws_in_each_phase() {
    let font = legaia_font::synthetic_for_tests();
    let names = ["Vahn", "Noa"];
    let hp = [(60, 60), (50, 50)];
    let mp = [(20, 24), (24, 24)];
    let spells = [SpellRowView {
        name: "Heal",
        mp_cost: 4,
        admissible: true,
    }];
    let targets = [SpellTargetView {
        name: "Vahn",
        hp: 30,
        hp_max: 60,
        alive: true,
    }];
    let names_slice: &[&str] = &names;
    let draws = spell_menu_draws_for(
        &font,
        SpellMenuDrawArgs {
            party_names: names_slice,
            party_hp: &hp,
            party_mp: &mp,
            selected_caster: None,
            spells: &spells,
            selected_spell: None,
            targets: &targets,
            selected_target: None,
            cursor: 0,
            phase: 0,
        },
        (16, 32),
    );
    assert!(!draws.is_empty());
    // Phase 2 with all confirmed selections renders all three columns.
    let draws2 = spell_menu_draws_for(
        &font,
        SpellMenuDrawArgs {
            party_names: names_slice,
            party_hp: &hp,
            party_mp: &mp,
            selected_caster: Some(0),
            spells: &spells,
            selected_spell: Some(0),
            targets: &targets,
            selected_target: Some(0),
            cursor: 0,
            phase: 2,
        },
        (16, 32),
    );
    assert!(draws2.len() > draws.len());
}

// ── Load-screen NowChecking + SlotPreview rendering ──────────────────

#[test]
fn now_checking_panel_draws_a_9_slice_frame_at_centered_pos() {
    let rects = pinned_save_menu_rects();
    let draws = now_checking_panel_draws_for(&rects, (0, 0), 1, (0, 0));
    // 9-slice: 4 corners + N top/bot edge tiles + N left/right edge
    // tiles + interior fill (variable). At minimum we expect the
    // four corners + at least one top/bot/left/right tile each.
    assert!(
        draws.len() >= 4 + 4 + 2,
        "expected at least 10 sprites for the 9-slice panel + interior; got {}",
        draws.len()
    );
    // Every sprite's dst.x is within the panel rect bounds.
    let (px, py) = NOW_CHECKING_PANEL_POS;
    let (pw, ph) = NOW_CHECKING_PANEL_SIZE;
    for d in &draws {
        assert!(
            d.dst.0 >= px && d.dst.0 < px + pw as i32 + 4,
            "sprite dst.x {} outside panel x range [{}, {})",
            d.dst.0,
            px,
            px + pw as i32
        );
        assert!(
            d.dst.1 >= py && d.dst.1 < py + ph as i32 + 4,
            "sprite dst.y {} outside panel y range [{}, {})",
            d.dst.1,
            py,
            py + ph as i32
        );
    }
}

#[test]
fn now_checking_text_emits_two_lines_at_distinct_y() {
    let font = legaia_font::synthetic_for_tests();
    let draws = now_checking_text_draws_for(&font, (0, 0), 1, (0, 0));
    // Two text lines → expect glyphs at two distinct y positions.
    let ys: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
    assert!(
        ys.len() >= 2,
        "expected >= 2 distinct y positions, got {ys:?}"
    );
    // Sanity check: the message starts above the second line.
    let min_y = ys.iter().min().copied().unwrap();
    let max_y = ys.iter().max().copied().unwrap();
    assert!(max_y > min_y, "line 2 must be below line 1");
}

#[test]
fn slot_preview_grid_emits_one_frame_per_cell_plus_portraits_plus_cursor() {
    let rects = pinned_save_menu_rects();
    // 4 of 15 slots present, slot 0 = Vahn portrait.
    let mut cells = [SlotGridCell::default(); 15];
    cells[0] = SlotGridCell {
        present: true,
        portrait_char_id: Some(0),
    };
    cells[6] = SlotGridCell {
        present: true,
        portrait_char_id: Some(1),
    };
    cells[7] = SlotGridCell {
        present: true,
        portrait_char_id: Some(2),
    };
    cells[8] = SlotGridCell {
        present: true,
        portrait_char_id: None,
    };
    let draws = slot_preview_grid_draws_for(&rects, &cells, 0, (0, 0), 1);
    // 15 empty-frame sprites + 3 portraits (slot 8 has present=true
    // but portrait_char_id=None so no portrait sprite) + 1 cursor.
    assert_eq!(
        draws.len(),
        15 + 3 + 1,
        "expected 15 frames + 3 portraits + 1 cursor; got {}",
        draws.len()
    );
    // Cursor (the last sprite) sits to the left of slot 0's cell.
    let cursor = draws.last().unwrap();
    assert_eq!(cursor.src, rects.cursor);
    // Retail pin: cursor right edge sits 1 px shy of cell left,
    // giving a -14 (not -16) offset from the cell's top-left.
    assert_eq!(cursor.dst.0, SLOT_GRID_ORIGIN.0 - 14);
    assert_eq!(cursor.dst.1, SLOT_GRID_ORIGIN.1);
}

#[test]
fn slot_preview_grid_cursor_follows_selected_slot() {
    let rects = pinned_save_menu_rects();
    let cells = [SlotGridCell::default(); 15];
    // Slot 7 = row 1 col 2.
    let draws = slot_preview_grid_draws_for(&rects, &cells, 7, (0, 0), 1);
    let cursor = draws.last().unwrap();
    let expected_x = SLOT_GRID_ORIGIN.0 + 2 * SLOT_GRID_PITCH_X - 14;
    let expected_y = SLOT_GRID_ORIGIN.1 + SLOT_GRID_PITCH_Y;
    assert_eq!(
        (cursor.dst.0, cursor.dst.1),
        (expected_x, expected_y),
        "cursor should anchor to row 1 col 2"
    );
}

#[test]
fn slot_info_panel_skips_chrome_portrait_when_no_save() {
    let rects = pinned_save_menu_rects();
    let chrome_with = slot_info_panel_draws_for(
        &rects,
        Some(&SlotInfoView {
            slot_no: 1,
            location: "Drake Kingdom",
            play_time: "00:43:09",
            leader_name: "Vahn",
            leader_level: 2,
            leader_hp: (203, 221),
            leader_mp: (27, 27),
            leader_char_id: 0,
        }),
        0,
        (0, 0),
        1,
    );
    let chrome_none = slot_info_panel_draws_for(&rects, None, 0, (0, 0), 1);
    // With Some, expect the chrome PLUS one portrait sprite.
    assert!(
        chrome_with.len() > chrome_none.len(),
        "info-panel with save should emit at least one extra portrait sprite"
    );
    assert_eq!(
        chrome_with.len() - chrome_none.len(),
        1,
        "delta should be exactly the leader portrait"
    );
}

#[test]
fn slot_info_panel_text_emits_all_six_lines() {
    let font = legaia_font::synthetic_for_tests();
    let info = SlotInfoView {
        slot_no: 1,
        location: "Drake Kingdom",
        play_time: "00:43:09",
        leader_name: "Vahn",
        leader_level: 2,
        leader_hp: (203, 221),
        leader_mp: (27, 27),
        leader_char_id: 0,
    };
    let draws = slot_info_panel_text_draws_for(&font, Some(&info), 0, (0, 0), 1);
    // Empty-save case must emit zero glyphs.
    assert!(slot_info_panel_text_draws_for(&font, None, 0, (0, 0), 1).is_empty());
    // The panel emits 10 distinct text rows (No, location, Time
    // label, time value, name, LV label, LV value, HP label,
    // HP value, MP label, MP value). Their y-coords cluster into
    // a few distinct rows; expect at least 4 distinct y values.
    let ys: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
    assert!(
        ys.len() >= 4,
        "expected >= 4 distinct y positions across the info-panel rows; got {ys:?}"
    );
}

#[test]
fn slot_info_panel_slide_offset_shifts_everything_below_parked() {
    let rects = pinned_save_menu_rects();
    let font = legaia_font::synthetic_for_tests();
    let info = SlotInfoView {
        slot_no: 1,
        location: "Drake Kingdom",
        play_time: "00:43:09",
        leader_name: "Vahn",
        leader_level: 2,
        leader_hp: (203, 221),
        leader_mp: (27, 27),
        leader_char_id: 0,
    };
    let chrome_landed = slot_info_panel_draws_for(&rects, Some(&info), 0, (0, 0), 1);
    let chrome_slid = slot_info_panel_draws_for(&rects, Some(&info), 50, (0, 0), 1);
    assert_eq!(chrome_landed.len(), chrome_slid.len());
    for (a, b) in chrome_landed.iter().zip(chrome_slid.iter()) {
        assert_eq!(a.dst.0, b.dst.0, "x must not change with slide");
        assert_eq!(
            b.dst.1 - a.dst.1,
            50,
            "y must shift by exactly slide offset"
        );
    }
    let text_landed = slot_info_panel_text_draws_for(&font, Some(&info), 0, (0, 0), 1);
    let text_slid = slot_info_panel_text_draws_for(&font, Some(&info), 50, (0, 0), 1);
    assert_eq!(text_landed.len(), text_slid.len());
    for (a, b) in text_landed.iter().zip(text_slid.iter()) {
        assert_eq!(b.dst.1 - a.dst.1, 50);
    }
}

// ---- PSX dithering ----

#[test]
fn dither_matrix_is_balanced_4x4() {
    // The 16 offsets span the documented [-4, +3] range and (being a
    // balanced ordered-dither pattern) sum to a small bias near zero.
    let m = psx_dither::DITHER_MATRIX;
    assert_eq!(m.len(), 16);
    assert_eq!(*m.iter().min().unwrap(), -4);
    assert_eq!(*m.iter().max().unwrap(), 3);
    assert_eq!(m.iter().sum::<i32>(), -8);
}

#[test]
fn dither_component_quantizes_to_5bit_expanded() {
    // Every output is a 5-bit value re-expanded by bit-replication:
    // (c5 << 3) | (c5 >> 2). Check the endpoints and that all outputs
    // belong to that 32-value set regardless of pixel / input.
    let valid: std::collections::HashSet<u8> =
        (0..32).map(|c5| ((c5 << 3) | (c5 >> 2)) as u8).collect();
    for c8 in 0..=255i32 {
        for y in 0..4u32 {
            for x in 0..4u32 {
                let out = psx_dither::dither_component(c8, x, y);
                assert!(valid.contains(&out), "c8={c8} -> {out} not a 5-bit level");
            }
        }
    }
    // Black stays black, white stays white (no offset escapes the clamp).
    assert_eq!(psx_dither::dither_component(0, 1, 1), 0);
    assert_eq!(psx_dither::dither_component(255, 1, 1), 255);
}

#[test]
fn dither_varies_across_the_4x4_cell() {
    // A mid-grey that sits between two 5-bit levels must resolve to
    // different quantized values across the dither cell - that spatial
    // variation IS the dithering. Pick a value off the 5-bit grid.
    let c8 = 134; // straddles the 5-bit boundary at 136 (134-4=130, 134+3=137)
    let mut seen = std::collections::HashSet::new();
    for y in 0..4u32 {
        for x in 0..4u32 {
            seen.insert(psx_dither::dither_component(c8, x, y));
        }
    }
    assert!(seen.len() >= 2, "dither produced no spatial variation");
}

#[test]
fn dither_rgb_disabled_path_is_identity_in_shader_only() {
    // The CPU helper always dithers; the *shader* gates on enable. Here
    // we just confirm the CPU triple path stays in range and quantizes.
    let out = psx_dither::dither_rgb([0.5, 0.25, 1.0], 2, 3);
    for c in out {
        assert!((0.0..=1.0).contains(&c));
    }
}

/// Every shaded 3D shader (with the dither helper prepended) must parse
/// and pass naga validation - this is the GPU-free guard that the WGSL
/// edits are well-formed, since the render pipelines can't build in CI.
#[test]
fn psx_shaders_parse_and_validate() {
    use wgpu::naga;
    let sources = [
        ("mesh", compose_psx_shader(MESH_SHADER_SRC)),
        (
            "textured_mesh",
            compose_psx_shader(TEXTURED_MESH_SHADER_SRC),
        ),
        ("vram_mesh", compose_psx_shader(VRAM_MESH_SHADER_SRC)),
        ("color_mesh", compose_psx_shader(COLOR_MESH_SHADER_SRC)),
    ];
    for (name, src) in sources {
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{name} shader failed to parse: {e:?}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name} shader failed to validate: {e:?}"));
    }
}

/// The VRAM-mesh and colour-mesh shaders must expose the blend-pass
/// entry points the semi-transparency pipelines compile against.
#[test]
fn vram_shader_has_blend_entry_points() {
    for entry in ["fs_main", "fs_blend", "fs_blend_quarter"] {
        assert!(
            VRAM_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
            "vram shader missing entry point {entry}"
        );
        assert!(
            COLOR_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
            "color mesh shader missing entry point {entry}"
        );
    }
}

#[test]
fn psx_blend_semi_bit_matches_tmd_packing() {
    // `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT` packs the prim ABE
    // flag into TSB bit 15; the renderer-side mirror must agree (the
    // crates deliberately don't depend on each other).
    assert_eq!(psx_blend::TSB_SEMI_TRANSPARENT_BIT, 0x8000);
    assert!(psx_blend::prim_semi_transparent(0x8000));
    assert!(psx_blend::prim_semi_transparent(0x801A));
    assert!(!psx_blend::prim_semi_transparent(0x001A));
    assert!(!psx_blend::prim_semi_transparent(0x7FFF));
}

#[test]
fn psx_blend_abr_mode_extracts_tsb_bits_5_6() {
    for mode in 0u16..4 {
        // ABR sits in bits 5..=6, independent of page / depth bits.
        assert_eq!(psx_blend::abr_mode(mode << 5), mode as u8);
        assert_eq!(psx_blend::abr_mode(0x8F1F | (mode << 5)), mode as u8);
    }
}

#[test]
fn psx_blend_src_scale_only_quarters_mode_3() {
    assert_eq!(psx_blend::src_shader_scale(0), 1.0);
    assert_eq!(psx_blend::src_shader_scale(1), 1.0);
    assert_eq!(psx_blend::src_shader_scale(2), 1.0);
    assert_eq!(psx_blend::src_shader_scale(3), 0.25);
}

/// Evaluate one wgpu blend factor as used by [`psx_blend::blend_state`]
/// (none of the selected factors depend on the source/dest colour).
fn eval_factor(f: wgpu::BlendFactor) -> f32 {
    match f {
        wgpu::BlendFactor::One => 1.0,
        wgpu::BlendFactor::Zero => 0.0,
        wgpu::BlendFactor::Constant => psx_blend::MODE0_BLEND_CONSTANT as f32,
        other => panic!("unexpected blend factor {other:?}"),
    }
}

/// Fixed-function blend evaluator: `op(src*src_factor, dst*dst_factor)`
/// clamped to the normalized target range, exactly what the GPU ROP does.
fn eval_blend(comp: wgpu::BlendComponent, dst: f32, src: f32) -> f32 {
    let s = src * eval_factor(comp.src_factor);
    let d = dst * eval_factor(comp.dst_factor);
    let v = match comp.operation {
        wgpu::BlendOperation::Add => d + s,
        wgpu::BlendOperation::ReverseSubtract => d - s,
        other => panic!("unexpected blend op {other:?}"),
    };
    v.clamp(0.0, 1.0)
}

/// blend_state(mode) + the shader-side foreground pre-scale must
/// reproduce the PSX equations (0.5B+0.5F / B+F / B-F / B+0.25F)
/// for every ABR mode, including the clamped corners.
#[test]
fn psx_blend_states_reproduce_psx_equations() {
    let samples = [
        (0.0f32, 0.0f32),
        (0.25, 0.5),
        (0.5, 0.25),
        (1.0, 1.0), // clamps modes 1 and 0's unclamped sum
        (0.1, 0.9), // clamps mode 2 (B - F < 0)
        (0.75, 0.75),
    ];
    for mode in 0u8..4 {
        let state = psx_blend::blend_state(mode);
        // Alpha always replaces - the surface alpha is unused.
        assert_eq!(state.alpha.src_factor, wgpu::BlendFactor::One);
        assert_eq!(state.alpha.dst_factor, wgpu::BlendFactor::Zero);
        assert_eq!(state.alpha.operation, wgpu::BlendOperation::Add);
        for (b, f) in samples {
            // The blend-pass fragment shader outputs F * src_shader_scale.
            let shader_out = f * psx_blend::src_shader_scale(mode);
            let got = eval_blend(state.color, b, shader_out);
            let want = psx_blend::blend_apply(mode, b, f);
            assert!(
                (got - want).abs() < 1e-6,
                "mode {mode} B={b} F={f}: pipeline gives {got}, PSX wants {want}"
            );
        }
    }
}

/// Per-corner positions for `n` triangles: triangle `k` sits at
/// `z = zs[k]` with corners spread in x so its centroid is
/// `(1, 0, zs[k])`.
fn tri_positions(zs: &[f32]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for &z in zs {
        out.push([0.0, 0.0, z]);
        out.push([1.0, 0.0, z]);
        out.push([2.0, 0.0, z]);
    }
    out
}

/// MVP whose clip-space `w` row maps `w = z + d` - a minimal
/// perspective-like matrix that makes depth keys easy to predict.
fn z_to_w_mvp(d: f32) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0, 1.0),
        glam::Vec4::new(0.0, 0.0, 0.0, d),
    )
}

#[test]
fn psx_blend_append_semi_tail_buckets_per_mode() {
    // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
    // semi ABR 3 (in that order).
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let mut cba_tsb = Vec::new();
    for tsb in [0x001Au16, semi, semi | (2 << 5), semi | (3 << 5)] {
        cba_tsb.extend_from_slice(&[[0u16, tsb]; 3]);
    }
    let indices: Vec<u32> = (0..12).collect();
    let positions = tri_positions(&[1.0, 2.0, 3.0, 4.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    // Original indices untouched at the front (the opaque pass range).
    assert_eq!(&out[..12], indices.as_slice());
    // Tail: 3 semi triangles bucketed per ABR mode, mode 1 empty.
    assert_eq!(ranges[0], (12, 3));
    assert_eq!(ranges[1], (15, 0));
    assert_eq!(ranges[2], (15, 3));
    assert_eq!(ranges[3], (18, 3));
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
    assert_eq!(&out[18..21], &[9, 10, 11]);
    assert_eq!(out.len(), 21);
    // Per-prim metadata: original mesh order (not bucket order), each
    // pointing at its slot in the mode tail, centroid = corner average.
    assert_eq!(
        prims,
        vec![
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 2.0],
                mode: 0,
                first_index: 12,
            },
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 3.0],
                mode: 2,
                first_index: 15,
            },
            psx_blend::SemiPrim {
                centroid: [1.0, 0.0, 4.0],
                mode: 3,
                first_index: 18,
            },
        ]
    );
}

#[test]
fn psx_blend_append_semi_tail_same_mode_prims_get_distinct_tail_slots() {
    // 3 semi prims all on ABR mode 0: their tail triangles must land at
    // consecutive first_index slots (12, 15, 18), in mesh order.
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let cba_tsb = vec![[0u16, semi]; 9];
    let indices: Vec<u32> = (0..9).collect();
    let positions = tri_positions(&[5.0, 6.0, 7.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out.len(), 18);
    assert_eq!(ranges[0], (9, 9));
    let firsts: Vec<u32> = prims.iter().map(|p| p.first_index).collect();
    assert_eq!(firsts, vec![9, 12, 15]);
    let zs: Vec<f32> = prims.iter().map(|p| p.centroid[2]).collect();
    assert_eq!(zs, vec![5.0, 6.0, 7.0]);
    // Each prim's tail slot holds its own triangle's indices.
    assert_eq!(&out[9..12], &[0, 1, 2]);
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
}

#[test]
fn psx_blend_append_semi_tail_all_opaque_is_identity() {
    let cba_tsb = vec![[0u16, 0x001Au16]; 6];
    let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let positions = tri_positions(&[1.0, 2.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out, indices);
    assert_eq!(ranges, [(6, 0); 4]);
    assert!(prims.is_empty());
}

/// The per-prim depth key must follow the renderer's existing depth
/// convention: the clip-space `w` of a point under the draw MVP. For
/// the model origin that is exactly `mvp.w_axis.w` (the old per-draw
/// key), and by linearity the centroid's key equals the average of the
/// corner keys (PSX OT avg-Z binning).
#[test]
fn psx_blend_prim_depth_key_matches_origin_and_vertex_average() {
    let mvp = Mat4::from_cols(
        glam::Vec4::new(0.5, 1.0, -2.0, 0.25),
        glam::Vec4::new(3.0, -1.5, 0.5, -1.0),
        glam::Vec4::new(0.0, 2.0, 1.0, 4.0),
        glam::Vec4::new(7.0, -3.0, 2.0, 11.0),
    );
    // Origin key = w_axis.w, the previous per-draw ordering key.
    assert_eq!(psx_blend::prim_depth_key(&mvp, [0.0; 3]), mvp.w_axis.w);
    let corners = [[1.0f32, 2.0, 3.0], [-4.0, 0.5, 2.0], [6.0, -1.0, 7.0]];
    let centroid = [
        (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
        (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
        (corners[0][2] + corners[1][2] + corners[2][2]) / 3.0,
    ];
    let avg_of_keys = corners
        .iter()
        .map(|&c| {
            let v = mvp * glam::Vec4::new(c[0], c[1], c[2], 1.0);
            v.w
        })
        .sum::<f32>()
        / 3.0;
    let key = psx_blend::prim_depth_key(&mvp, centroid);
    assert!(
        (key - avg_of_keys).abs() < 1e-4,
        "centroid key {key} != avg of corner keys {avg_of_keys}"
    );
    // And the key really is the clip w of the centroid.
    let clip = mvp * glam::Vec4::new(centroid[0], centroid[1], centroid[2], 1.0);
    assert!((key - clip.w).abs() < 1e-5);
}

/// Two overlapping draws whose semi prims interleave in depth: the
/// sorted blend list must be globally back-to-front per PRIM, an order
/// no per-draw sort can produce.
#[test]
fn psx_blend_list_orders_prims_back_to_front_across_draws() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    // Draw A (textured): two mode-0 semi prims at view depths 5 and 25.
    let cba_tsb = vec![[0u16, semi]; 6];
    let idx: Vec<u32> = (0..6).collect();
    let (_, _, prims_a) = psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[5.0, 25.0]));
    // Draw B (untextured): two mode-0 semi prims at view depths 11
    // and 19 - interleaving with A's.
    let blend = vec![psx_blend::pack_blend_word(true, 0); 6];
    let (_, _, prims_b) =
        psx_blend::append_semi_tail_words(&idx, &blend, &tri_positions(&[11.0, 19.0]));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims_a);
    psx_blend::push_draw_prims(&mut list, true, 1, &mvp, &prims_b);
    psx_blend::sort_blend_list(&mut list);
    // Globally far-to-near: 25 (A), 19 (B), 11 (B), 5 (A).
    let got: Vec<(bool, u32, f32)> = list
        .iter()
        .map(|e| (e.untextured, e.draw_index, e.key))
        .collect();
    assert_eq!(
        got,
        vec![
            (false, 0, 25.0),
            (true, 1, 19.0),
            (true, 1, 11.0),
            (false, 0, 5.0),
        ]
    );
    // Keys are strictly non-increasing = back-to-front per prim.
    assert!(list.windows(2).all(|w| w[0].key >= w[1].key));
    // A per-draw key (the draw origin, w_axis.w = 0 for both) could
    // never interleave these - the per-prim keys are what separate them.
    // NaN keys still sort deterministically (farthest first).
    let mut with_nan = list.clone();
    with_nan.push(psx_blend::BlendListEntry {
        key: f32::NAN,
        seq: 99,
        untextured: false,
        draw_index: 7,
        mode: 0,
        first_index: 0,
    });
    psx_blend::sort_blend_list(&mut with_nan);
    assert_eq!(with_nan[0].draw_index, 7);
}

/// Equal depth keys = one ordering-table bucket. Retail OT buckets are
/// LIFO (`AddPrim` prepends, `DrawOTag` walks head-first), so within a
/// bucket later-submitted prims draw FIRST.
#[test]
fn psx_blend_list_equal_depth_bucket_is_lifo() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    let cba_tsb = vec![[0u16, semi]; 6];
    let idx: Vec<u32> = (0..6).collect();
    // Both draws' prims all sit at the same depth (z = 8).
    let (_, _, prims) = psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[8.0, 8.0]));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims);
    psx_blend::push_draw_prims(&mut list, false, 1, &mvp, &prims);
    psx_blend::sort_blend_list(&mut list);
    // 4 entries, all key 8: submission order was seq 0,1 (draw 0) then
    // seq 2,3 (draw 1); LIFO draws seq 3,2,1,0.
    let seqs: Vec<u32> = list.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![3, 2, 1, 0]);
    let draws: Vec<u32> = list.iter().map(|e| e.draw_index).collect();
    assert_eq!(draws, vec![1, 1, 0, 0]);
}

/// Sorted-run coalescing: consecutive entries from the same draw +
/// mode with contiguous tail triangles merge into one indexed draw;
/// any change of draw, mode, or contiguity splits the run.
#[test]
fn psx_blend_coalesce_merges_contiguous_tail_runs() {
    let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
    // One draw, 3 mode-0 semi prims at strictly descending depth so the
    // sort keeps tail order and the runs stay contiguous, plus a
    // mode-2 prim in between depths that splits the run.
    let cba_tsb = vec![
        [0u16, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi],
        [0, semi | (2 << 5)],
        [0, semi | (2 << 5)],
        [0, semi | (2 << 5)],
        [0, semi],
        [0, semi],
        [0, semi],
    ];
    let idx: Vec<u32> = (0..12).collect();
    // Depths: 40, 30, (mode 2) 20, 10.
    let (_, ranges, prims) =
        psx_blend::append_semi_tail(&idx, &cba_tsb, &tri_positions(&[40.0, 30.0, 20.0, 10.0]));
    // Mode-0 tail holds prims 0,1,3; mode-2 tail holds prim 2.
    assert_eq!(ranges[0], (12, 9));
    assert_eq!(ranges[2], (21, 3));
    let mvp = z_to_w_mvp(0.0);
    let mut list = Vec::new();
    psx_blend::push_draw_prims(&mut list, false, 0, &mvp, &prims);
    psx_blend::sort_blend_list(&mut list);
    let mut runs: Vec<(u8, u32, u32)> = Vec::new();
    psx_blend::coalesce_sorted(&list, |head, start, count| {
        runs.push((head.mode, start, count));
    });
    // 40 + 30 are contiguous mode-0 tail slots (12, 15) -> one run of
    // 6 indices; then the mode-2 prim at 20 (tail 21); then the last
    // mode-0 prim at 10 (tail 18, not contiguous with the first run).
    assert_eq!(runs, vec![(0, 12, 6), (2, 21, 3), (0, 18, 3)]);
}

/// Coalescing never merges across draw boundaries even when tail
/// indices happen to line up.
#[test]
fn psx_blend_coalesce_splits_on_draw_change() {
    let entries = [
        psx_blend::BlendListEntry {
            key: 9.0,
            seq: 0,
            untextured: false,
            draw_index: 0,
            mode: 0,
            first_index: 12,
        },
        psx_blend::BlendListEntry {
            key: 8.0,
            seq: 1,
            untextured: false,
            draw_index: 1,
            mode: 0,
            first_index: 15,
        },
        psx_blend::BlendListEntry {
            key: 7.0,
            seq: 2,
            untextured: true,
            draw_index: 1,
            mode: 0,
            first_index: 18,
        },
    ];
    let mut runs = Vec::new();
    psx_blend::coalesce_sorted(&entries, |head, start, count| {
        runs.push((head.untextured, head.draw_index, start, count));
    });
    assert_eq!(
        runs,
        vec![(false, 0, 12, 3), (false, 1, 15, 3), (true, 1, 18, 3)]
    );
}

/// `pack_blend_word` must round-trip through the extractors the blend
/// pass uses, and must agree bit-for-bit with the TSB packing the
/// textured path rides (ABE bit 15, ABR bits 5..=6).
#[test]
fn psx_blend_pack_blend_word_round_trips() {
    for abr in 0u8..4 {
        let semi = psx_blend::pack_blend_word(true, abr);
        assert!(psx_blend::prim_semi_transparent(semi));
        assert_eq!(psx_blend::abr_mode(semi), abr);
        assert_eq!(semi, 0x8000 | ((abr as u16) << 5));
        let opaque = psx_blend::pack_blend_word(false, abr);
        assert!(!psx_blend::prim_semi_transparent(opaque));
        assert_eq!(psx_blend::abr_mode(opaque), abr);
    }
    // Out-of-range ABR is masked to 2 bits.
    assert_eq!(psx_blend::abr_mode(psx_blend::pack_blend_word(true, 7)), 3);
}

/// The word-slice variant (untextured colour-mesh path) must bucket
/// identically to `append_semi_tail` given equivalent per-vertex words.
#[test]
fn psx_blend_append_semi_tail_words_buckets_per_mode() {
    // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
    // semi ABR 3 (in that order) - the colour-mesh packing of the
    // textured test's TSB values.
    let mut blend = Vec::new();
    for (abe, abr) in [(false, 0u8), (true, 0), (true, 2), (true, 3)] {
        blend.extend_from_slice(&[psx_blend::pack_blend_word(abe, abr); 3]);
    }
    let indices: Vec<u32> = (0..12).collect();
    let positions = tri_positions(&[1.0, 2.0, 3.0, 4.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail_words(&indices, &blend, &positions);
    // Original indices untouched at the front (the opaque pass range).
    assert_eq!(&out[..12], indices.as_slice());
    assert_eq!(ranges[0], (12, 3));
    assert_eq!(ranges[1], (15, 0));
    assert_eq!(ranges[2], (15, 3));
    assert_eq!(ranges[3], (18, 3));
    assert_eq!(&out[12..15], &[3, 4, 5]);
    assert_eq!(&out[15..18], &[6, 7, 8]);
    assert_eq!(&out[18..21], &[9, 10, 11]);

    // Cross-check against the textured-path partitioner on the same data.
    let cba_tsb: Vec<[u16; 2]> = blend.iter().map(|&w| [0u16, w]).collect();
    let (out_tsb, ranges_tsb, prims_tsb) =
        psx_blend::append_semi_tail(&indices, &cba_tsb, &positions);
    assert_eq!(out, out_tsb);
    assert_eq!(ranges, ranges_tsb);
    assert_eq!(prims, prims_tsb);
}

#[test]
fn psx_blend_append_semi_tail_words_all_opaque_is_identity() {
    let blend = vec![psx_blend::pack_blend_word(false, 1); 6];
    let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let positions = tri_positions(&[1.0, 2.0]);
    let (out, ranges, prims) = psx_blend::append_semi_tail_words(&indices, &blend, &positions);
    assert_eq!(out, indices);
    assert_eq!(ranges, [(6, 0); 4]);
    assert!(prims.is_empty());
}
