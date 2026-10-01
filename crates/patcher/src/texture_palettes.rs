//! Which palette the game draws each pixel of a texture through - the
//! texture editor's view of it.
//!
//! A multi-palette TIM does not say which of its palettes a region is meant
//! to be seen in - the draw packet does. For most textures nothing on the
//! disc pins that per region, and every palette is an equally honest view.
//! For the **system-UI sheet** (the 256x192 4 bpp TIM at `PROT.DAT` `0x18E0`
//! uploaded to VRAM `(896, 256)`: menu and battle chrome, plates, status
//! badges, gauge, button glyphs) it is disc data, and the one kernel that
//! reads it is [`legaia_asset::tim_palette_context::texel_palettes`]: which
//! widget record draws which texel through which CLUT cell, which record
//! wins where two overlap, and which rectangle another TIM covers at
//! runtime. The asset viewer's "as the game draws it" view reads the same
//! kernel. This module only translates its CLUT cells into this texture's
//! palette indices for the [`legaia_tim::multi_palette`] editor.
//!
//! A cell on VRAM row 511 at `x = 16k` is sub-palette `k`. The sheet's
//! 16x16 CLUT block lands flattened on that row, so sub-palette `k < 16` is
//! the sheet's own palette `k`. Sub-palettes 16..18 are the three rows of a
//! CLUT-only sibling TIM at `PROT.DAT` `0x1858` (uploaded to `(256, 511)`),
//! carried here as read-only external palettes. The rectangle the
//! button-glyph TIM at `PROT.DAT` `0x7B00` covers is left out of the map and
//! named in [`TexturePalettes::notes`] - edit that TIM instead.

use anyhow::Result;

use legaia_asset::tim_palette_context::{
    SheetRegion, parse_button_glyph_tim, parse_subpalette_ext_tim, sheet_palette_regions,
    texel_palettes,
};
use legaia_asset::ui_widgets::{
    BUTTON_GLYPH_TIM_PROT_OFFSET, ClaimPart, SHEET_VRAM_ORIGIN, SUBPALETTE_EXT_FIRST,
    SUBPALETTE_EXT_TIM_PROT_OFFSET, WidgetTable,
};
use legaia_tim::multi_palette::{PaletteContext, own_palettes};
use legaia_tim::{PixelMode, Tim};

use crate::disc::DiscPatcher;

/// One rectangle of the texture and the palette the game draws it through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteRegion {
    /// `(x, y, w, h)` in texture pixels (clipped to the texture).
    pub rect: (u16, u16, u16, u16),
    /// Index into the texture's own palettes, then the external ones.
    pub palette: usize,
    /// The row-511 sub-palette number the game addresses.
    pub subpalette: u16,
    /// Widget record id that draws it.
    pub widget: u8,
    pub part: ClaimPart,
}

/// The palette map of one texture, plus what it rests on.
#[derive(Debug, Clone, Default)]
pub struct TexturePalettes {
    /// External palettes + per-pixel map, ready for
    /// [`legaia_tim::multi_palette::import_png`].
    pub context: PaletteContext,
    /// Every region the map was built from, in table order.
    pub regions: Vec<PaletteRegion>,
    /// Pixels no region covers (drawn through palette 0 in the map).
    pub unclaimed_pixels: usize,
    /// Pixels two regions draw through different palettes (the map holds
    /// the one with precedence).
    pub contested_pixels: usize,
    /// One line on where the map comes from, empty when there is none.
    pub source: String,
    /// Things a modder must know about this texture's regions (rectangles
    /// another texture covers at runtime, palettes that could not be
    /// resolved).
    pub notes: Vec<String>,
}

impl TexturePalettes {
    /// `true` when a per-region map is known.
    pub fn has_map(&self) -> bool {
        self.context.map.is_some()
    }
}

/// Is `tim` the system-UI sheet (by its placement, not its offset: any build
/// that uploads a 16-palette 4 bpp sheet to the widget page qualifies)?
pub fn is_system_ui_sheet(tim: &Tim) -> bool {
    let Some(c) = tim.clut.as_ref() else {
        return false;
    };
    tim.mode == PixelMode::Bpp4
        && (tim.image.fb_x, tim.image.fb_y) == SHEET_VRAM_ORIGIN
        && (c.fb_x, c.fb_y, c.w) == (0, 511, 16)
        && tim.palette_count() == 16
}

/// Build the system-UI sheet's map from the widget table's page regions
/// ([`sheet_palette_regions`]). `ext` is the sub-palette extension TIM and
/// `cover` the button-glyph TIM, each when it could be read.
pub fn sheet_palettes(
    tim: &Tim,
    page_regions: &[SheetRegion],
    ext: Option<&Tim>,
    cover: Option<&Tim>,
) -> TexturePalettes {
    let Some(texels) = texel_palettes(tim, page_regions, cover) else {
        return TexturePalettes::default();
    };
    let own = own_palettes(tim).len();
    let external: Vec<Vec<u16>> = ext.map(own_palettes).unwrap_or_default();
    // CLUT cell -> index into own palettes, then the external ones.
    let resolve = |r: &SheetRegion| -> Option<(usize, u16)> {
        let sub = r.subpalette()?;
        if (sub as usize) < own {
            return Some((sub as usize, sub));
        }
        let k = sub.checked_sub(SUBPALETTE_EXT_FIRST)? as usize;
        (k < external.len()).then_some((own + k, sub))
    };

    let mut notes = Vec::new();
    let mut unresolved: Vec<((u16, u16), Vec<u8>)> = Vec::new();
    let resolved: Vec<Option<(usize, u16)>> = texels.regions.iter().map(resolve).collect();
    let mut regions = Vec::new();
    for (r, res) in texels.regions.iter().zip(&resolved) {
        match res {
            Some((palette, subpalette)) => regions.push(PaletteRegion {
                rect: r.rect,
                palette: *palette,
                subpalette: *subpalette,
                widget: r.widget,
                part: r.part,
            }),
            None => match unresolved.iter_mut().find(|(c, _)| *c == r.clut_fb) {
                Some((_, ids)) => ids.push(r.widget),
                None => unresolved.push((r.clut_fb, vec![r.widget])),
            },
        }
    }
    let map: Vec<u16> = texels
        .owner
        .iter()
        .map(|o| {
            o.and_then(|i| resolved[i as usize])
                .map_or(0, |(p, _)| p as u16)
        })
        .collect();
    let unclaimed_pixels = texels
        .owner
        .iter()
        .filter(|o| o.and_then(|i| resolved[i as usize]).is_none())
        .count();

    if let (Some(c), Some(cv)) = (texels.covered, cover) {
        let (x, y, w, h) = c.rect;
        notes.push(format!(
            "texels ({x}, {y})..({}, {}) are covered at runtime by the {}x{} button-glyph TIM \
             at PROT.DAT 0x{BUTTON_GLYPH_TIM_PROT_OFFSET:X} (its own pixels and palette, \
             sub-palette {}) - what the game shows there is that TIM, so edit it instead; the \
             sheet's pixels under it are never seen",
            x + w - 1,
            y + h - 1,
            cv.pixel_width(),
            cv.pixel_height(),
            c.clut_fb.0 / 16,
        ));
    }
    for (cell, ids) in &unresolved {
        let ids: Vec<String> = ids.iter().map(|i| format!("0x{i:02X}")).collect();
        notes.push(format!(
            "widget(s) {} draw through the CLUT at VRAM {cell:?}, which is neither this \
             texture's palette nor its extension's - their texels are shown through palette 0 \
             here",
            ids.join(", ")
        ));
    }
    TexturePalettes {
        context: PaletteContext {
            external,
            map: Some(map),
        },
        regions,
        unclaimed_pixels,
        contested_pixels: texels.contested,
        source: "the SCUS widget-class table (0x800732A4): each sprite's rect and the palette \
                 byte it is drawn with"
            .to_string(),
        notes,
    }
}

/// The palette map of a texture read off `patcher`'s disc. Textures with no
/// known map (every one but the system-UI sheet) get an empty context: every
/// palette is an equally valid view.
pub fn texture_palettes(patcher: &DiscPatcher, tim: &Tim) -> Result<TexturePalettes> {
    if !is_system_ui_sheet(tim) {
        return Ok(TexturePalettes::default());
    }
    let exe = crate::translation::lift::boot_exe_name(patcher)
        .unwrap_or_else(|_| "SCUS_942.54".to_string());
    let Some(scus) = patcher.read_named_file(&exe) else {
        return Ok(TexturePalettes::default());
    };
    let Some(table) = WidgetTable::from_scus(&scus) else {
        return Ok(TexturePalettes::default());
    };
    let read = |off: usize| patcher.read_prot_bytes(off as u64, 0x800).ok();
    let ext = read(SUBPALETTE_EXT_TIM_PROT_OFFSET).and_then(|b| parse_subpalette_ext_tim(&b));
    let cover = read(BUTTON_GLYPH_TIM_PROT_OFFSET).and_then(|b| parse_button_glyph_tim(&b));
    Ok(sheet_palettes(
        tim,
        &sheet_palette_regions(&table),
        ext.as_ref(),
        cover.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic sheet: 4bpp 64x8 at (896, 256), CLUT 16x16 at (0, 511).
    fn sheet() -> Tim {
        let mut b = vec![];
        b.extend_from_slice(&0x10u32.to_le_bytes());
        b.extend_from_slice(&0x08u32.to_le_bytes());
        b.extend_from_slice(&(12u32 + 16 * 16 * 2).to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&511u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        for p in 0..16u16 {
            for e in 0..16u16 {
                b.extend_from_slice(&((p << 5) | e).to_le_bytes());
            }
        }
        b.extend_from_slice(&(12u32 + 16 * 8 * 2).to_le_bytes());
        b.extend_from_slice(&896u16.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend(std::iter::repeat_n(0x21u8, 16 * 8 * 2));
        legaia_tim::parse_strict(&b).unwrap()
    }

    fn ext() -> Tim {
        let mut b = vec![];
        b.extend_from_slice(&0x10u32.to_le_bytes());
        b.extend_from_slice(&0x08u32.to_le_bytes());
        b.extend_from_slice(&(12u32 + 16 * 3 * 2).to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&511u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend(std::iter::repeat_n(0x11u8, 16 * 3 * 2));
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(&896u16.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend(std::iter::repeat_n(0u8, 8));
        legaia_tim::parse_strict(&b).unwrap()
    }

    fn region(widget: u8, rect: (u16, u16, u16, u16), palette: u8) -> SheetRegion {
        SheetRegion {
            rect,
            clut_fb: legaia_asset::ui_widgets::clut_fb(palette),
            widget,
            class: 5,
            part: ClaimPart::Rect,
        }
    }

    #[test]
    fn map_resolves_sub_palettes_to_own_and_external_palettes() {
        let tim = sheet();
        assert!(is_system_ui_sheet(&tim));
        let regions = [
            region(9, (0, 0, 32, 8), 0x00),   // big panel, palette 0
            region(1, (8, 0, 8, 4), 0x04),    // plate inside it, palette 4
            region(2, (40, 0, 8, 8), 0x11),   // status badge on sub-palette 17
            region(3, (48, 0, 8, 8), 0x45),   // badge-block form: not this sheet's
            region(4, (60, 6, 16, 16), 0x86), // clipped at the edge, bit 7 ignored
        ];
        let e = ext();
        let m = sheet_palettes(&tim, &regions, Some(&e), None);
        let map = m.context.map.as_ref().unwrap();
        let at = |x: usize, y: usize| map[y * 64 + x];
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(8, 0), 4, "the smaller rect wins");
        assert_eq!(at(8, 5), 0);
        assert_eq!(at(40, 0), 16 + 1, "sub-palette 17 = external row 1");
        assert_eq!(at(48, 0), 0, "badge-block form never maps");
        assert_eq!(at(63, 7), 6);
        assert_eq!(m.context.external.len(), 3);
        let clipped = m.regions.iter().find(|r| r.widget == 4).unwrap();
        assert_eq!(clipped.rect, (60, 6, 4, 2));
        // Unclaimed: 64*8 - (32*8 + 8*8 + 4*2).
        assert_eq!(m.unclaimed_pixels, 64 * 8 - (256 + 64 + 8));
        assert!(m.notes.iter().any(|n| n.contains("0x03")), "{:?}", m.notes);
    }

    #[test]
    fn without_the_extension_its_regions_drop_out() {
        let tim = sheet();
        let regions = [region(2, (40, 0, 8, 8), 0x11)];
        let m = sheet_palettes(&tim, &regions, None, None);
        assert!(m.regions.is_empty());
        assert!(m.context.external.is_empty());
        assert!(
            m.notes.iter().any(|n| n.contains("(272, 511)")),
            "{:?}",
            m.notes
        );
    }
}
