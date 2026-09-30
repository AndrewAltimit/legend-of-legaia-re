//! Which palette the game draws each pixel of a texture through.
//!
//! A multi-palette TIM does not say which of its palettes a region is meant
//! to be seen in - the draw packet does. For most textures nothing on the
//! disc pins that per region, and every palette is an equally honest view.
//! For the **system-UI sheet** (the 256x192 4 bpp TIM at `PROT.DAT` `0x18E0`
//! uploaded to VRAM `(896, 256)`: menu and battle chrome, plates, status
//! badges, gauge, button glyphs) it is disc data: the widget-class table at
//! `SCUS_942.54` `0x800732A4` gives every sprite's `(u, v, w, h)` and the
//! palette byte it is drawn through ([`legaia_asset::ui_widgets`]). This
//! module turns that table into a per-pixel palette map the
//! [`legaia_tim::multi_palette`] editor consumes.
//!
//! How a palette byte names one of the sheet's palettes: the sheet's CLUT
//! block is 16x16 on disc (at `(0, 511)`), and at runtime its row `k` sits
//! at VRAM `(16k, 511)` - the "sub-palette `k`" a byte `b` with bit 6 clear
//! addresses as `b & 0x3F`. Sub-palettes 16..18 are **not** in the sheet:
//! they are the three rows of a CLUT-only sibling TIM at `PROT.DAT` `0x1858`
//! (uploaded to `(256, 511)`), and are carried here as read-only external
//! palettes. Bit 6 set addresses a badge CLUT block at `(896.., 498..)`;
//! every record that uses it samples texels below the sheet (`v >= 192`), so
//! it never reaches this map.
//!
//! Where two records sample the same texels, the draw paths whose sheet
//! reads are pinned win - single sprites (class 5), then plate runs (class
//! 3), then framed windows (class 0), then the rest - and within a path the
//! smaller rectangle (the most specific sprite), then the lower record id.
//! Texels no record samples are drawn through palette 0 in the map - overlay
//! code may still draw them with a palette of its own, and
//! [`TexturePalettes::unclaimed_pixels`] says how many there are.
//!
//! One rectangle of the sheet is not what the game shows at all: the
//! 64x32 button-glyph TIM at `PROT.DAT` `0x7B00` uploads over texels
//! `(128, 96)..(191, 127)` at runtime, and the records that draw there
//! (`0x37..=0x3E`, sub-palette 19 - that TIM's own palette) draw *its*
//! pixels. [`TexturePalettes::notes`] says so; edit that TIM instead.

use anyhow::Result;

use legaia_asset::ui_widgets::{
    ClaimPart, SHEET_VRAM_ORIGIN, SUBPALETTE_EXT_FIRST, SUBPALETTE_EXT_TIM_PROT_OFFSET, SheetClaim,
    WidgetTable,
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

/// Build the system-UI sheet's map from the widget claims. `ext` is the
/// sub-palette extension TIM when it could be read.
pub fn sheet_palettes(tim: &Tim, claims: &[SheetClaim], ext: Option<&Tim>) -> TexturePalettes {
    sheet_palettes_with(tim, claims, ext, None)
}

/// [`sheet_palettes`] plus the button-glyph TIM that covers part of the
/// sheet at runtime, when it could be read.
pub fn sheet_palettes_with(
    tim: &Tim,
    claims: &[SheetClaim],
    ext: Option<&Tim>,
    cover: Option<&Tim>,
) -> TexturePalettes {
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let own = own_palettes(tim).len();
    let external: Vec<Vec<u16>> = ext.map(own_palettes).unwrap_or_default();

    let resolve = |sub: u16| -> Option<usize> {
        let sub = sub as usize;
        if sub < own {
            Some(sub)
        } else {
            let k = sub.checked_sub(SUBPALETTE_EXT_FIRST as usize)?;
            (k < external.len()).then_some(own + k)
        }
    };

    let mut notes = Vec::new();
    let mut unresolved: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut regions = Vec::new();
    let mut classes = Vec::new();
    for c in claims {
        let Some(sub) = c.subpalette() else { continue };
        let Some(palette) = resolve(sub) else {
            if (c.rect.0 as usize) < w && (c.rect.1 as usize) < h {
                match unresolved.iter_mut().find(|(s, _)| *s == sub) {
                    Some((_, ids)) => ids.push(c.widget),
                    None => unresolved.push((sub, vec![c.widget])),
                }
            }
            continue;
        };
        let (u, v, cw, ch) = (
            c.rect.0 as usize,
            c.rect.1 as usize,
            c.rect.2 as usize,
            c.rect.3 as usize,
        );
        if u >= w || v >= h {
            continue;
        }
        let (cw, ch) = (cw.min(w - u), ch.min(h - v));
        regions.push(PaletteRegion {
            rect: (u as u16, v as u16, cw as u16, ch as u16),
            palette,
            subpalette: sub,
            widget: c.widget,
            part: c.part,
        });
        classes.push(c.class);
    }

    // Pinned draw paths first: single sprites, plate runs, framed windows.
    let tier = |class: u8| match class {
        5 => 0u8,
        3 => 1,
        0 => 2,
        _ => 3,
    };
    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by_key(|&i| {
        let r = regions[i].rect;
        (
            tier(classes[i]),
            r.2 as usize * r.3 as usize,
            regions[i].widget,
            i,
        )
    });
    let mut map = vec![0u16; w * h];
    let mut claimed = vec![false; w * h];
    for &i in &order {
        let r = &regions[i];
        for y in r.rect.1 as usize..(r.rect.1 + r.rect.3) as usize {
            for x in r.rect.0 as usize..(r.rect.0 + r.rect.2) as usize {
                let p = y * w + x;
                if !claimed[p] {
                    claimed[p] = true;
                    map[p] = r.palette as u16;
                }
            }
        }
    }
    let unclaimed_pixels = claimed.iter().filter(|&&c| !c).count();

    if let Some(cv) = cover {
        let u = cv.image.fb_x as i32 - tim.image.fb_x as i32;
        let v = cv.image.fb_y as i32 - tim.image.fb_y as i32;
        let (cw, ch) = (cv.pixel_width() as i32, cv.pixel_height() as i32);
        // VRAM words -> 4bpp texels on this page.
        let (u, cwt) = (u * 4, cw);
        if u >= 0 && v >= 0 && (u as usize) < w && (v as usize) < h {
            notes.push(format!(
                "texels ({u}, {v})..({}, {}) are covered at runtime by the {cwt}x{ch} \
                 button-glyph TIM at PROT.DAT 0x{:X} (its own pixels and palette, sub-palette \
                 {}) - what the game shows there is that TIM, so edit it instead; the sheet's \
                 pixels under it are never seen",
                u + cwt - 1,
                v + ch - 1,
                legaia_asset::ui_widgets::BUTTON_GLYPH_TIM_PROT_OFFSET,
                cv.clut.as_ref().map_or(0, |c| c.fb_x / 16),
            ));
        }
    }
    for (sub, ids) in &unresolved {
        let ids: Vec<String> = ids.iter().map(|i| format!("0x{i:02X}")).collect();
        notes.push(format!(
            "widget(s) {} draw through sub-palette {sub}, which is neither this texture's nor \
             its extension's - their texels are shown through palette 0 here",
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
    let ext = patcher
        .read_prot_bytes(SUBPALETTE_EXT_TIM_PROT_OFFSET as u64, 0x800)
        .ok()
        .and_then(|b| legaia_tim::parse_strict(&b).ok())
        .filter(|t| {
            t.clut
                .as_ref()
                .is_some_and(|c| (c.fb_x, c.fb_y) == (256, 511))
        });
    let cover = patcher
        .read_prot_bytes(
            legaia_asset::ui_widgets::BUTTON_GLYPH_TIM_PROT_OFFSET as u64,
            0x800,
        )
        .ok()
        .and_then(|b| legaia_tim::parse_strict(&b).ok())
        .filter(|t| t.clut.as_ref().is_some_and(|c| c.fb_y == 511));
    Ok(sheet_palettes_with(
        tim,
        &table.sheet_claims(),
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

    fn claim(widget: u8, rect: (u8, u8, u8, u8), palette: u8) -> SheetClaim {
        SheetClaim {
            widget,
            class: 5,
            part: ClaimPart::Rect,
            rect,
            palette,
        }
    }

    #[test]
    fn map_follows_claims_smallest_first_and_resolves_the_extension() {
        let tim = sheet();
        assert!(is_system_ui_sheet(&tim));
        let claims = [
            claim(9, (0, 0, 32, 8), 0x00),   // big panel, palette 0
            claim(1, (8, 0, 8, 4), 0x04),    // plate inside it, palette 4
            claim(2, (40, 0, 8, 8), 0x11),   // status badge on sub-palette 17
            claim(3, (48, 0, 8, 8), 0x45),   // badge-block form: not this sheet's
            claim(4, (60, 6, 16, 16), 0x86), // clipped at the edge, bit 7 ignored
        ];
        let e = ext();
        let m = sheet_palettes(&tim, &claims, Some(&e));
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
    }

    #[test]
    fn a_pinned_sprite_beats_a_smaller_rect_of_an_unpinned_class() {
        let tim = sheet();
        let mut odd = claim(6, (8, 0, 4, 4), 0x05);
        odd.class = 4;
        let claims = [claim(1, (0, 0, 16, 8), 0x0D), odd];
        let m = sheet_palettes(&tim, &claims, None);
        assert_eq!(m.context.map.as_ref().unwrap()[8], 13);
    }

    #[test]
    fn without_the_extension_its_regions_drop_out() {
        let tim = sheet();
        let claims = [claim(2, (40, 0, 8, 8), 0x11)];
        let m = sheet_palettes(&tim, &claims, None);
        assert!(m.regions.is_empty());
        assert!(m.context.external.is_empty());
        assert!(
            m.notes.iter().any(|n| n.contains("sub-palette 17")),
            "{:?}",
            m.notes
        );
    }
}
