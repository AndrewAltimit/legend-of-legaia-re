# legaia-asset

Legaia asset descriptor parser, dispatcher, and the structural detectors
that classify raw PROT entries.

The game's loader (`FUN_8001f05c` in `SCUS_942.54`) takes a buffer plus a
single `u32` packing `(type << 24) | (size & 0xFFFFFF)` and dispatches to a
type-specific handler. Each asset can be either LZS-compressed (the
common case - handled by `FUN_8001a55c` via [`legaia-lzs`]) or stored raw
(handled by `FUN_8001a8b0`, a sized memcpy).

## Contents

- [Core descriptor + decoder](#core-descriptor--decoder)
- [Streaming + pack formats](#streaming--pack-formats)
- [Structural detectors (for `categorize`)](#structural-detectors-for-categorize)
  - [Simple detectors (table)](#simple-detectors-table)
  - [`monster_archive`](#monster_archive)
  - [`move_power`](#move_power)
  - [`element_affinity`](#element_affinity)
  - [`befect_cluster`](#befect_cluster)
  - [SCUS static tables](#scus-static-tables) — `item_names`, `spell_names`, `steal_table`, `new_game`
  - [Cutscene / summon](#cutscene--summon) — `cutscene_text`, `summon_overlay`
  - [Scene + MAN editing](#scene--man-editing) — `man_edit`, scene tables
  - [TIM/TMD scan + catalog](#timtmd-scan--catalog)
- [CLI](#cli)
- [See also](#see-also)

## Core descriptor + decoder

- `AssetType` - the enum of known asset categories.
- `Descriptor` - `(type, size, data_offset)` parsed from the on-disc form.
- `decode` - apply a `Descriptor` + `DecodeMode` to a buffer.
- `parse_player_lzs` - header parser for `player.lzs`-style containers.

## Streaming + pack formats

- `pack` - used inside DATA_FIELD streaming chunks. Header is
  `u32 count` then `u32 word_offsets[count]`.
- `parse_streaming` - DATA_FIELD streaming-chunk walker
  (entry point: `FUN_8002541c`).

## Structural detectors (for `categorize`)

The dispatcher `categorize` runs every detector below and tags each entry's
`Class`. Detector coverage and provenance are tracked in
[`docs/formats/scene-bundles.md`](../../docs/formats/scene-bundles.md).

### Simple detectors (table)

| Module | What it detects |
|---|---|
| `categorize` | Dispatcher - runs every detector and tags the entry's `Class`. |
| `mips_overlay` | RAM overlays loaded into the `0x801C0000+` window. |
| `overlay_ptr_table` | Sister format: pointer tables that index into overlays. |
| `effect_bundle` | `efect.dat` and friends - magic `0x02018B0C`. |
| `field_pack` | Field bundles - magic `0x01059B84`. |
| `battle_data_pack` | `battle_data` pack: streaming preamble + 12-byte record table + per-record LZS streams. |
| `stage_geom` | Stage geometry: 12-byte prefix + 8-byte u16 quad records. |
| `scene_tmd_stream` | `[u32 chunk0][bare TMD][streaming chunks]`. |
| `scene_vab_stream` | `[u32 chunk0][VABp ...]`. |
| `scene_asset_table` | Per-scene asset slot table (CDNAME block layout). Plus `SceneAssetTable::size_word_offset` / `encode_size_word` for rewriting a descriptor's decompressed-size word after a variable-length asset edit. |
| `scene_v12_table` | Variant of the per-scene table. |
| `shop_stock` | Town gold-shop stock records inside a scene MAN (field-VM op `0x49` sub-op `0` = `[count][item_ids][name]`). `scan` byte-scans a decompressed MAN; `locate` decompresses a bundle entry's MAN and returns its [`ShopRecord`]s. Shared read side for the randomizer (`legaia_rando::shop`) and the engine shop catalog (`legaia_engine_core::shop_catalog`). |

### `static_overlay`

Static overlay-extraction pipeline. PSX overlays are clean copies of a
fixed-VA-linked blob, so each runtime overlay (field / battle / …) is extracted
straight from its `PROT.DAT` entry and disassembled at its load base, identity
attached from the source entry — the structural fix for the VA-aliasing identity
problem the `overlay_<label>_<addr>` dump naming works around. `recover_base`
recovers the load base statically from the overlay's own internal `jal` call
graph; `as_loaded` / `fingerprint` / `verify_fingerprint` back the committed map
(`data/static-overlays.toml`); `ghidra_import_jython` / `ghidra_import_driver`
emit the Ghidra import helpers. CLI: `asset overlay list/extract/verify/ghidra/generate`.
Complements the dynamic save-state captures (it does not address runtime values).
See [`docs/tooling/static-overlay-pipeline.md`](../../docs/tooling/static-overlay-pipeline.md).

### `monster_archive`

Global monster stat archive (PROT 867, extended footprint): per-id `0x14000` LZS
slot.

- `record(entry, id)` → name / HP / MP / stats / `element` (record `+0x1D`, the
  `0..=7` element id the `element_affinity` scale `FUN_801dd864` reads
  record-direct via the record-pointer table `0x801C9348[slot-3]`, not a copied
  live-actor field).
- `mesh(entry, id)` → the monster's embedded battle-model TMD (record `+0x04`).
- `MonsterMesh::texture()` → the decoded texture pool (record `+0x08`: fifteen
  16-colour CLUTs at `[0..0x1E0]` + a 4bpp page, layout from the loader
  `FUN_80055468`; palette = `cba & 0x3F`).
- `animations(entry, id)` / `idle_animation(entry, id)` decode the per-action
  transform-keyframe streams (one `MonsterAnimation` per action entry: `part_count`
  objects × `frame_count` `PartPose` translation+rotation keyframes; action 0 =
  idle).

CLI `asset monster-archive --id N --obj <out>` exports the mesh, `--texture-png
<out>` bakes the texture page, `--anim` lists the action animations, and `--glb
<out>` exports the whole thing — mesh + baked texture + every action animation — as
a binary glTF (`monster_gltf::export_glb`; per-object animated nodes + a per-palette
texture atlas).

See [`battle.md`](../../docs/subsystems/battle.md#monster-mesh-record-0x04) and
[`monster-animation.md`](../../docs/formats/monster-animation.md).

### `move_power`

Battle-action **per-move power table** (runtime VA `0x801F4F5C`), the
26-byte-stride table `FUN_801dd0ac` reads for the arts/physical attacker roll, plus
the 128-byte **id → index map** at `0x801F4E63` that resolves a battle move id
(`actor[+0x1df]`) to a record (`param_1 = map[move_id]`).

Static battle-overlay data, pinned in PROT 0898 at fixed raw-entry offsets
(byte-matched against the in-RAM table + map across two battle save states).

- `parse` → records (`MoveRecord::power()` = `+0 >> 2`, `counter_init()` = `+0x04`,
  `sound_cue_id()` = `+0x0d`).
- `parse_id_index_map` + `index_for_move_id` / `record_for_move_id` resolve a move
  id. Remaining record fields open.

CLI `asset move-power <PROT 0898 .BIN>`. See
[`spell-table.md`](../../docs/formats/spell-table.md).

### `element_affinity`

Battle **element-affinity** matrix (runtime VA `0x801F53E8`) + per-character element
table (`0x801F5480`), the static battle-overlay data `FUN_801dd864` reads to scale
the attacker roll.

- `parse` → the 8×8 matrix (`affinity_pct(atk, def)` = `matrix[attacker][defender]`,
  retail diagonal 96 / opposite-pairs 104 / default 100) + `character_element(char_id_1based)`
  (Vahn=fire, Noa=wind, Gala=thunder, Terra=wind).
- `Element` enum names the ids (2/3/4/7 pinned, 0/1/5/6 inferred).

PROT 0898, same link base as `move_power`. CLI `asset element-affinity <PROT 0898
.BIN>`. See
[`battle-formulas.md`](../../docs/subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8).

### `befect_cluster`

Cluster-aware extraction of the battle-effect `befect_data` cluster (CDNAME 872,
PROT 872..875). The naive per-entry extractor over-reads it (the entries overlap on
disc), so `extract(archive, cdname)` footprint-bounds each entry, expands the
LZS-container entry into its sections, and classifies each part (the `efect.dat`
2-pack / effect-model TMDs / effect-texture TIMs / packs).

CLI `asset befect-cluster PROT.DAT --cdname CDNAME.TXT --out DIR`. See
[`effect.md`](../../docs/formats/effect.md#battle-effect-cluster-befect_data-cdname-872).

### SCUS static tables

| Module | Table |
|---|---|
| `item_names` | `SCUS_942.54` item-name table (`PTR_DAT_8007436C[id*3]`, 256 ids): `ItemNameTable::from_scus` → `name(id)`. The id space a monster record's `drop_item` indexes; used by the web viewer's enemy table. See [`item-table.md`](../../docs/formats/item-table.md). |
| `item_effect` | `SCUS_942.54` item-effect descriptor table (`DAT_800752C0`, 130 records): `ItemEffectTable::from_scus` → `effect(id)` (item id → subtype → `[class, tier, flags]`). Effect class/tier + all-party/field/battle usability, plus the **literal restore amounts** — `heal_amounts()` / `restore_amount(id)` decode the static heal-amount table at `0x8007655C` (HP `[200,800,9999]` / MP `[50,200,20]`) the apply handler `FUN_800402F4` reads. See [`item-effect-table.md`](../../docs/formats/item-effect-table.md). |
| `equip_stats` | `SCUS_942.54` equipment stat-bonus table (`DAT_80074F68`, 8-byte stride): `EquipStatTable::from_scus` → `bonus(id)` (equippable id → property `+1` byte → record). Attack/def-up/def-down (byte-exact vs gamedata) + equip-character mask + slot type + Ra-Seru flag. See [`equipment-table.md`](../../docs/formats/equipment-table.md). |
| `spell_names` | `SCUS_942.54` spell table (`DAT_800754C8`/`DAT_800754D0`, 256 ids): `SpellNameTable::from_scus` → `name(id)` / `mp(id)`. Resolves a monster's global magic-attack ids (`MonsterRecord::magic_attacks`, record `+0x21..=+0x23`) into the on-screen spell name (`0x27` → `Tail Fire`). See [`spell-table.md`](../../docs/formats/spell-table.md). |
| `steal_table` | `SCUS_942.54` per-monster steal table (`DAT_80077828`, 1-based monster id, 2-byte `[chance, item]`): `StealTable::from_scus` → `entry(id)` / `steal_item(id)`. What the Evil God Icon steals; the item id resolves through `item_names`. NOT in the PROT 867 record. See [`steal-table.md`](../../docs/formats/steal-table.md). |

`new_game` — `SCUS_942.54` new-game seed data:

- `StartingParty::from_scus` decodes the starting-party template (`0x80078C4C`, 4
  records, 26-byte stride) into per-member opening stats + name
  (Vahn/Noa/Gala/Terra).
- `StartingInventory::from_scus` decodes the starting-inventory seed code in
  `FUN_80034A6C` (`0x80034b04`) into `(item_id, count)` slots (vanilla = Healing
  Leaf `0x77` ×5), reading back either the original `sb` byte-stores or the
  randomizer's packed `sh` halfword stores.

Seeds for the live `0x80084708 + n*0x414` records + the `0x80085958` bag;
`OPENING_SCENE` = `town01`. See
[`new-game-table.md`](../../docs/formats/new-game-table.md).

### Cutscene / summon

`cutscene_text` — inline cutscene-narration text embedded in a field-VM
cutscene-timeline record:

- `parse_narration` / `narration_pages` decode the `0x1F`/`0x00`-framed ASCII
  subtitle pages introduced by a `0x4C` op whose operand declares the page count
  (the `opdeene` opening-prologue narration). See
  [`cutscene.md`](../../docs/subsystems/cutscene.md#inline-narration-format).

`summon_overlay` — Seru-magic **summon scene-graph** part records:

- A per-summon stager overlay (e.g. PROT 0905, Gimard *Tail Fire*) stages each
  summon body part with a `FUN_80021B04` call passing a per-part record.
- `parse(bytes, link_base)` scans those call sites and recovers the records
  (`[i16 model_sel][u16 flags][move-VM bytecode]`, `model_sel == -1` =
  transform/pivot node). Records live in-file under link base `0x801F69D8`.

CLI `asset summon-overlay <PROT 0905 .BIN>`. See
[`open-rev-eng-threads.md`](../../docs/reference/open-rev-eng-threads.md) (Seru-magic
summon visual).

### Scene + MAN editing

`man_edit` — **variable-length editing of a decompressed MAN.**

- `scene_change_sites` enumerates the field-VM `0x3F` named-scene-change ("door")
  ops via a clean partition walk (they're partition-2 records).
- `apply_dest_edits` resizes a door's inline destination name and rebuilds the
  buffer, fixing the partition record-offset tables + header section-0 offset +
  intra-record relative-jump deltas, with a `validate` re-parse backstop. Powers the
  door randomizer.

See [`man-relocation.md`](../../docs/formats/man-relocation.md). (The per-scene asset
slot table `scene_asset_table` and its `scene_v12_table` variant are in the
[simple-detectors table](#simple-detectors-table).)

### TIM/TMD scan + catalog

| Module | What it does |
|---|---|
| `tim_scan` / `tmd_scan` | Brute-force magic search inside an entry. |
| `tim_catalog` | Flat strict-validated TIM inventory over the whole `PROT.DAT` image (catches the unindexed-gap TIMs `tim_scan` can't); maps each to its owning entry + offset; reproduces an external reference decoder's TIM set item-for-item. |
| `tim_deep_catalog` | Separate tier: strict-validated TIMs recovered from inside LZS-compressed sections, keyed by `(entry, LZS section, offset-in-section)`. |
| `tim_labels` | Curated semantic labels for cataloged TIMs (raw + deep), keyed by content fingerprint: coarse visual categories + precise reverse-engineered pins for the boot/title/menu textures. Our own annotations, not asset bytes. |

## CLI

```bash
asset describe         <input>            # parse + print descriptor
asset decode           <input> <output>   # apply the dispatcher
asset categorize       <PROT.DAT> [--cdname <CDNAME.TXT>]
asset find-overlay     <PROT.DAT>         # MIPS-code candidate scan
asset tim-scan         <input>            # locate embedded TIMs (per-entry, lenient)
asset tim-catalog      <PROT.DAT>         # flat strict TIM catalog (--out f.tsv|f.json, --rollup)
asset tim-deep-catalog <PROT.DAT>         # TIMs inside LZS-compressed sections (--out, --rollup)
asset tim-render-distinct <PROT.DAT> --out <dir>  # decode each distinct TIM to PNG (local only; drives tim_labels)
asset tmd-scan         <input>            # locate embedded TMDs
asset stage / stage-scan
asset field-pack / field-pack-scan
asset effect-bundle / effect-bundle-scan
asset battle-data-pack / battle-data-pack-scan
asset extract <PROT.DAT> <out_dir>        # full per-entry extraction
asset item-tables <SCUS_942.54>           # dump item-effect + equipment tables (--equipment-only / --consumables-only)
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
