// A seat: Interact sits the local player in this object's VRCStation
// (the same wire as the SDK's VRCChair3 sample, in UdonSharp). The card
// table's stools use it; the station itself carries the seated pose and
// the enter / exit transforms the builder authors.
//
// It also reports occupancy: VRChat exposes no "is this station in use"
// query, but OnStationEntered / OnStationExited fire on every client for
// every player, so `occupied` is a correct local mirror of the chair's
// state. The card table's NPC host polls it to decide whether players
// are using the table (LegaiaCardTableHost).
//
// `occupantId` names WHICH player, for the same reason and by the same
// mechanism: the card game needs to know which seat the local player is
// in (to route its button presses) and which player a seat's bets belong
// to (to settle its coins). It is a local mirror on every client - the
// callbacks fire everywhere - so nothing about it is synced.
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

        [Tooltip("True while any player sits here (maintained from the station callbacks).")]
        [HideInInspector] public bool occupied;

        [Tooltip("playerId of whoever sits here, -1 when empty (maintained from the station callbacks).")]
        [HideInInspector] public int occupantId = -1;

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

        public override void OnStationEntered(VRCPlayerApi player)
        {
            occupied = true;
            occupantId = player != null && player.IsValid() ? player.playerId : -1;
        }

        public override void OnStationExited(VRCPlayerApi player)
        {
            occupied = false;
            occupantId = -1;
        }
    }
}
