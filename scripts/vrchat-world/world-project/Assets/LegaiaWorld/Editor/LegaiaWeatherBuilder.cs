// The weather pass: builds "<root>/weather" - a rain emitter that
// follows the local player, a lightning flash light, a 2D thunder source
// with a synthesized clap - and wires the LegaiaWeather behaviour that
// drives all three off the shared server clock.
//
// Everything here is generated (primitives, a procedural drop sprite,
// synthesized thunder): no game data, nothing sampled.
//
// Two placement notes:
//
// - The built scene root is X-mirrored ("Match explorer orientation"),
//   so the weather container cancels the root's scale on itself: a
//   particle system under a negatively-scaled parent renders and
//   simulates against a mirrored basis, and the rain box would emit
//   sideways. With the cancel, everything under `weather` sits in plain
//   world space, which is also the space the behaviour positions the
//   rain box in each frame.
// - The rain emitter is rotated +90 on X so the box shape's emission
//   axis (local +Z) points straight down; the drops then need no
//   gravity ramp to reach a constant ~9 m/s.
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

            var container = new GameObject(CONTAINER);
            container.transform.SetParent(root.transform, false);
            // Cancel the root's mirror/scale: the rig works in world space.
            Vector3 ls = root.transform.lossyScale;
            container.transform.localScale = new Vector3(
                Mathf.Approximately(ls.x, 0f) ? 1f : 1f / ls.x,
                Mathf.Approximately(ls.y, 0f) ? 1f : 1f / ls.y,
                Mathf.Approximately(ls.z, 0f) ? 1f : 1f / ls.z);
            container.transform.localRotation = Quaternion.identity;

            var rain = BuildRain(container.transform, genDir, o);
            var flash = BuildFlashLight(container.transform);
            var thunder = BuildThunder(container.transform, genDir);

            var weather = LegaiaWorldBuilder.TryAttachUdon(container, "LegaiaWeather");
            LegaiaWorldBuilder.SetUdonField(weather, "rain", rain);
            LegaiaWorldBuilder.SetUdonField(weather, "rainRoot",
                rain != null ? rain.transform : null);
            LegaiaWorldBuilder.SetUdonField(weather, "maxEmission",
                Mathf.Max(0f, o.weatherRainEmission));
            LegaiaWorldBuilder.SetUdonField(weather, "flashLight", flash);
            LegaiaWorldBuilder.SetUdonField(weather, "lightningEnabled",
                o.weatherLightning);
            LegaiaWorldBuilder.SetUdonField(weather, "thunder", thunder);
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
            LegaiaWorldBuilder.SetUdonField(weather, "sun",
                sunT != null ? sunT.GetComponent<Light>() : null);

            // The ambience mixer is another agent's behaviour: found by
            // type NAME so this file compiles (and the pass runs) whether
            // or not that script is in the project.
            var mixType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaAmbienceMixer");
            Component mixer = mixType != null
                ? root.GetComponentInChildren(mixType, true) : null;
            LegaiaWorldBuilder.SetUdonField(weather, "ambienceMixer", mixer);
            LegaiaWorldBuilder.SyncUdonProxy(weather);

            Debug.Log("[Legaia] weather: rain + lightning + thunder wired" +
                (dn != null ? " (multiplying the day/night ambient)" : "") +
                (mixer != null ? " (feeding the ambience mixer)" : "") + ".");
            return container;
        }

        // --- Rain ---------------------------------------------------------

        static ParticleSystem BuildRain(Transform parent, string genDir,
            LegaiaRealismOptions o)
        {
            var go = new GameObject("rain");
            go.transform.SetParent(parent, false);
            // Local +Z becomes world -Y: the box emits straight down.
            go.transform.localRotation = Quaternion.Euler(90f, 0f, 0f);

            var ps = go.AddComponent<ParticleSystem>();
            var main = ps.main;
            main.loop = true;
            main.playOnAwake = true;
            main.duration = 4f;
            main.simulationSpace = ParticleSystemSimulationSpace.World;
            main.startLifetime = new ParticleSystem.MinMaxCurve(1.3f, 1.7f);
            main.startSpeed = new ParticleSystem.MinMaxCurve(8.5f, 9.5f);
            main.startSize = new ParticleSystem.MinMaxCurve(0.035f, 0.06f);
            main.startColor = new ParticleSystem.MinMaxGradient(
                new Color(0.72f, 0.78f, 0.86f, 0.42f));
            main.gravityModifier = new ParticleSystem.MinMaxCurve(0.15f);
            main.maxParticles = 1400;

            var emission = ps.emission;
            emission.enabled = true;
            // 1/s at build time; the behaviour scales this by intensity via
            // rateOverTimeMultiplier every frame (it is invisible until it
            // rains, and never zero, so the multiplier always has an effect).
            emission.rateOverTime = new ParticleSystem.MinMaxCurve(1f);

            var shape = ps.shape;
            shape.enabled = true;
            shape.shapeType = ParticleSystemShapeType.Box;
            // After the +90 X rotation: local X = world X, local Y = world Z.
            shape.scale = new Vector3(12f, 12f, 0.5f);
            shape.randomDirectionAmount = 0f;

            var colOverLife = ps.colorOverLifetime;
            colOverLife.enabled = false;
            var sizeOverLife = ps.sizeOverLifetime;
            sizeOverLife.enabled = false;
            var col = ps.collision;
            col.enabled = false; // per-particle collision is not a PC budget

            var pr = go.GetComponent<ParticleSystemRenderer>();
            pr.renderMode = ParticleSystemRenderMode.Stretch;
            pr.lengthScale = 5f;
            pr.velocityScale = 0.05f;
            pr.cameraVelocityScale = 0f;
            pr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            pr.receiveShadows = false;
            pr.sharedMaterial = EnsureRainMaterial(genDir);
            return ps;
        }

        /// A soft vertical streak sprite on an alpha-blended particle
        /// material - a stretched billboard multiplies its own length in,
        /// so the sprite only carries the drop's soft edges.
        static Material EnsureRainMaterial(string genDir)
        {
            string texPath = genDir + "/rain_drop.png";
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                const int W = 16, H = 64;
                var tex = new Texture2D(W, H, TextureFormat.RGBA32, false);
                for (int y = 0; y < H; y++)
                    for (int x = 0; x < W; x++)
                    {
                        float dx = Mathf.Abs(x - (W - 1) * 0.5f) / (W * 0.5f);
                        float dy = Mathf.Abs(y - (H - 1) * 0.5f) / (H * 0.5f);
                        float a = Mathf.Clamp01(1f - dx * dx * 2.2f) *
                                  Mathf.Clamp01(1f - dy * dy);
                        tex.SetPixel(x, y, new Color(1f, 1f, 1f, a));
                    }
                tex.Apply();
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
            }
            string matPath = genDir + "/rain_particle.mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (m == null)
            {
                m = new Material(Shader.Find("Legacy Shaders/Particles/Alpha Blended"));
                AssetDatabase.CreateAsset(m, matPath);
            }
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            return m;
        }

        // --- Lightning + thunder -------------------------------------------

        /// The flash gets its OWN directional light rather than spiking
        /// LegaiaSun: at night the sun points below the horizon and lights
        /// nothing, so a storm at 2 a.m. - the one you actually want to
        /// see - would flash invisibly. The sun is still bumped alongside
        /// it when it is up (LegaiaWeather.sun).
        static Light BuildFlashLight(Transform parent)
        {
            var go = new GameObject("lightning");
            go.transform.SetParent(parent, false);
            go.transform.rotation = Quaternion.Euler(62f, 25f, 0f);
            var l = go.AddComponent<Light>();
            l.type = LightType.Directional;
            l.color = new Color(0.86f, 0.90f, 1f);
            l.intensity = 0f;
            l.shadows = LightShadows.None; // a 0.15 s flash cannot pay for a shadow map
            l.enabled = false;
            return l;
        }

        static AudioSource BuildThunder(Transform parent, string genDir)
        {
            var go = new GameObject("thunder");
            go.transform.SetParent(parent, false);
            var src = go.AddComponent<AudioSource>();
            src.clip = LegaiaWeatherAudioGen.EnsureThunder(genDir);
            src.loop = false;
            src.playOnAwake = false;
            src.spatialBlend = 0f; // thunder is everywhere at once
            src.volume = 0.6f;
            // SDK compliance for a flat 2D bed (the disabled component the
            // SDK's own Auto Fix adds).
            LegaiaAudioGen.AddVrcSpatial(go, false, 0f, 0f, 0f);
            return src;
        }
    }
}
