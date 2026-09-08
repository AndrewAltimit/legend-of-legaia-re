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
//   OnNpcLeave  -> NpcClose()   the same clip scrubbed back to frame 0
//
// WHY THE CLIP IS SCRUBBED BY HAND. There is one swing clip and closing
// is it played backwards - but `Animator.speed` may not be negative at
// runtime: Unity rejects the assignment and logs "Animator.speed can
// only be negative when Animator recorder is enabled". The door then sat
// on the clamped last frame of a non-looping state, which is the OPEN
// pose, so a door a villager opened never shut and every close spent a
// warning saying so. Instead the Animator is parked at speed 0 and this
// behaviour walks the state's normalized time itself, one frame at a
// time, which runs in both directions and costs nothing while no door is
// moving.
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

        [Tooltip("Seconds the swing takes; the builder sets it from the clip's own length.")]
        public float swingSeconds = 0.7f;

        private bool opened;
        private bool playerOpened;
        private int npcUsers;
        private float swingT;    // 0 = closed (frame 0), 1 = open (last frame)
        private float targetT;
        private bool swinging;

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

        // The swing clip is the only motion the door has, and it runs both
        // ways: frame 0 is shut, the last frame is open. See the header for
        // why the time is walked by hand rather than played at speed -1.
        void Swing(bool open)
        {
            if (doorAnimator == null)
                return;
            if (open == opened)
                return;
            opened = open;
            targetT = open ? 1f : 0f;
            doorAnimator.speed = 0f;
            if (swinging)
                return; // the step loop is already running; it will turn round
            swinging = true;
            SwingStep();
        }

        /// One frame of the swing. Re-arms itself until the door reaches the
        /// pose it is heading for, then stops - so a town of doors costs
        /// nothing while they all stand still.
        public void SwingStep()
        {
            if (doorAnimator == null)
            {
                swinging = false;
                return;
            }
            float per = swingSeconds < 0.05f ? 0.05f : swingSeconds;
            swingT = Mathf.MoveTowards(swingT, targetT, Time.deltaTime / per);
            doorAnimator.Play(openStateName, 0, swingT);
            if (swingT == targetT)
            {
                swinging = false;
                return;
            }
            SendCustomEventDelayedFrames(nameof(SwingStep), 1);
        }
    }
}
