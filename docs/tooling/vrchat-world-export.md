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
legaia-engine export-glb --party --out glb-export              # Vahn, Noa, Gala field forms
```

Reads `extracted/` (default) or `--disc <image.bin>`. Flags: `--scale`
(glTF meters per PSX world unit, default 1/128 - one 128-unit walk tile
= 1 m; a larger 1/64 "2 m tile" reads oversized in a headset against real
player scale, retail's field proportions being generous;
equipment-item glbs are raw PSX units regardless, scaled at placement
time by the Unity builder), `--include-sky` (keep the sky-backdrop
shells the site viewers hide), `--no-npcs`, `--no-props`, `--items` (the
equipment export below - standalone or combined with scenes), `--party`
(the party's field forms below - likewise standalone).
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

- **World glb** - `engine-world-views::scene_staging::assemble_field_scene` (the
  field-scene page's build): ground heightfield (with the shared
  `GROUND_SINK`, minus the steep triangles of sloped far-bucket cells -
  see `field_ground::flat_refs`; a glb has no ordering table to draw them
  under the cliff that hides them in retail, and a triangle rising more
  than a cell's 128-unit run is a stray sheet, not a ramp. The filter is
  per triangle because such a cell is often one vertical half over one
  flat half that is real floor), terrain tiles, and
  placed objects with coplanar lifts
  applied, each instanced at its resolved `.MAP` transform. Placements
  whose object bind names a clip are posed at **frame 0** (the native
  play-window's static bake). Sky shells are dropped by the site's
  `isSkyMesh` heuristic unless `--include-sky`. Baking is
  `legaia_actor_formats::scene_gltf`: one VRAM-derived RGBA atlas, NEAREST
  samplers, packet colours on `COLOR_0` (sRGB-linearized so a
  linear-colour-space importer - glTFast under VRChat's mandated Linear
  setting included - reproduces retail's display-space `texel * colour/128`
  product; a project left in Gamma colour space will render these models
  darker than intended), PSX word-0 transparency as MASK cutout. ABE
  semi-transparent prims split into one `BLEND` material **per PSX ABR
  blend rate**, named `legaia_semi_abr<N>` (0 = `B/2 + F/2`, 1 = `B + F`
  additive, 2 = `B - F`, 3 = `B + F/4`): core glTF can only express alpha
  blending, so the alpha only approximates the rate (0.5, or 0.25 for
  rate 3) and the **name is the contract** an importer restores the real
  blend state from - the Unity kit maps rates 1/2/3 onto its additive
  shader, since alpha-blending an additive light shaft reads as a grey
  film instead of a glow. Rate 0 is alpha blending exactly, and the
  blended queue's depth-write skip is also what keeps retail's coincident
  water scroll layers from z-fighting in depth-buffered engines.
- **NPC glbs** - `engine-scene::npc_catalog` (the MAN partition-1 placement
  walk shared with the NPC browser page) + `legaia_actor_formats::character_gltf`:
  the scene TMD with its spawn clip first and every other
  bone-count-matching scene-ANM record baked as a named glTF animation
  (`record_N`). Spawn transforms live in the manifest, not the file.
- **Animated-prop glbs** - each distinct `(env mesh, bind clip)` placement
  pair whose clip has more than one frame, exported object-local with the
  clip, plus every placement transform in the manifest. The world glb
  keeps their frame-0 static twins, so these are opt-in upgrades.
- **Baked ambient vertex morphs** - when the scene arms the engine's
  scene-entry VDF pulse (`engine-field::vdf_pulse` - opt-in per scene:
  the jou family and Rim Elm's shoreline, the flagship), the world glb carries one glTF **morph target** per envelope
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
viewer runs** - `legaia_actor_formats::battle_char_assembly::loadout`, hoisted out
of the wasm session so both hosts share one implementation - and the
manifest repeats each record's honesty tags (`class`, `complete`,
`isolation_mode`, `curated`, `grip_bridges`) so a heuristic cut is
distinguishable from a curated one. `SCUS_942.54` supplies display names
and section labels when readable; without it, ids stand in. Unlike the
scene NPC / prop glbs (which bake the export scale onto their root),
item glbs ship in **raw PSX units** - matching the site's character
downloads they share a kernel with.

## `--party`: the party's field forms

`--party` exports Vahn, Noa and Gala **as they walk the towns**: the
PROT 0874 §0 field mesh of each active-party slot (retail's 10-live-group
cap applied, no equipment swap - the disc-form groups already are the cold
new-game look, and the swap would cost Vahn his head - [`character-mesh.md`](../formats/character-mesh.md)),
textured from the §2 field pages, with the character's own 7-clip
locomotion bank ([`anm.md`](../formats/anm.md)) baked as named takes:
`Idle` first (frame 0 = the rest pose), `Walk` second, the unpinned rest
as `Locomotion N`. Files land as `<out>/party/npcs/<vahn|noa|gala>.glb`
with the export scale on their root node, exactly like a scene's villager
glbs, and `party/manifest.json` carries an `npcs[]` roster in the same
shape as a scene manifest (`model_index` = the pack slot, `label` and
`name` = the character, `clips`, plus `walk_clip` naming the walk take)
so a consumer that resolves villagers from other scenes resolves these
the same way - the Unity kit's `add_npcs` rule takes `{"scene": "party",
"model": 1}` and Noa joins the living town. Same kernel as the site's
characters-page field-form download (`web-viewer::character`).

## Manifest

`manifest.json` carries: the scale and axis conventions (spelled out under
`conventions`), a suggested `spawn` (component-wise median of the placed
objects - the village centre, not the map-grid centre), composition
`stats`, and three placement arrays:

- `npcs[]` - file, kind (`talk` / `door` / `prop`), the dialog first line
  as a label, position (floor height via the retail sampler
  `World::sample_field_floor_height`), clip names, `conditional` (parked
  off-map until a script places it), `model_index` / `anim_id`. The
  position is **not** the raw placement-header tile: retail's installer
  pre-runs each record's spawn prologue, and a flag-branched prologue's
  taken arm teleports the actor before the first frame draws - so the
  catalog resolves that arm under cold fresh-game flags
  (`man_field_scripts::placement_spawn_relocation`). Reading the header
  tile alone stacks a flag-dispatched spawn on whatever actor is authored
  at its staging square (town01's running kids sit exactly on the
  standing kids' tiles) and draws dev records retail parks; the resolved
  roster is pinned against a live fresh-game capture's actor list;
- `doors[]` - portal placements with their field-VM target map;
- `animated_props[]` - file, clip frame count, a per-prop `cyclic` flag
  (does retail leave this clip **free-running**? The verdict comes from
  the placement's bind record: every actor is born with looping playback
  flags, and the spawn pass either parks the clip held - a door's
  `0x4C 0x35` reset-hold, so a builder should trigger it once on approach;
  retail advances those only while the bind script runs - or leaves the
  template flags alone, the windmill, whose sails spin forever under the
  unconditional anim tick (`legaia_engine_scene::field_env::prop_spawn_free_runs`).
  A keyframe-shape test is only the no-bind fallback: the windmill's
  0..179-degree spin ends displaced from frame 0 and its four-blade
  symmetry is what makes the loop seamless, so shape alone misreads
  retail's one always-running prop as a one-shot),
  and per-instance position + rotation plus two door tags: `is_door` (the
  instance stands on a doorway-teleport trigger - retail parks a door
  placement on its own doorway tile, so its clip is the door record's
  swing, meant to open on approach rather than loop; the tag covers both
  teleport families, where a script-tile anchor join structurally misses
  every map door) and `near_portal` (the instance stands on a scene-exit
  band - a gate leaf); the door tags catch exterior doors, `cyclic`
  catches the one-shots standing near no teleport at all;
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

Transforms are in the **export frame**: a PSX point `(x, y, z)` exports as
`(x, -y, -z)`, a 180-degree turn about X with determinant +1, already
multiplied by `scale`. PSX is X right, Y down, Z into the screen, which is
right-handed, and glTF is right-handed with Y up, so a rotation is the whole
conversion and a raw glb has the game's own layout: stand at the eye retail's
camera uses, look where it looks, and a glTF viewer draws what retail draws.

The world glb, the NPC and prop files (root node `Rx(180)`, `scale` baked on it,
`conventions.npc_prop_units: "scaled"`) and every number in the manifest share
this frame, so a placement needs no compensating mirror. The vertices are baked
in it; the props and NPCs reach it through their root node. Rotations carry all
three authored placement angles: each world-glb instance node and each manifest
prop instance (`rotation_xyzw`) holds the record's `Rx * Ry * Rz` composition
(`EnvDraw::rotate`, the kernel the play hosts place with) conjugated into the
frame, so a pitched or rolled record (a leaning plank, a tilted sign, a video
wall) keeps its tilt. A consumer applies `position`, then `rotation_xyzw`, to a
prop file and lands on its baked frame-0 twin. The legacy `rot_y_radians` is the
same rotation's yaw alone, a plain rotation about +Y, exact only for a record
with no pitch or roll. It is the authored retail yaw with its sign turned: the
export frame sees the ground from the far side of Z. `facing_dir` and `entry_xz`
are `[x, z]` pairs in the same frame.

The baker takes [`SceneInstance`](../../crates/actor-formats/src/scene_gltf.rs)
in the site pages' **render frame** (world Y negated, nothing else) and maps it
over in one place (`scene_gltf::render_to_export_position` /
`render_to_export_rotation`), which is why the page downloads, the Explorer's
model export and `export-glb` agree. `glb_export::export_translation` and
`export_rotation` are the same maps for the manifest.

`conventions.world_frame: "psx_rx180"` marks this frame. A manifest without it
is an older export whose world glb was the mirror image of the game, `(x, -y,
z)`, with NPC and prop files in the frame above. The Unity kit still builds
those as they always built; see [its frame note](../../scripts/vrchat-world/README.md#the-kits-frame).

### Winding

`SceneMesh::indices` keep retail's order, and a field mesh's front face runs
clockwise on the Y-down screen (the positive `NCLIP` retail's handlers keep,
[`renderer.md`](../subsystems/renderer.md#the-field-pass-culls-back-faces)).
glTF's front face runs counter-clockwise and a rotation leaves on-screen order
alone, so the bake reverses every triangle. A viewer that honours winding then
keeps what retail's cull keeps: town01 culled by glTF's rule from three retail
cameras draws the same picture as the double-sided render.

The walk-ground heightfield is the part with no retail order to keep. It is
engine-synthesised and its builder winds opposite to the disc meshes, so taken
raw, `hf.indices` would ship every ground triangle facing down and read as a
source whose winding is mixed. `world.rs` reverses it the way
`field_ground::render_indices` does, and the page exports read
`*_ground_render_indices`. By surface area, 95% of town01's faces point up and
99.8% of map01's.

What still points down is authored, and is not a mistake to correct: TMD prims
carry no double-sided flag and no normal. Sidedness is the winding plus a colour
word bit (`0x08000000`) no placed object sets. A face wound downward is an
underside or an interior surface retail never draws from above, and a wall
retail shows from both sides is two prims over one vertex triple
(`mark_double_sided_pairs`). So materials stay `doubleSided: true`: retail's
camera never walks around a building, a VR player does, and those back faces are
what stands between them and an open shell. No `NORMAL` stream ships; a consumer
that wants one derives it from the winding.

### The landmark test

Handedness questions here are settled by looking, not by counting reflections.
Counting through a shader chain (`u_pair_front` in `site/js/webgl-shaders.js`)
gave confident wrong answers in both directions.

Take a retail field state with a lopsided frame: `town01_field_card_boot` (the
cave mouth is on the left) or `town01_tetsu_topic_prompt` (the rock is on the
left, the shrine on the right of the wall). Its `retail-compare` report line has
the camera: pitch, yaw, `H`, the eye trio and the focus. The eye sits at
`focus - R^T * (trio / 6)`, with `R = Rx(pitch) * Ry(yaw)` and the field's 6x
base matrix ([`renderer.md`](../subsystems/renderer.md#the-field-view-matrix-where-tr-comes-from)),
and the view direction is `R`'s third row. Put both through the frame map, take
the up vector as `-Y` of `R`, and render the raw glb with a standard look-at
(`right = forward x up`), the camera any glTF viewer builds. There is no parity
choice left to make, so a mirrored file cannot pass. Compare the picture with
retail's frame and with its left-right flip: the export must score closer
unflipped, and a mirrored file scores closer flipped.

## Verification

The handedness is pinned without disc bytes in
`crates/engine-export/src/glb_export/handedness_tests.rs` and
`scene_gltf`'s own tests: a synthetic pitched, rolled and yawed record's
world vertices land on `EnvDraw::rotate` through the frame map; the prop file
placed from the manifest's numbers lands on the same points with no mirror
anywhere; the bake's determinant is +1; a triangle retail's `NCLIP` keeps is
front-facing by glTF's rule.

Disc-gated oracle `crates/engine-export/tests/glb_export_real.rs`: assembles
town01, bakes all three families, validates every glb container (magic,
length, JSON chunk), asserts the sky filter, the bind-resolve that feeds
the prop split, clip counts against manifest cross-references, both
doorway-teleport families plus the map01 gate portal, the door tagging,
the shoreline morph targets + `vdf_pulse` animation, the ground's up-facing
winding, and every animated prop instance against its world twin node. Skip-passes
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

On the deliberate-enhancement side, the kit's builder carries a **realism**
foldout (lit vertex-colour materials with generated normals + a sun,
day/night cycle, sky + fog, procedural grass, black interior-room shells
with window light, texture smoothing, synthesized ambience, wandering
villagers) plus an **equipment rack** placer that stands the `--items`
export near the spawn as grabbable pickups. Every realism pass defaults
on - unticking them all restores the faithful retail-shaded build - and
everything is generated from scratch on top of the export, documented in
[`scripts/vrchat-world/README.md`](../../scripts/vrchat-world/README.md#optional-realism-enhancements).

The same builder also spawns the furniture most VRChat worlds end up
with - a mirror (off by default, local, auto-off), a synced video TV
over the SDK's AVPro + Unity players, a card table with sittable stools
and a 52-card pickup deck (hold'em by day, a five-card "poker night" run
by a villager after dusk, blackjack), posters hung on the town's own
walls from a settings list - all built from primitives, generated
textures and SDK components, plus a slot list for any prefab asset
already installed in the project (the SDK's pen system, QvPen, ProTV,
a community deck). Guide + the community-prefab survey with licences:
[`scripts/vrchat-world/README.md`](../../scripts/vrchat-world/README.md#common-prefabs).

Hand-tuned corrections a specific scene needs (delete a generated object,
keep listed NPCs from wandering, drop an NPC, override the spawn, aim the
VRC scene descriptor at the spawn marker) live in a per-scene kit file,
`Assets/LegaiaWorld/Settings/<scene>.settings.json` - authored tuning
data, no game content - documented in
[`scripts/vrchat-world/README.md`](../../scripts/vrchat-world/README.md#per-scene-settings).

## The playable slot machine

The kit also builds the casino slot minigame as an in-world cabinet, on a
separate feed from `export-glb`: `asset slot-art` decodes the slot overlay
(PROT 0975) + art pack (PROT 1200) into PNGs and a manifest carrying the
payout table, the disc's scene-geometry tables and the retail projection
constants ([`extraction.md`](extraction.md#sub-asset-extraction-asset)).
An editor menu composes the whole retail screen - reel drums re-derived
per frame, glass furniture, dot-matrix marquee, paytable - directly onto a
cabinet mesh's screen face through the retail software projection, and an
UdonSharp port of the engine's rules kernel
([`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md))
drives it, verified against a committed rules-parity fixture generated by
`crates/engine-minigames/tests/slot_verify_fixture.rs`. The cabinet mesh is the
builder's one bring-your-own input (any model with a flat screen face
works; without one, the screen is hand-placed and fallback buttons are
generated). Guide + faithful-vs-approximated notes:
[`scripts/vrchat-world/README.md`](../../scripts/vrchat-world/README.md#the-casino-slot-machine).
