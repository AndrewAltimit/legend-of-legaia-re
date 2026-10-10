# Asset descriptor format

An asset descriptor table is the header of a multi-asset bundle: a count, then one `(type_size, data_offset)` pair per asset. Each pair says what kind of asset it is, how big it is once unpacked, and where its (usually [LZS](lzs.md)-compressed) bytes start. Scene bundles, the `player.lzs`-style character / effect containers and the minigame art container all open with this header at offset 0 of their own PROT entry.

Retail walks the table with `FUN_80020224`, which feeds every pair to the [asset-type dispatcher](asset-type.md). Implementation: `legaia_asset::parse_player_lzs` (parser) and `walk_descriptor_pairs` beside it in [`crates/asset/src/lib.rs`](../../crates/asset/src/lib.rs). The parser name is a leftover from `player.lzs`; the format is not player-specific.

## Layout

```
+0x00  u32 count
+0x04  u32 total_decompressed_size
+0x08  u32 type_size_0    u32 data_offset_0      descriptor 0
+0x10  u32 type_size_1    u32 data_offset_1      descriptor 1
...
+0x08 + count*8           first asset's bytes (descriptor 0's data_offset)
```

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | `count` | Number of descriptor pairs. Not fixed: 1, 3, 4, 5, 6 and 7 all occur | Confirmed |
| `+0x04` | u32 | `total_decompressed_size` | `sum(descriptor[i].size)`. Retail never reads it | Confirmed |
| `+0x08 + i*8` | u32 | `type_size` | [Asset-type byte](asset-type.md) in the high 8 bits, **decompressed** size in the low 24 | Confirmed |
| `+0x0C + i*8` | u32 | `data_offset` | Byte offset of the asset's data, relative to the buffer start | Confirmed |

Nothing bounds the count, so a detector anchors on descriptor 0 instead: its `data_offset` equals `8 + count*8`.

## The second header word is the bundle's total decompressed size

`+0x04` is `sum(descriptor[i].size)` exactly - how many bytes the container unpacks
to in total. A structural sweep of all 1233 extracted PROT entries for this shape
(`count` in `1..=64`, descriptor 0's `data_offset == 8 + count*8`, every type byte
`<= 0x14`, every offset inside the entry) yields **105** carriers, and the identity
holds in **105 of 105**. In all 105 the word also exceeds the carrying entry's own
byte length, which rules out a "file size" or "sector count" reading.
The 105 split by count as `{7: 80, 4: 10, 6: 8, 5: 4, 3: 2, 1: 1}`.

Retail never reads it. `FUN_80020224` takes the count from `+0x00` (`80020288`
`lw s3,0x0(s4)`) and steps descriptor pairs from `+0x08` (`8002029c`
`lw a0,0xc(s0)` / `lw a1,0x8(s0)` with `s0 += 8` per iteration), so `+0x04` is
never addressed. Sweeping every image for a dereference of the table-base pointer
`_DAT_8007B85C` - SCUS plus all 83 extracted overlays, `lui`+load pairs and
materialised-base+displacement walks (`scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x8007b85c`) -
finds 69 access sites: one store (`0x8001E2A0`, the allocator in `FUN_8001E1B4`
publishing the buffer base) and four sites that dereference the loaded pointer,
**every one of them at displacement `0x0`**. Every other site hands the pointer
straight to a callee as a load destination.

So the word is an authoring total: a confirmed format fact that is inert at
runtime. An editor must keep it consistent rather than recompute it freely
(`SceneAssetTable::total_size_is_consistent`).

The same word under its `scene_asset_table` name, with the per-descriptor
semantics, is on
[`scene-bundles.md`](scene-bundles.md#0x04-is-the-bundles-total-decompressed-size).

## Who walks it

`FUN_80020224` has zero static callers in `SCUS_942.54`. Its one caller on the disc is in the field overlay: `FUN_801D6704` (the overlay's `MAIN_INIT`) calls it at `0x801D6B0C` with `a0 = 0` and stores the result at `0x80087AF8`. For each pair it calls `FUN_8001F05C(base + data_offset, type_size, param, 0)` and ORs every return into one summary word.

The port runs the same walk at scene load: `scene_asset_table::mesh_pool` walks every scene bundle's table and registers the meshes of the two mesh cases (`0x02` pack, `0x09` bare) in walk order.

## Where it sits on disc

The 105 carriers above are top-level PROT entries, the same corpus
[`scene-bundles.md`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
classes as `scene_asset_table` plus the `lzs_container` shapes. A detector that
requires a fixed descriptor count misses most of them.

## See also

- [Asset-type dispatch](asset-type.md) - the type-byte handler the descriptor pairs feed.
- [Scene bundles](scene-bundles.md) - the `scene_asset_table` family that carries this header on disc.
- [DATA_FIELD streaming](data-field.md) - the other container the dispatcher is reached through.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader chain that walks descriptors at runtime.
