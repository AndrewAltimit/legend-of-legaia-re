# Effect bundles

Two distinct formats share the "effect" name — the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `data\battle\efect.dat`. Only one PROT entry uses the on-disc form; the runtime wrapper is what battle code actually consumes.

## On-disc effect bundle (magic `0x02018B0C`)

A bulk scan over the PROT corpus finds this format in exactly one entry: `0000_init_data` (engine bootstrap data).

### Header (12 bytes)

```
+0   u32 LE = 0x02018B0C       ; magic
+4   u32 LE = 0x0000001D       ; HEADER_A — TMD slot count (1 master + 28 sub = 29)
+8   u32 LE = 0x0000001E       ; HEADER_B — HEADER_A + 1 (sentinel/terminator)
```

Both header words are constant within the format. **HEADER_A = 29 = the maximum TMD count the format reserves space for** — 1 master TMD plus 0..28 sub-effect TMDs.

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

## Runtime effect format — 2-pack wrapper

The format `data\battle\efect.dat` (PROT entry 873) actually uses at runtime. Cross-validated against a live battle save state — the post-init buffer at RAM `_DAT_8007BD5C = 0x800E425C` is byte-identical to PROT.DAT bytes at sector `0x9086`.

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
+0   u8  instruction_count   ← N
+1   u8  flags               (bit 0 = X-mirror, bit 1 = Y-mirror)
+2   u8  ?
+3   u8  ?
+4   [N × 14-byte instructions]
   each instruction:
    +0  u8   pack0_anim_index
    +2  u16  base angle (sub-frames)
    +4  u16  ?
    +6  u16  randomization range (signed mod range)
    +8..+13  velocity / lifetime deltas
```

A live `0873_befect_data` sample carries 14 entries in pack0 and 33 entries in pack1. Inline sprite atlas entries (between `buffer+8` and pack0) decode as `(u16 u, u16 v, u16 page_descriptor, u8 clut, u8 ?)` — standard PSX sprite UV packets.

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
+0x010  4096 bytes 128 × 32-byte child slots — per-sprite render state
+0x1010 896 bytes  32 × 28-byte master slots — per-effect-instance state
+0x1390 1968 bytes (unused / future expansion)
```

32 max simultaneous effects × ~4 sprites avg = 128-child sprite pool.

### Side-band streaming-effect handler

`0x801F17F8`, called from `FUN_800520F0` case `0xFF`, streams two specific runtime-only files via `FUN_800558FC`:

- `data\battle\summon.dat` (PROT `0x37F`) — selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` (PROT `0x380`) — opposite branch.

Buffer size per slot: `0x10800` = 67584 bytes. Format unverified; may share the 2-pack layout but not yet confirmed.

### Open questions

- **Effect-ID → human effect name.** Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". Reachable by tracing call sites of `FUN_801DFDF8` in damage / battle-action code.
- **summon.dat / readef.dat formats.** Not yet decoded.

## Field-pack format (magic `0x01059B84`)

A small number of PROT entries lead with magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. The preamble→slot mapping is unknown — likely runtime-reconstructed from the schema's offset hints. Detector + dispatch live in `crates/asset/src/field_pack.rs`.
