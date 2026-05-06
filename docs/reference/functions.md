# Key function directory

A directory of the Ghidra-traced functions that matter for understanding Legaia's runtime. Each entry has a Ghidra-dump path under `ghidra/scripts/funcs/` (read it for the canonical disassembly + decompiled C). Functions in `0x801C0000+` are RAM-loaded overlays and are dumped under `overlay_<label>_<addr>.txt`.

## Asset loading + dispatch

| Address | Role |
|---|---|
| `8001A55C` | LZS decoder. Algorithm reverse-engineered from this function. |
| `8001A8B0` | Raw memcpy. Used by the asset dispatcher when `copy_only = 1`. |
| `8001E1B4` | Per-stage init. Allocates the 0x62C00-byte asset buffer at `_DAT_8007B85C`. |
| `8001F05C` | Asset-type dispatcher. The `(type_size, copy_only)` switch — see [`formats/asset-type.md`](../formats/asset-type.md). |
| `80020224` | Descriptor-pair walker. No static caller in `SCUS_942.54`; called at runtime from the town overlay's `FUN_801D6704`. |
| `80020454` / `80020DE0` | Actor allocator pair. Free-list LIFO at `_DAT_8007C348`, 512-pointer pool at `0x8007C370`. |

## Per-stage asset table machinery

| Address | Role |
|---|---|
| `8002541C` | Streaming-asset driver. Top-level for types 0xA (tim.dat) / 0xF (move.mdt) / 0x14 (DATA_FIELD). |
| `800255B8` | Filename builder + loader. Builds `h:\PROT\FIELD\<stage>\tim.dat`-style paths or falls through to by-index. |
| `800268DC` | TMD pointer fixup. Patches object-table pointers from offsets to absolute addresses. |
| `80026B4C` | TMD register. Validates `id == 0x80000002`, stores in TMD pointer table at `0x8007C018 + idx*4`. |

## Disc / loader chain

| Address | Role |
|---|---|
| `8003E4E8` | Boot-time TOC loader. Reads first 3 sectors of `PROT.DAT` into `0x801C70F0`. |
| `8003E6BC` | Path-based opener. Resolves dev paths into a PROT index via the CDNAME-driven name map. |
| `8003E800` | Low-level CD setup. Stores destination pointer + size, calls `FUN_8003F128`. |
| `8003E8A8` | LBA resolver. Reads in-RAM TOC at `0x801C70F0`; computes `(start_lba, size_sectors)`. |
| `8003EB98` | By-index loader. Wrapper around `FUN_8003E8A8` + `FUN_8003E800`. |
| `8003F128` | Async CD read kickoff. Stages parameters, submits to BIOS-level CD library. |
| `8005C42C` | BCD-MSF → LBA converter. `((m*60 + s)*75 + f) - 150`. |

## PSX runtime / standard libraries

Statically-linked PsyQ glue. Trivial to stub in a clean-room port.

| Address | Role |
|---|---|
| `80056678` | `EnterCriticalSection` — `syscall(0)` with `$a0=1`. |
| `80056688` | `ExitCriticalSection` — `syscall(0)` with `$a0=2`. |
| `80056738` / `80056748` / `80056768` / `80057014` / `8005ACE8` / `8005ACD8` / `8005BBE8` / `8005FD68` / `8005FD78` | `jr 0xA0/0xB0/0xC0` — PSX BIOS table dispatchers (libapi). Identified targets: `80056748` = `strncmp` (A0 0x18); `80056768` = `strlen` (A0 0x1B); `80057014` = `rand` (A0 0x2E); `8005ACD8` = `GPU_cw` (A0 0x49); `8005BBE8` = `FlushCache` (A0 0x44); `8005FD68` = B0 0x5B (card init); `8005FD78` = `ChangeClearRCnt` (C0 0x0A). |
| `80056658` | `TestEvent` BIOS thunk — `jr 0xB0` with `t1=0x0B`. Polls a kernel event handle. |
| `8006B844` | `WaitEvent` BIOS thunk — `jr 0xB0` with `t1=0x0A`. Blocks on a kernel event handle. |
| `80056698` / `800566A8` / `800566B8` / `800566C8` / `800566D8` / `800566E8` / `800566F8` / `80056708` / `80056718` | Byte-identical `li t2,0xB0; jr t2` BIOS B-vector thunks emitted by the linker once per caller. The selected B-routine is determined by `$t1` set up by the caller, not by the thunk. Same pattern at `8006EE14` / `8006EE24` / `8006EE34` (B0-vector cluster cited from menu/text helpers). |
| `800567B8` / `80056B18` | `printf`-class formatter (handles `%d %x %o %s %f`); writes into a static buffer. |
| `80057024` | `memmove` — overlap-safe direction-aware copy. |
| `8005ACAC` | `memset`. |
| `8005E540` | `memcpy` — forward-only byte copy. |
| `8005FF04` | `irq_mask_swap16` — atomic-swap on `_DAT_8007A868` (16-bit IRQ mask). Used by `FUN_8005A78C` to gate SIO setup. |
| `8005FCCC` | VSync wait spin (frame-budget gated). |
| `80060A04` | `BREAK 0x105` — debug trap. |
| `800608E0` / `8006xxxx` | libapi `fopen` / `fseek` / `fread` / `fclose` cluster. |
| `8005FD88` | libapi device-vtable trampoline (slot `+0xC` of `PTR_PTR_8007A860`). |

### libgte primitives

| Address | Role |
|---|---|
| `8005B7C0` | `SetRotMatrix`-shaped — `setCopControlWord(2, 0xD800, x)`. |
| `8005B7CC` | `SetTransMatrix`-shaped — `setCopControlWord(2, 0xE000, x)`. |
| `8005BA1C` | GTE square-root / normalize — `mtc2 0xF000 / mfc2 0xF800`. |
| `8005BB48` | `InitGeom` — saves return addr to `_DAT_8007BDF0`, `EnterCriticalSection`, copies exception-handler table from `DAT_8005BBB0`, `FlushCache`. |
| `8005AF0C` | `isqrt`-style normalise — uses `FUN_8005BA1C` (GTE normalize) then dispatches to `FUN_8005ADB8` with shift correction. |
| `8005ADB8` | Fixed-point bit-rotation / arc helper — consumed by `FUN_8005AF0C`. 85-instr ladder of conditional shifts. |

### libcd primitives

| Address | Role |
|---|---|
| `8005CCB4` | `CdSync` — 179-instr poll loop, spins on `FUN_8005C4AC` using timeout in `_DAT_801CADF8` (~16 ms). Strings: `"CD ready"`, `"CD timeout"`. |
| `8005D9A0` | `CdControl_raw` — writes CD MMIO regs at `_DAT_80079670/67C/6A4/680/6A8/6AC/6B0/6B4` with command bytes. Spins waiting bit `0x40` then `0x1000000`. |
| `8005C2C4` | `CdDiskReady` — wraps `FUN_8005D9A0`, returns `rc == 0`. |
| `8005A78C` | Pad / SIO init — touches `_DAT_80078E28/E34/E44` (SIO MMIO), wraps with `FUN_8005FF04` (IRQ disable), clears 256 B at `0x801C948C` and 6 KB at `0x801C9590` (pad RX/TX buffers). |
| `8005ABD0` | Pad-protocol-phase handshake — bitfield writes `0xE1001000 / 0x20000504 / 0x10000007` on `_DAT_80078E24/E28`. SIO digital-pad protocol selector, returns state code 0..4. |

## CD / file-system (libcd-style)

Used by the sound subsystem's dev branch and elsewhere when retail-async CD reads aren't appropriate. Stack ordering: SCUS path-opener (`0x8003E6BC`) → libcd file-system (`0x8005DEA0`) → libcd primitives (`0x8005CCB4 / D9A0 / E4D4`) → libapi device-vtable (`0x8005FD88`) → BIOS A0 / B0 / C0 traps.

| Address | Role |
|---|---|
| `8005DEA0` | Directory parser — reads the active sector, caches up to 128 entries into `0x801C4BEC`. |
| `8005E180` | Directory-entry lookup by ID; returns slot index or `-1`. |
| `8005E228` | File loader — reads sectors from a directory entry into a cache buffer. |
| `8005E4D4` | High-level open-then-read helper: `(buf, dir_entry, size)` → `bool`. Calls `FUN_8005C328` to build the CdlLOC, `FUN_8005BEFC(2,…)` to issue `CdlSetloc`, `FUN_8005E9A4` to set sector size `0x80` (2048 vs 2336). |
| `8005E574` | CD sector-reader state machine — handles block reads, timeout, completion callback at `_DAT_800796C0`. |
| `8005C42C` | BCD-MSF → LBA: `((m*60 + s)*75 + f) - 150`. |

## Helpers

| Address | Role |
|---|---|
| `80017888` | Malloc — the general-purpose allocator. |
| `8003C5F0` | Generic ramp scheduler. 64-slot pool at `0x801C66A0` (stride 0x20). Used for sound + render-bank ramps. |
| `8003C83C` | Script-context resolver. Special-cases `id == 0xF8` (returns cached pointer) and `id == 0xFB` (system channel). |
| `80036044` | Text-width measure for inline dialog/UI strings. Walks a byte stream: `>= 0x1F` = glyph (count 1); `0xC0..0xC7` = escape (substitutes from inventory / magic / item-name tables — `0xC1` = item-name @ `0x80084549 + idx*0x414`, `0xC2` = `PTR_DAT_8007436C[idx*3]`, `0xC3` = magic name @ `PTR_s_Magic_800754D0`, `0xC7` = `DAT_80073F24 + idx*8`); `0xCE` = newline (line++); `0xCF` = end-of-row. Returns total glyph count. |
| `8003CC98` | Single-line text render-and-measure. `FUN_80036044(buf)` for length + `FUN_80036888(buf, palette, 0, x, y)` to draw, returns the length. |
| `8003CD00` | Multi-line text layout. Walks a string line by line: measure with `FUN_80036044`, draw with `FUN_80036888`, advance Y by `0x0D` per line. Stops on the first sub-`0x20` control byte. Returns max line width. |
| `8003CE08` / `CE34` / `CE64` | SET / CLEAR / TEST against the **fourth flag bank** (256-bit bitfield at `DAT_80086D70`). Wired by field-VM opcodes 0x50 / 0x60 / 0x70. |
| `8003CE9C` | Signed-16-bit operand decoder (sign-extended `s16` from two bytes). |
| `8003CEB8` | 24-bit LE decoder. Reads 3 bytes as a u24. |
| `8003CED8` | 32-bit LE decoder. Reads 4 bytes as a u32. |

## Input + debug subsystem

| Address | Role |
|---|---|
| `8001822C` | Per-frame input handler / debug dispatcher. Reads BIOS pad at `0x800840F8`, builds button mask `_DAT_8007B850`. Gates upper 16 bits and all debug bindings on `_DAT_8007B98C != 0`. |
| `80016230` | Dev-print driver. Loads `program_no=%d` / `..\..\FIELD\PROGRAM\....\%d` strings only when debug enable is non-zero. |

## Move / animation subsystem

| Address | Role |
|---|---|
| `800204F8` | Move-buffer consumer. Sole reader of both `_DAT_8007B888` (MOVE) and `_DAT_8007B840` (MOVE2). Resolves `move_id` to a buffer record and stages it onto the actor — does **not** run opcodes itself; that's `FUN_80023070`. |
| `80020740` | Move-buffer pre-tick helper. Called from `FUN_800204F8` when actor flag bit `0x1000` is set. |
| `80023070` | **Move-table opcode interpreter.** 71 opcodes (`0x00..0x46`); JT at `0x80010778`. Walks the per-actor move buffer at `actor[+0x48]` indexed by PC at `actor[+0x70]` (u16 units). Opcode `0x2F` escapes to `FUN_801D362C`. See [`subsystems/move-vm.md`](../subsystems/move-vm.md). |
| `8003774C` | **Per-actor motion / path-stepping VM** (a third dispatcher distinct from the actor VM at `FUN_801D6628` and the field VM at `FUN_801DE840`). 577 instructions; switch on `cmd & 0x7F`: cases `0x37/0x41` (linear delta on `actor[+0x14]/[+0x18]` from tables `_DAT_80073F14/0x80073F04`), `0x38` (bearing ramp), `0x47` (XZ approach with quadrant select), `0x4C` (line-of-sight to target actor — resolves `0xF8/0xFB` system channels via `_DAT_8007C34C/8007C354/8007C364` actor lists, same idiom as `FUN_8003C83C`). Reads `_DAT_1F800393` (frame dt). Drives `actor[+0x05/+0x06/+0x15/+0x26]` (X / Z / step-counter / facing). Likely the per-actor pursue / patrol / face-target subroutine called by the actor or move VM. |
| `80021B04` | Actor-spawn helper. Builds per-actor OBJECT pointer table at `actor[0x44]+4`. Calls `FUN_80023070` once at spawn to run the initialisation opcodes in the move buffer. |
| `80021DF4` | Per-frame actor tick. Updates `actor[+0x54]` (wait timer), `+0x22` (rotation), state-2/5/6 animation slots; then calls `FUN_80023070` to step the move VM. |
| `801D362C` | Move-VM overlay extension (town overlay). 61 sub-opcodes (`0x00..0x3C`); JT at `0x801CE868`. Reached only via move-VM opcode `0x2F`. Sub-handlers include `0x801D31B0`, `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`. |

## Game-mode state machine

| Address | Role |
|---|---|
| `0x8007078C` (data) | Mode table — 28 entries × 24 bytes. `+0x00` = name string ptr; `+0x0A` = next-mode i16; `+0x10` = handler fn ptr; `+0x14` = parameter. |
| `gp[0x524]` (data) | Current-mode register (i16). |
| `800179C0` | Dev mode-transition writer. Reads input mask, advances current mode. Gated on `_DAT_8007B98C != 0`. |
| `80025EEC` | Default per-mode handler — used by 13 of 28 modes. Pipeline: `FUN_8001698C → FUN_80016444(1) → FUN_80016B6C`. |
| `80025C68` | Mode 0 (CONFIG INIT) handler. |
| `80025B64` | Mode 2 (MAIN INIT) handler — field/script-runtime init. |
| `80025DA0` | Mode 12 (MAPDSIP MODE INIT) handler — field-display init. |
| `80025F2C` | Mode 13 (MAPDSIP MODE) handler — field-display per-frame. |
| `80025E68` | Mode 8 (EFECT INIT) handler. |
| `8001DCF8` | Boot-time mode initializer. 1212-byte function. NOT the per-frame dispatcher. |

## Battle subsystem

| Address | Role |
|---|---|
| `80052FA0` / `800542C8` | Battle archive loaders (SCUS). The archive walk uses `FUN_800536BC` (record copy + offset fixup, stride `0x1C`), `FUN_80053898` (bubble sort), `FUN_80053B9C` (UI-buffer fan-out at `+0x894 + slot*0x1E0`). |
| `800520F0` | Battle scene loader (SCUS). 11-case state machine; case 0xA loads etmd.dat, case 0xB loads vdf.dat, case 0xC loads efect.dat into `_DAT_8007BD5C`, case 0xE calls effect-bundle init `0x801DE914(0x1000, 0xA00)`. |
| `0x801C9370` (data) | **Battle actor pointer table** — 8 entries × 4 bytes. Slots 0..2 = party, 3..7 = monsters. Resolved by `FUN_8004E2F0` and `FUN_80054CB0`. |
| `0x80074358..0x80074368` (data) | Global 4×u32 "active abilities" bitmask. `FUN_80042558` ORs each party member's `+0xF4..0x100` block into it every frame. |
| `800431D0` | Global ability bit-test: `(bit_id) -> bool`. The read-side primitive for the bitmask above — `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. Cited heavily across battle code. |
| `800349EC` | HP / threshold UI classifier: `(char_idx) -> color_idx`. Reads `[char_base + 0x0E]` (current HP) and `[char_base + 0x0C]` (max HP), returns `2`/`6`/`7`/`9` keyed on dead / quarter / half / healthy thresholds. Drives dialog HP-color tinting. |
| `80035EA8` | MP-side variant of `FUN_800349EC`. Reads `[char_base + 0x10]` / `[char_base + 0x12]`. |
| `8003FB10` | Action / ability validator. Sub-dispatches on `actor[+0x9A8]` byte; checks the global ability bitmask (`FUN_800431D0`) and per-actor flag bits to decide whether a queued action can proceed. |
| `0x80084708` (data) | Character record table base. Stride `0x414` per character. See [`subsystems/battle.md`](../subsystems/battle.md) → "Character record layout". |
| `80042558` | Per-frame stat aggregator. Walks the 3 active party members, caps stats at `0x3E7`, OR-aggregates `+0xF4..0x100` ability flags into the global bitmask. Calls `FUN_800432BC` / `FUN_80042DBC` to maintain the active-spell slot list at `[char + 0x2B0..]`. |
| `80043048` | Status-effect timer decrementer: `(idx, decrement, default)`. Walks a stride-2 table at `_DAT_80085958` — byte 0 = active flag, byte 1 = countdown value. Bounds-checked against `gp[+0x2D4]`. Decrements the value, clamps at zero, clears the flag when the value reaches zero. Cited from field-VM-region helpers `FUN_801D71F0` / `FUN_801D7210` (the "actor poison/sleep/buff timer tick" path). |
| `800431FC` | Spell-list contains check: `(char_idx, spell_id) -> bool`. Walks `[char + 0x13d ..]` (count at `+0x13c`). |
| `80043264` | Equipment-slot contains check: `(char_idx, item_id) -> bool`. Walks `[char + 0x196 ..]` (8 slots). |
| `800432BC` | Spell-list insert (sister of `FUN_80042DBC` which removes). |
| `8004E2F0` | Battle range / line-of-sight: `(actor_a_id, actor_b_id) -> i16 distance`. Reads `[0x801C9370 + id*4]` for both, sums `+0x1F` size bytes, clamps per-tier. |
| `80054CB0` | Monster init: `(record, monster_slot)` populates `[0x801C9370 + (slot+3)*4]` from a monster record (HP/MP/stats/magic-resist + XP at `+0x230`). |
| `80055468` | Battle damage-number renderer. Calls `FUN_800583C8` to push sprite primitives keyed on `_DAT_8007BD24+0x13` (active-character index). |
| `80055B4C` | Battle character display flag. Writes `_DAT_8007BD24+0x26B = char + 1`, `+0x26C = 0`. |
| `80050E2C` | Generic linear pointer-table search: `(table, char_id, count) -> idx_or_0xFF`. Used by battle UI lookups. |
| `801D0748` | Battle main dispatcher (battle overlay). 11 KB / 182 calls. |
| `801D8DE8` | Hottest battle utility (battle overlay). 3 KB / 77 incoming refs. |
| `801E295C` | **Battle action state machine** — `ctx[7]` dispatch, `+0x1DE` sub-state. 16 KB / 4099 instructions / 155 outgoing calls (the largest function in the battle overlay). Outer switch on `_DAT_8007BD24[7]` (the action-state cursor; 47 cases across bands `0x14`/`0x28`/`0x32`/`0x3C`/`0x46`/`0x50`/`0x5A`/`0x64`/`0x68`/`0x6E`); inner switch on `actor[+0x1DE]` (action category 0..5 = Martial-arts / Item / Magic / Attack / Spirit / Run). Reads battle actor pointers via `(&DAT_801C9370)[ctx[0x13]]`; ramps frame-timer at `ctx[+0x6D8]`; queues animations via `actor[+0x1DA]` and waits on `actor[+0x1D9]` to converge. Battle-end signalled via `DAT_8007BD71 = 0xFE`. Cross-refs: `FUN_8004E2F0` (range/LOS, called from `0x14`/`0x16`/`0x19`), `FUN_80042558` ability bitmask (read indirectly via character record at `0x80084708 + (party_id-1)*0x414`), effect spawn via `FUN_801D8DE8` → `FUN_801DBF9C` → `FUN_801DFDF8`, pose driver `FUN_801D5854(actor, pose_id)` (~30 call sites). See [`subsystems/battle-action.md`](../subsystems/battle-action.md). Captured from the mc8 (action-menu-open) save state as `overlay_battle_action_801e295c.txt`. |
| `801DE914` | Effect-bundle init / pack-fixup (battle overlay). |
| `801DFDF8` | Effect-bundle public spawn API (battle overlay): `(byte effect_id, short* world_pos, ushort angle)`. |
| `801E0088` | Effect-bundle per-frame walker (battle overlay). |
| `801F17F8` | `summon.dat` / `readef.dat` streaming loader (battle overlay). |

## Script VMs

| Address | Role |
|---|---|
| `801D6628` | Actor / sprite VM (title-screen overlay). 13 opcodes; dispatch table at `0x801CED70`. |
| `801D6704` | Town overlay MAIN INIT. Calls `FUN_80020224` at `0x801D6B0C`. |
| `801CF650` | Emitter ramp-actor allocator (town overlay). Calls `FUN_80020DE0(base + 0x27EC)`, configures the actor's curve / ramp slots: `+0x50 = sub_id`, `+0x6C = mode_byte`, `+0x80 / +0x8C = curve_table[curve_id] << 16` (table at `_DAT_1F80035C`), `+0x84 = (target << 17) / (duration + 1)`, `+0x88 = abs / duration`. Shared helper used by 0x43 sub-0x10 / sub-0x12 emitter setup ops. |
| `801DB7B0` | Generic 4-byte jump-table dispatcher (town overlay). 7 instructions: `(*(table[v1])(...))()` where table base = `v0 - 0xD6C`. Caller sets `v0` (lui-immediate) and `v1` (index). |
| `801DE840` | **Field / event script VM** (town/field overlay). 17.5 KB / 357 outgoing calls. The largest function in the corpus. See [`subsystems/script-vm.md`](../subsystems/script-vm.md). |
| `801E00F4` | Field-VM dispatcher switch table. |
| `801F5748` | Inventory / menu hub (town overlay). 11 KB / 192 calls. |
| `801EAD98` | Field subsystem hub (town overlay). 5.9 KB / 35 calls. |
| `801ED710` | MES dialog renderer (town overlay). 1.9 KB / 44 calls. |
| `801EF2B0` | Controller input handler (town overlay). 1.9 KB / 29 calls. |
| `801DD35C` | Top-level menu dispatcher (menu overlay). 12 KB / 3026 instructions / 134 outgoing calls. Sets text-actor depth slot `_DAT_8007B454 = 7` and `DAT_80073F20 = 0x10`; reads active-submenu index from `_DAT_8007BAB4`. Loaded only when the in-game item / magic / equip menu is open — captured via the mc5 save state as `overlay_menu_801dd35c.txt`. |
| `801D33D8` | Submenu rendering loop (menu overlay). 5.3 KB / 107 outgoing calls. |
| `801E1C1C` | Shared menu-element renderer (menu overlay). 4.5 KB / 8 incoming refs. |

## Renderer

| Address | Role |
|---|---|
| `8002735C` | Legaia TMD renderer. 60 GTE ops; per-mode descriptor table at `DAT_8007326C`. |
| `800460AC` | GTE billboard fan helper. Loads 3 vertices via SVTX0/1/2 with `(X-0x20, Y, Z), (X, Y, Z), (X+0x20, Y, Z)`, runs RTPT (cop opcode `0x280030`) 3 iterations decreasing Z, stores SXY/SZ at scratchpad `0x1F8002FC..`. Stage decoration / billboard sprite projection. |
| `0x8007326C` (data) | Per-prim-mode descriptor table. 6 entries × 8 bytes — see [`formats/tmd.md`](../formats/tmd.md). |
| `0x8007C018` (data) | TMD pointer table. Written by `FUN_80026B4C`; read by 4 setup-not-render functions. |
| `80021B04` | Actor-spawn helper. Builds per-actor OBJECT pointer table at `actor[0x44]+4`. |
| `80024D78` | Per-actor OBJECT-table rebuild. |
| `80031D00` | Per-frame text-actor tick. Walks the actor list at `gp[+0x148]` and dispatches on `actor[+0x1C]`: cases 0/1/D/11 render text via `FUN_80036888`/`FUN_8003CC98`; cases 4/6/C/21 hand off to sub-routines. The per-frame driver behind dialog/labels. |
| `8001EBEC` | Per-frame OBJECT[10/11] swap (pose select for player TMDs). Also: mode-aware sound-driver extension dispatcher. |
| `8001E890` | DATA_FIELD player loader. Loads `data_field_player_lzs` chains, registers TMDs. |

## Audio

| Address | Role |
|---|---|
| `8001FA88` | Sound subsystem init / `.dpk` loader. Loads `bse.dat` master bank then per-scene `.dpk` from `h:\main\bg\domepack\…`. |
| `8001FC00` | Streaming-asset loader. Builds paths under the `sound\` prefix; XA / `.pac` / `STR` consumer. |
| `800243F0` | Per-frame BGM/asset poller. Resolves BGM IDs via the PROT-relative offset scheme. |
| `800250D4` | Per-actor SFX trigger: `(sound_id, voice)`. Looks up sound table at `&DAT_8006F198 + sound_id*8` for `sound_id < 0x200`, or runtime-allocated table at `_DAT_8007B8D0` for higher IDs. Reads voice-count from `entry[3] & 0x1F`, calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each voice. Called from per-frame actor tick when `actor[+0xb4] != 0` or `actor[+0xac]` is staged. |
| `8003E104` | Monster-sound bank loader: `(monster_idx, slot, dst_buf)`. Reads `h:\mpack\monster.snd` for the given monster — LBA TOC at `0x801C8980-0x10` (4-byte stride, 2-entry pair = `[start_lba, end_lba+1]`). Dev path (`_DAT_8007B8C2 != 0`) goes through `FUN_800608F0`/`_920`/`_944`/`_910` (fopen/fseek/fread/fclose); retail path stages parameters into the gp window (`+0x97c`, `+0x894`) and kicks `FUN_8003F128` (async CD read). Called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8). |
| `80062340` | `SsSeqOpen` — allocates a sequencer slot from the 16-slot bitmap at `_DAT_801CD2B8`; emits `s_Can_t_Open_Sequence_data_any_mor_80015D34` on full. See [`subsystems/audio.md`](../subsystems/audio.md) → "SsAPI sequencer". |
| `80061D18` | `SsSeqClose` — clears bitmap bit, memsets all 16 channel records (`0xB0` each) to defaults. |
| `8006275C` / `8006282C` | `SsSeqPlay` (ramped + 1-arg shim). |
| `800628F0` | `_SsSeqCtrl` — Stop / Pause / Resume internal. |
| `800641EC` | `SsSeqRewind` — full slot reset to start of sequence. |
| `80062410` | `_SsSeqInit` — SEQ-header parser (`'Sp'` magic + version `0x01`). |
| `80061C68` | `_SsSeqGetVar` — MIDI-style varint delta-time decode. |
| `80061EDC` / `80067E9C` | `SsSeqSetVol` (per-channel + slot vol/pan). Clamps `0..0x7F`. |
| `80066E50` / `80067550` | `_SsPitchFromKey` + `_SsVoNoteOn` — note→pitch table at `_DAT_8007A940` + master×velocity×channel-vol×stereo-pan voice mixer. |
| `80062AA0` | `SsSetMVol` — packs `[cmd=3, x*0x81, y*0x81]`, calls `FUN_8006BCB4`. |
| `80068D94` | `SsSepOpen` / SEP loader core — validates `'VAP'` magic, allocates SPU memory, patches per-track pointers, writes MIDI body to SPU. |
| `80069B18` / `800697E0` / `80069DA8` | SPU transfer engine. `_DA8` = top-level `SpuWrite` (picks DMA vs CPU copy on `_DAT_8007AF5C`); `_B18` = 4-mode DMA state machine (arm-read / arm-write / set-addr / commit); `_7E0` = CPU-copy alternative. See [`subsystems/audio.md`](../subsystems/audio.md) → "SPU DMA transfer engine". |
| `8006A020` / `8006A04C` | `_spu_a` direction flips — set SPU command register bits `0x20000000` (read) / `0x22000000` (write). |
| `8006A078` | SPU register-settling delay (60-iter busy-wait). |
| `8006A158` | `SsSpuMalloc` — block-table first-fit allocator over `_DAT_8007AFA4`. |
| `8006A420` | `SpuFree` compactor — coalesces adjacent free entries, shifts table down. |
| `8006A728` | `SpuFree` — block-table free in `_DAT_8007AFA4`. |
| `8006BC9C` | `SpuIsTransferPaused` — `return _DAT_8007AF74 != 1`. |
| `8006ACBC` / `8006C048` | `SpuSetVoiceAttr` (mask dispatcher + 24-voice broadcaster). |
| `8006B1B4` | `SpuSetReverbModeParam` — 30-attr reverb commit, writes regs `0x1C0..0x1FE`. |
| `8006BCB4` | `SpuSetCommonAttr` — master vol L/R + reverb regs + SPUCNT bits. |
| `8006C6E4` | `_SsKey2Pitch` — `((key1*0x80+fine1) - (key2*0x80+fine2)) / 0x600` exponential build. Returns 14-bit SPU PITCH. |
| `_DAT_801CE564` / `_DAT_801CE574` (data) | Legaia-installed seq-context vfn pointers — `_564` resolves the active script-VM seq context, `_574` is a worker-availability check. Used by `FUN_8006CA7C / CB3C / CDB0 / CE30 / DDC8`. |

## Renderer / GPU primitives

| Address | Role |
|---|---|
| `80024EE4` | Push textured-quad GPU primitive onto the OT chain. `(layer, depth, color)` — writes a 6-word PSX GP0 packet (`0x05000000` length + `0x2B` polygon-with-tex command + four corner verts at `_DAT_1F80038C/0x18E` × `0xFFFC`) at `_DAT_1F8003A0`, then linkPrim via `FUN_8003D2C4`. Used by `FUN_800196A4` for the screen-fade / dim overlay. |
| `80035CB8` / `80035DA0` / `80035E44` | Text-actor sub-handlers. Children of the per-frame text-actor tick (`FUN_80031D00`). Each measures a row via `FUN_80036044` and renders via `FUN_8003CC98`. `_DA0` resolves a magic-name string from `PTR_DAT_80075DB0` keyed by the `0x800754CC + idx*0xC` magic table; `_CB8` advances state at gp `+0x87c` / `+0x13c`. |
| `8003C310` | Push `POLY_F3` (flat-shaded triangle) GPU primitive onto the OT. Writes size + color + verts; uses Y-offset `_DAT_8007B454`. |
| `8003F348` | Per-frame sprite/animation renderer tick. Walks list at `DAT_8007B7E0`, accumulates draw cost into `gp[+0x990]`. |
| `8003F3FC` | Per-frame particle-actor update. Clip-tests against viewport `_DAT_1F800384..387`, accumulates physics (`vx*dt`), tests against camera at `_DAT_8007C364+0x14/+0x18`, emits two GP0 line packets (cmd `0x9000000`) via `_DAT_1F8003A0` OT pointer. Calls `FUN_8003F838` (RNG) + `FUN_8003F86C` (line-clip + emit). |
| `8003F838` | Particle PRNG step — 13-instr LCG: `seed = seed * 12 + 2`, byte-swap. State at `_DAT_1F8002A8`. |
| `8003F86C` | OT line-segment emitter with GTE-projected endpoints. 148 instrs: cop2 `0x280030` (RTPT) + `0x1400006` (NCLIP); inserts into ordering table at `_DAT_1F8003F4`. Returns `1` on emit / `0` on cull. |
| `8001FA68` | Generic ringbuffer push-u16: `*(u16*)(p2 + (++*p1)*2) = val`. |
| `80049348` | Actor animation frame setter. Loads frame offsets from the battle actor pointer table (`0x801C9370`) into the animation tables at `0x80076908` / `0x80076914`. |
| `8004A908` | NTSC/PAL-adaptive color dithering + brightness mixer for OT primitives. Reads `_DAT_80078D4C` mode flag. |
| `80046978` | Palette fade / tint engine. Reads RGB components, applies global brightness from `_DAT_1F800393`. |
| `8004695C` | Initiates a color-fade operation: writes RGBA into `gp[+0x9D0]`, sets active-flag at `gp[+0x9D4]`. Mode byte at `_DAT_8007B6CC`. |
| `8005724C` | OT primitive initializer for sprite rectangle — pos / size / color / clip. Calls `FUN_800608E0` for display config and `FUN_80057FEC` for palette query. |
| `80059568` / `80059634` / `80059700` | OT coordinate packer trio for textured / textured-variant / opaque sprite primitives. Display-mode-aware mask + shift, COP2 tag bytes `0xE3` / `0xE4` / `0xE5`. |
| `80058068` | `SetDispMask` wrapper — controls display enable/disable via GP1 command `0x300` / `0x3000001`. |
| `8005800C` | DrawSync callback registration. |
| `80057C44` | Display-mode reset dispatcher — calls GTE init, memory clear, resolution setup. |
| `80058F1C` / `80058FA0` | Rect / Line OT primitive builder pair using COP2 coordinate transforms via the packer trio. |
| `8005AFB0` | GTE control-reg initializer (COP2 ctl regs `0xC000..0xF000`). |
| `8005B038` | GTE matrix-multiply loop — transforms a vertex stream through COP2. |
| `8005B0B8` | GTE shift-converter for texture / color bit packing. |
| `8005B618` | GTE matrix-loader (COP2 MTX regs `0x0..0x2000`). |
| `80021EAC` (data: `_DAT_8007BD24+0x26B`) | Animation tick counter incremented by `FUN_80055B4C`. |

## ANM animation container

The container parser is documented in [`formats/anm.md`](../formats/anm.md). The per-record bytecode dispatcher is overlay-resident (not yet captured); the public SCUS entry point only stages the per-record state on an actor.

| Address | Role |
|---|---|
| `80024CFC` | `play_anm_by_id(id, actor, ?)` — allocates an actor (via `FUN_80020DE0`), reads the per-record offset from `_DAT_8007B7C8 + (id*4) + 4`, and stores `(anm_base + record_offset)` in `actor[+0x4C]`. Writes `0xB` to `actor[+0x56]` (anim state) and `100` to `actor[+0x68]` (frame counter). The bytecode walk runs in a per-frame actor tick that hasn't been traced. |

## MES / dialog text interpreter

The MES bytecode interpreter is **statically linked into SCUS_942.54** — not overlay-resident as previously assumed. Four functions cover the encoding fully; the dialog window pager is overlay-resident in the dialog/town overlay. See [`formats/mes.md`](../formats/mes.md) for the per-byte decoding table.

| Address | Role |
|---|---|
| `8003CA38` | Glyph stride walker. 16 instructions: returns count of bytes until next terminator (`< 0x1F`). For each `(byte & 0xF0) == 0xC0` it consumes an extra byte. |
| `80036044` | Text width measurement. Same byte classification as the stride walker plus substitution dispatch on `(byte + 0x40) < 8` (catches `0xC0..0xC7`); the explicit cases `0xC1..0xC5` and `0xC7` follow substitution pointers into character-name / item / magic / spell / quest tables and recursively walk the substituted string. |
| `80036888` | Text renderer. Same opcode dispatch as `FUN_80036044`, but emits glyphs into the text-actor buffer instead of just measuring. Calls `FUN_80036514` to expand substitutions before walking. |
| `80036514` | Substitution expander. Copies from source bytecode to a working buffer, normalising the input-time aliases (`0x5E XX` → `0xCE (XX-0x2D)`, `0xFF` → `0xCF`) and inlining `0xC1..0xC5` / `0xC7` substitutions into glyph runs. |
| `FUN_801D84D0` (dialog overlay) | Dialog window pager. 26-state machine (`_DAT_801F2734`) for per-frame paging, 16-line buffer at `_DAT_801F3540`, terminator test `(byte & 0x7F) < 0x20`. Drives the actual on-screen dialog window. |
| `FUN_8001FD44` | Dialog opener. Sets `_DAT_1F800394 |= 0x40` (dialog-active story flag). Called from script-VM op `0x3F`. |

## Records / stats screen

The "records" page (battles fought, escapes, play time, per-character maximums) is rendered by a single function in the `0897` town overlay. Stats globals are persistent save data.

| Address | Role |
|---|---|
| `FUN_801ED710` (overlay 0897) | Records-screen renderer. Reads + draws six stats blocks via `FUN_8003CC98` (single-line text) and `FUN_80034B78` (number formatter): "No. of Battles" (`_DAT_800846A4`, capped at 99999), "No. of Escapes" (`_DAT_800846A8`), play time (`_DAT_800845DC` divided twice by `0x3C` for `H:MM:SS` decomposition, capped at 99h59m59s), then 3× per-character "Maximum Hits" / "Maximum Damage" / "Knockouts" iterating a stats record array at `0x80088140` (stride `0x414`, `+0x6B4` = max-hits u32, `+0x6B0` = max-damage u32). The function uses depth slot `_DAT_8007B454` (5 / 6 / 7 / 9 levels) to layer text-actor primitives over the panel background. |

## Inventory / spell list

| Address | Role |
|---|---|
| `80042DBC` | Spell-list pop: `(char_idx, spell_id, dst_slot)`. Per-character record stride `0x414` (matches the magic-table stride from `FUN_80036044`). Searches the per-character spell list at `[char_base + 0x13d ..]` for `spell_id`, copies the matched 4-byte record into the active-spell slot at `[char_base + dst_slot*0x14 + 0x2B0]`, then shifts the rest of the list down (counter at `[char_base + 0x13c]`). |

## Menu / HUD globals

| Address | Role |
|---|---|
| `80034A6C` | Menu / HUD globals reset. Initialises `0x80084594..0x800845B8` and `0x800846D0..0x800846DC` to default UI palette / cursor positions. Zeros the 512-byte save-data scratch slot at `0x80084340..0x8008453F`. Calls `FUN_8003CE08(0x1A)` (set 4th-flag-bank bit `0x1A`) when `_DAT_8007B868 != 0`. |
| `800337B0` | Menu-string formatter and renderer. 27 KB switch-on-mode that drives the character-status / equipment / spell-screen pages via `FUN_8003CD00` (multi-line) and `FUN_80036888` (raw draw) keyed on string buffers at `&DAT_8007B4B0..` and the multi-line label table at `gp + 0x13c + 0x7F86`. |

## Stub helpers

These are 2-instruction `jr ra` / nop bodies — likely retail-disabled debug hooks where the dev gate lives in the caller. Listed for completeness so a clean-room port can implement them as no-ops without further investigation.

| Address | Role |
|---|---|
| `80024C80` | Move-VM op `0x16` body. The opcode is a no-op. |
| `80024DFC` | Actor-cleanup hook (called from `FUN_8002519C` while freeing an actor). |
| `8002B93C` / `8002B944` / `8002B94C` / `8002B954` | Cluster of debug-disabled helpers. |
| `8003E7F0` | Reserved sound-path stub (called from `FUN_80017AAC`). |
