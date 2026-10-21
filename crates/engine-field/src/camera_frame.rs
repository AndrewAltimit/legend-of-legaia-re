//! The World-free half of the per-frame camera: which camera owns a frame
//! ([`FieldCameraFrame`]), its matrix and eye, and the fixed retail poses and
//! per-pass constants every host reads.
//!
//! Resolving *which* arm a frame takes reads the live world, and sits above
//! this crate (`legaia_engine_world_views::camera_view::resolve_field_camera`);
//! everything here is a function of the resolved frame alone, so the
//! kernels that project through a frame (the fog pool, the overworld ground
//! cue, the curvature bend) name it directly.
//!
//! The projection itself is [`legaia_engine_camera::psx_camera`], whose frame
//! convention (every matrix is for the **Y-up** render frame; a host keeping
//! raw retail Y-down world state post-multiplies one `scale(1,-1,1)`) is the
//! one rule both hosts follow.

use crate::camera::CUTSCENE_WORLD_SCALE;
use legaia_engine_camera::psx_camera;
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_mode::mode::SceneMode;

/// Field follow-camera pitch (`_DAT_8007B790`), PSX 12-bit units - the town01
/// anchor savestate's value (~39.6 deg down-tilt).
///
/// **A fallback, not the field camera.** Retail derives the pitch per scene
/// and per tile: the camera-region record the tile query hits is split into
/// the camera parameter block (`FUN_801DBC20`, [`crate::camera_zone`]), the
/// composer `FUN_801DAB90` turns the block plus the player's position into a
/// target and the per-frame ease walks `_DAT_8007B790` toward it. That path
/// is [`Camera::zone`](crate::camera::Camera::zone), live whenever the world carries field
/// terrain, and `legaia_engine_world_views::camera_view::field_follow_view` reads its output. This constant is what
/// a world with **no** terrain loaded (a unit world, a bare test scene)
/// frames with - the anchor state's own reading, which the zone camera
/// reproduces there from `town01`'s record.
pub const FIELD_PITCH_UNITS: f32 = 450.0;

/// Field follow-camera base yaw (`_DAT_8007B792`), PSX 12-bit units, from the
/// same anchor. The movement compass reads its negation (`alpha = -psi` for
/// the PSX GTE camera), which is what [`Camera::render_yaw_bias`](crate::camera::Camera::render_yaw_bias) declares -
/// and, while the zone camera drives the frame, what
/// [`Camera::compass_azimuth_units`](crate::camera::Camera::compass_azimuth_units) recomputes from the live yaw.
///
/// The same fallback rule as [`FIELD_PITCH_UNITS`]: retail's field yaw is a
/// per-frame *output* of the composer (a position-proportional sweep across
/// the walk-region box in modes 1 / 2, a bearing in modes 3 / 4), and the
/// zone camera produces it; this value frames only a terrain-less world.
pub const FIELD_FOLLOW_YAW_UNITS: f32 = -160.0;

/// Field GTE `H` (`_DAT_8007B6F4`) fallback. `512` in the field, `256` in
/// battle - written per phase, unlike `OFX` / `OFY`.
///
/// `legaia_engine_world_views::camera_view::field_follow_view` prefers the live
/// [`legaia_engine_camera::retail_cam::RetailCamGlobals::h`] whenever the camera carries one -
/// the zone camera's composed `B618`, or an op-`0x45` slot-`9` beat. Retail's
/// scene-entry reset leaves `H` at `0` ("as the scene establishes it"), which
/// is what makes this the fallback rather than dead code.
pub const FIELD_H: f32 = 512.0;

/// The retail eye-depth the terrain-less fallback frames with: the
/// zone-miss / default block's `B614 = 0x4000`, in retail GTE units.
/// [`FIELD_CAM_DEPTH`] is this divided by [`CUTSCENE_WORLD_SCALE`].
pub const RETAIL_FIELD_DEPTH_UNITS: f32 = 16384.0;

/// Field follow-camera eye-back depth for a world with **no field terrain**,
/// in the engine's 1x world frame: the zone-miss block's `B614 = 0x4000`
/// reduced by the 6x world scale retail folds into its camera rotation
/// (`RETAIL_FIELD_DEPTH_UNITS / CUTSCENE_WORLD_SCALE`).
///
/// Derived, not fitted. The field `TR` composition is pinned: the once-per-
/// frame view builder `FUN_800172C0` copies the eye-space translation trio
/// `_DAT_800840B8/BC/C0` into the working matrix's `t`
/// (`FUN_8005B4B8` at `0x80017334`), MVMVAs the negated focus
/// `_DAT_80089118/1C/20` through the scaled rotation into that same `t`
/// (`FUN_8003D344` at `0x80017370`, writing `0x1F8003DC` = the matrix's
/// `+0x14`), and uploads it as GTE `TR` (`FUN_8005B6A8` at `0x8001737C`).
/// The rotation is `_DAT_8007BF10 * Rot(_DAT_8007B790..94)`
/// (`FUN_80026988` + `FUN_8005B3A8` at `0x80017320`), and a live `town01`
/// field state holds `_DAT_8007BF10 = 24576 * I` - a 6x uniform scale. So
/// retail's transform is `screen = proj(H) * (S * Rot * (v - focus) +
/// tr_eye)`, and a 1x renderer reproduces it pixel-for-pixel with
/// `tr_eye / S` - the same reduction `legaia_engine_world_views::camera_view::cutscene_view` applies to an
/// op-`0x45` beat's offset trio.
///
/// While the zone camera is live, `legaia_engine_world_views::camera_view::field_follow_view` feeds the whole
/// composed trio instead of this constant.
pub const FIELD_CAM_DEPTH: f32 = RETAIL_FIELD_DEPTH_UNITS / CUTSCENE_WORLD_SCALE;

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
/// into [`Camera::render_yaw_bias`](crate::camera::Camera::render_yaw_bias), so
/// [`Camera::compass_azimuth_units`](crate::camera::Camera::compass_azimuth_units) reports the yaw the player actually
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

/// PSX 12-bit angle units (`4096` = a full turn) to radians.
pub fn to_rad(units: f32) -> f32 {
    units / 4096.0 * std::f32::consts::TAU
}

/// How far above the feet (raw retail Y-down, so subtracted) the follow
/// knobs' pivot sits: half a ~130-unit field character, so a tilt or a zoom
/// turns and dollies about the body's centre and the character holds its
/// screen point through both.
pub const FOLLOW_PIVOT_LIFT: f32 = 64.0;

/// The **terrain-less fallback** overworld walk view, for a controller zoom.
///
/// A world-map scene with its field terrain loaded frames through the zone
/// camera instead (`legaia_engine_world_views::camera_view::resolve_field_camera`'s world-map arm), which is what
/// retail runs; this pinned pose is only what a world with no zone table
/// (unit worlds, a headless overworld) is framed with. Two resident states
/// sit on it, but the per-region captures in `docs/subsystems/world-map.md`
/// show it is not one zoom axis.
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

/// Re-express a zone-camera `legaia_engine_world_views::camera_view::field_follow_view` pose in the overworld walk
/// frame [`world_map_walk_vp`] composes: focus at the origin (the player
/// translation carries it), eye trio back in retail GTE units (the follow
/// view divides it by the 6x world scale; the walk frame scales the world
/// instead - the same transform, and the one whose eye-space depth is
/// retail's). `azimuth` is the top-view controller's, which retail leaves
/// at `0` in walk mode; it is folded into the yaw so the camera-relative
/// d-pad remap, which reads it, keeps agreeing with the frame.
pub fn world_map_view_from_follow(v: &FieldCameraView, azimuth: i32) -> FieldCameraView {
    FieldCameraView {
        focus: [0.0; 3],
        yaw: v.yaw + to_rad(azimuth as f32),
        tr_eye: v.tr_eye.map(|c| c * WORLD_MAP_WORLD_SCALE),
        ..*v
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
/// ([`legaia_engine_camera::projection::battle_vp`]) and is not resolved
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
    /// Kingdom overworld, the L1 **map display** (game mode `0x0D`): the
    /// survey pose PROT 0981 stages - `TR = (0, 0, 0x9470)`, pitch `0x300`,
    /// `H = 0x170` - composed like the walk frame, over the display's own
    /// camera translation instead of the player. `player` is that focus.
    WorldMapDisplay {
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
    /// A minigame venue's own staged camera - the dance floor's, from the
    /// overlay entry's record (`legaia_engine_minigame_scenes::dance_venue::venue_camera`). A fixed
    /// retail pose in the field frame; no follow knob reaches it.
    Venue(FieldCameraView),
    /// No player actor to follow and no scripted shot - the host frames the
    /// scene with its own debug vantage.
    HostDebugOrbit,
}

impl FieldCameraFrame {
    /// This frame's pose in the **field** frame - 1x world, focus on the
    /// player, eye trio reduced by the 6x world scale - the form the screen
    /// effects that project raw world points take (the fog pool, the move-VM
    /// strips, the attached lights). The overworld walk arm converts back
    /// from its scaled-world form; the transform is the same one, so a sheet
    /// projected through this view lands where the terrain does. `None` for
    /// the two vantages with no retail pose (the top-view debug camera and
    /// the host's orbit).
    pub fn field_view(&self) -> Option<FieldCameraView> {
        match *self {
            FieldCameraFrame::Cutscene(v)
            | FieldCameraFrame::Follow(v)
            | FieldCameraFrame::Venue(v) => Some(v),
            FieldCameraFrame::WorldMapWalk { view, player }
            | FieldCameraFrame::WorldMapDisplay { view, player } => Some(FieldCameraView {
                focus: player,
                tr_eye: view.tr_eye.map(|c| c / WORLD_MAP_WORLD_SCALE),
                ..view
            }),
            FieldCameraFrame::WorldMapTopView { .. } | FieldCameraFrame::HostDebugOrbit => None,
        }
    }
}

/// Retail GTE **NCLIP** winding-rejection mode for this frame's scene pass -
/// the word both hosts hand their renderer (`Renderer::set_backface_cull` /
/// the play page's `setNclipCull`).
///
/// `2` = reject the retail back faces, `0` = draw both sides.
///
/// Retail rejects back faces on every field mesh draw. Each per-prim handler
/// behind the dispatcher `FUN_80043390` runs `NCLIP` after `RTPT` and ANDs the
/// signed area with a mask before its sign test (quad kind 13: `cop2
/// 0x1400006`, `mfc2 s2,$24`, `and s2,s2,s3` at `0x80043814`, `blez`; the
/// quad draws when either half faces the eye). The dispatcher loads that mask
/// as `0xFFFFFFFF` and only lowers it to `0x7FFFFFFF` - which makes every
/// area non-negative, i.e. double-sided - when the colour argument carries
/// bit `0x08000000` (`0x80043520..0x80043540`). A field actor's colour word
/// (`actor[+0x74]`) is born `0x00808080` (`FUN_80020DE0`) and the two
/// placed-object spawners only OR in `0x40000000` / `0x10000000`
/// (`FUN_8003A55C` at `0x8003A730..0x8003A76C`, the window sweep
/// `FUN_801D7B50` at `0x801D7D78..0x801D7DB4`), so a placed object is
/// single-sided. Drawing both sides is what let a sky dome the camera looks
/// at from outside (korout's, retona's) paint its outer shell over the scene.
///
/// So the mode is armed for the whole **field** pass - free roam and the
/// in-engine cutscene camera alike - and for a cutscene camera on any other
/// non-overworld mode. The world map keeps both-sided draws: its continent
/// terrain's winding parity is the world-map pass's, not the field pass's,
/// and the field-tuned cull would eat the ground tiles. Battle and the
/// other minigame venues keep theirs (a different per-pass parity).
///
/// The **dance hall** is armed too. The dance frame draws its hall the way
/// the field does - the render library's decoration pass (game mode `0x19`
/// is one of the three `FUN_80026CE4` runs `FUN_801F7088` for) plus the
/// placed objects through the same prim dispatcher - and a live dance
/// capture has those placed actors' colour words at `0x40808080`, without
/// the double-sided bit. The native window draws the hall in raw world
/// coordinates under the field frame's flip, so the field parity is its
/// parity.
///
/// Shared so the two hosts cannot arm it on different frames; each passes the
/// two values it can answer locally.
///
/// REF: FUN_80043390, FUN_80043768
pub fn nclip_cull_mode(cutscene_camera_active: bool, mode: SceneMode) -> u32 {
    let armed = matches!(mode, SceneMode::Field | SceneMode::Dance)
        || (cutscene_camera_active && mode != SceneMode::WorldMap);
    u32::from(armed) * 2
}

/// The ordering-table shift byte `0x1F8003A4` on the field: `3` in every
/// catalogued field state.
pub const FIELD_OT_SHIFT: u32 = 3;

/// The ordering-table shift byte `0x1F8003A4` in battle: `2` in every
/// catalogued battle state.
pub const BATTLE_OT_SHIFT: u32 = 2;

/// The projection `H` the GPU polygon-size limit is tested under this frame
/// (`legaia_render_kernels::prim_near_reject::gpu_span_rejected`), or `None` when
/// the pass does not apply it. The limit is the GPU's, so it holds wherever
/// the per-prim leaves hand it raw `SXY`: in battle (every catalogued battle
/// camera reads `H = 256`; a summon close-up seats a combatant between the
/// eye and the caster, `theeder_summon_mid_cast`'s monster `0xA1`) and on
/// the field, under the frame's own `H` (`field_h`, the live camera
/// globals' `_DAT_8007B6F4`). `rim_elm_queen_bee_battle`'s camera stands
/// beside a decoration rock whose near face projects past the span: retail's
/// GPU skips it and the frame shows the sky behind. The overworld and the
/// minigame venues keep their own rules.
pub fn prim_gpu_span_h(mode: SceneMode, retail_camera: bool, field_h: f32) -> Option<f32> {
    if !retail_camera {
        return None;
    }
    match mode {
        SceneMode::Battle => Some(BATTLE_GTE_H),
        SceneMode::Field if field_h > 0.0 => Some(field_h),
        _ => None,
    }
}

/// The battle camera's projection distance `H`.
pub const BATTLE_GTE_H: f32 = 256.0;

/// Retail's per-primitive near reject for this frame, as `(sz_per_w,
/// ot_shift)` for the renderer - or `None` when the pass does not run it.
///
/// Every TMD prim handler behind `FUN_80043390` drops a primitive whose
/// `OTZ` (mean corner `SZ >> ot_shift`) is below the scratch floor
/// `0x1F80037E = 0x10` (`sub s1,s2,t4` / `bltz s1` at `0x80043870` in
/// `FUN_80043768`); see `legaia_render_kernels::prim_near_reject`. The field and
/// battle cameras both put the GTE eye depth in clip `w`, so `sz_per_w` is
/// `1`. The kingdom overworld composes a world scale onto its camera and
/// draws through the overlay's own replacement handlers, so it stays off, as
/// do the minigame venues; `retail_camera = false` (the debug orbit) frames
/// from a vantage retail never had and draws everything.
///
/// Shared so the two hosts cannot arm it on different frames.
///
/// REF: FUN_80043390, FUN_80043768
pub fn prim_near_cut(mode: SceneMode, retail_camera: bool) -> Option<(f32, u32)> {
    if !retail_camera {
        return None;
    }
    match mode {
        SceneMode::Field => Some((1.0, FIELD_OT_SHIFT)),
        SceneMode::Battle => Some((1.0, BATTLE_OT_SHIFT)),
        _ => None,
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
        FieldCameraFrame::Cutscene(v)
        | FieldCameraFrame::Follow(v)
        | FieldCameraFrame::Venue(v) => Some(v.vp(aspect)),
        FieldCameraFrame::WorldMapWalk { view, player }
        | FieldCameraFrame::WorldMapDisplay { view, player } => {
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
        FieldCameraFrame::Cutscene(v)
        | FieldCameraFrame::Follow(v)
        | FieldCameraFrame::Venue(v) => Some(v.eye()),
        FieldCameraFrame::WorldMapWalk { view, player }
        | FieldCameraFrame::WorldMapDisplay { view, player } => {
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

/// How the hosts' mesh shaders turn a draw's `clip.w` into retail's `SZ`
/// for the curvature index, for this frame - `0.0` switches the bend off.
///
/// On while retail's overworld bit is up (`World::overworld_bit`)
/// and the frame is one of retail's own cameras:
///
/// - the overworld **walk** camera composes the 6x world scale into its
///   matrix ([`world_map_walk_vp`]), so its `clip.w` already is
///   `SZ` - factor `1`;
/// - a **scripted** shot (the `map01` aerial fly-in) and the field follow
///   pose are `1x` frames, so `SZ` is `clip.w` times the base matrix's scale
///   ([`CUTSCENE_WORLD_SCALE`]).
///
/// The top-view debug camera and the host's own orbit have no retail eye to
/// bend about, and keep the flat projection.
pub fn frame_curve_scale(overworld: bool, frame: &FieldCameraFrame) -> f32 {
    use FieldCameraFrame as F;
    if !overworld {
        return 0.0;
    }
    match frame {
        F::WorldMapWalk { .. } => 1.0,
        F::Cutscene(_) | F::Follow(_) => CUTSCENE_WORLD_SCALE,
        // The map display flattens the curvature table for its own
        // duration (`FUN_800271A8(0, 0)` at `0x801CE958`, rebuilt from the
        // saved coefficients on the way out at `0x801CEB88`).
        F::WorldMapDisplay { .. } => 0.0,
        // A minigame venue is never the overworld; flat.
        F::WorldMapTopView { .. } | F::HostDebugOrbit | F::Venue(_) => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bend_scale_follows_the_frame() {
        use FieldCameraFrame as F;
        let top = F::WorldMapTopView {
            azimuth: 0,
            zoom: 0,
            pan: [0, 0],
        };
        assert_eq!(frame_curve_scale(true, &top), 0.0);
        assert_eq!(frame_curve_scale(false, &F::HostDebugOrbit), 0.0);
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
}
