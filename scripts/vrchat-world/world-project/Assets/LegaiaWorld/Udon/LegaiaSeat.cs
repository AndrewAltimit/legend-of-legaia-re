// A seat: Interact sits the local player in this object's VRCStation
// (the same wire as the SDK's VRCChair3 sample, in UdonSharp). The card
// table's stools use it; the station itself carries the seated pose and
// the enter / exit transforms the builder authors.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaSeat : UdonSharpBehaviour
    {
        [Tooltip("The station on this stool (builder-wired; found on this object when left empty).")]
        public VRCStation station;

        void Start()
        {
            if (station == null)
                station = GetComponent<VRCStation>();
            InteractionText = "Sit";
        }

        public override void Interact()
        {
            if (station != null)
                station.UseStation(Networking.LocalPlayer);
        }
    }
}
