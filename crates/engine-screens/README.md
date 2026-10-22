# legaia-engine-screens

The screens both play hosts draw - the shop family, the battle HUD, the pause
menu - composed once. Free of wgpu,
winit and cpal, so it builds for native and `wasm32` alike.

`engine-ui` holds the renderer-agnostic builders and does not link
`engine-core`; `engine-core` holds the state and the retail kernels and draws
nothing. This crate is the projection between them for:

- the **gold shop** - the phase's window set, picker, paged list and party
  column (`engine-ui::shop_screen`) plus the retail descriptor windows: vendor
  plate (33), purse (32), item info (34), buy / sell quantity (35 / 37), sell
  detail (39), equipment-buy recipient list (36) and the Point Card toast (31);
- the casino **prize exchange** (windows 43..46) and **coin counter**;
- the **fallback panel** when no gold-shop screen is up - the seru-trade offer
  list / confirm, the inn prompt and resting caption, the `[label]` stand-in -
  with its gold frame sized off the panel's own rows;
- the **level-up / Seru-capture banners** (framed outside battle with the
  chrome, loose pens without);
- the **fishing HUD banner step** (`fishing_banner_frame`): the pond session's
  events seed the one-shot timers and the tick's draws come back for a draw
  pass that cannot advance them; the **fishing HUD draw list**
  (`fishing_hud_items`: persistent rows, catch HUD, result plate, banners)
  and the **rod and line** screen prims (`fishing_line_screen_prims`), and
  the **point-exchange screen** (`fishing_exchange_draws`) on the swaying
  venue panel;
- the **battle value readout's layout** (`battle_value_readout_layout`): the
  combo cluster and the newest numeral per struck actor, seated through the
  host's camera;
- smaller projections each host used to carry a copy of: the battle item
  window's frame (`with_battle_item_frame`), the command-menu chip cluster
  (`battle_command`), the inventory item-use screen
  (`inventory_use_session_draws`), the developer menu's row list and Records
  page with their pens and headings (`dev_menu`), the dance / Baka Fighter
  HUD primitives and the Muscle Dome arena hub's screen-quad selection
  (`minigame_hud::muscle_hub_quads`);
- the **battle screens** (`battle_screens`): the HUD frame off the host's
  `BattleHud` model, its chrome sprites (strip, item window, command chips,
  the field-side banner frame), the sparring tutorial's boxes, the arts
  command-input chrome and the Muscle Dome chrome gate, from one
  `BattleScreenInputs`;
- the **pause menu's screens** (`pause_screens`): the context-locked pair,
  the root list with its save-refusal notice, and every sub-screen but Load /
  Save, projected from the live sessions into `engine-ui::pause_menu`'s views
  for a host's `PauseMenuCtx`; and the save-select screen itself
  (`save_select_overlay_draws`), which the boot Continue -> Load path shares;
- the **battle text list** (`battle_screens::battle_text_draws`,
  `battle_tutorial_stage_draws`): the HUD text, the submenu stand-ins, the
  tutorial rows and the timed-fight strip;
- the **dialog reading box** (`dialog_screens`): the pager snapshot, its
  stage layout off the pager's own row word, and both halves of the box;
- the **field message layer** (`field_messages`): the text balloon, the map
  display's labels, the tile-board prompt, the Incense notice and the reading
  box, in one order under one set of gates;
- the **map display's reads of the world** (`world_map_labels`): the L1
  display's discovered place labels, compass marks and mini-map panel prims,
  projected off the world-map camera, which `field_messages` and
  `screen_layers` draw;
- the **field-side projections** (`field_screens`): the save-menu atlas'
  band table and badge cells, the name-entry view, the field party HUD, and
  the passive-ability badge column over the player's head
  (`passive_hud_icons` through the host's camera, `passive_hud_text_draws`);
- the **frame's field decisions** (`field_frame`): enhanced lighting's mood
  and the fog bank's tint under it, the enhanced scene lights
  a frame draws and the frame clear colour, off the world and the open
  screen; and the camera-occlusion fade's world half (`field_occlusion`:
  whether the fade arms, the player's body centre and feet it aims at),
  beside the host terms only a host can answer;
- the **screen-space primitive layers** (`screen_layers`): the frame's
  `under` / `over` PSX primitive lists, composed in one order for both hosts.

## How a host uses it

Build a [`ScreenInputs`](src/lib.rs) from the host's own holders - the scene
`World`, the `MenuRuntime`, the font, the menu overlay's window table, the
system-UI chrome rects, the spell / Seru name table and
`SceneHost::coin_counter_lines` - and call `shop_overlay_frame` once per frame.
The returned `ShopOverlayFrame` carries:

- `stage_texts` - retail 320x240 stage pixels; the host scales them through
  `pause_menu::stage_transform` + `scale_stage_text_draws`;
- `sprites` - surface pixels (window frames, atlas cursors / pictograms, the
  fallback panel's frame);
- `banner_texts` - stage pixels, scaled the same way.

What stays with the host: assembling the inputs, the stage scale, layer order,
and the upload (wgpu on `engine-shell`'s `play-window`, quad JSON on the
`web-play` play page). The native window builds the frame in its redraw and
hands it to both its text pass (`build_hud`) and its chrome sprite pass.

The individual pieces (`gold_shop_screen`, `shop_window_draws`,
`sell_detail_window_draws`, `recipient_window_draws`, `prize_window_draws`,
`fallback_panel_draws`, `banner_stage_draws`, ...) are public for tests and
tooling. The shared pens, window ids, renderer VAs and window-39 labels are
the crate's constants; no host keeps a copy, which is why the UI host-drift
gate (`scripts/ci/check-ui-host-drift.py`) credits a builder called here to
each host that calls one of the crate's entries (`shop_overlay_frame`,
`battle_hud_frame_draws`, `battle_chrome_sprite_draws`,
`field_menu_root_draws`, `field_menu_sub_draws`, `battle_text_draws`,
`dialog_box_chrome_draws`, `dialog_box_text_draws`, `field_party_hud_draws`,
`save_select_overlay_draws`, `battle_value_readout_layout`,
`fishing_hud_items`, `fishing_line_screen_prims`, `fishing_exchange_draws`,
`passive_hud_text_draws`;
[`host-drift.md`](../../docs/tooling/host-drift.md)).
