//! TMD viewer loader: builds either a flat-shaded mesh payload or a
//! VRAM-textured one (with a targeted TIM upload) for a single TMD.

use crate::common::short_path;
use anyhow::{Context, Result};
use legaia_tim::Vram;
use std::path::{Path, PathBuf};

/// CPU-side payload for the VRAM-mesh path. Built by [`load_tmd_for_view`];
/// uploaded to GPU on the renderer's thread.
pub(crate) struct VramMeshPayload {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) uvs: Vec<[u8; 2]>,
    pub(crate) cba_tsb: Vec<[u16; 2]>,
    pub(crate) normals: Vec<[f32; 3]>,
    /// Per-vertex baked prim colour - the PSX texture-modulation term
    /// (`texel * colour / 128`). See [`legaia_tmd::mesh::VramMesh::colors`].
    pub(crate) colors: Vec<[u8; 3]>,
    pub(crate) indices: Vec<u32>,
    /// CPU-side VRAM holding every TIM in the source PROT entry, placed at
    /// its canonical fb_x/fb_y. The fragment shader does the page+CLUT
    /// lookup using each vertex's (cba, tsb).
    pub(crate) vram: Vram,
    /// Number of TIMs uploaded into `vram` (window-title context only).
    pub(crate) tim_count: usize,
    /// Source dir we pulled the TIMs from (window-title context).
    pub(crate) tim_dir_label: String,
}

/// Loader result: either a flat-shaded mesh, or a VRAM-textured one with
/// every TIM in the source PROT entry uploaded to a software VRAM. Picked
/// by whether [`sibling_tim_dir`] turns up a directory with TIMs in it.
pub(crate) enum TmdViewData {
    Flat {
        positions: Vec<[f32; 3]>,
        indices: Vec<u32>,
    },
    Vram(VramMeshPayload),
}

pub(crate) fn load_tmd_for_view(
    tmd_path: &Path,
    vram_extras: &[PathBuf],
    no_textures: bool,
) -> Result<TmdViewData> {
    let bytes = std::fs::read(tmd_path).with_context(|| format!("read {}", tmd_path.display()))?;
    let tmd =
        legaia_tmd::parse(&bytes).with_context(|| format!("parse TMD {}", tmd_path.display()))?;
    let sibling = sibling_tim_dir(tmd_path);
    if !no_textures && (sibling.is_some() || !vram_extras.is_empty()) {
        // Order: extras first (shared/base data), sibling last (so the
        // mesh's own scene data overlays the base on collision).
        let mut dirs: Vec<&Path> = vram_extras.iter().map(|p| p.as_path()).collect();
        if let Some(s) = sibling.as_ref() {
            dirs.push(s.as_path());
        }
        // Collect the CLUT rows / texture pages that this TMD's primitives
        // actually sample. The TIM corpus on a single PROT entry can run
        // into the hundreds (399 TIMs across all of `0866_battle_data`),
        // and uploading every one of them clobbers the 1 MB VRAM with
        // overlapping image / CLUT regions - what was a valid palette row
        // for one prim ends up sampled as raw image data, which the
        // shader's paletted decode renders as rainbow garbage. Targeting
        // the upload to the prims actually present in this mesh keeps the
        // VRAM small and collision-free.
        let needs = legaia_tmd::vram_targeted::collect_prim_targets(&tmd, &bytes);
        let (vram, stats) = legaia_tmd::vram_targeted::build_vram_targeted(&dirs, &needs);
        let tim_count = stats.uploaded_tims;
        log::info!(
            "targeted VRAM upload: {} of {} TIMs contribute (both={} img-only={} clut-only={}) for {} prim target(s)",
            stats.uploaded_tims,
            stats.total_tims,
            stats.uploaded_both,
            stats.uploaded_image_only,
            stats.uploaded_clut_only,
            needs.len(),
        );
        if tim_count > 0 {
            // Drop primitives whose texture page wasn't supplied - they
            // would otherwise rasterise as flat `CLUT[0]` (commonly green
            // for Legaia palettes) and obscure correctly-textured geometry.
            // Aggregate the per-prim verdict so the warning at the end can
            // explain *why* prims were dropped (missing CLUT vs depth
            // mismatch vs missing texture page).
            let mut tally = PrimDropTally::default();
            let vram_mesh =
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&tmd, &bytes, |cba, tsb, uvs| {
                    let status = vram.prim_texture_status(cba, tsb, uvs);
                    tally.record(status);
                    status.ok()
                });
            tally.log_summary();
            if !vram_mesh.indices.is_empty() {
                warn_unfilled_cluts(&vram_mesh, &vram);
                let label = match sibling.as_ref() {
                    Some(s) if vram_extras.is_empty() => short_path(s),
                    Some(s) => format!("{} + {} extra dir(s)", short_path(s), vram_extras.len()),
                    None => format!("{} extra dir(s)", vram_extras.len()),
                };
                return Ok(TmdViewData::Vram(VramMeshPayload {
                    positions: vram_mesh.positions,
                    uvs: vram_mesh.uvs,
                    cba_tsb: vram_mesh.cba_tsb,
                    normals: vram_mesh.normals,
                    colors: vram_mesh.colors,
                    indices: vram_mesh.indices,
                    vram,
                    tim_count,
                    tim_dir_label: label,
                }));
            }
        }
    }
    let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &bytes);
    Ok(TmdViewData::Flat {
        positions: mesh.positions,
        indices: mesh.indices,
    })
}

// Per-prim VRAM target collection + targeted upload now live in
// [`legaia_tmd::vram_targeted`] so the asset-viewer GUI and the `tmd
// prims --vram-dir` / `tmd vram-dump` CLI agree on which prims are
// renderable. See `crates/tmd/src/vram_targeted.rs` for the algorithm.

/// Diagnostic: scan distinct CBA values referenced by the mesh and check
/// whether the corresponding CLUT row in VRAM has any non-zero data.
/// Empty rows mean the CLUT lives in a PROT entry we didn't load - the
/// user probably wants to add it via `--vram-extra-dir`.
fn warn_unfilled_cluts(mesh: &legaia_tmd::mesh::VramMesh, vram: &Vram) {
    let mut missing_rows: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    let mut seen_cba: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for ct in &mesh.cba_tsb {
        let cba = ct[0];
        if cba == 0 || !seen_cba.insert(cba) {
            continue;
        }
        let cy = ((cba >> 6) & 0x1FF) as usize;
        let cx_base = ((cba & 0x3F) * 16) as usize;
        // Sample 16 entries (one 4bpp palette). If all zero, this CLUT
        // wasn't uploaded - we'd render this prim with garbage.
        let any = (0..16).any(|i| vram.pixel(cx_base + i, cy) != 0);
        if !any {
            missing_rows.insert(cba >> 6);
        }
    }
    if !missing_rows.is_empty() {
        log::warn!(
            "VRAM is missing CLUT data for rows {:?} - mesh prims will sample zeros. Try --vram-extra-dir extracted/tim_scan/0866_battle_data (battle palettes are shared across level_up / town entries).",
            missing_rows.iter().collect::<Vec<_>>()
        );
    }
}

/// Per-reason counters built up while walking the per-prim VRAM verdict
/// during mesh construction. Lets the asset viewer print one merged
/// diagnostic that distinguishes "CLUT row not uploaded at all" from
/// "CLUT row uploaded but at the wrong palette depth", instead of just
/// "skipped N prims".
#[derive(Default)]
struct PrimDropTally {
    considered: usize,
    kept: usize,
    missing_clut_rows: std::collections::BTreeSet<u16>,
    missing_clut_count: usize,
    depth_mismatch_rows: std::collections::BTreeMap<u16, (u16, u16)>,
    depth_mismatch_count: usize,
    missing_page_tpages: std::collections::BTreeSet<u16>,
    missing_page_count: usize,
}

impl PrimDropTally {
    fn record(&mut self, status: legaia_tim::vram::PrimTextureStatus) {
        self.considered += 1;
        match status {
            legaia_tim::vram::PrimTextureStatus::Ok => self.kept += 1,
            legaia_tim::vram::PrimTextureStatus::MissingClut { row } => {
                self.missing_clut_rows.insert(row);
                self.missing_clut_count += 1;
            }
            legaia_tim::vram::PrimTextureStatus::ClutDepthMismatch {
                row,
                populated_width,
                expected_width,
            } => {
                self.depth_mismatch_rows
                    .insert(row, (populated_width, expected_width));
                self.depth_mismatch_count += 1;
            }
            legaia_tim::vram::PrimTextureStatus::MissingTexturePage { tpage } => {
                self.missing_page_tpages.insert(tpage);
                self.missing_page_count += 1;
            }
        }
    }

    fn log_summary(&self) {
        let dropped = self.considered.saturating_sub(self.kept);
        if dropped == 0 {
            return;
        }
        log::info!(
            "skipped {} prim(s) ({}/{} kept)",
            dropped,
            self.kept,
            self.considered,
        );
        if self.missing_clut_count > 0 {
            log::warn!(
                "  missing CLUT data for {} prim(s) across rows {:?} - the TIM(s) carrying these palettes weren't loaded; try --vram-extra-dir or a different --bundle / --scene",
                self.missing_clut_count,
                self.missing_clut_rows.iter().collect::<Vec<_>>(),
            );
        }
        if self.depth_mismatch_count > 0 {
            for (row, (populated, expected)) in &self.depth_mismatch_rows {
                log::warn!(
                    "  CLUT row {} IS populated but {} entries wide ({}-bit palette); prim expects {} entries ({}-bit) - prim dropped to avoid rainbow noise",
                    row,
                    populated,
                    if *populated >= 256 { 8 } else { 4 },
                    expected,
                    if *expected >= 256 { 8 } else { 4 },
                );
            }
        }
        if self.missing_page_count > 0 {
            log::warn!(
                "  missing texture-page data for {} prim(s) across tpages {:?} - the TIM(s) for these pages weren't loaded",
                self.missing_page_count,
                self.missing_page_tpages.iter().collect::<Vec<_>>(),
            );
        }
    }
}

/// Find the TIM directory that holds every TIM from the same PROT entry
/// as `tmd_path`. Convention: the bulk-scan extractors write TMDs to
/// `extracted/tmd_scan/<entry>/raw_off<HEX>.tmd` and TIMs to
/// `extracted/tim_scan/<entry>/raw_off<HEX>_<W>x<H>_<BPP>bpp.tim`.
/// Returns the matching `tim_scan/<entry>/` if it exists.
fn sibling_tim_dir(tmd_path: &Path) -> Option<PathBuf> {
    let entry_dir = tmd_path.parent()?;
    let entry_name = entry_dir.file_name()?;
    let scan_root = entry_dir.parent()?.parent()?; // up two: tmd_scan → extracted
    let tim_dir = scan_root.join("tim_scan").join(entry_name);
    tim_dir.is_dir().then_some(tim_dir)
}
