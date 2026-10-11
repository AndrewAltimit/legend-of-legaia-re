# legaia-actor-formats

What an actor is built from on the disc - the party in its field and battle
forms, the monsters, the summon creatures - plus the animation that poses
them and their glTF export. The crate was `legaia-battle-models` until it
took the field-form character pack; nothing here is battle-only any more.

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

The root manifest keeps a transitional `legaia-battle-models` key naming this
package, so a dependent not yet rewritten still links it as
`legaia_battle_models`. New code takes `legaia-actor-formats`.
