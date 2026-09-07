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
// LivingTown opens the same scene, runs the living-town pass over the
// built root and asserts the village is wired: a director with a non-empty
// typed station array on its BACKING behaviour, a brain on every eligible
// villager pointing back at that director, use-prop stations standing on
// real floor with room to stand, and a home for every villager within the
// per-door cap.
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

        // --- Living town ---------------------------------------------------

        /// The PLAY-MODE soak (LegaiaSoak): enters play mode with ClientSim,
        /// forces the clock, and watches the villagers actually run. Unlike
        /// every other method here it must be invoked WITHOUT `-quit` - it
        /// exits the editor itself once the run is over. Args:
        /// `-legaiaSoakMode night|day`, `-legaiaSoakSeconds N`,
        /// `-legaiaSoakScale S`, `-legaiaSoakLog <path>`. Recipe in
        /// Editor/LegaiaSoak.cs's header.
        public static void Soak()
        {
            LegaiaSoak.Run();
        }

        public static void LivingTown()
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
            var rootT = spawn.transform.parent;
            if (rootT == null || !rootT.name.StartsWith("Legaia_"))
                Fail("LegaiaSpawn is not under a Legaia_<scene> root");
            GameObject root = rootT.gameObject;
            string sceneName = rootT.name.Substring("Legaia_".Length);

            string manifestPath = "Assets/LegaiaImports/" + sceneName + "/manifest.json";
            if (!System.IO.File.Exists(manifestPath))
                Fail("no manifest at " + manifestPath +
                     " - copy the exported scene folder into the project first");
            object manifest = MiniJson.Parse(System.IO.File.ReadAllText(manifestPath));

            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            var settings = LegaiaSceneSettings.Load(sceneName);
            var o = new LegaiaLivingTownOptions();
            settings.ApplyLivingTown(o);
            // Apply TWICE: the pass must refresh, not stack. Every assert
            // below then runs against the second build, so a leaked brain or
            // a second director shows up as a failure rather than as a
            // slowly growing scene.
            LegaiaLivingTown.Apply(
                root, manifest, sceneName, new LegaiaLivingTownOptions(), settings);
            var container = LegaiaLivingTown.Apply(
                root, manifest, sceneName, new LegaiaLivingTownOptions(), settings);
            if (container == null)
                Fail("living town built nothing");
            int containers = 0;
            foreach (Transform child in rootT)
                if (child.name == LegaiaLivingTown.CONTAINER)
                    containers++;
            if (containers != 1)
                Fail(containers + " living_town containers under the root - " +
                     "re-applying stacked instead of refreshing");

            // --- The director ------------------------------------------------
            var dirType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaTownDirector");
            if (dirType == null)
                Fail("LegaiaTownDirector is not compiled - read the [UdonSharp] " +
                     "lines above: one U# compile error fails every wire");
            var directors = container.GetComponentsInChildren(dirType, true);
            if (directors.Length != 1)
                Fail("expected exactly 1 town director, found " + directors.Length);
            var director = directors[0];
            if (LegaiaCommonPrefabs.BackingUdon(director) == null)
                Fail("the town director has no backing UdonBehaviour");
            CheckVar(container, "LegaiaWorld.LegaiaTownDirector", "stations");
            CheckVar(container, "LegaiaWorld.LegaiaTownDirector", "brains");

            // The PROXY field must be a typed array (an object[] never
            // deserializes onto the backing variable); U# then stores the
            // BACKING UdonBehaviours, so the runtime array's element type is
            // Component - what matters there is that every slot is filled.
            var stationsField = dirType.GetField("stations");
            if (stationsField == null ||
                stationsField.FieldType.GetElementType().Name != "LegaiaNpcStation")
                Fail("LegaiaTownDirector.stations is not a LegaiaNpcStation[]");
            var stationArr = ReadVar(director, "stations") as System.Array;
            if (stationArr == null || stationArr.Length == 0)
                Fail("director.stations did not deserialize as an array");
            for (int i = 0; i < stationArr.Length; i++)
                if (stationArr.GetValue(i) == null)
                    Fail("director.stations[" + i + "] is null on the backing " +
                         "behaviour - the array did not survive serialization");
            var brainArr = ReadVar(director, "brains") as System.Array;

            // --- Stations ------------------------------------------------------
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            var npcRoot = rootT.Find("npcs");
            int propStations = 0, chatStations = 0, otherStations = 0;
            int carryStations = 0, visitStations = 0, indoorChat = 0;
            var handItemType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcHandItem");
            var visitType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaVisitSpot");
            if (handItemType == null || visitType == null)
                Fail("LegaiaNpcHandItem / LegaiaVisitSpot are not compiled - read " +
                     "the [UdonSharp] lines above: one U# compile error fails every wire");
            var openStands = new List<Transform>();
            foreach (var st in container.GetComponentsInChildren(stationType, true))
            {
                int kind = (int)stationType.GetField("kind").GetValue(st);
                var standPoint = stationType.GetField("standPoint").GetValue(st) as Transform;
                if (standPoint == null)
                    Fail(Path(st.transform) + " has no standPoint");
                Vector3 p = standPoint.position;
                if (kind == 0)
                {
                    propStations++;
                    RaycastHit hit;
                    if (!Physics.Raycast(p + Vector3.up * 1.5f, Vector3.down,
                            out hit, 4f, ~0, QueryTriggerInteraction.Ignore))
                        Fail(Path(st.transform) + " floats: no floor under " + p);
                    if (Mathf.Abs(hit.point.y - p.y) > 0.35f)
                        Fail(Path(st.transform) + " stands " +
                             (p.y - hit.point.y).ToString("0.00") + " m off its floor");
                    if (hit.normal.y < 0.7f)
                        Fail(Path(st.transform) + " stands on a wall (normal " +
                             hit.normal + ")");
                    foreach (var c in Physics.OverlapSphere(p + Vector3.up * 0.55f,
                                 0.26f, ~0, QueryTriggerInteraction.Ignore))
                        if (npcRoot == null || !c.transform.IsChildOf(npcRoot))
                            Fail(Path(st.transform) + " has no standing room: " +
                                 c.name + " is in the way");
                    if (stationType.GetField("handler").GetValue(st) == null)
                        Debug.LogWarning("[Legaia] selftest: " + Path(st.transform) +
                            " has no handler (its prop carries no LegaiaDoor)");
                }
                else if (kind == 3)
                {
                    chatStations++;
                    if ((bool)stationType.GetField("indoors").GetValue(st))
                        indoorChat++;
                }
                else
                {
                    otherStations++;
                    // Every stand spot the daytime pass builds is a place a
                    // villager is asked to STAND: floor under it, room for a
                    // body in it, and level with what it stands on. The
                    // use-prop block above says the same of kind 0; these
                    // are the ones this pass adds.
                    bool indoors = (bool)stationType.GetField("indoors").GetValue(st);
                    // A LOW ray, the same one the builders place against
                    // (LegaiaLivingTown.SnapFloorNear): a doorstep stand
                    // spot legitimately sits under the hut's eave, and a
                    // ray started at chest height finds the eave, not the
                    // floor - which reads as "stands 1.4 m off its floor".
                    RaycastHit sh;
                    if (!Physics.Raycast(p + Vector3.up * 0.6f, Vector3.down,
                            out sh, 2.6f, ~0, QueryTriggerInteraction.Ignore))
                        Fail(Path(st.transform) + " floats: no floor under " + p);
                    if (Mathf.Abs(sh.point.y - p.y) > 0.35f)
                        Fail(Path(st.transform) + " stands " +
                             (p.y - sh.point.y).ToString("0.00") + " m off its floor");
                    if (sh.normal.y < 0.7f)
                        Fail(Path(st.transform) + " stands on a wall (normal " +
                             sh.normal + ")");
                    foreach (var c in Physics.OverlapSphere(p + Vector3.up * 0.55f,
                                 0.26f, ~0, QueryTriggerInteraction.Ignore))
                        if (npcRoot == null || !c.transform.IsChildOf(npcRoot))
                            Fail(Path(st.transform) + " has no standing room: " +
                                 c.name + " is in the way");
                    if (!indoors)
                        openStands.Add(st.transform);

                    if (kind == 5)
                    {
                        carryStations++;
                        // The handler contract: a kind-5 station's item work
                        // is done by a LegaiaNpcHandItem whose `station`
                        // field reached its BACKING behaviour (a proxy-only
                        // edit runs nothing in-world), and which the station
                        // actually names as its handler.
                        var h = st.GetComponent(handItemType);
                        if (h == null)
                            Fail(Path(st.transform) + " is a kind-5 carry station " +
                                 "with no LegaiaNpcHandItem handler");
                        if (LegaiaCommonPrefabs.BackingUdon(h) == null)
                            Fail(Path(st.transform) + "'s hand-item handler has no " +
                                 "backing UdonBehaviour");
                        if (ReadVar(h, "station") == null)
                            Fail(Path(st.transform) + "'s hand-item handler does not " +
                                 "reference its station on the backing behaviour");
                        var named = stationType.GetField("handler").GetValue(st) as Object;
                        if (named != (Object)h)
                            Fail(Path(st.transform) + " does not name its " +
                                 "LegaiaNpcHandItem as the station handler");
                        int ik = (int)handItemType.GetField("itemKind").GetValue(h);
                        bool drop = (bool)handItemType.GetField("dropItem").GetValue(h);
                        if (ik < 0 && !drop)
                            Fail(Path(st.transform) + " neither hands over an item " +
                                 "nor takes one back - it is a plain stand spot");
                        if (ik >= LegaiaCarryArt.ITEMS)
                            Fail(Path(st.transform) + " hands over item " + ik +
                                 ", only " + LegaiaCarryArt.ITEMS + " exist");
                    }
                    else if (kind == 6)
                    {
                        visitStations++;
                        var h = st.GetComponent(visitType);
                        if (h == null)
                            Fail(Path(st.transform) + " is a kind-6 visit station " +
                                 "with no LegaiaVisitSpot handler");
                        if (LegaiaCommonPrefabs.BackingUdon(h) == null)
                            Fail(Path(st.transform) + "'s visit handler has no " +
                                 "backing UdonBehaviour");
                        if (ReadVar(h, "station") == null)
                            Fail(Path(st.transform) + "'s visit handler does not " +
                                 "reference its station on the backing behaviour");
                        if (ReadVar(h, "hostBubble") == null)
                            Fail(Path(st.transform) + " has no host bubble - the " +
                                 "fixed resident it calls on cannot answer");
                    }
                }
            }
            if (propStations < 1)
                Fail("no use-prop stations built - every one-shot prop was skipped");
            if (chatStations < 3)
                Fail("only " + chatStations + " chat stand point(s): a group of " +
                     "three needs at least one full ring");
            if (carryStations < 1)
                Fail("no carry/errand endpoints built - the day has nothing to " +
                     "fetch and nothing to put down");
            if (chatStations - indoorChat < 2)
                Fail("every conversation ring is indoors - the villagers who " +
                     "walk the village have nowhere to be matchmade to");
            if (openStands.Count < 4)
                Fail("only " + openStands.Count + " outdoor stand spot(s): the " +
                     "daytime villagers have nowhere to walk to");

            // --- Brains ---------------------------------------------------------
            var brainType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcBrain");
            var carryType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcCarry");
            if (carryType == null)
                Fail("LegaiaNpcCarry is not compiled");
            int eligible = 0, wired = 0, homed = 0, insideAlready = 0, unroutable = 0;
            var unroutableNpcs = new List<Transform>();
            var perDoor = new Dictionary<Object, int>();
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                if (settings.NpcIsRemoved(file) || settings.NpcIsStatic(file) ||
                    settings.NpcIsFrozen(file))
                    continue;
                Vector3 local = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                Transform placed = null;
                foreach (Transform child in npcRoot)
                    if ((child.localPosition - local).sqrMagnitude <= 1e-3f)
                    {
                        placed = child;
                        break;
                    }
                if (placed == null)
                    continue; // not placed in this build (conditional villagers)
                eligible++;
                var brains = placed.GetComponents(brainType);
                if (brains.Length != 1)
                    Fail(placed.name + " carries " + brains.Length +
                         " brain(s), expected exactly 1");
                var brain = brains[0];
                var bubbles = placed.GetComponentsInChildren(
                    LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaSpeechBubble"), true);
                if (bubbles.Length != 1)
                    Fail(placed.name + " carries " + bubbles.Length +
                         " speech bubble(s), expected exactly 1");
                if (LegaiaCommonPrefabs.BackingUdon(brain) == null)
                    Fail(placed.name + "'s brain has no backing UdonBehaviour");
                if (ReadVar(brain, "director") == null)
                    Fail(placed.name + "'s brain does not reference the director " +
                         "on its backing behaviour");
                if (ReadVar(brain, "loco") == null)
                    Fail(placed.name + "'s brain has no locomotion controller");

                // --- the carried-item rig --------------------------------
                // Everything a villager can hold is a CHILD of that
                // villager (so it follows without a line of per-frame code)
                // and every one of them is off at build time - a bucket
                // visible on a villager who is not on a fetch errand is the
                // failure this asserts away.
                var carries = placed.GetComponents(carryType);
                if (carries.Length != 1)
                    Fail(placed.name + " carries " + carries.Length +
                         " LegaiaNpcCarry rig(s), expected exactly 1");
                var carry = carries[0];
                if (LegaiaCommonPrefabs.BackingUdon(carry) == null)
                    Fail(placed.name + "'s carry rig has no backing UdonBehaviour");
                if (ReadVar(brain, "carry") == null)
                    Fail(placed.name + "'s brain does not reference its carry rig " +
                         "on the backing behaviour");
                var hand = ReadVar(carry, "hand") as Transform;
                if (hand == null)
                    Fail(placed.name + "'s carry rig has no hand node");
                if (!hand.IsChildOf(placed))
                    Fail(placed.name + "'s hand node is not parented under the NPC");
                var heldArr = ReadVar(carry, "items") as System.Array;
                if (heldArr == null || heldArr.Length != LegaiaCarryArt.ITEMS)
                    Fail(placed.name + "'s carry rig holds " +
                         (heldArr == null ? -1 : heldArr.Length) + " item(s), expected " +
                         LegaiaCarryArt.ITEMS);
                for (int k = 0; k < heldArr.Length; k++)
                {
                    var item = heldArr.GetValue(k) as GameObject;
                    if (item == null)
                        Fail(placed.name + "'s carry item " + k +
                             " is null on the backing behaviour");
                    if (!item.transform.IsChildOf(placed))
                        Fail(Path(item.transform) + " is not parented under " + placed.name);
                    if (item.activeSelf)
                        Fail(Path(item.transform) + " is visible at build - every " +
                             "carried item must start hidden");
                    if (item.GetComponentsInChildren<Collider>(true).Length > 0)
                        Fail(Path(item.transform) + " has a collider - a carried prop " +
                             "would shove the villager holding it");
                }
                // The hand must be ON the villager: measured against its
                // own rendered body, not against an assumed human.
                var bodyRends = placed.GetComponentsInChildren<Renderer>();
                if (bodyRends.Length > 0)
                {
                    Bounds body = bodyRends[0].bounds;
                    for (int k = 1; k < bodyRends.Length; k++)
                        body.Encapsulate(bodyRends[k].bounds);
                    float bh = body.size.y;
                    float dy = hand.position.y - body.min.y;
                    if (dy < bh * 0.10f || dy > bh * 0.85f)
                        Fail(placed.name + "'s hand sits at " +
                             (dy / bh).ToString("0.00") + " of its height - not an arm");
                    Vector2 off = new Vector2(hand.position.x - placed.position.x,
                        hand.position.z - placed.position.z);
                    if (off.magnitude > bh * 0.5f)
                        Fail(placed.name + "'s hand is " + off.magnitude.ToString("0.00") +
                             " m out from the body (height " + bh.ToString("0.00") + ")");
                }
                wired++;
                if (ReadVar(brain, "startIndoors") is bool inside && inside)
                {
                    // Placed inside a house by retail: home already.
                    if (ReadVar(brain, "homeDoor") != null)
                        Fail(placed.name + " starts indoors yet was given a front door");
                    insideAlready++;
                    continue;
                }
                if (ReadVar(brain, "noRoute") is bool cut && cut)
                {
                    if (ReadVar(brain, "homeDoor") != null)
                        Fail(placed.name + " is flagged noRoute yet was given a front door");
                    unroutable++;
                    unroutableNpcs.Add(placed);
                    continue;
                }
                var door = ReadVar(brain, "homeDoor") as Object;
                if (door != null)
                {
                    homed++;
                    int c;
                    perDoor.TryGetValue(door, out c);
                    perDoor[door] = c + 1;
                }
            }
            if (wired != eligible)
                Fail(wired + " brains for " + eligible + " eligible villagers");
            if (brainArr == null || brainArr.Length != wired)
                Fail("director.brains holds " +
                     (brainArr == null ? -1 : brainArr.Length) + " of " + wired +
                     " villagers");

            var homesRoot = container.transform.Find("homes");
            int homes = homesRoot != null ? homesRoot.childCount : 0;
            int cap = o.homeCap;

            // --- Navmesh + the door trip --------------------------------------
            // The bake must exist, be wired to its loader, and carry a
            // COMPLETE route from every villager's spawn to its door stand
            // spot, from the stand spot onto the doorway tile, and (where
            // the home has a way out) from the landing to the exit - the
            // walks the night routine makes. A partial route is exactly the
            // clipping-through-the-hillside walk this replaces.
            var navGo = rootT.Find(LegaiaNavMesh.CONTAINER);
            if (navGo == null)
                Fail("no " + LegaiaNavMesh.CONTAINER + " container under the root - " +
                     "the navmesh bake built nothing (no colliders?)");
            CheckVar(navGo.gameObject, "LegaiaWorld.LegaiaNavMeshLoader", "data");
            var navData = LegaiaNavMesh.LoadData(sceneName);
            if (navData == null)
                Fail("the navmesh asset was not saved under LegaiaGenerated/" + sceneName);
            var navInstance = LegaiaNavMesh.Register(navData);
            var navLinks = LegaiaNavMesh.LinksOf(root);
            int routes = 0, doorProps = 0, thresholds = 0, hopHomes = 0;
            var routeFailures = new List<string>();
            try
            {
                // Every OUTDOOR stand spot must be walkable-to by somebody:
                // the spawn, or one of the villagers themselves (town01's
                // beach sits on its own island of navmesh, and a spot only
                // the two beach villagers can use is a good spot, not a
                // broken one). A spot nobody can reach reads in-world as a
                // villager walking into a bank until its walk times out.
                var anchors = new List<Vector3> { spawn.transform.position };
                foreach (Transform child in npcRoot)
                    if (child.GetComponent(brainType) != null)
                        anchors.Add(child.position);
                var stranded = new List<string>();
                foreach (var stand in openStands)
                {
                    bool reach = false;
                    string reason;
                    for (int i = 0; i < anchors.Count && !reach; i++)
                        reach = LegaiaNavMesh.Reachable(anchors[i], stand.position,
                            1.2f, out reason);
                    if (!reach)
                        stranded.Add(Path(stand));
                }
                if (stranded.Count > 0)
                    Fail(stranded.Count + " outdoor stand spot(s) nobody can walk to: " +
                         string.Join(", ", stranded));

                foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                         ?? new List<object>())
                {
                    if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                        continue;
                    string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                    if (settings.NpcIsRemoved(file) || settings.NpcIsStatic(file) ||
                        settings.NpcIsFrozen(file))
                        continue;
                    Vector3 local = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                    Transform placed = null;
                    foreach (Transform child in npcRoot)
                        if ((child.localPosition - local).sqrMagnitude <= 1e-3f)
                        {
                            placed = child;
                            break;
                        }
                    if (placed == null)
                        continue;
                    var brain = placed.GetComponent(brainType);
                    var door = ReadVar(brain, "homeDoor") as Transform;
                    if (door == null)
                        continue;
                    var threshold = ReadVar(brain, "homeThreshold") as Transform;
                    if (threshold == null)
                        Fail(placed.name + " has a home door but no doorway tile (homeThreshold)");
                    thresholds++;
                    if (ReadVar(brain, "homeDoorProp") != null)
                        doorProps++;
                    string why;
                    bool hopped;
                    // A COMPLETE route may include one ledge hop: that is
                    // how the shore villagers below the village bank get
                    // home at all, and the locomotion controller composes
                    // exactly the same walk -> hop -> walk at runtime.
                    if (!LegaiaNavMesh.ReachableWithLinks(placed.position, door.position,
                            1.2f, navLinks, out hopped, out why))
                        routeFailures.Add(placed.name + " -> " + door.parent.name + "/door: " + why);
                    else
                    {
                        routes++;
                        if (hopped)
                            hopHomes++;
                    }
                    if (!LegaiaNavMesh.Reachable(door.position, threshold.position, 1.2f, out why))
                        routeFailures.Add(door.parent.name + " door -> threshold: " + why);
                    var landing = ReadVar(brain, "homeLanding") as Transform;
                    var exit = ReadVar(brain, "homeExit") as Transform;
                    if (landing != null && exit != null &&
                        !LegaiaNavMesh.Reachable(landing.position, exit.position, 1.2f, out why))
                        routeFailures.Add(door.parent.name + " landing -> exit: " + why);
                }
            }
            finally
            {
                try
                {
                    // A noRoute flag must be TRUE: re-derive it, so the flag
                    // can never hide a villager the bake simply lost.
                    foreach (var npc in unroutableNpcs)
                        foreach (Transform home in homesRoot)
                        {
                            var d = home.Find("door");
                            string why;
                            bool hopped;
                            if (d != null && LegaiaNavMesh.ReachableWithLinks(npc.position,
                                    d.position, 1.2f, navLinks, out hopped, out why))
                                Fail(npc.name + " is flagged noRoute but " + home.name +
                                     "/door is reachable from its spawn" +
                                     (hopped ? " over a ledge link" : ""));
                        }
                }
                finally
                {
                    UnityEngine.AI.NavMesh.RemoveNavMeshData(navInstance);
                }
            }
            if (routeFailures.Count > 0)
                Fail(routeFailures.Count + " night-routine route(s) have no complete " +
                     "navmesh path:\n  " + string.Join("\n  ", routeFailures));
            if (thresholds != homed)
                Fail(thresholds + " doorway tiles for " + homed + " homed villagers");
            foreach (var kv in perDoor)
                if (kv.Value > cap)
                    Fail("home door " + kv.Key.name + " holds " + kv.Value +
                         " villagers, cap is " + cap);
            int outside = wired - insideAlready - unroutable;
            if (homed < outside)
            {
                // Short of capacity is allowed; a free slot left over is not.
                if (homes * cap > homed)
                    Fail(homed + " of " + outside + " village-side villagers have a " +
                         "home while " + (homes * cap - homed) + " slot(s) sit free");
                Debug.LogWarning("[Legaia] selftest: " + (outside - homed) +
                    " villager(s) have no home - " + homes + " door(s) x cap " +
                    cap + " cannot seat " + outside);
            }

            Debug.Log("[Legaia] SELFTEST OK: living town wired - " + wired +
                " villager(s), " + homed + " homed across " + homes +
                " door(s) (cap " + cap + ", " + doorProps + " with a door prop), " +
                insideAlready + " living indoors already, " + unroutable +
                " cut off from every door, " + navLinks.Count + " ledge link(s) (" +
                hopHomes + " villager(s) get home over one), " +
                routes + " navmesh route(s) home complete, " + stationArr.Length +
                " station(s) on the director (" + propStations + " use-prop, " +
                chatStations + " chat of which " + indoorChat + " indoors, " +
                carryStations + " carry/errand, " + visitStations + " visit, " +
                (otherStations - carryStations - visitStations) + " stand spot), " +
                openStands.Count + " of them outdoors and all reachable, scene " +
                sceneName + " (not saved).");
        }

        /// The value of `varName` on a U# proxy's BACKING UdonBehaviour -
        /// CheckVar's reader without the assertion, for checks that need the
        /// value itself.
        static object ReadVar(Component proxy, string varName)
        {
            var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
            if (backing == null)
                return null;
            const System.Reflection.BindingFlags ANY =
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic |
                System.Reflection.BindingFlags.Instance;
            var bt = backing.GetType();
            object pv = bt.GetField("publicVariables", ANY)?.GetValue(backing)
                        ?? bt.GetProperty("publicVariables", ANY)?.GetValue(backing);
            if (pv == null)
                return null;
            foreach (var mi in pv.GetType().GetMethods())
                if (mi.Name == "TryGetVariableValue" && !mi.IsGenericMethod &&
                    mi.GetParameters().Length == 2)
                {
                    var args = new object[] { varName, null };
                    return (bool)mi.Invoke(pv, args) ? args[1] : null;
                }
            return null;
        }

        /// Ambience layer: rebuild it over the already-built root and assert
        /// the clips, the emitters and the mixer wiring all landed.
        ///
        ///   Unity.exe -batchmode -nographics -quit -projectPath <project>
        ///       -executeMethod LegaiaWorld.LegaiaBatchChecks.Ambience
        ///       [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
        public static void Ambience()
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
            LegaiaRealism.ApplyAmbienceOnly(root, sceneName, o);

            var ambT = root.transform.Find(LegaiaRealism.AMBIENCE);
            if (ambT == null)
                Fail("no \"" + LegaiaRealism.AMBIENCE + "\" container under " + root.name);
            var amb = ambT.gameObject;

            // --- The five 2D beds, each with its generated clip ----------
            // Durations come from LegaiaAudioGen's own constants, so a clip
            // that failed to import (or imported as the stale 16 s bed) is
            // caught here rather than in-world.
            AudioSource Bed(string name, int seconds)
            {
                var t = ambT.Find(name);
                if (t == null)
                    Fail("no " + name + " under the ambience container");
                var src = t.GetComponent<AudioSource>();
                if (src == null)
                    Fail(name + " has no AudioSource");
                if (src.clip == null)
                    Fail(name + " has no clip - generation or import failed");
                if (Mathf.Abs(src.clip.length - seconds) > 0.25f)
                    Fail(name + " is " + src.clip.length.ToString("F2") +
                         " s, expected " + seconds + " s");
                if (!src.loop || !src.playOnAwake)
                    Fail(name + " must loop and play on awake");
                if (src.spatialBlend != 0f)
                    Fail(name + " is a 2D bed but has spatialBlend " + src.spatialBlend);
                return src;
            }
            Bed(LegaiaRealism.BED_BASE, LegaiaAudioGen.BASE_SECONDS);
            Bed(LegaiaRealism.BED_DAY, LegaiaAudioGen.DAY_SECONDS);
            Bed(LegaiaRealism.BED_NIGHT, LegaiaAudioGen.NIGHT_SECONDS);
            Bed(LegaiaRealism.BED_WIND, LegaiaAudioGen.GUST_SECONDS);

            // --- Spatial emitter groups ----------------------------------
            int waves = 0, birds = 0, wildlife = 0, mills = 0, spatial = 0, beds = 0;
            foreach (var src in amb.GetComponentsInChildren<AudioSource>(true))
            {
                string n = src.name;
                if (n.StartsWith("bed_"))
                {
                    beds++;
                    continue;
                }
                spatial++;
                if (src.clip == null)
                    Fail(n + " has no clip");
                if (src.spatialBlend < 0.99f)
                    Fail(n + " is an emitter but its spatialBlend is " + src.spatialBlend);
                if (!src.loop || !src.playOnAwake)
                    Fail(n + " must loop and play on awake");
                if (src.maxDistance <= src.minDistance)
                    Fail(n + " has Far " + src.maxDistance + " <= Near " + src.minDistance);
                if (n.StartsWith("waves_")) waves++;
                else if (n.StartsWith("birds_")) birds++;
                else if (n.StartsWith("wildlife_")) wildlife++;
                else if (n.StartsWith("windmill_")) mills++;
                else Fail("unexpected source " + n + " under the ambience container");
            }
            if (waves < 1)
                Fail("no shore wave emitters - no water sheet found near spawn");
            if (birds < 1)
                Fail("no tree bird emitters - no canopy cluster found");
            if (wildlife < 1)
                Fail("no night wildlife emitters");

            // --- VRC spatial compliance on EVERY source ------------------
            // The SDK deprecates a bare AudioSource: 2D beds carry the
            // component disabled (the SDK Auto Fix shape), emitters carry it
            // enabled and configured.
            var spatialType =
                LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCSpatialAudioSource")
                ?? LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_SpatialAudioSource");
            if (spatialType == null)
                Fail("VRCSpatialAudioSource type not found - the SDK is missing");
            foreach (var src in amb.GetComponentsInChildren<AudioSource>(true))
            {
                var comp = src.GetComponent(spatialType);
                if (comp == null)
                    Fail(Path(src.transform) + " has no VRC spatial audio component");
                var beh = comp as Behaviour;
                bool wantEnabled = !src.name.StartsWith("bed_");
                if (beh != null && beh.enabled != wantEnabled)
                    Fail(src.name + "'s VRC spatial component is " +
                         (beh.enabled ? "enabled" : "disabled") + ", expected the opposite");
            }

            // --- The mixer owns every volume -----------------------------
            var mixerType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaAmbienceMixer");
            if (mixerType == null)
                Fail("LegaiaAmbienceMixer is not compiled");
            if (amb.GetComponent(mixerType) == null)
                Fail("no LegaiaAmbienceMixer on the ambience container");
            foreach (string f in new[]
                     { "baseBed", "dayBed", "nightBed", "windBed",
                       "daySources", "nightSources", "anySources",
                       "dayGroupVolume", "nightGroupVolume", "anyGroupVolume" })
                CheckVar(amb, "LegaiaWorld.LegaiaAmbienceMixer", f);

            // The day/night behaviour must hand the mixer the cycle, and
            // must NOT keep driving the two beds itself (one writer each).
            var sunT = root.transform.Find("LegaiaSun");
            var dnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
            if (sunT != null && dnType != null && sunT.GetComponent(dnType) != null)
            {
                CheckVar(amb, "LegaiaWorld.LegaiaAmbienceMixer", "dayNight");
                var dn = sunT.GetComponent(dnType);
                var dnDay = dnType.GetField("dayAmbience")?.GetValue(dn);
                var dnNight = dnType.GetField("nightAmbience")?.GetValue(dn);
                if (dnDay as Object != null || dnNight as Object != null)
                    Fail("LegaiaDayNight still holds bed references while the " +
                         "mixer is present - two writers on one AudioSource");
            }
            else
            {
                Debug.Log("[Legaia] selftest: no LegaiaDayNight on this root " +
                          "(day/night off) - the mixer stays on permanent day.");
            }

            if (beds != 4)
                Fail(beds + " 2D beds under the ambience container, expected 4 " +
                     "(base, day, night, wind gust)");
            Debug.Log("[Legaia] SELFTEST OK: ambience = " + beds + " bed(s) + " + spatial +
                      " spatial emitter(s) (" + waves + " shore, " + birds +
                      " bird, " + wildlife + " wildlife, " + mills +
                      " windmill) under " + Path(ambT) + " (scene " + sceneName +
                      ", not saved).");
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
