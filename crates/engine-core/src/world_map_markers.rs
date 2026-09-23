//! The overworld's entity and player **markers**, as screen-space quads.
//!
//! Not retail. Retail binds each world-map placement to its own actor model;
//! that per-entity mesh resolution is still open (see
//! `docs/subsystems/world-map.md`, "Rendering the placed entities"), so the
//! port draws a kind-coded upright marker at each placement instead - a post,
//! a base cross, and for the player a facing tick. This module is the whole
//! marker: which segments exist, their colours and sizes, and where each one
//! lands on the 320x240 display.
//!
//! It emits **quads** rather than lines because the browser play page has no
//! line primitive - its screen-space surface is the shared `screen_prim`
//! pass, which both hosts already run. Each segment becomes one quad (two
//! triangles) one display pixel wide, so the two hosts draw the same marker
//! set through the same pass, and the native window's line pipeline is no
//! longer needed for it.
//!
//! The projection is the frame's own: [`camera_view::frame_vp`] at the
//! display's 4:3 aspect, composed with the one world flip that takes the raw
//! retail Y-down marker position into that matrix's Y-up render frame.

use crate::camera_view::{self, FieldCameraFrame};
use crate::world::{World, WorldMapEntityKind, WorldMapEntityMarker, WorldMapPlayerMarker};

/// PSX display width the quads are authored in.
pub const DISPLAY_W: f32 = 320.0;
/// PSX display height the quads are authored in.
pub const DISPLAY_H: f32 = 240.0;
/// A segment's on-screen width, in display pixels.
pub const SEGMENT_WIDTH: f32 = 1.0;
/// Clip-space `w` below which an endpoint counts as behind the lens; a
/// segment with such an endpoint is dropped rather than wrapped through the
/// projection's pole.
pub const MIN_CLIP_W: f32 = 1.0;
/// Endpoints further than this from the display (in display pixels) are
/// off-screen for any purpose, and are dropped before the `i16` store.
pub const OFFSCREEN_LIMIT: f32 = 4096.0;

/// One marker segment in raw retail Y-down world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkerSegment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub rgba: [u8; 4],
}

/// One projected segment: four display-pixel corners in the `v0..v3` order
/// every `screen_prim` quad uses (`a+n`, `b+n`, `a-n`, `b-n`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerQuad {
    pub xy: [(i16, i16); 4],
    pub rgba: [u8; 4],
}

/// Colour key per entity kind: portals cyan, NPCs green, encounter zones red.
pub fn entity_color(kind: WorldMapEntityKind) -> [u8; 4] {
    match kind {
        WorldMapEntityKind::Portal => [0, 200, 255, 255],
        WorldMapEntityKind::Npc => [80, 220, 80, 255],
        WorldMapEntityKind::EncounterZone => [230, 80, 40, 255],
    }
}

/// The player marker's colour (white-yellow, so it reads above the kinds).
pub const PLAYER_COLOR: [u8; 4] = [255, 230, 60, 255];

/// Marker sizes in world units. Fixed rather than scaled off the scene box:
/// a kingdom's box spans tens of thousands of units and the walk camera
/// composes a 6x world scale, so a box-relative post towered over the frame.
/// Half a 128-unit map tile reads at the walk view; under the top view a
/// marker shrinks to the one-pixel floor [`segment_quad`] keeps.
pub const ENTITY_POST_H: f32 = 64.0;
/// Half-length of an entity marker's base cross arms.
pub const ENTITY_ARM: f32 = 24.0;
/// The player's post (taller, so it reads above the entity kinds).
pub const PLAYER_POST_H: f32 = 96.0;
/// Half-length of the player marker's base cross arms.
pub const PLAYER_ARM: f32 = 28.0;
/// Length of the player's facing tick.
pub const PLAYER_TICK: f32 = 56.0;

/// An entity marker's three segments: a vertical post (up = world `-Y`) and a
/// base cross along X and Z.
pub fn entity_segments(m: &WorldMapEntityMarker) -> [MarkerSegment; 3] {
    let (post_h, arm) = (ENTITY_POST_H, ENTITY_ARM);
    let [x, y, z] = m.world_pos;
    let rgba = entity_color(m.kind);
    [
        MarkerSegment {
            a: [x, y, z],
            b: [x, y - post_h, z],
            rgba,
        },
        MarkerSegment {
            a: [x - arm, y, z],
            b: [x + arm, y, z],
            rgba,
        },
        MarkerSegment {
            a: [x, y, z - arm],
            b: [x, y, z + arm],
            rgba,
        },
    ]
}

/// The player marker's four segments: a taller post, the base cross, and a
/// facing tick along the heading (PSX 12-bit angle, `0` = `+Z`, a quarter
/// turn = `+X`).
pub fn player_segments(p: &WorldMapPlayerMarker) -> [MarkerSegment; 4] {
    let (post_h, arm, tick) = (PLAYER_POST_H, PLAYER_ARM, PLAYER_TICK);
    let [x, y, z] = p.world_pos;
    let angle = (p.facing as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let rgba = PLAYER_COLOR;
    [
        MarkerSegment {
            a: [x, y, z],
            b: [x, y - post_h, z],
            rgba,
        },
        MarkerSegment {
            a: [x - arm, y, z],
            b: [x + arm, y, z],
            rgba,
        },
        MarkerSegment {
            a: [x, y, z - arm],
            b: [x, y, z + arm],
            rgba,
        },
        MarkerSegment {
            a: [x, y, z],
            b: [x + sin * tick, y, z + cos * tick],
            rgba,
        },
    ]
}

/// Every marker segment this frame: one set per placed entity, plus the
/// player's when `draw_player` (a host passes `false` while it draws the
/// party leader's real mesh, which the marker would otherwise stab through).
pub fn marker_segments(world: &World, draw_player: bool) -> Vec<MarkerSegment> {
    let mut out = Vec::new();
    for m in world.world_map_entity_markers() {
        out.extend(entity_segments(&m));
    }
    if draw_player && let Some(p) = world.world_map_player_marker() {
        out.extend(player_segments(&p));
    }
    out
}

/// Project raw Y-down world points through a Y-up `vp` onto the display.
/// `None` behind the lens or absurdly far off-screen.
fn project(vp: &[f32; 16], p: [f32; 3]) -> Option<[f32; 2]> {
    // WORLD_FLIP: the raw retail Y-down point in the matrix's Y-up frame.
    let v = [p[0], -p[1], p[2], 1.0];
    let row = |r: usize| (0..4).map(|c| vp[c * 4 + r] * v[c]).sum::<f32>();
    let (cx, cy, cw) = (row(0), row(1), row(3));
    if cw < MIN_CLIP_W {
        return None;
    }
    let sx = (cx / cw + 1.0) * 0.5 * DISPLAY_W;
    let sy = (1.0 - cy / cw) * 0.5 * DISPLAY_H;
    (sx.abs() < OFFSCREEN_LIMIT && sy.abs() < OFFSCREEN_LIMIT).then_some([sx, sy])
}

/// One segment as a display quad [`SEGMENT_WIDTH`] wide. A segment shorter
/// than that on screen still gets a square of that width, so a post seen
/// end-on stays visible.
pub fn segment_quad(vp: &[f32; 16], s: &MarkerSegment) -> Option<MarkerQuad> {
    let a = project(vp, s.a)?;
    let mut b = project(vp, s.b)?;
    let half = SEGMENT_WIDTH * 0.5;
    let (mut dx, mut dy) = (b[0] - a[0], b[1] - a[1]);
    let mut len = (dx * dx + dy * dy).sqrt();
    if len < SEGMENT_WIDTH {
        // End-on (a post under the top view): a square the segment's width.
        b = [a[0] + SEGMENT_WIDTH, a[1]];
        (dx, dy, len) = (SEGMENT_WIDTH, 0.0, SEGMENT_WIDTH);
    }
    let (nx, ny) = (-dy / len * half, dx / len * half);
    let pt = |x: f32, y: f32| (x.round() as i16, y.round() as i16);
    Some(MarkerQuad {
        xy: [
            pt(a[0] + nx, a[1] + ny),
            pt(b[0] + nx, b[1] + ny),
            pt(a[0] - nx, a[1] - ny),
            pt(b[0] - nx, b[1] - ny),
        ],
        rgba: s.rgba,
    })
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
    if world.mode != crate::world::SceneMode::WorldMap || world.cutscene_timeline_active() {
        return Vec::new();
    }
    if matches!(frame, FieldCameraFrame::Cutscene(_)) {
        return Vec::new();
    }
    let Some(vp) = camera_view::frame_vp(frame, aabb, DISPLAY_W / DISPLAY_H) else {
        return Vec::new();
    };
    marker_segments(world, draw_player)
        .iter()
        .filter_map(|s| segment_quad(&vp, s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SceneMode;

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
        let frame = camera_view::resolve_field_camera(
            &w,
            &crate::camera::Camera::default(),
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
        let frame = camera_view::resolve_field_camera(
            &w,
            &crate::camera::Camera::default(),
            None,
            [0.0, 0.0],
        );
        assert!(marker_quads(&w, &frame, AABB, false).is_empty());
    }

    #[test]
    fn nothing_outside_the_world_map() {
        let mut w = world_map_world();
        w.mode = SceneMode::Field;
        let frame = FieldCameraFrame::HostDebugOrbit;
        assert!(marker_quads(&w, &frame, AABB, true).is_empty());
    }

    #[test]
    fn segment_quad_is_one_pixel_wide_across_the_segment() {
        // Identity-ish vp: clip = (x, -y, z, 1) after the flip, so display
        // x = (x + 1) * 160 and y = (1 - (-y)) * 120 ... use a unit matrix.
        let mut vp = [0.0f32; 16];
        vp[0] = 1.0;
        vp[5] = 1.0;
        vp[10] = 1.0;
        vp[15] = 1.0;
        let s = MarkerSegment {
            a: [-0.5, 0.0, 0.0],
            b: [0.5, 0.0, 0.0],
            rgba: [1, 2, 3, 4],
        };
        let q = segment_quad(&vp, &s).unwrap();
        // Horizontal segment from x=80 to x=240 at y=120; half-width 0.5.
        assert_eq!(q.xy[0].0, 80);
        assert_eq!(q.xy[1].0, 240);
        assert!((q.xy[0].1 - q.xy[2].1).abs() <= 1);
    }

    #[test]
    fn behind_the_lens_is_dropped() {
        let mut vp = [0.0f32; 16];
        vp[0] = 1.0;
        vp[5] = 1.0;
        vp[10] = 1.0;
        // w = z
        vp[11] = 1.0;
        let s = MarkerSegment {
            a: [0.0, 0.0, -5.0],
            b: [0.0, 0.0, 5.0],
            rgba: [0; 4],
        };
        assert!(segment_quad(&vp, &s).is_none());
    }
}
