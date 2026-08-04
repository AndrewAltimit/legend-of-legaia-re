//! `LegaiaSummons` WASM bindings for `site/magic.html` - the Seru-magic
//! summon viewer: pick a spell, watch the creature it summons perform the cast.
//!
//! Every player Seru-magic cast (`0x81..=0xA0`) streams its creature out of
//! `\data\battle\summon.dat` (extraction PROT 893) while the cast plays. The
//! group's **last** slot is the summon-creature **actor record**: an inline
//! `[u32 name][u32 TMD][u32 texture pool]` head, a monster-shaped stat block,
//! and a `+0x4C` table of per-part entries whose packed `[u8 parts][u8 frames]`
//! keyframe streams sit at entry `+0x8C` - the same shape the monster archive
//! uses, which is why the whole thing renders through the ordinary battle
//! relocation ([`legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`]).
//! Retail agrees: the installer `FUN_801F19EC` hands the record's TMD and
//! texture pool to `FUN_80055468`, the monster mesh installer.
//!
//! Two bands, and the difference is the whole reason the seven big summons
//! were invisible until now:
//!
//! * `0x81..=0x99` (three-slot groups) reuse an ordinary `battle_data` enemy
//!   body - their actor-record TMD is byte-identical to an archive record, and
//!   [`legaia_asset::summon_creatures`] maps each to its creature.
//! * `0x9A..=0xA0` (four-slot groups - Palma, Mule, Horn, Jedo, Meta, Terra,
//!   Ozma) carry a **bespoke** mesh that matches no archive record, so the
//!   creature-id route resolves nothing for them. Their texture pool and their
//!   per-part keyframe entries live in the group's third *raw* slot instead:
//!   its head is a monster-shaped pool byte-for-byte (`0x1E0` CLUT region +
//!   `0x8000` page) and its `+0x81E0` part pool holds the streams the record's
//!   `+0x4C` offsets point into.
//!
//! Shading is retail: the page uploads the TMD's own per-vertex packet colour
//! ([`crate::packet_color::textured`]) and applies no light source.

use super::*;

use legaia_asset::monster_archive::MonsterAnimation;
use legaia_asset::summon_readef::{self, SummonCast};
use legaia_engine_core::summon as summon_core;

/// `summon.dat` in extraction PROT index space.
const SUMMON_PROT_INDEX: u32 = summon_readef::SUMMON_PROT_INDEX as u32;

/// Log a decode degradation. Browser console on wasm; stderr natively (the
/// disc-gated tests drive this module natively).
fn summon_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    console_log(s);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// Read one PROT entry's raw on-disc bytes.
fn entry_bytes<'a>(
    prot: &'a [u8],
    entries: &[disc::EntryMeta],
    prot_index: u32,
) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// Flatten a clip to the site animators' pose layout: six `i32` per part per
/// frame, `[tx, ty, tz, rx, ry, rz]`, frame `f` / part `p` / component `c` at
/// `(f * part_count + p) * 6 + c`. Rotations are unsigned 12-bit angles.
fn flatten_pose_frames(anim: &MonsterAnimation) -> Vec<i32> {
    let mut out = Vec::with_capacity(anim.frame_count * anim.part_count * 6);
    for frame in &anim.frames {
        for p in frame {
            out.extend_from_slice(&[
                p.tx as i32,
                p.ty as i32,
                p.tz as i32,
                p.rx as i32,
                p.ry as i32,
                p.rz as i32,
            ]);
        }
    }
    out
}

/// The cast currently on the canvas: the parsed group plus everything derived
/// from it once, so the per-buffer accessors are pure reads.
struct LoadedCast {
    cast: SummonCast,
    mesh: legaia_tmd::mesh::VramMesh,
    object_ids: Vec<u32>,
    vram: legaia_tim::Vram,
    /// Decoded FX texture pages, parallel to `cast.fx_slots`.
    fx_pages: Vec<summon_readef::FxPage>,
}

/// The site's Seru-magic summon host: a disc, plus one cast's decoded group.
#[wasm_bindgen]
pub struct LegaiaSummons {
    prot: Vec<u8>,
    entries: Vec<disc::EntryMeta>,
    current: Option<LoadedCast>,
}

impl Default for LegaiaSummons {
    fn default() -> Self {
        Self::new()
    }
}

/// Element names in actor-record `+0x1D` id order.
fn element_name(id: u8) -> &'static str {
    legaia_asset::element_affinity::Element::from_id(id).map_or("unknown", |e| e.name())
}

/// Build the JSON row describing one cast, without decoding its mesh - the
/// index the page renders its picker from.
fn cast_row(summon_dat: &[u8], spell_id: u8) -> Option<serde_json::Value> {
    let cast = summon_readef::parse_cast(summon_dat, spell_id).ok()?;
    let big = summon_core::big_summon(spell_id);
    let spell = legaia_engine_core::retail_magic::get(spell_id);
    Some(serde_json::json!({
        "spell_id": spell_id,
        // The summon's own name: the creature for the reused-body bands, the
        // summon's name for the seven bespoke ones.
        "summon": summon_core::summon_display_name(spell_id),
        // The cast's name as an ASCII string ON THE DISC (actor record rec[0]).
        "attack": cast.attack_name,
        // The SCUS spell-table name + MP, where the id is in the player block.
        "spell_name": spell.map(|s| s.name),
        "mp": spell.map(|s| s.mp),
        "element": element_name(cast.element),
        "element_id": cast.element,
        // true = bespoke body (the seven big summons); false = a reused
        // battle_data enemy body, named by `creature`.
        "bespoke": cast.bespoke,
        "creature": legaia_asset::summon_creatures::creature_for_spell(spell_id)
            .map(|c| c.creature_id),
        "ra_seru": big.is_some(),
        "clips": cast.clips.iter().enumerate().map(|(i, c)| serde_json::json!({
            "index": i,
            "tag": c.action_id,
            "rate": c.rate,
            "parts": c.part_count,
            "frames": c.frame_count,
        })).collect::<Vec<_>>(),
        "fx_pages": cast.fx_slots.len(),
        "actor_slot": cast.actor_slot,
    }))
}

#[wasm_bindgen]
impl LegaiaSummons {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            prot: Vec::new(),
            entries: Vec::new(),
            current: None,
        }
    }

    /// Load a full Mode2/2352 disc image (or a raw `PROT.DAT`) and parse the
    /// TOC. Returns `{"entries": N}` JSON; errors throw.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        let prot = if disc::is_mode2_2352_disc(&bytes) {
            disc::extract_prot_dat(&bytes).ok_or_else(|| {
                JsValue::from_str("summons: PROT.DAT not found in this disc image")
            })?
        } else {
            bytes
        };
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("summons: PROT.DAT TOC parse failed"))?;
        self.prot = prot;
        self.entries = entries;
        self.current = None;
        Ok(format!("{{\"entries\":{}}}", self.entries.len()))
    }

    /// Every player Seru-magic cast on this disc, id-ascending, as
    /// `{"casts":[...]}` - the page's picker index. Each row carries the
    /// summon's name, the cast's on-disc attack name, element, MP, whether the
    /// body is bespoke, and one entry per decodable keyframe clip.
    pub fn catalog(&self) -> String {
        let Some(summon_dat) = entry_bytes(&self.prot, &self.entries, SUMMON_PROT_INDEX) else {
            return r#"{"ok":false,"why":"summon.dat (PROT 893) is not present on this disc"}"#
                .to_string();
        };
        let casts: Vec<serde_json::Value> = summon_readef::PLAYER_CAST_IDS
            .filter_map(|id| cast_row(summon_dat, id))
            .collect();
        serde_json::json!({ "ok": true, "casts": casts }).to_string()
    }

    /// Decode cast `spell_id` and put it on the canvas. Returns a JSON summary
    /// (`{"ok":true, ...}` with the same fields as one [`Self::catalog`] row
    /// plus `part_count`), or `{"ok":false,"why":...}`.
    pub fn set_cast(&mut self, spell_id: u32) -> String {
        let spell_id = spell_id as u8;
        let Some(summon_dat) = entry_bytes(&self.prot, &self.entries, SUMMON_PROT_INDEX) else {
            self.current = None;
            return r#"{"ok":false,"why":"summon.dat (PROT 893) is not present"}"#.to_string();
        };
        let mut row = match cast_row(summon_dat, spell_id) {
            Some(r) => r,
            None => {
                self.current = None;
                return serde_json::json!({
                    "ok": false,
                    "why": format!("cast {spell_id:#04x} did not decode"),
                })
                .to_string();
            }
        };
        let cast = match summon_readef::parse_cast(summon_dat, spell_id) {
            Ok(c) => c,
            Err(e) => {
                self.current = None;
                return serde_json::json!({ "ok": false, "why": e.to_string() }).to_string();
            }
        };

        // Mesh + VRAM through the ordinary battle relocation, at the slot the
        // big-summon raw slot's own VRAM targets pin (monster battle slot 2).
        let mut vram = legaia_tim::Vram::new();
        let Some(mesh) = cast
            .mesh
            .battle_render_mesh(summon_readef::SUMMON_VRAM_SLOT, &mut vram)
        else {
            self.current = None;
            return serde_json::json!({
                "ok": false,
                "why": format!("cast {spell_id:#04x} carries no parseable TMD"),
            })
            .to_string();
        };
        if mesh.indices.is_empty() {
            self.current = None;
            return serde_json::json!({
                "ok": false,
                "why": format!("cast {spell_id:#04x} has no textured primitives"),
            })
            .to_string();
        }
        let object_ids = match legaia_tmd::parse(cast.mesh.tmd_bytes()) {
            Ok(tmd) => {
                legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, cast.mesh.tmd_bytes()).1
            }
            Err(e) => {
                summon_log(&format!("summons: {spell_id:#04x} object ids: {e}"));
                Vec::new()
            }
        };

        // FX pages: the per-cast CLUT row + 4bpp page the applier uploads to
        // VRAM while the cast plays.
        let fx_pages: Vec<summon_readef::FxPage> = cast
            .fx_slots
            .iter()
            .filter_map(|(index, t)| {
                let s = summon_dat.get(
                    index * summon_readef::SLOT_BYTES..(index + 1) * summon_readef::SLOT_BYTES,
                )?;
                summon_readef::decode_texture_slot(s, t, 0)
            })
            .collect();

        if let Some(obj) = row.as_object_mut() {
            obj.insert("ok".into(), serde_json::Value::Bool(true));
            obj.insert(
                "part_count".into(),
                serde_json::json!(cast.clips.first().map_or(0, |c| c.part_count)),
            );
            obj.insert(
                "object_count".into(),
                serde_json::json!(object_ids.iter().copied().max().map_or(0, |m| m + 1)),
            );
            obj.insert(
                "fx_page_sizes".into(),
                serde_json::json!(
                    fx_pages
                        .iter()
                        .map(|p| [p.width, p.height])
                        .collect::<Vec<_>>()
                ),
            );
        }
        let json = row.to_string();
        self.current = Some(LoadedCast {
            cast,
            mesh,
            object_ids,
            vram,
            fx_pages,
        });
        json
    }

    /// Per-vertex positions of the current cast's creature mesh (flat `f32`,
    /// 3 per vertex). Empty until [`Self::set_cast`].
    pub fn mesh_positions(&self) -> Vec<f32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.positions.len() * 3);
        for p in &c.mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` integer texel coords, parallel to the positions.
    pub fn mesh_uvs(&self) -> Vec<i32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.uvs.len() * 2);
        for uv in &c.mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[r, g, b, 255]` **packet colours** - the modulation half of
    /// retail's `texel * colour / 128`, parallel to the positions. This is the
    /// stream that keeps the body shaded the way retail shades it; an unbound
    /// colour attribute would default to white and read as `texel * 2`.
    pub fn mesh_flat_rgba(&self) -> Vec<u8> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        crate::packet_color::textured(&c.mesh)
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn mesh_cba_tsb(&self) -> Vec<u32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(c.mesh.cba_tsb.len() * 2);
        for ct in &c.mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices (`u32`, multiple of 3).
    pub fn mesh_indices(&self) -> Vec<u32> {
        self.current
            .as_ref()
            .map(|c| c.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the part a vertex hangs from), parallel to
    /// the positions - the channel the pose loop drives.
    pub fn mesh_object_ids(&self) -> Vec<u32> {
        self.current
            .as_ref()
            .map(|c| c.object_ids.clone())
            .unwrap_or_default()
    }

    /// Bounding sphere `[cx, cy, cz, r]` so the page can frame the creature
    /// before the first pose lands.
    pub fn mesh_bounds(&self) -> Vec<f32> {
        let Some(c) = &self.current else {
            return vec![0.0; 4];
        };
        if c.mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&c.mesh.positions)
    }

    /// The 1 MB PSX VRAM holding the cast's texture pool at the retail
    /// placement (CLUT row `486`, 4bpp page at `(448, 256)`).
    pub fn vram_bytes(&self) -> Vec<u8> {
        self.current
            .as_ref()
            .map(|c| c.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Clip `index`'s pose frames (see [`flatten_pose_frames`] layout). Empty
    /// when the index is out of range.
    pub fn clip_pose_frames(&self, index: u32) -> Vec<i32> {
        self.current
            .as_ref()
            .and_then(|c| c.cast.clips.get(index as usize))
            .map(flatten_pose_frames)
            .unwrap_or_default()
    }

    /// Every clip of the current cast concatenated into one timeline - what
    /// "play the cast" means: the phases run back to back in the actor
    /// record's own `+0x4C` table order. Clips whose part count differs from
    /// the first one's are skipped (the rig width has to match to share a
    /// pose buffer); [`Self::sequence_clip_indices`] reports which survived.
    pub fn sequence_pose_frames(&self) -> Vec<i32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for i in self.sequence_clip_indices() {
            out.extend(flatten_pose_frames(&c.cast.clips[i as usize]));
        }
        out
    }

    /// Indices of the clips [`Self::sequence_pose_frames`] concatenated.
    pub fn sequence_clip_indices(&self) -> Vec<u32> {
        let Some(c) = &self.current else {
            return Vec::new();
        };
        let Some(width) = c.cast.clips.first().map(|f| f.part_count) else {
            return Vec::new();
        };
        c.cast
            .clips
            .iter()
            .enumerate()
            .filter(|(_, k)| k.part_count == width)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Decoded RGBA of FX texture page `index` (row-major, `w * h * 4`) - the
    /// per-cast 4bpp page the applier uploads while the cast plays, resolved
    /// through the first 16-colour window of its CLUT row. Empty when out of
    /// range.
    pub fn fx_page_rgba(&self, index: u32) -> Vec<u8> {
        self.current
            .as_ref()
            .and_then(|c| c.fx_pages.get(index as usize))
            .map(|p| p.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of FX texture page `index`, or empty.
    pub fn fx_page_size(&self, index: u32) -> Vec<u32> {
        self.current
            .as_ref()
            .and_then(|c| c.fx_pages.get(index as usize))
            .map(|p| vec![p.width as u32, p.height as u32])
            .unwrap_or_default()
    }
}
