// The town's scheduler: one instance under the built root's `living_town`
// child. It owns nothing that moves - LegaiaNpcBrain executes and
// LegaiaNpcWander walks - it only decides WHO does WHAT and WHEN:
//
//   - conversations: matchmakes 2 OR 3 villagers onto a free chat ring
//     (three stand points around one spot), waits for everyone to arrive,
//     then runs the turn-taking that pops a speech bubble over whoever is
//     "talking", and disperses the group;
//   - AD-HOC conversations: two or three villagers who are simply standing
//     near one another get the same turn-taking with no ring at all, on a
//     centre invented from where they are. A village of four cannot fill
//     a three-point ring often; it can very easily have three people in
//     one corner of it;
//   - ERRANDS: an idle villager is given an ITINERARY of two to four
//     stations rather than one station - the shore, then a neighbour's
//     door, then home past the gate - and walks it stop by stop, with a
//     rest at the end. Picking is GENERIC over `kind` - use-prop, fishing,
//     seat, viewpoint, carry endpoint, visit spot and any kind a future
//     handler introduces are all just "a free station on my side of the
//     door", so stations built by other passes (the fishing spots and
//     card-table seats) land in itineraries without this file knowing they
//     exist. Only kind 3 (chat) is special-cased, because a ring is
//     matchmade as a group rather than handed out singly;
//   - PASSING GREETINGS: two villagers who walk within arm's reach of one
//     another stop, turn, one waves, and both resume the walk they were
//     on. Rate-limited per PAIR, and measured on the director's own 1 Hz
//     tick out of one cached position array - no per-frame proximity work
//     anywhere in the town;
//   - COMPANIONS: now and then the villager given an errand is given
//     somebody to walk it with, who keeps to its side and talks to it at
//     the end;
//   - the day/night routine: at nightfall (LegaiaDayNight.isNight)
//     villagers walk to their assigned home door and go inside through
//     the manifest's own doorway pair, the way a player does, and come
//     back out at dawn. A few (`daytimeIndoors` on their brain) stay
//     in by day as well.
//
// Homes are assigned at BUILD time (LegaiaLivingTown, seeded shuffle,
// capped per door) rather than here, so every client agrees on who lives
// where without a single synced variable - the same reasoning that lets
// the wander behaviour simulate locally.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using UnityEngine.AI;

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

        [Tooltip("Extra roots scanned for stations at Start (the top-level kit containers, outside this behaviour's own root).")]
        public Transform[] extraStationRoots;

        [Tooltip("Scene-constant seed for the director's own choices.")]
        public int seed = 20260907;

        [Tooltip("Seconds between scheduling decisions (movement stays per-frame).")]
        public float decisionInterval = 1f;

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

        // --- Errands, greetings, companions ------------------------------------

        [Tooltip("Share of hand-outs that are a multi-stop ITINERARY rather than one station.")]
        public float errandChance = 0.7f;

        [Tooltip("Longest itinerary the director builds (the brain caps it too).")]
        public int errandStops = 3;

        [Tooltip("How far a villager will walk between an errand's stops (meters).")]
        public float errandSeekRadius = 60f;

        [Tooltip("How close two walking villagers must pass to greet each other (meters).")]
        public float greetDistance = 1.7f;

        [Tooltip("Chance a passing pair actually stops for a greeting.")]
        public float greetChance = 0.85f;

        [Tooltip("The same PAIR will not greet again for this long (seconds).")]
        public float pairCooldown = 40f;

        [Tooltip("Chance an errand is walked by TWO villagers side by side.")]
        public float togetherChance = 0.3f;

        [Tooltip("How far the director will look for somebody to walk along with (meters).")]
        public float companionRadius = 12f;

        [Tooltip("Chance per decision that villagers standing near each other strike up a conversation on the spot.")]
        public float meetChance = 0.4f;

        [Tooltip("How near two idle villagers must be to talk where they stand (meters).")]
        public float meetDistance = 5f;

        [Tooltip("How far apart the members of an ad-hoc conversation stand (meters).")]
        public float meetRingRadius = 0.8f;

        [Tooltip("Check the navmesh before sending anybody anywhere: a station " +
                 "or a ring with no route from where the villager stands is " +
                 "skipped instead of walked at until the walk times out. " +
                 "Off, or with no bake, everything counts as reachable.")]
        public bool routeCheck = true;

        [Tooltip("Navmesh route checks per decision, at most (each is one CalculatePath).")]
        public int routeChecksPerTick = 6;

        [Tooltip("How far off the navmesh a route check may snap either end (meters).")]
        public float routeSnap = 1.5f;

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
        private int[] pickIndex = new int[GROUP_SIZE];

        // Per-tick scratch, all sized once in Start: the greeting scan is
        // O(n^2) over villagers and must not read a transform or call into
        // a behaviour inside the inner loop.
        private Vector3[] where;
        private bool[] canGreet;
        private bool[] freeNow;
        private bool[] insideNow;
        private float[] pairFreeAt;      // n*n, the per-PAIR greeting cooldown
        private LegaiaNpcStation[] planStops;
        private int routeBudget;
        private NavMeshPath probePath;
        private int navState;            // 0 unprobed, 1 a mesh is here, 2 none
        private int forceWaits;
        // NavMesh.AllAreas is not reachable through Udon's type exposure.
        private const int ALL_AREAS = -1;
        // LegaiaBubbleArt.ICON_NAMES index of the raised hand.
        private const int ICON_WAVE = 6;

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
            int n = brains == null ? 0 : brains.Length;
            where = new Vector3[n];
            canGreet = new bool[n];
            freeNow = new bool[n];
            insideNow = new bool[n];
            pairFreeAt = new float[n * n];
            planStops = new LegaiaNpcStation[errandStops < 2 ? 2 : errandStops];
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

            routeBudget = routeChecksPerTick < 1 ? 1 : routeChecksPerTick;
            Survey();
            bool shelter = Sheltering();
            RunShelter(shelter);
            RunConversations();
            RunGreetings();
            // Ad-hoc meetings first, then rings: both take villagers out of
            // the free pool, and every later pass asks Available() live
            // rather than trusting Survey's snapshot - two passes claiming
            // the same villager in one tick would leave one of them holding
            // a group member that is walking somewhere else.
            StartMeet();
            if (!shelter)
                StartConversation();
            AssignWork();
        }

        /// One pass over the villagers per decision, caching what the rest
        /// of the tick asks about them. Every later loop reads these arrays
        /// instead of touching a transform or calling into a behaviour -
        /// which is what keeps the O(n^2) greeting scan below to plain
        /// float arithmetic.
        void Survey()
        {
            if (brains == null || where == null)
                return;
            for (int i = 0; i < brains.Length && i < where.Length; i++)
            {
                LegaiaNpcBrain b = brains[i];
                if (b == null)
                {
                    canGreet[i] = false;
                    freeNow[i] = false;
                    continue;
                }
                where[i] = b.transform.position;
                canGreet[i] = b.Greetable();
                freeNow[i] = b.Available();
                insideNow[i] = b.Indoors();
            }
        }

        /// True while the town should be indoors: night.
        public bool Sheltering()
        {
            return dayNight != null && dayNight.isNight;
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
                    else if (!b.HasHome() && b.OnErrand())
                    {
                        // Nowhere to go in (town01's beach pair is cut off
                        // from every door): settle where it is rather than
                        // keep running errands round an empty village.
                        b.StopActivity();
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
                    if (b == null || !b.InTalk())
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
                        if (!convBrains[g * GROUP_SIZE + i].AtTalk())
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
                    b.EndTalk();
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
                // A ring across a bank a villager cannot climb is not a
                // conversation, it is a villager walking at a wall until
                // the gather times out.
                if (!Routable(b.transform.position, centre))
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
            // A FORCED three-way that finds only two people must WAIT, not
            // settle for a pair: settling spends the force on a two-way,
            // `twoWayRun` keeps climbing, and the force fires again next
            // tick with the same two people - which is why a village of
            // four never showed a group of three. The wait is bounded so a
            // town that genuinely never has three free villagers at once
            // still gets its conversations.
            if (found < want && forceThree)
            {
                forceWaits++;
                if (forceWaits <= 8)
                    return;
                forceWaits = 0;
            }
            else if (want >= 3)
            {
                forceWaits = 0;
            }
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
                    convBrains[free * GROUP_SIZE + i].EndTalk();
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

        // --- Passing greetings -----------------------------------------------------

        /// Two villagers walking past each other stop and say hello. The
        /// scan is O(n^2) but every term is a cached position and a cached
        /// flag (see Survey), it runs at the DECISION rate rather than per
        /// frame, and nothing here raycasts - which is what lets it stay on
        /// while the town runs on Quest.
        void RunGreetings()
        {
            if (brains == null || where == null || pairFreeAt == null)
                return;
            int n = brains.Length;
            float r2 = greetDistance * greetDistance;
            float now = Time.time;
            for (int i = 0; i < n; i++)
            {
                if (!canGreet[i])
                    continue;
                for (int j = i + 1; j < n; j++)
                {
                    if (!canGreet[j])
                        continue;
                    if (insideNow[i] != insideNow[j])
                        continue;
                    if (now < pairFreeAt[i * n + j])
                        continue;
                    Vector3 d = where[i] - where[j];
                    d.y = 0f;
                    if (d.sqrMagnitude > r2)
                        continue;
                    // Survey's flags are up to a decision old; ask both
                    // again now that a pair is actually in reach, so one of
                    // them can never bow to somebody who walked straight on.
                    if (!brains[i].Greetable() || !brains[j].Greetable())
                        continue;
                    pairFreeAt[i * n + j] = now + pairCooldown;
                    if (NextFloat() > greetChance)
                        continue;
                    // One of them waves and the other answers with a plain
                    // talk bubble, so the exchange reads as an exchange.
                    bool firstWaves = NextInt(2) == 0;
                    brains[i].Greet(where[j], firstWaves ? ICON_WAVE : 0);
                    brains[j].Greet(where[i], firstWaves ? 0 : ICON_WAVE);
                    canGreet[i] = false;
                    canGreet[j] = false;
                    break;
                }
            }
        }

        // --- Conversations struck up on the spot -----------------------------------

        /// Two (or three) villagers standing near one another start talking
        /// where they are: no ring, no station claimed, a centre invented
        /// from their own positions. This is what makes three-way groups
        /// happen in a village too small to fill a three-point ring.
        void StartMeet()
        {
            if (brains == null || where == null)
                return;
            if (NextFloat() > meetChance)
                return;
            int free = -1;
            int running = 0;
            for (int g = 0; g < MAX_GROUPS; g++)
            {
                if (convState[g] != 0)
                    running++;
                else if (free < 0)
                    free = g;
            }
            if (free < 0 || running >= maxConversations)
                return;

            int n = brains.Length;
            float r2 = meetDistance * meetDistance;
            int start = NextInt(n < 1 ? 1 : n);
            for (int a = 0; a < n; a++)
            {
                int i = (start + a) % n;
                if (!freeNow[i])
                    continue;
                int size = 0;
                pickBrains[size] = brains[i];
                pickIndex[size] = i;
                size++;
                Vector3 centre = where[i];
                for (int j = 0; j < n && size < GROUP_SIZE; j++)
                {
                    if (j == i || !freeNow[j] || insideNow[j] != insideNow[i])
                        continue;
                    Vector3 d = where[j] - where[i];
                    d.y = 0f;
                    if (d.sqrMagnitude > r2)
                        continue;
                    pickBrains[size] = brains[j];
                    pickIndex[size] = j;
                    centre += where[j];
                    size++;
                }
                if (size < 2)
                    continue;
                centre /= size;

                // Everybody takes a step onto the ring around that centre,
                // keeping the bearing they already had - so the group
                // closes up rather than swapping places.
                for (int k = 0; k < size; k++)
                {
                    LegaiaNpcBrain b = pickBrains[k];
                    Vector3 out3 = b.transform.position - centre;
                    out3.y = 0f;
                    if (out3.sqrMagnitude < 1e-4f)
                        out3 = Quaternion.AngleAxis(k * 120f, Vector3.up)
                               * Vector3.forward;
                    else
                        out3 = out3.normalized;
                    b.JoinMeet(centre + out3 * meetRingRadius, centre);
                    convBrains[free * GROUP_SIZE + k] = b;
                    freeNow[pickIndex[k]] = false;
                }
                for (int k = size; k < GROUP_SIZE; k++)
                    convBrains[free * GROUP_SIZE + k] = null;
                convSize[free] = size;
                convState[free] = 1;
                convTurn[free] = NextInt(size);
                convDeadline[free] = Time.time + gatherTimeout;
                twoWayRun = size >= 3 ? 0 : twoWayRun + 1;
                return;
            }
        }

        // --- Errands ---------------------------------------------------------------

        /// Give one idle villager something to do: usually a multi-stop
        /// ITINERARY, sometimes a single station (the old behaviour, kept
        /// because one errand in a village of four should not always be a
        /// tour). Occasionally a second villager is sent along with it.
        void AssignWork()
        {
            if (brains == null || stations == null)
                return;
            if (NextFloat() > stationChance)
                return;
            int start = NextInt(brains.Length < 1 ? 1 : brains.Length);
            for (int i = 0; i < brains.Length; i++)
            {
                int idx = (start + i) % brains.Length;
                LegaiaNpcBrain b = brains[idx];
                if (b == null || !b.Available())
                    continue;
                if (NextFloat() < errandChance && StartErrand(b, idx))
                    return;
                LegaiaNpcStation s = PickStation(b, stationSeekRadius);
                if (s == null)
                    continue;
                if (!s.Claim(b.transform, b))
                    continue;
                b.SendToStation(s);
                return;
            }
        }

        /// Build an itinerary for `b` out of free stations near it and set
        /// it walking. Stops are picked one at a time from the LAST stop's
        /// position, so the route reads as a round rather than as a star
        /// out of one point and back.
        bool StartErrand(LegaiaNpcBrain b, int idx)
        {
            if (planStops == null)
                return false;
            int want = 2 + NextInt(planStops.Length - 1);
            Vector3 from = b.transform.position;
            int got = 0;
            // `prefer` is what sequences a fetch: once a stop has put
            // something in the villager's hands (carryFlow 1), the next
            // stop is preferentially one that takes it back (2), so the
            // bucket filled at the water is carried to a doorstep instead
            // of being walked in a circle and dropped where it started.
            int prefer = 0;
            for (int k = 0; k < want; k++)
            {
                LegaiaNpcStation s = PickStationFrom(b, from,
                    k == 0 ? errandSeekRadius : errandSeekRadius * 0.6f,
                    got, k == 0, prefer);
                if (s == null)
                    break;
                planStops[got++] = s;
                from = s.StandPosition();
                if (s.carryFlow == 1)
                    prefer = 2;
                else if (s.carryFlow == 2)
                    prefer = 0;
            }
            if (got < 1)
                return false;
            b.PlanClear();
            for (int k = 0; k < got; k++)
                b.PlanAdd(planStops[k]);
            if (!b.PlanStart())
                return false;
            if (got >= 2 && NextFloat() < togetherChance)
                SendCompanion(b, idx);
            return true;
        }

        /// Somebody to walk it with: the nearest free villager on the same
        /// side of the door, which keeps to the leader's shoulder for the
        /// length of the errand and talks to it when they arrive.
        void SendCompanion(LegaiaNpcBrain leader, int leaderIdx)
        {
            int n = brains.Length;
            float r2 = companionRadius * companionRadius;
            int start = NextInt(n < 1 ? 1 : n);
            for (int a = 0; a < n; a++)
            {
                int j = (start + a) % n;
                if (j == leaderIdx || !freeNow[j] || brains[j] == null
                    || !brains[j].Available())
                    continue;
                if (insideNow[j] != insideNow[leaderIdx])
                    continue;
                Vector3 d = where[j] - where[leaderIdx];
                d.y = 0f;
                if (d.sqrMagnitude > r2)
                    continue;
                brains[j].FollowLeader(leader, NextInt(2) == 0 ? 1f : -1f);
                freeNow[j] = false;
                return;
            }
        }

        // --- Stations -------------------------------------------------------------

        /// A free station on the villager's own side of the door, of ANY
        /// kind but chat (rings are matchmade as groups). Reservoir-sampled
        /// so the choice spreads over everything in range.
        LegaiaNpcStation PickStation(LegaiaNpcBrain b, float radius)
        {
            return PickStationFrom(b, b.transform.position, radius, 0, true, 0);
        }

        /// The picker itineraries share: free, right side of the door,
        /// within `radius` of `from`, not already in this plan's first
        /// `taken` slots, and - when `checkRoute` - actually reachable over
        /// the navmesh from where the villager stands. That last test is
        /// what stops a beach villager being handed a station up the bank
        /// it cannot climb, which reads in-world as walking into a wall.
        LegaiaNpcStation PickStationFrom(LegaiaNpcBrain b, Vector3 from,
            float radius, int taken, bool checkRoute, int prefer)
        {
            bool inside = b.Indoors();
            Vector3 stood = b.transform.position;
            LegaiaNpcStation best = null;
            LegaiaNpcStation wanted = null;
            int seen = 0;
            int seenPref = 0;
            for (int i = 0; i < stations.Length; i++)
            {
                LegaiaNpcStation s = stations[i];
                if (s == null || s.kind == 3 || !s.IsFree())
                    continue;
                if (s.indoors != inside)
                    continue;
                if (Vector3.Distance(from, s.StandPosition()) > radius)
                    continue;
                bool dup = false;
                for (int k = 0; k < taken; k++)
                    if (planStops[k] == s)
                    {
                        dup = true;
                        break;
                    }
                if (dup)
                    continue;
                seen++;
                if (NextInt(seen) == 0)
                    best = s;
                // Two reservoirs in one sweep: any station, and one that
                // continues the carry the last stop started.
                if (prefer != 0 && s.carryFlow == prefer)
                {
                    seenPref++;
                    if (NextInt(seenPref) == 0)
                        wanted = s;
                }
            }
            if (wanted != null)
                best = wanted;
            if (best != null && checkRoute && !Routable(stood, best.StandPosition()))
                return null;
            return best;
        }

        // --- Reachability ------------------------------------------------------------

        /// Is there a complete navmesh route between these two points? With
        /// no bake registered (or the per-tick budget spent) everything
        /// counts as reachable, so the town behaves exactly as it did
        /// before the bake existed rather than freezing.
        bool Routable(Vector3 from, Vector3 to)
        {
            if (!routeCheck || routeBudget <= 0)
                return true;
            if (navState == 0)
            {
                // The loader registers the bake in its own Start and Start
                // order across behaviours is undefined - probe late.
                if (Time.timeSinceLevelLoad < 2f)
                    return true;
                NavMeshHit probe;
                navState = NavMesh.SamplePosition(from, out probe, 4f, ALL_AREAS)
                    ? 1 : 2;
            }
            if (navState != 1)
                return true;
            routeBudget--;
            NavMeshHit a, c;
            if (!NavMesh.SamplePosition(from, out a, routeSnap, ALL_AREAS))
                return true;
            if (!NavMesh.SamplePosition(to, out c, routeSnap, ALL_AREAS))
                return true;
            if (probePath == null)
                probePath = new NavMeshPath();
            if (!NavMesh.CalculatePath(a.position, c.position, ALL_AREAS, probePath))
                return false;
            return probePath.status == NavMeshPathStatus.PathComplete;
        }
    }
}
