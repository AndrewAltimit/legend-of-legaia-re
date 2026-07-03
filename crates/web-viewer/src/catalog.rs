//! TIM catalog + deep-catalog browse-mode exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    // --- TIM Catalog browse mode -----------------------------------------
    //
    // The catalog is a flat, jPSXdec-parity inventory of every standard TIM
    // in the loaded PROT.DAT, keyed by a stable id. These accessors let the
    // page page through all of them by id and switch CLUT variants, even for
    // TIMs that live in the unindexed system-UI gap (no owning PROT entry).

    /// Number of cataloged TIMs in the loaded PROT.DAT.
    pub fn catalog_len(&self) -> u32 {
        self.tim_catalog.len() as u32
    }

    /// Number of CLUT palettes available for cataloged TIM `id` (0 for
    /// 16/24bpp TIMs, which carry no palette).
    pub fn catalog_clut_count(&self, id: u32) -> u32 {
        self.tim_catalog
            .get(id as usize)
            .map(|t| t.clut_count as u32)
            .unwrap_or(0)
    }

    /// JSON describing cataloged TIM `id` (offset, owning entry, dimensions,
    /// CLUT count, byte length, fingerprint) for the info panel.
    pub fn catalog_info_json(&self, id: u32) -> String {
        match self.tim_catalog.get(id as usize) {
            Some(t) => {
                let entry = match t.entry_index {
                    Some(i) => i.to_string(),
                    None => "gap".to_string(),
                };
                format!(
                    "{{\"id\":{},\"abs_offset\":{},\"sector\":{},\"entry\":\"{}\",\
                     \"offset_in_entry\":{},\"width\":{},\"height\":{},\"bpp\":{},\
                     \"clut_count\":{},\"byte_len\":{},\"fnv1a\":\"{:016x}\",\"label\":{}}}",
                    t.id,
                    t.abs_offset,
                    t.sector,
                    entry,
                    t.offset_in_entry,
                    t.width,
                    t.height,
                    t.bpp,
                    t.clut_count,
                    t.byte_len,
                    t.fnv1a,
                    json_label(t.label),
                )
            }
            None => "{}".to_string(),
        }
    }

    /// Render cataloged TIM `id` with CLUT `clut` into the 2D canvas named
    /// `canvas_id`. The catalog browser uses its own canvas (separate from
    /// the PROT-entry browser's, which switches between 2D and WebGL), so it
    /// takes the target id explicitly rather than the viewer's bound canvas.
    pub fn render_catalog_tim(&self, id: u32, clut: u32, canvas_id: &str) -> Result<(), JsValue> {
        let t = self
            .tim_catalog
            .get(id as usize)
            .ok_or_else(|| JsValue::from_str(&format!("catalog id {id} out of range")))?;
        let off = t.abs_offset as usize;
        let tim = legaia_tim::parse(&self.disc[off..])
            .map_err(|e| JsValue::from_str(&format!("catalog[{id}] TIM parse: {e}")))?;
        let clut_idx = if t.clut_count > 0 {
            (clut as usize).min(t.clut_count - 1)
        } else {
            0
        };
        let rgba = legaia_tim::decode_rgba8(&tim, clut_idx)
            .map_err(|e| JsValue::from_str(&format!("catalog[{id}] decode: {e}")))?;
        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        let canvas = resolve_canvas(canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("catalog canvas has no 2D context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        if w == 0 || h == 0 {
            return Err(JsValue::from_str(&format!(
                "catalog[{id}]: empty TIM ({w}x{h})"
            )));
        }
        canvas.set_width(w);
        canvas.set_height(h);
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    // --- Deep TIM Catalog (compressed textures) --------------------------
    //
    // The deep catalog is the LZS-embedded tier: standard TIMs recovered from
    // inside compressed PROT sections, which the flat (raw-bytes) catalog
    // above can't reach. Keyed by (entry, lzs-section, offset-in-section).
    // These accessors mirror the flat-catalog ones so the page can drive a
    // second, clearly-labeled grid from the same UI code.

    /// Number of cataloged compressed TIMs in the loaded PROT.DAT.
    pub fn deep_catalog_len(&self) -> u32 {
        self.tim_deep_catalog.len() as u32
    }

    /// Number of CLUT palettes available for deep-catalog TIM `id`.
    pub fn deep_catalog_clut_count(&self, id: u32) -> u32 {
        self.tim_deep_catalog
            .get(id as usize)
            .map(|t| t.clut_count as u32)
            .unwrap_or(0)
    }

    /// JSON describing deep-catalog TIM `id` (owning entry, LZS section,
    /// offset within the decoded section, dimensions, CLUT count, byte
    /// length, fingerprint) for the info panel.
    pub fn deep_catalog_info_json(&self, id: u32) -> String {
        match self.tim_deep_catalog.get(id as usize) {
            Some(t) => format!(
                "{{\"id\":{},\"entry\":{},\"lzs_section\":{},\"offset_in_section\":{},\
                 \"width\":{},\"height\":{},\"bpp\":{},\"clut_count\":{},\
                 \"byte_len\":{},\"fnv1a\":\"{:016x}\",\"label\":{}}}",
                t.id,
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                json_label(t.label),
            ),
            None => "{}".to_string(),
        }
    }

    /// Decompress deep-catalog TIM `id`'s owning entry (via a one-entry cache)
    /// and return the decoded section bytes it lives in, plus the offset.
    fn deep_section_bytes(&self, id: u32) -> Result<(Vec<u8>, usize), JsValue> {
        let t = self
            .tim_deep_catalog
            .get(id as usize)
            .ok_or_else(|| JsValue::from_str(&format!("deep catalog id {id} out of range")))?;
        // Reuse the cached sections if this is the same entry as last time.
        {
            let cache = self.deep_section_cache.borrow();
            if let Some((cached_entry, sections)) = cache.as_ref()
                && *cached_entry == t.entry_index
            {
                let section = sections.get(t.lzs_section as usize).ok_or_else(|| {
                    JsValue::from_str(&format!("deep[{id}]: section {} gone", t.lzs_section))
                })?;
                return Ok((section.clone(), t.offset_in_section as usize));
            }
        }
        // Cache miss: find the entry span, slice, decompress, and cache.
        let entries = parse_prot_toc(&self.disc)
            .ok_or_else(|| JsValue::from_str("deep: PROT TOC parse failed"))?;
        let entry = entries
            .iter()
            .find(|e| e.index == t.entry_index)
            .ok_or_else(|| JsValue::from_str(&format!("deep[{id}]: entry gone")))?;
        let start = entry.byte_offset as usize;
        let end = start.saturating_add(entry.size_bytes as usize);
        if end > self.disc.len() {
            return Err(JsValue::from_str(&format!("deep[{id}]: entry span OOB")));
        }
        let sections = legaia_lzs::decompress_container(&self.disc[start..end])
            .map_err(|e| JsValue::from_str(&format!("deep[{id}]: LZS decode: {e}")))?;
        let section = sections
            .get(t.lzs_section as usize)
            .ok_or_else(|| JsValue::from_str(&format!("deep[{id}]: section gone")))?
            .clone();
        let off = t.offset_in_section as usize;
        *self.deep_section_cache.borrow_mut() = Some((t.entry_index, sections));
        Ok((section, off))
    }

    /// Render deep-catalog TIM `id` with CLUT `clut` into the 2D canvas named
    /// `canvas_id`.
    pub fn render_deep_catalog_tim(
        &self,
        id: u32,
        clut: u32,
        canvas_id: &str,
    ) -> Result<(), JsValue> {
        let (section, off) = self.deep_section_bytes(id)?;
        let tim = legaia_tim::parse(&section[off..])
            .map_err(|e| JsValue::from_str(&format!("deep[{id}] TIM parse: {e}")))?;
        let nclut = tim.palette_count();
        let clut_idx = if nclut > 0 {
            (clut as usize).min(nclut - 1)
        } else {
            0
        };
        let rgba = legaia_tim::decode_rgba8(&tim, clut_idx)
            .map_err(|e| JsValue::from_str(&format!("deep[{id}] decode: {e}")))?;
        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        if w == 0 || h == 0 {
            return Err(JsValue::from_str(&format!(
                "deep[{id}]: empty TIM ({w}x{h})"
            )));
        }
        let canvas = resolve_canvas(canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("deep catalog canvas has no 2D context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        canvas.set_width(w);
        canvas.set_height(h);
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }
}
