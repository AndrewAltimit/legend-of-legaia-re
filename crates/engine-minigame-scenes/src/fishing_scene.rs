//! The fishing minigame's **3D venue surface**: the pond scene (`other1`,
//! the bundle the fishing overlay belongs to) with the party seated on the
//! shore where the overlay's setup spawns them, framed by the venue camera -
//! the one kernel both play hosts draw while the world is in
//! [`legaia_engine_mode::mode::SceneMode::Fishing`], the way
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
//!   shared [`legaia_engine_scene::field_env`] resolution;
//! - the **party** is three field bodies (global character pack slots
//!   `0..=2`) at [`crate::fishing_venue::party_placements`], the lead on his
//!   party-bank idle, the others on their venue-bank clips; the lead's
//!   facing is the aim the D-pad turns (the venue's lead actor);
//! - the **camera** is the venue's scripted camera
//!   ([`crate::fishing_venue::venue_view`]): the shore view over the lead, the
//!   dolly out over the water on the cast, the in-water view, the catch
//!   close-up and showcase;
//! - the **water** heaves: the walk ground's corners are re-resolved every
//!   frame through the venue's live floor-height ladder
//!   ([`crate::fishing_venue::FishingVenue::floor_ladder`]), whose head the
//!   pond swell rewrites, as retail's ground pass re-reads the ladder on
//!   every frame it draws.

use std::collections::HashMap;
use std::sync::Arc;

use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_actor_formats::{character_pack, field_char_textures};
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_scene_formats::field_objects::FLAG_PLACED;
use legaia_tmd::mesh::VramMesh;

use crate::fishing_venue::{FishingPartyMember, party_placements, venue_camera_view};
use crate::minigame_state::MinigameState;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};

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
    /// The venue whose placed box this bake holds.
    venue: usize,
    env: VramMesh,
    env_flat: Vec<u8>,
    /// The walk ground baked into `env` and its first vertex there, kept for
    /// the per-frame swell (its corner tiers).
    ground: Option<(usize, legaia_scene_formats::field_objects::WalkHeightfield)>,
    bodies: Vec<Body>,
    /// The ten landed-fish bodies ([`crate::fishing_catch`]).
    catch: crate::fishing_catch::CatchBodies,
    vram: psx_tim::Vram,
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
pub(crate) fn clip_frames(
    anm: &PlayerAnmBundle,
    rec: usize,
    bones: usize,
) -> Option<Vec<Vec<[i32; 6]>>> {
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
    pub fn load(index: &ProtIndex, venue: usize, party_equip: [u8; 3]) -> Option<Self> {
        let scene = Scene::load(index, FISHING_VENUE_SCENE).ok()?;
        // The boot-resident atlas (raw entry 0 of the system UI bundle): the
        // lure and ripple rings sample its page (`(960, 256)`) and CLUT row
        // (`(64, 510)`), and no venue TIM uploads them. Left out, every ring
        // texel read index 0 of an all-zero palette and the ring drew nothing.
        let system_ui = index.system_ui_bundle().ok();
        let (res, _) = SceneResources::build_targeted_with_options(
            &scene,
            &[],
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: system_ui.as_deref(),
            },
        )
        .ok()?;
        let anm = legaia_engine_scene::npc_catalog::scene_anm_bundle(&scene);
        let (env, env_flat, ground) = bake_env(index, &scene, &res, anm.as_ref(), venue);
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
            // The loader's arm swap (`FUN_8001EBEC`): retail stages the pool
            // through the same `FUN_8001E890` here as on a field entry.
            if let Some(patch) = character_pack::equipment_swap::ACTIVE_PARTY_SLOTS
                .iter()
                .find(|p| p.slot == seat.model)
            {
                tmd_bytes = character_pack::equipment_swap::apply(
                    &tmd_bytes,
                    *patch,
                    party_equip[usize::from(patch.slot)],
                );
            }
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
            let flat = legaia_engine_field::packet_color::hybrid(&mesh, &shading);
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
        let catch = crate::fishing_catch::CatchBodies::load(&scene, anm.as_ref());
        Some(Self {
            venue,
            env,
            env_flat,
            ground,
            bodies,
            catch,
            vram,
        })
    }

    /// The pond VRAM: the scene upload with the field-character atlases.
    pub fn vram(&self) -> &psx_tim::Vram {
        &self.vram
    }
}

/// Bake the venue's static map into one world-space hybrid mesh: placed
/// objects + terrain tiles (coplanar-ranked, as every host ranks a field)
/// and the walk ground, sunk under the authored floor art. The ground comes
/// back too, with its first vertex in the mesh, for the per-frame swell.
#[allow(clippy::type_complexity)]
fn bake_env(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    anm: Option<&PlayerAnmBundle>,
    venue: usize,
) -> (
    VramMesh,
    Vec<u8>,
    Option<(usize, legaia_scene_formats::field_objects::WalkHeightfield)>,
) {
    use legaia_engine_scene::field_env;
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let binds = scene.field_object_binds(index).ok().flatten();
    // The floor pass `FUN_801D6BBC` spawns the unbound placed records only
    // inside the box the setup state latches at the venue's anchor tile (`jal
    // 0x800180EC` at `0x801CF65C`), so each venue sees its own half of the one
    // map: the other's mountain backdrop stands past this box.
    let (tile_x, tile_z) = crate::fishing_venue::PARTY_ANCHOR_TILE[venue.min(1)];
    let block = scene.field_map_region_block(index).ok().flatten();
    let region_box = crate::venue_floor::latched_region_box(
        block.as_deref(),
        (i32::from(tile_x), i32::from(tile_z)),
    );
    let map = scene
        .field_map_bytes(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let placement_records = crate::venue_floor::keep_spawned(
        scene
            .field_object_placements(index)
            .ok()
            .flatten()
            .unwrap_or_default(),
        &crate::venue_floor::floor_pass_cells(&map, region_box),
    );
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
    let planes = legaia_engine_scene::coplanar_draws::draw_plane_summaries(&ranked, res);
    // Ranked in retail's emission order (placements ahead of terrain), the
    // order equal-area ties resolve by; `ranked` keeps its own draw order.
    let lifts = legaia_engine_scene::coplanar_draws::coplanar_draw_offsets(
        &legaia_engine_scene::coplanar_draws::emission_order(&terrain, &placements),
        &planes,
    );

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
                    Some(o) => {
                        legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_posed(rtmd, o)
                    }
                    None => {
                        legaia_engine_scene::scene_assembly::build_hybrid_env_mesh(rtmd, &res.vram)
                    }
                }
            });
        let lift = lifts.get(draw).copied().unwrap_or([0.0; 3]);
        let mut placed = mesh.clone();
        for p in &mut placed.positions {
            *p = draw.place_point(*p, lift);
        }
        append(&mut out, &mut flat_out, &placed, flat);
    }
    let mut ground = None;
    if let Some(hf) = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty())
    {
        let base = out.positions.len() as u32;
        for p in &hf.positions {
            out.positions.push([
                p[0],
                p[1] + legaia_engine_scene::coplanar_draws::GROUND_SINK,
                p[2],
            ]);
            out.normals.push([0.0, -1.0, 0.0]);
        }
        out.uvs.extend_from_slice(&hf.uvs);
        out.cba_tsb.extend_from_slice(&hf.cba_tsb);
        out.colors.extend_from_slice(&hf.colors);
        for c in &hf.colors {
            flat_out.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        out.indices.extend(hf.indices.iter().map(|i| i + base));
        ground = Some((base as usize, hf));
    }
    (out, flat_out, ground)
}

/// The pond ground's drawn positions under the venue's live floor-height
/// ladder: every corner re-resolved from its tier, sunk like the bake
/// ([`legaia_engine_scene::field_ground::live_render_positions`], the kernel
/// the field's animated ladders draw through). Retail's ground pass reads the
/// ladder at `0x1F80035C` for each cell's four corners on every frame it
/// draws, so the water cells (tiers `1..=3`) ride the pond swell
/// ([`legaia_engine_fishing::fishing_chrome::water_swell`]) while the shore
/// (tier `0`) holds. The minigames page draws its ground through this too.
///
/// REF: FUN_801D03B0 (the swell the ladder carries)
pub fn swell_ground_positions(
    hf: &legaia_scene_formats::field_objects::WalkHeightfield,
    ladder: &[i16; 16],
) -> Vec<[f32; 3]> {
    legaia_engine_scene::field_ground::live_render_positions(hf, ladder)
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
const SKY_OFY: f32 = legaia_engine_camera::projection::GTE_OFY;

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
    sky_strip(sx, sy, -i32::from(lead_x), yaw_units)
}

/// The six strip quads for a horizon point projected to `(sx, sy)`: the
/// integer half of [`sky_quads`], from the `FUN_8003D368` call on
/// (`0x801D2588..0x801D2688`). `focus_x_stored` is the camera's stored focus
/// word `_DAT_80089118`, `yaw_units` the saved yaw global `_DAT_8007B792`.
// PORT: FUN_801d24ec
pub fn sky_strip(sx: i32, sy: i32, focus_x_stored: i32, yaw_units: i16) -> [SkyQuad; 6] {
    let biased = if focus_x_stored < 0 {
        focus_x_stored + 0x3F
    } else {
        focus_x_stored
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
        let n = legaia_engine_field::packet_color::NEUTRAL;
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
    /// Vertex offset and count of the walk ground inside the map span - the
    /// vertices the pond swell moves each frame (`(0, 0)` with no ground).
    pub ground_span: (usize, usize),
    /// Vertex offset and count of each species' landed-fish body
    /// ([`crate::fishing_catch`]); a body not on show this frame has every
    /// vertex at one point, so it rasterises nothing.
    pub catch_spans: Vec<Option<(usize, usize)>>,
    /// The landed fish this frame, as the kernel places it.
    pub catch_fish: Option<legaia_engine_fishing::fishing_actors::CatchFish>,
    /// The result plate's line this frame on the 320x240 retail screen,
    /// `(top, tie)` ([`crate::fishing_catch::plate_line`]).
    pub catch_line: Option<((i16, i16), (i16, i16))>,
}

impl FishingScene {
    /// The column-major view-projection a host multiplies a **raw** (Y-down)
    /// world vertex by: the camera's Y-up-frame matrix with the single world
    /// flip folded in.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        use legaia_engine_camera::psx_camera::{WORLD_FLIP, mat4_mul};
        mat4_mul(&self.camera.vp(aspect), &WORLD_FLIP)
    }
}

/// The camera yaw global a lead facing publishes (`_DAT_8007B792`).
fn fa_yaw(facing: i16) -> i16 {
    legaia_engine_fishing::fishing_actors::fish_camera(0, 0, 0, facing).yaw
}

/// Pose a body's `base` into `out` at clip frame `frame`, then place it at
/// `(x, y, z)` with yaw `facing` (12-bit) - per object `Rz Ry Rx v + T`, the
/// composition every field-body poser runs.
pub(crate) fn pose_into(
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
    /// The landed fish's clip frame.
    catch_frame: usize,
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
        party_equip: [u8; 3],
    ) -> Option<&FishingScene> {
        let Some(session) = mg.fishing.as_ref().filter(|_| in_fishing) else {
            if self.scene.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            self.cursors.clear();
            return None;
        };
        let venue = session.venue;
        if self.assets.as_ref().is_some_and(|a| a.venue != venue) {
            self.assets = None;
            self.scene = None;
            self.cursors.clear();
        }
        if self.assets.is_none() {
            if self.failed {
                return None;
            }
            match FishingSceneAssets::load(index, venue, party_equip) {
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
        let seats = party_placements(venue);
        // The lead's live pose: the venue's lead actor (its D-pad aim and its
        // floor solve), else his seat.
        let lead = mg.fishing_venue.wander.as_ref();
        let ground = |x: i16, z: i16| -> f32 {
            session
                .venue_map()
                .map(|v| {
                    let grid = legaia_engine_field::minigame_floor::FloorGrid::new(&v.map);
                    legaia_engine_fishing::fishing_chrome::float_actor_tick(
                        grid,
                        x,
                        z,
                        0,
                        &mg.fishing_venue.floor_ladder,
                    )
                    .y as f32
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
            let (catch_mesh, catch_flat, spans) = assets.catch.combined();
            let catch_base = m.positions.len();
            append(&mut m, &mut flat, &catch_mesh, &catch_flat);
            let catch_spans = spans
                .into_iter()
                .map(|sp| sp.map(|(b, n)| (catch_base + b, n)))
                .collect();
            let env_base = m.positions.len();
            append(&mut m, &mut flat, &assets.env, &assets.env_flat);
            let ground_span = assets
                .ground
                .as_ref()
                .map_or((0, 0), |(b, hf)| (env_base + b, hf.positions.len()));
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
                ground_span,
                catch_spans,
                catch_fish: None,
                catch_line: None,
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
        // The venue's scripted camera (shore view, cast dolly, in-water view,
        // catch close-up and showcase); the lead's shore view until it arms.
        let (camera, yaw) = match (lead, crate::fishing_venue::venue_view(mg)) {
            (Some(_), Some(view)) => (view, mg.fishing_venue.camera.camera().rot[1]),
            _ => (venue_camera_view(x, y, z, facing), fa_yaw(facing)),
        };
        scene.camera = camera;
        let sky = sky_positions(&scene.camera, x, yaw);
        let base = scene.sky_base;
        scene.positions[base..base + sky.len()].copy_from_slice(&sky);
        if let Some((_, hf)) = assets.ground.as_ref() {
            let (base, n) = scene.ground_span;
            let live = swell_ground_positions(hf, &mg.fishing_venue.floor_ladder);
            scene.positions[base..base + n].copy_from_slice(&live);
        }
        // The landed fish: the lifted body through the close-up, the
        // camera-locked one on the result plate, under this frame's camera.
        let fish = legaia_engine_fishing::fishing_actors::catch_fish(
            session,
            &mg.fishing_venue.camera,
            &mg.fishing_catch_poses,
        );
        self.catch_frame = if fish.is_some() {
            self.catch_frame.wrapping_add(1)
        } else {
            0
        };
        scene.catch_line = assets.catch.pose_spans(
            &mut scene.positions,
            &scene.catch_spans,
            fish.as_ref(),
            &mg.fishing_catch_poses,
            self.catch_frame,
            &scene.camera,
        );
        scene.catch_fish = fish;
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
    pub fn vram(&self) -> Option<&psx_tim::Vram> {
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
