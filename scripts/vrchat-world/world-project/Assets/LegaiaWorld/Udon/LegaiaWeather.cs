// Synced-by-clock weather: a deterministic sequence of weather spells
// (clear / overcast / light rain / storm / windy) every client derives
// from the shared server clock, exactly like LegaiaDayNight derives the
// sun. The only networked state is `timeOffset` - the synced jump the
// JumpToClear / JumpToRain events apply for everyone - so a storm rolls
// in on every client at the same second with no per-frame traffic.
//
// The schedule is a ring of SPELLS entries, each 3-8 minutes, kind and
// length hashed from the entry index (so the ring is the same on every
// client and across sessions) and the ring repeats. A spell's first
// `rampSeconds` blend the previous spell's targets into the new one, so
// rain arrives and leaves over ~45 s instead of switching on a frame.
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
//   public float rainIntensity   0..1, the NPC director's "go indoors"
//   public float windStrength    0..1+, also pushed to the grass
//                                material as _WindGust
//   ambienceMixer.SetProgramVariable("rainLevel" / "windLevel", 0..1)
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
        public const int LIGHT_RAIN = 2;
        public const int STORM = 3;
        public const int WINDY = 4;

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
        [Tooltip("Drive the rain particle emitter.")]
        public bool rainEnabled = true;

        [Tooltip("Grey + dim the ambient / fog with cloud cover.")]
        public bool skyEnabled = true;

        [Tooltip("Lightning flashes + thunder during storms.")]
        public bool lightningEnabled = true;

        [Tooltip("Drive _WindGust on the grass material.")]
        public bool windEnabled = true;

        // --- Wiring ------------------------------------------------------
        [Tooltip("The rain emitter (builder-built): follows the local player's head.")]
        public ParticleSystem rain;

        [Tooltip("Transform the rain emitter hangs on (usually the emitter itself).")]
        public Transform rainRoot;

        [Tooltip("Metres above the player's head the rain box sits.")]
        public float rainHeight = 8f;

        [Tooltip("Particles per second at rainIntensity 1 (PC budget).")]
        public float maxEmission = 700f;

        [Tooltip("Mute the rain when a ray straight up off the player's head hits something within this distance (a roof).")]
        public float indoorRayLength = 30f;

        [Tooltip("Dedicated flash light (builder-built, starts disabled).")]
        public Light flashLight;

        [Tooltip("Peak intensity of a lightning flash.")]
        public float flashIntensity = 2.6f;

        [Tooltip("The scene's sun (LegaiaSun), bumped alongside the flash when it is above the horizon.")]
        public Light sun;

        [Tooltip("2D thunder source (builder-built).")]
        public AudioSource thunder;

        [Tooltip("The procedural grass material (Legaia/Grass Wind) - its _WindGust is the gust.")]
        public Material grassMaterial;

        [Tooltip("The day/night behaviour, when the scene has one - its Update writes the ambient this behaviour multiplies.")]
        public LegaiaDayNight dayNight;

        [Tooltip("The ambient-audio mixer (LegaiaAmbienceMixer), wired loosely by the builder - fed rainLevel / windLevel each frame.")]
        public UdonSharpBehaviour ambienceMixer;

        // --- Published state (read-only for other behaviours) ------------
        [HideInInspector] public float rainIntensity;
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

        private float emitted = -1f;
        private bool flashOn;
        private float flashUntil;
        private float flashAmount;
        private int lastFlashSlot = -1;
        private double pendingFlashAt = -1.0;
        private float pendingThunderVolume = 0.6f;
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
            if (flashLight != null)
                flashLight.enabled = false;
            if (rain != null)
                rain.Play();
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
                // Weights: mostly fair weather, storms are the rare event.
                int k = CLEAR;
                if (roll >= 40 && roll < 65) k = OVERCAST;
                else if (roll >= 65 && roll < 80) k = LIGHT_RAIN;
                else if (roll >= 80 && roll < 90) k = STORM;
                else if (roll >= 90) k = WINDY;
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

        // Per-kind targets: x = rain, y = cloud, z = wind.
        Vector3 TargetsOf(int kind)
        {
            if (kind == OVERCAST) return new Vector3(0f, 0.75f, 0.40f);
            if (kind == LIGHT_RAIN) return new Vector3(0.35f, 0.85f, 0.35f);
            if (kind == STORM) return new Vector3(1f, 1f, 0.90f);
            if (kind == WINDY) return new Vector3(0f, 0.30f, 1f);
            return new Vector3(0f, 0f, 0.25f); // clear
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

            Vector3 cur = TargetsOf(kinds[i]);
            Vector3 prev = TargetsOf(kinds[(i - 1 + n) % n]);
            float ramp = Mathf.Max(1f, rampSeconds);
            Vector3 v = Vector3.Lerp(prev, cur, Mathf.Clamp01(elapsed / ramp));
            // Also ramp OUT: the tail of a spell eases toward the next one
            // so the ring has no step at the seam either way.
            Vector3 next = TargetsOf(kinds[(i + 1) % n]);
            float tail = spellEnd - w;
            if (tail < ramp)
                v = Vector3.Lerp(v, next, (1f - tail / ramp) * 0.5f);

            rainIntensity = Mathf.Clamp01(v.x);
            cloudiness = Mathf.Clamp01(v.y);
            windStrength = Mathf.Clamp(v.z, 0.05f, 1.5f);

            ApplyLightning(w);
            ApplyRain();
            ApplySky();
            ApplyWind();
            PushMixer();
        }

        void ApplyRain()
        {
            if (rain == null)
                return;
            float rate = rainEnabled ? rainIntensity * Mathf.Max(0f, maxEmission) : 0f;
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local != null && rainRoot != null)
            {
                VRCPlayerApi.TrackingData head =
                    local.GetTrackingData(VRCPlayerApi.TrackingDataType.Head);
                Vector3 p = head.position;
                rainRoot.position = new Vector3(p.x, p.y + rainHeight, p.z);
                // Under a roof? A single upward ray is enough here: the
                // village's interiors are detached rooms far from the huts,
                // so anything overhead within 30 m is cover.
                if (rate > 0f && indoorRayLength > 0f &&
                    Physics.Raycast(p + Vector3.up * 0.15f, Vector3.up,
                        indoorRayLength, -1, QueryTriggerInteraction.Ignore))
                    rate = 0f;
            }
            if (Mathf.Abs(rate - emitted) > Mathf.Max(1f, maxEmission * 0.01f))
            {
                emitted = rate;
                ParticleSystem.EmissionModule em = rain.emission;
                em.rateOverTimeMultiplier = rate;
            }
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
            Color bump = flashOn
                ? new Color(0.55f, 0.60f, 0.75f) * flashAmount
                : Color.black;
            RenderSettings.ambientSkyColor = Weathered(sky, over) + bump;
            RenderSettings.ambientEquatorColor = Weathered(eq, over) + bump * 0.7f;
            RenderSettings.ambientGroundColor = Weathered(gr, over) + bump * 0.4f;
            if (fogOn)
            {
                RenderSettings.fogColor = Weathered(fg, over) + bump * 0.5f;
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

        // Flashes are derived from the shared clock in fixed slots, so the
        // whole instance sees the same lightning at the same second (a
        // per-client Random would give everyone their own storm).
        void ApplyLightning(float w)
        {
            if (!lightningEnabled)
            {
                flashOn = false;
                if (flashLight != null && flashLight.enabled)
                    flashLight.enabled = false;
                return;
            }
            const float SLOT = 6f;
            int slot = Mathf.FloorToInt(w / SLOT);
            if (slot != lastFlashSlot)
            {
                lastFlashSlot = slot;
                pendingFlashAt = -1.0;
                if (spellKind == STORM && rainIntensity > 0.55f)
                {
                    int h = Hash(slot * 7919 + 31);
                    if (Positive(h) % 100 < 42)
                    {
                        pendingFlashAt = slot * SLOT + Positive(Hash(slot + 5)) % 100 * 0.06f;
                        flashAmount = 0.6f + Positive(Hash(slot + 11)) % 100 * 0.008f;
                    }
                }
            }
            if (pendingFlashAt >= 0.0 && w >= pendingFlashAt)
            {
                pendingFlashAt = -1.0;
                flashOn = true;
                flashUntil = Time.time + 0.08f + Positive(Hash(slot + 17)) % 100 * 0.0012f;
                if (flashLight != null)
                {
                    flashLight.intensity = flashIntensity * flashAmount;
                    flashLight.enabled = true;
                }
                // Distance-dependent thunder: the later it rolls in, the
                // farther the strike, the quieter the clap.
                float delay = 1f + Positive(Hash(slot + 23)) % 100 * 0.03f;
                pendingThunderVolume = Mathf.Clamp01(1.05f - delay * 0.18f);
                SendCustomEventDelayedSeconds("Thunder", delay);
            }
            if (flashOn && Time.time > flashUntil)
            {
                flashOn = false;
                if (flashLight != null)
                    flashLight.enabled = false;
            }
            if (flashOn && sun != null)
                sun.intensity = sun.intensity + flashIntensity * flashAmount * 0.35f;
        }

        public void Thunder()
        {
            if (thunder == null || thunder.clip == null)
                return;
            thunder.volume = pendingThunderVolume;
            thunder.pitch = 0.88f + pendingThunderVolume * 0.2f;
            thunder.PlayOneShot(thunder.clip, pendingThunderVolume);
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
            ambienceMixer.SetProgramVariable("rainLevel", rainIntensity);
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

        public void JumpToRain()
        {
            JumpToKind(STORM);
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
