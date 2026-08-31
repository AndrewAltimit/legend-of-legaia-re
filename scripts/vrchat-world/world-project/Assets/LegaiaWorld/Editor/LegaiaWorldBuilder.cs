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
// Orientation note: the exported glbs are parity-correct - the site pages'
// view chain nets a 180-degree screen rotation over the baked frame (model
// Y-flip + projection Y-flip + retail screen-X mirror = three reflections,
// the bake carries one), NOT a mirror. So the raw import reads "flipped" at
// first glance next to the explorer pages but every asymmetric detail is
// right, and the fix is a rotation, never a mirror (a -1 axis scale flips
// building parity for real). "Match explorer orientation" (default on)
// rotates the finished root 180 degrees about Y so the scene sits the way
// the site's field-scene viewer presents it.

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

        string manifestPath = "";
        bool matchExplorerOrientation = true;
        bool addWorldColliders = true;
        bool mergedWorldCollider = true;
        bool addNpcCapsules = true;
        bool loopNpcClips = true;
        bool loopPropClips = true;
        bool hideStaticPropTwins = true;
        bool includeConditionalNpcs = false;

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
                    "Rotate the built root 180 degrees about Y so the scene sits " +
                    "the way the site's field-scene viewer presents it. Never " +
                    "mirror instead - the glbs are parity-correct, and a negative " +
                    "axis scale genuinely flips the buildings"),
                matchExplorerOrientation);
            addWorldColliders = EditorGUILayout.Toggle("World mesh colliders", addWorldColliders);
            using (new EditorGUI.DisabledScope(!addWorldColliders))
            {
                mergedWorldCollider = EditorGUILayout.Toggle(
                    new GUIContent("  Merged + welded (recommended)",
                        "One welded collision mesh for the whole world instead of a " +
                        "collider per mesh - closes the hairline seams between tile " +
                        "colliders that a player capsule can slip through, and works " +
                        "in client builds without enabling Read/Write on the glb"),
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

            // --- Spawn marker (assign to your VRC scene descriptor) ---
            Vector3 spawn = G2U(MiniJson.GetVec3(MiniJson.Get(m, "spawn"), "position"));
            var spawnGo = new GameObject("LegaiaSpawn");
            spawnGo.transform.SetParent(root.transform, false);
            spawnGo.transform.localPosition = spawn + Vector3.up * 0.1f;

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
                go.transform.localScale = Vector3.one * scale;
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
                    go.transform.localScale = Vector3.one * scale;
                    go.transform.localPosition = G2U(MiniJson.GetVec3(inst, "position"));
                    float yaw = MiniJson.GetNum(inst, "rot_y_radians") * Mathf.Rad2Deg;
                    go.transform.localRotation = Quaternion.Euler(0, YAW_SIGN * yaw, 0);
                    if (loopPropClips)
                        AttachLoopingClip(go, dir + "/" + file, null, dir, sceneName);
                    if (hideStaticPropTwins)
                        HideStaticTwin(world, file, go.transform.localPosition);
                    propCount++;
                }
            }

            // Rotate the whole assembly at the very end: children keep their
            // manifest-frame local transforms, the root turns them into the
            // explorer pages' presentation as one unit. A rotation, never a
            // mirror - the glbs are parity-correct.
            if (matchExplorerOrientation)
                root.transform.localRotation = Quaternion.Euler(0f, 180f, 0f);

            Selection.activeGameObject = root;
            Debug.Log("[Legaia] built " + sceneName + ": world + " + npcCount +
                      " NPC(s) + " + propCount + " animated prop instance(s). " +
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
