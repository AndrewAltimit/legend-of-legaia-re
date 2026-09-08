// The handler behind a VISIT station (LegaiaNpcStation kind 4 with a host,
// built as kind 6): a stand spot in front of one of the village's fixed
// residents - the ones retail authored standing still and the scene
// settings keep static - so a walking villager has somewhere to go and
// somebody to go and see.
//
// The host never moves. Everything it "says" is its own speech bubble,
// which the living-town pass builds on static NPCs exactly as it builds
// one on a walking one; this behaviour only pops it. The visitor's half of
// the exchange is the station's `arriveIcon`, popped by the brain when it
// arrives, so the two bubbles alternate: the caller says hello, a beat
// passes, the host answers with its own first line from the manifest.
//
// Nothing is synced - the bubbles are cosmetic and every client runs the
// same villagers (see LegaiaNpcWander).
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaVisitSpot : UdonSharpBehaviour
    {
        [Tooltip("The station this handles (builder-wired; found on this object when empty).")]
        public LegaiaNpcStation station;

        [Tooltip("The fixed resident's own speech bubble.")]
        public LegaiaSpeechBubble hostBubble;

        [Tooltip("Icon the host answers with (see LegaiaBubbleArt.ICON_NAMES).")]
        public int hostIcon;

        [Tooltip("Beat between the caller's hello and the host's answer (seconds).")]
        public float replyDelay = 2.4f;

        [Tooltip("How long the host's bubble stays up (seconds).")]
        public float replySeconds = 4.5f;

        [Tooltip("Seconds between the host's answers while one villager keeps standing here.")]
        public float replyInterval = 9f;

        private bool visiting;

        void Start()
        {
            if (station == null)
                station = GetComponent<LegaiaNpcStation>();
        }

        public void OnNpcArrive()
        {
            if (hostBubble == null)
                return;
            visiting = true;
            SendCustomEventDelayedSeconds("HostReply", replyDelay);
        }

        /// The host's turn. Re-armed while the caller is still standing
        /// here, so a long visit is a short exchange rather than one line.
        public void HostReply()
        {
            if (!visiting || hostBubble == null)
                return;
            hostBubble.Show(hostIcon, replySeconds);
            SendCustomEventDelayedSeconds("HostReply",
                replyInterval < 3f ? 3f : replyInterval);
        }

        public void OnNpcLeave()
        {
            visiting = false;
            if (hostBubble != null)
                hostBubble.Hide();
        }
    }
}
