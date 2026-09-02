# VRChat world kit - Legend of Legaia scenes in Unity

Tooling + guide for turning `legaia-engine export-glb` output (a scene's
textured world, its NPCs, its animated props) into a **private** VRChat
world. Companion to the battle diorama transport in
[`../vrc-diorama/`](../vrc-diorama/README.md); full exporter reference:
[`docs/tooling/vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md).

**Legal first.** The exported `.glb` files contain Sony-derived geometry and
textures decoded from *your* disc. They are for your own local use: never
commit them, never redistribute them, and keep any VRChat upload **private**
(the default for a fresh world). This directory ships only from-scratch
tooling - scripts and a guide - no game data.

```
your disc ──legaia-extract──▶ extracted/ ──legaia-engine export-glb──▶ glb-export/town01/
                                                                          │ town01.glb  (world)
                                                                          │ npcs/*.glb  (animated)
                                                                          │ props/*.glb (windmill…)
                                                                          │ manifest.json
                                          Unity + glTFast + this kit ◀────┘
```

## Files

| File | Role |
|---|---|
| `world-project/Assets/LegaiaWorld/Editor/LegaiaWorldBuilder.cs` | Editor menu `Legaia > Build Scene From Manifest...`: instantiates the world, adds colliders, places NPCs + animated props, builds doorway-teleport triggers, wires proximity doors + the shoreline morph clip + the BGM loop, drops a spawn marker; also the **Equipment props** rack (the `--items` export placed as grabbable pickups near the spawn) and the camp props (below). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaCampProps.cs` | The **Camp props** pass: a pickup settings panel (world-space buttons - local music mute, synced day/night jumps) plus two carry-able torches and two campfires near spawn, all primitives + generated materials, in a top-level container outside the mirrored root (mirrored UI text would render backwards). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaAudioGen.cs` | Synthesized audio (fire crackle, day breeze + birds, night crickets - seamless loops, no disc audio) and the `VRC_SpatialAudioSource` compliance helper (the SDK deprecates bare AudioSources; 2D beds get the disabled component the SDK's own Auto Fix adds, spatial sources a configured one). |
| `world-project/Assets/LegaiaWorld/Editor/LegaiaRealism.cs` | The builder's "Realism enhancements" foldout: lit materials + generated normals + sun, day/night wiring + night doorway lamps, sky + fog, procedural grass, interior room shells, texture smoothing, synthesized ambience, wander wiring. Every pass defaults on; untick for the faithful look. |
| `world-project/Assets/LegaiaWorld/Editor/MiniJson.cs` | Dependency-free JSON reader for `manifest.json` (so the builder compiles in any project). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaLitVertexColor.shader` | Lit cutout stand-in for the exports' unlit materials: `COLOR_0` keeps modulating the texture, and lighting is the sign-independent two-sided Lambert `\|N.L\|` (the only stable answer over the mixed PSX winding - the header keeps the failed-flip history). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaLitVertexColorTransparent.shader` | The BLEND (water / light pool) sibling of the lit shader - alpha-blended, depth-write off. |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaGrassWind.shader` | Vertex-coloured wind sway for the procedural grass blades (sway weight in vertex alpha, world-position phase). |
| `world-project/Assets/LegaiaWorld/Shaders/LegaiaInteriorShell.shader` | Unlit black, front faces only: the interior-room dome, wound inward so it reads as black space from inside and is invisible (backface-culled) from outside. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoorway.cs` | UdonSharp doorway teleport: walking into the trigger repositions the local player at the landing marker with the authored arrival facing - the retail intra-scene door mechanism. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoor.cs` | UdonSharp proximity door: first approach plays the door's swing clip once and holds it open. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcWander.cs` | Optional UdonSharp stroll behaviour: an NPC wanders a small radius around its spawn between pauses - collision-aware (strolls clamp against walls, a waist-height ray stops a blocked walk, a downward ray follows the floor). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDayNight.cs` | Optional UdonSharp day/night cycle: sweeps the realism sun on a fixed cycle, synced across players via server time; night dims the trilight ambient + fog to a moonlit fraction, enables the night-lamp container, and crossfades the day/night ambience beds. `JumpToDay`/`JumpToNight` apply a synced offset (the settings panel's buttons). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaWorldMenu.cs` | The settings panel's behaviour: `ToggleMusic` (mutes the BGM locally - a personal preference), `SetDay`/`SetNight` (jump the shared cycle for everyone). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaTorch.cs` | Torch/campfire pickup: hold + Use toggles the flame container (fire + smoke particles, a Perlin-flickered point light - no glow orb) and a spatial crackle loop; `lit` is synced so a fire someone lights burns for everyone. Spawn-kinematic like the rack pickups. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaPickupProp.cs` | UdonSharp equipment-rack pickup: the prop spawns kinematic (frozen on the rack) and only becomes a free physics object the first time a player drops it - so a rack of dozens of bodies can't tunnel through the thin ground during world-load hitches. |

## Step 1 - export a scene

```bash
cargo build --release -p legaia-engine-shell
./target/release/legaia-engine export-glb --scene town01 --out glb-export
# or every scene that assembles:
./target/release/legaia-engine export-glb --all-scenes --out glb-export
# every equipment item (grabbable weapons and the rest):
./target/release/legaia-engine export-glb --items --out glb-export
```

Reads `extracted/` by default (`--disc "Legend of Legaia (USA).bin"` works
too). Per scene you get the world `.glb` (with the shoreline's baked
blendshape clip where the scene has one), `npcs/`, `props/`, `music/`
(the entry BGM as a seamless-loop WAV) and `manifest.json` (which now also
carries the doorway-teleport and door-tag data the builder wires below).
Default `--scale` is 1/128: one 128-unit walk tile becomes 1 m. (The
earlier 1/64 default read oversized in-headset - retail's field
proportions are generous, so "2 m per tile" made buildings loom over a
real-scale player.) `glb-export/` is gitignored -
it is Sony-derived output.

## Step 2 - a VRChat worlds project

1. Install the [VRChat Creator Companion](https://vcc.docs.vrchat.com/)
   (Windows; on Linux the community `vrc-get`/ALCOM tools do the same job)
   and create a **Worlds** project (Unity 2022.3).
2. Add **glTFast** for `.glb` import: `Window > Package Manager >
   + > Add package by name...` → `com.unity.cloud.gltfast` (a Unity-registry
   package - it never appears in the Creator Companion, which only manages
   VRChat's own packages; likewise the VRChat packages live in
   `Packages/vpm-manifest.json`, so don't be alarmed when
   `Packages/manifest.json` shows no `com.vrchat.*` entries). On the
   VRChat-mandated 2022.3.22f1 editor, pin version `6.14.1`: 6.15+ needs a
   newer 2022.3 patch (2022.3.67f2) and 6.19+ needs Unity 6, so the
   Package Manager refuses them. Equivalent to the UI route, one line in
   `Packages/manifest.json` under `"dependencies"` does the same job:

   ```json
   "com.unity.cloud.gltfast": "6.14.1",
   ```
3. Copy `world-project/Assets/LegaiaWorld/` into the project's `Assets/`.

The sibling diorama kit's
[`world-project/README.md`](../vrc-diorama/world-project/README.md) walks
the VCC setup in more detail if this is your first worlds project.

## Step 3 - import + build the scene

1. Copy an exported scene folder into the project, e.g.
   `Assets/LegaiaImports/town01/` (keep `npcs/`, `props/`,
   `manifest.json` beside the world glb). Let Unity import - glTFast picks
   up every `.glb`, and the baked NEAREST samplers keep the PSX
   point-sampled look without any material fixup.
2. Menu **Legaia > Build Scene From Manifest...**, browse to the copied
   `manifest.json`, and **Build scene**. Two default-on options worth
   knowing: **Match explorer orientation** mirrors the finished root on X
   so the scene reads the way the site's field-scene viewer presents it
   (the raw import genuinely is mirrored - see the troubleshooting entry
   for the landmark test that settled it), and **Merged + welded**
   builds one welded collision mesh for the whole world instead of a
   collider per mesh (closes the hairline seams between tile colliders a
   player capsule can slip through, and client builds stop needing
   Read/Write enabled on the glb). You get a `Legaia_town01` root:
   - `world` - the full map with its collider (ground included, so the
     retail walk surface is what you stand on). Semi-transparent submeshes
     are left OUT of collision - they represent light (window shafts,
     glow cones), and cooked solid a slanted shaft even acts as a
     walkable ramp - except ones shaped like a water sheet (large, flat,
     horizontal), so the sea keeps its floor. When the scene carries the
     baked shoreline morph clip (**Animate world morphs**) the sea edge
     washes in and out on a loop, and the exported BGM loops from an
     AudioSource on the root (**Scene music**);
   - `npcs/` - every catalogued villager at their MAN spawn tile, playing
     their retail spawn clip on a loop, capsule-collided;
   - `props/` - the animated placements (Rim Elm's windmills, doors, gate
     leaves) playing their bind clips. The world glb keeps a frame-0
     static twin under each one; the builder disables it by default
     (**Hide static prop twins**) so the pair doesn't z-fight. Instances
     the manifest tags as doors - plus every prop whose clip the manifest
     marks one-shot (`cyclic: false`: interior doors, cupboard doors,
     drawers - retail parks these held at spawn and only a script plays
     them, so looped they'd re-play their swing forever) - open **on
     approach** and stay open (**Doors open on approach**, via the
     `LegaiaDoor` behaviour). Only the props retail itself leaves
     free-running loop - the `cyclic` flag is the bind record's own
     spawn-pass verdict, which is what keeps the village windmill
     spinning (its clip ends 179° displaced, so any keyframe-shape test
     misreads it; the four-blade symmetry makes the loop seamless);
   - `teleports/` - one trigger volume per manifest doorway teleport
     (**Doorway teleports**): walk into a house door and you land in its
     interior with the authored facing, exactly the retail intra-scene
     door mechanism (`LegaiaDoorway`). Trigger boxes get an absolute
     player-sized floor (2.2 m across, player height) - the authored
     extents scale with the world, but the player doesn't, and a retail
     trigger hugging the door plane of a recessed entrance (Mei's house,
     the cave) is hard to clip at any smaller size; the wall absorbs the
     backward growth.
     A loop guard covers the retail landings that sit inside (or a
     capsule-width from) the paired door's trigger (town01's hilltop
     house lands inside its own exit band): before teleporting, the
     firing doorway suppresses every sibling doorway near the landing
     for a few seconds, so a landing can never chain-fire a ping-pong
     loop - which reads as "the door does nothing" when the hops cancel
     out. Stepping off a trigger re-arms it immediately (retail's own
     walk-away re-arm), so walking away and straight back always fires;
     Scene-exit portals also connect
     automatically when the target scene's built root (`Legaia_map01`,
     say) already exists in the same Unity scene;
   - `LegaiaSpawn` - the manifest's suggested spawn (the placed-object
     median, i.e. the village centre).

   A separate top-level `Legaia_camp_props` container (**Camp props**,
   default on) holds the settings panel and the torches/campfires - it
   sits outside the mirrored root on purpose (world-space UI text under
   the X-flip would render mirror-written). The panel is a pickup board:
   **Music On/Off** mutes the BGM for you alone, **Daytime**/**Nighttime**
   jump the shared day/night cycle for everyone (they do nothing until
   the realism pass builds the cycle). The two torches and two campfires
   are pickups too - hold one and press **Use** (left click / trigger)
   to light or snuff it: fire particles, a faint rising smoke plume, a
   warm point light with a Perlin fire flicker (no glow-orb mesh), and a
   spatial synthesized crackle loop; the lit state is synced, so a
   campfire someone lights burns for the whole instance. Every
   AudioSource the kit creates also carries the `VRC_SpatialAudioSource`
   component the SDK now expects (disabled on the flat 2D music/ambience
   beds, exactly what the SDK's Auto Fix does - the "2D audio source
   with no VRC Spatial Audio component" warning is gone).
3. Add the VRChat scene descriptor (`VRCWorld` prefab from the SDK) and
   point it at the spawn marker: select `VRCWorld`, expand the **VRC
   Scene Descriptor**'s **Spawns** list, and drag
   `Legaia_town01/LegaiaSpawn` into element 0 (replacing the prefab's own
   `Spawn` child - moving the marker does nothing until the list
   references it; with an *empty* list the descriptor's own transform is
   the spawn). Players face the spawn transform's +Z, so rotate the
   marker toward the view you want. Then `VRChat SDK > Show Control
   Panel > Build & Test`. Publish with **Upload** when it feels right - a
   fresh world is private until you explicitly publish it to Community
   Labs; leave it private.
4. Sanity checks: the sample scene's `GridFloor` plane sits near the
   origin - delete it (or keep it far below as a fall catch) so it can't
   mask the real walk surface; and after a build, `world` should carry
   one MeshCollider referencing
   `Assets/LegaiaGenerated/<scene>/world_collider.asset`.

## Making it feel alive

- **Wandering villagers**: add `LegaiaNpcWander` (UdonSharp) to an NPC the
  builder placed; tune radius/speed. Movement is computed per player -
  fine for ambience, use synced variables if everyone must agree.
- **Grabbable weapons**: `export-glb --items` exports **every equipment
  item** (all four characters' weapons, Ra-Seru, armour, headgear,
  footwear) to `glb-export/items/` - per record an item-alone `.glb`
  (grip repaired) and an exact-cut `.glb` with its host limb, both
  animated, plus a manifest of names and cut-honesty tags. Copy the
  `items/` folder into `Assets/`, point the builder's **Equipment props**
  section at its `manifest.json`, and **Place equipment rack near spawn**
  lines them up grounded on the world collider - one row per character,
  each prop scaled from raw PSX units by the scene's export scale times
  the **Size multiplier** (default 0.5: these are battle-mode models the
  field never shows, and at the raw battle-vs-field ratio they read
  comically large in hand),
  wrapped in a convex mesh collider cooked from its baked rest pose (a
  tight hull, not a bounding box; near-flat pieces fall back to a padded
  box), and wired as a `VRC Pickup` + `VRC Object Sync` physics pickup
  (static display without the SDK). Props spawn **frozen** on the rack
  (`LegaiaPickupProp` flips the body physical on first drop), so world
  load never scatters them or drops them through the thin ground mesh.
  **Weapons only** is the
  default filter; untick it to also rack armour, headgear, footwear and
  Ra-Seru. (The site's equipment viewer offers the same files per
  download.)
- **More clips**: every NPC glb carries *all* the scene-bundle clips whose
  bone count matches (`record_N` takes) - retarget the Animator the
  builder generated at any of them.

## Optional realism enhancements

The builder window's **Realism enhancements** foldout layers a set of
optional passes over the built root. Every pass defaults **on** - untick
them all for the faithful retail-shaded scene. Everything the passes
create is generated from scratch (shaders,
dome/grass geometry, synthesized audio): no game data is produced or
shipped beyond what the export already decoded. The **Apply enhancements to the
already-built root** button reruns just these passes over an existing
`Legaia_<scene>` root, so tuning a slider doesn't force a rebuild; each
pass is idempotent (it refreshes rather than stacks).

- **Realistic lighting**: the exported glbs are `KHR_materials_unlit` and
  carry **no normals**, so Unity lights can't touch them as imported. The
  pass duplicates every mesh into `Assets/LegaiaGenerated/<scene>/realism/`
  with smoothed, position-welded normals (sign-aligned - the PSX source
  winding is mixed, so raw face normals point both ways and would cancel;
  the lit shaders then light with the sign-independent two-sided Lambert
  `|N.L|`, since no per-vertex sign choice survives this data), swaps
  every material for `Legaia/Lit Vertex Color` (cutout or transparent by
  queue), and adds a warm directional sun with soft shadows plus a
  trilight ambient. The baked `COLOR_0` retail shading keeps modulating
  every surface, so the scene holds its palette - lighting layers on top
  instead of replacing it. NPC and prop materials additionally get
  **light wrap** (`_LightWrap`, slider "NPC/prop light wrap"): the
  `|N.L|` terminator cuts a harsh dark band right across a low-poly
  villager's face, so their angular term is flattened toward even
  lighting (shadow maps still attenuate); world surfaces keep the full
  directional response.
- **Day / night cycle** (under lighting): the `LegaiaDayNight` Udon
  behaviour sweeps the sun through a full day on a fixed cycle, with night
  compressed (`dayShare`). Every client derives the same angle from the
  shared server clock, so the cycle is synced with no networking events.
  Night genuinely darkens the landscape: the behaviour sweeps the trilight
  ambient (and fog colour) down to a moonlit, blue-shifted fraction of
  their daytime values ("Night darkness" slider, default 0.05 - walls and
  ground keep a little moonlight, not much; sun intensity alone leaves
  the ambient day-bright after sunset). **Night lamps** places a small
  warm light (no visible bulb mesh - the pool of light on the wall is
  the whole effect) at each village building **window**, anchored on
  the world mesh itself: the retail scene authors semi-transparent glow
  volumes exactly where light spills out of a hut window (town01 repeats
  one identically-sized glow object across three huts), so each
  village-side BLEND submesh of window-glow proportions anchors a
  tight-radius light. The glow volume is the light *shaft* angled down
  toward the ground - its centroid hangs in mid-air off the wall - so
  the lamp anchors on the shaft's own geometry: the centroid of its top
  band of vertices is the window opening, nudged slightly along the
  spill direction (a raycast wall-snap was tried first and grabbed
  unrelated nearby walls such as the palisade). Scenes with no authored
  glows fall back to a lamp above each village-side doorway (manifest
  teleport endpoints). The `night_lamps` container is enabled by the
  day/night behaviour only while the sun is below the horizon.
- **Sky + distance fog**: a procedural-skybox material (it tracks
  `RenderSettings.sun`, so with day/night on the sky darkens by itself)
  and linear fog scaled to the built root's bounds.
- **Ground foliage**: procedural grass - single-triangle blades in tufts,
  scattered over upward-facing world triangles whose ground colour reads
  green (texel x mean vertex colour at the triangle centre, the same
  product the retail shading displays). Ground only: a per-cell lowest
  upward-surface grid rejects any green triangle floating above other
  geometry, so tree canopies and roofs never sprout grass. Blades are tinted from the sampled
  ground so they blend with the terrain, and sway via `Legaia/Grass Wind`
  (weight in vertex alpha, world-position phase). Tune **density** and the
  **green threshold** (lower = more coverage, higher = keeps grass off
  paths); the scatter is deterministic per seed, capped at 25k tufts, and
  each rerun rescatters instead of stacking.
- **Interior room shells**: the doorway-teleport interiors are unused
  corners of the same map, so from inside a room you see the skybox above
  and the floating village past the doorway - retail frames these rooms
  against black. The pass detects each detached room from the manifest's
  own teleport data (endpoints beyond a spawn-distance threshold,
  clustered per room, then flood-filled outward to the whole building's
  meshes so the dome centres on the room, not on its doorway) and wraps
  it in a black ellipsoid dome fitted per-axis to the room's own geometry
  (a circumscribing sphere reached its half-diagonal in every direction
  and bled into neighbouring rooms), wound to face **inward only**:
  black space from inside, backface-culled (invisible) from outside,
  casting no shadow so the sun still lights the room. **Window light** adds a warm fill light per room
  so it reads window-lit inside its black surround.
- **Smooth textures**: bilinear + anisotropic filtering on every texture
  under the root, instead of the exports' PSX point sampling. This edits
  the imported texture objects in place, so a glb **reimport resets it** -
  rerun the pass after one.
- **Ambient audio beds**: three quiet synthesized loops on 2D sources
  (filtered noise + sines, loop-crossfaded, written to
  `LegaiaGenerated/`) - a wind/surf base that always plays, a daytime
  bed (breeze, leaf rustle, a few soft bird chirps) and a night bed
  (two interleaved cricket voices over a faint cool breeze). With the
  day/night cycle on, `LegaiaDayNight` crossfades day against night
  with the sun; without it the day bed simply stays up. Generated
  audio, not from the disc - the atmosphere holds even with the music
  muted from the settings panel.
- **Villagers wander**: wires `LegaiaNpcWander` on every talk-kind NPC
  from the manifest (matched by spawn position), so the town strolls
  instead of standing still. The behaviour is collision-aware: strolls are
  clamped against the world's colliders, a blocked walk re-picks instead
  of clipping through a hut, and a downward ray follows the floor.
  Movement is forward-only: a direction change pivots the whole body in
  place first, then steps off - an NPC never translates while mis-facing.
  Facing is **measured, not derived**: the exported rigs have no skins
  (each TMD object is a rigid mesh node) and the authored facing is baked
  into the node rest rotations themselves (the MAN placement has no
  facing byte), under a stack of mirrors and importer conversions that
  defeats sign-by-sign algebra. So at Start the behaviour picks the
  largest mesh node that rests upright (the torso), reads the direction
  it visibly faces off its `localToWorldMatrix` every walking frame
  (baked yaw, idle sway, every mirror included by construction), probes
  which way that visual forward responds to a transform yaw, and servos
  the yaw until the mesh faces the walk direction. The face-axis
  invariant (textured 4-view renders of every town01 model - wireframes
  cannot tell front from back) is **+Z in the glb scene frame at
  rest**; it is NOT a fixed node-local axis, because one rig family
  rests its nodes at -90 degrees with the vertices counter-rotated
  (npc_12 and kin, which walked sideways until the anchor's rest
  rotation was folded out of the measurement at Start). `flipFacing`
  covers a model violating the invariant; `facingYawOffset` adds a
  manual trim, and the realism foldout's **Facing overrides** field
  (`npc_30:90`, keys matching the NPC glb file name) applies such trims
  durably across rebuilds. Wall/floor ray heights are measured from the
  rendered model at Start, so they track any export scale.

Caveats: the sun / ambient / skybox / fog are **per-Unity-scene render
settings** - applying them from one built root is global, the last applied
root wins, and turning the options off later does not revert them (reset
via `Window > Rendering > Lighting`, and delete the root's `LegaiaSun` /
`foliage` / `interiors` / `ambience` / `night_lamps` children). The day/night and wander passes need the
VRChat SDK, same as doors and teleports. And the grass + realtime shadows
budget is a PC-world budget - trim density and shadow strength for a Quest
target.

## Troubleshooting

- **Everything looks mirrored**: it is - the raw import is X-mirrored
  relative to the site's field-scene viewer, and the builder's **Match
  explorer orientation** option (default on) mirrors the built root to
  compensate. This was settled *empirically* with a landmark test on
  town01, after deriving it from the shader reflection chain
  (`u_pair_front` in `site/js/webgl-shaders.js`) produced confident wrong
  answers in **both** directions: stand at the sea looking at the village
  (the sea-to-gate axis pins the viewpoint, so only parity can differ) -
  the raw glb puts the big terrace house left and the paired small huts
  right, the explorer page shows the opposite sides, and no rotation
  swaps sides across a content-pinned axis. Re-run that test rather than
  re-counting reflections. The double-sided merged collider keeps physics
  solid under the negative scale. A prop facing backward on a *different*
  importer is the separate handedness convention - flip `YAW_SIGN` in
  `LegaiaWorldBuilder.cs`.
- **Individual buildings face the wrong way** (windmill blades edge-on,
  a hut's door on the wrong side) while the overall layout is right: two
  separate causes, and each was fixed once - re-export with a current
  build and re-run a current builder before debugging further.
  1. *Yaw sign in the bake* (`legaia_asset::scene_gltf`): the site's
     `placementModelScaledY` has a transposed inline rotation block, and
     the bake once emitted the unnegated param, facing every yawed
     instance backwards while leaving every position (and any layout
     check) correct. Fixed at the node-quaternion emission; the manifest
     yaw flipped with it.
  2. *Handedness of placed props* (the builder): the world glb bakes the
     site's Y-mirror into its vertices (det -1) while NPC / prop glbs
     are proper-rotation models (root `Rx(180)`, det +1) - opposite
     chirality, so NO yaw value can align a placed prop with its baked
     frame-0 twin (a mirrored hut has its door on the wrong side at
     every angle; this is what made yaw-sign experiments look
     inconsistent). The builder supplies the missing mirror with a
     negative Z instance scale (`PROP_NPC_SCALE_Z`); with it, prop and
     twin coincide exactly, which is the invariant to check first
     (temporarily untick **Hide static prop twins**: each animated prop
     must z-fight its twin, not sit rotated against it).
- **Collider errors at build time**: with the default merged collider the
  cook runs off a generated readable asset and this doesn't arise; if you
  switched to per-mesh colliders, enable **Read/Write** in the glb's
  import inspector (Unity needs readable meshes to cook MeshColliders
  into a client build).
- **Falling through the floor**: use the default **Merged + welded**
  world collider. Two distinct causes it removes: per-mesh colliders leave
  hairline gaps where adjacent tile meshes meet (and silently vanish from
  client builds when the glb isn't Read/Write) - and, the big one, PhysX
  triangle meshes collide on the wound face only, while the PSX source
  data's winding is **mixed** (retail culled per-view via NCLIP; every
  renderer here draws double-sided, so it never shows). A single-sided
  collider therefore drops you through roughly half the floors. The
  merged collider appends every triangle reversed, so all geometry is
  solid from both sides.
- **Transparent surfaces look wrong**: PSX black-is-transparent bakes as
  alpha-0 with MASK (cutout) materials - correct for foliage and grates.
  Semi-transparent (ABE) prims - water sheets, light pools - split into a
  second `BLEND` material at half alpha (retail's dominant average blend
  mode); that also keeps the sea's stacked scroll layers from z-fighting,
  since blended materials skip depth writes. Additive/subtractive ABE
  modes flatten to the same alpha blend - nudge those few materials to
  Additive by hand where it matters.
- **Too big / too small**: re-export with a different `--scale`; the
  manifest records the scale used so the builder stays consistent.
- **A hand-placed NPC or prop is a giant**: current exports bake the
  scale onto each NPC/prop glb's root node
  (`conventions.npc_prop_units: "scaled"`), so a dragged-in file is
  world-sized as-is - re-export if yours predates that. On an older
  raw-PSX-units export, set `localScale = manifest scale` per instance
  (the builder detects the flag and does the right thing either way).
- **A scene looks empty**: world-map scenes (`deele1`…) have no MAN NPCs,
  and some scenes are cutscene-only shells; `export-glb --all-scenes`
  reports what each scene yielded.
- **Doors don't open / doorway teleports do nothing**: the `LegaiaDoorway`
  and `LegaiaDoor` behaviours need the VRChat worlds SDK (UdonSharp) in
  the project - without it the builder logs one warning per object and
  leaves the trigger inert. They also only react to *players*, so test in
  Build & Test / ClientSim, not by flying the editor scene camera through
  them. A door that opens but never teleports is the separate case of a
  scene-exit portal whose target scene isn't built in this Unity scene -
  the builder wires those only when `Legaia_<target>` exists.
- **Doors swing on a loop / teleports silently no-op after an older
  build**: both were builder defects - door tagging keyed on a join that
  missed every house door (fixed in the exporter: re-export the scene so
  the manifest's `is_door` flags are current), and Udon field values were
  set on the U# proxy without `CopyProxyToUdon`, so the backing behaviour
  kept null defaults. Rebuild with the current kit **and** a current
  manifest; the builder offers to replace a stale `Legaia_<scene>` root.
  Interior doors, cupboards and drawers looping their opening is the
  same symptom one layer deeper: those stand near no teleport or portal,
  so no proximity join can ever tag them - the manifest's per-prop
  `cyclic` flag is what routes them to the approach-open path, and it
  too needs a current manifest.
- **The windmill doesn't spin**: an older manifest judged `cyclic` by
  clip shape (last keyframe returns to the first), and the windmill's
  spin ends ~179° displaced - so it was mis-filed as a one-shot and
  frozen at frame 0. The flag is now the bind record's own retail
  verdict (an empty spawn pass keeps the actor's looping template
  flags; a door's reset-hold parks it) - re-export the scene and
  rebuild.
- **"Unable to find valid U# program asset associated with script"**:
  UdonSharp only auto-creates program assets for scripts made through its
  own Create menu, so the kit's bare `.cs` files have none and U# refuses
  to attach them (an older builder let that exception abort the whole
  build - nothing after the first door got wired). The builder now
  creates the missing `UdonSharpProgramAsset`s next to the scripts,
  resets U#'s lookup cache and compiles before wiring; each failed wire
  is also contained to its own object instead of ending the build.
- **The shoreline doesn't move**: the morph clip lives in the world glb -
  re-export with a current build (the manifest should carry `world_anim`),
  leave **Animate world morphs** on, and check the glTFast import kept
  animations enabled (the glb's import inspector, Animation tab).

## Faithful vs. approximated

The world geometry, textures, packet-colour shading, placement transforms,
floor heights, walk surface, doorway trigger/landing data, shoreline morph
deltas and the BGM render are the engine's own retail-parity kernels
(`engine-core::scene_assembly` and friends - the same code the site's
field-scene viewer renders). What is approximated: NPCs loop their spawn
clip instead of running their field-VM scripts, non-door props free-run
instead of waiting for script triggers, a door's full record choreography
collapses to open-on-approach + teleport, script-door arms are frozen at
the cold-entry story-flag state, and the shoreline's arming cadence is the
engine's scene-entry pulse enhancement. Dialog lines from the MES corpus
and shop counters remain the natural next Udon layer - the manifest
already carries each NPC's kind (`talk`/`door`/`prop`) and dialog first
line to seed it.

The realism foldout sits entirely on the *enhancement* side of this line:
lighting, sky, grass, shells, ambience and wander are deliberate
departures from retail, each its own toggle. Every pass ships enabled by
default - the project's ship-the-better-experience-by-default policy -
and unticking them all restores the faithful retail-shaded build, one
toggle away, same as the engine's own knobs. (The interior
shells are the one pass that *restores* retail framing: those rooms sit
against black space in the real game.)
