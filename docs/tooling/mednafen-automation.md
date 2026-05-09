# Mednafen automation

A scriptable substitute for mednafen's interactive memory-watchpoint
debugger. The toolkit treats each `.mc{0..9}` save state as a frozen RAM
snapshot, and uses pairwise diffs + targeted bisection to surface where
the runtime wrote between snapshots — the watchpoint-equivalent answer
without needing a live emulator session.

`crates/mednafen` provides the parser library + the `mednafen-state`
CLI; `scripts/mednafen/` contains the orchestrator scripts and the
declarative scenario manifest.

## Why scripted snapshots, not real breakpoints?

Mednafen's PSX module ships with a TUI debugger that supports memory
breakpoints, register stepping, and code tracing. None of those have a
scriptable interface — every debugger interaction is keyboard-driven
inside the running window. PCSX-Redux has a Lua scripting API but
requires running its own emulator process per session.

For most reverse-engineering work the interactive debugger is overkill.
What we usually want is *"between this point and that point, what
addresses got written?"*. Save-state diffs answer exactly that: take a
state before, take one after, diff the RAM. Any byte that changed was
written by code that ran in the gap. Cluster the changes into contiguous
regions and you have a ranked list of structures to look up writers for
in Ghidra.

## The save states

The user's `~/.mednafen/mcs/` directory holds 10 save states for the
retail US disc, each captured at a specific gameplay moment:

| Slot | Label              | Description                                  |
|------|--------------------|----------------------------------------------|
| mc0  | drake_castle       | Drake Castle (steady-state field with party) |
| mc1  | area_load_early    | Loading into a new area — early frame        |
| mc2  | area_load_mid      | Loading into a new area — mid frame          |
| mc3  | area_load_late     | Loading into a new area — late frame         |
| mc4  | battle_intro       | Loading into a battle (action menu)          |
| mc5  | battle_arts_view   | Viewing learned arts in battle               |
| mc6  | battle_anim_strike | Performing a somersault on an enemy          |
| mc7  | status_menu        | Status menu open                             |
| mc8  | options_menu       | Options/config menu open                     |
| mc9  | load_screen        | Save/load screen visible                     |

The progressive triplets (`mc1`/`mc2`/`mc3` for area-load,
`mc4`/`mc5`/`mc6` for battle) are deliberately bracketed so pairwise
diffs surface the writes done by each phase of the action.

Slot → label → watchpoint mapping is declared in
[`scripts/mednafen/scenarios.toml`](../../scripts/mednafen/scenarios.toml)
and consumed by the `mednafen-state watch` subcommand.

## Quick reference

### List the manifest

```bash
target/release/mednafen-state scenarios --manifest scripts/mednafen/scenarios.toml
```

### Inspect one save's section table

```bash
target/release/mednafen-state info \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0"
```

Prints the indexed sections (`MAIN`, `GPU`, `SPU`, `CDC`, `MDEC`, `DMA`,
`TIMER`, `MDFNRINP`, `BIOS_HASH`, `MDFNDRIVE_00000000`) with sub-entry
sizes, the resolved CPU PC if present, and the 2 MiB main-RAM offset.

### Slice a PSX-virtual-address window

```bash
target/release/mednafen-state extract \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc4" \
    --start 0x801C0000 --end 0x80200000 --out /tmp/battle_overlay.bin
```

This is the structured replacement for
`scripts/extract-mednafen-overlay.py` — same anchor-based fallback when
the structured `MainRAM.data8` lookup misses, plus a MIPS-shape sanity
check.

### Diff two saves in the overlay window

```bash
target/release/mednafen-state diff \
    "$HOME/.mednafen/mcs/...mc1" \
    "$HOME/.mednafen/mcs/...mc2" \
    --start 0x801C0000 --end 0x80200000 --top 8
```

Output:

```text
[diff] mc1 <-> mc2
[diff] window 0x801C0000..0x80200000  merge_gap=16  min_changed=4
[diff] 20 regions, 10029 bytes changed total
[diff] top 8 by bytes_changed:
       start           end     changed   left -> right (16 bytes)
    0x801F69D8  0x801F8F02   8631       90FFBD27... -> 147B1F80...
    0x801FFC28  0x801FFFBE    542       C42B0880... -> FFFFFFFF...
    0x801CDB50  0x801CDCD9     89       08000100... -> 06000C00...
    ...
```

The largest region (`0x801F69D8..0x801F8F02`, 8631 bytes) is a 9 KB
overlay window the area-load wrote into — that's the new scene's code
or data. The smaller regions are scattered global-state updates.

### Pairwise diff against the whole manifest

```bash
scripts/mednafen/auto-capture.sh
```

For every scenario, runs all configured `[scenarios.watchpoints]` against
each `diff_against` sister state, writes per-scenario JSON to
`/tmp/legaia_watch_<label>.json`, and prints a one-line summary per
watchpoint.

### Bisect for a transition

```bash
scripts/mednafen/watchpoint-bisect.py \
    --addr 0x8007B888 mc0 mc1 mc2 mc3
```

Walks the four save states in order; reports the first one in which the
target address transitions to the "bad" predicate (default: nonzero).
Output reports either `BracketedAt { before_idx, after_idx }` (the gap
between two adjacent states bracketed the write), `AlreadyBadFromStart`
(the address was already populated in mc0), or `NeverBecameBad`.

### Trace one address across many states

```bash
scripts/mednafen/watchpoint-bisect.py --addr 0x8007BAC8 --trace mc0 mc1 mc2 mc3 mc4 mc5
```

Prints the u32 value at `0x8007BAC8` in each state — useful when you
want to *see* the value evolve before deciding what predicate to bisect
on.

### Walk every scenario through extraction

```bash
scripts/mednafen/state-walk.sh --import
```

For every scenario in the manifest, slices its overlay window into
`/tmp/legaia_overlay_<label>.bin` and (with `--import`) imports it as a
labelled program in the Ghidra container via
`scripts/import-overlay-named.sh`. One command, all 10 scenarios staged.

## Workflow patterns

### "Find what writes to X" between two known points

1. Pick a `(before, after)` pair that brackets the suspected write
   (e.g. mc4 = battle intro, mc6 = mid-animation).
2. Run `mednafen-state diff before.mc after.mc --start <region>
   --end <region+N>`.
3. The largest region in the output is the candidate. Note its address.
4. In Ghidra, search for stores to that address in the relevant overlay
   (Search → For Direct References, or `find_lui_writers.py` for
   LUI+ADDIU pairs).
5. The writer function is what to dump and document.

### "When did X become populated?" with progressive states

1. Take save states at progressive points during a sequence.
2. `watchpoint-bisect.py --addr X save0 save1 save2 ...`
3. The reported `BracketedAt { i, j }` says "between save i and save j
   the write happened". Tighten with more saves between i and j if
   needed (record an .mcm movie that replays the same scripted action
   to add intermediate frames).

### "Diff the same scene at two camera angles"

When the user has two saves that differ only in camera or cursor state,
the diff naturally surfaces the camera/cursor-state addresses. Useful
for finding `cursor_x` / `cursor_y` style globals that show up nowhere
in static analysis.

## Recording new scenarios

Mednafen movie files (`.mcm`) record bit-exact controller input from
frame 0. Replaying them produces deterministic emulator state at every
frame.

1. Boot mednafen with the disc image.
2. Play to the point you want recording to start.
3. `Shift+F5` starts recording.
4. Play through the sequence (open menu, trigger battle, etc.).
5. `Shift+F5` again to stop. The .mcm lands in `~/.mednafen/mcm/`.

Replay deterministically with:

```bash
scripts/mednafen/run-mednafen.sh disc.bin --state mc1 --movie movie.mcm
```

To capture a state at a specific frame, replay up to that frame, hit `F5`
to save into a free slot, then `F7` to load it back later. Repeat for
multiple frames during the same scripted action. The resulting save
slots are interchangeable with the manifest.

## The scenarios manifest

[`scripts/mednafen/scenarios.toml`](../../scripts/mednafen/scenarios.toml)
declares every scenario, its overlay slice, its watchpoint regions, and
its `diff_against` sister-slot list. The schema:

```toml
[defaults]
filename_pattern = "Legend of Legaia (USA).<HASH>.mc{slot}"

[[scenarios]]
slot = 1
label = "area_load_early"
description = "Loading into a new area — early frame"
topics = ["scene-bundle preamble"]
diff_against = [2, 3]

[scenarios.overlay_slice]
start = 0x801C0000
end = 0x80200000

[[scenarios.watchpoints]]
label = "scene_bundle_pool"
start = 0x80084540
end = 0x80084580
hint = "FUN_800243F0 BGM/sound block layout; pool[+0..+0x40]"
```

`mednafen-state watch <label>` runs the scenario's watchpoints against
each `diff_against` sister, writing a per-scenario JSON report.

## Adding a new scenario

1. Capture a save state in mednafen at the moment you care about (`F5`
   in a free slot 0..9, or another slot if you free one up).
2. Add a `[[scenarios]]` block to `scripts/mednafen/scenarios.toml` with
   the slot index, a short label, and a description.
3. Optionally add `[[scenarios.watchpoints]]` blocks for regions you
   suspect carry the writes the scenario should surface.
4. Optionally add `diff_against = [...]` listing sister scenarios for
   the auto-capture pass to compare against.
5. Run `target/release/mednafen-state watch <label>` to see what's in
   the watch regions.

## Cross-links

- [`overlay-capture.md`](overlay-capture.md) — how the resulting overlay
  slices get imported into Ghidra and analysed.
- [`extraction.md`](extraction.md) — disc-side extraction; runs upstream
  of save-state work.
- [`crates/mednafen/README.md`](../../crates/mednafen/README.md) — the
  crate's API contract.
