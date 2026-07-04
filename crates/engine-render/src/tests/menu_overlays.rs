use super::*;

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
    let draws = field_menu_draws_for(&font, &rows, 0, 1234, 90, (24, 24), (24, 178));
    assert!(!draws.is_empty());
    // Selected row should produce ">" cursor glyph at the row x.
    let any_gold = draws.iter().any(|d| d.color[1] > 0.7 && d.color[2] < 0.5);
    assert!(any_gold);
    // Money + play-time land in the corner-box window (below the list).
    let any_in_money_box = draws.iter().any(|d| d.dst.1 >= 178);
    assert!(any_in_money_box);
}

#[test]
fn status_screen_draws_pack_panel() {
    let font = legaia_font::synthetic_for_tests();
    let stat_rows = [
        StatusStatRow {
            label: "ATK",
            value: 12,
            growth: 12,
        },
        StatusStatRow {
            label: "UDF",
            value: 9,
            growth: 9,
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
    let draws = status_screen_draws_for(&font, &panel, Some("L1/R1: Switch"), (16, 32), false);
    assert!(!draws.is_empty());
}

/// The status-page UI-icon sprites land at the byte-pinned retail
/// positions: for the retail id-28 window origin `(90, 16)` the AP gauge
/// pieces sit at x 154/178/234/250 on y=61, a single-digit AP value at
/// `(241, 66)`, the left pictogram column at x=90 on rows
/// 125/138/151/164 and the Goods column at x=196 on rows 138/151/164
/// (all pixel-verified against the golden `menu_status_town` capture).
#[test]
fn status_icon_sprites_pin_gauge_and_pictogram_positions() {
    let rects = super::title_save_screen::pinned_save_menu_rects();
    let draws = status_icon_sprites_for(&rects, (90, 16), 0, (0, 0), 1);
    let at = |x: i32, y: i32| {
        draws
            .iter()
            .find(|d| d.dst.0 == x && d.dst.1 == y)
            .unwrap_or_else(|| panic!("no sprite at ({x},{y})"))
    };
    // Gauge pieces (cap / trough / value box / right tip) on the bar row.
    assert_eq!(at(154, 61).src, rects.gauge_cap);
    assert_eq!(at(178, 61).src, rects.gauge_trough);
    assert_eq!(at(234, 61).src, rects.gauge_box);
    assert_eq!(at(250, 61).src, rects.gauge_tip);
    // AP value 0: no tens digit, the ones "0" cell at the anchor+0x56
    // position (FUN_8002c0b0), and no meter fill.
    let (dgx, dgy, _, _) = rects.gauge_digits;
    assert_eq!(at(240, 66).src, (dgx, dgy, 6, 6));
    assert!(!draws.iter().any(|d| d.src == rects.gauge_fill));
    // Equipment pictograms: left column (weapon/helmet/armor/boot) and
    // the 3-row Goods column.
    assert_eq!(at(90, 125).src, rects.icon_weapon);
    assert_eq!(at(90, 138).src, rects.icon_helmet);
    assert_eq!(at(90, 151).src, rects.icon_armor);
    assert_eq!(at(90, 164).src, rects.icon_boot);
    for y in [138, 151, 164] {
        assert_eq!(at(196, y).src, rects.icon_goods);
    }
}

/// The FUN_8002c0b0 value layout: tens digit at anchor+0x50, ones at
/// anchor+0x56 (6x6 cells), the meter fill stretched to `value/2` px
/// from anchor+0x1B - and a full 100 swaps the digits for the
/// dedicated "100" glyph with a 50-px fill.
#[test]
fn status_icon_sprites_gauge_value_and_fill_layout() {
    let rects = super::title_save_screen::pinned_save_menu_rects();
    let (dgx, dgy, _, _) = rects.gauge_digits;

    // AP 42: tens '4' at 234, ones '2' at 240, fill 21 px at (181, 66).
    let draws = status_icon_sprites_for(&rects, (90, 16), 42, (0, 0), 1);
    let digits: Vec<_> = draws
        .iter()
        .filter(|d| d.dst.1 == 66 && d.src.1 == dgy && d.src.2 == 6)
        .collect();
    assert_eq!(digits.len(), 2);
    assert_eq!(digits[0].dst.0, 234);
    assert_eq!(digits[0].src, (dgx + 4 * 6, dgy, 6, 6)); // '4'
    assert_eq!(digits[1].dst.0, 240);
    assert_eq!(digits[1].src, (dgx + 2 * 6, dgy, 6, 6)); // '2'
    let fill = draws
        .iter()
        .find(|d| d.src == rects.gauge_fill)
        .expect("AP 42 must draw a meter fill");
    assert_eq!(fill.dst, (181, 66, 21, 6));

    // AP 7: no tens digit, ones '7' only, 3-px fill (7 >> 1).
    let draws = status_icon_sprites_for(&rects, (90, 16), 7, (0, 0), 1);
    let digits: Vec<_> = draws
        .iter()
        .filter(|d| d.dst.1 == 66 && d.src.1 == dgy && d.src.2 == 6)
        .collect();
    assert_eq!(digits.len(), 1);
    assert_eq!(digits[0].dst.0, 240);
    assert_eq!(digits[0].src, (dgx + 7 * 6, dgy, 6, 6));
    let fill = draws.iter().find(|d| d.src == rects.gauge_fill).unwrap();
    assert_eq!(fill.dst, (181, 66, 3, 6));

    // AP 100: the "100" glyph at anchor+0x50, no digit cells, 50-px fill.
    let draws = status_icon_sprites_for(&rects, (90, 16), 100, (0, 0), 1);
    let glyph = draws
        .iter()
        .find(|d| d.src == rects.gauge_100)
        .expect("AP 100 must draw the dedicated glyph");
    assert_eq!((glyph.dst.0, glyph.dst.1), (234, 66));
    assert!(!draws.iter().any(|d| d.src.1 == dgy && d.src.2 == 6));
    let fill = draws.iter().find(|d| d.src == rects.gauge_fill).unwrap();
    assert_eq!(fill.dst, (181, 66, 50, 6));
}

/// With `label_icons` set, the AP text stand-in disappears (the sprite
/// gauge replaces it) and empty equipment names draw nothing at the
/// pictogram positions; occupied slots move to the +0x10 name offset.
#[test]
fn status_screen_label_icons_suppresses_ap_text_and_empty_equips() {
    let font = legaia_font::synthetic_for_tests();
    let equip_rows = [("Weapon", "#05"), ("Helmet", "")];
    let panel = StatusPanelView {
        name: "Vahn",
        level: 1,
        xp: 0,
        xp_to_next: 121,
        hp: 180,
        hp_max: 180,
        mp: 20,
        mp_max: 20,
        ap: 4,
        ap_max: 100,
        stat_rows: &[],
        equip_rows: &equip_rows,
    };
    let with_icons = status_screen_draws_for(&font, &panel, None, (90, 16), true);
    let without = status_screen_draws_for(&font, &panel, None, (90, 16), false);
    // The icons variant emits strictly fewer glyphs: no LV/HP/MP tags,
    // no "AP  4/100" readout, no empty-slot text.
    assert!(with_icons.len() < without.len());
    // The occupied slot's name lands at the +0x10 name offset
    // (icon x 90 -> text x 106) on the slot-0 row (y = 16 + 0x6d).
    assert!(with_icons.iter().any(|d| d.dst.0 >= 106 && d.dst.1 == 125));
    // No icon-position text on the empty slot-1 row.
    assert!(!with_icons.iter().any(|d| d.dst.1 == 138));
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
            label: "Battle Camera",
            value: Some("Close"),
            teal: false,
            advance: 14,
        },
        OptionsRowView {
            label: "Dual Shock",
            value: None,
            teal: false,
            advance: 14,
        },
        OptionsRowView {
            label: "  Battles",
            value: Some("Vibration On"),
            teal: true,
            advance: 14,
        },
    ];
    let draws = options_draws_for(&font, &rows, 0, None, (24, 40));
    assert!(!draws.is_empty());
    // Teal ink present (the Dual Shock sub-row label), gold value column,
    // and nothing below the last row (no footer hint).
    assert!(draws.iter().any(|d| d.color == OPTIONS_INK_TEAL));
    assert!(draws.iter().any(|d| d.color == OPTIONS_INK_GOLD));
    // Value column at the retail +140 inset.
    assert!(
        draws
            .iter()
            .any(|d| d.color == OPTIONS_INK_GOLD && d.dst.0 >= 24 + 140)
    );
    let max_y = draws.iter().map(|d| d.dst.1).max().unwrap();
    assert!(
        max_y < 40 + 3 * 14 + 14,
        "no footer below the rows: {max_y}"
    );
}

#[test]
fn options_draws_popup_lists_choices() {
    let font = legaia_font::synthetic_for_tests();
    let rows = [OptionsRowView {
        label: "Battle Camera",
        value: Some("Close"),
        teal: false,
        advance: 14,
    }];
    let popup = OptionsPopupDraw {
        rect: (170, 62, 128, 35),
        choices: &["Close", "Normal", "Far"],
        cursor: 1,
    };
    let draws = options_draws_for(&font, &rows, 0, Some(&popup), (24, 40));
    // Popup choice text starts at the retail +0x14 inset with a 13-px
    // pitch (three choices: rows at y = 62 / 75 / 88).
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 >= 170 + 0x14 && (62..75).contains(&d.dst.1))
    );
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 >= 170 + 0x14 && d.dst.1 >= 62 + 26)
    );
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

/// The retail equip-screen window pens (descriptor ids 21 / 23 / 22).
const EQUIP_PARTY_PEN: (i32, i32) = (14, 42);
const EQUIP_LIST_PEN: (i32, i32) = (174, 22);
const EQUIP_MAIN_PEN: (i32, i32) = (14, 96);

fn equip_view<'a>(
    slots: &'a [EquipSlotRow<'a>],
    candidates: &'a [EquipCandidateRow<'a>],
    stat_compare: &'a [EquipStatRow<'a>],
    phase: EquipDrawPhase,
) -> EquipScreenView<'a> {
    EquipScreenView {
        party_names: &["Vahn"][..1],
        party_cursor: 0,
        slots,
        candidates,
        stat_compare,
        phase,
        cursor: 0,
        active_slot: 0,
        confirm_label: None,
        text_cursor: true,
    }
}

/// Slot-picker phase: the "Best Equipment" header sits at main+0x10 and
/// equipped names at main+0x20 on the 0xE-pitch rows (FUN_801D21C0);
/// party names land at party+6 (FUN_801D2094); empty slots draw nothing.
#[test]
fn equip_screen_draws_slot_rows_at_retail_offsets() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![
        EquipSlotRow {
            label: "Weapon",
            current_name: "Iron Sword",
        },
        EquipSlotRow {
            label: "Helmet",
            current_name: "",
        },
    ];
    let view = equip_view(&slots, &[], &[], EquipDrawPhase::SlotPicker);
    let draws = equip_screen_draws_for(
        &font,
        &view,
        EQUIP_PARTY_PEN,
        EQUIP_LIST_PEN,
        EQUIP_MAIN_PEN,
    );
    let (mx, my) = EQUIP_MAIN_PEN;
    // Header glyphs start at (mx+0x10, my).
    assert!(draws.iter().any(|d| d.dst.0 == mx + 0x10 && d.dst.1 == my));
    // Slot 0's item name at (mx+0x20, my+0xE).
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 == mx + 0x20 && d.dst.1 == my + 0x0e)
    );
    // Empty slot 1 draws no name glyphs on its row.
    assert!(
        !draws
            .iter()
            .any(|d| d.dst.0 >= mx + 0x20 && d.dst.1 == my + 0x1c)
    );
    // Party name at (px+6, py).
    let (px, py) = EQUIP_PARTY_PEN;
    assert!(draws.iter().any(|d| d.dst.0 == px + 6 && d.dst.1 == py));
}

/// Item-picker phase: candidates fill the id-23 list window at the 0xD
/// list pitch and the stat-compare block lands at the traced main-window
/// offsets (label +0xA0, current +0xC8, preview +0xF0 only on change).
#[test]
fn equip_screen_draws_item_picker_fills_list_window_and_stat_compare() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![EquipSlotRow {
        label: "Weapon",
        current_name: "",
    }];
    let candidates = vec![
        EquipCandidateRow {
            name: "Iron Sword",
            count: 1,
        },
        EquipCandidateRow {
            name: "Wood Sword",
            count: 2,
        },
    ];
    let stat_compare = vec![
        EquipStatRow {
            label: "ATK",
            current: 40,
            preview: 45,
        },
        EquipStatRow {
            label: "UDF",
            current: 30,
            preview: 30,
        },
    ];
    let view = equip_view(
        &slots,
        &candidates,
        &stat_compare,
        EquipDrawPhase::ItemPicker,
    );
    let draws = equip_screen_draws_for(
        &font,
        &view,
        EQUIP_PARTY_PEN,
        EQUIP_LIST_PEN,
        EQUIP_MAIN_PEN,
    );
    let (lx, ly) = EQUIP_LIST_PEN;
    // Candidate rows at (lx+10, ly + 0xD*i).
    assert!(draws.iter().any(|d| d.dst.0 == lx + 10 && d.dst.1 == ly));
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 == lx + 10 && d.dst.1 == ly + 0x0d)
    );
    let (mx, my) = EQUIP_MAIN_PEN;
    // Stat labels at mx+0xA0 on rows my+0x48 / my+0x55.
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 == mx + 0xa0 && d.dst.1 == my + 0x48)
    );
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 == mx + 0xa0 && d.dst.1 == my + 0x55)
    );
    // ATK changed: a preview glyph in the +0xF0 3-digit field on row 0.
    assert!(
        draws
            .iter()
            .any(|d| d.dst.0 >= mx + 0xf0 && d.dst.1 == my + 0x48)
    );
    // UDF unchanged: nothing at/right of the arrow column on row 1.
    assert!(
        !draws
            .iter()
            .any(|d| d.dst.0 >= mx + 0xe4 && d.dst.1 == my + 0x55)
    );
}

#[test]
fn equip_screen_draws_confirm_phase_shows_yes_no_prompt() {
    let font = legaia_font::synthetic_for_tests();
    let slots = vec![EquipSlotRow {
        label: "Weapon",
        current_name: "Iron Sword",
    }];
    let candidates = vec![EquipCandidateRow {
        name: "Steel Sword",
        count: 1,
    }];
    let mut view = equip_view(&slots, &candidates, &[], EquipDrawPhase::Confirm);
    view.confirm_label = Some("Equip Steel Sword?");
    let with_confirm = equip_screen_draws_for(
        &font,
        &view,
        EQUIP_PARTY_PEN,
        EQUIP_LIST_PEN,
        EQUIP_MAIN_PEN,
    );
    let picker = equip_view(&slots, &candidates, &[], EquipDrawPhase::ItemPicker);
    let without = equip_screen_draws_for(
        &font,
        &picker,
        EQUIP_PARTY_PEN,
        EQUIP_LIST_PEN,
        EQUIP_MAIN_PEN,
    );
    // The confirm phase layers the label + Yes/No prompt on top of the
    // candidate list.
    assert!(with_confirm.len() > without.len());
}

/// The equip-screen sprites pin the retail placements: pictograms at
/// main+(0x10, 0xE*(i+1)), the party hand cursor at party+(-0xC,
/// 0xE*row), and the slot hand cursor on the hovered row.
#[test]
fn equip_screen_sprites_pin_pictogram_and_cursor_positions() {
    let rects = super::title_save_screen::pinned_save_menu_rects();
    let draws = equip_screen_sprites_for(
        &rects,
        8,
        EQUIP_MAIN_PEN,
        EQUIP_PARTY_PEN,
        0,
        Some(1),
        (0, 0),
        1,
    );
    let (mx, my) = EQUIP_MAIN_PEN;
    let at = |x: i32, y: i32| {
        draws
            .iter()
            .find(|d| d.dst.0 == x && d.dst.1 == y)
            .unwrap_or_else(|| panic!("no sprite at ({x},{y})"))
    };
    // Pictogram column: weapon fist / helmet / armor / (hand-guard fist)
    // / boot / 3x Goods ring, rows my+0xE onward.
    assert_eq!(at(mx + 0x10, my + 0x0e).src, rects.icon_weapon);
    assert_eq!(at(mx + 0x10, my + 0x1c).src, rects.icon_helmet);
    assert_eq!(at(mx + 0x10, my + 0x2a).src, rects.icon_armor);
    assert_eq!(at(mx + 0x10, my + 0x38).src, rects.icon_weapon);
    assert_eq!(at(mx + 0x10, my + 0x46).src, rects.icon_boot);
    for dy in [0x54, 0x62, 0x70] {
        assert_eq!(at(mx + 0x10, my + dy).src, rects.icon_goods);
    }
    // Party hand cursor overhangs the window's left edge (X-0xC).
    let (px, py) = EQUIP_PARTY_PEN;
    assert_eq!(at(px - 0x0c, py).src, rects.cursor);
    // Slot-picker hand cursor on row 1 at the main window's left edge.
    assert_eq!(at(mx, my + 0x1c).src, rects.cursor);

    // Outside the slot picker no main-window hand is drawn.
    let no_slot_hand = equip_screen_sprites_for(
        &rects,
        8,
        EQUIP_MAIN_PEN,
        EQUIP_PARTY_PEN,
        0,
        None,
        (0, 0),
        1,
    );
    assert_eq!(no_slot_hand.len(), draws.len() - 1);
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

/// The tab-banner plaque composes exactly like the retail RAM prim
/// scan of the `menu_status_town` capture: left cap at `(WX-8, WY-4)`,
/// the 16-wide body tile repeated across the 60-px content width with a
/// 12-px remainder, and the right cap at `(WX+w, WY-4)` - for the
/// window-3 content origin `(12, 12)` that is sprites at x
/// 4/12/28/44/60/72 on y=8.
#[test]
fn tab_banner_composes_retail_plaque_pieces() {
    let rects = super::title_save_screen::pinned_save_menu_rects();
    let draws = tab_banner_draws(&rects, (12, 12), 60, (0, 0), 1);
    assert_eq!(draws.len(), 6);
    // Left cap.
    assert_eq!(draws[0].src, rects.tab_cap_l);
    assert_eq!((draws[0].dst.0, draws[0].dst.1), (4, 8));
    // Body tiles: three full 16-wide + one 12-wide remainder.
    let (bx, by, _, bh) = rects.tab_body;
    for (i, (x, w)) in [(12, 16u32), (28, 16), (44, 16), (60, 12)]
        .iter()
        .enumerate()
    {
        let d = &draws[1 + i];
        assert_eq!(d.src, (bx, by, *w, bh));
        assert_eq!((d.dst.0, d.dst.1, d.dst.2), (*x, 8, *w));
    }
    // Right cap closes the plaque at WX + content_w.
    assert_eq!(draws[5].src, rects.tab_cap_r);
    assert_eq!((draws[5].dst.0, draws[5].dst.1), (72, 8));
    // Every piece is 20 tall.
    assert!(draws.iter().all(|d| d.dst.3 == 20));
}

/// The status satellite sprites land at the traced renderer offsets:
/// the party-list hand at `(WX-0xc, WY + cursor*0xe)` (FUN_801D2094),
/// the pager triangles at `WX-0x10` / `WX+0x3A` on `WY-2`
/// (FUN_801D30A4), and the summary LV icon + ATR element icon at
/// `(+0x1c, +0xf)` / `(+0x20, +0x1a)` (FUN_801D31EC). Retail window
/// pens: list (14,38), pager (14,92), summary (14,134).
#[test]
fn status_satellite_icons_pin_retail_positions() {
    let rects = super::title_save_screen::pinned_save_menu_rects();
    let draws =
        status_satellite_icon_sprites_for(&rects, 1, 0, (14, 38), (14, 92), (14, 134), (0, 0), 1);
    let at = |src: (u32, u32, u32, u32)| {
        draws
            .iter()
            .find(|d| d.src == src)
            .unwrap_or_else(|| panic!("no sprite with src {src:?}"))
    };
    // Hand cursor on row 1 (pitch 14).
    let hand = at(rects.cursor);
    assert_eq!((hand.dst.0, hand.dst.1), (14 - 0x0c, 38 + 14));
    // Pager triangles flank the Condition window, 2 px above its pen.
    let l = at(rects.pager_left);
    assert_eq!((l.dst.0, l.dst.1), (14 - 0x10, 90));
    let r = at(rects.pager_right);
    assert_eq!((r.dst.0, r.dst.1), (14 + 0x3a, 90));
    // Summary LV label + Vahn's ATR element icon.
    let lv = at(rects.label_lv);
    assert_eq!((lv.dst.0, lv.dst.1), (14 + 0x1c, 134 + 0x0f));
    let atr = at(rects.atr_icons[0]);
    assert_eq!((atr.dst.0, atr.dst.1), (14 + 0x20, 134 + 0x1a));
    // A Gala pick (char id 2) swaps only the ATR source rect.
    let draws2 =
        status_satellite_icon_sprites_for(&rects, 0, 2, (14, 38), (14, 92), (14, 134), (0, 0), 1);
    assert!(draws2.iter().any(|d| d.src == rects.atr_icons[2]));
    assert!(!draws2.iter().any(|d| d.src == rects.atr_icons[0]));
}

/// Satellite text: names at `WX+6` on the retail `0x0e` pitch; with
/// `label_icons` the ASCII cursor / pager-arrow stand-ins disappear
/// (the sprites replace them).
#[test]
fn status_satellite_text_uses_retail_pitch_and_suppresses_stand_ins() {
    let font = legaia_font::synthetic_for_tests();
    let view = StatusSatelliteView {
        party_names: &["Vahn", "Noa"],
        cursor: 0,
        name: "Vahn",
        level: 1,
    };
    let with_icons = status_satellite_draws_for(&font, &view, (14, 38), (14, 92), (14, 134), true);
    let without = status_satellite_draws_for(&font, &view, (14, 38), (14, 92), (14, 134), false);
    // Names land at x=20 (WX+6) on rows 38 and 52 (pitch 14).
    assert!(with_icons.iter().any(|d| d.dst.0 == 20 && d.dst.1 == 38));
    assert!(with_icons.iter().any(|d| d.dst.0 == 20 && d.dst.1 == 52));
    // The icons variant drops the ">" cursor, the two pager arrows and
    // the "LV" tag (4 glyph draws).
    assert!(with_icons.len() < without.len());
    // Nothing to the left of the party-list pen remains as text.
    assert!(with_icons.iter().all(|d| d.dst.0 >= 14));
}

/// The status panel's number fields sit on the retail fixed 8-px digit
/// cells: HP 180 in a 4-digit field at `+0x30` puts its glyphs at cells
/// `+0x38/+0x40/+0x48`, ending flush against the `/` at `+0x50` - and
/// the parenthesised base group renders in the retail teal ink.
#[test]
fn status_screen_hp_row_uses_retail_digit_cells_and_teal_parens() {
    let font = legaia_font::synthetic_for_tests();
    let panel = StatusPanelView {
        name: "Vahn",
        level: 1,
        xp: 0,
        xp_to_next: 121,
        hp: 180,
        hp_max: 180,
        mp: 20,
        mp_max: 20,
        ap: 0,
        ap_max: 100,
        stat_rows: &[StatusStatRow {
            label: "ATK",
            value: 24,
            growth: 24,
        }],
        equip_rows: &[],
    };
    let draws = status_screen_draws_for(&font, &panel, None, (90, 16), true);
    let hp_y = 16 + 0x13;
    let hp_row: Vec<i32> = draws
        .iter()
        .filter(|d| d.dst.1 == hp_y)
        .map(|d| d.dst.0)
        .collect();
    // Current-value digits at the 8-px cells of the +0x30 field.
    for x in [90 + 0x38, 90 + 0x40, 90 + 0x48] {
        assert!(hp_row.contains(&x), "no HP glyph at x={x} ({hp_row:?})");
    }
    // "/" at +0x50, "(" at +0x7c, ")" at +0xa4.
    for x in [90 + 0x50, 90 + 0x7c, 90 + 0xa4] {
        assert!(hp_row.contains(&x), "no separator at x={x}");
    }
    // The base group (paren + digits from +0x8c) is teal; the current
    // value is the retail text white.
    let teal = MENU_TEXT_TEAL;
    assert!(
        draws
            .iter()
            .filter(|d| d.dst.1 == hp_y)
            .any(|d| d.color == teal)
    );
    let paren = draws
        .iter()
        .find(|d| d.dst.1 == hp_y && d.dst.0 == 90 + 0x7c)
        .unwrap();
    assert_eq!(paren.color, teal);
    let slash = draws
        .iter()
        .find(|d| d.dst.1 == hp_y && d.dst.0 == 90 + 0x50)
        .unwrap();
    assert_eq!(slash.color, MENU_TEXT_WHITE);
    // Stat grid: "(" at +0x40, growth digits in the +0x48 field
    // (24 -> cells +0x50/+0x58), ")" at +0x60 - all teal.
    let stat_y = 16 + 0x42;
    for x in [90 + 0x40, 90 + 0x50, 90 + 0x58, 90 + 0x60] {
        let d = draws
            .iter()
            .find(|d| d.dst.1 == stat_y && d.dst.0 == x)
            .unwrap_or_else(|| panic!("no stat-grid glyph at x={x}"));
        assert_eq!(d.color, teal, "stat-grid glyph at x={x} not teal");
    }
}
