// The town's scheduler: one instance under the built root's `living_town`
// child. It owns nothing that moves - LegaiaNpcBrain executes and
// LegaiaNpcWander walks - it only decides WHO does WHAT and WHEN:
//
//   - conversations: matchmakes 2 OR 3 villagers onto a free chat ring
//     (three stand points around one spot), waits for everyone to arrive,
//     then runs the turn-taking that pops a speech bubble over whoever is
//     "talking", and disperses the group;
//   - stations: hands an idle villager a free LegaiaNpcStation to visit.
//     Picking is GENERIC over `kind` - use-prop, fishing, seat, viewpoint
//     and any kind a future handler introduces are all just "a free
//     station on my side of the door", so stations built by other passes
//     (the fishing spots and card-table seats) are picked up without this
//     file knowing they exist. Only kind 3 (chat) is special-cased,
//     because a ring is matchmade as a group rather than handed out singly;
//   - the day/night routine: at nightfall (LegaiaDayNight.isNight) - or
//     when the optional weather behaviour reports rain over the shelter
//     threshold - villagers walk to their assigned home door and go inside
//     through the manifest's own doorway pair, the way a player does, and
//     come back out at dawn. A few (`daytimeIndoors` on their brain) stay
//     in by day as well.
//
// Homes are assigned at BUILD time (LegaiaLivingTown, seeded shuffle,
// capped per door) rather than here, so every client agrees on who lives
// where without a single synced variable - the same reasoning that lets
// the wander behaviour simulate locally.
//
// The WEATHER hook is deliberately loose: any UdonSharpBehaviour with a
// public `float rainIntensity` can be dropped into `weather`, and a null
// field (no weather pass built) simply reads as dry. This director never
// names the weather behaviour's type.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaTownDirector : UdonSharpBehaviour
    {
        [Tooltip("Every station in the world - the builder fills this from the built root AND the top-level kit containers, and Start merges in anything it finds under them.")]
        public LegaiaNpcStation[] stations;

        [Tooltip("Every villager brain the living-town pass wired.")]
        public LegaiaNpcBrain[] brains;

        [Tooltip("The day/night cycle to read (dayFactor / isNight). Null = always day.")]
        public LegaiaDayNight dayNight;

        [Tooltip("Optional weather behaviour with a public float `rainIntensity`. Null = dry.")]
        public UdonSharpBehaviour weather;

        [Tooltip("Extra roots scanned for stations at Start (the top-level kit containers, outside this behaviour's own root).")]
        public Transform[] extraStationRoots;

        [Tooltip("Scene-constant seed for the director's own choices.")]
        public int seed = 20260907;

        [Tooltip("Seconds between scheduling decisions (movement stays per-frame).")]
        public float decisionInterval = 1f;

        [Tooltip("Rain above this fraction sends villagers to shelter, like nightfall.")]
        public float rainShelter = 0.5f;

        [Tooltip("Chance per decision that an idle villager is offered a conversation.")]
        public float chatChance = 0.5f;

        [Tooltip("Chance per decision that an idle villager is sent to a station.")]
        public float stationChance = 0.45f;

        [Tooltip("Conversations running at once.")]
        public int maxConversations = 3;

        [Tooltip("Chance a conversation is a THREE-way when three villagers are in range.")]
        public float threeChance = 0.55f;

        [Tooltip("Force a three-way after this many two-way conversations (0 = never force).")]
        public int forceThreeAfter = 2;

        [Tooltip("How long a conversation lasts once everyone has arrived.")]
        public float chatSeconds = 26f;

        [Tooltip("Seconds per speaking turn.")]
        public float turnSeconds = 3.2f;

        [Tooltip("Give up on a conversation nobody reached within this long.")]
        public float gatherTimeout = 40f;

        [Tooltip("How far a villager will walk to join a conversation (meters).")]
        public float chatSeekRadius = 22f;

        [Tooltip("How far a villager will walk to a station (meters).")]
        public float stationSeekRadius = 45f;

        [Tooltip("Chat stations closer together than this belong to the same ring.")]
        public float ringRadius = 2.5f;

        [Tooltip("Villagers sent home (or out) per decision - staggers the exodus at dusk.")]
        public int movesPerTick = 2;

        // --- Conversation bookkeeping ---------------------------------------
        // Fixed flat arrays: Udon has no generic collections, so group g
        // occupies slots [g*3 .. g*3+2].
        private const int MAX_GROUPS = 4;
        private const int GROUP_SIZE = 3;
        private LegaiaNpcBrain[] convBrains = new LegaiaNpcBrain[MAX_GROUPS * GROUP_SIZE];
        private int[] convSize = new int[MAX_GROUPS];
        private int[] convState = new int[MAX_GROUPS];   // 0 free, 1 gathering, 2 talking
        private int[] convTurn = new int[MAX_GROUPS];
        private float[] convDeadline = new float[MAX_GROUPS];
        private float[] convNextTurn = new float[MAX_GROUPS];

        // Ring index per station (-1 = not a chat station).
        private int[] ringOf;
        private int ringCount;
        private int twoWayRun;
        private int rng;

        // Scratch buffers reused every tick (no allocation in the loop).
        private LegaiaNpcBrain[] pickBrains = new LegaiaNpcBrain[GROUP_SIZE];
        private LegaiaNpcStation[] pickSlots = new LegaiaNpcStation[GROUP_SIZE];

        void Start()
        {
            rng = seed == 0 ? 987654321 : seed;
            // Always MERGE rather than only filling an empty array: the
            // builder wires what exists when the living-town pass runs, and
            // a pass that builds stations after it (or a station dropped in
            // by hand) would otherwise be invisible for the world's life.
            RescanStations();
            if (brains == null || brains.Length == 0)
                RescanBrains();
            BuildRings();
            Debug.Log("[Legaia] town director: " +
                (stations == null ? 0 : stations.Length) + " station(s), " +
                (brains == null ? 0 : brains.Length) + " villager(s), " +
                ringCount + " chat ring(s).");
            SendCustomEventDelayedSeconds("DirectorTick", 2f);
        }

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

        // --- Discovery --------------------------------------------------------

        void RescanStations()
        {
            Transform root = transform.root;
            LegaiaNpcStation[] mine =
                root.GetComponentsInChildren<LegaiaNpcStation>(true);
            int extra = stations == null ? 0 : stations.Length;
            if (extraStationRoots != null)
                for (int i = 0; i < extraStationRoots.Length; i++)
                    if (extraStationRoots[i] != null)
                        extra += extraStationRoots[i]
                            .GetComponentsInChildren<LegaiaNpcStation>(true).Length;
            LegaiaNpcStation[] all = new LegaiaNpcStation[mine.Length + extra];
            int n = 0;
            for (int i = 0; i < mine.Length; i++)
                all[n++] = mine[i];
            if (stations != null)
                for (int i = 0; i < stations.Length; i++)
                {
                    if (stations[i] == null)
                        continue;
                    bool dup = false;
                    for (int k = 0; k < n; k++)
                        if (all[k] == stations[i])
                        {
                            dup = true;
                            break;
                        }
                    if (!dup)
                        all[n++] = stations[i];
                }
            if (extraStationRoots != null)
                for (int i = 0; i < extraStationRoots.Length; i++)
                {
                    if (extraStationRoots[i] == null)
                        continue;
                    LegaiaNpcStation[] more = extraStationRoots[i]
                        .GetComponentsInChildren<LegaiaNpcStation>(true);
                    for (int j = 0; j < more.Length; j++)
                    {
                        bool dup = false;
                        for (int k = 0; k < n; k++)
                            if (all[k] == more[j])
                            {
                                dup = true;
                                break;
                            }
                        if (!dup)
                            all[n++] = more[j];
                    }
                }
            LegaiaNpcStation[] packed = new LegaiaNpcStation[n];
            for (int i = 0; i < n; i++)
                packed[i] = all[i];
            stations = packed;
        }

        void RescanBrains()
        {
            brains = transform.root.GetComponentsInChildren<LegaiaNpcBrain>(true);
        }

        // Chat stations within `ringRadius` of one another form one ring of
        // stand points around a single meeting spot. Grouping by proximity
        // rather than by a shared id keeps the station contract untouched.
        void BuildRings()
        {
            int count = stations == null ? 0 : stations.Length;
            ringOf = new int[count];
            for (int i = 0; i < count; i++)
                ringOf[i] = -1;
            ringCount = 0;
            for (int i = 0; i < count; i++)
            {
                if (stations[i] == null || stations[i].kind != 3 || ringOf[i] >= 0)
                    continue;
                int ring = ringCount++;
                ringOf[i] = ring;
                Vector3 p = stations[i].StandPosition();
                for (int j = i + 1; j < count; j++)
                {
                    if (stations[j] == null || stations[j].kind != 3 || ringOf[j] >= 0)
                        continue;
                    if (Vector3.Distance(p, stations[j].StandPosition()) <= ringRadius)
                        ringOf[j] = ring;
                }
            }
        }

        // --- Tick --------------------------------------------------------------

        public void DirectorTick()
        {
            float dt = decisionInterval;
            if (dt < 0.1f)
                dt = 0.1f;
            SendCustomEventDelayedSeconds("DirectorTick", dt);

            bool shelter = Sheltering();
            RunShelter(shelter);
            RunConversations();
            if (!shelter)
                StartConversation();
            AssignStations();
        }

        /// True while the town should be indoors: night, or rain over the
        /// shelter threshold.
        public bool Sheltering()
        {
            bool night = dayNight != null && dayNight.isNight;
            return night || RainIntensity() > rainShelter;
        }

        /// The optional weather behaviour's rain, read loosely by name so
        /// this file never depends on the weather pass existing.
        public float RainIntensity()
        {
            if (weather == null)
                return 0f;
            object v = weather.GetProgramVariable("rainIntensity");
            if (v == null)
                return 0f;
            return (float)v;
        }

        void RunShelter(bool shelter)
        {
            if (brains == null)
                return;
            int budget = movesPerTick < 1 ? 1 : movesPerTick;
            for (int i = 0; i < brains.Length && budget > 0; i++)
            {
                LegaiaNpcBrain b = brains[i];
                if (b == null)
                    continue;
                if (shelter)
                {
                    if (!b.Indoors() && b.HasHome() && !b.InChat())
                    {
                        b.GoHome();
                        budget--;
                    }
                }
                else if (b.Indoors() && !b.daytimeIndoors)
                {
                    b.ComeOut();
                    budget--;
                }
            }
        }

        // --- Conversations ------------------------------------------------------

        void RunConversations()
        {
            for (int g = 0; g < MAX_GROUPS; g++)
            {
                if (convState[g] == 0)
                    continue;
                int size = convSize[g];
                // A member that dropped out (blocked on the way, sent home)
                // ends the meeting rather than leaving the others waiting.
                for (int i = 0; i < size; i++)
                {
                    LegaiaNpcBrain b = convBrains[g * GROUP_SIZE + i];
                    if (b == null || !b.InChat())
                    {
                        EndGroup(g);
                        break;
                    }
                }
                if (convState[g] == 0)
                    continue;

                if (convState[g] == 1)
                {
                    bool all = true;
                    for (int i = 0; i < size; i++)
                        if (!convBrains[g * GROUP_SIZE + i].AtChat())
                        {
                            all = false;
                            break;
                        }
                    if (all)
                    {
                        convState[g] = 2;
                        convDeadline[g] = Time.time + chatSeconds;
                        convNextTurn[g] = Time.time + 0.4f;
                    }
                    else if (Time.time > convDeadline[g])
                    {
                        EndGroup(g);
                    }
                    continue;
                }

                if (Time.time > convDeadline[g])
                {
                    EndGroup(g);
                    continue;
                }
                if (Time.time < convNextTurn[g])
                    continue;
                // Turn-taking: round-robin, with an occasional skip so it
                // does not read as a metronome.
                int turn = convTurn[g];
                if (NextFloat() < 0.25f)
                    turn++;
                convTurn[g] = (turn + 1) % size;
                LegaiaNpcBrain speaker = convBrains[g * GROUP_SIZE + (turn % size)];
                float dur = turnSeconds * (0.7f + NextFloat() * 0.4f);
                speaker.Speak(PickIcon(), dur);
                convNextTurn[g] = Time.time + dur + 0.3f + NextFloat() * 0.6f;
            }
        }

        // 0 "...", 1 "!", 2 "?", 3 heart, 4 music, 5 laugh - weighted so the
        // plain talk bubble dominates and the flourishes stay occasional.
        int PickIcon()
        {
            int r = NextInt(100);
            if (r < 45) return 0;
            if (r < 60) return 1;
            if (r < 75) return 2;
            if (r < 84) return 5;
            if (r < 93) return 4;
            return 3;
        }

        void EndGroup(int g)
        {
            int size = convSize[g];
            for (int i = 0; i < size; i++)
            {
                LegaiaNpcBrain b = convBrains[g * GROUP_SIZE + i];
                if (b != null)
                    b.EndChat();
                convBrains[g * GROUP_SIZE + i] = null;
            }
            convSize[g] = 0;
            convState[g] = 0;
        }

        void StartConversation()
        {
            if (brains == null || stations == null || ringOf == null)
                return;
            int running = 0;
            int free = -1;
            for (int g = 0; g < MAX_GROUPS; g++)
            {
                if (convState[g] != 0)
                    running++;
                else if (free < 0)
                    free = g;
            }
            if (free < 0 || running >= maxConversations)
                return;
            if (NextFloat() > chatChance)
                return;

            // A ring is usable when every one of its stand points is free.
            int ring = -1;
            int slots = 0;
            int startRing = NextInt(ringCount < 1 ? 1 : ringCount);
            for (int r = 0; r < ringCount && ring < 0; r++)
            {
                int cand = (startRing + r) % ringCount;
                int n = 0;
                bool ok = true;
                for (int i = 0; i < stations.Length; i++)
                {
                    if (ringOf[i] != cand)
                        continue;
                    if (!stations[i].IsFree())
                    {
                        ok = false;
                        break;
                    }
                    if (n < GROUP_SIZE)
                        pickSlots[n++] = stations[i];
                }
                if (ok && n >= 2)
                {
                    ring = cand;
                    slots = n;
                }
            }
            if (ring < 0)
                return;

            Vector3 centre = Vector3.zero;
            for (int i = 0; i < slots; i++)
                centre += pickSlots[i].StandPosition();
            centre /= slots;

            // Who is near enough and free? Reservoir-sample up to three so
            // the same neighbours do not always pair off.
            int want = 2;
            bool forceThree = forceThreeAfter > 0 && twoWayRun >= forceThreeAfter;
            if (slots >= 3 && (forceThree || NextFloat() < threeChance))
                want = 3;
            int found = 0;
            int seen = 0;
            for (int i = 0; i < GROUP_SIZE; i++)
                pickBrains[i] = null;
            for (int i = 0; i < brains.Length; i++)
            {
                LegaiaNpcBrain b = brains[i];
                if (b == null || !b.Available())
                    continue;
                if (b.Indoors() != pickSlots[0].indoors)
                    continue;
                if (Vector3.Distance(b.transform.position, centre) > chatSeekRadius)
                    continue;
                seen++;
                if (found < want)
                {
                    pickBrains[found++] = b;
                }
                else if (NextInt(seen) < want)
                {
                    pickBrains[NextInt(want)] = b;
                }
            }
            if (found < 2)
                return;
            if (found < want)
                want = found;

            for (int i = 0; i < want; i++)
            {
                LegaiaNpcStation slot = pickSlots[i];
                LegaiaNpcBrain b = pickBrains[i];
                if (!slot.Claim(b.transform, b))
                    continue;
                convBrains[free * GROUP_SIZE + i] = b;
                b.JoinChat(slot, centre);
            }
            // Compact: a slot that failed to claim leaves a hole.
            int size = 0;
            for (int i = 0; i < want; i++)
            {
                LegaiaNpcBrain b = convBrains[free * GROUP_SIZE + i];
                if (b == null)
                    continue;
                convBrains[free * GROUP_SIZE + size] = b;
                size++;
            }
            for (int i = size; i < GROUP_SIZE; i++)
                convBrains[free * GROUP_SIZE + i] = null;
            if (size < 2)
            {
                for (int i = 0; i < size; i++)
                    convBrains[free * GROUP_SIZE + i].EndChat();
                for (int i = 0; i < GROUP_SIZE; i++)
                    convBrains[free * GROUP_SIZE + i] = null;
                return;
            }
            convSize[free] = size;
            convState[free] = 1;
            convTurn[free] = NextInt(size);
            convDeadline[free] = Time.time + gatherTimeout;
            twoWayRun = size >= 3 ? 0 : twoWayRun + 1;
        }

        // --- Stations -------------------------------------------------------------

        void AssignStations()
        {
            if (brains == null || stations == null)
                return;
            if (NextFloat() > stationChance)
                return;
            // One villager per decision: the town fills its stations over a
            // minute or so instead of everybody moving at once.
            int start = NextInt(brains.Length < 1 ? 1 : brains.Length);
            for (int i = 0; i < brains.Length; i++)
            {
                LegaiaNpcBrain b = brains[(start + i) % brains.Length];
                if (b == null || !b.Available())
                    continue;
                LegaiaNpcStation s = PickStation(b);
                if (s == null)
                    continue;
                if (!s.Claim(b.transform, b))
                    continue;
                b.SendToStation(s);
                return;
            }
        }

        /// A free station on the villager's own side of the door, of ANY
        /// kind but chat (rings are matchmade as groups). Reservoir-sampled
        /// so the choice spreads over everything in range.
        LegaiaNpcStation PickStation(LegaiaNpcBrain b)
        {
            Vector3 p = b.transform.position;
            bool inside = b.Indoors();
            LegaiaNpcStation best = null;
            int seen = 0;
            for (int i = 0; i < stations.Length; i++)
            {
                LegaiaNpcStation s = stations[i];
                if (s == null || s.kind == 3 || !s.IsFree())
                    continue;
                if (s.indoors != inside)
                    continue;
                if (Vector3.Distance(p, s.StandPosition()) > stationSeekRadius)
                    continue;
                seen++;
                if (NextInt(seen) == 0)
                    best = s;
            }
            return best;
        }
    }
}
