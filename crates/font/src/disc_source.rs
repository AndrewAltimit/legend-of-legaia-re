//! The disc-side font sources: the 4bpp font TIM decode and stencil pack,
//! and the width / escape tables read out of `SCUS_942.54`.

use super::*;

/// SCUS RAM address of the 256-byte per-character advance table.
pub(super) const WIDTH_TABLE_RAM: u32 = 0x8007_3F1C;
/// PSX-EXE `t_addr` header field offset + header size + the default load
/// address used when the executable header is absent/unparseable.
pub(super) const PSX_EXE_T_ADDR_OFFSET: usize = 0x18;
pub(super) const PSX_EXE_HEADER: u32 = 0x800;
pub(super) const SCUS_LOAD_ADDR_FALLBACK: u32 = 0x8001_0000;
/// Font-page CLUT indices: 0 = transparent background, 14 = drop-shadow,
/// everything else = glyph fill (whitewashed to pure white so the engine's
/// `texel.rgb * tint` shader can reach any retail ink colour).
pub(super) const FONT_SHADOW_INDEX: u8 = 14;

/// The drop-shadow texel's grey, stored against the ink retail draws text in.
///
/// Retail's shadow is a fixed CLUT entry, the 5-bit `4` a frame shows as
/// `(33, 33, 33)`, whatever the fill ink; the engine's shader multiplies the
/// whole texel by the ink. Retail's text ink is the tier-7 grey `206` almost
/// everywhere (dialogue, menus, every battle caption), so the shadow texel is
/// stored as `33 * 255 / 206`: under that ink it lands on retail's `33`. A
/// stored `32` drew every shadow at `26`.
pub const FONT_SHADOW_TEXEL: u8 = 41;

/// The alpha a drop-shadow texel carries instead of `255`: the mark the
/// overlay tint reads to give a shadow its own ink's CLUT entry rather than a
/// multiply of the fill ink (`legaia_render_kernels::ui_tint`). A tint that
/// reads no mark draws it as an opaque [`FONT_SHADOW_TEXEL`] like before.
pub const FONT_SHADOW_ALPHA: u8 = 254;

/// Decode the 4bpp dialog-font TIM (`font_tim` starting at the TIM magic) into
/// a 256×256 row-major indexed buffer (one byte per pixel, 0..=15). Validates
/// the header + the framebuffer `(896, 0)` so a wrong slice fails loudly rather
/// than producing garbage glyphs.
pub(super) fn decode_font_tim(font_tim: &[u8]) -> Result<Vec<u8>> {
    let rd_u32 = |o: usize| -> Result<u32> {
        font_tim
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    let rd_u16 = |o: usize| -> Result<u16> {
        font_tim
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd_u32(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd_u32(4)?;
    let bpp = flags & 0x7;
    if bpp != 0 {
        bail!("font TIM is not 4bpp (bpp mode {bpp})");
    }
    let mut p = 8usize;
    // Skip the CLUT block if present (bit 3): `[u32 block_len][block_len bytes]`.
    if flags & 0x8 != 0 {
        let clut_len = rd_u32(p)? as usize;
        p = p
            .checked_add(clut_len)
            .filter(|&e| e <= font_tim.len())
            .ok_or_else(|| anyhow::anyhow!("font TIM CLUT block overruns"))?;
    }
    // Image block: `[u32 len][u16 dx][u16 dy][u16 w16][u16 h][pixels]`.
    let _img_len = rd_u32(p)?;
    let (dx, dy, w16, h) = (
        rd_u16(p + 4)?,
        rd_u16(p + 6)?,
        rd_u16(p + 8)?,
        rd_u16(p + 10)?,
    );
    if (dx, dy) != (896, 0) || w16 != 64 || h != 256 {
        bail!("font TIM framebuffer ({dx},{dy}) {w16}x{h} != expected (896,0) 64x256");
    }
    let pixels = &font_tim[p + 12..];
    let stride = (w16 as usize) * 2; // bytes per row (2 px per byte)
    let width = w16 as usize * 4; // = 256 pixels
    let height = h as usize;
    let need = stride * height;
    if pixels.len() < need {
        bail!("font TIM pixel data truncated ({} < {need})", pixels.len());
    }
    let mut indexed = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let byte = pixels[y * stride + x / 2];
            // Low nibble = even pixel, high nibble = odd (PSX 4bpp order).
            indexed[y * width + x] = if x & 1 == 0 { byte & 0xF } else { byte >> 4 };
        }
    }
    Ok(indexed)
}

/// Pack the 256×256 indexed font page into the 224×210 (16×14 cells of 14×15)
/// atlas [`Font`] expects, as a whitewashed stencil: cell `c` lives in the page
/// at `U = (c & 0x0F) * 16`, `V = (c & 0xF0) - 0x20` (`docs/formats/dialog-font.md`),
/// and its upper-left 14×15 pixels copy to `glyph_origin(c)`. Index 0 → fully
/// transparent, [`FONT_SHADOW_INDEX`] → the [`FONT_SHADOW_TEXEL`] drop shadow, every
/// other index → pure white fill.
pub(super) fn pack_stencil_atlas(indexed: &[u8]) -> Vec<u8> {
    const PAGE: usize = 256;
    let aw = (COLS * GLYPH_W) as usize;
    let ah = (ROWS * GLYPH_H) as usize;
    let mut atlas = vec![0u8; aw * ah * 4];
    for c in FIRST_CHAR..=0xFFu8 {
        let Some((ox, oy)) = Font::glyph_origin(c) else {
            continue;
        };
        let src_u = ((c & 0x0F) as usize) * 16;
        let src_v = ((c as usize) & 0xF0).wrapping_sub(0x20);
        if src_v + GLYPH_H as usize > PAGE {
            continue;
        }
        for gy in 0..GLYPH_H as usize {
            for gx in 0..GLYPH_W as usize {
                let idx = indexed[(src_v + gy) * PAGE + (src_u + gx)];
                let px = match idx {
                    0 => [0, 0, 0, 0],
                    FONT_SHADOW_INDEX => [
                        FONT_SHADOW_TEXEL,
                        FONT_SHADOW_TEXEL,
                        FONT_SHADOW_TEXEL,
                        FONT_SHADOW_ALPHA,
                    ],
                    _ => [255, 255, 255, 255],
                };
                let dx = ox as usize + gx;
                let dy = oy as usize + gy;
                let off = (dy * aw + dx) * 4;
                atlas[off..off + 4].copy_from_slice(&px);
            }
        }
    }
    atlas
}

/// Read the 256-byte per-character advance table from `SCUS_942.54` (RAM
/// [`WIDTH_TABLE_RAM`]). Resolves the file offset through the PSX-EXE `t_addr`
/// header, falling back to the retail load address when the header is absent.
pub(super) fn read_scus_widths(scus: &[u8]) -> Result<[u8; 256]> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(
            scus[PSX_EXE_T_ADDR_OFFSET..PSX_EXE_T_ADDR_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    } else {
        SCUS_LOAD_ADDR_FALLBACK
    };
    let ram_off = WIDTH_TABLE_RAM
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow::anyhow!("width table below t_addr"))?;
    let file_off = ram_off
        .checked_add(PSX_EXE_HEADER)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow::anyhow!("width table offset overflow"))?;
    let slice = scus
        .get(file_off..file_off + 256)
        .ok_or_else(|| anyhow::anyhow!("width table past SCUS end"))?;
    let mut widths = [0u8; 256];
    widths.copy_from_slice(slice);
    Ok(widths)
}

/// SCUS RAM address of the 38-entry runtime escape table (the `0xCE`
/// substitution table [`EscapeTable`] models). Same table `font-extract`
/// reads from a save state's companion SCUS; here it feeds the disc-only
/// export of `dialog_font_metadata.json`.
pub(super) const ESCAPE_TABLE_RAM: u32 = 0x8007_4050;
/// Entry count of the escape table (4 bytes each).
pub(super) const ESCAPE_TABLE_ENTRIES: usize = 38;

/// Resolve an SCUS RAM address to a file slice through the PSX-EXE `t_addr`
/// header (falling back to the retail load address when the header is
/// absent), mirroring [`read_scus_widths`] for arbitrary tables.
pub(super) fn read_scus_ram(scus: &[u8], ram_addr: u32, len: usize) -> Result<&[u8]> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(
            scus[PSX_EXE_T_ADDR_OFFSET..PSX_EXE_T_ADDR_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    } else {
        SCUS_LOAD_ADDR_FALLBACK
    };
    let ram_off = ram_addr
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} below t_addr 0x{t_addr:08X}"))?;
    let file_off = ram_off
        .checked_add(PSX_EXE_HEADER)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} offset overflows"))?;
    scus.get(file_off..file_off + len)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} + {len} past SCUS end"))
}

/// Walk the dialog-font TIM header and return `(clut, pixel_bytes)`: the
/// TIM's own 16-entry BGR555 CLUT and the raw 4bpp page bytes (32768 bytes =
/// 256x256 pixels, two per byte). The pixel bytes are exactly what the
/// runtime uploads to VRAM `(896, 0)`, so they byte-match the
/// `dialog_font_vram_4bpp.bin` a save-state extraction produces.
pub(super) fn font_tim_clut_and_pixels(font_tim: &[u8]) -> Result<([u16; 16], &[u8])> {
    let rd_u32 = |o: usize| -> Result<u32> {
        font_tim
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd_u32(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd_u32(4)?;
    if flags & 0x7 != 0 {
        bail!("font TIM is not 4bpp");
    }
    let mut clut = [0u16; 16];
    let mut p = 8usize;
    if flags & 0x8 != 0 {
        let clut_len = rd_u32(p)? as usize;
        // Entries start after len(4) + destination rect(8).
        for (i, slot) in clut.iter_mut().enumerate() {
            let off = p + 12 + i * 2;
            *slot = font_tim
                .get(off..off + 2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .unwrap_or(0);
        }
        p = p
            .checked_add(clut_len)
            .filter(|&e| e <= font_tim.len())
            .ok_or_else(|| anyhow::anyhow!("font TIM CLUT block overruns"))?;
    }
    // Image block: `[u32 len][u16 dx][u16 dy][u16 w16][u16 h][pixels]`.
    let pixels = font_tim
        .get(p + 12..p + 12 + 256 * 128)
        .ok_or_else(|| anyhow::anyhow!("font TIM pixel data truncated"))?;
    Ok((clut, pixels))
}

/// Stencil colour for a 4bpp palette index: transparent background, the
/// [`FONT_SHADOW_TEXEL`] drop shadow, or pure-white fill (see [`pack_stencil_atlas`]).
pub(super) fn stencil_rgba(idx: u8) -> [u8; 4] {
    match idx {
        0 => [0, 0, 0, 0],
        FONT_SHADOW_INDEX => [
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_TEXEL,
            FONT_SHADOW_ALPHA,
        ],
        _ => [255, 255, 255, 255],
    }
}
