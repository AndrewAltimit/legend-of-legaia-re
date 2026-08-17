# scripts/

Helper scripts for the two project tracks: developer/CI maintenance, Ghidra
overlay analysis, asset reverse-engineering, and emulator-driven runtime
capture. This is a **map** of the layout - each script carries its own usage
header (`--help` or a top-of-file comment block).

A map that omits things is worse than no map, because a reader treats absence
as evidence: this page once described itself as the layout while leaving out a
hard pre-commit gate. Every gate and every committed data artifact is listed
below. One-off probes under `asset-investigation/` are grouped by subject
rather than enumerated, and that is the only place a name may be missing.

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
| `recomp/` | Static-recomp differential-oracle tooling; see [`#recomp`](#recomp) below and [`docs/tooling/recomp-differential.md`](../docs/tooling/recomp-differential.md). |
| `git-hooks/` | The shipped `pre-commit` hook (installed via `ci/install-hooks.sh`). |
| [`lib/`](#lib) | Sourced bash helpers shared by the shell scripts: process control that cannot match the caller, and run-and-capture that reports the real exit code. |
| `engine/` | Engine-side `scenarios.toml` for the determinism replay harness (distinct from the capture manifest above). |
| `replays/` | `j-replay-v1` record/replay fixtures for the determinism tests. |

### ci/

Run from the repo root; the pre-commit hook (`git-hooks/pre-commit`) and CI
invoke them by `scripts/ci/<name>` path.

Where each gate runs, and why a few are in only one place, is
[`docs/tooling/host-drift.md`](../docs/tooling/host-drift.md#where-the-gates-run).
The short version: hook and CI mirror each other, except that a gate whose
input is gitignored (the Ghidra dump corpus, `extracted/`) can only have teeth
in the hook, and must self-skip everywhere else.

- `install-hooks.sh` - point `core.hooksPath` at `git-hooks/` (run once per clone).
- `install-tools.sh` - install the local toolchain (Ghidra container, capstone, emulators).
- `check-doc-density.py` - doc legibility-density gate (long lines / over-budget table cells).
- `check-md-links.py` - Markdown intra-repo link + heading-anchor gate (the docs-side sibling of `check-site-links.py`).
- `check-site-links.py` - static-site internal-link + anchor gate.
- `check-site-generated-freshness.py` - generated `site/**/*.html` vs its `_content` fragment (subdirectories included): every asset the fragment references and every element `id` it declares must reach the generated page. Catches a `_content` edit that was never re-rendered, which the link gate cannot see because the defect is a *missing* reference, not a broken one - a page script reading `.checked` off an id the served page never grew is this shape.
- `check-site-doc-mirrors.py` - hard gate: a `site/_content/` page that restates a `docs/` page must claim every `##` heading of its source through a `data-doc` attribute. `_gen.py` never reads `docs/`, so a mirror drops whole areas of its source while still building and still passing the link gate. See [`docs/tooling/site-shell.md`](../docs/tooling/site-shell.md).
- `check-port-tags.py` - `// PORT:` / `// REF:` tag drift checker (warn-only in the hook).
- `check-port-provenance.py` - asks whether a `// PORT:` address names the right
  routine, which no other gate does. Ranked worklist off the disassembly; not in
  the hook. See [`docs/tooling/port-provenance.md`](../docs/tooling/port-provenance.md).
- `disc-coverage.py` (+ `disc-coverage-baseline.json`) - **hard gate**: coverage measured against the disc's own bytes rather than against our citations, ratcheted against a committed baseline. Self-skips without `extracted/` and the dump corpus, so it also runs in CI as a SKIPPED step. See [`docs/tooling/disc-coverage.md`](../docs/tooling/disc-coverage.md).
- `update-progress-metrics.py` - refresh `progress-metrics.json` from `disc-coverage.py` + `port-catalog.py`. Run locally on a machine with disc data and commit the result; the site build has none and can only render what is committed.
- `progress-metrics.json` - that committed output, read by `site/_gen.py` for the landing page.
- `check-shell-observer-traps.py` - hard gate over the shell corpus for the three "observer inside the observed" defects (pipe exit status, self-matching `pkill`/`pgrep`, `grep`'s no-match exit 1). Self-tests its detectors on every run. See [`docs/tooling/shell-observer-traps.md`](../docs/tooling/shell-observer-traps.md).
- `check-ui-host-drift.py` (+ `ui-host-drift-waivers.toml`) - hard gate: every `engine-ui` draw builder must reach both hosts, paired host geometry constants must carry equal values, and paired simulation injection sites must name the same kernels. See [`docs/tooling/host-drift.md`](../docs/tooling/host-drift.md).
- `check-trait-override-symmetry.py` (+ `trait-override-waivers.toml`) - hard gate: an `engine-core` trait whose methods all carry default bodies lets a host forget a hook with no compile error, so every host implementing one must override the same set. Same page.
- `port-catalog.py` (+ `port-catalog-ignore.toml`, `features.toml`, `port-catalog-baseline.json`) - per-function port worklist, `--dashboard`, and a `--check` ratchet over the worklist / ported counts. Hard gate in the hook; a SKIPPED step in CI, since the `dumped` column reads the gitignored dump corpus.
- `proposed-ignore-additions.toml` - generated by `ghidra-analysis/classify-worklist.py`, consumed by no gate. **Review row by row before merging any of it into `port-catalog-ignore.toml`** - the file's own header says why.
- `function-coverage.py` - Ghidra-dump citation coverage report.
- `build-wasm.sh` - web-viewer WASM build. `site/wasm/` is **not committed**; run this once to browse `site/` locally. It also stamps `site/wasm/SOURCE_STAMP.json` (untracked).
- `check-wasm.sh` - local WASM build smoke; `--full` verifies the stamp. **Not wired into anything** - CI builds `-p legaia-web-viewer --target wasm32-unknown-unknown` inline instead. Run it by hand.
- `check-wasm-freshness.py` - does your locally built `site/wasm/` bundle still match this tree's sources? Content-addressed, because mtime and `git log` reasoning both return false "in sync" answers. **Advisory**, in CI and in the hook alike: it never fails a build, because the bundle is untracked output the deploy job rebuilds. See [`docs/tooling/shipped-bundle-freshness.md`](../docs/tooling/shipped-bundle-freshness.md).
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
- `dump_header.py` - the ONE parser for a dump's `size=` / `entry=` header, imported by every instrument over the corpus. Each used to carry its own regex, and since the corpus spells each field several ways, each silently rejected a different subset of real dumps.
- `check-dump-stat-drift.py` - **hard gate**: committed prose quoting a dump statistic the dump no longer reports. A stale caveat ("the dump is empty, do not port this") suppresses work with no trace that it did.
- `check-dump-base-integrity.py` (+ `dump-base-baseline.json`) - **hard gate** (`--check`): a dump whose printed addresses are a constant delta from where its bytes actually live. `--shape` is the second axis (truncation / headerless), `--audit-dumpers` the third (which dump scripts still carry the defect).
- `check-jal-target-integrity.py` (+ `jal-target-baseline.json`) - **hard gate** (`--check`): a decoded call target landing on no function entry, which localises a byte window whose link base is unrecovered.
- `attribute-dump-extents.py` -> `dump-extent-attribution.csv` - which extracted image really holds each VA-ambiguous dump extent, decided by bytes. Committed, and consumed by `ci/disc-coverage.py`; without it every overlay extent stays ambiguous by address.
- `classify-worklist.py` -> `worklist-classification.csv` + `ci/proposed-ignore-additions.toml` - is a worklist address a portable function entry at all (`REAL` / `INTERIOR` / `DUPLICATE` / `VA_ALIASED` / ...). Both outputs are committed and neither is gated, so an empty one means "unrun" just as readily as "clean" - re-run before citing it.
- `resolve-phantom-va.py` - byte-level owner resolution for a printed VA against named candidate readings; picks up the short bodies and data regions the base-integrity sweep declines to judge.
- `locate-entry-image.py` - which based overlay image actually holds a worklist address's function entry, from disc bytes (stack-frame prologue + in-image `jal` sites). Disambiguates the VA aliasing at the shared `0x801CE818` / `0x801F69D8` bases; prints both signals rather than a verdict, because leaf entries have no frame and jump-table / SCUS-called entries have no in-overlay `jal`.
- `find-address-word-refs.py` - who references an address, in **all five** forms at once: literal LE word (function-pointer table / actor-template slot), `lui`+`addiu`/`ori` materialisation, `jal`, `j`, PC-relative branch. Sweeps SCUS, the based overlay images and (`--prot`) the raw bytes of every extracted PROT entry, so "no caller" becomes a statement about the disc rather than about one tool's blind spot. `--range` answers "who references this table", `--home` marks the branch hits a slot sibling contributes at the shared base. See [`docs/tooling/address-reference-scan.md`](../docs/tooling/address-reference-scan.md).

See [`docs/tooling/ghidra.md`](../docs/tooling/ghidra.md) and
[`docs/tooling/static-overlay-pipeline.md`](../docs/tooling/static-overlay-pipeline.md).

### asset-investigation/

Disc-asset RE probes. `decode_slot4_subbodies.py`, `slot4_to_obj.py`, and
`slot4_topdown_png.py` borrow disc helpers from `pcsx-redux/` via `sys.path`.

**Run these from the repo, not from a scratch directory.** `sys.path[0]` is the
running script's own directory, so any `.py` sitting beside a script shadows a
module of that name for every import beneath it - including imports a dependency
makes internally. A stray helper dropped next to a script has already presented
as `capstone` failing with a circular-import "partially initialized module",
reproducible only from that directory. Same shape as the self-matching observers
in [`shell-observer-traps.md`](../docs/tooling/shell-observer-traps.md): the tool
is inside what it is measuring.

- TIM/TMD: `build_tim_review.py` / `apply_tim_review.py`, `montage_tims.py`, `scan_tims_and_match_prot.py`, `find_large_tmd_packs.py`, `render_battle_char_true.py`, `render-unplaced-tmds.py`, `verify_battle_char_pack.py`.
- World-map / slot-4: `decode_slot4_subbodies.py`, `slot4_to_obj.py`, `slot4_topdown_png.py`, `classify_dat_8007c018.py`, `extract-world-placements.py`, `analyze_world_map_vm_log.py` (the live-RAM GPU-tile variant `analyze-walk-ground-tiles.py` lives in `ghidra-analysis/`).
- Scene / font / naming / save: `scene-asset-detect.py`, `find-font-carrier.py`, `cdname_shift_analysis.py`, `match_title_staging_to_prot.py`, `find_save_offsets.py`.
- Battle / story-flag sweeps: `audit_module_hp_stores.py` (stores to the battle-actor HP triple across the capture-class spell modules), `flag_helper_call_sweep.py` (every disc-wide `jal`/`j` into the story-flag SET/CLEAR/TEST helpers, with the `a0` operand classified constant vs computed), `battle-heap-walk.py` (walks the custom 2-pool heap's free + allocated rings offline in a main-RAM extract from either emulator's save state; see the heap-budget section of `docs/subsystems/battle.md`).
- Overlay disasm: `overlay_disasm.py <overlay.bin> <base_va_hex> [start_va_hex [n]]` - linear MIPS32-LE disassembler over an as-loaded overlay `.bin` (from `asset overlay extract`); decodes per-word so embedded data emits `.word` instead of halting the sweep. Whole-file dump (grep target) or a windowed function view.

### recomp/

Static-recomp differential-oracle tooling. Traces are Sony-derived and are
never committed. Pairs with `legaia-engine sim-trace`; the reference is
[`docs/tooling/recomp-differential.md`](../docs/tooling/recomp-differential.md).

- `probe.py` - TCP debug-server client + CLI, with the protocol's traps baked in.
- `preflight.py` - tells a self-wiping runtime, a stale build and a stale snapshot apart *before* a capture runs, so a capture failure is not silently read as a divergence.
- `apply_boot_state_fix.py` - reapplies the savestate-resume fix to a fresh runtime checkout.
- `trace_capture.py` / `trace_diff.py` - frame-tagged canonical JSONL capture, and the per-channel first-divergence report over two of them.
- `audio_note_capture.py` / `note_diff.py` - the same pair one level down, over note events read off the recomp's SPU rather than over frame state: key-on with ADPCM address, pitch, per-voice volume and ADSR, key-off, loop edges.
- `xa_cue_capture.py` - per-art CD-XA voice-shout cues, captured through the arts-voice selector and the XA clip starter.
- `minigame_warp.py` - warp a live instance into a mode-24 minigame through the field VM's op-`0x3E` door-warp arm (which names no scene and makes no call - it writes SCUS globals and lets the mode dispatcher do the rest).
- `test_*.py` - synthetic-fixture unit tests for the diff/preflight/warp/capture logic; no runtime and no disc needed.

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
