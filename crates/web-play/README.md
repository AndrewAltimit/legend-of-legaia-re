# legaia-web-play

The browser play page's runtime. `LegaiaRuntime` holds the same
`legaia_engine_session::BootSession` the native window ticks. The minigames
page's runtime, `LegaiaMinigames`, is
[`legaia-web-minigames`](../web-minigames/README.md); the play page holds one
for the art it draws over an in-world minigame.

It is not a second implementation of the game. It links the same engine
crates as `legaia-engine` and differs only in what draws (the site's WebGL
renderer in `site/js/`) and what plays (`engine-audio`'s WebAudio output).
Module by module, the exports are in
[`docs/subsystems/browser-host.md`](../../docs/subsystems/browser-host.md).

## What lives here

| Area | Modules |
|---|---|
| The play runtime | `runtime/` (the simulation: disc load, scene setup, the frame), `play/` and every `play_*` module (what the page draws and plays), `boot_title`, `boot_continue`, `cards`, `resume`, `session_save`, `host_slot`, `pad_bindings` |
| The field actor layer | `field_actors` (the MAN-placed actors over a live world), `field_npc` (the NPC catalog they draw from), `field_scene_anim` (the CLUT-walk runner) - shared with the field-scene viewer in `legaia-web-viewer` |

A visitor's disc image is read through `legaia_iso::image` (the in-memory
ISO walk and `PROT.DAT` / `CDNAME.TXT` / SCUS extraction) and
`legaia_prot::toc_index` (the zero-copy `PROT.DAT` entry index), the format
layer's borrowed-slice readers.

## The bundle

This crate is an `rlib`. The wasm module the site loads is
[`legaia-web-viewer`](../web-viewer/README.md)'s `cdylib`, which links this
crate: wasm-bindgen gathers the `#[wasm_bindgen]` exports of every linked
crate, so `LegaiaRuntime` (and, through this crate, `LegaiaMinigames`) reach the page through the
same `legaia_web_viewer.js` as the viewer exports, and the site's JS API does
not depend on which crate an export lives in.
[`scripts/ci/build-wasm.sh`](../../scripts/ci/build-wasm.sh) builds it.

## What it must not depend on

- **No `wgpu`, `winit` or `cpal`.** It never links `engine-render`, and it
  takes `engine-audio` with the `audio-webaudio` feature.
- **No native host.** `engine-shell` and `parity` are not dependencies;
  `engine-shell`'s dev-only host-parity tests link *this* crate, not the
  other way round.
- **No inspector or patcher.** The viewer pages and the ROM patcher are
  `legaia-web-viewer`'s; this crate never names it.

## Place in the layering

A host, at the top of the graph beside `engine-shell`: it sits on
`engine-session` and `engine-screens` (the game loop and the screens both
play hosts share), on `engine-core` / `engine-scene` and the kernel crates
directly. See
[`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#hosts) and, for
the gates that keep it in step with the native window,
[`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md).

## Tests

`tests/` holds the play page ladders and parity tests (with the play page's own minigame rungs), built
as one `integration` binary; the disc-gated ones skip without
`LEGAIA_DISC_BIN`. Run one file with
`cargo test -p legaia-web-play --test integration <name>::`.
