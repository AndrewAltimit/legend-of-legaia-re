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
///
/// "Cutscene framing" is two terms: the host's cutscene camera arm
/// (`cutscene_camera_owns_frame`, an op-`0x45` shot staged under the modal
/// timeline) and [`script_holds_frame`], every span a script context holds
/// the player - the gaps between a record's beats and an APPLY's ease back
/// included, where the staged set is empty and the cutscene arm has let go
/// while the framing is still the script's.
pub fn fade_armed(world: &World, cutscene_camera_owns_frame: bool) -> bool {
    world.mode == legaia_engine_mode::mode::SceneMode::Field
        && !cutscene_camera_owns_frame
        && !script_holds_frame(world)
}

/// Whether a script owns this frame's camera, so the fade stands down: the
/// retail engaged bit as the field tick reads it
/// ([`World::script_holds_camera`]).
///
/// Retail's tick skips the pad controller and the free-roam follow tail
/// while the bit is up (`FUN_801D1344`, `0x801D1694` / `0x801D17DC`), so the
/// shot is the record's own: `town01` `P2[5]` walks Vahn in through his
/// house's entrance hallway under a staged shot (`45 07 ..` at `+0x471`,
/// then the `C7 F8 ..` legs) and the wall between him and the lens is meant
/// to stay. The front door's record (`P0[29]`) spawns it as a concurrent
/// record, so the host's cutscene arm (the modal timeline's) never takes
/// that frame: the shot reaches the screen through the follow arm, which
/// reads the staged globals, and only this term sees it. A plain talk is not a term (it holds the pad through the
/// dialogue channel and keeps the follow framing) unless it stages a shot.
pub fn script_holds_frame(world: &World) -> bool {
    world.script_holds_camera()
}

/// The fade's arming verdict for one frame ([`host_fade_arming`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeArming {
    /// The fade arms: run the visibility gate and step the ramp on it.
    Armed,
    /// A script owns the frame ([`script_holds_frame`] or the host's
    /// cutscene arm) while every other term would arm. The gate does not
    /// run; the ramp eases out from wherever it stood
    /// ([`legaia_engine_scene::field_occlusion::FadeRamp::step`] on `false`),
    /// so a shot that opens with the player behind a wall dissolves the
    /// screen-door back over the fade's own release ease rather than popping,
    /// and a shot that opens with the fade at rest never touches the scene.
    /// When the script lets go the ramp starts from where it stands and eases
    /// in on the gate's verdict as it does after any cover edge.
    ScriptHeld,
    /// Not armed for a reason that is not a shot (toggle off, a screen or a
    /// debug vantage owning the frame, out of the field): the ramp resets.
    Off,
}

/// The host's own arming terms of the fade, beside [`fade_armed`]'s world
/// half. Each host fills the three it can see; [`host_fade_arming`] is the
/// one place they combine, so the two play hosts cannot again disagree on
/// which screens dissolve the walls behind them.
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

/// The fade's verdict this frame: the host's terms, a live name-entry prompt
/// (engine state, so not a host term), field mode, then the script terms.
///
/// The native window used to exclude only its boot UI and debug vantage, so
/// a pause menu or a name-entry prompt kept dissolving the walls behind it;
/// the browser page had added those screens itself. Both now ask this.
pub fn host_fade_arming(
    world: &World,
    cutscene_camera_owns_frame: bool,
    h: FadeHostTerms,
) -> FadeArming {
    if !h.enabled
        || h.debug_camera
        || h.screen_owned
        || world.name_entry_active()
        || world.mode != legaia_engine_mode::mode::SceneMode::Field
    {
        return FadeArming::Off;
    }
    if cutscene_camera_owns_frame || script_holds_frame(world) {
        return FadeArming::ScriptHeld;
    }
    FadeArming::Armed
}

/// Whether the fade arms this frame ([`host_fade_arming`] is
/// [`FadeArming::Armed`]).
pub fn host_fade_armed(world: &World, cutscene_camera_owns_frame: bool, h: FadeHostTerms) -> bool {
    host_fade_arming(world, cutscene_camera_owns_frame, h) == FadeArming::Armed
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
