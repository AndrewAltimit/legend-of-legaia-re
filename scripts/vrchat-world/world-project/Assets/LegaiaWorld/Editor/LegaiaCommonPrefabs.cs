// Common prefabs: the furniture every VRChat world seems to end up with,
// spawned near the built scene's spawn as an option of the builder.
//
// Three are built HERE from primitives, generated textures and the SDK's
// own components (no third-party package, no game data):
//
// - Mirror: two VRC mirror surfaces (full quality / players-only) in a
//   wooden frame, three buttons on a side post, off by default and
//   local (LegaiaMirror).
// - TV: the SDK's AVPro (PC) + Unity (Android) video players on one
//   screen, a URL field + status line, Play/Pause - Stop - Resync
//   buttons, owner-synced playhead (LegaiaVideoTv). YouTube links work
//   through VRChat's own resolver in the client.
// - Card table: a round table, N stools that are VRC stations (sit on
//   Interact, LegaiaSeat) and a 52-card deck of pickups with generated
//   faces (LegaiaCard) plus Shuffle / Gather buttons (LegaiaCardDeck).
//   The faces are drawn into one atlas at build time in the standard
//   playing-card arrangement - a ten carries ten pips, the lower half
//   of them upside down - so a card reads as itself across the felt;
//   LegaiaBatchChecks.CommonPrefabs counts the ink to prove it.
//
// The rest are spawned FROM PREFABS: the SDK's own sample pen system
// when the package ships it, and any prefab assets the user lists in
// the builder (QvPen, a ProTV / USharpVideo player, a community deck -
// whatever is installed in the project). The kit never bundles those:
// the README records what exists and under what licence.
//
// Everything lands in a top-level "Legaia_common_prefabs" container at
// the origin, deliberately outside the mirrored scene root (world-space
// UI text under the X-flip renders mirror-written, pickups and stations
// under a negative scale misbehave) - so an object's Inspector position
// IS its world position, which is what the per-scene settings file's
// prefab_transforms override stores (Legaia > Snapshot placements
// captures it from a hand-placed scene).
//
// Shading: after the build every generated material is converted to the
// kit's lit vertex-colour shaders (LegaiaRealism.ConvertPropToLit, the
// same pass the slot-machine cabinet gets) so the furniture sits under
// the scene's sun + ambient instead of Standard-PBR defaults; the mirror
// surfaces and the video screen keep their display shaders.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEditor.Events;
using UnityEngine;
using UnityEngine.UI;

namespace LegaiaWorld
{
    [System.Serializable]
    internal class LegaiaExtraPrefab
    {
        public GameObject prefab;
        public Vector3 offset = new Vector3(3f, 0f, -3.5f);
        public float yaw;
    }

    [System.Serializable]
    internal class LegaiaCommonPrefabOptions
    {
        public bool mirror = true;
        public Vector3 mirrorOffset = new Vector3(-4.6f, 0f, -1.6f);
        public bool tv = true;
        public Vector3 tvOffset = new Vector3(4.8f, 0f, 1.4f);
        public bool cardTable = true;
        public Vector3 cardTableOffset = new Vector3(0.4f, 0f, -5.4f);
        public int seats = 4;
        public bool sdkPens = false;
        public Vector3 pensOffset = new Vector3(-3.2f, 0f, 3.4f);
        // The casino cabinet + minigame: the model asset (a glb the user
        // keeps under Assets/Prefabs/), the `asset slot-art` folder, and
        // where it stands when the scene settings carry no slot_machine
        // block. Scale: the shipped cabinet is authored in centimetre-ish
        // units, 0.012 lands it at a real cabinet's height.
        public bool slotMachine = true;
        public string slotCabinetPath = "Assets/Prefabs/legaia slot machine.glb";
        public string slotArtDir = "Assets/LegaiaImports/slot-art";
        public Vector3 slotOffset = new Vector3(-3.4f, 0f, 3.6f);
        public float slotScale = 0.012f;
        public List<LegaiaExtraPrefab> extras = new List<LegaiaExtraPrefab>();

        public bool AnyEnabled => mirror || tv || cardTable || sdkPens || slotMachine ||
                                  extras.Exists(e => e != null && e.prefab != null);
    }

    internal static class LegaiaCommonPrefabs
    {
        internal const string CONTAINER = "Legaia_common_prefabs";
        internal const string SDK_PEN_PREFAB =
            "Packages/com.vrchat.worlds/Samples/UdonExampleScene/Prefabs/SimplePenSystem.prefab";
        const string SDK_MIRROR_MAT =
            "Packages/com.vrchat.base/Runtime/VRCSDK/Sample Assets/Materials/MirrorReflection.mat";

        // VRChat's fixed layers: Player 9, PlayerLocal 10, MirrorReflection 18.
        const int LOW_MIRROR_MASK = (1 << 9) | (1 << 10) | (1 << 18);

        internal static void Remove()
        {
            var old = GameObject.Find(CONTAINER);
            if (old != null)
                Undo.DestroyObjectImmediate(old);
        }

        /// Build every enabled prefab near `spawnW` (world space). The
        /// world colliders must already exist (ground snapping raycasts).
        /// `positions` (from the scene settings file) overrides the
        /// spawn-relative offset per item with an absolute world position.
        internal static GameObject Build(string genDir, Vector3 spawnW,
            LegaiaCommonPrefabOptions o, Dictionary<string, LegaiaPrefabTransform> placements,
            LegaiaSlotPlacement slot = null, List<LegaiaPosterRef> posters = null)
        {
            Remove();
            var container = new GameObject(CONTAINER);
            Undo.RegisterCreatedObjectUndo(container, "Build Legaia common prefabs");
            Directory.CreateDirectory(genDir);

            // Computed placement (grounded offset, facing the spawn) unless
            // the scene settings carry a hand-placed entry for the key.
            Vector3 Place(string key, Vector3 offset)
            {
                LegaiaPrefabTransform p;
                if (placements != null && placements.TryGetValue(key, out p))
                    return p.position;
                return LegaiaCampProps.Ground(spawnW + offset);
            }
            void Finish(GameObject go, string key)
            {
                LegaiaSceneSettings.ApplyPlacement(placements, key, go.transform);
            }

            var built = new List<string>();
            // The coin purse first: every minigame below pays into it, and
            // the ones built by other passes find it by this path at
            // runtime (Legaia_common_prefabs/wallet - keep the name).
            BuildWallet(container);
            if (o.mirror)
            {
                Finish(BuildMirror(container, genDir, Place("mirror", o.mirrorOffset), spawnW),
                    "mirror");
                built.Add("mirror");
            }
            if (o.tv)
            {
                Finish(BuildTv(container, genDir, Place("tv", o.tvOffset), spawnW), "tv");
                built.Add("TV");
            }
            if (o.cardTable)
            {
                Finish(BuildCardTable(container, genDir,
                    Place("card_table", o.cardTableOffset), spawnW,
                    Mathf.Clamp(o.seats, 0, 8), placements), "card_table");
                built.Add("card table");
            }
            if (o.sdkPens)
            {
                var pens = AssetDatabase.LoadAssetAtPath<GameObject>(SDK_PEN_PREFAB);
                if (pens == null)
                    Debug.LogWarning("[Legaia] SDK pen prefab not found at " +
                        SDK_PEN_PREFAB + " - is the VRChat worlds SDK installed?");
                else
                {
                    var inst = Spawn(container, pens, Place("pens", o.pensOffset), spawnW, 0f);
                    if (inst != null)
                    {
                        inst.name = PENS_NAME;
                        Finish(inst, "pens");
                        built.Add("SDK pens");
                    }
                }
            }
            foreach (var e in o.extras)
            {
                if (e == null || e.prefab == null)
                    continue;
                var inst = Spawn(container, e.prefab,
                    Place(e.prefab.name, e.offset), spawnW, e.yaw);
                if (inst != null)
                {
                    Finish(inst, e.prefab.name);
                    built.Add(e.prefab.name);
                }
            }

            // Shade like the scene: every generated Standard material
            // becomes a lit vertex-colour variant (mirror surfaces and the
            // video screen are skipped by the converter). The mirror's
            // button highlight swaps materials at runtime, so its fields
            // must point at the converted variants, not the originals.
            if (o.slotMachine)
            {
                var cab = BuildSlotMachine(container, spawnW, o, slot);
                if (cab != null)
                    built.Add("slot machine");
            }

            // Posters LAST of the kit-built things: the wall search needs
            // the object each poster hangs near (the card table) to stand
            // already, and the print materials must exist before the lit
            // conversion sweeps the container. `posters` is optional so
            // every existing caller keeps its signature - when it is null
            // the list is read from the same scene settings file the
            // caller loaded, keyed by the scene name genDir ends with.
            int hung = LegaiaPosters.Build(container, genDir, placements,
                posters ?? LegaiaSceneSettings.Load(SceneNameOf(genDir)).posters);
            if (hung > 0)
                built.Add(hung + " poster(s)");

            // Spawned prefabs (the SDK pens, a QvPen, a ProTV) keep their
            // authored materials - only the kit-built furniture converts.
            LegaiaRealism.ConvertPropToLit(container, genDir,
                r => !PrefabUtility.IsPartOfPrefabInstance(r.gameObject));
            RewireMirrorMaterials(container);

            Debug.Log("[Legaia] common prefabs: " + string.Join(", ", built) +
                      " placed near spawn.");
            return container;
        }

        /// Every caller builds `genDir` as "Assets/LegaiaGenerated/<scene>",
        /// so its last segment is the scene the settings file is named for.
        static string SceneNameOf(string genDir)
        {
            return Path.GetFileName(genDir.Replace("\\", "/").TrimEnd('/'));
        }

        const string PENS_NAME = "sdk_pens";
        internal const string SLOT_NAME = "slot_machine";
        const string SLOT_RIG = "LegaiaSlotGame";

        /// Place the casino cabinet (settings block first, builder fields
        /// otherwise) and run the slot-machine builder on it. A cabinet of
        /// the same asset left at the scene root by an earlier hand
        /// placement is retired first - its placement is what the settings
        /// block now carries - so a rebuild never leaves two machines.
        static GameObject BuildSlotMachine(GameObject container, Vector3 spawnW,
            LegaiaCommonPrefabOptions o, LegaiaSlotPlacement slot)
        {
            string cabinetPath = slot != null && !string.IsNullOrEmpty(slot.cabinetAsset)
                ? slot.cabinetAsset : o.slotCabinetPath;
            string art = slot != null && !string.IsNullOrEmpty(slot.artDir)
                ? slot.artDir : o.slotArtDir;
            var asset = AssetDatabase.LoadAssetAtPath<GameObject>(cabinetPath);
            if (asset == null)
            {
                // The path moved: find the model by file name anywhere in
                // the project (a folder rename should not lose the cabinet).
                string stem = Path.GetFileNameWithoutExtension(cabinetPath);
                foreach (string guid in AssetDatabase.FindAssets(stem + " t:GameObject"))
                {
                    string p = AssetDatabase.GUIDToAssetPath(guid);
                    if (Path.GetFileNameWithoutExtension(p) == stem)
                    {
                        asset = AssetDatabase.LoadAssetAtPath<GameObject>(p);
                        if (asset != null)
                        {
                            Debug.LogWarning("[Legaia] slot cabinet not at " + cabinetPath +
                                " - using " + p + " (update the settings file).");
                            break;
                        }
                    }
                }
            }
            if (asset == null)
            {
                Debug.LogWarning("[Legaia] slot cabinet asset not found (" + cabinetPath +
                    ") - drop the cabinet model under Assets/Prefabs/ or untick " +
                    "the slot machine.");
                return null;
            }

            RetireStrayCabinet(container, asset);

            Vector3 pos = slot != null && slot.hasPosition
                ? slot.position
                : LegaiaCampProps.Ground(spawnW + o.slotOffset);
            var inst = Spawn(container, asset, pos, spawnW, 0f);
            if (inst == null)
                return null;
            inst.name = SLOT_NAME;
            if (slot != null && slot.hasRotation)
                inst.transform.localRotation = Quaternion.Euler(slot.rotation);
            inst.transform.localScale = Vector3.one *
                (slot != null && slot.hasScale ? slot.scale : o.slotScale);

            if (!Directory.Exists(art))
            {
                Debug.LogWarning("[Legaia] slot art folder not found (" + art +
                    ") - cabinet placed, minigame not built. Run `asset slot-art` " +
                    "into that folder and rebuild.");
                return inst;
            }
            if (!LegaiaSlotMachineBuilder.BuildFor(inst, art))
                Debug.LogWarning("[Legaia] slot machine build on " + inst.name +
                    " did not produce a rig - see the errors above.");
            return inst;
        }

        /// A hand-placed cabinet of the same asset outside the container
        /// (the pre-settings workflow: drag the glb in, run the slot tool)
        /// is removed, with a log line saying so.
        static void RetireStrayCabinet(GameObject container, GameObject asset)
        {
            // Collect first, destroy after: the FindObjectsOfType array
            // still holds the retired cabinet's children.
            var stray = new List<GameObject>();
            foreach (var t in Object.FindObjectsOfType<Transform>(true))
            {
                if (t == null || t.name != SLOT_RIG || t.IsChildOf(container.transform))
                    continue;
                var root = CabinetRootOf(t);
                if (root == null || root == t.gameObject || stray.Contains(root))
                    continue;
                var src = PrefabUtility.GetCorrespondingObjectFromOriginalSource(root);
                if (src == null || AssetDatabase.GetAssetPath(src) != AssetDatabase.GetAssetPath(asset))
                    continue;
                stray.Add(root);
            }
            foreach (var root in stray)
            {
                Debug.Log("[Legaia] retiring the hand-placed slot cabinet '" + root.name +
                    "' at " + root.transform.position + " - the settings-driven one " +
                    "under " + CONTAINER + " replaces it.");
                Undo.DestroyObjectImmediate(root);
            }
        }

        /// The cabinet a slot rig was built onto: the outermost prefab
        /// instance above the rig, else the rig's top-most ancestor.
        internal static GameObject CabinetRootOf(Transform rig)
        {
            for (var p = rig.parent; p != null; p = p.parent)
                if (PrefabUtility.IsPartOfPrefabInstance(p.gameObject))
                    return PrefabUtility.GetOutermostPrefabInstanceRoot(p.gameObject);
            return rig.root.gameObject;
        }

        /// The settings-file key a direct child of the container is stored
        /// under by Legaia > Snapshot placements: the kit objects by their
        /// names, the SDK pens as "pens", an extra prefab by its name.
        internal static string SettingsKey(GameObject child)
        {
            return child.name == PENS_NAME ? "pens" : child.name;
        }

        static void RewireMirrorMaterials(GameObject container)
        {
            var mirrorT = container.transform.Find("mirror");
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaMirror");
            var mirror = mirrorT != null && t != null ? mirrorT.GetComponent(t) : null;
            if (mirror == null)
                return;
            var off = mirrorT.Find("btn_off")?.GetComponent<Renderer>();
            var high = mirrorT.Find("btn_high")?.GetComponent<Renderer>();
            if (off == null || high == null)
                return;
            LegaiaWorldBuilder.SetUdonField(mirror, "activeMaterial", off.sharedMaterial);
            LegaiaWorldBuilder.SetUdonField(mirror, "idleMaterial", high.sharedMaterial);
            LegaiaWorldBuilder.SyncUdonProxy(mirror);
        }

        // --- Shared helpers ----------------------------------------------------

        /// Root rotation so that a Quad / canvas child (visible from its
        /// -Z side) looks toward the spawn: forward points AWAY from it.
        static Quaternion FacingAwayFrom(Vector3 pos, Vector3 spawnW)
        {
            Vector3 away = pos - spawnW;
            away.y = 0f;
            return away.sqrMagnitude > 1e-6f
                ? Quaternion.LookRotation(away.normalized)
                : Quaternion.identity;
        }

        static GameObject Prim(PrimitiveType type, string name, Transform parent,
            Vector3 localPos, Vector3 localScale, Material mat, bool keepCollider = false)
        {
            var go = GameObject.CreatePrimitive(type);
            go.name = name;
            if (!keepCollider)
                Object.DestroyImmediate(go.GetComponent<Collider>());
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            go.transform.localScale = localScale;
            if (mat != null)
                go.GetComponent<MeshRenderer>().sharedMaterial = mat;
            return go;
        }

        /// Set one serialized property on an SDK component by name (keeps
        /// this file free of compile-time SDK references). Enums take an
        /// int; arrays of objects take an Object[].
        static void SetProp(Component c, string name, object value)
        {
            if (c == null) return;
            var so = new SerializedObject(c);
            var p = so.FindProperty(name);
            if (p == null)
            {
                Debug.LogWarning("[Legaia] " + c.GetType().Name + " has no property '" +
                    name + "' - SDK version drift?");
                return;
            }
            switch (value)
            {
                case bool b: p.boolValue = b; break;
                case int i:
                    if (p.propertyType == SerializedPropertyType.Enum) p.enumValueIndex = i;
                    else p.intValue = i;
                    break;
                case float f: p.floatValue = f; break;
                case string s: p.stringValue = s; break;
                case Object obj: p.objectReferenceValue = obj; break;
                case Object[] arr:
                    p.arraySize = arr.Length;
                    for (int k = 0; k < arr.Length; k++)
                        p.GetArrayElementAtIndex(k).objectReferenceValue = arr[k];
                    break;
                default:
                    Debug.LogWarning("[Legaia] SetProp: unsupported value for " + name);
                    return;
            }
            so.ApplyModifiedPropertiesWithoutUndo();
        }

        static Component AddSdk(GameObject go, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType(typeName);
            if (t == null)
            {
                Debug.LogWarning("[Legaia] " + typeName + " not found (VRChat SDK " +
                    "missing?) - " + go.name + " is built without it.");
                return null;
            }
            return go.AddComponent(t);
        }

        /// A collider button that sends `eventName` into `target` on Interact.
        static GameObject Button(Transform parent, string name, Vector3 localPos,
            Vector3 size, Material mat, Component target, string eventName, string prompt)
        {
            var go = Prim(PrimitiveType.Cube, name, parent, localPos, size, mat, true);
            var b = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaEventButton");
            LegaiaWorldBuilder.SetUdonField(b, "target", target);
            LegaiaWorldBuilder.SetUdonField(b, "eventName", eventName);
            LegaiaWorldBuilder.SetUdonField(b, "interactText", prompt);
            LegaiaWorldBuilder.SyncUdonProxy(b);
            return go;
        }

        static GameObject Spawn(GameObject container, GameObject prefab,
            Vector3 pos, Vector3 spawnW, float yaw)
        {
            GameObject inst;
            if (PrefabUtility.IsPartOfPrefabAsset(prefab))
                inst = PrefabUtility.InstantiatePrefab(prefab) as GameObject;
            else
            {
                Debug.LogWarning("[Legaia] " + prefab.name + " is not a prefab asset - " +
                    "drop a prefab from the Project window, not a scene object.");
                return null;
            }
            if (inst == null)
                return null;
            Undo.RegisterCreatedObjectUndo(inst, "Spawn prefab");
            inst.transform.SetParent(container.transform, true);
            inst.transform.position = pos;
            inst.transform.rotation = Quaternion.Euler(0f, yaw, 0f) *
                Quaternion.LookRotation(-(FacingAwayFrom(pos, spawnW) * Vector3.forward));
            return inst;
        }

        // --- Mirror -------------------------------------------------------------

        static GameObject BuildMirror(GameObject container, string genDir, Vector3 pos, Vector3 spawnW)
        {
            var wood = LegaiaCampProps.EnsureMat(genDir, "camp_wood", "Standard",
                new Color(0.36f, 0.24f, 0.13f));
            var dark = LegaiaCampProps.EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            var idle = LegaiaCampProps.EnsureMat(genDir, "button_idle", "Standard",
                new Color(0.42f, 0.4f, 0.36f));
            var active = EnsureEmissive(genDir, "button_active",
                new Color(0.95f, 0.8f, 0.35f), new Color(0.6f, 0.45f, 0.1f));

            var root = new GameObject("mirror");
            root.transform.SetParent(container.transform, false);
            root.transform.position = pos;
            root.transform.rotation = FacingAwayFrom(pos, spawnW);

            // Frame: shown only while a surface is on (LegaiaMirror toggles
            // it), so the mirror leaves no wall standing when it is off.
            var frame = Prim(PrimitiveType.Cube, "frame", root.transform,
                new Vector3(0f, 1.15f, 0f), new Vector3(1.72f, 2.3f, 0.06f), wood, true);
            frame.SetActive(false);

            var mirrorMat = AssetDatabase.LoadAssetAtPath<Material>(SDK_MIRROR_MAT);
            if (mirrorMat == null)
            {
                var sh = Shader.Find("FX/MirrorReflection") ?? Shader.Find("Standard");
                mirrorMat = LegaiaCampProps.EnsureMat(genDir, "mirror_surface", sh.name,
                    Color.white);
            }
            var high = MirrorSurface(root.transform, "surface_high", mirrorMat, -1);
            var low = MirrorSurface(root.transform, "surface_low", mirrorMat, LOW_MIRROR_MASK);

            // Button post beside the frame.
            Prim(PrimitiveType.Cube, "post", root.transform,
                new Vector3(1.05f, 0.6f, 0f), new Vector3(0.12f, 1.2f, 0.12f), dark, true);

            var mirror = LegaiaWorldBuilder.TryAttachUdon(root, "LegaiaMirror");
            var size = new Vector3(0.1f, 0.07f, 0.05f);
            var bOff = Button(root.transform, "btn_off", new Vector3(1.05f, 1.0f, -0.08f),
                size, active, mirror, "SetOff", "Mirror off");
            var bHigh = Button(root.transform, "btn_high", new Vector3(1.05f, 0.86f, -0.08f),
                size, idle, mirror, "SetHigh", "Mirror on");
            var bLow = Button(root.transform, "btn_low", new Vector3(1.05f, 0.72f, -0.08f),
                size, idle, mirror, "SetLow", "Mirror on (players only)");

            LegaiaWorldBuilder.SetUdonField(mirror, "highMirror", high);
            LegaiaWorldBuilder.SetUdonField(mirror, "lowMirror", low);
            LegaiaWorldBuilder.SetUdonField(mirror, "frame", frame);
            LegaiaWorldBuilder.SetUdonField(mirror, "buttons", new[]
            {
                bOff.GetComponent<Renderer>(), bHigh.GetComponent<Renderer>(),
                bLow.GetComponent<Renderer>(),
            });
            LegaiaWorldBuilder.SetUdonField(mirror, "idleMaterial", idle);
            LegaiaWorldBuilder.SetUdonField(mirror, "activeMaterial", active);
            LegaiaWorldBuilder.SyncUdonProxy(mirror);
            return root;
        }

        static GameObject MirrorSurface(Transform root, string name, Material mat, int layers)
        {
            var q = Prim(PrimitiveType.Quad, name, root,
                new Vector3(0f, 1.15f, -0.035f), new Vector3(1.58f, 2.18f, 1f), mat);
            var refl = AddSdk(q, "VRC.SDK3.Components.VRCMirrorReflection");
            SetProp(refl, "m_ReflectLayers", layers);
            SetProp(refl, "m_DisablePixelLights", true);
            SetProp(refl, "TurnOffMirrorOcclusion", true);
            q.SetActive(false); // LegaiaMirror turns one on per button press
            return q;
        }

        static Material EnsureEmissive(string genDir, string name, Color c, Color emission)
        {
            var m = LegaiaCampProps.EnsureMat(genDir, name, "Standard", c);
            m.EnableKeyword("_EMISSION");
            m.SetColor("_EmissionColor", emission);
            m.globalIlluminationFlags = MaterialGlobalIlluminationFlags.RealtimeEmissive;
            return m;
        }

        // --- TV -----------------------------------------------------------------

        static GameObject BuildTv(GameObject container, string genDir, Vector3 pos, Vector3 spawnW)
        {
            var dark = LegaiaCampProps.EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            var wood = LegaiaCampProps.EnsureMat(genDir, "camp_wood", "Standard",
                new Color(0.36f, 0.24f, 0.13f));
            var green = LegaiaCampProps.EnsureMat(genDir, "button_green", "Standard",
                new Color(0.25f, 0.6f, 0.3f));
            var red = LegaiaCampProps.EnsureMat(genDir, "button_red", "Standard",
                new Color(0.65f, 0.22f, 0.2f));
            var blue = LegaiaCampProps.EnsureMat(genDir, "button_blue", "Standard",
                new Color(0.25f, 0.4f, 0.65f));

            var root = new GameObject("tv");
            root.transform.SetParent(container.transform, false);
            root.transform.position = pos;
            root.transform.rotation = FacingAwayFrom(pos, spawnW);

            Prim(PrimitiveType.Cube, "cabinet", root.transform,
                new Vector3(0f, 0.4f, 0f), new Vector3(1.1f, 0.8f, 0.4f), wood, true);
            Prim(PrimitiveType.Cube, "neck", root.transform,
                new Vector3(0f, 0.83f, 0f), new Vector3(0.12f, 0.08f, 0.12f), dark);
            Prim(PrimitiveType.Cube, "bezel", root.transform,
                new Vector3(0f, 1.35f, 0f), new Vector3(1.72f, 1.0f, 0.06f), dark, true);

            // Screen: the SDK's video shader (what its own sample players
            // draw with) over a black idle texture. Both players write
            // this material's _MainTex.
            var screenMat = EnsureScreenMaterial(genDir);
            var screen = Prim(PrimitiveType.Quad, "screen", root.transform,
                new Vector3(0f, 1.35f, -0.035f), new Vector3(1.6f, 0.9f, 1f), screenMat);
            var screenR = screen.GetComponent<Renderer>();

            // Speaker: one spatial AudioSource shared by both players.
            var spk = new GameObject("speaker");
            spk.transform.SetParent(root.transform, false);
            spk.transform.localPosition = new Vector3(0f, 0.75f, -0.15f);
            var src = spk.AddComponent<AudioSource>();
            src.playOnAwake = false;
            src.spatialBlend = 1f;
            src.volume = 0.8f;
            src.minDistance = 1.5f;
            src.maxDistance = 16f;
            src.rolloffMode = AudioRolloffMode.Linear;
            LegaiaAudioGen.AddVrcSpatial(spk, true, 10f, 1.5f, 16f);

            var unity = AddSdk(root, "VRC.SDK3.Video.Components.VRCUnityVideoPlayer");
            SetProp(unity, "renderMode", 1); // MaterialOverride
            SetProp(unity, "targetMaterialRenderer", screenR);
            SetProp(unity, "targetMaterialProperty", "_MainTex");
            SetProp(unity, "targetAudioSources", new Object[] { src });
            SetProp(unity, "maximumResolution", 720);
            SetProp(unity, "autoPlay", false);
            SetProp(unity, "loop", false);

            var avpro = AddSdk(root, "VRC.SDK3.Video.Components.AVPro.VRCAVProVideoPlayer");
            SetProp(avpro, "maximumResolution", 720);
            SetProp(avpro, "autoPlay", false);
            SetProp(avpro, "loop", false);
            SetProp(avpro, "useLowLatency", false);
            if (avpro != null)
            {
                var avScreen = AddSdk(screen,
                    "VRC.SDK3.Video.Components.AVPro.VRCAVProVideoScreen");
                SetProp(avScreen, "videoPlayer", avpro);
                SetProp(avScreen, "materialIndex", 0);
                SetProp(avScreen, "textureProperty", "_MainTex");
                SetProp(avScreen, "useSharedMaterial", false);
                var avSpk = AddSdk(spk,
                    "VRC.SDK3.Video.Components.AVPro.VRCAVProVideoSpeaker");
                SetProp(avSpk, "videoPlayer", avpro);
                SetProp(avSpk, "mode", 0); // StereoMix
            }

            var tv = LegaiaWorldBuilder.TryAttachUdon(root, "LegaiaVideoTv");
            var backing = BackingUdon(tv);

            // Panel on the cabinet's front: URL field + status line.
            Text status;
            var urlField = BuildUrlPanel(root.transform, backing, out status);

            var size = new Vector3(0.14f, 0.06f, 0.09f);
            Button(root.transform, "btn_play", new Vector3(-0.3f, 0.83f, -0.12f),
                size, green, tv, "TogglePlay", "Play / Pause");
            Button(root.transform, "btn_stop", new Vector3(0f, 0.83f, -0.12f),
                size, red, tv, "StopVideo", "Stop");
            Button(root.transform, "btn_resync", new Vector3(0.3f, 0.83f, -0.12f),
                size, blue, tv, "Resync", "Resync");

            LegaiaWorldBuilder.SetUdonField(tv, "unityPlayer", unity);
            LegaiaWorldBuilder.SetUdonField(tv, "avproPlayer", avpro);
            LegaiaWorldBuilder.SetUdonField(tv, "urlField", urlField);
            LegaiaWorldBuilder.SetUdonField(tv, "statusText", status);
            LegaiaWorldBuilder.SyncUdonProxy(tv);
            return root;
        }

        static Material EnsureScreenMaterial(string genDir)
        {
            string texPath = genDir + "/tv_idle.png";
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                var tex = new Texture2D(4, 4, TextureFormat.RGBA32, false);
                var px = new Color[16];
                for (int i = 0; i < 16; i++) px[i] = new Color(0.02f, 0.02f, 0.03f, 1f);
                tex.SetPixels(px);
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
            }
            var shader = Shader.Find("Video/RealtimeEmissiveGamma")
                         ?? Shader.Find("Unlit/Texture");
            var m = LegaiaCampProps.EnsureMat(genDir, "tv_screen", shader.name, Color.white);
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            if (m.HasProperty("_Emission"))
                m.SetFloat("_Emission", 1f);
            m.EnableKeyword("_EMISSION");
            return m;
        }

        /// World-space canvas on the cabinet front with a VRCUrlInputField
        /// (end-edit -> OnURLChanged on the TV's backing UdonBehaviour) and
        /// a status Text. The canvas carries the VRCUiShape + its collider;
        /// nothing else near it has a collider in front of the UI plane.
        static Component BuildUrlPanel(Transform root, Component backing, out Text status)
        {
            var canvasGo = new GameObject("panel");
            canvasGo.transform.SetParent(root, false);
            canvasGo.transform.localPosition = new Vector3(0f, 0.5f, -0.205f);
            canvasGo.transform.localScale = Vector3.one * 0.001f;
            var canvas = canvasGo.AddComponent<Canvas>();
            canvas.renderMode = RenderMode.WorldSpace;
            var rt = canvasGo.GetComponent<RectTransform>();
            rt.sizeDelta = new Vector2(1000f, 300f);
            canvasGo.AddComponent<GraphicRaycaster>();
            var shapeType =
                LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCUiShape")
                ?? LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_UiShape");
            if (shapeType != null)
                canvasGo.AddComponent(shapeType);
            var cbox = canvasGo.AddComponent<BoxCollider>();
            cbox.size = new Vector3(1000f, 300f, 10f);

            var font = Resources.GetBuiltinResource<Font>("LegacyRuntime.ttf")
                       ?? Resources.GetBuiltinResource<Font>("Arial.ttf");

            var bg = new GameObject("background");
            bg.transform.SetParent(canvasGo.transform, false);
            var bgRt = bg.AddComponent<RectTransform>();
            bgRt.sizeDelta = new Vector2(1000f, 300f);
            bg.AddComponent<Image>().color = new Color(0.08f, 0.07f, 0.06f, 0.95f);

            // URL field.
            var fieldGo = new GameObject("url_field");
            fieldGo.transform.SetParent(canvasGo.transform, false);
            var frt = fieldGo.AddComponent<RectTransform>();
            frt.anchoredPosition = new Vector2(0f, 60f);
            frt.sizeDelta = new Vector2(940f, 110f);
            var fieldImg = fieldGo.AddComponent<Image>();
            fieldImg.color = new Color(0.95f, 0.93f, 0.88f, 1f);

            var placeholder = MakeText(fieldGo.transform, font, "Enter video URL (YouTube works)",
                40, Vector2.zero, new Vector2(900f, 100f), new Color(0.45f, 0.45f, 0.45f),
                TextAnchor.MiddleLeft);
            placeholder.name = "placeholder";
            placeholder.fontStyle = FontStyle.Italic;
            var text = MakeText(fieldGo.transform, font, "", 40, Vector2.zero,
                new Vector2(900f, 100f), new Color(0.1f, 0.1f, 0.1f), TextAnchor.MiddleLeft);
            text.name = "text";
            text.supportRichText = false;

            Component field = AddSdk(fieldGo, "VRC.SDK3.Components.VRCUrlInputField");
            if (field != null)
            {
                SetProp(field, "m_TargetGraphic", fieldImg);
                SetProp(field, "m_TextComponent", text);
                SetProp(field, "m_Placeholder", placeholder);
                SetProp(field, "AllowSendingOnEndEdit", true);
                var evtProp = field.GetType().GetProperty("onEndEdit");
                var evt = evtProp?.GetValue(field) as UnityEngine.Events.UnityEventBase;
                if (evt != null && backing != null)
                {
                    var action = (UnityEngine.Events.UnityAction<string>)
                        System.Delegate.CreateDelegate(
                            typeof(UnityEngine.Events.UnityAction<string>),
                            backing, "SendCustomEvent");
                    UnityEventTools.AddStringPersistentListener(evt, action, "OnURLChanged");
                    EditorUtility.SetDirty(field);
                }
            }

            status = MakeText(canvasGo.transform, font, "Enter a URL", 40,
                new Vector2(0f, -70f), new Vector2(940f, 90f), new Color(1f, 0.9f, 0.7f),
                TextAnchor.MiddleCenter);
            status.name = "status";
            return field;
        }

        static Text MakeText(Transform parent, Font font, string label, int size,
            Vector2 pos, Vector2 dims, Color color, TextAnchor anchor)
        {
            var go = new GameObject("text");
            go.transform.SetParent(parent, false);
            var rt = go.AddComponent<RectTransform>();
            rt.anchoredPosition = pos;
            rt.sizeDelta = dims;
            var text = go.AddComponent<Text>();
            text.font = font;
            text.fontSize = size;
            text.text = label;
            text.color = color;
            text.alignment = anchor;
            text.horizontalOverflow = HorizontalWrapMode.Overflow;
            return text;
        }

        /// The backing UdonBehaviour of a U# proxy (UI listeners must
        /// target it - a listener on the proxy does nothing in-world).
        internal static Component BackingUdon(Component proxy)
        {
            if (proxy == null)
                return null;
            var util = LegaiaWorldBuilder.FindType("UdonSharpEditor.UdonSharpEditorUtility");
            var mi = util?.GetMethod("GetBackingUdonBehaviour");
            if (mi == null)
                return null;
            try
            {
                return mi.Invoke(null, new object[] { proxy }) as Component;
            }
            catch (System.Exception e)
            {
                Debug.LogWarning("[Legaia] GetBackingUdonBehaviour failed: " +
                    (e.InnerException ?? e).Message);
                return null;
            }
        }

        // --- Wallet --------------------------------------------------------------

        internal const string WALLET_NAME = "wallet";

        /// The per-player coin purse (LegaiaWallet): one behaviour, no
        /// visuals, persisted through VRChat PlayerData. Consumers built
        /// here are wired to it; consumers built by other passes resolve
        /// `GameObject.Find("Legaia_common_prefabs/wallet")` in their Start.
        static Component BuildWallet(GameObject container)
        {
            var go = new GameObject(WALLET_NAME);
            go.transform.SetParent(container.transform, false);
            var wallet = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaWallet");
            LegaiaWorldBuilder.SyncUdonProxy(wallet);
            return wallet;
        }

        /// The wallet built by this pass (null when the container is not built).
        internal static Component FindWallet()
        {
            var c = GameObject.Find(CONTAINER + "/" + WALLET_NAME);
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaWallet");
            return c != null && t != null ? c.GetComponent(t) : null;
        }

        // --- Card table ---------------------------------------------------------

        const float CARD_W = 0.063f, CARD_L = 0.088f, CARD_T = 0.0005f;

        static GameObject BuildCardTable(GameObject container, string genDir, Vector3 pos,
            Vector3 spawnW, int seats, Dictionary<string, LegaiaPrefabTransform> placements = null)
        {
            var wood = LegaiaCampProps.EnsureMat(genDir, "camp_wood", "Standard",
                new Color(0.36f, 0.24f, 0.13f));
            var dark = LegaiaCampProps.EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            var felt = LegaiaCampProps.EnsureMat(genDir, "table_felt", "Standard",
                new Color(0.12f, 0.36f, 0.18f));

            var root = new GameObject("card_table");
            root.transform.SetParent(container.transform, false);
            root.transform.position = pos;
            root.transform.rotation = FacingAwayFrom(pos, spawnW);

            // Table: cylinder primitives carry a capsule collider, useless
            // for a flat top - swap in convex mesh colliders.
            var top = Prim(PrimitiveType.Cylinder, "top", root.transform,
                new Vector3(0f, 0.74f, 0f), new Vector3(1.24f, 0.02f, 1.24f), wood);
            top.AddComponent<MeshCollider>().convex = true;
            var feltGo = Prim(PrimitiveType.Cylinder, "felt", root.transform,
                new Vector3(0f, 0.762f, 0f), new Vector3(1.1f, 0.004f, 1.1f), felt);
            feltGo.AddComponent<MeshCollider>().convex = true;
            Prim(PrimitiveType.Cylinder, "pedestal", root.transform,
                new Vector3(0f, 0.37f, 0f), new Vector3(0.16f, 0.37f, 0.16f), dark, true);
            var foot = Prim(PrimitiveType.Cylinder, "foot", root.transform,
                new Vector3(0f, 0.02f, 0f), new Vector3(0.7f, 0.02f, 0.7f), dark);
            foot.AddComponent<MeshCollider>().convex = true;

            var pickupType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCPickup");
            var syncType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCObjectSync");

            var cardMat = EnsureCardMaterial(genDir);
            var meshes = EnsureCardMeshes(genDir);

            // Stools: a VRC station for players, and a LegaiaNpcStation
            // (kind 2) so the town's villagers take a free seat when
            // nobody is playing - the host below arbitrates between them.
            var seatStations = new List<Component>();
            var seatChairs = new List<Component>();
            for (int i = 0; i < seats; i++)
                BuildStool(root.transform, i, seats, wood, dark,
                    seatStations, seatChairs);

            // Deck: 52 pickups stacked face-down at the anchor.
            var anchor = new GameObject("deck_anchor");
            anchor.transform.SetParent(root.transform, false);
            anchor.transform.localPosition = new Vector3(0f, 0.766f + CARD_T, 0f);
            var deck = LegaiaWorldBuilder.TryAttachUdon(root, "LegaiaCardDeck");

            var cardsGo = new GameObject("cards");
            cardsGo.transform.SetParent(root.transform, false);
            var cards = new List<Component>();
            var cardTransforms = new List<Transform>();
            for (int i = 0; i < 52; i++)
            {
                var card = BuildCard(cardsGo.transform, i, meshes[i], cardMat,
                    anchor.transform.localPosition + Vector3.up * (0.0007f * i),
                    pickupType, syncType);
                if (card != null)
                {
                    cards.Add(card);
                    cardTransforms.Add(card.transform);
                }
            }

            // The NPC host: one behaviour for all four seat stations. It
            // frees them the moment a player sits down or a card leaves the
            // deck, and the button shoos the villagers off on demand.
            var host = LegaiaWorldBuilder.TryAttachUdon(root, "LegaiaCardTableHost");
            LegaiaWorldBuilder.SetUdonField(host, "seats",
                ToTypedArray(seatStations, "LegaiaNpcStation"));
            LegaiaWorldBuilder.SetUdonField(host, "seatChairs",
                ToTypedArray(seatChairs, "LegaiaSeat"));
            LegaiaWorldBuilder.SetUdonField(host, "deckAnchor", anchor.transform);
            LegaiaWorldBuilder.SetUdonField(host, "cards", cardTransforms.ToArray());
            LegaiaWorldBuilder.SyncUdonProxy(host);
            foreach (var st in seatStations)
            {
                LegaiaWorldBuilder.SetUdonField(st, "handler", host);
                LegaiaWorldBuilder.SyncUdonProxy(st);
            }

            // NO BUTTONS ON THE FELT. Shuffle / Gather / the NPC toggle
            // used to be three collider cubes standing on the table top,
            // where they sat inside the reach of a seated player's cards
            // and looked like furniture rather than controls. They are UI
            // buttons on the seat panel now (LegaiaCardGameBuilder), which
            // is also where every other table control already lives -
            // one surface to read, one surface to press.

            LegaiaWorldBuilder.SetUdonField(deck, "cards", ToTypedArray(cards, "LegaiaCard"));
            LegaiaWorldBuilder.SetUdonField(deck, "stackAnchor", anchor.transform);
            LegaiaWorldBuilder.SyncUdonProxy(deck);

            // The game itself (dealer, AI opponents, betting, the seat
            // panel): own file, see its header. It hangs a `game` child
            // off the table root - the director links to it by that path.
            LegaiaCardGameBuilder.Build(root, genDir, spawnW, host, deck,
                seatStations, seatChairs, cards, FindWallet(), placements);
            return root;
        }

        /// A `LegaiaCard[]` for the deck's field, built without naming the
        /// U# type at compile time.
        static System.Array ToTypedArray(List<Component> comps, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + typeName);
            if (t == null)
                return null;
            var arr = System.Array.CreateInstance(t, comps.Count);
            for (int i = 0; i < comps.Count; i++)
                arr.SetValue(comps[i], i);
            return arr;
        }

        static void BuildStool(Transform root, int i, int n, Material wood, Material dark,
            List<Component> seatStations, List<Component> seatChairs)
        {
            float a = (i + 0.5f) / n * Mathf.PI * 2f;
            Vector3 local = new Vector3(Mathf.Sin(a) * 0.98f, 0f, Mathf.Cos(a) * 0.98f);
            var stool = new GameObject("stool_" + i);
            stool.transform.SetParent(root, false);
            stool.transform.localPosition = local;
            // Face the table centre.
            stool.transform.localRotation = Quaternion.LookRotation(-local.normalized);
            Prim(PrimitiveType.Cylinder, "leg", stool.transform,
                new Vector3(0f, 0.22f, 0f), new Vector3(0.08f, 0.22f, 0.08f), dark);
            Prim(PrimitiveType.Cylinder, "seat", stool.transform,
                new Vector3(0f, 0.45f, 0f), new Vector3(0.38f, 0.025f, 0.38f), wood);
            var box = stool.AddComponent<BoxCollider>();
            box.center = new Vector3(0f, 0.24f, 0f);
            box.size = new Vector3(0.4f, 0.48f, 0.4f);

            var enter = new GameObject("Seat");
            enter.transform.SetParent(stool.transform, false);
            enter.transform.localPosition = new Vector3(0f, 0.5f, 0f);
            var exit = new GameObject("Exit");
            exit.transform.SetParent(stool.transform, false);
            exit.transform.localPosition = new Vector3(0f, 0f, -0.7f);

            var station = AddSdk(stool, "VRC.SDK3.Components.VRCStation");
            SetProp(station, "PlayerMobility", 1); // Immobilize (the SDK chair's setting)
            SetProp(station, "canUseStationFromStation", true);
            SetProp(station, "seated", true);
            SetProp(station, "disableStationExit", false);
            SetProp(station, "stationEnterPlayerLocation", enter.transform);
            SetProp(station, "stationExitPlayerLocation", exit.transform);

            var seat = LegaiaWorldBuilder.TryAttachUdon(stool, "LegaiaSeat");
            LegaiaWorldBuilder.SetUdonField(seat, "station", station);
            LegaiaWorldBuilder.SyncUdonProxy(seat);

            // The villagers' side of the same stool: a LegaiaNpcStation the
            // town director can send an idle NPC to. The stool already
            // faces the table centre, so its own transform is the stand
            // point (position AND facing); the host wires the handler.
            var npcSeat = LegaiaWorldBuilder.TryAttachUdon(stool, "LegaiaNpcStation");
            LegaiaWorldBuilder.SetUdonField(npcSeat, "kind", 2); // seat
            LegaiaWorldBuilder.SetUdonField(npcSeat, "standPoint", stool.transform);
            LegaiaWorldBuilder.SetUdonField(npcSeat, "dwellSeconds", 90f);
            LegaiaWorldBuilder.SetUdonField(npcSeat, "indoors", false);
            LegaiaWorldBuilder.SyncUdonProxy(npcSeat);

            // No cosmetic card fan on the stool: the rigs have no hand
            // bone, so a fan hung off the stool at hand height landed on the
            // FOREHEAD of a seated villager (they are about a metre tall).
            // The villager's real hand is the LegaiaCardGame deal onto the
            // felt in front of it.

            seatStations.Add(npcSeat);
            seatChairs.Add(seat);
        }

        static Component BuildCard(Transform parent, int index, Mesh mesh, Material mat,
            Vector3 localPos, System.Type pickupType, System.Type syncType)
        {
            var go = new GameObject("card_" + CardName(index));
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;

            var visual = new GameObject("face");
            visual.transform.SetParent(go.transform, false);
            visual.transform.localRotation = Quaternion.Euler(0f, 0f, 180f); // face down
            visual.AddComponent<MeshFilter>().sharedMesh = mesh;
            var mr = visual.AddComponent<MeshRenderer>();
            mr.sharedMaterial = mat;
            mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;

            // A fat grab box: the card itself is half a millimetre thick,
            // and the pointer has to hit something.
            var box = go.AddComponent<BoxCollider>();
            box.size = new Vector3(CARD_W, 0.008f, CARD_L);
            box.center = new Vector3(0f, 0.003f, 0f); // sits on the felt, not in it
            var rb = go.AddComponent<Rigidbody>();
            rb.mass = 0.02f;
            rb.isKinematic = true; // LegaiaCard frees it on first drop
            rb.collisionDetectionMode = CollisionDetectionMode.ContinuousDynamic;
            rb.interpolation = RigidbodyInterpolation.Interpolate;

            if (pickupType != null)
            {
                var pickup = go.AddComponent(pickupType);
                var autoHold = pickupType.GetField("AutoHold");
                if (autoHold != null && autoHold.FieldType.IsEnum)
                    autoHold.SetValue(pickup, System.Enum.Parse(autoHold.FieldType, "Yes"));
                pickupType.GetField("UseText")?.SetValue(pickup, "Flip");
                pickupType.GetField("InteractionText")?.SetValue(pickup, "Card");
                if (syncType != null)
                    go.AddComponent(syncType);
            }
            var card = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaCard");
            LegaiaWorldBuilder.SetUdonField(card, "visual", visual.transform);
            LegaiaWorldBuilder.SyncUdonProxy(card);
            return card;
        }

        static readonly string[] SUITS = { "spades", "hearts", "diamonds", "clubs" };
        static readonly string[] RANKS =
            { "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K" };

        static string CardName(int i) => RANKS[i % 13] + "_" + SUITS[i / 13];

        // Atlas: 13 rank columns x (4 suit rows + 1 row for back / edge).
        //
        // Cell size is exactly 2x the kit's first pass (96x136). A ten has
        // to carry four pip rows per column plus two interstitial centre
        // pips AND a corner index at each end; at 96x136 a pip small
        // enough to fit ten of them was three pixels of ink, which is why
        // the first pass drew one big centre pip and left the count to the
        // digits. 192x272 is ~30 px per centimetre of a 6.3 x 8.8 cm card,
        // which still reads through the mip chain at the half-metre to
        // metre a seated player looks from.
        //
        // That makes the atlas 2496x1360, past the importer's 2048 default
        // cap - EnsureCardMaterial raises maxTextureSize to 4096 and, more
        // importantly, turns npotScale OFF: the default ToNearest was
        // resampling even the OLD 1248x680 atlas down to 1024x512, so the
        // first pass never reached the GPU at the density it drew. 13x5
        // cells can never tile a power-of-two texture, so None is the only
        // setting that keeps the grid.
        //
        // COLS/ROWS must NOT change: Cell() turns them into the UVs baked
        // into cards_meshes.asset. CW/CH appear nowhere in that asset, so
        // a cell-size change alone leaves the cached meshes valid.
        const int CW = 192, CH = 272, COLS = 13, ROWS = 5;

        // EnsureCardMaterial only draws when the PNG is missing, so a
        // project that already built the old faces would keep them for
        // ever. The file name carries the layout version instead; bump it
        // (and add the old name to the obsolete list) on every face change.
        const string CARD_ATLAS = "cards_atlas_v2.png";
        static readonly string[] CARD_ATLAS_OBSOLETE = { "cards_atlas.png" };

        internal static string CardAtlasPath(string genDir) => genDir + "/" + CARD_ATLAS;

        static Material EnsureCardMaterial(string genDir)
        {
            string texPath = CardAtlasPath(genDir);
            foreach (string stale in CARD_ATLAS_OBSOLETE)
                if (AssetDatabase.LoadAssetAtPath<Texture2D>(genDir + "/" + stale) != null)
                    AssetDatabase.DeleteAsset(genDir + "/" + stale);
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                var tex = new Texture2D(CW * COLS, CH * ROWS, TextureFormat.RGBA32, false);
                var px = new Color[tex.width * tex.height];
                for (int r = 0; r < 4; r++)
                    for (int c = 0; c < COLS; c++)
                        DrawCardFace(px, tex.width, tex.height, c, r);
                DrawCardBack(px, tex.width, tex.height, 0, 4);
                DrawCardEdge(px, tex.width, tex.height, 1, 4);
                tex.SetPixels(px);
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
                var imp = AssetImporter.GetAtPath(texPath) as TextureImporter;
                if (imp != null)
                {
                    imp.alphaIsTransparency = true;
                    imp.wrapMode = TextureWrapMode.Clamp;
                    imp.mipmapEnabled = true;
                    // Cutout shader: without coverage-preserving mips the
                    // pips and the card silhouette dissolve with distance.
                    imp.mipMapsPreserveCoverage = true;
                    imp.alphaTestReferenceValue = 0.5f;
                    // A card on the felt is read at a grazing angle, which
                    // is exactly where bilinear-only sampling smears the
                    // corner index into paper.
                    imp.filterMode = FilterMode.Trilinear;
                    imp.anisoLevel = 8;
                    imp.npotScale = TextureImporterNPOTScale.None;
                    imp.maxTextureSize = 4096;
                    imp.SaveAndReimport();
                }
            }
            var shader = Shader.Find("Legacy Shaders/Transparent/Cutout/Diffuse")
                         ?? Shader.Find("Standard");
            var m = LegaiaCampProps.EnsureMat(genDir, "cards", shader.name, Color.white);
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            if (m.HasProperty("_Cutoff"))
                m.SetFloat("_Cutoff", 0.5f);
            return m;
        }

        /// Pixel (x, y) of atlas cell (c, r), r counted from the TOP row.
        static void Px(Color[] px, int w, int h, int c, int r, int x, int y, Color col)
        {
            if (x < 0 || y < 0 || x >= CW || y >= CH) return;
            int ax = c * CW + x;
            int ay = h - 1 - (r * CH + y);
            px[ay * w + ax] = col;
        }

        /// Ink over whatever is already there, with `a` coverage. Blending
        /// is what lets the analytic pip tests be supersampled instead of
        /// stair-stepped; a pixel outside the card body stays transparent.
        static void Blend(Color[] px, int w, int h, int c, int r, int x, int y,
            Color col, float a)
        {
            if (x < 0 || y < 0 || x >= CW || y >= CH || a <= 0f) return;
            int ax = c * CW + x;
            int ay = h - 1 - (r * CH + y);
            var dst = px[ay * w + ax];
            if (dst.a <= 0f) return;
            px[ay * w + ax] = Color.Lerp(dst, col, Mathf.Clamp01(a));
        }

        static bool InRoundedRect(int x, int y, int w, int h, int rad)
        {
            int cx = x < rad ? rad : (x >= w - rad ? w - 1 - rad : x);
            int cy = y < rad ? rad : (y >= h - rad ? h - 1 - rad : y);
            int dx = x - cx, dy = y - cy;
            return dx * dx + dy * dy <= rad * rad;
        }

        // --- Face layout ------------------------------------------------
        //
        // Every coordinate below is a fraction of CW / CH, so the whole
        // face survives another cell-size change; the batch check reads
        // the same helpers, so "the pips are where the counter looks" is
        // one definition, not two.

        const float PIP_TOP = 0.16f, PIP_BOT = 0.84f; // pip field, top / bottom row
        const float PIP_COL = 0.315f;                 // side pip columns from each edge

        static int Rad() => Mathf.Max(3, Mathf.RoundToInt(CH * 0.059f));
        static int IndexX() => Mathf.RoundToInt(CW * 0.115f);  // corner column centre
        static int IndexY() => Mathf.RoundToInt(CH * 0.045f);  // rank glyph top
        static int IndexPipY() => Mathf.RoundToInt(CH * 0.22f);
        static int IndexPipS() => Mathf.Max(3, Mathf.RoundToInt(CH * 0.032f));
        static int BodyPipS() => Mathf.Max(4, Mathf.RoundToInt(CH * 0.058f));
        static int AcePipS() => Mathf.Max(8, Mathf.RoundToInt(CH * 0.16f));
        static int ColLeft() => Mathf.RoundToInt(CW * PIP_COL);
        static int ColRight() => CW - Mathf.RoundToInt(CW * PIP_COL);

        /// Index glyph scale. "10" is the only two-glyph rank and it drops
        /// a step so the corner column stays as narrow as the single-digit
        /// ranks - the same thing a real deck does with a condensed 1.
        static int IndexScale(string rank) =>
            Mathf.Max(2, Mathf.RoundToInt(CW * (rank.Length > 1 ? 0.023f : 0.026f)));

        /// The corner-index block, top-left; the bottom-right one is its
        /// 180-degree mirror. Nothing else on the face may enter it - the
        /// pip counter excludes exactly these two rectangles, so anything
        /// that leaks out of one counts as a pip.
        internal static RectInt CardIndexRegion() =>
            new RectInt(0, 0, Mathf.RoundToInt(CW * 0.22f), Mathf.RoundToInt(CH * 0.28f));

        /// The court cards' frame, and the width of its border stroke.
        internal static RectInt CardCourtFrame() =>
            new RectInt(Mathf.RoundToInt(CW * 0.24f), Mathf.RoundToInt(CH * 0.21f),
                Mathf.RoundToInt(CW * 0.52f), Mathf.RoundToInt(CH * 0.58f));

        internal static int CardCourtStroke() => Mathf.Max(2, Mathf.RoundToInt(CW * 0.021f));

        internal static void CardAtlasLayout(out int cw, out int ch, out int cols, out int rows)
        {
            cw = CW; ch = CH; cols = COLS; rows = ROWS;
        }

        /// Cell-local (x, y), y down from the cell's top - the same
        /// mapping Px() writes with, so a check can read what was drawn.
        internal static Color CardPixel(Color[] px, int w, int h, int c, int r, int x, int y)
            => px[(h - 1 - (r * CH + y)) * w + c * CW + x];

        /// Ink vs paper. Paper is near-white, both inks are dark or deep
        /// red; the midpoint leaves the supersampled pip edges on the ink
        /// side down to about a third coverage.
        internal static bool CardIsInk(Color c)
            => c.a > 0.5f && (c.r + c.g + c.b) / 3f < 0.7f;

        /// Standard pip layouts for 2..10, as (column, row) pairs: column
        /// -1 / 0 / +1 = left / centre / right, row 0..1 spanning the pip
        /// field. The four-row columns of 9 and 10 sit at thirds, 6-8 use
        /// halves, and the centre pips of 7, 8 and 10 sit BETWEEN two
        /// column rows - that interstitial placement is what makes a real
        /// ten read as 4 + 4 + 2 instead of as a grid.
        static readonly float[][] PIP_LAYOUT =
        {
            new[] { 0f, 0f, 0f, 1f },                                                    // 2
            new[] { 0f, 0f, 0f, 0.5f, 0f, 1f },                                          // 3
            new[] { -1f, 0f, 1f, 0f, -1f, 1f, 1f, 1f },                                  // 4
            new[] { -1f, 0f, 1f, 0f, 0f, 0.5f, -1f, 1f, 1f, 1f },                        // 5
            new[] { -1f, 0f, 1f, 0f, -1f, 0.5f, 1f, 0.5f, -1f, 1f, 1f, 1f },             // 6
            new[] { -1f, 0f, 1f, 0f, 0f, 0.25f, -1f, 0.5f, 1f, 0.5f, -1f, 1f, 1f, 1f },  // 7
            new[] { -1f, 0f, 1f, 0f, 0f, 0.25f, -1f, 0.5f, 1f, 0.5f, 0f, 0.75f,          // 8
                    -1f, 1f, 1f, 1f },
            new[] { -1f, 0f, 1f, 0f, -1f, 1f / 3f, 1f, 1f / 3f, 0f, 0.5f,                // 9
                    -1f, 2f / 3f, 1f, 2f / 3f, -1f, 1f, 1f, 1f },
            new[] { -1f, 0f, 1f, 0f, 0f, 1f / 6f, -1f, 1f / 3f, 1f, 1f / 3f,             // 10
                    -1f, 2f / 3f, 1f, 2f / 3f, 0f, 5f / 6f, -1f, 1f, 1f, 1f },
        };

        static void DrawCardFace(Color[] px, int w, int h, int col, int row)
        {
            bool red = row == 1 || row == 2;
            Color ink = red ? new Color(0.78f, 0.1f, 0.1f) : new Color(0.1f, 0.1f, 0.12f);
            Color paper = new Color(0.97f, 0.96f, 0.92f);
            for (int y = 0; y < CH; y++)
                for (int x = 0; x < CW; x++)
                    Px(px, w, h, col, row, x, y,
                        InRoundedRect(x, y, CW, CH, Rad()) ? paper : Color.clear);
            string rank = RANKS[col];

            // Corner index: the rank glyph with its suit pip directly under
            // it on ONE column, top-left, and the same column rotated 180
            // at the bottom-right - what a player reads off a fanned hand.
            int k = IndexScale(rank);
            int gw = GlyphWidth(rank, k);
            DrawRank(px, w, h, col, row, rank, IndexX() - gw / 2, IndexY(), k, ink, false);
            DrawPip(px, w, h, col, row, row, IndexX(), IndexPipY(), IndexPipS(), ink, false);
            DrawRank(px, w, h, col, row, rank, CW - 1 - IndexX() + gw / 2,
                CH - 1 - IndexY(), k, ink, true);
            DrawPip(px, w, h, col, row, row, CW - 1 - IndexX(), CH - 1 - IndexPipY(),
                IndexPipS(), ink, true);

            if (col >= 10)
            {
                // Court: a framed big letter with one pip under it.
                var f = CardCourtFrame();
                int t = CardCourtStroke();
                for (int y = f.yMin; y < f.yMax; y++)
                    for (int x = f.xMin; x < f.xMax; x++)
                        if (y < f.yMin + t || y >= f.yMax - t ||
                            x < f.xMin + t || x >= f.xMax - t)
                            Px(px, w, h, col, row, x, y, ink);
                int lk = Mathf.Max(3, Mathf.RoundToInt(CW * 0.052f));
                DrawRank(px, w, h, col, row, rank, CW / 2 - GlyphWidth(rank, lk) / 2,
                    f.yMin + Mathf.RoundToInt(f.height * 0.12f), lk, ink, false);
                DrawPip(px, w, h, col, row, row, CW / 2,
                    f.yMax - Mathf.RoundToInt(f.height * 0.22f), BodyPipS(), ink, false);
            }
            else if (col == 0)
            {
                // Ace: one large pip, centred.
                DrawPip(px, w, h, col, row, row, CW / 2, CH / 2, AcePipS(), ink, false);
            }
            else
            {
                // 2..10: the standard arrangement, lower half upside down.
                float[] layout = PIP_LAYOUT[col - 1];
                int yTop = Mathf.RoundToInt(CH * PIP_TOP);
                int yBot = Mathf.RoundToInt(CH * PIP_BOT);
                for (int i = 0; i + 1 < layout.Length; i += 2)
                {
                    int cx = layout[i] < -0.5f ? ColLeft()
                        : layout[i] > 0.5f ? ColRight() : CW / 2;
                    float ty = layout[i + 1];
                    int cy = Mathf.RoundToInt(yTop + ty * (yBot - yTop));
                    DrawPip(px, w, h, col, row, row, cx, cy, BodyPipS(), ink, ty > 0.5f);
                }
            }
        }

        static void DrawCardBack(Color[] px, int w, int h, int col, int row)
        {
            Color teal = new Color(0.10f, 0.40f, 0.44f);
            Color light = new Color(0.55f, 0.78f, 0.80f);
            Color border = new Color(0.94f, 0.93f, 0.88f);
            int rim0 = Mathf.Max(2, Mathf.RoundToInt(CW * 0.052f));
            int step = Mathf.Max(6, Mathf.RoundToInt(CW * 0.125f));
            int bar = Mathf.Max(2, step / 6);
            for (int y = 0; y < CH; y++)
                for (int x = 0; x < CW; x++)
                {
                    if (!InRoundedRect(x, y, CW, CH, Rad()))
                    {
                        Px(px, w, h, col, row, x, y, Color.clear);
                        continue;
                    }
                    bool rim = x < rim0 || y < rim0 || x >= CW - rim0 || y >= CH - rim0;
                    bool lattice = (x + y) % step < bar || (x - y + 10000) % step < bar;
                    Px(px, w, h, col, row, x, y, rim ? border : (lattice ? light : teal));
                }
        }

        static void DrawCardEdge(Color[] px, int w, int h, int col, int row)
        {
            Color paper = new Color(0.93f, 0.92f, 0.88f);
            for (int y = 0; y < CH; y++)
                for (int x = 0; x < CW; x++)
                    Px(px, w, h, col, row, x, y, paper);
        }

        /// Suit pips as pixel tests: 0 spade, 1 heart, 2 diamond, 3 club.
        static void DrawPip(Color[] px, int w, int h, int col, int row, int suit,
            int cx, int cy, int s, Color ink, bool flip)
        {
            // 3x3 supersample: the suit tests are analytic, so the only
            // thing between a clean curve and a staircase is coverage -
            // and at ten pips a card the staircase is what you notice.
            const int SS = 3;
            int ry = s + s / 2 + 2, rx = s + 2;
            for (int y = -ry; y <= ry; y++)
                for (int x = -rx; x <= rx; x++)
                {
                    int hits = 0;
                    for (int sy = 0; sy < SS; sy++)
                        for (int sx = 0; sx < SS; sx++)
                        {
                            float fx = (x + (sx + 0.5f) / SS - 0.5f) / s;
                            float fy = (y + (sy + 0.5f) / SS - 0.5f) / s;
                            if (PipTest(suit, fx, flip ? -fy : fy))
                                hits++;
                        }
                    if (hits > 0)
                        Blend(px, w, h, col, row, cx + x, cy + y, ink,
                            hits / (float)(SS * SS));
                }
        }

        /// (fx, fy) in pip units, +fy DOWN the card.
        static bool PipTest(int suit, float fx, float fy)
        {
            switch (suit)
            {
                case 2: // diamond
                    return Mathf.Abs(fx) / 0.75f + Mathf.Abs(fy) <= 1f;
                case 1: // heart: two lobes up top, point at the bottom
                    return Heart(fx, fy);
                case 0: // spade: heart upside down + stem
                    return Heart(fx, -fy - 0.15f) || Stem(fx, fy);
                default: // club: three lobes + stem
                {
                    float r = 0.42f;
                    bool lobes = Sq(fx) + Sq(fy + 0.45f) <= r * r
                                 || Sq(fx - 0.42f) + Sq(fy + 0.02f) <= r * r
                                 || Sq(fx + 0.42f) + Sq(fy + 0.02f) <= r * r;
                    return lobes || Stem(fx, fy);
                }
            }
        }

        static float Sq(float v) => v * v;

        static bool Heart(float fx, float fy)
        {
            float r = 0.5f;
            bool lobes = Sq(fx - 0.45f) + Sq(fy + 0.35f) <= r * r
                         || Sq(fx + 0.45f) + Sq(fy + 0.35f) <= r * r;
            // Lower triangle from the lobe centres down to the tip.
            bool tri = fy >= -0.35f && fy <= 1.0f &&
                       Mathf.Abs(fx) <= 0.95f * (1f - (fy + 0.35f) / 1.35f);
            return lobes || tri;
        }

        static bool Stem(float fx, float fy)
        {
            return Mathf.Abs(fx) <= 0.12f + Mathf.Max(0f, fy - 0.4f) * 0.5f
                   && fy >= 0.3f && fy <= 1.05f;
        }

        // 5x7 bitmap glyphs for the rank characters.
        static readonly Dictionary<char, string[]> GLYPHS = new Dictionary<char, string[]>
        {
            { '0', new[] { "01110", "10001", "10011", "10101", "11001", "10001", "01110" } },
            { '1', new[] { "00100", "01100", "00100", "00100", "00100", "00100", "01110" } },
            { '2', new[] { "01110", "10001", "00001", "00010", "00100", "01000", "11111" } },
            { '3', new[] { "11111", "00010", "00100", "00010", "00001", "10001", "01110" } },
            { '4', new[] { "00010", "00110", "01010", "10010", "11111", "00010", "00010" } },
            { '5', new[] { "11111", "10000", "11110", "00001", "00001", "10001", "01110" } },
            { '6', new[] { "00110", "01000", "10000", "11110", "10001", "10001", "01110" } },
            { '7', new[] { "11111", "00001", "00010", "00100", "01000", "01000", "01000" } },
            { '8', new[] { "01110", "10001", "10001", "01110", "10001", "10001", "01110" } },
            { '9', new[] { "01110", "10001", "10001", "01111", "00001", "00010", "01100" } },
            { 'A', new[] { "01110", "10001", "10001", "11111", "10001", "10001", "10001" } },
            { 'J', new[] { "00111", "00010", "00010", "00010", "00010", "10010", "01100" } },
            { 'Q', new[] { "01110", "10001", "10001", "10001", "10101", "10010", "01101" } },
            { 'K', new[] { "10001", "10010", "10100", "11000", "10100", "10010", "10001" } },
        };

        /// Inked column range of a glyph. Advancing by the glyph's OWN
        /// width instead of a fixed 5 is what keeps "10" as narrow as a
        /// single digit: the '1' bitmap only uses three columns.
        static void GlyphBounds(string[] g, out int lo, out int hi)
        {
            lo = 5; hi = -1;
            for (int gy = 0; gy < 7; gy++)
                for (int gx = 0; gx < 5; gx++)
                    if (g[gy][gx] == '1')
                    {
                        if (gx < lo) lo = gx;
                        if (gx > hi) hi = gx;
                    }
            if (hi < lo) { lo = 0; hi = 0; }
        }

        /// Width in pixels of `rank` at scale `k` - one blank column of
        /// gap between glyphs, none after the last. Callers need it to
        /// CENTRE the index on its column instead of carrying a per-rank
        /// offset, which is what the old "10" special case was.
        static int GlyphWidth(string rank, int k)
        {
            int cursor = 0;
            foreach (char ch in rank)
            {
                string[] g;
                if (!GLYPHS.TryGetValue(ch, out g))
                    continue;
                GlyphBounds(g, out int lo, out int hi);
                cursor += (hi - lo + 2) * k;
            }
            return cursor <= 0 ? 0 : cursor - k;
        }

        /// Draw `rank` at (x0, y0) top-left with pixel scale `k`; `flip`
        /// draws it rotated 180 degrees with (x0, y0) as the bottom-right.
        static void DrawRank(Color[] px, int w, int h, int col, int row, string rank,
            int x0, int y0, int k, Color ink, bool flip)
        {
            int cursor = 0;
            foreach (char ch in rank)
            {
                string[] g;
                if (!GLYPHS.TryGetValue(ch, out g))
                    continue;
                GlyphBounds(g, out int lo, out int hi);
                for (int gy = 0; gy < 7; gy++)
                    for (int gx = lo; gx <= hi; gx++)
                    {
                        if (g[gy][gx] != '1')
                            continue;
                        for (int sy = 0; sy < k; sy++)
                            for (int sx = 0; sx < k; sx++)
                            {
                                int dx = cursor + (gx - lo) * k + sx, dy = gy * k + sy;
                                if (flip)
                                    Px(px, w, h, col, row, x0 - dx, y0 - dy, ink);
                                else
                                    Px(px, w, h, col, row, x0 + dx, y0 + dy, ink);
                            }
                    }
                cursor += (hi - lo + 2) * k;
            }
        }

        /// One box mesh per card: top face = the card's atlas cell, bottom
        /// = the back, four sides = the plain edge cell. Saved together in
        /// one asset so the scene can reference them.
        static Mesh[] EnsureCardMeshes(string genDir)
        {
            string path = genDir + "/cards_meshes.asset";
            var meshes = new Mesh[52];
            var existing = AssetDatabase.LoadAllAssetsAtPath(path);
            if (existing != null && existing.Length >= 52)
            {
                int found = 0;
                foreach (var obj in existing)
                {
                    var mesh = obj as Mesh;
                    if (mesh == null || !mesh.name.StartsWith("card_"))
                        continue;
                    int idx = System.Array.IndexOf(CardNames(), mesh.name.Substring(5));
                    if (idx >= 0 && meshes[idx] == null)
                    {
                        meshes[idx] = mesh;
                        found++;
                    }
                }
                if (found == 52)
                    return meshes;
            }
            AssetDatabase.DeleteAsset(path);
            for (int i = 0; i < 52; i++)
            {
                meshes[i] = CardMesh(i);
                meshes[i].name = "card_" + CardName(i);
                if (i == 0)
                    AssetDatabase.CreateAsset(meshes[i], path);
                else
                    AssetDatabase.AddObjectToAsset(meshes[i], path);
            }
            AssetDatabase.SaveAssets();
            return meshes;
        }

        static string[] CardNames()
        {
            var names = new string[52];
            for (int i = 0; i < 52; i++)
                names[i] = CardName(i);
            return names;
        }

        static Rect Cell(int c, int r)
        {
            float u0 = c / (float)COLS, u1 = (c + 1) / (float)COLS;
            float v1 = 1f - r / (float)ROWS, v0 = 1f - (r + 1) / (float)ROWS;
            return new Rect(u0, v0, u1 - u0, v1 - v0);
        }

        static Mesh CardMesh(int index)
        {
            var verts = new List<Vector3>();
            var norms = new List<Vector3>();
            var uvs = new List<Vector2>();
            var tris = new List<int>();
            float hw = CARD_W / 2f, ht = CARD_T / 2f, hl = CARD_L / 2f;
            Rect face = Cell(index % 13, index / 13);
            Rect back = Cell(0, 4);
            Rect edge = Cell(1, 4);
            // Shrink the edge sample to the cell interior so filtering
            // never bleeds a neighbour in.
            edge = new Rect(edge.x + edge.width * 0.3f, edge.y + edge.height * 0.3f,
                edge.width * 0.4f, edge.height * 0.4f);

            void Face(Vector3 a, Vector3 b, Vector3 c, Vector3 d, Vector3 n, Rect uv)
            {
                // a b c d clockwise seen from outside (Unity front face).
                int i0 = verts.Count;
                verts.Add(a); verts.Add(b); verts.Add(c); verts.Add(d);
                for (int k = 0; k < 4; k++) norms.Add(n);
                uvs.Add(new Vector2(uv.xMin, uv.yMax));
                uvs.Add(new Vector2(uv.xMax, uv.yMax));
                uvs.Add(new Vector2(uv.xMax, uv.yMin));
                uvs.Add(new Vector2(uv.xMin, uv.yMin));
                tris.Add(i0); tris.Add(i0 + 1); tris.Add(i0 + 2);
                tris.Add(i0); tris.Add(i0 + 2); tris.Add(i0 + 3);
            }

            // Top (+Y): card top edge toward +Z. Seen from above with +Z
            // up on screen, clockwise = (-x,+z) (+x,+z) (+x,-z) (-x,-z).
            Face(new Vector3(-hw, ht, hl), new Vector3(hw, ht, hl),
                new Vector3(hw, ht, -hl), new Vector3(-hw, ht, -hl), Vector3.up, face);
            // Bottom (-Y): the back, mirrored so it reads right from below.
            Face(new Vector3(-hw, -ht, -hl), new Vector3(hw, -ht, -hl),
                new Vector3(hw, -ht, hl), new Vector3(-hw, -ht, hl), Vector3.down, back);
            // Sides.
            Face(new Vector3(-hw, ht, hl), new Vector3(-hw, ht, -hl),
                new Vector3(-hw, -ht, -hl), new Vector3(-hw, -ht, hl), Vector3.left, edge);
            Face(new Vector3(hw, ht, -hl), new Vector3(hw, ht, hl),
                new Vector3(hw, -ht, hl), new Vector3(hw, -ht, -hl), Vector3.right, edge);
            Face(new Vector3(hw, ht, hl), new Vector3(-hw, ht, hl),
                new Vector3(-hw, -ht, hl), new Vector3(hw, -ht, hl), Vector3.forward, edge);
            Face(new Vector3(-hw, ht, -hl), new Vector3(hw, ht, -hl),
                new Vector3(hw, -ht, -hl), new Vector3(-hw, -ht, -hl), Vector3.back, edge);

            var m = new Mesh();
            m.SetVertices(verts);
            m.SetNormals(norms);
            m.SetUVs(0, uvs);
            m.SetTriangles(tris, 0);
            m.RecalculateBounds();
            return m;
        }
    }
}
