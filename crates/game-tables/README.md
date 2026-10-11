# legaia-game-tables

The game's decoded data tables: fixed-layout records read out of
`SCUS_942.54` or out of an overlay image, with no asset dispatcher or scene
bundle in between.

## The rule

[`legaia-overlay-images`](../overlay-images/README.md) describes **a code
image** (how to detect one, where it loads, how a slot-B module is laid out);
this crate decodes **the data tables inside an image**, wherever the table is
resident. A table module takes its overlay's PROT index and load base from
`legaia_overlay_images::static_overlay` instead of restating them, so this
crate depends on that one and never the reverse.

## What lives here

- **Items + equipment** (`SCUS_942.54`) - `item_names`, `item_effect`,
  `equip_stats`, `accessory_passive`, `steal_table`.
- **Spells** (`SCUS_942.54`) - `spell_names`.
- **Progression** (`SCUS_942.54`) - `level_up_tables`, `new_game`,
  `victory_pose`.
- **Battle overlay tables** (PROT 0898) - `move_power` (per-move power +
  behaviour, [`move-power.md`](../../docs/formats/move-power.md)),
  `element_affinity`, `battle_camera_table`, `battle_attack_camera_table`,
  `spell_anim_pairs`, `seru_side_effect`, `absorb_caption`.
- **Menu overlay tables** (PROT 0899) - `menu_windows` (the window descriptor
  table, [`field-menu.md`](../../docs/subsystems/field-menu.md)),
  `widget_script` (the window-script programs,
  [`window-script.md`](../../docs/formats/window-script.md)).
- **Modes, movies, sound** - `mode_table`, `sfx_table`, `xa_cue_table`
  (`SCUS_942.54`); `fmv_dispatch` (STR overlay, PROT 0970); `str_fmv_table`
  (the cutscene player's in-RAM movie lookup table).
- **World map** (`SCUS_942.54`) - `worldmap_menu` (the quick-travel landmark
  menu).
- **Derived** - `seru_trade`, the Seru-trade offer table the patcher and the
  engine both compute from a seed. It is not disc data; it lives here because
  it is keyed on the spell and item id spaces above.

The format pages under [`docs/formats/`](../../docs/formats/overview.md)
(`item-table.md`, `spell-table.md`, `equipment-table.md`, ...) carry each
table's layout and provenance.

## Layering

Depends on `legaia-bytes` and `legaia-overlay-images`. It must not depend on
the container layer or on any scene, UI, minigame or actor-model format
crate; those sit above it and read these tables.
