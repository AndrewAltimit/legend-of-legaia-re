//! The dance venue's **globals** over a `World`: the save / restore pair the
//! dance overlay's init `FUN_801CEF54` and teardown `FUN_801D414C` run around
//! the beat clock. The venue itself (its scene, resources, draws, camera pose
//! and face stamps) is `legaia_engine_minigame_scenes::dance_venue`; this
//! file keeps the one step that reads and writes the world.
//!
//! See `docs/subsystems/minigame-dance.md` § Entering and leaving the hall.

use crate::world::World;
use legaia_engine_field::camera::Camera;
use legaia_engine_minigame_scenes::dance_venue::{
    DanceVenueEdge, DanceVenueStage, venue_camera, venue_camera_at,
};
use legaia_engine_minigames::dance::dance_scene_entry;
use legaia_engine_mode::mode::SceneMode;

/// Stage or tear down the venue globals to match the world's mode. Call once
/// per host frame after the world tick, with the camera the host frames the
/// field with.
///
/// The staging edge is the entry `FUN_801CEF54`'s globals, straight off
/// [`dance_scene_entry`]: `_DAT_80084540` = [`DanceSceneEntry::scene_block_base`](legaia_engine_minigames::dance::DanceSceneEntry::scene_block_base)
/// (the port's mirror is `World::battle.map_id`, the value the scene loader
/// writes on every field entry), the scratchpad window `0x1F8003E8..EB` =
/// [`DanceSceneEntry::view_window`](legaia_engine_minigames::dance::DanceSceneEntry::view_window), and the camera pose [`venue_camera`]
/// derives. The restore edge is the half of the teardown `FUN_801D414C` the
/// port holds state for: the block base and the window go back to what the
/// walked-in scene had. The scene-name restore needs no write here - the
/// walked-in scene is never unloaded.
///
/// Every staged frame then advances the run's camera keyframe track
/// ([`DanceCameraTrack::tick`](legaia_engine_minigames::dance::DanceCameraTrack::tick), via [`legaia_engine_minigames::dance::DanceGame::advance_camera`])
/// and re-frames the staged camera on the pose it wrote
/// ([`venue_camera_at`]). A run with no track, or the how-to demo, keeps the
/// entry's pose.
///
// REF: FUN_801CEF54, FUN_801D414C (the record is
// `legaia_engine_minigames::dance::dance_scene_entry`, whose `PORT:` tag this call makes live)
pub fn sync_dance_venue(world: &mut World, camera: &mut Camera) -> Option<DanceVenueEdge> {
    let edge = stage_or_restore(world, camera);
    // The dance tick's camera keyframe track runs every staged frame - the
    // staging frame included, which is the tick's first after the entry and
    // lands on pose 0 exactly.
    if world.mode == SceneMode::Dance
        && let (Some(stage), Some(game)) = (
            world.minigames.dance_venue.as_mut(),
            world.minigames.dance.as_mut(),
        )
        && let Some(pose) = game.advance_camera(1)
    {
        stage.camera = venue_camera_at(&dance_scene_entry(), pose);
    }
    edge
}

fn stage_or_restore(world: &mut World, camera: &mut Camera) -> Option<DanceVenueEdge> {
    let in_dance = world.mode == SceneMode::Dance;
    match (in_dance, world.minigames.dance_venue.is_some()) {
        (true, false) => {
            let e = dance_scene_entry();
            let generation = world.minigames.dance_venue_generation.wrapping_add(1);
            world.minigames.dance_venue_generation = generation;
            world.minigames.dance_venue = Some(DanceVenueStage {
                camera: venue_camera(&e),
                saved_block_base: world.battle.map_id,
                saved_view_window: camera.zone.view_window,
                generation,
            });
            world.battle.map_id = u32::from(e.scene_block_base);
            let (x0, z0, x1, z1) = e.view_window;
            camera.zone.view_window = [x0, z0, x1, z1];
            Some(DanceVenueEdge::Staged)
        }
        (false, true) => {
            let stage = world.minigames.dance_venue.take()?;
            let teardown = legaia_engine_minigames::dance::dance_scene_stage();
            if teardown.restores_scene_block_base {
                world.battle.map_id = stage.saved_block_base;
            }
            camera.zone.view_window = stage.saved_view_window;
            Some(DanceVenueEdge::Restored)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_and_leaving_the_dance_stages_and_restores_the_globals() {
        let mut world = World::new();
        let mut camera = Camera::new();
        world.mode = SceneMode::Field;
        world.battle.map_id = 0x2A;
        let field_window = camera.zone.view_window;
        assert_eq!(sync_dance_venue(&mut world, &mut camera), None);

        world.mode = SceneMode::Dance;
        assert_eq!(
            sync_dance_venue(&mut world, &mut camera),
            Some(DanceVenueEdge::Staged)
        );
        let e = dance_scene_entry();
        assert_eq!(world.battle.map_id, u32::from(e.scene_block_base));
        assert_eq!(camera.zone.view_window, [-8, -10, 8, 10]);
        let stage = world.minigames.dance_venue.expect("staged");
        assert_eq!(stage.camera, venue_camera(&e));
        assert_eq!(stage.generation, 1);
        // Idempotent while the dance runs.
        assert_eq!(sync_dance_venue(&mut world, &mut camera), None);

        world.mode = SceneMode::Field;
        assert_eq!(
            sync_dance_venue(&mut world, &mut camera),
            Some(DanceVenueEdge::Restored)
        );
        assert_eq!(world.battle.map_id, 0x2A);
        assert_eq!(camera.zone.view_window, field_window);
        assert!(world.minigames.dance_venue.is_none());

        // A second run is a new generation.
        world.mode = SceneMode::Dance;
        sync_dance_venue(&mut world, &mut camera);
        assert_eq!(world.minigames.dance_venue.map(|s| s.generation), Some(2));
    }
}
