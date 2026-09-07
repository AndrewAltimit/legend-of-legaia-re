// The bounty layer of the living town: a weapon hitbox on every villager
// (LegaiaNpcHitbox), the coin-drop pool the brains spawn into when one is
// struck down (LegaiaCoinDrops / LegaiaCoinDrop), and the wallet link.
//
// The living-town pass calls Apply once the brains and the director are
// wired, and Remove before it rebuilds. What lands where:
//
//   <root>/npcs/<villager>/hitbox   a trigger capsule wrapped round the
//                                   villager's rendered body, carrying a
//                                   kinematic Rigidbody (Unity reports no
//                                   trigger crossing unless one side has
//                                   a body, and the villager is moved by a
//                                   controller, not by physics)
//   <root>/living_town/coins        the pool: twelve coin objects and the
//                                   LegaiaCoinDrops behaviour every brain's
//                                   loose `bounty` link points at
//
// SCALE. The built scene root is MIRRORED (the PSX-to-Unity chirality
// flip), and a collider under a mirror is a physics hazard - so both the
// hitbox and the pool cancel their parent's lossy scale and live in unit
// space, exactly as the living-props pass does for the fishing spots.
// The capsule is then measured from the villager's WORLD bounds and
// mapped back through the hitbox's own transform, so it fits whatever
// export scale the scene was built at.
//
// Per-scene overrides live in the settings file's `living_town` block:
// `"bounty": false` turns the layer off, `"respawn_seconds"` sets how
// long a slain villager stays down, `"coin_drop": [min, max]` sets what
// one is worth. Defaults: on, 120 s, 5-15 coins.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaBounty
    {
        internal const string POOL = "coins";
        internal const string HITBOX = "hitbox";

        const int POOL_SIZE = 12;
        const float DEFAULT_RESPAWN = 120f;
        const int DEFAULT_COIN_MIN = 5;
        const int DEFAULT_COIN_MAX = 15;
        const float COIN_RADIUS = 0.12f;
        const float COIN_THICKNESS = 0.012f;
        const float DROP_LIFE = 90f;

        /// Build the hitboxes + pool under `container` and wire every brain's
        /// `bounty` field to the pool.
        internal static void Apply(GameObject root, Transform container,
            List<Component> brains, LegaiaLivingTownOptions o, LegaiaSceneSettings settings)
        {
            if (root == null || container == null || brains == null)
                return;
            if (settings != null && !settings.bounty)
            {
                Debug.Log("[Legaia] bounty: off for this scene " +
                    "(living_town.bounty = false) - villagers cannot be struck.");
                return;
            }
            float respawn = settings != null && settings.bountyRespawnSeconds > 0f
                ? settings.bountyRespawnSeconds : DEFAULT_RESPAWN;
            int coinMin = settings != null && settings.bountyCoinMin >= 0
                ? settings.bountyCoinMin : DEFAULT_COIN_MIN;
            int coinMax = settings != null && settings.bountyCoinMax >= 0
                ? settings.bountyCoinMax : DEFAULT_COIN_MAX;
            if (coinMax < coinMin)
                coinMax = coinMin;

            string sceneName = root.name.StartsWith("Legaia_")
                ? root.name.Substring("Legaia_".Length) : "scene";
            string genDir = "Assets/LegaiaGenerated/" + sceneName + "/bounty";
            Directory.CreateDirectory(genDir);

            var wallet = LegaiaCommonPrefabs.FindWallet();
            var pool = BuildPool(container, genDir, wallet);
            if (pool == null)
            {
                Debug.LogWarning("[Legaia] bounty: LegaiaCoinDrops is not " +
                    "compiled (VRChat SDK / UdonSharp missing?) - no coins " +
                    "will drop.");
                return;
            }

            int hitboxes = 0;
            foreach (var brain in brains)
            {
                if (brain == null)
                    continue;
                if (BuildHitbox(brain, coinMin, coinMax))
                    hitboxes++;
                LegaiaWorldBuilder.SetUdonField(brain, "bounty", pool);
                LegaiaWorldBuilder.SetUdonField(brain, "respawnSeconds", respawn);
                LegaiaWorldBuilder.SyncUdonProxy(brain);
            }

            Debug.Log("[Legaia] bounty: " + hitboxes + " villager hitbox(es), " +
                POOL_SIZE + " pooled coin drop(s) worth " + coinMin + "-" +
                coinMax + " coins, respawn " + respawn.ToString("0") + " s" +
                (wallet != null ? ", wallet wired" : ", wallet resolved at runtime") +
                ".");
        }

        /// Strip what Apply put on the NPC instances (the pool goes with the
        /// living-town container).
        internal static void Remove(GameObject root)
        {
            if (root == null)
                return;
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return;
            foreach (Transform npc in npcRoot)
            {
                var old = npc.Find(HITBOX);
                if (old != null)
                    Undo.DestroyObjectImmediate(old.gameObject);
            }
        }

        // --- the pool ---------------------------------------------------------

        static Component BuildPool(Transform container, string genDir, Component wallet)
        {
            var poolGo = new GameObject(POOL);
            poolGo.transform.SetParent(container, false);
            Unmirror(poolGo.transform);

            var gold = LegaiaCampProps.EnsureMat(genDir, "coin_gold", "Unlit/Color",
                new Color(0.98f, 0.80f, 0.22f));

            var drops = new List<Component>();
            for (int i = 0; i < POOL_SIZE; i++)
            {
                var go = new GameObject("coin_" + i);
                go.transform.SetParent(poolGo.transform, false);

                // The coin face: Unity's cylinder is 2 units tall along +Y,
                // so a quarter turn about X stands it up like a coin on edge.
                var vis = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
                vis.name = "face";
                Object.DestroyImmediate(vis.GetComponent<Collider>());
                vis.transform.SetParent(go.transform, false);
                vis.transform.localRotation = Quaternion.Euler(90f, 0f, 0f);
                vis.transform.localScale = new Vector3(
                    COIN_RADIUS, COIN_THICKNESS, COIN_RADIUS);
                var mr = vis.GetComponent<MeshRenderer>();
                mr.sharedMaterial = gold;
                mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
                mr.receiveShadows = false;

                // Trigger, not solid: a coin lying in a doorway must not be
                // something a player walks into. VRChat's interact ray hits
                // triggers all the same.
                var box = go.AddComponent<BoxCollider>();
                box.isTrigger = true;
                box.size = Vector3.one * (COIN_RADIUS + 0.06f);

                var drop = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaCoinDrop");
                if (drop == null)
                    continue;
                LegaiaWorldBuilder.SetUdonField(drop, "wallet", wallet);
                LegaiaWorldBuilder.SetUdonField(drop, "visual", mr);
                LegaiaWorldBuilder.SetUdonField(drop, "hitBox", box);
                LegaiaWorldBuilder.SyncUdonProxy(drop);
                drops.Add(drop);

                // Out of sight until something dies (Start does this too,
                // but an editor scene should not show twelve floating coins).
                mr.enabled = false;
                box.enabled = false;
            }

            var pool = LegaiaWorldBuilder.TryAttachUdon(poolGo, "LegaiaCoinDrops");
            if (pool == null)
                return null;
            LegaiaWorldBuilder.SetUdonField(pool, "drops",
                Typed(drops, "LegaiaCoinDrop"));
            LegaiaWorldBuilder.SetUdonField(pool, "lifeSeconds", DROP_LIFE);
            LegaiaWorldBuilder.SyncUdonProxy(pool);
            return pool;
        }

        // --- one villager's hitbox -------------------------------------------

        static bool BuildHitbox(Component brain, int coinMin, int coinMax)
        {
            Transform npc = brain.transform;
            var old = npc.Find(HITBOX);
            if (old != null)
                Object.DestroyImmediate(old.gameObject);

            Bounds b;
            if (!WorldBounds(npc, out b))
                return false;

            var go = new GameObject(HITBOX);
            go.transform.SetParent(npc, false);
            go.transform.localPosition = Vector3.zero;
            go.transform.localRotation = Quaternion.identity;
            Unmirror(go.transform);

            var cap = go.AddComponent<CapsuleCollider>();
            cap.isTrigger = true;
            cap.direction = 1; // +Y, a standing body
            cap.center = go.transform.InverseTransformPoint(b.center);
            cap.height = Mathf.Clamp(b.size.y, 0.6f, 4f);
            cap.radius = Mathf.Clamp(
                Mathf.Max(b.size.x, b.size.z) * 0.5f, 0.15f, 1.2f);

            // Unity only reports a trigger crossing when one side carries a
            // Rigidbody, and the villager is driven by a controller rather
            // than by physics - so the hitbox brings its own, kinematic.
            var rb = go.AddComponent<Rigidbody>();
            rb.isKinematic = true;
            rb.useGravity = false;

            var hb = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaNpcHitbox");
            if (hb == null)
                return false;
            LegaiaWorldBuilder.SetUdonField(hb, "brain", brain);
            LegaiaWorldBuilder.SetUdonField(hb, "npcRoot", npc);
            LegaiaWorldBuilder.SetUdonField(hb, "coinsMin", coinMin);
            LegaiaWorldBuilder.SetUdonField(hb, "coinsMax", coinMax);
            LegaiaWorldBuilder.SyncUdonProxy(hb);
            return true;
        }

        // --- helpers ----------------------------------------------------------

        /// Cancel the parent chain's scale so this object (and the colliders
        /// on it) live in unit world space - the built root is mirrored, and
        /// a collider under a mirror is a physics hazard.
        static void Unmirror(Transform t)
        {
            Vector3 ls = t.parent != null ? t.parent.lossyScale : Vector3.one;
            t.localScale = new Vector3(Inv(ls.x), Inv(ls.y), Inv(ls.z));
        }

        static float Inv(float v)
        {
            return Mathf.Abs(v) < 1e-6f ? 1f : 1f / v;
        }

        static bool WorldBounds(Transform t, out Bounds b)
        {
            b = new Bounds(t.position, Vector3.zero);
            var rs = t.GetComponentsInChildren<Renderer>(true);
            bool any = false;
            foreach (var r in rs)
            {
                // Measure the BODY. A speech bubble floats over the head
                // and a carried item hangs off the hands; either one would
                // drag the capsule out of shape.
                if (Under(r.transform, t, HITBOX) ||
                    Under(r.transform, t, "speech_bubble") ||
                    Under(r.transform, t, "carry"))
                    continue;
                if (!any)
                {
                    b = r.bounds;
                    any = true;
                }
                else
                {
                    b.Encapsulate(r.bounds);
                }
            }
            return any;
        }

        /// Is `t` inside a child of `root` called `name`?
        static bool Under(Transform t, Transform root, string name)
        {
            for (var u = t; u != null && u != root; u = u.parent)
                if (u.name == name)
                    return true;
            return false;
        }

        /// A typed array (an object[] never deserializes onto an Udon
        /// variable) built without a compile-time reference to the U# type.
        static System.Array Typed(List<Component> comps, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + typeName);
            if (t == null)
                return null;
            var arr = System.Array.CreateInstance(t, comps.Count);
            for (int i = 0; i < comps.Count; i++)
                arr.SetValue(comps[i], i);
            return arr;
        }
    }
}
