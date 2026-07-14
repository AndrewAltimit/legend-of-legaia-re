//! Dance-minigame **presentation** methods of [`LegaiaMinigames`] - the
//! retail HUD art, the dancer face-stamp, the SFX cue bank and the BGM,
//! all decoded from the visitor's own disc at load time.
//!
//! The dance overlay (PROT 0980) draws its whole HUD through one emitter
//! (`FUN_801d2f38`) over a 34-record widget table in its own rodata; the art
//! those widgets sample is the PROT 1230 TIM pack the mode-24 entry path
//! stages at VRAM `(512, 0)` (see [`legaia_asset::dance_art`] and
//! `docs/subsystems/minigame-dance.md`). This module hands the page the same
//! table, the same page, and the traced emitter geometry, so the JS side
//! never invents a rect.

use super::*;

use std::collections::HashMap;

use legaia_asset::dance_art::{self, DanceWidget};
use legaia_asset::dance_cast::{self, DanceCast, DanceClip};
use legaia_asset::field_objects::FLAG_PLACED;
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_asset::{character_pack, field_char_textures};
use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};
use legaia_tmd::mesh::{VramMesh, tmd_to_vram_mesh_field_hybrid};

/// Everything the dance panel renders with, decoded once at disc load.
pub(crate) struct DancePresentation {
    /// The PROT 1230 pack (HUD page, face strips, venue pages).
    pub art: Vec<Tim>,
    /// The overlay's 34 HUD widget descriptors.
    pub widgets: Vec<DanceWidget>,
    /// Per-rig face frame tables out of the overlay image.
    pub face_frames: Vec<Vec<[u8; 4]>>,
    /// Noa's field atlas (PROT 0874 §2 entry 2) - the human dancer's strip.
    pub noa_atlas: Option<Tim>,
    /// The dance's own SFX cue bank (descriptors PROT 1228, samples PROT
    /// 1231 - the entry the TOC tail fix makes reachable).
    pub sfx: Option<SfxCueBank>,
    /// The same VAB, parsed directly, for the direct-keyed hit stings
    /// (`FUN_801d3d78` bypasses the cue ring and keys two voices itself).
    pub sting_vab: Option<(legaia_vab::VabReport, Vec<u8>)>,
}

/// One dancer's renderable body, built once at disc load.
pub(crate) struct DanceBodyMesh {
    mesh: VramMesh,
    /// Per-vertex TMD object index (the bone each vertex hangs from).
    object_ids: Vec<u32>,
    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid shader.
    flat: Vec<u8>,
    /// TMD object count = the pose rig width.
    part_count: usize,
    /// Dancer-kind descriptor index (`legaia_asset::dance_cast`; kind = the
    /// face-stamp rig id for kinds 0..=3).
    kind: usize,
    /// Qualifier-mode floor spawn `(x, z)` (the overlay's spawn table).
    spawn: (i16, i16),
}

/// The dance hall itself, baked into one **static world-space mesh** in the
/// dancer frame (retail Y-down world coordinates, translated so the human
/// dancer's floor spawn is the origin): the `other7` scene's environment mesh
/// pack instanced by its own `.MAP` placement + terrain-tile layers - the
/// stage, the checkered dance floor, the portrait banners, the spotlight
/// cones, the speaker/lamp fixtures - plus the walk-ground heightfield. In
/// retail the hall is exactly this: the host field scene's 3D geometry drawn
/// under the overlay's HUD (the overlay itself loads no mesh - see
/// `docs/subsystems/minigame-dance.md`). Placed props whose object bind names
/// a clip are baked **posed** at frame 0 of that clip, the same rest-state
/// rule the play page applies.
#[derive(Default)]
pub(crate) struct DanceEnv {
    positions: Vec<f32>,
    uvs: Vec<i32>,
    cba_tsb: Vec<u32>,
    flat: Vec<u8>,
    indices: Vec<u32>,
}

impl DanceEnv {
    /// Append one env-pack mesh instanced at an [`field_env::EnvDraw`],
    /// re-based to `origin` (the human dancer's spawn). The transform is the
    /// retail placement composition in the Y-down world frame: rotate the
    /// object-local vertices about Y by the authored yaw, then translate to
    /// the draw's world position (the browser twin of the play page's
    /// `placementModelScaledY`, without the view-time Y flip - the dance
    /// page's orbit camera applies that flip globally, exactly as it already
    /// does for the dancer bodies).
    fn append_draw(
        &mut self,
        mesh: &VramMesh,
        flat: &[u8],
        draw: &field_env::EnvDraw,
        origin: (f32, f32, f32),
    ) {
        let theta = (draw.rot_y & 0xFFF) as f32 * (std::f32::consts::TAU / 4096.0);
        let (sin, cos) = theta.sin_cos();
        let base = (self.positions.len() / 3) as u32;
        for p in &mesh.positions {
            let (vx, vy, vz) = (p[0], p[1], p[2]);
            self.positions
                .push(vx * cos + vz * sin + draw.world_x as f32 - origin.0);
            self.positions.push(vy + draw.world_y as f32 - origin.1);
            self.positions
                .push(-vx * sin + vz * cos + draw.world_z as f32 - origin.2);
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
            // Pure-textured mesh: every vertex samples VRAM.
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

/// Frame-0 rigid transforms of scene-ANM record `anim_id - 1`, for baking a
/// bound placement's rest pose - `None` (draw unposed) when the record is
/// missing or its bone count doesn't match the mesh's object count (retail's
/// count-equality contract, the same guard the play page applies).
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

/// Bake the `other7` scene's full static map into one [`DanceEnv`] mesh:
/// the env-pack vote + the `.MAP` placed-object / terrain-tile resolution
/// (the same [`field_env`] calls the play page makes - posed placements via
/// `resolve_placed_env_draws`, `FLAG_PLACED` records excluded from the
/// terrain sweep) + the walk-ground heightfield, every draw transformed to
/// world space and re-based on the human dancer's spawn.
fn bake_dance_env(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    anm: &PlayerAnmBundle,
    origin: (f32, f32, f32),
) -> DanceEnv {
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

    let mut out = DanceEnv::default();
    let mut built: HashMap<(usize, u8), (VramMesh, Vec<u8>)> = HashMap::new();
    for draw in placements.iter().chain(terrain.iter()) {
        let Some(rtmd) = res.tmds.get(draw.res_tmd) else {
            continue;
        };
        let key = (draw.env_slot, draw.anim_id);
        let entry = built.entry(key).or_insert_with(|| {
            let offsets = (draw.anim_id != 0)
                .then(|| frame0_bone_offsets(anm, draw.anim_id, rtmd.tmd.objects.len()))
                .flatten();
            match &offsets {
                Some(o) => crate::field_scene::build_hybrid_env_mesh_posed(rtmd, o),
                None => crate::field_scene::build_hybrid_env_mesh(rtmd, &res.vram),
            }
        });
        let (mesh, flat) = (&entry.0, &entry.1);
        out.append_draw(mesh, flat, draw, origin);
    }

    // The walk-ground heightfield: already world-space (Y-down), so only the
    // origin re-base applies.
    if let Some(hf) = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty())
    {
        let base = (out.positions.len() / 3) as u32;
        for p in &hf.positions {
            out.positions.push(p[0] - origin.0);
            out.positions.push(p[1] - origin.1);
            out.positions.push(p[2] - origin.2);
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
    out
}

/// The dance cast + the choreography ANM bundle + the VRAM the bodies sample,
/// decoded once at disc load. The dance overlay (PROT 0980) loads no mesh of
/// its own - its spawner (`FUN_801d0190`) draws each dancer from a 5-kind
/// descriptor table baked in the overlay: kind 0 is **Noa's resident field
/// mesh** (global pool slot 1 = PROT 0874 §0 slot 1) and kinds 1..4 are the
/// **dedicated dancer NPCs of the dance-hall scene module** (`other7`, the
/// block that also carries the dance's efect.dat + art pack) - Mary, the two
/// competitor dancers, and the Disco King. Every dance clip (idle, the
/// dance-groove loop, and the 11 judge-triggered moves) is a record of that
/// scene's 60-record MOVE ANM bundle (PROT 1229). See
/// `docs/subsystems/minigame-dance.md` § Dancer bodies and
/// [`legaia_asset::dance_cast`].
pub(crate) struct DanceBodies {
    /// The qualifier-mode floor cast, left..right by spawn x =
    /// `[kind 2, Noa (human), kind 3]`.
    dancers: Vec<DanceBodyMesh>,
    /// Index of the human dancer (kind 0) in `dancers`.
    human: usize,
    /// The dance-hall scene's MOVE ANM bundle - the choreography bank.
    anm: PlayerAnmBundle,
    /// The overlay's cast + choreography tables.
    cast: DanceCast,
    /// 1 MB PSX VRAM: the dance-hall scene upload (the dancer NPC atlases +
    /// row-480/481 CLUTs) with the PROT 0874 §2 field-character textures
    /// (Noa's atlas, row 478) merged on top.
    vram: Vec<u8>,
    /// The dance hall's static geometry, baked in the dancer frame
    /// (empty when the scene's placement layers didn't resolve).
    env: DanceEnv,
}

/// Number of clip slots exposed per dancer: idle, the dance loop, and the
/// [`dance_cast::MOVE_PAIRS`] judge-triggered moves.
const DANCE_CLIPS: usize = 2 + dance_cast::MOVE_PAIRS;

impl LegaiaMinigames {
    /// Decode the dance presentation off the loaded PROT bytes. Any piece
    /// that fails stays `None`/absent - the page states the gap instead of
    /// faking art.
    pub(crate) fn load_dance_presentation(&mut self) -> Option<DancePresentation> {
        let overlay = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
        )?;
        let art = entry_bytes(
            &self.prot,
            &self.entries,
            dance_art::DANCE_ART_PROT_INDEX as u32,
        )
        .and_then(|raw| dance_art::parse_art_pack(raw).ok())?;
        let widgets = dance_art::parse_widgets(&overlay).ok()?;
        let face_frames = dance_art::FACE_RIGS
            .iter()
            .map(|rig| dance_art::parse_face_frames(&overlay, rig).unwrap_or_default())
            .collect();
        let noa_atlas = entry_bytes(
            &self.prot,
            &self.entries,
            field_char_textures::PROT_ENTRY_INDEX,
        )
        .and_then(|raw| field_char_textures::parse(raw).ok())
        .and_then(|pack| {
            let rig = &dance_art::FACE_RIGS[0];
            pack.textures
                .into_iter()
                .map(|t| t.tim)
                .find(|t| t.image.fb_x == rig.base.0 && t.image.fb_y == rig.base.1)
        });
        let vab_entry = entry_bytes(
            &self.prot,
            &self.entries,
            dance_art::DANCE_SFX_VAB_PROT_INDEX as u32,
        );
        let sfx = match (
            entry_bytes(
                &self.prot,
                &self.entries,
                dance_art::DANCE_SFX_BANK_PROT_INDEX as u32,
            ),
            vab_entry,
        ) {
            (Some(bank), Some(vab)) => SfxCueBank::new(bank, vab).ok(),
            _ => None,
        };
        // Sample spans in the report index into the whole entry buffer.
        let sting_vab = vab_entry.and_then(|entry| {
            let off = *legaia_vab::find_vabs(entry).first()?;
            let report = legaia_vab::parse(entry, off).ok()?;
            Some((report, entry.to_vec()))
        });
        Some(DancePresentation {
            art,
            widgets,
            face_frames,
            noa_atlas,
            sfx,
            sting_vab,
        })
    }
}

/// Build one renderable body out of a Legaia TMD's raw bytes: the field
/// **hybrid** build (textured skin prims + flat-shaded body prims in one
/// vertex stream) with parallel per-vertex object ids for the pose composer.
fn hybrid_body(tmd_bytes: &[u8], kind: usize, spawn: (i16, i16)) -> Option<DanceBodyMesh> {
    let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
    let part_count = tmd.objects.len();
    let (mesh, object_ids, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
    let mut flat = Vec::with_capacity(shading.colors.len() * 4);
    for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
        flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
    }
    Some(DanceBodyMesh {
        mesh,
        object_ids,
        flat,
        part_count,
        kind,
        spawn,
    })
}

impl LegaiaMinigames {
    /// Noa's body - the overlay spawns dancer kind 0 from the resident global
    /// TMD pool (slot 1 = PROT 0874 §0 pack slot 1, her field-view mesh; the
    /// spawner writes that model id *without* the scene-pool base). Mirrors
    /// the viewer's field-character build: the active-party TMD is capped to
    /// the retail 10 live groups (groups 10/11 are the equipment templates,
    /// never drawn - FUN_8001E890).
    fn build_noa_body(&self, spawn: (i16, i16)) -> Option<DanceBodyMesh> {
        let raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX)?;
        let pack = character_pack::parse(raw).ok()?;
        let cslot = pack.slot(1)?;
        let mut tmd_bytes = cslot.tmd_bytes.clone();
        if cslot.is_active_party() && tmd_bytes.len() >= 0x0C {
            tmd_bytes[0x08..0x0C].copy_from_slice(&10u32.to_le_bytes());
        }
        hybrid_body(&tmd_bytes, 0, spawn)
    }

    /// Decode the dance cast off the loaded PROT bytes: the overlay's spawn +
    /// kind descriptor tables, the dance-hall scene's dancer NPC meshes +
    /// choreography ANM bundle, and the merged VRAM. `None` when any leg
    /// doesn't decode - the page then states the gap instead of faking a cast.
    pub(crate) fn load_dance_bodies(&mut self) -> Option<DanceBodies> {
        let overlay = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
        )?;
        let cast = dance_cast::parse(&overlay)?;

        // The dance-hall scene module. Only its CDNAME define is needed to
        // frame the block, so a two-line synthetic map keeps this path free
        // of the full CDNAME.TXT (the minigames class only holds PROT bytes).
        // The terminator define bounds the block at raw index 0x4D1 (the SFX
        // VAB, not scene data): the entries past it sit in the PROT TOC's
        // zeroed tail, where the indexed size formula underflows to a ~4 GiB
        // footprint - harmless on 64-bit hosts (overcommit), but a reserve
        // past `isize::MAX` on wasm32. The ProtIndex clone is dropped again
        // before this function returns.
        let index = ProtIndex::from_bytes(
            self.prot.clone(),
            Some(&format!(
                "#define {} 1228 \n#define {}_end 1233 \n",
                dance_cast::DANCE_SCENE_NAME,
                dance_cast::DANCE_SCENE_NAME
            )),
        )
        .ok()?;
        let scene = Scene::load(&index, dance_cast::DANCE_SCENE_NAME).ok()?;
        let (res, _stats) = SceneResources::build_targeted_with_options(
            &scene,
            &[],
            BuildOptions {
                kind: SceneLoadKind::Field,
                // Retail's loader DMA-uploads every scene TIM; the dancer
                // atlases + their row-480/481 CLUTs must all be resident.
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .ok()?;

        // The scene's MOVE ANM bundle - the 60-record choreography bank.
        let anm = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7].into_iter().find_map(|desc| {
                legaia_asset::player_anm::find_in_entry(&e.bytes, desc)
                    .into_iter()
                    .next()
            })
        })?;

        // Merged VRAM: the scene upload + Noa's field-character atlas
        // (PROT 0874 §2, row-478 CLUTs) - disjoint rects, one buffer.
        let mut vram = res.vram.clone();
        if let Some(raw) = entry_bytes(
            &self.prot,
            &self.entries,
            field_char_textures::PROT_ENTRY_INDEX,
        ) && let Ok(pack) = field_char_textures::parse(raw)
        {
            pack.upload_to_vram(&mut vram, false);
        }

        // The floor cast: the qualifier (yosenn) spawn table, left..right by
        // spawn x - `[kind 2, Noa, kind 3]` on the retail floor.
        let mut spawns = cast.qualifier.clone();
        spawns.sort_by_key(|s| s.x);
        let mut dancers = Vec::with_capacity(spawns.len());
        let mut human = 0usize;
        for s in &spawns {
            let kind = s.kind as usize;
            let body = if kind == 0 {
                human = dancers.len();
                self.build_noa_body((s.x, s.z))?
            } else {
                let model = cast.kinds.get(kind)?.model as usize;
                let t = res.tmds.get(model)?;
                hybrid_body(&t.raw, kind, (s.x, s.z))?
            };
            dancers.push(body);
        }

        // The dance hall around them - the scene's own placed geometry,
        // re-based so the human dancer's spawn is the origin (the frame the
        // page already poses the bodies in).
        let hs = &spawns[human.min(spawns.len() - 1)];
        let env = bake_dance_env(
            &index,
            &scene,
            &res,
            &anm,
            (hs.x as f32, hs.y as f32, hs.z as f32),
        );

        Some(DanceBodies {
            dancers,
            human,
            anm,
            cast,
            vram: vram.as_bytes().to_vec(),
            env,
        })
    }

    /// Dancer `dancer`'s clip slot `clip` (0 = idle, 1 = the dance-groove
    /// loop, `2 + k` = judge-triggered move pair `k`).
    fn dance_clip(&self, dancer: u32, clip: u32) -> Option<DanceClip> {
        let b = self.dance_bodies.as_ref()?;
        let d = b.dancers.get(dancer as usize)?;
        let k = b.cast.kinds.get(d.kind)?;
        match clip {
            0 => Some(k.idle),
            1 => Some(k.dance),
            n => k.moves.get(n as usize - 2).copied(),
        }
    }

    /// The choreography ANM record for dancer `dancer`'s clip slot `clip`:
    /// `(bundle, record_index)`.
    fn dance_anim_record(&self, dancer: u32, clip: u32) -> Option<(&PlayerAnmBundle, usize)> {
        let record = self.dance_clip(dancer, clip)?.record_index()?;
        Some((&self.dance_bodies.as_ref()?.anm, record))
    }

    fn dance_body(&self, dancer: u32) -> Option<&DanceBodyMesh> {
        self.dance_bodies.as_ref()?.dancers.get(dancer as usize)
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the dance's art pack + widget table decoded off this disc.
    /// When `false` the page falls back to its own glyphs - and says so.
    pub fn dance_art_ready(&self) -> bool {
        self.dance_pres.is_some()
    }

    // --------------------------------------------------------- dancer bodies
    //
    // The dance overlay draws no mesh of its own; its spawner (FUN_801d0190)
    // pulls each dancer kind from a baked descriptor table. Noa (the human
    // dancer, centre of the retail floor) is her real field-view model - the
    // same mesh the site's play / field view walks - and the AI dancers are
    // the dance-hall scene module's dedicated dancer NPCs (`other7` scene TMD
    // pool; face-strip rigs 2 and 3 in qualifier mode). The page poses them
    // off the scene's 60-record choreography ANM bundle (PROT 1229): the
    // dance-groove loop synced to the beat clock plus the judge-triggered
    // move clips. This is the browser twin of the Baka Fighter 3D render
    // (`minigames_baka.rs`): same VramMesh accessors, same per-(frame, bone)
    // pose format, so `site/js/minigame-dance.js` drives the shared
    // `TmdRenderer` exactly as `minigame-baka.js` does.

    /// Whether the dance cast (Noa + the dancer NPCs) and the choreography
    /// bundle decoded off this disc.
    pub fn dance_body_ready(&self) -> bool {
        self.dance_bodies.is_some()
    }

    /// Number of dancer bodies (3 on the qualifier floor: left / centre /
    /// right).
    pub fn dance_body_count(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.dancers.len() as u32)
            .unwrap_or(0)
    }

    /// Display index of the human dancer (Noa - the centre of the retail
    /// qualifier floor).
    pub fn dance_body_human_index(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.human as u32)
            .unwrap_or(0)
    }

    /// Dancer `dancer`'s kind descriptor index (0 = Noa, 1 = Mary, 2/3 = the
    /// competitor dancers, 4 = the Disco King) - also the face-stamp rig id
    /// for kinds 0..=3. `255` when out of range.
    pub fn dance_body_kind(&self, dancer: u32) -> u32 {
        self.dance_body(dancer)
            .map(|d| d.kind as u32)
            .unwrap_or(255)
    }

    /// The decoded cast + choreography map, so the page drives retail clips
    /// rather than invented ones:
    ///
    /// ```json
    /// { "human": 1,
    ///   "dancers": [
    ///     { "kind": 2, "model": 62, "x": 5952, "z": 13440,
    ///       "clips": [ { "id": 0, "record": 32, "frames": 20, "rate": 8,
    ///                    "translucent": false }, ... ] }, ... ],
    ///   "moves": { "miss_square": 2, "miss_circle": 3,
    ///              "seq_square": [4, 6, 8], "seq_circle": [5, 7, 9],
    ///              "beat": [10, 11, 12] } }
    /// ```
    ///
    /// Clip ids: `0` = idle (pre-game), `1` = the dance-groove loop, `2 + k` =
    /// judge-triggered move pair `k` (`FUN_801d1af4`'s return, in pair units).
    /// The `moves` map gives the clip id per judge event on each difficulty
    /// lane. `"[]"`-empty when the cast didn't decode.
    pub fn dance_cast_json(&self) -> String {
        let Some(b) = self.dance_bodies.as_ref() else {
            return "null".to_string();
        };
        let dancers = b
            .dancers
            .iter()
            .enumerate()
            .map(|(di, d)| {
                let clips = (0..DANCE_CLIPS as u32)
                    .map(|c| {
                        let clip = self.dance_clip(di as u32, c);
                        let (record, frames, rate, trans) = clip
                            .map(|cl| {
                                let rec = cl.record_index();
                                let frames = rec
                                    .and_then(|r| b.anm.record_lenient(r).ok())
                                    .map(|r| r.frame_count)
                                    .unwrap_or(0);
                                (
                                    rec.map(|r| r as i32).unwrap_or(-1),
                                    frames,
                                    cl.rate,
                                    cl.translucent,
                                )
                            })
                            .unwrap_or((-1, 0, 0, false));
                        format!(
                            r#"{{"id":{c},"record":{record},"frames":{frames},"rate":{rate},"translucent":{trans}}}"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let model = b.cast.kinds.get(d.kind).map(|k| k.model).unwrap_or(0);
                format!(
                    r#"{{"kind":{},"model":{},"x":{},"z":{},"clips":[{}]}}"#,
                    d.kind, model, d.spawn.0, d.spawn.1, clips
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let seq = |circle: bool| {
            (0..3)
                .map(|lane| (2 + dance_cast::move_sequence_pair(lane, circle)).to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let beat = (0..3)
            .map(|lane| (2 + dance_cast::move_beat_pair(lane)).to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                r#"{{"human":{},"dancers":[{}],"moves":{{"miss_square":{},"miss_circle":{},"#,
                r#""seq_square":[{}],"seq_circle":[{}],"beat":[{}]}}}}"#
            ),
            b.human,
            dancers,
            2 + dance_cast::MOVE_MISS_SQUARE,
            2 + dance_cast::MOVE_MISS_CIRCLE,
            seq(false),
            seq(true),
            beat,
        )
    }

    /// Per-vertex positions of dancer `dancer`'s body (object-local; the pose
    /// assembles them). Empty when the bodies didn't decode.
    pub fn dance_body_positions(&self, dancer: u32) -> Vec<f32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.positions.len() * 3);
        for p in &d.mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn dance_body_uvs(&self, dancer: u32) -> Vec<i32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.uvs.len() * 2);
        for uv in &d.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn dance_body_cba_tsb(&self, dancer: u32) -> Vec<u32> {
        let Some(d) = self.dance_body(dancer) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.mesh.cba_tsb.len() * 2);
        for ct in &d.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for dancer `dancer`'s body.
    pub fn dance_body_indices(&self, dancer: u32) -> Vec<u32> {
        self.dance_body(dancer)
            .map(|d| d.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the bone a vertex hangs from), parallel to
    /// the positions - the animator keys `R . v + T` on this.
    pub fn dance_body_object_ids(&self, dancer: u32) -> Vec<u32> {
        self.dance_body(dancer)
            .map(|d| d.object_ids.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
    /// shader path (the field body mixes textured skin with flat body prims).
    pub fn dance_body_flat_rgba(&self, dancer: u32) -> Vec<u8> {
        self.dance_body(dancer)
            .map(|d| d.flat.clone())
            .unwrap_or_default()
    }

    /// TMD object count (pose rig width) of dancer `dancer`'s body.
    pub fn dance_body_part_count(&self, dancer: u32) -> u32 {
        self.dance_body(dancer)
            .map(|d| d.part_count as u32)
            .unwrap_or(0)
    }

    /// `[bone_count, frame_count]` of dancer `dancer`'s clip slot `clip`
    /// (0 = idle, 1 = the dance loop, `2 + k` = move pair `k`). Lenient on
    /// the record-size invariant: several choreography records carry frame
    /// data past the header count that the retail cursor never plays.
    pub fn dance_body_anim_dims(&self, dancer: u32, clip: u32) -> Vec<u32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return vec![0, 0];
        };
        match bundle.record_lenient(record) {
            Ok(r) => vec![r.bone_count as u32, r.frame_count as u32],
            Err(_) => vec![0, 0],
        }
    }

    /// Dancer `dancer`'s clip slot `clip` decoded to absolute per-(frame,
    /// bone) `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
    /// `target_part_count` parts - the same pose stream the site's mesh
    /// animator consumes (identical shape to `baka_anim_pose_frames`).
    pub fn dance_body_pose_frames(
        &self,
        dancer: u32,
        clip: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record_lenient(record) else {
            return Vec::new();
        };
        let bones = rec.bone_count as usize;
        let frames = rec.frame_count as usize;
        let parts = (target_part_count as usize).max(bones);
        let mut out = Vec::with_capacity(frames * parts * 6);
        for f in 0..frames {
            for p in 0..parts {
                if p < bones {
                    let Some(t) = bundle.bone_transform(record, f, p) else {
                        return Vec::new();
                    };
                    out.extend_from_slice(&[t.t_x, t.t_y, t.t_z, t.r_x, t.r_y, t.r_z]);
                } else {
                    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
                }
            }
        }
        out
    }

    /// The 1 MB PSX VRAM the dancer bodies sample: the dance-hall scene's
    /// full TIM upload (the dancer NPC atlases + their row-480/481 CLUTs)
    /// merged with the PROT 0874 §2 field-character textures (Noa's atlas,
    /// row-478 CLUTs). Empty when the cast didn't decode.
    pub fn dance_body_vram(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.vram.clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------ the dance hall
    //
    // Retail draws the dance minigame inside the host field scene: the hall -
    // the raised stage, the yellow/black checkered dance floor, the portrait
    // banners on the walls, the spotlight cones, the speaker and lamp
    // fixtures - is the `other7` scene's own environment pack instanced by
    // its `.MAP` placement / terrain layers, textured by the same scene VRAM
    // the dancer bodies already sample. These accessors hand the page that
    // map as ONE static baked mesh in the dancer frame (the human spawn at
    // the origin, retail Y-down coordinates), so the page appends it to the
    // combined vertex buffer as unposed geometry behind the cast.

    /// Baked hall vertex positions (`[x, y, z, ...]`, dancer frame). Empty
    /// when the scene's placement layers didn't resolve - the page then keeps
    /// the neutral ground and says so.
    pub fn dance_env_positions(&self) -> Vec<f32> {
        self.dance_bodies
            .as_ref()
            .filter(|b| !b.env.is_empty())
            .map(|b| b.env.positions.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` texel coords for the baked hall.
    pub fn dance_env_uvs(&self) -> Vec<i32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.uvs.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]` for the baked hall.
    pub fn dance_env_cba_tsb(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.cba_tsb.clone())
            .unwrap_or_default()
    }

    /// Triangle indices for the baked hall.
    pub fn dance_env_indices(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the baked hall's hybrid
    /// textured / vertex-colour render (same convention as the bodies).
    pub fn dance_env_flat_rgba(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.flat.clone())
            .unwrap_or_default()
    }

    /// The 256x256 HUD page (VRAM `(512, 0)`) decoded through palette
    /// `palette` of its own row-500 CLUT strip, as RGBA8. Palette selection
    /// is load-bearing: the widget table names a palette per element, and
    /// the beat-track flash / note tint are pure CLUT swaps over the same
    /// texels (`0x7D08` idle / `0x7D0D` flash / `0x7D0E` notes).
    pub fn dance_hud_page_rgba(&self, palette: usize) -> Vec<u8> {
        self.dance_pres
            .as_ref()
            .and_then(|p| dance_art::hud_page_rgba(&p.art, palette).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The overlay's HUD widget table, one record per id `0..=33`:
    ///
    /// ```json
    /// [ { "u":0, "v":0, "w":16, "h":24, "palette":0,
    ///     "semi":0, "top":[255,255,255], "bottom":[255,255,255] }, ... ]
    /// ```
    ///
    /// Cells index the HUD page; `palette` is the row-500 CLUT column the
    /// record names (the emitters swap it at runtime for the flash states).
    pub fn dance_widgets_json(&self) -> String {
        let Some(p) = self.dance_pres.as_ref() else {
            return "[]".to_string();
        };
        let rows = p
            .widgets
            .iter()
            .map(|w| {
                format!(
                    concat!(
                        r#"{{"u":{},"v":{},"w":{},"h":{},"palette":{},"semi":{},"#,
                        r#""top":[{},{},{}],"bottom":[{},{},{}]}}"#
                    ),
                    w.u,
                    w.v,
                    w.w,
                    w.h,
                    w.palette_index(),
                    w.semi,
                    w.rgb_top[0],
                    w.rgb_top[1],
                    w.rgb_top[2],
                    w.rgb_bottom[0],
                    w.rgb_bottom[1],
                    w.rgb_bottom[2],
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// The traced HUD geometry, so the page draws at retail positions rather
    /// than invented ones. Everything here is an immediate in a traced
    /// emitter (`FUN_801d231c` / `FUN_801d2524` / `FUN_801d32f8` /
    /// `FUN_801d3e28` and the banner spawn sites in `FUN_801cf470` /
    /// `FUN_801d1af4` / `FUN_801d40dc`), on the retail 320x240 frame.
    /// Widgets draw **centred** on their `(x, y)`.
    ///
    /// - `score_boxes`: the three boxes; the **human dancer is the centre
    ///   box** (`FUN_801d231c` draws score slot 0 at the centre digit base).
    /// - `digit_bases`: x of digit slot 0 per box; 8 slots step 16, only
    ///   significant digits draw, so a 1-digit score lands at `base + 112`.
    /// - `track`: the beat lane - arrow, caps, 12 body tiles, the scrolling
    ///   notes (`x = track.x + i*16 - (phase*16/281 + 5) - 4`, clip window
    ///   `[track.x, track.x + 0x50)`), stock markers at `y + 16`.
    /// - `banners`: spawn points (`FUN_801d3fd0` stores `x<<3` and draws at
    ///   `>>3`): countdown / READY / GO / FINISH at centre, ratings below,
    ///   stars flanking by tier (`0x38`/`0x50` for Cool/Great).
    /// `screen_offset` is the global drawing-environment offset: every HUD
    /// element in the retail VRAM capture (score-box border, track pill,
    /// marker arrow) sits exactly 4 lines below the emitter's own `y`, so the
    /// active draw environment carries a `+4` Y offset. Pixel-pinned against
    /// the parked minigame capture.
    pub fn dance_layout_json(&self) -> String {
        concat!(
            r#"{"screen":[320,240],"screen_offset":[0,4],"#,
            r#""score_boxes":{"xs":[64,160,256],"y":20,"human":1},"#,
            r#""digit_bases":{"xs":[-32,64,160],"y":20,"step":16,"slots":8},"#,
            r#""gauge":{"lv_x":88,"digit_x":96,"y":192},"#,
            r#""track":{"x":120,"y":192,"arrow":[128,184],"cap_l":116,"cap_r":204,"#,
            r#""body_tiles":12,"body_step":8,"clip_w":80,"note_step":16,"#,
            r#""stock_y":208,"stock_step":16},"#,
            r#""banners":{"centre":[160,120],"miss":[160,128],"rating":[160,144],"#,
            r#""star_off":{"cool":56,"great":80,"fever":80},"good_star_off":56},"#,
            r#""flash":{"beat_mask":3,"phase_lt":70}}"#
        )
        .to_string()
    }

    /// One dancer's live face window as RGBA8: the strip's top window with
    /// pose `pose` stamped in by the two traced `MoveImage` blits
    /// (`FUN_801d03c4`). `dancer` is the rig index `0..=3`: `0` = **Noa**
    /// (her field atlas, PROT 0874 §2), `1..=3` = the pack strips. Pair with
    /// [`Self::dance_face_meta_json`] for dimensions. Empty when the strip
    /// didn't decode.
    pub fn dance_face_rgba(&self, dancer: usize, pose: usize) -> Vec<u8> {
        let Some(p) = self.dance_pres.as_ref() else {
            return Vec::new();
        };
        let Some(rig) = dance_art::FACE_RIGS.get(dancer) else {
            return Vec::new();
        };
        let frames = match p.face_frames.get(dancer) {
            Some(f) if !f.is_empty() => f,
            _ => return Vec::new(),
        };
        let strip = if dancer == 0 {
            p.noa_atlas.as_ref()
        } else {
            dance_art::pack_strip(&p.art, rig)
        };
        let Some(strip) = strip else {
            return Vec::new();
        };
        dance_art::face_window_rgba(strip, rig, frames, pose, 0, 64)
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// Face window metadata:
    /// `[{ "w":80, "h":64, "face":[0,0,32,48], "poses":5 }, ...]` - `w`/`h`
    /// are the buffer dimensions [`Self::dance_face_rgba`] returns, `face`
    /// the sub-rect that is the visible face (the rest of the window is
    /// neighbouring atlas cells).
    pub fn dance_face_meta_json(&self) -> String {
        let Some(p) = self.dance_pres.as_ref() else {
            return "[]".to_string();
        };
        let rows = dance_art::FACE_RIGS
            .iter()
            .enumerate()
            .map(|(i, rig)| {
                let strip = if i == 0 {
                    p.noa_atlas.as_ref()
                } else {
                    dance_art::pack_strip(&p.art, rig)
                };
                let (w, ok) = match strip {
                    Some(t) => (t.pixel_width(), !p.face_frames[i].is_empty()),
                    None => (0, false),
                };
                // The visible face sub-rect: Noa's window is the 32x48
                // top-left of her 80px atlas; the pack strips are 64x64.
                let face = if i == 0 {
                    [0, 0, 32, 48]
                } else {
                    [0, 0, 64, 64]
                };
                format!(
                    r#"{{"ok":{},"w":{},"h":64,"face":[{},{},{},{}],"poses":{}}}"#,
                    ok, w, face[0], face[1], face[2], face[3], rig.poses
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    // ------------------------------------------------------------- dance sound

    /// The dance's own cue bank (descriptors PROT 1228, samples PROT 1231):
    /// `[{ "id":528, "program":0, "tone":1, "note":66, "rate":44100 }, ...]`.
    /// Empty when either entry didn't decode - PROT 1231 sits in the PROT
    /// TOC's zeroed tail, so an image whose TOC truncates early loses it.
    pub fn dance_sfx_json(&self) -> String {
        let Some(bank) = self.dance_pres.as_ref().and_then(|p| p.sfx.as_ref()) else {
            return "[]".to_string();
        };
        let rows = bank
            .cues()
            .iter()
            .map(|c| {
                let rate = bank.decode(c.id).map(|(_, r)| r).unwrap_or(0);
                format!(
                    r#"{{"id":{},"program":{},"tone":{},"note":{},"rate":{}}}"#,
                    c.id, c.program, c.tone, c.note, rate
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// Decode one dance cue to mono PCM (`i16`). Empty when absent.
    pub fn dance_sfx_pcm(&self, cue: u16) -> Vec<i16> {
        self.dance_pres
            .as_ref()
            .and_then(|p| p.sfx.as_ref())
            .and_then(|b| b.decode(cue).ok())
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::dance_sfx_pcm`] (`0` when absent).
    pub fn dance_sfx_rate(&self, cue: u16) -> u32 {
        self.dance_pres
            .as_ref()
            .and_then(|p| p.sfx.as_ref())
            .and_then(|b| b.decode(cue).ok())
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }

    /// The retail cue ids (`FUN_801d1af4` sites): miss, the three combo-tier
    /// stings, the run-start and intro cues.
    pub fn dance_sfx_cue_ids(&self) -> String {
        format!(
            r#"{{"miss":{},"cool":{},"great":{},"fever":{},"start":{},"intro":{}}}"#,
            dance_art::CUE_DANCE_MISS,
            dance_art::CUE_DANCE_COOL,
            dance_art::CUE_DANCE_GREAT,
            dance_art::CUE_DANCE_FEVER,
            dance_art::CUE_DANCE_START,
            dance_art::CUE_DANCE_INTRO,
        )
    }

    /// One layer of a good-step **hit sting**. Retail keys these directly
    /// (`FUN_801d3d78`, bypassing the cue ring): a step picks `r = rand() % 3`
    /// and keys VAB program 1 tones `2r` (layer 0) and `2r + 1` (layer 1)
    /// together at note `0x3C + r`. Mono i16 PCM; empty when absent.
    pub fn dance_sting_pcm(&self, r: u8, layer: u8) -> Vec<i16> {
        self.dance_sting(r, layer)
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::dance_sting_pcm`] (`0` when absent).
    pub fn dance_sting_rate(&self, r: u8, layer: u8) -> u32 {
        self.dance_sting(r, layer).map(|(_, r)| r).unwrap_or(0)
    }

    // --------------------------------------------------------------- dance BGM

    /// Whether the dance BGM pair (VAB + SEQ in one `music_01` entry)
    /// resolves: `{"ok":true,"prot":1048,"alt":true}`. The overlay starts one
    /// of two songs by mode (`FUN_801cf470` state 6 branches on the mode
    /// global); `alt = false` picks extraction 1048, `true` picks 1054.
    pub fn dance_bgm_ready_json(&self) -> String {
        let probe = |idx: usize| {
            entry_bytes(&self.prot, &self.entries, idx as u32)
                .and_then(|buf| {
                    let vab = buf.windows(4).position(|w| w == b"pBAV")?;
                    buf[vab..].windows(4).position(|w| w == b"pQES")?;
                    Some(())
                })
                .is_some()
        };
        format!(
            r#"{{"ok":{},"prot":{},"alt":{}}}"#,
            probe(dance_art::DANCE_BGM_PROT_INDEX),
            dance_art::DANCE_BGM_PROT_INDEX,
            probe(dance_art::DANCE_BGM_ALT_PROT_INDEX),
        )
    }

    /// Render `seconds` of the dance BGM to interleaved stereo i16 PCM at
    /// [`Self::dance_bgm_rate`], through the clean-room SPU + sequencer -
    /// the same path the audio page uses. Empty when the pair didn't decode.
    pub fn dance_bgm_pcm_i16(&self, alt: bool, seconds: f32) -> Vec<i16> {
        let idx = if alt {
            dance_art::DANCE_BGM_ALT_PROT_INDEX
        } else {
            dance_art::DANCE_BGM_PROT_INDEX
        };
        let Some(buf) = entry_bytes(&self.prot, &self.entries, idx as u32) else {
            return Vec::new();
        };
        let Some(vab_off) = buf.windows(4).position(|w| w == b"pBAV") else {
            return Vec::new();
        };
        let Some(seq_rel) = buf[vab_off..].windows(4).position(|w| w == b"pQES") else {
            return Vec::new();
        };
        let Ok(vab_report) = legaia_vab::parse(buf, vab_off) else {
            return Vec::new();
        };
        let Ok(seq) = legaia_seq::Seq::parse(&buf[vab_off + seq_rel..]) else {
            return Vec::new();
        };
        let mut spu = legaia_engine_audio::Spu::new();
        let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(0x1000, 0x40_000);
        let bank = legaia_engine_audio::VabBank::upload(
            &mut spu,
            &mut alloc,
            &vab_report,
            &buf[vab_off..],
        );
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        let samples =
            (seconds.clamp(1.0, 120.0) * legaia_engine_audio::SPU_INTERNAL_RATE as f32) as usize;
        legaia_engine_audio::render_bgm_to_pcm(&mut sequencer, &mut spu, samples)
    }

    /// Sample rate of [`Self::dance_bgm_pcm_i16`] (the SPU's 44.1 kHz).
    pub fn dance_bgm_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }
}

impl LegaiaMinigames {
    /// Decode one sting layer: program 1, tone `2r + layer`, keyed at note
    /// `0x3C + r` against the tone's own centre note.
    fn dance_sting(&self, r: u8, layer: u8) -> Option<(Vec<i16>, u32)> {
        if r > 2 || layer > 1 {
            return None;
        }
        let (report, bytes) = self
            .dance_pres
            .as_ref()
            .and_then(|p| p.sting_vab.as_ref())?;
        let atr = report.tones.get(1)?.get((r * 2 + layer) as usize)?;
        if atr.vag <= 0 {
            return None;
        }
        let span = report.vag_samples.get(atr.vag as usize - 1)?;
        let body = bytes.get(span.byte_offset..span.byte_offset + span.size)?;
        let pcm = legaia_vab::decode_vag_aligned(body).ok()?;
        let semitones = (0x3C + r) as f64 - atr.center as f64;
        let rate = (44100.0 * 2f64.powf(semitones / 12.0)).round();
        Some((pcm, rate.clamp(4000.0, 96_000.0) as u32))
    }
}
