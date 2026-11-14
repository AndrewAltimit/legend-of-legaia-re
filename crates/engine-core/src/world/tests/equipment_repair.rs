//! The load-time repair of equipment a shop's equip-now row misfiled.

use super::*;
use legaia_game_tables::equip_stats::EquipSlot as Disc;

const WEAPON: u8 = 0x22;
const WEAPON_2: u8 = 0x23;
const ARMOUR: u8 = 0x44;
const ARMOUR_2: u8 = 0x45;
const HELMET: u8 = 0x35;
const BOOTS: u8 = 0x59;
const TERRA_STAGE: u8 = 0x0C;

fn category(id: u8) -> Option<Disc> {
    match id {
        WEAPON | WEAPON_2 => Some(Disc::Weapon),
        ARMOUR | ARMOUR_2 => Some(Disc::Body),
        HELMET => Some(Disc::Head),
        BOOTS => Some(Disc::Footwear),
        _ => None,
    }
}

fn world_wearing(ch: usize, slots: [u8; 8]) -> World {
    let mut w = World::default();
    w.load_party(legaia_save::Party::zeroed(3));
    w.party.roster.members[ch].set_equipment(legaia_save::EquipmentSlots { slots });
    w
}

fn worn(w: &World, ch: usize) -> [u8; 8] {
    w.party.roster.members[ch].equipment().slots
}

/// A record as retail's writers leave it - body, head, the weapon on the
/// character's byte, the Ra-Seru stage on the other, footwear - loads
/// untouched, for all three characters.
#[test]
fn a_well_formed_record_is_left_alone() {
    for (ch, slots) in [
        (0, [ARMOUR, HELMET, WEAPON, 0x03, BOOTS, 0x9A, 0, 0]),
        (1, [ARMOUR, HELMET, TERRA_STAGE, WEAPON, BOOTS, 0, 0, 0]),
        (2, [ARMOUR, HELMET, WEAPON, 0x15, BOOTS, 0, 0, 0]),
        (0, [0; 8]),
    ] {
        let mut w = world_wearing(ch, slots);
        assert_eq!(w.repair_misfiled_equipment_with(category), 0);
        assert_eq!(worn(&w, ch), slots);
        assert!(w.party.inventory.iter().all(|(_, c)| *c == 0));
    }
}

/// Vahn bought body armour with the equip-now row on a bad build: it sits
/// on his weapon byte and the weapon it displaced is in the bag. The armour
/// goes home when the body byte is empty, to the bag when it is not, and the
/// weapon byte is left empty either way.
#[test]
fn body_armour_on_the_weapon_byte_goes_home_or_to_the_bag() {
    let mut w = world_wearing(0, [0, HELMET, ARMOUR, 0x03, BOOTS, 0, 0, 0]);
    w.party.inventory.add(WEAPON, 1);
    assert_eq!(w.repair_misfiled_equipment_with(category), 1);
    assert_eq!(worn(&w, 0), [ARMOUR, HELMET, 0, 0x03, BOOTS, 0, 0, 0]);
    assert_eq!(w.party.inventory.get(&ARMOUR), None);
    assert_eq!(w.party.inventory.get(&WEAPON).copied(), Some(1));

    let mut w = world_wearing(0, [ARMOUR_2, HELMET, ARMOUR, 0x03, BOOTS, 0, 0, 0]);
    assert_eq!(w.repair_misfiled_equipment_with(category), 1);
    assert_eq!(worn(&w, 0), [ARMOUR_2, HELMET, 0, 0x03, BOOTS, 0, 0, 0]);
    assert_eq!(w.party.inventory.get(&ARMOUR).copied(), Some(1));
    // A second pass finds nothing.
    assert_eq!(w.repair_misfiled_equipment_with(category), 0);
}

/// A weapon bought the same way sits on the body byte beside the weapon
/// already held.
#[test]
fn a_weapon_on_the_body_byte_goes_home_or_to_the_bag() {
    let mut w = world_wearing(1, [WEAPON_2, 0, TERRA_STAGE, WEAPON, 0, 0, 0, 0]);
    assert_eq!(w.repair_misfiled_equipment_with(category), 1);
    assert_eq!(worn(&w, 1), [0, 0, TERRA_STAGE, WEAPON, 0, 0, 0, 0]);
    assert_eq!(w.party.inventory.get(&WEAPON_2).copied(), Some(1));

    let mut w = world_wearing(2, [WEAPON_2, 0, 0, 0x15, 0, 0, 0, 0]);
    assert_eq!(w.repair_misfiled_equipment_with(category), 1);
    assert_eq!(worn(&w, 2), [0, 0, WEAPON_2, 0x15, 0, 0, 0, 0]);
    assert_eq!(w.party.inventory.get(&WEAPON_2), None);
}

/// Noa's byte 2 is her Ra-Seru byte: the bad buy sent her Ra-Seru stage to
/// the bag. The armour leaves and the stage comes back.
#[test]
fn noas_ra_seru_stage_returns_from_the_bag() {
    let mut w = world_wearing(1, [0, 0, ARMOUR, WEAPON, 0, 0, 0, 0]);
    w.party.inventory.add(TERRA_STAGE, 1);
    assert_eq!(w.repair_misfiled_equipment_with(category), 1);
    assert_eq!(worn(&w, 1), [ARMOUR, 0, TERRA_STAGE, WEAPON, 0, 0, 0, 0]);
    assert_eq!(w.party.inventory.get(&TERRA_STAGE), None);
}

/// Both misfilings on one record swap back.
#[test]
fn armour_and_weapon_swapped_swap_back() {
    let mut w = world_wearing(0, [WEAPON, 0, ARMOUR, 0x03, 0, 0, 0, 0]);
    assert_eq!(w.repair_misfiled_equipment_with(category), 2);
    assert_eq!(worn(&w, 0), [ARMOUR, 0, WEAPON, 0x03, 0, 0, 0, 0]);
    assert!(w.party.inventory.iter().all(|(_, c)| *c == 0));
}

/// Without the disc table the repair has no categories and does nothing.
#[test]
fn no_table_no_repair() {
    let mut w = world_wearing(0, [WEAPON, 0, ARMOUR, 0, 0, 0, 0, 0]);
    assert_eq!(w.repair_misfiled_equipment(), 0);
    assert_eq!(worn(&w, 0), [WEAPON, 0, ARMOUR, 0, 0, 0, 0, 0]);
}
