# PCSX-Redux automation

PCSX-Redux ships a Lua scripting API + a breakpoint debugger over the live
PSX CPU. The `scripts/pcsx-redux/` directory contains closed-loop probes
that use this combination to answer questions static analysis can't:
*"what code reads this address?"*, *"when does this RAM region get
populated?"*, *"what's the dispatch path between two functions?"*.

The same shape applies across the catalogue: a Lua autorun script
loads a save state, arms a set of breakpoints, captures N VSyncs of
data, and writes a CSV / snapshot file. A wrapper shell script
launches the emulator headless with the right flags.

This page documents the pattern, the harness, and the catalogue.

## Why PCSX-Redux

Three properties make it the right tool for runtime probes:

- **Open-source + scriptable.** The Lua API exposes the CPU register
  file, main RAM as a file-like object, and a breakpoint manager.
- **Interpreter CPU + debug mode.** The interpreter (`-interpreter`)
  is the only CPU back-end that hits Lua breakpoints, and the
  interpreter only invokes the debug-process hook when
  `DebugSettings::Debug` is set (`-debugger`). Both flags are required;
  silently neither alone fires Lua breakpoints. (Source:
  `psxinterpreter.cc:1652` &mdash; `if constexpr (debug)`.)
- **Save-state load from Lua.** `PCSX.loadSaveState(zReader(file))`
  loads a `.sstate` file at runtime, which lets the autorun script
  reach any captured game state without driving the GUI.

Mednafen's binary save-state format is supported for offline RAM scans
via the [`mednafen-state`](mednafen-automation.md) crate, but its
runtime debugger is GUI-only; PCSX-Redux is where the breakpoint
probes run.

## Setup

The expected on-disk layout (matches the run-script defaults):

```
~/Tools/pcsx-redux/pcsx-redux                  # locally-built binary
~/Tools/pcsx-redux/<TITLE_ID>.sstate<N>        # PCSX-Redux quicksave (F1..F10 in-emulator)
~/.mednafen/firmware/SCPH1001.BIN              # PSX BIOS, reused from mednafen
~/Downloads/Legend of Legaia (USA)/            # disc image
```

The `<TITLE_ID>` is the PSX disc's product code (e.g. `SCUS94254` for the USA
release of Legaia); PCSX-Redux writes one file per quicksave slot when you
press the assigned F-key in the running emulator. Each probe's documentation
calls out which game state the save needs to be in — pick a save you've
prepared locally that matches.

Override any of these via env vars (`PCSX_REDUX`, `LEGAIA_BIOS`,
`LEGAIA_SSTATE`, `LEGAIA_ISO`). The repo doesn't ship the binary or
BIOS or disc; those stay local.

## The harness

[`scripts/pcsx-redux/run_world_map_probe.sh`](../../scripts/pcsx-redux/run_world_map_probe.sh)
is the canonical wrapper. Despite the name, every other Lua autorun
re-uses it via the `LEGAIA_LUA` override:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/<your-saved-state>.sstate \
LEGAIA_LUA=scripts/pcsx-redux/autorun_world_map_fog_probe.lua \
LEGAIA_OUT=/tmp/fog_probe.csv \
LEGAIA_FRAMES=600 \
    bash scripts/pcsx-redux/run_world_map_probe.sh
```

The wrapper:

1. Verifies the binary / BIOS / save state / Lua file all exist
   (fails early with a clear error if any one is missing).
2. Launches PCSX-Redux with `-interpreter -debugger -run -bios
   <SCPH> -iso <bin> -dofile <lua> -stdout` and pipes the emulator
   log to `logs/pcsx_<probe>.log`.
3. Tails the log for a `=== summary ===` block on exit.

The `-stdout` flag is what makes the autorun's `PCSX.log(...)`
calls visible to the parent shell.

## The probe pattern

Every autorun script under `scripts/pcsx-redux/` follows the same
state machine:

1. **WAIT_BOOT** &mdash; vsync listener counts up while the emulator
   boots the BIOS to a known state (typically 60 vsyncs = 1s).
2. **ARMED_LOADED** &mdash; load the save state, read the register
   file, compute breakpoint addresses (often GP-relative), arm the
   probes, write an initial snapshot. Capture for `LEGAIA_FRAMES`
   vsyncs while breakpoints log hits to the CSV.
3. **DONE** &mdash; disarm breakpoints, write a final snapshot,
   `PCSX.quit(0)`.

This pattern is factored out as a shared library at
[`scripts/pcsx-redux/lib/probe.lua`](../../scripts/pcsx-redux/lib/probe.lua).
A new probe doesn't reimplement the state machine, the memory readers,
the save-state loader, the pad-override helpers, the CSV writer, or the
live-snapshot writer - it imports them:

```lua
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local csv = probe.csv_open("/tmp/x.csv", "addr,pc,ra")

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", DEFAULT),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 600),
    snapshot_path  = "/tmp/x.hits.txt",
    on_arm = function()
        local descs = {}
        for _, addr in ipairs({ 0x801E76D4 }) do
            local d = { addr = addr, name = string.format("0x%08X", addr),
                        hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, d.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local r = PCSX.getRegisters()
                csv:row("0x%08X,0x%08X,0x%08X",
                    addr, tonumber(r.pc), tonumber(r.GPR.n.ra))
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,
    on_done = function() csv:close() end,
})
```

`probe.ram_offset(addr)` is `bit.band(addr, 0x1FFFFFFF)` &mdash; strips
the KSEG segment selector so KSEG0 (`0x80xxxxxx`) and KSEG1
(`0xA0xxxxxx`) map to the same physical byte. Always work in
absolute PSX virtual addresses on input; convert at the boundary.

### Call-context capture

`probe.capture_call_context(label)` returns a multi-line text snapshot
of the CPU at the moment of a breakpoint hit:

* All 32 GPRs by MIPS name (`zero`, `at`, `v0`, …, `ra`), four per
  row.
* The 8 instruction words straddling PC (`pc-0x20..pc+0x60`), one row
  per 16 bytes, with a `<- pc` marker on the row containing PC. Lets
  the reader see the calling instruction context without round-tripping
  through Ghidra.
* The 32 stack words at `sp` (`sp..sp+0x80`), 4 per row. The MIPS
  calling convention saves `ra` into a sp-relative prologue slot for
  any non-leaf function, so this captures the visible ra-chain
  without DWARF unwind info. Walking the chain still requires
  reading the prologue offsets out of the disassembly post-hoc, but
  the bytes you need to do that are already in the snapshot.

`probe.append_call_context(path, snap)` is the matching writer; it
opens the file in append mode so multi-shot probes can stack
snapshots without overwriting earlier ones. The slot-4 reader and the
XP-table probe both use this for first-hit detail dumps.

### Early-quit signal

`probe.run` polls `ctx.request_quit` each vsync and exits the
capture loop on the next tick if it's set. Probes use this to bail as
soon as their stop condition is met (e.g. every probe in a sweep has
hit at least once), instead of waiting for `LEGAIA_FRAMES` to elapse:

```lua
on_capture = function(ctx, _elapsed)
    if every_probe_hit() then
        ctx.request_quit = true
    end
end,
```

### Symbolic breakpoint addresses

Hard-coded `0x801DA51C`-style breakpoint targets break across overlay
re-imports that shift function entry points. Use the symbol resolver:

```lua
local symbols = require("symbols").load()  -- ghidra/scripts/symbols.lua
probe.arm_breakpoint(symbols.FUN_801DA51C, "Exec", 4, "world_map_sm", cb)
```

`ghidra/scripts/symbols.json` (canonical) and `ghidra/scripts/symbols.lua`
(LuaJIT-loadable convenience) are both auto-generated from the per-function
dump headers under `ghidra/scripts/funcs/*.txt`. Regenerate via

```bash
python3 scripts/pcsx-redux/build-symbols.py
```

after adding new dumps. The resolver fails loudly on a typo'd symbol
name &mdash; arming a breakpoint at `nil` otherwise silently captures
zero hits and the probe runs to completion with no diagnostic.

### Things that catch people out

- **Breakpoint width matters.** `lbu` from a watched word triggers
  only when the width-1 byte falls inside the breakpoint's range.
  Arming a width-4 probe at an LW target works; arming a width-1
  probe at an LBU target works; mismatches silently miss hits.
- **GP-relative addresses are decided at runtime.** A naive
  hard-coded address can be wrong across overlay swaps. Read `gp`
  from `PCSX.getRegisters()` after the save-state load, then
  compute breakpoint addresses from there.
- **Sign-extended u64s in Lua.** PCSX-Redux returns CPU register
  values as signed Lua numbers (64-bit doubles). `gp = 0xFFFFFFFF8007B318`
  is the sign-extended display of `0x8007B318`. Use `bit.band(v,
  0xFFFFFFFF)` to normalise before formatting.
- **In-RAM guard predicates.** Pure bitwise comparisons against
  literals like `0x80000000` interact with Lua's 32-bit signed
  return shape from `bit.band` &mdash; the literal is the
  unsigned 2147483648 while the bit-result is the signed
  -2147483648, so `~=` returns true even when the addresses match.
  Use the explicit `bit.band(addr, 0x1FFFFFFF) < RAM_SIZE` form
  from the existing helpers; don't reinvent it.

## Catalogue

The committed scripts live in
[`scripts/pcsx-redux/`](../../scripts/pcsx-redux/). Each Lua file
documents its purpose in a header comment block; the catalogue here
is the high-level index.

### Runtime probes (Lua autorun)

| Script | Probes | What it answered |
|---|---|---|
| `autorun_world_map_probe.lua` | Reads at `_DAT_8007BCD0..D8` (gate-arm params), gate flag `_DAT_801F351C` writes, and four `FUN_801D7EA0` entries | Pins the world-map POLY_FT4 emitter's one-shot gate flag + the three-param block driving it. |
| `autorun_world_map_fog_probe.lua` | Reads at five fog fields (GP-relative `-0x2E0 / -0x2DC / -0x2D1 / -0x2BC / +0x90`) + 1 KiB LUT dump | Captures the per-Z fog-tint LUT the overlay leaves at `0x801F7644..0x801F8690` consult on every vertex. |
| `autorun_prim_pool_writers.lua` | Writes across the 341 KB GPU prim pool at `0x800AD400+` | Confirms the eight overlay-resident high-mode renderers are the ones writing the pool (matches `FUN_80043390`'s dispatch table). |
| `autorun_cd_dma_probe.lua` | CD-DMA reads during a town &rarr; world-map transition | Disproved the "continent prim pool comes from CD-DMA" hypothesis. |
| `autorun_lzs_and_bundle_probe.lua` | LZS decode entries + bundle dispatcher (`FUN_8001F05C`) during world-map load | Pins which PROT entries get LZS-decoded for the world-map bundle. |
| `autorun_deep_pool_probe.lua` | Writes to the deep Buffer-A / Buffer-B region | Matches GPU prim-pool writes to the overlay emit leaves. |
| `autorun_slot4_readers.lua` | Reads at the live slot-4 RAM region (`0x8011A624+`) | **Drake-tuned**: probe offsets are tied to Drake's 15-body slot-4 layout, so they don't reliably land on records in Sebucus / Karisto. Pinned two distinct reader clusters at the Drake kingdom-bundle scene-load transition (held UP for 60 vsyncs from a Drake-on-map01 save): cluster A at `PC 0x80044B00..0x80045700` is a GTE-driven primitive emitter; cluster B at `PC 0x80059DE4` reads mid-body bytes. Steady-state dev-menu top-view reads nothing — the consumer only walks slot 4 at scene-load. `LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=60` drives the warp input from inside the probe. `LEGAIA_S4_DETAIL=1` adds a first-hit call-context dump (32 GPRs, code window around PC, 32 stack words at sp). `LEGAIA_S4_QUIT_AFTER_ALL=1` ends the capture as soon as every probe has logged at least one hit. For cross-kingdom verification use `autorun_slot4_consumer_pcs.lua` instead. |
| `autorun_slot4_consumer_pcs.lua` | Exec bps at the cluster-A + cluster-B LW PCs identified from the Drake Read-bp run | **Kingdom-agnostic**: hits the same SCUS function PCs regardless of where slot 4 lives in RAM for the destination kingdom. Confirmed cross-kingdom: cluster A and B fire on Drake, Sebucus (town → map02) and Karisto (town → map03) with the same caller RAs (cluster B's RA `0x80059C00` is byte-identical across all three; cluster A's RAs `0x8001B47C` inside `FUN_8001ada4` + `0x801F78D4` world-map overlay are present in every kingdom). Hit-count scales with per-kingdom record count. Output CSV is `probe_idx, cluster, pc, name, ra, a0..a3, s8`; `.detail.txt` sidecar captures first-hit call-context per PC. `LEGAIA_PC_CAP=N` raises the default 200-hit-per-PC cap for uncapped totals. |
| `autorun_slot4_dispatcher_args.lua` | Exec bp at `0x80043390` (cluster A dispatcher entry) | Captures the *original* call args before the kind handlers clobber `a1` / `a2`: caller RA, descriptor pointer `a0`, packed `cmd_flags` (`a1`), `fade_flags` (`a2`), and the first command word's `kind` / `count`. Use this to classify which of the four dispatcher banks (`0x00` / `0x50` / `0xA0` / `0xF0`) each call lands in; the `consumer_pcs` probe records register state inside handlers and can't recover the original args. `LEGAIA_DISP_CAP=N` raises the default 200000-hit cap. |
| `autorun_slot4_transcoder_hunt.lua` | Write bps across the `0x801BA000`-ish working buffer where cluster A's per-frame inputs live | Cross-kingdom Exec-bp captures show cluster A reads from `0x801BA000`-region per frame, not from slot 4's documented base — initial hypothesis was that slot 4 was transcoded into that buffer during world-map scene load. Probe ruled this out: writes at `+0x7F8` / `+0x8E4` come from `FUN_80028158`, a **per-frame procedural mesh builder** that reads only actor+0x9C struct fields (NOT slot 4). Deeper writes at `+0x6000` come from `FUN_8001E54C` (streaming chunk processor). Per-frame cluster-A is procedural rendering, not slot-4 walks. `LEGAIA_WB_BASE=0x801BA000` is overridable; `LEGAIA_WB_CAP=50` per-bp cap. |
| `autorun_slot4_loader_hunt.lua` | Write bps tiled across slot-4 RAM (`0x8011A624+` for Drake, same offsets as `autorun_slot4_readers.lua`) | Identifies the function that **populates slot-4 RAM** during the kingdom warp transition. Result: all writes come from `FUN_8001A55C` (the LZS decoder) at PC `0x8001A604` etc., called via the standard asset-dispatcher chain (`FUN_8001F05C` in the call stack). Slot 4 is just LZS-decoded verbatim into RAM — no special transcoder. `LEGAIA_LOAD_CAP=50` per-bp cap. |
| `autorun_dump_slot4.lua` | Dumps the slot-4 RAM region directly | Sister to the readers probe; produces the ground-truth byte buffer for `verify_slot4_in_ram.py`. |
| `autorun_xp_table_reader.lua` | Read bps tiled across the XP increment table at `0x8007123C..0x80071300` (98 u16 entries) | Pins the runtime XP-table reader function. Static LUI+ADDIU scans return zero hits across SCUS + every captured overlay; the reader either lives in an unimported overlay or builds the pointer indirectly. The probe writes a CSV of every hit + a `.detail.txt` sidecar with call-context for the first 8 hits (so the leveling formula's call site can be lifted into the engine). |
| `autorun_field_pack_projection.lua` | Exec bp at `FUN_8001F7C0` (scene asset loader) entry; one-shot Exec bp at the loader's return address; dumps post-load RAM window | Captures the loader's on-disc &rarr; RAM projection that a single save state can't observe. `LEGAIA_HOLD_BUTTON` / `LEGAIA_HOLD` drive the warp-tile input from inside the probe; the run quits ~30 vsyncs after the first post-load dump so the capture window terminates as soon as the data is on disk. Diff via [`scripts/diff_field_pack_projection.py`](../../scripts/diff_field_pack_projection.py) against the on-disc PROT bytes. Output is a per-slot diff over the canonical 97-slot field-pack schema, sorted by changed bytes. World-map scenes (`map01` / `map02` / `map03`) are not field-pack-formatted - running against them produces a 75 KB GP0-primitive pool projection at `_DAT_8007B8D0 - 0x12800` instead, useful as the consumer-side counterpart to the slot-4 readers probe. |
| `autorun_dump_full_ram.lua` | Dumps the full 2 MiB main RAM | One-shot snapshot for downstream analysis. |
| `log_world_map_vm.lua` | Exec breakpoint on `FUN_801D362C` | Surfaces calls into the world-map drawing VM dispatcher. |
| `probe_world_map_callchain.lua` | Multi-PC exec hooks | Diagnostic: traces why `log_world_map_vm.lua` saw zero dispatches. |

### Save-state to Python (offline analysis)

| Script | Input | Output |
|---|---|---|
| `dump_kingdom_ram_layout.py` | `.sstate` files for the three kingdoms | Per-kingdom RAM-layout JSON used by the `world-overview` page. |
| `walk_actor_lists.py` | `.sstate` for a world-map session | Walks the seven actor-list heads + dumps per-actor records (used by `resolve_actor_tmds.py`). |
| `resolve_actor_tmds.py` | `.sstate` + the kingdom slot-1 TMD pack | Walks `actor[+0x44]` mesh-head chains, finds the containing TMD via backward magic-word search, maps to a pack slot. Output is `site/world-overview-live.json`. |
| `verify_slot4_in_ram.py` | `autorun_dump_slot4.lua` output | Confirms the live RAM region matches the disc-decoded slot-4 sub-bodies byte-for-byte. |
| `diff_slot4_ram_vs_disc.py` | Live + disc slot-4 bytes | Generates the byte-level diff visualisation. |
| `match_prim_groups_to_disc.py` | Live prim-pool dump + disc TMD pack | Matches POLY_FT4 prim groups back to their source TMD bodies. |
| [`diff_field_pack_projection.py`](../../scripts/diff_field_pack_projection.py) (repo root) | `.post.NN.bin` + `.meta` from the field-pack projection probe; on-disc LZS-decoded PROT entry | Walks the canonical 97-slot field-pack schema; for each slot, compares runtime RAM bytes against on-disc bytes and prints a per-slot diff sorted by changed-byte count, plus a hex preview of the first divergence per slot. |

### One-shot wrappers

- [`run_world_map_probe.sh`](../../scripts/pcsx-redux/run_world_map_probe.sh)
  &mdash; the canonical shell harness; works as `LEGAIA_LUA=&lt;path&gt; \
  run_world_map_probe.sh` for any autorun.
- [`run_dump_slot4.sh`](../../scripts/pcsx-redux/run_dump_slot4.sh)
  &mdash; thin wrapper that calls the harness with the slot-4 dump
  Lua + a sensible default output path.

## Authoring a new probe

The fastest path to a new probe:

1. Start from
   [`scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua)
   &mdash; the canonical thin probe (~145 lines, kingdom-agnostic) that
   uses the shared library for everything except the per-probe
   breakpoint body. (`autorun_slot4_readers.lua` is the Drake-tuned
   precursor &mdash; kept as a historical reference but not the recommended
   starting point.)
2. Edit the `PROBE_OFFSETS` (or your own probe-address list), the CSV
   header, and the per-hit row written from inside the breakpoint
   callback. The boot-delay / capture-vsync / disarm state machine
   comes from `probe.run({...})` &mdash; don't reimplement it.
3. Run with the harness:
   ```bash
   LEGAIA_LUA=scripts/pcsx-redux/autorun_your_thing.lua \
   LEGAIA_OUT=/tmp/your_probe.csv \
       bash scripts/pcsx-redux/run_world_map_probe.sh
   ```
4. Iterate on the live CSV. The harness re-launches the emulator
   per run; the CSV is overwritten each time. While the probe is
   running, the snapshot file (`<probe>.hits.txt` next to the CSV)
   is rewritten every 60 vsyncs &mdash; tail it from another shell to
   watch hit counts climb live.

When the probe surfaces a useful signal, commit the Lua file under
`scripts/pcsx-redux/` and update the catalogue table above. The CSV
output itself is gitignored &mdash; it's a per-run artifact, not a
project state.
