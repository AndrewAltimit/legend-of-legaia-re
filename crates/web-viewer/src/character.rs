//! Field + battle character pack/mesh/palette exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    /// JSON summary of the five character-pack slots.
    ///
    /// Shape:
    /// ```json
    /// { "slots": [
    ///     { "slot": 0, "label": "Vahn", "disc_nobj": 12,
    ///       "tmd_bytes": 13220,
    ///       "patch": { "patched_group_index": 0,
    ///                  "equip_byte_record_offset": 406 } },
    ///     ...
    ///   ],
    ///   "patched_group_offset": 12,
    ///   "group_descriptor_bytes": 28,
    ///   "equip_group_zero_offset": 320,
    ///   "equip_group_nonzero_offset": 292
    /// }
    /// ```
    /// `patch` is present only for the 3 active-party slots (0..=2); slots
    /// 3/4 carry the auxiliary actors with no equipment swap. Returns
    /// `{"slots":[],"error":"..."}` when the disc is missing PROT 0874 or
    /// the LZS section fails to decode.
    pub fn character_pack_json(&self) -> String {
        let Some(slice) = self.character_pack_slice() else {
            return r#"{"slots":[]}"#.to_string();
        };
        let pack = match legaia_asset::character_pack::parse(slice) {
            Ok(p) => p,
            Err(e) => {
                return format!(r#"{{"slots":[],"error":"character pack: {e}"}}"#);
            }
        };
        let active = legaia_asset::character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;
        let slots: Vec<serde_json::Value> = pack
            .slots()
            .iter()
            .map(|s| {
                let patch = active
                    .iter()
                    .find(|p| (p.slot as usize) == s.slot)
                    .map(|p| {
                        serde_json::json!({
                            "patched_group_index": p.patched_group_index,
                            "equip_byte_record_offset": p.equip_byte_record_offset,
                        })
                    });
                serde_json::json!({
                    "slot": s.slot,
                    "label": legaia_asset::character_pack::slot_label(s.slot),
                    "disc_nobj": s.disc_nobj,
                    "tmd_bytes": s.tmd_bytes.len(),
                    "patch": patch,
                })
            })
            .collect();
        serde_json::json!({
            "slots": slots,
            "first_group_descriptor_offset":
                legaia_asset::character_pack::FIRST_GROUP_DESCRIPTOR_OFFSET,
            "group_descriptor_bytes":
                legaia_asset::character_pack::GROUP_DESCRIPTOR_BYTES,
            "equip_group_zero_offset":
                legaia_asset::character_pack::EQUIP_GROUP_ZERO_OFFSET,
            "equip_group_nonzero_offset":
                legaia_asset::character_pack::EQUIP_GROUP_NONZERO_OFFSET,
        })
        .to_string()
    }

    /// Slice of the disc holding PROT 0874 (`befect_data` extended footprint).
    /// Shared by every `character_*` accessor.
    fn character_pack_slice(&self) -> Option<&[u8]> {
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == legaia_asset::character_pack::PROT_ENTRY_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    /// Build slot `slot`'s renderable mesh, optionally with the equipment
    /// swap applied. `equip` of `None` returns the disc-form mesh
    /// (with retail's 10-group cap applied so groups 10/11 templates aren't
    /// drawn directly); `Some(byte)` runs the FUN_8001EBEC patch with that
    /// byte before parsing.
    fn build_character_mesh(
        &self,
        slot: usize,
        equip: Option<u8>,
    ) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let raw = self.character_pack_slice()?;
        let pack = legaia_asset::character_pack::parse(raw).ok()?;
        let cslot = pack.slot(slot)?;
        // Retail caps the active-party slots at 10 live groups (FUN_8001E890);
        // mirror that so groups 10/11 (the equip templates) aren't drawn as
        // visible geometry. The patched copy still contains the templates at
        // their disc offsets but `parse` walks only the first `nobj`.
        let mut tmd_bytes = if let Some(equip_byte) = equip {
            let active = legaia_asset::character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;
            if let Some(patch) = active.iter().find(|p| (p.slot as usize) == slot) {
                legaia_asset::character_pack::equipment_swap::apply(
                    &cslot.tmd_bytes,
                    *patch,
                    equip_byte,
                )
            } else {
                cslot.tmd_bytes.clone()
            }
        } else {
            cslot.tmd_bytes.clone()
        };
        if cslot.is_active_party() && tmd_bytes.len() >= 0x0C {
            // Overwrite TMD header `nobj` to 10 - the retail cap.
            let cap = 10u32.to_le_bytes();
            tmd_bytes[0x08..0x0C].copy_from_slice(&cap);
        }
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        Some((tmd, tmd_bytes))
    }

    /// Convenience: return the renderable `VramMesh` for slot `slot` under
    /// the chosen equipment toggle. Uses the lenient extractor that keeps
    /// flat-shaded primitives (the bulk of field-form character body
    /// parts) - the standard one would drop them.
    fn build_character_vram_mesh(
        &self,
        slot: usize,
        equip: Option<u8>,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, bytes) = self.build_character_mesh(slot, equip)?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(&tmd, &bytes).0)
    }

    /// Per-vertex positions for the player character at pack slot `slot`,
    /// optionally with the equipment swap applied (`equip_byte` < 0 means
    /// "no swap, draw disc-form mesh"). Empty if `slot` is out of range or
    /// the disc isn't loaded.
    pub fn character_mesh_positions(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex normals parallel to [`Self::character_mesh_positions`].
    pub fn character_mesh_normals(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for the player character at pack slot `slot`,
    /// `u32`, multiple of 3.
    pub fn character_mesh_indices(&self, slot: u32, equip_byte: i32) -> Vec<u32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        self.build_character_vram_mesh(slot as usize, equip)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` integer texel coords (parallel to
    /// [`Self::character_mesh_positions`], 2 i32 per vertex). The site page
    /// pairs these with the PROT 0876 atlas page to do its own NEAREST
    /// sample; we keep the integer texels here instead of normalising
    /// because the atlas dimensions aren't surfaced yet.
    pub fn character_mesh_uvs(&self, slot: u32, equip_byte: i32) -> Vec<i32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (CLUT-base / texture-page descriptor) so the
    /// JS shader can resolve VRAM texel + palette per the standard PSX TMD
    /// model. `2 u32` per vertex, parallel to [`Self::character_mesh_positions`].
    pub fn character_mesh_cba_tsb(&self, slot: u32, equip_byte: i32) -> Vec<u32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Bounding-sphere `[cx, cy, cz, r]` so the JS viewer can frame the model.
    /// Uses `centroid_bounds` so asymmetric poses (weapon extended, arm out)
    /// don't pull the camera target off the body.
    pub fn character_mesh_bounds(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&mesh.positions)
    }

    /// Per-vertex TMD object index for the player character at pack slot
    /// `slot`, parallel to [`Self::character_mesh_positions`]. The JS-side
    /// player-ANM animator uses it to apply per-bone (per-object) transforms
    /// without re-uploading geometry.
    pub fn character_mesh_object_ids(&self, slot: u32, equip_byte: i32) -> Vec<u32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some((tmd, bytes)) = self.build_character_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids_lenient(&tmd, &bytes).1
    }

    /// Per-vertex flat/gouraud shading attribute for the field-character
    /// **hybrid** render, parallel to [`Self::character_mesh_positions`]: 4
    /// bytes per vertex `[r, g, b, textured_flag]`. The field-form player mesh
    /// mixes textured prims (face / skin / clothing that sample the PROT 0874
    /// §2 atlas - `textured_flag == 1`) with untextured flat / gouraud prims
    /// (the bulk of the body - `textured_flag == 0`) that carry per-vertex RGB
    /// in the TMD instead of UVs. The shader samples VRAM for textured verts
    /// and uses `[r, g, b]` for untextured verts, so the body parts the pure
    /// textured path would discard render in their real colours. Vertex order
    /// matches the other `character_mesh_*` getters (same TMD walk).
    pub fn character_mesh_flat_colors(&self, slot: u32, equip_byte: i32) -> Vec<u8> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some((tmd, bytes)) = self.build_character_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let (_mesh, _oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &bytes);
        // The flag rides in the alpha byte; the JS binds this attribute
        // normalised, so emit 255 (→ 1.0) for textured and 0 for untextured.
        let mut out = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            out.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        out
    }

    /// Build the 1 MB PSX VRAM with the **field-character textures** (PROT
    /// 0874 **section 2**) uploaded, so the Field-form meshes render textured.
    ///
    /// Section 2 of the `player.lzs` container is an 8-TIM pack; entries 1/2/3
    /// are the Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with their
    /// CLUTs on row 478 (cols 0..63 / 64..127 / 128..191). Each TIM is uploaded
    /// via the retail `FUN_800198e0` semantic - image at its declared rect, CLUT
    /// as a **flat horizontal strip** (`w*h` colours at one row), STP off - so
    /// the meshes' per-primitive CBA columns sample the right palettes. Byte-
    /// exact against a live field VRAM dump (see
    /// [`legaia_asset::field_char_textures`]). The Field form renders against
    /// this VRAM through the same paletted pipeline the Battle form uses.
    pub fn field_char_vram_bytes(&self) -> Vec<u8> {
        let Some(raw) = self.character_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::field_char_textures::parse(raw) else {
            return Vec::new();
        };
        let mut vram = legaia_tim::Vram::new();
        pack.upload_to_vram(&mut vram, false);
        vram.as_bytes().to_vec()
    }

    /// Raw disc-form TMD bytes for slot `slot` - the same bytes the engine
    /// installs into `DAT_8007C018[slot]`. Useful for an in-page .tmd
    /// download / debug round-trip.
    pub fn character_tmd_bytes(&self, slot: u32) -> Vec<u8> {
        let Some(raw) = self.character_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::character_pack::parse(raw) else {
            return Vec::new();
        };
        pack.slot(slot as usize)
            .map(|s| s.tmd_bytes.clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Battle-form character pack - PROT 1204 (`other5`).
    //
    // Sister pack to the field-form one above. Same 5-slot shape, but
    // higher-fidelity battle TMDs (typical disc-nobj 15/16/15 vs 12/12/12)
    // and an explicit 7-atlas trailer (256x256 4bpp TIMs at fixed stride).
    // ------------------------------------------------------------------

    /// JSON summary of PROT 1204 (`other5`) - the battle-form mesh pack:
    /// 5 TMD slots + 7 character-atlas TIMs. Shape:
    /// ```text
    /// {
    ///   "slots":   [{"slot":0,"label":"Vahn","disc_nobj":15,"tmd_bytes":33516,"file_offset":4}, ...],
    ///   "atlases": [{"atlas":0,"clut_fb_y":490,"tim_bytes":33316,"file_offset":154628}, ...],
    ///   "atlas_stride_bytes": 33316,
    ///   "first_atlas_offset": 154628
    /// }
    /// ```
    pub fn battle_char_pack_json(&self) -> String {
        let Some(slice) = self.battle_char_pack_slice() else {
            return r#"{"slots":[],"atlases":[]}"#.to_string();
        };
        let pack = match legaia_asset::battle_char_pack::parse(slice) {
            Ok(p) => p,
            Err(e) => {
                return format!(r#"{{"slots":[],"atlases":[],"error":"battle char pack: {e}"}}"#);
            }
        };
        let slots: Vec<serde_json::Value> = pack
            .slots()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "slot": s.slot,
                    "label": legaia_asset::battle_char_pack::slot_label(s.slot),
                    "disc_nobj": s.disc_nobj,
                    "tmd_bytes": s.tmd_bytes.len(),
                    "file_offset": s.file_offset,
                })
            })
            .collect();
        let atlases: Vec<serde_json::Value> = pack
            .atlases
            .iter()
            .map(|a| {
                serde_json::json!({
                    "atlas": a.atlas_index,
                    "clut_fb_y": a.clut_fb_y,
                    "tim_bytes": a.tim_bytes.len(),
                    "file_offset": a.file_offset,
                })
            })
            .collect();
        serde_json::json!({
            "slots": slots,
            "atlases": atlases,
            "atlas_stride_bytes": legaia_asset::battle_char_pack::ATLAS_STRIDE_BYTES,
            "first_atlas_offset": legaia_asset::battle_char_pack::FIRST_ATLAS_OFFSET,
        })
        .to_string()
    }

    fn battle_char_pack_slice(&self) -> Option<&[u8]> {
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    fn build_battle_char_mesh(&self, slot: usize) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let raw = self.battle_char_pack_slice()?;
        let pack = legaia_asset::battle_char_pack::parse(raw).ok()?;
        let cslot = pack.slot(slot)?;
        let tmd_bytes = cslot.tmd_bytes.clone();
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        Some((tmd, tmd_bytes))
    }

    fn build_battle_char_vram_mesh(&self, slot: usize) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, bytes) = self.build_battle_char_mesh(slot)?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes))
    }

    /// Per-vertex positions for the battle-form character at pack slot `slot`.
    pub fn battle_char_mesh_positions(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex normals for the battle-form character at slot `slot`.
    pub fn battle_char_mesh_normals(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for the battle-form character at slot `slot`.
    pub fn battle_char_mesh_indices(&self, slot: u32) -> Vec<u32> {
        self.build_battle_char_vram_mesh(slot as usize)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` integer texel coords for the battle-form character.
    pub fn battle_char_mesh_uvs(&self, slot: u32) -> Vec<i32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` for the battle-form character.
    pub fn battle_char_mesh_cba_tsb(&self, slot: u32) -> Vec<u32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Bounding-sphere `[cx, cy, cz, r]` for the battle-form character.
    /// Uses the **vertex centroid** (mean position) rather than the AABB
    /// midpoint, so asymmetric poses (e.g. Vahn's stance with the weapon
    /// extended past the body's X axis) don't pull the camera target off the
    /// torso. Radius is the max distance from the centroid to any vertex.
    pub fn battle_char_mesh_bounds(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&mesh.positions)
    }

    /// Per-vertex TMD object index for the battle-form character at slot
    /// `slot`, parallel to [`Self::battle_char_mesh_positions`]. The JS-side
    /// player-ANM animator uses it to apply per-bone (per-object) transforms.
    pub fn battle_char_mesh_object_ids(&self, slot: u32) -> Vec<u32> {
        let Some((tmd, bytes)) = self.build_battle_char_mesh(slot as usize) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &bytes).1
    }

    /// Raw disc-form TMD bytes for battle-form slot `slot`.
    pub fn battle_char_tmd_bytes(&self, slot: u32) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        pack.slot(slot as usize)
            .map(|s| s.tmd_bytes.clone())
            .unwrap_or_default()
    }

    /// Raw TIM bytes for battle-form atlas `atlas` (0..=6). 256x256 4bpp with
    /// a 256x1 sub-CLUT row inside the TIM block.
    pub fn battle_char_atlas_bytes(&self, atlas: u32) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        pack.atlas(atlas as usize)
            .map(|a| a.tim_bytes.clone())
            .unwrap_or_default()
    }

    /// Build the 1 MB PSX VRAM with each of PROT 1204's seven atlas TIMs
    /// uploaded **with its bundled CLUT** at the declared `(fb_x, fb_y)`
    /// (rows 490..495, 497). These bundled sub-CLUTs are the pack's **authoring
    /// palette** - what the Baka Fighter minigame renders with directly. Both
    /// the Battle and Baka Fighter forms on the site render against this VRAM
    /// with the mesh's nominal CBA ([`Self::battle_char_mesh_cba_tsb`]).
    ///
    /// A real turn-based battle relocates the same geometry + textures into a
    /// packed per-slot VRAM band (rows 481..483) and recolours it with a
    /// per-battle party palette that is a **separate, battle-allocated runtime
    /// asset** (resident at RAM `0x800ebee8`+, 480 B / 15 sub-CLUTs per char) -
    /// distinct from this bundled palette and **not recoverable from the disc by
    /// byte search** (see `docs/formats/character-mesh.md`). Until that palette's
    /// disc source is pinned (open thread - needs a battle-LOAD overlay capture),
    /// the Battle form is the bundled-palette render, visually identical to Baka.
    pub fn battle_char_vram_bytes(&self) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        let mut vram = legaia_tim::Vram::new();
        for atlas in &pack.atlases {
            if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                vram.upload_tim(&tim);
            }
        }
        vram.as_bytes().to_vec()
    }

    /// Battle VRAM with the **true per-battle palette** overlaid for the slots
    /// whose disc palette source is known. This is the colour-correct render a
    /// real turn-based battle produces - the party CLUTs decoded from the
    /// character's `edstati3` record (`FUN_80052FA0`, see
    /// [`legaia_asset::battle_char_palette`]) and STP-set onto the VRAM rows the
    /// mesh's nominal CBA samples.
    ///
    /// Vahn (slot 0, extraction PROT `0863` - the `PLAYER1` file, raw TOC
    /// `0x361`; see `docs/formats/cdname.md` § numbering space) is validated
    /// byte-exact against a live battle VRAM capture (his tutorial-equipped
    /// state via [`legaia_asset::battle_char_palette::parse_record`]). Noa
    /// (slot 1, extraction `0864`) and Gala (slot 2, extraction `0865`) use the
    /// equipment-robust [`legaia_asset::battle_char_palette::collect_palette`]
    /// - record0 + the section separators' unequipped-default CLUTs, filtered
    /// to the columns each mesh samples (validated against a full-party
    /// capture: Noa ~98%, Gala 100%). All three player files load by
    /// `char + 0x360` → `FUN_8003e8a8` → `toc[idx+2]` (a sector offset into
    /// PROT.DAT); extraction entries `0863`/`0864`/`0865` begin exactly at
    /// those player-file offsets. The Baka Fighter form keeps
    /// [`Self::battle_char_vram_bytes`] (the bundled palette is the correct
    /// minigame colouring).
    pub fn battle_char_vram_bytes_battle(&self) -> Vec<u8> {
        let mut vram = self.battle_char_vram_bytes();
        if vram.is_empty() {
            return vram;
        }
        // Vahn (slot 0): the validated tutorial-equipped assembly, from the
        // canonical PLAYER1 entry (record0 leads the file).
        if let Some(pal) = self.edstati3_palette(863) {
            overlay_palette_rows(&mut vram, &self.battle_char_clut_rows(0), &pal);
        }
        // Noa (slot 1, PROT 0864 rec0=0) and Gala (slot 2, PROT 0865 rec0=0):
        // equipment-robust collection filtered to the columns each mesh samples.
        // (0865's entry begins exactly at Gala's player-file region in PROT.DAT.)
        for &(prot_index, slot) in &[(864u32, 1usize), (865, 2)] {
            if let Some(pal) = self.collected_palette(prot_index, slot) {
                overlay_palette_rows(&mut vram, &self.battle_char_clut_rows(slot), &pal);
            }
        }
        vram
    }

    /// Parse the battle CLUT bands out of a character's `edstati3` PROT entry
    /// (the fixed-stride [`parse_record`](legaia_asset::battle_char_palette::parse_record)
    /// assembly - exact for Vahn).
    fn edstati3_palette(
        &self,
        prot_index: u32,
    ) -> Option<legaia_asset::battle_char_palette::BattleCharPalette> {
        let slice = self.prot_entry(prot_index)?;
        let rec0 = legaia_asset::battle_char_palette::find_record0(slice)?;
        legaia_asset::battle_char_palette::parse_record(slice, rec0).ok()
    }

    /// Equipment-robust palette for `mesh_slot`'s character from PROT `prot_index`
    /// (record0 at file offset 0), filtered to the columns the mesh samples.
    fn collected_palette(
        &self,
        prot_index: u32,
        mesh_slot: usize,
    ) -> Option<legaia_asset::battle_char_palette::BattleCharPalette> {
        let cols = self.battle_char_clut_cols(mesh_slot);
        if cols.is_empty() {
            return None;
        }
        let slice = self.prot_entry(prot_index)?;
        legaia_asset::battle_char_palette::collect_palette(slice, 0, &cols).ok()
    }

    fn prot_entry(&self, prot_index: u32) -> Option<&[u8]> {
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == prot_index)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    /// Distinct VRAM CLUT rows the battle mesh at `slot` samples (decoded from
    /// each primitive's CBA: `row = (cba >> 6) & 0x1FF`). The true palette is
    /// written to each of these rows so the mesh's nominal CBA picks it up.
    fn battle_char_clut_rows(&self, slot: usize) -> Vec<u16> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot) else {
            return Vec::new();
        };
        let mut rows: Vec<u16> = mesh.cba_tsb.iter().map(|ct| (ct[0] >> 6) & 0x1FF).collect();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Distinct CLUT x-columns the battle mesh at `slot` samples
    /// (`(cba & 0x3F) * 16`) - the band bases that belong to this character.
    fn battle_char_clut_cols(&self, slot: usize) -> Vec<u16> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot) else {
            return Vec::new();
        };
        let mut cols: Vec<u16> = mesh.cba_tsb.iter().map(|ct| (ct[0] & 0x3F) * 16).collect();
        cols.sort_unstable();
        cols.dedup();
        cols
    }

    // ------------------------------------------------------------------
    // Player ANM bundles - per-scene asset bundle, section 2, type 0x05
    // ("MOVE" label but canonical ANM content with marker_1 = 0x080C).
    // See `legaia_asset::player_anm` + docs/formats/anm.md.
}

/// Write a character's true battle palette into the 1 MB PSX VRAM byte buffer.
/// Each band's STP-set colours (`PaletteBand::vram_words`) are written at
/// `(row, base + i)` for every CLUT row the mesh samples - the runtime collapses
/// a character's two nominal rows to one palette, so writing both is equivalent.
fn overlay_palette_rows(
    vram: &mut [u8],
    rows: &[u16],
    pal: &legaia_asset::battle_char_palette::BattleCharPalette,
) {
    const VRAM_W: usize = 1024;
    for &row in rows {
        for band in &pal.bands {
            for (i, w) in band.vram_words().iter().enumerate() {
                let col = band.base as usize + i;
                if col >= VRAM_W {
                    break;
                }
                let off = (row as usize * VRAM_W + col) * 2;
                if off + 2 <= vram.len() {
                    vram[off] = (*w & 0xFF) as u8;
                    vram[off + 1] = (*w >> 8) as u8;
                }
            }
        }
    }
}
