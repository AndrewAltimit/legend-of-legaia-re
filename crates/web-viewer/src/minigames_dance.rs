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

use legaia_asset::dance_art::{self, DanceWidget};
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_asset::{character_pack, field_char_textures};
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

/// One dancer's renderable field-form body, built once at disc load.
pub(crate) struct DanceBodyMesh {
    mesh: VramMesh,
    /// Per-vertex TMD object index (the bone each vertex hangs from).
    object_ids: Vec<u32>,
    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid shader.
    flat: Vec<u8>,
    /// TMD object count = the pose rig width (capped to the retail 10).
    part_count: usize,
    /// PROT 0874 §0 pack slot the body came from (0 Vahn / 1 Noa / 2 Gala) -
    /// also the locomotion-bank index for the pose lookup.
    char_slot: usize,
}

/// The three dancer bodies + the shared pose bank + the field VRAM they
/// sample, decoded once at disc load. The dance overlay (PROT 0980) loads no
/// mesh of its own; the dancers are actors drawn from the field-form character
/// pool the engine keeps resident across every field scene
/// (`DAT_8007C018[0..4]` = PROT 0874 §0). Noa (the human dancer, pinned by the
/// rig-0 face stamp = her field atlas, `docs/subsystems/minigame-dance.md`) is
/// pack slot 1; the two AI dancers are drawn from the other resident party
/// meshes (Vahn slot 0, Gala slot 2) - the only character bodies guaranteed
/// present with the overlay issuing no load. Poses come from the party
/// field-locomotion ANM bundle (PROT 0874 §1), the same clip set the field /
/// play view animates the pool actors with.
pub(crate) struct DanceBodies {
    /// Display order left / centre / right = `[Vahn, Noa(human), Gala]`.
    dancers: Vec<DanceBodyMesh>,
    /// The party field-locomotion ANM bundle (idle = bank slot 1, walk = 0).
    anm: PlayerAnmBundle,
    /// 1 MB PSX VRAM with the PROT 0874 §2 field-character textures uploaded.
    vram: Vec<u8>,
}

/// The centre dancer - the human player (Noa). Index into [`DanceBodies::dancers`].
const DANCE_HUMAN_INDEX: usize = 1;

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

impl LegaiaMinigames {
    /// Build one dancer's field-form body mesh from PROT 0874 §0 pack slot
    /// `char_slot`. Mirrors the viewer's field-character build
    /// (`crate::character` / `crate::field_npc`): the active-party slots are
    /// capped to the retail 10 live groups (groups 10/11 are the equipment
    /// templates, never drawn) and the mesh is the field **hybrid** build so
    /// the textured skin prims and the flat-shaded body prims both render.
    fn build_dance_body(&self, char_slot: usize) -> Option<DanceBodyMesh> {
        let raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX)?;
        let pack = character_pack::parse(raw).ok()?;
        let cslot = pack.slot(char_slot)?;
        let mut tmd_bytes = cslot.tmd_bytes.clone();
        if cslot.is_active_party() && tmd_bytes.len() >= 0x0C {
            // Overwrite the TMD header `nobj` to 10 - the retail cap
            // (FUN_8001E890), so the equip-template groups aren't drawn.
            tmd_bytes[0x08..0x0C].copy_from_slice(&10u32.to_le_bytes());
        }
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        let part_count = tmd.objects.len();
        let (mesh, object_ids, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, &tmd_bytes);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        Some(DanceBodyMesh {
            mesh,
            object_ids,
            flat,
            part_count,
            char_slot,
        })
    }

    /// Decode the three dancer bodies + their shared pose bank + the field
    /// VRAM off the loaded PROT bytes. `None` when PROT 0874 (the field
    /// character pool) doesn't decode.
    pub(crate) fn load_dance_bodies(&mut self) -> Option<DanceBodies> {
        let raw = entry_bytes(&self.prot, &self.entries, character_pack::PROT_ENTRY_INDEX)?;
        let anm = character_pack::field_locomotion_anm(raw).ok()?;
        let vram = {
            let pack = field_char_textures::parse(raw).ok()?;
            let mut v = legaia_tim::Vram::new();
            pack.upload_to_vram(&mut v, false);
            v.as_bytes().to_vec()
        };
        // Display order left / centre / right; the centre is the human (Noa).
        let order = [0usize, 1, 2];
        let mut dancers = Vec::with_capacity(order.len());
        for &cs in &order {
            dancers.push(self.build_dance_body(cs)?);
        }
        Some(DanceBodies { dancers, anm, vram })
    }

    /// The locomotion ANM record for dancer `dancer`'s `clip` (0 = idle, else
    /// walk): `(bundle, record_index)`.
    fn dance_anim_record(&self, dancer: u32, clip: u32) -> Option<(&PlayerAnmBundle, usize)> {
        let b = self.dance_bodies.as_ref()?;
        let d = b.dancers.get(dancer as usize)?;
        let bank_slot = if clip == 0 {
            character_pack::LOCOMOTION_IDLE_SLOT
        } else {
            character_pack::LOCOMOTION_WALK_SLOT
        };
        let record = character_pack::locomotion_record_index(d.char_slot, bank_slot);
        Some((&b.anm, record))
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
    // The dance overlay draws no mesh of its own; its dancers are field-scene
    // actors drawn from the resident field-character pool (PROT 0874 §0). Noa
    // (the human dancer, centre) is her real field-view model - the same mesh
    // the site's play / field view walks - and the two AI dancers are the
    // other resident party bodies (Vahn / Gala). The page poses them to the
    // beat off the party field-locomotion ANM (PROT 0874 §1), the same bank
    // the field pool actors animate with. This is the browser twin of the
    // Baka Fighter 3D render (`minigames_baka.rs`): same VramMesh accessors,
    // same per-(frame, bone) pose format, so `site/js/minigame-dance.js`
    // drives the shared `TmdRenderer` exactly as `minigame-baka.js` does.

    /// Whether the three dancer bodies (Noa's field mesh + the two AI dancers)
    /// and their pose bank decoded off this disc.
    pub fn dance_body_ready(&self) -> bool {
        self.dance_bodies.is_some()
    }

    /// Number of dancer bodies (3: left / centre / right).
    pub fn dance_body_count(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.dancers.len() as u32)
            .unwrap_or(0)
    }

    /// Display index of the human dancer (the centre box = Noa).
    pub fn dance_body_human_index(&self) -> u32 {
        DANCE_HUMAN_INDEX as u32
    }

    /// The PROT 0874 §0 pack slot dancer `dancer` is drawn from
    /// (0 = Vahn, 1 = Noa, 2 = Gala). `255` when out of range.
    pub fn dance_body_char_slot(&self, dancer: u32) -> u32 {
        self.dance_body(dancer)
            .map(|d| d.char_slot as u32)
            .unwrap_or(255)
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

    /// `[bone_count, frame_count]` of dancer `dancer`'s `clip` locomotion
    /// record (`clip` 0 = idle bank slot, else the walk bank slot).
    pub fn dance_body_anim_dims(&self, dancer: u32, clip: u32) -> Vec<u32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return vec![0, 0];
        };
        match bundle.record(record) {
            Ok(r) => vec![r.bone_count as u32, r.frame_count as u32],
            Err(_) => vec![0, 0],
        }
    }

    /// Dancer `dancer`'s `clip` locomotion record decoded to absolute
    /// per-(frame, bone) `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles),
    /// padded to `target_part_count` parts - the same pose stream the site's
    /// mesh animator consumes (identical shape to `baka_anim_pose_frames`).
    pub fn dance_body_pose_frames(
        &self,
        dancer: u32,
        clip: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some((bundle, record)) = self.dance_anim_record(dancer, clip) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record(record) else {
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

    /// The 1 MB PSX VRAM the dancer bodies sample - the PROT 0874 §2
    /// field-character textures (row-478 CLUTs), uploaded exactly as the field
    /// / play view uploads them. Empty when the bodies didn't decode.
    pub fn dance_body_vram(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.vram.clone())
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
