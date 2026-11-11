//! The **landed fish's body**: the models the catch shows, posed for the
//! frame the session and the venue camera are on.
//!
//! Retail draws the catch twice over (`docs/subsystems/minigame-fishing.md`,
//! "The landed fish"):
//!
//! - through the close-up, the hooked-fish actor (`FUN_801D4948` sub-state
//!   `4`) clears its skip bit and the render dispatcher draws its body at the
//!   lifted world point, flung up out of the water under the orbiting camera;
//! - on the result plate, the result actor (`FUN_801D5298`) draws the same
//!   body itself (`FUN_801D67BC`), locked to the camera, hanging from a line
//!   in the middle of the frame, and rising into place from below.
//!
//! The pose and place are the `engine-fishing` kernel's
//! ([`legaia_engine_fishing::fishing_actors::catch_fish`]); this module holds
//! the venue's ten bodies (scene models `0x1C..=0x25` with their swim clips)
//! and turns a [`CatchFish`] into raw world vertices every host draws under
//! the venue camera - a camera-locked body is placed through the inverse of
//! the frame's view, so it lands where retail's lock puts it.

//! REF: FUN_801d4948, FUN_801d5298

use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_fishing::fishing_actors::{CatchFish, CatchFishPlace};
use legaia_minigame_formats::fishing_species::{CatchPose, SPECIES_COUNT};
use legaia_tmd::mesh::VramMesh;

/// Each species' `(first vertex, count)` in a combined catch mesh, `None`
/// for a body that did not resolve.
pub type CatchSpans = Vec<Option<(usize, usize)>>;

/// One species' body.
#[derive(Debug, Clone)]
pub struct CatchBody {
    /// The model in rest pose, per-vertex object ids alongside.
    pub mesh: VramMesh,
    /// Per-vertex `[r, g, b, textured]` packet colour stream.
    pub flat: Vec<u8>,
    /// Per-vertex object (bone) index.
    pub oids: Vec<u32>,
    /// The swim clip, `frames[frame][bone] = [tx, ty, tz, rx, ry, rz]`.
    pub frames: Vec<Vec<[i32; 6]>>,
    /// Object 0's vertex list, model space - the line ties to one of them
    /// ([`CatchPose::tie_vertex`]).
    pub obj0: Vec<[f32; 3]>,
}

/// The venue's ten catch bodies.
#[derive(Debug, Clone, Default)]
pub struct CatchBodies {
    /// Per species; `None` where the model does not resolve.
    pub bodies: Vec<Option<CatchBody>>,
}

impl CatchBodies {
    /// Lift the bodies out of the loaded venue scene. A body whose clip rig
    /// does not match plays no clip (its rest pose).
    pub fn load(scene: &legaia_engine_scene::scene::Scene, anm: Option<&PlayerAnmBundle>) -> Self {
        let bank = legaia_engine_scene::model_bank::SceneModelBank::build(scene);
        let bodies = (0..SPECIES_COUNT)
            .map(|s| {
                let model = legaia_engine_fishing::fishing_actors::CATCH_FISH_MODEL_BASE + s as i16;
                let bytes = bank.tmd_bytes(scene, model)?;
                let tmd = legaia_tmd::parse(&bytes).ok()?;
                let (mesh, oids, shading) =
                    legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
                let flat = legaia_engine_field::packet_color::hybrid(&mesh, &shading);
                let frames = anm
                    .and_then(|a| crate::fishing_scene::clip_frames(a, s, tmd.objects.len()))
                    .unwrap_or_default();
                let obj0 = tmd
                    .objects
                    .first()
                    .map(|o| {
                        o.vertices
                            .iter()
                            .map(|v| [f32::from(v.x), f32::from(v.y), f32::from(v.z)])
                            .collect()
                    })
                    .unwrap_or_default();
                Some(CatchBody {
                    mesh,
                    flat,
                    oids,
                    frames,
                    obj0,
                })
            })
            .collect();
        Self { bodies }
    }

    /// The body for `species`.
    pub fn body(&self, species: usize) -> Option<&CatchBody> {
        self.bodies.get(species)?.as_ref()
    }
}

/// An eye-space point (world units) back to raw world under `view`: the
/// inverse of `R (p - focus) + tr`.
pub fn eye_to_world(view: &FieldCameraView, e: [f32; 3]) -> [f32; 3] {
    let d = [
        e[0] - view.tr_eye[0],
        e[1] - view.tr_eye[1],
        e[2] - view.tr_eye[2],
    ];
    let r = legaia_engine_camera::psx_camera::camera_rotation(view.pitch, view.yaw, view.roll);
    // `R^T d`; column-major `r[c*4 + row]` stores `R[row][c]`.
    let mut out = view.focus;
    for (i, o) in out.iter_mut().enumerate() {
        for (j, &dj) in d.iter().enumerate() {
            *o += r[i * 4 + j] * dj;
        }
    }
    out
}

/// Place one model-space point of `fish` (already posed on its clip) into
/// raw world under `view`.
pub fn place_point(fish: &CatchFish, view: &FieldCameraView, v: [f32; 3]) -> [f32; 3] {
    let m = fish.linear();
    let o = fish.origin();
    let p: [f32; 3] =
        std::array::from_fn(|r| o[r] + m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2]);
    match fish.place {
        CatchFishPlace::World(_) => p,
        CatchFishPlace::CameraLocked { .. } => eye_to_world(view, p),
    }
}

/// `body`'s vertices for `fish` at clip frame `frame`, raw world under
/// `view`, written into `out` (one per mesh vertex).
pub fn pose_body(
    out: &mut [[f32; 3]],
    body: &CatchBody,
    fish: &CatchFish,
    frame: usize,
    view: &FieldCameraView,
) {
    let keys = (!body.frames.is_empty()).then(|| &body.frames[frame % body.frames.len()]);
    crate::fishing_scene::pose_into(out, &body.mesh.positions, &body.oids, keys, [0.0; 3], 0);
    for p in out.iter_mut() {
        *p = place_point(fish, view, *p);
    }
}

/// The tie vertex `pose` names, posed on `frame` (object 0's bone), raw
/// world under `view`.
pub fn tie_point(
    body: &CatchBody,
    pose: &CatchPose,
    fish: &CatchFish,
    frame: usize,
    view: &FieldCameraView,
) -> Option<[f32; 3]> {
    let tie = *body.obj0.get(pose.tie_vertex as usize)?;
    let keys = (!body.frames.is_empty()).then(|| &body.frames[frame % body.frames.len()]);
    let mut out = [[0.0f32; 3]];
    crate::fishing_scene::pose_into(&mut out, &[tie], &[0], keys, [0.0; 3], 0);
    Some(place_point(fish, view, out[0]))
}

/// The plate's line this frame on the 320x240 retail screen: from the top of
/// the frame straight down to the projected tie vertex (`DAT_801D9154`), as
/// `(top, tie)`; `None` unless the fish hangs on the plate.
///
/// PORT: FUN_801d67bc (the tie vertex's projection into `DAT_801D9154`)
pub fn plate_line(
    body: &CatchBody,
    pose: &CatchPose,
    fish: &CatchFish,
    frame: usize,
    view: &FieldCameraView,
) -> Option<((i16, i16), (i16, i16))> {
    if !matches!(fish.place, CatchFishPlace::CameraLocked { .. }) {
        return None;
    }
    let p = tie_point(body, pose, fish, frame, view)?;
    let e = view.eye_space(p);
    if e[2] <= 1.0 {
        return None;
    }
    let sx = (160.0 + view.h * e[0] / e[2]).round() as i16;
    let sy = (legaia_engine_camera::projection::GTE_OFY + view.h * e[1] / e[2]).round() as i16;
    Some(((sx, 0), (sx, sy)))
}

impl CatchBodies {
    /// The ten bodies as one mesh, in species order, with each body's
    /// `(first vertex, count)` - `None` for a body that did not resolve.
    /// Hosts append it to their pond buffer and move it through
    /// [`Self::pose_span`] every frame.
    pub fn combined(&self) -> (VramMesh, Vec<u8>, CatchSpans) {
        let mut m = VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        };
        let mut flat = Vec::new();
        let spans = self
            .bodies
            .iter()
            .map(|b| {
                b.as_ref().map(|b| {
                    let base = m.positions.len();
                    m.positions.extend_from_slice(&b.mesh.positions);
                    m.uvs.extend_from_slice(&b.mesh.uvs);
                    m.cba_tsb.extend_from_slice(&b.mesh.cba_tsb);
                    m.normals.extend_from_slice(&b.mesh.normals);
                    m.colors.extend_from_slice(&b.mesh.colors);
                    m.indices
                        .extend(b.mesh.indices.iter().map(|i| i + base as u32));
                    flat.extend_from_slice(&b.flat);
                    (base, b.mesh.positions.len())
                })
            })
            .collect();
        (m, flat, spans)
    }

    /// Pose this frame's bodies into `out` (laid out as [`Self::combined`]
    /// lays them, `spans` offsets into `out`): the landed species' body for
    /// `fish` at clip frame `frame`, every other body collapsed onto one
    /// point (the camera's focus), so it rasterises nothing. Returns the plate's line
    /// ([`plate_line`]) when the fish hangs on the plate.
    pub fn pose_spans(
        &self,
        out: &mut [[f32; 3]],
        spans: &[Option<(usize, usize)>],
        fish: Option<&CatchFish>,
        poses: &[CatchPose],
        frame: usize,
        view: &FieldCameraView,
    ) -> Option<((i16, i16), (i16, i16))> {
        let mut line = None;
        for (s, span) in spans.iter().enumerate() {
            let (Some((base, n)), Some(body)) = (*span, self.body(s)) else {
                continue;
            };
            let Some(dst) = out.get_mut(base..base + n) else {
                continue;
            };
            match fish.filter(|f| f.species == s) {
                Some(f) => {
                    pose_body(dst, body, f, frame, view);
                    line = poses
                        .get(s)
                        .and_then(|p| plate_line(body, p, f, frame, view));
                }
                None => dst.fill(view.focus),
            }
        }
        line
    }
}
