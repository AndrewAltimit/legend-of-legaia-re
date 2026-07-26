# Per-scene field map (`DATA\FIELD\<scene>.MAP`)

Slot 0 of every scene's CDNAME block is an entry of **exactly `0x12000` bytes**,
and that number is the sum of the four regions the runtime addresses off the
per-scene field buffer `*(0x1F8003EC)`. The scene loader streams the entry
verbatim into that buffer, so a region's file offset and its runtime offset are
the same number.

Detection class: `field_map`. Container parser + region map:
[`crates/asset/src/field_map.rs`](../../crates/asset/src/field_map.rs).
Confidence: **Confirmed** - every offset below is read off a consumer's
disassembly, and the region sizes sum to the on-disc footprint with nothing
left over.

The **per-field semantics** of each region - wall nibbles, floor-elevation
tiers, the two floor-height models, the four trigger kinds and what each one
does to the player - live in
[`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). This page
is the container: what regions exist, where they start, and how the format is
recognised from bytes alone.

## Region map

```text
+0x00000  0x4000  object / actor descriptor table   0x20 stride, 512 slots
+0x04000  0x4000  collision + floor grid            1 byte/tile,  0x80-byte rows
+0x08000  0x8000  per-tile object-index map         u16/tile,    0x100-byte rows
+0x10000  0x2000  per-tile trigger block            header + 4 sub-tables
---------------
 = 0x12000
```

| Region | Consumer that pins the offset | Instructions |
|---|---|---|
| `+0x00000` object descriptors | `FUN_8003A55C` | `andi s2,v0,0x1ff` then `sll v0,s2,0x5` - a cell's low 9 bits scaled by `0x20` |
| `+0x04000` collision grid | `FUN_80019278`, `FUN_8003A55C` | `sll v0,a2,0x7` / `addiu v0,v0,0x4000`; `lbu v0,0x4000(v0)`; next-row neighbour `lbu v1,0x80(s0)` |
| `+0x08000` object-index map | `FUN_80019278` | `sll v0,v0,0x7` / `andi v0,v0,0x7f00` / `ori v0,zero,0x8000` |
| `+0x10000` trigger block | `FUN_801D5630` | `lui a3,0x1` for the primary block, `ori v0,v0,0x2000` for the fallback |

Dumps: `see ghidra/scripts/funcs/8003a55c.txt`,
`ghidra/scripts/funcs/80019278.txt`,
`ghidra/scripts/funcs/overlay_cutscene_mapview_801d5630.txt`.

Both grids are `128 x 128` tiles of 128 world units. The collision grid's row
stride is `0x80` (1 byte per tile) and the object map's is `0x100` (one `u16`
per tile), which is why the two regions differ in size despite covering the
same tiles.

## Trigger block (`+0x10000`)

The block opens with an 18-byte header naming four sub-tables, then the four
bodies laid back-to-back:

```text
+0x00   u16  end_offset      ; end of the last sub-table body, + 2
+0x02   s16  offset[0]       ; body offset, relative to the block base
+0x04   s16  count[0]
+0x06   s16  offset[1]
+0x08   s16  count[1]
+0x0A   s16  offset[2]
+0x0C   s16  count[2]
+0x0E   s16  offset[3]
+0x10   s16  count[3]
+0x12        bodies, each followed by a 2-byte gap
```

The header slots are pinned by the sub-table walker `FUN_801D5AE0`, which for
kind `k` forms `v1 = block + k*4` and then reads `lh v0,0x2(v1)` (body offset)
and `lh v1,0x4(v1)` (record count). Its record stride comes from a separate
4-byte table in SCUS BSS, `DAT_8007B318 + k` (`addiu v0,v0,-0x4ce8` off
`lui 0x8008`), and its match test is `rec[0] == tile_x && rec[1] == tile_z` -
`lbu v0,0x0(a3)` / `lbu v0,0x1(a3)` with the cursor advanced by
`addu a3,a3,a0`. Retail's strides are `4, 4, 4, 8`.
`see ghidra/scripts/funcs/overlay_cutscene_mapview_801d5ae0.txt`.

The fallback window is a property of the **read**, not of the file. The index
path of the scene-asset loader `FUN_8001F7C0` issues a `0x28`-sector request
(`li a1,0x28`, byte total `0x14000` - `lui s1,0x1` / `ori s1,s1,0x4000`), which
is `0x2000` bytes more than the map. Those trailing sectors are the *next* PROT
entry's leading bytes, and `FUN_801D5630` searches them with the same header
shape when the primary block misses. The same loader stages
`DATA\FIELD\<scene>.PCH` at `+0x12000` (zero-filling `0x800` bytes when the open
fails - `lui v0,0x1` / `ori v0,v0,0x2000`, `li a2,0x800`) and sets
`_DAT_8007B8D0 = base + 0x12800` (`lui a0,0x1` / `ori a0,a0,0x2800`), which is
what fixes `0x12000` as the end of the map proper.
`see ghidra/scripts/funcs/8001f7c0.txt`.

## Detection

`size == 0x12000` **and** a trigger-block header that chains: `offset[0]` equal
to the end of the header (`0x12`), each following `offset[k]` equal to the
previous body's end plus the 2-byte gap, and the `u16` at `+0x00` equal to the
end of the last body plus 2.

Across the whole PROT corpus that chain holds for 100 entries with **zero**
false positives, and **no** entry outside the `0x12000` size class satisfies it
at `+0x10000`. The chain is a five-way agreement over nine header words, so it
is not a shape random data falls into.

### The footprint is necessary, not sufficient

`0x12000` is 36 sectors - a size, not a signature. **111** PROT entries carry
it and only **101** are field maps. The other ten are ordinary members of their
scene blocks that happen to be that long:

| Entry | Block slot | Class |
|---|---|---|
| 63, 71, 378, 379, 701 | dolk+5, dolk2+5, taiku+9, taiku+10, rugi+7 | `scene_tmd_stream` (`[u32 size]` then the `0x80000002` TMD magic) |
| 648 | nilboa2+4 | `data_field_streaming` |
| 1074, 1087, 1089, 1187 | inside the `vab_01` block | `scene_vab_stream` (`VABp` at `+4`) |

The trigger chain rejects all ten, so the class is right - but a *reader* that
resolves "the field map" by footprint alone picks up ten strangers, five of
them inside named scene blocks. Resolve by slot (below), not by size.

### The zeroed trigger header

One `0x12000` entry ships the header all zeros: `rikuroa2`'s map (extraction
126), a cutscene-only scene. Its object table and collision grid are entirely
zero; its object-index map carries a single stray byte (`0x04` at `+0xC081`)
and its trigger *body* past the zeroed header is not empty either. The detector
accepts it with an absent trigger block rather than special-casing the scene,
because "no walkable field" is a legitimate state for a scene that is only ever
entered to play a script.

That acceptance is gated on **the object table and the collision grid both
being entirely zero**. Ungated it is not a test: an all-zero `0x12` -byte
window is a statement about 18 bytes, and entries 63 / 71 / 701 - TMD-stream
entries with a fully populated collision grid - satisfy it by accident. Pairing
the zeroed header with an empty field is what makes the escape hatch a
statement about the file.

### Why this size class hid for so long

Nothing about the byte statistics of a field map says "content". The four
regions are sparse by construction - a small map leaves most of two `128 x 128`
grids at zero - so before the class existed these entries scattered across
`mostly_zeros` (a *placeholder* verdict, which is the wrong answer for the file
that carries every scene's collision data) and `unknown_low_entropy`, with a
handful of the densest landing in `unknown_other`. Only the fixed footprint and
the trigger-block chain separate them from padding.

The corollary is a measurement trap worth carrying: a statistical class is not a
verdict. See
[`tooling/disc-coverage.md`](../tooling/disc-coverage.md#a-statistical-class-is-not-a-verdict).

## Numbering

The map is **slot 0 of the scene's block in extraction space**, i.e. extraction
index `define - 2` under the
[+2 numbering correction](cdname.md#numbering-space). Resolving it by "the first
`0x12000` entry at or after the scene's label" instead picks the *next* scene's
map - a mistake that once loaded the wrong collision grid for every field scene.
The runtime's own resolution is `FUN_8003E8A8`'s `toc[idx + 2]`; the engine
mirrors it in `Scene::field_map_index`. Two blocks outside the named-scene range
(`other1`, `other7`) also carry a `0x12000` slot 0, so the size class is not
exactly "one per named scene".

## The map alone does not draw a floor

The `+0x04000` grid holds only a 4-bit **tier** per tile. The elevation it
stands for lives in the 16-entry `s16` floor-height LUT that `FUN_8003AEB0`
installs at scene entry from the scene **MAN** header, so the visible ground is
the product of two separately-resolved assets:

```text
  scene bundle entry -> MAN payload -> floor-height LUT -> ground surface
                                                        -> every object's world Y
```

Both consumers fail *quietly* when the second link breaks.
`Scene::walk_heightfield` returns `None` with no LUT - no ground surface at all,
not a flat one - and `field_env::resolve_env_draws` falls back to `world_y = 0`
for every placed object and terrain tile. The result is a scene that still draws
its walls and props, flattened onto the origin plane, over nothing.

So a floorless field scene is a symptom of MAN resolution, not of this file: the
map parses, the object layer resolves, and the placement counts stay right.
`crates/engine-core/tests/field_floor_layer_disc.rs` is the corpus-wide guard -
it asserts the MAN and the LUT resolve for every field-map block on the disc bar
three named ones, because a draw-count assertion cannot see this failure.

## See also

- [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md) - the
  per-field semantics of every region, and the runtime that consumes them.
- [`scene-v12-table.md`](scene-v12-table.md) - the `.PCH` sidecar the loader
  stages at `+0x12000`.
- [`field-pack.md`](field-pack.md) - the field-asset region behind the map
  (`_DAT_8007B8D0 = base + 0x12800`).
- [`prot.md`](prot.md) - the TOC whose entry footprint fixes the `0x12000` size.
