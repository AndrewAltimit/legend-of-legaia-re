# Asset descriptor format

A `(type_size, data_offset)` pair list walked by `FUN_80020224`. The walker has zero static xrefs in `SCUS_942.54` and is reached at runtime from the town/field overlay's main init path. Implementation: `crates/asset/src/lib.rs::parse_player_lzs`.

## Layout

```
u32 count
u32 (header word, purpose unknown)
u32 type_size_0   u32 data_offset_0
u32 type_size_1   u32 data_offset_1
...
```

Each descriptor pair is `(type_size, data_offset)` where `data_offset` is byte-relative to the buffer start, and `type_size` packs the [asset-type byte](asset-type.md) in the high byte.

## How `FUN_80020224` got reached

`FUN_801D6704` (the town overlay's `MAIN_INIT`) calls `FUN_80020224` at `0x801D6B0C` with `a0 = 0`. The result is stored at `0x80087AF8`. So the format IS exercised by retail gameplay - through a runtime overlay rather than the on-disc `SCUS_942.54` static call graph.

## Strict-scan against the disc

Strictly scanning all 1232 PROT entries against this format finds zero hits. That's consistent with both "format isn't on disc as a top-level entry" and "format is on disc but indexed differently" (e.g., embedded inside a scene asset table or a compressed payload). The parser is left in place for when a real on-disc consumer surfaces.

## Note on naming

The Rust function is named `parse_player_lzs` for historical reasons - the format was first encountered while investigating `player.lzs` chains. The format itself isn't player-specific.

## See also

- [Asset-type dispatch](asset-type.md) - the type-byte handler this descriptor pairs feed.
- [DATA_FIELD streaming](data-field.md) - the streaming container that embeds descriptor-shaped chunks.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader chain that walks descriptors at runtime.
