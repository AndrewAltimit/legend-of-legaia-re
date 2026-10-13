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

/// The aggregator call the store rides in rebuilds the record's live HP / MP
/// maxima from its base window under the new equipment (`FUN_80042558`: a
/// tenth of the base for ability bit 0, a quarter for bit 1, the same two
/// for MP on bits 2 and 3, then the current values clamped down). Putting a
/// Max HP +10% accessory on raises the maximum; taking it off drops it and
/// the current HP with it.
#[test]
fn set_equip_slot_rebuilds_the_live_maxima_from_the_base_window() {
    use legaia_engine_battle::accessory_passives::AccessoryPassives;
    let mut world = field_world(1);
    world.set_accessory_passives(AccessoryPassives::from_entries([(0xD0, 0), (0xD1, 3)], []));
    let rec = &mut world.party.roster.members[0];
    let mut base = rec.record_stats();
    base.hp_max = 537;
    base.mp_max = 80;
    rec.set_record_stats(base);
    let mut hms = rec.hp_mp_sp();
    (hms.hp_max, hms.hp_cur, hms.mp_max, hms.mp_cur) = (537, 537, 80, 80);
    rec.set_hp_mp_sp(hms);

    // Slot 5 takes the Max HP +10% accessory, slot 6 the Max MP +25% one.
    world.load_field_script(vec![0x3B, 0x05, 0xD0, 0x3B, 0x06, 0xD1, 0x00]);
    let _ = world.tick();
    let hms = world.party.roster.members[0].hp_mp_sp();
    assert_eq!((hms.hp_max, hms.mp_max), (537 + 53, 80 + 20));
    assert_eq!(
        (hms.hp_cur, hms.mp_cur),
        (537, 80),
        "a raise is not a refill"
    );

    // Healed to the boosted maxima, then both accessories come off.
    let rec = &mut world.party.roster.members[0];
    let mut hms = rec.hp_mp_sp();
    (hms.hp_cur, hms.mp_cur) = (590, 100);
    rec.set_hp_mp_sp(hms);
    world.load_field_script(vec![0x3B, 0x05, 0x00, 0x3B, 0x06, 0x00, 0x00]);
    let _ = world.tick();
    let hms = world.party.roster.members[0].hp_mp_sp();
    assert_eq!((hms.hp_max, hms.mp_max), (537, 80));
    assert_eq!(
        (hms.hp_cur, hms.mp_cur),
        (537, 80),
        "current follows the maximum down"
    );
}
