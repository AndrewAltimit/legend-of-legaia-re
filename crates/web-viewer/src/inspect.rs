//! Entry inspector, VRAM/mesh accessors, fog LUT, save-state replay + status exports.
use super::*;

#[wasm_bindgen]
impl LegaiaViewer {
    /// Fog LUT bytes extracted from `SCUS_942.54` at disc-load time.
    /// 4 KiB = 2048 u16 BGR555-shaped entries that the world-map overlay's
    /// per-prim leaves at `0x801F7644..0x801F8690` consult on every vertex
    /// (the shared depth-cue ramp; the per-kingdom hue mixes in from the
    /// `fog_color` field at gp-0x2DC).
    ///
    /// Returns an empty Vec when no LUT was located - the JS side should
    /// treat empty as "fall back to the kingdom-tinted baseline" and not
    /// upload anything to the renderer.
    pub fn fog_lut_bytes(&self) -> Vec<u8> {
        self.fog_lut.clone().unwrap_or_default()
    }

    /// `flags` packs the prim cmd-byte mode bits: bit 0 = semi-transparent,
    /// bit 1 = raw texture (skip color modulation). JS computes the model-view
    /// matrix from `screen_w / screen_h` (orthographic 0..w x h..0 viewport).
    pub fn save_state_prim_replay(&self, save_state_bytes: Vec<u8>) -> Result<Vec<u8>, JsValue> {
        let save = legaia_mednafen::container::SaveState::from_compressed(&save_state_bytes)
            .map_err(|e| JsValue::from_str(&format!("parse save state: {e}")))?;
        let gpu = legaia_mednafen::gpu::PsxGpu::new(&save);
        let vram = gpu
            .vram_bytes()
            .ok_or_else(|| JsValue::from_str("save state has no GPU/VRAM section"))?;
        let regs = gpu.regs();
        let dot_clock_div = match regs.display_mode_raw.unwrap_or(0) & 0x07 {
            0 => 10,
            1 => 8,
            2 => 5,
            3 => 4,
            _ => 7,
        };
        let screen_w = match regs.display_h_range {
            Some((hs, he)) => (he.saturating_sub(hs) / dot_clock_div).clamp(256, 640) as u16,
            None => 320,
        };
        let screen_h = match regs.display_v_range {
            Some((vs, ve)) => ve.saturating_sub(vs).max(224) as u16,
            None => 240,
        };
        // Slice the prim pool out of main RAM (kuseg-relative).
        let ram = save
            .main_ram()
            .map_err(|e| JsValue::from_str(&format!("read main RAM: {e}")))?;
        let pool_start_kuseg = legaia_mednafen::prim_pool::POOL_BASE_DEFAULT;
        let pool_end_kuseg = 0x80102000u32;
        let pool_lo = (pool_start_kuseg - 0x8000_0000) as usize;
        let pool_hi = (pool_end_kuseg - 0x8000_0000) as usize;
        let pool = &ram[pool_lo..pool_hi.min(ram.len())];
        let prims = legaia_mednafen::prim_pool::decode(pool, pool_start_kuseg);

        // Build vertex array. Two triangles per quad prim. Three vertices
        // for tri prims (zero-area duplicate to keep stride uniform).
        // Sprites expand to a quad based on their fixed cmd-byte size.
        let mut verts: Vec<u8> = Vec::with_capacity(prims.len() * 14 * 6);
        let push_vertex = |verts: &mut Vec<u8>,
                           x: i16,
                           y: i16,
                           u: u8,
                           v: u8,
                           cba: u16,
                           tsb: u16,
                           color: [u8; 3],
                           flags: u8| {
            verts.extend_from_slice(&x.to_le_bytes());
            verts.extend_from_slice(&y.to_le_bytes());
            verts.push(u);
            verts.push(v);
            verts.extend_from_slice(&cba.to_le_bytes());
            verts.extend_from_slice(&tsb.to_le_bytes());
            verts.extend_from_slice(&color);
            verts.push(flags);
        };
        let flags_for = |cmd: u8| -> u8 {
            let mut f = 0u8;
            if cmd & 0x02 != 0 {
                f |= 0x01;
            } // semi-transparent
            if cmd & 0x01 != 0 {
                f |= 0x02;
            } // raw texture
            f
        };
        for p in &prims {
            match *p {
                legaia_mednafen::prim_pool::Prim::PolyFt4 {
                    cmd,
                    color,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    // PSX winding: (v0, v1, v2) + (v1, v3, v2)
                    for &i in &[0usize, 1, 2, 1, 3, 2] {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyGt4 {
                    cmd,
                    colors,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for &i in &[0usize, 1, 2, 1, 3, 2] {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyFt3 {
                    cmd,
                    color,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                    // Pad to 6-vertex stride so JS can stream draw as TRIANGLES.
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyGt3 {
                    cmd,
                    colors,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::Sprt16 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                } => {
                    sprite_to_quad(&mut verts, cmd, color, pos, uv, clut, 16);
                }
                legaia_mednafen::prim_pool::Prim::Sprt8 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                } => {
                    sprite_to_quad(&mut verts, cmd, color, pos, uv, clut, 8);
                }
            }
        }
        let vertex_count = (verts.len() / 14) as u32;

        // Pack the output buffer.
        let mut out = Vec::with_capacity(2 + 2 + 4 + vram.len() + 2 + 2 + 4 + verts.len());
        out.extend_from_slice(&1024u16.to_le_bytes()); // vram_width
        out.extend_from_slice(&512u16.to_le_bytes()); // vram_height
        out.extend_from_slice(&(vram.len() as u32).to_le_bytes());
        out.extend_from_slice(vram);
        out.extend_from_slice(&screen_w.to_le_bytes());
        out.extend_from_slice(&screen_h.to_le_bytes());
        out.extend_from_slice(&vertex_count.to_le_bytes());
        out.extend_from_slice(&verts);
        Ok(out)
    }

    /// Parse a mednafen save state and return the GPU's currently-displayed
    /// framebuffer as an RGBA8 byte buffer + dimensions.
    ///
    /// Layout of the returned `Vec<u8>`:
    /// `[u16 width, u16 height, RGBA8 pixels...]` packed little-endian. JS
    /// reads the leading 4 bytes for the dimensions and then wraps the rest
    /// in an `ImageData` to blit into a 2D canvas.
    ///
    /// This is the in-game top-down world-map view: the game's renderer has
    /// already composed the ~10,000 textured polygons that form the kingdom
    /// terrain, and the result is sitting in VRAM at the display-area
    /// offset. We just read it back. Source-mesh reconstruction is a separate
    /// follow-up (the live PSX GPU prim-pool sits around `0x800AD408` and
    /// the underlying mesh / tilemap data lives in the kingdom's
    /// `scene_v12_table` at PROT base+8 - both still being characterised).
    pub fn save_state_framebuffer(&self, save_state_bytes: Vec<u8>) -> Result<Vec<u8>, JsValue> {
        let save = legaia_mednafen::container::SaveState::from_compressed(&save_state_bytes)
            .map_err(|e| JsValue::from_str(&format!("parse save state: {e}")))?;
        let gpu = legaia_mednafen::gpu::PsxGpu::new(&save);
        let vram = gpu
            .vram_bytes()
            .ok_or_else(|| JsValue::from_str("save state has no GPU/VRAM section"))?;
        let regs = gpu.regs();
        // Display-area X/Y inside VRAM (defaults if regs absent: top-left).
        let (fb_x, fb_y) = regs.display_fb.unwrap_or((0, 0));
        // Display window width is derived from horizontal range; the standard
        // PSX 320x224 / 384x240 dot-clocks land around (608..3168) ~= 320 wide
        // at 7MHz, or (488..3288) ~= 384 wide at 5MHz, divided by the clock
        // ticks per pixel. Mednafen records `HorizStart/HorizEnd` in dot-clock
        // ticks. The simplest robust extraction is to compute the active
        // pixel count via the difference, falling back to 320x240 if the
        // registers are missing.
        let dot_clock_div = match regs.display_mode_raw.unwrap_or(0) & 0x07 {
            0 => 10, // 256
            1 => 8,  // 320
            2 => 5,  // 512
            3 => 4,  // 640
            _ => 7,  // 384 (mode bit 6)
        };
        let width = match (regs.display_h_range, regs.display_mode_raw) {
            (Some((hs, he)), _) => {
                let span = he.saturating_sub(hs);
                (span / dot_clock_div).clamp(256, 640) as u16
            }
            _ => 320,
        };
        let height = match (regs.display_v_range, regs.display_mode_raw) {
            (Some((vs, ve)), _) => (ve.saturating_sub(vs) as u16).max(224),
            _ => 240,
        };
        let w = width as usize;
        let h = height as usize;
        let mut out = Vec::with_capacity(4 + w * h * 4);
        out.push((width & 0xFF) as u8);
        out.push((width >> 8) as u8);
        out.push((height & 0xFF) as u8);
        out.push((height >> 8) as u8);
        // VRAM is 1024×512 BGR555 (2 bytes/pixel). Crop the (fb_x, fb_y)
        // window. Wrap around the right edge if necessary (PSX VRAM is
        // logically a torus on the X axis at 1024 pixels).
        for row in 0..h {
            let vy = (fb_y as usize + row) & 0x1FF;
            for col in 0..w {
                let vx = (fb_x as usize + col) & 0x3FF;
                let off = (vy * 1024 + vx) * 2;
                let word = u16::from_le_bytes([vram[off], vram[off + 1]]);
                let [r, g, b, _a] = legaia_mednafen::gpu::bgr555_to_rgba8(word);
                out.push(r);
                out.push(g);
                out.push(b);
                out.push(0xFF); // opaque
            }
        }
        Ok(out)
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

    /// JSON-encoded summary of the current entry - class label, byte size,
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
        // path - the data is fixed-shape).
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

    /// JSON metadata for the boot publisher-logo TIMs from PROT 0895
    /// (`init.pak`). Returns an empty array `"[]"` if the disc doesn't
    /// have PROT 0895 or the entry doesn't parse as init.pak.
    ///
    /// Each element shape:
    ///   `{ "name": str, "width": u32, "height": u32, "mode": u32,
    ///      "fb_x": u32, "fb_y": u32 }`
    pub fn init_pak_logos_json(&self) -> String {
        let bytes = match self.prot_0895_bytes() {
            Some(b) => b,
            None => return "[]".into(),
        };
        let pak = match legaia_asset::init_pak::parse(bytes) {
            Ok(p) => p,
            Err(_) => return "[]".into(),
        };
        const NAMES: [&str; 4] = ["PROKION", "Contrail", "SCEA", "WARNING"];
        let mut out = String::from("[");
        for (i, logo) in pak.logos.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            // Decode TIM to get the bpp-corrected pixel dimensions.
            let (w, h) = match legaia_tim::parse(logo.bytes) {
                Ok(t) => (t.pixel_width() as u32, t.image.h as u32),
                Err(_) => (logo.pixel_rect.2 as u32 * 2, logo.pixel_rect.3 as u32),
            };
            out.push_str(&format!(
                r#"{{"name":"{}","width":{},"height":{},"mode":{},"fb_x":{},"fb_y":{}}}"#,
                NAMES[i], w, h, logo.mode, logo.pixel_rect.0, logo.pixel_rect.1,
            ));
        }
        out.push(']');
        out
    }

    /// Decoded RGBA8 pixels for one publisher-logo TIM (0..3). Returns
    /// an empty vec when the disc doesn't have PROT 0895 or `idx` is
    /// out of range. Width / height come from [`init_pak_logos_json`].
    pub fn init_pak_logo_rgba(&self, idx: u32) -> Vec<u8> {
        let Some(bytes) = self.prot_0895_bytes() else {
            return Vec::new();
        };
        let Ok(pak) = legaia_asset::init_pak::parse(bytes) else {
            return Vec::new();
        };
        let Some(logo) = pak.logos.get(idx as usize) else {
            return Vec::new();
        };
        let Ok(tim) = legaia_tim::parse(logo.bytes) else {
            return Vec::new();
        };
        legaia_tim::decode_rgba8(&tim, 0).unwrap_or_default()
    }

    /// Locate PROT 0895's byte range in the loaded disc image. Returns
    /// the entry bytes, or `None` when the disc isn't loaded or PROT
    /// 0895 isn't present (single-TIM mode, raw PROT.DAT without the
    /// disc walk, etc.).
    fn prot_0895_bytes(&self) -> Option<&[u8]> {
        let entries = parse_prot_toc(&self.disc)?;
        let meta = entries.iter().find(|e| e.index == 895)?;
        let off = meta.byte_offset as usize;
        let end = (meta.byte_offset + meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        Some(&self.disc[off..end])
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
        self.build_current_vram()
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Returns the mesh data for the current entry's TMD as four typed arrays
    /// concatenated by use:
    ///   `[positions(f32 ×3 per vert), uvs(u8 ×2), cba_tsb(u16 ×2), indices(u32)]`
    /// Each as a separate getter so JS can pull them as typed arrays without
    /// reparsing JSON.
    pub fn mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_current_vram_mesh() else {
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

    pub fn mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn mesh_indices(&self) -> Vec<u32> {
        self.build_current_vram_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Returns the model's bounding sphere center (`[cx, cy, cz]`) and radius
    /// `r` packed as `[cx, cy, cz, r]`. JS uses this to build the MVP matrix
    /// without re-parsing the TMD each frame.
    pub fn mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_current_vram_mesh() else {
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
        let Some(tmd_buf) = self.tmd_bytes_for(entry) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&tmd_buf) else {
            return Vec::new();
        };
        let Some(prepared) = tmd3d::prepare(&tmd, &tmd_buf) else {
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
            "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"width\":{},\"height\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_tris\":{},\"tmd_verts\":{},\"tmd_pack_count\":{}}}",
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
            e.tmd_pack_count,
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
                "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"w\":{},\"h\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_pack_count\":{}}}",
                i,
                e.meta.index,
                e.class.name(),
                w,
                h,
                bpp,
                e.tim_count,
                has_tmd,
                e.tmd_pack_count,
            ));
        }
        s.push(']');
        s
    }
}
