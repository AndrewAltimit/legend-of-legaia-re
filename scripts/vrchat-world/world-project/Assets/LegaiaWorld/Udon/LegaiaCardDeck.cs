// The card table's deck controller: Shuffle stacks every card face-down
// at the deck anchor in a fresh random order, Gather stacks them in
// their fixed order (a "new deck"). Both act by taking ownership of each
// card and parking it - card positions travel on the cards' own Object
// Sync, so there is no per-deck position state to agree on. The shuffle
// seed is synced only so a late joiner's deck reports the same order in
// the log; the layout itself is already theirs through the cards.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaCardDeck : UdonSharpBehaviour
    {
        [Tooltip("Every card of the deck (builder-wired).")]
        public LegaiaCard[] cards;

        [Tooltip("Where the stack's bottom card sits; the stack grows along its up axis.")]
        public Transform stackAnchor;

        [Tooltip("Vertical gap between stacked cards, metres.")]
        public float cardSpacing = 0.0007f;

        [UdonSynced]
        public int lastSeed;

        public void Shuffle()
        {
            int n = cards == null ? 0 : cards.Length;
            if (n == 0 || stackAnchor == null)
                return;
            TakeOwnership();
            lastSeed = Random.Range(1, int.MaxValue);
            RequestSerialization();

            // Fisher-Yates over a fresh index array, driven by a small LCG
            // seeded from lastSeed (self-contained: the order is
            // reproducible from the seed alone, no engine RNG state).
            int[] order = new int[n];
            for (int i = 0; i < n; i++)
                order[i] = i;
            // int arithmetic only: Udon exposes no uint modulo. Overflow
            // wraps (Udon has no checked arithmetic), which is the LCG.
            int state = lastSeed;
            for (int i = n - 1; i > 0; i--)
            {
                state = state * 1664525 + 1013904223;
                int r = (state >> 8) & 0x7FFFFFFF;
                int j = r % (i + 1);
                int t = order[i];
                order[i] = order[j];
                order[j] = t;
            }
            Lay(order);
        }

        public void Gather()
        {
            int n = cards == null ? 0 : cards.Length;
            if (n == 0 || stackAnchor == null)
                return;
            TakeOwnership();
            int[] order = new int[n];
            for (int i = 0; i < n; i++)
                order[i] = i;
            Lay(order);
        }

        void TakeOwnership()
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null)
                return;
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(local, gameObject);
        }

        void Lay(int[] order)
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            Vector3 up = stackAnchor.up;
            Quaternion rot = stackAnchor.rotation;
            for (int slot = 0; slot < order.Length; slot++)
            {
                LegaiaCard card = cards[order[slot]];
                if (card == null)
                    continue;
                if (local != null && !Networking.IsOwner(card.gameObject))
                    Networking.SetOwner(local, card.gameObject);
                // A tiny yaw scatter reads as a real stack instead of a
                // rendered slab.
                Quaternion r = rot * Quaternion.Euler(0f, (slot * 37 % 7 - 3) * 0.6f, 0f);
                card.Park(stackAnchor.position + up * (cardSpacing * slot), r);
            }
        }
    }
}
