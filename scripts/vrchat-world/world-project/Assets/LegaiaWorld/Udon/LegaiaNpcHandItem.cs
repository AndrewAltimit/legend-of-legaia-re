// The handler behind a carry / errand station (LegaiaNpcStation kind 5):
// the thing a villager PICKS UP here, or PUTS DOWN here.
//
// It is the station contract's ordinary handler shape - the same one the
// cupboard doors and the fishing spots use - so the town director never
// learns that errands exist: it hands out a free station by kind the way
// it always did, the brain walks its villager there and calls Arrive(),
// and this behaviour reaches into the arriving NPC's own carry rig
// (LegaiaNpcCarry, on the NPC root) and swaps the item in its hand.
//
//   itemKind >= 0, dropItem false - the villager leaves holding this
//   dropItem true                 - the villager sets down whatever it held
//   action 1                      - it WORKS with the item while it stands
//                                   here (the carry rig's sway)
//
// `keepOnLeave` is what makes a carry errand read as one errand rather
// than as two unrelated visits: the bucket stays in hand while the
// villager walks to the next stop, and only the stop with `dropItem` (or
// the end of the itinerary, which the brain handles) empties the hands.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcHandItem : UdonSharpBehaviour
    {
        [Tooltip("The station this handles (builder-wired; found on this object when empty).")]
        public LegaiaNpcStation station;

        [Tooltip("Item handed over here: 0 bucket, 1 broom, 2 firewood, 3 basket. -1 = none.")]
        public int itemKind = -1;

        [Tooltip("The villager puts down whatever it is carrying when it arrives here.")]
        public bool dropItem;

        [Tooltip("It walks on still holding the item (a fetch errand's first stop).")]
        public bool keepOnLeave = true;

        [Tooltip("What it does with the item while standing here: 0 hold, 1 work (sway).")]
        public int action;

        void Start()
        {
            if (station == null)
                station = GetComponent<LegaiaNpcStation>();
        }

        public void OnNpcArrive()
        {
            LegaiaNpcCarry carry = CarryOf();
            if (carry == null)
                return;
            if (dropItem)
                carry.Hide();
            else if (itemKind >= 0)
                carry.Show(itemKind, action);
        }

        public void OnNpcLeave()
        {
            if (keepOnLeave)
                return;
            LegaiaNpcCarry carry = CarryOf();
            if (carry != null)
                carry.Hide();
        }

        // The arriving NPC's carry rig. The station sets `currentNpc` (the
        // NPC ROOT transform) before either event, and the carry behaviour
        // lives on that root next to the brain - so no search into the
        // rig's node tree is needed, which is what keeps this cheap.
        LegaiaNpcCarry CarryOf()
        {
            if (station == null || station.currentNpc == null)
                return null;
            return station.currentNpc.GetComponent<LegaiaNpcCarry>();
        }
    }
}
