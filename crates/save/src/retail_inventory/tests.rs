use super::*;

fn default_inv() -> RetailInventory {
    RetailInventory::new(ITEM_WINDOW_BASE, ITEM_WINDOW_SLOTS)
}

#[test]
fn oob_target_is_sc_plus_0x18a8() {
    assert_eq!(oob_target(ITEM_WINDOW_BASE, ITEM_WINDOW_SLOTS), 0x8008_59E8);
    // SC base = ITEM_WINDOW_BASE - 0x1818; OOB target = SC + 0x18A8.
    let sc = ITEM_WINDOW_BASE - 0x1818;
    assert_eq!(0x8008_59E8, sc + 0x18A8);
}

#[test]
fn merge_caps_at_99() {
    let mut inv = default_inv();
    assert_eq!(inv.add(0x10, 50), AddOutcome::Placed { slot: 0 });
    assert_eq!(
        inv.add(0x10, 80),
        AddOutcome::Merged {
            slot: 0,
            new_count: STACK_CAP
        }
    );
    assert_eq!(inv.find_count(0x10), 99);
}

#[test]
fn add_to_slot_saturates_and_rejects_empty_or_out_of_window() {
    let mut inv = default_inv();
    assert_eq!(inv.add(0x10, 50), AddOutcome::Placed { slot: 0 });
    assert_eq!(inv.add_to_slot(0, 10), Some(60));
    // Saturates at the cap rather than wrapping the count byte.
    assert_eq!(inv.add_to_slot(0, 200), Some(STACK_CAP));
    // An empty slot returns 0 and stores nothing - retail's id-byte test.
    assert_eq!(inv.add_to_slot(1, 5), None);
    assert_eq!(inv.slots()[1], (0, 0));
    // Past the active window: retail's bound check against `gp[+0x2D4]`.
    let past = inv.slots().len();
    assert_eq!(inv.add_to_slot(past, 5), None);
}

#[test]
fn add_places_in_lowest_empty_slot() {
    let mut inv = default_inv();
    assert_eq!(inv.add(0x10, 1), AddOutcome::Placed { slot: 0 });
    assert_eq!(inv.add(0x11, 2), AddOutcome::Placed { slot: 1 });
    assert_eq!(inv.add(0x12, 3), AddOutcome::Placed { slot: 2 });
    // Consuming slot 1 to zero leaves a HOLE (retail FUN_80042310 zeroes
    // the id in place and does not compact), so the next add reuses it.
    assert!(inv.consume(0x11, 2));
    assert_eq!(inv.slots()[1], (0, 0));
    assert_eq!(inv.slots()[2], (0x12, 3), "slot 2 must not renumber");
    assert_eq!(inv.add(0x13, 4), AddOutcome::Placed { slot: 1 });
}

#[test]
fn full_bag_add_returns_oob_and_does_not_mutate() {
    // Fill every slot with distinct non-zero ids.
    let slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
        .map(|i| ((i as u8).wrapping_add(1), 5))
        .collect();
    let mut inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots.clone());
    let before = inv.clone();
    let outcome = inv.add(0xFE, 7);
    assert_eq!(
        outcome,
        AddOutcome::OobIdWrite {
            oob_target: 0x8008_59E8,
            written_id: 0xFE,
        }
    );
    // No count applied, no slot mutated, no panic.
    assert_eq!(inv, before);
    assert_eq!(inv.slots(), &slots[..]);
}

#[test]
fn full_bag_with_matching_id_still_merges() {
    // A full bag whose ids include the target id merges (no OOB).
    let mut slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
        .map(|i| ((i as u8).wrapping_add(1), 5))
        .collect();
    slots[3] = (0x40, 10);
    let mut inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots);
    assert_eq!(
        inv.add(0x40, 3),
        AddOutcome::Merged {
            slot: 3,
            new_count: 13
        }
    );
}

#[test]
fn item_window_selector_matches_fun_8004313c() {
    // 0 members: retail returns before touching gp[+0x2D2]/[+0x2D4].
    assert_eq!(ItemWindow::select(0, true, false), None);
    // A solo party gates on story flag 20; when clear, SC+0x458 picks the
    // half.
    assert_eq!(ItemWindow::select(1, true, true), Some(ItemWindow::Full));
    assert_eq!(ItemWindow::select(1, false, false), Some(ItemWindow::Low));
    assert_eq!(ItemWindow::select(1, false, true), Some(ItemWindow::High));
    // Two or more members skips the flag test entirely - the live
    // battle-state read (3 members, window (0, 256, 256)).
    assert_eq!(ItemWindow::select(2, false, true), Some(ItemWindow::Full));
    assert_eq!(ItemWindow::select(3, false, true), Some(ItemWindow::Full));

    assert_eq!(ItemWindow::Full.bounds(), (0, 256));
    assert_eq!(ItemWindow::Low.bounds(), (0, 128));
    assert_eq!(ItemWindow::High.bounds(), (128, 256));
    assert_eq!(ItemWindow::High.len(), 128);

    // The full-window OOB lands past slot 255, not at the old slot-72
    // address the earlier reading recorded.
    assert_eq!(
        oob_target(ITEM_WINDOW_BASE, ItemWindow::Full.len()),
        0x8008_5B58
    );
    assert_eq!(
        oob_target(ITEM_WINDOW_BASE, ItemWindow::Low.len()),
        0x8008_5A58
    );
}

#[test]
fn normalize_merges_duplicate_stacks_and_squeezes() {
    // Two stacks of the same id plus a hole between them.
    let mut inv = RetailInventory::from_slots(
        ITEM_WINDOW_BASE,
        vec![(0x30, 40), (0, 0), (0x31, 2), (0x30, 5), (0, 0)],
    );
    inv.normalize();
    // The later 0x30 stack folds into the earlier one; survivors squeeze.
    assert_eq!(inv.slots()[0], (0x30, 45));
    assert_eq!(inv.slots()[1], (0x31, 2));
    assert_eq!(inv.slots()[2], (0, 0));
    assert_eq!(inv.slots()[3], (0, 0));
}

#[test]
fn normalize_caps_merged_stacks_and_keeps_live_zero_counts() {
    let mut inv = RetailInventory::from_slots(
        ITEM_WINDOW_BASE,
        vec![(0x40, 90), (0x40, 30), (0x41, 0), (0, 0)],
    );
    inv.normalize();
    assert_eq!(inv.slots()[0], (0x40, STACK_CAP), "merge clamps at 99");
    // Retail's occupancy test is `id != 0` alone: a live id with a zero
    // count survives normalization.
    assert_eq!(inv.slots()[1], (0x41, 0));
    assert_eq!(inv.slots()[2], (0, 0));
}

#[test]
fn find_slot_and_count_bounded() {
    let mut inv = default_inv();
    assert_eq!(inv.find_slot(0x20), None);
    assert_eq!(inv.find_count(0x20), 0);
    // id 0 is the empty sentinel: never matched.
    assert_eq!(inv.find_slot(0), None);
    inv.add(0x20, 9);
    assert_eq!(inv.find_slot(0x20), Some(0));
    assert_eq!(inv.find_count(0x20), 9);
}

#[test]
fn consume_to_zero_leaves_a_hole_in_place() {
    let mut inv = default_inv();
    inv.add(0xA0, 1);
    inv.add(0xA1, 4);
    inv.add(0xA2, 2);
    // Consume the middle stack to zero. Retail (FUN_80042310) zeroes the
    // id byte in place and returns - the survivors keep their slot indices.
    assert!(inv.consume(0xA1, 4));
    assert_eq!(inv.slots()[0], (0xA0, 1));
    assert_eq!(inv.slots()[1], (0, 0));
    assert_eq!(inv.slots()[2], (0xA2, 2));
    assert_eq!(inv.find_slot(0xA1), None);
    // The squeeze is a separate entry point (FUN_800423E0).
    let mut squeezed = inv.clone();
    squeezed.normalize();
    assert_eq!(squeezed.slots()[1], (0xA2, 2));
    // Partial consume leaves the stack in place.
    assert!(inv.consume(0xA2, 1));
    assert_eq!(inv.find_count(0xA2), 1);
    // Consuming an absent id is a no-op false.
    assert!(!inv.consume(0xFF, 1));
}

#[test]
fn consume_by_slot_zeroes_in_place_without_compacting() {
    let mut inv = default_inv();
    inv.add(0xB0, 3);
    inv.add(0xB1, 5);
    inv.add(0xB2, 2);
    // Partial slot-consume leaves the stack and order intact.
    assert_eq!(inv.consume_slot(1, 2, 0), 3);
    assert_eq!(inv.slots()[1], (0xB1, 3));
    // Consume slot 1 to zero: the id byte is zeroed IN PLACE - unlike
    // consume-by-id, the window is NOT compacted, so the hole remains and
    // the trailing slot keeps its index.
    assert_eq!(inv.consume_slot(1, 3, 0), 0);
    assert_eq!(inv.slots()[0], (0xB0, 3));
    assert_eq!(inv.slots()[1], (0, 0));
    assert_eq!(inv.slots()[2], (0xB2, 2));
    // Over-consume clamps the count at 0 (and zeroes the id).
    assert_eq!(inv.consume_slot(0, 99, 0), 0);
    assert_eq!(inv.slots()[0], (0, 0));
}

#[test]
fn consume_by_slot_noop_paths_echo_third_arg() {
    let mut inv = default_inv();
    inv.add(0xC0, 4);
    // Out-of-range slot index: no mutation, echoes the third argument.
    assert_eq!(inv.consume_slot(ITEM_WINDOW_SLOTS as i16, 1, 0x7A), 0x7A);
    assert_eq!(inv.consume_slot(-1, 1, 0x55), 0x55);
    // Already-empty slot (id == 0): same echo, no mutation.
    assert_eq!(inv.consume_slot(5, 1, 0x33), 0x33);
    assert_eq!(inv.find_count(0xC0), 4);
}

/// A full window with all-distinct ids: the next add can never merge or
/// place, so it always surfaces the OOB primitive.
fn full_distinct_bag() -> RetailInventory {
    // ids 1..=72 - all non-zero (no empty slot) and none equal to the test
    // ids below (0x80+), so neither the merge nor free-slot pass succeeds.
    let slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
        .map(|i| ((i as u8).wrapping_add(1), 5))
        .collect();
    RetailInventory::from_slots(ITEM_WINDOW_BASE, slots)
}

#[test]
fn oob_write_carries_the_added_id_as_value() {
    // The written byte is the *added item's id*, not qty - this is the
    // attacker-influenced value of the primitive.
    for id in [0x80u8, 0xCD, 0xFE, 0xFF] {
        let mut inv = full_distinct_bag();
        assert_eq!(
            inv.add(id, 1),
            AddOutcome::OobIdWrite {
                oob_target: 0x8008_59E8,
                written_id: id,
            }
        );
    }
}

#[test]
fn oob_write_is_independent_of_quantity() {
    // The shop buy-confirm path adds a variable quantity, but only the id
    // store is unguarded; qty never reaches the OOB store. So the outcome
    // is identical for qty = 1 and qty = 99.
    let mut inv1 = full_distinct_bag();
    let mut inv99 = full_distinct_bag();
    assert_eq!(
        inv1.add(0x90, 1),
        AddOutcome::OobIdWrite {
            oob_target: 0x8008_59E8,
            written_id: 0x90,
        }
    );
    assert_eq!(inv1.add(0x90, 1), inv99.add(0x90, 99));
}

#[test]
fn add_helper_caller_catalogue_is_complete_and_distinct() {
    // The five reverse-engineered unchecked call sites, each with its
    // documented source address. Guards against silent drift if a site is
    // added/removed without updating ALL.
    assert_eq!(AddHelperCaller::ALL.len(), 5);
    let addrs: Vec<u32> = AddHelperCaller::ALL
        .iter()
        .map(|c| c.source_addr())
        .collect();
    assert_eq!(
        addrs,
        vec![
            0x8004_E568,
            0x801C_36B0,
            0x801D_ABA4,
            0x801D_0F60,
            0x8020_E748
        ]
    );
    // All distinct.
    let mut sorted = addrs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), addrs.len());
}

#[test]
fn every_known_caller_reaches_the_oob_on_a_full_bag() {
    // Model each caller's full-bag add with a representative written id and
    // assert the primitive fires at the fixed key-item address. The value
    // column reflects each site's control over the written byte (per the
    // `AddHelperCaller` docs): shop = catalog id, loot = drop id, dead-slot
    // refund = committed item id, minigame = fixed 0xCD, swap-back refund =
    // displaced equip id.
    // Representative ids chosen > ITEM_WINDOW_SLOTS (72) so they don't
    // collide with full_distinct_bag's 1..=72 fill (which would merge).
    let site_ids = [
        (AddHelperCaller::BattleLoot, 0x80u8),
        (AddHelperCaller::ShopBuyConfirm, 0x91),
        (AddHelperCaller::DeadSlotItemRefund, 0xA5),
        (AddHelperCaller::MinigameReward, 0xCD),
        (AddHelperCaller::EquipSwapBackRefund, 0xB0),
    ];
    for (caller, id) in site_ids {
        let mut inv = full_distinct_bag();
        assert_eq!(
            inv.add(id, 1),
            AddOutcome::OobIdWrite {
                oob_target: 0x8008_59E8,
                written_id: id,
            },
            "caller {caller:?} (FUN_{:08X}) should reach the OOB on a full bag",
            caller.source_addr(),
        );
    }
}

#[test]
fn full_window_oob_is_unreachable_by_id_ceiling() {
    // The full [0, 256) window has 256 slots but only 255 distinct non-zero
    // ids exist (id 0 is the empty sentinel), and FUN_800421D4's merge pass
    // keeps at most one slot per id - so a hole always remains and the OOB
    // exit (a2 == end at 0x800422BC) can never be taken by the add path.
    assert_eq!(ItemWindow::Full.len(), 256);
    assert!(ItemWindow::Full.len() as u16 > MAX_DISTINCT_ITEM_IDS);
    assert_eq!(
        ItemWindow::Full.oob_reachability(),
        OobReachability::Unreachable,
    );
}

#[test]
fn half_windows_are_gated_by_selector_state_not_the_id_ceiling() {
    // 128 <= 255, so the id ceiling alone does not forbid a fill; retail
    // only installs these in the single-member / flag-20-clear state, whose
    // inventory obtainable during the solo phase is far below 128 (a progress
    // bound - the item table itself carries 250 named ids).
    for w in [ItemWindow::Low, ItemWindow::High] {
        assert_eq!(w.len(), 128);
        assert!(w.len() as u16 <= MAX_DISTINCT_ITEM_IDS);
        assert_eq!(w.oob_reachability(), OobReachability::GatedBySelectorState);
    }
}

/// The reachability verdict, exercised as a data assertion: build the
/// densest full `[0, 256)` window the add path could ever produce - every
/// one of the 255 distinct non-zero ids present exactly once - and show that
/// slot 255 is a forced hole, so no add of any real id `1..=255` reaches the
/// OOB (each either merges or places into the hole). The mechanical
/// counterpart to `ItemWindow::Full.oob_reachability() == Unreachable`.
#[test]
fn full_window_built_from_every_id_still_has_a_hole() {
    // ids 1..=255 in slots 0..=254; slot 255 is empty because there is no
    // 256th distinct non-zero id to occupy it.
    let slots: Vec<(u8, u8)> = (0..ItemWindow::Full.len())
        .map(|i| {
            let id = u8::try_from(i + 1).unwrap_or(0); // i == 255 -> id 0 (hole)
            (id, u8::from(id != 0))
        })
        .collect();
    assert_eq!(slots.len(), 256);
    assert_eq!(
        slots.iter().filter(|&&(id, _)| id == 0).count(),
        1,
        "exactly one forced hole"
    );
    assert_eq!(slots[255], (0, 0), "the hole is the last slot");

    let inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots);
    for id in 1u8..=255 {
        let outcome = inv.clone().add(id, 1);
        assert!(
            !matches!(outcome, AddOutcome::OobIdWrite { .. }),
            "id {id:#04x} must never reach the OOB in a 256-window built from real ids: {outcome:?}",
        );
    }
}

// ---------------------------------------------------------------
// The active window
// ---------------------------------------------------------------

/// A 256-slot array with one item in each half, so every window arm has
/// something to find and something to miss.
fn split_bag() -> RetailInventory {
    let mut slots = vec![(0u8, 0u8); ITEM_SLOTS_TOTAL];
    slots[3] = (0x77, 5);
    slots[200] = (0x78, 4);
    RetailInventory::from_slots(ITEM_WINDOW_BASE, slots)
}

#[test]
fn find_is_bounded_by_the_installed_window() {
    let mut inv = split_bag();
    assert_eq!(inv.find_slot(0x77), Some(3));
    assert_eq!(inv.find_slot(0x78), Some(200));

    inv.set_window(ItemWindow::Low);
    assert_eq!(inv.find_slot(0x77), Some(3));
    assert_eq!(inv.find_slot(0x78), None, "the high half is out of bounds");
    assert_eq!(inv.find_count(0x78), 0);

    inv.set_window(ItemWindow::High);
    assert_eq!(inv.find_slot(0x77), None, "the low half is out of bounds");
    assert_eq!(inv.find_slot(0x78), Some(200));
}

#[test]
fn consume_outside_the_window_returns_the_sentinel_and_stores_nothing() {
    let mut inv = split_bag();
    inv.set_window(ItemWindow::Low);
    assert_eq!(inv.consume_returning_slot(0x78, 1), NOT_IN_WINDOW);
    assert_eq!(
        inv.slots()[200],
        (0x78, 4),
        "the out-of-window slot is untouched"
    );
    // In window: the slot index comes back and the count drops.
    assert_eq!(inv.consume_returning_slot(0x77, 1), 3);
    assert_eq!(inv.slots()[3], (0x77, 4));
}

#[test]
fn add_places_into_the_windows_own_free_slots() {
    let mut inv = split_bag();
    inv.set_window(ItemWindow::High);
    // Slot 0 is free, but it is not in this window.
    assert_eq!(inv.add(0x79, 1), AddOutcome::Placed { slot: 128 });
    assert_eq!(inv.slots()[0], (0, 0));
    assert_eq!(inv.slots()[128], (0x79, 1));
}

#[test]
fn normalize_leaves_the_other_half_alone() {
    let mut inv = split_bag();
    // Two stacks of the same id in the low half, plus the high-half item.
    inv.set_window(ItemWindow::Full);
    assert_eq!(
        inv.add(0x77, 3),
        AddOutcome::Merged {
            slot: 3,
            new_count: 8
        }
    );
    inv.set_window(ItemWindow::Low);
    inv.normalize();
    assert_eq!(inv.slots()[0], (0x77, 8), "the low half squeezed to slot 0");
    assert_eq!(
        inv.slots()[200],
        (0x78, 4),
        "the high half kept its slot through a low-half normalize"
    );
}

#[test]
fn the_oob_target_follows_the_window_not_the_array() {
    let mut inv = split_bag();
    inv.set_window(ItemWindow::Low);
    assert_eq!(inv.oob_target(), 0x8008_5A58);
    inv.set_window(ItemWindow::Full);
    assert_eq!(inv.oob_target(), 0x8008_5B58);
}
