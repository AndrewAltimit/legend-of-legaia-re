# Asset descriptor format

A `(type_size, data_offset)` pair list walked by `FUN_80020224`. The walker has zero static xrefs in `SCUS_942.54` and is reached at runtime from the town/field overlay's main init path. Implementation: `crates/asset/src/lib.rs::parse_player_lzs`.

## Layout

```
u32 count
u32 total_decompressed_size   ; = sum(descriptor[i].size); retail never reads it
u32 type_size_0   u32 data_offset_0
u32 type_size_1   u32 data_offset_1
...
```

Each descriptor pair is `(type_size, data_offset)` where `data_offset` is byte-relative to the buffer start, and `type_size` packs the [asset-type byte](asset-type.md) in the high byte with the payload's **decompressed** size in the low 24 bits.

## The second header word is the bundle's total decompressed size

`+0x04` is `sum(descriptor[i].size)` exactly - how many bytes the container unpacks
to in total. A structural sweep of all 1233 extracted PROT entries for this shape
(`count` in `1..=64`, descriptor 0's `data_offset == 8 + count*8`, every type byte
`<= 0x14`, every offset inside the entry) yields **105** carriers, and the identity
holds in **105 of 105**. In all 105 the word also exceeds the carrying entry's own
byte length, which is what rules out the "file size" and "sector count" readings.
The count word is not fixed: the 105 split `{7: 80, 4: 10, 6: 8, 5: 4, 3: 2, 1: 1}`.

Retail never reads it. `FUN_80020224` takes the count from `+0x00` (`80020288`
`lw s3,0x0(s4)`) and steps descriptor pairs from `+0x08` (`8002029c`
`lw a0,0xc(s0)` / `lw a1,0x8(s0)` with `s0 += 8` per iteration), so `+0x04` is
never addressed. Sweeping every image for a dereference of the table-base pointer
`_DAT_8007B85C` - SCUS plus all 83 extracted overlays, `lui`+load pairs and
materialised-base+displacement walks (`scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x8007b85c`) -
finds 69 access sites: one store (`0x8001E2A0`, the allocator in `FUN_8001E1B4`
publishing the buffer base) and four sites that dereference the loaded pointer,
**every one of them at displacement `0x0`**. Every other site hands the pointer
straight to a callee as a load destination. So the word is an authoring total:
Confirmed as a format fact, inert at runtime, and therefore a consistency check an
editor must maintain rather than a value to recompute freely
(`SceneAssetTable::total_size_is_consistent`).

The same word under its `scene_asset_table` name, with the per-descriptor
semantics, is on
[`scene-bundles.md`](scene-bundles.md#0x04-is-the-bundles-total-decompressed-size).

## How `FUN_80020224` got reached

`FUN_801D6704` (the town overlay's `MAIN_INIT`) calls `FUN_80020224` at `0x801D6B0C` with `a0 = 0`. The result is stored at `0x80087AF8`. So the format IS exercised by retail gameplay - through a runtime overlay rather than the on-disc `SCUS_942.54` static call graph.

## The format is on the disc as a top-level entry

An earlier revision of this page recorded that "strictly scanning all 1233 PROT
entries against this format finds zero hits", and read that as the container
existing only after an LZS chain had been decoded. That is **falsified**: the
sweep above finds 105 top-level carriers, which is the same corpus
[`scene-bundles.md`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
classes as `scene_asset_table` plus the `lzs_container` shapes
(`parse_player_lzs`) - the scene bundles, `player.lzs`-style character/effect
containers and the minigame art container all wear this header at offset 0 of
their own entry. The zero-hit result came from a detector that required a fixed
descriptor count, not from the disc.

## Note on naming

The Rust function is named `parse_player_lzs` for historical reasons - the format was first encountered while investigating `player.lzs` chains. The format itself isn't player-specific.

## See also

- [Asset-type dispatch](asset-type.md) - the type-byte handler this descriptor pairs feed.
- [Scene bundles](scene-bundles.md) - the `scene_asset_table` family that carries this header on disc.
- [DATA_FIELD streaming](data-field.md) - the streaming container that embeds descriptor-shaped chunks.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader chain that walks descriptors at runtime.
