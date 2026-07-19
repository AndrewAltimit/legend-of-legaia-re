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

**The `flags` word is the relocation sentinel, and it makes the fixup idempotent.** `FUN_800268DC` early-returns when `flags == 1`, and sets `flags = 1` *before* walking the object table, so re-registering an already-relocated TMD is a no-op rather than a double-relocation. Retail depends on this: an actor's per-object render table can be rebuilt from a TMD that may or may not have been relocated yet (`FUN_80024D78`), and the guard is what makes that safe. A TMD reached through that path while still unrelocated hands the renderer file-relative offsets in pointer slots - which is why the fixup runs there at all.

Two implications: a static tool must never write `flags`, since a `1` on disc would make the runtime skip a relocation the mesh still needs; and a clean-room port that parses to typed offsets instead of patching pointers in place is structurally immune to the whole failure mode, so it should not reimplement the relocation.

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

Each row covers **four** flags values - a tri/quad pair for each of two adjacent
`flags` (the low bit of `flags` is not consumed by the row-and-quad math):

| row | flags (tri / quad) | raw 8 bytes               | byte3 (shape) | byte4 (vtx off) | family      |
|-----|--------------------|---------------------------|---------------|-----------------|-------------|
| 0   | 0x10,11 / 0x12,13  | `04 00 00 05 07 00 00 00` | 0x05          | 0x07            | FT, lit     |
| 1   | 0x14,15 / 0x16,17  | `09 00 00 07 06 00 00 00` | 0x07          | 0x06            | GT, lit     |
| 2   | 0x18,19 / 0x1A,1B  | `04 00 00 00 02 00 00 00` | 0x00          | 0x02            | F           |
| 3   | 0x1C,1D / 0x1E,1F  | `06 00 00 02 06 00 00 00` | 0x02          | 0x06            | G           |
| 4   | 0x20,21 / 0x22,23  | `07 03 00 01 07 00 00 00` | 0x01          | 0x07            | FT, baked   |
| 5   | 0x24,25 / 0x26,27  | `09 03 00 03 0B 00 00 00` | 0x03          | 0x0B            | GT, baked   |

`byte3 & 3` is the **shading / texture family** (`8002735c.txt:80027c50` etc.):
`0 = F` (flat untextured), `1 = FT` (flat textured), `2 = G` (gouraud
untextured), `3 = GT` (gouraud textured); the quad bit `(flags >> 1) & 1`
chooses tri vs quad. So e.g. flags `0x1B` → row 2, byte3 0x00 → **F4** (flat
quad), flags `0x1D` → row 3, byte3 0x02 → **G3** (gouraud tri), `0x1F` → row 3
→ **G4**.

### Vertex-index offset

The per-prim byte stride is `ilen * 4`; the vertex-index read offset comes from
`byte4` plus a per-`byte3` quad adjustment. **Triangles** read `byte4` (in u16
units) directly. **Quads** take a per-`byte3` chain - and the two renderers that
walk this table do not agree on it:

| byte3 | rows  | `FUN_80029888` (lit renderer) | `FUN_8002735c` |
|-------|-------|-------------------------------|----------------|
| 0     | 2     | 2                             | `byte4` (= 2)  |
| 1     | 4     | 8                             | 8              |
| 2     | 3     | 8                             | `byte4 + 2` (= 8) |
| 3     | 5     | 0xE                           | 0xE            |
| 5, 7  | 0, 1  | `byte4` (7 / 6)               | `byte4 + 2` (9 / 8) |

They agree everywhere except the **lit textured rows 0/1**, where
`FUN_8002735c`'s `byte4 + 2` fallback reads past the vertex indices and into the
normal-index block that trails them. The on-disc packets put the four vertex
indices of a lit textured quad at **byte 12** for both rows (see the layouts
below), so [`legaia_tmd::descriptor`](../../crates/tmd/src/descriptor.rs) resolves
`6` (u16 units) for both - which is what `FUN_80029888` computes for row 1, and
the one place its row-0 arithmetic (`byte4` = 7 → byte 14) parts company with the
data. Pinned across every field/town env pack (10105 lit quads, 107 scenes):
offset 12 is the only candidate with zero out-of-range indices, and it leaves the
quads planar (0.79 units mean out-of-plane, vs 68-167 for 14/16/18). Guarded by
`crates/engine-core/tests/env_mesh_prims_disc.rs`.

Reading rows 0/1 quads at the `FUN_8002735c` offset makes their indices exceed
the object's vertex count, so every mesh builder drops those prims - the visible
symptom is a town house rendering as a shredded pile (Rim Elm object 137 kept 58
of its 163 prims).

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
- **Colour block** (every row except the lit rows 0/1): a per-vertex RGB colour
  block (PSX `[R, G, B, code]` word; the 4th byte is the SDK GP0 command/code,
  not part of the colour - `0x20`/`0x24`/`0x28`/`0x2C`/`0x30`/`0x34`/`0x38`/`0x3C`,
  optionally `| 2` for the semi-transparent variant, and only on the *leading*
  word of a gouraud packet). **Flat** (`F3`/`F4`/`FT3`/`FT4`) stores **one**
  colour word at prim offset 0, shared by all corners; **gouraud**
  (`G3`/`G4`/`GT3`/`GT4`) stores one colour word **per corner** at a 4-byte
  stride from prim offset 0 (`colour[v] = bytes[v*4 .. v*4+3]`). On a *textured*
  prim this block is what pushes the texture block off offset 0. The renderer
  loads the word into the GTE `RGBC` register and runs `DPCS` (the depth cue) -
  **not** an `NC*` light op - and the GPU then modulates the texel by it
  (`texel * colour / 128`). This is the field's entire lighting signal; see
  [`subsystems/renderer.md`](../subsystems/renderer.md#lighting).

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

### Lit textured records (rows 0/1)

The baked rows above carry their shading in the colour block. The **lit** rows
carry it in **normal indices that trail the vertex indices**, and the texture
block is a fixed 12 bytes for both tris and quads - the quad packs `u3`/`v3` into
the half-word the tri layout leaves as padding:

```
GT3 (flags 0x14/15), ilen 6, 24-byte record:
  [0..12)  [u0 v0][cba][u1 v1][tsb][u2 v2][pad]
  [12..18) 3 × u16 vertex byte-offsets
  [18..24) 3 × u16 normal byte-offsets      (one per corner)

GT4 (flags 0x16/17), ilen 7, 28-byte record:
  [0..12)  [u0 v0][cba][u1 v1][tsb][u2 v2][u3 v3]
  [12..20) 4 × u16 vertex byte-offsets
  [20..28) 4 × u16 normal byte-offsets      (one per corner)

FT4 (flags 0x12/13), ilen 6, 24-byte record:
  [0..12)  [u0 v0][cba][u1 v1][tsb][u2 v2][u3 v3]
  [12..20) 4 × u16 vertex byte-offsets
  [20..22) 1 × u16 normal byte-offset       (flat: one per face)
  [22..24) pad
```

The object header's `n_normal` confirms the block: it is exactly
`1×FT4 + 3×GT3 + 4×GT4` summed over the object's lit groups (Rim Elm env pack
slot 36 object 0: `8 + 44*3 + 100*4 = 540`, its declared normal count). The
**tri** of row 0 (`FT3`, flags `0x10/11`) is the one shape that puts its single
normal *before* the vertices - `[12-byte texture block][n0][v0 v1 v2]` - which is
what row 0's `byte4` (7 → byte 14) encodes.

**Neither renderer lights from the normal array.** `FUN_8002735c` reads only the
prim pointer (`param_1[4]`), the vertex base (`param_1[0]`) and the loop count
(`param_1[5]`) - never the object header's normal table (`param_1[2]` /
`param_1[3]`). `FUN_80029888`, the sibling that handles the lit rows (the only
other function in `SCUS_942.54` that reads the descriptor table), issues no `NC*`
op either: between them the two renderers run exactly one GTE colour op, `DPCS`
(`cop2 0x780010`). So the lit rows' normals are authored but never transformed,
and shading is the stored **colour** word modulating the texel on the GPU. See
[`subsystems/renderer.md`](../subsystems/renderer.md#lighting). The engine port
re-derives smooth normals from the geometry, so it decodes the indices only to
know where the vertex block ends.

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
