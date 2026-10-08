//! When the overworld sky band draws, and from which camera.
//!
//! The band itself - its projection, scroll, clipping and colour - is the
//! `FUN_801F73E4` port [`legaia_engine_vm::world_map_sky`]; the primitive
//! both hosts link is `legaia_engine_ui::screen_prim::sky_band_prims`. This
//! module is the one gate both hosts call between them, so neither decides
//! on its own which frames carry a sky.
//!
//! Retail runs the emitter as the first call of the overworld terrain sweep
//! `FUN_801F69D8`, so every kingdom-overworld frame draws it - the walk view
//! and a timeline shot over the overworld (the `map01` fly-in) alike. It
//! reads the live camera globals, which is the pose the port keeps in
//! [`Camera::globals`]. The top-view debug camera has no retail pose behind
//! it and draws no band.

use legaia_engine_vm::world_map_sky::{SKY_DIM_FLAG, SkySprite, sky_band_sprites};

use crate::camera::Camera;
use crate::camera_view::FieldCameraFrame;
use crate::world::{SceneMode, World};

/// This frame's sky sprites, or none outside the overworld walk / timeline
/// views.
pub fn sky_band(world: &World, cam: &Camera, frame: &FieldCameraFrame) -> Vec<SkySprite> {
    if world.mode != SceneMode::WorldMap {
        return Vec::new();
    }
    match frame {
        FieldCameraFrame::WorldMapWalk { .. } | FieldCameraFrame::Cutscene(_) => {}
        _ => return Vec::new(),
    }
    sky_band_sprites(&cam.globals.0, world.system_flag_test(SKY_DIM_FLAG))
}
