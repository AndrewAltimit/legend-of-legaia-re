// One coin lying on the ground: what a slain villager leaves behind.
// Twelve of these sit in a pool (LegaiaCoinDrops) and are re-used; a
// drop is never created or destroyed at runtime.
//
// The GameObject stays ACTIVE for the world's whole life and only its
// renderer and its collider are toggled, because Udon never delivers an
// event into a behaviour on an inactive object - a pooled drop that
// switched itself off could never be told to spawn again.
//
// WHO GETS PAID. Interact broadcasts `TakenBy(playerId, serial)` to
// everyone instead of paying locally. Network events arrive in ONE
// server order on every client, so two players reaching for the same
// coin resolve identically everywhere: the first message wins, the
// second finds the drop already spent and pays nobody. The `serial`
// rides along because a message can outlive its spawn - a coin taken,
// recycled and respawned in the meantime would otherwise be paid twice
// off one click. Only the taker's own client calls wallet.Add, which is
// also the persistence model: a purse is only ever written by its owner.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDK3.UdonNetworkCalling;
using VRC.SDKBase;
using VRC.Udon.Common.Interfaces;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaCoinDrop : UdonSharpBehaviour
    {
        [Tooltip("The world's coin purse (builder-wired; resolved by path in Start when the pass ran before the prefabs).")]
        public LegaiaWallet wallet;

        [Tooltip("The coin cylinder - shown only while this drop is on the ground.")]
        public MeshRenderer visual;

        [Tooltip("The Interact box - enabled only while this drop is on the ground.")]
        public BoxCollider hitBox;

        [Tooltip("What this drop is worth (set per spawn by the pool).")]
        public int value;

        [Tooltip("Bumped on every spawn, so a late take for an older spawn is ignored.")]
        public int serial;

        [Tooltip("Degrees per second the coin turns on the ground.")]
        public float spinDegrees = 110f;

        [Tooltip("Bob height of the hover, metres.")]
        public float bobHeight = 0.04f;

        [Tooltip("Bob rate of the hover, cycles per second.")]
        public float bobSpeed = 1.6f;

        [Tooltip("True while this drop is lying on the ground (the pool reads it).")]
        [HideInInspector] public bool active;

        [Tooltip("Time.time of the current spawn (the pool recycles the oldest).")]
        [HideInInspector] public float spawnedAt;

        private Vector3 restPos;

        void Start()
        {
            InteractionText = "Take coins";
            if (wallet == null)
            {
                GameObject g = GameObject.Find("Legaia_common_prefabs/wallet");
                if (g != null)
                    wallet = g.GetComponent<LegaiaWallet>();
            }
            Show(false);
        }

        void Update()
        {
            if (!active)
                return;
            float lift = Mathf.Sin(Time.time * bobSpeed * Mathf.PI * 2f) * bobHeight
                       + bobHeight;
            transform.position = restPos + Vector3.up * lift;
            transform.rotation = Quaternion.Euler(0f, Time.time * spinDegrees, 0f);
        }

        /// Put this drop on the ground at a spot the pool has already
        /// floor-snapped. Runs on every client with the same arguments.
        public void Spawn(float x, float y, float z, int coins)
        {
            serial++;
            value = coins;
            active = true;
            spawnedAt = Time.time;
            restPos = new Vector3(x, y, z);
            transform.position = restPos;
            Show(true);
        }

        /// Take this drop off the ground without paying anybody (expiry,
        /// or a recycle when all twelve are busy).
        public void Despawn()
        {
            active = false;
            Show(false);
        }

        public override void Interact()
        {
            if (!active)
                return;
            VRCPlayerApi local = Networking.LocalPlayer;
            int id = local != null ? local.playerId : 0;
            SendCustomNetworkEvent(NetworkEventTarget.All, nameof(TakenBy),
                id, serial);
        }

        [NetworkCallable]
        public void TakenBy(int playerId, int s)
        {
            if (!active || s != serial)
                return;
            active = false;
            Show(false);
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local != null && playerId == local.playerId && wallet != null)
                wallet.Add(value);
        }

        void Show(bool on)
        {
            if (visual != null)
                visual.enabled = on;
            if (hitBox != null)
                hitBox.enabled = on;
        }
    }
}
