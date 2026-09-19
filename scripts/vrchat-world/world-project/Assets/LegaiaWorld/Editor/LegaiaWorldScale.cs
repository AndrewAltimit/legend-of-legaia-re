// Uniform world scale: the whole built scene - the Legaia root and every
// kit container at the origin - grown about the origin by one factor,
// the scene settings' `world_scale`. VR avatars stand a good half again
// taller than the 1 m-per-tile export assumed, and everything (the
// village, the villagers, the furniture, the cabinets) is meant to grow
// together so no relationship inside the scene changes.
//
// WHEN it runs is the whole design. Every builder pass measures the
// world in metres against avatar-sized rules - a window glow is a
// 0.5..4.5 m span, a poster wants a wall within 7 m, a villager's door
// stands 0.8 m out, the navmesh snap is 1.2 m, the grass scatters per
// square metre - so the passes ALL run at 1x (the living town included)
// and the scale goes on LAST, as one transform on each top-level
// object. Positions stored in the settings file are local under those
// objects and never change; the transforms above them do.
//
// Two kinds of value are not transforms and get their own treatment:
//
// - the navmesh: NavMeshData is world-space geometry, so after the
//   scale LegaiaNavMesh.Rebake bakes it again where the floors now are
//   (with the agent grown by the same factor - the villagers are) and
//   re-points the loader. Only the data: the container, its ledge links
//   and the stations are transforms wired into the villagers by
//   reference, and they scaled with the root. (Running the living town
//   itself in the scaled frame was tried and rejected: its metre
//   constants then refuse door stands and prop stations.)
// - distances: light ranges, audio min/max, reverb zones, fog, the
//   metre / metre-per-second fields the wander and living-town passes
//   wrote into the villagers, and particle systems (switched to
//   hierarchy scaling so a campfire flame grows with its logs).
//   Rescale() multiplies those by the change in scale, so it is exact
//   in both directions.
//
// Idempotent by TARGET: Apply(root, s) reads the scale the scene is at
// from the root's Y scale and applies only the difference, so a rebuild,
// "Apply enhancements" and the container-only rebuild all wrap their
// work in Unapply .. Apply and the passes between always see 1x.

using System.Collections.Generic;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaWorldScale
    {
        /// The top-level objects the scale goes on: the built root and
        /// the kit containers that sit at the origin beside it.
        internal static readonly string[] CONTAINERS =
        {
            LegaiaCommonPrefabs.CONTAINER, LegaiaCampProps.CONTAINER,
            "Legaia_night_torches", "Legaia_equipment",
        };

        /// Metre / metre-per-second fields on the villagers' UdonSharp
        /// behaviours, written by the passes that ran at 1x.
        static readonly Dictionary<string, string[]> UDON_METRE_FIELDS =
            new Dictionary<string, string[]>
            {
                { "LegaiaWorld.LegaiaNpcWander", new[]
                    { "speed", "walkSpeed", "radius", "wallClearance", "arriveRadius",
                      "probeDistance", "navSnapRadius", "cornerRadius", "hopApex",
                      "gaitBob", "gaitStride", "walkStride" } },
                { "LegaiaWorld.LegaiaNpcBrain", new[]
                    { "indoorRadius", "outdoorRadius", "doorArriveRadius",
                      "followSpacing", "noticeDistance" } },
            };

        /// The scale the scene is at: the root's Y scale (X carries the
        /// mirror sign), 1 for a scene never scaled.
        internal static float Current(GameObject root)
        {
            if (root == null)
                return 1f;
            float y = Mathf.Abs(root.transform.localScale.y);
            return y > 1e-4f ? y : 1f;
        }

        static List<GameObject> TopLevel(GameObject root)
        {
            var list = new List<GameObject>();
            if (root != null)
                list.Add(root);
            foreach (string name in CONTAINERS)
            {
                var go = GameObject.Find(name);
                if (go != null && go.transform.parent == null)
                    list.Add(go);
            }
            return list;
        }

        /// Bring the scene to scale `target` from wherever it is now.
        internal static void Apply(GameObject root, float target)
        {
            if (root == null)
                return;
            target = target > 1e-3f ? target : 1f;
            float current = Current(root);

            // Each top-level object is judged by ITS OWN scale, not the
            // root's: a container rebuilt since the last scale (the
            // container-only path, a check's rebuild) stands at 1x beside
            // a root already at the target, and must still be brought up.
            int changed = 0;
            foreach (var go in TopLevel(root))
            {
                var s = go.transform.localScale;
                float own = Mathf.Abs(s.y) > 1e-4f ? Mathf.Abs(s.y) : 1f;
                if (Mathf.Abs(own - target) < 1e-5f)
                    continue;
                Undo.RecordObject(go.transform, "Legaia world scale");
                float sx = go == root ? Mathf.Sign(s.x == 0f ? 1f : s.x) : 1f;
                go.transform.localScale = new Vector3(sx * target, target, target);
                Rescale(go, target / own);
                changed++;
            }
            if (changed == 0)
                return;

            // Fog is scene-wide: it follows the root's change only.
            if (RenderSettings.fog && Mathf.Abs(current - target) > 1e-5f)
            {
                float factor = target / current;
                RenderSettings.fogStartDistance *= factor;
                RenderSettings.fogEndDistance *= factor;
                RenderSettings.fogDensity /= factor;
            }
            // The physics scene does not follow a transform edit until a
            // step or a sync: the raycasts (ground probes, the checks'
            // floor tests) that run next would otherwise hit the colliders
            // where they stood before the scale.
            Physics.SyncTransforms();
            Debug.Log("[Legaia] world scale " + current + " -> " + target + " on " +
                TopLevel(root).Count + " top-level object(s).");
        }

        internal static void Unapply(GameObject root)
        {
            Apply(root, 1f);
        }

        /// Everything under `go` that is a distance rather than a
        /// transform, multiplied by `factor`.
        static void Rescale(GameObject go, float factor)
        {
            foreach (var l in go.GetComponentsInChildren<Light>(true))
                if (l.type != LightType.Directional)
                {
                    Undo.RecordObject(l, "Legaia world scale");
                    l.range *= factor;
                }
            foreach (var a in go.GetComponentsInChildren<AudioSource>(true))
            {
                Undo.RecordObject(a, "Legaia world scale");
                a.minDistance *= factor;
                a.maxDistance *= factor;
            }
            foreach (var z in go.GetComponentsInChildren<AudioReverbZone>(true))
            {
                Undo.RecordObject(z, "Legaia world scale");
                z.minDistance *= factor;
                z.maxDistance *= factor;
            }
            foreach (var ps in go.GetComponentsInChildren<ParticleSystem>(true))
            {
                var main = ps.main;
                if (main.scalingMode != ParticleSystemScalingMode.Hierarchy)
                {
                    Undo.RecordObject(ps, "Legaia world scale");
                    main.scalingMode = ParticleSystemScalingMode.Hierarchy;
                }
            }
            // Interact reach: an UdonBehaviour's `proximity` (and a
            // pickup's) is metres from the player, not a transform, so a
            // scaled cabinet's buttons keep their 1x reach until this
            // grows it - what "the buttons have too short a range" was.
            foreach (string typeName in new[] { "VRC.Udon.UdonBehaviour",
                                                "VRC.SDK3.Components.VRCPickup" })
            {
                var type = LegaiaWorldBuilder.FindType(typeName);
                if (type == null)
                    continue;
                var field = type.GetField("proximity");
                var prop = field == null ? type.GetProperty("proximity") : null;
                if (field == null && (prop == null || !prop.CanWrite))
                {
                    Debug.LogWarning("[Legaia] world scale: " + typeName + " has no " +
                        "proximity member - interact reach left at 1x (SDK drift?).");
                    continue;
                }
                int reached = 0;
                foreach (var c in go.GetComponentsInChildren(type, true))
                {
                    var comp = c as Component;
                    float v = (float)(field != null ? field.GetValue(comp) : prop.GetValue(comp));
                    Undo.RecordObject(comp, "Legaia world scale");
                    if (field != null)
                        field.SetValue(comp, v * factor);
                    else
                        prop.SetValue(comp, v * factor);
                    EditorUtility.SetDirty(comp);
                    reached++;
                }
                if (reached > 0)
                    Debug.Log("[Legaia] world scale: interact reach x" + factor.ToString("0.###") +
                        " on " + reached + " " + typeName + "(s) under " + go.name + ".");
            }
            foreach (var kv in UDON_METRE_FIELDS)
            {
                var type = LegaiaWorldBuilder.FindType(kv.Key);
                if (type == null)
                    continue;
                foreach (var c in go.GetComponentsInChildren(type, true))
                {
                    var comp = c as Component;
                    bool changed = false;
                    foreach (string field in kv.Value)
                    {
                        var f = type.GetField(field);
                        if (f == null || f.FieldType != typeof(float))
                            continue;
                        float v = (float)f.GetValue(comp);
                        if (v == 0f)
                            continue;
                        LegaiaWorldBuilder.SetUdonField(comp, field, v * factor);
                        changed = true;
                    }
                    if (changed)
                        LegaiaWorldBuilder.SyncUdonProxy(comp);
                }
            }
        }

        /// The living-town options as the scaled frame sees them: every
        /// metre-valued knob times `s`. Used for the navmesh re-bake (the
        /// agent is `s` bigger, like the villagers). A copy; the builder's
        /// own object is never touched.
        internal static LegaiaLivingTownOptions ScaledOptions(LegaiaLivingTownOptions o, float s)
        {
            var c = o.Clone();
            if (Mathf.Abs(s - 1f) < 1e-5f)
                return c;
            c.chatRingRadius *= s;
            c.propStandDistance *= s;
            c.walkSpeed *= s;
            c.interiorDistance *= s;
            c.navAgentRadius *= s;
            c.navAgentHeight *= s;
            c.navStepHeight *= s;
            c.doorStandDistance *= s;
            c.doorLeafRadius *= s;
            c.navJumpHeight *= s;
            c.navJumpDistance *= s;
            return c;
        }

        /// The builder's tail: scale the finished 1x scene to the
        /// settings' factor, then re-bake the navmesh data where the
        /// floors now are. `livingTown` supplies the agent dimensions
        /// (null = the defaults). No-op at scale 1.
        internal static void Finish(GameObject root, string sceneName,
            LegaiaSceneSettings settings, LegaiaLivingTownOptions livingTown)
        {
            float s = settings != null ? settings.worldScale : 1f;
            if (root == null || Mathf.Abs(s - 1f) < 1e-5f)
            {
                Apply(root, 1f);
                return;
            }
            Apply(root, s);
            var o = ScaledOptions(livingTown ?? new LegaiaLivingTownOptions(), s);
            LegaiaNavMesh.Rebake(root, sceneName, o);
        }
    }
}
