# VRChat world kit - Legend of Legaia scenes in Unity

Tooling + guide for turning `legaia-engine export-glb` output (a scene's
textured world, its NPCs, its animated props) into a **private** VRChat
world. Companion to the battle diorama transport in
[`../vrc-diorama/`](../vrc-diorama/README.md); full exporter reference:
[`docs/tooling/vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md).

**Legal first.** The exported `.glb` files contain Sony-derived geometry and
textures decoded from *your* disc. They are for your own local use: never
commit them, never redistribute them, and keep any VRChat upload **private**
(the default for a fresh world). This directory ships only clean-room
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
   + > Add package by name...` → `com.unity.cloud.gltfast`.
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
   `manifest.json`, and **Build scene**. You get a `Legaia_town01` root:
   - `world` - the full map with a MeshCollider per mesh (ground included,
     so the retail walk surface is what you stand on);
   - `npcs/` - every catalogued villager at their MAN spawn tile, playing
     their retail spawn clip on a loop, capsule-collided;
   - `props/` - the animated placements (Rim Elm's windmills and doors)
     playing their bind clips; the world keeps a frame-0 static twin
     underneath, so delete whichever of the pair you don't want moving
     (doors especially - a looping door clip swings forever);
   - `LegaiaSpawn` - the manifest's suggested spawn (the placed-object
     median, i.e. the village centre).
3. Add the VRChat scene descriptor (`VRCWorld` prefab from the SDK), move
   its spawn to `LegaiaSpawn`, then `VRChat SDK > Show Control Panel >
   Build & Test`. Publish with **Upload** when it feels right - a fresh
   world is private until you explicitly publish it to Community Labs;
   leave it private.

## Making it feel alive

- **Wandering villagers**: add `LegaiaNpcWander` (UdonSharp) to an NPC the
  builder placed; tune radius/speed. Movement is computed per player -
  fine for ambience, use synced variables if everyone must agree.
- **Grabbable weapons**: the site's equipment viewer downloads any weapon
  as an item-alone `.glb` (and the characters/bestiary pages export
  characters and monsters with their full animation banks). Import the
  same way, then add a `VRC Pickup` + a collider to hold Vahn's sword.
- **More clips**: every NPC glb carries *all* the scene-bundle clips whose
  bone count matches (`record_N` takes) - retarget the Animator the
  builder generated at any of them.

## Troubleshooting

- **Everything is mirrored / a prop faces backward**: glTF is
  right-handed, Unity left-handed; glTFast converts by inverting X and the
  builder maps manifest transforms through the same inversion. If your
  importer differs, flip `YAW_SIGN` in `LegaiaWorldBuilder.cs`.
- **Collider errors at build time**: enable **Read/Write** in the glb's
  import inspector (Unity needs readable meshes to cook MeshColliders
  into a client build).
- **Transparent surfaces look wrong**: PSX black-is-transparent bakes as
  alpha-0 with MASK (cutout) materials - correct for foliage and grates.
  Semi-transparent water sheets export with their blend metadata but Unity
  picks one blend mode per material; nudge the material to Fade/Additive
  where it matters.
- **Too big / too small**: re-export with a different `--scale`; the
  manifest records the scale used so the builder stays consistent.
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
