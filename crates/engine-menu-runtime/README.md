# legaia-engine-menu-runtime

The pause menu over a running `World`. `engine-menus` holds the menu rules and
screens and names no `World`; `engine-core` holds the `World` they edit. This
crate is the seam between them: it owns the menu's runtime state, builds each
sub-session from the world, and writes each outcome back.

## What lives here

- **`menu_runtime`** - `MenuRuntime`, the host of the `engine-menus` menu state
  machine: the save-slot directory and its disk round-trip (`SAVE_EXT`), the
  shop flow, the inn sessions (including `open_scene_inn`, which reads the
  scene's inn cost off the `SceneHost`), the level-up and art-learned notices,
  and the `MenuHost` implementation the state machine calls back into.
- **`field_menu_dispatch`** - the pause menu's seven rows routed to their
  sub-sessions (`FieldMenuSubsession`): the session builders that read the
  world (status snapshots, the Items bag, equipment, the spell list, the arts
  editor, the options screen, the warp list), the per-frame root-list step,
  and the outcome appliers that write each result back to the world.
- **`card_write`** - a session's `SaveFile` written into a mounted memory-card
  image, the one kernel both play hosts' card Save commits: the save number a
  block is filed under (`card_save_index`, over retail's
  `classify_card_directory`), the slot portrait off the disc's save-icon sheet
  (`retail_block_icon`), and the payload, resume point, block identity and
  directory claim (`write_save_into_card`). It reads no `World`; it sits here
  because it reads the disc through `ProtIndex` and is the card side of the
  save-slot directory above.
- **`dev_menu_host`** - the developer menu's `World` seam
  (`DevMenuWorldExt` on `engine-menus`' `DevMenuSession`): one frame of the
  screen against the live world and its camera, and its cues routed onto the
  world's SFX ring. Both hosts' dev menus call it.
- **`bag_rows`** - the bag's retail list rows (the SCUS content builder
  `FUN_80030628`) as the `WorldBagRows` extension trait: the Items screen's
  Use and Throw Out lists, the shop's sell list, and the by-slot spends their
  confirms make (`discard_bag_slot`, `consume_bag_slot`). The row builders
  themselves are `engine-menus::menu_list_rows`.
- **`menu_validator`** - the out-of-battle host of the action validator
  `FUN_8003FB10` (`RosterValidator`, over the roster records) and the Magic
  screen's "would this spell do anything" question (`spell_affects_anyone`).

## Place in the layering

```text
engine-shell / web-play                         hosts
engine-session, engine-screens, parity          host-shared top
engine-menu-runtime                             the pause menu over World
engine-scene-host                               SceneHost, resume, live preview
engine-core                                     World, the tick, battle loop
engine-menus, engine-* kernels                  World-free rules
```

Nothing below it names it: `World` holds no menu runtime, and the one fact the
battle HUD and the world-map panel read from the menu side, the roster's
display names, is `PartyState::roster_names` in `engine-menus`. The hosts and
`engine-screens` hold a `MenuRuntime` beside their `SceneHost`.

## Tests

`tests/*.rs` build as one `integration` binary; select a file by module path
(`cargo test -p legaia-engine-menu-runtime --test integration foo::`). Tests
that read a disc skip and pass when `LEGAIA_DISC_BIN` is unset.

## Where the behaviour is documented

| Topic | Page |
|---|---|
| Pause menu, window table, panels | [`field-menu.md`](../../docs/subsystems/field-menu.md) |
| Shop, inn, save screen | [`shop.md`](../../docs/subsystems/shop.md), [`inn.md`](../../docs/subsystems/inn.md), [`save-screen.md`](../../docs/subsystems/save-screen.md) |
| The item bag the Items row edits | [`inventory.md`](../../docs/subsystems/inventory.md) |
