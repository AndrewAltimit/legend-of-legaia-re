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
- [Field-VM disassembler (`field_disasm`)](#field-vm-disassembler-field_disasm)
- [Structural detectors (for `categorize`)](#structural-detectors-for-categorize)
  - [Simple detectors (table)](#simple-detectors-table)
  - [`static_overlay`](#static_overlay)
  - [`monster_archive`](#monster_archive)
  - [`move_power`](#move_power)
  - [`element_affinity`](#element_affinity)
  - [`befect_cluster`](#befect_cluster)
  - [Character meshes, textures, animation](#character-meshes-textures-animation) — `character_pack`, `battle_char_pack`, `battle_char_palette`, `field_char_textures`, `player_anm`
  - [World map](#world-map) — `kingdom_bundle`, `world_map_overlay`, `ocean`, `worldmap_menu`
  - [Boot / title / menu UI](#boot--title--menu-ui) — `init_pak`, `title_pak`, `menu_glyph_atlas`
  - [SCUS static tables](#scus-static-tables) — `item_names`, `item_effect`, `equip_stats`, `accessory_passive`, `spell_names`, `steal_table`, `sfx_table`, `level_up_tables`, `mode_table`, `new_game`
  - [Cutscene / FMV / summon](#cutscene--fmv--summon) — `cutscene_text`, `str_fmv_table`, `fmv_dispatch`, `summon_overlay`, `summon_readef`
  - [Scene + MAN](#scene--man) — `man_section`, `man_edit`, scene tables
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

## Field-VM disassembler (`field_disasm`)

Side-effect-free disassembler for the field/event-VM bytecode (the opcode
stream `FUN_801DE840` executes): the per-opcode width/format decoder plus
`LinearWalker`. Lives here (next to the MAN/scene parsers that carry the
bytecode) so disc tooling can walk scripts without the engine; the executing
VM re-exports it as `legaia_engine_vm::field_disasm`.

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
| `battle_data_pack` | Player battle files (`PLAYER1..4`, extraction 863..866 = retail `battle_data` block): header + 12-byte descriptor table + per-slot LZS streams of `[header + TMD + texture pool]`. |
| `stage_geom` | Stage geometry: 12-byte prefix + 8-byte u16 quad records. |
| `scene_tmd_stream` | `[u32 chunk0][bare TMD][streaming chunks]`. `sub_streams` enumerates the concatenated, `0x800`-aligned `[TMD][TIM chunks][terminator]` blocks (the entry holds N, not one continuation list). |
| `scene_vab_stream` | `[u32 chunk0][VABp ...]`. |
| `sound_pack` | Per-scene `.dpk` / `sound_data2`: a VAB + SEQ bundle in the type-2-terminated streaming container (chunk 0 = VAB header, chunk 1 = VAB sample pool, chunk 2 = SEQ). `extract` reconstitutes the contiguous VAB + slices the SEQ. |
| `scene_asset_table` | Per-scene asset slot table (CDNAME block layout). `resolve` / `slots` / `payload_range` walk the positional slot->payload mapping (the descriptor's `data_offset` IS the indirection - no separate table), unifying the bare and prescript-prefixed variants. Plus `SceneAssetTable::size_word_offset` / `encode_size_word` for rewriting a descriptor's decompressed-size word after a variable-length asset edit. |
| `scene_v12_table` | Variant of the per-scene table. |
| `shop_stock` | Town gold-shop stock records inside a scene MAN (field-VM op `0x49` sub-op `0` = `[count][item_ids][name]`). `scan` byte-scans a decompressed MAN; `locate` decompresses a bundle entry's MAN and returns its [`ShopRecord`]s. Shared read side for the randomizer (`legaia_rando::shop`) and the engine shop catalog (`legaia_engine_core::shop_catalog`). |
| `scene_scripted_asset_table` | Composite shape pairing a `[u16 count][u16 offsets[count]]` prescript with a canonical 7-asset table at the next sector boundary. |
| `scene_event_scripts` | Sister detector: the prescript exists but no asset table follows. (The records are word-aligned actor/event commands, NOT field-VM bytecode.) |
| `data_field_truncated` | Sister of `parse_streaming`: leading chunks decode cleanly but the last chunk's declared size walks past EOF. |
| `tmd_size_prefix` | Sister of `scene_tmd_stream`: `[u32 prefix][TMD]` with no trailing stream. |
| `anm_detect` | On-disc ANM (asset type 0x06) shape check wrapping `legaia_anm::parse`. |
| `vab_multi_bank` | Multi-bank VAB archive: `[u32 reserved][u32 count][u32 sector_nums[N]]` (PROT 0889-0891). |
| `field_objects` | Per-scene static-object placement table (terrain segments / buildings / props in world space). |

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

Footprint-bounded extraction of the four-entry window the CDNAME symbol
`befect_data` resolves to in define-number space (extraction PROT 872..875 —
retail-semantically `vdf.dat` / `efect.dat` / the `player_data` file
`player.lzs` / a `sound_data2` VAB stream; the retail befect block proper is
extraction 870..873, see `docs/formats/cdname.md`). The naive per-entry
extractor over-reads these entries (they overlap on disc), so
`extract(archive, cdname)` footprint-bounds each one, expands the
LZS-container entry into its sections, and classifies each part (the
`efect.dat` 2-pack / the field-character TMD pack / the field-character
texture TIMs / packs).

CLI `asset befect-cluster PROT.DAT --cdname CDNAME.TXT --out DIR`. See
[`effect.md`](../../docs/formats/effect.md#battle-effect-cluster-befect_data).

### Character meshes, textures, animation

| Module | What it parses |
|---|---|
| `character_pack` | Field-form player-character mesh pack (PROT 0874 §0, 5 slots: Vahn/Noa/Gala + 2 auxiliary), incl. the `FUN_8001EBEC` equipment-swap pose patch (`equipment_swap::apply`). CLI `asset character-pack`. |
| `battle_char_pack` | The PROT 1204 `other5` mesh pack (five `TMD2` streaming chunks + seven 256×256 4bpp atlases at `0x8224` stride): the Baka Fighter / default-equipment sibling of the assembled battle meshes. CLI `asset battle-char-pack`. |
| `battle_char_assembly` | Battle character-mesh assembler: selects a player file's five equipment sections by equipped item ids and splices them into the merged battle TMD (bone tags + attach bones; `PORT: FUN_80052770` case 4 / `FUN_800536BC` / `FUN_80053898`), plus `relocate_tsb_cba` - the registration-time per-slot TSB/CBA rewrite into the runtime VRAM band (`PORT: FUN_80053a28`; texpages `x in [512, 896), y = 256`, CLUT row `481 + slot`). Assembly + relocation reproduces the live runtime blob. |
| `battle_char_assembly` (battle animations) | The character's battle animations from `record[0]` of the same file (`battle_animations` / `idle_battle_animation`: action-offset table at the record head, monster-format `[parts][frames][9-byte TRS]` stream at entry `+0xAC`, `parts` = skeleton bones, entry first byte = action tag with tags `2..5`/`0xB` the hit-reaction family, rate byte at `+0x78` - the in-battle pose source for the assembled mesh, NOT PROT 1203), plus the per-object pose-channel map (`anm_bones` + `expand_animation_for_objects`; equipment extras ride their attach bone). |
| `battle_char_assembly` (swing + art animations) | The runtime action table's equipment half: `swing_battle_animations` decodes the per-equipped-item weapon-swing records (section payload `+0x04`/`+0x08`, runtime slots `0xC..0xF`; splice `PORT: FUN_80052FA0`, record shape `FUN_800557B8`), and `art_animation_bank` / `art_animation` the record[0] `+0x58` art-anim bank (`[u32 count]` + `0xD0`-stride matcher+entry records; dynamic slots `0x10`/`0x11` via `FUN_8004AD80`), resolving each record's keyframe stream through its `readef.DAT` `"ME"` archive (`art_me_archive`). |
| `me_archive` | `"ME"` keyframe-stream archive (`PORT: FUN_8002B28C` walk + `FUN_8002A9CC` channel-delta codec): `['M']['E'][u8 count][u16 sizes (bit 15 = compressed)][bodies]` → packed `[parts][frames][9-byte TRS]` streams. The art-animation stream source in `readef.DAT` slots `3*char+1` / `3*char+2`. |
| `face_anim` | Battle facial animation (`PORT: FUN_8004C7B4`): the action entries' eye (`+0x8C`) / mouth (`+0x98`) keyframe tracks (`FaceTracks` / `battle_face_tracks`), the static `SCUS_942.54` face-frame tables (`FaceFrameTables::from_scus`, `DAT_80076824..0x80076908`) and the per-frame stamp selection (`FaceFrameTables::stamps` → `MoveImage` rects the engine applies via `legaia_tim::Vram::move_image`). See [`battle-data-pack.md` § Facial animation tracks](../../docs/formats/battle-data-pack.md#facial-animation-tracks-entry-0x8c--0x98). |
| `battle_char_palette` | In-battle party CLUTs decoded from the per-character player files (extraction PROT 0863/0864/0865 = `PLAYER1..3`) — `PORT: FUN_80052FA0`. The PROT 1204 bundled CLUTs are authoring defaults, not the battle palettes. |
| `field_char_textures` | Field-character texture pack (PROT 0874 §2, "etim.dat"): eight TIM entries; 1/2/3 are the Vahn/Noa/Gala field atlas pages (texpage `(832,256)`, CLUT row 478). CLI `asset field-char-tex`. |
| `player_anm` | Per-scene player ANM bundle (each scene bundle's type-0x05 "MOVE" section; battle-form at PROT 1203): per-(bone,frame) 8-byte entries, frame 0 of idle = rest pose. CLI `asset player-anm` / `player-anm-scan`. |

See [`character-mesh.md`](../../docs/formats/character-mesh.md) and
[`anm.md`](../../docs/formats/anm.md).

### World map

| Module | What it parses |
|---|---|
| `kingdom_bundle` | Opens a kingdom PROT entry (`map01`/`map02`/`map03`) and decodes one slot of its 7-asset table. CLI `asset kingdom-slot`. |
| `world_map_overlay` | Slot-4 container: per-body object-local GTE vertex pools (`(i16 x,y,z,attr)` records, read in place by the renderer). CLI `asset slot4-png` renders a top-down wireframe PNG. See [`world-map-overlay.md`](../../docs/formats/world-map-overlay.md). |
| `ocean` | Ocean tile texture (4bpp 64×256) + its 13-frame CLUT animation from the kingdom bundles. |
| `worldmap_menu` | The quick-travel landmark menu out of `SCUS_942.54`: 16-entry name table (`DAT_80073B18`) + 6-byte placement records (`DAT_80073A98`). CLI `asset worldmap-menu` (`--json` = the web-viewer shape). |

### Boot / title / menu UI

| Module | What it parses |
|---|---|
| `init_pak` | The four publisher-logo TIMs from PROT 0895 (CDNAME says `bat_back_dat`; actually init.pak). |
| `title_pak` | The "Legend of Legaia" title-screen TIM + the system-UI sheet (load-screen panel / slot pills). |
| `menu_glyph_atlas` | The small-caps menu font atlas (title menu rows + shared menu UI). |

### SCUS static tables

| Module | Table |
|---|---|
| `sfx_table` | Sound-effect descriptor table (`DAT_8006F198`, 100 × 8-byte): `SfxTable::from_scus` → per-cue program/VAG, ADSR-region base, voice count + sustained bit, mixer channel. Feeds `SfxBank::from_descriptors`. See [`sfx-table.md`](../../docs/formats/sfx-table.md). |
| `level_up_tables` | Level-up data: `xp_thresholds_from_scus` (the 98-entry XP increment table) + `xp_correction_divisors_from_scus` (the per-level slots-1/2 threshold-correction divisors at `0x80070A2C`) + `growth_tables_from_scus` (the `DAT_80076918` per-character 8-stat growth curves). |
| `item_names` | `SCUS_942.54` item-name table (`PTR_DAT_8007436C[id*3]`, 256 ids): `ItemNameTable::from_scus` → `name(id)`. The id space a monster record's `drop_item` indexes; used by the web viewer's enemy table. See [`item-table.md`](../../docs/formats/item-table.md). |
| `item_effect` | `SCUS_942.54` item-effect descriptor table (`DAT_800752C0`, 130 records): `ItemEffectTable::from_scus` → `effect(id)` (item id → subtype → `[class, tier, flags]`). Effect class/tier + all-party/field/battle usability, plus the **literal restore amounts** — `heal_amounts()` / `restore_amount(id)` decode the static heal-amount table at `0x8007655C` (HP `[200,800,9999]` / MP `[50,200,20]`) the apply handler `FUN_800402F4` reads — and the **stat-up / buff taxonomy** — `stat_effect(id)` → `StatItemEffect` for the permanent stat-up *Water* line (class 6), the one-battle `×6/5` buff Elixirs (class 7), and Fury Boost (class 5). See [`item-effect-table.md`](../../docs/formats/item-effect-table.md). |
| `equip_stats` | `SCUS_942.54` equipment stat-bonus table (`DAT_80074F68`, 8-byte stride): `EquipStatTable::from_scus` → `bonus(id)` (equippable id → property `+1` byte → record). Attack/def-up/def-down (byte-exact vs gamedata) + equip-character mask + slot type + Ra-Seru flag. See [`equipment-table.md`](../../docs/formats/equipment-table.md). |
| `accessory_passive` | Accessory ("Goods") passive effects: `AccessoryPassiveTable::from_scus` → `passive(id)` (item id → descriptor `+3` / equip `+5` index byte → 64-slot passive index + the `0x8007625C` name/description/scope record). `stat_boosts(index)` mirrors the `FUN_80042558` percent arithmetic, `bit_location(index)` the `char+0xF4` ability-bitfield placement. Byte-validated vs the curated gamedata accessory table. See [`accessory-passive-table.md`](../../docs/formats/accessory-passive-table.md). |
| `spell_names` | `SCUS_942.54` spell table (`DAT_800754C8`/`DAT_800754D0`, 256 ids): `SpellNameTable::from_scus` → `name(id)` / `mp(id)`. Resolves a monster's global magic-attack ids (`MonsterRecord::magic_attacks`, record `+0x21..=+0x23`) into the on-screen spell name (`0x27` → `Tail Fire`). See [`spell-table.md`](../../docs/formats/spell-table.md). |
| `steal_table` | `SCUS_942.54` per-monster steal table (`DAT_80077828`, 1-based monster id, 2-byte `[chance, item]`): `StealTable::from_scus` → `entry(id)` / `steal_item(id)`. What the Evil God Icon steals; the item id resolves through `item_names`. NOT in the PROT 867 record. See [`steal-table.md`](../../docs/formats/steal-table.md). |
| `mode_table` | `SCUS_942.54` game-mode dispatch table (`0x8007078C`, 28 × 24-byte entries): `ModeTable::from_scus` → per-mode handler fn ptr / param / dev name. Recovers the index → retail-handler map from the disc (12 of 14 per-frame modes share `0x80025EEC`; field/town = 2/3 MAIN; world-map = 12/13 MAPDISP). CLI `asset mode-table`. See [`boot.md`](../../docs/subsystems/boot.md#game-mode-state-machine). |

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

### Cutscene / FMV / summon

`cutscene_text` — inline cutscene-narration text embedded in a field-VM
cutscene-timeline record:

- `parse_narration` / `narration_pages` decode the `0x1F`/`0x00`-framed ASCII
  subtitle pages introduced by a `0x4C` op whose operand declares the page count
  (the `opdeene` opening-prologue narration). See
  [`cutscene.md`](../../docs/subsystems/cutscene.md#inline-narration-format).

`str_fmv_table` — the compact in-RAM STR FMV file table (`0x801CAE40`,
24-byte stride × 6: name + libcd BCD MSF + size) the cutscene overlay uses to
resolve a movie without an ISO9660 walk. See
[`str-fmv-table.md`](../../docs/formats/str-fmv-table.md).

`fmv_dispatch` — the per-`fmv_id` movie + frame-range dispatch the STR/MDEC
overlay's play loop selects from, decoded straight from the overlay bytes.

`summon_overlay` — Seru-magic **summon scene-graph** part records:

- A per-summon stager overlay (player extraction PROT 0903..=0913 — Gimard
  *Tail Fire* `0x81` arithmetics to 0903 under the corrected loader index math
  — the evolved-Seru block `EVOLVED_SUMMON_STAGER_PROT` (0914..=0923,
  `spell_id 0x8C..=0x95`, the same arithmetic run), high-summon 0927..=0934, and
  the six Cort enemy boss stagers `ENEMY_BOSS_STAGER_PROT`) stages each summon
  body part with a `FUN_80021B04` call passing a per-part record — directly or
  through the `FUN_80050ED4` pool wrapper (both scanned).
- `parse(bytes, link_base)` scans those call sites and recovers the records
  (`[i16 model_sel][u16 flags][move-VM bytecode]`, `model_sel == -1` =
  transform/pivot node, `0x4000`/`0x4001` = render-mode nodes). Records live
  in-file under link base `0x801F69D8`. Trim the entry to its TOC-gap
  unique-content footprint first (`unique_content_len`) — stager extraction
  files over-read into the following entries.

CLI `asset summon-overlay <stager .BIN> [--trim 0xNNNN]`. See
[`open-rev-eng-threads.md`](../../docs/reference/open-rev-eng-threads.md) (Seru-magic
summon visual).

`summon_readef` — the battle side-band streaming files `summon.dat` /
`readef.DAT` (extraction PROT 893 / 894 = retail TOC `0x37F` / `0x380`,
CDNAME block `bat_back_dat`): `0x10800`-byte slots carrying per-special-attack
CLUT rows + 4bpp texture pages plus summon-creature actor records (name + Legaia
TMD + texture pool) and the player art-animation `"ME"` stream archives
(readef slots `3*char+1` / `3*char+2` → `SlotKind::MeArchive`). `parse`
classifies every slot; `stream_target(action_id)` mirrors the retail
id → (file, slot) formula (`FUN_801E295C` case `0x32`). See
[`summon-readef.md`](../../docs/formats/summon-readef.md).

### Scene + MAN

`man_section` — the per-scene MAN (asset type `0x03`) **multi-section header
walker** (`PORT: FUN_8003AEB0 / FUN_8003A1E4 / FUN_8003A110`): partitions,
per-section offset+length refs, the encounter section, and the world-map
bulk-terrain flag. CLI `asset man` / `man-scan`. The engine's
`encounter_table_from_man` builds on it.

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
asset overlay          list|extract|verify|ghidra|scan|find-sig|generate   # static overlay pipeline
asset tim-scan         <input>            # locate embedded TIMs (per-entry, lenient)
asset tim-catalog      <PROT.DAT>         # flat strict TIM catalog (--out f.tsv|f.json, --rollup)
asset tim-deep-catalog <PROT.DAT>         # TIMs inside LZS-compressed sections (--out, --rollup)
asset tim-render-distinct <PROT.DAT> --out <dir>  # decode each distinct TIM to PNG (local only; drives tim_labels)
asset tmd-scan         <input>            # locate embedded TMDs
asset clut-finder                         # which entry's TIM supplies a VRAM CLUT cell
asset stage / stage-scan
asset field-pack / field-pack-scan
asset effect-bundle / effect-bundle-scan
asset battle-data-pack / battle-data-pack-scan
asset befect-cluster   <PROT.DAT> --cdname <CDNAME.TXT> --out <dir>
asset monster-archive  [--id N --obj/--texture-png/--anim/--glb]
asset character-pack / battle-char-pack / field-char-tex
asset player-anm / player-anm-scan
asset scene-v12 / scene-v12-scan
asset man / man-scan                      # MAN multi-section walker (--with-encounter)
asset kingdom-slot / slot4-png            # world-map kingdom bundles
asset summon-overlay   <PROT 0905 .BIN>
asset move-power / element-affinity       # PROT 0898 battle-overlay tables
asset mode-table / worldmap-menu / item-tables   # SCUS_942.54 static tables
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
