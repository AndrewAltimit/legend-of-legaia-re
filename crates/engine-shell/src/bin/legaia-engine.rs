//! Top-level engine driver. The "single command" that turns extracted-disc
//! bytes into a runtime view of any CDNAME scene.
//!
//! Subcommands:
//!
//! - `info` - headless one-line summary of a scene's resolved asset chain.
//! - `list-scenes` - every CDNAME scene name with its PROT range.
//! - `play` - headless engine tick: world + camera + audio, no window.
//! - `play-window` - windowed engine: opens a wgpu surface, renders scene
//!   TMDs against the software PSX VRAM each frame. Input: arrows = D-pad,
//!   Z = Cross, Esc = quit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use glam::{Mat4, Vec3, Vec4};
use legaia_engine_core::menu_runtime::{MenuInput, MenuRuntime, MenuState};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_engine_core::world::{AnimPlayer, SceneMode};
use legaia_engine_render::{
    ColorSceneDraw, RenderTarget, Scene as RenderScene, SceneDraw, ShopRow, TextDraw, TextOverlay,
    UploadedColorMesh, UploadedFontAtlas, UploadedVram, UploadedVramMesh, capture_banner_draws_for,
    level_up_draws_for, shop_draws_for, text_draws_for,
    window::{EngineWindow, orbit_camera_mvp},
};
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
use legaia_engine_shell::replay::{PadEvent, ReplayFile, ReplayMeta};
use legaia_engine_shell::vram_oracle::{
    TexpageDivergence, build_engine_vram_bytes_with_frames, first_texpage_divergence,
    load_runtime_vram_from_save, vram_to_le_bytes,
};
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_font::Font;
use legaia_prot::cdname;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

#[derive(Parser, Debug)]
#[command(
    name = "legaia-engine",
    about = "Top-level driver for the Legaia clean-room engine. Boots a CDNAME scene from extracted PROT bytes."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build [`SceneResources`] for one scene and print a summary line. Use
    /// this to verify the asset chain produces the right state without
    /// firing up the windowed viewer.
    Info {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Optional: also write the populated [`SceneResources::vram`] to
        /// a 1024x512 RGBA8 PNG for offline comparison against a runtime
        /// VRAM dump (`mednafen-state vram-dump --out-bin`).
        #[arg(long)]
        vram_png: Option<PathBuf>,
        /// Optional: dump the raw 1 MiB BGR555 little-endian VRAM bytes to
        /// this path for pixel-exact diffs against a runtime capture.
        #[arg(long)]
        vram_bin: Option<PathBuf>,
        /// Optional: read a 1 MiB runtime VRAM blob (from
        /// `mednafen-state vram-dump --out-bin`) and report per-region
        /// coverage statistics: how many runtime non-zero rows the engine
        /// also populates, broken down by VRAM half (texture pages above
        /// y=256 vs framebuffers / scratch in the top half).
        #[arg(long)]
        runtime_vram: Option<PathBuf>,
        /// Optional: write a colour-coded diff PNG showing where engine
        /// VRAM matches / differs from the runtime VRAM. Used together
        /// with `--runtime-vram`.
        #[arg(long)]
        vram_diff_png: Option<PathBuf>,
        /// Optional: for every parsed TMD, walk the prim filter against
        /// the built VRAM and report kept / dropped counts. Surfaces
        /// "this mesh references VRAM the pre-pass didn't populate"
        /// failure modes without firing up the windowed viewer.
        #[arg(long)]
        tmd_stats: bool,
        /// Disable the asset-viewer-style targeted-VRAM upload. By
        /// default, each TIM's image and CLUT block are decided
        /// independently, suppressing the side that would clobber
        /// another mesh's data - this matches what the retail field
        /// loader does and dramatically reduces CLUT-row collisions on
        /// town / field scenes. Use `--no-targeted` to force the
        /// uniform-upload-every-TIM behaviour (legacy diagnostic mode).
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_targeted: bool,
    },
    /// Trace dropped CLUT references in a scene. Builds the scene's
    /// [`SceneResources`] with the default shared-block overlay, walks
    /// every TMD prim that drops as `MissingClut`, and reports which
    /// PROT entries on the disc carry a TIM whose CLUT block would
    /// supply each unique row. Use to discover which CDNAME blocks the
    /// engine needs to keep resident to lift the prim keep ratio.
    ///
    /// Optional `--runtime-vram <PATH>` cross-checks each missing row
    /// against the runtime VRAM ground truth (mednafen-state vram-dump
    /// --out-bin). Rows that are non-zero in the runtime VRAM but
    /// missing in the engine are the actionable gap.
    ClutTrace {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from
        /// a `.bin` disc image. When provided, `--extracted-root` is
        /// ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Optional runtime VRAM blob captured from a save state. When
        /// provided, the trace marks each missing row as "supplied in
        /// runtime VRAM" or "absent everywhere" - the former is where
        /// the engine's loader chain is incomplete.
        #[arg(long)]
        runtime_vram: Option<PathBuf>,
        /// Maximum number of PROT entries to report per missing CLUT row
        /// (multiple TIMs across the disc can supply the same row).
        #[arg(long, default_value_t = 4)]
        max_sources: usize,
    },
    /// Walk a scene's MAN partition-1 field-VM scripts (the per-actor
    /// interaction scripts; record 0 is the scene-entry system script) with
    /// the opcode-aware disassembler and report every `Yield` site whose
    /// trailing window decodes as an inline encounter record
    /// (`[reserved×3][count][monster_ids]`).
    ///
    /// This is the scripted-encounter hunt: it distinguishes a real inline
    /// `[count][ids]` arm at a decoded opcode boundary from the byte-scan
    /// false positives (every `0x37`/`0x41` byte in dialog text). For
    /// town01 the survey finds no inline literal — the opening Tetsu fight
    /// installs via the indexed formation table instead.
    ManScripts {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Print every partition-1 record (including the dialog-heavy
        /// interaction scripts), not just records carrying a decodable
        /// inline record.
        #[arg(long)]
        all: bool,
        /// Print the full field-VM opcode disassembly of one record (by
        /// index; in partition 1 record 0 is the scene-entry system script).
        /// Useful for tracing scene-entry / cutscene / dialog flow.
        #[arg(long)]
        disasm_record: Option<usize>,
        /// Partition the `--disasm-record` index lives in (default 1, the
        /// per-actor scripts). Use `2` to disassemble a cutscene-timeline
        /// record (e.g. opdeene's record 18, the prologue camera/actor/text
        /// sequence ending in `GFLAG_SET 26`).
        #[arg(long, default_value_t = 1)]
        disasm_partition: usize,
        /// Write the decoded (LZS-decompressed) MAN payload to this path so it
        /// can be scanned for embedded literals (e.g. a scripted scene-change
        /// target name). The scene-entry script + every partition's bytecode
        /// live in this blob.
        #[arg(long)]
        dump_man: Option<PathBuf>,
        /// Walk a partition's records as field-VM scripts and report their
        /// global-flag writes (`GFLAG_SET`/`GFLAG_CLEAR`). Partition 2 holds
        /// the cutscene-timeline records (e.g. opdeene's `GFLAG_SET 26`
        /// town01 hand-off arm). Reported at real opcode boundaries.
        #[arg(long)]
        gflag_partition: Option<usize>,
        /// Dump the inline cutscene-narration text pages embedded in a
        /// cutscene-timeline record (the `0x1F`/`0x00`-framed subtitle pages
        /// after a `0x4C` narration op). Pair with `--disasm-record N
        /// --disasm-partition 2` to target the right record; defaults to
        /// scanning every partition-2 record when no record is given.
        #[arg(long)]
        narration: bool,
    },
    /// Compare engine VRAM (built from the scene's targeted asset
    /// upload) against a runtime VRAM blob captured from a mednafen
    /// save state. Reports per-64x64-tile overlap and writes a
    /// colour-coded diff PNG (greyscale = exact match, blue = both
    /// non-zero but different, red = runtime-only, green = engine-only).
    ///
    /// Two modes:
    ///   - **Explicit**: pass `--scene` + `--runtime-vram` to point at a
    ///     CDNAME label and a 1 MiB VRAM dump produced by
    ///     `mednafen-state vram-dump --out-bin`.
    ///   - **Scenario**: pass `--scenario <label>` to resolve both from
    ///     `scripts/scenarios.toml`. The scene comes from
    ///     `expected_active_scene`, the VRAM is read live from the
    ///     scenario's `.mc{slot}` save via `legaia-mednafen`'s GPU
    ///     section parser. Optional `--frames N` boots a `BootSession`
    ///     and ticks the engine before sampling, so dynamic VRAM
    ///     uploads land before the diff. `--strict` asserts byte-exact
    ///     match in the texpage region (y ≥ 256) and fails with the
    ///     first divergent (row, col).
    VramOracle {
        /// CDNAME scene name (e.g. `town01`). Required in explicit mode;
        /// derived from the scenario's `expected_active_scene` in
        /// scenario mode.
        #[arg(long)]
        scene: Option<String>,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from
        /// a `.bin` disc image.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Explicit-mode VRAM blob: 1 MiB BGR555 LE bytes from a save
        /// state. Required when `--scenario` is not set; ignored when
        /// `--scenario` is set.
        #[arg(long)]
        runtime_vram: Option<PathBuf>,
        /// Scenario-mode entry: scenario label looked up in the
        /// manifest. Resolves scene + .mc save path automatically. The
        /// scenario must have `expected_active_scene` populated.
        #[arg(long)]
        scenario: Option<String>,
        /// Scenario manifest path. Default: `scripts/scenarios.toml`.
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
        /// Scenario mode only: number of engine frames to tick before
        /// sampling VRAM. Default 0 = pure pre-pass diff (no engine
        /// involvement, identical to the legacy explicit-mode build).
        /// Set > 0 to capture VRAM state after the engine has settled
        /// dynamic uploads.
        #[arg(long, default_value_t = 0)]
        frames: u64,
        /// Strict mode: assert byte-exact match in the texpage region
        /// (y ≥ 256). Reports first divergent (row, col) with hex
        /// values and exits non-zero. The framebuffer half (y < 256)
        /// is reported but not asserted - the engine port renders
        /// direct-to-wgpu, not to a simulated PSX framebuffer.
        #[arg(long, default_value_t = false)]
        strict: bool,
        /// Optional: write a 1024x512 RGBA8 colour-coded diff PNG.
        #[arg(long)]
        diff_png: Option<PathBuf>,
        /// Print per-tile (64x64) overlap counts instead of just bands.
        #[arg(long, default_value_t = false)]
        tiles: bool,
        /// Optional: write a per-row (Y=0..511) CSV of pixel-level diff
        /// stats. Columns: y, runtime_nz, engine_nz, overlap, runtime_only,
        /// engine_only. Useful for automated regression checks - drift
        /// in any single row above a threshold (e.g. row 479's NPC CLUT)
        /// surfaces as a high `runtime_only` count.
        #[arg(long)]
        rows_csv: Option<PathBuf>,
        /// Print a focused report on documented CLUT bands (NPC palette
        /// row 479, character / texture-page CLUT rows). One line per
        /// band with overlap percentage; non-zero "runtime_only" on a
        /// known band is the regression signature this catches.
        #[arg(long, default_value_t = false)]
        clut_regions: bool,
    },
    /// Phase-E3 mode-trace oracle. Boots a `BootSession` on the
    /// resolved scene, ticks it `--frames` times sampling
    /// `(scene_mode, active_scene)` per frame, emits the engine trace
    /// as JSONL, and (in scenario mode) compares the engine's settled
    /// state to a snapshot lifted from the matching mednafen
    /// `.mc{slot}` save.
    ///
    /// **Two modes:**
    ///   - **Explicit**: `--scene <NAME>` runs the engine alone and
    ///     emits its JSONL trace. No comparison.
    ///   - **Scenario**: `--scenario <LABEL>` resolves both the scene
    ///     (from `expected_active_scene` in `scripts/scenarios.toml`)
    ///     and the retail snapshot (from the `.mc{slot}` save). With
    ///     `--strict`, exits non-zero if no engine frame matches the
    ///     retail snapshot's `(scene_mode, active_scene)`.
    ///
    /// **Asymmetry.** The engine port doesn't model the 28-mode
    /// dispatcher yet, so engine-emitted frames have `game_mode = null`
    /// in the JSONL. Retail snapshots fill it from `_DAT_8007B83C`.
    /// See `crates/engine-shell/src/mode_trace_oracle.rs` for the
    /// long-form rationale.
    ModeTrace {
        /// CDNAME scene name (e.g. `town01`). Required in explicit
        /// mode; derived from the scenario's `expected_active_scene`
        /// in scenario mode.
        #[arg(long)]
        scene: Option<String>,
        /// Extracted-root directory containing `PROT.DAT` +
        /// `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly
        /// from a `.bin` disc image.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Scenario label looked up in the manifest. Resolves scene +
        /// `.mc` save automatically. The scenario must have
        /// `expected_active_scene` populated.
        #[arg(long)]
        scenario: Option<String>,
        /// Scenario manifest path.
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
        /// Engine frames to tick before sampling. Engine trace has
        /// `frames + 1` entries (one for boot state, one per tick).
        #[arg(long, default_value_t = 60)]
        frames: u64,
        /// Where to write the engine JSONL trace. Default `-` =
        /// stdout.
        #[arg(long, default_value = "-")]
        out: PathBuf,
        /// Strict mode: assert at least one engine frame matches the
        /// retail snapshot's `(scene_mode, active_scene)`. Exits
        /// non-zero with the last engine frame on divergence. Only
        /// valid in scenario mode.
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
    /// Audio-trace parity oracle. Boots a `BootSession` on the
    /// resolved scene, runs a private headless SPU + sequencer in
    /// parallel, ticks for `--frames` frames sampling
    /// `(voice_mask, voices[24], master_volume)` per frame, emits the
    /// engine trace as JSONL, and (in scenario mode) compares against
    /// the SPU section lifted from the matching mednafen `.mc{slot}`
    /// save.
    ///
    /// **Two modes:**
    ///   - **Explicit**: `--scene <NAME>` runs the engine alone and
    ///     emits its JSONL trace. No comparison.
    ///   - **Scenario**: `--scenario <LABEL>` resolves both the scene
    ///     (from `expected_active_scene`) and the retail snapshot
    ///     (from the `.mc{slot}` save's SPU section). With `--strict`,
    ///     exits non-zero on divergence.
    ///
    /// **Asymmetry.** A retail snapshot from a `.mc{slot}` save freezes
    /// one SPU cycle; the engine trace runs over a window. The
    /// `--retail-jsonl` mode consumes a multi-frame retail trace
    /// captured by `scripts/pcsx-redux/autorun_audio_trace.lua` +
    /// `scripts/pcsx-redux/extract_audio_trace_from_sstates.py`,
    /// flipping the comparison to multi-frame-vs-multi-frame.
    /// Convergence rule (per retail frame with active voices): some
    /// engine frame's mask must be a superset of retail's mask.
    ///
    /// **BGM playback.** The trace installs a private
    /// [`TraceBgmDirector`] and routes field-VM op `0x35` events into
    /// a headless sequencer, so the engine drives BGM the same way the
    /// retail engine does. `--bgm-id` is a manual boot-time override
    /// for scenes whose prescripts don't kick off audio.
    AudioTrace {
        /// CDNAME scene name (e.g. `town01`). Required in explicit
        /// mode; derived from the scenario's `expected_active_scene`
        /// in scenario mode.
        #[arg(long)]
        scene: Option<String>,
        /// Extracted-root directory containing `PROT.DAT` +
        /// `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly
        /// from a `.bin` disc image.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Scenario label looked up in the manifest. Resolves scene +
        /// `.mc` save automatically.
        #[arg(long)]
        scenario: Option<String>,
        /// Scenario manifest path.
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
        /// Engine frames to tick before sampling. Engine trace has
        /// `frames + 1` entries (one for boot state, one per tick).
        #[arg(long, default_value_t = 60)]
        frames: u64,
        /// Optional: BGM id started through the private sequencer
        /// before the trace loop begins. Use when a scene's prescript
        /// doesn't fire op `0x35` within the trace window.
        #[arg(long)]
        bgm_id: Option<u16>,
        /// Where to write the engine JSONL trace. Default `-` = stdout.
        #[arg(long, default_value = "-")]
        out: PathBuf,
        /// Multi-frame retail trace (JSONL) produced by
        /// `scripts/pcsx-redux/extract_audio_trace_from_sstates.py`.
        /// Overrides the scenario's `.mc{slot}` single-snapshot path
        /// for the convergence walk; `--scene` alone is enough in this
        /// mode (no `--scenario` lookup needed).
        #[arg(long)]
        retail_jsonl: Option<PathBuf>,
        /// Strict mode: exit non-zero on divergence between the
        /// engine trace and retail. Valid in scenario mode and in
        /// `--retail-jsonl` mode.
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
    /// PCM-window parity oracle - the I2 sibling of `audio-trace`.
    ///
    /// Emits stereo PCM windows from both sides at 44.1 kHz:
    ///
    ///   - **Engine**: boots a headless `BootSession`, runs a private
    ///     SPU + sequencer in parallel, routes field-VM op `0x35`
    ///     events through a `TraceBgmDirector`, accumulates per-frame
    ///     PCM over the trace window.
    ///   - **Retail**: lifts the SPU section from a mednafen
    ///     `.mc{slot}` save (or a path passed via `--retail-save`),
    ///     seeds a clean-room SPU through `engine_spu_from_retail`,
    ///     and renders one second of PCM. Voice mid-stream state is
    ///     not preserved by the translator (engine-audio's `Voice`
    ///     doesn't expose those internals), so this is "what the SPU
    ///     would play given retail's voice config" rather than a
    ///     bit-identical resume-from-snapshot.
    ///
    /// Two output flavours: WAV files (`--engine-wav`, `--retail-wav`)
    /// for human listening, plus stderr stats (peak / RMS /
    /// non-silent-sample count) for both sides.
    ///
    /// `--strict` enforces only the conservative bar exercised by the
    /// disc-gated `pcm_oracle` test: retail audible AND engine silent
    /// → exit non-zero. Anything finer is informational.
    PcmTrace {
        /// CDNAME scene name (e.g. `town01`). Required in explicit
        /// mode; derived from the scenario's `expected_active_scene`
        /// in scenario mode.
        #[arg(long)]
        scene: Option<String>,
        /// Extracted-root directory containing `PROT.DAT` +
        /// `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly
        /// from a `.bin` disc image.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Scenario label looked up in the manifest. Resolves scene +
        /// `.mc` save automatically.
        #[arg(long)]
        scenario: Option<String>,
        /// Scenario manifest path.
        #[arg(long, default_value = "scripts/scenarios.toml")]
        manifest: PathBuf,
        /// Engine frames to tick. PCM window length is `frames *
        /// 44_100 / 60` stereo samples.
        #[arg(long, default_value_t = 60)]
        frames: u64,
        /// Optional: BGM id started through the private sequencer
        /// before the trace loop begins. Use when a scene's prescript
        /// doesn't fire op `0x35` within the trace window.
        #[arg(long)]
        bgm_id: Option<u16>,
        /// Explicit retail save path. Overrides the scenario's `.mc`
        /// lookup; useful for ad-hoc captures.
        #[arg(long)]
        retail_save: Option<PathBuf>,
        /// Where to write the engine PCM (WAV). Default: skipped.
        #[arg(long)]
        engine_wav: Option<PathBuf>,
        /// Where to write the retail reference PCM (WAV). Default:
        /// skipped.
        #[arg(long)]
        retail_wav: Option<PathBuf>,
        /// Strict mode: exit non-zero when retail had audible output
        /// and the engine produced silence over the trace window.
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
    /// Run an engine playthrough headless from a `j-replay-v1`
    /// replay file (see `legaia_engine_shell::replay`). Drives a
    /// synthetic [`World`](legaia_engine_core::world::World) for
    /// `meta.frames` frames, samples per-frame `(scene_mode,
    /// active_scene)` into JSONL, and (with `--strict`) compares
    /// against the file's optional `[[expected]]` fixture.
    ///
    /// No disc required — the replay binds an RNG seed and a
    /// scenario label, not a CDNAME scene. This is Phase J's
    /// determinism + scripted-replay entry point; pair with `record`
    /// to capture human input from a play-window session.
    Replay {
        /// Path to the `j-replay-v1` replay file.
        #[arg(long)]
        input: PathBuf,
        /// Where to write the engine JSONL trace. Default `-` = stdout.
        #[arg(long, default_value = "-")]
        out: PathBuf,
        /// Strict mode: exit non-zero on the first divergence between
        /// the recorded engine trace and the replay's `[[expected]]`
        /// fixture (if present).
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
    /// Open a play-window session that captures pad transitions into
    /// a `j-replay-v1` file on close. Thin shim over `play-window`:
    /// every flag carries the same meaning, plus `--out` for the
    /// replay file path.
    Record {
        /// Where to write the captured replay.
        #[arg(long)]
        out: PathBuf,
        /// Starting scene name (CDNAME label). Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from
        /// a `.bin` disc image. When provided, `--extracted-root` is
        /// ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Disable audio output. Useful for CI / headless capture where
        /// cpal can't enumerate a device.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Open the world map controller instead of a field scene.
        #[arg(long, default_value_t = false)]
        world_map: bool,
        /// Save directory the record's save-select reads / writes.
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        /// Optional scenario label to bind into the recorded file's
        /// `meta.scenario` field. Replays preserve this so paired
        /// scenario state can be looked up from `scripts/scenarios.toml`.
        #[arg(long)]
        scenario: Option<String>,
        /// Initial RNG seed to bake into the recorded file's
        /// `meta.rng_seed` field. Default matches the engine's
        /// canonical `0xDEADC0DE`.
        #[arg(long, default_value_t = 0xDEAD_C0DE)]
        rng_seed: u32,
    },
    /// List every distinct scene name the CDNAME map exposes, with the
    /// PROT entry range each one covers.
    ListScenes {
        /// Extracted-root directory containing `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read CDNAME.TXT directly from a `.bin`
        /// disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
    },
    /// Save the current world's empty/default party to a slot file.
    /// Useful as a smoke test for the disk save path; engines drive this
    /// from menu mode at runtime.
    Save {
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Number of party-record entries to materialise in the save.
        #[arg(long, default_value_t = 3)]
        party_size: usize,
    },
    /// Load a slot file into a fresh world and print the resulting roster
    /// shape. Mirror of `save` for round-trip testing.
    Load {
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        slot: u8,
    },
    /// Boot the engine into a scene and tick it for `frames` frames.
    /// Drives the field VM, camera, BGM director, and per-actor move VMs;
    /// logs scene transitions and the per-frame BGM events. No window -
    /// for that, use `asset-viewer field <scene>`.
    ///
    /// When `--str-file` is provided the STR video is pre-decoded headlessly
    /// (frame count logged) before scene ticking begins. The scene label
    /// patterns that identify in-engine cutscenes (as opposed to FMV) are
    /// described by `engine_core::scene::is_cutscene_label`.
    Play {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Number of engine frames to run before exiting. `0` runs
        /// indefinitely.
        #[arg(long, default_value_t = 600)]
        frames: u64,
        /// Disable audio output. Useful for CI / headless smoke tests
        /// where cpal can't enumerate a device.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Per-frame sleep in milliseconds. Default 16 ms ≈ 60 FPS for a
        /// realtime feel; set to `0` for "as fast as possible" smoke runs.
        #[arg(long, default_value_t = 16)]
        frame_ms: u64,
        /// Optional path to a raw PSX STR file. When provided, the video is
        /// pre-decoded headlessly and the frame count is printed before scene
        /// ticking begins. Use for `op*`/`ed*` scenes paired with FMV files.
        #[arg(long)]
        str_file: Option<PathBuf>,
        /// Optional TOML mapping CDNAME labels to MV*.STR paths. When set,
        /// the engine consults this map first and falls through to the
        /// hard-coded heuristic for unmapped labels. Format:
        ///
        /// ```toml
        /// [scenes]
        /// opdeene = "MOV/MV1.STR"
        /// edteien = "MOV/MV6.STR"
        /// ```
        #[arg(long)]
        cutscene_map: Option<PathBuf>,
    },
    /// Open a window, boot a scene, and run the engine with rendering.
    /// Accepts keyboard input (arrows = D-pad, Z = Cross, Esc = quit).
    ///
    /// When `--str-file` is provided the STR video plays first in a windowed
    /// player (same as `play-str`). After the video window closes the scene
    /// window opens and runs normally.
    PlayWindow {
        /// Starting scene name. Default: `town01`.
        #[arg(long, default_value = "town01")]
        scene: String,
        /// Extracted-root directory.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Alternative source: read PROT.DAT + CDNAME.TXT directly from a
        /// `.bin` disc image. When provided, `--extracted-root` is ignored.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Disable audio output.
        #[arg(long, default_value_t = false)]
        no_audio: bool,
        /// Enable world-map mode: installs a WorldMapController and shows
        /// the top-view camera globals in the HUD. Arrow keys scroll the
        /// top-view camera; Q/W adjust azimuth; A/S adjust zoom.
        #[arg(long, default_value_t = false)]
        world_map: bool,
        /// Optional path to a raw PSX STR file. When provided, the STR video
        /// plays in a window first (phase 1); the scene window opens after
        /// the video window closes (phase 2).
        #[arg(long)]
        str_file: Option<PathBuf>,
        /// Open the boot UI flow before entering the scene: title screen
        /// → save-select on Continue → field/encounter/battle on
        /// New Game. The default (`false`) behaviour is the legacy
        /// "jump straight to the scene" path.
        #[arg(long, default_value_t = false)]
        boot_ui: bool,
        /// Save directory used by `--boot-ui` for the save-select pass.
        #[arg(long, default_value = "saves")]
        save_dir: PathBuf,
        /// Optional TOML CDNAME→STR map; same format as `play --cutscene-map`.
        #[arg(long)]
        cutscene_map: Option<PathBuf>,
        /// Optional GameShark `.gs.txt` or Mednafen `.cht` cheat file.
        /// Each entry is parsed and applied once at boot through the
        /// `legaia_engine_core::cheat_applier` registry. Per-entry
        /// status is logged to stderr. Conditional codes (`D0`/`E0`)
        /// are treated as always-true unless `--cheat-strict` is set.
        #[arg(long)]
        cheat_file: Option<PathBuf>,
        /// When `--cheat-file` is set, honour conditional codes
        /// strictly: skip every write that follows a `D0`/`E0` gate
        /// the engine doesn't emulate. Default is to apply
        /// conditionals as always-true (which is what the player
        /// expects for "Save Anywhere" / "Status Modifier Menu" /
        /// etc. style cheats).
        #[arg(long, default_value_t = false)]
        cheat_strict: bool,
        /// Enable the live gameplay loop: walking a field scene rolls
        /// step-driven random encounters, transitions Field -> Battle,
        /// resolves the battle, and returns to the field with loot on
        /// victory. Without this the scene only runs the field VM +
        /// locomotion (the legacy "explore but never fight" behaviour).
        #[arg(long, default_value_t = false)]
        live_loop: bool,
        /// Make battles player-driven (implies `--live-loop`): each party
        /// turn opens a battle command menu - select Attack (Up/Down) and a
        /// target (Left/Right), confirm with Cross - before the strike
        /// commits, instead of the loop auto-attacking. v0.1 enables only
        /// the Attack command.
        #[arg(long, default_value_t = false)]
        player_battle: bool,
        /// Route field NPC dialogue through the inline-script field-VM runner
        /// (branch handlers execute their flag-sets / scene-changes) instead of
        /// the simplified typewriter. Up/Down navigate a menu, Cross confirms.
        #[arg(long, default_value_t = false)]
        vm_dialogue: bool,
        /// Make the player follow the per-scene terrain elevation: each field
        /// locomotion step snaps the player's Y to the floor-height sample
        /// (`FUN_80019278`) at the new tile. Off by default (flat Y); enable to
        /// see slopes / steps. No effect on the world-map walk.
        #[arg(long, default_value_t = false)]
        terrain_y: bool,
        /// Route live basic-attack damage through the retail damage finisher
        /// (`FUN_801ddb30`): adds the 9999 cap and the rand-based no-damage
        /// floor on top of the raw roll. Off by default (flat path, 0xFFFF cap,
        /// min-floor 1). Equipment resistance / guard aren't modelled yet, so
        /// only the cap + floor stages contribute today.
        #[arg(long, default_value_t = false)]
        damage_finish: bool,
        /// BGM id to cross-fade to when a live-loop encounter starts; the
        /// field track resumes when the battle ends. Routed through the same
        /// BGM director as field op-`0x35` starts, so the id must resolve in
        /// the current scene's asset table. Omit to leave music untouched
        /// across the Battle transition.
        #[arg(long)]
        battle_bgm: Option<u16>,
    },
    /// Open a window and play back a PSX STR movie using the MDEC decoder,
    /// paced at the stream's real ~15 fps.
    ///
    /// Without `--disc` it plays a raw filesystem STR file (2048-byte Form-1
    /// sectors, video only - the `legaia-extract` shape). With `--disc <bin>`
    /// the `<str_file>` argument is an ISO path inside the disc image (e.g.
    /// `MOV/MV1.STR`); the movie is read as raw 2352-byte sectors so its
    /// interleaved XA audio track plays, with the video clock driven off the
    /// audio cursor for A/V sync.
    PlayStr {
        /// STR file to play. Without `--disc` this is a raw filesystem path
        /// (2048-byte Form-1 sectors, video only - the extracted shape).
        /// With `--disc` it is the ISO path of the movie inside the disc
        /// image (e.g. `MOV/MV1.STR`), read as raw 2352-byte sectors so the
        /// interleaved XA audio track plays in sync with the video.
        #[arg()]
        str_file: PathBuf,
        /// Disc image (`.bin`). When set, `str_file` is resolved as an ISO
        /// path inside it and the cutscene plays with its interleaved audio
        /// (the video clock is driven off the audio cursor).
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Window width.
        #[arg(long, default_value_t = 640)]
        width: u32,
        /// Window height.
        #[arg(long, default_value_t = 480)]
        height: u32,
    },
    /// Show or update the keyboard-to-pad-button input mapping.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Drive a synthetic battle round end-to-end: party of 3 vs N
    /// monsters, headless ticking through `BattleSession` phases.
    /// Reports per-phase events for inspection.
    Battle {
        /// Number of monster slots (1..=5). Each is initialised with HP
        /// equal to `--monster-hp`.
        #[arg(long, default_value_t = 1)]
        monsters: u8,
        /// Per-monster initial HP.
        #[arg(long, default_value_t = 50)]
        monster_hp: u16,
        /// Maximum number of session ticks to run before exiting.
        #[arg(long, default_value_t = 256)]
        max_ticks: u64,
        /// Pre-seeded turn script - comma-separated key letters fed once
        /// per tick during the CommandInput phase. Each character maps
        /// to one input bit:
        ///   `R/L/U/D` direction; `c` cross; `o` circle; `t` triangle;
        ///   `s` square (Spirit); `S` start (commit). All other chars
        ///   advance one tick with no input. Default empty.
        #[arg(long, default_value = "")]
        script: String,
    },
    /// Drive an inventory-use session against a synthetic World. Prints
    /// the cursor moves + commit outcome.
    Inventory {
        /// Item id used by the synthetic session (default 0x01 = Healing Leaf).
        #[arg(long, default_value_t = 0x01)]
        item: u8,
        /// Number of party members.
        #[arg(long, default_value_t = 3)]
        party_size: u8,
        /// Pre-seeded input sequence (same letters as `Battle`).
        #[arg(long, default_value = "cc")]
        script: String,
    },
    /// Drive an equip session for a synthetic character. Reports state
    /// transitions + the final committed equipment row.
    Equip {
        /// Slot to edit (0..=7).
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Item id to equip into `slot` (must be present in the synthetic
        /// inventory).
        #[arg(long, default_value_t = 0x05)]
        item: u8,
    },
    /// Replay a recorded GTE (cop2) trace file against a fresh emulator
    /// and report per-step register divergences. Useful for validating
    /// the emulator against captured retail RAM dumps.
    GteReplay {
        /// JSON trace path written by `engine-render::gte_trace::Cop2Trace`.
        #[arg(long)]
        trace: PathBuf,
        /// Print mismatch detail even when the trace replays cleanly
        /// (default off - silence is success).
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Drive a synthetic title screen → main-menu pick session.
    /// Reports per-tick events as the scripted input drives the SM.
    Title {
        /// Pre-seeded input sequence, one character per tick. `s` = start,
        /// `c` = cross, `o` = circle, `U`/`D` = up/down. All other chars
        /// advance one tick with no input.
        #[arg(long, default_value = "ssDc")]
        script: String,
        /// Treat the session as having no save data (Continue disabled).
        #[arg(long, default_value_t = false)]
        no_save: bool,
        /// Frames to spend in the fade-in phase before accepting input.
        #[arg(long, default_value_t = 4)]
        fade_frames: u16,
    },
    /// Drive a synthetic save-select session.
    SaveSelect {
        /// Mode: `load` (pick a non-empty slot) or `save` (pick any slot).
        #[arg(long, default_value = "load")]
        mode: String,
        /// Comma-separated slot presence mask (1 = present, 0 = empty).
        #[arg(long, default_value = "1,0,1")]
        slots: String,
        /// Pre-seeded input sequence (same letters as `Title`).
        #[arg(long, default_value = "cc")]
        script: String,
    },
    /// Roll a synthetic encounter session against a small table for `steps`
    /// steps. Reports the first triggered encounter (if any).
    Encounter {
        /// Trigger rate in 1/256 (default 64 ≈ 25%).
        #[arg(long, default_value_t = 64)]
        rate: u8,
        /// Number of steps to roll.
        #[arg(long, default_value_t = 100)]
        steps: u32,
        /// RNG seed (deterministic).
        #[arg(long, default_value_t = 0xDEAD_BEEF)]
        seed: u32,
    },
    /// Drive a synthetic battle target picker. Reports cursor moves +
    /// the resulting outcome.
    TargetPick {
        /// Target kind: one of `enemy`, `ally`, `ally-or-self`,
        /// `dead-ally`, `any-ally`, `all-enemies`, `all-allies`, `self`.
        #[arg(long, default_value = "enemy")]
        kind: String,
        /// Active actor slot (0..=2).
        #[arg(long, default_value_t = 0)]
        actor: u8,
        /// Pre-seeded input sequence.
        #[arg(long, default_value = "RRc")]
        script: String,
    },
    /// Drive a synthetic Tactical Arts chain editor session.
    ChainEditor {
        /// Character slot (0..=2).
        #[arg(long, default_value_t = 0)]
        char_slot: u8,
        /// Pre-seeded input sequence (`L`/`R`/`U`/`D` push directions;
        /// `c` = cross, `o` = circle, `t` = triangle, `n` = name-next).
        #[arg(long, default_value = "cLLLcc")]
        script: String,
    },
    /// Run the full Seru capture flow against the vanilla registry: roll
    /// `count` captures of a given Seru and report the resulting learn
    /// events.
    SeruCapture {
        /// Seru id to capture (default 1 = Spark).
        #[arg(long, default_value_t = 1)]
        seru: u16,
        /// Number of captures to roll.
        #[arg(long, default_value_t = 4)]
        count: u32,
        /// Comma-separated party slots (default `0,1,2`).
        #[arg(long, default_value = "0,1,2")]
        party: String,
    },
    /// Run the engine integration scenarios manifest. Loads
    /// `scripts/engine/scenarios.toml` (or the path under `--manifest`),
    /// boots each scenario headlessly, and asserts the SHA-256 of the
    /// resulting `SaveFile` byte stream matches the recorded
    /// `expected_save_sha256`. Use `--bless` to record observed hashes
    /// into the manifest in place. See
    /// `crates/engine-shell/src/scenarios.rs`.
    Scenarios {
        /// Manifest path. Defaults to `scripts/engine/scenarios.toml`
        /// relative to the cwd.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// Rewrite the manifest in place with observed hashes for any
        /// scenario whose recorded hash differs (or is empty).
        #[arg(long, default_value_t = false)]
        bless: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Print the current input mapping to stdout.
    Show {
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
    /// Set a single key binding. KEY is the user-friendly key name (e.g.
    /// `Z`, `Up`, `Enter`, `RShift`); BUTTON is the PSX pad button name
    /// (e.g. `Cross`, `Circle`, `Start`, `L1`).
    Set {
        /// Binding in KEY=BUTTON form, e.g. `--binding Z=Cross`.
        #[arg(long)]
        binding: String,
        /// Path to the TOML config file (default: `legaia-input.toml`).
        #[arg(long, default_value = "legaia-input.toml")]
        config_file: PathBuf,
    },
    /// Write the heuristic CDNAME→MV cutscene map to a TOML file.
    /// Useful as a starting point for engines that want to override one
    /// or two entries while keeping the rest of the heuristic intact.
    DumpCutsceneMap {
        /// Output path (use `-` for stdout).
        #[arg(long, default_value = "legaia-cutscene-map.toml")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            scene,
            extracted_root,
            disc,
            vram_png,
            vram_bin,
            runtime_vram,
            vram_diff_png,
            tmd_stats,
            no_targeted,
        } => cmd_info(
            &scene,
            &extracted_root,
            disc.as_deref(),
            vram_png.as_deref(),
            vram_bin.as_deref(),
            runtime_vram.as_deref(),
            vram_diff_png.as_deref(),
            tmd_stats,
            !no_targeted,
        ),
        Cmd::ListScenes {
            extracted_root,
            disc,
        } => cmd_list_scenes(&extracted_root, disc.as_deref()),
        Cmd::ClutTrace {
            scene,
            extracted_root,
            disc,
            runtime_vram,
            max_sources,
        } => cmd_clut_trace(
            &scene,
            &extracted_root,
            disc.as_deref(),
            runtime_vram.as_deref(),
            max_sources,
        ),
        Cmd::ManScripts {
            scene,
            extracted_root,
            disc,
            all,
            disasm_record,
            disasm_partition,
            dump_man,
            gflag_partition,
            narration,
        } => cmd_man_scripts(
            &scene,
            &extracted_root,
            disc.as_deref(),
            all,
            disasm_record,
            disasm_partition,
            dump_man.as_deref(),
            gflag_partition,
            narration,
        ),
        Cmd::VramOracle {
            scene,
            extracted_root,
            disc,
            runtime_vram,
            scenario,
            manifest,
            frames,
            strict,
            diff_png,
            tiles,
            rows_csv,
            clut_regions,
        } => cmd_vram_oracle(VramOracleArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            runtime_vram: runtime_vram.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            strict,
            diff_png: diff_png.as_deref(),
            tiles,
            rows_csv: rows_csv.as_deref(),
            clut_regions,
        }),
        Cmd::ModeTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            out,
            strict,
        } => cmd_mode_trace(ModeTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            out: &out,
            strict,
        }),
        Cmd::AudioTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            bgm_id,
            out,
            retail_jsonl,
            strict,
        } => cmd_audio_trace(AudioTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            bgm_id,
            out: &out,
            retail_jsonl: retail_jsonl.as_deref(),
            strict,
        }),
        Cmd::PcmTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            bgm_id,
            retail_save,
            engine_wav,
            retail_wav,
            strict,
        } => cmd_pcm_trace(PcmTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            bgm_id,
            retail_save: retail_save.as_deref(),
            engine_wav: engine_wav.as_deref(),
            retail_wav: retail_wav.as_deref(),
            strict,
        }),
        Cmd::Replay { input, out, strict } => cmd_replay(&input, &out, strict),
        Cmd::Record {
            out,
            scene,
            extracted_root,
            disc,
            no_audio,
            world_map,
            save_dir,
            scenario,
            rng_seed,
        } => cmd_record(
            &out,
            &scene,
            &extracted_root,
            disc.as_deref(),
            !no_audio,
            world_map,
            &save_dir,
            scenario.as_deref(),
            rng_seed,
        ),
        Cmd::Play {
            scene,
            extracted_root,
            disc,
            frames,
            no_audio,
            frame_ms,
            str_file,
            cutscene_map,
        } => cmd_play(
            &scene,
            &extracted_root,
            disc.as_deref(),
            frames,
            !no_audio,
            frame_ms,
            str_file.as_deref(),
            cutscene_map.as_deref(),
        ),
        Cmd::PlayWindow {
            scene,
            extracted_root,
            disc,
            no_audio,
            world_map,
            str_file,
            boot_ui,
            save_dir,
            cutscene_map,
            cheat_file,
            cheat_strict,
            live_loop,
            player_battle,
            vm_dialogue,
            terrain_y,
            damage_finish,
            battle_bgm,
        } => cmd_play_window(
            &scene,
            &extracted_root,
            disc.as_deref(),
            !no_audio,
            world_map,
            str_file.as_deref(),
            boot_ui,
            &save_dir,
            cutscene_map.as_deref(),
            cheat_file.as_deref(),
            cheat_strict,
            live_loop,
            player_battle,
            vm_dialogue,
            terrain_y,
            damage_finish,
            battle_bgm,
        ),
        Cmd::Save {
            extracted_root,
            disc,
            save_dir,
            slot,
            party_size,
        } => cmd_save(
            &extracted_root,
            disc.as_deref(),
            &save_dir,
            slot,
            party_size,
        ),
        Cmd::Load { save_dir, slot } => cmd_load(&save_dir, slot),
        Cmd::PlayStr {
            str_file,
            disc,
            width,
            height,
        } => cmd_play_str(&str_file, disc.as_deref(), width, height),
        Cmd::Config { cmd } => cmd_config(cmd),
        Cmd::Battle {
            monsters,
            monster_hp,
            max_ticks,
            script,
        } => cmd_battle(monsters, monster_hp, max_ticks, &script),
        Cmd::Inventory {
            item,
            party_size,
            script,
        } => cmd_inventory(item, party_size, &script),
        Cmd::Equip { slot, item } => cmd_equip(slot, item),
        Cmd::GteReplay { trace, verbose } => cmd_gte_replay(&trace, verbose),
        Cmd::Title {
            script,
            no_save,
            fade_frames,
        } => cmd_title(&script, no_save, fade_frames),
        Cmd::SaveSelect {
            mode,
            slots,
            script,
        } => cmd_save_select(&mode, &slots, &script),
        Cmd::Encounter { rate, steps, seed } => cmd_encounter(rate, steps, seed),
        Cmd::TargetPick {
            kind,
            actor,
            script,
        } => cmd_target_pick(&kind, actor, &script),
        Cmd::ChainEditor { char_slot, script } => cmd_chain_editor(char_slot, &script),
        Cmd::SeruCapture { seru, count, party } => cmd_seru_capture(seru, count, &party),
        Cmd::Scenarios {
            manifest,
            extracted_root,
            bless,
        } => cmd_scenarios(manifest.as_deref(), &extracted_root, bless),
    }
}

fn cmd_scenarios(
    manifest_override: Option<&Path>,
    extracted_root: &Path,
    bless: bool,
) -> Result<()> {
    use legaia_engine_shell::scenarios::{
        ScenariosManifest, bless as bless_manifest, default_manifest_path, run_all,
    };

    let manifest_path = manifest_override
        .map(PathBuf::from)
        .unwrap_or_else(default_manifest_path);
    let manifest = ScenariosManifest::from_toml_path(&manifest_path)?;
    println!(
        "engine scenarios: manifest={} ({} scenarios)  extracted_root={}",
        manifest_path.display(),
        manifest.scenarios.len(),
        extracted_root.display()
    );
    let results = run_all(&manifest, extracted_root)?;

    let mut passed = 0;
    let mut failed = 0;
    let mut unblessed = 0;
    for r in &results {
        match (&r.expected_sha256, r.passed()) {
            (None, _) => {
                unblessed += 1;
                println!(
                    "  [unblessed]   {:<32} scene={:<8} frames={:>3}  observed={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
            }
            (Some(_), true) => {
                passed += 1;
                println!(
                    "  [ok]          {:<32} scene={:<8} frames={:>3}  hash={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
            }
            (Some(exp), false) => {
                failed += 1;
                println!(
                    "  [DRIFT]       {:<32} scene={:<8} frames={:>3}",
                    r.name, r.scene, r.frames
                );
                println!("                expected:  {exp}");
                println!("                observed:  {}", r.observed_sha256);
            }
        }
    }
    println!("summary: {passed} passed, {failed} drifted, {unblessed} unblessed");

    if bless {
        let updated = bless_manifest(&manifest_path, &results)?;
        println!(
            "blessed: {updated} hash row(s) updated in {}",
            manifest_path.display()
        );
    }

    if failed > 0 {
        anyhow::bail!("{failed} scenario(s) drifted from manifest");
    }
    if unblessed > 0 && !bless {
        anyhow::bail!("{unblessed} scenario(s) need blessing - rerun with --bless after review");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_info(
    scene_name: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    vram_png: Option<&Path>,
    vram_bin: Option<&Path>,
    runtime_vram: Option<&Path>,
    vram_diff_png: Option<&Path>,
    tmd_stats: bool,
    targeted: bool,
) -> Result<()> {
    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let assets = SceneAssets::build(&scene);

    // Load the field-shared blocks (`init_data`, `player_data`) when we
    // can, so the engine VRAM mirrors the retail boot-then-scene layout.
    // Missing blocks (e.g. when running against a region whose CDNAME
    // doesn't carry one of the names) skip with a warning rather than
    // failing - the comparison still works against the rest.
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        match Scene::load(&index, name) {
            Ok(s) => shared_scenes.push(s),
            Err(e) => eprintln!("warning: shared block '{name}' not loaded: {e}"),
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (resources, targeted_stats) = if targeted {
        let (r, s) = SceneResources::build_targeted(&scene, &shared_refs)?;
        (r, Some(s))
    } else {
        (
            SceneResources::build_with_shared(&scene, &shared_refs)?,
            None,
        )
    };

    println!("scene '{}'", scene.name);
    println!(
        "  CDNAME range:           PROT [{}..{})",
        scene.start, scene.end
    );
    println!("  entries swept:          {}", scene.entries.len());
    println!(
        "  shared blocks loaded:   {:?}",
        shared_scenes
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    println!(
        "  TIMs uploaded to VRAM:  {} (scene-local: {}, shared: {}, parse failures: {})",
        resources.tim_count,
        resources.tim_count - resources.shared_tim_count,
        resources.shared_tim_count,
        resources.tim_parse_failures
    );
    println!(
        "  TMDs parsed:            {} (scene-local: {}, shared: {})",
        resources.tmds.len(),
        resources.tmds.len() - resources.shared_tmd_count,
        resources.shared_tmd_count
    );
    println!(
        "  MES container:          {}",
        if assets.mes.is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  SEQ entries (raw):      {} (in stream wrappers: {})",
        assets.seq_entries.len(),
        assets.seq_in_stream_entries.len()
    );
    println!("  VAB entries:            {}", assets.vab_entries.len());
    println!("  Event-script records:   {}", assets.event_records.len());
    if let Some(s) = &targeted_stats {
        println!(
            "  targeted VRAM upload:   total_tims={} uploaded={} both={} image_only={} clut_only={}",
            s.total_tims,
            s.uploaded_tims,
            s.uploaded_both,
            s.uploaded_image_only,
            s.uploaded_clut_only
        );
    }

    if tmd_stats {
        println!("  per-TMD filter stats (drop reasons):");
        let mut total_kept = 0usize;
        let mut total_miss_clut = 0usize;
        let mut total_depth_mismatch = 0usize;
        let mut total_miss_page = 0usize;
        let mut total_skipped = 0usize;
        for (i, rtmd) in resources.tmds.iter().enumerate() {
            let (_mesh, stats) = rtmd.build_filtered_vram_mesh_reasoned(&resources.vram);
            total_kept += stats.kept;
            total_miss_clut += stats.missing_clut;
            total_depth_mismatch += stats.clut_depth_mismatch;
            total_miss_page += stats.missing_texture_page;
            total_skipped += stats.skipped_bad_vert_index + stats.skipped_untextured;
            println!(
                "    tmd[{i:2}] entry={:4} off=0x{:06X}  kept={:4} miss_clut={:3} depth_mm={:3} miss_page={:4} no_uv={:3}  keep={:5.1}%",
                rtmd.entry_idx,
                rtmd.offset,
                stats.kept,
                stats.missing_clut,
                stats.clut_depth_mismatch,
                stats.missing_texture_page,
                stats.skipped_untextured,
                100.0 * stats.keep_ratio()
            );
        }
        let textured = total_kept + total_miss_clut + total_depth_mismatch + total_miss_page;
        let aggregate_keep = if textured > 0 {
            100.0 * total_kept as f32 / textured as f32
        } else {
            100.0
        };
        println!(
            "  aggregate filter:        kept={} miss_clut={} depth_mm={} miss_page={} skipped={} (textured kept={:.1}%)",
            total_kept,
            total_miss_clut,
            total_depth_mismatch,
            total_miss_page,
            total_skipped,
            aggregate_keep
        );
    }

    if vram_png.is_some() || vram_bin.is_some() || runtime_vram.is_some() {
        let engine_bytes = vram_to_le_bytes(&resources.vram);
        if let Some(p) = vram_png {
            write_vram_png(p, &engine_bytes)?;
            println!("[ok] wrote engine VRAM PNG to {}", p.display());
        }
        if let Some(p) = vram_bin {
            std::fs::write(p, &engine_bytes)
                .with_context(|| format!("writing engine VRAM bin to {}", p.display()))?;
            println!(
                "[ok] wrote engine VRAM bin to {} ({} bytes)",
                p.display(),
                engine_bytes.len()
            );
        }
        if let Some(p) = runtime_vram {
            let runtime_bytes = std::fs::read(p)
                .with_context(|| format!("reading runtime VRAM blob from {}", p.display()))?;
            if runtime_bytes.len() != engine_bytes.len() {
                anyhow::bail!(
                    "runtime VRAM size {} != expected {} (1 MiB BGR555)",
                    runtime_bytes.len(),
                    engine_bytes.len()
                );
            }
            let report = vram_coverage_report(&engine_bytes, &runtime_bytes);
            print_vram_coverage(&report);
            if let Some(diff_path) = vram_diff_png {
                write_vram_diff_png(diff_path, &engine_bytes, &runtime_bytes)?;
                println!("[ok] wrote VRAM diff PNG to {}", diff_path.display());
            }
        } else if vram_diff_png.is_some() {
            eprintln!("warning: --vram-diff-png requires --runtime-vram; skipping diff");
        }
    }
    Ok(())
}

/// Walk a scene's TMD pool, locate every primitive that drops as
/// `MissingClut`, and report which PROT entries on the disc carry a TIM
/// whose CLUT block lands at the missing row. Optional runtime VRAM
/// cross-check distinguishes "row absent from the engine but present at
/// runtime" (engine loader gap) from "row absent from runtime too"
/// (mesh references unreachable CLUT - likely a parser issue).
fn cmd_clut_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    runtime_vram: Option<&Path>,
    max_sources: usize,
) -> Result<()> {
    use legaia_asset::tim_scan;
    use legaia_tim::vram::PrimTextureStatus;
    use std::collections::BTreeMap;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;

    let mut shared_scenes: Vec<Scene> = Vec::new();
    for name in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(&index, name) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let (resources, _upload_stats) = SceneResources::build_targeted(&scene, &shared_refs)?;

    println!("scene '{}'", scene.name);
    println!(
        "  shared blocks loaded: {:?}",
        shared_scenes
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    println!(
        "  TMDs: {}  TIMs uploaded: {}",
        resources.tmds.len(),
        resources.tim_count
    );

    // Group dropped prims by (cba, depth). Multiple prims in multiple TMDs
    // often share the same CLUT row; we only need to find the supplier
    // once per unique row.
    let mut dropped: BTreeMap<(u16, u8), DroppedClut> = BTreeMap::new();
    for rtmd in &resources.tmds {
        for obj in &rtmd.tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                &rtmd.raw,
                obj.primitives_byte_offset,
                obj.primitives_byte_size,
            );
            for g in &groups {
                for prim in &g.prims {
                    if prim.uvs.is_empty() {
                        continue;
                    }
                    let depth = match (prim.tsb >> 7) & 0x3 {
                        0 => 4u8,
                        1 => 8,
                        _ => continue, // 16bpp / direct: no CLUT to be missing
                    };
                    let status = resources
                        .vram
                        .prim_texture_status(prim.cba, prim.tsb, &prim.uvs);
                    if let PrimTextureStatus::MissingClut { .. } = status {
                        let entry = dropped.entry((prim.cba, depth)).or_default();
                        entry.prim_count += 1;
                        entry.tmd_locations.insert((rtmd.entry_idx, rtmd.offset));
                    }
                }
            }
        }
    }

    if dropped.is_empty() {
        println!("  no MissingClut drops detected in this scene");
        return Ok(());
    }

    let runtime_bytes = match runtime_vram {
        Some(p) => Some(
            std::fs::read(p).with_context(|| format!("read runtime VRAM blob {}", p.display()))?,
        ),
        None => None,
    };
    if let Some(b) = &runtime_bytes
        && b.len() != 1024 * 512 * 2
    {
        anyhow::bail!("runtime VRAM size {} != 1 MiB (1024*512*2)", b.len());
    }

    // Pre-scan every PROT entry once: collect (entry_idx, cba_fb_x,
    // cba_fb_y, depth). One pass through the disc; subsequent lookups
    // are cheap.
    println!("  scanning PROT corpus for CLUT suppliers ...");
    let mut suppliers: Vec<TimSupplier> = Vec::new();
    for idx in 0..index.entry_count() as u32 {
        let Ok(bytes) = index.entry_bytes(idx) else {
            continue;
        };
        for hit in tim_scan::scan_buffer(&bytes) {
            let Ok(tim) = legaia_tim::parse(&bytes[hit.offset..hit.offset + hit.byte_len]) else {
                continue;
            };
            let Some(clut) = tim.clut.as_ref() else {
                continue;
            };
            suppliers.push(TimSupplier {
                entry_idx: idx,
                offset: hit.offset,
                fb_x: clut.fb_x,
                fb_y: clut.fb_y,
                width: clut.w,
                bpp: hit.bpp,
            });
        }
    }
    println!(
        "  scanned {} PROT entries, found {} TIMs with CLUT blocks",
        index.entry_count(),
        suppliers.len()
    );

    // For each unique missing (cba, depth) report what we found.
    println!();
    println!(
        "  {} unique missing CLUT row(s) across the scene's TMDs:",
        dropped.len()
    );
    let mut supplier_entries: BTreeMap<u32, BTreeMap<&str, ()>> = BTreeMap::new();
    let mut shared_block_recommend: BTreeMap<&'static str, u32> = BTreeMap::new();
    for ((cba, depth), info) in &dropped {
        let cx = (cba & 0x3F) * 16;
        let cy = (cba >> 6) & 0x1FF;
        let clut_w: usize = match depth {
            4 => 16,
            8 => 256,
            _ => 0,
        };
        let in_runtime = match runtime_bytes.as_ref() {
            Some(b) => row_has_data(b, cx as usize, cy as usize, clut_w),
            None => false,
        };

        // Match by rectangle CONTAINMENT - a TIM CLUT block covers the
        // missing slot if its (fb_x, fb_y, width, 1) rect contains
        // (cx, cy). PSX games commonly pack 16 distinct 4bpp palettes
        // into one 256-wide CLUT block, so the CBA's 16-pixel slot
        // sits inside a wider supplier rect.
        let matching: Vec<&TimSupplier> = suppliers
            .iter()
            .filter(|s| s.fb_y == cy && s.fb_x <= cx && (cx + clut_w as u16) <= (s.fb_x + s.width))
            .collect();

        println!(
            "    cba=0x{:04X} depth={}bpp clut@({:4},{:3}) prims={:4} tmds={:2} runtime_has_row={}",
            cba,
            depth,
            cx,
            cy,
            info.prim_count,
            info.tmd_locations.len(),
            in_runtime
        );
        if matching.is_empty() {
            println!("      ! no PROT entry on disc supplies this row");
        } else {
            for s in matching.iter().take(max_sources) {
                let scene_name = index.scene_for_index(s.entry_idx).unwrap_or("?");
                supplier_entries
                    .entry(s.entry_idx)
                    .or_default()
                    .insert(scene_name, ());
                if let Some(static_name) = known_scene_block_for(scene_name) {
                    *shared_block_recommend.entry(static_name).or_default() += 1;
                }
                println!(
                    "      supplier: PROT {:4} ({}) off=0x{:06X} clut_w={} bpp={}",
                    s.entry_idx, scene_name, s.offset, s.width, s.bpp
                );
            }
            if matching.len() > max_sources {
                println!(
                    "      ... {} more supplier(s) suppressed",
                    matching.len() - max_sources
                );
            }
        }
    }

    println!();
    println!("  PROT entries the engine would need to keep resident:");
    for (entry, scenes) in &supplier_entries {
        let scene_list = scenes.keys().copied().collect::<Vec<_>>().join(", ");
        println!("    PROT {entry:4} (scene blocks: {scene_list})");
    }

    if !shared_block_recommend.is_empty() {
        println!();
        println!("  recommended FIELD_SHARED_BLOCKS additions (by supplier hit count):");
        for (name, hits) in &shared_block_recommend {
            println!("    \"{name}\"   (supplies {hits} missing row(s))");
        }
    }

    Ok(())
}

/// Map a free-form CDNAME scene label to a stable shared-block name
/// the engine knows how to load. Conservative: only return a name if it
/// matches one of the well-known shared blocks we'd actually pin into
/// VRAM, not a per-scene town/field block.
fn known_scene_block_for(scene_name: &str) -> Option<&'static str> {
    match scene_name {
        "init_data" => Some("init_data"),
        "player_data" => Some("player_data"),
        "battle_data" => Some("battle_data"),
        "befect_data" => Some("befect_data"),
        "sound_data" => Some("sound_data"),
        "sound_data2" => Some("sound_data2"),
        "gameover_data" => Some("gameover_data"),
        "card_data" => Some("card_data"),
        _ => None,
    }
}

#[derive(Default)]
struct DroppedClut {
    prim_count: usize,
    tmd_locations: std::collections::BTreeSet<(u32, usize)>,
}

struct TimSupplier {
    entry_idx: u32,
    offset: usize,
    fb_x: u16,
    fb_y: u16,
    width: u16,
    bpp: u32,
}

/// True when any of the next `w` 16-bit words starting at `(x, y)` in
/// the 1 MiB BGR555 LE blob are non-zero. Used by `cmd_clut_trace` to
/// decide whether the runtime captured this CLUT row.
fn row_has_data(blob: &[u8], x: usize, y: usize, w: usize) -> bool {
    const VW: usize = 1024;
    const VH: usize = 512;
    if y >= VH {
        return false;
    }
    let row_start = (y * VW + x) * 2;
    let end = ((x + w).min(VW) * 2) + y * VW * 2;
    let end = end.min(blob.len());
    if row_start >= end {
        return false;
    }
    let mut i = row_start;
    while i + 1 < end {
        if blob[i] != 0 || blob[i + 1] != 0 {
            return true;
        }
        i += 2;
    }
    false
}

/// Side-by-side compare of engine VRAM (built via the scene's targeted
/// upload) against a runtime VRAM blob from a save state. Reports the
/// per-band overlap and per-tile (64x64) diff if `tiles` is set; writes
/// the colour-coded diff PNG when `diff_png` is provided.
#[allow(clippy::too_many_arguments)]
struct VramOracleArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    runtime_vram: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    strict: bool,
    diff_png: Option<&'a Path>,
    tiles: bool,
    rows_csv: Option<&'a Path>,
    clut_regions: bool,
}

/// Resolves the (scene_name, runtime_bytes, source_label) triple from
/// either explicit args or a scenario lookup. Scenario mode reads the
/// VRAM blob in-process via `legaia-mednafen`'s GPU section parser, so
/// no temp file is needed.
///
/// In scenario mode with `frames > 0`, additionally boots a
/// [`BootSession`] on the resolved scene and ticks it `frames` times
/// before returning the sampled engine VRAM. This catches dynamic
/// uploads (NPC palette swaps, fog ramps, per-frame CLUT mutations)
/// that the pure pre-pass doesn't see.
fn resolve_vram_inputs(args: &VramOracleArgs<'_>) -> Result<ResolvedVram> {
    use legaia_mednafen::ScenarioManifest;

    match (args.scenario, args.scene, args.runtime_vram) {
        (Some(label), _, _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene",)
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let runtime_bytes = load_runtime_vram_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedVram {
                scene_name,
                runtime_bytes,
                source_label,
            })
        }
        (None, Some(scene_name), Some(runtime_path)) => {
            let runtime_bytes = std::fs::read(runtime_path)
                .with_context(|| format!("read runtime VRAM blob {}", runtime_path.display()))?;
            Ok(ResolvedVram {
                scene_name: scene_name.to_owned(),
                runtime_bytes,
                source_label: runtime_path.display().to_string(),
            })
        }
        _ => anyhow::bail!(
            "vram-oracle: provide either `--scenario <label>` or both `--scene` + `--runtime-vram`"
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_man_scripts(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    all: bool,
    disasm_record: Option<usize>,
    disasm_partition: usize,
    dump_man: Option<&Path>,
    gflag_partition: Option<usize>,
    narration: bool,
) -> Result<()> {
    use legaia_engine_core::man_field_scripts::{
        partition_record_span, walk_partition_gflag_sites, walk_partition1_scripts,
    };
    use legaia_engine_core::scene_bundle;

    let index = open_index_from_args(extracted_root, disc)?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let bundle = scene_bundle::find_bundle(&scene).with_context(|| {
        format!("scene '{scene_name}' has no scene_asset_table bundle (no MAN)")
    })?;
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .with_context(|| format!("entry bytes for PROT[{}]", bundle.entry_idx()))?;
    let man = scene_bundle::extract_man_payload(&bundle, &entry_bytes)?
        .with_context(|| format!("scene '{scene_name}' MAN payload did not decode"))?;
    let man_file = legaia_asset::man_section::parse(&man)?;

    if let Some(path) = dump_man {
        std::fs::write(path, &man)
            .with_context(|| format!("write decoded MAN to {}", path.display()))?;
        println!(
            "wrote decoded MAN payload ({} bytes) to {}",
            man.len(),
            path.display()
        );
    }

    let records = walk_partition1_scripts(&man_file, &man);
    println!(
        "scene '{}' (PROT[{}]): {} partition-1 records, counts {:?}",
        scene.name,
        bundle.entry_idx(),
        records.len(),
        man_file.header.partition_counts,
    );

    let mut total_yields = 0usize;
    let mut total_records = 0usize;
    let mut tetsu = 0usize;
    for rec in &records {
        total_yields += rec.arm_sites.len();
        let candidates: Vec<_> = rec.encounter_arm_candidates().collect();
        total_records += candidates.len();
        let show = all || !candidates.is_empty() || rec.index == 0;
        if show {
            println!(
                "  P1[{:3}] start=0x{:05X} pc0={:3} body={:5}b insns={:4} errs={:3} yields={} candidates={}",
                rec.index,
                rec.script_start,
                rec.pc0,
                rec.body_len,
                rec.insn_count,
                rec.decode_errors,
                rec.arm_sites.len(),
                candidates.len(),
            );
        }
        for site in &rec.arm_sites {
            let Some(record) = site.record else { continue };
            if site.matches_tetsu() {
                tetsu += 1;
            }
            if show {
                println!(
                    "      yield 0x{:02X}{} @ 0x{:05X}  window={:02X?}  -> count={} ids={:02X?}{}",
                    site.opcode,
                    if site.wide { "(wide)" } else { "" },
                    site.abs_pc,
                    site.window,
                    record.count,
                    &record.monster_ids[..record.count as usize],
                    if site.matches_tetsu() {
                        "  <<< Tetsu (count=1 id=0x4F)"
                    } else {
                        ""
                    },
                );
            }
        }
    }
    println!(
        "summary: {} yield sites, {} decode as inline records, {} match the Tetsu signature",
        total_yields, total_records, tetsu,
    );

    if let Some(target) = disasm_record {
        use legaia_engine_vm::field_disasm::{LinearWalker, format_instruction};
        let (script_start, pc0, body_len) =
            partition_record_span(&man_file, &man, disasm_partition, target).with_context(
                || format!("partition {disasm_partition} record {target} has no decodable span"),
            )?;
        let end = script_start + body_len;
        let body = man
            .get(script_start..end)
            .with_context(|| format!("record {target} body slice out of range"))?;
        println!(
            "\n--- disasm P{disasm_partition}[{target}] (start=0x{script_start:05X} pc0={pc0} body={body_len}b) ---",
        );
        for insn in LinearWalker::new(body, pc0) {
            match insn {
                Ok(insn) => println!(
                    "  0x{:05X} (+0x{:04X})  {}",
                    script_start + insn.pc,
                    insn.pc,
                    format_instruction(&insn, body),
                ),
                Err((pc, e)) => {
                    let raw = body.get(pc).copied().unwrap_or(0);
                    println!(
                        "  0x{:05X} (+0x{:04X})  .byte 0x{raw:02X}  [{e:?}]",
                        script_start + pc,
                        pc,
                    );
                }
            }
        }
    }

    if let Some(partition) = gflag_partition {
        let sites = walk_partition_gflag_sites(&man_file, &man, partition);
        println!(
            "\n--- GFLAG writes in partition {partition} ({} sites) ---",
            sites.len(),
        );
        for s in &sites {
            println!(
                "  P{}[{}] GFLAG.{} bit={:<2} @ 0x{:05X} (op 0x{:02X})",
                s.partition,
                s.record,
                if s.set { "Set  " } else { "Clear" },
                s.bit,
                s.abs_pc,
                s.opcode,
            );
        }
    }

    if narration {
        use legaia_asset::cutscene_text::parse_narration;
        // Either a specific `--disasm-record` in `--disasm-partition`, or a
        // sweep of every record in `disasm_partition` (defaulting to 2, the
        // cutscene-timeline partition).
        let candidates: Vec<usize> = match disasm_record {
            Some(r) => vec![r],
            None => {
                let count = man_file
                    .header
                    .partition_counts
                    .get(disasm_partition)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                (0..count).collect()
            }
        };
        println!("\n--- inline cutscene narration (partition {disasm_partition}) ---",);
        let mut total = 0usize;
        for r in candidates {
            let Some((script_start, _pc0, body_len)) =
                partition_record_span(&man_file, &man, disasm_partition, r)
            else {
                continue;
            };
            let body = &man[script_start..script_start + body_len];
            let blocks = parse_narration(body);
            for (bi, block) in blocks.iter().enumerate() {
                total += block.pages.len();
                println!(
                    "  P{disasm_partition}[{r}] block {bi} @ 0x{:05X}: declared {} page(s), decoded {}{}",
                    script_start + block.op_offset,
                    block.declared_pages,
                    block.pages.len(),
                    if block.count_matches() {
                        ""
                    } else {
                        "  [count mismatch]"
                    },
                );
                for page in &block.pages {
                    println!(
                        "      0x{:05X}  {:?}",
                        script_start + page.offset,
                        page.text
                    );
                }
            }
        }
        println!("summary: {total} narration page(s) total");
    }
    Ok(())
}

struct ResolvedVram {
    scene_name: String,
    runtime_bytes: Vec<u8>,
    source_label: String,
}

fn cmd_vram_oracle(args: VramOracleArgs<'_>) -> Result<()> {
    let resolved = resolve_vram_inputs(&args)?;
    let engine_bytes = build_engine_vram_bytes_with_frames(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
    )?;
    let runtime_bytes = resolved.runtime_bytes;
    if runtime_bytes.len() != engine_bytes.len() {
        anyhow::bail!(
            "runtime VRAM size {} != expected {} (1 MiB BGR555)",
            runtime_bytes.len(),
            engine_bytes.len()
        );
    }

    let report = vram_coverage_report(&engine_bytes, &runtime_bytes);
    println!(
        "scene '{}'  vs runtime {}  (frames={})",
        resolved.scene_name, resolved.source_label, args.frames
    );
    print_vram_coverage(&report);
    let diff_png = args.diff_png;
    let tiles = args.tiles;
    let rows_csv = args.rows_csv;
    let clut_regions = args.clut_regions;

    if tiles {
        println!("  per-64x64-tile coverage (runtime non-zero / engine non-zero / overlap):");
        const W: usize = 1024;
        const H: usize = 512;
        for ty in 0..(H / 64) {
            for tx in 0..(W / 64) {
                let mut rt = 0u32;
                let mut en = 0u32;
                let mut ov = 0u32;
                for dy in 0..64 {
                    let y = ty * 64 + dy;
                    for dx in 0..64 {
                        let x = tx * 64 + dx;
                        let off = (y * W + x) * 2;
                        let rw = u16::from_le_bytes([runtime_bytes[off], runtime_bytes[off + 1]]);
                        let ew = u16::from_le_bytes([engine_bytes[off], engine_bytes[off + 1]]);
                        if rw != 0 {
                            rt += 1;
                        }
                        if ew != 0 {
                            en += 1;
                        }
                        if rw != 0 && ew != 0 {
                            ov += 1;
                        }
                    }
                }
                if rt > 0 || en > 0 {
                    println!(
                        "    tile ({:>3},{:>3})  rt={:5}  en={:5}  ov={:5}",
                        tx * 64,
                        ty * 64,
                        rt,
                        en,
                        ov
                    );
                }
            }
        }
    }

    if let Some(p) = diff_png {
        write_vram_diff_png(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote VRAM diff PNG to {}", p.display());
    }

    if let Some(p) = rows_csv {
        write_vram_rows_csv(p, &engine_bytes, &runtime_bytes)?;
        println!("[ok] wrote per-row VRAM CSV to {}", p.display());
    }

    if clut_regions {
        print_vram_clut_region_report(&engine_bytes, &runtime_bytes);
    }

    if args.strict {
        match first_texpage_divergence(&engine_bytes, &runtime_bytes) {
            None => {
                println!("[strict] texpage region (y >= 256): byte-exact match");
            }
            Some(TexpageDivergence {
                y,
                x,
                engine_word,
                runtime_word,
            }) => {
                anyhow::bail!(
                    "[strict] texpage region diverged at row {y} col {x}: engine=0x{engine_word:04X} runtime=0x{runtime_word:04X}",
                );
            }
        }
    }

    Ok(())
}

/// Phase-E3 mode-trace oracle - args struct for `cmd_mode_trace`.
struct ModeTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    out: &'a Path,
    strict: bool,
}

/// Resolved input triple - `(scene_name, retail_snapshot, source_label)`.
/// `retail_snapshot` is `None` in explicit mode (no comparison).
struct ResolvedModeTrace {
    scene_name: String,
    retail: Option<ModeTraceFrame>,
    source_label: String,
}

fn resolve_mode_trace_inputs(args: &ModeTraceArgs<'_>) -> Result<ResolvedModeTrace> {
    use legaia_mednafen::ScenarioManifest;

    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene",)
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let retail = load_runtime_mode_trace_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedModeTrace {
                scene_name,
                retail: Some(retail),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedModeTrace {
            scene_name: scene_name.to_owned(),
            retail: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!("mode-trace: provide either `--scenario <label>` or `--scene <name>`"),
    }
}

fn cmd_mode_trace(args: ModeTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() {
        anyhow::bail!(
            "mode-trace: `--strict` requires `--scenario` (no retail snapshot in explicit mode)"
        );
    }
    let resolved = resolve_mode_trace_inputs(&args)?;
    let trace = build_engine_mode_trace(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
    )?;
    let jsonl = mode_trace_to_jsonl(&trace);

    let out_label = if args.out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(args.out, jsonl.as_bytes())
            .with_context(|| format!("write mode-trace JSONL to {}", args.out.display()))?;
        args.out.display().to_string()
    };

    eprintln!(
        "scene '{}' vs {} (frames={}, trace_len={})  -> {}",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        trace.len(),
        out_label
    );

    if let Some(retail) = resolved.retail.as_ref() {
        let last = trace.last().unwrap();
        eprintln!(
            "  engine[last] scene_mode={:<10} active_scene={:?}",
            last.scene_mode, last.active_scene
        );
        eprintln!(
            "  retail       scene_mode={:<10} active_scene={:?}  game_mode={:?} ({})",
            retail.scene_mode,
            retail.active_scene,
            retail.game_mode,
            retail.game_mode_name.as_deref().unwrap_or("?"),
        );
        match first_mode_trace_divergence(&trace, retail) {
            None => {
                eprintln!("[ok] engine trace converges with retail snapshot");
            }
            Some(d) => {
                let msg = format!(
                    "[DRIFT] {:?}: engine(scene_mode={}, active_scene={:?}) vs retail(scene_mode={}, active_scene={:?})",
                    d.kind,
                    d.engine.scene_mode,
                    d.engine.active_scene,
                    d.retail.scene_mode,
                    d.retail.active_scene,
                );
                if args.strict {
                    anyhow::bail!("{msg}");
                } else {
                    eprintln!("{msg}");
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// audio-trace - audio parity oracle
// ---------------------------------------------------------------------------

/// Audio-trace oracle - args struct for `cmd_audio_trace`.
struct AudioTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    bgm_id: Option<u16>,
    out: &'a Path,
    retail_jsonl: Option<&'a Path>,
    strict: bool,
}

/// Resolved retail input for the convergence walk.
enum ResolvedRetail {
    /// Scenario-mode single SPU snapshot lifted from a mednafen `.mc{slot}`
    /// save. Compared via [`first_audio_trace_divergence`].
    Snapshot(AudioTraceFrame),
    /// Multi-frame trace lifted from a PCSX-Redux per-vsync capture (Lua
    /// probe → Python extractor → JSONL). Compared via
    /// [`first_audio_trace_divergence_multi`].
    Multi(Vec<AudioTraceFrame>),
}

/// Resolved input triple - `(scene_name, retail, source_label)`.
/// `retail` is `None` in explicit mode (no comparison).
struct ResolvedAudioTrace {
    scene_name: String,
    retail: Option<ResolvedRetail>,
    source_label: String,
}

fn resolve_audio_trace_inputs(args: &AudioTraceArgs<'_>) -> Result<ResolvedAudioTrace> {
    use legaia_mednafen::ScenarioManifest;

    // The retail-JSONL path is the multi-frame mode; it doesn't require a
    // scenario lookup because the JSONL is self-contained.
    if let Some(jsonl_path) = args.retail_jsonl {
        let scene_name = match (args.scenario, args.scene) {
            (Some(label), _) => {
                let manifest = ScenarioManifest::from_path(args.manifest)?;
                let scn = manifest.by_label(label).with_context(|| {
                    format!("scenario {label:?} not in {}", args.manifest.display())
                })?;
                scn.expected_active_scene.clone().with_context(|| {
                    format!(
                        "scenario {label:?} has no `expected_active_scene`; cannot derive scene"
                    )
                })?
            }
            (None, Some(name)) => name.to_owned(),
            _ => anyhow::bail!(
                "audio-trace --retail-jsonl: provide `--scene` or `--scenario` for the engine side"
            ),
        };
        let frames = load_runtime_audio_trace_jsonl(jsonl_path)?;
        let source_label = format!(
            "retail-jsonl {} ({} frame(s))",
            jsonl_path.display(),
            frames.len()
        );
        return Ok(ResolvedAudioTrace {
            scene_name,
            retail: Some(ResolvedRetail::Multi(frames)),
            source_label,
        });
    }

    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene")
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let retail = load_runtime_audio_trace_from_save(&save_path)?;
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedAudioTrace {
                scene_name,
                retail: Some(ResolvedRetail::Snapshot(retail)),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedAudioTrace {
            scene_name: scene_name.to_owned(),
            retail: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!("audio-trace: provide either `--scenario <label>` or `--scene <name>`"),
    }
}

fn cmd_audio_trace(args: AudioTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() && args.retail_jsonl.is_none() {
        anyhow::bail!(
            "audio-trace: `--strict` requires `--scenario` or `--retail-jsonl` (no retail in explicit mode)"
        );
    }
    let resolved = resolve_audio_trace_inputs(&args)?;
    let trace = engine_trace_from_paths(
        &resolved.scene_name,
        args.extracted_root,
        args.disc,
        args.frames,
        args.bgm_id,
    )?;
    let jsonl = audio_trace_to_jsonl(&trace);

    let out_label = if args.out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(args.out, jsonl.as_bytes())
            .with_context(|| format!("write audio-trace JSONL to {}", args.out.display()))?;
        args.out.display().to_string()
    };

    eprintln!(
        "scene '{}' vs {} (frames={}, trace_len={}, bgm_id={:?})  -> {}",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        trace.len(),
        args.bgm_id,
        out_label
    );

    let divergence = match resolved.retail.as_ref() {
        None => return Ok(()),
        Some(ResolvedRetail::Snapshot(retail)) => {
            let last = trace.last().unwrap();
            eprintln!(
                "  engine[last] mask=0b{:024b} master={:?} reverb_mode={:?}",
                last.active_voice_mask, last.master_volume, last.reverb_mode,
            );
            eprintln!(
                "  retail       mask=0b{:024b} master={:?} reverb_mode={:?}",
                retail.active_voice_mask, retail.master_volume, retail.reverb_mode,
            );
            first_audio_trace_divergence(&trace, retail)
        }
        Some(ResolvedRetail::Multi(retail_frames)) => {
            let retail_active = retail_frames
                .iter()
                .filter(|f| f.active_voice_mask != 0)
                .count();
            eprintln!(
                "  retail-trace frames={} ({} with active voices)",
                retail_frames.len(),
                retail_active,
            );
            first_audio_trace_divergence_multi(&trace, retail_frames)
        }
    };

    match divergence {
        None => eprintln!("[ok] engine trace converges with retail"),
        Some(d) => {
            let msg = format!(
                "[DRIFT] {:?}: engine(mask=0b{:024b}) vs retail(mask=0b{:024b})",
                d.kind, d.engine.active_voice_mask, d.retail.active_voice_mask,
            );
            if args.strict {
                anyhow::bail!("{msg}");
            } else {
                eprintln!("{msg}");
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// pcm-trace - PCM-window parity oracle (I2 sibling of audio-trace)
// ---------------------------------------------------------------------------

struct PcmTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    bgm_id: Option<u16>,
    retail_save: Option<&'a Path>,
    engine_wav: Option<&'a Path>,
    retail_wav: Option<&'a Path>,
    strict: bool,
}

struct ResolvedPcmTrace {
    scene_name: String,
    retail_save: Option<PathBuf>,
    source_label: String,
}

fn resolve_pcm_trace_inputs(args: &PcmTraceArgs<'_>) -> Result<ResolvedPcmTrace> {
    use legaia_mednafen::ScenarioManifest;

    // Explicit `--retail-save` always wins; needs `--scene` to know what
    // to boot.
    if let Some(save) = args.retail_save {
        let scene_name = args.scene.with_context(
            || "pcm-trace: `--retail-save` requires `--scene` (no scenario lookup)",
        )?;
        if !save.exists() {
            anyhow::bail!("pcm-trace: retail save not found at {}", save.display());
        }
        return Ok(ResolvedPcmTrace {
            scene_name: scene_name.to_owned(),
            retail_save: Some(save.to_path_buf()),
            source_label: format!("explicit save ({})", save.display()),
        });
    }
    match (args.scenario, args.scene) {
        (Some(label), _) => {
            let manifest = ScenarioManifest::from_path(args.manifest)?;
            let scn = manifest.by_label(label).with_context(|| {
                format!("scenario {label:?} not in {}", args.manifest.display())
            })?;
            let scene_name = scn.expected_active_scene.clone().with_context(|| {
                format!("scenario {label:?} has no `expected_active_scene`; cannot derive scene")
            })?;
            let save_path = manifest.save_path(scn.slot)?;
            if !save_path.exists() {
                anyhow::bail!(
                    "scenario {label:?} slot {} save not found at {}",
                    scn.slot,
                    save_path.display()
                );
            }
            let source_label = format!(
                "scenario {label:?} (slot {}, {})",
                scn.slot,
                save_path.display()
            );
            Ok(ResolvedPcmTrace {
                scene_name,
                retail_save: Some(save_path),
                source_label,
            })
        }
        (None, Some(scene_name)) => Ok(ResolvedPcmTrace {
            scene_name: scene_name.to_owned(),
            retail_save: None,
            source_label: "explicit (no retail comparison)".into(),
        }),
        _ => anyhow::bail!(
            "pcm-trace: provide either `--scenario`, `--scene`, or `--retail-save` + `--scene`"
        ),
    }
}

fn cmd_pcm_trace(args: PcmTraceArgs<'_>) -> Result<()> {
    if args.strict && args.scenario.is_none() && args.retail_save.is_none() {
        anyhow::bail!(
            "pcm-trace: `--strict` requires a retail source (`--scenario` or `--retail-save`)"
        );
    }
    let resolved = resolve_pcm_trace_inputs(&args)?;

    let opts = legaia_engine_shell::audio_trace_oracle::AudioTraceBuildOptions {
        scene: resolved.scene_name.clone(),
        bgm_id: args.bgm_id,
        us_per_frame: 1_000_000.0 / 60.0,
        frames: args.frames,
    };
    let engine: EnginePcmTrace = build_engine_pcm_trace(args.extracted_root, args.disc, &opts)?;
    let engine_stats = pcm_stats(&engine.pcm);

    if let Some(path) = args.engine_wav {
        write_wav(path, &engine.pcm)?;
    }

    eprintln!(
        "scene '{}' vs {} (frames={}, samples_per_frame={}, total_samples={})",
        resolved.scene_name,
        resolved.source_label,
        args.frames,
        engine.samples_per_frame,
        engine.pcm.len() / 2,
    );
    eprintln!(
        "  engine peak_abs={} rms={} non_silent_samples={} sample_pairs={}",
        engine_stats.peak_abs,
        engine_stats.rms,
        engine_stats.non_silent_samples,
        engine_stats.sample_pairs,
    );

    let Some(save_path) = resolved.retail_save.as_deref() else {
        return Ok(());
    };
    let retail = retail_reference_pcm(save_path, engine.pcm.len() / 2)?;
    let retail_stats = pcm_stats(&retail);
    if let Some(path) = args.retail_wav {
        write_wav(path, &retail)?;
    }

    eprintln!(
        "  retail peak_abs={} rms={} non_silent_samples={} sample_pairs={}",
        retail_stats.peak_abs,
        retail_stats.rms,
        retail_stats.non_silent_samples,
        retail_stats.sample_pairs,
    );

    // Conservative byte-level inspection: report first divergence at a
    // generous tolerance so callers see "is engine even close" without
    // false-positive spam.
    if let Some(d) = first_pcm_divergence(&engine.pcm, &retail, 4096) {
        eprintln!(
            "  first divergence sample_pair={} channel={} engine={} retail={} delta={}",
            d.sample_pair, d.channel, d.engine, d.retail, d.delta,
        );
    } else {
        eprintln!("  engine and retail PCM agree within +/-4096 on every sample");
    }

    let hard_fail = retail_stats.rms >= 256 && engine_stats.rms == 0;
    if hard_fail {
        let msg = format!(
            "[FAIL] retail had audible output (rms={}) but engine produced complete silence over {} frames",
            retail_stats.rms, args.frames,
        );
        if args.strict {
            anyhow::bail!("{msg}");
        } else {
            eprintln!("{msg}");
        }
    } else if engine_stats.rms == 0 {
        eprintln!(
            "[ok-quiet] retail also quiet (rms={}) - soft pass",
            retail_stats.rms
        );
    } else {
        eprintln!(
            "[ok] engine produced non-zero PCM (rms={})",
            engine_stats.rms
        );
    }

    // PcmStats / EnginePcmTrace are re-exported but the CLI doesn't
    // otherwise need them; reference the type to avoid an unused-import
    // warning on the `EnginePcmTrace` binding.
    let _ = std::mem::size_of::<PcmStats>();
    Ok(())
}

// ---------------------------------------------------------------------------
// replay (J3 — headless engine playback from a `j-replay-v1` file)
// ---------------------------------------------------------------------------

/// Drive a synthetic [`World`] from a [`ReplayFile`] and write the
/// resulting mode-trace JSONL. This mirrors the J2 determinism-gate
/// harness verbatim - the gate asserts byte-identity across two runs of
/// the same input, so the subcommand is just "the determinism gate's
/// driver, plus JSONL output".
///
/// `--strict` exits non-zero when the recorded trace disagrees with the
/// replay file's `[[expected]]` fixture; without it, divergence is
/// printed to stderr but doesn't fail.
fn cmd_replay(input: &Path, out: &Path, strict: bool) -> Result<()> {
    let replay = ReplayFile::from_path(input)?;
    let trace = synthetic_replay_trace(&replay);
    let jsonl = mode_trace_to_jsonl(&trace);
    let out_label = if out.as_os_str() == "-" {
        print!("{jsonl}");
        "<stdout>".to_string()
    } else {
        std::fs::write(out, jsonl.as_bytes())
            .with_context(|| format!("write replay trace JSONL to {}", out.display()))?;
        out.display().to_string()
    };
    eprintln!(
        "replay '{}' (frames={}, events={}, expected={}) -> {}",
        input.display(),
        replay.meta.frames,
        replay.events.len(),
        replay.expected.len(),
        out_label,
    );
    if let Some(d) = replay.diff(&trace) {
        let msg = format!(
            "[DRIFT] frame={} kind={:?}: expected(scene_mode={}, active_scene={:?}) vs recorded(scene_mode={}, active_scene={:?})",
            d.frame,
            d.kind,
            d.expected.scene_mode,
            d.expected.active_scene,
            d.recorded.scene_mode,
            d.recorded.active_scene,
        );
        if strict {
            anyhow::bail!("{msg}");
        }
        eprintln!("{msg}");
    } else if !replay.expected.is_empty() {
        eprintln!("[ok] recorded trace matches replay [[expected]] fixture");
    }
    Ok(())
}

/// Build the engine-side mode trace by driving a synthetic [`World`]
/// through `replay`'s frame count. Mirrors
/// `crates/engine-shell/tests/determinism_j2.rs::build_mode_trace` so
/// the subcommand's behaviour is the same the determinism gate tests.
fn synthetic_replay_trace(replay: &ReplayFile) -> Vec<ModeTraceFrame> {
    let pad_stream = replay.expand_pad_stream();
    let mut world = legaia_engine_core::world::World::new();
    while world.actors.len() < 8 {
        world
            .actors
            .push(legaia_engine_core::world::Actor::default());
    }
    world.rng_state = replay.meta.rng_seed;
    let mut out = Vec::with_capacity(pad_stream.len());
    out.push(synthetic_replay_sample(&world));
    for _ in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push(synthetic_replay_sample(&world));
    }
    out
}

fn synthetic_replay_sample(world: &legaia_engine_core::world::World) -> ModeTraceFrame {
    ModeTraceFrame {
        frame: world.frame,
        game_mode: None,
        game_mode_name: None,
        scene_mode: synthetic_replay_scene_mode_name(world.mode).to_string(),
        active_scene: None,
    }
}

fn synthetic_replay_scene_mode_name(m: legaia_engine_core::world::SceneMode) -> &'static str {
    use legaia_engine_core::world::SceneMode;
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
    }
}

// ---------------------------------------------------------------------------
// record (J3 — wraps `play-window` with pad-mask capture into ReplayFile)
// ---------------------------------------------------------------------------

/// Thin shim that opens a `play-window` session with the pad-capture
/// hook armed. Identical UX to `play-window`; the only added behaviour
/// is that every pad-mask transition is appended to a `Vec<PadEvent>`
/// on `PlayWindowApp` and flushed to `out` as a `j-replay-v1` file on
/// window close.
#[allow(clippy::too_many_arguments)]
fn cmd_record(
    out: &Path,
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    save_dir: &Path,
    scenario: Option<&str>,
    rng_seed: u32,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        None,
        false,
        save_dir,
        None,
        None,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
        Some(RecordTarget {
            out: out.to_path_buf(),
            scenario: scenario.map(str::to_string),
            rng_seed,
        }),
    )
}

/// Bundle of "where to write the captured replay + how to label it".
/// Threaded through into [`PlayWindowApp::record_log`] so the keyboard
/// handler can append events and the close handler can flush.
struct RecordTarget {
    out: std::path::PathBuf,
    scenario: Option<String>,
    rng_seed: u32,
}

/// Per-tick recorded-pad-event buffer + flush state. Lives on
/// [`PlayWindowApp`] when the user invoked the `record` subcommand;
/// `None` for plain `play-window` runs so the keyboard handler pays
/// nothing in the common case.
struct RecordLog {
    out_path: std::path::PathBuf,
    events: Vec<PadEvent>,
    /// Previous pad value the log saw. The keyboard handler dedups so a
    /// stream of "press, press, press" key events from auto-repeat
    /// collapses to a single PadEvent.
    last_pad: u16,
    scenario: Option<String>,
    rng_seed: u32,
    /// Highest frame index observed during the run. Used to populate
    /// `meta.frames` so the on-disk file faithfully describes the
    /// recorded duration.
    last_frame: u64,
    /// Once the file has been written, additional Close events become
    /// no-ops (winit can deliver CloseRequested + the loop's exit drop
    /// both).
    flushed: bool,
}

impl RecordLog {
    fn from_target(target: RecordTarget) -> Self {
        Self {
            out_path: target.out,
            events: Vec::new(),
            last_pad: 0,
            scenario: target.scenario,
            rng_seed: target.rng_seed,
            last_frame: 0,
            flushed: false,
        }
    }

    /// Record a pad transition iff `pad` differs from the previously
    /// logged value. Caller is responsible for emitting events in
    /// frame-ascending order (the keyboard handler always does).
    fn record_transition(&mut self, frame: u64, pad: u16) {
        if pad == self.last_pad {
            return;
        }
        self.events.push(PadEvent { frame, pad });
        self.last_pad = pad;
        if frame > self.last_frame {
            self.last_frame = frame;
        }
    }

    /// Note the frame counter advanced past `frame` without a pad
    /// change. Keeps `meta.frames` honest when the user closes the
    /// window with no input held.
    fn observe_frame(&mut self, frame: u64) {
        if frame > self.last_frame {
            self.last_frame = frame;
        }
    }

    /// Flush to disk. Idempotent.
    fn flush(&mut self) -> Result<()> {
        if self.flushed {
            return Ok(());
        }
        let meta = ReplayMeta {
            schema: legaia_engine_shell::replay::REPLAY_SCHEMA_V1.to_string(),
            scenario: self.scenario.clone(),
            rng_seed: self.rng_seed,
            frames: self.last_frame,
        };
        let mut file = ReplayFile::new(meta);
        file.events = self.events.clone();
        file.validate()?;
        file.write_to(&self.out_path)?;
        self.flushed = true;
        eprintln!(
            "record: wrote {} event(s) covering {} frame(s) -> {}",
            file.events.len(),
            file.meta.frames,
            self.out_path.display()
        );
        Ok(())
    }
}

/// VRAM regions known to carry CLUT (colour-lookup-table) data, by Y row
/// and approximate X span. The renderer treats CLUTs as 16- or 256-entry
/// rows of u16 BGR555 anywhere in VRAM; the project's RE has surfaced
/// specific bands that scene-pack uploads target.
///
/// Each entry is `(label, y, x_start, width)`; width is in pixels (not
/// bytes), and a CLUT row is one pixel tall by definition.
const VRAM_CLUT_BANDS: &[(&str, usize, usize, usize)] = &[
    // Row-479 NPC palette band (see docs/formats/npc-palette.md +
    // project_row479_global_hue_ramp memory). Scene-pack TIMs upload
    // 16- and 32-entry CLUTs into this row at fb_x=0..256.
    ("npc-clut row 479           x=  0..256", 479, 0, 256),
    // Common low-pages-area CLUT rows used by character / scene
    // textures. Most scenes touch at least one row in 480..512.
    ("char-clut row 480           x=  0..256", 480, 0, 256),
    ("char-clut row 481           x=  0..256", 481, 0, 256),
    ("char-clut row 496           x=  0..256", 496, 0, 256),
    // Display framebuffer scan rows. These are normally rewritten
    // every frame so any "engine populated this from the static
    // upload" content is suspect.
    ("framebuffer scanline y= 16  x=  0..640", 16, 0, 640),
    ("framebuffer scanline y=128  x=  0..640", 128, 0, 640),
];

fn write_vram_rows_csv(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: usize = 1024;
    const H: usize = 512;
    let mut s = String::new();
    s.push_str("y,runtime_nz,engine_nz,overlap,runtime_only,engine_only\n");
    for y in 0..H {
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let row_base = y * W * 2;
        for x in 0..W {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        s.push_str(&format!("{y},{rt},{en},{ov},{rt_only},{en_only}\n"));
    }
    std::fs::write(path, s).with_context(|| format!("write VRAM rows CSV {}", path.display()))?;
    Ok(())
}

fn print_vram_clut_region_report(engine: &[u8], runtime: &[u8]) {
    const W: usize = 1024;
    const H: usize = 512;
    println!();
    println!("VRAM CLUT-region health (engine vs runtime):");
    println!(
        "  {:<48} {:>5} {:>5} {:>5} {:>6} {:>6}",
        "band", "rt", "en", "ov", "rt-only", "en-only"
    );
    for &(label, y, x0, w) in VRAM_CLUT_BANDS {
        if y >= H {
            continue;
        }
        let row_base = y * W * 2;
        let mut rt = 0u32;
        let mut en = 0u32;
        let mut ov = 0u32;
        let mut rt_only = 0u32;
        let mut en_only = 0u32;
        let x_end = (x0 + w).min(W);
        for x in x0..x_end {
            let off = row_base + x * 2;
            let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
            let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
            let rnz = rw != 0;
            let enz = ew != 0;
            if rnz {
                rt += 1;
            }
            if enz {
                en += 1;
            }
            match (rnz, enz) {
                (true, true) => ov += 1,
                (true, false) => rt_only += 1,
                (false, true) => en_only += 1,
                _ => {}
            }
        }
        let pct = if rt > 0 {
            100.0 * (ov as f64) / (rt as f64)
        } else {
            0.0
        };
        let flag = if rt_only > 0 && rt > 0 {
            " <-- gap"
        } else {
            ""
        };
        println!(
            "  {label:<48} {rt:>5} {en:>5} {ov:>5} {rt_only:>6} {en_only:>6}  ({pct:5.1}%){flag}"
        );
    }
}

fn write_vram_png(path: &Path, bgr555_le: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let rgba = legaia_mednafen::vram_to_rgba8(bgr555_le);
    let f = std::fs::File::create(path)
        .with_context(|| format!("create VRAM PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Compact per-region VRAM coverage report.
struct VramCoverage {
    /// Per-tile counts. Each tile is 64x64 pixels (16 tiles wide, 8 rows tall).
    runtime_nonzero_pixels: u64,
    engine_nonzero_pixels: u64,
    overlap_pixels: u64,
    runtime_only_pixels: u64,
    engine_only_pixels: u64,
    /// `(y_range_label, runtime_nonzero, engine_nonzero, overlap)` for
    /// the common VRAM regions.
    bands: Vec<(&'static str, u64, u64, u64)>,
}

fn vram_coverage_report(engine: &[u8], runtime: &[u8]) -> VramCoverage {
    const W: usize = 1024;
    const H: usize = 512;
    let mut runtime_nz = 0u64;
    let mut engine_nz = 0u64;
    let mut overlap = 0u64;
    let mut runtime_only = 0u64;
    let mut engine_only = 0u64;
    for i in 0..(W * H) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        if rnz {
            runtime_nz += 1;
        }
        if enz {
            engine_nz += 1;
        }
        match (rnz, enz) {
            (true, true) => overlap += 1,
            (true, false) => runtime_only += 1,
            (false, true) => engine_only += 1,
            _ => {}
        }
    }
    // Band reports split VRAM into top half (display + scratch) and bottom
    // half (texture pages + CLUTs), then split the bottom into upper-256
    // (typical character / scene textures) and lower-256 (extra texture
    // pages, CLUT rows).
    let band = |y0: usize, y1: usize| -> (u64, u64, u64) {
        let mut rt = 0u64;
        let mut en = 0u64;
        let mut ov = 0u64;
        for y in y0..y1 {
            for x in 0..W {
                let off = (y * W + x) * 2;
                let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
                let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
                let rnz = rw != 0;
                let enz = ew != 0;
                if rnz {
                    rt += 1;
                }
                if enz {
                    en += 1;
                }
                if rnz && enz {
                    ov += 1;
                }
            }
        }
        (rt, en, ov)
    };
    let mut bands = Vec::new();
    let (rt, en, ov) = band(0, 256);
    bands.push(("top half y=  0..256 (display FB + scratch)", rt, en, ov));
    let (rt, en, ov) = band(256, 384);
    bands.push(("texpage rows y=256..384 (primary textures)", rt, en, ov));
    let (rt, en, ov) = band(384, 512);
    bands.push(("texpage rows y=384..512 (textures + CLUTs)", rt, en, ov));
    VramCoverage {
        runtime_nonzero_pixels: runtime_nz,
        engine_nonzero_pixels: engine_nz,
        overlap_pixels: overlap,
        runtime_only_pixels: runtime_only,
        engine_only_pixels: engine_only,
        bands,
    }
}

fn print_vram_coverage(c: &VramCoverage) {
    let total_runtime = c.runtime_nonzero_pixels.max(1);
    println!("VRAM coverage (engine vs runtime, BGR555 != 0 pixel mask)");
    println!(
        "  runtime non-zero pixels:  {:>8}   (= the loaded VRAM ground truth)",
        c.runtime_nonzero_pixels
    );
    println!("  engine  non-zero pixels:  {:>8}", c.engine_nonzero_pixels);
    println!(
        "  overlap (engine ∩ rt):    {:>8}   ({:.1}% of runtime)",
        c.overlap_pixels,
        100.0 * c.overlap_pixels as f64 / total_runtime as f64
    );
    println!(
        "  runtime-only (gap):       {:>8}   ({:.1}% missing in engine)",
        c.runtime_only_pixels,
        100.0 * c.runtime_only_pixels as f64 / total_runtime as f64
    );
    println!("  engine-only (extra):      {:>8}", c.engine_only_pixels);
    println!("  per-band breakdown:");
    for (label, rt, en, ov) in &c.bands {
        let pct = if *rt > 0 {
            100.0 * (*ov as f64) / (*rt as f64)
        } else {
            0.0
        };
        println!("    {label:<48} runtime={rt:>7} engine={en:>7} overlap={ov:>7} ({pct:5.1}%)");
    }
}

fn write_vram_diff_png(path: &Path, engine: &[u8], runtime: &[u8]) -> Result<()> {
    const W: u32 = 1024;
    const H: u32 = 512;
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for i in 0..(W as usize * H as usize) {
        let off = i * 2;
        let rw = u16::from_le_bytes([runtime[off], runtime[off + 1]]);
        let ew = u16::from_le_bytes([engine[off], engine[off + 1]]);
        let rnz = rw != 0;
        let enz = ew != 0;
        let color = match (rnz, enz) {
            (false, false) => [0u8, 0, 0, 0xFF],
            // Engine matches runtime exactly (same word) → grey
            (true, true) if rw == ew => [0x60, 0x60, 0x60, 0xFF],
            // Both non-zero but different content → blue
            (true, true) => [0x30, 0x80, 0xFF, 0xFF],
            // Runtime has content engine doesn't → red (the gap)
            (true, false) => [0xFF, 0x40, 0x40, 0xFF],
            // Engine has content runtime doesn't → green (extras / wrong slot)
            (false, true) => [0x40, 0xFF, 0x40, 0xFF],
        };
        rgba.extend_from_slice(&color);
    }
    let f = std::fs::File::create(path)
        .with_context(|| format!("create diff PNG {}", path.display()))?;
    let bw = std::io::BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    Ok(())
}

/// Decode a raw PSX STR file (2048-byte sectors) headlessly and return the
/// number of MDEC video frames that decode cleanly. Shared by the `play`
/// pre-decode pass and the in-flow cutscene driver.
fn decode_str_frame_count(str_path: &Path) -> Result<usize> {
    use legaia_mdec::{MdecDecoder, str_sector::StrFrameAssembler};
    let data = std::fs::read(str_path)?;
    let n_sectors = data.len() / 2048;
    let mut asm = StrFrameAssembler::new();
    let mut decoded = 0usize;
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            if dec.decode_frame(&bs).is_ok() {
                decoded += 1;
            }
        }
    }
    Ok(decoded)
}

/// Windowed in-flow cutscene playback state: the decoded STR frames the
/// field VM's FMV-trigger op resolved to, plus the current frame cursor and
/// the live GPU upload. Held on [`PlayWindowApp`] while the world sits in
/// [`SceneMode::Cutscene`]; the world resumes (`finish_cutscene`) once the
/// frames drain. Paced to the stream's detected frame rate (wall-clock gated),
/// like [`StrPlayerApp`], so the movie isn't tied to the display refresh rate.
struct WindowedCutscene {
    frames: Vec<legaia_mdec::VideoFrame>,
    idx: usize,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from `StrTiming::frame_period`).
    frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / period` (used only when there is no audio track).
    clock: Option<std::time::Instant>,
    /// The cutscene's interleaved XA audio track, staged into the engine's
    /// audio output on the first render so its cursor (the video clock for A/V
    /// sync) starts with the picture. `None` for a video-only (extract-sourced)
    /// cutscene. Taken once.
    pending_audio: Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
    /// `true` once an audio track has been staged - the render loop then reads
    /// the audio cursor as the master clock instead of wall-clock.
    has_audio: bool,
}

#[allow(clippy::too_many_arguments)]
fn cmd_play(
    scene: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    frames: u64,
    enable_audio: bool,
    frame_ms: u64,
    str_file: Option<&Path>,
    cutscene_map_path: Option<&Path>,
) -> Result<()> {
    // If the user supplied a `--cutscene-map` TOML doc, install it as the
    // explicit override layer; otherwise fall back to the heuristic.
    let cutscene_map = if let Some(p) = cutscene_map_path {
        legaia_engine_core::scene::CutsceneMap::from_toml_path(p)
            .with_context(|| format!("load cutscene map {}", p.display()))?
    } else {
        legaia_engine_core::scene::CutsceneMap::default()
    };
    if cutscene_map_path.is_some() {
        eprintln!(
            "info: cutscene-map loaded with {} explicit entry/entries",
            cutscene_map.len()
        );
    }
    // Auto-resolve a `--scene op*` / `--scene edteien` request to its
    // paired FMV via the cutscene map (which falls through to the
    // hard-coded heuristic) when the user didn't explicitly pass
    // `--str-file` and the extracted root has the file on disk.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => cutscene_map
            .resolve(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        // Disc-mode resolution would need an ISO9660 read; punt.
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());

    // If a STR file was supplied (explicitly or auto-resolved), pre-decode
    // it headlessly and log the frame count. This is phase 1 for
    // `op*`/`ed*` in-engine cutscene scenes where an FMV precedes the
    // dialogue-overlay scene proper. The scene ticking (phase 2) runs
    // unconditionally after this block.
    if let Some(str_path) = resolved_str {
        let decoded = decode_str_frame_count(str_path)
            .with_context(|| format!("read STR file {}", str_path.display()))?;
        println!(
            "play: pre-decoded {} STR frames from {}",
            decoded,
            str_path.display()
        );
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    println!(
        "play: scene='{}' frames={} audio={} (entries={}, MES={}, VAB={}, SEQ={})",
        scene,
        if frames == 0 {
            "∞".into()
        } else {
            frames.to_string()
        },
        if session.audio.is_some() { "on" } else { "off" },
        session
            .host
            .scene
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0),
        if session
            .host
            .assets
            .as_ref()
            .map(|a| a.mes.is_some())
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        },
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.vab_entries.len())
            .unwrap_or(0),
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.seq_entries.len() + a.seq_in_stream_entries.len())
            .unwrap_or(0),
    );

    let mut transitions = 0u64;
    let mut bgm_events = 0u64;
    let mut last_log = 0u64;
    let mut tick_count = 0u64;
    while frames == 0 || tick_count < frames {
        let event = session.tick()?;
        match event {
            SceneTickEvent::SceneEntered { name } => {
                transitions += 1;
                println!("frame {}: entered scene '{}'", tick_count, name);
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                println!(
                    "frame {}: scene_transition({}) had no mapped scene",
                    tick_count, map_id
                );
            }
            SceneTickEvent::Stepped => {}
        }
        // Field -> Cutscene -> Field flow: when the field VM's FMV-trigger op
        // flips the world into the cutscene mode (game mode 26 / StrInit), play
        // the resolved `MV*.STR` here (headless MDEC decode) and tell the world
        // playback finished so the field resumes. The STR overlay owns the
        // frame in retail; the world keeps the field VM suspended until then.
        if let Some(fmv_id) = session.host.world.active_fmv() {
            match session.host.world.active_fmv_str_filename() {
                Some(rel) => {
                    let path = extracted_root.join(rel);
                    match decode_str_frame_count(&path) {
                        Ok(n) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} ({n} frames)"
                        ),
                        Err(_) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} (not extracted; skipped)"
                        ),
                    }
                }
                None => {
                    println!("frame {tick_count}: cutscene fmv_id={fmv_id} (cut path; skipped)")
                }
            }
            session.host.world.finish_cutscene();
        }
        if let Some(bgm) = session.bgm.as_ref()
            && bgm.last_started.is_some()
        {
            bgm_events = bgm_events.max(1);
        }
        if tick_count - last_log >= 60 {
            last_log = tick_count;
            log::info!(
                "frame {}: world.frame={}, transitions={}, bgm_started={}",
                tick_count,
                session.host.world.frame,
                transitions,
                bgm_events
            );
        }
        if frame_ms > 0 {
            std::thread::sleep(Duration::from_millis(frame_ms));
        }
        tick_count += 1;
    }
    println!(
        "exit: ticked {} frames, world.frame={}, transitions={}",
        tick_count, session.host.world.frame, transitions
    );
    Ok(())
}

fn cmd_save(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    slot: u8,
    party_size: usize,
) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;
    use legaia_save::{CharacterRecord, Party};

    let _ = (extracted_root, disc);
    let mut world = World::default();
    let members = (0..party_size).map(|_| CharacterRecord::zeroed()).collect();
    world.load_party(Party { members });
    world.story_flags = 0;
    world.money = 0;
    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let path = runtime.save_to_slot(&mut world, slot)?;
    let sf = world.save_full();
    println!(
        "saved slot {} to {} (party={}, story_flags={:#010X}, money={}, inventory={})",
        slot,
        path.display(),
        sf.party.members.len(),
        sf.ext.story_flags,
        sf.ext.money,
        sf.ext.inventory.len()
    );
    Ok(())
}

fn cmd_load(save_dir: &std::path::Path, slot: u8) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;

    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let mut world = World::default();
    let path = runtime.load_from_slot(&mut world, slot)?;
    println!(
        "loaded slot {} from {} (party={}, story_flags={:#010X}, money={}, inventory={}, actors={})",
        slot,
        path.display(),
        world.roster.members.len(),
        world.story_flags,
        world.money,
        world.inventory.len(),
        world.actors.iter().filter(|a| a.active).count()
    );
    Ok(())
}

fn cmd_list_scenes(extracted_root: &std::path::Path, disc: Option<&std::path::Path>) -> Result<()> {
    let map: cdname::IndexMap = if let Some(disc_path) = disc {
        // Pull CDNAME.TXT bytes out of the disc image once.
        use legaia_engine_core::{DiscVfs, Vfs};
        let vfs = DiscVfs::open(disc_path)?;
        let bytes = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .context("CDNAME.TXT not present in disc image")?;
        let text = String::from_utf8(bytes).context("CDNAME.TXT is not valid UTF-8")?;
        cdname::parse_str(&text)?
    } else {
        let cdname_path = extracted_root.join("CDNAME.TXT");
        if !cdname_path.exists() {
            anyhow::bail!(
                "missing {} (run `legaia-extract` first or pass --disc PATH)",
                cdname_path.display()
            );
        }
        cdname::parse(&cdname_path).with_context(|| format!("parse {}", cdname_path.display()))?
    };

    let mut names: Vec<String> = map.values().cloned().collect();
    names.sort();
    names.dedup();

    println!("{} distinct scene names:", names.len());
    for name in &names {
        if let Some((start, end)) = cdname::block_range_for_name(&map, name) {
            println!(
                "  {:<24} PROT [{}..{}) ({} entries)",
                name,
                start,
                end,
                end - start
            );
        }
    }
    Ok(())
}

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

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    use legaia_engine_core::input::Mapping;
    match cmd {
        ConfigCmd::Show { config_file } => {
            let mapping = Mapping::load_or_default(&config_file);
            let mut pairs: Vec<_> = mapping.bindings.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            println!("input mapping ({})", config_file.display());
            for (key, btn) in &pairs {
                println!("  {key:<12} → {btn}");
            }
        }
        ConfigCmd::Set {
            binding,
            config_file,
        } => {
            let Some((key, btn)) = binding.split_once('=') else {
                anyhow::bail!("--binding must be KEY=BUTTON (e.g. Z=Cross)");
            };
            let key = key.trim().to_string();
            let btn = btn.trim().to_string();
            // Validate that the button name is known.
            if legaia_engine_core::input::PadButton::from_name(&btn).is_none() {
                anyhow::bail!(
                    "unknown pad button '{}'; valid names: Select L3 R3 Start Up Right Down Left L2 R2 L1 R1 Triangle Circle Cross Square",
                    btn
                );
            }
            let mut mapping = Mapping::load_or_default(&config_file);
            mapping.bindings.insert(key.clone(), btn.clone());
            mapping.save(&config_file)?;
            println!("binding saved: {key} → {btn} ({})", config_file.display());
        }
        ConfigCmd::DumpCutsceneMap { out } => {
            let map = legaia_engine_core::scene::CutsceneMap::from_heuristic();
            let toml_doc = map.to_toml_string();
            if out.as_os_str() == "-" {
                print!("{toml_doc}");
            } else {
                std::fs::write(&out, &toml_doc)
                    .with_context(|| format!("write {}", out.display()))?;
                println!(
                    "wrote {} cutscene-map entry/entries → {}",
                    map.len(),
                    out.display()
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// play-window
// ---------------------------------------------------------------------------

/// Pre-decoded publisher-logos atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0895; reused by [`BootUiState::PublisherLogos`]
/// to sample one logo per frame.
struct PublisherLogosAssets {
    /// Per-logo source rects in atlas pixels.
    rects: [(u32, u32, u32, u32); legaia_engine_core::publisher_logos::LOGO_COUNT],
    /// GPU-resident sprite atlas (vertically stacked logos).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded title-screen atlas + GPU upload. Created once at boot
/// when the disc has a valid PROT 0888 (`sound_data2` per CDNAME,
/// carries title art - see `legaia_asset::title_pak`); reused by
/// [`BootUiState::Title`] to blit one quad per frame.
struct TitleScreenAssets {
    /// Source rect to sample for the title quad
    /// (`(0, 0, width, height)` for the single-TIM layout).
    rect: (u32, u32, u32, u32),
    /// GPU-resident sprite atlas (the 256×256 title TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded menu-glyph atlas + GPU upload. Created once at boot
/// when the disc has a readable `PROT.DAT` (the source TIM lives in
/// the unindexed 240 KB pre-`init_data` gap; see
/// [`legaia_asset::menu_glyph_atlas`]). Reused by [`BootUiState::Title`]
/// to render the "NEW GAME" / "CONTINUE" / "OPTIONS" menu rows in
/// the retail small-caps font.
struct MenuGlyphAssets {
    /// GPU-resident sprite atlas (the 256×256 menu-glyph TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Pre-decoded save-menu UI atlas + GPU upload. Created once at boot
/// when the disc has both a readable PROT 0899 (carries the SLOT 1 /
/// SLOT 2 pill sprites with CLUT 7) AND a readable PROT.DAT (carries
/// the system-UI sprite sheet at offset `0x018E0` with the 9-slice
/// panel chrome under CLUT row 2). Reused by [`BootUiState::SaveSelect`]
/// to compose the retail 81×29 "Load" panel from 14 byte-pinned
/// textured-sprite primitives + the 2 slot pills over the dimmed
/// title background.
struct SaveMenuAssets {
    /// Source rects for the 9-slice panel tiles + slot pills.
    rects: legaia_engine_render::SaveMenuAtlasRects,
    /// GPU-resident sprite atlas (composite 256×256: panel tiles from
    /// system-UI TIM + slot pills from PROT 0899's save-menu TIM).
    atlas: legaia_engine_render::UploadedSpriteAtlas,
}

/// Windowed engine runner state. Owned by the winit event loop.
struct PlayWindowApp {
    session: BootSession,
    font: Font,
    /// Pre-built scene resources (VRAM + TMDs). Consumed by `upload_assets`
    /// when the renderer is first attached; `None` after that.
    scene_res: Option<SceneResources>,
    win: EngineWindow,
    font_atlas: Option<UploadedFontAtlas>,
    /// Publisher-logos atlas (built once from PROT 0895). `None` if the
    /// disc isn't loaded or init.pak doesn't parse.
    publisher_logos: Option<PublisherLogosAssets>,
    /// CPU-side atlas data waiting for renderer upload. Moved into
    /// `publisher_logos` on the first frame the renderer is available.
    pending_publisher_logos_atlas: Option<legaia_engine_core::publisher_logos::LogosAtlas>,
    /// Title-screen atlas (built once from PROT 0888). `None` if the
    /// disc isn't loaded or the title TIM doesn't parse.
    title_screen: Option<TitleScreenAssets>,
    /// CPU-side title atlas waiting for renderer upload. Moved into
    /// `title_screen` on the first frame the renderer is available.
    pending_title_screen_atlas: Option<legaia_engine_core::title_screen_atlas::TitleScreenAtlas>,
    /// Menu-glyph atlas (built once from raw `PROT.DAT` at 0x11218).
    /// `None` if the disc isn't loaded or the TIM doesn't parse.
    menu_glyphs: Option<MenuGlyphAssets>,
    /// CPU-side menu-glyph atlas waiting for renderer upload. Moved
    /// into `menu_glyphs` on the first frame the renderer is available.
    pending_menu_glyph_atlas: Option<legaia_engine_core::menu_glyph_atlas::MenuGlyphAtlas>,
    /// Save-menu UI atlas (built once from PROT 0899). `None` if the
    /// disc isn't loaded or the save-menu TIM doesn't parse.
    save_menu: Option<SaveMenuAssets>,
    /// CPU-side save-menu atlas waiting for renderer upload. Moved
    /// into `save_menu` on the first frame the renderer is available.
    pending_save_menu_atlas: Option<legaia_engine_core::save_menu_atlas::SaveMenuAtlas>,
    uploaded_vram: Option<UploadedVram>,
    meshes: Vec<UploadedVramMesh>,
    /// Retained TMD data (struct + raw bytes) parallel to `meshes`, used to
    /// re-pose animated actor meshes each frame via `tmd_to_vram_mesh_posed`.
    scene_tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)>,
    /// Field static-geometry draws: `(index into `meshes`, world model matrix)`
    /// per placed environment object, resolved at scene load from the field
    /// map's object table (`legaia_asset::field_objects`). Empty for non-field
    /// scenes. Drawn in `SceneMode::Field` so the town renders its buildings /
    /// terrain at their world positions instead of all at the origin.
    field_placement_draws: Vec<(usize, Mat4)>,
    /// Untextured (`F*`/`G*`) vertex-colour meshes for field props whose prims
    /// carry per-vertex colours instead of UVs (the textured VRAM-mesh path
    /// drops them). Parallel render list to `meshes`.
    color_meshes: Vec<UploadedColorMesh>,
    /// Field static-geometry colour draws: `(index into `color_meshes`, world
    /// model)` for the untextured props. Drawn alongside `field_placement_draws`.
    field_placement_color_draws: Vec<(usize, Mat4)>,
    /// World-map continent terrain draws: `(uploaded-mesh index, world model)`
    /// per visible tile of the kingdom's `.MAP` object grid
    /// (`Scene::field_terrain_tiles`, the dense `FUN_801F69D8` continent layer).
    /// Empty off the world map. Drawn in `SceneMode::WorldMap` so the overworld
    /// shows its tiled ground / trees / mountains rather than a handful of
    /// landmark objects.
    world_map_terrain_draws: Vec<(usize, Mat4)>,
    /// World-map continent **ground**: the heightfield surface built from the
    /// walk `.MAP` floor grid (`Scene::walk_heightfield`). `None` off the world
    /// map. Drawn in `SceneMode::WorldMap` as the continent ground (texturing
    /// provisional — a uniform ground texel); `world_map_terrain_draws` carries
    /// the sparse placed landmarks on top. Kept out of `meshes` (it has no
    /// `Tmd` / actor binding); drawn directly with a constant Y-flip model.
    world_map_heightfield: Option<UploadedVramMesh>,
    /// World-map ocean CLUT animation. `Some` for a world-map kingdom scene that
    /// ships the ocean tile; drives the 13-frame rolling-wave by overwriting the
    /// first 16 CLUT entries at VRAM `(0, 506)` each animation step (the retail
    /// per-frame DMA target). `None` off the world map.
    ocean_anim: Option<OceanAnim>,
    /// Pristine CPU-side scene VRAM, cloned at scene-load before any battle
    /// edits. A battle injects monster texture pools into a working copy and
    /// re-uploads; leaving battle restores this base so the field renders
    /// with clean VRAM. `None` until the first scene loads.
    cpu_vram_base: Option<legaia_tim::Vram>,
    /// Scene-mode from the previous frame, used to detect Field<->Battle
    /// transitions so monster meshes are uploaded / dropped exactly once.
    prev_scene_mode: Option<SceneMode>,
    /// Lazily-cached monster stat archive (PROT 867) bytes, decoded once and
    /// reused for every battle so each transition doesn't re-decompress 16 MB.
    monster_archive: Option<std::sync::Arc<Vec<u8>>>,
    /// `meshes.len()` at battle entry: the boundary appended battle monster
    /// meshes start at, so leaving battle truncates back to it.
    battle_mesh_base: usize,
    /// The battle VRAM (field base + injected monster textures) stashed by
    /// `enter_battle_render`, so a mid-battle player-summon spawn can inject its
    /// own creature texture into a clone and re-upload.
    battle_vram: Option<legaia_tim::Vram>,
    /// Number of monster texture slots in use this battle (0..=4). A player
    /// summon reuses the next free slot for its creature texture.
    battle_tex_slots_used: u8,
    /// World actor slot the spawned player-summon creature occupies (`>= 8`, so
    /// it never collides with the party/monster battle slots), or `None`.
    summon_actor_slot: Option<usize>,
    /// Mesh index of the battle-stage backdrop dome (the scene's
    /// `scene_tmd_stream` half-dome), drawn behind the actors. `None` when the
    /// scene has no stage or it failed to load.
    battle_stage_mesh: Option<usize>,
    /// Mesh index of the flat tiled ground grid drawn under the battle actors
    /// (retail's `func_0x801d02c0` grass grid). Reuses the stage dome's grass
    /// texel so it samples real grass from the battle VRAM. `None` outside a
    /// stage-dome battle or when the dome has no usable ground texel.
    battle_ground_mesh: Option<usize>,
    scene_aabb: ([f32; 3], [f32; 3]),
    /// Current held-button bitmask (PSX pad encoding). Updated per key event.
    pad: u16,
    /// Input binding loaded from file (or default).
    mapping: legaia_engine_core::input::Mapping,
    /// Menu runtime - drives shop / inn / status screens. Ticked per frame
    /// when `is_open()`; renders shop overlay via `shop_draws_for`.
    menu_runtime: legaia_engine_core::menu_runtime::MenuRuntime,
    /// Pad state from the previous frame - used to compute newly-pressed bits
    /// for boot-UI edge detection.
    prev_pad: u16,
    /// Rolling battle-event log surfaced in the HUD. Each tick drains
    /// `World::pending_battle_events` and folds the most recent N entries
    /// into this ring buffer (`ApplyArtStrike` events also get applied to
    /// the target's `BattleActor::hp` via `World::fold_battle_event`). The
    /// log is empty until a battle SM actually fires.
    battle_event_log: std::collections::VecDeque<String>,
    /// Damage-popup / status model for the battle HUD. Fed each frame from
    /// `World::drain_battle_hit_fx` (floating numbers) + the live status
    /// tracker (per-slot icons); aged by `BattleHud::tick`. Popups + status
    /// letters are drawn anchored to the per-slot HP rows in `build_hud`.
    battle_hud: legaia_engine_core::battle_hud::BattleHud,
    /// Actor slots queued for render-side mesh upload. Populated when a
    /// `FieldEvent::ActorSpawned` fires with a non-`None` `Actor::tmd_ref`
    /// (the field-VM `0x4C 0xD8` synchronous-spawn path); drained in the
    /// next render pass, which materializes a [`UploadedVramMesh`] from
    /// the actor's global-pool TMD, appends it to `self.meshes` /
    /// `self.scene_tmd_data`, and sets `actor.tmd_binding` to the new
    /// mesh index so the per-frame draws iteration picks it up.
    pending_dynamic_mesh_slots: Vec<u8>,
    /// Boot-UI state. `BootUiState::Inactive` means the legacy
    /// "go straight to the scene" path; `Title` / `SaveSelect` route
    /// player input through the boot sessions until the player picks
    /// New Game / Continue and the scene becomes interactive.
    boot_ui: BootUiState,
    /// Save directory the save-select session reads / writes against.
    save_dir: std::path::PathBuf,
    /// User-editable settings (BGM / SFX volume, message speed, …). Wired
    /// to the Options screen's [`OptionsSession`] and persisted via
    /// the engine's options round-trip path.
    options_state: legaia_engine_core::options::OptionsState,
    /// Phase J3 pad-capture state. `Some` when the user invoked the
    /// `record` subcommand; the keyboard handler appends transitions
    /// to `events` and the close handler flushes a `j-replay-v1` file
    /// to disk. `None` in plain `play-window` runs.
    record_log: Option<RecordLog>,
    /// Field-live options (live loop / player-driven battle / battle BGM)
    /// captured at startup, so the boot-UI NEW GAME handler can re-enter the
    /// opening cutscene scene (`opdeene`) with the same arming the startup
    /// `enter_field_live` used.
    field_live_opts: legaia_engine_shell::boot::FieldLiveOpts,
    /// Extracted-root directory, retained so the in-flow cutscene driver can
    /// resolve a field-VM FMV trigger's `MV*.STR` file (mirrors the headless
    /// `play` loop's `extracted_root.join(rel)`). `None` in disc-only runs,
    /// where the STR is read straight from the ISO via `disc_path`.
    extracted_root: Option<std::path::PathBuf>,
    /// Disc image, retained so the in-flow cutscene driver can read a movie's
    /// raw 2352-byte sectors off the ISO and play its interleaved XA audio in
    /// sync with the video. `None` when running from an extracted root (where
    /// the cutscene plays video only, since the extract truncates the audio).
    disc_path: Option<std::path::PathBuf>,
    /// Active windowed cutscene playback, when the field VM has flipped the
    /// world into [`SceneMode::Cutscene`] and the resolved STR decoded. While
    /// `Some`, world ticks are suspended and the window shows the video; the
    /// world resumes once the frames drain.
    cutscene: Option<WindowedCutscene>,
    /// Eases the in-engine cutscene camera between Camera Configure beats so
    /// the opening choreography blends instead of cutting. Reset (snaps) while
    /// no cutscene timeline is active.
    cutscene_cam_interp: legaia_engine_render::window::CutsceneCameraInterp,
    /// Active dialog box, mirroring `World::current_dialog`. Opened from the
    /// scene's MES container the frame a dialog request appears (field-VM
    /// op `0x3F` or the overworld talk-to), ticked for its typewriter reveal,
    /// and dropped when the world clears the request (the world owns dismissal
    /// via the field VM / overworld handler). `None` when no box is up.
    active_dialog: Option<legaia_engine_core::dialog::OwnedDialogPanel>,
}

/// Boot-UI state machine. Drives the pre-scene UI when `--boot-ui` is
/// supplied to play-window. The default `Inactive` variant is what
/// every other path uses (no boot UI).
#[allow(clippy::large_enum_variant)]
enum BootUiState {
    /// No boot UI - engine ticks the scene normally.
    Inactive,
    /// Boot publisher logos (PROT 0895). Runs before the title screen.
    /// The atlas + per-logo rects live on [`PlayWindowApp`] so the
    /// renderer can sample one quad per frame.
    PublisherLogos(legaia_engine_core::publisher_logos::PublisherLogosSession),
    /// Title screen is active. Pad input drives the
    /// [`legaia_engine_core::title::TitleSession`].
    Title(legaia_engine_core::title::TitleSession),
    /// Save-select panel is active.
    SaveSelect(legaia_engine_core::save_select::SaveSelectSession),
    /// Options / config panel is active.
    Options(legaia_engine_core::options::OptionsSession),
    /// Field (pause) menu is active. Wraps the live scene so dropping
    /// out returns control to the field tick.
    ///
    /// `sub` holds the active sub-session pushed by
    /// `FieldMenuOutcome::Confirmed(row)` (Status, Equip, Spells, Items,
    /// Save, Options, Arts) - when `Some`, input + draws route to the
    /// sub instead of the menu and the menu sits in `Suspended`
    /// underneath.
    FieldMenu {
        session: legaia_engine_core::field_menu::FieldMenuSession,
        sub: Option<legaia_engine_core::field_menu_dispatch::FieldMenuSubsession>,
    },
    /// Game-over panel is active after a party wipe.
    #[allow(dead_code)]
    GameOver(legaia_engine_core::game_over::GameOverSession),
}

impl BootUiState {
    fn is_active(&self) -> bool {
        !matches!(self, BootUiState::Inactive)
    }
}

impl PlayWindowApp {
    /// Maximum number of battle-event log lines kept in the HUD ring.
    const BATTLE_EVENT_LOG_CAP: usize = 6;

    /// Tick the boot-UI state machine (when active) using the latest
    /// pad bitmask. Returns `true` if the boot UI is still active and
    /// the scene tick should be skipped this frame.
    fn tick_boot_ui(&mut self) -> bool {
        // Build edge-triggered "newly pressed" mask so menu navigation
        // doesn't auto-repeat on held keys.
        let pressed = self.pad & !self.prev_pad;
        let cross = pressed & 0x4000 != 0;
        let circle = pressed & 0x2000 != 0;
        let triangle = pressed & 0x1000 != 0;
        let start = pressed & 0x0008 != 0;
        let up = pressed & 0x0010 != 0;
        let down = pressed & 0x0040 != 0;
        let left = pressed & 0x0080 != 0;
        let right = pressed & 0x0020 != 0;

        match &mut self.boot_ui {
            BootUiState::Inactive => false,
            BootUiState::PublisherLogos(session) => {
                // Start (or Cross) skips the boot sequence.
                if start || cross {
                    session.request_skip();
                }
                session.tick();
                if session.is_done() {
                    // Hand off to the title screen with the
                    // continue-enabled flag set per save-slot scan.
                    let snapshots = scan_save_dir(&self.save_dir);
                    let any_present = snapshots.iter().any(|s| s.present);
                    self.boot_ui = if any_present {
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new())
                    } else {
                        BootUiState::Title(
                            legaia_engine_core::title::TitleSession::without_save_data(),
                        )
                    };
                }
                true
            }
            BootUiState::Title(session) => {
                use legaia_engine_core::title::{TitleEvent, TitleInput, TitleOutcome};
                let input = TitleInput {
                    up,
                    down,
                    cross,
                    start,
                    circle,
                };
                let events = session.tick(input);
                for ev in &events {
                    match ev {
                        TitleEvent::NewGameSelected => {
                            log::info!("title: New Game");
                        }
                        TitleEvent::ContinueSelected => {
                            log::info!("title: Continue");
                        }
                        TitleEvent::OptionsSelected => {
                            // The selection event is informational; the Options
                            // panel opens when the title session resolves to
                            // `TitleOutcome::Options` below.
                            log::info!("title: Options");
                        }
                        _ => {}
                    }
                }
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        TitleOutcome::NewGame => {
                            // Mirror the retail NEW GAME → field-launch
                            // (master mode 2 → mode 3): establish a fresh slate
                            // and seed the starting party (Vahn) from the disc's
                            // SCUS template, then enter the prologue cutscene
                            // scene `opdeene` (the front-end launcher's opening
                            // scene id, verified live), which hands off to the
                            // interactive `town01`. See docs/subsystems/boot.md
                            // "New Game boot chain".
                            self.session.begin_new_game();
                            let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
                            match self
                                .session
                                .enter_field_live(cutscene, &self.field_live_opts)
                            {
                                Ok(mode) => {
                                    // The cutscene -> Rim Elm handoff is now armed
                                    // inside `enter_field_scene` by walking opdeene's
                                    // MAN cutscene-timeline for the real `GFLAG_SET 26`
                                    // write (World::arm_prologue_handoff_from_man), so
                                    // no blind arm is needed here. The confirm-gated
                                    // transition still fires in the field tick below
                                    // (World::take_prologue_handoff).
                                    log::info!(
                                        "new game: seeded party_count={}, entered opening cutscene \
                                         '{cutscene}' (mode={mode:?})",
                                        self.session.host.world.party_count,
                                    )
                                }
                                Err(e) => log::warn!(
                                    "new game: enter opening cutscene '{cutscene}' failed ({e:#}); \
                                     staying on the pre-booted scene"
                                ),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        TitleOutcome::Continue => {
                            // Open the save-select panel against `save_dir`.
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        TitleOutcome::Options => {
                            self.boot_ui = BootUiState::Options(
                                legaia_engine_core::options::OptionsSession::new(
                                    self.options_state.clone(),
                                ),
                            );
                        }
                    }
                }
                true
            }
            BootUiState::SaveSelect(session) => {
                use legaia_engine_core::save_select::{SelectInput, SelectOutcome};
                let input = SelectInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    triangle,
                };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        SelectOutcome::Loaded(slot) => {
                            // Hydrate the world from the slot file.
                            let runtime = legaia_engine_core::menu_runtime::MenuRuntime::new(
                                self.save_dir.clone(),
                            );
                            match runtime.load_from_slot(&mut self.session.host.world, slot) {
                                Ok(p) => log::info!("loaded slot {} from {}", slot, p.display()),
                                Err(e) => log::warn!("load slot {slot} failed: {e:#}"),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        SelectOutcome::Cancelled => {
                            // Back to title.
                            self.boot_ui =
                                BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                        }
                        SelectOutcome::Saved(_) | SelectOutcome::Deleted(_) => {
                            // Save-select in Load mode shouldn't emit these,
                            // but degrade gracefully.
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::Options(session) => {
                use legaia_engine_core::options::{OptionsInput, OptionsOutcome};
                let input = OptionsInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    start,
                };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        OptionsOutcome::Confirmed => {
                            self.options_state = session.state().clone();
                        }
                        OptionsOutcome::Cancelled => {
                            session.revert_if_cancelled();
                        }
                    }
                    // After options, route back to Title so the player can
                    // pick New Game / Continue (matches retail flow).
                    self.boot_ui =
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                }
                true
            }
            BootUiState::FieldMenu { session, sub } => {
                use legaia_engine_core::field_menu::{FieldMenuInput, FieldMenuOutcome};
                use legaia_engine_core::field_menu_dispatch::{
                    FieldMenuSubsession, apply_arts_outcome, apply_equip_outcome,
                    apply_inventory_outcome, apply_spell_outcome,
                };
                if let Some(active_sub) = sub.as_mut() {
                    // A sub-session is open - route input + check for done.
                    active_sub.tick_pad_edge(pressed);
                    if active_sub.is_done() {
                        // Drain into world side-effects + handle save.
                        let finished = sub.take().expect("sub was Some");
                        match finished {
                            FieldMenuSubsession::Items(s) => {
                                apply_inventory_outcome(&s, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Equip { session, char_slot } => {
                                let _ = apply_equip_outcome(
                                    &session,
                                    char_slot,
                                    &mut self.session.host.world,
                                );
                            }
                            FieldMenuSubsession::Spells(s) => {
                                apply_spell_outcome(&s, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Arts(editor) => {
                                // No persistent ChainLibrary on the world
                                // yet - the editor's outcome is dropped
                                // until engines wire one in.
                                let mut throwaway =
                                    legaia_engine_core::tactical_arts_editor::ChainLibrary::new();
                                let _ = apply_arts_outcome(editor, &mut throwaway);
                            }
                            FieldMenuSubsession::Status(_) => {}
                            FieldMenuSubsession::Save(s) => {
                                use legaia_engine_core::save_select::SelectOutcome;
                                if let Some(SelectOutcome::Saved(slot)) = s.outcome() {
                                    let runtime =
                                        legaia_engine_core::menu_runtime::MenuRuntime::new(
                                            self.save_dir.clone(),
                                        );
                                    match runtime.save_to_slot(&mut self.session.host.world, slot) {
                                        Ok(p) => log::info!(
                                            "field menu: saved slot {} to {}",
                                            slot,
                                            p.display()
                                        ),
                                        Err(e) => {
                                            log::warn!("field menu: save slot {slot} failed: {e:#}")
                                        }
                                    }
                                }
                            }
                            FieldMenuSubsession::Config(mut o) => {
                                o.revert_if_cancelled();
                                self.options_state = o.state().clone();
                            }
                        }
                        let _ = session.resume(false);
                    }
                    return true;
                }
                let input = FieldMenuInput {
                    up,
                    down,
                    cross,
                    circle,
                    start,
                };
                let _ = session.tick(input);
                // After Cross on a row the menu phase becomes Suspended.
                // Build the matching sub-session and route control there.
                if session.is_suspended()
                    && let legaia_engine_core::field_menu::FieldMenuPhase::Suspended { row } =
                        session.phase()
                {
                    let snapshots = scan_save_dir(&self.save_dir);
                    *sub = Some(FieldMenuSubsession::build(
                        row,
                        &self.session.host.world,
                        &self.options_state,
                        &snapshots,
                        &legaia_engine_core::tactical_arts_editor::ChainLibrary::new(),
                        &legaia_engine_core::spells::SpellCatalog::vanilla(),
                        &legaia_engine_core::battle_stats::EquipmentTable::new(),
                    ));
                }
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        FieldMenuOutcome::Closed => {
                            self.boot_ui = BootUiState::Inactive;
                        }
                        FieldMenuOutcome::Confirmed(_row) => {
                            // Sub-session signaled "close menu entirely"
                            // via resume(true). Drop straight to scene.
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::GameOver(session) => {
                use legaia_engine_core::game_over::{GameOverInput, GameOverOutcome};
                let input = GameOverInput { up, down, cross };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        GameOverOutcome::Continue => {
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        GameOverOutcome::Retry | GameOverOutcome::Quit => {
                            // Retry → drop to scene; Quit → back to title.
                            self.boot_ui = match outcome {
                                GameOverOutcome::Quit => BootUiState::Title(
                                    legaia_engine_core::title::TitleSession::new(),
                                ),
                                _ => BootUiState::Inactive,
                            };
                        }
                    }
                }
                true
            }
        }
    }

    /// Build text draws for the active boot UI (when applicable).
    fn boot_ui_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        match &self.boot_ui {
            BootUiState::Inactive => Vec::new(),
            BootUiState::PublisherLogos(_) => {
                // The publisher logos are drawn via the sprite overlay
                // (see `publisher_logo_sprite_draw`); no font text.
                Vec::new()
            }
            BootUiState::Title(s) => {
                use legaia_engine_core::title::TitlePhase;
                let (phase_id, cursor) = match s.phase() {
                    TitlePhase::FadeIn { .. } => (0, 0),
                    TitlePhase::PressStart { .. } => (1, 0),
                    TitlePhase::MainMenu { cursor } => (2, cursor),
                    TitlePhase::Done(_) => return Vec::new(),
                };
                // When the title-screen atlas is uploaded, the
                // main-menu rows render through the sprite path,
                // sampling NEW GAME / CONTINUE sub-rects from the
                // title TIM directly (retail-faithful). Suppress
                // the dialog-font fallback for phase 2 so the rows
                // aren't double-drawn. Earlier phases (fade /
                // press-start) still use the dialog font for their
                // prompt text.
                if phase_id == 2 && self.title_screen.is_some() {
                    return Vec::new();
                }
                let blink_on = match s.phase() {
                    TitlePhase::PressStart { blink_phase } => blink_phase < s.blink_period / 2,
                    _ => true,
                };
                // When the PROT 0888 title atlas is loaded, anchor the
                // menu text to the same centred + integer-scaled 256×256
                // stage `title_screen_sprite_draws` uses, so the menu
                // sits between the wordmark band (ends at src y=140)
                // and the press-start / copyright bands (start at src
                // y=178). Without an atlas we keep the legacy
                // (96, 100) pen so the no-disc fallback still renders.
                let atlas_present = self.title_screen.is_some();
                let pen = if atlas_present {
                    let atlas_w: u32 = 256;
                    let atlas_h: u32 = 256;
                    let scale = (surface_w / atlas_w.max(1))
                        .min(surface_h / atlas_h.max(1))
                        .clamp(1, 4) as i32;
                    let stage_x0 = (surface_w as i32 - (atlas_w as i32) * scale) / 2;
                    let stage_y0 = (surface_h as i32 - (atlas_h as i32) * scale) / 2;
                    // src-y=148 sits between the wordmark and the
                    // press-start/copyright bands; src-x=104 centres
                    // a ~6-glyph menu row inside the 256-wide stage.
                    (stage_x0 + 104 * scale, stage_y0 + 148 * scale)
                } else {
                    (96, 100)
                };
                legaia_engine_render::title_draws_for(
                    &self.font,
                    phase_id,
                    cursor,
                    s.continue_enabled,
                    blink_on,
                    atlas_present,
                    pen,
                )
            }
            BootUiState::SaveSelect(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
                    .slots()
                    .iter()
                    .map(|snap| legaia_engine_render::SaveSelectRow {
                        label: &snap.label,
                        present: snap.present,
                        party_lv: snap.party_lv,
                        play_time_seconds: snap.play_time_seconds,
                        money: snap.money,
                        location: &snap.location,
                    })
                    .collect();
                let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
                let (cursor, confirm) = match s.phase() {
                    SelectPhase::Browsing { cursor } => (cursor as usize, None),
                    SelectPhase::NowChecking { slot, .. } => (slot as usize, None),
                    SelectPhase::SlotPreview { slot } => (slot as usize, None),
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Always emit the base save-select chrome text ("Load"
                // title) so it stays visible in every Load-mode phase.
                // Skip the ASCII `>` cursor when the sprite-based
                // pointing-finger cursor is being emitted alongside
                // (i.e. when the save-menu atlas is loaded).
                let emit_text_cursor = self.save_menu.is_none();
                let mut out = legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "Load",
                    &rows,
                    cursor,
                    confirm,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                );
                // Phase-specific overlays.
                match s.phase() {
                    SelectPhase::NowChecking { .. } => {
                        // Retail slide: dialog x slides from
                        // NOW_CHECKING_SLIDE_START_X (416) to
                        // NOW_CHECKING_SLIDE_TARGET_X (160) over 16
                        // frames. Use the session's animation t to
                        // compute the per-frame x offset relative to
                        // the at-target rendering position.
                        let pos_x = legaia_engine_core::save_select::interpolate_anim(
                            (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                            (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                            s.slide_anim_t(),
                        )
                        .0;
                        let slide_offset =
                            (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
                        out.extend(legaia_engine_render::now_checking_text_draws_for(
                            &self.font,
                            stage_origin,
                            stage_scale,
                            slide_offset,
                        ));
                    }
                    SelectPhase::SlotPreview { slot } => {
                        let info = build_slot_info_view(s.slots(), slot);
                        let view = info.as_ref().map(|i| i.as_view());
                        let panel_y_offset = info_panel_slide_offset(s);
                        out.extend(legaia_engine_render::slot_info_panel_text_draws_for(
                            &self.font,
                            view.as_ref(),
                            panel_y_offset,
                            stage_origin,
                            stage_scale,
                        ));
                    }
                    _ => {}
                }
                out
            }
            BootUiState::Options(s) => {
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: &r.value,
                    })
                    .collect();
                legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    (96, 80),
                )
            }
            BootUiState::FieldMenu { session, sub } => {
                if let Some(active_sub) = sub {
                    // Render the active sub-session's overlay. Each branch
                    // builds the matching plain-data view + calls the
                    // shipped `*_draws_for` helper.
                    return self.field_menu_sub_draws(active_sub);
                }
                let view = session.view();
                let row_views: Vec<legaia_engine_render::FieldMenuRowView<'_>> = view
                    .rows
                    .iter()
                    .map(|r| legaia_engine_render::FieldMenuRowView {
                        label: r.label,
                        enabled: r.enabled,
                    })
                    .collect();
                legaia_engine_render::field_menu_draws_for(
                    &self.font,
                    &row_views,
                    view.cursor,
                    view.money,
                    view.play_time_seconds,
                    (32, 64),
                )
            }
            BootUiState::GameOver(s) => legaia_engine_render::game_over_draws_for(
                &self.font,
                s.cursor(),
                s.continue_enabled,
                (96, 100),
            ),
        }
    }

    /// Drain world field events and route them to whichever subsystem
    /// owns them. Currently:
    /// - [`FieldEvent::ActorSpawned`]: when the actor carries a non-`None`
    ///   `Actor::tmd_ref` (the `0x4C 0xD8` synchronous-spawn path), queue
    ///   the slot in [`Self::pending_dynamic_mesh_slots`] so the next
    ///   render pass uploads its mesh. ActorSpawned events without a
    ///   `tmd_ref` (the `0x4C 0x80` halt-acquire-gated bytecode-only
    ///   path) are dropped silently here - those actors have no visual
    ///   in this renderer until their bytecode runs.
    /// - All other events: not relevant to the play-window renderer yet,
    ///   surfaced via the HUD log instead by callers that want them.
    fn drain_and_route_field_events(&mut self) {
        use legaia_engine_core::field_events::FieldEvent;
        let world = &mut self.session.host.world;
        let events = world.drain_field_events();
        for ev in events {
            if let FieldEvent::ActorSpawned { slot, .. } = ev {
                let has_tmd = world
                    .actors
                    .get(slot as usize)
                    .is_some_and(|a| a.tmd_ref.is_some());
                if has_tmd {
                    self.pending_dynamic_mesh_slots.push(slot);
                }
            }
        }
    }

    /// Start windowed cutscene playback when the world has flipped into
    /// [`SceneMode::Cutscene`] (a field-VM FMV-trigger op fired). Resolves the
    /// active FMV's `MV*.STR` and decodes it: from the disc image (raw 2352-
    /// byte sectors, so the interleaved XA audio plays in sync) when booting
    /// from a disc, otherwise the video-only Form-1 extract under the extracted
    /// root. A cut/missing slot, an unresolvable path, or a decode that yields
    /// no frames drains the trigger immediately via `finish_cutscene` (no-op),
    /// matching the headless `play` loop. Leaves `self.cutscene = None` when
    /// nothing starts.
    fn try_start_windowed_cutscene(&mut self) {
        use legaia_engine_shell::cutscene_av::{decode_str_av_from_disc, decode_str_video_only};
        let Some(fmv_id) = self.session.host.world.active_fmv() else {
            return;
        };
        let Some(rel) = self.session.host.world.active_fmv_str_filename() else {
            log::info!("cutscene: fmv_id={fmv_id} (cut/unmapped slot); skipping");
            self.session.host.world.finish_cutscene();
            return;
        };

        let decoded: Option<(Vec<legaia_mdec::VideoFrame>, std::time::Duration, _)> = if let Some(
            disc_path,
        ) =
            self.disc_path.as_ref()
        {
            match resolve_iso_file(disc_path, Path::new(&rel)) {
                Ok((lba, size)) => {
                    let count = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
                    match decode_str_av_from_disc(disc_path, lba, count) {
                        Ok(av) if !av.frames.is_empty() => {
                            log::info!(
                                "cutscene: playing fmv_id={fmv_id} {rel} from disc \
                                     ({} frames, {:.2} fps, audio: {})",
                                av.frames.len(),
                                av.timing.fps,
                                if av.audio.is_some() { "yes" } else { "no" }
                            );
                            Some((av.frames, av.timing.frame_period(), av.audio))
                        }
                        Ok(_) => {
                            log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames");
                            None
                        }
                        Err(e) => {
                            log::warn!(
                                "cutscene: fmv_id={fmv_id} {rel} disc decode failed ({e:#})"
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} not on disc ({e:#})");
                    None
                }
            }
        } else if let Some(root) = self.extracted_root.as_ref() {
            let path = root.join(rel);
            match decode_str_video_only(&path) {
                Ok((frames, timing)) if !frames.is_empty() => {
                    log::info!(
                        "cutscene: playing fmv_id={fmv_id} {rel} ({} frames, {:.2} fps, no audio)",
                        frames.len(),
                        timing.fps
                    );
                    Some((frames, timing.frame_period(), None))
                }
                Ok(_) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames; skipping");
                    None
                }
                Err(e) => {
                    log::warn!(
                        "cutscene: fmv_id={fmv_id} {} decode failed ({e:#}); skipping",
                        path.display()
                    );
                    None
                }
            }
        } else {
            log::info!("cutscene: fmv_id={fmv_id} (no disc / extracted root); skipping");
            None
        };

        match decoded {
            Some((frames, frame_period, audio)) => {
                self.cutscene = Some(WindowedCutscene {
                    frames,
                    idx: 0,
                    uploaded: None,
                    frame_period,
                    clock: None,
                    pending_audio: audio,
                    has_audio: false,
                });
            }
            None => {
                // Drain the trigger so the field resumes next frame.
                self.session.host.world.finish_cutscene();
            }
        }
    }

    /// Render the active cutscene's current frame, paced to the stream's
    /// detected frame rate. The visible frame is `elapsed / frame_period`, so
    /// playback runs at the movie's real ~15 fps regardless of the display
    /// refresh rate (frames are held, or dropped if the host falls behind).
    /// `idx` tracks the due frame so the drain check at the top of the redraw
    /// handler resumes the field once the full duration has elapsed.
    fn render_windowed_cutscene(&mut self) {
        // Clone the audio handle before borrowing the renderer / cutscene so
        // staging the track and reading its cursor don't alias `self`.
        let audio_out = self.session.audio.clone();
        let Some(renderer) = self.win.renderer.as_ref() else {
            return;
        };
        if let Some(c) = self.cutscene.as_mut() {
            // Stage the interleaved audio on the first render so the audio
            // cursor (the A/V-sync master clock) starts with the picture. Pause
            // the scene sequencer so the cutscene track isn't layered over BGM.
            if let (Some(out), Some(track)) = (audio_out.as_ref(), c.pending_audio.take()) {
                out.set_sequencer_paused(true);
                out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                c.has_audio = true;
            }
            let now = std::time::Instant::now();
            let start = *c.clock.get_or_insert(now);
            let elapsed = now.duration_since(start).as_secs_f64();
            // A/V sync: drive the visible frame off the audio cursor while a
            // track is playing, else off wall-clock. `idx` reaching the frame
            // count signals end-of-playback to the drain check.
            let audio_secs = if c.has_audio {
                audio_out.as_ref().and_then(|o| o.xa_cursor_secs())
            } else {
                None
            };
            let due = legaia_engine_shell::cutscene_av::due_video_frame(
                audio_secs,
                elapsed,
                c.frame_period.as_secs_f64(),
            );
            c.idx = due;
            let show = due.min(c.frames.len().saturating_sub(1));
            if let Some(f) = c.frames.get(show) {
                match renderer.upload_texture(&f.rgba, f.width, f.height) {
                    Ok(tex) => c.uploaded = Some(tex),
                    Err(e) => log::warn!("cutscene upload: {e}"),
                }
            }
            match c.uploaded.as_ref() {
                Some(tex) => {
                    let _ = renderer.render(RenderTarget::Texture(tex));
                }
                None => {
                    let _ = renderer.render(RenderTarget::Clear);
                }
            }
        }
    }

    /// Drain world battle events, fold each into HP / status state, and
    /// append a one-line summary to the HUD ring. Called once per simulation
    /// tick from the redraw handler.
    fn drain_and_log_battle_events(&mut self) {
        let events = self.session.host.world.drain_battle_events();
        for ev in events {
            // Apply the gameplay-state side (currently only `ApplyArtStrike`
            // mutates HP / status; other events are visual-only here).
            self.session.host.world.fold_battle_event(&ev);
            // Surface in the HUD ring.
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(ev.summary());
        }

        // Floating damage / heal numbers: the live battle loop resolves and
        // applies HP itself, then queues a presentation-only FX per strike.
        // Feed those into the popup model and log the magnitude (the typed
        // events above are consumed inside the live loop, so this is the
        // only place per-strike damage surfaces while live).
        let fx = self.session.host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud.push_popup(
                    legaia_engine_core::battle_hud::DamagePopup::damage(f.target_slot, f.amount)
                        .crit(),
                );
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            let sign = if f.is_heal { '+' } else { '-' };
            self.battle_event_log
                .push_back(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }

        // Battle sound cues: the art-strike outcomes resolve per-strike SFX
        // cues (kind = the SfxBank id, played directly without classify_cue).
        // Enqueue each into the director's SfxScheduler at its strike-relative
        // timing, then advance the scheduler one frame and fire any matured cue
        // through the scene VAB. Drain first so the world borrow ends before
        // the director borrow. SFX touch the SPU only - no RNG - so battle
        // determinism stays bit-exact.
        let cues = self.session.host.world.drain_battle_sfx_cues();
        if let Some(bgm) = self.session.bgm.as_mut() {
            for cue in &cues {
                bgm.enqueue_sfx(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
            }
            for (id, voice) in bgm.tick_sfx_frame() {
                log::debug!("battle SFX cue {id:#04x} fired on voice {voice}");
            }
        } else {
            for cue in &cues {
                log::debug!(
                    "battle SFX cue {:#04x} @ +{} frames (actor {} -> target {}; no audio)",
                    cue.kind,
                    cue.timing_frames,
                    cue.actor_slot,
                    cue.target_slot
                );
            }
        }

        // Refresh per-slot status icons + age the popups one frame.
        if self.session.host.world.mode == SceneMode::Battle {
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &self.session.host.world.status_effects);
            }
        }
        self.battle_hud.tick();
    }

    /// Build [`TextDraw`]s for an active field-menu sub-session. Each
    /// variant maps to the matching `*_draws_for` helper in
    /// `legaia-engine-render`. Renderer-side state stays in this method
    /// so the sub-session enums in `legaia-engine-core` can stay
    /// renderer-agnostic.
    fn field_menu_sub_draws(
        &self,
        sub: &legaia_engine_core::field_menu_dispatch::FieldMenuSubsession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        match sub {
            FieldMenuSubsession::Status(s) => {
                let Some(snap) = s.current() else {
                    return Vec::new();
                };
                let stat_rows: Vec<legaia_engine_render::StatusStatRow<'_>> = snap
                    .stats
                    .iter()
                    .zip(snap.stat_labels.iter())
                    .map(|(v, l)| legaia_engine_render::StatusStatRow {
                        label: l,
                        value: *v as u32,
                    })
                    .collect();
                let equip_rows: Vec<(&str, &str)> = snap
                    .equip
                    .iter()
                    .map(|e| (e.label, e.item_name.as_str()))
                    .collect();
                let view = legaia_engine_render::StatusPanelView {
                    name: &snap.name,
                    level: snap.level,
                    xp: snap.xp,
                    xp_to_next: snap.xp_to_next,
                    hp: snap.hp,
                    hp_max: snap.hp_max,
                    mp: snap.mp,
                    mp_max: snap.mp_max,
                    ap: snap.ap,
                    ap_max: snap.ap_max,
                    stat_rows: &stat_rows,
                    equip_rows: &equip_rows,
                };
                legaia_engine_render::status_screen_draws_for(
                    &self.font,
                    &view,
                    Some("L1/R1: Switch  Circle: Back"),
                    (32, 32),
                )
            }
            FieldMenuSubsession::Config(s) => {
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: &r.value,
                    })
                    .collect();
                legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    (96, 80),
                )
            }
            FieldMenuSubsession::Save(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
                    .slots()
                    .iter()
                    .map(|snap| legaia_engine_render::SaveSelectRow {
                        label: &snap.label,
                        present: snap.present,
                        party_lv: snap.party_lv,
                        play_time_seconds: snap.play_time_seconds,
                        money: snap.money,
                        location: &snap.location,
                    })
                    .collect();
                let (cursor, confirm) = match s.phase() {
                    SelectPhase::Browsing { cursor } => (cursor as usize, None),
                    // Load-mode NowChecking / SlotPreview phases render
                    // separately (see slot_preview_draws / now_checking
                    // overlay below); pass through to a plain cursor.
                    SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                        (slot as usize, None)
                    }
                    SelectPhase::ConfirmOverwrite { slot, cursor } => {
                        (slot as usize, Some(("Overwrite slot?", cursor)))
                    }
                    SelectPhase::ConfirmDelete { slot, cursor } => {
                        (slot as usize, Some(("Delete slot?", cursor)))
                    }
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Field-menu Save subsession reuses the load-screen
                // chrome stage so the panel/pill sprites match retail
                // positions even when entered mid-game.
                let (sw, sh) = self
                    .win
                    .renderer()
                    .map(|r| r.surface_size())
                    .unwrap_or((1, 1));
                let (stage_origin, stage_scale) = self.save_select_stage(sw, sh);
                let emit_text_cursor = self.save_menu.is_none();
                legaia_engine_render::save_select_draws_for(
                    &self.font,
                    "Save",
                    &rows,
                    cursor,
                    confirm,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                )
            }
            FieldMenuSubsession::Spells(s) => {
                use legaia_engine_core::spell_menu::SpellMenuPhase;
                let names: Vec<&str> = s.party().iter().map(|c| c.name.as_str()).collect();
                let hp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.hp, c.hp)).collect();
                let mp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.mp, c.mp)).collect();
                let spell_rows = s.current_spell_rows();
                let spell_views: Vec<legaia_engine_render::SpellRowView<'_>> = spell_rows
                    .iter()
                    .map(|sr| legaia_engine_render::SpellRowView {
                        name: sr.name.as_str(),
                        mp_cost: sr.mp_cost,
                        admissible: sr.admissible,
                    })
                    .collect();
                let target_views: Vec<legaia_engine_render::SpellTargetView<'_>> = s
                    .targets()
                    .iter()
                    .map(|t| legaia_engine_render::SpellTargetView {
                        name: t.name.as_str(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        alive: t.alive(),
                    })
                    .collect();
                let (selected_caster, selected_spell, phase, cursor) = match s.phase() {
                    SpellMenuPhase::CharSelect { cursor } => (None, None, 0u8, *cursor),
                    SpellMenuPhase::SpellSelect { caster, cursor } => {
                        (Some(*caster), None, 1u8, *cursor)
                    }
                    SpellMenuPhase::TargetSelect {
                        caster,
                        spell_id,
                        cursor,
                    } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
                    SpellMenuPhase::Done(_) => return Vec::new(),
                };
                let names_arr: Vec<&str> = names.to_vec();
                let args = legaia_engine_render::SpellMenuDrawArgs {
                    party_names: &names_arr,
                    party_hp: &hp,
                    party_mp: &mp,
                    selected_caster,
                    spells: &spell_views,
                    selected_spell,
                    targets: &target_views,
                    selected_target: None,
                    cursor,
                    phase,
                };
                legaia_engine_render::spell_menu_draws_for(&self.font, args, (32, 32))
            }
            FieldMenuSubsession::Items(s) => self.items_session_draws(s),
            FieldMenuSubsession::Equip { session, char_slot } => {
                self.equip_session_draws(session, *char_slot)
            }
            FieldMenuSubsession::Arts(s) => self.arts_session_draws(s),
        }
    }

    /// Build draws for the inventory item-use overlay. Resolves item
    /// names through `ItemCatalog`, party / monster targets through the
    /// session's `targets` field. Drives both browsing and target-select
    /// phases via `inventory_use_draws_for`.
    fn items_session_draws(
        &self,
        s: &legaia_engine_core::inventory_use::InventoryUseSession,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::inventory_use::InventoryUseState;
        // Each visible item row needs its name + count + admissibility.
        // The session's `filtered_items` already lists indices into
        // `items` that pass the context filter; we render every owned
        // item but dim the ones outside the filter.
        let filter_set: std::collections::HashSet<usize> =
            s.filtered_items.iter().copied().collect();
        // Count duplicate item-ids so the overlay shows one row per
        // unique id rather than one row per stack slot.
        let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        for id in &s.items {
            *counts.entry(*id).or_insert(0) =
                counts.get(id).copied().unwrap_or(0).saturating_add(1);
        }
        // Stable order from first-seen.
        let mut seen: std::collections::HashSet<u8> = std::collections::HashSet::new();
        let mut row_data: Vec<(String, u8, bool)> = Vec::new();
        for (i, id) in s.items.iter().enumerate() {
            if !seen.insert(*id) {
                continue;
            }
            let entry = s.catalog.get(*id);
            let name = entry
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| format!("Item {id:02X}"));
            let count = counts.get(id).copied().unwrap_or(1);
            let admissible = filter_set.contains(&i);
            row_data.push((name, count, admissible));
        }
        let item_rows: Vec<legaia_engine_render::InventoryItemRow<'_>> = row_data
            .iter()
            .map(|(n, c, a)| legaia_engine_render::InventoryItemRow {
                name: n,
                count: *c,
                admissible: *a,
            })
            .collect();
        let target_rows: Vec<legaia_engine_render::InventoryTargetRow<'_>> = s
            .targets
            .iter()
            .map(|t| legaia_engine_render::InventoryTargetRow {
                name: &t.name,
                hp: t.hp,
                hp_max: t.hp_max,
                mp: t.mp,
                mp_max: t.mp_max,
                alive: t.alive,
            })
            .collect();
        let (phase, cursor) = match s.state {
            InventoryUseState::Browsing { cursor } => (0u8, cursor as u8),
            InventoryUseState::TargetSelect { cursor, .. } => (1u8, cursor as u8),
            _ => (0u8, 0),
        };
        let selected_item_name = s.current_item().map(|e| e.name);
        let in_battle = matches!(
            s.context,
            legaia_engine_core::inventory_use::InventoryContext::Battle
        );
        let args = legaia_engine_render::InventoryUseDrawArgs {
            items: &item_rows,
            targets: &target_rows,
            in_battle,
            cursor,
            phase,
            selected_item_name,
        };
        legaia_engine_render::inventory_use_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the equipment overlay. Resolves slot labels
    /// through `EquipSlot::label`, candidate names from the engine's
    /// equipment catalog, and per-candidate stat deltas by diffing the
    /// active modifier against the slot's current occupant.
    fn equip_session_draws(
        &self,
        session: &legaia_engine_core::equip_session::EquipSession,
        char_slot: u8,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::equip_session::EquipState;
        use legaia_engine_core::equipment::EquipSlot;

        // Display name comes from the world's roster snapshot; fall back
        // to "Slot N" if the world doesn't have a record for the slot.
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let record = session.record();
        let mut slot_label_buf: Vec<String> = Vec::with_capacity(8);
        for i in 0..8u8 {
            let label = EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"));
            slot_label_buf.push(label);
        }
        let mut slot_item_buf: Vec<String> = Vec::with_capacity(8);
        for &id in record.equip.iter() {
            slot_item_buf.push(if id == 0 {
                "(empty)".to_string()
            } else {
                format!("Item {id:02X}")
            });
        }
        let slot_rows: Vec<legaia_engine_render::EquipSlotRow<'_>> = (0..8usize)
            .map(|i| legaia_engine_render::EquipSlotRow {
                label: &slot_label_buf[i],
                current_name: &slot_item_buf[i],
            })
            .collect();

        let (phase, cursor, active_slot, confirm_label_owned) = match session.state() {
            EquipState::SlotPicker { cursor } => (
                legaia_engine_render::EquipDrawPhase::SlotPicker,
                cursor as u16,
                cursor,
                None,
            ),
            EquipState::ItemPicker { slot, cursor } => (
                legaia_engine_render::EquipDrawPhase::ItemPicker,
                cursor,
                slot,
                None,
            ),
            EquipState::Confirm {
                slot,
                item_id,
                cursor,
            } => {
                let label = format!("Equip Item {item_id:02X}?");
                (
                    legaia_engine_render::EquipDrawPhase::Confirm,
                    cursor as u16,
                    slot,
                    Some(label),
                )
            }
            EquipState::Done(_) => (legaia_engine_render::EquipDrawPhase::SlotPicker, 0, 0, None),
        };

        // Candidates only matter when we're past the slot picker.
        let (candidate_names, candidate_meta): (Vec<String>, Vec<(u8, i16, i16)>) =
            if phase == legaia_engine_render::EquipDrawPhase::SlotPicker {
                (Vec::new(), Vec::new())
            } else {
                let items = session.items_for_slot(active_slot);
                let current_id = record.equip[active_slot as usize];
                let current_mod = session
                    .equipment()
                    .get(current_id)
                    .copied()
                    .unwrap_or_default();
                let names: Vec<String> = items
                    .iter()
                    .map(|it| format!("Item {:02X}", it.id))
                    .collect();
                let meta: Vec<(u8, i16, i16)> = items
                    .iter()
                    .map(|it| {
                        let cand_mod = session.equipment().get(it.id).copied().unwrap_or_default();
                        let count = session.inventory().get(&it.id).copied().unwrap_or(0);
                        (
                            count,
                            cand_mod.atk - current_mod.atk,
                            cand_mod.udf - current_mod.udf,
                        )
                    })
                    .collect();
                (names, meta)
            };
        let candidate_rows: Vec<legaia_engine_render::EquipCandidateRow<'_>> = candidate_meta
            .iter()
            .enumerate()
            .map(
                |(i, (count, da, du))| legaia_engine_render::EquipCandidateRow {
                    name: &candidate_names[i],
                    count: *count,
                    atk_delta: *da,
                    udf_delta: *du,
                },
            )
            .collect();

        let args = legaia_engine_render::EquipDrawArgs {
            character_name: &character_name,
            slots: &slot_rows,
            candidates: &candidate_rows,
            phase,
            cursor,
            active_slot,
            confirm_label: confirm_label_owned.as_deref(),
        };
        legaia_engine_render::equipment_session_draws_for(&self.font, args, (16, 32))
    }

    /// Build draws for the Tactical Arts editor overlay. Pulls the
    /// saved-chain library snapshot the editor took at construction; the
    /// editor's `library_view` is the authoritative source until the
    /// engine calls `apply_outcome`.
    fn arts_session_draws(
        &self,
        s: &legaia_engine_core::tactical_arts_editor::ChainEditor,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::tactical_arts_editor::{ChainLibrary, EditorPhase};
        let char_slot = s.char_slot();
        let names = legaia_engine_core::field_menu_dispatch::roster_names(&self.session.host.world);
        let character_name = names
            .get(char_slot as usize)
            .cloned()
            .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

        let saved = s.library_view();
        let pretty_buf: Vec<String> = saved.iter().map(|c| c.pretty_sequence()).collect();
        let saved_rows: Vec<legaia_engine_render::ArtsChainRow<'_>> = saved
            .iter()
            .enumerate()
            .map(|(i, c)| legaia_engine_render::ArtsChainRow {
                name: &c.name,
                pretty_sequence: &pretty_buf[i],
            })
            .collect();

        let (phase_tag, browse_cursor, editing_pretty_owned, editing_len, naming_name_owned) =
            match s.phase() {
                EditorPhase::Browsing { cursor } => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    *cursor,
                    String::new(),
                    0usize,
                    String::new(),
                ),
                EditorPhase::Editing { working } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Editing,
                        0u8,
                        pretty,
                        working.len(),
                        String::new(),
                    )
                }
                EditorPhase::Naming { working, name } => {
                    let pretty = working
                        .iter()
                        .map(|c| match c {
                            legaia_art::queue::Command::Left => "L",
                            legaia_art::queue::Command::Right => "R",
                            legaia_art::queue::Command::Up => "U",
                            legaia_art::queue::Command::Down => "D",
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (
                        legaia_engine_render::ArtsEditorPhase::Naming,
                        0u8,
                        pretty,
                        working.len(),
                        name.clone(),
                    )
                }
                EditorPhase::Done(_) => (
                    legaia_engine_render::ArtsEditorPhase::Browsing,
                    0u8,
                    String::new(),
                    0usize,
                    String::new(),
                ),
            };

        let can_add_new = saved.len() < ChainLibrary::MAX_SLOTS;
        let args = legaia_engine_render::ArtsEditorDrawArgs {
            character_name: &character_name,
            phase: phase_tag,
            saved: &saved_rows,
            browse_cursor,
            editing_pretty: &editing_pretty_owned,
            editing_len,
            min_len: ChainLibrary::MIN_LEN,
            max_len: ChainLibrary::MAX_LEN,
            naming_name: &naming_name_owned,
            can_add_new,
        };
        legaia_engine_render::tactical_arts_editor_draws_for(&self.font, args, (16, 32))
    }
}

impl PlayWindowApp {
    /// Resolve the field static-geometry placement draws for the current
    /// scene: each placed environment object's scene-pack mesh paired with a
    /// world model matrix. Built from the field map's object table
    /// (`Scene::field_object_placements`) and the scene_asset_table TMD pack;
    /// the per-object pack index resolves via `legaia_asset::field_objects`.
    ///
    /// `tmd_src_index[j]` is the `res.tmds` index of uploaded mesh `j` (meshes
    /// skip empty-prim TMDs, so this bridges back). Returns empty for scenes
    /// with no field map / no bundle (e.g. battle or world-map blocks).
    ///
    /// World Y is left at the ground plane for now; the per-tile floor-height
    /// LUT (MAN header) is a separate refinement.
    fn resolve_field_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        let placements = match scene.field_object_placements(&self.session.host.index) {
            Ok(Some(p)) if !p.is_empty() => p,
            _ => return Vec::new(),
        };
        self.resolve_placement_draws(res, tmd_src_index, &placements)
    }

    /// World-map continent terrain draws: the dense visible-tile set
    /// (`Scene::field_terrain_tiles`, the `FUN_801F69D8` overhead sweep) rather
    /// than the placed-flag interactive objects. Tiles whose pack index falls
    /// outside the loaded slot-1 landmark pack (they reference the wider global
    /// TMD pool, not yet loaded for the world map) resolve to no mesh and are
    /// skipped by `resolve_placement_draws`.
    fn resolve_world_map_terrain_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        // Free-roam walk view: read the *walk* `.MAP` (`Scene::walk_field_map_
        // index`, the `block_start - 2` entry the runtime resolves through
        // `toc[idx+2]`) and sweep its `0x1000`-gated continent (`walk_terrain_
        // tiles`), then the placed-flag landmarks from the same `.MAP`. The
        // earlier path read the within-block decoy entry with the overhead
        // `0x2000` gate, which for the kingdoms resolved a different map and
        // produced the sparse mesh scatter.
        // Only the sparse placed landmarks (FUN_8003A55C, flags & 0x4) resolve
        // to slot-1 pack meshes via record[+0x10]+prefix. The bulk continent
        // ground is NOT per-cell pack meshes (the old `walk_terrain_tiles`
        // sweep floods 97% of cells with pool-5 because their record[+0x10] is
        // 0); it is the heightfield surface built separately in `upload_assets`
        // (`Scene::walk_heightfield`). See docs/subsystems/world-map.md.
        let tiles = match scene.walk_object_placements(&self.session.host.index) {
            Ok(Some(t)) => t,
            _ => Vec::new(),
        };
        if tiles.is_empty() {
            return Vec::new();
        }
        self.resolve_placement_draws(res, tmd_src_index, &tiles)
    }

    /// Resolve the world-map ocean CLUT animation for the active scene: scan the
    /// scene's PROT entries for the kingdom bundle, decode its slot-0 TIM_LIST,
    /// and pull the ocean tile's 13-frame CLUT animation table
    /// ([`legaia_asset::ocean::find_ocean_assets`]). Returns `None` for a
    /// non-world-map scene or a bundle without the ocean tile. The ocean tile
    /// texture and its base CLUT are already uploaded into VRAM by the slot-0
    /// TIM pass; this only recovers the per-frame palette overrides.
    fn resolve_ocean_anim(&self) -> Option<OceanAnim> {
        let scene = self.session.host.scene.as_ref()?;
        if !legaia_engine_core::scene::is_world_map_scene(&scene.name) {
            return None;
        }
        for entry in &scene.entries {
            let Ok(slot0) = legaia_asset::kingdom_bundle::decode_slot(&entry.bytes, 0) else {
                continue;
            };
            if let Some(ocean) = legaia_asset::ocean::find_ocean_assets(&slot0)
                && ocean.animation_frames.len() >= 32
            {
                return Some(OceanAnim {
                    frames: ocean.animation_frames,
                    cur: 0,
                    tick: 0,
                });
            }
        }
        None
    }

    /// Advance the world-map ocean CLUT animation one sim tick. When the frame
    /// cursor crosses [`OCEAN_ANIM_TICKS_PER_FRAME`], write the next frame's 16
    /// BGR555 entries into the CPU VRAM CLUT row at `(0, 506)` and re-upload the
    /// VRAM so the heightfield's water cells (which sample that CLUT) shimmer.
    /// No-op when no ocean animation is loaded. Cheap: the whole-VRAM re-upload
    /// fires only on a frame change (~10x/s), not every render frame.
    fn advance_ocean_animation(&mut self) {
        let frame: [u8; 32] = {
            let Some(anim) = self.ocean_anim.as_mut() else {
                return;
            };
            anim.tick += 1;
            if anim.tick < OCEAN_ANIM_TICKS_PER_FRAME {
                return;
            }
            anim.tick = 0;
            let nframes = anim.frames.len() / 32;
            if nframes == 0 {
                return;
            }
            anim.cur = (anim.cur + 1) % nframes;
            let off = anim.cur * 32;
            anim.frames[off..off + 32].try_into().unwrap()
        };
        let Some(base) = self.cpu_vram_base.as_mut() else {
            return;
        };
        // CLUT row at VRAM (0, 506) - the retail per-frame ocean DMA target.
        base.write_clut_row(0, 506, &frame);
        if let Some(r) = self.win.renderer.as_ref() {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: ocean CLUT re-upload: {e:#}"),
            }
        }
    }

    /// Shared placement -> world-transform resolver for both the field static-
    /// object layer and the world-map continent terrain. Maps each placement's
    /// scene-pack mesh index through the uploaded-mesh bridge and builds its
    /// world model matrix.
    fn resolve_placement_draws(
        &self,
        res: &SceneResources,
        tmd_src_index: &[usize],
        placements: &[legaia_asset::field_objects::Placement],
    ) -> Vec<(usize, Mat4)> {
        let Some(scene) = self.session.host.scene.as_ref() else {
            return Vec::new();
        };
        if placements.is_empty() {
            return Vec::new();
        }
        // Per-tile floor-height LUT (MAN header). World Y for a placed object
        // is `-lut[tile_floor_nibble] + y_off`; without it the town renders on
        // a flat plane (Rim Elm is on a cliff with real elevation changes).
        let floor_lut = scene
            .field_floor_height_lut(&self.session.host.index)
            .ok()
            .flatten();
        // The environment meshes are the scene_asset_table bundle entry's TMD
        // pack, in scan order; `pack_index` indexes that subset of `res.tmds`.
        let Some(bundle_entry) =
            legaia_engine_core::scene_bundle::find_bundle(scene).map(|b| b.entry_idx())
        else {
            return Vec::new();
        };
        let env_tmds: Vec<usize> = res
            .tmds
            .iter()
            .enumerate()
            .filter(|(_, t)| t.entry_idx == bundle_entry)
            .map(|(i, _)| i)
            .collect();
        // res.tmds index -> uploaded-mesh index (None where the mesh was
        // dropped for having no renderable prims).
        let mut res_to_mesh: Vec<Option<usize>> = vec![None; res.tmds.len()];
        for (mesh_idx, &src) in tmd_src_index.iter().enumerate() {
            if let Some(slot) = res_to_mesh.get_mut(src) {
                *slot = Some(mesh_idx);
            }
        }
        let mut draws = Vec::new();
        for p in placements {
            let Some(pack_index) = p.pack_index else {
                continue;
            };
            let Some(&res_idx) = env_tmds.get(pack_index as usize) else {
                continue;
            };
            let Some(mesh_idx) = res_to_mesh[res_idx] else {
                continue;
            };
            // World Y from the floor-height LUT (`-lut[nibble] + y_off`), or
            // the ground plane when the LUT / nibble is unavailable.
            let world_y = match (floor_lut, p.floor_nibble) {
                (Some(lut), Some(nib)) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
                _ => 0,
            };
            // PSX field coords (same convention as actor positions), Y-flipped
            // to match the geometry like `actor_model`.
            let model = Mat4::from_translation(Vec3::new(
                p.world_x as f32,
                world_y as f32,
                p.world_z as f32,
            )) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
            draws.push((mesh_idx, model));
        }
        log::info!(
            "play-window: {} field placement draws ({} placements, {} env meshes)",
            draws.len(),
            placements.len(),
            env_tmds.len(),
        );
        draws
    }

    fn upload_assets(&mut self) {
        let Some(res) = self.scene_res.take() else {
            return;
        };
        let (
            vram_opt,
            font_opt,
            meshes,
            tmd_data,
            tmd_src_index,
            color_meshes,
            color_tmd_src_index,
            lo,
            hi,
            world_map_hf,
        ) = {
            let Some(r) = self.win.renderer.as_ref() else {
                self.scene_res = Some(res);
                return;
            };
            let vram = r
                .upload_vram(&res.vram)
                .map_err(|e| log::error!("VRAM upload: {e:#}"))
                .ok();
            let font = r
                .upload_font(&self.font)
                .map_err(|e| log::error!("font upload: {e:#}"))
                .ok();
            let mut meshes = Vec::new();
            let mut tmd_data: Vec<(legaia_tmd::Tmd, Vec<u8>)> = Vec::new();
            // `res.tmds` index for each pushed mesh (meshes skip empty-prim
            // TMDs, so this is the bridge back to a `ResolvedTmd`).
            let mut tmd_src_index: Vec<usize> = Vec::new();
            // Untextured (F*/G*) prop meshes + their res.tmds-index bridge,
            // parallel to `meshes` / `tmd_src_index` but on the colour pipeline.
            let mut color_meshes: Vec<UploadedColorMesh> = Vec::new();
            let mut color_tmd_src_index: Vec<usize> = Vec::new();
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for (src_i, rtmd) in res.tmds.iter().enumerate() {
                // Build this mesh's untextured (F*/G*) vertex-colour primitives
                // and upload them to the colour pipeline. `tmd_to_color_mesh`
                // skips textured groups, so these are DISJOINT from the VRAM-mesh
                // primitives below: a mesh carrying both textured and untextured
                // primitives now renders BOTH halves rather than only the
                // textured one (the old code built the colour mesh only when the
                // textured build came back empty, silently dropping the
                // untextured half of a mixed mesh). A mesh whose only textured
                // prims reference missing CLUTs yields an empty colour mesh AND
                // an empty filtered VRAM mesh, so it correctly stays dropped.
                let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);
                if !cmesh.is_empty() {
                    for p in &cmesh.positions {
                        for ax in 0..3 {
                            if p[ax] < lo[ax] {
                                lo[ax] = p[ax];
                            }
                            if p[ax] > hi[ax] {
                                hi[ax] = p[ax];
                            }
                        }
                    }
                    match r.upload_color_mesh(&cmesh.positions, &cmesh.colors, &cmesh.indices) {
                        Ok(m) => {
                            color_meshes.push(m);
                            color_tmd_src_index.push(src_i);
                        }
                        Err(e) => log::warn!("color mesh upload skipped: {e:#}"),
                    }
                }

                // Use the VRAM-aware filter so prims whose CBA / TSB sample
                // un-uploaded regions get dropped at mesh-build time. This
                // matches the asset-viewer's cleanup and avoids the "flat
                // green CLUT[0]" shells over correctly-textured geometry
                // that the unfiltered builder produces.
                let vmesh = rtmd.build_filtered_vram_mesh(&res.vram);
                if vmesh.indices.is_empty() {
                    // No textured prims survived the VRAM filter; the colour prims
                    // (if any) were already uploaded above.
                    continue;
                }
                let (mlo, mhi) = vmesh.aabb();
                for ax in 0..3 {
                    if mlo[ax] < lo[ax] {
                        lo[ax] = mlo[ax];
                    }
                    if mhi[ax] > hi[ax] {
                        hi[ax] = mhi[ax];
                    }
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        tmd_data.push((rtmd.tmd.clone(), rtmd.raw.clone()));
                        meshes.push(m);
                        tmd_src_index.push(src_i);
                    }
                    Err(e) => log::warn!("TMD upload skipped: {e:#}"),
                }
            }
            // World-map continent ground: build the heightfield surface from
            // the walk `.MAP` floor grid (the slot-1 pack meshes are only the
            // landmarks, not a per-cell ground mesh) and texture each cell from
            // the retail terrain-type-keyed multi-page atlas - the heightfield
            // bakes the per-cell tile UV (`+0x14`) and page+palette
            // (`+0x15`/`+0x16..+0x18`) so grass / mountain / water cells sample
            // their own VRAM page. See docs/subsystems/world-map.md
            // "Ground texturing".
            let mut world_map_hf: Option<UploadedVramMesh> = None;
            let is_world_map = self
                .session
                .host
                .scene
                .as_ref()
                .is_some_and(|s| legaia_engine_core::scene::is_world_map_scene(&s.name));
            if is_world_map
                && let Some(scene) = self.session.host.scene.as_ref()
                && let Ok(Some(hf)) = scene.walk_heightfield(&self.session.host.index)
                && !hf.indices.is_empty()
            {
                let vmesh = heightfield_to_vram_mesh(&hf);
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        log::info!(
                            "play-window: world-map heightfield {} quads ({} verts)",
                            hf.quad_count(),
                            hf.positions.len()
                        );
                        world_map_hf = Some(m);
                    }
                    Err(e) => log::warn!("heightfield upload skipped: {e:#}"),
                }
            }
            (
                vram,
                font,
                meshes,
                tmd_data,
                tmd_src_index,
                color_meshes,
                color_tmd_src_index,
                lo,
                hi,
                world_map_hf,
            )
        };
        // Resolve the field static-geometry placement draws: each placed
        // environment object -> its scene-pack mesh -> a world transform.
        // Built here (not per-frame) because the placement table + pack are
        // fixed for the scene; the field draw branch just replays the list.
        let field_placement_draws = self.resolve_field_placement_draws(&res, &tmd_src_index);
        // Same resolver, but bridged through the colour-mesh list: the untextured
        // props' placement transforms map to `color_meshes` indices.
        let field_placement_color_draws =
            self.resolve_field_placement_draws(&res, &color_tmd_src_index);
        let world_map_terrain_draws = self.resolve_world_map_terrain_draws(&res, &tmd_src_index);
        // Keep a clean CPU copy of the scene VRAM so a battle can inject
        // monster textures into a throwaway clone and restore this on exit.
        self.cpu_vram_base = Some(res.vram.clone());
        if let Some(v) = vram_opt {
            self.uploaded_vram = Some(v);
        }
        if let Some(a) = font_opt {
            self.font_atlas = Some(a);
        }
        // Upload the publisher-logos atlas (if pre-decoded at boot) now
        // that the renderer is live.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_publisher_logos_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: publisher-logos atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.publisher_logos = Some(PublisherLogosAssets {
                        rects: atlas_data.rects,
                        atlas,
                    });
                }
                Err(e) => log::warn!("publisher-logos atlas upload skipped: {e:#}"),
            }
        }
        // Upload the title-screen atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_title_screen_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: title-screen atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.title_screen = Some(TitleScreenAssets {
                        rect: atlas_data.rect,
                        atlas,
                    });
                }
                Err(e) => log::warn!("title-screen atlas upload skipped: {e:#}"),
            }
        }
        // Upload the menu-glyph atlas the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_menu_glyph_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: menu-glyph atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    self.menu_glyphs = Some(MenuGlyphAssets { atlas });
                }
                Err(e) => log::warn!("menu-glyph atlas upload skipped: {e:#}"),
            }
        }
        // Upload the save-menu UI atlas (PROT 0899) the same way.
        if let (Some(atlas_data), Some(r)) = (
            self.pending_save_menu_atlas.take(),
            self.win.renderer.as_ref(),
        ) {
            match r.upload_sprite_atlas(&atlas_data.rgba, atlas_data.width, atlas_data.height) {
                Ok(atlas) => {
                    log::info!(
                        "play-window: save-menu atlas uploaded ({}x{})",
                        atlas_data.width,
                        atlas_data.height
                    );
                    let rects = legaia_engine_render::SaveMenuAtlasRects {
                        panel_tl: atlas_data.band_panel_tl(),
                        panel_tr: atlas_data.band_panel_tr(),
                        panel_bl: atlas_data.band_panel_bl(),
                        panel_br: atlas_data.band_panel_br(),
                        panel_top: atlas_data.band_panel_top(),
                        panel_bot: atlas_data.band_panel_bot(),
                        panel_left: atlas_data.band_panel_left(),
                        panel_right: atlas_data.band_panel_right(),
                        slot1: atlas_data.band_slot1(),
                        slot2: atlas_data.band_slot2(),
                        cursor: atlas_data.band_cursor(),
                        panel_interior: atlas_data.band_panel_interior(),
                        load_empty_frame: Some(atlas_data.band_load_empty_frame()),
                        load_portrait_by_char: [
                            atlas_data.band_load_portrait(0),
                            atlas_data.band_load_portrait(1),
                            atlas_data.band_load_portrait(2),
                        ],
                    };
                    self.save_menu = Some(SaveMenuAssets { rects, atlas });
                }
                Err(e) => log::warn!("save-menu atlas upload skipped: {e:#}"),
            }
        }
        self.meshes = meshes;
        self.scene_tmd_data = tmd_data;
        self.field_placement_draws = field_placement_draws;
        self.color_meshes = color_meshes;
        self.field_placement_color_draws = field_placement_color_draws;
        self.world_map_terrain_draws = world_map_terrain_draws;
        self.world_map_heightfield = world_map_hf;
        // World-map ocean: recover the 13-frame CLUT animation for the kingdom
        // (the ocean texture + base CLUT are already uploaded by the slot-0 TIM
        // pass). `None` off the world map, so the per-tick advance self-gates.
        self.ocean_anim = self.resolve_ocean_anim();
        if lo[0].is_finite() {
            self.scene_aabb = (lo, hi);
        }
        // Bind each uploaded mesh slot to the matching actor and wire up the
        // idle animation (record 0) when the scene carries an ANM pack for
        // that actor. Registration order: actor K → TMD slot K, mirroring
        // the retail `0x8007C018` table written by `FUN_8001E890`.
        let world = &mut self.session.host.world;
        for i in 0..self.scene_tmd_data.len() {
            world.set_actor_tmd_binding(i, i);
            if let Some(pack) = res.anm_pack_for_actor(i)
                && let Some(record_bytes) = pack.record_bytes(0)
            {
                let bone_count = self.scene_tmd_data[i].0.objects.len();
                match AnimPlayer::new(record_bytes.to_vec(), bone_count) {
                    Ok(player) => {
                        world.set_actor_animation(i, player);
                        log::info!("play-window: actor {i} animated ({bone_count} bones)");
                    }
                    Err(e) => log::warn!("play-window: actor {i} ANM init failed: {e:#}"),
                }
            }
        }
        log::info!(
            "play-window: {} meshes uploaded, VRAM {}",
            self.meshes.len(),
            if self.uploaded_vram.is_some() {
                "ready"
            } else {
                "failed"
            }
        );
    }

    fn camera_mvp(&self, aspect: f32) -> Mat4 {
        // Frame the player's vicinity, not the whole scene. Loading the full
        // town environment-geometry pack makes `scene_aabb` (the union of every
        // mesh's local extent) span thousands of units, so fitting it pulls the
        // orbit camera far enough out that the actually-drawn terrain near the
        // player shrinks to a speck. Until per-mesh world placement lands, build
        // a fixed-size framing box around the player actor (the field draws
        // actor-bound meshes at actor positions) so the view stays close.
        const FIELD_VIEW_HALF: f32 = 700.0;
        let (lo, hi) = self
            .session
            .host
            .world
            .actors
            .first()
            .filter(|p| p.active || p.tmd_binding.is_some())
            .map(|p| {
                let (cx, cy, cz) = (
                    p.move_state.world_x as f32,
                    p.move_state.world_y as f32,
                    p.move_state.world_z as f32,
                );
                (
                    [
                        cx - FIELD_VIEW_HALF,
                        cy - FIELD_VIEW_HALF,
                        cz - FIELD_VIEW_HALF,
                    ],
                    [
                        cx + FIELD_VIEW_HALF,
                        cy + FIELD_VIEW_HALF,
                        cz + FIELD_VIEW_HALF,
                    ],
                )
            })
            .unwrap_or((self.scene_aabb.0, self.scene_aabb.1));
        orbit_camera_mvp(lo, hi, 0.25, 0.4, self.win.elapsed_secs(), aspect)
    }

    /// Battle camera: frame the **monster** actors (the ones carrying a bound
    /// mesh + idle animation) rather than the player vicinity. The live-loop
    /// seats battle actors around the world origin (`enter_battle(.., 600)` —
    /// party at `x=-600`, monsters at `x=+600`), far from the field player's
    /// world coords, so `camera_mvp`'s player-centred box leaves the enemies
    /// entirely off-screen. Framing the enemy cluster (gently orbiting) puts
    /// the animated monsters centre-frame and at a useful size.
    fn battle_camera_mvp(&self, aspect: f32) -> Mat4 {
        let world = &self.session.host.world;
        let pc = world.party_count as usize;
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut any = false;
        for (i, a) in world.actors.iter().enumerate() {
            // Monster slots only (party occupies slots 0..party_count and isn't
            // mesh-bound in the play-window battle path anyway).
            if i < pc || a.tmd_binding.is_none() {
                continue;
            }
            let p = [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            any = true;
        }
        if !any {
            // No bound monsters yet — fall back to the field framing.
            return self.camera_mvp(aspect);
        }
        // The bare position box collapses to a point/line; expand it to enclose
        // the monster mesh bodies (a few hundred units tall/wide).
        const M: f32 = 450.0;
        for k in 0..3 {
            lo[k] -= M;
            hi[k] += M;
        }
        // Gentle orbit (slower than the field's 0.25) so the animated enemies
        // read in 3D from several angles without spinning fast.
        orbit_camera_mvp(lo, hi, 0.12, 0.35, self.win.elapsed_secs(), aspect)
    }

    /// Battle orbit yaw in radians, at the **retail rate**. The battle tick
    /// (`FUN_801D0748`) decrements the camera yaw `_DAT_8007b792` by
    /// `DAT_1f800393 * 2` (≈2) per frame while idle: ≈ -4 units/frame, and a
    /// PSX turn is 4096 units, so the idle orbit is `4*60/4096` turn/s ≈ 0.059
    /// turn/s. Decreasing yaw = retail's spin sense.
    fn battle_orbit_yaw_rad(&self) -> f32 {
        const RETAIL_UNITS_PER_SEC: f32 = 4.0 * 60.0; // -4 u/frame at 60 fps
        -self.win.elapsed_secs() * RETAIL_UNITS_PER_SEC / 4096.0 * std::f32::consts::TAU
    }

    /// The **exact** retail overworld-battle camera (game mode `0x15`), pinned
    /// from the four fingerprinted `overworld_battle_bg_angle_*` saves and
    /// `FUN_80026988`/`FUN_80026f50`. For a PSX (Y-down) world vertex `v` retail
    /// computes `screen = H * (R*v + TR) / Ze` with
    ///   `R  = Rx(pitch=32u) * Ry(yaw)`         (12-bit angles, 4096 = 360°),
    ///   `TR = (0, 1280, 7680)`                 (eye-space: depth 7680, height 1280),
    ///   `H  = 256`                             (GTE projection focal length),
    /// the look-at target is the world origin, and PSX screen `+Y` is **down**
    /// with screen-centre `(160, 120)` over the 320x240 frame.
    ///
    /// The engine draws its meshes Y-flipped (`scale(1,-1,1)` = `F`, PSX Y-down
    /// -> renderer Y-up), so this builds `cam = Proj_H * T(TR) * R * F`: every
    /// battle draw is `cam * model` where `model` already carries an `F` (the
    /// dome's plain flip, the actors' `Translate * F`), and `F*F = I` recovers
    /// the raw PSX vertex the retail transform expects. Verified by projecting
    /// PROT 88's dome through this matrix and matching the savestate framebuffer
    /// (sky / mountain-ring / horizon). See `project_battle_camera_re`.
    /// The exact retail dome projection (`tr = (0,1280,7680)`), kept as the
    /// camera-RE reference and the regression-test target. The live battle uses
    /// the unified [`battle_dome_camera_mvp`] (closer depth) for a coherent
    /// single-camera scene; this stays as the pinned ground truth.
    #[allow(dead_code)]
    fn retail_battle_mvp(yaw_rad: f32, aspect: f32) -> Mat4 {
        Self::battle_mvp_with_tr(yaw_rad, Vec3::new(0.0, 1280.0, 7680.0), aspect)
    }

    /// The shared battle projection-times-view for a given eye-space translation
    /// `tr`. Retail keeps a single rotation `R = Rx(32u)·Ry(yaw)` (stored
    /// rotation-only in `DAT_8007bf10`) and applies the translation per draw
    /// class: the backdrop gets `tr = (0, 1280, 7680)` (pushed far), the actors
    /// get their own (closer) translation off the rotation-only matrix
    /// ([`FUN_80048A08`] composes each actor's world transform onto `8007bf10`,
    /// NOT onto the backdrop's `7680`-deep matrix). Sharing `R` keeps the
    /// foreground and backdrop orbiting in lock-step; the differing `tr.z`
    /// is what lets the party read large while the dome sits on the horizon.
    fn battle_mvp_with_tr(yaw_rad: f32, tr: Vec3, aspect: f32) -> Mat4 {
        const H: f32 = 256.0;
        const PITCH_UNITS: f32 = 32.0;
        let pitch = PITCH_UNITS / 4096.0 * std::f32::consts::TAU;
        let r = Mat4::from_rotation_x(pitch) * Mat4::from_rotation_y(yaw_rad);
        let t = Mat4::from_translation(tr);
        let f = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        // PSX perspective onto a 320x240 frame: ndc.x = H*Ex/(160*Ez),
        // ndc.y = -H*Ey/(120*Ez) (PSX +Y down -> NDC up), clip.w = Ez, depth
        // mapped to wgpu [0,1]. Correct X for non-4:3 viewports so the 4:3
        // retail framing holds at any window size.
        let (near, far) = (4.0f32, 60000.0f32);
        let a = far / (far - near);
        let b = -near * far / (far - near);
        let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
        let proj = Mat4::from_cols(
            Vec4::new(H / 160.0 * aspect_fix, 0.0, 0.0, 0.0),
            Vec4::new(0.0, -H / 120.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, a, 1.0),
            Vec4::new(0.0, 0.0, b, 0.0),
        );
        proj * t * r * f
    }

    /// The single battle camera, used for **everything** in a stage-dome battle
    /// (dome, ground grid, and actors) so the scene reads as one coherent space
    /// rather than two overlapping layers. The retail dome projection wants
    /// `tr.z = 7680`, but that shoves the foreground actors onto the same far
    /// plane (tiny); driving a *separate* close camera for the actors instead
    /// split the horizon (the grass grid and the dome no longer met). A single
    /// middle depth keeps the dome's huge radius reading at the horizon while
    /// the party/enemies near the origin read at roughly retail scale. `tr.y`
    /// holds the dome's `1/6` down-shift ratio so the action sits just below
    /// centre. (The exact `tr.z = 7680` projection is preserved in
    /// [`retail_battle_mvp`] for the camera-RE reference + regression test.)
    fn battle_dome_camera_mvp(&self, aspect: f32) -> Mat4 {
        // Unified close depth so the SMALL battle meshes (party + monsters are
        // only ~130-370 units tall) read at a usable size while the dome's huge
        // radius still sits on the horizon. The probe-confirmed exact dome
        // projection is tr.z=7680, but at that true depth these small meshes are
        // only a few pixels (retail draws the actors off a separate close
        // matrix, not the backdrop's 7680 plane). Kept as the playable
        // single-camera compromise; the exact projection lives in
        // `retail_battle_mvp`.
        const DEPTH: f32 = 1500.0;
        Self::battle_mvp_with_tr(
            self.battle_orbit_yaw_rad(),
            Vec3::new(0.0, DEPTH / 6.0, DEPTH),
            aspect,
        )
    }

    /// Camera parameters for the cutscene shot, decoded from the cutscene
    /// timeline's executed op-`0x45` Camera Configure params (read from
    /// `World::camera_state`, committed by `FUN_801DE084`). Returns
    /// `(look_at, yaw_radians, fov_radians)`:
    ///
    /// - **look_at**: the camera focus. Retail stores the *negated* focus X / Z
    ///   in params 6 / 8 (`_DAT_80089118` / `_DAT_80089120` = the GTE
    ///   translation `-focus`; the follow-cam `FUN_801DBE9C` sets them to
    ///   `-(anchor X/Z)`), so X / Z are negated back to world space here; Y
    ///   (param 7) is stored un-negated. Any axis the cutscene hasn't staged
    ///   yet falls back to the lead actor (the cutscene anchor), then the
    ///   scene-AABB centre.
    /// - **yaw**: param 1 (`_DAT_8007b792`, camera yaw), PSX `4096` = full turn.
    /// - **fov**: derived from param 9 (`_DAT_8007b6f4`), which retail writes to
    ///   the GTE H projection register - the focal length. PSX projects onto a
    ///   ~240-tall frame, so the vertical FOV is `2*atan(120 / H)`. Inferred;
    ///   falls back to 60 deg when the param is absent or degenerate.
    fn cutscene_view(&self) -> ([f32; 3], f32, f32, f32) {
        use std::f32::consts::TAU;
        let world = &self.session.host.world;
        let params = &world.camera_state.params;
        let param = |slot: u8| {
            params
                .iter()
                .find(|p| p.slot == slot)
                .map(|p| p.value as i16 as f32)
        };
        let (px, py, pz) = world
            .actors
            .first()
            .filter(|a| a.active || a.tmd_binding.is_some())
            .map(|a| {
                (
                    a.move_state.world_x as f32,
                    a.move_state.world_y as f32,
                    a.move_state.world_z as f32,
                )
            })
            .unwrap_or_else(|| {
                (
                    (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                    (self.scene_aabb.0[1] + self.scene_aabb.1[1]) * 0.5,
                    (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                )
            });
        let look_at = [
            param(6).map(|v| -v).unwrap_or(px),
            param(7).unwrap_or(py),
            param(8).map(|v| -v).unwrap_or(pz),
        ];
        let yaw = param(1).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        // Slot 0 = op-0x45 camera pitch (`_DAT_8007B790`, GTE RotMatrixX angle,
        // 12-bit / 4096 = 360 deg). Beats that omit it default to the prior
        // fixed ~24 deg downward framing so absent-pitch shots are unchanged.
        let pitch = param(0)
            .map(|v| v / 4096.0 * TAU)
            .unwrap_or_else(|| 0.45f32.atan());
        let fov = param(9)
            .filter(|&h| h > 1.0)
            .map(|h| 2.0 * (120.0 / h).atan())
            .unwrap_or(60f32.to_radians());
        (look_at, pitch, yaw, fov)
    }

    fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.session.host.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        Mat4::from_translation(pos) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
    }

    /// The monster stat archive (PROT 867) bytes, decoded + cached on first
    /// use. `None` if no disc is attached or the entry can't be read.
    fn monster_archive_bytes(&mut self) -> Option<std::sync::Arc<Vec<u8>>> {
        if self.monster_archive.is_none() {
            const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;
            match self
                .session
                .host
                .index
                .entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY)
            {
                Ok(b) => self.monster_archive = Some(std::sync::Arc::new(b)),
                Err(e) => {
                    log::warn!("play-window: monster archive (PROT 867) load skipped: {e:#}");
                    return None;
                }
            }
        }
        self.monster_archive.clone()
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    fn sync_battle_render(&mut self) {
        let mode = self.session.host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev == Some(mode) {
            return;
        }
        match (prev, mode) {
            (_, SceneMode::Battle) => self.enter_battle_render(),
            (Some(SceneMode::Battle), _) => self.exit_battle_render(),
            _ => {}
        }
    }

    /// Bridge the decoded monster meshes for the current battle into the draw
    /// list: inject each enemy's texture pool into a clone of the field VRAM
    /// at the loader's per-slot coords, upload the relocated mesh, and bind it
    /// to the enemy actor. Re-uploads the edited VRAM so the injected texture
    /// pages resolve.
    /// Build the current scene's battle-stage backdrop, if it has one. Returns
    /// the battle VRAM (scene + stage-dome textures resident) and the stage
    /// dome's `(Tmd, raw)`. The faithful battle backdrop is the scene's
    /// `scene_tmd_stream` half-dome (sky + mountain ring + ground); building the
    /// scene in `SceneLoadKind::Battle` makes that dome TMD + its textures
    /// resident (the Field build excludes them). `None` when the scene has no
    /// stage entry.
    fn build_battle_stage(&self) -> Option<(legaia_tim::Vram, (legaia_tmd::Tmd, Vec<u8>))> {
        let scene = self.session.host.scene.as_ref()?;
        let scene_name = scene.name.clone();
        let stage_entry = *self
            .session
            .host
            .index
            .battle_stage_entries(&scene_name)
            .first()?;
        let mut shared: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            if let Ok(s) = Scene::load(&self.session.host.index, name) {
                shared.push(s);
            }
        }
        let refs: Vec<&Scene> = shared.iter().collect();
        let (res, _) = SceneResources::build_targeted_with_options(
            scene,
            &refs,
            BuildOptions {
                kind: SceneLoadKind::Battle,
                upload_all_tims: true,
            },
        )
        .ok()?;
        // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
        let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
        log::info!(
            "play-window: battle stage = scene '{scene_name}' PROT {stage_entry} \
             ({} objects)",
            dome.tmd.objects.len()
        );
        Some((res.vram.clone(), (dome.tmd.clone(), dome.raw.clone())))
    }

    fn enter_battle_render(&mut self) {
        let monsters = self.session.host.world.battle_monster_slots();
        if monsters.is_empty() {
            return;
        }
        let Some(field_base) = self.cpu_vram_base.clone() else {
            return;
        };
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        // Build the battle-stage backdrop (the scene's scene_tmd_stream
        // half-dome). Its VRAM (scene + stage-dome textures resident) becomes
        // the battle base, so the dome renders textured behind the actors;
        // fall back to the field VRAM when the scene has no stage.
        let stage = self.build_battle_stage();
        let base = match &stage {
            Some((sv, _)) => sv.clone(),
            None => field_base,
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };

        // Work on a throwaway copy so the field VRAM stays clean for the
        // restore on battle exit.
        let mut vram = base;
        // Blit the battle effect-texture atlas (PROT 870) into the battle VRAM
        // copy. Its pages land at fb_y=0 in the same columns the field stage
        // textures occupy, so this is a battle-only upload that battle exit
        // discards (the field VRAM base is untouched). Byte-verified against
        // live battle captures; soft-fails so a missing disc entry just leaves
        // the flame pages absent.
        if let Err(e) = legaia_engine_core::scene::upload_flame_atlas_into_vram(
            &self.session.host.index,
            &mut vram,
            true,
        ) {
            log::warn!("play-window: flame-atlas VRAM upload skipped: {e:#}");
        }
        self.battle_mesh_base = self.meshes.len();
        // Upload the stage dome mesh (drawn as the backdrop). Its textures live
        // in the stage VRAM, so build it unfiltered (all textured prims are
        // resident). Appended after `battle_mesh_base`, so battle exit truncates
        // it away with the monster meshes.
        self.battle_stage_mesh = None;
        if let Some((_, (tmd, raw))) = &stage {
            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
            if !vmesh.indices.is_empty()
                && let Ok(m) = r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                )
            {
                self.battle_stage_mesh = Some(self.meshes.len());
                self.meshes.push(m);
                self.scene_tmd_data.push((tmd.clone(), raw.clone()));
                // Flat tiled ground grid under the actors (retail's
                // `func_0x801d02c0` grass grid). Reuse the dome's grass texel so
                // it samples real grass from the battle VRAM; drawn with the
                // actor camera so the party stands on it.
                self.battle_ground_mesh = None;
                if let Some(grid) = build_battle_ground_grid(&vmesh)
                    && let Ok(gm) = r.upload_vram_mesh(
                        &grid.positions,
                        &grid.uvs,
                        &grid.cba_tsb,
                        &grid.normals,
                        &grid.indices,
                    )
                {
                    self.battle_ground_mesh = Some(self.meshes.len());
                    self.meshes.push(gm);
                    self.scene_tmd_data.push((tmd.clone(), raw.clone())); // keep meshes/data aligned
                }
            }
        }
        let mut bound = 0usize;
        for (actor_idx, monster_id, slot) in monsters {
            let mesh = match legaia_asset::monster_archive::mesh(&archive, monster_id) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("play-window: monster {monster_id} mesh decode failed: {e:#}");
                    continue;
                }
            };
            // Parse the embedded TMD up front so it can be retained parallel
            // to `meshes`; `battle_render_mesh` only yields a mesh when this
            // same parse succeeds.
            let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
                continue;
            };
            let Some(vmesh) = mesh.battle_render_mesh(slot, &mut vram) else {
                continue;
            };
            if vmesh.indices.is_empty() {
                continue;
            }
            match r.upload_vram_mesh(
                &vmesh.positions,
                &vmesh.uvs,
                &vmesh.cba_tsb,
                &vmesh.normals,
                &vmesh.indices,
            ) {
                Ok(m) => {
                    let idx = self.meshes.len();
                    self.meshes.push(m);
                    // Keep `scene_tmd_data` length-parallel with `meshes`.
                    self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
                    self.session.host.world.actors[actor_idx].tmd_binding = Some(idx);
                    // Record the texture slot so the posed-animation rebuild can
                    // re-apply the per-slot CBA/TSB relocation (the raw-TMD posed
                    // mesh otherwise carries the nominal on-disc addresses and
                    // samples the wrong VRAM page → untextured/white).
                    self.session.host.world.actors[actor_idx].battle_tex_slot = Some(slot);
                    // Attach the monster's idle clip so its limbs move in
                    // battle. The clip's part count should equal the TMD object
                    // count (one rigid transform per object); MonsterAnimPlayer
                    // tolerates a mismatch (extra objects stay at rest).
                    match legaia_asset::monster_archive::idle_animation(&archive, monster_id) {
                        Ok(Some(idle)) => {
                            if let Some(player) =
                                legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
                            {
                                self.session
                                    .host
                                    .world
                                    .set_actor_battle_animation(actor_idx, player);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("play-window: monster {monster_id} idle anim decode: {e:#}")
                        }
                    }
                    bound += 1;
                }
                Err(e) => log::warn!("play-window: monster {monster_id} mesh upload: {e:#}"),
            }
        }

        // Load the REAL battle party meshes (PROT 1204 Vahn/Noa/Gala) and bind
        // them to the party actor slots, so the party renders its large
        // battle-form models (live-confirmed: actors run at scale 1.0 and sit at
        // the exact camera's `tr.z=7680` plane, so they read large only because
        // the battle MESHES are large - the field/placeholder meshes are why the
        // party was invisible/tiny). Mirrors the web-viewer's
        // `battle_char_vram_bytes_battle`: upload the 7 atlases, overlay each
        // character's decoded battle palette onto the rows its mesh CBA samples,
        // then build the mesh against that VRAM with its nominal CBA/TSB.
        let mut party_bound = 0usize;
        let party_count = self.session.host.world.party_count as usize;
        if party_count > 0
            && let Ok(pack_raw) = self
                .session
                .host
                .index
                .entry_bytes_extended(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
            && let Ok(pack) = legaia_asset::battle_char_pack::parse(&pack_raw)
        {
            // Upload the 7 character atlases (256x256 4bpp + their CLUT rows).
            for atlas in &pack.atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
                }
            }
            // The battle party mesh is a set of object-local TMD pieces (head,
            // torso, limbs) - NOT pre-assembled. The retail engine sockets them
            // with the battle ANM (PROT 1203 `other5`): frame 0 of each
            // character's idle record = the combat-stance rest pose, applied
            // R*v + T per object. Load that bundle; its 30 records are per-char
            // banks (Vahn 0-8 / Noa 9-17 / Gala 18-26), idle = each bank's first.
            let battle_anm = self
                .session
                .host
                .index
                .entry_bytes_extended(1203)
                .ok()
                .and_then(|raw| {
                    [6usize, 3, 5, 7].iter().find_map(|&dc| {
                        legaia_asset::player_anm::find_in_entry(&raw, dc)
                            .into_iter()
                            .next()
                    })
                });
            // Per-character: build the assembled mesh, overlay its battle palette
            // onto the CLUT rows the mesh samples, upload, bind to the party actor.
            // char slot 0/1/2 = Vahn/Noa/Gala; palette source PROT 861/864/865.
            for member in 0..party_count.min(3) {
                let cslot = member; // actor slot i -> char slot i (Vahn/Noa/Gala)
                let Some(slot) = pack.slot(cslot) else {
                    continue;
                };
                let Ok(tmd) = legaia_tmd::parse(&slot.tmd_bytes) else {
                    continue;
                };
                // Assemble: frame-0 (T,R) per object from this char's idle record.
                // PROT 1203 banks (per docs/formats/character-mesh.md): records
                // 0-8 = Vahn (15-bone), 9-17 = Noa (16), 18-26 = Gala (15); the
                // FIRST record of each bank is that character's idle rest pose.
                let bone_offsets: Vec<([i16; 3], [i16; 3])> = match &battle_anm {
                    Some(b) => {
                        let rec = [0usize, 9, 18][cslot]; // idle record per bank
                        (0..tmd.objects.len())
                            .map(|o| match b.bone_transform(rec, 0, o) {
                                Some(t) => (
                                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                                ),
                                None => ([0; 3], [0; 3]),
                            })
                            .collect()
                    }
                    None => Vec::new(),
                };
                let vmesh = if bone_offsets.is_empty() {
                    legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &slot.tmd_bytes)
                } else {
                    legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
                        &tmd,
                        &slot.tmd_bytes,
                        &bone_offsets,
                    )
                };
                if vmesh.indices.is_empty() {
                    continue;
                }
                // Rows the mesh CBA samples, and (for collect) the columns.
                let mut rows: Vec<u16> =
                    vmesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
                rows.sort_unstable();
                rows.dedup();
                let mut cols: Vec<u16> = vmesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
                cols.sort_unstable();
                cols.dedup();
                // Decode + overlay the battle palette. Vahn (861) = byte-exact
                // parse_record; Noa (864) / Gala (865) = equipment-robust collect.
                let pal = match cslot {
                    0 => self
                        .session
                        .host
                        .index
                        .entry_bytes_extended(861)
                        .ok()
                        .and_then(|f| {
                            let rec0 = legaia_asset::battle_char_palette::find_record0(&f)?;
                            legaia_asset::battle_char_palette::parse_record(&f, rec0).ok()
                        }),
                    1 | 2 => {
                        let prot = if cslot == 1 { 864 } else { 865 };
                        self.session
                            .host
                            .index
                            .entry_bytes_extended(prot)
                            .ok()
                            .and_then(|f| {
                                legaia_asset::battle_char_palette::collect_palette(&f, 0, &cols)
                                    .ok()
                            })
                    }
                    _ => None,
                };
                if let Some(pal) = pal {
                    // STP-set palette bands onto each row the mesh CBA samples.
                    for &row in &rows {
                        for band in &pal.bands {
                            let bytes: Vec<u8> = band
                                .vram_words()
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect();
                            vram.write_clut_row(band.base, row, &bytes);
                        }
                    }
                }
                match r.upload_vram_mesh(
                    &vmesh.positions,
                    &vmesh.uvs,
                    &vmesh.cba_tsb,
                    &vmesh.normals,
                    &vmesh.indices,
                ) {
                    Ok(m) => {
                        let idx = self.meshes.len();
                        self.meshes.push(m);
                        self.scene_tmd_data.push((tmd, slot.tmd_bytes.clone()));
                        self.session.host.world.actors[member].tmd_binding = Some(idx);
                        party_bound += 1;
                    }
                    Err(e) => log::warn!("play-window: party {cslot} mesh upload: {e:#}"),
                }
            }
        }

        if bound > 0 || party_bound > 0 {
            match r.upload_vram(&vram) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: battle VRAM re-upload: {e:#}"),
            }
            log::info!(
                "play-window: battle render bound {bound} monster + {party_bound} party mesh(es)"
            );
        }
        // Stash the battle VRAM + the monster-slot count so a mid-battle player
        // summon can inject its creature texture into the next free slot.
        self.battle_tex_slots_used = (bound as u8).min(4);
        self.battle_vram = Some(vram);
    }

    /// Spawn the player Seru-magic summon as a battle creature, the faithful
    /// render (the summon reuses its namesake `battle_data` enemy creature -
    /// see `summon::summon_creature_id`). Loads that creature's mesh + texture
    /// (`battle_render_mesh`) into a free battle texture slot and binds it to a
    /// high actor slot with its idle [`MonsterAnimPlayer`], so the existing
    /// battle render animates + textures it exactly like an enemy. Replaces the
    /// move-VM `SummonScene` stand-in for the visual. No-op outside battle.
    fn spawn_summon_creature(&mut self, spell_id: u8) {
        if self.session.host.world.mode != SceneMode::Battle {
            return;
        }
        let Some(archive) = self.monster_archive_bytes() else {
            return;
        };
        let Some(creature) = legaia_engine_core::summon::summon_creature_id(spell_id, &archive)
        else {
            return;
        };
        let Some(mut vram) = self.battle_vram.clone() else {
            return;
        };
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let mesh = match legaia_asset::monster_archive::mesh(&archive, creature) {
            Ok(Some(m)) => m,
            _ => return,
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return;
        };
        // Inject the creature texture into the next free battle slot.
        let tex_slot = self.battle_tex_slots_used.min(4);
        let Some(vmesh) = mesh.battle_render_mesh(tex_slot, &mut vram) else {
            return;
        };
        if vmesh.indices.is_empty() {
            return;
        }
        let uploaded = match r.upload_vram_mesh(
            &vmesh.positions,
            &vmesh.uvs,
            &vmesh.cba_tsb,
            &vmesh.normals,
            &vmesh.indices,
        ) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("play-window: summon mesh upload: {e:#}");
                return;
            }
        };
        let idx = self.meshes.len();
        self.meshes.push(uploaded);
        self.scene_tmd_data.push((tmd, mesh.tmd_bytes().to_vec()));
        match r.upload_vram(&vram) {
            Ok(v) => self.uploaded_vram = Some(v),
            Err(e) => log::error!("play-window: summon VRAM re-upload: {e:#}"),
        }

        // Seat the summon in a free high actor slot (>= 8) so it never collides
        // with the party/monster battle slots. Place it on the party side
        // (`enter_battle` seats party at `x = -600`, enemies at `x = +600`), in
        // front of the party and clearly clear of the enemy cluster, so the
        // battle camera frames it distinct from the enemies it attacks.
        let slot = self
            .summon_actor_slot
            .unwrap_or_else(|| 8 + (self.session.host.world.party_count as usize));
        self.summon_actor_slot = Some(slot);
        if let Some(a) = self.session.host.world.actors.get_mut(slot) {
            a.active = true;
            a.tmd_binding = Some(idx);
            a.battle_tex_slot = Some(tex_slot);
            a.move_state.world_x = -350;
            a.move_state.world_y = 0;
            a.move_state.world_z = 0;
        }
        if let Ok(Some(idle)) = legaia_asset::monster_archive::idle_animation(&archive, creature)
            && let Some(player) = legaia_engine_core::battle_anim::MonsterAnimPlayer::new(&idle)
        {
            self.session
                .host
                .world
                .set_actor_battle_animation(slot, player);
        }
        log::info!(
            "play-window: summon spell {spell_id:#04x} -> battle_data creature {creature} \
             (mesh slot {idx}, tex slot {tex_slot}, actor slot {slot})"
        );
    }

    /// Leave battle: restore the clean field VRAM and drop the appended
    /// battle monster meshes (the field actor table was already restored from
    /// the pre-battle snapshot, so those slots no longer reference them).
    fn exit_battle_render(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: field VRAM restore: {e:#}"),
            }
        }
        let keep = self.battle_mesh_base.min(self.meshes.len());
        self.meshes.truncate(keep);
        self.scene_tmd_data
            .truncate(keep.min(self.scene_tmd_data.len()));
        // Tear down a spawned player-summon creature.
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(a) = self.session.host.world.actors.get_mut(slot)
        {
            a.active = false;
            a.tmd_binding = None;
            a.battle_tex_slot = None;
            a.battle_animation = None;
            a.pose_frame = None;
        }
        self.battle_vram = None;
        self.battle_tex_slots_used = 0;
        self.battle_stage_mesh = None;
        self.battle_ground_mesh = None;
    }

    /// Build the per-strip [`legaia_engine_render::SpriteDraw`] list for
    /// the active publisher logo.
    ///
    /// PROKION and SCEA are stored as vertically-packed sprite atlases
    /// (see [`legaia_engine_core::publisher_logos::STRIPS_PER_LOGO`]);
    /// retail unfolds them by drawing the `N` strips side-by-side. We
    /// compute one [`SpriteDraw`] per strip, all sharing the session's
    /// current alpha, then integer-scale + centre the unfolded layout.
    /// Returns an empty vec when boot-UI isn't `PublisherLogos` or the
    /// atlas wasn't uploaded.
    fn publisher_logo_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let BootUiState::PublisherLogos(session) = &self.boot_ui else {
            return Vec::new();
        };
        let Some(assets) = self.publisher_logos.as_ref() else {
            return Vec::new();
        };
        let idx = session.current_logo();
        if idx >= legaia_engine_core::publisher_logos::LOGO_COUNT {
            return Vec::new();
        }
        let (sx, sy, sw, sh) = assets.rects[idx];
        if sw == 0 || sh == 0 {
            return Vec::new();
        }
        let (cols, rows) = legaia_engine_core::publisher_logos::STRIP_GRID[idx];
        let cols = cols.max(1);
        let rows = rows.max(1);
        let strips_total = cols * rows;
        let strip_h_src = sh / strips_total;
        if strip_h_src == 0 {
            return Vec::new();
        }
        let unfolded_w = sw * cols;
        let unfolded_h = strip_h_src * rows;
        // Integer-multiple up-scale that fits inside the surface, capped
        // at 4× to keep logos crisp at typical 960×720. `max(1)` falls
        // back to native size (and accepts clipping) for layouts wider
        // than the surface.
        let scale_w = surface_w / unfolded_w.max(1);
        let scale_h = surface_h / unfolded_h.max(1);
        let scale = scale_w.min(scale_h).clamp(1, 4);
        let strip_w_dst = sw * scale;
        let strip_h_dst = strip_h_src * scale;
        let dst_w_total = unfolded_w * scale;
        let dst_h_total = unfolded_h * scale;
        let dst_x0 = (surface_w as i32 - dst_w_total as i32) / 2;
        let dst_y0 = (surface_h as i32 - dst_h_total as i32) / 2;
        let alpha = session.alpha().clamp(0.0, 1.0);
        let color = [1.0, 1.0, 1.0, alpha];
        // Source strips are stored column-major: source strip index
        // `s = c * rows + r` lands at output (col c, row r).
        let mut out = Vec::with_capacity(strips_total as usize);
        for r in 0..rows {
            for c in 0..cols {
                let s = c * rows + r;
                let src_y = sy + s * strip_h_src;
                let dst_x = dst_x0 + (c * strip_w_dst) as i32;
                let dst_y = dst_y0 + (r * strip_h_dst) as i32;
                out.push(legaia_engine_render::SpriteDraw {
                    dst: (dst_x, dst_y, strip_w_dst, strip_h_dst),
                    src: (sx, src_y, sw, strip_h_src),
                    color,
                });
            }
        }
        out
    }

    /// Canonical PSX-framebuffer (320×240) stage origin + scale, shared
    /// by every boot-UI element (title art, save-select chrome, slot
    /// pills, cursor, menu glyphs). Every retail-pinned position is
    /// expressed in 320×240 framebuffer pixels, so this is the single
    /// stage transform that maps them to screen coords. Using the same
    /// stage for the title art AND the save-select panel ensures
    /// relative positions remain correct at any window resolution.
    fn save_select_stage(&self, surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
        let stage_w = legaia_engine_render::BOOT_UI_STAGE_W;
        let stage_h = legaia_engine_render::BOOT_UI_STAGE_H;
        let scale = (surface_w / stage_w).min(surface_h / stage_h).clamp(1, 4);
        let sw = stage_w * scale;
        let sh = stage_h * scale;
        let x0 = (surface_w as i32 - sw as i32) / 2;
        let y0 = (surface_h as i32 - sh as i32) / 2;
        ((x0, y0), scale)
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// retail save-screen chrome (panel frame + slot pills). Anchored
    /// at the same 256×256 stage origin the title atlas uses so the
    /// chrome overlays the title art at retail-equivalent positions.
    ///
    /// Returns an empty vec when the save-menu atlas wasn't uploaded
    /// (e.g. running without a disc) or when the boot UI isn't in a
    /// SaveSelect / field-Save sub-state.
    fn save_select_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        use legaia_engine_core::save_select::{SaveSelectSession, SelectPhase};
        // The save-select session (or field-menu Save sub-session) that
        // drives both pill chrome and any retail Load-mode overlays.
        let session: &SaveSelectSession = match &self.boot_ui {
            BootUiState::SaveSelect(s) => s,
            BootUiState::FieldMenu {
                sub: Some(active), ..
            } => {
                use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
                if let FieldMenuSubsession::Save(s) = active {
                    s
                } else {
                    return Vec::new();
                }
            }
            _ => return Vec::new(),
        };
        let slot_count = session.slots().len().min(2);
        let cursor_row = (session.current_slot() as usize).min(1);
        // Retail draws every visible slot pill during Browsing and the
        // Confirm prompts, but hides the non-selected pills once a
        // slot has been confirmed for load (NowChecking + SlotPreview
        // both show only the picked pill). Build the pill slice
        // accordingly so the sprite chrome matches retail. AND retail
        // relocates that single visible pill up under the Load panel
        // (SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE) during Load-active.
        // The relocation is animated — mode 2 of FUN_801E1C1C slides
        // the slot composite linearly from screen `(136, 96)` (=
        // param_3=0xa0 with `sVar6 -= 0x18` x-shift, param_4=0x60) to
        // `(24, 40)` over 16 frames, driven by `DAT_801ef194`. We
        // interpolate against `session.slide_anim_t()` so the engine
        // matches retail's slide-in.
        let (pills, pill_anchor): (Vec<u8>, (i32, i32)) = match session.phase() {
            SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => {
                // Slide start: retail mode-2 start `(160, 96)` minus
                // the `-0x18` x-shift baked into the inline emit
                // before the GPU command -> top-left `(136, 96)`.
                const SLIDE_START_TOPLEFT: (i32, i32) = (136, 96);
                let pos = session.interpolate(
                    SLIDE_START_TOPLEFT,
                    legaia_engine_render::SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
                );
                (vec![slot], pos)
            }
            _ => (
                (0..slot_count as u8).collect(),
                legaia_engine_render::SAVE_SELECT_SLOT1_POS,
            ),
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut draws = legaia_engine_render::save_select_chrome_draws_for(
            &assets.rects,
            &pills,
            pill_anchor,
            stage_origin,
            stage_scale,
        );
        // Pointing-finger cursor sprite — retail's small white hand
        // pointing at the selected slot pill, byte-pinned to CLUT row
        // 7 of the system-UI TIM. Emit last so it draws on top of
        // the pills. Suppress during NowChecking (dialog covers the
        // pill row) and SlotPreview (the grid emits its own cursor
        // on the focused cell).
        let emit_pill_cursor = !matches!(
            session.phase(),
            SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
        );
        if slot_count > 0 && emit_pill_cursor {
            draws.push(legaia_engine_render::save_select_cursor_draw_for(
                &assets.rects,
                cursor_row,
                stage_origin,
                stage_scale,
            ));
        }
        // Phase-specific overlays: SlotPreview shows the 5×3 grid + a
        // bottom info panel; NowChecking shows a centered dialog box
        // with the "Now checking. Do not remove MEMORY CARD" message.
        match session.phase() {
            SelectPhase::SlotPreview { slot } => {
                // Build per-cell views from the session's slot
                // snapshots. Each cell maps to one memory-card block;
                // up to 15 cells (5×3 grid).
                let cells: Vec<legaia_engine_render::SlotGridCell> = (0..15)
                    .map(|i| {
                        session
                            .slots()
                            .get(i)
                            .map(|s| legaia_engine_render::SlotGridCell {
                                present: s.present,
                                portrait_char_id: if s.present {
                                    Some(slot_leader_char_id(s))
                                } else {
                                    None
                                },
                            })
                            .unwrap_or_default()
                    })
                    .collect();
                draws.extend(legaia_engine_render::slot_preview_grid_draws_for(
                    &assets.rects,
                    &cells,
                    slot,
                    stage_origin,
                    stage_scale,
                ));
                let info = build_slot_info_view(session.slots(), slot);
                let view = info.as_ref().map(|i| i.as_view());
                let panel_y_offset = info_panel_slide_offset(session);
                draws.extend(legaia_engine_render::slot_info_panel_draws_for(
                    &assets.rects,
                    view.as_ref(),
                    panel_y_offset,
                    stage_origin,
                    stage_scale,
                ));
            }
            SelectPhase::NowChecking { .. } => {
                // Slide the panel left-from-right alongside the text,
                // matching retail mode-0's `pos = (416, 112) -> (160,
                // 112)` interpolation.
                let pos_x = legaia_engine_core::save_select::interpolate_anim(
                    (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                    (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                    session.slide_anim_t(),
                )
                .0;
                let slide_offset = (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
                draws.extend(legaia_engine_render::now_checking_panel_draws_for(
                    &assets.rects,
                    stage_origin,
                    stage_scale,
                    slide_offset,
                ));
            }
            _ => {}
        }
        draws
    }

    /// Build the [`legaia_engine_render::SpriteDraw`] list for the
    /// title-screen quad. Composes the retail title screen by drawing
    /// per-band sub-rects of the PROT 0888 title TIM: orb + wordmark
    /// always, "PRESS START BUTTON" only during the PressStart phase,
    /// and the two copyright lines in every post-fade phase. The
    /// `<DEMO>` band and the small "NEW GAME CONTINUE" footer band are
    /// intentionally skipped - the former is a demo-build leftover
    /// retail never draws, the latter is replaced by larger
    /// font-rendered menu labels (see [`Self::boot_ui_draws`]).
    ///
    /// Each band is positioned at its source `y` within a centred,
    /// integer-scaled 256×256 stage. Returns an empty vec when
    /// boot-UI isn't `Title`, the atlas wasn't uploaded, or the title
    /// session has reached [`legaia_engine_core::title::TitlePhase::Done`].
    fn title_screen_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        // Active during both the Title phases and the SaveSelect boot
        // sub-state. SaveSelect dims the bands to ~45 % brightness so
        // the panel + slot pills layered on top read clearly. Retail
        // pivots to pure black once a slot is confirmed (NowChecking /
        // SlotPreview): the dialog + portrait grid + info panel are
        // composed against black, never the title art.
        let title_session: Option<&legaia_engine_core::title::TitleSession> = match &self.boot_ui {
            BootUiState::Title(s) => Some(s),
            BootUiState::SaveSelect(s) => {
                use legaia_engine_core::save_select::SelectPhase;
                if matches!(
                    s.phase(),
                    SelectPhase::NowChecking { .. } | SelectPhase::SlotPreview { .. }
                ) {
                    return Vec::new();
                }
                None
            }
            _ => return Vec::new(),
        };
        let (alpha, dim) = if let Some(session) = title_session {
            if matches!(
                session.phase(),
                legaia_engine_core::title::TitlePhase::Done(_)
            ) {
                return Vec::new();
            }
            let alpha = match session.phase() {
                legaia_engine_core::title::TitlePhase::FadeIn { frames_remaining } => {
                    let total = session.fade_in_frames.max(1) as f32;
                    1.0 - (frames_remaining as f32 / total).clamp(0.0, 1.0)
                }
                _ => 1.0,
            };
            (alpha, false)
        } else {
            (1.0, true)
        };
        let Some(assets) = self.title_screen.as_ref() else {
            return Vec::new();
        };
        let (_atlas_x, _atlas_y, atlas_w, atlas_h) = assets.rect;
        if atlas_w == 0 || atlas_h == 0 {
            return Vec::new();
        }
        // Share the canonical PSX framebuffer (320×240) stage with
        // every other boot-UI element so the title art aligns with
        // the save-select panel, slot pills, and cursor — all of
        // which use retail-pinned framebuffer coords. The title TIM's
        // bands are sampled at their natural src (sx, sy) but drawn
        // at dst (TITLE_ART_POS + sx, TITLE_ART_POS + sy), i.e.
        // offset by retail's title-quad top-left placement.
        let ((stage_x0, stage_y0), scale) = self.save_select_stage(surface_w, surface_h);
        let lum = if dim { 0.45 } else { 1.0 };
        let color = [lum, lum, lum, alpha];
        let emit_press_start = matches!(
            &self.boot_ui,
            BootUiState::Title(s)
                if matches!(s.phase(), legaia_engine_core::title::TitlePhase::PressStart { .. })
        );
        use legaia_asset::title_pak;
        // Each entry: (src_rect, dst_x_src, dst_y_src, tint). Most
        // bands draw at their own (src_x, src_y); the menu rows are
        // sampled from a packed single-row band and re-positioned so
        // "NEW GAME" sits at src_y=143 and "CONTINUE" at src_y=159
        // (matching the retail stacked layout, which puts these
        // ~14 px apart between the wordmark and the copyright lines).
        let scale_i32 = scale as i32;
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        // `dst_src_x/y` are coords inside the title TIM's source rect
        // (0..256, 0..256). We offset by TITLE_ART_POS so the result
        // lands at retail's framebuffer position.
        let title_pos_x = legaia_engine_render::TITLE_ART_POS.0;
        let title_pos_y = legaia_engine_render::TITLE_ART_POS.1;
        let push_band = |out: &mut Vec<legaia_engine_render::SpriteDraw>,
                         src: (u32, u32, u32, u32),
                         dst_src_x: i32,
                         dst_src_y: i32,
                         tint: [f32; 4]| {
            let (sx, sy, sw, sh) = src;
            out.push(legaia_engine_render::SpriteDraw {
                dst: (
                    stage_x0 + (title_pos_x + dst_src_x) * scale_i32,
                    stage_y0 + (title_pos_y + dst_src_y) * scale_i32,
                    sw * scale,
                    sh * scale,
                ),
                src: (sx, sy, sw, sh),
                color: tint,
            });
        };

        // Wordmark always.
        let wm = title_pak::TITLE_BAND_WORDMARK;
        push_band(&mut out, wm, wm.0 as i32, wm.1 as i32, color);

        // PressStart prompt only during that phase.
        if emit_press_start {
            let ps = title_pak::TITLE_BAND_PRESS_START;
            push_band(&mut out, ps, ps.0 as i32, ps.1 as i32, color);
        }

        // Main-menu rows (NEW GAME / CONTINUE) — drawn during MainMenu
        // (selected row bright, unselected dim) and also during
        // SaveSelect (both dim — they sit in the background behind the
        // slot pills and don't reflect a live cursor).
        let menu_state: Option<(u8, bool)> = match &self.boot_ui {
            BootUiState::Title(s) => match s.phase() {
                legaia_engine_core::title::TitlePhase::MainMenu { cursor } => Some((cursor, true)),
                _ => None,
            },
            BootUiState::SaveSelect(_) => Some((1, false)),
            _ => None,
        };
        if let Some((cursor, has_focus)) = menu_state {
            let row_white = color;
            let row_dim = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            let ng = title_pak::TITLE_BAND_MENU_NEW_GAME;
            let co = title_pak::TITLE_BAND_MENU_CONTINUE;
            // Center inside the title-art width so the rows sit on
            // the screen's horizontal center (fb_x=160) after the
            // TITLE_ART_POS.x=33 offset is applied by push_band.
            let title_art_w = legaia_engine_render::TITLE_ART_SIZE.0 as u32;
            let ng_x = ((title_art_w - ng.2) / 2) as i32;
            let co_x = ((title_art_w - co.2) / 2) as i32;
            // Sit the menu between wordmark (ends y~141) and copyrights (start y~195).
            let ng_y: i32 = 154;
            let co_y: i32 = ng_y + ng.3 as i32 + 4;
            let ng_tint = if has_focus && cursor == 0 {
                row_white
            } else {
                row_dim
            };
            let co_tint = if has_focus && cursor == 1 {
                row_white
            } else {
                row_dim
            };
            push_band(&mut out, ng, ng_x, ng_y, ng_tint);
            push_band(&mut out, co, co_x, co_y, co_tint);
        }

        // Copyright lines always (post-fade).
        let tm = title_pak::TITLE_BAND_TM_COPYRIGHT;
        push_band(&mut out, tm, tm.0 as i32, tm.1 as i32, color);
        let cc = title_pak::TITLE_BAND_C_COPYRIGHT;
        push_band(&mut out, cc, cc.0 as i32, cc.1 as i32, color);
        out
    }

    /// **Deprecated path** kept as a no-disc fallback. The retail title
    /// menu now renders via `title_screen_sprite_draws` sampling the
    /// dedicated NEW GAME / CONTINUE sub-rects from the title TIM
    /// (PROT 0888 @ y=227..237). When the title atlas is present this
    /// method returns an empty vec so the title-TIM path is the
    /// single source of menu glyphs.
    ///
    /// Returns an empty vec when:
    /// - boot UI isn't [`BootUiState::Title`], or
    /// - the title session has already reached
    ///   [`legaia_engine_core::title::TitlePhase::Done`], or
    /// - the title-screen atlas IS uploaded (retail-faithful path
    ///   covers the menu rows itself), or
    /// - the menu-glyph atlas wasn't uploaded, or
    /// - the title phase isn't `MainMenu`.
    fn title_menu_glyph_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let BootUiState::Title(session) = &self.boot_ui else {
            return Vec::new();
        };
        if self.menu_glyphs.is_none() {
            return Vec::new();
        }
        // When the title-screen atlas is loaded, the retail-faithful
        // path inside `title_screen_sprite_draws` already emits the
        // NEW GAME / CONTINUE rows from the title TIM itself — skip
        // the debug-atlas fallback to avoid double-rendering.
        if self.title_screen.is_some() {
            return Vec::new();
        }
        use legaia_engine_core::title::TitlePhase;
        let (phase_id, cursor) = match session.phase() {
            TitlePhase::MainMenu { cursor } => (2u8, cursor),
            _ => return Vec::new(),
        };
        // Anchor inside the same centred + integer-scaled 256×256
        // title stage that `title_screen_sprite_draws` uses. The menu
        // rows sit between the wordmark band (ends at src y=140) and
        // the copyright bands (start at src y=195) — the menu-glyph
        // cell is 14 px tall at 1× and we render at 2× the title-art
        // scale for retail-faithful sizing (~28 px atlas-pixels per
        // row, two rows + gutter = ~60 px in source).
        let atlas_w: u32 = 256;
        let atlas_h: u32 = 256;
        let title_scale = (surface_w / atlas_w.max(1))
            .min(surface_h / atlas_h.max(1))
            .clamp(1, 4);
        let title_scale_i32 = title_scale as i32;
        let stage_x0 = (surface_w as i32 - (atlas_w as i32) * title_scale_i32) / 2;
        let stage_y0 = (surface_h as i32 - (atlas_h as i32) * title_scale_i32) / 2;
        // Render menu glyphs at 2× the title-art scale so the letters
        // match the retail proportion (~28 px tall in framebuffer
        // pixels at 1×). "NEW GAME" is 8 cells × 8 px × 2 = 128 px at
        // 1× glyph_scale, then × title_scale for the on-screen size.
        let glyph_scale = title_scale;
        let menu_w_src = 8 * 8; // 8 chars × 8 px (1× glyph multiplier)
        // Centre horizontally inside the 256-wide title stage.
        let pen_src_x = (atlas_w as i32 - menu_w_src) / 2;
        let pen_src_y = 152;
        let pen = (
            stage_x0 + pen_src_x * title_scale_i32,
            stage_y0 + pen_src_y * title_scale_i32,
        );
        legaia_engine_render::title_menu_draws_for(
            phase_id,
            cursor,
            session.continue_enabled,
            pen,
            glyph_scale,
        )
    }

    /// Keep the rendered dialog panel ([`Self::active_dialog`]) in sync with
    /// the world's pending dialog request.
    ///
    /// The world owns dismissal: the field VM's op-`0x4C` dialog-advance hook
    /// and the overworld talk-to handler both clear `World::current_dialog` on
    /// a confirm/cancel press. This method only mirrors that state into a
    /// visible, typed-out box - it opens a panel from the scene's MES the frame
    /// a request appears, ticks its typewriter reveal, and drops the panel the
    /// frame the world clears the request. It never clears `current_dialog`
    /// itself, so it can't race the world's dismiss.
    fn sync_dialog_panel(&mut self) {
        // When the inline-script field-VM runner owns dialogue, it manages its
        // own box (rendered from `world.inline_dialogue`); don't also open the
        // simplified panel.
        if self.session.host.world.use_vm_dialogue {
            self.active_dialog = None;
            return;
        }
        if self.session.host.world.current_dialog.is_none() {
            self.active_dialog = None;
            return;
        }
        if self.active_dialog.is_none()
            && let Some(mut panel) = self.session.host.open_pending_dialog()
        {
            panel.set_glyphs_per_frame(2);
            self.active_dialog = Some(panel);
        }
        if let Some(panel) = self.active_dialog.as_mut() {
            panel.tick();
        }
    }

    fn build_hud(&self, w: u32, h: u32) -> Vec<TextDraw> {
        let Some(atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let _ = atlas;
        // Boot UI is fullscreen - when active, suppress every other HUD layer
        // and just render the active panel (title screen / save-select).
        if self.boot_ui.is_active() {
            return self.boot_ui_draws(w, h);
        }
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        let scene_name = self
            .session
            .host
            .scene
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("(none)");
        let line1 = format!(
            "scene {}  frame {}  meshes {}",
            scene_name,
            self.session.host.world.frame,
            self.meshes.len()
        );
        let layout1 = self.font.layout_ascii(&line1);
        let mut out = text_draws_for(&layout1, (8, 8), white);
        let audio_str = if self.session.audio.is_some() {
            "audio on"
        } else {
            "no audio"
        };
        let line2 = format!(
            "t {:.1}s  {}  arrows=dpad Z=X",
            self.win.elapsed_secs(),
            audio_str
        );
        let layout2 = self.font.layout_ascii(&line2);
        out.extend(text_draws_for(&layout2, (8, 26), dim));
        if let Some(ctrl) = &self.session.host.world.world_map_ctrl {
            let mode_str = if ctrl.is_top_view() {
                "top-view"
            } else {
                "walk"
            };
            let line3 = format!(
                "world-map {} | cam ({},{}) az {} zoom {}",
                mode_str, ctrl.camera_x, ctrl.camera_z, ctrl.azimuth, ctrl.zoom
            );
            let layout3 = self.font.layout_ascii(&line3);
            out.extend(text_draws_for(&layout3, (8, 44), white));
        }
        // Shop / inn overlay: rendered at the bottom of the screen when the menu
        // runtime is in any shop, inn, or confirmation state.
        if self.menu_runtime.is_open() {
            let label = self.menu_runtime.current_label();
            if let Some(shop) = &self.menu_runtime.shop_session {
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let gold = self.session.host.world.money;
                let (title, rows, show_gold) = match state {
                    Some(MenuState::ShopBuy) => {
                        let rows: Vec<ShopRow<'_>> = shop
                            .inventory
                            .items
                            .iter()
                            .map(|item| ShopRow {
                                label: "Item",
                                price: Some(item.price),
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopSell) => {
                        let inv_items = MenuRuntime::inventory_items(&self.session.host.world);
                        let rows: Vec<ShopRow<'_>> = inv_items
                            .iter()
                            .map(|(_id, _qty)| ShopRow {
                                label: "Item",
                                price: None,
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopQuantity) => {
                        let rows: Vec<ShopRow<'_>> = (1u32..=9)
                            .map(|_| ShopRow {
                                label: "qty",
                                price: None,
                            })
                            .collect();
                        (label, rows, None)
                    }
                    Some(MenuState::ShopConfirm) => {
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
                        (label, rows, Some(gold))
                    }
                    _ => (label, Vec::new(), None),
                };
                if !rows.is_empty() {
                    let shop_draws =
                        shop_draws_for(&self.font, title, &rows, cursor, show_gold, (8, 140));
                    out.extend(shop_draws);
                }
            } else if self.menu_runtime.inn_session.is_some() {
                // Inn overlay: cost prompt with Yes / No cursor.
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let cost = self
                    .menu_runtime
                    .inn_session
                    .as_ref()
                    .map(|s| s.cost)
                    .unwrap_or(0);
                let gold = self.session.host.world.money;
                match state {
                    Some(MenuState::InnConfirm) => {
                        let title = format!("INN  Rest for {}G?", cost);
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
                        let inn_draws =
                            shop_draws_for(&self.font, &title, &rows, cursor, Some(gold), (8, 140));
                        out.extend(inn_draws);
                    }
                    Some(MenuState::InnSleep) => {
                        let layout = self.font.layout_ascii("Resting...");
                        out.extend(text_draws_for(&layout, (8, 140), white));
                    }
                    _ => {
                        let menu_label = format!("[{}]", label);
                        let ml_layout = self.font.layout_ascii(&menu_label);
                        out.extend(text_draws_for(&ml_layout, (8, 140), white));
                    }
                }
            } else {
                // Non-shop, non-inn menu: show current mode label.
                let menu_label = format!("[{}]", label);
                let ml_layout = self.font.layout_ascii(&menu_label);
                out.extend(text_draws_for(&ml_layout, (8, 140), white));
            }
        }
        // Battle-event log: rendered along the right edge when non-empty.
        // Most recent at the bottom of the column.
        if !self.battle_event_log.is_empty() {
            let log_color = [1.0f32, 0.95, 0.7, 1.0];
            let line_height = 14;
            let bottom_y = 280;
            let n = self.battle_event_log.len();
            for (i, line) in self.battle_event_log.iter().enumerate() {
                let layout = self.font.layout_ascii(line);
                let y = bottom_y - ((n - 1 - i) as i32) * line_height;
                out.extend(text_draws_for(&layout, (220, y), log_color));
            }
        }
        // Battle HUD: party + monster HP plus, when the battle is
        // player-driven, the live command menu / target cursor. Only drawn in
        // SceneMode::Battle; harmless when the live loop is off (it just never
        // enters battle).
        if self.session.host.world.mode == SceneMode::Battle {
            use legaia_engine_core::battle_input::{BattleCommand, CommandPhase};
            use legaia_engine_core::target_picker::{CursorRow, PickerState};
            let bw = &self.session.host.world;
            let pc = (bw.party_count.clamp(1, 3) as usize).min(bw.actors.len());
            let down_color = [0.6f32, 0.6, 0.6, 1.0];
            let enemy_color = [1.0f32, 0.7, 0.6, 1.0];

            // Per-actor-index row Y, recorded as rows are drawn so popups +
            // status icons anchor to the right slot even though the monster
            // loop skips empty slots.
            let mut row_y: [Option<i32>; 8] = [None; 8];
            let mut y = 60i32;
            for (i, a) in bw.actors.iter().take(pc).enumerate() {
                let line = format!("P{}  HP {:>4}/{:<4}", i + 1, a.battle.hp, a.battle.max_hp);
                let color = if a.battle.liveness != 0 {
                    white
                } else {
                    down_color
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                if i < row_y.len() {
                    row_y[i] = Some(y);
                }
                y += 16;
            }
            y += 8;
            for (mi, a) in bw.actors.iter().skip(pc).enumerate() {
                if a.battle.max_hp == 0 {
                    continue;
                }
                let alive = a.battle.liveness != 0;
                let line = format!(
                    "M{}  HP {:>4}/{:<4}{}",
                    mi + 1,
                    a.battle.hp,
                    a.battle.max_hp,
                    if alive { "" } else { "  DOWN" }
                );
                let color = if alive { enemy_color } else { down_color };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                let actor_idx = pc + mi;
                if actor_idx < row_y.len() {
                    row_y[actor_idx] = Some(y);
                }
                y += 16;
            }

            // Status-effect icon strip per slot (single-letter abbreviations
            // from the live tracker), drawn to the right of the HP row.
            let status_color = [1.0f32, 0.95, 0.4, 1.0];
            for (slot, anchor) in row_y.iter().enumerate() {
                let Some(ry) = anchor else { continue };
                let letters = self.battle_hud.slots[slot].status_letters();
                for (k, letter) in letters.iter().enumerate() {
                    let s = (*letter as char).to_string();
                    out.extend(text_draws_for(
                        &self.font.layout_ascii(&s),
                        (170 + k as i32 * 8, *ry),
                        status_color,
                    ));
                }
            }

            // Floating damage / heal numbers, anchored just above each slot's
            // HP row and fading with the popup's remaining lifetime.
            let dmg_color = [0.5f32, 0.85, 1.0, 1.0];
            let heal_color = [0.5f32, 1.0, 0.5, 1.0];
            let crit_color = [1.0f32, 0.95, 0.4, 1.0];
            for p in self.battle_hud.popup_views() {
                let Some(Some(ry)) = row_y.get(p.slot as usize) else {
                    continue;
                };
                let base = if p.is_heal {
                    heal_color
                } else if p.is_crit {
                    crit_color
                } else {
                    dmg_color
                };
                let color = [base[0], base[1], base[2], base[3] * p.alpha.clamp(0.0, 1.0)];
                let text = if let Some(letter) = p.status_letter {
                    format!("[{}]", letter as char)
                } else if p.is_heal {
                    format!("+{}", p.amount)
                } else {
                    format!("-{}", p.amount)
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&text),
                    (120, *ry - 14),
                    color,
                ));
            }

            // Player-driven submenus (opened from the Arts / Magic / Item
            // commands). Each parks both the SM and the command session while
            // open, so it takes priority over the command menu.
            if let Some(arts) = &bw.battle_arts_menu {
                use legaia_engine_core::battle_arts::ArtsPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &arts.phase {
                    ArtsPhase::Select { cursor } => {
                        let header = format!("P{} - arts:", arts.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if arts.arts.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no saved arts)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in arts.arts.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = match (row.miracle, row.super_art) {
                                (Some(name), _) => {
                                    format!("{} {} x{} *{}*", marker, row.name, row.hits(), name)
                                }
                                (None, Some(name)) => {
                                    format!("{} {} x{} <{}>", marker, row.name, row.hits(), name)
                                }
                                (None, None) => format!("{} {} x{}", marker, row.name, row.hits()),
                            };
                            let color = if sel { white } else { dim };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    ArtsPhase::Targeting { picker, .. } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("art -> target M{}", slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("art -> target P{}", slot + 1),
                            _ => "art -> select target".to_string(),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if let Some(spell) = &bw.battle_spell_menu {
                use legaia_engine_core::battle_magic::SpellPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &spell.phase {
                    SpellPhase::Select { cursor } => {
                        let header = format!("P{} - magic:", spell.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if spell.spells.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no spells)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in spell.spells.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = format!("{} {} {:>2}MP", marker, row.name, row.mp_cost);
                            let color = if !row.affordable {
                                down_color
                            } else if sel {
                                white
                            } else {
                                dim
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    SpellPhase::Targeting { picker, .. } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("cast -> target M{}", slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("cast -> target P{}", slot + 1),
                            _ => "cast -> select target".to_string(),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if let Some(menu) = &bw.battle_item_menu {
                out.extend(self.items_session_draws(menu));
            } else if let Some(cmd) = &bw.battle_command {
                let menu_x = 8i32;
                let mut my = 210i32;
                match &cmd.phase {
                    CommandPhase::Menu { .. } => {
                        let header = format!("P{} - command:", cmd.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        let cur = cmd.menu_command();
                        for c in BattleCommand::MENU {
                            let marker = if Some(c) == cur { ">" } else { " " };
                            let line = if c.enabled() {
                                format!("{} {}", marker, c.label())
                            } else {
                                format!("{} {} --", marker, c.label())
                            };
                            let color = if Some(c) == cur {
                                white
                            } else if c.enabled() {
                                dim
                            } else {
                                down_color
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    CommandPhase::Targeting { command, picker } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("{} -> target M{}", command.label(), slot + 1),
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("{} -> target P{}", command.label(), slot + 1),
                            _ => format!("{} -> select target", command.label()),
                        };
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&line),
                            (menu_x, my),
                            white,
                        ));
                        my += 14;
                        let hint = "Left/Right=move  Cross=confirm  Circle=back";
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(hint),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            }
        }
        // Level-up banner: rendered near the top when active after a battle win.
        if let Some(banner) = &self.session.host.world.current_level_up_banner {
            let draws = level_up_draws_for(
                &self.font,
                banner.char_id,
                banner.new_level,
                banner.hp_gained,
                banner.mp_gained,
                (8, 60),
            );
            out.extend(draws);
        }
        // Seru-capture banner: shown after a battle in which a Seru was
        // captured (and, if a threshold was crossed, a spell learned).
        if let Some(banner) = &self.session.host.world.current_capture_banner
            && let Some(text) = banner.current_banner()
        {
            out.extend(capture_banner_draws_for(&self.font, &text, (8, 40)));
        }
        // Opening-cutscene narration: the `opdeene` prologue subtitle pages,
        // centered near the bottom of the screen one page at a time.
        if let Some(narration) = &self.session.host.world.cutscene_narration
            && let Some(text) = narration.current_text()
        {
            let gold = [1.0f32, 0.92, 0.6, 1.0];
            let center_x = (w / 2) as i32;
            // Retail draws the subtitle at Y=180 on a 240px-tall virtual
            // screen (FUN_8003C764) - 3/4 down; scale to the real surface.
            let top_y = (h as i32 * 3 / 4).min(h as i32 - 16).max(0);
            out.extend(legaia_engine_render::cutscene_narration_draws_for(
                &self.font, text, center_x, top_y, gold,
            ));
        }
        // Name-entry overlay: the opening `town01` lead-character naming prompt.
        if let Some(entry) = &self.session.host.world.name_entry {
            use legaia_engine_core::name_entry::{CHAR_CELLS, GRID, GRID_COLS};
            let (grid_cursor, control_cursor) = if entry.cursor < CHAR_CELLS {
                (
                    Some((entry.cursor / GRID_COLS, entry.cursor % GRID_COLS)),
                    None,
                )
            } else {
                // Map the control-row column to a button index (Back=0/Space=1/End=2)
                // via the cell's resolved action.
                use legaia_engine_core::name_entry::Control;
                let ctrl = entry.control_at(entry.cursor);
                let idx = match ctrl {
                    Some(Control::Backspace) => Some(0),
                    Some(Control::Space) => Some(1),
                    Some(Control::End) => Some(2),
                    _ => None,
                };
                (None, idx)
            };
            let view = legaia_engine_render::NameEntryView {
                grid_rows: &GRID,
                control_labels: &["Back", "Space", "End"],
                name: &entry.name,
                grid_cursor,
                control_cursor,
                confirming: entry.state == legaia_engine_core::name_entry::NameEntryState::Confirm,
                confirm_yes: entry.confirm_yes,
                caret_on: (self.session.host.world.frame / 16).is_multiple_of(2),
            };
            out.extend(legaia_engine_render::name_entry_draws_for(
                &self.font,
                &view,
                (32, 24),
            ));
        }
        // Dialog box: the active NPC / event message, typed out one line near
        // the bottom of the screen (1/8 from the left, ~70% down - the
        // single-line layout the retail field VM emits). The panel mirrors
        // `World::current_dialog`; the world owns dismissal.
        if let Some(panel) = self.active_dialog.as_ref() {
            let to_ascii = |bytes: &[u8]| -> String {
                bytes
                    .iter()
                    .map(|&b| {
                        if (0x20..=0x7E).contains(&b) {
                            b as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            };
            let page = to_ascii(&panel.page_bytes());
            let layout = self.font.layout_ascii(&page);
            let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
            out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));

            // Multiple-choice menu: draw the decoded option labels under the
            // prompt, one row each, with a `>` cursor on the highlighted option
            // (the picker decoded from the inline interaction script).
            if panel.menu_active()
                && let Some(picker) = panel.picker()
            {
                // The proportional dialog font is a ~14px cell; one row per
                // option below the prompt.
                let line_h = 16i32;
                let cursor = panel.picker_cursor();
                for (i, opt) in picker.options.iter().enumerate() {
                    let selected = i == cursor;
                    let marker = if selected { "> " } else { "  " };
                    let label = format!("{marker}{}", to_ascii(&opt.label));
                    let row_layout = self.font.layout_ascii(&label);
                    let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                    let color = if selected {
                        [1.0, 1.0, 0.6, 1.0]
                    } else {
                        [0.8, 0.85, 1.0, 1.0]
                    };
                    out.extend(text_draws_for(&row_layout, row_pen, color));
                }
            }
        }

        // Inline-script field-VM runner box (the `--vm-dialogue` faithful path).
        // Same layout as the simplified panel, but the source is
        // `world.inline_dialogue`, which the world ticks itself.
        if let Some(id) = self.session.host.world.inline_dialogue.as_ref() {
            let to_ascii = |bytes: &[u8]| -> String {
                bytes
                    .iter()
                    .map(|&b| {
                        if (0x20..=0x7E).contains(&b) {
                            b as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            };
            let page = to_ascii(&id.page_bytes());
            if !page.is_empty() {
                let layout = self.font.layout_ascii(&page);
                let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
                out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));
                if id.menu_active()
                    && let Some(picker) = id.picker()
                {
                    let line_h = 16i32;
                    let cursor = id.picker_cursor();
                    for (i, opt) in picker.options.iter().enumerate() {
                        let selected = i == cursor;
                        let marker = if selected { "> " } else { "  " };
                        let label = format!("{marker}{}", to_ascii(&opt.label));
                        let row_layout = self.font.layout_ascii(&label);
                        let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                        let color = if selected {
                            [1.0, 1.0, 0.6, 1.0]
                        } else {
                            [0.8, 0.85, 1.0, 1.0]
                        };
                        out.extend(text_draws_for(&row_layout, row_pen, color));
                    }
                }
            }
        }
        out
    }
}

impl ApplicationHandler for PlayWindowApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "legaia-engine") {
            return;
        }
        // Opt-in PSX-faithful rendering: affine (perspective-incorrect) UV
        // warp + sub-pixel vertex jitter + 15-bit BGR555 ordered dithering on
        // the 3D mesh pipelines. Off by default (clean modern output); enable
        // with `LEGAIA_PSX_RENDER=1`.
        if std::env::var_os("LEGAIA_PSX_RENDER").is_some()
            && let Some(r) = self.win.renderer.as_ref()
        {
            r.set_psx_mode(true);
            log::info!("play-window: PSX-faithful render mode enabled");
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Flush any pending record log before exiting so an Escape /
                // window-close mid-session produces a usable replay file.
                if let Some(log) = self.record_log.as_mut()
                    && let Err(e) = log.flush()
                {
                    log::error!("record: flush on CloseRequested failed: {e:#}");
                }
                evl.exit();
            }
            WindowEvent::Resized(size) => self.win.handle_resize(size.width, size.height),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                if matches!(code, KeyCode::Escape) && state == ElementState::Pressed {
                    if let Some(log) = self.record_log.as_mut()
                        && let Err(e) = log.flush()
                    {
                        log::error!("record: flush on Escape failed: {e:#}");
                    }
                    evl.exit();
                    return;
                }
                // Dev affordance: spawn a debug effect marker at the player so
                // the effect-pool render bridge can be exercised by hand
                // before the runtime effect catalog is wired into battle-enter.
                if matches!(code, KeyCode::KeyE)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    let pos = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x as f32,
                                a.move_state.world_y as f32,
                                a.move_state.world_z as f32,
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0]);
                    self.session.host.world.spawn_debug_effect(pos);
                    return;
                }
                // `F`: seat a synthetic *Tail Fire* effect carrying its `etmd`
                // 3D model (global TMD pool index 4, textured by the resident
                // `etim` texels) at the first active actor, exercising the
                // model render path with a fixed model index. The data-driven
                // effect-id -> etmd-model selection is decoded (move-power
                // record `+0x12`/`+0x16` effect-id lists -> the `0x801F6324`
                // prototype table -> record `model_sel` -> `global_tmd_pool
                // [model_sel + 3]`; `legaia_asset::move_power::EffectListEntry`
                // + `World::spawn_move_fx`, exercised by the `H` key); `F` is
                // just the simpler fixed-model dev hand-spawn.
                if matches!(code, KeyCode::KeyF)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    let pos = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x as f32,
                                a.move_state.world_y as f32,
                                a.move_state.world_z as f32,
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0]);
                    // Prefer the real Gimard *Tail Fire* mesh from the PROT
                    // 0871 effect-model library when it's resident; fall back
                    // to the PROT 0874 §0 preview stand-in otherwise.
                    let model_index = if self
                        .session
                        .host
                        .world
                        .global_tmd(legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
                        .is_some()
                    {
                        legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX
                    } else {
                        legaia_engine_core::scene::ETMD_TAIL_FIRE_MODEL_INDEX
                    };
                    self.session
                        .host
                        .world
                        .spawn_debug_effect_model(pos, model_index);
                    return;
                }
                // `G`: debug-spawn the Gimard *Tail Fire* summon scene-graph
                // (PROT 0905). Loads the stager overlay, parses its part
                // records, and seats a `SummonScene` at the first active actor;
                // it then animates each frame via `tick_summon` (the move VM)
                // and renders through the summon-part draw block below. Not the
                // production cast-band trigger (state 0x29) -- a hand-spawn to
                // exercise the driver, like the `F`-key static effect spawn.
                if matches!(code, KeyCode::KeyG)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    // In battle, debug-spawn the faithful summon creature
                    // (Gimard, 0x81) through the battle-creature render path.
                    if self.session.host.world.mode == SceneMode::Battle {
                        self.spawn_summon_creature(0x81);
                        return;
                    }
                    const PROT_GIMARD_SUMMON_STAGER: u32 = 905;
                    let origin = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ]
                        })
                        .unwrap_or([0, 0, 0]);
                    match self
                        .session
                        .host
                        .index
                        .entry_bytes(PROT_GIMARD_SUMMON_STAGER)
                    {
                        Ok(bytes) => {
                            let overlay = legaia_asset::summon_overlay::parse(
                                &bytes,
                                legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE,
                            );
                            self.session.host.world.spawn_summon(
                                &overlay,
                                &bytes,
                                legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX,
                                origin,
                            );
                            log::info!(
                                "spawned Gimard summon: {} parts at {origin:?}",
                                overlay.parts.len()
                            );
                        }
                        Err(e) => log::warn!("summon spawn: read PROT 0905: {e:#}"),
                    }
                    return;
                }
                // `H`: debug-spawn a battle move's effect-FX scene-graph (the
                // move-power `0x801f6324` prototype records, summon-format
                // move-VM parts spawned through `FUN_80021B04`). Battle only -
                // the move-power table + overlay are battle-context. Move id
                // 0x06 is the worked example with library-mesh Spawn entries
                // (0x27 / 0x28); its parts resolve into the effect-model library
                // `global_tmd_pool[model_sel + 3]` and animate via `tick_move_fx`,
                // rendered by the move-FX draw block below.
                if matches!(code, KeyCode::KeyH)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                {
                    if self.session.host.world.mode != SceneMode::Battle {
                        log::info!("move-FX spawn (H) is battle-only");
                        return;
                    }
                    const MOVE_FX_DEBUG_MOVE_ID: u8 = 0x06;
                    let origin = self
                        .session
                        .host
                        .world
                        .actors
                        .iter()
                        .find(|a| a.active)
                        .map(|a| {
                            [
                                a.move_state.world_x,
                                a.move_state.world_y,
                                a.move_state.world_z,
                            ]
                        })
                        .unwrap_or([0, 0, 0]);
                    if self
                        .session
                        .host
                        .world
                        .spawn_move_fx(MOVE_FX_DEBUG_MOVE_ID, origin)
                    {
                        log::info!(
                            "spawned move-FX for move {MOVE_FX_DEBUG_MOVE_ID:#04x}: {} mesh parts at {origin:?}",
                            self.session.host.world.active_move_fx_part_draws().len()
                        );
                        // Consume the surfaced presentation fields: the trail
                        // texpage (render layer's streak pass) and the sound cue
                        // (routed through the FUN_8004fcc8 dispatch decode). The
                        // host has no battle SFX bank wired yet, so the cue is
                        // resolved + logged rather than fired through the SPU.
                        if let Some(trail) = self.session.host.world.active_move_fx_trail_texpage()
                        {
                            log::info!("  move-FX trail texpage = {trail:#06x}");
                        }
                        if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                            let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                            log::info!("  move-FX sound cue {cue:#04x} -> {dispatch:?}");
                        }
                    } else {
                        log::info!(
                            "move-FX spawn for move {MOVE_FX_DEBUG_MOVE_ID:#04x} produced no parts \
                             (table not installed / no spawnable entries)"
                        );
                    }
                    return;
                }
                // `N`: open the name-entry overlay for the lead character. The
                // NEW GAME flow now opens it automatically at the `opdeene` ->
                // `town01` opening hand-off (see the prologue-handoff block
                // below); this key is a dev hand-trigger to exercise the ported
                // overlay outside that flow. The exact in-script field-VM op
                // that opens it mid-establishing-sequence is still an open RE
                // thread.
                if matches!(code, KeyCode::KeyN)
                    && state == ElementState::Pressed
                    && !self.boot_ui.is_active()
                    && !self.session.host.world.name_entry_active()
                {
                    self.session.host.world.open_name_entry(0);
                    return;
                }
                // Menu input: while the active dialog box is a multiple-choice
                // menu (a picker decoded from the inline interaction script),
                // Up/Down move the option cursor and a confirm button applies
                // the chosen option's relative jump (`FUN_80038050`) instead of
                // driving movement / dismissing the box. Resolve the key->button
                // first so the immutable `mapping` borrow ends before the
                // mutable `active_dialog` borrow.
                if state == ElementState::Pressed {
                    let is_confirm = matches!(
                        self.mapping.pad_button_for_key(keycode_to_name(code)),
                        Some(
                            legaia_engine_core::input::PadButton::Cross
                                | legaia_engine_core::input::PadButton::Circle
                        )
                    );
                    if let Some(panel) = self.active_dialog.as_mut()
                        && panel.menu_active()
                    {
                        if matches!(code, KeyCode::ArrowUp) {
                            panel.move_picker_cursor(-1);
                            return;
                        }
                        if matches!(code, KeyCode::ArrowDown) {
                            panel.move_picker_cursor(1);
                            return;
                        }
                        if is_confirm {
                            panel.confirm_menu();
                            return;
                        }
                    }
                }
                let key_name = keycode_to_name(code);
                if let Some(button) = self.mapping.pad_button_for_key(key_name) {
                    let prev = self.pad;
                    if state == ElementState::Pressed {
                        self.pad |= button.mask();
                    } else {
                        self.pad &= !button.mask();
                    }
                    // Record the transition iff the pad actually changed
                    // (auto-repeat sends a stream of Pressed events with
                    // identical mask; dedup in RecordLog::record_transition).
                    if self.pad != prev
                        && let Some(log) = self.record_log.as_mut()
                    {
                        log.record_transition(self.session.frames, self.pad);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let dt = self.win.advance_tick(100);
                // Drain up to 4 ticks per render frame so we never spiral
                // but can still catch up from minor vsync jitter.
                let ticks = self.win.drain_ticks(dt, 4);
                // In-flow windowed cutscene: when the field VM's FMV-trigger
                // op flips the world into SceneMode::Cutscene and the STR has
                // decoded, suspend world ticks and play the video in-window.
                // Once its frames drain, resume the field (`finish_cutscene`).
                if self
                    .cutscene
                    .as_ref()
                    .is_some_and(|c| c.idx >= c.frames.len())
                {
                    // Stop the cutscene audio and resume the scene sequencer
                    // (BGM was paused while the movie played).
                    if let Some(out) = self.session.audio.as_ref() {
                        out.stop_xa();
                        out.set_sequencer_paused(false);
                    }
                    self.session.host.world.finish_cutscene();
                    self.cutscene = None;
                }
                let run_ticks = if self.cutscene.is_some() { 0 } else { ticks };
                for _ in 0..run_ticks {
                    // When the boot UI is active, route input there and skip
                    // the scene tick - the player hasn't entered the world
                    // yet (or has paused into save-select).
                    if self.boot_ui.is_active() {
                        let _ = self.tick_boot_ui();
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Start in field opens the pause menu. Edge-detect so a
                    // held key doesn't auto-reopen.
                    let pressed_edge = self.pad & !self.prev_pad;
                    // Name-entry overlay is modal: while it's open the field is
                    // frozen and every pad edge routes into the entry SM (one
                    // cell / glyph per press). Mirrors the opening `town01`
                    // naming prompt, which suspends the field VM.
                    if self.session.host.world.name_entry_active() {
                        let p = pressed_edge;
                        let input = legaia_engine_core::name_entry::NameEntryInput {
                            up: p & 0x0010 != 0,
                            down: p & 0x0040 != 0,
                            left: p & 0x0080 != 0,
                            right: p & 0x0020 != 0,
                            confirm: p & 0x4000 != 0, // Cross
                            cancel: p & 0x1000 != 0,  // Triangle
                        };
                        self.session.host.world.step_name_entry(input);
                        // Keep the frame counter advancing so the caret blinks.
                        self.session.host.world.frame =
                            self.session.host.world.frame.wrapping_add(1);
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Opening-cutscene narration plays first. While its
                    // subtitle pages are on screen the field is held and a
                    // confirm press (Cross) skips to the next page; the
                    // per-page timer (World::tick) auto-advances otherwise.
                    // Only once the narration completes does a confirm reach
                    // the hand-off gate below - so the prologue narration
                    // precedes the Rim Elm transition, mirroring retail order.
                    if self.session.host.world.cutscene_narration_active() {
                        if pressed_edge & 0x4000 != 0 {
                            self.session.host.world.skip_cutscene_narration();
                        }
                        // Freeze player movement (pad held at 0) but keep the
                        // world ticking so the narration's per-page timer
                        // advances and the scene still renders.
                        self.session.host.world.set_pad(0);
                        if let Err(e) = self.session.tick() {
                            log::error!("session tick (narration): {e:#}");
                        }
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Prologue cutscene -> Rim Elm handoff. While in `opdeene`
                    // with the trigger armed, a confirm press (Cross) hands off
                    // to `town01`, mirroring FUN_801D1344's flag + pad gate.
                    if let Some(target) = self
                        .session
                        .host
                        .world
                        .take_prologue_handoff(pressed_edge & 0x4000 != 0)
                    {
                        match self.session.enter_field_live(target, &self.field_live_opts) {
                            Ok(mode) => {
                                log::info!("prologue handoff: entered '{target}' (mode={mode:?})");
                                // `enter_field_scene` installs `town01`'s opening
                                // cutscene timeline (gated on the prologue hand-off):
                                // the establishing camera + Vahn's scripted walk-out
                                // play, and the name-entry overlay opens when the
                                // timeline reaches its pinned op-`0x49` STATE_RESUME
                                // (P2[3] body `0x02c6`) - the faithful in-script
                                // trigger, not a blind host call at the hand-off.
                            }
                            Err(e) => {
                                log::warn!("prologue handoff: enter '{target}' failed ({e:#})")
                            }
                        }
                        self.prev_pad = self.pad;
                        continue;
                    }
                    if pressed_edge & 0x0008 != 0 && !self.menu_runtime.is_open() {
                        let view_money = self.session.host.world.money;
                        let play_secs = self.session.host.world.play_time_seconds;
                        let mut s = legaia_engine_core::field_menu::FieldMenuSession::new();
                        s.money = view_money.max(0) as u32;
                        s.play_time_seconds = play_secs;
                        self.boot_ui = BootUiState::FieldMenu {
                            session: s,
                            sub: None,
                        };
                        self.prev_pad = self.pad;
                        continue;
                    }
                    // Route this frame's pad into the engine before the
                    // tick so World::tick's mode dispatch (world-map
                    // controller, field-VM dialog-advance poll) sees real
                    // input. Edge detection lives in World.input. While a
                    // menu-runtime overlay (shop / inn) is up the pad drives
                    // the menu, not the field, so feed the field a neutral pad
                    // (the player must not walk while shopping).
                    let field_pad = if self.menu_runtime.is_open() {
                        0
                    } else {
                        self.pad
                    };
                    self.session.host.world.set_pad(field_pad);
                    if let Err(e) = self.session.tick() {
                        log::error!("session tick: {e:#}");
                    }
                    // A field-VM shop op (`0x49` sub-0 inline shop record) opened
                    // a priced gold shop this tick: hand the player into its buy
                    // list. The field VM is suspended (op-0x49 Armed) until the
                    // player leaves, at which point `finish_field_shop` (below)
                    // lets it resume past the merchant op.
                    if let Some(shop) = self.session.host.world.take_pending_field_shop() {
                        self.menu_runtime.open_shop_buy(shop);
                    }
                    // Production cast-band trigger: a player Seru-magic cast
                    // (spell id 0x81..=0x8b) requests a summon spawn. The
                    // faithful render is the namesake battle_data creature drawn
                    // through the enemy animation pipeline (the summon reuses
                    // that creature's mesh + per-object TRS animation), so spawn
                    // it as a battle creature rather than the move-VM scene-graph
                    // stand-in (`summon::summon_creature_id`).
                    if let Some((spell_id, _origin)) =
                        self.session.host.world.take_pending_summon_spawn()
                    {
                        self.spawn_summon_creature(spell_id);
                    }
                    // Advance an active Seru-magic summon scene-graph (the cast
                    // above, or the `G` debug spawn) through the move VM.
                    self.session.host.world.tick_summon(0x0400);
                    // Advance an active battle move-FX scene-graph (the `H` debug
                    // spawn) through the same move VM.
                    self.session.host.world.tick_move_fx(0x0400);
                    // In battle, advance each monster actor's per-object idle
                    // animation into its `pose_frame` (the render pass below
                    // deforms the mesh via the rigid `posed_rot` builder).
                    if self.session.host.world.mode == SceneMode::Battle {
                        self.session.host.world.tick_battle_animations();
                    }
                    // World-map ocean shimmer: cycle the 13-frame CLUT animation
                    // (self-gates to None off the world map).
                    self.advance_ocean_animation();
                    if self.menu_runtime.is_open() {
                        let p = self.pad;
                        let input = MenuInput {
                            cross: p & 0x4000 != 0,
                            circle: p & 0x2000 != 0,
                            triangle: p & 0x1000 != 0,
                            square: p & 0x8000 != 0,
                            up: p & 0x0010 != 0,
                            down: p & 0x0040 != 0,
                            left: p & 0x0080 != 0,
                            right: p & 0x0020 != 0,
                        };
                        self.menu_runtime.tick(&mut self.session.host.world, input);
                    }
                    // A field-VM-triggered shop the player has now closed: tell
                    // the world so the suspended op-0x49 resumes (Armed -> Done)
                    // and the field VM advances past the merchant op next tick.
                    if self.session.host.world.field_shop_open && !self.menu_runtime.is_open() {
                        self.session.host.world.finish_field_shop();
                    }
                    self.prev_pad = self.pad;
                    // Record-mode: advance the log's frame counter so
                    // `meta.frames` reflects the recorded duration even
                    // when the user closes mid-run with no pad transitions.
                    if let Some(log) = self.record_log.as_mut() {
                        log.observe_frame(self.session.frames);
                    }
                    // Drain whatever battle events the SM fired this tick,
                    // fold their gameplay-state side into the world (HP /
                    // status), and ring them into the HUD log.
                    self.drain_and_log_battle_events();
                    // Route field events: ActorSpawned events whose actor
                    // carries a `tmd_ref` queue a render-pass mesh upload
                    // so spawn-record actors appear in the scene.
                    self.drain_and_route_field_events();
                    // Mirror the world's dialog request into a rendered,
                    // typed-out panel (opened from the scene MES, dropped when
                    // the world dismisses the box).
                    self.sync_dialog_panel();
                }
                // A tick this frame may have flipped the world into
                // SceneMode::Cutscene (field-VM FMV-trigger op). Start
                // windowed STR playback if so; a cut/missing slot drains the
                // trigger as a no-op (mirrors the headless `play` loop).
                if self.cutscene.is_none() {
                    self.try_start_windowed_cutscene();
                }
                // While a cutscene plays, the window shows the video and the
                // scene render is skipped entirely.
                if self.cutscene.is_some() {
                    self.render_windowed_cutscene();
                    self.win.request_redraw();
                    return;
                }
                // On a Field<->Battle transition, upload/drop monster meshes
                // and swap the VRAM. Must run before the render borrows
                // `uploaded_vram` below (this method may re-upload it).
                self.sync_battle_render();
                // Ease the in-engine cutscene camera between Camera Configure
                // beats. Done here (outside the renderer borrow below) so the
                // interpolator can take `&mut self`; while no cutscene timeline
                // owns the scene the interp is reset so the next opening shot
                // snaps in rather than sweeping from a stale pose.
                let cutscene_cam = if self.session.host.world.mode != SceneMode::WorldMap
                    && self.session.host.world.cutscene_timeline_active()
                {
                    let (look_at, pitch, yaw, fov) = self.cutscene_view();
                    // ~0.15/frame ease => a few-frame blend at the redraw cadence.
                    Some(
                        self.cutscene_cam_interp
                            .approach(look_at, pitch, yaw, fov, 0.15),
                    )
                } else {
                    self.cutscene_cam_interp.reset();
                    None
                };
                if let (Some(r), Some(vram), Some(atlas)) = (
                    self.win.renderer.as_ref(),
                    self.uploaded_vram.as_ref(),
                    self.font_atlas.as_ref(),
                ) {
                    let (w, h) = r.surface_size();
                    let aspect = w as f32 / h.max(1) as f32;
                    // World-map mode frames the loaded map with the
                    // controller-driven camera (azimuth / zoom / pan); an active
                    // in-engine cutscene (opdeene opening prologue) frames the
                    // cutscene's executed op-0x45 camera target; every other mode
                    // uses the orbit camera.
                    let in_world_map = self.session.host.world.mode == SceneMode::WorldMap;
                    let cam = if in_world_map {
                        let world = &self.session.host.world;
                        let (az, zoom, px, pz, walk_mode) = world
                            .world_map_ctrl
                            .as_ref()
                            .map(|c| (c.azimuth, c.zoom, c.camera_x, c.camera_z, !c.is_top_view()))
                            .unwrap_or((0, 0, 0, 0, true));
                        // In walk mode the camera follows the player: pan so the
                        // framing centre tracks the player's world position
                        // (the AABB-relative offset world_map_camera_mvp adds to
                        // its centre). Top-view debug keeps the controller scroll.
                        let (pan_x, pan_z) = if walk_mode {
                            let center = [
                                (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
                                (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
                            ];
                            world
                                .player_actor_slot
                                .and_then(|s| world.actors.get(s as usize))
                                .map(|a| {
                                    (
                                        (a.move_state.world_x as f32 - center[0]) as i32,
                                        (a.move_state.world_z as f32 - center[1]) as i32,
                                    )
                                })
                                .unwrap_or((px, pz))
                        } else {
                            (px, pz)
                        };
                        // Walk view frames a fixed WORLD-space radius around the
                        // player rather than the (small, object-local) kingdom-
                        // pack AABB - the continent terrain now draws at world
                        // tile coordinates (`field_placement_draws`), so the
                        // pack-AABB radius would frame only the one tile under
                        // the player. Keep the box centred at the pack-AABB
                        // centre (the pan re-centres it on the player) and widen
                        // it; top-view keeps the full-pack framing for the
                        // overhead continent sweep.
                        let (cam_lo, cam_hi) = if walk_mode {
                            // Frame a wide world-space radius around the player so
                            // the overworld reads at retail's overhead scale (the
                            // walk camera also sits steeper - see
                            // `walk_view_camera_mvp`).
                            const WALK_HALF: f32 = 4200.0;
                            let cx = (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5;
                            let cz = (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5;
                            (
                                [cx - WALK_HALF, self.scene_aabb.0[1], cz - WALK_HALF],
                                [cx + WALK_HALF, self.scene_aabb.1[1], cz + WALK_HALF],
                            )
                        } else {
                            (self.scene_aabb.0, self.scene_aabb.1)
                        };
                        if walk_mode {
                            legaia_engine_render::window::walk_view_camera_mvp(
                                cam_lo, cam_hi, az, zoom, pan_x, pan_z, aspect,
                            )
                        } else {
                            legaia_engine_render::window::world_map_camera_mvp(
                                cam_lo, cam_hi, az, zoom, pan_x, pan_z, aspect,
                            )
                        }
                    } else if let Some((look_at, pitch, yaw, fov)) = cutscene_cam {
                        legaia_engine_render::window::cutscene_camera_mvp(
                            look_at,
                            pitch,
                            yaw,
                            fov,
                            self.scene_aabb.0,
                            self.scene_aabb.1,
                            aspect,
                        )
                    } else if self.session.host.world.mode == SceneMode::Battle {
                        if self.battle_stage_mesh.is_some() {
                            // Stage-dome battle: low front-facing shot into the
                            // dome (grass foreground, mountains on the horizon).
                            self.battle_dome_camera_mvp(aspect)
                        } else {
                            // No stage: frame the animated enemies (the battle
                            // actors live at the world origin).
                            self.battle_camera_mvp(aspect)
                        }
                    } else {
                        self.camera_mvp(aspect)
                    };
                    // Drain queued spawn slots: build a VRAM mesh from each
                    // actor's `tmd_ref` (global-pool TMD that the field-VM
                    // 0x4C 0xD8 host hook installed) and append it to
                    // `self.meshes` / `self.scene_tmd_data`, then bind
                    // `actor.tmd_binding` to the new mesh index so the
                    // draws iteration below picks it up. Idempotent: if
                    // the actor already has a binding (e.g. an earlier
                    // pass already uploaded), the spawn is skipped.
                    let pending = std::mem::take(&mut self.pending_dynamic_mesh_slots);
                    for slot in pending {
                        let actor = match self.session.host.world.actors.get(slot as usize) {
                            Some(a) => a,
                            None => continue,
                        };
                        if actor.tmd_binding.is_some() {
                            continue;
                        }
                        let Some(gtmd) = actor.tmd_ref.as_ref().map(std::sync::Arc::clone) else {
                            continue;
                        };
                        let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                        if vmesh.indices.is_empty() {
                            log::warn!(
                                "play-window: spawn slot {slot} has TMD with 0 indices; skipping"
                            );
                            continue;
                        }
                        match r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.indices,
                        ) {
                            Ok(m) => {
                                let new_idx = self.meshes.len();
                                self.meshes.push(m);
                                self.scene_tmd_data
                                    .push((gtmd.tmd.clone(), gtmd.raw.clone()));
                                self.session.host.world.actors[slot as usize].tmd_binding =
                                    Some(new_idx);
                                log::info!("play-window: spawn slot {slot} -> mesh slot {new_idx}");
                            }
                            Err(e) => log::warn!("spawn mesh upload: {e:#}"),
                        }
                    }
                    // For each active actor with a tmd_binding and a current
                    // pose_frame, regenerate and re-upload the posed mesh.
                    // posed_overrides[i] replaces meshes[i] when present.
                    let mut posed_overrides: Vec<Option<UploadedVramMesh>> =
                        (0..self.scene_tmd_data.len()).map(|_| None).collect();
                    for actor in &self.session.host.world.actors {
                        if !actor.active {
                            continue;
                        }
                        let (Some(tmd_idx), Some(pose)) = (actor.tmd_binding, &actor.pose_frame)
                        else {
                            continue;
                        };
                        let Some((tmd, raw)) = self.scene_tmd_data.get(tmd_idx) else {
                            continue;
                        };
                        // Battle actors carry a per-object rigid-transform clip
                        // (rotation matters), so use the full `R·v + T` builder;
                        // field actors keep the translation-only ANM path
                        // unchanged.
                        let mut vmesh = if actor.battle_animation.is_some() {
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(
                                tmd,
                                raw,
                                &pose.bone_outputs,
                            )
                        } else {
                            legaia_tmd::mesh::tmd_to_vram_mesh_posed(tmd, raw, &pose.bone_outputs)
                        };
                        // The posed mesh is rebuilt from the raw TMD, so its
                        // CBA/TSB are the nominal on-disc defaults. Re-apply the
                        // per-slot relocation `battle_render_mesh` did for the
                        // rest mesh, or the animated monster samples the wrong
                        // VRAM page and renders white.
                        if let Some(slot) = actor.battle_tex_slot {
                            for ct in &mut vmesh.cba_tsb {
                                ct[0] = legaia_asset::monster_archive::relocate_cba(ct[0], slot);
                                ct[1] = legaia_asset::monster_archive::relocate_tsb(ct[1], slot);
                            }
                        }
                        if vmesh.indices.is_empty() {
                            continue;
                        }
                        match r.upload_vram_mesh(
                            &vmesh.positions,
                            &vmesh.uvs,
                            &vmesh.cba_tsb,
                            &vmesh.normals,
                            &vmesh.indices,
                        ) {
                            Ok(m) => posed_overrides[tmd_idx] = Some(m),
                            Err(e) => log::warn!("posed mesh upload: {e:#}"),
                        }
                    }

                    // Iterate every actor that has a `tmd_binding`. Scene-init
                    // actors (slots 0..N from `init_scene_animations`) have
                    // their bindings set but aren't necessarily `.active` -
                    // the original draws iteration walked meshes directly,
                    // so we preserve that behaviour by not gating on
                    // `.active` here. Dynamically spawned actors set both
                    // `.active` and a binding to their freshly uploaded
                    // mesh slot (beyond `scene_tmd_data.len()`) via the
                    // spawn pass above.
                    //
                    // Suppress 3D draws while the boot UI is active so the
                    // last-loaded scene (e.g. a town) doesn't show through
                    // behind publisher logos / title / save-select.
                    let mut draws: Vec<SceneDraw<'_>> = Vec::new();
                    // Untextured (F*/G*) field props, drawn on the colour
                    // pipeline alongside the textured `draws`.
                    let mut color_draws: Vec<ColorSceneDraw<'_>> = Vec::new();
                    if self.boot_ui.is_active() {
                        // Boot UI is fullscreen - suppress 3D draws.
                    } else if in_world_map {
                        // World-map continent = two layers, both in the shared
                        // player / entity-marker world frame:
                        //
                        // 1. The **ground** is a heightfield surface
                        //    (`world_map_heightfield`) built from the walk
                        //    `.MAP` floor grid (`Scene::walk_heightfield`,
                        //    elevation per `FUN_80019278`). It draws with a
                        //    provisional uniform ground texel: per-tile
                        //    texturing has no clean source - the record `+0x14`
                        //    byte is terrain-type metadata, not an atlas
                        //    selector (no draw path reads it; see
                        //    docs/subsystems/world-map.md "Open (texturing)").
                        // 2. The sparse **placed landmarks** (trees / mountains
                        //    / castle) are slot-1 pack meshes positioned per
                        //    occupied tile (`world_map_terrain_draws`, the
                        //    `flags & 0x4` set resolved via record[+0x10]+prefix).
                        //
                        // The earlier per-cell pack-mesh sweep that stamped a
                        // mesh on every `0x1000` cell was wrong (it flooded the
                        // map with pool-5; see docs/subsystems/world-map.md).
                        let yflip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                        if let Some(hf_mesh) = self.world_map_heightfield.as_ref() {
                            draws.push(SceneDraw {
                                mesh: hf_mesh,
                                mvp: cam * yflip,
                            });
                        }
                        for (mesh_idx, model) in self.world_map_terrain_draws.iter() {
                            if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * *model,
                                });
                            }
                        }
                        // Last-resort fallback: nothing resolved at all -> draw
                        // the whole pack at pack-local coords so the map isn't
                        // blank.
                        if self.world_map_heightfield.is_none()
                            && self.world_map_terrain_draws.is_empty()
                        {
                            for mesh in &self.meshes {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * yflip,
                                });
                            }
                        }
                    } else {
                        let in_battle = self.session.host.world.mode == SceneMode::Battle;
                        if in_battle {
                            // Battle backdrop: the scene's `scene_tmd_stream`
                            // dome (PROT 88 for the overworld map01 battle) —
                            // sky hemisphere + mountain arc + grass — drawn at its
                            // **raw world coordinates** under the exact retail
                            // orbit camera (`retail_battle_mvp`). `model = F`
                            // (plain Y-flip): the camera bakes in `F`, so
                            // `cam * F` recovers the raw PSX vertex the retail
                            // transform expects.
                            //
                            // Drawn ONCE, world-fixed — matching retail, which
                            // sets the dome up as a background **actor**
                            // (`FUN_800513F0`: `tmd_register` -> `DAT_8007C018[]`
                            // + `FUN_80020de0` actor_alloc + `FUN_80020f88` link)
                            // rendered by the normal actor path `FUN_80048A08`.
                            // The dome is a FRONT half (verts `Z in [-1260,
                            // +12155]`), so it is NOT a full surround: as the
                            // camera orbits, different portions of the front arc
                            // come into view and the rest of the horizon is open
                            // sky/grass. The retail captures bear this out —
                            // mountains cover only 44–81% of the horizon columns
                            // depending on angle (NOT a ring). Earlier engine
                            // builds added a 180° mirror to "complete" the
                            // surround; that over-fills what retail leaves as a
                            // gap, so it is removed. See
                            // `project_battle_backdrop_is_prot88_dome`.
                            if let Some(stage_idx) = self.battle_stage_mesh
                                && let Some(mesh) = self.meshes.get(stage_idx)
                            {
                                let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                // Front half-dome at raw world coords.
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * flip,
                                });
                                // The dome geometry is a FRONT half (Z>=0), so a
                                // single instance leaves the back of the horizon
                                // open. Draw a 180-deg-Y mirror so the mountain
                                // ring + sky complete the full circle around the
                                // actors. (Retail's dome is a front half with
                                // partial coverage; the mirror reads fuller.)
                                let back = flip * Mat4::from_rotation_y(std::f32::consts::PI);
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: cam * back,
                                });
                            }
                        } else {
                            // Static environment geometry: draw each placed
                            // building / terrain mesh at its world transform
                            // (resolved at scene load in
                            // `resolve_field_placement_draws`).
                            for (mesh_idx, model) in &self.field_placement_draws {
                                if let Some(mesh) = self.meshes.get(*mesh_idx) {
                                    draws.push(SceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                            // Untextured props (the F*/G* meshes the VRAM path
                            // drops) on the colour pipeline, same transforms.
                            for (mesh_idx, model) in &self.field_placement_color_draws {
                                if let Some(mesh) = self.color_meshes.get(*mesh_idx) {
                                    color_draws.push(ColorSceneDraw {
                                        mesh,
                                        mvp: cam * *model,
                                    });
                                }
                            }
                        }
                        // Single battle camera for the actors too (same `cam` as
                        // the dome + grid) so the whole scene shares one space.
                        let actor_cam = cam;
                        // Flat tiled ground grid (retail's func_0x801d02c0 grass)
                        // under the actors, on the same battle camera so the
                        // party stands on it and the foreground reads as grass
                        // instead of the bare clear colour. `cam` bakes in the
                        // Y-flip, so `* flip` recovers the raw PSX y=0 plane.
                        if in_battle
                            && let Some(gi) = self.battle_ground_mesh
                            && let Some(gmesh) = self.meshes.get(gi)
                        {
                            let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                            draws.push(SceneDraw {
                                mesh: gmesh,
                                mvp: cam * flip,
                            });
                        }
                        for (i, actor) in self.session.host.world.actors.iter().enumerate() {
                            let Some(tmd_idx) = actor.tmd_binding else {
                                continue;
                            };
                            // In a stage-dome battle, draw only the ACTIVE battle
                            // actors (party + monsters). The scene-init actors
                            // (bound but inactive, parked at the origin) would
                            // otherwise pile their meshes at world (0,0,0) - the
                            // "duplicate Vahn" + scattered scene geometry.
                            if in_battle && self.battle_stage_mesh.is_some() && !actor.active {
                                continue;
                            }
                            let mesh = posed_overrides
                                .get(tmd_idx)
                                .and_then(|o| o.as_ref())
                                .or_else(|| self.meshes.get(tmd_idx));
                            if let Some(mesh) = mesh {
                                draws.push(SceneDraw {
                                    mesh,
                                    mvp: actor_cam * self.actor_model(i),
                                });
                            }
                        }
                    }
                    let hud = self.build_hud(w, h);
                    let overlay = TextOverlay { atlas, draws: &hud };

                    // Boot-phase sprite overlay: alternates between the
                    // publisher-logos atlas (during PublisherLogos) and
                    // the title-screen atlas (during Title). PROKION/SCEA
                    // are vertically-packed sprite atlases —
                    // `publisher_logo_sprite_draws` unfolds them into N
                    // side-by-side strips; Contrail/WARNING + the title
                    // TIM produce a single quad each.
                    let logo_draw_vec = self.publisher_logo_sprite_draws(w, h);
                    let title_draw_vec = self.title_screen_sprite_draws(w, h);
                    let menu_glyph_draw_vec = self.title_menu_glyph_sprite_draws(w, h);
                    let save_chrome_draw_vec = self.save_select_chrome_sprite_draws(w, h);
                    let logo_overlay = self.publisher_logos.as_ref().map(|p| TextOverlay {
                        atlas: &p.atlas,
                        draws: &logo_draw_vec,
                    });
                    let title_overlay = self.title_screen.as_ref().map(|t| TextOverlay {
                        atlas: &t.atlas,
                        draws: &title_draw_vec,
                    });
                    let menu_glyph_overlay = self.menu_glyphs.as_ref().map(|m| TextOverlay {
                        atlas: &m.atlas,
                        draws: &menu_glyph_draw_vec,
                    });
                    let save_chrome_overlay = self.save_menu.as_ref().map(|sm| TextOverlay {
                        atlas: &sm.atlas,
                        draws: &save_chrome_draw_vec,
                    });

                    // Force a pure-black background during boot UI so the
                    // logos / title / save-select panels read on PSX-style
                    // black instead of the default dark-blue clear. In a
                    // stage-dome battle clear to a sky blue so the gaps the
                    // front-half dome leaves open read as sky (like retail)
                    // rather than the bare grey clear.
                    let scene_clear = if self.boot_ui.is_active() {
                        Some([0.0, 0.0, 0.0, 1.0])
                    } else if self.session.host.world.mode == SceneMode::Battle
                        && self.battle_stage_mesh.is_some()
                    {
                        Some([0.32, 0.46, 0.66, 1.0])
                    } else {
                        None
                    };

                    // Slot 1: logos OR title-art bands (title still
                    // emits during SaveSelect, dimmed). Slot 2: either
                    // the save-menu chrome (panel + slot pills) when
                    // SaveSelect is active, or the menu-glyph atlas
                    // (deprecated no-disc title-menu fallback) otherwise.
                    let sprites_slot_1 = if !logo_draw_vec.is_empty() {
                        logo_overlay.as_ref()
                    } else if !title_draw_vec.is_empty() {
                        title_overlay.as_ref()
                    } else {
                        None
                    };
                    let sprites_slot_2 = if !save_chrome_draw_vec.is_empty() {
                        save_chrome_overlay.as_ref()
                    } else if !menu_glyph_draw_vec.is_empty() {
                        menu_glyph_overlay.as_ref()
                    } else {
                        None
                    };
                    // Effect-pool billboards: bridge live effect child sprites
                    // into the renderer as faithful camera-facing quads sized
                    // and UV-addressed from the effect bundle's inline atlas
                    // (`World::active_effect_sprites`). Each draws two ways: a
                    // textured quad sampling the scene VRAM at the sprite's
                    // atlas page/clut/uv (the retail FUN_801E0088 pass-2 path -
                    // invisible while the texel-source upload is unpinned, real
                    // once it lands), plus a tinted outline through the Lines
                    // pipeline so the spawn is visible now. See
                    // docs/subsystems/effect-vm.md.
                    let (effect_billboard, effect_lines) = if self.boot_ui.is_active() {
                        (None, None)
                    } else {
                        let sprites = self.session.host.world.active_effect_sprites();
                        if sprites.is_empty() {
                            (None, None)
                        } else {
                            // Camera right/up in world space (clip-space basis
                            // dirs mapped back through the inverse MVP).
                            let inv = cam.inverse();
                            let right = inv.transform_vector3(Vec3::X).normalize_or_zero();
                            let up = inv.transform_vector3(Vec3::Y).normalize_or_zero();
                            let mesh = effect_billboard_mesh(r, &sprites, right, up);
                            let (pos, col, idx) = effect_sprite_line_geometry(&sprites, right, up);
                            let lines = match r.upload_lines(&pos, &col, &idx) {
                                Ok(m) => Some(m),
                                Err(e) => {
                                    log::warn!("effect outline lines upload: {e:#}");
                                    None
                                }
                            };
                            (mesh, lines)
                        }
                    };
                    if let Some(mesh) = effect_billboard.as_ref() {
                        draws.push(SceneDraw { mesh, mvp: cam });
                    }
                    // World-map overlay lines: a kind-coded upright marker for
                    // each placed entity (portal / NPC / encounter zone, from
                    // `World::world_map_entity_markers`) plus the player marker
                    // (`World::world_map_player_marker`) - the player's own mesh
                    // isn't drawn in world-map mode. Both build into one Lines
                    // mesh, routed through the same overlay slot as the effect
                    // outlines (mutually exclusive: no effects spawn on the
                    // world map). Without this the installed entities + player
                    // carry positions but never appear on screen.
                    let world_map_entity_lines = if in_world_map && !self.boot_ui.is_active() {
                        let markers = self.session.host.world.world_map_entity_markers();
                        let mut pos: Vec<[f32; 3]> = Vec::new();
                        let mut col: Vec<[u8; 4]> = Vec::new();
                        let mut idx: Vec<u32> = Vec::new();
                        if !markers.is_empty() {
                            let (p, c, i) = world_map_entity_line_geometry(
                                &markers,
                                self.scene_aabb.0,
                                self.scene_aabb.1,
                            );
                            pos = p;
                            col = c;
                            idx = i;
                        }
                        if let Some(player) = self.session.host.world.world_map_player_marker() {
                            let (p, c, i) = world_map_player_line_geometry(
                                &player,
                                self.scene_aabb.0,
                                self.scene_aabb.1,
                            );
                            let base = pos.len() as u32;
                            pos.extend(p);
                            col.extend(c);
                            idx.extend(i.into_iter().map(|v| v + base));
                        }
                        if idx.is_empty() {
                            None
                        } else {
                            match r.upload_lines(&pos, &col, &idx) {
                                Ok(m) => Some(m),
                                Err(e) => {
                                    log::warn!("world-map overlay marker lines upload: {e:#}");
                                    None
                                }
                            }
                        }
                    } else {
                        None
                    };
                    // Effect 3D models (`etmd.dat`): spell effects like Tail
                    // Fire are small Gouraud-shaded `etmd` meshes textured by
                    // the resident `etim` texels, not billboards. Build a
                    // per-frame VRAM mesh + transform for each live effect that
                    // has a model assigned (same Y-flip model-matrix convention
                    // as `actor_model`). Held in a local Vec so the meshes
                    // outlive the render borrow.
                    let mut effect_model_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
                    if !self.boot_ui.is_active() && !in_world_map {
                        for em in self.session.host.world.active_effect_models() {
                            let Some(gtmd) = self
                                .session
                                .host
                                .world
                                .global_tmd(em.tmd_index as i16)
                                .map(std::sync::Arc::clone)
                            else {
                                continue;
                            };
                            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                            if vmesh.indices.is_empty() {
                                continue;
                            }
                            match r.upload_vram_mesh(
                                &vmesh.positions,
                                &vmesh.uvs,
                                &vmesh.cba_tsb,
                                &vmesh.normals,
                                &vmesh.indices,
                            ) {
                                Ok(m) => {
                                    let model = Mat4::from_translation(Vec3::from(em.world_pos))
                                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                    effect_model_draws.push((m, model));
                                }
                                Err(e) => log::warn!("effect model mesh upload: {e:#}"),
                            }
                        }
                    }
                    for (mesh, model) in &effect_model_draws {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }

                    // Active Seru-magic summon scene-graph (debug-spawned via
                    // `G`): one textured mesh per move-VM-driven part, posed by
                    // the part's interpreted transform (world pos + rotation
                    // banks). The animation computation is faithful (move VM);
                    // the transform composition is the open PROT 0900 piece.
                    let mut summon_part_draws: Vec<(UploadedVramMesh, Mat4)> = Vec::new();
                    if !self.boot_ui.is_active() && !in_world_map {
                        // Summon parts and battle move-FX parts render identically
                        // (both are move-VM scene-graph parts resolving into
                        // `global_tmd_pool`); draw both sets the same way.
                        let part_draws = self
                            .session
                            .host
                            .world
                            .active_summon_part_draws()
                            .into_iter()
                            .chain(self.session.host.world.active_move_fx_part_draws());
                        for sp in part_draws {
                            let Some(gtmd) = self
                                .session
                                .host
                                .world
                                .global_tmd(sp.model_index as i16)
                                .map(std::sync::Arc::clone)
                            else {
                                continue;
                            };
                            let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&gtmd.tmd, &gtmd.raw);
                            if vmesh.indices.is_empty() {
                                continue;
                            }
                            match r.upload_vram_mesh(
                                &vmesh.positions,
                                &vmesh.uvs,
                                &vmesh.cba_tsb,
                                &vmesh.normals,
                                &vmesh.indices,
                            ) {
                                Ok(m) => {
                                    let model = Mat4::from_translation(Vec3::from(sp.world_pos))
                                        * Mat4::from_rotation_y(sp.rot[1])
                                        * Mat4::from_rotation_x(sp.rot[0])
                                        * Mat4::from_rotation_z(sp.rot[2])
                                        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
                                    summon_part_draws.push((m, model));
                                }
                                Err(e) => log::warn!("summon/move-FX part mesh upload: {e:#}"),
                            }
                        }
                    }
                    for (mesh, model) in &summon_part_draws {
                        draws.push(SceneDraw {
                            mesh,
                            mvp: cam * *model,
                        });
                    }
                    let scene = RenderScene {
                        vram,
                        draws: &draws,
                        color_draws: &color_draws,
                        overlay_lines: world_map_entity_lines
                            .as_ref()
                            .or(effect_lines.as_ref())
                            .map(|m| (m, cam)),
                        overlay_sprites: sprites_slot_1,
                        overlay_sprites_2: sprites_slot_2,
                        overlay_text: Some(&overlay),
                        clear_color: scene_clear,
                    };
                    if let Err(e) = r.render(RenderTarget::Scene(&scene)) {
                        log::error!("render: {e:#}");
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

/// World-unit size of one texel when drawing an effect billboard (the atlas
/// stores sprite extents in texels; the renderer scales them to world units).
const EFFECT_TEXEL_WORLD: f32 = 1.0;

/// The four world-space corners of a camera-facing billboard for `sprite`,
/// using the camera's world `right`/`up` basis. Order: TL, TR, BL, BR.
fn effect_sprite_corners(
    sprite: &legaia_engine_core::world::EffectSprite,
    right: Vec3,
    up: Vec3,
) -> [Vec3; 4] {
    let c = Vec3::from(sprite.world_pos);
    let hw = sprite.size[0] * 0.5 * EFFECT_TEXEL_WORLD;
    let hh = sprite.size[1] * 0.5 * EFFECT_TEXEL_WORLD;
    let rx = right * hw;
    let uy = up * hh;
    [c - rx + uy, c + rx + uy, c - rx - uy, c + rx - uy]
}

/// Build a textured billboard mesh for the live effect sprites: one
/// camera-facing quad per child, sampling the scene VRAM at the sprite's
/// atlas `(u, v)` / `tpage` / `clut`. Mirrors the retail per-frame walker
/// (`FUN_801E0088` pass 2), which emits one GPU sprite primitive per child.
///
/// The texel-source upload for battle effects is not yet pinned, so a quad
/// over empty VRAM samples all-zero texels which the VRAM-mesh shader
/// discards (clean, not garbage); real pixels appear once that upload lands.
/// Returns `None` when there is nothing to draw.
fn effect_billboard_mesh(
    r: &legaia_engine_render::Renderer,
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> Option<UploadedVramMesh> {
    if sprites.is_empty() {
        return None;
    }
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut uvs: Vec<[u8; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut cba_tsb: Vec<[u16; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(sprites.len() * 6);
    // Quad faces the camera; a single normal toward the viewer keeps the
    // lambert term stable rather than relying on the derivative fallback.
    let face = right.cross(up).normalize_or_zero().to_array();
    for s in sprites {
        let [u0, v0] = s.uv;
        let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1)).min(255) as u8;
        let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1)).min(255) as u8;
        let u0 = (u0 & 0xFF) as u8;
        let v0 = (v0 & 0xFF) as u8;
        let corners = effect_sprite_corners(s, right, up);
        let corner_uv = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]];
        let base = positions.len() as u32;
        for (corner, uv) in corners.iter().zip(corner_uv) {
            positions.push(corner.to_array());
            uvs.push(uv);
            cba_tsb.push([s.clut, s.page]);
            normals.push(face);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    match r.upload_vram_mesh(&positions, &uvs, &cba_tsb, &normals, &indices) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("effect billboard mesh upload: {e:#}");
            None
        }
    }
}

/// Build a tinted outline for each effect billboard through the Lines
/// pipeline (a camera-facing rectangle, sized from the sprite atlas, faded by
/// age). This keeps spawned effects visible while the textured-quad's VRAM
/// source is unpinned - the billboard's geometry and animation are faithful
/// even when its texels are not yet resident.
fn effect_sprite_line_geometry(
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(sprites.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(sprites.len() * 8);
    for s in sprites {
        let [tl, tr, bl, br] = effect_sprite_corners(s, right, up);
        // Warm spark colour, dimmed as the effect ages toward retirement.
        let fade = (1.0 - s.age01).clamp(0.0, 1.0);
        let c = [
            (80.0 + 175.0 * fade) as u8,
            (200.0 * fade) as u8,
            (255.0 * fade) as u8,
            255,
        ];
        let base = pos.len() as u32;
        for corner in [tl, tr, br, bl] {
            pos.push(corner.to_array());
            col.push(c);
        }
        // Four edges of the rectangle (LineList).
        for &(a, b) in &[(0u32, 1u32), (1, 2), (2, 3), (3, 0)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// RGBA colour of a world-map entity marker, keyed by its kind: portals
/// (town/dungeon entrances) cyan, NPCs green, encounter zones warm red.
fn world_map_entity_marker_color(kind: legaia_engine_core::world::WorldMapEntityKind) -> [u8; 4] {
    use legaia_engine_core::world::WorldMapEntityKind as K;
    match kind {
        K::Portal => [0, 200, 255, 255],
        K::Npc => [80, 220, 80, 255],
        K::EncounterZone => [230, 80, 40, 255],
    }
}

/// World-map ocean CLUT animation state. Holds the 13 BGR555 frames (32 bytes
/// each) decoded from the kingdom bundle and the current frame cursor + tick
/// accumulator. Each step overwrites the first 16 CLUT entries at VRAM
/// `(0, 506)` with the next frame, reproducing the retail rolling-wave DMA.
struct OceanAnim {
    /// 13 frames × 32 bytes (16 BGR555 entries each), as decoded by
    /// [`legaia_asset::ocean::find_ocean_assets`].
    frames: Vec<u8>,
    /// Current frame index (`0..frames.len()/32`).
    cur: usize,
    /// Sim-tick accumulator; the frame advances every
    /// [`OCEAN_ANIM_TICKS_PER_FRAME`] ticks.
    tick: u32,
}

/// Sim ticks between ocean-CLUT frame advances. A gentle shimmer: the 13-frame
/// cycle completes in ~1.3 s at 60 Hz. The exact retail DMA cadence isn't
/// pinned, so this is a tuned approximation, not a parity figure.
const OCEAN_ANIM_TICKS_PER_FRAME: u32 = 6;

/// Convert a [`WalkHeightfield`] into a renderer [`VramMesh`]. The heightfield
/// supplies per-vertex UVs (the `+0x14` atlas tile) **and** per-vertex
/// `[clut, tpage]` (the cell's terrain page + palette from `+0x15` /
/// `+0x16..+0x18`), so grass / mountain / water / forest cells each sample their
/// own VRAM page within the single ground mesh. Normals are left at the
/// `[0,0,0]` sentinel so the shader derives screen-space normals (flat-lit).
/// See docs/subsystems/world-map.md "Ground texturing".
fn heightfield_to_vram_mesh(
    hf: &legaia_asset::field_objects::WalkHeightfield,
) -> legaia_tmd::mesh::VramMesh {
    let n = hf.positions.len();
    legaia_tmd::mesh::VramMesh {
        positions: hf.positions.clone(),
        uvs: hf.uvs.clone(),
        // Per-cell terrain page + palette (multi-page terrain atlas).
        cba_tsb: hf.cba_tsb.clone(),
        normals: vec![[0.0, 0.0, 0.0]; n],
        indices: hf.indices.clone(),
    }
}

/// MAN), so they sit correctly relative to the player even while the kingdom
/// terrain mesh renders at its own pack-local coordinates.
fn world_map_entity_line_geometry(
    markers: &[legaia_engine_core::world::WorldMapEntityMarker],
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.06;
    let arm = diag * 0.02;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(markers.len() * 6);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(markers.len() * 6);
    let mut idx: Vec<u32> = Vec::with_capacity(markers.len() * 6);
    for m in markers {
        let [x, y, z] = m.world_pos;
        let c = world_map_entity_marker_color(m.kind);
        let base = pos.len() as u32;
        // 0: base, 1: top (up = world -Y under the geometry convention),
        // 2..=5: base-cross arm ends along +/-X and +/-Z.
        let verts = [
            [x, y, z],
            [x, y - post_h, z],
            [x - arm, y, z],
            [x + arm, y, z],
            [x, y, z - arm],
            [x, y, z + arm],
        ];
        for v in verts {
            pos.push(v);
            col.push(c);
        }
        // Vertical post + the two base-cross segments.
        for &(a, b) in &[(0u32, 1u32), (2, 3), (4, 5)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// Build a LineList for the overworld player marker: a taller upright post (so
/// the player reads above the kind-coded entity markers), a base cross, and a
/// facing tick pointing in the player's heading. White-yellow, sized relative
/// to the scene AABB. Same Y-flip convention as the entity markers.
fn world_map_player_line_geometry(
    marker: &legaia_engine_core::world::WorldMapPlayerMarker,
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.09;
    let arm = diag * 0.025;
    let tick = diag * 0.05;
    let [x, y, z] = marker.world_pos;
    let c = [255u8, 230, 60, 255];
    // Heading: PSX 12-bit angle, 0 = +Z, quarter turn (1024) = +X.
    let angle = (marker.facing as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let verts = [
        [x, y, z],                           // 0 base
        [x, y - post_h, z],                  // 1 top
        [x - arm, y, z],                     // 2 -X arm
        [x + arm, y, z],                     // 3 +X arm
        [x, y, z - arm],                     // 4 -Z arm
        [x, y, z + arm],                     // 5 +Z arm
        [x + sin * tick, y, z + cos * tick], // 6 facing tick end
    ];
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(7);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(7);
    for v in verts {
        pos.push(v);
        col.push(c);
    }
    // Post + base-cross (X/Z arms) + facing tick.
    let idx = vec![0, 1, 2, 3, 4, 5, 0, 6];
    (pos, col, idx)
}

/// Map a winit `KeyCode` to the user-friendly key name used in
/// [`legaia_engine_core::input::Mapping`]. Returns `""` for keys outside
/// the default set.
fn keycode_to_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyW => "W",
        KeyCode::Enter => "Enter",
        KeyCode::ShiftRight => "RShift",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        _ => "",
    }
}

/// Parse a GameShark `.gs.txt` or Mednafen `.cht` cheat file and
/// apply every entry to `world` through the
/// [`legaia_engine_core::cheat_applier`] registry. Logs per-entry
/// status to stderr.
fn apply_cheat_file(
    world: &mut legaia_engine_core::world::World,
    path: &Path,
    strict: bool,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut db = if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cht"))
        .unwrap_or(false)
    {
        legaia_cheats::parse_mednafen_cht(&text)?
    } else {
        legaia_cheats::parse_gs_text(&text)?
    };
    db.dedupe_identical();
    let opts = legaia_engine_core::cheat_applier::ApplyOptions {
        execute_conditionals: !strict,
        skip_unmapped: false,
    };
    let report = legaia_engine_core::cheat_applier::apply(world, &db, opts);
    eprintln!(
        "Cheat report ({} entries, {} writes; {} applied, {} unmapped, {} unknown):",
        report.per_entry.len(),
        report.total_writes,
        report.applied,
        report.unmapped,
        report.unknown_addresses
    );
    for entry in &report.per_entry {
        let total = entry.applied + entry.skipped;
        let tag = if entry.applied == total {
            "ok  "
        } else if entry.applied == 0 {
            "skip"
        } else {
            "part"
        };
        eprintln!(
            "  {tag}  {:.<60} {}/{} writes",
            entry.description, entry.applied, total
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    vm_dialogue: bool,
    terrain_y: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        str_file,
        boot_ui,
        save_dir,
        cutscene_map_path,
        cheat_file,
        cheat_strict,
        live_loop,
        player_battle,
        vm_dialogue,
        terrain_y,
        damage_finish,
        battle_bgm,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn cmd_play_window_with_record(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    vm_dialogue: bool,
    terrain_y: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
    record_to: Option<RecordTarget>,
) -> Result<()> {
    // Optional cutscene-map override layered on top of the heuristic.
    let cutscene_map = if let Some(p) = cutscene_map_path {
        legaia_engine_core::scene::CutsceneMap::from_toml_path(p)
            .with_context(|| format!("load cutscene map {}", p.display()))?
    } else {
        legaia_engine_core::scene::CutsceneMap::default()
    };
    if cutscene_map_path.is_some() {
        eprintln!(
            "info: cutscene-map loaded with {} explicit entry/entries",
            cutscene_map.len()
        );
    }
    // Auto-resolve op*/ed* cutscene scenes to their MV*.STR file when
    // the user didn't pass --str-file but the extracted root has the
    // file on disk. Mirrors the same convenience path in cmd_play.
    let auto_str = match (str_file, disc) {
        (Some(_), _) => None,
        (None, None) => cutscene_map
            .resolve(scene)
            .map(|rel| extracted_root.join(rel))
            .filter(|p| p.exists()),
        (None, Some(_)) => None,
    };
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());
    // Phase 1: if a STR file is provided (or auto-resolved), play the
    // video in a window first. The user closes (or ESC) the STR window,
    // then the scene window opens.
    //
    // When booting from a disc image we can resolve the scene's movie inside
    // the ISO and play it with its interleaved XA audio (read raw 2352-byte
    // sectors). Otherwise we fall back to the filesystem (video only).
    if let Some(str_path) = resolved_str {
        cmd_play_str(str_path, None, 640, 480)?;
    } else if let (Some(disc_path), None) = (disc, str_file) {
        // Disc mode, no explicit file: resolve the scene's MV*.STR via the
        // cutscene map / heuristic and play it from the disc with audio.
        if let Some(rel) = cutscene_map.resolve(scene) {
            let iso_path = Path::new(&rel);
            match cmd_play_str(iso_path, Some(disc_path), 640, 480) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("info: scene '{scene}' STR '{rel}' not played from disc ({e:#})")
                }
            }
        }
    }

    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio,
    };
    let mut session = match disc {
        Some(disc_path) => BootSession::open_disc(disc_path, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    // Opt-in: drive field dialogue through the inline-script field-VM runner
    // (branch handlers execute). Off by default → identical to before.
    session.host.world.use_vm_dialogue = vm_dialogue;
    // Opt-in: snap the player's Y to the per-scene floor height each
    // locomotion step. Off by default → flat-Y behaviour preserved.
    session.host.world.follow_terrain_height = terrain_y;
    // Opt-in: route live basic-attack damage through the retail damage
    // finisher (9999 cap + no-damage floor). Off by default → flat path.
    session.host.world.use_damage_finish = damage_finish;
    // Field-live arming, built once and reused: at startup for the direct path
    // and later by the boot-UI NEW GAME handler when it enters `opdeene`.
    let field_live_opts = legaia_engine_shell::boot::FieldLiveOpts {
        live_loop,
        player_battle,
        battle_bgm,
    };
    if world_map {
        // Load the scene's resources, route its region-keyed encounter table
        // onto the overworld, install the player, and enter world-map mode
        // (camera controller included). World::tick drives locomotion + the
        // per-tile encounter roll from the pad routed via world.set_pad.
        match session.enter_world_map_live(scene, &field_live_opts) {
            Ok(mode) => {
                log::info!("play-window: entered world-map scene '{scene}' (mode={mode:?})")
            }
            Err(e) => log::warn!("play-window: enter_world_map_live('{scene}') failed: {e:#}"),
        }
        // Start in walk mode so the d-pad walks the overworld player (and the
        // per-tile encounter roll fires). The top-view debug camera (orbit /
        // zoom / pan) stays reachable via the toggle combo (debug_enabled).
        if let Some(ctrl) = session.host.world.world_map_ctrl.as_mut() {
            ctrl.debug_enabled = true;
            ctrl.view_mode = 0;
        }
    }
    if !world_map {
        // Drop into the live field scene (run record 0, install the encounter
        // table, arm the live loop). Shared with the v0.1 oracle + headless
        // drivers via `BootSession::enter_field_live`.
        let opts = field_live_opts.clone();
        match session.enter_field_live(scene, &opts) {
            Ok(mode) => log::info!("play-window: entered field scene '{scene}' (mode={mode:?})"),
            Err(e) => log::warn!("play-window: enter_field_live('{scene}') failed: {e:#}"),
        }
    }

    // Play-window demo seeding (NOT part of the shared field-live core): when
    // a player-driven battle is requested but the boot save carries no items /
    // saved chains, seed a couple so the Item / Arts submenus are exercisable
    // by hand. No-ops when the save already has inventory / chains.
    if player_battle {
        let world = &mut session.host.world;
        if world.inventory.is_empty() {
            world.inventory.insert(0x01, 5); // Healing Leaf
            world.inventory.insert(0x13, 3); // Bomb (offensive)
        }
        if world.saved_chains.is_empty() {
            use legaia_save::SavedChainRecord;
            for slot in 0u8..3 {
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Quick".into(),
                    sequence: vec![1, 2],
                });
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Combo".into(),
                    sequence: vec![1, 2, 3, 4],
                });
            }
            // Stage a demo art record per character so the "Combo" chain
            // (it ends in Up) resolves through the real art-power path -
            // two damage strikes that burn the target. "Quick" has no
            // matching record and falls back to the synthetic profile.
            use legaia_art::power::PowerByte;
            use legaia_art::queue::{ActionConstant, Command};
            use legaia_art::record::EnemyEffect;
            for character in legaia_art::Character::all() {
                world.set_art_record(
                    character,
                    ActionConstant::Art1B,
                    legaia_art::ArtRecord {
                        action: ActionConstant::Art1B,
                        commands: vec![Command::Up],
                        anim_index: 0,
                        anim_extra: vec![],
                        name: None,
                        power: vec![PowerByte::from_byte(0x18), PowerByte::from_byte(0x1D)],
                        dmg_timing: vec![],
                        effect_cues: Default::default(),
                        hit_cues: vec![],
                        identifier: 0,
                        anim_speed: 0,
                        enemy_effect: EnemyEffect::Burned,
                        repeat_frames: Default::default(),
                        background: 0,
                        runtime_address: None,
                    },
                );
            }
        }
    }

    // Apply the cheat file (if any) to the live World before building
    // scene resources. The applier mutates `world.roster` /
    // `world.money` / `world.play_time_seconds` etc. through the
    // ram_map registry.
    if let Some(path) = cheat_file {
        apply_cheat_file(&mut session.host.world, path, cheat_strict)?;
    }

    let scene_res = {
        let s = session
            .host
            .scene
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no scene loaded after BootSession::open"))?;
        // Load the shared blocks (`init_data` + `player_data`) so the
        // player TMD + shared UI atlas stay resident across field
        // transitions, then build with the targeted VRAM-upload
        // heuristic. Without this every prim sampled non-uploaded
        // VRAM regions and the filter dropped 100% of the mesh.
        let mut shared_scenes: Vec<Scene> = Vec::new();
        for name in FIELD_SHARED_BLOCKS {
            match Scene::load(&session.host.index, name) {
                Ok(sc) => shared_scenes.push(sc),
                Err(e) => log::warn!("play-window: shared block '{name}' not loaded: {e:#}"),
            }
        }
        let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
        // Field-load model (matches retail FUN_8001F7C0 + the engine's
        // `enter_field_scene`): `SceneLoadKind::Field` skips the battle-
        // character `scene_tmd_stream` meshes, and the TMD scan now pulls
        // the town's environment geometry out of the scene_asset_table's
        // LZS-packed mesh pack (previously invisible to the raw scanner,
        // which left the field with a single stray battle mesh). Upload
        // every TIM, as retail's field loader DMAs the whole atlas - the
        // town meshes sample texture pages across all of VRAM, so a
        // render-targeted upload drops most of their prims.
        // World-map scenes (`map\d\d`) draw the kingdom-bundle slot-1
        // landmark pack, not the generic field sweep. Mirror the host's
        // `enter_field_scene` kind selection so the rendered meshes match the
        // gameplay-side resources (otherwise the window draws the Field-mode
        // 2-mesh fallback while the host loaded the full 40-TMD pack).
        let load_kind = if legaia_engine_core::scene::is_world_map_scene(scene) {
            SceneLoadKind::WorldMap
        } else {
            SceneLoadKind::Field
        };
        let (res, _stats) = SceneResources::build_targeted_with_options(
            s,
            &shared_refs,
            BuildOptions {
                kind: load_kind,
                upload_all_tims: true,
            },
        )?;
        res
    };
    log::info!(
        "play-window: scene '{}', {} TMDs, {} TIMs",
        scene,
        scene_res.tmds.len(),
        scene_res.tim_count
    );

    let font = Font::load_or_placeholder(extracted_root);

    let mapping = legaia_engine_core::input::Mapping::load_or_default(&std::path::PathBuf::from(
        "legaia-input.toml",
    ));
    // Try to decode the publisher logos from PROT 0895 (init.pak) up
    // front. Falls back silently when the disc isn't loaded or the
    // entry doesn't parse - retail discs always have it.
    let publisher_logos_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::init_pak::PROT_INDEX as u32)
    {
        Ok(b) => match legaia_engine_core::publisher_logos::build_atlas_from_init_pak(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: publisher-logos atlas built ({}x{}, {} logos)",
                    a.width,
                    a.height,
                    a.rects.len()
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: publisher-logos build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0895 read failed: {e:#}");
            None
        }
    };

    // Try to decode the title-screen TIM from PROT 0888 (`sound_data2`
    // per CDNAME, actually carries title art) up front. Falls back
    // silently when the disc isn't loaded or the entry doesn't parse -
    // retail discs always have it.
    let title_screen_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::title_pak::PROT_INDEX_PRIMARY as u32)
    {
        Ok(b) => match legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888(
            &b,
            legaia_asset::title_pak::TITLE_TIM_OFFSET,
        ) {
            Ok(a) => {
                log::info!(
                    "play-window: title-screen atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: title-screen build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0888 read failed: {e:#}");
            None
        }
    };

    // Try to decode the menu-glyph atlas from the unindexed pre-init_data
    // gap in `PROT.DAT` (offset `0x11218`). Carries the small-caps font
    // retail samples for "NEW GAME" / "CONTINUE" menu rows. The
    // per-entry extractor never visits this gap, so we read PROT.DAT
    // raw bytes — see `legaia_asset::menu_glyph_atlas`.
    let menu_glyph_atlas_data = match session.host.index.prot_dat_raw_bytes(
        legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
        legaia_asset::menu_glyph_atlas::TIM_SIZE,
    ) {
        Ok(b) => match legaia_engine_core::menu_glyph_atlas::build_atlas_from_prot_dat_slice(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: menu-glyph atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: menu-glyph build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    // Try to decode the save-menu UI atlas. Needs TWO disc sources:
    //   1. PROT 0899's extended footprint @ `OVERLAY_SAVE_MENU_TIM_OFFSET`
    //      carries the SLOT 1 / SLOT 2 pill sprites (CLUT 7).
    //   2. Raw PROT.DAT @ `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`
    //      carries the 9-slice panel chrome (CLUT row 2).
    // The atlas builder composites both into one 256x256 RGBA atlas;
    // see `crates/engine-core/src/save_menu_atlas.rs`. The 9-slice
    // tile geometry was pinned via `scripts/pcsx-redux/scan_panel_prims.py`
    // against sstate9's RAM dump — every primitive's source u/v + CLUT
    // is byte-pinned to the retail render.
    let save_menu_atlas_data = match (
        session
            .host
            .index
            .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32),
        // Pull a slice that covers BOTH the system-UI sheet (panel
        // chrome, cursor) AND the load-screen portrait + frame TIMs
        // (`OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`..end of
        // `OVERLAY_LOAD_EMPTY_FRAME_TIM`). The slice starts at the
        // system-UI TIM header so existing offsets stay
        // slice-relative; `build_atlas` handles both shapes.
        {
            let base = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
            let end = legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
            session
                .host
                .index
                .prot_dat_raw_bytes(base as u64, end - base)
        },
    ) {
        (Ok(pill_bytes), Ok(panel_bytes)) => {
            match legaia_engine_core::save_menu_atlas::build_atlas(&panel_bytes, &pill_bytes) {
                Ok(a) => {
                    log::info!(
                        "play-window: save-menu atlas built ({}x{}) — 9-slice from PROT.DAT[0x018E0] + pills from PROT 0899",
                        a.width,
                        a.height
                    );
                    Some(a)
                }
                Err(e) => {
                    log::warn!("play-window: save-menu build failed: {e:#}");
                    None
                }
            }
        }
        (Err(e), _) => {
            log::warn!("play-window: PROT 0899 read failed: {e:#}");
            None
        }
        (_, Err(e)) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    let initial_boot_ui = if boot_ui {
        if publisher_logos_atlas_data.is_some() {
            BootUiState::PublisherLogos(
                legaia_engine_core::publisher_logos::PublisherLogosSession::new(),
            )
        } else {
            let snapshots = scan_save_dir(save_dir);
            let any_present = snapshots.iter().any(|s| s.present);
            if any_present {
                BootUiState::Title(legaia_engine_core::title::TitleSession::new())
            } else {
                BootUiState::Title(legaia_engine_core::title::TitleSession::without_save_data())
            }
        }
    } else {
        BootUiState::Inactive
    };
    let mut app = PlayWindowApp {
        session,
        font,
        scene_res: Some(scene_res),
        win: EngineWindow::new(),
        font_atlas: None,
        publisher_logos: None,
        pending_publisher_logos_atlas: publisher_logos_atlas_data,
        title_screen: None,
        pending_title_screen_atlas: title_screen_atlas_data,
        menu_glyphs: None,
        pending_menu_glyph_atlas: menu_glyph_atlas_data,
        save_menu: None,
        pending_save_menu_atlas: save_menu_atlas_data,
        uploaded_vram: None,
        meshes: Vec::new(),
        scene_tmd_data: Vec::new(),
        field_placement_draws: Vec::new(),
        color_meshes: Vec::new(),
        field_placement_color_draws: Vec::new(),
        world_map_terrain_draws: Vec::new(),
        world_map_heightfield: None,
        ocean_anim: None,
        cpu_vram_base: None,
        battle_vram: None,
        battle_tex_slots_used: 0,
        summon_actor_slot: None,
        battle_stage_mesh: None,
        battle_ground_mesh: None,
        prev_scene_mode: None,
        monster_archive: None,
        battle_mesh_base: 0,
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
        prev_pad: 0,
        battle_event_log: std::collections::VecDeque::new(),
        battle_hud: legaia_engine_core::battle_hud::BattleHud::new(),
        pending_dynamic_mesh_slots: Vec::new(),
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
        options_state: legaia_engine_core::options::OptionsState::default(),
        record_log: record_to.map(RecordLog::from_target),
        field_live_opts,
        // In-flow cutscene STR resolves from the extracted root (video only)
        // or, when booting from a disc image, straight from the ISO with its
        // interleaved XA audio. Exactly one of these is set.
        extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
        disc_path: disc.map(|d| d.to_path_buf()),
        cutscene: None,
        cutscene_cam_interp: legaia_engine_render::window::CutsceneCameraInterp::new(),
        active_dialog: None,
    };

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    // After the event loop returns, flush any pending record log. The
    // Escape / CloseRequested handlers also flush proactively so a
    // mid-run crash still produces a partial replay file - the trailing
    // flush is the safety net.
    if let Some(log) = app.record_log.as_mut()
        && let Err(e) = log.flush()
    {
        log::error!("record: flush on exit failed: {e:#}");
    }
    Ok(())
}

/// Walk `save_dir` and build per-slot `SlotSnapshot` entries from any
/// LGSF v1 / v2 files found there. Empty slots produce
/// `SlotSnapshot::empty(slot)`. Up to 8 slots are scanned (the retail
/// PSX memory card supports 15 blocks; engines wishing to scan more can
/// drive their own scanner and feed the result into `SaveSelectSession`).
/// Pluck the lead-character roster index out of a [`SlotSnapshot`] for
/// the load-screen portrait grid. The snapshot already exposes the
/// leader's char_id (scan_save_dir picks it from the parsed
/// [`legaia_save::SaveFile`]); this thin helper exists so render-time
/// call sites read clearly.
fn slot_leader_char_id(snap: &legaia_engine_core::save_select::SlotSnapshot) -> u8 {
    snap.leader_char_id
}

/// Build a per-frame [`legaia_engine_render::SlotInfoView`] for the
/// info panel shown at the bottom of the slot-preview screen.
/// Returns `None` for empty slots (the info panel renders only when
/// a save is present).
fn build_slot_info_view(
    slots: &[legaia_engine_core::save_select::SlotSnapshot],
    cursor_slot: u8,
) -> Option<SlotInfoOwned> {
    let snap = slots.get(cursor_slot as usize)?;
    if !snap.present {
        return None;
    }
    Some(SlotInfoOwned {
        slot_no: snap.slot.saturating_add(1),
        location: snap.location.clone(),
        play_time: snap.play_time_string(),
        leader_name: snap.leader_name.clone(),
        leader_level: snap.party_lv,
        leader_hp: snap.leader_hp,
        leader_mp: snap.leader_mp,
        leader_char_id: snap.leader_char_id,
    })
}

/// Compute the slide-in y-offset (delta from parked y) for the
/// bottom info panel. Mirrors retail FUN_801E08D8's inline
/// `local_34 = (anim_t * -0x100) / 0xFFF >> 12 + 0x18A`: the panel
/// slides from `INFO_PANEL_OFFSCREEN_Y = 394` (off-screen below) up
/// to `INFO_PANEL_PARKED_Y = 138` (parked under load chrome) as
/// `info_panel_slide_anim_t` ramps 0 → 4096. Returns the delta from
/// parked y, so 0 = fully landed.
fn info_panel_slide_offset(session: &legaia_engine_core::save_select::SaveSelectSession) -> i32 {
    let (_, y) = legaia_engine_core::save_select::interpolate_anim(
        (0, legaia_engine_core::save_select::INFO_PANEL_OFFSCREEN_Y),
        (0, legaia_engine_core::save_select::INFO_PANEL_PARKED_Y),
        session.info_panel_slide_anim_t(),
    );
    y - legaia_engine_core::save_select::INFO_PANEL_PARKED_Y
}

/// Owned-string flavour of [`legaia_engine_render::SlotInfoView`] used
/// to keep the strings alive across the render call. The borrowed
/// view referenced by the renderer is taken via [`Self::as_view`].
struct SlotInfoOwned {
    slot_no: u8,
    location: String,
    play_time: String,
    leader_name: String,
    leader_level: u8,
    leader_hp: (u16, u16),
    leader_mp: (u16, u16),
    leader_char_id: u8,
}

impl SlotInfoOwned {
    fn as_view(&self) -> legaia_engine_render::SlotInfoView<'_> {
        legaia_engine_render::SlotInfoView {
            slot_no: self.slot_no,
            location: &self.location,
            play_time: &self.play_time,
            leader_name: &self.leader_name,
            leader_level: self.leader_level,
            leader_hp: self.leader_hp,
            leader_mp: self.leader_mp,
            leader_char_id: self.leader_char_id,
        }
    }
}

fn scan_save_dir(save_dir: &Path) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    use legaia_engine_core::save_select::SlotSnapshot;
    // Scan up to 15 slots (one per retail PSX memory-card block) so
    // the load-screen 5×3 grid can render every potential slot.
    const MAX_SLOTS: u8 = 15;
    let mut out = Vec::with_capacity(MAX_SLOTS as usize);
    for slot in 0..MAX_SLOTS {
        // Saves are written by the field menu via `MenuRuntime` as
        // `<dir>/slot_NN.<SAVE_EXT>` (zero-padded slot, see
        // `menu_runtime::slot_path`). The title-screen and
        // save-select scanners must use the same shape; an earlier
        // mismatch (`slot_N.lgsf`) made every save invisible at boot,
        // greying out Continue even with valid saves on disk.
        let path = save_dir.join(format!("slot_{slot:02}.{SAVE_EXT}"));
        let snap = match std::fs::read(&path).ok().and_then(|b| {
            legaia_save::SaveFile::parse(&b)
                .ok()
                .map(|sf| (b.len(), sf))
        }) {
            Some((_, sf)) => {
                // Read the cumulative XP value from the active-party
                // leader's record (`+0x04..+0x06`, pinned by the captured
                // level-up observation triplet) and infer the level from
                // the retail XP table.
                // Engines that capture the actual level byte can override.
                let leader = sf.party.members.first();
                let lv = leader
                    .map(|r| legaia_save::level_for_cumulative_xp(r.cumulative_xp() as u32))
                    .unwrap_or(1);
                let leader_hp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.hp_cur, v.hp_max)
                    })
                    .unwrap_or((0, 0));
                let leader_mp = leader
                    .map(|r| {
                        let v = r.hp_mp_sp();
                        (v.mp_cur, v.mp_max)
                    })
                    .unwrap_or((0, 0));
                // Retail saves serialise the scene name into the SC
                // block (`+0x200..0x208`, ASCII null-padded). Our LGSF
                // saves don't carry that field yet, so default to the
                // most-common starting kingdom; engines that capture
                // it can override.
                let _ = sf.ext_v2.active_party.is_empty(); // kept-for-future-use
                let location = "Drake Kingdom".to_string();
                SlotSnapshot {
                    slot,
                    present: true,
                    label: format!("Slot {slot}"),
                    play_time_seconds: sf.ext_v2.play_time_seconds,
                    party_lv: lv,
                    location,
                    money: sf.ext.money.max(0) as u32,
                    // Lead char is always Vahn (char_id=0) in retail
                    // Legaia — Vahn is the protagonist and slot 0 of
                    // the SC character record array.
                    leader_char_id: 0,
                    leader_name: "Vahn".to_string(),
                    leader_hp,
                    leader_mp,
                }
            }
            None => SlotSnapshot::empty(slot),
        };
        out.push(snap);
    }
    out
}

// ── STR video player ────────────────────────────────────────────────────────

fn cmd_play_str(
    str_file: &Path,
    disc: Option<&Path>,
    _win_width: u32,
    _win_height: u32,
) -> Result<()> {
    use legaia_engine_shell::cutscene_av::{
        CutsceneAudio, decode_str_av_from_disc, decode_str_video_only,
    };

    // With a disc image the STR is read as raw 2352-byte sectors so its
    // interleaved XA audio track comes along; without one we play the
    // (video-only) Form-1 extract from the filesystem.
    let (frames, timing, audio): (_, _, Option<CutsceneAudio>) = if let Some(disc_path) = disc {
        let (lba, size) = resolve_iso_file(disc_path, str_file)?;
        let count = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let av = decode_str_av_from_disc(disc_path, lba, count)
            .with_context(|| format!("decode STR {} from disc", str_file.display()))?;
        (av.frames, av.timing, av.audio)
    } else {
        let (f, t) = decode_str_video_only(str_file)?;
        (f, t, None)
    };
    if frames.is_empty() {
        anyhow::bail!("no video frames found in {}", str_file.display());
    }
    println!(
        "play-str: {} frames, {}×{}, {:.2} fps, audio: {}",
        frames.len(),
        frames[0].width,
        frames[0].height,
        timing.fps,
        match &audio {
            Some(a) => format!(
                "{:.1} kHz {} ({:.1}s)",
                a.sample_rate as f64 / 1000.0,
                if matches!(a.channels, legaia_xa::Channels::Stereo) {
                    "stereo"
                } else {
                    "mono"
                },
                a.duration_secs()
            ),
            None => "none".into(),
        }
    );

    // Open the audio device only when there is a track to play. A device
    // failure (CI / headless) degrades to wall-clock-paced video, not an error.
    let audio_out = if audio.is_some() {
        match legaia_engine_audio::AudioOut::new() {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("play-str: audio device unavailable ({e:#}); playing video only");
                None
            }
        }
    } else {
        None
    };

    let mut app = StrPlayerApp {
        win: EngineWindow::new(),
        frames,
        uploaded: None,
        frame_period: timing.frame_period(),
        clock: None,
        audio_out,
        pending_audio: audio,
    };
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
    Ok(())
}

/// Resolve an ISO9660 path inside a disc image to its `(lba, size)`. Matches
/// case-insensitively and tolerates a leading slash. Errors if not found.
fn resolve_iso_file(disc_path: &Path, iso_path: &Path) -> Result<(u32, u32)> {
    use legaia_iso::iso9660;
    let want = iso_path
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_uppercase();
    let mut disc = legaia_iso::raw::RawDisc::open(disc_path)
        .with_context(|| format!("open disc {}", disc_path.display()))?;
    let vol = iso9660::read_volume(&mut disc).context("read ISO volume")?;
    let files = iso9660::walk_files(&mut disc, &vol.root).context("walk ISO files")?;
    files
        .into_iter()
        .find(|(p, _)| p.to_ascii_uppercase() == want)
        .map(|(_, rec)| (rec.lba, rec.size))
        .ok_or_else(|| anyhow::anyhow!("{} not found on disc {}", want, disc_path.display()))
}

struct StrPlayerApp {
    win: EngineWindow,
    frames: Vec<legaia_mdec::VideoFrame>,
    uploaded: Option<legaia_engine_render::UploadedTexture>,
    /// Wall-clock duration to hold each frame (from the stream's detected fps).
    frame_period: std::time::Duration,
    /// When playback started; the wall-clock fallback frame index is
    /// `elapsed / frame_period` (used only when no audio track is playing).
    clock: Option<std::time::Instant>,
    /// Live audio output, present only when an interleaved XA track decoded
    /// and the device opened. The video clock reads its cursor for A/V sync.
    /// Owned solely by the player (single-threaded), so no `Arc` is needed.
    audio_out: Option<legaia_engine_audio::AudioOut>,
    /// The decoded audio track, staged into `audio_out` on the first redraw so
    /// the audio cursor and the video start together. Taken once.
    pending_audio: Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
}

impl ApplicationHandler for StrPlayerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.win.open(event_loop, "legaia-engine play-str");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.win.handle_resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                // Stage the audio on the first redraw so its cursor (the video
                // clock) starts when the picture does.
                if let (Some(out), Some(track)) =
                    (self.audio_out.as_ref(), self.pending_audio.take())
                {
                    out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                }
                // A/V sync: drive the visible frame off the audio cursor when a
                // track is playing (audio is the hardware-paced master clock);
                // otherwise pace off wall-clock. Once the due frame passes the
                // last decoded frame, playback is done and the window closes.
                let now = std::time::Instant::now();
                let start = *self.clock.get_or_insert(now);
                let wall = now.duration_since(start).as_secs_f64();
                let audio_secs = self.audio_out.as_ref().and_then(|o| o.xa_cursor_secs());
                let due = legaia_engine_shell::cutscene_av::due_video_frame(
                    audio_secs,
                    wall,
                    self.frame_period.as_secs_f64(),
                );
                if due >= self.frames.len() {
                    if let Some(out) = self.audio_out.as_ref() {
                        out.stop_xa();
                    }
                    event_loop.exit();
                    return;
                }
                if let Some(renderer) = self.win.renderer() {
                    let f = &self.frames[due];
                    match renderer.upload_texture(&f.rgba, f.width, f.height) {
                        Ok(tex) => self.uploaded = Some(tex),
                        Err(e) => log::warn!("upload: {e}"),
                    }
                    if let Some(tex) = &self.uploaded {
                        let _ = renderer.render(RenderTarget::Texture(tex));
                    } else {
                        let _ = renderer.render(RenderTarget::Clear);
                    }
                }
                self.win.request_redraw();
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------
// Battle / Inventory / Equip / GteReplay subcommands
// ---------------------------------------------------------------------

/// Drive a synthetic [`BattleSession`] end-to-end. Reports per-frame
/// session events and the final phase. Intended as a smoke test for the
/// orchestrator wiring; engines that want a full UI use `play-window`
/// (which can host a `BattleSession` via the renderer's HUD draws).
fn cmd_battle(monsters: u8, monster_hp: u16, max_ticks: u64, script: &str) -> Result<()> {
    use legaia_art::Character;
    use legaia_engine_core::ap_gauge::ApGauge;
    use legaia_engine_core::battle_session::{
        BattlePhase, BattleSession, SessionInput, SessionSlotInfo,
    };
    use legaia_engine_core::battle_stats::StatRecord;
    use legaia_engine_core::world::{Actor, World};

    let mut session = BattleSession::new();
    session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let names = ["Vahn", "Noa", "Gala"];
    for (i, name) in names.iter().enumerate() {
        session.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    let monster_count = monsters.min(5);
    for i in 0..monster_count {
        session.set_slot_info(
            3 + i,
            SessionSlotInfo {
                name: format!("Mon{i}"),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: 30,
                    base_udf: 20,
                    base_ldf: 15,
                    base_accuracy: 70,
                    base_evasion: 10,
                    ..Default::default()
                }),
                mp_max: 0,
            },
        );
    }
    session.set_monster_count(monster_count);

    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    for i in 0..3 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.mp = 30;
        world.ap_gauges[i] = ApGauge::with_base(8);
    }
    for i in 0..monster_count as usize {
        world.actors[3 + i].battle.hp = monster_hp;
        world.actors[3 + i].battle.max_hp = monster_hp;
    }

    session.begin_round(&mut world);
    println!(
        "battle: party=3 monsters={} phase={:?}",
        monster_count,
        session.phase()
    );

    let mut script_iter = script.chars();
    let mut total_events = 0usize;
    for tick in 0..max_ticks {
        let mut input = SessionInput::default();
        if let Some(c) = script_iter.next() {
            apply_script_char(c, &mut input);
        }
        let events = session.tick(&mut world, input);
        if !events.is_empty() {
            total_events += events.len();
            for ev in &events {
                println!("[t{tick}] {ev:?}");
            }
        }
        if session.is_done() {
            println!("battle ended at tick {tick}: {:?}", session.phase());
            break;
        }
        if matches!(session.phase(), BattlePhase::Idle) {
            break;
        }
    }
    println!(
        "battle: total_events={} final_phase={:?} hud_active_slots={}",
        total_events,
        session.phase(),
        session.hud.active_slots()
    );
    Ok(())
}

fn apply_script_char(c: char, input: &mut legaia_engine_core::battle_session::SessionInput) {
    use legaia_engine_core::battle_session::SessionInput as SI;
    let _: &SI = input;
    match c {
        'R' => input.right = true,
        'L' => input.left = true,
        'U' => input.up = true,
        'D' => input.down = true,
        'c' => input.cross = true,
        'o' => input.circle = true,
        't' => input.triangle = true,
        's' => input.square = true,
        'S' => input.start = true,
        _ => {}
    }
}

/// Drive a synthetic [`InventoryUseSession`] against a small world.
/// Reports cursor moves + the final outcome.
fn cmd_inventory(item: u8, party_size: u8, script: &str) -> Result<()> {
    use legaia_engine_core::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };
    use legaia_engine_core::items::ItemCatalog;

    let catalog = ItemCatalog::vanilla();
    if catalog.get(item).is_none() {
        anyhow::bail!(
            "item id 0x{item:02X} not in vanilla catalog - pick from 0x10..0x41 or extend the catalog"
        );
    }
    let mut targets: Vec<TargetRow> = Vec::new();
    for i in 0..party_size {
        targets.push(TargetRow::new(i, format!("Slot{i}")).with_stats(50, 100, 10, 30));
    }

    let mut session =
        InventoryUseSession::new(catalog, vec![item], targets, InventoryContext::Field);
    println!("inventory: item=0x{item:02X} party_size={party_size}");
    for (idx, c) in script.chars().enumerate() {
        let input = match c {
            'U' => InventoryUseInput::Up,
            'D' => InventoryUseInput::Down,
            'c' => InventoryUseInput::Confirm,
            'o' => InventoryUseInput::Cancel,
            _ => continue,
        };
        session.input(input);
        let evs = session.drain_events();
        for ev in &evs {
            println!("[s{idx}={c}] {ev:?}");
        }
        if session.is_done() {
            break;
        }
    }
    println!("inventory: state={:?}", session.state);
    Ok(())
}

/// Run an equip session that confirms `item` into `slot`. Useful as a
/// smoke test for the SM and the BattleStats recompute path.
fn cmd_equip(slot: u8, item: u8) -> Result<()> {
    use legaia_engine_core::battle_stats::{
        EquipmentTable, ItemModifier, StatRecord, StatusModifiers,
    };
    use legaia_engine_core::equip_session::{EquipInput, EquipOutcome, EquipSession};
    use std::collections::HashMap;

    let record = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 35,
        base_int: 18,
        equip: [0; 8],
    };
    let mut inv = HashMap::new();
    // Re-encode the item id so its implied slot matches the requested
    // slot - the synthetic test catalog uses `id >> 5` as the slot bits.
    let encoded_id = (slot << 5) | (item & 0x1F);
    inv.insert(encoded_id, 1);
    let mut eq = EquipmentTable::new();
    eq.set(
        encoded_id,
        ItemModifier {
            atk: 10,
            ..Default::default()
        },
    );
    let mut session = EquipSession::new(record, inv, eq, StatusModifiers::default(), Vec::new());

    println!("equip: requested slot={slot} item=0x{item:02X} (encoded 0x{encoded_id:02X})");

    // Drive: down `slot` times to reach the slot, cross to enter picker,
    // cross to confirm item, cross to commit.
    let mut step_count = 0;
    for _ in 0..slot {
        session.input(EquipInput {
            down: true,
            ..Default::default()
        });
        step_count += 1;
    }
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;

    println!(
        "equip: drove {step_count} inputs; outcome={:?}",
        session.outcome()
    );
    if let Some(EquipOutcome::Committed {
        added,
        slot: out_slot,
        removed,
    }) = session.outcome()
    {
        println!("equip: committed slot={out_slot} added=0x{added:02X} removed=0x{removed:02X}");
        println!(
            "equip: post-commit ATK={} (record.equip[{}]=0x{:02X})",
            session.preview_stats.atk,
            out_slot,
            session.record().equip[out_slot as usize]
        );
    }
    Ok(())
}

/// Load a JSON Cop2Trace and replay it through a fresh emulator. Reports
/// any per-step register divergence; exits 0 on clean replay.
fn cmd_gte_replay(trace_path: &Path, verbose: bool) -> Result<()> {
    use legaia_engine_render::gte_trace::Cop2Trace;
    let bytes = std::fs::read(trace_path)
        .with_context(|| format!("read trace file {}", trace_path.display()))?;
    let json = std::str::from_utf8(&bytes).context("trace file is not valid UTF-8")?;
    let trace = Cop2Trace::read_json(json).context("parse trace JSON")?;
    println!(
        "gte-replay: loaded {} steps (label={})",
        trace.len(),
        trace.label.as_deref().unwrap_or("<none>")
    );
    let mismatches = trace.replay();
    if mismatches.is_empty() {
        println!("gte-replay: clean - every step replayed bit-exact");
        if verbose {
            println!("gte-replay: trace label = {:?}", trace.label);
        }
        return Ok(());
    }
    eprintln!(
        "gte-replay: {} step(s) diverged from the recorded snapshot",
        mismatches.len()
    );
    for m in &mismatches {
        eprintln!("  step {} ({}):", m.step, m.op);
        for f in &m.fields {
            eprintln!(
                "    {} expected={} actual={}",
                f.field, f.expected, f.actual
            );
        }
    }
    anyhow::bail!("trace replay produced mismatches");
}

/// Map an input letter to a [`legaia_engine_core::title::TitleInput`] mask.
fn title_input_for(c: char) -> legaia_engine_core::title::TitleInput {
    use legaia_engine_core::title::TitleInput;
    let mut i = TitleInput::default();
    match c {
        's' => i.start = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

fn cmd_title(script: &str, no_save: bool, fade_frames: u16) -> Result<()> {
    use legaia_engine_core::title::{TitleEvent, TitleSession};
    let mut s = if no_save {
        TitleSession::without_save_data()
    } else {
        TitleSession::new()
    };
    s.fade_in_frames = fade_frames;
    s.skip_fade_in();
    println!("title: starting (no_save={no_save})");
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(title_input_for(ch));
        for e in evs {
            match e {
                TitleEvent::CursorMoved { row } => println!("  tick {i}: cursor → {row}"),
                TitleEvent::StartPressed => println!("  tick {i}: start pressed"),
                TitleEvent::MenuConfirmed { row } => println!("  tick {i}: confirmed row {row}"),
                TitleEvent::NewGameSelected => println!("  tick {i}: NewGame"),
                TitleEvent::ContinueSelected => println!("  tick {i}: Continue"),
                TitleEvent::OptionsSelected => println!("  tick {i}: Options"),
                TitleEvent::FadeInDone => println!("  tick {i}: fade-in done"),
            }
        }
    }
    println!("title: outcome = {:?}", s.outcome());
    Ok(())
}

/// Build the flat tiled battle ground grid (retail's `func_0x801d02c0` grass
/// grid): a `(N+1)x(N+1)` vertex grid of quads on the PSX `y=0` plane centred at
/// the world origin, every vertex sampling the stage dome's **grass texel** so
/// it reads as real grass from the battle VRAM instead of the bare clear colour.
/// Returns `None` if the dome has no ground-plane (`|y|` small) textured vertex
/// to borrow the texel from. Drawn with the actor camera so the party stands on
/// it; coarse cells are fine because every vertex samples the same texel.
fn build_battle_ground_grid(
    dome: &legaia_tmd::mesh::VramMesh,
) -> Option<legaia_tmd::mesh::VramMesh> {
    // Borrow the dome's GRASS texture, targeting the exact tile retail's grid
    // (func_0x801d02c0) uses. `mednafen-state prim-trace` on the real map01
    // battle shows those ground tiles at uv ~ (132..140, 2..13) with their own
    // CBA/TSB. PROT 88 is the same TMD, so the dome's grass vertices carry the
    // same UVs - find the flat ground vertex nearest that tile centre, take its
    // CBA/TSB, and tile that small window. (Earlier picks - "first textured
    // vertex", "largest XZ area + bbox centre" - landed on the ground-mist
    // object or a 2-tone checker region of the texture, hence the checkerboard.)
    const GU0: u8 = 132;
    const GU1: u8 = 140;
    const GV0: u8 = 2;
    const GV1: u8 = 13;
    let tcu = ((GU0 as u16 + GU1 as u16) / 2) as i32;
    let tcv = ((GV0 as u16 + GV1 as u16) / 2) as i32;
    let best = (0..dome.positions.len())
        .filter(|&i| dome.positions[i][1].abs() < 5.0 && dome.cba_tsb[i] != [0, 0])
        .min_by_key(|&i| {
            let [u, v] = dome.uvs[i];
            (u as i32 - tcu).pow(2) + (v as i32 - tcv).pow(2)
        })?;
    let cba_tsb = dome.cba_tsb[best];
    let [bu, bv] = dome.uvs[best];
    log::info!(
        "battle ground grid: grass tile uv [{GU0}..{GU1}]x[{GV0}..{GV1}] cba_tsb={cba_tsb:?} (nearest dome vert uv=({bu},{bv}))"
    );
    let (u0, u1, v0, v1) = (GU0, GU1, GV0, GV1);

    const N: i32 = 28; // cells per side (retail func_0x801d02c0 grid)
    const P: f32 = 512.0; // retail func_0x801d02c0 cell pitch (0x200) -> ~+/-16384 extent
    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
    };
    // Per-cell quads (own 4 vertices each) so EVERY cell maps to the same full
    // grass UV tile `[u0..u1]x[v0..v1]`. Shared-vertex grids forced a single UV
    // per vertex, which (alternating box corners by parity) made adjacent cells
    // sample different texture columns -> green-vs-dirt whole-cell jumps. With
    // each cell carrying the whole tile, the grass repeats uniformly.
    let half = N / 2;
    for iz in 0..N {
        for ix in 0..N {
            let (x0, z0) = ((ix - half) as f32 * P, (iz - half) as f32 * P);
            let (x1, z1) = (x0 + P, z0 + P);
            let base = m.positions.len() as u32;
            for (x, z, u, v) in [
                (x0, z0, u0, v0),
                (x1, z0, u1, v0),
                (x0, z1, u0, v1),
                (x1, z1, u1, v1),
            ] {
                m.positions.push([x, 0.0, z]);
                m.uvs.push([u, v]);
                m.cba_tsb.push(cba_tsb);
                m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y (flat ground faces up)
            }
            m.indices
                .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    Some(m)
}

fn select_input_for(c: char) -> legaia_engine_core::save_select::SelectInput {
    use legaia_engine_core::save_select::SelectInput;
    let mut i = SelectInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'L' => i.left = true,
        'R' => i.right = true,
        _ => {}
    }
    i
}

fn cmd_save_select(mode: &str, slots: &str, script: &str) -> Result<()> {
    use legaia_engine_core::save_select::{
        SaveSelectMode, SaveSelectSession, SelectEvent, SlotSnapshot,
    };
    let mode = match mode.to_ascii_lowercase().as_str() {
        "load" => SaveSelectMode::Load,
        "save" => SaveSelectMode::Save,
        other => anyhow::bail!("unknown save-select mode: {other}"),
    };
    let snapshots: Vec<SlotSnapshot> = slots
        .split(',')
        .enumerate()
        .map(|(i, p)| {
            let present = p.trim() == "1";
            if present {
                SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    label: format!("Slot {i}: Vahn  Lv 5"),
                    play_time_seconds: 1234,
                    party_lv: 5,
                    location: "Town01".into(),
                    money: 100,
                    leader_char_id: 0,
                    leader_name: "Vahn".into(),
                    leader_hp: (100, 100),
                    leader_mp: (20, 20),
                }
            } else {
                SlotSnapshot::empty(i as u8)
            }
        })
        .collect();
    let mut s = SaveSelectSession::new(mode, snapshots);
    println!(
        "save-select: mode={:?}, {} slot(s)",
        s.mode(),
        s.slots().len()
    );
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(select_input_for(ch));
        for e in evs {
            match e {
                SelectEvent::CursorMoved { slot } => {
                    println!("  tick {i}: cursor → slot {slot}")
                }
                SelectEvent::EnteredConfirm { slot, kind } => {
                    println!("  tick {i}: entered {:?} confirm on slot {slot}", kind)
                }
                SelectEvent::Confirmed { slot, kind } => {
                    println!("  tick {i}: confirmed {:?} on slot {slot}", kind)
                }
                SelectEvent::ConfirmCancelled { slot, kind } => {
                    println!("  tick {i}: cancelled {:?} on slot {slot}", kind)
                }
                SelectEvent::InvalidConfirm => println!("  tick {i}: invalid confirm"),
                SelectEvent::EnteredNowChecking { slot } => {
                    println!("  tick {i}: entered NowChecking on slot {slot}")
                }
                SelectEvent::EnteredSlotPreview { slot } => {
                    println!("  tick {i}: entered SlotPreview on slot {slot}")
                }
                SelectEvent::LoadConfirmed { slot } => {
                    println!("  tick {i}: load confirmed on slot {slot}")
                }
                SelectEvent::SlotPreviewCancelled { slot } => {
                    println!("  tick {i}: slot preview cancelled on slot {slot}")
                }
                SelectEvent::Cancelled => println!("  tick {i}: cancelled"),
            }
        }
    }
    println!("save-select: outcome = {:?}", s.outcome());
    Ok(())
}

fn cmd_encounter(rate: u8, steps: u32, seed: u32) -> Result<()> {
    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("test_scene");
    table.set_trigger_rate(rate);
    table.push(EncounterEntry::new(1, 50));
    table.push(EncounterEntry::new(2, 30));
    table.push(EncounterEntry::new(3, 20));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    let mut rng = seed;
    let mut hit_step = None;
    for step in 0..steps {
        // xorshift32
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        if session.on_step(rng) {
            hit_step = Some(step);
            break;
        }
    }
    if let Some(s) = hit_step {
        // Drain through transition.
        for _ in 0..session.transition_frames + 1 {
            session.tick_frame();
        }
        if let Some(roll) = session.drain_triggered() {
            println!(
                "encounter: triggered at step {s} → formation {} (roll q8={})",
                roll.formation_id, roll.roll_q8
            );
        } else {
            println!("encounter: triggered at step {s} but transition lost");
        }
    } else {
        println!("encounter: no trigger after {steps} step(s)");
    }
    println!(
        "encounter: total_steps={} steps_since_last={}",
        session.tracker().total_steps(),
        session.tracker().steps_since_last_battle()
    );
    Ok(())
}

fn picker_input_for(c: char) -> legaia_engine_core::target_picker::PickerInput {
    use legaia_engine_core::target_picker::PickerInput;
    let mut i = PickerInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

fn cmd_target_pick(kind: &str, actor: u8, script: &str) -> Result<()> {
    use legaia_engine_core::target_picker::{
        PickerEvent, SlotState, TargetKind, TargetPickerSession,
    };
    let kind = match kind.to_ascii_lowercase().as_str() {
        "enemy" => TargetKind::SingleEnemy,
        "ally" => TargetKind::SingleAlly,
        "ally-or-self" => TargetKind::SingleAllyOrSelf,
        "dead-ally" => TargetKind::DeadAlly,
        "any-ally" => TargetKind::AnyAlly,
        "all-enemies" => TargetKind::AllEnemies,
        "all-allies" => TargetKind::AllAllies,
        "self" => TargetKind::Self_,
        other => anyhow::bail!("unknown target kind: {other}"),
    };
    let party = [SlotState::alive(true, true); 3];
    let monsters = [SlotState::alive(true, true); 5];
    let mut s = TargetPickerSession::new(kind, actor, party, monsters);
    println!("target-pick: kind={:?} actor={actor}", s.kind());
    for ch in script.chars() {
        if s.is_done() {
            break;
        }
        s.input(picker_input_for(ch));
        for e in s.drain_events() {
            match e {
                PickerEvent::CursorMoved { row, slot } => {
                    println!("  cursor → {:?} slot {slot}", row)
                }
                PickerEvent::RowSwitched { row, slot } => {
                    println!("  row switched → {:?} slot {slot}", row)
                }
                PickerEvent::Confirmed { row, slot } => {
                    println!("  confirmed {:?} slot {slot}", row)
                }
                PickerEvent::SweepConfirmed { row } => {
                    println!("  sweep confirmed {:?}", row)
                }
                PickerEvent::Cancelled => println!("  cancelled"),
                PickerEvent::InvalidConfirm => println!("  invalid confirm"),
            }
        }
    }
    println!("target-pick: outcome = {:?}", s.outcome());
    Ok(())
}

fn editor_input_for(c: char) -> legaia_engine_core::tactical_arts_editor::EditInput {
    use legaia_engine_core::tactical_arts_editor::EditInput;
    let mut i = EditInput::default();
    match c {
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'n' => i.name_next = true,
        _ => {}
    }
    i
}

fn cmd_chain_editor(char_slot: u8, script: &str) -> Result<()> {
    use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary, EditEvent};
    let lib = ChainLibrary::new();
    let mut ed = ChainEditor::new(char_slot, &lib);
    println!("chain-editor: char_slot={char_slot}");
    for ch in script.chars() {
        if ed.is_done() {
            break;
        }
        for e in ed.tick(editor_input_for(ch)) {
            match e {
                EditEvent::BrowseCursorMoved { row } => println!("  cursor → row {row}"),
                EditEvent::EnteredEdit { editing_slot } => {
                    println!("  entered edit slot={:?}", editing_slot)
                }
                EditEvent::SequenceAppended { command, len } => {
                    println!("  appended {:?} (len={len})", command)
                }
                EditEvent::SequencePopped { len } => println!("  popped (len={len})"),
                EditEvent::InvalidCommit { len } => println!("  invalid commit at len {len}"),
                EditEvent::EnteredNaming => println!("  entered naming"),
                EditEvent::Saved { slot } => println!("  saved slot {slot}"),
                EditEvent::Replaced { slot } => println!("  replaced slot {slot}"),
                EditEvent::Deleted { slot } => println!("  deleted slot {slot}"),
                EditEvent::Cancelled => println!("  cancelled"),
            }
        }
    }
    println!("chain-editor: outcome = {:?}", ed.outcome());
    Ok(())
}

fn cmd_seru_capture(seru: u16, count: u32, party: &str) -> Result<()> {
    use legaia_engine_core::seru_learning::{SeruCaptureLog, SeruRegistry, record_capture};
    let registry = SeruRegistry::retail();
    let party: Vec<u8> = party
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();
    let mut log = SeruCaptureLog::new();
    println!("seru-capture: seru={seru} count={count} party={:?}", party);
    for i in 0..count {
        let out = record_capture(&registry, &mut log, seru, &party);
        if !out.accepted {
            println!("  capture {i}: rejected (unknown seru)");
            return Ok(());
        }
        if !out.learns.is_empty() {
            for ev in &out.learns {
                println!(
                    "  capture {i}: char {} learned spell {:#04x} from seru {}",
                    ev.char_slot, ev.spell_id, ev.seru_id
                );
            }
        }
    }
    println!(
        "seru-capture: final per-char totals: {:?}",
        party
            .iter()
            .map(|c| (*c, log.total_points(*c)))
            .collect::<Vec<_>>()
    );
    for c in &party {
        println!("  char {c} learned spells: {:?}", log.learned_spells(*c));
    }
    Ok(())
}

#[cfg(test)]
mod battle_camera_tests {
    use super::PlayWindowApp;
    use glam::Vec4;
    use std::f32::consts::TAU;

    /// `retail_battle_mvp` must reproduce the exact retail overworld-battle
    /// projection `screen = H*(Rx(32u)*Ry(yaw)*v + (0,1280,7680))/Ze`, H=256,
    /// PSX +Y down, screen-centre (160,120) over 320x240 — pinned from the
    /// `overworld_battle_bg_angle_*` saves + `FUN_80026988`. Disc-free: pure
    /// math. Guards the glam matrix construction against regression.
    #[test]
    fn retail_battle_mvp_matches_psx_projection() {
        // Hand-rolled retail projection (the savestate-verified reference).
        fn handrolled(v: [f32; 3], yaw_u: f32) -> Option<(f32, f32)> {
            let yaw = yaw_u / 4096.0 * TAU;
            let pitch = 32.0 / 4096.0 * TAU;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let ry = [cy * v[0] + sy * v[2], v[1], -sy * v[0] + cy * v[2]];
            let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
            let ez = e[2] + 7680.0;
            if ez <= 1.0 {
                return None;
            }
            Some((
                256.0 * e[0] / ez + 160.0,
                256.0 * (e[1] + 1280.0) / ez + 120.0,
            ))
        }
        // Sample several world points and orbit angles (4:3 so aspect_fix == 1).
        let mvp_aspect = 4.0 / 3.0;
        for &yaw_u in &[0.0f32, 224.0, 1024.0, 2632.0, 3136.0, 3808.0] {
            let mvp = PlayWindowApp::retail_battle_mvp(yaw_u / 4096.0 * TAU, mvp_aspect);
            for &v in &[
                [1000.0f32, -500.0, 3000.0],
                [-2000.0, 0.0, 6000.0],
                [0.0, -3000.0, -800.0],
                [5000.0, 12.0, 5000.0],
            ] {
                // Engine draws `cam * model` with `model` carrying the Y-flip F,
                // so the dome sample is `cam * F * v_psx` == flip v.y first.
                let clip = mvp * Vec4::new(v[0], -v[1], v[2], 1.0);
                if clip.w <= 1.0 {
                    continue;
                }
                let ndc = (clip.x / clip.w, clip.y / clip.w);
                let sx = 160.0 + ndc.0 * 160.0;
                let sy = 120.0 - ndc.1 * 120.0; // NDC up+ -> PSX screen down+
                if let Some((hx, hy)) = handrolled(v, yaw_u) {
                    let d = ((sx - hx).powi(2) + (sy - hy).powi(2)).sqrt();
                    assert!(
                        d < 0.05,
                        "yaw={yaw_u} v={v:?}: {d}px off ({sx},{sy} vs {hx},{hy})"
                    );
                }
            }
        }
    }
}
