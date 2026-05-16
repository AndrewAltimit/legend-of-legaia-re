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

The 28-mode state machine table at `0x8007078C` is the top-level "what is the game doing" dispatcher. Each entry is a `(handler_addr, …)` tuple corresponding to a major mode (boot, title, field, battle, world map, menu, cutscene, etc.).

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

A **town/field subsystem** uses a separate format-string pool at `0x80011079..0x80011109` (`"    town "`, `"mode %d"`, `"    baria mode "`, `"    walking set"`, `"end of mes works set"`, `"open port.dat"`, `"nt_group_table %x"`). These print at retail-build runtime but have no LUI+ADDIU caller resident until the town/field overlay is loaded — i.e. the "mode 17 / mode 16" runtime printfs are *town-subsystem* mode transitions, not the master 28-mode state machine index.

## Debug flags

- `_DAT_8007B8C2` - dev/retail build toggle. Several subsystems (sound init, field loader) carry an "if dev" branch keyed on this byte. No writers exist in `SCUS_942.54`; the writer must live in an unswept overlay or come from external POKE (TCRF GameShark codes confirm both this flag and `_DAT_8007B98F` are runtime-writable).
- `_DAT_8007B98F` - separate debug-mode flag (NA build offset; JP retail uses `0x07D51F`, an `0x1B90` build-shift).

The input dispatcher `FUN_8001822C` reads these flags but doesn't write them; the writer is downstream of one of the option-menu / cheat-menu overlays (`0896` or similar).
