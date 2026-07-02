# Legaia TMD (3D mesh)

Legaia uses a custom variant of Sony's PSX TMD format. Magic is `0x80000002` instead of the standard `0x00000041`; pointer fields are byte-relative offsets that the runtime patches to absolute addresses on load. Implementation: `crates/tmd/src/lib.rs` + `crates/tmd/src/legaia_prims.rs`. Reverser context: `ghidra/scripts/funcs/{80026B4C, 800268DC, 8001F05C, 8002735C}.txt`.

## Header (12 bytes)

```
u32 id        // ALWAYS 0x80000002 in Legaia. Bit 31 = FLIST_BIT
              // (pointers are byte offsets relative to header end);
              // low byte 0x02 = "Legaia TMD format version 2"
u32 flags     // 0 on disc; runtime sets to 1 after pointer fixup
u32 nobj      // number of objects
```

The `0x80000002` magic was confirmed via the dev string `"Model Version Err: %x"` in `FUN_80026B4C` - the registration function bails when `*tmd != 0x80000002`.

## Object table (28 bytes per object × nobj)

```
u32 vert_top      // byte offset from end of header (0x0C)
u32 n_vert
u32 normal_top    // byte offset from end of header
u32 n_normal
u32 prim_top      // byte offset from end of header
u32 n_primitive   // SUM of all primitives across primitive-section groups
i32 scale         // ALWAYS 0x00808080 in Legaia (Legaia-custom; standard
                  // PSX uses signed log2 scale)
```

After `FUN_800268DC` runs at load time, `vert_top` / `normal_top` / `prim_top` are patched in-place to absolute RAM addresses. Static tools should NOT do this patch - they should use the offsets as `(ptr_base + offset)` where `ptr_base = 12` (HEADER_SIZE).

## Vertex / normal data

Both are arrays of `SVECTOR { i16 x, y, z, pad }` = 8 bytes each.

```rust
pub struct Vector { pub x: i16, pub y: i16, pub z: i16, pub _pad: i16 }
```

## Primitive section

The primitive section is a sequence of **groups**. Each group has an **8-byte header** followed by a fixed-stride array of prim data.

```
group header (8 bytes):
  +0  u16 count          // how many primitives in this group
  +2  u16 flags          // selects entry in per-mode table (see below)
  +4  u8  olen           // PSX SDK "output length" (packet word count)
  +5  u8  ilen           // PSX SDK "input length" -- per-prim WORD stride
                         // per-prim byte stride = ilen * 4
  +6  u8  flag           // PSX SDK flag byte (lighting / shading / etc)
  +7  u8  mode           // PSX SDK mode byte (FT3/FT4/GT3/GT4/etc)
prim data (count × ilen*4 bytes):
  count × [ilen u32 words]
```

`n_primitive` in the OBJECT header is the **sum** of `count` across all groups in the object's primitive section.

The per-prim layout depends on the prim type. The renderer (`FUN_8002735C`)
treats the 8-byte-stride table at `0x8007326C` as a packed `{u32 first; u32
second}` per row, selects the row with `row = ((flags >> 1) - 8) >> 1`, and
reads exactly **two fields** from it (`8002735c.txt:80027460..80027500`):

- **`byte3`** = `first >> 24` → the **shape selector** (stored to `sp+0xbb`).
- **`byte4`** = `second & 0xFF` → the **base vertex-index offset** in u16 units
  (stored to `sp+0xbc`).

Bytes 0/1/2 of `first` and 1/2/3 of `second` are not consumed by the fast-path
dispatch. The accurate per-row projection (matching
[`legaia_tmd::descriptor`](../../crates/tmd/src/descriptor.rs)'s `TABLE`):

| flags   | row | raw 8 bytes               | byte3 (shape) | byte4 (vtx off) |
|---------|-----|---------------------------|---------------|-----------------|
| 0x10/11 | 0   | `04 00 00 05 07 00 00 00` | 0x05          | 0x07            |
| 0x12/13 | 1   | `09 00 00 07 06 00 00 00` | 0x07          | 0x06            |
| 0x14/15 | 2   | `04 00 00 00 02 00 00 00` | 0x00          | 0x02            |
| 0x16/17 | 3   | `06 00 00 02 06 00 00 00` | 0x02          | 0x06            |
| 0x18/19 | 4   | `07 03 00 01 07 00 00 00` | 0x01          | 0x07            |
| 0x1A/1B | 5   | `09 03 00 03 0B 00 00 00` | 0x03          | 0x0B            |
| 0x20-27 | (re-uses rows 0-5 via the same `(flags>>1)-8` math) |  |  |  |

`byte3 & 3` is the **shading / texture family** (`8002735c.txt:80027c50` etc.):
`0 = F` (flat untextured), `1 = FT` (flat textured), `2 = G` (gouraud
untextured), `3 = GT` (gouraud textured); the quad bit `(flags >> 1) & 1`
chooses tri vs quad. So e.g. flags `0x1B` → row 2, byte3 0x00 → **F4** (flat
quad), flags `0x1D` → row 3, byte3 0x02 → **G3** (gouraud tri), `0x1F` → row 3
→ **G4**.

The renderer then derives the per-prim byte stride from `ilen` and the vertex
read offset from `byte4` with a quad adjustment (`vert_off = byte4`, `+2` for a
quad, overridden to `8` when byte3==1 and `0xE` when byte3==3, and the `+2`
cancelled when byte3==0; `× 2` for the byte offset - see
`Descriptor::vertex_offset_bytes`).

### Per-prim color / texture block

The bytes from the prim's start up to the vertex-index offset hold either a
**texture block** (`FT*`/`GT*` textured prims) or a **color block** (`F*`/`G*`
untextured prims) - selected by the packet shape (byte-3 low 2 bits: `1` / `3`
are textured, `0` / `2` are flat / gouraud). [`legaia_tmd::descriptor`](../../crates/tmd/src/descriptor.rs)
resolves the shape and the `vertex_offset` per group.

- **Textured** (`legaia_prims::extract_textures`): `[u0, v0, cba_lo, cba_hi, u1,
  v1, tsb_lo, tsb_hi, u2, v2 (, u3, v3)]`. The block's **start** is
  `Descriptor::texture_block_offset`, resolved from the table entry's **byte 1**
  (the renderer's per-vertex colour base at `sp+0xb9`):
  - **byte1 = 0** (rows 0/1, flags `0x10`-`0x17`): the light-source-lit variant.
    The texture block is at **offset 0** (ahead of the vertices); per-vertex data
    trails the vertex indices. e.g. Rim Elm env-mesh pack 36 `FT4`/`GT3`/`GT4`
    prims carry `cba = 0x7ac?` (CLUT row 491) at prim byte 2.
  - **byte1 = 3** (rows 4/5, flags `0x20`-`0x27`): baked per-vertex colours
    precede the block - **one** colour word for flat `FT*` (block at byte 4),
    `n_vertices` words for gouraud `GT*` (block at byte 12 for `GT3`, byte 16 for
    `GT4`). Here the block does end at the vertex-index offset.

  The earlier walker assumed `block_start = vertex_offset - block_len`, which only
  held for the byte1 = 3 rows; the byte1 = 0 rows then read `(cba, tsb)` from
  geometry bytes and rendered as rainbow garbage (the Rim Elm decorative-plant
  props). `texture_block_offset` fixes this while leaving rows 4/5 byte-identical.
- **Untextured**: a per-vertex RGB colour block (PSX `[R, G, B, code]` word; the
  4th byte is the SDK GP0 command/code, not part of the colour). **Flat**
  (`F3`/`F4`) stores **one** colour word at prim offset 0, shared by all corners
  (the renderer loads it into the GTE RGBC reg and runs `NCDS` once); **gouraud**
  (`G3`/`G4`) stores one colour word **per corner** at a 4-byte stride from prim
  offset 0 (`colour[v] = bytes[v*4 .. v*4+3]`, `NCDS` per vertex).

The exact untextured record layouts (colour block first, then the vertex-index
block), pinned byte-exact from the renderer + real town01 props (pack 31 obj
315, pack 109 obj 114):

```
F4 (flags 0x1B), ilen 3, 12-byte record:
  [0..4)   colour word  R G B code   (one, broadcast to all 4 verts)
  [4..12)  4 × u16 vertex byte-offsets (÷8 = index); quad winding [0,1,3,2]

G3 (flags 0x1D), ilen 5, 20-byte record:
  [0..4)   v0 colour     [4..8)  v1 colour     [8..12) v2 colour
  [12..18) 3 × u16 vertex byte-offsets          [18..20) pad

G4 (flags 0x1F), ilen 6, 24-byte record:
  [0..16)  4 × colour word (read in winding order [0,1,3,2] via DAT_8007b410)
  [16..24) 4 × u16 vertex byte-offsets
```

The quad winding-remap table `DAT_8007b410 = 00 01 03 02` reorders both the
colour slots and the vertex indices for quads, so `colour[i]` pairs with
`vertex[i]` after the remap.

**No per-prim normal field.** The renderer reads only the prim pointer
(`param_1[4]`), the vertex base (`param_1[0]`) and the loop count
(`param_1[5]`) - it never reads the object header's normal table
(`param_1[2]`/`param_1[3]`). Lighting is the GTE `NCDS` op fed the stored
**colour** word; the object's global light/colour matrices are set once at
function entry. Some TMDs populate a normal array, but the retail renderer
ignores it - there is no per-prim normal offset to decode.

Mis-reading an untextured colour block as a texture block yields bogus `(cba,
tsb)` and samples a random VRAM page - the historic "flat green tint / transparent
hole". `legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid` surfaces both (textured
UVs + untextured colours) for a hybrid render; see
[`character-mesh.md` § Hybrid render](character-mesh.md#hybrid-render-textured--untextured-prims).

## Worked example

Small TMD `0001.tmd` (5 prims, 168-byte section):

| Section offset | Bytes | Meaning |
|---|---|---|
| 0   | `04 00 20 00 07 05 01 27` | Group 1 header: count=4, flags=0x20, olen=7, ilen=5, flag=0x01, mode=0x27 |
| 8   | (20 bytes) | Prim 0 (5 words = 20 bytes; FT3-style) |
| 28  | (20 bytes) | Prim 1 |
| 48  | (20 bytes) | Prim 2 |
| 68  | (20 bytes) | Prim 3 |
| 88  | (zeros)    | Padding |
| 108 | `01 00 22 00 09 06 01 2f` | Group 2 header: count=1, flags=0x22, olen=9, ilen=6, flag=0x01, mode=0x2F |
| 116 | (24 bytes) | Prim 4 (6 words = 24 bytes; FT4-style) |
| 140 | (zeros)    | Trailing padding |

Big TMD `0000.tmd` (760 prims, 15232-byte section):

| Section offset | Bytes | Meaning |
|---|---|---|
| 0   | `f8 02 10 00 07 05 00 26` | Group header: count=0x02F8=760, flags=0x0010, olen=7, ilen=5, flag=0x00, mode=0x26 |
| 8   | (20 bytes per prim) | Prim 0 - start of uniform 20-byte stride |
| 8 + 760×20 = 15208 | Trailing 24 bytes of padding | |

The per-prim data IS uniform 20 bytes (= `ilen*4`) - there are no per-prim sub-headers. Walker: `legaia_tmd::legaia_prims::iter_groups`.

## TMD pointer table

`FUN_80026B4C` writes registered TMDs to `*(int **)(idx * 4 + 0x8007C018)`. Confirmed readers in retail (4 functions, all setup-not-render):

| Function | Role |
|---|---|
| `FUN_80021B04` | Actor-spawn helper; builds per-actor OBJECT pointer table at `actor[0x44]+4` |
| `FUN_80024D78` | Per-actor OBJECT-table rebuild |
| `FUN_8001EBEC` | Per-frame OBJECT[10/11] swap (pose select for player TMDs) |
| `FUN_8001E890` | "DATA_FIELD player loader" - calls `FUN_8003eb98(0x36C, …)` (PROT 876 = `player_data`) and the dev paths `data\field\player.lzs` / `h:\prot\all\data\field\player.lz`. The retail bytes the loader reads (PROT 876 = streaming-format VAB+TIM_LIST+SEQ; the dev `data\field\player.lzs` file is absent from the ISO9660 walk) **do not** carry the `[0..4]` character TMDs. Those come from PROT 0874 (`befect_data`) section 0 - see [`world-map-overlay.md` § Disc-side source of `[0..4]`](world-map-overlay.md#disc-side-source-of-04). What this function *does* do that's still consumed at `DAT_8007C018[0..2]` is the post-install group-count cap (`entry[+0x08] = 10`) and the equipment-conditional patch dispatch into `FUN_8001EBEC`. |

The per-actor `OBJECT[i]` is a 28-byte struct copied into `actor[0x44][i+1]` from `tmd + 12 + i*28` - `sizeof(OBJECT) = 28`.

The renderer itself is documented separately under [renderer subsystem](../subsystems/renderer.md).

## See also

- [PSX TIM](tim.md) - the texture format these meshes sample.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the TMD renderer (`FUN_8002735C`).
- [Monster animation](monster-animation.md) - the enemy keyframe morph applied to these meshes.
- [`subsystems/move-vm.md`](../subsystems/move-vm.md) - the move-table VM that poses character meshes.
