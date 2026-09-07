// The weapon hitbox on a villager: a trigger capsule wrapped round the
// NPC's rendered body, sized at build time from its own bounds, that
// turns a swung weapon into a slain villager and a pile of coins.
//
// Why a trigger and not a raycast: a VRChat pickup is a real physics
// object in the swinging player's hands, so the cheapest correct test is
// to let it cross the villager's volume and read the prop it belongs to.
// The capsule carries a kinematic Rigidbody of its own because Unity
// only reports trigger crossings when at least one side has a body, and
// the villager root is moved by a controller rather than by physics.
//
// WHO DECIDES. OnTriggerEnter fires on every client whose local copy of
// the villager the prop happens to overlap - but a held pickup is only
// where the HOLDER sees it. So the strike is gated on the prop being
// held by the LOCAL player: exactly one client passes, and that client
// calls brain.Slay, which broadcasts the outcome (and the coin value it
// rolled) to everybody. The `Stay` path is polled rather than run per
// contact frame, and a per-hitbox cooldown keeps one arc through the
// body from registering as several strikes.
//
// The debug entry points at the bottom are what the headless economy
// soak drives: a network-callable event never fires in editor play mode
// without a network, so the soak calls the brain's SlainAt directly with
// the position this hitbox would have reported.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcHitbox : UdonSharpBehaviour
    {
        [Tooltip("The villager this hitbox belongs to (builder-wired).")]
        public LegaiaNpcBrain brain;

        [Tooltip("The villager's root transform - where the coins land (builder-wired; the parent when empty).")]
        public Transform npcRoot;

        [Tooltip("Fewest / most coins a slain villager drops (the striking client rolls one value and it travels in the broadcast).")]
        public int coinsMin = 5;
        public int coinsMax = 15;

        [Tooltip("Seconds this hitbox ignores further contacts after a strike - one swing through the body is one strike.")]
        public float cooldownSeconds = 1.5f;

        [Tooltip("Seconds between OnTriggerStay tests (a resting weapon must not poll physics every frame).")]
        public float stayPollSeconds = 0.2f;

        private float freeAt;
        private float stayPollAt;

        void Start()
        {
            if (npcRoot == null)
                npcRoot = transform.parent;
        }

        void OnTriggerEnter(Collider other)
        {
            TryStrike(other);
        }

        void OnTriggerStay(Collider other)
        {
            // A weapon left resting against a villager would otherwise run
            // the whole test every physics step, per contact.
            if (Time.time < stayPollAt)
                return;
            stayPollAt = Time.time + stayPollSeconds;
            TryStrike(other);
        }

        void TryStrike(Collider other)
        {
            if (other == null || brain == null)
                return;
            if (Time.time < freeAt || brain.Dead())
                return;
            LegaiaPickupProp prop = other.GetComponent<LegaiaPickupProp>();
            if (prop == null)
                prop = other.GetComponentInParent<LegaiaPickupProp>();
            if (prop == null || !prop.weapon)
                return;
            // Only the holder's client passes: everybody else sees the prop
            // wherever the network last put it, which is not a swing.
            if (!prop.HeldByLocal())
                return;
            if (prop.SwingSpeed() < prop.minSwingSpeed)
                return;
            freeAt = Time.time + cooldownSeconds;
            int lo = coinsMin < 0 ? 0 : coinsMin;
            int hi = coinsMax < lo ? lo : coinsMax;
            brain.Slay(Random.Range(lo, hi + 1));
        }

        /// Test hook: strike this villager as if a weapon had connected
        /// (the broadcast path, so it needs a network to be seen).
        public void DebugStrike()
        {
            if (brain != null)
                brain.Slay(7);
        }

        /// Test hook for the headless soak, which has no network: run the
        /// broadcast's BODY directly, with the position the strike would
        /// have carried.
        public void DebugSlainHere()
        {
            if (brain == null)
                return;
            Vector3 p = npcRoot != null ? npcRoot.position : transform.position;
            brain.SlainAt(p.x, p.y, p.z, 7);
        }
    }
}
