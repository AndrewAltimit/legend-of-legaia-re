# Boot path

The boot sequence does three things before anything else: read the PROT.DAT TOC into RAM, populate the asset-type dispatcher, and hand control to the title-screen overlay.

## TOC loader (`FUN_8003E4E8`)

Reads the first three sectors of `PROT.DAT` (= 6 KB) into RAM at `0x801C70F0`. Called from `FUN_8003EFE8` and `FUN_8003F08C` at boot.

The on-disc TOC and the in-RAM TOC have **different strides** - see [`formats/prot.md`](../formats/prot.md). The on-disc-to-in-RAM transformation function hasn't been reversed; it presumably runs once at boot.

After this completes, two resolvers are usable:

- `FUN_8003E8A8` - index-based; consumed directly by the streaming loader and the dev-build sound branch.
- `FUN_8003E6BC` - path-based; resolves dev paths (`data\battle\efect.dat`, `h:\PROT\FIELD\<scene>\…`) into an index via the [CDNAME.TXT name map](../formats/cdname.md), then delegates to the LBA resolver. Most retail-build code paths land here.

## Asset-type dispatcher (`FUN_8001F05C`)

The central per-asset-format dispatcher - every TIM, TMD, MES, ANM, etc. branch is reached through it. Documented at [`formats/asset-type.md`](../formats/asset-type.md). Calling convention: `result = FUN_8001F05C(byte *src_data, u32 type_and_size, int param3, int copy_only)` where `type_and_size` packs the type byte in the high 8 bits and the size in the low 24 bits.

The boot path doesn't call the dispatcher itself; it just makes sure the buffer pointers it writes to are valid. `FUN_80020224` (the asset descriptor walker) is one of the dispatcher's two static call sites and gets called from the town overlay's `FUN_801D6704` (MAIN_INIT) at runtime.

## Game-mode state machine

The mode-dispatch table at `0x8007078C` is **28 entries × 24 bytes = 672 bytes** (already documented in [`reference/functions.md` § Game-mode state machine](../reference/functions.md#game-mode-state-machine)). Each entry layout:

| Offset | Width | Field |
|---|---|---|
| `+0x00` | u32 | Name-string pointer. Even modes (init) point at BSS labels in `0x8007B3DC..0x8007B408` (runtime-initialised). Odd modes (per-frame) point at static dev-mode-name strings in the `0x800109D0..0x80010AD8` pool. |
| `+0x04` | u32 | Reserved / zero. |
| `+0x08` | u16 | Reserved / zero (low half of the next-mode word). |
| `+0x0A` | i16 | Next-mode index: `-1` = self-managed, `0` = return to mode 0 (CONFIG). The word at `+0x08` reads `0xFFFF0000` on self-managed modes - that is the `-1` over a zero low half, not a sentinel constant. Retail uses only those two values. |
| `+0x0C` | u32 | Reserved / zero. |
| `+0x10` | u32 | Handler function pointer (some land in the overlay window `0x801C0000+` when an overlay is resident, e.g. mode 6 TMD-TEST's `0x801CF730`). |
| `+0x14` | u32 | Handler parameter. |

Dev mode-name strings (12-byte stride in the static pool):

| Mode pair | Name | Mode pair | Name |
|---|---|---|---|
| `0/1` | `CONFIG INIT` / `CONFIG MODE` | `14/15` | `MAP TEST` / `MAP MODE` |
| `2/3` | `MAIN INIT` / `MAIN MODE` | `16/17` | `READ INIT` / `READ MODE` |
| `4/5` | `MONSTER TEST` / `MONSTER MODE` | `18/19` | `GAMEOVER INIT` / `GAMEOVER MODE` |
| `6/7` | `TMD TEST` / `TMD MODE` | `20/21` | `BATTLE INIT` / `BATTLE MODE` |
| `8/9` | `EFECT TEST` / `EFECT MODE` | `22/23` | `CARD INIT` / `CARD MODE` |
| `10/11` | `TEST TEST` / `TEST MODE` | `24/25` | `OTHER INIT` / `OTHER MODE` |
| `12/13` | `MAPDISP INIT` / `MAPDISP MODE` | `26/27` | `STR INIT` / `STR MODE` |

Verified handler→PROT mappings (`FUN_8003EBE4` and `FUN_8003EC70` are the two parallel overlay
loaders, destination buffer pointers `*DAT_8001038C` / `*DAT_80010390` respectively; both call
`FUN_8003E8A8(param + 0x381)`). **Index spaces:** the resolver indexes the in-RAM TOC at
`0x801C70F0`, which is raw `PROT.DAT` from byte 0 (byte-verified against the
`door_warp_town01_to_map01` save state), reading `start = toc[idx+2]`; the extraction index
space (`crates/prot`, `extracted/PROT/NNNN_*.BIN`) slices entry `p`'s start from file word
`p+4`. So in extraction index space the loaded entry is **`prot_index = param + 0x37F`** - two
below the raw `+ 0x381`. Every content-anchored overlay confirms this: param 2 → 0897 field, 3 →
0898 battle (RAM-byte-verified), 4 → 0899 menu (RAM-byte-verified), 0x4A → 0969 STR-path table,
0x4B → 0970 cutscene/STR, 0x4C → 0971 debug menu, 0x54 → 0979 (literal `"efect init"` strings),
and the seven mode-24 minigame slots whose init VAs land on prologues (see [script-vm.md § 0x3E
WARP](script-vm.md#0x3e-warp-mode-24-minigame-door-warp)).

The census is exhaustive for static `SCUS_942.54`: a full-image scan for both loaders' `jal`
sites (with the `a0` setup decoded) finds 16 callsites. Constant params: 2 / 3 / 4 / 7 / 0x4B /
0x4C / 0x53 / 0x54 / 0x56 plus the mode-24 `sub_id + 0x4D` band; computed params: the battle
SM's special-attack (`+0x28`) and summon-stager (`id - 0x79`) bands, the battle stage band
(`+0x47`), and the slot-B default `FUN_80025BA0` (param 5 or 6 by flag `DAT_8007B6A8` →
extraction 0900/0901, the summon-render pair - agreeing with 0900's byte-residency in mid-cast
saves). No site can produce param 0 or 1, so extraction entries 0895/0896 are unreachable from
any static loader call (see the 0896 row in
[open-rev-eng-threads.md](../reference/open-rev-eng-threads.md#prot-0896-bat_back_dat-identity)).

| Mode | Init handler | Loader call | PROT idx | Content (verified) |
|---|---|---|---|---|
| 0 `CONFIG INIT` | `FUN_80025C68` | `FUN_8003EBE4(0x4C)` | 971 | **Debug-menu overlay** - "DEBUG MODE" header + FOG / WORK_TBL / SAVE DATA / MAP NAME / TMD NO / POLY / VERT dev-menu strings (the `overlay_debug_menu` capture family). The dev label "CONFIG" is a misnomer. (An earlier `+ 0x381`-arithmetic reading placed this at 973 and took the slot-machine text in 973's over-read tail for its content - 973 itself is the 1-sector `OTHER2` dev module at mode-24 warp sub-id 1; the casino slot machine is 975, sub-id 3.) |
| 2 `MAIN INIT` | `FUN_80025B64` | `FUN_8003EBE4(2)` | 897 | **Field/town gameplay INIT.** Loads the field overlay (PROT 0897, the entry the static overlay map pins at slot-A base `0x801CE818`), then calls the per-scene initializer `FUN_801D6704` (map + MAN + camera + fog + BGM load; game-mode work buffer alloc), which hands off to mode 3 by writing `_DAT_8007B83C = 3`. The title screen's NEW GAME path launches this mode. (The earlier "loads 899, the field/town/menu overlay" reading was the same off-by-2: the "Display Off / Vibration On / Voices On" options strings live in the **menu** overlay 0899, which mode 22 loads.) |
| 8 `EFECT TEST` | `FUN_80025E68` | `FUN_8003EBE4(0x54)` | 979 | Effect-test dev mode - the entry's own strings are literally `"efect init"` / `"efect init end"` / `"battle bgm %d"`. |
| 22 `CARD INIT` | `FUN_8002574C` | `FUN_8003EBE4(4)` | 899 | **Menu / memory-card overlay** (the in-field pause menu runs under this pair; see below). RAM-byte-verified as the menu overlay in the static overlay map. |
| 24 `OTHER INIT` | `FUN_80025980` | `FUN_8003EBE4(sub_id + 0x4D)` (`+2` first when `sub_id >= 6`) | 972..977, 980 | Minigame door-warp entry (field-VM op `0x3E`, `sub_id = op0 - 100`). Backs up the active scene name `0x80084548` → `0x8007BAE8` (+ `_DAT_80084540` → `0x8007BAC4`), streams the per-sub-id minigame overlay into slot A over the field overlay, then calls its init entry; `FUN_80026018` restores both on exit and re-enters mode 2. Sub-id table: [script-vm.md § 0x3E WARP](script-vm.md#0x3e-warp-mode-24-minigame-door-warp). Live-confirmed (Baka Fighter capture, sub-id `0x8007BA34 = 4` → PROT 0976). The "mode-24 loads PROT 0896" association is **refuted**: 0896's bytes appear nowhere in RAM across the entry window nor in any parked library state. |
| 12 `MAPDSIP INIT` | `FUN_80025DA0` | `FUN_8003EBE4(0x56)` | 981 | World-map display mode entry - a transient sub-overlay swap over the field overlay 0897's head, same save/restore pattern as mode 24. `FUN_80025DA0` saves the slot-A head (`*0x8001038C` = `0x801CE818`, `0x4000` bytes), loads PROT 981 over it, and calls its init `0x801CF4AC` (file `+0xC94`, so **base `0x801CE818` is pinned by the call target**); on exit it restores 0897's head and re-enters it. The init seeds the scratchpad display-list base `0x1F800314` from world-state globals; the body is a 21-state display SM reading the co-resident 0897 body (`0x801D5334`, beyond the swap window), so the world-map *controller* proper stays in 0897 (`FUN_801E76D4`). See [`world-map.md`](world-map.md#per-frame-dispatch-scus-resident). |
| 18 `GAME OVER INIT` | `FUN_80025B30` | `FUN_8003EBE4(7)` | 902 | Game-over overlay - the loader census corroborates the static map's content pin (`gameover` row, entry 0902). |
| 26 `STR INIT` | `FUN_80025FB4` | `FUN_8003EBE4(0x4B)` | 970 | Cutscene / STR FMV mode entry (the `cutscene_str` overlay in the static overlay map). Title-overlay tick writes `_DAT_8007B83C = 0x1A` (= 26) on attract underflow → enters this mode. |

##### Full handler map (recovered from the disc)

[`legaia_asset::mode_table`](https://github.com/altimit-mii/legend-of-legaia-re/tree/main/crates/asset/src/mode_table.rs) reads the whole table out of `SCUS_942.54` (`asset mode-table SCUS_942.54`; disc-gated `mode_table_real`). Init handlers (even index) and per-frame handlers (odd index):

| Mode | Name (disc string) | Init handler | Per-frame handler |
|---|---|---|---|
| 0/1 | CONFIG | `0x80025C68` | `0x80025EEC` |
| 2/3 | MAIN (field/town) | `0x80025B64` | `0x80025EEC` |
| 4/5 | MONSTER TEST | `0x8002611C` | `0x80025EEC` |
| 6/7 | TMD TEST | `0x801CF730` (overlay) | `0x80025EEC` |
| 8/9 | EFECT TEST | `0x80025E68` | `0x80025EEC` |
| 10/11 | TEST | `0x8002B97C` | `0x80025EEC` |
| 12/13 | MAPDSIP (world-map display) | `0x80025DA0` | `0x80025F2C` |
| 14/15 | MAP TEST | `0x8002B904` | `0x80025EEC` |
| 16/17 | READ | `0x8002612C` | `0x80025EEC` |
| 18/19 | GAME OVER | `0x80025B30` | `0x80025EEC` |
| 20/21 | BATTLE | `0x800565D8` | `0x80025EEC` |
| 22/23 | CARD (menu / memory card) | `0x8002574C` | `0x80025F74` |
| 24/25 | OTHER | `0x80025980` | `0x80025EEC` |
| 26/27 | STR (FMV) | `0x80025FB4` | `0x80025EEC` |

**Structural fact:** 12 of the 14 per-frame modes share the generic per-frame handler `0x80025EEC`; only Mode 13 (world-map display) and Mode 23 (menu / memory card) carry their own. So the per-frame "MODE" half of the state machine is mostly one shared tick parameterised by `+0x14`, not 14 distinct handlers. (The `0x80025DA0` MAPDSIP-init dev string is misspelled on the disc - "MAPDSIP", not "MAPDISP".)

**The in-field pause menu runs under the CARD pair (mode 23), not field mode 3.** Every menu-open capture in the save library - equipment / status / options, opened from both the field (`map01`) and town (`town01`) - holds `_DAT_8007B83C = 0x17` (23). So the "CARD" dev label covers the whole menu / memory-card overlay surface (the field menu carries the Save flow), which is also why mode 23 is one of the two per-frame modes with its own handler.

The engine mirrors this: `BootSession` hosts the pause-menu session headlessly (`open_field_menu` / the Start-edge path in `BootSession::tick`), holding the world in `SceneMode::Menu` while the menu is open - `engine_core::mode` maps the CARD pair (`CardInit`/`CardMode`) to that scene mode - and the mode-trace oracle (`mode_trace_e3`) drives menu-open scenarios with a scripted Start press and asserts full menu-mode convergence (scene mode + active scene + the `game_mode` byte, engine `0x17` vs the retail snapshot).

**The dev mode-names mislead.** `MAIN INIT`/`MAIN MODE` (modes 2/3) are the **field/town gameplay** init/run pair (`game_mode 0x03` is the on-field / in-town loop), *not* a standalone options screen - the per-scene initializer `FUN_801D6704` they reach is unmistakably the map loader (debug strings `map_name`, `map_read`, `man_set`, `camera_set`, `fog_set`, `tmds: %d`, `game_mode`, `program_mode`; calls the field asset loader `FUN_8001F7C0` and MAN decoder `FUN_8003AEB0`). `CONFIG INIT` doesn't initialise game config; it initialises the dev debug-menu mode (PROT 0971). The engine-core `GameMode` enum in `crates/engine-core/src/mode.rs` shares these dev names; its docstrings now reflect the field-mode semantics.

#### New Game boot chain (title → field)

The title-screen NEW GAME selection is the entry point into modes 2/3:

1. **Title confirm.** In the title overlay tick (`FUN_801DD35C`), the menu handler reads the live cursor (`state[+0x1FC]`), and on `L1|Cross` (`pad & 0x44`) stashes the chosen row at `state[+0x200]`, then advances to sub-mode `0x14`. NEW GAME is row 0; a non-zero row (CONTINUE) routes to the save/card load path instead.
2. **Launch write.** The row-0 sub-phases reach `0x801DFC00`: `li v0,0x2; sh v0,-0x47C4(v1)` writes `_DAT_8007B83C = 2`, resets the title sub-mode, and kicks a fade-out (`FUN_80024EE4(1, 2, 0xFFFFFF)`). (The other master-mode writes in this tick set `0x1A` = STR/intro-FMV, the attract/demo path.)
3. **Mode-2 init.** The mode dispatcher runs `FUN_80025B64`: load the field overlay (`FUN_8003EBE4(2)`) → call `FUN_801D6704`.
4. **Field scene init.** `FUN_801D6704` reads the resident map id, loads geometry + MAN + camera + fog + BGM, allocates the game-mode work buffer, and writes `_DAT_8007B83C = 3` - the field per-frame loop ("MAIN MODE") takes over the next frame.

The mode-transition control flow is mirrored in `crates/engine-vm/src/title_overlay.rs` (`MASTER_GAME_MODE_FIELD_LAUNCH` = 2, `MASTER_GAME_MODE_FIELD_RUN` = 3, `FIELD_SCENE_INIT_PC`, `MENU_INDEX_NEW_GAME`) and `crates/engine-core/src/world.rs` (`World::begin_new_game`). `FUN_801D6704` itself is generic field entry, used for every scene transition, and reads the *fresh-state seed* a new game establishes (starting party stats, gold, starting scene id) from globals rather than seeding it.

The fresh-state seed is the new-game data-init `FUN_80034A6C` (called via the boot mode initializer `FUN_8001DCF8`). It establishes the new-game world state:

- **Gold.** Party gold (`_DAT_8008459C`, the word the battle-victory reward writer `FUN_8004F0E8` credits) is set to a hardcoded **`500`** - a constant in the init routine, not a field of the starting-party template. Mirror: `NEW_GAME_STARTING_GOLD` in `crates/engine-core/src/world.rs`.
- **Story flags.** The routine zeroes a ~`0x200`-byte story-flag region, so a New Game starts with every story flag clear. (`World::begin_new_game` matches this.)
- **Starting party.** `FUN_80034A6C` calls `FUN_800560B4`, which expands a static `SCUS_942.54` template into the live per-character records (stride `0x414`). The template is `[8×u16 stats][10-byte name]` per record (Vahn, Noa, Gala, Terra), parsed by [`legaia_asset::new_game`](../formats/new-game-table.md). Vahn's row (HP 180 / MP 20 / AGL 100 / ATK 24 / uDEF 16 / lDEF 12 / SPD 19 / INT 9) is byte-validated against an early `town01` save state. `FUN_800560B4` copies the template's **default name** (`Vahn`) into the record. This default is what the downstream **name-entry** screen (the *"Select your name."* character grid, save-state `name_input_ui`) pre-fills and lets the player overwrite - that screen fires in the field/event flow after the opening, not here.
  (The front-end launcher's `s_opdeene` write is the opening *scene id*, not a name - see the sub-mode dispatcher section.)
- **Opening scene.** The default map-name buffer holds the literal `"town01"` (Rim Elm) - the interactive scene a New Game enters. `FUN_8001D424` (the global reset/init) leaves the buffer at `town01` and reads an optional dev `initmap.txt` override when the debug flag `_DAT_8007B8C2` is clear. The data seed does not itself set the scene.

`FUN_801D6704` then reads this seeded state from globals during the field scene init; it is generic field entry used for every scene transition, not new-game-specific.

#### Title screen is not in the mode table

The title screen is not one of the 28 modes - its tick (`FUN_801DD35C`) is loaded by a pre-mode-dispatch boot routine, ahead of the mode table being consulted at all. NEW GAME is how control crosses from that title overlay into the mode table (at mode 2). The title overlay code lives in the unindexed 60-sector gap inside `PROT.DAT` between TOC entries 899 and 900 (see [§ Title-overlay source on disc](#title-overlay-source-on-disc) below). The title *wordmark* TIM is PROT 888/890 (read by `legaia_asset::title_pak`); PROT 899 carries the options-menu config bundle. So the recurring "which mode-table row is the title screen?" question has an empty answer - there isn't one.

### CD-read API stack

The SCUS-side CD I/O is layered. Bottom-up:

| Function | Role |
|---|---|
| `FUN_8005D9A0` | CD-DMA-channel-3 synchronous read primitive. Writes CD command registers and triggers DMA. Takes `(dest_buffer, mode)`. The `_DAT_800795B4` pointer table mentioned in some older notes does not exist - `0x8005DA40` is just an instruction inside this function (`lui v1, 0x8008`), promoted to a fake `FUN_xxxxx` label by Ghidra. |
| `FUN_8005C2C4` | 1-line wrapper around `FUN_8005D9A0` returning `iVar1 == 0`. |
| `FUN_8005C42C` | BCD-MSF → LBA conversion: `(minBCD * 60 + secBCD) * 75 + frameBCD - 150`. Standard PSX MSF math. |
| `FUN_8005C328` | LBA → BCD-MSF conversion (inverse of `FUN_8005C42C`). |
| `FUN_8005DBB4` | ISO9660 directory lookup: `(file_info_out, filename)` → fills `file_info_out` with `{msf[3], size, ...}`. |
| `FUN_8005E574` | Streaming-read per-IRQ callback (registered by `FUN_8005E788`). Drives multi-sector reads via globals `DAT_800796CC` (destination cursor), `DAT_800796D8` (sectors remaining), `DAT_800796E4` (current LBA). |
| `FUN_8005E788` | Streaming-read **starter**: copies `DAT_800796C8` → `DAT_800796CC` and `DAT_800796C4` → `DAT_800796D8`, registers `FUN_8005E574` as IRQ callback, sets initial LBA via `FUN_8005C42C(FUN_8005BD70())`. |
| `FUN_8005E9A4` | Public streaming-read API: `(sector_count, dest_buffer, mode_flags)`. Sets the streaming globals + calls `FUN_8005E788(0)`. Caller must SetLoc beforehand. Sector size from `mode_flags`: bits `&0x30 == 0` → 0x200 (2048, data), `== 0x20` → 0x249 (2336, XA), else 0x246. |
| `FUN_8005E4D4` | Sync LBA-based file reader: `(sector_count, lba, dest_buffer)`. Wraps `FUN_8005C328` + `CdControl(SetLoc)` + `FUN_8005E9A4` + completion poll. |
| `FUN_8003D3C4` | Path-based ISO9660 file loader: `(path, dest)`. Wraps `FUN_8005DBB4` + SetLoc + `FUN_8005E9A4`. Used for `.STR`/`.XA` filesystem files. |
| `FUN_8003E4E8` | Boot-time TOC loader: `(filename_str, do_read_flag)`. Hardcoded for `"PROT.DAT"` from `FUN_8003F08C(0)`. Reads 3 sectors (= 6 KB) into `0x801C70F0`. |
| `FUN_8003E800` | Async LBA-based loader: `(dest, lba, flags)`. Queues a load via globals `gp+0x97c` (lba) / `gp+0x894` (dest), kicks via `FUN_8003F128`. Used by both overlay loaders. |
| `FUN_8003E8A8` | PROT TOC index resolver: `(prot_index, flag)` → LBA. Reads `*(0x801C70F0 + (index+2)*4)` matching the [PROT TOC math](../formats/prot.md). |
| `FUN_8003EBE4` / `FUN_8003EC70` | Parallel overlay loaders A/B (see Game-mode state machine section). Both call `FUN_8003E8A8(param + 0x381)`; in extraction index space that is **entry `param + 0x37F`** (the resolver indexes the raw in-RAM `PROT.DAT` head, 2 entries above the extraction indexing - see the index-spaces note above the mode table). Differ only in destination buffer pointer (`*DAT_8001038C` vs `*DAT_80010390`) and current-id tracker (`gp+0x924` vs `gp+0x934`; `gp = 0x8007B318`, so `0x8007BC3C` / `0x8007BC4C`). |

`FUN_8003E360` shows a **dual-mode loader pattern** keyed on the dev/retail flag `_DAT_8007B8C2`: retail branch uses ISO9660 file system (`FUN_800608F0` open + `FUN_80060944` read), debug branch uses PROT TOC index (`FUN_8003E8A8` + `FUN_8003E800`). The two branches load the same data from different on-disc locations.

### Pre-`init_data` system-UI gap (menu-glyph atlas + boot cursors)

A separate 236 KB / 118-sector unindexed region sits **between the TOC and the first indexed entry** (`init_data` at LBA 121). The TOC ends at PROT.DAT offset `0x1800` (3 sectors); the first indexed payload starts at `0x3C800` (sector 121). Everything in between is uncovered by the per-entry extractor.

The gap is a packed bundle of system-UI TIMs (boot-time cursors, the menu-glyph small-caps font, ornamental sprite strips). All TIMs are 4bpp + CLUT and target the bottom-right corner of PSX VRAM (the canonical "system UI" rectangle at `fb_x >= 640`).

| PROT.DAT offset | TIM dims | VRAM target          | Purpose                                                      |
|-----------------|----------|----------------------|--------------------------------------------------------------|
| `0x01858`       | tiny     | `(896,256)` 1×4      | boot cursor variant                                          |
| `0x018E0`       | 256×192  | `(896,256)` 64×192   | large UI sprite sheet                                        |
| `0x07B00`       | 32×32    | `(928,352)` 16×32    | UI element                                                   |
| `0x07F40`       | 256×256  | `(896,0)`   64×256   | dialog-font / large bitmap sheet                             |
| `0x0FF80`       | 4×4      | `(896,448)` 1×4      | cursor                                                       |
| `0x10028`       | 4×4      | `(896,448)` 1×4      | cursor                                                       |
| `0x100D0`       | 4×4      | `(896,448)` 1×4      | cursor                                                       |
| `0x10178`       | 256×32   | `(896,448)` 64×32    | AP / status-icon sprite sheet                                |
| **`0x11218`**   | 256×256  | `(960,256)` 64×256   | **menu-glyph small-caps font** (NEW GAME / CONTINUE / …)     |
| `0x19438`       | 240×24   | `(960,400)` 60×24    | UI sprite strip                                              |
| `0x1AC90`       | 16×16    | `(976,256)` 4×16     | cursor part                                                  |
| `0x1AD50`       | 16×16    | `(980,256)` 4×16     | cursor part                                                  |
| `0x1AE10`       | 16×16    | `(984,256)` 4×16     | cursor part                                                  |
| `0x1AED0`       | 32×32    | `(976,272)` 8×32     | cursor                                                       |
| `0x1B80C`       | 256×256  | `(640,0)`   64×256   | system sprite sheet                                          |

#### Menu-glyph atlas

The TIM at `PROT.DAT[0x11218..0x11218 + 33312]` (256×256 @ 4bpp + 16×16 CLUT bank) is a small-caps glyph atlas used by the in-game menu UI (shop / inventory / status panels). Confirmed by pinning the in-RAM copy at vaddr `0x80106478` (sstate8, live title-menu state) against PROT.DAT - byte-equal modulo the runtime CLUT relocation. The atlas does NOT appear in any extracted PROT entry; it's strictly in this pre-`init_data` gap.

| Glyph row  | Atlas Y    | Cell W | Cells | Content                                  |
|------------|------------|--------|-------|------------------------------------------|
| Digits     | 209..220   | 8      | 10    | `0123456789`                             |
| Alphabet   | 224..238   | 8      | 26    | `ABCDEFGHIJKLMNOPQRSTUVWXYZ`             |

Each cell is 8 px wide on a fixed 8 px pitch starting at `x = 8`. The atlas also carries non-glyph debug content (a `<DEMO>` row, the dev string `ここは常駐エフェクトが入る予定 / Pochi`, a `FONT CLUT` palette-bar indicator, and various cursor / arrow sprites) - all ignored by the engine.

CLUT row 0 renders the alphabet in solid red with magenta highlights; retail switches CLUT rows per context to read white / gold / dim. The clean-room engine sidesteps the CLUT-switching logic by decoding once to a stencil (pixel-index 0 → transparent, indices 1..15 → opaque white) and applying a `SpriteDraw::color` tint at draw time - see `crates/engine-core/src/menu_glyph_atlas.rs`.

**Note on title-screen "NEW GAME" / "CONTINUE":** The title menu rows are NOT rendered from this atlas - retail samples a pre-rendered band at `y=227..237` inside the title TIM itself (PROT 0888 / 0889 / 0890; see [`legaia_asset::title_pak::TITLE_BAND_MENU_NEW_GAME`] / [`TITLE_BAND_MENU_CONTINUE`]). The band carries both strings packed into a single 128×10 strip; the clean-room engine emits two `SpriteDraw`s sampling the left half (x=0..65) and right half (x=65..127) of that strip. Selection is colour-coded: bright/white for the cursor row, dim/gray otherwise - there's no arrow / cursor mark in retail.

#### Extraction

```rust
use legaia_asset::menu_glyph_atlas;
let prot_dat = std::fs::read("extracted/PROT.DAT")?;
let tim = menu_glyph_atlas::extract_from_prot_dat(&prot_dat)?;
// tim.bytes is the 33312-byte slice starting at PROT.DAT[0x11218].
```

The engine reads the slice directly via `ProtIndex::prot_dat_raw_bytes(byte_offset, len)` (added on top of the existing `entry_bytes` / `entry_bytes_extended` API).

#### Loader pathway (hypothesis)

These TIMs land in main RAM at vaddrs `0x80105000..0x80110200` (well below the `0x801C0000+` overlay window), which means they're treated as **shared static assets**, loaded once at boot before any overlay. The loader has not been pinned function-by-function yet; the most likely candidate is the same CD-DMA-channel-3 read primitive (`FUN_8005D9A0`) that delivers the title overlay, driven from the SCUS-side boot sequence. (There is no separate "bulk-initializer" - the `FUN_8005DA40` of earlier notes is a Ghidra-promoted intra-function label inside `FUN_8005D9A0`; see the negative findings below.)
Confirming this requires a Write-breakpoint capture targeting the `0x80105000..0x80110200` range on cold boot, mirroring the title-overlay hunt in [`scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua`](../../scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua).

### Title-overlay source on disc

The title-overlay code (function `FUN_801DD35C` at `0x801DD35C`, the captured `overlay_title.bin` 256-KiB window) lives in an **unindexed 60-sector gap inside `PROT.DAT`** between TOC entries 899 and 900. The per-entry extractor stops at each TOC entry's claimed size, so the gap bytes never land in `extracted/PROT/`. To reach them, slice `PROT.DAT` directly.

| Range (PROT.DAT) | Sectors | Bytes | Contents |
|---|---|---|---|
| `0x5C3D800..0x5C44800` | 47227..47241 | 28 672 | PROT entry 899 indexed payload (extracted as `0899_xxx_dat.BIN`) |
| `0x5C44800..0x5C62800` | 47241..47301 | **122 880** | **Unindexed gap = title overlay code** |
| `0x5C62800..0x5C67800` | 47301..47311 | 20 480 | PROT entry 900 indexed payload (extracted as `0900_xxx_dat.BIN`) |

The title-tick body (`FUN_801DD35C`) source is at `PROT.DAT` offset `0x5C4C344` (gap-relative `+0x7B44`, sector +15 within the gap). Capture the gap as a standalone file with:

```python
raw = open("extracted/PROT.DAT","rb").read()
open("title_overlay.bin","wb").write(raw[47241*0x800 : 47301*0x800])
```

#### How the load happens

The SCUS boot sequence issues a multi-sector `ReadN` starting at PROT 899's LBA (47227) and reads ~74 sectors of contiguous on-disc data - crossing PROT 899's TOC-claimed end (47241) into the unindexed gap. The CD-DMA primitive (`FUN_8005D9A0`) breaks the read into 5 sequential DMA bursts:

| DMA burst | RAM dst | PROT.DAT source offset | Caller |
|---|---|---|---|
| 1 | `0x801CF818` | `0x5C3E800` (PROT 899 +0x1000, sec +2) | `pc=0x8005DA50, ra=0x8005C2D4` |
| 2 | `0x801D4818` | `0x5C43800` (PROT 899 +0x6000, sec +12) | same |
| 3 | `0x801D9818` | `0x5C48800` (gap +0x4000, sec +8) | same |
| 4 | `0x801DD018` | `0x5C4C000` (gap +0x7800, sec +15) | same |
| 5 | `0x801E4818` | `0x5C53800` (gap +0xF000, sec +30) | same |

Capture pipeline: [`scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua`](../../scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua) (cold-boot mode, `LEGAIA_NO_SSTATE=1`) arms Write breakpoints inside the overlay range and captures the DMA-driven writes - PCSX-Redux Lua Write BPs catch DMA writes from CD-DMA-channel-3.

#### Why the TOC misses it

The per-entry size formula `size_sectors = toc[p+5] - toc[p+3] + 4` (see [`docs/formats/prot.md`](../formats/prot.md) and [`crates/prot/src/archive.rs`](../../crates/prot/src/archive.rs)) gives 14 sectors for PROT 899, but the on-disc contiguous range between PROT 899 and PROT 900 is 74 sectors. The formula appears to describe an "indexed" subset of each entry's disc footprint, with trailing unindexed bytes carrying overlay code that the SCUS loader reads by passing an explicit larger sector count. The same pattern may apply to other entries - comparing each TOC slot's claimed size to the gap to the next entry would identify other hidden overlays.

#### Negative findings (corrects earlier notes)

- The historical claim "title overlay code is not in any PROT entry" was **methodologically** correct (it isn't in any **extracted PROT file**) but missed the disc-level reality: the bytes ARE in PROT.DAT, just outside the indexed entries.
- A lossy-LZS brute-force scan returned zero hits because the title overlay is **not compressed**; the CD-DMA primitive copies raw bytes straight into the overlay window.
- The "FUN_8005DA40 walks pointer table _DAT_800795B4" claim from earlier notes is unverified. `0x8005DA40` is an intra-function instruction inside `FUN_8005D9A0` (the CD-DMA-channel-3 read primitive) - Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx`. The actual DMA-driver site is `pc=0x8005DA50`.

The script VM that drives every running script is **not** in `SCUS_942.54` - it lives in RAM overlays at `0x801C0000+`. The actor / sprite VM (`FUN_801D6628`) is in the title-screen overlay; the field/event VM (`FUN_801DE840`) is in the town/field overlay; the effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`) is in the battle overlay. See [actor VM](actor-vm.md), [field VM](script-vm.md), and [effect VM](effect-vm.md).

## Title-screen overlay state

The title-screen overlay loads into `0x801E0000+` during the boot sequence and keeps its mode state in a struct at `0x801EF018`. Known fields:

| Offset | Width | Field |
|---|---|---|
| `+0x154` | u32 | Title-attract idle countdown (`_DAT_801EF16C`). Initialized to `0x8000`; decremented per-frame by `_DAT_1F800393` (the global per-frame scalar - same byte used by `World::tick_move_vms_with_delta`); underflow writes the master game-mode index to `0x1A` (= STR FMV mode 26) and zeroes the FMV id at `_DAT_8007BA78` → `MV1.STR`. See [`cutscene.md`](cutscene.md). |
| `+0x158` | u32 | Title-overlay frame counter (`_DAT_801EF170`). Incremented unconditionally every tick. |

Initial values are **disc bytes**: the CD-DMA-channel-3 primitive `FUN_8005D9A0` copies the overlay image (initialized data included) into the overlay window, and a write-watch on the countdown fires at the DMA-trigger instruction `0x8005DA4C` inside it. The countdown's `0x8000` sentinel is therefore part of the overlay's on-disc initialized data, not computed by an init routine. (An earlier reading attributed this to a "bulk-initializer `FUN_8005DA40` walking a pointer table `_DAT_800795B4`" - both artifacts of a Ghidra-promoted intra-function label; see the negative findings above.)

### Tick function

The per-frame tick function is `FUN_801DD35C` (entry `0x801DD35C`, 12 104 bytes / 3 026 instructions, in the title overlay at `0x801C0000+`, **not** in SCUS). Pinned via a PCSX-Redux watchpoint on the countdown - the BP captured `pc=0x801DDCCC` on the exact `sw v0, -0xe94(a0)` instruction that writes the decremented value back. Full disassembly + decompile in `ghidra/scripts/funcs/overlay_title_801ddccc.txt`; capture pipeline in `scripts/pcsx-redux/autorun_countdown_trigger.lua` (defaults to slot-8 save state; outputs RAM + screenshot + regs to `captures/boot_walk/overlay_title.bin*`).

Decrement sequence (around `0x801DDCB0..0x801DDCCC`):

```asm
lui   a0, 0x801f
lui   v1, 0x1f80
lbu   v1, 0x393(v1)     ; v1 = *_DAT_1F800393  (per-frame scalar)
lw    v0, -0xe94(a0)    ; v0 = *0x801EF16C     (countdown, u32)
nop
subu  v0, v0, v1        ; v0 -= scalar
bgez  v0, 0x801dfc3c    ; if signed >= 0, branch to "still counting"
_sw   v0, -0xe94(a0)    ; <-- captured pc: store decremented value
```

The "still counting" path branches to `0x801DFC3C` (the normal per-frame attract loop: rendering, input, cursor logic). The "underflow" path falls through past `0x801DDCCC` into a block that prepares draw primitives via `0x80058490` and writes the master game-mode index `_DAT_8007B83C = 0x1A`, zeroing `_DAT_8007BA78` (FMV id slot) → `MV1.STR`.

### Sub-mode dispatcher

The first ~250 instructions of `FUN_801DD35C` set up per-frame state (input read, fade-fill via `FUN_80024EE4`, slider/cursor clamps) and then fan out via a 25-entry jump table:

```asm
801dd6ac  lw   a0, 0x204(v0)        ; a0 = state[0x204]  (= sub-mode)
801dd6b0  jal  0x801e38d0            ; identity (jr ra ; _move v0,a0)
...                                  ; input/cursor/screen-fade preamble
801dd7f8  sltiu v0, s2, 0x19         ; clamp s2 < 25
801dd7fc  beq  v0, zero, 0x801dfc3c  ; out-of-range → main body
801dd800  _lui  v0, 0x801d
801dd804  addiu v0, v0, -0xdbc       ; JT base = 0x801CF244
801dd808  sll  v1, s2, 0x2
801dd80c  addu v1, v1, v0
801dd810  lw   v0, 0x0(v1)
801dd818  jr   v0                    ; dispatch
```

`FUN_801E38D0` is a 2-instruction identity, so `s2 == state[0x204]` after the call. The 25-entry JT at `0x801CF244` (read directly out of `captures/boot_walk/overlay_title.bin`):

| Mode | Handler PC | Mode | Handler PC | Mode | Handler PC |
|------|------------|------|------------|------|------------|
| `0x00` | `0x801dd820` | `0x09` | `0x801de638` | `0x12` | `0x801def38` |
| `0x01` | `0x801dfc3c` (= tail) | `0x0a` | `0x801de798` | `0x13` | `0x801df404` |
| `0x02` | `0x801dddfc` | `0x0b` | `0x801dea5c` | `0x14` | `0x801ddf30` |
| `0x03` | `0x801df5bc` | `0x0c` | `0x801de680` | `0x15` | `0x801de260` |
| `0x04` | `0x801df33c` | `0x0d` | `0x801de728` | `0x16` | `0x801df8d0` |
| `0x05` | `0x801df82c` | `0x0e` | `0x801dec40` | `0x17` | `0x801df6f4` |
| `0x06` | `0x801dfb5c` | `0x0f` | `0x801dee0c` | `0x18` | `0x801ddd94` |
| `0x07` | `0x801de134` | `0x10` | `0x801ddb0c` | | |
| `0x08` | `0x801de4a4` | `0x11` | `0x801dda90` | | |

Mode `0x01` jumps directly to the post-dispatch tail (no-op for that frame). The eligible attract-fire mode is the one whose handler runs through the countdown decrement at `0x801DDCCC` (mode `0x10` per the cutscene-trigger watchpoint capture).

**This sub-mode SM is the front-end title-menu + memory-card manager + new-game/continue launcher** - not an opening-narration/name-entry sequence.

Full C-decomp of `FUN_801DD35C` (a `switch` over sub-mode `DAT_801f0204`, cases `0x00..0x18`) shows every state is title-menu or memory-card UI:

- The strings are all card/save messages (`s_Now_checking_MEMORY_CARD`, `s_Do_you_wish_to_format`, `s_Load_successful`, `s_No_Legend_of_Legaia_data_on_this`, …) and `FUN_801E3EE0`/`FUN_801E36C4` are the centered card-message text+box drawers (used for "Now checking MEMORY CARD", "Load/Save successful").
- Sub-mode `0x10` is the **menu** (2-option cursor `_DAT_8007B820`, up/down `pad & 0x4000/0x1000`, confirm `0x844`; idle timeout → master-mode `0x1A` = attract/opening FMV).
- `0x15` is the **card-check** poll (counter to `0x259`, "Now checking" / "An error occurred" + retry).
- Menu confirm → fade sub-mode `0x16` → `init_game` (`0x06`, `0x801DFB5C`).

NEW GAME and CONTINUE both funnel **menu → fade → init_game → master-mode 2 (field)**; `init_game` writes the **opening scene id** `opdeene` (the prologue cutscene, CDNAME/PROT #748) into the active-scene-name buffers (`0x8007050C` / `0x80084548`) - verified live: at the `new_game_cutscene_intro_a` save the scene name is `opdeene`, and at the later Rim Elm saves it is `town01`. (`s_opdeene` is therefore the opening *scene id*, not a player name.)

There is **no opening narration and no name-entry anywhere in this front-end SM** - the engine's "menu → `begin_new_game` → field" jump is faithful to retail's *front-end*. The opening narration and the name-entry happen **downstream of the field launch**, not as title sub-modes.

The retail opening (pinned by a PCSX-Redux cold-boot pixel capture; earlier anchors in the save-state corpus `new_game_cutscene_intro_a` / `rim_elm_zoom_intro` / `vahn_walks_out` / `name_input_ui`, [`scripts/scenarios.toml`](../../scripts/scenarios.toml)) is a **five-scene engine-rendered chain**, not a single cutscene: `opdeene` (the Genesis-tree creation-myth crawl, *"It was the Seru."* - not an STR FMV) → `opstati` (Seru intro) → `opurud` (Mist story) → `map01` (the world-map fly-in: title card + crawl over an aerial approach of Rim Elm) → `town01` (establishing pan → **name entry**, *"Select your name."*, default `Vahn` → Vahn's scripted walk-out → free roam).

The whole chain runs in master mode `0x03` (field RUN) with **zero input**; each leg chains by its own script (see below). The new-game data-init (`FUN_80034A6C`, gold/flags/stats) runs before this. The `FUN_801D1344` scene-change packet described below is the **intro skip**; the name-entry is the menu overlay described below. Full chain + narration mechanics: [`cutscene.md`](cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain).

The JT, state-struct field offsets, and observed `state[+0x204] = N` transitions are pinned in [`legaia_engine_vm::title_overlay`](../../crates/engine-vm/src/title_overlay.rs). Four modes are semantically labelled: `Init` (`0x00` - entry init that routes to `Phase02` or `AttractDelay`), `Idle` (`0x01` - body-tail no-op), `AttractIdle` (`0x10` - Press-Start poll), `AttractDelay` (`0x11` - pre-attract delay). The other 21 carry `Phase0xNN` placeholders with traced-transition docstrings; the module's `STATE_204_WRITES` table holds the full graph. Notably, **Phase06 writes `_DAT_8007B83C = 0x02` at `0x801DFC00`** - the title-screen → main-game master-mode transition (exported as `MASTER_GAME_MODE_FIELD_LAUNCH` + `PHASE06_LAUNCH_GAME_PC`).

### The opening scene chain + the `FUN_801D1344` intro skip

The natural (zero-input) opening chains scene to scene by **script execution**: `opdeene`'s timeline record P2[18] ends with a field-VM `0x3F` SceneChange to `opstati`, which chains to `opurud`, then to `map01`, which scene-changes into `town01` at tile `(0x1D, 0x5B)`. Each leg's opening record spawns through one of two mechanisms, both pinned by a live PCSX-Redux exec-breakpoint on the record dispatcher `FUN_8003BDE0` (exactly 5 hits across the opening): **op `0x44` SPAWN_RECORD** in the scene's P1[0] entry script (`opdeene` / `opstati` / `opurud`) or the **walk-on tile trigger** at the arrival tile (`map01` / `town01`; `FUN_801D1EC4` → `FUN_801D5630` → `FUN_8003BDE0`). See [`cutscene.md`](cutscene.md#record-spawn-mechanisms-live-probe-pinned).

On top of that, a **confirm press at any time after `opdeene`'s timeline arms `GFLAG 26`** (near the record's top) fires a name-based scene-change packet straight to `town01` - the **intro SKIP**. (Its former reading as the *required* hand-off gate - "narration plays, then a confirm press triggers the transition" - is superseded: the chain advances by itself, and the packet only fires on the skip.) The packet path is not the map-id door-warp (the `0x3E`/`FUN_80025980` path below) - confirmed empirically: the WARP handler backs up the active scene name into `0x8007BAE8`, but that buffer is **empty in the `town01` opening saves**.

The skip mechanism, traced through the field/cutscene overlay:

- **`FUN_8001FD44(name_ptr)`** - the scene-change-packet API. Copies the target scene name into `0x8007050C`, syncs it to the active buffer `0x80084548` via **`FUN_8001D7F8`**, and (gated on the dev/debug flag `_DAT_8007B8C2`) stages the load. The error string `s_ERR_CHANGE_PACKET` guards re-entry while a previous packet is pending (`_DAT_8007BA3C`). It takes a single argument - the `a1`/`3` the decompiler shows at the opening call site is dead (the body never reads `a1`). The next field-init (`FUN_801D6704`) reads `0x80084548` and loads the named scene.
- **`FUN_801D1344`** - the per-frame field/cutscene controller that issues the skip packet. It fires a **one-shot, flag-gated, pad-gated** block:

  ```c
  if (_DAT_8007b868 == 0 && (_DAT_1f800394 & 0x4000000) && (_DAT_8007b850 & 0x100)) {
      FUN_801d58f0(2, 0, 0xffffff, 0, 0x3c, -1);   // fade out
      _DAT_80073ef4 = 0xec0;  _DAT_80073ef8 = 0x2dc0;   // town01 entry coords
      _DAT_1f800394 &= 0xfbffffff;                  // clear bit 0x4000000 (fire-once)
      func_0x8001fd44(s_town01_801ce82c, 3);        // next scene = "town01"
  }
  ```

  The target name **`"town01"` is hardcoded as the overlay literal at `0x801CE82C`** - that is why a scan of `opdeene`'s per-scene data (MAN + event scripts) finds no `town01` string. The pad bit `_DAT_8007B850 & 0x100` is the player's **skip press** - it fires mid-narration too (the crawl is timer-driven, not confirm-paced).

  **Trigger flag (`_DAT_1F800394 & 0x4000000`, bit 26).** Set by the field VM's generic scratchpad-bit opcode **`GFLAG_SET` (op `0x2E`, operand `0x1A`)** - the dispatcher in `FUN_801DE840` runs `_DAT_1f800394 |= 1 << (idx & 0x1f)`; `idx = 0x1A` is bit 26. The only `GFLAG_SET 26` in `opdeene`'s decoded MAN is **not** in the partition-1 per-actor/scene-entry scripts; it lives in the **last record of the MAN's third record partition** (partition index 2, count 19; record start at MAN file offset `0xA47`, the `2E 1A` at `0xA5E` = body `+0x17`).
  That record is the cutscene-timeline script - it arms the skip bit **near its top**, right after the opening white fade-in, so the skip is available almost immediately; the record then stages the whole vignette choreography and ends with the `0x3F` SceneChange to `opstati`.
  (Earlier notes guessing a `0x4C` MenuCtrl sub-op in record 0 are falsified - there is no `0x2E`/`0x2F` byte anywhere in record 0.)

The executable's **default** scene name is also `town01`: `FUN_8001D424` reads the dev file `initmap.txt` and copies 16 bytes into `0x8007050C`. `init_game` overrides this with `opdeene` for a real New Game; the natural chain (or the skip packet above) eventually sets it back to `town01`.

**Clean-room port.** [`World::take_prologue_handoff`](../../crates/engine-core/src/world/narration.rs) mirrors the `FUN_801D1344` gate: while the opening chain is playing (`World::opening_chain_active`, set at the `opdeene` entry and carried through the `opstati` / `opurud` / `map01` legs) and the trigger bit ([`PROLOGUE_HANDOFF_FLAG`] = `1 << 26` in [`World::story_flags`], the engine's `_DAT_1F800394` mirror) is set, a confirm press clears the bit (fire-once), tears down the playing narration / timeline, and returns `town01`. On the `Some(target)`, the host runs `BootSession::enter_field_live(target)` (the engine's scene-change-packet equivalent).

The arm fires **by execution**: entering `opdeene` installs its timeline record as a spawned field-VM context ([`World::load_cutscene_timeline_from_man`](../../crates/engine-core/src/world/narration.rs)) and the `GFLAG_SET 26` writes the bit through the same host path the main field VM uses. A static MAN-walk arm ([`World::arm_prologue_handoff_from_man`], built on [`man_field_scripts::walk_partition_gflag_sites`](../../crates/engine-core/src/man_field_scripts.rs)) remains as the fallback, so a cutscene scene that never issues that write can never produce a false skip. The disc-gated test `opdeene_prologue_arm.rs` pins the `GFLAG_SET 26` at the partition-2 record-18 offset `0xA5E` and asserts `town01` carries no such arm.

The opening narration **plays from the timeline records by execution**: each leg's inline subtitle pages (`0x1F`/`0x00`-framed ASCII decoded by `legaia_asset::cutscene_text`) roll through the bottom-up **crawl roller** (`FUN_80037174`; engine [`CutsceneNarration`](../../crates/engine-core/src/cutscene_narration.rs)), spawned as a child context so the parent timeline keeps running and the between-block camera cuts play under the scroll (it blocks only on a scene's last block) - see [`cutscene.md`](cutscene.md#narration-playback---the-crawl-roller-fun_80037174). The field-VM op that auto-opens the name-entry overlay is pinned - see [Name-entry overlay](#name-entry-overlay).

### Name-entry overlay

The *"Select your name."* screen (default `Vahn`) runs **after** the field launches, as a menu overlay invoked during the `town01` opening (master mode `0x03`, captured in the `name_input_ui` save state). It is part of the field/dialog overlay (loaded at `0x801C0000`), not a title sub-mode.

**The field-VM op that opens it is pinned:** op `0x49` **STATE_RESUME sub-op 3** at `town01` partition-2 record 3 (P2[3]) body offset `0x02c6` (`49 03 00`), in the opening cutscene timeline. After the establishing camera pan, the script suspends on this STATE_RESUME and `op49_invoke_setup` (`func_0x80020de0(0x8007065c, _DAT_8007c34c)`) hands off to the name-entry overlay; Vahn's scripted walk-out plays **after** the name commits (the walk-out is post-confirm).
Confirmed by executing P2[3] through the engine field VM and correlating against this save: `_DAT_8007B450` (the op-`0x49` state slot) holds `0x800EB297`, which is the `0x49` op's RAM address + 1 (the record loads with body `0x02b0` at RAM `0x800EB280`, byte-identical), so the field script is parked precisely at this op while name entry is up. Regression:
`crates/engine-core/tests/town01_opening_timeline_trace.rs`.

This is **executed in-engine**: the `town01` entry installs P2[3] as a spawned cutscene timeline - on the natural chain arrival via the walk-on tile trigger at `(0x1D, 0x5B)` (its C1 gate lists flag `0x225`, the one-shot), or on the intro skip via [`World::install_town01_opening_timeline`](../../crates/engine-core/src/world/narration.rs) (gated on `entering_town01_opening` AND the record's own C1 gate, so both routes share the retail one-shot). Flag `0x225` (549) itself lands from the record's opening `52 25` script bytes when the timeline executes - the self-latching one-shot (disc-gated `organic_beat_records_disc.rs`).
The timeline plays the establishing camera beats over ~490 frames (stepping past the conditional-wait parks the engine doesn't model - `0x4C` nibble-C `script_alloc`/globals, `0x2D`/`0x30` flag-tests - while honoring `0x4A` timed waits), then op `0x49` opens the name-entry overlay through the op-49 host hooks (`op49_invoke_setup` → `open_name_entry(0)`; `op49_state` Armed while open, Done after commit).
The timeline freezes while the overlay is up and resumes - playing Vahn's walk-out - once a name commits. Disc-gated `town01_opening_name_entry_wiring.rs` + `opening_full_chain_e2e.rs`.

Pinned addresses (live in `name_input_ui`):

| Datum | Address | Notes |
|---|---|---|
| Character grid | `0x801F29F0` | Flat ASCII, 6 rows × 17 bytes: `ABCDE\|abcde\|12345`, `FGHIJ\|fghij\|67890`, `KLMNO\|klmno\|!?#%&`, `PQRST\|pqrst\|.,'<>`, `UVWXY\|uvwxy\|+-*/=`, `Z␣␣␣␣\|z␣␣␣␣\|:;()~`, NUL-terminated. Three column-groups of 5 (`\|` = `0x7C` separators); spaces pad the short Z/z row → 15 cols × 6 rows. |
| Live name buffer | `0x801F2A6C` | The name being edited (`Vahn` by default). |
| Cursor index | `0x8007BB88` | Linear position over a **7-row × 17-col** navigation space (`0..0x77` = 119), wrapped modulo `0x77`. Cells `0..0x66` (102) are the glyph rows; `0x66..0x77` are the control row. `row = cursor/17`, `col = cursor%17`. |
| Pad edge bits | `0x8007BB84` | Just-pressed mask for this frame (d-pad tested as `0x1000`/`0x4000`/`0x2000`, confirm via the button-mask table AND-ed with held pad `0x8007B874`). |
| Char-record pointer / op-0x49 slot | `0x8007B450` | The field-VM op-`0x49` STATE_RESUME state slot (`0` idle, `1` done, else an armed PC pointer). While name entry is open it holds `0x800EB297` = the opening timeline's `0x49` op address + 1 (the script is suspended there). The live character record being named is reachable through it; the committed name lands at record offset `+0x2A7` (record base `0x80084708 + n*0x414`; save-block offset `+0x86F` for slot 0). |
| Prompts | `0x801CF698`+ | "Is this name okay?", "Cannot enter that name.", "Tell me my name.", "Select your name.", "[Nameless]". |

Two functions carry the screen:

- **`FUN_801E6B34`** (render) - draws the 6×17 grid (skipping `|` / space) via the glyph drawer `FUN_80036888`, plus the current name, the blinking caret (the `Vahn_` underscore, measured with MES `FUN_8003CA38` + width `FUN_80035F04`), and the box frames (`FUN_8002C69C`).
- **`FUN_801F03F0`** (state machine) - substate at `struct+0x54`, dispatched through a **5-entry jump table at `0x801CF71C`**:
  - `0x801F0444` **init** - sets the active flag and advances to interactive.
  - `0x801F0480` **interactive** - d-pad deltas `-0x11` (up) / `+0x11` (down) / `+1` (right) / `-1` (left); after each move the cursor wraps modulo `0x77` and **skips non-selectable cells** (the `|`=`0x7C` separators in the glyph rows) in the direction of travel. Confirm resolves the cell: a glyph cell appends its character to the name (length-bounded by the proportional-font pixel width, cap `0x39`=57 px); a control-row cell runs its action via the row's sentinel bytes - `0x66` = **Backspace** (truncate one glyph), `0x64` = **Space**, `0x65` = **End** (gated on a non-empty name via the `blez` check → advances to confirm).
  - `0x801F095C` / `0x801F09C0` / `0x801F097C` **confirm** - the "Is this name okay?" Yes/No prompt; Yes commits the name into the record's name field at `+0x2A7` (save-block `+0x86F`) and exits, No returns to interactive.

The control row (grid row 6) tiles those sentinel bytes across its columns: `00 00 | 66×6 | 64×6 | 65×3` (filler / Backspace / Space / End).

**Clean-room engine port.** The whole SM is ported as a standalone overlay in [`legaia_engine_core::name_entry`](../../crates/engine-core/src/name_entry.rs) (`NameEntry` + `NameEntryState` + `Control`), driven on the world by `World::open_name_entry` / `step_name_entry` (committing into `World::party_names`) and rendered through [`legaia_engine_render::name_entry_draws_for`](../../crates/engine-render/src/lib.rs). In `legaia-engine play-window`, the NEW GAME flow reaches the prompt through the scene's own bytecode - the `town01` opening timeline's pinned op `0x49` (above) - whether the player rode the natural chain or skipped; the P2[3] C1 gate (flag `0x225`) keeps a normal later `town01` visit from re-prompting. A dev `N` key also opens it for testing outside the new-game flow.

### Sprite-emit helpers

The title-tick body reaches into three SCUS-side helpers to emit GPU primitives. All three are ported clean-room in [`legaia_engine_vm::title_prim`](../../crates/engine-vm/src/title_prim.rs):

- `FUN_80058298` (`ClearImage` rect-fill queue, 37 instructions) → `exec_clear_image(host, rect, r, g, b)`.
- `FUN_80058490` (`MoveImage` VRAM-to-VRAM copy, 49 instructions) → `exec_move_image(host, src, dst_x, dst_y)`, with early-out on zero extent matching the original's `li v0, -1` path.
- `FUN_800198E0` (sprite-descriptor dispatcher, 146 instructions) → `exec_sprite_descriptor(host, &SpriteDescriptor)`, with full tag-`0x11` simple variant + complex variant routing (alpha-OR pre-pass under `flags & 8`, four width-divisor variants from `flags & 3`).

`SpriteDescriptor { tag, flags, rect, pixel_data_ptr }` and `Rect12 { x, y, w, h }` capture the wire shapes. The `PrimHost` trait abstracts the four engine-side callbacks (`queue_clear_rect`, `queue_move_image`, `emit_sprite`, `alpha_or_gate_set`); engines wire those to a real GPU back-end. The overlay-side helpers (`FUN_801E1C1C` / `FUN_801E373C` / `FUN_801E3EE0` / `FUN_801E36C4`, each ~8 KiB, shared across menu / battle / shop / save UI overlays) are deferred to their own focused port - the title-tick body's calls into them can be stubbed against the same `PrimHost`.

### State struct (extended)

Base `0x801F0000` (the `a0` arg). Sibling region at `0x801EF014..0x801EF200` reached via *negative* displacements off the same `lui 0x801f`.

| Address | Off | Use |
|---|---|---|
| `0x801EF14C` | `-0xeb4` | Horizontal slider X, clamped `[0, 0x2c]`. Direction in `state[+0x1e0]` (`1`=left, `2`=right, else idle). Step per frame = `frame_scalar * 8`. |
| `0x801EF160` | `-0xea0` | Fade/sweep accumulator (clamped `[0, 0x1000]`). |
| `0x801EF16C` | `-0xe94` | Attract countdown (u32, init `0x8000`). |
| `0x801EF170` | `-0xe90` | Tick counter (unconditional increment). |
| `0x801EF190` | `-0xe70` | Alpha A, clamp `0x1000`. |
| `0x801EF194` | `-0xe6c` | Alpha B, clamp `0x1000`. |
| `0x801EF1A0` | `-0xe60` | Alpha C, clamp `0x1000`. |
| `0x801F01E0` | `+0x1e0` | Slider direction. |
| `0x801F01F4` | `+0x1f4` | X-cursor grid, clamp `[0, 4]`. |
| `0x801F01F8` | `+0x1f8` | Y-cursor grid, clamp `[0, 2]`. |
| `0x801F01FC` | `+0x1fc` | Linear cursor index, clamp `[0, s7-1]`. |
| `0x801F0204` | `+0x204` | **Sub-mode dispatcher** (drives the JT above). |
| `0x801F0230` | `+0x230` | Top-of-tick early-out guard. |

The 256-KiB `overlay_title.bin` window does carry two real TIMs embedded in the title overlay's data segment, at the same addresses the tick body's `FUN_800198E0` sprite-descriptor calls reference: `0x801E5120` (256×256 4bpp save-menu UI atlas - memcard icons + Japanese strings) and `0x801EE120` (256×16 4bpp animated PSX memcard icon strip, 14 frames). Both byte-match `extracted/PROT/0899_xxx_dat.BIN` at file offsets `0x16908` / `0x1F908` (i.e. they live in the trailing-overlay portion of PROT 899). A reusable scanner at [`scripts/asset-investigation/scan_tims_and_match_prot.py`](../../scripts/asset-investigation/scan_tims_and_match_prot.py) walks a PSX main-RAM dump for TIM-magic records and byte-greps the PROT corpus to pin each candidate.

The **main title-screen art** itself (Legend of Legaia wordmark, orb, `PRESS START BUTTON`, `NEW GAME` / `CONTINUE` menu, copyright lines) lives outside the title-overlay window - it's loaded into main RAM at `0x80170DF8` and sourced from **PROT 0888** (CDNAME label `sound_data2`; the multi-bank sound-data cluster carries title art in the trailing pool past the audio payload). Duplicate copies live in PROT 0889 and 0890 at slightly different file offsets:

```text
PROT 0888 @ 0x1AA28    - 256×256 8bpp, 66 080 bytes - PRIMARY
PROT 0889 @ 0x19A28    - same content (multi-bank dup)
PROT 0890 @ 0x14228    - same content (multi-bank dup)
```

The 256×256 image is a **sprite sheet** that bundles every text band the title screen *could* draw - retail composes the screen by sampling specific sub-rects rather than blitting the full quad. The bands, top to bottom in source-y, are:

| Source rect (`x, y, w, h`) | Content | Drawn when |
|---|---|---|
| `(0, 17, 256, 124)` | Orb + "Legend of Legaia" wordmark | every post-fade phase |
| `(96, 151, 64, 10)` | `<DEMO>` | **never** - demo-build leftover |
| `(60, 178, 196, 16)` | "PRESS START BUTTON" prompt | PressStart phase only |
| `(4, 195, 244, 14)` | "TM of Sony..." copyright | every post-fade phase |
| `(8, 209, 234, 14)` | "© 1998,1999..." copyright | every post-fade phase |
| `(0, 226, 256, 11)` | small "NEW GAME CONTINUE" footer | replaced by larger font glyphs |

The `<DEMO>` band is a residual from a development demo build that retail simply never samples - verified by capturing main RAM at the live title screen (sstate8, sub-mode `0x10` AttractIdle) and confirming the in-RAM TIM bytes byte-match PROT 0888 while the live framebuffer omits the band. The small footer "NEW GAME CONTINUE" is similarly never drawn; retail renders the menu labels using the dialog-font glyph atlas instead (which is why the on-screen "NEW GAME / CONTINUE" letters are visibly larger than the embedded footer text).

A typed parser lives at [`legaia_asset::title_pak`](../../crates/asset/src/title_pak.rs) - `extract_title_tim(&prot_0888_bytes, TITLE_TIM_OFFSET)` returns a zero-copy slice + decoded VRAM rects, and the band-rect constants `TITLE_BAND_WORDMARK` / `TITLE_BAND_PRESS_START` / `TITLE_BAND_TM_COPYRIGHT` / `TITLE_BAND_C_COPYRIGHT` (plus `TITLE_BAND_DEMO` for reference) pin the sub-rects listed above. The disc-gated unit test (`extracts_real_title_tim_when_disc_extracted`) locks the on-disc layout. An engine-side RGBA decoder lives at [`legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888`](../../crates/engine-core/src/title_screen_atlas.rs);
the play-window subcommand uploads it as a sprite atlas and emits one [`SpriteDraw`] per active band each frame (`title_screen_sprite_draws` in [`legaia-engine`](../../crates/engine-shell/src/bin/legaia-engine.rs)), with the press-start band gated on phase. The font-rendered "PRESS START" overlay is suppressed via the `atlas_present` flag on `title_draws_for` so the TIM band isn't duplicated.

### Pad-mask layout (important)

The per-frame mask at `_DAT_8007B850` and the newly-pressed mask at `_DAT_8007B874` use a **packed** layout built by `FUN_8001822C` - not the raw 16-bit PSX pad word. The builder does `~((pad[2] << 8) | pad[3]) & 0xFFFF`, so the libpad face/shoulder byte (`pad[3]`) lives in bits 0..7 and the dpad/system byte (`pad[2]`) lives in bits 8..15:

| Bit | Button | Bit | Button |
|----:|--------|----:|--------|
| 0 | L2 | 8 | Select |
| 1 | R2 | 9 | L3 |
| 2 | L1 | 10 | R3 |
| 3 | R1 | 11 | Start |
| 4 | Triangle | 12 | Up |
| 5 | Circle | 13 | Right |
| 6 | Cross | 14 | Down |
| 7 | Square | 15 | Left |

Masks the title tick exercises in this layout: `0x44 = L1|Cross` (confirm), `0x21 = L2|Circle` (cancel), `0x844 = Start|L1|Cross` (press-start / confirm), `0xf5` = all face buttons + L1 + L2 (generic "any interaction" filter). `crates/engine-core/src/input.rs::PadButton` uses the raw PSX layout (which is fine for host-side keyboard/gamepad plumbing); any code path that ingests retail RAM-side input directly needs a re-encoding step.

A **town/field subsystem** uses a separate format-string pool at `0x80011079..0x80011109` (`"    town "`, `"mode %d"`, `"    baria mode "`, `"    walking set"`, `"end of mes works set"`, `"open port.dat"`, `"nt_group_table %x"`). These print at retail-build runtime but have no LUI+ADDIU caller resident until the town/field overlay is loaded - i.e. the "mode 17 / mode 16" runtime printfs are *town-subsystem* mode transitions, not the master 28-mode state machine index.

## Boot init.pak (PROT 0895)

PROT entry `0895_bat_back_dat` is the **boot-time `init.pak` bundle** - the `bat_back_dat` label is a CDNAME block-inheritance artifact (in raw-TOC index space the `bat_back_dat 895` define lands on the `summon.dat`/`readef.DAT` battle-backdrop streaming files = extraction 893/894; see [`formats/summon-readef.md`](../formats/summon-readef.md)). The first 16 bytes are a small pack header; the rest is a string pool followed by four uncompressed PSX TIMs:

```
+0x0000  16 bytes  pack header (4 × u32 LE)
+0x0010  ~528 byte string pool with embedded dev paths:
           "init program \n"
           "h:\prot\field\init\init.pak"
           "h:\prot\field\title\title.pak"
           "h:\mpack\monster.snd"
           "\XA\XA%d.XA;1", "not xa file %d"
           "\LEGAIA\MOV\MV2.STR;1"
           "card name %s ", "card_sts=%d old=%d"
           "bu%1d%1d:*", "BISCUS-94254PRO-"
+0x21c4  TIM  PROKION         (8bpp, 176×256, ~45.6 KB) - boot logo
+0xd3e4  TIM  Contrail        (8bpp, 184×256, ~47.6 KB) - "A Contrail Production"
+0x18e04 TIM  SCEA Presents   (4bpp, 256×128, ~16.4 KB)
+0x1ce44 TIM  WARNING         (4bpp, 256×256, ~32.8 KB) - health warning
```

CLUT and pixel data are byte-identical to live RAM after boot extraction - only the RECT fields (VRAM target coords) are runtime-relocated. On-disc each TIM has CLUT `fb=(0, 480+N)` and pixel `fb=(640..800, 0..256)`; the boot loader rewrites these to per-logo VRAM regions before calling LoadImage.

A typed parser lives at [`legaia_asset::init_pak`](../../crates/asset/src/init_pak.rs) - call `parse(&prot_0895_bytes)` to get a struct view over the four logos (slice pointers + decoded VRAM rects). The disc-gated unit test (`parses_real_init_pak_when_disc_extracted`) locks the on-disc layout.

### Strip-grid unfolding

Two of the four TIMs (PROKION, SCEA) are **vertically-packed sprite atlases**: the decoded bitmap stacks several smaller strips that retail unfolds into a horizontal layout via multiple GPU quads. Blitting the whole TIM as one quad shows the packed layout (PROKION as `PROK` over `KION`, SCEA as four wrapped text rows), not the on-screen logo.

The per-logo grid is captured by [`legaia_engine_core::publisher_logos::STRIP_GRID`](../../crates/engine-core/src/publisher_logos.rs):

| Logo     | TIM       | Grid `(cols, rows)` | Source strip | Unfolded |
|----------|-----------|---------------------|--------------|----------|
| PROKION  | 176×256   | `(2, 1)`            | 176×128      | 352×128  |
| Contrail | 184×256   | `(1, 1)`            | 184×256      | 184×256  |
| SCEA     | 256×128   | `(2, 2)`            | 256×32       | 512×64   |
| WARNING  | 256×256   | `(1, 1)`            | 256×256      | 256×256  |

Source strips are stored **column-major** in the bitmap; the output grid is row-major, so source strip `s = c * rows + r` lands at output cell `(col c, row r)`. PROKION's two halves combine into `PROK ☉ KION` (the green hemispheres in each half complete a single sun in the middle when adjacent). SCEA's four 32-row strips read top-line `Sony Computer Entertainment America` + bottom-line `Presents`.

The actual on-screen layout the retail boot code uses still has to be RE'd from the unlocated title-overlay tick body - the `STRIP_GRID` constants are hypothesis-fit-to-visible-content, not pinned to specific GPU draw commands.

The `h:\prot\field\title\title.pak` string is **only a debug-print referent** - the title-screen content lives in **PROT 0888** (`sound_data2` per CDNAME, see the title-overlay-state section above) referenced by integer constant from SCUS boot code, not by string lookup. SCUS does not contain the literal string `title.pak` anywhere. The mismatch between the debug path and the actual PROT entry is the same pattern as PROT 0895 being labelled `bat_back_dat` while actually carrying `init.pak`: CDNAME labels are misleading for several entries, so always cross-validate against the loader-call constant or the file's magic bytes.

The dev-tree `title.pak` is split across two retail PROT entries, both confirmed by the init.pak fingerprint method against the `title_screen_new_game` save state (sweep RAM for TIM headers, fingerprint each CLUT, grep the PROT corpus): the **title wordmark** TIM is **PROT 0888/0890** (the big-logo RAM TIM fingerprint-matches it), and the **options / config-menu bundle** is **PROT 0899** (`xxx_dat`). Entry 0899's indexed payload opens with the config-menu string pool - `Display Off` / `Gradual` / `Immediate` / `Field HP Display` / `Encounters` / `Battles` / `Vibration Off`/`On` / `Dual Shock` / `Voices Off`/`On` / `Battle Camera` / `Monaural` / `Stereo` / `Sound` - followed by the small config-screen TIMs (CLUTs byte-matched at 0899 offsets `0x169DC` and `0x1F91C`+).
The title-overlay *code* lives in the unindexed gap immediately after entry 0899 (see [`legaia_asset::title_pak`](../../crates/asset/src/title_pak.rs) and the title-overlay-source notes). So 0888 = title image, 0899 = options/config bundle + trailing overlay code.

The TIM-upload helper for these (and for the title overlay's per-frame sprites) is `FUN_800198E0` - it consumes a packed struct with custom magic `0x11` OR a real PSX TIM (flags bit 3 = "has CLUT"), and dispatches to `FUN_800583C8` (the `LoadImage` wrapper, identified by the literal string `s_LoadImage_800156d4` it references for debug logging).

## Debug flags

- `_DAT_8007B8C2` - dev/retail build toggle. Several subsystems (sound init, field loader, save-card path, scene-change packet, title overlay) carry an "if dev" branch keyed on this byte. **Read-only at runtime**: every captured caller (`FUN_8001D424`, `FUN_8001D8FC`, `FUN_8001FA88`, `FUN_8001FC00`, `FUN_80020118`, `FUN_8003DE7C`, `overlay_menu_801DE234`, `overlay_field_battle_intro_801CF5BC`, `overlay_save_ui_*_801DD35C`, `overlay_title_801DD6B8/CCC`, ...) does a `_DAT_8007B8C2 == 0` retail-mode test; a sweep across the entire dump corpus (`SCUS_942.54` + 2660 overlay function dumps) returns **zero writes**. So the flag is BSS-resident (initialised to 0 = retail at boot) and is only mutated via external POKE - the TCRF GameShark codes that flip it to dev mode are the only known writers.
- `_DAT_8007B98F` - the most-significant byte (offset +3, little-endian) of the
  32-bit debug-mode word `_DAT_8007B98C` (NA build offset; JP retail uses
  `0x07D51F`, an `0x1B90` build-shift). The dump-corpus sweep returns zero reads
  of the *byte* `0x8007B98F` precisely because the consumer reads the *word*:
  `_DAT_8007B98C` is tested as the debug gate by `FUN_8001822c`
  (`8001822c.txt:500/533`, the input dispatcher) plus ~14 resident field-overlay
  (0897) gates, and written by the shared menu/title/save-init routine. Writing
  `0x8007B98F = 1` via external RAM poke sets the word's MSB, so every
  `_DAT_8007B98C != 0` gate reads debug mode active - SELECT+△ then brings up the
  debug menu in the NA retail build. The consumer is statically pinned (no
  uncaptured overlay needed). See `docs/reference/builds.md` "Debug input
  bindings" for the full combo table.

The input dispatcher `FUN_8001822C` reads `_DAT_8007B8C2` but doesn't write it; both flags' writers, if they ever existed, are outside any captured overlay.

## See also

**Reference** -
[PROT.DAT TOC](../formats/prot.md) ·
[Asset loader](asset-loader.md) ·
[Extraction pipeline](../tooling/extraction.md)
