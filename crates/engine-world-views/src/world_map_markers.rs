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
    DISPLAY_H, DISPLAY_W, MarkerQuad, MarkerSegment, WorldMapEntityKind, WorldMapEntityMarker,
    WorldMapPlayerMarker, entity_segments, player_segments, segment_quad_depth,
};

/// Snapshot every placed overworld entity for the renderer: one
/// [`WorldMapEntityMarker`] per installed entity that carries a world
/// position, paired with its coarse [`WorldMapEntityKind`].
///
/// Returns an empty vector unless the disc-placement seeding
/// (`World::install_world_map_entities_at`) populated
/// [`legaia_engine_core::world::WorldMapState::entity_positions`] - the
/// config-only installers (which leave positions empty) produce no markers,
/// so a camera-only or synthetic world map degrades cleanly. The marker `y`
/// is the player actor's current plane (the placements are 2D), so markers
/// sit on the player's walking plane rather than at an arbitrary `y = 0`.
pub fn world_map_entity_markers(world: &World) -> Vec<WorldMapEntityMarker> {
    if world.world_map.entity_positions.is_empty() {
        return Vec::new();
    }
    let base_y = world
        .player_actor_slot
        .and_then(|s| world.actors.get(s as usize))
        .map(|a| a.move_state.world_y as f32)
        .unwrap_or(0.0);
    world
        .world_map
        .entity_positions
        .iter()
        .enumerate()
        .map(|(i, &(x, z))| {
            let kind = WorldMapEntityKind::of_config(world.world_map.entity_configs.get(i));
            WorldMapEntityMarker {
                world_pos: [x as f32, base_y, z as f32],
                kind,
            }
        })
        .collect()
}

/// The player's overworld position for the renderer, or `None` when there
/// is no active player actor. The world-map draw path renders the party
/// leader's field mesh at this position, oriented by
/// [`WorldMapPlayerMarker::facing`]; the marker doubles as the diagnostic
/// wireframe stand-in when the mesh upload failed.
pub fn world_map_player_marker(world: &World) -> Option<WorldMapPlayerMarker> {
    let slot = world.player_actor_slot? as usize;
    let a = world.actors.get(slot)?;
    if !a.active {
        return None;
    }
    Some(WorldMapPlayerMarker {
        world_pos: [
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        ],
        facing: a.move_state.render_26,
    })
}

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
        world_map_entity_markers(world)
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
    if draw_player && let Some(p) = world_map_player_marker(world) {
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
    use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;
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
        assert_eq!(world_map_entity_markers(&w).len(), 1);
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

    /// Placed overworld entities surface as render markers: one per installed
    /// position, paired with its kind, at the player's walking plane.
    #[test]
    fn world_map_entity_markers_pair_position_and_kind() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_field_player(0);
        // Put the player on a known plane so the marker `y` is deterministic.
        world.actors[0].move_state.world_y = -200;
        world.install_world_map_entities_at(vec![
            (
                WorldMapEntityConfig::MinigameDoor { sub_id: 3, slot: 0 },
                (448, 320),
            ),
            (
                WorldMapEntityConfig::Npc {
                    interact_id: 4,
                    text_id: None,
                    inline: Vec::new(),
                },
                (640, 128),
            ),
            (
                WorldMapEntityConfig::EncounterZone { formation_id: 2 },
                (-64, 512),
            ),
        ]);

        let markers = world_map_entity_markers(&world);
        assert_eq!(markers.len(), 3);
        // Position x/z come straight from the placement; y is the player plane.
        assert_eq!(markers[0].world_pos, [448.0, -200.0, 320.0]);
        assert_eq!(markers[0].kind, WorldMapEntityKind::Portal);
        assert_eq!(markers[1].world_pos, [640.0, -200.0, 128.0]);
        assert_eq!(markers[1].kind, WorldMapEntityKind::Npc);
        assert_eq!(markers[2].world_pos, [-64.0, -200.0, 512.0]);
        assert_eq!(markers[2].kind, WorldMapEntityKind::EncounterZone);
    }

    /// The player surfaces as an overworld marker at its actor position; with no
    /// player actor installed there is no marker.
    #[test]
    fn world_map_player_marker_tracks_player_actor() {
        let mut world = World::default();
        world.enter_world_map();
        assert!(
            world_map_player_marker(&world).is_none(),
            "no player actor -> no marker"
        );
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 320;
        world.actors[0].move_state.world_y = -64;
        world.actors[0].move_state.world_z = 256;
        let m = world_map_player_marker(&world).expect("player marker present");
        assert_eq!(m.world_pos, [320.0, -64.0, 256.0]);
    }

    /// Walking on the overworld records a heading the player marker exposes (the
    /// world-map walk sets `render_26` itself, since it uses the camera-relative
    /// bits rather than the field heading decoder).
    #[test]
    fn world_map_walking_sets_player_marker_facing() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_field_player(0);
        world.set_world_map_encounter(false, 50, 0, 64);
        world.reset_field_collision_grid(); // all-walkable so the step commits
        world.actors[0].move_state.world_z = 200; // away from the -Z boundary
        let start_z = world.actors[0].move_state.world_z;
        // At the default azimuth the walk frame is retail's identity, so "screen
        // up" walks +Z (dx=0, dz=1) -> atan2(0, 1) = 0 -> heading 0.
        world.set_pad(legaia_engine_system::input::PadButton::Up.mask());
        let _ = world.tick();
        let m = world_map_player_marker(&world).expect("player marker present");
        assert_eq!(m.facing, 0, "walking +Z faces heading 0");
        assert!(
            world.actors[0].move_state.world_z > start_z,
            "the player advanced +Z (start {start_z} -> {})",
            world.actors[0].move_state.world_z
        );
    }

    /// Config-only installs (no disc placements) produce no markers, so a
    /// camera-only or synthetic world map draws nothing.
    #[test]
    fn world_map_entity_markers_empty_without_positions() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
            interact_id: 1,
            text_id: None,
            inline: Vec::new(),
        }]);
        assert!(world_map_entity_markers(&world).is_empty());
    }
}
