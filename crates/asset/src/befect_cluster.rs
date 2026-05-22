//! Cluster-aware extractor for the battle-effect `befect_data` cluster.
//!
//! The per-entry PROT extractor over-reads this cluster: its entries overlap
//! on disc - each starts only a few sectors into the previous entry's extended
//! footprint, so the naive per-entry `.BIN` files bleed into their neighbours
//! (e.g. `0873_befect_data.BIN` at offset `0x2000` is byte-identical to the
//! start of `0874_befect_data.BIN`). This module slices each cluster entry at
//! its true *footprint* (`next_lba - this_lba`), expands the LZS-container
//! entry into its sub-files, and classifies every resulting blob by content
//! signature.
//!
//! The cluster (CDNAME `befect_data`, four entries ending at `player_data`)
//! resolves to:
//!   - a generic offset pack (effect billboard geometry),
//!   - the effect-script 2-pack `efect.dat` (inline sprite atlas + pack0 anim
//!     batches + pack1 effect scripts) - the first ~`0x2000` of "entry 873",
//!   - an LZS container of three sub-files: effect 3D models (Legaia TMDs), a
//!     generic offset pack, and the effect-texture TIMs (CLUTs in the high
//!     VRAM rows 475..478, pixels at fb_y=256),
//!   - a trailing data blob.
//!
//! See [`docs/formats/effect.md`](../../../docs/formats/effect.md).

use anyhow::{Result, anyhow};
use legaia_prot::archive::{Archive, SECTOR};
use legaia_prot::cdname::{self, IndexMap};
use serde::Serialize;

const CLUSTER_SYMBOL: &str = "befect_data";
const TMD_MAGIC: u32 = 0x8000_0002;
const TIM_MAGIC: u32 = 0x0000_0010;

/// One embedded PSX TIM found inside a cluster part, with its VRAM target.
#[derive(Debug, Clone, Serialize)]
pub struct TimTarget {
    /// Byte offset of the TIM header within the part.
    pub offset: usize,
    /// Bits-per-pixel (4, 8, 15, 24).
    pub bpp: u8,
    /// Pixel-block framebuffer destination (VRAM halfword coords).
    pub fb_x: u16,
    pub fb_y: u16,
    /// Pixel-block width in halfwords and height in rows.
    pub w_hw: u16,
    pub h: u16,
    /// CLUT framebuffer destination, if the TIM carries one.
    pub clut_fb: Option<(u16, u16)>,
}

/// Content classification of one cluster part.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum Component {
    /// The `efect.dat` runtime 2-pack: inline sprite atlas, pack0 animation
    /// batches, pack1 effect scripts.
    EffectScript2Pack {
        atlas_entries: usize,
        anim_batches: usize,
        scripts: usize,
    },
    /// A pack carrying Legaia TMDs (magic `0x80000002`) - effect 3D models.
    TmdPack { count: usize },
    /// Carries one or more embedded PSX TIMs - effect texture pages.
    TimImages { tims: Vec<TimTarget> },
    /// A generic `u32 count + u32 offset[count]` pack with no recognised
    /// sub-asset magic.
    OffsetPack { count: usize },
    /// Unclassified bytes.
    Raw,
}

/// One extracted part of the cluster: either a whole footprint-bounded entry
/// or a single section of the LZS-container entry.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterPart {
    pub prot_index: u32,
    /// `Some(i)` when this part is section `i` of an LZS-container entry;
    /// `None` for a whole footprint-bounded entry.
    pub lzs_section: Option<usize>,
    pub len: usize,
    pub component: Component,
    /// The part's bytes (footprint slice, or decompressed section).
    #[serde(skip)]
    pub data: Vec<u8>,
}

/// The fully-extracted cluster.
#[derive(Debug, Default, Serialize)]
pub struct BefectCluster {
    pub first_index: u32,
    pub parts: Vec<ClusterPart>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

/// `count` of `0x80000002` TMD magics in the first `scan` bytes.
fn tmd_magic_count(b: &[u8]) -> usize {
    (0..b.len().saturating_sub(3))
        .step_by(4)
        .filter(|&o| u32_at(b, o) == Some(TMD_MAGIC))
        .count()
}

/// Scan a blob for embedded PSX TIMs with sane VRAM targets.
fn scan_tims(b: &[u8]) -> Vec<TimTarget> {
    let mut out = Vec::new();
    let mut o = 0usize;
    while o + 8 <= b.len() {
        // Reject parses with implausible VRAM coords (the magic word recurs
        // in pixel data, so guard tightly).
        if u32_at(b, o) == Some(TIM_MAGIC)
            && let Ok(tim) = legaia_tim::parse(&b[o..])
            && tim.image.fb_x < legaia_tim::VRAM_WIDTH as u16
            && tim.image.fb_y < legaia_tim::VRAM_HEIGHT as u16
            && tim.image.fb_w > 0
            && tim.image.h > 0
        {
            let img = &tim.image;
            let bpp = match tim.mode {
                legaia_tim::PixelMode::Bpp4 => 4,
                legaia_tim::PixelMode::Bpp8 => 8,
                legaia_tim::PixelMode::Bpp16 => 16,
                legaia_tim::PixelMode::Bpp24 => 24,
                legaia_tim::PixelMode::Mixed => 0,
            };
            out.push(TimTarget {
                offset: o,
                bpp,
                fb_x: img.fb_x,
                fb_y: img.fb_y,
                w_hw: img.fb_w,
                h: img.h,
                clut_fb: tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y)),
            });
            // Skip past this TIM's pixel block to avoid re-matching the magic
            // word inside its data.
            o += (img.fb_w as usize * img.h as usize * 2).max(4) + 0x14;
            continue;
        }
        o += 4;
    }
    out
}

/// `efect.dat` 2-pack: leading `pack0_offset`, `pack1_offset`, then `(p0-8)/8`
/// inline 8-byte atlas entries; `pack0`/`pack1` are `count + offsets[]` packs.
fn as_2pack(b: &[u8]) -> Option<Component> {
    let p0 = u32_at(b, 0)? as usize;
    let p1 = u32_at(b, 4)? as usize;
    if !(8..p1).contains(&p0) || p1 >= b.len() || !(p0 - 8).is_multiple_of(8) {
        return None;
    }
    let anim_batches = u32_at(b, p0)? as usize;
    let scripts = u32_at(b, p1)? as usize;
    // A real 2-pack has small pack counts; reject if these read as garbage.
    if anim_batches == 0 || anim_batches > 256 || scripts == 0 || scripts > 256 {
        return None;
    }
    Some(Component::EffectScript2Pack {
        atlas_entries: (p0 - 8) / 8,
        anim_batches,
        scripts,
    })
}

/// Generic `u32 count + u32 offset[count]` pack with ascending in-bounds
/// offsets.
fn as_offset_pack(b: &[u8]) -> Option<usize> {
    let count = u32_at(b, 0)? as usize;
    if count == 0 || count > 256 || 4 + count * 4 > b.len() {
        return None;
    }
    let mut prev = 0u32;
    for i in 0..count {
        let off = u32_at(b, 4 + i * 4)?;
        if off as usize >= b.len() || off < prev {
            return None;
        }
        prev = off;
    }
    Some(count)
}

fn classify(b: &[u8]) -> Component {
    if let Some(c) = as_2pack(b) {
        return c;
    }
    let tmds = tmd_magic_count(b);
    if tmds > 0 {
        return Component::TmdPack { count: tmds };
    }
    let tims = scan_tims(b);
    if !tims.is_empty() {
        return Component::TimImages { tims };
    }
    if let Some(count) = as_offset_pack(b) {
        return Component::OffsetPack { count };
    }
    Component::Raw
}

/// Extract the `befect_data` cluster from an open PROT archive, using its
/// CDNAME symbol map to locate the entry range. Each entry is read at its
/// footprint size (`next_lba - this_lba`), not the extended/indexed size; the
/// one LZS-container entry is expanded into its sections; every part is
/// classified by content signature.
pub fn extract(archive: &mut Archive, cdname: &IndexMap) -> Result<BefectCluster> {
    let (start, end) = cdname::block_range_for_name(cdname, CLUSTER_SYMBOL)
        .ok_or_else(|| anyhow!("CDNAME has no `{CLUSTER_SYMBOL}` symbol"))?;
    let mut cluster = BefectCluster {
        first_index: start,
        parts: Vec::new(),
    };
    for idx in start..end {
        let entry = archive
            .entries
            .get(idx as usize)
            .ok_or_else(|| anyhow!("PROT entry {idx} out of range"))?
            .clone();
        // True per-file size is the footprint to the next entry; the indexed /
        // extended size over-reads this cluster.
        let footprint_sectors = match archive.entries.get(idx as usize + 1) {
            Some(next) => next.start_lba.saturating_sub(entry.start_lba),
            None => entry.size_sectors,
        };
        let len = footprint_sectors as usize * SECTOR as usize;
        let mut buf = Vec::with_capacity(len);
        archive.read_raw(entry.byte_offset, len, &mut buf)?;

        // LZS container? Use the strict decoder (rejects offset-pack false
        // positives) and require a section to magic-check, so a flat pack
        // never decompresses to synthesised garbage.
        if let Ok(sections) = legaia_lzs::decompress_container_strict(&buf) {
            let real = sections
                .iter()
                .any(|s| tmd_magic_count(s) > 0 || !scan_tims(s).is_empty());
            if real {
                for (i, sec) in sections.into_iter().enumerate() {
                    let component = classify(&sec);
                    cluster.parts.push(ClusterPart {
                        prot_index: idx,
                        lzs_section: Some(i),
                        len: sec.len(),
                        component,
                        data: sec,
                    });
                }
                continue;
            }
        }

        let component = classify(&buf);
        cluster.parts.push(ClusterPart {
            prot_index: idx,
            lzs_section: None,
            len: buf.len(),
            component,
            data: buf,
        });
    }
    Ok(cluster)
}
