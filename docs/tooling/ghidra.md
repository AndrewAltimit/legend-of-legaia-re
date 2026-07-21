# Ghidra setup

The static-analysis path: how this project disassembles `SCUS_942.54`, and how the per-function dumps the rest of the docs cite are produced.

**Reach for it when** you want to know what a function *is* - its disassembly, its decompiled C, who calls it. Ghidra runs headlessly in Docker and is driven by scripts, not by a GUI: you ask a question, a Jython script answers it into a file on the host.

**Two things to know before you start.**

*You need extracted disc files first.* The container mounts `./extracted` read-only, so run the [extraction pipeline](extraction.md) before importing anything.

*Static analysis alone will hit a wall.* Most of Legaia's game logic - the field/event VM, the dialog renderer, the actor / battle / menu VMs - is not in `SCUS_942.54` at all. It lives in RAM overlays paged in at `0x801C0000+`, so a function can be heavily used at runtime and have **zero static callers**. When that happens the answer is not here; it is in [overlay capture](overlay-capture.md) or the [static overlay pipeline](static-overlay-pipeline.md).

The container itself is `blacktop/ghidra:latest`, wrapped by `docker/ghidra.Dockerfile` to map the container user to the host's UID/GID, so files written into the bind-mounted `/projects` and `/scripts` come back owned by you.

## The short version

```bash
docker compose up -d ghidra          # once - leave it running

docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process SCUS_942.54 -noanalysis \
    -postScript /scripts/dump_funcs.py
```

Bring the service up **once** and issue one `exec` per query - don't restart it per command. Dumps land in `ghidra/scripts/funcs/<addr>.txt` on the host (gitignored: they are Sony-derived).

The rest of this page is the detail: [setup](#bringing-the-service-up), [importing](#importing-scus_94254), the [investigation patterns](#investigation-patterns), the [script catalogue](#script-catalogue), and the [LUI+ADDIU gotcha](#the-luiaddiu-gotcha) you will need the first time a cross-reference search comes back empty.

## Toolchain

- **Ghidra 12.x** in `blacktop/ghidra:latest`. Bundles OpenJDK 21 and stock Ghidra at `/ghidra`.
- **Jython 2.7** (bundled with Ghidra) for analysis scripts. Scripts must be **ASCII-only** - Jython 2 chokes on Unicode in source unless an encoding declaration is added.
- **PCSX-Redux** for runtime tracing. See [overlay capture](overlay-capture.md).

## Bringing the service up

```bash
# Build (auto-uses USER_ID / GROUP_ID from .env or defaults to 1000:1000)
docker compose build ghidra

# Start the long-running container
docker compose up -d ghidra
```

The service uses these mounts (from `docker-compose.yml`):

| Mount | Mode | Purpose |
|---|---|---|
| `./extracted` → `/data` | read-only | Disc-extracted files (BIN, TIM, TMD, etc.) |
| `./ghidra/projects` → `/projects` | read-write | Ghidra project DB (gitignored) |
| `./ghidra/scripts` → `/scripts` | read-write | Analysis scripts + per-function dumps |

If you've never built the wrapper before, first run also handles UID/GID matching - see the comment at the top of `docker-compose.yml` for `.env` overrides.

## Importing SCUS_942.54

PSX executables are PSX-EXE format: skip the 0x800-byte header, base address `0x80010000`.

```bash
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia \
    -import /data/SCUS_942.54 \
    -loader BinaryLoader \
    -loader-baseAddr 0x80010000 \
    -processor MIPS:LE:32:default
```

> **Use `MIPS:LE:32:default`, not `MIPS:LE:32:R3000`.** Ghidra rejects `R3000` as `Unsupported language`. The PSX R3000A is a strict subset of MIPS-I; the default little-endian profile handles it correctly.

After import, run analysis:

```bash
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process SCUS_942.54
```

This takes a few minutes and populates the database with functions, references, and decompilation results.

## The LUI+ADDIU gotcha

MIPS forms 32-bit constants from two 16-bit immediates:

```asm
lui   r1, 0x801C       ; r1 = 0x801C0000
addiu r1, r1, 0x70F0   ; r1 = 0x801C70F0
```

**Ghidra's reference manager does NOT auto-resolve this combination across instructions.** A direct query "give me xrefs to `0x801C70F0`" returns zero results, even when the address is heavily used.

Workaround: `ghidra/scripts/find_lui_writers.py` walks instructions, tracks per-register LUI immediates, and flags `addiu` / load / store offsets that combine with a tracked LUI to land in a target range. Use it any time you suspect a static address is being missed.

```bash
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process SCUS_942.54 -noanalysis \
    -postScript /scripts/find_lui_writers.py
```

Modify `LO` / `HI` constants in the script to scan a different range.

Computed addresses are still missed - `lw r4, 0x18(r3)` where `r3 = 0x80080000 + index*4` can't be statically resolved when `index` is only known at runtime. Functions reading from arrays via runtime-computed indexing won't appear in xref lists; for these, dynamic analysis with watchpoints is the only static-tool-free path.

## Investigation patterns

### "Find what writes / reads a global"

Use `find_lui_writers.py` with `LO` / `HI` narrowed to the target address - it
catches the LUI+ADDIU/load/store combos that Ghidra's reference manager misses.

### "Find callers of a function"

Use `find_callers_of.py` (edit `TARGETS_HEX` to the entry point) or
`dispatcher_callers.py` for the asset-dispatcher / LZS chain specifically.

### "Is this function actually called?"

The reference manager is unreliable for indirect calls. Use:
- `find_callers_of.py` for direct `jal` references.
- `find_addr_data.py` to find the address as data (function-pointer tables, callbacks).

If both return zero hits, the function has no static caller *in the program currently loaded into Ghidra* - that's NOT the same as "dead code in retail". Most game logic lives in RAM-loaded overlays at `0x801C0000+` that aren't part of `SCUS_942.54`. The negative result bounds where the caller can possibly live, but doesn't prove the function unreachable.

### "Where does this constant address get used?"

If the address is referenced via `lui+addiu`, the reference manager will miss it. Use `find_lui_writers.py` with `LO`/`HI` narrowed to your target range.

### "Ghidra says nothing writes / reads this global, but I know something does"

Common when the address is materialized by `lui+addiu` and then *passed* to a helper (so the actual `sw`/`lw` is in the helper, against `$a0`/`$a1`), OR when it's stored as a function-argument base that an `addu` reroutes (so the constant tracker bails and the final `sw` doesn't appear in the xref database).

Use `find_addr_materializers.py` to walk every instruction in a program, track per-register `lui` + `addiu` pairs, and report every site where the combined value lands on one of your target addresses - plus the next 6 instructions for use-classification (store base = writer, load base = reader, `jal`/`jalr` follows = address passed as argument).

```bash
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process SCUS_942.54 -noanalysis \
    -postScript /scripts/find_addr_materializers.py \
    0x8007C018 0x8007BB38 0x8007B7DC
```

Arguments may be decimal or hex (`0x...` prefix). Multiple addresses are scanned in a single pass. Alternative: set `GHIDRA_FIND_ADDRS='0x8007c018,0x8007bb38'` and run without args.

The pattern this catches (the actual installer for `DAT_8007C018` at `FUN_80026B4C` - missed by the reference manager):

```asm
lui   v0, 0x8008
lui   v1, 0x8008
lw    v1, -0x488c(v1)     ; v1 = *DAT_8007B774 (index counter)
addiu v0, v0, -0x3fe8     ; v0 = 0x8007C018   <-- the materializer site
sll   v1, v1, 0x2         ; v1 = idx * 4
addu  v1, v1, v0          ; v1 = idx*4 + 0x8007C018
sw    a0, 0(v1)           ; store to table    <-- the missed writer
```

The reference manager tracks `lui+addiu` pairs but bails when `addu` mixes the propagated constant with a value loaded from memory. So `sw a0, 0(v1)` is invisible to it - but the `addiu v0, v0, -0x3fe8` site IS visible to a manual scanner that knows the combination forms the target address. Once you see the materializer, the surrounding 6 instructions usually make the role obvious.

### "What format does this PROT entry use?"

Empirical workflow:
1. `xxd extracted/PROT/<entry>.BIN | head -5` - eyeball the header.
2. Try each known parser:
   - `asset stream <file>` - DATA_FIELD streaming.
   - `asset describe <file>` - descriptor format (when applicable).
   - `lzs-decode raw --size N <file>` - top-level LZS.
   - `asset categorize <DIR>` - runs every detector and emits a per-class breakdown.
3. If nothing matches, dig into the function that loads it (find by reversing the call site).

## Adding a new function dump

1. Edit `ghidra/scripts/dump_funcs.py`'s `TARGETS` list to add the entry-point address.
2. Run the dump:
   ```bash
   docker compose exec ghidra /ghidra/support/analyzeHeadless \
       /projects legaia -process SCUS_942.54 -noanalysis \
       -postScript /scripts/dump_funcs.py
   ```
3. Open `ghidra/scripts/funcs/<addr>.txt` and analyze.
4. Update [`reference/functions.md`](../reference/functions.md) if it's a notable entry point.

## Script catalogue

The Ghidra-side scripts (Jython, run inside the container) live in `ghidra/scripts/`. Edit the `TARGETS` / `LO` / `HI` constants at the top of any script to point at the addresses you want to trace.

Every script needs the `# @runtime Jython` header line (with `# @category Legaia`); without it the headless analyzer routes `.py` to the PyGhidra (Python 3) provider, which the image doesn't enable, and the load fails with *"Ghidra was not started with PyGhidra"*.

**Symbol re-application**

| Script | Purpose |
|---|---|
| `apply_known_symbols.py` | Re-apply this project's pinned function names to a fresh import of `SCUS_942.54`. Reads the curated `(address, name, role-comment)` table in `known_symbols.py` and names each function + sets a one-line PLATE comment, so the asset/loader/CD/dispatch cluster is readable immediately instead of a wall of `FUN_xxxxxxxx`. The clean-room counterpart to a PsyQ FidDB pass (replays our own RE labels, no external SDK). SCUS-resident (`0x80010000..0x8007C000`) only - RAM overlays alias by address, so naming them blind would mislabel. Run with `-process SCUS_942.54 -noanalysis -postScript /scripts/apply_known_symbols.py`. |

**Per-function dumps**

| Script | Purpose |
|---|---|
| `dump_funcs.py` | Dump disassembly + decompiled C for a list of function entry points. Output goes to `ghidra/scripts/funcs/<addr>.txt`. |
| `force_disasm_dump.py` | Force-disassemble + create-function at addresses Ghidra didn't auto-detect (JALR-only entry points), then dump. Validates the result has `>=8` instructions ending in `jr $ra` before committing the function. |
| `resolve_render_tail.py` | Companion to the [trace-driven coverage](playthrough-coverage.md) program: for a hardcoded list of overlay trace-hit addresses (default = the S5 battle render-tail), reports `getFunctionContaining` + `memory.contains` per hit in the currently-open overlay program - separating "in-program but un-analyzed" (a create+dump target) from "out-of-program" (a different co-resident overlay). |
| `dump_battle_rendertail.py` | Disassemble + create-function + dump the in-`0898` battle render-tail functions the trace found un-analyzed (e.g. `FUN_801E0080`). Output naming matches the overlay dumps (`overlay_battle_action_<addr>.txt`). Run against `overlay_battle_action.bin`. |
| `dump_battle_rendertail_0x801f.py` | Dump the `0x801F` render tail the older windowed `overlay_battle_action.bin` import stops short of. Run against a **full-length** re-import of the `0898` blob at base `0x801CE818` (`-import /data/overlays/overlay_battle_action_0898.bin -loader BinaryLoader -loader-baseAddr 0x801CE818`, span `0x801CE818..0x801F8018`); resolves the `0x801F0xxx` hits cleanly (`FUN_801F0450`). The `0x801F6xxx`/`0x801F7xxx` sub-cluster is *not* `0898` (a resident-RAM comparison found the live bytes differ) - it is the co-resident sparring-tutorial overlay PROT 0967 (see next row). |
| `dump_effect_overlay_0967.py` | Dump the battle **sparring-tutorial overlay PROT 0967**, co-resident at base `0x801F69D8` during the Tetsu tutorial fight (overlapping `0898`'s rodata tail). Run against a fresh import of `/data/PROT/0967_xxx_dat.BIN` at `-loader-baseAddr 0x801F69D8`; create+dumps the S5 `0x801F6xxx`/`0x801F7xxx` hit functions (message-pacing driver `FUN_801F71E0` + the step text emitters) as `overlay_effect_0967_<addr>.txt`. |
| `dump_menu_inventory_refs.py` | Content-grep dumper: decompiles every function in the current program and dumps the C for any whose body mentions a configurable needle list (default: the inventory array `0x80085958` + the SCUS accessor family + the `gp+0x2D2/0x2D4/0x2D6` window registers). Robust against the LUI+ADDIU xref gap (matches decompiled text, not the reference manager). Used to audit `overlay_menu.bin` for raw-index inventory writes (found none - every mutation goes through the bounds-checked helpers). |
| `dump_arts_input.py` | Decompiles the battle-overlay (0898) arts-combo execution cluster: the Arms resolver `FUN_801EC3E4` (with its caller list from the reference manager) plus every function referencing the move-power tables (`0x801F4F5C` per-move power, `0x801F64E4` power-byte, `0x801F4E63` 128-byte action map). Confirms the resolver is dispatched by a runtime function pointer (0 static callers) and that the move-power referrers are damage/action-step builders, not the arts-input bar builder. |
| `dump_terrain_trigger.py` | Per-overlay-aware dumper for the world-map render-pipeline chain (`FUN_801D7EA0` / `FUN_801D8258` / `FUN_801D1344` / `FUN_80016444` + SCUS callers and the 0897 relocation copy). Uses `prog.getMemory().contains(addr)` to skip any TARGET that isn't mapped in the current program, so the same script can be run against SCUS plus each overlay and only emits files for the addresses that exist there. Output naming: `<program_label>_<addr>.txt`. |
| `trace_field_loader.py` | Targeted trace of the per-scene field-file loader `FUN_8001f7c0`; pins the loader's **dual-mode** dispatch. → [detail](#trace_field_loaderpy-detail) |
| `find_mesh_chain_writer.py` | Finds the writer of the field/world-map actor's mesh-chain pointer `actor+0x44` (the chain `FUN_8001ADA4` case 5 draws). Scans for non-stack `sw/sh …,0x44(reg)`, scores each containing function by pool-table (`DAT_8007C018`) refs / TMD object-stride (`0x1c`) math / actor-field reads, dumps the top candidates. Pins the resolver chain for the walk view: `FUN_80024d78` builds `actor+0x44` from `DAT_8007C018[*(u16*)(actor+0x64)]`, and `FUN_80020f88` sets `actor+0x64 = .MAP_record[+0x10] + DAT_8007b6f8 (prefix)` → so the per-object pool index is `record[+0x10] + prefix`. |

###### `trace_field_loader.py` detail

Targeted trace of the per-scene field-file loader `FUN_8001f7c0`:

- Dumps the load chain; reads the path-template + extension string constants (`DATA\FIELD\`, ext globals `DAT_8007b3bc=".MAP"` / `DAT_8007b3c4=".PCH"`, `\efect.dat`).
- Finds LUI+ADDIU/mem accessors of the scene-name (`0x80084548`), PROT-index (`0x80084540`) and dual-mode gate (`0x8007b868`/`0x8007b8c2`) globals.
- Verifies the in-RAM PROT TOC base (`0x801c70f0`) inside the retail resolver `FUN_8003e8a8`.

Pins the loader's **dual-mode** dispatch: retail resolves the `.MAP` by **PROT index** (`FUN_8003e8a8(param_3=*(0x80084540))`, e.g. `map01` → entry `0085`), while the `break 0x103` path (`FUN_800608f0`) is the **dev-host `fopen`** of `DATA\FIELD\<scene>.MAP` (no extension→PROT map, never taken on retail).

**LUI+ADDIU and address-resolution helpers**

| Script | Purpose |
|---|---|
| `find_lui_writers.py` | Generic LUI+ADDIU resolver. Walks instructions, tracks per-register LUI immediates, reports any combined access landing in `[LO, HI]`. Critical for finding references the ref manager misses. Edit `LO`/`HI` per run. |
| `find_addr_materializers.py` | Per-address LUI+ADDIU materializer finder. Reports every `addiu` whose combined value lands on one of the targets, plus the next 6 instructions for use-classification. Accepts addresses via `getScriptArgs()` or the `GHIDRA_FIND_ADDRS` env var - no source edit needed per invocation. See the LUI+ADDIU + ADDU+SW investigation pattern above. |
| `find_addr_data.py` | Search the program memory for any 4-byte LE word equal to a target address - catches function-pointer tables. |
| `find_data_word.py` | Generic u32-LE-literal scanner across every initialized memory block, with surrounding-dword context. Useful when you suspect a function pointer is stuffed in a dispatch table somewhere; reports the containing function (if any) plus 8 dwords of surrounding data so the table structure is visible. |
| `find_terrain_emitter_caller.py` | Combined ref-manager + LUI+ADDIU + ori + `jal` / `j` direct-target sweep against a configurable target-address set. Reports every overlay where each target is loaded as an immediate, stored / loaded via `base+offset`, or called directly. Useful pattern for any "who calls function X across the overlay set?" question: edit `TARGET_ADDRS` and `TARGETS_HEX`, run against each `-process <overlay>` in turn. The cross-program `jal` sweep is the unlock - Ghidra's ref manager only sees refs internal to one program. |
| `find_string_xrefs.py` | Resolve dev-path string literals (`h:\\prot\\...`) to RAM addresses and dump every code site that references them. |

**Caller / xref helpers**

| Script | Purpose |
|---|---|
| `find_callers_of.py` | Generic "callers of these target functions" tool. Edit `TARGETS_HEX`. |
| `find_callers_of.py` + `find_addr_data.py` | Combined check for "is this function actually called?" - direct `jal` plus address-as-data. |
| `dispatcher_callers.py` | Callers of `FUN_8001f05c` (asset dispatcher) and `FUN_8001a55c` (LZS). |
| `find_jalr_handlers.py` | Locate dispatch-table indirect calls (`lw R, +0x10(...)` followed by `jalr R`). |

**Subsystem-targeted scanners**

| Script | Purpose |
|---|---|
| `find_sound_path_builders.py` | LUI+ADDIU pairs landing in the sound-driver string cluster `0x8007B380..0x8007B3D0` (see [`docs/formats/sound-driver.md`](../formats/sound-driver.md)). |
| `find_debug_flag_writers.py` | Two-pass scan for writers/readers of the documented debug-flag RAM band `0x8007B400..0x8007BCFF`. |
| `find_move_table_consumers.py` | Readers of the MOVE / MOVE2 buffers (`0x8007B888` / `0x8007B840`). |
| `find_anm_buffer_users.py` | Readers/writers of the ANM buffer pointer (`_DAT_8007b7c8`). |
| `find_mes_buffer_users.py` | Readers/writers of the MES dialog buffer pointer (`_DAT_8007b8a8`). |
| `find_tmd_renderer.py` | Readers of the TMD pointer table at `0x8007C018 + idx*4`. |
| `find_gte_users.py` | Count COP2 / GTE instructions per function - surfaces renderer + transform candidates. |
| `find_streaming_consumers.py` | DATA_FIELD streaming buffer trail: callers of `FUN_8002541c` plus direct readers of `0x8007b85c`. |
| `find_xp_table_readers.py` | LUI+ADDIU resolver targeting the address originally (and wrongly) documented as the retail XP table (`0x8007123C..0x80071300`). **Superseded:** the real XP curve is `DAT_80076AF4`, read by the overlay applier `FUN_801E9504`, not anything near `0x8007123C` (an off-by-`0x800` confusion; the corrected `0x80070A3C` is a sin-LUT slice) - see [`subsystems/level-up.md`](../subsystems/level-up.md#xp-table). Kept only for the generic LUI+ADDIU resolver pattern; retarget before re-running. |
| `find_xp_table_all_overlays.py` | Same scan, recursive across every imported program. Returns zero hits - but that finding is moot in the current framing (see the row above). |
| `find_prot_consumers.py` | Static map of every call site that passes a constant PROT index to the LBA resolver chain. |
| `find_scene_name_writers.py` | Writers of the scene-name buffer at `0x80084548`. |
| `find_field_loader_callers.py` | Callers of the field/town asset loaders (`FUN_8001f7c0` / `FUN_800255b8`) with arg-prep context. |
| `asset_table_xrefs.py` | Xrefs to and around `0x801C70F0` (the in-RAM PROT TOC). |
| `find_effect_bundle_consumers.py` | Effect-bundle init / spawn / walker (run on an imported battle overlay). |
| `dump_field_locomotion_cluster.py` | Re-decompile the 0897 field camera / region cluster (`801db81c` / `801dbec4` / `801f5748`) + raw-disassemble the surrounding window. Read-only; surfaces the data holes that corrupt the decompiles. |
| `fix_field_locomotion_flow.py` | DB-modifying repair for the same cluster: force-disassemble the `jal 0x8003ce9c` (non-returning operand reader) data holes, drop mid-block fake `FUN_` entries, re-create functions at real `addiu sp,sp,-N` prologues, then re-decompile. General pattern for any overlay region split into bogus mid-block functions by a non-returning-call hole. |
| `dump_player_locomotion_integrator.py` | Dumps the player free-movement controller `FUN_801d01b0` + collision `FUN_801cfe4c` / `FUN_801cf9f4` + pad-remap `func_0x800467e8` / `FUN_80046494`, pinned by the `autorun_player_pos_watch.lua` write-watchpoint. `in_program` guards run it across SCUS + overlay_0897. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). |
| `dump_4c_jumptables.py` | Dumps the field-VM main dispatcher JT (`0x801E00F4`) + the `0x4C` outer-nibble JT (`0x801CEE60`, 16 entries) with each target's containing function. Use to pin a `0x4C` sub-opcode's exact nibble when the decompiler's reconstructed `case` numbering is ambiguous - e.g. confirmed the collision-grid paint is nibble-7 (`0x801e1c64`), not the decompile's misleading "case 5". |

**Game-mode state-machine recon**

| Script | Purpose |
|---|---|
| `find_field_program_xrefs.py` | Resolve the field-program / mode-name string literals and dump xrefs. |
| `find_game_mode_dispatcher.py` | Hunt for the game-mode dispatcher via the documented mode strings. |
| `find_game_mode_writers.py` | Writers of the game-mode register at `gp[0x524]` / `gp[0x494]`. |
| `find_gp_init_and_mode_table.py` | Locate `$gp` initialization and readers of the 28-entry mode table at `0x8007078C`. |
| `find_per_mode_callers.py` | Direct or indirect callers of any handler in the mode table. |

**Overlay capture and analysis**

| Script | Purpose |
|---|---|
| `find_overlay_candidates.py` | Stand-alone Python (no Ghidra) - scans extracted PROT entries for MIPS-code-likelihood and ranks candidates. |
| `dump_overlay.lua` | PCSX-Redux Lua: dump the runtime overlay code window `0x801C0000..0x801EFFFF` to `/tmp/`. |
| `import_overlay.sh` | Bash wrapper that imports + analyzes a captured overlay dump as Raw Binary at base `0x801C0000`. |
| `find_overlay_calls.py` | Every call (jal or resolved jalr) into the RAM-resident overlay region `0x801C0000..0x801FFFFF`. |
| `find_overlay_asset_loads.py` | Run on an imported overlay program: const-track every `jal` to a known SCUS asset loader and emit a CSV of `loader,prot_index_or_string,caller_func,call_site`. |
| `inventory_overlay.py` | Per-program function inventory. Emits `inventory_<programname>.csv` with one row per function (entry / size / outgoing / incoming / top callees). |
| `list_overlay_functions.py` | List functions in the active overlay program sorted by size, with outgoing-call counts. |
| `list_programs.py` | List every program currently in the Ghidra project. |

**Static-analysis utilities**

| Script | Purpose |
|---|---|
| `explore.py` | Dump a JSON report of `SCUS_942.54`: every function with an LZSS-decoder fingerprint score, plus every defined string and its inbound xrefs. |

Cross-cutting helpers under `scripts/` (host-side, not Ghidra):

| Script | Purpose |
|---|---|
| `scripts/ci/function-coverage.py` | Citation-ranked missing-helper tracker over the function dumps. |
| `scripts/ghidra-analysis/call-graph.py` | `callees` / `callers` / `xref` over the dumps; replaces grep-across-files. |
| `scripts/asset-investigation/scene-asset-detect.py` | Joins `categorize.json` with TIM/TMD scan hits to surface unknown-bucket entries that look like scene bundles. |
| `scripts/ghidra-analysis/bulk-import-overlays.sh` | Reads `find-overlay` output, imports each high-score candidate, runs analysis + the inventory dumper. |
| `scripts/ghidra-analysis/extract-mednafen-overlay.py` | Slices `0x801C0000-0x80200000` (256 KB) out of a gzipped mednafen save state. |
| `scripts/ghidra-analysis/analyze-overlay.sh` | One-shot capture pipeline: decompress save → slice → import → emit asset-load CSV. |

## Known dev paths in the binary

`SCUS_942.54` contains leftover Windows paths from the dev environment. Useful for guessing format families:

```
h:\PROT\FIELD\
DATA\FIELD\
data\field\player.lzs
h:\prot\all\data\field\player.lzs
h:\prot\field\card\tim.dat
h:\prot\battle\etim.dat
\tim.dat
\move.mdt
```

The `h:\` prefix indicates a Windows dev box. The runtime doesn't actually open these paths in retail (no real `h:\` drive on a PSX); the strings are leftover format artefacts that point at where each subsystem's data lives in PROT.

## Decompiler artifacts that have produced false claims

Ghidra's C output is a *rendering* of the instruction stream, and each artifact below has already put a wrong statement into this repo's committed docs. Port and document from the disassembly; treat the C as a hint that tells you where to look.

| Artifact | What it looks like | What settles it |
|---|---|---|
| Dropped register arguments | A call printed `f(1)` whose callee reads three arguments. Ghidra infers the signature from one call site, so arguments left untouched in `a1`/`a2` never appear. | Read the `jal` and its delay slot; check whether `a1`/`a2` are written between the caller's prologue and the call. |
| `\|\|` rendered as nested `if`s | Two siblings that share one branch pair look like they use different operators, inventing a behavioural difference. | Compare the branch pairs themselves, not the C. De Morgan makes `if (w) { if (h) }` and `w == 0 \|\| h == 0` the same predicate. |
| Reordered or dropped stores | A store hoisted above its neighbours, or omitted entirely - so "field X is copied from field Y" and "only three of four slots are written" both come out wrong. | Take store order and store count from the instruction stream. |
| Hand-written annotations | A Ghidra label or plate comment (`path_opener`, "dev path -> PROT index via CDNAME map") read as fact. It is somebody's earlier guess. `FUN_8003E6BC` was named `path_opener`, which reads as a filesystem abstraction retail could plausibly own - its body is `strcpy` -> `break 0x103` -> fseek/fread/fclose, an unservable dev-station host trap. The name alone kept a backwards branch polarity alive across a dozen pages. | Read the body. An annotation is provenance-free. |
| `size=1 bytes, 0 instructions` | A dump with decompiled C but an empty disassembly section - nothing to cross-check the C against. Corpus-wide: **207** dumps carry no disassembly section at all and **380** report zero instructions, ~16% of 3,624. | Disassemble from `SCUS_942.54` directly: text VA `0x80010000`, file offset `0x800 + va - base`. |
| Dump-sweep negatives | "An exhaustive sweep of the dumped corpus finds no reader of X." The sweep ran over dumps, and ~16% of them have no instructions to sweep. The searcher sees zero hits either way, so the failure is silent and looks like a clean result. | Sweep **bytes**, not dumps - word-wise capstone over `extracted/SCUS_942.54` and the images in `extracted/overlays/`. State coverage explicitly ("no reader in SCUS exhaustive, 15 overlays; 11 dump-only") instead of asserting exhaustiveness. |
| Mis-based dumps | Printed addresses are a property of the load base a dump was imported at. Get it wrong and every address is wrong by a constant while the instruction text stays plausible. 263 dumps are shifted, 167 of them by exactly `0xE818`. A filename prefix is not evidence of base correctness. | Verify against the extracted image, and see [`dump-corpus-integrity.md`](dump-corpus-integrity.md) - it carries the census, the clusters, and a re-runnable checker. |
| `unaff_*` / `in_stack_*` | Read as proof the address is a mid-function fragment. But leaf functions legitimately open on `lw`, `unaff_gp` is ordinary gp-relative addressing, and a prologue can sit several instructions in. | A fragment is proven by a missing `addiu sp,sp,-N` in the **disassembly**, plus callee-saved reads with no matching save.  |
| Absolute-only address sweeps | A "no static writers" claim from a sweep that searched only the absolute `lui`+offset form. MIPS reaches the same address gp-relative, and that form carries a different immediate, so it is invisible to the scan. `0x8007B8C2` was recorded as writer-less across 2661 dumps; the writer is `0x80015F08 sh v0,0x5aa(gp)`. The same form also hides **reads**: `0x8007B8C2` has three gp-relative `lh 0x5aa(gp)` sites (`0x80015FD4`/`0x80016038`/`0x8001631C`) an absolute-only sweep undercounts, so its read total is 43, not the 40 an absolute-form scan finds. | Resolve `gp` first (`lui gp` / `addiu gp,gp` in the entry stub), then scan **both** forms - absolute `lui base`+offset and `gp`-relative - before asserting anything about writers **or reader counts**. |

The label-call idiom (intra-function labels promoted to fake `FUN_` entries) is the eighth member of this family; it has its own catalogue in [`script-vm.md`](../subsystems/script-vm.md#intra-function-label-catalogue).

The absolute-only-sweep row is worth dwelling on, because it is the one that compounds. "Zero writers" became "BSS zero-init establishes the value", which became "retail runs with the flag at 0", which inverted the documented polarity of `_DAT_8007B8C2` across a dozen pages and two crates. Neither downstream inference was independently checked - and the second was wrong on its own terms too, since `SCUS_942.54`'s PS-X EXE header has `b_size = 0` and the BIOS therefore clears no BSS here at all. Treat a negative result about writers as a claim needing the same evidence bar as a positive one.

### Two rules for any negative, code or data

The artifacts above are about misreading what a sweep *found*. These two are about
misreading what a sweep *didn't* find, and they are not specific to dumps - they
bite equally on negatives over disc bytes, where the range is bounded and the sweep
really can be exhaustive.

**A negative needs a positive control.** Before reporting "0 hits", run the *same
scanner and the same validator* over a corpus where the thing is known present, and
show it finds exactly the known instances - no more, no fewer. A negative produced
by an unvalidated detector is not evidence of absence; it is indistinguishable from
a detector that finds nothing anywhere. State the control's numbers next to the
negative's, because the control is what makes the negative load-bearing.

The worked example is the `"ME"` archive negative in
[`battle-data-pack.md`](../formats/battle-data-pack.md#the-me-footprint-sweep): 5 raw
magic hits across the player files, 0 validating. On its own that is unfalsifiable.
The same scanner over `readef.DAT`, where the archives are documented, accepts
exactly 8 out of 151 raw hits - one per documented slot, with the documented entry
counts, zero false positives and zero false negatives. 143 of 151 hits reject, so
the validator is doing real work, which is precisely what licenses reading the
player-file zero as absence.

**Structural fit is not validation.** Size tables that fit, offsets that align,
counts that look plausible - these produce convincing intermediate numbers that
survive a shallow check, and they are the reason a sweep reports a hit it should
have rejected. Decompose every near-miss before calling it one. Two hits from the
same sweep pass the `"ME"` size-table fit test and still fail on the body chain and
the codec; a validator stopping at "the sizes fit" would have reported false
positives in the very corpus used to prove it works.

The mirror case is a near-miss that is really pool overlap. Scanning the PROT 867
monster record for the steal table, byte offset `0x48` agrees with the SCUS steal
item for 31 of 185 ids - far above the noise floor of 7, and readable as "nearly the
field". It is not: `0x48` is `drop_item`, and steal and drop draw from the same
39-item consumable pool, so the agreement is expected and none of those 31 also
agree on chance at `0x49`. A rate well above chance is a prompt to explain the
mechanism, not evidence of a hit.

Both rules cut the same way as the rest of this page: the artifact is in the
*rendering* of the result, not the bytes. A sweep's output is a rendering too.

## See also

- [`docs/reference/functions.md`](../reference/functions.md) - the canonical directory of Ghidra-traced entry points these scripts dump.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) - RAM map + globals the LUI+ADDIU writer hunts resolve.
- [`docs/tooling/port-catalog.md`](port-catalog.md) - tracks which dumped functions are documented / ported.
- [`docs/tooling/extraction.md`](extraction.md) - the disc-side extraction that feeds `extracted/` into the container.
