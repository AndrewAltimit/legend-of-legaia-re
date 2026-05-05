# Overlay capture

Most of Legaia's gameplay code doesn't live in `SCUS_942.54` — it lives in RAM-loaded overlays at `0x801C0000+` that the runtime pages in per-mode (title, town/field, battle, options menu, world map, cutscene). Capturing these requires dumping live RAM from a running emulator.

PCSX-Redux is the recommended emulator (open-source, built-in debugger, Lua scripting). Mednafen is also supported via the gzipped save-state extraction path; both give equivalent overlay dumps.

## What's in the overlay window

The overlay window spans `0x801C0000-0x80200000` (256 KB). Several overlays share this window — only one is loaded at any moment:

| Overlay | Where it lives | Subsystems |
|---|---|---|
| Title screen | (loaded at boot) | Actor / sprite VM (`FUN_801D6628`) |
| Town / field / dialog / inventory | PROT entry `0897_xxx_dat`, RAM base `0x801CE818` | Field/event VM (`FUN_801DE840`), MES renderer (`FUN_801ED710`), inventory hub (`FUN_801F5748`), MAIN INIT (`FUN_801D6704`) |
| Battle | PROT entry `0898_xxx_dat`, RAM base `0x801CE818` | Per-actor state machine (`FUN_801E295C`), battle main dispatcher (`FUN_801D0748`), effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`) |
| Options / config menu | PROT entry `0896` (which is 0897 + a 36 KB prefix) | In-game options UI |
| Fishing / dev menu | PROT entry `0971_xxx_dat` | Fishing minigame + dev/test menu strings |
| Dance minigame / field reuse | PROT entry `0978_other_game` | Disco King + field-loader stubs |
| Cutscene | not yet captured | likely a separate cutscene-specific overlay |
| World map | not yet captured | |
| Level-up | PROT entry 891 (size suggests yes; not yet imported as code) | post-battle level-up screen |

The base address depends on which overlay; battle and town both load at `0x801CE818`, but smaller overlays like fishing load at different offsets. The `find-overlay` heuristic ranks PROT entries by likelihood-of-being-overlay-code via `addiu sp, sp, -X` prologue density (see [`mips_overlay`](../formats/mips-overlay.md) and [`overlay_ptr_table`](../formats/overlay-ptr-table.md)).

## Capturing with PCSX-Redux

1. Boot PCSX-Redux with the disc image; run the game to the scene whose overlay you want to capture.
2. `File → Show Lua Console`.
3. Run `ghidra/scripts/dump_overlay.lua` from the Lua console — it writes `0x801C0000-0x801EFFFF` to `/tmp/legaia_overlay_<TIMESTAMP>.bin`.

> The 192 KB window in `dump_overlay.lua` is too narrow for some battle-effect handlers (which live in `0x801F0000+`). For the full 256 KB use `extract-mednafen-overlay.py --start 0x801C0000 --end 0x80200000`.

Then load the dump into Ghidra:

```bash
docker compose cp /tmp/legaia_overlay_<TIMESTAMP>.bin ghidra:/data/overlay.bin
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia \
    -import /data/overlay.bin \
    -loader BinaryLoader \
    -loader-baseAddr 0x801C0000 \
    -processor MIPS:LE:32:default \
    -overwrite

docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process overlay.bin
```

## Capturing with mednafen (one-shot pipeline)

The `scripts/analyze-overlay.sh` helper turns a gzipped mednafen save state into a labelled overlay program with an asset-load CSV in one step:

```bash
scripts/analyze-overlay.sh \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" \
    --label level_up
```

What it does:
1. Decompresses the gzipped mednafen save state and slices `0x801C0000-0x801F0000` to `/tmp/legaia_overlay_<label>.bin`.
2. Re-imports as `overlay.bin` in the Ghidra project (overwrites the previous import — keep separate labels per scene).
3. Runs `find_overlay_asset_loads.py` to scan every `jal` to a known SCUS asset loader (`FUN_8003E8A8`, `FUN_8003EB98`, `FUN_8003E6BC`, `FUN_800520F0`, `FUN_8001F7C0`, `FUN_8001E890`, `FUN_8001ED60`) and const-tracks the `$a0` argument.
4. Writes a CSV to `/tmp/overlay_loads_<label>.csv` and prints a summary.

The CSV gives the *exact* PROT entries the runtime loader requests for that scene — replaces the iterative `--vram-extra-dir` guesswork in the asset viewer.

## Capture protocol per overlay

### Town / field

1. Start a new game or load past character creation.
2. Walk into a town map (any town will do).
3. Save state.
4. Run `analyze-overlay.sh ... --label town`.

### Battle

1. Load a save with characters.
2. Engage a battle (random encounter or scripted boss).
3. Save state during the action menu (a clean state, not mid-animation).
4. Run `analyze-overlay.sh ... --label battle`.

### Level-up

1. Load a save with characters that gain XP.
2. Engage a battle and let a character level up.
3. Save state while the level-up screen is displayed (auto-shown post-battle).
4. Run `analyze-overlay.sh ... --label level_up`.

## Bulk import of static overlay candidates

The `find-overlay` heuristic surfaces PROT entries that look like overlay code (high `addiu sp, sp, -X` density). To bulk-import the top candidates:

```bash
scripts/bulk-import-overlays.sh --score 3.5
```

Reads the `find-overlay` output, filters by score, imports each at base `0x801C0000` (the overlay window) and runs auto-analysis + the inventory dumper. Per-overlay function inventories land in `ghidra/scripts/inventory_overlay_<stem>.bin.csv`.

The bulk-imported overlays still need a subsystem-naming pass (correlating strings + dispatcher shapes against the inventories) — bulk import only gives you the function lists.
