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
  saved resume PC.** `savestate_poll` serialises the block-leader resume PC
  into the snapshot, but `boot_state.c`'s `apply_section` has shipped in two
  forms. The self-wiping form forces `cpu->pc = entry_pc`; that entry is the
  game's BSS-clear routine, which zeroes the region holding the game-mode
  word `0x8007B83C`, so the load restores the RAM image and then immediately
  wipes the state that gives it meaning - the machine falls back into the
  boot chain and parks there. The working form is
  `cpu->pc = c->pc ? c->pc : entry_pc;`, which resumes where the snapshot was
  taken. The ack is `{"ok":true}` under both, so the failure is silent unless
  the caller verifies - always pass `--expect-scene` / `--expect-mode`.
  Check which form your runtime has before concluding a slot is dead:
  `grep -n 'cpu->pc =' runtime/src/boot_state.c` in the recomp workspace.
  Slots written before resume-PC capture existed carry `c->pc == 0` and fall
  back to `entry_pc`, so they self-wipe even on a fixed runtime; a slot that
  loads to mode `0` is a stale snapshot, not necessarily a broken build.
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

**The engine's camera never leaves a fixed height.** `cam.eye.y` is
constant at 80 in every scene measured. Retail's moves continuously over
hundreds of distinct values per scene, climbing past 5000 and dropping
below -3000 as the scripted shots play. Nothing about this depends on
frame alignment: the two value sets are disjoint.

**Camera position tracks the player instead of the script.** The engine's
`cam.focus` sits on the player's world X/Z at that same height 80, and
`cam.eye` a short fixed distance off it - a follow orbit. Retail's focus is
an absolute scripted point far outside the player's neighbourhood, with
`focus.y` identically 0. So the op-`0x45` Configure *angle* slots and `h`
do reach the engine camera - `cam.pitch` / `cam.yaw` / `cam.h` take
plausible per-beat values in the scenes where a Configure runs - while the
eye and focus position slots do not survive into the pose the camera
renders from.

**Pose changes are held, not interpolated.** Across a 3000-frame window the
engine emits a handful of distinct camera poses, holding each for hundreds
of frames; retail's camera state changes every few frames throughout. This
is consistent with the position half above rather than independent of it,
and a beat's curve mode cannot be attributed from the state trace alone -
treat it as a symptom pointing at the same defect, not a separate one.

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

Two are structural - they reproduce on any window length, so they are not
capture-alignment artifacts:

**Per-voice volume is written in the wrong domain.** The engine's key-on
volumes occupy `0..127` where retail's occupy the SPU's 14-bit `0..0x3FFF`.
`vab_bind.rs`'s `fire` states the intended chain in its own comment -
`bank * prog * vel / 127^3 * 0x3FFF` - but divides by `127 * 127` and never
applies the final widening, leaving the result short by a factor of `0x81`
(the same `0..127 → 0..16383` constant the libspu command shims use). This
makes the `vol` channel diverge at the first note of any track.

**Tone selection collapses.** On a bank whose program table offers many
tones, the engine keys only a few distinct VAGs while retail draws across
the table; extending the trace window several-fold adds no new tones, so
this is a program-change / tone-region lookup defect rather than a track
that simply has not reached its other instruments yet. Because a wrong tone
carries a wrong base note, the `pitch` channel diverges as a consequence -
read `vag` first and treat `pitch` as downstream of it.

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
