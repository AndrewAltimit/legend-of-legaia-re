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

/// Field follow-camera pitch (`_DAT_8007B790`), PSX 12-bit units - the town01
/// anchor savestate's value (~39.6 deg down-tilt).
///
/// **A fallback, not the field camera.** Retail derives the pitch per scene
/// and per tile: the camera-region record the tile query hits is split into
/// the camera parameter block (`FUN_801DBC20`, [`crate::camera_zone`]), the
/// composer `FUN_801DAB90` turns the block plus the player's position into a
/// target and the per-frame ease walks `_DAT_8007B790` toward it. That path
/// is [`crate::camera::Camera::zone`], live whenever the world carries field
/// terrain, and [`field_follow_view`] reads its output. This constant is what
/// a world with **no** terrain loaded (a unit world, a bare test scene)
/// frames with - the anchor state's own reading, which the zone camera
/// reproduces there from `town01`'s record.
pub const FIELD_PITCH_UNITS: f32 = 450.0;

/// Field follow-camera base yaw (`_DAT_8007B792`), PSX 12-bit units, from the
/// same anchor. The movement compass reads its negation (`alpha = -psi` for
/// the PSX GTE camera), which is what [`Camera::render_yaw_bias`] declares -
/// and, while the zone camera drives the frame, what
/// [`Camera::compass_azimuth_units`] recomputes from the live yaw.
///
/// The same fallback rule as [`FIELD_PITCH_UNITS`]: retail's field yaw is a
/// per-frame *output* of the composer (a position-proportional sweep across
/// the walk-region box in modes 1 / 2, a bearing in modes 3 / 4), and the
/// zone camera produces it; this value frames only a terrain-less world.
pub const FIELD_FOLLOW_YAW_UNITS: f32 = -160.0;

/// Field GTE `H` (`_DAT_8007B6F4`) fallback. `512` in the field, `256` in
/// battle - written per phase, unlike `OFX` / `OFY`.
///
/// [`field_follow_view`] prefers the live
/// [`crate::camera::RetailCamGlobals::h`] whenever the camera carries one -
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
/// `tr_eye / S` - the same reduction [`cutscene_view`] applies to an
/// op-`0x45` beat's offset trio.
///
/// While the zone camera is live, [`field_follow_view`] feeds the whole
/// composed trio instead of this constant.
pub const FIELD_CAM_DEPTH: f32 = RETAIL_FIELD_DEPTH_UNITS / CUTSCENE_WORLD_SCALE;

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
/// While the zone camera is live ([`Camera::zone`] has composed from this
/// scene's terrain), pitch, yaw, `H` and the eye-back depth are the live
/// globals it eases - retail's per-scene / per-tile camera. Otherwise (a
/// world with no field terrain) pitch and yaw are the savestate-pinned
/// fallbacks above and `H` the live global or [`FIELD_H`]. The look-at
/// target is the player anchor with its floor height sampled (retail's
/// follow-cam `FUN_801DBE9C` folds `-(anchor X/Z)` into the focus globals each
/// frame, and the port's `sample_field_floor_height` supplies the Y a raw
/// `world_y` of `0` would put under an elevated town tier). Four user knobs
/// compose onto the base and are all retail-identical at their defaults:
/// [`Camera::distance`] scales the eye-back depth (the coarse preset),
/// [`Camera::manual_orbit`] swings the yaw around the player in the compass
/// sense - the PSX render yaw is its negation - and [`Camera::manual_zoom`]
/// (the continuous wheel) and [`Camera::manual_tilt`] (clamped through
/// [`follow_knobs::composed_pitch`](crate::camera::follow_knobs::composed_pitch))
/// dolly and pitch the pose **about the character's body**
/// ([`FOLLOW_PIVOT_LIFT`]), so the character holds its screen point through
/// both. None of them reaches a cutscene shot -
/// [`resolve_field_camera`] hands a running timeline the
/// [`FieldCameraFrame::Cutscene`] arm, which reads none of the four.
///
/// The eye trio is the live `_DAT_800840B8/BC/C0` the ease walks, divided by
/// the 6x world scale retail folds into its camera rotation - eye X
/// (`-(depth >> 7)`), eye Y (`0x200 + depth >> 8` plus the floor-height
/// compensation) and the depth, all of them. See [`FIELD_CAM_DEPTH`] for the
/// `FUN_800172C0` chain that pins the composition.
///
/// `None` when no player actor exists to follow; a host falls back to its own
/// debug vantage there.
///
/// REF: FUN_801DBE9C, FUN_801DAB90
pub fn field_follow_view(cam: &Camera, world: &World) -> Option<FieldCameraView> {
    follow_view_with_knobs(
        cam,
        world,
        cam.manual_orbit,
        cam.manual_tilt,
        cam.manual_zoom,
    )
}

/// [`field_follow_view`] with the three user follow knobs passed explicitly
/// rather than read off the camera. The overworld walk arm passes identity:
/// the knobs are field-only ([`Camera::follow_knobs_live`]), so a tilt or
/// zoom banked in a town must not frame the kingdom map, where the page can
/// neither see nor undo it. It used to - a town tilt tipped the walk camera
/// toward top-down or under the terrain, and the curvature bend then
/// indexed off that eye.
fn follow_view_with_knobs(
    cam: &Camera,
    world: &World,
    manual_orbit: f32,
    manual_tilt: f32,
    manual_zoom: f32,
) -> Option<FieldCameraView> {
    let (wx, wz) = lead_actor_xz(world)?;
    let floor_y = world.sample_field_floor_height(wx as i32, wz as i32) as f32;
    let s = CUTSCENE_WORLD_SCALE;
    // The coarse distance preset scales the eye-back depth alone (its
    // historical shape, which a `Far` capture baseline pins); the continuous
    // wheel zoom and the tilt re-pivot the pose about the character below.
    let depth_scale = cam.distance.scale();
    let (pitch_units, yaw_units, tr_eye) = if cam.zone.active {
        let g = &cam.globals.0;
        (
            f32::from(g[0] as i16),
            f32::from(g[1] as i16),
            // The live eye-space translation trio, reduced by the 6x world
            // scale retail folds into its rotation. Mode 4 flips a negative
            // depth into the yaw, so the depth axis takes the magnitude and
            // a degenerate shot never puts the lens inside the player; the
            // floor is `FIELD_CAM_DEPTH / 8`, which is closer than any
            // composed shot and still in front of the mesh.
            [
                g[3] as f32 / s,
                g[4] as f32 / s,
                ((g[5] as f32 / s).abs()).max(FIELD_CAM_DEPTH / 8.0) * depth_scale,
            ],
        )
    } else {
        (
            FIELD_PITCH_UNITS,
            FIELD_FOLLOW_YAW_UNITS,
            [0.0, 0.0, FIELD_CAM_DEPTH * depth_scale],
        )
    };
    let retail = FieldCameraView {
        // Retail's focus trio is `_DAT_80089118/1C/20`, and only X and Z are
        // ever written in the field (`FUN_801DBE9C`'s retail leg and the
        // focus clamp `FUN_801DAA50` both write those two). Its Y global
        // measures `0` on every sampled field frame while the player's
        // footing on those frames is not, so **retail's** focus sits at
        // world Y `0` and the vertical framing rides the composed eye Y
        // alone - see docs/subsystems/renderer.md.
        //
        // Zeroing it moves the focus 128 units vertically in `town01`, which
        // is enough to show any tilt in the walk direction: the page's
        // compass oracle measures a *finite* displacement through the frame's
        // own projection, so a heading that is off by up to 45 degrees prints
        // a cross-term that a floor-anchored focus had been flattening. The
        // heading is now rung at 45 degrees like retail's
        // (`World::decode_field_direction` -> `remap_pad_direction`), which
        // halves that worst case, so the measured framing ships.
        //
        // A world with no terrain has no composed eye trio to ride, so the
        // sampled floor still stands in for it there.
        //
        // With the zone camera live the focus is the globals themselves, not
        // the lead actor: `FUN_800172C0` MVMVAs `-(_DAT_80089118/1C/20)` and
        // nothing else. They equal the player in plain free roam (the
        // follow writeback pins them there after the world tick), but three
        // legs put them elsewhere - the edge clamp `FUN_801DAA50` at a
        // room's rim, a mode-5 fixed shot's anchor, and an op-`0x45` beat's
        // focus slots from a record that is **not** the modal timeline (a
        // spawned helper such as `jouine`'s evolved-Cort arrival stages every
        // shot through them). Framing on the actor there aims the lens at a
        // point the retail frame is not looking at. The overworld keeps the
        // actor: its walk arm scales the world about this point.
        focus: if cam.zone.active && world.mode != SceneMode::WorldMap {
            cam.globals.focus_world().map(|v| v as f32)
        } else {
            [wx, if cam.zone.active { 0.0 } else { floor_y }, wz]
        },
        pitch: to_rad(pitch_units),
        // PSX camera yaw is the compass negation, so a positive manual orbit
        // subtracts from the render yaw.
        yaw: to_rad(yaw_units) - manual_orbit,
        // The field follow camera never rolls: `FUN_80025C24` seeds the roll
        // global to `0` on scene entry and only an op-`0x45` beat writes it,
        // and the follow ease's descriptor list has no roll entry. Measured
        // `0` in 51 of 51 field states.
        roll: 0.0,
        // The live GTE `H` when the camera carries one - retail's scene-entry
        // reset leaves it `0`, so a scene that never staged an `H` falls back
        // to the field default. Reading the pin unconditionally dropped an
        // op-`0x45` slot-`9` beat on the floor.
        h: match cam.globals.h() {
            0 => FIELD_H,
            v => v as f32,
        },
        tr_eye,
    };
    if manual_tilt == 0.0 && manual_zoom == 1.0 {
        // Both knobs at identity: the retail pose, bit for bit.
        return Some(retail);
    }
    // Tilt and zoom pivot about the CHARACTER, not the retail focus. The
    // focus is retail's look target - a floor-level point that sits a tier
    // below the feet on an elevated town tier (`town01`) - so a dolly or a
    // pitch about it walks the character out of frame. Instead the pose is
    // re-expressed about the body's centre `q` (feet lifted by half a
    // character height): the rotation takes the user's tilt, the focus
    // becomes `q`, and `q`'s eye-space position - which alone fixes its
    // screen point - is kept and scaled by the zoom. At identity this
    // re-expression is the retail pose (`R (v - q) + (R (q - focus) + tr)`
    // = `R (v - focus) + tr`), which the early return above keeps exact.
    let pivot = [wx, floor_y - FOLLOW_PIVOT_LIFT, wz];
    let q = retail.eye_space(pivot);
    Some(FieldCameraView {
        focus: pivot,
        // The scene's pitch plus the user's tilt, clamped so the lens stays
        // above the floor and short of top-down.
        pitch: crate::camera::follow_knobs::composed_pitch(retail.pitch, manual_tilt),
        tr_eye: q.map(|c| c * manual_zoom),
        ..retail
    })
}

/// How far above the feet (raw retail Y-down, so subtracted) the follow
/// knobs' pivot sits: half a ~130-unit field character, so a tilt or a zoom
/// turns and dollies about the body's centre and the character holds its
/// screen point through both.
pub const FOLLOW_PIVOT_LIFT: f32 = 64.0;

/// Whether a scripted shot owns this frame's camera: a cutscene timeline is
/// running **and** some op-`0x45` beat has staged a camera slot.
///
/// Retail keeps one set of camera globals. A record that issues no camera
/// beat leaves them where the field camera put them, so its frame is the
/// field camera's - `garmel`'s Songi taunt (`P2[62]`, three BGM ops and
/// the text) is captured on the follow framing, pitch / yaw / `H` / eye trio
/// unchanged from the walk. A host that switched to [`cutscene_view`]'s
/// per-slot fallbacks with nothing staged framed an invented shot. Both
/// hosts gate on this one kernel; the world map has always been gated this
/// way (its Drake mist-wall force-walk bands carry no camera beats).
pub fn cutscene_owns_camera(world: &World) -> bool {
    world.cutscene_timeline_active() && !world.camera.state.params.is_empty()
}

/// The **op-`0x45` cutscene shot**'s inputs, decoded from the camera state the
/// field VM staged.
///
/// Slot map (`FUN_801DE084` writes the globals, `FUN_800172C0` builds the
/// rotation through `FUN_80026988`): `0` pitch, `1` yaw, `2` roll, `3/4/5` the eye-space translation
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

/// The **terrain-less fallback** overworld walk view, for a controller zoom.
///
/// A world-map scene with its field terrain loaded frames through the zone
/// camera instead ([`resolve_field_camera`]'s world-map arm), which is what
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

/// Re-express a zone-camera [`field_follow_view`] pose in the overworld walk
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
    /// A minigame venue's own staged camera - the dance floor's, from the
    /// overlay entry's record ([`crate::dance_venue::venue_camera`]). A fixed
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
            FieldCameraFrame::WorldMapWalk { view, player } => Some(FieldCameraView {
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

/// Retail's per-primitive near reject for this frame, as `(sz_per_w,
/// ot_shift)` for the renderer - or `None` when the pass does not run it.
///
/// Every TMD prim handler behind `FUN_80043390` drops a primitive whose
/// `OTZ` (mean corner `SZ >> ot_shift`) is below the scratch floor
/// `0x1F80037E = 0x10` (`sub s1,s2,t4` / `bltz s1` at `0x80043870` in
/// `FUN_80043768`); see `legaia_engine_ui::prim_near_reject`. The field and
/// battle cameras both put the GTE eye depth in clip `w`, so `sz_per_w` is
/// `1`. The kingdom overworld composes a world scale onto its camera and
/// draws through the overlay's own replacement handlers, so it stays off, as
/// do the minigame venues; `retail_camera = false` (the debug orbit) frames
/// from a vantage retail never had and draws everything.
///
/// Shared so the two hosts cannot arm it on different frames.
///
/// REF: FUN_80043390, FUN_80043768
/// The projection `H` the GPU polygon-size limit is tested under this frame
/// (`legaia_engine_ui::prim_near_reject::gpu_span_rejected`), or `None` when
/// the pass does not apply it. Battle only: every catalogued battle state's
/// camera reads `H = 256`, and the battle stage puts a combatant between the
/// eye and the caster on the summon close-ups (`theeder_summon_mid_cast`'s
/// monster, `0xA1`), whose legs reach past the eye; retail's GPU drops those
/// primitives. The field and the dance hall keep their own rules.
pub fn prim_gpu_span_h(mode: SceneMode, retail_camera: bool) -> Option<f32> {
    (retail_camera && mode == SceneMode::Battle).then_some(BATTLE_GTE_H)
}

/// The battle camera's projection distance `H`.
pub const BATTLE_GTE_H: f32 = 256.0;

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
    // The dance floor frames through the pose its overlay entry staged, not
    // the follow camera over the walked-in scene's player.
    if world.mode == SceneMode::Dance
        && let Some(stage) = world.minigames.dance_venue.as_ref()
    {
        return FieldCameraFrame::Venue(stage.camera);
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
        // The retail walk camera is the field zone camera (see
        // `camera::zone_camera_scene`): the pose the kingdom MAN's
        // section-3 records compose for the player's tile, eased per region.
        // The user's follow knobs stay out of it (see
        // `follow_view_with_knobs`).
        if cam.zone.active
            && let Some(v) = follow_view_with_knobs(cam, world, 0.0, 0.0, 1.0)
        {
            return FieldCameraFrame::WorldMapWalk {
                view: world_map_view_from_follow(&v, az),
                player: v.focus,
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
        FieldCameraFrame::Cutscene(v)
        | FieldCameraFrame::Follow(v)
        | FieldCameraFrame::Venue(v) => Some(v.vp(aspect)),
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
        FieldCameraFrame::Cutscene(v)
        | FieldCameraFrame::Follow(v)
        | FieldCameraFrame::Venue(v) => Some(v.eye()),
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

    /// While the zone camera drives the frame the whole composed eye trio
    /// reaches the view, divided by the base matrix's 6x world scale -
    /// `FUN_800172C0` uploads that trio as GTE `TR` with no depth constant
    /// anywhere in the chain.
    #[test]
    fn follow_view_feeds_the_whole_composed_trio_reduced_by_the_world_scale() {
        let w = world_with_player(0, 0);
        let mut cam = Camera::default();
        cam.zone.active = true;
        cam.globals.0[0] = 0x1C0;
        cam.globals.0[1] = -0x50;
        cam.globals.0[3] = -84;
        cam.globals.0[4] = 553;
        cam.globals.0[5] = 10712;
        let v = field_follow_view(&cam, &w).expect("player actor");
        let s = CUTSCENE_WORLD_SCALE;
        assert!((v.tr_eye[0] - -84.0 / s).abs() < 1e-3);
        assert!((v.tr_eye[1] - 553.0 / s).abs() < 1e-3);
        assert!((v.tr_eye[2] - 10712.0 / s).abs() < 1e-3);
        assert!((v.pitch - to_rad(448.0)).abs() < 1e-6);
        // A mode-4 shot stores a negative depth and folds the side into the
        // yaw, so the depth axis takes the magnitude.
        cam.globals.0[5] = -10712;
        let f = field_follow_view(&cam, &w).unwrap();
        assert!((f.tr_eye[2] - 10712.0 / s).abs() < 1e-3);
        // A degenerate shot never puts the lens inside the player.
        cam.globals.0[5] = 0;
        let z = field_follow_view(&cam, &w).unwrap();
        assert!((z.tr_eye[2] - FIELD_CAM_DEPTH / 8.0).abs() < 1e-3);
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

    /// Tilt and zoom pivot about the character's body: its eye-space
    /// position (which alone fixes its screen point) is unchanged by a tilt
    /// and scaled by the zoom, so the character holds its place on screen
    /// while the scene around it turns and gains or loses perspective.
    #[test]
    fn follow_view_tilts_and_zooms_about_the_character() {
        let w = world_with_player(300, -200);
        let mut cam = Camera::default();
        cam.distance = crate::camera::CameraDistance::Far;
        cam.zone.active = true;
        cam.globals.0[0] = FIELD_PITCH_UNITS as i32;
        cam.globals.0[1] = FIELD_FOLLOW_YAW_UNITS as i32;
        cam.globals.0[3] = -600;
        cam.globals.0[4] = 1200;
        cam.globals.0[5] = 4800;
        let base = field_follow_view(&cam, &w).unwrap();
        let pivot = [300.0, -FOLLOW_PIVOT_LIFT, -200.0];
        let q0 = base.eye_space(pivot);
        let screen = |v: &FieldCameraView| {
            let q = v.eye_space(pivot);
            [q[0] / q[2], q[1] / q[2]]
        };

        cam.manual_tilt = 0.2;
        let t = field_follow_view(&cam, &w).unwrap();
        assert!((t.pitch - (base.pitch + 0.2)).abs() < 1e-6);
        assert_eq!(t.yaw, base.yaw);
        assert_eq!(t.h, base.h);
        let (s0, s1) = (screen(&base), screen(&t));
        assert!(
            (s0[0] - s1[0]).abs() < 1e-4 && (s0[1] - s1[1]).abs() < 1e-4,
            "tilt: {s0:?} vs {s1:?}"
        );
        // The eye moved: steeper means higher above the pivot.
        assert!(t.eye()[1] < base.eye()[1], "a downward tilt lifts the eye");

        cam.manual_tilt = 0.0;
        cam.manual_zoom = 0.5;
        let z = field_follow_view(&cam, &w).unwrap();
        assert_eq!(z.pitch, base.pitch);
        let q1 = z.eye_space(pivot);
        for k in 0..3 {
            assert!(
                (q1[k] - q0[k] * 0.5).abs() < 1e-3,
                "axis {k}: {q1:?} vs {q0:?}"
            );
        }
        let s2 = screen(&z);
        assert!(
            (s0[0] - s2[0]).abs() < 1e-4 && (s0[1] - s2[1]).abs() < 1e-4,
            "zoom: {s0:?} vs {s2:?}"
        );
        let d = |e: [f32; 3]| {
            ((e[0] - pivot[0]).powi(2) + (e[1] - pivot[1]).powi(2) + (e[2] - pivot[2]).powi(2))
                .sqrt()
        };
        assert!(
            (d(z.eye()) - d(base.eye()) * 0.5).abs() < 1e-2,
            "zoom halves the eye distance"
        );
    }

    /// Both knobs at identity return the retail pose bit for bit - no
    /// re-expression about the pivot leaks a rounding into the faithful
    /// frame.
    #[test]
    fn follow_view_is_bit_identical_at_identity_knobs() {
        let w = world_with_player(300, -200);
        let mut cam = Camera::default();
        cam.manual_orbit = 0.3;
        cam.distance = crate::camera::CameraDistance::Far;
        let a = field_follow_view(&cam, &w).unwrap();
        cam.manual_tilt = 0.0;
        cam.manual_zoom = 1.0;
        let b = field_follow_view(&cam, &w).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.focus[1], w.sample_field_floor_height(300, -200) as f32);
    }

    /// The tilt clamps the composed pitch at both ends of the knob range.
    #[test]
    fn follow_view_tilt_clamps_to_the_knob_range() {
        use crate::camera::follow_knobs::{PITCH_MAX, PITCH_MIN};
        let w = world_with_player(0, 0);
        let mut cam = Camera::default();
        cam.manual_tilt = 5.0;
        assert_eq!(field_follow_view(&cam, &w).unwrap().pitch, PITCH_MAX);
        cam.manual_tilt = -5.0;
        assert_eq!(field_follow_view(&cam, &w).unwrap().pitch, PITCH_MIN);
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

    /// A running record owns the camera only once a beat staged a slot: a
    /// taunt with no op-`0x45` (garmel's Songi `P2[62]`) keeps the field
    /// camera on both hosts.
    #[test]
    fn a_timeline_owns_the_camera_only_after_a_camera_beat() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = world_with_player(700, 800);
        assert!(!cutscene_owns_camera(&w), "no timeline");
        w.cutscene.timeline = Some(crate::cutscene_timeline::CutsceneTimeline::new(
            vec![0x4A, 0xFF, 0x7F],
            0,
        ));
        assert!(w.cutscene_timeline_active());
        assert!(!cutscene_owns_camera(&w), "timeline, nothing staged");
        w.camera.state.params = vec![CameraParam {
            slot: 0,
            value: 300,
        }];
        assert!(cutscene_owns_camera(&w), "timeline + a staged slot");
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

    /// On the overworld the zone camera owns the walk frame: the resolver
    /// hands back the zone pose's retail words (eye trio in GTE units, focus
    /// on the player), and the field-frame view the screen effects project
    /// through lands every world point on the same screen pixel.
    #[test]
    fn overworld_walk_frame_is_the_zone_pose() {
        let mut w = world_with_player(8266, 8700);
        w.mode = SceneMode::WorldMap;
        let mut cam = Camera::default();
        cam.zone.active = true;
        // The `keikoku_chest_preload` live words: pitch 370, eye
        // (-69, 776, 8875), H 368.
        cam.globals.0[0] = 370;
        cam.globals.0[1] = 0;
        cam.globals.0[3] = -69;
        cam.globals.0[4] = 776;
        cam.globals.0[5] = 8875;
        cam.globals.0[9] = 368;
        let frame = resolve_field_camera(&w, &cam, None, [0.0, 0.0]);
        let FieldCameraFrame::WorldMapWalk { view, player } = frame else {
            panic!("expected the walk frame, got {frame:?}");
        };
        assert_eq!(player, [8266.0, 0.0, 8700.0]);
        assert_eq!(view.h, 368.0);
        assert!((view.pitch - to_rad(370.0)).abs() < 1e-6);
        for (got, want) in view.tr_eye.iter().zip([-69.0f32, 776.0, 8875.0]) {
            assert!((got - want).abs() < 1e-2, "{got} vs {want}");
        }
        // Not the pinned fallback.
        assert_ne!(view.tr_eye, world_map_walk_view(0, 0).tr_eye);

        let fv = frame.field_view().expect("field-frame view");
        let a = frame_vp(&frame, ([0.0; 3], [1.0; 3]), 4.0 / 3.0).unwrap();
        let b = fv.vp(4.0 / 3.0);
        let project = |m: &[f32; 16], p: [f32; 3]| {
            let v = [p[0], p[1], p[2], 1.0];
            let mut c = [0.0f32; 4];
            for (r, o) in c.iter_mut().enumerate() {
                *o = m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * v[3];
            }
            [c[0] / c[3], c[1] / c[3]]
        };
        for p in [
            [8266.0, 0.0, 8700.0],
            [8400.0, 150.0, 9000.0],
            [8000.0, -300.0, 9500.0],
        ] {
            let (pa, pb) = (project(&a, p), project(&b, p));
            assert!(
                (pa[0] - pb[0]).abs() < 1e-4 && (pa[1] - pb[1]).abs() < 1e-4,
                "{p:?}: {pa:?} vs {pb:?}"
            );
        }

        // Without a zone pose the terrain-less fallback still frames it.
        cam.zone.active = false;
        let FieldCameraFrame::WorldMapWalk { view, .. } =
            resolve_field_camera(&w, &cam, None, [0.0, 0.0])
        else {
            panic!("expected the walk frame");
        };
        assert_eq!(view, world_map_walk_view(0, 0));
    }

    /// The follow knobs are field-only, so a tilt / zoom / orbit banked in a
    /// town must not reach the overworld walk frame: the walk pose with every
    /// knob set equals the walk pose with none, and the d-pad compass agrees.
    #[test]
    fn overworld_walk_frame_ignores_banked_follow_knobs() {
        let mut w = world_with_player(8266, 8700);
        w.mode = SceneMode::WorldMap;
        let mut cam = Camera::default();
        cam.zone.active = true;
        cam.globals.0[0] = 370;
        cam.globals.0[3] = -69;
        cam.globals.0[4] = 776;
        cam.globals.0[5] = 8875;
        cam.globals.0[9] = 368;
        let clean = resolve_field_camera(&w, &cam, None, [0.0, 0.0]);
        let clean_az = cam.compass_azimuth_units_for(&w);
        cam.manual_orbit = 0.7;
        cam.manual_tilt = 0.5;
        cam.manual_zoom = 1.8;
        assert_eq!(resolve_field_camera(&w, &cam, None, [0.0, 0.0]), clean);
        assert_eq!(cam.compass_azimuth_units_for(&w), clean_az);
        // Back in the field the same knobs still steer.
        w.mode = SceneMode::Field;
        assert_ne!(
            resolve_field_camera(&w, &cam, None, [0.0, 0.0]).field_view(),
            clean.field_view()
        );
    }
}
