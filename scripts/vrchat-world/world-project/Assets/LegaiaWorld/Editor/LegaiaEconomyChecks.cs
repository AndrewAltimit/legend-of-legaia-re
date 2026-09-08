// Headless checks for the ECONOMY layer - the coin purse and the three
// ways a player fills it - runnable without opening the editor UI:
//
//   Unity.exe -batchmode -nographics -quit -projectPath <project>
//       -executeMethod LegaiaWorld.LegaiaEconomyChecks.Run
//       [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
// Run rebuilds the pieces the economy hangs off - the common prefabs
// (which is where the wallet lives), the shoreline fishing spots, and
// the living town (whose last pass is the bounty layer) - and then
// asserts the wiring reached the BACKING UdonBehaviours, which is the
// only thing that runs in-world: a field set on a U# proxy and never
// copied down is a silent no-op, not an error.
//
// What it asserts:
//   - the purse exists at Legaia_common_prefabs/wallet with a program;
//   - the slot machine's `wallet` is wired, or its Start fallback path
//     (that same object) resolves;
//   - every fishing spot carries an Interact collider, a stake, and a
//     handler with the wallet, the gear and the reward label on it;
//   - every villager has a `hitbox` trigger pointing back at its brain,
//     and every brain's loose `bounty` link points at the one pool;
//   - the pool holds twelve drops, each with a purse and a collider;
//   - a Legaia_equipment rack, if the scene has one, carries the weapon
//     flag on the pieces the item manifest calls weapons.
//
// Soak is the play-mode half: it drives one villager through the whole
// bounty loop (struck down -> coins on the ground -> back on its feet)
// and fails if any leg of it does not happen. A network-callable event
// never fires in editor play mode, so the strike goes in through the
// hitbox's DebugSlainHere, which runs the broadcast's body directly.
//
//   Unity.exe -batchmode -nographics -projectPath <copy>
//       -executeMethod LegaiaWorld.LegaiaEconomyChecks.Soak
//       -logFile <copy>\Logs\economy-soak.log
//
// NOTE: no `-quit` on Soak - it enters play mode and exits itself
// (0 pass, 1 an assertion failed, 3 play mode never started, 4 the
// wall-clock watchdog fired), exactly as LegaiaSoak does.

using System.Collections.Generic;
using System.Reflection;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaEconomyChecks
    {
        const string WALLET_PATH = "Legaia_common_prefabs/wallet";
        const int POOL_SIZE = 12;

        // --- edit-mode wiring check -------------------------------------------

        public static void Run()
        {
            string scenePath = Arg("-legaiaScene", "Assets/Scenes/VRCDefaultWorldScene.unity");
            EditorSceneManager.OpenScene(scenePath, OpenSceneMode.Single);

            GameObject spawn = FindSpawn(scenePath);
            var rootT = spawn.transform.parent;
            if (rootT == null || !rootT.name.StartsWith("Legaia_"))
                Fail("LegaiaSpawn is not under a Legaia_<scene> root");
            GameObject root = rootT.gameObject;
            string sceneName = rootT.name.Substring("Legaia_".Length);

            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            var settings = LegaiaSceneSettings.Load(sceneName);

            // The purse first: every consumer below wires to whatever
            // LegaiaCommonPrefabs.FindWallet() answers at ITS build time.
            var prefabOpts = new LegaiaCommonPrefabOptions
            {
                mirror = false, tv = false, cardTable = false, seats = 0,
                sdkPens = false, slotMachine = true,
            };
            var prefabs = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + sceneName, spawn.transform.position,
                prefabOpts, settings.prefabTransforms, settings.slotMachine);
            if (prefabs == null)
                Fail("the common prefabs pass built nothing - no wallet, no machine");

            var realism = new LegaiaRealismOptions();
            var living = LegaiaLivingProps.Apply(root, sceneName, realism);
            if (living == null)
                Fail("living props built nothing - no fishing spots to check");

            object manifest = LoadManifest(sceneName);
            string manifestDir = "Assets/LegaiaImports/" + sceneName;
            settings.ApplyNpcOverrides(manifest, manifestDir, root);
            LegaiaWorldBuilder.ReconcileNpcs(manifest, manifestDir, root, sceneName, settings);
            var container = LegaiaLivingTown.Apply(
                root, manifest, sceneName, new LegaiaLivingTownOptions(), settings);
            if (container == null)
                Fail("the living-town pass built nothing - no villagers to hunt");

            // --- the purse ---------------------------------------------------
            var walletGo = GameObject.Find(WALLET_PATH);
            if (walletGo == null)
                Fail("no coin purse at " + WALLET_PATH);
            var walletType = Type("LegaiaWallet");
            if (walletType == null)
                Fail("LegaiaWallet is not compiled - read the [UdonSharp] lines " +
                     "above: one U# compile error fails every wire in the scene");
            var wallet = walletGo.GetComponent(walletType);
            if (wallet == null)
                Fail(WALLET_PATH + " carries no LegaiaWallet");
            if (LegaiaCommonPrefabs.BackingUdon(wallet) == null)
                Fail("the coin purse has no backing UdonBehaviour");

            // --- the slot machine --------------------------------------------
            var machineType = Type("LegaiaSlotMachine");
            var machines = Object.FindObjectsOfType(machineType, true);
            int machinesWired = 0;
            foreach (var m in machines)
            {
                var proxy = m as Component;
                var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
                if (backing == null)
                    Fail("a slot machine has no backing UdonBehaviour");
                object w = ReadVar(proxy, "wallet");
                if (w != null)
                    machinesWired++;
                else if (GameObject.Find(WALLET_PATH) == null)
                    Fail("the slot machine has no wallet AND its Start fallback " +
                         "path " + WALLET_PATH + " does not resolve");
            }
            if (machines.Length == 0)
                Debug.LogWarning("[Legaia] ECONOMY: no slot machine in this scene " +
                    "- the cabinet's wallet path is unchecked.");

            // --- fishing -------------------------------------------------------
            var spotType = Type("LegaiaFishingSpot");
            var spots = living.GetComponentsInChildren(spotType, true);
            if (spots.Length == 0)
                Fail("no LegaiaFishingSpot handlers under " + living.name);
            foreach (var s in spots)
            {
                var go = s.gameObject;
                string where = PathOf(go.transform);
                var box = go.GetComponent<BoxCollider>();
                if (box == null)
                    Fail(where + " has no Interact collider - the stake is unreachable");
                if (!box.isTrigger)
                    Fail(where + "'s Interact box is solid; it must be a trigger " +
                         "so nobody walks into the stake");
                if (go.transform.Find("stake") == null)
                    Fail(where + " has no stake prop");
                foreach (string field in new[] { "wallet", "rewardText", "gear",
                                                 "station", "bobber", "rodTip" })
                    if (ReadVar(s, field) == null)
                        Fail(where + ": LegaiaFishingSpot." + field +
                             " did not reach the backing UdonBehaviour");
            }

            // --- bounty ---------------------------------------------------------
            var poolT = container.transform.Find(LegaiaBounty.POOL);
            if (poolT == null)
                Fail("no coin pool at " + LegaiaLivingTown.CONTAINER + "/" +
                     LegaiaBounty.POOL);
            var poolType = Type("LegaiaCoinDrops");
            var pool = poolT.GetComponent(poolType);
            if (pool == null)
                Fail("the coin pool carries no LegaiaCoinDrops");
            var poolBacking = LegaiaCommonPrefabs.BackingUdon(pool);
            if (poolBacking == null)
                Fail("the coin pool has no backing UdonBehaviour");

            var dropsArr = ReadVar(pool, "drops") as System.Array;
            if (dropsArr == null)
                Fail("LegaiaCoinDrops.drops did not deserialize as an array");
            if (dropsArr.Length != POOL_SIZE)
                Fail("the pool holds " + dropsArr.Length + " drop(s), expected " +
                     POOL_SIZE);
            var dropType = Type("LegaiaCoinDrop");
            int dropsWithPurse = 0;
            foreach (Transform child in poolT)
            {
                var d = child.GetComponent(dropType);
                if (d == null)
                    continue;
                string where = PathOf(child);
                if (child.GetComponent<BoxCollider>() == null)
                    Fail(where + " has no collider - it could never be taken");
                if (ReadVar(d, "visual") == null)
                    Fail(where + ": LegaiaCoinDrop.visual did not reach the backing " +
                         "behaviour");
                if (ReadVar(d, "wallet") != null)
                    dropsWithPurse++;
            }
            if (dropsWithPurse != POOL_SIZE)
                Fail(dropsWithPurse + " of " + POOL_SIZE + " coin drops carry a " +
                     "purse - the rest could never pay");

            var brainType = Type("LegaiaNpcBrain");
            var hitboxType = Type("LegaiaNpcHitbox");
            var npcRoot = rootT.Find("npcs");
            if (npcRoot == null)
                Fail("no npcs container under the built root");
            int villagers = 0, hitboxes = 0;
            foreach (Transform npc in npcRoot)
            {
                var brain = npc.GetComponent(brainType);
                if (brain == null)
                    continue;
                villagers++;
                var hbT = npc.Find(LegaiaBounty.HITBOX);
                if (hbT == null)
                    Fail(PathOf(npc) + " has no " + LegaiaBounty.HITBOX + " child");
                var cap = hbT.GetComponent<CapsuleCollider>();
                if (cap == null || !cap.isTrigger)
                    Fail(PathOf(hbT) + " needs a trigger CapsuleCollider");
                if (cap.height < 0.3f || cap.radius < 0.05f)
                    Fail(PathOf(hbT) + " is degenerate (h=" + cap.height +
                         " r=" + cap.radius + ") - the villager's bounds " +
                         "did not measure");
                var rb = hbT.GetComponent<Rigidbody>();
                if (rb == null || !rb.isKinematic)
                    Fail(PathOf(hbT) + " needs a kinematic Rigidbody - Unity " +
                         "reports no trigger crossing without one");
                var hb = hbT.GetComponent(hitboxType);
                if (hb == null)
                    Fail(PathOf(hbT) + " carries no LegaiaNpcHitbox");
                if (ReadVar(hb, "brain") == null)
                    Fail(PathOf(hbT) + ": LegaiaNpcHitbox.brain did not reach the " +
                         "backing behaviour");
                if (ReadVar(hb, "npcRoot") == null)
                    Fail(PathOf(hbT) + ": LegaiaNpcHitbox.npcRoot did not reach the " +
                         "backing behaviour");
                object bounty = ReadVar(brain, "bounty");
                if (bounty == null)
                    Fail(PathOf(npc) + ": the brain's bounty link did not reach the " +
                         "backing behaviour - a slain villager would drop nothing");
                if (!ReferenceEquals(bounty, poolBacking))
                    Fail(PathOf(npc) + "'s bounty link points at " + bounty +
                         ", not at the one pool");
                hitboxes++;
            }
            if (villagers == 0)
                Fail("no villagers with a brain - nothing to hunt");

            // --- the weapon rack -------------------------------------------------
            int rackProps = 0, rackWeapons = 0, manifestWeapons = -1;
            var rack = GameObject.Find("Legaia_equipment");
            if (rack != null)
            {
                var propType = Type("LegaiaPickupProp");
                foreach (var pc in rack.GetComponentsInChildren(propType, true))
                {
                    rackProps++;
                    // A rack placed before the bounty layer existed carries
                    // no `weapon` entry at all - not a wiring failure, just
                    // a rack that has not been re-placed since.
                    object flag = ReadVar(pc, "weapon");
                    if (flag is bool && (bool)flag)
                        rackWeapons++;
                }
                manifestWeapons = CountManifestWeapons();
                if (rackWeapons > 0 && manifestWeapons >= 0 &&
                    rackWeapons != manifestWeapons)
                    Fail("the rack flags " + rackWeapons + " weapon(s) but the item " +
                         "manifest lists " + manifestWeapons);
                if (rackWeapons == 0)
                    Debug.LogWarning("[Legaia] ECONOMY: the rack carries no weapon " +
                        "flags - it was placed before the bounty layer existed. " +
                        "Re-run 'Place equipment rack near spawn' to arm it.");
            }

            Debug.Log("[Legaia] ECONOMY: purse at " + WALLET_PATH + ", " +
                machines.Length + " slot machine(s) (" + machinesWired +
                " wallet-wired), " + spots.Length + " fishing spot(s), " +
                hitboxes + "/" + villagers + " villager hitbox(es), " +
                POOL_SIZE + " coin drop(s), rack " +
                (rack == null ? "absent"
                    : rackWeapons + "/" + rackProps + " weapons" +
                      (manifestWeapons >= 0 ? " (manifest " + manifestWeapons + ")" : "")) +
                ".");
            Debug.Log("[Legaia] SELFTEST OK: economy wiring complete.");
        }

        /// How many "Weapon" rows the project's item manifest lists, or -1
        /// when no item manifest is in the project.
        static int CountManifestWeapons()
        {
            string[] guids = AssetDatabase.FindAssets("manifest");
            foreach (string guid in guids)
            {
                string path = AssetDatabase.GUIDToAssetPath(guid);
                if (!path.EndsWith(".json"))
                    continue;
                object m;
                try
                {
                    m = MiniJson.Parse(System.IO.File.ReadAllText(path));
                }
                catch
                {
                    continue;
                }
                var items = MiniJson.AsList(MiniJson.Get(m, "items"));
                if (items == null || items.Count == 0)
                    continue;
                if (MiniJson.Get(items[0], "section_label") == null)
                    continue;
                int n = 0;
                foreach (object it in items)
                {
                    string label = MiniJson.AsStr(MiniJson.Get(it, "section_label")) ?? "";
                    string file = MiniJson.AsStr(MiniJson.Get(it, "alone"))
                        ?? MiniJson.AsStr(MiniJson.Get(it, "with_limb"));
                    if (file != null && label.Contains("Weapon"))
                        n++;
                }
                return n;
            }
            return -1;
        }

        // --- slot rules parity -------------------------------------------------

        /// Replay the engine-kernel fixture through the C# slot rules
        /// (Legaia > Verify Slot Rules, headless). The wallet path leaves
        /// the arithmetic alone by construction - a bare machine has no
        /// purse and takes the old per-cabinet branch - and this is the
        /// gate that says so out loud after an edit to the machine.
        ///
        ///   Unity.exe -batchmode -nographics -quit -projectPath <project>
        ///       -executeMethod LegaiaWorld.LegaiaEconomyChecks.SlotRules
        ///       -logFile <log>
        public static void SlotRules()
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaSlotTools");
            if (t == null)
                Fail("LegaiaSlotTools is not compiled");
            var mi = t.GetMethod("VerifyRules", BindingFlags.Static |
                BindingFlags.NonPublic | BindingFlags.Public);
            if (mi == null)
                Fail("LegaiaSlotTools.VerifyRules not found");
            // It reports through the console, so read the console.
            var errors = new List<string>();
            Application.LogCallback handler = (msg, stack, type) =>
            {
                if (type == LogType.Error && msg.Contains("slot verify"))
                    errors.Add(msg);
            };
            Application.logMessageReceived += handler;
            try
            {
                mi.Invoke(null, null);
            }
            finally
            {
                Application.logMessageReceived -= handler;
            }
            if (errors.Count > 0)
                Fail(errors.Count + " slot-rule mismatch(es) against the engine " +
                     "kernel - the first is: " + errors[0]);
            Debug.Log("[Legaia] SELFTEST OK: slot rules match the engine kernel " +
                "with the wallet path in place.");
        }

        // --- play-mode soak ----------------------------------------------------

        const string K_ACTIVE = "legaia.economy.soak.active";

        public static void Soak()
        {
            string scenePath = Arg("-legaiaScene", "Assets/Scenes/VRCDefaultWorldScene.unity");
            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            EditorSceneManager.OpenScene(scenePath, OpenSceneMode.Single);

            GameObject spawn = FindSpawn(scenePath);
            var rootT = spawn.transform.parent;
            if (rootT == null || !rootT.name.StartsWith("Legaia_"))
                Fail("LegaiaSpawn is not under a Legaia_<scene> root");
            string sceneName = rootT.name.Substring("Legaia_".Length);
            var settings = LegaiaSceneSettings.Load(sceneName);
            object manifest = LoadManifest(sceneName);
            string manifestDir = "Assets/LegaiaImports/" + sceneName;

            var prefabOpts = new LegaiaCommonPrefabOptions
            {
                mirror = false, tv = false, cardTable = false, seats = 0,
                sdkPens = false, slotMachine = false,
            };
            LegaiaCommonPrefabs.Build("Assets/LegaiaGenerated/" + sceneName,
                spawn.transform.position, prefabOpts, settings.prefabTransforms,
                settings.slotMachine);
            settings.ApplyNpcOverrides(manifest, manifestDir, rootT.gameObject);
            LegaiaWorldBuilder.ReconcileNpcs(manifest, manifestDir, rootT.gameObject,
                sceneName, settings);
            if (LegaiaLivingTown.Apply(rootT.gameObject, manifest, sceneName,
                    new LegaiaLivingTownOptions(), settings) == null)
                Fail("the living-town pass built nothing - nothing to soak");

            StripRendering();
            SessionState.SetInt(K_ACTIVE, 1);
            Debug.Log("[Legaia] economy soak: entering play mode.");
            EditorApplication.EnterPlaymode();
        }

        /// Everything a -nographics editor still tries to draw and the soak
        /// does not simulate (the shadow pass over an offscreen camera dies
        /// with no graphics device).
        static void StripRendering()
        {
            QualitySettings.shadows = ShadowQuality.Disable;
            QualitySettings.shadowDistance = 0f;
            foreach (var l in Object.FindObjectsOfType<Light>(true))
                l.shadows = LightShadows.None;
            foreach (var c in Object.FindObjectsOfType<Camera>(true))
                c.enabled = false;
            var mirrorType = LegaiaWorldBuilder.FindType(
                "VRC.SDK3.Components.VRCMirrorReflection");
            if (mirrorType == null)
                return;
            foreach (var m in Object.FindObjectsOfType(mirrorType, true))
            {
                var c = m as Component;
                if (c != null)
                    c.gameObject.SetActive(false);
            }
        }

        [InitializeOnLoadMethod]
        static void Hook()
        {
            if (SessionState.GetInt(K_ACTIVE, 0) == 0)
                return;
            EditorApplication.update -= Drive;
            EditorApplication.update += Drive;
        }

        const float SETTLE_SECONDS = 10f;
        const float RESPAWN_SECONDS = 8f;
        const int STATE_DEAD = 40;

        static bool s_inited;
        static float s_wallStart;
        static float s_t0;
        static float s_struckAt;
        static int s_phase;
        static Component s_brain;      // backing UdonBehaviour
        static Component s_hitbox;     // backing UdonBehaviour
        static Component s_pool;       // backing UdonBehaviour
        static Transform s_npc;
        static Vector3 s_spawnPos;
        static MethodInfo s_getVar, s_setVar, s_send;

        static void Drive()
        {
            if (SessionState.GetInt(K_ACTIVE, 0) == 0)
            {
                EditorApplication.update -= Drive;
                return;
            }
            if (!EditorApplication.isPlaying)
            {
                if (s_wallStart == 0f)
                    s_wallStart = Time.realtimeSinceStartup;
                if (Time.realtimeSinceStartup - s_wallStart > 300f)
                    Finish(3, "play mode never started within 300 s");
                return;
            }
            if (EditorApplication.isCompiling)
                return;
            if (!s_inited)
            {
                Init();
                return;
            }
            Step();
        }

        static void Init()
        {
            s_wallStart = Time.realtimeSinceStartup;
            s_t0 = Time.time;
            s_phase = 0;
            s_inited = true;

            var hitboxType = Type("LegaiaNpcHitbox");
            var poolType = Type("LegaiaCoinDrops");
            var poolProxy = Object.FindObjectOfType(poolType, true) as Component;
            s_pool = poolProxy != null ? Backing(poolProxy) : null;
            if (s_pool == null)
                Finish(1, "no coin pool in the running scene");

            foreach (var hb in Object.FindObjectsOfType(hitboxType, true))
            {
                var proxy = hb as Component;
                var backing = Backing(proxy);
                object brainProxy = ReadVar(proxy, "brain");
                if (backing == null || brainProxy == null)
                    continue;
                s_hitbox = backing;
                s_brain = brainProxy as Component;
                s_npc = proxy.transform.parent;
                break;
            }
            if (s_hitbox == null || s_brain == null)
                Finish(1, "no villager hitbox with a brain in the running scene");
            Debug.Log("[Legaia] economy soak: watching " +
                (s_npc != null ? s_npc.name : "?") + ".");
        }

        static void Step()
        {
            float t = Time.time - s_t0;
            if (Time.realtimeSinceStartup - s_wallStart > 420f)
            {
                Finish(4, "wall-clock watchdog at t=" + t.ToString("0") + " s");
                return;
            }
            if (s_phase == 0)
            {
                if (t < SETTLE_SECONDS)
                    return;
                // Shorten the sentence so the whole loop fits the budget,
                // then run the broadcast's body directly - a NetworkCallable
                // never fires in editor play mode.
                SetVar(s_brain, "respawnSeconds", RESPAWN_SECONDS);
                s_spawnPos = s_npc != null ? s_npc.position : Vector3.zero;
                Send(s_hitbox, "DebugSlainHere");
                s_struckAt = Time.time;
                s_phase = 1;
                Debug.Log("[Legaia] economy soak: struck at t=" + t.ToString("0.0"));
                return;
            }
            if (s_phase == 1)
            {
                if (Time.time - s_struckAt < 1.5f)
                    return;
                int state = GetInt(s_brain, "state", -1);
                if (state != STATE_DEAD)
                {
                    Finish(1, "the villager was struck but its state is " + state +
                        ", not " + STATE_DEAD + " (Dead)");
                    return;
                }
                if (GetInt(s_brain, "slainCount", 0) < 1)
                {
                    Finish(1, "the villager toppled but slainCount never counted it");
                    return;
                }
                int active = ActiveDrops();
                if (active < 1)
                {
                    Finish(1, "no coin drop went active where the villager fell");
                    return;
                }
                Debug.Log("[Legaia] economy soak: toppled, " + active +
                    " coin drop(s) on the ground.");
                s_phase = 2;
                return;
            }
            if (s_phase == 2)
            {
                if (Time.time - s_struckAt < RESPAWN_SECONDS + 4f)
                    return;
                int state = GetInt(s_brain, "state", -1);
                if (state == STATE_DEAD)
                {
                    Finish(1, "the villager is still down " +
                        (Time.time - s_struckAt).ToString("0") + " s after a " +
                        RESPAWN_SECONDS + " s sentence");
                    return;
                }
                bool shown = false;
                if (s_npc != null)
                    foreach (var r in s_npc.GetComponentsInChildren<Renderer>(true))
                        if (r.enabled)
                        {
                            shown = true;
                            break;
                        }
                if (!shown)
                {
                    Finish(1, "the villager respawned with every renderer still off");
                    return;
                }
                Vector3 d = s_npc.position - s_spawnPos;
                d.y = 0f;
                Debug.Log("[Legaia] ECONOMY SOAK: struck -> " + ActiveDrops() +
                    " drop(s) -> respawned " + d.magnitude.ToString("0.0") +
                    " m from where it fell, state " + state + ".");
                Finish(0, null);
            }
        }

        static int ActiveDrops()
        {
            var arr = GetVar(s_pool, "drops") as System.Array;
            if (arr == null)
                return 0;
            int n = 0;
            for (int i = 0; i < arr.Length; i++)
            {
                var d = arr.GetValue(i) as Component;
                if (d == null)
                    continue;
                object a = GetVar(d, "active");
                if (a is bool && (bool)a)
                    n++;
            }
            return n;
        }

        static void Finish(int code, string why)
        {
            if (code == 0)
                Debug.Log("[Legaia] SELFTEST OK: economy soak complete.");
            else
                Debug.LogError("[Legaia] ECONOMY SOAK FAIL: " + why);
            SessionState.SetInt(K_ACTIVE, 0);
            EditorApplication.update -= Drive;
            if (Application.isBatchMode)
                EditorApplication.Exit(code);
            else
                EditorApplication.isPlaying = false;
        }

        // --- helpers ------------------------------------------------------------

        static GameObject FindSpawn(string scenePath)
        {
            foreach (var t in Object.FindObjectsOfType<Transform>())
                if (t.name == "LegaiaSpawn")
                    return t.gameObject;
            Fail("no LegaiaSpawn in " + scenePath + " - build the scene first");
            return null;
        }

        static object LoadManifest(string sceneName)
        {
            string path = "Assets/LegaiaImports/" + sceneName + "/manifest.json";
            if (!System.IO.File.Exists(path))
                Fail("no manifest at " + path +
                     " - copy the exported scene folder into the project first");
            return MiniJson.Parse(System.IO.File.ReadAllText(path));
        }

        static System.Type Type(string name)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + name);
            if (t == null)
                Fail(name + " is not compiled - read the [UdonSharp] lines above: " +
                     "one U# compile error fails every wire in the scene");
            return t;
        }

        static string PathOf(Transform t)
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

        static string Arg(string name, string fallback)
        {
            var args = System.Environment.GetCommandLineArgs();
            for (int i = 0; i + 1 < args.Length; i++)
                if (args[i] == name)
                    return args[i + 1];
            return fallback;
        }

        static Component Backing(Component proxy)
        {
            return proxy == null ? null : LegaiaCommonPrefabs.BackingUdon(proxy);
        }

        /// A field's value AS THE BACKING BEHAVIOUR HOLDS IT - the only
        /// reading that says anything about what runs in-world.
        ///
        /// Two readings, because an UdonBehaviour has two: in EDIT mode the
        /// values live in the serialized `publicVariables` table and the
        /// program heap does not exist yet, so GetProgramVariable answers
        /// null for every field that was in fact wired. In PLAY mode the
        /// heap is the truth. Try the table first, fall back to the heap.
        static object ReadVar(Component proxy, string name)
        {
            var backing = Backing(proxy);
            if (backing == null)
                return null;
            object v = PublicVar(backing, name);
            return v != null ? v : GetVar(backing, name);
        }

        /// One entry of an UdonBehaviour's serialized public-variable table.
        static object PublicVar(Component backing, string name)
        {
            const BindingFlags ANY = BindingFlags.Public |
                BindingFlags.NonPublic | BindingFlags.Instance;
            var bt = backing.GetType();
            object pv = bt.GetField("publicVariables", ANY)?.GetValue(backing)
                        ?? bt.GetProperty("publicVariables", ANY)?.GetValue(backing);
            if (pv == null)
                return null;
            foreach (var mi in pv.GetType().GetMethods())
                if (mi.Name == "TryGetVariableValue" && !mi.IsGenericMethod &&
                    mi.GetParameters().Length == 2)
                {
                    var args = new object[] { name, null };
                    return (bool)mi.Invoke(pv, args) ? args[1] : null;
                }
            return null;
        }

        static MethodInfo Method(System.Type t, string name, int argc)
        {
            foreach (var m in t.GetMethods())
                if (m.Name == name && !m.IsGenericMethod &&
                    m.GetParameters().Length == argc)
                    return m;
            return null;
        }

        static object GetVar(Component udon, string name)
        {
            if (udon == null)
                return null;
            // The non-generic overload by hand: UdonBehaviour also carries
            // GetProgramVariable<T>(string), and asking GetMethod for a
            // (string) signature is an AmbiguousMatchException.
            if (s_getVar == null)
                s_getVar = Method(udon.GetType(), "GetProgramVariable", 1);
            if (s_getVar == null)
                return null;
            try
            {
                return s_getVar.Invoke(udon, new object[] { name });
            }
            catch
            {
                return null;
            }
        }

        static int GetInt(Component udon, string name, int fallback)
        {
            object v = GetVar(udon, name);
            return v is int ? (int)v : fallback;
        }

        static void SetVar(Component udon, string name, object value)
        {
            if (udon == null)
                return;
            if (s_setVar == null)
                s_setVar = Method(udon.GetType(), "SetProgramVariable", 2);
            if (s_setVar == null)
                return;
            try
            {
                s_setVar.Invoke(udon, new object[] { name, value });
            }
            catch
            {
            }
        }

        static void Send(Component udon, string ev)
        {
            if (udon == null)
                return;
            if (s_send == null)
                s_send = Method(udon.GetType(), "SendCustomEvent", 1);
            if (s_send == null)
                return;
            try
            {
                s_send.Invoke(udon, new object[] { ev });
            }
            catch (System.Exception e)
            {
                Debug.LogError("[Legaia] economy soak: " + ev + " threw: " +
                    (e.InnerException ?? e).Message);
            }
        }
    }
}
