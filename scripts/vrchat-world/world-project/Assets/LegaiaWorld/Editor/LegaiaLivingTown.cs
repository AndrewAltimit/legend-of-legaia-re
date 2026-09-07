// The LIVING TOWN pass: turns the builder's strolling villagers into a
// small Sims-shaped village - people meet and talk in twos and threes,
// walk to things and use them (a cupboard swings open while someone is at
// it), and go indoors at night through the same doorway pairs a player
// walks through.
//
// What it builds under the built root, in a `living_town` child:
//
//   living_town/director            LegaiaTownDirector (one per scene)
//   living_town/stations/...        LegaiaNpcStation instances:
//                                     kind 0 in front of every one-shot,
//                                       non-door prop (cupboards, drawers,
//                                       the shop's upstairs door), with the
//                                       prop's own LegaiaDoor as handler -
//                                       that is what opens it;
//                                     kind 3 chat RINGS: three stand points
//                                       around one meeting spot, so a group
//                                       of three is expressible in the
//                                       station contract without changing
//                                       it (the director groups a ring by
//                                       proximity);
//                                     kind 4 indoor viewpoints, so a
//                                       villager who went inside has
//                                       somewhere to be.
//   living_town/homes/home_N/...    door / landing / exit / emerge markers
//                                   lifted from the manifest's teleports.
//
// and, on each eligible NPC, a LegaiaNpcBrain plus a `speech_bubble` child
// (LegaiaSpeechBubble + the generated icon quads).
//
// STATION KINDS ARE NOT OURS ALONE. The director picks free stations
// generically by kind / indoors / IsFree(), so the fishing spots and
// card-table seats another pass builds against the same contract are used
// by these villagers without either side knowing about the other. This
// pass only ever CREATES kinds 0, 3 and 4.
//
// INTERIORS. Retail's house interiors are unused corners of the same map,
// reached only by a doorway teleport, so "inside" is a distance test, not a
// coordinate rule: an endpoint further than `interiorDistance` from the
// manifest spawn is interior (the same reading the realism pass's interior
// shells use). A teleport whose trigger is outside and whose destination is
// interior is a front DOOR; the interior-side teleport nearest that landing
// is its way out. Homes are assigned at BUILD time from a seeded shuffle,
// capped per door, so every client agrees on who lives where with no synced
// state at all.
//
// MIRRORS. Every position here is computed in the manifest frame (which is
// the built root's LOCAL frame, one G2U flip from the manifest) and every
// direction that must survive into world space is taken as a difference of
// two TransformPoints - never TransformDirection, which drops the scale
// mirrors the root and the NPC instances carry. Stand-point facings are
// stored as world ROTATIONS on the marker, which is what the station
// contract's StandForward() reads.
//
// Idempotent: Apply() strips the previous container, brains and bubbles
// first, so re-applying refreshes rather than stacks.

using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Animations;
using UnityEngine;

namespace LegaiaWorld
{
    [System.Serializable]
    public class LegaiaLivingTownOptions
    {
        public bool livingTown = true;
        [Tooltip("Villagers assigned to one house door, at most.")]
        public int homeCap = 4;
        [Tooltip("Conversation spots built in the village.")]
        public int maxChatSpots = 5;
        [Tooltip("Open-air spots to stand and look, so daytime has errands " +
                 "even where every usable prop is indoors.")]
        public int outdoorViewpoints = 6;
        [Tooltip("Radius of a conversation ring (meters).")]
        public float chatRingRadius = 0.85f;
        [Tooltip("How far in front of a prop the NPC stands (meters).")]
        public float propStandDistance = 0.7f;
        [Tooltip("Speed of a purposeful walk (m/s); the stroll keeps its own.")]
        public float walkSpeed = 0.7f;
        [Tooltip("Speech bubbles over the talker's head.")]
        public bool speechBubbles = true;
        [Tooltip("Print the NPC's own first dialog line under the bubble icon.")]
        public bool bubbleText = true;
        [Tooltip("Share of villagers who stay indoors by day as well.")]
        public float daytimeIndoorsShare = 0.18f;
        [Tooltip("Scene-constant seed: home assignment and personalities.")]
        public int seed = 20260907;
        [Tooltip("A teleport endpoint further than this from the spawn is an interior.")]
        public float interiorDistance = 30f;
        [Tooltip("Clip name to bind as the walk cycle when a rig carries it.")]
        public string walkClip = "record_36";
        [Tooltip("Bind the measured walk cycle (idle/walk Animator) where the rig has one.")]
        public bool walkAnimator = true;

        /// A working copy, so a scene settings override never mutates the
        /// builder window's own options object.
        public LegaiaLivingTownOptions Clone()
        {
            return (LegaiaLivingTownOptions)MemberwiseClone();
        }
    }

    public static class LegaiaLivingTown
    {
        public const string CONTAINER = "living_town";

        /// One home: the village-side door, the interior landing it drops
        /// you at, the interior-side way out and the village landing it
        /// returns to. Lifted from a pair of manifest teleports.
        class Home
        {
            public Vector3 door;       // root-local
            public Vector3 landing;
            public Vector3 landingFace; // root-local direction
            public bool hasExit;
            public Vector3 exit;
            public Vector3 emerge;
            public Vector3 emergeFace;
            public int occupants;
            public Transform doorT, landingT, exitT, emergeT;
        }

        // --- Entry points -----------------------------------------------------

        public static GameObject Apply(GameObject root, object manifest,
            string sceneName, LegaiaLivingTownOptions o, LegaiaSceneSettings settings)
        {
            if (root == null || manifest == null || o == null || !o.livingTown)
                return null;
            if (settings == null)
                settings = new LegaiaSceneSettings();
            o = o.Clone();
            settings.ApplyLivingTown(o);
            Remove(root);

            string genDir = "Assets/LegaiaGenerated/" + sceneName + "/livingtown";
            Directory.CreateDirectory(genDir);

            var container = new GameObject(CONTAINER);
            container.transform.SetParent(root.transform, false);
            Undo.RegisterCreatedObjectUndo(container, "Legaia living town");

            s_npcRoot = root.transform.Find("npcs");
            Vector3 spawn = LegaiaWorldBuilder.G2U(
                MiniJson.GetVec3(MiniJson.Get(manifest, "spawn"), "position"));

            var homes = ReadHomes(manifest, spawn, o);
            var homesRoot = new GameObject("homes");
            homesRoot.transform.SetParent(container.transform, false);
            for (int i = 0; i < homes.Count; i++)
                MakeHomeMarkers(root.transform, homesRoot.transform, homes[i], i);

            var stationsRoot = new GameObject("stations");
            stationsRoot.transform.SetParent(container.transform, false);
            int propStations = BuildPropStations(root, manifest, stationsRoot.transform,
                spawn, o);
            int chatStations = BuildChatRings(root, manifest, stationsRoot.transform,
                settings, spawn, o);
            int viewStations = BuildIndoorViewpoints(root, stationsRoot.transform,
                homes, o);
            viewStations += BuildOutdoorViewpoints(root, stationsRoot.transform,
                spawn, o);

            var brains = WireBrains(root, manifest, sceneName, genDir, settings, o, homes);

            var director = BuildDirector(root, container.transform, brains, o);
            // The back-reference: brains are built before the director (it
            // needs their array), so the link is closed here.
            if (director != null)
                foreach (var brain in brains)
                {
                    LegaiaWorldBuilder.SetUdonField(brain, "director", director);
                    LegaiaWorldBuilder.SyncUdonProxy(brain);
                }

            Debug.Log("[Legaia] living town: " + brains.Count + " villager(s), " +
                homes.Count + " home(s) (cap " + o.homeCap + "), " +
                propStations + " prop station(s), " + chatStations +
                " chat stand point(s), " + viewStations + " viewpoint(s).");
            if (director == null)
                Debug.LogWarning("[Legaia] living town: no director behaviour " +
                    "(VRChat SDK / UdonSharp missing?) - the village stays inert.");
            return container;
        }

        /// Strip everything a previous Apply built: the container, and the
        /// brains + bubbles that live on the NPC instances themselves.
        public static void Remove(GameObject root)
        {
            if (root == null)
                return;
            var old = root.transform.Find(CONTAINER);
            if (old != null)
                Undo.DestroyObjectImmediate(old.gameObject);
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return;
            foreach (Transform npc in npcRoot)
            {
                StripUdon(npc.gameObject, "LegaiaNpcBrain");
                var bubble = npc.Find("speech_bubble");
                if (bubble != null)
                {
                    StripUdon(bubble.gameObject, "LegaiaSpeechBubble");
                    Undo.DestroyObjectImmediate(bubble.gameObject);
                }
            }
        }

        /// Destroy a U# proxy AND its backing UdonBehaviour (destroying only
        /// the proxy leaves the program that actually runs in-world).
        static void StripUdon(GameObject go, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + typeName);
            if (t == null)
                return;
            foreach (var comp in go.GetComponents(t))
            {
                var backing = LegaiaCommonPrefabs.BackingUdon(comp);
                if (backing != null)
                    Undo.DestroyObjectImmediate(backing);
                Undo.DestroyObjectImmediate(comp);
            }
        }

        // --- Homes ---------------------------------------------------------------

        static bool IsInterior(Vector3 local, Vector3 spawn, LegaiaLivingTownOptions o)
        {
            float dx = local.x - spawn.x, dz = local.z - spawn.z;
            return Mathf.Sqrt(dx * dx + dz * dz) > o.interiorDistance;
        }

        /// Pair the manifest's teleports into homes: an outside->inside
        /// teleport is a front door, and the inside->outside teleport whose
        /// trigger is nearest that landing is its way back out.
        static List<Home> ReadHomes(object manifest, Vector3 spawn,
            LegaiaLivingTownOptions o)
        {
            var entries = new List<Home>();
            var exitTrig = new List<Vector3>();
            var exitDest = new List<Vector3>();
            var exitFace = new List<Vector3>();
            foreach (object tp in MiniJson.AsList(MiniJson.Get(manifest, "teleports"))
                     ?? new List<object>())
            {
                Vector3 trig = LegaiaWorldBuilder.G2U(
                    MiniJson.GetVec3(MiniJson.Get(tp, "trigger"), "position"));
                Vector3 dest = LegaiaWorldBuilder.G2U(
                    MiniJson.GetVec3(MiniJson.Get(tp, "destination"), "position"));
                Vector3 face = Vector3.zero;
                var fd = MiniJson.AsList(MiniJson.Get(tp, "facing_dir"));
                if (fd != null && fd.Count >= 2)
                    face = LegaiaWorldBuilder.G2U(new Vector3(
                        (float)MiniJson.AsNum(fd[0]), 0f, (float)MiniJson.AsNum(fd[1])));
                bool trigIn = IsInterior(trig, spawn, o);
                bool destIn = IsInterior(dest, spawn, o);
                if (!trigIn && destIn)
                {
                    entries.Add(new Home
                    {
                        door = trig,
                        landing = dest,
                        landingFace = face,
                    });
                }
                else if (trigIn && !destIn)
                {
                    exitTrig.Add(trig);
                    exitDest.Add(dest);
                    exitFace.Add(face);
                }
            }
            // Match each door's landing with the nearest interior-side exit.
            for (int i = 0; i < entries.Count; i++)
            {
                int best = -1;
                float bestD = 12f;
                for (int j = 0; j < exitTrig.Count; j++)
                {
                    float d = Vector3.Distance(entries[i].landing, exitTrig[j]);
                    if (d < bestD)
                    {
                        bestD = d;
                        best = j;
                    }
                }
                if (best < 0)
                    continue;
                entries[i].hasExit = true;
                entries[i].exit = exitTrig[best];
                entries[i].emerge = exitDest[best];
                entries[i].emergeFace = exitFace[best];
                // No authored arrival facing: look into the room, i.e. away
                // from the door you just came through.
                if (entries[i].landingFace.sqrMagnitude < 1e-6f)
                    entries[i].landingFace = entries[i].landing - entries[i].exit;
            }
            foreach (var h in entries)
                if (h.emergeFace.sqrMagnitude < 1e-6f)
                    h.emergeFace = h.emerge - h.door;
            return entries;
        }

        static void MakeHomeMarkers(Transform root, Transform parent, Home h, int index)
        {
            var go = new GameObject("home_" + index);
            go.transform.SetParent(parent, false);
            h.doorT = Marker(root, go.transform, "door", h.door, h.landing - h.door);
            h.landingT = Marker(root, go.transform, "landing", h.landing, h.landingFace);
            if (h.hasExit)
            {
                h.exitT = Marker(root, go.transform, "exit", h.exit, h.exit - h.landing);
                h.emergeT = Marker(root, go.transform, "emerge", h.emerge, h.emergeFace);
            }
        }

        /// A marker at a root-LOCAL position, floor-snapped, whose world
        /// rotation encodes `localDir` as a facing. The direction is turned
        /// into world space as a TransformPoint difference so the root's
        /// mirror is applied exactly once (TransformDirection would drop it).
        static Transform Marker(Transform root, Transform parent, string name,
            Vector3 local, Vector3 localDir)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.position = SnapFloor(root.TransformPoint(local));
            Vector3 world = root.TransformPoint(local + localDir)
                - root.TransformPoint(local);
            world.y = 0f;
            if (world.sqrMagnitude > 1e-6f)
                go.transform.rotation =
                    Quaternion.LookRotation(world.normalized, Vector3.up);
            return go.transform;
        }

        static Vector3 SnapFloor(Vector3 world)
        {
            RaycastHit hit;
            if (Physics.Raycast(world + Vector3.up * 3f, Vector3.down, out hit, 12f,
                    ~0, QueryTriggerInteraction.Ignore))
                return hit.point + Vector3.up * 0.02f;
            return world;
        }

        static bool HasFloor(Vector3 world, out Vector3 floor)
        {
            RaycastHit hit;
            if (Physics.Raycast(world + Vector3.up * 3f, Vector3.down, out hit, 12f,
                    ~0, QueryTriggerInteraction.Ignore))
            {
                floor = hit.point + Vector3.up * 0.02f;
                return hit.normal.y > 0.7f;
            }
            floor = world;
            return false;
        }

        /// Room to stand: nothing solid in the villager-sized volume just
        /// above the spot (the floor itself sits below the sphere).
        /// The VILLAGERS' own capsules do not count - a chat spot is chosen
        /// exactly where people already stand, and counting their colliders
        /// would reject every good meeting place in the village.
        static bool StandingRoom(Vector3 floor)
        {
            var hits = Physics.OverlapSphere(floor + Vector3.up * 0.55f, 0.28f,
                ~0, QueryTriggerInteraction.Ignore);
            for (int i = 0; i < hits.Length; i++)
            {
                if (s_npcRoot != null && hits[i].transform.IsChildOf(s_npcRoot))
                    continue;
                return false;
            }
            return true;
        }

        // The scene's `npcs` container while a pass runs (see StandingRoom).
        static Transform s_npcRoot;

        // --- Stations -------------------------------------------------------------

        static Component MakeStation(Transform parent, string name, int kind,
            Vector3 standWorld, Vector3 faceWorld, bool indoors, float dwell,
            Component handler)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.position = standWorld;
            faceWorld.y = 0f;
            if (faceWorld.sqrMagnitude > 1e-6f)
                go.transform.rotation =
                    Quaternion.LookRotation(faceWorld.normalized, Vector3.up);
            var udon = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaNpcStation");
            LegaiaWorldBuilder.SetUdonField(udon, "kind", kind);
            LegaiaWorldBuilder.SetUdonField(udon, "standPoint", go.transform);
            LegaiaWorldBuilder.SetUdonField(udon, "indoors", indoors);
            LegaiaWorldBuilder.SetUdonField(udon, "dwellSeconds", dwell);
            LegaiaWorldBuilder.SetUdonField(udon, "available", true);
            if (handler != null)
                LegaiaWorldBuilder.SetUdonField(udon, "handler", handler);
            LegaiaWorldBuilder.SyncUdonProxy(udon);
            return udon;
        }

        /// One use-prop station in front of every one-shot, non-door prop
        /// (cupboards, drawers, the shop's upstairs door). The handler is
        /// the prop's own LegaiaDoor, so standing there opens it.
        static int BuildPropStations(GameObject root, object manifest,
            Transform parent, Vector3 spawn, LegaiaLivingTownOptions o)
        {
            var propRoot = root.transform.Find("props");
            if (propRoot == null)
                return 0;
            var doorType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDoor");
            int made = 0, idx = 0;
            foreach (object p in MiniJson.AsList(MiniJson.Get(manifest, "animated_props"))
                     ?? new List<object>())
            {
                bool oneShot = MiniJson.Get(p, "cyclic") is bool cyc && !cyc;
                if (!oneShot)
                    continue; // free-running props (windmills) are not usable
                foreach (object inst in MiniJson.AsList(MiniJson.Get(p, "instances"))
                         ?? new List<object>())
                {
                    idx++;
                    // Doors and gate leaves belong to the player's own
                    // approach behaviour, not to a villager's errand.
                    if (MiniJson.Get(inst, "is_door") is bool db && db)
                        continue;
                    if (MiniJson.Get(inst, "near_portal") is double)
                        continue;
                    Vector3 local = LegaiaWorldBuilder.G2U(
                        MiniJson.GetVec3(inst, "position"));
                    Transform prop = FindAt(propRoot, local, true);
                    if (prop == null)
                        continue;
                    Component handler = null;
                    if (doorType != null)
                    {
                        // The approach trigger is an unscaled sibling parked
                        // at the prop's own position (see AttachProximityDoor).
                        foreach (Transform sib in propRoot)
                        {
                            if ((sib.localPosition - local).sqrMagnitude > 1e-3f)
                                continue;
                            var d = sib.GetComponent(doorType);
                            if (d != null)
                            {
                                handler = d;
                                break;
                            }
                        }
                    }
                    Vector3 stand, face;
                    if (!StandSpotFor(prop, o.propStandDistance, out stand, out face))
                        continue;
                    MakeStation(parent, "station_prop_" + idx, 0, stand, face,
                        IsInterior(local, spawn, o), 11f, handler);
                    made++;
                }
            }
            return made;
        }

        /// Where a villager stands to use `prop`: the most open direction
        /// around it, nudged toward the side the prop's own front faces
        /// (measured through the transform chain, mirrors included - the
        /// leaf swings out of the face it presents). Fails when nothing
        /// around the prop has both floor and standing room.
        ///
        /// The anchor is the prop INSTANCE ORIGIN, not its render bounds:
        /// these meshes hang below their origin (a cupboard's box reaches
        /// 1.3 m under it, the shop door's 3.3 m), because retail parks the
        /// origin on the prop's floor tile and models upward from there. A
        /// bounds-centre anchor put the probe rays under the floor, and
        /// every cupboard in town01 was skipped for "no clearance".
        static bool StandSpotFor(Transform prop, float dist,
            out Vector3 stand, out Vector3 face)
        {
            stand = Vector3.zero;
            face = Vector3.forward;
            Vector3 anchor = prop.position;
            Vector3 grounded;
            if (HasFloor(anchor, out grounded))
                anchor = grounded;
            float h = 1f;
            var rends = prop.GetComponentsInChildren<Renderer>(true);
            if (rends.Length > 0)
            {
                Bounds wb = rends[0].bounds;
                for (int i = 1; i < rends.Length; i++)
                    wb.Encapsulate(rends[i].bounds);
                h = Mathf.Clamp(wb.size.y, 0.4f, 3f);
            }
            Vector3 eye = anchor + Vector3.up * Mathf.Clamp(h * 0.4f, 0.35f, 1.1f);
            Vector3 front = prop.TransformPoint(Vector3.forward)
                - prop.TransformPoint(Vector3.zero);
            front.y = 0f;
            front = front.sqrMagnitude > 1e-6f ? front.normalized : Vector3.forward;

            // Two stand distances: the authored one, then closer in - a
            // cupboard in a small room may have less than 0.7 m of floor.
            for (int pass = 0; pass < 2; pass++)
            {
                float d = pass == 0 ? dist : dist * 0.7f;
                float bestScore = -1f;
                Vector3 bestStand = Vector3.zero, bestDir = Vector3.forward;
                for (int k = 0; k < 12; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 30f, Vector3.up) * front;
                    RaycastHit hit;
                    float clear = 3f;
                    if (Physics.Raycast(eye, dir, out hit, 3f, ~0,
                            QueryTriggerInteraction.Ignore))
                        clear = hit.distance;
                    if (clear < d * 0.6f)
                        continue;
                    Vector3 floor;
                    if (!HasFloor(anchor + dir * d, out floor)
                            || !StandingRoom(floor))
                        continue;
                    float score = clear + 0.4f * Vector3.Dot(dir, front);
                    if (score > bestScore)
                    {
                        bestScore = score;
                        bestStand = floor;
                        bestDir = dir;
                    }
                }
                if (bestScore < 0f)
                    continue;
                stand = bestStand;
                // Stand facing the prop, not away from it.
                face = anchor - bestStand;
                return true;
            }
            return false;
        }

        /// Conversation rings: open, floor-checked spots in the village near
        /// where villagers already stand, each with three stand points
        /// around it (a ring, so a group of three is a plain station claim).
        static int BuildChatRings(GameObject root, object manifest, Transform parent,
            LegaiaSceneSettings settings, Vector3 spawn, LegaiaLivingTownOptions o)
        {
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return 0;
            var pts = new List<Vector3>();
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                if (settings.NpcIsRemoved(file))
                    continue;
                Vector3 local = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                Transform placed = FindAt(npcRoot, local, false);
                if (placed != null)
                    pts.Add(placed.position);
            }
            // Greedy clustering: the point with the most neighbours seeds a
            // spot, its cluster is consumed, repeat.
            int made = 0, spot = 0;
            var used = new bool[pts.Count];
            for (int iter = 0; iter < o.maxChatSpots; iter++)
            {
                int best = -1, bestN = 1;
                for (int i = 0; i < pts.Count; i++)
                {
                    if (used[i])
                        continue;
                    int n = 0;
                    for (int j = 0; j < pts.Count; j++)
                        if (!used[j] && Vector3.Distance(pts[i], pts[j]) < 7f)
                            n++;
                    if (n > bestN)
                    {
                        bestN = n;
                        best = i;
                    }
                }
                if (best < 0)
                    break;
                Vector3 centre = Vector3.zero;
                int count = 0;
                for (int j = 0; j < pts.Count; j++)
                    if (!used[j] && Vector3.Distance(pts[best], pts[j]) < 7f)
                    {
                        centre += pts[j];
                        used[j] = true;
                        count++;
                    }
                centre /= count;
                Vector3 centreFloor;
                if (!HasFloor(centre, out centreFloor))
                    continue;
                bool inside = IsInterior(
                    root.transform.InverseTransformPoint(centreFloor), spawn, o);
                int ringMade = 0;
                for (int k = 0; k < 3; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 120f + spot * 37f,
                        Vector3.up) * Vector3.forward;
                    Vector3 floor;
                    Vector3 cand = centreFloor + dir * o.chatRingRadius;
                    if (!HasFloor(cand, out floor) || !StandingRoom(floor))
                        continue;
                    MakeStation(parent, "station_chat_" + spot + "_" + k, 3,
                        floor, centreFloor - floor, inside, 20f, null);
                    ringMade++;
                }
                if (ringMade >= 2)
                {
                    made += ringMade;
                    spot++;
                }
                else
                {
                    // A ring of one is no meeting: drop what was made.
                    for (int k = parent.childCount - 1; k >= 0; k--)
                        if (parent.GetChild(k).name.StartsWith("station_chat_" + spot + "_"))
                            Undo.DestroyObjectImmediate(parent.GetChild(k).gameObject);
                }
            }
            return made;
        }

        /// Open-air places to stand and look. town01's usable props all
        /// sit INSIDE the houses (the four cupboards, the drawer, the
        /// shop's upstairs door), so without these a daytime villager has
        /// nothing but conversations to do - the fishing spots and seats
        /// other passes build land in the same picker and fill this out.
        /// Sampled on a spiral around the manifest spawn and kept only
        /// where there is real floor, standing room, and daylight between
        /// this and every station already built.
        static int BuildOutdoorViewpoints(GameObject root, Transform parent,
            Vector3 spawn, LegaiaLivingTownOptions o)
        {
            if (o.outdoorViewpoints <= 0)
                return 0;
            Vector3 centre = root.transform.TransformPoint(spawn);
            var taken = new List<Vector3>();
            foreach (Transform t in parent)
                taken.Add(t.position);
            int made = 0;
            for (int ring = 0; ring < 5 && made < o.outdoorViewpoints; ring++)
            {
                float r = 5f + ring * 4.5f;
                for (int k = 0; k < 8 && made < o.outdoorViewpoints; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 45f + ring * 21f,
                        Vector3.up) * Vector3.forward;
                    Vector3 cand = centre + dir * r;
                    Vector3 floor;
                    if (!HasFloor(cand, out floor) || !StandingRoom(floor))
                        continue;
                    if (IsInterior(root.transform.InverseTransformPoint(floor),
                            spawn, o))
                        continue;
                    bool crowded = false;
                    for (int i = 0; i < taken.Count; i++)
                        if (Vector3.Distance(taken[i], floor) < 4f)
                        {
                            crowded = true;
                            break;
                        }
                    if (crowded)
                        continue;
                    MakeStation(parent, "station_view_out_" + made, 4, floor,
                        floor - centre, false, 14f, null);
                    taken.Add(floor);
                    made++;
                }
            }
            return made;
        }

        /// Somewhere for a villager who went inside to stand: a couple of
        /// viewpoints per interior landing.
        static int BuildIndoorViewpoints(GameObject root, Transform parent,
            List<Home> homes, LegaiaLivingTownOptions o)
        {
            int made = 0;
            for (int i = 0; i < homes.Count; i++)
            {
                Transform landing = homes[i].landingT;
                if (landing == null)
                    continue;
                int here = 0;
                for (int k = 0; k < 8 && here < 2; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 45f + i * 23f,
                        Vector3.up) * landing.forward;
                    Vector3 cand = landing.position + dir * (1.1f + 0.4f * here);
                    Vector3 floor;
                    if (!HasFloor(cand, out floor) || !StandingRoom(floor))
                        continue;
                    MakeStation(parent, "station_view_" + i + "_" + here, 4,
                        floor, landing.position - floor, true, 18f, null);
                    here++;
                    made++;
                }
            }
            return made;
        }

        // --- Villagers -------------------------------------------------------------

        static Transform FindAt(Transform parent, Vector3 local, bool skipTriggers)
        {
            foreach (Transform child in parent)
            {
                if ((child.localPosition - local).sqrMagnitude > 1e-3f)
                    continue;
                if (skipTriggers && child.name.EndsWith("_approach"))
                    continue;
                return child;
            }
            return null;
        }

        static List<Component> WireBrains(GameObject root, object manifest,
            string sceneName, string genDir, LegaiaSceneSettings settings,
            LegaiaLivingTownOptions o, List<Home> homes)
        {
            var brains = new List<Component>();
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return brains;
            string dir = ManifestDir(sceneName, manifest);
            Material[] icons = o.speechBubbles
                ? LegaiaBubbleArt.IconMaterials(genDir) : null;
            Mesh quad = o.speechBubbles ? LegaiaBubbleArt.QuadMesh(genDir) : null;

            // Eligible villagers, in manifest order (deterministic).
            var files = new List<string>();
            var objs = new List<Transform>();
            var labels = new List<string>();
            var idles = new List<string>();
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                if (settings.NpcIsRemoved(file) || settings.NpcIsStatic(file)
                    || settings.NpcIsFrozen(file))
                    continue;
                Vector3 local = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                Transform placed = FindAt(npcRoot, local, false);
                if (placed == null)
                    continue;
                files.Add(file);
                objs.Add(placed);
                labels.Add(MiniJson.AsStr(MiniJson.Get(n, "label")) ?? "");
                idles.Add(FirstClip(n));
            }

            var order = SeededOrder(files.Count, o.seed);
            var homeOf = new int[files.Count];
            for (int i = 0; i < homeOf.Length; i++)
                homeOf[i] = -1;
            // Round-robin the shuffled villagers over the houses, capped.
            if (homes.Count > 0 && o.homeCap > 0)
            {
                int h = 0;
                for (int k = 0; k < order.Length; k++)
                {
                    int idx = order[k];
                    string token = Path.GetFileNameWithoutExtension(files[idx]);
                    int forced = settings.HomeOverride(token);
                    if (forced >= 0 && forced < homes.Count)
                    {
                        homeOf[idx] = forced;
                        homes[forced].occupants++;
                        continue;
                    }
                    for (int tries = 0; tries < homes.Count; tries++)
                    {
                        int cand = (h + tries) % homes.Count;
                        if (homes[cand].occupants >= o.homeCap)
                            continue;
                        homeOf[idx] = cand;
                        homes[cand].occupants++;
                        h = cand + 1;
                        break;
                    }
                }
            }

            var walkWired = new HashSet<string>();
            int walked = 0;
            int indoorsWanted = Mathf.RoundToInt(files.Count *
                Mathf.Clamp01(o.daytimeIndoorsShare));
            int indoorsMade = 0;
            for (int k = 0; k < order.Length; k++)
            {
                int i = order[k];
                Transform npc = objs[i];
                int seed = StableSeed(files[i], o.seed);

                // The locomotion controller: the wander pass normally wired
                // it already; wire it here when that pass is off, so the
                // living town never depends on the order of the two.
                var loco = EnsureLoco(npc.gameObject, o);
                // Bind the measured walk cycle where this rig family has
                // one - after the controller exists, so its animator field
                // is set on a component that is certainly there.
                if (o.walkAnimator && walkWired.Add(files[i]))
                    if (WireWalkAnimator(npc.gameObject, dir + "/" + files[i],
                            idles[i], o.walkClip, genDir))
                        walked++;

                var brain = LegaiaWorldBuilder.TryAttachUdon(
                    npc.gameObject, "LegaiaNpcBrain");
                if (brain == null)
                    continue;
                Component bubble = o.speechBubbles
                    ? BuildBubble(npc, icons, quad, o) : null;
                bool dayIn = homeOf[i] >= 0 && indoorsMade < indoorsWanted;
                if (dayIn)
                    indoorsMade++;

                LegaiaWorldBuilder.SetUdonField(brain, "loco", loco);
                LegaiaWorldBuilder.SetUdonField(brain, "bubble", bubble);
                LegaiaWorldBuilder.SetUdonField(brain, "seed", seed);
                LegaiaWorldBuilder.SetUdonField(brain, "daytimeIndoors", dayIn);
                LegaiaWorldBuilder.SetUdonField(brain, "firstLine",
                    o.bubbleText ? Shorten(labels[i]) : "");
                if (homeOf[i] >= 0)
                {
                    Home home = homes[homeOf[i]];
                    LegaiaWorldBuilder.SetUdonField(brain, "homeDoor", home.doorT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeLanding", home.landingT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeExit", home.exitT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeEmerge", home.emergeT);
                }
                LegaiaWorldBuilder.SyncUdonProxy(brain);
                brains.Add(brain);
            }
            if (o.walkAnimator)
                Debug.Log("[Legaia] living town: walk cycle '" + o.walkClip +
                    "' bound on " + walked + " of " + files.Count +
                    " villager(s) (the rigs whose family carries one).");
            return brains;
        }

        /// The NPC's locomotion controller, wired if the wander pass did not
        /// (its facing overrides and radius are that pass's business).
        static Component EnsureLoco(GameObject npc, LegaiaLivingTownOptions o)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcWander");
            Component loco = t != null ? npc.GetComponent(t) : null;
            if (loco == null)
            {
                loco = LegaiaWorldBuilder.TryAttachUdon(npc, "LegaiaNpcWander");
                if (loco == null)
                    return null;
            }
            LegaiaWorldBuilder.SetUdonField(loco, "walkSpeed", o.walkSpeed);
            LegaiaWorldBuilder.SyncUdonProxy(loco);
            return loco;
        }

        static string Shorten(string label)
        {
            if (string.IsNullOrEmpty(label))
                return "";
            label = label.Trim();
            return label.Length <= 26 ? label : label.Substring(0, 24) + "...";
        }

        /// The bubble rig: an always-active holder (a U# call into a
        /// DISABLED behaviour never runs, so the behaviour's own object must
        /// stay on) carrying a `visual` child with one quad per icon.
        static Component BuildBubble(Transform npc, Material[] icons, Mesh quad,
            LegaiaLivingTownOptions o)
        {
            float h = 1f;
            var rends = npc.GetComponentsInChildren<Renderer>(true);
            if (rends.Length > 0)
            {
                Bounds wb = rends[0].bounds;
                for (int i = 1; i < rends.Length; i++)
                    wb.Encapsulate(rends[i].bounds);
                h = Mathf.Clamp(wb.size.y, 0.3f, 2.5f);
            }
            var go = new GameObject("speech_bubble");
            go.transform.SetParent(npc, false);
            go.transform.position = npc.position + Vector3.up * (h * 1.18f + 0.12f);
            go.transform.rotation = Quaternion.identity;
            // Cancel the parent chain's mirrors so the quad's world scale is
            // positive and uniform: with a mirrored scale the billboard
            // would render the text back to front and no rotation fixes it.
            Vector3 ls = npc.lossyScale;
            float s = Mathf.Clamp(h * 0.62f, 0.3f, 1.1f);
            go.transform.localScale = new Vector3(
                s / NonZero(ls.x), s / NonZero(ls.y), s / NonZero(ls.z));

            var visual = new GameObject("visual");
            visual.transform.SetParent(go.transform, false);
            var iconObjs = new GameObject[LegaiaBubbleArt.ICONS];
            for (int i = 0; i < LegaiaBubbleArt.ICONS; i++)
            {
                var q = new GameObject("icon_" + LegaiaBubbleArt.ICON_NAMES[i]);
                q.transform.SetParent(visual.transform, false);
                q.AddComponent<MeshFilter>().sharedMesh = quad;
                var mr = q.AddComponent<MeshRenderer>();
                mr.sharedMaterial = icons[i];
                mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
                mr.receiveShadows = false;
                q.SetActive(false);
                iconObjs[i] = q;
            }
            TMPro.TextMeshPro label = null;
            if (o.bubbleText)
            {
                var lg = new GameObject("label");
                lg.transform.SetParent(visual.transform, false);
                lg.transform.localPosition = new Vector3(0f, -0.62f, -0.01f);
                label = lg.AddComponent<TMPro.TextMeshPro>();
                label.alignment = TMPro.TextAlignmentOptions.Center;
                label.fontSize = 1.15f;
                label.enableWordWrapping = false;
                label.color = new Color(0.96f, 0.94f, 0.85f);
                label.text = "";
                label.rectTransform.sizeDelta = new Vector2(6.5f, 0.4f);
                var lmr = lg.GetComponent<MeshRenderer>();
                if (lmr != null)
                    lmr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            }
            visual.SetActive(false);

            var udon = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaSpeechBubble");
            LegaiaWorldBuilder.SetUdonField(udon, "icons", iconObjs);
            LegaiaWorldBuilder.SetUdonField(udon, "visual", visual);
            if (label != null)
                LegaiaWorldBuilder.SetUdonField(udon, "label", label);
            LegaiaWorldBuilder.SyncUdonProxy(udon);
            return udon;
        }

        static float NonZero(float v)
        {
            return Mathf.Abs(v) < 1e-4f ? 1f : v;
        }

        // --- Director ----------------------------------------------------------------

        static Component BuildDirector(GameObject root, Transform container,
            List<Component> brains, LegaiaLivingTownOptions o)
        {
            var go = new GameObject("director");
            go.transform.SetParent(container, false);
            var udon = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaTownDirector");
            if (udon == null)
                return null;

            // Every station in the world: the built root plus the kit's own
            // top-level containers (another pass's fishing spots and seats
            // live there), scoped rather than a scene-wide sweep.
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            var stations = new List<Component>();
            var extraRoots = new List<Transform>();
            var scopes = new List<GameObject> { root };
            foreach (string top in new[]
                     { LegaiaCommonPrefabs.CONTAINER, "Legaia_camp_props" })
            {
                var g = GameObject.Find(top);
                if (g == null)
                    continue;
                scopes.Add(g);
                extraRoots.Add(g.transform);
            }
            if (stationType != null)
                foreach (var scope in scopes)
                    foreach (var s in scope.GetComponentsInChildren(stationType, true))
                        if (!stations.Contains(s))
                            stations.Add(s);

            LegaiaWorldBuilder.SetUdonField(udon, "stations",
                Typed(stations, "LegaiaNpcStation"));
            LegaiaWorldBuilder.SetUdonField(udon, "brains",
                Typed(brains, "LegaiaNpcBrain"));
            LegaiaWorldBuilder.SetUdonField(udon, "extraStationRoots",
                extraRoots.ToArray());
            LegaiaWorldBuilder.SetUdonField(udon, "seed", o.seed);
            // The director groups a ring by proximity; the threshold must
            // clear the ring's own chord (r * sqrt(3)) without swallowing a
            // neighbouring spot.
            LegaiaWorldBuilder.SetUdonField(udon, "ringRadius",
                Mathf.Max(2.5f, o.chatRingRadius * 2.4f));

            var dnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
            if (dnType != null)
            {
                var dn = Object.FindObjectOfType(dnType, true) as Component;
                if (dn != null)
                    LegaiaWorldBuilder.SetUdonField(udon, "dayNight", dn);
            }
            // Weather is optional and loosely typed: any behaviour with a
            // public float `rainIntensity`. Whatever a weather pass named
            // its class, this finds it without a compile-time reference.
            var weather = FindWeather();
            if (weather != null)
                LegaiaWorldBuilder.SetUdonField(udon, "weather", weather);

            LegaiaWorldBuilder.SyncUdonProxy(udon);
            return udon;
        }

        static Component FindWeather()
        {
            foreach (string name in new[]
                     { "LegaiaWorld.LegaiaWeather", "LegaiaWorld.LegaiaRain",
                       "LegaiaWorld.LegaiaSkyWeather" })
            {
                var t = LegaiaWorldBuilder.FindType(name);
                if (t == null)
                    continue;
                var c = Object.FindObjectOfType(t, true) as Component;
                if (c != null && c.GetType().GetField("rainIntensity") != null)
                    return c;
            }
            return null;
        }

        /// A typed `T[]` for an Udon array field: an object[] would not
        /// deserialize onto the backing behaviour's variable.
        static System.Array Typed(List<Component> comps, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + typeName);
            if (t == null)
                return null;
            var arr = System.Array.CreateInstance(t, comps.Count);
            for (int i = 0; i < comps.Count; i++)
                arr.SetValue(comps[i], i);
            return arr;
        }

        // --- Walk animator --------------------------------------------------------------

        static string FirstClip(object npc)
        {
            var clips = MiniJson.AsList(MiniJson.Get(npc, "clips"));
            return clips != null && clips.Count > 0 ? MiniJson.AsStr(clips[0]) : null;
        }

        /// Where the manifest (and therefore the glbs) live. The builder
        /// keeps no back-reference, so recover it from the imported world
        /// glb's asset path.
        static string ManifestDir(string sceneName, object manifest)
        {
            string world = MiniJson.AsStr(MiniJson.Get(manifest, "world_glb"));
            if (!string.IsNullOrEmpty(world))
                foreach (string guid in AssetDatabase.FindAssets(
                             Path.GetFileNameWithoutExtension(world)))
                {
                    string p = AssetDatabase.GUIDToAssetPath(guid);
                    if (p.EndsWith("/" + world))
                        return p.Substring(0, p.Length - world.Length - 1);
                }
            return "Assets/LegaiaImports/" + sceneName;
        }

        /// Two-state idle/walk Animator for the rigs that carry a measured
        /// walk cycle. The walk clip is a rig-FAMILY property (the humanoid
        /// family's `record_36`: legs anti-phase at exactly half a period,
        /// arms contralateral, head and torso amplitude zero, body centroid
        /// fixed in x - a stride in place); rigs without it keep their
        /// single looping spawn clip, which is why this returns quietly.
        static bool WireWalkAnimator(GameObject npc, string glbPath,
            string idleClipName, string walkClipName, string genDir)
        {
            if (string.IsNullOrEmpty(idleClipName) || string.IsNullOrEmpty(walkClipName))
                return false;
            var clips = AssetDatabase.LoadAllAssetsAtPath(glbPath)
                .OfType<AnimationClip>()
                .Where(c => !c.name.StartsWith("__preview"))
                .ToList();
            if (clips.Count == 0)
                return false;
            AnimationClip idle = null, walk = null;
            foreach (var c in clips)
            {
                if (c.name == idleClipName)
                    idle = c;
                if (c.name == walkClipName || c.name.EndsWith("_" + walkClipName))
                    walk = c;
            }
            if (idle == null || walk == null)
                return false;

            string stem = LegaiaWorldBuilder.Sanitize(
                Path.GetFileNameWithoutExtension(glbPath));
            AnimationClip idleAsset = LoopedCopy(idle, genDir + "/" + stem + "_idle.anim");
            AnimationClip walkAsset = LoopedCopy(walk, genDir + "/" + stem + "_walk.anim");

            string ctrlPath = genDir + "/" + stem + "_loco.controller";
            var ctrl = AssetDatabase.LoadAssetAtPath<AnimatorController>(ctrlPath);
            if (ctrl == null)
            {
                ctrl = AnimatorController.CreateAnimatorControllerAtPath(ctrlPath);
                var sm = ctrl.layers[0].stateMachine;
                var idleState = sm.AddState("idle");
                idleState.motion = idleAsset;
                var walkState = sm.AddState("walk");
                walkState.motion = walkAsset;
                sm.defaultState = idleState;
            }
            var animator = npc.GetComponentInChildren<Animator>();
            if (animator == null)
                animator = npc.AddComponent<Animator>();
            animator.runtimeAnimatorController = ctrl;

            var loco = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcWander");
            var comp = loco != null ? npc.GetComponent(loco) : null;
            if (comp != null)
            {
                LegaiaWorldBuilder.SetUdonField(comp, "locoAnimator", animator);
                LegaiaWorldBuilder.SetUdonField(comp, "idleState", "idle");
                LegaiaWorldBuilder.SetUdonField(comp, "walkState", "walk");
                LegaiaWorldBuilder.SyncUdonProxy(comp);
            }
            return true;
        }

        static AnimationClip LoopedCopy(AnimationClip src, string path)
        {
            var existing = AssetDatabase.LoadAssetAtPath<AnimationClip>(path);
            if (existing != null)
                return existing;
            var copy = Object.Instantiate(src);
            var s = AnimationUtility.GetAnimationClipSettings(copy);
            s.loopTime = true;
            AnimationUtility.SetAnimationClipSettings(copy, s);
            AssetDatabase.CreateAsset(copy, path);
            return copy;
        }

        // --- Determinism -------------------------------------------------------------

        /// A seeded permutation of 0..n-1 (Fisher-Yates on a small LCG), so
        /// home assignment is the same on every client and every rebuild.
        static int[] SeededOrder(int n, int seed)
        {
            var order = new int[n];
            for (int i = 0; i < n; i++)
                order[i] = i;
            int state = seed == 0 ? 1 : seed;
            for (int i = n - 1; i > 0; i--)
            {
                state = unchecked(state * 1103515245 + 12345);
                int j = (state & 0x7FFFFFFF) % (i + 1);
                int tmp = order[i];
                order[i] = order[j];
                order[j] = tmp;
            }
            return order;
        }

        /// Per-NPC personality seed: stable across rebuilds because it is a
        /// hash of the exported file stem, not an index.
        static int StableSeed(string file, int seed)
        {
            int h = seed == 0 ? 17 : seed;
            string stem = Path.GetFileNameWithoutExtension(file);
            for (int i = 0; i < stem.Length; i++)
                h = unchecked(h * 31 + stem[i]);
            return h & 0x7FFFFFFF;
        }
    }
}
