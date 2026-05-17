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
use legaia_engine_core::scene_resources::{FIELD_SHARED_BLOCKS, SceneResources};

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

/// Build engine-side VRAM via the targeted-upload pre-pass. No
/// `BootSession` involvement - fast and stateless.
pub fn build_engine_vram_bytes_prepass(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
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
    let (resources, _) = SceneResources::build_targeted(&scene, &shared_refs)?;
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
