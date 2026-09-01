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
| `world-project/Assets/LegaiaWorld/Editor/LegaiaWorldBuilder.cs` | Editor menu `Legaia > Build Scene From Manifest...`: instantiates the world, adds colliders, places NPCs + animated props, builds doorway-teleport triggers, wires proximity doors + the shoreline morph clip + the BGM loop, drops a spawn marker. |
| `world-project/Assets/LegaiaWorld/Editor/MiniJson.cs` | Dependency-free JSON reader for `manifest.json` (so the builder compiles in any project). |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoorway.cs` | UdonSharp doorway teleport: walking into the trigger repositions the local player at the landing marker with the authored arrival facing - the retail intra-scene door mechanism. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaDoor.cs` | UdonSharp proximity door: first approach plays the door's swing clip once and holds it open. |
| `world-project/Assets/LegaiaWorld/Udon/LegaiaNpcWander.cs` | Optional UdonSharp stroll behaviour: an NPC wanders a small radius around its spawn between pauses. |

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
Default `--scale` is 1/64: one 128-unit walk tile becomes 2 m, which lands
doorways and NPCs near VRChat human scale. `glb-export/` is gitignored -
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
     retail walk surface is what you stand on). When the scene carries the
     baked shoreline morph clip (**Animate world morphs**) the sea edge
     washes in and out on a loop, and the exported BGM loops from an
     AudioSource on the root (**Scene music**);
   - `npcs/` - every catalogued villager at their MAN spawn tile, playing
     their retail spawn clip on a loop, capsule-collided;
   - `props/` - the animated placements (Rim Elm's windmills, doors, gate
     leaves) playing their bind clips. The world glb keeps a frame-0
     static twin under each one; the builder disables it by default
     (**Hide static prop twins**) so the pair doesn't z-fight. Instances
     the manifest tags as doors open **on approach** and stay open
     (**Doors open on approach**, via the `LegaiaDoor` behaviour) instead
     of looping their swing;
   - `teleports/` - one trigger volume per manifest doorway teleport
     (**Doorway teleports**): walk into a house door and you land in its
     interior with the authored facing, exactly the retail intra-scene
     door mechanism (`LegaiaDoorway`). Scene-exit portals also connect
     automatically when the target scene's built root (`Legaia_map01`,
     say) already exists in the same Unity scene;
   - `LegaiaSpawn` - the manifest's suggested spawn (the placed-object
     median, i.e. the village centre).
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
  animated, plus a manifest of names and cut-honesty tags. Import one the
  same way, then add a `VRC Pickup` + a collider to hold Vahn's sword.
  (The site's equipment viewer offers the same files per download, and
  the characters/bestiary pages export characters and monsters with
  their full animation banks.) Item glbs are raw PSX units - scale them
  like NPCs.
- **More clips**: every NPC glb carries *all* the scene-bundle clips whose
  bone count matches (`record_N` takes) - retarget the Animator the
  builder generated at any of them.

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
