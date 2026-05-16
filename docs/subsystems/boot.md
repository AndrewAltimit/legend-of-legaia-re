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
| 2 `MAIN INIT` | `FUN_80025B64` | `FUN_8003EBE4(2)` | 899 | Options/config menu — "Display Off / Vibration On / Voices On" strings + 27 MIPS prologues. **The dev label "MAIN INIT" refers to the OPTIONS MENU**, not the title screen. |
| 24 `OTHER INIT` | `FUN_80025980` | ? | 896 | Mode-24 OTHER overlay (cited by `dump_round8.py`'s `OVERLAY_0896_TARGETS`) — **not** "battle background" despite CDNAME `bat_back_dat`. |
| 26 `STR INIT` | `FUN_80025FB4` | (FMV path) | — | Cutscene / STR FMV mode entry. Title-overlay tick writes `_DAT_8007B83C = 0x1A` (= 26) on attract underflow → enters this mode. |

**The dev mode-names mislead.** `MAIN INIT` doesn't initialise the title screen; it initialises the options menu (likely so-named because options is the "main game-options" screen in dev parlance). `CONFIG INIT` doesn't initialise game config; it initialises the slot-machine debug mode. The engine-core `GameMode` enum in `crates/engine-core/src/mode.rs` shares these dev names but its implied semantics (e.g., comment "Mode 2 - main init (boot to title screen)") are wrong — verify each variant's docstring against retail behaviour before relying on it.

**The title screen does not appear to be one of the 28 modes** — every mode whose init handler we've traced loads a debug/test/menu overlay or a major game mode (field/battle/cutscene), none of which match the title-overlay tick at `FUN_801DD35C`. The title overlay is loaded by a pre-mode-dispatch boot routine, ahead of the mode table being consulted at all.

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

### Title-overlay source PROT (open)

The title-overlay code (function `FUN_801DD35C` at `0x801DD35C`, the captured `overlay_title.bin` 256-KiB window) is **LZS-compressed on disc** - byte-search for `0x801D6704`'s first 32 bytes finds no match across the 1232 uncompressed PROT entries. None of the named mode handlers (`MAIN MODE`, `CONFIG MODE`, `MONSTER MODE`, `TMD MODE`, `EFECT MODE`, `MEM TEST`) load it directly. Two candidate paths for next-session tracing:

- **Pre-mode-dispatch boot path.** PROT 0895 (`init.pak`) gets loaded by some routine before the mode-table starts dispatching - the same routine probably loads the title overlay immediately afterwards. The PROT-895 loader doesn't use the `+0x381` offset (895 = 0x37F, 0x37F − 0x381 = −2), so a different constant or a different loader API is in play.
- **The "FUN_8005DA40 walks pointer table _DAT_800795B4" memory claim is unverified.** No SCUS dump references `_DAT_800795B4`; `FUN_8005DA40` itself is not a real function entry - the address `0x8005DA40` is an intra-function instruction inside `FUN_8005D9A0` (the CD-DMA-read primitive that triggers DMA channel 3). Per CLAUDE.md's "Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx`" caveat, this label is a mis-attribution.

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

The captured `overlay_title.bin` does NOT contain raw TIM-magic bytes - the title-screen TIM data is either uploaded to VRAM at an earlier boot phase and freed from main RAM, or it lives in a separately-mapped region. The `FUN_800198E0` "draw a sprite descriptor" calls from the tick body pass two in-overlay template addresses (`0x801E5120` and `0x801EE120`) whose payloads encode TPAGE/CLUT coords referencing data already in VRAM.

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

The `h:\prot\field\title\title.pak` string is **only a debug-print referent** — the title-screen content lives in a separate PROT entry referenced by integer constant from SCUS boot code, not by string lookup. SCUS does not contain the literal string `title.pak` anywhere.

The TIM-upload helper for these (and for the title overlay's per-frame sprites) is `FUN_800198E0` — it consumes a packed struct with custom magic `0x11` OR a real PSX TIM (flags bit 3 = "has CLUT"), and dispatches to `FUN_800583C8` (the `LoadImage` wrapper, identified by the literal string `s_LoadImage_800156d4` it references for debug logging).

## Debug flags

- `_DAT_8007B8C2` - dev/retail build toggle. Several subsystems (sound init, field loader) carry an "if dev" branch keyed on this byte. No writers exist in `SCUS_942.54`; the writer must live in an unswept overlay or come from external POKE (TCRF GameShark codes confirm both this flag and `_DAT_8007B98F` are runtime-writable).
- `_DAT_8007B98F` - separate debug-mode flag (NA build offset; JP retail uses `0x07D51F`, an `0x1B90` build-shift).

The input dispatcher `FUN_8001822C` reads these flags but doesn't write them; the writer is downstream of one of the option-menu / cheat-menu overlays (`0896` or similar).
