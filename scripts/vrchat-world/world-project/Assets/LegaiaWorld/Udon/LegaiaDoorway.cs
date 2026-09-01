// Doorway teleport: walking into this trigger repositions the local player
// at the destination marker - the retail intra-scene door mechanism (Rim
// Elm's house doors put the interior in an unused corner of the same map,
// so "entering a house" is a teleport, not a scene change; see
// docs/subsystems/field-locomotion.md).
//
// The builder creates one of these per manifest `teleports[]` entry (and per
// wired `scene_portals[]` entry when the target scene is present in the same
// Unity scene): a trigger BoxCollider sized like the retail trigger tile /
// contact box, plus a `dest` child marker whose position is the retail
// landing (floor-height sampled) and whose rotation is the authored arrival
// facing.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    public class LegaiaDoorway : UdonSharpBehaviour
    {
        [Tooltip("Landing marker: player teleports to its position (and its facing when Align to destination is on).")]
        public Transform destination;

        [Tooltip("Rotate the player to the marker's facing on arrival (retail doors author an arrival facing so you never emerge staring at the door you just used). Off = keep the walked-in facing.")]
        public bool alignToDestination = true;

        public override void OnPlayerTriggerEnter(VRCPlayerApi player)
        {
            if (player == null || !player.isLocal || destination == null)
                return;
            Quaternion rot = alignToDestination
                ? destination.rotation
                : player.GetRotation();
            player.TeleportTo(destination.position, rot);
        }
    }
}
