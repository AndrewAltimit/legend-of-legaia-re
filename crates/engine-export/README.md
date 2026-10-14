# legaia-engine-export

The world exporter behind `legaia-engine export-glb`: it bakes a scene the
engine can load into textured `.glb` files plus a JSON placement manifest, for
Unity / VRChat world building. Free of wgpu, winit and cpal.

The output carries Sony-derived asset data. It is user-generated and local,
and must never be committed or redistributed.

## What it produces

One module, `glb_export`, with one entry point per artifact family:

- **World glb** (`export_world_glb`) - ground heightfield, terrain tiles and
  placed objects, each draw instanced at its resolved world transform, bound
  placements posed at frame 0 of their object-bind clip.
- **NPC glbs** (`export_npc_glbs`) - one animated `.glb` per catalogued MAN
  placement, with every bone-count-matching clip of the scene bundle baked as
  a named glTF animation.
- **Animated-prop glbs** (`export_animated_prop_glbs`) - each distinct
  `(env mesh, clip)` pair whose bind clip runs more than one frame, exported
  object-local with its placement transforms in the manifest.
- **Traversal data** (`export_scene_traversal`, `FloorSampler`) - walk grid,
  doorway teleports, portals and trigger reach, sampled off the retail floor
  sampler.
- **Equipment items and party field forms** (`export_equipment_item_glbs`,
  `export_party_field_glbs`) with their manifests (`items_manifest`,
  `party_manifest`), beside the per-scene `world_manifest`.

## Where it sits

Above `engine-core`, beside the hosts' other shared layers. The mesh and
transform kernels are `engine-scene::scene_assembly` and the glTF writers are
`actor-formats::scene_gltf` / `character_gltf` - the same ones the browser
viewers render with, so an exported file matches the on-screen scene. What
this crate adds is the part that needs the simulation: it steps a scratch
`World` to sample ambient animation lanes and to resolve the traversal data
against a scene staged at its free-roam visit, and it reads NPC placements
through `engine-scene::npc_catalog`.

Nothing in `engine-core` or below names this crate, so the play hosts'
library builds and the wasm bundle do not link it; the native `legaia-engine`
binary is its one consumer.

## Tests

`tests/glb_export_real.rs` is the disc-gated oracle: it assembles a town
scene, bakes every family and validates each `.glb` container and the
manifest's cross-references. It skips when `extracted/` or `LEGAIA_DISC_BIN`
is missing.

Reference: [`docs/tooling/vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md).
