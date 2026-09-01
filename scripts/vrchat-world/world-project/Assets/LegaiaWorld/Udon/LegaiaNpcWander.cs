// Optional Udon behaviour: makes a placed NPC wander a small radius around
// its spawn point, pausing between strolls - the "town feels inhabited"
// layer on top of the looping idle clip the builder wires up.
//
// Collision-aware: strolls are clamped against the world's colliders (the
// builder's merged double-sided collider included), a short waist-height
// ray stops a walk that would clip a wall, and the NPC follows the floor
// with a downward ray - so villagers no longer amble through huts. Trigger
// colliders (doorway teleports, door-approach boxes) are ignored, so a
// wandering NPC neither blocks on them nor fires them. Facing is
// mirror-aware: the builder's instances carry scale mirrors that decouple
// the mesh's visual forward from the transform's +Z, and the walk facing
// maps through those signs (see Start) so villagers face the way they walk.
// Movement is forward-only: on a direction change the NPC pivots in place
// until aligned, then steps off - it never translates while mis-facing.
// Some spawn clips bake a facing yaw into the skeleton itself (town01's
// spawn_record_17 holds every top bone at -90deg; other idles sway bones
// over the loop), which turns the mesh under the transform and defeats
// any transform-only facing math. The behaviour tracks it live: it
// captures every top-level bone's rest rotation in Start (before the
// Animator's first evaluation), then every walking frame measures the
// yaw the clip is currently applying - the circular mean across the top
// bones, so limb swings cancel - and subtracts it from the walk facing
// (see UpdateClipYaw).
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
        [Tooltip("How far from the spawn point the NPC strolls (meters).")]
        public float radius = 3f;

        [Tooltip("Walk speed in m/s. Legaia townsfolk amble - keep it low.")]
        public float speed = 0.8f;

        [Tooltip("Average pause between strolls (seconds).")]
        public float pauseSeconds = 5f;

        [Tooltip("Turn rate toward the walk direction (degrees/second).")]
        public float turnSpeed = 240f;

        [Tooltip("Clear space kept between the NPC and any wall (meters).")]
        public float wallClearance = 0.45f;

        [Tooltip("Tick if villagers stroll backwards on your import stack - " +
                 "the facing math assumes the model faces +Z in its own file.")]
        public bool flipFacing = false;

        [Tooltip("Extra facing correction in degrees, added on top of the " +
                 "automatic clip-yaw calibration - for hand-tuning one NPC.")]
        public float facingYawOffset = 0f;

        private Vector3 home;
        private Vector3 target;
        private float pauseUntil;
        private float faceX = 1f;
        private float faceZ = 1f;
        private Transform[] faceBones;
        private Quaternion[] faceBoneRest;
        private int calibrateAfterFrame;
        private float clipYaw;

        void Start()
        {
            home = transform.position;
            target = home;
            pauseUntil = Time.time + Random.Range(0f, pauseSeconds);

            // The builder places NPC instances with a negative-Z local scale
            // (the handedness mirror) under a root that usually carries a
            // negative-X mirror. Mirrors in the scale chain mean the model's
            // VISUAL forward is not the transform's +Z - aiming LookRotation
            // at the walk direction then reads as backwards (or mirrored)
            // walking. With the mirrors diagonal, visual forward
            // = R * (sign(parent.x) * sign(local.z) * x, y,
            //        sign(parent.z) * sign(local.z) * z)-remap of +Z, so
            // pre-mapping the walk direction through those signs makes the
            // MESH face the way it walks, whatever the mirror combination.
            Vector3 ps = transform.parent != null
                ? transform.parent.lossyScale : Vector3.one;
            float fz = Mathf.Sign(transform.localScale.z)
                * (flipFacing ? -1f : 1f);
            faceX = Mathf.Sign(ps.x) * fz;
            faceZ = Mathf.Sign(ps.z) * fz;

            // Rest-pose anchors for the clip-yaw tracking: every TOP-LEVEL
            // bone (a bone whose parent is not itself a bone), captured
            // before the Animator's first evaluation overwrites them with
            // the spawn clip's pose. These rigs are flat - several
            // top-level nodes animated independently - so no single bone
            // is "the body": a limb node can swing 80deg while the body
            // holds still, and anchoring on one bone (rootBone) injected
            // that limb's animation into the walk facing on rigs where the
            // binding happened to point there.
            var smr = GetComponentInChildren<SkinnedMeshRenderer>();
            if (smr != null)
            {
                Transform[] bones = smr.bones;
                int nTop = 0;
                for (int i = 0; i < bones.Length; i++)
                    if (IsTopBone(bones, i))
                        nTop++;
                faceBones = new Transform[nTop];
                faceBoneRest = new Quaternion[nTop];
                int k = 0;
                for (int i = 0; i < bones.Length; i++)
                {
                    if (!IsTopBone(bones, i))
                        continue;
                    faceBones[k] = bones[i];
                    faceBoneRest[k] = bones[i].localRotation;
                    k++;
                }
            }
            calibrateAfterFrame = Time.frameCount + 2;
        }

        bool IsTopBone(Transform[] bones, int i)
        {
            Transform b = bones[i];
            if (b == null)
                return false;
            Transform p = b.parent;
            for (int j = 0; j < bones.Length; j++)
                if (j != i && bones[j] == p)
                    return false;
            return true;
        }

        // Some spawn clips pose the whole skeleton at a yaw of their own
        // (the authored facing lives in the ANM record, not the placement),
        // and that yaw is NOT always constant - idles sway or turn bones
        // over the loop. So the compensation is tracked live, every frame
        // while walking, not calibrated once. The measured quantity is the
        // yaw common to ALL top-level bones - the baked facing offsets
        // every one of them equally (town01's spawn_record_17 holds all
        // four at -90deg), while limb swings point different ways and
        // cancel in the circular mean. Each bone's rest-to-current delta
        // lives in its parent frame, whose vertical is flipped by the glb
        // root's Rx(180) (the up-dot sign) and whose yaw sense each
        // horizontal mirror in our own scale conjugates.
        void UpdateClipYaw()
        {
            clipYaw = 0f;
            if (faceBones == null || faceBones.Length == 0)
                return;
            Vector3 acc = Vector3.zero;
            Transform first = null;
            for (int i = 0; i < faceBones.Length; i++)
            {
                Transform b = faceBones[i];
                if (b == null)
                    continue;
                if (first == null)
                    first = b;
                Quaternion d = b.localRotation
                    * Quaternion.Inverse(faceBoneRest[i]);
                Vector3 f = d * Vector3.forward;
                f.y = 0f;
                // A pitch-dominated delta has no reliable yaw reading.
                if (f.sqrMagnitude < 0.25f)
                    continue;
                acc += f.normalized;
            }
            if (first == null || acc.sqrMagnitude < 0.25f)
                return;
            float local = Vector3.SignedAngle(Vector3.forward, acc, Vector3.up);
            float sVert = Mathf.Sign(
                Vector3.Dot(first.parent.up, transform.up));
            float sMirror = Mathf.Sign(
                transform.localScale.x * transform.localScale.z);
            clipYaw = local * sVert * sMirror;
        }

        void Update()
        {
            // Wait for the Animator's first evaluation so the rest-pose
            // anchor captured in Start is meaningfully different from the
            // animated pose.
            if (Time.frameCount < calibrateAfterFrame)
                return;

            if (Time.time < pauseUntil)
                return;

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
            if (Physics.Raycast(transform.position + Vector3.up * 0.9f, dir,
                    out RaycastHit blocked, wallClearance,
                    ~0, QueryTriggerInteraction.Ignore))
            {
                PickNextTarget();
                return;
            }

            UpdateClipYaw();
            Quaternion face = Quaternion.LookRotation(
                new Vector3(dir.x * faceX, 0f, dir.z * faceZ), Vector3.up)
                * Quaternion.AngleAxis(
                    -(clipYaw + facingYawOffset), Vector3.up);
            transform.rotation = Quaternion.RotateTowards(
                transform.rotation, face, turnSpeed * Time.deltaTime);

            // Turn in place first: no stepping until the body points down
            // the walk direction, so a direction change reads as a pivot
            // followed by a forward walk - never a strafe or moonwalk.
            if (Quaternion.Angle(transform.rotation, face) > 3f)
                return;

            transform.position += dir * (speed * Time.deltaTime);

            // Follow the floor so a stroll across sloped ground doesn't
            // float or sink.
            Vector3 p = transform.position;
            if (Physics.Raycast(p + Vector3.up * 1.5f, Vector3.down,
                    out RaycastHit ground, 4f,
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
            if (Physics.Raycast(transform.position + Vector3.up * 0.9f, dir,
                    out RaycastHit hit, dist,
                    ~0, QueryTriggerInteraction.Ignore))
                dist = Mathf.Max(0f, hit.distance - wallClearance);
            target = transform.position + dir * dist;
        }
    }
}
