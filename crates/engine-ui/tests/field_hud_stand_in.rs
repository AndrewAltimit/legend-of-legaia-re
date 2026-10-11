//! The field party HUD's no-projection stand-in against the kernel that
//! reads it.
//!
//! `engine-ui` restates the dodge threshold as a literal because the kernel
//! (`legaia_engine_field::world_map_panel_actors`) is not one of its
//! dependencies; this is the pin that keeps the two equal.

use legaia_engine_field::world_map_panel_actors::HUD_DODGE_THRESHOLD;
use legaia_engine_ui::field_party_hud::NO_PROJECTION_STAND_IN;

#[test]
fn the_stand_in_is_the_dodge_threshold() {
    assert_eq!(NO_PROJECTION_STAND_IN, HUD_DODGE_THRESHOLD);
}
