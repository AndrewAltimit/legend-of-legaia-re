# legaia-web-viewer

WebAssembly bindings for browsing a Legend of Legaia disc image in the
browser.

Auto-detects the input form: a full Mode2/2352 `.bin` disc, a raw
`PROT.DAT`, or a single `.tim`. After loading a disc, classifies every
PROT entry via `legaia_asset::categorize` and pre-scans for embedded
TIMs, so the UI shows a filtered, browsable list of viewable entries
instead of every raw entry.

## What's wrapped

- `legaia-iso` - disc reader.
- `legaia-prot` - TOC + CDNAME.
- `legaia-asset` - categorize + tim_scan + `monster_archive`.
- `legaia-lzs` - LZS decoder.
- `legaia-tmd` - mesh parser.
- `legaia-rando` + `legaia-iso` - the randomizer / disc patcher (see `rom_patcher` below).

## Assembled full-scene maps (`field_scene`)

`LegaiaViewer::set_scene_field(name)` loads a CDNAME field/town scene
through the **real engine loaders** and surfaces the whole assembled map -
the answer to "a `scene_asset_table` entry viewed alone shows one
object-local mesh at the origin". The build is the engine-parity path:
`SceneResources::build_targeted_with_options` (field-mode VRAM pre-pass +
the LZS-packed environment TMD pack), the shared
`engine-core::field_env` kernel (env-pack vote + `.MAP` object-grid
placement resolution + floor-height-LUT world Y), the terrain-tile layer,
and the walk-ground heightfield. Accessors mirror the kingdom `pack_*`
family: `field_scene_mesh(slot)` + `field_scene_mesh_*` per-mesh arrays,
`field_scene_vram_bytes`, `field_scene_placement_{slots,positions}`,
`field_scene_terrain_{slots,positions}`, `field_scene_ground_*`. The
site's viewer page drives it from the "full map" button, streaming the
draws through the WebGL renderer's instanced scene-mesh path (the same
plumbing as the world-overview kingdom continents). Disc-gated parity
test: `tests/field_scene_assembly.rs`.

## In-browser ROM patcher (`rom_patcher`)

`rom_patcher::patch_rom(image, seed, drops, encounters, chests)` runs the
Track-1 [`legaia-rando`](../rando/README.md) randomizer entirely client-side and
returns `{ data, summary, seed }` - the patched disc bytes for download, a
human-readable change report, and the resolved numeric seed. `resolve_seed`
exposes the seed-string hash so the page can display it. It drives the static
site's `tooling/rom-patcher.html` page: the user supplies their own disc, toggles
the drop / encounter / chest settings, and downloads a patched image. The disc
bytes never leave the browser and nothing is uploaded - the same "user supplies
the disc" model as the CLI, so the site ships only code.

`LegaiaViewer::monster_archive_json` decodes the global monster stat
archive (PROT entry 867, extended footprint) into a JSON array of every
populated record (id / name / HP / MP / stats). It drives the static
site's `monsters.html` enemy-table page entirely client-side - the disc
bytes never leave the browser. Per row, the page also renders the enemy's
3D battle model: `monster_mesh_{positions,normals,indices,bounds,uvs,palette_index}`
plus `monster_texture_{indices,palette_rgba,dims}` feed a textured WebGL2
viewer (the embedded TMD at record `+0x04`, coloured from the decoded
texture pool at `+0x08` via the prim-CBA palette lookup).

Rendering targets the canvas's 2D context (`CanvasRenderingContext2d` +
`ImageData`) for TIM blits and the canvas's WebGL2 context for textured
3D TMDs - no `wgpu` dependency on the WASM target. The 3D path
(`tmd3d` module + `site/js/webgl-tmd.js`) does either software
painter's-algorithm rasterisation or a paletted GPU shader matching
the engine-render VRAM-mesh pipeline, which is enough for "browse the
asset library from a phone" but not enough to drive a real game scene.

A canvas can only ever bind one rendering context type for its lifetime
(once `getContext("webgl2")` succeeds, `getContext("2d")` returns null
forever on that element). The host page swaps in a fresh `<canvas>`
between entry switches and `LegaiaViewer` re-resolves it by id on every
2D draw, so flipping back to a TIM entry after viewing a TMD entry
keeps working. When a disc is loaded, primitives whose texture pages
weren't supplied are dropped from the mesh before upload, which
prevents the "solid green / cyan tint" symptom for entries that
reference TIMs sitting in other PROT entries.

## Build

`wasm-bindgen` for the JS bindings; `wasm-pack` for packaging.
`wasm-opt` is disabled in `Cargo.toml` to keep the build reproducible
across environments without an emscripten install.

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

The viewer instantiates `mod.LegaiaViewer('viewer-canvas')` against the
canvas in `site/viewer.html`. Drop a `.bin`, `.dat`, or `.tim` onto the
page; nothing leaves the browser.

## Crate type

`crate-type = ["cdylib", "rlib"]` - `cdylib` for the WASM build,
`rlib` so the host renderer in `site/` can also link against it for
ahead-of-time bundling experiments.

## See also

- [`site/`](../../site/) - the landing site that hosts this viewer.
