# VRChat / Unity world export

`legaia-engine export-glb` bakes any CDNAME field, town or world-map scene
into textured `.glb` files plus a JSON manifest, sized and organised for
building the scene as a (private) VRChat world - or for any other glTF
consumer (Blender, Godot, three.js). The Unity-side kit and the end-to-end
guide live in [`scripts/vrchat-world/`](../../scripts/vrchat-world/README.md).

**The output is Sony-derived asset data** decoded from the user's own disc:
it is local, gitignored (`glb-export/`), and never redistributed - same rule
as `extracted/`.

## Invocation

```bash
legaia-engine export-glb --scene town01 --out glb-export       # one scene
legaia-engine export-glb --all-scenes --out glb-export         # everything
legaia-engine export-glb --items --out glb-export              # every equipment item
```

Reads `extracted/` (default) or `--disc <image.bin>`. Flags: `--scale`
(glTF meters per PSX world unit, default 1/64 - one 128-unit walk tile
= 2 m, near VRChat human scale), `--include-sky` (keep the sky-backdrop
shells the site viewers hide), `--no-npcs`, `--no-props`, `--items` (the
equipment export below - standalone or combined with scenes).
`--all-scenes` skips cutscene labels, reports each scene's yield, and
continues past failures.

## What one scene exports

```
glb-export/town01/
├── town01.glb        the assembled static world
├── npcs/*.glb        one animated glb per catalogued MAN placement
├── props/*.glb       animated placed props (windmills, doors)
└── manifest.json     transforms + labels + conventions tying it together
```

Everything composes through the same kernels the browser viewers render
with, so the export matches the on-screen pages:

- **World glb** - `engine-core::scene_assembly::assemble_field_scene` (the
  field-scene page's build): ground heightfield (with the shared
  `GROUND_SINK`), terrain tiles, and placed objects with coplanar lifts
  applied, each instanced at its resolved `.MAP` transform. Placements
  whose object bind names a clip are posed at **frame 0** (the native
  play-window's static bake). Sky shells are dropped by the site's
  `isSkyMesh` heuristic unless `--include-sky`. Baking is
  `legaia_asset::scene_gltf`: one VRAM-derived RGBA atlas, NEAREST
  samplers, packet colours on `COLOR_0` (sRGB-linearized so a
  linear-colour-space importer - glTFast under VRChat's mandated Linear
  setting included - reproduces retail's display-space `texel * colour/128`
  product; a project left in Gamma colour space will render these models
  darker than intended), PSX word-0 transparency as MASK cutout. ABE
  semi-transparent prims split into a second `BLEND` material at half
  alpha (retail's dominant `B/2 + F/2` mode exactly; the blended queue's
  depth-write skip is also what keeps retail's coincident water scroll
  layers from z-fighting in depth-buffered engines).
- **NPC glbs** - `engine-core::npc_catalog` (the MAN partition-1 placement
  walk shared with the NPC browser page) + `legaia_asset::character_gltf`:
  the scene TMD with its spawn clip first and every other
  bone-count-matching scene-ANM record baked as a named glTF animation
  (`record_N`). Spawn transforms live in the manifest, not the file.
- **Animated-prop glbs** - each distinct `(env mesh, bind clip)` placement
  pair whose clip has more than one frame, exported object-local with the
  clip, plus every placement transform in the manifest. The world glb
  keeps their frame-0 static twins, so these are opt-in upgrades.

## `--items`: every equipment item

`--items` walks the four player battle files (`data\battle\PLAYER1..4`,
extraction PROT 863..866) and exports **every equippable record** - all
four characters' weapons, Ra-Seru, armour, headgear and footwear - into
`<out>/items/<character>/`, two files per record plus `items/manifest.json`:

- `*_alone.glb` - the opinionated item-alone cut with its grip repaired
  ([`equip_isolate`](../formats/character-mesh.md) +
  `equip_repair`), what a "give me just the great axe" consumer wants (a
  VRC Pickup, a Blender prop);
- `*_with_limb.glb` - the record-keeping exact palette cut with its
  ground-truth host limb.

Both carry the character's battle action bank and the weapon's spliced
direction swings as named clips, so the piece moves the way it does in
hand. This is the **same kernel the site's characters-page equipment
viewer runs** - `legaia_asset::battle_char_assembly::loadout`, hoisted out
of the wasm session so both hosts share one implementation - and the
manifest repeats each record's honesty tags (`class`, `complete`,
`isolation_mode`, `curated`, `grip_bridges`) so a heuristic cut is
distinguishable from a curated one. `SCUS_942.54` supplies display names
and section labels when readable; without it, ids stand in. Like the NPC
and prop glbs, item glbs ship in **raw PSX units**.

## Manifest

`manifest.json` carries: the scale and axis conventions (spelled out under
`conventions`), a suggested `spawn` (component-wise median of the placed
objects - the village centre, not the map-grid centre), composition
`stats`, and three placement arrays:

- `npcs[]` - file, kind (`talk` / `door` / `prop`), the dialog first line
  as a label, position (floor height via the retail sampler
  `World::sample_field_floor_height`), clip names, `conditional` (parked
  off-map until a script places it), `model_index` / `anim_id`;
- `doors[]` - portal placements with their field-VM target map;
- `animated_props[]` - file, clip frame count, per-instance
  position + yaw.

Transforms are in the **export frame**: the site renderers' convention
(mesh-local Y flipped at bake so the model reads +Y-up,
mirror-handedness and all), already multiplied by `scale`; yaw in radians
about +Y exactly as the world-glb instances apply it - a **standard**
positive rotation about +Y, which lands on the authored retail yaw. That
is a sign conversion away from the internal `placementModelScaledY`
param (the page function's inline rotY block is transposed, so its param
is the negated yaw); `scene_gltf` performs the negation when emitting
node quaternions, and the manifest mirrors it. A consumer that applies
`rot_y_radians` as a plain `Ry` therefore matches the baked frame-0
twins exactly. The frame is
**X-mirrored relative to the site pages' presentation**: a consumer that
wants the explorer's orientation mirrors the assembled scene once on X
(the kit builder's default-on toggle). That fact is settled by a
landmark test, not by the shader chain - viewing town01 from the sea
(the sea-to-gate axis pins the viewpoint, so only parity can differ),
the raw glb puts known buildings on the opposite sides from the
field-scene page, and no rotation swaps sides across a content-pinned
axis. Counting reflections through the page view chain (`u_pair_front`
in `site/js/webgl-shaders.js`) gave confident wrong answers in both
directions before the landmark test - re-run the test, don't re-count.
The NPC / prop `.glb`s themselves stay in **raw PSX units**
(matching the site's character downloads), so a consumer scales each
instance by the manifest's `scale` - the kit's builder does.

## Verification

Disc-gated oracle `crates/engine-core/tests/glb_export_real.rs`: assembles
town01, bakes all three families, validates every glb container (magic,
length, JSON chunk), asserts the sky filter, the bind-resolve that feeds
the prop split, clip counts against manifest cross-references. Skip-passes
without `LEGAIA_DISC_BIN`.

## Faithful vs. approximated

Geometry, textures, packet-colour shading, placement transforms, floor
heights and the walk surface are retail-parity kernels. Approximations, by
design of a static export: NPCs loop a clip instead of running their
field-VM scripts, props free-run instead of waiting for script triggers,
doors don't warp. The manifest's `kind` / `dialog` / `target_map` fields
exist to seed the next layer - porting those behaviours to Udon in the
world kit.
