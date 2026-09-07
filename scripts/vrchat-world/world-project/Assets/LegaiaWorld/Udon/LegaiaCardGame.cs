// The card table's dealer: five-card draw poker and blackjack, played at
// the four stools by players AND by the town's villagers, for the coins
// the wallet holds.
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

        [Tooltip("The deck's stack anchor - discards and undealt cards go back here.")]
        public Transform stackAnchor;

        // --- panel widgets ----------------------------------------------------

        public Text modeText;
        public Text potText;
        public Text msgText;
        public RawImage[] rowPortrait;
        public Text[] rowName;
        public Text[] rowCoins;
        public Text[] rowStatus;
        public Button btnDeal;
        public Button btnMode;
        public Button btnCall;
        public Button btnRaise;
        public Button btnFold;
        public Button btnDraw;
        public Button btnHit;
        public Button btnStand;
        public Button[] btnHold;
        public Text btnCallText;
        public Text btnRaiseText;
        public Text btnDealText;
        public Text[] btnHoldText;

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

        [Tooltip("Gap between hands while the villagers play among themselves.")]
        public float selfPlayGap = 8f;

        [Tooltip("Villagers keep playing at an empty table.")]
        public bool npcSelfPlay = true;

        [Tooltip("Virtual chips a villager sits down with (refilled every time it takes a stool).")]
        public int npcStartChips = 60;

        [Tooltip("How far the director may reach for a villager to fill a stool, metres.")]
        public float summonRadius = 60f;

        [Tooltip("Seconds between summon attempts.")]
        public float summonInterval = 1.5f;

        [Tooltip("Set by the editor unit tests: never call RequestSerialization.")]
        [System.NonSerialized] public bool suppressSerialization;

        // --- synced state -----------------------------------------------------

        [UdonSynced] public int mode;            // 0 poker, 1 blackjack
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
        [UdonSynced] public string message = "";

        [UdonSynced] public int[] seatKind;      // 0 empty, 1 player, 2 npc
        [UdonSynced] public int[] seatPlayerId;
        [UdonSynced] public int[] seatChips;     // villagers' virtual chips
        [UdonSynced] public int[] seatState;     // see H_*
        [UdonSynced] public int[] seatBet;       // committed THIS betting round
        [UdonSynced] public int[] seatPaid;      // committed this hand
        [UdonSynced] public int[] seatWon;       // taken from the pot this hand
        [UdonSynced] public int[] seatResult;    // net coin delta, applied on settleSerial
        [UdonSynced] public int[] seatHold;      // draw-phase hold bitmask
        [UdonSynced] public int[] seatCards;     // seat*5 + slot -> card index, -1 empty
        [UdonSynced] public int[] dealerCards;   // blackjack, -1 empty

        // --- phases / seat states / actions ------------------------------------

        const int MODE_POKER = 0, MODE_BJ = 1;
        const int PH_IDLE = 0, PH_BET1 = 2, PH_DRAW = 3, PH_BET2 = 4, PH_SHOW = 5;
        const int K_EMPTY = 0, K_PLAYER = 1, K_NPC = 2;
        const int H_OUT = 0, H_IN = 1, H_FOLD = 2, H_SITOUT = 3, H_STAND = 4, H_BUST = 5;
        const int A_CALL = 0, A_RAISE = 1, A_FOLD = 2, A_DRAW = 3, A_HIT = 4,
                  A_STAND = 5, A_DEAL = 6, A_MODE = 7, A_HOLD0 = 10;

        // Poker hand categories, low to high.
        const int CAT_HIGH = 0, CAT_PAIR = 1, CAT_TWOPAIR = 2, CAT_TRIPS = 3,
                  CAT_STRAIGHT = 4, CAT_FLUSH = 5, CAT_FULL = 6, CAT_QUADS = 7,
                  CAT_SFLUSH = 8;

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

        void Start()
        {
            seatCount = chairs != null ? chairs.Length
                : (stations != null ? stations.Length : 0);
            EnsureArrays();
            brainAt = new LegaiaNpcBrain[seatCount < 1 ? 1 : seatCount];
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
                seatHold = new int[n];
                seatCards = new int[n * 5];
                for (int i = 0; i < seatCards.Length; i++)
                    seatCards[i] = -1;
                for (int i = 0; i < n; i++)
                    seatPlayerId[i] = -1;
            }
            if (dealerCards == null || dealerCards.Length != 6)
            {
                dealerCards = new int[6];
                for (int i = 0; i < 6; i++)
                    dealerCards[i] = -1;
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
            RefreshPanel();
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
            if (phase == PH_IDLE)
            {
                MaybeAutoDeal();
                return;
            }
            if (phase == PH_SHOW)
            {
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
                if (kind == K_NPC && seatKind[i] != K_NPC)
                    seatChips[i] = npcStartChips; // refill on every re-seat
                seatKind[i] = kind;
                seatPlayerId[i] = id;
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
            for (int i = 0; i < seatCount; i++)
            {
                seatBet[i] = 0;
                seatPaid[i] = 0;
                seatWon[i] = 0;
                seatResult[i] = 0;
                seatHold[i] = 0;
                for (int c = 0; c < 5; c++)
                    seatCards[i * 5 + c] = -1;
                seatState[i] = seatKind[i] == K_EMPTY ? H_OUT : H_IN;
            }
            for (int i = 0; i < 6; i++)
                dealerCards[i] = -1;

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
            else
                DealPoker();
            Sync();
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
            nextDeal = Time.time + selfPlayGap;
            ReturnAllCards();
            Sync();
        }

        void DealPoker()
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
            message = "Betting";
            BeginBetRound(PH_BET1);
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
            for (int i = 0; i < seatCount; i++)
                if (seatState[i] == H_IN && HandValue(i) == 21)
                    seatState[i] = H_STAND;
            phase = PH_BET1;
            message = "Hit or stand";
            turnSeat = FirstActor(-1);
            ArmTurn();
            if (turnSeat < 0)
                DealerPlay();
        }

        // --- betting ------------------------------------------------------------

        void BeginBetRound(int ph)
        {
            phase = ph;
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
            if (phase == PH_BET1 || phase == PH_BET2)
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
                return;
            }
            if (phase == PH_DRAW)
            {
                turnSeat = NextUndrawn(turnSeat);
                if (turnSeat < 0)
                {
                    NextPhase();
                    return;
                }
                seatHold[turnSeat] = AiHoldMask(turnSeat);
                ArmTurn();
            }
        }

        // Each seat draws exactly once: `betActed` is reused as the
        // "has drawn" set (the betting round that set it is over).
        int NextUndrawn(int after)
        {
            for (int k = 1; k <= seatCount; k++)
            {
                int i = ((after < 0 ? seatCount - 1 : after) + k) % seatCount;
                if (seatState[i] == H_IN && (betActed & (1 << i)) == 0)
                    return i;
            }
            return -1;
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

        void NextPhase()
        {
            if (mode == MODE_BJ)
            {
                DealerPlay();
                return;
            }
            if (phase == PH_BET1)
            {
                phase = PH_DRAW;
                message = "Draw";
                betActed = 0;   // reused below as the "has drawn" set
                turnSeat = NextUndrawn(-1);
                if (turnSeat < 0)
                {
                    Showdown();
                    return;
                }
                seatHold[turnSeat] = AiHoldMask(turnSeat);
                ArmTurn();
                return;
            }
            if (phase == PH_DRAW)
            {
                message = "Betting";
                BeginBetRound(PH_BET2);
                return;
            }
            Showdown();
        }

        // --- draw ---------------------------------------------------------------

        void ResolveDraw(int seat)
        {
            int mask = seatHold[seat];
            for (int c = 0; c < 5; c++)
            {
                if ((mask & (1 << c)) != 0)
                    continue;
                if (deckPos >= 52)
                    break;
                ReturnCard(seatCards[seat * 5 + c]);
                int card = DrawCard();
                seatCards[seat * 5 + c] = card;
                PlaceCard(seat, c, card, seatKind[seat] == K_PLAYER);
            }
            betActed = betActed | (1 << seat);
            AdvanceTurn();
        }

        // --- showdown / settlement ----------------------------------------------

        void Showdown()
        {
            phase = PH_SHOW;
            turnSeat = -1;
            int best = -1;
            int bestScore = -1;
            int winners = 0;
            for (int i = 0; i < seatCount; i++)
            {
                if (seatState[i] != H_IN)
                    continue;
                RevealSeat(i);
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
            message = winners > 1
                ? "Split pot - " + CategoryName(bestScore / 759375)
                : SeatName(best) + " wins " + given + " (" +
                  CategoryName(bestScore / 759375) + ")";
            Settle();
            ReactToResult();
            showUntil = Time.time + showSeconds;
            Sync();
        }

        void DealerPlay()
        {
            phase = PH_SHOW;
            turnSeat = -1;
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
                if (phase != PH_IDLE)
                    return;
                mode = mode == MODE_POKER ? MODE_BJ : MODE_POKER;
                message = mode == MODE_POKER ? "Five-card draw" : "Blackjack";
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

            if (phase == PH_DRAW)
            {
                if (action >= A_HOLD0 && action < A_HOLD0 + 5)
                {
                    seatHold[seat] = seatHold[seat] ^ (1 << (action - A_HOLD0));
                    Sync();
                }
                else if (action == A_DRAW)
                {
                    ResolveDraw(seat);
                }
                return;
            }
            if (phase != PH_BET1 && phase != PH_BET2)
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
            if (phase == PH_DRAW)
            {
                ResolveDraw(seat);
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
            if (phase == PH_DRAW)
            {
                seatHold[seat] = AiHoldMask(seat);
                ResolveDraw(seat);
                return;
            }
            int cat = ScoreSeat(seat) / 759375;
            int owed = currentBet - seatBet[seat];
            int nerve = Personality(seat);          // 0..99, higher = bolder
            bool canRaise = raises < maxRaises && CanCover(seat, owed + betStep);

            if (cat >= CAT_TRIPS)
            {
                if (canRaise)
                {
                    if (DoRaise(seat) && brainAt != null && brainAt[seat] != null)
                        brainAt[seat].Speak(1, 2.5f);
                }
                else
                    DoCall(seat);
            }
            else if (cat == CAT_TWOPAIR)
            {
                if (canRaise && nerve > 60)
                {
                    if (DoRaise(seat) && brainAt != null && brainAt[seat] != null)
                        brainAt[seat].Speak(1, 2.5f);
                }
                else
                    DoCall(seat);
            }
            else if (cat == CAT_PAIR)
            {
                if (owed <= betStep || nerve > 70)
                    DoCall(seat);
                else
                    Fold(seat);
            }
            else
            {
                if (owed <= 0)
                {
                    // A free look: a bold villager takes a swing at it.
                    if (canRaise && nerve > 85 && raises == 0)
                    {
                        if (DoRaise(seat) && brainAt != null && brainAt[seat] != null)
                            brainAt[seat].Speak(1, 2.5f);
                    }
                    else
                        DoCall(seat);
                }
                else if (nerve > 92 && raises == 0 && CanCover(seat, owed))
                    DoCall(seat);   // a bluff-call
                else
                    Fold(seat);
            }
            AdvanceTurn();
        }

        void Fold(int seat)
        {
            DoFold(seat);
            if (brainAt != null && brainAt[seat] != null)
                brainAt[seat].Speak(0, 2f);
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

        /// Which of a seat's five cards the AI keeps: made hands stand,
        /// then a four-flush or an open-ended four-straight, then the high
        /// cards.
        public int AiHoldMask(int seat)
        {
            int a = seatCards[seat * 5];
            int b = seatCards[seat * 5 + 1];
            int c = seatCards[seat * 5 + 2];
            int d = seatCards[seat * 5 + 3];
            int e = seatCards[seat * 5 + 4];
            if (a < 0 || b < 0 || c < 0 || d < 0 || e < 0)
                return 31;
            int score = Eval5(a, b, c, d, e);
            int cat = score / 759375;
            if (cat >= CAT_STRAIGHT)
                return 31;               // stand pat

            int[] v = new int[5];
            int[] s = new int[5];
            v[0] = RankValue(a); v[1] = RankValue(b); v[2] = RankValue(c);
            v[3] = RankValue(d); v[4] = RankValue(e);
            s[0] = a / 13; s[1] = b / 13; s[2] = c / 13; s[3] = d / 13; s[4] = e / 13;

            if (cat == CAT_TRIPS || cat == CAT_TWOPAIR || cat == CAT_PAIR)
            {
                int mask = 0;
                for (int i = 0; i < 5; i++)
                {
                    int n = 0;
                    for (int k = 0; k < 5; k++)
                        if (v[k] == v[i])
                            n++;
                    if (n >= 2)
                        mask = mask | (1 << i);
                }
                return mask;
            }

            // Four of one suit: throw the odd card away.
            for (int suit = 0; suit < 4; suit++)
            {
                int n = 0;
                int mask = 0;
                for (int i = 0; i < 5; i++)
                    if (s[i] == suit)
                    {
                        n++;
                        mask = mask | (1 << i);
                    }
                if (n == 4)
                    return mask;
            }

            // Four to an open-ended straight (four consecutive ranks, both
            // ends live - so not A-2-3-4 and not J-Q-K-A).
            for (int lo = 3; lo <= 10; lo++)
            {
                int mask = 0;
                int n = 0;
                for (int r = lo; r < lo + 4; r++)
                    for (int i = 0; i < 5; i++)
                        if (v[i] == r && (mask & (1 << i)) == 0)
                        {
                            mask = mask | (1 << i);
                            n++;
                            break;
                        }
                if (n == 4)
                    return mask;
            }

            // Nothing: keep the picture cards, or the single highest.
            int hi = 0;
            int held = 0;
            int highest = -1;
            int highestAt = 0;
            for (int i = 0; i < 5; i++)
            {
                if (v[i] > highest)
                {
                    highest = v[i];
                    highestAt = i;
                }
                if (v[i] >= 13 && held < 2)
                {
                    hi = hi | (1 << i);
                    held++;
                }
            }
            return hi != 0 ? hi : (1 << highestAt);
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

        int ScoreSeat(int seat)
        {
            return Eval5(seatCards[seat * 5], seatCards[seat * 5 + 1],
                seatCards[seat * 5 + 2], seatCards[seat * 5 + 3],
                seatCards[seat * 5 + 4]);
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
            Vector3 p = a.position + a.right * ((slot - 2) * 0.075f);
            Quaternion r = a.rotation * Quaternion.Euler(0f, (slot - 2) * 5f, 0f);
            card.Deal(p, r, faceUp);
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

        void ReturnSeatCards(int seat)
        {
            for (int c = 0; c < 5; c++)
            {
                ReturnCard(seatCards[seat * 5 + c]);
                seatCards[seat * 5 + c] = -1;
            }
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
        }

        // --- the seat panel --------------------------------------------------------

        /// The panel's buttons all land here (SendCustomEvent from the UI
        /// listener), resolve the presser's own seat, and go to the owner.
        public void UiDeal() { SendAct(A_DEAL); }
        public void UiMode() { SendAct(A_MODE); }
        public void UiCall() { SendAct(A_CALL); }
        public void UiRaise() { SendAct(A_RAISE); }
        public void UiFold() { SendAct(A_FOLD); }
        public void UiDraw() { SendAct(A_DRAW); }
        public void UiHit() { SendAct(A_HIT); }
        public void UiStand() { SendAct(A_STAND); }
        public void UiHold0() { SendAct(A_HOLD0); }
        public void UiHold1() { SendAct(A_HOLD0 + 1); }
        public void UiHold2() { SendAct(A_HOLD0 + 2); }
        public void UiHold3() { SendAct(A_HOLD0 + 3); }
        public void UiHold4() { SendAct(A_HOLD0 + 4); }

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

        void RefreshPanel()
        {
            if (seatKind == null)
                return;
            if (modeText != null)
                modeText.text = mode == MODE_POKER
                    ? "Five-card draw  -  ante " + ante
                    : "Blackjack  -  stake " + ante;
            if (potText != null)
                potText.text = mode == MODE_BJ
                    ? "Dealer " + (dealerUpCount >= 2 ? "" + DealerTotal()
                        : "showing " + BjCardValue(dealerCards[0]))
                    : "Pot " + pot;
            if (msgText != null)
                msgText.text = message;

            for (int i = 0; i < seatCount; i++)
                RefreshRow(i);
            RefreshButtons();
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
            if (rowName != null && i < rowName.Length && rowName[i] != null)
                rowName[i].text = name;
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
            bool betting = myTurn && mode == MODE_POKER &&
                           (phase == PH_BET1 || phase == PH_BET2);
            bool drawing = myTurn && mode == MODE_POKER && phase == PH_DRAW;
            bool bj = myTurn && mode == MODE_BJ && phase == PH_BET1;

            Enable(btnDeal, seated && phase == PH_IDLE);
            Enable(btnMode, seated && phase == PH_IDLE);
            Enable(btnCall, betting);
            Enable(btnRaise, betting && raises < maxRaises);
            Enable(btnFold, betting);
            Enable(btnDraw, drawing);
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

            if (btnHold == null)
                return;
            for (int c = 0; c < btnHold.Length && c < 5; c++)
            {
                Enable(btnHold[c], drawing);
                if (btnHoldText == null || c >= btnHoldText.Length ||
                    btnHoldText[c] == null)
                    continue;
                int card = seated ? seatCards[ls * 5 + c] : -1;
                bool held = seated && (seatHold[ls] & (1 << c)) != 0;
                btnHoldText[c].text = card < 0 ? "-"
                    : CardLabel(card) + (held ? "\n[hold]" : "\n");
                btnHoldText[c].color = card >= 0 && IsRedSuit(card)
                    ? new Color(0.95f, 0.5f, 0.45f)
                    : new Color(0.95f, 0.92f, 0.85f);
            }
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
