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

        void Start()
        {
            rng = seed == 0 ? 12345 : seed;
            if (loco == null)
                loco = GetComponent<LegaiaNpcWander>();
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
