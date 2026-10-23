use super::*;

// The rects are the disc records for these ids (menu-overlay window
// table, `legaia_game_tables::menu_windows`); pinning them here keeps the
// pen arithmetic checkable without a disc.
const W5: PainterRect = PainterRect {
    x: 58,
    y: 82,
    w: 204,
    h: 56,
};
const W6: PainterRect = PainterRect {
    x: 50,
    y: 68,
    w: 216,
    h: 82,
};
const W32: PainterRect = PainterRect {
    x: 202,
    y: 20,
    w: 104,
    h: 10,
};
const W46: PainterRect = PainterRect {
    x: 16,
    y: 84,
    w: 104,
    h: 42,
};

#[test]
fn record_title_text_starts_past_the_prefix() {
    assert_eq!(title_record_text_offset(0), 3);
    assert_eq!(title_record_text_offset(4), 7);
}

// -- FUN_801D603C / FUN_801D61B0 -----------------------------------

#[test]
fn suppress_bit_beats_every_other_arm() {
    let f = ChoiceFlags(0x4000 | 0x2000 | 0x1000);
    assert_eq!(f.marker_variant(0), None);
    assert_eq!(f.marker_variant(1), None);
}

#[test]
fn mid_change_arm_marks_both_rows_and_ignores_the_index() {
    // Bit 0x1000 clear -> variant 4; the low bits are not compared.
    let f = ChoiceFlags(0x2000 | 7);
    assert_eq!(f.marker_variant(0), Some(4));
    assert_eq!(f.marker_variant(1), Some(4));
    // Bit 0x1000 set -> variant 0.
    let f = ChoiceFlags(0x2000 | 0x1000);
    assert_eq!(f.marker_variant(0), Some(0));
    assert_eq!(f.marker_variant(1), Some(0));
}

#[test]
fn settled_arm_marks_only_the_selected_row() {
    let f = ChoiceFlags(1);
    assert_eq!(f.marker_variant(0), None);
    assert_eq!(f.marker_variant(1), Some(1));
    // Bit 0x1000 flips the settled variant to 0 - and it is part of
    // the index comparison's complement, not of the index itself.
    let f = ChoiceFlags(0x1000);
    assert_eq!(f.marker_variant(0), Some(0));
    assert_eq!(f.marker_variant(1), None);
}

#[test]
fn the_two_choice_windows_differ_only_in_marker_column_and_first_row() {
    let flags = ChoiceFlags(0);
    let font = legaia_font::Font::placeholder();
    let (_, s46) = choice_panel_draws_for(&font, W46, "H", ["A", "B"], flags);
    assert_eq!((s46[0].x, s46[0].y), (W46.x + 0x30, W46.y + 0x10));

    let (_, s5) = two_line_choice_panel_draws_for(&font, W5, ["H", "I"], ["A", "B"], flags);
    assert_eq!(
        (s5[0].x, s5[0].y),
        (W5.x + 0x3C, W5.y + PAINTER_ROW_PITCH + 0x10)
    );
}

// -- FUN_801D6360 --------------------------------------------------

#[test]
fn label_list_cursor_is_the_only_extent_anchored_pen() {
    let font = legaia_font::Font::placeholder();
    let (_, cur) = label_list_draws_for(&font, W6, &["a", "b", "c", "d", "e", "f"]);
    assert_eq!(cur.x, W6.x + W6.w - 0x10);
    assert_eq!(cur.y, W6.y + W6.h - PAINTER_ROW_PITCH);
}

// -- FUN_801DCF84 / FUN_801DD028 -----------------------------------

#[test]
fn counter_pictogram_sits_two_below_the_origin() {
    let font = legaia_font::Font::placeholder();
    let (_, pic) = counter_panel_draws_for(&font, W32, COUNTER_PICTOGRAM_PRIMARY, 1234);
    assert_eq!((pic.id, pic.x, pic.y), (0x62, W32.x, W32.y + 2));
}

// -- FUN_801DCC20 --------------------------------------------------

#[test]
fn count_panel_reserves_its_sub_rect_even_with_no_selection() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(14, 108, 144, 40);
    let (draws, reserved) = count_panel_draws_for(&font, rect, None);
    assert!(draws.is_empty());
    assert_eq!(reserved, (14, 108 + 0x38, 0x90, 0x28));
    let (draws, _) = count_panel_draws_for(&font, rect, Some(7));
    assert!(!draws.is_empty());
}

// -- FUN_801DCCB4 / FUN_801DCE20 -----------------------------------

/// Window 31's three pens, off `FUN_801DCE20`: the heading at the
/// content origin, the 8-cell bank one row pitch below it in the
/// accent pen, the unit label `0x40` right of that, and the corner
/// cursor at the fixed `(+0xE6, +0xD)` inset the whole notify family
/// shares with window 7.
#[test]
fn amount_prompt_puts_its_number_a_row_below_the_heading() {
    let font = legaia_font::Font::placeholder();
    let (draws, cur) = amount_prompt_draws_for(&font, W32, "earned", 12, "pt");
    assert_eq!((cur.x, cur.y), (W32.x + 0xE6, W32.y + 0x0D));
    let row1 = W32.y + PAINTER_ROW_PITCH;
    // The 8-cell field is right-aligned, so "12" inks the last two
    // cells - never the field origin.
    let digits: Vec<i32> = draws
        .iter()
        .filter(|d| d.dst.1 == row1 && d.color == PAINTER_INK_ACCENT)
        .map(|d| d.dst.0)
        .collect();
    assert_eq!(digits.len(), 2);
    assert!(digits.iter().all(|x| *x > W32.x));
    // The unit label shares the number's row but keeps the default pen.
    assert!(
        draws
            .iter()
            .any(|d| d.dst.1 == row1 && d.dst.0 >= W32.x + 0x40 && d.color == MENU_TEXT_WHITE)
    );
    // The heading is the only thing on the origin row.
    assert!(draws.iter().any(|d| d.dst.1 == W32.y));
}

// -- FUN_801D4A80 --------------------------------------------------

#[test]
fn only_accessories_route_through_the_passive_table() {
    assert_eq!(description_source(0, 0), DescriptionSource::Item);
    assert_eq!(description_source(1, 0x7F), DescriptionSource::Item);
    assert_eq!(
        description_source(ITEM_KIND_ACCESSORY, 0x3F),
        DescriptionSource::AccessoryPassive(0x3F)
    );
    // The bound is exclusive, and out-of-range draws nothing at all.
    assert_eq!(
        description_source(ITEM_KIND_ACCESSORY, 0x40),
        DescriptionSource::None
    );
}

#[test]
fn the_not_held_sentinel_reads_as_zero() {
    assert_eq!(owned_count_or_zero(OWNED_COUNT_ABSENT, 9), 0);
    assert_eq!(owned_count_or_zero(3, 9), 9);
}

#[test]
fn item_panel_is_empty_without_a_selection() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(138, 166, 168, 38);
    assert!(item_description_draws_for(&font, rect, false, "n", 1, "d").is_empty());
    assert!(!item_description_draws_for(&font, rect, true, "n", 1, "d").is_empty());
}

#[test]
fn a_pipe_breaks_the_description_onto_the_next_row() {
    let font = legaia_font::Font::placeholder();
    let one = broken_text_draws_for(&font, "a", (0, 0), MENU_TEXT_WHITE);
    let two = broken_text_draws_for(&font, "a|a", (0, 0), MENU_TEXT_WHITE);
    // Two rows of one glyph each - no pipe glyph drawn.
    assert_eq!(two.len(), 2 * one.len());
    let y0 = two.first().unwrap().dst.1;
    let y1 = two.last().unwrap().dst.1;
    assert_eq!(y1 - y0, PAINTER_ROW_PITCH);
    assert_eq!(two.first().unwrap().dst.0, two.last().unwrap().dst.0);
}

// -- FUN_801D56FC --------------------------------------------------

#[test]
fn the_fourth_party_class_matches_no_equipment() {
    assert_eq!(equip_class_mask(0), 1);
    assert_eq!(equip_class_mask(1), 2);
    assert_eq!(equip_class_mask(2), 4);
    assert_eq!(equip_class_mask(3), 0);
    // Mask 7 = "any party member" still excludes class 3, because the
    // table entry is zero rather than a fourth bit.
    assert!(equip_row_enabled(7, 0));
    assert!(!equip_row_enabled(7, 3));
}

#[test]
fn equip_list_rows_take_marker_index_one_and_up() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(138, 98, 168, 52);
    let rows = [
        (
            EquipTargetRow {
                member_class: 0,
                equippable: true,
            },
            "a",
        ),
        (
            EquipTargetRow {
                member_class: 1,
                equippable: false,
            },
            "b",
        ),
    ];
    // Settled on index 2 -> only the second member row is marked.
    let (_, sprites) = equip_target_list_draws_for(&font, rect, "h", &rows, ChoiceFlags(2));
    assert_eq!(sprites.len(), 1);
    assert_eq!(sprites[0].x, rect.x + 4);
    assert_eq!(sprites[0].y, rect.y + 2 * PAINTER_ROW_PITCH);
}

// -- FUN_801D5944 --------------------------------------------------

#[test]
fn the_digit_ladder_is_not_a_digit_count() {
    assert_eq!(sell_total_digits(0), 4);
    assert_eq!(sell_total_digits(99), 4);
    assert_eq!(sell_total_digits(100), 5);
    assert_eq!(sell_total_digits(999), 5);
    assert_eq!(sell_total_digits(1_000), 6);
    assert_eq!(sell_total_digits(9_999), 6);
    // The >= 10000 arm assigns 5 and then the two adds still apply.
    assert_eq!(sell_total_digits(10_000), 7);
}

#[test]
fn the_sell_total_is_half_the_list_price() {
    assert_eq!(sell_total(1, 100), 50);
    assert_eq!(sell_total(3, 25), 37);
    assert_eq!(sell_total(0, 9_999), 0);
}

#[test]
fn the_sell_row_carries_its_separator_glyph() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(14, 46, 144, 33);
    let (draws, _, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10);
    // Glyph 6 at (WX + 0x20, WY + 0x14) - one draw between the quantity
    // and the held count, which is what separates "how many" from "how
    // many you have".
    let sep = separator_glyph_draws(&font, SEPARATOR_GLYPH_SLASH, (0, 0), MENU_TEXT_WHITE);
    assert_eq!(sep.len(), 1);
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 == rect.x + 0x20 && d.dst.1 == rect.y + 0x14)
    );
}

#[test]
fn only_the_three_pinned_separator_ids_resolve() {
    assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_SLASH), Some('/'));
    assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_COLON), Some(':'));
    assert_eq!(separator_glyph_char(SEPARATOR_GLYPH_DOT), Some('.'));
    assert_eq!(separator_glyph_char(0), None);
    let font = legaia_font::Font::placeholder();
    assert!(separator_glyph_draws(&font, 0, (0, 0), MENU_TEXT_WHITE).is_empty());
}

#[test]
fn a_wider_total_pushes_the_pictogram_left() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(14, 46, 144, 33);
    let (_, cheap, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10);
    let (_, dear, _) = sell_quantity_draws_for(&font, rect, true, "h", 1, 9, 10_000);
    assert!(dear.unwrap().x < cheap.unwrap().x);
    let (draws, pic, cur) = sell_quantity_draws_for(&font, rect, false, "h", 1, 9, 10);
    assert!(draws.is_empty() && pic.is_none() && cur.is_none());
}

// -- FUN_801E4140 --------------------------------------------------

#[test]
fn the_box_emit_clips_below_the_display() {
    assert_eq!(guarded_box_rect(0, 0xF0, 8, 8), Some((0, 0xF0, 8, 8)));
    assert_eq!(guarded_box_rect(0, 0xF1, 8, 8), None);
}

#[test]
fn both_prompt_windows_share_the_corner_cursor_inset() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(38, 100, 244, 28);
    let (_, a) = char_prompt_draws_for(&font, rect, b"line");
    let (_, b) = amount_prompt_draws_for(&font, rect, "head", 12, "unit");
    assert_eq!((a.x, a.y), (rect.x + 0xE6, rect.y + 0x0D));
    assert_eq!((b.x, b.y), (a.x, a.y));
}

/// Window 8 shares window 7's pens, and a `0x7C` break drops the second
/// line one string-primitive pitch (`0xE`) below the first.
#[test]
fn the_notify_window_stacks_its_lines_and_shares_the_cursor() {
    let font = legaia_font::Font::placeholder();
    let rect = PainterRect::new(38, 100, 244, 28);
    let lines = vec!["a".to_string(), "a".to_string()];
    let (draws, cursor) = notify_prompt_draws_for(&font, rect, &lines);
    let (_, prompt) = char_prompt_draws_for(&font, rect, b"line");
    assert_eq!((cursor.x, cursor.y), (prompt.x, prompt.y));
    let (one, _) = notify_prompt_draws_for(&font, rect, &lines[..1]);
    assert!(!one.is_empty() && draws.len() > one.len());
    let top = draws.iter().map(|d| d.dst.1).min().unwrap();
    let bottom = draws.iter().map(|d| d.dst.1).max().unwrap();
    assert_eq!(bottom - top, MENU_STRING_LINE_PITCH);
    assert_eq!(
        crate::painter_for_renderer_va(0x801D_CD58),
        Some(crate::MenuWindowPainter::NotifyPrompt)
    );
}
