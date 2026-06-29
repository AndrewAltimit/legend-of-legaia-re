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
| `LEGAIA_FRAMES` | Capture budget in vsyncs. |

Run a segment (cold-boot S1 example - drive the title menu by hand or via
`LEGAIA_INPUTS`; wrap in `timeout` because PCSX-Redux does not reliably self-
quit):

```bash
LEGAIA_NO_SSTATE=1 \
LEGAIA_LUA=scripts/pcsx-redux/autorun_trace_segment.lua \
LEGAIA_OUT=captures/trace/s1_boot.csv \
LEGAIA_FRAMES=5400 \
    timeout --kill-after=10s 420s bash scripts/pcsx-redux/run_probe.sh
```

For a save-state-anchored segment (S2+), drop `LEGAIA_NO_SSTATE` and pass
`--scenario <label>` (or `LEGAIA_SSTATE=<path>`); scripted `LEGAIA_INPUTS` time
reliably against the dense in-game vsync clock.

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

## The latency loop

The author/run split is structural: the agent authors probes + input timelines
and mines artifacts; the operator runs PCSX-Redux with the disc. Each segment is
one turn of the loop:

1. **Author** the input timeline + run command for the next segment.
2. **Operator runs** the probe; hands back `trace_segment.csv` + `.modes.txt` +
   an end-of-segment save state.
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
hits are clean. Name entry (end of S1) is fiddly to script letter-by-letter;
capturing an "after name entry" save state as the S2 anchor is preferred over
scripting the input. Encounter timing (S5) is RNG-sensitive - keep it a short
standalone segment and expect a couple of attempts.

## Gap-burndown

The headline metric. Snapshot the global gap-set size after each documentation
pass so the trend is visible. Regenerate with the worklist generator (the count
is printed) or `scripts/ci/port-catalog.py --dashboard`.

| Checkpoint | Gap-set size | SCUS | Overlay |
|---|---:|---:|---:|
| program start | 780 | 167 | 613 |

Each documented function moves an address out of the gap-set on the next
regenerate; the table above grows one row per triage pass.
