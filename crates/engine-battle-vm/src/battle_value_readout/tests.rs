use super::*;

/// `battle_vahn_tri_somersault_super`: `242` on its drawn frame (timer
/// `0x70`) is three 3x3 cells at `x = 183, 187, 191`, row 133.
#[test]
fn a_fresh_popup_is_three_tiny_cells() {
    let cells = popup_cells(242, (187, 133, 190, 136), 0x70);
    let xs: Vec<i32> = cells.iter().map(|c| c.x).collect();
    assert_eq!(xs, vec![183, 187, 191]);
    assert!(cells.iter().all(|c| c.w == 4 && c.y == 133));
    assert_eq!(
        cells.iter().map(|c| c.digit).collect::<Vec<_>>(),
        vec![2, 4, 2]
    );
}

/// `battle_melee_hit_spark`'s `15`: a projected square wider than 24 px
/// is cut to a 22-px span (pitch 23) and pinned to the row-32 ceiling.
#[test]
fn a_grown_popup_caps_its_width_and_rests_on_row_32() {
    let cells = popup_cells(15, (113, 20, 139, 46), 0x120);
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].w, 23);
    assert_eq!(cells[1].x - cells[0].x, 23);
    assert!(cells.iter().all(|c| c.y == RESTING_TOP_Y));
}

/// 37 frames of life; the anchor rises 24 units a frame as the square
/// grows 8.
#[test]
fn the_popup_lives_37_frames_and_rises_as_it_grows() {
    assert!(popup_live(0));
    assert!(popup_live(36));
    assert!(!popup_live(37));
    assert_eq!(popup_anchor([0, -500, 0], 0), ([0, -250, 0], 8));
    assert_eq!(popup_anchor([0, -500, 0], 7), ([0, -250 - 168, 0], 64));
}

#[test]
fn combo_cluster_reproduces_the_steal_banner_packets() {
    // `player_steal_skeleton_banner`: `5 HIT` / `TOTAL 72` at rest.
    let c = combo_cluster(ComboStyle::HitTotal, 5, 72, 0);
    let hit = c.labels.iter().find(|l| l.word == "HIT").unwrap();
    let total = c.labels.iter().find(|l| l.word == "TOTAL").unwrap();
    assert_eq!((hit.x, hit.y, hit.size()), (272, 152, (32, 16)));
    assert_eq!((total.x, total.y, total.size()), (216, 170, (48, 16)));
    // The count: one 24x24 cell whose right edge is 264.
    let count: Vec<_> = c.cells.iter().filter(|k| k.y == COMBO_COUNT_Y).collect();
    assert_eq!(count.len(), 1);
    assert_eq!((count[0].x, count[0].w, count[0].digit), (240, 24, 5));
    // The value: `7` at 272, `2` at 288, both 16x16.
    let value: Vec<_> = c.cells.iter().filter(|k| k.y == COMBO_VALUE_Y).collect();
    assert_eq!(
        value
            .iter()
            .map(|k| (k.x, k.w, k.digit))
            .collect::<Vec<_>>(),
        vec![(272, 16, 7), (288, 16, 2)]
    );
}

#[test]
fn combo_cluster_reproduces_the_tail_fire_packets() {
    // `battle_gimard_tail_fire_a`: `DAMAGE 16`.
    let c = combo_cluster(ComboStyle::Damage, 0, 16, 0);
    assert_eq!(c.labels.len(), 1);
    let d = &c.labels[0];
    assert_eq!((d.x, d.y, d.size(), d.word), (208, 170, (56, 16), "DAMAGE"));
    assert_eq!(
        c.cells
            .iter()
            .map(|k| (k.x, k.y, k.w, k.digit))
            .collect::<Vec<_>>(),
        vec![(272, 168, 16, 1), (288, 168, 16, 6)]
    );
}

#[test]
fn combo_cluster_slides_in_from_the_right() {
    // `battle_melee_hit_spark`: the anchor's glide record reads total
    // 16, elapsed 12, start x 328, target x 168, and its handle sits at
    // x = 208 - every packet of the cluster is 40 px right of rest.
    assert_eq!(combo_slide(12), 40);
    assert_eq!(combo_slide(0), COMBO_SLIDE_IN_X);
    assert_eq!(COMBO_SLIDE_IN_X, 160);
    assert_eq!(combo_slide(COMBO_SLIDE_FRAMES), 0);
    assert_eq!(combo_slide(200), 0);
    let rest = combo_cluster(ComboStyle::HitTotal, 1, 15, 0);
    let sliding = combo_cluster(ComboStyle::HitTotal, 1, 15, combo_slide(12));
    for (a, b) in rest.labels.iter().zip(&sliding.labels) {
        assert_eq!(b.x - a.x, 40);
        assert_eq!(a.y, b.y);
    }
    for (a, b) in rest.cells.iter().zip(&sliding.cells) {
        assert_eq!(b.x - a.x, 40);
    }
}

#[test]
fn combo_glide_is_linear_over_the_pinned_frame_count() {
    // FUN_801D9BBC: start + (target - start) * elapsed / total.
    for age in 0..COMBO_SLIDE_FRAMES {
        let expect = 160 - 160 * i32::from(age) / 16;
        assert_eq!(combo_slide(age), expect, "age {age}");
    }
}

#[test]
fn combo_label_moves_left_with_a_longer_value() {
    // `shiny_refactor_gimard_levelup`: `DAMAGE 233`, label quad at
    // `x = 192..247` in the retail packet stream, value at 256.
    let c = combo_cluster(ComboStyle::Damage, 0, 233, 0);
    assert_eq!(c.labels[0].x, 192);
    assert_eq!(c.cells[0].x, 256);
    // The label keeps the same gap to the value whatever its length.
    for total in [16, 233, 4096] {
        let c = combo_cluster(ComboStyle::Damage, 0, total, 0);
        assert_eq!(c.cells[0].x - c.labels[0].x, 64, "{total}");
        let c = combo_cluster(ComboStyle::HitTotal, 1, total, 0);
        let t = c.labels.iter().find(|l| l.word == "TOTAL").unwrap();
        let first = c.cells.iter().find(|k| k.y == COMBO_VALUE_Y).unwrap();
        assert_eq!(first.x - t.x, 56, "{total}");
    }
}

#[test]
fn combo_value_cells_grow_leftward_from_the_pinned_right_edge() {
    let c = combo_cluster(ComboStyle::Damage, 0, 1234, 0);
    let xs: Vec<i32> = c.cells.iter().map(|k| k.x).collect();
    assert_eq!(xs, vec![240, 256, 272, 288]);
    // The cells sample the 24-texel strip at the digit's own column.
    assert_eq!(c.cells[0].u, digit_cell_u(1));
    assert_eq!(c.cells[0].cell, DIGIT_CELL);
}

#[test]
fn teardown_needs_the_flag_and_a_nonzero_value() {
    assert!(teardown_runs(1, [0, 0, 7, 0]));
    assert!(!teardown_runs(0, [1, 1, 1, 1]), "flag down");
    assert!(!teardown_runs(1, [0; 4]), "window empty");
}

#[test]
fn teardown_pairs_each_id_with_the_one_four_below() {
    assert_eq!(teardown_pair(0x50), (0x50, 0x4C));
    assert_eq!(teardown_pair(0x04), (0x04, 0));
    // Below four the second id goes negative rather than wrapping.
    assert_eq!(teardown_pair(0x02), (0x02, -2));
    assert_eq!(teardown_pair(0), (0, -4));
}

#[test]
fn teardown_row_is_chosen_by_the_count() {
    assert_eq!(teardown_row_offset(0, 0), None);
    assert_eq!(teardown_row_offset(1, 0), Some(0));
    assert_eq!(teardown_row_offset(1, 3), Some(3));
    assert_eq!(teardown_row_offset(2, 0), Some(4));
    assert_eq!(teardown_row_offset(3, 2), Some(10));
}

#[test]
fn widget_lookup_is_a_byte_record_then_a_word_table() {
    assert_eq!(slot_widget_index_offset(0), 0x11B6);
    assert_eq!(slot_widget_index_offset(1), 0x11B6 + 0x0C);
    assert_eq!(slot_widget_index_offset(4), 0x11B6 + 0x30);
    assert_eq!(widget_offset(0), 0x1074);
    assert_eq!(widget_offset(3), 0x1074 + 0x0C);
}

#[test]
fn reciprocals_agree_with_plain_division_across_the_value_range() {
    for v in 0u32..=0xFFFF {
        assert_eq!(reciprocal::div10(v), v / 10, "div10 of {v}");
        assert_eq!(reciprocal::div10000(v), v / 10000, "div10000 of {v}");
    }
}

#[test]
fn digits_drop_leading_zeros_but_keep_a_single_zero() {
    assert_eq!(decimal_digits(0), vec![0]);
    assert_eq!(decimal_digits(7), vec![7]);
    assert_eq!(decimal_digits(70), vec![7, 0]);
    assert_eq!(decimal_digits(1234), vec![1, 2, 3, 4]);
    assert_eq!(decimal_digits(65535), vec![6, 5, 5, 3, 5]);
    assert_eq!(decimal_digits(10000), vec![1, 0, 0, 0, 0]);
}

#[test]
fn digit_count_never_exceeds_five() {
    for v in [0u16, 1, 9, 10, 99, 100, 9999, 10000, 0xFFFF] {
        let d = decimal_digits(v);
        assert!((1..=5).contains(&d.len()), "value {v} -> {d:?}");
        // Reassembling the digits recovers the value.
        let back = d.iter().fold(0u32, |acc, &x| acc * 10 + x as u32);
        assert_eq!(back, v as u32);
    }
}

#[test]
fn label_quad_is_a_one_to_one_blit_of_a_fifty_five_pixel_strip() {
    let q = label_quad(100, 80);
    assert_eq!(q.tag, OT_TAG);
    assert_eq!(q.code_colour, CODE_COLOUR);
    assert_eq!(q.xy[0], (110, 66));
    assert_eq!(q.xy[3], (165, 81));
    // Screen extent equals texel extent in both axes.
    assert_eq!(q.xy[1].0 - q.xy[0].0, (q.uv[1].0 - q.uv[0].0) as i16);
    assert_eq!(q.xy[2].1 - q.xy[0].1, (q.uv[2].1 - q.uv[0].1) as i16);
    assert_eq!(q.clut, GLYPH_CLUT);
    assert_eq!(q.tpage, GLYPH_TPAGE);
}

#[test]
fn label_quad_corners_share_edges() {
    let q = label_quad(0, 32);
    assert_eq!(q.xy[0].1, q.xy[1].1, "top edge");
    assert_eq!(q.xy[2].1, q.xy[3].1, "bottom edge");
    assert_eq!(q.xy[0].0, q.xy[2].0, "left edge");
    assert_eq!(q.xy[1].0, q.xy[3].0, "right edge");
}

#[test]
fn digit_strip_starts_at_one_and_ends_at_zero() {
    // Read off the decoded sheet: the row is "1234567890".
    assert_eq!(digit_cell_u(1), 0);
    assert_eq!(digit_cell_u(2), 24);
    assert_eq!(digit_cell_u(9), 192);
    assert_eq!(digit_cell_u(0), 216);
    // Every cell is inside the 256-texel page row.
    for d in 0u8..=9 {
        assert!(u16::from(digit_cell_u(d)) + u16::from(DIGIT_CELL) <= 256);
    }
}

#[test]
fn label_rects_are_the_three_the_sheet_carries() {
    // `label_quad`'s own rect is the DAMAGE label.
    let q = label_quad(0, 0);
    assert_eq!(
        (q.uv[0].0, q.uv[0].1, q.uv[3].0, q.uv[3].1),
        LABEL_DAMAGE,
        "label_quad draws DAMAGE"
    );
    // HIT and TOTAL share the row below it and do not overlap.
    assert_eq!(LABEL_HIT.1, LABEL_TOTAL.1);
    assert!(LABEL_HIT.2 < LABEL_TOTAL.0);
    // Measured drawn widths: 55 / 31 / 47 px for inclusive corner spans.
    assert_eq!(LABEL_DAMAGE.2 - LABEL_DAMAGE.0, 55);
    assert_eq!(LABEL_HIT.2 - LABEL_HIT.0, 31);
    assert_eq!(LABEL_TOTAL.2 - LABEL_TOTAL.0, 47);
}

#[test]
fn the_pop_grows_to_a_one_to_one_cell_and_holds() {
    assert_eq!(pop_cell_px(0), POP_START_CELL);
    assert_eq!(pop_cell_px(POP_FRAMES), u32::from(DIGIT_CELL));
    assert_eq!(pop_cell_px(POP_FRAMES * 4), u32::from(DIGIT_CELL));
    // Monotonic.
    for age in 0..POP_FRAMES {
        assert!(pop_cell_px(age) <= pop_cell_px(age + 1));
    }
}

#[test]
fn the_run_rises_to_the_resting_row_and_stops() {
    assert_eq!(pop_top_y(49, 0), 49);
    assert_eq!(pop_top_y(49, POP_FRAMES), RESTING_TOP_Y);
    assert_eq!(pop_top_y(49, 240), RESTING_TOP_Y);
    // A seat already above the row is left alone rather than pushed down.
    assert_eq!(pop_top_y(10, 0), 10);
    assert_eq!(pop_top_y(10, POP_FRAMES), 10);
}

#[test]
fn the_run_grows_about_a_fixed_centre() {
    // `battle_melee_hit_spark`: `15` at cells 18 and 22 px, centre 136.5
    // in both arenas (118..155 then 114..159).
    let narrow = value_cells(15, 136, 49, 0);
    let wide = value_cells(15, 136, 49, POP_FRAMES);
    let span = |c: &[ValueCell]| {
        let l = c.first().unwrap().x;
        let r = c.last().unwrap().x + c.last().unwrap().w as i32;
        (l + r) / 2
    };
    assert_eq!(
        span(&narrow),
        span(&wide),
        "centre holds while the cells grow"
    );
    assert!(wide[0].w > narrow[0].w);
    // Pitch IS the drawn width: the far corner is inclusive, so cells laid
    // this far apart abut without a gap column.
    assert_eq!(wide[1].x - wide[0].x, wide[0].w as i32 + DIGIT_GAP);
}

#[test]
fn value_cells_address_the_right_digits() {
    let cells = value_cells(15, 100, 40, POP_FRAMES);
    assert_eq!(cells.len(), 2);
    assert_eq!((cells[0].digit, cells[0].u), (1, 0));
    assert_eq!((cells[1].digit, cells[1].u), (5, 96));
    assert!(
        cells
            .iter()
            .all(|c| c.v == DIGIT_ROW_V && c.cell == DIGIT_CELL)
    );
    // Zero still draws one cell rather than none.
    assert_eq!(value_cells(0, 0, 0, 0).len(), 1);
}

#[test]
fn prim_size_matches_a_ten_word_quad() {
    assert_eq!(PRIM_BYTES, 10 * 4);
    // The tag's length field counts the words after it.
    assert_eq!(OT_TAG >> 24, 9);
}
