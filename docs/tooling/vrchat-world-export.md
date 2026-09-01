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
├── town01.glb        the assembled world (+ baked morph animation, below)
├── npcs/*.glb        one animated glb per catalogued MAN placement
├── props/*.glb       animated placed props (windmills, doors)
├── music/bgm_*.wav   the scene's entry BGM as a seamless loop
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
- **Baked ambient vertex morphs** - when the scene arms the engine's
  scene-entry VDF pulse (`engine-core::vdf_pulse` - a populated type-7
  morph pack with no retail entry arming; Rim Elm's shoreline is the
  flagship), the world glb carries one glTF **morph target** per envelope
  lane on the affected meshes (Unity blendshapes, named
  `vdf_lane_<n>` via `extras.targetNames`) plus a looping `vdf_pulse`
  weights animation sampled over exactly one envelope period
  (fingerprint-detected, ~3 s for town01) - the wave sheet washes in and
  out in any glTF player. Scenes where retail owns the morph arming are
  left alone, same self-guard as the play hosts.
- **Scene music** - the MAN's first op-`0x35` sub-1 BGM start, rendered
  through the engine SPU + sequencer (`render_bgm_loop_region`) to
  `music/bgm_<id>.wav`. Only the detected loop region is written, so a
  looping `AudioSource` repeats it seamlessly; the manifest `music` block
  carries the id, curated title, sample rate and loop length.

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
and section labels when readable; without it, ids stand in. Unlike the
scene NPC / prop glbs (which bake the export scale onto their root),
item glbs ship in **raw PSX units** - matching the site's character
downloads they share a kernel with.

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
  position + yaw, plus two door tags: `is_door` (the instance stands on a
  doorway-teleport trigger - retail parks a door placement on its own
  doorway tile, so its clip is the door record's swing, meant to open on
  approach rather than loop; the tag covers both teleport families, where
  a script-tile anchor join structurally misses every map door) and
  `near_portal` (the instance stands on a scene-exit band - a gate leaf);
- `teleports[]` - retail's **intra-scene doorways**, both families
  ([`field-locomotion.md`](../subsystems/field-locomotion.md) § intra-scene
  doorways): the `.MAP` kind-0 map-door table and the object walk-touch
  script doors (arm-resolved against the cold-entry flag state). Each
  entry is a trigger box (position + `half_extents` - one collision tile
  wide for a map door, the retail `0x50` contact half-extent plus a
  capsule allowance for a script door; the vertical span covers the
  min..max floor sampled around the contact, because retail's dispatch is
  a height-blind 2D check for a point player and a literal box misses
  where the contact tile samples a different floor layer than the
  approach ground, or where a recessed door's channel is narrower than a
  player capsule), a floor-sampled destination, and `facing_dir` (the authored
  arrival facing as a unit XZ direction, `null` = keep the walked-in
  facing). Walking into the box and repositioning the player IS the retail
  behaviour - a house interior is an unused corner of the same map;
- `scene_portals[]` - gate-1 walk-on portal sites whose partition-2 record
  runs a `0x3F` named scene change (town exits / overworld entrances):
  trigger box, `target_scene`, the arrival `entry_xz` in the **target**
  scene's export frame, `facing_dir`, and the story-flag `conditional`
  alternative when the record branches. A multi-scene build wires these
  into cross-scene teleports;
- `music` - the rendered BGM loop (`file`, `bgm_id`, curated `title`,
  `sample_rate`, `loop_seconds`, `seamless_loop`);
- `world_anim` - present when the world glb carries the baked `vdf_pulse`
  morph clip (name + loop length).

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
The NPC / prop `.glb`s carry the export `scale` **baked on their root
node** (`conventions.npc_prop_units: "scaled"`), so a file dragged into a
scene next to the built world is already world-sized - unlike the site's
character downloads, which stay in raw PSX units. They still differ from
the world glb in **handedness**: the world bakes the site convention's
Y-mirror into its vertices (determinant -1), while the NPC / prop files
are proper-rotation models (root `Rx(180)`, determinant +1). Placing a
prop into the world therefore needs one mirror in the instance transform -
`Ry(rot_y_radians) * diag(1, 1, -1)` composed onto the file's root - or no
yaw will ever line it up with its baked frame-0 twin. The kit's builder
applies this as a negative-Z instance scale (unit magnitude for current
exports; a manifest without the `npc_prop_units` flag is an older
raw-units export whose instances get the full `scale`).

## Verification

Disc-gated oracle `crates/engine-core/tests/glb_export_real.rs`: assembles
town01, bakes all three families, validates every glb container (magic,
length, JSON chunk), asserts the sky filter, the bind-resolve that feeds
the prop split, clip counts against manifest cross-references, both
doorway-teleport families plus the map01 gate portal, the door tagging,
and the shoreline morph targets + `vdf_pulse` animation. Skip-passes
without `LEGAIA_DISC_BIN`.

## Faithful vs. approximated

Geometry, textures, packet-colour shading, placement transforms, floor
heights, the walk surface, the doorway trigger/landing data, the shoreline
morph deltas and the BGM render are retail-parity kernels. Approximations,
by design of a static export: NPCs loop a clip instead of running their
field-VM scripts, non-door props free-run instead of waiting for script
triggers, door swings collapse to open-on-approach (the kit's `LegaiaDoor`)
rather than the full door-record choreography, the morph *arming* cadence is
the engine's scene-entry pulse enhancement rather than a retail cue, and
script-door arms are frozen at the cold-entry story-flag state. The
manifest's `kind` / `dialog` fields still seed the next layer (dialog lines
from the MES corpus, shop counters).
