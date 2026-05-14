# Mednafen automation

A scriptable substitute for mednafen's interactive memory-watchpoint
debugger. The toolkit treats each `.mc{0..9}` save state as a frozen RAM
snapshot, and uses pairwise diffs + targeted bisection to surface where
the runtime wrote between snapshots - the watchpoint-equivalent answer
without needing a live emulator session.

`crates/mednafen` provides the parser library + the `mednafen-state`
CLI; `scripts/mednafen/` contains the orchestrator scripts and the
declarative scenario manifest.

## Why scripted snapshots, not real breakpoints?

Mednafen's PSX module ships with a TUI debugger that supports memory
breakpoints, register stepping, and code tracing. None of those have a
scriptable interface - every debugger interaction is keyboard-driven
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

The toolkit operates on mednafen `.mc{0..9}` save states stored under
`~/.mednafen/mcs/`. Each save is a frozen RAM snapshot at a specific
gameplay moment - the slot number itself is ephemeral, so the
toolkit identifies scenarios by **label** rather than slot index.

Slot → label → watchpoint mapping is declared in
[`scripts/mednafen/scenarios.toml`](../../scripts/mednafen/scenarios.toml)
and consumed by the `mednafen-state watch` subcommand. Sister-state
pairings (e.g. "pre-encounter ↔ post-encounter", "pre-rank-up ↔
post-rank-up") are the primary unit of analysis: the diff between a
pair surfaces every RAM region touched in the gap between the two
captures.

Capture conventions:

- One save per scenario keeps diffs interpretable. A single state
  capturing two gameplay events at once (e.g. *both* the encounter
  trigger *and* a level-up) widens the diff window and forces the
  reader to disentangle two unrelated write streams.
- Town-resident, field-resident, battle-intro, and battle-active
  states are usually each worth keeping in the corpus - the engine
  pipeline maintains distinct RAM layouts across those modes.
- Save before *and* after any one-shot event you want to study
  (item use, magic-rank-up, character level-up, scene transition).
  The two states are then a diff pair.

The committed manifest carries the slot ↔ label mapping for the
canonical corpus; user-managed slots may differ.

## Quick reference

### List the manifest

```bash
target/release/mednafen-state scenarios --manifest scripts/mednafen/scenarios.toml
```

### Inspect one save's section table

```bash
target/release/mednafen-state info \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.<SLOT>"
```

Prints the indexed sections (`MAIN`, `GPU`, `SPU`, `CDC`, `MDEC`, `DMA`,
`TIMER`, `MDFNRINP`, `BIOS_HASH`, `MDFNDRIVE_00000000`) with sub-entry
sizes, the resolved CPU PC if present, and the 2 MiB main-RAM offset.

### Slice a PSX-virtual-address window

```bash
target/release/mednafen-state extract \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.<SLOT>" \
    --start 0x801C0000 --end 0x80200000 --out /tmp/battle_overlay.bin
```

This is the structured replacement for
`scripts/extract-mednafen-overlay.py` - same anchor-based fallback when
the structured `MainRAM.data8` lookup misses, plus a MIPS-shape sanity
check.

### Diff two saves in the overlay window

```bash
target/release/mednafen-state diff \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.<BEFORE>" \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.<AFTER>" \
    --start 0x801C0000 --end 0x80200000 --top 8
```

Sample output (for a pair bracketing a field → battle transition):

```text
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
overlay window the area-load wrote into - that's the new scene's code
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
    --addr 0x8007B888 <save1> <save2> <save3> <save4>
```

Walks the named save states in order; reports the first one in which
the target address transitions to the "bad" predicate (default:
nonzero). Output reports either `BracketedAt { before_idx, after_idx }`
(the gap between two adjacent states bracketed the write),
`AlreadyBadFromStart` (the address was already populated in the first
state), or `NeverBecameBad`.

### Trace one address across many states

```bash
scripts/mednafen/watchpoint-bisect.py --addr 0x8007BAC8 --trace <save1> <save2> ...
```

Prints the u32 value at `0x8007BAC8` in each state - useful when you
want to *see* the value evolve before deciding what predicate to bisect
on.

### Walk every scenario through extraction

```bash
scripts/mednafen/state-walk.sh --import
```

For every scenario in the manifest, slices its overlay window into
`/tmp/legaia_overlay_<label>.bin` and (with `--import`) imports it as
a labelled program in the Ghidra container via
`scripts/import-overlay-named.sh`. One command, all scenarios staged.

### Dump the runtime GPU VRAM as a PNG

```bash
mednafen-state vram-dump \
  ~/.mednafen/mcs/"Legend of Legaia (USA)."*".<SLOT>" \
  --out vram.png --out-bin vram.bin --regs
```

Decodes the `&GPURAM[0][0]` blob inside the save state's `GPU` section
(1 MiB BGR555 + STP) and writes it as a 1024x512 RGBA8 PNG plus the
optional raw byte blob. `--regs` adds the GPU control-register
snapshot (clip rect, draw offset, texture window, texture page, display
framebuffer) - the same registers the runtime is reading from at the
moment of capture. Useful as a ground-truth oracle for engine-side VRAM
state: pair with `legaia-engine info --scene <name> --runtime-vram
vram.bin --vram-diff-png diff.png` for a colour-coded per-pixel diff
against the engine's `SceneResources::build_targeted` output.

### Byte-match a battle_data pack against VRAM

```bash
mednafen-state clut-trace \
  --pack extracted/PROT/0865_battle_data.BIN \
  --json /tmp/clut_corpus.json \
  ~/.mednafen/mcs/"Legend of Legaia (USA)."*".<TOWN_SLOT>" \
  ~/.mednafen/mcs/"Legend of Legaia (USA)."*".<BATTLE_SLOT>"
```

LZS-decompresses every record in the named battle_data pack, slides a
32-byte halfword-aligned window past each record's embedded TMD, and
searches the save state's VRAM for an exact byte match. Each hit is one
`(record_idx, record_offset, fb_x, fb_y)` tuple - the corpus narrows
the encoding of the per-record post-TMD descriptor at `u32[3..0x20]`.
See [`docs/formats/battle-data-pack.md`](../formats/battle-data-pack.md)
for the analysis methodology and findings.

### Decode the per-prim renderer dispatch tables

```bash
mednafen-state prim-dispatch-table <save>
mednafen-state prim-dispatch-table <save> --overlay-targets-only
```

Decodes `FUN_80043390`'s SCUS-resident table at `0x8007657C` (4 alpha
rows × 20 slots) and the overlay-resident variant at `0x801F8968` (1
alpha row only - the overlay path skips the alpha offset). Reports
every populated slot, classifies it (SCUS / overlay / other), and
surfaces the eight overlay-resident high-mode renderers at
`0x801F7644..0x801F8690` - the per-prim emit leaves the world-map
top-view routes its TMD prims through. The overlay table reports as
empty when the world-map overlay isn't paged in; pass
`--overlay-targets-only` to pipe the eight addresses into a Ghidra
`dump_funcs.py` `TARGETS` list. See
[`docs/subsystems/world-map.md`](../subsystems/world-map.md#bulk-continent-terrain-emit-mechanism-pinned)
for the mechanism and
[`crates/mednafen/src/prim_dispatch.rs`](../../crates/mednafen/src/prim_dispatch.rs)
for the typed accessors.

### Survey dispatch tables across multiple saves

```bash
mednafen-state prim-dispatch-survey <save> <save>...
```

Runs `prim-dispatch-table` against multiple saves in one pass and prints
a side-by-side comparison. Useful after adding a new save capture, to
confirm:

- The SCUS-resident dispatch table is **byte-identical** across every
  save (it lives in code, so RAM writes can't legally touch it). The
  command exits non-zero if drift is detected.
- Which saves have the world-map overlay paged in (`status = POP`,
  eight high-mode targets in `0x801F76..0x801F86`) vs. saves where the
  overlay address space holds leftover code or zeros (`stale` / `empty`).
- Targets outside the documented `0x801C0000..0x801F9000` window flag
  with `(OTHER!)` - that's an early-warning sign the overlay window
  needs widening.

The same invariants are asserted as disc-gated tests in
[`crates/mednafen/tests/dispatch_table.rs`](../../crates/mednafen/tests/dispatch_table.rs);
the survey command is the one-shot equivalent for spot-checking.

### Pin SC-block fields by diffing two saves

```bash
target/release/save-tool sc-diff \
  ~/.mednafen/sav/"Legend of Legaia (USA).<HASH>.0.mcr" \
  ~/.mednafen/sav/"Legend of Legaia (USA).<HASH>.1.mcr" \
  --save-index 1 --coalesce 8
```

Diffs the two memory cards' SC save blocks and surfaces every
differing byte cluster, annotated against the documented SC-block
layout (`SC magic`, icon palette, location name, scene CDNAMEs,
etc.). The `global header (story_flags / inventory candidate)` band
is the one to watch when you're hunting the not-yet-pinned story-flag
word or inventory slot array: pick two saves bracketing a single
known state change (item picked up, story flag flipped, money
changed) and the cluster width inside that band tells you the field's
type (4 bytes &rarr; u32 story flags; 2-byte stride &rarr; inventory
`(item_id, count)` array).

`--coalesce N` merges runs of differing bytes whose gap is &le; `N`
into one cluster (default 8). `--range LO..HI` (hex or decimal)
restricts the scan; the default range skips the per-character record
region (`0x086F..`) since per-character changes are visible via
`save-tool character`. Either argument can be a raw 8192-byte SC-block
file or a `.mcr` memory-card image; the tool detects which.

Layout reference: [`docs/subsystems/save-screen.md#retail-sc-block-layout`](../subsystems/save-screen.md#retail-sc-block-layout).

## Workflow patterns

### "Find what writes to X" between two known points

1. Pick a `(before, after)` save-state pair that brackets the
   suspected write (e.g. a battle-intro state and a mid-animation
   state from the same encounter).
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
scripts/mednafen/run-mednafen.sh disc.bin --state <slot> --movie movie.mcm
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
label = "pre_encounter"
description = "Walking the field, one step before an encounter triggers (`map01`)"
topics = ["encounter table base", "field state", "navmesh"]
diff_against = [2, 3]

[scenarios.overlay_slice]
start = 0x801C0000
end = 0x80200000

[[scenarios.watchpoints]]
label = "battle_overlay_window"
start = 0x801CE808
end = 0x801F3818
hint = "133 KB battle overlay. Loaded between the pre-encounter and post-encounter sister states."
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

- [`overlay-capture.md`](overlay-capture.md) - how the resulting overlay
  slices get imported into Ghidra and analysed.
- [`extraction.md`](extraction.md) - disc-side extraction; runs upstream
  of save-state work.
- [`crates/mednafen/README.md`](../../crates/mednafen/README.md) - the
  crate's API contract.
