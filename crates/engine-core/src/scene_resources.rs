//! Materialised scene resources - the runtime view that turns a [`Scene`]'s
//! raw entry bytes into a populated PSX VRAM and a parsed TMD pool, without
//! the legacy `tim_scan/<entry>/` filesystem intermediate the asset-viewer
//! used.
//!
//! This is the engine-side mirror of the retail field-loader chain
//! ([`docs/subsystems/asset-loader.md`]): the runtime DMAs every TIM in the
//! scene's CDNAME block into VRAM up front (so cross-entry CLUT references
//! resolve), then the renderer walks each TMD with its CBA / TSB pointing
//! into the now-populated VRAM. This module performs the same bulk pre-pass
//! against bytes already in memory - so an engine binary can boot a scene
//! straight from `PROT.DAT` without any pre-extracted scan dirs.
//!
//! Build once per scene transition with [`SceneResources::build`]. Drop
//! when the scene changes.
//!
//! ## Shared-block overlay
//!
//! The retail field / town engine doesn't only load the scene's own
//! CDNAME block - it also keeps a small number of shared blocks resident
//! in VRAM across every field transition. The two confirmed examples are
//! `init_data` (PROT 0; shared UI/sprite atlas slots) and `player_data`
//! (PROT 876; the player character TMD + 256x256 atlas at VRAM
//! `fb=(768, 0)`). These survive scene transitions because they live in
//! VRAM slots the per-scene loader never touches.
//!
//! Callers that want this engine-side parity drive
//! [`SceneResources::build_with_shared`] and pass [`FIELD_SHARED_BLOCKS`]
//! as the shared scene set. The default zero-arg [`SceneResources::build`]
//! stays backward-compatible.

use anyhow::Result;
use legaia_asset::{anm_detect, tim_scan, tmd_scan};
use legaia_tim::Vram;

use crate::scene::Scene;

/// One TMD model the scene exposes, paired with its parsed structure.
///
/// The retail asset chain registers each TMD into the per-scene mesh pointer
/// table at `0x8007C018 + idx*4` via `FUN_80026B4C`. Until the runtime
/// registration order is reverse-engineered, this module surfaces every TMD
/// hit in scene order (CDNAME entry order, then byte-offset within each
/// entry) - an engine can pick its meshes by `(entry_idx, offset)` and
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
    /// Raw bytes of the TMD slice (same buffer passed to [`legaia_tmd::parse`]).
    /// Required by [`legaia_tmd::mesh::tmd_to_vram_mesh`] for primitive data reads.
    pub raw: Vec<u8>,
}

impl ResolvedTmd {
    /// Build a renderable VRAM mesh, dropping primitives whose CBA / TSB
    /// would sample VRAM regions the scene's TIM uploads didn't populate.
    /// Mirrors the asset-viewer's filter so engine-side scene rendering
    /// inherits the same "skip prims that would render as flat green" /
    /// "skip prims with palette-depth mismatches" cleanup.
    ///
    /// Use [`legaia_tmd::mesh::tmd_to_vram_mesh`] directly instead when
    /// the caller wants every prim regardless of VRAM state (e.g. the
    /// flat-shaded pre-VRAM path).
    pub fn build_filtered_vram_mesh(&self, vram: &Vram) -> legaia_tmd::mesh::VramMesh {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&self.tmd, &self.raw, |cba, tsb, uvs| {
            vram.prim_has_texture_data(cba, tsb, uvs)
        })
    }

    /// Same as [`ResolvedTmd::build_filtered_vram_mesh`] but also
    /// returns [`legaia_tmd::mesh::FilterStats`] so callers can report
    /// how many prims fell through the VRAM-coverage filter. Engines
    /// use this for "town01 mesh K: 92% prims kept" diagnostics.
    pub fn build_filtered_vram_mesh_stats(
        &self,
        vram: &Vram,
    ) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::FilterStats) {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered_stats(&self.tmd, &self.raw, |cba, tsb, uvs| {
            vram.prim_has_texture_data(cba, tsb, uvs)
        })
    }

    /// Same shape as [`ResolvedTmd::build_filtered_vram_mesh_stats`] but
    /// surfaces *why* each dropped prim was rejected (MissingClut /
    /// DepthMismatch / MissingTexturePage) via
    /// [`legaia_tmd::mesh::FilterStatsByReason`]. Useful for engine
    /// diagnostics that want to distinguish "the loader skipped the
    /// texture page" from "two TIMs collided on the same CLUT row".
    pub fn build_filtered_vram_mesh_reasoned(
        &self,
        vram: &Vram,
    ) -> (
        legaia_tmd::mesh::VramMesh,
        legaia_tmd::mesh::FilterStatsByReason,
    ) {
        legaia_tmd::mesh::tmd_to_vram_mesh_status_stats(&self.tmd, &self.raw, |cba, tsb, uvs| {
            use legaia_tim::vram::PrimTextureStatus;
            use legaia_tmd::mesh::PrimDecision;
            match vram.prim_texture_status(cba, tsb, uvs) {
                PrimTextureStatus::Ok => PrimDecision::Keep,
                PrimTextureStatus::MissingClut { .. } => PrimDecision::MissingClut,
                PrimTextureStatus::ClutDepthMismatch { .. } => PrimDecision::ClutDepthMismatch,
                PrimTextureStatus::MissingTexturePage { .. } => PrimDecision::MissingTexturePage,
            }
        })
    }
}

/// One ANM pack found in the scene's CDNAME block.
#[derive(Clone)]
pub struct ResolvedAnm {
    /// PROT entry index this ANM pack came from.
    pub entry_idx: u32,
    /// Byte offset of the ANM payload within the entry.
    pub offset: usize,
    /// Byte length of the ANM payload.
    pub byte_len: usize,
    /// Parsed ANM pack (count + record ranges).
    pub pack: legaia_anm::AnmPack,
    /// Raw payload bytes (no preamble). Index into this with
    /// `pack.records[i].offset .. pack.records[i].offset + pack.records[i].size`
    /// to get the record bytes for `AnimPlayer::new`.
    pub payload: Vec<u8>,
}

impl ResolvedAnm {
    /// Slice the record bytes for record `idx`. Returns `None` if out of range.
    pub fn record_bytes(&self, idx: usize) -> Option<&[u8]> {
        let rec = self.pack.records.get(idx)?;
        self.payload.get(rec.offset..rec.offset + rec.size)
    }
}

/// Default shared CDNAME blocks the retail field / town engine keeps
/// resident across scene transitions. Pair with
/// [`SceneResources::build_with_shared`] to match the runtime VRAM layout.
///
/// `init_data` (PROT 0) holds shared sprite / UI tiles at VRAM
/// `fb=(704, 0)` and `fb=(704, 256)`. `player_data` (PROT 876) holds
/// the player-character TMD + 256x256 atlas at VRAM `fb=(768, 0)` with
/// CLUT at `(0, 500)`.
///
/// The four town-NPC character TMDs that drop ~97 prims each on
/// town01 reference CLUT row y=479 slots at x=128..240 (CBAs
/// `0x77C8..0x77CF`). The 256x1 palette block that owns those slots
/// lives inside `battle_data` (PROT 865..869), packed inside a custom
/// container the raw TIM scanner doesn't descend into. The retail
/// engine reaches them through `FUN_8001E890`'s data-field-player
/// chain. Until that loader is ported, those rows stay unsupplied -
/// document the gap in `docs/subsystems/asset-loader.md` rather than
/// inflate `FIELD_SHARED_BLOCKS` with a block whose payload the
/// current scanner can't reach.
pub const FIELD_SHARED_BLOCKS: &[&str] = &["init_data", "player_data"];

/// Per-scene runtime resources: VRAM populated from every TIM in the
/// CDNAME block, plus a parsed TMD pool, plus a count of how many TIMs
/// fed VRAM. Owns its bytes - safe to hold across a subsequent scene
/// transition (the next [`SceneResources::build`] yields fresh state).
#[derive(Clone)]
pub struct SceneResources {
    /// Fully-populated PSX VRAM. Every TIM that the scene's CDNAME block
    /// carries (in any of its entries) has been DMA'd into the canonical
    /// `(fb_x, fb_y)` slot encoded in its TIM header - same model the PSX
    /// runtime uses, so cross-entry CLUT references resolve naturally.
    pub vram: Vram,
    /// Number of TIMs uploaded to [`SceneResources::vram`]. Useful for
    /// HUD / log output.
    pub tim_count: usize,
    /// Number of TIMs that failed to parse (typically zero - the TIM scanner
    /// is conservative). When non-zero, indicates the entry bytes carry a
    /// header magic that passes the cheap scan but fails the structural
    /// parse.
    pub tim_parse_failures: usize,
    /// Parsed TMDs - every TMD the scanner found across the scene's entries.
    /// The order is CDNAME-entry order, then byte-offset within each entry.
    pub tmds: Vec<ResolvedTmd>,
    /// Parsed ANM packs - every ANM container found across the scene's entries.
    /// The order is CDNAME-entry order, then byte-offset within each entry.
    pub anm_packs: Vec<ResolvedAnm>,
    /// Number of TIMs contributed by shared CDNAME blocks (e.g.
    /// `init_data`, `player_data`). Counted separately so the diagnostic
    /// "this scene has N scene-local TIMs and K shared TIMs" survives a
    /// single field. Zero when `build` was used (no shared overlay).
    pub shared_tim_count: usize,
    /// Number of TMDs contributed by shared CDNAME blocks. Same semantics
    /// as `shared_tim_count`.
    pub shared_tmd_count: usize,
}

impl SceneResources {
    /// Build a fresh resource set from a loaded [`Scene`]. Sweeps every
    /// entry's bytes once, runs the TIM and TMD scanners, populates VRAM
    /// for every TIM that parses, and parses every TMD that the scanner
    /// hits.
    ///
    /// The TIM and TMD scanners are conservative - they validate header
    /// shape before reporting a hit - so spurious parse failures are
    /// rare. When they happen the count is exposed via
    /// [`SceneResources::tim_parse_failures`] for diagnostic logging.
    pub fn build(scene: &Scene) -> Result<Self> {
        Self::build_with_shared(scene, &[])
    }

    /// Same as [`SceneResources::build_with_shared`] but uses the
    /// asset-viewer-style *targeted* VRAM-upload heuristic: every scene
    /// TMD's prim CBA/TSB/UV needs are collected first, then each TIM's
    /// image and CLUT blocks are uploaded *independently* based on
    /// whether they overlap with mesh-required regions and whether they
    /// would clobber some other mesh's data.
    ///
    /// Returns `None` for the upload stats when the scene has no
    /// textured prims (caller falls back to the unfiltered upload).
    /// This is the engine-side parity for what the retail field
    /// loader's asset-chain does: only DMA the TIM bytes the current
    /// frame's meshes need, leaving the rest of VRAM for other scene
    /// resources.
    ///
    /// See [`legaia_tmd::vram_targeted::build_vram_targeted_from_buffers`]
    /// for the per-TIM block-arbitration rule.
    pub fn build_targeted(
        scene: &Scene,
        shared_scenes: &[&Scene],
    ) -> Result<(Self, legaia_tmd::vram_targeted::VramUploadStats)> {
        let mut tmds = Vec::new();
        let mut anm_packs = Vec::new();
        let mut shared_tmd_count = 0usize;

        // Pass 1: parse every TMD and ANM from shared + scene so we know
        // exactly what VRAM regions the meshes will sample.
        let parse_scene_tmds_anms = |s: &Scene,
                                     tmds: &mut Vec<ResolvedTmd>,
                                     anm_packs: &mut Vec<ResolvedAnm>,
                                     tmd_count: &mut usize| {
            for entry in &s.entries {
                let bytes: &[u8] = &entry.bytes;
                for hit in tmd_scan::scan_buffer(bytes) {
                    let payload = bytes[hit.offset..hit.offset + hit.byte_len].to_vec();
                    if let Ok(tmd) = legaia_tmd::parse(&payload) {
                        tmds.push(ResolvedTmd {
                            entry_idx: entry.idx,
                            offset: hit.offset,
                            byte_len: hit.byte_len,
                            tmd,
                            raw: payload,
                        });
                        *tmd_count += 1;
                    }
                }
                if let Some(det) = anm_detect::detect(bytes) {
                    let payload = bytes[..det.size].to_vec();
                    if let Ok(pack) = legaia_anm::parse(&payload) {
                        anm_packs.push(ResolvedAnm {
                            entry_idx: entry.idx,
                            offset: 0,
                            byte_len: det.size,
                            pack,
                            payload,
                        });
                    }
                }
            }
        };
        for shared in shared_scenes {
            parse_scene_tmds_anms(shared, &mut tmds, &mut anm_packs, &mut shared_tmd_count);
        }
        parse_scene_tmds_anms(scene, &mut tmds, &mut anm_packs, &mut 0usize);

        // Pass 2: collect prim targets from every TMD - the union is
        // what the targeted upload aims for.
        let mut needs = Vec::new();
        for rtmd in &tmds {
            needs.extend(legaia_tmd::vram_targeted::collect_prim_targets(
                &rtmd.tmd, &rtmd.raw,
            ));
        }

        // Pass 3: walk every TIM hit (across both shared and scene
        // entries) and feed the targeted builder. The scan walks both
        // raw entry bytes AND any LZS-decompressed sections inside the
        // entry - many battle / level-up bundles wrap their character
        // TIM bank in an LZS container, so a raw-only scan misses
        // them and leaves the dropping CLUT rows unsupplied.
        let mut tim_bufs: Vec<Vec<u8>> = Vec::new();
        let mut tim_parse_failures = 0usize;
        let collect_tim_bufs =
            |s: &Scene, tim_bufs: &mut Vec<Vec<u8>>, tim_parse_failures: &mut usize| {
                for entry in &s.entries {
                    let bytes: &[u8] = &entry.bytes;
                    let scan = tim_scan::scan_entry(bytes);
                    for (source, hit) in &scan.hits {
                        let src: &[u8] = match source {
                            tim_scan::Source::Raw => bytes,
                            tim_scan::Source::Lzs(idx) => scan.lzs_sections[*idx].as_slice(),
                        };
                        let end = hit.offset + hit.byte_len;
                        if end > src.len() {
                            continue;
                        }
                        let payload = &src[hit.offset..end];
                        if legaia_tim::parse(payload).is_ok() {
                            tim_bufs.push(payload.to_vec());
                        } else {
                            *tim_parse_failures += 1;
                        }
                    }
                }
            };
        for shared in shared_scenes {
            collect_tim_bufs(shared, &mut tim_bufs, &mut tim_parse_failures);
        }
        let shared_tim_count = tim_bufs.len();
        collect_tim_bufs(scene, &mut tim_bufs, &mut tim_parse_failures);
        let tim_count = tim_bufs.len();

        let (vram, upload_stats) = legaia_tmd::vram_targeted::build_vram_targeted_from_buffers(
            tim_bufs.iter().map(|v| v.as_slice()),
            &needs,
        );

        Ok((
            Self {
                vram,
                tim_count,
                tim_parse_failures,
                tmds,
                anm_packs,
                shared_tim_count,
                shared_tmd_count,
            },
            upload_stats,
        ))
    }

    /// Same as [`SceneResources::build`] but also bulk-uploads every TIM
    /// and parses every TMD from a set of *shared* CDNAME-block scenes -
    /// the retail engine keeps these resident across scene transitions
    /// at VRAM slots the per-scene loader never touches (player TMD,
    /// shared UI sprite atlas, etc.).
    ///
    /// The shared scenes contribute to [`SceneResources::vram`] *first*,
    /// so scene-local TIMs that map to overlapping slots win the upload
    /// (mirrors the runtime "load shared at boot, then load scene"
    /// order). Their TIM/TMD counts feed [`SceneResources::shared_tim_count`]
    /// / [`SceneResources::shared_tmd_count`]. The TMDs and ANM packs
    /// are appended to the same flat pools so existing
    /// [`SceneResources::tmd_by_coords`] / [`SceneResources::anm_pack_for_actor`]
    /// lookups work unchanged.
    ///
    /// See [`FIELD_SHARED_BLOCKS`] for the names a field / town caller
    /// passes in.
    pub fn build_with_shared(scene: &Scene, shared_scenes: &[&Scene]) -> Result<Self> {
        let mut vram = Vram::new();
        let mut tim_count = 0usize;
        let mut tim_parse_failures = 0usize;
        let mut tmds = Vec::new();
        let mut anm_packs = Vec::new();
        let mut shared_tmd_count = 0usize;

        let sweep_scene = |s: &Scene,
                           vram: &mut Vram,
                           tmds: &mut Vec<ResolvedTmd>,
                           anm_packs: &mut Vec<ResolvedAnm>,
                           tim_count: &mut usize,
                           tim_parse_failures: &mut usize,
                           tmd_count: &mut usize| {
            for entry in &s.entries {
                let bytes: &[u8] = &entry.bytes;
                for hit in tim_scan::scan_buffer(bytes) {
                    let payload = &bytes[hit.offset..hit.offset + hit.byte_len];
                    match legaia_tim::parse(payload) {
                        Ok(tim) => {
                            vram.upload_tim(&tim);
                            *tim_count += 1;
                        }
                        Err(_) => *tim_parse_failures += 1,
                    }
                }
                for hit in tmd_scan::scan_buffer(bytes) {
                    let payload = bytes[hit.offset..hit.offset + hit.byte_len].to_vec();
                    if let Ok(tmd) = legaia_tmd::parse(&payload) {
                        tmds.push(ResolvedTmd {
                            entry_idx: entry.idx,
                            offset: hit.offset,
                            byte_len: hit.byte_len,
                            tmd,
                            raw: payload,
                        });
                        *tmd_count += 1;
                    }
                }
                if let Some(det) = anm_detect::detect(bytes) {
                    let payload = bytes[..det.size].to_vec();
                    if let Ok(pack) = legaia_anm::parse(&payload) {
                        anm_packs.push(ResolvedAnm {
                            entry_idx: entry.idx,
                            offset: 0,
                            byte_len: det.size,
                            pack,
                            payload,
                        });
                    }
                }
            }
        };

        // Shared blocks first so scene-local TIMs win any slot collision.
        for shared in shared_scenes {
            sweep_scene(
                shared,
                &mut vram,
                &mut tmds,
                &mut anm_packs,
                &mut tim_count,
                &mut tim_parse_failures,
                &mut shared_tmd_count,
            );
        }
        // After the shared sweep, tim_count IS the shared contribution -
        // snapshot it before the scene sweep mutates it further.
        let shared_tim_count = tim_count;

        sweep_scene(
            scene,
            &mut vram,
            &mut tmds,
            &mut anm_packs,
            &mut tim_count,
            &mut tim_parse_failures,
            &mut 0usize,
        );

        Ok(Self {
            vram,
            tim_count,
            tim_parse_failures,
            tmds,
            anm_packs,
            shared_tim_count,
            shared_tmd_count,
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

    /// Look up the ANM pack for actor slot `actor_idx`. Returns `None` if the
    /// scene has fewer ANM packs than the requested slot index.
    ///
    /// Ordering follows CDNAME entry order - the same ordering `FUN_8001E890`
    /// uses to register TMDs into `0x8007C018` (actor K → slot K).
    pub fn anm_pack_for_actor(&self, actor_idx: usize) -> Option<&ResolvedAnm> {
        self.anm_packs.get(actor_idx)
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
    fn build_filtered_vram_mesh_returns_empty_for_empty_tmd() {
        // ResolvedTmd around an empty TMD just to exercise the helper -
        // the filter only matters when prims exist, but we want to prove
        // the wiring compiles and runs without panicking on the no-prim
        // edge case (which is what most synthetic test fixtures hit).
        let rtmd = ResolvedTmd {
            entry_idx: 0,
            offset: 0,
            byte_len: 0,
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x80000002,
                    flags: 0,
                    nobj: 0,
                    flist_bit_set: false,
                },
                objects: vec![],
            },
            raw: Vec::new(),
        };
        let vram = Vram::new();
        let mesh = rtmd.build_filtered_vram_mesh(&vram);
        assert!(mesh.indices.is_empty());
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
        assert_eq!(res.shared_tim_count, 0);
        assert_eq!(res.vram.pixel(0, 0), 0xAAAA);
        assert_eq!(res.vram.pixel(128, 128), 0xAAAA);
    }

    /// Shared CDNAME blocks (e.g. `init_data`, `player_data`) must
    /// populate VRAM at slots the per-scene block doesn't touch - this
    /// proves the order is shared-first then scene, so the scene wins
    /// any slot collision, but non-overlapping shared slots survive.
    #[test]
    fn build_with_shared_uploads_shared_tims_into_distinct_slots() {
        let scene_only = make_scene(vec![SceneEntry {
            idx: 0,
            class: Class::UnknownOther,
            bytes: Arc::new(synth_tim_16bpp(64, 0)),
        }]);
        let shared = make_scene(vec![SceneEntry {
            idx: 100,
            class: Class::UnknownOther,
            bytes: Arc::new(synth_tim_16bpp(768, 0)),
        }]);
        let res = SceneResources::build_with_shared(&scene_only, &[&shared]).unwrap();
        // 1 shared TIM + 1 scene TIM = 2 total
        assert_eq!(res.tim_count, 2);
        assert_eq!(res.shared_tim_count, 1);
        // Shared TIM landed at (768, 0); scene TIM at (64, 0).
        assert_eq!(res.vram.pixel(768, 0), 0xAAAA);
        assert_eq!(res.vram.pixel(64, 0), 0xAAAA);
    }

    /// Scene-local TIMs win when the shared and scene blocks both target
    /// the same VRAM slot. Mirrors the retail boot-then-scene ordering:
    /// init_data lays down the boot atlas, then a scene-specific TIM at
    /// the same slot overwrites it.
    #[test]
    fn build_with_shared_scene_wins_overlapping_slots() {
        // Shared writes 0xAAAA at (0, 0); scene writes 0x5555 at (0, 0).
        // Build two distinct 1px TIMs that target the same slot.
        fn pixel_at(x: u16, y: u16, color: u16) -> Vec<u8> {
            let mut b = Vec::new();
            b.extend_from_slice(&0x10u32.to_le_bytes());
            b.extend_from_slice(&0x02u32.to_le_bytes());
            b.extend_from_slice(&20u32.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&y.to_le_bytes());
            b.extend_from_slice(&4u16.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            for _ in 0..4 {
                b.extend_from_slice(&color.to_le_bytes());
            }
            b
        }
        let shared = make_scene(vec![SceneEntry {
            idx: 100,
            class: Class::UnknownOther,
            bytes: Arc::new(pixel_at(0, 0, 0xAAAA)),
        }]);
        let scene_only = make_scene(vec![SceneEntry {
            idx: 0,
            class: Class::UnknownOther,
            bytes: Arc::new(pixel_at(0, 0, 0x5555)),
        }]);
        let res = SceneResources::build_with_shared(&scene_only, &[&shared]).unwrap();
        assert_eq!(
            res.vram.pixel(0, 0),
            0x5555,
            "scene must win slot collision"
        );
    }
}
