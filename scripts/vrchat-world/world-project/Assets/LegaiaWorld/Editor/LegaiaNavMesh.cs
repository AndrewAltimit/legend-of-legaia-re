// The NPC navmesh: a walkable-surface mesh baked from the scene's physics
// colliders at build time, so a villager's commanded walk (to a station,
// a chat ring, its front door) follows a route over ground it can stand
// on instead of a straight line through a hillside or a hut wall.
//
// WHY COLLIDERS, NOT RENDERERS: the built root is X-mirrored, which flips
// every render mesh's winding, and a navmesh bake decides "floor" from the
// triangle normal the winding implies. The builder's merged world
// collider is double-sided (both windings of every face), so baking from
// PHYSICS COLLIDERS gives the voxelizer an up-facing copy of every floor
// whatever the mirror stack did - and it is exactly the geometry the
// locomotion controller's own rays already walk against.
//
// WHY A RUNTIME LOADER: without the AI Navigation package there is no
// scene component that carries a NavMeshData asset, so the bake is saved
// as an asset under LegaiaGenerated and a tiny Udon behaviour
// (LegaiaNavMeshLoader) registers it with NavMesh.AddNavMeshData at
// Start. The runtime NavMesh.CalculatePath / SamplePosition queries the
// controller makes are all exposed to Udon.
//
// Sources: every non-trigger collider under the built root plus the kit's
// top-level containers (the card table and the camp props are obstacles
// villagers walk around), minus the NPC capsules (they would carve a hole
// under every villager) and anything with a Rigidbody (a pickup is not
// terrain). Idempotent: the pass replaces its container and its asset.

using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.AI;

namespace LegaiaWorld
{
    internal static class LegaiaNavMesh
    {
        internal const string CONTAINER = "navmesh";

        internal static void Remove(GameObject root)
        {
            var old = root.transform.Find(CONTAINER);
            if (old != null)
                Object.DestroyImmediate(old.gameObject);
        }

        /// The NavMeshData asset the pass saved for `sceneName`, or null.
        internal static NavMeshData LoadData(string sceneName)
        {
            return AssetDatabase.LoadAssetAtPath<NavMeshData>(AssetPath(sceneName));
        }

        static string AssetPath(string sceneName)
        {
            return "Assets/LegaiaGenerated/" + sceneName + "/livingtown/navmesh.asset";
        }

        /// Bake the navmesh for the villagers and wire its runtime loader
        /// under `root`. Returns the container (null when nothing walkable
        /// was found).
        internal static GameObject Apply(GameObject root, string sceneName,
            LegaiaLivingTownOptions o, LegaiaSceneSettings settings)
        {
            Remove(root);

            // --- Sources ---------------------------------------------------
            var markups = new List<NavMeshBuildMarkup>();
            foreach (string skip in new[] { "npcs", LegaiaLivingTown.CONTAINER,
                         "teleports", LegaiaWeatherBuilder.CONTAINER })
            {
                var t = root.transform.Find(skip);
                if (t != null)
                    markups.Add(new NavMeshBuildMarkup { root = t, ignoreFromBuild = true });
            }
            var sources = new List<NavMeshBuildSource>();
            var scopes = new List<Transform> { root.transform };
            foreach (string top in new[] { LegaiaCommonPrefabs.CONTAINER, "Legaia_camp_props" })
            {
                var g = GameObject.Find(top);
                if (g != null)
                    scopes.Add(g.transform);
            }
            foreach (var scope in scopes)
            {
                var found = new List<NavMeshBuildSource>();
                NavMeshBuilder.CollectSources(scope, ~0,
                    NavMeshCollectGeometry.PhysicsColliders, 0, markups, found);
                foreach (var s in found)
                {
                    // Pickups and other moving bodies are not terrain.
                    if (s.component != null &&
                        s.component.GetComponentInParent<Rigidbody>() != null)
                        continue;
                    sources.Add(s);
                }
            }
            if (sources.Count == 0)
            {
                Debug.LogWarning("[Legaia] navmesh: no colliders under " + root.name +
                    " - build the world with colliders first; villagers keep " +
                    "their straight-line walks.");
                return null;
            }

            // Bounds: the union of every source collider, padded.
            bool any = false;
            var bounds = new Bounds();
            foreach (var s in sources)
            {
                var c = s.component as Collider;
                if (c == null)
                    continue;
                if (!any)
                {
                    bounds = c.bounds;
                    any = true;
                }
                else
                    bounds.Encapsulate(c.bounds);
            }
            if (!any)
                bounds = new Bounds(root.transform.position, Vector3.one * 200f);
            bounds.Expand(2f);

            // --- Settings --------------------------------------------------
            // Agent type 0 (Humanoid) is what NavMesh.CalculatePath queries
            // by default; the dimensions are the villager's, not a
            // player's: at the 1 m-per-tile export these models stand
            // well under a metre.
            NavMeshBuildSettings bake = NavMesh.GetSettingsByID(0);
            bake.agentRadius = Mathf.Max(0.05f, o.navAgentRadius);
            bake.agentHeight = Mathf.Max(0.2f, o.navAgentHeight);
            bake.agentClimb = Mathf.Max(0.02f, o.navStepHeight);
            bake.agentSlope = Mathf.Clamp(o.navMaxSlope, 5f, 60f);
            // Small enough to keep a shore or a porch, big enough that a
            // 0.3 m furniture ledge is not its own walkable island the
            // ledge-link pass then spends a hop on.
            bake.minRegionArea = 1.5f;
            bake.overrideVoxelSize = true;
            bake.voxelSize = Mathf.Max(0.03f, bake.agentRadius / 3f);
            bake.overrideTileSize = true;
            bake.tileSize = 128;

            var data = NavMeshBuilder.BuildNavMeshData(bake, sources, bounds,
                Vector3.zero, Quaternion.identity);
            if (data == null)
            {
                Debug.LogWarning("[Legaia] navmesh: bake produced no data.");
                return null;
            }

            // --- Asset + loader ------------------------------------------------
            string path = AssetPath(sceneName);
            System.IO.Directory.CreateDirectory(
                System.IO.Path.GetDirectoryName(path).Replace('\\', '/'));
            if (AssetDatabase.LoadAssetAtPath<NavMeshData>(path) != null)
                AssetDatabase.DeleteAsset(path);
            data.name = sceneName + "_navmesh";
            AssetDatabase.CreateAsset(data, path);

            var container = new GameObject(CONTAINER);
            container.transform.SetParent(root.transform, false);
            var loader = LegaiaWorldBuilder.TryAttachUdon(container, "LegaiaNavMeshLoader");
            LegaiaWorldBuilder.SetUdonField(loader, "data", data);
            LegaiaWorldBuilder.SyncUdonProxy(loader);

            BuildLinks(root, container.transform, data, o, settings);

            Debug.Log("[Legaia] navmesh: baked from " + sources.Count +
                " collider source(s) (agent r " + bake.agentRadius + " m, h " +
                bake.agentHeight + " m, step " + bake.agentClimb +
                " m, slope " + bake.agentSlope + " deg) -> " + path +
                (loader == null ? " (no loader: UdonSharp missing, the bake is inert)" : ""));
            return container;
        }

        /// Editor-side registration for the batch checks (and anything else
        /// that wants to query the bake without entering play mode). Pair
        /// with NavMesh.RemoveNavMeshData.
        internal static NavMeshDataInstance Register(NavMeshData data)
        {
            return NavMesh.AddNavMeshData(data);
        }

        // --- Ledge links --------------------------------------------------------
        //
        // A drop the agent cannot climb splits the bake into islands with
        // no route between them: town01's shore sits 1.2 m under the
        // village and two villagers stand down there with nowhere to go at
        // night. A LEDGE LINK is a pair of points, one on each island,
        // close enough together that a villager can jump: the locomotion
        // controller composes walk -> hop -> walk over it (Udon exposes
        // NavMesh.AddLink, but NavMeshLinkData has no constructor extern,
        // so a link cannot be built inside Udon - the route is composed
        // instead, which also keeps the hop under this kit's control).
        //
        // Candidates come off the navmesh's own BOUNDARY edges (an edge
        // that belongs to one triangle is an island's rim), pushed a
        // little inward so they sit on walkable ground. A pair qualifies
        // when the gap is short, the height difference is jumpable, both
        // ends have standing room, the air above the gap is clear, and -
        // the point of the exercise - no walk already connects them.

        internal class Link
        {
            public Vector3 from;
            public Vector3 to;
        }

        /// Every link the last bake installed, in the order of the markers.
        internal static List<Link> LinksOf(GameObject root)
        {
            var list = new List<Link>();
            var container = root.transform.Find(CONTAINER);
            var links = container == null ? null : container.Find("links");
            if (links == null)
                return list;
            foreach (Transform l in links)
            {
                var a = l.Find("from");
                var b = l.Find("to");
                if (a != null && b != null)
                    list.Add(new Link { from = a.position, to = b.position });
            }
            return list;
        }

        /// The link markers themselves, `end` = "from" or "to", in marker
        /// order - what the locomotion controllers are wired with.
        internal static Transform[] LinkEnds(GameObject root, string end)
        {
            var list = new List<Transform>();
            var container = root.transform.Find(CONTAINER);
            var links = container == null ? null : container.Find("links");
            if (links != null)
                foreach (Transform l in links)
                {
                    var t = l.Find(end);
                    if (t != null)
                        list.Add(t);
                }
            return list.ToArray();
        }

        static void BuildLinks(GameObject root, Transform container, NavMeshData data,
            LegaiaLivingTownOptions o, LegaiaSceneSettings settings)
        {
            var found = new List<Link>();
            NavMeshDataInstance inst = NavMesh.AddNavMeshData(data);
            try
            {
                // Hand-pinned pairs first: they are the scene author's
                // verdict and never compete with the automatic ones.
                if (settings != null)
                    foreach (var pair in settings.navLinks)
                        found.Add(new Link
                        {
                            from = SnapOn(root.transform.TransformPoint(
                                LegaiaWorldBuilder.G2U(pair[0]))),
                            to = SnapOn(root.transform.TransformPoint(
                                LegaiaWorldBuilder.G2U(pair[1]))),
                        });
                found.AddRange(FindLedgeLinks(o, found.Count));
            }
            finally
            {
                NavMesh.RemoveNavMeshData(inst);
            }
            if (found.Count == 0)
            {
                Debug.Log("[Legaia] navmesh: no ledge links - every walkable " +
                    "island is either connected or too far apart to jump.");
                return;
            }
            var linksRoot = new GameObject("links");
            linksRoot.transform.SetParent(container, false);
            for (int i = 0; i < found.Count; i++)
            {
                var go = new GameObject("link_" + i);
                go.transform.SetParent(linksRoot.transform, false);
                var a = new GameObject("from");
                a.transform.SetParent(go.transform, false);
                a.transform.position = found[i].from;
                var b = new GameObject("to");
                b.transform.SetParent(go.transform, false);
                b.transform.position = found[i].to;
                Debug.Log("[Legaia] navmesh: ledge link " + i + " " +
                    found[i].from.ToString("F2") + " <-> " + found[i].to.ToString("F2") +
                    " (rise " + (found[i].to.y - found[i].from.y).ToString("F2") +
                    " m, gap " + Flat(found[i].to - found[i].from).ToString("F2") + " m).");
            }
            Debug.Log("[Legaia] navmesh: " + found.Count + " ledge link(s) - " +
                "villagers hop these where no walk connects.");
        }

        static float Flat(Vector3 v)
        {
            v.y = 0f;
            return v.magnitude;
        }

        static Vector3 SnapOn(Vector3 p)
        {
            NavMeshHit hit;
            return NavMesh.SamplePosition(p, out hit, 2f, NavMesh.AllAreas)
                ? hit.position : p;
        }

        /// The automatic pass. Requires the bake to be registered.
        static List<Link> FindLedgeLinks(LegaiaLivingTownOptions o, int already)
        {
            var kept = new List<Link>();
            int budget = Mathf.Max(0, o.navMaxLinks - already);
            if (budget == 0)
                return kept;
            float maxGap = Mathf.Max(0.2f, o.navJumpDistance);
            float maxRise = Mathf.Max(0.2f, o.navJumpHeight);

            NavMeshTriangulation tri = NavMesh.CalculateTriangulation();
            Vector3[] verts = tri.vertices;
            int[] idx = tri.indices;
            if (verts == null || idx == null || idx.Length < 3)
                return kept;

            // Weld: a tiled bake repeats a vertex per tile, so raw indices
            // never share an edge across the seam and every edge would look
            // like a boundary.
            var weld = new Dictionary<long, int>();
            var wid = new int[verts.Length];
            for (int i = 0; i < verts.Length; i++)
            {
                long key = Key(verts[i]);
                int w;
                if (!weld.TryGetValue(key, out w))
                {
                    w = weld.Count;
                    weld[key] = w;
                }
                wid[i] = w;
            }

            // Edge use counts, and one candidate point per boundary edge:
            // the edge midpoint pushed toward the triangle it belongs to.
            var uses = new Dictionary<long, int>();
            var mid = new Dictionary<long, Vector3>();
            var inward = new Dictionary<long, Vector3>();
            for (int t = 0; t + 2 < idx.Length; t += 3)
            {
                Vector3 c = (verts[idx[t]] + verts[idx[t + 1]] + verts[idx[t + 2]]) / 3f;
                for (int e = 0; e < 3; e++)
                {
                    int a = idx[t + e], b = idx[t + (e + 1) % 3];
                    long k = EdgeKey(wid[a], wid[b]);
                    int n;
                    uses.TryGetValue(k, out n);
                    uses[k] = n + 1;
                    if (n == 0)
                    {
                        Vector3 m = (verts[a] + verts[b]) * 0.5f;
                        mid[k] = m;
                        Vector3 dir = c - m;
                        dir.y = 0f;
                        inward[k] = dir.sqrMagnitude > 1e-6f
                            ? dir.normalized : Vector3.zero;
                    }
                }
            }

            // Two positions per sample. `rims` is the rim itself - the
            // navmesh polygon's own edge - and that is what the jump
            // distance is measured between, because the LEDGE is what a
            // villager jumps. `pts` is the same point pushed a little onto
            // the mesh, which is where the hop actually starts and lands
            // (SamplePosition on a rim is a coin toss). Measuring the gap
            // between the PUSHED points instead adds half a metre of our
            // own sampling margin to every ledge in the scene, and that
            // alone hid town01's shore bank: the two rims sit 0.6 m apart
            // and the pushed pair read 1.20 m, just past the cutoff.
            var pts = new List<Vector3>();
            var rims = new List<Vector3>();
            var seen = new HashSet<long>();
            foreach (var kv in uses)
            {
                if (kv.Value != 1)
                    continue;
                Vector3 rim = mid[kv.Key];
                Vector3 p = rim + inward[kv.Key] * 0.25f;
                NavMeshHit onMesh;
                if (!NavMesh.SamplePosition(p, out onMesh, 0.5f, NavMesh.AllAreas))
                    continue;
                p = onMesh.position;
                // One candidate per 0.4 m cell: a rim is sampled far more
                // finely than a jump needs.
                long cell = Key(p * 2.5f);
                if (!seen.Add(cell))
                    continue;
                pts.Add(p);
                rims.Add(rim);
            }

            // Bucket by a grid the size of the longest jump, so each point
            // only meets its neighbours.
            var grid = new Dictionary<long, List<int>>();
            for (int i = 0; i < pts.Count; i++)
            {
                long cell = Cell(pts[i], maxGap);
                List<int> bucket;
                if (!grid.TryGetValue(cell, out bucket))
                {
                    bucket = new List<int>();
                    grid[cell] = bucket;
                }
                bucket.Add(i);
            }

            var cand = new List<Link>();
            var costs = new List<float>();
            var probePath = new NavMeshPath();
            int probes = 0;
            int rejGap = 0, rejRise = 0, rejFlat = 0, rejBlocked = 0, rejWalkable = 0;
            var nearMiss = new List<Link>();
            var nearMissGap = new List<float>();
            for (int i = 0; i < pts.Count; i++)
            {
                Vector3 a = pts[i];
                long baseCell = Cell(a, maxGap);
                for (int dx = -1; dx <= 1 && probes < 20000; dx++)
                    for (int dz = -1; dz <= 1 && probes < 20000; dz++)
                    {
                        List<int> bucket;
                        if (!grid.TryGetValue(baseCell + dx * 1000003L + dz, out bucket))
                            continue;
                        foreach (int j in bucket)
                        {
                            if (j <= i)
                                continue;
                            Vector3 b = pts[j];
                            // Measured rim to rim (the ledge); jumped
                            // pushed-point to pushed-point (the mesh).
                            float gap = Flat(rims[j] - rims[i]);
                            float rise = Mathf.Abs(b.y - a.y);
                            // NO lower bound worth speaking of. A vertical
                            // bank puts the two rims almost exactly above
                            // one another - town01's shore reads as 0.03 to
                            // 0.19 m across and 1.7 m down - so a "the two
                            // ends must be a stride apart" floor threw away
                            // every climb the links exist for. Two points
                            // that are really the same ground are caught by
                            // the level-with-each-other and already-walkable
                            // tests below instead.
                            if (gap < 0.02f || gap > maxGap)
                            {
                                rejGap++;
                                // Diagnostic: the narrowest BANK-sized gaps
                                // the jump distance just misses. When a
                                // scene leaves an island stranded, these
                                // rows say what navJumpDistance it needs.
                                if (gap <= maxGap + 3f && rise > 0.3f &&
                                    rise < 4f && nearMiss.Count < 4000)
                                {
                                    nearMiss.Add(new Link { from = a, to = b });
                                    nearMissGap.Add(gap);
                                }
                                continue;
                            }
                            if (rise > maxRise)
                            {
                                rejRise++;
                                continue;
                            }
                            // A ledge, not a seam in the same floor: a pair
                            // level with each other is either already
                            // walkable or a doorway, never a jump.
                            if (rise < 0.25f)
                            {
                                rejFlat++;
                                continue;
                            }
                            if (!Clear(a, b, maxRise))
                            {
                                rejBlocked++;
                                continue;
                            }
                            probes++;
                            // Already walkable? Then it is no link.
                            NavMesh.CalculatePath(a, b, NavMesh.AllAreas, probePath);
                            if (probePath.status == NavMeshPathStatus.PathComplete)
                            {
                                rejWalkable++;
                                continue;
                            }
                            cand.Add(new Link { from = a, to = b });
                            costs.Add(gap + rise);
                        }
                    }
            }

            // ONE LINK PER PAIR OF ISLANDS, cheapest of that pair. Ranking
            // by cost alone starved the link that mattered: town01 offers
            // 1342 jumpable pairs, and the shortest are all half-metre
            // steps inside the interior rooms, so a flat "keep the cheapest
            // eight" spent every slot on those and left the shore - the one
            // island with villagers stranded on it - unconnected. An island
            // is identified by walking to a representative point.
            // Kruskal over the islands: cheapest link first, kept only when
            // it JOINS two groups that hopping cannot already get between.
            // A flat "cheapest N" spent every slot on half-metre steps
            // inside the interior rooms and left the shore - the one island
            // with villagers stranded on it - unreached; a spanning
            // structure connects everything that can be connected and never
            // stacks two hops onto the same join.
            var reps = new List<Vector3>();
            var repOf = new Dictionary<long, int>();
            var group = new List<int>();
            var order = new List<int>();
            for (int i = 0; i < cand.Count; i++)
                order.Add(i);
            order.Sort((x, y) => costs[x].CompareTo(costs[y]));
            foreach (int i in order)
            {
                if (kept.Count >= budget)
                    break;
                int ia = IslandOf(cand[i].from, reps, repOf, probePath);
                int ib = IslandOf(cand[i].to, reps, repOf, probePath);
                while (group.Count < reps.Count)
                    group.Add(group.Count);
                int ra = Root(group, ia), rb = Root(group, ib);
                if (ra == rb)
                    continue;
                group[rb] = ra;
                kept.Add(cand[i]);
            }
            Debug.Log("[Legaia] navmesh: " + pts.Count + " rim sample(s), " +
                cand.Count + " jumpable pair(s) with no walk between them, " +
                reps.Count + " walkable island(s), " + kept.Count +
                " kept - one per island pair (max jump " + maxGap +
                " m across / " + maxRise + " m up). Rejected: " + rejGap +
                " too far apart, " + rejRise + " too tall, " + rejFlat +
                " level with each other, " + rejBlocked +
                " blocked overhead, " + rejWalkable +
                " already walkable.");
            var missOrder = new List<int>();
            for (int i = 0; i < nearMiss.Count; i++)
                missOrder.Add(i);
            missOrder.Sort((x, y) => nearMissGap[x].CompareTo(nearMissGap[y]));
            int shown = 0;
            var missSeen = new HashSet<long>();
            foreach (int i in missOrder)
            {
                if (shown >= 6)
                    break;
                // One row per stretch of rim, not per sample on it.
                if (!missSeen.Add(Cell(nearMiss[i].from, 4f)))
                    continue;
                shown++;
                Debug.Log("[Legaia] navmesh: near miss - " +
                    nearMiss[i].from.ToString("F2") + " <-> " +
                    nearMiss[i].to.ToString("F2") + " gap " +
                    nearMissGap[i].ToString("F2") + " m, rise " +
                    (nearMiss[i].to.y - nearMiss[i].from.y).ToString("F2") + " m.");
            }
            return kept;
        }

        static int Root(List<int> group, int i)
        {
            while (group[i] != i)
                i = group[i];
            return i;
        }

        /// Which walkable island `p` belongs to: the first representative
        /// point a plain walk reaches, or a new one. The representative
        /// list stays short (a village, a shore, one per detached interior
        /// room), so this is a handful of path queries per candidate.
        static int IslandOf(Vector3 p, List<Vector3> reps,
            Dictionary<long, int> memo, NavMeshPath scratch)
        {
            long key = Key(p);
            int id;
            if (memo.TryGetValue(key, out id))
                return id;
            for (int i = 0; i < reps.Count; i++)
            {
                NavMesh.CalculatePath(p, reps[i], NavMesh.AllAreas, scratch);
                if (scratch.status == NavMeshPathStatus.PathComplete)
                {
                    memo[key] = i;
                    return i;
                }
            }
            reps.Add(p);
            id = reps.Count - 1;
            memo[key] = id;
            return id;
        }

        /// Clear air over the gap: the villager arcs from one ledge to the
        /// other without passing through anything. The line runs at the
        /// APEX of the arc, not at knee height - a climb's whole point is
        /// that the bank face is between the two ends.
        ///
        /// There is deliberately NO standing-room test at the ends. Both
        /// ends were snapped onto the navmesh, and the bake has already
        /// answered "can a villager stand here" with the agent's own radius
        /// and height. Re-asking it with a sphere rejects exactly the
        /// places links exist for: a ledge has a wall against it by
        /// definition, and a 0.25 m sphere beside town01's shore bank hit
        /// it every time - which is why the two stranded villagers stayed
        /// stranded through several rounds of tuning.
        static bool Clear(Vector3 a, Vector3 b, float rise)
        {
            float top = Mathf.Max(a.y, b.y) + rise * 0.5f + 0.3f;
            Vector3 ha = new Vector3(a.x, top, a.z);
            Vector3 hb = new Vector3(b.x, top, b.z);
            return !Physics.Linecast(ha, hb, ~0, QueryTriggerInteraction.Ignore);
        }

        static long Key(Vector3 p)
        {
            long x = Mathf.RoundToInt(p.x * 100f);
            long y = Mathf.RoundToInt(p.y * 100f);
            long z = Mathf.RoundToInt(p.z * 100f);
            return x * 73856093L ^ y * 19349663L ^ z * 83492791L;
        }

        static long Cell(Vector3 p, float size)
        {
            long x = Mathf.FloorToInt(p.x / size);
            long z = Mathf.FloorToInt(p.z / size);
            return x * 1000003L + z;
        }

        static long EdgeKey(int a, int b)
        {
            int lo = a < b ? a : b;
            int hi = a < b ? b : a;
            return (long)lo * 1000003L + hi;
        }

        /// True when a complete navmesh route exists between two world
        /// points, each snapped to the mesh within `snap` metres.
        /// Reachable by walking, or by a CHAIN of walks and ledge hops -
        /// the same greedy composition LegaiaNpcWander does at runtime, and
        /// bounded by the same hop count, so a home this says is reachable
        /// is one a villager can actually walk to. `hopped` says whether
        /// any hop was needed.
        internal const int MAX_HOPS = 3;

        internal static bool ReachableWithLinks(Vector3 from, Vector3 to, float snap,
            List<Link> links, out bool hopped, out string why)
        {
            hopped = false;
            if (Reachable(from, to, snap, out why))
                return true;
            if (links == null || links.Count == 0)
            {
                why += "; no ledge link bridges it either";
                return false;
            }
            var used = new bool[links.Count];
            Vector3 cur = from;
            for (int step = 0; step < MAX_HOPS; step++)
            {
                int best = -1;
                bool bestFlip = false;
                float bestScore = float.MaxValue;
                for (int i = 0; i < links.Count; i++)
                {
                    if (used[i])
                        continue;
                    for (int dir = 0; dir < 2; dir++)
                    {
                        Vector3 p = dir == 0 ? links[i].from : links[i].to;
                        Vector3 q = dir == 0 ? links[i].to : links[i].from;
                        float score = Vector3.Distance(q, to);
                        if (score >= bestScore)
                            continue;
                        string leg;
                        if (!Reachable(cur, p, snap, out leg))
                            continue;
                        bestScore = score;
                        best = i;
                        bestFlip = dir == 1;
                    }
                }
                if (best < 0)
                    break;
                used[best] = true;
                hopped = true;
                cur = bestFlip ? links[best].from : links[best].to;
                string rest;
                if (Reachable(cur, to, snap, out rest))
                {
                    why = null;
                    return true;
                }
            }
            hopped = false;
            why += "; no chain of up to " + MAX_HOPS + " ledge hop(s) bridges it either";
            return false;
        }

        internal static bool Reachable(Vector3 from, Vector3 to, float snap,
            out string why)
        {
            NavMeshHit a, b;
            if (!NavMesh.SamplePosition(from, out a, snap, NavMesh.AllAreas))
            {
                why = "start " + from + " is not within " + snap + " m of the navmesh";
                return false;
            }
            if (!NavMesh.SamplePosition(to, out b, snap, NavMesh.AllAreas))
            {
                why = "goal " + to + " is not within " + snap + " m of the navmesh";
                return false;
            }
            var path = new NavMeshPath();
            NavMesh.CalculatePath(a.position, b.position, NavMesh.AllAreas, path);
            if (path.status != NavMeshPathStatus.PathComplete)
            {
                why = "path " + a.position + " -> " + b.position + " is " + path.status;
                var corners = path.corners;
                if (corners != null && corners.Length > 0)
                {
                    Vector3 end = corners[corners.Length - 1];
                    why += "; it gets as far as " + end + " (" +
                           Vector3.Distance(end, b.position).ToString("0.0") +
                           " m short) over " + corners.Length + " corner(s)";
                }
                return false;
            }
            why = null;
            return true;
        }
    }
}
