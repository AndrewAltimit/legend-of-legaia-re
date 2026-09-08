// The PLAY-MODE soak: the living town actually running, headless.
//
// Every other check in this kit is edit-mode geometry - markers exist, a
// route is complete, a field reached its backing behaviour. None of them
// runs a villager. This one enters play mode with ClientSim, forces the
// clock to night (or day), samples every brain at 2 Hz for a bounded
// stretch of simulated time, writes a per-villager TIMELINE, and asserts
// the night routine actually happened: walk to the door, door swings,
// step onto the doorway tile, teleport in, door shuts - and back out at
// dawn.
//
// RECIPE (from a project copy - never the editor's open project):
//
//   Unity.exe -batchmode -nographics -projectPath <copy>
//       -executeMethod LegaiaWorld.LegaiaBatchChecks.Soak
//       -legaiaSoakMode night -legaiaSoakSeconds 240
//       [-legaiaSoakScale 1] [-legaiaSoakLog <copy>\Logs\soak.timeline.log]
//       [-legaiaScene Assets/Scenes/<scene>.unity]
//       -logFile <copy>\Logs\soak.log
//
// NOTE: no `-quit`. The run enters play mode and drives itself from
// EditorApplication.update; it calls EditorApplication.Exit itself (0 =
// pass, 1 = an assertion failed, 3 = play mode never started, 4 = the
// wall-clock watchdog fired). `-quit` would tear the editor down before
// play mode ever begins.
//
// Modes:
//   night  the night routine. Forces LegaiaDayNight to night for the
//          first 65% of the budget, then to day, and asserts every homed
//          villager went in through its door and came back out.
//   day    a daytime soak: forces day for the whole budget and REPORTS
//          (never fails on) station visits, conversations, blocked walks
//          and distance walked per villager. This is the mode to run over
//          a merged kit when a new daytime layer lands.
//
// THE NIGHT HOST is watched separately and is NOT part of the exodus. A
// villager the settings pinned to a station for the night (town01: Cara at
// the card table) keeps that station by design, so counting it as a homed
// villager that never went in would fail every run. Instead the night mode
// asserts what it IS supposed to do: it reached its station, it held the
// seat for most of the night, and it never went home through a door. A
// scene built WITHOUT the station (the card table is another pass's
// object) leaves the villager ordinary, and it is watched as one.
//
// HOW IT SURVIVES THE DOMAIN RELOAD: entering play mode reloads the
// script domain, so the batch method cannot simply block. The config
// goes into SessionState (which survives a reload), and the
// [InitializeOnLoadMethod] hook below re-subscribes the update driver on
// the other side. The timeline file is written with AutoFlush, so a run
// that dies mid-way still leaves everything it had seen.
//
// FORCING THE CLOCK: LegaiaDayNight derives its phase from
// Networking.GetServerTimeInSeconds every frame, so writing its published
// fields while it runs would be overwritten within a frame. The harness
// disables the day/night UdonBehaviour and writes `isNight` / `dayFactor`
// / `sunUp` on its heap instead. The director reads those through
// GetProgramVariable, which does not care that the behaviour is disabled.

using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaSoak
    {
        const string K_ACTIVE = "legaia.soak.active";
        const string K_MODE = "legaia.soak.mode";
        const string K_SECONDS = "legaia.soak.seconds";
        const string K_SCALE = "legaia.soak.scale";
        const string K_LOG = "legaia.soak.log";

        // --- Entry (edit mode) ------------------------------------------------

        internal static void Run()
        {
            string scenePath = Arg("-legaiaScene", "Assets/Scenes/VRCDefaultWorldScene.unity");
            string mode = (Arg("-legaiaSoakMode", "night") ?? "night").ToLowerInvariant();
            if (mode != "night" && mode != "day")
                Bail("-legaiaSoakMode must be night or day, got '" + mode + "'");
            float seconds = ParseFloat(Arg("-legaiaSoakSeconds", "240"), 240f);
            float scale = Mathf.Clamp(ParseFloat(Arg("-legaiaSoakScale", "1"), 1f), 0.25f, 8f);
            string log = Arg("-legaiaSoakLog", null);
            if (string.IsNullOrEmpty(log))
                log = Path.Combine(Directory.GetCurrentDirectory(),
                    "Logs/soak-" + mode + ".timeline.log");

            // Compile the U# programs BEFORE the scene opens: U#'s
            // scene-upgrade pass runs inside OpenScene and reports
            // "Could not retrieve C# class" for any behaviour whose script
            // changed since the last run, which leaves every brain in the
            // scene without a proxy for the rest of the session.
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
                Bail("no LegaiaSpawn in " + scenePath + " - build the scene first");
            var rootT = spawn.transform.parent;
            if (rootT == null || !rootT.name.StartsWith("Legaia_"))
                Bail("LegaiaSpawn is not under a Legaia_<scene> root");
            string sceneName = rootT.name.Substring("Legaia_".Length);
            string manifestPath = "Assets/LegaiaImports/" + sceneName + "/manifest.json";
            if (!File.Exists(manifestPath))
                Bail("no manifest at " + manifestPath);
            object manifest = MiniJson.Parse(File.ReadAllText(manifestPath));

            // Re-apply the pass, so the kit under test is what runs (the
            // scene on disk may carry an older build of it).
            var settings = LegaiaSceneSettings.Load(sceneName);
            string manifestDir = "Assets/LegaiaImports/" + sceneName;
            settings.ApplyNpcOverrides(manifest, manifestDir, rootT.gameObject);
            LegaiaWorldBuilder.ReconcileNpcs(manifest, manifestDir, rootT.gameObject, sceneName, settings);
            var applied = LegaiaLivingTown.Apply(rootT.gameObject, manifest, sceneName,
                new LegaiaLivingTownOptions(), settings);
            if (applied == null)
                Bail("the living-town pass built nothing - nothing to soak");

            StripRendering();
            Directory.CreateDirectory(Path.GetDirectoryName(log).Replace('\\', '/'));
            SessionState.SetString(K_MODE, mode);
            SessionState.SetFloat(K_SECONDS, Mathf.Max(10f, seconds));
            SessionState.SetFloat(K_SCALE, scale);
            SessionState.SetString(K_LOG, log);
            SessionState.SetInt(K_ACTIVE, 1);
            Debug.Log("[Legaia] soak: entering play mode - mode " + mode + ", " +
                seconds + " simulated s at timeScale " + scale + ", timeline -> " + log);
            EditorApplication.EnterPlaymode();
        }

        /// Everything the soak does not simulate but a -nographics editor
        /// (shared with the card-game soak, which needs exactly the same
        /// teardown before it enters play mode)
        /// still tries to draw. `-batchmode -nographics` enters play mode
        /// with no graphics device, and the shadow-map pass over an
        /// offscreen camera (a VRChat mirror's reflection camera is one)
        /// dereferences it: the run dies with SIGSEGV in
        /// GfxDeviceClient::DrawSharedGeometryJobs before a single frame of
        /// the town is simulated. None of it is under test here.
        internal static void StripRendering()
        {
            QualitySettings.shadows = ShadowQuality.Disable;
            QualitySettings.shadowDistance = 0f;
            foreach (var l in Object.FindObjectsOfType<Light>(true))
                l.shadows = LightShadows.None;
            foreach (var c in Object.FindObjectsOfType<Camera>(true))
                c.enabled = false;
            var mirrorType = LegaiaWorldBuilder.FindType(
                "VRC.SDK3.Components.VRCMirrorReflection");
            int mirrors = 0;
            if (mirrorType != null)
                foreach (var m in Object.FindObjectsOfType(mirrorType, true))
                {
                    var c = m as Component;
                    if (c == null)
                        continue;
                    c.gameObject.SetActive(false);
                    mirrors++;
                }
            Debug.Log("[Legaia] soak: rendering stripped for the headless run - " +
                "shadows off, cameras off, " + mirrors + " mirror(s) disabled.");
        }

        static void Bail(string msg)
        {
            Debug.LogError("[Legaia] SOAK FAIL: " + msg);
            SessionState.SetInt(K_ACTIVE, 0);
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

        static float ParseFloat(string s, float fallback)
        {
            float v;
            return float.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out v)
                ? v : fallback;
        }

        // --- Driver (play mode, after the domain reload) ----------------------

        [InitializeOnLoadMethod]
        static void Hook()
        {
            if (SessionState.GetInt(K_ACTIVE, 0) == 0)
                return;
            EditorApplication.update -= Drive;
            EditorApplication.update += Drive;
        }

        static bool s_inited;
        static float s_wallStart;
        static float s_simStart;
        static float s_nextSample;
        static bool s_dayForced;
        static StreamWriter s_out;
        static List<Watch> s_watch;
        static Component s_dayNight;
        static string s_mode;
        static float s_seconds;
        static float s_scale;
        static float s_nightEnd;

        class Watch
        {
            public string name;
            public Transform tr;
            public Component brain;      // backing UdonBehaviour
            public Component loco;
            public Component doorUdon;   // the home's LegaiaDoor, backing
            public Animator doorAnim;
            public Transform homeDoor, homeThreshold, homeLanding, homeEmerge;
            public bool startIndoors, noRoute, hasHome, daytimeIndoors;

            public int lastState = -99;
            public bool lastIndoors, lastBlocked, lastDoorOpen;
            public int maxState;
            public int retries;          // 5/7/8 -> 0 without going in
            public int stationVisits;    // 1 -> 2
            public int chats;            // 3 -> 4
            public int blockedEdges;
            public int doorOpens, doorCloses;

            // The worst tilt this villager's rendered body reached, in
            // degrees FROM ITS OWN REST - not from world up. Most of these
            // rig families rest with their node axes flipped (RigPose
            // reports almost none of them as upright), so measuring
            // against world up says 180 for a villager standing perfectly
            // still. The gait and the nod tilt a few degrees on purpose;
            // a large drift means a gesture stopped taking itself back
            // off, which is how a villager ends up looking at the sky.
            public Vector3 restUp = Vector3.zero;
            public float maxTilt;
            public bool sawGoDoor, sawSwingWait, sawThreshold, wentIndoors, cameOut;
            public float inAt = -1f, outAt = -1f;
            public float doorDwell;      // simulated seconds spent in state 5
            public float distance;
            public Vector3 lastPos;
            public int ticks0 = -1, ticksN;
            public int hops;
            public int brainRetries;
            public float lastT;
            public bool nightIdle;
            // The night host (living_town.night_host): the villager pinned
            // to a station for the whole night instead of going home.
            public bool nightHost;
            public string hostPath = "";
            public float hostSeated;     // simulated seconds seated BEFORE dawn
            public bool hostEverSeated;
            public int hostRetries;
        }

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
            s_mode = SessionState.GetString(K_MODE, "night");
            s_seconds = SessionState.GetFloat(K_SECONDS, 240f);
            s_scale = SessionState.GetFloat(K_SCALE, 1f);
            string log = SessionState.GetString(K_LOG, "Logs/soak.timeline.log");
            s_nightEnd = s_mode == "night" ? s_seconds * 0.65f : s_seconds;

            try
            {
                s_out = new StreamWriter(log, false);
                s_out.AutoFlush = true;
            }
            catch (System.Exception e)
            {
                Debug.LogError("[Legaia] soak: cannot open " + log + ": " + e.Message);
                s_out = null;
            }

            Time.timeScale = s_scale;
            s_watch = Collect();
            s_dayNight = FindDayNight();
            s_dayForced = s_mode != "night";
            ForceClock(s_dayForced);
            s_simStart = Time.time;
            s_wallStart = Time.realtimeSinceStartup;
            s_nextSample = 0f;
            s_inited = true;

            Line("# legaia living-town soak, mode " + s_mode + ", budget " +
                 s_seconds + " simulated s, timeScale " + s_scale);
            Line("# columns: t  npc  st=state in=indoors p=(x,z) a=arrived b=blocked " +
                 "path=havePath m=locoMode door=<prop> dd=metres to the door stand spot");
            Debug.Log("[Legaia] soak: play mode up - " + s_watch.Count +
                " brain(s) watched, clock forced to " +
                (s_dayForced ? "day" : "night") + ".");
            foreach (var w in s_watch)
                Debug.Log("[Legaia] soak: " + w.name +
                    " home=" + (w.hasHome ? Fmt(w.homeDoor.position) : "-") +
                    " threshold=" + (w.homeThreshold != null ? Fmt(w.homeThreshold.position) : "-") +
                    " doorProp=" + (w.doorUdon != null ? w.doorUdon.transform.name : "-") +
                    " startIndoors=" + w.startIndoors + " noRoute=" + w.noRoute);
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

            // Per-frame edge detection; the timeline itself is written at 2 Hz.
            bool emit = t >= s_nextSample;
            if (emit)
                s_nextSample = t + 0.5f;
            for (int i = 0; i < s_watch.Count; i++)
                Sample(s_watch[i], t, emit);

            if (s_mode == "night" && !s_dayForced && t >= s_nightEnd)
            {
                s_dayForced = true;
                ForceClock(true);
                Line("# --- dawn forced at t=" + t.ToString("0.0") + " ---");
                Debug.Log("[Legaia] soak: dawn forced at t=" + t.ToString("0.0"));
            }
            if (t >= s_seconds)
                Report();
        }

        static void Sample(Watch w, float t, bool emit)
        {
            if (w.tr == null)
                return;
            int state = GetInt(w.brain, "state", -1);
            bool indoors = GetBool(w.brain, "indoors");
            bool arrived = GetBool(w.loco, "arrived");
            bool blocked = GetBool(w.loco, "blocked");
            bool havePath = GetBool(w.loco, "havePath");
            int lmode = GetInt(w.loco, "mode", -1);
            int ticks = GetInt(w.brain, "tickCount", -1);
            if (w.ticks0 < 0 && ticks >= 0)
                w.ticks0 = ticks;
            if (ticks >= 0)
                w.ticksN = ticks;

            Vector3 p = w.tr.position;
            if (w.lastState != -99)
            {
                float step = Vector3.Distance(p, w.lastPos);
                if (step < 2f)       // a teleport is not a walk
                    w.distance += step;
            }
            w.lastPos = p;

            if (state > w.maxState)
                w.maxState = state;
            if (state == 5)
            {
                w.sawGoDoor = true;
                // Accumulate from the sample clock, not Time.deltaTime:
                // the editor's update callback can fire more than once per
                // rendered frame, and summing deltaTime there counts the
                // same frame twice (it read 2343 s over a 156 s night).
                w.doorDwell += Mathf.Max(0f, t - w.lastT);
            }
            if (state == 9)
                w.nightIdle = true;
            if (w.nightHost)
            {
                w.hostRetries = GetInt(w.brain, "nightHostRetries", w.hostRetries);
                if (GetBool(w.brain, "nightHostSeated"))
                {
                    w.hostEverSeated = true;
                    // Only the night counts: the seat is released at dawn.
                    if (t <= s_nightEnd)
                        w.hostSeated += Mathf.Max(0f, t - w.lastT);
                }
            }
            w.lastT = t;
            w.hops = GetInt(w.loco, "hops", w.hops);
            w.brainRetries = GetInt(w.brain, "homeRetries", w.brainRetries);
            if (state == 7)
                w.sawSwingWait = true;
            if (state == 8)
                w.sawThreshold = true;
            if (w.lastState != state)
            {
                if (w.lastState == 1 && state == 2)
                    w.stationVisits++;
                if (w.lastState == 3 && state == 4)
                    w.chats++;
                if ((w.lastState == 5 || w.lastState == 7 || w.lastState == 8)
                    && state == 0 && !indoors)
                    w.retries++;
                w.lastState = state;
            }
            if (blocked && !w.lastBlocked)
                w.blockedEdges++;
            w.lastBlocked = blocked;
            if (indoors && !w.wentIndoors)
            {
                w.wentIndoors = true;
                w.inAt = t;
            }
            if (w.wentIndoors && !indoors && !w.cameOut && !w.startIndoors)
            {
                w.cameOut = true;
                w.outAt = t;
            }
            w.lastIndoors = indoors;

            // Body tilt, measured on the RENDERED up of the villager's own
            // mesh (TransformPoint difference, so the builder's mirrors are
            // included) rather than on transform.up, which they invert.
            if (w.tr != null)
            {
                // The glb's own root under the instance - the node the
                // kit's body pose owns (bob, roll, nod all compose into
                // one absolute write there). Deliberately NOT the first
                // mesh node found: on several rig families that is a limb,
                // and the sitting pose swings a thigh through 85 degrees
                // quite legitimately, which reads as a flipped villager.
                Transform bt = null;
                foreach (Transform c in w.tr)
                {
                    if (c.name == "speech_bubble" || c.name == "carry")
                        continue;
                    if (c.GetComponentInChildren<MeshFilter>() == null)
                        continue;
                    bt = c;
                    break;
                }
                if (bt != null)
                {
                    Vector3 up = (bt.TransformPoint(Vector3.up)
                                  - bt.TransformPoint(Vector3.zero)).normalized;
                    if (w.restUp == Vector3.zero)
                        w.restUp = up;   // the first sample IS the rest pose
                    else
                    {
                        float tilt = Vector3.Angle(up, w.restUp);
                        if (tilt > w.maxTilt)
                            w.maxTilt = tilt;
                    }
                }
            }

            string door = "-";
            if (w.doorUdon != null)
            {
                bool open = GetBool(w.doorUdon, "opened");
                if (open != w.lastDoorOpen)
                {
                    if (open)
                        w.doorOpens++;
                    else
                        w.doorCloses++;
                    w.lastDoorOpen = open;
                }
                door = open ? "OPEN" : "shut";
                if (w.doorAnim != null)
                {
                    var si = w.doorAnim.GetCurrentAnimatorStateInfo(0);
                    door += "/" + (si.IsName("open") ? "anim:open" : "anim:closed") +
                        "@" + si.normalizedTime.ToString("0.00");
                }
            }

            if (!emit)
                return;
            Line(t.ToString("000.0") + "  " + w.name.PadRight(28) +
                 " st=" + state + " in=" + (indoors ? 1 : 0) +
                 " p=" + Fmt(p) +
                 " a=" + (arrived ? 1 : 0) + " b=" + (blocked ? 1 : 0) +
                 " path=" + (havePath ? 1 : 0) + " m=" + lmode +
                 " hop=" + w.hops +
                 " door=" + door +
                 (w.hasHome ? " dd=" + Vector3.Distance(p, w.homeDoor.position).ToString("0.00") : ""));
        }

        // --- Report ------------------------------------------------------------

        static void Report()
        {
            int homed = 0, wentIn = 0, cameOut = 0, swung = 0, stuck = 0, hops = 0;
            int hosts = 0, dayIn = 0;
            float hostSeated = 0f;
            var problems = new List<string>();
            Line("");
            Line("# --- summary ---");
            for (int i = 0; i < s_watch.Count; i++)
            {
                Watch w = s_watch[i];
                float rate = w.ticks0 >= 0 && s_seconds > 0f
                    ? (w.ticksN - w.ticks0) / s_seconds : -1f;
                string row = w.name + ": home=" + (w.hasHome ? "yes" : "no") +
                    " startIndoors=" + w.startIndoors + " dayIn=" + w.daytimeIndoors +
                    " noRoute=" + w.noRoute +
                    " maxState=" + w.maxState + " goDoor=" + w.sawGoDoor +
                    " swingWait=" + w.sawSwingWait + " threshold=" + w.sawThreshold +
                    " in=" + w.wentIndoors + "@" + w.inAt.ToString("0.0") +
                    " out=" + w.cameOut + "@" + w.outAt.ToString("0.0") +
                    " retries=" + w.retries + " blocked=" + w.blockedEdges +
                    " doorOpens=" + w.doorOpens + " doorCloses=" + w.doorCloses +
                    " stations=" + w.stationVisits + " chats=" + w.chats +
                    " walked=" + w.distance.ToString("0.0") + "m" +
                    " hops=" + w.hops + " brainRetries=" + w.brainRetries +
                    " gaveUp=" + w.nightIdle +
                    " atDoor=" + w.doorDwell.ToString("0.0") + "s" +
                    " maxTilt=" + w.maxTilt.ToString("0.0") + "deg" +
                    " ticks/s=" + rate.ToString("0.0");
                Line(row);
                Debug.Log("[Legaia] soak: " + row);
                // The gait rolls a few degrees and the nod dips a few more.
                // A villager tilted past 25 degrees is not gesturing, it is
                // accumulating one - the failure that had a villager
                // looking at the sky after enough conversations.
                if (w.maxTilt > 25f)
                    problems.Add(w.name + " tilted " + w.maxTilt.ToString("0") +
                        " degrees off upright - a gesture is not taking itself " +
                        "back off");
                if (w.nightHost && s_mode == "night")
                {
                    // The night host is NOT part of the exodus: it keeps its
                    // station all night by design, so counting it as a homed
                    // villager that never went in would fail every run.
                    hosts++;
                    string hrow = w.name + ": NIGHT HOST " + w.hostPath +
                        " seated=" + w.hostSeated.ToString("0.0") + "s of " +
                        s_nightEnd.ToString("0") + "s night, everSeated=" +
                        w.hostEverSeated + " wentIndoors=" + w.wentIndoors +
                        " retries=" + w.hostRetries;
                    Line(hrow);
                    Debug.Log("[Legaia] soak: " + hrow);
                    hostSeated = w.hostSeated;
                    if (!w.hostEverSeated)
                        problems.Add(w.name + " is the night host but never took " +
                            w.hostPath + " (" + w.hostRetries + " retries, " +
                            w.blockedEdges + " blocked walks)");
                    else if (w.hostSeated < s_nightEnd * 0.5f)
                        problems.Add(w.name + " held its night station for only " +
                            w.hostSeated.ToString("0") + " s of a " +
                            s_nightEnd.ToString("0") + " s night");
                    if (w.wentIndoors)
                        problems.Add(w.name + " is the night host but went home " +
                            "through a door instead of keeping its station");
                    continue;
                }
                if (!w.hasHome)
                    continue;
                homed++;
                hops += w.hops;
                if (w.wentIndoors)
                    wentIn++;
                else
                    problems.Add(w.name + " never got inside (maxState " + w.maxState +
                        ", " + w.retries + " retries, " + w.blockedEdges +
                        " blocked, " + w.doorDwell.ToString("0") + " s at the door)");
                if (w.cameOut)
                    cameOut++;
                else if (w.wentIndoors && !w.daytimeIndoors)
                    problems.Add(w.name + " never came back out at dawn");
                else if (w.wentIndoors)
                    // `daytimeIndoors` is the share the pass deliberately
                    // keeps in by day (a shopkeeper, somebody's
                    // grandmother), and the builder always picks it from
                    // the HOMED villagers - so a night soak that asserted
                    // "everybody who went in came out" was asserting
                    // against the pass's own design.
                    dayIn++;
                if (w.doorUdon != null)
                {
                    if (w.doorOpens > 0 && w.doorCloses > 0)
                        swung++;
                    else
                        problems.Add(w.name + "'s door prop never swung " +
                            "(opens " + w.doorOpens + ", closes " + w.doorCloses + ")");
                }
                if (w.retries > 2 || w.brainRetries > 2 || w.nightIdle)
                {
                    stuck++;
                    problems.Add(w.name + " gave up on the door trip (" +
                        w.retries + " state retries, " + w.brainRetries +
                        " brain retries" + (w.nightIdle ? ", stood out all night" : "") +
                        ") - it is stuck at the door, not walking home");
                }
            }

            int stations = 0, chats = 0, blockedWalks = 0, hopsAll = 0;
            float walked = 0f;
            for (int i = 0; i < s_watch.Count; i++)
            {
                stations += s_watch[i].stationVisits;
                chats += s_watch[i].chats;
                blockedWalks += s_watch[i].blockedEdges;
                hopsAll += s_watch[i].hops;
                walked += s_watch[i].distance;
            }
            string summary = s_mode == "day"
                ? "[Legaia] SOAK day: " + s_watch.Count + " villager(s) over " +
                  s_seconds.ToString("0") + " simulated s - " + stations +
                  " station visit(s), " + chats + " conversation(s) joined, " +
                  hopsAll + " ledge hop(s), " + blockedWalks +
                  " blocked walk(s), " + walked.ToString("0") + " m walked."
                : "[Legaia] SOAK night: " + wentIn + "/" + homed +
                  " homed villager(s) went in, " + cameOut + "/" + homed +
                  " came back out at dawn, " + swung +
                  " door prop(s) swung open and shut, " + hops +
                  " ledge hop(s) (" + dayIn + " stayed in by day, as built), " +
                  stuck + " stuck at a door, " + hosts +
                  " night host(s) keeping a station (" +
                  hostSeated.ToString("0") + " s seated of a " +
                  s_nightEnd.ToString("0") + " s night).";
            Line("");
            Line(summary);
            Debug.Log(summary);

            if (s_mode == "day")
            {
                Finish(0, null);
                return;
            }
            if (homed == 0)
                problems.Add("no villager has a home at all - the night routine " +
                    "cannot run in this scene");
            if (problems.Count > 0)
            {
                for (int i = 0; i < problems.Count; i++)
                    Debug.LogError("[Legaia] SOAK FAIL: " + problems[i]);
                Finish(1, problems.Count + " problem(s)");
                return;
            }
            Finish(0, null);
        }

        static void Finish(int code, string why)
        {
            if (why != null)
                Line("# FAILED: " + why);
            if (code == 0)
                Debug.Log("[Legaia] SELFTEST OK: " + s_mode + " soak complete.");
            else
                Debug.LogError("[Legaia] SOAK FAIL: " + why);
            SessionState.SetInt(K_ACTIVE, 0);
            EditorApplication.update -= Drive;
            if (s_out != null)
            {
                s_out.Flush();
                s_out.Close();
                s_out = null;
            }
            if (Application.isBatchMode)
                EditorApplication.Exit(code);
            else
                EditorApplication.isPlaying = false;
        }

        // --- Scene reading -----------------------------------------------------

        static List<Watch> Collect()
        {
            var list = new List<Watch>();
            var brainType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcBrain");
            if (brainType == null)
                return list;
            var found = Object.FindObjectsOfType(brainType, true);
            for (int i = 0; i < found.Length; i++)
            {
                var proxy = found[i] as Component;
                if (proxy == null)
                    continue;
                var w = new Watch();
                w.name = proxy.transform.name;
                w.tr = proxy.transform;
                w.brain = LegaiaCommonPrefabs.BackingUdon(proxy);
                w.lastPos = w.tr.position;
                w.homeDoor = Field(proxy, "homeDoor") as Transform;
                w.homeThreshold = Field(proxy, "homeThreshold") as Transform;
                w.homeLanding = Field(proxy, "homeLanding") as Transform;
                w.homeEmerge = Field(proxy, "homeEmerge") as Transform;
                w.startIndoors = Field(proxy, "startIndoors") is bool si && si;
                w.daytimeIndoors = Field(proxy, "daytimeIndoors") is bool di && di;
                w.noRoute = Field(proxy, "noRoute") is bool nr && nr;
                w.hasHome = w.homeDoor != null && w.homeLanding != null;
                var loco = Field(proxy, "loco") as Component;
                if (loco != null)
                    w.loco = LegaiaCommonPrefabs.BackingUdon(loco);
                w.hostPath = Field(proxy, "nightHostStationPath") as string ?? "";
                // Only a host whose station is actually in this scene counts:
                // the card table is another pass's object, and a scene built
                // without it leaves the villager an ordinary one.
                w.nightHost = w.hostPath.Length > 0 &&
                    GameObject.Find(w.hostPath) != null;
                var prop = Field(proxy, "homeDoorProp") as Component;
                if (prop != null)
                {
                    w.doorUdon = LegaiaCommonPrefabs.BackingUdon(prop);
                    w.doorAnim = Field(prop, "doorAnimator") as Animator;
                }
                list.Add(w);
            }
            list.Sort((a, b) => string.CompareOrdinal(a.name, b.name));
            return list;
        }

        static Component FindDayNight()
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
            if (t == null)
                return null;
            var proxy = Object.FindObjectOfType(t, true) as Component;
            return proxy == null ? null : LegaiaCommonPrefabs.BackingUdon(proxy);
        }

        /// Pin the published day/night phase. The behaviour itself is
        /// switched off first: it rewrites these fields every frame from the
        /// server clock, so a write while it runs lasts one frame.
        static void ForceClock(bool day)
        {
            if (s_dayNight == null)
            {
                Debug.LogWarning("[Legaia] soak: no LegaiaDayNight in the scene - " +
                    "the director will never shelter anyone.");
                return;
            }
            var beh = s_dayNight as Behaviour;
            if (beh != null)
                beh.enabled = false;
            SetVar(s_dayNight, "isNight", !day);
            SetVar(s_dayNight, "dayFactor", day ? 1f : 0f);
            SetVar(s_dayNight, "sunUp", day ? 1f : -1f);
            SetVar(s_dayNight, "phase", day ? 0.35f : 0.85f);
        }

        // --- Reflection helpers -------------------------------------------------

        static MethodInfo s_getVar, s_setVar;

        /// The one non-generic overload of `name` taking `argc` arguments.
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
            // Pick the NON-generic overload by hand: UdonBehaviour also
            // carries GetProgramVariable<T>(string), and asking GetMethod
            // for a (string) signature is an AmbiguousMatchException.
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
            catch (System.Exception e)
            {
                Debug.LogWarning("[Legaia] soak: cannot set " + name + ": " + e.Message);
            }
        }

        static int GetInt(Component udon, string name, int fallback)
        {
            object v = GetVar(udon, name);
            return v is int ? (int)v : fallback;
        }

        static bool GetBool(Component udon, string name)
        {
            object v = GetVar(udon, name);
            return v is bool && (bool)v;
        }

        /// A field off a U# PROXY (object references the pass wired), which
        /// survives into play mode - U# only nukes proxies for a real build.
        static object Field(Component proxy, string name)
        {
            if (proxy == null)
                return null;
            var f = proxy.GetType().GetField(name,
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            return f == null ? null : f.GetValue(proxy);
        }

        static string Fmt(Vector3 p)
        {
            return "(" + p.x.ToString("0.00") + "," + p.z.ToString("0.00") + ")";
        }

        static void Line(string s)
        {
            if (s_out != null)
                s_out.WriteLine(s);
        }
    }
}
