//! Player-ANM corpus + record decode exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    // ------------------------------------------------------------------

    /// JSON summary of every player-ANM bundle accessible from this disc.
    /// Shape:
    /// ```text
    /// {
    ///   "bundles": [
    ///     {
    ///       "prot_index": 4,
    ///       "record_count": 69,
    ///       "decoded_bytes": 96448,
    ///       "records": [
    ///         { "index": 0, "offset": 0x118, "size": 496, "marker_1": 0x080C },
    ///         ...
    ///       ]
    ///     }, ...
    ///   ]
    /// }
    /// ```
    /// Surveys the corpus by walking each scene's first PROT slot
    /// (parse_player_lzs descriptor count = 6, the canonical scene-bundle
    /// shape) and emitting one entry per cleanly-decoded type-0x05 section.
    pub fn player_anm_corpus_json(&self) -> String {
        let toc = match parse_prot_toc(&self.disc) {
            Some(t) => t,
            None => return r#"{"bundles":[],"error":"no PROT TOC"}"#.to_string(),
        };
        let mut bundles: Vec<serde_json::Value> = Vec::new();
        for meta in &toc {
            let off = meta.byte_offset as usize;
            let end = off.saturating_add(meta.size_bytes as usize);
            let Some(buf) = self.disc.get(off..end) else {
                continue;
            };
            // The vast majority of scene bundles use 6 descriptors; that's the
            // detector spread the disc-gated test pins. Lower counts catch a
            // handful of `befect_data` / `other5` variants.
            for desc_count in [6, 3, 5, 7] {
                let found = legaia_asset::player_anm::find_in_entry(buf, desc_count);
                if !found.is_empty() {
                    for b in found {
                        let recs: Vec<serde_json::Value> = (0..b.record_count as usize)
                            .map(|i| {
                                let bytes = b.record_bytes(i);
                                let rec = b.record(i).ok();
                                // Stillness score for frame 0: sum of
                                // each bone's rotation distance from a
                                // **90° cardinal** (multiples of 1024 in
                                // PSX angle units). Rest-pose anims for
                                // characters whose TMD has Z-mirrored
                                // limbs (Vahn's field form) use an
                                // ry≈180° flip on one shin to unmirror
                                // it; measuring against cardinals (not
                                // just 0/360°) keeps those records
                                // scoring low. Lower = closer to an idle.
                                let stillness = if let Some(r) = rec.as_ref() {
                                    let mut score: i64 = 0;
                                    for bone in 0..(r.bone_count as usize) {
                                        if let Some(t) = b.bone_transform(i, 0, bone) {
                                            for r_ang in [t.r_x, t.r_y, t.r_z] {
                                                let m = r_ang.rem_euclid(1024);
                                                score += m.min(1024 - m) as i64;
                                            }
                                        }
                                    }
                                    score
                                } else {
                                    i64::MAX
                                };
                                serde_json::json!({
                                    "index": i,
                                    "offset": b.record_offsets[i],
                                    "size": bytes.len(),
                                    "marker_1": b.record_marker_1(i).unwrap_or(0),
                                    "a": rec.as_ref().map(|r| r.a).unwrap_or(0),
                                    "b": rec.as_ref().map(|r| r.b).unwrap_or(0),
                                    "flag": rec.as_ref().map(|r| r.flag).unwrap_or(0),
                                    "bone_count": rec.as_ref().map(|r| r.bone_count).unwrap_or(0),
                                    "frame_count": rec.as_ref().map(|r| r.frame_count).unwrap_or(0),
                                    "stillness": stillness,
                                })
                            })
                            .collect();
                        bundles.push(serde_json::json!({
                            "prot_index": meta.index,
                            "record_count": b.record_count,
                            "decoded_bytes": b.decoded.len(),
                            "records": recs,
                        }));
                    }
                    break;
                }
            }
        }
        serde_json::json!({ "bundles": bundles }).to_string()
    }

    /// Find a single player-ANM bundle by its PROT entry index and return
    /// the LZS-decoded bytes. Empty if the entry doesn't carry a bundle.
    pub fn player_anm_decoded(&self, prot_index: u32) -> Vec<u8> {
        let toc = match parse_prot_toc(&self.disc) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let Some(meta) = toc.into_iter().find(|e| e.index == prot_index) else {
            return Vec::new();
        };
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        let Some(buf) = self.disc.get(off..end) else {
            return Vec::new();
        };
        for desc_count in [6, 3, 5, 7] {
            let found = legaia_asset::player_anm::find_in_entry(buf, desc_count);
            if let Some(b) = found.into_iter().next() {
                return b.decoded;
            }
        }
        Vec::new()
    }

    /// Raw bytes of one record from the player-ANM bundle at `prot_index`.
    /// Includes the per-record header (`a`, `b`, `marker_1 = 0x080C`, `flag`),
    /// the 8-byte per-anim prologue, and the
    /// `(frame_count × bone_count × 8)` byte frame table.
    pub fn player_anm_record_bytes(&self, prot_index: u32, record_index: u32) -> Vec<u8> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return Vec::new();
        };
        bundle.record_bytes(record_index as usize).to_vec()
    }

    /// Decoded per-record header for one player-ANM record. Returned as a
    /// `Vec<i32>` packed as `[a, b, marker_1, flag, bone_count, frame_count,
    /// frame0_bone0_u8[0..8]]` - total 14 entries (the 8 bytes after the
    /// header are bone 0 of frame 0's TR entry, since the body sits
    /// immediately after the 8-byte header - there is no prologue).
    /// Returns an empty Vec on out-of-range record or size-invariant failure.
    pub fn player_anm_record_header(&self, prot_index: u32, record_index: u32) -> Vec<i32> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record(record_index as usize) else {
            return Vec::new();
        };
        let mut out = vec![
            rec.a as i32,
            rec.b as i32,
            rec.marker_1 as i32,
            rec.flag as i32,
            rec.bone_count as i32,
            rec.frame_count as i32,
        ];
        let bf = bundle.bone_frame_bytes(record_index as usize, 0, 0);
        for i in 0..8 {
            out.push(*bf.get(i).unwrap_or(&0) as i32);
        }
        out
    }

    /// Per-frame bone-transform table for one player-ANM record, packed as
    /// `i16` LE for ease of JS-side `Int16Array` overlay.
    ///
    /// Layout: `frame_count * bone_count * 4 i16` (`8` bytes per (bone, frame)
    /// entry, read as 4 little-endian `i16`s). Returns an empty Vec on
    /// out-of-range record or size-invariant failure.
    ///
    /// The semantic meaning of the 4 i16s per (bone, frame) entry is the
    /// still-open thread (see `docs/formats/anm.md` § "Open threads"). The
    /// working hypothesis is `(rot_x, rot_y, rot_z, _flag)` in PSX 12-bit
    /// fixed-point (4096 = 360°). The viewer applies this and lets you see
    /// what motion the bytes describe.
    pub fn player_anm_record_frames(&self, prot_index: u32, record_index: u32) -> Vec<u8> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record(record_index as usize) else {
            return Vec::new();
        };
        let bone_count = rec.bone_count as usize;
        let frame_count = rec.frame_count as usize;
        let mut out = Vec::with_capacity(frame_count * bone_count * 8);
        for f in 0..frame_count {
            let frame = bundle.frame_bytes(record_index as usize, f);
            if frame.len() != bone_count * 8 {
                return Vec::new();
            }
            out.extend_from_slice(frame);
        }
        out
    }

    /// Player-ANM record frames decoded into the same pose format the
    /// site's `MonsterMeshView` animator consumes:
    /// `Int32Array`, `6` entries per part per frame, as
    /// `[tx, ty, tz, rx, ry, rz]`.
    ///
    /// Each 8-byte (bone, frame) entry is decoded as the retail engine does
    /// it (`FUN_8001BE80`): bytes 0..4 hold three signed 12-bit translation
    /// values (joint offset in actor-local space, PSX model units), bytes
    /// 5/6/7 hold three u8 rotation angles that map to PSX 12-bit angles via
    /// `<< 4` (so the JS animator's `4096`-unit convention applies
    /// directly).
    ///
    /// The transforms are **absolute** per frame (NOT delta-from-frame-0):
    /// frame 0 carries the rest-pose assembly transform that places each
    /// TMD object at its joint position with its rest-pose orientation.
    /// Applying these to objects whose vertices are in object-local space
    /// produces the assembled character.
    ///
    /// The output is padded to `target_part_count` parts (typically the
    /// TMD's `nobj`) - bones beyond the record's own `bone_count` get
    /// identity transforms so the un-animated parts (e.g. field-form
    /// equipment templates at groups 10/11) stay at their TMD-local
    /// origin. Pass `0` to leave the part count at the record's own
    /// bone_count.
    pub fn player_anm_record_pose_frames(
        &self,
        prot_index: u32,
        record_index: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return Vec::new();
        };
        let Ok(rec) = bundle.record(record_index as usize) else {
            return Vec::new();
        };
        let anm_bone_count = rec.bone_count as usize;
        let frame_count = rec.frame_count as usize;
        let part_count = (target_part_count as usize).max(anm_bone_count);
        let mut out = Vec::with_capacity(frame_count * part_count * 6);
        for f in 0..frame_count {
            #[allow(clippy::needless_range_loop)]
            for p in 0..part_count {
                if p < anm_bone_count {
                    let Some(t) = bundle.bone_transform(record_index as usize, f, p) else {
                        return Vec::new();
                    };
                    out.push(t.t_x);
                    out.push(t.t_y);
                    out.push(t.t_z);
                    out.push(t.r_x);
                    out.push(t.r_y);
                    out.push(t.r_z);
                } else {
                    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
                }
            }
        }
        out
    }

    /// `[bone_count, frame_count]` for one player-ANM record so the JS
    /// animator can size its scratch buffers without re-walking the bundle.
    /// Empty `[0, 0]` if the record doesn't exist or fails size invariants.
    pub fn player_anm_record_dims(&self, prot_index: u32, record_index: u32) -> Vec<u32> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return vec![0, 0];
        };
        match bundle.record(record_index as usize) {
            Ok(r) => vec![r.bone_count as u32, r.frame_count as u32],
            Err(_) => vec![0, 0],
        }
    }
}
