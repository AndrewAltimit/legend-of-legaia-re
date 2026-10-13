//! Field-VM op `0x3B` **SET_EQUIP_SLOT** against the world host.
//!
//! Retail arm (`0x801E07DC..0x801E081C`, disassembly): the operand byte's
//! high nibble picks the character record and its low nibble the equipment
//! byte, the store lands in the delay slot of the `FUN_80042558` call that
//! rebuilds the record's stats, and the bag is never read or written. Driven
//! through `load_field_script` + `tick`, so the real dispatch is exercised.

use super::*;
use legaia_engine_mode::mode::SceneMode;

fn field_world(members: usize) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.load_party(legaia_save::Party::zeroed(members));
    w
}

/// `3B 03 01`: the first record's slot 3 takes item `0x01`, and nothing
/// lands in the bag - the op is not an inventory write.
#[test]
fn set_equip_slot_writes_the_record_and_leaves_the_bag_alone() {
    let mut world = field_world(1);
    world.load_field_script(vec![0x3B, 0x03, 0x01, 0x00]);
    let _ = world.tick();
    assert_eq!(world.party.roster.members[0].equipment().slots[3], 0x01);
    assert!(
        world.party.inventory.iter().all(|(_, &n)| n == 0),
        "SET_EQUIP_SLOT must not touch the bag"
    );
}

/// The high nibble addresses the record: `3B 12 11` is the second record's
/// slot 2, and the first record is untouched.
#[test]
fn set_equip_slot_addresses_the_record_by_the_high_nibble() {
    let mut world = field_world(2);
    world.load_field_script(vec![0x3B, 0x12, 0x11, 0x00]);
    let _ = world.tick();
    assert_eq!(world.party.roster.members[1].equipment().slots[2], 0x11);
    assert_eq!(world.party.roster.members[0].equipment().slots, [0; 8]);
}

/// A later stage replaces the earlier one in place; the replaced item does
/// not come back to the bag.
#[test]
fn a_second_write_replaces_the_slot_without_returning_the_old_item() {
    let mut world = field_world(1);
    world.load_field_script(vec![0x3B, 0x03, 0x01, 0x3B, 0x03, 0x02, 0x00]);
    let _ = world.tick();
    assert_eq!(world.party.roster.members[0].equipment().slots[3], 0x02);
    assert!(world.party.inventory.iter().all(|(_, &n)| n == 0));
}
