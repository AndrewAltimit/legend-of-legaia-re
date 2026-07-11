//! Scene/kingdom, walk-ground, continent, pack, ocean, slot-4 wireframe + world-map menu exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    /// Open a world-map kingdom's 7-asset bundle, LZS-decode slot 0
    /// (TIM_LIST) into a shared VRAM, and LZS-decode slot 1 (TMD pack) for
    /// per-slot mesh access. Returns the pack count (= number of scene-pool
    /// TMDs available to `pack_mesh`).
    ///
    /// `prot_base` is the kingdom's leading PROT entry index - 85 for Drake
    /// (`map01`), 244 for Sebucus (`map02`), 391 for Karisto (`map03`).
    /// Either the `scene_scripted_asset_table` (PROT base) or the bare
    /// `scene_asset_table` (PROT base+1) works; the detector finds the
    /// 7-asset table at the first 0x800-aligned offset whose `u32_le[0] == 7`
    /// and `descriptor[0].data_offset == 0x40`.
    ///
    /// Implementation mirrors `FUN_8001F05C case 2` (TMD-pack dispatch): the
    /// pack is `[u32 count][u32 word_offsets[count]][TMD bodies]` with
    /// offsets in 4-byte words (`puVar1 + puVar5[1]` on `uint*`). The
    /// VRAM upload is unconditional (every TIM in slot 0 is uploaded);
    /// per-prim filtering happens later in `pack_mesh_*`.
    pub fn set_scene_kingdom(&mut self, prot_base: u32) -> Result<u32, JsValue> {
        let entries = parse_prot_toc(&self.disc)
            .ok_or_else(|| JsValue::from_str("set_scene_kingdom: no PROT TOC available"))?;
        // Try PROT base first (scene_scripted_asset_table variant), then
        // base+1 (bare scene_asset_table). Either carries the same 7-asset
        // bundle for the world-map kingdoms.
        let pack = self
            .try_load_kingdom_at(&entries, prot_base)
            .or_else(|_| self.try_load_kingdom_at(&entries, prot_base + 1))
            .map_err(|e| {
                JsValue::from_str(&format!("set_scene_kingdom({prot_base}) failed: {e}"))
            })?;
        let count = pack.byte_offsets.len() as u32;
        console_log(&format!(
            "kingdom PROT {} loaded: {} TMDs in pack ({} bytes), VRAM filled",
            pack.prot_index,
            count,
            pack.pack.len()
        ));
        self.kingdom = Some(pack);
        // Sweep nearby entries (+8..+12) for a second 7-asset table holding
        // the bulk continent terrain TMDs. Drake's is at +8 (entry 0093, 70
        // TMDs); Sebucus's at +9 (entry 0253, 43 TMDs); Karisto's not yet
        // pinned. We try a window of plausible offsets and accept the first
        // entry that decodes cleanly.
        self.continent = None;
        for off in 8..=12u32 {
            let candidate = prot_base + off;
            if let Ok(p) = self.try_load_kingdom_at(&entries, candidate) {
                console_log(&format!(
                    "continent PROT {} loaded: {} TMDs in pack ({} bytes)",
                    p.prot_index,
                    p.byte_offsets.len(),
                    p.pack.len()
                ));
                self.continent = Some(p);
                break;
            }
        }

        // Build the walk-view continent ground heightfield for this kingdom.
        // The native engine's world-map render draws this surface (the slot-1
        // pack is only the sparse landmarks); reproducing it here brings the
        // site viewer to terrain parity. Sources the walk `.MAP` floor grid +
        // the kingdom MAN's floor-height LUT; reuses `build_walk_heightfield`.
        self.walk_ground = build_walk_ground(&self.disc, &entries, prot_base);
        if let Some(hf) = &self.walk_ground {
            console_log(&format!(
                "walk heightfield: {} quads ({} verts) for PROT base {}",
                hf.quad_count(),
                hf.positions.len(),
                prot_base
            ));
        } else {
            console_log(&format!(
                "walk heightfield: unavailable for PROT base {prot_base} (no walk .MAP / floor LUT)"
            ));
        }

        // Resolve the walk-frame placed landmarks (the slot-1 pack meshes
        // `FUN_8003A55C` stamps on the continent) in the same world frame as
        // the heightfield, so the viewer can draw them on top of the terrain.
        self.walk_placements = build_walk_placements(&self.disc, &entries, prot_base);
        if let Some(ps) = &self.walk_placements {
            console_log(&format!(
                "walk placements: {} landmarks for PROT base {prot_base}",
                ps.len()
            ));
        }
        Ok(count)
    }

    /// Number of walk-frame placed landmarks for the currently-loaded kingdom
    /// (slot-1 pack meshes positioned on the continent terrain). 0 when no
    /// kingdom is loaded or the walk `.MAP` / floor LUT couldn't be resolved.
    pub fn walk_placement_count(&self) -> u32 {
        self.walk_placements
            .as_ref()
            .map(|p| p.len() as u32)
            .unwrap_or(0)
    }

    /// Per-placement kingdom pack-mesh slot (record `+0x10`), one `u32` per
    /// walk-frame landmark in placement order. Feed each into `pack_mesh` to
    /// select the mesh, then draw it at the matching
    /// [`Self::walk_placement_positions`] entry.
    pub fn walk_placement_slots(&self) -> Vec<u32> {
        self.walk_placements
            .as_ref()
            .map(|ps| ps.iter().map(|p| p.pack_index).collect())
            .unwrap_or_default()
    }

    /// Per-placement world positions `[x, y, z, ...]` (flattened), in the same
    /// pre-Y-flip `col*128` world frame as [`Self::walk_ground_positions`], so
    /// the JS renderer draws each landmark with the same `(1, -1, 1)` model
    /// flip at scale `1` (the slot-1 meshes are already in true world units).
    pub fn walk_placement_positions(&self) -> Vec<f32> {
        let Some(ps) = self.walk_placements.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(ps.len() * 3);
        for p in ps {
            out.push(p.world_x as f32);
            out.push(p.world_y as f32);
            out.push(p.world_z as f32);
        }
        out
    }

    /// Per-placement authored yaw (object record `+0x0A`), one value per
    /// walk-frame landmark in placement order, in PSX angle units (`4096` =
    /// full revolution) - the Sebucus island bridges' quarter-turns and the
    /// decoration layer's per-tree variety. The JS renderer converts with
    /// `rotY = -(rot & 0xFFF) * Math.PI / 2048` (retail's yaw sense is the
    /// opposite of `placementModelScaled*`'s).
    pub fn walk_placement_rot_y(&self) -> Vec<u16> {
        self.walk_placements
            .as_ref()
            .map(|ps| ps.iter().map(|p| p.rot_y).collect())
            .unwrap_or_default()
    }

    /// Per-vertex world positions of the walk-view continent ground
    /// heightfield, flattened `[x, y, z, ...]`. Empty until a kingdom is loaded.
    /// Same pre-Y-flip world frame as the landmark placement draws, so the JS
    /// renderer applies the same `(1, -1, 1)` model flip (scale 1, no offset).
    pub fn walk_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            out.extend_from_slice(p);
        }
        out
    }

    /// Per-vertex page-local UVs (`u8` pairs) of the walk-view ground, flattened
    /// `[u, v, ...]`. Each cell's four corners cover its `32 x 32` atlas tile.
    pub fn walk_ground_uvs(&self) -> Vec<u8> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.uvs.len() * 2);
        for uv in &hf.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    /// Per-vertex `[clut, tpage]` (PSX CBA + tpage words) of the walk-view
    /// ground, flattened. Distinct per cell so grass / mountain / water / forest
    /// cells sample their own VRAM page from the kingdom slot-0 atlas.
    pub fn walk_ground_cba_tsb(&self) -> Vec<u16> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.cba_tsb.len() * 2);
        for ct in &hf.cba_tsb {
            out.extend_from_slice(ct);
        }
        out
    }

    /// Triangle indices of the walk-view ground (two triangles per cell quad).
    pub fn walk_ground_indices(&self) -> Vec<u32> {
        self.walk_ground
            .as_ref()
            .map(|hf| hf.indices.clone())
            .unwrap_or_default()
    }

    /// Number of ground cells (quads) in the walk-view heightfield. 0 when no
    /// kingdom is loaded or the heightfield couldn't be resolved.
    pub fn walk_ground_quad_count(&self) -> u32 {
        self.walk_ground
            .as_ref()
            .map(|hf| hf.quad_count() as u32)
            .unwrap_or(0)
    }

    /// Number of TMDs in the currently-loaded continent pack. 0 when no
    /// continent pack was found for this kingdom.
    pub fn continent_pack_count(&self) -> u32 {
        self.continent
            .as_ref()
            .map(|k| k.byte_offsets.len() as u32)
            .unwrap_or(0)
    }

    /// PROT index the continent pack was loaded from (0 when none).
    pub fn continent_prot_index(&self) -> u32 {
        self.continent.as_ref().map(|k| k.prot_index).unwrap_or(0)
    }

    /// VRAM bytes (1 MB) built from the continent pack's slot 0. Distinct from
    /// the landmark VRAM since the two packs ship independent TIM_LISTs.
    pub fn continent_pack_vram_bytes(&self) -> Vec<u8> {
        self.continent
            .as_ref()
            .map(|k| k.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Select the active continent pack slot. Parallel to `pack_mesh` but
    /// operates on the continent pack.
    pub fn continent_pack_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let k = self
            .continent
            .as_mut()
            .ok_or_else(|| JsValue::from_str("continent_pack_mesh: no continent loaded"))?;
        let s = slot as usize;
        if s >= k.byte_offsets.len() {
            return Err(JsValue::from_str(&format!(
                "continent_pack_mesh: slot {s} >= count {}",
                k.byte_offsets.len()
            )));
        }
        k.cur_slot = Some(s);
        Ok(slot)
    }

    pub fn continent_pack_mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn continent_pack_mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn continent_pack_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn continent_pack_mesh_indices(&self) -> Vec<u32> {
        self.build_continent_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    pub fn continent_pack_mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_continent_mesh() else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let cz = (lo[2] + hi[2]) * 0.5;
        let dx = (hi[0] - lo[0]) * 0.5;
        let dy = (hi[1] - lo[1]) * 0.5;
        let dz = (hi[2] - lo[2]) * 0.5;
        let r = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
        vec![cx, cy, cz, r]
    }

    /// Set the active pack-mesh slot. Subsequent `pack_mesh_*` calls source
    /// from `pack[byte_offsets[slot]..byte_ends[slot]]`. Returns an error
    /// when no kingdom is loaded or `slot >= pack count`.
    pub fn pack_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let k = self
            .kingdom
            .as_mut()
            .ok_or_else(|| JsValue::from_str("pack_mesh: no kingdom loaded"))?;
        let s = slot as usize;
        if s >= k.byte_offsets.len() {
            return Err(JsValue::from_str(&format!(
                "pack_mesh: slot {s} >= count {}",
                k.byte_offsets.len()
            )));
        }
        k.cur_slot = Some(s);
        Ok(slot)
    }

    /// Number of TMDs in the currently-loaded kingdom pack. 0 when no
    /// kingdom is loaded.
    pub fn pack_count(&self) -> u32 {
        self.kingdom
            .as_ref()
            .map(|k| k.byte_offsets.len() as u32)
            .unwrap_or(0)
    }

    /// VRAM bytes (1 MB) built from every TIM in the kingdom's slot 0
    /// (TIM_LIST). Reuse across every `pack_mesh_*` call - the kingdom
    /// pack's per-slot TMDs all sample from this one shared image.
    pub fn pack_vram_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .map(|k| k.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Ocean tile pixel data (4bpp indexed), 64 halfwords × 256 rows =
    /// 32 768 bytes. Each byte holds 2 pixels (low nibble first). The
    /// CLUT index addressing is `pixel = byte >> 4` for the high pixel
    /// and `byte & 0x0F` for the low pixel. Empty when the kingdom is
    /// not a world-map kingdom or the ocean TIM wasn't found.
    pub fn ocean_texture_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.texture.clone())
            .unwrap_or_default()
    }

    /// Static base CLUT for the ocean tile row: 256 entries × 2 bytes
    /// (BGR555 LE) = 512 bytes. The first 16 entries are the ones the
    /// animation cycle overrides each frame; entries 16..255 stay fixed
    /// and belong to other tiles sharing the same VRAM row.
    pub fn ocean_base_clut_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.base_clut.clone())
            .unwrap_or_default()
    }

    /// 13-frame ocean CLUT animation table: 13 × 32 bytes = 416 bytes,
    /// frame-0 first. Each frame is 16 BGR555 entries (the same shape as
    /// the first 16 entries of [`Self::ocean_base_clut_bytes`]). The
    /// runtime DMAs one frame at a time onto VRAM (0, 506) to cycle
    /// the wave colours through the ocean tile.
    pub fn ocean_animation_frames(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.animation_frames.clone())
            .unwrap_or_default()
    }

    /// Number of valid ocean animation frames (typically 13). Returns 0
    /// when the kingdom doesn't have ocean assets.
    pub fn ocean_frame_count(&self) -> u32 {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| (o.animation_frames.len() / 32) as u32)
            .unwrap_or(0)
    }

    /// Parallel to [`Self::mesh_positions`] but sources from the currently
    /// selected kingdom pack slot.
    pub fn pack_mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn pack_mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn pack_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn pack_mesh_indices(&self) -> Vec<u32> {
        self.build_kingdom_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    pub fn pack_mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let cz = (lo[2] + hi[2]) * 0.5;
        let dx = (hi[0] - lo[0]) * 0.5;
        let dy = (hi[1] - lo[1]) * 0.5;
        let dz = (hi[2] - lo[2]) * 0.5;
        let r = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
        vec![cx, cy, cz, r]
    }

    /// Decode the slot-4 world-map overlay wireframe for the kingdom at
    /// `prot_base` and return a packed line-segment list for top-down
    /// rendering.
    ///
    /// The wireframe is the dev-menu top-view overlay - coastline curves
    /// (Drake body 12 = 1200-vertex outline) and the ±32K world-boundary
    /// frame (Drake body 13). Loaded verbatim into RAM at `0x8011A624` for
    /// Drake (32304 bytes); format is fully reversed (see
    /// [`docs/formats/world-map-overlay.md`]).
    ///
    /// `style` selects the polyline-construction mode:
    /// `"row"` (each group as one polyline), `"col"` (each record-slot as
    /// one polyline across groups), `"pairs"` (every 2 consecutive
    /// records emit one segment), or `"grid"` (both row and column
    /// edges of the `count_a x count_b` vertex grid). Unknown values
    /// fall back to `"row"`.
    ///
    /// Output layout (single packed `Vec<u8>`, little-endian):
    ///
    /// ```text
    /// [u32 line_count]
    /// [Line; line_count]   ; struct, 12 bytes each:
    ///     u8  body_index
    ///     u8  group_index_low   ; group_index = (low | (high << 8))
    ///     u8  group_index_high
    ///     u8  _pad
    ///     i16 x0
    ///     i16 z0
    ///     i16 x1
    ///     i16 z1
    /// ```
    ///
    /// Returns an empty buffer when slot 4 is missing or fails to parse.
    /// The JS-side renderer assigns per-body colors based on `body_index`.
    pub fn slot4_wireframe_lines(&self, prot_base: u32, style: &str, axes: &str) -> Vec<u8> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let mode = match style {
            "col" => legaia_asset::world_map_overlay::PolylineMode::ColumnMajor,
            "pairs" => legaia_asset::world_map_overlay::PolylineMode::PairWise,
            "grid" => legaia_asset::world_map_overlay::PolylineMode::Grid,
            _ => legaia_asset::world_map_overlay::PolylineMode::RowMajor,
        };
        let opts = legaia_asset::world_map_overlay::WireframeOptions {
            mode,
            axes: parse_axes(axes),
            ..Default::default()
        };
        let lines = legaia_asset::world_map_overlay::top_down_lines(&slot, &opts);

        let mut out = Vec::with_capacity(4 + lines.len() * 12);
        out.extend_from_slice(&(lines.len() as u32).to_le_bytes());
        for l in &lines {
            out.push(l.body_index);
            out.push((l.group_index & 0xFF) as u8);
            out.push((l.group_index >> 8) as u8);
            out.push(0); // pad
            out.extend_from_slice(&l.x0.to_le_bytes());
            out.extend_from_slice(&l.z0.to_le_bytes());
            out.extend_from_slice(&l.x1.to_le_bytes());
            out.extend_from_slice(&l.z1.to_le_bytes());
        }
        out
    }

    /// Decode the slot-4 world-map overlay as a topology-free point cloud.
    /// Useful when the on-disc draw-mode dispatch isn't fully reverse-
    /// engineered: the points themselves are byte-verified against live
    /// RAM, so plotting them straight is the most honest visualization.
    ///
    /// Output layout (little-endian):
    ///
    /// ```text
    /// [u32 point_count]
    /// [Point; point_count] ; 8 bytes each:
    ///     u8  body_index
    ///     u8  group_index_low
    ///     u8  group_index_high
    ///     u8  _pad
    ///     i16 x
    ///     i16 z
    /// ```
    pub fn slot4_wireframe_points(&self, prot_base: u32, axes: &str) -> Vec<u8> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let opts = legaia_asset::world_map_overlay::WireframeOptions {
            axes: parse_axes(axes),
            ..Default::default()
        };
        let pts = legaia_asset::world_map_overlay::record_points(&slot, &opts);

        let mut out = Vec::with_capacity(4 + pts.len() * 8);
        out.extend_from_slice(&(pts.len() as u32).to_le_bytes());
        for (body, group, x, z) in &pts {
            out.push(*body);
            out.push((*group & 0xFF) as u8);
            out.push((*group >> 8) as u8);
            out.push(0); // pad
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&z.to_le_bytes());
        }
        out
    }

    /// Bounding box of every non-zero record in the kingdom's slot-4
    /// wireframe, as `[amin, bmin, amax, bmax]` (i32) for the requested
    /// axis pair (`"xz"` / `"xy"` / `"zy"`, etc). Useful for re-framing
    /// the top-down camera when the overlay is toggled on. Empty vec
    /// when slot 4 can't be decoded.
    pub fn slot4_wireframe_bounds(&self, prot_base: u32, axes: &str) -> Vec<i32> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let (ah, av) = parse_axes(axes);
        match legaia_asset::world_map_overlay::axis_bounds(&slot, ah, av) {
            Some((amin, bmin, amax, bmax)) => {
                vec![amin as i32, bmin as i32, amax as i32, bmax as i32]
            }
            None => Vec::new(),
        }
    }

    /// Per-body inventory of the slot-4 wireframe, as a JSON string.
    /// Used by the inspector panel to show which bodies are present.
    /// Returns `"[]"` when slot 4 can't be decoded.
    pub fn slot4_body_inventory_json(&self, prot_base: u32) -> String {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return "[]".into();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return "[]".into();
        };
        let mut s = String::from("[");
        for (i, b) in slot.bodies.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (xlo, xhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::X,
            )
            .unwrap_or((0, 0));
            let (ylo, yhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::Y,
            )
            .unwrap_or((0, 0));
            let (zlo, zhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::Z,
            )
            .unwrap_or((0, 0));
            s.push_str(&format!(
                r#"{{"index":{},"count_a":{},"count_b":{},"flag_a":{},"flag_b":{},"kind":{},"records":{},"x":[{},{}],"y":[{},{}],"z":[{},{}]}}"#,
                b.index,
                b.count_a,
                b.count_b,
                b.flag_a,
                b.flag_b,
                b.kind,
                b.records.len(),
                xlo, xhi, ylo, yhi, zlo, zhi,
            ));
        }
        s.push(']');
        s
    }

    /// Decode the live PSX GPU primitive pool out of a mednafen save state
    /// and return per-vertex attribute arrays for replay in WebGL2 against
    /// the save state's VRAM.
    ///
    /// Pool location is per `legaia_mednafen::prim_pool::POOL_BASE_DEFAULT`
    /// (= `0x800AD400`, consistent across the Drake / Sebucus / Karisto
    /// top-view captures). Each accepted primitive (POLY_FT4, POLY_GT4,
    /// POLY_FT3, POLY_GT3, SPRT_16, SPRT_8) is expanded into two
    /// triangles in screen-space.
    ///
    /// Return layout (single packed `Vec<u8>`, little-endian, in this order):
    ///
    /// ```text
    /// [u16 vram_width = 1024]
    /// [u16 vram_height = 512]
    /// [u32 vram_byte_len = 1048576]
    /// [u8;  1048576] VRAM bytes (raw BGR555+STP halfwords)
    /// [u16 screen_w]
    /// [u16 screen_h]
    /// [u32 vertex_count]
    /// [Vertex; vertex_count]   ; struct, 14 bytes each:
    ///     i16 x, i16 y
    ///     u8  u, u8 v
    ///     u16 cba, u16 tsb
    ///     u8  r, u8 g, u8 b, u8 flags
    /// ```
    ///
    /// JSON dump of the world-map quick-travel menu parsed out of
    /// `SCUS_942.54` at disc-load time. Returns `null` if no disc was
    /// loaded as a Mode2/2352 image (raw PROT.DAT paths skip SCUS).
    ///
    /// Shape:
    /// ```json
    /// { "names": [..16 strings..],
    ///   "placements": [{ "index": u32, "name_idx": u8,
    ///                    "discovery_flag": u8, "scene_id": u16,
    ///                    "menu_x": u8, "menu_y": u8 }, ...] }
    /// ```
    pub fn worldmap_menu_json(&self) -> String {
        match &self.worldmap_menu {
            Some(menu) => serde_json::to_string(menu)
                .unwrap_or_else(|e| format!("{{\"error\":\"serialize failed: {e}\"}}")),
            None => "null".to_string(),
        }
    }
}
