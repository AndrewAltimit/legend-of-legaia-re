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

        [MenuItem("Legaia/Build Scene From Manifest...")]
        static void Open()
        {
            GetWindow<LegaiaWorldBuilder>("Legaia World Builder");
        }

        void OnGUI()
        {
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
                    "leaves on an exit band) play their swing once when a " +
                    "player comes near and hold the open pose, instead of " +
                    "swinging on a loop forever"),
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
            using (new EditorGUI.DisabledScope(string.IsNullOrEmpty(manifestPath)))
            {
                if (GUILayout.Button("Build scene"))
                    Build();
            }
        }

        static Vector3 G2U(Vector3 g) => new Vector3(-g.x, g.y, g.z);

        void Build()
        {
            string dir = Path.GetDirectoryName(manifestPath).Replace('\\', '/');
            object m = MiniJson.Parse(File.ReadAllText(manifestPath));
            string sceneName = MiniJson.AsStr(MiniJson.Get(m, "scene")) ?? "scene";
            string worldGlb = MiniJson.AsStr(MiniJson.Get(m, "world_glb"));

            float scale = MiniJson.GetNum(m, "scale", 1f);

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
                // NPC/prop glbs ship in raw PSX units (same as the site's
                // downloads); manifest positions are pre-scaled, the meshes
                // are not - scale each instance by the manifest's factor.
                // Negative Z: the handedness mirror (see header note).
                go.transform.localScale =
                    new Vector3(scale, scale, PROP_NPC_SCALE_Z * scale);
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
                foreach (object inst in insts)
                {
                    var go = InstantiateGlb(dir + "/" + file, propRoot.transform);
                    if (go == null) continue;
                    // Negative Z: the handedness mirror (see header note) -
                    // without it no yaw can align a prop with its baked twin.
                    go.transform.localScale =
                        new Vector3(scale, scale, PROP_NPC_SCALE_Z * scale);
                    go.transform.localPosition = G2U(MiniJson.GetVec3(inst, "position"));
                    float yaw = MiniJson.GetNum(inst, "rot_y_radians") * Mathf.Rad2Deg;
                    go.transform.localRotation = Quaternion.Euler(0, YAW_SIGN * yaw, 0);
                    // A door-tagged instance (its bind record teleports the
                    // player, or it stands on a scene-exit band - a gate
                    // leaf) opens on approach and stays open; anything else
                    // free-runs its clip.
                    bool isDoor = MiniJson.Get(inst, "is_door") is bool db && db;
                    bool gateLeaf = MiniJson.Get(inst, "near_portal") is double;
                    if (openDoorsOnApproach && (isDoor || gateLeaf))
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
                    teleportCount++;
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
                    teleportCount++;
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

            // Approach trigger from the render bounds, padded so the swing
            // starts a step or two before the player reaches the leaf.
            var box = go.AddComponent<BoxCollider>();
            box.isTrigger = true;
            var renderers = go.GetComponentsInChildren<Renderer>();
            if (renderers.Length > 0)
            {
                var b = renderers[0].bounds;
                foreach (var r in renderers) b.Encapsulate(r.bounds);
                box.center = go.transform.InverseTransformPoint(b.center);
                Vector3 size = go.transform.InverseTransformVector(b.size);
                box.size = new Vector3(
                    Mathf.Abs(size.x) + 2.5f,
                    Mathf.Max(Mathf.Abs(size.y), 2f),
                    Mathf.Abs(size.z) + 2.5f);
            }
            else
            {
                box.size = new Vector3(3f, 2.5f, 3f);
            }

            var udon = TryAttachUdon(go, "LegaiaDoor");
            SetUdonField(udon, "doorAnimator", animator);
        }

        /// Attach an UdonSharp behaviour by type name without a compile-time
        /// dependency on the VRChat SDK (this Editor script must compile in a
        /// bare Unity project too). Prefers UdonSharpEditor's
        /// AddUdonSharpComponent so the backing UdonBehaviour is created;
        /// falls back to a plain AddComponent; warns when the type isn't
        /// compiled (SDK missing) so the object is visibly inert, not
        /// silently so.
        static Component TryAttachUdon(GameObject go, string typeName)
        {
            System.Type t = null;
            foreach (var asm in System.AppDomain.CurrentDomain.GetAssemblies())
            {
                t = asm.GetType("LegaiaWorld." + typeName);
                if (t != null) break;
            }
            if (t == null)
            {
                Debug.LogWarning("[Legaia] " + typeName + " is not compiled " +
                    "(VRChat SDK / UdonSharp missing?) - " + go.name +
                    " stays inert until the behaviour is added manually.");
                return null;
            }
            var ext = System.Type.GetType(
                "UdonSharpEditor.UdonSharpComponentExtensions, UdonSharp.Editor");
            if (ext != null)
            {
                var mi = ext.GetMethod("AddUdonSharpComponent",
                    new[] { typeof(GameObject), typeof(System.Type) });
                if (mi != null)
                    return (Component)mi.Invoke(null, new object[] { go, t });
            }
            return go.AddComponent(t);
        }

        /// Set a public field on an attached Udon behaviour via reflection
        /// (keeps this file free of compile-time VRC SDK references).
        static void SetUdonField(Component comp, string field, object value)
        {
            if (comp == null) return;
            var f = comp.GetType().GetField(field);
            if (f == null) return;
            f.SetValue(comp, value);
            EditorUtility.SetDirty(comp);
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

        static string Sanitize(string s)
        {
            foreach (char c in Path.GetInvalidFileNameChars())
                s = s.Replace(c, '_');
            return s;
        }
    }
}
