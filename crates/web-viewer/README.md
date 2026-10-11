# legaia-web-viewer

The WebAssembly crate behind the static site. It takes a disc image the
visitor picks - a full Mode2/2352 `.bin`, a raw `PROT.DAT`, or a single
`.tim` - and nothing leaves the browser.

## Three products in one crate

The crate serves three different things, and each has its own corner of
`src/`:

| Product | What it is | Where in `src/` |
|---|---|---|
| **Asset inspectors** | The disc browser and the viewer pages: entry classification, TIM / TMD / MES / SEQ / VAB inspectors, full-map field scenes, NPC, arts, summon and equipment viewers, `.glb` export, the world-overview geometry. | `nav_disc`, `inspect`, `viewer_render`, `disc`, `catalog`, `field_scene`, `field_npc`, `character`, `monster`, `arts_view`, `equipment_view`, `summon_view`, `sfx_view`, `scene_export`, `scene_geom`, `audio_api` |
| **The play / minigames runtime** | The browser hosts of the port. `LegaiaRuntime` holds the same `BootSession` the native window ticks; `LegaiaMinigames` drives the minigame rules engines outside a scene. | `runtime/`, `play/`, every `play_*` module, `boot_title`, `host_slot`, `pad_bindings`, `session_save`, `cards`, `resume`; `minigames/` and every `minigames_*` module |
| **The ROM patcher** | The client-side randomizer, texture replacer and language-pack workbench over `legaia-patcher` and its layer crates. | `rom_patcher/`, `texture_registry`, `texture_pack`, `translate_workbench` |

The runtime is not a second implementation of the game: it links the same
engine crates as `legaia-engine` and differs only in what draws (the site's
WebGL renderer in `site/js/`) and what plays (`engine-audio`'s WebAudio
output). Module by module, the exports are in
[`docs/subsystems/browser-host.md`](../../docs/subsystems/browser-host.md).

## What it must not depend on

- **No `wgpu`, `winit` or `cpal`.** It never links `engine-render`, and it
  takes `engine-audio` with the `audio-webaudio` feature.
- **No native host.** `engine-shell` and `parity` are not dependencies;
  `engine-shell`'s dev-only host-parity tests link *this* crate, not the
  other way round.

## Place in the layering

A host, at the top of the graph beside `engine-shell`: it sits on
`engine-session` and `engine-screens` (the game loop and shop-family screens
both play hosts share), on `engine-core` / `engine-scene` and the kernel
crates directly, on the format crates for the inspectors, and on `patcher`
with its layer crates for the ROM patcher. See
[`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#hosts) and,
for the gates that keep it in step with the native window,
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
native Rust can link it: this crate's own `tests/` and `engine-shell`'s
dev-only host-parity tests drive the browser host beside the native one.

## See also

- [`site/`](../../site/) - the site that hosts these pages.
- [`docs/tooling/shipped-bundle-freshness.md`](../../docs/tooling/shipped-bundle-freshness.md) - is the local `site/wasm/` bundle still this tree's.
