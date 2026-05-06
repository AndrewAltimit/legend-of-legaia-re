//! Materialised scene resources — the runtime view that turns a [`Scene`]'s
//! raw entry bytes into a populated PSX VRAM and a parsed TMD pool, without
//! the legacy `tim_scan/<entry>/` filesystem intermediate the asset-viewer
//! used.
//!
//! This is the engine-side mirror of the retail field-loader chain
//! ([`docs/subsystems/asset-loader.md`]): the runtime DMAs every TIM in the
//! scene's CDNAME block into VRAM up front (so cross-entry CLUT references
//! resolve), then the renderer walks each TMD with its CBA / TSB pointing
//! into the now-populated VRAM. This module performs the same bulk pre-pass
//! against bytes already in memory — so an engine binary can boot a scene
//! straight from `PROT.DAT` without any pre-extracted scan dirs.
//!
//! Build once per scene transition with [`SceneResources::build`]. Drop
//! when the scene changes.

use anyhow::Result;
use legaia_asset::{tim_scan, tmd_scan};
use legaia_tim::Vram;

use crate::scene::Scene;

/// One TMD model the scene exposes, paired with its parsed structure.
///
/// The retail asset chain registers each TMD into the per-scene mesh pointer
/// table at `0x8007C018 + idx*4` via `FUN_80026B4C`. Until the runtime
/// registration order is reverse-engineered, this module surfaces every TMD
/// hit in scene order (CDNAME entry order, then byte-offset within each
/// entry) — an engine can pick its meshes by `(entry_idx, offset)` and
/// pre-resolve the actor → TMD binding through a side table.
#[derive(Debug, Clone)]
pub struct ResolvedTmd {
    /// PROT entry index this TMD came from.
    pub entry_idx: u32,
    /// Byte offset of the TMD magic within the entry's bytes.
    pub offset: usize,
    /// Length of the TMD payload (bytes from magic to end of last primitive).
    pub byte_len: usize,
    /// Parsed TMD ready for primitive walking + render upload.
    pub tmd: legaia_tmd::Tmd,
}

/// Per-scene runtime resources: VRAM populated from every TIM in the
/// CDNAME block, plus a parsed TMD pool, plus a count of how many TIMs
/// fed VRAM. Owns its bytes — safe to hold across a subsequent scene
/// transition (the next [`SceneResources::build`] yields fresh state).
#[derive(Clone)]
pub struct SceneResources {
    /// Fully-populated PSX VRAM. Every TIM that the scene's CDNAME block
    /// carries (in any of its entries) has been DMA'd into the canonical
    /// `(fb_x, fb_y)` slot encoded in its TIM header — same model the PSX
    /// runtime uses, so cross-entry CLUT references resolve naturally.
    pub vram: Vram,
    /// Number of TIMs uploaded to [`SceneResources::vram`]. Useful for
    /// HUD / log output.
    pub tim_count: usize,
    /// Number of TIMs that failed to parse (typically zero — the TIM scanner
    /// is conservative). When non-zero, indicates the entry bytes carry a
    /// header magic that passes the cheap scan but fails the structural
    /// parse.
    pub tim_parse_failures: usize,
    /// Parsed TMDs — every TMD the scanner found across the scene's entries.
    /// The order is CDNAME-entry order, then byte-offset within each entry.
    pub tmds: Vec<ResolvedTmd>,
}

impl SceneResources {
    /// Build a fresh resource set from a loaded [`Scene`]. Sweeps every
    /// entry's bytes once, runs the TIM and TMD scanners, populates VRAM
    /// for every TIM that parses, and parses every TMD that the scanner
    /// hits.
    ///
    /// The TIM and TMD scanners are conservative — they validate header
    /// shape before reporting a hit — so spurious parse failures are
    /// rare. When they happen the count is exposed via
    /// [`SceneResources::tim_parse_failures`] for diagnostic logging.
    pub fn build(scene: &Scene) -> Result<Self> {
        let mut vram = Vram::new();
        let mut tim_count = 0usize;
        let mut tim_parse_failures = 0usize;
        let mut tmds = Vec::new();

        for entry in &scene.entries {
            let bytes: &[u8] = &entry.bytes;
            for hit in tim_scan::scan_buffer(bytes) {
                let payload = &bytes[hit.offset..hit.offset + hit.byte_len];
                match legaia_tim::parse(payload) {
                    Ok(tim) => {
                        vram.upload_tim(&tim);
                        tim_count += 1;
                    }
                    Err(_) => tim_parse_failures += 1,
                }
            }
            for hit in tmd_scan::scan_buffer(bytes) {
                let payload = &bytes[hit.offset..hit.offset + hit.byte_len];
                if let Ok(tmd) = legaia_tmd::parse(payload) {
                    tmds.push(ResolvedTmd {
                        entry_idx: entry.idx,
                        offset: hit.offset,
                        byte_len: hit.byte_len,
                        tmd,
                    });
                }
            }
        }

        Ok(Self {
            vram,
            tim_count,
            tim_parse_failures,
            tmds,
        })
    }

    /// Look up a TMD by its `(entry_idx, offset)` coordinates. Useful when
    /// an actor is bound to a specific TMD slot and the engine needs to
    /// fetch it from the resource pool.
    pub fn tmd_by_coords(&self, entry_idx: u32, offset: usize) -> Option<&ResolvedTmd> {
        self.tmds
            .iter()
            .find(|t| t.entry_idx == entry_idx && t.offset == offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneEntry;
    use legaia_asset::categorize::Class;
    use std::sync::Arc;

    /// Synthetic 16bpp TIM at `(fb_x, fb_y)` (4 px × 1 row, no CLUT).
    /// `bs_len` = 12 (block header) + 4 * 1 * 2 (pixel data) = 20.
    fn synth_tim_16bpp(fb_x: u16, fb_y: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
        buf.extend_from_slice(&20u32.to_le_bytes()); // bs_len
        buf.extend_from_slice(&fb_x.to_le_bytes());
        buf.extend_from_slice(&fb_y.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes()); // fb_w
        buf.extend_from_slice(&1u16.to_le_bytes()); // h
        for px in [0xAAAAu16, 0x5555, 0xF00F, 0x0FF0] {
            buf.extend_from_slice(&px.to_le_bytes());
        }
        buf
    }

    fn make_scene(entries: Vec<SceneEntry>) -> Scene {
        Scene {
            name: "test".into(),
            start: 0,
            end: entries.len() as u32,
            entries,
        }
    }

    #[test]
    fn build_uploads_tim_into_vram() {
        let scene = make_scene(vec![SceneEntry {
            idx: 0,
            class: Class::UnknownOther,
            bytes: Arc::new(synth_tim_16bpp(64, 64)),
        }]);
        let res = SceneResources::build(&scene).unwrap();
        assert_eq!(res.tim_count, 1);
        assert_eq!(res.tim_parse_failures, 0);
        // The 4 image pixels should be visible in VRAM at (64..68, 64).
        assert_eq!(res.vram.pixel(64, 64), 0xAAAA);
        assert_eq!(res.vram.pixel(65, 64), 0x5555);
        assert_eq!(res.vram.pixel(66, 64), 0xF00F);
        assert_eq!(res.vram.pixel(67, 64), 0x0FF0);
    }

    #[test]
    fn build_handles_empty_scene() {
        let scene = make_scene(vec![]);
        let res = SceneResources::build(&scene).unwrap();
        assert_eq!(res.tim_count, 0);
        assert_eq!(res.tim_parse_failures, 0);
        assert!(res.tmds.is_empty());
    }

    #[test]
    fn build_uploads_multiple_tims_at_distinct_slots() {
        let scene = make_scene(vec![
            SceneEntry {
                idx: 0,
                class: Class::UnknownOther,
                bytes: Arc::new(synth_tim_16bpp(0, 0)),
            },
            SceneEntry {
                idx: 1,
                class: Class::UnknownOther,
                bytes: Arc::new(synth_tim_16bpp(128, 128)),
            },
        ]);
        let res = SceneResources::build(&scene).unwrap();
        assert_eq!(res.tim_count, 2);
        assert_eq!(res.vram.pixel(0, 0), 0xAAAA);
        assert_eq!(res.vram.pixel(128, 128), 0xAAAA);
    }
}
