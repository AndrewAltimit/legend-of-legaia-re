// The handler behind a shoreline LegaiaNpcStation (kind 1): while a
// villager stands here, it holds a rod out over the water with a line
// down to a bobbing float, ripple rings spread where the float sits, and
// every now and then something bites.
//
// The NPC rigs are rigid-node models with no hand bone (see the wander
// behaviour's facing note), so the rod is NOT parented to the NPC: it
// lives under the station and is positioned each frame relative to
// `station.currentNpc` at hand height - measured from the NPC's own
// rendered bounds at arrival, so it tracks any export scale.
//
// The station calls us:
//   OnNpcArrive  - show the gear, size it to this NPC, start fishing
//   OnNpcLeave   - hide it again
// Everything here is cosmetic and per-client (no sync): two players may
// see the float dip on different seconds, which nobody can tell.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaFishingSpot : UdonSharpBehaviour
    {
        [Tooltip("The station this handles (builder-wired; found on this object when empty).")]
        public LegaiaNpcStation station;

        [Tooltip("Container holding rod + line + float + ripples - hidden while nobody fishes.")]
        public GameObject gear;

        [Tooltip("The rod (a thin cylinder, +Y along its length).")]
        public Transform rod;

        [Tooltip("Empty at the rod's far tip - the line hangs from here.")]
        public Transform rodTip;

        [Tooltip("The float bobbing on the water.")]
        public Transform bobber;

        [Tooltip("Line from the rod tip to the float (world-space, 2 positions).")]
        public LineRenderer line;

        [Tooltip("Ripple-ring particles at the float (played while fishing).")]
        public ParticleSystem ripples;

        [Tooltip("A small fish quad that flashes out of the water on a catch.")]
        public Transform fish;

        [Tooltip("Water surface height in world space (the builder measures the sheet).")]
        public float waterY;

        [Tooltip("How far out over the water the float lands, metres.")]
        public float castDistance = 2.2f;

        [Tooltip("Hand height as a fraction of the NPC's measured height.")]
        public float handHeightFraction = 0.55f;

        [Tooltip("Shortest / longest wait before a bite, seconds.")]
        public float minBiteSeconds = 20f;
        public float maxBiteSeconds = 60f;

        [Tooltip("Free the station while a player stands within this radius of the stand point (0 = never).")]
        public float playerBlockRadius = 1f;

        private bool fishing;
        private float npcHeight = 1.6f;
        private Vector3 castPoint;
        private float nextBite;
        private float biteUntil;
        private float catchUntil;
        private float playerClock;
        private bool blocked;

        void Start()
        {
            if (station == null)
                station = GetComponent<LegaiaNpcStation>();
            if (gear != null)
                gear.SetActive(false);
            if (fish != null)
                fish.gameObject.SetActive(false);
        }

        public void OnNpcArrive()
        {
            if (station == null || station.currentNpc == null)
                return;
            npcHeight = MeasureHeight(station.currentNpc);
            Vector3 stand = station.StandPosition();
            Vector3 fwd = station.StandForward();
            castPoint = new Vector3(
                stand.x + fwd.x * castDistance, waterY, stand.z + fwd.z * castDistance);
            if (bobber != null)
                bobber.position = castPoint;
            if (gear != null)
                gear.SetActive(true);
            if (ripples != null)
                ripples.Play();
            fishing = true;
            nextBite = Time.time + Random.Range(minBiteSeconds, maxBiteSeconds);
            biteUntil = 0f;
            catchUntil = 0f;
        }

        public void OnNpcLeave()
        {
            fishing = false;
            if (ripples != null)
                ripples.Stop();
            if (gear != null)
                gear.SetActive(false);
            if (fish != null)
                fish.gameObject.SetActive(false);
        }

        void Update()
        {
            BlockForPlayers();
            if (!fishing || station == null)
                return;
            Transform npc = station.currentNpc;
            if (npc == null)
            {
                // The brain released the station without the leave event
                // reaching us (a despawn, a reload) - clean up anyway.
                OnNpcLeave();
                return;
            }

            float t = Time.time;
            bool biting = t < biteUntil;
            // Rod: held at hand height, angled up and out over the water.
            if (rod != null)
            {
                Vector3 fwd = station.StandForward();
                Vector3 hand = npc.position + Vector3.up * (npcHeight * handHeightFraction)
                             + fwd * (npcHeight * 0.18f);
                rod.position = hand;
                float jerk = biting ? -22f : 0f;
                // +Y is the rod's length: tip it forward off vertical.
                rod.rotation = Quaternion.LookRotation(fwd, Vector3.up) *
                               Quaternion.Euler(58f + jerk, 0f, 0f);
            }
            // Float: a slow bob, a sharp dip while something is on.
            if (bobber != null)
            {
                float y = waterY + Mathf.Sin(t * 1.7f) * 0.018f
                        + Mathf.Sin(t * 0.63f) * 0.01f;
                if (biting)
                    y -= 0.055f + Mathf.Abs(Mathf.Sin(t * 14f)) * 0.02f;
                bobber.position = new Vector3(castPoint.x, y, castPoint.z);
            }
            if (line != null && rodTip != null && bobber != null)
            {
                line.SetPosition(0, rodTip.position);
                line.SetPosition(1, bobber.position);
            }
            // The catch: the float dips, then a fish flashes up on the line.
            if (t > nextBite && !biting && t > catchUntil)
            {
                biteUntil = t + 1.1f;
                catchUntil = t + 2.0f;
                nextBite = t + Random.Range(minBiteSeconds, maxBiteSeconds);
            }
            if (fish != null)
            {
                bool show = t > biteUntil && t < catchUntil;
                if (show != fish.gameObject.activeSelf)
                    fish.gameObject.SetActive(show);
                if (show && bobber != null)
                {
                    float k = 1f - (catchUntil - t) / 0.9f; // 0..1 over the arc
                    Vector3 p = Vector3.Lerp(bobber.position,
                        rodTip != null ? rodTip.position : bobber.position, k * 0.55f);
                    fish.position = p + Vector3.up * (Mathf.Sin(k * Mathf.PI) * 0.5f);
                    fish.rotation = Quaternion.Euler(0f, t * 220f, 25f);
                }
            }
        }

        // Keep an NPC from fishing through a player standing on the spot.
        void BlockForPlayers()
        {
            if (playerBlockRadius <= 0f || station == null)
                return;
            playerClock += Time.deltaTime;
            if (playerClock < 0.4f)
                return;
            playerClock = 0f;
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null)
                return;
            Vector3 d = local.GetPosition() - station.StandPosition();
            d.y = 0f;
            bool near = d.sqrMagnitude < playerBlockRadius * playerBlockRadius;
            if (near == blocked)
                return;
            blocked = near;
            // Only ever hand back what we took: an unavailable station the
            // brain is already using stays that way until it releases.
            station.available = !near;
        }

        float MeasureHeight(Transform npc)
        {
            Renderer[] rs = npc.GetComponentsInChildren<Renderer>();
            if (rs.Length == 0)
                return 1.6f;
            Bounds b = rs[0].bounds;
            for (int i = 1; i < rs.Length; i++)
                b.Encapsulate(rs[i].bounds);
            return Mathf.Clamp(b.size.y, 0.4f, 4f);
        }
    }
}
