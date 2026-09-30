//! **Which palette does the game actually draw this texture with?**
//!
//! A TIM file carries a CLUT block, and a viewer that decodes the texture
//! "through palette N" reads row N of that block. That is only the right
//! answer when the game samples the texture through the same VRAM cell the
//! file's own CLUT uploads to *and* nothing overwrote that cell since. Three
//! Legaia shapes break it, and each makes a standalone export look wrong:
//!
//! 1. **One sheet, many palettes.** A 4bpp UI sheet is one grid of 4-bit
//!    indices shared by dozens of sprites, and each sprite picks its own
//!    16-colour palette. Viewed through any single palette most of the sheet
//!    looks mis-coloured, because most of it was never meant to be seen in
//!    that palette. The system-UI sheet's sprites name their palette in the
//!    widget-class table (`legaia_asset::ui_widgets`); [`sheet_palette_regions`]
//!    turns that table into a per-rectangle palette map, and
//!    [`composite_rgba`] decodes each rectangle through its own palette.
//! 2. **A palette from another file.** A sprite record's palette byte is a
//!    VRAM address, not an index into "its" TIM. Row 511's strip runs on past
//!    the sheet's own sixteen palettes into a second, CLUT-only TIM; the
//!    element badges take palettes from the `(896.., 498..501)` block.
//! 3. **A palette overwritten at boot.** The retail per-TIM uploader
//!    `FUN_800198E0` flattens every CLUT block into a `w*h x 1` strip (see
//!    [`crate::system_ui_bundle`]) and uploads the boot bundle in pack
//!    order, last write wins. A member whose strip a later member covers
//!    never has its own palette in VRAM at all - the ASCII battle font's
//!    `(0, 510)` strip is covered by the menu-glyph atlas's 256-entry strip,
//!    and the game draws the font through that atlas's sub-palette 13.
//!    [`BootClutVram::clut_fate`] reports this, and
//!    [`BootClutVram::row_palettes`] lists the palettes VRAM really holds on
//!    the row, so a viewer can offer them.
//!
//! Everything here reads the disc (the boot bundle out of `PROT.DAT`, the
//! widget table out of `SCUS_942.54`) - no capture, no hand-kept list.

use legaia_tim::{PixelMode, Tim, Vram};

use crate::system_ui_bundle::SystemUiBundle;
use crate::ui_widgets::{self, WidgetTable};

/// VRAM origin of the system-UI texture page (`ui_widgets::SHEET_TPAGE`).
pub const SHEET_PAGE_ORIGIN: (u16, u16) = (896, 256);

/// Where a TIM's own CLUT ends up after the boot upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClutFate {
    /// The TIM is not a member of the boot-resident system-UI bundle; its
    /// CLUT goes to VRAM whenever its scene / mode loads it, which a static
    /// read cannot tell.
    NotBootResident,
    /// Boot-resident, and every one of its palettes is in VRAM byte-for-byte.
    Survives,
    /// Boot-resident, but a later upload covers (part of) its CLUT strip.
    /// `palettes` lists the palette indices whose 16 (or 256) entries differ
    /// from what VRAM holds after boot; `by_offset` is the `PROT.DAT` byte
    /// offset of the bundle member whose strip overwrote them, when one did.
    Overwritten {
        palettes: Vec<usize>,
        by_offset: Option<u64>,
    },
}

/// A palette as VRAM holds it: its CLUT cell and its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VramPalette {
    pub fb_x: u16,
    pub fb_y: u16,
    pub entries: Vec<u16>,
}

/// The boot-resident CLUT state: VRAM after the system-UI bundle upload.
pub struct BootClutVram {
    vram: Vram,
    /// `(PROT.DAT offset, flat CLUT strip rect)` per bundle member, upload order.
    strips: Vec<(u64, (u16, u16, u16, u16))>,
}

impl BootClutVram {
    /// Build from a whole in-memory `PROT.DAT`.
    pub fn from_prot_dat(prot: &[u8]) -> Option<Self> {
        let bundle = crate::system_ui_bundle::parse_from_prot_dat_bytes(prot).ok()?;
        let entry_starts = raw_entry_starts(prot)?;
        Some(Self::from_bundle(&bundle, entry_starts))
    }

    /// Build from a parsed bundle. `entry_starts` is the `PROT.DAT` byte
    /// offset of raw TOC entries 0 and 1 (members' `entry_offset` is
    /// relative to them).
    pub fn from_bundle(bundle: &SystemUiBundle, entry_starts: [u64; 2]) -> Self {
        let mut vram = Vram::new();
        bundle.upload_to_vram(&mut vram);
        let strips = bundle
            .tims
            .iter()
            .filter_map(|m| {
                let rect = m.clut_strip_rect()?;
                let abs = entry_starts[m.raw_entry as usize] + m.entry_offset as u64;
                Some((abs, rect))
            })
            .collect();
        Self { vram, strips }
    }

    /// The VRAM this context holds.
    pub fn vram(&self) -> &Vram {
        &self.vram
    }

    /// Is the TIM at `PROT.DAT` offset `abs_offset` a boot-bundle member?
    pub fn is_boot_member(&self, abs_offset: u64) -> bool {
        self.strips.iter().any(|(o, _)| *o == abs_offset)
    }

    /// Where the CLUT of the TIM at `abs_offset` ends up after boot.
    pub fn clut_fate(&self, abs_offset: u64, tim: &Tim) -> ClutFate {
        let Some(idx) = self.strips.iter().position(|(o, _)| *o == abs_offset) else {
            return ClutFate::NotBootResident;
        };
        let Some(clut) = tim.clut.as_ref() else {
            return ClutFate::Survives;
        };
        let per = entries_per_palette(tim.mode);
        if per == 0 {
            return ClutFate::Survives;
        }
        let (sx, sy, _, _) = self.strips[idx].1;
        let mut differing = Vec::new();
        for p in 0..clut.n_palettes(tim.mode) {
            let Some(own) = clut.palette(tim.mode, p) else {
                continue;
            };
            let base = sx as usize + p * per;
            let same = own
                .iter()
                .enumerate()
                .all(|(i, &c)| self.vram.pixel(base + i, sy as usize) == c);
            if !same {
                differing.push(p);
            }
        }
        if differing.is_empty() {
            return ClutFate::Survives;
        }
        // The overwriter is the LAST later member whose strip covers the
        // first differing palette's cell.
        let first_x = sx as usize + differing[0] * per;
        let by_offset = self.strips[idx + 1..]
            .iter()
            .rev()
            .find(|(_, (x, y, w, _))| {
                *y == sy && (*x as usize) <= first_x && first_x < *x as usize + *w as usize
            })
            .map(|(o, _)| *o);
        ClutFate::Overwritten {
            palettes: differing,
            by_offset,
        }
    }

    /// Every palette cell VRAM holds on row `fb_y` after boot, at `per`
    /// entries each (16 for 4bpp, 256 for 8bpp), left to right. A cell is
    /// listed when some boot upload wrote it - an all-zero cell nobody
    /// uploaded is not a palette.
    pub fn row_palettes(&self, fb_y: u16, per: usize) -> Vec<VramPalette> {
        if per == 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut x = 0usize;
        while x + per <= 1024 {
            if self.vram.is_written(x, fb_y as usize) {
                out.push(self.palette_at(x as u16, fb_y, per));
            }
            x += per;
        }
        out
    }

    /// The `per` entries at VRAM `(fb_x, fb_y)`.
    pub fn palette_at(&self, fb_x: u16, fb_y: u16, per: usize) -> VramPalette {
        VramPalette {
            fb_x,
            fb_y,
            entries: (0..per)
                .map(|i| self.vram.pixel(fb_x as usize + i, fb_y as usize))
                .collect(),
        }
    }
}

/// `PROT.DAT` byte offsets of raw TOC entries 0 and 1.
fn raw_entry_starts(prot: &[u8]) -> Option<[u64; 2]> {
    let w = |i: usize| -> Option<u64> {
        let o = 8 + i * 4;
        Some(u32::from_le_bytes(prot.get(o..o + 4)?.try_into().ok()?) as u64 * 0x800)
    };
    Some([w(0)?, w(1)?])
}

fn entries_per_palette(mode: PixelMode) -> usize {
    match mode {
        PixelMode::Bpp4 => 16,
        PixelMode::Bpp8 => 256,
        _ => 0,
    }
}

/// Indices of the TIM's palettes that are **all `0x0000`** on disc - rows
/// that carry no colour at all (every texel through them decodes fully
/// transparent). Scene texture TIMs pad the row-479 band this way; the
/// colours those cells show in game come from another TIM of the same
/// scene (see `docs/formats/npc-palette.md`).
pub fn empty_palettes(tim: &Tim) -> Vec<usize> {
    let Some(clut) = tim.clut.as_ref() else {
        return Vec::new();
    };
    (0..clut.n_palettes(tim.mode))
        .filter(|&p| {
            clut.palette(tim.mode, p)
                .is_some_and(|pal| pal.iter().all(|&c| c == 0))
        })
        .collect()
}

/// How many of palette `idx`'s entries carry the STP (semi-transparency)
/// bit, and how many are the `0x0000` transparent key. STP entries draw
/// blended when the primitive enables semi-transparency; a PNG shows them
/// opaque.
pub fn palette_flag_counts(tim: &Tim, idx: usize) -> (usize, usize) {
    let Some(pal) = tim.clut.as_ref().and_then(|c| c.palette(tim.mode, idx)) else {
        return (0, 0);
    };
    let stp = pal.iter().filter(|&&c| c & 0x8000 != 0).count();
    let transparent = pal.iter().filter(|&&c| c == 0).count();
    (stp, transparent)
}

/// One rectangle of the system-UI texture page and the CLUT cell the game
/// draws it through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRegion {
    /// `(u, v, w, h)` on the page (texel coordinates from
    /// [`SHEET_PAGE_ORIGIN`]).
    pub rect: (u16, u16, u16, u16),
    /// VRAM cell of its 16-entry CLUT.
    pub clut_fb: (u16, u16),
    /// Widget record ids that name this rectangle with this palette.
    pub widget_ids: Vec<u8>,
}

/// Every `(rectangle, palette)` pair the widget-class table draws off the
/// system-UI page: each record's own sprite rect (`FUN_8002C488`), plus the
/// frame tile-set quads of class-0 records and the cap pair of class-3 plate
/// records (`FUN_8002C69C`'s arms), each through the record's palette byte.
/// The portrait records sample another page and are left out. Duplicate
/// `(rect, clut)` pairs merge; the same rectangle in two palettes stays as
/// two regions.
pub fn sheet_palette_regions(table: &WidgetTable) -> Vec<SheetRegion> {
    let mut out: Vec<SheetRegion> = Vec::new();
    let mut push = |rect: (u16, u16, u16, u16), clut_fb: (u16, u16), id: u8| {
        if rect.2 == 0 || rect.3 == 0 {
            return;
        }
        if let Some(r) = out
            .iter_mut()
            .find(|r| r.rect == rect && r.clut_fb == clut_fb)
        {
            if !r.widget_ids.contains(&id) {
                r.widget_ids.push(id);
            }
        } else {
            out.push(SheetRegion {
                rect,
                clut_fb,
                widget_ids: vec![id],
            });
        }
    };
    for (i, w) in table.records().iter().enumerate() {
        let id = i as u8;
        if (ui_widgets::SPRITE_PORTRAIT_FIRST..ui_widgets::SPRITE_PORTRAIT_FIRST + 3).contains(&id)
            || id == ui_widgets::SPRITE_PORTRAIT_FRAME
        {
            continue;
        }
        let clut = w.clut_fb();
        let (u, v, ww, hh) = w.rect;
        push((u as u16, v as u16, ww as u16, hh as u16), clut, id);
        match w.class {
            0 => {
                if let Some(quads) = table.tileset(w.tileset) {
                    for q in quads {
                        push((q.u as u16, q.v as u16, q.w as u16, q.h as u16), clut, id);
                    }
                }
            }
            3 => {
                if let Some((l, r)) = table.plate_caps(w.tileset) {
                    for q in [l, r] {
                        push((q.u as u16, q.v as u16, q.w as u16, q.h as u16), clut, id);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Result of [`composite_rgba`].
pub struct Composite {
    /// Row-major RGBA8, the TIM's own dimensions.
    pub rgba: Vec<u8>,
    /// Texels some region decoded through its game palette.
    pub covered: usize,
    /// Texels two regions claim with **different** palettes (the first
    /// region in table order wins; the same art is drawn in several colours).
    pub contested: usize,
}

/// Decode a 4bpp TIM that sits on a texture page as the game draws it:
/// every texel a region covers goes through that region's palette (read
/// from `vram`), and every other texel through `fallback` (typically the
/// TIM's own palette 0) with its alpha halved so uncovered art reads as
/// "not placed by the table" at a glance. `page_origin` is the VRAM
/// origin the regions' `(u, v)` are relative to. `None` for a non-4bpp TIM.
pub fn composite_rgba(
    tim: &Tim,
    page_origin: (u16, u16),
    regions: &[SheetRegion],
    vram: &Vram,
    fallback: &[u16],
) -> Option<Composite> {
    if tim.mode != PixelMode::Bpp4 || fallback.len() < 16 {
        return None;
    }
    let w = tim.pixel_width();
    let h = tim.pixel_height();
    // Texel offset of the TIM's top-left on the page (4 texels per word).
    let ox = (tim.image.fb_x as i64 - page_origin.0 as i64) * 4;
    let oy = tim.image.fb_y as i64 - page_origin.1 as i64;
    // Per-texel owning region (first in table order) + contest marks.
    let mut owner: Vec<Option<usize>> = vec![None; w * h];
    let mut contested = vec![false; w * h];
    for (ri, r) in regions.iter().enumerate() {
        let (u, v, rw, rh) = r.rect;
        for y in v as i64..(v + rh) as i64 {
            let ty = y - oy;
            if ty < 0 || ty >= h as i64 {
                continue;
            }
            for x in u as i64..(u + rw) as i64 {
                let tx = x - ox;
                if tx < 0 || tx >= w as i64 {
                    continue;
                }
                let k = ty as usize * w + tx as usize;
                match owner[k] {
                    None => owner[k] = Some(ri),
                    Some(o) if regions[o].clut_fb != r.clut_fb => contested[k] = true,
                    _ => {}
                }
            }
        }
    }
    let palettes: Vec<Vec<u16>> = regions
        .iter()
        .map(|r| {
            (0..16)
                .map(|i| vram.pixel(r.clut_fb.0 as usize + i, r.clut_fb.1 as usize))
                .collect()
        })
        .collect();
    let row_bytes = tim.image.fb_w as usize * 2;
    let mut rgba = Vec::with_capacity(w * h * 4);
    let mut covered = 0;
    for y in 0..h {
        for x in 0..w {
            let byte = *tim.image.data.get(y * row_bytes + x / 2)?;
            let nib = if x & 1 == 0 { byte & 0x0F } else { byte >> 4 } as usize;
            let k = y * w + x;
            let px = match owner[k] {
                Some(ri) => {
                    covered += 1;
                    legaia_tim::bgr555_to_rgba8(palettes[ri][nib])
                }
                None => {
                    let mut p = legaia_tim::bgr555_to_rgba8(fallback[nib]);
                    p[3] /= 2;
                    p
                }
            };
            rgba.extend_from_slice(&px);
        }
    }
    Some(Composite {
        rgba,
        covered,
        contested: contested.iter().filter(|&&c| c).count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_tim::{Clut, Image};

    fn tim4(fb: (u16, u16), fb_w: u16, h: u16, clut_fb: (u16, u16), pals: &[[u16; 16]]) -> Tim {
        Tim {
            flags: 8,
            mode: PixelMode::Bpp4,
            clut: Some(Clut {
                fb_x: clut_fb.0,
                fb_y: clut_fb.1,
                w: 16,
                h: pals.len() as u16,
                entries: pals.iter().flatten().copied().collect(),
            }),
            image: Image {
                fb_x: fb.0,
                fb_y: fb.1,
                fb_w,
                h,
                // Every texel is index 1.
                data: vec![0x11; fb_w as usize * 2 * h as usize],
            },
        }
    }

    fn pal(c: u16) -> [u16; 16] {
        let mut p = [0u16; 16];
        p[1] = c;
        p
    }

    #[test]
    fn composite_decodes_each_region_through_its_own_vram_palette() {
        // 8x2 texels at the page origin; left half region -> VRAM cell A,
        // right half -> cell B, row 1 uncovered.
        let t = tim4(SHEET_PAGE_ORIGIN, 2, 2, (0, 511), &[pal(0x001F)]);
        let mut vram = Vram::new();
        vram.write_clut_row(0, 511, &[0, 0, 0xE0, 0x03]); // cell A: [1] = green
        vram.write_clut_row(16, 511, &[0, 0, 0x00, 0x7C]); // cell B: [1] = blue
        let regions = vec![
            SheetRegion {
                rect: (0, 0, 4, 1),
                clut_fb: (0, 511),
                widget_ids: vec![1],
            },
            SheetRegion {
                rect: (4, 0, 4, 1),
                clut_fb: (16, 511),
                widget_ids: vec![2],
            },
        ];
        let own = t.clut.as_ref().unwrap().entries.clone();
        let c = composite_rgba(&t, SHEET_PAGE_ORIGIN, &regions, &vram, &own).unwrap();
        assert_eq!(c.covered, 8);
        assert_eq!(c.contested, 0);
        assert_eq!(&c.rgba[0..4], &[0, 255, 0, 255]);
        assert_eq!(&c.rgba[4 * 4..4 * 4 + 4], &[0, 0, 255, 255]);
        // Row 1 falls back to the TIM's own palette at half alpha.
        assert_eq!(&c.rgba[8 * 4..8 * 4 + 4], &[255, 0, 0, 127]);
    }

    #[test]
    fn composite_counts_texels_drawn_in_two_palettes() {
        let t = tim4(SHEET_PAGE_ORIGIN, 1, 1, (0, 511), &[pal(0x001F)]);
        let vram = Vram::new();
        let regions = vec![
            SheetRegion {
                rect: (0, 0, 4, 1),
                clut_fb: (0, 511),
                widget_ids: vec![1],
            },
            SheetRegion {
                rect: (2, 0, 2, 1),
                clut_fb: (16, 511),
                widget_ids: vec![2],
            },
        ];
        let own = t.clut.as_ref().unwrap().entries.clone();
        let c = composite_rgba(&t, SHEET_PAGE_ORIGIN, &regions, &vram, &own).unwrap();
        assert_eq!(c.contested, 2);
    }

    #[test]
    fn composite_offsets_a_tim_that_sits_lower_on_the_page() {
        // The extension strip sits at (896, 448): page v 192.
        let t = tim4((896, 448), 1, 1, (896, 498), &[pal(0x001F)]);
        let mut vram = Vram::new();
        vram.write_clut_row(0, 511, &[0, 0, 0xE0, 0x03]);
        let regions = vec![SheetRegion {
            rect: (0, 192, 4, 1),
            clut_fb: (0, 511),
            widget_ids: vec![0x8B],
        }];
        let own = t.clut.as_ref().unwrap().entries.clone();
        let c = composite_rgba(&t, SHEET_PAGE_ORIGIN, &regions, &vram, &own).unwrap();
        assert_eq!(c.covered, 4);
        assert_eq!(&c.rgba[0..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn empty_and_flag_counts() {
        let mut p0 = pal(0x801F);
        p0[2] = 0x8000;
        let t = tim4((0, 0), 1, 1, (0, 479), &[p0, [0; 16]]);
        assert_eq!(empty_palettes(&t), vec![1]);
        // entries 1 and 2 carry STP; 14 entries are the 0x0000 key.
        assert_eq!(palette_flag_counts(&t, 0), (2, 14));
    }

    #[test]
    fn row_palettes_lists_only_written_cells() {
        let bundle = SystemUiBundle {
            tims: Vec::new(),
            row_patches: Vec::new(),
            member_counts: [0, 0],
        };
        let mut ctx = BootClutVram::from_bundle(&bundle, [0, 0]);
        ctx.vram.write_clut_row(32, 510, &[1, 0]);
        let pals = ctx.row_palettes(510, 16);
        assert_eq!(pals.len(), 1);
        assert_eq!((pals[0].fb_x, pals[0].fb_y), (32, 510));
        assert_eq!(pals[0].entries[0], 1);
    }
}
