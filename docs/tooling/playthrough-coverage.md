# Playthrough trace-driven coverage

A program for turning *what the game actually executes during the opening* into
a systematic documentation worklist. Instead of guessing which dumped-but-
unexplained functions matter, we play a scripted segment of the start of the
game under PCSX-Redux with a breakpoint armed on every not-yet-understood
function entry, and let the hits tell us what to document next.

This page is the program's **instrument**: the segment ledger, the gap-burndown
tracker, and the run/triage loop. The probe harness it drives lives in
[`pcsx-redux-automation.md`](pcsx-redux-automation.md); the gap-set is defined
off the [port catalog](port-catalog.md).

## Contents

- [Why trace-driven](#why-trace-driven)
- [The gap-set](#the-gap-set)
- [The harness](#the-harness)
- [Attribution: SCUS vs overlay](#attribution-scus-vs-overlay)
- [The latency loop](#the-latency-loop)
- [Segment ledger](#segment-ledger)
- [Gap-burndown](#gap-burndown)

## Why trace-driven

Static analysis has documented most of the boot / field-VM / dialog paths, but
the dump corpus still holds hundreds of functions that have Ghidra output and no
prose explanation. Some are dead, some are demand-loaded by content we haven't
exercised, some are hot paths nobody has written up. Tracing the opening
separates the live residue from the dead weight: a function that fires while you
walk out of Rim Elm is, by construction, reachable and worth a doc paragraph.

The value is the **residue** and it grows as the playthrough enters less-trodden
ground. Early segments (boot, first field frame, dialog) are mostly green - that
code is already documented. The first battle's internals, the story scripting,
and named-NPC interaction are where the unexplained hits concentrate. Success is
measured as gap-burndown per segment, not as raw coverage.

## The gap-set

The set of function entries the tracer arms breakpoints on:

```
GAP-SET = dumped AND NOT documented AND NOT ignored
```

- **dumped** - a `ghidra/scripts/funcs/<addr>.txt` exists, so the address is a
  real function entry we can break on.
- **NOT documented** - no file under `docs/` cites the address. Documented code
  is excluded so breakpoints (and the interpreter budget) go only to the
  unexplained residue.
- **NOT ignored** - not in
  [`port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml). The
  ignore-list is host-replaced PsyQ / BIOS / libgte / libspu code; it is both
  out of scope for the port and hot every frame, so it would flood the trace.

Regenerate the committed worklist after any new dump or doc lands:

```bash
scripts/pcsx-redux/build_gap_worklist.py            # -> gap_worklist.txt
scripts/pcsx-redux/build_gap_worklist.py --bucket scus   # SCUS-only subset
```

The worklist is `scripts/pcsx-redux/gap_worklist.txt`: one address per line with
its dump-source stem and bucket. It shrinks as documentation lands - the same
mechanism that proves the program is working.

## The harness

[`autorun_trace_segment.lua`](../../scripts/pcsx-redux/autorun_trace_segment.lua)
arms one non-pausing exec breakpoint per gap-set address, plays a segment, and
writes `trace_segment.csv` (`addr, hits, first_frame, first_mode, first_ra,
stem`) plus a `.modes.txt` game-mode timeline. It is **passive** - it records
whatever runs - so an input timeline is optional. Key env knobs:

| Env | Meaning |
|---|---|
| `LEGAIA_NO_SSTATE=1` | Cold boot from BIOS (segment S1); the save loader is no-op'd. |
| `LEGAIA_WORKLIST` | Gap-set file (default `gap_worklist.txt`). |
| `LEGAIA_ADDR_LO` / `LEGAIA_ADDR_HI` | Address window, e.g. SCUS-only with `HI=0x801C0000`. |
| `LEGAIA_MAX_BPS` | Cap breakpoints (0 = all) for a smoke run. |
| `LEGAIA_INPUTS` | Optional `<frame>:+BTN,<frame>:-BTN,...` timeline (vsync-since-capture). |
| `LEGAIA_MASH` | Optional `<BTN>:<period>` auto-advance pulse (headless title/dialog/cutscene driver). |
| `LEGAIA_FRAMES` | Capture budget in vsyncs. |

### Running it headless (agent-operated)

The agent runs the emulator directly under a headless X server - this is not an
author/run hand-off.

> **Anchor on catalogued, fingerprinted saves - never an ephemeral live slot.**
> A PCSX-Redux quicksave slot (`~/Tools/pcsx-redux/SCUS94254.sstateN`) is
> overwritten the next time you save in it; reverse-engineering against one is
> not reproducible and silently changes meaning. Every reproducible trace must
> load a save from the immutable library (`saves/library/pcsx-redux/<sha>`) by
> its `backup_fingerprint`, via `run_probe.sh --scenario <label>`. The library
> corpus is built by the boot-onward driver below (it `createSaveState`s each
> checkpoint, which is then `manage-states.py backup`-ed and catalogued), not by
> grabbing whatever happens to be in a live slot.

A save-state-anchored trace loads a catalogued checkpoint (the save jumps past
the slow BIOS boot and fires `GPU::Vsync` immediately, so the probe arms at
once), with `boot_delay=2`:

```bash
LEGAIA_BOOT_DELAY=2 \
LEGAIA_LUA=scripts/pcsx-redux/autorun_trace_segment.lua \
LEGAIA_OUT=captures/trace/seg.csv LEGAIA_FRAMES=600 \
    xvfb-run -a timeout --kill-after=15s 220s \
    bash scripts/pcsx-redux/run_probe.sh --scenario <checkpoint_label>
```

Reliability notes (hard-won; these are the environment's quirks, not the
harness's):

- **Use a headless X server (`xvfb-run -a`).** On a real `DISPLAY`, PCSX-Redux
  v-syncs to the monitor and an unfocused/occluded window is throttled to a
  crawl - boot never reaches the arming point. Bare `Xvfb` (vs `xvfb-run`) can
  crash PCSX's GLX init during boot; `xvfb-run` sets up the Xauthority it needs.
- **Save-state load must be early (`boot_delay=2`).** The driver waits
  `boot_delay` `GPU::Vsync` events before loading; those are sparse during BIOS
  boot, and the boot occasionally hangs on a CD read before the save loads. A
  low `boot_delay` wins that race more often.
- **Boot is a lottery - retry.** A run that never logs `gap-set exec probes
  armed` lost the boot race; just relaunch. A small retry loop (relaunch until
  the CSV has rows) makes capture reliable.
- **Cold boot needs a bespoke per-vsync driver, not the gap-set harness's
  arming gate.** `autorun_trace_segment.lua` defers to `probe.run`, which waits
  `boot_delay` `GPU::Vsync` events before doing anything - and those are sparse
  during the pre-render CD-boot phase, so a cold boot can sit in `waiting for
  boot`. The fix (used by [the boot-onward driver](#driving-from-boot-segment-s1))
  is a bespoke `GPU::Vsync` listener that polls `game_mode` from the first frame
  and reacts to state transitions: navigation is pure pad injection (no
  breakpoints), so the arming gate is irrelevant to it. Tracing the boot/title
  code itself (if wanted) arms on a memory-watchpoint at a known boot-transition
  register (`_DAT_801EF16C`, the title countdown) rather than a vsync count.
- **Breakpoint-count ceiling: arm a SMALL set per run.** Arming the full
  780-entry gap-set in one go stalls the emulator before capture; ~150 arms
  fine. Capture the whole gap-set as a UNION of windowed passes
  (`LEGAIA_ADDR_HI=0x801C0000` for the 167 SCUS, then overlay windows via
  `LEGAIA_ADDR_LO/HI`), each pass under the ceiling. The probe writes the CSV
  incrementally (every ~60 vsyncs) so a late PCSX abort - this build aborts a
  few hundred vsyncs into some resumed saves - still leaves the captured hits on
  disk; the `.hits.txt` snapshot is a second copy.

For a scripted-input segment, add `LEGAIA_INPUTS` / `LEGAIA_MASH`; in-game
vsyncs are dense so the timeline times reliably.

### Driving from boot (segment S1)

[`autorun_play_from_boot.lua`](../../scripts/pcsx-redux/autorun_play_from_boot.lua)
is the boot-onward driver that builds the catalogued checkpoint corpus. It is a
bespoke `GPU::Vsync` listener (not `probe.run`) that polls `game_mode` every
frame, mashes START+CROSS to skip logos / the "PRESS START" gate / intro FMV and
confirm NEW GAME (row 0) + advance opening dialogue, logs the `game_mode`
timeline, and at a target mode writes a checkpoint. It also **resumes** from a
save (`LEGAIA_SSTATE`) to drive the *next* segment forward - that is how the
corpus grows: each run resumes the previous segment's checkpoint and plays on.

**Checkpoint mechanism (validated round-trip).** `PCSX.createSaveState()` returns
a `{_type="Slice", _wrapper=cdata}` wrapper; this PCSX-Redux build does **not**
export the ffi slice accessors (`getSliceSize`/`getSliceData` - the older
audio-trace pattern is dead here), so the slice is written via the Support.File
API: `Support.File.open(path,"CREATE"):writeMoveSlice(slice)`, emitting the raw
(uncompressed) protobuf (~19 MB). The host gzips it to the GUI `.sstate` format
(`gzip -c x.rawsstate > x.sstate`, ~1.8 MB) and catalogs it
(`manage-states.py backup pcsx-redux x.sstate --label sN_…`). Byte-validated:
the gzipped checkpoint reloads and restores the exact captured mode.

**Cold boot works end to end (non-vsync tick).** Launch with
`-interpreter -debugger -fastboot`. Two facts shaped the solution:

1. `-fastboot` is required - the *default* boot path stalls on an early CD read
   (`loc 30 52 28`) in headless `-run` (both interpreter and `--fast`). With it,
   the game boots normally and reaches the title.
2. The bespoke `GPU::Vsync` listener goes **blind** from the title onward: the
   title's XA-BGM streaming stops `VSync(0)` delivery to the autorun, and it does
   not resume through the field load. (A GDB read of `game_mode` from outside the
   Lua VM proved the game keeps running - it advances to the attract FMV `0x1A`
   if un-driven, then loads the field once NEW GAME is confirmed - while the
   listener sees a "freeze". It was never a CD/emulator hang.)

The fix is a **non-vsync tick**: an exec breakpoint on the per-frame title tick
`FUN_801DD35C` fires regardless of GPU rendering. The driver drives *both* the
START+CROSS mash (PRESS-START gate + NEW GAME confirm) and the target-mode
detection + checkpoint from it. Validated end to end: cold boot -> title ->
NEW GAME -> opening field scene (`game_mode 0x03`, "walking set") -> checkpoint,
which gzips to a GUI `.sstate` that **reloads to the field**. The whole opening
ran with no crash. Notes: the title-tick BP fires through field-INIT but stops
once field-RUN begins, so use a small `LEGAIA_SETTLE` (the checkpoint must land
in the init window); pressing START+CROSS *together* navigates the title (single
-button variants stall at the menu mode `0x17`).

So cold-boot S1 is now capturable headless + reproducibly, alongside the resume
path. (GDB note: `gdb_probe.py`'s packet parser mis-frames the `+` ack into the
payload, raising a spurious checksum error - the read value is still visible in
the error text; a one-line framing fix makes it a clean host-side state oracle.)

## Attribution: SCUS vs overlay

A breakpoint is armed by virtual address. SCUS addresses (`0x800xxxxx`) are
always resident, so a SCUS hit unambiguously means *that* function ran. Overlay
addresses (`0x801c0000`+) are **VA-aliased**: different overlays occupy the same
address window, so a hit at an overlay address only means "the currently-
resident overlay executed that address." Attribute an overlay hit with two
pieces the trace records: the `first_mode` column (the game mode at first hit)
and the `.modes.txt` timeline (which overlay window was resident then), plus the
`stem` (the dump's overlay identity). When in doubt, prefer the 167 SCUS gap-set
addresses as the clean signal and confirm overlay hits against the resident
overlay before documenting.

## The capture + triage loop

The agent runs the emulator headless (see above) and mines the artifacts itself.
The one input the agent cannot synthesize is a PCSX-Redux save state at a desired
new location; capturing those interactively (a title screen, a post-name field
spawn) is where the operator helps. Each segment is one turn of the loop:

1. **Author** the segment: pick the start `.sstate`, the gap-set window, and any
   `LEGAIA_INPUTS` / `LEGAIA_MASH` timeline.
2. **Run** the probe headless (retry past the boot lottery); produce
   `trace_segment.csv` + `.modes.txt`.
3. **Triage** the CSV: drop addresses already documented (the worklist already
   excludes them), then for each new hit in hit-count order, read the dump
   (`ghidra/scripts/funcs/<addr>.txt`; extend `dump_funcs.py` TARGETS if
   missing), understand it, and document it in the right subsystem/format doc +
   a `functions.md` row (+ a `// PORT` / `// REF` tag if a crate consumes it).
4. **Catalogue** the end state as the next segment's start (back it up, add a
   `scenarios.toml` row, cite the `backup_fingerprint` in the ledger below) and
   advance the gap-burndown.

## Segment ledger

Each segment anchors on a start save state and produces an end save state that
becomes the next segment's start. Save states are gitignored Sony RAM - cite the
`backup_fingerprint` from `scripts/scenarios.toml`, never raw bytes. Status:
PENDING (authored, not yet run), CAPTURED (artifact in hand), DOCUMENTED (residue
triaged).

| Seg | Span | Start anchor | Status | New hits | Documented |
|---|---|---|---|---:|---:|
| S1 | cold boot -> title -> NEW GAME -> opening prologue (`opdeene`) | cold boot (`-fastboot`) | CAPTURED + CATALOGED (`s1_newgame_field`) | - | - |
| S2 | opening prologue -> Rim Elm (`town01`) | S1 checkpoint | CAPTURED + CATALOGED (`s2_rimelm_town01`) | - | - |
| S3 | first free walk (Rim Elm) | S2 checkpoint | CAPTURED + CATALOGED (`s3_rimelm_freeroam`); was [name entry](#s3-captured-the-town01-opening-is-the-name-entry-screen) | - | - |
| S4 | first scene transition / house door | S3 checkpoint | CAPTURED + CATALOGED (`s4_rimelm_door_transition`); [grid-BFS door-nav out of Vahn's house](#s4-captured-the-grid-bfs-door-nav-walks-out-of-vahns-house) | - | - |
| S5 | first battle (scripted Tetsu spar) | S4 end state | CAPTURED + CATALOGED (`s5_tetsu_battle`); [the scripted Tetsu spar, reached by record-then-replay of a human playthrough](#s5-the-first-battle-is-the-scripted-tetsu-spar-not-a-random-encounter) | - | - |

Both anchors are cataloged in `scripts/scenarios.toml` + `saves/library` by
`backup_fingerprint`, resolvable via `run_probe.sh --scenario <label>`.

**The universal field tick made chaining work.** The breakthrough was a second
per-frame exec-bp on `FUN_8001698C` (the default mode handler's vsync-sync,
`FUN_80025EEC`), which fires every frame at field-run + 12-13 of 14 modes - where
the title-tick BP (`FUN_801DD35C`) stops. The driver runs both:
`FUN_801DD35C` navigates the title (START+CROSS), `FUN_8001698C` drives the
in-game advance (CROSS-only) + target-detection + checkpoint, both regardless of
GPU::Vsync delivery. Two consequences:

- **S1 is now captured at field-RUN** (scene `opdeene`, after the scene load +
  a 20-tick settle), not the fragile field-INIT - it resumes cleanly (the
  field-INIT capture segfaulted within ~180 vsyncs on resume).
- **S2 was reached by chaining forward from S1 in one continuous resumed run**:
  the field tick CROSS-mashed through the opening prologue scenes
  (`opdeene` -> `opstati` -> `opurud` -> `map01` -> `town01`, ~3500 frames, no
  crash), and `LEGAIA_CKPT_SCENE=town01` checkpointed at Rim Elm. So
  segment-chaining-by-resume *is* viable - the earlier "blocked" reading was a
  consequence of the fragile field-INIT anchor + the missing field tick.

S3+ were *intended* to chain the same way (resume the previous scenario, drive
with the field tick, checkpoint on the next scene/mode). **S3 does not chain by
input mashing** - the town01 opening is a non-dialogue script wait, detailed
below. S4 chains from S3 by the grid-BFS door-nav (`autorun_s4_doornav.lua`); S5
chains from the S4 exterior anchor.

### S3 captured: the town01 opening is the name-entry screen

**Resolved + captured.** The `s2_rimelm_town01` "opening" stall is the
**"Select your name." name-entry screen** for Vahn - not a mysterious cutscene
wait. Driving it to completion reaches first free-roam in Rim Elm, captured as
the catalogued anchor **`s3_rimelm_freeroam`** (resumes to `game_mode 0x03`,
scene `town01`, player engaged flag `0x80000` clear). The pad timeline that
completes it is at the end of this section.

The trace that pinned it, layer by layer:

1. **Engaged, no dialogue.** With [`autorun_s3_recon.lua`](../../scripts/pcsx-redux/autorun_s3_recon.lua):
   the player engaged flag `*0x8007C364 +0x10 & 0x80000` is set and never clears;
   the field-control dialog byte (`*0x801C6EA4 +0x62`), picker cursor (`+0xc`) and
   interact flag (`+0x60`) are all `0`. No input advances it - a full button
   sweep and a sustained CROSS+CIRCLE mash (8000 frames from the anchor, and
   ~18000 ticks driving **continuously from S1**, timeline never save-interrupted)
   all leave it frozen. So the wait is inherent, and it is not a dialog/picker.
2. **The parked instruction.** A field-VM PC histogram ([`autorun_s3_pc.lua`](../../scripts/pcsx-redux/autorun_s3_pc.lua),
   BP on `FUN_801DE840`) shows the engaged context re-enters at one constant
   `pc=0x02C6` every frame; its runtime byte signature locates it uniquely at
   **town01 partition-2 record 3, `+0x02C6`, opcode `49 03`** = `STATE_RESUME`
   (op 0x49) subtype 3.
3. **The hand-off.** `STATE_RESUME` is a tristate on `_DAT_8007B450` (`FUN_801DE840`
   case 0x49): Idle arms it + spawns the effect-actor `FUN_80020DE0(0x8007065C,…)`;
   **Armed** halts at the same PC every frame; Done (`==1`) advances. The spawned
   actor's handler is `FUN_801F159C` (0897; dumped via
   `ghidra/scripts/dump_state_resume_overlay.py`): it runs the inner sub-state
   dispatch `PTR_FUN_801F33B4[actor+0x50]` and writes `_DAT_8007B450 = 1` (Done)
   **only when `*(_DAT_801C6EA4 +0x3E) == 0`**. Its worker `FUN_801F1278` sets
   `scene+0x3E = 1` and `actor+0x50` into the dispatch chain.
4. **The parked sub-state.** A runtime histogram of `actor+0x50` at `FUN_801F159C`
   ([`autorun_s3_substate.lua`](../../scripts/pcsx-redux/autorun_s3_substate.lua))
   shows the actor parked in sub-state **`0x22`** with `scene+0x3E` stuck at `1`
   and `_DAT_8007B450` Armed. `PTR_FUN_801F33B4[0x22] = FUN_801F03F0` - the
   **name-entry state machine** (`docs/reference/functions.md` `801F03F0`: the
   char-grid "Select your name." screen; substate at `struct+0x54`).

So the field op `49 03` suspends the opening script and hands off to name entry;
name entry holds the player engaged (no dialog box - hence `+0x62`/`+0x60`/pager
all idle) until a name is entered + confirmed, at which point it drives
`scene+0x3E -> 0`, the effect-actor sets `_DAT_8007B450 = 1`, and the field VM
unparks. Naive CROSS-mashing only appends grid glyphs (a garbage name) and never
selects the accept option, so it stalls forever - the targeted driver below
completes it. This was already pinned statically in `functions.md`; the trace
confirms it from the runtime side and identifies the exact parked sub-state.

**Reproduce the disassembly.** town01's MAN is `PROT[4]`, partition counts
`[36, 53, 39]`; the name-entry hand-off lives in partition-2 record 3:
`legaia-engine man-scripts --scene town01 --disc <bin> --disasm-partition 2
--disasm-record 3`. The 0897 effect-actor handler chain (`FUN_801F159C` +
`FUN_801F1278` + dispatch table `PTR_FUN_801F33B4` + the parked sub-handler
`FUN_801F03F0`) is dumped by `ghidra/scripts/dump_state_resume_overlay.py`
against `overlay_0897.bin.0`.

**The pad timeline that captures S3** ([`autorun_s3_capture.lua`](../../scripts/pcsx-redux/autorun_s3_capture.lua)).
The name grid (read live via [`autorun_s3_namegrid.lua`](../../scripts/pcsx-redux/autorun_s3_namegrid.lua))
is 17 cols x 7 rows; the cursor (`0x8007BB88`) already sits on `End` (idx 116,
bottom-right) and the name buffer holds the default "Vahn". The confirm button is
**CROSS** - not by assumption: the interactive handler `0x801F0480` selects when
`_DAT_8007B874 & *(0x800846D0)` is nonzero, and the configured select mask
`*(0x800846D0) = 0x44` matches CROSS's `0x40` ([`autorun_s3_btnmask.lua`](../../scripts/pcsx-redux/autorun_s3_btnmask.lua)).
Sequence: press CROSS on `End` -> the inner sub-state `actor+0x54` opens the
Yes/No confirm (`2`/`4`). Its selection is the toggle `_DAT_8007B458`; the
confirm sub-handler `0x801F097C` shows that toggle `!= 0` **loops** (`actor+0x50`
stays `0x22`) while toggle `== 0` **advances** the outer state to `0x1A` (out of
name entry). So the driver holds `_DAT_8007B458 = 0` (selecting the accept
option, equivalent to navigating to it) and pulses CROSS. The opening then plays
~1300 more frames, the engaged flag clears, and the driver checkpoints first
free-roam in `town01`. Catalogued as `s3_rimelm_freeroam`; re-decode/resume via
`run_probe.sh --scenario s3_rimelm_freeroam`. (The clean-room mirror of this
screen is `legaia_engine_core::name_entry`.) Encounter timing (S5) is
RNG-sensitive - keep it a short standalone segment.

### S4 captured: the grid-BFS door-nav walks out of Vahn's house

`s4_rimelm_door_transition` is captured + cataloged by **`autorun_s4_doornav.lua`**,
a grid-BFS door-navigation controller. From `s3_rimelm_freeroam` the player spawns
**inside a walled room** (Vahn's house interior); the controller walks it to the
front-door tile, where a walk-touch warp jumps the player
`(4134,10588) -> (3264,3520)` - tile `(32,82) -> (25,27)`, an intra-`town01`
warp out into the open village exterior (the post-warp grid map is a large bounded
town, not the small interior room). It settles at mode `0x03` free-roam and
checkpoints. Faithful playthrough: D-pad + the interact button only, real
collision, **no position pokes**.

How it works:

1. Read the per-scene walkability grid at `*(_DAT_1f8003ec)+0x4000` (1 byte /
   128-unit tile, `0x80`-byte rows; high nibble = 4 sub-cell wall bits) and the
   player tile from `player+0x14`/`+0x18`. BFS the reachable walkable tiles from
   the player tile (159 reachable in the house interior), then collect the
   **boundary** tiles (a reachable tile touching a wall - door triggers live on
   the edge) and visit them nearest-first.
2. Follow each BFS path with **online-adaptive** pad input: keep a per-pad-button
   EMA of its observed world `(dX,dZ)` and each frame press the button whose
   direction best matches the vector to the next path tile (handles a rotated
   camera). Pulse CROSS throughout (walk-touch doors + NPC story triggers), and at
   each boundary tile nudge toward the adjacent wall - where the warp fires.
3. A transition = the scene name leaves `town01` **or** the player position jumps
   `> 300` units in a single field tick (the warp signal that actually fired here:
   a `7938`-unit jump). Settle, then checkpoint a raw save state.

**Root cause of the earlier "this is impossible" dead-end (a measurement bug, not
game behaviour).** The prior nav attempts read `player+0x14`/`+0x18` as **`u32`**.
But `+0x14` (X, `s16`) and `+0x18` (Z, `s16`) each have a **16-bit** field
*immediately after* them - `+0x16` is the facing word - so a `u32` read folds the
facing into the high 16 bits of the coordinate. Live proof from
`autorun_s4_gridrecon.lua`: at the house spawn the clean read is `X=4160`, the
`u32` read is `4286582848` (`= facing 0xFF80 << 16 | 4160`). Every displacement
measurement was corrupted the instant the player turned. The two conclusions that
bug produced are **both false** and are retracted:

- *"Positions are 16.16 fixed, spawn `(-128.06, 0.18)`"* - **no**, they are plain
  `s16` 1-unit world coordinates (spawn tile `(32,92)`, byte-walkable); the
  `-128.06` was the facing word read as a fixed-point fraction.
- *"The camera-remap is dynamic - the same pad reaches different Z as the player
  moves"* - **no**, with the clean 16-bit read the facing word holds constant
  through a direction hold (`-128` unchanged across an entire press) and the pad
  maps to world consistently (`RIGHT -> +X`). The "dynamic" wandering was the
  facing word leaking into the position. The controller still estimates pad->world
  online (cheap insurance against real camera yaw between rooms), but the static
  per-room camera is why a simple greedy follow converges.

The lesson generalises: **when a probe reads a struct field, match the field's
real width** - the locomotion position fields are `s16`, and the neighbouring
facing word silently poisons a `u32` read. The clean grid + correct field width is
what turned "blind coverage exhausted" into a first-attempt capture.

### S5: the first battle is the scripted Tetsu spar, not a random encounter

The S5 span as originally written ("first random encounter") does not match the
opening: **Rim Elm (`town01`) has no random encounters at this story point.**
`autorun_s5_encounter.lua` wandered the exterior for ~148 tile-steps from the S4
anchor with `game_mode` pinned at `0x03` the whole time and no battle. The town's
encounters are **story-gated**, not absent: the MAN declares formations, and they
switch on **briefly after a later story-dialogue beat**, go peaceful again, and
return briefly **near the endgame** - so the encounter table is real but inactive
during the post-name-entry free-roam the S4 anchor sits in. The actual first
battle in the opening is the **scripted Tetsu sparring tutorial**
(`formation_id` 4), started by **talking to the sparring partner** - the same
fight the existing `v0_1_*_tetsu` anchor chain captures
(`v0_1_pre_battle_tetsu` -> `v0_1_tetsu_dialogue_accept` -> `v0_1_battle_start_tetsu`
-> ... -> `v0_1_post_battle_tetsu_town`).

How S5 was reached:

- **Battle detector pinned.** A battle is live when `game_mode` (`0x8007B83C`) is
  `0x15` **or** the battle-context pointer `0x8007BD24` is non-zero (`0` in the
  field; `0x800EB654` while a battle is resident). The capture runs from the
  **field-tick exec-BP** (`FUN_8001698C`), which keeps firing through this battle -
  a `GPU::Vsync`-only capture missed it (the Vsync clock did not advance the
  capture while the field tick did).
- **Tetsu located + reached.** `rimelm_npc_press_tetsu` pins the sparring partner
  at world `(2752,1856)` = tile `(21,14)`. The human playthrough (and the auto-nav
  `autorun_s5_tetsu.lua`) walks there from the S4 spot (tile `(25,27)`); the route
  passes through a door warp into Tetsu's sub-area before reaching him.
- **Captured by record-then-replay of a human playthrough.** The auto-driver
  (`autorun_s5_spar.lua`) reaches Tetsu but never started the spar - it only
  mashed CROSS and never navigated **down to the 3rd option** of Tetsu's list (his
  prompt is a few text boxes then a **4-item list whose 3rd entry is the training
  fight**, not a Yes/No; and `*(0x801C6EA4)+0x62` is a typewriter sawtooth, not the
  picker, so the auto-driver had no picker signal). The fix was to let a human play
  the route once and replay it: `autorun_record_inputs.lua` logs the per-frame
  button mask `0x8007B850` while a person walks S4 -> Tetsu -> his dialogue -> 3rd
  option -> start; `autorun_replay_inputs.lua` reproduces it deterministically by
  driving the pad via `pad.force` (RAM writes to `0x8007B850` don't stick -
  `FUN_8001822C` rebuilds it from the actual pad after the field-tick BP). The
  replay reaches `game_mode 0x15`, `battle_ctx 0x8007BD24 = 0x800EB654` (resident)
  over `town01`; `s5_tetsu_battle` is the validated checkpoint.

  **This retracts the earlier "off the scripted path / not story-armed" reading.**
  The spar *is* reachable from the S4 door-nav exterior - the human playthrough
  walks straight to Tetsu and the list option `3` starts the fight. The auto-nav's
  failure was a **dialogue-navigation gap** (mash-only, no list-cursor control),
  not story-gating and not a wrong sub-area. (Rim Elm's *random* encounters remain
  story-gated and off here - that part stands; it is the scripted spar that is the
  first battle, and it is available.)

  The **record/replay tooling** (`autorun_record_inputs.lua` +
  `autorun_replay_inputs.lua`, mask layout pinned by `autorun_btnmap.lua`) is the
  reusable primitive for any segment that needs a human-played step turned into a
  reproducible anchor. Two facts it rests on: `0x8007B850` is the byte-swapped PSX
  controller word (UP=`0x1000`, DOWN=`0x4000`, CROSS=`0x0040`, ...), and the
  battle capture must run off the **field-tick clock** (the `FUN_8001698C` BP keeps
  firing through this battle while a Vsync-only capture missed it).

## Gap-burndown

The headline metric. Snapshot the global gap-set size after each documentation
pass so the trend is visible. Regenerate with the worklist generator (the count
is printed) or `scripts/ci/port-catalog.py --dashboard`.

| Checkpoint | Gap-set size | SCUS | Overlay | Notes |
|---|---:|---:|---:|---|
| program start | 780 | 167 | 613 | - |
| field/mode-24 trace | 780 | 167 | 613 | 42 SCUS + 60 overlay functions confirmed live; SCUS-low mostly infra; overlay hits attribution-pending. Checkpoint records *what executes*. |
| dance-cluster deep-dive | 762 | 161 | 601 | Mode-24 pinned = Noa dance overlay 0980 (resident slot-A help text + sub-id 0x06). Documented the dance-floor render cluster (`FUN_801d2a10`/`801d3f54`/`801d3ec0`/`801d3a2c` + interior PCs) in [`minigame-dance.md`](../subsystems/minigame-dance.md); identified the SCUS-low infra hits (SPU queue drain, heap allocator, angle-lerp). -18 from the gap-set. |
| field-0897 deep-dive | 762 | 161 | 601 | No net burndown - the hot field matches resolve to the already-documented per-actor tick path (validation that the trace surfaces the central per-frame actor loop). Promoted the per-actor dispatcher `FUN_8003BC08` to the canonical `functions.md`; surfaced the `FUN_801D79E8` mesh-vs-glyph open thread. |
| S1/S2 anchor SCUS trace | 739 | 138 | 601 | First trace against the **reproducible** cataloged anchors (`s1_newgame_field` + `s2_rimelm_town01`), not an ephemeral save. 43 SCUS gap-set functions hit (union). Resolved the 23 always-resident S1 hits: **18 -> ignore-set** (PsyQ libgte/libcd/libc + libgpu prim composers + dev-profiler HUD + a noop stub), **4 -> `functions.md`** (field footstep/ambient + timed audio-cue ticks, a guarded sub-dispatch), 1 already documented (`8005A5FC` = the `FUN_8005A4A0` flusher interior). -23 from the gap-set (all SCUS). |
| S2 scene-load characterization | 719 | 118 | 601 | Characterized the 20 S2-only town scene-load callees (callees of the per-stage loader `FUN_8001E1B4` / boot mode-init `FUN_8001DCF8` / field init `FUN_801D6704`). **14 -> `functions.md`** (new "Scene / stage init" section: overlay-slot teardown, tile visibility/adjacency build, actor node-pool init/pop, field-camera reset, scene-script-ref binding, overlay-sprite pair, GTE projection-scale), **6 -> ignore-set** (2 retail-stripped noop tile emitters, libc InitHeap + coalescing-free, libgte SetTrans-vector + SetColorMatrix). -20, all SCUS. |

Each documented function moves an address out of the gap-set on the next
regenerate; the table above grows one row per triage pass.

### First-capture finding (SCUS-167, field/mode-24 state)

The first headless capture armed the 167 SCUS gap-set addresses against a
field-then-mode-24 save (`game_mode` `0x03` -> `0x18` -> `0x19`); 42 functions
hit. Triaging the hottest by decomp showed the **SCUS-low residue is dominated
by host-replaced library infrastructure**, not game logic:

- `FUN_8005a4a0` - SPU/DMA transfer-queue drain (64-entry ring, spins on the
  hardware-ready bit).
- `FUN_8002b468` - best-fit heap allocator (free-list walk + block split).
- `FUN_8005fde8` / `FUN_8006a7c8` - fn-ptr dispatch wrapper + a GPU/IO table
  bit-set helper.
- `FUN_8001d088` - a genuine engine helper: 12-bit angle lerp with wraparound
  (shortest-arc on a 4096-unit circle), writing interpolated facing into a
  per-slot table; the one clear game-logic helper in the hot set.

Implication for targeting: the game-logic residue concentrates in the **overlay**
gap-set (battle / field / menu) and higher SCUS functions, not these low-SCUS
helpers. Several SCUS hits carry an overlay-range caller `ra` (e.g. `8003d0bc`
called from `0x801D2D28`), i.e. SCUS leaf helpers invoked by overlay code - so
the overlay passes are where the burndown should focus.

### Overlay capture (613 addrs, 6 windowed passes, same field/mode-24 state)

The 613 overlay gap-set addresses were captured as a union of six ~120-bp
address windows (each armed first-try - the windowed count is well under the
ceiling). **60 functions hit**, split by `first_mode`: 39 at `0x03` (field), 21
at `0x18` (mode 24). The hottest field hits are `0x801D7B40` / `0x801D7A5C`
(~420 each, both called from SCUS `0x8003BC3C`) and the `0x801F7xxx` render band.

This pass is the concrete demonstration of the **VA-aliasing attribution rule**:
an overlay address has *multiple* dumps (e.g. `0x801D7A5C` carries 0897 / dance /
fishing / slot / debug_menu dumps), and only the one matching the resident
overlay is the real code. Resolution by `first_mode`:

- **mode `0x03` + `overlay_0897` stem = clean match** (the field overlay is 0897,
  resident at field mode). `0x801D7B40` is genuine field code - it branches on
  `_DAT_8007b450`, the documented tile-board-grid flag.
- **mode `0x03` + non-0897 stem = alias mismatch** (e.g. `0x801F7000`'s
  `magic_level_up` stem): the hit is real but the dump identity is wrong for this
  context; the resident code is the field/world-map overlay at that VA. Do not
  document from the mismatched dump.
- **mode `0x18` (mode 24) hits** need the resident mode-24 overlay pinned before
  attribution (the `0x801D2xxx`/`0x801D3xxx` cluster is a tight self-calling
  subroutine group; stems are mixed dance/menu).

Next deep-dive: the clean field-0897 matches, and pinning sstate1's mode-24
resident overlay (e.g. `asset overlay find-sig` on the hot cluster's prologue),
before writing per-function docs.

### Field-0897 matches (mode 0x03) - resolve to the per-actor tick path

The clean field hits (mode `0x03`, `overlay_0897` stem) resolve to the
**already-documented per-actor tick loop**, which is itself the finding: the
trace correctly surfaces the central per-frame actor driver. The two hottest
(`0x801D7A5C` / `0x801D7B40`, ~420 each, both called from `0x8003BC3C`) are
interior PCs of `FUN_801D79E8`, the field overlay's per-actor draw helper,
invoked by `FUN_8003BC08` - the per-actor tick for the `_DAT_8007C354` list.
`FUN_8003BC08` is the unifying driver that runs, per actor, the inline-dialogue
SM (`FUN_80039B7C`), the motion VM (`FUN_8003774C`), and the move-table VM; it
was documented only in two subsystem tables, so it is now promoted to the
canonical [`functions.md`](../reference/functions.md) directory with its full
verified dispatch list.

Open thread surfaced here: `FUN_801D79E8`'s precise render is unsettled -
[`field-locomotion.md`](../subsystems/field-locomotion.md) describes the
static-object actor as drawing its **mesh**, but the (interior-entry, incomplete)
decomp at `0x801D79E8` emits dialog-font glyph cells (`func_0x8003c1f8` cells
4/5) + a 3-digit number field (`func_0x80034b78`). A clean re-decompile from the
true entry is needed to reconcile these; not asserted either way.

### Cataloged-anchor SCUS trace (S1 opening / S2 Rim Elm, mode 0x03)

The first gap-set trace run against the **reproducible catalogued anchors**
(`run_probe.sh --scenario s1_newgame_field` / `s2_rimelm_town01`) rather than an
ephemeral live save, driven by `scripts/pcsx-redux/trace_scenario.sh` (one
windowed pass per address window, CROSS-mash to advance dialogue, union the
per-window CSVs). Captures land in `captures/trace/<label>/union.csv`.

- **S1 (`opdeene`, opening prologue):** 52 gap-set functions hit (23 SCUS + 29
  overlay), every window armed first-try.
- **S2 (`town01`, Rim Elm):** 111 hit (42 SCUS + 69 overlay). Richer than S1 -
  the town has more actors/NPCs, and the CROSS-mash walked into a door so the
  trace also captured the **scene-load path** (`first_mode 0x02`), e.g. the
  per-cell tile-sprite emitter (1024 hits during load).

**SCUS resolution (the burndown).** The 23 always-resident SCUS functions S1
surfaced were triaged by decomp. The honest split: most always-resident
per-frame field SCUS code is host-replaced infrastructure, not portable game
logic.

- **18 -> ignore-set** (`scripts/ci/port-catalog-ignore.toml`): PsyQ libgte
  (`8003D190` clear-translation, `8005B268` PushMatrix, `8005B4B8`
  set-translation, `8005B4E8` ScaleMatrix, `8005B648` SetLightMatrix), libcd
  (`8005CA34` CdSync, `8005CF80` CdControl, `8005FEDC` CD-status accessor), libc
  (`8002B688` heap block-size summer), libgpu prim-word composers (`80059280`
  sprite builder, `80059510` tpage, `80059744` texwindow, `8005AA30` GPU-queue
  timeout-arm), the dev-profiler HUD (`800173BC` + its `800178F0` / `8001A89C`
  marks and `8001ABC8` digit drawer), and a noop stub (`80019890`).
- **4 -> `functions.md`** (genuine field-tick logic, in `## Audio` / `##
  Helpers`): `80018DB0` (field footstep/ambient audio cadence tick), `80018F94`
  (positional-voice slot update), `800267FC` (timed audio-cue / event trigger),
  `8001D058` (guarded per-frame sub-dispatch to `FUN_80026CE4`).
- **1 already documented:** `8005A5FC` re-traces the interior of the
  GPU-queue flusher `FUN_8005A4A0` - confirmation the trace hits known code too.

Net: gap-set 762 -> 739 (SCUS 161 -> 138). Every overlay-range hit attributes to
the **field overlay 0897** (all `first_mode 0x03`); the misleading dump stems
(`overlay_dance_*`, `overlay_slot_machine_*`, ...) are just the static dump's
home overlay under VA-aliasing, not the resident code.

**S2 scene-load callees (now characterized).** The 20 S2-only SCUS hits are the
town **scene-load** path (mostly `first_mode 0x02`), all callees of the per-stage
asset loader `FUN_8001E1B4`, the boot mode-init `FUN_8001DCF8`, and the field
init `FUN_801D6704`. They split cleanly: the genuine Legaia scene-init logic
(overlay-slot teardown, tile visibility/adjacency rebuild, actor node-pool
init/pop, field-camera reset, scene-script-ref binding, GTE projection-scale)
went to the new "Scene / stage init" section of [`functions.md`](../reference/functions.md);
the host-replaced infra (two retail-stripped noop tile-sprite emitters, the libc
heap InitHeap/free pair, two libgte matrix-register loaders) to the ignore-set.
The high-overlay window `ov5` (top of the overlay range) is boot-lottery-flaky
for the S2 anchor and low-yield in the field state - those VAs host mostly
non-resident overlays - so S2's overlay union omits it.

**Cautionary note (the documented-classifier trap).** "Documented" =
*the address hex is cited from any file under `docs/`*, so naming a still-open
target in prose silently drops it from the gap-set. When recording a *pending*
target, refer to it by cluster/mode and leave the bare address in the capture
CSV + `gap_worklist.txt`, not under `docs/`; only cite the hex once it is
actually characterized.
