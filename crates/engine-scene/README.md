# legaia-engine-scene

The disc-facing half of the engine: resolving a CDNAME scene to its PROT
entries and loading / decoding what the scene is built from. Nothing here
holds simulation state. `legaia-engine-core` depends on this crate and adds
the `World` and the scene *host* that ticks a loaded scene; this crate never
depends on it. Hosts, tools and tests name its modules directly
(`legaia_engine_scene::scene::Scene`).

## What lives here

| Module | Covers |
|---|---|
| `vfs` | Where asset bytes come from: the `Vfs` trait, its `DirVfs` (an extracted directory), `DiscVfs` (a disc image) and `MemoryVfs` backends, and the `AssetCache` over them. |
| `scene` | `ProtIndex` (the opened `PROT.DAT` + CDNAME map, with entry and class caches), `Scene` (one CDNAME block's classified entries and every per-scene resolver on it: MAN, `.MAP`, floor LUT, triggers, heightfield, encounters, BGM), the map-id resolvers, the cutscene label / `MVn.STR` map, and the effect-texture VRAM uploads. See [`asset-loader.md`](../../docs/subsystems/asset-loader.md). |
| `scene_bundle` | The per-scene asset table inside a loaded `Scene`, walked into typed sub-assets. See [`scene-bundles.md`](../../docs/formats/scene-bundles.md). |
| `scene_assets` | `SceneAssets`: the typed per-scene snapshot (MES, SEQ / VAB stream entries, event scripts). |
| `scene_resources` | `SceneResources`: a scene's TMD pools, ANM bundle and software VRAM, built per load kind (`build_targeted`). |
| `model_bank` | The retail model pool `DAT_8007C018` and the id space a placement's model byte and the scripted-motion VM's op `0x0E` share. |
| `field_env` | The environment-geometry draw resolver: which `SceneResources` TMD each `.MAP` placement or terrain tile draws and where, object binds, prop animation, floor anchors. |
| `coplanar_draws` | Cross-draw coplanar-surface separation over a resolved `EnvDraw` list. |
| `scene_assembly` | The full static scene assembly kernel (`AssembledScene`, `assemble_field_scene_with`) and the hybrid textured / vertex-colour mesh builders. |
| `man_field_scripts` | A scene's MAN carriers and the scene-name entry points of the censuses whose decoders are `legaia_engine_field::man_field_scripts`. |
| `clut_walk_anim` | The CLUT-walk shimmer: installs a scene's walker table and steps it over a VRAM. |
| `cutscene_caption` | The opening's baked caption TIM, decoded to RGBA. |
| `cd_dma` / `overlay_loader` / `stream_file` | The CD streaming reader host traits, the `ProtIndex`-backed host, the overlay loaders and the dual-mode stream-file API. |

## The split line

A module is here when its dependency closure inside the engine stops at the
`World`-free kernel crates: `engine-field` (field regions, MAN script
decoders), `engine-battle` (encounter tables), `engine-dialog`,
`engine-mode`, `engine-system` and `render-kernels`. It must not name
`legaia-engine-core`, and the compiler enforces that - a consumer that only
reads scenes links this crate without the simulation.

What stays above, in `engine-core`:

- `scene_host` - `SceneHost`, which owns a `World` and a `ProtIndex`, enters
  scenes and ticks them.
- `scene_staging` - the scene questions that take simulation state: whether
  a placed object is live against a running world's static-object window,
  the story-hidden record set (a scratch `World` staged at the scene's
  free-roam visit), and `assemble_field_scene`, which hands that set to
  `scene_assembly::assemble_field_scene_with`.
- `npc_catalog` and `glb_export` - the first resolves placements against the
  world's global model pool, the second steps a `World` to bake animation.
- The per-subsystem disc readers that return another kernel crate's types
  (`battle_open`, `dance`).

Doc comments name `World` items in plain code spans, since rustdoc cannot
resolve a link into a dependent crate.

## See also

- [`crates/engine-core`](../engine-core/README.md) - the simulation half
  above this crate.
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - crate
  layering for the whole engine track.
- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  - the retail loader chain these modules mirror.
