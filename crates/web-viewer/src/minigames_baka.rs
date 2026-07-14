//! Baka Fighter **presentation** exports for `LegaiaMinigames` - everything
//! the site's duel draws with, decoded from the visitor's own disc:
//!
//! * the two fighters' real 3D meshes - the player side out of the battle-form
//!   party pack (PROT 1204, `legaia_asset::battle_char_pack`), the opponent out
//!   of its own per-rung pack (PROT `1206..=1219`,
//!   [`legaia_asset::baka_opponents::parse_fighter_pack`]);
//! * their animation banks - the player's from the PROT 1203 battle-form ANM
//!   bank (records `char*9 + action`, per `docs/formats/character-mesh.md`),
//!   the opponent's from its own pack's anim chunk (canonical ANM records,
//!   `bone_count == nobj`, disc-gated in `baka_presentation_real.rs`);
//! * the HUD widget descriptor table (`DAT_801d7160`, 51 records - the
//!   "PRESS START" / "ROUND" / "FIGHT!" / "YOU WIN!" cells, the stage digit,
//!   the pips and combo glyph cells) plus the 9-TIM art pack (PROT 1203) the
//!   widgets sample;
//! * the 4-TMD stage set (PROT 1203 descriptor 1).
//!
//! Everything decodes at call time from `self.prot` - no cached state, no
//! pixel shipped with the site. When an asset does not decode, the page names
//! ids instead of inventing art (same contract as the slot machine section).

use super::*;

use legaia_asset::baka_opponents::{self as baka};
use legaia_asset::{DecodeMode, decode as decode_descriptor, pack as asset_pack, parse_player_lzs};
use legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid;

/// Records per character bank in the PROT 1203 battle-form ANM bundle.
const BANK_RECORDS_PER_CHAR: usize = 9;

/// A fighter's decoded render bundle, built per call.
struct FighterMesh {
    mesh: legaia_tmd::mesh::VramMesh,
    object_ids: Vec<u32>,
    flat: Vec<u8>,
    part_count: usize,
}

impl LegaiaMinigames {
    fn baka_entry(&self, prot_index: usize) -> Option<&[u8]> {
        entry_bytes(&self.prot, &self.entries, prot_index as u32)
    }

    /// The 9-TIM HUD art pack (PROT 1203 descriptor 0).
    fn baka_art(&self) -> Option<Vec<legaia_tim::Tim>> {
        let entry = self.baka_entry(baka::BAKA_HUD_ART_PROT_INDEX)?;
        minigame_art::parse_art_pack(entry).ok()
    }

    /// The 51-record HUD widget table out of the as-loaded overlay.
    fn baka_widgets(&self) -> Option<Vec<baka::BakaHudWidget>> {
        let img = overlay_image(
            &self.prot,
            &self.entries,
            baka::BAKA_OVERLAY_PROT_INDEX as u32,
        )?;
        baka::parse_baka_hud(&img)
    }

    /// One stage TMD's raw bytes (PROT 1203 descriptor 1, a pack of 4).
    fn baka_stage_tmd_bytes(&self, index: usize) -> Option<Vec<u8>> {
        let entry = self.baka_entry(baka::BAKA_HUD_ART_PROT_INDEX)?;
        let container = parse_player_lzs(entry, 4).ok()?;
        let desc = container.descriptors.iter().find(|d| d.type_byte == 0x02)?;
        let body = decode_descriptor(entry, desc, DecodeMode::Lzs).ok()?;
        let bodies = asset_pack::extract_pack(&body).ok()?;
        bodies.get(index).map(|b| b.to_vec())
    }

    /// Build one side's renderable mesh. `side` 0 = player (`id` = character
    /// 0..=2, PROT 1204 slot), `side` 1 = opponent (`id` = roster id 3..=16).
    fn baka_fighter_mesh(&self, side: u32, id: u32) -> Option<FighterMesh> {
        let tmd_bytes = match side {
            0 => {
                let raw =
                    self.baka_entry(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX as usize)?;
                let pack = legaia_asset::battle_char_pack::parse(raw).ok()?;
                pack.slot(id as usize)?.tmd_bytes.clone()
            }
            1 => {
                let prot_index = baka::fighter_pack_prot_index(id as usize)?;
                let entry = self.baka_entry(prot_index)?;
                baka::parse_fighter_pack(entry)?.tmd_bytes
            }
            _ => return None,
        };
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        let part_count = tmd.objects.len();
        let (mesh, object_ids, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, &tmd_bytes);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        Some(FighterMesh {
            mesh,
            object_ids,
            flat,
            part_count,
        })
    }

    /// One side's animation bank record, decoded to a `PlayerAnmBundle`.
    /// Player: the PROT 1203 bank (record = `char*9 + action`); opponent: the
    /// fighter pack's own anim chunk (record = `action`).
    fn baka_anim_record(
        &self,
        side: u32,
        id: u32,
        action: u32,
    ) -> Option<(legaia_asset::player_anm::PlayerAnmBundle, usize)> {
        match side {
            0 => {
                let entry = self.baka_entry(baka::BAKA_HUD_ART_PROT_INDEX)?;
                let bundle = legaia_asset::player_anm::find_in_entry(entry, 4)
                    .into_iter()
                    .next()?;
                let record = id as usize * BANK_RECORDS_PER_CHAR + action as usize;
                Some((bundle, record))
            }
            1 => {
                let prot_index = baka::fighter_pack_prot_index(id as usize)?;
                let entry = self.baka_entry(prot_index)?;
                let pack = baka::parse_fighter_pack(entry)?;
                let bundle = legaia_asset::player_anm::parse(&pack.anim_bytes).ok()?;
                Some((bundle, action as usize))
            }
            _ => None,
        }
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the duel's presentation assets decode off this disc: the HUD
    /// art + widget table, the battle-form party pack, and at least the first
    /// ladder fighter's pack.
    pub fn baka_presentation_ready(&self) -> bool {
        self.baka_art().is_some()
            && self.baka_widgets().is_some()
            && self.baka_fighter_mesh(0, 0).is_some()
            && self.baka_fighter_mesh(1, 5).is_some()
    }

    /// The HUD widget descriptor table (`DAT_801d7160`, 51 records), as JSON:
    ///
    /// ```json
    /// [ { "scale": 4096, "page": 0, "palette": 4, "u": 48, "v": 48,
    ///     "w": 112, "h": 16, "rgb_top": [160,160,255],
    ///     "rgb_bottom": [255,255,255], "semi": 1, "abr": 1 }, ... ]
    /// ```
    ///
    /// `page` resolves the record's texpage into an index of the PROT 1203
    /// art pack (pair with [`Self::baka_page_rgba`]); `palette` is the CLUT
    /// column within that page's 256x1 strip. Empty when either side didn't
    /// decode.
    pub fn baka_hud_json(&self) -> String {
        let (Some(widgets), Some(art)) = (self.baka_widgets(), self.baka_art()) else {
            return "[]".to_string();
        };
        let rows = widgets
            .iter()
            .map(|w| {
                let page = art
                    .iter()
                    .position(|t| t.image.fb_x == w.page_x() && t.image.fb_y == w.page_y())
                    .map(|p| p.to_string())
                    .unwrap_or("null".into());
                format!(
                    concat!(
                        r#"{{"scale":{},"page":{},"palette":{},"u":{},"v":{},"w":{},"h":{},"#,
                        r#""rgb_top":[{},{},{}],"rgb_bottom":[{},{},{}],"semi":{},"abr":{}}}"#
                    ),
                    w.scale,
                    page,
                    w.palette_index(),
                    w.u,
                    w.v,
                    w.w,
                    w.h,
                    w.rgb_top[0],
                    w.rgb_top[1],
                    w.rgb_top[2],
                    w.rgb_bottom[0],
                    w.rgb_bottom[1],
                    w.rgb_bottom[2],
                    w.semi,
                    w.abr,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// One PROT 1203 art page decoded through one of its palettes, RGBA8.
    /// Pages are 256x256 4bpp; the palette index comes from the widget record.
    pub fn baka_page_rgba(&self, page: usize, palette: usize) -> Vec<u8> {
        self.baka_art()
            .and_then(|art| minigame_art::slot_page(&art, page, palette).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// Pixel width of PROT 1203 art page `page` (`0` when it didn't decode).
    pub fn baka_page_width(&self, page: usize) -> usize {
        self.baka_art()
            .and_then(|art| art.get(page).map(|t| t.pixel_width()))
            .unwrap_or(0)
    }

    // ------------------------------------------------------------- duel 3D

    /// Per-vertex positions for one duel fighter. `side` 0 = player
    /// (`id` = character 0..=2), `side` 1 = opponent (`id` = roster 3..=16).
    pub fn baka_fighter_positions(&self, side: u32, id: u32) -> Vec<f32> {
        let Some(f) = self.baka_fighter_mesh(side, id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(f.mesh.positions.len() * 3);
        for p in &f.mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn baka_fighter_uvs(&self, side: u32, id: u32) -> Vec<i32> {
        let Some(f) = self.baka_fighter_mesh(side, id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(f.mesh.uvs.len() * 2);
        for uv in &f.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn baka_fighter_cba_tsb(&self, side: u32, id: u32) -> Vec<u32> {
        let Some(f) = self.baka_fighter_mesh(side, id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(f.mesh.cba_tsb.len() * 2);
        for ct in &f.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for one duel fighter.
    pub fn baka_fighter_indices(&self, side: u32, id: u32) -> Vec<u32> {
        self.baka_fighter_mesh(side, id)
            .map(|f| f.mesh.indices)
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the bone a vertex hangs from).
    pub fn baka_fighter_object_ids(&self, side: u32, id: u32) -> Vec<u32> {
        self.baka_fighter_mesh(side, id)
            .map(|f| f.object_ids)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
    /// shader path (some fighter prims are untextured flat colour).
    pub fn baka_fighter_flat_rgba(&self, side: u32, id: u32) -> Vec<u8> {
        self.baka_fighter_mesh(side, id)
            .map(|f| f.flat)
            .unwrap_or_default()
    }

    /// `[part_count]` for one fighter (TMD object count = pose rig width).
    pub fn baka_fighter_part_count(&self, side: u32, id: u32) -> u32 {
        self.baka_fighter_mesh(side, id)
            .map(|f| f.part_count as u32)
            .unwrap_or(0)
    }

    /// `[bone_count, frame_count]` of one fighter's animation record.
    /// Player actions index the PROT 1203 bank (`char*9 + action`), opponent
    /// actions the fighter pack's own bank (typically 8 records, 0 = idle).
    pub fn baka_anim_dims(&self, side: u32, id: u32, action: u32) -> Vec<u32> {
        let Some((bundle, record)) = self.baka_anim_record(side, id, action) else {
            return vec![0, 0];
        };
        match bundle.record(record) {
            Ok(r) => vec![r.bone_count as u32, r.frame_count as u32],
            Err(_) => vec![0, 0],
        }
    }

    /// One fighter animation record decoded to absolute per-(frame, bone)
    /// `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
    /// `target_part_count` parts - the same pose format the site's mesh
    /// animators consume.
    pub fn baka_anim_pose_frames(
        &self,
        side: u32,
        id: u32,
        action: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some((bundle, record)) = self.baka_anim_record(side, id, action) else {
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

    /// Number of animation records one fighter's bank carries (9 per player
    /// character bank; the opponent packs carry their own count, idle first).
    pub fn baka_anim_record_count(&self, side: u32, id: u32) -> u32 {
        match side {
            0 => BANK_RECORDS_PER_CHAR as u32,
            1 => self
                .baka_anim_record(side, id, 0)
                .map(|(b, _)| b.record_count)
                .unwrap_or(0),
            _ => 0,
        }
    }

    // ------------------------------------------------------------- stage set

    /// Per-vertex positions of stage TMD `index` (PROT 1203 descriptor 1,
    /// four meshes: three single-object dressing pieces + a 10-object set).
    pub fn baka_stage_positions(&self, index: usize) -> Vec<f32> {
        let Some(bytes) = self.baka_stage_tmd_bytes(index) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes) else {
            return Vec::new();
        };
        let (mesh, _, _) = tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// UVs / CBA-TSB / indices / flat colours of stage TMD `index`, matching
    /// [`Self::baka_stage_positions`]'s vertex order.
    pub fn baka_stage_uvs(&self, index: usize) -> Vec<i32> {
        let Some(bytes) = self.baka_stage_tmd_bytes(index) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes) else {
            return Vec::new();
        };
        let (mesh, _, _) = tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    pub fn baka_stage_cba_tsb(&self, index: usize) -> Vec<u32> {
        let Some(bytes) = self.baka_stage_tmd_bytes(index) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes) else {
            return Vec::new();
        };
        let (mesh, _, _) = tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    pub fn baka_stage_indices(&self, index: usize) -> Vec<u32> {
        let Some(bytes) = self.baka_stage_tmd_bytes(index) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes) else {
            return Vec::new();
        };
        tmd_to_vram_mesh_field_hybrid(&tmd, &bytes).0.indices
    }

    pub fn baka_stage_flat_rgba(&self, index: usize) -> Vec<u8> {
        let Some(bytes) = self.baka_stage_tmd_bytes(index) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes) else {
            return Vec::new();
        };
        let (_, _, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        let mut out = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            out.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        out
    }

    // ---------------------------------------------------------------- VRAM

    /// Build the duel's 1 MB PSX VRAM: the PROT 1203 HUD/stage pages, the
    /// PROT 1204 party atlases (their bundled CLUT strips are the minigame's
    /// own palette - see `docs/formats/character-mesh.md`), and the chosen
    /// opponent's atlas last (roster 4's pack shares the `(512, 256)` page +
    /// row-497 CLUT with party atlas 6; retail loads them one at a time too).
    pub fn baka_duel_vram(&self, opponent: u32) -> Vec<u8> {
        let mut vram = legaia_tim::Vram::new();
        if let Some(art) = self.baka_art() {
            for tim in &art {
                vram.upload_tim(tim);
            }
        }
        if let Some(raw) =
            self.baka_entry(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX as usize)
            && let Ok(pack) = legaia_asset::battle_char_pack::parse(raw)
        {
            for atlas in &pack.atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
                }
            }
        }
        if let Some(prot_index) = baka::fighter_pack_prot_index(opponent as usize)
            && let Some(entry) = self.baka_entry(prot_index)
            && let Some(pack) = baka::parse_fighter_pack(entry)
            && let Ok(tim) = legaia_tim::parse(&pack.tim_bytes)
        {
            vram.upload_tim(&tim);
        }
        vram.as_bytes().to_vec()
    }
}
