# legaia-game-tables

The game's static data tables: fixed-layout records parsed straight out of
`SCUS_942.54` and the battle / field / STR overlay images, with no asset
dispatcher or scene bundle in between. The one crate dependency besides
`legaia-bytes` is [`legaia-overlay-images`](../overlay-images/README.md): a
table resident in an overlay validates pointers against the overlay window
and reads effect records in the slot-B part-record format. Split out of
[`legaia-asset`](../asset/README.md), which re-exports every module here at
its old path (`legaia_asset::item_names`, ...), so its README's per-module
rows keep describing them.

- **Items + equipment** - `item_names`, `item_effect`, `equip_stats`,
  `accessory_passive`, `steal_table`.
- **Spells + Seru** - `spell_names`, `spell_anim_pairs`, `seru_side_effect`,
  `seru_trade`, `absorb_caption`.
- **Progression** - `level_up_tables`, `new_game`, `victory_pose`.
- **Battle overlay tables** - `element_affinity`, `battle_camera_table`,
  `battle_attack_camera_table`, `move_power` (per-move power + behaviour,
  [`move-power.md`](../../docs/formats/move-power.md)).
- **Menu overlay tables** - `menu_windows` (the window descriptor table,
  [`field-menu.md`](../../docs/subsystems/field-menu.md)), `widget_script`
  (the window-script programs,
  [`window-script.md`](../../docs/formats/window-script.md)).
- **Modes, movies, sound** - `mode_table`, `str_fmv_table`, `fmv_dispatch`,
  `sfx_table`, `xa_cue_table`.
- **World map** - `worldmap_menu` (the quick-travel landmark menu).

The format pages under [`docs/formats/`](../../docs/formats/overview.md)
(`item-table.md`, `spell-table.md`, `equipment-table.md`, ...) carry each
table's layout and provenance.
