//! Per-frame camera resolution: which camera owns this frame, and what its
//! retail GTE inputs are.
//!
//! [`crate::camera::Camera`] holds the ten retail camera globals and the
//! op-`0x45` event routing. This module is the layer above it that every host
//! calls: it reads the live [`World`] and answers with a [`FieldCameraFrame`]
//! (one arm per camera retail runs in a walkable scene) whose view resolves to
//! a [`FieldCameraView`] and, through [`FieldCameraView::vp`], the single
//! column-major matrix the host uploads.
//!
//! The point of the split is that **no host derives a camera angle**. The
//! native `play-window` and the browser play page ship different renderers on
//! one engine, and a camera each of them builds separately is a drift that no
//! gate can see: the two frames diverge without a shared symbol changing. The
//! browser page used to run its own orbit projection here - a spherical
//! `(yaw, pitch, halfWidth)` model that had to *approximate* the retail GTE
//! camera, and that the op-`0x45` cutscene shots were re-mapped onto - while
//! the native window consumed the retail model directly.
//!
//! The projection itself is [`legaia_engine_vm::psx_camera`], whose frame
//! convention (every matrix is for the **Y-up** render frame; a host keeping
//! raw retail Y-down world state post-multiplies one `scale(1,-1,1)`) is the
//! one rule both hosts follow.
//!
//! REF: FUN_801DE084 (the op-`0x45` Configure apply handler), FUN_801DBE9C
//! (the field follow camera), FUN_801E76D4 (the world-map controller).

use crate::camera::Camera;
use crate::world::{SceneMode, World};
use legaia_engine_vm::psx_camera::{self, FieldCameraView};

/// Field follow-camera pitch (`_DAT_8007B790`), PSX 12-bit units - the
/// town01 anchor savestate's value (~39.6 deg down-tilt).
pub const FIELD_PITCH_UNITS: f32 = 450.0;

/// Field follow-camera base yaw (`_DAT_8007B792`), PSX 12-bit units, from the
/// same anchor. The movement compass reads its negation (`alpha = -psi` for
/// the PSX GTE camera), which is what [`Camera::render_yaw_bias`] carries.
pub const FIELD_FOLLOW_YAW_UNITS: f32 = -160.0;

/// Field GTE `H` (`_DAT_8007B6F4`). `512` in the field, `256` in battle -
/// written per phase, unlike `OFX` / `OFY`.
pub const FIELD_H: f32 = 512.0;

/// Field follow-camera eye-back depth, in the engine's 1x world frame.
///
/// An engine calibration rather than a savestate read: retail's exact field
/// `TR` composition is not pinned (the anchor's offset trio does not project
/// to the observed framing), so this is fitted so the player's on-screen
/// height matches the retail frame - ~55 px of 240 for the ~130-unit mesh at
/// `H = 512`.
pub const FIELD_CAM_DEPTH: f32 = 1200.0;

/// Retail folds a 6x uniform world scale into the camera rotation (base
/// matrix `DAT_8007BF10` = `24576 * I`, GTE `4096` = 1.0); the engine renders
/// at 1x, so an op-`0x45` eye-space translation reduces by this factor.
pub const CUTSCENE_WORLD_SCALE: f32 = 6.0;

/// The same 6x scale as a *world* transform, which is how the overworld walk
/// view composes it - the continent draws at raw world tile coordinates and
/// the camera scales about the player.
pub const WORLD_MAP_WORLD_SCALE: f32 = 6.0;

/// Overworld walk-view GTE `H` (`_DAT_8007B6F4` on both resident overworld
/// savestates).
pub const WORLD_MAP_H: f32 = 368.0;

/// The two pinned overworld walk-view zoom states, read out of the sebucus and
/// karisto resident savestates: `(pitch units, tr_eye)`. The controller zoom
/// slides along the axis between them, anchored at the closer state.
pub const WORLD_MAP_ZOOM_PINS: [(f32, [f32; 3]); 2] = [
    (360.0, [0.0, 536.0, 9139.0]),
    (476.0, [0.0, 406.0, 11041.0]),
];

/// The fixed framing bias a host that renders the retail follow view pushes
/// into [`Camera::render_yaw_bias`], so
/// [`Camera::compass_azimuth_units`] reports the yaw the player actually
/// sees and the d-pad remap tracks the on-screen camera.
///
/// The compass sense is the PSX render yaw's negation (`alpha = -psi`), which
/// is the whole content of this function - and the reason it is a function
/// rather than a literal on each host is that a host that spells it out
/// locally can get the sign right on one side and wrong on the other, which
/// reads as "the controls invert at a quarter turn" rather than as a camera
/// bug. A headless host leaves the bias at `0` and keeps the historical
/// yaw-only feed bit-identical.
pub fn retail_field_render_yaw_bias() -> f32 {
    -FIELD_FOLLOW_YAW_UNITS / 4096.0 * std::f32::consts::TAU
}

fn to_rad(units: f32) -> f32 {
    units / 4096.0 * std::f32::consts::TAU
}

/// The lead actor's world position, or `None` when no player actor is live.
fn lead_actor_xz(world: &World) -> Option<(f32, f32)> {
    world
        .actors
        .first()
        .filter(|a| a.active || a.tmd_binding.is_some())
        .map(|a| (a.move_state.world_x as f32, a.move_state.world_z as f32))
}

/// The **retail field follow camera**'s inputs for this frame.
///
/// Pitch / yaw / `H` are the savestate-pinned anchors above; the look-at
/// target is the player anchor with its floor height sampled (retail's
/// follow-cam `FUN_801DBE9C` folds `-(anchor X/Z)` into the focus globals each
/// frame, and the port's `sample_field_floor_height` supplies the Y a raw
/// `world_y` of `0` would put under an elevated town tier). Two user knobs
/// compose onto the pinned base and are both retail-identical at their
/// defaults: [`Camera::distance`] scales the eye-back depth, and
/// [`Camera::manual_orbit`] swings the yaw around the player in the compass
/// sense - the PSX render yaw is its negation.
///
/// `None` when no player actor exists to follow; a host falls back to its own
/// debug vantage there.
///
/// REF: FUN_801DBE9C
pub fn field_follow_view(cam: &Camera, world: &World) -> Option<FieldCameraView> {
    let (wx, wz) = lead_actor_xz(world)?;
    let floor_y = world.sample_field_floor_height(wx as i32, wz as i32) as f32;
    Some(FieldCameraView {
        focus: [wx, floor_y, wz],
        pitch: to_rad(FIELD_PITCH_UNITS),
        // PSX camera yaw is the compass negation, so a positive manual orbit
        // subtracts from the render yaw.
        yaw: to_rad(FIELD_FOLLOW_YAW_UNITS) - cam.manual_orbit,
        // The field follow camera never rolls: `FUN_80025C24` seeds the roll
        // global to `0` on scene entry and only an op-`0x45` beat writes it.
        roll: 0.0,
        h: FIELD_H,
        tr_eye: [0.0, 0.0, FIELD_CAM_DEPTH * cam.distance.scale()],
    })
}

/// The **op-`0x45` cutscene shot**'s inputs, decoded from the camera state the
/// field VM staged.
///
/// Slot map (`FUN_801DE084` writes the globals, `FUN_8001CF50` builds the
/// rotation): `0` pitch, `1` yaw, `2` roll, `3/4/5` the eye-space translation
/// trio, `6/7/8` the focus (X and Z stored **negated**), `9` GTE `H`. Angles
/// are 12-bit (`4096` = full turn). A beat that omits a slot keeps the prior
/// value, which is what the per-slot fallbacks below encode:
///
/// * focus X/Z fall back to the lead actor - the cutscene anchor - and focus Y
///   to retail's `0` (the vertical framing rides `tr_eye`, not the focus);
/// * pitch falls back to the historical fixed ~24 deg framing so an
///   absent-pitch shot is unchanged;
/// * `H` falls back to the field `512`, and the eye trio to a mid cutscene
///   depth (opdeene always supplies all three).
///
/// `fallback_focus_xz` is used only when there is no lead actor either; hosts
/// pass their scene centre.
///
/// REF: FUN_801DE084
pub fn cutscene_view(world: &World, fallback_focus_xz: [f32; 2]) -> FieldCameraView {
    let params = &world.camera.state.params;
    let param = |slot: u8| {
        params
            .iter()
            .find(|p| p.slot == slot)
            .map(|p| p.value as i16 as f32)
    };
    let (px, pz) = lead_actor_xz(world).unwrap_or((fallback_focus_xz[0], fallback_focus_xz[1]));
    let s = CUTSCENE_WORLD_SCALE;
    FieldCameraView {
        focus: [
            param(6).map(|v| -v).unwrap_or(px),
            param(7).unwrap_or(0.0),
            param(8).map(|v| -v).unwrap_or(pz),
        ],
        pitch: param(0)
            .map(|v| v / 4096.0 * std::f32::consts::TAU)
            .unwrap_or_else(|| 0.45f32.atan()),
        yaw: param(1)
            .map(|v| v / 4096.0 * std::f32::consts::TAU)
            .unwrap_or(0.0),
        roll: param(2)
            .map(|v| v / 4096.0 * std::f32::consts::TAU)
            .unwrap_or(0.0),
        h: param(9).filter(|&h| h > 1.0).unwrap_or(FIELD_H),
        tr_eye: [
            param(3).unwrap_or(0.0) / s,
            param(4).unwrap_or(1200.0) / s,
            param(5).filter(|&z| z.abs() > 1.0).unwrap_or(17000.0) / s,
        ],
    }
}

/// The **overworld walk view**'s inputs for a controller zoom.
///
/// Pinned from the two resident overworld savestates (sebucus / karisto):
/// `screen = H * (R*(6*(v - player)) + TR) / Ez` with `H = 368`, `R` from the
/// `0x8007B790` trio (pitch-only at azimuth 0; the controller azimuth feeds
/// the yaw), focus = the player's world X/Z (`0x80089118/20` hold its
/// negation, Y = 0) and `TR` from `0x800840B8`. Controller zoom is
/// positive-in, so negative values pull back along the pinned axis.
///
/// The 6x world scale is *not* in this view - it is a world transform the
/// projection composes under the camera; see [`world_map_walk_vp`].
pub fn world_map_walk_view(azimuth: i32, zoom: i32) -> FieldCameraView {
    let t = ((-zoom) as f32 / 64.0).clamp(0.0, 1.0);
    let (p0, tr0) = WORLD_MAP_ZOOM_PINS[0];
    let (p1, tr1) = WORLD_MAP_ZOOM_PINS[1];
    FieldCameraView {
        focus: [0.0; 3],
        pitch: to_rad(p0 + t * (p1 - p0)),
        yaw: to_rad(azimuth as f32),
        // The world-map camera carries no roll: `_DAT_8007B794` is the
        // top-view AZIMUTH there, and that is already the yaw above.
        roll: 0.0,
        h: WORLD_MAP_H,
        tr_eye: [
            tr0[0] + t * (tr1[0] - tr0[0]),
            tr0[1] + t * (tr1[1] - tr0[1]),
            tr0[2] + t * (tr1[2] - tr0[2]),
        ],
    }
}

/// [`world_map_walk_view`]'s matrix, with the retail 6x world scale about the
/// player composed under the camera. Y-up render frame (see the module docs).
///
/// `player` is the player's world position with `y = 0`, exactly as the focus
/// globals hold it. Uniform scale commutes with the frame flip, so this one
/// matrix serves a Y-up host directly and a raw-Y-down host after the usual
/// single `scale(1,-1,1)` post-multiply.
pub fn world_map_walk_vp(view: &FieldCameraView, player: [f32; 3], aspect: f32) -> [f32; 16] {
    psx_camera::mat4_mul(
        &view.vp(aspect),
        &psx_camera::mat4_mul(
            &psx_camera::mat4_scale(WORLD_MAP_WORLD_SCALE),
            &psx_camera::mat4_translation([-player[0], -player[1], -player[2]]),
        ),
    )
}

/// Which camera owns this frame. One arm per camera retail runs in a walkable
/// scene; battle has its own kernel
/// ([`legaia_engine_vm::battle_cam_script::battle_vp`]) and is not resolved
/// here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldCameraFrame {
    /// A cutscene timeline staged op-`0x45` params. Wins over every
    /// mode-derived camera, the world map included - the opening chain's
    /// `map01` leg is the retail Rim Elm aerial fly-in, driven by the same
    /// globals as the field cutscenes.
    Cutscene(FieldCameraView),
    /// Kingdom overworld, walk mode: the retail player-follow vantage.
    WorldMapWalk {
        view: FieldCameraView,
        player: [f32; 3],
    },
    /// Kingdom overworld, top-view debug (`DAT_801F2B94 != 0`): the survey
    /// vantage the R1+R2+Cross chord toggles.
    WorldMapTopView {
        azimuth: i32,
        zoom: i32,
        pan: [i32; 2],
    },
    /// Field free-roam: the retail follow camera.
    Follow(FieldCameraView),
    /// No player actor to follow and no scripted shot - the host frames the
    /// scene with its own debug vantage.
    HostDebugOrbit,
}

/// Resolve this frame's camera from the live world.
///
/// `cutscene` is the host's already-glided cutscene view (see
/// [`legaia_engine_vm::psx_camera::CutsceneCameraInterp`]) or `None` when no
/// timeline owns the frame; `scene_center_xz` is the host's fallback focus.
///
/// The world-map gate is the one place a reading is easy to get wrong: a
/// timeline running on the overworld only takes the camera when it actually
/// **staged a param**, so a world-map beat record with no camera beats (the
/// Drake mist-wall force-walk bands) keeps the ordinary walk camera.
pub fn resolve_field_camera(
    world: &World,
    cam: &Camera,
    cutscene: Option<FieldCameraView>,
    scene_center_xz: [f32; 2],
) -> FieldCameraFrame {
    if let Some(v) = cutscene {
        return FieldCameraFrame::Cutscene(v);
    }
    if world.mode == SceneMode::WorldMap {
        let (az, zoom, px, pz, top_view) = world
            .world_map
            .ctrl
            .as_ref()
            .map(|c| (c.azimuth, c.zoom, c.camera_x, c.camera_z, c.is_top_view()))
            .unwrap_or((0, 0, 0, 0, false));
        if top_view {
            return FieldCameraFrame::WorldMapTopView {
                azimuth: az,
                zoom,
                pan: [px, pz],
            };
        }
        let player = world
            .player_actor_slot
            .and_then(|s| world.actors.get(s as usize))
            .map(|a| {
                [
                    a.move_state.world_x as f32,
                    0.0,
                    a.move_state.world_z as f32,
                ]
            })
            .unwrap_or([scene_center_xz[0], 0.0, scene_center_xz[1]]);
        return FieldCameraFrame::WorldMapWalk {
            view: world_map_walk_view(az, zoom),
            player,
        };
    }
    match field_follow_view(cam, world) {
        Some(v) => FieldCameraFrame::Follow(v),
        None => FieldCameraFrame::HostDebugOrbit,
    }
}

/// The matrix for a resolved frame, Y-up render frame. `None` for
/// [`FieldCameraFrame::HostDebugOrbit`], which is the host's own vantage.
///
/// `scene_aabb` is the loaded meshes' bounding box; only the top-view debug
/// camera reads it.
pub fn frame_vp(
    frame: &FieldCameraFrame,
    scene_aabb: ([f32; 3], [f32; 3]),
    aspect: f32,
) -> Option<[f32; 16]> {
    match frame {
        FieldCameraFrame::Cutscene(v) | FieldCameraFrame::Follow(v) => Some(v.vp(aspect)),
        FieldCameraFrame::WorldMapWalk { view, player } => {
            Some(world_map_walk_vp(view, *player, aspect))
        }
        FieldCameraFrame::WorldMapTopView { azimuth, zoom, pan } => {
            Some(psx_camera::world_map_top_view_vp(
                scene_aabb.0,
                scene_aabb.1,
                *azimuth,
                *zoom,
                pan[0],
                pan[1],
                aspect,
            ))
        }
        FieldCameraFrame::HostDebugOrbit => None,
    }
}

/// The world-space lens for a resolved frame, in raw retail Y-down world
/// coordinates - what the camera-occlusion fade's visibility gate ray-casts
/// from. `None` where the frame has no retail-model eye (the top-view debug
/// camera and the host's own orbit).
pub fn frame_eye(frame: &FieldCameraFrame) -> Option<[f32; 3]> {
    match frame {
        FieldCameraFrame::Cutscene(v) | FieldCameraFrame::Follow(v) => Some(v.eye()),
        FieldCameraFrame::WorldMapWalk { view, player } => {
            // The walk camera orbits the player through the 6x world scale,
            // so its eye divides back out of that frame.
            let e = view.eye();
            Some([
                player[0] + e[0] / WORLD_MAP_WORLD_SCALE,
                player[1] + e[1] / WORLD_MAP_WORLD_SCALE,
                player[2] + e[2] / WORLD_MAP_WORLD_SCALE,
            ])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::Position as ActorVmPosition;

    fn world_with_player(x: i16, z: i16) -> World {
        let mut w = World::default();
        let a = w.spawn_actor(0);
        a.default_pos = ActorVmPosition::new(x, 0);
        a.move_state.world_x = x;
        a.move_state.world_z = z;
        w.player_actor_slot = Some(0);
        w
    }

    #[test]
    fn follow_view_anchors_on_the_player_and_carries_the_pinned_angles() {
        let w = world_with_player(1200, -400);
        let cam = Camera::default();
        let v = field_follow_view(&cam, &w).expect("player actor");
        assert_eq!(v.focus[0], 1200.0);
        assert_eq!(v.focus[2], -400.0);
        assert_eq!(v.h, FIELD_H);
        assert_eq!(v.roll, 0.0);
        assert!((v.pitch - to_rad(FIELD_PITCH_UNITS)).abs() < 1e-6);
        assert!((v.yaw - to_rad(FIELD_FOLLOW_YAW_UNITS)).abs() < 1e-6);
        assert_eq!(v.tr_eye[2], FIELD_CAM_DEPTH);
    }

    /// The manual orbit is the PSX yaw's negation, and the distance preset
    /// scales the eye-back depth only - nothing else in the pose moves.
    #[test]
    fn follow_view_composes_the_two_user_knobs_and_nothing_else() {
        let w = world_with_player(0, 0);
        let base = field_follow_view(&Camera::default(), &w).unwrap();
        let mut cam = Camera::default();
        cam.manual_orbit = 0.5;
        cam.distance = crate::camera::CameraDistance::Far;
        cam.follow_slot = 0;
        let v = field_follow_view(&cam, &w).unwrap();
        assert!((v.yaw - (base.yaw - 0.5)).abs() < 1e-6);
        assert!((v.tr_eye[2] - FIELD_CAM_DEPTH * 1.35).abs() < 1e-3);
        assert_eq!(v.pitch, base.pitch);
        assert_eq!(v.focus, base.focus);
        assert_eq!(v.h, base.h);
    }

    /// The follow camera's analytic eye is behind and above the player, in
    /// raw retail Y-down coordinates (up = negative Y).
    #[test]
    fn follow_eye_sits_above_the_focus_in_y_down_world() {
        let w = world_with_player(0, 0);
        let v = field_follow_view(&Camera::default(), &w).unwrap();
        let eye = v.eye();
        assert!(eye[1] < -100.0, "eye Y = {} (should be above)", eye[1]);
        let planar = (eye[0] * eye[0] + eye[2] * eye[2]).sqrt();
        assert!(planar > 100.0, "eye should stand off the focus");
    }

    #[test]
    fn a_player_less_field_frame_falls_through_to_the_host_vantage() {
        let w = World::default();
        let f = resolve_field_camera(&w, &Camera::default(), None, [0.0, 0.0]);
        assert_eq!(f, FieldCameraFrame::HostDebugOrbit);
        assert!(frame_vp(&f, ([0.0; 3], [0.0; 3]), 4.0 / 3.0).is_none());
    }

    #[test]
    fn a_staged_cutscene_view_wins_over_the_follow_camera() {
        let w = world_with_player(10, 20);
        let staged = FieldCameraView {
            focus: [1.0, 2.0, 3.0],
            pitch: 0.1,
            yaw: 0.2,
            roll: 0.3,
            h: 400.0,
            tr_eye: [0.0, 10.0, 900.0],
        };
        let f = resolve_field_camera(&w, &Camera::default(), Some(staged), [0.0, 0.0]);
        assert_eq!(f, FieldCameraFrame::Cutscene(staged));
    }

    /// The zoom slides along the pinned axis: the two pins are the endpoints
    /// and nothing outside them is reachable.
    #[test]
    fn world_map_walk_zoom_interpolates_between_the_two_pinned_states() {
        let near = world_map_walk_view(0, 0);
        let far = world_map_walk_view(0, -64);
        assert!((near.pitch - to_rad(WORLD_MAP_ZOOM_PINS[0].0)).abs() < 1e-6);
        assert!((far.pitch - to_rad(WORLD_MAP_ZOOM_PINS[1].0)).abs() < 1e-6);
        assert_eq!(near.tr_eye, WORLD_MAP_ZOOM_PINS[0].1);
        assert_eq!(far.tr_eye, WORLD_MAP_ZOOM_PINS[1].1);
        // Positive (zoom-in) and past-the-far zooms clamp onto the pins.
        assert_eq!(world_map_walk_view(0, 40).tr_eye, near.tr_eye);
        assert_eq!(world_map_walk_view(0, -400).tr_eye, far.tr_eye);
    }

    /// The op-`0x45` slot decode: focus X/Z come back **negated** out of the
    /// globals, the eye trio divides by the folded-in 6x world scale, and an
    /// absent slot takes its own fallback rather than voiding the beat.
    #[test]
    fn cutscene_view_decodes_the_op45_slots_per_slot() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = world_with_player(700, 800);
        w.camera.state.params = vec![
            CameraParam {
                slot: 6,
                value: 0x2000u16.wrapping_neg(),
            },
            CameraParam {
                slot: 1,
                value: 1024,
            },
            CameraParam {
                slot: 5,
                value: 12000,
            },
        ];
        let v = cutscene_view(&w, [0.0, 0.0]);
        // Slot 6 held -(-0x2000) = 0x2000 as the world focus X.
        assert_eq!(v.focus[0], 8192.0);
        // Slot 8 absent -> the lead actor's Z.
        assert_eq!(v.focus[2], 800.0);
        assert!((v.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((v.tr_eye[2] - 12000.0 / CUTSCENE_WORLD_SCALE).abs() < 1e-3);
        // Slot 9 absent -> the field H.
        assert_eq!(v.h, FIELD_H);
    }
}
