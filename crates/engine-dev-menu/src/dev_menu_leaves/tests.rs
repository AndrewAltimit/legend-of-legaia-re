use super::*;

// --- FUN_801E5B4C sub-panel ------------------------------------------

fn panel_input() -> EquipPanelInput {
    EquipPanelInput {
        x: 0x20,
        y: 0x30,
        char_index: 1,
        slots: [1, 0, 0, 0, 0],
        base_stats: [10, 20, 30],
        ..EquipPanelInput::default()
    }
}

/// Item 1 is an equippable with bonus row 1; item 2 is `kind 2`; item 3 is
/// an equippable Noa cannot wear; item 9 is a kind the panel ignores.
fn item(id: u8) -> ItemProps {
    match id {
        1 => ItemProps {
            kind: 1,
            stat_index: 1,
        },
        2 => ItemProps {
            kind: 2,
            stat_index: 0,
        },
        3 => ItemProps {
            kind: 1,
            stat_index: 3,
        },
        9 => ItemProps {
            kind: 7,
            stat_index: 0,
        },
        _ => ItemProps::default(),
    }
}

fn equip(idx: u8) -> EquipProps {
    match idx {
        1 => EquipProps {
            bonuses: [0, 5, 0, 0, 0],
            char_mask: 7,
            slot_bits: 0x40,
        },
        3 => EquipProps {
            bonuses: [0, 9, 0, 0, 0],
            char_mask: 1,
            slot_bits: 0x40,
        },
        _ => EquipProps::default(),
    }
}

#[test]
fn the_mode_ladder_picks_the_candidate_source() {
    let inv = [0u8, 4, 8];
    for m in EQUIP_MODE_INVENTORY {
        assert_eq!(equip_candidate(m, 2, &inv), EquipCandidate::Item(8));
    }
    // `0x3000` uses the cursor as the id itself.
    assert_eq!(
        equip_candidate(EQUIP_MODE_DIRECT, 2, &inv),
        EquipCandidate::Item(2)
    );
    // `0x4000` compares against nothing, and only on cursor 1.
    assert_eq!(
        equip_candidate(EQUIP_MODE_BLANK, 1, &inv),
        EquipCandidate::Empty
    );
    assert_eq!(
        equip_candidate(EQUIP_MODE_BLANK, 0, &inv),
        EquipCandidate::None
    );
    // Anything else falls through to the plain block.
    assert_eq!(equip_candidate(0, 1, &inv), EquipCandidate::None);
    assert_eq!(equip_candidate(0x2000, 1, &inv), EquipCandidate::None);
}

#[test]
fn the_plain_block_is_three_label_value_rows() {
    let input = panel_input();
    let out = equip_stat_panel(&input, item, equip);
    assert_eq!(out.len(), 6, "three rows, label + current only");
    // Row 0 sits at the pen; rows step by the wide pitch.
    let want_y = [0x30, 0x30 + 0x0E, 0x30 + 0x1C];
    for (row, &y) in want_y.iter().enumerate() {
        assert_eq!(
            out[row * 2],
            EquipPanelDraw::Label {
                label_va: EQUIP_LABEL_VAS[row],
                x: 0x20 + EQUIP_COL_LABEL,
                y,
                ink: EQUIP_INK_NORMAL,
            }
        );
        // Slot 0 holds item 1, whose bonus lands on accumulator 1 - so
        // only the ATK row picks it up.
        let bonus = if row == 0 { 5 } else { 0 };
        assert_eq!(
            out[row * 2 + 1],
            EquipPanelDraw::Value {
                value: input.base_stats[row] + bonus,
                x: 0x20 + EQUIP_COL_CURRENT,
                y,
                ink: EQUIP_INK_NORMAL,
            }
        );
    }
}

#[test]
fn the_board_flag_tightens_the_row_pitch_by_one() {
    let mut input = panel_input();
    input.tight_rows = true;
    let out = equip_stat_panel(&input, item, equip);
    let ys: Vec<i16> = out
        .iter()
        .filter_map(|d| match d {
            EquipPanelDraw::Label { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(ys, vec![0x30, 0x30 + 0x0D, 0x30 + 0x1A]);
}

#[test]
fn a_candidate_adds_the_arrow_and_inks_the_column_beside_it() {
    // Trial-equip item 1 (ATK +5) over an empty weapon slot: ATK rises.
    let mut input = panel_input();
    input.slots = [0; 5];
    input.base_stats = [10, 20, 30];
    input.mode = EQUIP_MODE_DIRECT;
    input.cursor = 1;
    let out = equip_stat_panel(&input, item, equip);

    // Row 0: label, current, arrow, candidate. Rows 1/2: no arrow.
    assert_eq!(
        out[2],
        EquipPanelDraw::Arrow {
            glyph: EQUIP_GLYPH_HIGHER,
            x: 0x20 + EQUIP_COL_ARROW,
            y: 0x30,
            // The arrow itself draws under the label's ink; its own store
            // lands after the `jal`.
            ink: EQUIP_INK_NORMAL,
        }
    );
    assert_eq!(
        out[3],
        EquipPanelDraw::Value {
            value: 10 + 5,
            x: 0x20 + EQUIP_COL_CANDIDATE,
            y: 0x30,
            ink: EQUIP_INK_HIGHER,
        }
    );
    // The unchanged rows still print a candidate column, just no arrow and
    // no ink change.
    assert_eq!(
        out[6],
        EquipPanelDraw::Value {
            value: 20,
            x: 0x20 + EQUIP_COL_CANDIDATE,
            y: 0x30 + 0x0E,
            ink: EQUIP_INK_NORMAL,
        }
    );
    assert!(
        !out.iter().any(|d| matches!(
            d,
            EquipPanelDraw::Arrow { y, .. } if *y != 0x30
        )),
        "only the stat that moved gets an arrow"
    );
}

#[test]
fn losing_a_stat_picks_the_other_glyph_and_ink() {
    let mut input = panel_input();
    // Currently wearing item 1 in the weapon slot; the candidate is item 2,
    // which is `kind 2` - so retail draws the plain block, not a swap.
    input.mode = EQUIP_MODE_DIRECT;
    input.cursor = 2;
    assert_eq!(equip_stat_panel(&input, item, equip).len(), 6);

    // A real downgrade: wearing item 3 (+9), trial item 1 (+5).
    let mut input = panel_input();
    input.char_index = 0;
    input.slots = [0, 0, 3, 0, 0];
    input.weapon_slots = vec![2, 2, 2];
    input.mode = EQUIP_MODE_DIRECT;
    input.cursor = 1;
    let out = equip_stat_panel(&input, item, equip);
    assert_eq!(
        out[2],
        EquipPanelDraw::Arrow {
            glyph: EQUIP_GLYPH_LOWER,
            x: 0x20 + EQUIP_COL_ARROW,
            y: 0x30,
            ink: EQUIP_INK_NORMAL,
        }
    );
    assert!(matches!(
        out[3],
        EquipPanelDraw::Value {
            value: 15,
            ink: EQUIP_INK_LOWER,
            ..
        }
    ));
}

#[test]
fn an_unequippable_candidate_replaces_the_whole_panel_with_one_line() {
    let mut input = panel_input();
    // Item 3's mask is Vahn-only and this entry is Noa.
    input.char_index = 1;
    input.mode = EQUIP_MODE_DIRECT;
    input.cursor = 3;
    let out = equip_stat_panel(&input, item, equip);
    assert_eq!(
        out,
        vec![EquipPanelDraw::Label {
            label_va: EQUIP_REJECT_VA,
            x: 0x20 + EQUIP_REJECT_OFFSET.0,
            y: 0x30 + EQUIP_REJECT_OFFSET.1,
            ink: EQUIP_INK_REJECT,
        }]
    );
}

#[test]
fn a_candidate_of_any_other_kind_draws_nothing_at_all() {
    let mut input = panel_input();
    input.mode = EQUIP_MODE_DIRECT;
    input.cursor = 9;
    assert!(
        equip_stat_panel(&input, item, equip).is_empty(),
        "`bne a0,v0,0x801E63E0` at 0x801E6280 skips the epilogue's draws"
    );
}

#[test]
fn the_empty_candidate_compares_the_loadout_against_nothing() {
    let mut input = panel_input();
    input.mode = EQUIP_MODE_BLANK;
    input.cursor = 1;
    let out = equip_stat_panel(&input, item, equip);
    // ATK is 10 + 5 now and 10 bare, so the candidate column is lower.
    assert!(matches!(
        out[2],
        EquipPanelDraw::Arrow {
            glyph: EQUIP_GLYPH_LOWER,
            ..
        }
    ));
    assert!(matches!(
        out[3],
        EquipPanelDraw::Value {
            value: 10,
            ink: EQUIP_INK_LOWER,
            ..
        }
    ));
}

#[test]
fn fixed_decimal_zero_pads() {
    assert_eq!(format_fixed_decimal(7, 3), "007");
    assert_eq!(format_fixed_decimal(0, 2), "00");
    assert_eq!(format_fixed_decimal(1234, 4), "1234");
}

#[test]
fn fixed_decimal_truncates_to_width() {
    // value % 10^width - the retail reduction.
    assert_eq!(format_fixed_decimal(1234, 3), "234");
    assert_eq!(format_fixed_decimal(99999, 4), "9999");
}

#[test]
fn fixed_decimal_negative_prefixes_minus() {
    assert_eq!(format_fixed_decimal(-5, 3), "-005");
    assert_eq!(format_fixed_decimal(-42, 2), "-42");
}

#[test]
fn dev_menu_row_indexing() {
    assert_eq!(DevMenuRow::from_index(0), Some(DevMenuRow::MapChange));
    assert_eq!(DevMenuRow::from_index(0x0C), Some(DevMenuRow::EquipVahn));
    assert_eq!(DevMenuRow::from_index(0x0E), Some(DevMenuRow::EquipGala));
    assert_eq!(DevMenuRow::from_index(0x17), Some(DevMenuRow::ResetErrors));
    assert_eq!(DevMenuRow::from_index(0x18), None);
}

#[test]
fn only_map_card_rows_close() {
    assert!(DevMenuRow::MapChange.is_closed(1));
    assert!(DevMenuRow::CardOption.is_closed(0x1234));
    assert!(!DevMenuRow::MapChange.is_closed(0));
    assert!(!DevMenuRow::PlayerStatus.is_closed(1));
}

#[test]
fn camera_readout_sentinel_and_average() {
    assert_eq!(decode_camera_readout(0x7F7F_0000), None);
    // low lane bytes 0x40 & 0x60 -> (0x40+0x60)/2 = 0x50
    // high lane bytes 0x10 & 0x30 -> (0x10+0x30)/2 = 0x20
    let w = 0x30_60_10_40u32; // b3=0x30, b2=0x60, b1=0x10, b0=0x40
    assert_eq!(decode_camera_readout(w), Some((0x50, 0x20)));
}

#[test]
fn panel_geometry_bottom_anchors() {
    // 5 rows -> height 40, y = 0xD0 - 40 = 168.
    assert_eq!(panel_geometry(0, 4), PanelGeometry { y: 168, height: 40 });
    // single row.
    assert_eq!(panel_geometry(3, 3), PanelGeometry { y: 0xC8, height: 8 });
}

#[test]
fn cursor_wraps_by_swap() {
    // up from top -> jumps to bottom.
    assert_eq!(dev_menu_cursor_step(0, 0, 4, true, false), (4, true));
    // down from bottom -> jumps to top.
    assert_eq!(dev_menu_cursor_step(4, 0, 4, false, true), (0, true));
    // plain move.
    assert_eq!(dev_menu_cursor_step(2, 0, 4, false, true), (3, true));
    // no input -> no move.
    assert_eq!(dev_menu_cursor_step(2, 0, 4, false, false), (2, false));
}

#[test]
fn list_body_draw_gate() {
    // phase Active(1) with input allowed -> draws.
    assert!(list_body_draws(1, 1));
    // phase CancelUnwind(3) while unwind running -> draws.
    assert!(list_body_draws(3, 1));
    // phase CancelUnwind after unwind done -> no draw.
    assert!(!list_body_draws(3, 0));
    // phase ConfirmSettle -> never draws.
    assert!(!list_body_draws(2, 1));
    // phase Active with input suppressed -> no draw.
    assert!(!list_body_draws(1, 0));
}

#[test]
fn play_time_decomposition_and_clamp() {
    // 90 frames -> 1 second (90/60 = 1).
    assert_eq!(decompose_play_time(90), (0, 0, 1));
    // 1 hour = 60*60*60 frames.
    assert_eq!(decompose_play_time(60 * 60 * 60), (1, 0, 0));
    // over 99h clamps to 99:59:59.
    assert_eq!(decompose_play_time(200 * 60 * 60 * 60), (99, 59, 59));
}

#[test]
fn records_screen_clamps() {
    let chars = [
        CharRecordStats {
            max_hits: 5000,
            max_damage: 50_000_000,
            knockouts: 5000,
            monsters_defeated: 5_000_000,
            hyper_arts: 7,
            magic: 3,
        },
        CharRecordStats::default(),
        CharRecordStats::default(),
    ];
    let r = records_screen(200_000, 42, 60, &chars, 25, 50);
    assert_eq!(r.battles, 99_999); // capped
    assert_eq!(r.escapes, 42); // uncapped
    assert_eq!(r.max_hits[0], 999);
    assert_eq!(r.max_damage[0], 9_999_999);
    assert_eq!(r.knockouts[0], 999);
    assert_eq!(r.monsters_defeated[0], 999_999);
    assert_eq!(r.hyper_arts[0], 7);
    assert!(r.treasure_shown);
    assert_eq!(r.treasure_percent, 50); // 25*100/50
    assert_eq!(r.treasure_fraction, 0);
}

#[test]
fn records_treasure_hidden_when_no_total() {
    let chars = [CharRecordStats::default(); 3];
    let r = records_screen(0, 0, 0, &chars, 0, 0);
    assert!(!r.treasure_shown);
    assert_eq!(r.treasure_percent, 0);
}

#[test]
fn treasure_fraction_nonzero() {
    let chars = [CharRecordStats::default(); 3];
    // 1 of 3 -> 33.33%
    let r = records_screen(0, 0, 0, &chars, 1, 3);
    assert_eq!(r.treasure_percent, 33); // 100/3
    assert_eq!(r.treasure_fraction, 33); // 10000/3=3333, minus 33*100=3300 -> 33
}

#[test]
fn equip_aggregation_sums_bonuses() {
    // slots 1 and 2 equipped, ids 3 and 4 respectively.
    let slots = [1u8, 2, 0, 0, 0];
    let item_stat = |id: u8| match id {
        1 => 10, // stat index 10
        2 => 20,
        _ => 0,
    };
    let bonus = |idx: u8| match idx {
        10 => [1, 2, 3, 4, 5],
        20 => [5, 4, 3, 2, 1],
        _ => [0, 0, 0, 0, 0],
    };
    let totals = aggregate_slot_stats(&slots, item_stat, bonus);
    assert_eq!(totals, [6, 6, 6, 6, 6]);
}

#[test]
fn equippability_mask() {
    // mask bit per character.
    assert!(can_equip(0b001, 0)); // Vahn
    assert!(!can_equip(0b001, 1)); // Noa cannot
    assert!(can_equip(0b010, 1)); // Noa
    assert!(can_equip(0b100, 2)); // Gala
    assert!(can_equip(0, 5)); // out-of-range char treated equippable
}

#[test]
fn slot_bits_resolution() {
    let weapon = [0i16, 1, 2, 3];
    assert_eq!(resolve_equip_slot(0x00, 0, &weapon), 0); // sel 0
    assert_eq!(resolve_equip_slot(0x20, 0, &weapon), 1); // sel 1
    assert_eq!(resolve_equip_slot(0x40, 2, &weapon), 2); // sel 2 -> weapon[2]
    assert_eq!(resolve_equip_slot(0x60, 0, &weapon), 4); // sel 3 -> slot 4
}

#[test]
fn stat_delta_arrows() {
    let current = [10, 20, 30, 0, 0];
    let candidate = [15, 20, 25, 0, 0];
    let d = stat_deltas(&current, &candidate);
    assert_eq!(d[0], StatDelta::Up);
    assert_eq!(d[1], StatDelta::Same);
    assert_eq!(d[2], StatDelta::Down);
}
