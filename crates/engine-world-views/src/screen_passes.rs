//! The field and overworld screen-space passes both play hosts draw, each
//! one call from a live `World` to the finished [`ScreenPrim`]s.
//!
//! Every pass here is the same three steps: a mode gate, the resolver's
//! non-scripted frame (`resolve_field_camera(.., None, centre)`, with the
//! scene box's X / Z centre as the fallback focus) and a shared kernel
//! wrapped by its shared prim builder. Each host used to spell all three
//! itself - the native window in `redraw_passes.rs`, the browser play page
//! in `play_field_fx.rs` and `play_world_map_markers.rs` - which is two
//! copies of one decision per pass. What a host still decides alone is only
//! what it holds alone (the native window's boot-title gate, the page's
//! "is the leader's mesh resident" answer).
//!
//! The fog sheets are not here: their render step is simulation (it ages
//! the pool the next tick's spawner reads), so it runs per tick through
//! `legaia_engine_session::BootSession::step_field_fog`, not per draw.

use legaia_engine_core::camera_view::resolve_field_camera;
use legaia_engine_core::world::World;
use legaia_engine_field::camera::Camera;
use legaia_engine_field::camera_frame::FieldCameraFrame;
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::screen_prim::{
    ScreenPrim, drop_shadow_prim, light_pool_prims, sky_band_prims,
};

/// A scene box, `(min, max)` in retail world units.
pub type SceneBox = ([f32; 3], [f32; 3]);

/// The follow camera's fallback focus: the scene box's X / Z centre, which
/// every field screen pass hands `resolve_field_camera`. A pass that pinned
/// `[0, 0]` projected through a camera aimed at the world origin on any
/// frame whose camera fell back to the host centre.
pub fn scene_centre(aabb: SceneBox) -> [f32; 2] {
    [(aabb.0[0] + aabb.1[0]) * 0.5, (aabb.0[2] + aabb.1[2]) * 0.5]
}

/// The resolver's non-scripted frame - the call never advances the cutscene
/// glide, so a draw pass may make it as often as it likes.
fn frame(world: &World, camera: &Camera, aabb: SceneBox) -> FieldCameraFrame {
    resolve_field_camera(world, camera, None, scene_centre(aabb))
}

/// This frame's actor drop shadows (`legaia_render_kernels::drop_shadow`,
/// retail's `FUN_8001C394` blob): `World::field_drop_shadows` through the
/// field follow frame, wrapped by `drop_shadow_prim`. Each cell carries its
/// corners' scene depth, so a host depth-tests it against the scene already
/// drawn: under the actor standing on it, over the ground. Empty outside
/// game mode 3.
pub fn drop_shadow_screen_prims(world: &World, camera: &Camera, aabb: SceneBox) -> Vec<ScreenPrim> {
    if !World::fog_mode(world.mode) {
        return Vec::new();
    }
    let Some(view) = frame(world, camera, aabb).field_view() else {
        return Vec::new();
    };
    world
        .field_drop_shadows(&view)
        .iter()
        .map(|q| drop_shadow_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index, q.depth))
        .collect()
}

/// This frame's move-VM strip spans: every extension sub-op `0x2C`
/// execution of the latest tick (the scanline strip emitter `FUN_801D31B0`,
/// `MoveVmGlobals::strip_frame`), projected through the follow frame by
/// `move_strip_prims`. The set is the latest tick's and is not drained, so
/// a redraw with no tick draws it again. Field scenes only - the extension
/// dispatcher exists only in the field overlay.
pub fn move_strip_screen_prims(world: &World, camera: &Camera, aabb: SceneBox) -> Vec<ScreenPrim> {
    let requests = world.move_vm.strip_frame();
    if requests.is_empty() || world.mode != SceneMode::Field {
        return Vec::new();
    }
    let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) =
        frame(world, camera, aabb)
    else {
        return Vec::new();
    };
    legaia_render_kernels::move_strip::move_strip_prims(requests, &view)
}

/// This frame's attached lights - the field VM's op `0x34` sub-1 light pools
/// (`World::field_light_draws`, the `FUN_801E4470` draw feeding
/// `FUN_801E3984`) - wrapped by `light_pool_prims`, which owns the blend
/// class, vertex order and triangle split. Empty outside a field scene or
/// with no light live.
pub fn light_screen_prims(world: &World, camera: &Camera, aabb: SceneBox) -> Vec<ScreenPrim> {
    if world.mode != SceneMode::Field || world.script_actors.lights.is_empty() {
        return Vec::new();
    }
    let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) =
        frame(world, camera, aabb)
    else {
        return Vec::new();
    };
    world
        .field_light_draws(&view)
        .iter()
        .flat_map(|d| light_pool_prims(d.abr, &d.polys))
        .collect()
}

/// This frame's overworld markers as screen primitives
/// ([`crate::world_map_markers::marker_prims`] through the resolver's
/// frame). `draw_player` is the host's answer to "did the leader's real mesh
/// fail to resolve" - the player marker is its stand-in.
pub fn world_map_marker_screen_prims(
    world: &World,
    camera: &Camera,
    aabb: SceneBox,
    draw_player: bool,
) -> Vec<ScreenPrim> {
    if world.mode != SceneMode::WorldMap {
        return Vec::new();
    }
    crate::world_map_markers::marker_prims(world, &frame(world, camera, aabb), aabb, draw_player)
}

/// This frame's overworld sky band (`FUN_801F73E4`) through the gate
/// [`crate::world_map_sky::sky_band`] and `sky_band_prims`. Each quad is
/// depth-tested at the far plane, so it fills only what the terrain left
/// clear - retail's farthest-bucket draw order.
pub fn world_map_sky_screen_prims(
    world: &World,
    camera: &Camera,
    aabb: SceneBox,
) -> Vec<ScreenPrim> {
    if world.mode != SceneMode::WorldMap {
        return Vec::new();
    }
    sky_band_prims(&crate::world_map_sky::sky_band(
        world,
        camera,
        &frame(world, camera, aabb),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AABB: SceneBox = ([0.0, -500.0, 0.0], [4000.0, 0.0, 2000.0]);

    #[test]
    fn the_fallback_focus_is_the_box_centre() {
        assert_eq!(scene_centre(AABB), [2000.0, 1000.0]);
    }

    #[test]
    fn every_pass_is_empty_off_its_mode() {
        let world = World::default();
        let cam = Camera::default();
        assert!(drop_shadow_screen_prims(&world, &cam, AABB).is_empty());
        assert!(move_strip_screen_prims(&world, &cam, AABB).is_empty());
        assert!(light_screen_prims(&world, &cam, AABB).is_empty());
        assert!(world_map_marker_screen_prims(&world, &cam, AABB, true).is_empty());
        assert!(world_map_sky_screen_prims(&world, &cam, AABB).is_empty());
    }
}
