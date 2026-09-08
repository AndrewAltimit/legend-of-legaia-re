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
// --- Animation ---------------------------------------------------------
//
// Everything that MOVES is the `visual` child; the ROOT is always at the
// pose the caller asked for, on the frame it asked. That is not a style
// choice - the game (LegaiaCardGame) reads card positions to decide what
// is on the felt, the table host measures cards against the deck anchor,
// and the Object Sync serialises the root. A root that eased toward its
// target would make every one of those read a card that is not there
// yet. So `Deal` teleports the root and hands the CHILD the offset the
// card just travelled, which then eases to zero: the card appears to
// slide across the felt while the simulation already sees it landed.
//
// The flip is driven by `faceUp` CHANGING, not by whoever called it:
// the owner's direct call and every other client's OnDeserialization
// enter the same BeginFlip, so the animation plays locally on each
// client off its own clock and never depends on the owner's frame
// timing. The deal slide is owner-side only - remote clients see the
// Object Sync's own interpolation to the new spot, which is the same
// travel by a different road; there is no per-frame position poll here
// to reconstruct it, because that would cost 52 always-awake Updates.
//
// Update returns on the first line while both animations are idle
// (`flipT`/`slideT` negative), the same shape LegaiaSpeechBubble uses -
// Udon has no coroutines, and 52 cards make a cheap idle path matter.
//
// A card a player is HOLDING skips both animations and snaps: the visual
// child's offset is measured in the ROOT's frame, and while VRC Pickup
// drives that root from a moving hand the offset would read as a card
// swimming inside the player's fist.
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

        [Tooltip("Seconds a face flip takes (the dealer assumes the card has settled inside half a second).")]
        public float flipSeconds = 0.45f;

        [Tooltip("How far the card lifts off the felt at mid-flip, metres.")]
        public float flipLift = 0.02f;

        [Tooltip("Seconds a dealt card takes to slide from where it was to where it now is.")]
        public float dealSeconds = 0.3f;

        [Tooltip("How high the slide arcs above the straight line, as a fraction of the distance travelled.")]
        public float dealArc = 0.12f;

        [UdonSynced]
        public bool faceUp;

        private Rigidbody body;
        private VRCObjectSync sync;
        private VRCPickup pickup;
        private bool shownFaceUp = true;

        // Where the card mesh sits when nothing is animating (whatever the
        // builder authored - read once, never assumed to be the origin).
        private Vector3 restLocal;

        // Flip: t in [0,1] while running, negative when idle. `flipFrom` is
        // the face the animation starts on, so the mesh turns from where it
        // visibly is even when faceUp changes twice inside one flip.
        private float flipT = -1f;
        private bool flipFrom;

        // Slide: the root-local offset the mesh starts at (where the card
        // came FROM, expressed in the root's new frame) easing to zero.
        private float slideT = -1f;
        private Vector3 slideFrom;
        private float slideRise;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            sync = GetComponent<VRCObjectSync>();
            pickup = GetComponent<VRCPickup>();
            if (visual != null)
                restLocal = visual.localPosition;
            // Local write on purpose: Object Sync's SetKinematic is an
            // owner-only call, and at Start most cards belong to someone else.
            if (body != null)
                body.isKinematic = true;
            Settle();
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

        bool Held()
        {
            return pickup != null && pickup.IsHeld;
        }

        /// Put the mesh exactly where the current `faceUp` says, with no
        /// animation left running. Park, Start and every held card use it.
        void Settle()
        {
            flipT = -1f;
            slideT = -1f;
            if (visual == null)
                return;
            shownFaceUp = faceUp;
            visual.localRotation = faceUp
                ? Quaternion.identity
                : Quaternion.Euler(0f, 0f, 180f);
            visual.localPosition = restLocal;
        }

        /// Start (or restart) the turn-over animation, unless the shown face
        /// already matches - this is the ONE entry point for a face change,
        /// reached identically from the owner's write and from every other
        /// client's OnDeserialization.
        void BeginFlip()
        {
            if (visual == null || shownFaceUp == faceUp)
                return;
            if (Held() || flipSeconds <= 0f)
            {
                shownFaceUp = faceUp;
                visual.localRotation = faceUp
                    ? Quaternion.identity
                    : Quaternion.Euler(0f, 0f, 180f);
                return;
            }
            flipFrom = shownFaceUp;
            shownFaceUp = faceUp;
            flipT = 0f;
        }

        void Update()
        {
            // Idle: one comparison and out, 52 times a frame.
            if (flipT < 0f && slideT < 0f)
                return;
            if (visual == null)
            {
                flipT = -1f;
                slideT = -1f;
                return;
            }
            // A card that ended up in someone's hand mid-animation lands
            // now: the offsets below are measured in the root's frame, and
            // that frame is being flown around by the pickup.
            if (Held())
            {
                Settle();
                return;
            }

            float dt = Time.deltaTime;
            Vector3 pos = restLocal;

            if (flipT >= 0f)
            {
                flipT += dt / Mathf.Max(0.01f, flipSeconds);
                float u = flipT >= 1f ? 1f : Ease(flipT);
                float from = flipFrom ? 0f : 180f;
                float to = shownFaceUp ? 0f : 180f;
                // Raw Lerp, not LerpAngle: 0 -> 180 and 180 -> 0 must both
                // pass through the edge-on 90 degrees, which is the flip.
                visual.localRotation = Quaternion.Euler(0f, 0f, Mathf.Lerp(from, to, u));
                // Off the felt at mid-turn and back down - a card pivoting
                // through its own thickness looks like it is cutting the
                // table in half.
                pos = pos + Vector3.up * (flipLift * Mathf.Sin(u * Mathf.PI));
                if (flipT >= 1f)
                {
                    flipT = -1f;
                    visual.localRotation = shownFaceUp
                        ? Quaternion.identity
                        : Quaternion.Euler(0f, 0f, 180f);
                }
            }

            if (slideT >= 0f)
            {
                slideT += dt / Mathf.Max(0.01f, dealSeconds);
                float u = slideT >= 1f ? 1f : Ease(slideT);
                pos = pos + Vector3.Lerp(slideFrom, Vector3.zero, u) +
                      Vector3.up * (slideRise * Mathf.Sin(u * Mathf.PI));
                if (slideT >= 1f)
                    slideT = -1f;
            }

            visual.localPosition = pos;
        }

        /// Smoothstep: the flip and the slide both start and end at rest.
        float Ease(float t)
        {
            if (t <= 0f) return 0f;
            if (t >= 1f) return 1f;
            return t * t * (3f - 2f * t);
        }

        public override void OnDeserialization()
        {
            BeginFlip();
        }

        public override void OnDrop()
        {
            SetKinematic(false);
            Settle();
        }

        public override void OnPickup()
        {
            Settle();
        }

        public override void OnPickupUseDown()
        {
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            faceUp = !faceUp;
            BeginFlip();
        }

        /// Called by the dealer (LegaiaCardGame, which takes ownership
        /// first): put the card on a hand anchor, frozen, showing the side
        /// the game wants. Same as Park but the face is a parameter - a
        /// player's cards are dealt face up, a villager's face down.
        ///
        /// The ROOT lands on `position`/`rotation` this frame; only the
        /// mesh child travels, from where the card was to where it now is.
        public void Deal(Vector3 position, Quaternion rotation, bool up)
        {
            SetKinematic(true);
            Vector3 was = transform.position;
            transform.SetPositionAndRotation(position, rotation);
            if (sync != null)
                sync.FlagDiscontinuity();

            if (visual != null && !Held() && dealSeconds > 0f)
            {
                // Root-LOCAL, and measured after the move: the mesh is a
                // child, so its offset has to be expressed in the frame it
                // now hangs from.
                Vector3 offset = transform.InverseTransformPoint(was);
                float travel = offset.magnitude;
                if (travel > 0.01f && travel < 20f)
                {
                    slideFrom = offset;
                    slideRise = travel * Mathf.Max(0f, dealArc);
                    slideT = 0f;
                }
                else
                {
                    slideT = -1f;
                    visual.localPosition = restLocal;
                }
            }

            faceUp = up;
            BeginFlip();
        }

        /// Showdown: turn this card over where it lies. Continuous sync
        /// sends `faceUp` on its own from the owner (there is no
        /// RequestSerialization under Continuous), so the owner-side write
        /// plus BeginFlip is the whole flip; every other client starts the
        /// same animation from OnDeserialization when the bool lands.
        public void Reveal()
        {
            faceUp = true;
            BeginFlip();
        }

        /// Called by the deck (which owns this card at that moment): drop
        /// the card face-down at a stack position, frozen. Instant on
        /// purpose - Shuffle restacks all 52 in one call, and 52 cards
        /// flying across the table at once is a card storm, not a shuffle.
        public void Park(Vector3 position, Quaternion rotation)
        {
            SetKinematic(true);
            transform.SetPositionAndRotation(position, rotation);
            if (sync != null)
                sync.FlagDiscontinuity();
            faceUp = false;
            Settle();
        }
    }
}
