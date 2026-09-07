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
// STEERING (commanded walks only): the local reactive layer under the
// route. When the line to the next corner is blocked within
// `probeDistance` (another villager's capsule, a player-moved prop), a
// fan of rays at +/-30, +/-60, +/-85 degrees looks for a clear lane and
// the NPC commits to it for a moment before re-aiming. When no lane is
// clear, or progress stalls, the route is re-planned once from where the
// NPC stands; failing that, Blocked() goes true and the brain picks
// something else (it never teleports through the wall).
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
        // toward `faceDir` and then hold.
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
            commandTarget = worldPos;
            mode = 1;
            arrived = false;
            blocked = false;
            steerUntil = 0f;
            replanned = false;
            progressMark = transform.position;
            progressAt = Time.time;
            PlanRoute();
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
            cornerIndex = 0;
            if (!NavAvailable())
                return;
            NavMeshHit from, to;
            if (!NavMesh.SamplePosition(transform.position, out from, navSnapRadius, ALL_AREAS))
                return;
            if (!NavMesh.SamplePosition(commandTarget, out to, navSnapRadius, ALL_AREAS))
                return;
            if (path == null)
                path = new NavMeshPath();
            if (!NavMesh.CalculatePath(from.position, to.position, ALL_AREAS, path))
                return;
            if (path.status == NavMeshPathStatus.PathInvalid)
                return;
            corners = path.corners;
            if (corners == null || corners.Length < 2)
                return;
            // corners[0] is where the NPC already stands.
            cornerIndex = 1;
            havePath = true;
        }

        /// The point the walk is currently heading for: the next route
        /// corner, or the target itself on a straight-line walk / the last
        /// leg. Advances past corners as they are reached.
        Vector3 CurrentAim()
        {
            if (!havePath)
                return commandTarget;
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
            return cornerIndex >= last ? commandTarget : corners[cornerIndex];
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

        void Update()
        {
            if (mode == 1)
                CommandStep();
            else if (mode == 2)
                walking = false;
            else if (mode == 3)
                FaceStep();
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
            Vector3 to = commandTarget - transform.position;
            to.y = 0f;
            float dist = to.magnitude;
            if (dist < arriveRadius)
            {
                arrived = true;
                walking = false;
                havePath = false;
                return;
            }
            // Head for the next route corner (or the target itself).
            Vector3 leg = CurrentAim() - transform.position;
            leg.y = 0f;
            float legDist = leg.magnitude;
            if (legDist < 1e-3f)
            {
                leg = to;
                legDist = dist;
            }
            Vector3 aim = leg / legDist;

            // Steering: commit to a detour lane for a moment, then re-aim.
            Vector3 dir = aim;
            if (Time.time < steerUntil)
            {
                dir = steerDir;
            }
            else if (!PathClear(aim, Mathf.Min(probeDistance, legDist + wallClearance)))
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
        }

        // One fresh route from the current position; false when it has
        // already been tried for this command or the navmesh offers none.
        bool Replan()
        {
            if (replanned)
                return false;
            replanned = true;
            steerUntil = 0f;
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
