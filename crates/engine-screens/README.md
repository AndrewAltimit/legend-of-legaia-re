# legaia-engine-screens

The shop-family screens both play hosts draw, composed once. Free of wgpu,
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
  chrome, loose pens without).

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
`web-viewer` play page). The native window builds the frame in its redraw and
hands it to both its text pass (`build_hud`) and its chrome sprite pass.

The individual pieces (`gold_shop_screen`, `shop_window_draws`,
`sell_detail_window_draws`, `recipient_window_draws`, `prize_window_draws`,
`fallback_panel_draws`, `banner_stage_draws`, ...) are public for tests and
tooling. The shared pens, window ids, renderer VAs and window-39 labels are
the crate's constants; no host keeps a copy, which is why the UI host-drift
gate (`scripts/ci/check-ui-host-drift.py`) credits a builder called here to
each host that calls `shop_overlay_frame` ([`host-drift.md`](../../docs/tooling/host-drift.md)).
