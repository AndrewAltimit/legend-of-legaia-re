// Headless checks for the kit, runnable without opening the editor UI:
//
//   Unity.exe -batchmode -nographics -quit -projectPath <project>
//       -executeMethod LegaiaWorld.LegaiaBatchChecks.CommonPrefabs
//       [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
// Weather does the same for the weather rig, the shoreline fishing
// stations and the card table's NPC seating: it applies those passes to
// the already-built root, rebuilds the common prefabs, and asserts the
// wiring reached the backing behaviours.
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
            Expect("LegaiaWorld.LegaiaEventButton", 3 + 3 + 3);
            Expect("LegaiaWorld.LegaiaNpcStation", 4, table);
            Expect("LegaiaWorld.LegaiaCardTableHost", 1, table);
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
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "seats");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "seatChairs");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "cards");
            CheckVar(container, "LegaiaWorld.LegaiaNpcStation", "handler");
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

        /// Weather + living props + card-table NPC seating, applied to the
        /// already-built root, then the common prefabs rebuilt on top.
        public static void Weather()
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
            var root = spawn.transform.parent != null
                ? spawn.transform.parent.gameObject : null;
            if (root == null || !root.name.StartsWith("Legaia_"))
                Fail("LegaiaSpawn is not under a built Legaia_<scene> root");
            string sceneName = root.name.Substring("Legaia_".Length);

            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            var o = new LegaiaRealismOptions();

            // --- Weather -------------------------------------------------
            var weather = LegaiaWeatherBuilder.Apply(root, sceneName, o);
            if (weather == null)
                Fail("weather pass built nothing");
            var rain = weather.GetComponentInChildren<ParticleSystem>(true);
            if (rain == null)
                Fail("no rain ParticleSystem under " + Path(weather.transform));
            var flash = weather.GetComponentInChildren<Light>(true);
            if (flash == null)
                Fail("no lightning Light under " + Path(weather.transform));
            if (flash.enabled)
                Fail("the lightning light must start disabled");
            var thunderSrc = weather.GetComponentInChildren<AudioSource>(true);
            if (thunderSrc == null || thunderSrc.clip == null)
                Fail("thunder AudioSource has no imported clip (generation failed?)");
            if (thunderSrc.clip.length < 3f)
                Fail("thunder clip is only " + thunderSrc.clip.length + " s");
            CheckVar(weather, "LegaiaWorld.LegaiaWeather", "rain");
            CheckVar(weather, "LegaiaWorld.LegaiaWeather", "rainRoot");
            CheckVar(weather, "LegaiaWorld.LegaiaWeather", "flashLight");
            CheckVar(weather, "LegaiaWorld.LegaiaWeather", "thunder");
            CheckVar(weather, "LegaiaWorld.LegaiaWeather", "maxEmission");
            // The grass shader's gust hook must exist, or wind does nothing.
            var grass = Shader.Find("Legaia/Grass Wind");
            if (grass == null)
                Fail("Legaia/Grass Wind shader missing");
            if (grass.FindPropertyIndex("_WindGust") < 0)
                Fail("Legaia/Grass Wind has no _WindGust property - the " +
                     "weather behaviour's only wind hook (Shader.SetGlobalFloat " +
                     "is not exposed to Udon)");
            if (AssetDatabase.LoadAssetAtPath<Material>(
                    "Assets/LegaiaGenerated/" + sceneName + "/realism/grass.mat") != null)
                CheckVar(weather, "LegaiaWorld.LegaiaWeather", "grassMaterial");

            // --- Living props: shoreline fishing stations ----------------
            var living = LegaiaLivingProps.Apply(root, sceneName, o);
            if (living == null)
                Fail("living props built nothing - no standable shoreline found");
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            if (stationType == null)
                Fail("LegaiaNpcStation is not compiled");
            var spots = living.GetComponentsInChildren(stationType, true);
            if (spots.Length < 3 || spots.Length > 4)
                Fail("expected 3-4 fishing stations, found " + spots.Length);
            var spotType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaFishingSpot");
            if (spotType == null || living.GetComponentsInChildren(spotType, true).Length
                    != spots.Length)
                Fail("every fishing station needs its own LegaiaFishingSpot handler");
            for (int i = 0; i < spots.Length; i++)
            {
                var t = spots[i].transform;
                // On the floor: the collider must be right under it.
                if (!Physics.Raycast(t.position + Vector3.up * 2f, Vector3.down,
                        out RaycastHit hit, 4f, -1, QueryTriggerInteraction.Ignore))
                    Fail(Path(t) + " does not stand on the world collider");
                if (Mathf.Abs(hit.point.y - t.position.y) > 0.6f)
                    Fail(Path(t) + " floats " + (t.position.y - hit.point.y) +
                         " m over the floor");
                Vector3 d = t.position - spawn.transform.position;
                d.y = 0f;
                if (d.magnitude > o.interiorRoomDistance)
                    Fail(Path(t) + " is " + d.magnitude + " m from the spawn");
                for (int j = i + 1; j < spots.Length; j++)
                {
                    Vector3 e = t.position - spots[j].transform.position;
                    e.y = 0f;
                    if (e.magnitude < 2f)
                        Fail("fishing spots " + i + " and " + j + " are " +
                             e.magnitude + " m apart");
                }
            }
            CheckVar(living, "LegaiaWorld.LegaiaNpcStation", "handler");
            CheckVar(living, "LegaiaWorld.LegaiaNpcStation", "standPoint");
            CheckVar(living, "LegaiaWorld.LegaiaFishingSpot", "station");
            CheckVar(living, "LegaiaWorld.LegaiaFishingSpot", "gear");
            CheckVar(living, "LegaiaWorld.LegaiaFishingSpot", "bobber");
            CheckVar(living, "LegaiaWorld.LegaiaFishingSpot", "line");
            CheckVar(living, "LegaiaWorld.LegaiaFishingSpot", "rodTip");

            // --- Card table: four seat stations on one host ---------------
            var opts = new LegaiaCommonPrefabOptions
            {
                mirror = false, tv = false, cardTable = true, seats = 4,
                sdkPens = false, slotMachine = false,
            };
            var settings = LegaiaSceneSettings.Load(sceneName);
            var container = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + sceneName, spawn.transform.position, opts,
                settings.prefabTransforms, null);
            var table = container.transform.Find("card_table");
            if (table == null)
                Fail("no card_table under " + container.name);
            var seatStations = table.GetComponentsInChildren(stationType, true);
            if (seatStations.Length != 4)
                Fail("expected 4 seat stations, found " + seatStations.Length);
            var hostType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardTableHost");
            if (hostType == null)
                Fail("LegaiaCardTableHost is not compiled");
            var host = table.GetComponentInChildren(hostType, true);
            if (host == null)
                Fail("no LegaiaCardTableHost on the card table");
            foreach (var st in seatStations)
            {
                var handler = st.GetType().GetField("handler")?.GetValue(st);
                if (!ReferenceEquals(handler, host))
                    Fail(Path(st.transform) + "'s handler is not the table host");
                var kind = st.GetType().GetField("kind")?.GetValue(st);
                if (!(kind is int k) || k != 2)
                    Fail(Path(st.transform) + " is not a kind-2 (seat) station");
            }
            var btn = table.Find("btn_npcs");
            if (btn == null)
                Fail("no btn_npcs on the card table");
            var btnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaEventButton");
            var btnComp = btn.GetComponent(btnType);
            if (btnComp == null)
                Fail("btn_npcs carries no LegaiaEventButton");
            if (!ReferenceEquals(btnType.GetField("target").GetValue(btnComp), host))
                Fail("btn_npcs does not target the table host");
            if ((string)btnType.GetField("eventName").GetValue(btnComp) != "ToggleNpcs")
                Fail("btn_npcs does not send ToggleNpcs");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "seats");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "seatChairs");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "seatHands");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "deckAnchor");
            CheckVar(container, "LegaiaWorld.LegaiaCardTableHost", "cards");
            CheckVar(container, "LegaiaWorld.LegaiaEventButton", "target");

            // Every U# proxy the three passes created must have a backing
            // behaviour with a program (the "outdated behaviour version"
            // symptom shows up here first).
            var usbType = LegaiaWorldBuilder.FindType("UdonSharp.UdonSharpBehaviour");
            if (usbType == null)
                Fail("UdonSharp not present - the checks above cannot mean anything");
            int proxies = 0;
            foreach (var scope in new[] { weather, living, container })
                foreach (var proxy in scope.GetComponentsInChildren(usbType, true))
                {
                    proxies++;
                    var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
                    if (backing == null)
                        Fail(proxy.GetType().Name + " on " + proxy.name +
                             " has no backing UdonBehaviour");
                    var bt = backing.GetType();
                    object prog = bt.GetField("programSource")?.GetValue(backing)
                                  ?? bt.GetProperty("programSource")?.GetValue(backing);
                    if (prog == null)
                        Fail(proxy.GetType().Name + " on " + proxy.name +
                             " has no program source");
                }

            Debug.Log("[Legaia] SELFTEST OK: weather rig + " + spots.Length +
                      " fishing station(s) + 4 seat stations, " + proxies +
                      " U# proxies wired (scene " + sceneName + ", not saved).");
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
