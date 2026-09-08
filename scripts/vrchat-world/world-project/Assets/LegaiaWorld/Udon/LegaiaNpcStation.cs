// A place an NPC can go and do something: stand in front of a cupboard,
// fish off the shore, sit at the card table, meet for a chat. The living
// town's NPC brain (LegaiaNpcBrain, driven by LegaiaTownDirector) claims a
// free station, walks its NPC to `standPoint`, turns it toward
// `standPoint.forward`, and then hands the visit to the station's
// optional `handler` behaviour by SendCustomEvent - so the thing the NPC
// "does" there is pluggable without the brain knowing about it:
//
//   handler.SendCustomEvent("OnNpcArrive")   after the NPC has arrived
//   handler.SendCustomEvent("OnNpcLeave")    just before the NPC walks off
//
// The station sets `currentNpc` (the NPC root transform) and `currentBrain`
// before either event so the handler can read them
// (`station.currentNpc`). A cupboard handler opens its door on arrive and
// closes it on leave; a fishing handler shows a rod + float; a seat handler
// parks the NPC on its stool. A station with no handler is just a stand
// spot (a chat meeting point, a scenic view).
//
// `kind` tags what the station is for so the director can pick per
// activity and per time of day; the brain never interprets it beyond
// picking. Values (keep in sync with LegaiaTownDirector):
//   0 = use-prop (cupboard / chest / drawer - opens while the NPC stands)
//   1 = fishing spot (shoreline)
//   2 = seat (card table stool)
//   3 = chat spot (meeting point for a 2-3 NPC conversation)
//   4 = viewpoint (stand and look; no handler)
//   5 = carry / errand endpoint (LegaiaNpcHandItem: something is picked up
//       or put down here - a bucket at the shore, a broom at a doorway)
//   6 = visit (LegaiaVisitSpot: a stand spot in front of one of the
//       village's fixed residents, whose own bubble answers)
// The four fields directly under `kind` below are read by the BRAIN and
// the DIRECTOR rather than by a handler, because they describe what the
// VISITOR does here rather than what the station does - so a plain stand
// spot can pop a bubble, glance about, or take its place in a sequenced
// fetch errand without needing a behaviour of its own.
// `indoors` marks stations inside an interior room (only reachable through
// a doorway teleport - the brain routes through the door pair it is given
// by the director), so daytime pickers can prefer outdoor ones and the
// night routine can prefer indoor ones.
//
// Ownership is local per client (every client simulates the same NPCs,
// like LegaiaNpcWander) - `Claim`/`Release` only stop two NPCs on ONE
// client's simulation from using a station at the same time.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcStation : UdonSharpBehaviour
    {
        [Tooltip("0 use-prop, 1 fishing, 2 seat, 3 chat spot, 4 viewpoint, 5 carry/errand endpoint, 6 visit a fixed resident.")]
        public int kind;

        [Tooltip("Bubble icon the ARRIVING villager pops when it gets here (-1 = none). See LegaiaBubbleArt.ICON_NAMES.")]
        public int arriveIcon = -1;

        [Tooltip("Glance around while standing here (a villager on an errand looks at the view, not through it).")]
        public bool glance = true;

        [Tooltip("Carry flow, so an itinerary can be SEQUENCED rather than shuffled: 0 nothing changes hands here, 1 something is picked up, 2 something is put down.")]
        public int carryFlow;

        [Tooltip("Free-text tag for the builder's own bookkeeping (\"shore\", \"doorway\", \"path\"...); nothing reads it at runtime.")]
        public string role = "";

        [Tooltip("Where the NPC stands (position) and faces (+Z) while using the station. Defaults to this transform.")]
        public Transform standPoint;

        [Tooltip("Optional behaviour that receives OnNpcArrive / OnNpcLeave (a cupboard opener, the fishing rod, the seat).")]
        public UdonSharpBehaviour handler;

        [Tooltip("Inside an interior room (reached through a doorway teleport).")]
        public bool indoors;

        [Tooltip("Typical dwell time at this station in seconds; the brain randomises around it.")]
        public float dwellSeconds = 12f;

        [Tooltip("When false the director never assigns this station (a seat someone is using, a fishing spot at high tide...). Handlers may toggle it.")]
        public bool available = true;

        [HideInInspector] public Transform currentNpc;
        [HideInInspector] public UdonSharpBehaviour currentBrain;

        public bool IsFree()
        {
            return available && currentNpc == null;
        }

        public Vector3 StandPosition()
        {
            return standPoint != null ? standPoint.position : transform.position;
        }

        public Vector3 StandForward()
        {
            Vector3 f = standPoint != null ? standPoint.forward : transform.forward;
            f.y = 0f;
            return f.sqrMagnitude < 1e-6f ? Vector3.forward : f.normalized;
        }

        /// The brain reserves the station before walking. Returns false when
        /// another NPC (on this client) already holds it.
        public bool Claim(Transform npc, UdonSharpBehaviour brain)
        {
            if (!IsFree())
                return false;
            currentNpc = npc;
            currentBrain = brain;
            return true;
        }

        /// The brain calls this once the NPC stands at the stand point.
        public void Arrive()
        {
            if (handler != null)
                handler.SendCustomEvent("OnNpcArrive");
        }

        /// The brain calls this before the NPC walks away; the station is
        /// free again afterwards.
        public void Release()
        {
            if (currentNpc != null && handler != null)
                handler.SendCustomEvent("OnNpcLeave");
            currentNpc = null;
            currentBrain = null;
        }
    }
}
