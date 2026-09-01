// Optional Udon behaviour: makes a placed NPC wander a small radius around
// its spawn point, pausing between strolls - the "town feels inhabited"
// layer on top of the looping idle clip the builder wires up.
//
// Collision-aware: strolls are clamped against the world's colliders (the
// builder's merged double-sided collider included), a short waist-height
// ray stops a walk that would clip a wall, and the NPC follows the floor
// with a downward ray - so villagers no longer amble through huts. Trigger
// colliders (doorway teleports, door-approach boxes) are ignored, so a
// wandering NPC neither blocks on them nor fires them.
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

        private Vector3 home;
        private Vector3 target;
        private float pauseUntil;

        void Start()
        {
            home = transform.position;
            target = home;
            pauseUntil = Time.time + Random.Range(0f, pauseSeconds);
        }

        void Update()
        {
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

            Quaternion face = Quaternion.LookRotation(dir, Vector3.up);
            transform.rotation = Quaternion.RotateTowards(
                transform.rotation, face, turnSpeed * Time.deltaTime);
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
