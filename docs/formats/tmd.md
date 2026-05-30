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

The per-prim layout depends on the prim type. The renderer (`FUN_8002735C`) indexes into an 8-byte-stride table at `0x8007326C` using `((flags >> 1) - 8) >> 1) * 8`:

| flags  | table idx | byte 0 | byte 4 | meaning of byte 4 |
|--------|-----------|--------|--------|-------------------|
| 0x10/11 | 0 | 0x04 | 0x05 | vertex-index byte offset / 2 within prim |
| 0x12/13 | 1 | 0x09 | 0x07 | (varies per type) |
| 0x14/15 | 2 | 0x04 | 0x00 | |
| 0x16/17 | 3 | 0x06 | 0x06 | |
| 0x18/19 | 4 | 0x07 | 0x07 | FT3-style prims (uVar3 = 7 → vertex idx at byte 14 of 20-byte prim) |
| 0x1A/1B | 5 | 0x09 | 0x0B | FT4-style prims |
| 0x20-23 | 4 | (same as above) | | |

Each entry's first u32 has bytes `[?, ?, ?, type_bits]` where the low 2 bits of byte 3 select the OT packet shape (0/1/2/3 → different DrawPolyXX variants). Each entry's second u32 has the vertex-index offset (in u16 units) within the prim in its low byte.

### Per-prim color / texture block

The bytes from the prim's start up to the vertex-index offset hold either a
**texture block** (`FT*`/`GT*` textured prims) or a **color block** (`F*`/`G*`
untextured prims) — selected by the packet shape (byte-3 low 2 bits: `1` / `3`
are textured, `0` / `2` are flat / gouraud). [`legaia_tmd::descriptor`](../../crates/tmd/src/descriptor.rs)
resolves the shape and the `vertex_offset` per group.

- **Textured** (`legaia_prims::extract_textures`): `[u0, v0, cba_lo, cba_hi, u1,
  v1, tsb_lo, tsb_hi, u2, v2 (, u3, v3)]` — the block ends exactly at the
  vertex-index offset.
- **Untextured**: a per-vertex BGR colour block. **Flat** (`F3`/`F4`) stores one
  RGB (`[r, g, b]` + a code byte) shared by all corners; **gouraud** (`G3`/`G4`)
  stores one RGB per corner at a 4-byte stride (`colour[v] = bytes[v*4 .. v*4+3]`).

Mis-reading an untextured colour block as a texture block yields bogus `(cba,
tsb)` and samples a random VRAM page — the historic "flat green tint / transparent
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
| `FUN_8001E890` | "DATA_FIELD player loader" — calls `FUN_8003eb98(0x36C, …)` (PROT 876 = `player_data`) and the dev paths `data\field\player.lzs` / `h:\prot\all\data\field\player.lz`. The retail bytes the loader reads (PROT 876 = streaming-format VAB+TIM_LIST+SEQ; the dev `data\field\player.lzs` file is absent from the ISO9660 walk) **do not** carry the `[0..4]` character TMDs. Those come from PROT 0874 (`befect_data`) section 0 — see [`world-map-overlay.md` § Disc-side source of `[0..4]`](world-map-overlay.md#disc-side-source-of-04). What this function *does* do that's still consumed at `DAT_8007C018[0..2]` is the post-install group-count cap (`entry[+0x08] = 10`) and the equipment-conditional patch dispatch into `FUN_8001EBEC`. |

The per-actor `OBJECT[i]` is a 28-byte struct copied into `actor[0x44][i+1]` from `tmd + 12 + i*28` - `sizeof(OBJECT) = 28`.

The renderer itself is documented separately under [renderer subsystem](../subsystems/renderer.md).

## See also

- [PSX TIM](tim.md) - the texture format these meshes sample.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the TMD renderer (`FUN_8002735C`).
- [Monster animation](monster-animation.md) - the enemy keyframe morph applied to these meshes.
- [`subsystems/move-vm.md`](../subsystems/move-vm.md) - the move-table VM that poses character meshes.
