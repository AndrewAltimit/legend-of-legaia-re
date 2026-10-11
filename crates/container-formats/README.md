# legaia-container-formats

The container layer every Legaia asset arrives in: the asset-type
dispatcher, the descriptor, the DATA_FIELD chunk stream and the pack format.

The game's loader (`FUN_8001F05C` in `SCUS_942.54`) takes a buffer plus one
`u32` packing `(type << 24) | (size & 0xFFFFFF)` and dispatches to a
type-specific handler; the payload is LZS-compressed (`FUN_8001A55C`, via
[`legaia-lzs`](../lzs/README.md)) or stored raw (`FUN_8001A8B0`).

## What lives here

| Item | Covers |
|---|---|
| `AssetType`, `Descriptor`, `DecodeMode`, `decode` | The type byte, the `(type, size, data_offset)` descriptor and its decode. See [`asset-type.md`](../../docs/formats/asset-type.md), [`asset-descriptor.md`](../../docs/formats/asset-descriptor.md). |
| `parse_player_lzs`, `walk_descriptor_pairs`, `validate` | The `player.lzs`-style descriptor-table container. |
| `parse_streaming`, `parse_streaming_with` | The DATA_FIELD chunk-stream walker. See [`data-field.md`](../../docs/formats/data-field.md). |
| `pack` | The pack a streaming chunk carries (`u32 count`, `u32 word_offsets[count]`). See [`pack.md`](../../docs/formats/pack.md). |
| `data_field_truncated` | Detector for a DATA_FIELD stream cut at the entry boundary. |
| `effect_bundle`, `efect_pack` | The magic-prefixed effect bundle and the runtime `efect.dat` 2-pack. See [`effect.md`](../../docs/formats/effect.md). |
| `anm_detect`, `tmd_size_prefix` | On-disc ANM and size-prefixed TMD detectors. |
| `tim_scan`, `tmd_scan` | Find TIMs / Legaia TMDs embedded in an arbitrary buffer, raw or behind LZS. |

## Layering

The bottom of the format stack. It depends on the leaf codecs
(`legaia-lzs`, `legaia-bytes`, `legaia-anm`, `legaia-tim`, `legaia-tmd`) and
must not depend on any crate that describes game content: the scene, UI,
minigame, sound and battle-model format crates all sit on this one, and
[`legaia-disc-audit`](../disc-audit/README.md) sits on all of them.

`legaia-asset` re-exports every item here at its old `legaia_asset::` path.
