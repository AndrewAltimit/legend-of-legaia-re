//! The Muscle Dome hub's **ringside still**: the two `POLY_FT4` quads the
//! contest hub draws a re-entered hub's backdrop with, and the 16bpp sheet
//! they sample.
//!
//! Two retail routines meet here, one on each side of VRAM:
//!
//! * the upload - `FUN_801F6B24` in the PROT `0978` `field_back_read` image
//!   (slot-B base `0x801F69D8`) streams one of the two headerless stills
//!   (extraction `1221` / `1222`, `docs/formats/ringside-still.md`) into VRAM
//!   `(384, 0)` as four `320 x 64` `LoadImage` bands;
//! * the draw - `FUN_801D00F8` in the contest hub (PROT `0977`, slot-A base
//!   `0x801CE818`), whose still arm writes two `POLY_FT4` packets addressing
//!   that region through 16-bit texture pages `0x106` and `0x109`.
//!
//! [`still_sheet_rgba`] is the upload: it lays the entry's four bands down at
//! the rectangles the loader's own rect stores name
//! ([`legaia_engine_vm::panel_backread_loader::backread_slice_rect`]),
//! relative to the region's origin, so the sheet's pixel `(x, y)` is VRAM
//! `(384 + x, y)`. [`ringside_still_quads`] is the draw, and
//! [`StillDraw::from_quad`] turns a quad's texture-page address back into a
//! rectangle of that sheet - which is the one step that has to know the
//! still's VRAM origin, and so the one place the two halves are joined.
//!
//! Neither host samples a software VRAM for the hub: both draw the hub's
//! other screens from per-page sheets (the native window's sprite atlas, the
//! play page's canvas sheets), and the still rides the same path as one
//! more sheet. What makes it the still and not an arbitrary picture is that
//! the quads name it by texture page, exactly as retail's do.

use crate::other_game_hud::{HudQuad, HudSprite};
use legaia_asset::ringside_still as still;
use legaia_engine_vm::panel_backread_loader::{BACKREAD_RECT_X, backread_slice_rect};

/// GP0 command byte of the two packets: `0x2C`, a flat-shaded, textured,
/// opaque, modulated four-point polygon (`POLY_FT4`).
pub const GP0_POLY_FT4: u8 = 0x2C;

/// The two texture-page words, left quad then right quad (`li v0,0x106` at
/// `0x801D01F4`, `li v0,0x109` at `0x801D0264`). `0x100` is `tp = 2` (15-bit
/// direct colour); the low nibble is the page's VRAM x in 64-halfword units,
/// so the pages open at x = 384 and x = 576.
pub const STILL_TPAGES: [u16; 2] = [0x106, 0x109];

/// Screen y of both quads' top edge (`li s6,-0x14` at `0x801D01C0`).
pub const STILL_SCREEN_Y0: i16 = -0x14;
/// Screen y of both quads' bottom edge (`li s2,0xdc` at `0x801D01D0`).
pub const STILL_SCREEN_Y1: i16 = 0xDC;
/// Screen x where the left quad ends and the right one starts
/// (`li s1,0xc0` at `0x801D01F8`).
pub const STILL_SPLIT_X: i16 = 0xC0;
/// Screen x of the right quad's right edge (`li v0,0x140` at `0x801D026C`).
pub const STILL_SCREEN_X1: i16 = 0x140;
/// Texel v of both quads' bottom edge (`li s3,0xf0` at `0x801D01D8`).
pub const STILL_V1: u8 = 0xF0;
/// Texel u of the right quad's right edge (`li v0,0x80` at `0x801D0278`).
pub const STILL_RIGHT_U1: u8 = 0x80;

/// Clamp the routine's `a0` the way its prologue does: `slti v0,s1,0x100`
/// then `bgez s1` (`0x801D0104..0x801D0134`), so `0..=0xFF`.
fn clamp_level(level: i32) -> u8 {
    level.clamp(0, 0xFF) as u8
}

/// The still arm of the hub backdrop emitter: two `POLY_FT4` quads, one
/// 320x240 image split down the middle at x = 192, every vertex coloured
/// with the caller's fade level.
///
/// `level` is the hub's backdrop counter `*(0x801D1A7C)` - a fade level, not
/// a selector - which the routine clamps to `0..=0xFF` and broadcasts into
/// all three colour lanes of each packet's `code + rgb` word. Retail skips
/// the call when it is zero (`beqz a0` at `0x801D00A4`); a host does the same
/// by drawing nothing when [`ringside_still_quads`] would be black.
///
/// The CLUT halfword of each packet is not written - a 15-bit page reads no
/// palette - so the returned quads carry `clut = 0`.
///
/// PORT: FUN_801d00f8 (the `_DAT_801D1AE0 != 0` arm, `0x801D01BC..0x801D02C4`)
pub fn ringside_still_quads(level: i32) -> [HudQuad; 2] {
    let c = clamp_level(level);
    let rgb = [[c; 3]; 4];
    let (y0, y1) = (STILL_SCREEN_Y0, STILL_SCREEN_Y1);
    let left = HudQuad {
        xy: [(0, y0), (STILL_SPLIT_X, y0), (0, y1), (STILL_SPLIT_X, y1)],
        uv: [
            (0, 0),
            (STILL_SPLIT_X as u8, 0),
            (0, STILL_V1),
            (STILL_SPLIT_X as u8, STILL_V1),
        ],
        rgb,
        tpage: STILL_TPAGES[0],
        clut: 0,
        semi_transparent: false,
    };
    let right = HudQuad {
        xy: [
            (STILL_SPLIT_X, y0),
            (STILL_SCREEN_X1, y0),
            (STILL_SPLIT_X, y1),
            (STILL_SCREEN_X1, y1),
        ],
        uv: [
            (0, 0),
            (STILL_RIGHT_U1, 0),
            (0, STILL_V1),
            (STILL_RIGHT_U1, STILL_V1),
        ],
        rgb,
        tpage: STILL_TPAGES[1],
        clut: 0,
        semi_transparent: false,
    };
    [left, right]
}

/// Sprite-table record the first-visit backdrop tiles (`li a2,0x2` at
/// `0x801D0164`) - the brick wall behind the title card.
pub const FIRST_VISIT_TILE_RECORD: i32 = 2;

/// The other arm of the same emitter - the one a **first** visit takes, with
/// the re-entry latch `_DAT_801D1AE0` still zero: a `3 x 2` grid of the
/// sprite-table record [`FIRST_VISIT_TILE_RECORD`] through the corner-anchored
/// emitter `FUN_801D08EC`, top-left `(i << 7, j << 7)`, at scale `0x1000` and
/// the caller's fade level, rows outermost (`0x801D0148..0x801D0194`).
///
/// The draws carry no brightness: [`crate::other_game_hud::hub_screen_quads`]
/// takes the level, which is the same `*(0x801D1A7C)` the still arm reads.
/// The arm's last packet, the full-screen shade, is [`first_visit_shade`].
///
/// PORT: FUN_801d00f8 (the `_DAT_801D1AE0 == 0` arm's tile grid,
/// `0x801D0148..0x801D0194`)
pub fn first_visit_tile_draws() -> Vec<crate::other_game_hud::HubDraw> {
    use crate::other_game_hud::{HubAnchor, HubDraw};
    let mut out = Vec::with_capacity(6);
    for j in 0..2i16 {
        for i in 0..3i16 {
            out.push(HubDraw {
                sel: FIRST_VISIT_TILE_RECORD,
                x: i << 7,
                y: j << 7,
                anchor: HubAnchor::Corner,
                scale: 0x1000,
                call_site: 0x801D_0170,
            });
        }
    }
    out
}

/// GP0 command byte of the backdrop shade: `0x3A`, a Gouraud-shaded,
/// untextured, semi-transparent four-point polygon (`lui t0,0x3a10` at
/// `0x801D1624`).
pub const GP0_POLY_G4_SEMI: u8 = 0x3A;

/// The shade's top-corner colour (`li v0,0x64` at `0x801D1654`, stored in
/// all three lanes of vertices 0 and 1); the bottom corners are `0`.
pub const SHADE_TOP: u8 = 0x64;

/// The tpage word of the draw-mode packet the shade appends
/// (`li a3,0x46` at `0x801D16CC`): ABR bits `(0x46 >> 5) & 3 = 2`, the
/// subtractive `B - F` equation.
pub const SHADE_DRAW_MODE_TPAGE: u16 = 0x46;

/// Ordering-table slot the first visit links its shade at (`li v0,0x384`
/// at `0x801D0198`), in front of the wall tiles at `0x3E8`.
pub const SHADE_OT: u32 = 0x384;

/// Ordering-table slot the first visit's wall tiles link at (`li s3,0x3e8`,
/// stored to `DAT_801D1AA8` before each tile at `0x801D016C`).
pub const TILE_OT: u32 = 0x3E8;

/// One `FUN_801D1610` packet: an untextured Gouraud quad over
/// `(x, y, w, h)`, top corners [`SHADE_TOP`], bottom corners black, drawn
/// subtractively. Its colour does not follow the backdrop level: whenever
/// the backdrop runs at all, the shade darkens the top of the frame by the
/// full `0x64` and nothing at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackdropShade {
    /// Vertices in retail `v0..v3` order.
    pub xy: [(i16, i16); 4],
    /// Per-vertex colour.
    pub rgb: [[u8; 3]; 4],
    /// ABR equation of the appended draw-mode packet (`2`, `B - F`).
    pub abr: u8,
    /// Ordering-table slot.
    pub ot: u32,
}

/// Build the shade packet over `(x, y, w, h)` at OT slot `ot`.
///
/// PORT: FUN_801d1610 (PROT 0977; the first-visit arm of `FUN_801D00F8`
/// calls it once, `0x801D01AC`)
pub fn backdrop_shade(x: i16, y: i16, w: i16, h: i16, ot: u32) -> BackdropShade {
    let (x1, y1) = (x.wrapping_add(w), y.wrapping_add(h));
    let top = [SHADE_TOP; 3];
    BackdropShade {
        xy: [(x, y), (x1, y), (x, y1), (x1, y1)],
        rgb: [top, top, [0; 3], [0; 3]],
        abr: ((SHADE_DRAW_MODE_TPAGE >> 5) & 3) as u8,
        ot,
    }
}

/// The first visit's shade: `FUN_801D1610(0, 0, 0x140, 0xF0)` at
/// [`SHADE_OT`] (`0x801D0198..0x801D01B0`).
pub fn first_visit_shade() -> BackdropShade {
    backdrop_shade(0, 0, 0x140, 0xF0, SHADE_OT)
}

/// The levels the first visit's hub draws a frame at - one per screen, or
/// `None` for a screen that arm does not draw. The engine's
/// `muscle_ringside::FirstVisitHub::frame` produces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FirstVisitLevels {
    /// The wall's level `*(0x801D1A7C)`; `0` draws no backdrop at all.
    pub backdrop: i32,
    /// The intro strip's level.
    pub intro: Option<i32>,
    /// The title art's face scale.
    pub title_scale: Option<i32>,
    /// The course card's level.
    pub course_card: Option<i32>,
    /// The ROUND card's level.
    pub round_card: Option<i32>,
}

/// One first-visit hub frame, back to front: the wall tiles, the shade over
/// them, then the hub screens' quads in paint order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FirstVisitDraw {
    /// The `3 x 2` wall tiles at [`TILE_OT`].
    pub tiles: Vec<HudQuad>,
    /// The shade at [`SHADE_OT`], present whenever the tiles are.
    pub shade: Option<BackdropShade>,
    /// Every hub-screen quad at slot `3`, in paint order.
    pub hud: Vec<HudQuad>,
}

/// Compose a first-visit hub frame from the sprite table: the backdrop
/// emitter's latch-`0` arm (tiles + shade) when `levels.backdrop` is
/// non-zero, then the arm's screens in the order its emitter calls run -
/// intro strip; or course card, title face, title shadow; or the ROUND card
/// - reversed into paint order, since they all share one OT slot.
///
/// `course` is `*(0x801D1A90)`, `round` the displayed round number.
pub fn first_visit_hub_draw(
    table: &mut [HudSprite],
    levels: &FirstVisitLevels,
    course: i32,
    round: i32,
) -> FirstVisitDraw {
    use crate::other_game_hud as hud;
    let mut out = FirstVisitDraw::default();
    if levels.backdrop != 0 {
        out.tiles = hud::hub_screen_quads(table, &first_visit_tile_draws(), levels.backdrop);
        out.shade = Some(first_visit_shade());
    }
    // Each call returns its own quads in paint order; flip them back to
    // emit order, concatenate the arm's calls, and flip the whole slot once.
    let emit_order = |mut q: Vec<HudQuad>| {
        q.reverse();
        q
    };
    let mut emit: Vec<HudQuad> = Vec::new();
    if let Some(level) = levels.intro {
        emit.extend(emit_order(hud::hub_screen_quads(
            table,
            hud::HUB_INTRO_CARD,
            level,
        )));
    }
    if let Some(level) = levels.course_card {
        emit.extend(emit_order(hud::hub_screen_quads(
            table,
            &hud::course_card_draws(course),
            level,
        )));
    }
    if let Some(scale) = levels.title_scale {
        // `title_art_quads` repeats retail's per-frame clear of record 4's
        // transparency byte before the face.
        emit.extend(emit_order(hud::title_art_quads(table, scale)));
    }
    if let Some(level) = levels.round_card {
        emit.extend(emit_order(hud::hub_screen_quads(
            table,
            &hud::round_banner_draws(round),
            level,
        )));
    }
    emit.reverse();
    out.hud = emit;
    out
}

/// Whether a hub quad samples a 15-bit direct-colour page (`tp = 2`, tpage
/// bits 7-8) - which on the hub is only ever the still: every sprite-table
/// record names a 4bpp page.
pub fn is_still_quad(q: &HudQuad) -> bool {
    (q.tpage >> 7) & 3 == 2
}

/// One still quad as a host blit: a rectangle of the retail 320x240 frame and
/// the rectangle of the still sheet it samples.
///
/// A PSX polygon excludes its right and bottom edges, so the extents here are
/// the plain vertex differences (192 + 128 = 320 columns, 240 rows), not the
/// inclusive `+ 1` the sprite-table emitters' quads take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StillDraw {
    /// `(x, y, w, h)` in the retail frame.
    pub dst: (i32, i32, u32, u32),
    /// `(x, y, w, h)` in the still sheet ([`still_sheet_rgba`]'s pixels).
    pub src: (u32, u32, u32, u32),
    /// The packet's colour byte - the fade level. `0x80` is a textured
    /// primitive's neutral modulation.
    pub level: u8,
}

impl StillDraw {
    /// Resolve a still quad through its texture page: the page's VRAM x
    /// (`(tpage & 0xF) * 64`) plus the quad's `u`, less the still's VRAM
    /// origin, is the sheet column. `None` for a quad that is not a 15-bit page, or one whose page
    /// lies left of the still.
    pub fn from_quad(q: &HudQuad) -> Option<StillDraw> {
        if !is_still_quad(q) {
            return None;
        }
        let page_x = u32::from(q.tpage & 0xF) * 64;
        let page_y = u32::from((q.tpage >> 4) & 1) * 256;
        let origin_x = u32::from(BACKREAD_RECT_X as u16);
        let sx = (page_x + u32::from(q.uv[0].0)).checked_sub(origin_x)?;
        let sy = (page_y + u32::from(q.uv[0].1)).checked_sub(u32::from(still::VRAM_Y))?;
        let sw = u32::from(q.uv[1].0.saturating_sub(q.uv[0].0));
        let sh = u32::from(q.uv[2].1.saturating_sub(q.uv[0].1));
        let dw = (i32::from(q.xy[1].0) - i32::from(q.xy[0].0)).max(0) as u32;
        let dh = (i32::from(q.xy[2].1) - i32::from(q.xy[0].1)).max(0) as u32;
        Some(StillDraw {
            dst: (i32::from(q.xy[0].0), i32::from(q.xy[0].1), dw, dh),
            src: (sx, sy, sw, sh),
            level: q.rgb[0][0],
        })
    }
}

/// The still as it sits in VRAM after the loader's four uploads, as an RGBA8
/// sheet of [`still::WIDTH`] x [`still::HEIGHT`] pixels whose origin is VRAM
/// `(384, 0)`.
///
/// Each band is placed at the rectangle
/// [`backread_slice_rect`] names for it - `x = 0x180`, `y = n * 0x40`,
/// `0x140 x 0x40` - so the sheet is assembled from the loader's own rect
/// stores rather than from the entry's byte order. Pixels decode through
/// [`still::to_rgba8`]'s channel expansion. A `0x0000` texel, which a PSX
/// textured primitive treats as transparent, stays opaque black here: the
/// still sits at the ordering table's far end, so what shows through such a
/// texel on retail is the frame's clear, which this treats as black - an
/// assumption, since no capture here reads the hub's clear colour.
///
/// `None` unless `entry` is exactly the `0x28000`-byte still shape.
pub fn still_sheet_rgba(entry: &[u8]) -> Option<Vec<u8>> {
    let pixels = still::to_rgba8(entry)?;
    let row_bytes = still::WIDTH * 4;
    let mut out = vec![0u8; still::WIDTH * still::HEIGHT * 4];
    for band in 0..still::BAND_COUNT {
        let (x, y, w, h) = backread_slice_rect(band as u32);
        let (x, y) = (
            (x - BACKREAD_RECT_X) as usize,
            (y as u16 - still::VRAM_Y) as usize,
        );
        let (w, h) = (w as usize, h as usize);
        let src = still::band_span(band)?;
        // The band's own bytes, as pixels, row-major inside the band.
        let band_px = &pixels[src.start * 2..src.end * 2];
        for row in 0..h {
            let dst = (y + row) * row_bytes + x * 4;
            out[dst..dst + w * 4].copy_from_slice(&band_px[row * w * 4..(row + 1) * w * 4]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shade_darkens_the_top_and_leaves_the_bottom() {
        let s = first_visit_shade();
        assert_eq!(s.xy, [(0, 0), (0x140, 0), (0, 0xF0), (0x140, 0xF0)]);
        assert_eq!(s.rgb[0], [0x64; 3]);
        assert_eq!(s.rgb[1], [0x64; 3]);
        assert_eq!(s.rgb[2], [0; 3]);
        assert_eq!(s.abr, 2, "draw-mode tpage 0x46 selects B - F");
        assert_eq!(s.ot, 0x384);
    }

    fn table() -> Vec<HudSprite> {
        (0..17)
            .map(|i| HudSprite {
                size: 0x1000,
                tpage: if (4..=8).contains(&i) { 0x15 } else { 0x5 },
                clut: 0x7DC0,
                u0: 0,
                v0: 0,
                w: 32,
                h: 16,
                rgb_top: [0x80; 3],
                semi_transparent: 0,
                rgb_bottom: [0x80; 3],
                page: 0,
            })
            .collect()
    }

    #[test]
    fn a_first_visit_frame_puts_the_card_shadows_under_their_faces() {
        let mut t = table();
        let levels = FirstVisitLevels {
            backdrop: 0x80,
            course_card: Some(0x80),
            title_scale: Some(0x1000),
            ..Default::default()
        };
        let d = first_visit_hub_draw(&mut t, &levels, 1, 1);
        assert_eq!(d.tiles.len(), 6);
        assert!(d.shade.is_some());
        assert_eq!(
            d.hud.len(),
            8,
            "six course-card draws + title face + shadow"
        );
        // Painted first: the title's subtractive shadow; painted last: the
        // course name's additive face at (8, 0x78).
        assert_eq!(d.hud[0].tpage & 0x60, 0x40, "variant 2 = ABR 2 first");
        assert_eq!(d.hud[7].xy[0], (8, 0x78));
        assert_eq!(d.hud[7].tpage & 0x60, 0x20, "variant 1 = ABR 1 last");
        // The title face sits between and is opaque despite the shadow's
        // write-back into record 4.
        let face = d
            .hud
            .iter()
            .find(|q| q.tpage & 0x60 == 0 && q.tpage & 0x10 != 0)
            .unwrap();
        assert!(!face.semi_transparent);
    }

    #[test]
    fn no_backdrop_level_draws_no_wall_or_shade() {
        let mut t = table();
        let levels = FirstVisitLevels {
            round_card: Some(0x40),
            ..Default::default()
        };
        let d = first_visit_hub_draw(&mut t, &levels, 0, 3);
        assert!(d.tiles.is_empty());
        assert!(d.shade.is_none());
        assert!(!d.hud.is_empty());
    }

    use super::*;

    #[test]
    fn the_first_visit_wall_is_a_three_by_two_grid_of_record_two_rows_outermost() {
        let draws = first_visit_tile_draws();
        let seats: Vec<(i16, i16)> = draws.iter().map(|d| (d.x, d.y)).collect();
        assert_eq!(
            seats,
            [(0, 0), (128, 0), (256, 0), (0, 128), (128, 128), (256, 128)]
        );
        assert!(draws.iter().all(|d| {
            d.sel == FIRST_VISIT_TILE_RECORD
                && d.scale == 0x1000
                && d.anchor == crate::other_game_hud::HubAnchor::Corner
        }));
    }

    #[test]
    fn the_pair_is_one_320x240_image_split_at_192() {
        let [l, r] = ringside_still_quads(0x80);
        let dl = StillDraw::from_quad(&l).unwrap();
        let dr = StillDraw::from_quad(&r).unwrap();
        assert_eq!(dl.dst, (0, -20, 192, 240));
        assert_eq!(dr.dst, (192, -20, 128, 240));
        // Page 6 u 0 is VRAM x 384 = sheet 0; page 9 u 0 is VRAM x 576 =
        // sheet 192, so the two halves abut in the sheet as on screen.
        assert_eq!(dl.src, (0, 0, 192, 240));
        assert_eq!(dr.src, (192, 0, 128, 240));
        assert_eq!(dl.level, 0x80);
    }

    #[test]
    fn the_level_clamps_like_the_prologue() {
        assert_eq!(ringside_still_quads(-5)[0].rgb[0], [0; 3]);
        assert_eq!(ringside_still_quads(0x1234)[1].rgb[3], [0xFF; 3]);
    }

    #[test]
    fn only_a_15_bit_page_is_a_still_quad() {
        let mut q = ringside_still_quads(0x80)[0];
        assert!(is_still_quad(&q));
        q.tpage = 0x0005;
        assert!(!is_still_quad(&q));
        assert!(StillDraw::from_quad(&q).is_none());
    }

    #[test]
    fn the_sheet_is_the_four_bands_stacked() {
        let mut entry = vec![0u8; still::ENTRY_BYTES];
        // Mark the first pixel of each band with a distinct red.
        for band in 0..still::BAND_COUNT {
            let at = still::band_span(band).unwrap().start;
            entry[at..at + 2].copy_from_slice(&((band as u16 + 1) * 4).to_le_bytes());
        }
        let sheet = still_sheet_rgba(&entry).unwrap();
        assert_eq!(sheet.len(), 320 * 256 * 4);
        for band in 0..still::BAND_COUNT {
            let px = band * still::BAND_HEIGHT * 320 * 4;
            let c = (band as u8 + 1) * 4;
            assert_eq!(
                sheet[px],
                c << 3 | c >> 2,
                "band {band} lands at y {}",
                band * 64
            );
        }
        assert!(still_sheet_rgba(&entry[..16]).is_none());
    }
}
