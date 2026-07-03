//! Monster stat-archive, mesh, animation + glTF exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    /// Decode the global monster stat archive (PROT entry 867, the
    /// `battle_data` block's extended footprint) into a JSON array of every
    /// populated record. Sony bytes never leave the browser - the archive is
    /// LZS-decoded from the user's own loaded disc, the same client-side model
    /// the rest of this viewer uses; nothing is shipped with the static site.
    ///
    /// Shape:
    /// ```json
    /// { "records": [ { "id": u16, "name": "Gimard", "hp": u16, "mp": u16,
    ///                  "stats": [u16; 6], "battle_stats": [u16; 6],
    ///                  "magic_count": u8, "gold": u16,
    ///                  "element": u8, "element_name": "fire"|null,
    ///                  "exp": u16, "drop_item": u8, "drop_chance_pct": u8,
    ///                  "steal_item": u8, "steal_item_name": "Incense"|null,
    ///                  "steal_chance_pct": u8,
    ///                  "spells": [ { "id": u8, "agl_cost": u8,
    ///                               "castable": bool } ] }, ... ] }
    /// ```
    ///
    /// Returns `{"records":[]}` when the entry isn't present (a standalone-TIM
    /// or regional load that lacks PROT 867), or `{"error":...}` on a genuine
    /// LZS decode failure.
    pub fn monster_archive_json(&self) -> String {
        const MONSTER_ARCHIVE_INDEX: u32 = 867;
        let Some(meta) = parse_prot_toc(&self.disc)
            .and_then(|es| es.into_iter().find(|e| e.index == MONSTER_ARCHIVE_INDEX))
        else {
            return "{\"records\":[]}".to_string();
        };
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        if end > self.disc.len() {
            return "{\"records\":[]}".to_string();
        }
        let records = match legaia_asset::monster_archive::records(&self.disc[off..end]) {
            Ok(r) => r,
            Err(e) => return format!("{{\"error\":\"monster archive decode failed: {e}\"}}"),
        };
        // Resolve a raw drop id into its in-game name via the SCUS item-name
        // table (present only on full-disc loads). `null` falls the JS back to
        // the raw id.
        let drop_name = |id: u8| -> Option<&str> {
            (id != 0)
                .then(|| self.item_names.as_ref().and_then(|t| t.name(id)))
                .flatten()
        };
        // Resolve a monster's steal entry from the SCUS steal table (Evil God
        // Icon). Returns `(item_id, item_name, chance_pct)` only when the monster
        // is stealable (item != 0 && chance != 0); `(0, None, 0)` otherwise.
        let steal_of = |id: u16| -> (u8, Option<&str>, u8) {
            match self.steal_table.as_ref().and_then(|t| t.entry(id)) {
                Some(e) if e.is_stealable() => (e.item_id, drop_name(e.item_id), e.chance_pct),
                _ => (0, None, 0),
            }
        };
        // Resolve a monster's global magic-attack ids into named spells via the
        // SCUS spell table. Each entry carries the name + MP cost; `null` name
        // falls the JS back to the raw id.
        let magic_attacks = |ids: &[u8]| -> Vec<serde_json::Value> {
            ids.iter()
                .map(|&id| {
                    serde_json::json!({
                        "id": id,
                        "name": self.spell_names.as_ref().and_then(|t| t.name(id)),
                        "mp": self.spell_names.as_ref().and_then(|t| t.mp(id)),
                    })
                })
                .collect()
        };
        let arr: Vec<serde_json::Value> = records
            .into_iter()
            .map(|r| {
                let (steal_item, steal_item_name, steal_chance) = steal_of(r.id);
                serde_json::json!({
                    "id": r.id,
                    "name": r.name,
                    "hp": r.hp,
                    "mp": r.mp,
                    "stats": r.stats,
                    // The combat stats the battle loader installs into the live
                    // actor (FUN_80054cb0 boost: ATK ×5/4, UDF/LDF ×2, INT ×9/8),
                    // i.e. what the player actually fights - the raw `stats`
                    // understate it. See `MonsterRecord::battle_stats`.
                    "battle_stats": r.battle_stats(),
                    "magic_count": r.magic_count,
                    "gold": r.gold,
                    "exp": r.exp,
                    "element": r.element,
                    "element_name": legaia_asset::element_affinity::Element::from_id(r.element)
                        .map(|e| e.name()),
                    "drop_item": r.drop_item,
                    "drop_item_name": drop_name(r.drop_item),
                    "drop_chance_pct": r.drop_chance_pct,
                    "steal_item": steal_item,
                    "steal_item_name": steal_item_name,
                    "steal_chance_pct": steal_chance,
                    "spells": r.spells.iter().map(|s| serde_json::json!({
                        "id": s.id,
                        "agl_cost": s.agl_cost,
                        "castable": s.is_castable(),
                    })).collect::<Vec<_>>(),
                    "magic": magic_attacks(&r.magic_attacks),
                })
            })
            .collect();
        serde_json::json!({ "records": arr }).to_string()
    }

    /// Slice of the disc holding the monster stat archive (PROT entry 867,
    /// extended footprint). Shared by [`Self::monster_archive_json`] and the
    /// per-monster mesh accessors.
    fn monster_archive_slice(&self) -> Option<&[u8]> {
        const MONSTER_ARCHIVE_INDEX: u32 = 867;
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == MONSTER_ARCHIVE_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    /// Build monster `id`'s embedded mesh (the Legaia TMD at archive-block
    /// `+0x04`) as a [`legaia_tmd::mesh::VramMesh`]. All monster prims are
    /// textured, so this keeps the full geometry with per-vertex UVs +
    /// CBA/TSB; the JS side textures it from the decoded pool (see
    /// [`Self::monster_texture_indices`]) and directional-lights it via the
    /// per-vertex normals. Returns `None` for a filler / out-of-range id.
    fn build_monster_mesh(&self, id: u16) -> Option<legaia_tmd::mesh::VramMesh> {
        let slice = self.monster_archive_slice()?;
        let mesh = legaia_asset::monster_archive::mesh(slice, id).ok()??;
        let tmd = legaia_tmd::parse(mesh.tmd_bytes()).ok()?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, mesh.tmd_bytes()))
    }

    /// Decode monster `id`'s texture pool (archive-block `+0x08`). `None` for a
    /// filler / out-of-range id or a slot with no pool.
    fn build_monster_texture(
        &self,
        id: u16,
    ) -> Option<legaia_asset::monster_archive::MonsterTexture> {
        let slice = self.monster_archive_slice()?;
        let mesh = legaia_asset::monster_archive::mesh(slice, id).ok()??;
        mesh.texture()
    }

    /// Per-vertex `[x, y, z]` positions for monster `id`'s mesh (flat array,
    /// 3 floats per vertex). Empty if the id has no mesh.
    pub fn monster_mesh_positions(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex smooth normals for monster `id`'s mesh (parallel to
    /// [`Self::monster_mesh_positions`]).
    pub fn monster_mesh_normals(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for monster `id`'s mesh (`u32`, multiple of 3).
    pub fn monster_mesh_indices(&self, id: u16) -> Vec<u32> {
        self.build_monster_mesh(id)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Bounding-sphere `[cx, cy, cz, r]` for monster `id`'s mesh, so the JS
    /// side can frame the model without re-parsing the geometry.
    pub fn monster_mesh_bounds(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let c = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let d = [
            (hi[0] - lo[0]) * 0.5,
            (hi[1] - lo[1]) * 0.5,
            (hi[2] - lo[2]) * 0.5,
        ];
        let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
        vec![c[0], c[1], c[2], r]
    }

    /// Per-vertex texture coords for monster `id`'s mesh, normalised to
    /// `[0, 1]` against the texture-page dimensions (parallel to
    /// [`Self::monster_mesh_positions`], 2 floats per vertex). Empty if the id
    /// has no mesh or no texture.
    pub fn monster_mesh_uvs(&self, id: u16) -> Vec<f32> {
        let (Some(mesh), Some(tex)) = (self.build_monster_mesh(id), self.build_monster_texture(id))
        else {
            return Vec::new();
        };
        // Texel-centre offset so the JS shader's NEAREST sample lands on the
        // intended texel rather than rounding to its neighbour at edges.
        let (w, h) = (tex.width as f32, tex.height as f32);
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[(uv[0] as f32 + 0.5) / w, (uv[1] as f32 + 0.5) / h]);
        }
        out
    }

    /// Per-vertex palette index (`cba & 0x3F`) for monster `id`'s mesh, as
    /// floats (parallel to [`Self::monster_mesh_positions`]). The JS shader
    /// uses it to pick the row of the palette texture.
    pub fn monster_mesh_palette_index(&self, id: u16) -> Vec<f32> {
        self.build_monster_mesh(id)
            .map(|m| m.cba_tsb.iter().map(|ct| (ct[0] & 0x3F) as f32).collect())
            .unwrap_or_default()
    }

    /// Monster `id`'s 4bpp texture page as one palette index (`0..=15`) per
    /// texel, row-major (`width * height` bytes). Upload as an `R8UI`/`R8`
    /// texture and pair with [`Self::monster_texture_palette_rgba`]. Empty if
    /// the id has no texture.
    pub fn monster_texture_indices(&self, id: u16) -> Vec<u8> {
        self.build_monster_texture(id)
            .map(|t| t.indices)
            .unwrap_or_default()
    }

    /// Monster `id`'s 15 palettes flattened to a `15 * 16` RGBA8 row (palette
    /// `p`, colour `c` at pixel `p * 16 + c`). Index-0 transparent colours
    /// carry alpha 0. Empty if the id has no texture.
    pub fn monster_texture_palette_rgba(&self, id: u16) -> Vec<u8> {
        self.build_monster_texture(id)
            .map(|t| t.palette_rgba())
            .unwrap_or_default()
    }

    /// `[width, height]` of monster `id`'s texture page in texels (128 or 256
    /// wide, always 256 tall). `[0, 0]` if the id has no texture.
    pub fn monster_texture_dims(&self, id: u16) -> Vec<u32> {
        self.build_monster_texture(id)
            .map(|t| vec![t.width as u32, t.height as u32])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Per-vertex TMD object (body-part) index for monster `id`'s mesh, parallel
    /// to [`Self::monster_mesh_positions`]. The JS idle-animation player uses it
    /// to apply each animated part's per-frame transform. Empty if no mesh.
    pub fn monster_mesh_object_ids(&self, id: u16) -> Vec<u32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(Some(mesh)) = legaia_asset::monster_archive::mesh(slice, id).ok() else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1
    }

    /// `[part_count, frame_count]` for monster `id`'s **idle** animation (action
    /// index 0). `[0, 0]` if the slot has no decodable animation. Pair with
    /// [`Self::monster_idle_animation_frames`].
    pub fn monster_idle_animation_header(&self, id: u16) -> Vec<u32> {
        let Some(slice) = self.monster_archive_slice() else {
            return vec![0, 0];
        };
        match legaia_asset::monster_archive::idle_animation(slice, id)
            .ok()
            .flatten()
        {
            Some(a) => vec![a.part_count as u32, a.frame_count as u32],
            None => vec![0, 0],
        }
    }

    /// Monster `id`'s idle animation keyframes as a flat `i32` array, six values
    /// per part per frame: `[tx, ty, tz, rx, ry, rz]`. Frame `f`, part `p`,
    /// component `c` is at `(f * part_count + p) * 6 + c`. Translations are
    /// signed model units; rotations are unsigned 12-bit angles (`4096` = a full
    /// turn). Empty if the slot has no decodable idle animation.
    pub fn monster_idle_animation_frames(&self, id: u16) -> Vec<i32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(anim) = legaia_asset::monster_archive::idle_animation(slice, id)
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
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

    /// Metadata for **every** decodable action animation of monster `id`, as a
    /// JSON array in `+0x4C` action-table order:
    /// `[{"action_id":N,"part_count":P,"frame_count":F}, ...]`. Array index `0`
    /// is the idle loop (see [`Self::monster_idle_animation_header`]); the rest
    /// are the monster's attack / spell / special actions. The array index is
    /// the handle the JS viewer passes to [`Self::monster_animation_frames_at`]
    /// to fetch a given action's keyframes. `"[]"` if the slot is empty / filler
    /// or carries no decodable animation.
    pub fn monster_animations_json(&self, id: u16) -> String {
        let Some(slice) = self.monster_archive_slice() else {
            return "[]".to_string();
        };
        let anims = match legaia_asset::monster_archive::animations(slice, id) {
            Ok(Some(a)) => a,
            _ => return "[]".to_string(),
        };
        let labels = legaia_asset::monster_archive::action_labels(&anims);
        let arr: Vec<serde_json::Value> = anims
            .iter()
            .enumerate()
            .map(|(i, a)| {
                serde_json::json!({
                    "action_id": a.action_id,
                    "part_count": a.part_count,
                    "frame_count": a.frame_count,
                    "label": labels.get(i),
                })
            })
            .collect();
        serde_json::Value::Array(arr).to_string()
    }

    /// Keyframes for monster `id`'s action animation at array `index` (the
    /// position in [`Self::monster_animations_json`]). Same flat layout as
    /// [`Self::monster_idle_animation_frames`]: six `i32` per part per frame,
    /// `[tx, ty, tz, rx, ry, rz]`, with frame `f` / part `p` / component `c` at
    /// `(f * part_count + p) * 6 + c`. Empty if the index is out of range or the
    /// slot has no decodable animation.
    pub fn monster_animation_frames_at(&self, id: u16, index: u32) -> Vec<i32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(anims) = legaia_asset::monster_archive::animations(slice, id)
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
        let Some(anim) = anims.get(index as usize) else {
            return Vec::new();
        };
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

    /// Monster `id`'s mesh + baked texture + **all** action animations packed
    /// into one binary glTF (`.glb`) blob - the universal format that carries
    /// geometry, material, and animation together (Blender / three.js / etc.).
    /// Each TMD object becomes an animated node; the texture is baked into a
    /// per-palette atlas. Empty if the slot has no exportable mesh.
    pub fn monster_glb(&self, id: u16) -> Vec<u8> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        legaia_asset::monster_gltf::export_glb(slice, id)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Player-character pack (PROT 0874 §0) - Vahn / Noa / Gala + 2 auxiliary
    //
    // Sister accessors of `monster_*`: surface the five character TMDs the
    // engine keeps resident at `DAT_8007C018[0..=4]`. The active-party slots
    // expose the `FUN_8001EBEC` equipment swap so the JS viewer can flip the
    // visible weapon-bearing group descriptor in place.
    // -----------------------------------------------------------------------
}
