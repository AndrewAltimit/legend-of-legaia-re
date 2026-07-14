//! Clap command-line interface: the `Cli` root parser and the `Cmd` /
//! `ConfigCmd` subcommand enums (help text doubles as the user-facing docs).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "legaia-engine",
    about = "Top-level driver for the Legaia clean-room engine. Boots a CDNAME scene from extracted PROT bytes."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Cmd {
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
    /// town01 the survey finds no inline literal - the opening Tetsu fight
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
        /// Operate on the scene's standalone **variant** MAN carried by this
        /// PROT extraction entry (the type-3 chunk of a DATA_FIELD streaming
        /// entry in the scene's block, e.g. `157` for rikuroa's post-Caruban
        /// story-state MAN) instead of the asset-table bundle MAN. Use the
        /// census output's `PROT[NNNN] VARIANT-MAN` tag to find the index.
        #[arg(long)]
        variant: Option<u32>,
        /// Walk a partition's records as field-VM scripts and report their
        /// flag write/test sites - both the scratchpad global-flag ops
        /// (`GFLAG_SET`/`GFLAG_CLEAR`) and the wide SYSTEM-flag ops
        /// (`0x50..=0x7F`, SET/CLEAR/TEST). Partition 2 holds the
        /// cutscene-timeline records (e.g. opdeene's `GFLAG_SET 26` town01
        /// hand-off arm). Reported at real opcode boundaries.
        #[arg(long)]
        gflag_partition: Option<usize>,
        /// Run a disc-wide SYSTEM-flag census across *every* CDNAME scene's
        /// MAN (all partitions) and print `flag -> [(scene, partition, record,
        /// op, kind)]` sorted by flag number. Surfaces the setters for the
        /// overworld progress gates (e.g. `0x193`/`0x482`/`0x2FC`), which
        /// usually live in a different scene's MAN than the one that gates on
        /// them. Ignores the per-scene `--scene` argument (it is disc-wide).
        #[arg(long)]
        system_flag_census: bool,
        /// Run a disc-wide **motion-VM** flag census: decode every scene
        /// MAN's tail-section-1 motion scripts (the `FUN_80038158` bytecode
        /// `FUN_8003A9D4` installs on actors at scene entry) and print every
        /// op-`0x07` SET / op-`0x08` CLEAR of the `DAT_80085758` system-flag
        /// bank, plus the per-stream variant gate flags. This is the second
        /// bytecode VM writing the same bank - `--system-flag-census` (field
        /// VM ops `0x50/0x60/0x70`) is structurally blind to it. Disc-wide;
        /// ignores the per-scene `--scene` argument.
        #[arg(long)]
        motion_flag_census: bool,
        /// Run a disc-wide **op-0x49 flag-WINDOW census**: decode every scene
        /// MAN's partition records with the field-VM disassembler and print
        /// every op-`0x49` (`STATE_RESUME`) site with its operand bytes
        /// interpreted under the `FUN_801EF014` flag-window descriptor
        /// (`+1` count, `+2` default, `+3` rows, `+4..5` u16 base flag), plus
        /// containment / near-miss verdicts for the spine flags
        /// `0x142`/`0x482`/`0x1BE`/`0x225`. This closes the "a window's
        /// base+offset arithmetic lands on a spine flag with no literal in
        /// the corpus" hypothesis. Disc-wide; ignores `--scene`.
        #[arg(long)]
        op49_window_census: bool,
        /// Dump the inline cutscene-narration text pages embedded in a
        /// cutscene-timeline record (the `0x1F`/`0x00`-framed subtitle pages
        /// after a `0x4C` narration op). Pair with `--disasm-record N
        /// --disasm-partition 2` to target the right record; defaults to
        /// scanning every partition-2 record when no record is given.
        #[arg(long)]
        narration: bool,
        /// Print every partition-2 record's C1/C2 header gate lists (the
        /// `FUN_8003BDE0` spawn conditions: C1 blocks the spawn if ANY listed
        /// flag is set - the one-shot latch; C2 requires ALL listed flags
        /// set) plus the record's name bytes. This is the record-HEADER flag
        /// read surface the inline op censuses are structurally blind to -
        /// the gate-family mining view (`partition2_record_gates`).
        #[arg(long)]
        p2_gates: bool,
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
    /// No disc required - the replay binds an RNG seed and a
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
        /// Present-party composition: comma-separated character names
        /// (vahn/noa/gala/terra) or 0-based roster indices, in battle
        /// order - e.g. `--party noa,terra`. Battle ordinal i renders in
        /// VRAM texture band i with member i's own player-file mesh /
        /// palette / spells (the live-verified retail banding rule).
        /// Caps at the 3 on-screen positions. Default: the save's
        /// composition (identity Vahn/Noa/Gala when none is recorded).
        #[arg(long)]
        party: Option<String>,
        /// Fall back to the simplified typewriter for field NPC dialogue. By
        /// default dialogue runs through the inline-script field-VM runner so
        /// branch handlers execute their flag-sets / scene-changes (Up/Down
        /// navigate a menu, Cross confirms); pass this to disable that and use
        /// the plain panel with no branch execution.
        #[arg(long, default_value_t = false)]
        simple_dialogue: bool,
        /// Disable terrain-following: keep the player's Y flat instead of
        /// snapping each field locomotion step to the floor-height sample
        /// (`FUN_80019278`) at the new tile. Floor-snap is the retail
        /// behaviour and the default; pass this to get the old flat-Y walk.
        /// No effect on the world-map walk.
        #[arg(long, default_value_t = false)]
        flat_y: bool,
        /// Block field walking with retail's three-probe leading-edge wall
        /// footprint (`FUN_801cfe4c`'s `DAT_801f2214` table): the player rests
        /// ~47 units off a wall plane exactly like retail instead of walking
        /// up to it. Off by default (candidate-centre test).
        #[arg(long, default_value_t = false)]
        edge_collision: bool,
        /// Make field NPCs solid with retail's actor-collision probes
        /// (`FUN_801cfc40`'s `DAT_801f21b4` table): walking into an NPC's
        /// body box blocks the step, as in retail. Off by default
        /// (walk-through). Placed PROPS (doors, cupboards) are solid
        /// unconditionally - retail keeps them in the collision candidate
        /// list until their script's `31 00` exempts them - so this flag
        /// only gates the NPC arm.
        #[arg(long, default_value_t = false)]
        solid_npcs: bool,
        /// Animate field NPCs: drive each placement's authored walk route
        /// (its script's `0x4C 0x51` move-to-tile ops) through the motion VM
        /// so villagers patrol like retail. Off by default (NPCs rest at
        /// their placement anchors). Pairs well with `--solid-npcs` (the
        /// moving NPC's collision box follows its live position).
        #[arg(long, default_value_t = false)]
        live_npcs: bool,
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
        /// Headless-ish screenshot: render offscreen and write a PNG of the
        /// framebuffer at `--screenshot-tick`, then exit. Deterministic - no
        /// `scrot` screen-scrape. Combine with `--pad-script` to auto-open a
        /// menu first. The window still opens (a real wgpu surface is needed)
        /// but the captured pixels come from an offscreen readback.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// World-tick at which `--screenshot` captures (default 120). Ticks
        /// advance at the fixed 100 Hz sim rate, so the same script + tick
        /// reproduces the same frame run-to-run.
        #[arg(long, default_value_t = 120)]
        screenshot_tick: u64,
        /// Periodic screenshot sweep: capture a PNG every N ticks into
        /// `--screenshot-dir` (named `tick_%05d.png`). Composable with
        /// `--pad-script`; the run exits after the capture at
        /// `--screenshot-last-tick` (or when the window closes). Independent
        /// of the single-shot `--screenshot`.
        #[arg(long)]
        screenshot_every: Option<u64>,
        /// Output directory for `--screenshot-every` captures (required with
        /// it; created if missing).
        #[arg(long)]
        screenshot_dir: Option<PathBuf>,
        /// Last world-tick of a `--screenshot-every` sweep: exit after the
        /// capture at/past this tick. Omit to run until the window closes.
        #[arg(long)]
        screenshot_last_tick: Option<u64>,
        /// Scripted pad input for a screenshot run: `TICK:BUTTON` pairs,
        /// comma-separated, e.g. `--pad-script "30:Start,50:Down,50:Down,70:Cross"`.
        /// Each entry presses BUTTON for exactly the named tick (a one-tick
        /// edge); a `FIRST-LAST:BUTTON` entry HOLDS the button across the
        /// inclusive tick range (e.g. `10-200:Up` walks the player). BUTTON
        /// names match the pad buttons (Start/Cross/Circle/Up/Down/Left/
        /// Right/...). Replaces `xdotool` for menu navigation.
        #[arg(long)]
        pad_script: Option<String>,
        /// Seed the New Game starting party (Vahn from the SCUS template) at
        /// boot so the pause menu's Status / party screens show real content -
        /// name / LV / HP·MP / stat grid / XP - instead of an empty roster.
        /// This resets story flags / money / inventory to a fresh New Game.
        /// Matches the retail early-game single-Vahn party.
        #[arg(long, default_value_t = false)]
        seed_party: bool,
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
pub(crate) enum ConfigCmd {
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
