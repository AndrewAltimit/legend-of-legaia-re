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
| `effect_bundle` | Detector for the word `0x02018B0C`, once read as an effect-bundle magic: a type-`0x02` DATA_FIELD chunk header on a `pack` of 29 TMDs. See [`effect.md`](../../docs/formats/effect.md#on-disc-effect-bundle-magic-0x02018b0c). |
| `efect_pack` | The *runtime* `efect.dat` 2-pack (`[u32 pack0_off][u32 pack1_off][sprite atlas][pack0][pack1]`), recognised on the pack tables' own self-consistency. Distinct from `effect_bundle`; see [`docs/formats/effect.md`](../../docs/formats/effect.md). |
| `data_field_truncated` | Retired class (produced by nothing): its one hit, 0892, is a `pack` of two TIMs whose header words the streaming reader took for chunk headers, and the three entries once named as members are ordinary four-chunk bundles. Pinned at zero in the validation suite. |
| `anm_detect` | On-disc ANM (asset type 0x06) shape check wrapping `legaia_anm::parse`. |
| `tmd_size_prefix` | Sister of `scene_tmd_stream`: `[u32 prefix][TMD]` with no trailing stream. |
| `tim_scan` / `tmd_scan` | Brute-force magic search for TIMs / Legaia TMDs inside an entry, raw or behind LZS. |
| `sound_pack` | Per-scene `.dpk` / `sound_data2`: a VAB + SEQ bundle in the type-2-terminated streaming container (chunk 0 = VAB header, chunk 1 = VAB sample pool, chunk 2 = SEQ). `extract` reconstitutes the contiguous VAB + slices the SEQ. |
| `scene_vab_stream` | `[u32 chunk0][VABp ...]`: the chunk stream whose leading chunk is a VAB header. See [`vab.md`](../../docs/formats/vab.md). |
| `vab_multi_bank` | The disc's one multi-bank VAB archive, extraction 0891 (`monster.snd`): `[u32 reserved][u32 count][u32 start_sector[count + 1]]`, then one two-chunk VAB per bank on its own sectors. Reader `FUN_8003E104`; layout in [`vab.md`](../../docs/formats/vab.md#the-multi-bank-archive-monstersnd). |
| `bse_bank` | `bse.dat`, the **battle** occupant of the runtime SFX descriptor bank (cue ids `>= 0x200`), loaded at battle init by `FUN_8001FA88` (raw TOC `0x37A` = extraction 888) - `[u16 tag][u16 body_offset = 4][8-byte records]` whose columns are the `sfx-table.md` columns (`program`, `tone`, `level`, `flags`, `category`). Extraction 1195 is a scene prescript of the same shape. See [`docs/formats/bse-dat.md`](../../docs/formats/bse-dat.md). |

## Layering

The bottom of the format stack. It depends on the leaf codecs
(`legaia-lzs`, `legaia-bytes`, `legaia-anm`, `legaia-tim`, `legaia-tmd`) and
must not depend on any crate that describes game content: the scene, UI,
minigame and actor format crates all sit on this one, and
[`legaia-disc-audit`](../disc-audit/README.md) sits on all of them.
