// Optional Udon behaviour: makes a placed NPC wander a small radius around
// its spawn point, pausing between strolls - the "town feels inhabited"
// layer on top of the looping idle clip the builder wires up.
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
//
// The one convention that must be ASSUMED is which node-local axis the
// mesh's face points down, and it is -Z, pinned empirically: with +Z the
// entire town walked backwards in-world - a uniform 180 - while wireframe
// renders of every rig cannot distinguish front from back (a projected
// silhouette is identical both ways). -Z is also consistent with history:
// the builder's own negative-Z instance mirror flips file -Z onto the
// transform's +Z, which is why the old LookRotation-on-transform code
// looked correct on every rest-yaw-0 rig.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK via the Creator
// Companion). Drop this component on an NPC instance the builder placed;
// tune radius/speed per NPC. Movement is local (each player computes it
// independently from the same deterministic-ish seed of Random) - fine for
// ambience; use synced variables if you need every player to agree.

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    public class LegaiaNpcWander : UdonSharpBehaviour
    {
        [Tooltip("How far from the spawn point the NPC strolls (meters). " +
                 "Default suits the 1 m-per-tile export scale.")]
        public float radius = 1.5f;

        [Tooltip("Walk speed in m/s. Legaia townsfolk amble - keep it low.")]
        public float speed = 0.4f;

        [Tooltip("Average pause between strolls (seconds).")]
        public float pauseSeconds = 5f;

        [Tooltip("Turn rate toward the walk direction (degrees/second).")]
        public float turnSpeed = 240f;

        [Tooltip("Clear space kept between the NPC and any wall (meters).")]
        public float wallClearance = 0.3f;

        [Tooltip("Tick if this NPC's mesh is authored facing +Z instead of " +
                 "-Z (walks exactly backwards with the automatic facing).")]
        public bool flipFacing = false;

        [Tooltip("Extra facing correction in degrees, added on top of the " +
                 "measured visual forward - for hand-tuning one NPC.")]
        public float facingYawOffset = 0f;

        private Vector3 home;
        private Vector3 target;
        private float pauseUntil;
        private Transform anchor;
        private float servoSign = 1f;
        private bool walking;
        private Vector3 lastForward = Vector3.forward;
        // Measured at Start from the rendered rest pose, so the wall and
        // floor rays stay proportioned to the model at any export scale.
        private float npcHeight = 1.6f;
        private float rayHeight = 0.8f;

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
        }

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
            // -Z is the face axis of these meshes (empirically pinned: +Z
            // walked the whole town backwards; see the header note).
            Vector3 f = anchor.TransformPoint(Vector3.back)
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

        void Update()
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
            if (Physics.Raycast(transform.position + Vector3.up * rayHeight,
                    dir, out RaycastHit blocked, wallClearance,
                    ~0, QueryTriggerInteraction.Ignore))
            {
                PickNextTarget();
                return;
            }

            // Servo the yaw until the MESH faces the walk direction.
            float err = Vector3.SignedAngle(VisualForward(), dir, Vector3.up)
                + facingYawOffset;
            float step = Mathf.Clamp(err,
                -turnSpeed * Time.deltaTime, turnSpeed * Time.deltaTime);
            transform.rotation =
                Quaternion.AngleAxis(servoSign * step, Vector3.up)
                * transform.rotation;

            // Turn in place first: no stepping until the body points down
            // the walk direction, so a direction change reads as a pivot
            // followed by a forward walk - never a strafe or moonwalk. Once
            // walking, only a gross misalignment (idle sway is compensated
            // live, but a swaying clip can outpace one frame's servo step)
            // pauses the stepping again.
            float abs = Mathf.Abs(err);
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
            Vector3 p = transform.position;
            if (Physics.Raycast(p + Vector3.up * npcHeight, Vector3.down,
                    out RaycastHit ground, 3f * npcHeight,
                    ~0, QueryTriggerInteraction.Ignore))
            {
                p.y = ground.point.y;
                transform.position = p;
            }
        }

        // Rest, then pick the next spot inside the circle - shortened to
        // stop short of the first collider on the way, so a stroll never
        // aims through a wall in the first place.
        void PickNextTarget()
        {
            pauseUntil = Time.time + Random.Range(0.5f, pauseSeconds * 2f);
            Vector2 r = Random.insideUnitCircle * radius;
            Vector3 cand = home + new Vector3(r.x, 0, r.y);
            Vector3 d = cand - transform.position;
            d.y = 0;
            float dist = d.magnitude;
            if (dist < 0.05f)
            {
                target = transform.position;
                return;
            }
            Vector3 dir = d / dist;
            if (Physics.Raycast(transform.position + Vector3.up * rayHeight,
                    dir, out RaycastHit hit, dist,
                    ~0, QueryTriggerInteraction.Ignore))
                dist = Mathf.Max(0f, hit.distance - wallClearance);
            target = transform.position + dir * dist;
        }
    }
}
