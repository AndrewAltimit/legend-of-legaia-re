// Headless checks for the TV: the shared video config, the arbitration
// rule, and the wiring that carries both into the world.
//
//   Run  (edit mode, -quit is fine)
//     Unity.exe -batchmode -nographics -quit -projectPath <copy>
//         -executeMethod LegaiaWorld.LegaiaVideoChecks.Run
//         [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>
//
// Three things are worth a check here and none of them is visible by
// looking at the set.
//
// THE CONFIG. Settings/video.settings.json is hand-edited text that
// nothing else validates: a show with no URL, a playlist entry whose
// "url" key was spelled wrong, an NPC number that became a string. Each
// of those builds a TV that looks finished and plays nothing.
//
// THE RULE. "A villager may only take over the default playlist" is one
// line of logic guarding every social moment the TV has, and it is
// exactly the kind of line an edit inverts by accident. The U# proxy is
// an ordinary MonoBehaviour in the editor, so ShowRequestAllowed can be
// called directly against every source state - the same trick
// LegaiaCardGameChecks uses on the poker evaluator. `source` is private
// and only the network can normally move it, so the check sets it by
// reflection: the point is to test the rule, not the transitions that
// reach it.
//
// THE WIRING. Fields are asserted on the BACKING UdonBehaviour, not on
// the proxy - the proxy is what the builder writes and the backing is
// what runs in-world, and a missing CopyProxyToUdon leaves the first
// looking perfect while the second plays nothing. The flattened show
// windows (showStart / showCount over showUrls) are checked to cover the
// array exactly: an off-by-one there plays the wrong villager's video.
//
// Owners that match no NPC in the scene are a WARNING, not a failure -
// the config is shared across worlds by design, so a town without Tetsu
// is not a broken config, it is a town without Tetsu.

using System.Collections.Generic;
using System.Reflection;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaVideoChecks
    {
        static readonly BindingFlags ANY =
            BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;

        static int s_warnings;

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
            Debug.LogError("[Legaia] VIDEO FAIL: " + msg);
            if (Application.isBatchMode)
                EditorApplication.Exit(1);
            throw new System.Exception(msg);
        }

        static void Warn(string msg)
        {
            s_warnings++;
            Debug.LogWarning("[Legaia] video: " + msg);
        }

        public static void Run()
        {
            s_warnings = 0;
            var video = CheckConfig();
            var tv = BuildAndFindTv();
            CheckRule(tv, video);
            CheckFileDetection(tv);
            CheckMatching(tv, video);
            CheckWiring(tv, video);
            CheckProps(tv, video);
            Debug.Log("[Legaia] VIDEO OK: " + video.defaultPlaylist.Count +
                      " playlist item(s), " + video.shows.Count + " show(s) across " +
                      OwnerCount(video) + " villager(s), " + s_warnings + " warning(s).");
            if (Application.isBatchMode)
                EditorApplication.Exit(0);
        }

        static int OwnerCount(LegaiaVideoSettings video)
        {
            var seen = new HashSet<string>();
            foreach (var s in video.shows)
                seen.Add((s.who + "/" + s.token).ToLower());
            return seen.Count;
        }

        // --- The config -----------------------------------------------------

        static LegaiaVideoSettings CheckConfig()
        {
            var video = LegaiaVideoSettings.Load(SceneNameOf());
            if (!video.loaded)
                Fail("no " + LegaiaVideoSettings.PATH + " - the TV has no playlist");
            if (video.defaultPlaylist.Count == 0)
                Fail("the default playlist is empty; the TV would start dark");

            var seenUrl = new Dictionary<string, string>();
            for (int i = 0; i < video.defaultPlaylist.Count; i++)
            {
                var e = video.defaultPlaylist[i];
                CheckUrl(e.url, "playlist item " + (i + 1));
                if (string.IsNullOrEmpty(e.title))
                    Warn("playlist item " + (i + 1) + " has no title - the panel " +
                         "will show its number alone");
                if (seenUrl.ContainsKey(e.url))
                    Warn("playlist item " + (i + 1) + " repeats " + seenUrl[e.url]);
                else
                    seenUrl[e.url] = "playlist item " + (i + 1);
            }

            for (int i = 0; i < video.shows.Count; i++)
            {
                var s = video.shows[i];
                string where = "show '" + s.title + "'";
                if (s.urls.Count == 0)
                    Fail(where + " has no URL");
                foreach (string u in s.urls)
                    CheckUrl(u, where);
                if (string.IsNullOrEmpty(s.who) && string.IsNullOrEmpty(s.token))
                    Fail(where + " belongs to nobody");
                if (!string.IsNullOrEmpty(s.token) && !s.token.StartsWith("npc_"))
                    Fail(where + " has the NPC token '" + s.token +
                         "', which matches no NPC object name (they all start npc_)");
                if (string.IsNullOrEmpty(s.title))
                    Warn(where.Replace("show ''", "a show") + " for " + s.who +
                         " has no title");
                if (s.console && s.urls.Count == 1)
                    Warn("console show '" + s.title + "' is a single video - " +
                         "fine, but the console appears for that one video only");
            }
            Debug.Log("[Legaia] video config: " + video.defaultPlaylist.Count +
                      " playlist item(s), " + video.shows.Count + " show(s), " +
                      "console shows: " + ConsoleTitles(video));
            return video;
        }

        static string ConsoleTitles(LegaiaVideoSettings video)
        {
            var names = new List<string>();
            foreach (var s in video.shows)
                if (s.console)
                    names.Add(s.who + "'s " + s.title);
            return names.Count == 0 ? "(none)" : string.Join(", ", names.ToArray());
        }

        static void CheckUrl(string url, string where)
        {
            if (string.IsNullOrEmpty(url))
                Fail(where + " has an empty URL");
            if (!url.StartsWith("http://") && !url.StartsWith("https://"))
                Fail(where + " is not an http(s) URL: " + url);
            if (url.Contains(" "))
                Fail(where + " has a space in its URL: " + url);
        }

        // --- The scene ------------------------------------------------------

        static string s_sceneName;

        static string SceneNameOf()
        {
            return s_sceneName ?? "town01";
        }

        static Component BuildAndFindTv()
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
            s_sceneName = spawn.transform.parent != null &&
                          spawn.transform.parent.name.StartsWith("Legaia_")
                ? spawn.transform.parent.name.Substring("Legaia_".Length)
                : "selftest";

            var o = new LegaiaCommonPrefabOptions
            {
                mirror = false, tv = true, cardTable = false, seats = 0,
                sdkPens = false, slotMachine = false,
            };
            var settings = LegaiaSceneSettings.Load(s_sceneName);
            var container = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + s_sceneName, spawn.transform.position, o,
                settings.prefabTransforms, settings.slotMachine);
            if (container == null)
                Fail("no container built");
            var tvT = container.transform.Find("tv");
            if (tvT == null)
                Fail("no tv under " + container.name);
            var type = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaVideoTv");
            if (type == null)
                Fail("LegaiaVideoTv type not found");
            var tv = tvT.GetComponent(type);
            if (tv == null)
                Fail("the tv carries no LegaiaVideoTv");
            return tv;
        }

        // --- The rule -------------------------------------------------------

        /// Every (source state, show) pair, against the one line that
        /// decides them. The playlist is interruptible and nothing else is.
        static void CheckRule(Component tv, LegaiaVideoSettings video)
        {
            var f = tv.GetType().GetField("source", ANY);
            if (f == null)
                Fail("LegaiaVideoTv has no `source` field - the rule cannot be tested");
            var allowed = tv.GetType().GetMethod("ShowRequestAllowed");
            if (allowed == null)
                Fail("LegaiaVideoTv has no ShowRequestAllowed(int)");
            object before = f.GetValue(tv);

            bool Ask(int src, int show)
            {
                f.SetValue(tv, src);
                return (bool)allowed.Invoke(tv, new object[] { show });
            }

            int n = video.shows.Count;
            for (int i = 0; i < n; i++)
            {
                if (!Ask(0, i))
                    Fail("show " + i + " (" + video.shows[i].who +
                         ") refused while the default playlist plays - villagers " +
                         "could never put anything on");
                if (Ask(1, i))
                    Fail("show " + i + " accepted over ANOTHER villager's show");
                if (Ask(2, i))
                    Fail("show " + i + " accepted over a guest's video - a player's " +
                         "choice must never be interrupted");
            }
            // Out of range in every state.
            foreach (int src in new[] { 0, 1, 2 })
            {
                if (Ask(src, -1) || Ask(src, n) || Ask(src, n + 50))
                    Fail("an out-of-range show index was accepted in source " + src);
            }
            f.SetValue(tv, before);
            Debug.Log("[Legaia] video rule: " + n + " show(s) x 3 source states + " +
                      "3 out-of-range cases - the playlist is the only interruptible one");
        }

        /// The other half of "why is nothing playing": which URLs are worth
        /// handing to the Unity player at all. A page link is not - it goes
        /// to Windows Media Foundation as a byte stream and comes back
        /// 0xc00d36c4 - and getting this wrong is invisible except as a
        /// wall of errors in somebody's console.
        static void CheckFileDetection(Component tv)
        {
            var mi = tv.GetType().GetMethod("DirectFile", BindingFlags.Static |
                BindingFlags.NonPublic | BindingFlags.Public);
            if (mi == null)
                Fail("LegaiaVideoTv has no DirectFile(string) - the player " +
                     "fallback cannot be tested");
            bool Direct(string u)
            {
                return (bool)mi.Invoke(null, new object[] { u });
            }
            string[] files =
            {
                "https://example.com/clip.mp4",
                "https://example.com/clip.MP4?token=1",
                "https://example.com/a/b.webm",
                "https://example.com/live/index.m3u8",
            };
            string[] pages =
            {
                "https://youtu.be/EnkLIHM_Tzo",
                "https://www.youtube.com/watch?v=EnkLIHM_Tzo",
                "https://www.twitch.tv/someone",
                "https://vimeo.com/12345",
                "",
            };
            foreach (string u in files)
                if (!Direct(u))
                    Fail("'" + u + "' is a media file but was not recognised as one - " +
                         "the Unity player would never be tried for it");
            foreach (string u in pages)
                if (Direct(u))
                    Fail("'" + u + "' is a page, not a file - handing it to the Unity " +
                         "player only produces a Media Foundation error");
            Debug.Log("[Legaia] video fallback: " + files.Length + " file URL(s) and " +
                      pages.Length + " page URL(s) classified correctly");
        }

        // --- Owner matching --------------------------------------------------

        static void CheckMatching(Component tv, LegaiaVideoSettings video)
        {
            var showFor = tv.GetType().GetMethod("ShowFor");
            var belongs = tv.GetType().GetMethod("ShowBelongsTo");
            if (showFor == null || belongs == null)
                Fail("LegaiaVideoTv is missing ShowFor / ShowBelongsTo");

            int Pick(string name, string obj)
            {
                return (int)showFor.Invoke(tv, new object[] { name, obj });
            }
            bool Belongs(int show, string name, string obj)
            {
                return (bool)belongs.Invoke(tv, new object[] { show, name, obj });
            }

            // Nobody's villager owns nothing, however it is spelled.
            if (Pick("Nobody At All", "npc_77_stranger") != -1)
                Fail("a villager who owns no show was handed one");
            if (Pick("", "") != -1)
                Fail("an unnamed villager was handed a show");

            // Every configured owner resolves - by NAME and, where the
            // config gives one, by TOKEN alone (a scene that never pinned
            // the display name still gets its shows).
            var byOwner = new Dictionary<string, List<int>>();
            for (int i = 0; i < video.shows.Count; i++)
            {
                var s = video.shows[i];
                string key = s.who.ToLower();
                if (!byOwner.ContainsKey(key))
                    byOwner[key] = new List<int>();
                byOwner[key].Add(i);

                if (!string.IsNullOrEmpty(s.who) && Pick(s.who, "") < 0)
                    Fail(s.who + " matches none of their own shows by name");
                if (!string.IsNullOrEmpty(s.token) && Pick("", s.token + "_anything") < 0)
                    Fail("token " + s.token + " matches none of " + s.who + "'s shows");
                if (!string.IsNullOrEmpty(s.who) && !Belongs(i, s.who.ToUpper(), ""))
                    Fail("owner matching is case sensitive - " + s.who.ToUpper() +
                         " did not match " + s.who);
                // A token must match the NPC name as a PREFIX SEGMENT, never
                // as a substring: npc_1 must not answer for npc_14.
                if (!string.IsNullOrEmpty(s.token) && Belongs(i, "", s.token + "9_x"))
                    Fail("token " + s.token + " matched " + s.token + "9_x - " +
                         "the prefix test is missing its separator");
            }

            // A villager with several shows must get a different one each
            // visit, or the second visit is a repeat of the first.
            foreach (var kv in byOwner)
            {
                if (kv.Value.Count < 2 || string.IsNullOrEmpty(kv.Key))
                    continue;
                string who = video.shows[kv.Value[0]].who;
                var seen = new HashSet<int>();
                for (int visit = 0; visit < kv.Value.Count * 2; visit++)
                    seen.Add(Pick(who, ""));
                if (seen.Count != kv.Value.Count)
                    Fail(who + " has " + kv.Value.Count + " shows but " +
                         seen.Count + " came up over " + (kv.Value.Count * 2) +
                         " visits - the round-robin is stuck");
                Debug.Log("[Legaia] video: " + who + " rotates " + kv.Value.Count +
                          " shows across visits");
            }

            // Whether the scene actually holds these people is a property
            // of the WORLD, not of the config - a warning, never a failure.
            foreach (var kv in byOwner)
            {
                var s = video.shows[kv.Value[0]];
                if (!NpcInScene(s))
                    Warn("no NPC in this scene answers to '" + s.who + "' / '" +
                         s.token + "' - their show(s) will never come on here");
            }
        }

        /// Is there an NPC object in the built scene for this show's owner?
        /// Matched the way the runtime matches: object name against the
        /// token as a prefix segment, or a brain label against the name.
        static bool NpcInScene(LegaiaVideoShow show)
        {
            var brainType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcBrain");
            foreach (var t in Object.FindObjectsOfType<Transform>(true))
            {
                string n = t.gameObject.name;
                if (!string.IsNullOrEmpty(show.token) &&
                    (n == show.token || n.StartsWith(show.token + "_")))
                    return true;
                if (brainType == null || string.IsNullOrEmpty(show.who))
                    continue;
                var brain = t.GetComponent(brainType);
                if (brain == null)
                    continue;
                var label = brain.GetType().GetField("label", ANY)?.GetValue(brain) as string;
                if (!string.IsNullOrEmpty(label) &&
                    label.ToLower() == show.who.ToLower())
                    return true;
            }
            return false;
        }

        // --- The wiring ------------------------------------------------------

        static MethodInfo s_tryGet;

        /// A wired value off the BACKING UdonBehaviour in edit mode - the
        /// serialized side, which is what runs in-world.
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

        static void CheckWiring(Component tv, LegaiaVideoSettings video)
        {
            var backing = LegaiaCommonPrefabs.BackingUdon(tv);
            if (backing == null)
                Fail("the TV has no backing UdonBehaviour");

            int Len(string field)
            {
                object v = EditVar(backing, field);
                var arr = v as System.Array;
                if (arr == null)
                    Fail("the TV's " + field + " never reached the backing behaviour");
                return arr.Length;
            }

            if (Len("playlistUrls") != video.defaultPlaylist.Count)
                Fail("playlistUrls holds " + Len("playlistUrls") + " URL(s), the config " +
                     "has " + video.defaultPlaylist.Count);
            if (Len("playlistTitles") != video.defaultPlaylist.Count)
                Fail("playlistTitles does not match playlistUrls");

            int flat = video.FlatShowUrls().Count;
            if (Len("showUrls") != flat)
                Fail("showUrls holds " + Len("showUrls") + " URL(s), the shows have " + flat);
            foreach (string f in new[] { "showStart", "showCount", "showTitles",
                                         "showOwners", "showTokens", "showConsole" })
                if (Len(f) != video.shows.Count)
                    Fail(f + " has " + Len(f) + " entries for " + video.shows.Count +
                         " show(s)");

            // The windows must tile showUrls exactly - no gap, no overlap,
            // nothing past the end. This is the off-by-one that would play
            // the wrong villager's video.
            var starts = EditVar(backing, "showStart") as int[];
            var counts = EditVar(backing, "showCount") as int[];
            if (starts == null || counts == null)
                Fail("showStart / showCount are not int arrays on the backing behaviour");
            int at = 0;
            for (int i = 0; i < starts.Length; i++)
            {
                if (starts[i] != at)
                    Fail("show " + i + " starts at " + starts[i] + ", expected " + at);
                if (counts[i] != video.shows[i].urls.Count)
                    Fail("show " + i + " covers " + counts[i] + " URL(s), the config " +
                         "gives it " + video.shows[i].urls.Count);
                at += counts[i];
            }
            if (at != flat)
                Fail("the show windows cover " + at + " of " + flat + " URL(s)");

            // And the URLs themselves must be the config's, in order: a
            // VRCUrl array that survived serialization empty would pass
            // every length check above.
            var urls = EditVar(backing, "showUrls") as System.Array;
            var wanted = video.FlatShowUrls();
            for (int i = 0; i < wanted.Count; i++)
            {
                string got = UrlText(urls.GetValue(i));
                if (got != wanted[i])
                    Fail("show URL " + i + " serialized as '" + got + "', config says '" +
                         wanted[i] + "'");
            }
            var plUrls = EditVar(backing, "playlistUrls") as System.Array;
            for (int i = 0; i < video.defaultPlaylist.Count; i++)
            {
                string got = UrlText(plUrls.GetValue(i));
                if (got != video.defaultPlaylist[i].url)
                    Fail("playlist URL " + i + " serialized as '" + got + "'");
            }
            Debug.Log("[Legaia] video wiring: " + video.defaultPlaylist.Count +
                      " playlist + " + flat + " show URL(s) reached the backing " +
                      "behaviour, windows tile exactly");
        }

        static string UrlText(object vrcUrl)
        {
            if (vrcUrl == null)
                return "";
            var mi = vrcUrl.GetType().GetMethod("Get", new System.Type[0]);
            return mi == null ? vrcUrl.ToString() : (string)mi.Invoke(vrcUrl, null);
        }

        // --- The props -------------------------------------------------------

        static void CheckProps(Component tv, LegaiaVideoSettings video)
        {
            var tvT = tv.transform;
            var spotT = tvT.Find(LegaiaCommonPrefabs.WATCH_NAME);
            if (spotT == null)
                Fail("no " + LegaiaCommonPrefabs.WATCH_NAME + " under the TV - " +
                     "villagers have nowhere to stand and no show can ever start");

            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            var spotType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaTvWatchSpot");
            var station = stationType != null ? spotT.GetComponent(stationType) : null;
            var spot = spotType != null ? spotT.GetComponent(spotType) : null;
            if (station == null)
                Fail("the watch spot carries no LegaiaNpcStation - the director " +
                     "will never send anyone");
            if (spot == null)
                Fail("the watch spot carries no LegaiaTvWatchSpot");

            var sb = LegaiaCommonPrefabs.BackingUdon(station);
            var pb = LegaiaCommonPrefabs.BackingUdon(spot);
            if (!(EditVar(sb, "kind") is int kind) || kind != 0)
                Fail("the watch station is kind " + EditVar(sb, "kind") +
                     ", the director hands out kind 0 for props");
            if (EditVar(sb, "handler") == null)
                Fail("the watch station has no handler - arriving villagers would " +
                     "stand there and nothing would happen");
            if (EditVar(sb, "standPoint") == null)
                Fail("the watch station has no stand point");
            if (EditVar(pb, "tv") == null)
                Fail("the watch spot is not wired to the TV");
            if (!(EditVar(pb, "controllerItem") is int item) ||
                item != LegaiaCarryArt.ITEM_CONTROLLER)
                Fail("the watch spot hands out carry item " +
                     EditVar(pb, "controllerItem") + ", the controller is " +
                     LegaiaCarryArt.ITEM_CONTROLLER);

            // The villager must face the set, or the whole tableau reads as
            // someone standing with their back to the television.
            Vector3 toTv = tvT.position - spotT.position;
            toTv.y = 0f;
            float off = Vector3.Angle(spotT.forward, toTv.normalized);
            if (off > 30f)
                Fail("the watch spot faces " + off.ToString("0") +
                     " degrees away from the set");

            // The speaker: a TV is something a room listens to together,
            // so its field is deliberately wide. Four places carry these
            // numbers (both AudioSource radii and the VRC spatial
            // component's near / far) and three of them are silent when
            // wrong - the sound just stops sooner than anyone expects.
            var spk = tvT.Find("speaker");
            if (spk == null)
                Fail("no speaker under the TV");
            var au = spk.GetComponent<AudioSource>();
            if (au == null)
                Fail("the TV speaker has no AudioSource");
            if (Mathf.Abs(au.minDistance - LegaiaCommonPrefabs.TV_AUDIO_NEAR) > 0.01f ||
                Mathf.Abs(au.maxDistance - LegaiaCommonPrefabs.TV_AUDIO_FAR) > 0.01f)
                Fail("the TV speaker carries " + au.minDistance + " / " + au.maxDistance +
                     " m, the kit says " + LegaiaCommonPrefabs.TV_AUDIO_NEAR + " / " +
                     LegaiaCommonPrefabs.TV_AUDIO_FAR);
            if (au.rolloffMode != AudioRolloffMode.Linear)
                Fail("the TV speaker is on " + au.rolloffMode + " rolloff; the radii " +
                     "are chosen for Linear, where the field scales off maxDistance");
            if (au.spatialBlend < 0.99f)
                Fail("the TV speaker is not fully 3D (spatialBlend " + au.spatialBlend + ")");
            var vrcSpatial = LegaiaWorldBuilder.FindType(
                "VRC.SDK3.Components.VRCSpatialAudioSource");
            var vs = vrcSpatial != null ? spk.GetComponent(vrcSpatial) : null;
            if (vs != null)
            {
                var far = vrcSpatial.GetField("Far")?.GetValue(vs);
                if (far is float f && Mathf.Abs(f - LegaiaCommonPrefabs.TV_AUDIO_FAR) > 0.01f)
                    Fail("the VRC spatial component cuts the TV off at " + f +
                         " m while the AudioSource reaches " +
                         LegaiaCommonPrefabs.TV_AUDIO_FAR);
            }
            Debug.Log("[Legaia] video audio: speaker " +
                      LegaiaCommonPrefabs.TV_AUDIO_NEAR + " - " +
                      LegaiaCommonPrefabs.TV_AUDIO_FAR + " m, linear, 3D" +
                      (vs != null ? ", VRC spatial agrees" : " (no VRC spatial component)"));

            // The console: built only when a show wants one, and OFF until
            // one plays.
            var consoleT = tvT.Find(LegaiaCommonPrefabs.CONSOLE_NAME);
            if (video.AnyConsole())
            {
                if (consoleT == null)
                    Fail("a console show is configured but no " +
                         LegaiaCommonPrefabs.CONSOLE_NAME + " was built");
                if (consoleT.gameObject.activeSelf)
                    Fail("the console starts switched ON - it would sit in front of " +
                         "the set for a playlist nobody is playing on it");
                if (EditVar(LegaiaCommonPrefabs.BackingUdon(tv), "consoleProp") == null)
                    Fail("the console is built but the TV cannot switch it on");
                float dist = Vector3.Distance(consoleT.position, spotT.position);
                if (dist > 1.2f)
                    Fail("the console stands " + dist.ToString("0.00") +
                         " m from the watch spot - too far to read as theirs");
                if (consoleT.GetComponentsInChildren<Collider>(true).Length > 0)
                    Fail("the console has a collider - a player would trip on a prop " +
                         "that is not there most of the time");
                Debug.Log("[Legaia] video props: console " +
                          dist.ToString("0.00") + " m from the watch spot, inactive; " +
                          "watch spot faces the set within " + off.ToString("0") + " deg");
            }
            else if (consoleT != null)
            {
                Warn("a console prop exists but no show asks for one");
            }
        }
    }
}
