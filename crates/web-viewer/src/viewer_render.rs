//! Internal render/build helpers for `LegaiaViewer` (not exported to JS).
use super::*;

impl LegaiaViewer {
    pub(crate) fn render_current(&mut self) -> Result<(), JsValue> {
        let Some(entry) = self.viewable.get(self.current).cloned() else {
            return Ok(());
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Err(JsValue::from_str("entry out of bounds"));
        }
        let entry_bytes = &self.disc[off..end];
        let label = format!("PROT {} · {}", entry.meta.index, entry.class.name());

        // The 3D path is driven from JS via render_tmd_triangles; if the
        // entry has a TMD, leave the canvas as the JS side set it up
        // (the rAF loop will repaint it). Don't try to acquire a 2D
        // context here - JS may already have bound webgl2 to it, in
        // which case getContext("2d") returns null.
        if entry.tmd_source.is_some() {
            return Ok(());
        }

        // 2D path: render the entry's first decodable TIM.
        let Some(hit) = &entry.first_tim else {
            return self.draw_message(&format!(
                "{label}: classified, but no decodable TIM or TMD found"
            ));
        };

        match hit.source.clone() {
            TimSource::Raw(o) => {
                let buf = &entry_bytes[o..].to_vec();
                self.render_tim_at(buf, 0, &label)
            }
            TimSource::Lzs { section, offset } => {
                let scan = tim_scan::scan_entry(entry_bytes);
                let Some(s) = scan.lzs_sections.get(section) else {
                    return self.draw_message(&format!("{label}: LZS section vanished"));
                };
                let buf = s[offset..].to_vec();
                self.render_tim_at(&buf, 0, &format!("{label} (LZS)"))
            }
        }
    }

    /// Build the VRAM the current entry would have at boot (every TIM the
    /// entry contains, uploaded at its declared `(fb_x, fb_y)`). Returns
    /// `None` when there's no current entry or the entry is out of bounds.
    /// Used by both [`Self::current_vram_bytes`] (GPU upload) and the
    /// [`Self::build_current_vram_mesh`] filter (drops prims whose texture
    /// pages weren't supplied so the WebGL pipeline doesn't rasterise
    /// solid-`CLUT[0]` tints over correctly-textured geometry).
    ///
    /// When the entry has a parseable TMD, the upload is *targeted* to
    /// just the TIMs whose image / CLUT regions overlap something the
    /// mesh actually samples. A single PROT entry can contain hundreds
    /// of TIMs, and uploading all of them into the 1 MB VRAM produces
    /// collisions (last-write-wins clobbers a CLUT row with image data
    /// from an unrelated TIM) which the paletted decode then renders as
    /// rainbow noise.
    pub(crate) fn build_current_vram(&self) -> Option<legaia_tim::Vram> {
        let entry = self.viewable.get(self.current)?;
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        let buf = &self.disc[off..end];
        let needs = self.tmd_prim_targets();
        let mut vram = legaia_tim::Vram::new();
        let scan = tim_scan::scan_entry(buf);
        for (source, hit) in &scan.hits {
            let tim_buf: Option<&[u8]> = match source {
                tim_scan::Source::Raw => Some(&buf[hit.offset..]),
                tim_scan::Source::Lzs(idx) => scan.lzs_sections.get(*idx).map(|s| &s[hit.offset..]),
            };
            if let Some(b) = tim_buf
                && let Ok(tim) = legaia_tim::parse(b)
            {
                if needs.is_empty() {
                    vram.upload_tim(&tim);
                } else {
                    let (img, clut) = tim_block_targeting(&tim, &needs);
                    if !img && !clut {
                        continue;
                    }
                    vram.upload_tim_partial(&tim, img, clut);
                }
            }
        }
        Some(vram)
    }

    /// Collect the CLUT + page rectangles every textured primitive in the
    /// current entry's TMD samples. Empty when the entry has no TMD or
    /// the TMD has no textured prims (= no targeting; upload everything).
    pub(crate) fn tmd_prim_targets(&self) -> Vec<PrimTarget> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for o in &tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                &tmd_buf,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            );
            for g in &groups {
                for p in &g.prims {
                    if p.uvs.is_empty() {
                        continue;
                    }
                    let (cx, cy) = p.cba_xy();
                    let (px, py, depth, _) = p.tpage_xy();
                    let clut_w: u16 = match depth {
                        4 => 16,
                        8 => 256,
                        _ => 0,
                    };
                    let mut umin = u8::MAX;
                    let mut umax = 0u8;
                    let mut vmin = u8::MAX;
                    let mut vmax = 0u8;
                    for &(u, v) in &p.uvs {
                        umin = umin.min(u);
                        umax = umax.max(u);
                        vmin = vmin.min(v);
                        vmax = vmax.max(v);
                    }
                    let (u_lo, u_hi) = match depth {
                        4 => (umin as u16 >> 2, umax as u16 >> 2),
                        8 => (umin as u16 >> 1, umax as u16 >> 1),
                        _ => (umin as u16, umax as u16),
                    };
                    out.push(PrimTarget {
                        clut: (cx, cy, clut_w, 1),
                        page: (
                            px + u_lo,
                            py + vmin as u16,
                            u_hi.saturating_sub(u_lo) + 1,
                            (vmax as u16).saturating_sub(vmin as u16) + 1,
                        ),
                    });
                }
            }
        }
        out
    }

    /// Build the current entry's mesh, dropped down to just the primitives
    /// whose texture pages have data in the entry's VRAM. Returns `None`
    /// if the entry has no parseable TMD.
    pub(crate) fn build_current_vram_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, tmd_buf) = self.parse_current_tmd()?;
        let vram = self.build_current_vram();
        Some(match vram {
            Some(v) => {
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&tmd, &tmd_buf, |cba, tsb, uvs| {
                    v.prim_has_texture_data(cba, tsb, uvs)
                })
            }
            None => legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf),
        })
    }

    /// Parse the current entry's TMD if it has one. Returns the parsed TMD
    /// plus the byte slice it was parsed from (caller may need it again to
    /// walk per-object primitive sections).
    /// Resolve an entry's renderable TMD bytes for any [`TmdSource`] variant,
    /// decompressing the LZS section on demand for the environment-geometry
    /// mesh pack. Centralises the slicing the 3D paths share.
    pub(crate) fn tmd_bytes_for(&self, entry: &ViewerEntry) -> Option<Vec<u8>> {
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        let buf = self.disc.get(off..end)?;
        match entry.tmd_source? {
            TmdSource::Direct { offset } => buf.get(offset..).map(<[u8]>::to_vec),
            TmdSource::SceneTmdStream { offset, len } => {
                buf.get(offset..offset + len).map(<[u8]>::to_vec)
            }
            TmdSource::Lzs {
                section,
                offset,
                len,
            } => {
                let scan = legaia_asset::tmd_scan::scan_entry(buf);
                scan.lzs_sections
                    .get(section)?
                    .get(offset..offset + len)
                    .map(<[u8]>::to_vec)
            }
        }
    }

    pub(crate) fn parse_current_tmd(&self) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let entry = self.viewable.get(self.current)?;
        let tmd_buf = self.tmd_bytes_for(entry)?;
        let tmd = legaia_tmd::parse(&tmd_buf).ok()?;
        Some((tmd, tmd_buf))
    }

    /// Decode slot 4 (world-map overlay outlines) of the kingdom bundle at
    /// `prot_base`. Mirrors the runtime loader's "try base, then base+1"
    /// fallback so both `scene_scripted_asset_table` and bare
    /// `scene_asset_table` variants succeed.
    pub(crate) fn decode_kingdom_slot4(&self, prot_base: u32) -> Option<Vec<u8>> {
        let entries = parse_prot_toc(&self.disc)?;
        for candidate in [prot_base, prot_base + 1] {
            let meta = entries.iter().find(|e| e.index == candidate)?;
            let off = meta.byte_offset as usize;
            let end = (meta.byte_offset + meta.size_bytes) as usize;
            if end > self.disc.len() {
                continue;
            }
            let buf = &self.disc[off..end];
            if let Ok(decoded) = legaia_asset::kingdom_bundle::decode_slot(buf, 4) {
                return Some(decoded);
            }
        }
        None
    }

    /// Try to load a kingdom 7-asset bundle from the PROT entry at `prot_index`.
    /// Returns the populated [`KingdomPack`] or a human-readable error.
    pub(crate) fn try_load_kingdom_at(
        &self,
        entries: &[EntryMeta],
        prot_index: u32,
    ) -> Result<KingdomPack, String> {
        let meta = entries
            .iter()
            .find(|e| e.index == prot_index)
            .ok_or_else(|| format!("PROT entry {prot_index} not in TOC"))?;
        let off = meta.byte_offset as usize;
        let end = (meta.byte_offset + meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Err(format!(
                "PROT entry {prot_index} ranges [{off}..{end}) exceed disc len {}",
                self.disc.len()
            ));
        }
        let buf = &self.disc[off..end];

        // Find the 7-asset table inside the entry. We scan 0x800-aligned
        // offsets for `[u32 count = 7]` + `descriptor[0].data_offset == 0x40`;
        // this catches both the prescript-prefixed variant (PROT base) and
        // the bare scene_asset_table variant (PROT base+1) without needing
        // separate detectors.
        let table_off = find_asset_table_offset(buf)
            .ok_or_else(|| format!("PROT entry {prot_index}: no 7-asset table found"))?;
        let table = &buf[table_off..];

        // Slot 0 = TIM_LIST (type 0x01).
        let slot0_ts = read_u32_le_slice(table, 8)?;
        let slot0_off = read_u32_le_slice(table, 12)? as usize;
        let slot0_type = (slot0_ts >> 24) as u8;
        let slot0_size = (slot0_ts & 0x00FF_FFFF) as usize;
        if slot0_type != 0x01 {
            return Err(format!("slot 0 type 0x{slot0_type:02X} != 0x01 (TIM_LIST)"));
        }

        // Slot 1 = TMD pack (type 0x02).
        let slot1_ts = read_u32_le_slice(table, 16)?;
        let slot1_off = read_u32_le_slice(table, 20)? as usize;
        let slot1_type = (slot1_ts >> 24) as u8;
        let slot1_size = (slot1_ts & 0x00FF_FFFF) as usize;
        if slot1_type != 0x02 {
            return Err(format!("slot 1 type 0x{slot1_type:02X} != 0x02 (TMD)"));
        }

        // Each descriptor's `data_offset` is the table-relative file position
        // of that slot's LZS-compressed payload. Decode each independently.
        let tim_src = table
            .get(slot0_off..)
            .ok_or_else(|| format!("slot 0 offset 0x{slot0_off:X} out of range"))?;
        let tim_decoded = legaia_lzs::decompress(tim_src, slot0_size)
            .map_err(|e| format!("slot 0 LZS decode failed: {e}"))?;

        let pack_src = table
            .get(slot1_off..)
            .ok_or_else(|| format!("slot 1 offset 0x{slot1_off:X} out of range"))?;
        let pack = legaia_lzs::decompress(pack_src, slot1_size)
            .map_err(|e| format!("slot 1 LZS decode failed: {e}"))?;

        // Build VRAM by walking every TIM the TIM_LIST decompresses to.
        // TIM_LIST format is `[u32 count][u32 word_offsets[count]][TIMs]`
        // (same shape as the TMD pack but with TIM bodies). Mirror the
        // pointer math from `FUN_8001F05C case 1` -> byte_offset = word * 4.
        let mut vram = legaia_tim::Vram::new();
        if tim_decoded.len() >= 4 {
            let count = u32::from_le_bytes(tim_decoded[0..4].try_into().unwrap()) as usize;
            let table_bytes = 4 + count * 4;
            if tim_decoded.len() >= table_bytes {
                for k in 0..count {
                    let woff =
                        u32::from_le_bytes(tim_decoded[4 + k * 4..8 + k * 4].try_into().unwrap())
                            as usize;
                    let bo = woff.saturating_mul(4);
                    if bo >= tim_decoded.len() {
                        continue;
                    }
                    if let Ok(tim) = legaia_tim::parse(&tim_decoded[bo..]) {
                        vram.upload_tim(&tim);
                    }
                }
            }
        }
        let ocean = find_ocean_assets(&tim_decoded);

        // Parse the TMD-pack table.
        if pack.len() < 4 {
            return Err("TMD pack < 4 bytes".into());
        }
        let count = u32::from_le_bytes(pack[0..4].try_into().unwrap()) as usize;
        let table_bytes = 4 + count * 4;
        if pack.len() < table_bytes {
            return Err(format!(
                "TMD pack header truncated (need {table_bytes}, have {})",
                pack.len()
            ));
        }
        let mut byte_offsets = Vec::with_capacity(count);
        for k in 0..count {
            let woff = u32::from_le_bytes(pack[4 + k * 4..8 + k * 4].try_into().unwrap()) as usize;
            byte_offsets.push(woff.saturating_mul(4));
        }
        let mut byte_ends = Vec::with_capacity(count);
        for k in 0..count {
            byte_ends.push(byte_offsets.get(k + 1).copied().unwrap_or(pack.len()));
        }

        Ok(KingdomPack {
            prot_index,
            vram,
            pack,
            byte_offsets,
            byte_ends,
            cur_slot: None,
            ocean,
        })
    }

    /// Build the textured mesh for the currently-selected kingdom pack slot.
    ///
    /// Uses the unfiltered [`legaia_tmd::mesh::tmd_to_vram_mesh`] (not the
    /// VRAM-targeted filter the per-PROT-entry path uses). The kingdom's
    /// TIM_LIST packs ~50 TIMs into rows 479-510, so many CLUT rows hold
    /// data from multiple TIMs and the filter's depth-mismatch heuristic
    /// drops almost every prim. We accept that some prims may sample
    /// "wrong" CLUT data (whichever TIM won the last-write-wins race for
    /// that VRAM row) in exchange for geometry coverage: the assembled
    /// continent view is "show the kingdom's shape and colour" first,
    /// "pixel-perfect texturing" later. A future refinement is per-TMD
    /// targeted upload (compute the prim CBA/TSB set, upload only the
    /// matching TIMs), which would avoid the collision but require
    /// rebuilding VRAM per slot.
    pub(crate) fn build_kingdom_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        Self::build_pack_mesh(self.kingdom.as_ref()?)
    }

    /// Mirror of `build_kingdom_mesh` for the bulk-continent pack loaded
    /// from slot +N of the kingdom bundle (Drake +8, Sebucus +9). The
    /// continent pack carries its own VRAM + its own TMD list, so the
    /// pack-mesh accessors route through it via `cur_slot`.
    pub(crate) fn build_continent_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        Self::build_pack_mesh(self.continent.as_ref()?)
    }

    pub(crate) fn build_pack_mesh(k: &KingdomPack) -> Option<legaia_tmd::mesh::VramMesh> {
        let slot = k.cur_slot?;
        let start = *k.byte_offsets.get(slot)?;
        let end = *k.byte_ends.get(slot)?;
        let tmd_buf = k.pack.get(start..end)?;
        let tmd = legaia_tmd::parse(tmd_buf).ok()?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, tmd_buf))
    }

    pub(crate) fn tmd_stats(&self, entry: &ViewerEntry) -> (usize, usize) {
        let Some(tmd_buf) = self.tmd_bytes_for(entry) else {
            return (0, 0);
        };
        let Ok(tmd) = legaia_tmd::parse(&tmd_buf) else {
            return (0, 0);
        };
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &tmd_buf);
        (mesh.triangle_count(), mesh.vertex_count())
    }

    pub(crate) fn render_tim_at(
        &self,
        src: &[u8],
        offset: usize,
        label: &str,
    ) -> Result<(), JsValue> {
        let buf = &src[offset..];
        let tim = legaia_tim::parse(buf)
            .map_err(|e| JsValue::from_str(&format!("{label}: TIM parse: {e}")))?;
        let rgba = legaia_tim::decode_rgba8(&tim, self.clut_idx)
            .map_err(|e| JsValue::from_str(&format!("{label}: decode: {e}")))?;

        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        if w == 0 || h == 0 {
            return self.draw_message(&format!("{label}: empty TIM ({}x{})", w, h));
        }

        let (canvas, ctx) = self.acquire_2d_context()?;
        canvas.set_width(w);
        canvas.set_height(h);

        let clamped = rgba;
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&clamped), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    pub(crate) fn draw_message(&self, msg: &str) -> Result<(), JsValue> {
        let (canvas, ctx) = self.acquire_2d_context()?;
        canvas.set_width(800);
        canvas.set_height(200);
        ctx.set_fill_style_str("#0a0e15");
        ctx.fill_rect(0.0, 0.0, 800.0, 200.0);
        ctx.set_fill_style_str("#8b949e");
        ctx.set_font("16px JetBrains Mono, ui-monospace, monospace");
        ctx.fill_text(msg, 16.0, 100.0)?;
        Ok(())
    }

    /// Resolve `canvas_id` to its current `HtmlCanvasElement` and a fresh
    /// 2D rendering context. The element is re-fetched from the DOM each
    /// time because the JS UI replaces the canvas when switching between
    /// the 2D (TIM blit) and 3D (WebGL2) modes - getContext returns null
    /// for the second context type bound to a single canvas, and any
    /// cached reference goes stale the moment `oldCanvas.replaceWith(...)`
    /// runs in `startTexturedTmdLoop` / `startFlatTmdLoop`.
    pub(crate) fn acquire_2d_context(
        &self,
    ) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
        let canvas = resolve_canvas(&self.canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| {
                JsValue::from_str(
                    "no 2d context (canvas was already bound to webgl - JS must \
                     replace the canvas element before requesting a 2D draw)",
                )
            })?
            .dyn_into::<CanvasRenderingContext2d>()?;
        Ok((canvas, ctx))
    }
}
