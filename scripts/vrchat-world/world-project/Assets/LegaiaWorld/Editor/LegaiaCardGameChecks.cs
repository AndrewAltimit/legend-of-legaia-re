// Headless checks for the card table's game.
//
//   Run  (edit mode, -quit is fine)
//     Unity.exe -batchmode -nographics -quit -projectPath <copy>
//         -executeMethod LegaiaWorld.LegaiaCardGameChecks.Run
//         [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
//   Builds the common prefabs on the scene's built root, asserts the
//   `game` child stands under card_table with every field wired on its
//   BACKING behaviour (the U# proxy is not what runs in-world) and the
//   five community anchors stand clear across the felt, renders a
//   portrait for every villager in the scene and asserts each brain got
//   one, and then drives the poker evaluator, the best-of-seven wrapper,
//   the day/night rules switch and the blackjack settlement as plain C#
//   on a throwaway GameObject - the U# proxy is an ordinary MonoBehaviour
//   in the editor, so its rules are directly callable with no Udon
//   runtime and no networking (the same trick LegaiaSlotTools uses for
//   the slot machine's parity fixture). An off-by-one in a kicker or a
//   3:2 rounding slip compiles fine and would otherwise only show up as
//   somebody quietly losing coins.
//
//   Soak  (play mode - NO -quit; it exits itself)
//     Unity.exe -batchmode -nographics -projectPath <copy>
//         -executeMethod LegaiaWorld.LegaiaCardGameChecks.Soak
//         [-legaiaCardSeconds 150] [-legaiaCardScale 2]
//         [-legaiaCardNight 1]
//         [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
//   Enters play mode with nobody at the table and lets the villagers play
//   themselves (there is no local player at all in a headless editor -
//   Networking.LocalPlayer is null - which the game reads as "I am the
//   only simulation" and runs the dealer side). It samples the game's
//   BACKING behaviour every frame and asserts: hands actually complete,
//   every stool was sat in at some point, the pot is exactly what the
//   seats paid in and exactly what the winners took out, no villager ever
//   goes chip-negative, a folded seat holds no card at all, and every
//   seat that reached a contested showdown has its cards face up on the
//   FELT (read a beat later, so an animated flip has landed). Exits 0
//   pass / 1 assertion / 3 play mode never started / 4 watchdog, like
//   LegaiaSoak.
//
//   `-legaiaCardNight 1` runs the same soak under Cara's night rules and
//   asserts the mirror image: no community card is ever turned over, and
//   the table stays on poker (the blackjack half-way switch is a daytime
//   thing). It forces the GAME's `rulesOverride`, not the clock - see
//   Init for why moving the clock to night would empty the stools and
//   turn this into a test of the hour.

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
            CheckArray(proxy, backing, "rowTalk", 4);
            // The board: five spots across the middle of the felt.
            CheckArray(proxy, backing, "communityAnchors", 5);
            foreach (string b in new[] { "btnDeal", "btnMode", "btnCall", "btnRaise",
                                         "btnFold", "btnHit", "btnStand",
                                         "btnShuffle", "btnGather" })
                CheckRef(proxy, backing, b);
            foreach (string t in new[] { "modeText", "potText", "msgText",
                                         "communityText", "handText",
                                         "btnCallText", "btnRaiseText", "btnDealText",
                                         "btnNpcsText", "talk" })
                CheckRef(proxy, backing, t);

            // The panel's clicks must land on the BACKING behaviour: a
            // persistent listener onto the U# proxy does nothing in-world.
            // And they do NOT all land on the same one - the bottom row of
            // table controls (Shuffle / Gather / the villager toggle) moved
            // off the felt onto this canvas and still belongs to the deck
            // and the host, so each listener is matched by target AND by
            // the event string it carries.
            var deckType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardDeck");
            var hostType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaCardTableHost");
            if (deckType == null || hostType == null)
                Fail("LegaiaCardDeck / LegaiaCardTableHost are not compiled");
            var deckProxy = table.GetComponentInChildren(deckType, true);
            var hostProxy = table.GetComponentInChildren(hostType, true);
            if (deckProxy == null || hostProxy == null)
                Fail("the card table carries no LegaiaCardDeck / LegaiaCardTableHost");
            var deckBacking = LegaiaCommonPrefabs.BackingUdon(deckProxy);
            var hostBacking = LegaiaCommonPrefabs.BackingUdon(hostProxy);
            if (deckBacking == null || hostBacking == null)
                Fail("the deck / host has no backing UdonBehaviour");

            int wired = 0, deckWired = 0, hostWired = 0;
            var events = new List<string>();
            foreach (var btn in game.transform.parent
                         .GetComponentsInChildren<UnityEngine.UI.Button>(true))
            {
                int n = btn.onClick.GetPersistentEventCount();
                for (int i = 0; i < n; i++)
                {
                    var target = btn.onClick.GetPersistentTarget(i);
                    if (target == null)
                        Fail(btn.name + ": persistent listener with no target");
                    if (target == (Object)proxy || target == (Object)deckProxy ||
                        target == (Object)hostProxy)
                        Fail(btn.name + ": listener points at the U# PROXY, not " +
                             "the backing UdonBehaviour - it would be dead in-world");
                    if (btn.onClick.GetPersistentMethodName(i) != "SendCustomEvent")
                        Fail(btn.name + ": listener calls " +
                             btn.onClick.GetPersistentMethodName(i) +
                             ", not SendCustomEvent");
                    string ev = ClickEvent(btn, i);
                    if (string.IsNullOrEmpty(ev))
                        Fail(btn.name + ": listener carries no event-name argument");
                    if (target == (Object)backing)
                    {
                        wired++;
                        events.Add(ev);
                    }
                    else if (target == (Object)deckBacking)
                    {
                        deckWired++;
                        if (ev != "Shuffle" && ev != "Gather")
                            Fail(btn.name + " sends " + ev + " to the deck, expected " +
                                 "Shuffle or Gather");
                    }
                    else if (target == (Object)hostBacking)
                    {
                        hostWired++;
                        if (ev != "ToggleNpcs")
                            Fail(btn.name + " sends " + ev + " to the table host, " +
                                 "expected ToggleNpcs");
                    }
                    else
                        Fail(btn.name + ": listener targets " + target.name +
                             ", none of the game / deck / host behaviours");
                }
            }
            // Deal / Mode / Check-Call / Bet-Raise / Fold / Hit / Stand. The
            // five hold buttons and Draw went with five-card draw.
            foreach (string ev in new[] { "UiDeal", "UiMode", "UiCall", "UiRaise",
                                          "UiFold", "UiHit", "UiStand" })
                if (!events.Contains(ev))
                    Fail("no panel button sends " + ev + " to the game");
            if (wired != 7)
                Fail(wired + " panel button(s) wired into the game behaviour, " +
                     "expected 7");
            if (deckWired != 2)
                Fail(deckWired + " panel button(s) wired into the deck, expected 2 " +
                     "(Shuffle, Gather)");
            if (hostWired != 1)
                Fail(hostWired + " panel button(s) wired into the table host, " +
                     "expected 1 (ToggleNpcs)");
            // ... and nothing may be left standing on the felt.
            var strayBtn = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaEventButton");
            if (strayBtn != null && table.GetComponentsInChildren(strayBtn, true).Length != 0)
                Fail("a collider LegaiaEventButton is still on the card table - the " +
                     "three table controls live on the panel now");

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
            CheckAnchors(table);
            CheckPoker();
            CheckHoldem();
            CheckRules();
            CheckBlackjack();
            Debug.Log("[Legaia] CARDS: seat panel wired (" + wired +
                " buttons to the game, " + deckWired + " to the deck, " + hostWired +
                " to the table host; no collider buttons left on the felt), " +
                "evaluator and settlement cases pass.");
            Debug.Log("[Legaia] SELFTEST OK: card game.");
        }

        /// The string argument of a Button's `i`th persistent onClick call -
        /// the event name SendCustomEvent will raise. UnityEvent exposes the
        /// target and the method name but not the argument, so it is read
        /// off the serialized call list.
        static string ClickEvent(UnityEngine.UI.Button btn, int i)
        {
            var so = new SerializedObject(btn);
            var calls = so.FindProperty("m_OnClick.m_PersistentCalls.m_Calls");
            if (calls == null || i >= calls.arraySize)
                return null;
            return calls.GetArrayElementAtIndex(i)
                .FindPropertyRelative("m_Arguments.m_StringArgument").stringValue;
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
                string cara = talk.Compose(k, "Cara", "", k * 11 + 5);
                if (!cara.StartsWith("Cara: ") || cara.Length < 14)
                    Fail("talk: Cara kind " + k + " composed '" + cara + "'");
            }
            // Her voice is HERS: no line of Cara's is a line any other
            // villager can say (a missing pool would silently fall back).
            for (int k = 0; k < LegaiaTableTalk.KIND_COUNT; k++)
            {
                var villagerLines = new HashSet<string>();
                for (int salt = 0; salt < 48; salt++)
                    villagerLines.Add(talk.Compose(k, "Bram", "", salt).Substring(6));
                for (int salt = 0; salt < 48; salt++)
                {
                    string line = talk.Compose(k, "Cara", "", salt).Substring(6);
                    if (villagerLines.Contains(line))
                        Fail("talk: Cara kind " + k + " fell back to the villager " +
                             "pool ('" + line + "')");
                }
            }
            // Cara's night opener has to be the rules on her poster.
            bool caraRules = false;
            for (int salt = 0; salt < 32; salt++)
            {
                string line = talk.Compose(LegaiaTableTalk.T_NIGHT, "Cara", "", salt);
                string low = line.ToLower();
                if (low.Contains("five") && (low.Contains("best hand") ||
                        low.Contains("cheat")))
                    caraRules = true;
            }
            if (!caraRules)
                Fail("talk: Cara never opens her poker night with the rules " +
                     "(five cards / best hand / no cheating)");
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
                LegaiaTableTalk.KIND_COUNT + " kinds x 4 voices, longest line " +
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

        /// The board stands in the middle of the felt: five spots in a row,
        /// clear of the blackjack dealer's row and of every seat's fan, and
        /// all at the felt height the cards are dealt onto.
        static void CheckAnchors(Transform table)
        {
            var seen = new List<Transform>();
            for (int i = 0; i < 5; i++)
            {
                var a = table.Find("community_anchor_" + i);
                if (a == null)
                    Fail("no community_anchor_" + i + " under card_table");
                seen.Add(a);
            }
            var dealer = table.Find("dealer_anchor");
            if (dealer == null)
                Fail("no dealer_anchor under card_table");
            for (int i = 0; i < seen.Count; i++)
            {
                if (Mathf.Abs(seen[i].localPosition.y - dealer.localPosition.y) > 1e-3f)
                    Fail("community_anchor_" + i + " is not at the felt height " +
                         "(y " + seen[i].localPosition.y + " vs dealer " +
                         dealer.localPosition.y + ")");
                Vector3 d = seen[i].position - dealer.position;
                d.y = 0f;
                if (d.magnitude < 0.11f)
                    Fail("community_anchor_" + i + " sits " + d.magnitude.ToString("0.00") +
                         " m from the blackjack dealer row - the two would " +
                         "share a card's footprint");
                for (int k = 0; k < 4; k++)
                {
                    var hand = table.Find("hand_anchor_" + k);
                    if (hand == null)
                        continue;
                    Vector3 h = seen[i].position - hand.position;
                    h.y = 0f;
                    if (h.magnitude < 0.2f)
                        Fail("community_anchor_" + i + " is " + h.magnitude.ToString("0.00") +
                             " m from hand_anchor_" + k);
                }
                if (i > 0)
                {
                    float gap = (seen[i].position - seen[i - 1].position).magnitude;
                    if (gap < 0.06f || gap > 0.2f)
                        Fail("community anchors " + (i - 1) + " and " + i +
                             " are " + gap.ToString("0.000") + " m apart");
                }
            }
            Debug.Log("[Legaia] CARDS: 5 community anchors in a row across the " +
                "felt, " + (seen[4].position - seen[0].position).magnitude
                    .ToString("0.00") + " m end to end.");
        }

        /// Hold'em: the best five out of seven. `Eval5` stays the exact
        /// evaluator (CheckPoker pins it card for card) and `BestOfSeven`
        /// is only allowed to pick the best combination out of what it is
        /// handed - never to invent a better score than one of them.
        static void CheckHoldem()
        {
            var go = new GameObject("~legaia-card-holdem");
            go.hideFlags = HideFlags.HideAndDontSave;
            int cases = 0;
            try
            {
                var g = go.AddComponent<LegaiaCardGame>();
                g.suppressSerialization = true;

                // Two hole cards + a board that makes the nut flush only
                // when one hole card is used.
                int flush = g.BestOfSeven(Card(A, H), Card(R4, S),
                    Card(K, H), Card(R9, H), Card(R2, H), Card(R7, H), Card(Q, S));
                if (flush / 759375 != 5)
                    Fail("A-high heart flush out of seven scored as category " +
                         flush / 759375);
                cases++;
                if (flush != g.Eval5(Card(A, H), Card(K, H), Card(R9, H),
                        Card(R7, H), Card(R2, H)))
                    Fail("the best of seven is not the five-card score of the " +
                         "five it should have picked");
                cases++;

                // A full house beats the flush hiding in the same seven.
                int full = g.BestOfSeven(Card(K, S), Card(K, D),
                    Card(K, H), Card(R9, H), Card(R2, H), Card(R7, H), Card(R9, S));
                if (full / 759375 != 6)
                    Fail("KKK99 out of seven scored as category " + full / 759375);
                cases += Order("full house > the flush in the same seven", full, flush);

                // Playing the board: two rags with a straight on the felt
                // must score exactly the board's own hand.
                int board = g.Eval5(Card(T, S), Card(J, H), Card(Q, D), Card(K, C),
                    Card(A, S));
                int rags = g.BestOfSeven(Card(R2, H), Card(R3, D),
                    Card(T, S), Card(J, H), Card(Q, D), Card(K, C), Card(A, S));
                if (rags != board)
                    Fail("two rags on a broadway board did not score the board " +
                         "itself (" + rags + " vs " + board + ")");
                cases++;

                // Fewer than five cards is no hand at all - the pre-flop
                // state every seat is in before the flop turns over.
                if (g.BestOfSeven(Card(A, S), Card(K, S), -1, -1, -1, -1, -1) != -1)
                    Fail("two hole cards and no board scored as a hand");
                cases++;
                // Exactly five must agree with Eval5 to the digit.
                if (g.BestOfSeven(Card(A, S), Card(K, S), Card(Q, S), Card(J, S),
                        Card(T, S), -1, -1) !=
                    g.Eval5(Card(A, S), Card(K, S), Card(Q, S), Card(J, S), Card(T, S)))
                    Fail("BestOfSeven over exactly five disagrees with Eval5");
                cases++;
                // Six cards: the sixth may only ever help.
                int six = g.BestOfSeven(Card(A, S), Card(A, H), Card(A, D),
                    Card(R7, C), Card(R2, S), Card(A, C), -1);
                if (six / 759375 != 7)
                    Fail("four aces out of six scored as category " + six / 759375);
                cases++;

                // Pre-flop strength: the shape every starting-hand chart has.
                int aces = g.PreflopStrength(Card(A, S), Card(A, H));
                int deuces = g.PreflopStrength(Card(R2, S), Card(R2, H));
                int akSuited = g.PreflopStrength(Card(A, S), Card(K, S));
                int akOff = g.PreflopStrength(Card(A, S), Card(K, H));
                int ragsPre = g.PreflopStrength(Card(R7, S), Card(R2, H));
                cases += Order("a pair of aces > a pair of deuces", aces, deuces);
                cases += Order("a pair of aces > ace-king suited", aces, akSuited);
                cases += Order("ace-king suited > ace-king offsuit", akSuited, akOff);
                cases += Order("ace-king offsuit > seven-deuce", akOff, ragsPre);
                // A pair, even the smallest, beats two unpaired rags.
                cases += Order("a pair of deuces > seven-deuce", deuces, ragsPre);
                int qjOff = g.PreflopStrength(Card(Q, S), Card(J, H));
                cases += Order("a pair of deuces > queen-jack offsuit", deuces, qjOff);
                if (aces > 99 || ragsPre < 0)
                    Fail("pre-flop strength left the 0..99 band (" + aces + " / " +
                         ragsPre + ")");
                cases++;
            }
            finally
            {
                Object.DestroyImmediate(go);
            }
            Debug.Log("[Legaia] CARDS: " + cases + " hold'em best-of-seven case(s) pass.");
        }

        /// The day/night switch: hold'em by day, Cara's five-card night
        /// game after dusk, decided by the town clock and never by a
        /// button. A null director (no living town) is day.
        static void CheckRules()
        {
            var go = new GameObject("~legaia-card-rules-clock");
            go.hideFlags = HideFlags.HideAndDontSave;
            var clockGo = new GameObject("~legaia-card-clock");
            clockGo.hideFlags = HideFlags.HideAndDontSave;
            int cases = 0;
            try
            {
                var g = go.AddComponent<LegaiaCardGame>();
                g.suppressSerialization = true;
                g.PollRules();
                if (g.rules != 0)
                    Fail("with no town director the table is not on hold'em (rules " +
                         g.rules + ")");
                cases++;

                var director = clockGo.AddComponent<LegaiaTownDirector>();
                var clock = clockGo.AddComponent<LegaiaDayNight>();
                director.dayNight = clock;
                g.director = director;
                clock.isNight = false;
                g.PollRules();
                if (g.rules != 0)
                    Fail("by day the table is not on hold'em (rules " + g.rules + ")");
                cases++;
                if (!g.RulesName().Contains("hold"))
                    Fail("the day mode line reads '" + g.RulesName() + "'");
                cases++;

                clock.isNight = true;
                g.PollRules();
                if (g.rules != 1)
                    Fail("after dusk the table did not become the night game (rules " +
                         g.rules + ")");
                cases++;
                if (!g.RulesName().Contains("Cara"))
                    Fail("the night mode line reads '" + g.RulesName() +
                         "' - it must name Cara's poker night");
                cases++;

                // The override the soaks drive, and the only way anything
                // but the clock decides.
                g.rulesOverride = 0;
                g.PollRules();
                if (g.rules != 0)
                    Fail("rulesOverride 0 did not force hold'em at night");
                cases++;
                g.rulesOverride = 1;
                clock.isNight = false;
                g.PollRules();
                if (g.rules != 1)
                    Fail("rulesOverride 1 did not force the night game by day");
                cases++;
            }
            finally
            {
                Object.DestroyImmediate(go);
                Object.DestroyImmediate(clockGo);
            }
            Debug.Log("[Legaia] CARDS: " + cases + " day/night rules case(s) pass.");
        }

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
        const string K_NIGHT = "legaia.cards.night";

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
            int night = Arg("-legaiaCardNight", "0") == "1" ? 1 : 0;
            SessionState.SetFloat(K_SECONDS, Mathf.Max(20f, seconds));
            SessionState.SetFloat(K_SCALE, scale);
            SessionState.SetInt(K_NIGHT, night);
            SessionState.SetInt(K_ACTIVE, 1);
            Debug.Log("[Legaia] CARDS: entering play mode - " + seconds +
                " simulated s at timeScale " + scale + ", villagers only, " +
                (night == 1 ? "Cara's night rules forced." : "hold'em by day."));
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
        // The built sitting pose, read on the live rig: the knee's
        // direction from the hip against the villager's rendered face
        // (+Z of the instance through its full matrix), once the pose has
        // had a second to blend in. Legged rigs only.
        static float[] s_seatedSince;
        static int s_kneeSamples;
        static float s_minKneeDot = 2f;
        static System.Type s_wanderType;
        static Transform[] s_prevNpc;
        static Component[] s_prevBrain;
        static float[] s_prevDist;
        static int s_giveUps;
        static readonly Dictionary<string, int> s_giveUpBy = new Dictionary<string, int>();
        static float s_prevT;
        static int s_walkedIn;
        static readonly HashSet<string> s_talkSeen = new HashSet<string>();
        static bool[] s_sawNpc;
        static int s_lastSettle = -1;
        static int s_settlements;
        static bool s_switched;
        static int s_handsAtSwitch;
        static int s_exceptions;
        static bool s_night;
        static int s_maxCommunity;
        static int s_showdownsSeen;
        static float s_verifyAt;
        static int s_verifySeats;
        static int s_facesVerified;
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
            s_prevNpc = new Transform[4];
            s_prevBrain = new Component[4];
            s_prevDist = new float[4];
            s_seatedSince = new float[4];
            s_kneeSamples = 0;
            s_minKneeDot = 2f;
            s_geomSeen.Clear();
            s_wanderType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcWander");
            s_giveUps = 0;
            s_giveUpBy.Clear();
            s_prevT = Time.time;
            s_walkedIn = 0;
            s_talkSeen.Clear();

            // The CLOCK stays at day even for the night-rules soak: at
            // night the villagers go home through their doors and there is
            // nobody outdoors to fill a stool, which would make this a test
            // of the hour and not of the game. What the night soak forces
            // instead is the game's own `rulesOverride` - the same switch
            // PollRules honours, with the clock path covered statically by
            // CheckRules.
            ForceDay();
            s_night = SessionState.GetInt(K_NIGHT, 0) == 1;
            SetVar(s_game, "rulesOverride", s_night ? 1 : 0);
            s_maxCommunity = 0;
            s_showdownsSeen = 0;
            s_verifyAt = 0f;
            s_verifySeats = 0;
            s_facesVerified = 0;
            s_faceReadable = true;
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
            float dt = Time.time - s_prevT;
            s_prevT = Time.time;
            for (int i = 0; i < 4; i++)
            {
                if (s_stools[i] == null || s_stations[i] == null)
                    continue;
                var npc = Var(s_stations[i], "currentNpc") as Transform;
                // A claim that ends before the villager got within arm's
                // reach of the stool is a give-up: log the brain's own
                // reason, and fail on the loop shape (the same villager
                // giving up on the same stool again and again - what
                // nearest-first summoning did to Vahn beside the table).
                if (s_prevNpc[i] != null && npc != s_prevNpc[i] && s_prevDist[i] > 0.35f)
                {
                    string loop = NoteGiveUp(i, s_prevNpc[i], s_prevBrain[i], s_prevDist[i]);
                    if (loop != null)
                    {
                        Finish(1, loop);
                        return;
                    }
                }
                if (npc == null)
                {
                    s_prevNpc[i] = null;
                    s_prevBrain[i] = null;
                    continue;
                }
                if (s_prevNpc[i] != npc)
                    s_prevBrain[i] = Var(s_stations[i], "currentBrain") as Component;
                Vector3 d = npc.position - s_stools[i].position;
                float lift = d.y;
                d.y = 0f;
                // The villager WALKS in: between two samples it may close
                // on the stool no faster than a hop (3 m/s) plus slack. A
                // claimed villager that appears on the stool from metres
                // away was teleported by the host (the first cut sat it
                // the moment the station named it, before the walk).
                if (s_prevNpc[i] == npc)
                {
                    float closed = s_prevDist[i] - d.magnitude;
                    if (closed > 3f * Mathf.Max(dt, 0.02f) + 0.4f)
                    {
                        Finish(1, npc.name + " jumped " + closed.ToString("0.0") +
                            " m toward stool_" + i + " in " + dt.ToString("0.00") +
                            " s - teleported, not walked");
                        return;
                    }
                    if (s_prevDist[i] > 0.35f && d.magnitude <= 0.35f)
                        s_walkedIn++;   // watched one arrive on foot
                }
                s_prevNpc[i] = npc;
                s_prevDist[i] = d.magnitude;
                if (d.magnitude > 0.35f)
                    continue;   // still walking up
                s_seatedSamples++;
                // The built pose is judged only once the HOST has seated
                // the rig (a villager within reach of the stool may still
                // be walking the last step) and the blend has had a second.
                if (!Seated(npc))
                    s_seatedSince[i] = Time.time;
                else if (Time.time - s_seatedSince[i] > 1f)
                {
                    string knee = KneeAhead(npc);
                    if (knee == null)
                        knee = SeatGeometry(npc, s_stools[i], i);
                    if (knee != null)
                    {
                        Finish(1, npc.name + " on stool_" + i + ": " + knee);
                        return;
                    }
                }
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

            // What the table says, now one line PER SEAT: names in front,
            // never the retail line.
            var said = Var(s_game, "seatTalk") as string[];
            if (said != null)
                for (int i = 0; i < said.Length; i++)
                    if (!string.IsNullOrEmpty(said[i]) && s_talkSeen.Add(said[i]))
                        Debug.Log("[Legaia] CARDS: talk (seat " + i + ") - " + said[i]);

            // The board: how far the community reveal ever got. Hold'em
            // must reach all five by a contested showdown; the night game
            // has no board at all and must never turn one card over.
            int up = VarInt(s_game, "communityUp", 0);
            if (up > s_maxCommunity)
                s_maxCommunity = up;
            if (s_night && up != 0)
            {
                Finish(1, "the night game turned " + up + " community card(s) " +
                    "over - Cara's rules have no board");
                return;
            }

            // A folded seat's cards leave the felt AT ONCE - both of them.
            var states = VarInts(s_game, "seatState");
            var held = VarInts(s_game, "seatCards");
            if (states != null && held != null)
                for (int i = 0; i < states.Length; i++)
                {
                    if (states[i] != 2)   // H_FOLD
                        continue;
                    for (int c = 0; c < 5 && i * 5 + c < held.Length; c++)
                        if (held[i * 5 + c] >= 0)
                        {
                            Finish(1, "seat " + i + " folded but still holds card " +
                                held[i * 5 + c] + " in slot " + c);
                            return;
                        }
                }

            // Every seat that reached a contested showdown has its cards
            // face up on the FELT, not just in the panel. Checked a beat
            // after the settlement so an animated flip has landed.
            if (s_verifyAt > 0f && Time.time >= s_verifyAt)
            {
                string bad = VerifyFaces(s_verifySeats);
                s_verifyAt = 0f;
                if (bad != null)
                {
                    Finish(1, bad);
                    return;
                }
            }

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
                // Arm the face-up verification for the seats that were
                // still in the hand: a contested poker showdown shows
                // every one of them, an uncontested pot shows nobody.
                s_verifySeats = 0;
                int inHand = 0;
                var st = VarInts(s_game, "seatState");
                if (st != null)
                    for (int i = 0; i < st.Length; i++)
                        if (st[i] == 1)   // H_IN
                        {
                            s_verifySeats |= 1 << i;
                            inHand++;
                        }
                if (VarInt(s_game, "mode", 0) == 0 && inHand > 1)
                {
                    s_showdownsSeen++;
                    s_verifyAt = Time.time + 1.2f;
                }
                else
                {
                    s_verifySeats = 0;
                }
                Debug.Log("[Legaia] CARDS: settlement " + s_settlements + " (" +
                    (VarInt(s_game, "mode", 0) == 0 ? "poker" : "blackjack") +
                    ") pot " + pot + " paid " + sp + " won " + sw + " net " + sr +
                    " board " + VarInt(s_game, "communityUp", 0) + " contenders " +
                    inHand);
            }

            // Half way through, hand the table to blackjack. Self-play
            // never presses the Mode button (that needs a seated player),
            // so without this the whole blackjack loop - deal, hit/stand,
            // the dealer drawing to 17, the 3:2 settlement - would only
            // ever be covered by the unit cases. The night soak stays on
            // poker: what it is here to watch is Cara's five-card game.
            if (!s_night && !s_switched && t >= s_seconds * 0.5f &&
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
                    " rules=" + VarInt(s_game, "rules", -1) +
                    " street=" + VarInt(s_game, "street", -1) +
                    " board=" + up +
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
            if (!s_night)
            {
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
                // Hold'em ran its whole board at least once: three, one and
                // one, with a betting round between each.
                if (s_maxCommunity != 5)
                {
                    Finish(1, "the community reveal never reached five cards " +
                        "(deepest board " + s_maxCommunity + ") - the flop / " +
                        "turn / river staging did not complete");
                    return;
                }
            }
            else if (s_maxCommunity != 0)
            {
                Finish(1, "the night game dealt a board of " + s_maxCommunity +
                    " - Cara's rules are five cards and one round");
                return;
            }
            if (s_faceReadable && s_showdownsSeen > 0 && s_facesVerified == 0)
            {
                Finish(1, "no contested showdown was ever sampled with its cards " +
                    "face up on the felt (" + s_showdownsSeen + " showdown(s))");
                return;
            }
            if (s_seatedSamples == 0)
            {
                Finish(1, "no villager was ever sampled sitting on a stool");
                return;
            }
            if (s_walkedIn == 0)
            {
                Finish(1, "no villager was ever watched walking onto a stool - " +
                    "they all appeared there");
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
            Debug.Log("[Legaia] CARDS: " +
                (s_night ? hands + " night-rules poker hand(s)"
                         : s_handsAtSwitch + " poker hand(s) + " +
                           (hands - s_handsAtSwitch) + " blackjack hand(s)") +
                "; seated villagers " +
                "sampled " + s_seatedSamples + "x at " + s_minSeatY.ToString("0.00") +
                ".." + s_maxSeatY.ToString("0.00") + " m above the stool floor, " +
                s_walkedIn + " arrival(s) on foot, " + s_giveUps + " give-up(s); knees ahead on " +
                s_kneeSamples + " seated sample(s) (worst dot " +
                (s_kneeSamples > 0 ? s_minKneeDot.ToString("0.00") : "n/a") + "); " +
                s_talkSeen.Count + " distinct table-talk lines; " +
                (s_night ? "night rules, no board" : "deepest board " +
                    s_maxCommunity) + ", " + s_showdownsSeen +
                " contested showdown(s), " + s_facesVerified +
                " verified face up on the felt.");
            Finish(0, null);
        }

        /// Every seat in `mask` must have its dealt cards face up on the
        /// actual card pickups - the panel agreeing is not the same thing.
        /// Null when they all do.
        static string VerifyFaces(int mask)
        {
            if (mask == 0)
                return null;
            var cards = Var(s_game, "cards") as System.Array;
            var held = VarInts(s_game, "seatCards");
            if (cards == null || held == null)
                return null;
            for (int i = 0; i < 4; i++)
            {
                if ((mask & (1 << i)) == 0)
                    continue;
                for (int c = 0; c < 5 && i * 5 + c < held.Length; c++)
                {
                    int idx = held[i * 5 + c];
                    if (idx < 0 || idx >= cards.Length)
                        continue;
                    bool up;
                    if (!CardFaceUp(cards.GetValue(idx), out up))
                    {
                        if (s_faceReadable)
                            Debug.LogWarning("[Legaia] CARDS: cannot read a card's " +
                                "faceUp off the live heap - the showdown face check " +
                                "is not exercised.");
                        s_faceReadable = false;
                        return null;
                    }
                    if (!up)
                        return "seat " + i + " reached a contested showdown with " +
                               "card " + idx + " still face down on the felt";
                    s_facesVerified++;
                }
            }
            return null;
        }

        static bool s_faceReadable = true;

        /// `faceUp` off one card's LIVE heap. The array may hold either the
        /// backing UdonBehaviours or the U# proxies depending on how the
        /// field was serialized, and a proxy's own C# field is a stale
        /// shell in play mode - so the read always goes through
        /// GetProgramVariable, on the backing behaviour when the entry is
        /// a proxy.
        static bool CardFaceUp(object entry, out bool up)
        {
            up = false;
            var comp = entry as Component;
            if (comp == null)
                return false;
            Component udon = comp;
            if (Method(comp.GetType(), "GetProgramVariable", 1) == null)
                udon = LegaiaCommonPrefabs.BackingUdon(comp);
            var m = udon != null ? Method(udon.GetType(), "GetProgramVariable", 1) : null;
            if (m == null)
                return false;
            try
            {
                object v = m.Invoke(udon, new object[] { "faceUp" });
                if (!(v is bool))
                    return false;
                up = (bool)v;
                return true;
            }
            catch
            {
                return false;
            }
        }

        static Component Wander(Transform npc)
        {
            if (s_wanderType == null)
                return null;
            var proxy = npc.GetComponent(s_wanderType) as Component;
            return LegaiaCommonPrefabs.BackingUdon(proxy);
        }

        static bool Seated(Transform npc)
        {
            return Var(Wander(npc), "seated") is bool b && b;
        }

        // The seated villager's knee against its face, on the live rig:
        // null when it sits right (or has no leg pair), else why not.
        static string KneeAhead(Transform npc)
        {
            var w = Wander(npc);
            if (w == null)
                return null;
            var up = Var(w, "legUpper") as Transform[];
            var lo = Var(w, "legLower") as Transform[];
            if (up == null || lo == null || up.Length == 0 || lo.Length == 0 ||
                up[0] == null || lo[0] == null)
                return null;
            Vector3 face = npc.TransformPoint(Vector3.forward) - npc.position;
            face.y = 0f;
            Vector3 knee = lo[0].position - up[0].position;
            float drop = -knee.y;
            knee.y = 0f;
            if (face.sqrMagnitude < 1e-6f || knee.sqrMagnitude < 1e-6f)
                return null;
            float dot = Vector3.Dot(knee.normalized, face.normalized);
            s_kneeSamples++;
            s_minKneeDot = Mathf.Min(s_minKneeDot, dot);
            string why = null;
            if (dot < 0.5f)
                why = "seated with the knee " + (dot < -0.5f ? "BEHIND" : "beside") +
                      " the hip (dot " + dot.ToString("0.00") + ")";
            else if (drop > 0.12f)
                why = "seated with the thigh hanging " + drop.ToString("0.00") +
                      " m below the hip - not turned forward";
            if (why == null)
                return null;
            return why + " [hip " + up[0].name + " at " + up[0].position.ToString("0.00") +
                   " local rot " + up[0].localRotation.eulerAngles.ToString("0") +
                   ", knee " + lo[0].name + " at " + lo[0].position.ToString("0.00") +
                   ", thighTurn " + Var(w, "thighTurn") + ", armTurn " + Var(w, "armTurn") +
                   ", sitWeight " + Var(w, "sitWeight") + ", seated " + Var(w, "seated") +
                   ", legs " + up.Length + "/" + lo.Length + "]";
        }

        static readonly HashSet<string> s_geomSeen = new HashSet<string>();

        // Where the seated rig actually is, once per villager and stool:
        // its origin, hip (thigh pivot), knee (shin pivot) and sole (the
        // lowest foot vertex now) above the stool's floor, against the
        // seat top the host aims the hip at. Null when it sits right.
        static string SeatGeometry(Transform npc, Transform stool, int i)
        {
            var w = Wander(npc);
            var up = Var(w, "legUpper") as Transform[];
            var lo = Var(w, "legLower") as Transform[];
            float floor = stool.position.y;
            float origin = npc.position.y - floor;
            float seat = s_host != null && Var(s_host, "seatHeight") is float sh ? sh : 0.475f;
            string key = npc.name + "|" + i;
            bool first = s_geomSeen.Add(key);
            if (up == null || lo == null || up.Length == 0 || lo.Length == 0 ||
                up[0] == null || lo[0] == null)
            {
                if (first)
                    Debug.Log("[Legaia] CARDS: seat geometry " + npc.name + " on stool_" + i +
                              " (no leg pair): origin " + origin.ToString("0.00") +
                              " m above the stool floor, seat top " + seat.ToString("0.00"));
                return null;
            }
            float hip = up[0].position.y - floor;
            float knee = lo[0].position.y - floor;
            float sole = float.MaxValue;
            var mf = lo[0].GetComponent<MeshFilter>();
            if (mf != null && mf.sharedMesh != null)
                foreach (Vector3 v in mf.sharedMesh.vertices)
                    sole = Mathf.Min(sole, lo[0].TransformPoint(v).y - floor);
            if (first)
                Debug.Log("[Legaia] CARDS: seat geometry " + npc.name + " on stool_" + i +
                          ": origin " + origin.ToString("0.00") + ", hip " + hip.ToString("0.00") +
                          ", knee " + knee.ToString("0.00") + ", sole " + sole.ToString("0.00") +
                          " m above the stool floor; seat top " + seat.ToString("0.00") +
                          ", hipHeight " + Var(w, "hipHeight"));
            if (Mathf.Abs(hip - seat) > 0.06f)
                return "seated with the hip " + hip.ToString("0.00") + " m above the stool floor (seat top " +
                       seat.ToString("0.00") + ")";
            if (sole < float.MaxValue && sole > seat - 0.05f)
                return "seated with the sole " + sole.ToString("0.00") + " m above the stool floor - the feet do not hang below the seat (" +
                       seat.ToString("0.00") + ")";
            return null;
        }

        static string NoteGiveUp(int stool, Transform npc, Component brain, float dist)
        {
            s_giveUps++;
            string why = brain != null ? Var(brain, "lastFailure") as string : null;
            string key = npc.name + "|" + stool;
            int n;
            s_giveUpBy.TryGetValue(key, out n);
            s_giveUpBy[key] = ++n;
            Debug.LogWarning("[Legaia] CARDS: " + npc.name + " gave up " +
                dist.ToString("0.0") + " m short of stool_" + stool + " (" + n +
                "x): " + (string.IsNullOrEmpty(why) ? "no reason recorded" : why));
            if (n >= 3)
                return "walk loop: " + npc.name + " gave up on stool_" + stool + " " +
                       n + " times - " +
                       (string.IsNullOrEmpty(why) ? "no reason recorded" : why);
            return null;
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
