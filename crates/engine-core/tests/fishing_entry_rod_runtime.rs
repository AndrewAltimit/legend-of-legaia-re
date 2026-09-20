//! Disc-free runtime oracle for the fishing bring-up's rod scan: the live bag
//! decides which rod a session runs on, and the persistent cell follows.
//!
//! The kernel is [`legaia_engine_core::fishing::entry_rod_index`] (retail
//! `FUN_801CF070`, `0x801cf35c..0x801cf39c`); the host is
//! [`World::resolve_fishing_entry_rod`], which the mode-24 door warp calls on
//! the way into a venue. What the test pins is the part that bites: a stale
//! index must not survive into the tension gauge's divisor.

use legaia_engine_core::fishing::{ROD_KINDS, rod_item_id};
use legaia_engine_core::world::World;

/// Rods are items `0xA0..=0xA2`; give the party exactly one of them.
fn world_holding(rod: u32) -> World {
    let mut world = World::new();
    world.party.inventory.add(rod_item_id(rod) as u8, 1);
    world
}

#[test]
fn a_held_rod_is_kept() {
    for rod in 0..ROD_KINDS {
        let mut world = world_holding(rod);
        world.minigames.fishing_rod = rod;
        assert_eq!(world.resolve_fishing_entry_rod(), rod as i32);
        assert_eq!(world.minigames.fishing_rod, rod);
    }
}

#[test]
fn a_stale_index_steps_to_the_rod_the_party_has() {
    // Saved rod 1, but only rod 2 is in the bag: the scan wraps forward onto
    // it and writes the correction back, so the HUD row and the gauge divisor
    // read the same rod.
    let mut world = world_holding(2);
    world.minigames.fishing_rod = 1;
    assert_eq!(world.resolve_fishing_entry_rod(), 2);
    assert_eq!(world.minigames.fishing_rod, 2);

    // Saved rod 2, only rod 0 held: the wrap at ROD_KINDS is what finds it.
    let mut world = world_holding(0);
    world.minigames.fishing_rod = 2;
    assert_eq!(world.resolve_fishing_entry_rod(), 0);
}

#[test]
fn a_rodless_party_lands_on_rod_zero() {
    // Retail's give-up write: six probes, none owned, index 0. The point is
    // that the result stays inside `0..ROD_KINDS`, which the placeholder stat
    // the debug launchers pass does not.
    let mut world = World::new();
    world.minigames.fishing_rod = 2;
    assert_eq!(world.resolve_fishing_entry_rod(), 0);
    assert!((world.minigames.fishing_rod) < ROD_KINDS);
}
