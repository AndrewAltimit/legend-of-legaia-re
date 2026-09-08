// One villager's head: the per-NPC state machine of the living-town layer.
// LegaiaTownDirector schedules (who talks to whom, which station is free,
// when the town goes indoors); this behaviour EXECUTES, driving the
// locomotion controller (LegaiaNpcWander's command API) and the speech
// bubble, and it is the only thing that touches the station contract's
// Claim / Arrive / Release for its own NPC.
//
// States:
//   0 STROLL     - the locomotion controller's own autonomous amble, around
//                  the village spawn outdoors or around the landing indoors
//   1 GO_STATION - walking to a claimed station's stand point
//   2 AT_STATION - standing at it, facing its +Z, until the dwell runs out
//                  (the station's handler is what makes the cupboard open)
//   3 GO_CHAT    - walking to a claimed chat-ring slot
//   4 CHAT       - standing in the ring facing its centre, taking turns
//   5 GO_DOOR    - walking (by the navmesh route) to the stand spot in
//                  front of the assigned home's door
//   7 DOOR_OPEN  - standing there while the door prop swings open
//   8 GO_THRESH  - the last step onto the doorway tile itself, then the
//                  teleport to the interior landing - the same doorway
//                  pair a player walks through, and the door closes
//                  behind unless a player opened it
//   6 GO_EXIT    - the reverse, at dawn: walk to the interior-side
//                  doorway, teleport out, the door swings behind
//   9 NIGHT_IDLE - the door trip failed `homeRetryLimit` times: step BACK
//                  from the doorway and wait out the night there, facing
//                  home, instead of retrying for ever (which reads as
//                  aimless wandering around the house). The step back is
//                  the keep-out rule: the doorway tile is a teleport a
//                  player walks through, and nobody may spend a night in it
//  30 NIGHT_HOST_WAIT - the villager pinned to a station for the night
//                  (town01: Cara at card-table stool 0) is waiting a
//                  couple of metres off it because a PLAYER has the seat.
//                  The director sends it over the moment the seat frees
//
//  40 DEAD       - struck down by a player's weapon (LegaiaNpcHitbox ->
//                   Slay, broadcast as SlainAt to every client): the
//                   villager topples where it stood, fades out, and walks
//                   back in at its own spawn `respawnSeconds` later. Coins
//                   are dropped through the loose `bounty` link (a
//                   LegaiaCoinDrops pool) on every client at the same spot.
//
// STATE NUMBERING is a shared space: 0-9 and 30+ belong to this file,
// 10-29 to the daytime social layer that adds its own `else if` arms to
// BrainTick. Keep any new state inside those bands.
//
// OTHER PASSES HOLD A VILLAGER IN PLACE through `HoldStation(seconds)`:
// the card table calls it for every seated villager while a hand is being
// played, so the dwell timer never walks a player's opponent away
// mid-hand. `Seated()` / `AtStation()` / `Dead()` are the matching
// queries; `label` and `portrait` are what a panel shows for this
// villager (the manifest label and a build-time head render).
//
// THE NIGHT HOST. One villager per scene may be pinned to a station for
// the whole night (`nightHostStationPath`, resolved by NAME at Start
// because the station belongs to another pass's object). It is an
// ordinary villager by day; at nightfall the director claims the station
// and sends it there instead of home, renews the hold every tick, and
// releases it at dawn. `Available()` is false for the whole shift, which
// is what keeps every other picker - Summon, errands, chat rings, ad-hoc
// meetings, passing greetings - away from it in one test. The seat is an
// ordinary kind-2 station, so the card table's own handler sits her down
// through the existing OnNpcArrive event: this file knows nothing about
// card tables.
//
// KEEP-OUT ZONES belong to the locomotion controller (LegaiaNpcWander's
// `keepOut`), but the cap on them is set from here: it is pulled in while
// the villager is indoors, because an interior room is barely wider than
// the village-sized zone around its own way out.
//
// SOCIAL DETAIL. Three things make a conversation read as people rather
// than as bubbles on a timer, and all three are cosmetic - no state, no
// sync, no Animator: the group turns to face the CURRENT speaker after a
// beat (`LookAt`), a listener answers the topic with a reaction a
// half-second later (`ReactTo` / `SpeakAfter`, table in `ReplyIcon`), and
// the speaker nods (`LegaiaNpcWander.Nod`, a LateUpdate pose blend). Two
// companions walking an errand together swap the same pair of bubbles
// every few seconds, and a villager the LOCAL player walks up to glances
// at them - and, once in a while, waves.
//
// THE DOOR TRIP IS REUSABLE. `DoorTrip(door, threshold, prop, landing)`
// runs stand spot -> swing -> tile -> teleport for ANY doorway pair, and
// `LeaveThrough(exit, prop, emerge)` runs it backwards; `GoHome` /
// `ComeOut` are those two aimed at this villager's own home. A daytime
// errand that calls at a house uses the same pair.
//
// The DAYTIME states, 10 and up, are what makes a villager look like it
// has somewhere to be rather than like it is circling its spawn:
//
//  10 ERRAND_GO   - walking to the next stop of an ITINERARY: two to four
//                   stations claimed one at a time (a doorway to knock at,
//                   the shore, a neighbour to visit, a bundle of wood to
//                   move), not one station handed out on its own
//  11 ERRAND_STOP - standing at that stop while its dwell runs, glancing
//                   around, holding whatever the stop's handler put in
//                   its hands; then straight on to the next stop
//  12 GREET       - two villagers who passed within arm's reach stop,
//                   turn to each other, one waves, and both carry on
//                   exactly where they left off (the interrupted walk is
//                   resumed, not restarted)
//  13 FOLLOW      - walking BESIDE another villager on its errand: the
//                   leader owns the itinerary, the follower re-aims at a
//                   point offset to the leader's side twice a second
//  20 GO_MEET     - walking to an ad-hoc meeting spot: a conversation
//                   that started because people met on a path, with no
//                   chat ring involved and a centre invented on the spot
//  21 MEET        - standing in one. The director runs the turn-taking
//                   for ring conversations and ad-hoc ones alike (see
//                   InTalk / AtTalk / EndTalk).
//
// Ticks at 10 Hz through SendCustomEventDelayedSeconds (staggered per NPC
// by the personality seed), so ~30 villagers cost 300 decisions a second
// between them; per-frame work stays in the locomotion controller and only
// while it is actually stepping.
//
// The personality `seed` is a build-time constant per NPC (the editor pass
// derives it from the NPC's file stem), so a villager's dwell lengths,
// chattiness and home are the same on every client even though the motion
// itself is simulated locally, like LegaiaNpcWander's.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;
using VRC.SDK3.UdonNetworkCalling;
using VRC.Udon.Common.Interfaces;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcBrain : UdonSharpBehaviour
    {
        [Tooltip("The town director that schedules this villager.")]
        public LegaiaTownDirector director;

        [Tooltip("Display name for panels (the manifest's label; builder-set).")]
        public string label = "";

        [Tooltip("Head-and-shoulders portrait for the card table's seat panel (LegaiaPortraits renders it at build time; null = no picture).")]
        public Texture2D portrait;

        [Tooltip("Seconds a slain villager stays down before it walks back in at its spawn.")]
        public float respawnSeconds = 120f;

        [Tooltip("Seconds the topple takes; the body fades out a moment after.")]
        public float fallSeconds = 0.7f;

        [Tooltip("The coin-drop pool (LegaiaCoinDrops; loose link, builder-wired): receives dropX/dropY/dropZ/dropCoins then SpawnDrop.")]
        public UdonSharpBehaviour bounty;

        [Tooltip("Times this villager has been slain (statistics for the soak / checks).")]
        [HideInInspector] public int slainCount;

        [Tooltip("This NPC's locomotion controller (same GameObject).")]
        public LegaiaNpcWander loco;

        [Tooltip("This NPC's speech bubble (a child of the NPC).")]
        public LegaiaSpeechBubble bubble;

        [Tooltip("Personality seed (build-time constant, derived from the NPC file name).")]
        public int seed = 1;

        [Tooltip("This villager stays indoors during the day too (a shopkeeper, someone's grandmother).")]
        public bool daytimeIndoors;

        [Tooltip("Retail placed this villager inside a house: it starts indoors and has no front door to walk to.")]
        public bool startIndoors;

        [Tooltip("Build-time finding: no walkable route from this villager's spawn to any front door, so it has no home (informational).")]
        public bool noRoute;

        [Tooltip("Village-side stand spot in front of this NPC's assigned home door.")]
        public Transform homeDoor;

        [Tooltip("The doorway tile itself (the teleport trigger) - the last step before going in.")]
        public Transform homeThreshold;

        [Tooltip("The home's door prop (LegaiaDoor), swung open on the way in and out. Null = no visible door.")]
        public LegaiaDoor homeDoorProp;

        [Tooltip("How long the NPC waits at the door for it to swing open (seconds).")]
        public float doorSwingSeconds = 0.9f;

        [Tooltip("Interior landing the home door drops you at.")]
        public Transform homeLanding;

        [Tooltip("Interior stand spot at the way out (the interior-side doorway trigger).")]
        public Transform homeExit;

        [Tooltip("Village-side landing when leaving the house at dawn.")]
        public Transform homeEmerge;

        [Tooltip("Stroll radius indoors (rooms are small).")]
        public float indoorRadius = 0.8f;

        [Tooltip("Stroll radius outdoors - the locomotion controller's own radius is restored from here.")]
        public float outdoorRadius = 1.25f;

        [Tooltip("Give up on a walk that takes longer than this (seconds).")]
        public float walkTimeout = 45f;

        [Tooltip("After giving up on the way to a station, this villager is " +
                 "not sent to that SAME station again for this long (seconds) " +
                 "- the director tries the next villager instead. Without it " +
                 "the nearest villager is re-summoned onto the same failing " +
                 "approach for ever: the walk loop.")]
        public float failRetrySeconds = 120f;

        /// The last station walk that failed and why ("blocked -> stool_2:
        /// no lane past ...", "timed out -> ..."). Diagnostics.
        [HideInInspector] public string lastFailure = "";

        [Tooltip("How close to the door stand spot counts as arrived (meters). " +
                 "Wider than a plain walk's: the spot sits a step out from a " +
                 "doorway, between the hut wall and the door leaf, and the " +
                 "steering cannot thread a 0.3 m circle in there.")]
        public float doorArriveRadius = 0.6f;

        [Tooltip("Seconds the scripted step from the stand spot onto the doorway " +
                 "tile takes (a straight slide, not a steered walk - the tile sits " +
                 "inside the frame).")]
        public float thresholdStepSeconds = 0.9f;

        [Tooltip("How many times a failed door trip is retried before the villager " +
                 "gives up for the night and waits it out near its door (state 9).")]
        public int homeRetryLimit = 3;

        [Tooltip("How far BACK from the doorway a villager who gave up on the " +
                 "door trip waits out the night (metres). It must be clear of " +
                 "the doorway's keep-out zone: the tile is a teleport a player " +
                 "walks through.")]
        public float nightIdleBackOff = 3f;

        // Published for the play-mode soak harness (Editor/LegaiaSoak.cs):
        // decisions taken, so a soak can check that Udon's delayed-event
        // scheduler really does follow Time.timeScale before trusting a
        // time-compressed run; and door-trip retries, which is what
        // "wandering aimlessly near the house" measures as.
        [HideInInspector] public int tickCount;
        [HideInInspector] public int homeRetries;

        // --- Daytime errands ------------------------------------------------

        [Tooltip("This villager's carried-item rig (same GameObject) - the " +
                 "hands are emptied here when an errand ends.")]
        public LegaiaNpcCarry carry;

        [Tooltip("Stops an itinerary may hold. Raising it costs one array slot.")]
        public int maxStops = 4;

        [Tooltip("Shortest / longest rest between two itineraries (seconds).")]
        public float restMin = 5f;
        public float restMax = 18f;

        [Tooltip("How long a passing greeting holds both villagers (seconds).")]
        public float greetSeconds = 2.6f;

        [Tooltip("This villager will not greet the same neighbour again for this long (seconds).")]
        public float greetCooldown = 30f;

        [Tooltip("How far to the leader's side a companion walks (meters).")]
        public float followSpacing = 0.85f;

        [Tooltip("Seconds between a companion's re-aims at the leader's side.")]
        public float followInterval = 0.55f;

        [Tooltip("Seconds between two companions swapping a bubble as they walk.")]
        public float followTalkSeconds = 8f;

        // --- Noticing the local player -----------------------------------------

        [Tooltip("How near the LOCAL player a strolling villager notices them (metres).")]
        public float noticeDistance = 2f;

        [Tooltip("How long the villager holds the glance at the player (seconds).")]
        public float noticeSeconds = 2.4f;

        [Tooltip("At most one wave at the player per villager per this long (seconds). " +
                 "The glance itself is more frequent - a wave every time would " +
                 "read as a shop greeter, not a village.")]
        public float wavePlayerCooldown = 40f;

        // --- The night host (Cara at the card table) ----------------------------

        [Tooltip("Scene path of a station this villager HOSTS at night " +
                 "(`Legaia_common_prefabs/card_table/stool_0`). Resolved by " +
                 "name in Start, because the station belongs to another " +
                 "pass's object. Empty = an ordinary villager, which is all " +
                 "but one of them.")]
        public string nightHostStationPath = "";

        [Tooltip("How far from its night station the host waits when a PLAYER " +
                 "has the seat (metres).")]
        public float nightHostWaitRadius = 2.2f;

        [Tooltip("Keep-out radius cap while INDOORS: an interior room is " +
                 "barely wider than the village-sized zone around its own " +
                 "way out, so the full radius would leave the room with " +
                 "nowhere to stand. See LegaiaNpcWander.keepOutCap.")]
        public float indoorKeepOut = 0.75f;

        /// The night host is Seated() at its own station right now - read by
        /// the play-mode soak, which asserts it held the seat all night.
        [HideInInspector] public bool nightHostSeated;
        /// Times the night host could not take its seat (claim refused, or
        /// the walk gave up) - diagnostics, same shape as homeRetries.
        [HideInInspector] public int nightHostRetries;

        private int state;
        private bool indoors;
        private float leaveAt;
        // Death: where to come back, how the topple plays, when to return.
        private Vector3 spawnPos;
        private Vector3 spawnFacing;
        private Quaternion fallFrom;
        private Vector3 fallAxis;
        private float fallStart;
        private float deadUntil;
        private bool faded;
        private Renderer[] bodyRenderers;
        private float giveUpAt;
        private float busyUntil;
        private LegaiaNpcStation failedStation;
        private float failedAt = -1e9f;
        private Vector3 outdoorHome;
        private Vector3 chatCentre;
        private LegaiaNpcStation station;
        private int rng;
        private bool started;

        // The doorway pair the trip in flight is using. GoHome / ComeOut
        // point these at this villager's own home; DoorTrip / LeaveThrough
        // point them anywhere, which is what makes the sequence reusable.
        private Transform tripDoor;
        private Transform tripThreshold;
        private Transform tripLanding;
        private LegaiaDoor tripProp;
        private Transform tripEmerge;
        private bool tripIsHome;

        // --- Errand / interaction state --------------------------------------
        // The itinerary is a fixed array filled through PlanClear / PlanAdd
        // rather than an array handed over by the director: Udon has no
        // generic collections and passing an array across behaviours would
        // allocate one per errand, on every client, forever.
        private LegaiaNpcStation[] plan;
        private int planCount;
        private int planCursor;
        private float glanceAt;
        private Vector3 glanceAim;
        private int greetResume;
        private float greetFreeAt;
        private LegaiaNpcBrain leader;
        private float followSide;
        private float followNext;
        private float followTalkAt;
        private Vector3 lastLeaderPos;

        // --- Social detail ------------------------------------------------------
        // A REPLY the director scheduled: a listener's reaction, a beat
        // after the speaker's bubble. Fired from BrainTick rather than from
        // a delayed event so the icon is dropped for free when the villager
        // walks off mid-conversation.
        private int pendingIcon = -1;
        private float pendingAt;
        private float pendingSeconds;
        // Whom to look at in a conversation, and when the head turns: the
        // CURRENT speaker, after a short beat, so the group reads as
        // attention rather than as a rack of heads snapping round.
        private bool attending;
        private Vector3 attendAim;
        private float attendAt;
        // Noticing the local player (cosmetic, local-only, never synced).
        private float noticeUntil;
        private float waveFreeAt;

        // --- Night host ----------------------------------------------------------
        private LegaiaNpcStation nightStation;
        private Vector3 hostWaitLook;
        // Night idle (state 9): where it waits and whether it got there.
        private Vector3 nightIdleLook;
        private bool nightIdleParked;

        void Start()
        {
            rng = seed == 0 ? 12345 : seed;
            plan = new LegaiaNpcStation[maxStops < 2 ? 2 : maxStops];
            if (loco == null)
                loco = GetComponent<LegaiaNpcWander>();
            if (carry == null)
                carry = GetComponent<LegaiaNpcCarry>();
            outdoorHome = transform.position;
            spawnPos = transform.position;
            spawnFacing = transform.forward;
            bodyRenderers = GetComponentsInChildren<Renderer>(true);
            indoors = startIndoors;
            if (loco != null)
            {
                outdoorRadius = loco.radius;
                // Already in a room: stroll the room's radius from the start.
                if (startIndoors)
                    loco.radius = indoorRadius;
                loco.keepOutCap = startIndoors ? indoorKeepOut : 1e9f;
            }
            // The night station belongs to ANOTHER pass's object (the card
            // table lives in the kit's top-level prefab container), so the
            // link is loose - by name, at Start, exactly like the
            // director's back-reference into the card game. Nothing
            // happens when the table is not built.
            if (nightHostStationPath != null && nightHostStationPath.Length > 0)
            {
                GameObject g = GameObject.Find(nightHostStationPath);
                if (g != null)
                    nightStation = g.GetComponent<LegaiaNpcStation>();
                if (nightStation == null)
                    Debug.Log("[Legaia] " + gameObject.name + ": no station at '" +
                        nightHostStationPath + "' - it hosts nothing tonight.");
            }
            started = true;
            // Stagger the first tick across the town so 30 brains never land
            // their decisions on the same frame.
            SendCustomEventDelayedSeconds("BrainTick",
                0.2f + (NextInt(100) / 100f) * 2f);
        }

        // --- Personality RNG (deterministic per NPC) ------------------------
        // A plain LCG on int: Udon has no unsigned types and no `unchecked`,
        // and its int arithmetic wraps silently, so the mask is what keeps
        // the value positive rather than an overflow check.
        int NextInt(int n)
        {
            rng = rng * 1103515245 + 12345;
            int v = rng & 0x7FFFFFFF;
            return n <= 0 ? 0 : v % n;
        }

        float NextFloat()
        {
            return NextInt(10000) / 10000f;
        }

        // --- Director queries ----------------------------------------------

        /// Free to be given something to do (strolling, nothing claimed).
        /// A villager ON NIGHT DUTY is never free: the card table's host is
        /// not summoned to a stool by the game, matchmade onto a chat ring,
        /// pulled into an ad-hoc meeting or given an errand while she is
        /// keeping the table. One test covers every picker in the director,
        /// because they all ask this.
        public bool Available()
        {
            return started && state == 0 && station == null
                   && Time.time >= busyUntil && !NightHostOnDuty();
        }

        // --- The night host --------------------------------------------------
        // One villager per scene may be pinned to a station for the whole
        // night (town01: Cara at card-table stool 0). It is an ordinary
        // villager by day; at nightfall the director claims its station and
        // sends it, holds it there, and lets it go at dawn. The seat is a
        // kind-2 station, so the card table's own handler sits her down
        // through the existing OnNpcArrive event - this file knows nothing
        // about card tables.

        /// This villager hosts a station at night (the link resolved).
        public bool IsNightHost()
        {
            return nightStation != null;
        }

        /// The station it hosts (null when it is an ordinary villager).
        public LegaiaNpcStation NightStation()
        {
            return nightStation;
        }

        /// On duty right now: a night host, and the town is sheltering.
        public bool NightHostOnDuty()
        {
            return nightStation != null && director != null
                   && director.Sheltering() && state != 40;
        }

        /// Sitting at its own night station.
        public bool NightHostSeated()
        {
            return station != null && station == nightStation && state == 2;
        }

        /// Walking to its night station right now.
        public bool NightHostWalking()
        {
            return state == 1 && station != null && station == nightStation;
        }

        /// Waiting beside it for a player to get up.
        public bool NightHostWaiting()
        {
            return state == 30;
        }

        /// Anywhere in the shift: walking to the seat, sitting on it, or
        /// waiting beside it.
        public bool NightHostBusy()
        {
            return NightHostWaiting() || NightHostWalking() || NightHostSeated();
        }

        /// Wait a couple of metres off the station, facing it, until it is
        /// free again (a PLAYER is sitting on the stool). State 30 - the
        /// host's own; it is not a chat, an errand or a station visit, so
        /// none of those pickers can see it.
        public void NightHostWait(Vector3 spot, Vector3 lookAt)
        {
            if (loco == null || nightStation == null)
                return;
            if (state == 30)
                return;
            ReleaseStation();
            DropDaytime();
            hostWaitLook = lookAt;
            state = 30;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(spot);
        }

        void TickNightHostWait()
        {
            // Arrived, blocked or out of time: stand and watch the table.
            // There is nowhere else to be - the director re-checks the
            // stool every tick and sends her over the moment it frees.
            if (loco.Arrived() || loco.Blocked() || Time.time > giveUpAt)
                loco.FaceToward(hostWaitLook);
        }

        /// Dawn: off the stool, back to the village.
        public void EndNightHost()
        {
            if (!IsNightHost())
                return;
            if (state != 30 && !(station != null && station == nightStation))
                return;
            ReleaseStation();
            BackToStroll(1f + NextFloat() * 3f);
        }

        /// A failed attempt at the seat (the director counts them so a
        /// station nobody can reach reads as a number, not as a mystery).
        public void NoteNightHostFailure()
        {
            nightHostRetries++;
        }

        /// Inside a house (reached through a doorway teleport).
        public bool Indoors()
        {
            return indoors;
        }

        /// Has a home to go to at nightfall.
        public bool HasHome()
        {
            return homeDoor != null && homeLanding != null;
        }

        /// In a conversation ring and standing in place (the director waits
        /// for every member before the turn-taking starts).
        public bool AtChat()
        {
            return state == 4;
        }

        /// Still on its way to (or standing in) a conversation.
        public bool InChat()
        {
            return state == 3 || state == 4;
        }

        /// In a conversation of EITHER shape - a claimed chat ring, or an
        /// ad-hoc meeting struck up where two villagers ran into each other.
        /// The director's conversation runner keys on this, so both shapes
        /// get the same turn-taking; InChat / AtChat keep their original
        /// meaning (a ring, with a claimed station) for the night routine.
        public bool InTalk()
        {
            return InChat() || state == 20 || state == 21;
        }

        /// Standing still in one, whichever shape.
        public bool AtTalk()
        {
            return AtChat() || state == 21;
        }

        /// Walking an itinerary (or standing at one of its stops).
        public bool OnErrand()
        {
            return state == 10 || state == 11;
        }

        /// Walking beside somebody else's errand.
        public bool Following()
        {
            return state == 13;
        }

        /// Actually stepping (the locomotion controller's own report).
        public bool Walking()
        {
            return loco != null && loco.Walking();
        }

        /// Standing at a claimed station (a single stop or an itinerary stop).
        public bool AtStation()
        {
            return state == 2 || state == 11;
        }

        /// Sitting on a card-table stool (a kind-2 station it has arrived at).
        public bool Seated()
        {
            return AtStation() && station != null && station.kind == 2;
        }

        /// The station this villager holds right now (null when none).
        public LegaiaNpcStation CurrentStation()
        {
            return station;
        }

        /// Struck down and not yet back.
        public bool Dead()
        {
            return state == 40;
        }

        /// Keep this villager where it stands for at least `seconds` more:
        /// the dwell timer of the station it is at is pushed out (never
        /// pulled in). No effect unless it is standing at a station.
        public void HoldStation(float seconds)
        {
            if (!AtStation())
                return;
            float until = Time.time + seconds;
            if (until > leaveAt)
                leaveAt = until;
        }

        // --- Bounty -----------------------------------------------------------

        /// A player's weapon connected (LegaiaNpcHitbox, on the STRIKING
        /// client only). Every client is told, with the spot the coins
        /// land on, so the drop is in one place for everyone even though
        /// each client walks its own copy of the villager.
        public void Slay(int coins)
        {
            if (state == 40)
                return;
            Vector3 p = transform.position;
            SendCustomNetworkEvent(NetworkEventTarget.All, nameof(SlainAt),
                p.x, p.y, p.z, coins);
        }

        [NetworkCallable]
        public void SlainAt(float x, float y, float z, int coins)
        {
            if (state == 40 || loco == null)
                return;
            ReleaseStation();
            DropDaytime();
            if (bubble != null)
                bubble.Hide();
            loco.Stop();
            // The controller's facing servo and floor snap would fight the
            // topple: it is switched off for the duration and given the
            // body back at the respawn teleport.
            loco.enabled = false;
            state = 40;
            slainCount++;
            fallStart = Time.time;
            fallFrom = transform.rotation;
            Vector3 f = loco.Facing();
            f.y = 0f;
            if (f.sqrMagnitude < 1e-6f)
                f = transform.forward;
            fallAxis = Vector3.Cross(Vector3.up, f.normalized);
            if (fallAxis.sqrMagnitude < 1e-6f)
                fallAxis = Vector3.right;
            fallAxis.Normalize();
            faded = false;
            deadUntil = Time.time + (respawnSeconds < 5f ? 5f : respawnSeconds);
            if (bounty != null && coins > 0)
            {
                bounty.SetProgramVariable("dropX", x);
                bounty.SetProgramVariable("dropY", y);
                bounty.SetProgramVariable("dropZ", z);
                bounty.SetProgramVariable("dropCoins", coins);
                bounty.SendCustomEvent("SpawnDrop");
            }
        }

        void TickDead()
        {
            float t = Time.time - fallStart;
            float dur = fallSeconds < 0.1f ? 0.1f : fallSeconds;
            if (t < dur)
            {
                // Tip over backwards about the feet (the root sits at the
                // floor), easing out like something heavy going down.
                float k = t / dur;
                k = 1f - (1f - k) * (1f - k);
                transform.rotation =
                    Quaternion.AngleAxis(-88f * k, fallAxis) * fallFrom;
                return;
            }
            if (!faded && t > dur + 2.5f)
            {
                faded = true;
                ShowBody(false);
            }
            if (Time.time >= deadUntil)
                Respawn();
        }

        void ShowBody(bool on)
        {
            if (bodyRenderers == null)
                return;
            for (int i = 0; i < bodyRenderers.Length; i++)
                if (bodyRenderers[i] != null)
                    bodyRenderers[i].enabled = on;
            if (!on && bubble != null)
                bubble.Hide();
        }

        void Respawn()
        {
            transform.rotation = fallFrom;
            ShowBody(true);
            indoors = startIndoors;
            homeRetries = 0;
            if (loco != null)
            {
                loco.enabled = true;
                loco.radius = startIndoors ? indoorRadius : outdoorRadius;
                loco.keepOutCap = startIndoors ? indoorKeepOut : 1e9f;
                loco.Teleport(spawnPos, spawnFacing);
                loco.SetHome(spawnPos);
            }
            outdoorHome = spawnPos;
            BackToStroll(1.5f);
        }

        /// Out on a walk and free to be stopped for a passing greeting: on
        /// an errand leg, and not fresh from greeting somebody else (the
        /// director rate-limits per PAIR on top of this). A COMPANION is
        /// deliberately not greetable - it walks at its leader's shoulder,
        /// so the two of them would greet each other on a timer forever.
        public bool Greetable()
        {
            return started && state == 10 && Time.time >= greetFreeAt
                   && !NightHostOnDuty();
        }

        // --- Director commands ----------------------------------------------

        /// Walk to a station the director has already claimed for this NPC.
        public void SendToStation(LegaiaNpcStation s)
        {
            if (s == null || loco == null)
                return;
            station = s;
            state = 1;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(s.StandPosition());
        }

        /// Walk to a claimed chat-ring slot and stand facing `centre`.
        public void JoinChat(LegaiaNpcStation slot, Vector3 centre)
        {
            if (slot == null || loco == null)
                return;
            station = slot;
            chatCentre = centre;
            attending = false;
            state = 3;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(slot.StandPosition());
        }

        // LegaiaBubbleArt.ICON_NAMES indices (Udon cannot reach the editor class).
        private const int ICON_DOTS = 0;
        private const int ICON_EXCLAIM = 1;
        private const int ICON_QUERY = 2;
        private const int ICON_HEART = 3;
        private const int ICON_MUSIC = 4;
        private const int ICON_LAUGH = 5;
        private const int ICON_WAVE = 6;
        private const int ICON_TOPIC_FIRST = 8;
        private const int ICON_TOPIC_COUNT = 6;
        private const int ICON_HOUSE = 9;
        private const int ICON_SLEEP = 11;

        /// Take a turn in the conversation: a pictogram over the head, and
        /// - if the villager is standing still - the small nod that makes a
        /// turn read as somebody speaking rather than as a sign appearing.
        /// The gesture is a LateUpdate pose blend in the locomotion
        /// controller, not an Animator state.
        public void Speak(int icon, float seconds)
        {
            if (bubble != null)
                bubble.Show(icon, seconds);
            if (loco == null || loco.Walking())
                return;
            float nod = seconds;
            if (nod < 1.2f)
                nod = 1.2f;
            else if (nod > 2.2f)
                nod = 2.2f;
            loco.Nod(nod);
        }

        /// A bubble a beat from now (a listener's reaction to what was just
        /// said). Fired from BrainTick rather than from a delayed event, so
        /// a villager who walks off mid-conversation simply never says it.
        public void SpeakAfter(int icon, float seconds, float delay)
        {
            if (icon < 0)
                return;
            pendingIcon = icon;
            pendingSeconds = seconds;
            pendingAt = Time.time + delay;
        }

        /// React to what somebody else just said. The reaction TABLE lives
        /// here rather than in the director so the ring conversation, the
        /// ad-hoc meeting and two companions walking together all answer
        /// out of the same vocabulary.
        public void ReactTo(int topic, float delay, float seconds)
        {
            SpeakAfter(ReplyIcon(topic), seconds, delay);
        }

        // Which reaction a topic draws. Two plausible answers each, so the
        // same topic twice running does not draw the same face: a laugh or
        // an "!" at the fish, a "..." or a "?" at somebody's house, music
        // or a heart at the sun, a heart at food, and the storm gets the
        // long silence it deserves.
        int ReplyIcon(int topic)
        {
            bool flip = NextInt(2) == 0;
            if (topic == ICON_TOPIC_FIRST)                 // fish
                return flip ? ICON_LAUGH : ICON_EXCLAIM;
            if (topic == ICON_HOUSE)
                return flip ? ICON_DOTS : ICON_QUERY;
            if (topic == ICON_TOPIC_FIRST + 2)             // sun
                return flip ? ICON_MUSIC : ICON_HEART;
            if (topic == ICON_SLEEP)
                return flip ? ICON_DOTS : ICON_LAUGH;
            if (topic == ICON_TOPIC_FIRST + 4)             // food
                return flip ? ICON_HEART : ICON_EXCLAIM;
            if (topic == ICON_TOPIC_FIRST + 5)             // storm
                return flip ? ICON_DOTS : ICON_QUERY;
            int r = NextInt(3);
            return r == 0 ? ICON_DOTS : (r == 1 ? ICON_EXCLAIM : ICON_QUERY);
        }

        /// Turn to whoever is talking, after `delay` seconds. Held until
        /// the next call or the end of the conversation; a member with no
        /// aim keeps facing the group's centre, which is what it did
        /// before there was a speaker to look at.
        public void LookAt(Vector3 pos, float delay)
        {
            attending = true;
            attendAim = pos;
            attendAt = Time.time + delay;
        }

        // Where a member of a conversation points its face this frame.
        Vector3 AttentionAim(Vector3 fallback)
        {
            return attending && Time.time >= attendAt ? attendAim : fallback;
        }

        // --- Itineraries ------------------------------------------------------
        // The director fills a plan stop by stop and starts it. Only the
        // stop being walked to is CLAIMED: holding three stations for a
        // whole errand would starve a village of four villagers, and a stop
        // that turns out to be taken by the time we get to it is simply
        // skipped.

        /// Begin describing a new itinerary (drops any half-built one).
        public void PlanClear()
        {
            planCount = 0;
            planCursor = 0;
        }

        /// Append a stop. False when the itinerary is full.
        public bool PlanAdd(LegaiaNpcStation s)
        {
            if (s == null || plan == null || planCount >= plan.Length)
                return false;
            plan[planCount++] = s;
            return true;
        }

        /// Set off. False (and the villager keeps strolling) when not one
        /// stop could be claimed.
        public bool PlanStart()
        {
            if (loco == null || planCount <= 0)
                return false;
            ReleaseStation();
            planCursor = -1;
            return NextStop();
        }

        // Claim the next stop that is still free and walk to it; when the
        // itinerary runs out, rest.
        bool NextStop()
        {
            while (true)
            {
                planCursor++;
                if (plan == null || planCursor >= planCount)
                {
                    EndErrand();
                    return false;
                }
                LegaiaNpcStation s = plan[planCursor];
                if (s == null || !s.Claim(transform, this))
                    continue;
                station = s;
                state = 10;
                giveUpAt = Time.time + walkTimeout;
                loco.GoTo(s.StandPosition());
                return true;
            }
        }

        // Itinerary over: hands empty, a rest, then the director will find
        // this villager something else.
        void EndErrand()
        {
            ReleaseStation();
            planCount = 0;
            planCursor = 0;
            if (carry != null)
                carry.Hide();
            BackToStroll(restMin + NextFloat() * (restMax - restMin));
        }

        /// Cancel whatever daytime activity is running. The night routine
        /// owns states 5-8 and is never interrupted; the director uses this
        /// to settle a villager that has no home to walk to when the town
        /// goes indoors, so it stops running errands round an empty
        /// village instead.
        public void StopActivity()
        {
            if (state == 5 || state == 6 || state == 7 || state == 8)
                return; // the night routine owns those; never interrupt it
            leader = null;
            planCount = 0;
            ReleaseStation();
            if (carry != null)
                carry.Hide();
            BackToStroll(0.5f + NextFloat() * 2f);
        }

        // --- Passing greetings --------------------------------------------------

        /// Stop, turn to the villager at `otherPos`, optionally wave, then
        /// pick the interrupted walk back up exactly where it was.
        public void Greet(Vector3 otherPos, int icon)
        {
            if (loco == null || !Greetable())
                return;
            greetResume = state;
            state = 12;
            greetFreeAt = Time.time + greetCooldown;
            leaveAt = Time.time + greetSeconds;
            loco.FaceToward(otherPos);
            if (icon >= 0)
                Speak(icon, greetSeconds * 0.85f);
        }

        void TickGreet()
        {
            if (Time.time < leaveAt)
                return;
            if (bubble != null)
                bubble.Hide();
            // Resume rather than restart: the errand leg re-issues its
            // walk to the SAME stop it was already claiming.
            if (greetResume == 10 && station != null)
            {
                state = 10;
                giveUpAt = Time.time + walkTimeout;
                loco.GoTo(station.StandPosition());
                return;
            }
            BackToStroll(0.5f + NextFloat() * 2f);
        }

        // --- Walking together ------------------------------------------------------

        /// Walk beside `lead` for as long as its errand lasts. `side` is
        /// +1 or -1: which shoulder to keep to.
        public void FollowLeader(LegaiaNpcBrain lead, float side)
        {
            if (lead == null || loco == null)
                return;
            ReleaseStation();
            planCount = 0;
            leader = lead;
            followSide = side < 0f ? -1f : 1f;
            state = 13;
            lastLeaderPos = lead.transform.position;
            followNext = 0f;
            followTalkAt = Time.time + followTalkSeconds * 0.5f;
            giveUpAt = Time.time + walkTimeout * 3f;
        }

        void TickFollow()
        {
            if (leader == null || !leader.OnErrand() || Time.time > giveUpAt)
            {
                leader = null;
                BackToStroll(1f + NextFloat() * 3f);
                return;
            }
            if (Time.time < followNext)
                return;
            followNext = Time.time + (followInterval < 0.2f ? 0.2f : followInterval);

            // Walking together is talking together: every few seconds one of
            // them says something and the other answers a beat later, so a
            // pair crossing the village reads as two people rather than as
            // one villager with a shadow.
            if (Time.time >= followTalkAt)
            {
                float gap = followTalkSeconds < 2f ? 2f : followTalkSeconds;
                followTalkAt = Time.time + gap * (0.75f + NextFloat() * 0.5f);
                int topic = ICON_TOPIC_FIRST + NextInt(ICON_TOPIC_COUNT);
                Speak(topic, 2.2f);
                leader.ReactTo(topic, 0.4f + NextFloat() * 0.5f, 1.8f);
            }

            Vector3 lp = leader.transform.position;
            // The leader's heading, measured from where it WAS: there is no
            // facing getter on the locomotion controller, and reading its
            // transform.forward would be the mirror trap (a villager's
            // rendered front is not its transform's +Z).
            Vector3 dir = lp - lastLeaderPos;
            dir.y = 0f;
            if (dir.sqrMagnitude < 1e-4f)
            {
                dir = lp - transform.position;
                dir.y = 0f;
            }
            lastLeaderPos = lp;
            if (dir.sqrMagnitude < 1e-6f)
                dir = Vector3.forward;
            else
                dir = dir.normalized;

            Vector3 beside = lp
                + Vector3.Cross(Vector3.up, dir) * (followSide * followSpacing)
                - dir * 0.25f;
            if (Vector3.Distance(beside, transform.position) < 0.4f
                && !leader.Walking())
            {
                // Alongside and the leader has stopped: face the same way
                // instead of shuffling on the spot.
                loco.FaceToward(transform.position + dir);
                return;
            }
            loco.GoTo(beside);
        }

        // --- Ad-hoc conversations ------------------------------------------------

        /// Talk where you stand: no ring, no claimed station, a centre the
        /// director invents from where the group happens to be.
        public void JoinMeet(Vector3 stand, Vector3 centre)
        {
            if (loco == null)
                return;
            ReleaseStation();
            planCount = 0;
            leader = null;
            chatCentre = centre;
            attending = false;
            state = 20;
            giveUpAt = Time.time + 20f;
            loco.GoTo(stand);
        }

        void TickGoMeet()
        {
            // Blocked counts as arrived here: the whole point of an ad-hoc
            // meeting is that it happens where the people already are.
            if (loco.Arrived() || loco.Blocked() || Time.time > giveUpAt)
            {
                state = 21;
                loco.FaceToward(chatCentre);
            }
        }

        void TickMeet()
        {
            loco.FaceToward(AttentionAim(chatCentre));
        }

        /// End a conversation of either shape (the director ends every
        /// member of a group at once).
        public void EndTalk()
        {
            if (state == 3 || state == 4)
            {
                EndChat();
                return;
            }
            if (state != 20 && state != 21)
                return;
            if (bubble != null)
                bubble.Hide();
            BackToStroll(2f + NextFloat() * 6f);
        }

        /// The conversation is over (the director ends every member).
        public void EndChat()
        {
            if (state != 3 && state != 4)
                return;
            ReleaseStation();
            BackToStroll(2f + NextFloat() * 6f);
        }

        /// True while a door trip (either direction) is running - the
        /// daytime layer checks this before starting an errand.
        public bool OnDoorTrip()
        {
            return state == 5 || state == 6 || state == 7 || state == 8;
        }

        /// Walk in through ANY doorway pair: to `door` (the village-side
        /// stand spot), swing `prop`, step onto `threshold` (the teleport
        /// tile) and come out at `landing`, ending in state 0 indoors.
        /// `threshold` and `prop` may be null (a bare doorway). Returns
        /// false when the trip cannot be started.
        public bool DoorTrip(Transform door, Transform threshold,
            LegaiaDoor prop, Transform landing)
        {
            if (loco == null || door == null || landing == null || indoors)
                return false;
            if (OnDoorTrip())
                return false;
            ReleaseStation();
            DropDaytime();
            tripDoor = door;
            tripThreshold = threshold;
            tripProp = prop;
            tripLanding = landing;
            tripEmerge = null;
            tripIsHome = false;
            StartWalkToDoor();
            return true;
        }

        /// The reverse: from inside, walk to `exit` (the interior-side
        /// doorway), swing `prop` and step back out at `emerge`.
        public bool LeaveThrough(Transform exit, LegaiaDoor prop, Transform emerge)
        {
            if (loco == null || !indoors)
                return false;
            if (state == 6)
                return false;
            ReleaseStation();
            DropDaytime();
            tripProp = prop;
            tripEmerge = emerge;
            if (exit == null)
            {
                EmergeNow();
                return true;
            }
            state = 6;
            giveUpAt = Time.time + walkTimeout;
            loco.GoToWithin(exit.position, doorArriveRadius);
            return true;
        }

        /// Head home for the night.
        public void GoHome()
        {
            if (indoors || !HasHome() || loco == null)
                return;
            // Already on the way (the director asks every tick): restarting
            // the trip here would re-open the door and reset the walk.
            // A slain villager stays down until it respawns.
            if (OnDoorTrip() || state == 9 || state == 40)
                return;
            // Bounded: a villager whose door it cannot reach used to be
            // sent back at it every few seconds for the whole night.
            if (homeRetries >= homeRetryLimit)
            {
                StandForTheNight();
                return;
            }
            ReleaseStation();
            DropDaytime();
            tripDoor = homeDoor;
            tripThreshold = homeThreshold;
            tripProp = homeDoorProp;
            tripLanding = homeLanding;
            tripEmerge = homeEmerge;
            tripIsHome = true;
            StartWalkToDoor();
            // "Off home": the house pictogram as the walk starts.
            Speak(ICON_HOUSE, 2.5f);
        }

        /// A door trip interrupts the daytime layer: the itinerary is
        /// dropped, a companion stops being led, and the hands are
        /// emptied - nobody walks home for the night carrying a bucket.
        void DropDaytime()
        {
            leader = null;
            planCount = 0;
            planCursor = 0;
            if (carry != null)
                carry.Hide();
        }

        void StartWalkToDoor()
        {
            state = 5;
            giveUpAt = Time.time + walkTimeout;
            // A wider circle than a plain errand: see doorArriveRadius.
            loco.GoToWithin(tripDoor.position, doorArriveRadius);
        }

        /// Come back out at dawn.
        public void ComeOut()
        {
            if (!indoors || loco == null || state == 40)
                return;
            if (state == 6)
                return; // already walking to the way out
            tripIsHome = true;
            LeaveThrough(homeExit, homeDoorProp, homeEmerge);
        }

        // --- Machine ----------------------------------------------------------

        public void BrainTick()
        {
            SendCustomEventDelayedSeconds("BrainTick", 0.1f);
            tickCount++;
            if (loco == null)
                return;
            // A reaction the director (or a walking companion) scheduled.
            if (pendingIcon >= 0 && Time.time >= pendingAt)
            {
                int icon = pendingIcon;
                pendingIcon = -1;
                Speak(icon, pendingSeconds);
            }
            nightHostSeated = NightHostSeated();
            if (state == 0)
                TickStroll();
            else if (state == 1)
                TickGoStation();
            else if (state == 2)
                TickAtStation();
            else if (state == 3)
                TickGoChat();
            else if (state == 4)
                TickChat();
            else if (state == 5)
                TickGoDoor();
            else if (state == 6)
                TickGoExit();
            else if (state == 7)
                TickDoorOpening();
            else if (state == 8)
                TickGoThreshold();
            else if (state == 9)
                TickNightIdle();
            else if (state == 10)
                TickErrandGo();
            else if (state == 11)
                TickErrandStop();
            else if (state == 12)
                TickGreet();
            else if (state == 13)
                TickFollow();
            else if (state == 20)
                TickGoMeet();
            else if (state == 21)
                TickMeet();
            else if (state == 30)
                TickNightHostWait();
            else if (state == 40)
                TickDead();
        }

        // --- Strolling, and noticing the player -------------------------------

        /// The amble itself is the locomotion controller's; the only thing
        /// decided here is whether the villager has noticed the LOCAL
        /// player standing next to it. Purely cosmetic and purely local -
        /// each client's copy glances at its own player, nothing is synced,
        /// and the glance is bounded by `busyUntil` so the director never
        /// hands this villager an errand in the middle of it.
        void TickStroll()
        {
            if (noticeUntil > 0f)
            {
                if (Time.time < noticeUntil)
                    return;
                noticeUntil = 0f;
                loco.Stop();       // back to the amble
                return;
            }
            if (Time.time < busyUntil || NightHostOnDuty())
                return;
            VRCPlayerApi p = Networking.LocalPlayer;
            if (p == null)
                return;
            Vector3 pp = p.GetPosition();
            Vector3 d = pp - transform.position;
            d.y = 0f;
            float nd = noticeDistance < 0.5f ? 0.5f : noticeDistance;
            if (d.sqrMagnitude > nd * nd)
                return;
            noticeUntil = Time.time + noticeSeconds;
            busyUntil = noticeUntil;
            loco.FaceToward(pp);
            // A wave is rationed per villager: one every time somebody walks
            // past would read as a shop greeter rather than as a village.
            if (Time.time < waveFreeAt)
                return;
            waveFreeAt = Time.time + wavePlayerCooldown;
            Speak(ICON_WAVE, noticeSeconds * 0.8f);
        }

        // Gave up on the door for tonight: step BACK from the doorway and
        // wait there until the town stops sheltering, facing home. It used
        // to stand where it got to, which is a metre from its own front
        // door - the one place in the village a villager must never spend
        // the night, because it is the tile a player walks through. The
        // alternative to waiting at all - being sent back at an
        // unreachable door every few seconds - is the aimless wandering
        // this replaces.
        void TickNightIdle()
        {
            if (director != null && !director.Sheltering())
            {
                homeRetries = 0;
                nightIdleParked = false;
                BackToStroll(1f);
                return;
            }
            if (nightIdleParked)
                return;
            if (loco.Arrived() || loco.Blocked() || Time.time > giveUpAt)
            {
                nightIdleParked = true;
                loco.FaceToward(nightIdleLook);
            }
        }

        void StandForTheNight()
        {
            state = 9;
            // Nowhere to go tonight: a yawn, then quiet.
            Speak(ICON_SLEEP, 4f);
            Transform look = homeThreshold != null ? homeThreshold : homeDoor;
            nightIdleParked = false;
            if (look == null)
            {
                loco.SetIdle(true);
                nightIdleParked = true;
                return;
            }
            nightIdleLook = look.position;
            // A few metres back along the line it came in on. The direction
            // is measured from where the villager already stands, so the
            // step back is into the village rather than through the hut.
            Vector3 away = transform.position - nightIdleLook;
            away.y = 0f;
            away = away.sqrMagnitude < 1e-4f ? Vector3.forward : away.normalized;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(nightIdleLook + away * (nightIdleBackOff < 1.5f
                ? 1.5f : nightIdleBackOff));
        }

        /// A door trip that did not work out: count it, and stop trying
        /// once the budget is spent.
        void DoorTripFailed()
        {
            homeRetries++;
            if (tripIsHome && homeRetries >= homeRetryLimit)
            {
                Debug.Log("[Legaia] " + gameObject.name + " could not reach its " +
                    "front door in " + homeRetries + " tries - standing out " +
                    "for the night.");
                StandForTheNight();
                return;
            }
            BackToStroll(6f + NextFloat() * 6f);
        }


        // --- Errand legs -------------------------------------------------------

        void TickErrandGo()
        {
            if (loco.Arrived())
            {
                state = 11;
                loco.FaceToward(station.StandPosition() + station.StandForward());
                float dwell = station.dwellSeconds;
                if (dwell < 2f)
                    dwell = 2f;
                leaveAt = Time.time + dwell * (0.7f + NextFloat() * 0.8f);
                glanceAt = Time.time + 1.5f + NextFloat() * 2.5f;
                // The handler runs the stop's own business: a cupboard
                // swings, a bucket lands in the villager's hands, a fixed
                // resident answers.
                station.Arrive();
                if (station.arriveIcon >= 0)
                    Speak(station.arriveIcon, 2.6f);
                return;
            }
            // A stop that cannot be reached is dropped, not retried: the
            // errand goes on to the next one rather than the villager
            // standing against a wall for the length of the walk timeout.
            if (loco.Blocked() || Time.time > giveUpAt)
            {
                ReleaseStation();
                NextStop();
            }
        }

        void TickErrandStop()
        {
            bool revoked = station != null && !station.available;
            if (Time.time < leaveAt && !revoked)
            {
                // Idle business: look about every few seconds instead of
                // standing rigid at the stop's exact facing.
                if (station != null && station.glance && Time.time >= glanceAt)
                {
                    glanceAt = Time.time + 2f + NextFloat() * 3.5f;
                    Vector3 f = station.StandForward();
                    glanceAim = station.StandPosition()
                        + Quaternion.AngleAxis((NextFloat() - 0.5f) * 110f,
                            Vector3.up) * f * 2f;
                    loco.FaceToward(glanceAim);
                }
                return;
            }
            ReleaseStation();
            NextStop();
        }

        void TickGoStation()
        {
            if (loco.Arrived())
            {
                state = 2;
                // Stand on the spot and turn onto the station's own +Z (the
                // builder points it at the cupboard / view). FaceToward
                // turns in place - no translation - which is exactly the
                // "standing there" pose.
                loco.FaceToward(station.StandPosition() + station.StandForward());
                float dwell = station.dwellSeconds;
                if (dwell < 2f)
                    dwell = 2f;
                leaveAt = Time.time + dwell * (0.7f + NextFloat() * 0.8f);
                station.Arrive();
                return;
            }
            if (loco.Blocked() || Time.time > giveUpAt)
            {
                NoteFailure(loco.Blocked() ? "blocked" : "timed out");
                ReleaseStation();
                BackToStroll(4f + NextFloat() * 8f);
            }
        }

        /// Remember a station this villager could not reach, so the
        /// director hands it to somebody else for a while (RecentlyFailed).
        void NoteFailure(string what)
        {
            failedStation = station;
            failedAt = Time.time;
            lastFailure = what + " -> " + (station != null ? station.name : "?") +
                          (loco != null && loco.Blocked() ? ": " + loco.blockReason : "");
        }

        /// True while this villager's last failed station walk was to `s`
        /// and less than `failRetrySeconds` ago.
        public bool RecentlyFailed(LegaiaNpcStation s)
        {
            return s != null && failedStation == s &&
                   Time.time - failedAt < failRetrySeconds;
        }

        void TickAtStation()
        {
            // A handler may withdraw the station while the villager is on
            // it - the card table when players sit down or press "shoo",
            // a fishing spot when a player walks onto it. Leave at once
            // rather than at the end of the dwell.
            bool revoked = station != null && !station.available;
            if (Time.time < leaveAt && !revoked)
                return;
            ReleaseStation();
            BackToStroll(revoked ? 0.5f : 1f + NextFloat() * 4f);
        }

        void TickGoChat()
        {
            if (loco.Arrived())
            {
                state = 4;
                loco.FaceToward(chatCentre);
                station.Arrive();
                return;
            }
            if (loco.Blocked() || Time.time > giveUpAt)
            {
                ReleaseStation();
                BackToStroll(4f + NextFloat() * 8f);
            }
        }

        void TickChat()
        {
            // Hold the ring pose, but face whoever is TALKING when the
            // director has named one - the ring's centre is only the
            // fallback for a group nobody has spoken in yet.
            loco.FaceToward(AttentionAim(chatCentre));
        }

        void TickGoDoor()
        {
            if (loco.Arrived())
            {
                // At the stand spot: turn onto the doorway and open the
                // door the way a player's approach does, then wait for the
                // swing before stepping onto the tile.
                Vector3 tile = tripThreshold != null
                    ? tripThreshold.position : tripDoor.position;
                loco.FaceToward(tile);
                if (tripProp != null)
                {
                    tripProp.NpcOpen();
                    state = 7;
                    leaveAt = Time.time + Mathf.Max(0.1f, doorSwingSeconds);
                    return;
                }
                StepOntoThreshold();
                return;
            }
            if (loco.Blocked() || Time.time > giveUpAt)
                DoorTripFailed();
        }

        void TickDoorOpening()
        {
            if (Time.time < leaveAt)
                return;
            StepOntoThreshold();
        }

        // The last step: onto the doorway tile (the teleport trigger a
        // player walks into). No tile paired with this home = go straight
        // through from the stand spot.
        void StepOntoThreshold()
        {
            if (tripThreshold == null)
            {
                EnterHome();
                return;
            }
            state = 8;
            giveUpAt = Time.time + thresholdStepSeconds + 2f;
            // A SCRIPTED slide, not a steered walk. The tile sits inside
            // the door frame with the hut wall on both sides and the
            // (visually open, but still solid in the merged world
            // collider) leaf beside it, so the probe fan reads the opening
            // as a wall and the villager sidesteps instead of walking in.
            // The step is under a metre, straight ahead, and still follows
            // the floor - it is what a player does at the same tile.
            loco.SlideTo(tripThreshold.position, thresholdStepSeconds);
        }

        void TickGoThreshold()
        {
            // A step from the door already: blocked or late, go in anyway
            // rather than leave the door hanging open on an empty step.
            if (loco.Arrived() || loco.Blocked() || Time.time > giveUpAt)
                EnterHome();
        }

        // Through the doorway: the same landing + facing the player's
        // teleport uses. The door swings shut once the villager is inside
        // (NpcClose defers to a player-opened latch and other users).
        void EnterHome()
        {
            Vector3 facing = tripLanding.forward;
            loco.Teleport(tripLanding.position, facing);
            indoors = true;
            homeRetries = 0;
            loco.radius = indoorRadius;
            loco.keepOutCap = indoorKeepOut;
            loco.SetHome(tripLanding.position);
            if (tripProp != null)
                SendCustomEventDelayedSeconds("CloseHomeDoor", 0.7f);
            BackToStroll(1f);
        }

        /// Deferred door close after going in or coming out.
        public void CloseHomeDoor()
        {
            if (tripProp != null)
                tripProp.NpcClose();
        }

        void TickGoExit()
        {
            if (loco.Arrived())
            {
                EmergeNow();
                return;
            }
            if (loco.Blocked() || Time.time > giveUpAt)
                EmergeNow(); // never leave a villager stuck inside at dawn
        }

        void EmergeNow()
        {
            Vector3 pos = tripEmerge != null
                ? tripEmerge.position
                : (tripDoor != null ? tripDoor.position : outdoorHome);
            Vector3 facing = tripEmerge != null
                ? tripEmerge.forward
                : (outdoorHome - pos);
            // Out through the door: it swings open as the villager appears
            // on the village side and shuts again a moment later.
            if (tripProp != null)
            {
                tripProp.NpcOpen();
                SendCustomEventDelayedSeconds("CloseHomeDoor", 1.6f);
            }
            loco.Teleport(pos, facing);
            indoors = false;
            loco.radius = outdoorRadius;
            loco.keepOutCap = 1e9f;
            loco.SetHome(pos);
            outdoorHome = pos;
            BackToStroll(1f);
        }

        void BackToStroll(float cooldown)
        {
            state = 0;
            attending = false;
            pendingIcon = -1;
            noticeUntil = 0f;
            busyUntil = Time.time + cooldown;
            if (bubble != null)
                bubble.Hide();
            loco.Stop();
        }

        void ReleaseStation()
        {
            if (station == null)
                return;
            station.Release();
            station = null;
        }
    }
}
