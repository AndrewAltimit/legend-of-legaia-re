//! Fishing-minigame **3D presentation** methods of [`LegaiaMinigames`]: the
//! fishing venue's own field scene plus the player's field body, decoded from
//! the visitor's disc at load time.
//!
//! Retail hosts the fishing minigame inside the `other1` scene bundle (raw
//! CDNAME `#define other1 1195` - the block directly carrying the fishing
//! overlay's dev name `data\OTHER1`; the overlay's own scene stager writes the
//! `other1` scene name on teardown - the string literal lives in this
//! overlay and nowhere else, which is what pins the venue to fishing rather
//! than to the dance hall (`other7`); see the note on
//! `engine-core::dance::DANCE_SCENE_BLOCK_BASE`
//! and `docs/subsystems/minigame-fishing.md`). The pond, the wooden pier, the
//! shore props and the water sheets are that scene's environment mesh pack
//! instanced by its `.MAP` placement + terrain layers - the same
//! [`field_env`] resolution the site's field-scene viewer and the dance
//! hall bake run.
//!
//! The anglers are the party's real field bodies, seated where the
//! overlay's setup spawns them
//! ([`legaia_engine_core::fishing_venue::party_placements`], three
//! `FUN_80020DE0` calls in `FUN_801CF3BC`): the lead (global pack slot 0)
//! on the venue's anchor tile playing his standing-idle clip from the party
//! locomotion bank (PROT 0874 §1), the second and third flanking him and
//! playing clips `0xB` / `0xC` of the venue scene's own ANM bank. The page
//! frames them through the venue camera the same kernel composes
//! ([`legaia_engine_core::fishing_venue::venue_camera_view`]): behind the
//! party, looking with them across the water.

use super::*;

use legaia_asset::field_objects::{FLAG_PLACED, WalkHeightfield};
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_asset::{character_pack, field_char_textures};
use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};
use legaia_tmd::mesh::{VramMesh, tmd_to_vram_mesh_field_hybrid};
use std::collections::HashMap;

/// Raw CDNAME `#define` index of the `other1` block (the fishing venue
/// scene). The synthetic two-line map below hands `ProtIndex` exactly the
/// frame the real CDNAME.TXT carries (extraction entries 1193..1197 under the
/// -2 filename shift), so the minigames class needs only PROT bytes.
pub(crate) const FISHING_SCENE_DEFINE: usize = 1195;

/// Raw index bounding the block (the next define, `other4`).
pub(crate) const FISHING_SCENE_DEFINE_END: usize = 1200;

/// CDNAME scene name of the fishing venue bundle.
pub(crate) const FISHING_SCENE_NAME: &str = "other1";

/// One static baked mesh (world space, retail Y-down coordinates).
#[derive(Default)]
pub(crate) struct FishingEnv {
    positions: Vec<f32>,
    uvs: Vec<i32>,
    cba_tsb: Vec<u32>,
    flat: Vec<u8>,
    indices: Vec<u32>,
}

impl FishingEnv {
    /// Append one env-pack mesh instanced at an [`field_env::EnvDraw`] - the
    /// record's three authored angles then the world translation
    /// ([`field_env::EnvDraw::place_point`], the same placement composition
    /// as the dance-hall bake, in world space).
    fn append_draw(
        &mut self,
        mesh: &VramMesh,
        flat: &[u8],
        draw: &field_env::EnvDraw,
        lift: [f32; 3],
    ) {
        let base = (self.positions.len() / 3) as u32;
        for p in &mesh.positions {
            self.positions
                .extend_from_slice(&draw.place_point(*p, lift));
        }
        for uv in &mesh.uvs {
            self.uvs.push(uv[0] as i32);
            self.uvs.push(uv[1] as i32);
        }
        for ct in &mesh.cba_tsb {
            self.cba_tsb.push(ct[0] as u32);
            self.cba_tsb.push(ct[1] as u32);
        }
        if flat.is_empty() {
            // The neutral modulation byte, not white: the shader reads
            // `a_flat_rgba` as `texel * rgb * 255/128`, so a white stream
            // doubles the texel. (Unreachable in practice - `flat` is
            // `packet_color::textured`'s walk of `mesh.colors`, parallel to
            // `mesh.positions`.)
            let neutral = [
                legaia_engine_core::packet_color::NEUTRAL,
                legaia_engine_core::packet_color::NEUTRAL,
                legaia_engine_core::packet_color::NEUTRAL,
                255,
            ];
            self.flat
                .extend(std::iter::repeat_n(neutral, mesh.positions.len()).flatten());
        } else {
            self.flat.extend_from_slice(flat);
        }
        self.indices.extend(mesh.indices.iter().map(|i| i + base));
    }

    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// One renderable field body (the angler): hybrid mesh + per-vertex object
/// ids for the pose composer.
pub(crate) struct FishingBody {
    mesh: VramMesh,
    object_ids: Vec<u32>,
    flat: Vec<u8>,
    part_count: usize,
}

/// One shore actor: its field body, its placement, and the clip it plays as
/// absolute per-(frame, bone) `[tx, ty, tz, rx, ry, rz]` + `[bones, frames]`.
pub(crate) struct FishingMember {
    body: FishingBody,
    placement: legaia_engine_core::fishing_venue::FishingPartyMember,
    clip_dims: [u32; 2],
    clip_frames: Vec<i32>,
    /// Whether the member's scene-bank clip matched its rig (else it plays
    /// its locomotion idle).
    scene_clip: bool,
}

/// Every frame of ANM record `rec` as the page's absolute pose stream, or
/// `None` when the record does not decode or its rig width is not `bones`.
fn clip_stream(anm: &PlayerAnmBundle, rec: usize, bones: usize) -> Option<([u32; 2], Vec<i32>)> {
    let r = anm.record_lenient(rec).ok()?;
    if r.bone_count as usize != bones {
        return None;
    }
    let mut out = Vec::with_capacity(r.frame_count as usize * bones * 6);
    for f in 0..r.frame_count as usize {
        for b in 0..bones {
            match anm.bone_transform(rec, f, b) {
                Some(t) => out.extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]),
                None => out.extend_from_slice(&[0; 6]),
            }
        }
    }
    Some(([r.bone_count as u32, r.frame_count as u32], out))
}

/// Everything the fishing panel's 3D layer renders with.
pub(crate) struct FishingScene {
    /// The venue map, baked into one static world-space mesh.
    env: FishingEnv,
    /// World AABB of the baked map (`[lo], [hi]`).
    aabb: ([f32; 3], [f32; 3]),
    /// The walk-ground heightfield, kept for shore-anchor height queries.
    ground: Option<WalkHeightfield>,
    /// The shore party: one body per spawned actor, in spawn order (lead
    /// first). A member whose field body does not decode is left out.
    party: Vec<FishingMember>,
    /// 1 MB PSX VRAM: the scene upload with the PROT 0874 §2 field-character
    /// textures merged on top.
    vram: Vec<u8>,
    /// The venue's `.MAP` buffer and its `+0x10000` region block - what the
    /// cast lure probes for the walk-grid drift and the water class.
    pub(crate) map: Option<Vec<u8>>,
    /// See [`FishingScene::map`].
    pub(crate) region_block: Option<Vec<u8>>,
    /// The venue's three rods and their bend, off the same scene bank - the
    /// geometry the fishing line's rod end is a vertex of.
    pub(crate) rod_mesh: Option<legaia_engine_core::fishing_actors::RodMesh>,
}

/// Frame-0 rigid transforms of scene-ANM record `anim_id - 1` for a bound
/// placement's rest pose (count-equality contract, as on the play page).
fn frame0_bone_offsets(
    anm: &PlayerAnmBundle,
    anim_id: u8,
    objects: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let rec_idx = (anim_id as usize).checked_sub(1)?;
    let rec = anm.record_lenient(rec_idx).ok()?;
    if rec.bone_count as usize != objects {
        return None;
    }
    Some(
        (0..objects)
            .map(|b| match anm.bone_transform(rec_idx, 0, b) {
                Some(t) => (
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                ),
                None => ([0; 3], [0; 3]),
            })
            .collect(),
    )
}

/// Bake the venue's full static map (placed objects + terrain tiles + the
/// walk-ground heightfield) into one [`FishingEnv`], world space.
fn bake_env(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    anm: Option<&PlayerAnmBundle>,
) -> (FishingEnv, Option<WalkHeightfield>) {
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

    // Coplanar ranking across both layers, terrain first then placements -
    // the same order the play page and the native window feed the kernel, so
    // the venue lifts the same member of each coplanar pair they do.
    let mut ranked: Vec<field_env::EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    ranked.extend(terrain.iter().copied());
    ranked.extend(placements.iter().copied());
    let planes = legaia_engine_core::coplanar_draws::draw_plane_summaries(&ranked, res);
    let lifts = legaia_engine_core::coplanar_draws::coplanar_draw_offsets(&ranked, &planes);

    let mut out = FishingEnv::default();
    let mut built: HashMap<(usize, u8), (VramMesh, Vec<u8>)> = HashMap::new();
    for draw in placements.iter().chain(terrain.iter()) {
        let Some(rtmd) = res.tmds.get(draw.res_tmd) else {
            continue;
        };
        let key = (draw.env_slot, draw.anim_id);
        let entry = built.entry(key).or_insert_with(|| {
            let offsets = (draw.anim_id != 0)
                .then(|| {
                    anm.and_then(|a| frame0_bone_offsets(a, draw.anim_id, rtmd.tmd.objects.len()))
                })
                .flatten();
            match &offsets {
                Some(o) => crate::field_scene::build_hybrid_env_mesh_posed(rtmd, o),
                None => crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram),
            }
        });
        let (mesh, flat) = (&entry.0, &entry.1);
        let lift = lifts.get(draw).copied().unwrap_or([0.0; 3]);
        out.append_draw(mesh, flat, draw, lift);
    }

    // The walk-ground heightfield is already world-space.
    let ground = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty());
    if let Some(hf) = ground.as_ref() {
        let base = (out.positions.len() / 3) as u32;
        for p in &hf.positions {
            // Sink the drawn grid below the env pack's authored floor art -
            // both share one plane, so an un-sunk grid draws wedge streaks
            // along its cell diagonals. Render-site only: `ground` below keeps
            // the authored heights, which is what the shore-anchor queries
            // (`fishing_scene_height_at`, `fishing_scene_ground_json`) read.
            out.positions.extend_from_slice(&[
                p[0],
                p[1] + legaia_engine_core::coplanar_draws::GROUND_SINK,
                p[2],
            ]);
        }
        for uv in &hf.uvs {
            out.uvs.push(uv[0] as i32);
            out.uvs.push(uv[1] as i32);
        }
        for ct in &hf.cba_tsb {
            out.cba_tsb.push(ct[0] as u32);
            out.cba_tsb.push(ct[1] as u32);
        }
        // The heightfield's own per-vertex modulation triple
        // (`field_objects::GROUND_PRIM_COLOR` = the neutral byte).
        for c in &hf.colors {
            out.flat.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        out.indices.extend(hf.indices.iter().map(|i| i + base));
    }
    (out, ground)
}

/// Build one hybrid field body out of a Legaia TMD's raw bytes (textured skin
/// prims + flat-shaded body prims in one stream, with per-vertex object ids).
fn hybrid_body(tmd_bytes: &[u8]) -> Option<FishingBody> {
    let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
    let part_count = tmd.objects.len();
    let (mesh, object_ids, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
    let flat = crate::packet_color::hybrid(&mesh, &shading);
    Some(FishingBody {
        mesh,
        object_ids,
        flat,
        part_count,
    })
}

/// Walk-ground height (world Y, retail Y-down) under world `(x, z)`: the
/// nearest heightfield vertex's Y.
fn ground_height(hf: &WalkHeightfield, x: f32, z: f32) -> f32 {
    let mut best = f32::INFINITY;
    let mut y = 0.0f32;
    for p in &hf.positions {
        let d = (p[0] - x) * (p[0] - x) + (p[2] - z) * (p[2] - z);
        if d < best {
            best = d;
            y = p[1];
        }
    }
    y
}

impl FishingScene {
    fn member(&self, member: u32) -> Option<&FishingMember> {
        self.party.get(member as usize)
    }

    /// The floor under a shore seat the way retail settles an actor: the
    /// venue `.MAP` through the shared ground solver (`FUN_801D6028`, the
    /// lead's float tick) - the library capture reads `-128` under the lead.
    /// Falls back to the drawn walk ground without a map.
    fn seat_floor(&self, x: i16, z: i16) -> f32 {
        if let Some(buf) = self.map.as_deref() {
            let ramp = legaia_engine_core::minigame_floor::height_ramp();
            let grid = legaia_engine_core::minigame_floor::FloorGrid::new(buf);
            return legaia_engine_core::fishing_chrome::float_actor_tick(grid, x, z, 0, &ramp).y
                as f32;
        }
        self.ground
            .as_ref()
            .map_or(0.0, |hf| ground_height(hf, x as f32, z as f32))
    }
}

impl LegaiaMinigames {
    /// Decode the fishing venue scene + the angler's body off the loaded PROT
    /// bytes. `None` when the scene bundle doesn't resolve - the page then
    /// plays over a neutral pond drawing and says so.
    pub(crate) fn load_fishing_scene(&mut self) -> Option<FishingScene> {
        // The `other1` bundle, framed by the same synthetic-CDNAME trick the
        // dance-hall build uses (the minigames class holds only PROT bytes).
        let index = ProtIndex::from_bytes(
            self.prot.clone(),
            Some(&format!(
                "#define {FISHING_SCENE_NAME} {FISHING_SCENE_DEFINE} \n#define {FISHING_SCENE_NAME}_end {FISHING_SCENE_DEFINE_END} \n"
            )),
        )
        .ok()?;
        let scene = Scene::load(&index, FISHING_SCENE_NAME).ok()?;
        let (res, _stats) = SceneResources::build_targeted_with_options(
            &scene,
            &[],
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .ok()?;

        // The scene's own ANM bundle (for posed placements), when it carries
        // one - optional, unlike the dance's choreography bank.
        let anm = legaia_engine_core::npc_catalog::scene_anm_bundle(&scene);

        let (env, ground) = bake_env(&index, &scene, &res, anm.as_ref());
        if env.is_empty() {
            return None;
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in env.positions.as_chunks::<3>().0 {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }

        // Merged VRAM: the scene upload + the field-character atlases
        // (PROT 0874 §2, row-478 CLUTs) for the angler's skin.
        let mut vram = res.vram.clone();
        if let Some(raw) = entry_bytes(
            &self.prot,
            &self.entries,
            field_char_textures::PROT_ENTRY_INDEX,
        ) && let Ok(pack) = field_char_textures::parse(raw)
        {
            pack.upload_to_vram(&mut vram, false);
        }

        // The shore party: the setup's three spawns, each the field body in
        // its global-pool slot (Vahn / Noa / Gala - the page has no live
        // party, so the pool is the pack's own order). Active-party TMDs cap
        // to the 10 live groups (the equipment templates are never drawn).
        let pack_raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX);
        let locomotion = pack_raw.and_then(|b| character_pack::field_locomotion_anm(b).ok());
        let pack = pack_raw.and_then(|raw| character_pack::parse(raw).ok());
        let mut party = Vec::new();
        for placement in legaia_engine_core::fishing_venue::party_placements(0) {
            let Some(cslot) = pack.as_ref().and_then(|p| p.slot(placement.model as usize)) else {
                continue;
            };
            let idle_rec = character_pack::locomotion_record_index(
                placement.model as usize,
                character_pack::LOCOMOTION_IDLE_SLOT,
            );
            let idle_bones = locomotion
                .as_ref()
                .and_then(|b| b.record_lenient(idle_rec).ok())
                .map(|r| r.bone_count as u32);
            let mut tmd_bytes = cslot.tmd_bytes.clone();
            if let Some(cap) = idle_bones
                && cslot.is_active_party()
                && tmd_bytes.len() >= 0x0C
            {
                tmd_bytes[0x08..0x0C].copy_from_slice(&cap.to_le_bytes());
            }
            let Some(body) = hybrid_body(&tmd_bytes) else {
                continue;
            };
            // The bound clip: a party-bank clip id resolves against the
            // locomotion bank at the character's stride (clip 2 = record 1,
            // the standing idle); a scene clip against the venue's own bank
            // (`record = id - 1`). A scene clip whose rig does not match the
            // body falls back to the character's idle.
            let bones = body.part_count;
            let scene_stream = (!placement.party_bank)
                .then(|| {
                    anm.as_ref().and_then(|a| {
                        clip_stream(a, (placement.clip as usize).saturating_sub(1), bones)
                    })
                })
                .flatten();
            let scene_clip = scene_stream.is_some();
            let (clip_dims, clip_frames) = scene_stream
                .or_else(|| {
                    locomotion
                        .as_ref()
                        .and_then(|l| clip_stream(l, idle_rec, bones))
                })
                .unwrap_or(([0, 0], Vec::new()));
            party.push(FishingMember {
                body,
                placement,
                clip_dims,
                clip_frames,
                scene_clip,
            });
        }

        let map = scene
            .field_map_index(&index)
            .and_then(|i| index.entry_bytes_extended(i).ok());
        let region_block = scene.field_map_region_block(&index).ok().flatten();
        let rod_mesh = legaia_engine_core::fishing_actors::rod_mesh_from_scene(&scene);

        Some(FishingScene {
            env,
            aabb: (lo, hi),
            ground,
            party,
            vram: vram.as_bytes().to_vec(),
            map,
            region_block,
            rod_mesh,
        })
    }
}

impl FishingScene {
    /// The pond's VRAM as 16-bit words, the shape the screen-primitive
    /// rasteriser samples.
    pub(crate) fn vram_words(&self) -> Vec<u16> {
        self.vram
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect()
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the fishing venue scene decoded off this disc.
    pub fn fishing_scene_ready(&self) -> bool {
        self.fishing_scene.is_some()
    }

    /// Scene status for the page:
    /// `{"aabb":[[lo],[hi]],"player":true,"party":3,"idle_frames":N,"ground":true}`
    /// (`idle_frames` is the lead's clip length).
    pub fn fishing_scene_info_json(&self) -> String {
        let Some(s) = self.fishing_scene.as_ref() else {
            return "null".to_string();
        };
        let (lo, hi) = s.aabb;
        format!(
            r#"{{"aabb":[[{},{},{}],[{},{},{}]],"player":{},"party":{},"idle_frames":{},"ground":{}}}"#,
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            !s.party.is_empty(),
            s.party.len(),
            s.party.first().map_or(0, |m| m.clip_dims[1]),
            s.ground.is_some(),
        )
    }

    /// Baked venue-map vertex positions (`[x, y, z, ...]`, retail world
    /// space, Y down). Empty when the scene didn't decode.
    pub fn fishing_scene_positions(&self) -> Vec<f32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.env.positions.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` texel coords for the baked map.
    pub fn fishing_scene_uvs(&self) -> Vec<i32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.env.uvs.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]` for the baked map.
    pub fn fishing_scene_cba_tsb(&self) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.env.cba_tsb.clone())
            .unwrap_or_default()
    }

    /// Triangle indices for the baked map.
    pub fn fishing_scene_indices(&self) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.env.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the baked map's hybrid
    /// textured / vertex-colour render.
    pub fn fishing_scene_flat_rgba(&self) -> Vec<u8> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.env.flat.clone())
            .unwrap_or_default()
    }

    /// The 1 MB PSX VRAM the venue + angler sample.
    pub fn fishing_scene_vram(&self) -> Vec<u8> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.vram.clone())
            .unwrap_or_default()
    }

    /// Walk-ground geometry summary for the page's shore-anchor fit:
    /// `{"aabb":[[lo],[hi]],"centroid":[x,y,z],"verts":N}` over the
    /// heightfield's own vertices (the walkable shore band - a much tighter
    /// frame than the whole map's AABB). `null` when the scene has no
    /// resolvable floor grid.
    pub fn fishing_scene_ground_json(&self) -> String {
        let Some(hf) = self.fishing_scene.as_ref().and_then(|s| s.ground.as_ref()) else {
            return "null".to_string();
        };
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut sum = [0f64; 3];
        for p in &hf.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
                sum[k] += p[k] as f64;
            }
        }
        let n = hf.positions.len().max(1) as f64;
        format!(
            r#"{{"aabb":[[{},{},{}],[{},{},{}]],"centroid":[{},{},{}],"verts":{}}}"#,
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            sum[0] / n,
            sum[1] / n,
            sum[2] / n,
            hf.positions.len(),
        )
    }

    /// Walk-ground height (world Y, retail Y-down) under world `(x, z)`:
    /// the nearest heightfield vertex's Y. `NaN`-free: returns `0` with no
    /// ground.
    pub fn fishing_scene_height_at(&self, x: f32, z: f32) -> f32 {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.ground.as_ref())
            .map_or(0.0, |hf| ground_height(hf, x, z))
    }

    /// Number of shore-party bodies that decoded (`0..=3`).
    pub fn fishing_party_count(&self) -> u32 {
        self.fishing_scene
            .as_ref()
            .map_or(0, |s| s.party.len() as u32)
    }

    /// The shore party's seats for `venue` (`0` Buma, `1` Vidna), member by
    /// member: `[{"model":m,"x":x,"y":y,"z":z,"facing":f,"rate":r}, ...]`.
    /// `x` / `z` / `facing` are the setup's spawn words
    /// ([`legaia_engine_core::fishing_venue::party_placements`]); `y` is the
    /// floor under the seat (the venue `.MAP` through the ground solver); `rate` the clip rate in sixteenths of a
    /// frame per tick.
    pub fn fishing_party_json(&self, venue: u32) -> String {
        let Some(s) = self.fishing_scene.as_ref() else {
            return "[]".to_string();
        };
        let seats = legaia_engine_core::fishing_venue::party_placements(venue as usize);
        let rows: Vec<String> = s
            .party
            .iter()
            .filter_map(|m| {
                let p = seats.iter().find(|p| p.model == m.placement.model)?;
                let y = s.seat_floor(p.x, p.z);
                let rate = if p.rate == 0 { 16 } else { p.rate };
                Some(format!(
                    r#"{{"model":{},"x":{},"y":{},"z":{},"facing":{},"rate":{}}}"#,
                    p.model, p.x, y, p.z, p.facing, rate
                ))
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// The venue camera for `venue` as a column-major view-projection over
    /// the baked (world-space, Y-down) map: the lead's seat composed through
    /// [`legaia_engine_core::fishing_venue::venue_camera_view`] at the rest
    /// facing. Empty when the scene did not decode.
    pub fn fishing_venue_vp(&self, venue: u32, aspect: f32) -> Vec<f32> {
        let Some(s) = self.fishing_scene.as_ref() else {
            return Vec::new();
        };
        let lead = legaia_engine_core::fishing_venue::party_placements(venue as usize)[0];
        let y = s.seat_floor(lead.x, lead.z);
        let view = legaia_engine_core::fishing_venue::venue_camera_view(
            lead.x,
            y as i16,
            lead.z,
            lead.facing,
        );
        crate::minigames::dance_presentation::dance_venue_vp_in_frame(
            &view,
            (0.0, 0.0, 0.0),
            aspect,
        )
        .to_vec()
    }

    /// The pond's sky backdrop for `venue` under the venue camera (the
    /// shared [`legaia_engine_core::fishing_scene::sky_mesh`] - retail's
    /// `FUN_801D24EC` strip plus the clear colour, as world-space stand-ins
    /// that project onto the retail screen rects). Positions follow the
    /// camera; the other streams are fixed.
    fn fishing_sky(&self, venue: u32) -> Option<(legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        let s = self.fishing_scene.as_ref()?;
        let lead = legaia_engine_core::fishing_venue::party_placements(venue as usize)[0];
        let y = s.seat_floor(lead.x, lead.z);
        let view = legaia_engine_core::fishing_venue::venue_camera_view(
            lead.x,
            y as i16,
            lead.z,
            lead.facing,
        );
        let yaw = legaia_engine_core::fishing_actors::fish_camera(0, 0, 0, lead.facing).yaw;
        Some(legaia_engine_core::fishing_scene::sky_mesh(
            &view, lead.x, yaw,
        ))
    }

    /// Sky backdrop vertex positions for `venue` (world space, Y down).
    pub fn fishing_sky_positions(&self, venue: u32) -> Vec<f32> {
        self.fishing_sky(venue)
            .map(|(m, _)| m.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Sky backdrop `[u, v]` per vertex.
    pub fn fishing_sky_uvs(&self) -> Vec<i32> {
        self.fishing_sky(0)
            .map(|(m, _)| {
                m.uvs
                    .iter()
                    .flat_map(|uv| [uv[0] as i32, uv[1] as i32])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sky backdrop `[cba, tsb]` per vertex.
    pub fn fishing_sky_cba_tsb(&self) -> Vec<u32> {
        self.fishing_sky(0)
            .map(|(m, _)| {
                m.cba_tsb
                    .iter()
                    .flat_map(|c| [c[0] as u32, c[1] as u32])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sky backdrop `[r, g, b, textured]` per vertex.
    pub fn fishing_sky_flat_rgba(&self) -> Vec<u8> {
        self.fishing_sky(0).map(|(_, f)| f).unwrap_or_default()
    }

    /// Sky backdrop triangle indices (local to its own vertices).
    pub fn fishing_sky_indices(&self) -> Vec<u32> {
        self.fishing_sky(0)
            .map(|(m, _)| m.indices)
            .unwrap_or_default()
    }

    /// Member body vertex positions (object-local; the clip pose assembles
    /// them). Empty for a member that did not decode.
    pub fn fishing_player_positions(&self, member: u32) -> Vec<f32> {
        let Some(m) = self.fishing_scene.as_ref().and_then(|s| s.member(member)) else {
            return Vec::new();
        };
        m.body.mesh.positions.iter().flatten().copied().collect()
    }

    /// Per-vertex `[u, v]` for a member body.
    pub fn fishing_player_uvs(&self, member: u32) -> Vec<i32> {
        let Some(m) = self.fishing_scene.as_ref().and_then(|s| s.member(member)) else {
            return Vec::new();
        };
        m.body
            .mesh
            .uvs
            .iter()
            .flat_map(|uv| [uv[0] as i32, uv[1] as i32])
            .collect()
    }

    /// Per-vertex `[cba, tsb]` for a member body.
    pub fn fishing_player_cba_tsb(&self, member: u32) -> Vec<u32> {
        let Some(m) = self.fishing_scene.as_ref().and_then(|s| s.member(member)) else {
            return Vec::new();
        };
        m.body
            .mesh
            .cba_tsb
            .iter()
            .flat_map(|ct| [ct[0] as u32, ct[1] as u32])
            .collect()
    }

    /// Triangle indices for a member body.
    pub fn fishing_player_indices(&self, member: u32) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map(|m| m.body.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (pose bone), parallel to the positions.
    pub fn fishing_player_object_ids(&self, member: u32) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map(|m| m.body.object_ids.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for a member's hybrid render.
    pub fn fishing_player_flat_rgba(&self, member: u32) -> Vec<u8> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map(|m| m.body.flat.clone())
            .unwrap_or_default()
    }

    /// TMD object count (pose rig width) of a member body.
    pub fn fishing_player_part_count(&self, member: u32) -> u32 {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map_or(0, |m| m.body.part_count as u32)
    }

    /// `[bone_count, frame_count]` of the clip a member plays.
    pub fn fishing_player_idle_dims(&self, member: u32) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map_or_else(|| vec![0, 0], |m| m.clip_dims.to_vec())
    }

    /// The clip a member plays as absolute per-(frame, bone)
    /// `[tx, ty, tz, rx, ry, rz]` - the shared pose-stream shape
    /// (`dance_body_pose_frames` / `baka_anim_pose_frames`).
    pub fn fishing_player_idle_frames(&self, member: u32) -> Vec<i32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.member(member))
            .map(|m| m.clip_frames.clone())
            .unwrap_or_default()
    }

    /// Which clip a member plays: `{"clip":id,"party_bank":b,"scene_clip":b}`
    /// - `scene_clip` is whether the venue bank's record matched the body's
    /// rig (otherwise the member plays its locomotion idle).
    pub fn fishing_player_clip_json(&self, member: u32) -> String {
        let Some(m) = self.fishing_scene.as_ref().and_then(|s| s.member(member)) else {
            return "null".to_string();
        };
        format!(
            r#"{{"clip":{},"party_bank":{},"scene_clip":{}}}"#,
            m.placement.clip, m.placement.party_bank, m.scene_clip
        )
    }
}
