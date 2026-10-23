# legaia-web-minigames

The browser minigames page's runtime: `LegaiaMinigames`, the
`#[wasm_bindgen]` class behind `site/minigames.html`. It is a thin JSON
shell over the minigame rules engines (`engine-minigames`, `engine-fishing`,
`engine-battle-menus`' Muscle Dome session) and holds no `World`: each game
runs straight off the tables decoded from the visitor's own disc. Module by
module, the exports are in
[`docs/subsystems/browser-host.md`](../../docs/subsystems/browser-host.md#playable-minigames-legaia-web-minigames).

## What lives here

| Area | Modules |
|---|---|
| The class and its disc load | `lib.rs` (`LegaiaMinigames`, the dance / Baka / slot state), `disc_load` |
| Per-game sessions | `dance_session`, `baka_session`, `slot_session`, `fishing_web`, `muscle_web/` |
| Presentation exports | `dance_presentation/`, `baka_presentation` (and its `duel_surface` reader), `slot_presentation`, `fishing_scene_web` |
| Sound | `bgm` (`Music01Render`), `live_spu`, `slot_sound` |

## Who else uses it

The play page (`legaia-web-play`) draws an in-world minigame with this
page's decoders and renderers: it holds one `LegaiaMinigames` for the art and
reads it through `duel_surface`, `baka_state_json_for` and the plain
accessors (`slot_cabinet`, the dance cast and venue reads). The class's own
`#[wasm_bindgen]` impls are unchanged by that reuse: the accessors sit in
plain `impl` blocks, so the page's JS API is the same whichever host links it.

## The bundle

An `rlib`, like `web-play`. The wasm module the site loads is
[`legaia-web-viewer`](../web-viewer/README.md)'s `cdylib`, which links this
crate through `web-play`; wasm-bindgen gathers the exports of every linked
crate, so `LegaiaMinigames` reaches the page through the same
`legaia_web_viewer.js` as the rest.

## What it must not depend on

- **No `wgpu`, `winit` or `cpal`.** It takes `engine-audio` with the
  `audio-webaudio` feature.
- **No play runtime.** It never names `web-play`, `engine-core` or
  `engine-scene-host` (the last only as a dev-dependency of one disc-gated
  ladder test): the minigames page runs without a `World`.

## Place in the layering

A host, at the top of the graph beside `web-play`, which sits on it. See
[`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#hosts) and
[`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md) for the
gates that keep the three hosts in step.

## Tests

`tests/` holds the minigames page's API and disc-gated ladders as one
`integration` binary; the disc-gated ones skip without `LEGAIA_DISC_BIN`.
Run one file with
`cargo test -p legaia-web-minigames --test integration <name>::`.
