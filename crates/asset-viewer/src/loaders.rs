//! TIM / VAB byte loaders shared by the single-file modes and the PROT
//! browser.

use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn load_tim(bytes: &[u8], clut_idx: usize) -> Result<(Vec<u8>, u32, u32)> {
    let tim = legaia_tim::parse(bytes).context("parse TIM")?;
    let rgba = legaia_tim::decode_rgba8(&tim, clut_idx).context("decode TIM to RGBA")?;
    Ok((rgba, tim.pixel_width() as u32, tim.pixel_height() as u32))
}

pub(crate) fn load_tim_path(path: &Path, clut_idx: usize) -> Result<(Vec<u8>, u32, u32)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    load_tim(&bytes, clut_idx)
}

/// Same as [`load_tim_path`] but reads a TIM at a non-zero byte
/// offset within `path`. Useful for parsing TIMs embedded in larger
/// containers (PROT entries) and for TIMs in the unindexed pre-
/// `init_data` gap of `PROT.DAT` (e.g. the system-UI sprite sheet at
/// offset `0x018E0` or the menu-glyph atlas at offset `0x11218` -
/// see [`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET`]).
pub(crate) fn load_tim_path_at_offset(
    path: &Path,
    offset: u64,
    clut_idx: usize,
) -> Result<(Vec<u8>, u32, u32)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let off = offset as usize;
    if off >= bytes.len() {
        anyhow::bail!(
            "offset 0x{:X} past end of {} ({} bytes)",
            off,
            path.display(),
            bytes.len()
        );
    }
    load_tim(&bytes[off..], clut_idx)
}

/// Decode the save-slot portrait sheet out of a PROT entry 899 image and lay
/// it out for display: every tile rendered against **its own** palette, which
/// is the whole point - the plain TIM viewer would paint all sixteen tiles
/// through one CLUT and show fifteen wrong-coloured portraits.
///
/// `tile = Some(n)` shows a single tile; `None` shows the whole strip with a
/// one-pixel separator between tiles. `scale` is an integer nearest-neighbour
/// zoom (16x16 tiles are unreadable at 1:1).
pub(crate) fn load_save_icon_sheet(
    path: &Path,
    tile: Option<usize>,
    scale: u32,
) -> Result<(Vec<u8>, u32, u32)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let sheet = legaia_asset::save_icon::parse_entry(&bytes)
        .with_context(|| format!("locate the save-icon sheet in {}", path.display()))?;

    let scale = scale.max(1);
    let size = legaia_asset::save_icon::TILE_SIZE as u32;
    let tiles: Vec<usize> = match tile {
        Some(t) => vec![t],
        None => (0..legaia_asset::save_icon::TILE_COUNT).collect(),
    };
    let gap = if tiles.len() > 1 { 1u32 } else { 0 };
    let cell = size * scale + gap;
    let w = cell * tiles.len() as u32 - gap;
    let h = size * scale;
    // Separator colour: a mid grey that neither a portrait nor the blank tile
    // can be confused with.
    let mut out = vec![0x60u8; (w * h * 4) as usize];
    for (col, &t) in tiles.iter().enumerate() {
        let rgba = sheet
            .tile_rgba(t)
            .with_context(|| format!("decode save-icon tile {t}"))?;
        let x0 = col as u32 * cell;
        for y in 0..h {
            let sy = (y / scale) as usize;
            for x in 0..size * scale {
                let sx = (x / scale) as usize;
                let src = (sy * size as usize + sx) * 4;
                let dst = (((y * w) + x0 + x) * 4) as usize;
                out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
            }
        }
    }
    Ok((out, w, h))
}

/// Decode VAG sample `idx` from a VAB header located at `offset` in `path`.
/// Returns mono i16 PCM.
pub(crate) fn load_vab_sample(path: &Path, offset: usize, idx: usize) -> Result<Vec<i16>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    decode_vab_sample(&bytes, offset, idx)
}

pub(crate) fn decode_vab_sample(bytes: &[u8], offset: usize, idx: usize) -> Result<Vec<i16>> {
    let report = legaia_vab::parse(bytes, offset).context("parse VAB")?;
    let span = report
        .vag_samples
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("VAB has only {} samples", report.vag_samples.len()))?;
    let body = &bytes[span.byte_offset..span.byte_offset + span.size];
    legaia_vab::decode_vag(body).context("decode VAG body")
}
