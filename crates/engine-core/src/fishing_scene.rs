//! The fishing minigame's **3D venue surface**: the pond scene (`other1`,
//! the bundle the fishing overlay belongs to) with the party seated on the
//! shore where the overlay's setup spawns them, framed by the venue camera -
//! the one kernel both play hosts draw while the world is in
//! [`crate::world::SceneMode::Fishing`], the way
//! [`crate::muscle_dome_scene::MuscleDomeSurface`] serves the dome.
//!
//! Retail's fishing is a scene of its own: the mode-24 door warp
//! (`FUN_80025980`) swaps the field for the overlay, whose setup state
//! (`FUN_801CF3BC`) seats three party actors on the pond's anchor tile and
//! installs the venue camera; the return warp brings the backed-up field
//! scene back. The port keeps the departure field loaded underneath (its
//! render state is never touched, so leaving shows it exactly as it was, the
//! player where he stood) and draws this surface in its place:
//!
//! - the **pond** is the `other1` scene's environment pack instanced by its
//!   `.MAP` placements and terrain layers plus the walk ground, through the
//!   shared [`crate::field_env`] resolution;
//! - the **party** is three field bodies (global character pack slots
//!   `0..=2`) at [`crate::fishing_venue::party_placements`], the lead on his
//!   party-bank idle, the others on their venue-bank clips; the lead's
//!   facing is the aim the D-pad turns (the venue's lead actor);
//! - the **camera** is [`crate::fishing_venue::venue_camera_view`] over the
//!   lead.

use std::collections::HashMap;
use std::sync::Arc;

use legaia_asset::field_objects::FLAG_PLACED;
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_asset::{character_pack, field_char_textures};
use legaia_engine_vm::psx_camera::FieldCameraView;
use legaia_tmd::mesh::VramMesh;

use crate::fishing_venue::{FishingPartyMember, party_placements, venue_camera_view};
use crate::scene::{ProtIndex, Scene};
use crate::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};
use crate::world::MinigameState;

/// CDNAME label of the fishing venue bundle.
pub const FISHING_VENUE_SCENE: &str = "other1";

/// One seated body: its mesh, per-vertex object ids, packet colour stream
/// and the clip it plays (`frames[frame][bone] = [tx, ty, tz, rx, ry, rz]`).
struct Body {
    seat: FishingPartyMember,
    mesh: VramMesh,
    flat: Vec<u8>,
    oids: Vec<u32>,
    frames: Vec<Vec<[i32; 6]>>,
}

/// The decoded pond: the static map, the three bodies and the VRAM.
pub struct FishingSceneAssets {
    env: VramMesh,
    env_flat: Vec<u8>,
    bodies: Vec<Body>,
    vram: legaia_tim::Vram,
}

impl std::fmt::Debug for FishingSceneAssets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FishingSceneAssets")
            .field("env_vertices", &self.env.positions.len())
            .field("bodies", &self.bodies.len())
            .finish()
    }
}

fn empty_mesh() -> VramMesh {
    VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    }
}

fn append(dst: &mut VramMesh, dst_flat: &mut Vec<u8>, src: &VramMesh, flat: &[u8]) {
    let base = dst.positions.len() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.uvs.extend_from_slice(&src.uvs);
    dst.cba_tsb.extend_from_slice(&src.cba_tsb);
    dst.normals.extend_from_slice(&src.normals);
    dst.colors.extend_from_slice(&src.colors);
    dst.indices.extend(src.indices.iter().map(|i| i + base));
    dst_flat.extend_from_slice(flat);
}

/// Every frame of ANM record `rec` on a `bones`-wide rig, or `None` when the
/// record does not decode or its rig is another width.
fn clip_frames(anm: &PlayerAnmBundle, rec: usize, bones: usize) -> Option<Vec<Vec<[i32; 6]>>> {
    let r = anm.record_lenient(rec).ok()?;
    if r.bone_count as usize != bones || r.frame_count == 0 {
        return None;
    }
    Some(
        (0..r.frame_count as usize)
            .map(|f| {
                (0..bones)
                    .map(|b| {
                        anm.bone_transform(rec, f, b)
                            .map_or([0; 6], |t| [t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z])
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Frame-0 rigid transforms of scene-ANM record `anim_id - 1` (a bound
/// placement's rest pose), when its rig matches.
fn frame0_offsets(
    anm: &PlayerAnmBundle,
    anim_id: u8,
    objects: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let f = clip_frames(anm, (anim_id as usize).checked_sub(1)?, objects)?;
    Some(
        f.first()?
            .iter()
            .map(|k| {
                (
                    [k[0] as i16, k[1] as i16, k[2] as i16],
                    [k[3] as i16, k[4] as i16, k[5] as i16],
                )
            })
            .collect(),
    )
}

impl FishingSceneAssets {
    /// Decode the pond off `index`. `None` when the venue scene does not
    /// resolve on this image.
    pub fn load(index: &ProtIndex) -> Option<Self> {
        let scene = Scene::load(index, FISHING_VENUE_SCENE).ok()?;
        let (res, _) = SceneResources::build_targeted_with_options(
            &scene,
            &[],
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .ok()?;
        let anm = crate::npc_catalog::scene_anm_bundle(&scene);
        let (env, env_flat) = bake_env(index, &scene, &res, anm.as_ref());
        if env.indices.is_empty() {
            return None;
        }
        let mut vram = res.vram.clone();
        let pack_raw = index.entry_bytes(character_pack::PROT_ENTRY_INDEX).ok();
        if let Some(raw) = pack_raw.as_deref()
            && let Ok(t) = field_char_textures::parse(raw)
        {
            t.upload_to_vram(&mut vram, false);
        }
        let locomotion = pack_raw
            .as_deref()
            .and_then(|b| character_pack::field_locomotion_anm(b).ok());
        let pack = pack_raw
            .as_deref()
            .and_then(|b| character_pack::parse(b).ok());
        let mut bodies = Vec::new();
        for seat in party_placements(0) {
            let Some(cslot) = pack.as_ref().and_then(|p| p.slot(seat.model as usize)) else {
                continue;
            };
            let idle_rec = character_pack::locomotion_record_index(
                seat.model as usize,
                character_pack::LOCOMOTION_IDLE_SLOT,
            );
            let mut tmd_bytes = cslot.tmd_bytes.clone();
            if let Some(cap) = locomotion
                .as_ref()
                .and_then(|l| l.record_lenient(idle_rec).ok())
                .map(|r| r.bone_count as u32)
                && cslot.is_active_party()
                && tmd_bytes.len() >= 0x0C
            {
                tmd_bytes[0x08..0x0C].copy_from_slice(&cap.to_le_bytes());
            }
            let Ok(tmd) = legaia_tmd::parse(&tmd_bytes) else {
                continue;
            };
            let bones = tmd.objects.len();
            let (mesh, oids, shading) =
                legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &tmd_bytes);
            let flat = crate::packet_color::hybrid(&mesh, &shading);
            let frames = (!seat.party_bank)
                .then(|| {
                    anm.as_ref()
                        .and_then(|a| clip_frames(a, (seat.clip as usize).saturating_sub(1), bones))
                })
                .flatten()
                .or_else(|| {
                    locomotion
                        .as_ref()
                        .and_then(|l| clip_frames(l, idle_rec, bones))
                })
                .unwrap_or_default();
            bodies.push(Body {
                seat,
                mesh,
                flat,
                oids,
                frames,
            });
        }
        Some(Self {
            env,
            env_flat,
            bodies,
            vram,
        })
    }

    /// The pond VRAM: the scene upload with the field-character atlases.
    pub fn vram(&self) -> &legaia_tim::Vram {
        &self.vram
    }
}

/// Bake the venue's static map into one world-space hybrid mesh: placed
/// objects + terrain tiles (coplanar-ranked, as every host ranks a field)
/// and the walk ground, sunk under the authored floor art.
fn bake_env(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    anm: Option<&PlayerAnmBundle>,
) -> (VramMesh, Vec<u8>) {
    use crate::field_env;
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let binds = scene.field_object_binds(index).ok().flatten();
    let placement_records = scene
        .field_object_placements(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let terrain_records: Vec<_> = scene
        .field_terrain_tiles(index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & FLAG_PLACED == 0)
        .collect();
    let (placements, _) = field_env::resolve_placed_env_draws(
        &env_tmds,
        &placement_records,
        floor_lut,
        binds.as_ref(),
    );
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let mut ranked: Vec<field_env::EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    ranked.extend(terrain.iter().copied());
    ranked.extend(placements.iter().copied());
    let planes = crate::coplanar_draws::draw_plane_summaries(&ranked, res);
    let lifts = crate::coplanar_draws::coplanar_draw_offsets(&ranked, &planes);

    let mut out = empty_mesh();
    let mut flat_out = Vec::new();
    let mut built: HashMap<(usize, u8), (VramMesh, Vec<u8>)> = HashMap::new();
    for draw in placements.iter().chain(terrain.iter()) {
        let Some(rtmd) = res.tmds.get(draw.res_tmd) else {
            continue;
        };
        let (mesh, flat) = built
            .entry((draw.env_slot, draw.anim_id))
            .or_insert_with(|| {
                let offsets = (draw.anim_id != 0)
                    .then(|| {
                        anm.and_then(|a| frame0_offsets(a, draw.anim_id, rtmd.tmd.objects.len()))
                    })
                    .flatten();
                match &offsets {
                    Some(o) => crate::scene_assembly::build_hybrid_env_mesh_posed(rtmd, o),
                    None => crate::scene_assembly::build_hybrid_env_mesh(rtmd, &res.vram),
                }
            });
        let lift = lifts.get(draw).copied().unwrap_or([0.0; 3]);
        let mut placed = mesh.clone();
        for p in &mut placed.positions {
            *p = draw.place_point(*p, lift);
        }
        append(&mut out, &mut flat_out, &placed, flat);
    }
    if let Some(hf) = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty())
    {
        let base = out.positions.len() as u32;
        for p in &hf.positions {
            out.positions
                .push([p[0], p[1] + crate::coplanar_draws::GROUND_SINK, p[2]]);
            out.normals.push([0.0, -1.0, 0.0]);
        }
        out.uvs.extend_from_slice(&hf.uvs);
        out.cba_tsb.extend_from_slice(&hf.cba_tsb);
        out.colors.extend_from_slice(&hf.colors);
        for c in &hf.colors {
            flat_out.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        out.indices.extend(hf.indices.iter().map(|i| i + base));
    }
    (out, flat_out)
}

/// The sky strip's texture pages: two 8bpp pages side by side at VRAM
/// `(512, 0)` / `(576, 0)` (`addiu a2,s0,0x88` at `0x801D25F8`, `s0` = the
/// column's parity).
pub const SKY_TPAGE: [u16; 2] = [0x88, 0x89];

/// The sky strip's CLUT word: row 501, x 0 (`li v0,0x7d40` at `0x801D2618`).
pub const SKY_CLUT: u16 = 0x7D40;

/// The fishing frame's clear colour, the `r0 / g0 / b0` bytes the backdrop
/// emitter stores into both draw environments (`0x801D24F8..0x801D2544`).
pub const SKY_CLEAR_RGB: [u8; 3] = [0x17, 0x50, 0xA0];

/// GTE screen centre the backdrop projects through (`OFX`, `OFY`).
const SKY_OFX: f32 = 160.0;
const SKY_OFY: f32 = legaia_engine_vm::battle_cam_script::GTE_OFY;

/// Eye-space depths the backdrop's world-space stand-ins sit at: far behind
/// any venue geometry, inside the projection's far plane, the clear colour
/// behind the strip.
const SKY_DEPTH: f32 = 400_000.0;
const SKY_CLEAR_DEPTH: f32 = 450_000.0;

/// One screen-space backdrop quad: corners in retail 320x240 screen pixels
/// (`xy0`, `xy1`, `xy2`, `xy3` - top-left, top-right, bottom-left,
/// bottom-right), its texels and texture words.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyQuad {
    pub xy: [[f32; 2]; 4],
    pub uv: [[u8; 2]; 4],
    pub clut: u16,
    pub tpage: u16,
}

/// The fishing sky strip, as `FUN_801D24EC` emits it every frame.
///
/// The routine zeroes the yaw global, rebuilds the view (`FUN_800172C0`),
/// loads the eye trio as `TR` and projects the view-space point
/// `(0, 0, 0x1000)` (`FUN_8003D368`) - a point straight ahead at the camera's
/// pitch, so its screen `y` is the horizon. The strip spans `0x100` rows from
/// `sy - 0x74` to `sy + 0x8C`. Horizontally it scrolls with the camera: the
/// start column is `((sx + (focus_x_stored / 64) + yaw) & 0xFF) - 0xFF`, with
/// `focus_x_stored = -x` (rounded toward zero) and `yaw` the saved
/// `_DAT_8007B792`; six 128-wide `POLY_FT4`s follow from there, alternating
/// the two 8bpp pages, `u 0..0x80`, `v 0..0xFF`, colour `0x80`, CLUT row
/// 501, linked at OT word `0x400` - behind the whole venue.
///
/// `view` is the venue camera, `lead_x` the lead's world `x` (the focus) and
/// `yaw_units` the camera yaw global (`-((facing + 0x800) & 0xFFF)`).
///
/// PORT: FUN_801d24ec
pub fn sky_quads(view: &FieldCameraView, lead_x: i16, yaw_units: i16) -> [SkyQuad; 6] {
    // The horizon point, projected with the yaw zeroed.
    let flat = FieldCameraView { yaw: 0.0, ..*view };
    let ahead = [flat.focus[0], flat.focus[1], flat.focus[2] + 4096.0];
    // Retail's eye point is `6 R (0, 0, 0x1000) + TR` (the rotation carries
    // the 6x world scale); the port's eye trio is `TR / 6`, so
    // `R (0, 0, 0x1000) + tr` is the same point at a sixth of the scale - the
    // same screen position.
    let e = flat.eye_space(ahead);
    let (sx, sy) = if e[2] > 0.0 {
        (
            (SKY_OFX + flat.h * e[0] / e[2]).round() as i32,
            (SKY_OFY + flat.h * e[1] / e[2]).round() as i32,
        )
    } else {
        (SKY_OFX as i32, SKY_OFY as i32)
    };
    let focus_stored = -i32::from(lead_x);
    let biased = if focus_stored < 0 {
        focus_stored + 0x3F
    } else {
        focus_stored
    };
    let start = ((sx + (biased >> 6) + i32::from(yaw_units)) & 0xFF) - 0xFF;
    let (top, bottom) = ((sy - 0x74) as f32, (sy + 0x8C) as f32);
    std::array::from_fn(|i| {
        let (row, col) = (i / 2, i % 2);
        let x = (start + (row as i32) * 0x100 + (col as i32) * 0x80) as f32;
        SkyQuad {
            xy: [[x, top], [x + 128.0, top], [x, bottom], [x + 128.0, bottom]],
            uv: [[0, 0], [0x80, 0], [0, 0xFF], [0x80, 0xFF]],
            clut: SKY_CLUT,
            tpage: SKY_TPAGE[col],
        }
    })
}

/// A retail screen point at eye-space depth `z`, back into raw world
/// coordinates under `view` - the inverse of the GTE projection, so a
/// world-space quad built from these corners lands on the screen rect the
/// retail packet covers, behind everything the venue draws.
fn unproject(view: &FieldCameraView, x: f32, y: f32, z: f32) -> [f32; 3] {
    let h = view.h.max(1.0);
    let e = [(x - SKY_OFX) * z / h, (y - SKY_OFY) * z / h, z];
    let d = [
        e[0] - view.tr_eye[0],
        e[1] - view.tr_eye[1],
        e[2] - view.tr_eye[2],
    ];
    let r = legaia_engine_vm::psx_camera::camera_rotation(view.pitch, view.yaw, view.roll);
    // `R^T d`; column-major `r[c*4 + row]` stores `R[row][c]`.
    let mut out = view.focus;
    for (i, o) in out.iter_mut().enumerate() {
        for (j, &dj) in d.iter().enumerate() {
            *o += r[i * 4 + j] * dj;
        }
    }
    out
}

/// The backdrop as world-space geometry under `view`: the clear-colour
/// plate (untextured, [`SKY_CLEAR_RGB`], the whole frame) behind the six
/// strip quads (textured, neutral modulation), `4 * 7` vertices. Hosts move
/// it every frame through [`sky_positions`].
pub fn sky_mesh(view: &FieldCameraView, lead_x: i16, yaw_units: i16) -> (VramMesh, Vec<u8>) {
    let mut m = empty_mesh();
    let mut flat = Vec::new();
    let mut push = |m: &mut VramMesh,
                    corners: [[f32; 3]; 4],
                    uv: [[u8; 2]; 4],
                    ct: [u16; 2],
                    rgba: [u8; 4]| {
        let base = m.positions.len() as u32;
        m.positions.extend_from_slice(&corners);
        m.uvs.extend_from_slice(&uv);
        for _ in 0..4 {
            m.cba_tsb.push(ct);
            m.normals.push([0.0, 0.0, -1.0]);
            m.colors.push([rgba[0], rgba[1], rgba[2]]);
            flat.extend_from_slice(&rgba);
        }
        m.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    };
    let [r, g, b] = SKY_CLEAR_RGB;
    push(
        &mut m,
        sky_positions_clear(view),
        [[0, 0]; 4],
        [0, 0],
        [r, g, b, 0],
    );
    for q in sky_quads(view, lead_x, yaw_units) {
        let n = crate::packet_color::NEUTRAL;
        push(
            &mut m,
            q.xy.map(|p| unproject(view, p[0], p[1], SKY_DEPTH)),
            q.uv,
            [q.clut, q.tpage],
            [n, n, n, 255],
        );
    }
    (m, flat)
}

fn sky_positions_clear(view: &FieldCameraView) -> [[f32; 3]; 4] {
    // Well past the 320x240 frame on every side, so any aspect the host
    // letterboxes to is covered.
    [
        [-640.0, -480.0],
        [960.0, -480.0],
        [-640.0, 720.0],
        [960.0, 720.0],
    ]
    .map(|p| unproject(view, p[0], p[1], SKY_CLEAR_DEPTH))
}

/// This frame's backdrop vertex positions (the layout [`sky_mesh`] builds).
pub fn sky_positions(view: &FieldCameraView, lead_x: i16, yaw_units: i16) -> Vec<[f32; 3]> {
    let mut out = sky_positions_clear(view).to_vec();
    for q in sky_quads(view, lead_x, yaw_units) {
        out.extend(q.xy.map(|p| unproject(view, p[0], p[1], SKY_DEPTH)));
    }
    out
}

/// The posed pond for one frame (raw retail Y-down world coordinates):
/// the bodies first, then the map.
#[derive(Debug, Clone)]
pub struct FishingScene {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Per-vertex packet colour (the textured modulation / untextured fill).
    pub colors: Vec<[u8; 3]>,
    /// Per-vertex `[r, g, b, textured]` - the browser renderer's stream.
    pub flat_rgba: Vec<u8>,
    pub indices: Vec<u32>,
    /// The triangles whose first vertex is textured / untextured.
    pub textured_indices: Vec<u32>,
    pub untextured_indices: Vec<u32>,
    /// The venue camera this frame draws under.
    pub camera: FieldCameraView,
    /// Vertex offset of each body.
    pub bases: Vec<usize>,
    /// Vertex offset of the backdrop ([`sky_mesh`]), the last span.
    pub sky_base: usize,
}

impl FishingScene {
    /// The column-major view-projection a host multiplies a **raw** (Y-down)
    /// world vertex by: the camera's Y-up-frame matrix with the single world
    /// flip folded in.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        use legaia_engine_vm::psx_camera::{WORLD_FLIP, mat4_mul};
        mat4_mul(&self.camera.vp(aspect), &WORLD_FLIP)
    }
}

/// The camera yaw global a lead facing publishes (`_DAT_8007B792`).
fn fa_yaw(facing: i16) -> i16 {
    crate::fishing_actors::fish_camera(0, 0, 0, facing).yaw
}

/// Pose a body's `base` into `out` at clip frame `frame`, then place it at
/// `(x, y, z)` with yaw `facing` (12-bit) - per object `Rz Ry Rx v + T`, the
/// composition every field-body poser runs.
fn pose_into(
    out: &mut [[f32; 3]],
    base: &[[f32; 3]],
    oids: &[u32],
    keys: Option<&Vec<[i32; 6]>>,
    at: [f32; 3],
    facing: i16,
) {
    let a2r = std::f32::consts::TAU / 4096.0;
    let rots: Vec<([f32; 3], [f32; 3], [f32; 3])> = keys
        .map(|k| {
            k.iter()
                .map(|k| {
                    let (sx, cx) = (k[3] as f32 * a2r).sin_cos();
                    let (sy, cy) = (k[4] as f32 * a2r).sin_cos();
                    let (sz, cz) = (k[5] as f32 * a2r).sin_cos();
                    (
                        [sx, sy, sz],
                        [cx, cy, cz],
                        [k[0] as f32, k[1] as f32, k[2] as f32],
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let (ws, wc) = (f32::from(facing) * a2r).sin_cos();
    for (v, (o, b)) in out.iter_mut().zip(base.iter()).enumerate() {
        let [mut x, mut y, mut z] = *b;
        if let Some((s, c, t)) = oids.get(v).and_then(|&o| rots.get(o as usize)) {
            let (ny, nz) = (y * c[0] - z * s[0], y * s[0] + z * c[0]);
            y = ny;
            z = nz;
            let (nx, nz) = (x * c[1] + z * s[1], -x * s[1] + z * c[1]);
            x = nx;
            z = nz;
            let (nx, ny) = (x * c[2] - y * s[2], x * s[2] + y * c[2]);
            x = nx + t[0];
            y = ny + t[1];
            z += t[2];
        }
        *o = [x * wc + z * ws + at[0], y + at[1], -x * ws + z * wc + at[2]];
    }
}

/// The per-host cache both play hosts drive once a frame.
#[derive(Debug, Default)]
pub struct FishingSurface {
    assets: Option<Arc<FishingSceneAssets>>,
    failed: bool,
    scene: Option<FishingScene>,
    generation: u32,
    /// Per-body clip cursor, sixteenths of a frame.
    cursors: Vec<u32>,
}

impl FishingSurface {
    /// One frame. `None` - and the posed buffers dropped - unless a fishing
    /// session is live (`in_fishing`) and the pond decodes. The assets load
    /// once through `index`, bumping [`Self::generation`].
    pub fn frame(
        &mut self,
        index: &ProtIndex,
        mg: &MinigameState,
        in_fishing: bool,
    ) -> Option<&FishingScene> {
        let Some(session) = mg.fishing.as_ref().filter(|_| in_fishing) else {
            if self.scene.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            self.cursors.clear();
            return None;
        };
        if self.assets.is_none() {
            if self.failed {
                return None;
            }
            match FishingSceneAssets::load(index) {
                Some(a) => {
                    self.assets = Some(Arc::new(a));
                    self.generation = self.generation.wrapping_add(1);
                }
                None => {
                    self.failed = true;
                    return None;
                }
            }
        }
        let assets = self.assets.clone()?;
        let venue = session.venue;
        let seats = party_placements(venue);
        // The lead's live pose: the venue's lead actor (its D-pad aim and its
        // floor solve), else his seat.
        let lead = mg.fishing_venue.wander.as_ref();
        let ground = |x: i16, z: i16| -> f32 {
            session
                .venue_map()
                .map(|v| {
                    let ramp = crate::minigame_floor::height_ramp();
                    let grid = crate::minigame_floor::FloorGrid::new(&v.map);
                    crate::fishing_chrome::float_actor_tick(grid, x, z, 0, &ramp).y as f32
                })
                .unwrap_or(0.0)
        };
        let scene = self.scene.get_or_insert_with(|| {
            let mut m = empty_mesh();
            let mut flat = Vec::new();
            let mut bases = Vec::new();
            for b in &assets.bodies {
                bases.push(m.positions.len());
                append(&mut m, &mut flat, &b.mesh, &b.flat);
            }
            append(&mut m, &mut flat, &assets.env, &assets.env_flat);
            let sky_base = m.positions.len();
            let cam = venue_camera_view(seats[0].x, 0, seats[0].z, seats[0].facing);
            let (sky, sky_flat) = sky_mesh(&cam, seats[0].x, fa_yaw(seats[0].facing));
            append(&mut m, &mut flat, &sky, &sky_flat);
            let (mut tex, mut untex) = (Vec::new(), Vec::new());
            for t in m.indices.as_chunks::<3>().0 {
                let textured = flat.get(t[0] as usize * 4 + 3).is_some_and(|&a| a != 0);
                if textured { &mut tex } else { &mut untex }.extend_from_slice(t);
            }
            FishingScene {
                positions: m.positions,
                uvs: m.uvs,
                cba_tsb: m.cba_tsb,
                colors: m.colors,
                flat_rgba: flat,
                indices: m.indices,
                textured_indices: tex,
                untextured_indices: untex,
                camera: cam,
                bases,
                sky_base,
            }
        });
        self.cursors.resize(assets.bodies.len(), 0);
        let mut lead_pose = None;
        for (i, b) in assets.bodies.iter().enumerate() {
            let Some(seat) = seats.iter().find(|s| s.model == b.seat.model) else {
                continue;
            };
            let (x, z, facing, y) = match (i, lead) {
                (0, Some(w)) => (w.x, w.z, w.facing, f32::from(w.y)),
                _ => (seat.x, seat.z, seat.facing, ground(seat.x, seat.z)),
            };
            if i == 0 {
                lead_pose = Some((x, y as i16, z, facing));
            }
            let n = b.frames.len();
            let rate = if seat.rate == 0 {
                16
            } else {
                u32::from(seat.rate)
            };
            let frame = if n == 0 {
                0
            } else {
                (self.cursors[i] >> 4) as usize % n
            };
            self.cursors[i] = if n == 0 {
                0
            } else {
                (self.cursors[i] + rate) % (n as u32 * 16)
            };
            let base = scene.bases[i];
            let len = b.mesh.positions.len();
            pose_into(
                &mut scene.positions[base..base + len],
                &b.mesh.positions,
                &b.oids,
                b.frames.get(frame),
                [f32::from(x), y, f32::from(z)],
                facing,
            );
        }
        let (x, y, z, facing) = lead_pose.unwrap_or((seats[0].x, 0, seats[0].z, seats[0].facing));
        scene.camera = venue_camera_view(x, y, z, facing);
        let sky = sky_positions(&scene.camera, x, fa_yaw(facing));
        let base = scene.sky_base;
        scene.positions[base..base + sky.len()].copy_from_slice(&sky);
        self.scene.as_ref()
    }

    /// Bumped whenever the static buffers or the VRAM a host holds are stale.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The live scene, as the last [`Self::frame`] posed it.
    pub fn scene(&self) -> Option<&FishingScene> {
        self.scene.as_ref()
    }

    /// The pond's VRAM.
    pub fn vram(&self) -> Option<&legaia_tim::Vram> {
        self.scene.as_ref()?;
        Some(self.assets.as_ref()?.vram())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sky_strip_lands_where_the_capture_draws_it() {
        // `minigame_fishing`: six 128x256 packets from x = -169, y 19..275,
        // pages alternating 0x88 / 0x89, CLUT 0x7D40.
        let view = venue_camera_view(4736, -128, 10752, 0x800);
        let q = sky_quads(&view, 4736, fa_yaw(0x800));
        let xs: Vec<f32> = q.iter().map(|q| q.xy[0][0]).collect();
        assert_eq!(xs, vec![-169.0, -41.0, 87.0, 215.0, 343.0, 471.0]);
        assert!(q.iter().all(|q| q.xy[0][1] == 19.0 && q.xy[2][1] == 275.0));
        assert_eq!(q.map(|q| q.tpage), [0x88, 0x89, 0x88, 0x89, 0x88, 0x89]);
        assert!(q.iter().all(|q| q.clut == SKY_CLUT));
    }

    #[test]
    fn the_sky_stand_ins_project_back_onto_their_screen_rects() {
        let view = venue_camera_view(4736, -128, 10752, 0x780);
        let q = sky_quads(&view, 4736, fa_yaw(0x780));
        let pos = sky_positions(&view, 4736, fa_yaw(0x780));
        for (i, q) in q.iter().enumerate() {
            for (k, p) in pos[4 + i * 4..8 + i * 4].iter().enumerate() {
                let e = view.eye_space(*p);
                let sx = SKY_OFX + view.h * e[0] / e[2];
                let sy = SKY_OFY + view.h * e[1] / e[2];
                assert!((sx - q.xy[k][0]).abs() < 0.05 && (sy - q.xy[k][1]).abs() < 0.05);
            }
        }
    }
}
