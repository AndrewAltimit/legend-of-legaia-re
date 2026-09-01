// Udon behaviour for the equipment-rack pickups: spawn frozen, go
// physical on first drop.
//
// A rack of several dozen dynamic Rigidbodies all waking during world
// load is a physics hazard: the load hitches stretch the frame steps,
// the bodies pick up fall speed, tunnel through the paper-thin PSX
// ground mesh, hit the respawn height, get put back, and loop. So each
// prop starts kinematic - rock-solid on its rack, no simulation at all -
// and only becomes a free physics object the first time a player drops
// it. While held, VRC Pickup drives the body; OnDrop hands it to gravity.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK). The builder's
// "Place equipment rack near spawn" attaches this next to the VRC Pickup.

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    public class LegaiaPickupProp : UdonSharpBehaviour
    {
        private Rigidbody body;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            if (body != null)
                body.isKinematic = true;
        }

        public override void OnDrop()
        {
            if (body != null)
                body.isKinematic = false;
        }
    }
}
