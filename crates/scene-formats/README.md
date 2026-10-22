# legaia-scene-formats

The formats a field or world-map scene is built from.

| Group | Modules |
|---|---|
| Scene bundle shapes | `scene_asset_table`, `scene_scripted_asset_table`, `scene_event_scripts`, `scene_tmd_stream`, `scene_v12_table`, `scene_vdf`, `field_pack`. See [`scene-bundles.md`](../../docs/formats/scene-bundles.md), [`scene-v12-table.md`](../../docs/formats/scene-v12-table.md). |
| MAN | `man_section` (section walker), `man_edit` (variable-length editing, [`man-relocation.md`](../../docs/formats/man-relocation.md)), `man_motion` (motion-VM script table). |
| Field-VM bytecode as data | `field_disasm`, the side-effect-free disassembler and the disc-wide opcode census ([`field-op-census.md`](../../docs/tooling/field-op-census.md)). |
| Field geometry | `field_objects`, `field_map` ([`field-map.md`](../../docs/formats/field-map.md)), `stage_geom`, `battle_backdrop`. |
| Carried inline by a scene script | `shop_stock`, `inn_costs`, `place_names` ([`place-names.md`](../../docs/formats/place-names.md)), `cutscene_text`. |
| World map | `kingdom_bundle`, `world_map_overlay` ([`world-map-overlay.md`](../../docs/formats/world-map-overlay.md)), `clut_walk`, `ocean`. |

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) and the
leaf codecs (`legaia-lzs`, `legaia-tim`, `legaia-tmd`, `legaia-bytes`). The one
sideways edge is [`legaia-overlay-images`](../overlay-images/README.md):
`scene_event_scripts` reads the summon part records an event script can
spawn. It must not depend on the UI, minigame or actor format crates,
nor on `legaia-disc-audit`.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `field_map` | Per-scene `DATA\FIELD\<scene>.MAP` - the fixed `0x12000`-byte slot 0 of every scene block. Region map + trigger-block header; detected on the trigger block's sub-table chain, not on the footprint (111 entries share it, 101 are maps). See [`docs/formats/field-map.md`](../../docs/formats/field-map.md). |
| `field_pack` | **Not a format** - kept as the reader for the scene texture packs at raw-TOC `+4` of a CDNAME block (`scene_pack` / `ScenePack`). The `0x01059B84` word is town01's DATA_FIELD `TIM_LIST` chunk header, `(type << 24) \| payload_len`, over an ordinary `container-formats::pack`; chunk-headered carriers classify `data_field_streaming`, bare ones `pack`. See [`docs/formats/field-pack.md`](../../docs/formats/field-pack.md). |
| `stage_geom` | Stage geometry: 12-byte prefix + 8-byte u16 quad records. |
| `scene_tmd_stream` | `[u32 chunk0][bare TMD][streaming chunks]`. `sub_streams` enumerates the concatenated, `0x800`-aligned `[TMD][TIM chunks][terminator]` blocks (the entry holds N, not one continuation list). |
| `scene_asset_table` | Per-scene asset slot table (CDNAME block layout). `resolve` / `slots` / `payload_range` walk the positional slot->payload mapping (the descriptor's `data_offset` IS the indirection - no separate table), unifying the bare and prescript-prefixed variants. Plus `SceneAssetTable::size_word_offset` / `encode_size_word` for rewriting a descriptor's decompressed-size word after a variable-length asset edit. |
| `scene_v12_table` | Variant of the per-scene table. |
| `vdf_morph` | One VDF sub-entry's morph records and the stager over them: record walk, the `FUN_8005B038` weighted-delta blend, the `FUN_8001C604` rest-pose copy + per-group apply. Pure byte kernels; the actor's lanes and weights are `legaia_engine_vm::vdf_morph`. |
| `scene_vdf` | Scene bundle type-7 slot: the vertex-morph delta pack (`[u32 count][u32 offsets]` + morph-record sub-entries) the morph stager `FUN_8001C604` consumes. 61 bundles populated. See [`docs/subsystems/field-ambient-fx.md`](../../docs/subsystems/field-ambient-fx.md). |
| `shop_stock` | Town gold-shop stock records inside a scene MAN (field-VM op `0x49` sub-op `0` = `[count][item_ids][name]`). `scan` byte-scans a decompressed MAN; `locate` decompresses a bundle entry's MAN and returns its [`ShopRecord`]s. Shared read side for the randomizer (`legaia_patcher::shop`) and the engine shop catalog (`legaia_engine_menus::shop_catalog`). |
| `inn_costs` | Scripted gold charges (inn stays, tours, rides, casino coin buys) inside a scene MAN: op `0x4E` gold-gate (sub-3 u16 / sub-10 u32 literal vs `_DAT_8008459C`) paired with a negative op `0x3A` `ADD_MONEY` debit. `scan` byte-scans a decompressed MAN; `locate` decompresses a bundle entry's MAN and returns its [`GoldCharge`]s. Retail has no inn cost table - the costs are these script literals (`docs/subsystems/inn.md`). |
| `scene_scripted_asset_table` | Composite shape pairing a `[u16 count][u16 offsets[count]]` prescript with a canonical 7-asset table at the next sector boundary. |
| `scene_event_scripts` | The `[u16 count][u16 offsets]` prescript entry a scene block seats at slot 2. Two tiers: `detect` (shape + frame-opener rate) and `detect_structural` (shape only). Records are move-VM stager records, NOT field-VM bytecode. |
| `field_objects` | Per-scene static-object placement table (terrain segments / buildings / props in world space). |
| `kingdom_bundle` | Opens a kingdom PROT entry (`map01`/`map02`/`map03`) and decodes one slot of its 7-asset table. CLI `asset kingdom-slot`. |
| `world_map_overlay` | Slot-4 container: the world-map scene's type-`0x05` ANM animation bank - per-clip 8-byte rigid-transform entries (`Slot4Transform`), decoded per part and frame. CLI `asset slot4-png` renders translation paths as a top-down PNG (a byte-inspection aid). See [`world-map-overlay.md`](../../docs/formats/world-map-overlay.md). |
| `ocean` | Ocean tile texture (4bpp 64×256) + its 13-frame CLUT animation from the kingdom bundles. |
| `clut_walk` | The type-6 CLUT-walk `MoveImage` walker table + its parked source strips. Not kingdom-only: `from_scene_bundle` / `scene_park_strips` resolve any bundle's slot by type byte (12 carriers, incl. 9 water/waterfall field scenes - [`field-ambient-fx.md`](../../docs/subsystems/field-ambient-fx.md)). |
| `place_names` | The two **MAN-resident** place-name carriers `worldmap_menu` is not: section 2 = the scene's display name (the on-entry banner, `_DAT_801C6EA0`), and the section-5 trailer = the 29-record world-map location table the label pass walks (`DAT_80073EE0`, region + map x/y + discovery flag + name). See [`place-names.md`](../../docs/formats/place-names.md). |
| `man_motion` | The motion-VM (`FUN_80038158`) script table, MAN tail-section 1. |
| `encounter_record` | The encounter record `FUN_801DA51C` reads at `actor[+0x94]` (`[3 reserved][count][monster_ids]`): the trailing operand bytes of the field-VM install op. See [`encounter.md`](../../docs/formats/encounter.md). |

### Field-VM disassembler (`field_disasm`)

Side-effect-free disassembler for the field/event-VM bytecode (the opcode
stream `FUN_801DE840` executes): the per-opcode width/format decoder plus
`LinearWalker`. Lives here (next to the MAN/scene parsers that carry the
bytecode) so disc tooling can walk scripts without the engine; the executing
VM is `legaia-engine-vm`. CLI: `asset field-disasm
file|scene-event-scripts|scan-prot`; `asset field-op-census` is the disc-wide
opcode census built on it ([`docs/tooling/field-op-census.md`](../../docs/tooling/field-op-census.md)).

### `battle_backdrop`

How retail *places* a `scene_tmd_stream` battle-stage shell, which is not
"every object, once". `FUN_800513F0` registers the TMD once and links **two**
background actors to it, drops object **1** from both draw lists, and gives the
second actor a per-stage transform that closes the authored half.

- `MirrorXTable::from_scus` parses the zero-terminated `u16` stage table at
  `DAT_80078B50` (`SCUS_942.54` file `0x69350`);
  `second_copy_for_prot_index` resolves an entry to `SecondCopy::HalfTurn`
  (`actor+0x26 = 0x800`) or `SecondCopy::MirrorX` (`actor+0x5A = 2`).
- `SecondCopy::scale` / `flips_winding` give the transform as an exact integer
  diagonal plus its determinant sign - pair with
  `legaia_tmd::mesh::VramMesh::append_scaled`.
- `drawn_object_indices` / `drawn_objects_tmd` apply the object-1 drop;
  `runtime_stage_id` converts between the table's id space and PROT
  extraction indices.

Corpus sweep in `crates/disc-audit/tests/battle_backdrop_real.rs`. See
[`battle.md`](../../docs/subsystems/battle.md#backdrop-shell---two-copies-of-one-mesh).

### `cutscene_text`

`cutscene_text` - inline cutscene-narration text embedded in a field-VM
cutscene-timeline record:

- `parse_narration` / `narration_pages` decode the `0x1F`/`0x00`-framed ASCII
  subtitle pages introduced by a `0x4C` op whose operand declares the page count
  (the `opdeene` opening-prologue narration). See
  [`cutscene.md`](../../docs/subsystems/cutscene.md#inline-narration-format).

### `man_section`

`man_section` - the per-scene MAN (asset type `0x03`) **multi-section header
walker** (`PORT: FUN_8003AEB0 / FUN_8003A1E4 / FUN_8003A110`): partitions,
per-section offset+length refs, the encounter section, and the world-map
bulk-terrain flag. CLI `asset man` / `man-scan`. The engine's
`encounter_table_from_man` builds on it.

The encounter section's region array is **grouped, not flat**: the condition
array partitions it into consecutive story-flag-gated slices and the retail
roll searches exactly one of them. `active_region_group` runs that walk for a
given flag state and `region_records_in` reads the slice; `region_records`
stays the whole-corpus view. Reading the array flat lands on group 0, which in
most scenes is a placeholder variant - see
[`docs/formats/encounter.md`](../../docs/formats/encounter.md#the-condition-array-story-flag-gated-region-groups).

### `man_edit`

`man_edit` - **variable-length editing of a decompressed MAN.**

- `scene_change_sites` enumerates the field-VM `0x3F` named-scene-change ("door")
  ops via a clean partition walk (they're partition-2 records); `site_p2_slots`
  joins each site to the partition-2 slot a `.MAP` kind-1 walk-on tile names,
  `scene_destinations` is the full (P1 and P2) destination set, and
  `move_to_sites` the `0x23 MOVE_TO` sites.
- `apply_dest_edits` resizes a door's inline destination name and rebuilds the
  buffer, fixing the partition record-offset tables + header section-0 offset +
  intra-record relative-jump deltas, with a `validate` re-parse backstop. Powers the
  door randomizer.
- `apply_text_edits` resizes dialog text inside record script bodies (refusing
  records with an absolute-reference op), checked by
  `text_edits_preserve_scripts`; `apply_insertions` inserts bytes at
  instruction boundaries. The first powers the language-pack dialog import,
  the second the starting-bag and Delilas Challenge script rewrites.

See [`man-relocation.md`](../../docs/formats/man-relocation.md). The per-scene asset
slot table `scene_asset_table` and its `scene_v12_table` variant are in the
table above.
