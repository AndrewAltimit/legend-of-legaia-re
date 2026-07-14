# Engine determinism + scripted-input replay

The engine ships a record/replay loop that captures per-frame pad input
to a `.toml` file and plays it back deterministically. The same input
file run twice produces a bit-identical state trace - that property is
asserted by a disc-free regression test so any future change that
introduces non-determinism fails CI.

Three pieces:

| Component | Lives in | Role |
|---|---|---|
| `j-replay-v1` schema | [`legaia_engine_shell::replay`](../../crates/engine-shell/src/replay.rs) | TOML format + parser/writer/validator |
| Determinism gate | [`crates/engine-shell/tests/determinism_j2.rs`](../../crates/engine-shell/tests/determinism_j2.rs) | Disc-free cargo-test; runs in CI |
| `legaia-engine replay` / `record` | [`crates/engine-shell/src/bin/legaia-engine.rs`](../../crates/engine-shell/src/bin/legaia-engine.rs) | Headless playback + interactive capture |

## File format (`j-replay-v1`)

```toml
[meta]
schema = "j-replay-v1"
scenario = "title_attract"        # optional; resolves into scripts/scenarios.toml
rng_seed = 0xDEADC0DE              # initial RNG seed (battle_formulas PsyQ PRNG)
frames = 600                       # total frame count

# Pad-mask transitions. Sparse: only frames where the bitmask changes
# are stored. The dense per-frame stream has length `frames + 1` and
# is reconstructed by `ReplayFile::expand_pad_stream` - the mask in
# slot N is the mask in force on frame N.
[[event]]
frame = 0
pad = 0x0000

[[event]]
frame = 42
pad = 0x4000                       # Cross pressed

[[event]]
frame = 44
pad = 0x0000                       # released

# Optional regression fixture. Each row constrains the recorded
# engine trace at a specific frame; the comparison is by `frame`
# value, not slice index. `active_scene = None` means don't-care.
[[expected]]
frame = 0
scene_mode = "Title"

[[expected]]
frame = 600
scene_mode = "Field"
active_scene = "town01"
```

Pad bits match
[`legaia_engine_core::input::PadButton::mask`](../../crates/engine-core/src/input.rs):
Cross = `0x4000`, Circle = `0x2000`, Up = `0x0010`, Down = `0x0040`,
Left = `0x0080`, Right = `0x0020`, etc. Stored as a plain `u16` so the
on-disk wire form stays byte-readable.

`ReplayFile::validate` rejects schema mismatches, out-of-order events,
and frame indices past `meta.frames` at parse time. Writers MUST emit
events in frame-ascending order; readers don't sort.

## Subcommands

### `legaia-engine replay`

Drives a synthetic `World` from a replay file and emits the per-frame
mode trace as JSONL (the same shape as `legaia-engine mode-trace`):

```
legaia-engine replay --input my.replay.toml [--out trace.jsonl] [--strict]
```

The synthetic driver mirrors the determinism-gate harness: `World::new`
+ an 8-slot actor pool, RNG seeded from `meta.rng_seed`, ticked once
per replay frame. No disc required.

`--strict` exits non-zero on the first divergence between the recorded
trace and the file's `[[expected]]` fixture. Without it, divergence is
printed to stderr but the command succeeds.

### `legaia-engine record`

Thin wrapper over `play-window` with a pad-capture hook armed:

```
legaia-engine record --out my.replay.toml [--scene town01] [--scenario LABEL] [--rng-seed 0xDEADC0DE]
```

Every keyboard transition that changes the pad mask is appended to a
`RecordLog` hanging off `PlayWindowApp`. Escape, window-close, and
event-loop drop all flush a `j-replay-v1` file to the configured
output - a mid-session close still produces a usable replay. Auto-repeat
deduplication collapses a stream of identical-mask press events to a
single `PadEvent`.

The file's `meta.frames` reflects the actual recorded duration
(highest `session.frames` observed during the run), so playback of the
captured file replays exactly as long as the human session was.

**Interactive-toggle caveat.** `j-replay-v1` captures the pad stream
only - the play-window's interactive camera/movement toggles (camera
distance preset, left-mouse drag-orbit, the precise-movement toggle)
are not recorded. The defaults are safe: the distance preset and orbit
are pure render framing (no simulation effect), and replays run with
`precise_movement` off (the retail-faithful quantised remap), matching
the engine-core defaults. A session recorded while precise movement was
ON (or with a non-zero drag-orbit compass) is not replay-stable; keep
the toggles at their defaults when capturing replay fixtures.

## Determinism gate

[`crates/engine-shell/tests/determinism_j2.rs`](../../crates/engine-shell/tests/determinism_j2.rs)
is the load-bearing regression check. It drives a synthetic `World`
twice through the same `ReplayFile` and asserts the per-frame state
trace bytes are bit-identical between runs.

The state digest covers:

- `frame` - wall-clock counter from `World::frame`
- `scene_mode` - matches `ModeTraceFrame::scene_mode`
- `pad` - the mask in effect on this frame (from the dense replay stream)
- `rng_state` - PsyQ PRNG running state, the single most important
  drift signal
- `money`, `party_hp_total`, `dialog_active` - structural gameplay state

Three companion tests double-lock the gate's coverage: a different pad
stream produces a different trace (input dimension is observed), a
different RNG seed produces a different trace (seed dimension is
observed), and an `[[expected]]` fixture round-trips through
`ReplayFile::diff` so the regression-comparison side stays honest.

Runs in CI without `LEGAIA_DISC_BIN`.

## Composition with the other oracles

The replay format is a peer to the existing parity gates:

- [`vram_oracle_e1`](../../crates/engine-shell/tests/vram_oracle_e1.rs)
  compares engine VRAM against retail mednafen captures (byte-exact in
  the texpage region).
- [`mode_trace_e3`](../../crates/engine-shell/tests/mode_trace_e3.rs)
  compares engine `(scene_mode, active_scene)` per frame against retail
  snapshots.
- `determinism_j2` compares engine traces against *themselves*, no
  retail capture required - the disc-free side of the parity stack.

Recorded replays bind a scenario label in their `meta.scenario` field,
so a captured session can be paired back to its retail starting state
via [`scripts/scenarios.toml`](../../scripts/scenarios.toml). Future
work pairs `record` + `replay` with E1/E3 to produce identical engine
traces from canonical inputs.

The [`v0_1_playthrough`](../../crates/engine-shell/tests/v0_1_playthrough.rs)
oracle composes these: a disc-free determinism gate plus a disc-gated
convergence gate. Its engine driver is
`mode_trace_oracle::build_engine_mode_trace_field_live`, which calls
[`BootSession::enter_field_live`] so the engine drives a cold boot into
the scenario's field scene (run record 0, install the encounter table,
arm the live loop) instead of sitting in `Title`. Phase 1 asserts the
engine reaches `Field`, the replay `[[expected]]` Field rows hold, the
retail mode-trace converges, and an SC round-trip on the post-Field world
is byte-identical. The scripted-encounter Battle leg is deferred (see the
"Scripted Tetsu encounter → Battle" row in
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

## See also

- [`docs/subsystems/engine.md`](../subsystems/engine.md) - the clean-room engine the record/replay loop drives.
- [`docs/subsystems/script-vm.md`](../subsystems/script-vm.md) - the field/event VM whose pad-driven state the trace captures.
