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
// LOOP GUARD: several retail landings sit inside (or a capsule-width from)
// the PAIRED door's trigger - town01's hilltop house lands you inside its
// own exit band, and the cave mouth's return lands a step from the entry
// box - so a bare teleport ping-pongs the player between the pair (which
// reads as "the door does nothing" when the hops cancel out, or as a
// visible loop when they don't; retail is immune because its script doors
// re-arm on walk-away). Before teleporting, the firing doorway tells every
// sibling doorway under the same teleports root to ignore trigger entries
// for a few seconds if the landing is inside or near its box. Standing
// still never re-fires (only a fresh trigger ENTER does), so the window
// only has to outlive the landing's own physics-step enter event.
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

        private LegaiaDoorway[] siblings;
        private BoxCollider box;
        private float suppressUntil;

        void Start()
        {
            box = GetComponent<BoxCollider>();
            siblings = transform.parent != null
                ? transform.parent.GetComponentsInChildren<LegaiaDoorway>()
                : new LegaiaDoorway[0];
        }

        // A teleport just landed a player at `landing`: ignore trigger
        // entries for a moment if that spot is inside or a capsule-width
        // from this doorway's box, so the landing can't chain-fire us.
        public void SuppressNear(Vector3 landing)
        {
            if (box == null)
                return;
            Bounds b = box.bounds;
            b.Expand(1.5f); // 0.75 m per side: player capsule + imprecision
            if (b.Contains(landing))
                suppressUntil = Time.time + 3f;
        }

        // Stepping off the trigger re-arms it immediately (retail's own
        // walk-away re-arm): without this, leaving the box and walking
        // straight back within the timed window still did nothing.
        public override void OnPlayerTriggerExit(VRCPlayerApi player)
        {
            if (player != null && player.isLocal)
                suppressUntil = 0f;
        }

        public override void OnPlayerTriggerEnter(VRCPlayerApi player)
        {
            if (player == null || !player.isLocal || destination == null)
                return;
            if (Time.time < suppressUntil)
                return;
            Vector3 dest = destination.position;
            Quaternion rot = alignToDestination
                ? destination.rotation
                : player.GetRotation();
            if (siblings != null)
                for (int i = 0; i < siblings.Length; i++)
                    if (siblings[i] != null)
                        siblings[i].SuppressNear(dest);
            player.TeleportTo(dest, rot);
        }
    }
}
