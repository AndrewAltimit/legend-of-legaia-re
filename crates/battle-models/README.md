# legaia-battle-models

Battle model formats and their glTF export, split out of
[`legaia-asset`](../asset/README.md), which re-exports every module here at
its old path (`legaia_asset::monster_archive`, ...), so its README's
per-module rows keep describing them.

- **Monsters** - `monster_archive` (the PROT 867 monster archive: records,
  meshes, animations) and `monster_model` (the modder-facing OBJ+PNG
  export / import surface).
- **Party** - `battle_data_pack` (the player battle files `PLAYER1..4`),
  `battle_char_assembly` (the per-character equipment assembly, battle
  animations, swing / art animations, the equipment cuts and the loadout
  kernel), `battle_char_pack` (the `other5` battle-form mesh pack),
  `battle_char_palette` (the in-battle party CLUTs), `face_anim` (battle
  facial animation) and `me_archive` (the `"ME"` keyframe-stream archive).
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
