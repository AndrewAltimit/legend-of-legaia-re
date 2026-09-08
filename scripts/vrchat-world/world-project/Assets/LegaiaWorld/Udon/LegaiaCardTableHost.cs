// The card table's NPC host: the single handler behind the four stool
// LegaiaNpcStations (kind 2). Villagers drift over, sit down and hold a
// fanned pair of cards - and a player sitting down is an INVITATION, not
// an eviction: the free stools stay available while people play, and the
// card game (LegaiaCardGame) summons villagers into them.
//
// That is the one rule that changed when the table got a game. The host
// used to withdraw every station the moment a player sat or a card left
// the deck, on the theory that the table was "in use"; the game wants the
// opposite - company. So availability now turns on exactly two things:
// the synced `npcsAllowed` toggle behind the table's "NPCs: sit / shoo"
// button, and whether a PLAYER is sitting on that particular stool
// (LegaiaSeat's `occupied`, mirrored on every client by the station
// callbacks). Cards scattered across the felt shoo nobody any more.
//
// `gameActive` is still published for anything that wants "is the table
// busy" - it is true while a player sits or a card is out of the deck
// (the deck's cards are ordinary pickups on their own Object Sync, so a
// card further than `cardAwayRadius` from the anchor means a hand is in
// play) - but nothing in this file gates on it. An NPC already seated is
// never teleported off: its station goes unavailable and the NPC brain,
// which re-checks `available` while dwelling, walks it away on its own.
//
// Seated pose: the exported rigs have no sit clip (they are rigid-node
// models playing a looping spawn clip), so sitting is APPROXIMATED - the
// NPC root is nudged onto the stool centre and set so its HIPS land on
// the stool's seat: root height = seat top - `hipFraction` of the rig's
// own measured height, never below the floor it stood on. The first cut
// dropped the whole rig 35% of its height BELOW the floor instead, on the
// theory that a lower head reads as sitting; it read as a villager sunk
// to the waist in the ground, head level with the felt. Hips on the seat
// puts a short rig's feet dangling in front of the stool leg and a tall
// one's on the floor, and every head above the table. Restored when the
// villager leaves. The README says so out loud.
//
// Seat bookkeeping is reconciled by polling the stations a few times a
// second rather than from the arrive / leave events alone: one handler
// serves four stations and the events carry no station argument, so the
// events only force an immediate reconcile. The poll reads the station's
// `currentNpc` for WHO, and the brain's `Seated()` for WHETHER it is here
// yet: the station names its villager when the director claims it, before
// the walk, and sitting on that alone teleported villagers onto the
// stools from across the square.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaCardTableHost : UdonSharpBehaviour
    {
        [Tooltip("The stool stations, one per seat (builder-wired).")]
        public LegaiaNpcStation[] seats;

        [Tooltip("The LegaiaSeat on each stool, same order as `seats` - polled for player occupancy.")]
        public LegaiaSeat[] seatChairs;

        [Tooltip("Per-seat fanned card-back props, same order - shown while an NPC sits.")]
        public GameObject[] seatHands;

        [Tooltip("The deck's stack anchor - cards further out than cardAwayRadius mean a game is on.")]
        public Transform deckAnchor;

        [Tooltip("Every card of the deck (builder-wired).")]
        public Transform[] cards;

        [Tooltip("How far (metres, horizontal) a card must sit from the anchor to count as played.")]
        public float cardAwayRadius = 0.25f;

        [Tooltip("The table stays 'in use' this long after the last card came back.")]
        public float gameIdleSeconds = 60f;

        [Tooltip("Height of the stool's seat above the stool's own origin (metres) - the builder's stool geometry.")]
        public float seatHeight = 0.475f;

        [Tooltip("Where a standing rig's hips sit, as a fraction of its measured height; the root is placed so the hips land on the seat.")]
        public float hipFraction = 0.45f;

        [Tooltip("Synced: villagers may take a free stool. The table button toggles it.")]
        [UdonSynced] public bool npcsAllowed = true;

        [HideInInspector] public bool gameActive;

        private Transform[] seated;
        private float[] drops;
        private float pollClock;
        private float lastCardMotion = -1000f;

        void Start()
        {
            int n = seats == null ? 0 : seats.Length;
            seated = new Transform[n];
            drops = new float[n];
            Reconcile();
        }

        void Update()
        {
            pollClock += Time.deltaTime;
            if (pollClock < 0.5f)
                return;
            pollClock = 0f;
            Reconcile();
        }

        /// The table button (LegaiaEventButton -> "ToggleNpcs").
        public void ToggleNpcs()
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local != null && !Networking.IsOwner(gameObject))
                Networking.SetOwner(local, gameObject);
            npcsAllowed = !npcsAllowed;
            RequestSerialization();
            Reconcile();
        }

        public override void OnDeserialization()
        {
            Reconcile();
        }

        // The station events carry no station id (one handler, four
        // stations), so they only force the poll to run now.
        public void OnNpcArrive()
        {
            Reconcile();
        }

        public void OnNpcLeave()
        {
            Reconcile();
        }

        void Reconcile()
        {
            if (seats == null || seated == null)
                return;
            bool players = AnyPlayerSeated();
            if (CardsInPlay())
                lastCardMotion = Time.time;
            gameActive = players || (Time.time - lastCardMotion) < gameIdleSeconds;
            // Invitation, not eviction: only the toggle withdraws the
            // stools, and only the stool a player is actually sitting on
            // is taken out of the villagers' reach.
            bool allow = npcsAllowed;

            for (int i = 0; i < seats.Length; i++)
            {
                LegaiaNpcStation s = seats[i];
                if (s == null)
                    continue;
                bool seatTaken = seatChairs != null && i < seatChairs.Length &&
                                 seatChairs[i] != null && seatChairs[i].occupied;
                s.available = allow && !seatTaken;

                // `currentNpc` is set at CLAIM time - before the walk - so
                // it alone would sit the villager the moment the table
                // chose it, snapping it onto the stool from across the
                // square (which is exactly what it did). Sit only once the
                // brain reports it has arrived at this seat.
                Transform npc = s.currentNpc;
                bool arrived = false;
                if (npc != null && s.currentBrain != null)
                {
                    LegaiaNpcBrain b = s.currentBrain.GetComponent<LegaiaNpcBrain>();
                    arrived = b != null && b.Seated();
                }
                if (npc != null && arrived && seated[i] == null)
                    SitDown(i, npc);
                else if ((npc == null || !arrived) && seated[i] != null)
                    StandUp(i);
            }
        }

        void SitDown(int i, Transform npc)
        {
            float h = MeasureHeight(npc);
            Vector3 p = npc.position;
            Vector3 stand = seats[i].StandPosition();
            // Onto the stool centre, hips on the seat: root = seat top minus
            // the hip height, never below the stool's own floor. Measured
            // from the STOOL, not from where the rig is: it walked onto the
            // stool's collider to get here, so its own height is the seat
            // top already, and "never below where it stands" would leave it
            // standing on the seat.
            float y = stand.y + Mathf.Max(0f, seatHeight - h * Mathf.Clamp01(hipFraction));
            npc.position = new Vector3(stand.x, y, stand.z);
            seated[i] = npc;
            // What StandUp adds back (negative: the rig was lifted).
            drops[i] = p.y - y;
            if (seatHands != null && i < seatHands.Length && seatHands[i] != null)
                seatHands[i].SetActive(true);
        }

        void StandUp(int i)
        {
            Transform npc = seated[i];
            seated[i] = null;
            if (seatHands != null && i < seatHands.Length && seatHands[i] != null)
                seatHands[i].SetActive(false);
            if (npc == null)
                return;
            // Only undo the drop if the NPC is still where we put it - the
            // brain may already have walked it onto its own floor ray.
            Vector3 d = npc.position - seats[i].StandPosition();
            d.y = 0f;
            if (d.sqrMagnitude < 0.25f)
                npc.position = npc.position + Vector3.up * drops[i];
            drops[i] = 0f;
        }

        bool AnyPlayerSeated()
        {
            if (seatChairs == null)
                return false;
            for (int i = 0; i < seatChairs.Length; i++)
                if (seatChairs[i] != null && seatChairs[i].occupied)
                    return true;
            return false;
        }

        bool CardsInPlay()
        {
            if (cards == null || deckAnchor == null)
                return false;
            Vector3 a = deckAnchor.position;
            float r2 = cardAwayRadius * cardAwayRadius;
            for (int i = 0; i < cards.Length; i++)
            {
                if (cards[i] == null)
                    continue;
                Vector3 d = cards[i].position - a;
                d.y = 0f;
                if (d.sqrMagnitude > r2)
                    return true;
            }
            return false;
        }

        float MeasureHeight(Transform npc)
        {
            Renderer[] rs = npc.GetComponentsInChildren<Renderer>();
            if (rs.Length == 0)
                return 1.6f;
            Bounds b = rs[0].bounds;
            for (int i = 1; i < rs.Length; i++)
                b.Encapsulate(rs[i].bounds);
            return Mathf.Clamp(b.size.y, 0.4f, 4f);
        }
    }
}
