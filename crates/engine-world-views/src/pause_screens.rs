//! The pause screens' `World` seam.
//!
//! The pause-menu sessions, window models and route tables live in
//! [`legaia_engine_menus::pause_screens`]. This module holds the one entry
//! point that reads the world: the
//! Items screen's window-14 target panel, which takes the disc item-effect
//! table and the roster off a [`World`].

use legaia_engine_menus::pause_screens::*;

use legaia_engine_core::world::World;

/// Host entry point for the window-14 target panel off the live world: the
/// disc item-effect table and the roster are the only fields it reads. See
/// [`target_panel_view_model_of`].
pub fn target_panel_view_model(s: &PauseItemsSession, world: &World) -> Option<TargetPanelModel> {
    target_panel_view_model_of(
        s,
        world.tables.item_effects.as_ref(),
        &world.party.roster.members,
    )
}
