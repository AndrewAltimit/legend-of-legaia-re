//! The casino slot machine's **3D scene** as screen primitives: the cabinet
//! mesh, the reel cylinders, the glass furniture, the dot-matrix marquee and
//! the coin HUD, each sampling the machine's own art pages in VRAM.
//!
//! Every element is the emitter the overlay runs, projected through the
//! machine's fitted projection (`legaia_asset::minigame_slot_scene::project`)
//! and linked at a depth-derived ordering-table bucket, so the painter's order
//! is retail's OT order rather than a hand-picked layer stack:
//!
//! | element | retail emitter | packet |
//! |---|---|---|
//! | cabinet body | the shared TMD renderer over PROT 1200 descriptor 1 | flat / gouraud tri + quad |
//! | reels | `FUN_801d0fa8` | `POLY_GT4` `0x3C`, depth-cued shade per edge |
//! | pedestals, medallions, marquee panel, lamps | `FUN_801d08e4` -> `FUN_800195a8` | `POLY_FT4` `0x2C` |
//! | dot matrix | `FUN_801d0e1c` | `SPRT` `0x64`, 2x2, linked at OT `10` |
//! | coin digits | `FUN_801d2914` | `POLY_FT4` `0x2E`, linked at OT `1` |
//! | info panel, "COIN" label | `FUN_801d2cc0` (from `FUN_801cfff0`) | `POLY_GT4` `0x3C \| semi << 1` |
//!
//! The paylines are not here: they are `crate::ui_slot_paylines`, which every
//! host already draws.
//!
//! Coordinates are produced on the machine's 640x240 framebuffer and halved
//! horizontally into the 320x240 display space every screen primitive is
//! authored in, so a corner lands to the nearest even framebuffer column.
//!
//! The art the quads sample is the five-TIM pack (PROT 1200) uploaded at its
//! own framebuffer destinations; a host that draws this list binds a VRAM
//! holding it ([`legaia_asset::minigame_art::parse_art_pack`]).

use legaia_asset::minigame_art::{
    SLOT_BONUS_CLUT_BASE, SLOT_DIGIT_CLUT, SLOT_SYMBOL_CLUT_BASE, SlotHudWidget,
};
use legaia_asset::minigame_slot_scene as sc;

use crate::screen_prim::{FlatQuad, ScreenPrim, ScreenQuad};

/// OT bucket of the dot matrix (`FUN_801d0e1c` links every dot and its
/// `DR_TPAGE` at `ot + 0x28`, i.e. bucket 10).
pub const DOTS_OT: u32 = 10;
/// OT bucket of the coin digits (`FUN_801d2914`: `ot + 4`).
pub const DIGITS_OT: u32 = 1;
/// OT bucket of the "COIN" label. `FUN_801d2cc0` links at
/// `DAT_801d379c` (3 after the first call), and the label is linked before
/// the panel in the same bucket, so it draws over it.
pub const LABEL_OT: u32 = 3;
/// OT bucket of the info panel (same bucket as the label, linked after it, so
/// drawn first - one deeper here).
pub const PANEL_OT: u32 = 4;
/// First OT bucket of the depth-sorted 3D scene. Everything projected sits
/// behind the dot matrix and the HUD.
const SCENE_OT_BASE: u32 = 16;

/// Texpage of the reel symbol page (`0x0C` = `(768, 0)`); a bonus value adds
/// one (`0x0D` = `(832, 0)`).
const REEL_TPAGE: u16 = 0x0C;
/// Texpage of the medallion page.
const MEDALLION_TPAGE: u16 = 0x0C;
/// Texpage of the pedestals, lamps and marquee panel (`0x1C` = `(768, 256)`).
const FURNITURE_TPAGE: u16 = 0x1C;
/// Texpage of the dot matrix's lamp swatches (`0x1D` = `(832, 256)`).
const DOTS_TPAGE: u16 = 0x1D;
/// CLUT row of the dots' two blink palettes (`0x7B40` = row 493, column 0):
/// retail `MoveImage`s column `blink & 1` into column 15 (`0x7B4F`) every
/// frame and samples that; sampling the source column directly is the same
/// texels.
const DOTS_CLUT_ROW: u16 = 0x7B40;
/// Neutral texture modulation (`0x80` = texel unchanged).
const NEUTRAL: u32 = 0x0080_8080;

/// The casino slot machine's resident data: what the overlay init
/// `FUN_801CEC94` loads before the reel state machine runs - the five-TIM art
/// pack and the cabinet mesh (PROT 1200), and the scene graph + HUD widget
/// table out of the overlay's own rodata (PROT 0975) - plus the VRAM the art
/// pack uploads into, which is the whole texture set the machine samples.
pub struct SlotCabinetAssets {
    pub scene: legaia_asset::minigame_slot_scene::SlotScene,
    pub cabinet: Option<legaia_asset::minigame_slot_scene::SlotCabinetMesh>,
    pub hud: Vec<legaia_asset::minigame_art::SlotHudWidget>,
    pub vram: legaia_tim::Vram,
}

impl SlotCabinetAssets {
    /// Decode the machine's data off the disc through `read` (an extraction
    /// PROT index to its raw bytes). `Err` names the half that did not
    /// decode - the art pack or the scene graph; the cabinet mesh is
    /// optional, and without it the reels and furniture still draw.
    pub fn load(read: impl Fn(usize) -> Option<Vec<u8>>) -> Result<Self, String> {
        use legaia_asset::minigame_art as art;
        let art_raw = read(art::SLOT_ART_PROT_INDEX).ok_or("art pack (PROT 1200) unreadable")?;
        let overlay = read(legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX)
            .ok_or("slot overlay (PROT 0975) unreadable")?;
        let tims = art::parse_art_pack(&art_raw)
            .map_err(|e| format!("art pack (PROT 1200) did not decode: {e:#}"))?;
        let (idx, w, _) = art::slot_page_indices(&tims, sc::DOT_PAGE)
            .map_err(|e| format!("dot page did not decode: {e:#}"))?;
        let scene = sc::parse_scene(&overlay, &idx, w)
            .map_err(|e| format!("scene graph (PROT 0975) did not decode: {e:#}"))?;
        let hud = art::parse_slot_hud(&overlay).unwrap_or_default();
        let cabinet = sc::parse_cabinet(&art_raw).ok();
        let mut vram = legaia_tim::Vram::new();
        for t in &tims {
            vram.upload_tim(t);
        }
        Ok(Self {
            scene,
            cabinet,
            hud,
            vram,
        })
    }
}

/// Everything one frame of the machine draws from.
pub struct SlotCabinetInput<'a> {
    /// The scene graph off the overlay's rodata.
    pub scene: &'a sc::SlotScene,
    /// The cabinet body, when its descriptor decoded.
    pub cabinet: Option<&'a sc::SlotCabinetMesh>,
    /// The three HUD widget records (`DAT_801d347c`).
    pub hud: &'a [SlotHudWidget],
    /// Each reel's raw position (`DAT_801d3cc0[r]`).
    pub reel_pos: [i32; 3],
    /// Each reel's display strip.
    pub strips: [&'a [u8]; 3],
    /// Each reel's per-spin "stop accepted" flag (`DAT_801d3d00[r]`).
    pub stopped: [bool; 3],
    /// The winning-line word (`DAT_801d3c8c`); `-1` lights nothing.
    pub winning_line: i32,
    /// The dot buffer, `buf[col * DOT_STRIDE + row]` = palette nibble.
    pub dots: &'a [u8],
    /// The blink counter (`DAT_801d3c9c`) after this frame's advance.
    pub blink: u32,
    /// The balance the coin readout prints (`DAT_801d4114`).
    pub balance: i32,
}

/// The 640-wide framebuffer point `(x, y)` in display space.
fn disp(p: (f32, f32)) -> (i16, i16) {
    ((p.0 * 0.5).round() as i16, p.1.round() as i16)
}

/// Depth-derived OT bucket for model-space depth `z` (`-z` is toward the
/// viewer): proportional to the view-space depth the GTE's `OTZ` reads.
fn depth_ot(z: f32) -> u32 {
    SCENE_OT_BASE + ((sc::PROJ_Z0 + z).max(0.0) as u32 >> 2)
}

fn shade_word(s: i32) -> u32 {
    let s = s.clamp(0, 0xFF) as u32;
    (s << 16) | (s << 8) | s
}

#[allow(clippy::too_many_arguments)]
fn textured(
    xy: [(f32, f32); 4],
    uv: [(u8, u8); 4],
    clut: u16,
    tpage: u16,
    color: u32,
    gouraud: Option<[u32; 4]>,
    semi: bool,
    ot: u32,
) -> ScreenPrim {
    ScreenPrim::Textured(ScreenQuad {
        xy: xy.map(disp),
        uv,
        clut,
        tpage,
        color,
        gouraud,
        semi_transparent: semi,
        ot_index: ot,
        depth: None,
    })
}

/// The cabinet body: every triangle projected, back faces dropped, linked by
/// its mean depth.
fn cabinet_prims(mesh: &sc::SlotCabinetMesh, out: &mut Vec<ScreenPrim>) {
    for t in &mesh.tris {
        let p = t
            .pos
            .map(|v| sc::project(v.x as i32, v.y as i32, v.z as i32));
        // NCLIP: the TMD renderer culls a single-sided prim that faces away.
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area <= 0.0 {
            continue;
        }
        let z = t.pos.iter().map(|v| v.z as f32).sum::<f32>() / 3.0;
        let c = |i: usize| [t.rgb[i][0], t.rgb[i][1], t.rgb[i][2], 0xFF];
        let xy = [disp(p[0]), disp(p[1]), disp(p[2]), disp(p[2])];
        out.push(ScreenPrim::Flat(FlatQuad {
            xy,
            color: c(0),
            gouraud: Some([c(0), c(1), c(2), c(2)]),
            semi_transparent: t.semi,
            abr_mode: 0,
            ot_index: depth_ot(z),
            depth: None,
        }));
    }
}

/// The three reel cylinders (`FUN_801d0fa8`): eight faces each, the payline
/// face fourth from the top.
fn reel_prims(input: &SlotCabinetInput<'_>, out: &mut Vec<ScreenPrim>) {
    let len = sc::STRIP_LEN;
    for r in 0..sc::REEL_COUNT {
        let strip = input.strips[r];
        if strip.len() < len as usize {
            continue;
        }
        let pos = input.reel_pos[r];
        // `sra 8` after a `+0xFF` bias for negatives: division toward zero.
        let row0 = pos / 0x100;
        let frac = pos - row0 * 0x100;
        let x0 = sc::reel_x(r);
        let x1 = x0 + sc::REEL_WIDTH;
        for f in 0..sc::REEL_FACES as i32 {
            let a_top = (sc::REEL_ANGLE_BASE + frac + f * sc::REEL_ANGLE_STEP) & 0xFFF;
            let a_bot = (a_top + sc::REEL_ANGLE_STEP) & 0xFFF;
            let (y_t, z_t) = (sc::reel_y(a_top), sc::reel_z(a_top));
            let (y_b, z_b) = (sc::reel_y(a_bot), sc::reel_z(a_bot));
            // Face 4 is the payline face; the engine's payline row is the
            // strip index the payout reads, so face `f` shows `row + 4 - f`
            // (retail's own row counter starts four rows on and counts down).
            let row = (row0 + 4 - f).rem_euclid(len) as usize;
            let value = strip[row] as u16;
            let u0 = ((value & 3) << 6) as u8;
            let v0 = ((value << 4) & 0xC0) as u8;
            let (u1, v1) = (u0 + 0x3F, v0 + 0x3F);
            let bonus = value >= 0x10;
            let clut = if bonus {
                SLOT_BONUS_CLUT_BASE
            } else {
                SLOT_SYMBOL_CLUT_BASE
            } + (value & 0xF);
            let tpage = REEL_TPAGE + u16::from(bonus);
            let st = shade_word(sc::reel_shade(z_t));
            let sb = shade_word(sc::reel_shade(z_b));
            out.push(textured(
                [
                    sc::project(x0, y_t, z_t),
                    sc::project(x1, y_t, z_t),
                    sc::project(x0, y_b, z_b),
                    sc::project(x1, y_b, z_b),
                ],
                [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
                clut,
                tpage,
                st,
                Some([st, st, sb, sb]),
                false,
                depth_ot((z_t + z_b) as f32 * 0.5),
            ));
        }
    }
}

/// One `FUN_800195a8` billboard: a view-space rect around the projected
/// centre, `v0..v3` = top-left, top-right, bottom-left, bottom-right.
fn billboard(
    pos: sc::Pos3,
    half: (i32, i32),
    cell: (u8, u8, u8, u8),
    clut: u16,
    tpage: u16,
    color: u32,
) -> ScreenPrim {
    let (cx, cy) = sc::project(pos.x as i32, pos.y as i32, pos.z as i32);
    let (hw, hh) = sc::billboard_half(half.0, half.1, pos.z as i32);
    let (u0, v0, w, h) = cell;
    let (u1, v1) = (u0.wrapping_add(w), v0.wrapping_add(h));
    textured(
        [
            (cx - hw, cy - hh),
            (cx + hw, cy - hh),
            (cx - hw, cy + hh),
            (cx + hw, cy + hh),
        ],
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        clut,
        tpage,
        color,
        None,
        false,
        depth_ot(pos.z as f32),
    )
}

/// The glass furniture (`FUN_801d08e4`), in its four passes.
fn furniture_prims(input: &SlotCabinetInput<'_>, out: &mut Vec<ScreenPrim>) {
    // Pass 1: the reel-stop pedestals. A taken stop swaps the palette and
    // slides the cell left on its own row.
    for r in 0..sc::REEL_COUNT {
        let pos = sc::Pos3 {
            x: (sc::PEDESTAL_X0 + r as i32 * sc::PEDESTAL_X_STEP) as i16,
            y: sc::PEDESTAL_Y as i16,
            z: sc::GLASS_Z as i16,
        };
        let stopped = input.stopped[r];
        let clut = if stopped {
            sc::PEDESTAL_CLUT_STOPPED
        } else {
            sc::PEDESTAL_CLUT_SPINNING
        } + r as u16;
        out.push(billboard(
            pos,
            sc::PEDESTAL_HALF,
            sc::pedestal_cell(r, stopped),
            clut,
            FURNITURE_TPAGE,
            NEUTRAL,
        ));
    }
    // Any stop taken brightens the medallions and the marquee (`0xA0`); the
    // record whose index is the winning-line word brightens further (`0xE0`).
    // The marquee pass compares its own index against the same word.
    let any_stopped = input.stopped.iter().any(|&s| s);
    let tint = |i: usize| {
        if i as i32 == input.winning_line {
            0x00E0_E0E0
        } else if any_stopped {
            0x00A0_A0A0
        } else {
            NEUTRAL
        }
    };
    // Pass 2: the payline medallions down the left.
    for (i, m) in input.scene.medallions.iter().enumerate() {
        out.push(billboard(
            m.pos,
            sc::MEDALLION_HALF,
            sc::MEDALLION_CELL,
            sc::medallion_clut(m.art).0,
            MEDALLION_TPAGE,
            tint(i),
        ));
    }
    // Pass 3: the marquee panel and the two mascots.
    for (i, m) in input.scene.marquee.iter().enumerate() {
        out.push(billboard(
            m.pos,
            (m.half_w as i32, m.half_h as i32),
            (m.u, m.v, m.w, m.h),
            sc::MARQUEE_CLUT_BASE.wrapping_add(m.clut_off as u16),
            FURNITURE_TPAGE,
            tint(i),
        ));
    }
    // Pass 4: the payline lamps down the right; the winning line's is lit.
    for (i, m) in input.scene.lamps.iter().enumerate() {
        let lit = i as i32 == input.winning_line;
        out.push(billboard(
            m.pos,
            sc::LAMP_HALF,
            if lit {
                sc::LAMP_CELL_LIT
            } else {
                sc::LAMP_CELL_UNLIT
            },
            sc::LAMP_CLUT,
            FURNITURE_TPAGE,
            NEUTRAL,
        ));
    }
}

/// The 78x13 dot matrix (`FUN_801d0e1c`): every dot is drawn, an unlit one
/// sampling the swatch at `u = 0`.
fn dot_prims(input: &SlotCabinetInput<'_>, out: &mut Vec<ScreenPrim>) {
    let clut = DOTS_CLUT_ROW + (input.blink & 1) as u16;
    let size = sc::DOT_SIZE as f32;
    for row in 0..sc::DOT_ROWS {
        for col in 0..sc::DOT_COLS {
            let nib = input
                .dots
                .get(col * sc::DOT_STRIDE + row)
                .copied()
                .unwrap_or(0);
            let u = (nib as u32 * sc::DOT_U_PER_NIBBLE) as u8;
            let (x, y) = sc::project(
                sc::DOT_X0 + col as i32 * sc::DOT_X_STEP,
                sc::DOT_Y0 + row as i32 * sc::DOT_Y_STEP,
                sc::DOT_Z,
            );
            let (x, y) = (x.trunc(), y.trunc());
            let s = sc::DOT_SIZE as u8;
            out.push(textured(
                [(x, y), (x + size, y), (x, y + size), (x + size, y + size)],
                [(u, 0), (u + s, 0), (u, s), (u + s, s)],
                clut,
                DOTS_TPAGE,
                NEUTRAL,
                None,
                false,
                DOTS_OT,
            ));
        }
    }
}

/// One `FUN_801d2cc0` widget, centred on `(x, y)` (the anchor mode `0` both
/// of `FUN_801cfff0`'s calls use) at brightness `0x80` and unit scale.
fn hud_widget(w: &SlotHudWidget, x: i32, y: i32, ot: u32) -> ScreenPrim {
    let scale = |c: u8| (c as u32 * 0x80) >> 8;
    let word = |c: [u8; 3]| (scale(c[0]) << 16) | (scale(c[1]) << 8) | scale(c[2]);
    let (top, bot) = (word(w.rgb_top), word(w.rgb_bottom));
    // `(base * scale) >> 12` twice, each with retail's toward-zero bias, then
    // halved toward zero for the centred anchor.
    let ww = ((w.w as i32 * w.scale) >> 12) / 2;
    let hh = ((w.h as i32 * w.scale) >> 12) / 2;
    let (x0, x1, y0, y1) = (
        (x - ww) as f32,
        (x + ww) as f32,
        (y - hh) as f32,
        (y + hh) as f32,
    );
    let (u0, v0) = (w.u, w.v);
    let (u1, v1) = (u0.wrapping_add(w.w), v0.wrapping_add(w.h));
    textured(
        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        w.clut.0,
        w.texpage.0 + ((w.abr as u16) << 5),
        top,
        Some([top, top, bot, bot]),
        w.semi,
        ot,
    )
}

/// `FUN_801d2914(value, 5, 0x222, 0xA8)`: five 16x16 digits right to left from
/// `x + 4 * 10`, leading zeros drawn.
fn coin_digit_prims(balance: i32, out: &mut Vec<ScreenPrim>) {
    let mut n = balance.clamp(0, 99_999);
    let (count, x, y) = (5i32, 0x222i32, 0xA8i32);
    let mut dx = x + (count - 1) * 10;
    for _ in 0..count {
        let d = (n % 10) as u8;
        n /= 10;
        let u0 = 0x40 + d * 0x10;
        let (x0, y0) = (dx as f32, y as f32);
        out.push(textured(
            [
                (x0, y0),
                (x0 + 16.0, y0),
                (x0, y0 + 16.0),
                (x0 + 16.0, y0 + 16.0),
            ],
            [(u0, 0xC0), (u0 + 0x10, 0xC0), (u0, 0xD0), (u0 + 0x10, 0xD0)],
            SLOT_DIGIT_CLUT,
            REEL_TPAGE,
            NEUTRAL,
            None,
            true,
            DIGITS_OT,
        ));
        dx -= 0x10;
    }
}

/// The whole machine, as screen primitives, for one frame.
pub fn slot_cabinet_prims(input: &SlotCabinetInput<'_>) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    if let Some(mesh) = input.cabinet {
        cabinet_prims(mesh, &mut out);
    }
    reel_prims(input, &mut out);
    furniture_prims(input, &mut out);
    dot_prims(input, &mut out);
    // `FUN_801cfff0`'s HUD: the "COIN" label (widget 1) at (560, 160), the
    // readout, then the info panel (widget 0) at (560, 128).
    if let Some(w) = input.hud.get(1) {
        out.push(hud_widget(w, 0x230, 0xA0, LABEL_OT));
    }
    coin_digit_prims(input.balance, &mut out);
    if let Some(w) = input.hud.first() {
        out.push(hud_widget(w, 0x230, 0x80, PANEL_OT));
    }
    out
}

/// The marquee's two free-running presentation counters and the legend's
/// last message, owned by whichever host draws the machine.
///
/// `counter` is `DAT_801d3ca0` (the legend scroll), `blink` is `DAT_801d3c9c`
/// (the dot palette swap); the reel renderer's tail advances both once a
/// frame. `legend_msg` is `DAT_801d3c88`, the id the last scroll call drew.
#[derive(Debug, Clone, Default)]
pub struct SlotMarqueeClock {
    pub counter: i32,
    pub blink: u32,
    pub legend_msg: Option<usize>,
}

impl SlotMarqueeClock {
    /// Advance one frame, then compose this frame's dot buffer: the payout
    /// caption or the tally strip when the machine's state puts one up
    /// ([`sc::compose_marquee_frame`]), else the attract legend
    /// ([`sc::attract_legend`]). The bonus-anticipation latch `DAT_801d3ca4`
    /// is the caller's (`anticipation`, `0` when it is not modelled).
    pub fn frame(
        &mut self,
        marquee: &sc::MarqueeFrame,
        anticipation: i32,
        messages: &[sc::MarqueeMessage],
    ) -> Vec<u8> {
        self.counter = self.counter.wrapping_add(1);
        self.blink = self.blink.wrapping_add(1);
        let placements = sc::compose_marquee_frame(marquee);
        let caption = marquee.payout != 0 && marquee.payout_frame != 0;
        if caption || sc::MARQUEE_TALLY_MODES.contains(&marquee.feature_mode) {
            return sc::render_marquee(&placements, messages);
        }
        match sc::attract_legend(marquee.feature_mode, anticipation, self.counter) {
            Some((msg, x)) => {
                let x = if self.legend_msg != Some(msg) {
                    // `FUN_801d069c`: a new id resets the counter and scrolls
                    // from column 0 this call.
                    self.legend_msg = Some(msg);
                    self.counter = sc::LEGEND_COUNTER_RESET;
                    0
                } else {
                    x
                };
                messages
                    .get(msg)
                    .map(|m| sc::compose_marquee(m, x, 0))
                    .unwrap_or_else(sc::clear_dots)
            }
            None => sc::clear_dots(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> sc::SlotScene {
        sc::SlotScene {
            paylines: Vec::new(),
            medallions: Vec::new(),
            lamps: Vec::new(),
            marquee: Vec::new(),
            messages: Vec::new(),
        }
    }

    #[test]
    fn the_payline_face_shows_the_row_the_payout_reads() {
        let sc_ = scene();
        let strip: Vec<u8> = (0..sc::STRIP_LEN as u8).collect();
        let dots = sc::clear_dots();
        let input = SlotCabinetInput {
            scene: &sc_,
            cabinet: None,
            hud: &[],
            reel_pos: [7 * 0x100, 0, 0],
            strips: [&strip, &strip, &strip],
            stopped: [false; 3],
            winning_line: -1,
            dots: &dots,
            blink: 0,
            balance: 0,
        };
        let prims = slot_cabinet_prims(&input);
        // Reel 0's fifth face (index 4) carries strip value 7: u = (7&3)<<6,
        // v = (7<<4)&0xC0, CLUT 0x7A80 + 7.
        let ScreenPrim::Textured(q) = prims[4] else {
            panic!("reel face");
        };
        assert_eq!(q.clut, SLOT_SYMBOL_CLUT_BASE + 7);
        assert_eq!(q.uv[0], (0xC0, 0x40));
        // The payline face is the one nearest the viewer: full shade.
        let g = q.gouraud.unwrap();
        assert!(g[0] & 0xFF >= 0xA0 && g[3] & 0xFF >= 0xA0, "{g:x?}");
    }

    #[test]
    fn a_bonus_value_rebases_page_and_palette() {
        let sc_ = scene();
        let strip = vec![0x13u8; sc::STRIP_LEN as usize];
        let dots = sc::clear_dots();
        let input = SlotCabinetInput {
            scene: &sc_,
            cabinet: None,
            hud: &[],
            reel_pos: [0; 3],
            strips: [&strip, &strip, &strip],
            stopped: [false; 3],
            winning_line: -1,
            dots: &dots,
            blink: 0,
            balance: 0,
        };
        let ScreenPrim::Textured(q) = slot_cabinet_prims(&input)[0] else {
            panic!("reel face");
        };
        assert_eq!(q.tpage, 0x0D);
        assert_eq!(q.clut, SLOT_BONUS_CLUT_BASE + 3);
    }

    #[test]
    fn a_new_legend_message_restarts_its_scroll() {
        let mut clock = SlotMarqueeClock::default();
        let msgs: Vec<sc::MarqueeMessage> = (0..6)
            .map(|_| sc::MarqueeMessage {
                u: 0,
                v: 0,
                w: 200,
                h: 13,
                bitmap: (0..200 * 13).map(|i| (i % 200) as u8 % 15 + 1).collect(),
            })
            .collect();
        let mut f = sc::MarqueeFrame {
            feature_mode: 1,
            reel_state: 1,
            ..Default::default()
        };
        clock.frame(&f, 0, &msgs);
        assert_eq!(clock.legend_msg, Some(1));
        assert_eq!(clock.counter, sc::LEGEND_COUNTER_RESET);
        let buf = clock.frame(&f, 0, &msgs);
        // counter 101: 101 % 168 - 84 = 17 -> source column 17 at matrix col 0.
        assert_eq!(buf[0], msgs[1].bitmap[17]);
        f.feature_mode = 5;
        assert!(clock.frame(&f, 0, &msgs).iter().all(|&b| b == 0));
    }

    #[test]
    fn coin_digits_run_right_to_left_with_leading_zeros() {
        let mut out = Vec::new();
        coin_digit_prims(70, &mut out);
        assert_eq!(out.len(), 5);
        let u_of = |p: &ScreenPrim| match p {
            ScreenPrim::Textured(q) => q.uv[0].0,
            _ => unreachable!(),
        };
        // Units first, at x = 0x222 + 40 = 586 -> display 293.
        assert_eq!(u_of(&out[0]), 0x40);
        assert_eq!(u_of(&out[1]), 0x40 + 7 * 0x10);
        assert_eq!(u_of(&out[4]), 0x40);
        let ScreenPrim::Textured(q) = out[0] else {
            unreachable!()
        };
        assert_eq!(q.xy[0], (293, 0xA8));
    }
}
