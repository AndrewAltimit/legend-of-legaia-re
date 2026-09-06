// Headless checks for the kit, runnable without opening the editor UI:
//
//   Unity.exe -batchmode -nographics -quit -projectPath <project>
//       -executeMethod LegaiaWorld.LegaiaBatchChecks.CommonPrefabs
//       [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
// CommonPrefabs opens the scene, finds the built root's LegaiaSpawn,
// builds every common prefab next to it with the SDK types the project
// has, and asserts the result is fully wired: the expected components
// exist, every UdonSharp proxy has a backing UdonBehaviour, and the
// wired fields reached it. It never saves the scene (batch -quit drops
// the changes), so the project's scene file is untouched; generated
// assets under Assets/LegaiaGenerated/<scene>/ are refreshed, which a
// normal build does too. Prints "[Legaia] SELFTEST OK" on success and
// exits non-zero on the first failure so a script can gate on it.

using System.Collections.Generic;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaBatchChecks
    {
        static string Arg(string name, string fallback)
        {
            var args = System.Environment.GetCommandLineArgs();
            for (int i = 0; i + 1 < args.Length; i++)
                if (args[i] == name)
                    return args[i + 1];
            return fallback;
        }

        static string Path(Transform t)
        {
            string p = t.name;
            for (var u = t.parent; u != null; u = u.parent)
                p = u.name + "/" + p;
            return p;
        }

        static void Fail(string msg)
        {
            Debug.LogError("[Legaia] SELFTEST FAIL: " + msg);
            if (Application.isBatchMode)
                EditorApplication.Exit(1);
            throw new System.Exception(msg);
        }

        public static void CommonPrefabs()
        {
            string scenePath = Arg("-legaiaScene", "Assets/Scenes/VRCDefaultWorldScene.unity");
            EditorSceneManager.OpenScene(scenePath, OpenSceneMode.Single);

            GameObject spawn = null;
            foreach (var t in Object.FindObjectsOfType<Transform>())
                if (t.name == "LegaiaSpawn")
                {
                    spawn = t.gameObject;
                    break;
                }
            if (spawn == null)
                Fail("no LegaiaSpawn in " + scenePath + " - build the scene first");
            string sceneName = spawn.transform.parent != null &&
                               spawn.transform.parent.name.StartsWith("Legaia_")
                ? spawn.transform.parent.name.Substring("Legaia_".Length)
                : "selftest";

            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            var o = new LegaiaCommonPrefabOptions
            {
                mirror = true, tv = true, cardTable = true, seats = 4,
                sdkPens = AssetDatabase.LoadAssetAtPath<GameObject>(
                    LegaiaCommonPrefabs.SDK_PEN_PREFAB) != null,
            };
            var settings = LegaiaSceneSettings.Load(sceneName);
            var container = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + sceneName, spawn.transform.position, o,
                settings.prefabTransforms, settings.slotMachine);
            if (container == null)
                Fail("no container built");

            int Count(string typeName, GameObject scope = null)
            {
                var t = LegaiaWorldBuilder.FindType(typeName);
                if (t == null)
                    return -1;
                return (scope ?? container).GetComponentsInChildren(t, true).Length;
            }

            void Expect(string typeName, int n, GameObject scope = null)
            {
                int got = Count(typeName, scope);
                if (got != n)
                    Fail(typeName + ": expected " + n + ", found " + got +
                         (scope != null ? " under " + scope.name : ""));
            }

            // Counts that a spawned SDK prefab (the pen system carries its
            // own pickups) must not pollute are scoped to the kit object.
            var table = container.transform.Find("card_table")?.gameObject;
            if (table == null)
                Fail("no card_table under " + container.name);

            Expect("VRC.SDK3.Components.VRCMirrorReflection", 2);
            Expect("VRC.SDK3.Video.Components.VRCUnityVideoPlayer", 1);
            Expect("VRC.SDK3.Video.Components.AVPro.VRCAVProVideoPlayer", 1);
            Expect("VRC.SDK3.Video.Components.AVPro.VRCAVProVideoScreen", 1);
            Expect("VRC.SDK3.Video.Components.AVPro.VRCAVProVideoSpeaker", 1);
            Expect("VRC.SDK3.Components.VRCUrlInputField", 1);
            Expect("VRC.SDK3.Components.VRCStation", 4, table);
            Expect("VRC.SDK3.Components.VRCPickup", 52, table);
            Expect("VRC.SDK3.Components.VRCObjectSync", 52, table);
            Expect("LegaiaWorld.LegaiaCard", 52, table);
            Expect("LegaiaWorld.LegaiaSeat", 4);
            Expect("LegaiaWorld.LegaiaCardDeck", 1);
            Expect("LegaiaWorld.LegaiaMirror", 1);
            Expect("LegaiaWorld.LegaiaVideoTv", 1);
            Expect("LegaiaWorld.LegaiaEventButton", 3 + 3 + 2);
            if (o.sdkPens && Count("VRC.Udon.UdonBehaviour") < 1)
                Fail("SDK pen prefab spawned without any UdonBehaviour");

            // The SDK's world validator refuses a VRC Object Sync on the
            // same object as a manually synchronized Udon behaviour
            // ("Object Sync cannot share an object with a manually
            // synchronized Udon Behaviour") - catch it here, headless,
            // instead of at upload time.
            var objectSyncType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCObjectSync");
            var udonType = LegaiaWorldBuilder.FindType("VRC.Udon.UdonBehaviour");
            if (objectSyncType != null && udonType != null)
            {
                var syncProp = udonType.GetProperty("SyncMethod");
                if (syncProp == null)
                    Fail("UdonBehaviour has no SyncMethod property - the Object Sync check cannot run");
                int paired = 0;
                foreach (var os in container.GetComponentsInChildren(objectSyncType, true))
                    foreach (var ub in os.GetComponents(udonType))
                    {
                        paired++;
                        string method = syncProp.GetValue(ub)?.ToString() ?? "?";
                        if (method != "Continuous" && method != "None")
                            Fail(Path(os.transform) + " pairs a VRC Object Sync with a " + method +
                                 "-sync Udon behaviour (the SDK validator rejects that)");
                    }
                Debug.Log("[Legaia] selftest: " + paired + " Udon behaviour(s) share an object " +
                          "with a VRC Object Sync, none Manual-sync");
            }

            // Shading: every kit renderer is on a Legaia lit shader except
            // the display surfaces (mirror, video screen) and whatever a
            // spawned SDK prefab brought along.
            var slotRoot = container.transform.Find(LegaiaCommonPrefabs.SLOT_NAME);
            foreach (var r in container.GetComponentsInChildren<Renderer>(true))
            {
                if (PrefabUtility.IsPartOfPrefabInstance(r.gameObject))
                    continue; // spawned prefabs keep their own materials
                if (slotRoot != null && r.transform.IsChildOf(slotRoot))
                    continue; // the slot rig follows the slot builder's own rules
                foreach (var m in r.sharedMaterials)
                {
                    if (m == null || m.shader == null)
                        Fail(r.name + " has a null material");
                    string sh = m.shader.name;
                    bool display = sh.StartsWith("FX/") || sh.StartsWith("Video/");
                    // "Legaia/" = the lit family, "LegaiaWorld/" = the slot
                    // screen's own unlit display shaders.
                    if (!display && !sh.StartsWith("Legaia"))
                        Fail(r.name + " still shades with " + sh);
                }
            }
            // The mirror's runtime material swap must reference converted
            // (lit) materials, not the Standard originals.
            CheckVar(container, "LegaiaWorld.LegaiaMirror", "idleMaterial");

            // Slot machine: when the cabinet asset exists, the pass must
            // have placed it as the settings say and built the minigame.
            string slotAsset = settings.slotMachine?.cabinetAsset ?? o.slotCabinetPath;
            if (AssetDatabase.LoadAssetAtPath<GameObject>(slotAsset) != null)
            {
                var cab = container.transform.Find(LegaiaCommonPrefabs.SLOT_NAME);
                if (cab == null)
                    Fail("slot cabinet not placed under " + container.name);
                Expect("LegaiaWorld.LegaiaSlotMachine", 1, cab.gameObject);
                Expect("LegaiaWorld.LegaiaSlotButton", 3, cab.gameObject);
                var sp = settings.slotMachine;
                if (sp != null && sp.hasPosition &&
                    (cab.localPosition - sp.position).magnitude > 0.001f)
                    Fail("slot cabinet at " + cab.localPosition + ", settings say " + sp.position);
                if (sp != null && sp.hasRotation &&
                    Quaternion.Angle(cab.localRotation, Quaternion.Euler(sp.rotation)) > 0.01f)
                    Fail("slot cabinet rotated " + cab.localEulerAngles + ", settings say " + sp.rotation);
                if (sp != null && sp.hasScale && Mathf.Abs(cab.localScale.x - sp.scale) > 1e-4f)
                    Fail("slot cabinet scale " + cab.localScale.x + ", settings say " + sp.scale);
                int strays = 0;
                foreach (var t in Object.FindObjectsOfType<Transform>(true))
                    if (t.name == "LegaiaSlotGame" && !t.IsChildOf(container.transform))
                        strays++;
                if (strays > 0)
                    Fail(strays + " slot rig(s) left outside the container");
            }

            // Placements from the scene settings file must win over the
            // computed offsets (position AND rotation).
            var placements = LegaiaSceneSettings.Load(sceneName).prefabTransforms;
            foreach (var kv in placements)
            {
                var child = container.transform.Find(kv.Key == "pens" ? "sdk_pens" : kv.Key);
                if (child == null)
                    continue; // e.g. "menu" lives under the camp container
                if ((child.localPosition - kv.Value.position).magnitude > 0.001f)
                    Fail(kv.Key + " placed at " + child.localPosition + ", settings say " +
                         kv.Value.position);
                if (kv.Value.hasRotation &&
                    Quaternion.Angle(child.localRotation, Quaternion.Euler(kv.Value.rotation)) > 0.01f)
                    Fail(kv.Key + " rotated " + child.localEulerAngles + ", settings say " +
                         kv.Value.rotation);
            }

            // Every U# proxy must have a backing UdonBehaviour with a
            // program, and the wired references must have reached it.
            var usbType = LegaiaWorldBuilder.FindType("UdonSharp.UdonSharpBehaviour");
            if (usbType == null)
                Fail("UdonSharp not present - the checks above cannot mean anything");
            int proxies = 0;
            foreach (var proxy in container.GetComponentsInChildren(usbType, true))
            {
                proxies++;
                var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
                if (backing == null)
                    Fail(proxy.GetType().Name + " on " + proxy.name + " has no backing UdonBehaviour");
                var bt = backing.GetType();
                object prog = bt.GetField("programSource")?.GetValue(backing)
                              ?? bt.GetProperty("programSource")?.GetValue(backing);
                if (prog == null)
                    Fail(proxy.GetType().Name + " on " + proxy.name + " has no program source");
            }

            // Spot-check field wiring through the backing behaviour's
            // public variables (what actually runs in-world).
            CheckVar(container, "LegaiaWorld.LegaiaMirror", "highMirror");
            CheckVar(container, "LegaiaWorld.LegaiaMirror", "frame");
            var frameT = container.transform.Find("mirror/frame");
            if (frameT == null || frameT.gameObject.activeSelf)
                Fail("mirror frame must start hidden (it shows with the glass)");
            CheckVar(container, "LegaiaWorld.LegaiaVideoTv", "urlField");
            CheckVar(container, "LegaiaWorld.LegaiaVideoTv", "avproPlayer");
            CheckVar(container, "LegaiaWorld.LegaiaCardDeck", "cards");
            CheckVar(container, "LegaiaWorld.LegaiaCardDeck", "stackAnchor");
            CheckVar(container, "LegaiaWorld.LegaiaSeat", "station");
            CheckVar(container, "LegaiaWorld.LegaiaEventButton", "target");

            // The URL field must call the TV's backing behaviour on end-edit.
            var fieldType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCUrlInputField");
            var field = container.GetComponentInChildren(fieldType, true);
            var so = new SerializedObject(field);
            var calls = so.FindProperty("m_OnEndEdit.m_PersistentCalls.m_Calls");
            if (calls == null || calls.arraySize != 1)
                Fail("URL field has " + (calls == null ? -1 : calls.arraySize) +
                     " end-edit listener(s), expected 1");
            var call = calls.GetArrayElementAtIndex(0);
            if (call.FindPropertyRelative("m_MethodName").stringValue != "SendCustomEvent" ||
                call.FindPropertyRelative("m_Arguments.m_StringArgument").stringValue != "OnURLChanged" ||
                call.FindPropertyRelative("m_Target").objectReferenceValue == null)
                Fail("URL field listener is not SendCustomEvent(OnURLChanged) on a target");

            Debug.Log("[Legaia] SELFTEST OK: " + proxies + " U# proxies wired under " +
                      container.name + " (scene " + sceneName + ", not saved).");
        }

        /// Read `varName` off the backing UdonBehaviour of the first proxy
        /// of `typeName` (publicVariables.TryGetVariableValue) and fail when
        /// it is null - the symptom of a proxy edit that never reached Udon.
        static void CheckVar(GameObject container, string typeName, string varName)
        {
            var t = LegaiaWorldBuilder.FindType(typeName);
            var proxy = t != null ? container.GetComponentInChildren(t, true) : null;
            if (proxy == null)
                Fail("no " + typeName + " under " + container.name);
            var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
            if (backing == null)
                Fail(typeName + " has no backing behaviour");
            // UdonBehaviour.publicVariables is a field (Odin-serialized),
            // not a property.
            const System.Reflection.BindingFlags ANY =
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic |
                System.Reflection.BindingFlags.Instance;
            var bt = backing.GetType();
            object pv = bt.GetField("publicVariables", ANY)?.GetValue(backing)
                        ?? bt.GetProperty("publicVariables", ANY)?.GetValue(backing);
            if (pv == null)
                Fail(typeName + ": backing behaviour exposes no publicVariables");
            System.Reflection.MethodInfo tryGet = null;
            foreach (var mi in pv.GetType().GetMethods())
                if (mi.Name == "TryGetVariableValue" && !mi.IsGenericMethod &&
                    mi.GetParameters().Length == 2)
                {
                    tryGet = mi;
                    break;
                }
            if (tryGet == null)
                Fail("no TryGetVariableValue on " + pv.GetType().Name);
            var args = new object[] { varName, null };
            bool ok = (bool)tryGet.Invoke(pv, args);
            if (!ok || args[1] == null)
                Fail(typeName + "." + varName + " did not reach the backing UdonBehaviour");
            var arr = args[1] as System.Array;
            if (arr != null && arr.Length == 0)
                Fail(typeName + "." + varName + " is an empty array on the backing behaviour");
        }
    }
}
