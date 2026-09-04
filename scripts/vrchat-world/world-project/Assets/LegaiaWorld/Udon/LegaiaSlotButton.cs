// One of the slot cabinet's three face buttons: forwards Interact to the
// machine with its reel index. In the idle state any button charges the bet
// and spins (retail: any face button); while the reels run, button 0/1/2
// stops reel 0/1/2 (retail pad bits 0x80/0x40/0x20); in the payout state any
// button collects early.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaSlotButton : UdonSharpBehaviour
    {
        [Tooltip("The cabinet's machine behaviour.")]
        public LegaiaSlotMachine machine;

        [Tooltip("Reel this button stops (0 left, 1 middle, 2 right).")]
        public int buttonIndex;

        public override void Interact()
        {
            if (machine != null)
                machine.PressButton(buttonIndex);
        }
    }
}
