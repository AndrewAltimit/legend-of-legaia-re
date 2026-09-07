// The coin-drop pool: twelve re-used LegaiaCoinDrop objects under
// <root>/living_town/coins, and the one entry point a slain villager
// reaches them through.
//
// THE CONTRACT with LegaiaNpcBrain (a loose UdonSharpBehaviour link, so
// the brain never needs this type): the brain writes `dropX`, `dropY`,
// `dropZ` and `dropCoins`, then sends `SpawnDrop`. It does that inside
// its own SlainAt, which is a network broadcast - so every client runs
// SpawnDrop with the SAME four values and picks the same drop out of the
// pool, and the coins are in one place for everybody without this
// behaviour syncing anything at all.
//
// The villager's own position is where it stood, which on a slope or a
// stair can be a hand's width inside the floor mesh; the spawn point is
// therefore re-snapped with a short downward ray and lifted clear.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaCoinDrops : UdonSharpBehaviour
    {
        [Tooltip("The pooled drops (twelve, builder-made).")]
        public LegaiaCoinDrop[] drops;

        [Tooltip("Seconds a drop stays on the ground before it leaves the pool.")]
        public float lifeSeconds = 90f;

        [Tooltip("Floor snap: metres above the reported spot the ray starts.")]
        public float probeUp = 2f;

        [Tooltip("Floor snap: metres below the reported spot the ray reaches.")]
        public float probeDown = 6f;

        [Tooltip("Metres above the floor the coin rests.")]
        public float clearance = 0.06f;

        [Tooltip("Seconds between expiry sweeps.")]
        public float sweepSeconds = 1f;

        // --- the brain's four-field contract ---------------------------------

        [Tooltip("Where the coins land (written by the brain before SpawnDrop).")]
        [HideInInspector] public float dropX;
        [HideInInspector] public float dropY;
        [HideInInspector] public float dropZ;

        [Tooltip("What the drop is worth (written by the brain before SpawnDrop).")]
        [HideInInspector] public int dropCoins;

        private float sweepAt;

        void Update()
        {
            if (drops == null || Time.time < sweepAt)
                return;
            sweepAt = Time.time + sweepSeconds;
            float now = Time.time;
            for (int i = 0; i < drops.Length; i++)
            {
                LegaiaCoinDrop d = drops[i];
                if (d != null && d.active && now - d.spawnedAt > lifeSeconds)
                    d.Despawn();
            }
        }

        /// The brain's event: put `dropCoins` on the ground at the spot it
        /// wrote. Called on every client with identical values.
        public void SpawnDrop()
        {
            if (drops == null || drops.Length == 0 || dropCoins <= 0)
                return;
            Vector3 p = new Vector3(dropX, dropY, dropZ);
            RaycastHit hit;
            if (Physics.Raycast(p + Vector3.up * probeUp, Vector3.down, out hit,
                    probeUp + probeDown, -1, QueryTriggerInteraction.Ignore))
                p.y = hit.point.y;
            p.y += clearance;

            int pick = -1;
            for (int i = 0; i < drops.Length; i++)
                if (drops[i] != null && !drops[i].active)
                {
                    pick = i;
                    break;
                }
            if (pick < 0)
            {
                // All twelve busy: the one that has been lying about
                // longest gives up its place.
                float oldest = 0f;
                for (int i = 0; i < drops.Length; i++)
                {
                    if (drops[i] == null)
                        continue;
                    if (pick < 0 || drops[i].spawnedAt < oldest)
                    {
                        pick = i;
                        oldest = drops[i].spawnedAt;
                    }
                }
            }
            if (pick < 0)
                return;
            drops[pick].Spawn(p.x, p.y, p.z, dropCoins);
        }

        /// How many drops are lying on the ground (what the checks read).
        public int ActiveCount()
        {
            if (drops == null)
                return 0;
            int n = 0;
            for (int i = 0; i < drops.Length; i++)
                if (drops[i] != null && drops[i].active)
                    n++;
            return n;
        }
    }
}
