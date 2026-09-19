// Optional Udon behaviour the builder wires when the realism foldout's
// "Day / night cycle" is on: sweeps the realism sun (this object's own
// transform - the directional light sits on the same GameObject) through
// a full day on a fixed cycle. Every client derives the same sun angle
// from the shared server clock, so the cycle is synced across players
// by construction; the only networked state is `timeOffset`, the synced
// jump the settings panel's Day / Dusk / Night buttons apply for everyone.
//
// What one frame of the cycle writes:
//
//   sun        elevation from the clock; intensity ramps in over the
//              first ~15 degrees; colour runs deep-orange -> golden ->
//              white through the morning (and back). The Light is
//              DISABLED below the horizon so the moon's shadows are the
//              only directional shadows at night (one shadowed
//              directional at a time).
//   moon       a second directional light (`moon`, the builder's
//              LegaiaMoon child), opposite the sun with an azimuth offset
//              so it is never exactly antipodal; cool, faint, enabled only
//              while the sun is down. Its phase advances one eighth per
//              cycle - full on cycle 0, new on cycle 4 - and its light
//              scales with the lit fraction.
//   ambient    the trilight sweeps from the daytime values captured at
//              Start down to a moonlit, blue-shifted fraction
//              (`nightAmbientScale`), with a purple twilight tint through
//              the sunset bell.
//   fog        the colour follows the SKY HORIZON of the moment (so the
//              haze goes orange at sunset and near-black at night)
//              blended toward the captured daytime fog by day.
//   sky        every property of the Legaia/Sky material (`skyMaterial`):
//              zenith / horizon / ground palettes (day, dusk, night),
//              sun + moon direction and colour, star strength and the
//              star frame's rotation (the stars wheel with the sun),
//              cloud cover from the weather layer's cloudiness (or a
//              fair-weather default) and a cloud offset that drifts with
//              the wind. One writer: the weather behaviour never touches
//              the sky material.
//   lamps      `nightLights` / `nightTorches` containers enabled while
//              the sun is below the horizon.
//   ambience   the two legacy beds crossfade on the day factor when no
//              LegaiaAmbienceMixer owns them.
//
// Shorten cycleMinutes or raise dayShare if the dark stretch drags.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
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

        [Tooltip("Sun colour through the golden hour (about 10 degrees up).")]
        public Color goldenColor = new Color(1f, 0.80f, 0.55f);

        [Tooltip("Sun colour just above the horizon (dawn / dusk).")]
        public Color horizonColor = new Color(1f, 0.50f, 0.22f);

        [Tooltip("Midnight ambient as a fraction of the daytime trilight - the landscape's night darkness (the sun itself is already off at night). 0 = pitch black, 1 = night stays day-bright.")]
        public float nightAmbientScale = 0.02f;

        [Tooltip("Ambient tint blended in through the sunset / sunrise bell.")]
        public Color twilightAmbient = new Color(0.46f, 0.30f, 0.40f);

        [Tooltip("Root object holding the night-only lamps (the realism pass's night_lamps container): enabled while the sun is below the horizon, disabled by day.")]
        public GameObject nightLights;

        [Tooltip("Root object holding the always-burning night torches (the realism pass's top-level torch container): enabled while the sun is below the horizon, disabled by day.")]
        public GameObject nightTorches;

        [Tooltip("Daytime ambience bed (breeze + birds) - its volume is faded in with the sun.")]
        public AudioSource dayAmbience;

        [Tooltip("Night ambience bed (crickets) - its volume is faded in as the sun sets.")]
        public AudioSource nightAmbience;

        [Tooltip("Peak volume of the daytime ambience bed.")]
        public float dayAmbienceVolume = 0.16f;

        [Tooltip("Peak volume of the night ambience bed.")]
        public float nightAmbienceVolume = 0.2f;

        // --- Moon ------------------------------------------------------------
        [Header("Moon")]
        [Tooltip("The moon's directional light (the builder's LegaiaMoon child). Enabled only while the sun is down.")]
        public Light moon;

        [Tooltip("Moonlight intensity at a full moon, high in the sky.")]
        public float moonIntensity = 0.14f;

        [Tooltip("Moonlight colour.")]
        public Color moonColor = new Color(0.62f, 0.72f, 1f);

        [Tooltip("Degrees the moon's heading differs from the sun's opposite - so it is never exactly antipodal.")]
        public float moonAzimuthOffset = 35f;

        // --- Sky -------------------------------------------------------------
        [Header("Sky")]
        [Tooltip("The Legaia/Sky material (RenderSettings.skybox) - every property is written from here each frame.")]
        public Material skyMaterial;

        [Tooltip("The weather behaviour, when the scene has one - its cloudiness and wind drive the cloud layer. Null = fair-weather clouds.")]
        public LegaiaWeather weather;

        [Tooltip("Cloud cover with no weather layer, or on a clear spell (0 = none, 1 = solid).")]
        public float fairWeatherClouds = 0.30f;

        [Tooltip("Cloud drift in noise units per second at a wind strength of 1.")]
        public float cloudDrift = 0.02f;

        public Color zenithDay = new Color(0.24f, 0.45f, 0.85f);
        public Color zenithDusk = new Color(0.18f, 0.20f, 0.46f);
        public Color zenithNight = new Color(0.006f, 0.008f, 0.022f);
        public Color horizonDay = new Color(0.70f, 0.80f, 0.92f);
        public Color horizonDusk = new Color(0.86f, 0.46f, 0.30f);
        public Color horizonNight = new Color(0.020f, 0.025f, 0.048f);
        public Color groundDay = new Color(0.40f, 0.42f, 0.45f);
        public Color groundDusk = new Color(0.28f, 0.20f, 0.22f);
        public Color groundNight = new Color(0.012f, 0.012f, 0.018f);
        public Color cloudLitDay = new Color(1f, 1f, 1f);
        public Color cloudLitDusk = new Color(1f, 0.62f, 0.42f);
        public Color cloudLitNight = new Color(0.10f, 0.12f, 0.20f);
        public Color cloudShadeDay = new Color(0.62f, 0.66f, 0.76f);
        public Color cloudShadeDusk = new Color(0.38f, 0.26f, 0.36f);
        public Color cloudShadeNight = new Color(0.035f, 0.04f, 0.07f);

        // --- Published phase (read-only for other behaviours) ------------
        // The living-town layer (NPC director, ambience mixer, weather)
        // reads the cycle from here instead of re-deriving it: `dayFactor`
        // is the same 0..1 the ambient sweep and the ambience crossfade
        // use (1 = full day, 0 = night), `sunUp` is the signed sine of the
        // sun's elevation (negative below the horizon), `isNight` is the
        // lamps-on state, and `phase` is the raw 0..1 position in the
        // cycle (0 = sunrise, dayShare = sunset). `moonPhase` is the
        // moon's lit phase (0 new, 0.5 full) and `twilight` the 0..1
        // sunset / sunrise bell.
        [HideInInspector] public float dayFactor = 1f;
        [HideInInspector] public float sunUp = 1f;
        [HideInInspector] public bool isNight;
        [HideInInspector] public float phase;
        [HideInInspector] public float moonPhase = 0.5f;
        [HideInInspector] public float twilight;

        // Synced jump applied on top of the server clock, so the menu's
        // "Day" / "Night" buttons move the cycle for everyone at once.
        [UdonSynced]
        private double timeOffset;

        private float azimuth;
        private Color daySky;
        private Color dayEquator;
        private Color dayGround;
        private Color dayFog;
        private bool fogOn;
        private bool lightsOn;
        private bool sunOn = true;
        private bool moonOn;
        private Vector2 cloudOffset;
        private Vector3 starAxis = Vector3.right;

        void Start()
        {
            // The builder aims the sun with a world-space rotation; keep its
            // compass heading and let this behaviour own only the elevation.
            azimuth = transform.eulerAngles.y;
            // The celestial axis: the sun's Euler(elev, azimuth, 0) rotates
            // about the heading-rotated X axis, so the stars wheel about it.
            starAxis = Quaternion.Euler(0f, azimuth, 0f) * Vector3.right;
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
            else if (nightTorches != null)
                lightsOn = nightTorches.activeSelf;
            if (sun != null)
                sunOn = sun.enabled;
            if (moon != null)
            {
                moonOn = moon.enabled;
                moon.color = moonColor;
            }
            cloudOffset = new Vector2(37.2f, 11.6f);
        }

        // Moonlit version of a daytime colour: dimmed to nightAmbientScale
        // with a blue shift so night reads cold instead of gray.
        Color NightOf(Color day)
        {
            return new Color(day.r * 0.7f, day.g * 0.85f, day.b * 1.3f)
                * nightAmbientScale;
        }

        // Day / dusk / night palette blend: night -> day on the sun's
        // height, the dusk stop pulled in through the twilight bell.
        Color Palette(Color night, Color dusk, Color day, float dayF, float bell)
        {
            Color c = Color.Lerp(night, day, dayF);
            return Color.Lerp(c, dusk, bell);
        }

        void Update()
        {
            if (sun == null)
                return;
            double cycle = cycleMinutes * 60.0;
            if (cycle < 1.0)
                cycle = 1.0;
            float ds = Mathf.Clamp(dayShare, 0.05f, 0.95f);
            double t = Networking.GetServerTimeInSeconds() + timeOffset;
            double wrapped = t % cycle;
            if (wrapped < 0.0)
                wrapped += cycle;
            phase = (float)(wrapped / cycle);
            // 0..dayShare maps to the 180 degrees above the horizon,
            // the rest to the 180 below - a piecewise-constant-rate sweep.
            float elev = phase < ds
                ? phase / ds * 180f
                : 180f + (phase - ds) / (1f - ds) * 180f;
            transform.rotation = Quaternion.Euler(elev, azimuth, 0f);
            float up = Mathf.Sin(elev * Mathf.Deg2Rad);
            sunUp = up;

            // The moon's phase advances an eighth per cycle: full on the
            // first cycle of the clock, new four cycles later. Derived from
            // the same clock, so every client sees the same moon.
            int cycleIndex = (int)((t - wrapped) / cycle + 0.5);
            int eighth = ((cycleIndex % 8) + 8) % 8;
            moonPhase = (0.5f + eighth / 8f) % 1f;
            float fullness = 1f - Mathf.Abs(moonPhase - 0.5f) * 2f;

            // --- Sun -------------------------------------------------------
            sun.intensity = Mathf.Clamp01(up * 4f) * dayIntensity;
            Color sunCol = up < 0.15f
                ? Color.Lerp(horizonColor, goldenColor, Mathf.Clamp01(up / 0.15f))
                : Color.Lerp(goldenColor, dayColor, Mathf.Clamp01((up - 0.15f) / 0.35f));
            sun.color = sunCol;
            bool wantSun = up > -0.03f;
            if (wantSun != sunOn)
            {
                sunOn = wantSun;
                sun.enabled = wantSun;
            }

            // --- Moon ------------------------------------------------------
            if (moon != null)
            {
                moon.transform.rotation =
                    Quaternion.Euler(elev + 180f, azimuth + moonAzimuthOffset, 0f);
                float moonUp = -up;
                moon.intensity = moonIntensity * Mathf.Clamp01(moonUp * 4f)
                                 * (0.3f + 0.7f * fullness);
                bool wantMoon = up < 0.03f;
                if (wantMoon != moonOn)
                {
                    moonOn = wantMoon;
                    moon.enabled = wantMoon;
                }
            }

            // --- Ambient + fog ---------------------------------------------
            // Landscape darkness: sweep the trilight ambient down to the
            // moonlit fraction as the sun sets, with the twilight tint
            // through the bell either side of the horizon.
            float dayF = Mathf.Clamp01(up * 2.5f);
            dayFactor = dayF;
            float bell = 1f - Mathf.Clamp01(Mathf.Abs(up - 0.03f) / 0.22f);
            bell = bell * bell * (3f - 2f * bell);
            twilight = bell;
            Color ambSky = Color.Lerp(NightOf(daySky), daySky, dayF);
            Color ambEq = Color.Lerp(NightOf(dayEquator), dayEquator, dayF);
            Color ambGr = Color.Lerp(NightOf(dayGround), dayGround, dayF);
            float tint = bell * 0.45f;
            RenderSettings.ambientSkyColor = Color.Lerp(ambSky, twilightAmbient, tint);
            RenderSettings.ambientEquatorColor =
                Color.Lerp(ambEq, twilightAmbient * 0.8f, tint);
            RenderSettings.ambientGroundColor =
                Color.Lerp(ambGr, twilightAmbient * 0.5f, tint);

            Color horizonNow = Palette(horizonNight, horizonDusk, horizonDay, dayF, bell);
            if (fogOn)
                RenderSettings.fogColor = Color.Lerp(horizonNow, dayFog, dayF * 0.65f);

            // --- Sky ---------------------------------------------------------
            if (skyMaterial != null)
                WriteSky(elev, up, dayF, bell, fullness, horizonNow, sunCol);

            // Building lamps + planted torches: on from just before sunset
            // to just after sunrise. One SetActive per container flips all.
            bool night = up < 0.05f;
            isNight = night;
            if (night != lightsOn)
            {
                lightsOn = night;
                if (nightLights != null)
                    nightLights.SetActive(night);
                if (nightTorches != null)
                    nightTorches.SetActive(night);
            }

            // Ambience beds: crossfade breeze/birds against crickets with
            // the same day factor the ambient sweep uses.
            if (dayAmbience != null)
                dayAmbience.volume = dayAmbienceVolume * dayF;
            if (nightAmbience != null)
                nightAmbience.volume = nightAmbienceVolume * (1f - dayF);
        }

        void WriteSky(float elev, float up, float dayF, float bell, float fullness,
            Color horizonNow, Color sunCol)
        {
            Material m = skyMaterial;
            m.SetColor("_ZenithColor", Palette(zenithNight, zenithDusk, zenithDay, dayF, bell));
            m.SetColor("_HorizonColor", horizonNow);
            m.SetColor("_GroundColor", Palette(groundNight, groundDusk, groundDay, dayF, bell));
            m.SetFloat("_HorizonGlow", bell);

            // The disc keeps most of its colour at the horizon (that IS the
            // sunset); the shader hides it once it is below the ground line.
            Vector3 toSun = -transform.forward;
            m.SetVector("_SunDir", new Vector4(toSun.x, toSun.y, toSun.z, 0f));
            m.SetColor("_SunColor", sunCol * (0.35f + 0.65f * Mathf.Clamp01(up * 4f)));

            float dark = Mathf.Clamp01((-up + 0.02f) * 6f);
            if (moon != null)
            {
                Vector3 toMoon = -moon.transform.forward;
                m.SetVector("_MoonDir", new Vector4(toMoon.x, toMoon.y, toMoon.z, 0f));
            }
            m.SetColor("_MoonColor", moonColor * (0.55f + 0.45f * dark));
            m.SetFloat("_MoonPhase", moonPhase);
            m.SetFloat("_MoonGlow", 0.5f * fullness * dark);

            m.SetFloat("_StarStrength", dark);
            m.SetVector("_StarAxis", new Vector4(starAxis.x, starAxis.y, starAxis.z, 0f));
            // The raw sweep angle, not eulerAngles.x: past 90 degrees Unity
            // re-expresses the rotation and the stars would snap.
            m.SetFloat("_StarAngle", elev * Mathf.Deg2Rad);

            // Clouds: cover and drift from the weather layer when there is
            // one (clear spells still carry a few fair-weather clouds).
            float cover = fairWeatherClouds;
            float wind = 0.25f;
            if (weather != null)
            {
                cover = Mathf.Lerp(fairWeatherClouds, 0.92f, Mathf.Clamp01(weather.cloudiness));
                wind = Mathf.Clamp(weather.windStrength, 0.05f, 1.5f);
            }
            float step = cloudDrift * (0.3f + wind) * Time.deltaTime;
            cloudOffset.x = Mathf.Repeat(cloudOffset.x + step, 256f);
            cloudOffset.y = Mathf.Repeat(cloudOffset.y + step * 0.35f, 256f);
            m.SetFloat("_CloudCover", cover);
            m.SetVector("_CloudOffset", new Vector4(cloudOffset.x, cloudOffset.y, 0f, 0f));
            m.SetColor("_CloudColor", Palette(cloudLitNight, cloudLitDusk, cloudLitDay, dayF, bell));
            m.SetColor("_CloudShadeColor",
                Palette(cloudShadeNight, cloudShadeDusk, cloudShadeDay, dayF, bell));
        }

        // --- Menu jumps -------------------------------------------------
        // Jump the shared cycle so "now" lands at the requested phase, and
        // sync the offset: every player's clock-derived sun agrees again on
        // the next serialization.

        public void JumpToDay()
        {
            float ds = Mathf.Clamp(dayShare, 0.05f, 0.95f);
            JumpToPhase(0.5f * ds); // high noon
        }

        public void JumpToNight()
        {
            float ds = Mathf.Clamp(dayShare, 0.05f, 0.95f);
            JumpToPhase(ds + 0.5f * (1f - ds)); // midnight
        }

        public void JumpToDusk()
        {
            float ds = Mathf.Clamp(dayShare, 0.05f, 0.95f);
            JumpToPhase(ds * (1f - 12f / 180f)); // 12 degrees before sunset
        }

        void JumpToPhase(float target)
        {
            double cycle = cycleMinutes * 60.0;
            if (cycle < 1.0)
                cycle = 1.0;
            double t = Networking.GetServerTimeInSeconds();
            double current = (t + timeOffset) % cycle;
            if (current < 0.0)
                current += cycle;
            timeOffset += target * cycle - current;
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            RequestSerialization();
        }
    }
}
