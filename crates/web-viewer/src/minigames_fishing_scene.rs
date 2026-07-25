//! Fishing-minigame **3D presentation** methods of [`LegaiaMinigames`]: the
//! fishing venue's own field scene plus the player's field body, decoded from
//! the visitor's disc at load time.
//!
//! Retail hosts the fishing minigame inside the `other1` scene bundle (raw
//! CDNAME `#define other1 1195` - the block directly carrying the fishing
//! overlay's dev name `data\OTHER1`; the overlay's own scene stager writes the
//! `other1` scene name, see `engine-core::dance::DANCE_SCENE_NAME` provenance
//! and `docs/subsystems/minigame-fishing.md`). The pond, the wooden pier, the
//! shore props and the water sheets are that scene's environment mesh pack
//! instanced by its `.MAP` placement + terrain layers - the same
//! [`field_env`] resolution the site's field-scene viewer and the dance
//! hall bake run.
//!
//! The angler is the lead's real field body: Vahn's mesh from the global
//! character pack (PROT 0874 §0 slot 0) posed by his standing-idle clip from
//! the party locomotion ANM bundle (PROT 0874 §1, idle = bank slot 1 - the
//! same records the play page walks). The browser twin of the dance floor's
//! cast build (`minigames_dance.rs`).
//!
//! What is **fitted rather than traced** (stated on the page): the retail
//! fishing camera's exact framing and the angler's exact shore anchor - the
//! overlay positions its actors through runtime globals no static parse
//! recovers - so the page frames the venue from the scene's own bounds and
//! anchors the angler on the walk-ground heightfield.

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
    /// authored yaw about Y, then the world translation (the same placement
    /// composition as the dance-hall bake, in world space).
    fn append_draw(&mut self, mesh: &VramMesh, flat: &[u8], draw: &field_env::EnvDraw) {
        let theta = (draw.rot_y & 0xFFF) as f32 * (std::f32::consts::TAU / 4096.0);
        let (sin, cos) = theta.sin_cos();
        let base = (self.positions.len() / 3) as u32;
        for p in &mesh.positions {
            let (vx, vy, vz) = (p[0], p[1], p[2]);
            self.positions
                .push(vx * cos + vz * sin + draw.world_x as f32);
            self.positions.push(vy + draw.world_y as f32);
            self.positions
                .push(-vx * sin + vz * cos + draw.world_z as f32);
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
            self.flat
                .extend(std::iter::repeat_n([255u8; 4], mesh.positions.len()).flatten());
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

/// Everything the fishing panel's 3D layer renders with.
pub(crate) struct FishingScene {
    /// The venue map, baked into one static world-space mesh.
    env: FishingEnv,
    /// World AABB of the baked map (`[lo], [hi]`).
    aabb: ([f32; 3], [f32; 3]),
    /// The walk-ground heightfield, kept for shore-anchor height queries.
    ground: Option<WalkHeightfield>,
    /// The angler's field body (lead character, PROT 0874 §0 slot 0).
    player: Option<FishingBody>,
    /// The angler's standing-idle clip: `[bones, frames]` + the absolute
    /// per-(frame, bone) pose stream.
    idle_dims: [u32; 2],
    idle_frames: Vec<i32>,
    /// 1 MB PSX VRAM: the scene upload with the PROT 0874 §2 field-character
    /// textures merged on top.
    vram: Vec<u8>,
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
        out.append_draw(mesh, flat, draw);
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
            out.positions.extend_from_slice(p);
        }
        for uv in &hf.uvs {
            out.uvs.push(uv[0] as i32);
            out.uvs.push(uv[1] as i32);
        }
        for ct in &hf.cba_tsb {
            out.cba_tsb.push(ct[0] as u32);
            out.cba_tsb.push(ct[1] as u32);
        }
        out.flat
            .extend(std::iter::repeat_n([255u8; 4], hf.positions.len()).flatten());
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
    let mut flat = Vec::with_capacity(shading.colors.len() * 4);
    for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
        flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
    }
    Some(FishingBody {
        mesh,
        object_ids,
        flat,
        part_count,
    })
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
        let anm = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7].into_iter().find_map(|desc| {
                legaia_asset::player_anm::find_in_entry(&e.bytes, desc)
                    .into_iter()
                    .next()
            })
        });

        let (env, ground) = bake_env(&index, &scene, &res, anm.as_ref());
        if env.is_empty() {
            return None;
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in env.positions.chunks_exact(3) {
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

        // The angler: the lead's field body (Vahn, global pack slot 0) posed
        // by his standing-idle locomotion clip. Active-party TMDs cap to the
        // 10 live groups (the equipment templates are never drawn).
        let pack_raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX);
        let locomotion = pack_raw.and_then(|b| character_pack::field_locomotion_anm(b).ok());
        let idle_rec =
            character_pack::locomotion_record_index(0, character_pack::LOCOMOTION_IDLE_SLOT);
        let mut idle_dims = [0u32; 2];
        let mut idle_frames = Vec::new();
        let player = pack_raw
            .and_then(|raw| character_pack::parse(raw).ok())
            .and_then(|pack| {
                let cslot = pack.slot(0)?;
                let mut tmd_bytes = cslot.tmd_bytes.clone();
                let mut cap = None;
                if let Some(bundle) = locomotion.as_ref()
                    && let Ok(rec) = bundle.record_lenient(idle_rec)
                {
                    cap = Some(rec.bone_count as u32);
                    idle_dims = [rec.bone_count as u32, rec.frame_count as u32];
                    let bones = rec.bone_count as usize;
                    for f in 0..rec.frame_count as usize {
                        for b in 0..bones {
                            match bundle.bone_transform(idle_rec, f, b) {
                                Some(t) => idle_frames
                                    .extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]),
                                None => idle_frames.extend_from_slice(&[0; 6]),
                            }
                        }
                    }
                }
                if let Some(cap) = cap
                    && cslot.is_active_party()
                    && tmd_bytes.len() >= 0x0C
                {
                    tmd_bytes[0x08..0x0C].copy_from_slice(&cap.to_le_bytes());
                }
                hybrid_body(&tmd_bytes)
            });

        Some(FishingScene {
            env,
            aabb: (lo, hi),
            ground,
            player,
            idle_dims,
            idle_frames,
            vram: vram.as_bytes().to_vec(),
        })
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the fishing venue scene decoded off this disc.
    pub fn fishing_scene_ready(&self) -> bool {
        self.fishing_scene.is_some()
    }

    /// Scene status for the page + the fitted framing:
    /// `{"aabb":[[lo],[hi]],"player":true,"idle_frames":N,"ground":true}`.
    pub fn fishing_scene_info_json(&self) -> String {
        let Some(s) = self.fishing_scene.as_ref() else {
            return "null".to_string();
        };
        let (lo, hi) = s.aabb;
        format!(
            r#"{{"aabb":[[{},{},{}],[{},{},{}]],"player":{},"idle_frames":{},"ground":{}}}"#,
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            s.player.is_some(),
            s.idle_dims[1],
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

    /// Walk-ground height (world Y, retail Y-down) under world `(x, z)`:
    /// the nearest heightfield vertex's Y. `NaN`-free: returns `0` with no
    /// ground. Used by the page to anchor the angler on the shore.
    pub fn fishing_scene_height_at(&self, x: f32, z: f32) -> f32 {
        let Some(hf) = self.fishing_scene.as_ref().and_then(|s| s.ground.as_ref()) else {
            return 0.0;
        };
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

    /// Angler body vertex positions (object-local; the idle pose assembles
    /// them). Empty when the character pack didn't decode.
    pub fn fishing_player_positions(&self) -> Vec<f32> {
        let Some(p) = self.fishing_scene.as_ref().and_then(|s| s.player.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(p.mesh.positions.len() * 3);
        for v in &p.mesh.positions {
            out.extend_from_slice(v);
        }
        out
    }

    /// Per-vertex `[u, v]` for the angler body.
    pub fn fishing_player_uvs(&self) -> Vec<i32> {
        let Some(p) = self.fishing_scene.as_ref().and_then(|s| s.player.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(p.mesh.uvs.len() * 2);
        for uv in &p.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` for the angler body.
    pub fn fishing_player_cba_tsb(&self) -> Vec<u32> {
        let Some(p) = self.fishing_scene.as_ref().and_then(|s| s.player.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(p.mesh.cba_tsb.len() * 2);
        for ct in &p.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for the angler body.
    pub fn fishing_player_indices(&self) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.player.as_ref())
            .map(|p| p.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (pose bone), parallel to the positions.
    pub fn fishing_player_object_ids(&self) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.player.as_ref())
            .map(|p| p.object_ids.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the angler's hybrid render.
    pub fn fishing_player_flat_rgba(&self) -> Vec<u8> {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.player.as_ref())
            .map(|p| p.flat.clone())
            .unwrap_or_default()
    }

    /// TMD object count (pose rig width) of the angler body.
    pub fn fishing_player_part_count(&self) -> u32 {
        self.fishing_scene
            .as_ref()
            .and_then(|s| s.player.as_ref())
            .map(|p| p.part_count as u32)
            .unwrap_or(0)
    }

    /// `[bone_count, frame_count]` of the angler's standing-idle clip.
    pub fn fishing_player_idle_dims(&self) -> Vec<u32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.idle_dims.to_vec())
            .unwrap_or_else(|| vec![0, 0])
    }

    /// The angler's standing-idle clip as absolute per-(frame, bone)
    /// `[tx, ty, tz, rx, ry, rz]` - the shared pose-stream shape
    /// (`dance_body_pose_frames` / `baka_anim_pose_frames`).
    pub fn fishing_player_idle_frames(&self) -> Vec<i32> {
        self.fishing_scene
            .as_ref()
            .map(|s| s.idle_frames.clone())
            .unwrap_or_default()
    }
}
