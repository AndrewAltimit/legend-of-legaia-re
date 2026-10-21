//! The overworld markers' world half: which marker segments this frame
//! owes and the screen quads they become under the resolved frame.
//!
//! The marker itself - segments, colours, sizes, the projection onto the
//! 320x240 display and the occlusion depth - is
//! [`legaia_engine_field::world_map_markers`]. What reads the world stays
//! here: the debug-overlay gate
//! ([`legaia_engine_core::world::WorldToggles::overworld_marker_overlay`], off by default:
//! a retail frame at a town entrance shows the town's mesh and no post over
//! it), the floor height a marker stands on, and the mode / cutscene gates.
//! The player's marker is a stand-in for a missing leader mesh and stays on.

use legaia_engine_core::world::World;
use legaia_engine_field::camera_frame;
use legaia_engine_field::camera_frame::FieldCameraFrame;
use legaia_engine_field::world_map_markers::{
    DISPLAY_H, DISPLAY_W, MarkerQuad, MarkerSegment, entity_segments, player_segments,
    segment_quad_depth,
};

/// Every marker segment this frame: one set per placed entity while the
/// debug overlay is up ([`legaia_engine_core::world::WorldToggles::overworld_marker_overlay`]),
/// plus the player's when `draw_player` (a host passes `false` while it draws
/// the party leader's real mesh, which the marker would otherwise stab
/// through).
///
/// Each entity marker stands on the ground under it: with the scene's
/// collision grid loaded its `y` is the floor height sampled at the
/// placement (`World::sample_field_floor_height`, the `FUN_80019278` port),
/// the height retail's placement spawn adds from the same floor LUT. The
/// seam's own `y` is the player's plane, which buries a marker whose ground
/// sits higher than the player's under the terrain once the quads are
/// depth-tested. Without a grid the seam's plane stands.
pub fn marker_segments(world: &World, draw_player: bool) -> Vec<MarkerSegment> {
    let mut out = Vec::new();
    let grounded = !world.terrain.collision_grid.is_empty();
    let entities = if world.toggles.overworld_marker_overlay {
        world.world_map_entity_markers()
    } else {
        Vec::new()
    };
    for mut m in entities {
        if grounded {
            m.world_pos[1] = world
                .sample_field_floor_height(m.world_pos[0] as i32, m.world_pos[2] as i32)
                as f32;
        }
        out.extend(entity_segments(&m));
    }
    if draw_player && let Some(p) = world.world_map_player_marker() {
        out.extend(player_segments(&p));
    }
    out
}

/// This frame's marker quads, or nothing where no marker is owed: outside
/// the world map, under a scripted shot (retail's aerial fly-in shows the
/// bare continent), or when the frame has no engine camera to project with.
///
/// `frame` is the resolved world-map frame (`resolve_field_camera`), `aabb`
/// the scene box the top-view camera frames.
pub fn marker_quads(
    world: &World,
    frame: &FieldCameraFrame,
    aabb: ([f32; 3], [f32; 3]),
    draw_player: bool,
) -> Vec<MarkerQuad> {
    if world.mode != legaia_engine_mode::mode::SceneMode::WorldMap
        || world.cutscene_timeline_active()
    {
        return Vec::new();
    }
    if matches!(frame, FieldCameraFrame::Cutscene(_)) {
        return Vec::new();
    }
    let Some(vp) = camera_frame::frame_vp(frame, aabb, DISPLAY_W / DISPLAY_H) else {
        return Vec::new();
    };
    let eye = camera_frame::frame_eye(frame);
    let curve = (
        legaia_engine_field::camera_frame::frame_curve_scale(world.overworld_bit(), frame),
        world.overworld_curve_bias().unwrap_or(0),
    );
    marker_segments(world, draw_player)
        .iter()
        .filter_map(|s| segment_quad_depth(&vp, s, eye, curve))
        .collect()
}

/// [`marker_quads`] wrapped by `screen_prim::world_map_marker_prim` - the
/// one-pixel quads both hosts' screen-prim passes draw (the page has no line
/// primitive, which is why the kernel emits quads rather than lines).
pub fn marker_prims(
    world: &World,
    frame: &FieldCameraFrame,
    aabb: ([f32; 3], [f32; 3]),
    draw_player: bool,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    marker_quads(world, frame, aabb, draw_player)
        .iter()
        .map(|q| legaia_render_kernels::screen_prim::world_map_marker_prim(q.xy, q.rgba, q.depth))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::world_map_markers::{PLAYER_COLOR, depth_toward};
    use legaia_engine_mode::mode::SceneMode;

    fn world_map_world() -> World {
        let mut w = World {
            mode: SceneMode::WorldMap,
            ..World::default()
        };
        let a = w.spawn_actor(0);
        a.move_state.world_x = 1000;
        a.move_state.world_z = 2000;
        a.active = true;
        w.player_actor_slot = Some(0);
        w
    }

    const AABB: ([f32; 3], [f32; 3]) = ([0.0, -500.0, 0.0], [4000.0, 0.0, 4000.0]);

    #[test]
    fn player_marker_projects_on_screen_under_the_walk_camera() {
        let w = world_map_world();
        let frame = crate::camera_view::resolve_field_camera(
            &w,
            &legaia_engine_field::camera::Camera::default(),
            None,
            [0.0, 0.0],
        );
        assert!(matches!(frame, FieldCameraFrame::WorldMapWalk { .. }));
        let quads = marker_quads(&w, &frame, AABB, true);
        assert_eq!(quads.len(), 4, "post + cross + facing tick");
        // The walk camera follows the player: the base lands on the display's
        // centre column, inside the frame (the pinned eye trio puts it a
        // little below the middle row).
        let (x, y) = quads[0].xy[0];
        assert!(
            (x - 160).abs() < 8 && (60..180).contains(&y),
            "base at {x},{y}"
        );
        assert!(quads.iter().all(|q| q.rgba == PLAYER_COLOR));
    }

    #[test]
    fn no_player_marker_when_the_host_draws_the_mesh() {
        let w = world_map_world();
        let frame = crate::camera_view::resolve_field_camera(
            &w,
            &legaia_engine_field::camera::Camera::default(),
            None,
            [0.0, 0.0],
        );
        assert!(marker_quads(&w, &frame, AABB, false).is_empty());
    }

    /// The per-placement entity markers are a debug overlay: a placed portal
    /// draws nothing until the toggle raises it, and three segments after.
    #[test]
    fn entity_markers_draw_only_under_the_debug_toggle() {
        let mut w = world_map_world();
        w.world_map.entity_positions = vec![(1200, 2100)];
        assert_eq!(w.world_map_entity_markers().len(), 1);
        assert!(
            marker_segments(&w, false).is_empty(),
            "no entity marker by default - retail draws none"
        );
        w.toggles.overworld_marker_overlay = true;
        assert_eq!(marker_segments(&w, false).len(), 3);
    }

    #[test]
    fn nothing_outside_the_world_map() {
        let mut w = world_map_world();
        w.mode = SceneMode::Field;
        let frame = FieldCameraFrame::HostDebugOrbit;
        assert!(marker_quads(&w, &frame, AABB, true).is_empty());
    }

    /// Under the walk camera every quad carries its corners' depth, pulled a
    /// little toward the eye; a farther marker reads deeper.
    #[test]
    fn walk_camera_quads_carry_scene_depth() {
        let w = world_map_world();
        let frame = crate::camera_view::resolve_field_camera(
            &w,
            &legaia_engine_field::camera::Camera::default(),
            None,
            [0.0, 0.0],
        );
        let quads = marker_quads(&w, &frame, AABB, true);
        assert!(!quads.is_empty());
        let vp = camera_frame::frame_vp(&frame, AABB, DISPLAY_W / DISPLAY_H).unwrap();
        let eye = camera_frame::frame_eye(&frame).unwrap();
        for q in &quads {
            let d = q.depth.expect("walk frame quads are depth-tested");
            assert!(d.iter().all(|z| (0.0..=1.0).contains(z)), "{d:?}");
        }
        // The pull puts the sampled depth just in front of the true point.
        let p = [1000.0, 0.0, 2000.0];
        let exact = {
            let v = [p[0], -p[1], p[2], 1.0];
            let row = |r: usize| (0..4).map(|c| vp[c * 4 + r] * v[c]).sum::<f32>();
            row(2) / row(3)
        };
        let pulled = depth_toward(&vp, p, eye).unwrap();
        assert!(pulled < exact, "pulled {pulled} vs exact {exact}");
        // And a point farther along the eye-to-player ray is deeper.
        let dir = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
        let far_p = [p[0] + dir[0], p[1] + dir[1], p[2] + dir[2]];
        let far = depth_toward(&vp, far_p, eye).unwrap();
        assert!(far > pulled, "far {far} vs {pulled}");
    }
}
