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

// --- Palette context: which palette the game really draws a TIM with -------
//
// A TIM's own CLUT rows are the right palettes only when the game samples the
// texture through the cell the file uploads to and nothing overwrote it. See
// `legaia_asset::tim_palette_context` for the three shapes that break it. The
// accessors below let the catalog detail panel (a) say which shape applies,
// (b) offer the palettes VRAM really holds on the TIM's CLUT row after the
// boot upload, and (c) decode the system-UI page "as the game draws it" -
// every widget sprite through its own palette.

use legaia_asset::tim_palette_context::{self as palctx, ClutFate};

/// How a catalog detail render picks its palette. Parsed from the page's
/// `<select>` value: `own:N`, `vram:X:Y` or `composite`.
enum PaletteChoice {
    Own(usize),
    Vram(u16, u16),
    Composite,
}

fn parse_palette_choice(s: &str) -> Option<PaletteChoice> {
    let mut it = s.split(':');
    match it.next()? {
        "own" => Some(PaletteChoice::Own(it.next()?.parse().ok()?)),
        "vram" => Some(PaletteChoice::Vram(
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
        )),
        "composite" => Some(PaletteChoice::Composite),
        _ => None,
    }
}

fn entries_per_palette(tim: &legaia_tim::Tim) -> usize {
    match tim.mode {
        legaia_tim::PixelMode::Bpp4 => 16,
        legaia_tim::PixelMode::Bpp8 => 256,
        _ => 0,
    }
}

fn join_ids(v: &[usize]) -> String {
    v.iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The sentences every tier shares: all-zero palettes, several palettes,
/// STP colours.
fn shared_palette_notes(
    notes: &mut Vec<String>,
    empty: &[usize],
    stp: &[usize],
    n_own: usize,
    several: bool,
) {
    if !empty.is_empty() {
        notes.push(format!(
            "Palette(s) {} are all zeros on disc, so they show fully transparent here. In \
             game those VRAM cells hold colours another TIM of the same scene uploads.",
            join_ids(empty)
        ));
    }
    if several && n_own > 1 {
        notes.push(
            "Several palettes: each one recolours the WHOLE image, and a model or sprite \
             picks one per polygon / sprite - so most of the image usually looks wrong in \
             any single palette. That is expected, not an export bug."
                .to_string(),
        );
    }
    if stp.iter().any(|&c| c > 0) {
        notes.push(
            "Some colours carry the STP (semi-transparency) bit: they draw blended in game \
             when the sprite / polygon enables blending, but show opaque here."
                .to_string(),
        );
    }
}

impl LegaiaViewer {
    fn catalog_tim_parsed(&self, id: u32) -> Result<(u64, legaia_tim::Tim), String> {
        let t = self
            .tim_catalog
            .get(id as usize)
            .ok_or_else(|| format!("catalog id {id} out of range"))?;
        let tim = legaia_tim::parse(&self.disc[t.abs_offset as usize..])
            .map_err(|e| format!("catalog[{id}] TIM parse: {e}"))?;
        Ok((t.abs_offset, tim))
    }

    /// Composite decode of a sheet-page TIM, if the palette map applies and
    /// covers any of it. Returns `(rgba, covered, contested)`. Reads the
    /// same kernel as the ROM patcher's region map
    /// (`legaia_asset::tim_palette_context::texel_palettes`).
    fn catalog_composite(&self, tim: &legaia_tim::Tim) -> Option<(Vec<u8>, usize, usize)> {
        let regions = self.sheet_regions.as_ref()?;
        let boot = self.boot_cluts.as_ref()?;
        if !palctx::on_sheet_page(tim) {
            return None;
        }
        let cover = self
            .disc
            .get(legaia_asset::ui_widgets::BUTTON_GLYPH_TIM_PROT_OFFSET..)
            .and_then(palctx::parse_button_glyph_tim);
        let texels = palctx::texel_palettes(tim, regions, cover.as_ref())?;
        let covered = texels.claimed();
        if covered == 0 {
            return None;
        }
        let fallback = tim.clut.as_ref()?.palette(tim.mode, 0)?.to_vec();
        let rgba = palctx::composite_rgba(tim, &texels, boot.vram(), &fallback, cover.as_ref())?;
        Some((rgba, covered, texels.contested))
    }

    /// Decode catalog TIM `id` through palette `choice` (see
    /// [`PaletteChoice`]). Native-callable so the disc-gated tests drive the
    /// same decode the page does. Returns `(width, height, rgba)`.
    pub fn catalog_decode_with_choice(
        &self,
        id: u32,
        choice: &str,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        let (_, tim) = self.catalog_tim_parsed(id)?;
        let w = tim.pixel_width() as u32;
        let h = tim.pixel_height() as u32;
        let rgba = match parse_palette_choice(choice)
            .ok_or_else(|| format!("bad palette choice {choice:?}"))?
        {
            PaletteChoice::Own(i) => {
                let n = tim.palette_count();
                let i = if n > 0 { i.min(n - 1) } else { 0 };
                legaia_tim::decode_rgba8(&tim, i).map_err(|e| e.to_string())?
            }
            PaletteChoice::Vram(x, y) => {
                let boot = self
                    .boot_cluts
                    .as_ref()
                    .ok_or("no boot VRAM for this input")?;
                let pal = boot.palette_at(x, y, entries_per_palette(&tim));
                legaia_tim::decode_rgba8_with_palette(&tim, &pal.entries)
                    .map_err(|e| e.to_string())?
            }
            PaletteChoice::Composite => {
                self.catalog_composite(&tim)
                    .ok_or("no palette map covers this texture")?
                    .0
            }
        };
        Ok((w, h, rgba))
    }
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Palette context for cataloged TIM `id`, as JSON:
    ///
    /// * `fate`: `"not_boot"` / `"survives"` / `"overwritten"` - where the
    ///   TIM's own CLUT ends up after the boot upload;
    /// * `overwritten_palettes` + `overwritten_by` (catalog id, or null);
    /// * `empty_palettes`: own palettes that are all `0x0000` on disc;
    /// * `stp_counts`: per own palette, entries with the STP bit set;
    /// * `vram_palettes`: `[{x, y, used}]` - the cells VRAM holds on the TIM's
    ///   CLUT row after boot (boot members only), plus any other cell a widget
    ///   sprite on this page names; `used` = a widget sprite draws through it;
    /// * `composite`: `{covered, contested, total}` when the as-drawn view
    ///   applies, else null;
    /// * `notes`: plain-language sentences for the info panel.
    pub fn catalog_palette_context_json(&self, id: u32) -> String {
        let Ok((abs, tim)) = self.catalog_tim_parsed(id) else {
            return "{}".to_string();
        };
        let per = entries_per_palette(&tim);
        let n_own = tim.palette_count();
        let empty = palctx::empty_palettes(&tim);
        let stp: Vec<usize> = (0..n_own)
            .map(|p| palctx::palette_flag_counts(&tim, p).0)
            .collect();
        let mut fate = "not_boot";
        let mut over_pals: Vec<usize> = Vec::new();
        let mut over_by: Option<u32> = None;
        let mut vram_pals = Vec::new();
        if let Some(boot) = self.boot_cluts.as_ref() {
            match boot.clut_fate(abs, &tim) {
                ClutFate::NotBootResident => {}
                ClutFate::Survives => fate = "survives",
                ClutFate::Overwritten {
                    palettes,
                    by_offset,
                } => {
                    fate = "overwritten";
                    over_pals = palettes;
                    over_by = by_offset.and_then(|o| {
                        self.tim_catalog
                            .iter()
                            .find(|t| t.abs_offset == o)
                            .map(|t| t.id)
                    });
                }
            }
            if fate != "not_boot"
                && let Some(clut) = tim.clut.as_ref()
            {
                let used: Vec<(u16, u16)> = if palctx::on_sheet_page(&tim) {
                    self.sheet_regions
                        .iter()
                        .flatten()
                        .map(|r| r.clut_fb)
                        .collect()
                } else {
                    Vec::new()
                };
                for p in boot.row_palettes(clut.fb_y, per) {
                    vram_pals.push(serde_json::json!({
                        "x": p.fb_x, "y": p.fb_y,
                        "used": used.contains(&(p.fb_x, p.fb_y)),
                    }));
                }
                // Cells this page's sprites use on OTHER rows (the element
                // badges' (896.., 498..501) block).
                let mut extra: Vec<(u16, u16)> = used
                    .iter()
                    .copied()
                    .filter(|&(_, y)| y != clut.fb_y)
                    .collect();
                extra.sort_unstable_by_key(|&(x, y)| (y, x));
                extra.dedup();
                for (x, y) in extra {
                    vram_pals.push(serde_json::json!({"x": x, "y": y, "used": true}));
                }
            }
        }
        let mut notes: Vec<String> = Vec::new();
        match fate {
            "overwritten" => notes.push(format!(
                "This texture's own palette ({}) never reaches VRAM: {} uploads over the same \
                 VRAM cells at boot. The game draws it through a palette VRAM does hold - pick \
                 one from the VRAM entries in the palette list.",
                join_ids(&over_pals),
                over_by
                    .map(|i| format!("TIM #{i}"))
                    .unwrap_or_else(|| "a later boot TIM".to_string()),
            )),
            "survives" => notes.push(
                "Boot-resident: its palettes stay in VRAM for the whole game. A sprite can also \
                 draw it through a neighbouring palette on the same VRAM row (VRAM entries in \
                 the palette list)."
                    .to_string(),
            ),
            _ => {}
        }
        let composite = self.catalog_composite(&tim).map(|(_, covered, contested)| {
            serde_json::json!({
                "covered": covered,
                "contested": contested,
                "total": tim.pixel_width() * tim.pixel_height(),
            })
        });
        if composite.is_some() {
            notes.push(
                "\"As the game draws it\" colours every sprite rectangle the game's widget table \
                 names through that sprite's own palette; dimmed areas are art no table entry \
                 places (other code draws them, with a palette this view cannot know)."
                    .to_string(),
            );
        }
        shared_palette_notes(&mut notes, &empty, &stp, n_own, composite.is_none());
        serde_json::json!({
            "fate": fate,
            "overwritten_palettes": over_pals,
            "overwritten_by": over_by,
            "empty_palettes": empty,
            "stp_counts": stp,
            "vram_palettes": vram_pals,
            "composite": composite,
            "notes": notes,
        })
        .to_string()
    }

    /// Render cataloged TIM `id` into canvas `canvas_id` through palette
    /// `choice` (`own:N`, `vram:X:Y` or `composite`).
    pub fn render_catalog_tim_choice(
        &self,
        id: u32,
        choice: &str,
        canvas_id: &str,
    ) -> Result<(), JsValue> {
        let (w, h, rgba) = self
            .catalog_decode_with_choice(id, choice)
            .map_err(|e| JsValue::from_str(&e))?;
        if w == 0 || h == 0 {
            return Err(JsValue::from_str(&format!("catalog[{id}]: empty TIM")));
        }
        let canvas = resolve_canvas(canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("catalog canvas has no 2D context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        canvas.set_width(w);
        canvas.set_height(h);
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    /// Palette notes for deep-catalog TIM `id` (compressed scene / character
    /// textures, never boot-resident): `empty_palettes`, `stp_counts` and
    /// `notes`, as JSON in the same shape as
    /// [`LegaiaViewer::catalog_palette_context_json`].
    pub fn deep_catalog_palette_context_json(&self, id: u32) -> String {
        let Ok((section, off)) = self.deep_section_bytes(id) else {
            return "{}".to_string();
        };
        let Ok(tim) = legaia_tim::parse(&section[off..]) else {
            return "{}".to_string();
        };
        let empty = palctx::empty_palettes(&tim);
        let stp: Vec<usize> = (0..tim.palette_count())
            .map(|p| palctx::palette_flag_counts(&tim, p).0)
            .collect();
        let mut notes: Vec<String> = Vec::new();
        shared_palette_notes(&mut notes, &empty, &stp, tim.palette_count(), true);
        serde_json::json!({
            "fate": "not_boot",
            "overwritten_palettes": [],
            "overwritten_by": null,
            "empty_palettes": empty,
            "stp_counts": stp,
            "vram_palettes": [],
            "composite": null,
            "notes": notes,
        })
        .to_string()
    }
}
