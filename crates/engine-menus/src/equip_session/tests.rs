use super::*;

/// Retail's `+0x196` array is `[body, head, weapon, weapon, footwear,
/// goods x3]` with the weapon on a per-character byte; the engine's
/// slot order puts the weapon first and the other byte of the pair on
/// its Hand Guard row. Vahn's record (the `equip_ui_vahn_*` captures:
/// armour, seal, sword, Ra-Seru, boots) lands row for row on retail's
/// browse order, and the mapping is a bijection for every character.
#[test]
fn the_record_bytes_map_onto_the_browse_rows() {
    let record = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7];
    let engine = engine_equip_from_record(record, 0);
    // Weapon (byte 2), helmet (1), body (0), Ra-Seru (3), boots (4)...
    assert_eq!(engine, [0xA2, 0xA1, 0xA0, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7]);
    let rows: Vec<u8> = BROWSE_SLOT_ORDER
        .iter()
        .map(|&s| engine[usize::from(s)])
        .collect();
    // Retail's seven rows; the Ra-Seru byte (engine Hand Guard) has none.
    assert_eq!(rows, [0xA2, 0xA1, 0xA0, 0xA4, 0xA5, 0xA6, 0xA7]);
    // Noa's weapon is byte 3.
    assert_eq!(engine_equip_from_record(record, 1)[0], 0xA3);
    for party in 0..3u8 {
        let mut seen = [false; 8];
        for slot in 0..8u8 {
            seen[record_byte_for_engine_slot(slot, party)] = true;
        }
        assert!(seen.iter().all(|&b| b), "character {party}");
    }
    for row in 1..SLOT_BROWSE_ROWS {
        assert_eq!(browse_row_for_slot(slot_for_browse_row(row)), row);
    }
}

fn fresh_record() -> StatRecord {
    StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 35,
        base_int: 18,
        equip: [0; 8],
    }
}

fn fresh_session() -> EquipSession {
    let mut inv = crate::item_bag::ItemBag::new();
    // Item 0x05 - slot (5 >> 5) = 0; helmet-class.
    inv.insert(0x05, 1);
    // Item 0x25 - slot 1 (0x25 >> 5 = 1)
    inv.insert(0x25, 2);
    let mut eq = EquipmentTable::new();
    eq.set(
        0x05,
        ItemModifier {
            atk: 5,
            ..Default::default()
        },
    );
    eq.set(
        0x25,
        ItemModifier {
            udf: 7,
            ..Default::default()
        },
    );
    EquipSession::new(
        fresh_record(),
        inv,
        eq,
        StatusModifiers::default(),
        Vec::new(),
    )
}

#[test]
fn new_session_starts_on_slot_picker_cursor_zero() {
    let s = fresh_session();
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
    assert!(!s.is_done());
}

#[test]
fn down_in_slot_picker_advances_cursor_and_emits_event() {
    let mut s = fresh_session();
    s.input(EquipInput {
        down: true,
        ..Default::default()
    });
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 1 });
    let evs = s.drain_events();
    assert!(matches!(
        evs[0],
        EquipEvent::CursorMoved {
            state: EquipStateKind::SlotPicker,
            cursor: 1
        }
    ));
}

/// Retail's slot browse runs the list kernel with wrap (`FUN_801D688C(...,
/// 8, 1)` at `0x801D9B00`): Up on the Best Equipment row lands on the last
/// Goods row, and Down there comes back to the top.
#[test]
fn slot_picker_wraps_both_ways() {
    let mut s = fresh_session();
    s.input(EquipInput {
        up: true,
        ..Default::default()
    });
    assert_eq!(
        s.state(),
        EquipState::SlotPicker {
            cursor: SLOT_BROWSE_ROWS - 1
        }
    );
    s.input(EquipInput {
        down: true,
        ..Default::default()
    });
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
}

/// Row `n` of the browse space opens slot `n - 1`: the row space leads
/// with Best Equipment, so the first slot needs one Down first.
#[test]
fn cross_on_a_slot_row_opens_that_slots_item_picker() {
    let mut s = fresh_session();
    s.input(EquipInput {
        down: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(
        s.state(),
        EquipState::ItemPicker { slot: 0, cursor: 0 }
    ));
}

/// Row 0 is Best Equipment and never opens a picker - with nothing
/// worth swapping it reports the retail buzz case and stays put.
#[test]
fn cross_on_row_zero_runs_best_equipment_instead_of_a_picker() {
    let mut s = fresh_session();
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
    let evs = s.drain_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            EquipEvent::BestEquipmentApplied { .. } | EquipEvent::BestEquipmentUnchanged
        )),
        "row 0 confirm must report a Best Equipment outcome, got {evs:?}"
    );
}

#[test]
fn circle_in_slot_picker_terminates_session_with_cancel() {
    let mut s = fresh_session();
    s.input(EquipInput {
        circle: true,
        ..Default::default()
    });
    assert!(s.is_done());
    assert!(s.swaps().is_empty());
}

#[test]
fn items_for_slot_filters_by_implied_slot() {
    let s = fresh_session();
    let slot0 = s.items_for_slot(0);
    assert_eq!(slot0.len(), 1);
    assert_eq!(slot0[0].id, 0x05);
    let slot1 = s.items_for_slot(1);
    assert_eq!(slot1.len(), 1);
    assert_eq!(slot1[0].id, 0x25);
    let slot7 = s.items_for_slot(7);
    assert!(slot7.is_empty());
}

/// The bag candidates of a list, past its leading Remove / equipped rows.
fn candidates(items: &[EquipItem]) -> Vec<u8> {
    items
        .iter()
        .filter(|i| i.id != REMOVE_ROW_ID && !i.equipped)
        .map(|i| i.id)
        .collect()
}

/// With disc restrictions an armament list is retail's: Remove always
/// first, the equipped id second when the slot holds one, then the accepted
/// bag slots in **slot** order (not id order), the equipped id skipped.
#[test]
fn armament_lists_take_retails_leading_rows_and_bag_order() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let weapon = |mask| DiscEquipEntry {
        mask,
        category: Disc::Weapon,
        is_ra_seru: false,
        passive_index: None,
    };
    let info =
        DiscEquipInfo::from_entries([(0x30, weapon(7)), (0x20, weapon(7)), (0x28, weapon(7))]);
    // Bag slot order 0x30, 0x20, 0x28 - not id order.
    let inv = crate::item_bag::ItemBag::from_slots(&[(0x30, 1), (0x20, 1), (0x28, 1)]);
    let empty = EquipSession::new_with_restrictions(
        fresh_record(),
        inv.clone(),
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info.clone(),
        0,
    );
    let ids: Vec<u8> = empty.items_for_slot(0).iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        [REMOVE_ROW_ID, 0x30, 0x20, 0x28],
        "empty slot: Remove, then bag order"
    );

    let mut rec = fresh_record();
    rec.equip[0] = 0x20;
    let worn = EquipSession::new_with_restrictions(
        rec,
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        0,
    );
    let rows = worn.items_for_slot(0);
    let ids: Vec<u8> = rows.iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        [REMOVE_ROW_ID, 0x20, 0x30, 0x28],
        "worn: Remove, equipped, rest"
    );
    assert!(rows[1].equipped && !rows[2].equipped);
}

#[test]
fn restrictions_gate_item_list_by_character_mask() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_game_tables::equip_stats::EquipSlot as Disc;

    // Two weapons: one Vahn-only (mask 1), one anyone (mask 7).
    let mut inv = crate::item_bag::ItemBag::new();
    inv.insert(0x20, 1); // Vahn sword
    inv.insert(0x30, 1); // universal weapon

    let info = DiscEquipInfo::from_entries([
        (
            0x20,
            DiscEquipEntry {
                mask: 1,
                category: Disc::Weapon,
                is_ra_seru: false,
                passive_index: None,
            },
        ),
        (
            0x30,
            DiscEquipEntry {
                mask: 7,
                category: Disc::Weapon,
                is_ra_seru: false,
                passive_index: None,
            },
        ),
    ]);

    // Vahn (slot 0) sees both weapons in the weapon slot (0).
    let vahn = EquipSession::new_with_restrictions(
        fresh_record(),
        inv.clone(),
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info.clone(),
        0,
    );
    let v_weapons = vahn.items_for_slot(0);
    let mut v_ids: Vec<u8> = candidates(&v_weapons);
    v_ids.sort();
    assert_eq!(v_ids, vec![0x20, 0x30]);

    // Noa (slot 1) only sees the universal weapon (the Vahn-only one is
    // masked out).
    let noa = EquipSession::new_with_restrictions(
        fresh_record(),
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        1,
    );
    let n_weapons = noa.items_for_slot(0);
    let n_ids: Vec<u8> = candidates(&n_weapons);
    assert_eq!(n_ids, vec![0x30]);
}

#[test]
fn restrictions_gate_unambiguous_slot_by_disc_category() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_game_tables::equip_stats::EquipSlot as Disc;

    let mut inv = crate::item_bag::ItemBag::new();
    inv.insert(0x20, 1); // weapon
    inv.insert(0x40, 1); // body armor

    let info = DiscEquipInfo::from_entries([
        (
            0x20,
            DiscEquipEntry {
                mask: 7,
                category: Disc::Weapon,
                is_ra_seru: false,
                passive_index: None,
            },
        ),
        (
            0x40,
            DiscEquipEntry {
                mask: 7,
                category: Disc::Body,
                is_ra_seru: false,
                passive_index: None,
            },
        ),
    ]);
    let s = EquipSession::new_with_restrictions(
        fresh_record(),
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        0,
    );
    // UI slot 0 = Weapon -> only the weapon.
    let weapons = s.items_for_slot(0);
    assert_eq!(candidates(&weapons), vec![0x20]);
    // UI slot 2 = BodyArmor -> only the armor.
    let body = s.items_for_slot(2);
    assert_eq!(candidates(&body), vec![0x40]);
    // UI slot 0 must not surface the body armor.
    assert!(!weapons.iter().any(|i| i.id == 0x40));
}

#[test]
fn preview_reflects_equipment_spd_and_int() {
    // A footwear/head item that carries SPD + INT bonuses (the equipment
    // table's +4 / +0 bytes). The preview must move SPD/INT, not acc/eva.
    let mut inv = crate::item_bag::ItemBag::new();
    inv.insert(0x05, 1);
    let mut eq = EquipmentTable::new();
    eq.set(
        0x05,
        ItemModifier {
            spd: 6,
            int: 4,
            ..Default::default()
        },
    );
    let mut s = EquipSession::new(
        fresh_record(),
        inv,
        eq,
        StatusModifiers::default(),
        Vec::new(),
    );
    // Open slot 0's picker, then confirm item 0x05.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    // fresh_record base SPD=35, INT=18.
    assert_eq!(s.preview_stats.spd, 41);
    assert_eq!(s.preview_stats.int, 22);
    // Derived accuracy / evasion stay at the base (AGL-derived).
    assert_eq!(s.preview_stats.acc, 80);
    assert_eq!(s.preview_stats.eva, 20);
}

/// Retail has no prompt: the confirm writes the record on the frame of the
/// press (`0x801DA150..0x801DA1CC`) and the tail steps the hand to the next
/// slot row and hands the pad back to the browse (`0x801DA1DC..0x801DA254`).
#[test]
fn a_candidate_confirm_commits_at_once_and_steps_to_the_next_slot() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 2 });
    assert!(!s.is_done(), "the screen stays up");
    assert_eq!(s.record().equip[0], 0x05);
    assert_eq!(s.take_swaps(), vec![(0, 0, 0x05)]);
    assert!(s.swaps().is_empty(), "a drain hands each swap over once");
}

/// The tail's wrap is at 8 (`slti v0,s4,0x8`, `clear s4`): the last slot row's
/// confirm leaves the hand on Best Equipment.
#[test]
fn the_tail_wraps_the_hand_from_the_last_row_to_best_equipment() {
    let mut s = fresh_session();
    let slot = (0..EQUIP_SLOTS)
        .find(|&sl| browse_row_for_slot(sl) == SLOT_BROWSE_ROWS - 1)
        .expect("a slot sits on the last row");
    // The disc-free rule files an id under slot `id >> 5`.
    let id = (slot << 5) | 1;
    s.give_item(id, 1);
    s.equipment.set(id, ItemModifier::default());
    s.state = EquipState::ItemPicker { slot, cursor: 0 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 0 });
}

/// The worn row tests no class the confirm arm knows and goes straight to the
/// tail: no write, the hand on the next row. Remove on an empty slot buzzes
/// (`0x801DA128`) and jumps to the same tail.
#[test]
fn a_refused_confirm_still_runs_the_tail() {
    use crate::equipment::{DiscEquipEntry, DiscEquipInfo};
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let weapon = DiscEquipEntry {
        mask: 7,
        category: Disc::Weapon,
        is_ra_seru: false,
        passive_index: None,
    };
    let info = DiscEquipInfo::from_entries([(0x30, weapon), (0x20, weapon)]);
    let inv = crate::item_bag::ItemBag::from_slots(&[(0x20, 1)]);
    let mut rec = fresh_record();
    let cross = EquipInput {
        cross: true,
        ..Default::default()
    };
    // Remove on the empty slot.
    let mut s = EquipSession::new_with_restrictions(
        rec,
        inv.clone(),
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info.clone(),
        0,
    );
    s.state = EquipState::ItemPicker { slot: 0, cursor: 0 };
    s.input(cross);
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 2 });
    assert!(s.swaps().is_empty());
    assert!(s.drain_events().contains(&EquipEvent::InvalidConfirm));
    // The worn row: [Remove, worn 0x30, ...], the row the list opens on.
    rec.equip[0] = 0x30;
    let mut s = EquipSession::new_with_restrictions(
        rec,
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        0,
    );
    s.state = EquipState::ItemPicker { slot: 0, cursor: 1 };
    assert!(s.items_for_slot(0)[1].equipped);
    s.input(cross);
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 2 });
    assert!(s.swaps().is_empty());
    assert_eq!(s.record().equip[0], 0x30);
}

#[test]
fn commit_decrements_new_item_and_restores_old_inventory() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    // Pre-equip slot 0 with 0x05 to test the "restore old" path.
    s.give_item(0x06, 1);
    s.equipment.set(
        0x06,
        ItemModifier {
            atk: 7,
            ..Default::default()
        },
    );
    s.record.equip[0] = 0x05;
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    // The slot is occupied, so the list is [Remove, 0x05, 0x06], and it opens
    // on row 1, below Remove. One step down lands on 0x06.
    s.input(EquipInput {
        down: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.record().equip[0], 0x06);
    // Old item (0x05) returned to inventory: should now have qty 2.
    assert_eq!(s.inventory.get(&0x05).copied().unwrap_or(0), 2);
    // New item (0x06) used: qty was 1, now 0.
    assert_eq!(s.inventory.get(&0x06).copied().unwrap_or(0), 0);
}

#[test]
fn commit_recomputes_battle_stats() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    // ATK should now be base 50 + item 0x05's +5 = 55.
    assert_eq!(s.preview_stats.atk, 55);
}

#[test]
fn item_picker_circle_returns_to_slot_picker() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    s.input(EquipInput {
        circle: true,
        ..Default::default()
    });
    // Back on the slot's own browse row, which is `slot + 1`.
    assert!(matches!(s.state(), EquipState::SlotPicker { cursor: 1 }));
}

#[test]
fn drain_events_empties_buffer() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.input(EquipInput {
        down: true,
        ..Default::default()
    });
    let evs = s.drain_events();
    assert!(!evs.is_empty());
    assert!(s.drain_events().is_empty());
}

#[test]
fn item_picker_cursor_clamps_to_list_length() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.give_item(0x06, 1);
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    // Two items in slot 0 picker now.
    for _ in 0..10 {
        s.input(EquipInput {
            down: true,
            ..Default::default()
        });
    }
    match s.state() {
        EquipState::ItemPicker { cursor, .. } => assert_eq!(cursor, 1),
        _ => panic!("expected item picker"),
    }
}

#[test]
fn best_equipment_swaps_only_differing_in_bag_candidates() {
    // FUN_801cf760 law: per slot, skip when candidate == equipped or
    // the bag lacks the candidate; otherwise take one, return the old
    // item, count the change.
    let mut equips = [0u8; 8];
    equips[0] = 0x05; // already the best - must not move
    equips[1] = 0x25; // will be replaced by 0x26
    let mut inv = crate::item_bag::ItemBag::new();
    inv.insert(0x26, 1);
    // Slot 2's candidate 0x45 is NOT in the bag - skipped, no count.
    let changed = apply_best_equipment(
        &mut equips,
        [0x05, 0x26, 0x45, 0],
        ARMAMENT_ENGINE_SLOTS,
        &mut inv,
    );
    assert_eq!(changed, 1);
    assert_eq!(equips[0], 0x05);
    assert_eq!(equips[1], 0x26);
    assert_eq!(equips[2], 0);
    assert_eq!(inv.get(&0x26), None, "the last copy leaves a hole");
    // The displaced 0x25 went back to the bag.
    assert_eq!(inv.get(&0x25), Some(&1));
}

#[test]
fn slot_browse_row0_applies_best_and_other_rows_open_candidates() {
    let mut s = fresh_session();
    s.give_item(0x06, 1); // better slot-0 item in the bag
    match s.slot_browse_confirm(0, [0x06, 0, 0, 0]) {
        SlotBrowseOutcome::BestEquipApplied(1) => {}
        other => panic!("expected one swap, got {other:?}"),
    }
    assert_eq!(s.record().equip[0], 0x06);
    // Nothing left to improve: retail buzzes 0x23.
    assert_eq!(
        s.slot_browse_confirm(0, [0x06, 0, 0, 0]),
        SlotBrowseOutcome::BestEquipNothing
    );
    // A slot row opens the candidate list for row - 1.
    assert_eq!(
        s.slot_browse_confirm(3, [0, 0, 0, 0]),
        SlotBrowseOutcome::OpenCandidates { slot: 2 }
    );
}

#[test]
fn unequip_returns_item_to_bag_and_refuses_empty_slot() {
    let mut s = fresh_session();
    // Park on slot row 0 (browse row 1); row 0 is Best Equipment.
    s.state = EquipState::SlotPicker { cursor: 1 };
    // Equip 0x05 into slot 0 first (bag holds one).
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.record().equip[0], 0x05);
    // The last one taken empties the slot whole (`FUN_80043048`).
    assert_eq!(s.inventory().get(&0x05), None);

    // Remove row: item returns to the bag, slot clears.
    let mut s2 = s.clone();
    assert_eq!(s2.unequip(0), Some(0x05));
    assert_eq!(s2.record().equip[0], 0);
    assert_eq!(s2.inventory().get(&0x05), Some(&1));
    // Already-empty slot refuses (retail buzz 0x37, no commit).
    assert_eq!(s2.unequip(1), None);
}

/// The applier writes the armament's *record* slot, not its own loop
/// index. Retail reaches it through `DAT_801E43E8`; the engine's
/// equivalent has to skip the Hand Guard slot, so footwear lands on
/// engine slot 4 and slot 3 is never touched by Best Equipment.
#[test]
fn best_equipment_writes_footwear_to_the_boot_slot_not_the_hand_guard() {
    let mut equips = [0u8; 8];
    let mut inv = crate::item_bag::ItemBag::new();
    inv.insert(0x60, 1);
    let changed = apply_best_equipment(
        &mut equips,
        [0, 0, 0, 0x60],
        ARMAMENT_ENGINE_SLOTS,
        &mut inv,
    );
    assert_eq!(changed, 1);
    assert_eq!(equips[3], 0, "hand guard must stay untouched");
    assert_eq!(equips[4], 0x60, "footwear belongs to the boot slot");
}

/// The candidate list's Remove row exists exactly while the slot holds
/// something, and confirming it runs the retail Remove commit.
#[test]
fn remove_row_appears_only_on_an_occupied_slot_and_confirming_it_unequips() {
    let mut s = fresh_session();
    assert_eq!(
        s.items_for_slot(0).first().map(|i| i.id),
        Some(0x05),
        "an empty slot has no Remove row"
    );
    s.record.equip[0] = 0x05;
    let rows = s.items_for_slot(0);
    assert_eq!(rows[0].id, REMOVE_ROW_ID);
    assert!(rows[0].owned, "the Remove row is always confirmable");

    s.state = EquipState::ItemPicker { slot: 0, cursor: 0 };
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.record().equip[0], 0);
    assert_eq!(s.take_swaps(), vec![(0, 0x05, 0)]);
    assert_eq!(s.state(), EquipState::SlotPicker { cursor: 2 });
}

/// Row 0's confirm has to compute the candidates first - the engine's
/// producer is `best_equipment_now`, and it must find a strictly better
/// bag weapon the same way `FUN_801CF88C` does.
#[test]
fn best_equipment_now_seeds_from_the_worn_armaments_and_beats_them() {
    let mut s = fresh_session();
    s.record.equip[0] = 0x05; // worn weapon, ATK +5
    s.give_item(0x06, 1);
    s.equipment.set(
        0x06,
        ItemModifier {
            atk: 40,
            ..Default::default()
        },
    );
    assert_eq!(s.best_equipment_now()[0], 0x06);
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert_eq!(s.record().equip[0], 0x06);
    assert_eq!(s.inventory().get(&0x05), Some(&2));
}

#[test]
fn preview_candidate_is_a_trial_that_leaves_the_record_alone() {
    let mut s = fresh_session();
    let base_atk = s.preview_stats.atk;
    s.preview_candidate(0, 0x05); // +5 atk modifier from fresh_session
    assert_eq!(s.preview_stats.atk, base_atk + 5);
    // The trial restored the record - nothing equipped.
    assert_eq!(s.record().equip[0], 0);
    // Remove-row preview (candidate 0) lands back on the bare stats.
    s.preview_candidate(0, 0);
    assert_eq!(s.preview_stats.atk, base_atk);
}

#[test]
fn armament_slot_permutes_the_raw_slot_bits() {
    assert_eq!(armament_slot_of(0x00), 2); // body
    assert_eq!(armament_slot_of(0x20), 1); // head
    assert_eq!(armament_slot_of(0x40), 0); // weapon
    assert_eq!(armament_slot_of(0x60), 3); // footwear
    // The Ra-Seru bit and anything else outside 0x60 is ignored.
    assert_eq!(armament_slot_of(0x41), 0);
}

fn cand(kind: u8, attack: u8, defence: (u8, u8), slot_bits: u8) -> EquipCandidate {
    EquipCandidate {
        kind,
        attack,
        defence,
        char_mask: 7,
        slot_bits,
    }
}

#[test]
fn best_equipment_fills_empty_slots_with_the_first_eligible_item() {
    let table = |id: u8| match id {
        0x10 => Some(cand(1, 4, (0, 0), 0x40)),
        0x11 => Some(cand(1, 9, (0, 0), 0x40)),
        _ => None,
    };
    // Empty weapon slot: the first eligible bag entry takes it outright,
    // then the stronger one displaces it on the comparison.
    let best = best_equipment_candidates([0, 0, 0, 0], 7, &[(0x10, 1), (0x11, 1)], table, |_| 0);
    assert_eq!(best[0], 0x11);
    // Reversed bag order: the weaker one never displaces the stronger.
    let best = best_equipment_candidates([0, 0, 0, 0], 7, &[(0x11, 1), (0x10, 1)], table, |_| 0);
    assert_eq!(best[0], 0x11);
}

#[test]
fn armour_ranks_on_udf_plus_ldf_only() {
    // 0x20 is pure INT/SPD as far as this scan is concerned (both
    // defence fields zero) and must not displace the incumbent 0x21.
    let table = |id: u8| match id {
        0x20 => Some(cand(1, 200, (0, 0), 0x20)),
        0x21 => Some(cand(1, 0, (1, 0), 0x20)),
        _ => None,
    };
    let best = best_equipment_candidates([0, 0x21, 0, 0], 7, &[(0x20, 1)], table, |_| 0);
    assert_eq!(best[1], 0x21);
    // A single point of LDF is enough to win.
    let table2 = |id: u8| match id {
        0x20 => Some(cand(1, 0, (1, 1), 0x20)),
        0x21 => Some(cand(1, 0, (1, 0), 0x20)),
        _ => None,
    };
    let best = best_equipment_candidates([0, 0x21, 0, 0], 7, &[(0x20, 1)], table2, |_| 0);
    assert_eq!(best[1], 0x20);
}

#[test]
fn weapon_category_score_dominates_raw_attack() {
    let table = |id: u8| match id {
        0x30 => Some(cand(1, 255, (0, 0), 0x40)),
        0x31 => Some(cand(1, 1, (0, 0), 0x40)),
        _ => None,
    };
    // 0x31 is category-favoured; its flat 1000 beats 0x30's 255 ATK.
    let score = |id: u8| {
        if id == 0x31 {
            crate::menu_item_category::CATEGORY_MATCH_SCORE
        } else {
            0
        }
    };
    let best = best_equipment_candidates([0x30, 0, 0, 0], 7, &[(0x31, 1)], table, score);
    assert_eq!(best[0], 0x31);
}

#[test]
fn best_equipment_skips_non_equipment_wrong_character_and_empty_stacks() {
    let table = |id: u8| match id {
        0x40 => Some(cand(2, 99, (9, 9), 0x40)), // not equipment
        0x41 => Some(EquipCandidate {
            char_mask: 2, // Noa-only
            ..cand(1, 99, (9, 9), 0x40)
        }),
        0x42 => Some(cand(1, 99, (9, 9), 0x40)),
        _ => None,
    };
    let bag = [(0x40, 1), (0x41, 1), (0x42, 0), (0, 5)];
    let best = best_equipment_candidates([0, 0, 0, 0], 1, &bag, table, |_| 0);
    assert_eq!(best, [0, 0, 0, 0]);
}

/// The armament list ranks best first: a favoured weapon over any other, the
/// worn row among them, the Remove verb fixed on top. `0x80031614..0x800316E0`.
#[test]
fn armament_rows_rank_on_the_key_with_the_worn_row_among_them() {
    use crate::equipment::DiscEquipInfo;
    use legaia_game_tables::equip_stats::EquipStatTable;
    // [INT, ATK, UDF, LDF, SPD, passive, mask, +7 slot bits].
    let weapon = |atk: u8| [0, atk, 0, 0, 0, 0x40, 7, 0x40];
    let table = EquipStatTable::from_entries(&[
        (0x10, weapon(5)),
        (0x11, weapon(50)),
        (0x12, weapon(20)),
        (0x13, weapon(1)),
    ]);
    let info = DiscEquipInfo::from_disc(&table);
    let mut inv = crate::item_bag::ItemBag::new();
    for id in [0x10, 0x11, 0x13] {
        inv.insert(id, 1);
    }
    let mut record = fresh_record();
    record.equip[0] = 0x12;
    let s = EquipSession::new_with_restrictions(
        record,
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        0,
    )
    .with_weapon_category(vec![crate::menu_item_category::CategoryEntry {
        item_id: 0x13,
        mask: 0x01,
    }]);
    let ids: Vec<u8> = s.items_for_slot(0).iter().map(|r| r.id).collect();
    assert_eq!(ids, [REMOVE_ROW_ID, 0x13, 0x11, 0x12, 0x10]);
    assert!(s.items_for_slot(0)[3].equipped);
}

/// The candidate step opens on the row under Remove, and on Remove when the
/// list holds nothing else (`FUN_80030104` clamps the selection).
#[test]
fn the_candidate_list_opens_below_remove() {
    let mut s = fresh_session();
    s.state = EquipState::SlotPicker { cursor: 1 };
    s.record.equip[0] = 0x05;
    s.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(
        s.state(),
        EquipState::ItemPicker { slot: 0, cursor: 1 }
    ));
}
