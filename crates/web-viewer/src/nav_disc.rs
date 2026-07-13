//! Disc load + entry navigation exports for `LegaiaViewer`.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str) -> Result<LegaiaViewer, JsValue> {
        console_error_panic_hook::set_once();
        // Validate that the id resolves to a canvas at construction, even
        // though we re-resolve on every render. This catches the common
        // typo case immediately instead of silently no-oping later.
        let _ = resolve_canvas(canvas_id)?;
        Ok(Self {
            canvas_id: canvas_id.to_string(),
            disc: Vec::new(),
            viewable: Vec::new(),
            current: 0,
            clut_idx: 0,
            kingdom: None,
            continent: None,
            worldmap_menu: None,
            fog_lut: None,
            item_names: None,
            spell_names: None,
            steal_table: None,
            tim_catalog: Vec::new(),
            tim_deep_catalog: Vec::new(),
            deep_section_cache: std::cell::RefCell::new(None),
            walk_ground: None,
            walk_placements: None,
            field_scene: None,
            field_npcs: None,
            prot_index: None,
            cdname_text: None,
            scene_export: None,
        })
    }

    /// Load a disc image. Auto-detects: full Mode2/2352 .bin, raw PROT.DAT,
    /// or single TIM. Returns the count of viewable entries (entries with at
    /// least one decodable TIM) for the JS UI.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        self.viewable.clear();
        self.current = 0;

        self.worldmap_menu = None;
        self.fog_lut = None;
        self.item_names = None;
        self.spell_names = None;
        self.steal_table = None;
        self.tim_catalog = Vec::new();
        self.tim_deep_catalog = Vec::new();
        *self.deep_section_cache.borrow_mut() = None;
        self.walk_ground = None;
        self.walk_placements = None;
        self.field_scene = None;
        self.field_npcs = None;
        self.prot_index = None;
        self.cdname_text = None;
        let prot_bytes = if let Some(extracted) = extract_prot_dat(&bytes) {
            // Keep the CDNAME text: `self.disc` only retains the extracted
            // PROT.DAT, but the full-scene assembler needs the scene-name ->
            // block map to resolve CDNAME labels.
            self.cdname_text = crate::disc::extract_cdname_txt(&bytes);
            console_log(&format!(
                "Detected Mode2/2352 disc image ({} MB); extracted PROT.DAT ({} MB)",
                bytes.len() / 1024 / 1024,
                extracted.len() / 1024 / 1024
            ));
            // Best-effort parse of the world-map menu out of SCUS_942.54.
            // Failures are silent: the user might be loading a region build
            // that ships a differently-named executable, and the rest of the
            // viewer still works without the menu overlay.
            if let Some(scus) = extract_scus(&bytes) {
                match worldmap_menu::parse_scus(&scus) {
                    Ok(menu) => {
                        console_log(&format!(
                            "Parsed world-map menu: {} names, {} placements",
                            menu.names.len(),
                            menu.placements.len()
                        ));
                        self.worldmap_menu = Some(menu);
                    }
                    Err(e) => console_log(&format!("worldmap_menu::parse_scus skipped: {e}")),
                }
                // Locate the world-map fog LUT - same bytes the runtime
                // consults on every vertex. The viewer auto-uploads
                // these to the GL renderer so the user doesn't need to
                // drop a separate fog_probe.lut.bin file.
                if let Some(lut) = fog_lut::find(&scus) {
                    self.fog_lut = Some(lut.to_vec());
                    console_log(&format!(
                        "Extracted fog LUT from SCUS ({} bytes, {} entries)",
                        lut.len(),
                        lut.len() / 2
                    ));
                } else {
                    console_log("fog_lut::find skipped: no LUT signature in SCUS");
                }
                // Decode the item-name table so the enemy table can show drop
                // names instead of raw ids. Silent on failure (regional build
                // with a different table address) - the page falls back to ids.
                if let Some(table) = legaia_asset::item_names::ItemNameTable::from_scus(&scus) {
                    console_log(&format!(
                        "Decoded item-name table from SCUS ({} named ids)",
                        table.named_count()
                    ));
                    self.item_names = Some(table);
                } else {
                    console_log("item_names::from_scus skipped: table not found in SCUS");
                }
                // Decode the spell-name table so the enemy table can show the
                // monster's magic attacks by name instead of raw ids.
                if let Some(table) = legaia_asset::spell_names::SpellNameTable::from_scus(&scus) {
                    self.spell_names = Some(table);
                } else {
                    console_log("spell_names::from_scus skipped: table not found in SCUS");
                }
                // Decode the per-monster steal table so the enemy table can show
                // what the Evil God Icon steals from each monster (item + chance).
                if let Some(table) = legaia_asset::steal_table::StealTable::from_scus(&scus) {
                    self.steal_table = Some(table);
                } else {
                    console_log("steal_table::from_scus skipped: table not found in SCUS");
                }
            }
            extracted
        } else if parse_prot_toc(&bytes).is_some() {
            console_log("Loading bytes as raw PROT.DAT");
            bytes
        } else if let Ok(tim) = legaia_tim::parse(&bytes) {
            console_log(&format!(
                "Loading standalone TIM ({:?}, {}x{})",
                tim.mode,
                tim.pixel_width(),
                tim.image.h
            ));
            self.disc = bytes;
            self.render_tim_at(&self.disc.clone(), 0, "Standalone TIM")?;
            return Ok(0);
        } else {
            return Err(JsValue::from_str(
                "Unrecognised buffer: not a Mode2/2352 disc, not a PROT.DAT, not a TIM",
            ));
        };

        let entries = parse_prot_toc(&prot_bytes)
            .ok_or_else(|| JsValue::from_str("PROT TOC parse failed"))?;

        // Build the flat TIM catalog from the whole image + TOC spans. This
        // catches TIMs in the unindexed system-UI gap that the per-entry
        // tim-scan below never sees, and gives the UI a stable per-TIM id.
        let spans: Vec<(u64, u64, u32)> = entries
            .iter()
            .map(|e| (e.byte_offset, e.size_bytes, e.index))
            .collect();
        self.tim_catalog = tim_catalog::build_from_spans(&prot_bytes, &spans);
        console_log(&format!(
            "Cataloged {} TIMs in PROT.DAT",
            self.tim_catalog.len()
        ));

        // Deep tier: LZS-decompress every entry and catalog the compressed
        // TIMs the flat (raw-bytes) catalog can't see.
        self.tim_deep_catalog = tim_deep_catalog::build_from_spans(&prot_bytes, &spans);
        console_log(&format!(
            "Cataloged {} TIMs inside LZS-compressed sections",
            self.tim_deep_catalog.len()
        ));

        console_log(&format!(
            "Found {} PROT entries - classifying…",
            entries.len()
        ));
        self.disc = prot_bytes;

        // Index the strict TIM catalog by owning entry so the entry browser
        // shows the SAME TIM the catalog does (strict-validated, jPSXdec
        // parity) instead of whatever the lenient per-entry scan picked first.
        // Entries whose TIMs only exist inside LZS-compressed sections aren't
        // in the catalog (the flat scan doesn't decompress), so those still
        // fall back to the lenient scan below.
        // entry index -> its catalog TIM ids, ascending (the catalog is built
        // in ascending-offset order, so push order is already correct).
        let mut catalog_by_entry: std::collections::HashMap<u32, Vec<u32>> =
            std::collections::HashMap::new();
        for t in &self.tim_catalog {
            if let Some(idx) = t.entry_index {
                catalog_by_entry.entry(idx).or_default().push(t.id);
            }
        }

        // Classify + tim-scan each entry. Skip non-viewable classes early to
        // keep this fast on the user's main thread. The expensive step is
        // tim_scan::scan_entry which LZS-decompresses + walks magic offsets;
        // we only run it on classes that can plausibly contain TIMs.
        let mut viewable = Vec::new();
        for e in entries {
            let off = e.byte_offset as usize;
            let end = (e.byte_offset + e.size_bytes) as usize;
            if end > self.disc.len() {
                continue;
            }
            let buf = &self.disc[off..end];
            let report = classify(buf);

            let cat_ids: &[u32] = catalog_by_entry
                .get(&e.index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let has_cat = !cat_ids.is_empty();

            // Skip classes that never carry TIMs - unless the catalog already
            // found a (strict) TIM here, in which case show it regardless.
            if !has_cat
                && matches!(
                    report.class,
                    Class::Empty
                        | Class::Tiny
                        | Class::AllZeros
                        | Class::MostlyZeros
                        | Class::ConstantByte
                        | Class::PochiFiller
                        | Class::MipsOverlay
                        | Class::OverlayPtrTable
                        | Class::SceneVabStream
                )
            {
                continue;
            }

            // Prefer the strict catalog TIM for this entry; fall back to the
            // lenient scan only when the catalog has none (LZS-only entries).
            let (first_tim, tim_count) = if has_cat {
                let t = &self.tim_catalog[cat_ids[0] as usize];
                let ft = TimHit {
                    source: TimSource::Raw(t.offset_in_entry as usize),
                    width: t.width,
                    height: t.height,
                    bpp: t.bpp,
                };
                (Some(ft), cat_ids.len())
            } else {
                let scan = tim_scan::scan_entry(buf);
                let tim_count = scan.hits.len();
                // First hit whose bytes actually decode (not just magic match).
                let mut first_tim = None;
                for (source, hit) in &scan.hits {
                    let bytes_for_parse: Option<&[u8]> = match source {
                        tim_scan::Source::Raw => Some(&buf[hit.offset..]),
                        tim_scan::Source::Lzs(idx) => {
                            scan.lzs_sections.get(*idx).map(|s| &s[hit.offset..])
                        }
                    };
                    if let Some(b) = bytes_for_parse
                        && legaia_tim::parse(b).is_ok()
                    {
                        let ts = match source {
                            tim_scan::Source::Raw => TimSource::Raw(hit.offset),
                            tim_scan::Source::Lzs(idx) => TimSource::Lzs {
                                section: *idx,
                                offset: hit.offset,
                            },
                        };
                        first_tim = Some(TimHit {
                            source: ts,
                            width: hit.width,
                            height: hit.height,
                            bpp: hit.bpp,
                        });
                        break;
                    }
                }
                (first_tim, tim_count)
            };

            // Detect a leading TMD for the 3D viewer path (raw, scene_tmd_stream,
            // or the first of an LZS-packed environment-geometry mesh pack).
            let (tmd_source, tmd_pack_count) = detect_tmd_in_entry(buf, report.class);

            // Skip entries that have neither a viewable TIM nor a parseable TMD.
            if first_tim.is_none() && tmd_source.is_none() {
                continue;
            }

            viewable.push(ViewerEntry {
                meta: e,
                class: report.class,
                first_tim,
                tim_count,
                tmd_source,
                tmd_pack_count,
            });
        }

        console_log(&format!(
            "Filtered to {} viewable entries (any embedded TIM)",
            viewable.len()
        ));
        self.viewable = viewable;
        // Don't render here - the JS side decides which canvas surface
        // (2D blit vs WebGL2 mesh viewer vs assembled world map) is
        // active and calls the matching accessor. Rendering at load time
        // unconditionally requested a 2D context, which failed when the
        // canvas had been WebGL2-bound by a prior session in the same
        // page lifetime ("no 2d context (canvas was already bound to
        // webgl...)" - the world-overview page hits this any time the
        // user reloads a disc after entering the top-down or mesh view).
        Ok(self.viewable.len() as u32)
    }

    pub fn entry_count(&self) -> u32 {
        self.viewable.len() as u32
    }

    pub fn current_index(&self) -> u32 {
        self.viewable
            .get(self.current)
            .map(|e| e.meta.index)
            .unwrap_or(0)
    }

    pub fn next_entry(&mut self) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        self.current = (self.current + 1) % self.viewable.len();
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn prev_entry(&mut self) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        self.current = if self.current == 0 {
            self.viewable.len() - 1
        } else {
            self.current - 1
        };
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    /// Jump to the slot in the filtered list (NOT the PROT index). Used by
    /// the dropdown / list-click UI.
    pub fn set_slot(&mut self, slot: u32) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        let s = (slot as usize).min(self.viewable.len() - 1);
        self.current = s;
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn set_clut(&mut self, idx: u32) -> Result<(), JsValue> {
        self.clut_idx = idx as usize;
        self.render_current()
    }
}
