# Key function directory

A directory of the Ghidra-traced functions that matter for understanding Legaia's runtime. Each entry has a Ghidra-dump path under `ghidra/scripts/funcs/` (read it for the canonical disassembly + decompiled C). Functions in `0x801C0000+` are RAM-loaded overlays and are dumped under `overlay_<label>_<addr>.txt`.

## Asset loading + dispatch

| Address | Role |
|---|---|
| `8001A55C` | LZS decoder. Algorithm reverse-engineered from this function. |
| `8001A8B0` | Raw memcpy. Used by the asset dispatcher when `copy_only = 1`. |
| `8001E1B4` | Per-stage init. Allocates the 0x62C00-byte asset buffer at `_DAT_8007B85C`. |
| `8001F05C` | Asset-type dispatcher. The `(type_size, copy_only)` switch - see [`formats/asset-type.md`](../formats/asset-type.md). |
| `8001FE70` | Battle-init per-PROT walker for [`scene_tmd_stream`](../formats/scene-bundles.md) entries. Reads chunk0 as `[TMD body size][TMD body]` (copies into `_DAT_8007B864`), then loops: type `0x01` -> `LoadImage(payload)`, type `0x02` or size `0` -> stop, otherwise skip. Called from `FUN_800513F0` (battle scene-loader state) after `FUN_8001FA88` reads the PROT entry. Distinct from `FUN_8002541C` despite the matching chunk-header packing - type `0x01` here means "single bare TIM", not `TIM_LIST`. See [`formats/scene-bundles.md`](../formats/scene-bundles.md#streaming-tail---fun_8001fe70-walker). |
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

Full 13-function CD-read API stack documented in [`subsystems/boot.md` § CD-read API stack](../subsystems/boot.md#cd-read-api-stack); per-function memory entry in `project_cd_read_api_stack.md`.

| Address | Role |
|---|---|
| `8003D3C4` | Path-based ISO9660 file loader: `(path, dest)`. Wraps `FUN_8005DBB4` (dir lookup) + SetLoc + `FUN_8005E9A4`. Used for `.STR` / `.XA` filesystem files. |
| `8003E360` | Demonstrates the **dual-mode loader pattern**: retail (`_DAT_8007B8C2 == 0`) branches to ISO9660 file system (`FUN_800608F0` open + `FUN_80060944` read); debug branches to PROT TOC index (`FUN_8003E8A8` + `FUN_8003E800`). |
| `8003E4E8` | Boot-time TOC loader. `(filename_str, do_read_flag)`. Called from `FUN_8003F08C(0)` with `"PROT.DAT"`. Reads first 3 sectors of `PROT.DAT` (= 6 KB) into `0x801C70F0`. |
| `8001F7C0` | **Per-scene field-asset loader.** `(dest, scene_name, field_record)` fills the field buffer at `dest` (`_DAT_1f8003ec` base). The leading region — collision grid (`+0x4000`), object map (`+0x8000`) — is the main `.MAP` file; field-pack at `+0x12000` and `efect.dat` at `+0x12800` are separate files. Retail: ISO9660 `DATA\FIELD\<scene>.MAP` by name. Debug (`_DAT_8007B8C2 != 0`): `FUN_8003E8A8(field_record, 1)` sets the `CdlLOC` from `PROT_TOC[field_record+2]`, then `FUN_8003E800(dest, 0x28, 1)` streams 40 sectors (`0x14000` bytes). So the per-scene collision grid is the `+0x4000..+0x8000` slice of `<scene>.MAP`. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). |
| `8001FD44` | **Scene-change-packet API** `(name_ptr)`. Sets the next scene by CDNAME string: copies `name_ptr` → `0x8007050C`, syncs to the active buffer `0x80084548` via `FUN_8001D7F8`, and stages the load (gated retail-vs-debug on `_DAT_8007B8C2`). `s_ERR_CHANGE_PACKET` guards re-entry while a packet is pending (`_DAT_8007BA3C`). One argument only — the `a1` the decompiler shows at some call sites is dead. This is the **name-based** scene transition (distinct from the map-id door-warp `FUN_80025980`); it carries the opening `opdeene`→`town01` handoff. See [`subsystems/boot.md`](../subsystems/boot.md#opdeene--town01-handoff-scene-change-packet). |
| `801D1344` | **Per-frame field/cutscene controller.** Among its per-frame work it issues the one-shot opening handoff: when `_DAT_8007B868 == 0 && (_DAT_1F800394 & 0x4000000) && (_DAT_8007B850 & 0x100)` (normal mode + the cutscene-set trigger flag + the player's confirm pad bit), it fades (`FUN_801D58F0`), sets the town01 entry coords (`_DAT_80073EF4 = 0xEC0`, `_DAT_80073EF8 = 0x2DC0`), clears the trigger flag (fire-once), and calls `FUN_8001FD44(s_town01_801ce82c)` — the target name `"town01"` is the overlay literal at `0x801CE82C`. |
| `8001D424` | Loads the dev file `initmap.txt` (via `FUN_8003E6BC`) and copies 16 bytes into the scene-name buffer `0x8007050C` — the executable's default start map (`"town01"`). |
| `8001D7F8` | Syncs the scene name `0x8007050C` → the active buffer `0x80084548` (`FUN_80056758`). Called by `FUN_8001FD44` and the field initializer. |
| `8003E6BC` | Path-based opener. Resolves dev paths into a PROT index via the CDNAME-driven name map. |
| `8003E800` | Async LBA-based loader. `(dest, lba, flags)`. Queues a load via `gp+0x97c` (lba) / `gp+0x894` (dest), kicks via `FUN_8003F128` when `flags & 1`; blocks on completion when `flags & 2`. |
| `8003E8A8` | PROT TOC index resolver. `(prot_index, flag)` → LBA. Reads `*(0x801C70F0 + (index+2)*4)`. Matches the [PROT TOC math](../formats/prot.md). |
| `8003EB98` | By-index sync loader. Wrapper around `FUN_8003E8A8` + `FUN_8003E800(…, 1)`. |
| `8003EBE4` | **Overlay loader A.** `param + 0x381` → PROT index. Destination buffer pointer in `*DAT_8001038C`; current-id tracked in `gp+0x924`. |
| `8003EC70` | **Overlay loader B.** Parallel to `FUN_8003EBE4`. `param + 0x381` → PROT index. Destination buffer in `*DAT_80010390`; current-id tracked in `gp+0x934`. Allows two overlays resident simultaneously. The battle-action SM uses it to page in the **per-summon Seru-magic overlays**: a player Seru-magic cast (spell id `actor[+0x1df]` in `0x81..0x8b`) calls `FUN_8003EC70(id - 0x79)` → PROT `id - 0x79 + 0x381`, i.e. **PROT 905..915** (Gimard *Tail Fire* `0x81` → PROT 905). These overlays carry the summon's 3D models + animation/render logic (the small-TMD library that registers into `DAT_8007C018`); the flame is **not** in `befect_data`. See [`subsystems/battle-action.md`](../subsystems/battle-action.md#seru-magic-summon-overlay-dispatch). |
| `8003F128` | Async CD read kickoff. Stages parameters, submits to BIOS-level CD library. |
| `8003F08C` / `8003EFE8` | Boot-time entry points that call `FUN_8003E4E8("PROT.DAT", 1)` to populate the TOC at `0x801C70F0`. |
| `8005C328` | LBA → BCD-MSF converter. Inverse of `FUN_8005C42C`. |
| `8005C42C` | BCD-MSF → LBA converter. `((m*60 + s)*75 + f) - 150`. |
| `8005D9A0` | CD-DMA-channel-3 synchronous read primitive. Writes CD command registers (`*DAT_800796A4` etc.) and triggers DMA via `*DAT_800796B4 = 0x11000000`. Takes `(dest_buffer, mode)`. The address `0x8005DA40` is an instruction inside this function (`lui v1, 0x8008`) — Ghidra promotes that intra-function label to a fake `FUN_8005DA40` entry. There is no real function at `0x8005DA40` and no `_DAT_800795B4` pointer table. |
| `8005DBB4` | ISO9660 directory lookup. `(file_info_out, filename)` → fills `{msf[3], size_bytes, …}`. |
| `8005E574` | Streaming-read per-IRQ callback. Drives multi-sector reads via streaming-read working globals (`DAT_800796CC` destination cursor, `DAT_800796D8` sectors remaining, `DAT_800796E4` current LBA). Registered by `FUN_8005E788`. |
| `8005E788` | Streaming-read starter. Copies source globals (`DAT_800796C8` → `DAT_800796CC`; `DAT_800796C4` → `DAT_800796D8`) and registers `FUN_8005E574` as IRQ callback. Sets initial LBA via `FUN_8005C42C(FUN_8005BD70())` (reads BCD MSF from libcd's GetLoc-equivalent). |
| `8005E9A4` | **Public streaming-read API.** `(sector_count, dest_buffer, mode_flags)`. Sets the streaming-read source globals + calls `FUN_8005E788(0)`. Caller must SetLoc beforehand. Sector size from `mode_flags`: `& 0x30 == 0` → 0x800 (2048, data); `== 0x20` → 0x924 (2336, XA); else 0x918. |
| `8005E4D4` | Synchronous LBA-based file reader: `(sector_count, lba, dest_buffer)`. Wraps `FUN_8005C328` + `CdControl(SetLoc)` + `FUN_8005E9A4` + completion poll. |
| `8003EF14` | **Field-buffer per-sector streaming poller.** Distinct from the `DAT_800796xx` streaming path above: uses the `0x8007BCxx` global bank (`gp+0x940` destination cursor, `gp+0x968` sectors remaining, `gp+0x8c8` sector sequence, `gp+0x964` timeout). On each CD ready-IRQ, DMAs one 2048-byte sector via `FUN_8005C2C4`/`FUN_8005D9A0` to `*(gp+0x940)`, then advances the cursor `0x800` and decrements `gp+0x968`; on completion calls `FUN_8005BEE4(0)` + `FUN_8005C034(9,0)`. This is the path that streams a field scene's buffer — collision grid (`+0x4000`), object map (`+0x8000`), field-pack (`+0x12000`) — into RAM at scene load. Pinned by a runtime Write-watchpoint on the live collision grid (caller chain `FUN_8005D9A0` ← `FUN_8005C2C4` ← `FUN_8003EF14`@`0x8003EF68`); see [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The per-scene start LBA + sector count + dest are set by the field-asset loader `FUN_8001F7C0` (debug branch: `FUN_8003E8A8` sets the `CdlLOC` from `PROT_TOC[field_record+2]`, then `FUN_8003E800(dest, 0x28, 1)`; retail branch: ISO9660 `DATA\FIELD\<scene>.MAP` opened by name). |

## PSX runtime / standard libraries

Statically-linked PsyQ glue. Trivial to stub in a clean-room port.

| Address | Role |
|---|---|
| `80056678` | `EnterCriticalSection` - `syscall(0)` with `$a0=1`. |
| `80056688` | `ExitCriticalSection` - `syscall(0)` with `$a0=2`. |
| `80056738` / `80056748` / `80056758` / `80056768` / `80057014` / `8005ACE8` / `8005ACD8` / `8005BBE8` / `8005FD68` / `8005FD78` | `jr 0xA0/0xB0/0xC0` - PSX BIOS table dispatchers (libapi). Identified targets: `80056748` = `strncmp` (A0 0x18); `80056758` = `strncpy` (A0 0x19, `li t2,0xA0; jr t2; li t1,0x19`) - the name-copy thunk used by `FUN_800560B4` (party-name seed) and `FUN_8001D7F8` (scene-name sync); `80056768` = `strlen` (A0 0x1B); `80057014` = `rand` (A0 0x2E); `8005ACD8` = `GPU_cw` (A0 0x49); `8005BBE8` = `FlushCache` (A0 0x44); `8005FD68` = B0 0x5B (card init); `8005FD78` = `ChangeClearRCnt` (C0 0x0A). |
| `80056658` | `TestEvent` BIOS thunk - `jr 0xB0` with `t1=0x0B`. Polls a kernel event handle. |
| `8006B844` | `WaitEvent` BIOS thunk - `jr 0xB0` with `t1=0x0A`. Blocks on a kernel event handle. |
| `80056698` / `800566A8` / `800566B8` / `800566C8` / `800566D8` / `800566E8` / `800566F8` / `80056708` / `80056718` | Byte-identical `li t2,0xB0; jr t2` BIOS B-vector thunks emitted by the linker once per caller. The selected B-routine is determined by `$t1` set up by the caller, not by the thunk. Same pattern at `8006EE14` / `8006EE24` / `8006EE34` (B0-vector cluster cited from menu/text helpers). |
| `8006D7A4` | BIOS C0 thunk - `li t2,0xC0; jr t2; li t1,0x3`. Dispatches to C0 vector 0x03 (`ChangeThreadSubFunction`). Called from `FUN_8006D2AC` (audio subsystem init). |
| `8006EF18` (caller) + `8006EF68` / `8006F088` / `8006F118` (trio) | SPU voice-state init sequence. `FUN_8006EF18` calls all three in order. `_EF68` = B0 0x4C (`InitCd`-adjacent). `_F088` = B0 0x57 then swaps 5 dwords between `DAT_8006F058..F06C` (static table) and `iVar1 + 0x9C8` (SPU voice block) + `FlushCache`. `_F118` = B0 0x56 + symmetric swap at `iVar1 + 0x18 / -0xE80`. |
| `800567B8` / `80056B18` | `printf`-class formatter (handles `%d %x %o %s %f`); writes into a static buffer. |
| `80057024` | `memmove` - overlap-safe direction-aware copy. |
| `8005ACAC` | `memset`. |
| `8005E540` | `memcpy` - forward-only byte copy. |
| `8005FF04` | `irq_mask_swap16` - atomic-swap on `_DAT_8007A868` (16-bit IRQ mask). Used by `FUN_8005A78C` to gate SIO setup. |
| `8005FCCC` | VSync wait spin (frame-budget gated). |
| `80060A04` | `BREAK 0x105` - debug trap. |
| `800608E0` / `8006xxxx` | libapi `fopen` / `fseek` / `fread` / `fclose` cluster. |
| `8005FD88` | libapi device-vtable trampoline (slot `+0xC` of `PTR_PTR_8007A860`). |

### libgte primitives

| Address | Role |
|---|---|
| `8005B7C0` | `SetRotMatrix`-shaped - `setCopControlWord(2, 0xD800, x)`. |
| `8005B7CC` | `SetTransMatrix`-shaped - `setCopControlWord(2, 0xE000, x)`. |
| `8001CF50` | **Cutscene/field camera GTE rotation build.** Composes the view rotation matrix from the three camera-angle globals by rotating about each axis: `RotMatrixX(pitch=_DAT_8007B790)` / `RotMatrixY(yaw=_DAT_8007B792)` / `RotMatrixZ(roll=_DAT_8007B794)` (each gated per-object by a flag bit at `obj+0x52` so a draw can inherit the globals), then sets the GTE translation from the focus and emits. This is what consumes the op-`0x45` camera angles - the build path `FUN_801DAB90` / `FUN_801DB8EC` only *stage* the angles + focus + H. See [`subsystems/cutscene.md`](../subsystems/cutscene.md). |
| `800461A4` / `8004629C` / `8004638C` | GTE `RotMatrixX` / `RotMatrixY` / `RotMatrixZ`. `(angle12)`: masks the angle to 12 bits (`4096 = 360°`), indexes the shared sin/cos LUT at `0x80070A2C` (`+0x800` = the quarter-wave cosine offset), and multiplies the current matrix by the per-axis rotation via GTE `mvmva`. Used by the camera build `FUN_8001CF50` and other per-object transforms. |
| `8005BA1C` | GTE square-root / normalize - `mtc2 0xF000 / mfc2 0xF800`. |
| `8005BA68` | GTE 3-point transform helper. Loads 3 vertex pairs into COP2 registers, calls `copFunction(2, 0x280030)` (RTPT), reads back SXY0/1/2 and OTZ. Cited from the cutscene/world-map sprite batcher (`FUN_801CFC40`). |
| `8005BB48` | `InitGeom` - saves return addr to `_DAT_8007BDF0`, `EnterCriticalSection`, copies exception-handler table from `DAT_8005BBB0`, `FlushCache`. |
| `8005AF0C` | `isqrt`-style normalise - uses `FUN_8005BA1C` (GTE normalize) then dispatches to `FUN_8005ADB8` with shift correction. |
| `8005ADB8` | Fixed-point bit-rotation / arc helper - consumed by `FUN_8005AF0C`. 85-instr ladder of conditional shifts. |

### libcd primitives

| Address | Role |
|---|---|
| `8005CCB4` | `CdSync` - 179-instr poll loop, spins on `FUN_8005C4AC` using timeout in `_DAT_801CADF8` (~16 ms). Strings: `"CD ready"`, `"CD timeout"`. |
| `8005D9A0` | `CdControl_raw` - writes CD MMIO regs at `_DAT_80079670/67C/6A4/680/6A8/6AC/6B0/6B4` with command bytes. Spins waiting bit `0x40` then `0x1000000`. |
| `8005C2C4` | `CdDiskReady` - wraps `FUN_8005D9A0`, returns `rc == 0`. |
| `8005A78C` | Pad / SIO init - touches `_DAT_80078E28/E34/E44` (SIO MMIO), wraps with `FUN_8005FF04` (IRQ disable), clears 256 B at `0x801C948C` and 6 KB at `0x801C9590` (pad RX/TX buffers). |
| `8005ABD0` | Pad-protocol-phase handshake - bitfield writes `0xE1001000 / 0x20000504 / 0x10000007` on `_DAT_80078E24/E28`. SIO digital-pad protocol selector, returns state code 0..4. |

## CD / file-system (libcd-style)

Used by the sound subsystem's dev branch and elsewhere when retail-async CD reads aren't appropriate. Stack ordering: SCUS path-opener (`0x8003E6BC`) → libcd file-system (`0x8005DEA0`) → libcd primitives (`0x8005CCB4 / D9A0 / E4D4`) → libapi device-vtable (`0x8005FD88`) → BIOS A0 / B0 / C0 traps.

| Address | Role |
|---|---|
| `8005DEA0` | Directory parser - reads the active sector, caches up to 128 entries into `0x801C4BEC`. |
| `8005E180` | Directory-entry lookup by ID; returns slot index or `-1`. |
| `8005E228` | File loader - reads sectors from a directory entry into a cache buffer. |
| `8005E4D4` | High-level open-then-read helper: `(buf, dir_entry, size)` → `bool`. Calls `FUN_8005C328` to build the CdlLOC, `FUN_8005BEFC(2,…)` to issue `CdlSetloc`, `FUN_8005E9A4` to set sector size `0x80` (2048 vs 2336). |
| `8005E574` | CD sector-reader state machine - handles block reads, timeout, completion callback at `_DAT_800796C0`. |
| `8005C42C` | BCD-MSF → LBA: `((m*60 + s)*75 + f) - 150`. |

## Helpers

| Address | Role |
|---|---|
| `80017888` | Malloc - the general-purpose allocator. |
| `8003C5F0` | Generic ramp scheduler. 64-slot pool at `0x801C66A0` (stride 0x20). Used for sound + render-bank ramps. |
| `8003D038` | Animation index filter. Writes `DAT_80073F1C = param` when `(&DAT_801C6470)[param * 4] != -0x74`; silently skips invalid entries. Called from the cutscene/world-map sprite batcher (`FUN_801CFC40`) with actor`+0x50` (anim-index field). |
| `8001FA34` | Sprite-list consumer. Decrements the u16 count at `*param_1` and returns `*(short *)(param_2 + 2*(count-1))`; returns -1 on underflow. Pops the "current" entry index from a compact sprite-list header. Cited from the cutscene sprite emitter (`FUN_801D629C`). |
| `8003C83C` | Script-context resolver. `id == 0xF8` returns `_DAT_8007C364` (the player / camera-anchor actor); `id == 0xFB` is the system channel; any other id matches an actor by `node[+0x14]` in the `_DAT_8007C354` list. Resolves the field VM's cross-context (`0x80`-bit) target byte. |
| `8003BDE0` | **Partition-record → VM-context dispatcher.** Resolves a scene-MAN partition record by index, walks its header, and spawns a field-VM context (`ctx[+0x90]` = record base, `ctx[+0x9e]` = entry PC, `ctx[+0x10] \|= 0x100`). Partition-2 (cutscene-timeline) records use a **named-record header** `[u8 name_len][name_len*2 SJIS][u8 C0][C0 bytes][u8 C1][C1*u16][u8 C2][C2*u16]<script>` (blocks 1/2 are story-flag OR/AND gates); script entry = `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`. Invoked from scene-entry / tile-trigger SMs (`FUN_801D27E0` / `FUN_801D1EC4`). |
| `80039B7C` | Per-context field-script runner. Loops `FUN_801DE840(ctx[+0x90], ctx[+0x9e], ctx)` until a yield/halt, gated on `ctx[+0x10] & 0x100`. The per-frame tick of a context `FUN_8003BDE0` spawned. |
| `8003C764` | On-screen text-balloon spawner. Allocates a text context from the pool, sets bytecode = the page bytes, seeds the per-page display timer `actor[+0x9c] = 0x78` (120 frames), centers the line `X = (320 − width)/2` and fixes `Y = 180`. The draw + pacing path the field-VM narration op (`0x4C` outer-nibble-8) routes to. |
| `80036044` | Text-width measure for inline dialog/UI strings. Walks a byte stream: `>= 0x1F` = glyph (count 1); `0xC0..0xC7` = escape (substitutes from inventory / magic / item-name tables - `0xC1` = item-name @ `0x80084549 + idx*0x414`, `0xC2` = `PTR_DAT_8007436C[idx*3]`, `0xC3` = magic name @ `PTR_s_Magic_800754D0`, `0xC7` = `DAT_80073F24 + idx*8`); `0xCE` = newline (line++); `0xCF` = end-of-row. Returns total glyph count. |
| `8003CC98` | Single-line text render-and-measure. `FUN_80036044(buf)` for length + `FUN_80036888(buf, palette, 0, x, y)` to draw, returns the length. |
| `8003CD00` | Multi-line text layout. Walks a string line by line: measure with `FUN_80036044`, draw with `FUN_80036888`, advance Y by `0x0D` per line. Stops on the first sub-`0x20` control byte. Returns max line width. |
| `8003CE08` / `CE34` / `CE64` | SET / CLEAR / TEST against the **fourth flag bank** (256-bit bitfield at `DAT_80086D70`). Wired by field-VM opcodes 0x50 / 0x60 / 0x70. |
| `8003CE9C` | Signed-16-bit operand decoder (sign-extended `s16` from two bytes). |
| `8003CEB8` | 24-bit LE decoder. Reads 3 bytes as a u24. |
| `8003CED8` | 32-bit LE decoder. Reads 4 bytes as a u32. |
| `80032434` | Linked-list head allocator. Lazily allocates a 0x34-byte sentinel-circular doubly-linked-list head at `gp[0x148]` (via `FUN_80017888`); the `prev = next = self` + `+8 = 0xFFFF` initialiser is the empty-list sentinel. `param_1` is a kind tag, `param_2` is a 14-halfword config record copied into the head. Used by the dialog overlay's per-frame init path (`FUN_801ECD0C` case 0). |

## Input + debug subsystem

| Address | Role |
|---|---|
| `8001822C` | Per-frame input handler / debug dispatcher. Reads BIOS pad at `0x800840F8`, builds button mask `_DAT_8007B850`. Gates upper 16 bits and all debug bindings on `_DAT_8007B98C != 0`. |
| `80016230` | Dev-print driver. Loads `program_no=%d` / `..\..\FIELD\PROGRAM\....\%d` strings only when debug enable is non-zero. |

## Move / animation subsystem

| Address | Role |
|---|---|
| `800204F8` | Move-buffer consumer. Sole reader of both `_DAT_8007B888` (MOVE) and `_DAT_8007B840` (MOVE2). Resolves `move_id` to a buffer record and stages it onto the actor - does **not** run opcodes itself; that's `FUN_80023070`. |
| `80020740` | Move-buffer pre-tick helper. Called from `FUN_800204F8` when actor flag bit `0x1000` is set. |
| `80023070` | **Move-table opcode interpreter.** 71 opcodes (`0x00..0x46`); JT at `0x80010778`. Walks the per-actor move buffer at `actor[+0x48]` indexed by PC at `actor[+0x70]` (u16 units). Opcode `0x2F` escapes to `FUN_801D362C`. See [`subsystems/move-vm.md`](../subsystems/move-vm.md). |
| `8003774C` | **Per-actor motion / path-stepping VM** (a third dispatcher distinct from the actor VM at `FUN_801D6628` and the field VM at `FUN_801DE840`). 577 instructions; switch on `cmd & 0x7F`: cases `0x37/0x41` (linear delta on `actor[+0x14]/[+0x18]` from tables `_DAT_80073F14/0x80073F04`), `0x38` (bearing ramp), `0x47` (XZ approach with quadrant select), `0x4C` (line-of-sight to target actor - resolves `0xF8/0xFB` system channels via `_DAT_8007C34C/8007C354/8007C364` actor lists, same idiom as `FUN_8003C83C`). Reads `_DAT_1F800393` (frame dt). Drives `actor[+0x05/+0x06/+0x15/+0x26]` (X / Z / step-counter / facing). Likely the per-actor pursue / patrol / face-target subroutine called by the actor or move VM. |
| `80021B04` | Actor-spawn helper. Builds per-actor OBJECT pointer table at `actor[0x44]+4`. Calls `FUN_80023070` once at spawn to run the initialisation opcodes in the move buffer. |
| `80050ED4` | **Summon / effect-actor pool allocator.** Scans the 0x60-slot pointer pool at `DAT_801C90F0`; on the first null slot calls `FUN_80021B04` (the actor-spawn helper above), stores the new actor pointer into the slot, and returns it (returns `0` when the pool is full). The alternate spawn path the effect VM takes for the "summon" effect kind instead of the generic spawn helper. Cited from `crates/engine-vm/src/effect_vm.rs` (`func_0x80050ed4` summon handler). `see ghidra/scripts/funcs/80050ed4.txt`. |
| `80021DF4` | Per-frame actor tick. Updates `actor[+0x54]` (wait timer), `+0x22` (rotation), state-2/5/6 animation slots; then calls `FUN_80023070` to step the move VM. |
| `801D362C` | Move-VM overlay extension dispatcher. 61 sub-opcodes (`0x00..0x3C`); JT at `0x801CE868`. Reached only via move-VM opcode `0x2F`. Resident in multiple overlays at the same RAM address (the `world_map` / `world_map_top` / `world_map_walk` / `0897` field / `dialog_mc4` / `dialog_typing` / `cutscene_dialogue` / `cutscene_mapview` variants all carry a copy); each overlay supplies its own JT contents. Sub-handlers shared across overlays include `0x801D31B0` (per-scanline POLY_FT4 strip emitter), `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`. |
| `8002519c` | Per-frame actor-list iterator (328 bytes). Walks a linked-list head, dispatching each node by `jalr node[+0xC]`. Five lists at `_DAT_8007C34C..._DAT_8007C36C` are iterated per frame from `FUN_80016444` (one call per render pass). Per node: `+0x00` = next ptr, `+0x0C` = tick fn ptr, `+0x10` = flags (bit `0x8` selects early-return path, bit `0x200` is the "already-emitted" guard), `+0x44` = optional prim-chain head to free. |

## Game-mode state machine

The 28 × 24-byte table at `0x8007078C` is detailed in [`subsystems/boot.md` § Game-mode state machine](../subsystems/boot.md#game-mode-state-machine).

| Address | Role |
|---|---|
| `0x8007078C` (data) | Mode table - 28 entries × 24 bytes. `+0x00` = name string ptr; `+0x10` = handler fn ptr; `+0x14` = parameter. |
| `gp[0x524]` (data) | Current-mode register (i16). |
| `_DAT_8007B83C` (data) | Master game-mode index, u16. Title overlay writes `0x1A` (= STR FMV mode 26) on attract countdown underflow; FMV id slot at `_DAT_8007BA78` is zeroed in the same block → `MV1.STR`. |
| `800179C0` | Dev mode-transition writer. Reads input mask, advances current mode. Gated on `_DAT_8007B98C != 0`. |
| `80025EEC` | Default per-frame mode handler - used by all 14 odd-indexed (per-frame) modes. Pipeline: `FUN_8001698C → FUN_80016444(1) → FUN_80016B6C`. |
| `80025C68` | Mode 0 (CONFIG INIT) handler - **loads PROT 973 (slot-machine debug overlay)** via `FUN_8003EBE4(0x4C)`. Despite the dev name "CONFIG", this is a slot-machine debug mode, not a game-config init. |
| `80025B64` | Mode 2 (MAIN INIT) handler - **field/town gameplay INIT.** Loads the field overlay via `FUN_8003EBE4(2)` (PROT 899) then calls the per-scene initializer `FUN_801D6704`, which hands off to mode 3 (field per-frame). The title screen's NEW GAME path launches this mode (`_DAT_8007B83C = 2` at `0x801DFC00`). Despite the dev name "MAIN" / older "options menu" notes, this is the field entry; the options strings in PROT 899 belong to the in-game options submenu carried by the same overlay. |
| `80034A6C` | **New-game data-init.** Establishes the fresh-game world state: writes party gold `_DAT_8008459C = 500` (hardcoded), zeroes a ~`0x200`-byte story-flag region, sets assorted party-default globals, and calls `FUN_800560B4` to expand the starting-party stat template. Does **not** set the opening scene, prompt for a name, or trigger the opening cutscene. Reached via the boot mode initializer `FUN_8001DCF8` (and `FUN_8001FFA4`). Engine mirror: `World::begin_new_game` + `NEW_GAME_STARTING_GOLD`. |
| `800560B4` | **Starting-party stat seed.** Expands the static `SCUS_942.54` starting-party template at `0x80078C4C` (`[8×u16 stats][10-byte name]`, stride `0x1A`, 4 records Vahn/Noa/Gala/Terra) into the live per-character records (stride `0x414`), copying stats + the template's **default name** (via `FUN_80056758`). Called by `FUN_80034A6C`. Parser: [`legaia_asset::new_game`](../formats/new-game-table.md); engine mirror `World::seed_starting_party`. |
| `80025DA0` | Mode 12 (MAPDISP INIT) handler - field/town init - this is the actual gameplay-mode entry. |
| `80025F2C` | Mode 13 (MAPDISP MODE) handler - field-display per-frame. |
| `80025E68` | Mode 8 (EFECT TEST INIT) handler - effect-bundle test mode. |
| `80025980` | Mode 24 (OTHER INIT) handler - loads PROT 896 (cited by `ghidra/scripts/dump_round8.py` `OVERLAY_0896_TARGETS`). |
| `80025FB4` | Mode 26 (STR INIT) handler - cutscene / STR FMV mode entry. This is the mode the title-overlay attract-loop underflow falls through to (`_DAT_8007B83C = 0x1A`). |
| `8001DCF8` | Boot-time mode initializer. 1212-byte function. NOT the per-frame dispatcher. |

## Title overlay

| Address | Role |
|---|---|
| `FUN_801DD35C` (**title overlay**, 12 104 bytes / 3 026 instructions) | Per-frame title-overlay tick. Pinned via PCSX-Redux watchpoint on the attract countdown - the BP captured `pc=0x801DDCCC` on the `sw` that writes the decremented value back. Decrements `_DAT_801EF16C` by the per-frame scalar at `_DAT_1F800393`; `bgez` branches to `0x801DFC3C` while still counting; underflow falls through and writes `_DAT_8007B83C = 0x1A` (= STR FMV mode 26). Capture pipeline: `scripts/pcsx-redux/autorun_countdown_trigger.lua`; dump at `ghidra/scripts/funcs/overlay_title_801ddccc.txt`. |
| `0x801DDCCC` (instruction) | The `sw v0, -0xe94(a0)` that writes the decremented countdown back. Acts as the watchpoint-pinning anchor for `FUN_801DD35C`. |
| `0x801DFC3C` (branch target) | Normal per-frame attract loop (rendering, input, cursor logic). Reached via `bgez v0` from inside `FUN_801DD35C` when the countdown is still positive. Not yet dumped. |
| `FUN_8005DA40` | **Not a real function** — `0x8005DA40` is an instruction (`lui v1, 0x8008`) inside `FUN_8005D9A0` (the CD-DMA-channel-3 read primitive). Ghidra promotes the intra-function label to a fake `FUN_8005DA40` entry. Earlier notes claimed this function "walks `_DAT_800795B4` and stamps `0x8000` into BSS"; that's wrong. The title state struct (including the `0x8000` countdown initial value) is populated by DMA from disc bytes, not by code. See [`subsystems/boot.md` § Title-overlay state struct](../subsystems/boot.md#title-screen-overlay-state). |

## Battle subsystem

| Address | Role |
|---|---|
| `80052FA0` / `800542C8` | Battle archive loaders (SCUS). The archive walk uses `FUN_800536BC` (record copy + offset fixup, stride `0x1C`), `FUN_80053898` (bubble sort), `FUN_80053B9C` (UI-buffer fan-out at `+0x894 + slot*0x1E0`). |
| `800520F0` | Battle scene loader (SCUS). Sequential state machine (sub-state at `gp+0xa59`) that pulls the `befect_data` cluster (CDNAME 872) via the dual-mode loader (retail dev-path string / debug PROT index): case 0x8 loads `h:\prot\battle\etim.dat` (the **effect sprite texels**), case 0xB loads `etmd.dat` (effect TMDs, registered via `FUN_80026B4C` which asserts magic `0x80000002`) + `vdf.dat`, case 0xC loads `efect.dat` into `_DAT_8007BD5C`, case 0xE calls effect-bundle init `0x801DE914(0x1000, 0xA00)`. State `9` (`LAB_800526C8`) dispatches the just-loaded `etim.dat` pack to VRAM by calling `FUN_800198E0` (→ `LoadImage`) per pack entry. See [`formats/effect.md`](../formats/effect.md#battle-effect-cluster-befect_data-cdname-872--texel-upload-source). |
| `80020050` | **Flame / effect-texture atlas loader (SCUS).** Uploads PROT entry `0x366` (870 — the flame TIMs) into VRAM **twice**, via `FUN_8001fc00(0x366, 0, region, pass, …)` (a PROT-index→VRAM wrapper that dispatches `FUN_8003e8a8`, the TOC resolver). The destination VRAM region is set up by `FUN_80017888(0, 0xf000)` / `FUN_8001e54c(0, region, 0xf000)` (the `0xf000` argument recurs in both passes), then `FUN_8002630c` / `FUN_80017b94` finalise. Gated on `_DAT_8007b868 == 0` (the same field-camera / mode gate `FUN_801dbe9c` reads). This is the long-missing blit site for PROT 870 — it is **not** loaded by the battle-bundle path `FUN_800520F0` (which pulls `0x367..0x36d`). See [`reference/open-rev-eng-threads.md`](open-rev-eng-threads.md). |
| `0x801C9370` (data) | **Battle actor pointer table** - 8 entries × 4 bytes. Slots 0..2 = party, 3..7 = monsters. Resolved by `FUN_8004E2F0` and `FUN_80054CB0`. |
| `0x80074358..0x80074368` (data) | Global 4×u32 "active abilities" bitmask. `FUN_80042558` ORs each party member's `+0xF4..0x100` block into it every frame. |
| `800431D0` | Global ability bit-test: `(bit_id) -> bool`. The read-side primitive for the bitmask above - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. Cited heavily across battle code. |
| `800349EC` | HP / threshold UI classifier: `(char_idx) -> color_idx`. Reads `[char_base + 0x0E]` (current HP) and `[char_base + 0x0C]` (max HP), returns `2`/`6`/`7`/`9` keyed on dead / quarter / half / healthy thresholds. Drives dialog HP-color tinting. |
| `80035EA8` | MP-side variant of `FUN_800349EC`. Reads `[char_base + 0x10]` / `[char_base + 0x12]`. |
| `8003FB10` | Action / ability validator. Sub-dispatches on `actor[+0x9A8]` byte; checks the global ability bitmask (`FUN_800431D0`) and per-actor flag bits to decide whether a queued action can proceed. |
| `0x80084708` (data) | Character record table base. Stride `0x414` per character. See [`subsystems/battle.md`](../subsystems/battle.md) → "Character record layout". |
| `80042558` | Per-frame stat aggregator. Walks the 3 active party members, caps stats at `0x3E7`, OR-aggregates `+0xF4..0x100` ability flags into the global bitmask. Calls `FUN_800432BC` / `FUN_80042DBC` to maintain the active-spell slot list at `[char + 0x2B0..]`. |
| `80043048` | Status-effect timer decrementer: `(idx, decrement, default)`. Walks a stride-2 table at `_DAT_80085958` - byte 0 = active flag, byte 1 = countdown value. Bounds-checked against `gp[+0x2D4]`. Decrements the value, clamps at zero, clears the flag when the value reaches zero. Cited from field-VM-region helpers `FUN_801D71F0` / `FUN_801D7210` (the "actor poison/sleep/buff timer tick" path). |
| `800431FC` | Spell-list contains check: `(char_idx, spell_id) -> bool`. Walks `[char + 0x13d ..]` (count at `+0x13c`). |
| `80043264` | Equipment-slot contains check: `(char_idx, item_id) -> bool`. Walks `[char + 0x196 ..]` (8 slots). |
| `800432BC` | Spell-list insert (sister of `FUN_80042DBC` which removes). |
| `8004E2F0` | Battle range / line-of-sight: `(actor_a_id, actor_b_id) -> i16 distance`. Reads `[0x801C9370 + id*4]` for both, sums `+0x1F` size bytes, clamps per-tier. |
| `80054CB0` | Monster init: `(record, monster_slot)` populates `[0x801C9370 + (slot+3)*4]` from a monster record (HP/MP/stats/magic-resist + XP at `+0x230`). |
| `8005567C` | **Battle-id → formation-cell expander** (SCUS). Reads the transient battle-id `DAT_8007b7fc` and writes the formation cell `0x8007BD0C..0F`: a plain id fills slots 0/1/2 (`DAT_8007bd0c/d/e`), id ranges `0x07..0x09` / `0x49..0x4d` / `0x88..0x8b` / `0xa2..0xff` get bespoke multi-monster / boss expansions (slot 1+ from `DAT_8007bd10..`), and id `0` falls back to `[4,_,4,4]`. The **alternate** formation source to the `FUN_801DA51C` `actor[+0x94]` record path - used for battles cued by a battle-id rather than an entity record. The cell *shape* distinguishes them (this writes 3 slots for a plain id; `FUN_801DA51C` writes only as many slots as the record's count). See [`formats/encounter.md`](../formats/encounter.md#scripted-battle-id-path-fun_8005567c). |
| `80055B6C` | Battle-init (SCUS). Zeroes the per-battle scratch (`DAT_801C8FA0[0..0x10]`, `_DAT_8007bd08/34/38/44/48/4c`), then resolves the formation: when `DAT_8007b7fc != 0` it calls `FUN_80055B20` + `FUN_8005567C` (battle-id path); when `0` it only refreshes the cell from `FUN_8005567C` if the cell is empty (preserving an `actor[+0x94]`-installed formation). Calls `FUN_8005567C`. |
| `80055468` | Monster battle texture / CLUT pool loader: `(pool_ptr, tmd_ptr, wide_flag, slot)`. Builds a `StoreImage` RECT keyed on the battle slot - page at `(slot*0x40 + 0x140, 0x100)` (`= (slot*64 + 320, 256)`), width `0x20`/`0x40` fb-units per the wide flag - and calls `FUN_800583C8` twice to upload the 4bpp page and the CLUT region. The `_DAT_8007BD24+0x13` read selects the active battle slot for placement. Decoded into `legaia_asset::monster_archive`; see [battle](../subsystems/battle.md#monster-mesh-record-0x04). |
| `80055B4C` | Battle character display flag. Writes `_DAT_8007BD24+0x26B = char + 1`, `+0x26C = 0`. |
| `80050E2C` | Generic linear pointer-table search: `(table, char_id, count) -> idx_or_0xFF`. Used by battle UI lookups. |
| `801D0748` | Battle / level-up main tick (battle overlay). 11 KB / 2781 instructions / 26 outgoing. Per-frame driver for the battle + post-battle sequence. Reads sub-state byte at `_DAT_8007BD24[6]`; sub-states `0x1E`/`0x32`/`0x6E`/`0xFE` update camera yaw `_DAT_8007B792` from pad `DAT_1f800393`. Checks `_DAT_800846C8` (battle-active flag) and `_DAT_8007BD24[0x275]` (party-member count). After input handling calls `FUN_801D3444` + `FUN_801D9BBC`. Character-select input (L1/R1 = pad bits `0x2000`/`0x4000`/`0x1000`/`0x8000`) writes highlight byte to `(actor_table[n] + 0x1D)`. Captured as `overlay_magic_level_up_801d0748.txt`. |
| `801D388C` | Battle actor animation dispatcher (battle overlay). 7.8 KB / 39 callers. `(animation_type, param_2)`. Switch on `animation_type` (0..0x31+): cases 0/2 call `FUN_801DB318` and fall through; case 3 clears `actor[0x1E7]` and `actor[0x1DE]` for all 3 party slots; cases 5/7 compute `_DAT_80076D3A = func_0x80035F04(actor[0x1BC])` (animation-look-up into per-actor anim descriptor). Increments the battle frame counter at `_DAT_8007BD24[0x6B2]`. Actor pointers read from `DAT_801C9370/74/78`. Captured as `overlay_magic_level_up_801d388c.txt`. |
| `801D5854` | Battle actor pose driver (battle overlay). 6.5 KB / 47 callers. `(actor_slot, pose_id)`. Switch on `pose_id` (0..9+); pose 0 sets up a GTE transform from `actor[0x46]` (height), `actor[0x34/36/38]` (XYZ), scaled by `0x8F0 - actor[0x46]` and `DAT_8007BD10[slot]`-derived table entry. Pose 1 calls `func_0x80019B28` for angle-to-screen projection targeting `actor[0x1DD]`'s slot. Poses update `_DAT_8007BD24[0x87C]` via pad accumulator and clamp `_DAT_8007BD24[0x26E/270]` at 200. The `~30` call sites from `FUN_801E295C` match the action-SM's per-swing animation triggers. Captured as `overlay_magic_level_up_801d5854.txt`. |
| `801D8DE8` | Hottest battle utility (battle overlay). 3 KB / 77 incoming refs. |
| `801DA6B4` | Battle actor display-state controller (battle overlay). 204 bytes / 9 callers. `(visible)`. Walks battle actors 3..6 (`DAT_801C937C` array); for alive actors (`+0x14C != 0`): `visible=0` sets `actor[+0x21C] = 200` (opacity) and `actor[4] = 0x401004` (pose flags) for non-focused actors, `actor[+0x21C] = 5` for the focused one; `visible=1` clears `actor[+0x21C]` and `actor[+0x0C]`. `overlay_battle_action_801da6b4.txt`. |
| `801DB81C` | Next-valid-target scan (battle overlay). 152 bytes / 10 callers. Returns the next party slot after `_DAT_8007BD24[0x13]` whose battle actor has `+0x14C != 0` (alive) and `+0x16E & 0xF84 == 0` (no death/stone/silence). Used in level-up and action-select to advance the character cursor. `overlay_battle_action_801db81c.txt`. |
| `801DB8F4` | Actor status-flag write (battle overlay). 208 bytes / 6 callers. `overlay_battle_action_801db8f4.txt`. |
| `801DBDDC` | Battle timer ramp helper (battle overlay). 232 bytes / 4 callers. `overlay_battle_action_801dbddc.txt`. |
| `801E295C` | **Battle action state machine** - `ctx[7]` dispatch, `+0x1DE` sub-state. 16 KB / 4099 instructions / 155 outgoing calls (the largest function in the battle overlay). Outer switch on `_DAT_8007BD24[7]` (the action-state cursor; 47 cases across bands `0x14`/`0x28`/`0x32`/`0x3C`/`0x46`/`0x50`/`0x5A`/`0x64`/`0x68`/`0x6E`); inner switch on `actor[+0x1DE]` (action category 0..5 = Martial-arts / Item / Magic / Attack / Spirit / Run). Reads battle actor pointers via `(&DAT_801C9370)[ctx[0x13]]`; ramps frame-timer at `ctx[+0x6D8]`; queues animations via `actor[+0x1DA]` and waits on `actor[+0x1D9]` to converge. Battle-end signalled via `DAT_8007BD71 = 0xFE`. The global pseudo-action `case 0xFF` increments the battle-mode counter `_DAT_8007BD24[0x28A]` (the boss-phase gate the AI picker `FUN_801E9FD4` reads; ported as `engine-core::World::advance_battle_mode`). Cross-refs: `FUN_8004E2F0` (range/LOS, called from `0x14`/`0x16`/`0x19`), `FUN_80042558` ability bitmask (read indirectly via character record at `0x80084708 + (party_id-1)*0x414`), effect spawn via `FUN_801D8DE8` → `FUN_801DBF9C` → `FUN_801DFDF8`, pose driver `FUN_801D5854(actor, pose_id)` (~30 call sites). See [`subsystems/battle-action.md`](../subsystems/battle-action.md). Captured from an action-menu-open save state as `overlay_battle_action_801e295c.txt`. |
| `801DE914` | Effect-bundle init / pack-fixup (battle overlay). |
| `801DFDF8` | Effect-bundle public spawn API (battle overlay): `(byte effect_id, short* world_pos, ushort angle)`. |
| `801E0088` | Effect-bundle per-frame walker (battle overlay). |
| `801F17F8` | `summon.dat` / `readef.dat` streaming loader (battle overlay). |
| `801E9FD4` | **Monster-AI action picker** (battle overlay; the magic-capture-overlay dump at the same address is a different routine). Called per monster from `FUN_801DABA4`. Generic core: rolls `rand % (1 + live_magic_count)` over the record's `+0x21..=+0x23` global magic ids → physical strike or a cast (gated on MP `actor[+0x150]` vs `spell_table[id*0xC+3]`), target by shape `spell_table[id*0xC+2] & 0x60`. Then a `switch` on `DAT_8007BD0C[slot]` overrides with scripted casts. `DAT_8007BD0C[slot]` is the **per-slot monster id** (`FUN_801DA51C` copies the encounter record's `[+4+slot]` ids into it), so each `switch` case is bespoke AI for a specific monster id - not an abstract AI-type. Writes `actor[+0x1DD]` (target/class), `+0x1DE` (action kind), `+0x1DF..` (chosen id / SP chain queue). Generic core ported as `engine-core::World::pick_monster_action`; the per-monster-id switch + recent-target ring ported as `engine-core::monster_ai` (`decide` / `apply_recent_target_ring`, over `MonsterAiState`). `overlay_battle_action_801e9fd4.txt`. |
| `801E7320` | **Monster-AI target resolver** - the `monster_setup` hook (`FUN_801E295C` `ActionSeed`, gated on `actor[+0x16e] & 0x380`). Expands the targeting class in `actor[+0x1DD]`: class `0..2` → living monster slot (`rand % ctx[+1] + party`), `3..6` → living party slot (`rand % ctx[+0]`), `8`/other → `rand%3` gate for all-target codes `8`/`9` / self. Ported exactly as `engine-core::World::resolve_monster_target`. `overlay_battle_action_801e7320.txt`. |
| `801DABA4` | **`recompute_battle_order`** - picks the next actor to act. First loop zeroes the initiative key `+0x16c` of every dead actor (`+0x14c == 0`); then it scans the actor table for the living actor with the **highest** `+0x16c`, collecting ties, and selects one at random (`rand % tie_count`) into `_DAT_8007BD24[0x274]` (active actor). For a monster pick it drives `FUN_801E9FD4` (the AI action picker). Keys are seeded per round from SPD (`+0x164`) by `overlay_0897_801e23ec`: `+0x16c = speed + rand()%(speed/2+1) + 1`. Selection core ported as `engine-core::World::next_combatant_by_initiative` (with a round-robin fallback when no actor carries SPD); see [`subsystems/battle.md`](../subsystems/battle.md#auto-resolve-vs-player-driven). `overlay_battle_action_801daba4.txt`. |
| `801DA51C` | World-map / battle-entity SM (case 1 = encounter trigger). Fills the per-slot monster-id array `DAT_8007BD0C[slot]` from the inline encounter record at `actor+0x94` (`[+3]` = count, `[+4+slot]` = ids; the `docs/formats/encounter.md` format). `801da51c.txt`. |
| `80047430` | Battle actor-update routine (knockback / drift / damage integrator). Sets `actor[+0x16e] |= 0x380` (the "delegate this turn to the AI target resolver `FUN_801E7320`" flag) **only on party slots** (`actor[+0x5a] < 3`) whose character-record status word `+0x00` has bit `0x2000` (Confuse/Charm) - it routes a charmed party member through the monster-AI resolver, not a monster behaviour. Normal monsters keep `0x380` clear, so `monster_setup` stays dormant for them and the picker's `!ai380` scripted-cast cases fire. `80047430.txt`. |

### Ra-Seru capture overlay

All 78 functions dumped as `overlay_magic_capture_<addr>.txt`. Loaded during the
Ra-Seru capture mechanic (Gimard and other Ra-Serus); captured from a save state
taken during the capture animation. Shares actor struct layout
with the regular battle overlay (`_DAT_8007BD24` context pointer, `+0x1DE`
sub-state, `+0x07` action-type).

| Address | Role |
|---|---|
| `801D0748` | Capture outer dispatcher (11 KB, 26 outgoing). Same sub-state structure as the battle outer dispatcher; sub-states `0x1E`/`0x32`/`0x6E`/`0xFE` update camera yaw. `overlay_magic_capture_801d0748.txt`. |
| `801D388C` | Capture animation dispatcher (7.8 KB, 39 callers). Same interface as the battle overlay's `FUN_801D388C`. `overlay_magic_capture_801d388c.txt`. |
| `801D5854` | Capture actor pose driver (6.5 KB, 47 callers). Same interface as the battle overlay's `FUN_801D5854`. `overlay_magic_capture_801d5854.txt`. |
| `801D8DE8` | Hottest capture utility (3 KB, 75 callers). JT dispatcher; only callee is `FUN_801DB7B0` (the generic 4-byte JT helper). `overlay_magic_capture_801d8de8.txt`. |
| `801E295C` | **Capture battle state machine** (16.4 K-, 19 outgoing). Outer switch on `_DAT_8007BD24[7]` cases `0xB`/`0xC` (capture-specific action types). Inner switch on `actor[+0x1DE]`. Distinct from `overlay_battle_action_801e295c.txt` despite sharing the same entry address. `overlay_magic_capture_801e295c.txt`. |
| `801EC3E4` | Large capture helper (10 KB, 0 incoming — top-level from game-mode dispatch). Calls `FUN_801E91E8`. `overlay_magic_capture_801ec3e4.txt`. |
| `801E9FD4` | Capture sub-system (8.5 KB, 1 incoming). Calls `FUN_801EC0DC`. `overlay_magic_capture_801e9fd4.txt`. |

## Script VMs

| Address | Role |
|---|---|
| `801D6628` | Actor / sprite VM (title-screen overlay). 13 opcodes; dispatch table at `0x801CED70`. |
| `801D6704` | **Field/town per-scene initializer** (the body of mode-2 "MAIN INIT", invoked by `FUN_80025B64`; also every field scene transition). Reads the resident map id, loads geometry + MAN (`FUN_8003AEB0`) + camera + fog + BGM via the field loader `FUN_8001F7C0`, allocates the game-mode work buffer, calls `FUN_80020224` at `0x801D6B0C`, and ends by writing `_DAT_8007B83C = 3` to hand off to the field per-frame loop. Debug strings `map_name`/`map_read`/`man_set`/`camera_set`/`fog_set`/`tmds: %d`/`game_mode`/`program_mode`. The title NEW GAME path lands here via mode 2. |
| `801CF650` | Emitter ramp-actor allocator (town overlay). Calls `FUN_80020DE0(base + 0x27EC)`, configures the actor's curve / ramp slots: `+0x50 = sub_id`, `+0x6C = mode_byte`, `+0x80 / +0x8C = curve_table[curve_id] << 16` (table at `_DAT_1F80035C`), `+0x84 = (target << 17) / (duration + 1)`, `+0x88 = abs / duration`. Shared helper used by 0x43 sub-0x10 / sub-0x12 emitter setup ops. |
| `801DB7B0` | Generic 4-byte jump-table dispatcher (town overlay). 7 instructions: `(*(table[v1])(...))()` where table base = `v0 - 0xD6C`. Caller sets `v0` (lui-immediate) and `v1` (index). |
| `801DE840` | **Field / event script VM** (town/field overlay). 17.5 KB / 357 outgoing calls. The largest function in the corpus. See [`subsystems/script-vm.md`](../subsystems/script-vm.md). |
| `801D01B0` | **Player free-movement locomotion controller** (field overlay). 1964 bytes. Camera-remaps the held pad (`func_0x800467e8` + `FUN_80046494` → direction bits `& 0xf000`), computes per-frame speed (`base_step * player[+0x72] >> 12 * DAT_1f800393`, terrain-slow + diagonal modifiers), steps the player position 2 units/iteration with per-axis collision (`FUN_801cfe4c`), sets facing `player[+0x26]`. Gated off by `player.flags & 0x80000`. Pinned by write-watchpoint `autorun_player_pos_watch.lua`. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). |
| `801CFE4C` | **Field collision check** (field overlay). `FUN_801cfe4c(player, scene, dir)` → `0` clear / `2` wall. Samples the per-scene walkability grid at `*(_DAT_1f8003ec) + 0x4000` (one byte per 128-unit tile, high nibble = 4 sub-cell wall bits); direction probe offsets in tables `DAT_801f21b4` / `DAT_801f2214`. Sibling sampler `FUN_801d5718`. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#collision--fun_801cfe4c). |
| `8003AEB0` | **Field/town scene-entry map-init** (SCUS). Debug strings `town_mode` / `baria_mode` / `walking_set`. Fills the 16-entry floor-height LUT at scratchpad `0x1f80035c` from the MAN header (`_DAT_8007b898 + 2`, 16 negated `short`s); ORs the `0x400` footprint flag into the `+0x8000` per-tile attribute map from field-pack records (`+0x12000`, offset/count at `+0x12006`/`+0x12008`); configures the player actor `_DAT_8007c364` for free movement (`+0x72 = 0x1000`, `+0x6a = 8`); then calls `FUN_8003a55c` to spawn the scene's objects/NPCs. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#where-the-collision-grid-comes-from). |
| `8003A55C` | **Object/actor spawn iterator** (SCUS). Walks the `+0x8000` per-tile attribute map (`u16`/tile, `0x80×0x80`); for each cell whose low 9 bits index an object record (`+0x0000` table, 0x20 stride) flagged `[+0x12] & 4`, spawns the actor via `FUN_80024c88`, adds the floor-height LUT value `LUT[(+0x4000 byte) & 0xf]` to its Y, and attaches its field-VM script. The read of the `+0x4000` low nibble is what pins it as a floor-elevation tier. |
| `801E00F4` | Field-VM dispatcher switch table. |
| `801E0C3C` | Field-VM outer-op `0x4C` second-stage dispatcher: re-reads byte 1 of the bytecode, takes `byte1 >> 4`, and routes through the 16-entry JT at `0x801CEE60`. The combined `0x4C <byte1>` family covers menu / party / camera / scene-state writes; per-nibble handlers re-dispatch on `byte1 & 0xF`. |
| `801E3040` | Field-VM `0x4C` outer-nibble-`0xE` dispatcher (reached via the `0x801CEE60` JT entry 14). 15-entry sub-JT at `0x801CF008` indexed by `byte1 & 0xF`. Cluster covers misc scene writes, FMV trigger, camera animate / zoom, etc. |
| `801E30E4` | Field-VM `0x4C 0xE2` (FMV trigger). Writes `_DAT_8007BA78 = (s16)bytecode[2..3]` (FMV index for the runtime table at `0x801D0A6C`) and pokes `_DAT_8007B83C = 0x1A` (next game mode = 26 = `StrInit`). PC += 6 from byte 1 (op total 7 bytes); trailing 3 bytes are reserved. See [`subsystems/cutscene.md`](../subsystems/cutscene.md#field-vm-fmv-trigger-op). |
| `801CF098` | STR/MDEC FMV main play loop (str_fmv overlay). 1236 bytes / 309 instructions / 9 outgoing calls — the largest function in the captured slice. Takes `(int loop_mode, &runtime_fmv_state)`; called from `0x801CECA0` with `param_2 = 0x801D0A6C + (s16)_DAT_8007BA78 * 64`. Drives the per-frame `CdReadFile` → `StrFrameAssembler` → MDEC → blit pipeline; reads `_DAT_8007BA78` again at `0x801CF4E0` as an early-abort flag. |
| `8003CE9C` | Field-VM context resolver: `(s16)*(u16*)param_1`. Reads a little-endian 16-bit value at the bytecode pointer and sign-extends. Used by every field-VM op that takes an `s16` operand (BGM id, FMV id, ramp targets, etc.). |
| `801F5748` | Inventory / menu hub (town overlay). 11 KB / 192 calls. |
| `801EAD98` | Field subsystem hub (town overlay). 5.9 KB / 35 calls. |
| `801ED710` | Battle records / stats screen renderer (field overlay). 2 KB. See Records section below. |
| `801DAB90` | GTE camera-matrix transform helper (field overlay). 2.4 KB / 3 callers. `(src_transform, dst_transform)`. Negates src's `+0x14/+0x18/+0x22` into dst; copies `_DAT_8007B790` (camera X) and `_DAT_8007B792` (yaw) into dst`+2/+6`. Saves and restores 16 GTE SPU-matrix registers at `0x1F800314+0x48` while calling `func_0x80019278(src_transform)`. When `DAT_8007B607 >> 4 == 4`, resolves camera ground position via `func_0x80019B28` from pad-corner analog values. Captured as `overlay_cutscene_dialogue_801dab90.txt`. |
| `801EF2B0` | Controller input handler (town overlay). 1.9 KB / 29 calls. |-
| `801DD35C` | Top-level menu dispatcher (menu overlay). 12 KB / 3026 instructions / 134 outgoing calls. Sets text-actor depth slot `_DAT_8007B454 = 7` and `DAT_80073F20 = 0x10`; reads active-submenu index from `_DAT_8007BAB4`. Loaded only when the in-game item / magic / equip menu is open — captured via an item-menu-open save state as `overlay_menu_801dd35c.txt`. |
| `801D33D8` | Submenu rendering loop (menu overlay). 5.3 KB / 107 outgoing calls. |
| `801E1C1C` | Shared menu-element renderer (menu overlay). 4.5 KB / 8 incoming refs. |
| `801CF650` | Equipment stat aggregator (menu overlay). 272 bytes / 10 callers. `(char_slot)`. Walks the 5 equipment bytes at char record `+0x196`; for each non-zero slot looks up the item entry at stride `0xc` from `0x8007433C` (item table); if `entry[0] == 1` (equippable type), reads a stat-bonus row at `entry[1] * 8` from `0x8007EF68` and accumulates into `DAT_801EF08C/090/094/098/09C` (STR/INT/DEF/LUCK/…). Called by menu subscreen equipment-stat display. `overlay_menu_801cf650.txt`. |
| `801DD0C0` | Item category / slot validity check (menu overlay). 108 bytes / 2 callers. `(slot_id, item_type, flag) -> u32`. Walks the category table at `DAT_801E4B88` (byte pairs: type, bitmask); returns `0` if item_type not found or bitmask bit `(slot_id + flag*4)` is clear, else `1000`. `overlay_menu_801dd0c0.txt`. |
| `801F03F0` | **Name-entry SM** (field/dialog overlay). The *"Select your name."* character-grid screen. Substate at `struct+0x54`, dispatched through a 5-entry jump table at `0x801CF71C`: init (`0x801F0444`) → interactive (`0x801F0480`) → three confirm handlers (`0x801F095C`/`09C0`/`097C`). Interactive applies d-pad deltas `-0x11`/`+0x11`/`+1`/`-1` over a 7×17 nav grid (cursor `0x8007BB88`, wrap modulo `0x77`), skips `|`=`0x7C` separators, appends glyphs (pixel-width cap `0x39`), and runs the control row's Backspace/Space/End sentinels (`0x66`/`0x64`/`0x65`); End → Yes/No confirm → commit into the char record's name field at `+0x2A7` (save-block `+0x86F` for slot 0). Charset grid at `0x801F29F0`, name buffer at `0x801F2A6C`, prompts at `0x801CF698`+. The **field-VM op that opens this overlay** during the `town01` opening is pinned: op `0x49` STATE_RESUME sub-op 3 at partition-2 record 3 body offset `0x02c6` (`49 03 00`), whose `op49_invoke_setup` (`func_0x80020de0(0x8007065c, _DAT_8007c34c)`) suspends the field script and hands off here (save-correlated via `_DAT_8007B450` = that op's RAM address + 1). Ported clean-room as `legaia_engine_core::name_entry`. See [`subsystems/boot.md`](../subsystems/boot.md#name-entry-overlay). |
| `801E6B34` | Name-entry **renderer** (field/dialog overlay). Draws the 6×17 grid (skipping `|` / space) via glyph drawer `FUN_80036888`, plus the working name, the blinking `Vahn_` caret (measured via MES `FUN_8003CA38` + width `FUN_80035F04`), and the box frames (`FUN_8002C69C`). Called by the interactive substate of `FUN_801F03F0`. |

### Actor / sprite-VM SCUS callees (invoked from `FUN_801D6628`)

The actor / sprite VM at `FUN_801D6628` (13 opcodes, 4-byte instruction, the smallest VM in the corpus) dispatches every opcode through this cluster of SCUS-side helpers. All seven share a doubly-linked-list lookup pattern: they walk `*(gp + 0x148)` (the active-actor list head) comparing each entry's `+0x4` field against `param_1` (actor id). The Rust port at `crates/engine-vm/src/lib.rs` abstracts each one as a `Host` trait method; engines wire whatever per-actor record they have.

| Address | Role |
|---|---|
| `80035334` | Actor-exists query. 24 instructions. `(actor_id) -> *list_entry or 0`. Walks `gp+0x148`'s doubly-linked list comparing `*(entry + 8) == actor_id`; returns the matching list entry pointer, or `0` when no match. Used by every opcode that needs to gate on "does this actor have a slot". Maps to [`Host::actor_exists`]. |
| `800326AC` | Actor spawn at default position. 230 instructions. `(actor_id, table_entry_ptr) -> *new_entry`. Reads the per-actor table entry's kind byte (`param_2[1]`, 0..=N switch), computes a default world position from the kind-specific formula (uses scratchpad `_DAT_1F800388 / _DAT_1F80038E / _DAT_1F80038e+0xC`), and links a fresh list entry into `gp+0x148`. Maps to [`Host::spawn`]. |
| `800357FC` | Set actor position (snap). 30 instructions. `(actor_id, x, y)`. Calls the standard list-lookup; if found and the sub-record at `entry[+0x24]` is live (`*sub != 0`, `entry[+0x20] != -2`), writes the new (x, y) into the sub-record's position slots. No-op when the actor is missing or sub-record is null. Maps to [`Host::set_position`]. |
| `800358C0` | Start actor motion / glide. 30 instructions. `(actor_id, x, y)`. Same lookup as `800357FC`; on match writes (x, y) to BOTH the source slots (`entry[+0x0A]`, `sub[+0xA]`, `sub[+0x6]`) and the target slot - i.e. resets the motion source to current position then queues a glide toward target. Maps to [`Host::start_motion`]. |
| `80035978` | Delete actor sprite. 41 instructions. `(actor_id)`. List lookup; on match, when `entry[+0x20] == 0` writes `entry[+0x20] = 0xFFFF` and `entry[+0x1E] = 0` (state machine = "marked deleted, awaiting cleanup"). Maps to [`Host::delete_sprite`]. |
| `80035A4C` | Global per-frame actor-system tick. 200+ instructions. Outer loop walks `gp+0x148` once; per-entry inner loop walks the same list again searching for a "linked" entry whose state needs advancing. Cleans up deleted entries (`state == 0xFFFF`), advances motion source toward target, and ages out the per-entry cooldown counter. Maps to [`Host::global_update`]. Invoked by op `0x05 GlobalUpdate`. |
| `800319A8` | Trigger actor effect. 79 instructions. `(actor_id)`. List lookup; on match, when `entry[+0x18]` is non-zero and the kind byte at `entry[+0x1C]` matches one of the in-game effect kinds (filter: `bVar1 > 1 && bVar1 != 0xD && bVar1 != 0x11`), calls `FUN_80017B94` (the effect-trigger leaf). Followup state cleanup writes `entry[+0x20] = 0xFFFF` when `entry[+0x24]` is non-zero. Maps to [`Host::actor_effect`]. |

### Field-VM helpers (callbacks invoked from `FUN_801DE840`)

These are the SCUS-callee leaves the field-VM dispatcher reaches via per-opcode trait-method hooks in `crates/engine-vm/src/field.rs`. Each lives in the field/town overlay (`overlay_0897.bin`) and is also present in every sibling overlay that paged in the same scripting cluster (`overlay_cutscene_dialogue`, `overlay_baka_fighter`, etc. — the captured dumps with matching entry addresses are the cleanest reference).

| Address | Role |
|---|---|
| `801D596C` | Party-state init helper. 46 instructions. Walks the `_DAT_8007C34C` actor list via `func_0x8003CF04(list, FUN_801D2EBC)` looking for an existing match; if found, sets `actor[+0x10] \|= 0x8`. When the global gate `_DAT_800845B8` is non-zero and no match exists, allocates a fresh actor from pool `0x801F22DC` and seeds it with `+0x14=0xC0, +0x16=0x10, +0x26=0xFFF0, +0x54=0, +0x6A=0x200`. Invoked by field-VM op `0x4C 0xD3` (party-state setup). |
| `801D835C` | Actor-clone helper for op `0x4C` sub-1 sub-op `0x14`. 48 instructions. `(src_actor, param_2, param_3)`. Spawns a duplicate actor from pool `0x80070644` via `func_0x80020DE0(pool, _DAT_8007C34C)` and copies six u32 slots from `src+0x14..+0x4F` plus `src+0x68`; writes `dst+0x54 = param_3`, `dst+0x74 = param_2`, and sets the scene-global `_DAT_80070648 = src[+0x64]`. |
| `801DB8EC` | **Camera focus + projection setter** (field overlay; the headless Ghidra overlay dump truncates it — disassembled from live overlay RAM in the `new_game_cutscene_intro_a` save state). `(transform_ptr)`. When `DAT_8007B606 != 0`: calls `FUN_801DAB90(transform_ptr, 0x801F3580)` (builds the camera-param struct), walks a typed-record table at `_DAT_801F2798` / `&DAT_801F279C` (type `0x2` = u16 copy, type `0x4` = sign-extended u16 → i32 copy), then **sets the GTE H register** from `_DAT_8007B6F4` via `func_0x8003D254` (= `setCopControlWord(2,…)`) and **writes the focus globals** `_DAT_80089118`/`_DAT_80089120`: when the camera-mode byte `DAT_8007B607 >> 4 == 5` (scripted) the focus is tile-derived `-(tile << 7) - 0x40` from `_DAT_8007B61C`/`_DAT_8007B624`; otherwise (anchor-follow) `-(anchor+0x14)` / `-(anchor+0x18)`. So this is the focus + FOV setter, not the eye-position build — the camera has no explicit eye-distance (see [`subsystems/cutscene.md`](../subsystems/cutscene.md) op-`0x45`). Invoked by field-VM op `0x4C 0x39` and op `0x4C 0x3E`. |
| `801DD9D4` | Per-actor GPU-prim emitter. 69 instructions. `(actor_ptr)`. Builds a polygon-draw header `0x05000000` + flat-color packet `0x28808080` at `_DAT_1F8003A0` (OT chain head), copies `actor[+0xB8..+0xBC]` as RGB into the packet, then iterates a jump-table at `0x801CEC40` calling `func_0x8003D2C4(_DAT_1F8003F4 + actor[+0x50]*4)` once per slot. Used as the predicate callback the field-VM hands to `func_0x8003CF04` from op `0x43 0xE` (mark currently-iterating actor with flag bit `0x8`). |
| `801DDFE4` | Camera init helper. 3-instruction tail-call wrapper: writes `local_stack[+0x10] = 0x100` then jumps to `0x801EC96C` → `FUN_801D6274`. Sets up the 256-tick camera-config preroll consumed by `FUN_801DE084`. Invoked from the field-VM op `0x45` CAMERA CONFIGURE prelude. |
| `801DE084` | Camera apply / commit (op `0x45` CAMERA CONFIGURE apply path). `(struct, apply_trigger)`: when `apply_trigger == 0`, reads the 10 op-`0x45` param slots from the staging struct (`+0x02 + i*4`) and commits each to a camera global - param 0/1/2 → `_DAT_8007b790` (**pitch**, GTE `RotMatrixX`) / `b792` (**yaw**, `RotMatrixY`) / `b794` (**roll**, `RotMatrixZ`); params 3/4/5 → `_DAT_800840b8/bc/c0` (shake/offset); params 6/7/8 → `_DAT_80089118/1c/20` (the **negated camera focus** X / Y-direct / Z, = the GTE translation `-focus`); param 9 → `_DAT_8007b6f4`, passed to `func_0x8003d254` = `setCopControlWord(2, …)` (the GTE H projection register = focal length / zoom). When `apply_trigger != 0` it tail-calls `FUN_801DD310` instead. The `FUN_801DAB90` build path is the inverse (camera-actor state + globals → this struct). Captured as `overlay_cutscene_dialogue_801de084.txt`. (The `overlay_0897_801de084.txt` dump is a *different* function - overlay 897 has an itoa routine at this address; the camera commit lives in the field / cutscene overlays.) |
| `801DE2B0` | Op `0x34` sub-1 "capture-PC for existing actor" allocator. 51 instructions. `(operand_table_ptr, packed24)`. Allocates an actor from pool `0x801F2888` via `func_0x80020DE0`; copies 9 u16 fields from the operand table into `actor[+0xB8..+0xC6]` / `actor[+0xD2..+0xD4]` and writes the packed-24 value to `actor[+0xD6]`. The trailing actor returned by `FUN_801DE2B0` is what the field-VM stamps a captured-PC payload onto. |
| `801DE3E0` | Sub-tile broadcast helper. 6-instruction wrapper. Calls `func_0x80035A4C(0x37)` (sound trigger id `0x37`), writes `*(active_ctx + 0x54) = 2` (move-substate), then tail-calls `FUN_801ECCAC`. Invoked by field-VM op `0x4C` sub-3 sub-8 / sub-D / sub-C4 (player subtile refresh + sub-tile broadcast). |
| `801E4C58` | Op `0x4C n6 sub-0x61` 16-byte halt-acquire emitter (baka_fighter dump: 77 instructions). `(captured_pc, operand_ptr)`. Reads `func_0x8003CE9C(operand+0xD)` (height gate). When zero, reads two more s16s and dispatches between `func_0x800583C8` (sprite emit, 16-element local buffer filled with `operand+5`) and `func_0x80058490` (MoveImage VRAM-to-VRAM copy) based on the post-`func_0x8003CE9C(operand+7)` value. When non-zero, allocates an actor from pool `0x801F2918` and stores `(captured_pc, operand_ptr)` at `+0x94 / +0x90`. |
| `801E573C` | Op `0x4C n8 sub-6` actor allocator with 6-axis rotation matrix (baka_fighter dump: 45 instructions). `(captured_pc, ctx_ptr, x, y, z, rx, ry, rz)`. Allocates an actor from pool `0x801F2948` via `func_0x80020DE0`; stores `captured_pc` at `+0x90`, `ctx_ptr` at `+0x94`, and the six i16 position+rotation values at `+0x80..+0x8A`. Returns silently when the pool is exhausted. |

## Field-locomotion math helpers

SCUS-side leaves the field free-movement controller `FUN_801D01B0` and the cutscene/world-map camera helper `FUN_801DAB90` reach for pad-direction remapping, footprint collision, floor-height interpolation, and bearing. The first two are the camera-relative pad decode pair; the last two are the GTE-input geometry helpers. All sample the per-scene grid at `_DAT_1F8003EC + 0x4000` (walkability) / `+0x8000` (per-tile attributes) and the floor-height LUT at `_DAT_1F80035C`. See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md).

| Address | Role |
|---|---|
| `800467E8` | **Camera-relative pad-direction remap** (in place). `(pad_word_ptr)`. When the camera-azimuth quadrant `gp+0x2D8` is non-zero and the held mask carries a direction (`& 0xF000`), finds the held direction's index in the 8-entry direction table at `DAT_800766FC`, rotates it by the quadrant, and rewrites the direction nibble of `*pad_word_ptr` so "screen up" maps to the correct world axis regardless of camera angle. Both the field controller and the tile-board walker call it. Cited from `crates/engine-core/src/world.rs` + `tile_board.rs`. `see ghidra/scripts/funcs/800467e8.txt`. |
| `80046494` | **Movement-direction + footprint collision resolver.** `(player_ptr)`. Reads the (already remapped) held mask at `gp+0x538`; for diagonals (`0x9000/0xC000/0x3000/0x6000`) passes the mask through unchanged, otherwise walks the direction table at `DAT_800766BC`, probes walkability at three offsets per candidate via `func_0x801D56C4`, accumulates the per-axis hit bits, and slides the resolved direction along walls using the offset tables at `DAT_800766DC/EC`. Returns the movement-direction mask in bits `& 0xF000`. Sibling of `FUN_800467E8`; together they decode locomotion input for `FUN_801D01B0`. `see ghidra/scripts/funcs/80046494.txt`. |
| `80019278` | **Floor-height / collision-Y resolver.** `(transform_ptr)`. Reads the actor's tile coords from `+0x14/+0x18`, samples the per-tile attribute word at `_DAT_1F8003EC + 0x8000` to set the render flag bits in `+0x10`, then bilinearly interpolates the 16-entry floor-height LUT at `&DAT_1F80035C` across the four sub-cell corner nibbles of the `+0x4000` walkability grid (with a ramp-tile fast path via `func_0x801D5630`). Returns the world-Y for the transform. Called by `FUN_801DAB90` (camera matrix) and the locomotion/collision step. `see ghidra/scripts/funcs/80019278.txt`. |
| `80019B28` | **Atan2-style bearing resolver.** `(x1, y1, x2, y2) -> u12 angle`. Computes the bearing from point 1 to point 2 as a 12-bit angle (`0..0x1000`) by quadrant-selecting on the signs of `dx`/`dy`, dividing the minor by the major axis, and looking the ratio up in the arctangent LUT at `DAT_8006F4C8`. Used for camera ground-position resolution (`FUN_801DAB90`) and per-actor face-target math. Ported as part of the motion VM; cited from `crates/engine-vm/src/motion_vm.rs` (`// PORT: FUN_80019b28`) and `move_vm.rs`. `see ghidra/scripts/funcs/80019b28.txt`. |

## Renderer

| Address | Role |
|---|---|
| `8002735C` | Legaia TMD renderer. 60 GTE ops; per-mode descriptor table at `DAT_8007326C`. Reached as the **landmark** emit leaf via `FUN_8001ADA4` case-5 — each landmark TMD in a kingdom slot-1 pack passes through here. The bulk world-map continent does **not** flow through this path; it flows through `FUN_80043390`'s per-prim dispatcher (textured-TMD default for case-5), which mode-switches to overlay-resident fog leaves when the world-map overlay is paged in. Cmd byte read from `DAT_8007326C`, so static `addprim` hunters miss both. |
| `8001ADA4` | Per-actor RENDER dispatcher (2456 bytes). Switch on `actor[+0x56]` (render mode 1..0xB). Case 4 (multi-target): dispatches on `actor[+0x9e]` flags - bit `0x4000` → `FUN_8002A5A4`, bit `0x2000` → `FUN_801CFA48` (overlay-resident), else → `FUN_80028158`. Case 5 (full TMD): iterates the mesh chain at `actor[+0x44]` (`puVar5[0]`=count, `puVar5[1..n]`=mesh ptrs) and calls `FUN_80043390` (textured) / `FUN_80029888` (env-mapped, when `actor[+0x7a] != 0`) / `FUN_8002735C` (bone-animated TMD). Called 6x per frame via the `FUN_8001D140` wrapper against the same actor lists as the tick pass. |
| `8001D140` | Tiny stack-swap wrapper (`_DAT_1F8002BC = scratch; jal FUN_8001ADA4`). Called 6x per frame from `FUN_80016444` against `_DAT_8007C34C..0x36C` — the render-pass counterpart to the tick-pass `FUN_8002519C`. |
| `8002519C` | Per-frame actor-list TICK iterator (328 bytes). Walks the linked list, calls `actor[+0x0c]` (tick fn). Called 5x per frame from `FUN_80016444` against actor lists at `_DAT_8007C34C..0x36C` (different render passes). Distinct from `FUN_8001D140` (render pass). |
| `8002C69C` | HUD / dialog / menu sprite-batch emitter. 10 `cmd=0x2C` (POLY_FT4) lui/li sites in SCUS — the most prolific addprim emitter on a static scan. All callers pass small counts (`a3 = 0xb..0x44` = 11..68 prims each); total across all world-map call sites is ~120 prims. UI text rows, dialog frames, dev-menu strips. NOT the bulk continent emitter. |
| `800460AC` | GTE billboard fan helper. Loads 3 vertices via SVTX0/1/2 with-`(X-0x20, Y, Z), (X, Y, Z), (X+0x20, Y, Z)`, runs RTPT (cop opcode `0x280030`) 3 iterations decreasing Z, stores SXY/SZ at scratchpad `0x1F8002FC..`. Stage decoration / billboard sprite projection. |
| `80059BD4` | **VRAM image/CLUT upload (LoadImage-equivalent).** `FUN_80059bd4(a0 = RECT*, a1 = src_ptr)` where `RECT` is `[+0]=x, [+2]=y, [+4]=w, [+6]=h` (shorts) clamped against the VRAM extent at `0x8007_8D58`/`0x8007_8D5A`. Sends GP0(`0xA0`) "copy CPU→VRAM" then streams `a1` to the GP0 data port (CPU-FIFO loop at `0x80059D78`) or sets up DMA channel 2 (`MADR=a1` at `0x80059DBC`, `BCR`, `CHCR`); the GPU register pointers live in BSS at `gp-0x71D8`(GP1)/`-0x71DC`(GP0)/`-0x71D4`(D2_MADR)/`-0x71D0`(D2_BCR)/`-0x71CC`(D2_CHCR). Pinned by a read-watchpoint on Vahn's CLUT source (`0x800E96A0`) + the dump. Hook its entry to capture every VRAM upload's `(dest rect, source ptr)` — the [`autorun_clut_upload_hook.lua`](../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) probe filters to the character CLUT band (dst rows 488..499) and dumps each source. The character CLUT band (rows 488/490/497/498/499 + the row-495/496 effect sub-CLUTs) flows through here from scattered RAM sources; **Vahn's row-490 source is the resident field-scene buffer at `0x800E9690`**. |
| `800198E0` | **Per-TIM VRAM uploader + texpage→CLUT-row recorder** — the battle/scene texture-install leaf, called by `FUN_800520F0`'s per-descriptor loop (and `FUN_8001F05C`). Takes an asset chunk `[type, flags, clut-block, image-block, data...]`; if `flags & 8`, uploads the CLUT block via `LoadImage` (`FUN_800583C8`) at the chunk's **declared** `(x,y)`, then uploads the image block. **Crucially**, after the image upload it writes `table_0x8007BEC0[ ((img_x>>6) + (img_y>>8)*0x10) & 0x1f ] = clut_y` — i.e. it records the CLUT's VRAM row keyed by the image's **PSX texpage** (`(y/256)*16 + (x/64)`). So uploaded textures register "texpage T's palette is at row clut_y". This is the **CLUT-row "relocation"**: there is no per-battle dynamic VRAM allocator — each scene's character TIMs simply declare their own rows, the upload puts the CLUT there, and this table lets the renderer resolve a primitive's CLUT row from its texpage at draw time, **overriding the TMD's nominal CBA row**. Different scenes declaring different rows for the same character (mc2 vs a map01 battle) is why the party CLUT band shifts between captures. |
| `0x8007BEC0` (data) | **Texpage→CLUT-row table** (32 × `u16`). Written by `FUN_800198E0` at each textured upload (`[texpage & 0x1f] = clut_row`); read by the battle render path (`FUN_8004AD80` / `FUN_8004CE2C`) to resolve a primitive's CLUT **row** by its texpage. The TMD2's stored CBA supplies the CLUT **x** (sub-CLUT) but the **row** comes from here — the mechanism behind the battle-form party palettes appearing at scene-specific rows rather than the disc-nominal 490..495. |
| `800583C8` | **PsyQ `LoadImage(RECT *rect, u_long *data)`** (dev string `"LoadImage"`). Enqueues a CPU→VRAM transfer: calls the GPU-queue add (`FUN_8005A1C0`) with `handler = table[8] = FUN_80059BD4`, the rect, and the source data. Sibling wrappers in the same cluster are `ClearImage` (`FUN_80058298`/`8005832C`), `StoreImage` (`FUN_8005842C`), `MoveImage` (`FUN_80058490`), `DrawOTag` (`FUN_80058704`), `PutDrawEnv` (`FUN_80058778`), `DrawOTagEnv` (`FUN_8005887C`) — Legaia's statically-linked PsyQ libgpu. |
| `8005A1C0` | **GPU command-queue enqueue** (PsyQ-style). `FUN_8005A1C0(handler, rect/data, inline_size, src)`: when the ring has room, writes ring entry at `0x801C9590 + tail*0x60` = `{[+0]=handler, [+4]=rect (or inline copy of `inline_size` bytes), [+8]=src}`, bumps tail `0x80078E58`, and kicks `FUN_8005A4A0`. The flusher dispatches `entry[+0](entry[+4], entry[+8])`. Head `0x80078E5C` / tail `0x80078E58`; reset by `FUN_8005A78C` / `FUN_8005AA64`. |
| `80078D0C` (data) | **GPU-op handler dispatch table** (op-type → handler fn-ptr; 18 entries). Index 8 = `FUN_80059BD4` (image/CLUT upload), 9 = `FUN_8005A4A0`. `0x80078D4C` holds a pointer back to the table base (the live table ref the enqueuer/flusher dereference). The enqueuer (`FUN_80057C44` materialises the base; `FUN_800589D0` etc. read `0x80078D4C`) looks up `handler = table[op_type]` and writes `{handler, rect, src}` into the upload ring. So GPU uploads are queued **by op-type**, not by passing the handler — which is why no LUI+ADDIU site materialises `0x80059BD4`. |
| `8005A4A0` | GPU upload-**queue flusher** (748 B). Drains the ring at base `0x801C9590` (0x40 entries × 0x60 B; `[+0]`=handler, `[+4]`=rect, `[+8]`=src), head idx `0x80078E5C` / tail `0x80078E58`, and `jalr`s `entry[+0]`. **The battle character CLUTs are sourced from the active field scene's decompressed sec0 TIM_LIST** (LZS on disc): every slot-5 battle CLUT upload (VRAM rows 490/495/496/497/498/499) is byte-present in `0086_map01` sec0 decompressed, and renders as a character palette. They upload through op-type 8 to **dynamically-allocated VRAM rows**, so the disc TMD2's nominal CBA (e.g. Noa→492) is relocated per-battle to the allocated row — explaining why mc2 shows the party at rows 492/494 while a map01 battle uses 490/495..499. Original entry below: Drains a ring of pending upload requests (head/tail indices at `gp-0x71A4`/`-0x71A8`, mod `0x40`), waits on GPUSTAT bit `0x4000000`, and for each entry indirect-`jalr`s the handler (`FUN_80059BD4` for image/CLUT uploads) with the queued `(rect, src)`. So upload sources are set by whatever **enqueues** into this ring, then flushed here once per frame. The character CLUT band is enqueued during battle-actor render (only when the party characters actually render — not at battle-init), which is why headless probes that can't drive real combat never observe the Noa/Gala (rows 492/494) uploads even though Vahn/effects (490/495..499) flow through every frame. |
| `0x8007326C` (data) | Per-prim-mode descriptor table. 6 entries × 8 bytes — see [`formats/tmd.md`](../formats/tmd.md). |
| `0x8007C018` (data) | Global TMD pointer table. Installed by `FUN_80026B4C` (asset-dispatcher case 2 per-TMD; `sw a0, 0(v1)` where `v1 = lui+addiu(0x8007C018) + idx*4` — Ghidra's static-xref misses the store because the intermediate `addu` defeats constant propagation). Counters: `DAT_8007B774` (write/next-free), `DAT_8007BB38` (walk). Each entry points to a TMD blob with magic `0x80000002`; `+0x8` is `group_count`, `+0xC..` is the `count × 0x1C-byte` group descriptors. Consumed by `FUN_80021B04` (actor allocator), `FUN_801D77F4` (overlay actor allocator + vertex copy), `FUN_801D8280` (table walker), `FUN_801F69D8` (world-map top-view tile dispatcher), `FUN_8001E890` (per-pack count override). See [`formats/world-map-overlay.md`](../formats/world-map-overlay.md#dat_8007c018--global-tmd-pointer-table-the-actual-cluster-a-source). |
| `80026B4C` | Per-TMD installer. Verifies TMD magic `0x80000002`, stores `tmd_ptr` at `DAT_8007C018[DAT_8007B774++]`, then calls `FUN_800268DC` (builds the `+0xC` group descriptors). Reached from `FUN_8001F05C` case 2 (TMD-pack) and case 9 (TMD2). 35 instructions; tiny. |
| `801F69D8` | World-map top-view tile-visibility dispatcher (in `overlay_world_map_top_ext`). 643 instr / 2572 B. Bulk-copies camera struct from `0x8007BF10` into scratchpad, nested-loops over visible tile cells in scratchpad table `_DAT_1F8003EC + 0x8000`, dereferences each 0x20-byte object record, applies frustum + GTE RTPT, then routes the TMD via `DAT_8007C018[(object_kind8 + DAT_8007B6F8)*4]` and calls `FUN_80043390(tmd+0xC, color, fog)`. Color = `0xD0D0D0` default / `0x40D0D0D0` if interactive / OR `0x10000000` if extra flag. Fog = `clamp((GTE_z - 0x5000) >> 3, 0, 0x1000)`. Was the captured warp-transition cluster-A caller (Drake Read-bp's `ra = 0x801F725C`). |
| `801D8280` | `DAT_8007C018` table walker (overlay-resident, in every world-map / cutscene-mapview / 0897 overlay variant). Iterates entries `0..DAT_8007BB38` and for each pointed-to TMD calls `FUN_801D5E20` on each 0x1C-byte sub-record. 55 instr. |
| `801D77F4` | Overlay-resident actor allocator (alt to `FUN_80021B04`). Script-VM `4C D8` host hook (9-byte opcode). Takes `(vdf_idx: i16, tmd_idx: i16, kind: u16, variant: u16)`. Allocates actor slot via `FUN_80020DE0(0x8007068C, _DAT_8007C34C)`; resolves TMD from `DAT_8007C018[(i16)tmd_idx]` and VDF body from `_DAT_8007B7DC + body_offsets[(i16)vdf_idx]`. Two-pass vertex-pool build: sum `TMD_groups[record.idx].vertex_count * 8` into `_DAT_8007BA74`, malloc via `FUN_80017888`, then copy each referenced group's vertices into the pool. Populates `actor[+0x3C]=kind, [+0x3E]=variant, [+0x48]=TMD_ptr, [+0x4C]=VDF_body_ptr, [+0x90]=vertex_pool` (and zeros `+0x56/+0x5C/+0x68/+0x6E`). Dev printf strings `"tmd"`/`"otbl"`/`"vdf_n"` (preserved in the cutscene_dialogue overlay dump) confirm the structure. 125 instr / 500 B. |
| `80021B04` | SCUS-resident actor-spawn helper. Looks up `DAT_8007C018[actor[+0x64].i16]`, copies position/rotation into actor fields, populates per-actor OBJECT pointer table at `actor[+0x44]` (`[0] = tmd_group_count`, `[1..n] = sub-record pointers at stride 0x1C`). Then calls `FUN_80023070` (move-VM entry) and `FUN_8003D344` (5-op GTE transform). |
| `80024D78` | Per-actor OBJECT-table rebuild. |
| `80031D00` | Per-frame text-actor tick. Walks the actor list at `gp[+0x148]` and dispatches on `actor[+0x1C]`: cases 0/1/D/11 render text via `FUN_80036888`/`FUN_8003CC98`; cases 4/6/C/21 hand off to sub-routines. The per-frame driver behind dialog/labels. |
| `8001EBEC` | Per-frame OBJECT[10/11] swap (pose select for player TMDs). Also: mode-aware sound-driver extension dispatcher. |
| `8001E890` | "DATA_FIELD player loader" — calls `FUN_8003eb98(0x36C, …)` and references the dev paths `data\field\player.lzs` (PROT 876) and `h:\prot\all\data\field\player.lz`. Despite the name, the retail-PROT-876 bytes don't match the LZS-descriptor shape the function reads at `piVar2[2..7]`, and PROT 876 itself carries no TMDs. The `DAT_8007C018[0..4]` character TMDs originate from **PROT 0874** (`befect_data`) section 0 — see [`docs/formats/world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04). What `FUN_8001E890` still does that hits `DAT_8007C018[0..2]` is the post-install group-count cap (`entry[+0x08] = 10`) and the equipment-conditional patch dispatch into `FUN_8001EBEC`. |
| `8003E8A8` | PROT-by-index size lookup. Reads `start_lba = PROT_TOC[p+2]` and `next_lba = PROT_TOC[p+3]` (TOC base `0x801C70F0`; see [`prot.md`](../formats/prot.md)) and returns `next_lba - start_lba` (LBA count for the entry). Also stows `start_lba` at `gp[+0x8F0]` and the entry index at `gp[+0x90C]` so the matching `FUN_8003E800` read can pick them up. |
| `8003E800` | Issues the actual sector read scheduled by `FUN_8003E8A8`. `param_1` = destination buffer, `param_2` = LBA count, `param_3` = flag bits (`& 1` enables the libcd request via `FUN_8003F128`; `& 2` blocks on completion). The pair `FUN_8003E8A8` + `FUN_8003E800` is wrapped by `FUN_8003EB98(prot_idx, dst, 1)` for one-shot PROT-by-index loads. |

## Audio

| Address | Role |
|---|---|
| `8001FA88` | Sound subsystem init / `.dpk` loader. Loads `bse.dat` master bank then per-scene `.dpk` from `h:\main\bg\domepack\…`. |
| `8001FC00` | Streaming-asset loader. Builds paths under the `sound\` prefix; XA / `.pac` / `STR` consumer. |
| `800243F0` | Per-frame BGM/asset poller. Resolves BGM IDs via the PROT-relative offset scheme. |
| `800250D4` | Per-actor SFX trigger: `(sound_id, voice)`. Looks up sound table at `&DAT_8006F198 + sound_id*8` for `sound_id-< 0x200`, or runtime-allocated table at `_DAT_8007B8D0` for higher IDs. Reads voice-count from `entry[3] & 0x1F`, calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each voice. Called from per-frame actor tick when `actor[+0xb4] != 0` or `actor[+0xac]` is staged. |
| `80035B50` | **SFX-cue enqueue (4-slot ring).** `(cue_id: u16)`. Writes `cue_id` into the next slot of the 4-entry u16 ring at `&DAT_8007B6D8`, parks `gp+0x15A` (previous head) at the slot index just written, advances the head `gp+0x158` (wraps at 4), zeroes the parallel u32 timing array at `&DAT_8007C338[head]`. Common cue ids: `0x20` = field step, `0x21` = picker move, `0x23` = bonk / collision-blocked. Engines see this as the "queue a one-shot SFX" call from the field locomotion controller, the tile-board walker, the save-screen picker, and the dialog pager — see [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md), [`subsystems/tile-board.md`](../subsystems/tile-board.md), [`subsystems/save-screen.md`](../subsystems/save-screen.md). |
| `80035BD0` | **SFX-cue overwrite (current slot).** `(cue_id: u16)`. Same 4-slot ring as `80035B50` but writes only at the current head — does NOT advance `gp+0x158`. Used to replace an in-flight cue (e.g. the bonk SFX `0x23` overrides the pending step when a movement is blocked). The 4-slot ring is then drained by the audio per-frame tick (consumer side not yet pinned). |
| `8003E104` | Monster-soun- bank loader: `(monster_idx, slot, dst_buf)`. Reads `h:\mpack\monster.snd` for the given monster — LBA TOC at `0x801C8980-0x10` (4-byte stride, 2-entry pair = `[start_lba, end_lba+1]`). Dev path (`_DAT_8007B8C2 != 0`) goes through `FUN_800608F0`/`_920`/`_944`/`_910` (fopen/fseek/fread/fclose); retail path stages parameters into the gp window (`+0x97c`, `+0x894`) and kicks `FUN_8003F128` (async CD read). Called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8). |
| `80062340` | `SsSeqOpen` —-allocates a sequencer slot from the 16-slot bitmap at `_DAT_801CD2B8`; emits `s_Can_t_Open_Sequence_data_any_mor_80015D34` on full. See [`subsystems/audio.md`](../subsystems/audio.md) → "SsAPI sequencer". |
| `80061D18` | `SsSeqClose` — clears bitmap bit, memsets all 16 channel records (`0xB0` each) to defaults. |
| `8006275C` / `8006282C` | -SsSeqPlay` (ramped + 1-arg shim). |
| `800628F0` | `_SsSeqCtrl` —-Stop / Pause / Resume internal. |
| `800641EC` | `SsSeqRewind`-— full slot reset to start of sequence. |
| `80062410` | `_SsSeqInit` — -EQ-header parser (`'Sp'` magic + version `0x01`). |
| `80061C68` | `_SsSeqGetVar` — MIDI-style varint delta-time decode. |
| `80061EDC` / `80067E9C` | `SsSeqSetVol` (per-channel + slot -ol/pan). Clamps `0..0x7F`. |
| `80066E50` / `80067550` |-`_SsPitchFromKey` + `_SsVoNoteOn` — note→pitch table at `_DAT_8007A940` + master×velocity×channel-vol×stereo-pan voice mixer. |
| `80062AA0` | `SsSetMVol` — packs `[cmd=3, x-0x81, y*0x81]`, calls `FUN_8006BCB4`. |
| `80068D94` | `SsSepOpen` / SEP loader core — validates `'VAP'` magic, allocates SPU memory, patches per-track pointers, writes MIDI body to SPU. |
| `80069B18` / `800697E0` / `80069DA8` | SPU transfer-engine. `_DA8` = top-level `SpuWrite` (picks DMA vs CPU copy on `_DAT_8007AF5C`); `_B18` = 4-mode DMA state machine (arm-read / arm-write / set-addr / commit); `_7E0` = CPU-copy alternative. See [`subsystems/audio.md`](../subsystems/audio.md) → "SPU DMA transfer engine". |
| `8006A020` / `8006A04C` | `_spu_a` direction flips — set SPU command register bits `0x20000000` (read) / `0x22000000` (write). |
| `8006A078` | SPU register-s-ttling delay (60-iter busy-wait). |
| `8006A158` | `SsSpuMalloc` — bloc--table first-fit allocator over `_DAT_8007AFA4`. |
| `8006A420` | `SpuFree` -ompactor — coalesces adjacent free entries, shifts table down. |
| `8006A728` | `SpuFree` — block-tabl- free in `_DAT_8007AFA4`. |
| `8006BC9C` | `SpuIsTransferPaused` — `return _DAT_8007AF74 != 1`. |
| `8006ACBC` / `8006C048` | `SpuSetVoic-Attr` (mask dispatcher + 24-voice broadcaster). |
| `8006B1B4` | `SpuSetReverbModePa-am` — 30-attr reverb commit, writes regs `0x1C0..0x1FE`. |
| `8006BCB4` | `SpuSetCommonAt-r` — master vol L/R + reverb regs + SPUCNT bits. |
| `8006C6E4` | `_SsKey2Pitch` — `((key1*0x80+fine1) - (key2*0x80+fine2)) / 0x600` expon-ntial build. Returns 14-bit SPU PITCH. |
| `_DAT_801CE564` / `_DAT_801CE574` (data) | Legaia-installed seq-context vfn pointers — `_564` resolves the active script-VM seq context, `_574` is a worker-availability check. Used by `FUN_8006CA7C / CB3C / CDB0 / CE30 / DDC8`. |

## Renderer / GPU primitives

| Address | Role |
|---|---|-
| `80024EE4` | Push textured-quad GPU primitive onto the OT chain. `(layer, depth, color)` — writes a 6-word PSX GP0 packet (`0x05000000` length + `0x2B` polygon-with-tex command + four corner verts at `_DAT_1F80038C/0x18E` × `0xFFFC`) at `_DAT_1F8003A0`, then linkPrim via `FUN_8003D2C4`. Used by `FUN_800196A4` for the screen-fade / dim overlay. |
| `80035CB8` / `80035DA0` / `80035E44` | Text-actor sub-handlers. Children of the per-frame text-actor tick (`FUN_80031D00`). Each measures a row via `FUN_80036044` and renders via `FUN_8003CC98`. `_DA0` resolves a magic-name string from `PTR_DAT_80075DB0` keyed by the `0x800754CC + idx*0xC` magic table; `_CB8` advances state at gp `+0x87c` / `+0x13c`. |
| `8003C310` | Push `POLY_F3` (flat-shaded triangle) GPU primitive onto the OT. Writes size + color + verts; uses Y-offset `_DAT_8007B454`. |
| `8003F348` | Per-frame sprite/animation renderer tick. Walks list at `DAT_8007B7E0`, accumulates draw cost into `gp[+0x990]`. |
| `8003F3FC` | Per-frame particle--ctor update. Clip-tests against viewport `_DAT_1F800384..387`, accumulates physics (`vx*dt`), tests against camera at `_DAT_8007C364+0x14/+0x18`, emits two GP0 line packets (cmd `0x9000000`) via `_DAT_1F8003A0` OT pointer. Calls `FUN_8003F838` (RNG) + `FUN_8003F86C` (line-clip + emit). |
| `8003F838` | Particle PRNG step — 13-instr LCG: `seed = seed * 12 + 2`, byte-swap. State at `_DAT_1F8002A8`. |
| `8003F86C` | OT line-segment emitter with GTE-projected endpoints. 148 instrs: cop2 `0x280030` (RTPT) + `0x1400006` (NCLIP); inserts into ordering table at `_DAT_1F8003F4`. Returns `1` on emit / `0` on cull. |
| `8001FA68` | Generic ringbuffer push-u16: `*(u16*)(p2 + (++*p1)*2) = val`. |
| `80049348` | Actor animation frame setter. Loads frame offsets from the battle actor pointer table (`0x801C9370`) into the animation tables at `0x80076908` / `0x80076914`. |
| `8004A908` | NTSC/PAL-adaptive color dithering + brightness mixer for OT primitives. Reads `_DAT_80078D4C` mode flag. |
| `80046978` | Palette fade / tint engine. Reads RGB components, applies global brightness from `_DAT_1F800393`. |
| `8004695C` | Initiates a color-fade operation: writes RGBA -nto `gp[+0x9D0]`, sets active-flag at `gp[+0x9D4]`. Mode byte at `_DAT_8007B6CC`. |
| `8005724C` | OT primitive initializer for sprite rectangle — pos / size / color / clip. Calls `FUN_800608E0` for display config and `FUN_80057FEC` for palette query. |
| `80059568` / `80059634` / `80059700- | OT coordinate packer trio for textured / textured-variant / opaque sprite primitives. Display-mode-aware mask + shift, COP2 tag bytes `0xE3` / `0xE4` / `0xE5`. |
| `800198E0` | **TIM-upload helper.** Consumes either a custom Legaia sprite descriptor (magic `0x11`, single LoadImage call) OR a real PSX TIM (flags bit 3 = "has CLUT", two LoadImage calls — one for CLUT, one for pixels). Dispatches to `FUN_800583C8` for each block. Optional alpha-bit ORing (`*entry |= 0x8000`) per CLUT entry when `_DAT_8007B998 != 0`. Confirmed in `ghidra/scripts/funcs/800198e0.txt`. |
| `800583C8` | **`LoadImage` wrapper.** Pushes a libgpu `LoadImage(RECT*, void*)` request — identified by the literal debug-format string reference `s_LoadImage_800156d4`. The actual PSX BIOS `LoadImage` call site lives downstream. |
| `80058490` | **`MoveImage` wrapper.** Sister to `FUN_800583C8`. Identified by the debug-format-string reference `s_MoveImage_800156ec`. Push a libgpu VRAM-to-VRAM `MoveImage(RECT*, dest_x, dest_y)` request. |
| `80058068` | `SetDispMask` wrapper — controls display enable/disable via GP1 command `0x300` / `0x3000001`. |
| `8005800C` | DrawSync callback registration- |
| `80057C44` | Display-mode reset dispatcher — calls GTE init, memory clear, resolution setup. |
| `80058F1C` / `80058FA0` | Rect / Line OT primitive builder pair using COP2 coordinate transforms via the packer trio. |
| `8005AFB0` | GTE control-reg initializ-r (COP2 ctl regs `0xC000..0xF000`). |
| `8005B038` | GTE matrix-multiply loop — transforms a vertex stream through COP2. |
| `8005B0B8` | GTE shift-converter for texture / color bit packing. |
| `8005B618` | GTE matrix-loader (COP2 MTX regs `0x0..0x2000`). |
| `80021EAC` (data: `_DAT_8007BD24+0x26B`) | Animation tick counter incremented by `FUN_80055B4C`. |

## ANM animation container

The container parser is documented in [`formats/anm.md`](../formats/anm.md). The per-record bytecode dispatcher is overlay-resident (not yet captured); the public SCUS entry point only stages the per-record state on an actor.

| Address | Role |
|---|---|-
| `80024CFC` | `play_anm_by_id(id, actor, ?)` — allocates an actor (via `FUN_80020DE0`), reads the per-record offset from `_DAT_8007B7C8 + (id*4) + 4`, and stores `(anm_base + record_offset)` in `actor[+0x4C]`. Writes `0xB` to `actor[+0x56]` (anim state) and `100` to `actor[+0x68]` (frame counter). The bytecode walk runs in a per-frame actor tick that hasn't been traced. |

## MES / dialog text interpreter
-
The MES bytecode interpreter is **statically linked into SCUS_942.54** — not overlay-resident as previously assumed. Four functions cover the encoding fully; the dialog window pager is overlay-resident in the dialog/town overlay. See [`formats/mes.md`](../formats/mes.md) for the per-byte decoding table.

| Address | Role |
|---|---|
| `8003CA38` | Glyph stride walker. 16 instructions: returns count of bytes until next terminator (`< 0x1F`). For each `(byte & 0xF0) == 0xC0` it consumes an extra byte. |
| `80036044` | Text width measurement. Same byte classification as the stride walker plus substitution dispatch on `(byte + 0x40) < 8` (catches `0xC0..0xC7`); the explicit cases `0xC1..0xC5` and `0xC7` follow substitution pointers into character-name / item / magic / spell / quest tables and recursively walk the substituted string. |
| `80036888` | Text renderer. Same opcode dispatch as `FUN_80036044`, but emits glyphs into the text-actor buffer instead of just measuring. Calls `FUN_80036514` to expand substitutions before walking. |
| `80036514` | Substitution expander. Copies from source bytecode to a working buffer, normalising the input-time aliases (`0x5E XX` → `0xCE (XX-0x2D)`, `0xFF` → `0xCF`) and inlining `0xC1..0xC5` / `0xC7` substitutions into glyph runs. |
| `80035F04` | **Max-line-width measure.** Expands the source string via `FUN_80036514` into a stack buffer (default escape value seeded by `FUN_80056788`), then walks the expanded bytes accumulating per-glyph advance: `0x7C` resets the running width and tracks the max-so-far (column separator), `0xCE`-class escapes add a fixed width from the dialog-font escape table at `0x80074050`, ordinary glyphs add their proportional width from the table at `0x80073F3C` offset by `_DAT_800740E8`. Returns the widest line. The label-/name-width helper the title and name-entry renderers call before centring. Cited from `crates/engine-vm/src/title_prim.rs` (`FUN_80035f04` label measure) and `FUN_801E6B34` (name-entry caret width). `see ghidra/scripts/funcs/80035f04.txt`. |
| `FUN_801D84D0` (dialog overlay) | Dialog window pager. 26-state machine (`_DAT_801F2734`) for per-frame paging, 16-line buffer at `_DAT_801F3540`, terminator test `(byte & 0x7F) < 0x20`. Drives the actual on-screen dialog window. |
| `FUN_8001FD44` | **Scene-change packet** (full entry above). Sets `_DAT_1F800394 |= 0x40` (scene-transition-pending flag — *not* a "dialog active" lock, an earlier mislabel) and copies the destination scene name into `0x8007050C`/`0x80084548`. Called from field-VM op `0x3F` (named scene-change), which carries the destination name inline. |

## Dialog-overlay actor-frame helpers

Per-frame substeps of `FUN_801D1344` (the actor frame handler in the dialog overlay). They split the frame into "compute screen position", "step actor physics", "emit sprite primitives", and "build collision bitmask".

| Address | Role |
|---|---|
| `FUN_801CF754` (dialog overlay) | Camera-frame projector. Caches `_DAT_1F800020/24` from the active camera struct (`+0x14/+0x18`), then walks the linked actor list at `*param_2`, looking up each actor's tile descriptor at `_DAT_1F8003EC + slot * 0x20` and computing screen-space `(X, Y)` via the `(s8 << 7) + (s8 << 4)` packing the renderer expects. Skips actors with state bits `0x3` set. |
| `FUN_801D0B90` (dialog overlay) | Per-character training-stat tick. Subtracts `0x20` from `_DAT_801F2274` per call; on underflow, walks every party-character record (stride `0x414` from base `0x80084200`) and bumps the `+0x44E` u16 by 8 (clamped at the `+0x44C` cap) when state flag `0x1000000` is set. The "gauge filling while standing in dialog" tick. |
| `FUN_801D1BA0` (dialog overlay) | Vertical-step physics for the active actor. Computes `step = DAT_1F800393 * 0xC` (halved when actor flag `0x2000` is set), clamps Y delta by ground-collision via `FUN_801D1878`, and writes back to `actor[+0x16]`. Also resolves the special "frozen drop" path when `actor[+0x9E] == 0`. |
| `FUN_801D9D30` (dialog overlay) | Camera-shake jitter. Subtracts cached camera offsets, then if `_DAT_8007B630 != 0` calls the LCG RNG (`func_0x80056798`) twice to seed new shake offsets at `DAT_801C6EA4 + 0x18/0x1C`, masked against `(1 << (0x15 - amplitude)) - 1`. |
| `FUN_801DB510` (dialog overlay) | Actor sprite emitter. Walks the per-actor sprite-anim table at `0x801F2798/0x801F2804`, emitting GP0 primitives. Reads from the actor history-pose buffer (`+0x14/+0x18` vs `+0x1C/+0x20`) to do motion-blur trail rendering. |
| `FUN_801DE234` (dialog overlay) | Tile-collision bitmask builder. Iterates `func_0x80017FBC(idx, x_tile, y_tile)` until it returns 0, ORing `1 << (hit[+4] & 0x1F)` into `_DAT_8007B8F4`. Used by the actor's footprint test gated on flag `0x400000`. |

## Records / stats screen

The "records" page (battles fought, escapes, play time, per-character maximums) is rendered by a single function in the field overlay. Stats globals are persistent save data.

| Address | Role |
|---|---|
| `FUN_801ED710` (field overlay) | Records-screen renderer. Reads + draws six stats blocks via `FUN_8003CC98` (single-line text) and `FUN_80034B78` (number formatter): "No. of Battles" (`_DAT_800846A4`, capped at 99999), "No. of Escapes" (`_DAT_800846A8`), play time (`_DAT_800845DC` divided twice by `0x3C` for `H:MM:SS` decomposition, capped at 99h59m59s), then 3× per-character "Maximum Hits" / "Maximum Damage" iterating the stats record at `0x80088140 + n*0x414` (`+0x6B4` = max-hits u32, `+0x6B0` = max-damage u32). Depth slots 5 / 6 / 7 / 9. Captured as `overlay_cutscene_dialogue_801ed710.txt`. |
| `FUN_801DC6B4` (menu overlay) | Save-screen per-frame state machine. Sub-state in `_DAT_8007B43C` (0 = init, 1 = fade-in, …). Init (state 0): sets panel origin `DAT_801E4A4E = 0xB4` (x=180), `DAT_801E4A52 = 0x18` (y=24), adjusted +/-0xE when `func_0x8003CE64(8)` (flag-8 test) is non-zero; sets up screen-fade via `_DAT_8007B440 = 0xF2`, `DAT_801E46A0 = -0xF2`. Entry-context pointer `_DAT_8007B450` routes to sub-state: `NULL`/0→0x1A (normal save), `\x01`→0x19, `\x07`→0x20, `\r`→4. Reads pad from `_DAT_1F8003A0`. Captured as `overlay_shop_save_801dc6b4.txt`; see also [`subsystems/save-screen.md`](../subsystems/save-screen.md). |

## Inventory / spell list

| Address | Role |
|---|---|
| `80042DBC` | Spell-list pop: `(char_idx, spell_id, dst_slot)`. Per-character record stride `0x414` (matches the magic-table stride from `FUN_80036044`). Searches the per-character spell list at `[char_base + 0x13d ..]` for `spell_id`, copies the matched 4-byte record into the active-spell slot at `[char_base + dst_slot*0x14 + 0x2B0]`, then shifts the rest of the list down (counter at `[char_base + 0x13c]`). |

## Menu / HUD globals

| Address | Role |
|---|---|
| `80034A6C` | Menu / HUD globals reset. Initialises `0x80084594..0x800845B8` and `0x800846D0..0x800846DC` to default UI palette / cursor positions. Zeros the 512-byte save-data scratch slot at `0x80084340..0x8008453F`. Calls `FUN_8003CE08(0x1A)` (set 4th-flag-bank bit `0x1A`) when `_DAT_8007B868 != 0`. |
| `800337B0` | Menu-string formatter and renderer. 27 KB switch-on-mode that drives the character-status / equipment / spell-screen pages via `FUN_8003CD00` (multi-line) and `FUN_80036888` (raw draw) keyed on string buffers at `&DAT_8007B4B0..` and the multi-line label table at `gp + 0x13c + 0x7F86`. |
| `8004313C` | HUD panel-geometry setup, gated on party member count (`DAT_80084594`). Writes the panel start/end/span triple `gp+0x2D2` (start), `gp+0x2D4` (end), `gp+0x2D6` (span = end - start): a single-member party (`< 2`) with flag-bank bit `0x14` clear (`FUN_8003CE64(0x14)`) collapses the panel to `0..0x80` (or `0x80..0x100` when `DAT_80084598`, the first party id, is non-zero); otherwise the full `0..0x100`. Cited from `crates/engine-vm/src/field.rs` + `world_map.rs`; the surrounding crate comment's tentative "stop current SFX" reading is not what the dump does. `see ghidra/scripts/funcs/8004313c.txt`. |

## World map

Two overlay variants: normal-walk (`overlay_world_map`) and top-view debug (`overlay_world_map_top`).
Both live at `0x801C0000+`. Full architecture in `docs/subsystems/world-map.md`.

| Address | Role |
|---|---|
| `FUN_801E76D4` (world_map overlay) | World map controller. Handles the debug top-view toggle (combo: `_DAT_8007B98C != 0` + `pad 0x4A` + `held 0x40`), flips `DAT_801F2B94` (view flag at offset past 192 KB window), captures camera origin into `_DAT_801F35A8/AA/AC`. In top-view mode processes D-pad input into `_DAT_80089120/_18` (XZ scroll) and `_DAT_8007B794`/`_6F4` (azimuth/zoom). Normal-walk path ticks field VM + actor + motion VM. |
| `FUN_801EAD98` (world_map overlay) | World map developer menu renderer. Scrollable 24-entry list: MAP_CHANGE / CARD_OPTION / PLAYER_STATUS / CAMERA (shows `_DAT_80089120/_18`) / ENCOUNT (`DAT_8007B5F8`) / OTHER_SETTINGS / BGM_CALL (`_DAT_801F2E90`) / DEBUG. `_DAT_8007B868` gates MAP_CHANGE and CARD_OPTION to "CLOSED". |
| `FUN_801ECA08` (world_map overlay) | World map panel sizer. Computes panel height `= (row_end - row_start + 1) * 8`, centres in 208 px. 6-way dispatch on `ctx[+0x54]`; cases 1 + 3 delegate to `FUN_801EAD98`. |
| `FUN_801EE90C` (world_map overlay) | World map text-box dispatcher. 15-entry JT on `ctx[+0x54]`; out-of-range path calls `FUN_80031D00` (text-actor tick) when `ctx[+0x54] < 10`. |
| `FUN_801CFC40` (world_map_top only) | World map sprite batcher (top-view). Writes `actor[+0x14/16/18]` into GPU coord registers `0x1F800020/22/24`, iterates sprite list at `DAT_801C93C8`. Delegates to `FUN_801CF9F4` when `_DAT_8007B6B8 == 0x20`. |
| `FUN_801DA51C` (world_map overlay) | World map entity tick. 5-state SM on `entity[+0x8A]` (JT `0x801CEC28`); at state 0 calls `FUN_800243F0` (BGM/scene resolver) and checks `_DAT_8007BB38` pad for interaction. |
| `FUN_801D9E1C` (world_map overlay) | World-map encounter handler. 349 instructions / 1396 bytes. `(entity_ptr, resolver_result)`. Invoked from `FUN_801DA51C` state-0 when the encounter countdown drains and the per-entity gate is open. Early-outs when the encounter-records table head `**(char**)(DAT_801c6ea4 + 0x20)` is empty or when `_DAT_1f800394 & 0x8000` (dialog-active flag) is set. Otherwise resyncs `entity[+0x8e..+0x8f]` against the player's tile coords at `_DAT_8007c364 + 0x14/+0x18` and gates on the player being within `±1` tile of the prior cached position. On match, walks the encounter-records table at `(DAT_801c6ea4 + 0x20/+0x24/+0x28)` using stride `*(byte*)(DAT_801c6ea4 + 0x5e)` and `func_0x8003CE9C` to decode each record's BGM/scene id; per-record `func_0x8003CE64()` gates the fight trigger. Maps to [`WorldMapEntityHost::on_encounter`] in `crates/engine-vm/src/world_map.rs`. |

### World-map render pipeline

The render chain that gets the POLY_FT4 batch from the per-frame SCUS dispatch into the overlay-resident emitter. Walked end-to-end in [`docs/subsystems/world-map.md`](../subsystems/world-map.md#render-pipeline). The SCUS dispatcher entries `FUN_80025EEC` and `FUN_80025F2C` are documented under [Game-mode state machine](#game-mode-state-machine) above; both route through the per-frame render tick below.

| Address | Role |
|---|---|
| `FUN_80016444` (SCUS, 1352 bytes) | Per-frame world-map render tick reached via `FUN_80025EEC(1)` (default per-mode handler) or `FUN_80025F2C(0)` (Mode 13 MAPDSIP handler). Reads `_DAT_8007BC3C`; if `== 2` performs a direct `jal 0x801D7EA0` (PC `0x80016764`) into the overlay-resident POLY_FT4 emitter. |
| `FUN_801D7EA0` (world_map overlay, 832 bytes) | Parametric POLY_FT4 emitter. Gated by one-shot self-clearing flag `_DAT_801F351C`. 224-iter outer loop emitting 2× POLY_FT4 (literal `0x2C808080` GP0 cmd, chain tag `0x9000000`) + 1 small prim (chain tag `0x3000000`) per iter using cos-rotation projection from the LUT at `0x8007B81C`. ~670 prims per call. Horizon / sky / animated background. The bulk continent (~4300 POLY_FT4 prims per kingdom) is **not** emitted here — it flows through ordinary case-5 TMD rendering via `FUN_80043390`'s overlay-mode dispatch table at `0x801F8968` (eight per-prim fog-enabled leaves at `0x801F7644..0x801F8690`, each a SCUS-side sibling body plus a GTE `dpcs`/`dpct` distance-cue post-process). See [world-map subsystem § bulk continent terrain emit mechanism (pinned)](../subsystems/world-map.md#top-view-bulk-terrain-render-path-overlay-replaced-per-prim-renderers). |
| `FUN_801D8258` (world_map overlay, 40 bytes) | Gate-arm trigger. Writes `_DAT_801F351C = 1`, then `_DAT_801F3520/3524/3528 = param_2/3/4` (scale / step / OT-layer for the next emission). |
| `FUN_801D1344` (**world_map overlay**, 1332 bytes) | Gate-arm caller. Function-pointer-only entry (Ghidra `incoming=0`); reads three globals at `_DAT_8007BCD0/_D4/_D8` and forwards them to `FUN_801D8258` at PC `0x801D1470: jal 0x801D8258`. **Distinct from `FUN_801D1344` in the dialog overlay** (the actor frame handler with sub-helpers at `FUN_801CF754` / `FUN_801D0B90` / `FUN_801D1BA0` / `FUN_801D9D30` / `FUN_801DB510` / `FUN_801DE234`, see [Dialog-overlay actor-frame helpers](#dialog-overlay-actor-frame-helpers)) - same RAM address, different code per overlay. |
| `FUN_801C2B2C` (0897 field overlay, 1332 bytes) | Code-identical relocation copy of the world_map overlay's `FUN_801D1344`. Calls `jal 0x801D8258` at PC `0x801C2C58`. Same body at a different load address. |
| `FUN_801C9688` (0897 field overlay) | Sibling reader / clearer of `_DAT_801F351C`. Field-mode equivalent of the world-map emitter's gate-check path. |

## Stub helpers
-
These are 2-instruction `jr ra` / nop bodies — likely retail-disabled debug hooks where the dev gate lives in the caller. Listed for completeness so a clean-room port can implement them as no-ops without further investigation.

| Address | Role |
|---|---|
| `80024C80` | Move-VM op `0x16` body. The opcode is a no-op. |
| `80024DFC` | Actor-cleanup hook (called from `FUN_8002519C` while freeing an actor). |
| `8002B93C` / `8002B944` / `8002B94C` / `8002B954` | Cluster of debug-disabled helpers. |
| `8003E7F0` | Reserved sound-path stub (called from `FUN_80017AAC`). |

## See also

- [`docs/subsystems/script-vm.md`](../subsystems/script-vm.md) — the field/event VM dispatcher (`FUN_801DE840`) anchoring many of these entries.
- [`docs/subsystems/battle-action.md`](../subsystems/battle-action.md) — the battle-action SM (`FUN_801E295C`) and its helpers.
- [`docs/reference/memory-map.md`](memory-map.md) — the RAM globals these functions read and write.
- [`docs/tooling/ghidra.md`](../tooling/ghidra.md) — how the dumps backing this directory are produced.
