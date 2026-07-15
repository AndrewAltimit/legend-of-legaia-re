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

## Playing the port in the browser (`runtime` + `play`)

`LegaiaRuntime` is the engine, not a re-implementation of it: it owns a real
`legaia_engine_core::scene::SceneHost` - the same host the native
`legaia-engine play-window` drives - so the browser runs the ported field /
event VM, the free-movement controller against the per-scene walkability grid,
floor-height sampling, the NPC motion VMs, the interaction probe, and the
inline-script dialogue runner. Drives `site/play.html`.

The split is deliberate:

- **`runtime`** - the simulation. `load_disc` (user's own image, in memory, in
  their browser), `enter_field(name)`, `set_pad(mask)` (PSX pad word),
  `set_camera_azimuth(units)` (so the d-pad remaps camera-relative),
  `tick_frame()` (returns the label of the scene a door just walked into, so the
  page rebuilds around a transition), and `state_json()` (frame / mode / player
  transform / live dialogue box).
- **`play`** - what the page draws, resolved against the **same**
  `SceneResources` the host already built at scene entry (nothing is decoded
  twice): the assembled map (`field_*` accessors), the lead's field mesh posed
  each frame from the world's live `pose_frame` (`player_mesh_*`,
  `player_transform`), and the scene's MAN-placed NPCs at their live world
  positions (`play_npc_*`).

The map + NPC layers make the **native play-window's exact resolver calls**,
pinned by the disc-gated parity test `tests/play_parity.rs`:

- the placed-object layer goes through
  `field_env::resolve_placed_env_draws` **with the scene's object binds**, so
  a placed prop whose bind names a clip carries its `anim_id`
  (`field_placement_anim_ids`) and the page draws it through
  `field_mesh_posed(slot, anim)` - the frame-0 rest pose of scene ANM record
  `anim_id - 1` (cupboard doors closed on the cabinet's front face) with the
  native fallback to the raw mesh under the bone-count contract;
- the terrain sweep excludes `FLAG_PLACED` records (already drawn - posed -
  by the placement layer; the second copy would be the unposed one);
- the NPC catalog (`field_npc::build_npc_catalog_play`) lists **everything
  the native window draws**: the `model >= 0xF0` global-pool specials (save
  crystal / party heads, meshed from the world's pool and posed from the
  PROT 0874 locomotion bundle) and the clipless multi-object actors retail
  draws raw (draw kind 5), which the curated NPC-browser catalog withholds;
- a catalogued NPC's mesh truncates its TMD object table to its clip's bone
  count (the objects past it are equipment-swap templates), and a slot with
  no seeded heading renders at identity, both as the native draw pass does.

Two things the browser host has to do that the native one gets for free:

- **Seating.** Scene entry puts the player at the retail *cold-boot spawn*,
  which is only meaningful for `town01` - every other scene is normally entered
  through a door that supplies the arrival tile. Picking a scene from a list has
  no door, so `LegaiaRuntime::seat_player` keeps the cold spawn when it has floor
  under it and otherwise seats the player on the walk-ground heightfield, never
  on a walk-on trigger tile (which would fire on the first tick and warp the
  scene out from under them).
- **Framing.** Retail authors a camera per scene. The page has one follow camera,
  so `site/js/play-app.js` culls any mesh straddling the camera-to-player line -
  without it a cave roof or a house's upper storey fills the screen.

## Retail pause menu (`play_menu`)

Start (Enter) opens the real retail field menu, drawn from the same wgpu-free
`legaia-engine-ui` builders the native `play-window` uses - not a DOM stand-in.
`LegaiaRuntime::play_menu_*` owns the state + navigation and serves two draw
lists (`play_menu_draws_json`): the gold 9-slice + navy-filigree window chrome
as sprite quads off the disc's menu-UI atlas (`save_menu_atlas::build_atlas`
over PROT 0899 + the PROT.DAT system-UI sheet), and the labels as font-glyph
quads. Window geometry is the disc-parsed descriptor table
(`legaia_asset::menu_windows`) with the native window's pinned fallback. The two
atlases upload once (`play_menu_font_rgba` / `play_menu_chrome_rgba`); the page's
`AtlasBlitter` (an `image-rendering: pixelated` overlay `<canvas>` over the GL
view) blits the quads with a per-quad multiply tint. The top-level command list
plus the Status and Options sub-screens run their live `legaia-engine-core`
sessions (`StatusScreenSession` / `OptionsSession`); the remaining rows open the
generic framed window the native shell also uses for its not-yet-pinned screens.

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

The per-vertex `cba_tsb` stream carries each prim's PSX **semi-transparency**
state (ABE enable in TSB bit 15 - `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`
- and the ABR blend mode in bits 5..=6) for textured prims and, via the
hybrid merge, for the untextured colour-half verts (their `ColorMesh::blend`
word lands in the TSB slot; the flat path never samples VRAM, so it's blend
metadata only). The WebGL renderer (`site/js/webgl-tmd.js`) draws these prims
in a deferred **blend pass**: the opaque pass discards blending texels
(prim ABE + texel STP, `u_semi_pass = 0`), then per-ABR-mode index tails
(`buildSemiTail`, the browser mirror of engine-render's
`psx_blend::append_semi_tail`) re-draw them depth-tested but not
depth-written with the matching GL blend state (`0.5B+0.5F` / `B+F` / `B-F`
/ `B+0.25F`). This is what makes fountain water (Hunter's Spring) and house
window light read translucent instead of opaque grey. Disc-gated pin:
`tests/field_scene_blend.rs`.

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

## Playable minigames (`minigames`)

`LegaiaMinigames` is a standalone `#[wasm_bindgen]` class (its own
`load_disc`, no canvas) that runs three of the game's side-games in the
browser for `site/minigames.html`. It is a thin JSON shell over the
clean-room rules engines in `legaia-engine-core` - the beat clock + judge
(`dance`), the rock-paper-scissors duel (`baka_fighter`), the reel state
machine + payout eval (`slot_machine`). It carries no rules of its own.

Every table each game plays with is decoded from the visitor's own disc via
the same path the play-window uses (raw PROT entry ->
`static_overlay::as_loaded` -> table parser): the step chart out of PROT
0980, the roster + action tables out of 0976, the payout table out of 0975.
Nothing is shipped with the site.

Per game: `<g>_start` / `<g>_tick` / an input method
(`dance_press` / `baka_choose` / `slot_spin` + `slot_stop` +
`slot_collect`) / `<g>_state_json`. `load_disc` returns a status object
naming which games' overlays resolved, so a disc that can't feed one game
still plays the others. `dance_state_json` deliberately surfaces **both**
halves of retail's split chart lookup - `judged` (what the hit judge
matches, the step to press) and `displayed` (the display half's
held-sequence substitution); see `docs/subsystems/minigame-dance.md`.
Disc-gated oracle: `tests/minigames_wasm_api.rs`.

`minigames_baka.rs` adds the Baka Fighter duel's **presentation** exports so
the page draws with the cabinet's own assets: per-side fighter mesh buffers
(player = PROT 1204 battle pack slot, opponent = its own PROT 1206..=1219
`[TIM][TMD][anim]` pack), pose-frame decodes from the real animation banks
(PROT 1203 bank records `char*9 + action` for the party, the pack's own bank
for the opponent), the 51-record HUD widget table + PROT 1203 art pages, the
stage TMD set, and a per-duel 1 MB VRAM build. Consumed by
`site/js/minigame-baka.js`; see
`docs/subsystems/minigame-baka-fighter.md` § HUD widget table.

`minigames_dance.rs` adds the dance's **presentation** exports: the PROT
1230 art pack's HUD page decoded through its row-500 CLUT strip
(`dance_hud_page_rgba`), the overlay's 34-record widget table + the traced
emitter geometry (`dance_widgets_json` / `dance_layout_json`, incl. the
capture-pinned `+4`-line draw-environment offset), the dancer face-stamp
windows with the pose blits replayed (`dance_face_rgba`; dancer 0 = Noa's
field atlas, PROT 0874 §2), the SFX cue bank (PROT 1228 descriptors +
the PROT 1231 sample VAB - a TOC-tail entry) plus the direct-keyed hit
stings (`dance_sting_pcm`), and the real BGM pair rendered through the
clean-room SPU (`dance_bgm_pcm_i16`). Consumed by
`site/js/minigame-dance.js`; see `docs/subsystems/minigame-dance.md`.
Disc-gated oracle: `tests/minigames_dance_api.rs`.

## Session saves + retail cards (`session_save`)

The play page's save boundary. Engine sessions round-trip as **LGSF**
(`LegaiaRuntime.export_save` / `import_save` = `World::save_full` /
`load_full` with magic + version validation - a corrupt upload throws a
readable message and leaves the session untouched). Retail **emulator
saves** are first-class: `card_saves_json(bytes)` lists the Legaia saves
inside a raw `.mcr`/`.mcd` card image, DexDrive `.gme`, or single-save
`.mcs` (party names, gold, coins, location, the CDNAME scene label);
`LegaiaRuntime.import_card_save(bytes, block)` lifts one into the live
world via `legaia_save::SaveFile::from_retail_sc_block`; and
`card_patch_coins(bytes, block, coins)` banks browser-minigame coin
winnings into the pinned retail coin slot (SC `+0x464`, RAM
`0x800845A4`) **in place** - the container comes back in the format it
arrived in with only those 4 bytes changed, so an untouched export is
byte-identical and the patched save still loads in the emulator. PS3
`.psv` is rejected (signed container). The minigames page's **save bar**
draws on two more exports: `card_icon_rgba(bytes, block)` decodes the SC
block's own 16x16 memory-card icon (palette `+0x60`, 4bpp pixels
`+0x80` - for Legaia that is the lead character's baked portrait), and
`LegaiaMinigames.save_portrait_rgba(char_id)` decodes the three 16x16
load-screen portrait TIMs (Vahn / Noa / Gala) from the pre-`init_data`
gap of `PROT.DAT`, the faces the bar's tiles show; save summaries carry
the lead's displayed level (record `+0x130`). Persistence (localStorage,
base64) lives in `site/js/legaia-saves.js`; the bar itself is
`site/js/minigame-saves.js`; this module is serialization only.

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

An optional `lang_pack` YAML argument (default `""` = English, strictly opt-in)
applies a [language pack](../rando/README.md#translation-packs) **before** any
randomizer pass (translate-then-randomize composes; the reverse loses relocated
scenes' lines). The page offers the shipped `site/lang/*.yaml` packs by dropdown,
plus an import path (user-supplied YAML) and `export_lang_pack` (dump a
source-bearing working pack from the user's own disc to author one) and
`validate_lang_pack` (disc-measured dry run before patching). The packs are
static assets fetched from `site/lang/`, never bundled into the WASM.

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
