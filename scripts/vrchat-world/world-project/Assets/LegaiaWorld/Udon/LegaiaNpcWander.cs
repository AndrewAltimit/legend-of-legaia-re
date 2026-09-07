// The NPC LOCOMOTION CONTROLLER: everything that moves a placed villager
// around the world. Two ways in:
//
//   1. Autonomous stroll (the original behaviour, still the default): the
//      NPC wanders a small radius around its spawn point, pausing between
//      strolls - the "town feels inhabited" layer on top of the looping
//      idle clip the builder wires up. An NPC with nothing else attached
//      behaves exactly as before.
//   2. A COMMAND API for the living-town layer (LegaiaNpcBrain drives it,
//      LegaiaTownDirector schedules it): GoTo / FaceToward / Arrived /
//      Blocked / Stop / Teleport / SetIdle / SetHome. A commanded walk uses
//      `walkSpeed` (a purposeful errand, faster than the amble) and steers
//      around obstacles; Stop() hands the NPC back to the stroll.
//
// Collision-aware: strolls are clamped against the world's colliders (the
// builder's merged double-sided collider included), a short waist-height
// ray stops a walk that would clip a wall, and the NPC follows the floor
// with a downward ray - so villagers no longer amble through huts. Trigger
// colliders (doorway teleports, door-approach boxes) are ignored, so a
// wandering NPC neither blocks on them nor fires them. Movement is
// forward-only: on a direction change the NPC pivots in place until
// aligned, then steps off - it never translates while mis-facing.
//
// PATHFINDING (commanded walks): a GoTo first asks the baked navmesh
// (LegaiaNavMesh bakes it from the world's colliders at build time and
// LegaiaNavMeshLoader registers it on load) for a route, and the walk
// then follows the route's corners one by one. That is what keeps a
// villager on ground it can stand on: the route climbs the hill by its
// path and rounds the huts rather than aiming straight at the door and
// walking into the slope (which the floor ray then read as "the floor is
// up there" - the clipping-through-terrain look). A world with no bake,
// or a target no mesh reaches, falls back to the straight-line walk below,
// so nothing depends on the navmesh existing.
//
// LEDGE HOPS: the bake leaves a drop the agent cannot climb as two
// separate islands (town01's beach sits 1.2 m below the village), and no
// route crosses between them. LegaiaNavMesh finds the places where the
// gap is short enough to jump and hands them here as `linkFrom` /
// `linkTo` marker pairs; when a straight CalculatePath does not complete,
// the walk is re-composed as a CHAIN of walks and hops (up to MAX_HOPS of
// them - town01's shore needs two, the bank up onto the path and a step
// off the path into the village). The hop itself is a scripted parabola
// (mode 6), not a physics jump: the floor ray is off while airborne so
// the villager clears the bank instead of being dragged back onto it.
//
// STEERING (commanded walks only): the local reactive layer under the
// route. When the line to the next corner is blocked within
// `probeDistance` (another villager's capsule, a player-moved prop), a
// fan of rays at +/-30, +/-60, +/-85 degrees looks for a clear lane and
// the NPC commits to it for a moment before re-aiming. When no lane is
// clear, or progress stalls, the route is re-planned once from where the
// NPC stands; failing that, Blocked() goes true and the brain picks
// something else (it never teleports through the wall).
//
// The steering is switched OFF on the final approach - inside a stride of
// the target with no corner left to turn. A doorway is narrower than the
// probe fan can read as passable, so every lane but the straight one is
// blocked, and a villager that keeps re-picking lanes circles a step
// short of its own front door for ever. Two watchdogs back that up: the
// old displacement one, and a PROGRESS one that ends the command when the
// walk has not come closer to its target in five seconds - the only test
// a sidestepping livelock cannot pass.
//
// FACING - measured, not derived. The exported NPC glbs have no skins:
// each TMD object is a rigid mesh on its own animated node, and the node
// REST rotations are frame 0 of the spawn clip - so the facing retail
// authored for the NPC is baked into the node transforms themselves
// (town01's spawn_record_17 family rests the whole rig at -90 degrees;
// most rigs rest at 0). On top of that sit the glb root's Rx(180), the
// importer's handedness conversion, the builder's scale mirrors and any
// idle sway the clip animates - too many stacked sign conventions to fold
// by hand (each attempt so far has been wrong for some rig). So this
// behaviour derives nothing:
//   - Start picks a facing ANCHOR: the largest mesh node whose rendered
//     rest pose keeps the model's up axis vertical (the torso - limb and
//     head nodes rest tilted, checked across every town01 rig).
//   - The anchor's VISUAL forward is read through the full transform
//     matrix (as a TransformPoint difference, so scale mirrors count),
//     which bakes in every mirror, conversion and animated rotation.
//   - Start also probes which way that visual forward moves when the
//     transform yaws +10 degrees (mirrors can flip it), and Update servos
//     the yaw with the probed sign until visual forward lies on the walk
//     direction. No rest capture, no calibration frames, no sign algebra.
// Teleport() uses the same measurement (one servo step of the whole error)
// rather than assuming any relation between transform.forward and the
// visible facing.
//
// The one convention that must be ASSUMED is the face axis, and the
// invariant that holds across every town01 model (textured 4-view
// renders - wireframes cannot tell front from back) is: the mesh faces
// +Z in the INSTANCE-local (glb scene) frame at rest. It is NOT a fixed
// node-local axis: one rig family rests its nodes at -90 deg yaw with
// the vertices counter-rotated (the "male warrior" family - npc_12 and
// kin - which walked sideways while every rest-yaw-0 rig walked right),
// so the anchor's node-local face axis varies per family. Start folds
// the anchor's rest rotation out once (anchorFaceLocal below); with
// that, rest-yaw-0 rigs reduce exactly to the previously verified
// behaviour and the rotated family lands on its true axis.
//
// WALK ANIMATION: with `locoAnimator` wired to an idle/walk controller
// (the living-town pass generates one when a rig family has a clip that
// measures as a walk cycle), the controller crossfades between the two
// states as stepping starts and stops. Left null - which is the case
// whenever no clip convincingly walks - the NPC keeps looping whatever
// clip the builder attached, exactly as before.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK via the Creator
// Companion). Drop this component on an NPC instance the builder placed;
// tune radius/speed per NPC. Movement is local (each player computes it
// independently) - fine for ambience; use synced variables if you need
// every player to agree.

using UdonSharp;
using UnityEngine;
using UnityEngine.AI;

namespace LegaiaWorld
{
    public class LegaiaNpcWander : UdonSharpBehaviour
    {
        // NavMesh.AllAreas: the constant is not reachable through Udon's
        // type exposure, so its value is spelled out.
        const int ALL_AREAS = -1;

        [Tooltip("How far from the spawn point the NPC strolls (meters). " +
                 "Default suits the 1 m-per-tile export scale.")]
        public float radius = 1.5f;

        [Tooltip("Walk speed in m/s. Legaia townsfolk amble - keep it low.")]
        public float speed = 0.4f;

        [Tooltip("Speed of a COMMANDED walk (GoTo) in m/s - an errand across " +
                 "the village is purposeful, not an amble.")]
        public float walkSpeed = 0.7f;

        [Tooltip("Average pause between strolls (seconds).")]
        public float pauseSeconds = 5f;

        [Tooltip("Turn rate toward the walk direction (degrees/second).")]
        public float turnSpeed = 240f;

        [Tooltip("Clear space kept between the NPC and any wall (meters).")]
        public float wallClearance = 0.3f;

        [Tooltip("How close to a commanded target counts as arrived (meters).")]
        public float arriveRadius = 0.3f;

        [Tooltip("How far ahead a commanded walk looks for obstacles (meters).")]
        public float probeDistance = 0.9f;

        [Tooltip("Route commanded walks over the baked navmesh (LegaiaNavMesh). " +
                 "Off, or with no bake in the scene, walks aim straight at the target.")]
        public bool useNavMesh = true;

        [Tooltip("How far off the navmesh a walk's start / goal may sit and still be snapped onto it (meters).")]
        public float navSnapRadius = 1.2f;

        [Tooltip("How close to a route corner counts as having turned it (meters).")]
        public float cornerRadius = 0.22f;

        [Tooltip("Village-side end of each ledge link the bake found (LegaiaNavMesh), " +
                 "paired index-for-index with linkTo. A route that no walk connects " +
                 "is retried as walk -> hop -> walk over one of these.")]
        public Transform[] linkFrom;

        [Tooltip("Far end of each ledge link (see linkFrom).")]
        public Transform[] linkTo;

        [Tooltip("How long one ledge hop takes (seconds).")]
        public float hopSeconds = 0.6f;

        [Tooltip("How high the hop arcs over its higher end (meters).")]
        public float hopApex = 0.3f;

        // Ledge hops taken - published for the play-mode soak harness.
        [HideInInspector] public int hops;

        [Tooltip("Optional Animator with 'idle' and 'walk' states - crossfaded " +
                 "as stepping starts and stops. Null keeps the builder's " +
                 "looping spawn clip.")]
        public Animator locoAnimator;

        [Tooltip("Animator state played while standing still.")]
        public string idleState = "idle";

        [Tooltip("Animator state played while stepping.")]
        public string walkState = "walk";

        [Tooltip("Crossfade time between idle and walk (seconds).")]
        public float animCrossfade = 0.15f;

        [Tooltip("Tick if this NPC walks exactly backwards: covers a model " +
                 "authored facing -Z in its file where every known rig " +
                 "faces +Z.")]
        public bool flipFacing = false;

        [Tooltip("Extra facing correction in degrees, added on top of the " +
                 "measured visual forward - for hand-tuning one NPC.")]
        public float facingYawOffset = 0f;

        private Vector3 home;
        private Vector3 target;
        private float pauseUntil;
        private Transform anchor;
        private Vector3 anchorFaceLocal = Vector3.back;
        private float servoSign = 1f;
        private bool walking;
        private Vector3 lastForward = Vector3.forward;
        // Measured at Start from the rendered rest pose, so the wall and
        // floor rays stay proportioned to the model at any export scale.
        private float npcHeight = 1.6f;
        private float rayHeight = 0.8f;

        // --- Command state ------------------------------------------------
        // mode 0 = autonomous stroll (the default), 1 = commanded walk to
        // `commandTarget`, 2 = idle (frozen in place), 3 = turn in place
        // toward `faceDir` and then hold, 4 = unused, 5 = scripted slide
        // (the step through a doorway), 6 = a ledge hop over a link.
        private int mode;
        private Vector3 commandTarget;
        private Vector3 faceDir = Vector3.forward;
        private bool arrived;
        private bool blocked;
        private Vector3 progressMark;
        private float progressAt;
        private Vector3 steerDir;
        private float steerUntil;
        private bool animWalking;
        private bool animStarted;

        // --- Route state ---------------------------------------------------
        // The navmesh route of the current command: its corners, and the
        // one the walk is heading for. `havePath` false = straight line.
        private NavMeshPath path;
        private Vector3[] corners;
        private int cornerIndex;
        private bool havePath;
        private bool replanned;
        // Per-command arrive radius (GoToWithin); 0 = the field default.
        private float commandArrive;
        // Progress watchdog: the closest this command has ever been to its
        // target, and when that record was last beaten. A steering livelock
        // (sidestep left, sidestep right, forever, half a metre short of a
        // doorway) moves plenty and gets no closer, so a per-frame
        // displacement test never catches it - this does.
        private float bestDist;
        private float betterAt;
        private int watchedCorner = -1;
        // The chain of ledge hops this route takes, near end and far end
        // per hop, and which of them is next. A route is walked one LEG at
        // a time (this position -> the next hop's near end, or the target)
        // and the leg after a hop is planned on landing, so no jagged
        // corner arrays are needed.
        private const int MAX_HOPS = 3;
        private Vector3[] chainP = new Vector3[MAX_HOPS];
        private Vector3[] chainQ = new Vector3[MAX_HOPS];
        private int chainCount;
        private int chainIndex;
        private bool haveHop;
        private Vector3 hopA, hopB;
        private float hopStart;
        // Scripted slide (mode 5).
        private Vector3 slideFrom, slideTo;
        private float slideStart, slideSeconds;
        // 0 = not yet probed, 1 = a navmesh is registered here, 2 = none.
        // Probed lazily: the loader registers the bake in its own Start,
        // and Start order across behaviours is not defined.
        private int navState;

        void Start()
        {
            home = transform.position;
            target = home;
            pauseUntil = Time.time + Random.Range(0f, pauseSeconds);

            // Facing anchor: the biggest mesh node standing upright in the
            // rest pose (Start runs before the Animator's first evaluation,
            // so the nodes still hold the glb defaults = spawn-clip frame 0).
            // The torso qualifies on every town01 rig; heads, limbs and
            // bowing poses rest tilted and are skipped. Uprightness is
            // tested on the RENDERED direction (through the full matrix
            // chain), so mirrors and the importer's conversion are included.
            MeshFilter[] filters = GetComponentsInChildren<MeshFilter>();
            float bestUpright = -1f;
            float bestAny = -1f;
            Transform anyAnchor = null;
            for (int i = 0; i < filters.Length; i++)
            {
                Mesh mesh = filters[i].sharedMesh;
                if (mesh == null)
                    continue;
                Vector3 s = mesh.bounds.size;
                // Flat meshes have zero volume; the vertex count keeps them
                // comparable without ever outranking a real solid.
                float score = s.x * s.y * s.z + mesh.vertexCount * 1e-6f;
                Transform t = filters[i].transform;
                // TransformPoint difference = the full matrix applied to a
                // direction, scale mirrors included (TransformDirection
                // ignores scale and would miss them).
                Vector3 up = (t.TransformPoint(Vector3.up)
                    - t.TransformPoint(Vector3.zero)).normalized;
                if (score > bestAny)
                {
                    bestAny = score;
                    anyAnchor = t;
                }
                if (up.y > 0.9f && score > bestUpright)
                {
                    bestUpright = score;
                    anchor = t;
                }
            }
            if (anchor == null)
                anchor = anyAnchor;
            // The face axis in the ANCHOR's local frame, captured at rest
            // (before the Animator's first evaluation). Every model faces
            // +Z in the instance frame at rest, but the anchor node's rest
            // rotation varies per rig family (one family rests at -90 deg
            // yaw with counter-rotated vertices), so the composed rest
            // rotation is folded out once here. Pure quaternions are safe
            // for this: every mirror in the chain lives on scales at or
            // above the instance, and Transform.rotation composes
            // rotations only - the mirrors re-enter through TransformPoint
            // in VisualForward.
            if (anchor != null)
                anchorFaceLocal = Quaternion.Inverse(anchor.rotation)
                    * (transform.rotation * Vector3.forward);
            lastForward = transform.forward;

            // Model height from the rendered rest bounds: the ray heights
            // must track the villager, not an assumed human - at the
            // 1 m-per-tile export scale these models stand well under 1 m,
            // and a fixed waist ray would pass over their heads.
            Renderer[] rends = GetComponentsInChildren<Renderer>();
            if (rends.Length > 0)
            {
                Bounds wb = rends[0].bounds;
                for (int i = 1; i < rends.Length; i++)
                    wb.Encapsulate(rends[i].bounds);
                npcHeight = Mathf.Clamp(wb.size.y, 0.3f, 2.5f);
            }
            rayHeight = 0.5f * npcHeight;

            // Servo-sign probe: yaw the instance +10 degrees, see which way
            // the visual forward actually moves (a mirror in the scale chain
            // reverses it), and restore. The walk servo then always turns
            // the visible mesh TOWARD the walk direction, whatever the
            // mirror stack is.
            if (anchor != null)
            {
                Vector3 f0 = VisualForward();
                Quaternion saved = transform.rotation;
                transform.rotation =
                    Quaternion.AngleAxis(10f, Vector3.up) * saved;
                Vector3 f1 = VisualForward();
                transform.rotation = saved;
                float resp = Vector3.SignedAngle(f0, f1, Vector3.up);
                servoSign = resp < 0f ? -1f : 1f;
            }
            faceDir = VisualForward();
            progressMark = transform.position;
            progressAt = Time.time;
        }

        // --- Command API (LegaiaNpcBrain) ---------------------------------

        /// Walk to a world position. Arrived() goes true within
        /// `arriveRadius`; Blocked() goes true when no route opens up.
        public void GoTo(Vector3 worldPos)
        {
            GoToWithin(worldPos, arriveRadius);
        }

        /// GoTo with a per-command arrive radius. A stand spot a step out
        /// from a doorway cannot be reached to the default 0.3 m: the hut
        /// wall and the closed door leaf are both inside the probe fan's
        /// reach there, so the steering keeps finding a lane a hand's width
        /// off the line and never converges. The brain asks for a wider
        /// circle on the last leg of the door trip instead.
        public void GoToWithin(Vector3 worldPos, float arrive)
        {
            commandTarget = worldPos;
            commandArrive = arrive < 0.05f ? arriveRadius : arrive;
            mode = 1;
            arrived = false;
            blocked = false;
            steerUntil = 0f;
            replanned = false;
            progressMark = transform.position;
            progressAt = Time.time;
            watchedCorner = -1;
            PlanRoute();
        }

        /// A scripted straight step over a short distance (the walk through
        /// a doorway): position is interpolated, the mesh faces the way it
        /// goes, and the floor ray still runs - but no probe, no lane and
        /// no route. Only for the last metre through an opening the local
        /// steering cannot read as passable; Arrived() goes true at the end.
        public void SlideTo(Vector3 worldPos, float seconds)
        {
            slideFrom = transform.position;
            slideTo = worldPos;
            slideStart = Time.time;
            slideSeconds = seconds < 0.1f ? 0.1f : seconds;
            mode = 5;
            arrived = false;
            blocked = false;
            havePath = false;
            haveHop = false;
            Vector3 d = worldPos - transform.position;
            d.y = 0f;
            if (d.sqrMagnitude > 1e-6f)
                faceDir = d.normalized;
        }

        /// Ledge hops taken since load (the soak harness reads this).
        public int Hops()
        {
            return hops;
        }

        // --- Navmesh route ---------------------------------------------------

        /// True when a navmesh is registered under this NPC (probed once,
        /// after the loader has had a chance to run).
        bool NavAvailable()
        {
            if (!useNavMesh)
                return false;
            if (navState == 0)
            {
                // Give the loader's Start a moment; until then, straight line.
                if (Time.timeSinceLevelLoad < 0.5f)
                    return false;
                NavMeshHit probe;
                navState = NavMesh.SamplePosition(transform.position, out probe,
                    Mathf.Max(navSnapRadius, 2f), ALL_AREAS) ? 1 : 2;
            }
            return navState == 1;
        }

        /// Ask the navmesh for a route from here to `commandTarget`. Leaves
        /// `havePath` false (straight-line walk) when there is no mesh, no
        /// mesh near either end, or no route at all.
        void PlanRoute()
        {
            havePath = false;
            haveHop = false;
            cornerIndex = 0;
            if (!NavAvailable())
                return;
            NavMeshHit from, to;
            if (!NavMesh.SamplePosition(transform.position, out from, navSnapRadius, ALL_AREAS))
                return;
            if (!NavMesh.SamplePosition(commandTarget, out to, navSnapRadius, ALL_AREAS))
                return;
            Vector3[] direct = RouteBetween(from.position, to.position);
            if (direct != null)
            {
                corners = direct;
                cornerIndex = 1;
                havePath = true;
                return;
            }
            // Nothing walkable connects the two: the goal is on another
            // island of the mesh (the beach below the village bank is one).
            // Retry as a CHAIN of walks and hops over the ledge links the
            // bake found.
            if (!PlanChain(from.position, to.position))
                return;
            chainIndex = 0;
            hopA = chainP[0];
            hopB = chainQ[0];
            haveHop = true;
            PlanLegTo(hopA);
        }

        /// Plan the current leg: this position to `goal`, on the navmesh.
        void PlanLegTo(Vector3 goal)
        {
            havePath = false;
            cornerIndex = 0;
            NavMeshHit here;
            if (!NavMesh.SamplePosition(transform.position, out here,
                    navSnapRadius, ALL_AREAS))
                return;
            Vector3[] c = RouteBetween(here.position, goal);
            if (c == null)
                return;
            corners = c;
            cornerIndex = 1;
            havePath = true;
        }

        /// Greedy best-first chain of at most MAX_HOPS ledge links from `a`
        /// to `b`: at each step, of the links whose near end this leg can
        /// still walk to, take the one whose far end lands nearest the
        /// goal. town01's shore needs two of them - the bank up to the path
        /// and a step off the path into the village - which is why a
        /// single-hop composition left the two shore villagers homeless
        /// even with the bank link baked.
        bool PlanChain(Vector3 a, Vector3 b)
        {
            chainCount = 0;
            if (linkFrom == null || linkTo == null)
                return false;
            int n = linkFrom.Length < linkTo.Length ? linkFrom.Length : linkTo.Length;
            if (n == 0)
                return false;
            bool[] used = new bool[n];
            Vector3 cur = a;
            for (int step = 0; step < MAX_HOPS; step++)
            {
                int bestLink = -1;
                bool bestFlip = false;
                float bestScore = 1e9f;
                for (int i = 0; i < n; i++)
                {
                    if (used[i] || linkFrom[i] == null || linkTo[i] == null)
                        continue;
                    for (int dir = 0; dir < 2; dir++)
                    {
                        Vector3 p = dir == 0 ? linkFrom[i].position : linkTo[i].position;
                        Vector3 q = dir == 0 ? linkTo[i].position : linkFrom[i].position;
                        float score = Vector3.Distance(q, b);
                        if (score >= bestScore)
                            continue;
                        if (RouteBetween(cur, p) == null)
                            continue;
                        bestScore = score;
                        bestLink = i;
                        bestFlip = dir == 1;
                    }
                }
                if (bestLink < 0)
                    return false;
                used[bestLink] = true;
                Vector3 near = bestFlip
                    ? linkTo[bestLink].position : linkFrom[bestLink].position;
                Vector3 far = bestFlip
                    ? linkFrom[bestLink].position : linkTo[bestLink].position;
                chainP[chainCount] = near;
                chainQ[chainCount] = far;
                chainCount++;
                cur = far;
                if (RouteBetween(cur, b) != null)
                    return true;
            }
            chainCount = 0;
            return false;
        }

        /// The corner list of a COMPLETE route, or null. corners[0] is the
        /// start, so a caller walks from index 1.
        Vector3[] RouteBetween(Vector3 a, Vector3 b)
        {
            if (path == null)
                path = new NavMeshPath();
            if (!NavMesh.CalculatePath(a, b, ALL_AREAS, path))
                return null;
            if (path.status != NavMeshPathStatus.PathComplete)
                return null;
            Vector3[] c = path.corners;
            return c == null || c.Length < 2 ? null : c;
        }

        /// What this LEG of the walk ends at: the near side of the ledge
        /// link when the route hops, otherwise the commanded target.
        Vector3 LegTarget()
        {
            return haveHop ? hopA : commandTarget;
        }

        Vector3 CurrentAim()
        {
            if (!havePath)
                return LegTarget();
            int last = corners.Length - 1;
            while (cornerIndex < last)
            {
                Vector3 c = corners[cornerIndex] - transform.position;
                c.y = 0f;
                if (c.magnitude > cornerRadius)
                    break;
                cornerIndex++;
            }
            // The final corner is the navmesh's snap of the target; the
            // exact target is what the brain asked for.
            return cornerIndex >= last ? LegTarget() : corners[cornerIndex];
        }

        /// Turn in place until the MESH faces `worldPos` (no translation).
        /// This CANCELS a walk command: a brain calls it on arrival, and
        /// leaving the walk mode running would keep the arrival test - not
        /// the facing servo - in charge of the NPC.
        public void FaceToward(Vector3 worldPos)
        {
            Vector3 d = worldPos - transform.position;
            d.y = 0f;
            if (d.sqrMagnitude > 1e-6f)
                faceDir = d.normalized;
            mode = 3;
            walking = false;
        }

        /// True once a commanded walk has reached its target.
        public bool Arrived()
        {
            return arrived;
        }

        /// True when the commanded walk found no way through (a wall with no
        /// clear lane, or no forward progress for a few seconds).
        public bool Blocked()
        {
            return blocked;
        }

        /// Cancel any command and hand the NPC back to its autonomous
        /// stroll around `home`.
        public void Stop()
        {
            mode = 0;
            arrived = false;
            blocked = false;
            walking = false;
            steerUntil = 0f;
            havePath = false;
            haveHop = false;
            commandArrive = 0f;
            target = transform.position;
            pauseUntil = Time.time + Random.Range(0.2f, 1.2f);
        }

        /// Hard reposition (the NPC "walks through" a doorway teleport the
        /// way a player does). `facing` is a world direction; the visible
        /// mesh is turned onto it in one step using the same measurement
        /// the walk servo uses. The NPC is left idle - the brain decides
        /// what happens on the other side.
        public void Teleport(Vector3 pos, Vector3 facing)
        {
            transform.position = pos;
            SnapToFloor();
            facing.y = 0f;
            if (facing.sqrMagnitude > 1e-6f)
            {
                faceDir = facing.normalized;
                float err = Vector3.SignedAngle(VisualForward(), faceDir, Vector3.up)
                    + facingYawOffset;
                transform.rotation =
                    Quaternion.AngleAxis(servoSign * err, Vector3.up)
                    * transform.rotation;
            }
            mode = 2;
            walking = false;
            arrived = false;
            blocked = false;
            havePath = false;
            haveHop = false;
            commandArrive = 0f;
            home = transform.position;
            target = home;
        }

        /// Freeze in place (standing at a station) or resume strolling.
        public void SetIdle(bool idle)
        {
            if (idle)
            {
                mode = 2;
                walking = false;
            }
            else if (mode == 2)
            {
                Stop();
            }
        }

        /// Move the stroll circle (an NPC that went indoors strolls around
        /// the room it landed in, not around its village spawn).
        public void SetHome(Vector3 pos)
        {
            home = pos;
            target = pos;
        }

        /// The stroll circle's centre - the brain restores it on the way out.
        public Vector3 Home()
        {
            return home;
        }

        /// Measured model height (metres) - the brain sizes its speech
        /// bubble from it instead of assuming a human-sized villager.
        public float Height()
        {
            return npcHeight;
        }

        /// True while the NPC is actually stepping (drives the walk clip).
        public bool Walking()
        {
            return walking;
        }

        /// The direction the MESH visibly faces, flattened to the ground
        /// plane. Measured through the anchor's transform chain, so it is
        /// steady while the body turns - a follower that differences the
        /// leader's positions instead jitters whenever the leader pivots
        /// in place.
        public Vector3 Facing()
        {
            return VisualForward();
        }

        // --- Facing measurement --------------------------------------------

        // The direction the mesh visibly faces, in world space, flattened to
        // the ground plane - read off the anchor node's full transform chain
        // (TransformPoint difference: rotation AND scale mirrors) every
        // frame, so baked rest yaw, idle sway, mirrors and importer
        // conversions are all accounted for by construction. Falls back to
        // the last good reading while the clip pitches the anchor too
        // vertical for a yaw to mean anything.
        Vector3 VisualForward()
        {
            if (anchor == null)
                return transform.forward;
            // anchorFaceLocal is the rest-calibrated face axis (see Start);
            // through the anchor's full matrix it tracks baked yaw, idle
            // sway and every mirror at once.
            Vector3 f = anchor.TransformPoint(anchorFaceLocal)
                - anchor.TransformPoint(Vector3.zero);
            if (flipFacing)
                f = -f;
            float m2 = f.sqrMagnitude;
            f.y = 0f;
            // Yaw is unreadable past ~72 degrees of pitch.
            if (m2 < 1e-12f || f.sqrMagnitude < 0.09f * m2)
                return lastForward;
            f = f.normalized;
            lastForward = f;
            return f;
        }

        // Servo one frame's worth of yaw toward `dir`; returns the absolute
        // alignment error in degrees BEFORE the step.
        float ServoToward(Vector3 dir)
        {
            float err = Vector3.SignedAngle(VisualForward(), dir, Vector3.up)
                + facingYawOffset;
            float step = Mathf.Clamp(err,
                -turnSpeed * Time.deltaTime, turnSpeed * Time.deltaTime);
            transform.rotation =
                Quaternion.AngleAxis(servoSign * step, Vector3.up)
                * transform.rotation;
            return Mathf.Abs(err);
        }

        // Follow the floor. The ray starts INSIDE the NPC's own capsule
        // (half height - PhysX never reports a shape a ray starts in), so
        // it can neither land the villager on its own collider nor miss a
        // step below waist height. Another villager's capsule is skipped
        // rather than stood on: two NPCs brushing past each other is not a
        // change of floor.
        void SnapToFloor()
        {
            Vector3 p = transform.position;
            RaycastHit ground;
            if (Physics.Raycast(p + Vector3.up * (0.5f * npcHeight), Vector3.down,
                    out ground, 3f * npcHeight, ~0, QueryTriggerInteraction.Ignore))
            {
                if (ground.collider.GetType() == typeof(CapsuleCollider))
                    return;
                p.y = ground.point.y;
                transform.position = p;
            }
        }

        bool PathClear(Vector3 dir, float dist)
        {
            RaycastHit hit;
            return !Physics.Raycast(transform.position + Vector3.up * rayHeight,
                dir, out hit, dist, ~0, QueryTriggerInteraction.Ignore);
        }

        // The same probe, recording WHAT it hit: `probeHitNpc` is true when
        // the obstacle is another villager's capsule rather than the world.
        // (No `out` parameter - Udon method signatures stay plain.)
        private bool probeBlocked;
        private bool probeHitNpc;

        void Probe(Vector3 dir, float dist)
        {
            RaycastHit hit;
            probeBlocked = Physics.Raycast(transform.position + Vector3.up * rayHeight,
                dir, out hit, dist, ~0, QueryTriggerInteraction.Ignore);
            probeHitNpc = probeBlocked &&
                hit.collider.GetType() == typeof(CapsuleCollider);
        }

        void Update()
        {
            if (mode == 1)
                CommandStep();
            else if (mode == 2)
                walking = false;
            else if (mode == 3)
                FaceStep();
            else if (mode == 5)
                SlideStep();
            else if (mode == 6)
                HopStep();
            else
                StrollStep();
            DriveAnimator();
        }

        // Idle / walk crossfade for the rigs that have a measured walk clip.
        void DriveAnimator()
        {
            if (locoAnimator == null)
                return;
            if (!animStarted)
            {
                animStarted = true;
                animWalking = walking;
                locoAnimator.Play(walking ? walkState : idleState, 0, 0f);
                return;
            }
            if (walking == animWalking)
                return;
            animWalking = walking;
            locoAnimator.CrossFade(walking ? walkState : idleState,
                animCrossfade, 0);
        }

        // --- Commanded walk -------------------------------------------------

        void CommandStep()
        {
            float arriveR = commandArrive < 0.05f ? arriveRadius : commandArrive;
            Vector3 to = LegTarget() - transform.position;
            to.y = 0f;
            float dist = to.magnitude;
            if (haveHop)
            {
                // The near side of a ledge link: stop walking and jump.
                if (dist < Mathf.Max(cornerRadius, 0.3f))
                {
                    StartHop();
                    return;
                }
            }
            else if (dist < arriveR)
            {
                arrived = true;
                walking = false;
                havePath = false;
                return;
            }
            // Head for the next route corner (or the leg's own target).
            Vector3 leg = CurrentAim() - transform.position;
            leg.y = 0f;
            float legDist = leg.magnitude;
            if (legDist < 1e-3f)
            {
                leg = to;
                legDist = dist;
            }
            Vector3 aim = leg / legDist;

            // FINAL APPROACH. Inside a stride of the target, with no route
            // corner left to turn, aim straight at it and stop looking for
            // lanes. A doorway is a gap narrower than the probe fan can
            // read as passable: every ray but the straight one hits the
            // frame, so the steering picks a lane, commits to it, re-aims,
            // picks the other lane - and the villager circles a step short
            // of its own front door forever. That is the "wandering
            // aimlessly near the house" the night routine looked like.
            bool lastLeg = !havePath || cornerIndex >= corners.Length - 1;
            bool finalApproach = lastLeg && dist < arriveR + 0.8f;

            // Steering: commit to a detour lane for a moment, then re-aim.
            //
            // ON A ROUTE, only another VILLAGER is worth a detour. The bake
            // used this villager's own radius and height, so the corners it
            // returns are walkable by construction - and sidestepping the
            // world anyway is what jammed a villager thirteen metres from
            // its door, shuffling between two lanes beside a hut wall the
            // route was going to round on its own. A wall the route walks
            // past is not an obstacle; a wall in the way of a straight-line
            // fallback still is, and so is anyone standing in the road.
            Vector3 dir = aim;
            if (finalApproach)
            {
                steerUntil = 0f;
            }
            else if (Time.time < steerUntil)
            {
                dir = steerDir;
            }
            else
            {
                Probe(aim, Mathf.Min(probeDistance, legDist + wallClearance));
                if (probeBlocked && (!havePath || probeHitNpc))
                {
                    dir = PickLane(aim);
                    if (dir.sqrMagnitude < 0.5f)
                    {
                        if (Replan())
                            return;
                        blocked = true;
                        walking = false;
                        return;
                    }
                    steerDir = dir;
                    steerUntil = Time.time + 0.7f;
                }
            }

            float abs = ServoToward(dir);
            if (!walking && abs > 3f)
                return;
            if (abs > 25f)
            {
                walking = false;
                return;
            }
            walking = true;
            transform.position += dir * (walkSpeed * Time.deltaTime);
            SnapToFloor();

            // Stall watchdog: a lane that keeps grinding along a wall makes
            // no progress; re-plan the route once from here, then report
            // Blocked so the brain picks something else instead of the NPC
            // shuffling forever.
            if (Time.time - progressAt > 2.5f)
            {
                if ((transform.position - progressMark).magnitude < 0.2f)
                {
                    if (!Replan())
                    {
                        blocked = true;
                        walking = false;
                    }
                }
                progressMark = transform.position;
                progressAt = Time.time;
            }

            // PROGRESS watchdog. The displacement test above cannot see a
            // steering livelock - a villager sidestepping between two lanes
            // covers plenty of ground. This one measures whether the walk
            // has come closer to the corner it is currently heading for.
            //
            // The corner, NOT the target: a route round the back of a hut
            // walks AWAY from the door for ten metres by design, and
            // measuring the target would call that a stall and abandon the
            // trip - which is what stranded the villager whose front door
            // sits on the raised hut. Every corner, by contrast, is
            // approached monotonically, and the record resets as each one
            // is turned.
            if (cornerIndex != watchedCorner)
            {
                watchedCorner = cornerIndex;
                bestDist = legDist;
                betterAt = Time.time;
            }
            else if (legDist < bestDist - 0.2f)
            {
                bestDist = legDist;
                betterAt = Time.time;
            }
            else if (Time.time - betterAt > 5f)
            {
                betterAt = Time.time;
                bestDist = legDist;
                if (!Replan())
                {
                    blocked = true;
                    walking = false;
                }
            }
        }

        // --- Ledge hop (mode 6) ----------------------------------------------

        void StartHop()
        {
            hopStart = Time.time;
            hopA = transform.position;
            mode = 6;
            walking = false;
            hops++;
            Vector3 d = hopB - hopA;
            d.y = 0f;
            if (d.sqrMagnitude > 1e-6f)
                faceDir = d.normalized;
        }

        // A plain parabola between the link's two ends: the NPC leaves the
        // ground, arcs `hopApex` over the higher end and lands on the far
        // side. No floor ray while airborne (it would drag the villager
        // back down onto the ledge it is clearing); one on landing.
        void HopStep()
        {
            float span = hopSeconds < 0.1f ? 0.1f : hopSeconds;
            float u = (Time.time - hopStart) / span;
            ServoToward(faceDir);
            if (u >= 1f)
            {
                transform.position = hopB;
                SnapToFloor();
                // The next leg: on to the following hop in the chain, or
                // the last stretch to the target itself.
                chainIndex++;
                if (chainIndex < chainCount)
                {
                    hopA = chainP[chainIndex];
                    hopB = chainQ[chainIndex];
                    haveHop = true;
                    PlanLegTo(hopA);
                }
                else
                {
                    haveHop = false;
                    PlanLegTo(commandTarget);
                }
                mode = 1;
                watchedCorner = -1;
                progressMark = transform.position;
                progressAt = Time.time;
                return;
            }
            Vector3 p = Vector3.Lerp(hopA, hopB, u);
            float top = (hopA.y > hopB.y ? hopA.y : hopB.y) + hopApex;
            // Height: the straight line plus a hump that peaks mid-flight
            // and reaches `top` there.
            float lineY = Mathf.Lerp(hopA.y, hopB.y, u);
            float hump = 4f * u * (1f - u);
            p.y = lineY + hump * (top - Mathf.Lerp(hopA.y, hopB.y, 0.5f));
            transform.position = p;
            walking = true;
        }

        // --- Scripted slide (mode 5) -------------------------------------------

        void SlideStep()
        {
            float u = (Time.time - slideStart) / slideSeconds;
            ServoToward(faceDir);
            if (u >= 1f)
            {
                transform.position = slideTo;
                SnapToFloor();
                arrived = true;
                walking = false;
                mode = 2;
                return;
            }
            transform.position = Vector3.Lerp(slideFrom, slideTo, u);
            SnapToFloor();
            walking = true;
        }

        // One fresh route from the current position; false when it has
        // already been tried for this command or the navmesh offers none.
        bool Replan()
        {
            if (replanned)
                return false;
            replanned = true;
            steerUntil = 0f;
            watchedCorner = -1;
            PlanRoute();
            progressMark = transform.position;
            progressAt = Time.time;
            return havePath;
        }

        // Fan of probe rays either side of the straight line; the first
        // clear lane wins, nearest the straight line first, alternating
        // sides so the NPC does not always dodge the same way.
        Vector3 PickLane(Vector3 aim)
        {
            float side = Random.value < 0.5f ? 1f : -1f;
            for (int i = 0; i < 3; i++)
            {
                float ang = 30f + i * 27.5f;
                Vector3 a = Quaternion.AngleAxis(ang * side, Vector3.up) * aim;
                if (PathClear(a, probeDistance))
                    return a;
                Vector3 b = Quaternion.AngleAxis(-ang * side, Vector3.up) * aim;
                if (PathClear(b, probeDistance))
                    return b;
            }
            return Vector3.zero;
        }

        // --- Turn in place ---------------------------------------------------

        void FaceStep()
        {
            walking = false;
            ServoToward(faceDir);
        }

        // --- Autonomous stroll ------------------------------------------------

        void StrollStep()
        {
            if (Time.time < pauseUntil)
            {
                walking = false;
                return;
            }

            Vector3 to = target - transform.position;
            to.y = 0;
            if (to.magnitude < 0.05f)
            {
                PickNextTarget();
                return;
            }
            Vector3 dir = to.normalized;

            // A wall within clearance directly ahead (waist height; the ray
            // starts inside the NPC's own capsule, which PhysX never reports
            // from the inside): rest, then stroll somewhere else.
            if (!PathClear(dir, wallClearance))
            {
                PickNextTarget();
                return;
            }

            // Servo the yaw until the MESH faces the walk direction.
            float abs = ServoToward(dir);

            // Turn in place first: no stepping until the body points down
            // the walk direction, so a direction change reads as a pivot
            // followed by a forward walk - never a strafe or moonwalk. Once
            // walking, only a gross misalignment (idle sway is compensated
            // live, but a swaying clip can outpace one frame's servo step)
            // pauses the stepping again.
            if (!walking && abs > 3f)
                return;
            if (abs > 25f)
            {
                walking = false;
                return;
            }
            walking = true;

            transform.position += dir * (speed * Time.deltaTime);

            // Follow the floor so a stroll across sloped ground doesn't
            // float or sink.
            SnapToFloor();
        }

        // Rest, then pick the next spot inside the circle - shortened to
        // stop short of the first collider on the way, so a stroll never
        // aims through a wall in the first place.
        void PickNextTarget()
        {
            pauseUntil = Time.time + Random.Range(0.5f, pauseSeconds * 2f);
            Vector2 r = Random.insideUnitCircle * radius;
            Vector3 cand = home + new Vector3(r.x, 0, r.y);
            // With a navmesh in the scene a stroll only ever aims at ground
            // the bake calls walkable: the spot is pulled onto the mesh,
            // and a candidate off it (over the shore, on a roof edge) is
            // skipped for this rest.
            if (NavAvailable())
            {
                NavMeshHit onMesh;
                if (!NavMesh.SamplePosition(cand, out onMesh, 0.35f, ALL_AREAS))
                {
                    target = transform.position;
                    return;
                }
                cand = onMesh.position;
            }
            Vector3 d = cand - transform.position;
            d.y = 0;
            float dist = d.magnitude;
            if (dist < 0.05f)
            {
                target = transform.position;
                return;
            }
            Vector3 dir = d / dist;
            RaycastHit hit;
            if (Physics.Raycast(transform.position + Vector3.up * rayHeight,
                    dir, out hit, dist, ~0, QueryTriggerInteraction.Ignore))
                dist = Mathf.Max(0f, hit.distance - wallClearance);
            target = transform.position + dir * dist;
        }
    }
}
