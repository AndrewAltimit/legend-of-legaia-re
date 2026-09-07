// Udon behaviour for the equipment-rack pickups: spawn frozen, go
// physical on first drop - and, for the pieces off the WEAPON rows, be a
// weapon a villager can actually be struck with.
//
// A rack of several dozen dynamic Rigidbodies all waking during world
// load is a physics hazard: the load hitches stretch the frame steps,
// the bodies pick up fall speed, tunnel through the paper-thin PSX
// ground mesh, hit the respawn height, get put back, and loop. So each
// prop starts kinematic - rock-solid on its rack, no simulation at all -
// and only becomes a free physics object the first time a player drops
// it. While held, VRC Pickup drives the body; OnDrop hands it to gravity.
//
// THE SWING. The bounty layer (LegaiaNpcHitbox) needs to tell a swing
// from a carry, and Unity's Rigidbody velocity is no use here: while a
// VRC Pickup is held the body is kinematic and its reported velocity is
// whatever the last physics step left behind. So the speed is measured
// the only way that is true on every rig - a per-frame position delta -
// and only while the local player is the one holding it, so a rack of
// idle props costs nothing per frame. The reading decays rather than
// resetting, holding the peak of a swing for a few frames, because the
// trigger crossing usually lands a frame or two after the fastest part
// of the arc.
//
// `weapon` is set at build time from the item manifest's section label
// (the "Weapon" rows), so a shield or a helmet is carried and thrown
// like any other prop but never counts as a strike.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK). The builder's
// "Place equipment rack near spawn" attaches this next to the VRC Pickup.

using UdonSharp;
using UnityEngine;
using VRC.SDK3.Components;
using VRC.SDKBase;

namespace LegaiaWorld
{
    public class LegaiaPickupProp : UdonSharpBehaviour
    {
        [Tooltip("This piece is a weapon: swung fast enough it strikes a villager down (LegaiaNpcHitbox). Builder-set from the manifest's section label.")]
        public bool weapon;

        [Tooltip("Metres per second the held prop must be moving for a strike to count.")]
        public float minSwingSpeed = 2.5f;

        [Tooltip("How much of last frame's reading is held when the prop slows - keeps the peak of a swing alive across the contact frame.")]
        public float swingDecay = 0.6f;

        private Rigidbody body;
        private VRCPickup pickup;
        private Vector3 lastPos;
        private float swingSpeed;
        private bool tracking;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            if (body != null)
                body.isKinematic = true;
            pickup = GetComponent<VRCPickup>();
        }

        void Update()
        {
            if (!HeldByLocal())
            {
                // Nothing to measure, and nothing to pay for: a rack of
                // untouched props leaves this branch every frame.
                if (tracking)
                {
                    tracking = false;
                    swingSpeed = 0f;
                }
                return;
            }
            Vector3 p = transform.position;
            if (!tracking)
            {
                tracking = true;
                lastPos = p;
                swingSpeed = 0f;
                return;
            }
            float dt = Time.deltaTime;
            if (dt > 1e-4f)
            {
                float v = (p - lastPos).magnitude / dt;
                float held = swingSpeed * swingDecay;
                swingSpeed = v > held ? v : held;
            }
            lastPos = p;
        }

        /// The local player is holding this prop right now.
        public bool HeldByLocal()
        {
            return pickup != null && pickup.IsHeld &&
                   pickup.currentPlayer != null && pickup.currentPlayer.isLocal;
        }

        /// How fast the held prop is travelling, metres per second (0 when
        /// it is not in the local player's hands).
        public float SwingSpeed()
        {
            return swingSpeed;
        }

        public override void OnDrop()
        {
            tracking = false;
            swingSpeed = 0f;
            if (body != null)
                body.isKinematic = false;
        }
    }
}
