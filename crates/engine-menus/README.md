# legaia-engine-menus

The engine's menu, title and memory-card front end, minus `World`: the item
and equipment catalogs, the inventory-use and spell-menu sessions, the SCUS
list-row model every pause-menu list window shares, the menu overlay's widget
choreography and tables, the title and boot screens, the card write flow and
`bu` I/O layer, and the field dialog pager's presentation kernels. Free of
wgpu, winit and cpal, so it builds for native and `wasm32` alike.

`legaia-engine-core` owns the composition - `MenuRuntime`, the pause-menu
dispatch, `World::use_item` and the stepping of the dialog and cutscene
contexts - and re-exports every
module here at its old path, so `legaia_engine_core::items` and
`legaia_engine_menus::items` name the same module. Two are re-exported by
glob from a thin engine-core module of the same name instead (`shop`,
`pause_screens`): each screen here takes the slice of the world it reads
(the roster, the equipment stat table, the item-effect table) as plain
arguments (`party_compare_members_of`, `target_panel_view_model_of`), and
the engine-core module adds the `World`-taking entry that passes them.

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, `legaia-engine-system` (pad input) or the other `World`-free crates
below `engine-core` (`legaia-engine-battle` for the stat aggregator and spell
catalog, `legaia-engine-vm` for the window-script VM). Doc links that point
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
  its active window, with a map-shaped adapter; `World` holds one and
  engine-core re-exports it as `world::ItemBag`) and `equip_session` (the
  pause-menu Equip screen's session, which borrows the bag rather than the
  world).
- **Pause + shop screens** - `field_menu` (the pause root's session),
  `pause_screens` (the Items / Magic / Equip screen sessions and view
  models, the root route table, the kind-4 list kernel) and `shop` /
  `shop_catalog` (the buy / sell / quantity / recipient sessions, the shop
  window set and the disc gold-shop catalog); see
  [`docs/subsystems/shop.md`](../../docs/subsystems/shop.md).
- **Spells** - `spell_menu` (the out-of-battle cast flow) and
  `spell_party_broadcast` (`FUN_8003053C`).
- **Menu overlay** - `menu_widget` (the window-widget choreography:
  `MenuWidgetScripts` resolves the window-script VM's programs out of the
  menu-overlay image, and `MenuWidgetState` is the `legaia_engine_vm::Host`
  window-list model the shop open / Sell slide-away programs run against,
  with engine-core's `MenuRuntime::tick` driving the edges as `FUN_801DAFD4`
  does; see [`docs/formats/window-script.md`](../../docs/formats/window-script.md)),
  `menu_open_sequence` (`FUN_801DAD6C`), `menu_cues` (which blip a frame's pad
  edges fire), `menu_glyph_atlas`, `save_menu_atlas` (the shared 256x256
  save / pause / battle-badge sprite bake), `debug_char_editor` (the
  developer character editor) and `key_rebind`.
- **Title + boot** - `publisher_logos`, `title` (title state machine),
  `title_screen_atlas`, `name_entry` (the `town01` naming screen) and
  `game_over` (party wipe to the title).
- **Memory card + save screen** - `card_flow` (the write / format flow over
  the card I/O machine), `card_bu_io` (the `bu` device wrappers),
  `save_select` (the slot-select session, card directory and card I/O
  machine), `save_screen` (the save screen's host half), `save_subscreen`
  (the menu overlay's sub-screen dispatcher and its routed-id table), and the
  two pages it reaches without `World`: `status_screen` and `list_order`; see
  [`docs/subsystems/save-screen.md`](../../docs/subsystems/save-screen.md).
- **Dialog presentation** - `dialog` (the pager panel `OwnedDialogPanel`
  over a scene MES message or an inline segment, and `SceneMes`, the scene's
  resolved MES container), `dialog_window` (the pager's row window and
  scroll), `dialog_pacing` (typewriter reveal), `dialog_picker_slide` (how a
  picker enters) and `text_balloon` (the `4C E1` one-line balloon).
- **Spawned script contexts** - `inline_dialogue` (the resumable state of an
  actor's inline interaction script run through the field VM) and
  `cutscene_timeline` (the spawned partition-2 record context, the modal
  cutscene timeline). The stepping of both stays on engine-core's `World`,
  which holds the field host borrow; these modules hold the state.
- **Inn** - `inn` (the rest confirmation and HP / MP restore session; see
  [`docs/subsystems/inn.md`](../../docs/subsystems/inn.md)).

## See also

- [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md) - the
  pause menu these kernels serve.
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
- [`crates/engine-system`](../engine-system/README.md) - pad input below this
  crate.
