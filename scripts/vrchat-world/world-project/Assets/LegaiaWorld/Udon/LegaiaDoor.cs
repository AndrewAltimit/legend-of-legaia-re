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
// NPC USE (the living-town layer): a use-prop station (LegaiaNpcStation
// kind 0) in front of a cupboard / drawer / interior door names this
// behaviour as its `handler`, so a villager standing there opens it and
// closes it again on the way out:
//
//   OnNpcArrive -> NpcOpen()    swing forward, hold open
//   OnNpcLeave  -> NpcClose()   same clip at speed -1, back to closed
//
// The two paths do not fight: a PLAYER-opened door stays open forever (the
// original behaviour - `playerOpened` latches), and NpcClose only closes a
// door no player has opened and no other NPC is still using (`npcUsers`
// counts them, so two villagers at the same cupboard close it once).
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
        private bool playerOpened;
        private int npcUsers;

        public override void OnPlayerTriggerEnter(VRCPlayerApi player)
        {
            playerOpened = true;
            Swing(true);
        }

        /// A villager arrived at the use-prop station in front of this door.
        public void NpcOpen()
        {
            npcUsers++;
            Swing(true);
        }

        /// The villager walked off. The door swings shut unless a player
        /// opened it (retail's own "you opened it, it stays open") or
        /// another villager is still standing at it.
        public void NpcClose()
        {
            npcUsers--;
            if (npcUsers < 0)
                npcUsers = 0;
            if (playerOpened || npcUsers > 0)
                return;
            Swing(false);
        }

        /// The station contract's arrive / leave events (LegaiaNpcStation
        /// sends these by name to its `handler`).
        public void OnNpcArrive()
        {
            NpcOpen();
        }

        public void OnNpcLeave()
        {
            NpcClose();
        }

        // The swing clip is the only motion the door has: forward at speed
        // +1 from frame 0, back at speed -1 from the last frame. A
        // non-looping Animator state clamps at the end it is heading for,
        // which is exactly "hold open" / "hold closed".
        void Swing(bool open)
        {
            if (doorAnimator == null)
                return;
            if (open == opened)
                return;
            opened = open;
            doorAnimator.speed = open ? 1f : -1f;
            doorAnimator.Play(openStateName, 0, open ? 0f : 1f);
        }
    }
}
