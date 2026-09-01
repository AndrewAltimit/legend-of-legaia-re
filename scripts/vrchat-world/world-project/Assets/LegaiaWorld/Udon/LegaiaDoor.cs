// Proximity door: the first time any player walks into this trigger, the
// door's Animator plays its swing clip once and holds the open pose. Retail
// door meshes carry their swing as the object-bind clip and retail advances
// it only while the door record's script runs - a free-running loop would
// swing forever, so the builder wires door-tagged props (`is_door` /
// `near_portal` in the manifest) through this instead.
//
// The open state is per client, not networked: it opens when a player's
// collider (local or remote) enters, so clients converge as soon as anyone
// approaches; a late joiner sees it closed until the next approach.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    public class LegaiaDoor : UdonSharpBehaviour
    {
        [Tooltip("Animator whose controller has a 'closed' default state (clip parked at frame 0) and an 'open' state (the swing clip, loop off). The builder generates it.")]
        public Animator doorAnimator;

        [Tooltip("State to play once on first approach; with loop off the Animator holds its last frame, so the door stays open.")]
        public string openStateName = "open";

        private bool opened;

        public override void OnPlayerTriggerEnter(VRCPlayerApi player)
        {
            if (opened || doorAnimator == null)
                return;
            opened = true;
            doorAnimator.Play(openStateName, 0, 0f);
        }
    }
}
