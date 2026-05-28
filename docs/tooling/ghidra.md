# Ghidra setup

The static-analysis path. Ghidra is run headlessly inside the `blacktop/ghidra:latest` Docker image, wrapped by `docker/ghidra.Dockerfile` to map the container user to the host's UID/GID so files written into the bind-mounted `/projects` and `/scripts` directories come back as the host user.

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
| `dump_terrain_trigger.py` | Per-overlay-aware dumper for the world-map render-pipeline chain (`FUN_801D7EA0` / `FUN_801D8258` / `FUN_801D1344` / `FUN_80016444` + SCUS callers and the 0897 relocation copy). Uses `prog.getMemory().contains(addr)` to skip any TARGET that isn't mapped in the current program, so the same script can be run against SCUS plus each overlay and only emits files for the addresses that exist there. Output naming: `<program_label>_<addr>.txt`. |
| `trace_field_loader.py` | Targeted trace of the per-scene field-file loader `FUN_8001f7c0`: dumps the load chain, reads the path-template + extension string constants (`DATA\FIELD\`, ext globals `DAT_8007b3bc=".MAP"` / `DAT_8007b3c4=".PCH"`, `\efect.dat`), finds LUI+ADDIU/mem accessors of the scene-name (`0x80084548`), PROT-index (`0x80084540`) and dual-mode gate (`0x8007b868`/`0x8007b8c2`) globals, and verifies the in-RAM PROT TOC base (`0x801c70f0`) inside the retail resolver `FUN_8003e8a8`. Pins the loader's **dual-mode** dispatch: retail resolves the `.MAP` by **PROT index** (`FUN_8003e8a8(param_3=*(0x80084540))`, e.g. `map01` → entry `0085`), while the `break 0x103` path (`FUN_800608f0`) is the **dev-host `fopen`** of `DATA\FIELD\<scene>.MAP` (no extension→PROT map, never taken on retail). |
| `find_mesh_chain_writer.py` | Finds the writer of the field/world-map actor's mesh-chain pointer `actor+0x44` (the chain `FUN_8001ADA4` case 5 draws). Scans for non-stack `sw/sh …,0x44(reg)`, scores each containing function by pool-table (`DAT_8007C018`) refs / TMD object-stride (`0x1c`) math / actor-field reads, dumps the top candidates. Pins the resolver chain for the walk view: `FUN_80024d78` builds `actor+0x44` from `DAT_8007C018[*(u16*)(actor+0x64)]`, and `FUN_80020f88` sets `actor+0x64 = .MAP_record[+0x10] + DAT_8007b6f8 (prefix)` → so the per-object pool index is `record[+0x10] + prefix`. |

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
| `find_xp_table_readers.py` | LUI+ADDIU resolver targeting the address originally documented as the retail XP table (`0x8007123C..0x80071300`). **The address was wrong** (off-by-`0x800` file/virtual confusion; the bytes at the corrected address `0x80070A3C` are a sin-LUT slice, not an XP table — see [`subsystems/level-up.md`](../subsystems/level-up.md#xp-table)). Kept for the LUI+ADDIU resolver pattern itself, which is generic; retarget the address before re-running. |
| `find_xp_table_all_overlays.py` | Same scan, recursive across every imported program. Returns zero hits — but that finding is moot in the current framing (see the row above). |
| `find_prot_consumers.py` | Static map of every call site that passes a constant PROT index to the LBA resolver chain. |
| `find_scene_name_writers.py` | Writers of the scene-name buffer at `0x80084548`. |
| `find_field_loader_callers.py` | Callers of the field/town asset loaders (`FUN_8001f7c0` / `FUN_800255b8`) with arg-prep context. |
| `asset_table_xrefs.py` | Xrefs to and around `0x801C70F0` (the in-RAM PROT TOC). |
| `find_effect_bundle_consumers.py` | Effect-bundle init / spawn / walker (run on an imported battle overlay). |
| `dump_field_locomotion_cluster.py` | Re-decompile the 0897 field camera / region cluster (`801db81c` / `801dbec4` / `801f5748`) + raw-disassemble the surrounding window. Read-only; surfaces the data holes that corrupt the decompiles. |
| `fix_field_locomotion_flow.py` | DB-modifying repair for the same cluster: force-disassemble the `jal 0x8003ce9c` (non-returning operand reader) data holes, drop mid-block fake `FUN_` entries, re-create functions at real `addiu sp,sp,-N` prologues, then re-decompile. General pattern for any overlay region split into bogus mid-block functions by a non-returning-call hole. |
| `dump_player_locomotion_integrator.py` | Dumps the player free-movement controller `FUN_801d01b0` + collision `FUN_801cfe4c` / `FUN_801cf9f4` + pad-remap `func_0x800467e8` / `FUN_80046494`, pinned by the `autorun_player_pos_watch.lua` write-watchpoint. `in_program` guards run it across SCUS + overlay_0897. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). |
| `dump_4c_jumptables.py` | Dumps the field-VM main dispatcher JT (`0x801E00F4`) + the `0x4C` outer-nibble JT (`0x801CEE60`, 16 entries) with each target's containing function. Use to pin a `0x4C` sub-opcode's exact nibble when the decompiler's reconstructed `case` numbering is ambiguous — e.g. confirmed the collision-grid paint is nibble-7 (`0x801e1c64`), not the decompile's misleading "case 5". |

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
| `scripts/function-coverage.py` | Citation-ranked missing-helper tracker over the function dumps. |
| `scripts/call-graph.py` | `callees` / `callers` / `xref` over the dumps; replaces grep-across-files. |
| `scripts/scene-asset-detect.py` | Joins `categorize.json` with TIM/TMD scan hits to surface unknown-bucket entries that look like scene bundles. |
| `scripts/bulk-import-overlays.sh` | Reads `find-overlay` output, imports each high-score candidate, runs analysis + the inventory dumper. |
| `scripts/extract-mednafen-overlay.py` | Slices `0x801C0000-0x80200000` (256 KB) out of a gzipped mednafen save state. |
| `scripts/analyze-overlay.sh` | One-shot capture pipeline: decompress save → slice → import → emit asset-load CSV. |

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

## See also

- [`docs/reference/functions.md`](../reference/functions.md) — the canonical directory of Ghidra-traced entry points these scripts dump.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) — RAM map + globals the LUI+ADDIU writer hunts resolve.
- [`docs/tooling/port-catalog.md`](port-catalog.md) — tracks which dumped functions are documented / ported.
- [`docs/tooling/extraction.md`](extraction.md) — the disc-side extraction that feeds `extracted/` into the container.
