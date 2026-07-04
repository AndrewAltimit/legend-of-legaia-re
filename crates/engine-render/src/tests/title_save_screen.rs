use super::*;

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
        panel_filigree: (0, 200, 32, 29),
        label_lv: (40, 232, 16, 12),
        label_hp: (60, 232, 16, 12),
        label_mp: (80, 232, 16, 12),
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
