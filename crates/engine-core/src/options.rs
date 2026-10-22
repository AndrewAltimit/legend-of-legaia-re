//! The options' push onto a [`World`].
//!
//! The options state, its persistence, the options screen and the key-rebind
//! flow are [`legaia_engine_menus::options`], which names no `World`. This
//! module holds the one function that does.

use crate::world::World;
use legaia_engine_menus::options::OptionsState;
use legaia_engine_system::option_values::FieldMoveOpt;

/// Push the options that are **simulation** knobs onto the world: the
/// precise-movement decode, the Field Move default (the run button that
/// inverts it is latched by `World::set_pad`), the photosensitivity guard
/// over the ambient palette cyclers, and the battle Select Attack word
/// (`0x800846C4`). Host policy over world state that a scene change or a
/// New Game can reseed, so both hosts call this one push - the native
/// window every tick, the browser page whenever the options change or a
/// scene is entered. Audio and camera knobs stay with the host that owns
/// the device.
pub fn apply_to_world(opts: &OptionsState, world: &mut World) {
    world.locomotion.precise_movement = opts.precise_movement;
    world.locomotion.run_default = opts.field_move == FieldMoveOpt::Run;
    world.toggles.reduce_flashing = opts.reduce_flashing;
    world.toggles.select_attack = opts.battle_select_attack;
    world.toggles.battle_command = opts.battle_command;
    world.toggles.battle_camera = opts.battle_camera;
    world.toggles.field_hp_display = opts.field_hp_display;
    world.terrain.static_window.retail_windowing = opts.retail_static_window;
    world.toggles.view_window_crop = opts.retail_view_window;
    world.toggles.volumetric_fog = opts.volumetric_fog;
    world.toggles.retail_options = opts.retail_options();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's sub-area static-object windowing is the play hosts' default:
    /// the props its region box leaves out are other rooms' scenery (retona's
    /// cloud bowl covers the cave from outside its region), not an
    /// enhancement worth drawing.
    #[test]
    fn retail_static_window_defaults_on_and_reaches_the_world() {
        let opts = OptionsState::default();
        assert!(opts.retail_static_window);
        let mut w = World::new();
        assert!(!w.terrain.static_window.retail_windowing);
        apply_to_world(&opts, &mut w);
        assert!(w.terrain.static_window.retail_windowing);
    }
}
