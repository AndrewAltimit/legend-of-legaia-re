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
| `legaia-engine replay` / `record` | [`crates/engine-shell/src/bin/legaia-engine/cli.rs`](../../crates/engine-shell/src/bin/legaia-engine/cli.rs) | Headless playback + interactive capture (flag definitions) |

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

Every argument is a flag - `--input` is not positional - and `--out`
defaults to `-` (stdout), so the trace pipes without a temporary file.

The synthetic driver mirrors the determinism-gate harness: `World::new`
+ an 8-slot actor pool, RNG seeded from `meta.rng_seed`, ticked once
per replay frame. No disc required.

`--strict` exits non-zero on the first divergence between the recorded
trace and the file's `[[expected]]` fixture. Without it, divergence is
printed to stderr but the command succeeds.

### `legaia-engine record`

Thin wrapper over `play-window` with a pad-capture hook armed:

```
legaia-engine record --out my.replay.toml --disc "Legend of Legaia (USA).bin" \
    [--scene town01] [--scenario LABEL] [--rng-seed 0xDEADC0DE]
```

Assets come from `--extracted-root` (default `extracted`) unless `--disc`
points at a `.bin`, which supersedes it. The bracketed values above are the
defaults. `--no-audio`, `--world-map` and `--save-dir` behave as they do on
`play-window`, which is the host this wraps.

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

## Critical-path replay: the game-denominated sibling

[`crates/engine-shell/tests/critical_path_replay.rs`](../../crates/engine-shell/tests/critical_path_replay.rs)
drives the chapter-1 spine with the **pad** as the only actuator and scores
how far it gets, against a ratcheted baseline in
[`scripts/replays/critical_path_baseline.toml`](../../scripts/replays/critical_path_baseline.toml).

It exists because every other progression oracle is denominated in the disc.
`chapter1_spine_oracle` proves the same legs, but moves the player with
`seat_player_at_tile` - its `walk_onto_tile` helper is a teleport pair that
synthesises the tile crossing, and `chapter1_spine.toml` says as much: the
pad rows "document the traversal" while the diff "drives the transitions by
seating the player on the trigger tiles". That is correct for asking *does
the scene graph connect*, and structurally blind to locomotion speed and
heading, the collision probe, the walkability grid, and the camera-relative
pad remap. Nothing is seated here.

Waypoints come from a BFS over the walkability grid whose edges are certified
by `field_dir_blocked` - the same probe the locomotion step runs. Two
constraints fix the lattice pitch, and both are tighter than "one wall bit":
the probe reaches only ~47 units ahead of the source, so an edge longer than
that is not covered by its own certificate; and a tile centre is `128t + 64`,
so the pitch must divide 64 or the planner can never stand on a doorway's
centreline, where the probe's laterally-spread points then read a one-tile
opening as sealed. Both failure modes were observed before the pitch settled
at 32.

The goal is best-effort by design: a scene exit is a door, and a door tile
reads as a wall (`seat_player_at_tile` documents this), so the planner routes
to the closest reachable node and the follower presses from there. A leg
succeeds on the transition firing, not on arrival.

A stall reports the tile, the world position, the next waypoint, and both
wall arms (`field_dir_blocked` and `field_actor_dir_blocked`) separately -
"the player stopped here" and "the engine says every useful direction is a
wall" are different findings and the bare tile cannot tell them apart. The
first run's stall is written up as
[Rim Elm's south gate](../reference/re-settled-threads.md#rim-elms-south-gate);
it found a defect every seated oracle is blind to, and its first two
diagnoses were both wrong, which is the argument for making a stall
self-describing rather than a bare failure.

### Hazards, and the difference between occupying and entering

On top of the walls the planner routes around **hazard tiles**: tiles the grid
calls walkable and stepping onto ends the leg somewhere else - another scene's
overworld portal, or the door a dungeon leg arrived through. They are keyed in
the dispatch frame (`world >> 7`), which is the frame the walk-on dispatch
compares in.

A hazard is unsafe to **enter**, never unsafe to *occupy*, and conflating the
two seals scenes that are wide open. Retail's dispatcher fires on a tile
*change*, so a step that stays inside one tile fires nothing - while a planner
that tests only the destination tile refuses every step out of a tile it starts
inside. A dungeon arrival is seated on the door it came in through, i.e. inside
a hazard, so that planner reports the whole interior unreachable. The fix is to
compare source and destination tiles, not to shrink the hazard set.

Measured on `keikoku` (`engine-shell/examples/keikoku_reach_probe`, which runs
this per mouth and needs no ladder), from the `(58, 24)` arrival:

| sub-cells reached | avoid set |
|---|---|
| 400,001 | nothing - capped; the grid's `& 0x7F` wrap makes the set endless |
| 1 | trigger tiles within 12, seat tile included |
| 1,114 | the same, seat tile dropped (89 tiles: one chamber) |
| 25,939 | the arrival record's band, and only that |

The four mouths behave identically, which is what rules the arrival seat and
the mouth choice out as causes. Both middle rows are seals in their own right:
the self-block, and a radius that cannot tell a door from the corridor onward.

### Scoring a dungeon leg: the door, not the event

Every `keikoku` exit returns to `map01`, so `Transitioned("map01")` cannot tell
a traverse from a step back out of the entrance - the shape the rung's first
draft passed on, aiming 47 tiles away, walking two and reporting a clean
transition. What separates them is on the disc: the scene's `.MAP` gate-1
triggers joined to their partition-2 records give four scene-change records,
each returning to its **own** `map01` tile, so the arrival coordinate names the
door. Leaving by a different record is a condition a backed-out leg cannot
meet, and `LEGAIA_CPR_RUNG5_BACKOUT=1` re-aims the leg at its own entrance to
demonstrate that the rung then reads unclear.

Grouping by record also separates three populations a tile radius cannot:
those four doors, the four single-tile arrival beats, and a 27-tile shared beat
record across the chambers' inner doorways. A radius filter hazards the
corridor onward and aims at a beat band.

### A scripted sequence may be waiting for the player

Legs hand the frame to `drain_scripted` while a cutscene or dialogue owns
input. A neutral pad drains a sequence that runs on its own clock; it does
**not** drain one that pages, because a narration page waits on a confirm. Held
neutral, `keikoku`'s record 7 - a story cutscene one-shot-latched on system
flag `0x2BB` - never advances, and the leg reports an engine hang at the band's
first tile. The drain pulses Cross (press 2, release 14) rather than holding
it, because the advance is edge-triggered and a held button pages once.

The pad-inversion arithmetic, the door-identity clause and the baseline parser
are covered by disc-free unit tests, so the file stays non-vacuous in CI where
the ladder skips.

## Scene-frontier ladder: the breadth-denominated sibling

[`crates/engine-core/tests/chapter1_frontier_ladder.rs`](../../crates/engine-core/tests/chapter1_frontier_ladder.rs)
asks the other question. The critical-path ladder walks one route well and
reports one number; this one enumerates the whole chapter-1 reachable scene
set and gives every member a verdict, against a ratcheted baseline in
[`scripts/replays/chapter1_frontier_baseline.toml`](../../scripts/replays/chapter1_frontier_baseline.toml).
A route visits five scenes and is silent about the other twenty-two, so
"the engine cannot get past the Ravine" and "no fixture drives past the
Ravine" read identically until something scores the scenes one at a time.

The scene set is the BFS closure of `town01` over each scene's own decoded
`0x3F` destinations, stopped at the **kingdom boundary** - a destination that
is another kingdom's overworld is recorded and not expanded, since `map02` /
`map03` have their own spine oracles. The Drake kingdom's only handoff is
`jiji -> map02`, and the ladder pins that edge. Two limits are structural: the
closure is a reachability partition rather than a narrative one, and it is a
closure over `0x3F` only, so scenes reached by the sibling `0x3E` door warp
(which carries a scene-*type* selector rather than a name) are outside it.

Six rungs per scene, ordered and cumulative: the assets resolve, the MAN
parses, `SceneHost` enters it, the entry script settles and hands control
back, pad-only input displaces the player, an exit record fires and lands in
the scene it names. A scene whose script leaves on its own is marked
not-applicable on the last two rather than failed. Failures are
self-describing in the same spirit as the critical-path stalls: a script park
reports `(pc, opcode)` off the live field VM, a locomotion stall reports the
tile.

### Two guards that changed what the numbers mean

**"Control released" needs a third clause.** The critical-path ladder's
predicate is "no cutscene timeline and no dialogue owning input". A
first-visit record is neither - it is a helper context - so that predicate
returns while `izumi`'s spring choreography is still moving the player. The
frontier ladder waits for the spawned records too.

**A locomotion rung needs a released-pad control.** Without one, "the player
walked" and "a script moved the player" are the same measurement. Each scene's
driven probe is scored against a released-pad run of the same length from the
same state, and when the driven run fails to beat its control the scene is
probed once more as a *revisit* (flag banks left latched), which separates
"the engine cannot walk here" from "this scene's first-visit script owns the
player". `izumi` is the case: 30 tiles driven, 30 tiles with the pad released,
4 tiles on the revisit.

### Where the frontier stops, and what stops it

Every scene in the closure loads, enters and settles. The remaining stops are
all about **doors**, and Part D separates the shapes by running three decoders
over the same MAN: a clean per-partition fall-through walk (what a door *op*
looks like), the recovering destination-table pass (what a door *entry* looks
like), and the ladder's own `.MAP` gate-1 trigger → partition-2 record →
`0x3F` join (what a door the player can *walk onto* looks like).

Ordinary interiors have all three. The Uru Mais chain (`uru`, `urudre1`,
`urudre2`, `urudre3`) has only the middle one: the clean walk finds no `0x3F`
anywhere in those four MANs, so their destinations are known to this project
as table entries and not as decoded ops. `jouine` has none of the three.

Two further probes turn that from a decoder note into a playability one.
Stepping onto every gate-1 walk-on tile the five carry fires **nothing**, and
executing 160 of their own partition-1 and partition-2 record bodies through
the field VM reaches no scene change either. In the port as it stands,
entering the Uru Mais chain or `jouine` is one-way, and the four Uru Mais
graph edges rest on weaker footing than the rest of the closure.

How retail leaves them is unidentified, and the record probe bounds its own
claim: a warp behind a story gate, an inventory check or an actor-motion wait
a headless world never satisfies would not be reached from a 180-frame run
either. "No record this probe executed warped" is the measurement; "the bytes
contain no warp" is not.

## See also

- [`docs/subsystems/engine.md`](../subsystems/engine.md) - the from-scratch engine the record/replay loop drives.
- [`docs/subsystems/script-vm.md`](../subsystems/script-vm.md) - the field/event VM whose pad-driven state the trace captures.
