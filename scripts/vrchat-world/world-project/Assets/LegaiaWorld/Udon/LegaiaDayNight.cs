// Optional Udon behaviour the builder wires when the realism foldout's
// "Day / night cycle" is on: sweeps the realism sun (this object's own
// transform - the directional light sits on the same GameObject) through
// a full day on a fixed cycle. Every client derives the same sun angle
// from the shared server clock, so the cycle is synced across players
// with no networking events or ownership.
//
// Night darkness: sun intensity alone is not enough - the trilight
// ambient keeps lighting the landscape at daytime levels after sunset -
// so this behaviour also sweeps the ambient (and fog colour) down to a
// moonlit fraction of the daytime values it captures at Start. It also
// enables `nightLights` (the realism pass's night_lamps container -
// warm lamps on the buildings) only while the sun is below the horizon.
// Shorten cycleMinutes or raise dayShare if the dark stretch drags.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    public class LegaiaDayNight : UdonSharpBehaviour
    {
        [Tooltip("The directional light this behaviour drives (the builder's LegaiaSun - on this same GameObject).")]
        public Light sun;

        [Tooltip("Full day+night cycle length in minutes.")]
        public float cycleMinutes = 20f;

        [Tooltip("Fraction of the cycle spent above the horizon - night passes faster than day.")]
        public float dayShare = 0.7f;

        [Tooltip("Sun intensity at high noon (the builder copies its sun-intensity slider here).")]
        public float dayIntensity = 1.15f;

        [Tooltip("Sun colour at high noon.")]
        public Color dayColor = new Color(1f, 0.956f, 0.878f);

        [Tooltip("Sun colour just above the horizon (dawn / dusk).")]
        public Color horizonColor = new Color(1f, 0.55f, 0.25f);

        [Tooltip("Midnight ambient as a fraction of the daytime trilight - the landscape's night darkness (the sun itself is already off at night). 0 = pitch black, 1 = night stays day-bright.")]
        public float nightAmbientScale = 0.05f;

        [Tooltip("Root object holding the night-only lamps (the realism pass's night_lamps container): enabled while the sun is below the horizon, disabled by day.")]
        public GameObject nightLights;

        private float azimuth;
        private Color daySky;
        private Color dayEquator;
        private Color dayGround;
        private Color dayFog;
        private bool fogOn;
        private bool lightsOn;

        void Start()
        {
            // The builder aims the sun with a world-space rotation; keep its
            // compass heading and let this behaviour own only the elevation.
            azimuth = transform.eulerAngles.y;
            // The realism pass's daytime scene values are the reference the
            // night interpolates from - captured once, and this behaviour is
            // their only writer afterwards.
            daySky = RenderSettings.ambientSkyColor;
            dayEquator = RenderSettings.ambientEquatorColor;
            dayGround = RenderSettings.ambientGroundColor;
            fogOn = RenderSettings.fog;
            dayFog = RenderSettings.fogColor;
            if (nightLights != null)
                lightsOn = nightLights.activeSelf;
        }

        // Moonlit version of a daytime colour: dimmed to nightAmbientScale
        // with a blue shift so night reads cold instead of gray.
        Color NightOf(Color day)
        {
            return new Color(day.r * 0.7f, day.g * 0.85f, day.b * 1.3f)
                * nightAmbientScale;
        }

        void Update()
        {
            if (sun == null)
                return;
            double cycle = cycleMinutes * 60.0;
            if (cycle < 1.0)
                cycle = 1.0;
            float ds = Mathf.Clamp(dayShare, 0.05f, 0.95f);
            float phase = (float)((Networking.GetServerTimeInSeconds() % cycle) / cycle);
            // 0..dayShare maps to the 180 degrees above the horizon,
            // the rest to the 180 below - a piecewise-constant-rate sweep.
            float elev = phase < ds
                ? phase / ds * 180f
                : 180f + (phase - ds) / (1f - ds) * 180f;
            transform.rotation = Quaternion.Euler(elev, azimuth, 0f);
            float up = Mathf.Sin(elev * Mathf.Deg2Rad);
            sun.intensity = Mathf.Clamp01(up * 4f) * dayIntensity;
            sun.color = Color.Lerp(horizonColor, dayColor, Mathf.Clamp01(up * 2.5f));

            // Landscape darkness: sweep the trilight ambient (and the fog
            // colour, so distant haze doesn't glow day-bright) down to the
            // moonlit fraction as the sun sets.
            float dayF = Mathf.Clamp01(up * 2.5f);
            RenderSettings.ambientSkyColor =
                Color.Lerp(NightOf(daySky), daySky, dayF);
            RenderSettings.ambientEquatorColor =
                Color.Lerp(NightOf(dayEquator), dayEquator, dayF);
            RenderSettings.ambientGroundColor =
                Color.Lerp(NightOf(dayGround), dayGround, dayF);
            if (fogOn)
                RenderSettings.fogColor = Color.Lerp(NightOf(dayFog), dayFog, dayF);

            // Building lamps: on from just before sunset to just after
            // sunrise. One SetActive on the container flips every lamp.
            bool night = up < 0.05f;
            if (nightLights != null && night != lightsOn)
            {
                lightsOn = night;
                nightLights.SetActive(night);
            }
        }
    }
}
