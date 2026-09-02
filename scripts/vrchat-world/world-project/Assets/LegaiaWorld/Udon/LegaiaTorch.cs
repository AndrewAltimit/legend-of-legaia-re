// Udon behaviour for the carry-able torches and campfires the builder
// places near spawn: pick one up and press Use (left click / trigger)
// to light or snuff it. The flame container (particles + point light +
// glow) and the crackle AudioSource start off; `lit` is synced, so a
// torch someone lights burns for everyone, and the pickup's Object Sync
// carries its position.
//
// Spawn-kinematic like the equipment rack (LegaiaPickupProp): a dynamic
// body waking during world-load hitches can tunnel through the
// paper-thin PSX ground and respawn-loop, so the body only goes
// physical on first drop.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Continuous)]
    public class LegaiaTorch : UdonSharpBehaviour
    {
        [Tooltip("Container holding the flame visuals (particles, glow, point light) - inactive while unlit.")]
        public GameObject flame;

        [Tooltip("Looping fire-crackle AudioSource (spatial), played only while lit.")]
        public AudioSource crackle;

        [UdonSynced]
        public bool lit;

        private bool shown;
        private Rigidbody body;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            if (body != null)
                body.isKinematic = true;
            shown = !lit; // force the first Apply
            Apply();
        }

        public override void OnDrop()
        {
            if (body != null)
                body.isKinematic = false;
        }

        public override void OnPickupUseDown()
        {
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            lit = !lit;
            Apply();
        }

        public override void OnDeserialization()
        {
            Apply();
        }

        void Apply()
        {
            if (shown == lit)
                return;
            shown = lit;
            if (flame != null)
                flame.SetActive(lit);
            if (crackle != null)
            {
                if (lit)
                    crackle.Play();
                else
                    crackle.Stop();
            }
        }
    }
}
