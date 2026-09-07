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
        public int outdoorViewpoints = 10;
        [Tooltip("Stand spots along the village paths, further out than the " +
                 "viewpoint ring and spread over the whole walkable area.")]
        public int landmarkStands = 10;
        [Tooltip("A stand spot beside each house door, so a villager can call " +
                 "on a neighbour (it stands a step to the side and knocks).")]
        public bool doorwayStands = true;
        [Tooltip("A stand spot in front of each of the village's fixed " +
                 "residents, so walking villagers go and see them.")]
        public bool visitSpots = true;
        [Tooltip("Give villagers things to carry: a bucket fetched from the " +
                 "low ground, a broom at a doorway, firewood between two points.")]
        public bool carryItems = true;
        [Tooltip("Conversation spots inside the interior rooms, one per home " +
                 "landing, so two villagers in one room talk to each other.")]
        public bool indoorChatRings = true;
        [Tooltip("Stand spots per interior landing.")]
        public int indoorViewpoints = 3;
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
        [Tooltip("Bake a navmesh from the world's colliders so commanded walks follow walkable routes.")]
        public bool navMesh = true;
        [Tooltip("Navmesh agent radius (meters) - a villager's, not a player's.")]
        public float navAgentRadius = 0.2f;
        [Tooltip("Navmesh agent height (meters).")]
        public float navAgentHeight = 0.8f;
        [Tooltip("Highest step a villager climbs without a ramp (meters).")]
        public float navStepHeight = 0.3f;
        [Tooltip("Steepest walkable slope (degrees).")]
        public float navMaxSlope = 40f;
        [Tooltip("How far in front of the doorway tile the villager stops to open the door (meters).")]
        public float doorStandDistance = 0.8f;

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
            public Vector3 door;       // root-local: the doorway tile (teleport trigger)
            public Vector3 landing;
            public Vector3 landingFace; // root-local direction
            public bool hasExit;
            public Vector3 exit;
            public Vector3 emerge;
            public Vector3 emergeFace;
            public int occupants;
            // doorT = the stand spot in front of the tile, thresholdT = the
            // tile itself; doorProp = the LegaiaDoor swung on the way through.
            public Transform doorT, thresholdT, landingT, exitT, emergeT;
            public Component doorProp;
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

            // The navmesh first: every collider the villagers walk against
            // exists by now (this pass runs last), and the home markers
            // below are placed against the same floors it is baked from.
            GameObject nav = o.navMesh ? LegaiaNavMesh.Apply(root, sceneName, o) : null;

            s_npcRoot = root.transform.Find("npcs");
            s_doorStand = Mathf.Max(0.3f, o.doorStandDistance);
            Vector3 spawn = LegaiaWorldBuilder.G2U(
                MiniJson.GetVec3(MiniJson.Get(manifest, "spawn"), "position"));

            var homes = ReadHomes(manifest, spawn, o);
            var homesRoot = new GameObject("homes");
            homesRoot.transform.SetParent(container.transform, false);
            for (int i = 0; i < homes.Count; i++)
                MakeHomeMarkers(root.transform, homesRoot.transform, homes[i], i);

            var stationsRoot = new GameObject("stations");
            stationsRoot.transform.SetParent(container.transform, false);

            // The bake goes live for the WHOLE of the station + brain build,
            // not only for home assignment: an outdoor stand spot no
            // villager can walk to is not an activity, it is a villager
            // walking at a bank until its walk times out (town01's beach
            // sits 1.2 m below the village in the collider and nothing
            // connects the two). s_reach below is what rejects those.
            var navData = nav != null ? LegaiaNavMesh.LoadData(sceneName) : null;
            var navInst = navData != null
                ? LegaiaNavMesh.Register(navData) : new UnityEngine.AI.NavMeshDataInstance();
            int propStations, chatStations, viewStations, carryStations, visitStations;
            List<Component> brains;
            try
            {
                s_reachFrom = navData != null
                    ? ReachAnchors(root, manifest, settings, spawn) : null;
                propStations = BuildPropStations(root, manifest, stationsRoot.transform,
                    spawn, o);
                chatStations = BuildChatRings(root, manifest, stationsRoot.transform,
                    settings, spawn, o);
                chatStations += EnsureOutdoorRing(root, stationsRoot.transform,
                    spawn, o);
                if (o.indoorChatRings)
                    chatStations += BuildIndoorChatRings(root, stationsRoot.transform,
                        homes, o);
                viewStations = BuildIndoorViewpoints(root, stationsRoot.transform,
                    homes, o);
                viewStations += BuildOutdoorViewpoints(root, stationsRoot.transform,
                    spawn, o);
                viewStations += BuildLandmarkStands(root, stationsRoot.transform,
                    spawn, o);
                carryStations = o.doorwayStands
                    ? BuildDoorwayStands(root, stationsRoot.transform, homes, o) : 0;
                visitStations = o.visitSpots
                    ? BuildVisitSpots(root, manifest, sceneName, genDir, settings,
                        stationsRoot.transform, spawn, o) : 0;
                carryStations += AssignErrandRoles(stationsRoot.transform, root, spawn, o);
                brains = WireBrains(root, manifest, sceneName, genDir, settings, o, homes,
                    navData != null);
            }
            finally
            {
                s_reachFrom = null;
                if (navData != null)
                    UnityEngine.AI.NavMesh.RemoveNavMeshData(navInst);
            }

            var director = BuildDirector(root, container.transform, brains, o);
            // The back-reference: brains are built before the director (it
            // needs their array), so the link is closed here.
            if (director != null)
                foreach (var brain in brains)
                {
                    LegaiaWorldBuilder.SetUdonField(brain, "director", director);
                    LegaiaWorldBuilder.SyncUdonProxy(brain);
                }

            int doorProps = 0;
            foreach (var h in homes)
                if (h.doorProp != null)
                    doorProps++;
            Debug.Log("[Legaia] living town: " + brains.Count + " villager(s), " +
                homes.Count + " home(s) (cap " + o.homeCap + ", " + doorProps +
                " with a door prop to swing), " +
                propStations + " prop station(s), " + chatStations +
                " chat stand point(s), " + viewStations + " viewpoint(s), " +
                carryStations + " carry/errand endpoint(s), " + visitStations +
                " visit spot(s)" +
                (nav != null ? ", navmesh baked" : ", no navmesh") + ".");
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
            LegaiaNavMesh.Remove(root);
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return;
            foreach (Transform npc in npcRoot)
            {
                StripUdon(npc.gameObject, "LegaiaNpcBrain");
                StripUdon(npc.gameObject, "LegaiaNpcCarry");
                var held = npc.Find("carry");
                if (held != null)
                    Undo.DestroyObjectImmediate(held.gameObject);
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
            // The doorway tile (retail's teleport trigger, which the door
            // prop's origin also sits on) and, a step out from it on the
            // open side, the spot the villager stands on to open the door.
            h.thresholdT = Marker(root, go.transform, "threshold", h.door,
                h.landing - h.door);
            Vector3 tileW = h.thresholdT.position;
            Vector3 standW = DoorStandSpot(tileW, s_doorStand);
            var stand = new GameObject("door");
            stand.transform.SetParent(go.transform, false);
            stand.transform.position = standW;
            Vector3 toTile = tileW - standW;
            toTile.y = 0f;
            if (toTile.sqrMagnitude > 1e-6f)
                stand.transform.rotation =
                    Quaternion.LookRotation(toTile.normalized, Vector3.up);
            h.doorT = stand.transform;
            h.doorProp = DoorPropNear(root, tileW);
            h.landingT = Marker(root, go.transform, "landing", h.landing, h.landingFace);
            if (h.hasExit)
            {
                h.exitT = Marker(root, go.transform, "exit", h.exit, h.exit - h.landing);
                h.emergeT = Marker(root, go.transform, "emerge", h.emerge, h.emergeFace);
            }
        }

        // Door stand distance for the pass in flight (MakeHomeMarkers has
        // no options parameter; see Apply).
        static float s_doorStand = 0.8f;

        /// Where a villager stands to open a front door: `dist` metres out
        /// from the doorway tile in the most open direction - the hut wall
        /// sits on one side of the tile, the village on the other, and the
        /// probe rays find which is which. Falls back to the tile itself
        /// when nothing around it has floor and standing room.
        static Vector3 DoorStandSpot(Vector3 tile, float dist)
        {
            Vector3 eye = tile + Vector3.up * 0.45f;
            float bestScore = -1f;
            Vector3 best = tile;
            for (int k = 0; k < 16; k++)
            {
                Vector3 dir = Quaternion.AngleAxis(k * 22.5f, Vector3.up) * Vector3.forward;
                RaycastHit hit;
                float clear = 4f;
                if (Physics.Raycast(eye, dir, out hit, 4f, ~0,
                        QueryTriggerInteraction.Ignore))
                    clear = hit.distance;
                if (clear < dist + 0.25f)
                    continue;
                Vector3 floor;
                if (!HasFloorNear(tile + dir * dist, out floor) || !StandingRoom(floor))
                    continue;
                // Level with the tile, or nearly: a step up is a porch, a
                // metre up is the roof over the porch.
                if (Mathf.Abs(floor.y - tile.y) > 0.35f)
                    continue;
                float score = clear - Mathf.Abs(floor.y - tile.y) * 2f;
                if (score > bestScore)
                {
                    bestScore = score;
                    best = floor;
                }
            }
            return best;
        }

        /// The LegaiaDoor (on the approach trigger the builder parks at a
        /// door prop's origin) nearest the doorway tile, within a couple of
        /// metres - the prop the villager swings on the way through. Null
        /// when the home has no door prop (a bare doorway).
        static Component DoorPropNear(Transform root, Vector3 tile)
        {
            var doorType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDoor");
            var propRoot = root.Find("props");
            if (doorType == null || propRoot == null)
                return null;
            Component best = null;
            float bestD = 2.5f;
            foreach (Transform sib in propRoot)
            {
                var d = sib.GetComponent(doorType);
                if (d == null)
                    continue;
                Vector3 p = sib.position;
                float dd = Vector2.Distance(new Vector2(p.x, p.z), new Vector2(tile.x, tile.z));
                if (dd < bestD)
                {
                    bestD = dd;
                    best = d;
                }
            }
            return best;
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
            go.transform.position = SnapFloorNear(root.TransformPoint(local));
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

        /// Floor snap for a position whose height is ALREADY authored
        /// (retail's teleport tiles and landings sit on their floor): the
        /// ray starts just above it, so a hut's eave or porch roof over a
        /// doorway - which the 3 m ray of SnapFloor lands on, putting the
        /// "door" on the roof - is never what it finds.
        static Vector3 SnapFloorNear(Vector3 world)
        {
            RaycastHit hit;
            if (Physics.Raycast(world + Vector3.up * 0.7f, Vector3.down, out hit, 2.2f,
                    ~0, QueryTriggerInteraction.Ignore))
                return hit.point + Vector3.up * 0.02f;
            return world;
        }

        static bool HasFloor(Vector3 world, out Vector3 floor)
        {
            return HasFloorFrom(world, 3f, 12f, out floor);
        }

        /// HasFloor with a low ray (see SnapFloorNear): for spots next to an
        /// authored floor position, under whatever roof hangs over it.
        static bool HasFloorNear(Vector3 world, out Vector3 floor)
        {
            return HasFloorFrom(world, 0.7f, 2.2f, out floor);
        }

        static bool HasFloorFrom(Vector3 world, float above, float range, out Vector3 floor)
        {
            RaycastHit hit;
            if (Physics.Raycast(world + Vector3.up * above, Vector3.down, out hit, range,
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

        // --- Reachability -----------------------------------------------------------
        // Every place a villager can START from: the spawn, and each
        // eligible villager's own authored position. A stand spot is worth
        // building only if SOMEBODY can walk to it - and "somebody" cannot
        // be the spawn alone, because town01's two beach villagers live on
        // an island of navmesh the village never reaches, and a spot only
        // they can use is exactly the spot they should be given.
        // Null while no bake is registered: then everything counts as
        // reachable and the pass behaves as it did before the bake existed.
        static List<Vector3> s_reachFrom;

        static List<Vector3> ReachAnchors(GameObject root, object manifest,
            LegaiaSceneSettings settings, Vector3 spawn)
        {
            var pts = new List<Vector3> { SnapFloor(root.transform.TransformPoint(spawn)) };
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return pts;
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
                if (placed != null)
                    pts.Add(placed.position);
            }
            return pts;
        }

        /// Can anybody actually walk to this spot?
        static bool Reachable(Vector3 stand)
        {
            if (s_reachFrom == null)
                return true;
            string why;
            for (int i = 0; i < s_reachFrom.Count; i++)
                if (LegaiaNavMesh.Reachable(s_reachFrom[i], stand, 1.2f, out why))
                    return true;
            return false;
        }

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

        /// The village square: one conversation ring OUT OF DOORS, built
        /// only when the NPC-cluster pass produced none. It nearly always
        /// produces none in a village like town01, where eleven of the
        /// fifteen talkers were authored inside the houses, so every
        /// cluster it finds is a room - and an outdoor villager would then
        /// have no ring to be matchmade onto at all. The spot is the first
        /// open, reachable place near the spawn with room for three.
        static int EnsureOutdoorRing(GameObject root, Transform parent,
            Vector3 spawn, LegaiaLivingTownOptions o)
        {
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            if (stationType == null)
                return 0;
            foreach (Transform t in parent)
            {
                var st = t.GetComponent(stationType);
                if (st == null)
                    continue;
                if ((int)stationType.GetField("kind").GetValue(st) == 3
                    && !(bool)stationType.GetField("indoors").GetValue(st))
                    return 0; // the village already has one
            }
            Vector3 centre = root.transform.TransformPoint(spawn);
            for (int ring = 0; ring < 6; ring++)
            {
                float r = ring * 2.5f;
                for (int k = 0; k < 8; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 45f + ring * 17f,
                        Vector3.up) * Vector3.forward;
                    Vector3 centreFloor;
                    if (!HasFloor(centre + dir * r, out centreFloor))
                        continue;
                    if (IsInterior(root.transform.InverseTransformPoint(centreFloor),
                            spawn, o) || !Reachable(centreFloor))
                        continue;
                    int made = 0;
                    for (int i = 0; i < 3; i++)
                    {
                        Vector3 d = Quaternion.AngleAxis(i * 120f, Vector3.up)
                                    * Vector3.forward;
                        Vector3 floor;
                        Vector3 cand = centreFloor + d * o.chatRingRadius;
                        if (!HasFloor(cand, out floor) || !StandingRoom(floor)
                            || !Reachable(floor))
                            continue;
                        MakeStation(parent, "station_chat_square_" + i, 3, floor,
                            centreFloor - floor, false, 20f, null);
                        made++;
                    }
                    if (made >= 2)
                        return made;
                    for (int i = parent.childCount - 1; i >= 0; i--)
                        if (parent.GetChild(i).name.StartsWith("station_chat_square_"))
                            Undo.DestroyObjectImmediate(parent.GetChild(i).gameObject);
                }
            }
            return 0;
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
                    if (crowded || !Reachable(floor))
                        continue;
                    var st = MakeStation(parent, "station_view_out_" + made, 4, floor,
                        floor - centre, false, 14f, null);
                    Tag(st, -1, "view", true);
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
                int want = Mathf.Clamp(o.indoorViewpoints, 1, 6);
                int here = 0;
                for (int k = 0; k < 12 && here < want; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 30f + i * 23f,
                        Vector3.up) * landing.forward;
                    Vector3 cand = landing.position + dir * (1.1f + 0.35f * here);
                    Vector3 floor;
                    if (!HasFloor(cand, out floor) || !StandingRoom(floor))
                        continue;
                    var st = MakeStation(parent, "station_view_" + i + "_" + here, 4,
                        floor, landing.position - floor, true, 18f, null);
                    Tag(st, -1, "room", true);
                    here++;
                    made++;
                }
            }
            return made;
        }

        // --- Errand furniture -------------------------------------------------------
        // Everything below exists because of one measurement: town01 has
        // four villagers who can walk outdoors and NOT ONE usable prop out
        // of doors - every cupboard retail authored is inside a house. A
        // village whose only outdoor activity is "stand in a circle" reads
        // as aimless however good the walking is, so the pass builds the
        // outdoor half of the day itself: doorsteps to call at, paths to
        // walk, low ground to fetch water from, and the village's fixed
        // residents to go and see.

        /// Set the visitor-side fields on a station the builders make.
        static void Tag(Component station, int arriveIcon, string role, bool glance)
        {
            if (station == null)
                return;
            LegaiaWorldBuilder.SetUdonField(station, "arriveIcon", arriveIcon);
            LegaiaWorldBuilder.SetUdonField(station, "role", role);
            LegaiaWorldBuilder.SetUdonField(station, "glance", glance);
            LegaiaWorldBuilder.SyncUdonProxy(station);
        }

        /// Attach a carry handler (LegaiaNpcHandItem) to a station and turn
        /// it into a kind-5 errand endpoint.
        static Component MakeCarryHandler(Component station, int itemKind,
            bool dropItem, bool keepOnLeave, int action)
        {
            if (station == null)
                return null;
            var handler = LegaiaWorldBuilder.TryAttachUdon(
                station.gameObject, "LegaiaNpcHandItem");
            if (handler == null)
                return null;
            LegaiaWorldBuilder.SetUdonField(handler, "station", station);
            LegaiaWorldBuilder.SetUdonField(handler, "itemKind", itemKind);
            LegaiaWorldBuilder.SetUdonField(handler, "dropItem", dropItem);
            LegaiaWorldBuilder.SetUdonField(handler, "keepOnLeave", keepOnLeave);
            LegaiaWorldBuilder.SetUdonField(handler, "action", action);
            LegaiaWorldBuilder.SyncUdonProxy(handler);
            LegaiaWorldBuilder.SetUdonField(station, "kind", 5);
            LegaiaWorldBuilder.SetUdonField(station, "carryFlow",
                dropItem ? 2 : (itemKind >= 0 && keepOnLeave ? 1 : 0));
            LegaiaWorldBuilder.SetUdonField(station, "handler", handler);
            LegaiaWorldBuilder.SyncUdonProxy(station);
            return handler;
        }

        /// A stand spot beside each house door, facing the doorway: the
        /// "call on a neighbour" stop. It stands a step to the SIDE of the
        /// night routine's own door stand spot rather than on it, so a
        /// villager sweeping a doorstep at dusk is never parked in the way
        /// of the villager trying to get through that door.
        static int BuildDoorwayStands(GameObject root, Transform parent,
            List<Home> homes, LegaiaLivingTownOptions o)
        {
            int made = 0;
            for (int i = 0; i < homes.Count; i++)
            {
                Transform door = homes[i].doorT;
                Transform tile = homes[i].thresholdT;
                if (door == null || tile == null)
                    continue;
                Vector3 toTile = tile.position - door.position;
                toTile.y = 0f;
                if (toTile.sqrMagnitude < 1e-4f)
                    continue;
                toTile = toTile.normalized;
                Vector3 side = Vector3.Cross(Vector3.up, toTile);
                bool placed = false;
                for (int k = 0; k < 2 && !placed; k++)
                {
                    Vector3 cand = door.position + side * (k == 0 ? 0.7f : -0.7f);
                    Vector3 floor;
                    if (!HasFloorNear(cand, out floor) || !StandingRoom(floor))
                        continue;
                    if (Mathf.Abs(floor.y - door.position.y) > 0.35f)
                        continue;
                    if (!Reachable(floor))
                        continue;
                    // Half sweep the step (a broom changes hands, so those
                    // are kind 5 with a handler), half simply call at it - a
                    // kind-4 stand spot whose arriveIcon is the wave, which
                    // needs no behaviour at all.
                    bool sweep = o.carryItems && (i % 2) == 0;
                    var st = MakeStation(parent, "station_door_" + i, 4, floor,
                        tile.position - floor, false, sweep ? 14f : 9f, null);
                    if (sweep)
                    {
                        MakeCarryHandler(st, 1, false, false, 1);
                        Tag(st, LegaiaBubbleArt.WORK, "doorstep_sweep", false);
                    }
                    else
                    {
                        Tag(st, LegaiaBubbleArt.WAVE, "doorstep_call", false);
                    }
                    made++;
                    placed = true;
                }
            }
            return made;
        }

        /// A stand spot in front of every FIXED resident, and a speech
        /// bubble on the resident itself so the visit is an exchange rather
        /// than one villager talking at a statue. The resident never moves:
        /// where it faces is measured off its own transform chain (mirrors
        /// included - TransformDirection would drop them) and the caller is
        /// placed in front of that and turned back toward it.
        static int BuildVisitSpots(GameObject root, object manifest, string sceneName,
            string genDir, LegaiaSceneSettings settings, Transform parent,
            Vector3 spawn, LegaiaLivingTownOptions o)
        {
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return 0;
            Material[] icons = o.speechBubbles
                ? LegaiaBubbleArt.IconMaterials(genDir) : null;
            Mesh quad = o.speechBubbles ? LegaiaBubbleArt.QuadMesh(genDir) : null;
            int made = 0, idx = 0;
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                if (!settings.NpcIsStatic(file) || settings.NpcIsRemoved(file)
                    || settings.NpcIsFrozen(file))
                    continue;
                Vector3 local = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                Transform host = FindAt(npcRoot, local, false);
                if (host == null)
                    continue;
                idx++;
                Vector3 front = host.TransformPoint(Vector3.forward)
                    - host.TransformPoint(Vector3.zero);
                front.y = 0f;
                if (front.sqrMagnitude < 1e-6f)
                    front = Vector3.forward;
                else
                    front = front.normalized;

                // In front first; if the resident stands against a wall,
                // try around it rather than skipping the visit.
                Vector3 stand = Vector3.zero;
                bool found = false;
                for (int k = 0; k < 8 && !found; k++)
                {
                    float ang = (k + 1) / 2 * 45f * ((k % 2) == 0 ? 1f : -1f);
                    Vector3 dir = Quaternion.AngleAxis(ang, Vector3.up) * front;
                    Vector3 floor;
                    if (!HasFloor(host.position + dir * 0.85f, out floor))
                        continue;
                    if (!StandingRoom(floor) || !Reachable(floor))
                        continue;
                    stand = floor;
                    found = true;
                }
                if (!found)
                    continue;

                var st = MakeStation(parent, "station_visit_" + idx, 6, stand,
                    host.position - stand,
                    IsInterior(root.transform.InverseTransformPoint(stand), spawn, o),
                    16f, null);
                var handler = LegaiaWorldBuilder.TryAttachUdon(
                    st.gameObject, "LegaiaVisitSpot");
                if (handler != null)
                {
                    Component bubble = null;
                    if (o.speechBubbles && host.Find("speech_bubble") == null)
                        bubble = BuildBubble(host, icons, quad, o);
                    else if (o.speechBubbles)
                        bubble = host.Find("speech_bubble").GetComponent(
                            LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaSpeechBubble"));
                    LegaiaWorldBuilder.SetUdonField(handler, "station", st);
                    if (bubble != null)
                        LegaiaWorldBuilder.SetUdonField(handler, "hostBubble", bubble);
                    LegaiaWorldBuilder.SetUdonField(handler, "hostLine",
                        o.bubbleText
                            ? Shorten(MiniJson.AsStr(MiniJson.Get(n, "label")) ?? "")
                            : "");
                    LegaiaWorldBuilder.SyncUdonProxy(handler);
                    LegaiaWorldBuilder.SetUdonField(st, "handler", handler);
                }
                Tag(st, LegaiaBubbleArt.WAVE, "visit", false);
                made++;
            }
            return made;
        }

        /// Stand spots spread over the walkable OUTDOORS, further out than
        /// the viewpoint ring: the village paths, the gate, the low ground
        /// by the water. Sampled on a spiral, kept only where there is real
        /// floor, standing room, separation from what is already built, and
        /// a walkable route from somebody. A spot that sits well BELOW the
        /// spawn's own floor is shoreline: it faces outward (at the water)
        /// and is tagged so the errand-role pass can make it a water source.
        static int BuildLandmarkStands(GameObject root, Transform parent,
            Vector3 spawn, LegaiaLivingTownOptions o)
        {
            if (o.landmarkStands <= 0)
                return 0;
            Vector3 centre = root.transform.TransformPoint(spawn);
            Vector3 centreFloor;
            float baseY = HasFloor(centre, out centreFloor) ? centreFloor.y : centre.y;
            var taken = new List<Vector3>();
            foreach (Transform t in parent)
                taken.Add(t.position);
            int made = 0;
            for (int ring = 0; ring < 8 && made < o.landmarkStands; ring++)
            {
                float r = 7f + ring * 4f;
                for (int k = 0; k < 12 && made < o.landmarkStands; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 30f + ring * 13f,
                        Vector3.up) * Vector3.forward;
                    Vector3 floor;
                    if (!HasFloor(centre + dir * r, out floor) || !StandingRoom(floor))
                        continue;
                    if (IsInterior(root.transform.InverseTransformPoint(floor),
                            spawn, o))
                        continue;
                    bool crowded = false;
                    for (int i = 0; i < taken.Count; i++)
                        if (Vector3.Distance(taken[i], floor) < 3.5f)
                        {
                            crowded = true;
                            break;
                        }
                    if (crowded || !Reachable(floor))
                        continue;
                    bool low = floor.y < baseY - 0.6f;
                    var st = MakeStation(parent,
                        (low ? "station_shore_" : "station_path_") + made, 4, floor,
                        low ? floor - centre : centre - floor, false,
                        low ? 16f : 11f, null);
                    Tag(st, -1, low ? "shore" : "path", true);
                    taken.Add(floor);
                    made++;
                }
            }
            return made;
        }

        /// A conversation ring inside each interior room, so the villagers
        /// retail parked in one room talk to each other instead of only
        /// taking turns at the room's cupboard.
        static int BuildIndoorChatRings(GameObject root, Transform parent,
            List<Home> homes, LegaiaLivingTownOptions o)
        {
            int made = 0, spot = 0;
            for (int i = 0; i < homes.Count; i++)
            {
                Transform landing = homes[i].landingT;
                if (landing == null)
                    continue;
                Vector3 centreFloor;
                if (!HasFloorNear(landing.position + landing.forward * 1.2f,
                        out centreFloor)
                    && !HasFloorNear(landing.position, out centreFloor))
                    continue;
                // A ring the outdoor/NPC-cluster pass already put in this
                // room would be merged with this one by the director's
                // proximity grouping - one ring per room, not two.
                bool already = false;
                foreach (Transform t in parent)
                {
                    if (!t.name.StartsWith("station_chat_"))
                        continue;
                    if (Vector3.Distance(t.position, centreFloor) < 4f)
                    {
                        already = true;
                        break;
                    }
                }
                if (already)
                    continue;
                int here = 0;
                for (int k = 0; k < 3; k++)
                {
                    Vector3 dir = Quaternion.AngleAxis(k * 120f + i * 41f,
                        Vector3.up) * Vector3.forward;
                    Vector3 floor;
                    if (!HasFloorNear(centreFloor + dir * o.chatRingRadius, out floor)
                        || !StandingRoom(floor))
                        continue;
                    MakeStation(parent, "station_chat_in" + spot + "_" + k, 3,
                        floor, centreFloor - floor, true, 20f, null);
                    here++;
                }
                if (here >= 2)
                {
                    made += here;
                    spot++;
                }
                else
                {
                    for (int k = parent.childCount - 1; k >= 0; k--)
                        if (parent.GetChild(k).name
                                .StartsWith("station_chat_in" + spot + "_"))
                            Undo.DestroyObjectImmediate(parent.GetChild(k).gameObject);
                }
            }
            return made;
        }

        /// Turn a few of the plain stand spots into CARRY endpoints, so the
        /// day has fetching and hauling in it and not only standing:
        ///
        ///   - the lowest outdoor spot (a shoreline one where the scene has
        ///     water) becomes the water source: a bucket is filled there;
        ///   - the doorway stand furthest from it becomes where the bucket
        ///     is set down;
        ///   - the two outdoor spots furthest apart become a firewood run,
        ///     one picking the bundle up and one putting it down.
        ///
        /// Roles are picked by MEASUREMENT (lowest, furthest apart) rather
        /// than by scene-specific coordinates, so the same code gives any
        /// scene a plausible pair of errands.
        static int AssignErrandRoles(Transform parent, GameObject root,
            Vector3 spawn, LegaiaLivingTownOptions o)
        {
            if (!o.carryItems)
                return 0;
            var stationType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcStation");
            if (stationType == null)
                return 0;
            var open = new List<Component>();
            foreach (Transform t in parent)
            {
                var st = t.GetComponent(stationType);
                if (st == null)
                    continue;
                int kind = (int)stationType.GetField("kind").GetValue(st);
                bool indoors = (bool)stationType.GetField("indoors").GetValue(st);
                if (kind == 4 && !indoors)
                    open.Add(st);
            }
            if (open.Count < 2)
                return 0;

            // Water: the lowest-lying open spot. In a scene with a shore
            // that is the water's edge; in one without, it is still the
            // bottom of the village, which is where a well would be.
            int water = 0;
            for (int i = 1; i < open.Count; i++)
                if (open[i].transform.position.y < open[water].transform.position.y)
                    water = i;

            // Firewood: the two spots furthest from each other, so the run
            // crosses the village rather than being two steps.
            int a = -1, b = -1;
            float best = -1f;
            for (int i = 0; i < open.Count; i++)
            {
                if (i == water)
                    continue;
                for (int j = i + 1; j < open.Count; j++)
                {
                    if (j == water)
                        continue;
                    float d = Vector3.Distance(open[i].transform.position,
                        open[j].transform.position);
                    if (d > best)
                    {
                        best = d;
                        a = i;
                        b = j;
                    }
                }
            }

            int made = 0;
            MakeCarryHandler(open[water], 0, false, true, 0);
            Tag(open[water], LegaiaBubbleArt.WORK, "water_fill", true);
            open[water].gameObject.name = "station_water";
            made++;
            if (a >= 0 && b >= 0)
            {
                MakeCarryHandler(open[a], 2, false, true, 0);
                Tag(open[a], LegaiaBubbleArt.WORK, "wood_take", false);
                open[a].gameObject.name = "station_wood_take";
                MakeCarryHandler(open[b], -1, true, false, 0);
                Tag(open[b], -1, "wood_drop", true);
                open[b].gameObject.name = "station_wood_drop";
                made += 2;
            }
            // Somewhere for the bucket to end up: the doorway stand
            // furthest from the water, which reads as carrying it home.
            Component drop = null;
            float far = -1f;
            foreach (Transform t in parent)
            {
                if (!t.name.StartsWith("station_door_"))
                    continue;
                var st = t.GetComponent(stationType);
                if (st == null || st.GetComponent(
                        LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcHandItem")) != null)
                    continue;
                float d = Vector3.Distance(t.position, open[water].transform.position);
                if (d > far)
                {
                    far = d;
                    drop = st;
                }
            }
            if (drop != null)
            {
                MakeCarryHandler(drop, -1, true, false, 0);
                Tag(drop, -1, "water_drop", true);
                made++;
            }
            return made;
        }

        // --- The carried-item rig ----------------------------------------------------

        /// Build one villager's hand + items and wire LegaiaNpcCarry.
        ///
        /// FINDING THE HAND. These rigs have no skeleton: the instance is a
        /// flat set of mesh nodes (head, torso, two arm segments a side, and
        /// on the full humanoid two leg segments a side). Their NAMES carry
        /// no meaning across families - the arm is nodes 2-5 on the
        /// ten-node humanoid and the six-node robed rig but 3-6 on the
        /// short skirted one - so the hand is found by SHAPE: among the
        /// body's nodes at arm height, the ones standing widest of the
        /// centreline, and of those the LOWEST, which is a forearm or a
        /// hand on every family measured. Everything is done in the NPC's
        /// own local frame, so the mirrors on the instance and on the built
        /// root never enter the arithmetic.
        // How the hand point was arrived at, over the pass (reported once,
        // so the heuristic above is a measurement in the log rather than an
        // assumption in the code).
        static int s_handArm, s_handTorso;
        static float s_handFraction;

        static Component BuildCarry(Transform npc, string genDir,
            LegaiaLivingTownOptions o)
        {
            var rends = npc.GetComponentsInChildren<Renderer>(true);
            if (rends.Length == 0)
                return null;
            Bounds wb = rends[0].bounds;
            for (int i = 1; i < rends.Length; i++)
                wb.Encapsulate(rends[i].bounds);
            float lossyY = Mathf.Abs(npc.lossyScale.y);
            if (lossyY < 1e-4f)
                lossyY = 1f;

            // Body height in the NPC's LOCAL units, measured off the body
            // nodes alone: the speech bubble's icon quads are children too
            // and they sit well above the head.
            var filters = npc.GetComponentsInChildren<MeshFilter>(true);
            float top = 0f;
            for (int i = 0; i < filters.Length; i++)
            {
                if (filters[i].sharedMesh == null || IsBubblePart(filters[i].transform))
                    continue;
                Vector3 c = npc.InverseTransformPoint(
                    filters[i].transform.TransformPoint(filters[i].sharedMesh.bounds.center));
                if (c.y > top)
                    top = c.y;
            }
            float h = top > 0.05f ? top * 1.12f : wb.size.y / lossyY;
            h = Mathf.Clamp(h, 0.25f, 3f);

            // Widest node at arm height, then the lowest of those.
            float widest = 0f;
            for (int i = 0; i < filters.Length; i++)
            {
                if (filters[i].sharedMesh == null || IsBubblePart(filters[i].transform))
                    continue;
                Vector3 c = npc.InverseTransformPoint(
                    filters[i].transform.TransformPoint(filters[i].sharedMesh.bounds.center));
                if (c.y < h * 0.22f || c.y > h * 0.80f)
                    continue;
                float ax = Mathf.Abs(c.x);
                if (ax > widest)
                    widest = ax;
            }
            Vector3 hand;
            if (widest >= h * 0.08f)
            {
                s_handArm++;
                float bestY = float.MaxValue;
                Vector3 pick = Vector3.zero;
                for (int i = 0; i < filters.Length; i++)
                {
                    if (filters[i].sharedMesh == null || IsBubblePart(filters[i].transform))
                        continue;
                    Vector3 c = npc.InverseTransformPoint(
                        filters[i].transform.TransformPoint(filters[i].sharedMesh.bounds.center));
                    if (c.y < h * 0.22f || c.y > h * 0.80f)
                        continue;
                    if (Mathf.Abs(c.x) < widest * 0.85f)
                        continue;
                    if (c.y < bestY)
                    {
                        bestY = c.y;
                        pick = c;
                    }
                }
                // A hand's width out from the arm node, so the item hangs
                // beside the body rather than inside it.
                hand = pick + new Vector3(Mathf.Sign(pick.x) * h * 0.06f,
                    -h * 0.05f, 0f);
            }
            else
            {
                // No arm to find (a signpost, a two-node rig): beside the
                // torso at the same height. A LATERAL offset needs no guess
                // about which way the model faces.
                s_handTorso++;
                hand = new Vector3(h * 0.17f, h * 0.42f, 0f);
            }

            s_handFraction += h > 1e-4f ? hand.y / h : 0f;
            var holder = new GameObject("carry");
            holder.transform.SetParent(npc, false);
            holder.transform.localPosition = hand;
            holder.transform.localRotation = Quaternion.identity;

            var items = LegaiaCarryArt.Build(holder.transform, genDir, h);
            var udon = LegaiaWorldBuilder.TryAttachUdon(
                npc.gameObject, "LegaiaNpcCarry");
            if (udon == null)
                return null;
            LegaiaWorldBuilder.SetUdonField(udon, "hand", holder.transform);
            LegaiaWorldBuilder.SetUdonField(udon, "items", items);
            LegaiaWorldBuilder.SyncUdonProxy(udon);
            return udon;
        }

        /// A node belonging to the speech-bubble rig rather than to the
        /// villager's body (it is a child of this pass's own bubble object).
        static bool IsBubblePart(Transform t)
        {
            for (var u = t; u != null; u = u.parent)
                if (u.name == "speech_bubble" || u.name == "carry")
                    return true;
            return false;
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
            LegaiaLivingTownOptions o, List<Home> homes, bool navLive)
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
            // A villager retail placed INSIDE a house (the detached
            // interior rooms) is home already: no front door is reachable
            // from its island, so it gets no door and keeps to its room.
            var insideAlready = new bool[files.Count];
            // No walkable route from the spawn to ANY front door (a
            // villager on the beach below the village bank, say): no home,
            // it stays out at night rather than clip up the bank.
            var noRoute = new bool[files.Count];
            for (int i = 0; i < homeOf.Length; i++)
            {
                homeOf[i] = -1;
                insideAlready[i] = InsideAHome(objs[i].position, homes);
            }
            // Round-robin the shuffled villagers over the houses, capped.
            if (homes.Count > 0 && o.homeCap > 0)
            {
                int h = 0;
                for (int k = 0; k < order.Length; k++)
                {
                    int idx = order[k];
                    if (insideAlready[idx])
                        continue;
                    string token = Path.GetFileNameWithoutExtension(files[idx]);
                    int forced = settings.HomeOverride(token);
                    if (forced >= 0 && forced < homes.Count)
                    {
                        homeOf[idx] = forced;
                        homes[forced].occupants++;
                        continue;
                    }
                    bool anyRoute = false;
                    for (int tries = 0; tries < homes.Count; tries++)
                    {
                        int cand = (h + tries) % homes.Count;
                        if (navLive)
                        {
                            string why;
                            if (!LegaiaNavMesh.Reachable(objs[idx].position,
                                    homes[cand].doorT.position, 1.2f, out why))
                                continue;
                        }
                        anyRoute = true;
                        if (homes[cand].occupants >= o.homeCap)
                            continue;
                        homeOf[idx] = cand;
                        homes[cand].occupants++;
                        h = cand + 1;
                        break;
                    }
                    if (navLive && !anyRoute)
                    {
                        noRoute[idx] = true;
                        Debug.LogWarning("[Legaia] living town: " + objs[idx].name +
                            " has no walkable route to any front door - it keeps " +
                            "no home and stays out at night.");
                    }
                }
            }

            var walkWired = new HashSet<string>();
            s_handArm = 0;
            s_handTorso = 0;
            s_handFraction = 0f;
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
                // The carry rig is measured off the BODY, so it is built
                // before the bubble's icon quads would be counted as nodes
                // (BuildCarry skips them explicitly too).
                Component held = o.carryItems ? BuildCarry(npc, genDir, o) : null;
                bool dayIn = homeOf[i] >= 0 && indoorsMade < indoorsWanted;
                if (dayIn)
                    indoorsMade++;

                LegaiaWorldBuilder.SetUdonField(brain, "loco", loco);
                LegaiaWorldBuilder.SetUdonField(brain, "bubble", bubble);
                if (held != null)
                    LegaiaWorldBuilder.SetUdonField(brain, "carry", held);
                LegaiaWorldBuilder.SetUdonField(brain, "seed", seed);
                LegaiaWorldBuilder.SetUdonField(brain, "daytimeIndoors",
                    dayIn || insideAlready[i]);
                LegaiaWorldBuilder.SetUdonField(brain, "startIndoors", insideAlready[i]);
                LegaiaWorldBuilder.SetUdonField(brain, "noRoute", noRoute[i]);
                LegaiaWorldBuilder.SetUdonField(brain, "firstLine",
                    o.bubbleText ? Shorten(labels[i]) : "");
                if (homeOf[i] >= 0)
                {
                    Home home = homes[homeOf[i]];
                    LegaiaWorldBuilder.SetUdonField(brain, "homeDoor", home.doorT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeThreshold", home.thresholdT);
                    if (home.doorProp != null)
                        LegaiaWorldBuilder.SetUdonField(brain, "homeDoorProp", home.doorProp);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeLanding", home.landingT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeExit", home.exitT);
                    LegaiaWorldBuilder.SetUdonField(brain, "homeEmerge", home.emergeT);
                }
                LegaiaWorldBuilder.SyncUdonProxy(brain);
                brains.Add(brain);
            }
            if (o.carryItems && s_handArm + s_handTorso > 0)
                Debug.Log("[Legaia] living town: hand point measured from an ARM " +
                    "node on " + s_handArm + " rig(s), from a torso offset on " +
                    s_handTorso + " (no arm in the node set); mean hand height " +
                    (s_handFraction / (s_handArm + s_handTorso)).ToString("0.00") +
                    " of body height.");
            if (o.walkAnimator)
                Debug.Log("[Legaia] living town: walk cycle '" + o.walkClip +
                    "' bound on " + walked + " of " + files.Count +
                    " villager(s) (the rigs whose family carries one).");
            return brains;
        }

        /// A villager standing in one of the detached interior rooms: nearer
        /// to some home's landing (or way out) than to any front door.
        /// Geometry, not a radius from spawn - a villager at the far edge
        /// of the village is still outside, and one in a room no homed
        /// door leads to (a shop's back room) is still inside.
        static bool InsideAHome(Vector3 world, List<Home> homes)
        {
            float nearestIn = float.MaxValue, nearestDoor = float.MaxValue;
            foreach (var h in homes)
            {
                if (h.landingT != null)
                    nearestIn = Mathf.Min(nearestIn,
                        Vector3.Distance(world, h.landingT.position));
                if (h.exitT != null)
                    nearestIn = Mathf.Min(nearestIn,
                        Vector3.Distance(world, h.exitT.position));
                if (h.thresholdT != null)
                    nearestDoor = Mathf.Min(nearestDoor,
                        Vector3.Distance(world, h.thresholdT.position));
            }
            return nearestIn < nearestDoor;
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
            LegaiaWorldBuilder.SyncUdonProxy(udon);
            return udon;
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
