// One playing card of the kit's deck: a pickup whose face flips on Use.
//
// The pickup root never rotates on its own - VRC Pickup owns the root
// while a card is held - so the flip lives on the `visual` child (the
// card mesh), rotated 180 degrees about its long axis. Desktop players
// cannot turn a held object, which is why the flip is a button and not
// just "turn it over". `faceUp` is synced so a card someone turns shows
// the same side to everyone; the position rides on the VRC Object Sync
// the builder adds next to this. That Object Sync is why the sync mode
// is Continuous, not Manual: the SDK's world validator refuses an
// Object Sync on the same object as a manually synchronized behaviour
// ("Object Sync cannot share an object with a manually synchronized
// Udon Behaviour"), and Continuous sends the bool on its own - the
// same pairing LegaiaTorch uses.
//
// Spawn-kinematic like the equipment rack (LegaiaPickupProp): the deck
// starts as a stack of 52 bodies, and a stack waking during a world-load
// hitch is exactly the tunnel-through-the-floor hazard. A card goes
// physical the first time it is dropped, and the deck's Shuffle / Gather
// re-parks it kinematic in the stack.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDK3.Components;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Continuous)]
    public class LegaiaCard : UdonSharpBehaviour
    {
        [Tooltip("The card mesh child - rotated 180 degrees about its long axis when face down.")]
        public Transform visual;

        [UdonSynced]
        public bool faceUp;

        private Rigidbody body;
        private VRCObjectSync sync;
        private bool shownFaceUp = true;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            sync = GetComponent<VRCObjectSync>();
            // Local write on purpose: Object Sync's SetKinematic is an
            // owner-only call, and at Start most cards belong to someone else.
            if (body != null)
                body.isKinematic = true;
            ApplyFace();
        }

        /// Owner-side kinematic switch (OnDrop and Park both run on the
        /// owner): Object Sync mirrors the flag to every client, where a
        /// bare Rigidbody write would only change it here.
        void SetKinematic(bool k)
        {
            if (sync != null)
                sync.SetKinematic(k);
            else if (body != null)
                body.isKinematic = k;
        }

        void ApplyFace()
        {
            if (visual == null || shownFaceUp == faceUp)
                return;
            shownFaceUp = faceUp;
            visual.localRotation = faceUp
                ? Quaternion.identity
                : Quaternion.Euler(0f, 0f, 180f);
        }

        public override void OnDeserialization()
        {
            ApplyFace();
        }

        public override void OnDrop()
        {
            SetKinematic(false);
        }

        public override void OnPickupUseDown()
        {
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            faceUp = !faceUp;
            ApplyFace();
        }

        /// Called by the deck (which owns this card at that moment): drop
        /// the card face-down at a stack position, frozen.
        public void Park(Vector3 position, Quaternion rotation)
        {
            SetKinematic(true);
            transform.SetPositionAndRotation(position, rotation);
            if (sync != null)
                sync.FlagDiscontinuity();
            faceUp = false;
            ApplyFace();
        }
    }
}
