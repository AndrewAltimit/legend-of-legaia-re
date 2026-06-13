# Overlay capture

Most of Legaia's gameplay code doesn't live in `SCUS_942.54` - it lives in RAM-loaded overlays at `0x801C0000+` that the runtime pages in per-mode (title, town/field, battle, options menu, world map, cutscene). Capturing these requires dumping live RAM from a running emulator.

PCSX-Redux is the recommended emulator (open-source, built-in debugger, Lua scripting). Mednafen is also supported via the gzipped save-state extraction path. Duckstation `.sav` save states are supported via `scripts/ghidra-analysis/extract-duckstation-overlay.py` (zstd-compressed; same anchor-string approach, 256 KB slice). All three give equivalent overlay dumps.

## What's in the overlay window

The overlay window spans `0x801C0000-0x80200000` (256 KB). Several overlays share this window - only one is loaded at any moment.

The base address depends on which overlay; battle and town both load at `0x801CE818`, but smaller overlays like fishing load at different offsets. The `find-overlay` heuristic ranks PROT entries by likelihood-of-being-overlay-code via `addiu sp, sp, -X` prologue density (see [`mips_overlay`](../formats/mips-overlay.md) and [`overlay_ptr_table`](../formats/overlay-ptr-table.md)).

### Capture status

The dump count column reflects committed function dumps under [`ghidra/scripts/funcs/`](../../ghidra/scripts/funcs/) at the time of writing - see `overlay_<label>_<addr>.txt` per overlay.

| Overlay | Captured? | Named program | Subsystems |
|---|---|---|---|
| Title screen | ✓ | `overlay_title.bin` | Actor / sprite VM (`FUN_801D6628`); title-overlay tick `FUN_801DD35C` (pinned via watchpoint on the title-attract countdown at `0x801EF16C` &mdash; decrement instruction at `0x801DDCCC`, see [`subsystems/boot.md` § Tick function](../subsystems/boot.md#tick-function)). Captured live via [`scripts/pcsx-redux/autorun_countdown_trigger.lua`](../../scripts/pcsx-redux/autorun_countdown_trigger.lua) against a save state at the title screen; sidecar `.screen` blob is a PNG-decodable framebuffer of the live title (`scripts/pcsx-redux/decode_pcsx_screen.py`). |
| Town / field / dialog / inventory (`0897`) | ✓ | `overlay_dialog_mc4.bin` (= walk) / `overlay_dialog_typing.bin` | Field/event VM (`FUN_801DE840`), MES renderer (`FUN_801ED710`), inventory hub (`FUN_801F5748`), MAIN INIT (`FUN_801D6704`); top-20 dumped per program |
| Field overlay - battle-start transition | ✓ | `overlay_field_battle_intro.bin` | Partial 0897 image captured mid-camera-spin; 29 functions dumped including 13 unique to this capture (`FUN_801D081C`, `FUN_801D0370`, `FUN_801CFDA0`, `FUN_801D11D0`, and 9 more) |
| Battle / battle-action (`0898`) | ✓ | `overlay_battle_action.bin` / `overlay_magic_capture.bin` | Per-actor state machine (`FUN_801E295C`), battle main dispatcher (`FUN_801D0748`), effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`); all 78 functions dumped |
| Options / config / all pause-menus (`0899`) | ✓ | `overlay_menu.bin` | Items / magic / equipment / status / options UI; equipment stat aggregator (`FUN_801CF650` at base+0x0e38); all 129 functions dumped. **Source pinned: PROT 0899 @ base `0x801CE818`** (function-signature byte-search + 101/139 menu-dump function alignment + static base recovery; see [`static-overlay-pipeline.md`](static-overlay-pipeline.md)). VA-alias sibling of the field overlay (PROT 0897) in slot A — both load at `0x801CE818`. The earlier `0896` attribution was wrong (`0896`/`bat_back_dat` is not an overlay that loads here at all — its once-recovered `0x801C5818` base was an over-read artifact; see the static-overlay pipeline's cautionary tale). |
| Save / load screen | ✓ | `overlay_save_ui_select.bin` / `overlay_save_ui_saving.bin` | Save-screen SM (`FUN_801DC6B4`); 33 sub-state handlers at `PTR_FUN_801E4F40` dumped; top-20 per program dumped; select and saving layouts are identical. **Source: the save UI lives IN the menu overlay PROT 0899** (`FUN_801DC6B4` at base+0xDE9C; signature byte-matches only 0899 via `asset overlay find-sig`), not a separate entry. |
| Shop / merchant | ✓ | `overlay_shop_save.bin` | Item buy / sell, gold ledger; 130 functions dumped. **Source: also IN the menu overlay PROT 0899** (shares `FUN_801CF650`), not a separate entry — `overlay_shop_save.bin` is a menu-overlay capture taken during a shop session. |
| Level-up (`0891`) | ✓ | `overlay_magic_level_up.bin` / `overlay_magic_level_up_full.bin` | XP / stat gain UI; 78 functions dumped; full 256 KB re-capture for data section analysis |
| World map | ✓ | `overlay_world_map.bin` / `overlay_world_map_top.bin` / `overlay_world_map_walk.bin` | World map controller (`FUN_801E76D4`), dev menu renderer (`FUN_801EAD98`); top-20 dumped per program; `world_map_top` lacks `FUN_801DE840` and `FUN_801EAD98` (top-view capture, no movement). **Source: the overworld controller lives IN the field overlay PROT 0897** (`FUN_801E76D4` at base+0x18EBC; signature byte-matches 0897 via `asset overlay find-sig`) — these captures are the field overlay resident during overworld, not a separate "world-map overlay". |
| Cutscene / dialogue | ✓ | `overlay_cutscene_dialogue.bin` / `overlay_cutscene_mapview.bin` | These two are the FIELD overlay resident during actor-scripted dialogue (op*/ed* labels), NOT the FMV decoder. The actual STR/MDEC FMV-decoder overlay is **PROT 0970** (`cutscene_str`, modes 26/27; pinned statically from the disc — see [`static-overlay-pipeline.md`](static-overlay-pipeline.md)). |
| Minigame hub - fishing, slot, Baka Fighter, dance, debug menu | ✓ | `overlay_fishing.bin` / `overlay_slot_machine.bin` / `overlay_baka_fighter.bin` / `overlay_dance.bin` / `overlay_debug_menu.bin` | Five DISTINCT slot-A overlays that VA-alias the same window, not one binary. **Static sources: fishing 0972, slot machine 0975, baka fighter 0976, dance 0980** — mode-24 door-warp sub-ids 0/3/4/6 (op `0x3E`; each anchored by a documented function prologue; the old 0973 slot attribution was its image in 0973's over-read tail). See [`script-vm.md § 0x3E WARP`](../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp). → [detail](#minigame-hub-overlay-controllers) |
| Muscle Dome / Baka card battle | ✓ | `overlay_muscle_dome.bin` | Distinct from the minigame-hub family; per-frame match controller `FUN_801D0748`. → [detail](#muscle-dome-overlay-controllers) |

#### Minigame-hub overlay controllers

All five hub minigames are variants of the same overlay binary (101–154 shared prologues), but they **VA-alias** — they are distinct files sharing a library core, so a given address hosts a *different* function per minigame; always read the overlay-qualified dump. `overlay_debug_menu.bin` is the superset (189 functions). Per-frame controllers (each a switch-on-state-byte SM, documented in the per-minigame pages under [`subsystems/`](../subsystems/)):

- Fishing `FUN_801CF3BC` (`DAT_801d926c` SM)
- Slot machine `FUN_801CF0D8`
- Baka Fighter `FUN_801D3468`
- Dance `FUN_801CF470` (`DAT_801d5334` SM)

The previously-listed per-minigame "main entry" addresses (`801D63B0` / `801D2CC0` / `801D5ED0` / `801D2F38`) are the shared **textured-quad sprite/HUD emitter** the minigame reuses for every draw — their high caller counts reflect that, not control flow. All functions dumped.

#### Muscle Dome overlay controllers

Completely distinct from the minigame-hub family (only 17 shared prologues). The per-frame match controller is `FUN_801D0748` (pad read, phase dispatch on `ctx+6`, pick/commit/resolve/score loop); `FUN_801D5854` is the camera/view director, `FUN_801D8DE8` the HUD/element renderer, `FUN_801D388C` the card/presentation driver (4-slot deal → commit into the actor `+0x1df` action queue under a point budget). 148 functions dumped. See [`subsystems/minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md).

### Level-up overlay data section (resolved)

A level-up save state was re-extracted at the full 256 KB window
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

Per-character growth does not come from a table in this *display* overlay — it
is in static `SCUS_942.54` (`DAT_800769CC` curves + `DAT_80076918` param block),
applied by the victory-path level-up function `FUN_801E9504`. The writer-search
here came up empty because it scanned the `magic_level_up` overlay, not that
applier. The earlier "HP grant at Seru `+0x74`" reading is **falsified** — those
`+0x74` reads surface a `0x80808080` battle-state flag, not a stat grant. See
[`subsystems/level-up.md`](../subsystems/level-up.md#stat-gains).

## Capturing with PCSX-Redux

1. Boot PCSX-Redux with the disc image; run the game to the scene whose overlay you want to capture.
2. `File → Show Lua Console`.
3. Run `ghidra/scripts/dump_overlay.lua` from the Lua console - it writes `0x801C0000-0x801EFFFF` to `/tmp/legaia_overlay_<TIMESTAMP>.bin`.

> The 192 KB window in `dump_overlay.lua` is too narrow for some battle-effect handlers and for the world-map overlay's high-mode prim renderers at `0x801F7644..0x801F8690` (consumed by `FUN_80043390`'s overlay-mode dispatch table at `0x801F8968`). Use `extract-mednafen-overlay.py` (default window is now `0x801C0000-0x801F9000`, 228 KB) - or pass `--end 0x80200000` for the full 256 KB.

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

## Caveat: overlay-buffer captures are mixed content

A captured 256 KiB slice of an overlay buffer at `0x801C0000` is **not equivalent to a single overlay file on disc**. The buffer holds interleaved data from multiple sources at any given moment:

- Old overlay code/data from previous mode (only partially overwritten).
- Streaming buffers that share address space (e.g. SEQ data from the BGM streamer).
- Multi-pak loads where different ranges of the buffer come from different PROT entries.
- Runtime-initialised BSS/state that has no on-disc counterpart.

Concrete example: at title-screen showing, `captures/boot_walk/overlay_title.bin` contains PROT 1053 (`music_01`) SEQ data at `0x00000..~0x03000`, PROT 0899 options-menu data at `0x0E818..0x15818`, PROT 0897 trailing shared menu helpers at `0x0EFE8..0x10818`, and the title-overlay code proper at `0x0F000..0x25000`. Byte-search for a fingerprint from one region will only find the corresponding source PROT entry, not a single "title overlay" file. To pin a region's source: identify the region boundaries first (look for transitions in content type), pick a fingerprint UNIQUE to that region, then sweep PROT.

## Extracting TIMs from a RAM snapshot

A captured RAM dump often contains transient TIMs that the game staged in main RAM before uploading to VRAM. These can be identified, decoded, and **traced back to their source PROT entry** even when the source is uncompressed - the on-disc CLUT + pixel data is byte-identical to the staged copy; only the RECT fields (VRAM target coords) get rewritten at runtime.

### Methodology

1. Sweep the RAM dump for byte sequence `10 00 00 00` followed by valid PSX TIM flags:
   ```python
   import struct
   for off in range(0, len(data) - 32, 4):
       if struct.unpack_from('<I', data, off)[0] != 0x10:
           continue
       flags = struct.unpack_from('<I', data, off+4)[0]
       mode = flags & 7
       if mode > 3: continue
       # validate CLUT block size + RECT, then pixel block size + RECT
       # (within sane bounds: w/h <= 1024 / 512, sizes in plausible ranges)
   ```
2. Extract each hit as a `.tim` file and decode with `legaia-tim convert` to PNG to identify visually.
3. Build a 16-byte fingerprint from the first CLUT row at offset `0x14` inside the TIM file (skip the RECT bytes at `0x0C..0x14` because those get runtime-relocated).
4. Grep the PROT corpus (`extracted/PROT/*.BIN`) for that fingerprint. Each hit identifies the source entry; byte-compare CLUT + pixel data to confirm.

### Worked example

- The captured `captures/boot_walk/snap_vsync_0300.bin` (full 2 MiB main RAM, taken during the publisher-logo phase) contains four TIMs at `0x801D09DC`, `0x801DBBFC`, `0x801E761C`, `0x801EB65C`.
- Visual decode: PROKION, Contrail "A Contrail Production", "Sony Computer Entertainment America Presents", and the WARNING screen.
- All four CLUT fingerprints match `0895_bat_back_dat.BIN` at well-separated offsets - PROT 0895 is the boot `init.pak` bundle (the `bat_back_dat` label is inherited from the CDNAME define at 895 — the define numbers live in raw-TOC space, where 895/896 are the `summon.dat`/`readef.DAT` battle-backdrop files at extraction 893/894 and extraction 0895 is the first `xxx_dat` slot; see [`formats/summon-readef.md`](../formats/summon-readef.md) and [`cdname.md` § numbering space](../formats/cdname.md#numbering-space)). Documented in [`subsystems/boot.md` § Boot init.pak](../subsystems/boot.md#boot-initpak-prot-0895).

The same method should work for any other transient TIM (battle backgrounds, menu chrome, world map terrain textures) provided the source PROT entry stores the TIM uncompressed. LZS-compressed sources won't match by direct byte search - either decompress them first or use a different signature (e.g., the rendered pixel histogram or framebuffer-area VRAM coords).

## One-command capture (mednafen + Duckstation)

For new captures, the highest-leverage entry point is
[`scripts/ghidra-analysis/auto-name-overlay.py`](../../scripts/ghidra-analysis/auto-name-overlay.py).
It detects the save-state format from the magic bytes (mednafen
gzip+`MDFNSVST` or Duckstation `DUCCS`+zstd), extracts the overlay
window, fingerprints which overlay is loaded by counting matches
against an anchor-function table (curated from the capture-status
table below), and emits both the binary slice and a stub
`dump_<label>_overlay.py` Ghidra script with the top-N largest
function entry-points pre-seeded.

```bash
scripts/ghidra-analysis/auto-name-overlay.py "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0"
# [info] format: mednafen; sliced 262,144 bytes
# [info] auto-detected label: world_map  (world_map=4, field=3)
# [ok]   /tmp/overlay_world_map.bin
# [ok]   ghidra/scripts/dump_world_map_overlay.py
```

When the auto-detection picks the wrong label (the anchor table is
incomplete for some scenes &mdash; shop, cutscene, level-up subset all
currently miss because no documented function is exclusive to them),
pass `--label name` to override:

```bash
scripts/ghidra-analysis/auto-name-overlay.py SAVE.mc0 --label cutscene_dialogue
```

The stub is preserved if it already exists (pass `--force` to
overwrite). After running, follow with the existing Ghidra import:

```bash
scripts/ghidra-analysis/import-overlay-named.sh /tmp/overlay_<label>.bin <label>
docker compose exec ghidra /ghidra/support/analyzeHeadless /projects legaia \
    -process overlay_<label>.bin -noanalysis \
    -postScript /scripts/dump_<label>_overlay.py
```

Cuts the per-scene reverse cycle from manually identifying which
overlay is loaded + hand-rolling a TARGETS list to a single command +
a Ghidra import.

To grow the anchor table: when you confirm a function is exclusive to
a specific overlay (via the dump-script comments + cross-overlay
inventory diffs), add it to `ANCHOR_FUNCTIONS` in
[`scripts/ghidra-analysis/auto-name-overlay.py`](../../scripts/ghidra-analysis/auto-name-overlay.py).

## Mednafen pipeline with asset-loader CSV

> For the broader save-state automation toolkit - diffing two saves to
> see what was written between them, bisecting a sequence of saves to
> find a transition, and the declarative scenario manifest that names
> each `mc{0..9}` slot - see
> [`mednafen-automation.md`](mednafen-automation.md).

The `scripts/ghidra-analysis/analyze-overlay.sh` helper is the older flow. Use it when
you specifically need the asset-loader CSV (which PROT entries the
runtime loader requested for that scene); for plain "capture overlay
+ stub dump" use the auto-name helper above.

```bash
scripts/ghidra-analysis/analyze-overlay.sh \
    "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" \
    --label level_up
```

What it does:
1. Decompresses the gzipped mednafen save state and slices `0x801C0000-0x801F9000` to `/tmp/legaia_overlay_<label>.bin` (default; covers the world-map overlay's full extent).
2. Re-imports as `overlay.bin` in the Ghidra project (overwrites the previous import - keep separate labels per scene).
3. Runs `find_overlay_asset_loads.py` to scan every `jal` to a known SCUS asset loader (`FUN_8003E8A8`, `FUN_8003EB98`, `FUN_8003E6BC`, `FUN_800520F0`, `FUN_8001F7C0`, `FUN_8001E890`, `FUN_8001ED60`) and const-tracks the `$a0` argument.
4. Writes a CSV to `/tmp/overlay_loads_<label>.csv` and prints a summary.

The CSV gives the *exact* PROT entries the runtime loader requests for that scene - replaces the iterative `--vram-extra-dir` guesswork in the asset viewer.

## Capturing with Duckstation

Duckstation `.sav` save-state files use `DUCCS` magic followed by a zstd-compressed binary stream. The `scripts/ghidra-analysis/extract-duckstation-overlay.py` script decompresses the stream with the system `zstd` binary and locates main RAM using the same anchor-string approach as `extract-mednafen-overlay.py`. The default slice is `0x801C0000–0x80200000` (256 KB).

```bash
scripts/ghidra-analysis/extract-duckstation-overlay.py SCUS-94254_1.sav --out /tmp/legaia_overlay_fishing.bin
scripts/ghidra-analysis/import-overlay-named.sh /tmp/legaia_overlay_fishing.bin fishing
```

The `import-overlay-named.sh` step imports as `overlay_fishing.bin` in the Ghidra project (base `0x801C0000`, MIPS LE) and runs auto-analysis. Run `inventory_overlay.py` afterwards to get the function list, then write a `dump_<label>_overlay.py` for the functions of interest.

### Minigame hub overlay (six variants from Duckstation saves)

Seven Duckstation saves cover the minigame overlays. Saves 1–4 and 6 are all variants of the same overlay binary:

| Save | Scene | Label | Unique prologues |
|---|---|---|---|
| 1 | Fishing minigame | `overlay_fishing.bin` | 2 (vs debug_menu) |
| 2 | Slot machine (Wild Card) | `overlay_slot_machine.bin` | 17 (vs fishing) |
| 3 | Baka Fighter (fist fight) | `overlay_baka_fighter.bin` | 34 (vs fishing) |
| 4 | Disco King (dance) | `overlay_dance.bin` | 32 (vs fishing) |
| 5 | Muscle Dome / Baka card battle | `overlay_muscle_dome.bin` | distinct family |
| 6 | Dev/debug menu | `overlay_debug_menu.bin` | 12 (superset of fishing) |
| 7 | Baka card battle (alt state) | - | same code as save 5 |

Saves 5 and 7 share identical code at the first prologue positions (100% match on first 32 KB of code); save 7 is not imported separately.

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
4. Run `scripts/ghidra-analysis/analyze-overlay.sh "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" --label dialog`.
5. Run `scripts/ghidra-analysis/import-overlay-named.sh dialog` so the overlay imports as
   a named program (preserved across re-imports of other overlays).

What to look for after import:
- Strings near the overlay base - Japanese / English glyph table headers.
- Functions that take a `MES container ptr + msg_id + (x, y)` shape - likely a
  message-box renderer feeding the dialog pager `FUN_801D84D0`. (Field NPC
  dialogue itself has no opener function: it's the actor's inline MES walked by
  `FUN_80039b7c` — see [`subsystems/script-vm.md` § Field dialogue](../subsystems/script-vm.md#field-dialogue-has-no-opcode). `FUN_8001FD44` is the scene-change packet, not a dialog opener.)
- `LoadImage`-shaped writes to VRAM via `_DAT_8007AF40`-region SPU/GPU regs
  - that's the per-page glyph upload.

This unblocks the dialog-rendering side of the engine. Once captured, the
crate `legaia-mes` already has the bytecode walker; the renderer-side
quads can land in `crates/engine-render` against the extracted font atlas.

### Cutscene

Cutscenes use XA-streamed audio + a per-cutscene mode driver in an overlay
distinct from town/battle. The XA demuxer is in `crates/xa`; the
game-mode driver is the STR mode-26/27 dispatcher described in
[`cutscene.md`](../subsystems/cutscene.md). The missing piece is the
cutscene overlay's outer state machine that picks XA tracks + scene
transitions.

1. Load a save just before a known cutscene trigger (post-boss,
   chapter-end, etc.).
2. **Once the cutscene starts playing** (XA audio audible, fullscreen
   playback), save state. The first 1-2 seconds work - the overlay is
   resident as long as the cutscene is active.
3. Run `scripts/ghidra-analysis/analyze-overlay.sh "$HOME/.mednafen/mcs/Legend of Legaia (USA).<HASH>.mc0" --label cutscene`.
4. Run `scripts/ghidra-analysis/import-overlay-named.sh cutscene`.

What to look for after import:
- `jal` to `_DAT_8007AF40`-region SPU regs at the XA-DMA destination
  (mirror of the SPU port in `engine-audio`).
- A 28-mode-style table indexed by cutscene ID - the cutscene equivalent
  of the global game-mode table at `0x8007078C`.
- Strings with cutscene-specific filenames (`opening.xa`, `ending.xa`,
  per-chapter labels).

Once captured, the engine-side cutscene driver in `engine-core` can
upgrade from "stub" to "drives the XA stream against the captured
mode table."

## Bulk import of static overlay candidates

The `find-overlay` heuristic surfaces PROT entries that look like overlay code (high `addiu sp, sp, -X` density). To bulk-import the top candidates:

```bash
scripts/ghidra-analysis/bulk-import-overlays.sh --score 3.5
```

Reads the `find-overlay` output, filters by score, imports each at base `0x801C0000` (the overlay window) and runs auto-analysis + the inventory dumper. Per-overlay function inventories land in `ghidra/scripts/inventory_overlay_<stem>.bin.csv`.

The bulk-imported overlays still need a subsystem-naming pass (correlating strings + dispatcher shapes against the inventories) - bulk import only gives you the function lists.

## See also

- [`docs/tooling/static-overlay-pipeline.md`](static-overlay-pipeline.md) — the **static** complement: extract each clean-copy overlay from the disc at its recovered base, with identity attached from the PROT entry (solves the VA-aliasing identity problem structurally). This page (dynamic capture) stays authoritative for runtime values.
- [`docs/tooling/mednafen-automation.md`](mednafen-automation.md) — the save-state diff / bisect toolkit these slices come from.
- [`docs/reference/functions.md`](../reference/functions.md) — overlay-resident entry points the captured slices expose.
- [`docs/reference/memory-map.md`](../reference/memory-map.md) — the `0x801C0000+` overlay window addresses.
