# Recomp differential oracle

A frame-tagged differential harness between the static recomp of the retail
game and the clean-room engine. Both sides emit the same canonical JSONL
state-trace shape in retail units, so a per-channel diff pinpoints the first
frame where the engine's camera pose, player position, or an NPC's heading
departs from retail behaviour - the measurement layer for parity work on
story-event cameras, NPC facing, and battle-scene framing.

Four moving parts:

| Component | Lives in | Role |
|---|---|---|
| `probe.py` | [`scripts/recomp/`](../../scripts/recomp/probe.py) | Client library + CLI for the recomp's TCP debug server, protocol traps baked in |
| `trace_capture.py` | [`scripts/recomp/`](../../scripts/recomp/trace_capture.py) | Frame-tagged capture of named RAM address maps into the canonical JSONL |
| `legaia-engine sim-trace` | [`crates/engine-shell`](../../crates/engine-shell/src/sim_trace.rs) | The engine side: ticks a `BootSession` and emits the same JSONL in retail units |
| `trace_diff.py` | [`scripts/recomp/`](../../scripts/recomp/trace_diff.py) | Aligns two traces and reports the first divergence per channel |

**Captured traces are Sony-derived** (retail game RAM values) and must stay
untracked - like every capture artifact in this repo, they live under a
scratch directory, never in git. The synthetic fixtures in
`scripts/recomp/test_trace_diff.py` are the only committed trace-shaped data.

## Contents

- [The recomp side](#the-recomp-side)
- [Protocol traps the client bakes in](#protocol-traps-the-client-bakes-in)
- [Canonical trace shape](#canonical-trace-shape)
- [Capturing a recomp trace](#capturing-a-recomp-trace)
- [The engine side: `legaia-engine sim-trace`](#the-engine-side-legaia-engine-sim-trace)
- [Diffing](#diffing)
- [See also](#see-also)

## The recomp side

The recomp runtime exposes a JSON-over-newline TCP debug server (one JSON
object per line; `{"id":N,"cmd":"..."}` in, `{"id":N,"ok":true,...}` out).
`probe.py` wraps it as a library (`RecompClient`) and a small CLI:

```bash
python3 scripts/recomp/probe.py --port 4494 ping
python3 scripts/recomp/probe.py --port 4494 read 0x8007050C 8
python3 scripts/recomp/probe.py --port 4494 press cross 30
python3 scripts/recomp/probe.py --port 4494 load-state 4 --expect-mode 0x15
python3 scripts/recomp/probe.py --port 4494 hot --top 64 --out hot.json
python3 scripts/recomp/probe.py --port 4494 vram-peek 0 0 256 4
python3 scripts/recomp/probe.py --port 4494 launch --cache-dir /tmp/mycache --wait-tcp
python3 scripts/recomp/probe.py kill <PID>
```

Environment contract: `LEGAIA_RECOMP_DIR` points at the recomp workspace
(binary at `build-dbg/Legend_of_Legaia_Recompiled`, `game.toml` at the
root), `LEGAIA_RECOMP_BIOS` at a PSX BIOS image, `LEGAIA_RECOMP_PORT`
sets the default port. All three are overridable per-invocation with flags.
The recomp workspace itself is a separate untracked tree - nothing from it
is committed here.

Headless instances (`--headless --debug-port N`) serve TCP in tens of
seconds and cannot screenshot ("display disabled") - the harness is
structural reads only, which is exactly what the differential needs.

## Protocol traps the client bakes in

Every one of these is verified against a live server; scripts that bypass
`probe.py` re-discover them the hard way.

- **One request per connection.** The server closes the TCP connection
  after *every* response. `RecompClient.call` opens a fresh connection per
  request; a hand-rolled persistent-socket client sees its second request
  die with a broken pipe.
- **`press` wants `buttons` + `frames`.** The button field is named
  `buttons` (a wrong key is silently ignored) and carries a raw
  **active-low** SIO pad word: Cross `0xBFFF`, Circle `0xDFFF`, Start
  `0xFFF7` (not `0xF7FF`), Up `0xFFEF`, Down `0xFFBF`, Left `0xFF7F`.
  `probe.BUTTON_WORDS` derives the full set from the SIO bit layout. Use
  >= 30-frame holds for confirms - short presses are silently dropped on
  some screens.
- **`press` is non-blocking.** It returns before the hold elapses;
  `RecompClient.press` sleeps the hold out by default so back-to-back
  presses don't overwrite each other.
- **`savestate` load is staged.** The server acks `{"ok":true}` and the
  load executes at the next block boundary, unwinding the guest and
  dropping every TCP connection - sometimes *after* a quick reconnect
  already succeeded. `RecompClient.load_savestate` retries the
  verification reads through reconnects and then verifies the state took:
  scene name (8 bytes at `0x8007050C`) and game mode (u16 at
  `0x8007B83C`). Always pass `--expect-scene` / `--expect-mode` when known;
  a stale slot loads "successfully" into the wrong state.
- **`pause` / `step` / `run_to_frame` are REMOVED.** The debug server
  returns an error explaining the migration: observation goes through ring
  buffers, not synchronous stepping. The frame-exact primitive is the
  per-frame snapshot ring - `set_snapshot` (4 slots x 128 bytes, recorded
  every frame into a 36000-frame ring) + `read_frame_ram` (read a region
  *as of a specific frame*). `trace_capture.py`'s ring engine is built on
  it.
- **`vram_peek` clamps to 128 px wide** per call; `RecompClient.vram_peek`
  chunks wider rects.
- **`dirty_exec_hot` PCs are KSEG-masked physical.** OR `0x80000000`
  before resolving against overlay VAs (`probe.hot_pc_to_va`).
- **Kill by PID, never by pattern.** `pkill -f <pattern>` matches your own
  shell's command line when the pattern appears in it. `probe.py launch`
  prints the PID; `probe.py kill <PID>` signals exactly that process.
- **One port, one instance.** Two instances launched on the same port
  silently share it and fake responses; `launch` refuses to start when the
  port already answers.

## Canonical trace shape

One JSON object per line; every field except `frame` is optional per line
(absent = not captured that frame):

```json
{"frame": 32915, "scene": "jou ene", "mode": 21,
 "cam": {"pitch": 32, "yaw": 2408, "roll": 0, "h": 256,
         "eye": [0, 1280, 7920], "focus": [0, 0, 0]},
 "player": {"x": 100, "z": 200, "heading": 0},
 "actors": [{"i": 1, "x": -40, "z": 80, "heading": 1024}]}
```

Units are retail on both sides: angles in the PSX 12-bit space (4096 = full
turn, masked to `0xFFF`), positions in retail world units, `mode` in the
retail game-mode word space (`_DAT_8007B83C`).

The recomp-side address map is the pinned retail globals (provenance:
[`memory-map.md`](../reference/memory-map.md),
[`field-locomotion.md`](../subsystems/field-locomotion.md),
[`cutscene.md`](../subsystems/cutscene.md) for the camera trio the op-`0x45`
apply handler `FUN_801DE084` writes): camera rotation trio u16 at
`0x8007B790/92/94`, GTE projection `H` at `0x8007B6F4`, eye trio i32 at
`0x800840B8`, focus trio i32 at `0x80089118`, player pointer at
`0x8007C364` (`+0x14` X, `+0x18` Z, `+0x26` heading), scene name at
`0x8007050C`, game mode at `0x8007B83C`.

## Capturing a recomp trace

```bash
python3 scripts/recomp/trace_capture.py --port 4494 \
    --savestate 4 --expect-mode 0x15 \
    --frames 100 --map camera --out /tmp/scratch/recomp_cam.jsonl
```

Built-in maps: `camera` (rotation trio + H + eye + focus), `player`
(position + heading through the player pointer), `scene` (scene name +
mode). `--actors` adds a configurable per-actor sweep -
`base=0x...,kind=records|ptrs,stride=0x...,count=N` plus per-entry field
offsets (defaults = the player-record layout) - so lanes can trace NPC
headings without touching the script.

Two capture engines (`--engine auto|ring|poll`):

- **ring** - frame-exact. Configures per-frame snapshot regions and reads
  them back per frame from the ring after the window elapses. Every sample
  sits on the same frame boundary. Budget: 4 regions x 128 bytes - the
  `camera` map alone uses all 4, which is why `camera`+`player` together
  fall back to polling.
- **poll** - best-effort live loop tagged with the frame counter. Skipped
  frames are absent lines (the diff aligns on frame numbers, not indices);
  a sample can straddle a frame boundary. Required for actor sweeps
  (pointer chases) and maps past the region budget.

Capture guidance: right after a savestate load the camera can sit static
for a settle window (the battle idle auto-orbit resumes a few seconds in) -
a capture that needs motion should start after the settle, or capture
longer and let alignment find the first change.

## The engine side: `legaia-engine sim-trace`

```bash
LEGAIA_DISC_BIN=... # or --disc / --extracted-root
legaia-engine sim-trace --scene town01 --disc "$LEGAIA_DISC_BIN" \
    --frames 100 --out /tmp/scratch/engine_town01.jsonl
```

Boots a `BootSession` on the scene, drops into a live field scene
(`enter_field_live` - field VM, locomotion, and camera events armed the way
the windowed host arms them; `--no-field-live` samples the plain
`load_scene` boot state instead), ticks `--frames` sim frames, and emits
`frames + 1` canonical records. Per-tick emission of existing sim state
only - the subcommand adds no simulation features.

Retail-unit mapping (see [`sim_trace.rs`](../../crates/engine-shell/src/sim_trace.rs)
module docs for the full table): the camera controller's radians convert
back to 12-bit units (the exact inverse of the op-`0x45` decode); roll and
H are read raw from the last Camera Configure payload (the engine camera
models neither); player/actor samples come from `move_state.world_x/z` +
`render_26`. `mode` maps `SceneMode` onto the retail game-mode word (Field
3, WorldMap 13, Battle 21 / `0x15`, Menu 23 / `0x17`, Cutscene 27) and is
omitted for modes with no retail equivalent (Title, minigame sessions) so
a diff flags them as absent rather than faking a match.

## Diffing

```bash
python3 scripts/recomp/trace_diff.py /tmp/scratch/recomp_cam.jsonl \
    /tmp/scratch/engine_town01.jsonl --tol-angle 2 --tol-pos 2
```

Alignment: `--offset N` (trace B frame = trace A frame + N), defaulting to
auto-alignment on the **first camera change** in each trace - the two sides
boot from arbitrary frame counters, but the first scripted camera cut is
the same event on both. Falls back to aligning first frames when neither
trace has a camera change.

Comparison is per-channel over the aligned overlap (`cam.yaw`,
`player.x`, `actors[2].heading`, `scene`, `mode`, ...). Angle channels use
4096-wraparound distance; position channels absolute distance; `scene` /
`mode` compare exactly. Channels present on only one side are reported and
skipped, not counted as divergence. For each divergent channel the report
shows the FIRST divergent frame with a +/-5-frame context window of both
sides' values; the exit status is non-zero when anything diverged.

`scripts/recomp/test_trace_diff.py` (pure python, synthetic fixtures)
locks the alignment + wraparound + tolerance semantics:

```bash
cd scripts/recomp && python3 -m unittest test_trace_diff
```

## See also

- [`determinism-replay.md`](determinism-replay.md) - the engine-vs-itself
  side of the parity stack; this page is the engine-vs-retail-recomp side.
- [`pcsx-redux-automation.md`](pcsx-redux-automation.md) /
  [`mednafen-automation.md`](mednafen-automation.md) - emulator-based
  retail observation (breakpoints / save-state diffing); the recomp path
  adds cheap frame-tagged structural reads at full speed.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) - the
  pinned retail globals the address maps read.
