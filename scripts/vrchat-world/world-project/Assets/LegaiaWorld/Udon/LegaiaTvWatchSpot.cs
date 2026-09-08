// The spot in front of the TV where a villager stands to watch it - and
// the one place that knows villagers have favourite shows.
//
// It is the station contract's ordinary handler shape (the same one the
// cupboards, the fishing spots and the card table use), so the town
// director never learns that television exists: it hands out a free
// station of kind 0, the brain walks its villager over and calls
// Arrive(), and this behaviour asks the TV whether the arriving villager
// owns a show.
//
//   OnNpcArrive - if this villager has a show AND the TV is on its
//                 default playlist, the show goes on. Any other TV state
//                 (a guest's URL, another villager's show, off) refuses
//                 the request, and the villager just watches whatever is
//                 already playing. That refusal is the whole social rule
//                 and it lives in LegaiaVideoTv.RequestShow.
//   OnNpcLeave  - if what is playing is this villager's show, the TV goes
//                 back to the playlist. They put it on; they take it with
//                 them.
//
// THE CONSOLE. A show may be flagged as a console show (Noa's Spyro run).
// While one plays, the floor console appears in front of the set - the TV
// does that itself, off the synced state, so every client sees it - and
// the owner holds a controller, which is this behaviour's job because
// only it can reach the villager standing here. The controller follows
// the SYNCED show rather than this client's own request, so on a client
// whose copy of Noa did not start the run she still holds the pad while
// it plays. A tick re-checks it, because a show can end (or a player can
// take the TV) while she is still standing there.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaTvWatchSpot : UdonSharpBehaviour
    {
        [Tooltip("The station this handles (builder-wired; found on this object when empty).")]
        public LegaiaNpcStation station;

        [Tooltip("The TV this spot watches.")]
        public LegaiaVideoTv tv;

        [Tooltip("Carry-rig item index for the controller (LegaiaCarryArt.ITEM_NAMES).")]
        public int controllerItem = 4;

        [Tooltip("Seconds between re-checks while a villager stands here.")]
        public float tickSeconds = 2f;

        private bool occupied;
        private bool holdingPad;

        void Start()
        {
            if (station == null)
                station = GetComponent<LegaiaNpcStation>();
        }

        public void OnNpcArrive()
        {
            occupied = true;
            holdingPad = false;
            if (tv == null || station == null || station.currentNpc == null)
                return;
            int show = tv.ShowFor(NameOf(), station.currentNpc.gameObject.name);
            if (show >= 0)
                tv.RequestShow(show); // refused unless the playlist is on
            Watch();
        }

        public void OnNpcLeave()
        {
            occupied = false;
            if (tv != null && station != null && station.currentNpc != null)
            {
                int playing = tv.PlayingShow();
                if (playing >= 0 &&
                    tv.ShowBelongsTo(playing, NameOf(), station.currentNpc.gameObject.name))
                    tv.EndShow(playing);
            }
            SetPad(false);
        }

        /// While someone stands here: keep the controller in step with
        /// what is actually playing. Re-armed by itself, and stops as soon
        /// as the spot empties.
        public void Watch()
        {
            if (!occupied)
            {
                SetPad(false);
                return;
            }
            bool want = false;
            if (tv != null && station != null && station.currentNpc != null &&
                tv.ConsoleShowPlaying())
                want = tv.ShowBelongsTo(tv.PlayingShow(), NameOf(),
                    station.currentNpc.gameObject.name);
            SetPad(want);
            SendCustomEventDelayedSeconds(nameof(Watch), tickSeconds);
        }

        string NameOf()
        {
            if (station == null || station.currentNpc == null)
                return "";
            LegaiaNpcBrain brain = station.currentNpc.GetComponent<LegaiaNpcBrain>();
            return brain == null ? "" : brain.label;
        }

        void SetPad(bool on)
        {
            if (on == holdingPad)
                return;
            holdingPad = on;
            LegaiaNpcCarry carry = station != null && station.currentNpc != null
                ? station.currentNpc.GetComponent<LegaiaNpcCarry>()
                : null;
            if (carry == null)
                return;
            if (on)
                carry.Show(controllerItem, 0);
            else
                carry.Hide();
        }
    }
}
