# legaia-asset

Legaia asset descriptor parser, dispatcher, and the structural detectors
that classify raw PROT entries.

The game's loader (`FUN_8001f05c` in `SCUS_942.54`) takes a buffer plus a
single `u32` packing `(type << 24) | (size & 0xFFFFFF)` and dispatches to a
type-specific handler. Each asset can be either LZS-compressed (the
common case - handled by `FUN_8001a55c` via [`legaia-lzs`]) or stored raw
(handled by `FUN_8001a8b0`, a sized memcpy).

## What it provides

### Core descriptor + decoder

- `AssetType` - the enum of known asset categories.
- `Descriptor` - `(type, size, data_offset)` parsed from the on-disc form.
- `decode` - apply a `Descriptor` + `DecodeMode` to a buffer.
- `parse_player_lzs` - header parser for `player.lzs`-style containers.

### Streaming + pack formats

- `pack` - used inside DATA_FIELD streaming chunks. Header is
  `u32 count` then `u32 word_offsets[count]`.
- `parse_streaming` - DATA_FIELD streaming-chunk walker
  (entry point: `FUN_8002541c`).

### Structural detectors (for `categorize`)

| Module | What it detects |
|---|---|
| `categorize` | Dispatcher - runs every detector and tags the entry's `Class`. |
| `mips_overlay` | RAM overlays loaded into the `0x801C0000+` window. |
| `overlay_ptr_table` | Sister format: pointer tables that index into overlays. |
| `effect_bundle` | `efect.dat` and friends - magic `0x02018B0C`. |
| `field_pack` | Field bundles - magic `0x01059B84`. |
| `battle_data_pack` | `battle_data` pack: streaming preamble + 12-byte record table + per-record LZS streams. |
| `monster_archive` | Global monster stat archive (PROT 867, extended footprint): per-id `0x14000` LZS slot; `record(entry, id)` → name / HP / MP / stats, `mesh(entry, id)` → the monster's embedded battle-model TMD (record `+0x04`), and `MonsterMesh::texture()` → the decoded texture pool (record `+0x08`: fifteen 16-colour CLUTs at `[0..0x1E0]` + a 4bpp page, layout from the loader `FUN_80055468`; palette = `cba & 0x3F`). `animations(entry, id)` / `idle_animation(entry, id)` decode the per-action transform-keyframe streams (one `MonsterAnimation` per action entry: `part_count` objects × `frame_count` `PartPose` translation+rotation keyframes; action 0 = idle). CLI `asset monster-archive --id N --obj <out>` exports the mesh, `--texture-png <out>` bakes the texture page, and `--anim` lists the action animations. See [`battle.md`](../../docs/subsystems/battle.md#monster-mesh-record-0x04) and [`monster-animation.md`](../../docs/formats/monster-animation.md). |
| `befect_cluster` | Cluster-aware extraction of the battle-effect `befect_data` cluster (CDNAME 872, PROT 872..875). The naive per-entry extractor over-reads it (the entries overlap on disc), so `extract(archive, cdname)` footprint-bounds each entry, expands the LZS-container entry into its sections, and classifies each part (the `efect.dat` 2-pack / effect-model TMDs / effect-texture TIMs / packs). CLI `asset befect-cluster PROT.DAT --cdname CDNAME.TXT --out DIR`. See [`effect.md`](../../docs/formats/effect.md#battle-effect-cluster-befect_data-cdname-872). |
| `stage_geom` | Stage geometry: 12-byte prefix + 8-byte u16 quad records. |
| `item_names` | `SCUS_942.54` item-name table (`PTR_DAT_8007436C[id*3]`, 256 ids): `ItemNameTable::from_scus` → `name(id)`. The id space a monster record's `drop_item` indexes; used by the web viewer's enemy table. See [`item-table.md`](../../docs/formats/item-table.md). |
| `spell_names` | `SCUS_942.54` spell table (`DAT_800754C8`/`DAT_800754D0`, 256 ids): `SpellNameTable::from_scus` → `name(id)` / `mp(id)`. Resolves a monster's global magic-attack ids (`MonsterRecord::magic_attacks`, record `+0x21..=+0x23`) into the on-screen spell name (`0x27` → `Tail Fire`). See [`spell-table.md`](../../docs/formats/spell-table.md). |
| `new_game` | `SCUS_942.54` new-game starting-party template (`0x80078C4C`, 4 records, 26-byte stride): `StartingParty::from_scus` → per-member opening stats + name (Vahn/Noa/Gala/Terra). The seed for the live `0x80084708 + n*0x414` records; `OPENING_SCENE` = `town01`. See [`new-game-table.md`](../../docs/formats/new-game-table.md). |
| `cutscene_text` | Inline cutscene-narration text embedded in a field-VM cutscene-timeline record: `parse_narration` / `narration_pages` decode the `0x1F`/`0x00`-framed ASCII subtitle pages introduced by a `0x4C` op whose operand declares the page count (the `opdeene` opening-prologue narration). See [`cutscene.md`](../../docs/subsystems/cutscene.md#inline-narration-format). |
| `scene_tmd_stream` | `[u32 chunk0][bare TMD][streaming chunks]`. |
| `scene_vab_stream` | `[u32 chunk0][VABp ...]`. |
| `scene_asset_table` | Per-scene asset slot table (CDNAME block layout). |
| `scene_v12_table` | Variant of the per-scene table. |
| `tim_scan` / `tmd_scan` | Brute-force magic search inside an entry. |

Detector coverage and provenance are tracked in
[`docs/formats/scene-bundles.md`](../../docs/formats/scene-bundles.md).

## CLI

```bash
asset describe         <input>            # parse + print descriptor
asset decode           <input> <output>   # apply the dispatcher
asset categorize       <PROT.DAT> [--cdname <CDNAME.TXT>]
asset find-overlay     <PROT.DAT>         # MIPS-code candidate scan
asset tim-scan         <input>            # locate embedded TIMs
asset tmd-scan         <input>            # locate embedded TMDs
asset stage / stage-scan
asset field-pack / field-pack-scan
asset effect-bundle / effect-bundle-scan
asset battle-data-pack / battle-data-pack-scan
asset extract <PROT.DAT> <out_dir>        # full per-entry extraction
asset validate                            # cross-check detector coverage
```

`asset --help` lists the rest. `categorize` is the one most other tools
key off - its JSON output drives the asset-viewer's "browseable PROT
entry list" and the cross-reference scripts under [`scripts/`](../../scripts/).

## See also

- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  - full asset-loader chain from disc to dispatch.
- [`docs/formats/`](../../docs/formats/overview.md) - per-format byte-level
  specs that this crate's detectors implement.
