//! The Muscle Dome's `World` seam.
//!
//! The dome session, its command menu, ring and loadout kernels live in
//! [`legaia_engine_menus::muscle_dome`] (which itself re-exports the course
//! ladder and score tables of `legaia_engine_minigames::muscle_dome`) and are
//! re-exported here at their old paths. What stays is the one entry that
//! reads the world: a fighter's magic loadout off the live roster.

pub use legaia_engine_menus::muscle_dome::*;

use crate::world::World;

/// Build a dome fighter's [`DomeMagic`] out of a live world's roster - the
/// one door both native dome entry paths (the arena-door warp and the
/// window's own dome entry) install through. The roster record, the Seru
/// log's captured spells, the spell catalog and the slot's ability word are
/// the whole slice it reads; see [`magic_loadout_of`].
///
/// Returns `None` when the roster has no such member.
pub fn magic_loadout_for(world: &World, roster_slot: usize, special: u32) -> Option<DomeMagic> {
    let member = world.party.roster.members.get(roster_slot)?;
    Some(magic_loadout_of(
        member,
        world.seru.log.learned_spells(roster_slot as u8),
        &world.tables.spell_catalog,
        world
            .party
            .character_ability_bits
            .get(roster_slot)
            .copied()
            .unwrap_or(0),
        roster_slot,
        special,
    ))
}
