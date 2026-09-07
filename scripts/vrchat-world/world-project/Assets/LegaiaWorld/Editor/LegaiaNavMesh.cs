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
            LegaiaLivingTownOptions o)
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
            NavMeshBuildSettings settings = NavMesh.GetSettingsByID(0);
            settings.agentRadius = Mathf.Max(0.05f, o.navAgentRadius);
            settings.agentHeight = Mathf.Max(0.2f, o.navAgentHeight);
            settings.agentClimb = Mathf.Max(0.02f, o.navStepHeight);
            settings.agentSlope = Mathf.Clamp(o.navMaxSlope, 5f, 60f);
            settings.minRegionArea = 0.4f;
            settings.overrideVoxelSize = true;
            settings.voxelSize = Mathf.Max(0.03f, settings.agentRadius / 3f);
            settings.overrideTileSize = true;
            settings.tileSize = 128;

            var data = NavMeshBuilder.BuildNavMeshData(settings, sources, bounds,
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

            Debug.Log("[Legaia] navmesh: baked from " + sources.Count +
                " collider source(s) (agent r " + settings.agentRadius + " m, h " +
                settings.agentHeight + " m, step " + settings.agentClimb +
                " m, slope " + settings.agentSlope + " deg) -> " + path +
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

        /// True when a complete navmesh route exists between two world
        /// points, each snapped to the mesh within `snap` metres.
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
