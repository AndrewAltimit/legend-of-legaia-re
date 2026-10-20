//! The shop screens' `World` seam.
//!
//! The shop's sessions, layout kernels and window models live in
//! [`legaia_engine_menus::shop`]. This module holds the one entry point
//! that reads the world: the
//! window-41 party compare, which takes the roster and the equipment stat
//! table off a [`World`]. The page-length helper keyed on the menu state is
//! `legaia_engine_menus::shop_cues::shop_list_page_rows`.

use legaia_engine_menus::shop::*;

use legaia_engine_core::world::World;

/// Window 41's model for staged item `item_id` off the live world: the
/// roster and the equipment stat table are the only fields it reads. See
/// [`party_compare_members_of`].
///
/// REF: FUN_801D4C28
pub fn party_compare_members(
    world: &World,
    info: Option<&legaia_engine_menus::equipment::DiscEquipInfo>,
    item_id: u8,
) -> Vec<PartyCompareMember> {
    party_compare_members_of(
        &world.party.roster.members,
        &world.present_party_list(),
        &world.tables.equipment_table,
        info,
        item_id,
    )
}
