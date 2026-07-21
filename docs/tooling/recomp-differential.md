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
- [The savestate-resume fix](#the-savestate-resume-fix)
- [Protocol traps the client bakes in](#protocol-traps-the-client-bakes-in)
- [Canonical trace shape](#canonical-trace-shape)
- [Capturing a recomp trace](#capturing-a-recomp-trace)
- [Driving the game to an uncovered scene](#driving-the-game-to-an-uncovered-scene)
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
seconds. They **can** screenshot: `{"cmd":"screenshot","path":"..."}`
writes a 24-bit BMP of the guest display with no host window and no X
server. The `display disabled` error some calls return is not a statement
about the host - `screenshot` reads `gpu_get_display_info`, so the error
means the *guest* GPU's display-enable bit is off, which it is during early
boot and between attract-demo segments. Retry once the game is showing a
picture and the same call succeeds. Screenshot-guided menu navigation
therefore needs no `xvfb-run`; see
[Driving the game to an uncovered scene](#driving-the-game-to-an-uncovered-scene).

## The savestate-resume fix

Stock, the recomp runtime cannot resume a savestate. `boot_state.c`'s
`apply_section` forces `cpu->pc = entry_pc` on load; that entry is the game's
BSS-clear routine, so every load faithfully restores the RAM image and then
immediately zeroes the region holding the game-mode word `0x8007B83C`. The
machine falls back into the boot chain and parks there. The ack is
`{"ok":true}` either way, so nothing announces the failure.

One line fixes it - honour the PC `savestate_poll` recorded, falling back to
the entry only for snapshots that predate resume-PC capture:

```c
cpu->pc = c->pc ? c->pc : entry_pc;
```

The recomp workspace is an untracked sibling tree, so that edit has no home
there and has been lost to a stray `git checkout` before. The durable copy
lives here, as a script that reapplies it to a fresh checkout:

```bash
python3 scripts/recomp/apply_boot_state_fix.py          # idempotent
cd "$LEGAIA_RECOMP_DIR/build-dbg" && make psx-runtime
```

It is a script rather than a `.patch` for two reasons. psxrecomp is PolyForm
Noncommercial and this repo is `MIT OR Unlicense`, so a context diff would
vendor third-party source lines under a license this repo does not grant; an
anchored replacement carries only the line it rewrites. It also survives
upstream line-number drift that would break a diff, and refuses rather than
guesses if the anchor is missing or ambiguous. `--check` reports the current
form, `--revert` restores stock.

Build the `psx-runtime` target, **not** `Legend_of_Legaia_Recompiled`. The
executable of that name sits in the build directory, so make treats it as an
already-satisfied file target and does nothing, successfully and silently -
leaving a stale binary that behaves exactly like an unpatched one.

With the fix built in, the parked slots resume into live scenes rather than
the boot entry: a battle slot reaches mode `0x15`, town slots reach `town01`
in field mode `0x3`.

### Preflight

Three different faults all present as "the trace was taken at the boot
entry", and only one of them is the runtime:

| Fault | Signal | Fix |
|---|---|---|
| Self-wiping runtime | `boot_state.c` carries `cpu->pc = entry_pc` | `apply_boot_state_fix.py` |
| Stale build | runtime binary older than `boot_state.c` | `make psx-runtime` |
| Stale snapshot | the `.pst` itself records `pc == 0` | recapture that slot |

`scripts/recomp/preflight.py` separates them, reading the runtime form and
build mtimes from the workspace and the resume PC out of each snapshot's CPU
section. A slot whose stored PC is zero falls back to the entry even on a
correct build, so it is reported as a slot fault and never as a runtime one.

```bash
python3 scripts/recomp/preflight.py            # runtime + every slot found
python3 scripts/recomp/probe.py preflight --slot 4
```

It is wired in where a miss is expensive rather than left to be remembered:
`probe.py launch` refuses to start a runtime that cannot resume,
`trace_capture.py --savestate` refuses to capture before it connects, and a
failed `--expect-mode` / `--expect-scene` check attaches the diagnosis to the
error. When preflight is clean the message says so, so a genuine divergence
is not written off as a harness problem. Both gates take `--skip-preflight`.

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
- **`savestate` load restores a live scene only if the runtime honours the
  saved resume PC.** A stock runtime re-enters at the game entry and wipes
  the state it just restored, and the ack is `{"ok":true}` either way, so the
  failure is silent unless the caller verifies - always pass
  `--expect-scene` / `--expect-mode`. Before concluding a slot is dead, run
  the preflight: a load that lands at mode `0` is as likely to be a stale
  snapshot or a stale build as a broken runtime. See
  [The savestate-resume fix](#the-savestate-resume-fix).
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

**`cam.eye` and `cam.focus` are not a world-space eye / look-at pair.** They
are the retail camera globals verbatim, and both carry a convention that a
reader who assumes "eye position, focus point" will get wrong:

- `cam.eye` is the **eye-space translation trio** `_DAT_800840B8` - the
  post-rotation `(dx, dy, depth)` of `screen = H·(R·(v − focus) + tr_eye)/Ze`.
  Its Z is the eye-back depth (order `10^4`), in a space carrying retail's
  `6×` world scale. It is not a position in the world.
- `cam.focus` is `_DAT_80089118`, which stores the focus **negated in X and
  Z**: a shot focused on world `(8640, 0, 10304)` reads `(-8640, 0, -10304)`.
  `focus.y` is un-negated and is `0` through the opening chain.

An engine side that emits a world-space `(eye, look_at)` here compares
different quantities in different coordinate frames. That produces a total,
permanent divergence on both channels in every scene - one that looks exactly
like a camera defect and cannot be closed by any change to the camera. Check
the convention before reading a camera divergence as a camera bug.

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
    --frames 100 --map camera --out /tmp/scratch/recomp_cam.jsonl
```

The instance must already be in the scene of interest. Budget for it: the
pre-title attract demo runs at a fraction of real speed under `--headless`,
and a tight polling loop starves the emulator further, so throttle any wait
loop to a few samples a second. Getting there is the subject of the next
section.

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

## Driving the game to an uncovered scene

Two routes reach a scene the opening chain does not contain. Prefer the
first; it costs seconds rather than minutes.

### Route 1 - load a parked savestate

On a runtime that honours the saved resume PC (above), a slot load drops
straight into a live scene:

```bash
python3 scripts/recomp/probe.py --port 4497 load-state 4 \
    --expect-scene 'jou ene' --expect-mode 0x15
```

Verify liveness before capturing - sample `{"cmd":"frame"}` twice a few
seconds apart and confirm the counter advances. A slot that reports its
expected scene but a frozen frame counter is not usable.

Slot contents drift: anyone pressing F1-F12 in a windowed run overwrites
one, so the slot number is a hint and the verification is the evidence.
Read scene + mode after every load rather than trusting an inventory, and
take a screenshot when the mode alone is ambiguous - a slot reported as a
field scene can turn out to be parked on the name-entry screen, which
shares the field mode word.

### Route 2 - cold boot and load a memory-card save

Route 2 reaches anywhere a save file reaches, including late-game areas no
savestate covers. It needs a card with a save on it, which is the part that
silently fails: the runtime resolves the card next to the **executable**
(`default_memcard_dir` is the exe's directory, deliberately never the cwd),
and a freshly formatted `card1.mcd` there has every directory frame set to
`0xA0` (free). `CONTINUE` on a blank card offers nothing to load, so the
title screen simply re-arms and the mode word never leaves `0x17`.

Seed it from a real card, backing up first, and restore afterwards - the
file is shared state:

```bash
B=<recomp-workspace>/build-dbg
cp "$B/card1.mcd" "$B/card1.mcd.backup"
cp ~/.mednafen/sav/<a Legaia card>.0.mcr "$B/card1.mcd"
# ... run the capture ...
cp "$B/card1.mcd.backup" "$B/card1.mcd"
```

Confirm the card carries a save before booting: an active block's directory
frame starts `0x51` and the block name reads `BASCUS-94254...`.
`save-tool saves <card>` reports the same thing, and `save-tool party` reads
the party out so a lane can pick a save deep enough for the scene it wants.

Then boot headless and drive the pad, checking each screen with a
screenshot rather than pressing blind. The navigation is
`START` (skip attract) -> `DOWN` (move off `NEW GAME`) -> `CROSS`
(`CONTINUE`) -> `CROSS` (card port) -> `CROSS` (save block) -> `UP` ->
`CROSS` (confirm). Four traps sit on that path:

- **`CONTINUE` is not the default.** The title cursor starts on `NEW GAME`;
  a bare `CROSS` starts a new game instead.
- **The load confirm defaults to `No`.** The `Yes`/`No` box opens on `No`,
  so a blind `CROSS` cancels back to the title - indistinguishable, from
  the mode word alone, from never having pressed anything.
- **The title times out back to the attract demo.** A screenshot round-trip
  costs seconds, so poll for mode `0x17` and issue the pad input
  immediately on seeing it; save the confirming screenshot for after.
- **`START` does not always skip the attract demo.** The field-demo segment
  plays out on its own; poll gently for mode `0x17` rather than mashing.

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
module docs for the full table): every `cam.*` channel is read from the
engine's live retail camera globals
([`Camera::globals`](../../crates/engine-core/src/camera.rs)), which are the
same ten words the recomp-side address map reads, so the two sides report the
same quantity in the same frame; player/actor samples come from
`move_state.world_x/z` + `render_26`. `mode` maps `SceneMode` onto the retail game-mode word (Field
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

Two things make an auto-aligned offset worth checking before believing a
report:

- **`sim-trace`'s first record is a pre-tick boot sample**, so the engine
  camera always "changes" between its first two lines as the controller
  initialises. Aligning retail's first real cut onto that artifact throws
  the offset off by the whole lead-in. `--skip-lead-b` (default 1) drops
  it; `--skip-lead-a` is the same knob for the reference trace.
- **The reported first-divergence frame can be the first frame of the
  overlap.** When the offset places trace B's start before trace A's, the
  aligned region begins mid-trace, and a divergence "at B frame 595" may
  simply be the earliest frame that exists on both sides - not a run of
  595 matching frames. Read the context window: if the divergence has no
  rows above it, nothing was compared before it.

Angle channels reduce both values mod 4096 *before* measuring wraparound
distance, so a capture that forwards retail's raw u16 angle word (rather
than masking to 12 bits, as `trace_capture` does) still compares correctly.
Without that reduction the wraparound term goes negative and the channel
reports OK on every frame - the reason the reduction is not optional.

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

### Divergences a matched run surfaces

Run against the opening-chain scenes, the state oracle reports the same
shape of result on every one of them, and the strongest evidence is the
part that needs no alignment at all - comparing each channel's *range* over
a window sidesteps the offset question entirely.

**The camera-position divergence had two stacked causes, and the outer one
was a measurement artifact.** The engine originally emitted its runtime
camera's world-space `eye` / `look_at` on channels whose retail side is the
eye-space translation trio and the negated focus globals ([above](#canonical-trace-shape)).
Those value sets are disjoint by construction, so the reading "the engine's
camera never leaves a fixed height" (`cam.eye.y` constant at 80) was
measuring `follow_height`, not a camera that refused to move.

Underneath it sat a real defect: [`Camera`](../../crates/engine-core/src/camera.rs)
modelled no eye-space translation trio at all. The op-`0x45` Configure
angle slots and `h` reached it, but slots 3/4/5 had nowhere to land and the
controller stayed in its follow orbit through scripted shots, so scenes
rendered and traced from the follow pose. The retail-faithful decode existed
only in the shell's `cutscene_view`, which `sim-trace` does not go through.

Both are closed: `Camera` now carries the ten globals, applies each masked
slot per-axis, and runs [`camera_mover`](../../crates/engine-vm/src/camera_mover.rs)
for `apply != 0` beats. Measured alignment-free over the opening chain, the
angle channels reproduce retail exactly (`opstati` mid-window pitch/yaw
`4066` / `3706` against retail's `65506` / `65146` masked to 12 bits) and
the eye channels overlap where they previously could not (`opstati`
`eye.x` shares 308 of retail's 323 distinct values).

**Pose changes are held, not interpolated** - same root cause, now confirmed
rather than assumed. With no mover and no translation trio the engine held
2-6 distinct poses per 3000-frame window; with both, the same windows carry
769-1313. The residual is not a camera defect but the direct-boot confound
below.

**The remaining position gap is a timeline-phase difference, not a pose
error.** `sim-trace` boots a scene cold at its frame 0 while a retail capture
of the same scene arrives through the prologue chain already several beats
in, so an aligned frame compares the engine's beat 0 against retail's beat 5.
`opdeene` is the clear case: the engine's staged focus values walk
`-10816 → -5824 → -8568 → -8640`, and retail's capture window contains only
the last two. Distinguishing a pose error from this needs the engine driven
through the chain, not booted into the scene - the same confound as the
"scenes run no camera script at all" reading below.

**Some scenes run no camera script at all.** In the scenes the engine
enters cold at record 0, `cam.h` never appears and the angles never leave
0, meaning no Configure executes across the whole window, while retail runs
a full sequence of beats in the same scene. This one carries a real
confound: retail reached the scene through the prologue chain with its
story flags set, and `sim-trace` boots the scene directly. A camera beat
gated on arrival state would produce exactly this reading without any
camera-system defect. Distinguishing the two needs the engine driven
through the chain, not booted into the scene.

`cam.roll` matches (both sides hold 0), as do `scene` and `mode`.

## Note-level audio differential

The same alignment-and-first-divergence shape, applied to BGM instead of
frame state. Where the state trace compares camera and actor channels per
frame, this compares *note sequences* - the stream of key-ons a sequencer
asked the SPU for - which is the level at which "missing notes" is a
measurable claim rather than an impression.

| Component | Lives in | Role |
|---|---|---|
| `audio_note_capture.py` | [`scripts/recomp/`](../../scripts/recomp/audio_note_capture.py) | Retail note timeline from the recomp's SPU rings |
| `note-trace` | [`crates/engine-audio`](../../crates/engine-audio/src/bin/note-trace.rs) | The same timeline through the engine's own sequencer |
| `note_diff.py` | [`scripts/recomp/`](../../scripts/recomp/note_diff.py) | Aligns two note traces, reports the first divergence per channel |

Both sides record at the same layer - the instant a voice is keyed on,
snapshotting the ADPCM start address, pitch, per-voice volumes and raw ADSR
words - so a divergence localises directly. A missing note-on means the
sequencer never asked for it; a wrong start address means tone selection
diverged; a wrong pitch means the note or its bend resolved differently.

```bash
python3 scripts/recomp/audio_note_capture.py --port 4472 \
    --seconds 30 --out /tmp/scratch/recomp_notes.jsonl --summary
./target/release/note-trace --extracted extracted --track 0 \
    --frames 1800 --out /tmp/scratch/engine_notes.jsonl
python3 scripts/recomp/note_diff.py /tmp/scratch/recomp_notes.jsonl \
    /tmp/scratch/engine_notes.jsonl
```

### The unclocked-SPU trap

`spu_render()` in the recomp runtime is driven by the host audio pump, so a
runtime started with `--headless` never clocks the SPU: `render_frames`
stays 0, every voice sits frozen at `env_level == 0`, and no envelope ever
decays. The retail sound driver picks a free voice by polling CURVOL for
`env_level == 0`, so against a frozen SPU it believes all 24 voices are free
forever and keys nearly everything onto voice 0. The resulting capture looks
plausible and is entirely an artifact.

`audio_note_capture.py` refuses to run unless `render_frames` is advancing.
To get a clocked instance without an audio device or a desktop:

```bash
SDL_AUDIODRIVER=dummy xvfb-run -a \
    ./build-dbg/Legend_of_Legaia_Recompiled --debug-port 4472 \
    --no-launcher --bios SCPH1001.BIN --game game.toml
```

Wall-clock speed is irrelevant; what matters is that SPU frames advance at
735 per guest frame (44100/60), which puts the sequencer and the envelopes
in the same time base as retail even when the host runs below real time.

### Reading the diff

Each side's allocator lays the VAB's VAGs out in SPU RAM itself, so raw
addresses never match. Both allocate in bank upload order, ascending, so
`note_diff.py` maps addresses to dense **VAG ids** by ascending order within
each trace - allocator-independent tone identity, and the handle back to the
disc.

That renumbering has a sharp edge worth knowing before trusting a clean
`vag` column: it is per-trace, so a tone played on one side and never on the
other shifts every id above it, and two different tones can then share an
id. The tool warns when the two sides' distinct-VAG counts differ; in that
case `pitch` is the reliable channel.

Alignment is on note *ordinal*, not wall time - a capture generally starts
mid-track, and the two sides' frame counters have unrelated origins.

### Matching the two sides to the same track

A note diff is meaningless unless both sides play the same score. The
earlier reading that field BGM is scene-bundle-resident (and therefore
unmatchable against the `music_01` corpus) is **falsified**: every scene
that starts BGM selects a global-pool id, so every track a capture can
contain is a `music_01` entry and `note-trace --track` can reproduce it.
See [`subsystems/audio.md`](../subsystems/audio.md#which-track-a-scene-plays).

Pick the track without guessing by reading the resolver's own globals out of
the running recomp - `0x8007BAC8` is the live BGM id and `0x8007BAB8` the
PROT index it resolved to:

```bash
python3 scripts/recomp/probe.py --port 4471 \
    --json '{"cmd":"read_ram","addr":"0x8007BAC8","len":4}'
```

Both read `0` until a scene actually starts music, so drive the game to a
field scene first; the ids in the opening chain only appear once the
prologue hands off.

**`--track` is not the sound-test slot.** `note-trace` enumerates every
VAB+SEQ pair in the `music_01` CDNAME block, which begins two entries below
the bank base the resolver uses: `prot_entry = 988 + track`, so
`track = bgm_id - 2000 + 2`. Cross-check against the `prot_entry` column of
`note-trace --list` rather than computing it from the id.

**Verify the 735.0 ratio yourself.** The capture script's guard only checks
that `render_frames` is *advancing*, not that it advances at the right rate,
so a throttled or partially-clocked instance passes it. Sample
`spu_status.render_frames` and `frame` across an interval and confirm the
quotient is exactly `735.0` before trusting a capture.

Some bank entries carry **more than one** VAB. `note-trace` binds the first
`pBAV` to the first `pQES`, which is the pairing the resolver uses; a
second bank later in the entry belongs to a different pair, so a tone-count
mismatch is not by itself evidence that bank staging picked wrong.

### Divergences a matched run surfaces

Two structural engine defects surfaced here - both reproduced on any window
length, so neither was a capture-alignment artifact - and both are closed:

**Per-voice volume was written in the wrong domain.** The engine's key-on
volumes occupied `0..127` where retail's occupy the SPU's 14-bit
`0..0x3FFF` - `vab_bind.rs`'s `fire` divided by `127^2` and never applied
the final `×0x3FFF` widening, leaving every key-on short by a factor of
`0x81` (the same `0..127 → 0..16383` constant the libspu command shims
use), which made the `vol` channel diverge at the first note of any track.
`fire` now carries the retail chain of `FUN_80067550` with its staged
truncation points - `vel × bank_mvol × 0x3FFF / 0x3F01`, then
`× prog_mvol × tone_vol / 0x3F01` (the program-level `ProgAtr.mvol` /
`.mpan` factors included), and the sequencer path's closing square taper
(`v²/0x3FFF` per side) lands in `sequencer.rs`'s `channel_mix`.

**Tone selection collapsed.** On a bank whose program table offers many
tones, the engine keyed only a few distinct VAGs while retail draws across
the whole table, and extending the trace window several-fold added no new
tones - a program-change lookup defect, not a track that had not reached
its other instruments. Root cause: the VAB file packs one tone page per
*used* program, and the engine indexed those packed pages with the raw
program number. Retail resolves a program number to its page by **rank
among the used `ProgAtr` slots** (`FUN_80068d94` writes the rank table at
VAB open, `FUN_80068b98` reads it at program change), and 66 of the 217
disc banks - 43 of the 77 music banks - author sparse program sets, so on
those banks most program numbers landed on the wrong page or (309 program
numbers corpus-wide) fell off the packed table and dropped outright.
`VabBank::upload` now expands the pages into program-number space; the law
is pinned corpus-wide by `engine-audio/tests/real_vab_program_mapping.rs`.
Because a wrong tone carries a wrong base note, the `pitch` channel
diverged as a consequence of `vag` - read `vag` first and treat `pitch` as
downstream of it.

The `v` (voice index) channel differs whenever either of the above does:
allocation order is a function of the note stream, so it is an effect, not
an independent finding.

## See also

- [`determinism-replay.md`](determinism-replay.md) - the engine-vs-itself
  side of the parity stack; this page is the engine-vs-retail-recomp side.
- [`pcsx-redux-automation.md`](pcsx-redux-automation.md) /
  [`mednafen-automation.md`](mednafen-automation.md) - emulator-based
  retail observation (breakpoints / save-state diffing); the recomp path
  adds cheap frame-tagged structural reads at full speed.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) - the
  pinned retail globals the address maps read.
- [`docs/formats/seq.md`](../formats/seq.md) - the SEQ grammar the engine
  side parses, including how a truncated stream reports itself.
