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
author/run hand-off. The reliable recipe is a **save-state-anchored** segment:
load a PCSX-Redux `.sstate` (the save jumps past the slow BIOS boot and fires
`GPU::Vsync` immediately, so the probe arms at once), with `boot_delay=2`:

```bash
LEGAIA_SSTATE="$HOME/Tools/pcsx-redux/SCUS94254.sstate1" \
LEGAIA_BOOT_DELAY=2 \
LEGAIA_LUA=scripts/pcsx-redux/autorun_trace_segment.lua \
LEGAIA_OUT=captures/trace/seg.csv LEGAIA_FRAMES=600 \
    xvfb-run -a timeout --kill-after=15s 220s bash scripts/pcsx-redux/run_probe.sh
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
- **Cold boot (`LEGAIA_NO_SSTATE=1`) is not viable headless.** The title is
  reached only after minutes of interpreter boot, and `GPU::Vsync` (the arming
  gate) does not fire during the pre-render CD-boot phase - the probe sits in
  `waiting for boot` indefinitely. Capture the S1 title/new-game code by
  anchoring on a title-screen `.sstate` instead (capture one interactively
  once), not by cold boot.
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
| S1 | cold boot -> title -> NEW GAME -> opening cutscene | cold boot (`LEGAIA_NO_SSTATE=1`) | PENDING | - | - |
| S2 | name entry -> Rim Elm (town01) spawn | S1 end state | PENDING | - | - |
| S3 | first free walk + first NPC dialogue | S2 end state | PENDING | - | - |
| S4 | first scene transition / house door | S3 end state | PENDING | - | - |
| S5 | first random encounter -> battle -> victory -> loot | S4 end state | PENDING | - | - |

S1 runs the full 780-entry gap-set (all buckets - no `LEGAIA_ADDR_HI` filter),
so its overlay hits need the mode/overlay disambiguation in
[Attribution](#attribution-scus-vs-overlay) before documenting; the 167 SCUS
hits are clean. **S1 cannot be captured by headless cold boot** (see the
reliability notes - the arming gate never fires during BIOS boot); anchor it on
a title-screen `.sstate` captured interactively. Name entry (end of S1) is
fiddly to script letter-by-letter;
capturing an "after name entry" save state as the S2 anchor is preferred over
scripting the input. Encounter timing (S5) is RNG-sensitive - keep it a short
standalone segment and expect a couple of attempts.

## Gap-burndown

The headline metric. Snapshot the global gap-set size after each documentation
pass so the trend is visible. Regenerate with the worklist generator (the count
is printed) or `scripts/ci/port-catalog.py --dashboard`.

| Checkpoint | Gap-set size | SCUS | Overlay | Notes |
|---|---:|---:|---:|---|
| program start | 780 | 167 | 613 | - |
| field/mode-24 trace | 780 | 167 | 613 | 42 SCUS + 60 overlay functions confirmed live; SCUS-low mostly infra; overlay hits attribution-pending (deep-dive next). No docs landed yet, so the gap-set is unchanged - this checkpoint records *what executes*, the input to burndown. |

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
