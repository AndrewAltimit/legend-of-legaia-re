//! WebAssembly bindings for browsing a Legend of Legaia disc image in the browser.
//!
//! Auto-detects: full Mode2/2352 .bin disc, raw PROT.DAT, or a single TIM.
//! After loading a disc, classifies every PROT entry via `legaia_asset::categorize`
//! and pre-scans them for embedded TIMs so the UI shows a filtered, browsable
//! list of viewable entries instead of every raw entry.

pub mod disc;
pub mod tmd3d;

use disc::{EntryMeta, extract_prot_dat, parse_prot_toc};
use legaia_asset::categorize::{Class, classify};
use legaia_asset::tim_scan;
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

fn console_log(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
}

/// One entry's metadata + its first viewable TIM hit (if any).
#[derive(Clone)]
struct ViewerEntry {
    meta: EntryMeta,
    class: Class,
    first_tim: Option<TimHit>,
    /// Total number of TIM hits found by tim_scan (for the status line).
    tim_count: usize,
    /// Where the entry's leading TMD lives (if any). Used by the 3D viewer
    /// path. None ⇒ no TMD; render the TIM instead (or a "no TMD" message).
    tmd_source: Option<TmdSource>,
}

#[derive(Clone, Copy)]
enum TmdSource {
    /// Bare TMD at offset 0 of the entry.
    Direct { offset: usize },
    /// scene_tmd_stream wrapper: 4-byte chunk0 header + bare TMD.
    SceneTmdStream { offset: usize, len: usize },
}

#[derive(Clone)]
struct TimHit {
    /// Source for the bytes: Raw is offset within the entry; Lzs(i, off) is
    /// section index + offset within that decompressed section.
    source: TimSource,
    width: u32,
    height: u32,
    bpp: u32,
}

#[derive(Clone)]
enum TimSource {
    Raw(usize),
    Lzs { section: usize, offset: usize },
}

#[wasm_bindgen]
pub struct LegaiaViewer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    disc: Vec<u8>,
    /// Filtered list of entries visible in the UI. Order matches PROT order.
    viewable: Vec<ViewerEntry>,
    current: usize,
    /// CLUT index to use when rendering paletted TIMs.
    clut_idx: usize,
}

#[wasm_bindgen]
impl LegaiaViewer {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str) -> Result<LegaiaViewer, JsValue> {
        console_error_panic_hook::set_once();
        let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let doc = win
            .document()
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let canvas = doc
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str("canvas not found"))?
            .dyn_into::<HtmlCanvasElement>()?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("no 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        Ok(Self {
            canvas,
            ctx,
            disc: Vec::new(),
            viewable: Vec::new(),
            current: 0,
            clut_idx: 0,
        })
    }

    /// Load a disc image. Auto-detects: full Mode2/2352 .bin, raw PROT.DAT,
    /// or single TIM. Returns the count of viewable entries (entries with at
    /// least one decodable TIM) for the JS UI.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        self.viewable.clear();
        self.current = 0;

        let prot_bytes = if let Some(extracted) = extract_prot_dat(&bytes) {
            console_log(&format!(
                "Detected Mode2/2352 disc image ({} MB); extracted PROT.DAT ({} MB)",
                bytes.len() / 1024 / 1024,
                extracted.len() / 1024 / 1024
            ));
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
        console_log(&format!(
            "Found {} PROT entries — classifying…",
            entries.len()
        ));
        self.disc = prot_bytes;

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

            // Skip classes that never carry TIMs.
            if matches!(
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
            ) {
                continue;
            }

            let scan = tim_scan::scan_entry(buf);
            let tim_count = scan.hits.len();

            // Find the first hit whose bytes actually decode (not just magic match).
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

            // Detect a leading TMD for the 3D viewer path.
            let tmd_source = detect_tmd_in_entry(buf, report.class);

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
            });
        }

        console_log(&format!(
            "Filtered to {} viewable entries (any embedded TIM)",
            viewable.len()
        ));
        self.viewable = viewable;
        if !self.viewable.is_empty() {
            self.render_current()?;
        }
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
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn set_clut(&mut self, idx: u32) -> Result<(), JsValue> {
        self.clut_idx = idx as usize;
        self.render_current()
    }

    /// True if the current entry has a parseable TMD, suitable for the 3D
    /// rendering path. JS uses this to decide whether to switch to the 3D
    /// render loop instead of the TIM blit.
    pub fn current_has_tmd(&self) -> bool {
        self.viewable
            .get(self.current)
            .map(|e| e.tmd_source.is_some())
            .unwrap_or(false)
    }

    /// JSON-encoded summary of the current entry — class label, byte size,
    /// MES record count (if any), SEQ presence (if any), VAB presence
    /// (if any). The JS side parses this and shows it in the inspector
    /// panel without needing N round-trips for each individual field.
    pub fn current_entry_info_json(&self) -> String {
        let Some(entry) = self.viewable.get(self.current) else {
            return "{}".into();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        let buf: &[u8] = if end <= self.disc.len() {
            &self.disc[off..end]
        } else {
            &[]
        };
        let class = format!("{:?}", entry.class);
        let mes = legaia_mes::parse(buf).ok();
        let mes_format = mes.as_ref().map(|m| format!("{:?}", m.format));
        let mes_records = mes.as_ref().and_then(|m| m.records.as_ref().map(Vec::len));
        let mes_offsets = mes
            .as_ref()
            .and_then(|m| m.offset_table.as_ref().map(Vec::len));
        let seq_off = buf.windows(4).position(|w| w == b"pQES");
        let vab_off = buf.windows(4).position(|w| w == b"pBAV");
        let tim_count = entry.tim_count;
        let prot_idx = entry.meta.index;

        // Hand-rolled JSON to keep wasm size down (no serde_json on this
        // path — the data is fixed-shape).
        let mut s = String::new();
        s.push('{');
        s.push_str(&format!(r#""prot_index":{prot_idx},"#));
        s.push_str(&format!(r#""size_bytes":{},"#, buf.len()));
        s.push_str(&format!(r#""class":"{class}","#));
        s.push_str(&format!(r#""tim_count":{tim_count},"#));
        s.push_str(&format!(r#""has_tmd":{},"#, entry.tmd_source.is_some()));
        if let Some(off) = vab_off {
            s.push_str(&format!(r#""vab_offset":{off},"#));
        }
        if let Some(off) = seq_off {
            s.push_str(&format!(r#""seq_offset":{off},"#));
            // Try parsing the SEQ header for the JS-visible BPM display.
            if let Ok(hdr) = legaia_seq::parse_header(&buf[off..]) {
                s.push_str(&format!(r#""seq_ppqn":{},"#, hdr.ppqn));
                s.push_str(&format!(r#""seq_bpm":{:.1},"#, hdr.bpm()));
            }
        }
        if let Some(fmt) = mes_format {
            s.push_str(&format!(r#""mes_format":"{fmt}","#));
        }
        if let Some(n) = mes_records {
            s.push_str(&format!(r#""mes_records":{n},"#));
        }
        if let Some(n) = mes_offsets {
            s.push_str(&format!(r#""mes_offsets":{n},"#));
        }
        // Trim trailing comma if present.
        if s.ends_with(',') {
            s.pop();
        }
        s.push('}');
        s
    }

    /// Resolve a MES message id to its first 64 bytes as a hex string (for
    /// preview in the inspector panel). Returns an empty string if the
    /// current entry isn't a MES container or `text_id` is out of range.
    pub fn current_mes_message_hex(&self, text_id: u32) -> String {
        let Some(entry) = self.viewable.get(self.current) else {
            return String::new();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return String::new();
        }
        let buf = &self.disc[off..end];
        let Ok(blob) = legaia_mes::parse(buf) else {
            return String::new();
        };
        let body_off = match blob.format {
            legaia_mes::Format::Compact => blob
                .offset_table
                .as_ref()
                .and_then(|t| t.get(text_id as usize).copied())
                .map(|v| v as usize),
            legaia_mes::Format::Records => blob
                .records
                .as_ref()
                .and_then(|r| r.get(text_id as usize))
                .map(|r| r.offset),
        };
        let Some(start) = body_off else {
            return String::new();
        };
        if start >= buf.len() {
            return String::new();
        }
        let n = (buf.len() - start).min(64);
        let mut s = String::with_capacity(n * 3);
        for &b in &buf[start..start + n] {
            s.push_str(&format!("{b:02X} "));
        }
        s
    }

    /// Build a 1024×512 PSX VRAM from every TIM the current entry contains.
    /// Returns the raw bytes (2 MB if a CLUT block is present, but VRAM is
    /// always exactly 1 MB = 1024×512×2). Used by the WebGL2 path to upload
    /// to a R16UI texture.
    pub fn current_vram_bytes(&self) -> Vec<u8> {
        let Some(entry) = self.viewable.get(self.current) else {
            return Vec::new();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Vec::new();
        }
        let buf = &self.disc[off..end];

        let mut vram = legaia_tim::Vram::new();

        // Walk every TIM in this entry (raw + LZS-decompressed) and upload
        // each at its declared (fb_x, fb_y).
        let scan = tim_scan::scan_entry(buf);
        for (source, hit) in &scan.hits {
            let tim_buf: Option<&[u8]> = match source {
                tim_scan::Source::Raw => Some(&buf[hit.offset..]),
                tim_scan::Source::Lzs(idx) => scan.lzs_sections.get(*idx).map(|s| &s[hit.offset..]),
            };
            if let Some(b) = tim_buf
                && let Ok(tim) = legaia_tim::parse(b)
            {
                vram.upload_tim(&tim);
            }
        }

        vram.as_bytes().to_vec()
    }

    /// Returns the mesh data for the current entry's TMD as four typed arrays
    /// concatenated by use:
    ///   `[positions(f32 ×3 per vert), uvs(u8 ×2), cba_tsb(u16 ×2), indices(u32)]`
    /// Each as a separate getter so JS can pull them as typed arrays without
    /// reparsing JSON.
    pub fn mesh_positions(&self) -> Vec<f32> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf);
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn mesh_uvs(&self) -> Vec<u8> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf);
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf);
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn mesh_indices(&self) -> Vec<u32> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf).indices
    }

    /// Returns the model's bounding sphere center (`[cx, cy, cz]`) and radius
    /// `r` packed as `[cx, cy, cz, r]`. JS uses this to build the MVP matrix
    /// without re-parsing the TMD each frame.
    pub fn mesh_bounds(&self) -> Vec<f32> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return vec![0.0; 4];
        };
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf);
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

    /// Render the current entry's TMD at the given rotation into a flat
    /// `Vec<f32>` of triangle data (7 floats per triangle, painter's-sorted
    /// back-to-front).
    ///
    /// Format per triangle: `[x0, y0, x1, y1, x2, y2, brightness 0..1]`.
    ///
    /// Returns an empty vec if the current entry has no TMD or the TMD has
    /// no triangles.
    #[allow(clippy::too_many_arguments)]
    pub fn render_tmd_triangles(
        &self,
        yaw: f32,
        pitch: f32,
        distance: f32,
        pan_x: f32,
        pan_y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<f32> {
        let Some(entry) = self.viewable.get(self.current) else {
            return Vec::new();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Vec::new();
        }
        let buf = &self.disc[off..end];

        let (tmd_buf, tmd_len) = match entry.tmd_source {
            Some(TmdSource::Direct { offset }) => (&buf[offset..], buf.len() - offset),
            Some(TmdSource::SceneTmdStream { offset, len }) => (&buf[offset..offset + len], len),
            None => return Vec::new(),
        };
        let _ = tmd_len;
        let Ok(tmd) = legaia_tmd::parse(tmd_buf) else {
            return Vec::new();
        };
        let Some(prepared) = tmd3d::prepare(&tmd, tmd_buf) else {
            return Vec::new();
        };
        tmd3d::render(
            &prepared, yaw, pitch, distance, pan_x, pan_y, viewport_w, viewport_h,
        )
    }

    /// JSON status string: PROT index, class name, dims, current slot.
    pub fn status(&self) -> String {
        let Some(e) = self.viewable.get(self.current) else {
            return "{}".into();
        };
        let (w, h, bpp) = match &e.first_tim {
            Some(t) => (t.width, t.height, t.bpp),
            None => (0, 0, 0),
        };
        let has_tmd = e.tmd_source.is_some();
        let (tmd_tris, tmd_verts) = match e.tmd_source {
            Some(_) => self.tmd_stats(e),
            None => (0, 0),
        };
        format!(
            "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"width\":{},\"height\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_tris\":{},\"tmd_verts\":{}}}",
            self.current,
            e.meta.index,
            e.class.name(),
            w,
            h,
            bpp,
            e.tim_count,
            has_tmd,
            tmd_tris,
            tmd_verts,
        )
    }

    /// Returns a JSON array describing every viewable entry: PROT index, class,
    /// dimensions, has-TMD flag. The UI uses this to populate a sidebar list / search.
    pub fn entry_list_json(&self) -> String {
        let mut s = String::from("[");
        for (i, e) in self.viewable.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (w, h, bpp) = match &e.first_tim {
                Some(t) => (t.width, t.height, t.bpp),
                None => (0, 0, 0),
            };
            let has_tmd = e.tmd_source.is_some();
            s.push_str(&format!(
                "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"w\":{},\"h\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{}}}",
                i,
                e.meta.index,
                e.class.name(),
                w,
                h,
                bpp,
                e.tim_count,
                has_tmd,
            ));
        }
        s.push(']');
        s
    }
}

impl LegaiaViewer {
    fn render_current(&mut self) -> Result<(), JsValue> {
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
        // entry has a TMD, we leave the canvas blank so the rAF loop can
        // take over. The TIM path is the default for entries without a TMD.
        if entry.tmd_source.is_some() {
            // Hand off to the JS-driven 3D loop. Just clear the canvas.
            self.canvas.set_width(800);
            self.canvas.set_height(600);
            self.ctx.set_fill_style_str("#0a0e15");
            self.ctx.fill_rect(0.0, 0.0, 800.0, 600.0);
            return Ok(());
        }

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

    /// Parse the current entry's TMD if it has one. Returns the parsed TMD
    /// plus the byte slice it was parsed from (caller may need it again to
    /// walk per-object primitive sections).
    fn parse_current_tmd(&self) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let entry = self.viewable.get(self.current)?;
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        let buf = &self.disc[off..end];
        let tmd_buf: Vec<u8> = match entry.tmd_source? {
            TmdSource::Direct { offset } => buf[offset..].to_vec(),
            TmdSource::SceneTmdStream { offset, len } => buf[offset..offset + len].to_vec(),
        };
        let tmd = legaia_tmd::parse(&tmd_buf).ok()?;
        Some((tmd, tmd_buf))
    }

    fn tmd_stats(&self, entry: &ViewerEntry) -> (usize, usize) {
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return (0, 0);
        }
        let buf = &self.disc[off..end];
        let tmd_buf = match entry.tmd_source {
            Some(TmdSource::Direct { offset }) => &buf[offset..],
            Some(TmdSource::SceneTmdStream { offset, len }) => &buf[offset..offset + len],
            None => return (0, 0),
        };
        let Ok(tmd) = legaia_tmd::parse(tmd_buf) else {
            return (0, 0);
        };
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, tmd_buf);
        (mesh.triangle_count(), mesh.vertex_count())
    }

    fn render_tim_at(&self, src: &[u8], offset: usize, label: &str) -> Result<(), JsValue> {
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

        self.canvas.set_width(w);
        self.canvas.set_height(h);

        let clamped = rgba;
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&clamped), w, h)?;
        self.ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    fn draw_message(&self, msg: &str) -> Result<(), JsValue> {
        self.canvas.set_width(800);
        self.canvas.set_height(200);
        self.ctx.set_fill_style_str("#0a0e15");
        self.ctx.fill_rect(0.0, 0.0, 800.0, 200.0);
        self.ctx.set_fill_style_str("#8b949e");
        self.ctx
            .set_font("16px JetBrains Mono, ui-monospace, monospace");
        self.ctx.fill_text(msg, 16.0, 100.0)?;
        Ok(())
    }
}

/// Detect a parseable Legaia TMD inside a PROT entry buffer. Two layouts:
///   - SceneTmdStream entries: `[u32 chunk0][bare TMD][streaming chunks]`.
///     The asset crate's detector returns the exact TMD byte range.
///   - Bare TMD at offset 0 (rare; caught by raw TMD magic check).
///
/// Returns None if no TMD is present.
fn detect_tmd_in_entry(buf: &[u8], class: Class) -> Option<TmdSource> {
    if class == Class::SceneTmdStream
        && let Some(s) = legaia_asset::scene_tmd_stream::detect(buf)
    {
        let r = s.tmd_range();
        // Validate the TMD actually parses; the detector is structural only.
        if legaia_tmd::parse(&buf[r.start..r.end]).is_ok() {
            return Some(TmdSource::SceneTmdStream {
                offset: r.start,
                len: r.end - r.start,
            });
        }
    }
    // Bare TMD at offset 0?
    if buf.len() >= legaia_tmd::HEADER_SIZE
        && let Ok(_) = legaia_tmd::parse(buf)
    {
        return Some(TmdSource::Direct { offset: 0 });
    }
    None
}
