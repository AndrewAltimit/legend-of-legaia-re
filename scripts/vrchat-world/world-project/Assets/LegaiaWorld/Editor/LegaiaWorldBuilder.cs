// Legaia world builder: assembles a Unity scene from one `legaia-engine
// export-glb` output folder (world glb + npcs/ + props/ + manifest.json)
// after glTFast has imported the .glb files.
//
// Usage (see scripts/vrchat-world/README.md for the full pipeline):
//   1. copy an exported scene folder into Assets/ (e.g.
//      Assets/LegaiaImports/town01/) and let Unity + glTFast import it;
//   2. menu "Legaia > Build Scene From Manifest...", pick the folder's
//      manifest.json;
//   3. the builder instantiates the world at the origin, adds mesh
//      colliders, places every NPC and animated prop from the manifest with
//      a looping Animator on its spawn clip, and drops a "LegaiaSpawn"
//      marker at the manifest's suggested spawn.
//
// The "Realism enhancements" foldout layers optional passes over the built
// root (lit materials + sun, day/night, sky + fog, grass, interior room
// shells, texture smoothing, ambience, wandering villagers) - see
// LegaiaRealism.cs. The graphics passes default ON; untick them for the
// faithful retail-shaded build.
//
// Coordinate note: glTFast converts glTF's right-handed frame to Unity's
// left-handed one by inverting X, so manifest positions (glTF frame) are
// mapped through the same inversion here, and yaw flips sign. If a prop
// faces mirrored in your importer, flip YAW_SIGN.
//
// Handedness note (the trap that ate three yaw-sign attempts): the WORLD
// glb bakes the site viewers' Y-mirror into its vertices (determinant -1
// geometry), while the NPC / prop glbs are proper-rotation models (root
// node Rx(180deg), determinant +1, raw PSX units). Opposite handedness
// means NO yaw value can make a placed prop coincide with its baked
// frame-0 twin - a mirrored building has its door on the wrong side at
// every angle. The fix is PROP_NPC_SCALE_Z = -1: a negative Z on each
// instance's scale supplies the missing mirror (algebra: the required
// instance transform is Ry(yaw) * diag(1,-1,1) * Rx(pi)^-1
// = Ry(yaw) * diag(1,1,-1)). Materials are exported double-sided, so the
// flipped winding doesn't cull.
//
// Orientation note: the raw import IS mirrored relative to the site's
// field-scene viewer, and the fix is a -1 X root scale. This was settled
// EMPIRICALLY with a landmark test on town01 - stand at the sea looking at
// the village (the sea-to-gate axis pins the viewpoint, so only parity can
// differ): the raw glb shows the big house left / huts right / gate
// slightly right of the animated gate doors' wall, the explorer shows the
// exact opposite sides. No rotation swaps sides across a content-pinned
// axis. Do NOT re-derive this from the shader reflection chain
// (webgl-shaders.js u_pair_front) - counting reflections there produced
// confident wrong answers in BOTH directions before the landmark test
// settled it. The double-sided merged collider keeps physics correct under
// the negative scale, and the shared material is double-sided so the
// flipped winding doesn't cull.

using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Animations;
using UnityEngine;

namespace LegaiaWorld
{
    public class LegaiaWorldBuilder : EditorWindow
    {
        const float YAW_SIGN = -1f;

        // NPC / prop instances need a Z-mirror to match the world glb's
        // baked (mirror-handed) geometry - see the handedness note in the
        // header. Set to +1 only for an importer whose world glb is
        // handedness-corrected on import.
        const float PROP_NPC_SCALE_Z = -1f;

        string manifestPath = "";
        bool matchExplorerOrientation = true;
        bool addWorldColliders = true;
        bool mergedWorldCollider = true;
        bool addNpcCapsules = true;
        bool loopNpcClips = true;
        bool loopPropClips = true;
        bool hideStaticPropTwins = true;
        bool includeConditionalNpcs = false;
        bool wireTeleports = true;
        bool openDoorsOnApproach = true;
        bool playWorldMorphClip = true;
        bool addMusic = true;
        float musicVolume = 0.5f;

        // Optional realism layer (LegaiaRealism.cs) - every pass defaults
        // on; untick everything in the foldout for the faithful
        // retail-shaded scene.
        LegaiaRealismOptions realism = new LegaiaRealismOptions();
        bool showRealism = true;
        Vector2 scroll;

        // Equipment props: place the `export-glb --items` weapons as
        // (grabbable) props near the spawn. The size multiplier sits on
        // top of the scene's export scale: these are BATTLE-mode models,
        // authored against battle proportions the field never shows, and
        // at 1:1 with the field scale they read comically large in hand -
        // half the world scale lands them at believable prop size.
        string itemsManifestPath = "";
        bool equipWeaponsOnly = true;
        bool equipPickups = true;
        float equipSizeMult = 0.5f;

        [MenuItem("Legaia/Build Scene From Manifest...")]
        static void Open()
        {
            GetWindow<LegaiaWorldBuilder>("Legaia World Builder");
        }

        void OnGUI()
        {
            scroll = EditorGUILayout.BeginScrollView(scroll);
            GUILayout.Label("Exported scene manifest", EditorStyles.boldLabel);
            EditorGUILayout.BeginHorizontal();
            manifestPath = EditorGUILayout.TextField(manifestPath);
            if (GUILayout.Button("Browse", GUILayout.Width(70)))
            {
                string abs = EditorUtility.OpenFilePanel(
                    "manifest.json inside Assets/", Application.dataPath, "json");
                if (!string.IsNullOrEmpty(abs))
                    manifestPath = "Assets" + abs.Substring(Application.dataPath.Length);
            }
            EditorGUILayout.EndHorizontal();

            matchExplorerOrientation = EditorGUILayout.Toggle(
                new GUIContent("Match explorer orientation",
                    "Mirror the built root on X so the scene reads the way the " +
                    "site's field-scene viewer presents it (verified by landmark " +
                    "test on town01: the raw import puts known buildings on the " +
                    "wrong side of the sea-to-gate axis)"),
                matchExplorerOrientation);
            addWorldColliders = EditorGUILayout.Toggle("World mesh colliders", addWorldColliders);
            using (new EditorGUI.DisabledScope(!addWorldColliders))
            {
                mergedWorldCollider = EditorGUILayout.Toggle(
                    new GUIContent("  Merged + welded (recommended)",
                        "One welded, double-sided collision mesh for the whole world " +
                        "instead of a collider per mesh. The PSX data's mixed winding " +
                        "makes single-sided colliders intangible from one side (you " +
                        "fall through half the floors), per-mesh colliders leave " +
                        "hairline seams, and this also works in client builds without " +
                        "enabling Read/Write on the glb"),
                    mergedWorldCollider);
            }
            addNpcCapsules = EditorGUILayout.Toggle("NPC capsule colliders", addNpcCapsules);
            loopNpcClips = EditorGUILayout.Toggle("Loop NPC spawn clips", loopNpcClips);
            loopPropClips = EditorGUILayout.Toggle("Loop animated-prop clips", loopPropClips);
            hideStaticPropTwins = EditorGUILayout.Toggle(
                new GUIContent("Hide static prop twins",
                    "The world glb keeps a frame-0 static copy under every " +
                    "animated prop; hide it so the pair doesn't z-fight " +
                    "(the merged collider still includes its geometry)"),
                hideStaticPropTwins);
            includeConditionalNpcs = EditorGUILayout.Toggle(
                new GUIContent("Include conditional NPCs",
                    "Story-gated spawns retail parks off-map until a script places them"),
                includeConditionalNpcs);
            wireTeleports = EditorGUILayout.Toggle(
                new GUIContent("Doorway teleports",
                    "Build trigger volumes from the manifest's `teleports` " +
                    "(retail's intra-scene doors: walk into a doorway, land " +
                    "in the interior) and wire the LegaiaDoorway Udon " +
                    "behaviour. Scene portals also connect when the target " +
                    "scene's built root is present in this Unity scene"),
                wireTeleports);
            openDoorsOnApproach = EditorGUILayout.Toggle(
                new GUIContent("Doors open on approach",
                    "Props the manifest tags as doors (`is_door` / gate " +
                    "leaves on an exit band) - plus any prop whose clip is " +
                    "a one-shot (`cyclic: false`: interior doors, cupboards, " +
                    "drawers) - play their swing once when a player comes " +
                    "near and hold the open pose, instead of swinging on a " +
                    "loop forever"),
                openDoorsOnApproach);
            playWorldMorphClip = EditorGUILayout.Toggle(
                new GUIContent("Animate world morphs",
                    "Play the world glb's baked `vdf_pulse` blendshape clip " +
                    "on a loop - Rim Elm's shoreline washing in and out"),
                playWorldMorphClip);
            addMusic = EditorGUILayout.Toggle(
                new GUIContent("Scene music",
                    "Loop the exported BGM WAV (manifest `music`) on the " +
                    "scene root as a 2D AudioSource"),
                addMusic);
            using (new EditorGUI.DisabledScope(!addMusic))
                musicVolume = EditorGUILayout.Slider("  Music volume", musicVolume, 0f, 1f);

            GUILayout.Space(8);
            RealismGUI();

            GUILayout.Space(8);
            EquipmentGUI();

            GUILayout.Space(8);
            using (new EditorGUI.DisabledScope(string.IsNullOrEmpty(manifestPath)))
            {
                if (GUILayout.Button("Build scene"))
                    Build();
            }
            EditorGUILayout.EndScrollView();
        }

        void EquipmentGUI()
        {
            GUILayout.Label("Equipment props", EditorStyles.boldLabel);
            EditorGUILayout.BeginHorizontal();
            itemsManifestPath = EditorGUILayout.TextField(itemsManifestPath);
            if (GUILayout.Button("Browse", GUILayout.Width(70)))
            {
                string abs = EditorUtility.OpenFilePanel(
                    "items/manifest.json inside Assets/ (from export-glb --items)",
                    Application.dataPath, "json");
                if (!string.IsNullOrEmpty(abs))
                    itemsManifestPath = "Assets" + abs.Substring(Application.dataPath.Length);
            }
            EditorGUILayout.EndHorizontal();
            equipWeaponsOnly = EditorGUILayout.Toggle(
                new GUIContent("Weapons only",
                    "Place only the weapon sections (swords, axes, whips...). " +
                    "Untick to also rack armour, headgear, footwear and Ra-Seru"),
                equipWeaponsOnly);
            equipPickups = EditorGUILayout.Toggle(
                new GUIContent("Grabbable (VRC Pickup)",
                    "Wire each prop as a physics pickup (Rigidbody + VRC Pickup " +
                    "+ VRC Object Sync - needs the VRChat SDK; without it the " +
                    "rack is a static display). Props spawn frozen on the rack " +
                    "and only go physical the first time a player drops one"),
                equipPickups);
            equipSizeMult = EditorGUILayout.FloatField(
                new GUIContent("Size multiplier",
                    "Extra scale on top of the scene's export scale. These " +
                    "are battle-mode models the field never shows at field " +
                    "proportions; 0.5 lands them at believable hand-prop " +
                    "size, 1.0 is the raw battle-vs-field ratio"),
                equipSizeMult);
            using (new EditorGUI.DisabledScope(
                string.IsNullOrEmpty(itemsManifestPath) || string.IsNullOrEmpty(manifestPath)))
            {
                if (GUILayout.Button("Place equipment rack near spawn"))
                    PlaceEquipmentProps();
            }
        }

        /// Place the `export-glb --items` item-alone glbs as props on a rack
        /// near the built scene's spawn: one row per character, grounded on
        /// the world collider, wrapped in a convex mesh collider cooked from
        /// the rest pose, and (optionally) wired as VRChat pickups. Items ship in raw PSX units (`conventions.units`), so
        /// each instance carries the scene manifest's scale; the rack lives
        /// at the TOP level, not under the mirrored scene root, so the
        /// proper-rotation item models keep their chirality and the pickup
        /// physics never fights a parent mirror.
        void PlaceEquipmentProps()
        {
            object sm = MiniJson.Parse(File.ReadAllText(manifestPath));
            string sceneName = MiniJson.AsStr(MiniJson.Get(sm, "scene")) ?? "scene";
            var sceneRoot = GameObject.Find("Legaia_" + sceneName);
            if (sceneRoot == null)
            {
                EditorUtility.DisplayDialog("Legaia World Builder",
                    "No built root 'Legaia_" + sceneName +
                    "' in this scene - build first (the rack grounds on its " +
                    "collider and stands near its spawn).", "OK");
                return;
            }
            // Scene export scale x the battle-model size trim (see the
            // field comment on equipSizeMult). Every use below - instance
            // scale, collider bounds, the flat-piece box floor - goes
            // through this one value, so the collider always matches the
            // rendered size.
            float scale = MiniJson.GetNum(sm, "scale", 1f / 64f) * equipSizeMult;
            var spawnT = sceneRoot.transform.Find("LegaiaSpawn");
            Vector3 origin = spawnT != null
                ? spawnT.position : sceneRoot.transform.position;

            string dir = Path.GetDirectoryName(itemsManifestPath).Replace('\\', '/');
            object im = MiniJson.Parse(File.ReadAllText(itemsManifestPath));
            var items = MiniJson.AsList(MiniJson.Get(im, "items"));
            if (items == null || items.Count == 0)
            {
                EditorUtility.DisplayDialog("Legaia World Builder",
                    "No items in " + itemsManifestPath, "OK");
                return;
            }

            var old = GameObject.Find("Legaia_equipment");
            if (old != null)
                Undo.DestroyObjectImmediate(old);
            var rack = new GameObject("Legaia_equipment");
            Undo.RegisterCreatedObjectUndo(rack, "Place Legaia equipment");

            // Fresh collider-mesh assets each run (the hulls must persist as
            // assets or the scene loses them on reload / in a client build).
            string eqDir = "Assets/LegaiaGenerated/" + sceneName + "/equipment";
            AssetDatabase.DeleteAsset(eqDir);
            Directory.CreateDirectory(eqDir);

            var pickupType = FindType("VRC.SDK3.Components.VRCPickup");
            var syncType = FindType("VRC.SDK3.Components.VRCObjectSync");
            if (equipPickups && pickupType == null)
                Debug.LogWarning("[Legaia] VRC Pickup not found (VRChat SDK " +
                    "missing?) - placing the rack as a static display.");
            if (equipPickups && pickupType != null)
                EnsureUdonProgramAssets(); // for LegaiaPickupProp

            var charRow = new Dictionary<string, int>();
            var charCursor = new Dictionary<string, float>();
            int placed = 0, missing = 0;
            foreach (object it in items)
            {
                string label = MiniJson.AsStr(MiniJson.Get(it, "section_label")) ?? "";
                if (equipWeaponsOnly && !label.Contains("Weapon"))
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(it, "alone"))
                    ?? MiniJson.AsStr(MiniJson.Get(it, "with_limb"));
                if (file == null)
                    continue;
                string character = MiniJson.AsStr(MiniJson.Get(it, "character")) ?? "?";
                var go = InstantiateGlb(dir + "/" + file, rack.transform);
                if (go == null)
                {
                    missing++;
                    continue;
                }
                string name = MiniJson.AsStr(MiniJson.Get(it, "name"));
                go.name = character + " - " +
                    (name ?? Path.GetFileNameWithoutExtension(file));
                go.transform.localScale = Vector3.one * scale;

                if (!charRow.ContainsKey(character))
                {
                    charRow[character] = charRow.Count;
                    charCursor[character] = 0f;
                }
                int row = charRow[character];
                Vector3 pos = origin + Vector3.forward * (2.5f + row * 1.2f)
                    + Vector3.right * (1f + charCursor[character]);
                if (Physics.Raycast(pos + Vector3.up * 5f, Vector3.down,
                        out RaycastHit hit, 60f, ~0, QueryTriggerInteraction.Ignore))
                    pos.y = hit.point.y;
                go.transform.position = pos;

                // Collide the geometry as it actually RENDERS. The items are
                // SKINNED meshes (they carry the action bank), and a
                // SkinnedMeshRenderer's .bounds before any animator runs is
                // the imported clip-union volume in character space - metres
                // wide and metres off the weapon; colliders built from that
                // overlapped the whole rack (physics explosion at start) and
                // held each piece high off its own visual. Bake the rest
                // pose instead and cook a CONVEX MeshCollider from it, which
                // wraps a blade far tighter than any axis-aligned box (and a
                // dynamic Rigidbody demands convex anyway).
                Mesh colMesh = BuildRestPoseMesh(go);
                if (colMesh != null)
                {
                    Bounds lb = colMesh.bounds; // go-local (uniform scale)
                    Bounds b = new Bounds(
                        go.transform.TransformPoint(lb.center), lb.size * scale);
                    // Slide so the piece starts at the row cursor and rests
                    // on the floor, then advance the cursor by its real width.
                    Vector3 shift = new Vector3(
                        pos.x - b.min.x,
                        pos.y - b.min.y + 0.01f,
                        pos.z - b.center.z);
                    go.transform.position += shift;
                    b.center += shift;
                    charCursor[character] += b.size.x + 0.25f;

                    float minDim = Mathf.Min(b.size.x,
                        Mathf.Min(b.size.y, b.size.z));
                    if (minDim < 0.03f)
                    {
                        // Near-flat piece: convex cooking is unreliable at
                        // ~zero volume - a padded box does better.
                        Object.DestroyImmediate(colMesh);
                        var box = go.AddComponent<BoxCollider>();
                        box.center = go.transform.InverseTransformPoint(b.center);
                        box.size = Vector3.Max(lb.size,
                            Vector3.one * (0.06f / Mathf.Max(scale, 1e-6f)));
                    }
                    else
                    {
                        colMesh.name = Sanitize(
                            Path.GetFileNameWithoutExtension(file)) + "_col";
                        AssetDatabase.CreateAsset(colMesh,
                            eqDir + "/" + colMesh.name + ".asset");
                        var mc = go.AddComponent<MeshCollider>();
                        mc.convex = true;
                        mc.sharedMesh = colMesh;
                    }
                }
                else
                {
                    charCursor[character] += 0.7f;
                }

                if (equipPickups)
                {
                    var rb = go.AddComponent<Rigidbody>();
                    rb.mass = 1.5f;
                    rb.collisionDetectionMode =
                        CollisionDetectionMode.ContinuousDynamic;
                    // Spawn kinematic regardless of the SDK: dozens of
                    // dynamic bodies waking during world load pick up fall
                    // speed in the load hitches and tunnel through the
                    // paper-thin ground mesh, then respawn-loop.
                    // LegaiaPickupProp frees the body on first drop.
                    rb.isKinematic = true;
                    if (pickupType != null)
                    {
                        go.AddComponent(pickupType);
                        if (syncType != null)
                            go.AddComponent(syncType);
                        SyncUdonProxy(TryAttachUdon(go, "LegaiaPickupProp"));
                    }
                }
                placed++;
            }
            if (missing > 0)
                Debug.LogWarning("[Legaia] " + missing + " item glb(s) not " +
                    "found under Assets - copy the exported items/ folder " +
                    "(with its character subfolders) next to the items " +
                    "manifest and let Unity import first.");
            Debug.Log("[Legaia] placed " + placed + " equipment prop(s) on " +
                      "the rack near LegaiaSpawn.");
        }

        /// One combined mesh of the geometry as it actually RENDERS right
        /// now, in `go`'s local space: skinned meshes are baked at their
        /// current (rest) pose - their `.bounds` is the imported clip-union
        /// volume, useless for a collider - and static meshes contribute
        /// through their transform. Baking skips the transform scale and the
        /// combine maps through localToWorldMatrix, so the scale chain is
        /// applied exactly once on either path. Feed the result to a convex
        /// MeshCollider (and save it as an asset - a scene-only mesh is lost
        /// on reload). Editor-only: mesh reads are always allowed here.
        static Mesh BuildRestPoseMesh(GameObject go)
        {
            var combine = new List<CombineInstance>();
            var temps = new List<Mesh>();
            Matrix4x4 toLocal = go.transform.worldToLocalMatrix;
            foreach (var r in go.GetComponentsInChildren<Renderer>())
            {
                Mesh src = null;
                if (r is SkinnedMeshRenderer smr && smr.sharedMesh != null)
                {
                    var baked = new Mesh();
                    smr.BakeMesh(baked, false);
                    temps.Add(baked);
                    src = baked;
                }
                else
                {
                    var mf = r.GetComponent<MeshFilter>();
                    if (mf != null)
                        src = mf.sharedMesh;
                }
                if (src == null)
                    continue;
                var m = toLocal * r.transform.localToWorldMatrix;
                for (int sm = 0; sm < src.subMeshCount; sm++)
                    combine.Add(new CombineInstance
                    {
                        mesh = src,
                        subMeshIndex = sm,
                        transform = m
                    });
            }
            if (combine.Count == 0)
                return null;
            var outMesh = new Mesh
            {
                indexFormat = UnityEngine.Rendering.IndexFormat.UInt32
            };
            outMesh.CombineMeshes(combine.ToArray(), true, true);
            foreach (var t in temps)
                Object.DestroyImmediate(t);
            return outMesh;
        }

        void RealismGUI()
        {
            showRealism = EditorGUILayout.Foldout(showRealism,
                "Realism enhancements (all on by default - untick for the faithful look)",
                true);
            if (!showRealism)
                return;
            EditorGUI.indentLevel++;
            realism.lighting = EditorGUILayout.Toggle(
                new GUIContent("Realistic lighting",
                    "Swap the unlit retail materials for the kit's lit " +
                    "vertex-colour shaders (smoothed normals are generated - " +
                    "the glbs carry none) and add a warm directional sun with " +
                    "soft shadows and a three-colour ambient. The baked retail " +
                    "shading still modulates every surface, so the scene keeps " +
                    "its palette. Scene-wide lighting settings - reset via " +
                    "Window > Rendering > Lighting to undo"),
                realism.lighting);
            using (new EditorGUI.DisabledScope(!realism.lighting))
            {
                realism.sunElevation = EditorGUILayout.Slider(
                    "  Sun elevation", realism.sunElevation, 5f, 90f);
                realism.sunAzimuth = EditorGUILayout.Slider(
                    "  Sun azimuth", realism.sunAzimuth, 0f, 360f);
                realism.sunIntensity = EditorGUILayout.Slider(
                    "  Sun intensity", realism.sunIntensity, 0f, 2f);
                realism.shadowStrength = EditorGUILayout.Slider(
                    "  Shadow strength", realism.shadowStrength, 0f, 1f);
                realism.dayNight = EditorGUILayout.Toggle(
                    new GUIContent("  Day / night cycle",
                        "Udon: the sun sweeps a full day on a fixed cycle, " +
                        "synced across players via server time (needs the " +
                        "VRChat SDK). Night keeps the dim ambient"),
                    realism.dayNight);
                if (realism.dayNight)
                    realism.dayNightMinutes = EditorGUILayout.Slider(
                        "    Cycle (minutes)", realism.dayNightMinutes, 1f, 120f);
            }
            realism.skyAndFog = EditorGUILayout.Toggle(
                new GUIContent("Sky + distance fog",
                    "Procedural skybox (tracks the sun, so it darkens with the " +
                    "day/night cycle) and linear fog scaled to the scene's " +
                    "bounds. Scene-wide render settings, same undo note as " +
                    "lighting"),
                realism.skyAndFog);
            realism.foliage = EditorGUILayout.Toggle(
                new GUIContent("Ground foliage (grass)",
                    "Procedural wind-swayed grass blades scattered over " +
                    "green-reading ground, tinted from the terrain itself - " +
                    "generated geometry, no game data. Deterministic per seed"),
                realism.foliage);
            using (new EditorGUI.DisabledScope(!realism.foliage))
            {
                realism.grassDensity = EditorGUILayout.Slider(
                    new GUIContent("  Density (tufts / m^2)"),
                    realism.grassDensity, 0.5f, 30f);
                realism.grassGreenThreshold = EditorGUILayout.Slider(
                    new GUIContent("  Green threshold",
                        "How green a ground texel must read before grass grows " +
                        "on it - lower to cover more ground, raise to keep " +
                        "grass off paths"),
                    realism.grassGreenThreshold, 0f, 0.2f);
                realism.grassSeed = EditorGUILayout.IntField(
                    "  Scatter seed", realism.grassSeed);
            }
            realism.interiorShells = EditorGUILayout.Toggle(
                new GUIContent("Interior room shells",
                    "Wrap each detached interior room (the doorway-teleport " +
                    "destinations parked off the village map) in a black " +
                    "inside-only dome: from inside, the sky above and the " +
                    "doorway behind read as black space - retail's own " +
                    "framing - while the dome is invisible from outside and " +
                    "casts no shadow, so the room stays lit"),
                realism.interiorShells);
            using (new EditorGUI.DisabledScope(!realism.interiorShells))
            {
                realism.interiorGlow = EditorGUILayout.Toggle(
                    new GUIContent("  Window light",
                        "A warm fill light per room, so it reads window-lit " +
                        "inside its black shell"),
                    realism.interiorGlow);
                realism.interiorShellMargin = EditorGUILayout.Slider(
                    new GUIContent("  Shell margin (m)",
                        "Clearance between the room geometry and the dome"),
                    realism.interiorShellMargin, 1f, 8f);
                realism.interiorRoomDistance = EditorGUILayout.Slider(
                    new GUIContent("  Room distance (m)",
                        "How far from the spawn a teleport endpoint must sit " +
                        "to count as a detached interior room (town01: village " +
                        "endpoints stay within ~52m, rooms start at ~86m)"),
                    realism.interiorRoomDistance, 20f, 150f);
            }
            realism.smoothTextures = EditorGUILayout.Toggle(
                new GUIContent("Smooth textures (bilinear)",
                    "Bilinear + anisotropic filtering on every texture under " +
                    "the root instead of the exports' PSX point sampling. A " +
                    "glb reimport resets it - rerun after one"),
                realism.smoothTextures);
            realism.ambientAudio = EditorGUILayout.Toggle(
                new GUIContent("Ambient audio bed",
                    "A quiet synthesized wind/surf noise loop on a 2D source - " +
                    "generated audio, not from the disc"),
                realism.ambientAudio);
            using (new EditorGUI.DisabledScope(!realism.ambientAudio))
                realism.ambientVolume = EditorGUILayout.Slider(
                    "  Ambience volume", realism.ambientVolume, 0f, 1f);
            realism.npcWander = EditorGUILayout.Toggle(
                new GUIContent("Villagers wander",
                    "Wire the LegaiaNpcWander Udon behaviour on every " +
                    "talk-kind NPC so the town strolls instead of standing " +
                    "still (needs the VRChat SDK)"),
                realism.npcWander);
            using (new EditorGUI.DisabledScope(!realism.npcWander))
                realism.wanderRadius = EditorGUILayout.Slider(
                    "  Wander radius (m)", realism.wanderRadius, 0.5f, 8f);

            GUILayout.Space(4);
            using (new EditorGUI.DisabledScope(
                string.IsNullOrEmpty(manifestPath) || !realism.AnyEnabled))
            {
                if (GUILayout.Button("Apply enhancements to the already-built root"))
                    ApplyRealismToExisting();
            }
            EditorGUI.indentLevel--;
        }

        /// Run just the realism passes over an existing `Legaia_<scene>`
        /// root (so tuning a slider doesn't force a full rebuild). Every
        /// pass is idempotent - rerunning refreshes rather than stacks.
        void ApplyRealismToExisting()
        {
            object m = MiniJson.Parse(File.ReadAllText(manifestPath));
            string sceneName = MiniJson.AsStr(MiniJson.Get(m, "scene")) ?? "scene";
            var root = GameObject.Find("Legaia_" + sceneName);
            if (root == null)
            {
                EditorUtility.DisplayDialog("Legaia World Builder",
                    "No built root 'Legaia_" + sceneName +
                    "' in this scene - build first.", "OK");
                return;
            }
            if (realism.NeedsUdon)
                EnsureUdonProgramAssets();
            LegaiaRealism.Apply(root, m, sceneName, realism);
        }

        internal static Vector3 G2U(Vector3 g) => new Vector3(-g.x, g.y, g.z);

        void Build()
        {
            string dir = Path.GetDirectoryName(manifestPath).Replace('\\', '/');
            object m = MiniJson.Parse(File.ReadAllText(manifestPath));
            string sceneName = MiniJson.AsStr(MiniJson.Get(m, "scene")) ?? "scene";
            string worldGlb = MiniJson.AsStr(MiniJson.Get(m, "world_glb"));

            float scale = MiniJson.GetNum(m, "scale", 1f);
            // Current exports bake the scale onto each NPC / prop glb's own
            // root node (conventions.npc_prop_units == "scaled") so a file
            // dragged into the scene by hand is already world-sized;
            // instances then carry only the handedness mirror. An older
            // manifest without the flag shipped raw-PSX-unit files whose
            // instances need the full scale here.
            bool scaledAssets = MiniJson.AsStr(MiniJson.Get(
                MiniJson.Get(m, "conventions"), "npc_prop_units")) == "scaled";
            float instScale = scaledAssets ? 1f : scale;

            // Rebuilding over a stale root leaves two overlapping worlds (and
            // any half-wired behaviours from an aborted earlier build keep
            // spamming the console) - offer to clear it first.
            var existingRoot = GameObject.Find("Legaia_" + sceneName);
            if (existingRoot != null && EditorUtility.DisplayDialog(
                    "Legaia World Builder",
                    "A built root 'Legaia_" + sceneName + "' already exists " +
                    "in this scene. Replace it?", "Replace", "Keep both"))
                Undo.DestroyObjectImmediate(existingRoot);

            // U# refuses to attach a behaviour whose script has no
            // UdonSharpProgramAsset - and bare .cs files copied into a
            // project have none. Create any missing ones before wiring.
            if (wireTeleports || openDoorsOnApproach || realism.NeedsUdon)
                EnsureUdonProgramAssets();

            var root = new GameObject("Legaia_" + sceneName);
            Undo.RegisterCreatedObjectUndo(root, "Build Legaia scene");

            // --- World ---
            var world = InstantiateGlb(dir + "/" + worldGlb, root.transform);
            if (world == null)
            {
                EditorUtility.DisplayDialog("Legaia World Builder",
                    "Could not load " + worldGlb + " - is glTFast installed and " +
                    "the exported folder inside Assets/?", "OK");
                DestroyImmediate(root);
                return;
            }
            world.name = "world";
            if (addWorldColliders)
            {
                if (mergedWorldCollider)
                {
                    AddMergedCollider(world, sceneName);
                }
                else
                {
                    foreach (var mf in world.GetComponentsInChildren<MeshFilter>())
                    {
                        if (mf.sharedMesh == null || mf.GetComponent<MeshCollider>() != null)
                            continue;
                        mf.gameObject.AddComponent<MeshCollider>().sharedMesh = mf.sharedMesh;
                    }
                }
            }

            // Shoreline & other baked vertex morphs: the world glb carries a
            // looping `vdf_pulse` blendshape clip when the exporter armed the
            // engine's scene-entry VDF pulse - play it on the world instance.
            object worldAnim = MiniJson.Get(m, "world_anim");
            if (playWorldMorphClip && worldAnim != null)
                AttachLoopingClip(world, dir + "/" + worldGlb,
                    MiniJson.AsStr(MiniJson.Get(worldAnim, "clip")), dir, sceneName);

            // --- Spawn marker (assign to your VRC scene descriptor) ---
            Vector3 spawn = G2U(MiniJson.GetVec3(MiniJson.Get(m, "spawn"), "position"));
            var spawnGo = new GameObject("LegaiaSpawn");
            spawnGo.transform.SetParent(root.transform, false);
            spawnGo.transform.localPosition = spawn + Vector3.up * 0.1f;

            // --- Scene music (the exported seamless BGM loop) ---
            string musicFile = MiniJson.AsStr(
                MiniJson.Get(MiniJson.Get(m, "music"), "file"));
            if (addMusic && musicFile != null)
            {
                var clip = AssetDatabase.LoadAssetAtPath<AudioClip>(dir + "/" + musicFile);
                if (clip != null)
                {
                    var src = root.AddComponent<AudioSource>();
                    src.clip = clip;
                    src.loop = true;
                    src.playOnAwake = true;
                    src.spatialBlend = 0f; // the town theme plays everywhere
                    src.volume = musicVolume;
                }
                else
                {
                    Debug.LogWarning("[Legaia] music clip not imported yet: " +
                                     dir + "/" + musicFile);
                }
            }

            // --- NPCs ---
            var npcRoot = new GameObject("npcs");
            npcRoot.transform.SetParent(root.transform, false);
            int npcCount = 0;
            foreach (object n in MiniJson.AsList(MiniJson.Get(m, "npcs")) ?? new List<object>())
            {
                bool conditional = MiniJson.Get(n, "conditional") is bool b && b;
                if (conditional && !includeConditionalNpcs)
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file"));
                var go = InstantiateGlb(dir + "/" + file, npcRoot.transform);
                if (go == null) continue;
                // Negative Z: the handedness mirror (see header note);
                // instScale covers legacy raw-PSX-unit exports.
                go.transform.localScale =
                    new Vector3(instScale, instScale, PROP_NPC_SCALE_Z * instScale);
                go.transform.localPosition = G2U(MiniJson.GetVec3(n, "position"));
                string label = MiniJson.AsStr(MiniJson.Get(n, "label"));
                if (!string.IsNullOrEmpty(label))
                    go.name += " (" + label + ")";
                var clips = MiniJson.AsList(MiniJson.Get(n, "clips"));
                if (loopNpcClips && clips != null && clips.Count > 0)
                    AttachLoopingClip(go, dir + "/" + file,
                        MiniJson.AsStr(clips[0]), dir, sceneName);
                if (addNpcCapsules)
                    AddCapsule(go);
                npcCount++;
            }

            // --- Animated props ---
            var propRoot = new GameObject("props");
            propRoot.transform.SetParent(root.transform, false);
            int propCount = 0;
            int doorCount = 0;
            foreach (object p in MiniJson.AsList(MiniJson.Get(m, "animated_props"))
                     ?? new List<object>())
            {
                string file = MiniJson.AsStr(MiniJson.Get(p, "file"));
                var insts = MiniJson.AsList(MiniJson.Get(p, "instances"));
                if (file == null || insts == null) continue;
                // A one-shot clip (swing/slide that ends displaced from its
                // first frame - interior doors, cupboards, drawers) must
                // never free-loop: looped it re-plays its opening forever.
                // Older manifests lack the flag; treat those as cyclic so
                // only the teleport/portal tagging fires, as before.
                bool oneShot = MiniJson.Get(p, "cyclic") is bool cyc && !cyc;
                foreach (object inst in insts)
                {
                    // Per-instance guard: one failed wire (e.g. an Udon
                    // attach refusal) must not abort the rest of the build -
                    // that once cost a build every teleport it never reached.
                    try
                    {
                    var go = InstantiateGlb(dir + "/" + file, propRoot.transform);
                    if (go == null) continue;
                    // Negative Z: the handedness mirror (see header note) -
                    // without it no yaw can align a prop with its baked twin.
                    go.transform.localScale =
                        new Vector3(instScale, instScale, PROP_NPC_SCALE_Z * instScale);
                    go.transform.localPosition = G2U(MiniJson.GetVec3(inst, "position"));
                    float yaw = MiniJson.GetNum(inst, "rot_y_radians") * Mathf.Rad2Deg;
                    go.transform.localRotation = Quaternion.Euler(0, YAW_SIGN * yaw, 0);
                    // A door-tagged instance (its bind record teleports the
                    // player, or it stands on a scene-exit band - a gate
                    // leaf) opens on approach and stays open, and so does
                    // any one-shot clip regardless of tags; only cyclic
                    // clips free-run.
                    bool isDoor = MiniJson.Get(inst, "is_door") is bool db && db;
                    bool gateLeaf = MiniJson.Get(inst, "near_portal") is double;
                    if (openDoorsOnApproach && (isDoor || gateLeaf || oneShot))
                    {
                        AttachProximityDoor(go, dir + "/" + file, sceneName);
                        doorCount++;
                    }
                    else if (loopPropClips)
                    {
                        AttachLoopingClip(go, dir + "/" + file, null, dir, sceneName);
                    }
                    if (hideStaticPropTwins)
                        HideStaticTwin(world, file, go.transform.localPosition);
                    propCount++;
                    }
                    catch (System.Exception e)
                    {
                        Debug.LogError("[Legaia] prop instance of " + file +
                            " failed to wire, continuing: " + (e.InnerException ?? e).Message);
                    }
                }
            }

            // --- Doorway teleports + scene portals ---
            // The trigger boxes and landings are retail's own door data (see
            // the manifest's `teleports` / `scene_portals`); the arrival
            // facing is resolved AFTER the final mirror below, so collect the
            // (marker, root-local direction) pairs here.
            var facingMarkers = new List<KeyValuePair<Transform, Vector3>>();
            int teleportCount = 0;
            if (wireTeleports)
            {
                var tpRoot = new GameObject("teleports");
                tpRoot.transform.SetParent(root.transform, false);
                var tps = MiniJson.AsList(MiniJson.Get(m, "teleports"))
                          ?? new List<object>();
                for (int i = 0; i < tps.Count; i++)
                {
                    try
                    {
                    object tp = tps[i];
                    object trig = MiniJson.Get(tp, "trigger");
                    string kind = MiniJson.AsStr(MiniJson.Get(tp, "kind"));
                    Vector3 half = MiniJson.GetVec3(trig, "half_extents");
                    var go = new GameObject("teleport_" + i + "_" + kind);
                    go.transform.SetParent(tpRoot.transform, false);
                    // Trigger position is at the floor; centre the box a
                    // half-height up so it covers a standing player.
                    go.transform.localPosition =
                        G2U(MiniJson.GetVec3(trig, "position")) + Vector3.up * half.y;
                    var box = go.AddComponent<BoxCollider>();
                    box.isTrigger = true;
                    box.size = new Vector3(half.x * 2f, half.y * 2f, half.z * 2f);

                    var dst = new GameObject("dest");
                    dst.transform.SetParent(go.transform, false);
                    // Destination is absolute in the manifest frame (which is
                    // the root's local frame), not trigger-relative.
                    Vector3 destLocal =
                        G2U(MiniJson.GetVec3(MiniJson.Get(tp, "destination"), "position"))
                        + Vector3.up * 0.05f;
                    dst.transform.localPosition = destLocal - go.transform.localPosition;

                    var fd = MiniJson.AsList(MiniJson.Get(tp, "facing_dir"));
                    bool hasFacing = fd != null && fd.Count >= 2;
                    if (hasFacing)
                        facingMarkers.Add(new KeyValuePair<Transform, Vector3>(
                            dst.transform,
                            G2U(new Vector3((float)MiniJson.AsNum(fd[0]), 0,
                                            (float)MiniJson.AsNum(fd[1])))));

                    var udon = TryAttachUdon(go, "LegaiaDoorway");
                    SetUdonField(udon, "destination", dst.transform);
                    SetUdonField(udon, "alignToDestination", hasFacing);
                    SyncUdonProxy(udon);
                    teleportCount++;
                    }
                    catch (System.Exception e)
                    {
                        Debug.LogError("[Legaia] teleport " + i +
                            " failed to wire, continuing: " + (e.InnerException ?? e).Message);
                    }
                }

                // Scene portals (town exits / entrances) connect only when
                // the target scene is already built in this Unity scene: the
                // landing goes at the manifest entry point in the TARGET
                // root's frame, grounded by a downward raycast against its
                // colliders.
                var portals = MiniJson.AsList(MiniJson.Get(m, "scene_portals"))
                              ?? new List<object>();
                for (int i = 0; i < portals.Count; i++)
                {
                    try
                    {
                    object p = portals[i];
                    string target = MiniJson.AsStr(MiniJson.Get(p, "target_scene"));
                    var entry = MiniJson.AsList(MiniJson.Get(p, "entry_xz"));
                    if (target == null || entry == null || entry.Count < 2)
                        continue;
                    var targetRoot = GameObject.Find("Legaia_" + target);
                    if (targetRoot == null)
                        continue; // single-scene build: leave the exit inert
                    object trig = MiniJson.Get(p, "trigger");
                    Vector3 half = MiniJson.GetVec3(trig, "half_extents");
                    var go = new GameObject("portal_" + i + "_" + target);
                    go.transform.SetParent(tpRoot.transform, false);
                    go.transform.localPosition =
                        G2U(MiniJson.GetVec3(trig, "position")) + Vector3.up * half.y;
                    var box = go.AddComponent<BoxCollider>();
                    box.isTrigger = true;
                    box.size = new Vector3(half.x * 2f, half.y * 2f, half.z * 2f);

                    var dst = new GameObject("arrival_from_" + sceneName);
                    dst.transform.SetParent(targetRoot.transform, false);
                    dst.transform.localPosition = new Vector3(
                        -(float)MiniJson.AsNum(entry[0]), 1f,
                        (float)MiniJson.AsNum(entry[1]));
                    Vector3 probe = dst.transform.position + Vector3.up * 50f;
                    if (Physics.Raycast(probe, Vector3.down, out RaycastHit hit, 200f))
                        dst.transform.position = hit.point + Vector3.up * 0.05f;

                    var fd = MiniJson.AsList(MiniJson.Get(p, "facing_dir"));
                    if (fd != null && fd.Count >= 2)
                        facingMarkers.Add(new KeyValuePair<Transform, Vector3>(
                            dst.transform,
                            G2U(new Vector3((float)MiniJson.AsNum(fd[0]), 0,
                                            (float)MiniJson.AsNum(fd[1])))));

                    var udon = TryAttachUdon(go, "LegaiaDoorway");
                    SetUdonField(udon, "destination", dst.transform);
                    SetUdonField(udon, "alignToDestination", fd != null && fd.Count >= 2);
                    SyncUdonProxy(udon);
                    teleportCount++;
                    }
                    catch (System.Exception e)
                    {
                        Debug.LogError("[Legaia] scene portal " + i +
                            " failed to wire, continuing: " + (e.InnerException ?? e).Message);
                    }
                }
            }

            // Mirror the whole assembly at the very end: children keep their
            // manifest-frame local transforms, the root flips them into the
            // explorer pages' presentation as one unit (see the orientation
            // note in the header - this is empirically a mirror, not a
            // rotation).
            if (matchExplorerOrientation)
                root.transform.localScale = new Vector3(-1f, 1f, 1f);

            // Resolve arrival facings AFTER the final mirror stands: a
            // Transform's `.rotation` ignores parent scale, so the facing is
            // baked as a world rotation from the mirrored direction
            // (`TransformDirection` DOES include the scale). The stored
            // direction is root-local (one G2U flip), so the parent chain
            // supplies the mirror exactly once, matching how every position
            // flows.
            foreach (var fm in facingMarkers)
            {
                Vector3 dirWorld = fm.Key.parent.TransformDirection(fm.Value);
                dirWorld.y = 0;
                if (dirWorld.sqrMagnitude > 1e-6f)
                    fm.Key.rotation =
                        Quaternion.LookRotation(dirWorld.normalized, Vector3.up);
            }

            // Optional realism layer, after the final mirror stands (its
            // passes sample and scatter in the finished world frame).
            if (realism.AnyEnabled)
                LegaiaRealism.Apply(root, m, sceneName, realism);

            Selection.activeGameObject = root;
            Debug.Log("[Legaia] built " + sceneName + ": world + " + npcCount +
                      " NPC(s) + " + propCount + " animated prop instance(s) (" +
                      doorCount + " proximity door(s)) + " + teleportCount +
                      " doorway teleport(s). " +
                      "Point your VRC scene descriptor spawn at LegaiaSpawn.");
        }

        /// The world glb keeps every animated placement's frame-0 static twin
        /// (`mesh_<slot>_anim<id>` at the same spot the manifest places the
        /// animated instance). Hide it so the pair doesn't z-fight - matched
        /// by mesh name (prop file stem `prop_15_anim2` -> `mesh_15_anim2`,
        /// which glTFast preserves) plus position, so nothing else is
        /// touched. Disabled, not destroyed: re-enable to swap back to the
        /// static version, and the merged collider (built before the props
        /// loop) keeps its geometry either way.
        static void HideStaticTwin(GameObject world, string propFile, Vector3 localPos)
        {
            string stem = Path.GetFileNameWithoutExtension(propFile);
            string meshName = stem.StartsWith("prop_")
                ? "mesh_" + stem.Substring("prop_".Length)
                : stem;
            foreach (var mf in world.GetComponentsInChildren<MeshFilter>())
            {
                if (mf.sharedMesh == null || mf.sharedMesh.name != meshName)
                    continue;
                var p = world.transform.InverseTransformPoint(mf.transform.position);
                if ((p - localPos).sqrMagnitude < 1e-4f)
                    mf.gameObject.SetActive(false);
            }
        }

        /// One welded collision mesh for the whole world: every renderer
        /// submesh (semi-transparent water included, so there's a floor over
        /// the sea) combined in world-local space, saved as an asset, cooked
        /// with colocated-vertex welding so hairline seams between adjacent
        /// tile meshes close instead of dropping a player capsule through.
        static void AddMergedCollider(GameObject world, string sceneName)
        {
            var combine = new List<CombineInstance>();
            foreach (var mf in world.GetComponentsInChildren<MeshFilter>())
            {
                if (mf.sharedMesh == null) continue;
                var toLocal = world.transform.worldToLocalMatrix * mf.transform.localToWorldMatrix;
                for (int sm = 0; sm < mf.sharedMesh.subMeshCount; sm++)
                    combine.Add(new CombineInstance
                    {
                        mesh = mf.sharedMesh,
                        subMeshIndex = sm,
                        transform = toLocal
                    });
            }
            if (combine.Count == 0) return;

            var mesh = new Mesh
            {
                name = "world_collider",
                indexFormat = UnityEngine.Rendering.IndexFormat.UInt32
            };
            mesh.CombineMeshes(combine.ToArray(), true, true);

            // Double-side the collision: PhysX triangle meshes collide on the
            // wound face only, and the PSX source data's winding is mixed
            // (retail culled per-view via NCLIP; the renderers draw
            // double-sided, so it never shows) - single-sided cooking leaves
            // roughly half the floors intangible. Appending each triangle
            // reversed makes every surface solid from both sides.
            int[] tris = mesh.triangles;
            int[] both = new int[tris.Length * 2];
            tris.CopyTo(both, 0);
            for (int i = 0; i < tris.Length; i += 3)
            {
                both[tris.Length + i] = tris[i];
                both[tris.Length + i + 1] = tris[i + 2];
                both[tris.Length + i + 2] = tris[i + 1];
            }
            mesh.triangles = both;

            string genDir = "Assets/LegaiaGenerated/" + sceneName;
            Directory.CreateDirectory(genDir);
            string meshPath = genDir + "/world_collider.asset";
            AssetDatabase.DeleteAsset(meshPath);
            AssetDatabase.CreateAsset(mesh, meshPath);

            var col = world.AddComponent<MeshCollider>();
            col.cookingOptions = MeshColliderCookingOptions.EnableMeshCleaning
                | MeshColliderCookingOptions.WeldColocatedVertices
                | MeshColliderCookingOptions.UseFastMidphase;
            col.sharedMesh = mesh;
        }

        /// Instantiate the glTFast-imported prefab at `assetPath` (null when
        /// the asset is missing or not yet imported).
        static GameObject InstantiateGlb(string assetPath, Transform parent)
        {
            var prefab = AssetDatabase.LoadAssetAtPath<GameObject>(assetPath);
            if (prefab == null) return null;
            var go = (GameObject)PrefabUtility.InstantiatePrefab(prefab);
            go.transform.SetParent(parent, false);
            return go;
        }

        /// Wire a looping Animator playing one imported clip: the clip named
        /// `clipName` (or the glb's first clip when null), duplicated into
        /// Assets/LegaiaGenerated/ so its loop flag is editable, driven by a
        /// one-state AnimatorController.
        static void AttachLoopingClip(
            GameObject go, string glbPath, string clipName, string sceneDir, string sceneName)
        {
            var clips = AssetDatabase.LoadAllAssetsAtPath(glbPath)
                .OfType<AnimationClip>()
                .Where(c => !c.name.StartsWith("__preview"))
                .ToList();
            if (clips.Count == 0) return;
            var clip = clipName != null
                ? clips.FirstOrDefault(c => c.name == clipName) ?? clips[0]
                : clips[0];

            string genDir = "Assets/LegaiaGenerated/" + sceneName;
            Directory.CreateDirectory(genDir);
            string baseName = Path.GetFileNameWithoutExtension(glbPath) + "_" + clip.name;
            string clipPath = genDir + "/" + Sanitize(baseName) + ".anim";
            var looped = AssetDatabase.LoadAssetAtPath<AnimationClip>(clipPath);
            if (looped == null)
            {
                looped = Object.Instantiate(clip);
                var settings = AnimationUtility.GetAnimationClipSettings(looped);
                settings.loopTime = true;
                AnimationUtility.SetAnimationClipSettings(looped, settings);
                AssetDatabase.CreateAsset(looped, clipPath);
            }

            string ctrlPath = genDir + "/" + Sanitize(baseName) + ".controller";
            var ctrl = AssetDatabase.LoadAssetAtPath<AnimatorController>(ctrlPath);
            if (ctrl == null)
                ctrl = AnimatorController.CreateAnimatorControllerAtPathWithClip(
                    ctrlPath, looped);

            var animator = go.GetComponentInChildren<Animator>();
            if (animator == null)
                animator = go.AddComponent<Animator>();
            animator.runtimeAnimatorController = ctrl;
        }

        /// Door prop: a two-state AnimatorController - `closed` (default; the
        /// swing clip parked at frame 0, which IS the closed pose) and `open`
        /// (the clip once, loop off, so the Animator holds the final open
        /// frame) - plus an approach trigger and the LegaiaDoor Udon
        /// behaviour that plays `open` on first contact.
        static void AttachProximityDoor(GameObject go, string glbPath, string sceneName)
        {
            var clips = AssetDatabase.LoadAllAssetsAtPath(glbPath)
                .OfType<AnimationClip>()
                .Where(c => !c.name.StartsWith("__preview"))
                .ToList();
            if (clips.Count == 0) return;
            var clip = clips[0];

            string genDir = "Assets/LegaiaGenerated/" + sceneName;
            Directory.CreateDirectory(genDir);
            string baseName =
                Path.GetFileNameWithoutExtension(glbPath) + "_" + clip.name + "_door";
            string clipPath = genDir + "/" + Sanitize(baseName) + ".anim";
            var once = AssetDatabase.LoadAssetAtPath<AnimationClip>(clipPath);
            if (once == null)
            {
                once = Object.Instantiate(clip);
                var settings = AnimationUtility.GetAnimationClipSettings(once);
                settings.loopTime = false;
                AnimationUtility.SetAnimationClipSettings(once, settings);
                AssetDatabase.CreateAsset(once, clipPath);
            }

            string ctrlPath = genDir + "/" + Sanitize(baseName) + ".controller";
            var ctrl = AssetDatabase.LoadAssetAtPath<AnimatorController>(ctrlPath);
            if (ctrl == null)
            {
                ctrl = AnimatorController.CreateAnimatorControllerAtPath(ctrlPath);
                var sm = ctrl.layers[0].stateMachine;
                var closed = sm.AddState("closed");
                closed.motion = once;
                closed.speed = 0f; // parked at frame 0 = closed
                var open = sm.AddState("open");
                open.motion = once;
                sm.defaultState = closed;
            }

            var animator = go.GetComponentInChildren<Animator>();
            if (animator == null)
                animator = go.AddComponent<Animator>();
            animator.runtimeAnimatorController = ctrl;

            // Approach trigger: an UNSCALED sibling at the prop's position.
            // The prop instance carries the PSX-unit scale (plus the negative
            // Z mirror), so a collider on the prop itself needs error-prone
            // local-unit math - and a door prop is often a whole hut mesh
            // whose door node sits at the prop origin (which retail parks on
            // the doorway trigger tile), so render bounds would open the door
            // from anywhere around the building. A fixed people-sized box at
            // the origin opens it a couple of steps out, on the doorway side
            // and every other - close enough to retail's walk-up feel.
            var trigger = new GameObject(go.name + "_approach");
            trigger.transform.SetParent(go.transform.parent, false);
            trigger.transform.localPosition = go.transform.localPosition;
            var box = trigger.AddComponent<BoxCollider>();
            box.isTrigger = true;
            box.center = Vector3.up * 1.2f;
            // Player-reach sized, in absolute meters (the player stays
            // real-sized whatever the world scale): at the 1 m-per-tile
            // export a wider box would swallow a whole hut interior and
            // pop its cupboards open from the doorway.
            box.size = new Vector3(3f, 3f, 3f);

            var udon = TryAttachUdon(trigger, "LegaiaDoor");
            SetUdonField(udon, "doorAnimator", animator);
            SyncUdonProxy(udon);
        }

        /// Attach an UdonSharp behaviour by type name without a compile-time
        /// dependency on the VRChat SDK (this Editor script must compile in a
        /// bare Unity project too). Prefers UdonSharpEditor's
        /// AddUdonSharpComponent so the backing UdonBehaviour is created;
        /// falls back to a plain AddComponent; warns when the type isn't
        /// compiled (SDK missing) so the object is visibly inert, not
        /// silently so.
        internal static System.Type FindType(string fullName)
        {
            foreach (var asm in System.AppDomain.CurrentDomain.GetAssemblies())
            {
                var t = asm.GetType(fullName);
                if (t != null) return t;
            }
            return null;
        }

        /// UdonSharp refuses AddUdonSharpComponent for a script that has no
        /// UdonSharpProgramAsset ("Unable to find valid U# program asset
        /// associated with script"), and bare .cs files copied into a project
        /// have none - U# only creates them for scripts made through its own
        /// Create menu. Create any missing ones next to the scripts (the same
        /// CreateInstance + sourceCsScript pattern U#'s own editor uses),
        /// reset U#'s script-to-program lookup cache, and compile so the
        /// backing programs exist before the first behaviour is wired.
        internal static void EnsureUdonProgramAssets()
        {
            var paType = FindType("UdonSharp.UdonSharpProgramAsset");
            var utilType = FindType("UdonSharpEditor.UdonSharpEditorUtility");
            if (paType == null || utilType == null)
                return; // no VRC SDK - TryAttachUdon warns per object
            var getPa = utilType.GetMethod(
                "GetUdonSharpProgramAsset", new[] { typeof(System.Type) });
            bool created = false;
            foreach (string name in new[]
                     { "LegaiaDoorway", "LegaiaDoor", "LegaiaNpcWander",
                       "LegaiaDayNight", "LegaiaPickupProp" })
            {
                var t = FindType("LegaiaWorld." + name);
                if (t == null) continue;
                if (getPa != null && getPa.Invoke(null, new object[] { t }) != null)
                    continue; // already has one
                MonoScript script = null;
                foreach (string guid in AssetDatabase.FindAssets(name + " t:MonoScript"))
                {
                    var ms = AssetDatabase.LoadAssetAtPath<MonoScript>(
                        AssetDatabase.GUIDToAssetPath(guid));
                    if (ms != null && ms.GetClass() == t)
                    {
                        script = ms;
                        break;
                    }
                }
                if (script == null)
                {
                    Debug.LogWarning("[Legaia] no MonoScript asset found for " +
                        name + " - cannot create its U# program asset.");
                    continue;
                }
                string scriptPath = AssetDatabase.GetAssetPath(script);
                string assetPath = Path.ChangeExtension(scriptPath, ".asset");
                if (AssetDatabase.LoadAssetAtPath<Object>(assetPath) != null)
                    assetPath = scriptPath.Substring(0, scriptPath.Length - 3)
                        + "Program.asset";
                var pa = ScriptableObject.CreateInstance(paType);
                paType.GetField("sourceCsScript").SetValue(pa, script);
                // A fresh program asset carries ScriptVersion Unknown, and
                // CopyProxyToUdon refuses to serialize until BOTH versions
                // reach CurrentVersion. CompileSync below bumps
                // CompiledVersion, but ScriptVersion is only ever bumped by
                // U#'s editor-update-deferred upgrader - whose rewrite pass
                // is a no-op for these scripts (plain Unity-serializable
                // public fields) and terminates by setting exactly this
                // value. Set it up front so wiring works in this same build.
                var sv = paType.GetProperty("ScriptVersion");
                if (sv != null)
                    sv.SetValue(pa,
                        System.Enum.Parse(sv.PropertyType, "CurrentVersion"));
                AssetDatabase.CreateAsset(pa, assetPath);
                Debug.Log("[Legaia] created U# program asset " + assetPath);
                created = true;
            }
            if (!created) return;
            AssetDatabase.Refresh();
            // The attach path's script->program lookup is cached; reset it so
            // the new assets are visible in this same build pass.
            utilType.GetMethod("ResetCaches",
                System.Reflection.BindingFlags.NonPublic |
                System.Reflection.BindingFlags.Static)?.Invoke(null, null);
            var compile = FindType("UdonSharp.Compiler.UdonSharpCompilerV1")
                ?.GetMethod("CompileSync");
            if (compile != null)
                compile.Invoke(null, new object[] { null });
        }

        internal static Component TryAttachUdon(GameObject go, string typeName)
        {
            var t = FindType("LegaiaWorld." + typeName);
            if (t == null)
            {
                Debug.LogWarning("[Legaia] " + typeName + " is not compiled " +
                    "(VRChat SDK / UdonSharp missing?) - " + go.name +
                    " stays inert until the behaviour is added manually.");
                return null;
            }
            var ext = FindType("UdonSharpEditor.UdonSharpComponentExtensions");
            if (ext != null)
            {
                var mi = ext.GetMethod("AddUdonSharpComponent",
                    new[] { typeof(GameObject), typeof(System.Type) });
                if (mi != null)
                {
                    try
                    {
                        return (Component)mi.Invoke(null, new object[] { go, t });
                    }
                    catch (System.Exception e)
                    {
                        // Most common cause: no UdonSharpProgramAsset for the
                        // script (EnsureUdonProgramAssets should have made
                        // one). Leave the object inert rather than adding a
                        // bare proxy that spams serialization errors.
                        var inner = e.InnerException ?? e;
                        Debug.LogError("[Legaia] U# attach of " + typeName +
                            " to " + go.name + " failed: " + inner.Message);
                        return null;
                    }
                }
            }
            // Plain AddComponent creates the proxy but not necessarily the
            // backing UdonBehaviour - visible so it can be fixed by hand.
            Debug.LogWarning("[Legaia] UdonSharpEditor not found; adding " +
                typeName + " to " + go.name + " without the U# attach path - " +
                "verify a backing UdonBehaviour exists on it.");
            return go.AddComponent(t);
        }

        /// Set a public field on an attached Udon behaviour via reflection
        /// (keeps this file free of compile-time VRC SDK references). Fields
        /// land on the U# PROXY component only - call SyncUdonProxy once all
        /// fields are set, or the backing UdonBehaviour (what actually runs
        /// in-world) keeps null defaults and the behaviour silently no-ops.
        internal static void SetUdonField(Component comp, string field, object value)
        {
            if (comp == null) return;
            var f = comp.GetType().GetField(field);
            if (f == null) return;
            f.SetValue(comp, value);
            EditorUtility.SetDirty(comp);
        }

        /// Copy an UdonSharpBehaviour proxy's serialized fields down to its
        /// backing UdonBehaviour (UdonSharpEditorUtility.CopyProxyToUdon) -
        /// the documented requirement after editing a proxy from an editor
        /// script. Without it the teleport destinations and door animator
        /// references never reach the program that runs in-world.
        internal static void SyncUdonProxy(Component comp)
        {
            if (comp == null) return;
            var util = FindType("UdonSharpEditor.UdonSharpEditorUtility");
            if (util == null) return;
            foreach (var mi in util.GetMethods(
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.Static))
            {
                if (mi.Name != "CopyProxyToUdon") continue;
                var ps = mi.GetParameters();
                if (ps.Length != 1 || !ps[0].ParameterType.IsInstanceOfType(comp))
                    continue;
                try
                {
                    mi.Invoke(null, new object[] { comp });
                }
                catch (System.Exception e)
                {
                    // Unwrap TargetInvocationException so the real U#
                    // message reaches the console.
                    Debug.LogError("[Legaia] CopyProxyToUdon failed on " +
                        comp.gameObject.name + ": " +
                        (e.InnerException ?? e).Message);
                }
                return;
            }
            Debug.LogWarning("[Legaia] CopyProxyToUdon not found - " +
                comp.gameObject.name + "'s Udon fields may not be applied.");
        }

        /// A rough person-sized capsule from the instance's render bounds so
        /// players don't walk through NPCs.
        static void AddCapsule(GameObject go)
        {
            var renderers = go.GetComponentsInChildren<Renderer>();
            if (renderers.Length == 0) return;
            var b = renderers[0].bounds;
            foreach (var r in renderers) b.Encapsulate(r.bounds);
            var cap = go.AddComponent<CapsuleCollider>();
            cap.center = go.transform.InverseTransformPoint(b.center);
            cap.height = Mathf.Max(b.size.y, 0.5f);
            cap.radius = Mathf.Max(Mathf.Min(b.size.x, b.size.z) * 0.35f, 0.15f);
        }

        internal static string Sanitize(string s)
        {
            foreach (char c in Path.GetInvalidFileNameChars())
                s = s.Replace(c, '_');
            return s;
        }
    }
}
