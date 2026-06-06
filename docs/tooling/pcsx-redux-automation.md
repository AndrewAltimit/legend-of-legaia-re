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

## Contents

- [Why PCSX-Redux](#why-pcsx-redux)
- [Setup](#setup)
- [The harness](#the-harness)
- [The probe pattern](#the-probe-pattern)
- [Catalogue](#catalogue)
  - [Runtime probes (Lua autorun)](#runtime-probes-lua-autorun)
  - [Save-state to Python (offline analysis)](#save-state-to-python-offline-analysis)
  - [One-shot wrappers](#one-shot-wrappers)
  - [GDB-stub bridge (`gdb_probe.py`)](#gdb-stub-bridge-gdb_probepy)
  - [Analysing probe outputs (`probe.py`)](#analysing-probe-outputs-probepy)
- [Authoring a new probe](#authoring-a-new-probe)
- [See also](#see-also)

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

### Save-state library (immutable backups)

PCSX-Redux quicksave slots (`<TITLE_ID>.sstate<N>`) and mednafen `mc{N}`
cards are **ephemeral** — the next time you save in that slot, the bytes
are gone, and a save you reverse-engineered against has to be recaptured
from scratch. To stop that, back interesting states up into a
fingerprint-named library:

```
scripts/manage-states.py backup pcsx-redux ~/Tools/pcsx-redux/SCUS94254.sstate6 \
    --label field_walled_collision_pin
scripts/manage-states.py library          # list what's backed up + catalogue status
```

`backup` copies the file to `saves/library/<emulator>/<sha256>.<ext>`
(immutable; the sha256 is the filename, so it never collides or gets
overwritten) and records the fingerprint on the named `scripts/scenarios.toml`
scenario as `backup_fingerprint`. The library directory is **gitignored**
(it holds Sony game RAM); the committed pointer is the manifest's
`backup_fingerprint` field. When a scenario has one, both
`scripts/manage-states.py` and `run_probe.sh --scenario` resolve the
**library copy in preference** to the live slot — so probes keep working
after you've saved over the original slot. See the field schema +
workflow at the top of [`scripts/scenarios.toml`](../../scripts/scenarios.toml).

## The harness

[`scripts/pcsx-redux/run_probe.sh`](../../scripts/pcsx-redux/run_probe.sh)
is the canonical wrapper. Despite the name, every other Lua autorun
re-uses it via the `LEGAIA_LUA` override:

```bash
LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/<your-saved-state>.sstate \
LEGAIA_LUA=scripts/pcsx-redux/autorun_world_map_fog_probe.lua \
LEGAIA_OUT=/tmp/fog_probe.csv \
LEGAIA_FRAMES=600 \
    bash scripts/pcsx-redux/run_probe.sh
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
[`scripts/pcsx-redux/lib/probe.lua`](../../scripts/pcsx-redux/lib/probe.lua),
which is an umbrella that re-exports the per-concern submodules under
[`scripts/pcsx-redux/lib/probe/`](../../scripts/pcsx-redux/lib/probe/) -
`env`, `mem`, `sstate`, `pad`, `bp`, `csv`, `snapshot`, `sm`, `watch`, `step`,
and `symbols`. A new probe doesn't reimplement the state machine, the
memory readers, the save-state loader, the pad-override helpers, the
CSV writer, or the live-snapshot writer - it imports them:

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

### Write-watchpoint logging (`probe.watch`)

The recurring "*what writes this address?*" probe arms a Write breakpoint
and, in the callback, logs `(elapsed, label, addr, pc, ra, new_value)` to a
CSV plus a first-N call-context dump. `probe.watch` factors that closure out
(it composes `bp` + `mem` + `snapshot`, adding no new emulator interaction):

```lua
local w = probe.watch.new{
    csv         = probe.csv_open(probe.out_path("hits.csv"),
                                 "tick,label,addr,pc,ra,value"),
    detail_path = probe.out_path("hits.detail.txt"),  -- optional
    elapsed     = function() return g_elapsed end,
}
w:arm(player_ptr + 0x14, 2, "playerX")  -- width 1/2/4; kind defaults "Write"
-- ... at end: print("total writes:", w:total())
```

### Instruction tracing + write attribution (`probe.step`)

PCSX-Redux's Lua FFI exposes **no native single-step** (only `pauseEmulator` /
`resumeEmulator` and non-pausing breakpoints — the internal `m_debug->stepIn()`
is not bound). `probe.step` reconstructs the two things single-stepping is used
for, on top of breakpoints, so fine-grained RE stays scriptable instead of
needing the GUI debugger:

```lua
-- Observational single-step over a code region: an Exec BP on every 4-byte
-- instruction in [lo, hi); each fires in execution order with LIVE
-- pre-execution registers. opts.gate() restricts recording to a window.
local tr = probe.step.trace(0x801de840, 0x801df000, { gate = on_door_frame })

-- Width-correct range write-finder: arms width-`unit` (default 2) Write BPs
-- across [addr, addr+len) so a store of unknown width/alignment to a struct
-- is caught with the correct faulting PC + live registers + post-store bytes.
local fw = probe.step.find_writer(player + 0x10, 0x10, { on_write = log })
-- ... fw:count() / fw:records() / fw:dump(path)
```

Two gotchas these encode:

- **Watch *width* matters as much as address.** A `Write` BP only matches
  accesses overlapping `[addr, addr+width)`, and PCSX supports width 1/2/4 only
  — a width-2 watch at exactly `+0x14` **misses** a wider/offset store into the
  same struct. `find_writer` covers a range by arming a unit BP per slot. (This
  is what hid the Mei's-house door reposition behind a 2-byte no-op re-store;
  the range watch found the real writer — a field-VM `0x23 MOVE_TO` — in one run.)
- **A Write BP fires *at* the store with live registers** (not after the
  function returns); read `getRegisters()` directly. The earlier "stale
  registers" symptom was a misread — the store instruction simply didn't use the
  register in question.

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
re-imports that shift function entry points. The symbol resolver
accepts Ghidra-canonical names from two sources:

* **Function entry points** (`FUN_801DA51C`, slot-4 `k10_shared` labels,
  named overlays). Source: per-function dump headers under
  `ghidra/scripts/funcs/*.txt`.
* **Global data labels** (`DAT_8007078C` / `_DAT_8007BCD0`, both case
  forms accepted). Source: the same dump-header walk, plus a regex
  harvest of `DAT_xxxxxxxx` references from the decomp body content
  (so DAT names show up even before `dump_globals.py` has been run
  for a given program), plus a dedicated `dump_globals.py` Jython
  script for authoritative names + lengths.

Three ways to use it:

```lua
-- Bespoke autorun:
local symbols = require("probe.symbols").load()
probe.arm_breakpoint(symbols.FUN_801DA51C, "Exec", 4, "world_map_sm", cb)
```

```toml
# .probe.toml: addr/base accept either an int or a symbol-name string.
[[breakpoint]]
addr = "FUN_801DD35C"     # resolves at spec-load time
kind = "Exec"
[[breakpoint]]
addr  = "_DAT_801EF16C"
kind  = "Read"
width = 4
```

```bash
# Regenerate after adding new dumps (covers funcs/* dumps and globals_*).
python3 scripts/pcsx-redux/build-symbols.py
```

```bash
# Authoritative globals (one-time per program; optional but lossless):
docker compose exec ghidra /ghidra/support/analyzeHeadless /projects legaia \
    -process SCUS_942.54 -noanalysis -postScript /scripts/dump_globals.py
# ... or pass `-process overlay_<name>.bin` for per-overlay globals.
python3 scripts/pcsx-redux/build-symbols.py
```

The resolver fails loudly on a typo'd symbol name &mdash; arming a
breakpoint at `nil` otherwise silently captures zero hits and the probe
runs to completion with no diagnostic. The hex portion of the name is
case-insensitive: docs use `FUN_801DD35C`, Ghidra emits
`FUN_801dd35c`, both resolve identically.

`scripts/pcsx-redux/probes/_check_specs.py` cross-validates every
`.probe.toml` spec's symbol references against `symbols.json` so a
typo'd symbol fails CI rather than the probe run.

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
- **`GPU::Vsync` events fire on game-driven `VSync(0)` calls, not 60 Hz hardware.** PCSX-Redux delivers `GPU::Vsync` when the game calls libcd's `VSync(0)` syscall, which is sparse during boot init / CD-DMA phases. A probe waiting on `vsync_count >= 600` to fire during boot can sit for minutes of wall time even when emulator-time has advanced past the target. For boot-phase timing use a memory watchpoint at a known transition register (e.g. `_DAT_801EF16C` title countdown) instead of a vsync-count target &mdash; the watchpoint fires precisely when the game writes the state transition.
- **Don't `readAt(2 MiB, 0)` inside a vsync callback.** A single 2 MiB `PCSX.getMemoryAsFile():readAt(...)` call permanently degrades subsequent `GPU::Vsync` event delivery in the same emulator launch &mdash; subsequent callbacks fire rarely or not at all (probably a heavyweight Lua GC pass disrupts the event loop). One-shot full-RAM dumps work because the script transitions to a quit-soon state after the single read; multi-snapshot probes break. Workarounds: keep individual reads small (64 KiB at a time is safe), or take one dump per emulator launch (chained single-shots).
- **PCSX.quit(0) doesn't always exit the process.** Wrap every probe invocation with `timeout --kill-after=10s <budget>` so a hung emulator gets reliably killed. The captured data is already on disk by the time PCSX.quit fires &mdash; the timeout-kill is purely cleanup.

## Catalogue

The committed scripts live in
[`scripts/pcsx-redux/`](../../scripts/pcsx-redux/). Each Lua file
documents its purpose in a header comment block; the catalogue here
is the high-level index.

### Runtime probes (Lua autorun)

The table below is an index: each script's one-line purpose plus a link to
its detail subsection. The shorter probes carry their full description inline;
the longer ones (`Probes` + `What it answered`) are written out as
[per-probe detail](#runtime-probe-details) below the table.

| Script | What it answered |
|---|---|
| `autorun_world_map_probe.lua` | Pins the world-map POLY_FT4 emitter's one-shot gate flag + the three-param block driving it. Reads at `_DAT_8007BCD0..D8` (gate-arm params), gate flag `_DAT_801F351C` writes, and four `FUN_801D7EA0` entries. |
| `autorun_world_map_fog_probe.lua` | Captures the per-Z fog-tint LUT the overlay leaves at `0x801F7644..0x801F8690` consult on every vertex. Reads at five fog fields (GP-relative `-0x2E0 / -0x2DC / -0x2D1 / -0x2BC / +0x90`) + 1 KiB LUT dump. |
| `autorun_prim_pool_writers.lua` | Confirms the eight overlay-resident high-mode renderers are the ones writing the pool (matches `FUN_80043390`'s dispatch table). Writes across the 341 KB GPU prim pool at `0x800AD400+`. |
| `autorun_lzs_and_bundle_probe.lua` | Pins which PROT entries get LZS-decoded for the world-map bundle. LZS decode entries + bundle dispatcher (`FUN_8001F05C`) during world-map load. |
| `autorun_slot4_consumer_pcs.lua` | Kingdom-agnostic slot-4 consumer PCs. → [detail](#autorun_slot4_consumer_pcslua) |
| `autorun_slot4_dispatcher_args.lua` | Captures the original cluster-A dispatcher call args before the kind handlers clobber them. → [detail](#autorun_slot4_dispatcher_argslua) |
| `autorun_dump_slot4.lua` | Dumps the slot-4 RAM region directly. Produces the ground-truth byte buffer for `verify_slot4_in_ram.py`. |
| `autorun_xp_table_reader.lua` | Tiled read-bp scan over `0x8007123C..0x80071300`; **superseded** by the `DAT_80076AF4` XP curve. → [detail](#autorun_xp_table_readerlua) |
| `autorun_field_pack_projection.lua` | Captures the scene-asset loader's on-disc → RAM projection a single save state can't observe. → [detail](#autorun_field_pack_projectionlua) |
| `autorun_dump_full_ram.lua` | Dumps the full 2 MiB main RAM. One-shot snapshot for downstream analysis. **One dump per launch only** — see the `readAt(2 MiB)` caveat above. |
| `autorun_boot_walk_snapshots.lua` | Multi-snapshot RAM-and-register walk across `LEGAIA_TARGETS` vsyncs. → [detail](#autorun_boot_walk_snapshotslua) |
| `autorun_countdown_trigger.lua` | Watchpoint-driven RAM + screenshot snapshot; pinned `FUN_801DD35C` as the title-overlay tick. → [detail](#autorun_countdown_triggerlua) |
| `autorun_player_pos_watch.lua` | Pinned the town/field free-movement integrator (`FUN_801d01b0`). → [detail](#autorun_player_pos_watchlua) |
| `autorun_house_door_writer.lua` | Cracked the intra-town (house/interior) door mechanism (field-VM `0x23 MOVE_TO`). → [detail](#autorun_house_door_writerlua) |
| `autorun_man_source.lua` | Pinned a field scene's runtime MAN source (`_DAT_8007b898`). → [detail](#autorun_man_sourcelua) |
| `autorun_title_overlay_writer_hunt.lua` | Pins the SCUS-side title-overlay loader. → [detail](#autorun_title_overlay_writer_huntlua) |
| `autorun_monster_record_source.lua` | Pinned the monster stat archive to PROT entry `0867_battle_data`. → [detail](#autorun_monster_record_sourcelua) |
| `autorun_battle_reward_source.lua` | Confirmed the victory reward path. → [detail](#autorun_battle_reward_sourcelua) |
| `autorun_title_staging_capture.lua` | Pins the PROT source of the title overlay. → [detail](#autorun_title_staging_capturelua) |
| `autorun_battle_palette_source.lua` | Confirms the scene bundle is LZS-decompressed into the work arena at load; does NOT pin the party palette. → [detail](#autorun_battle_palette_sourcelua) |
| `autorun_load_screen_dump.lua` | Ground-truth capture for the load-screen panel border + slot-pill source sprites. → [detail](#autorun_load_screen_dumplua) |
| `autorun_town01_script_flow.lua` | Pins a field scene's script execution model. → [detail](#autorun_town01_script_flowlua) |
| [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) | Pins the disc source of the battle-form party CLUT band (VRAM rows 490..497). → [detail](#autorun_battle_char_clut_sourcelua) |
| [`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua) | Pins the battle-form party-mesh install callsite. → [detail](#autorun_battle_party_mesh_installlua) |
| [`autorun_battle_render_capture.lua`](../../scripts/pcsx-redux/autorun_battle_render_capture.lua) | Live-confirms the exact battle camera byte-exact. → [detail](#autorun_battle_render_capturelua) |
| [`autorun_audio_trace.lua`](../../scripts/pcsx-redux/autorun_audio_trace.lua) | Multi-frame retail-trace input for the audio-trace parity oracle. → [detail](#autorun_audio_tracelua) |
| [`autorun_summon_model_base.lua`](../../scripts/pcsx-redux/autorun_summon_model_base.lua) | Targets `gp[0x754]`, the `model_sel` additive base read in the shared spawn stager `FUN_80021B04`. Exec-bp the stager during a summon (default `gimard_summon_start`) or an enemy special-attack frame; each hit logs `$gp`, the absolute `gp+0x754` global, the base value, and the part record's `model_sel`/`flags`. The one residual unblocking both summon and move-power effect-FX render (the records share this stager). |

#### Runtime probe details

##### `autorun_slot4_consumer_pcs.lua`

- **Probes:** Exec bps at the cluster-A + cluster-B LW PCs identified during the slot-4 RE.
- **What it answered: Kingdom-agnostic** — hits the same SCUS function PCs regardless of where slot 4 lives in RAM for the destination kingdom. Confirmed cross-kingdom: cluster A and B fire on Drake, Sebucus (town → map02) and Karisto (town → map03) with the same caller RAs (cluster B's RA `0x80059C00` is byte-identical across all three; cluster A's RAs `0x8001B47C` inside `FUN_8001ada4` + `0x801F78D4` world-map overlay are present in every kingdom). Hit-count scales with per-kingdom record count. Output CSV is `probe_idx, cluster, pc, name, ra, a0..a3, s8`; `.detail.txt` sidecar captures first-hit call-context per PC. `LEGAIA_PC_CAP=N` raises the default 200-hit-per-PC cap for uncapped totals.

##### `autorun_slot4_dispatcher_args.lua`

- **Probes:** Exec bp at `0x80043390` (cluster A dispatcher entry).
- **What it answered:** Captures the *original* call args before the kind handlers clobber `a1` / `a2`: caller RA, descriptor pointer `a0`, packed `cmd_flags` (`a1`), `fade_flags` (`a2`), and the first command word's `kind` / `count`. Use this to classify which of the four dispatcher banks (`0x00` / `0x50` / `0xA0` / `0xF0`) each call lands in. `LEGAIA_DISP_CAP=N` raises the default 200000-hit cap.

##### `autorun_xp_table_reader.lua`

- **Probes:** Read bps tiled across `0x8007123C..0x80071300`.
- **What it answered:** Originally written to pin the runtime XP-table reader. **Superseded** — the real XP curve is `DAT_80076AF4`, read by the overlay applier `FUN_801E9504`; the old `0x8007123C` target is an off-by-`0x800` artefact over a sin-LUT slice (see [`subsystems/level-up.md`](../subsystems/level-up.md#xp-table)). Re-target the bps to `0x80076AF4` before re-running. The CSV / detail-sidecar shape of the probe is generic and reusable for any tiled-read-bp scan.

##### `autorun_field_pack_projection.lua`

- **Probes:** Exec bp at `FUN_8001F7C0` (scene asset loader) entry; one-shot Exec bp at the loader's return address; dumps post-load RAM window.
- **What it answered:** Captures the loader's on-disc &rarr; RAM projection that a single save state can't observe. `LEGAIA_HOLD_BUTTON` / `LEGAIA_HOLD` drive the warp-tile input from inside the probe; the run quits ~30 vsyncs after the first post-load dump. Diff via [`scripts/pcsx-redux/diff_field_pack_projection.py`](../../scripts/pcsx-redux/diff_field_pack_projection.py) against the on-disc PROT bytes. World-map scenes (`map01` / `map02` / `map03`) are not field-pack-formatted — running against them produces a 75 KB GP0-primitive pool projection at `_DAT_8007B8D0 - 0x12800` instead.

##### `autorun_boot_walk_snapshots.lua`

- **Probes:** Multi-snapshot RAM-and-register probe; dumps at each emulator vsync in `LEGAIA_TARGETS` (comma-separated) with chunked reads spread across vsync callbacks.
- **What it answered:** Walks a save state through several timeline points in one emulator launch. **Known limitation**: the chunked-read workaround works for ~2-4 close-together snapshots but degrades past ~10 chunks; for high-vsync targets prefer chained single-shots of `autorun_dump_full_ram.lua`.

##### `autorun_countdown_trigger.lua`

- **Probes:** Memory write-watchpoint at `LEGAIA_WATCH_ADDR` (default `0x801EF16C`, the title-attract countdown); width-2 `Write` BP. Optional screenshot via `PCSX.GPU.takeScreenShot()` taken inside the BP callback before the deferred RAM dump.
- **What it answered: Watchpoint-driven RAM + screenshot snapshot** — fires the dump at the exact moment the game writes the watched register. `LEGAIA_HIT_SKIP` ignores the first N hits before snapshotting (default `1` to skip the boot-time DMA write). `LEGAIA_DUMP_BASE` / `LEGAIA_DUMP_LEN` restrict the dump window (default `0x801C0000` / `0x40000` = overlay window). Decode the screen to PNG via [`scripts/pcsx-redux/decode_pcsx_screen.py`](../../scripts/pcsx-redux/decode_pcsx_screen.py). Pinned `FUN_801DD35C` as the title-overlay tick — see [`subsystems/boot.md` § Tick function](../subsystems/boot.md#tick-function).

##### `autorun_player_pos_watch.lua`

- **Probes:** Write-watchpoint on the player actor world-position fields (`*(0x8007C364) + 0x14` X / `+0x18` Z), armed lazily in `on_capture` after the save loads (the target is a runtime pointer deref). Cycles the four d-pad directions (camera facing unknown) so at least one produces a position write.
- **What it answered: Pinned the town/field free-movement integrator** — hits land in `FUN_801d01b0` (overlay 0897) at the four `sh player[+0x14/0x18]` stores `0x801D0684/06E4/0744/07B4`, with collision via `FUN_801cfe4c`. CSV columns `tick, axis, write_addr, pc, ra, new_val` + a `.detail.txt` call-context sidecar. Run against a save parked in a walkable field/town. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md).

##### `autorun_house_door_writer.lua`

- **Probes:** `probe.step.find_writer` over the player position block `*(0x8007C364)+0x10..+0x20` (range write-watch, robust to store width/alignment), holding Up to enter a Rim Elm house. Writes each store **incrementally** (so a manually-closed window keeps the data).
- **What it answered: Cracked the intra-town (house/interior) door mechanism** — entering a house is a field-VM `0x23 MOVE_TO` to the interior tile (the writer lands in `FUN_801de840` `case 0x23` at `0x801debc4`), not a scene change — same op class as the `0x3F` scene-change the door randomizer handles. Earlier write-watchpoints missed it (width-2 watch caught only a 2-byte no-op re-store); the range watch found the real writer in one run. See [`autorun_house_door_trace.lua`](../../scripts/pcsx-redux/autorun_house_door_trace.lua) (companion).

##### `autorun_man_source.lua`

- **Probes:** Exec breakpoint at the asset-type dispatcher `FUN_8001F05C`, filtered to the MAN dispatch (`a1 >> 24 == 3`). On hit logs `a0` (source pointer), size, `a2`/`a3` flags, caller RA, and the resulting `_DAT_8007b898` buffer, captures call context, and dumps the source bytes; also dumps the resident MAN at capture start. Drive a transition with `LEGAIA_HOLD_BUTTON` / `LEGAIA_HOLD`.
- **What it answered: Pinned a field scene's runtime MAN source** (`_DAT_8007b898`). Caller is `FUN_80020224`, the `scene_asset_table` walker that reads the table base from `_DAT_8007b85c` and feeds the dispatcher `source = table_base + descriptor.data_offset`. Captured a standalone-town load: the MAN's LZS stream byte-matches a [`count=6 scene_asset_table`](../formats/scene-bundles.md) descriptor in the town's own PROT block - the variant a strict count-7 detector skipped. Run against the `overworld_into_town_man_load` scenario (Down ~0.75s into a town entrance).

##### `autorun_title_overlay_writer_hunt.lua`

- **Probes:** Write bps at 8 anchor addresses across the title-overlay code region (`0x801CC000..0x801EF018`).
- **What it answered:** Pins the SCUS-side title-overlay loader: any write into the overlay window fires a BP whose `pc` + `ra` + call-context dump identify the writer function. Run cold-boot (`LEGAIA_NO_SSTATE=1`) since in-game saves are past the load point.

##### `autorun_monster_record_source.lua`

- **Probes:** Exec bps at the monster init `FUN_80054CB0` (logs the live record: name / HP / MP / stats), the battle archive loader `FUN_800542C8`, the relative disc-seek `FUN_8003E964` (`a0 = (id-1)*40` sectors → monster id), the generic disc read `FUN_8003E800` (logs the CdlLOC → disc LBA → PROT.DAT offset for 40-sector reads), and the retail host-trap open `FUN_800608F0`.
- **What it answered: Pinned the monster stat archive** to PROT entry `0867_battle_data` (extended footprint): per-id `0x14000` LZS slot at `(id-1)*0x14000`. Run against a battle save (Rim Elm scripted fights). Three decoded records match the live actor stats byte-for-byte. The `monster_data` label (PROT 869) is a stub. See [`subsystems/battle.md` § Monster archive](../subsystems/battle.md#monster-archive-prot-entry-867).

##### `autorun_battle_reward_source.lua`

- **Probes:** Write breakpoints on the XP accumulator `0x80084440`, party gold `0x8008459C`, party XP bank `0x800845A4`, and a candidate gold accumulator; each hit logs the writing PC + all GPRs + the new value, and the staged totals are snapshotted each second. Exec bps at the commit `FUN_80026018` and monster-init `FUN_80054CB0`.
- **What it answered: Confirmed the victory reward path.** Run against the `rim_elm_gimard_victory` scenario (a lone-enemy fight captured mid-combo so it resolves without input). Gimard's gold went `500 → 515` (+15) via a write at `FUN_8004E568`, matching the record's base gold (`+0x44`=60) through the lone-enemy `floor((gold>>1)/2)` formula. Pinned the reward fields to record `+0x44..+0x49` (gold / EXP / drop id / drop %). See [`subsystems/battle-formulas.md` § Victory spoils](../subsystems/battle-formulas.md#victory-spoils-rewards).

##### `autorun_title_staging_capture.lua`

- **Probes:** Exec bp at `FUN_8001A55C` (LZS decoder); per-decode src buffer dump.
- **What it answered:** Pins the PROT source of the title overlay. Each fired decode dumps the compressed source bytes to `<OUT_DIR>/decode_NNN_*.bin`; an offline script byte-matches against PROT entries. Run cold-boot.

##### `autorun_battle_palette_source.lua`

- **Probes:** Write breakpoints on the party-palette blocks `0x800EBEE8` / `0x800EC0C8` / `0x800EC2A8` (Vahn / Noa / Gala); each hit logs the writing PC + all GPRs and flags any register whose 32 bytes match the block (the source). On the first LZS-range write it dumps the loaded source buffer `0x80180000..0x80186000`.
- **What it answered:** Run against `rim_elm_queen_bee_battle` (auto-starts, no input). **Caveat:** in that capture the writes to `0x800EBEE8` come from `FUN_8001A55C` reading the loaded `town0c` scene bundle (PROT 0022 from `0x23430`), but the resulting value (`0x7965481F`) is **scene data, not the party palette** — `0x800EBEE8` is a *shared* work-arena address. So this probe confirms the scene bundle is LZS-decompressed into the arena at load, but does **not** pin the party palette (which is character-intrinsic, `0x409d…`, and is *not* a stored disc blob — see [`formats/character-mesh.md`](../formats/character-mesh.md), proven via the `lzs-decode find` brute). To pin the palette, write-watchpoint the *final* party-palette write in a **clean Tetsu/Drake fight**, not the queen_bee context. The companion `autorun_battle_palette_lzs_src.lua` (Exec bp at the LZS entry) crashes under the battle's heavy decode load.

##### `autorun_load_screen_dump.lua`

- **Probes:** Loads sstate9 (parked on the Continue → Load screen), settles `LEGAIA_FRAMES` vsyncs, then dumps the rendered framebuffer via `PCSX.GPU.takeScreenShot()` + full 2 MiB main RAM.
- **What it answered:** Ground-truth capture for pinning the load-screen panel border + slot-pill source sprites. Output `load_screen_fb.raw` + `.meta` decode to PNG via [`scripts/pcsx-redux/decode_load_screen.py`](../../scripts/pcsx-redux/decode_load_screen.py). The framebuffer pixels match PSX 320×240 coords 1:1, so sprite-rect dst positions can be measured directly. For full ground-truth VRAM (not just the rendered framebuffer), pair with `extract_vram_from_sstate.py` + `decode_vram.py` on the same save state — that pipeline pinned the load-screen panel CLUT to row 2 of the system-UI TIM at `PROT.DAT[0x018E0]`. The probe arms no breakpoints, so it runs with `--fast` for ~30s end-to-end. See [`subsystems/save-screen.md` § Sprite asset sources](../subsystems/save-screen.md#sprite-asset-sources-continue--load-screen).

##### `autorun_town01_script_flow.lua`

- **Probes:** Exec bps at the scene-load init `FUN_8003aeb0`, the system-script prologue runner `FUN_8003ab2c`, the per-frame VM step `FUN_801de840` (deduped into a per-context table keyed by `a2` = ctx ptr: script_id `ctx+0x50`, bytecode `ctx+0x90`, pc range, hits), and the three nibble-7 collision-grid write sites `0x801e1d00 / 0x801e1d74 / 0x801e1e84`. Dumps the live collision grid (`*_DAT_1f8003ec + 0x4000`, scratchpad-resolved) at first + last frame with a wall-tile count + ASCII map.
- **What it answered:** Pins a field scene's **script execution model** — which contexts run, their scripts, and whether walls are painted per-frame or only at load. On the `field_walled_collision_pin` scenario it showed: 7455 painted wall tiles, a single steady-state context (script_id `0xFB`, bytecode `0x8010F092`, looping pc `0x102..0x297` — matching the clean-room engine's static trace), and **zero** nibble-7 paints while standing still (walls are load-time only). To capture the load-time paint flow, replay a pre-transition save / drive a step into a scene exit so `FUN_8003aeb0` + the nibble-7 BPs fire. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md).

##### `autorun_battle_char_clut_source.lua`

- **Probes:** [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) holds a walk direction to trigger a random encounter, then exec-bps the disc seek/read primitives (`FUN_8003E8A8` / `FUN_8003E964` / `FUN_8003E800`) to log every CdlLOC → absolute LBA → `PROT.DAT` offset over the battle-init window.
- **What it answered: Pins the disc source of the battle-form party CLUT band** (VRAM rows 490..497, x=0..255). Save-state analysis proved these palettes are battle-context-loaded, persist in VRAM, and are NOT in main RAM in any captured save (transient decompress→DMA→free), nor verbatim on disc except Vahn's row 490 (map01/map02 sec0). **Run against a field sstate where the band is NOT yet resident** (the band is absent right after boot / before the first battle) so battle-init forces a fresh disc load. Map the logged LBAs to PROT entries with [`map_clut_disc_reads.py`](../../scripts/pcsx-redux/map_clut_disc_reads.py) (`--vram <mc_vram.bin>` confirms which entry's decompressed section holds the row-492 palette). See [`reference/open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md) "Battle character image + CLUT source".

##### `autorun_battle_party_mesh_install.lua`

- **Probes:** [`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua) write-watchpoints the three party TMD-pointer slots `DAT_8007C018[0..2]` plus an exec-bp on `tmd_register` (`FUN_80026B4C`) filtered to party indices (`DAT_8007B774 ∈ {0,1,2}`) and one on the battle loader `FUN_800520F0`. Loads a field save that auto-starts a battle (`--scenario rim_elm_queen_bee_battle`) so the field→battle transition is captured live; logs the installed pointer (`a0`), the **real caller** `ra` (from `tmd_register` entry, before its prologue saves `ra`), and a call-context snapshot.
- **What it answered: Pins the battle-form party-mesh install callsite** — long mis-assumed to live in an uncaptured overlay. The party meshes are registered through the generic `tmd_register` from two **static SCUS** state-handlers: `FUN_800513F0` (lead/active actors, `tmd_register(*(actor+0x50)+0x18)` in a `while<3` loop, alongside the `FUN_80052FA0` palette decode; caller `ra=0x8005148C`) and `FUN_800542C8` (additional members, per-member loop `tmd_register(*(*rec+4))`; caller `ra=0x80054804`). Both are dispatched indirectly, so a static `0x8007C018` xref finds no writer. Installed pointers byte-match the battle form (Vahn → `0x80165F48`, the value a battle save holds). **Caveat:** the write-watchpoint's `value` column shows the *pre-write* (old field) pointer because PCSX-Redux fires Write BPs pre-commit; the `tmd_register`-entry `a0` is the authoritative new value. See [`formats/character-mesh.md` § Battle form](../formats/character-mesh.md#assembly--object-local-pieces-posed-by-the-battle-anm).

##### `autorun_battle_render_capture.lua`

- **Probes:** [`autorun_battle_render_capture.lua`](../../scripts/pcsx-redux/autorun_battle_render_capture.lua) reads the battle-render state from **inside the `func_0x801d02c0` grid-render breakpoint** (at frame 0 the camera globals + battle ctx hold stale field state and `_DAT_8007b83c` reads `0x00`): the camera globals (pitch `0x8007b790` / yaw `0x8007b792` / roll `0x8007b794` / TR `0x800840b8..c0` / H `0x8007b6f4`), the `func_0x801d02c0` grid dims + tile-constant scratchpad (`probe.read_scratch_u32`, NOT `read_u32` — the `0x1f80xxxx` scratchpad needs `PCSX.getScratchPtr`), and the battle actor structs (scanning the ctx for pointers whose `+0x72` scale is `~0x1000`).
- **What it answered: Live-confirms the exact battle camera byte-exact.** Run on a real `map01` overworld battle save: `mode=0x15 pitch=32 roll=0 TR=(0,1280,7680) H=256`; grid `28×28` cells (`0x200` pitch); actors at scale `+0x72=0x1000` (1.0, not scaled — large on-screen size comes from the meshes); dome at `DAT_8007C018[2]`. Validates the RE'd camera in [`subsystems/battle.md` § Battle camera](../subsystems/battle.md#battle-camera-exact).

##### `autorun_audio_trace.lua`

- **Probes:** [`autorun_audio_trace.lua`](../../scripts/pcsx-redux/autorun_audio_trace.lua) calls `PCSX.createSaveState()` every `LEGAIA_INTERVAL` vsyncs; walks the protobuf in-place via FFI pointer arithmetic; slices out only the SPU sub-message (~600 KiB per capture vs. 20 MiB for the full state); appends to one binary stream prefixed with `LEGSPU01`.
- **What it answered:** Multi-frame retail-trace input for the I1b(b) audio-trace parity oracle. Pair with [`extract_audio_trace_from_sstates.py`](../../scripts/pcsx-redux/extract_audio_trace_from_sstates.py) to decode into the JSONL `AudioTraceFrame` shape that `legaia-engine audio-trace --retail-jsonl` consumes. The probe runs against any save state — best signal comes from one parked mid-BGM. PCSX-Redux's Lua API does not expose the SPU register file directly, so `createSaveState` is the load-bearing primitive; the FFI walk avoids materialising the full 20 MiB state per vsync (which would degrade `GPU::Vsync` delivery via Lua GC pressure, same shape as the `readAt(2 MiB)` caveat above).

### Save-state to Python (offline analysis)

| Script | Input | Output |
|---|---|---|
| `dump_kingdom_ram_layout.py` | `.sstate` files for the three kingdoms | Per-kingdom RAM-layout JSON used by the `world-overview` page. |
| `walk_actor_lists.py` | `.sstate` for a world-map session | Walks the seven actor-list heads + dumps per-actor records (used by `resolve_actor_tmds.py`). |
| `resolve_actor_tmds.py` | `.sstate` + the kingdom slot-1 TMD pack | Walks `actor[+0x44]` mesh-head chains, finds the containing TMD via backward magic-word search, maps to a pack slot. Output is `site/world-overview-live.json`. |
| `verify_slot4_in_ram.py` | `autorun_dump_slot4.lua` output | Confirms the live RAM region matches the disc-decoded slot-4 sub-bodies byte-for-byte. |
| `diff_slot4_ram_vs_disc.py` | Live + disc slot-4 bytes | Generates the byte-level diff visualisation. |
| `match_prim_groups_to_disc.py` | Live prim-pool dump + disc TMD pack | Matches POLY_FT4 prim groups back to their source TMD bodies. |
| [`diff_field_pack_projection.py`](../../scripts/pcsx-redux/diff_field_pack_projection.py) | `.post.NN.bin` + `.meta` from the field-pack projection probe; on-disc LZS-decoded PROT entry | Walks the canonical 97-slot field-pack schema; for each slot, compares runtime RAM bytes against on-disc bytes and prints a per-slot diff sorted by changed-byte count, plus a hex preview of the first divergence per slot. |
| [`decode_pcsx_screen.py`](../../scripts/pcsx-redux/decode_pcsx_screen.py) | `<OUT>.screen` + `.screen.meta` from `autorun_countdown_trigger.lua` (or any probe that calls `PCSX.GPU.takeScreenShot()`) | PNG of the visible framebuffer at the capture moment. Decodes BGR555 (`bpp=16`) or BGR888 (`bpp=24`). Pillow required for PNG output; falls back to raw RGB888 if Pillow is missing. |
| [`decode_load_screen.py`](../../scripts/pcsx-redux/decode_load_screen.py) | `load_screen_fb.raw` + `.meta` from `autorun_load_screen_dump.lua` | PNG of the rendered load-screen framebuffer. Dependency-free (uses stdlib `zlib` + manual PNG chunks); pixel coordinates match PSX 320×240 framebuffer 1:1. Pairs with the panel-source RE in `subsystems/save-screen.md`. |
| [`extract_audio_trace_from_sstates.py`](../../scripts/pcsx-redux/extract_audio_trace_from_sstates.py) | The `LEGSPU01`-magic binary stream from `autorun_audio_trace.lua` | JSONL stream of `AudioTraceFrame` records consumed by `legaia-engine audio-trace --retail-jsonl` and the disc-gated `audio_trace_multi` integration test. Walks PCSX-Redux's SPU protobuf schema: 24 × Channel sub-messages (Chan::Data + ADSRInfo + ADSRInfoEx) plus the 512-byte SPU register file (MainVol_L / MainVol_R at offset 0x180/0x182, Reverb_Mode at 0x1AA). Voice "audible" = `Chan::Data.on || Chan::Data.stop`; `ADSRInfoEx.state` is the configured envelope shape and reads as Sustain for unused voices, so it is not a reliable audibility signal. |
| [`extract_vram_from_sstate.py`](../../scripts/pcsx-redux/extract_vram_from_sstate.py) | A PCSX-Redux `.sstate*` file | 1 MiB raw BGR555 VRAM blob (`vram.bin`). Gunzips the save state and finds the GPU.vram protobuf field (canonical tag `0x1A 0x80 0x80 0x40` = field 3, wire-type 2, length 0x100000). Dependency-free. The PCSX-Redux equivalent of `mednafen-state vram-dump`: ground-truth VRAM at any parked state, useful for back-referencing sprite sources and CLUT rows against the extracted TIM corpus. |
| [`decode_vram.py`](../../scripts/pcsx-redux/decode_vram.py) | `vram.bin` from `extract_vram_from_sstate.py` | 1024×512 PNG of the BGR555 VRAM. Stdlib-only. Pixel coords map 1:1 to PSX VRAM `(fb_x, fb_y)`, so CLUT rows at `fb_y=480+` and texture pages at `fb_x≥640` are visible at a glance. |
| [`scan_panel_prims.py`](../../scripts/pcsx-redux/scan_panel_prims.py) | A 2 MiB main-RAM dump (e.g. `load_screen_ram.bin`) + optional `--rect X0 Y0 X1 Y1` framebuffer rect | Lists every GP0 textured-sprite primitive (cmd byte `0x64..0x67`) whose dst falls in the rect, decoded into `(dst_x, dst_y, u, v, clut_x, clut_y, w, h)`. Groups by CLUT so the unique source tiles each CLUT references stand out. Used to pin the 9-slice tile geometry of the load-screen panel (14 prims sampling CLUT row 2 of the system-UI TIM) — see [`subsystems/save-screen.md`](../subsystems/save-screen.md#sprite-asset-sources-continue--load-screen). |

### One-shot wrappers

[`run_probe.sh`](../../scripts/pcsx-redux/run_probe.sh) is the single
canonical shell harness for every probe. It accepts both env vars
(`LEGAIA_LUA`, `LEGAIA_SSTATE`, `LEGAIA_OUT`, …) and matching `--lua`
/ `--sstate` / `--out` / `--scenario` / `--fast` flags. Output
defaults to `captures/<probe-stem>/<iso-timestamp>/` so each run gets
a fresh per-run subtree.

```bash
# Default world-map probe (interpreter mode, Lua BPs fire).
bash scripts/pcsx-redux/run_probe.sh

# Pick a different probe.
bash scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_dump_slot4.lua

# Resolve the save state via a named scenario from scripts/scenarios.toml.
bash scripts/pcsx-redux/run_probe.sh --scenario cold_boot_pre_init \
    --lua scripts/pcsx-redux/autorun_countdown_trigger.lua

# Fast (recompiler) mode — drops `-interpreter -debugger`. Lua **BPs do
# NOT fire** under the recompiler, so this is only useful for
# vsync-event-only probes (e.g. autorun_dump_full_ram.lua).
bash scripts/pcsx-redux/run_probe.sh --fast \
    --lua scripts/pcsx-redux/autorun_dump_full_ram.lua
```

The earlier `run_world_map_probe.sh` / `run_fast_probe.sh` /
`run_dump_slot4.sh` wrappers were folded into this one runner.

### GDB-stub bridge (`gdb_probe.py`)

[`gdb_probe.py`](../../scripts/pcsx-redux/gdb_probe.py) is the
one-shot escape hatch. PCSX-Redux exposes a GDB Remote Serial Protocol
stub on TCP port 3333 (settings: *Emulator → GDB server port*); this
script speaks the protocol directly. Use it when the `.probe.toml`
state machine is overkill &mdash; ad-hoc reads, single-shot
"break-here-read-there" investigations, register dumps.

| Subcommand | Use |
|---|---|
| `read-mem ADDR LEN [--out F]` | Hex dump or raw bytes to file. ADDR is hex or a Ghidra symbol. |
| `read-regs` | Dump 38 PSX MIPS GPRs + PC. |
| `write-mem ADDR HEXBYTES` | Patch memory in-flight. |
| `when-pc-hits ADDR --read-mem A,L [--out F]` | One-shot: arm exec BP, continue, read on hit, disarm. |
| `watch ADDR LEN --kind {read,write,access}` | Insert a watchpoint, print the stop reply when it fires. |
| `selftest` | Run protocol-codec + client self-tests against an in-process mock server (no live emulator needed). |

When to use this vs `.probe.toml`:
* `.probe.toml` for **repeatable captures** that produce a CSV which
  `probe.py regress` can gate on.
* `gdb_probe.py` for **one-shot ad-hoc queries** &mdash; no schema, no
  scenario, no state machine to author.

```bash
# Read 512 bytes of the kingdom slot-4 region in-flight:
scripts/pcsx-redux/gdb_probe.py read-mem 0x8011A624 512

# Dump registers right now:
scripts/pcsx-redux/gdb_probe.py read-regs

# One-shot break-and-read: when the title overlay tick fires, dump the
# attract-countdown register:
scripts/pcsx-redux/gdb_probe.py when-pc-hits FUN_801DD35C \
    --read-mem _DAT_801EF16C,16
```

Symbol names resolve via the same `ghidra/scripts/symbols.json` the Lua
probe layer uses; misses raise with the regenerate-via hint. Hex
(`0x801DE840`, `801de840`) is always accepted.

### Analysing probe outputs (`probe.py`)

[`probe.py`](../../scripts/pcsx-redux/probe.py) is the Python-side
companion to a `.probe.toml` run. It operates on the CSV outputs and
provides four operations the Lua side intentionally doesn't try to do
in-emulator:

| Subcommand | Use |
|---|---|
| `probe.py summary RUN` | Header + row count + canonical fingerprint. |
| `probe.py fingerprint RUN` | SHA-256 over canonicalised rows. Independent of row order and of `--ignore`d columns. |
| `probe.py diff BASELINE CURRENT` | Set-diff: added / removed rows. Useful for inspecting why two runs differ. |
| `probe.py regress BASELINE CURRENT` | Fingerprint compare. Exits 0 on match, 1 on regression. Foundation for Phase G CI gating. |

`--ignore COL[,COL...]` drops named columns before comparison /
hashing. Use it for fields that naturally vary between runs without
representing a regression &mdash; most commonly `tick` (the per-bp hit
counter is order-dependent) and sometimes `pc` (when the same code path
gets reached via different inlining decisions across overlay rebuilds).

```bash
# Re-run a probe spec, compare against a committed baseline:
bash scripts/pcsx-redux/run_probe.sh --spec scripts/pcsx-redux/probes/xp_table_readers.probe.toml
scripts/pcsx-redux/probe.py regress \
    captures/baselines/xp_table_readers.csv \
    captures/xp_table_readers/<latest>/xp_table_readers.csv \
    --ignore tick
```

## Authoring a new probe

Two shapes are supported, in order of preference:

### Declarative .probe.toml (simple probes)

For "arm N breakpoints, dump K columns to CSV" or "settle then dump a
RAM region", the probe is a single TOML file under
[`scripts/pcsx-redux/probes/`](../../scripts/pcsx-redux/probes/) with
no Lua code at all. The shared
[`probes/_runner.lua`](../../scripts/pcsx-redux/probes/_runner.lua)
parses the spec via
[`lib/probe/toml.lua`](../../scripts/pcsx-redux/lib/probe/toml.lua)
and dispatches into
[`lib/probe/spec.lua`](../../scripts/pcsx-redux/lib/probe/spec.lua).

Schema (see
[`probes/xp_table_readers.probe.toml`](../../scripts/pcsx-redux/probes/xp_table_readers.probe.toml)
for the breakpoint-fan-out case and
[`probes/dump_full_ram.probe.toml`](../../scripts/pcsx-redux/probes/dump_full_ram.probe.toml)
for the RAM-dump case):

```toml
scenario        = "title_attract"   # informational; LEGAIA_SSTATE wins
capture_frames  = 600
output_path     = "my_probe.csv"
capture_columns = ["tick", "addr", "pc", "ra", "value_u32"]

[detail]                            # optional: first N hits get full
hits = 8                            # register/code/stack snapshots in a
path = "my_probe.detail.txt"        # .detail.txt sidecar

[[breakpoint]]                      # individual breakpoint
addr  = 0x80017EC8
kind  = "Exec"                      # "Exec" | "Read" | "Write"
width = 4
name  = "world_map_tick"

[[breakpoint_range]]                # fan out N adjacent breakpoints
base     = 0x8007123C
length   = 196                      # bytes
stride   = 4                        # bytes per bp
kind     = "Read"
name_fmt = "xp+0x%03X"              # %X / %x / %d = byte offset from base
```

Capture-column vocab (built into
[`lib/probe/spec.lua`](../../scripts/pcsx-redux/lib/probe/spec.lua)):
`tick`, `addr`, `offset`, `pc`, `ra`, `sp`, `width`,
`value_u8` / `value_u16` / `value_u32`.

Run it:

```bash
bash scripts/pcsx-redux/run_probe.sh \
    --spec scripts/pcsx-redux/probes/my_probe.probe.toml \
    --scenario title_attract     # or --sstate /path/to/state.sstate
```

Validate the schema (without launching PCSX-Redux):

```bash
python3 scripts/pcsx-redux/probes/_check_specs.py
```

If `lua5.1` is available, the validator also parses each spec via
`lib/probe/toml.lua` and asserts the structural output matches Python's
`tomllib` &mdash; catches divergence between the Lua TOML reader and
the canonical TOML spec.

### Lua autorun (bespoke probes)

For anything more elaborate (per-hit logic that depends on register
state, multi-state-machine probes, dynamic breakpoint arming, etc.),
write a Lua autorun. The fastest path:

1. Start from
   [`scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua`](../../scripts/pcsx-redux/autorun_slot4_consumer_pcs.lua)
   &mdash; the canonical thin probe (~145 lines) that uses the shared
   library for everything except the per-probe breakpoint body.
2. Edit the `PROBE_OFFSETS` (or your own probe-address list), the CSV
   header, and the per-hit row written from inside the breakpoint
   callback. The boot-delay / capture-vsync / disarm state machine
   comes from `probe.run({...})` &mdash; don't reimplement it.
3. Run with the harness:
   ```bash
   LEGAIA_LUA=scripts/pcsx-redux/autorun_your_thing.lua \
   LEGAIA_OUT=/tmp/your_probe.csv \
       bash scripts/pcsx-redux/run_probe.sh
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

## See also

- [`mednafen-automation.md`](mednafen-automation.md) - the save-state diff / bisect sibling of these live probes.
- [`overlay-capture.md`](overlay-capture.md) - capturing overlay RAM slices for Ghidra import.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) - the RAM addresses the probes break on and watch.
