//! The camera-occlusion fade's **arming terms and anchors over a [`World`]**.
//!
//! The visibility gate itself - the CPU ray-cast of eye-to-player segments
//! against the assembled field scene - and the fade's strength ramp are
//! [`legaia_engine_scene::field_occlusion`], which names no `World`. This
//! module holds what reads one: whether the fade arms this frame, and the
//! two points on the player the gate and the shader both anchor to.

use legaia_engine_core::world::World;
use legaia_engine_scene::field_occlusion::HALF_CHAR_HEIGHT;

/// The world-side arming terms of the camera-occlusion fade.
///
/// The fade is a field free-roam feature: the follow camera's eye is what
/// the visibility gate ray-casts from, so cutscene framing (authored),
/// battle and the world map (each framing their own subject) are out.
/// A host adds its own terms on top - its master toggle, a boot / pause UI
/// owning the screen, its debug vantage, a VR first-person eye - and the
/// native window and the browser play page had different sets of those:
/// the page excluded only battle and the minigames, so a pause menu or a
/// scripted shot kept dissolving the walls behind them.
pub fn fade_armed(world: &World, cutscene_camera_owns_frame: bool) -> bool {
    world.mode == legaia_engine_mode::mode::SceneMode::Field && !cutscene_camera_owns_frame
}

/// The host's own arming terms of the fade, beside [`fade_armed`]'s world
/// half. Each host fills the three it can see; [`host_fade_armed`] is the one
/// place they combine, so the two play hosts cannot again disagree on which
/// screens dissolve the walls behind them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FadeHostTerms {
    /// The host's master toggle (`--no-occlusion-fade` / `F4` natively, the
    /// page's "See-through walls" checkbox).
    pub enabled: bool,
    /// A debug vantage owns the frame (the native `F3` orbit, the page's
    /// debug camera, a VR first-person eye) - nothing sits between it and
    /// the player in the follow camera's sense.
    pub debug_camera: bool,
    /// A UI screen owns the frame: the boot UI, or any menu-runtime screen
    /// (the pause menu, a shop, the prize counter - `MenuRuntime::is_open`).
    pub screen_owned: bool,
}

/// Whether the fade arms this frame: the host's terms, a live name-entry
/// prompt (engine state, so not a host term) and [`fade_armed`].
///
/// The native window used to exclude only its boot UI and debug vantage, so
/// a pause menu or a name-entry prompt kept dissolving the walls behind it;
/// the browser page had added those screens itself. Both now ask this.
pub fn host_fade_armed(world: &World, cutscene_camera_owns_frame: bool, h: FadeHostTerms) -> bool {
    h.enabled
        && !h.debug_camera
        && !h.screen_owned
        && !world.name_entry_active()
        && fade_armed(world, cutscene_camera_owns_frame)
}

/// The player's body centre in **raw retail Y-down world coordinates** - the
/// one point the camera-occlusion fade is about.
///
/// The floor tier under the actor (the sampler the follow camera anchors to)
/// lifted half a character height; up is negative in this frame. `None` when
/// there is no live player actor, which is the same guard that stops the
/// fade arming at all.
///
/// A host that renders Y-up negates the Y on the way out to its draw frame;
/// the gate ([`FieldOccluders::fully_occluded`]) takes this frame directly.
pub fn player_body_centre(world: &World) -> Option<[f32; 3]> {
    let slot = world.player_actor_slot?;
    let a = world.actors.get(slot as usize)?;
    let (wx, wz) = (a.move_state.world_x, a.move_state.world_z);
    let floor_y = world.sample_field_floor_height(wx as i32, wz as i32);
    Some([wx as f32, floor_y as f32 - HALF_CHAR_HEIGHT, wz as f32])
}

/// The floor point under the player - their **feet** - in raw retail
/// Y-down world coordinates: [`player_body_centre`] lowered by
/// [`HALF_CHAR_HEIGHT`], i.e. the same floor-tier sample.
///
/// The fade's second anchor. A host stages it beside the body centre so the
/// renderer can draw the feet line: a fragment that projects at or below the
/// character's feet on screen cannot be hiding them, so the floor in front
/// of the character and the foot of the wall that hides them never dissolve
/// (`engine-render::occlusion_fade::lift_factor`).
pub fn player_feet(world: &World) -> Option<[f32; 3]> {
    player_body_centre(world).map(|c| [c[0], c[1] + HALF_CHAR_HEIGHT, c[2]])
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The host terms veto on their own: any one of them keeps the fade off.
    #[test]
    fn host_terms_veto() {
        let w = World::new();
        let on = FadeHostTerms {
            enabled: true,
            ..Default::default()
        };
        // An empty world is not in field free-roam with a player, so arming
        // follows `fade_armed`; the host terms can only remove it.
        let base = host_fade_armed(&w, false, on);
        assert_eq!(base, fade_armed(&w, false) && !w.name_entry_active());
        for t in [
            FadeHostTerms {
                enabled: false,
                ..on
            },
            FadeHostTerms {
                debug_camera: true,
                ..on
            },
            FadeHostTerms {
                screen_owned: true,
                ..on
            },
        ] {
            assert!(!host_fade_armed(&w, false, t));
        }
    }
}
