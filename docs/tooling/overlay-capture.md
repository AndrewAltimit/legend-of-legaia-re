# Overlay capture

Most of Legaia's gameplay code doesn't live in `SCUS_942.54` — it lives in RAM-loaded overlays at `0x801C0000+` that the runtime pages in per-mode (title, town/field, battle, options menu, world map, cutscene). Capturing these requires dumping live RAM from a running emulator.

PCSX-Redux is the recommended emulator (open-source, built-in debugger, Lua scripting). Mednafen is also supported via the gzipped save-state extraction path; both give equivalent overlay dumps.

## What's in the overlay window

The overlay window spans `0x801C0000-0x80200000` (256 KB). Several overlays share this window — only one is loaded at any moment.

The base address depends on which overlay; battle and town both load at `0x801CE818`, but smaller overlays like fishing load at different offsets. The `find-overlay` heuristic ranks PROT entries by likelihood-of-being-overlay-code via `addiu sp, sp, -X` prologue density (see [`mips_overlay`](../formats/mips-overlay.md) and [`overlay_ptr_table`](../formats/overlay-ptr-table.md)).

### Capture status

The dump count column reflects committed function dumps under [`ghidra/scripts/funcs/`](../../ghidra/scripts/funcs/) at the time of writing — see `overlay_<label>_<addr>.txt` per overlay.

| Overlay | Captured? | Named program | Subsystems |
|---|---|---|---|
| Title screen | ✓ (loaded at boot, in SCUS-range) | — (in SCUS address range) | Actor / sprite VM (`FUN_801D6628`) |
| Town / field / dialog / inventory (`0897`) | ✓ | `overlay_dialog_mc4.bin` (= walk) / `overlay_dialog_typing.bin` | Field/event VM (`FUN_801DE840`), MES renderer (`FUN_801ED710`), inventory hub (`FUN_801F5748`), MAIN INIT (`FUN_801D6704`); top-20 dumped per program |
| Field overlay — battle-start transition | ✓ | `overlay_field_battle_intro.bin` | Partial 0897 image captured mid-camera-spin; 29 functions dumped including 13 unique to this capture (`FUN_801D081C`, `FUN_801D0370`, `FUN_801CFDA0`, `FUN_801D11D0`, and 9 more) |
| Battle / battle-action (`0898`) | ✓ | `overlay_battle_action.bin` / `overlay_magic_capture.bin` | Per-actor state machine (`FUN_801E295C`), battle main dispatcher (`FUN_801D0748`), effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`); all 78 functions dumped |
| Options / config / all pause-menus (`0896`) | ✓ | `overlay_menu.bin` | Items / magic / equipment / status / options UI; equipment stat aggregator (`FUN_801CF650`); all 129 functions dumped |
| Save / load screen | ✓ | `overlay_save_ui_select.bin` / `overlay_save_ui_saving.bin` | Save-screen SM (`FUN_801DC6B4`); 33 sub-state handlers at `PTR_FUN_801E4F40` dumped; top-20 per program dumped; select and saving layouts are identical |
| Shop / merchant | ✓ | `overlay_shop_save.bin` | Item buy / sell, gold ledger; 130 functions dumped |
| Level-up (`0891`) | ✓ | `overlay_magic_level_up.bin` / `overlay_magic_level_up_full.bin` | XP / stat gain UI; 78 functions dumped; full 256 KB re-capture for data section analysis |
| World map | ✓ | `overlay_world_map.bin` / `overlay_world_map_top.bin` / `overlay_world_map_walk.bin` | World map controller (`FUN_801E76D4`), dev menu renderer (`FUN_801EAD98`); top-20 dumped per program; `world_map_top` lacks `FUN_801DE840` and `FUN_801EAD98` (top-view capture, no movement) |
| Cutscene / dialogue | ✓ | `overlay_cutscene_dialogue.bin` / `overlay_cutscene_mapview.bin` | XA driver + cutscene mode table; 128 functions each |
| Fishing / dev menu (`0971`) | ✓ partial | `overlay_0971_xxx_dat.bin` | Fishing minigame + dev/test menu strings |
| Dance minigame / field reuse (`0978`) | ✓ partial | `overlay_0978_other_game.bin` | Disco King + field-loader stubs |
| Mini-games (Card Battle, Inova game) | ✗ | — | Per-game UI + scoring |

### Overlays still to capture

1. **Mini-games** — Card Battle (Baka Game), Inova card-sort minigame, and the fishing minigame `0971` extended code. Each loads its own overlay slot; none has been fully dumped.

### Level-up overlay data section (resolved)

The mc3 save state was re-extracted at the full 256 KB window
(`0x801C0000–0x801FFFFF`) and imported as `overlay_magic_level_up_full.bin`.
The data section (`ghidra/scripts/dump_levelup_data_section.py`) was dumped
in ten 4 KB blocks. Key findings:

| Address | Content |
|---|---|
| `0x801F4B8C` | 4-byte display row-ID array for magic slots |
| `0x801F4B98` | Magic-type name strings (Spirit / Defense / Meta / Terra / Ozma) |
| `0x801F4C28+` | Battle-result text strings (win / wipe / escape / …) |
| `0x801F5CF8`, `0x801F5D90` | Binary animation tables passed to particle spawner |
| `0x801F6000+` | Live animation state globals (zero at rest) |

Per-character HP/MP/STR/DEF growth does not come from a static table in the
overlay. Stat increments are sourced from per-Seru structs loaded from PROT
entries at runtime (HP grant at Seru `+0x74`). See
[`subsystems/level-up.md`](../subsystems/level-up.md#stat-gains).

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
