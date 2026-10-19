# legaia-actor-formats

What an actor is built from on the disc - the party in its field and battle
forms, the monsters, the summon creatures - plus the animation that poses
them and their glTF export.

- **Monsters** - `monster_archive` (the PROT 867 monster archive: records,
  meshes, animations) and `monster_model` (the modder-facing OBJ+PNG
  export / import surface).
- **Party** - `battle_data_pack` (the player battle files `PLAYER1..4`),
  `battle_char_assembly` (the per-character equipment assembly, battle
  animations, swing / art animations, the equipment cuts and the loadout
  kernel), `battle_char_pack` (the `other5` battle-form mesh pack),
  `battle_char_palette` (the in-battle party CLUTs), `face_anim` (battle
  facial animation) and `me_archive` (the `"ME"` keyframe-stream archive).
- **Field form** - `character_pack` (the PROT 0874 section-0 field mesh
  pack), `player_anm` (the party locomotion and per-scene NPC animation
  bundles), `field_char_textures` (PROT 0874 section 2) and `befect_cluster`
  (the `befect_data` cluster those sections sit in). `player_file_annex`
  reads a player battle file whose slot region is annexed outside PROT.
- **Side-band** - `summon_readef` (the `summon.dat` / `readef.DAT` streaming
  slots) and `summon_creatures` (the Seru-magic summon to `battle_data`
  creature map).
- **Textures + export** - `battle_texture_catalog` (the headerless 4bpp
  battle texture blocks), `mesh_raster` (a software rasteriser for posed
  meshes), `gltf_color`, `monster_gltf`, `scene_gltf` and `character_gltf`
  (the `.glb` builders).

The equipment-isolation override table `equip_isolate` compiles in sits at
[`data/equip-isolation.toml`](data/equip-isolation.toml). The format pages
[`battle-data-pack.md`](../../docs/formats/battle-data-pack.md),
[`character-mesh.md`](../../docs/formats/character-mesh.md),
[`monster-animation.md`](../../docs/formats/monster-animation.md) and
[`summon-readef.md`](../../docs/formats/summon-readef.md) carry the layouts.

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) (the
descriptor decode and pack the field pack is read through) and
[`legaia-game-tables`](../game-tables/README.md). It must not depend on the
scene, UI or minigame format crates, nor on `legaia-disc-audit`.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `battle_data_pack` | Player battle files (`PLAYER1..4`, extraction 863..866 = retail `battle_data` block): header + 12-byte descriptor table + per-slot LZS streams of `[header + TMD + texture pool]`. |
| `character_pack` | Field-form player-character mesh pack (PROT 0874 §0, 5 slots: Vahn/Noa/Gala + savepoint + aux), incl. the `FUN_8001EBEC` equipment-swap pose patch (`equipment_swap::apply`) and the §1 **party locomotion ANM bundle** accessor (`field_locomotion_anm` + the `LOCOMOTION_*` bank constants: three 7-record character banks, idle = bank slot 1, walk = slot 0, both capture-pinned; frame 0 of idle = the field rest pose). CLI `asset character-pack`. |
| `battle_char_pack` | The `other5` battle-form mesh pack, **two entries**: PROT 1204's five `TMD2` streaming chunks and PROT 1205's eight 256×256 4bpp atlases at `0x8224` stride. The Baka Fighter / default-equipment sibling of the assembled battle meshes. CLI `asset battle-char-pack ... --atlas-entry`. |
| `battle_char_assembly` | Battle character-mesh assembler: selects a player file's five equipment sections by equipped item ids and splices them into the merged battle TMD (bone tags + attach bones; `PORT: FUN_80052770` case 4 / `FUN_800536BC` / `FUN_80053898`), plus `relocate_tsb_cba` - the registration-time per-slot TSB/CBA rewrite into the runtime VRAM band (`PORT: FUN_80053a28`; texpages `x in [512, 896), y = 256`, CLUT row `481 + slot`; texture block at the `DAT_80078C38` word offset). Assembly + relocation reproduces the live runtime blob. `character_texture_uploads` adds one port-side step: `stripped_pool_donor` gives Noa's pool-stripped Force Blade record its Short Sword pool. |
| `battle_char_assembly` (battle animations) | The character's battle animations from `record[0]` of the same file (`battle_animations` / `idle_battle_animation`: action-offset table at the record head, monster-format `[parts][frames][9-byte TRS]` stream at entry `+0xAC`, `parts` = skeleton bones, entry first byte = action tag with tags `2..5`/`0xB` the hit-reaction family, rate byte at `+0x78` - the in-battle pose source for the assembled mesh, NOT PROT 1203), plus the per-object pose-channel map (`anm_bones` + `expand_animation_for_objects`; equipment extras ride their attach bone). |
| `battle_char_assembly` (swing + art animations) | The runtime action table's equipment half: `swing_battle_animations` decodes the per-equipped-item weapon-swing records (section payload `+0x04`/`+0x08`, runtime slots `0xC..0xF`; splice `PORT: FUN_80052FA0`, record shape `FUN_800557B8`), `swing_command_costs` reduces them to the four `+0x74` AP prices the Arts input and the Muscle Dome both charge per command, and `art_animation_bank` / `art_animation` the record[0] `+0x58` art-anim bank (`[u32 count]` + `0xD0`-stride matcher+entry records; dynamic slots `0x10`/`0x11` via `FUN_8004AD80`), resolving each record's keyframe stream through its `readef.DAT` `"ME"` archive (`art_me_archive`). |
| `battle_char_assembly` (equipment cuts) | `equip_diff` (presentation diff of one loadout against the all-defaults assembly), `equip_item` (the exact palette cut of a held item out of its bone object, graded `own-object` / `separate` / `welded` / `fused`), `equip_isolate` (the opinionated **item-alone** cut: colour diff against the bare limb for held items and headgear, identity for body and footwear, plus the per-record override table `battle-models/data/equip-isolation.toml`; `item_mesh` hands the cut back as object-local geometry), and `equip_repair` (the **grip bridge**: pairs the open rims a welded weapon leaves where the fist hid the shaft - same object, facing, coaxial - and lofts a tube between them; never caps a lone rim). See `docs/formats/battle-data-pack.md`. |
| `battle_char_assembly::equip_skin_mask` | The item-alone cut's texel-level half: skin a kept primitive paints beside the item (forehead under a clip, shoulders above a neckline) is made transparent in a masked 15-bit copy of the primitive's texels, and the primitive is re-pointed at it. |
| `battle_char_assembly::loadout` | The shared **loadout kernel** over all of the above: assemble one character at one equipped-id set (mesh minus `200+` byte-copy duplicates, VRAM band, labelled action/swing/art clip bank, per-item cuts; `decode_art_clips` names the arts through `legaia_art::clip_ladder`) plus the two per-item `.glb` bakers (`item_only_glb` grip-repaired item-alone, `item_glb` exact cut with host limb) and the section catalog/labels. One implementation behind the site's characters-page equipment viewer and `legaia-engine export-glb --items`. |
| `mesh_raster` | Software rasteriser for a posed `VramMesh` (textured through PSX VRAM, depth-buffered, framed to fit, optional card framing: sticks stood on their long axis, compact pieces as worn) - the site's per-item card thumbnails and the disc-gated contact sheets, where a GPU context is unavailable or unwanted. |
| `me_archive` | `"ME"` keyframe-stream archive (`PORT: FUN_8002B28C` walk + `FUN_8002A9CC` channel-delta codec): `['M']['E'][u8 count][u16 sizes (bit 15 = compressed)][bodies]` → packed `[parts][frames][9-byte TRS]` streams. The art-animation stream source in `readef.DAT` slots `3*char+1` / `3*char+2`. Sibling container of the raw inline record[0] streams - same pose format, different encoding ([`battle-data-pack.md`](../../docs/formats/battle-data-pack.md#two-container-encodings-one-pose-format)). |
| `face_anim` | Battle facial animation (`PORT: FUN_8004C7B4`): the action entries' eye (`+0x8C`) / mouth (`+0x98`) keyframe tracks (`FaceTracks` / `battle_face_tracks`), the static `SCUS_942.54` face-frame tables (`FaceFrameTables::from_scus`, `DAT_80076824..0x80076908`) and the per-frame stamp selection (`FaceFrameTables::stamps` → `MoveImage` rects the engine applies via `legaia_tim::Vram::move_image`). See [`battle-data-pack.md` § Facial animation tracks](../../docs/formats/battle-data-pack.md#facial-animation-tracks-entry-0x8c--0x98). |
| `battle_char_palette` | In-battle party CLUTs decoded from the per-character player files (extraction PROT 0863/0864/0865 = `PLAYER1..3`) - `PORT: FUN_80052FA0`. The PROT 1204 bundled CLUTs are authoring defaults, not the battle palettes. |
| `field_char_textures` | Field-character texture pack (PROT 0874 §2, "etim.dat"): eight TIM entries; 1/2/3 are the Vahn/Noa/Gala field atlas pages (texpage `(832,256)`, CLUT row 478). CLI `asset field-char-tex`. |
| `player_anm` | Frame-stream ANM container parser (per-(bone,frame) 8-byte entries; frame 0 of idle = rest pose). Decodes the per-scene **NPC / scene-actor** bundles (each scene bundle's type-0x05 "MOVE" section; an NPC's record = its MAN placement `anim_id - 1`), the PROT 0874 §1 party locomotion bundle, and the battle-form banks at PROT 1203. CLI `asset player-anm` / `player-anm-scan`. |
| `battle_texture_catalog` | A third texture tier keyed by `(entry, record, section, pool offset)`: the headerless 4bpp party-character art in the player battle files (PROT 863..866). Not TIMs - no magic, no header, geometry from the loader's rect table - so both TIM catalogs miss the family by construction. Admission is the exact-extent rule. Rows carry the same fields a `tim_deep_catalog` row does plus a human-readable `label` (item names join in via the descriptor's equipment id), and `decode_block` renders one to RGBA from just the PROT image + spans. |
| `player_file_annex` | Player battle files whose slot region is annexed outside the PROT entry. |

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
- `page(entry, id)` / `pages(entry)` → the same pool as an addressable,
  nameable texture row (`MonsterPage`): one LZS decode for the name, the mesh
  pointer and the pool together. `MonsterPage::ownership()` walks the embedded
  TMD and answers *which palette each texel is read through*, and
  `MonsterPage::rgba()` decodes the page through that map - a texel no
  primitive samples is transparent, because no palette is the truth about a
  texel nothing reads. Decoding a whole page through one palette instead is a
  plausible-looking wrong image; see [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md#texture-replacement).
- `animations(entry, id)` / `idle_animation(entry, id)` decode the per-action
  transform-keyframe streams (one `MonsterAnimation` per action entry: `part_count`
  objects × `frame_count` `PartPose` translation+rotation keyframes; action 0 =
  idle).

CLI `asset monster-archive --id N --obj <out>` exports the mesh, `--texture-png
<out>` bakes the texture page, `--anim` lists the action animations, and `--glb
<out>` exports the whole thing - mesh + baked texture + every action animation - as
a binary glTF (`monster_gltf::export_glb`; per-object animated nodes + a per-palette
texture atlas). For editing rather than exporting, `--dump-block` writes the
monster's whole LZS-decoded block to a file and `--write-block` re-packs an
edited block into its slot in place (`decode_block` / `encode_slot`); the
disc-image equivalent is `legaia-patcher monster-block`.

`monster_model` is the modder-facing OBJ+PNG codec built on top: lossless
export of a monster's mesh + texture page (per-part `o part_NN` groups,
per-vertex baked colours, palette-carrying material names) and the reverse
importer (auto-palletizing into at most 15 CLUTs), feeding
`monster_archive::replace_mesh_and_pool` - the full model-replacement path
behind `legaia-patcher monster-model`.

All three `.glb` exporters share one vertex-colour convention, defined once in
`gltf_color`: a textured prim's packet word rides `COLOR_0` as the
`texel * colour / 128` blend factor, an untextured prim's as a `/255` fill -
both **sRGB-linearized**, because a glTF viewer multiplies `COLOR_0` in linear
light while retail multiplies display bytes, and the raw ratio washes dark
shading out toward gray - and
materials declare `KHR_materials_unlit` because retail applies no light source.
The floats exceed 1.0 for words above `0x80` - deliberately; the module doc has
the reasoning, and `gltf_color::glb_probe` reads an exported `.glb` back so the
claims are asserted on the bytes.

`scene_gltf::build_scene_glb` is the sibling exporter for assembled
VRAM-textured scenes (kingdom continents, full town maps, single scene
TMDs): instanced meshes carrying page-local UVs + per-vertex `(cba, tsb)`
are baked against a supplied `legaia_tim::Vram` into one RGBA atlas (one
256x256 tile per distinct `(cba, tsb-page)` pair; hybrid flat-colour
vertices ride `COLOR_0` + a white tile). Consumed by the web viewer's
`scene_export_*` WASM session behind the site's download buttons.
Disc-gated smoke: `tests/scene_gltf_real.rs`.

`character_gltf::build_character_glb` combines the two models for the
assembled player battle characters: `monster_gltf`'s per-object animated
nodes (one node per rigid TMD object, TRS keyframe channels from clips
pre-expanded per object) textured through `scene_gltf`'s VRAM tile atlas
(the assembled mesh samples runtime VRAM, not a monster page). Each
supplied `CharacterClip` becomes a named glTF animation on its own
timeline (`7.5 * rate` fps). Consumed by the web viewer's arts page
(`LegaiaArts::export_character_glb` - "download this character with every
battle animation"); disc-gated coverage rides
`crates/web-viewer/tests/arts_view_real.rs`.

See [`battle.md`](../../docs/subsystems/battle.md#monster-mesh-record-0x04) and
[`monster-animation.md`](../../docs/formats/monster-animation.md).

### `befect_cluster`

Footprint-bounded extraction of the four-entry window the CDNAME symbol
`befect_data` resolves to in define-number space (extraction PROT 872..875 -
retail-semantically `vdf.dat` / `efect.dat` / the `player_data` file
`player.lzs` / a `sound_data2` VAB stream; the retail befect block proper is
extraction 870..873, see `docs/formats/cdname.md`).
`extract(archive, cdname)` footprint-bounds each entry (a no-op now that
`Archive::read_entry` returns the entry, kept as the explicit contract),
expands the LZS-container entry into its sections, and classifies each part (the
`efect.dat` 2-pack / the field-character TMD pack / the field-character
texture TIMs / packs).

CLI `asset befect-cluster PROT.DAT --cdname CDNAME.TXT --out DIR`. See
[`effect.md`](../../docs/formats/effect.md#battle-effect-cluster-befect_data).

### `summon_readef`

`summon_readef` - the battle side-band streaming files `summon.dat` /
`readef.DAT` (extraction PROT 893 / 894 = retail TOC `0x37F` / `0x380`,
CDNAME block `bat_back_dat`): `0x10800`-byte slots carrying per-special-attack
CLUT rows + 4bpp texture pages plus summon-creature actor records (name + Legaia
TMD + texture pool) and the player art-animation `"ME"` stream archives
(readef slots `3*char+1` / `3*char+2` → `SlotKind::MeArchive`). `parse`
classifies every slot; `stream_target(action_id)` mirrors the retail
id → (file, slot) formula (`FUN_801E295C` case `0x32`). See
[`summon-readef.md`](../../docs/formats/summon-readef.md).

`detect` is the `categorize` entry point (`Class::SummonReadef`): footprint
divisible by `0x10800`, at least 16 slots, and at least three quarters of them
classifying as something other than `SlotKind::Payload`. It **must** run before
the `monster_sound_bank` detector - `summon.dat` opens with `[u32 mode = 2]`
followed by a 256-entry CLUT whose every colour carries the STP bit, which is
byte-for-byte the `[u32 format = 2][u16 spu_addrs[256] all >= 0x8000]` shape that
detector matches. With `summon_readef` ahead of it, `monster_sound_bank` matches
no PROT entry at all.

### `summon_creatures`

`summon_creatures` - the player-summon → namesake `battle_data` creature map.
A base or evolved-Seru summon renders an ordinary `monster_archive` creature
(PROT 867), and that creature's mesh is **byte-identical** to the `summon.dat`
group's actor-record Legaia TMD - so matching each group's TMD against the
archive recovers the whole map for `0x81..=0x95` (base block + evolved-Seru,
including the two evolved legs no capture state covered: `0x90` Kemaro, `0x91`
Spoon). The high block `0x99..=0xA0` has no archive byte-match (bespoke summon
meshes). `SUMMON_CREATURES` + `creature_for_spell`; byte-validated by the
disc-gated `summon_creature_tmd_map_real`. CLI `asset summon-creatures`
(`--scus <SCUS_942.54>` annotates each row with the summon's spell name;
`--json` for the machine-readable map).
