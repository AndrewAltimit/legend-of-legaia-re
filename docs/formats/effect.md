# Effect bundles

Two distinct formats share the "effect" name - the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `data\battle\efect.dat`. Only one PROT entry uses the on-disc form; the runtime wrapper is what battle code actually consumes.

## On-disc effect bundle (magic `0x02018B0C`)

A bulk scan over the PROT corpus finds this format in exactly one entry: `0000_init_data` (engine bootstrap data).

### Header (12 bytes)

```
+0   u32 LE = 0x02018B0C       ; magic
+4   u32 LE = 0x0000001D       ; HEADER_A - TMD slot count (1 master + 28 sub = 29)
+8   u32 LE = 0x0000001E       ; HEADER_B - HEADER_A + 1 (sentinel/terminator)
```

Both header words are constant within the format. **HEADER_A = 29 = the maximum TMD count the format reserves space for** - 1 master TMD plus 0..28 sub-effect TMDs.

### Offset table (112 bytes after the header)

28 strictly-ascending u32 LE values:

```
0x17F4, 0x1832, 0x198F, 0x1B9B, 0x1D75, 0x1EFD, 0x20BB, 0x224B,
0x2438, 0x260B, 0x26DD, 0x27C3, 0x2982, 0x2AA1, 0x2C44, 0x2D9F,
0x2F77, 0x30E6, 0x3300, 0x34BE, 0x36BE, 0x3805, 0x39B6, 0x3AB4,
0x3C4A, 0x3E23, 0x3F78, 0x404D
```

Slot sizes derive from `offset[i+1] - offset[i]`. The 28th slot's size depends on the post-table asset-region layout.

### Asset region (after the table)

The asset region begins with a **master Legaia TMD** at `assets_start`. The single observed master carries `1 object, 382 verts, 760 normals, 760 primitives`. The 28 schema offsets do not byte-align with sub-TMD file positions; they likely index into an abstract runtime buffer whose semantics live in a consumer that hasn't been reached.

### API

```rust
use legaia_asset::effect_bundle;
if let Some(eb) = effect_bundle::detect(&buf) {
    println!("magic @ 0x{:X}, asset region 0x{:X}..0x{:X}",
             eb.magic_offset, eb.assets_start, eb.file_size);
    for (i, slot) in eb.slots.iter().enumerate() {
        let size = slot.size.map(|s| format!("{}", s)).unwrap_or("?".into());
        println!("  slot[{}] off=0x{:X} size={}", i, slot.offset, size);
    }
}
```

Implementation: `crates/asset/src/effect_bundle.rs`.

## Runtime effect format - 2-pack wrapper

The format `data\battle\efect.dat` (PROT entry 873) actually uses at runtime. Cross-validated against a live battle save state - the post-init buffer at RAM `_DAT_8007BD5C = 0x800E425C` is byte-identical to PROT.DAT bytes at sector `0x9086`.

### Buffer layout

```
+0    u32   pack0_offset    ← fixed up to absolute pointer on first init
+4    u32   pack1_offset    ← fixed up to absolute pointer on first init
+8    [N × 8-byte sprite atlas entries]
...   [pack0]  u32 count, u32 entry_offsets[count]   ← packs of frame-batched anim entries
...   [pack1]  u32 count, u32 entry_offsets[count]   ← packs of effect scripts
```

Each **pack0 entry** is a frame-batch animation record:

```
+0   u8 frame_count     ← number of 6-byte frames
+1   u8 flags
+2   [N × 6-byte frame records]
   each frame:
    +0  u8  sprite_atlas_index   (indexes the inline 8-byte atlas at buffer+8)
    +1..+5 (timing / dir bits)
```

Each **pack1 entry** is an effect-ID script:

```
+0   u8   child_count         ← N - number of child sprites to spawn
+1   u8   flags               (bit 0 = use random child distribution)
+2   u16  spread              ← half-range modulo for random child position (signed 8.8 fixed)
+4   [N × 14-byte child sprite descriptors]
   each descriptor (retail offsets from the per-frame walker):
    +0x00  u16  sprite_id     ← indexes pack0; copied to master slot on spawn
    +0x02  i16  width         ← half-width of random X distribution (8.8 fixed)
    +0x04  u16  anim_flags    ← animation / shading flags read by the per-frame walker
    +0x06  i16  depth         ← half-width of random Z distribution (8.8 fixed)
    +0x08  u8[6] tail         ← animation curves / sound-id / timing (per-frame walker only)
```

The retail random-distribution loop (`FUN_801E0088` pass 1) reads only `+0x02` (width) and `+0x06` (depth) per child - those two govern where a child sprite spawns relative to the effect origin. `anim_flags` and `tail` are consumed later by the per-frame walker when advancing a live child slot's animation state.

A live `0873_befect_data` sample carries 14 entries in pack0 and 33 entries in pack1. The pack0/pack1 offset tables hold **absolute file offsets** (not the `word*4` offsets of `asset::pack`). Parser: `legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes`.

Inline sprite atlas entries (between `buffer+8` and pack0) are 8 bytes each. The layout is pinned from the consumer (`FUN_801E0088` pass 2, the sprite-emit block ~`0x801E0840`), which reads them byte-wise to build each child's GPU sprite primitive:

```
+0  u8  u       ; source texel U within the texture page
+1  u8  v       ; source texel V
+2  u8  w       ; sprite width in texels
+3  u8  h       ; sprite height
+4  u16 tpage   ; PSX texture-page descriptor, copied verbatim to the primitive
+6  u8  clut    ; CLUT (CBA) id
+7  u8  ?        ; unknown / reserved
```

The texel rectangle is `(u, v)..(u+w-1, v+h-1)`. A typical entry is a 32×32 sprite (`w=h=0x20`) with `tpage=0x7680` (page base (0,0), 8-bit colour mode) and a per-effect CLUT id. The pixels live in VRAM, uploaded once at battle load from the sibling **`etim.dat`** file in the same `befect_data` cluster (see "Battle effect cluster" below). The atlas only carries the VRAM coordinates; the actual texel bytes are blitted by the battle scene loader.

### Battle effect cluster (`befect_data`, CDNAME 872) + texel-upload source

`efect.dat` (the atlas + anim + script 2-pack) is one of four files in the `befect_data` cluster the battle scene loader `FUN_800520F0` pulls in. `FUN_800520F0` is a sequential state machine (sub-state byte at `gp+0xa59`); each state loads one file through the dual-mode loader - retail opens the dev-path string, debug uses a PROT index (`FUN_8003e8a8`):

| Loader state / case | Dev-path string | Role |
|---|---|---|
| case `0x8` | `h:\prot\battle\etim.dat` (`0x80015358`) | **Effect sprite texels.** The TIM-image pack the sprite atlas samples. |
| case `0xb` | `h:\prot\battle\etmd.dat` (`0x80015370`) | Effect 3D models (Legaia TMDs; registered via `FUN_80026b4c`, which asserts magic `0x80000002`). |
| case `0xb` | `h:\prot\battle\vdf.dat` (`0x80015388`) | VDF buffer (asset type `0x07`, appended via `FUN_8001fbcc`). |
| case `0xc` | `data\battle\efect.dat` (`0x800153a0`) | The 2-pack above; initialised by `FUN_801DE914` (offset fixup only - no texel upload). |

**The texels come from `etim.dat`, not from `efect.dat`.** After `etim.dat` is read into the load buffer, loader **state `9`** (`LAB_800526c8` in `FUN_800520F0`) walks it as an `asset::pack` (`u32 count` + `u32 word_offsets[count]`) and calls **`FUN_800198e0`** on each entry. `FUN_800198e0` is the engine's general packed-image → VRAM uploader: it reads a per-chunk tag/flag word, builds a PSX `RECT` `(x, y, w, h)`, and calls **`FUN_800583c8` = `LoadImage`** (`0x800156d4`) to DMA the pixels into VRAM, maintaining a CLUT cache at `0x8007BEC0`. (The same routine the title / menu / save overlays and the type-`0x01` CLUT walker `FUN_8001fe70` use.) The atlas's `tpage` / `clut` fields then address whatever VRAM page `etim.dat`'s blits populated.

**Extraction caveat - the cluster slices overlap.** The per-entry PROT extractor does *not* cleanly separate the four logical files. "PROT 0873" at offset `0x2000` is byte-identical to the start of "PROT 0874," and the real `efect.dat` 2-pack is only the first ~`0x2000` of "PROT 0873" (`pack0@0x488`, `pack1@0x900`, structures ending by `0x1bf8`). So the four files are packed within a contiguous streamed cluster region and don't align to the extractor's TOC-derived entry boundaries. Decoding `etim.dat`'s exact blit RECTs - and uploading real effect texels engine-side - needs a corrected cluster extraction that follows the loader's per-file open/read offsets rather than the naive per-entry slice. (Earlier notes said the texel source was "unknown / PROT 0872 is not a plain TIM pack"; extracted-0872's first entries are ~96-byte geometry records, so it is not a TIM pack - but the texel source is `etim.dat` via `LoadImage`; it is the extraction boundary that is wrong, not the source.)

### Consumer cluster

The runtime consumer lives in the battle overlay (`0898_xxx_dat`):

| Function | Span | Role |
|---|---|---|
| `0x801DE914` | 0x138 | **Init / pack-fixup.** Called by `FUN_800520F0` case `0xE` with `(id=0x1000, param=0xA00)`. Zeros the 5008-byte runtime pool at `_DAT_8007BD30`, treats `_DAT_8007BD5C` as the 2-pack wrapper, walks both packs converting offsets→pointers (fixup gated by `byte[3] == 0`). Stores post-fixup state in the table-head 16-byte record `(u16 id, u16 param, u32 buf+8, u32 pack0_data+4, u32 pack1_data+4)`. Sets the init flag `_DAT_8007BD58 = 1`. |
| `0x801DFDF8` | 0x290 | **Public spawn-effect API.** Signature: `(byte effect_id, short* world_pos, ushort angle)`. Reads `pack1[effect_id]` from the head record to find the effect's script. Allocates the first free slot in the 32-entry × 28-byte master pool, writes pos/angle, copies script header bytes, sets script cursor to `entry + 4`. Special-cases `effect_id = 4 → 0x801F5D90` and `effect_id = 0x13 → 0x801F5CF8`. |
| `0x801E0088` | 0x970 | **Per-frame walker (update + render).** Two passes. Pass 1 (32 master slots): for each active slot, decrement state byte; if zero, fetch next 14-byte script instruction (byte 0 indexes pack0 → an anim batch; the rest provides angle/dir/lifetime). Spawns into the 128 child slots; applies sin/cos via lookup tables at `_DAT_8007B7F8` and `_DAT_8007B81C`. Pass 2 (128 child slots): builds a PSX GPU sprite primitive (`0x9000000` opcode + `0x2E000000` RGB), pulls UV/size/page from the inline sprite atlas via the child's anim cursor, submits via `func_0x8003D2C4`. |

Decompiled output: `ghidra/scripts/funcs/overlay_battle_*.txt`.

### Runtime pool layout (`_DAT_8007BD30`, 5008 bytes total)

```
+0x000  16 bytes   table-head record set by init
+0x010  4096 bytes 128 × 32-byte child slots - per-sprite render state
+0x1010 896 bytes  32 × 28-byte master slots - per-effect-instance state
+0x1390 1968 bytes (unused / future expansion)
```

32 max simultaneous effects × ~4 sprites avg = 128-child sprite pool.

### Side-band streaming-effect handler

`0x801F17F8`, called from `FUN_800520F0` case `0xFF`, streams two specific runtime-only files via `FUN_800558FC`:

- `data\battle\summon.dat` (PROT `0x37F`) - selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` (PROT `0x380`) - opposite branch.

Buffer size per slot: `0x10800` = 67584 bytes. Format unverified; may share the 2-pack layout but not yet confirmed.

### Open questions

- **Effect-ID → human effect name.** Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". Reachable by tracing call sites of `FUN_801DFDF8` in damage / battle-action code.
- **`etim.dat` byte layout + blit RECTs.** The texel-upload *path* is pinned (`etim.dat` → `FUN_800198e0` → `LoadImage`), but the exact on-disc bytes need a cluster-aware extraction (see the overlap caveat above) before the per-sprite VRAM destination rectangles can be decoded and replayed engine-side.
- **summon.dat / readef.dat formats.** Not yet decoded.

## Field-pack format (magic `0x01059B84`)

A small number of PROT entries lead with magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. The preamble→slot mapping is unknown - likely runtime-reconstructed from the schema's offset hints. Detector + dispatch live in `crates/asset/src/field_pack.rs`.
