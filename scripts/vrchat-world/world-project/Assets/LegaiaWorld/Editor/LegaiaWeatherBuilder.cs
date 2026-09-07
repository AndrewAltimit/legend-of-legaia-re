// The weather pass: builds "<root>/weather" - the container carrying the
// LegaiaWeather behaviour, which runs a clear / overcast / windy schedule
// off the shared server clock (greyed ambient and fog, grass gusts, and a
// wind level for the ambience mixer).
//
// Nothing here is sampled from game data; the pass only creates the
// container and wires references.
//
// Placement note: the built scene root is X-mirrored ("Match explorer
// orientation"), so the weather container cancels the root's scale on
// itself - everything under `weather` then sits in plain world space.
//
// Idempotent: the pass destroys and rebuilds its container, so applying
// the realism layer twice leaves one weather rig, not two.

using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaWeatherBuilder
    {
        internal const string CONTAINER = "weather";

        internal static void Remove(GameObject root)
        {
            var old = root.transform.Find(CONTAINER);
            if (old != null)
                Object.DestroyImmediate(old.gameObject);
        }

        /// Build the weather rig under `root` and wire LegaiaWeather.
        internal static GameObject Apply(GameObject root, string sceneName,
            LegaiaRealismOptions o)
        {
            Remove(root);
            string genDir = "Assets/LegaiaGenerated/" + sceneName + "/weather";
            Directory.CreateDirectory(genDir);
            DeleteLegacyAssets(genDir);

            var container = new GameObject(CONTAINER);
            container.transform.SetParent(root.transform, false);
            // Cancel the root's mirror/scale: the rig works in world space.
            Vector3 ls = root.transform.lossyScale;
            container.transform.localScale = new Vector3(
                Mathf.Approximately(ls.x, 0f) ? 1f : 1f / ls.x,
                Mathf.Approximately(ls.y, 0f) ? 1f : 1f / ls.y,
                Mathf.Approximately(ls.z, 0f) ? 1f : 1f / ls.z);
            container.transform.localRotation = Quaternion.identity;

            var weather = LegaiaWorldBuilder.TryAttachUdon(container, "LegaiaWeather");
            // Wind: the foliage pass's grass material carries _WindGust.
            // Absent (foliage off) the field stays null and wind is a no-op.
            LegaiaWorldBuilder.SetUdonField(weather, "grassMaterial",
                AssetDatabase.LoadAssetAtPath<Material>(
                    "Assets/LegaiaGenerated/" + sceneName + "/realism/grass.mat"));
            LegaiaWorldBuilder.SetUdonField(weather, "minSpellMinutes",
                Mathf.Max(0.5f, o.weatherSpellMinutes * 0.5f));
            LegaiaWorldBuilder.SetUdonField(weather, "maxSpellMinutes",
                Mathf.Max(1f, o.weatherSpellMinutes * 1.35f));

            // The day/night behaviour, when the realism pass built one: its
            // Update writes the ambient colours weather multiplies in
            // LateUpdate. Absent, weather uses its own captured base.
            var sunT = root.transform.Find("LegaiaSun");
            var dnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
            Component dn = sunT != null && dnType != null
                ? sunT.GetComponent(dnType) : null;
            LegaiaWorldBuilder.SetUdonField(weather, "dayNight", dn);

            // The ambience mixer is another agent's behaviour: found by
            // type NAME so this file compiles (and the pass runs) whether
            // or not that script is in the project.
            var mixType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaAmbienceMixer");
            Component mixer = mixType != null
                ? root.GetComponentInChildren(mixType, true) : null;
            LegaiaWorldBuilder.SetUdonField(weather, "ambienceMixer", mixer);
            LegaiaWorldBuilder.SyncUdonProxy(weather);

            // The settings panel's "Clear sky" button jumps this schedule -
            // wire its weather reference (the panel lives in the builder's
            // top-level camp container, if built), the same way the realism
            // pass hands it the day/night cycle.
            var menuGo = GameObject.Find("LegaiaMenu");
            if (menuGo != null)
            {
                var menuType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaWorldMenu");
                var menu = menuType != null ? menuGo.GetComponent(menuType) : null;
                if (menu != null)
                {
                    LegaiaWorldBuilder.SetUdonField(menu, "weather", weather);
                    LegaiaWorldBuilder.SyncUdonProxy(menu);
                }
            }

            Debug.Log("[Legaia] weather: clear / overcast / windy schedule wired" +
                (dn != null ? " (multiplying the day/night ambient)" : "") +
                (mixer != null ? " (feeding the ambience mixer)" : "") + ".");
            return container;
        }

        /// Assets an older weather pass generated for the rain / thunder rig.
        /// A project built before they were dropped still carries them, and
        /// they are dead weight in the world's build - so clear them out.
        static void DeleteLegacyAssets(string genDir)
        {
            string[] stale =
            {
                genDir + "/rain_drop.png",
                genDir + "/rain_particle.mat",
                genDir + "/thunder.wav",
            };
            foreach (string path in stale)
                if (AssetDatabase.LoadAssetAtPath<Object>(path) != null)
                    AssetDatabase.DeleteAsset(path);
        }
    }
}
