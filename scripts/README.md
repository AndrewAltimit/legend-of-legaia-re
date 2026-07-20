# scripts/

Helper scripts for the two project tracks: developer/CI maintenance, Ghidra
overlay analysis, asset reverse-engineering, and emulator-driven runtime
capture. This is a **map** of the layout - each script carries its own usage
header (`--help` or a top-of-file comment block).

Two files stay at this top level because they are operational entry points
referenced by code, not analysis one-offs:

| File | Role |
|---|---|
| `scenarios.toml` | The save-state / capture **scenario manifest** (`ScenarioManifest`). Hard-wired as a default path in `legaia-engine` and the disc-/library-gated oracle tests, so it lives at a stable location. |
| `manage-states.py` | Curates the save-state catalogue that `scenarios.toml` indexes (list / fingerprint / import mednafen + PCSX states). |

## Layout

| Directory | Scope |
|---|---|
| [`ci/`](#ci) | Repo-maintenance gates and build/install helpers the pre-commit hook and CI run. |
| [`ghidra-analysis/`](#ghidra-analysis) | Static analysis: overlay extraction + import into Ghidra, MIPS/GTE disassembly, GPU-packet and call-graph tooling. |
| [`asset-investigation/`](#asset-investigation) | One-off RE probes over disc assets: TIM/TMD review + render, slot-4 / world-map decode, scene/font/CDNAME/save-format hunts. |
| `pcsx-redux/` | PCSX-Redux Lua probe library (`lib/probe`) + `autorun_*.lua` capture scripts + Python decoders, driven by `run_probe.sh` (`run_probe.ps1` on Windows). See [`docs/tooling/pcsx-redux-automation.md`](../docs/tooling/pcsx-redux-automation.md); [`COMMUNITY-CAPTURE.md`](pcsx-redux/COMMUNITY-CAPTURE.md) is the hand-out guide for volunteer playthrough captures. |
| [`vrc-diorama/`](#vrc-diorama) | Clean-room MIDI transport for the VRChat live battle-diorama: register schema + codegen, the Lua encoder/sink riding the battle-state probe, and the UdonSharp decoder scaffold. No Sony bytes. |
| `mednafen/` | Mednafen save-state automation: capture, diff, bisect, bulk-terrain resolve, plus `movies/` for optional `.mcm` input recordings. See [`docs/tooling/mednafen-automation.md`](../docs/tooling/mednafen-automation.md). |
| `recomp/` | Static-recomp differential-oracle tooling: `probe.py` (TCP debug-server client + CLI, protocol traps baked in), `trace_capture.py` (frame-tagged canonical JSONL capture), `trace_diff.py` (per-channel first-divergence report) + its synthetic-fixture unit test. Pairs with `legaia-engine sim-trace`. See [`docs/tooling/recomp-differential.md`](../docs/tooling/recomp-differential.md). |
| `git-hooks/` | The shipped `pre-commit` hook (installed via `ci/install-hooks.sh`). |
| [`lib/`](#lib) | Sourced bash helpers shared by the shell scripts: process control that cannot match the caller, and run-and-capture that reports the real exit code. |
| `engine/` | Engine-side `scenarios.toml` for the determinism replay harness (distinct from the capture manifest above). |
| `replays/` | `j-replay-v1` record/replay fixtures for the determinism tests. |

### ci/

Run from the repo root; the pre-commit hook (`git-hooks/pre-commit`) and CI
invoke them by `scripts/ci/<name>` path.

- `install-hooks.sh` - point `core.hooksPath` at `git-hooks/` (run once per clone).
- `install-tools.sh` - install the local toolchain (Ghidra container, capstone, emulators).
- `check-doc-density.py` - doc legibility-density gate (long lines / over-budget table cells).
- `check-md-links.py` - Markdown intra-repo link + heading-anchor gate (the docs-side sibling of `check-site-links.py`).
- `check-site-links.py` - static-site internal-link + anchor gate.
- `check-port-tags.py` - `// PORT:` / `// REF:` tag drift checker (warn-only in the hook).
- `check-shell-observer-traps.py` - hard gate over the shell corpus for the three "observer inside the observed" defects (pipe exit status, self-matching `pkill`/`pgrep`, `grep`'s no-match exit 1). Self-tests its detectors on every run. See [`docs/tooling/shell-observer-traps.md`](../docs/tooling/shell-observer-traps.md).
- `port-catalog.py` (+ `port-catalog-ignore.toml`, `features.toml`) - per-function port worklist + `--dashboard`.
- `function-coverage.py` - Ghidra-dump citation coverage report.
- `build-wasm.sh` / `check-wasm.sh` - web-viewer WASM build + CI smoke.
- `setup-cross-toolchain.sh` - provision one release target's cross toolchain (rustup std, zig + `cargo-zigbuild`, the amd64 ALSA sysroot); idempotent, root-free except mingw-w64, which it only checks for. See [`docs/tooling/releases.md`](../docs/tooling/releases.md).
- `release-build.sh` - build + package one release target into `target/dist` (archive + `.sha256`). Driven per target by `.github/workflows/release.yml`.

### lib/

Sourced, not executed:

```bash
source "$(git rev-parse --show-toplevel)/scripts/lib/proc.sh"
```

- `proc.sh` - `proc_kill_tree` / `proc_spawn_group` / `proc_group_alive` /
  `proc_kill_group` / `proc_wait_pid` replace `pkill -f` and `pgrep -f`, which
  match the caller's own command line; `run_capture` replaces
  `cmd | tail && echo OK`, which reports the tail's exit status rather than the
  command's; `grep_count` / `grep_found` keep `grep`'s no-match exit 1 from
  aborting a script under `set -e`. Rationale and the failure history:
  [`docs/tooling/shell-observer-traps.md`](../docs/tooling/shell-observer-traps.md).

### ghidra-analysis/

Static-overlay and code-analysis tooling. Some scripts import siblings as
modules (`disasm-overlay-fn.py` → `mips_gte`; `find-addprim-emitters.py` /
`analyze-walk-ground-tiles.py` → `gpu_packets`), which is why they share this
directory.

- `extract-mednafen-overlay.py` / `extract-duckstation-overlay.py` - slice a runtime overlay out of a save state.
- `analyze-overlay.sh` / `import-overlay-named.sh` / `bulk-import-overlays.sh` / `sweep-overlays.sh` - extract → import-into-Ghidra pipelines (`overlays*.spec` drive the sweep).
- `auto-name-overlay.py` - auto-label an imported overlay.
- `disasm-overlay-fn.py` + `mips_gte.py` - capstone MIPS disassembly with COP2/GTE annotation.
- `gpu_packets.py` + `find-addprim-emitters.py` + `analyze-walk-ground-tiles.py` - PSX GPU-primitive decode + emitter/ground-tile analysis.
- `call-graph.py` / `scan_funcs_for_addr_range.py` - call-graph + address-range scans over the Ghidra dumps.

See [`docs/tooling/ghidra.md`](../docs/tooling/ghidra.md) and
[`docs/tooling/static-overlay-pipeline.md`](../docs/tooling/static-overlay-pipeline.md).

### asset-investigation/

Disc-asset RE probes. `decode_slot4_subbodies.py`, `slot4_to_obj.py`, and
`slot4_topdown_png.py` borrow disc helpers from `pcsx-redux/` via `sys.path`.

- TIM/TMD: `build_tim_review.py` / `apply_tim_review.py`, `montage_tims.py`, `scan_tims_and_match_prot.py`, `find_large_tmd_packs.py`, `render_battle_char_true.py`, `render-unplaced-tmds.py`, `verify_battle_char_pack.py`.
- World-map / slot-4: `decode_slot4_subbodies.py`, `slot4_to_obj.py`, `slot4_topdown_png.py`, `classify_dat_8007c018.py`, `extract-world-placements.py`, `analyze_world_map_vm_log.py` (the live-RAM GPU-tile variant `analyze-walk-ground-tiles.py` lives in `ghidra-analysis/`).
- Scene / font / naming / save: `scene-asset-detect.py`, `find-font-carrier.py`, `cdname_shift_analysis.py`, `match_title_staging_to_prot.py`, `find_save_offsets.py`.
- Overlay disasm: `overlay_disasm.py <overlay.bin> <base_va_hex> [start_va_hex [n]]` - linear MIPS32-LE disassembler over an as-loaded overlay `.bin` (from `asset overlay extract`); decodes per-word so embedded data emits `.word` instead of halting the sweep. Whole-file dump (grep target) or a windowed function view.

### vrc-diorama/

The transport layer that carries live battle state into a VRChat world for the
diorama feature. It rides on the battle-state probe (`pcsx-redux/lib/probe/battle_state.lua`)
and is otherwise self-contained; its own [`README.md`](vrc-diorama/README.md) is the
reference. No Sony bytes - wire-protocol structure only.

- `register_schema.toml` - single source of truth for the MIDI register protocol.
- `codegen.py` - emits `generated/registers.lua` (encoder) + the UdonSharp
  `Registers.cs` (decoder); `--check` is a pre-commit drift gate.
- `midi_encoder.lua` / `midi_sink.lua` - `BattleState` -> CC messages (MSB-first,
  commit-latched) -> ALSA `snd-virmidi` device. Driven by
  `pcsx-redux/autorun_battle_midi_stream.lua`.
- `setup-virmidi.sh` / `verify-virmidi.sh` - one-time virtual-port setup + a
  no-VRChat end-to-end loopback check.
- `test_*.lua` - offline encoder/sink/round-trip validation (run with `luajit`).
- `world-project/` - drop-in VRChat world assets (UdonSharp decoder + `.meta` +
  Windows VCC setup guide).
