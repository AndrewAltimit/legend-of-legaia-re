# legaia-engine-menus

The engine's menu and memory-card front end, minus `World`: the item
and equipment catalogs, the inventory-use and spell-menu sessions, the SCUS
list-row model every pause-menu list window shares, the menu overlay's widget
choreography and tables, the card write flow and
`bu` I/O layer. The field dialog pager sits one crate down, in
[`legaia-engine-dialog`](../engine-dialog/README.md); the publisher logos and
the title screen are a sibling, [`legaia-engine-title`](../engine-title/README.md). Free of
wgpu, winit and cpal, so it builds for native and `wasm32` alike.

`legaia-engine-menu-runtime` owns the composition - `MenuRuntime`, the
pause-menu dispatch - and `legaia-engine-core` owns `World::use_item` and
the stepping of the dialog and cutscene contexts. Consumers name the modules here directly
(`legaia_engine_menus::items`). Two have an engine-core module of the same
name beside them (`shop`,
`pause_screens`): each screen here takes the slice of the world it reads
(the roster, the equipment stat table, the item-effect table) as plain
arguments (`party_compare_members_of`, `target_panel_view_model_of`), and
the engine-core module holds the `World`-taking entry that passes them.

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, `legaia-engine-system` (pad input) or the other `World`-free crates
below `engine-core` (`legaia-engine-battle` for the stat aggregator and spell
catalog, `legaia-engine-vm` for the window-script VM,
[`legaia-engine-dev-menu`](../engine-dev-menu/README.md) for the field
overlay's dev-menu row kernels and the equip applier the pause menu's equip
session shares, `legaia-engine-camera` for the options screen's camera
distance preset). The crate names no field kernel: the dev-menu bodies sit in
a crate below both it and `legaia-engine-field`. Doc links that point
back up at `engine-core` are plain code spans, since rustdoc cannot resolve a
link into a dependent crate.

## Modules

- **Items + equipment** - `items` (the item-effect catalog keyed by real
  retail item ids; `apply_effect` resolves an `ItemEffect` against a
  `TargetSnapshot`), `equipment` (the equipment slot model and vanilla
  table), `inventory_use` (`InventoryUseSession`, the field + battle item-use
  state machine whose outcome `World::use_item` folds in), `menu_list_rows`
  (the SCUS list-node allocator and row builders), `menu_item_category` (the
  item category / weapon-favor table) and `menu_arrange` (the Arrange rank
  table at menu-overlay `0x801E4A88` and the bag-sort kernel
  `FUN_801D64A8`), `item_bag` (`ItemBag`, retail's 256-slot bag array and
  its active window, with a map-shaped adapter; `World` holds one) and `equip_session` (the
  pause-menu Equip screen's session, which borrows the bag rather than the
  world).
- **Pause + shop screens** - `field_menu` (the pause root's session),
  `pause_screens` (the Items / Magic / Equip screen sessions and view
  models, the root route table, the kind-4 list kernel) and `shop` /
  `shop_catalog` (the buy / sell / quantity / recipient sessions, the shop
  window set and the disc gold-shop catalog), and `shop_cues` (the root
  picker's rows and labels, the list page lengths, and the UI cue each shop
  input raises); see
  [`docs/subsystems/shop.md`](../../docs/subsystems/shop.md).
- **Battle command input** - not in this crate. The command picker, the
  open banner, the command-flow byte, the timed fight and the Muscle Dome
  match session are
  [`legaia-engine-battle-menus`](../engine-battle-menus/README.md), a
  sibling; the options screen's value enums they key on (`option_values`)
  are `legaia-engine-system`'s.
- **Options** - `options`: `OptionsState` and its TOML persistence, the
  options screen session and its row model, the key-rebind flow over
  `key_rebind`, and the retail-deviation report. The push of the simulation
  knobs onto a `World` is `engine-core::options::apply_to_world`.
- **Spells** - `spell_menu` (the out-of-battle cast flow) and
  `spell_party_broadcast` (`FUN_8003053C`).
- **Menu overlay** - `menu_widget` (the window-widget choreography:
  `MenuWidgetScripts` resolves the window-script VM's programs out of the
  menu-overlay image, and `MenuWidgetState` is the `legaia_engine_vm::Host`
  window-list model the shop open / Sell slide-away programs run against,
  with engine-menu-runtime's `MenuRuntime::tick` driving the edges as `FUN_801DAFD4`
  does; see [`docs/formats/window-script.md`](../../docs/formats/window-script.md)),
  `menu_open_sequence` (`FUN_801DAD6C`), `menu_cues` (which blip a frame's pad
  edges fire), `menu_glyph_atlas`, `save_menu_atlas` (the shared 256x256
  save / pause / battle-badge sprite bake), `debug_char_editor` (the
  developer character editor), `dev_menu_host` (the developer menu screen
  it sits on; engine-menu-runtime's `DevMenuWorldExt` runs it against a `World`) and
  `key_rebind`.
- **Naming + game over** - `name_entry` (the `town01` naming screen) and
  `game_over` (party wipe to the title). The publisher logos and the title
  screen are [`legaia-engine-title`](../engine-title/README.md).
- **Memory card + save screen** - `card_flow` (the write / format flow over
  the card I/O machine), `card_bu_io` (the `bu` device wrappers),
  `save_select` (the slot-select session, card directory and card I/O
  machine), `save_screen` (the save screen's host half), `save_subscreen`
  (the menu overlay's sub-screen dispatcher and its routed-id table), and the
  two pages it reaches without `World`: `status_screen` and `list_order`; see
  [`docs/subsystems/save-screen.md`](../../docs/subsystems/save-screen.md).
- **Dialog presentation + spawned script contexts** - not in this crate. The
  pager (`dialog`, `dialog_window`, `dialog_pacing`, `dialog_picker_slide`,
  `text_balloon`) is
  [`legaia-engine-dialog`](../engine-dialog/README.md); the
  `inline_dialogue` and `cutscene_timeline` context state is
  `legaia-engine-field`. The stepping of both contexts is on
  engine-core's `World`, which holds the field host borrow.
- **Inn** - `inn` (the rest confirmation and HP / MP restore session; see
  [`docs/subsystems/inn.md`](../../docs/subsystems/inn.md)).
- **Session state held on `World`** - `party_state` (roster, active party,
  money, inventory, arts, level-up tracking, name entry) and `shop_state`
  (the gold-shop and prize-exchange sessions), each one subsystem's state
  held as a unit on the composite `World`.
- **New Game record seeding** - `new_game` (the live record a starting-party
  template row seeds, the scene-picker party's starter loadout, the
  cold-boot defaults bundle `NewGameDefaults` and the game-state cold-reset
  values `GAME_STATE_COLD_RESET`). engine-core's `new_game` applies them to
  a `World`.

## Game-over banner and menu state machine

- `gameover_banner` - the game-over banner letter stager `FUN_801CE844`.
- `menu` - the engine's pause / shop / inn menu state machine.

## See also

- [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md) - the
  pause menu these kernels serve.
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
- [`crates/engine-system`](../engine-system/README.md) - pad input below this
  crate.
