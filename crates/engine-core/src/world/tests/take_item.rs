//! Field-VM op `0x4C` n5 sub-2 **TAKE_ITEM** against the world host.
//!
//! Driven through `load_field_script` + `tick`, so these exercise the real
//! dispatch, not the override in isolation: the arm is only reached if the
//! `0x4C` outer-nibble-5 / sub-2 decode still lands where the overlay's
//! two-level table says it does.
//!
//! Retail arm (`0x801E1ABC`, disassembly): consume one of the operand id from
//! the bag, and **only** on the not-found sentinel take it off whoever is
//! wearing it. Both halves are asserted, and so is the order between them -
//! see [`take_item_prefers_the_bag_over_the_worn_copy`], which is the one an
//! implementation that unequips unconditionally still fails.

use super::*;

/// `[4C, 52, item_id]` followed by a HALT, so the VM stops after the arm
/// instead of running off the end of the buffer.
fn take_script(item_id: u8) -> Vec<u8> {
    vec![0x4C, 0x52, item_id, 0x00]
}

/// A party whose members carry the given accessory ("Goods") triples in
/// equipment slots `5..8`. Mirrors `equipment::tests::party_with_goods`.
fn party_with_goods(goods: &[[u8; 3]]) -> legaia_save::Party {
    let mut p = legaia_save::Party::zeroed(goods.len());
    for (m, g) in p.members.iter_mut().zip(goods) {
        let mut eq = m.equipment();
        eq.slots[5..8].copy_from_slice(g);
        m.set_equipment(eq);
    }
    p
}

fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w
}

/// The bag path: one copy leaves the stack and the rest stays.
#[test]
fn take_item_consumes_one_from_the_bag() {
    let mut world = field_world();
    world.inventory.insert(0x42, 3);
    world.load_field_script(take_script(0x42));
    let _ = world.tick();
    assert_eq!(
        world.inventory.get(&0x42).copied(),
        Some(2),
        "TAKE_ITEM should decrement the stack by one"
    );
}

/// The last copy clears the entry rather than leaving a zero-count stack -
/// retail zeroes the slot's id byte when the count reaches 0
/// (`FUN_80042310`), and the world's map models an absent id as no key.
#[test]
fn taking_the_last_copy_clears_the_bag_entry() {
    let mut world = field_world();
    world.inventory.insert(0x42, 1);
    world.load_field_script(take_script(0x42));
    let _ = world.tick();
    assert!(
        !world.inventory.contains_key(&0x42),
        "the last copy should leave no entry, got {:?}",
        world.inventory.get(&0x42)
    );
}

/// The fallback: the bag does not hold it, so it comes off the character who
/// is wearing it. Without the world-side override the item stays equipped and
/// the script's precondition silently fails.
#[test]
fn take_item_unequips_a_worn_accessory_when_the_bag_misses() {
    let mut world = field_world();
    world.roster = party_with_goods(&[[0x11, 0x12, 0x13], [0, 0x99, 0]]);
    world.load_field_script(take_script(0x99));
    let _ = world.tick();
    assert_eq!(
        world.roster.members[1].equipment().slots[5..8],
        [0, 0, 0],
        "a bag miss should unequip the worn copy"
    );
    assert_eq!(
        world.roster.members[0].equipment().slots[5..8],
        [0x11, 0x12, 0x13],
        "the scan stops at the first match"
    );
}

/// **The direction test.** `0x100` is the consume primitive's *not-found*
/// sentinel, so the unequip is a fallback and not a second effect. With the
/// id both in the bag and on a character, only the bag copy may go.
///
/// An implementation that unequips unconditionally - or that reads the
/// sentinel as "consume finished" and runs both - passes every other
/// assertion in this file and fails this one.
#[test]
fn take_item_prefers_the_bag_over_the_worn_copy() {
    let mut world = field_world();
    world.inventory.insert(0x42, 1);
    world.roster = party_with_goods(&[[0x42, 0, 0]]);
    world.load_field_script(take_script(0x42));
    let _ = world.tick();
    assert!(
        !world.inventory.contains_key(&0x42),
        "the bag copy is the one that goes"
    );
    assert_eq!(
        world.roster.members[0].equipment().slots[5],
        0x42,
        "a bag hit must leave equipment alone"
    );
}

/// A miss with nothing to fall back on is not an error and must not stall:
/// the PC advances by 3 on both retail paths (the delta sits in the call's
/// branch-delay slot), so the script runs on to its HALT.
#[test]
fn take_item_with_nothing_to_take_still_advances() {
    let mut world = field_world();
    world.roster = party_with_goods(&[[0x11, 0x12, 0x13]]);
    // `[4C, 52, 0x99]` then GIVE_ITEM 7, then HALT. The give only runs if the
    // take advanced instead of halting in place, so the bag entry is the
    // evidence that the arm is 3 bytes wide and non-blocking.
    world.load_field_script(vec![0x4C, 0x52, 0x99, 0x39, 0x07, 0x00]);
    for _ in 0..4 {
        let _ = world.tick();
    }
    assert_eq!(
        world.inventory.get(&0x07).copied(),
        Some(1),
        "the instruction after TAKE_ITEM should have run"
    );
    assert_eq!(
        world.roster.members[0].equipment().slots[5..8],
        [0x11, 0x12, 0x13],
        "nothing carried the id, so nothing is unequipped"
    );
}
