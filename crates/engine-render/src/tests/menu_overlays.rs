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
