//! VRAM oracle plumbing shared between the `legaia-engine vram-oracle`
//! subcommand and the disc-gated `vram_oracle_e1` integration test.
//!
//! The oracle compares the engine's built-from-PROT VRAM (1 MiB BGR555
//! LE) against a runtime VRAM blob captured from a mednafen save state.
//! The engine port renders direct-to-wgpu, so the framebuffer half of
//! VRAM (y < 256) is not deterministically populated by the engine -
//! the byte-exact assertion is intentionally scoped to the texpage
//! region (y >= 256).
//!
//! Two engine-side build paths:
//!   - `build_engine_vram_bytes_prepass` - pure `SceneResources` build,
//!     no engine ticking. Fast, deterministic, what the scene-bundle
//!     loader produces at load time.
//!   - `build_engine_vram_bytes_with_frames` - wraps `BootSession` and
//!     ticks `frames` times before sampling, so dynamic uploads (CLUT
//!     swaps, fog ramps) land in the snapshot.
//!
//! Runtime-side: `load_runtime_vram_from_save` lifts the GPU section's
//! 1 MiB blob out of a mednafen `.mc{slot}` via `legaia-mednafen`.

use std::path::Path;

use anyhow::{Context, Result};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

use crate::{BootConfig, BootSession};

/// PSX VRAM dimensions in BGR555 cells.
pub const VRAM_WIDTH: usize = 1024;
pub const VRAM_HEIGHT: usize = 512;
/// Texpage region starts at `y = 256`. Top half (y < 256) is
/// framebuffer + scratch; the engine port doesn't write to it.
pub const TEXPAGE_Y_START: usize = 256;
/// 1 MiB - matches `mednafen-state vram-dump --out-bin` output.
pub const VRAM_BYTES: usize = VRAM_WIDTH * VRAM_HEIGHT * 2;

/// Serialise a software `Vram` into a 1 MiB little-endian BGR555 buffer
/// matching `mednafen-state vram-dump --out-bin`.
pub fn vram_to_le_bytes(vram: &legaia_tim::Vram) -> Vec<u8> {
    let mut out = Vec::with_capacity(VRAM_BYTES);
    for y in 0..VRAM_HEIGHT {
        for x in 0..VRAM_WIDTH {
            out.extend_from_slice(&vram.pixel(x, y).to_le_bytes());
        }
    }
    out
}

/// The load kind the standalone oracle build uses for `scene_name`, mirroring
/// the live `enter_field_scene` choice: world-map scenes (`map\d\d`) decode
/// their slot-0 terrain atlas via [`SceneLoadKind::WorldMap`], every other
/// scene uses [`SceneLoadKind::Field`]. Without the world-map kind the generic
/// TIM scanner can't see the kingdom bundle's descriptor-table slot, so the
/// oracle would report the terrain pages (grass / water) as a phantom gap the
/// engine doesn't actually have.
pub fn oracle_load_kind(scene_name: &str) -> SceneLoadKind {
    if legaia_engine_core::scene::is_world_map_scene(scene_name) {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    }
}

/// Build engine-side VRAM via the targeted-upload pre-pass. No
/// `BootSession` involvement - fast and stateless. Picks the load kind via
/// [`oracle_load_kind`] so the standalone build matches what the engine
/// uploads live.
pub fn build_engine_vram_bytes_prepass(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
) -> Result<Vec<u8>> {
    build_engine_vram_bytes_prepass_with_kind(
        scene_name,
        extracted_root,
        disc,
        oracle_load_kind(scene_name),
    )
}

/// As [`build_engine_vram_bytes_prepass`] but with the load kind forced. The
/// auto-selecting wrapper is the parity path; this exists so a regression test
/// can contrast the kinds (the world-map alignment lifts terrain-page
/// residency vs a field-kind build).
pub fn build_engine_vram_bytes_prepass_with_kind(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    kind: SceneLoadKind,
) -> Result<Vec<u8>> {
    let index = open_index(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    // Parity oracle: the retail field loader uploads every scene TIM to VRAM,
    // not just the render-targeted subset a mesh prim samples, so
    // `upload_all_tims` is on regardless of kind.
    let options = BuildOptions {
        kind,
        upload_all_tims: true,
    };
    let (mut resources, _) =
        SceneResources::build_targeted_with_options(&scene, &shared_refs, options)?;
    // Mirror the live field-entry path: upload the `befect_data` (PROT 0874
    // section 2) effect-texture TIMs into VRAM. These sit at fb_y=256
    // (fb_x 320/384/832/852/872/880) and are resident across field + battle
    // in retail; the targeted scene build alone never touches them, so without
    // this the oracle reports them as a phantom static gap the engine doesn't
    // actually have. Image pages only (`upload_clut = false`): retail keeps the
    // effect *pixels* field-resident but uploads their CLUTs (rows 473..=478)
    // at battle entry, so writing the CLUTs here would be a wrong static upload.
    // Soft-fail: a scene without the cluster just stays as-is.
    let _ = legaia_engine_core::scene::upload_effect_textures_into_vram(
        &index,
        &mut resources.vram,
        false,
    );
    Ok(vram_to_le_bytes(&resources.vram))
}

/// Boot a `BootSession` on `scene_name`, tick it `frames` times, then
/// sample its VRAM. Use this when the oracle needs to catch dynamic
/// uploads that the pre-pass doesn't see.
pub fn build_engine_vram_bytes_with_frames(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
) -> Result<Vec<u8>> {
    if frames == 0 {
        return build_engine_vram_bytes_prepass(scene_name, extracted_root, disc);
    }
    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    for _ in 0..frames {
        let _ = session.tick()?;
    }
    let resources = session
        .host
        .resources
        .as_ref()
        .context("BootSession did not produce SceneResources after ticking")?;
    Ok(vram_to_le_bytes(&resources.vram))
}

/// Lift the runtime VRAM blob (1 MiB BGR555 LE) out of a mednafen
/// `.mc{slot}` save. Matches `mednafen-state vram-dump --out-bin`.
pub fn load_runtime_vram_from_save(save: &Path) -> Result<Vec<u8>> {
    use legaia_mednafen::{PsxGpu, SaveState};
    let state = SaveState::from_path(save)
        .with_context(|| format!("load mednafen save {}", save.display()))?;
    let gpu = PsxGpu::new(&state);
    let bytes = gpu
        .vram_bytes()
        .with_context(|| format!("save state {} has no GPU.GPURAM entry", save.display()))?;
    Ok(bytes.to_vec())
}

/// First byte-divergence between `engine` and `runtime` in the texpage
/// region (`y >= TEXPAGE_Y_START`). Returns `None` on byte-exact match.
#[derive(Debug, Clone, Copy)]
pub struct TexpageDivergence {
    pub y: usize,
    pub x: usize,
    pub engine_word: u16,
    pub runtime_word: u16,
}

pub fn first_texpage_divergence(engine: &[u8], runtime: &[u8]) -> Option<TexpageDivergence> {
    assert_eq!(engine.len(), VRAM_BYTES);
    assert_eq!(runtime.len(), VRAM_BYTES);
    for y in TEXPAGE_Y_START..VRAM_HEIGHT {
        let row_base = y * VRAM_WIDTH * 2;
        for x in 0..VRAM_WIDTH {
            let off = row_base + x * 2;
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            if ew != rw {
                return Some(TexpageDivergence {
                    y,
                    x,
                    engine_word: ew,
                    runtime_word: rw,
                });
            }
        }
    }
    None
}

/// VRAM rows occupied by the **runtime-managed NPC / character CLUT band**
/// (centred on the row-479 NPC palette row; character palettes stack into the
/// adjacent rows). This region is *not* part of the static DMA-every-TIM scene
/// upload: the retail engine paints it per-frame via the targeted CLUT pass
/// keyed on which NPCs / party members are present (see
/// [`docs/formats/npc-palette.md`] and the row-479 merge-zeros mechanism). It
/// is therefore scene/actor-state-dependent rather than a static scene
/// texture, so the static-mask oracle excludes it. Measured empirically: all
/// engine-vs-retail discrepancies on the town01 static mask fall inside this
/// band; the bulk texture region is byte-exact on every uploaded static pixel.
pub const NPC_CLUT_BAND_ROWS: std::ops::Range<usize> = 476..486;

/// CLUT cells the **world-map walk-view runtime rewrites** (water / shoreline
/// palette cycling). The kingdom bundle's slot-0 terrain atlas declares its
/// CLUTs on rows 506..=509. Row 506's 16-entry head is the documented
/// **13-frame ocean CLUT animation** (`legaia_asset::ocean`,
/// `docs/subsystems/world-map.md` "Ocean animation") - retail DMAs a
/// precomputed frame over it each step, so a capture holds an arbitrary
/// animation phase, never the disc base CLUT. A ten-state capture census
/// (per-column variance across the map01-resident catalog states) pins the
/// runtime-touched columns precisely:
///
/// - **row 506, cols 0..48**: the ocean head (0..15), a second block head
///   (16..31), a rotating ring of STP-set near-copies of the ocean colours
///   (32..39, phase-locked to the ocean), and the runtime-*generated*
///   pure-channel tail (40..47) whose intensity animates with the same
///   phase - one coupled animator, no disc source.
/// - **row 508, cols 0..48**: animated head entries ({1, 14, 15, 26, 27})
///   plus a live-maintained mirror at 32..47 (`[32..47] == [0..15]` at every
///   captured phase - the disc base there is a different palette the runtime
///   overwrites).
/// - **row 509, cols 42..44**: exactly two animated entries; the rest of the
///   row is byte-exact vs the disc build in every capture.
///
/// Row 507 is fully static (256/256 byte-exact) and stays asserted, as do
/// the remaining columns of 506/508/509 - the census lets the oracle assert
/// them instead of excluding the whole rows.
pub const WORLD_MAP_CLUT_CYCLE_CELLS: [(usize, std::ops::Range<usize>); 3] =
    [(506, 0..48), (508, 0..48), (509, 42..44)];

/// Clear `mask` on every cell of [`WORLD_MAP_CLUT_CYCLE_CELLS`]. Apply to a
/// world-map scene's static mask before asserting upload parity; field/town
/// scenes keep the cells asserted (their content there is scene-owned).
pub fn clear_world_map_clut_cycle_rows(mask: &mut [bool]) {
    for (y, cols) in WORLD_MAP_CLUT_CYCLE_CELLS {
        mask[y * VRAM_WIDTH + cols.start..y * VRAM_WIDTH + cols.end].fill(false);
    }
}

/// Refine a per-scene static mask with **cross-scene** evidence for the
/// global shared effect-texture band (`befect_data`, see
/// [`legaia_engine_core::scene::effect_texture_image_rects`]): a cell inside
/// those rects stays static only if every capture of **every** scene holds
/// the same word there. The band is one global disc source uploaded
/// identically for all field scenes, but a few of its pixels are
/// history-dependent (the pause-menu entry path writes a 3-word F-variant
/// onto row 271 that the first battle effect use overwrites with the disc
/// bytes again), so same-scene captures that share menu/battle history
/// misclassify them as static; any same-band disagreement anywhere in the
/// capture corpus proves the cell dynamic.
pub fn refine_mask_with_shared_band(
    mask: &mut [bool],
    rects: &[(u16, u16, u16, u16)],
    all_snapshots: &[&[u8]],
) {
    let Some(first) = all_snapshots.first() else {
        return;
    };
    for &(fb_x, fb_y, w, h) in rects {
        for y in fb_y as usize..(fb_y as usize + h as usize).min(VRAM_HEIGHT) {
            for x in fb_x as usize..(fb_x as usize + w as usize).min(VRAM_WIDTH) {
                let off = (y * VRAM_WIDTH + x) * 2;
                let f = &first[off..off + 2];
                if all_snapshots[1..].iter().any(|s| &s[off..off + 2] != f) {
                    mask[y * VRAM_WIDTH + x] = false;
                }
            }
        }
    }
}

/// Per-word "static" mask across a set of same-scene runtime VRAM snapshots:
/// `mask[i] == true` where every snapshot holds the **same** 16-bit word, i.e.
/// the pixel is part of the scene's static VRAM rather than dynamic /
/// residual state (animation frames, battle leftovers, scroll position). The
/// engine's stateless pre-pass can only be held to the static set. Requires at
/// least one snapshot; with one snapshot every pixel is trivially "static".
pub fn compute_static_mask(snapshots: &[&[u8]]) -> Vec<bool> {
    let words = VRAM_WIDTH * VRAM_HEIGHT;
    let mut mask = vec![true; words];
    if snapshots.len() < 2 {
        return mask;
    }
    let first = snapshots[0];
    for other in &snapshots[1..] {
        for (m, (fa, ob)) in mask
            .iter_mut()
            .zip(first.chunks_exact(2).zip(other.chunks_exact(2)))
        {
            if fa != ob {
                *m = false;
            }
        }
    }
    mask
}

/// First pixel where the engine's upload is **wrong** on the static mask: a
/// pixel that is (a) static (`mask` true), (b) in the texpage region
/// (`y >= TEXPAGE_Y_START`), (c) **outside** [`NPC_CLUT_BAND_ROWS`], (d)
/// uploaded by the engine (`engine_word != 0`), yet (e) differs from the
/// runtime word. Returns `None` when the engine's static uploads are all
/// byte-exact. Incompleteness (engine `0` where retail has texture) is *not*
/// flagged - the engine is allowed to be a faithful subset (it doesn't yet
/// assemble every boot-resident texture), but it must never upload a wrong
/// texel where the scene is static.
pub fn first_static_upload_divergence(
    engine: &[u8],
    runtime: &[u8],
    static_mask: &[bool],
) -> Option<TexpageDivergence> {
    assert_eq!(engine.len(), VRAM_BYTES);
    assert_eq!(runtime.len(), VRAM_BYTES);
    for y in TEXPAGE_Y_START..VRAM_HEIGHT {
        if NPC_CLUT_BAND_ROWS.contains(&y) {
            continue;
        }
        let row_base = y * VRAM_WIDTH;
        for x in 0..VRAM_WIDTH {
            let widx = row_base + x;
            if !static_mask[widx] {
                continue;
            }
            let off = widx * 2;
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            if ew == 0 {
                continue; // incompleteness not asserted
            }
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            if ew != rw {
                return Some(TexpageDivergence {
                    y,
                    x,
                    engine_word: ew,
                    runtime_word: rw,
                });
            }
        }
    }
    None
}

fn open_index(extracted_root: &Path, disc: Option<&Path>) -> Result<ProtIndex> {
    if let Some(disc_path) = disc {
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)
            .with_context(|| format!("open disc image {}", disc_path.display()))?;
        let prot_bytes = vfs
            .read("prot.dat")
            .context("PROT.DAT not present in disc image")?;
        let cdname_text = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .ok()
            .map(|b| String::from_utf8(b).context("CDNAME.TXT is not valid UTF-8"))
            .transpose()?;
        return ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
            .with_context(|| format!("build ProtIndex from {}", disc_path.display()));
    }
    let prot = extracted_root.join("PROT.DAT");
    if !prot.exists() {
        anyhow::bail!(
            "missing {} (run `legaia-extract` first or pass --disc PATH)",
            prot.display()
        );
    }
    let prot_bytes = std::fs::read(&prot).with_context(|| format!("read {}", prot.display()))?;
    let cdname_path = extracted_root.join("CDNAME.TXT");
    let cdname_text = if cdname_path.exists() {
        Some(
            std::fs::read_to_string(&cdname_path)
                .with_context(|| format!("read {}", cdname_path.display()))?,
        )
    } else {
        None
    };
    ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
        .with_context(|| format!("build ProtIndex from {}", extracted_root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Vec<u8> {
        vec![0u8; VRAM_BYTES]
    }
    fn set(buf: &mut [u8], x: usize, y: usize, w: u16) {
        let off = (y * VRAM_WIDTH + x) * 2;
        buf[off..off + 2].copy_from_slice(&w.to_le_bytes());
    }

    #[test]
    fn static_mask_flags_only_disagreeing_words() {
        let mut a = blank();
        let mut b = blank();
        // y=300 x=10 agrees; y=300 x=11 disagrees.
        set(&mut a, 10, 300, 0x1234);
        set(&mut b, 10, 300, 0x1234);
        set(&mut a, 11, 300, 0x1111);
        set(&mut b, 11, 300, 0x2222);
        let mask = compute_static_mask(&[a.as_slice(), b.as_slice()]);
        assert!(mask[300 * VRAM_WIDTH + 10], "agreeing word is static");
        assert!(!mask[300 * VRAM_WIDTH + 11], "disagreeing word is dynamic");
    }

    #[test]
    fn single_snapshot_is_all_static() {
        let a = blank();
        let mask = compute_static_mask(&[a.as_slice()]);
        assert!(mask.iter().all(|&b| b));
    }

    #[test]
    fn wrong_static_upload_in_bulk_region_is_flagged() {
        let mut engine = blank();
        let mut runtime = blank();
        // Static pixel in the bulk texpage region (y=300, outside the CLUT band):
        // engine uploads 0xAAAA but retail has 0xBBBB.
        set(&mut engine, 5, 300, 0xAAAA);
        set(&mut runtime, 5, 300, 0xBBBB);
        let mask = vec![true; VRAM_WIDTH * VRAM_HEIGHT];
        let d = first_static_upload_divergence(&engine, &runtime, &mask)
            .expect("wrong static upload must be flagged");
        assert_eq!((d.y, d.x), (300, 5));
        assert_eq!((d.engine_word, d.runtime_word), (0xAAAA, 0xBBBB));
    }

    #[test]
    fn incompleteness_and_over_upload_and_clut_band_are_not_flagged() {
        let mut engine = blank();
        let mut runtime = blank();
        let mask = vec![true; VRAM_WIDTH * VRAM_HEIGHT];

        // (a) Incompleteness: engine 0 where retail has texture - allowed.
        set(&mut runtime, 5, 300, 0xBBBB);
        // (b) Over-upload INTO the CLUT band (row 479): engine wrong, retail 0 -
        //     excluded because the band is runtime-managed.
        set(&mut engine, 5, 479, 0xAAAA);
        // (c) Wrong upload but on a NON-static pixel - excluded by the mask.
        let mut m = mask.clone();
        set(&mut engine, 9, 300, 0xCCCC);
        set(&mut runtime, 9, 300, 0xDDDD);
        m[300 * VRAM_WIDTH + 9] = false;

        assert!(first_static_upload_divergence(&engine, &runtime, &m).is_none());
    }

    #[test]
    fn framebuffer_half_is_not_asserted() {
        let mut engine = blank();
        let mut runtime = blank();
        // y < TEXPAGE_Y_START: a wrong upload here is ignored.
        set(&mut engine, 5, 100, 0xAAAA);
        set(&mut runtime, 5, 100, 0xBBBB);
        let mask = vec![true; VRAM_WIDTH * VRAM_HEIGHT];
        assert!(first_static_upload_divergence(&engine, &runtime, &mask).is_none());
    }
}
