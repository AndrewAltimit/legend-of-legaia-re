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
`field_scene_terrain_{slots,positions}`, `field_scene_ground_*`.

Each `field_scene_mesh` is a **hybrid** (`build_hybrid_env_mesh`): the
VRAM-filtered textured prims plus the untextured flat/gouraud
vertex-colour prims the textured builder drops - the browser sibling of
the native engine's colour-mesh pipeline, so colour-only props (benches /
fences / small furniture) render instead of vanishing.
`field_scene_mesh_flat_rgba` returns the parallel per-vertex
`[r, g, b, flag]` array (flag `255` = textured / sample VRAM, `0` =
untextured / use the colour; empty for pure-textured meshes), consumed by
the WebGL shader's `u_use_flat_colors` / `a_flat_rgba` hybrid path.

Two site pages drive it through the shared `site/js/field-scene-view.js`
(`window.FieldSceneView`): the **game-world** page's town navigator, which
swaps locations in place, and the **asset viewer**'s "full map" button. The
view streams the draws through the WebGL renderer's instanced scene-mesh
path (the same plumbing as the world-overview kingdom continents) and
classifies sky-dome shells and horizon-backdrop planes (huge-footprint /
zero-depth-sheet AABB heuristic, `FieldSceneView.isSkyMesh`), hiding their
draws - under the assembled camera they'd paint over the map they surround
in retail. `load()` is re-entrant, so a navigator swap releases the previous
scene's GL meshes rather than leaking them. Disc-gated parity test:
`tests/field_scene_assembly.rs` (incl. the colour-only prop recovery).

## Field-NPC catalog (`field_npc`)

`LegaiaViewer::set_scene_npcs(name)` loads the field scene (for its TMD pool
+ VRAM) and catalogs every actor the scene's MAN places;
`field_npc_catalog_json()` returns the list. An NPC is not a separate asset
class - it's a **MAN partition-1 placement record**: a model byte indexing
the scene's TMD pool (`res.tmds[model_index]`, *not* the env-pack subset the
map placements use), an anim byte naming a record in the scene's ANM bundle,
and tile bytes for the spawn. `build_npc_catalog` is the pure builder behind
the binding (disc-gated test: `tests/field_npc_catalog.rs`).

Per-actor mesh accessors mirror the character family:
`field_npc_mesh(catalog_idx)` + `field_npc_mesh_{positions,uvs,cba_tsb,
indices,object_ids,flat_rgba,bounds}`. The mesh is the field-hybrid build
(`tmd_to_vram_mesh_field_hybrid`), so it carries per-vertex object ids *and*
flat colours in one stream.

**The pose is load-bearing.** A multi-object character TMD ships its
vertices in object-local space, so drawn raw its parts collapse onto the
origin; the figure only assembles as `v_world = R_bone . v_object_local +
T_bone` from frame 0 of the placement's clip (the page composes this via the
existing `player_anm_record_pose_frames`, keyed on the catalog's `anm_prot`).
The catalog therefore lists only actors it can assemble: multi-object actors
with no clip, or in a scene shipping no ANM bundle at all (`rikuroa`), are
withheld and reported as `unposable_count` rather than shown as a heap.
Party / save-point heads (`model_index >= 0xF0`) draw from the global pool
instead of the scene's and are routed to the characters page
(`special_count`). Off-map "hidden" spawns are script-gated story actors,
fully resolvable, so they *are* listed - flagged `conditional`.

## Scene `.glb` export (`scene_export`)

Builder-style session on `LegaiaViewer` so the site pages can download
**exactly what they render** as a binary glTF: `scene_export_begin(name)` /
`scene_export_set_vram(bytes)` / `scene_export_add_mesh(name, positions,
uvs, cba_tsb, indices, flat_rgba) -> handle` /
`scene_export_add_instance(handle, tx, ty, tz, rot_y, scale)` /
`scene_export_finish() -> Vec<u8>`. The page feeds the same mesh buffers it
uploads to WebGL plus the same per-draw `(translation, rotY, scale)`
triples it builds model matrices from; the bake
(`legaia_asset::scene_gltf::build_scene_glb`) renders every distinct
`(cba, tsb-page)` pair the vertices sample into a 256x256 tile of one RGBA
atlas (the PSX VRAM+CLUT indirection has no glTF equivalent), remaps UVs,
and keeps hybrid meshes' untextured vertices via `COLOR_0` + a white atlas
tile. Consumers: the world-overview page (assembled continent), the
game-world page's town navigator and the viewer page's full-map mode (both
via `FieldSceneView.exportGlb`), the viewer's single-TMD inspector, and the
characters + NPC pages (via `MeshView.exportGlb`, which bakes the **posed**
vertices - the object-local parts would otherwise arrive in the file as a
heap at the origin). The monster page's enemy export stays on the sibling
`monster_gltf::export_glb` (it additionally carries the action
animations). Disc-gated smoke: `legaia-asset`'s
`tests/scene_gltf_real.rs`.

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
