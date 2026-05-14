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
~/Tools/pcsx-redux/pcsx-redux            # locally-built binary
~/Tools/pcsx-redux/SCUS94254.sstate1     # quicksave slot 1 (F1)
~/Tools/pcsx-redux/SCUS94254.sstate2     # quicksave slot 2 (F2)
...
~/.mednafen/firmware/SCPH1001.BIN        # PSX BIOS, reused from mednafen
~/Downloads/Legend of Legaia (USA)/      # disc image
```

Override any of these via env vars (`PCSX_REDUX`, `LEGAIA_BIOS`,
`LEGAIA_SSTATE`, `LEGAIA_ISO`). The repo doesn't ship the binary or
BIOS or disc; those stay local.

## The harness

[`scripts/pcsx-redux/run_world_map_probe.sh`](../../scripts/pcsx-redux/run_world_map_probe.sh)
is the canonical wrapper. Despite the name, every other Lua autorun
re-uses it via the `LEGAIA_LUA` override:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate2 \
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

This pattern factors into common helpers reused across scripts:

```lua
local function read_u32(mf, addr)
    if not in_ram(addr, 4) then return nil end
    local ok, v = pcall(function() return mf:readU32At(ram_offset(addr)) end)
    return ok and tonumber(v) or nil
end

local function arm_probe(addr, width, label, cb)
    return PCSX.addBreakpoint(addr, "Read", width, "probe:" .. label, cb)
end
```

`ram_offset(addr)` is just `bit.band(addr, 0x1FFFFFFF)` &mdash; strips
the KSEG segment selector so KSEG0 (`0x80xxxxxx`) and KSEG1
(`0xA0xxxxxx`) map to the same physical byte. Always work in
absolute PSX virtual addresses on input; convert at the boundary.

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
| `autorun_slot4_readers.lua` | Reads at the live slot-4 RAM region (`0x8011A624+`) | Found the slot-4 container is byte-identical to disc but with zero readers from the dev-menu top-view; the consumer runs at scene-load only. |
| `autorun_dump_slot4.lua` | Dumps the slot-4 RAM region directly | Sister to the readers probe; produces the ground-truth byte buffer for `verify_slot4_in_ram.py`. |
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

### One-shot wrappers

- [`run_world_map_probe.sh`](../../scripts/pcsx-redux/run_world_map_probe.sh)
  &mdash; the canonical shell harness; works as `LEGAIA_LUA=&lt;path&gt; \
  run_world_map_probe.sh` for any autorun.
- [`run_dump_slot4.sh`](../../scripts/pcsx-redux/run_dump_slot4.sh)
  &mdash; thin wrapper that calls the harness with the slot-4 dump
  Lua + a sensible default output path.

## Authoring a new probe

The fastest path to a new probe:

1. Copy `autorun_world_map_probe.lua` to
   `autorun_<your_thing>.lua`.
2. Replace the `PROBE_ADDRS` / `CSV_HEADER` / breakpoint-arm block
   with your fields. Keep the boot-delay + capture-vsync state
   machine intact.
3. Run with the harness:
   ```bash
   LEGAIA_LUA=scripts/pcsx-redux/autorun_your_thing.lua \
   LEGAIA_OUT=/tmp/your_probe.csv \
       bash scripts/pcsx-redux/run_world_map_probe.sh
   ```
4. Iterate on the live CSV. The harness re-launches the emulator
   per run; the CSV is overwritten each time.

When the probe surfaces a useful signal, commit the Lua file under
`scripts/pcsx-redux/` and update the catalogue table above. The CSV
output itself is gitignored &mdash; it's a per-run artifact, not a
project state.
