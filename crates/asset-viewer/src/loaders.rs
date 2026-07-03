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
