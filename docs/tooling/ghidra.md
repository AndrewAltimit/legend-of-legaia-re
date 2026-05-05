# Ghidra setup

The static-analysis path. Ghidra is run headlessly inside the `blacktop/ghidra:latest` Docker image, wrapped by `docker/ghidra.Dockerfile` to map the container user to the host's UID/GID so files written into the bind-mounted `/projects` and `/scripts` directories come back as the host user.

## Toolchain

- **Ghidra 12.x** in `blacktop/ghidra:latest`. Bundles OpenJDK 21 and stock Ghidra at `/ghidra`.
- **Jython 2.7** (bundled with Ghidra) for analysis scripts. Scripts must be **ASCII-only** — Jython 2 chokes on Unicode in source unless an encoding declaration is added.
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

If you've never built the wrapper before, first run also handles UID/GID matching — see the comment at the top of `docker-compose.yml` for `.env` overrides.

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

Computed addresses are still missed — `lw r4, 0x18(r3)` where `r3 = 0x80080000 + index*4` can't be statically resolved when `index` is only known at runtime. Functions reading from arrays via runtime-computed indexing won't appear in xref lists; for these, dynamic analysis with watchpoints is the only static-tool-free path.

## Investigation patterns

### "Find what writes / reads a global"

Use `find_writes_to_8007b85c.py` (edit `TARGETS`). If the global is read via `lui+addiu`, also run `find_lui_writers.py`.

### "Find callers of a function"

Use `find_callers.py` (generic) or `dispatcher_callers.py` (built-in targets).

### "Is this function actually called?"

The reference manager is unreliable for indirect calls. Use:
- `find_callers.py` for direct `jal` references.
- `find_addr_data.py` to find the address as data (function-pointer tables, callbacks).

If both return zero hits, the function has no static caller *in the program currently loaded into Ghidra* — that's NOT the same as "dead code in retail". Most game logic lives in RAM-loaded overlays at `0x801C0000+` that aren't part of `SCUS_942.54`. The negative result bounds where the caller can possibly live, but doesn't prove the function unreachable.

### "Where does this constant address get used?"

If the address is referenced via `lui+addiu`, the reference manager will miss it. Use `find_lui_writers.py` with `LO`/`HI` narrowed to your target range.

### "What format does this PROT entry use?"

Empirical workflow:
1. `xxd extracted/PROT/<entry>.BIN | head -5` — eyeball the header.
2. Try each known parser:
   - `asset stream <file>` — DATA_FIELD streaming.
   - `asset describe <file>` — descriptor format (when applicable).
   - `lzs-decode raw --size N <file>` — top-level LZS.
   - `asset categorize <DIR>` — runs every detector and emits a per-class breakdown.
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

The Ghidra-side scripts (Jython, run inside the container) live in `ghidra/scripts/`:

| Script | Purpose |
|---|---|
| `dump_funcs.py` | Dumps disassembly + decompiled C for a hardcoded list of function entry points (SCUS targets). |
| `dump_top_missing.py` | Dumps the next batch of high-citation missing helpers (SCUS range). Refresh `TARGETS` from `scripts/function-coverage.py --json`. |
| `dump_top_overlay_missing.py` / `dump_round6_overlay_missing.py` | Same as above for overlay programs. |
| `dump_battle_overlay_funcs.py` / `create_and_dump_battle_funcs.py` | Battle-overlay variants; the `create_and_dump` form force-creates functions Ghidra didn't auto-detect (JALR-only entry points). |
| `find_lui_writers.py` | Big LUI+ADDIU resolver. Critical for finding references the ref manager misses. |
| `find_addr_data.py` | Searches for a 4-byte LE word matching a target address — catches function-pointer tables. |
| `find_callers.py` / `find_callers_of.py` | Generic xref tools. |
| `find_writes_to_8007b85c.py` / `find_dat_80084540_writers.py` | Targeted writers/readers of specific globals. |
| `find_debug_flag_writers.py` | Two-pass scan for writers in the documented debug-flag RAM band. |
| `find_sound_path_builders.py` | Walks LUI+ADDIU pairs landing in the sound-driver string cluster `0x8007B380..0x8007B3D0`. |
| `find_move_table_consumers.py` | Two-pass scan for readers of the MOVE / MOVE2 buffers. |
| `find_effect_bundle_consumers.py` | Run on a battle-overlay save state to surface the effect-bundle subsystem (init / spawn / walker). |
| `find_field_program_xrefs.py` / `find_game_mode_dispatcher.py` / `find_game_mode_writers.py` / `find_gp_init_and_mode_table.py` / `dump_mode_table.py` / `dump_mode_names_and_handlers.py` | Game-mode state-machine recon family — locate the 28-mode table at `0x8007078C` and trace handler functions. |
| `inventory_overlay.py` | Per-program function inventory dumper. Emits `inventory_<programname>.csv` with one row per function (entry / size / outgoing / incoming / top callees). |
| `find_tmd_renderer.py` / `find_tmd_table_readers.py` / `find_gte_users.py` | TMD renderer recon — locate readers of the TMD pointer table and count GTE ops per function. |
| `dump_data_region.py` | Dumps arbitrary byte ranges as hex + u32 LE. Useful for extracting in-binary tables once their address is known. |
| `dump_field_vm_dispatchers.py` | One-shot dumper for the field-VM 0x50/0x60/0x70 default-route trio + the generic ramp scheduler. |
| `import_overlay.sh` | Bash wrapper that imports + analyzes a captured overlay dump as Raw Binary at base `0x801C0000`. |

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
