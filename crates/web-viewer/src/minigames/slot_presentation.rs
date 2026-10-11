//! The slot machine's disc-decoded art (PROT 1200 textures) and its 3D scene
//! graph, as the buffers and JSON the page draws from.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ------------------------------------------------------------- slot art
    //
    // Everything below decodes the *retail* slot machine's own textures out of
    // the visitor's disc (PROT entry 1200, the pack `FUN_801CEC94` loads). No
    // pixel ships with the site.

    /// Whether the slot machine's art pack decoded off this disc. When `false`
    /// the page must fall back to symbol *ids*, not to invented artwork.
    pub fn slot_art_ready(&self) -> bool {
        self.slot_art.is_some()
    }

    /// One reel symbol (`0..=9`) as a 64x64 RGBA8 buffer, at the exact cell and
    /// **per-symbol CLUT** the retail reel renderer `FUN_801d0fa8` samples
    /// (`U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, CLUT `0x7A80 + sym`).
    ///
    /// The palette is load-bearing: symbols 0/1/2 are one piece of artwork
    /// recoloured three ways, and so are 4/5. Empty when the art didn't decode.
    pub fn slot_symbol_rgba(&self, sym: usize) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_symbol(art, sym).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// One **bonus reel numeral** (`1..=10`) as a 64x64 RGBA8 buffer - the big
    /// coloured digit the reels carry during a bonus round.
    ///
    /// These are the retail faces, not a scaled coin font: ten 64x64 cells of
    /// their own artwork on art-pack page 1, each drawn through its own palette
    /// column (`CLUT 0x7AC0 + n - 1`), which is why every numeral is a different
    /// colour. `FUN_801d0fa8` reaches them by the same UV arithmetic it uses for
    /// the symbols - a bonus strip value simply clears `0x10`, which bumps the
    /// texpage to `0x0D` and the CLUT base to `0x7AC0`.
    ///
    /// Empty when the art pack didn't decode - in which case the page must say
    /// so, not draw digits of its own.
    pub fn slot_bonus_number_rgba(&self, number: usize) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_bonus_number(art, number).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The coin readout's font strip - the `"COIN"` label (`x = 0..64`) followed
    /// by digits `0..=9` at `x = 64 + d * 16` - as a 224x16 RGBA8 buffer
    /// (`FUN_801d2914`, CLUT `0x7A8D`).
    pub fn slot_digits_rgba(&self) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_digit_strip(art).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// One of the 3 HUD widgets the retail rasteriser `FUN_801d2cc0` draws from
    /// the descriptor table `DAT_801d347c`, decoded through *its own* texpage +
    /// CLUT: `0` = the cabinet panel, `1` = the "COIN" label, `2` = the cash-out
    /// cursor. RGBA8; pair with [`Self::slot_hud_json`] for the dimensions.
    pub fn slot_hud_rgba(&self, index: usize) -> Vec<u8> {
        let (Some(art), Some(hud)) = (self.slot_art.as_ref(), self.slot_hud.as_ref()) else {
            return Vec::new();
        };
        hud.get(index)
            .and_then(|w| minigame_art::slot_hud_sprite(art, w).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The 3 HUD widget descriptors, as parsed off the disc:
    ///
    /// ```json
    /// [ { "u": 0, "v": 16, "w": 127, "h": 239,
    ///     "page": 4, "palette": 0, "texpage": [640, 0], "clut": [0, 494] }, ... ]
    /// ```
    ///
    /// `page` is the index into the art pack the record's texpage resolves to,
    /// and `palette` the CLUT column - so a caller can re-decode the same traced
    /// rect through a different palette. That is not academic: the retail
    /// rasteriser `FUN_801d2cc0` lets the *call site* override the record's CLUT
    /// (the id's high field swaps in `0x7D0F`), so a widget's on-screen colour
    /// is not always the one its descriptor names.
    pub fn slot_hud_json(&self) -> String {
        let (Some(hud), Some(art)) = (self.slot_hud.as_ref(), self.slot_art.as_ref()) else {
            return "[]".to_string();
        };
        let rows = hud
            .iter()
            .map(|w| {
                let page = art
                    .iter()
                    .position(|t| t.image.fb_x == w.texpage.x() && t.image.fb_y == w.texpage.y())
                    .map(|p| p.to_string())
                    .unwrap_or("null".into());
                format!(
                    concat!(
                        r#"{{"u":{},"v":{},"w":{},"h":{},"page":{},"palette":{},"#,
                        r#""texpage":[{},{}],"clut":[{},{}]}}"#
                    ),
                    w.u,
                    w.v,
                    w.w,
                    w.h,
                    page,
                    w.clut.palette_index(),
                    w.texpage.x(),
                    w.texpage.y(),
                    w.clut.x(),
                    w.clut.y(),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// A whole art page decoded through one of its 16 palettes, as RGBA8. Every
    /// on-screen rect the machine draws is traced to its emitter, so a caller
    /// pairs this with the cells in [`Self::slot_scene_json`] rather than
    /// cropping by eye. Pages 0..=3 are 256x256; page 4 is 512x256.
    pub fn slot_page_rgba(&self, page: usize, palette: usize) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_page(art, page, palette).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The machine's **paytable / coin info panel** - HUD record 0, the 127x239
    /// board `FUN_801cfff0` draws at screen `(560, 128)` ("x30 back", "x9 back",
    /// "Bonus games", with the coin readout under it). RGBA8.
    ///
    /// It has its own entry point because its page is sampled as **8bpp** (the
    /// texpage attribute's colour bit), not the 4bpp its TIM header declares -
    /// decoding it as the header claims yields noise.
    pub fn slot_panel_rgba(&self) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_info_panel(art).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// Pixel width of art page `page` (`0` when the pack didn't decode).
    pub fn slot_page_width(&self, page: usize) -> usize {
        self.slot_art
            .as_ref()
            .and_then(|art| art.get(page))
            .map(|t| t.pixel_width())
            .unwrap_or(0)
    }

    // -------------------------------------------------------------- slot scene

    /// Whether the machine's 3D scene graph decoded off this disc.
    pub fn slot_scene_ready(&self) -> bool {
        self.slot_scene.is_some()
    }

    /// The five payline **line prims** for one frame, built by the ported
    /// payline pass (`FUN_801d3380`,
    /// [`legaia_engine_minigames::slot_machine::payline_prims`]) over this disc's
    /// geometry table:
    ///
    /// ```json
    /// [ { "i":0, "a":[-640,-192,-768], "b":[640,-192,-768],
    ///     "sa":[82.1,112.9], "sb":[423.9,112.9],
    ///     "rgb":[128,128,128], "semi":true, "lit":false }, ... ]
    /// ```
    ///
    /// `sa` / `sb` are the endpoints already projected onto the 640x240 slot
    /// framebuffer by the engine's one projection pass
    /// ([`legaia_engine_minigames::slot_machine::projected_paylines`]) - the same
    /// segments the native window draws.
    ///
    /// `winning_line` is retail's `DAT_801d3c8c`, compared for equality, so
    /// any value outside `0..5` (the page passes `-1` before a win) lights
    /// nothing. `a` / `b` stay model-space for a consumer that projects
    /// itself (`proj` in [`Self::slot_scene_json`]); a two-point line in
    /// `rgb` - half-blended over the scene when `semi`
    /// (GP0 `0x43` carries the semi-transparency bit, and the lit colour
    /// rewrites only the three colour bytes). `[]` when the scene did not
    /// decode.
    pub fn slot_payline_prims_json(&self, winning_line: i32) -> String {
        let Some(sc) = self.slot_scene.as_ref() else {
            return "[]".to_string();
        };
        let pos = |p: &slot_scene::Pos3| format!("[{},{},{}]", p.x, p.y, p.z);
        let rows = legaia_engine_minigames::slot_machine::projected_paylines(&sc.paylines, winning_line)
            .iter()
            .map(|p| {
                let l = &p.prim;
                format!(
                    r#"{{"i":{},"a":{},"b":{},"sa":[{},{}],"sb":[{},{}],"rgb":[{},{},{}],"semi":{},"lit":{}}}"#,
                    l.index,
                    pos(&l.a),
                    pos(&l.b),
                    p.a.0,
                    p.a.1,
                    p.b.0,
                    p.b.1,
                    l.color.0,
                    l.color.1,
                    l.color.2,
                    l.code & 0x02 != 0,
                    l.lit
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// The slot machine's **3D scene**, as the overlay's own rodata defines it,
    /// plus the projection that puts it on the retail 640x240 framebuffer.
    ///
    /// The retail machine is not a sprite collage: every element is a quad in a
    /// 3D scene projected through the GTE (see
    /// [`legaia_minigame_formats::minigame_slot_scene`]). This hands the page the same
    /// scene graph, in model space, so it can project it itself:
    ///
    /// ```json
    /// { "proj": { "ofx": 320, "ofy": 114, "z0": 8160, "sx0": 0.251,
    ///             "vanish": [240, -6.667], "aspect": 2, "xscale": 6,
    ///             "w": 640, "h": 240 },
    ///   "paylines":  [ { "a":[-640,-192,-768], "b":[640,-192,-768] }, ... ],
    ///   "row_offsets": [[1,1,1],[0,0,0],[-1,-1,-1],[-1,0,1],[1,0,-1]],
    ///   "medallions":[ { "pos":[-602,-192,-800], "art":1 }, ... ],
    ///   "lamps":     [ { "pos":[632,-192,-800] }, ... ],
    ///   "pedestals": [ { "pos":[-384,480,-800] }, ... ],
    ///   "marquee":   [ { "pos":[-554,-560,-800], "clut":0, "half":[1024,320],
    ///                    "cell":[0,0,64,64] }, ... ],
    ///   "reels":     { "x":[-512,-128,256], "w":256, "faces":8,
    ///                  "angle_base":896, "angle_step":256 },
    ///   "cells": { "medallion":[168,128,32,32], "lamp_lit":[0,224,16,16], ... },
    ///   "dots":  { "cols":78, "rows":13, "x0":-429, "y0":-640, "dx":11, "dy":12,
    ///              "z":-800, "page":3, "blink_palettes":[0,1], "u_per_nibble":4 },
    ///   "messages": [ { "w":84, "h":13, "bitmap":"0,0,1,..." }, ... ] }
    /// ```
    ///
    /// `messages` are the dot-matrix marquee's 21 bitmaps, one palette *nibble*
    /// per dot (`0` = unlit); `bitmap` is a comma-separated row-major run.
    pub fn slot_scene_json(&self) -> String {
        let Some(sc) = self.slot_scene.as_ref() else {
            return r#"{"ok":false}"#.to_string();
        };
        let pos = |p: &slot_scene::Pos3| format!("[{},{},{}]", p.x, p.y, p.z);
        let paylines = sc
            .paylines
            .iter()
            .map(|l| format!(r#"{{"a":{},"b":{}}}"#, pos(&l.a), pos(&l.b)))
            .collect::<Vec<_>>()
            .join(",");
        let medallions = sc
            .medallions
            .iter()
            .map(|m| format!(r#"{{"pos":{},"art":{}}}"#, pos(&m.pos), m.art))
            .collect::<Vec<_>>()
            .join(",");
        let lamps = sc
            .lamps
            .iter()
            .map(|m| format!(r#"{{"pos":{}}}"#, pos(&m.pos)))
            .collect::<Vec<_>>()
            .join(",");
        let pedestals = (0..slot_scene::REEL_COUNT)
            .map(|r| {
                format!(
                    r#"{{"pos":[{},{},{}]}}"#,
                    slot_scene::PEDESTAL_X0 + r as i32 * slot_scene::PEDESTAL_X_STEP,
                    slot_scene::PEDESTAL_Y,
                    slot_scene::GLASS_Z
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let marquee = sc
            .marquee
            .iter()
            .map(|m| {
                format!(
                    r#"{{"pos":{},"clut":{},"half":[{},{}],"cell":[{},{},{},{}]}}"#,
                    pos(&m.pos),
                    slot_scene::MARQUEE_CLUT_BASE.wrapping_add(m.clut_off as u16) & 0x3F,
                    m.half_w,
                    m.half_h,
                    m.u,
                    m.v,
                    m.w,
                    m.h,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let messages = sc
            .messages
            .iter()
            .map(|m| {
                let bits = m
                    .bitmap
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"w":{},"h":{},"bitmap":"{}"}}"#, m.w, m.h, bits)
            })
            .collect::<Vec<_>>()
            .join(",");
        let reel_x = (0..slot_scene::REEL_COUNT)
            .map(|r| slot_scene::reel_x(r).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let row_offsets = slot_scene::PAYLINE_ROW_OFFSETS
            .iter()
            .map(|o| format!("[{},{},{}]", o[0], o[1], o[2]))
            .collect::<Vec<_>>()
            .join(",");
        let cell = |c: (u8, u8, u8, u8)| format!("[{},{},{},{}]", c.0, c.1, c.2, c.3);
        format!(
            concat!(
                r#"{{"ok":true,"#,
                r#""proj":{{"ofx":{ofx},"ofy":{ofy},"z0":{z0},"sx0":{sx0},"#,
                r#""vanish":[{vx},{vy}],"#,
                r#""aspect":{aspect},"xscale":{xscale},"w":{sw},"h":{sh}}},"#,
                r#""paylines":[{paylines}],"row_offsets":[{row_offsets}],"#,
                r#""medallions":[{medallions}],"lamps":[{lamps}],"#,
                r#""pedestals":[{pedestals}],"marquee":[{marquee}],"#,
                r#""reels":{{"x":[{reel_x}],"w":{rw},"faces":{faces},"#,
                r#""angle_base":{ab},"angle_step":{as_},"angle_full":{af},"#,
                r#""y_radius":{yr},"z_shift":{zs},"strip_len":{sl},"#,
                r#""shade_max":{smax},"shade_bias":{sbias},"shade_gain":{sgain},"#,
                r#""shade_neutral":{sneu},"centre_row_bias":{crb}}},"#,
                r#""cells":{{"medallion":{c_med},"medallion_page":{p_med},"#,
                r#""medallion_clut_base":{cb_med},"#,
                r#""lamp_lit":{c_ll},"lamp_unlit":{c_lu},"lamp_page":{p_lamp},"#,
                r#""lamp_palette":{pal_lamp},"lamp_half":[{lhw},{lhh}],"#,
                r#""medallion_half":[{mhw},{mhh}],"pedestal_half":[{phw},{phh}],"#,
                r#""pedestal_cells":[{c_p0},{c_p1},{c_p2}],"#,
                r#""pedestal_cells_stopped":[{c_s0},{c_s1},{c_s2}],"#,
                r#""pedestal_page":{p_ped},"#,
                r#""pedestal_clut_spinning":{pcs},"pedestal_clut_stopped":{pct},"#,
                r#""marquee_page":{p_mar}}},"#,
                r#""dots":{{"cols":{dc},"rows":{dr},"x0":{dx0},"y0":{dy0},"#,
                r#""dx":{ddx},"dy":{ddy},"z":{dz},"page":{dp},"size":{dsz},"#,
                r#""blink_palettes":[{dcl0},{dcl1}],"u_per_nibble":{dun}}},"#,
                r#""messages":[{messages}]}}"#
            ),
            ofx = slot_scene::PROJ_OFX,
            ofy = slot_scene::PROJ_OFY,
            z0 = slot_scene::PROJ_Z0,
            sx0 = slot_scene::PROJ_SX0,
            vx = slot_scene::PROJ_VANISH.0,
            vy = slot_scene::PROJ_VANISH.1,
            aspect = slot_scene::PROJ_ASPECT,
            xscale = slot_scene::PROJ_X_SCALE,
            sw = slot_scene::SCREEN_W,
            sh = slot_scene::SCREEN_H,
            paylines = paylines,
            row_offsets = row_offsets,
            medallions = medallions,
            lamps = lamps,
            pedestals = pedestals,
            marquee = marquee,
            reel_x = reel_x,
            rw = slot_scene::REEL_WIDTH,
            faces = slot_scene::REEL_FACES,
            ab = slot_scene::REEL_ANGLE_BASE,
            as_ = slot_scene::REEL_ANGLE_STEP,
            af = slot_scene::ANGLE_FULL,
            yr = slot_scene::REEL_Y_RADIUS,
            zs = slot_scene::REEL_Z_SHIFT,
            sl = slot_scene::STRIP_LEN,
            smax = slot_scene::REEL_SHADE_MAX,
            sbias = slot_scene::REEL_SHADE_Z_BIAS,
            sgain = slot_scene::REEL_SHADE_Z_GAIN,
            sneu = slot_scene::SHADE_NEUTRAL,
            crb = slot_scene::PAYLINE_CENTRE_ROW_BIAS,
            c_med = cell(slot_scene::MEDALLION_CELL),
            p_med = slot_scene::MEDALLION_PAGE,
            cb_med = slot_scene::MEDALLION_CLUT_BASE & 0x3F,
            c_ll = cell(slot_scene::LAMP_CELL_LIT),
            c_lu = cell(slot_scene::LAMP_CELL_UNLIT),
            p_lamp = slot_scene::LAMP_PAGE,
            pal_lamp = slot_scene::LAMP_CLUT & 0x3F,
            lhw = slot_scene::LAMP_HALF.0,
            lhh = slot_scene::LAMP_HALF.1,
            mhw = slot_scene::MEDALLION_HALF.0,
            mhh = slot_scene::MEDALLION_HALF.1,
            phw = slot_scene::PEDESTAL_HALF.0,
            phh = slot_scene::PEDESTAL_HALF.1,
            c_p0 = cell(slot_scene::pedestal_cell(0, true)),
            c_p1 = cell(slot_scene::pedestal_cell(1, true)),
            c_p2 = cell(slot_scene::pedestal_cell(2, true)),
            c_s0 = cell(slot_scene::pedestal_cell(0, false)),
            c_s1 = cell(slot_scene::pedestal_cell(1, false)),
            c_s2 = cell(slot_scene::pedestal_cell(2, false)),
            p_ped = slot_scene::PEDESTAL_PAGE,
            pcs = slot_scene::PEDESTAL_CLUT_OPEN & 0x3F,
            pct = slot_scene::PEDESTAL_CLUT_IDLE & 0x3F,
            p_mar = slot_scene::MARQUEE_PAGE,
            dc = slot_scene::DOT_COLS,
            dr = slot_scene::DOT_ROWS,
            dx0 = slot_scene::DOT_X0,
            dy0 = slot_scene::DOT_Y0,
            ddx = slot_scene::DOT_X_STEP,
            ddy = slot_scene::DOT_Y_STEP,
            dz = slot_scene::DOT_Z,
            dp = slot_scene::DOT_PAGE,
            dsz = slot_scene::DOT_SIZE,
            dcl0 = slot_scene::DOT_BLINK_PALETTES[0],
            dcl1 = slot_scene::DOT_BLINK_PALETTES[1],
            dun = slot_scene::DOT_U_PER_NIBBLE,
            messages = messages,
        )
    }

    /// The live reel positions (`DAT_801d3cc0`) - fixed-point angles whose high
    /// byte is the strip row and whose low byte is the sub-symbol fraction. The
    /// renderer needs the fraction: the reel is a 3D cylinder and the fraction is
    /// what rotates it between symbols.
    pub fn slot_reel_pos(&self) -> Vec<i32> {
        match self.slot.as_ref() {
            Some(m) => (0..slot_scene::REEL_COUNT).map(|r| m.reel_pos(r)).collect(),
            None => Vec::new(),
        }
    }

    /// The 20-symbol display strip of `reel`, as the renderer reads it.
    pub fn slot_strip(&self, reel: usize) -> Vec<u8> {
        match self.slot.as_ref() {
            Some(m) => m.strips()[reel].to_vec(),
            None => Vec::new(),
        }
    }
}
