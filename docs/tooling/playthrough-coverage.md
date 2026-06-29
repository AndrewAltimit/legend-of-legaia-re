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
| S3 | first free walk + first NPC dialogue | S2 checkpoint | PENDING | - | - |
| S4 | first scene transition / house door | S3 end state | PENDING | - | - |
| S5 | first random encounter -> battle -> victory -> loot | S4 end state | PENDING | - | - |

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

S3+ chain the same way: resume the previous segment's scenario, drive with the
field tick, checkpoint on the next scene/mode. Encounter timing (S5) is
RNG-sensitive - keep it a short standalone segment.

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
