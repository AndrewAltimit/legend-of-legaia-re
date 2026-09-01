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
// Some spawn clips bake a constant facing yaw into the skeleton itself
// (town01's spawn_record_17 holds every top bone at -90deg), which turns
// the mesh under the transform and defeats any transform-only facing
// math. The behaviour self-calibrates: it captures the skeleton root's
// rest rotation in Start (before the Animator's first evaluation),
// measures the yaw the clip applied two frames later, and subtracts it
// from the walk facing (see CalibrateClipYaw).
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
        private Transform faceRef;
        private Quaternion faceRefRest;
        private int calibrateAfterFrame;
        private bool calibrated;
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

            // Rest-pose anchor for the clip-yaw calibration: the skinned
            // skeleton's root bone, captured before the Animator's first
            // evaluation overwrites it with the spawn clip's pose.
            var smr = GetComponentInChildren<SkinnedMeshRenderer>();
            if (smr != null && smr.rootBone != null)
            {
                faceRef = smr.rootBone;
                faceRefRest = faceRef.localRotation;
            }
            calibrateAfterFrame = Time.frameCount + 2;
        }

        // Some spawn clips pose the whole skeleton at a constant yaw (the
        // authored facing lives in the ANM record, not the placement), so
        // the mesh faces off-axis from the transform. Measure that yaw as
        // the root bone's rotation delta between rest and the animated
        // pose, then map it into the transform's frame: the delta lives in
        // the bone's parent frame, whose vertical is flipped by the glb
        // root's Rx(180) (the up-dot sign) and whose yaw sense each
        // horizontal mirror in our own scale conjugates.
        void CalibrateClipYaw()
        {
            calibrated = true;
            clipYaw = 0f;
            if (faceRef == null)
                return;
            Quaternion d = faceRef.localRotation
                * Quaternion.Inverse(faceRefRest);
            Vector3 f = d * Vector3.forward;
            f.y = 0f;
            if (f.sqrMagnitude < 1e-4f)
                return;
            float local = Vector3.SignedAngle(Vector3.forward, f, Vector3.up);
            float sVert = Mathf.Sign(
                Vector3.Dot(faceRef.parent.up, transform.up));
            float sMirror = Mathf.Sign(
                transform.localScale.x * transform.localScale.z);
            clipYaw = local * sVert * sMirror;
        }

        void Update()
        {
            if (!calibrated)
            {
                if (Time.frameCount < calibrateAfterFrame)
                    return;
                CalibrateClipYaw();
            }

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
