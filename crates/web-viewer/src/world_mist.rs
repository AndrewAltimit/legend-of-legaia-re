//! World-overview Mist exports: the kingdom MAN's fog-region table and the
//! fog sheet's texels.
//!
//! Retail draws the Mist over a kingdom overworld with the field fog pool
//! (`FUN_801D629C` spawner, `FUN_8003F3FC` / `FUN_8003F86C` draw): additive
//! half-sheets spawned inside the MAN section-4 regions whose story flag is
//! still clear. These two exports hand the page what it needs to draw the
//! same sheets over the whole continent - see
//! `docs/subsystems/world-overview-viewer.md#the-mist`.
use super::*;

/// Index of the fog-region table among [`legaia_scene_formats::man_section::ManFile::sibling_sections`]
/// (MAN section 4, `DAT_80073ED8`).
const FOG_REGION_SIBLING_SECTION: usize = 3;

/// Fog sheet art: page `(448, 0)` 4bpp, rows `v 0x40..0x6F`, `u 0..0x3F`.
const SHEET_PAGE_X: usize = 448;
const SHEET_V0: usize = 0x40;
const SHEET_W: usize = 64;
const SHEET_H: usize = 48;
/// CLUT `0x7640` = `(0, 473)`.
const SHEET_CLUT_Y: usize = 473;

/// Words per region in [`LegaiaViewer::walk_mist_regions`].
const MIST_REGION_WORDS: usize = 9;

/// Decode the fog-region table of the kingdom MAN at `prot_base` (or
/// `prot_base + 1`, the probe order the walk-ground loader uses).
pub fn kingdom_fog_regions(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<Vec<legaia_render_kernels::fog_volume::FogRegion>> {
    [prot_base, prot_base + 1].into_iter().find_map(|idx| {
        let meta = entries.iter().find(|e| e.index == idx)?;
        let off = meta.byte_offset as usize;
        let end = off.checked_add(meta.size_bytes as usize)?;
        let man = legaia_scene_formats::kingdom_bundle::decode_slot(disc.get(off..end)?, 2).ok()?;
        let file = legaia_scene_formats::man_section::parse(&man).ok()?;
        let body = file
            .sibling_sections()
            .get(FOG_REGION_SIBLING_SECTION)?
            .body(&man)?;
        Some(legaia_render_kernels::fog_volume::parse_fog_regions(body))
    })
}

/// Flatten regions to the export layout: per region `[enabled, x0, z0, x1,
/// z1, angle_base, angle_spread, speed, flag_index]`.
pub fn flatten_regions(regions: &[legaia_render_kernels::fog_volume::FogRegion]) -> Vec<u16> {
    regions
        .iter()
        .flat_map(|r| -> [u16; MIST_REGION_WORDS] {
            [
                u16::from(r.enabled),
                u16::from(r.x0),
                u16::from(r.z0),
                u16::from(r.x1),
                u16::from(r.z1),
                u16::from(r.angle_base),
                u16::from(r.angle_spread),
                u16::from(r.speed),
                r.flag_index,
            ]
        })
        .collect()
}

/// RGBA8 of the fog sheet cells, `64 x 48`, row 0 = `v 0x40`. A texel whose
/// CLUT word is `0x0000` is transparent (alpha 0); every other texel is
/// opaque (alpha 255) - the sheets draw additively, so the alpha only marks
/// which texels the GPU would blend at all.
pub fn sheet_rgba(vram: &legaia_tim::Vram) -> Vec<u8> {
    let mut out = Vec::with_capacity(SHEET_W * SHEET_H * 4);
    for row in 0..SHEET_H {
        let y = SHEET_V0 + row;
        for u in 0..SHEET_W {
            let word = vram.pixel(SHEET_PAGE_X + u / 4, y);
            let idx = usize::from((word >> ((u % 4) * 4)) & 0xF);
            let c = vram.pixel(idx, SHEET_CLUT_Y);
            let five = |s: u16| (((c >> s) & 0x1F) as u8) << 3 | (((c >> s) & 0x1F) as u8) >> 2;
            out.extend_from_slice(&[five(0), five(5), five(10), if c == 0 { 0 } else { 255 }]);
        }
    }
    out
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// The kingdom MAN's fog-region table (MAN section 4, `DAT_80073ED8`),
    /// [`MIST_REGION_WORDS`] `u16` per region in table order:
    /// `[enabled, x0, z0, x1, z1, angle_base, angle_spread, speed,
    /// flag_index]`. The box is open on all four sides in 128-unit tiles,
    /// and the spawner keeps the **first** region that holds a tile. Empty
    /// when the kingdom MAN does not decode.
    pub fn walk_mist_regions(&self, prot_base: u32) -> Vec<u16> {
        let Some(entries) = parse_prot_toc(&self.disc) else {
            return Vec::new();
        };
        kingdom_fog_regions(&self.disc, &entries, prot_base)
            .map(|r| flatten_regions(&r))
            .unwrap_or_default()
    }

    /// The fog sheet's art as RGBA8, `64 x 48` (`v 0x40..0x6F` of the PROT
    /// 0874 section-2 effect page at VRAM `(448, 0)` through CLUT `(0, 473)`).
    /// Rows `0..24` are the right half's cells (staged UV row 0), rows
    /// `24..48` the left half's (row 1). Empty when PROT 0874 does not parse.
    pub fn mist_sheet_rgba(&self) -> Vec<u8> {
        let Some(meta) = parse_prot_toc(&self.disc).and_then(|toc| {
            toc.into_iter()
                .find(|e| e.index == legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
        }) else {
            return Vec::new();
        };
        let off = meta.byte_offset as usize;
        let Some(raw) = self
            .disc
            .get(off..off.saturating_add(meta.size_bytes as usize))
        else {
            return Vec::new();
        };
        let Ok(pack) = legaia_actor_formats::field_char_textures::parse(raw) else {
            return Vec::new();
        };
        let mut vram = legaia_tim::Vram::new();
        pack.upload_to_vram(&mut vram, false);
        sheet_rgba(&vram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_flatten_in_table_order() {
        let body = [1u8, 1, 2, 3, 4, 5, 0x10, 0xFF, 0x20, 0, 0x1A, 0x05];
        let regs = legaia_render_kernels::fog_volume::parse_fog_regions(&body);
        let flat = flatten_regions(&regs);
        assert_eq!(flat, vec![1, 2, 3, 4, 5, 0x10, 0xFF, 0x20, 0x051A]);
        assert_eq!(flat.len(), MIST_REGION_WORDS);
    }

    /// Disc-gated: every kingdom carries a fog-region table and the sheet
    /// art decodes to non-empty cells.
    #[test]
    fn kingdom_fog_regions_decode_on_disc() {
        let Ok(path) = std::env::var("LEGAIA_DISC_BIN") else {
            return;
        };
        let Ok(bin) = std::fs::read(&path) else {
            return;
        };
        let Some(prot) = extract_prot_dat(&bin) else {
            return;
        };
        let entries = parse_prot_toc(&prot).expect("PROT TOC");
        for (base, min) in [(85u32, 10usize), (244, 8), (391, 9)] {
            let regs = kingdom_fog_regions(&prot, &entries, base).expect("fog regions");
            assert_eq!(regs.len(), min, "kingdom {base}");
            assert!(regs.iter().all(|r| r.x0 < r.x1 && r.z0 < r.z1));
        }
        let meta = entries
            .iter()
            .find(|e| e.index == legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
            .expect("PROT 0874");
        let raw = &prot[meta.byte_offset as usize..(meta.byte_offset + meta.size_bytes) as usize];
        let pack = legaia_actor_formats::field_char_textures::parse(raw).expect("0874 s2");
        let mut vram = legaia_tim::Vram::new();
        pack.upload_to_vram(&mut vram, false);
        let rgba = sheet_rgba(&vram);
        assert_eq!(rgba.len(), SHEET_W * SHEET_H * 4);
        let lit = rgba
            .chunks(4)
            .filter(|p| p[3] != 0 && p[..3] != [0, 0, 0])
            .count();
        assert!(lit > 200, "fog cells mostly empty: {lit}");
    }
}
