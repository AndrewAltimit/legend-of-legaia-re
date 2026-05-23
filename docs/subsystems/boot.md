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
| `+0x08` | u32 | `0xFFFF0000` sentinel on most init modes; `0` on per-frame modes. |
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

Verified handler→PROT mappings (`FUN_8003EBE4` and `FUN_8003EC70` are the two parallel overlay loaders; both resolve `prot_index = param + 0x381` via `FUN_8003E8A8`, with destination buffer pointers `*DAT_8001038C` / `*DAT_80010390` respectively):

| Mode | Init handler | Loader call | PROT idx | Content (verified) |
|---|---|---|---|---|
| 0 `CONFIG INIT` | `FUN_80025C68` | `FUN_8003EBE4(0x4C)` | 973 | Slot-machine debug overlay — "OTHER2 / CICLE1 / SPRITE1 / SPREAD / GT4 DIV16" strings + slot-game text. **The dev label "CONFIG" is a misnomer for the slot-machine debug mode.** |
| 2 `MAIN INIT` | `FUN_80025B64` | `FUN_8003EBE4(2)` | 899 | **Field/town gameplay INIT.** Loads the field/town/menu overlay, then calls the per-scene initializer `FUN_801D6704` (map + MAN + camera + fog + BGM load; game-mode work buffer alloc), which hands off to mode 3 by writing `_DAT_8007B83C = 3`. The title screen's NEW GAME path launches this mode. The "Display Off / Vibration On / Voices On" strings in PROT 899 belong to the **in-game options submenu** carried by this same overlay (reached through the field menu) — they do not make mode 2 a standalone options mode. |
| 24 `OTHER INIT` | `FUN_80025980` | ? | 896 | Mode-24 OTHER overlay (cited by `dump_round8.py`'s `OVERLAY_0896_TARGETS`) — **not** "battle background" despite CDNAME `bat_back_dat`. |
| 26 `STR INIT` | `FUN_80025FB4` | (FMV path) | — | Cutscene / STR FMV mode entry. Title-overlay tick writes `_DAT_8007B83C = 0x1A` (= 26) on attract underflow → enters this mode. |

**The dev mode-names mislead.** `MAIN INIT`/`MAIN MODE` (modes 2/3) are the **field/town gameplay** init/run pair (`game_mode 0x03` is the on-field / in-town loop), *not* a standalone options screen — the per-scene initializer `FUN_801D6704` they reach is unmistakably the map loader (debug strings `map_name`, `map_read`, `man_set`, `camera_set`, `fog_set`, `tmds: %d`, `game_mode`, `program_mode`; calls the field asset loader `FUN_8001F7C0` and MAN decoder `FUN_8003AEB0`). `CONFIG INIT` doesn't initialise game config; it initialises the slot-machine debug mode. The engine-core `GameMode` enum in `crates/engine-core/src/mode.rs` shares these dev names; its docstrings now reflect the field-mode semantics.

#### New Game boot chain (title → field)

The title-screen NEW GAME selection is the entry point into modes 2/3:

1. **Title confirm.** In the title overlay tick (`FUN_801DD35C`), the menu handler reads the live cursor (`state[+0x1FC]`), and on `L1|Cross` (`pad & 0x44`) stashes the chosen row at `state[+0x200]`, then advances to sub-mode `0x14`. NEW GAME is row 0; a non-zero row (CONTINUE) routes to the save/card load path instead.
2. **Launch write.** The row-0 sub-phases reach `0x801DFC00`: `li v0,0x2; sh v0,-0x47C4(v1)` writes `_DAT_8007B83C = 2`, resets the title sub-mode, and kicks a fade-out (`FUN_80024EE4(1, 2, 0xFFFFFF)`). (The other master-mode writes in this tick set `0x1A` = STR/intro-FMV, the attract/demo path.)
3. **Mode-2 init.** The mode dispatcher runs `FUN_80025B64`: load the field overlay (`FUN_8003EBE4(2)`) → call `FUN_801D6704`.
4. **Field scene init.** `FUN_801D6704` reads the resident map id, loads geometry + MAN + camera + fog + BGM, allocates the game-mode work buffer, and writes `_DAT_8007B83C = 3` — the field per-frame loop ("MAIN MODE") takes over the next frame.

The mode-transition control flow is mirrored in `crates/engine-vm/src/title_overlay.rs` (`MASTER_GAME_MODE_FIELD_LAUNCH` = 2, `MASTER_GAME_MODE_FIELD_RUN` = 3, `FIELD_SCENE_INIT_PC`, `MENU_INDEX_NEW_GAME`) and `crates/engine-core/src/world.rs` (`World::begin_new_game`). The *fresh-state seed* a new game establishes (starting party stats, gold, starting scene id) is set by a separate, not-yet-pinned new-game-init routine; `FUN_801D6704` itself is generic field entry, used for every scene transition, and reads that state from globals rather than seeding it.

**The title screen is not one of the 28 modes** — its tick (`FUN_801DD35C`) is loaded by a pre-mode-dispatch boot routine, ahead of the mode table being consulted at all. NEW GAME is how control crosses from that title overlay into the mode table (at mode 2).

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
| `FUN_8003EBE4` / `FUN_8003EC70` | Parallel overlay loaders A/B (see Game-mode state machine section). Both: `prot_index = param + 0x381`. Differ only in destination buffer pointer (`*DAT_8001038C` vs `*DAT_80010390`) and current-id tracker (`gp+0x924` vs `gp+0x934`). |

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

The TIM at `PROT.DAT[0x11218..0x11218 + 33312]` (256×256 @ 4bpp + 16×16 CLUT bank) is a small-caps glyph atlas used by the in-game menu UI (shop / inventory / status panels). Confirmed by pinning the in-RAM copy at vaddr `0x80106478` (sstate8, live title-menu state) against PROT.DAT — byte-equal modulo the runtime CLUT relocation. The atlas does NOT appear in any extracted PROT entry; it's strictly in this pre-`init_data` gap.

| Glyph row  | Atlas Y    | Cell W | Cells | Content                                  |
|------------|------------|--------|-------|------------------------------------------|
| Digits     | 209..220   | 8      | 10    | `0123456789`                             |
| Alphabet   | 224..238   | 8      | 26    | `ABCDEFGHIJKLMNOPQRSTUVWXYZ`             |

Each cell is 8 px wide on a fixed 8 px pitch starting at `x = 8`. The atlas also carries non-glyph debug content (a `<DEMO>` row, the dev string `ここは常駐エフェクトが入る予定 / Pochi`, a `FONT CLUT` palette-bar indicator, and various cursor / arrow sprites) — all ignored by the engine.

CLUT row 0 renders the alphabet in solid red with magenta highlights; retail switches CLUT rows per context to read white / gold / dim. The clean-room engine sidesteps the CLUT-switching logic by decoding once to a stencil (pixel-index 0 → transparent, indices 1..15 → opaque white) and applying a `SpriteDraw::color` tint at draw time — see `crates/engine-core/src/menu_glyph_atlas.rs`.

**Note on title-screen "NEW GAME" / "CONTINUE":** The title menu rows are NOT rendered from this atlas — retail samples a pre-rendered band at `y=227..237` inside the title TIM itself (PROT 0888 / 0889 / 0890; see [`legaia_asset::title_pak::TITLE_BAND_MENU_NEW_GAME`] / [`TITLE_BAND_MENU_CONTINUE`]). The band carries both strings packed into a single 128×10 strip; the clean-room engine emits two `SpriteDraw`s sampling the left half (x=0..65) and right half (x=65..127) of that strip. Selection is colour-coded: bright/white for the cursor row, dim/gray otherwise — there's no arrow / cursor mark in retail.

#### Extraction

```rust
use legaia_asset::menu_glyph_atlas;
let prot_dat = std::fs::read("extracted/PROT.DAT")?;
let tim = menu_glyph_atlas::extract_from_prot_dat(&prot_dat)?;
// tim.bytes is the 33312-byte slice starting at PROT.DAT[0x11218].
```

The engine reads the slice directly via `ProtIndex::prot_dat_raw_bytes(byte_offset, len)` (added on top of the existing `entry_bytes` / `entry_bytes_extended` API).

#### Loader pathway (hypothesis)

These TIMs land in main RAM at vaddrs `0x80105000..0x80110200` (well below the `0x801C0000+` overlay window), which means they're treated as **shared static assets**, loaded once at boot before any overlay. The loader has not been pinned function-by-function yet; the most likely candidate is the same CD-DMA primitive (`FUN_8005D9A0`) that delivers the title overlay, driven from the SCUS-side bulk-initializer (`FUN_8005DA40` site). Confirming this requires a Write-breakpoint capture targeting the `0x80105000..0x80110200` range on cold boot, mirroring the title-overlay hunt in [`scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua`](../../scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua).

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

The SCUS boot sequence issues a multi-sector `ReadN` starting at PROT 899's LBA (47227) and reads ~74 sectors of contiguous on-disc data — crossing PROT 899's TOC-claimed end (47241) into the unindexed gap. The CD-DMA primitive (`FUN_8005D9A0`) breaks the read into 5 sequential DMA bursts:

| DMA burst | RAM dst | PROT.DAT source offset | Caller |
|---|---|---|---|
| 1 | `0x801CF818` | `0x5C3E800` (PROT 899 +0x1000, sec +2) | `pc=0x8005DA50, ra=0x8005C2D4` |
| 2 | `0x801D4818` | `0x5C43800` (PROT 899 +0x6000, sec +12) | same |
| 3 | `0x801D9818` | `0x5C48800` (gap +0x4000, sec +8) | same |
| 4 | `0x801DD018` | `0x5C4C000` (gap +0x7800, sec +15) | same |
| 5 | `0x801E4818` | `0x5C53800` (gap +0xF000, sec +30) | same |

Capture pipeline: [`scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua`](../../scripts/pcsx-redux/autorun_title_overlay_writer_hunt.lua) (cold-boot mode, `LEGAIA_NO_SSTATE=1`) arms Write breakpoints inside the overlay range and captures the DMA-driven writes — PCSX-Redux Lua Write BPs catch DMA writes from CD-DMA-channel-3.

#### Why the TOC misses it

The per-entry size formula `size_sectors = toc[p+5] - toc[p+3] + 4` (see [`docs/formats/prot.md`](../formats/prot.md) and [`crates/prot/src/archive.rs`](../../crates/prot/src/archive.rs)) gives 14 sectors for PROT 899, but the on-disc contiguous range between PROT 899 and PROT 900 is 74 sectors. The formula appears to describe an "indexed" subset of each entry's disc footprint, with trailing unindexed bytes carrying overlay code that the SCUS loader reads by passing an explicit larger sector count. The same pattern may apply to other entries — comparing each TOC slot's claimed size to the gap to the next entry would identify other hidden overlays.

#### Negative findings (corrects earlier notes)

- The historical claim "title overlay code is not in any PROT entry" was **methodologically** correct (it isn't in any **extracted PROT file**) but missed the disc-level reality: the bytes ARE in PROT.DAT, just outside the indexed entries.
- A lossy-LZS brute-force scan returned zero hits because the title overlay is **not compressed**; the CD-DMA primitive copies raw bytes straight into the overlay window.
- The "FUN_8005DA40 walks pointer table _DAT_800795B4" claim from earlier notes is unverified. `0x8005DA40` is an intra-function instruction inside `FUN_8005D9A0` (the CD-DMA-channel-3 read primitive) — Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx`. The actual DMA-driver site is `pc=0x8005DA50`.

The script VM that drives every running script is **not** in `SCUS_942.54` - it lives in RAM overlays at `0x801C0000+`. The actor / sprite VM (`FUN_801D6628`) is in the title-screen overlay; the field/event VM (`FUN_801DE840`) is in the town/field overlay; the effect VM cluster (`FUN_801DE914 / 801DFDF8 / 801E0088`) is in the battle overlay. See [actor VM](actor-vm.md), [field VM](script-vm.md), and [effect VM](effect-vm.md).

## Title-screen overlay state

The title-screen overlay loads into `0x801E0000+` during the boot sequence and keeps its mode state in a struct at `0x801EF018`. Known fields:

| Offset | Width | Field |
|---|---|---|
| `+0x154` | u32 | Title-attract idle countdown (`_DAT_801EF16C`). Initialized to `0x8000`; decremented per-frame by `_DAT_1F800393` (the global per-frame scalar - same byte used by `World::tick_move_vms_with_delta`); underflow writes the master game-mode index to `0x1A` (= STR FMV mode 26) and zeroes the FMV id at `_DAT_8007BA78` → `MV1.STR`. See [`cutscene.md`](cutscene.md). |
| `+0x158` | u32 | Title-overlay frame counter (`_DAT_801EF170`). Incremented unconditionally every tick. |

Initial values come from a SCUS-side bulk-initializer at `FUN_8005DA40` (called via `0x8005C2D4`) that walks a pointer table at `_DAT_800795B4` and writes initial values into multiple overlay BSS regions in one pass. The countdown's `0x8000` sentinel is set during this init pass, before the overlay's tick function starts running. The same initializer writes other addresses sharing a `…116C` low-half offset, suggesting `_DAT_800795B4` is a list of struct bases the init pass walks with a common per-struct displacement.

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

The JT, state-struct field offsets, and observed `state[+0x204] = N` transitions are pinned in [`legaia_engine_vm::title_overlay`](../../crates/engine-vm/src/title_overlay.rs). Four modes are semantically labelled: `Init` (`0x00` — entry init that routes to `Phase02` or `AttractDelay`), `Idle` (`0x01` — body-tail no-op), `AttractIdle` (`0x10` — Press-Start poll), `AttractDelay` (`0x11` — pre-attract delay). The other 21 carry `Phase0xNN` placeholders with traced-transition docstrings; the module's `STATE_204_WRITES` table holds the full graph. Notably, **Phase06 writes `_DAT_8007B83C = 0x02` at `0x801DFC00`** — the title-screen → main-game master-mode transition (exported as `MASTER_GAME_MODE_FIELD_LAUNCH` + `PHASE06_LAUNCH_GAME_PC`).

### Sprite-emit helpers

The title-tick body reaches into three SCUS-side helpers to emit GPU primitives. All three are ported clean-room in [`legaia_engine_vm::title_prim`](../../crates/engine-vm/src/title_prim.rs):

- `FUN_80058298` (`ClearImage` rect-fill queue, 37 instructions) → `exec_clear_image(host, rect, r, g, b)`.
- `FUN_80058490` (`MoveImage` VRAM-to-VRAM copy, 49 instructions) → `exec_move_image(host, src, dst_x, dst_y)`, with early-out on zero extent matching the original's `li v0, -1` path.
- `FUN_800198E0` (sprite-descriptor dispatcher, 146 instructions) → `exec_sprite_descriptor(host, &SpriteDescriptor)`, with full tag-`0x11` simple variant + complex variant routing (alpha-OR pre-pass under `flags & 8`, four width-divisor variants from `flags & 3`).

`SpriteDescriptor { tag, flags, rect, pixel_data_ptr }` and `Rect12 { x, y, w, h }` capture the wire shapes. The `PrimHost` trait abstracts the four engine-side callbacks (`queue_clear_rect`, `queue_move_image`, `emit_sprite`, `alpha_or_gate_set`); engines wire those to a real GPU back-end. The overlay-side helpers (`FUN_801E1C1C` / `FUN_801E373C` / `FUN_801E3EE0` / `FUN_801E36C4`, each ~8 KiB, shared across menu / battle / shop / save UI overlays) are deferred to their own focused port — the title-tick body's calls into them can be stubbed against the same `PrimHost`.

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

The 256-KiB `overlay_title.bin` window does carry two real TIMs embedded in the title overlay's data segment, at the same addresses the tick body's `FUN_800198E0` sprite-descriptor calls reference: `0x801E5120` (256×256 4bpp save-menu UI atlas — memcard icons + Japanese strings) and `0x801EE120` (256×16 4bpp animated PSX memcard icon strip, 14 frames). Both byte-match `extracted/PROT/0899_xxx_dat.BIN` at file offsets `0x16908` / `0x1F908` (i.e. they live in the trailing-overlay portion of PROT 899). A reusable scanner at [`scripts/scan_tims_and_match_prot.py`](../../scripts/scan_tims_and_match_prot.py) walks a PSX main-RAM dump for TIM-magic records and byte-greps the PROT corpus to pin each candidate.

The **main title-screen art** itself (Legend of Legaia wordmark, orb, `PRESS START BUTTON`, `NEW GAME` / `CONTINUE` menu, copyright lines) lives outside the title-overlay window — it's loaded into main RAM at `0x80170DF8` and sourced from **PROT 0888** (CDNAME label `sound_data2`; the multi-bank sound-data cluster carries title art in the trailing pool past the audio payload). Duplicate copies live in PROT 0889 and 0890 at slightly different file offsets:

```text
PROT 0888 @ 0x1AA28    — 256×256 8bpp, 66 080 bytes — PRIMARY
PROT 0889 @ 0x19A28    — same content (multi-bank dup)
PROT 0890 @ 0x14228    — same content (multi-bank dup)
```

The 256×256 image is a **sprite sheet** that bundles every text band the title screen *could* draw — retail composes the screen by sampling specific sub-rects rather than blitting the full quad. The bands, top to bottom in source-y, are:

| Source rect (`x, y, w, h`) | Content | Drawn when |
|---|---|---|
| `(0, 17, 256, 124)` | Orb + "Legend of Legaia" wordmark | every post-fade phase |
| `(96, 151, 64, 10)` | `<DEMO>` | **never** — demo-build leftover |
| `(60, 178, 196, 16)` | "PRESS START BUTTON" prompt | PressStart phase only |
| `(4, 195, 244, 14)` | "TM of Sony..." copyright | every post-fade phase |
| `(8, 209, 234, 14)` | "© 1998,1999..." copyright | every post-fade phase |
| `(0, 226, 256, 11)` | small "NEW GAME CONTINUE" footer | replaced by larger font glyphs |

The `<DEMO>` band is a residual from a development demo build that retail simply never samples — verified by capturing main RAM at the live title screen (sstate8, sub-mode `0x10` AttractIdle) and confirming the in-RAM TIM bytes byte-match PROT 0888 while the live framebuffer omits the band. The small footer "NEW GAME CONTINUE" is similarly never drawn; retail renders the menu labels using the dialog-font glyph atlas instead (which is why the on-screen "NEW GAME / CONTINUE" letters are visibly larger than the embedded footer text).

A typed parser lives at [`legaia_asset::title_pak`](../../crates/asset/src/title_pak.rs) — `extract_title_tim(&prot_0888_bytes, TITLE_TIM_OFFSET)` returns a zero-copy slice + decoded VRAM rects, and the band-rect constants `TITLE_BAND_WORDMARK` / `TITLE_BAND_PRESS_START` / `TITLE_BAND_TM_COPYRIGHT` / `TITLE_BAND_C_COPYRIGHT` (plus `TITLE_BAND_DEMO` for reference) pin the sub-rects listed above. The disc-gated unit test (`extracts_real_title_tim_when_disc_extracted`) locks the on-disc layout. An engine-side RGBA decoder lives at [`legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888`](../../crates/engine-core/src/title_screen_atlas.rs); the play-window subcommand uploads it as a sprite atlas and emits one [`SpriteDraw`] per active band each frame (`title_screen_sprite_draws` in [`legaia-engine`](../../crates/engine-shell/src/bin/legaia-engine.rs)), with the press-start band gated on phase. The font-rendered "PRESS START" overlay is suppressed via the `atlas_present` flag on `title_draws_for` so the TIM band isn't duplicated.

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

A **town/field subsystem** uses a separate format-string pool at `0x80011079..0x80011109` (`"    town "`, `"mode %d"`, `"    baria mode "`, `"    walking set"`, `"end of mes works set"`, `"open port.dat"`, `"nt_group_table %x"`). These print at retail-build runtime but have no LUI+ADDIU caller resident until the town/field overlay is loaded — i.e. the "mode 17 / mode 16" runtime printfs are *town-subsystem* mode transitions, not the master 28-mode state machine index.

## Boot init.pak (PROT 0895)

PROT entry `0895_bat_back_dat` is the **boot-time `init.pak` bundle** — despite the misleading CDNAME label. The first 16 bytes are a small pack header; the rest is a string pool followed by four uncompressed PSX TIMs:

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
+0x21c4  TIM  PROKION         (8bpp, 176×256, ~45.6 KB) — boot logo
+0xd3e4  TIM  Contrail        (8bpp, 184×256, ~47.6 KB) — "A Contrail Production"
+0x18e04 TIM  SCEA Presents   (4bpp, 256×128, ~16.4 KB)
+0x1ce44 TIM  WARNING         (4bpp, 256×256, ~32.8 KB) — health warning
```

CLUT and pixel data are byte-identical to live RAM after boot extraction — only the RECT fields (VRAM target coords) are runtime-relocated. On-disc each TIM has CLUT `fb=(0, 480+N)` and pixel `fb=(640..800, 0..256)`; the boot loader rewrites these to per-logo VRAM regions before calling LoadImage.

A typed parser lives at [`legaia_asset::init_pak`](../../crates/asset/src/init_pak.rs) — call `parse(&prot_0895_bytes)` to get a struct view over the four logos (slice pointers + decoded VRAM rects). The disc-gated unit test (`parses_real_init_pak_when_disc_extracted`) locks the on-disc layout.

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

The actual on-screen layout the retail boot code uses still has to be RE'd from the unlocated title-overlay tick body — the `STRIP_GRID` constants are hypothesis-fit-to-visible-content, not pinned to specific GPU draw commands.

The `h:\prot\field\title\title.pak` string is **only a debug-print referent** — the title-screen content lives in **PROT 0888** (`sound_data2` per CDNAME, see the title-overlay-state section above) referenced by integer constant from SCUS boot code, not by string lookup. SCUS does not contain the literal string `title.pak` anywhere. The mismatch between the debug path and the actual PROT entry is the same pattern as PROT 0895 being labelled `bat_back_dat` while actually carrying `init.pak`: CDNAME labels are misleading for several entries, so always cross-validate against the loader-call constant or the file's magic bytes.

The TIM-upload helper for these (and for the title overlay's per-frame sprites) is `FUN_800198E0` — it consumes a packed struct with custom magic `0x11` OR a real PSX TIM (flags bit 3 = "has CLUT"), and dispatches to `FUN_800583C8` (the `LoadImage` wrapper, identified by the literal string `s_LoadImage_800156d4` it references for debug logging).

## Debug flags

- `_DAT_8007B8C2` - dev/retail build toggle. Several subsystems (sound init, field loader) carry an "if dev" branch keyed on this byte. No writers exist in `SCUS_942.54`; the writer must live in an unswept overlay or come from external POKE (TCRF GameShark codes confirm both this flag and `_DAT_8007B98F` are runtime-writable).
- `_DAT_8007B98F` - separate debug-mode flag (NA build offset; JP retail uses `0x07D51F`, an `0x1B90` build-shift).

The input dispatcher `FUN_8001822C` reads these flags but doesn't write them; the writer is downstream of one of the option-menu / cheat-menu overlays (`0896` or similar).
