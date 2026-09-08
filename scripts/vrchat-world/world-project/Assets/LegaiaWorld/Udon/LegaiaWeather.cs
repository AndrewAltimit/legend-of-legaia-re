// Synced-by-clock weather: a deterministic sequence of weather spells
// (clear / overcast / windy) every client derives from the shared server
// clock, exactly like LegaiaDayNight derives the sun. The only networked
// state is `timeOffset` - the synced jump the JumpToClear event applies
// for everyone - so a grey spell rolls in on every client at the same
// second with no per-frame traffic.
//
// The schedule is a ring of SPELLS entries, each 3-8 minutes, kind and
// length hashed from the entry index (so the ring is the same on every
// client and across sessions) and the ring repeats. A spell's first
// `rampSeconds` blend the previous spell's targets into the new one, so
// cloud cover arrives and leaves over ~45 s instead of switching on a
// frame.
//
// WHY LateUpdate: LegaiaDayNight writes the trilight ambient and the fog
// colour from ITS captured daytime values every Update. Weather must not
// fight that, so it runs afterwards and multiplies what DayNight just
// wrote (grey + dim by cloudiness). Without a day/night cycle in the
// scene nobody rewrites those values each frame, so the behaviour then
// multiplies its own Start-captured base instead - either way the
// darkening is applied exactly once per frame and never accumulates.
//
// Contracts other living-town behaviours read (do not rename):
//   public float cloudiness      0..1, the grey/dim amount
//   public float windStrength    0..1+, also pushed to the grass
//                                material as _WindGust
//   ambienceMixer.SetProgramVariable("windLevel", 0..1)
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaWeather : UdonSharpBehaviour
    {
        // Spell kinds. Keep in sync with the README table.
        public const int CLEAR = 0;
        public const int OVERCAST = 1;
        public const int WINDY = 2;

        [Tooltip("How many spells the (repeating) schedule ring holds.")]
        public int spells = 16;

        [Tooltip("Schedule seed - change it for a different weather year.")]
        public int seed = 20240117;

        [Tooltip("Shortest weather spell, minutes.")]
        public float minSpellMinutes = 3f;

        [Tooltip("Longest weather spell, minutes.")]
        public float maxSpellMinutes = 8f;

        [Tooltip("Cross-fade between two spells, seconds.")]
        public float rampSeconds = 45f;

        // --- Effects (each its own toggle) -------------------------------
        [Tooltip("Grey + dim the ambient / fog with cloud cover.")]
        public bool skyEnabled = true;

        [Tooltip("Drive _WindGust on the grass material.")]
        public bool windEnabled = true;

        // --- Wiring ------------------------------------------------------
        [Tooltip("The procedural grass material (Legaia/Grass Wind) - its _WindGust is the gust.")]
        public Material grassMaterial;

        [Tooltip("The day/night behaviour, when the scene has one - its Update writes the ambient this behaviour multiplies.")]
        public LegaiaDayNight dayNight;

        [Tooltip("The ambient-audio mixer (LegaiaAmbienceMixer), wired loosely by the builder - fed windLevel each frame.")]
        public UdonSharpBehaviour ambienceMixer;

        // --- Published state (read-only for other behaviours) ------------
        [HideInInspector] public float cloudiness;
        [HideInInspector] public float windStrength = 0.25f;
        [HideInInspector] public int spellKind = CLEAR;
        [HideInInspector] public float spellSecondsLeft;

        [UdonSynced]
        private double timeOffset;

        // Schedule ring, built once at Start from the seed.
        private int[] kinds;
        private float[] starts; // cumulative seconds at which spell i begins
        private float ringSeconds;

        // Captured scene state (the base the weather multiplies).
        private Color baseSky, baseEquator, baseGround, baseFog;
        private bool fogOn;
        private bool fogLinear;
        private float baseFogStart, baseFogEnd, baseFogDensity;

        private float mixerClock;
        private float lastGust = -1f;

        void Start()
        {
            BuildSchedule();
            baseSky = RenderSettings.ambientSkyColor;
            baseEquator = RenderSettings.ambientEquatorColor;
            baseGround = RenderSettings.ambientGroundColor;
            fogOn = RenderSettings.fog;
            baseFog = RenderSettings.fogColor;
            fogLinear = RenderSettings.fogMode == FogMode.Linear;
            baseFogStart = RenderSettings.fogStartDistance;
            baseFogEnd = RenderSettings.fogEndDistance;
            baseFogDensity = RenderSettings.fogDensity;
        }

        // --- Schedule ----------------------------------------------------

        // Wrapping int hash (Udon has no unchecked/uint; the wrap IS the
        // mix, same idiom as LegaiaCardDeck's LCG).
        int Hash(int n)
        {
            int h = n * 374761393 + seed * 668265263;
            h = (h ^ (h >> 13)) * 1274126177;
            return h ^ (h >> 16);
        }

        int Positive(int h)
        {
            return (h >> 8) & 0x7FFFFFFF;
        }

        void BuildSchedule()
        {
            int n = spells < 4 ? 4 : spells;
            kinds = new int[n];
            starts = new float[n];
            float lo = Mathf.Max(0.5f, minSpellMinutes) * 60f;
            float hi = Mathf.Max(lo + 30f, maxSpellMinutes * 60f);
            float t = 0f;
            for (int i = 0; i < n; i++)
            {
                int h = Hash(i);
                int roll = Positive(h) % 100;
                // Weights: mostly fair weather, a grey stretch now and then,
                // the odd blustery spell.
                int k = CLEAR;
                if (roll >= 55 && roll < 80) k = OVERCAST;
                else if (roll >= 80) k = WINDY;
                // Never run the same kind twice in a row - a 16-minute
                // grey stretch reads as a broken schedule.
                if (i > 0 && k == kinds[i - 1])
                    k = k == CLEAR ? OVERCAST : CLEAR;
                kinds[i] = k;
                starts[i] = t;
                t += lo + Positive(Hash(i + 977)) % Mathf.RoundToInt(hi - lo);
            }
            ringSeconds = t;
        }

        double WeatherTime()
        {
            double t = Networking.GetServerTimeInSeconds() + timeOffset;
            double w = t % (double)ringSeconds;
            if (w < 0.0)
                w += ringSeconds;
            return w;
        }

        int SpellAt(float w)
        {
            int n = kinds.Length;
            for (int i = n - 1; i >= 0; i--)
                if (w >= starts[i])
                    return i;
            return 0;
        }

        // Per-kind targets: x = cloud, y = wind.
        Vector2 TargetsOf(int kind)
        {
            if (kind == OVERCAST) return new Vector2(0.75f, 0.40f);
            if (kind == WINDY) return new Vector2(0.30f, 1f);
            return new Vector2(0f, 0.25f); // clear
        }

        // --- Per frame ---------------------------------------------------

        void LateUpdate()
        {
            if (kinds == null || kinds.Length == 0)
                return;
            float w = (float)WeatherTime();
            int i = SpellAt(w);
            int n = kinds.Length;
            float spellStart = starts[i];
            float spellEnd = i + 1 < n ? starts[i + 1] : ringSeconds;
            float elapsed = w - spellStart;
            spellSecondsLeft = spellEnd - w;
            spellKind = kinds[i];

            Vector2 cur = TargetsOf(kinds[i]);
            Vector2 prev = TargetsOf(kinds[(i - 1 + n) % n]);
            float ramp = Mathf.Max(1f, rampSeconds);
            Vector2 v = Vector2.Lerp(prev, cur, Mathf.Clamp01(elapsed / ramp));
            // Also ramp OUT: the tail of a spell eases toward the next one
            // so the ring has no step at the seam either way.
            Vector2 next = TargetsOf(kinds[(i + 1) % n]);
            float tail = spellEnd - w;
            if (tail < ramp)
                v = Vector2.Lerp(v, next, (1f - tail / ramp) * 0.5f);

            cloudiness = Mathf.Clamp01(v.x);
            windStrength = Mathf.Clamp(v.y, 0.05f, 1.5f);

            ApplySky();
            ApplyWind();
            PushMixer();
        }

        // Grey + dim, multiplicatively, on top of whatever DayNight just
        // wrote this frame (or the captured base when there is no cycle).
        void ApplySky()
        {
            if (!skyEnabled)
                return;
            float over = cloudiness;
            bool live = dayNight != null;
            Color sky = live ? RenderSettings.ambientSkyColor : baseSky;
            Color eq = live ? RenderSettings.ambientEquatorColor : baseEquator;
            Color gr = live ? RenderSettings.ambientGroundColor : baseGround;
            Color fg = live ? RenderSettings.fogColor : baseFog;
            RenderSettings.ambientSkyColor = Weathered(sky, over);
            RenderSettings.ambientEquatorColor = Weathered(eq, over);
            RenderSettings.ambientGroundColor = Weathered(gr, over);
            if (fogOn)
            {
                RenderSettings.fogColor = Weathered(fg, over);
                float thick = 1f - 0.45f * over;
                if (fogLinear)
                {
                    RenderSettings.fogStartDistance = baseFogStart * (1f - 0.30f * over);
                    RenderSettings.fogEndDistance = baseFogEnd * thick;
                }
                else
                {
                    RenderSettings.fogDensity = baseFogDensity * (1f + 1.2f * over);
                }
            }
        }

        Color Weathered(Color c, float over)
        {
            float g = c.r * 0.35f + c.g * 0.5f + c.b * 0.15f;
            Color grey = new Color(g * 0.94f, g * 0.97f, g * 1.06f, c.a);
            Color mixed = Color.Lerp(c, grey, 0.75f * over);
            float dim = 1f - 0.45f * over;
            return new Color(mixed.r * dim, mixed.g * dim, mixed.b * dim, c.a);
        }

        void ApplyWind()
        {
            if (!windEnabled || grassMaterial == null)
                return;
            // Shader.SetGlobalFloat is not exposed to Udon, so the gust
            // rides the grass material's own _WindGust (default 1 = the
            // calm sway a world without weather has always had).
            float gust = Mathf.Max(0.05f, 0.55f + windStrength);
            if (Mathf.Abs(gust - lastGust) < 0.01f)
                return;
            lastGust = gust;
            grassMaterial.SetFloat("_WindGust", gust);
        }

        void PushMixer()
        {
            if (ambienceMixer == null)
                return;
            // 10 Hz is plenty for a volume fade and keeps the string-keyed
            // variable writes off the per-frame path.
            mixerClock += Time.deltaTime;
            if (mixerClock < 0.1f)
                return;
            mixerClock = 0f;
            ambienceMixer.SetProgramVariable("windLevel", Mathf.Clamp01(windStrength));
        }

        // --- Menu jumps --------------------------------------------------
        // Same shape as LegaiaDayNight's day / night jumps: move the shared
        // offset so "now" lands at the start of the next spell of the kind
        // asked for, and sync it.

        public void JumpToClear()
        {
            JumpToKind(CLEAR);
        }

        void JumpToKind(int kind)
        {
            if (kinds == null || kinds.Length == 0)
                return;
            int pick = -1;
            for (int i = 0; i < kinds.Length; i++)
                if (kinds[i] == kind)
                {
                    pick = i;
                    break;
                }
            if (pick < 0)
                return;
            // Land a little past the spell's start so the ramp is done.
            double target = starts[pick] + Mathf.Min(rampSeconds, 30f);
            double now = WeatherTime();
            timeOffset += target - now;
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            RequestSerialization();
        }
    }
}
