// The card table's dealer: community-card poker, Cara's five-card night
// game and blackjack, played at the four stools by players AND by the
// town's villagers, for the coins the wallet holds.
//
// THREE GAMES, TWO SWITCHES. `mode` picks poker (0) or blackjack (1) and
// is the panel's Mode button. `rules` picks WHICH poker: 0 hold'em (two
// hole cards a seat, five community cards in the middle of the felt,
// betting between each reveal) and 1 the five-card night game (five
// cards a seat, one betting round, best hand wins). Nobody presses the
// second switch: `rules` follows the town clock - by day hold'em, and
// from dusk it is Cara's poker night, the game the rule poster by the
// table describes. The clock is polled on the 0.25 s tick through
// `director.dayNight.isNight` (the director pushes itself in at its own
// Start, so a null director simply means day), and a change only lands
// BETWEEN hands: a hand in progress always finishes under the rules it
// was dealt under. `rulesOverride` is the editor checks' way in and is
// -1 in a built world.
//
// THE COMMUNITY CARDS are five real pickups laid face down across the
// middle of the felt at build time (`community_anchor_0..4`) and turned
// over in stages - three for the flop, one for the turn, one for the
// river - with a betting round after each. The stagger is the master
// scheduling itself with SendCustomEventDelayedSeconds, and what the
// other clients render from is the synced `communityUp` count, so a late
// joiner sees exactly as many faces as everyone else. Each delayed step
// is guarded by a `pending` flag plus its own deadline, and MasterTick
// carries a watchdog that steps the sequence on if the delayed event
// never arrives (a scaled-time editor soak can outrun it) - the guard is
// what stops the watchdog and a late event from both revealing.
//
// SHOWDOWN IS PACED. Losing seats turn over first, the winner last, a
// beat apart, and only then does the message name the pot. When
// everyone but one seat folds there is no showdown at all: the last seat
// takes the pot without showing a card ("wins uncontested"), which is
// both the etiquette and the reason a fold returns BOTH hole cards to
// the stack at once.
//
// WHO IS THE DEALER. This behaviour's OWNER is the master: it holds the
// deck order, decides whose turn it is, runs the AI, and writes every
// synced field. Every other client renders what it is told
// (OnDeserialization) and sends its player's presses to the owner with
// SendCustomNetworkEvent(Owner, "Act", seat, action) - the owner then
// validates that it really is that seat's turn and that the CALLING
// player occupies the seat, so a client can only ever play its own hand.
// A client whose player sits down takes ownership when the current owner
// holds no seat, which keeps the dealer on someone who is actually at the
// table (and off a player who has left the instance).
//
// NO WALLET IS WRITTEN BY THE DEALER. LegaiaWallet is local-write by
// design (PlayerData holds the local player's own record and nobody
// else's), so the master never spends anyone's coins. It syncs a per-seat
// coin DELTA plus a `settleSerial`, and every client applies the delta for
// its own local player only when that serial changes. Nothing is debited
// while a hand is running: the master checks affordability through
// `wallet.CoinsOf(player)` (PlayerData is replicated, so any client can
// read any purse) and settles the whole hand in one delta at showdown.
// That is also what makes "the last player stood up" cheap to handle - a
// voided hand simply never settles.
//
// VILLAGERS ARE INVITED, NOT SHOOED. A player sitting down used to
// withdraw the stools from the town director; now it does the opposite -
// the free stools stay available and the table SUMMONS the nearest idle
// villager to each of them (LegaiaTownDirector.Summon, rate-limited to
// one call every couple of seconds), then holds their dwell timer open
// (LegaiaNpcBrain.HoldStation) for as long as a hand is on. The manual
// "NPCs: sit / shoo" button on the table remains the override.
//
// Summoning and holding run on EVERY client, not just the master: the
// villagers are simulated locally on each client (LegaiaNpcWander), so a
// summon that only the master made would move nobody anywhere else. What
// is synced is that a seat is an NPC seat and what its hand holds; WHICH
// villager is sitting there is read locally off the station.
//
// NPC-ONLY SELF-PLAY. With nobody seated the table keeps playing itself
// (`npcSelfPlay`): a hand every few seconds among the seated villagers,
// so the table reads as alive from across the square - and so the whole
// machine can be soaked headless, where there is no local player at all
// (Networking.LocalPlayer is null; the behaviour treats that as "I am the
// only simulation" and runs the dealer side).
//
// THE CARDS ARE REAL. Dealing moves the 52 LegaiaCard pickups onto the
// per-seat hand anchors; a player's cards are dealt FACE UP (everyone at
// the table can read them - a physical deck of pickups cannot hide a hand
// from a client that can walk around it, so the game does not pretend to)
// and a villager's face down until the showdown flips them. A player may
// still pick a card up and carry it off: the game tracks card IDENTITY by
// index and never reads a position back, so a wandering card changes
// nothing but the view.
//
// The RNG is an int LCG seeded and synced by the master, so a client that
// takes over mid-hand rebuilds exactly the same undealt remainder.
//
// TABLE TALK IS PER SEAT. Every line belongs to the seat that said it
// and is printed on THAT seat's row, beside the name - a synced
// `seatTalk[]` slot holding the newest line, cleared after `talkHold`.
// (It used to be one italic block at the bottom of the panel holding the
// last two lines from anybody, which read as a chat log rather than as
// people talking.) The lines themselves are composed by LegaiaTableTalk
// on the master at the moments that matter - sitting down, the deal, a
// raise, a fold, each community reveal, a bad beat, the pot going one
// way, a player joining, between hands - and synced as plain text, so
// every client reads the same words. The names in front of the lines
// are the villagers' names (LegaiaLivingTown), never their retail
// dialogue; see the talk file for why, and for Vahn, Noa and Cara.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using UnityEngine.UI;
using VRC.SDK3.UdonNetworkCalling;
using VRC.SDKBase;
using VRC.Udon.Common.Interfaces;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaCardGame : UdonSharpBehaviour
    {
        // --- wiring (builder-set) --------------------------------------------

        [Tooltip("The town director - set by ITS Start (GameObject.Find on this object's path). Null when the living town is not built.")]
        public LegaiaTownDirector director;

        [Tooltip("The table's NPC host (seat stations, the sit/shoo toggle).")]
        public LegaiaCardTableHost host;

        [Tooltip("The deck controller - used to re-stack between hands.")]
        public LegaiaCardDeck deck;

        [Tooltip("The world coin purse (Legaia_common_prefabs/wallet).")]
        public LegaiaWallet wallet;

        [Tooltip("The villagers' lines (on this object; builder-wired). Null = a silent table.")]
        public LegaiaTableTalk talk;

        [Tooltip("Seat stations, one per stool (same order as chairs / handAnchors).")]
        public LegaiaNpcStation[] stations;

        [Tooltip("Player chairs, one per stool (same order).")]
        public LegaiaSeat[] chairs;

        [Tooltip("The 52 cards, in build order: index = rank + 13 * suit.")]
        public LegaiaCard[] cards;

        [Tooltip("Per-seat hand anchor on the felt; five fanned slots along its right axis.")]
        public Transform[] handAnchors;

        [Tooltip("Where the table's own (blackjack dealer) cards are laid.")]
        public Transform dealerAnchor;

        [Tooltip("The five community-card spots across the middle of the felt (hold'em).")]
        public Transform[] communityAnchors;

        [Tooltip("The deck's stack anchor - discards and undealt cards go back here.")]
        public Transform stackAnchor;

        // --- panel widgets ----------------------------------------------------

        public Text modeText;
        public Text potText;
        public Text msgText;
        [Tooltip("The board: the community cards turned over so far (hold'em).")]
        public Text communityText;
        [Tooltip("The local seat's best hand right now, named.")]
        public Text handText;
        public RawImage[] rowPortrait;
        public Text[] rowName;
        public Text[] rowCoins;
        public Text[] rowStatus;
        [Tooltip("Per-seat speech line, printed on that seat's own row.")]
        public Text[] rowTalk;
        public Button btnDeal;
        public Button btnMode;
        public Button btnCall;
        public Button btnRaise;
        public Button btnFold;
        public Button btnHit;
        public Button btnStand;
        public Text btnCallText;
        public Text btnRaiseText;
        public Text btnDealText;

        [Tooltip("Flat silhouette tint drawn in a seat's portrait slot when a PLAYER sits there.")]
        public Color playerTint = new Color(0.45f, 0.55f, 0.75f, 1f);

        // --- tuning -----------------------------------------------------------

        [Tooltip("Coins every seat puts in before the cards come out (the blackjack stake too).")]
        public int ante = 2;

        [Tooltip("Size of a bet or a raise in the poker betting rounds.")]
        public int betStep = 2;

        [Tooltip("Raises allowed per betting round.")]
        public int maxRaises = 3;

        [Tooltip("A human seat that does nothing for this long is checked (or folded when the call costs coins).")]
        public float turnTimeout = 25f;

        [Tooltip("How long a villager 'thinks' before acting.")]
        public float npcThink = 1.2f;

        [Tooltip("How long the showdown stays up before the table clears.")]
        public float showSeconds = 6f;

        [Tooltip("Beat between two community cards turning over (the flop's three).")]
        public float communityGap = 0.7f;

        [Tooltip("Beat between two seats turning their cards over at the showdown.")]
        public float showdownGap = 0.6f;

        [Tooltip("Editor checks only: 0 forces hold'em, 1 forces the night game, -1 (the built world) follows the town clock.")]
        public int rulesOverride = -1;

        [Tooltip("Gap between hands while the villagers play among themselves.")]
        public float selfPlayGap = 8f;

        [Tooltip("Villagers keep playing at an empty table.")]
        public bool npcSelfPlay = true;

        [Tooltip("How long the last table-talk line stays on the panel after the villagers go quiet (seconds).")]
        public float talkHold = 14f;

        [Tooltip("Gap between idle remarks while the table waits between hands (seconds).")]
        public float idleTalkGap = 16f;

        [Tooltip("Virtual chips a villager sits down with (refilled every time it takes a stool).")]
        public int npcStartChips = 60;

        [Tooltip("How far the director may reach for a villager to fill a stool, metres - kept under what the brain's walk timeout (45 s at 0.7 m/s) can cover, or a far villager gives up half way.")]
        public float summonRadius = 28f;

        [Tooltip("Seconds between summon attempts.")]
        public float summonInterval = 1.5f;

        [Tooltip("Set by the editor unit tests: never call RequestSerialization.")]
        [System.NonSerialized] public bool suppressSerialization;

        // --- synced state -----------------------------------------------------

        [UdonSynced] public int mode;            // 0 poker, 1 blackjack
        [UdonSynced] public int rules;           // 0 hold'em, 1 five-card night
        [UdonSynced] public int street;          // hold'em: 0 pre-flop, 1 flop, 2 turn, 3 river
        [UdonSynced] public int phase;           // see PH_*
        [UdonSynced] public int pot;
        [UdonSynced] public int turnSeat = -1;
        [UdonSynced] public int currentBet;
        [UdonSynced] public int raises;
        [UdonSynced] public int shuffleSeed;
        [UdonSynced] public int deckPos;
        [UdonSynced] public int settleSerial;
        [UdonSynced] public int handsCompleted;
        [UdonSynced] public int dealerUpCount;   // dealer cards revealed so far
        [UdonSynced] public int communityUp;     // community cards turned over so far
        [UdonSynced] public int shownMask;       // bit i: seat i's cards are face up
        [UdonSynced] public string message = "";

        [UdonSynced] public int[] community;     // 5 board card indices, -1 empty
        [UdonSynced] public string[] seatTalk;   // newest line per seat, "" when quiet
        [UdonSynced] public int[] seatKind;      // 0 empty, 1 player, 2 npc
        [UdonSynced] public int[] seatPlayerId;
        [UdonSynced] public int[] seatChips;     // villagers' virtual chips
        [UdonSynced] public int[] seatState;     // see H_*
        [UdonSynced] public int[] seatBet;       // committed THIS betting round
        [UdonSynced] public int[] seatPaid;      // committed this hand
        [UdonSynced] public int[] seatWon;       // taken from the pot this hand
        [UdonSynced] public int[] seatResult;    // net coin delta, applied on settleSerial
        [UdonSynced] public int[] seatCards;     // seat*5 + slot -> card index, -1 empty
        [UdonSynced] public int[] dealerCards;   // blackjack, -1 empty

        // --- phases / seat states / actions ------------------------------------

        const int MODE_POKER = 0, MODE_BJ = 1;
        const int RULES_HOLDEM = 0, RULES_NIGHT = 1;
        // PH_BET keeps the value the old PH_BET1 had (the blackjack
        // hit/stand phase is the same number), so a soak log reads the same.
        const int PH_IDLE = 0, PH_BET = 2, PH_SHOW = 5, PH_REVEAL = 6;
        const int K_EMPTY = 0, K_PLAYER = 1, K_NPC = 2;
        const int H_OUT = 0, H_IN = 1, H_FOLD = 2, H_SITOUT = 3, H_STAND = 4, H_BUST = 5;
        const int A_CALL = 0, A_RAISE = 1, A_FOLD = 2, A_HIT = 4,
                  A_STAND = 5, A_DEAL = 6, A_MODE = 7;

        // Poker hand categories, low to high.
        const int CAT_HIGH = 0, CAT_PAIR = 1, CAT_TWOPAIR = 2, CAT_TRIPS = 3,
                  CAT_STRAIGHT = 4, CAT_FLUSH = 5, CAT_FULL = 6, CAT_QUADS = 7,
                  CAT_SFLUSH = 8;

        // Table-talk kinds (mirror LegaiaTableTalk.T_*).
        const int T_SIT = 0, T_DEAL = 1, T_RAISE = 2, T_CALL = 3, T_FOLD = 4,
                  T_WIN = 5, T_LOSE = 6, T_BUST = 7, T_NATURAL = 8, T_IDLE = 9,
                  T_PLAYER = 10, T_LEAVE = 11, T_FLOP = 12, T_TURN = 13,
                  T_RIVER = 14, T_BADBEAT = 15, T_NIGHT = 16;

        // --- local (never synced) ----------------------------------------------

        private int seatCount;
        private bool isLocalOwner;
        private int[] deckOrder;
        private int deckSeedBuilt;
        private float pollClock;
        private float turnDeadline;
        private float showUntil;
        private float nextDeal;
        private float nextSummon;
        private int betActed;          // bitmask: seats that acted since the last raise
        private int appliedSerial;
        private bool handHadPlayer;
        private LegaiaNpcBrain[] brainAt;
        private string[] seatLabel;     // last villager name seen per seat
        private float[] seatTalkAt;     // master: when each seat's line expires
        private float nextIdleTalk;
        private int talkSalt;

        // The two staggered reveals. Each is a delayed self-event plus a
        // deadline, and the `pending` flag is what makes a watchdog step
        // and a late delayed event collapse into one (see the header).
        private bool pendingCommunity;
        private float communityNext;
        private int communityTarget;
        private bool pendingShow;
        private float showNext;
        private int[] showOrder;        // seats in reveal order, losers first
        private int showCount;
        private int showIdx;
        private string showMessage = "";

        void Start()
        {
            seatCount = chairs != null ? chairs.Length
                : (stations != null ? stations.Length : 0);
            EnsureArrays();
            brainAt = new LegaiaNpcBrain[seatCount < 1 ? 1 : seatCount];
            seatLabel = new string[seatCount < 1 ? 1 : seatCount];
            seatTalkAt = new float[seatCount < 1 ? 1 : seatCount];
            showOrder = new int[seatCount < 1 ? 1 : seatCount];
            nextIdleTalk = Time.time + 6f;
            deckOrder = new int[52];
            for (int i = 0; i < 52; i++)
                deckOrder[i] = i;
            deckSeedBuilt = 0;
            RefreshOwner();
            appliedSerial = settleSerial;
            if (isLocalOwner)
                message = "Sit down to play";
            RefreshPanel();
        }

        // Synced arrays are only ever REPLACED by deserialization, so they
        // are allocated once here at the seat count the builder wired.
        void EnsureArrays()
        {
            int n = seatCount < 1 ? 1 : seatCount;
            if (seatKind == null || seatKind.Length != n)
            {
                seatKind = new int[n];
                seatPlayerId = new int[n];
                seatChips = new int[n];
                seatState = new int[n];
                seatBet = new int[n];
                seatPaid = new int[n];
                seatWon = new int[n];
                seatResult = new int[n];
                seatCards = new int[n * 5];
                seatTalk = new string[n];
                for (int i = 0; i < seatCards.Length; i++)
                    seatCards[i] = -1;
                for (int i = 0; i < n; i++)
                {
                    seatPlayerId[i] = -1;
                    seatTalk[i] = "";
                }
            }
            if (seatTalk == null || seatTalk.Length != n)
            {
                seatTalk = new string[n];
                for (int i = 0; i < n; i++)
                    seatTalk[i] = "";
            }
            if (dealerCards == null || dealerCards.Length != 6)
            {
                dealerCards = new int[6];
                for (int i = 0; i < 6; i++)
                    dealerCards[i] = -1;
            }
            if (community == null || community.Length != 5)
            {
                community = new int[5];
                for (int i = 0; i < 5; i++)
                    community[i] = -1;
            }
        }

        void Sync()
        {
            if (suppressSerialization)
                return;
            RequestSerialization();
        }

        // The editor / headless case (no local player at all) is treated as
        // "I am the only simulation there is" so the dealer keeps running.
        void RefreshOwner()
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            isLocalOwner = local == null || Networking.IsOwner(gameObject);
        }

        public override void OnOwnershipTransferred(VRCPlayerApi player)
        {
            RefreshOwner();
            // A new dealer rebuilds the undealt remainder from the synced
            // seed: same order, same deckPos, no cards repeated.
            BuildDeck(shuffleSeed);
        }

        public override void OnDeserialization()
        {
            EnsureArrays();
            ApplySettlement();
            RenderRevealed();
            RefreshPanel();
        }

        /// Every client draws the faces from the SYNCED counts, never from
        /// its own idea of how far the hand has got: `communityUp` board
        /// cards and every seat in `shownMask` are face up. Purely local -
        /// the card's own Object Sync carries the owner's authoritative
        /// `faceUp`, and this is what a late joiner (whose card sync has
        /// not caught up) sees in the meantime. It never turns a card back
        /// over, so it cannot fight the owner.
        void RenderRevealed()
        {
            if (cards == null)
                return;
            for (int i = 0; i < communityUp && community != null &&
                 i < community.Length; i++)
                RevealLocal(community[i]);
            for (int i = 0; i < seatCount; i++)
            {
                if ((shownMask & (1 << i)) == 0)
                    continue;
                for (int c = 0; c < 5; c++)
                    RevealLocal(seatCards[i * 5 + c]);
            }
        }

        void RevealLocal(int idx)
        {
            if (cards == null || idx < 0 || idx >= cards.Length || cards[idx] == null)
                return;
            cards[idx].Reveal();
        }

        // --- main loop ----------------------------------------------------------

        void Update()
        {
            pollClock += Time.deltaTime;
            if (pollClock < 0.25f)
                return;
            pollClock = 0f;

            RefreshOwner();
            MaybeTakeOwnership();
            ReadStations();     // local: which villager sits where
            InviteVillagers();  // local: every client walks its own NPCs over
            HoldSeated();
            if (isLocalOwner)
                MasterTick();
            ApplySettlement();
            RefreshPanel();
        }

        /// A client whose player is seated takes the table over when the
        /// current owner holds no seat (they left, or never sat down).
        void MaybeTakeOwnership()
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null || Networking.IsOwner(gameObject))
                return;
            if (LocalSeat() < 0)
                return;
            VRCPlayerApi owner = Networking.GetOwner(gameObject);
            if (owner != null && owner.IsValid() && SeatOfPlayer(owner.playerId) >= 0)
                return;
            Networking.SetOwner(local, gameObject);
            RefreshOwner();
        }

        void ReadStations()
        {
            if (stations == null || brainAt == null)
                return;
            for (int i = 0; i < seatCount; i++)
            {
                LegaiaNpcBrain b = null;
                LegaiaNpcStation s = stations[i];
                if (s != null && s.currentBrain != null)
                {
                    b = s.currentBrain.GetComponent<LegaiaNpcBrain>();
                    if (b != null && (!b.Seated() || b.Dead()))
                        b = null;
                }
                brainAt[i] = b;
                if (b != null && seatLabel != null)
                    seatLabel[i] = b.label;
            }
        }

        /// Free stools invite company. Rate-limited so a table with four
        /// empty seats does not empty the square in one frame.
        void InviteVillagers()
        {
            if (director == null || stations == null)
                return;
            if (host != null && !host.npcsAllowed)
                return;
            if (!AnyPlayerSeated() && !npcSelfPlay)
                return;
            if (Time.time < nextSummon)
                return;
            for (int i = 0; i < seatCount; i++)
            {
                LegaiaNpcStation s = stations[i];
                if (s == null || !s.IsFree())
                    continue;
                // A failed call (nobody free, nobody who can route here)
                // comes back sooner than a successful one, so a stool the
                // town could not fill yet is not starved for company.
                nextSummon = Time.time + (director.Summon(s, summonRadius) != null
                    ? summonInterval : summonInterval * 0.6f);
                return;
            }
        }

        /// A villager at the table stays put while a hand is on or a player
        /// is sitting - its own dwell timer would otherwise walk it off
        /// mid-hand.
        void HoldSeated()
        {
            if (brainAt == null)
                return;
            if (phase == PH_IDLE && !AnyPlayerSeated())
                return;
            for (int i = 0; i < seatCount; i++)
                if (brainAt[i] != null)
                    brainAt[i].HoldStation(30f);
        }

        void MasterTick()
        {
            UpdateSeatModel();
            TalkTick();
            if (phase == PH_IDLE)
            {
                PollRules();
                MaybeAutoDeal();
                return;
            }
            if (phase == PH_REVEAL)
            {
                // Watchdog: the delayed self-event should have stepped the
                // board on by now (see the header - a scaled-time editor
                // soak can outrun SendCustomEventDelayedSeconds).
                if (pendingCommunity && Time.time >= communityNext + 1.5f)
                    CommunityStep();
                return;
            }
            if (phase == PH_SHOW)
            {
                if (pendingShow)
                {
                    if (Time.time >= showNext + 1.5f)
                        ShowdownStep();
                    return;
                }
                if (Time.time >= showUntil)
                    EndHand();
                return;
            }
            if (turnSeat < 0 || turnSeat >= seatCount)
            {
                AdvanceTurn();
                return;
            }
            if (seatKind[turnSeat] == K_NPC)
            {
                if (Time.time >= turnDeadline)
                    NpcAct(turnSeat);
                return;
            }
            if (seatKind[turnSeat] == K_PLAYER)
            {
                if (Time.time >= turnDeadline)
                    TimeoutAct(turnSeat);
                return;
            }
            // The seat emptied out from under the turn.
            seatState[turnSeat] = H_FOLD;
            AdvanceTurn();
        }

        // --- seat bookkeeping ---------------------------------------------------

        void UpdateSeatModel()
        {
            bool dirty = false;
            for (int i = 0; i < seatCount; i++)
            {
                int kind = K_EMPTY;
                int id = -1;
                if (chairs != null && chairs[i] != null && chairs[i].occupantId >= 0)
                {
                    kind = K_PLAYER;
                    id = chairs[i].occupantId;
                }
                else if (brainAt != null && brainAt[i] != null)
                {
                    kind = K_NPC;
                }
                if (kind == seatKind[i] && id == seatPlayerId[i])
                    continue;

                // Someone in a live hand vanished: the seat forfeits what it
                // has already put in and takes no further part.
                if (phase != PH_IDLE && seatState[i] == H_IN)
                {
                    seatState[i] = H_FOLD;
                    if (turnSeat == i)
                        AdvanceTurnNoSync();
                }
                int was = seatKind[i];
                if (kind == K_NPC && was != K_NPC)
                    seatChips[i] = npcStartChips; // refill on every re-seat
                seatKind[i] = kind;
                seatPlayerId[i] = id;
                if (kind == K_NPC && was != K_NPC)
                    Say(T_SIT, i);
                else if (kind == K_EMPTY && was == K_NPC)
                    Say(T_LEAVE, i);
                else if (kind == K_PLAYER && was != K_PLAYER)
                    SayAny(T_PLAYER);
                if (kind == K_EMPTY)
                {
                    seatState[i] = H_OUT;
                    seatChips[i] = 0;
                }
                dirty = true;
            }

            // The last player stood up mid-hand: void it. Nothing was ever
            // debited (settlement is one delta at showdown), so returning
            // the bets is simply not settling, and the villagers get their
            // virtual chips back.
            if (phase != PH_IDLE && phase != PH_SHOW && handHadPlayer &&
                !AnyPlayerSeated())
            {
                VoidHand("Hand voided - the players left");
                dirty = true;
            }
            if (dirty)
                Sync();
        }

        bool AnyPlayerSeated()
        {
            if (chairs == null)
                return false;
            for (int i = 0; i < seatCount; i++)
                if (chairs[i] != null && chairs[i].occupied)
                    return true;
            return false;
        }

        int LocalSeat()
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null || chairs == null)
                return -1;
            for (int i = 0; i < seatCount; i++)
                if (chairs[i] != null && chairs[i].occupantId == local.playerId)
                    return i;
            return -1;
        }

        int SeatOfPlayer(int playerId)
        {
            for (int i = 0; i < seatCount; i++)
                if (seatKind[i] == K_PLAYER && seatPlayerId[i] == playerId)
                    return i;
            return -1;
        }

        int InHandSeats()
        {
            int n = 0;
            for (int i = 0; i < seatCount; i++)
                if (seatState[i] == H_IN)
                    n++;
            return n;
        }

        // --- starting a hand ----------------------------------------------------

        /// Which poker the table is playing. Called only between hands, so
        /// dusk never changes the rules under a hand that is already out.
        /// `rulesOverride` is the editor checks' way in; a built world
        /// leaves it at -1 and follows the town clock. A null director (no
        /// living town) or a null cycle is day.
        public void PollRules()
        {
            int want = rulesOverride >= 0 ? rulesOverride
                : (director != null && director.dayNight != null &&
                   director.dayNight.isNight ? RULES_NIGHT : RULES_HOLDEM);
            if (want == rules)
                return;
            rules = want;
            if (mode == MODE_POKER)
                message = rules == RULES_NIGHT
                    ? "Cara's poker night" : "Hold'em - deal when you're ready";
            Sync();
        }

        void MaybeAutoDeal()
        {
            if (AnyPlayerSeated())
                return;   // players deal with the button
            if (!npcSelfPlay)
                return;
            int seated = 0;
            for (int i = 0; i < seatCount; i++)
                if (seatKind[i] == K_NPC)
                    seated++;
            if (seated < 2)
                return;
            if (Time.time < nextDeal)
                return;
            StartHand();
        }

        void StartHand()
        {
            handHadPlayer = AnyPlayerSeated();
            ReturnAllCards();
            shuffleSeed = NextSeed();
            BuildDeck(shuffleSeed);
            deckPos = 0;
            pot = 0;
            currentBet = 0;
            raises = 0;
            betActed = 0;
            dealerUpCount = 0;
            communityUp = 0;
            shownMask = 0;
            street = 0;
            pendingCommunity = false;
            pendingShow = false;
            showCount = 0;
            showIdx = 0;
            for (int i = 0; i < seatCount; i++)
            {
                seatBet[i] = 0;
                seatPaid[i] = 0;
                seatWon[i] = 0;
                seatResult[i] = 0;
                for (int c = 0; c < 5; c++)
                    seatCards[i * 5 + c] = -1;
                seatState[i] = seatKind[i] == K_EMPTY ? H_OUT : H_IN;
            }
            for (int i = 0; i < 6; i++)
                dealerCards[i] = -1;
            for (int i = 0; i < 5; i++)
                community[i] = -1;

            // Ante. A seat that cannot cover it sits the hand out.
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN)
                    continue;
                if (!CanCover(i, ante))
                {
                    seatState[i] = H_SITOUT;
                    continue;
                }
                Commit(i, ante);
            }
            int players = InHandSeats();
            int need = mode == MODE_BJ ? 1 : 2;
            if (players < need)
            {
                VoidHand(players == 0 ? "Nobody can ante" : "Need two players");
                return;
            }

            if (mode == MODE_BJ)
                DealBlackjack();
            else if (rules == RULES_NIGHT)
                DealNightPoker();
            else
                DealHoldem();
            Sync();
        }

        /// How many cards a seat holds under the rules in play - what the
        /// fan on the felt is centred on, and how far the readers look.
        int HandSlots()
        {
            if (mode == MODE_BJ)
                return 5;
            return rules == RULES_NIGHT ? 5 : 2;
        }

        void VoidHand(string why)
        {
            // Give the villagers their chips back; players were never
            // debited, so there is nothing to return.
            for (int i = 0; i < seatCount; i++)
            {
                if (seatKind[i] == K_NPC)
                    seatChips[i] += seatPaid[i];
                seatPaid[i] = 0;
                seatBet[i] = 0;
                seatWon[i] = 0;
                seatState[i] = H_OUT;
            }
            pot = 0;
            phase = PH_IDLE;
            turnSeat = -1;
            message = why;
            communityUp = 0;
            shownMask = 0;
            pendingCommunity = false;
            pendingShow = false;
            nextDeal = Time.time + selfPlayGap;
            ReturnAllCards();
            Sync();
        }

        /// Hold'em: two hole cards a seat (a player's face up, a
        /// villager's face down), then the five board cards laid face down
        /// across the middle of the felt. Nothing on the board turns over
        /// until the pre-flop betting is done.
        void DealHoldem()
        {
            for (int c = 0; c < 2; c++)
                for (int i = 0; i < seatCount; i++)
                {
                    if (seatState[i] != H_IN)
                        continue;
                    int card = DrawCard();
                    seatCards[i * 5 + c] = card;
                    PlaceCard(i, c, card, seatKind[i] == K_PLAYER);
                }
            for (int c = 0; c < 5; c++)
            {
                community[c] = DrawCard();
                PlaceCommunityCard(c, community[c], false);
            }
            message = "Pre-flop betting";
            SayAny(T_DEAL);
            BeginBetRound();
        }

        /// Cara's poker night: five cards a seat, one betting round, best
        /// hand wins. No board, no draw - the rule poster by the table.
        void DealNightPoker()
        {
            for (int c = 0; c < 5; c++)
                for (int i = 0; i < seatCount; i++)
                {
                    if (seatState[i] != H_IN)
                        continue;
                    int card = DrawCard();
                    seatCards[i * 5 + c] = card;
                    PlaceCard(i, c, card, seatKind[i] == K_PLAYER);
                }
            message = "Betting - best hand wins";
            if (!SayCara(T_NIGHT))
                SayAny(T_DEAL);
            BeginBetRound();
        }

        void DealBlackjack()
        {
            for (int c = 0; c < 2; c++)
            {
                for (int i = 0; i < seatCount; i++)
                {
                    if (seatState[i] != H_IN)
                        continue;
                    int card = DrawCard();
                    seatCards[i * 5 + c] = card;
                    PlaceCard(i, c, card, seatKind[i] == K_PLAYER);
                }
                dealerCards[c] = DrawCard();
                PlaceDealerCard(c, dealerCards[c], c == 0);
            }
            dealerUpCount = 1;
            // A natural stands itself.
            bool spoke = false;
            for (int i = 0; i < seatCount; i++)
                if (seatState[i] == H_IN && HandValue(i) == 21)
                {
                    seatState[i] = H_STAND;
                    if (seatKind[i] == K_NPC && !spoke)
                    {
                        Say(T_NATURAL, i);
                        spoke = true;
                    }
                }
            if (!spoke)
                SayAny(T_DEAL);
            phase = PH_BET;
            message = "Hit or stand";
            turnSeat = FirstActor(-1);
            ArmTurn();
            if (turnSeat < 0)
                DealerPlay();
        }

        // --- betting ------------------------------------------------------------

        void BeginBetRound()
        {
            phase = PH_BET;
            currentBet = 0;
            raises = 0;
            betActed = 0;
            for (int i = 0; i < seatCount; i++)
                seatBet[i] = 0;
            turnSeat = FirstActor(-1);
            ArmTurn();
            if (turnSeat < 0)
                NextPhase();
        }

        int FirstActor(int after)
        {
            for (int k = 1; k <= seatCount; k++)
            {
                int i = ((after < 0 ? seatCount - 1 : after) + k) % seatCount;
                if (seatState[i] == H_IN)
                    return i;
            }
            return -1;
        }

        void ArmTurn()
        {
            if (turnSeat < 0)
                return;
            turnDeadline = Time.time +
                (seatKind[turnSeat] == K_NPC ? npcThink : turnTimeout);
        }

        bool CanCover(int seat, int extra)
        {
            if (seatKind[seat] == K_NPC)
                return seatChips[seat] >= extra;
            if (seatKind[seat] != K_PLAYER)
                return false;
            if (wallet == null)
                return true;   // no purse in the world: play for fun
            VRCPlayerApi p = VRCPlayerApi.GetPlayerById(seatPlayerId[seat]);
            if (p == null || !p.IsValid())
                return false;
            return wallet.CoinsOf(p) >= seatPaid[seat] + extra;
        }

        void Commit(int seat, int amount)
        {
            if (amount <= 0)
                return;
            seatPaid[seat] += amount;
            seatBet[seat] += amount;
            pot += amount;
            if (seatKind[seat] == K_NPC)
                seatChips[seat] -= amount;
        }

        void DoCall(int seat)
        {
            int owed = currentBet - seatBet[seat];
            if (owed > 0)
            {
                if (!CanCover(seat, owed))
                {
                    DoFold(seat);
                    return;
                }
                Commit(seat, owed);
            }
            betActed = betActed | (1 << seat);
        }

        bool DoRaise(int seat)
        {
            if (raises >= maxRaises)
            {
                DoCall(seat);
                return false;
            }
            int target = currentBet + betStep;
            int owed = target - seatBet[seat];
            if (!CanCover(seat, owed))
            {
                DoCall(seat);
                return false;
            }
            Commit(seat, owed);
            currentBet = target;
            raises++;
            betActed = 1 << seat;   // everyone else owes an answer again
            return true;
        }

        void DoFold(int seat)
        {
            seatState[seat] = H_FOLD;
            betActed = betActed | (1 << seat);
            ReturnSeatCards(seat);
        }

        bool BetRoundDone()
        {
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN)
                    continue;
                if ((betActed & (1 << i)) == 0)
                    return false;
                if (seatBet[i] != currentBet)
                    return false;
            }
            return true;
        }

        void AdvanceTurn()
        {
            AdvanceTurnNoSync();
            Sync();
        }

        void AdvanceTurnNoSync()
        {
            if (InHandSeats() <= 1 && mode == MODE_POKER)
            {
                Showdown();
                return;
            }
            if (phase == PH_BET)
            {
                if (mode == MODE_BJ)
                {
                    // Blackjack: the "bet" phase is the hit/stand phase.
                    turnSeat = FirstActor(turnSeat);
                    if (turnSeat < 0)
                    {
                        DealerPlay();
                        return;
                    }
                    ArmTurn();
                    return;
                }
                if (BetRoundDone())
                {
                    NextPhase();
                    return;
                }
                turnSeat = NextUnacted(turnSeat);
                if (turnSeat < 0)
                {
                    NextPhase();
                    return;
                }
                ArmTurn();
            }
        }

        int NextUnacted(int after)
        {
            for (int k = 1; k <= seatCount; k++)
            {
                int i = (after + k) % seatCount;
                if (seatState[i] != H_IN)
                    continue;
                if ((betActed & (1 << i)) == 0 || seatBet[i] != currentBet)
                    return i;
            }
            return -1;
        }

        /// A betting round is over. The night game has exactly one, so it
        /// goes straight to the showdown; hold'em turns the next stage of
        /// the board over and bets again, until the river is behind it.
        void NextPhase()
        {
            if (mode == MODE_BJ)
            {
                DealerPlay();
                return;
            }
            if (rules == RULES_NIGHT || street >= 3)
            {
                Showdown();
                return;
            }
            BeginCommunity(street + 1);
        }

        // --- the board ----------------------------------------------------------

        /// Start turning `st`'s cards over: the flop (three), the turn or
        /// the river (one each). The first flip is scheduled like the rest
        /// so that the pause after the last bet reads as the dealer
        /// reaching for the deck.
        void BeginCommunity(int st)
        {
            street = st;
            phase = PH_REVEAL;
            turnSeat = -1;
            communityTarget = st == 1 ? 3 : (st == 2 ? 4 : 5);
            message = st == 1 ? "The flop" : (st == 2 ? "The turn" : "The river");
            ScheduleCommunity(communityGap * 0.6f);
            Sync();
        }

        void ScheduleCommunity(float delay)
        {
            pendingCommunity = true;
            communityNext = Time.time + delay;
            SendCustomEventDelayedSeconds("CommunityStep", delay);
        }

        /// One board card over. Public because it is the master's own
        /// delayed event; the `pending` + deadline guard is what makes a
        /// duplicate (the MasterTick watchdog and a late delayed event)
        /// a no-op instead of a double reveal.
        public void CommunityStep()
        {
            if (!isLocalOwner || phase != PH_REVEAL)
                return;
            if (!pendingCommunity || Time.time < communityNext - 0.05f)
                return;
            pendingCommunity = false;
            if (communityUp < communityTarget && communityUp < 5)
            {
                RevealCommunity(communityUp);
                communityUp++;
            }
            if (communityUp < communityTarget)
            {
                ScheduleCommunity(communityGap);
                Sync();
                return;
            }
            SayAny(street == 1 ? T_FLOP : (street == 2 ? T_TURN : T_RIVER));
            message = "Betting";
            BeginBetRound();
            Sync();
        }

        // --- showdown / settlement ----------------------------------------------

        /// The pot is decided here, but nothing is SHOWN yet: the losers
        /// turn over first and the winner last, a beat apart
        /// (`ShowdownStep`), and the message that names the pot waits for
        /// the last card. When everyone but one seat has folded there is
        /// no showdown at all - the last seat takes it without showing,
        /// which is both the etiquette and what makes folding mean
        /// something at a table of face-up pickups.
        void Showdown()
        {
            phase = PH_SHOW;
            turnSeat = -1;
            pendingCommunity = false;
            int best = -1;
            // -2, not -1: a hand that ends BEFORE the flop scores -1 (two
            // hole cards are not five cards), and a `-1` floor would read
            // that as "nobody has a hand" and void a pot somebody had
            // just won by everyone else folding.
            int bestScore = -2;
            int winners = 0;
            int contenders = 0;
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN)
                    continue;
                contenders++;
                int sc = ScoreSeat(i);
                if (sc > bestScore)
                {
                    bestScore = sc;
                    best = i;
                    winners = 1;
                }
                else if (sc == bestScore)
                {
                    winners++;
                }
            }
            if (best < 0)
            {
                VoidHand("No hand");
                return;
            }
            int share = winners > 0 ? pot / winners : pot;
            int given = 0;
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN || ScoreSeat(i) != bestScore)
                    continue;
                int take = share;
                if (i == best)
                    take += pot - share * winners;   // odd chip to the first winner
                seatWon[i] = take;
                given += take;
                if (seatKind[i] == K_NPC)
                    seatChips[i] += take;
            }

            // Everyone else folded: no cards are shown at all.
            if (contenders <= 1)
            {
                showMessage = SeatName(best) + " wins " + given + " uncontested";
                if (seatKind[best] == K_NPC)
                    Say(T_WIN, best);
                FinishShowdown();
                return;
            }

            showMessage = winners > 1
                ? "Split pot - " + CategoryName(bestScore / 759375)
                : SeatName(best) + " wins " + given + " (" +
                  CategoryName(bestScore / 759375) + ")";
            // Losers first, the winner (and any seat splitting with it)
            // last, so the table watches the hand that takes it turn over.
            showCount = 0;
            showIdx = 0;
            for (int pass = 0; pass < 2; pass++)
                for (int i = 0; i < seatCount && showCount < showOrder.Length; i++)
                {
                    if (seatState[i] != H_IN)
                        continue;
                    bool winner = ScoreSeat(i) == bestScore;
                    if ((pass == 0) == winner)
                        continue;
                    showOrder[showCount] = i;
                    showCount++;
                }
            message = "Showdown";
            ScheduleShowdown(showdownGap * 0.5f);
            Sync();
        }

        void ScheduleShowdown(float delay)
        {
            pendingShow = true;
            showNext = Time.time + delay;
            SendCustomEventDelayedSeconds("ShowdownStep", delay);
        }

        /// One seat's cards over. Public because it is the master's own
        /// delayed event - same guard as CommunityStep.
        public void ShowdownStep()
        {
            if (!isLocalOwner || phase != PH_SHOW || !pendingShow)
                return;
            if (Time.time < showNext - 0.05f)
                return;
            pendingShow = false;
            if (showIdx < showCount)
            {
                int seat = showOrder[showIdx];
                RevealSeat(seat);
                shownMask = shownMask | (1 << seat);
                showIdx++;
            }
            if (showIdx < showCount)
            {
                ScheduleShowdown(showdownGap);
                Sync();
                return;
            }
            FinishShowdown();
        }

        /// The last card is over: name the pot, pay it, and start the
        /// clock that clears the table.
        void FinishShowdown()
        {
            pendingShow = false;
            message = showMessage;
            int best = -1;
            int bestScore = -2;   // see Showdown: a pre-flop pot scores -1
            for (int i = 0; i < seatCount; i++)
                if (seatWon[i] > 0 && seatState[i] == H_IN)
                {
                    int sc = ScoreSeat(i);
                    if (sc > bestScore)
                    {
                        bestScore = sc;
                        best = i;
                    }
                }
            if (best >= 0 && seatKind[best] == K_NPC && shownMask != 0)
                Say(T_WIN, best);
            // A villager who turned over a made hand and still lost says
            // so - a bad beat is the one line the table remembers.
            for (int i = 0; i < seatCount; i++)
                if (i != best && seatState[i] == H_IN && seatKind[i] == K_NPC &&
                    seatWon[i] == 0)
                {
                    Say(ScoreSeat(i) / 759375 >= CAT_TRIPS ? T_BADBEAT : T_LOSE, i);
                    break;
                }
            Settle();
            ReactToResult();
            showUntil = Time.time + showSeconds;
            Sync();
        }

        void DealerPlay()
        {
            phase = PH_SHOW;
            turnSeat = -1;
            pendingShow = false;   // blackjack has no paced showdown
            // The hole card turns over, then the table draws to 17 and
            // stands on soft 17.
            dealerUpCount = 2;
            RevealDealer();
            int guard = 0;
            while (guard++ < 8)
            {
                int total = DealerTotal();
                if (total >= 17)
                    break;
                int slot = DealerCardCount();
                if (slot >= 6 || deckPos >= 52)
                    break;
                dealerCards[slot] = DrawCard();
                PlaceDealerCard(slot, dealerCards[slot], true);
                dealerUpCount = slot + 1;
            }
            int dv = DealerTotal();
            bool dealerNatural = DealerCardCount() == 2 && dv == 21;
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN && seatState[i] != H_STAND &&
                    seatState[i] != H_BUST)
                    continue;
                RevealSeat(i);
                shownMask = shownMask | (1 << i);
                int stake = seatPaid[i];
                int pv = HandValue(i);
                bool natural = SeatCardCount(i) == 2 && pv == 21;
                int won;
                if (seatState[i] == H_BUST || pv > 21)
                    won = 0;
                else if (natural && dealerNatural)
                    won = stake;
                else if (natural)
                    won = stake + (stake * 3) / 2;   // 3:2, rounded down
                else if (dealerNatural)
                    won = 0;
                else if (dv > 21 || pv > dv)
                    won = stake * 2;
                else if (pv == dv)
                    won = stake;
                else
                    won = 0;
                seatWon[i] = won;
                if (seatKind[i] == K_NPC)
                    seatChips[i] += won;
            }
            message = "Dealer " + (dv > 21 ? "busts" : "" + dv);
            bool won1 = false, lost1 = false;
            for (int i = 0; i < seatCount; i++)
            {
                if (seatKind[i] != K_NPC || seatPaid[i] <= 0)
                    continue;
                if (seatWon[i] > seatPaid[i] && !won1)
                {
                    Say(T_WIN, i);
                    won1 = true;
                }
                else if (seatWon[i] == 0 && !lost1 && seatState[i] != H_BUST)
                {
                    Say(T_LOSE, i);
                    lost1 = true;
                }
            }
            Settle();
            ReactToResult();
            showUntil = Time.time + showSeconds;
            Sync();
        }

        /// One delta per seat, one serial bump. Each client applies its own
        /// player's delta and nobody else's - see the header.
        void Settle()
        {
            for (int i = 0; i < seatCount; i++)
                seatResult[i] = seatWon[i] - seatPaid[i];
            settleSerial++;
        }

        void ApplySettlement()
        {
            if (settleSerial == appliedSerial)
                return;
            appliedSerial = settleSerial;
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null || wallet == null || seatResult == null)
                return;
            for (int i = 0; i < seatCount && i < seatResult.Length; i++)
            {
                if (seatKind[i] != K_PLAYER || seatPlayerId[i] != local.playerId)
                    continue;
                int d = seatResult[i];
                if (d < 0)
                    wallet.Spend(-d);
                else if (d > 0)
                    wallet.Add(d);
            }
        }

        void ReactToResult()
        {
            if (brainAt == null)
                return;
            for (int i = 0; i < seatCount; i++)
            {
                if (brainAt[i] == null || seatKind[i] != K_NPC)
                    continue;
                if (seatPaid[i] <= 0)
                    continue;
                brainAt[i].Speak(seatWon[i] > seatPaid[i] ? 5 : 13, 3f);
            }
        }

        void EndHand()
        {
            handsCompleted++;
            pot = 0;
            phase = PH_IDLE;
            turnSeat = -1;
            communityUp = 0;
            shownMask = 0;
            street = 0;
            pendingCommunity = false;
            pendingShow = false;
            for (int i = 0; i < seatCount; i++)
            {
                seatBet[i] = 0;
                seatState[i] = H_OUT;
            }
            handHadPlayer = false;
            nextDeal = Time.time + selfPlayGap;
            message = AnyPlayerSeated() ? "Press Deal" : "";
            ReturnAllCards();
            Sync();
        }

        // --- player / NPC actions ------------------------------------------------

        /// The one entry point for a seat's action. Runs on the OWNER only:
        /// clients reach it through SendCustomNetworkEvent(Owner, ...), and
        /// the owner checks that the CALLING player really holds the seat.
        [NetworkCallable]
        public void Act(int seat, int action)
        {
            if (!isLocalOwner)
                return;
            if (seatKind == null || seat < 0 || seat >= seatCount)
                return;
            VRCPlayerApi caller = NetworkCalling.CallingPlayer;
            if (caller != null && caller.IsValid())
            {
                // A networked call must come from the seat's occupant.
                if (seatKind[seat] != K_PLAYER || seatPlayerId[seat] != caller.playerId)
                    return;
            }
            else if (seatKind[seat] == K_EMPTY)
            {
                return;   // local call from a seat nobody holds
            }

            if (action == A_DEAL)
            {
                if (phase == PH_IDLE)
                    StartHand();
                return;
            }
            if (action == A_MODE)
            {
                // Poker / blackjack only: WHICH poker is the town clock's
                // call, never a button (see PollRules).
                if (phase != PH_IDLE)
                    return;
                mode = mode == MODE_POKER ? MODE_BJ : MODE_POKER;
                message = mode == MODE_BJ ? "Blackjack" : RulesName();
                Sync();
                return;
            }
            if (turnSeat != seat || seatState[seat] != H_IN)
                return;

            if (mode == MODE_BJ)
            {
                if (action == A_HIT)
                    Hit(seat);
                else if (action == A_STAND)
                {
                    seatState[seat] = H_STAND;
                    AdvanceTurn();
                }
                return;
            }

            if (phase != PH_BET)
                return;
            if (action == A_CALL)
            {
                DoCall(seat);
                AdvanceTurn();
            }
            else if (action == A_RAISE)
            {
                if (DoRaise(seat) && brainAt != null && brainAt[seat] != null)
                    brainAt[seat].Speak(1, 2.5f);
                AdvanceTurn();
            }
            else if (action == A_FOLD)
            {
                DoFold(seat);
                AdvanceTurn();
            }
        }

        void Hit(int seat)
        {
            int slot = SeatCardCount(seat);
            if (slot >= 5 || deckPos >= 52)
            {
                seatState[seat] = H_STAND;
                AdvanceTurn();
                return;
            }
            int card = DrawCard();
            seatCards[seat * 5 + slot] = card;
            PlaceCard(seat, slot, card, seatKind[seat] == K_PLAYER);
            if (HandValue(seat) > 21)
            {
                seatState[seat] = H_BUST;
                if (seatKind[seat] == K_NPC)
                    Say(T_BUST, seat);
                AdvanceTurn();
                return;
            }
            if (HandValue(seat) == 21 || slot + 1 >= 5)
            {
                seatState[seat] = H_STAND;
                AdvanceTurn();
                return;
            }
            ArmTurn();
            Sync();
        }

        /// A human seat that let the clock run out: check when the call is
        /// free, fold when it costs coins.
        void TimeoutAct(int seat)
        {
            if (mode == MODE_BJ)
            {
                seatState[seat] = H_STAND;
                AdvanceTurn();
                return;
            }
            if (currentBet - seatBet[seat] <= 0)
                DoCall(seat);
            else
                DoFold(seat);
            AdvanceTurn();
        }

        // --- the villagers' play -------------------------------------------------

        void NpcAct(int seat)
        {
            if (mode == MODE_BJ)
            {
                if (BjShouldHit(seat))
                    Hit(seat);
                else
                {
                    seatState[seat] = H_STAND;
                    AdvanceTurn();
                }
                return;
            }
            int owed = currentBet - seatBet[seat];
            int nerve = Personality(seat);          // 0..99, higher = bolder
            bool canRaise = raises < maxRaises && CanCover(seat, owed + betStep);
            int strength = HandStrength(seat);      // 0..99

            if (strength >= 74)
            {
                if (canRaise)
                    NpcRaise(seat);
                else
                    NpcCall(seat);
            }
            else if (strength >= 56)
            {
                if (canRaise && nerve > 60)
                    NpcRaise(seat);
                else
                    NpcCall(seat);
            }
            else if (strength >= 36)
            {
                if (owed <= betStep || nerve > 70)
                    NpcCall(seat);
                else
                    Fold(seat);
            }
            else
            {
                if (owed <= 0)
                {
                    // A free look: a bold villager takes a swing at it.
                    if (canRaise && nerve > 85 && raises == 0)
                        NpcRaise(seat);
                    else
                        NpcCall(seat);
                }
                else if (nerve > 92 && raises == 0 && CanCover(seat, owed))
                    NpcCall(seat);   // a bluff-call
                else
                    Fold(seat);
            }
            AdvanceTurn();
        }

        /// How good a villager thinks its hand is, 0..99. Pre-flop that is
        /// the two hole cards alone (a pair, two pictures, suited,
        /// connected); once there is a board it is the made-hand category
        /// of the best five out of what it can see - discounted when the
        /// board alone makes that hand, because a villager playing the
        /// board is not beating anybody with it.
        public int HandStrength(int seat)
        {
            if (mode == MODE_POKER && rules == RULES_HOLDEM && communityUp < 3)
                return PreflopStrength(seatCards[seat * 5], seatCards[seat * 5 + 1]);
            int cat = ScoreSeat(seat) / 759375;
            int s = CategoryStrength(cat);
            if (rules == RULES_HOLDEM && communityUp >= 5 && cat == BoardCategory())
                s = s / 2 + 8;   // playing the board
            return s > 99 ? 99 : s;
        }

        int CategoryStrength(int cat)
        {
            if (cat >= CAT_QUADS) return 99;
            if (cat == CAT_FULL) return 95;
            if (cat == CAT_FLUSH) return 88;
            if (cat == CAT_STRAIGHT) return 84;
            if (cat == CAT_TRIPS) return 78;
            if (cat == CAT_TWOPAIR) return 62;
            if (cat == CAT_PAIR) return 44;
            return 20;
        }

        /// The category the five board cards make on their own.
        int BoardCategory()
        {
            if (community == null || communityUp < 5)
                return -1;
            int sc = Eval5(community[0], community[1], community[2],
                community[3], community[4]);
            return sc < 0 ? -1 : sc / 759375;
        }

        /// Two hole cards, 0..99: a pair is worth its rank, two pictures
        /// beat two rags, and suited or connected is worth a few points on
        /// top - the shape of every starting-hand chart, without the chart.
        public int PreflopStrength(int a, int b)
        {
            if (a < 0 || b < 0)
                return 0;
            int va = RankValue(a), vb = RankValue(b);
            int hi = va > vb ? va : vb;
            int lo = va > vb ? vb : va;
            bool suited = a / 13 == b / 13;
            int gap = hi - lo;
            int s;
            if (va == vb)
                s = 52 + (hi - 2) * 4;              // 52 (deuces) .. 100 (aces)
            else
            {
                s = 8 + (hi - 2) * 2 + (lo - 2);    // high card carries it
                if (hi >= 13 && lo >= 10)
                    s += 12;                        // two pictures
                if (gap <= 2)
                    s += 6;                         // connected
                if (suited)
                    s += 8;
            }
            if (s < 0)
                s = 0;
            return s > 99 ? 99 : s;
        }

        void NpcRaise(int seat)
        {
            if (!DoRaise(seat))
                return;
            if (brainAt != null && brainAt[seat] != null)
                brainAt[seat].Speak(1, 2.5f);
            Say(T_RAISE, seat);
        }

        void NpcCall(int seat)
        {
            DoCall(seat);
            SayMaybe(T_CALL, seat, 35);
        }

        void Fold(int seat)
        {
            DoFold(seat);
            if (brainAt != null && brainAt[seat] != null)
                brainAt[seat].Speak(0, 2f);
            SayMaybe(T_FOLD, seat, 70);
        }

        /// Nerve, 0..99, from the villager's own personality seed - the same
        /// villager plays the same way every night.
        int Personality(int seat)
        {
            if (brainAt == null || brainAt[seat] == null)
                return 50;
            int s = brainAt[seat].seed;
            int h = s * 1664525 + 1013904223;
            h = (h >> 8) & 0x7FFFFFFF;
            return h % 100;
        }

        bool BjShouldHit(int seat)
        {
            int up = BjCardValue(dealerCards[0]);
            int total = HandValue(seat);
            bool soft = HandSoft(seat);
            if (soft)
                return total <= 17 || (total == 18 && up >= 9);
            if (total <= 11)
                return true;
            if (total == 12)
                return up < 4 || up > 6;
            if (total <= 16)
                return up >= 7;
            return false;
        }

        // --- hand evaluation ------------------------------------------------------

        /// Ace high (14), king 13 ... deuce 2. Card index = rank + 13*suit,
        /// rank 0 = ace.
        public int RankValue(int card)
        {
            int r = card % 13;
            return r == 0 ? 14 : r + 1;
        }

        public int BjCardValue(int card)
        {
            if (card < 0)
                return 0;
            int r = card % 13;
            if (r == 0)
                return 11;
            return r >= 9 ? 10 : r + 1;
        }

        /// A seat's hand under the rules in play: the five it holds in the
        /// night game, or the best five of its two plus whatever of the
        /// board is face up in hold'em (so an AI reading the flop is
        /// scoring the same seven a player can see, never the buried
        /// cards). -1 while a seat holds too few cards to make a hand.
        public int ScoreSeat(int seat)
        {
            if (mode == MODE_POKER && rules == RULES_HOLDEM)
                return BestOfSeven(seatCards[seat * 5], seatCards[seat * 5 + 1],
                    BoardAt(0), BoardAt(1), BoardAt(2), BoardAt(3), BoardAt(4));
            return Eval5(seatCards[seat * 5], seatCards[seat * 5 + 1],
                seatCards[seat * 5 + 2], seatCards[seat * 5 + 3],
                seatCards[seat * 5 + 4]);
        }

        /// Board card `i`, or -1 while it is still face down.
        int BoardAt(int i)
        {
            if (community == null || i < 0 || i >= community.Length || i >= communityUp)
                return -1;
            return community[i];
        }

        /// The best five-card score out of up to seven cards (-1 marks a
        /// card that is not there). Fewer than five is no hand at all;
        /// exactly five is Eval5, which stays the exact evaluator every
        /// other reader and the checks pin.
        public int BestOfSeven(int a, int b, int c, int d, int e, int f, int g)
        {
            int[] pool = new int[7];
            int n = 0;
            if (a >= 0) { pool[n] = a; n++; }
            if (b >= 0) { pool[n] = b; n++; }
            if (c >= 0) { pool[n] = c; n++; }
            if (d >= 0) { pool[n] = d; n++; }
            if (e >= 0) { pool[n] = e; n++; }
            if (f >= 0) { pool[n] = f; n++; }
            if (g >= 0) { pool[n] = g; n++; }
            if (n < 5)
                return -1;
            int best = -1;
            for (int i0 = 0; i0 <= n - 5; i0++)
                for (int i1 = i0 + 1; i1 <= n - 4; i1++)
                    for (int i2 = i1 + 1; i2 <= n - 3; i2++)
                        for (int i3 = i2 + 1; i3 <= n - 2; i3++)
                            for (int i4 = i3 + 1; i4 <= n - 1; i4++)
                            {
                                int sc = Eval5(pool[i0], pool[i1], pool[i2],
                                    pool[i3], pool[i4]);
                                if (sc > best)
                                    best = sc;
                            }
            return best;
        }

        /// One comparable number for a five-card hand: category in base
        /// 15^5, then five kicker ranks in descending significance. Higher
        /// always beats lower, kickers included.
        public int Eval5(int a, int b, int c, int d, int e)
        {
            if (a < 0 || b < 0 || c < 0 || d < 0 || e < 0)
                return -1;
            int[] v = new int[5];
            v[0] = RankValue(a); v[1] = RankValue(b); v[2] = RankValue(c);
            v[3] = RankValue(d); v[4] = RankValue(e);
            int suit = a / 13;
            bool flush = b / 13 == suit && c / 13 == suit && d / 13 == suit &&
                         e / 13 == suit;

            int[] cnt = new int[15];
            for (int i = 0; i < 5; i++)
                cnt[v[i]]++;

            // Straights, wheel included: at high = 5 the run asks for a 1,
            // which is the ace read low.
            int straightHigh = 0;
            for (int hi = 14; hi >= 5; hi--)
            {
                bool ok = true;
                for (int k = 0; k < 5; k++)
                {
                    int need = hi - k;
                    if (need == 1)
                        need = 14;
                    if (cnt[need] == 0)
                    {
                        ok = false;
                        break;
                    }
                }
                if (ok)
                {
                    straightHigh = hi;
                    break;
                }
            }

            int quad = 0, trip = 0, pair1 = 0, pair2 = 0;
            for (int r = 14; r >= 2; r--)
            {
                if (cnt[r] == 4)
                    quad = r;
                else if (cnt[r] == 3)
                    trip = r;
                else if (cnt[r] == 2)
                {
                    if (pair1 == 0)
                        pair1 = r;
                    else if (pair2 == 0)
                        pair2 = r;
                }
            }

            int cat;
            int k0 = 0, k1 = 0, k2 = 0, k3 = 0, k4 = 0;
            if (flush && straightHigh > 0)
            {
                cat = CAT_SFLUSH;
                k0 = straightHigh;
            }
            else if (quad > 0)
            {
                cat = CAT_QUADS;
                k0 = quad;
                k1 = Singles(cnt, 1, 0);
            }
            else if (trip > 0 && pair1 > 0)
            {
                cat = CAT_FULL;
                k0 = trip;
                k1 = pair1;
            }
            else if (flush)
            {
                cat = CAT_FLUSH;
                k0 = Nth(cnt, 0); k1 = Nth(cnt, 1); k2 = Nth(cnt, 2);
                k3 = Nth(cnt, 3); k4 = Nth(cnt, 4);
            }
            else if (straightHigh > 0)
            {
                cat = CAT_STRAIGHT;
                k0 = straightHigh;
            }
            else if (trip > 0)
            {
                cat = CAT_TRIPS;
                k0 = trip;
                k1 = Singles(cnt, 1, 0);
                k2 = Singles(cnt, 1, 1);
            }
            else if (pair1 > 0 && pair2 > 0)
            {
                cat = CAT_TWOPAIR;
                k0 = pair1;
                k1 = pair2;
                k2 = Singles(cnt, 1, 0);
            }
            else if (pair1 > 0)
            {
                cat = CAT_PAIR;
                k0 = pair1;
                k1 = Singles(cnt, 1, 0);
                k2 = Singles(cnt, 1, 1);
                k3 = Singles(cnt, 1, 2);
            }
            else
            {
                cat = CAT_HIGH;
                k0 = Nth(cnt, 0); k1 = Nth(cnt, 1); k2 = Nth(cnt, 2);
                k3 = Nth(cnt, 3); k4 = Nth(cnt, 4);
            }
            return cat * 759375 + k0 * 50625 + k1 * 3375 + k2 * 225 + k3 * 15 + k4;
        }

        // The `n`th rank (descending) counted with multiplicity.
        int Nth(int[] cnt, int n)
        {
            int seen = 0;
            for (int r = 14; r >= 2; r--)
                for (int k = 0; k < cnt[r]; k++)
                {
                    if (seen == n)
                        return r;
                    seen++;
                }
            return 0;
        }

        // The `n`th rank (descending) whose count is exactly `want`.
        int Singles(int[] cnt, int want, int n)
        {
            int seen = 0;
            for (int r = 14; r >= 2; r--)
            {
                if (cnt[r] != want)
                    continue;
                if (seen == n)
                    return r;
                seen++;
            }
            return 0;
        }

        public string CategoryName(int cat)
        {
            if (cat == CAT_SFLUSH) return "straight flush";
            if (cat == CAT_QUADS) return "four of a kind";
            if (cat == CAT_FULL) return "full house";
            if (cat == CAT_FLUSH) return "flush";
            if (cat == CAT_STRAIGHT) return "straight";
            if (cat == CAT_TRIPS) return "three of a kind";
            if (cat == CAT_TWOPAIR) return "two pair";
            if (cat == CAT_PAIR) return "a pair";
            return "high card";
        }

        int SeatCardCount(int seat)
        {
            int n = 0;
            for (int c = 0; c < 5; c++)
                if (seatCards[seat * 5 + c] >= 0)
                    n++;
            return n;
        }

        int DealerCardCount()
        {
            int n = 0;
            for (int c = 0; c < 6; c++)
                if (dealerCards[c] >= 0)
                    n++;
            return n;
        }

        /// Blackjack total for a seat: aces count 11 and demote to 1 while
        /// the hand is over 21.
        public int HandValue(int seat)
        {
            int total = 0, aces = 0;
            for (int c = 0; c < 5; c++)
            {
                int card = seatCards[seat * 5 + c];
                if (card < 0)
                    continue;
                total += BjCardValue(card);
                if (card % 13 == 0)
                    aces++;
            }
            while (total > 21 && aces > 0)
            {
                total -= 10;
                aces--;
            }
            return total;
        }

        bool HandSoft(int seat)
        {
            int total = 0, aces = 0;
            for (int c = 0; c < 5; c++)
            {
                int card = seatCards[seat * 5 + c];
                if (card < 0)
                    continue;
                total += BjCardValue(card);
                if (card % 13 == 0)
                    aces++;
            }
            while (total > 21 && aces > 0)
            {
                total -= 10;
                aces--;
            }
            return aces > 0;
        }

        public int DealerTotal()
        {
            int total = 0, aces = 0;
            for (int c = 0; c < 6; c++)
            {
                int card = dealerCards[c];
                if (card < 0)
                    continue;
                total += BjCardValue(card);
                if (card % 13 == 0)
                    aces++;
            }
            while (total > 21 && aces > 0)
            {
                total -= 10;
                aces--;
            }
            return total;
        }

        /// Blackjack settlement for one hand, factored out so the editor
        /// checks can drive it without a table: `stake` back plus winnings.
        public int BlackjackPayout(int playerTotal, int playerCards,
            int dealerTotalValue, int dealerCardsCount, int stake)
        {
            bool pNat = playerCards == 2 && playerTotal == 21;
            bool dNat = dealerCardsCount == 2 && dealerTotalValue == 21;
            if (playerTotal > 21)
                return 0;
            if (pNat && dNat)
                return stake;
            if (pNat)
                return stake + (stake * 3) / 2;
            if (dNat)
                return 0;
            if (dealerTotalValue > 21 || playerTotal > dealerTotalValue)
                return stake * 2;
            if (playerTotal == dealerTotalValue)
                return stake;
            return 0;
        }

        // --- the deck --------------------------------------------------------------

        int NextSeed()
        {
            int t = Networking.GetServerTimeInMilliseconds();
            int s = t ^ (handsCompleted * 1664525) ^ 0x5BD1E995;
            return s == 0 ? 1 : s;
        }

        /// Fisher-Yates over 0..51 driven by an int LCG, the same shape the
        /// deck controller uses: the order is a function of the seed alone,
        /// so a client that takes the table over rebuilds it exactly.
        void BuildDeck(int seed)
        {
            if (deckOrder == null || deckOrder.Length != 52)
                deckOrder = new int[52];
            for (int i = 0; i < 52; i++)
                deckOrder[i] = i;
            int state = seed == 0 ? 1 : seed;
            for (int i = 51; i > 0; i--)
            {
                state = state * 1664525 + 1013904223;
                int r = (state >> 8) & 0x7FFFFFFF;
                int j = r % (i + 1);
                int t = deckOrder[i];
                deckOrder[i] = deckOrder[j];
                deckOrder[j] = t;
            }
            deckSeedBuilt = seed;
        }

        int DrawCard()
        {
            if (deckSeedBuilt != shuffleSeed)
                BuildDeck(shuffleSeed);
            if (deckPos >= 52)
                return deckOrder[51];
            int card = deckOrder[deckPos];
            deckPos++;
            return card;
        }

        // --- the physical cards -------------------------------------------------

        void TakeCard(LegaiaCard card)
        {
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null || card == null)
                return;
            if (!Networking.IsOwner(card.gameObject))
                Networking.SetOwner(local, card.gameObject);
        }

        void PlaceCard(int seat, int slot, int cardIndex, bool faceUp)
        {
            if (cards == null || cardIndex < 0 || cardIndex >= cards.Length)
                return;
            if (handAnchors == null || seat >= handAnchors.Length ||
                handAnchors[seat] == null)
                return;
            LegaiaCard card = cards[cardIndex];
            if (card == null)
                return;
            TakeCard(card);
            Transform a = handAnchors[seat];
            // The fan is centred on however many cards THESE rules deal:
            // two hole cards sit in front of the seat, not off to its left.
            float off = slot - (HandSlots() - 1) * 0.5f;
            Vector3 p = a.position + a.right * (off * 0.075f);
            Quaternion r = a.rotation * Quaternion.Euler(0f, off * 5f, 0f);
            card.Deal(p, r, faceUp);
        }

        void PlaceCommunityCard(int slot, int cardIndex, bool faceUp)
        {
            if (cards == null || cardIndex < 0 || cardIndex >= cards.Length)
                return;
            if (communityAnchors == null || slot < 0 ||
                slot >= communityAnchors.Length || communityAnchors[slot] == null)
                return;
            LegaiaCard card = cards[cardIndex];
            if (card == null)
                return;
            TakeCard(card);
            Transform a = communityAnchors[slot];
            card.Deal(a.position, a.rotation, faceUp);
        }

        /// Turn board card `slot` over where it lies - the master's half of
        /// a staged reveal (the count is what every other client renders).
        void RevealCommunity(int slot)
        {
            if (cards == null || community == null || slot < 0 ||
                slot >= community.Length)
                return;
            int idx = community[slot];
            if (idx < 0 || idx >= cards.Length || cards[idx] == null)
                return;
            TakeCard(cards[idx]);
            cards[idx].Reveal();
        }

        void PlaceDealerCard(int slot, int cardIndex, bool faceUp)
        {
            if (cards == null || cardIndex < 0 || cardIndex >= cards.Length)
                return;
            if (dealerAnchor == null)
                return;
            LegaiaCard card = cards[cardIndex];
            if (card == null)
                return;
            TakeCard(card);
            Vector3 p = dealerAnchor.position + dealerAnchor.right * ((slot - 2) * 0.075f);
            Quaternion r = dealerAnchor.rotation *
                Quaternion.Euler(0f, (slot - 2) * 5f, 0f);
            card.Deal(p, r, faceUp);
        }

        void RevealSeat(int seat)
        {
            if (cards == null)
                return;
            for (int c = 0; c < 5; c++)
            {
                int idx = seatCards[seat * 5 + c];
                if (idx < 0 || idx >= cards.Length || cards[idx] == null)
                    continue;
                TakeCard(cards[idx]);
                cards[idx].Reveal();
            }
        }

        void RevealDealer()
        {
            if (cards == null)
                return;
            for (int c = 0; c < 6; c++)
            {
                int idx = dealerCards[c];
                if (idx < 0 || idx >= cards.Length || cards[idx] == null)
                    continue;
                TakeCard(cards[idx]);
                cards[idx].Reveal();
            }
        }

        void ReturnCard(int cardIndex)
        {
            if (cards == null || stackAnchor == null)
                return;
            if (cardIndex < 0 || cardIndex >= cards.Length || cards[cardIndex] == null)
                return;
            TakeCard(cards[cardIndex]);
            cards[cardIndex].Deal(
                stackAnchor.position + stackAnchor.up * (0.0007f * (cardIndex % 52)),
                stackAnchor.rotation, false);
        }

        /// A fold takes the seat's WHOLE hand off the felt in one go - both
        /// hole cards in hold'em, all five in the night game - so a folded
        /// seat is read as folded from across the table and not just on the
        /// panel.
        void ReturnSeatCards(int seat)
        {
            for (int c = 0; c < 5; c++)
            {
                ReturnCard(seatCards[seat * 5 + c]);
                seatCards[seat * 5 + c] = -1;
            }
            shownMask = shownMask & ~(1 << seat);
        }

        /// Re-stack everything the table dealt. Only the dealt cards move:
        /// a card a player carried off comes back, one nobody touched
        /// never left the stack.
        void ReturnAllCards()
        {
            for (int i = 0; i < seatCount; i++)
                for (int c = 0; c < 5; c++)
                {
                    ReturnCard(seatCards[i * 5 + c]);
                    seatCards[i * 5 + c] = -1;
                }
            for (int c = 0; c < 6; c++)
            {
                ReturnCard(dealerCards[c]);
                dealerCards[c] = -1;
            }
            if (community == null)
                return;
            for (int c = 0; c < community.Length; c++)
            {
                ReturnCard(community[c]);
                community[c] = -1;
            }
        }

        // --- the seat panel --------------------------------------------------------

        /// The panel's buttons all land here (SendCustomEvent from the UI
        /// listener), resolve the presser's own seat, and go to the owner.
        public void UiDeal() { SendAct(A_DEAL); }
        public void UiMode() { SendAct(A_MODE); }
        public void UiCall() { SendAct(A_CALL); }
        public void UiRaise() { SendAct(A_RAISE); }
        public void UiFold() { SendAct(A_FOLD); }
        public void UiHit() { SendAct(A_HIT); }
        public void UiStand() { SendAct(A_STAND); }

        void SendAct(int action)
        {
            int seat = LocalSeat();
            if (seat < 0)
                return;
            if (Networking.LocalPlayer == null || Networking.IsOwner(gameObject))
            {
                Act(seat, action);
                return;
            }
            SendCustomNetworkEvent(NetworkEventTarget.Owner, "Act", seat, action);
        }

        /// The poker in play, named for the panel's mode line.
        public string RulesName()
        {
            return rules == RULES_NIGHT
                ? "Cara's poker night - best hand wins"
                : "Texas hold'em";
        }

        void RefreshPanel()
        {
            if (seatKind == null)
                return;
            if (modeText != null)
                modeText.text = mode == MODE_BJ
                    ? "Blackjack  -  stake " + ante
                    : RulesName() + "  -  ante " + ante;
            if (potText != null)
                potText.text = mode == MODE_BJ
                    ? "Dealer " + (dealerUpCount >= 2 ? "" + DealerTotal()
                        : "showing " + BjCardValue(dealerCards[0]))
                    : "Pot " + pot;
            if (msgText != null)
                msgText.text = message;
            RefreshBoard();

            for (int i = 0; i < seatCount; i++)
                RefreshRow(i);
            RefreshButtons();
        }

        /// The board block: the community cards turned over so far (red
        /// suits tinted through rich text - one Text, several colours) and
        /// the local seat's best hand named. Blackjack and the night game
        /// have no board, so the line says what the seat is holding
        /// instead of pretending there is one.
        void RefreshBoard()
        {
            if (communityText != null)
            {
                string s = "";
                if (mode == MODE_POKER && rules == RULES_HOLDEM)
                {
                    for (int i = 0; i < 5; i++)
                    {
                        int idx = i < communityUp ? BoardAt(i) : -1;
                        s += (s.Length > 0 ? "   " : "") +
                             (idx < 0 ? "<color=#5a5650>--</color>" : TintedLabel(idx));
                    }
                }
                communityText.text = s;
            }
            if (handText == null)
                return;
            int seat = LocalSeat();
            if (seat < 0 || seatState == null || seat >= seatCount ||
                seatCards[seat * 5] < 0)
            {
                handText.text = "";
                return;
            }
            if (mode == MODE_BJ)
            {
                handText.text = "You hold " + HandValue(seat);
                return;
            }
            int score = ScoreSeat(seat);
            handText.text = score < 0
                ? "Your hand: " + TintedLabel(seatCards[seat * 5]) + " " +
                  TintedLabel(seatCards[seat * 5 + 1])
                : "Your hand: " + CategoryName(score / 759375);
        }

        /// A card label with the red suits tinted (Unity UI rich text).
        string TintedLabel(int card)
        {
            if (card < 0)
                return "--";
            return IsRedSuit(card)
                ? "<color=#f28a7a>" + CardLabel(card) + "</color>"
                : CardLabel(card);
        }

        // --- table talk ---------------------------------------------------------

        /// Seat `seat` speaks (a villager; players never do). The master
        /// composes, the synced string carries it. Callers sync.
        void Say(int kind, int seat)
        {
            if (!isLocalOwner || talk == null || seat < 0 || seat >= seatCount)
                return;
            if (seatKind[seat] != K_NPC && kind != T_LEAVE)
                return;
            string name = seatLabel != null && seatLabel[seat] != null &&
                          seatLabel[seat].Length > 0 ? seatLabel[seat] : "Villager";
            talkSalt++;
            string line = talk.Compose(kind, name, OthersAt(seat), talkSalt ^ shuffleSeed);
            if (line == null || line.Length == 0)
                return;
            seatTalk[seat] = line;
            if (seatTalkAt != null && seat < seatTalkAt.Length)
                seatTalkAt[seat] = Time.time + talkHold;
        }

        /// The seat Cara is sitting in, or -1. Her poker night is hers to
        /// open, so the night deal looks for her by name before it falls
        /// back to whoever is at the table.
        int CaraSeat()
        {
            if (seatLabel == null)
                return -1;
            for (int i = 0; i < seatCount; i++)
                if (seatKind[i] == K_NPC && seatLabel[i] != null &&
                    seatLabel[i].ToLower() == "cara")
                    return i;
            return -1;
        }

        bool SayCara(int kind)
        {
            int seat = CaraSeat();
            if (seat < 0)
                return false;
            Say(kind, seat);
            return true;
        }

        /// One of the seated villagers speaks - a different one each time.
        void SayAny(int kind)
        {
            if (seatCount < 1)
                return;
            talkSalt++;
            for (int k = 0; k < seatCount; k++)
            {
                int i = (talkSalt + k) % seatCount;
                if (seatKind[i] == K_NPC)
                {
                    Say(kind, i);
                    return;
                }
            }
        }

        /// Speak about `percent` of the time (calls would drown the panel).
        void SayMaybe(int kind, int seat, int percent)
        {
            talkSalt++;
            if (((talkSalt * 37 + shuffleSeed) & 0x7FFF) % 100 < percent)
                Say(kind, seat);
        }

        /// Everyone else at the table, lower-case, '|'-joined - what lets a
        /// villager talk ABOUT Vahn or Noa when they sit here.
        string OthersAt(int seat)
        {
            string s = "";
            for (int i = 0; i < seatCount; i++)
            {
                if (i == seat)
                    continue;
                string who = null;
                if (seatKind[i] == K_NPC && seatLabel != null && seatLabel[i] != null &&
                    seatLabel[i].Length > 0)
                    who = seatLabel[i].ToLower();
                else if (seatKind[i] == K_PLAYER)
                    who = "player";
                if (who == null)
                    continue;
                s = s.Length == 0 ? who : s + "|" + who;
            }
            return s;
        }

        bool AnyNpcSeated()
        {
            for (int i = 0; i < seatCount; i++)
                if (seatKind[i] == K_NPC)
                    return true;
            return false;
        }

        /// Master: fade each seat's line once it has had its time, and let
        /// the table mutter between hands.
        void TalkTick()
        {
            bool cleared = false;
            for (int i = 0; i < seatCount && seatTalk != null &&
                 i < seatTalk.Length; i++)
            {
                if (seatTalk[i] == null || seatTalk[i].Length == 0)
                    continue;
                if (seatTalkAt != null && i < seatTalkAt.Length &&
                    Time.time < seatTalkAt[i])
                    continue;
                seatTalk[i] = "";
                cleared = true;
            }
            if (cleared)
                Sync();
            if (phase == PH_IDLE && Time.time >= nextIdleTalk)
            {
                nextIdleTalk = Time.time + idleTalkGap + (talkSalt % 5) * 2f;
                if (AnyNpcSeated())
                {
                    SayAny(T_IDLE);
                    Sync();
                }
            }
        }

        void RefreshRow(int i)
        {
            string name = "-";
            string coins = "";
            Texture2D portrait = null;
            bool player = seatKind[i] == K_PLAYER;
            if (player)
            {
                VRCPlayerApi p = VRCPlayerApi.GetPlayerById(seatPlayerId[i]);
                name = p != null && p.IsValid() ? p.displayName : "player";
                coins = wallet != null && p != null ? "" + wallet.CoinsOf(p) : "";
            }
            else if (seatKind[i] == K_NPC)
            {
                LegaiaNpcBrain b = brainAt != null ? brainAt[i] : null;
                name = b != null && b.label != null && b.label.Length > 0
                    ? b.label : "villager";
                coins = "" + seatChips[i];
                if (b != null)
                    portrait = b.portrait;
            }
            else if (seatTalk != null && i < seatTalk.Length &&
                     seatTalk[i] != null && seatTalk[i].Length > 0 &&
                     seatLabel != null && seatLabel[i] != null &&
                     seatLabel[i].Length > 0)
            {
                // A villager who just stood up still gets its parting line
                // printed under the name it had.
                name = seatLabel[i];
            }
            if (rowName != null && i < rowName.Length && rowName[i] != null)
                rowName[i].text = name;
            if (rowTalk != null && i < rowTalk.Length && rowTalk[i] != null)
            {
                // The row already prints the name, so the quote drops the
                // "Name: " the composer puts in front (Vahn's stage
                // directions have none and pass through whole).
                string said = seatTalk != null && i < seatTalk.Length &&
                              seatTalk[i] != null ? seatTalk[i] : "";
                string tag = name + ": ";
                if (said.Length > tag.Length && said.StartsWith(tag))
                    said = said.Substring(tag.Length);
                rowTalk[i].text = said.Length > 0 ? "\"" + said + "\"" : "";
            }
            if (rowCoins != null && i < rowCoins.Length && rowCoins[i] != null)
                rowCoins[i].text = coins;
            if (rowStatus != null && i < rowStatus.Length && rowStatus[i] != null)
                rowStatus[i].text = StatusOf(i);
            if (rowPortrait != null && i < rowPortrait.Length && rowPortrait[i] != null)
            {
                bool show = seatKind[i] != K_EMPTY;
                rowPortrait[i].enabled = show;
                rowPortrait[i].texture = portrait;
                // A player has no portrait: the slot is a flat silhouette
                // tint, which is what an untextured RawImage draws.
                rowPortrait[i].color = portrait != null ? Color.white : playerTint;
            }
        }

        string StatusOf(int i)
        {
            if (seatKind[i] == K_EMPTY)
                return "Empty";
            if (seatState[i] == H_SITOUT)
                return "Sitting out";
            if (seatState[i] == H_FOLD)
                return "Folded";
            if (seatState[i] == H_BUST)
                return "Bust " + HandValue(i);
            if (phase == PH_SHOW && seatPaid[i] > 0)
            {
                int d = seatWon[i] - seatPaid[i];
                if (seatWon[i] > 0 && d > 0)
                    return "Winner +" + d;
                if (seatWon[i] > 0)
                    return "Back " + seatWon[i];
                return "Lost " + seatPaid[i];
            }
            if (seatState[i] == H_STAND)
                return "Stand " + HandValue(i);
            if (phase != PH_IDLE && turnSeat == i)
                return "Turn";
            if (seatState[i] == H_IN && seatBet[i] > 0)
                return "Bet " + seatBet[i];
            if (seatState[i] == H_IN)
                return mode == MODE_BJ ? "" + HandValue(i) : "In";
            return "Ready";
        }

        void RefreshButtons()
        {
            int ls = LocalSeat();
            bool seated = ls >= 0;
            bool myTurn = seated && turnSeat == ls && seatState[ls] == H_IN;
            bool betting = myTurn && mode == MODE_POKER && phase == PH_BET;
            bool bj = myTurn && mode == MODE_BJ && phase == PH_BET;

            Enable(btnDeal, seated && phase == PH_IDLE);
            Enable(btnMode, seated && phase == PH_IDLE);
            Enable(btnCall, betting);
            Enable(btnRaise, betting && raises < maxRaises);
            Enable(btnFold, betting);
            Enable(btnHit, bj);
            Enable(btnStand, bj);

            if (btnDealText != null)
                btnDealText.text = phase == PH_IDLE ? "Deal" : "Playing";
            if (btnCallText != null)
                btnCallText.text = seated && currentBet - seatBet[ls] > 0
                    ? "Call " + (currentBet - seatBet[ls]) : "Check";
            if (btnRaiseText != null)
                btnRaiseText.text = currentBet > 0
                    ? "Raise +" + betStep : "Bet " + betStep;
        }

        void Enable(Button b, bool on)
        {
            if (b != null)
                b.interactable = on;
        }

        bool IsRedSuit(int card)
        {
            int s = card / 13;
            return s == 1 || s == 2;   // hearts, diamonds (build order)
        }

        public string CardLabel(int card)
        {
            if (card < 0)
                return "-";
            return RankLabel(card % 13) + SuitLabel(card / 13);
        }

        string RankLabel(int r)
        {
            if (r == 0) return "A";
            if (r == 1) return "2";
            if (r == 2) return "3";
            if (r == 3) return "4";
            if (r == 4) return "5";
            if (r == 5) return "6";
            if (r == 6) return "7";
            if (r == 7) return "8";
            if (r == 8) return "9";
            if (r == 9) return "10";
            if (r == 10) return "J";
            if (r == 11) return "Q";
            return "K";
        }

        string SuitLabel(int s)
        {
            if (s == 0) return "S";
            if (s == 1) return "H";
            if (s == 2) return "D";
            return "C";
        }

        string SeatName(int i)
        {
            if (seatKind[i] == K_PLAYER)
            {
                VRCPlayerApi p = VRCPlayerApi.GetPlayerById(seatPlayerId[i]);
                if (p != null && p.IsValid())
                    return p.displayName;
                return "Player";
            }
            LegaiaNpcBrain b = brainAt != null ? brainAt[i] : null;
            if (b != null && b.label != null && b.label.Length > 0)
                return b.label;
            return "Seat " + (i + 1);
        }
    }
}
