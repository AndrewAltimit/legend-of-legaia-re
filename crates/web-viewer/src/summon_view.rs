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

// ---------------------------------------------------------------------------
// Camera framing over a posed point cloud
// ---------------------------------------------------------------------------

/// How many scaled MADs from the median a **part** may sit before it stops
/// setting the camera scale.
///
/// The extremes cannot set the scale. A summon's rig routinely holds one part
/// a very long way from the body - Meta's sword is a separate object thrown
/// clear of the knight - and a bound over *every* vertex is then dominated by
/// that separation: the camera pulls back to fit a mostly empty volume and the
/// body renders as a speck. That is the bounding box behaving correctly and
/// still producing a useless frame.
///
/// A percentile of *vertex* distance does not fix it either, and measuring
/// said so: the value that framed Meta's knight (0.80) cropped Terra's wings,
/// and the value that kept Terra whole (0.90) left Meta a speck. A vertex
/// percentile cannot tell "one big contiguous body" from "small body plus a
/// distant part" - both just have vertices far from the centre.
///
/// The discriminating measurement is per **part**: take each animated object's
/// centroid distance from the body centre, and reject the ones that are
/// outliers *against the spread of the other parts* (median + `k` × scaled
/// MAD). Terra's wings sit at distances comparable to her other parts, so they
/// are kept and she frames whole; Meta's sword sits far outside the spread of
/// the knight's parts, so it is dropped and the knight fills the frame. The
/// cost is that such a part can sit outside the frame - which is what the
/// page's "frame the whole cast" toggle is for.
pub const FRAMING_OUTLIER_K: f32 = 3.0;

/// Vertex-distance percentile used only when there is no usable part index
/// (an unrigged point cloud), where the part-level test cannot run.
pub const FRAMING_FALLBACK_PERCENTILE: f32 = 0.90;

/// Scale factor making the median absolute deviation a standard-deviation
/// estimate for normally distributed data, so `k` reads as "sigmas".
const MAD_TO_SIGMA: f32 = 1.4826;

/// Per-axis median of the eligible vertices - the framing centre.
///
/// Median rather than mean: the mean is still dragged by a distant part in
/// proportion to its vertex count, whereas the median lands on whichever mode
/// holds the bulk of the geometry, which is the body.
fn framing_center_of(positions: &[f32], object_ids: &[u32], part_count: u32) -> [f32; 3] {
    let mut axes: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (i, p) in positions.chunks_exact(3).enumerate() {
        if !object_ids.is_empty() && object_ids.get(i).is_none_or(|&o| o >= part_count) {
            continue;
        }
        for k in 0..3 {
            axes[k].push(p[k]);
        }
    }
    let mut out = [0.0f32; 3];
    for (k, v) in axes.iter_mut().enumerate() {
        if v.is_empty() {
            continue;
        }
        let mid = v.len() / 2;
        v.select_nth_unstable_by(mid, f32::total_cmp);
        out[k] = v[mid];
    }
    out
}

/// Median of `v` (mutates the order). `None` when empty.
fn median(v: &mut [f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    let mid = v.len() / 2;
    v.select_nth_unstable_by(mid, f32::total_cmp);
    Some(v[mid])
}

fn dist(p: &[f32], c: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (p[0] - c[0], p[1] - c[1], p[2] - c[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Centroid distance from `c` for every animated object that owns at least one
/// vertex, as `(object index, distance)`.
fn part_centroid_distances(
    positions: &[f32],
    object_ids: &[u32],
    part_count: u32,
    c: [f32; 3],
) -> Vec<(u32, f32)> {
    let n = part_count as usize;
    let mut sum = vec![[0f64; 3]; n];
    let mut count = vec![0usize; n];
    for (i, p) in positions.chunks_exact(3).enumerate() {
        let Some(&o) = object_ids.get(i) else {
            continue;
        };
        if o >= part_count {
            continue;
        }
        let o = o as usize;
        for k in 0..3 {
            sum[o][k] += p[k] as f64;
        }
        count[o] += 1;
    }
    (0..n)
        .filter(|&o| count[o] > 0)
        .map(|o| {
            let m = count[o] as f64;
            let ct = [
                (sum[o][0] / m) as f32,
                (sum[o][1] / m) as f32,
                (sum[o][2] / m) as f32,
            ];
            (o as u32, dist(&ct, c))
        })
        .collect()
}

/// The set of objects whose centroid is **not** a distance outlier among the
/// parts (median + `k` × scaled MAD). `None` when there is no usable spread to
/// judge against - fewer than three parts, or every part at the same distance.
fn retained_parts(parts: &[(u32, f32)], k: f32) -> Option<Vec<u32>> {
    if parts.len() < 3 {
        return None;
    }
    let mut d: Vec<f32> = parts.iter().map(|&(_, d)| d).collect();
    let med = median(&mut d)?;
    let mut dev: Vec<f32> = parts.iter().map(|&(_, d)| (d - med).abs()).collect();
    let mad = median(&mut dev)? * MAD_TO_SIGMA;
    if mad <= 0.0 {
        return None;
    }
    let cutoff = med + k.max(0.0) * mad;
    let kept: Vec<u32> = parts
        .iter()
        .filter(|&&(_, d)| d <= cutoff)
        .map(|&(o, _)| o)
        .collect();
    (!kept.is_empty() && kept.len() < parts.len()).then_some(kept)
}

/// `[cx, cy, cz, radius]` for a posed point cloud.
///
/// The centre is the per-axis vertex median. The radius is built in two steps,
/// and the composition is the point - neither step works alone:
///
/// 1. **Reject outlier parts.** Any object whose centroid sits more than `k`
///    scaled MADs beyond the median part distance stops counting. This is what
///    drops Meta's thrown sword while keeping every one of Terra's wings, which
///    sit at distances comparable to her other parts.
/// 2. **Take a percentile over what is left.** The scale is then the
///    [`FRAMING_FALLBACK_PERCENTILE`] of vertex distance *among the retained
///    parts*, so a single stretched limb inside the body still cannot set it.
///
/// Step 1 alone leaves the radius at the retained set's maximum, which for a
/// body with no outlier is just the naive bound again - measured, and it was
/// worse than the percentile everywhere. Step 2 alone cannot tell a wide body
/// from a body plus a distant part - also measured, and it traded Meta against
/// Terra. See [`FRAMING_OUTLIER_K`].
///
/// `object_ids` may be empty, in which case every vertex counts and only step 2
/// runs; otherwise a vertex is eligible only when its object index is below
/// `part_count`, the same filter the page's poser applies.
fn framing_bound_of(positions: &[f32], object_ids: &[u32], part_count: u32, k: f32) -> [f32; 4] {
    let c = framing_center_of(positions, object_ids, part_count);

    // Step 1: which objects still count.
    let kept = (!object_ids.is_empty() && part_count > 0)
        .then(|| {
            let parts = part_centroid_distances(positions, object_ids, part_count, c);
            retained_parts(&parts, k)
        })
        .flatten();

    // Step 2: a percentile of vertex distance over the retained objects.
    let mut d: Vec<f32> = positions
        .chunks_exact(3)
        .enumerate()
        .filter(|(i, _)| match object_ids.get(*i) {
            _ if object_ids.is_empty() => true,
            Some(&o) if o < part_count => kept.as_ref().is_none_or(|s| s.contains(&o)),
            _ => false,
        })
        .map(|(_, p)| dist(p, c))
        .collect();
    if d.is_empty() {
        return [c[0], c[1], c[2], 1.0];
    }
    let q = FRAMING_FALLBACK_PERCENTILE.clamp(0.05, 1.0);
    let idx = (((d.len() as f32 - 1.0) * q).round() as usize).min(d.len() - 1);
    d.select_nth_unstable_by(idx, f32::total_cmp);
    [c[0], c[1], c[2], d[idx].max(1.0)]
}

/// `[cx, cy, cz, radius]` for a posed point cloud - see [`FRAMING_OUTLIER_K`]
/// for why the radius rejects distant parts instead of bounding everything.
///
/// The page calls this once when a clip starts (to set both centre and radius)
/// and [`summon_framing_center`] every frame afterwards (to follow the body
/// without the scale breathing). Both live here rather than in the page so
/// there is one implementation of the statistic, and so it can be tested.
#[wasm_bindgen]
pub fn summon_framing_bound(
    positions: Vec<f32>,
    object_ids: Vec<u32>,
    part_count: u32,
    outlier_k: f32,
) -> Vec<f32> {
    framing_bound_of(&positions, &object_ids, part_count, outlier_k).to_vec()
}

/// `[cx, cy, cz]` - the framing centre alone, for the per-frame follow.
#[wasm_bindgen]
pub fn summon_framing_center(
    positions: Vec<f32>,
    object_ids: Vec<u32>,
    part_count: u32,
) -> Vec<f32> {
    framing_center_of(&positions, &object_ids, part_count).to_vec()
}

/// The default part-outlier cutoff, exported so the page does not hard-code a
/// second copy of the constant.
#[wasm_bindgen]
pub fn summon_framing_outlier_k() -> f32 {
    FRAMING_OUTLIER_K
}

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

#[cfg(test)]
mod framing_tests {
    use super::*;

    /// Vertices per body part in the synthetic rigs.
    const PART_VERTS: usize = 60;

    /// A rig: `parts` small clusters spread evenly out to `spread` from the
    /// origin (one object each), optionally plus one extra object of
    /// `far_verts` vertices parked at `far`. This is the shape the real
    /// summons have - a body made of many parts, sometimes with one part
    /// thrown clear of it.
    ///
    /// `far_verts` matters: give the outlier only a handful of vertices and a
    /// plain distance percentile would suppress it on its own, so the test
    /// would pass without the part-level rejection doing anything. The tests
    /// below hand it a share big enough to survive the percentile, which is
    /// what makes them about step 1.
    fn rig(
        parts: usize,
        spread: f32,
        far: Option<[f32; 3]>,
        far_verts: usize,
    ) -> (Vec<f32>, Vec<u32>) {
        let mut pos = Vec::new();
        let mut ids = Vec::new();
        for o in 0..parts {
            let t = o as f32 / parts as f32;
            let a = t * std::f32::consts::TAU;
            let c = [spread * a.cos(), spread * (t * 3.1 - 1.5), spread * a.sin()];
            for i in 0..PART_VERTS {
                let u = i as f32 / PART_VERTS as f32;
                pos.extend_from_slice(&[
                    c[0] + spread * 0.08 * (u * 19.0).sin(),
                    c[1] + spread * 0.08 * (u * 23.0).cos(),
                    c[2] + spread * 0.08 * (u * 29.0).sin(),
                ]);
                ids.push(o as u32);
            }
        }
        if let Some(f) = far {
            for _ in 0..far_verts {
                pos.extend_from_slice(&f);
                ids.push(parts as u32);
            }
        }
        (pos, ids)
    }

    /// Plain maximum distance from the origin over every vertex - the naive
    /// bound the framing statistic replaces.
    fn naive_max(pos: &[f32]) -> f32 {
        pos.chunks_exact(3)
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0f32, f32::max)
    }

    /// The defect this statistic exists for: one part thrown clear of the body
    /// must not set the camera scale. Meta's sword is the live case.
    #[test]
    fn a_single_distant_part_cannot_set_the_scale() {
        // The outlier gets a third of the vertices, so a distance percentile
        // alone would keep it - only the part-level rejection can drop it.
        let (pos, ids) = rig(12, 10.0, Some([900.0, 0.0, 0.0]), 12 * PART_VERTS / 2);
        let b = framing_bound_of(&pos, &ids, 13, FRAMING_OUTLIER_K);
        // Centre stays on the body, not drawn out toward the outlier.
        assert!(
            b[0].abs() < 15.0,
            "centre x {} drifted to the outlier",
            b[0]
        );
        // Radius is the body's, not the separation's.
        assert!(
            b[3] < 40.0,
            "radius {} was set by the distant part (body spread is ~10)",
            b[3]
        );
        // Non-vacuity, two ways. The input really does hold far geometry ...
        assert!(naive_max(&pos) > 800.0, "control: max distance is tame");
        // ... and with the outlier test switched off (a cutoff nothing can
        // exceed) that geometry does set the radius. So the assertion above is
        // about step 1, not about an input the percentile handled anyway.
        let wide = framing_bound_of(&pos, &ids, 13, 1.0e6);
        assert!(
            wide[3] > 800.0,
            "control: with no outlier rejection the radius is {}",
            wide[3]
        );
    }

    /// The control that keeps the fix from becoming a crop: a body whose parts
    /// are genuinely spread out must keep its radius. Terra's wings are the
    /// live case, and a vertex-percentile version of this statistic cropped
    /// them - the part-level test is what tells her apart from Meta.
    #[test]
    fn a_wide_body_of_many_parts_keeps_its_radius() {
        let (pos, ids) = rig(14, 100.0, None, 0);
        let b = framing_bound_of(&pos, &ids, 14, FRAMING_OUTLIER_K);
        assert!(
            b[3] > 90.0,
            "radius {} cropped a legitimately wide body (spread 100)",
            b[3]
        );
    }

    /// The two cases side by side: the same *number* of far vertices reads as
    /// "the body" when many parts share that distance, and as "an outlier"
    /// when only one part does.
    #[test]
    fn spread_out_parts_and_one_distant_part_are_told_apart() {
        let wide = rig(14, 100.0, None, 0);
        let compact = rig(14, 8.0, Some([100.0, 0.0, 0.0]), 14 * PART_VERTS / 2);
        let bw = framing_bound_of(&wide.0, &wide.1, 14, FRAMING_OUTLIER_K);
        let bc = framing_bound_of(&compact.0, &compact.1, 15, FRAMING_OUTLIER_K);
        assert!(
            bw[3] > 90.0 && bc[3] < 30.0,
            "wide body radius {} must stay large while the compact body with a \
             part at the same distance stays small (got {})",
            bw[3],
            bc[3]
        );
    }

    /// The centre follows the body when it sits off the origin.
    #[test]
    fn the_centre_tracks_the_body_not_the_origin() {
        let (mut pos, ids) = rig(10, 8.0, Some([-900.0, 700.0, 0.0]), 60);
        for (i, p) in pos.chunks_exact_mut(3).enumerate() {
            if ids[i] < 10 {
                p[0] += 200.0;
                p[1] -= 50.0;
                p[2] += 30.0;
            }
        }
        let c = framing_center_of(&pos, &ids, 11);
        assert!((c[0] - 200.0).abs() < 25.0, "cx {}", c[0]);
        assert!((c[1] + 50.0).abs() < 25.0, "cy {}", c[1]);
        assert!((c[2] - 30.0).abs() < 25.0, "cz {}", c[2]);
    }

    /// Vertices whose object index is at or past `part_count` are not animated
    /// by the clip, so they must not be framed either - the same filter the
    /// page's poser applies.
    #[test]
    fn vertices_past_the_rig_width_are_excluded() {
        let (pos, ids) = rig(6, 5.0, Some([400.0, 0.0, 0.0]), 6 * PART_VERTS);
        // part_count = 6 -> the `far` object (index 6) is out of the rig.
        let b = framing_bound_of(&pos, &ids, 6, 1.0e6);
        assert!(
            b[3] < 20.0,
            "radius {} included vertices past the rig width",
            b[3]
        );
        // With the rig wide enough to include it AND no outlier rejection, it
        // does count - proving the width filter is what excluded it above.
        let b2 = framing_bound_of(&pos, &ids, 7, 1.0e6);
        assert!(b2[3] > 300.0, "control radius {}", b2[3]);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert_eq!(
            framing_bound_of(&[], &[], 0, FRAMING_OUTLIER_K),
            [0.0, 0.0, 0.0, 1.0]
        );
        assert_eq!(framing_center_of(&[], &[], 0), [0.0, 0.0, 0.0]);
        // No eligible vertex (every object past the rig width).
        let b = framing_bound_of(&[1.0, 2.0, 3.0], &[7], 1, FRAMING_OUTLIER_K);
        assert_eq!(b[3], 1.0, "radius floors at 1 rather than collapsing");
        // An empty object-id list means "every vertex counts", via the
        // percentile fallback.
        let b = framing_bound_of(&[0.0, 0.0, 0.0, 100.0, 0.0, 0.0], &[], 0, FRAMING_OUTLIER_K);
        assert!(b[3] >= 50.0, "radius {} with no id filter", b[3]);
        // Fewer than three parts gives no spread to judge an outlier against,
        // so nothing is rejected.
        let (pos, ids) = rig(1, 5.0, Some([500.0, 0.0, 0.0]), PART_VERTS);
        let b = framing_bound_of(&pos, &ids, 2, FRAMING_OUTLIER_K);
        assert!(b[3] > 400.0, "two parts: keep everything, got {}", b[3]);
    }
}
