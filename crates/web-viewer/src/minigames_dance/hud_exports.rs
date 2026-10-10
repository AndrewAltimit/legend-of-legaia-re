//! The dance HUD's exports: the PROT 1230 page, the overlay's widget table
//! and layout, and the dancer face stamps.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The 256x256 HUD page (VRAM `(512, 0)`) decoded through palette
    /// `palette` of its own row-500 CLUT strip, as RGBA8. Palette selection
    /// is load-bearing: the widget table names a palette per element, and
    /// the beat-track flash / note tint are pure CLUT swaps over the same
    /// texels (`0x7D08` idle / `0x7D0D` flash / `0x7D0E` notes).
    pub fn dance_hud_page_rgba(&self, palette: usize) -> Vec<u8> {
        self.dance_pres
            .as_ref()
            .and_then(|p| dance_art::hud_page_rgba(&p.art, palette).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The overlay's HUD widget table, one record per id `0..=33`:
    ///
    /// ```json
    /// [ { "u":0, "v":0, "w":16, "h":24, "palette":0,
    ///     "semi":0, "top":[255,255,255], "bottom":[255,255,255] }, ... ]
    /// ```
    ///
    /// Cells index the HUD page; `palette` is the row-500 CLUT column the
    /// record names (the emitters swap it at runtime for the flash states).
    pub fn dance_widgets_json(&self) -> String {
        let Some(p) = self.dance_pres.as_ref() else {
            return "[]".to_string();
        };
        let rows = p
            .widgets
            .iter()
            .map(|w| {
                format!(
                    concat!(
                        r#"{{"u":{},"v":{},"w":{},"h":{},"palette":{},"semi":{},"#,
                        r#""top":[{},{},{}],"bottom":[{},{},{}]}}"#
                    ),
                    w.u,
                    w.v,
                    w.w,
                    w.h,
                    w.palette_index(),
                    w.semi,
                    w.rgb_top[0],
                    w.rgb_top[1],
                    w.rgb_top[2],
                    w.rgb_bottom[0],
                    w.rgb_bottom[1],
                    w.rgb_bottom[2],
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// The traced HUD geometry, so the page draws at retail positions rather
    /// than invented ones. Everything here is an immediate in a traced
    /// emitter (`FUN_801d231c` / `FUN_801d2524` / `FUN_801d32f8` /
    /// `FUN_801d3e28` and the banner spawn sites in `FUN_801cf470` /
    /// `FUN_801d1af4` / `FUN_801d40dc`), on the retail 320x240 frame.
    /// Widgets draw **centred** on their `(x, y)`.
    ///
    /// - `score_boxes`: the three boxes; the **human dancer is the centre
    ///   box** (`FUN_801d231c` draws score slot 0 at the centre digit base).
    /// - `digit_bases`: x of digit slot 0 per box; 8 slots step 16, only
    ///   significant digits draw, so a 1-digit score lands at `base + 112`.
    /// - `track`: the beat lane - arrow, caps, 12 body tiles, the scrolling
    ///   notes (`x = track.x + i*16 - (phase*16/281 + 5) - 4`, clip window
    ///   `[track.x, track.x + 0x50)`), stock markers at `y + 16`.
    /// - `banners`: spawn points (`FUN_801d3fd0` stores `x<<3` and draws at
    ///   `>>3`): countdown / READY / GO / FINISH at centre, ratings below,
    ///   stars flanking by tier (`0x38`/`0x50` for Cool/Great).
    /// `screen_offset` is the global drawing-environment offset: every HUD
    /// element in the retail VRAM capture (score-box border, track pill,
    /// marker arrow) sits exactly 4 lines below the emitter's own `y`, so the
    /// active draw environment carries a `+4` Y offset. Pixel-pinned against
    /// the parked minigame capture.
    pub fn dance_layout_json(&self) -> String {
        concat!(
            r#"{"screen":[320,240],"screen_offset":[0,4],"#,
            r#""score_boxes":{"xs":[64,160,256],"y":20,"human":1},"#,
            r#""digit_bases":{"xs":[-32,64,160],"y":20,"step":16,"slots":8},"#,
            r#""gauge":{"lv_x":88,"digit_x":96,"y":192},"#,
            r#""track":{"x":120,"y":192,"arrow":[128,184],"cap_l":116,"cap_r":204,"#,
            r#""body_tiles":12,"body_step":8,"clip_w":80,"note_step":16,"#,
            r#""stock_y":208,"stock_step":16},"#,
            r#""banners":{"centre":[160,120],"miss":[160,128],"rating":[160,144],"#,
            r#""star_off":{"cool":56,"great":80,"fever":80},"good_star_off":56},"#,
            r#""flash":{"beat_mask":3,"phase_lt":70}}"#
        )
        .to_string()
    }

    /// One dancer's live face window as RGBA8: the strip's top window with
    /// pose `pose` stamped in by the two traced `MoveImage` blits
    /// (`FUN_801d03c4`). `dancer` is the rig index `0..=3`: `0` = **Noa**
    /// (her field atlas, PROT 0874 §2), `1..=3` = the pack strips. Pair with
    /// [`Self::dance_face_meta_json`] for dimensions. Empty when the strip
    /// didn't decode.
    pub fn dance_face_rgba(&self, dancer: usize, pose: usize) -> Vec<u8> {
        let Some(p) = self.dance_pres.as_ref() else {
            return Vec::new();
        };
        let Some(rig) = dance_art::FACE_RIGS.get(dancer) else {
            return Vec::new();
        };
        let frames = match p.face_frames.get(dancer) {
            Some(f) if !f.is_empty() => f,
            _ => return Vec::new(),
        };
        let strip = if dancer == 0 {
            p.noa_atlas.as_ref()
        } else {
            dance_art::pack_strip(&p.art, rig)
        };
        let Some(strip) = strip else {
            return Vec::new();
        };
        dance_art::face_window_rgba(strip, rig, frames, pose, 0, 64)
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// Face window metadata:
    /// `[{ "w":80, "h":64, "face":[0,0,32,48], "poses":5 }, ...]` - `w`/`h`
    /// are the buffer dimensions [`Self::dance_face_rgba`] returns, `face`
    /// the sub-rect that is the visible face (the rest of the window is
    /// neighbouring atlas cells).
    pub fn dance_face_meta_json(&self) -> String {
        let Some(p) = self.dance_pres.as_ref() else {
            return "[]".to_string();
        };
        let rows = dance_art::FACE_RIGS
            .iter()
            .enumerate()
            .map(|(i, rig)| {
                let strip = if i == 0 {
                    p.noa_atlas.as_ref()
                } else {
                    dance_art::pack_strip(&p.art, rig)
                };
                let (w, ok) = match strip {
                    Some(t) => (t.pixel_width(), !p.face_frames[i].is_empty()),
                    None => (0, false),
                };
                // The visible face sub-rect: Noa's window is the 32x48
                // top-left of her 80px atlas; the pack strips are 64x64.
                let face = if i == 0 {
                    [0, 0, 32, 48]
                } else {
                    [0, 0, 64, 64]
                };
                format!(
                    r#"{{"ok":{},"w":{},"h":64,"face":[{},{},{},{}],"poses":{}}}"#,
                    ok, w, face[0], face[1], face[2], face[3], rig.poses
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }
}
