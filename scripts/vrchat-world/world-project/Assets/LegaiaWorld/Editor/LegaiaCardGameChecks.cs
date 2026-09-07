// Headless checks for the card table's game.
//
//   Run  (edit mode, -quit is fine)
//     Unity.exe -batchmode -nographics -quit -projectPath <copy>
//         -executeMethod LegaiaWorld.LegaiaCardGameChecks.Run
//         [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
//   Builds the common prefabs on the scene's built root, asserts the
//   `game` child stands under card_table with every field wired on its
//   BACKING behaviour (the U# proxy is not what runs in-world), renders a
//   portrait for every villager in the scene and asserts each brain got
//   one, and then drives the poker evaluator and the blackjack settlement
//   as plain C# on a throwaway GameObject - the U# proxy is an ordinary
//   MonoBehaviour in the editor, so its rules are directly callable with
//   no Udon runtime and no networking (the same trick LegaiaSlotTools
//   uses for the slot machine's parity fixture). An off-by-one in a
//   kicker or a 3:2 rounding slip compiles fine and would otherwise only
//   show up as somebody quietly losing coins.
//
//   Soak  (play mode - NO -quit; it exits itself)
//     Unity.exe -batchmode -nographics -projectPath <copy>
//         -executeMethod LegaiaWorld.LegaiaCardGameChecks.Soak
//         [-legaiaCardSeconds 150] [-legaiaCardScale 2]
//         [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
//   Enters play mode with nobody at the table and lets the villagers play
//   themselves (there is no local player at all in a headless editor -
//   Networking.LocalPlayer is null - which the game reads as "I am the
//   only simulation" and runs the dealer side). It samples the game's
//   BACKING behaviour every frame and asserts: hands actually complete,
//   every stool was sat in at some point, the pot is exactly what the
//   seats paid in and exactly what the winners took out, and no villager
//   ever goes chip-negative. Exits 0 pass / 1 assertion / 3 play mode
//   never started / 4 watchdog, like LegaiaSoak.

using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaCardGameChecks
    {
        // --- shared plumbing ----------------------------------------------------

        static string Arg(string name, string fallback)
        {
            var args = System.Environment.GetCommandLineArgs();
            for (int i = 0; i + 1 < args.Length; i++)
                if (args[i] == name)
                    return args[i + 1];
            return fallback;
        }

        static void Fail(string msg)
        {
            Debug.LogError("[Legaia] CARDS FAIL: " + msg);
            if (Application.isBatchMode)
                EditorApplication.Exit(1);
            throw new System.Exception(msg);
        }

        static readonly BindingFlags ANY =
            BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;

        static object Field(Component c, string name)
        {
            if (c == null)
                return null;
            var f = c.GetType().GetField(name, ANY);
            return f == null ? null : f.GetValue(c);
        }

        static MethodInfo Method(System.Type t, string name, int argc)
        {
            foreach (var m in t.GetMethods())
                if (m.Name == name && !m.IsGenericMethod &&
                    m.GetParameters().Length == argc)
                    return m;
            return null;
        }

        static MethodInfo s_tryGet;

        /// A wired value off a BACKING UdonBehaviour in EDIT mode. The heap
        /// only exists while the program runs, so the serialized side is
        /// `publicVariables.TryGetVariableValue` - the same reader the kit's
        /// other build-time checks use. GetProgramVariable answers null here
        /// even for a field that copied down perfectly.
        static object EditVar(Component udon, string name)
        {
            if (udon == null)
                return null;
            var bt = udon.GetType();
            object pv = bt.GetField("publicVariables", ANY)?.GetValue(udon)
                        ?? bt.GetProperty("publicVariables", ANY)?.GetValue(udon);
            if (pv == null)
                Fail("the backing behaviour exposes no publicVariables");
            if (s_tryGet == null)
                foreach (var mi in pv.GetType().GetMethods())
                    if (mi.Name == "TryGetVariableValue" && !mi.IsGenericMethod &&
                        mi.GetParameters().Length == 2)
                    {
                        s_tryGet = mi;
                        break;
                    }
            if (s_tryGet == null)
                Fail("no TryGetVariableValue on " + pv.GetType().Name);
            var args = new object[] { name, null };
            return (bool)s_tryGet.Invoke(pv, args) ? args[1] : null;
        }

        static MethodInfo s_getVar;

        /// The same value at RUNTIME, off the program's live heap - what the
        /// play-mode soak samples.
        static object Var(Component udon, string name)
        {
            if (udon == null)
                return null;
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

        static int VarInt(Component udon, string name, int fallback)
        {
            object v = Var(udon, name);
            return v is int ? (int)v : fallback;
        }

        static int[] VarInts(Component udon, string name)
        {
            return Var(udon, name) as int[];
        }

        static GameObject OpenBuiltScene(out string sceneName, out GameObject root)
        {
            string scenePath = Arg("-legaiaScene",
                "Assets/Scenes/VRCDefaultWorldScene.unity");
            LegaiaWorldBuilder.EnsureUdonProgramAssets();
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
            sceneName = rootT.name.Substring("Legaia_".Length);
            root = rootT.gameObject;
            return spawn;
        }

        static GameObject BuildPrefabs(GameObject spawn, string sceneName)
        {
            var o = new LegaiaCommonPrefabOptions
            {
                mirror = true, tv = true, cardTable = true, seats = 4,
            };
            var settings = LegaiaSceneSettings.Load(sceneName);
            var container = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + sceneName, spawn.transform.position, o,
                settings.prefabTransforms, settings.slotMachine);
            if (container == null)
                Fail("no common-prefab container built");
            return container;
        }

        // --- edit-mode checks ----------------------------------------------------

        public static void Run()
        {
            string sceneName;
            GameObject root;
            var spawn = OpenBuiltScene(out sceneName, out root);
            // The living town first, as the soak does: the names and
            // portraits under test are the ones THIS kit assigns, not
            // whatever the saved scene carries from an older build.
            string manifestPath = "Assets/LegaiaImports/" + sceneName + "/manifest.json";
            if (File.Exists(manifestPath))
            {
                object manifest = MiniJson.Parse(File.ReadAllText(manifestPath));
                var sceneSettings = LegaiaSceneSettings.Load(sceneName);
                string manifestDir = "Assets/LegaiaImports/" + sceneName;
                sceneSettings.ApplyNpcOverrides(manifest, manifestDir, root);
                LegaiaWorldBuilder.ReconcileNpcs(manifest, manifestDir, root, sceneName,
                    sceneSettings);
                LegaiaLivingTown.Apply(root, manifest, sceneName,
                    new LegaiaLivingTownOptions(), sceneSettings);
            }
            var container = BuildPrefabs(spawn, sceneName);

            var table = container.transform.Find("card_table");
            if (table == null)
                Fail("no card_table under " + container.name);
            var game = table.Find("game");
            if (game == null)
                Fail("no `game` child under card_table - the town director " +
                     "finds it by that exact path");

            var gameType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardGame");
            if (gameType == null)
                Fail("LegaiaCardGame is not compiled (VRChat SDK / UdonSharp missing?)");
            var proxy = game.GetComponent(gameType) as Component;
            if (proxy == null)
                Fail("the `game` object carries no LegaiaCardGame");
            var backing = LegaiaCommonPrefabs.BackingUdon(proxy);
            if (backing == null)
                Fail("LegaiaCardGame has no backing UdonBehaviour - " +
                     "it would be inert in-world");
            // The director pushes itself in by name; the field has to exist.
            if (proxy.GetType().GetField("director") == null)
                Fail("LegaiaCardGame declares no public `director` field - " +
                     "LegaiaTownDirector.LinkCardGame could not reach it");

            CheckRef(proxy, backing, "host");
            CheckRef(proxy, backing, "deck");
            CheckRef(proxy, backing, "wallet");
            CheckRef(proxy, backing, "dealerAnchor");
            CheckRef(proxy, backing, "stackAnchor");
            CheckArray(proxy, backing, "stations", 4);
            CheckArray(proxy, backing, "chairs", 4);
            CheckArray(proxy, backing, "cards", 52);
            CheckArray(proxy, backing, "handAnchors", 4);
            CheckArray(proxy, backing, "rowPortrait", 4);
            CheckArray(proxy, backing, "rowName", 4);
            CheckArray(proxy, backing, "rowCoins", 4);
            CheckArray(proxy, backing, "rowStatus", 4);
            CheckArray(proxy, backing, "btnHold", 5);
            CheckArray(proxy, backing, "btnHoldText", 5);
            foreach (string b in new[] { "btnDeal", "btnMode", "btnCall", "btnRaise",
                                         "btnFold", "btnDraw", "btnHit", "btnStand" })
                CheckRef(proxy, backing, b);
            foreach (string t in new[] { "modeText", "potText", "msgText", "talkText",
                                         "btnCallText", "btnRaiseText", "btnDealText",
                                         "talk" })
                CheckRef(proxy, backing, t);

            // The panel's clicks must land on the BACKING behaviour: a
            // persistent listener onto the U# proxy does nothing in-world.
            int wired = 0;
            foreach (var btn in game.transform.parent
                         .GetComponentsInChildren<UnityEngine.UI.Button>(true))
            {
                int n = btn.onClick.GetPersistentEventCount();
                for (int i = 0; i < n; i++)
                {
                    var target = btn.onClick.GetPersistentTarget(i);
                    if (target == null)
                        Fail(btn.name + ": persistent listener with no target");
                    if (target == (Object)proxy)
                        Fail(btn.name + ": listener points at the U# PROXY, not " +
                             "the backing UdonBehaviour - it would be dead in-world");
                    if (target == (Object)backing)
                        wired++;
                }
            }
            if (wired < 13)
                Fail("only " + wired + " panel button(s) wired into the game " +
                     "behaviour, expected 13");

            var shape = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCUiShape");
            if (shape != null &&
                table.GetComponentsInChildren(shape, true).Length != 1)
                Fail("the seat panel's canvas has no VRCUiShape - its buttons " +
                     "would never receive a pointer");

            // A world-space canvas is legible from its -Z side (the viewer
            // looks along its +Z), and GraphicRaycaster drops rays that
            // arrive from the +Z side. Facing the wrong way is invisible in
            // a component count and total in-world (every line mirror-
            // written, no button takes a press), so the SIGN is measured:
            // the canvas's +Z must point INTO the table, so that a reader
            // standing outside it is on the -Z side. The aim is authored in
            // the table's local frame precisely because the scene's hand
            // placement re-rotates the root after the builder returns, so
            // this is measured against the table and not against spawn.
            var panel = table.Find("panel");
            var canvas = table.Find("panel/canvas");
            if (panel == null || canvas == null)
                Fail("no panel/canvas beside the card table");
            // The canvas sits on the panel root's -Z face whatever the
            // placement: that is the side the text reads from.
            if (canvas.localPosition.z > -0.005f)
                Fail("the seat panel's canvas is not on its root's -Z face");
            Vector3 out2 = canvas.position - table.position;
            out2.y = 0f;
            float facing = Vector3.Dot(canvas.forward, out2.normalized);
            var settings = LegaiaSceneSettings.Load(sceneName);
            LegaiaPrefabTransform panelPlace;
            if (settings.prefabTransforms.TryGetValue("card_table_panel", out panelPlace))
            {
                // Hand-placed: the settings value must come back digit for
                // digit, and which way it reads is the scene's choice.
                if ((panel.localPosition - panelPlace.position).magnitude > 1e-3f)
                    Fail("card_table_panel: built at local " + panel.localPosition +
                         ", settings say " + panelPlace.position);
                if (panelPlace.hasRotation &&
                    Quaternion.Angle(panel.localRotation,
                        Quaternion.Euler(panelPlace.rotation)) > 0.1f)
                    Fail("card_table_panel: built at local rotation " +
                         panel.localEulerAngles + ", settings say " + panelPlace.rotation);
                Debug.Log("[Legaia] CARDS: panel hand-placed from settings at local " +
                    panel.localPosition + " / yaw " +
                    panel.localEulerAngles.y.ToString("0.0") + " - it reads from " +
                    (facing < 0f ? "OUTSIDE the table" : "the STOOLS (inside)") +
                    " (canvas +Z vs outward dot " + facing.ToString("0.00") + ").");
            }
            else
            {
                if (facing > -0.85f)
                    Fail("the seat panel's canvas presents its +Z to the outside of " +
                         "the table (dot " + facing.ToString("0.00") + ") - the text " +
                         "reads mirrored and the buttons reject every press");
                // ... and the canvas must stand on the outside of the board, not
                // between the board and the table, or the board hides it.
                Vector3 boardOut = canvas.position - panel.position;
                boardOut.y = 0f;
                if (Vector3.Dot(boardOut, out2.normalized) < 0.005f)
                    Fail("the seat panel's canvas is on the table side of its board");
                Vector3 toSpawn = spawn.transform.position - canvas.position;
                toSpawn.y = 0f;
                Debug.Log("[Legaia] CARDS: panel reads from outside the table (canvas +Z " +
                    "into the table, dot " + facing.ToString("0.00") + "), spawn side dot " +
                    Vector3.Dot(-canvas.forward, toSpawn.normalized).ToString("0.00") + ".");
            }
            // ... and it must not stand where a player sits.
            for (int i = 0; i < 4; i++)
            {
                var stool = table.Find("stool_" + i);
                if (stool == null)
                    continue;
                Vector3 d = stool.position - panel.position;
                d.y = 0f;
                if (d.magnitude < 0.6f)
                    Fail("the seat panel's post stands " + d.magnitude.ToString("0.00") +
                         " m from stool_" + i);
            }

            CheckPortraits(root, sceneName);
            CheckNames(root);
            CheckTalk(game.gameObject);
            CheckPoker();
            CheckBlackjack();
            Debug.Log("[Legaia] CARDS: seat panel wired (" + wired +
                " buttons), evaluator and settlement cases pass.");
            Debug.Log("[Legaia] SELFTEST OK: card game.");
        }

        static void CheckRef(Component proxy, Component backing, string name)
        {
            if (Field(proxy, name) == null)
                Fail("LegaiaCardGame." + name + " is not wired on the proxy");
            if (EditVar(backing, name) == null)
                Fail("LegaiaCardGame." + name + " never reached the backing " +
                     "behaviour (missing CopyProxyToUdon?)");
        }

        static void CheckArray(Component proxy, Component backing, string name, int n)
        {
            var arr = Field(proxy, name) as System.Array;
            if (arr == null || arr.Length != n)
                Fail("LegaiaCardGame." + name + ": expected " + n + " entries, got " +
                     (arr == null ? "null" : "" + arr.Length));
            for (int i = 0; i < arr.Length; i++)
                if (arr.GetValue(i) == null)
                    Fail("LegaiaCardGame." + name + "[" + i + "] is null");
            var live = EditVar(backing, name) as System.Array;
            if (live == null || live.Length != n)
                Fail("LegaiaCardGame." + name + " never reached the backing " +
                     "behaviour with " + n + " entries");
        }

        // --- portraits -------------------------------------------------------------

        static void CheckPortraits(GameObject root, string sceneName)
        {
            var brainType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcBrain");
            if (brainType == null)
                return;
            var brains = new List<Component>();
            foreach (var c in root.GetComponentsInChildren(brainType, true))
                brains.Add(c as Component);
            if (brains.Count == 0)
            {
                Debug.Log("[Legaia] CARDS: no villagers in this scene - " +
                    "portraits not exercised.");
                return;
            }
            int made = LegaiaPortraits.Apply(root, brains, sceneName,
                "Assets/LegaiaGenerated/" + sceneName);
            if (made != brains.Count)
                Fail("portraits: " + made + " made for " + brains.Count + " villager(s)");
            for (int i = 0; i < brains.Count; i++)
                if (Field(brains[i], "portrait") == null)
                    Fail("portraits: " + brains[i].gameObject.name +
                         " has no portrait after the pass");
            // Nothing may be left standing in the scene.
            foreach (var cam in Object.FindObjectsOfType<Camera>(true))
                if (cam.name.StartsWith("~legaia-portrait"))
                    Fail("portraits: a temporary camera was left in the scene");
            Debug.Log("[Legaia] CARDS: " + made + " villager portrait(s) set.");
        }

        /// Every villager's `label` is a NAME now, never the retail dialogue
        /// fragment the manifest carries ("Tetsu: You were a child when the",
        /// "I am a dummy.") - short, no sentence punctuation, unique.
        static void CheckNames(GameObject root)
        {
            var brainType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcBrain");
            if (brainType == null)
                return;
            var seen = new HashSet<string>();
            var cast = new List<string>();
            foreach (var c in root.GetComponentsInChildren(brainType, true))
            {
                string label = Field(c, "label") as string;
                if (string.IsNullOrEmpty(label))
                    Fail("names: " + c.gameObject.name + " has no label");
                if (label.IndexOfAny(new[] { ':', '.', '!', ',', '?' }) >= 0 ||
                    label.Length > 20 || label.Split(' ').Length > 2)
                    Fail("names: " + c.gameObject.name + " is labelled '" + label +
                         "' - that is dialogue, not a name");
                if (!seen.Add(label))
                    Fail("names: two villagers are both called '" + label + "'");
                cast.Add(label);
            }
            Debug.Log("[Legaia] CARDS: cast of " + cast.Count + " - " +
                string.Join(", ", cast) + ".");
        }

        /// The table talk composes a line of the right shape for every kind
        /// and every voice, talks ABOUT the heroes when they sit, and never
        /// repeats itself back to back.
        static void CheckTalk(GameObject game)
        {
            var talk = game.GetComponent<LegaiaTableTalk>();
            if (talk == null)
                Fail("talk: the game object carries no LegaiaTableTalk");
            int longest = talk.LongestLine();
            if (longest > talk.maxLength)
                Fail("talk: a pool line runs " + longest + " characters, panel holds " +
                     talk.maxLength);
            for (int k = 0; k < LegaiaTableTalk.KIND_COUNT; k++)
            {
                string v = talk.Compose(k, "Bram", "", k * 3 + 1);
                if (!v.StartsWith("Bram: ") || v.Length < 12)
                    Fail("talk: villager kind " + k + " composed '" + v + "'");
                string vahn = talk.Compose(k, "Vahn", "", k * 5 + 2);
                if (vahn.Length < 8 || !vahn.Contains("Vahn") || vahn.Contains(":") ||
                    vahn.Contains("*"))
                    Fail("talk: Vahn kind " + k + " composed '" + vahn +
                         "' - he never speaks a quoted word");
                string noa = talk.Compose(k, "Noa", "", k * 7 + 3);
                if (!noa.StartsWith("Noa: ") || noa.IndexOf("Noa", 5) < 0)
                    Fail("talk: Noa kind " + k + " composed '" + noa + "'");
            }
            bool aboutVahn = false, aboutNoa = false, aboutBoth = false;
            for (int salt = 0; salt < 64; salt += 2)
            {
                string a = talk.Compose(LegaiaTableTalk.T_SIT, "Bram", "vahn", salt);
                if (a.Contains("Vahn") || a.Contains("Val") || a.Contains("Meta"))
                    aboutVahn = true;
                string b = talk.Compose(LegaiaTableTalk.T_IDLE, "Bram", "player|noa", salt);
                if (b.Contains("Noa") || b.Contains("wolf") || b.Contains("Snowdrift"))
                    aboutNoa = true;
                string c = talk.Compose(LegaiaTableTalk.T_DEAL, "Bram", "vahn|noa", salt);
                if (c.Contains("Vahn") && c.Contains("Noa"))
                    aboutBoth = true;
            }
            if (!aboutVahn || !aboutNoa || !aboutBoth)
                Fail("talk: the villagers never talked about the heroes at the table " +
                     "(Vahn " + aboutVahn + ", Noa " + aboutNoa + ", both " + aboutBoth + ")");
            string prev = null;
            for (int i = 0; i < 40; i++)
            {
                string line = talk.Compose(LegaiaTableTalk.T_IDLE, "Bram", "", i * 7 + 3);
                if (line == prev)
                    Fail("talk: the same idle line twice in a row ('" + line + "')");
                prev = line;
            }
            Debug.Log("[Legaia] CARDS: table talk composes for " +
                LegaiaTableTalk.KIND_COUNT + " kinds x 3 voices, longest line " +
                longest + " chars; e.g. '" +
                talk.Compose(LegaiaTableTalk.T_SIT, "Rena", "vahn", 4) + "'");
        }

        // --- rules ------------------------------------------------------------------

        // Card index = rank + 13 * suit; rank 0 = ace, 12 = king.
        // Suits, in build order: 0 spades, 1 hearts, 2 diamonds, 3 clubs.
        const int A = 0, R2 = 1, R3 = 2, R4 = 3, R5 = 4, R6 = 5, R7 = 6, R8 = 7,
                  R9 = 8, T = 9, J = 10, Q = 11, K = 12;
        const int S = 0, H = 1, D = 2, C = 3;

        static int Card(int rank, int suit) { return rank + 13 * suit; }

        static void CheckPoker()
        {
            var go = new GameObject("~legaia-card-rules");
            go.hideFlags = HideFlags.HideAndDontSave;
            int cases = 0;
            try
            {
                var g = go.AddComponent<LegaiaCardGame>();
                g.suppressSerialization = true;

                int royal = g.Eval5(Card(A, S), Card(K, S), Card(Q, S), Card(J, S), Card(T, S));
                int sflush = g.Eval5(Card(R9, S), Card(R8, S), Card(R7, S), Card(R6, S), Card(R5, S));
                int quads = g.Eval5(Card(K, S), Card(K, H), Card(K, D), Card(K, C), Card(R2, S));
                int full = g.Eval5(Card(A, S), Card(A, H), Card(A, D), Card(K, C), Card(K, S));
                int flush = g.Eval5(Card(A, H), Card(J, H), Card(R9, H), Card(R5, H), Card(R3, H));
                int straight = g.Eval5(Card(R9, S), Card(R8, H), Card(R7, D), Card(R6, C), Card(R5, S));
                int trips = g.Eval5(Card(Q, S), Card(Q, H), Card(Q, D), Card(R7, C), Card(R2, S));
                int twoPair = g.Eval5(Card(A, S), Card(A, H), Card(K, D), Card(K, C), Card(R5, S));
                int pair = g.Eval5(Card(A, S), Card(A, H), Card(R7, D), Card(R5, C), Card(R3, S));
                int high = g.Eval5(Card(A, S), Card(K, H), Card(Q, D), Card(J, C), Card(R9, S));

                cases += Order("royal flush > straight flush", royal, sflush);
                cases += Order("straight flush > four of a kind", sflush, quads);
                cases += Order("four of a kind > full house", quads, full);
                cases += Order("full house > flush", full, flush);
                cases += Order("flush > straight", flush, straight);
                cases += Order("straight > three of a kind", straight, trips);
                cases += Order("three of a kind > two pair", trips, twoPair);
                cases += Order("two pair > pair", twoPair, pair);
                cases += Order("pair > high card", pair, high);

                // Kickers.
                int aaKQJ = g.Eval5(Card(A, S), Card(A, H), Card(K, D), Card(Q, C), Card(J, S));
                int aaKQ9 = g.Eval5(Card(A, S), Card(A, H), Card(K, D), Card(Q, C), Card(R9, S));
                cases += Order("pair kicker: AA KQJ > AA KQ9", aaKQJ, aaKQ9);
                int aaKQJ2 = g.Eval5(Card(A, D), Card(A, C), Card(K, S), Card(Q, H), Card(J, D));
                if (aaKQJ != aaKQJ2)
                    Fail("the same ranks in different suits scored differently");
                cases++;

                // The wheel: A-5 is a straight, and the LOWEST one.
                int wheel = g.Eval5(Card(A, S), Card(R2, H), Card(R3, D), Card(R4, C), Card(R5, S));
                int sixHigh = g.Eval5(Card(R2, S), Card(R3, H), Card(R4, D), Card(R5, C), Card(R6, S));
                if (wheel / 759375 != 4)
                    Fail("the wheel (A-2-3-4-5) did not score as a straight");
                cases++;
                cases += Order("6-high straight > the wheel", sixHigh, wheel);
                cases += Order("the wheel > three of a kind", wheel, trips);
                // A wheel in one suit is a straight flush, not an ace-high one.
                int wheelFlush = g.Eval5(Card(A, H), Card(R2, H), Card(R3, H), Card(R4, H), Card(R5, H));
                if (wheelFlush / 759375 != 8)
                    Fail("a suited wheel did not score as a straight flush");
                cases += Order("royal flush > wheel straight flush", royal, wheelFlush);

                // Ace-high straight, unsuited.
                int broadway = g.Eval5(Card(A, S), Card(K, H), Card(Q, D), Card(J, C), Card(T, S));
                if (broadway / 759375 != 4)
                    Fail("10-J-Q-K-A did not score as a straight");
                cases += Order("broadway > 9-high straight", broadway, straight);

                // Blackjack totals off real cards.
                g.seatCards = new[] { Card(A, S), Card(K, H), -1, -1, -1 };
                if (g.HandValue(0) != 21)
                    Fail("A + K should be 21, got " + g.HandValue(0));
                cases++;
                g.seatCards = new[] { Card(A, S), Card(A, H), Card(R9, D), -1, -1 };
                if (g.HandValue(0) != 21)
                    Fail("A + A + 9 should be 21 (one ace demoted), got " + g.HandValue(0));
                cases++;
                g.seatCards = new[] { Card(K, S), Card(Q, H), Card(R5, D), -1, -1 };
                if (g.HandValue(0) != 25)
                    Fail("K + Q + 5 should bust at 25, got " + g.HandValue(0));
                cases++;
            }
            finally
            {
                Object.DestroyImmediate(go);
            }
            Debug.Log("[Legaia] CARDS: " + cases + " poker evaluator case(s) pass.");
        }

        static int Order(string what, int higher, int lower)
        {
            if (higher <= lower)
                Fail(what + " - scored " + higher + " vs " + lower);
            return 1;
        }

        static void CheckBlackjack()
        {
            var go = new GameObject("~legaia-card-bj");
            go.hideFlags = HideFlags.HideAndDontSave;
            int cases = 0;
            try
            {
                var g = go.AddComponent<LegaiaCardGame>();
                g.suppressSerialization = true;
                cases += Pay(g, "natural pays 3:2", 21, 2, 20, 2, 4, 10);
                cases += Pay(g, "natural 3:2 rounds down", 21, 2, 19, 2, 5, 12);
                cases += Pay(g, "natural vs natural pushes", 21, 2, 21, 2, 4, 4);
                cases += Pay(g, "dealer natural beats a drawn 21", 21, 3, 21, 2, 4, 0);
                cases += Pay(g, "a bust pays nothing", 22, 3, 18, 2, 4, 0);
                cases += Pay(g, "a bust loses even to a dealer bust", 23, 3, 24, 4, 4, 0);
                cases += Pay(g, "dealer bust pays 1:1", 18, 3, 24, 4, 4, 8);
                cases += Pay(g, "a higher total pays 1:1", 20, 3, 18, 3, 4, 8);
                cases += Pay(g, "an equal total pushes", 18, 3, 18, 3, 4, 4);
                cases += Pay(g, "a lower total pays nothing", 18, 3, 20, 3, 4, 0);
            }
            finally
            {
                Object.DestroyImmediate(go);
            }
            Debug.Log("[Legaia] CARDS: " + cases + " blackjack settlement case(s) pass.");
        }

        static int Pay(LegaiaCardGame g, string what, int pv, int pc, int dv, int dc,
            int stake, int want)
        {
            int got = g.BlackjackPayout(pv, pc, dv, dc, stake);
            if (got != want)
                Fail(what + " - returned " + got + ", expected " + want);
            return 1;
        }

        // --- play-mode soak ---------------------------------------------------------

        const string K_ACTIVE = "legaia.cards.active";
        const string K_SECONDS = "legaia.cards.seconds";
        const string K_SCALE = "legaia.cards.scale";

        public static void Soak()
        {
            string sceneName;
            GameObject root;
            var spawn = OpenBuiltScene(out sceneName, out root);

            // The living town first (the table needs villagers to summon),
            // then the prefabs, so the kit under test is what runs.
            string manifestPath = "Assets/LegaiaImports/" + sceneName + "/manifest.json";
            if (!File.Exists(manifestPath))
                Fail("no manifest at " + manifestPath);
            object manifest = MiniJson.Parse(File.ReadAllText(manifestPath));
            var settings = LegaiaSceneSettings.Load(sceneName);
            string manifestDir = "Assets/LegaiaImports/" + sceneName;
            settings.ApplyNpcOverrides(manifest, manifestDir, root);
            LegaiaWorldBuilder.ReconcileNpcs(manifest, manifestDir, root, sceneName, settings);
            if (LegaiaLivingTown.Apply(root, manifest, sceneName,
                    new LegaiaLivingTownOptions(), settings) == null)
                Fail("the living-town pass built nothing - no villagers to play cards");
            BuildPrefabs(spawn, sceneName);

            LegaiaSoak.StripRendering();
            float seconds = ParseFloat(Arg("-legaiaCardSeconds", "200"), 200f);
            float scale = Mathf.Clamp(ParseFloat(Arg("-legaiaCardScale", "2"), 2f),
                0.25f, 8f);
            SessionState.SetFloat(K_SECONDS, Mathf.Max(20f, seconds));
            SessionState.SetFloat(K_SCALE, scale);
            SessionState.SetInt(K_ACTIVE, 1);
            Debug.Log("[Legaia] CARDS: entering play mode - " + seconds +
                " simulated s at timeScale " + scale + ", villagers only.");
            EditorApplication.EnterPlaymode();
        }

        static float ParseFloat(string s, float fallback)
        {
            float v;
            return float.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture,
                out v) ? v : fallback;
        }

        [InitializeOnLoadMethod]
        static void Hook()
        {
            if (SessionState.GetInt(K_ACTIVE, 0) == 0)
                return;
            EditorApplication.update -= Drive;
            EditorApplication.update += Drive;
        }

        static bool s_inited;
        static float s_wallStart, s_simStart, s_seconds, s_scale, s_nextLine;
        static Component s_game;
        static Component s_host;
        static Transform[] s_stools;
        static Component[] s_stations;
        static int s_seatedSamples;
        static float s_minSeatY = 99f, s_maxSeatY = -99f;
        static readonly HashSet<string> s_talkSeen = new HashSet<string>();
        static bool[] s_sawNpc;
        static int s_lastSettle = -1;
        static int s_settlements;
        static bool s_switched;
        static int s_handsAtSwitch;
        static int s_exceptions;
        static readonly List<string> s_problems = new List<string>();

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
            s_seconds = SessionState.GetFloat(K_SECONDS, 200f);
            s_scale = SessionState.GetFloat(K_SCALE, 2f);
            Time.timeScale = s_scale;

            var go = GameObject.Find("Legaia_common_prefabs/card_table/game");
            if (go == null)
            {
                Finish(1, "no Legaia_common_prefabs/card_table/game in the scene");
                return;
            }
            var gameType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardGame");
            var proxy = gameType != null ? go.GetComponent(gameType) as Component : null;
            s_game = LegaiaCommonPrefabs.BackingUdon(proxy);
            if (s_game == null)
            {
                Finish(1, "the card game has no backing UdonBehaviour");
                return;
            }
            var hostType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardTableHost");
            if (hostType != null)
                s_host = Object.FindObjectOfType(hostType, true) as Component;
            // The stools and their stations: the seated-pose check reads
            // each station's `currentNpc` off the live heap.
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            s_stools = new Transform[4];
            s_stations = new Component[4];
            for (int i = 0; i < 4; i++)
            {
                var stool = GameObject.Find("Legaia_common_prefabs/card_table/stool_" + i);
                s_stools[i] = stool != null ? stool.transform : null;
                var st = stool != null && stationType != null
                    ? stool.GetComponent(stationType) as Component : null;
                s_stations[i] = LegaiaCommonPrefabs.BackingUdon(st);
            }
            s_seatedSamples = 0;
            s_minSeatY = 99f;
            s_maxSeatY = -99f;
            s_talkSeen.Clear();

            ForceDay();
            s_sawNpc = new bool[4];
            s_switched = false;
            s_handsAtSwitch = 0;
            s_simStart = Time.time;
            s_wallStart = Time.realtimeSinceStartup;
            s_nextLine = 0f;
            s_lastSettle = VarInt(s_game, "settleSerial", 0);
            Application.logMessageReceived -= OnLog;
            Application.logMessageReceived += OnLog;
            s_inited = true;
            Debug.Log("[Legaia] CARDS: play mode up - " +
                Object.FindObjectsOfType<Transform>().Length +
                " transforms, self-play " + Var(s_game, "npcSelfPlay") + ".");
        }

        /// Pin the clock to daytime for the whole soak. At night the
        /// villagers go home through their doors and there is nobody
        /// outdoors for the table to summon, which makes "did every stool
        /// fill" a test of the hour rather than of the table. The
        /// behaviour is switched off first: it rewrites these fields every
        /// frame off the server clock (LegaiaSoak does the same).
        static void ForceDay()
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
            var proxy = t != null ? Object.FindObjectOfType(t, true) as Component : null;
            var udon = LegaiaCommonPrefabs.BackingUdon(proxy);
            if (udon == null)
            {
                Debug.Log("[Legaia] CARDS: no day/night cycle in the scene.");
                return;
            }
            var beh = udon as Behaviour;
            if (beh != null)
                beh.enabled = false;
            var set = Method(udon.GetType(), "SetProgramVariable", 2);
            if (set == null)
                return;
            set.Invoke(udon, new object[] { "isNight", false });
            set.Invoke(udon, new object[] { "dayFactor", 1f });
            set.Invoke(udon, new object[] { "sunUp", 1f });
            set.Invoke(udon, new object[] { "phase", 0.35f });
            Debug.Log("[Legaia] CARDS: clock forced to day.");
        }

        static void OnLog(string condition, string stack, LogType type)
        {
            if (type != LogType.Exception)
                return;
            if (condition.Contains("Legaia") || stack.Contains("Legaia"))
            {
                s_exceptions++;
                if (s_problems.Count < 8)
                    s_problems.Add(condition);
            }
        }

        static void Step()
        {
            float t = Time.time - s_simStart;
            float wall = Time.realtimeSinceStartup - s_wallStart;
            if (wall > s_seconds / Mathf.Max(0.25f, s_scale) + 420f)
            {
                Finish(4, "wall-clock watchdog: " + wall.ToString("0") +
                    " s elapsed for " + t.ToString("0") + " simulated s");
                return;
            }

            var kind = VarInts(s_game, "seatKind");
            if (kind != null)
                for (int i = 0; i < kind.Length && i < s_sawNpc.Length; i++)
                    if (kind[i] == 2)
                        s_sawNpc[i] = true;

            // Seated villagers sit ON the stool: never below the floor the
            // stool stands on (the first cut sank them to the waist), never
            // standing on top of it either.
            for (int i = 0; i < 4; i++)
            {
                if (s_stools[i] == null || s_stations[i] == null)
                    continue;
                var npc = Var(s_stations[i], "currentNpc") as Transform;
                if (npc == null)
                    continue;
                Vector3 d = npc.position - s_stools[i].position;
                float lift = d.y;
                d.y = 0f;
                if (d.magnitude > 0.35f)
                    continue;   // still walking up
                s_seatedSamples++;
                s_minSeatY = Mathf.Min(s_minSeatY, lift);
                s_maxSeatY = Mathf.Max(s_maxSeatY, lift);
                if (lift < -0.03f)
                {
                    Finish(1, npc.name + " sits " + (-lift).ToString("0.00") +
                        " m BELOW the floor of stool_" + i);
                    return;
                }
                if (lift > 0.5f)
                {
                    Finish(1, npc.name + " stands " + lift.ToString("0.00") +
                        " m above stool_" + i + "'s floor - on the seat, not in it");
                    return;
                }
            }

            // What the table says: names in front, never the retail line.
            string said = Var(s_game, "talkLine") as string;
            if (!string.IsNullOrEmpty(said))
                foreach (string line in said.Split('\n'))
                    if (line.Length > 0 && s_talkSeen.Add(line))
                        Debug.Log("[Legaia] CARDS: talk - " + line);

            // Chips can never go below zero, hand or no hand.
            var chips = VarInts(s_game, "seatChips");
            if (chips != null)
                for (int i = 0; i < chips.Length; i++)
                    if (chips[i] < 0)
                    {
                        Finish(1, "seat " + i + " went chip-negative (" + chips[i] + ")");
                        return;
                    }

            // Every settlement is a closed pot: what the seats paid in is
            // what the winners took out, and the net across the table is 0
            // (self-play never leaves poker, where the table is not a house).
            int serial = VarInt(s_game, "settleSerial", s_lastSettle);
            if (serial != s_lastSettle)
            {
                s_lastSettle = serial;
                s_settlements++;
                int pot = VarInt(s_game, "pot", 0);
                var paid = VarInts(s_game, "seatPaid");
                var won = VarInts(s_game, "seatWon");
                var res = VarInts(s_game, "seatResult");
                int sp = Sum(paid), sw = Sum(won), sr = Sum(res);
                if (VarInt(s_game, "mode", 0) == 0)
                {
                    if (sp != pot)
                        Finish(1, "settlement " + s_settlements + ": seats paid " +
                            sp + " but the pot holds " + pot);
                    else if (sw != pot)
                        Finish(1, "settlement " + s_settlements + ": winners took " +
                            sw + " out of a pot of " + pot);
                    else if (sr != 0)
                        Finish(1, "settlement " + s_settlements + ": the table's net " +
                            "is " + sr + ", coins were created or destroyed");
                    if (SessionState.GetInt(K_ACTIVE, 0) == 0)
                        return;
                }
                else if (won != null && paid != null)
                {
                    // Blackjack: the table is the house, so the net is not
                    // zero - but no seat may ever be paid more than 3x its
                    // stake (a natural returns 2.5x), and none may be paid
                    // for a stake it never made.
                    for (int i = 0; i < won.Length && i < paid.Length; i++)
                        if (won[i] < 0 || won[i] > paid[i] * 3)
                        {
                            Finish(1, "blackjack settlement " + s_settlements +
                                ": seat " + i + " staked " + paid[i] + " and was " +
                                "paid " + won[i]);
                            return;
                        }
                }
                Debug.Log("[Legaia] CARDS: settlement " + s_settlements + " (" +
                    (VarInt(s_game, "mode", 0) == 0 ? "poker" : "blackjack") +
                    ") pot " + pot + " paid " + sp + " won " + sw + " net " + sr);
            }

            // Half way through, hand the table to blackjack. Self-play
            // never presses the Mode button (that needs a seated player),
            // so without this the whole blackjack loop - deal, hit/stand,
            // the dealer drawing to 17, the 3:2 settlement - would only
            // ever be covered by the unit cases.
            if (!s_switched && t >= s_seconds * 0.5f &&
                VarInt(s_game, "phase", -1) == 0)
            {
                s_switched = true;
                s_handsAtSwitch = VarInt(s_game, "handsCompleted", 0);
                SetVar(s_game, "mode", 1);
                Debug.Log("[Legaia] CARDS: table switched to blackjack at t=" +
                    t.ToString("0") + " after " + s_handsAtSwitch + " poker hand(s).");
            }

            if (t >= s_nextLine)
            {
                s_nextLine = t + 10f;
                Debug.Log("[Legaia] CARDS: t=" + t.ToString("0") +
                    " phase=" + VarInt(s_game, "phase", -1) +
                    " turn=" + VarInt(s_game, "turnSeat", -9) +
                    " pot=" + VarInt(s_game, "pot", -1) +
                    " hands=" + VarInt(s_game, "handsCompleted", -1) +
                    " seats=" + Join(kind));
            }

            if (t < s_seconds)
                return;
            Report();
        }

        static void Report()
        {
            int hands = VarInt(s_game, "handsCompleted", 0);
            int stools = 0;
            for (int i = 0; i < s_sawNpc.Length; i++)
                if (s_sawNpc[i])
                    stools++;
            Debug.Log("[Legaia] CARDS: hands=" + hands + " settlements=" +
                s_settlements + " stools that saw a villager=" + stools + "/" +
                s_sawNpc.Length + " kit exceptions=" + s_exceptions);
            foreach (string p in s_problems)
                Debug.LogError("[Legaia] CARDS: exception - " + p);

            if (s_exceptions > 0)
            {
                Finish(1, s_exceptions + " exception(s) out of the kit during the soak");
                return;
            }
            if (hands < 1)
            {
                Finish(1, "no hand completed in " + s_seconds + " simulated s " +
                    "(villagers never filled two stools, or the dealer stalled)");
                return;
            }
            if (stools < s_sawNpc.Length)
            {
                Finish(1, "only " + stools + " of " + s_sawNpc.Length +
                    " stools ever had a villager on it");
                return;
            }
            if (!s_switched)
            {
                Finish(1, "the table never reached an idle moment to switch " +
                    "to blackjack - the blackjack loop went unexercised");
                return;
            }
            if (hands - s_handsAtSwitch < 1)
            {
                Finish(1, "no blackjack hand completed after the switch (" +
                    hands + " total, " + s_handsAtSwitch + " before it)");
                return;
            }
            if (s_seatedSamples == 0)
            {
                Finish(1, "no villager was ever sampled sitting on a stool");
                return;
            }
            foreach (string line in s_talkSeen)
            {
                bool named = line.IndexOf(": ") > 0 && line.IndexOf(": ") <= 22;
                bool direction = !line.Contains(":");   // Vahn's stage directions
                if (!named && !direction)
                {
                    Finish(1, "table talk line without a name in front: '" + line + "'");
                    return;
                }
            }
            if (s_talkSeen.Count < 4)
            {
                Finish(1, "only " + s_talkSeen.Count + " distinct table-talk line(s) " +
                    "seen over " + hands + " hand(s)");
                return;
            }
            Debug.Log("[Legaia] CARDS: " + s_handsAtSwitch + " poker hand(s) + " +
                (hands - s_handsAtSwitch) + " blackjack hand(s); seated villagers " +
                "sampled " + s_seatedSamples + "x at " + s_minSeatY.ToString("0.00") +
                ".." + s_maxSeatY.ToString("0.00") + " m above the stool floor; " +
                s_talkSeen.Count + " distinct table-talk lines.");
            Finish(0, null);
        }

        static MethodInfo s_setVar;

        static void SetVar(Component udon, string name, object value)
        {
            if (udon == null)
                return;
            if (s_setVar == null)
                s_setVar = Method(udon.GetType(), "SetProgramVariable", 2);
            s_setVar?.Invoke(udon, new object[] { name, value });
        }

        static int Sum(int[] a)
        {
            if (a == null)
                return 0;
            int n = 0;
            for (int i = 0; i < a.Length; i++)
                n += a[i];
            return n;
        }

        static string Join(int[] a)
        {
            if (a == null)
                return "-";
            string s = "";
            for (int i = 0; i < a.Length; i++)
                s += a[i];
            return s;
        }

        static void Finish(int code, string why)
        {
            if (code == 0)
                Debug.Log("[Legaia] SELFTEST OK: card-game soak complete.");
            else
                Debug.LogError("[Legaia] CARDS FAIL: " + why);
            SessionState.SetInt(K_ACTIVE, 0);
            EditorApplication.update -= Drive;
            Application.logMessageReceived -= OnLog;
            if (Application.isBatchMode)
                EditorApplication.Exit(code);
            else
                EditorApplication.isPlaying = false;
        }
    }
}
