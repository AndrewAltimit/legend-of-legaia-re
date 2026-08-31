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
| `world-project/Assets/LegaiaWorld/Editor/LegaiaWorldBuilder.cs` | Editor menu `Legaia > Build Scene From Manifest...`: instantiates the world, adds colliders, places NPCs + animated props from the manifest, wires looping Animators, drops a spawn marker. |
| `world-project/Assets/LegaiaWorld/Editor/MiniJson.cs` | Dependency-free JSON reader for `manifest.json` (so the builder compiles in any project). |
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
too). Per scene you get the world `.glb`, `npcs/`, `props/`, and
`manifest.json`. Default `--scale` is 1/64: one 128-unit walk tile becomes
2 m, which lands doorways and NPCs near VRChat human scale. `glb-export/`
is gitignored - it is Sony-derived output.

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
     retail walk surface is what you stand on);
   - `npcs/` - every catalogued villager at their MAN spawn tile, playing
     their retail spawn clip on a loop, capsule-collided;
   - `props/` - the animated placements (Rim Elm's windmills and doors)
     playing their bind clips. The world glb keeps a frame-0 static twin
     under each one; the builder disables it by default (**Hide static
     prop twins**) so the pair doesn't z-fight - for a prop you'd rather
     have still (doors especially - a looping door clip swings forever),
     delete the animated instance and re-enable its twin;
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
- **A hand-placed NPC or prop is a giant**: the NPC/prop `.glb`s ship in
  raw PSX units (like the site's downloads) while manifest *positions*
  are pre-scaled - the builder sets each instance's scale from the
  manifest; do the same (`localScale = manifest scale`) when placing one
  by hand.
- **A scene looks empty**: world-map scenes (`deele1`…) have no MAN NPCs,
  and some scenes are cutscene-only shells; `export-glb --all-scenes`
  reports what each scene yielded.

## Faithful vs. approximated

The world geometry, textures, packet-colour shading, placement transforms,
floor heights and walk surface are the engine's own retail-parity kernels
(`engine-core::scene_assembly` - the same code the site's field-scene
viewer renders). What is approximated: NPCs loop their spawn clip instead
of running their field-VM scripts, animated props free-run instead of
waiting for script triggers, and door meshes don't warp anywhere. Porting
those behaviours to Udon (doors that open on approach, dialog lines from
the MES corpus, shop counters) is the natural next layer - the manifest
already carries each NPC's kind (`talk`/`door`/`prop`), dialog first line,
and door target map to seed it.
