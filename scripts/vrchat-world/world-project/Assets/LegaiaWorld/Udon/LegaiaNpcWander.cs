// Optional Udon behaviour: makes a placed NPC wander a small radius around
// its spawn point, pausing between strolls - the "town feels inhabited"
// layer on top of the looping idle clip the builder wires up.
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
                // Arrived: rest, then pick the next spot inside the circle.
                pauseUntil = Time.time + Random.Range(0.5f, pauseSeconds * 2f);
                Vector2 r = Random.insideUnitCircle * radius;
                target = home + new Vector3(r.x, 0, r.y);
                return;
            }

            Quaternion face = Quaternion.LookRotation(to.normalized, Vector3.up);
            transform.rotation = Quaternion.RotateTowards(
                transform.rotation, face, turnSpeed * Time.deltaTime);
            transform.position += to.normalized * (speed * Time.deltaTime);
        }
    }
}
