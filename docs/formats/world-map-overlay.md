# World-map overlay outlines

The `slot 4` payload of each world-map (kingdom) bundle holds the
wireframe/outline data the dev-menu top-view renderer draws over the
world map. Three carriers, one per kingdom:

| Bundle | PROT index | CDNAME label |
|---|---|---|
| Drake | 0085 | `map01` |
| Sebucus | 0244 | `map02` |
| Karisto | 0391 | `map03` |

The 7-asset bundle these live in is the [`scene_asset_table`](scene-bundles.md#scene_asset_table---canonical-7-asset-bundle)
shape (`(1, 2, 3, 4, 5, 6, 7)` type sequence). Slot 4's type byte is
`0x05` per [asset-type](asset-type.md) - the standard table calls this
"MOVE", but the kingdom-bundle consumer interprets the bytes as
world-map outline data, **not** as a move table. (Similar mismatch to
the `vab_01` / `move_program_no` CDNAME labels - the type byte is
just a routing tag, and the kingdom path uses it differently.)

## Layout

### Outer pack

```text
+0x00   u32  count                ; number of sub-bodies
+0x04   u32  byte_offsets[count]  ; absolute byte offset into the
                                  ; decoded payload (NOT word offsets,
                                  ; unlike the slot-1 TMD pack)
+offset bodies[count]             ; contiguous sub-bodies
```

Drake decodes to 32304 bytes with `count = 15`. First entry is
`0x40 = 4 + 4*15` (right after the header). Each subsequent entry is
strictly greater; the last body extends to end-of-decoded-payload.

### Sub-body header (8 bytes)

```text
+0x00   u8   count_a              ; records per group
+0x01   u8   flag_a               ; usually 0; 1 in Drake body 13
+0x02   u8   count_b              ; number of groups
+0x03   u8   flag_b               ; usually 0
+0x04   u16  marker               ; constant 0x080C across all bodies
+0x06   u16  kind                 ; 1, 2, or 4 (semantic ambiguous)
```

### Body payload

```text
+0x08   record[count_a * count_b] ; each record is 8 bytes
                                  ; ( i16 x, i16 y, i16 z, i16 attr )
+...    trailer (8 bytes)         ; always 8 zero bytes in Drake
```

Total body size is always `8 + count_a * count_b * 8 + 8`. The math
fits all 15 of Drake's bodies exactly.

### Drake body inventory (reference)

| Body | count_a | count_b | kind | Size | What the points look like top-down |
|---|---|---|---|---|---|
| 0 | 10 | 20 | 1 | 1616 | Inner contour, ~200 vertices |
| 1 | 10 | 20 | 1 | 1616 | Sister contour to body 0 |
| 2 | 10 | 30 | 1 | 2416 | Inner contour, ~300 vertices |
| 3 | 2 | 30 | 2 | 496 | Polyline, X stepping with `(0, 0, 0, 0)` pad records between |
| 4 | 2 | 20 | 2 | 336 | Polyline (20 vertices stepping in X by 256 units, fixed Y/Z) |
| 5 | 10 | 30 | 2 | 2416 | Inner contour (25 unique of 30 groups) |
| 6 | 10 | 26 | 2 | 2096 | Inner contour |
| 7 | 10 | 30 | 2 | 2416 | Inner contour (25 unique of 30 groups) |
| 8 | 10 | 3 | 2 | 256 | 3 IDENTICAL groups of 10 records (reserved/padding) |
| 9 | 12 | 30 | 2 | 2896 | Mid-density feature |
| 10 | 12 | 30 | 2 | 2896 | Mid-density feature |
| 11 | 12 | 10 | 2 | 976 | Small feature |
| 12 | 10 | 120 | 2 | 9616 | **Continent coastline** - 120 segments × 10 sub-points |
| 13 | 14 | 15 | 4 | 1696 | **World-map boundary frame** at ±32K (perimeter only) |
| 14 | 2 | 30 | 2 | 496 | Polyline |

The `flag_a` byte is 1 for body 13 (the boundary frame) and 0 for
every other Drake body. Whether `flag_a` selects between draw modes
or just modifies the count interpretation is not pinned down.

## RAM layout

Slot 4 is loaded **verbatim into RAM** with zero per-byte diffs vs
disc. Drake's load address is `0x8011A664`; bodies sit contiguously
through `0x80122454` (32240 bytes total - the 64-byte header is
the only RAM/disc-offset slip). No runtime fixup is applied.

Confirmed by `scripts/pcsx-redux/verify_slot4_in_ram.py` against a
real PCSX-Redux save state.

## Why it isn't the bulk continent terrain

The top-down rendering of all bodies' points (see
`scripts/slot4_topdown_png.py`) reveals body 12 traces a continent
coastline and body 13 traces the world boundary - both are 1D
contours, not 2D terrain meshes. Total ~4000 vertices across all 15
bodies is far below the ~17000+ vertex positions the textured
continent in the GPU prim pool would need (Drake's pool has 4994
prims, mostly POLY_FT4 quads with 4 verts each).

The bulk continent geometry source is therefore separate. The most
likely candidate is a procedural emitter, sibling of the horizon
emitter [`FUN_801D7EA0`](../subsystems/world-map.md#fun_801d7ea0---world-map-poly_ft4-batch-emitter)
reachable from the per-frame world-map tick `FUN_80016444`. Pinning
that is still open.

## Consumer (presumed)

Slot 4 is loaded by the standard kingdom-bundle loader. The reader
that interprets these bytes is presumably the world-map controller
`FUN_801E76D4` (top-view branch) and/or the developer menu renderer
`FUN_801EAD98`, both documented under [world map subsystem](../subsystems/world-map.md).
The exact reading call has not been pinned down in Ghidra; the
overlap with PSX SVECTOR shape (3 int16 + pad) and the 0x080C marker
suggest the data feeds a small VRAM/GPU-prim-pool generator rather
than the standard TMD renderer (which would have used the type
`0x02` TMD-pack slot 1 instead).

## Tooling

| Script | Role |
|---|---|
| `scripts/decode_slot4_subbodies.py` | Per-body hex dump + header parse + grid-hypothesis analysis; OBJ export per body. |
| `scripts/slot4_to_obj.py` | Combined OBJ writer (polys / lines / points modes). |
| `scripts/slot4_topdown_png.py` | Top-down PGM/PNG renderer of the point cloud (X-Z plane). |
| `scripts/pcsx-redux/verify_slot4_in_ram.py` | Byte-for-byte cross-check of disc-decoded bodies against PCSX-Redux save-state RAM. |

## Open questions

1. **`kind = 1, 2, 4` semantic.** Not yet tied to a draw-mode or
   sub-format. Body 13 (kind = 4) is the only `kind != 2` body that
   spans the full ±32K bounds; could be a "boundary" tag.
2. **Per-record 4th `int16` column (`attr`).** Always 0 for body 4,
   has 22 distinct values across 300 records in body 5, 214 distinct
   values in body 12. Probably packs `(tpage, clut)` or a zone-id;
   depends on the consumer.
3. **Per-body kind→draw routine mapping.** Needs Ghidra capture of the
   consumer (likely in the `world_map_top` overlay).
4. **Whether Sebucus/Karisto slot 4 share the same per-body kind
   counts** or have kingdom-specific layouts.
