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
use crate::ui_widgets::{self, ClaimPart, SHEET_VRAM_ORIGIN, SUBPALETTE_EXT_FIRST, WidgetTable};

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

/// One rectangle of the system-UI texture page one widget record samples,
/// and the CLUT cell it samples it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetRegion {
    /// `(u, v, w, h)` in texels. From [`sheet_palette_regions`] it is on the
    /// page (relative to [`SHEET_VRAM_ORIGIN`]); in a [`TexelPalettes`] it is
    /// relative to that TIM's top-left and clipped to it.
    pub rect: (u16, u16, u16, u16),
    /// VRAM cell of its 16-entry CLUT ([`ui_widgets::clut_fb`] of the
    /// record's palette byte).
    pub clut_fb: (u16, u16),
    /// Widget record id.
    pub widget: u8,
    /// The record's frame class (selects the draw arm).
    pub class: u8,
    pub part: ClaimPart,
}

impl SheetRegion {
    /// Row-511 sub-palette number (`clut_fb.x / 16`), or `None` for the
    /// `(896.., 498..501)` badge-block form.
    pub const fn subpalette(&self) -> Option<u16> {
        if self.clut_fb.1 == 511 {
            Some(self.clut_fb.0 / 16)
        } else {
            None
        }
    }

    /// Precedence when two regions sample the same texel (lower wins):
    /// the draw paths whose sheet reads are pinned first - single sprites
    /// (class 5, `FUN_8002C488`), then plate runs (class 3), bar runs
    /// (class 4), framed windows (class 0), then the rest - and within a
    /// path the smaller rectangle (the most specific sprite), then the
    /// lower record id.
    fn precedence(&self) -> (u8, usize, u8) {
        let tier = match self.class {
            5 => 0u8,
            3 => 1,
            4 => 2,
            0 => 3,
            _ => 4,
        };
        (
            tier,
            self.rect.2 as usize * self.rect.3 as usize,
            self.widget,
        )
    }
}

/// Every `(rectangle, CLUT cell)` the widget-class table draws off the
/// system-UI page ([`WidgetTable::sheet_claims`]), in table order, one per
/// claim. The portrait records sample another page and are left out.
pub fn sheet_palette_regions(table: &WidgetTable) -> Vec<SheetRegion> {
    table
        .sheet_claims()
        .into_iter()
        .map(|c| SheetRegion {
            rect: (
                c.rect.0 as u16,
                c.rect.1 as u16,
                c.rect.2 as u16,
                c.rect.3 as u16,
            ),
            clut_fb: ui_widgets::clut_fb(c.palette),
            widget: c.widget,
            class: c.class,
            part: c.part,
        })
        .collect()
}

/// A rectangle of a sheet-page TIM another TIM uploads over at runtime: the
/// game never shows the covered TIM's texels there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoveredRect {
    /// `(x, y, w, h)` in the covered TIM's texels.
    pub rect: (u16, u16, u16, u16),
    /// `PROT.DAT` byte offset of the TIM whose pixels the game shows there.
    pub by_offset: u64,
    /// VRAM cell of that TIM's own CLUT.
    pub clut_fb: (u16, u16),
}

/// The one answer to "which palette draws which texel" for a TIM on the
/// system-UI page. Both the viewer's as-drawn composite and the texture
/// editor's per-pixel palette map read it.
#[derive(Debug, Clone, Default)]
pub struct TexelPalettes {
    pub width: usize,
    pub height: usize,
    /// Every region that lands on this TIM outside the covered rectangle,
    /// TIM-relative and clipped, in table order.
    pub regions: Vec<SheetRegion>,
    /// Per texel (row-major): index into `regions` of the region the texel
    /// is attributed to ([`SheetRegion`] precedence), `None` when no region
    /// samples it or it is covered.
    pub owner: Vec<Option<u16>>,
    /// Texels two regions sample through **different** CLUT cells - the
    /// game really draws that art in several colours; `owner` names the one
    /// with precedence.
    pub contested: usize,
    /// The rectangle another TIM covers at runtime, when one does.
    pub covered: Option<CoveredRect>,
}

impl TexelPalettes {
    /// Texels some region is attributed to.
    pub fn claimed(&self) -> usize {
        self.owner.iter().filter(|o| o.is_some()).count()
    }

    /// The CLUT cell texel `(x, y)` is drawn through, if a region names one.
    pub fn clut_at(&self, x: usize, y: usize) -> Option<(u16, u16)> {
        let o = (*self.owner.get(y * self.width + x)?)?;
        Some(self.regions[o as usize].clut_fb)
    }

    /// Is texel `(x, y)` inside the covered rectangle?
    pub fn is_covered(&self, x: usize, y: usize) -> bool {
        self.covered.is_some_and(|c| {
            let (cx, cy, cw, ch) = c.rect;
            (cx as usize..(cx + cw) as usize).contains(&x)
                && (cy as usize..(cy + ch) as usize).contains(&y)
        })
    }
}

/// Is `tim` a 4bpp image lying on the system-UI texture page?
pub fn on_sheet_page(tim: &Tim) -> bool {
    let (px, py) = SHEET_VRAM_ORIGIN;
    let img = &tim.image;
    tim.mode == PixelMode::Bpp4
        && img.fb_x >= px
        && img.fb_x + img.fb_w <= px + 64
        && img.fb_y >= py
        && img.fb_y + img.h <= py + 256
}

/// Parse the button-glyph TIM ([`ui_widgets::BUTTON_GLYPH_TIM_PROT_OFFSET`])
/// out of bytes starting at it, checking it is the row-511 sprite set.
pub fn parse_button_glyph_tim(bytes: &[u8]) -> Option<Tim> {
    legaia_tim::parse_strict(bytes)
        .ok()
        .filter(|t| on_sheet_page(t) && t.clut.as_ref().is_some_and(|c| c.fb_y == 511))
}

/// Parse the row-511 sub-palette extension TIM
/// ([`ui_widgets::SUBPALETTE_EXT_TIM_PROT_OFFSET`]) out of bytes starting at
/// it, checking its CLUT lands on `(256, 511)`.
pub fn parse_subpalette_ext_tim(bytes: &[u8]) -> Option<Tim> {
    legaia_tim::parse_strict(bytes).ok().filter(|t| {
        t.clut
            .as_ref()
            .is_some_and(|c| (c.fb_x, c.fb_y) == (SUBPALETTE_EXT_FIRST * 16, 511))
    })
}

/// Attribute every texel of `tim` (a 4bpp TIM on the system-UI page) to the
/// region that draws it. `cover` is the button-glyph TIM when it could be
/// read: its image uploads over part of the sheet at runtime, so the
/// regions under it draw *its* texels and are left out here.
pub fn texel_palettes(
    tim: &Tim,
    regions: &[SheetRegion],
    cover: Option<&Tim>,
) -> Option<TexelPalettes> {
    if tim.mode != PixelMode::Bpp4 {
        return None;
    }
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    // Texel offset of the TIM's top-left on the page (4 texels per word).
    let ox = (tim.image.fb_x as i64 - SHEET_VRAM_ORIGIN.0 as i64) * 4;
    let oy = tim.image.fb_y as i64 - SHEET_VRAM_ORIGIN.1 as i64;
    let clip = |(u, v, rw, rh): (u16, u16, u16, u16)| -> Option<(u16, u16, u16, u16)> {
        let x0 = (u as i64 - ox).max(0);
        let y0 = (v as i64 - oy).max(0);
        let x1 = (u as i64 + rw as i64 - ox).min(w as i64);
        let y1 = (v as i64 + rh as i64 - oy).min(h as i64);
        (x0 < x1 && y0 < y1).then_some((x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16))
    };
    let covered = cover
        .filter(|c| (c.image.fb_x, c.image.fb_y) != (tim.image.fb_x, tim.image.fb_y))
        .and_then(|c| {
            let rect = clip((
                (c.image.fb_x - SHEET_VRAM_ORIGIN.0) * 4,
                c.image.fb_y.checked_sub(SHEET_VRAM_ORIGIN.1)?,
                c.pixel_width() as u16,
                c.pixel_height() as u16,
            ))?;
            let clut = c.clut.as_ref()?;
            Some(CoveredRect {
                rect,
                by_offset: ui_widgets::BUTTON_GLYPH_TIM_PROT_OFFSET as u64,
                clut_fb: (clut.fb_x, clut.fb_y),
            })
        });
    let mut out = TexelPalettes {
        width: w,
        height: h,
        covered,
        ..Default::default()
    };
    for r in regions {
        let Some(rect) = clip(r.rect) else { continue };
        let tr = SheetRegion { rect, ..*r };
        let (x, y, cw, ch) = rect;
        let all_covered = out.is_covered(x as usize, y as usize)
            && out.is_covered((x + cw - 1) as usize, (y + ch - 1) as usize);
        if !all_covered {
            out.regions.push(tr);
        }
    }
    let mut order: Vec<usize> = (0..out.regions.len()).collect();
    order.sort_by_key(|&i| (out.regions[i].precedence(), i));
    out.owner = vec![None; w * h];
    let mut contested = vec![false; w * h];
    for &i in &order {
        let r = out.regions[i];
        for y in r.rect.1 as usize..(r.rect.1 + r.rect.3) as usize {
            for x in r.rect.0 as usize..(r.rect.0 + r.rect.2) as usize {
                if out.is_covered(x, y) {
                    continue;
                }
                let k = y * w + x;
                match out.owner[k] {
                    None => out.owner[k] = Some(i as u16),
                    Some(o) if out.regions[o as usize].clut_fb != r.clut_fb => contested[k] = true,
                    _ => {}
                }
            }
        }
    }
    out.contested = contested.iter().filter(|&&c| c).count();
    Some(out)
}

/// Decode a sheet-page TIM as the game draws it: every attributed texel
/// through its region's CLUT read from `vram`, the covered rectangle as the
/// covering TIM's own texels through its own CLUT (from `vram`), and every
/// other texel through `fallback` (typically the TIM's own palette 0) with
/// its alpha halved, so art no table entry places reads as such at a
/// glance. Returns row-major RGBA8 at the TIM's own size.
pub fn composite_rgba(
    tim: &Tim,
    texels: &TexelPalettes,
    vram: &Vram,
    fallback: &[u16],
    cover: Option<&Tim>,
) -> Option<Vec<u8>> {
    if tim.mode != PixelMode::Bpp4 || fallback.len() < 16 {
        return None;
    }
    let (w, h) = (texels.width, texels.height);
    let cell = |(x, y): (u16, u16)| -> Vec<u16> {
        (0..16)
            .map(|i| vram.pixel(x as usize + i, y as usize))
            .collect()
    };
    let palettes: Vec<Vec<u16>> = texels.regions.iter().map(|r| cell(r.clut_fb)).collect();
    let nibble = |t: &Tim, x: usize, y: usize| -> Option<usize> {
        let byte = *t.image.data.get(y * t.image.fb_w as usize * 2 + x / 2)?;
        Some(if x & 1 == 0 { byte & 0x0F } else { byte >> 4 } as usize)
    };
    let cover_pal = texels.covered.map(|c| cell(c.clut_fb));
    // Covered texel (x, y) of `tim` -> texel of `cover`.
    let cover_at = |x: usize, y: usize| -> Option<(usize, usize)> {
        let c = cover?;
        let cx = (tim.image.fb_x as i64 - c.image.fb_x as i64) * 4 + x as i64;
        let cy = tim.image.fb_y as i64 - c.image.fb_y as i64 + y as i64;
        (cx >= 0 && cy >= 0).then_some((cx as usize, cy as usize))
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            if texels.is_covered(x, y)
                && let (Some(pal), Some((cx, cy)), Some(c)) = (&cover_pal, cover_at(x, y), cover)
            {
                let nib = nibble(c, cx, cy)?;
                rgba.extend_from_slice(&legaia_tim::bgr555_to_rgba8(pal[nib]));
                continue;
            }
            let nib = nibble(tim, x, y)?;
            let px = match texels.owner[y * w + x] {
                Some(ri) => legaia_tim::bgr555_to_rgba8(palettes[ri as usize][nib]),
                None => {
                    let mut p = legaia_tim::bgr555_to_rgba8(fallback[nib]);
                    p[3] /= 2;
                    p
                }
            };
            rgba.extend_from_slice(&px);
        }
    }
    Some(rgba)
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

    fn region(
        widget: u8,
        class: u8,
        rect: (u16, u16, u16, u16),
        clut_fb: (u16, u16),
    ) -> SheetRegion {
        SheetRegion {
            rect,
            clut_fb,
            widget,
            class,
            part: ClaimPart::Rect,
        }
    }

    #[test]
    fn composite_decodes_each_region_through_its_own_vram_palette() {
        // 8x2 texels at the page origin; left half region -> VRAM cell A,
        // right half -> cell B, row 1 uncovered.
        let t = tim4(SHEET_VRAM_ORIGIN, 2, 2, (0, 511), &[pal(0x001F)]);
        let mut vram = Vram::new();
        vram.write_clut_row(0, 511, &[0, 0, 0xE0, 0x03]); // cell A: [1] = green
        vram.write_clut_row(16, 511, &[0, 0, 0x00, 0x7C]); // cell B: [1] = blue
        let regions = vec![
            region(1, 5, (0, 0, 4, 1), (0, 511)),
            region(2, 5, (4, 0, 4, 1), (16, 511)),
        ];
        let tp = texel_palettes(&t, &regions, None).unwrap();
        assert_eq!(tp.claimed(), 8);
        assert_eq!(tp.contested, 0);
        let own = t.clut.as_ref().unwrap().entries.clone();
        let rgba = composite_rgba(&t, &tp, &vram, &own, None).unwrap();
        assert_eq!(&rgba[0..4], &[0, 255, 0, 255]);
        assert_eq!(&rgba[4 * 4..4 * 4 + 4], &[0, 0, 255, 255]);
        // Row 1 falls back to the TIM's own palette at half alpha.
        assert_eq!(&rgba[8 * 4..8 * 4 + 4], &[255, 0, 0, 127]);
    }

    #[test]
    fn precedence_is_draw_path_then_smallest_rect_and_contests_are_counted() {
        let t = tim4(SHEET_VRAM_ORIGIN, 4, 8, (0, 511), &[pal(0x001F)]);
        let regions = vec![
            // A class-4 bar body over the same texels as a class-5 badge:
            // the pinned single sprite wins even though it is larger.
            region(6, 4, (0, 0, 4, 4), (80, 511)),
            region(0x1F, 5, (0, 0, 8, 4), (208, 511)),
            // Two class-5 sprites: the smaller one wins.
            region(9, 5, (0, 4, 16, 4), (0, 511)),
            region(1, 5, (8, 4, 4, 4), (64, 511)),
        ];
        let tp = texel_palettes(&t, &regions, None).unwrap();
        assert_eq!(tp.clut_at(0, 0), Some((208, 511)));
        assert_eq!(tp.clut_at(8, 5), Some((64, 511)));
        assert_eq!(tp.clut_at(0, 5), Some((0, 511)));
        assert_eq!(tp.contested, 16 + 16);
        assert_eq!(regions[1].subpalette(), Some(13));
    }

    #[test]
    fn a_covering_tim_hides_its_rectangle_and_the_composite_shows_it() {
        // Sheet 16x4 texels; the cover TIM sits at page texel (8, 0), 8x4,
        // every texel index 1 through its own CLUT at (304, 511).
        let t = tim4(SHEET_VRAM_ORIGIN, 4, 4, (0, 511), &[pal(0x001F)]);
        let cover = tim4((898, 256), 2, 4, (304, 511), &[pal(0x03E0)]);
        let mut vram = Vram::new();
        vram.write_clut_row(0, 511, &[0, 0, 0x1F, 0x00]); // sub 0: red
        vram.write_clut_row(304, 511, &[0, 0, 0xE0, 0x03]); // sub 19: green
        let regions = vec![
            region(9, 5, (0, 0, 16, 4), (0, 511)),
            region(0x37, 5, (8, 0, 8, 4), (304, 511)),
        ];
        let tp = texel_palettes(&t, &regions, Some(&cover)).unwrap();
        assert_eq!(tp.covered.unwrap().rect, (8, 0, 8, 4));
        assert_eq!(tp.regions.len(), 1, "the glyph region is wholly covered");
        assert_eq!(tp.clut_at(9, 1), None);
        assert_eq!(tp.claimed(), 32);
        let own = t.clut.as_ref().unwrap().entries.clone();
        let rgba = composite_rgba(&t, &tp, &vram, &own, Some(&cover)).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[9 * 4..9 * 4 + 4], &[0, 255, 0, 255]);
        // The cover TIM is not covered by itself.
        let own_view = texel_palettes(&cover, &regions, Some(&cover)).unwrap();
        assert!(own_view.covered.is_none());
    }

    #[test]
    fn composite_offsets_a_tim_that_sits_lower_on_the_page() {
        // The extension strip sits at (896, 448): page v 192.
        let t = tim4((896, 448), 1, 1, (896, 498), &[pal(0x001F)]);
        let mut vram = Vram::new();
        vram.write_clut_row(0, 511, &[0, 0, 0xE0, 0x03]);
        let regions = vec![region(0x8B, 5, (0, 192, 4, 1), (0, 511))];
        let tp = texel_palettes(&t, &regions, None).unwrap();
        assert_eq!(tp.claimed(), 4);
        let own = t.clut.as_ref().unwrap().entries.clone();
        let rgba = composite_rgba(&t, &tp, &vram, &own, None).unwrap();
        assert_eq!(&rgba[0..4], &[0, 255, 0, 255]);
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
