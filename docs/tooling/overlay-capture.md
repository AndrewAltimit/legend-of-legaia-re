# Overlay capture

Most of Legaia's gameplay code doesn't live in `SCUS_942.54` — it lives in RAM-loaded overlays at `0x801C0000+` that the runtime pages in per-mode (title, town/field, battle, options menu, world map, cutscene). Capturing these requires dumping live RAM from a running emulator.

PCSX-Redux is the recommended emulator (open-source, built-in debugger, Lua scripting). Mednafen is also supported via the gzipped save-state extraction path; both give equivalent overlay dumps.

## What's in the overlay window

The overlay window spans `0x801C0000-0x80200000` (256 KB). Several overlays share this window — only one is loaded at any moment.

The base address depends on which overlay; battle and town both load at `0x801CE818`, but smaller overlays like fishing load at different offsets. The `find-overlay` heuristic ranks PROT entries by likelihood-of-being-overlay-code via `addiu sp, sp, -X` prologue density (see [`mips_overlay`](../formats/mips-overlay.md) and [`overlay_ptr_table`](../formats/overlay-ptr-table.md)).

### Capture status

The dump count column reflects committed function dumps under [`ghidra/scripts/funcs/`](../../ghidra/scripts/funcs/) at the time of writing — see `overlay_<label>_<addr>.txt` per overlay.

| Overlay | Captured? | Where it lives | Subsystems |
|---|---|---|---|
| Title screen | ✓ (loaded at boot, in SCUS-range) | `0x801C0000+` | Actor / sprite VM (`FUN_801D6628`) |
| Town / field / dialog / inventory (`0897`) | ✓ | PROT entry `0897_xxx_dat`, RAM base `0x801CE818` | Field/event VM (`FUN_801DE840`), MES renderer (`FUN_801ED710`), inventory hub (`FUN_801F5748`), MAIN INIT (`FUN_801D6704`) |
| Battle | ✓ | PROT entry `0898_xxx_dat`, RAM base `0x801CE818` | Per-actor state machine (`FUN_801E295C`), battle main dispatcher (`FUN_801D0748`), effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`) |
| Battle action | ✓ (separate capture) | re-uses battle window | Action SM full coverage including `FUN_801E295C` outer dispatch + sub-states |
| Options / config menu (`0896`) | ✓ | PROT entry `0896` (= 0897 + 36 KB prefix) | In-game options UI |
| Fishing / dev menu (`0971`) | ✓ partial (top-6 dumped) | PROT entry `0971_xxx_dat` | Fishing minigame + dev/test menu strings |
| Dance minigame / field reuse (`0978`) | ✓ partial (top-8 dumped) | PROT entry `0978_other_game` | Disco King + field-loader stubs |
| Status / inventory menu | ✓ | overlay re-uses `0897` window; mc5 capture | Per-character status panel, equipment swap, item-use validator gate (cf. `crate::engine_vm::action_validator`) |
| Dialog (proportional font + MES renderer) | ✓ partial (21 dumps) | overlay loaded only while text is on screen | MES bytecode interpreter, glyph-bitmap upload |
| Level-up (PROT 891) | ✗ — overlay window not yet captured | post-battle screen | XP / stat gain UI; ramp-to-cap interaction with `action_validator` arm 6 |
| Shop / merchant | ✗ | town overlay variant — re-loads after `FUN_8003F2B8` clears | Item buy / sell, gold ledger |
| World map | ✗ | overlay loaded on world-map entry | Per-tile destination lookup, party-trail animation |
| Save / load screen | ✗ | overlay loaded on memory-card menu | PSX memory-card I/O wrapper, slot UI |
| Cutscene | ✗ | overlay loaded once XA stream starts | XA driver + per-cutscene mode table |
| Mini-games (Card Battle, Inova game) | ✗ | each loads its own overlay slot | Per-game UI + scoring |

### Overlays still to capture (priority for engine completeness)

1. **World map** — gates field-mode entry from the global travel map. Without this, the engine can boot a single field scene but not navigate between them. Capture procedure: load any post-prologue save, walk to the world map, save state, run `scripts/analyze-overlay.sh ... --label world_map`.
2. **Save / load screen** — gates persistence. The `legaia-save` crate parses the on-card record, but the runtime UI that reads + writes to the card is overlay-resident. Capture: open the in-game save menu, save state, label `save_screen`.
3. **Shop** — a frequently-loaded gameplay overlay; needed for any "buy a healing item, then validate via action_validator arm 6" flow. Capture: enter any town's shop, talk to merchant, save state mid-buy, label `shop`.
4. **Cutscene** — XA streaming + scene transitions. The XA demuxer + audio mixer already work; the missing piece is the per-cutscene mode driver. Capture procedure documented under [Cutscene](#cutscene) below.
5. **Level-up** (PROT 891) — currently classified as `MipsOverlay` by the categorizer but not imported. Capture: enter a battle that levels at least one character, save state on the level-up screen, label `level_up`.

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

### Dialog (text-renderer overlay)

The proportional dialog font's glyph bitmaps and the MES bytecode interpreter
both live in an overlay that's only present while a dialog box is open.
The `legaia-mes` parser can already walk MES container bytes; the missing
piece is the renderer's overlay-resident byte→quad pipeline.

1. Load a save where you can talk to an NPC (any town).
2. Initiate dialog (Cross on an NPC).
3. **As soon as the dialog box appears**, save state. (The overlay unloads
   when the box closes; capturing mid-conversation is essential.)
4. Run `scripts/analyze-overlay.sh "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" --label dialog`.
5. Run `scripts/import-overlay-named.sh dialog` so the overlay imports as
   a named program (preserved across re-imports of other overlays).

What to look for after import:
- Strings near the overlay base — Japanese / English glyph table headers.
- Functions that take a `MES container ptr + msg_id + (x, y)` shape — likely
  the dialog opener `FUN_8001FD44`'s overlay callee.
- `LoadImage`-shaped writes to VRAM via `_DAT_8007AF40`-region SPU/GPU regs
  — that's the per-page glyph upload.

This unblocks the dialog-rendering side of the engine. Once captured, the
crate `legaia-mes` already has the bytecode walker; the renderer-side
quads can land in `crates/engine-render` against the extracted font atlas.

### Cutscene

Cutscenes use XA-streamed audio + a per-cutscene mode driver in an overlay
distinct from town/battle. The XA demuxer is in `crates/xa`; the
game-mode driver landed in PR #9. The missing piece is the cutscene
overlay's outer state machine that picks XA tracks + scene transitions.

1. Load a save just before a known cutscene trigger (post-boss,
   chapter-end, etc.).
2. **Once the cutscene starts playing** (XA audio audible, fullscreen
   playback), save state. The first 1-2 seconds work — the overlay is
   resident as long as the cutscene is active.
3. Run `scripts/analyze-overlay.sh "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" --label cutscene`.
4. Run `scripts/import-overlay-named.sh cutscene`.

What to look for after import:
- `jal` to `_DAT_8007AF40`-region SPU regs at the XA-DMA destination
  (mirror of the SPU port in `engine-audio`).
- A 28-mode-style table indexed by cutscene ID — the cutscene equivalent
  of the global game-mode table at `0x8007078C`.
- Strings with cutscene-specific filenames (`opening.xa`, `ending.xa`,
  per-chapter labels).

Once captured, the engine-side cutscene driver in `engine-core` can
upgrade from "stub" to "drives the XA stream against the captured
mode table."

## Bulk import of static overlay candidates

The `find-overlay` heuristic surfaces PROT entries that look like overlay code (high `addiu sp, sp, -X` density). To bulk-import the top candidates:

```bash
scripts/bulk-import-overlays.sh --score 3.5
```

Reads the `find-overlay` output, filters by score, imports each at base `0x801C0000` (the overlay window) and runs auto-analysis + the inventory dumper. Per-overlay function inventories land in `ghidra/scripts/inventory_overlay_<stem>.bin.csv`.

The bulk-imported overlays still need a subsystem-naming pass (correlating strings + dispatcher shapes against the inventories) — bulk import only gives you the function lists.
