//! Dance-minigame **presentation** methods of [`LegaiaMinigames`] - the
//! retail HUD art, the dancer face-stamp, the SFX cue bank and the BGM,
//! all decoded from the visitor's own disc at load time.
//!
//! The dance overlay (PROT 0980) draws its whole HUD through one emitter
//! (`FUN_801d2f38`) over a 34-record widget table in its own rodata; the art
//! those widgets sample is the PROT 1230 TIM pack the mode-24 entry path
//! stages at VRAM `(512, 0)` (see [`legaia_minigame_formats::dance_art`] and
//! `docs/subsystems/minigame-dance.md`). This module hands the page the same
//! table, the same page, and the traced emitter geometry, so the JS side
//! never invents a rect.

use super::*;

use std::collections::HashMap;

use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_actor_formats::{character_pack, field_char_textures};
use legaia_engine_minigame_scenes::dance_venue::{self, DanceVenue};
use legaia_engine_minigames::dance::dance_hit_sting_voices;
use legaia_engine_scene::field_env;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::SceneResources;
use legaia_minigame_formats::dance_art::{self, DanceWidget};
use legaia_minigame_formats::dance_cast::{self, DanceCast, DanceClip};
use legaia_tmd::mesh::{VramMesh, tmd_to_vram_mesh_field_hybrid};

mod body_exports;
mod cast_scene_frame;
mod hud_exports;
mod markers;
mod sound_exports;
mod venue_exports;
pub(crate) use markers::DanceMarkers;
use markers::bake_dance_markers;

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
    /// Dancer-kind descriptor index (`legaia_minigame_formats::dance_cast`; kind = the
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

impl DanceBodies {
    /// The dance hall's choreography bank, for
    /// [`DanceGame::attach_clip_bank`](legaia_engine_minigames::dance::DanceGame::attach_clip_bank).
    pub(crate) fn clip_bank(&self) -> &PlayerAnmBundle {
        &self.anm
    }
}

impl DanceEnv {
    /// Append one env-pack mesh instanced at an [`field_env::EnvDraw`],
    /// re-based to `origin` (the human dancer's spawn). The transform is the
    /// retail placement composition in the Y-down world frame: rotate the
    /// object-local vertices by the record's three authored angles
    /// (`Rx * Ry * Rz`, [`field_env::EnvDraw::place_point`] - the
    /// composition the native window bakes its venue with), then translate
    /// to the draw's world position plus its coplanar lift. Yaw alone is not
    /// enough here: the hall's three video walls are tilted placements, and
    /// a yaw-only bake stood them upright inside the back wall, where the
    /// wall's own faces hid every texel - the page drew bare brick where the
    /// native window draws the dancer screens.
    fn append_draw(
        &mut self,
        mesh: &VramMesh,
        flat: &[u8],
        draw: &field_env::EnvDraw,
        origin: (f32, f32, f32),
        lift: [f32; 3],
    ) {
        let base = (self.positions.len() / 3) as u32;
        for p in &mesh.positions {
            let [x, y, z] = draw.place_point(*p, lift);
            self.positions
                .extend_from_slice(&[x - origin.0, y - origin.1, z - origin.2]);
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
            // Pure-textured mesh: every vertex samples VRAM. The modulation
            // triple is the **neutral** byte, not white - the shader reads
            // `a_flat_rgba` as `texel * rgb * 255/128`, so a white stream is
            // `texel * 2` and the mesh reads over-lit rather than as having
            // lost its colour word. (In practice this branch is unreachable:
            // `packet_color::textured` walks `mesh.colors`, which is parallel
            // to `mesh.positions`, so an empty `flat` means an empty mesh.)
            let neutral = [
                legaia_engine_field::packet_color::NEUTRAL,
                legaia_engine_field::packet_color::NEUTRAL,
                legaia_engine_field::packet_color::NEUTRAL,
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

/// A world-frame camera as the view-projection of a page frame re-based on
/// `origin` in retail Y-down coordinates: the focus moves with the frame and
/// the Y column negates, which is the single flip the page's own orbit
/// framing applies.
pub(crate) fn dance_venue_vp_in_frame(
    camera: &legaia_engine_camera::psx_camera::FieldCameraView,
    origin: (f32, f32, f32),
    aspect: f32,
) -> [f32; 16] {
    let v = legaia_engine_camera::psx_camera::FieldCameraView {
        focus: [
            camera.focus[0] - origin.0,
            camera.focus[1] - origin.1,
            camera.focus[2] - origin.2,
        ],
        ..*camera
    };
    let mut m = v.vp(aspect);
    for x in &mut m[4..8] {
        *x = -*x;
    }
    m
}

/// Bake the venue's full static map into one [`DanceEnv`] mesh: the
/// kernel's ranked terrain + placement draw list
/// ([`legaia_engine_minigame_scenes::dance_venue::DanceVenue::draws`], the same
/// [`field_env`] resolution and coplanar lifts every host draws the venue
/// with; bound props posed at frame 0 of their clip) + the walk-ground
/// heightfield, every draw transformed to world space and re-based on
/// `origin`.
fn bake_dance_env(index: &ProtIndex, venue: &DanceVenue, origin: (f32, f32, f32)) -> DanceEnv {
    let scene = &venue.scene;
    let res = &venue.resources;
    let mut out = DanceEnv::default();
    let mut built: HashMap<(usize, u8), (VramMesh, Vec<u8>)> = HashMap::new();
    for vd in &venue.draws {
        let draw = &vd.draw;
        let Some(rtmd) = res.tmds.get(draw.res_tmd) else {
            continue;
        };
        let key = (draw.env_slot, draw.anim_id);
        let entry = built.entry(key).or_insert_with(|| {
            let offsets = (draw.anim_id != 0)
                .then(|| venue.frame0_bone_offsets(draw.anim_id, rtmd.tmd.objects.len()))
                .flatten();
            match &offsets {
                Some(o) => {
                    legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_posed(rtmd, o)
                }
                None => legaia_engine_scene::scene_assembly::build_hybrid_env_mesh(rtmd, &res.vram),
            }
        });
        let (mesh, flat) = (&entry.0, &entry.1);
        out.append_draw(mesh, flat, draw, origin, vd.lift);
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
            // Ground sinks below the env floor art (`GROUND_SINK`) - the
            // stage floor and the walk grid share one plane.
            out.positions
                .push(p[1] + legaia_engine_scene::coplanar_draws::GROUND_SINK - origin.1);
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
        // The heightfield carries its own per-vertex modulation triple
        // (`field_objects::GROUND_PRIM_COLOR`, the neutral byte), parallel to
        // its positions - use it rather than fabricating a stream, so the one
        // disc-derived fact has one home.
        for c in &hf.colors {
            out.flat.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
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
/// [`legaia_minigame_formats::dance_cast`].
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
    /// The floor's step-marker tiles and their flipbooks (empty when the
    /// venue's `.MAP` carries no clip-`6..=9` cell).
    markers: DanceMarkers,
    /// Did the HUD's own texture page land in [`Self::vram`]? The page states
    /// this through `LegaiaRuntime::play_mg_dance_hud_art_staged`, and it is
    /// what decides - on both hosts, from one predicate - whether the
    /// count-in banner draws as retail's sprite or as placeholder text.
    hud_staged: bool,
    /// The world point every baked position is re-based on (the human
    /// dancer's floor spawn), for [`LegaiaMinigames::dance_venue_vp`].
    origin: (f32, f32, f32),
    /// The dance entry's camera ([`dance_venue::venue_camera`]), in world
    /// coordinates.
    camera: legaia_engine_camera::psx_camera::FieldCameraView,
    /// The floor's body meshes + choreography bank for the engine's cast
    /// surface ([`legaia_engine_minigame_scenes::dance_cast_scene`]), which the play page
    /// poses the run's own cast through - whatever mode it is.
    cast_assets:
        Option<std::sync::Arc<legaia_engine_minigame_scenes::dance_cast_scene::DanceCastAssets>>,
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
            legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
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
        // The audio bank the dance entry streams (`FUN_8001FC00` with raw
        // `0x4D1`), read off the entry record rather than a second constant.
        let vab_entry = entry_bytes(
            &self.prot,
            &self.entries,
            dance_venue::venue_sfx_vab_index(&legaia_engine_minigames::dance::dance_scene_entry()),
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
    let flat = legaia_engine_field::packet_color::hybrid(&mesh, &shading);
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
    /// `camera` as a column-major view-projection over this page's baked
    /// hall frame. The play page frames its hall through the world's staged
    /// dance camera with this; the standalone page through its own run's
    /// track ([`Self::dance_venue_vp`]). Empty when the cast did not decode.
    pub fn dance_venue_vp_with(
        &self,
        camera: &legaia_engine_camera::psx_camera::FieldCameraView,
        aspect: f32,
    ) -> Vec<f32> {
        let Some(b) = self.dance_bodies.as_ref() else {
            return Vec::new();
        };
        dance_venue_vp_in_frame(camera, b.origin, aspect).to_vec()
    }

    /// The baked hall's triangles the PSX GPU draws under `camera`
    /// ([`dance_venue::psx_gpu_visible_indices`], the kernel the native
    /// window cuts its hall with): [`Self::dance_env_indices`] less every
    /// triangle whose screen span the GPU refuses. Empty with no hall.
    pub fn dance_env_visible_indices_with(
        &self,
        camera: &legaia_engine_camera::psx_camera::FieldCameraView,
    ) -> Vec<u32> {
        let Some(b) = self.dance_bodies.as_ref().filter(|b| !b.env.is_empty()) else {
            return Vec::new();
        };
        let positions: Vec<[f32; 3]> = b.env.positions.as_chunks::<3>().0.to_vec();
        let (ox, oy, oz) = b.origin;
        dance_venue::psx_gpu_visible_indices(camera, &positions, [ox, oy, oz], &b.env.indices)
    }

    /// Noa's body - the overlay spawns dancer kind 0 from the resident global
    /// TMD pool (slot 1 = PROT 0874 §0 pack slot 1, her field-view mesh; the
    /// spawner writes that model id *without* the scene-pool base). Mirrors
    /// the viewer's field-character build: the active-party TMD is capped to
    /// the retail 10 live groups (groups 10/11 are the equipment templates,
    /// never drawn - FUN_8001E890).
    fn build_noa_body(&self, spawn: (i16, i16)) -> Option<DanceBodyMesh> {
        let raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX)?;
        let tmd_bytes = legaia_engine_minigame_scenes::dance_cast_scene::resident_body_tmd(raw, 1)?;
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
            legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
        )?;
        let cast = dance_cast::parse(&overlay)?;

        // The dance-hall venue, through the kernel the native window loads it
        // with: the dance entry's record names the block (raw base `0x4CC`)
        // and bounds it at its audio bank (raw `0x4D1`), so a two-define map
        // built off the record frames it without the full CDNAME.TXT (this
        // class only holds PROT bytes). Bounding it there also keeps the block
        // off the PROT TOC's zeroed tail, where the indexed size formula
        // underflows to a ~4 GiB footprint - a reserve past `isize::MAX` on
        // wasm32. The venue build also puts Noa's field atlas, the HUD page
        // and the entry's five face stamps into its VRAM. The ProtIndex clone
        // is dropped again before this function returns.
        let entry = legaia_engine_minigames::dance::dance_scene_entry();
        let index = ProtIndex::from_bytes(
            self.prot.clone(),
            Some(&dance_venue::venue_cdname_stub(&entry)),
        )
        .ok()?;
        let venue = DanceVenue::build(&index, Some(&overlay))?;
        let cast_assets =
            legaia_engine_minigame_scenes::dance_cast_scene::DanceCastAssets::from_venue(
                &venue,
                &cast,
                entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX),
            )
            .map(std::sync::Arc::new);
        let res = &venue.resources;
        // The scene's MOVE ANM bundle - the 60-record choreography bank.
        let anm = venue.anm.clone()?;
        let vram = &res.vram;
        let hud_staged = venue.hud_staged;

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
        let origin = (hs.x as f32, hs.y as f32, hs.z as f32);
        let env = bake_dance_env(&index, &venue, origin);

        // The step-marker tiles that stand on that floor. Same origin, so
        // they land in the frame the bodies and the hall already share.
        let markers = bake_dance_markers(&index, &venue.scene, res, &overlay, origin);

        Some(DanceBodies {
            dancers,
            human,
            anm,
            cast,
            vram: vram.as_bytes().to_vec(),
            env,
            markers,
            hud_staged,
            origin,
            camera: venue.camera,
            cast_assets,
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

    /// The engine cast surface's assets off this disc's venue, and the world
    /// point the page's baked hall is re-based on.
    pub fn dance_cast_assets(
        &self,
    ) -> Option<(
        std::sync::Arc<legaia_engine_minigame_scenes::dance_cast_scene::DanceCastAssets>,
        (f32, f32, f32),
    )> {
        let b = self.dance_bodies.as_ref()?;
        Some((b.cast_assets.clone()?, b.origin))
    }

    fn dance_body(&self, dancer: u32) -> Option<&DanceBodyMesh> {
        self.dance_bodies.as_ref()?.dancers.get(dancer as usize)
    }
}
