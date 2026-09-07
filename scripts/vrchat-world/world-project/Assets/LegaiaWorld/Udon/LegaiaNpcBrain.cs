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

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcBrain : UdonSharpBehaviour
    {
        [Tooltip("The town director that schedules this villager.")]
        public LegaiaTownDirector director;

        [Tooltip("This NPC's locomotion controller (same GameObject).")]
        public LegaiaNpcWander loco;

        [Tooltip("This NPC's speech bubble (a child of the NPC).")]
        public LegaiaSpeechBubble bubble;

        [Tooltip("The NPC's first dialog line from the manifest - shown under the bubble icon.")]
        public string firstLine = "";

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

        private int state;
        private bool indoors;
        private float leaveAt;
        private float giveUpAt;
        private float busyUntil;
        private Vector3 outdoorHome;
        private Vector3 chatCentre;
        private LegaiaNpcStation station;
        private int rng;
        private bool started;

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
        private Vector3 lastLeaderPos;

        void Start()
        {
            rng = seed == 0 ? 12345 : seed;
            plan = new LegaiaNpcStation[maxStops < 2 ? 2 : maxStops];
            if (loco == null)
                loco = GetComponent<LegaiaNpcWander>();
            if (carry == null)
                carry = GetComponent<LegaiaNpcCarry>();
            outdoorHome = transform.position;
            indoors = startIndoors;
            if (loco != null)
            {
                outdoorRadius = loco.radius;
                // Already in a room: stroll the room's radius from the start.
                if (startIndoors)
                    loco.radius = indoorRadius;
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
        public bool Available()
        {
            return started && state == 0 && station == null
                   && Time.time >= busyUntil;
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

        /// Out on a walk and free to be stopped for a passing greeting: on
        /// an errand leg, and not fresh from greeting somebody else (the
        /// director rate-limits per PAIR on top of this). A COMPANION is
        /// deliberately not greetable - it walks at its leader's shoulder,
        /// so the two of them would greet each other on a timer forever.
        public bool Greetable()
        {
            return started && state == 10 && Time.time >= greetFreeAt;
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
            state = 3;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(slot.StandPosition());
        }

        /// Take a turn in the conversation: an icon over the head, plus this
        /// NPC's own first dialog line when the bubble carries a label.
        public void Speak(int icon, float seconds)
        {
            if (bubble != null)
                bubble.Show(icon, seconds, firstLine);
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
            loco.FaceToward(chatCentre);
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

        /// Head home for the night.
        public void GoHome()
        {
            if (indoors || !HasHome() || loco == null)
                return;
            // Already on the way (the director asks every tick): restarting
            // the trip here would re-open the door and reset the walk.
            if (state == 5 || state == 7 || state == 8)
                return;
            ReleaseStation();
            state = 5;
            giveUpAt = Time.time + walkTimeout;
            loco.GoTo(homeDoor.position);
        }

        /// Come back out at dawn.
        public void ComeOut()
        {
            if (!indoors || loco == null)
                return;
            if (state == 6)
                return; // already walking to the way out
            ReleaseStation();
            if (homeExit != null)
            {
                state = 6;
                giveUpAt = Time.time + walkTimeout;
                loco.GoTo(homeExit.position);
                return;
            }
            // No interior-side doorway was paired with this home: step
            // straight back out at the village landing (or the door).
            EmergeNow();
        }

        // --- Machine ----------------------------------------------------------

        public void BrainTick()
        {
            SendCustomEventDelayedSeconds("BrainTick", 0.1f);
            if (loco == null)
                return;
            if (state == 1)
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
                ReleaseStation();
                BackToStroll(4f + NextFloat() * 8f);
            }
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
            // Hold the ring pose; the director drives the turns and ends it.
            loco.FaceToward(chatCentre);
        }

        void TickGoDoor()
        {
            if (loco.Arrived())
            {
                // At the stand spot: turn onto the doorway and open the
                // door the way a player's approach does, then wait for the
                // swing before stepping onto the tile.
                Vector3 tile = homeThreshold != null
                    ? homeThreshold.position : homeDoor.position;
                loco.FaceToward(tile);
                if (homeDoorProp != null)
                {
                    homeDoorProp.NpcOpen();
                    state = 7;
                    leaveAt = Time.time + Mathf.Max(0.1f, doorSwingSeconds);
                    return;
                }
                StepOntoThreshold();
                return;
            }
            if (loco.Blocked() || Time.time > giveUpAt)
                BackToStroll(15f + NextFloat() * 15f); // try again later
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
            if (homeThreshold == null)
            {
                EnterHome();
                return;
            }
            state = 8;
            giveUpAt = Time.time + 12f;
            loco.GoTo(homeThreshold.position);
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
            Vector3 facing = homeLanding.forward;
            loco.Teleport(homeLanding.position, facing);
            indoors = true;
            loco.radius = indoorRadius;
            loco.SetHome(homeLanding.position);
            if (homeDoorProp != null)
                SendCustomEventDelayedSeconds("CloseHomeDoor", 0.7f);
            BackToStroll(1f);
        }

        /// Deferred door close after going in or coming out.
        public void CloseHomeDoor()
        {
            if (homeDoorProp != null)
                homeDoorProp.NpcClose();
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
            Vector3 pos = homeEmerge != null
                ? homeEmerge.position
                : (homeDoor != null ? homeDoor.position : outdoorHome);
            Vector3 facing = homeEmerge != null
                ? homeEmerge.forward
                : (outdoorHome - pos);
            // Out through the door: it swings open as the villager appears
            // on the village side and shuts again a moment later.
            if (homeDoorProp != null)
            {
                homeDoorProp.NpcOpen();
                SendCustomEventDelayedSeconds("CloseHomeDoor", 1.6f);
            }
            loco.Teleport(pos, facing);
            indoors = false;
            loco.radius = outdoorRadius;
            loco.SetHome(pos);
            outdoorHome = pos;
            BackToStroll(1f);
        }

        void BackToStroll(float cooldown)
        {
            state = 0;
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
