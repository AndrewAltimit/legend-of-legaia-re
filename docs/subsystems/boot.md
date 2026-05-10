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

## Debug flags

- `_DAT_8007B8C2` - dev/retail build toggle. Several subsystems (sound init, field loader) carry an "if dev" branch keyed on this byte. No writers exist in `SCUS_942.54`; the writer must live in an unswept overlay or come from external POKE (TCRF GameShark codes confirm both this flag and `_DAT_8007B98F` are runtime-writable).
- `_DAT_8007B98F` - separate debug-mode flag (NA build offset; JP retail uses `0x07D51F`, an `0x1B90` build-shift).

The input dispatcher `FUN_8001822C` reads these flags but doesn't write them; the writer is downstream of one of the option-menu / cheat-menu overlays (`0896` or similar).
