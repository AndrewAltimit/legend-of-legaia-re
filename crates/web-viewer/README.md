# legaia-web-viewer

The WebAssembly bundle behind the static site. It takes a disc image the
visitor picks - a full Mode2/2352 `.bin`, a raw `PROT.DAT`, or a single
`.tim` - and nothing leaves the browser.

## One bundle, two crates

The site loads one wasm module, and it is this crate's `cdylib`. One of
the products in it is this crate's own; the others are linked in:

| Product | What it is | Where |
|---|---|---|
| **Asset inspectors** | The disc browser and the viewer pages: entry classification, TIM / TMD / MES / SEQ / VAB inspectors, full-map field scenes, NPC, shop-vendor, arts, summon and equipment viewers, `.glb` export, the world-overview geometry. | `nav_disc`, `inspect`, `viewer_render`, `catalog`, `field_scene`, `field_npc`, `npc_dialog`, `shop_vendors`, `character`, `monster`, `arts_view`, `equipment_view`, `summon_view`, `sfx_view`, `scene_export`, `scene_geom`, `audio_api` |
| **The ROM patcher** | The client-side randomizer, texture replacer and language-pack workbench over `legaia-patcher` and its layer crates. | [`legaia-web-patcher`](../web-patcher/README.md) |
| **The play / minigames runtime** | The browser hosts of the port: `LegaiaRuntime` and `LegaiaMinigames`. | [`legaia-web-play`](../web-play/README.md), [`legaia-web-minigames`](../web-minigames/README.md) |

wasm-bindgen gathers the `#[wasm_bindgen]` exports of every crate the
`cdylib` links, so the runtime's exports reach the page through the same
`legaia_web_viewer.js` as this crate's: the site's JS API does not depend on
which crate an export lives in. The viewer pages also stand on `web-play`'s
field actor layer (`field_actors`, `field_npc`, `field_scene_anim`), naming
those modules there directly, and read the disc through `legaia_iso::image`
and `legaia_prot::toc_index`. Module
by module, the exports are in
[`docs/subsystems/browser-host.md`](../../docs/subsystems/browser-host.md).

## What it must not depend on

- **No `wgpu`, `winit` or `cpal`.** It never links `engine-render`, and it
  takes `engine-audio` with the `audio-webaudio` feature.
- **No native host.** `engine-shell` and `parity` are not dependencies.

## Place in the layering

At the top of the graph beside `engine-shell`: it sits on `web-play` (the
browser hosts), on `engine-scene-host` / `engine-scene` and the kernel crates the
viewer pages read, on the format crates for the inspectors, and on
`web-patcher` for the ROM patcher (named in `lib.rs` with `extern crate` so
its exports are linked: nothing else here references it). See
[`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#hosts) and,
for the gates that keep the browser host in step with the native window,
[`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md).

## Build

`wasm-bindgen` for the JS bindings; `wasm-pack` for packaging.
`wasm-opt` runs strip-only (`--strip-debug --strip-producers`, set in
`Cargo.toml`): a full `-O` pass costs far more build time than the few
percent of gzipped size it saves.

```bash
# Direct invocation:
wasm-pack build crates/web-viewer --target web

# Or via the convenience script that also syncs into site/wasm/ for
# local previewing:
scripts/ci/build-wasm.sh
```

The generated `pkg/` is consumed by the static site under
[`site/`](../../site/). `site/wasm/` is gitignored - the build script
regenerates it from `pkg/` on demand.

## Serve locally

```bash
scripts/ci/build-wasm.sh
python3 -m http.server -d site 8000
# then open http://localhost:8000/viewer.html
```

## Crate type

`crate-type = ["cdylib", "rlib"]` - `cdylib` for the WASM build, `rlib` so
this crate's own `tests/` can link it natively. The browser host's tests,
and `engine-shell`'s dev-only host-parity tests, link `legaia-web-play`
instead.

## See also

- [`site/`](../../site/) - the site that hosts these pages.
- [`docs/tooling/shipped-bundle-freshness.md`](../../docs/tooling/shipped-bundle-freshness.md) - is the local `site/wasm/` bundle still this tree's.
