//! The map display's **place labels**: which records draw this frame and
//! where each lands on the 320x240 display.
//!
//! Retail's loop (`0x801CF368..0x801CF438` in PROT 0981's tick) takes every
//! record of the kingdom MAN's location table whose kingdom is the live one
//! and whose discovery flag is set, builds the world point
//! `(map_x << 7, -0x200, map_y << 7)`, projects it with `RTPS`
//! (`FUN_8003D368`) under the survey camera, and draws
//!
//! ```text
//! FUN_80036888(name, 0, 0, sx, sy - 1)       ; the name
//! FUN_80034B6C(0)                            ; stage window style 0
//! FUN_8002C69C(sx, sy, FUN_80035F04(name), 8); its box
//! ```
//!
//! Window style `0` is descriptor `0x800732A4[0]`, byte-identical to the
//! dialog skin `0x44`: the box is the ordinary gradient-filled 9-slice window
//! around a `width x 8` centre rect, not an underline.
//!
//! This module is the gate and the projection; the two draws are
//! `legaia_engine_ui::map_label_*`. The projection is the frame's own matrix
//! ([`camera_frame::frame_vp`]) at the display's 4:3 aspect, the same route
//! [`legaia_engine_world_views::world_map_markers`] takes.
//!
//! REF: FUN_801CE850, FUN_8003D368, FUN_8003CE64

use legaia_engine_core::world::World;
use legaia_engine_field::camera_frame;
use legaia_engine_field::camera_frame::FieldCameraFrame;
use legaia_engine_field::map_display::LABEL_WORLD_Y;
use legaia_engine_world_views::camera_view;
use legaia_render_kernels::screen_prim;

/// PSX display size the label pens are authored in.
const DISPLAY_W: f32 = 320.0;
const DISPLAY_H: f32 = 240.0;

/// One label: the record's name and the projected `(sx, sy)` retail hands
/// the glyph renderer and the window emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLabel {
    /// Index of the record in the location table.
    pub record: usize,
    /// The name's bytes, up to its NUL.
    pub name: Vec<u8>,
    /// Projected screen point. The name's pen is `(sx, sy - 1)`; the box's
    /// centre rect starts at `(sx, sy)`.
    pub sx: i32,
    pub sy: i32,
}

impl MapLabel {
    /// The glyph renderer's pen (`addiu v0,v0,-0x1` at `0x801CF3F8`).
    pub fn pen(&self) -> (i32, i32) {
        (self.sx, self.sy - 1)
    }

    /// The window emitter's centre rect for a name `width` pixels wide.
    pub fn box_rect(&self, width: i32) -> (i32, i32, i32, i32) {
        (self.sx, self.sy, width, 8)
    }
}

/// Project a raw Y-down world point through a Y-up `vp` onto the display.
/// `None` behind the lens.
fn project(vp: &[f32; 16], p: [f32; 3]) -> Option<(i32, i32)> {
    let v = [p[0], -p[1], p[2], 1.0];
    let row = |r: usize| (0..4).map(|c| vp[c * 4 + r] * v[c]).sum::<f32>();
    let (cx, cy, cw) = (row(0), row(1), row(3));
    if cw < 1.0 {
        return None;
    }
    let sx = (cx / cw + 1.0) * 0.5 * DISPLAY_W;
    let sy = (1.0 - cy / cw) * 0.5 * DISPLAY_H;
    (sx.abs() < 4096.0 && sy.abs() < 4096.0).then(|| (sx.floor() as i32, sy.floor() as i32))
}

/// This frame's labels, in table order. Empty unless the map display is in
/// its interactive mode with labels on and `frame` is its survey camera.
pub fn map_display_labels(world: &World, frame: &FieldCameraFrame) -> Vec<MapLabel> {
    let Some(display) = world.world_map.map_display.as_ref() else {
        return Vec::new();
    };
    if !matches!(frame, FieldCameraFrame::WorldMapDisplay { .. }) {
        return Vec::new();
    }
    let Some(vp) = camera_frame::frame_vp(frame, ([0.0; 3], [0.0; 3]), DISPLAY_W / DISPLAY_H)
    else {
        return Vec::new();
    };
    let kingdom = world.world_map_kingdom_index().map_or(u32::MAX, u32::from);
    let locations = &world.world_map.locations;
    display
        .visible_labels(locations, kingdom, world.map_display_show_all(), |f| {
            world.system_flag_test(f)
        })
        .into_iter()
        .filter_map(|i| {
            let r = &locations[i];
            let p = [
                f32::from(u16::from(r.map_x) << 7),
                f32::from(LABEL_WORLD_Y),
                f32::from(u16::from(r.map_y) << 7),
            ];
            let (sx, sy) = project(&vp, p)?;
            Some(MapLabel {
                record: i,
                name: r.name.clone(),
                sx,
                sy,
            })
        })
        .collect()
}

/// [`map_display_labels`] under the display's own survey frame - the form a
/// host calls. A scripted shot cannot own the camera while the display is
/// up: the display owns the whole frame and steps no timeline.
pub fn map_display_labels_now(world: &World) -> Vec<MapLabel> {
    match camera_view::map_display_frame(world) {
        Some(frame) => map_display_labels(world, &frame),
        None => Vec::new(),
    }
}

/// The overworld map display's mini-map panel as screen primitives, in
/// retail's call order (`0x801CF020..0x801CF344` of PROT 0981's tick): the
/// player dot, the view box, the mini-map, its darkening quad, then the four
/// compass plates. Every one links at ordering-table bucket `1`
/// (`AddPrim(ot + 4, ..)`), where the last linked draws first, so the dot
/// ends up on top of the map and the map on top of its backing.
///
/// The textures are the kingdom scene pack's own, resident at texpage `0xD`
/// for the whole overworld; nothing is uploaded for the display.
///
/// REF: FUN_801CE850, FUN_8001CD68
pub fn map_display_prims(world: &World) -> Vec<screen_prim::ScreenPrim> {
    use legaia_engine_field::map_display::{BACKING_RGB, ChromeSprite};
    let Some(c) = world.map_display_chrome() else {
        return Vec::new();
    };
    const BUCKET: u32 = 1;
    let sprite = |s: &ChromeSprite| {
        let (x1, y1) = (s.x + s.w, s.y + s.h);
        let (u1, v1) = (s.u.wrapping_add(s.w as u8), s.v.wrapping_add(s.h as u8));
        screen_prim::ScreenPrim::Textured(screen_prim::ScreenQuad {
            xy: [(s.x, s.y), (x1, s.y), (s.x, y1), (x1, y1)],
            uv: [(s.u, s.v), (u1, s.v), (s.u, v1), (u1, v1)],
            clut: s.clut,
            tpage: s.tpage,
            color: 0x80_80_80,
            gouraud: None,
            semi_transparent: s.semi,
            ot_index: BUCKET,
            depth: None,
        })
    };
    let (x0, y0, x1, y1) = c.backing;
    let [r, g, b] = BACKING_RGB;
    let mut out = vec![
        sprite(&c.player_dot),
        sprite(&c.view_box),
        sprite(&c.minimap),
        // `POLY_F4` `0x2A505050` under `SetDrawMode(tpage = 0x4C)`: ABR 2,
        // the background less the grey.
        screen_prim::ScreenPrim::Flat(screen_prim::FlatQuad {
            xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
            color: [r, g, b, 0xFF],
            gouraud: None,
            semi_transparent: true,
            abr_mode: 2,
            ot_index: BUCKET,
            depth: None,
        }),
    ];
    out.extend(c.compass.iter().map(|m| sprite(&m.sprite)));
    out
}

/// The compass letters of the mini-map panel as `(letter, pen)`, for the
/// host's text layer (`FUN_80036888` at `0x801CF240` and its three siblings).
pub fn map_display_compass_marks(world: &World) -> Vec<(u8, (i32, i32))> {
    world
        .map_display_chrome()
        .map(|c| {
            c.compass
                .iter()
                .map(|m| (m.letter, (i32::from(m.pen.0), i32::from(m.pen.1))))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::map_display::{MODE_INTERACTIVE, MapDisplay, MapLocation};

    fn world_with(locs: Vec<MapLocation>, cam: (i32, i32)) -> World {
        let mut w = World::new();
        w.enter_world_map();
        w.active_scene_label = "map01".into();
        w.world_map.locations = locs;
        let mut d = MapDisplay::enter();
        d.mode = MODE_INTERACTIVE;
        d.camera_staged = true;
        d.cam_x = cam.0;
        d.cam_z = cam.1;
        w.world_map.map_display = Some(d);
        w
    }

    fn loc(x: u8, y: u8, flag: u16) -> MapLocation {
        MapLocation {
            region: 0,
            map_x: x,
            map_y: y,
            discovery_flag: flag,
            name: b"Rim Elm".to_vec(),
        }
    }

    /// A place straight under the camera lands near the display's centre
    /// column, and one a few tiles east of it lands to its right.
    #[test]
    fn a_discovered_place_projects_where_the_survey_camera_puts_it() {
        let mut w = world_with(
            vec![loc(64, 64, 0x484), loc(70, 64, 0x485), loc(64, 64, 0x486)],
            (-(64 << 7), -(64 << 7)),
        );
        w.system_flag_set(0x484);
        w.system_flag_set(0x485);
        let labels = map_display_labels_now(&w);
        assert_eq!(
            labels.iter().map(|l| l.record).collect::<Vec<_>>(),
            vec![0, 1],
            "the undiscovered record draws nothing"
        );
        assert!((labels[0].sx - 160).abs() <= 1, "{:?}", labels[0]);
        assert!(labels[1].sx > labels[0].sx + 8, "{labels:?}");
        assert!((0..240).contains(&labels[0].sy), "{:?}", labels[0]);
        assert_eq!(labels[0].pen(), (labels[0].sx, labels[0].sy - 1));
        assert_eq!(labels[0].box_rect(40).3, 8);
    }

    #[test]
    fn nothing_draws_without_the_display_or_with_labels_off() {
        let mut w = world_with(vec![loc(64, 64, 0x484)], (-(64 << 7), -(64 << 7)));
        w.system_flag_set(0x484);
        w.world_map.map_display.as_mut().unwrap().overlay = 2;
        assert!(map_display_labels_now(&w).is_empty());
        w.world_map.map_display = None;
        assert!(map_display_labels_now(&w).is_empty());
    }

    /// The map display rebuilds the curvature table inside a running frame
    /// (`OFY = 114` against the builder's `120`), so from its mode `1` on
    /// every bent overworld draw sits six rows higher - through the display,
    /// on the walk frame after it, and until the next scene load rebuilds
    /// the table under the field init's `OFY`.
    #[test]
    fn the_map_display_leaves_the_curvature_table_six_rows_up() {
        use legaia_render_kernels::overworld_curvature::REBUILD_BIAS;
        let mut w = World::new();
        w.enter_world_map();
        w.active_scene_label = "map01".into();
        assert_eq!(w.overworld_curve_bias(), Some(0));
        w.world_map.map_display = Some(MapDisplay::enter());
        // Mode `0` only seeds the fade timer; mode `1` flattens the table.
        w.tick_map_display();
        assert_eq!(w.overworld_curve_bias(), Some(0));
        w.tick_map_display();
        assert_eq!(w.overworld_curve_bias(), Some(REBUILD_BIAS));
        // Closing the display does not put it back.
        w.world_map.map_display = None;
        assert_eq!(w.overworld_curve_bias(), Some(-6));
        // The next scene load's own build does.
        w.install_world_map_locations(None);
        assert_eq!(w.overworld_curve_bias(), Some(0));
        // Off the overworld no consumer reads the table.
        w.world_map.curve_bias = REBUILD_BIAS;
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        assert_eq!(w.overworld_curve_bias(), None);
    }
}
