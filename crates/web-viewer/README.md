# legaia-web-viewer

WebAssembly bindings for browsing a Legend of Legaia disc image in the
browser.

Auto-detects the input form: a full Mode2/2352 `.bin` disc, a raw
`PROT.DAT`, or a single `.tim`. After loading a disc, classifies every
PROT entry via `legaia_asset::categorize` and pre-scans for embedded
TIMs, so the UI shows a filtered, browsable list of viewable entries
instead of every raw entry.

## What's wrapped

- `legaia-iso` — disc reader.
- `legaia-prot` — TOC + CDNAME.
- `legaia-asset` — categorize + tim_scan.
- `legaia-lzs` — LZS decoder.
- `legaia-tmd` — mesh parser.

Rendering targets the canvas's 2D context (`CanvasRenderingContext2d` +
`ImageData`) — no `wgpu` dependency on the WASM target. The 3D path
(`tmd3d` module) does software rasterisation onto the same canvas,
which is enough for "browse the asset library from a phone" but not
enough to drive a real game scene.

## Build

`wasm-bindgen` for the JS bindings; `wasm-pack` for packaging.
`wasm-opt` is disabled in `Cargo.toml` to keep the build reproducible
across environments without an emscripten install.

```bash
# Direct invocation:
wasm-pack build crates/web-viewer --target web

# Or via the convenience script that also syncs into site/wasm/ for
# local previewing:
scripts/build-wasm.sh
```

The generated `pkg/` is consumed by the static site under
[`site/`](../../site/). `site/wasm/` is gitignored — the build script
regenerates it from `pkg/` on demand.

## Serve locally

```bash
scripts/build-wasm.sh
python3 -m http.server -d site 8000
# then open http://localhost:8000/viewer.html
```

The viewer instantiates `mod.LegaiaViewer('viewer-canvas')` against the
canvas in `site/viewer.html`. Drop a `.bin`, `.dat`, or `.tim` onto the
page; nothing leaves the browser.

## Crate type

`crate-type = ["cdylib", "rlib"]` — `cdylib` for the WASM build,
`rlib` so the host renderer in `site/` can also link against it for
ahead-of-time bundling experiments.

## See also

- [`site/`](../../site/) — the landing site that hosts this viewer.
