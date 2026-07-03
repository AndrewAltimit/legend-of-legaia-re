//! Headless subcommand implementations: scene inspection (`info`,
//! `list-scenes`, `clut-trace`, `man-scripts`), the parity/trace oracles
//! (`vram-oracle`, `mode-trace`, `audio-trace`, `pcm-trace`, `replay`,
//! `scenarios`), the save/load smoke commands, and the synthetic
//! session drivers (`battle`, `inventory`, `equip`, `title`, ...).
//!
//! This module is a thin coordinator: the command families live in the
//! `commands/` submodules and are re-exported here so callers keep
//! resolving them as `commands::cmd_*`. The shared `open_index_from_args`
//! helper stays here because it is used across several families.

use crate::cli::ConfigCmd;
use crate::{AudioTraceArgs, ModeTraceArgs, PcmTraceArgs, VramOracleArgs, decode_str_frame_count};
use anyhow::{Context, Result};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::SceneResources;
use legaia_engine_shell::audio_trace_oracle::{
    AudioTraceFrame, audio_trace_to_jsonl, engine_trace_from_paths, first_audio_trace_divergence,
    first_audio_trace_divergence_multi, load_runtime_audio_trace_from_save,
    load_runtime_audio_trace_jsonl,
};
use legaia_engine_shell::mode_trace_oracle::{
    ModeTraceFrame, build_engine_mode_trace, first_mode_trace_divergence,
    load_runtime_mode_trace_from_save, mode_trace_to_jsonl,
};
use legaia_engine_shell::pcm_oracle::{
    EnginePcmTrace, PcmStats, build_engine_pcm_trace, first_pcm_divergence, pcm_stats,
    retail_reference_pcm, write_wav,
};
use legaia_engine_shell::replay::ReplayFile;
use legaia_engine_shell::vram_oracle::{
    TexpageDivergence, build_engine_vram_bytes_with_frames, first_texpage_divergence,
    load_runtime_vram_from_save, vram_to_le_bytes,
};
use legaia_prot::cdname;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "commands/info.rs"]
mod info;
#[path = "commands/replay.rs"]
mod replay;
#[path = "commands/run.rs"]
mod run;
#[path = "commands/sessions.rs"]
mod sessions;
#[path = "commands/trace.rs"]
mod trace;
#[path = "commands/vram.rs"]
mod vram;

pub(crate) use info::*;
pub(crate) use replay::*;
pub(crate) use run::*;
pub(crate) use sessions::*;
pub(crate) use trace::*;
pub(crate) use vram::*;

/// Open a `ProtIndex` from either an extracted directory (default) or a
/// disc image (when `--disc` was provided). Used by subcommands that
/// accept either source.
fn open_index_from_args(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
) -> Result<ProtIndex> {
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
        ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
            .with_context(|| format!("build ProtIndex from {}", disc_path.display()))
    } else {
        let prot = extracted_root.join("PROT.DAT");
        if !prot.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                prot.display()
            );
        }
        ProtIndex::open_extracted(extracted_root)
            .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))
    }
}
