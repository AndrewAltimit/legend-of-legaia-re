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
#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::AudioSink;

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
    pub(crate) fn baka_entry(&self, prot_index: usize) -> Option<&[u8]> {
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
                let slots = legaia_asset::battle_char_pack::parse_slots(raw).ok()?;
                slots.get(id as usize)?.tmd_bytes.clone()
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
        let flat = crate::packet_color::hybrid(&mesh, &shading);
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

    /// One side's VITAL bar frame, as the HUD renderer lays it
    /// ([`legaia_engine_core::baka_cabinet::vital_frame_cells`], the cell
    /// table at `0x801DBC34`): `side` 0 = the player's, 1 = the opponent's.
    ///
    /// ```json
    /// { "page": 0, "palette": 0,
    ///   "cells": [ { "x0": 28, "y0": 32, "x1": 36, "y1": 48,
    ///                "u0": 24, "v0": 0, "u1": 31, "v1": 15 }, ... ] }
    /// ```
    ///
    /// `page` indexes the PROT 1203 art pack like [`Self::baka_hud_json`]'s
    /// (the frame samples texpage 5 = VRAM `(320, 0)`, CLUT `0x7D80` =
    /// palette 0); the UVs are the inclusive corner span. `{}` when the
    /// overlay or the art pack did not decode.
    pub fn baka_bar_frame_json(&self, side: u32) -> String {
        let img = overlay_image(
            &self.prot,
            &self.entries,
            baka::BAKA_OVERLAY_PROT_INDEX as u32,
        );
        let (Some(cells), Some(art)) = (
            img.as_deref().and_then(baka::parse_baka_bar_frame),
            self.baka_art(),
        ) else {
            return "{}".to_string();
        };
        let page = art
            .iter()
            .position(|t| t.image.fb_x == 320 && t.image.fb_y == 0)
            .map(|p| p.to_string())
            .unwrap_or("null".into());
        let quads = legaia_engine_core::baka_cabinet::vital_frame_cells(side as usize, &cells[..])
            .iter()
            .map(|c| {
                format!(
                    concat!(
                        r#"{{"x0":{},"y0":{},"x1":{},"y1":{},"#,
                        r#""u0":{},"v0":{},"u1":{},"v1":{}}}"#
                    ),
                    c.x0, c.y0, c.x1, c.y1, c.uv[0].0, c.uv[0].1, c.uv[3].0, c.uv[3].1,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"page":{page},"palette":0,"cells":[{quads}]}}"#)
    }

    /// One HUD widget resolved through the **ported POLY_GT4 emitter**
    /// ([`legaia_engine_core::baka_fighter::hud_widget_quad`], `FUN_801d5ed0`):
    ///
    /// ```json
    /// { "page": 5, "palette": 0, "x0": 104, "y0": 22, "x1": 215, "y1": 37,
    ///   "u0": 48, "v0": 48, "u1": 159, "v1": 63, "mirror": false,
    ///   "rgb_top": [160,160,255], "rgb_bottom": [255,255,255],
    ///   "semi": true, "abr": 1 }
    /// ```
    ///
    /// The corners are the retail arithmetic, not a float approximation: the
    /// half-extent is `((cell * scale) >> 13) * size >> 12` with both shifts
    /// rounding toward zero, the span is `x - hw ..= x + hw - 1`, the UVs
    /// cover the cell **inclusively**, and each colour channel is
    /// `channel * brightness >> 8`. `mirror` swaps the left/right texture
    /// columns (retail's one-shot `DAT_801dbe98`).
    ///
    /// `brightness` is `0..=0xFF` (`0x80` = unmodulated) and `size` a
    /// `0x1000`-based scale. `{}` when the widget table or the art pack did
    /// not decode.
    pub fn baka_hud_quad_json(
        &self,
        id: usize,
        x: i16,
        y: i16,
        brightness: i32,
        size: i32,
        mirror: bool,
    ) -> String {
        let (Some(widgets), Some(art)) = (self.baka_widgets(), self.baka_art()) else {
            return "{}".to_string();
        };
        let Some(w) = widgets.get(id) else {
            return "{}".to_string();
        };
        let page = art
            .iter()
            .position(|t| t.image.fb_x == w.page_x() && t.image.fb_y == w.page_y())
            .map(|p| p.to_string())
            .unwrap_or("null".into());
        let q =
            legaia_engine_core::baka_fighter::hud_widget_quad(w, x, y, brightness, size, mirror);
        // The emitter hands back the four packet corners in `v0..v3` order;
        // the page samples an axis-aligned cell, so it wants the span.
        let (u0, v0) = q.uv[0];
        let (u1, v1) = q.uv[3];
        format!(
            concat!(
                r#"{{"page":{},"palette":{},"x0":{},"y0":{},"x1":{},"y1":{},"#,
                r#""u0":{},"v0":{},"u1":{},"v1":{},"mirror":{},"#,
                r#""rgb_top":[{},{},{}],"rgb_bottom":[{},{},{}],"semi":{},"abr":{}}}"#
            ),
            page,
            w.palette_index(),
            q.x0,
            q.y0,
            q.x1,
            q.y1,
            u0.min(u1),
            v0.min(v1),
            u0.max(u1),
            v0.max(v1),
            mirror,
            q.rgb_top[0],
            q.rgb_top[1],
            q.rgb_top[2],
            q.rgb_bottom[0],
            q.rgb_bottom[1],
            q.rgb_bottom[2],
            q.poly_code & 2 != 0,
            (q.tpage_attr >> 5) & 3,
        )
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
        let (mesh, _, shading) = tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        crate::packet_color::hybrid(&mesh, &shading)
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
            self.baka_entry(legaia_asset::battle_char_pack::ATLAS_PROT_ENTRY_INDEX as usize)
            && let Ok(atlases) = legaia_asset::battle_char_pack::parse_atlases(raw)
        {
            for atlas in &atlases {
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

    /// The duel's stage layout - which side of the arena each fighter stands
    /// on and which way it faces - as JSON:
    ///
    /// ```json
    /// { "player": { "side": -1, "facing": 1 },
    ///   "opponent": { "side": 1, "facing": -1 } }
    /// ```
    ///
    /// `side` is the sign of the fighter's X placement (the player stands on
    /// the LEFT, the opponent on the RIGHT); `facing` is the sign of its
    /// heading's X (the player faces RIGHT toward the opponent, the opponent
    /// faces LEFT toward the player). Each `facing` is the negation of the
    /// other fighter's `side`, so both look at their rival - the retail
    /// arrangement (`docs/subsystems/minigame-baka-fighter.md`).
    ///
    /// This is the **single source of truth** for the duel facing: the site's
    /// pose step (`site/js/minigame-baka.js`) turns `facing` into a world yaw
    /// (`facing * PI/2`) instead of hard-coding it, so the facing is testable
    /// off-disc. The player and opponent mesh families share the same intrinsic
    /// authored facing, so they need **opposite** world yaws to face each
    /// other; an earlier reading assumed opposite intrinsic facings and spun
    /// both the same way, leaving both looking left.
    pub fn baka_duel_facing_json(&self) -> String {
        // player: left of the arena (side -1), faces right toward the opponent
        // (facing +1). opponent: right of the arena (side +1), faces left
        // toward the player (facing -1).
        concat!(
            r#"{"player":{"side":-1,"facing":1},"#,
            r#""opponent":{"side":1,"facing":-1}}"#
        )
        .to_string()
    }
}

/// The live duel's state as the three duel hosts' pages read it - the one
/// builder behind the standalone page's `baka_state_json` and the play
/// page's `play_mg_baka_state_json`.
///
/// ```json
/// { "live": true, "phase": "fighting"|"round_over"|"match_over",
///   "round": 0, "hp": [3200, 2900], "hp_start": 3200,
///   "wins": [0, 1], "combo": [0, 2], "chosen": [2, null],
///   "can_choose": true, "clock": [96, 0], "motion": [[1, 6], [0, 3]],
///   "ghosts": [ { "owner": 0, "passes": [ { "bit": 0, "frame": 3, "cue": 2048 } ] } ],
///   "gold": 30, "winner": null,
///   "last": { "winner": 0, "draw": false, "damage": 512,
///             "critical": false, "special": false } }
/// ```
///
/// `clock` is each fighter's strike-clock cursor (1/16 clip frame) over its
/// chosen attack - the frame the retail clip is on, since the strike lands
/// when it crosses the action's keyframe. `motion` is each fighter's display
/// clip `[action record, whole frame]` (`BakaFight::motion`, the clip the
/// actor shows - it plays an attack out past the booked exchange, then
/// idles). `ghosts` are the special's afterimage passes that drew this tick.
pub(crate) fn baka_state_json_for(f: &legaia_engine_core::baka_fighter::BakaFight) -> String {
    let phase = match f.phase() {
        MatchPhase::Fighting => "fighting",
        MatchPhase::RoundOver(_) => "round_over",
        MatchPhase::MatchOver(_) => "match_over",
    };
    let chosen = |s: usize| match f.chosen(s) {
        Some(a) => a.type_id().to_string(),
        None => "null".to_string(),
    };
    let last = match f.last_exchange() {
        Some(e) => format!(
            r#"{{"winner":{},"draw":{},"damage":{},"critical":{},"special":{}}}"#,
            e.winner, e.draw, e.damage, e.critical, e.special_round_win
        ),
        None => "null".to_string(),
    };
    let winner = match f.winner() {
        Some(w) => w.to_string(),
        None => "null".to_string(),
    };
    // The special's afterimage ghosts (`FUN_801D49E8`): per live actor,
    // the fighter it trails and each drawn ghost's whole clip frame and
    // depth-cue level (`0x1000` = fully the black colour word).
    let ghosts = f
        .afterimages()
        .iter()
        .map(|(owner, fr)| {
            let passes = fr
                .passes
                .iter()
                .filter(|p| p.drawn)
                .map(|p| {
                    format!(
                        r#"{{"bit":{},"frame":{},"cue":{}}}"#,
                        p.bit,
                        p.cursor >> 4,
                        p.cue
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(r#"{{"owner":{owner},"passes":[{passes}]}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let clock = |s: usize| f.strike_clock(s).cursor;
    format!(
        concat!(
            r#"{{"live":true,"phase":{},"round":{},"hp":[{},{}],"hp_start":{},"#,
            r#""wins":[{},{}],"combo":[{},{}],"chosen":[{},{}],"can_choose":{},"#,
            r#""clock":[{},{}],"motion":[[{},{}],[{},{}]],"ghosts":[{}],"#,
            r#""gold":{},"winner":{},"last":{}}}"#
        ),
        jstr(phase),
        f.round(),
        f.hp(0),
        f.hp(1),
        legaia_engine_core::baka_fighter::HP_START,
        f.round_wins(0),
        f.round_wins(1),
        f.combo(0),
        f.combo(1),
        chosen(0),
        chosen(1),
        f.can_choose(0),
        clock(0),
        clock(1),
        f.motion(0).record,
        f.motion(0).frame(),
        f.motion(1).record,
        f.motion(1).frame(),
        ghosts,
        f.gold_reward(),
        winner,
        last,
    )
}

// ------------------------------------------------------------ duel surface

/// The duel surface's buffers flattened for a WebGL upload - the one reader
/// both browser duel hosts (this page's `baka_scene_*` and the play page's
/// `play_mg_baka_scene_*`) hand their `TmdRenderer`, so the two pages cannot
/// read the same `BakaDuelSurface` into different layouts.
pub(crate) mod duel_surface {
    use legaia_engine_core::baka_duel_scene::BakaDuelSurface;

    /// Posed positions, `[x, y, z]` per vertex, raw retail world (Y down).
    pub(crate) fn positions(s: &BakaDuelSurface) -> Vec<f32> {
        s.scene()
            .map(|s| s.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]`.
    pub(crate) fn uvs(s: &BakaDuelSurface) -> Vec<u8> {
        s.scene()
            .map(|s| s.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]`.
    pub(crate) fn cba_tsb(s: &BakaDuelSurface) -> Vec<u16> {
        s.scene()
            .map(|s| s.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, flag]` (the hybrid textured / fill layout).
    pub(crate) fn flat_rgba(s: &BakaDuelSurface) -> Vec<u8> {
        s.scene().map(|s| s.flat_rgba.clone()).unwrap_or_default()
    }

    /// Triangle indices.
    pub(crate) fn indices(s: &BakaDuelSurface) -> Vec<u32> {
        s.scene().map(|s| s.indices.clone()).unwrap_or_default()
    }

    /// The duel VRAM for the seated pair (with the cameo's wink applied).
    pub(crate) fn vram(s: &BakaDuelSurface) -> Vec<u8> {
        s.vram().map(|v| v.as_bytes().to_vec()).unwrap_or_default()
    }

    /// The scene's attribute generation, `-1` with no scene.
    pub(crate) fn attr_generation(s: &BakaDuelSurface) -> i32 {
        s.scene().map_or(-1, |s| s.attr_generation() as i32)
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Pose this page's duel on the engine's 3D surface
    /// (`legaia_engine_core::baka_duel_scene::BakaDuelSurface::frame`) - the
    /// call the native window and the play page make over the world's duel -
    /// and return its generation, or `-1` with no duel live. A generation the
    /// page has not seen means the static buffers and the VRAM changed (a new
    /// pairing): re-read them before the positions.
    ///
    /// The surface is the fighters posed by their display clips at the round
    /// setup's stand-offs, the special's afterimage ghosts, the four arena
    /// walls, the floor grid, the round-start cameo and the impact parts,
    /// framed by [`Self::baka_scene_vp`].
    pub fn baka_scene_frame(&mut self) -> i32 {
        let (prot, entries) = (&self.prot, &self.entries);
        let read = |i: usize| entry_bytes(prot, entries, i as u32).map(<[u8]>::to_vec);
        match self.baka_surface.frame(read, self.baka.as_ref()) {
            Some(_) => self.baka_surface.generation() as i32,
            None => -1,
        }
    }

    /// The scene's attribute generation (`BakaDuelScene::attr_generation`):
    /// it moves when a pose rewrote the UVs, CBA/TSB words or colours - the
    /// impact effect's flip-book cells and fades. `-1` with no scene.
    pub fn baka_scene_attr_generation(&self) -> i32 {
        duel_surface::attr_generation(&self.baka_surface)
    }

    /// This frame's posed positions, raw retail world coordinates (Y down).
    pub fn baka_scene_positions(&self) -> Vec<f32> {
        duel_surface::positions(&self.baka_surface)
    }

    /// Per-vertex `[u, v]`.
    pub fn baka_scene_uvs(&self) -> Vec<u8> {
        duel_surface::uvs(&self.baka_surface)
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn baka_scene_cba_tsb(&self) -> Vec<u16> {
        duel_surface::cba_tsb(&self.baka_surface)
    }

    /// Per-vertex `[r, g, b, flag]`.
    pub fn baka_scene_flat_rgba(&self) -> Vec<u8> {
        duel_surface::flat_rgba(&self.baka_surface)
    }

    /// Triangle indices.
    pub fn baka_scene_indices(&self) -> Vec<u32> {
        duel_surface::indices(&self.baka_surface)
    }

    /// The duel VRAM for the seated pair.
    pub fn baka_scene_vram(&self) -> Vec<u8> {
        duel_surface::vram(&self.baka_surface)
    }

    /// The arena camera's view-projection for a raw (Y-down) world vertex,
    /// column-major (`DuelCamera::vp_raw`): the round setup's snap, the
    /// spin, the special glides and the result close-up. Empty with no duel.
    pub fn baka_scene_vp(&self, aspect: f32) -> Vec<f32> {
        self.baka
            .as_ref()
            .map(|f| f.duel_camera().vp_raw(aspect).to_vec())
            .unwrap_or_default()
    }

    /// Hand the duel this frame's **packed** held pad word (`_DAT_8007B850`,
    /// Legaia's layout) - `BakaFight::set_held_pad`, the call `World`'s duel
    /// tick makes for both play hosts. Its reader is the round setup's cameo
    /// test: Triangle (`0x10`) held at a round setup sends the ring girl on.
    pub fn baka_set_held_pad(&mut self, packed: u16) {
        if let Some(f) = self.baka.as_mut() {
            f.set_held_pad(packed);
        }
    }

    /// Hand the cabinet this frame's **packed** pad edge (Legaia's layout:
    /// `0x8000` left, `0x2000` right, `0x40` Cross, `0x800` Start) for the
    /// next [`Self::baka_tick`] - the edge the attract card, the player
    /// select and the "NEXT GAME / PAY OUT" sheet read
    /// ([`legaia_engine_core::baka_fighter::BakaFight::set_cabinet_pad`]).
    pub fn baka_cabinet_pad(&mut self, packed_edge: u16) {
        if let Some(f) = self.baka.as_mut() {
            f.set_cabinet_pad(packed_edge);
        }
    }

    /// Where the cabinet is and what it draws this frame:
    ///
    /// ```json
    /// { "state": 11, "front_end": true, "lineup": [1, 42],
    ///   "player": 0, "opponent": 5,
    ///   "cells": [ { "w": 12, "x": 160, "y": 32, "b": 128 } ] }
    /// ```
    ///
    /// `lineup` is the player-select cursor and the lineup's idle clock
    /// (`null` off that screen); `cells` are the cabinet's own widget draws
    /// (the attract "PRESS START" prompt, the "PLAYER SELECT" banner, the
    /// "NEXT GAME / PAY OUT" sheet) at retail's emitter arguments - the list
    /// the two play hosts label (`BakaFight::cabinet_cells`).
    pub fn baka_cabinet_json(&self) -> String {
        let Some(f) = self.baka.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let cells: Vec<serde_json::Value> = f
            .cabinet_cells()
            .iter()
            .map(|c| serde_json::json!({ "w": c.widget, "x": c.x, "y": c.y, "b": c.brightness }))
            .collect();
        serde_json::json!({
            "live": true,
            "state": f.cabinet().state(),
            "front_end": f.cabinet().front_end(),
            "lineup": f.select_lineup().map(|(c, t)| [c as i32, t]),
            "player": f.player_roster(),
            "opponent": f.opponent_roster(),
            "cells": cells,
        })
        .to_string()
    }
}

// ------------------------------------------------------------ announcer XA

/// Rounds whose banner line is staged (`XA32` channel = round index). A
/// best-of-three ends by the third, and a drawn round only repeats one.
const ANNOUNCER_ROUNDS: i32 = 4;

/// Every announcer line the duel's chrome can start, one per `(clip,
/// channel)`: [`legaia_engine_core::baka_fighter_chrome::announcer_xa_prestage`]
/// over the staged rounds.
fn announcer_cues() -> Vec<legaia_engine_core::baka_fighter_chrome::XaCue> {
    let mut cues: Vec<_> = (0..ANNOUNCER_ROUNDS)
        .flat_map(legaia_engine_core::baka_fighter_chrome::announcer_xa_prestage)
        .collect();
    // The Muscle Dome hub's two announcer lines share the lane: the minigames
    // page plays them off the same staged bank.
    cues.extend(
        legaia_engine_core::muscle_ringside::hub_xa_prestage()
            .into_iter()
            .map(|c| legaia_engine_core::baka_fighter_chrome::XaCue {
                clip: c.clip,
                chan: c.channel,
                dur: c.duration_sectors,
            }),
    );
    cues.sort_by_key(|c| (c.clip, c.chan, c.dur));
    cues.dedup_by_key(|c| (c.clip, c.chan));
    cues
}

/// Decode every announcer line out of a raw Mode 2/2352 image: per line,
/// the span the clip starter reads from `XA<clip + 1>.XA`'s first sector
/// (`xa_clip_bank::read_span_sectors`, `FUN_8003D53C`'s stop point),
/// demuxed to the line's channel. Only the decoded PCM is kept - the same
/// staging the play page does lazily off its disc bytes. A line whose file
/// or channel is missing is left out and plays silent.
pub(crate) fn stage_baka_announcer(image: &[u8]) -> legaia_engine_audio::XaClipBank {
    use legaia_engine_audio::xa_clip_bank::{decode_channel_span, read_span_sectors};
    const RAW: usize = crate::play_xa::RAW_SECTOR_BYTES;
    let mut bank = legaia_engine_audio::XaClipBank::new();
    for cue in announcer_cues() {
        let path = format!("XA/XA{}.XA", u32::from(cue.clip) + 1);
        let Some((lba, size)) = legaia_iso::iso9660::find_path_in_image(image, &path) else {
            continue;
        };
        let sectors = read_span_sectors(u32::from(cue.dur)).min(size.div_ceil(2048));
        let start = lba as usize * RAW;
        let Some(span) = image.get(start..start + sectors as usize * RAW) else {
            continue;
        };
        if let Some((clip, width)) = decode_channel_span(span, cue.chan) {
            bank.insert(cue.clip, cue.chan, clip);
            bank.set_channel_count(cue.clip, width.max(bank.channel_count(cue.clip)));
        }
    }
    bank
}

impl LegaiaMinigames {
    /// Start one announcer line - `FUN_8003D53C(clip, channel, dur)` cut at
    /// the retail read span, through the XA path the play page's clips take.
    /// Silent when the line did not stage or audio is off.
    pub(crate) fn play_baka_xa(&mut self, xa: legaia_engine_core::baka_fighter_chrome::XaCue) {
        let Some(pcm) = self.baka_xa.cut(xa.clip, xa.chan, u32::from(xa.dur)) else {
            return;
        };
        self.baka_xa_fired = self.baka_xa_fired.wrapping_add(1);
        #[cfg(target_arch = "wasm32")]
        if let Some(out) = self.audio_out.as_ref() {
            let channels = if pcm.stereo {
                legaia_xa::Channels::Stereo
            } else {
                legaia_xa::Channels::Mono
            };
            out.play_xa_shout(
                pcm.pcm,
                pcm.sample_rate,
                channels,
                crate::play_xa::XA_GAIN_UNITY,
                legaia_engine_audio::SHOUT_CD_RESPONSE_DELAY,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = pcm;
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The announcer lane: lines staged off the disc out of those the chrome
    /// can start, and lines the duel has started.
    ///
    /// ```json
    /// { "staged": 11, "lines": 11, "fired": 3 }
    /// ```
    pub fn baka_xa_state_json(&self) -> String {
        let cues = announcer_cues();
        let staged = cues
            .iter()
            .filter(|c| self.baka_xa.is_staged(c.clip, c.chan))
            .count();
        format!(
            r#"{{"staged":{staged},"lines":{},"fired":{}}}"#,
            cues.len(),
            self.baka_xa_fired
        )
    }
}
