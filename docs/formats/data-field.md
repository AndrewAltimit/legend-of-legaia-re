# DATA_FIELD streaming format

A stream of typed chunks consumed by `FUN_8002541C` on its `0x14` (DATA_FIELD) branch. The walker passes every chunk through the [asset-type dispatcher](asset-type.md) with `copy_only=1`, so chunks are always uncompressed.

Implementation: `crates/asset/src/lib.rs::parse_streaming`.

## Layout

```
[u32 type_size] [size_bytes of raw data]
[u32 type_size] [size_bytes of raw data]
...
[u32 terminator]    // type_size with low 24 bits all zero
```

Where:
- `type_size = (type_byte << 24) | (size_bytes & 0x00FFFFFF)`.
- `type_byte` matches the [asset type table](asset-type.md).
- The next chunk header starts at `current_pos + 4 + (size & ~3)` — i.e., header + size truncated to a 4-byte boundary. Sizes are always 4-aligned in practice.
- Terminator: any header `u32` whose low 24 bits are zero.

## What's in the wild

Strict-validating PROT entries with `asset scan-stream` finds 26 hits, concentrated in the `_other5` cluster (entries 1214–1219+) plus a few in `dolk2`, `rikuroa2`, `rayman`. Each entry has 3 chunks of single-asset shape:

```
chunk[0]: TIM   (single, magic 0x10)        — sprite atlas / texture
chunk[1]: TMD2  (single, magic 0x80000002)  — single Legaia TMD
chunk[2]: MOVE2 (single, magic 0x08)        — animation data
terminator
```

The chunk layouts are **single assets** here (one TIM, one TMD2, one MOVE2), not packs. Other clusters elsewhere in the corpus do use pack-shaped TIM_LIST / TMD chunks; the [pack format](pack.md) handles that case.

## Trailer data

Some entries contain bytes past the streaming terminator. `asset extract` preserves these as `_trailer.bin` next to the extracted chunks. The function that consumes the trailer hasn't been located; tracing the caller of `FUN_8002541C` in the field/town overlay is the next move if a specific entry's trailer looks structured.

## Related shapes

- The [scene-TMD-prefixed streaming](scene-bundles.md) shape is structurally similar but the leading chunk has no `[u32 type_size]` header and the inner content is a bare TMD instead of a TIM.
- The [scene-VAB-prefixed streaming](scene-bundles.md) shape uses the same chunk0-header trick but with VAB content.
- [Pack format](pack.md) lives *inside* TIM_LIST / TMD chunks when the chunk's data is a pack rather than a single asset.
