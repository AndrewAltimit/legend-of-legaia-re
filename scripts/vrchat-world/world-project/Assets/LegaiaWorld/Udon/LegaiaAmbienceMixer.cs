// The ambience layer's mixer: one behaviour that owns every ambient
// volume in the world, so day/night, weather and distance never fight
// each other over the same AudioSource.
//
// It sits on the realism pass's `ambience` container (a child of the
// built root) and drives:
//
//   - the 2D beds: a base wind/surf bed that always plays, a day bed and
//     a night bed crossfaded against LegaiaDayNight's published
//     `dayFactor`, and two weather beds (rain, wind gust) that idle at
//     volume 0 until the weather layer asks for them;
//   - the spatial emitter groups: day-only (tree birds), night-only
//     (owls / frogs / crickets by the trees and the water) and all-hours
//     (the shore waves, the windmill), each an array with one peak
//     volume for the group.
//
// The clips themselves are 30-100 s loops, so several emitters sharing
// one clip would phase-lock into a chorus if they all started at t=0.
// AudioSource playback position is not serialized, so the offsets cannot
// be authored in the editor - Start() seeds each source's `time` here
// instead, spreading the group across the loop.
//
// --- Weather contract ------------------------------------------------
// The weather layer finds this behaviour by TYPE NAME
//
//     LegaiaWorld.LegaiaAmbienceMixer      under <root>/ambience
//
// and writes exactly two floats, every frame or whenever they change,
// either typed (a LegaiaAmbienceMixer reference) or through the backing
// UdonBehaviour:
//
//     mixer.SetProgramVariable("rainLevel", 0f..1f);
//     mixer.SetProgramVariable("windLevel", 0f..1f);
//
//   rainLevel  0 = dry, 1 = full downpour. Fades the rain bed in, ducks
//              the bird group hard (birds shelter in rain), and takes a
//              little off the day/night beds so the rain reads as the
//              dominant layer.
//   windLevel  0 = calm, 1 = storm. Fades the gust bed in and lifts the
//              base bed slightly.
//
// Both are clamped and slewed here (WEATHER_SLEW per second), so the
// writer may step them instantly without a click. No other field is part
// of the contract; nothing else writes these two.
//
// LegaiaDayNight keeps working with no mixer in the scene (it fades its
// own dayAmbience / nightAmbience sources). When the mixer is present the
// realism pass clears those two references, so exactly one behaviour owns
// each source.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaAmbienceMixer : UdonSharpBehaviour
    {
        [Tooltip("The day/night cycle this mixer crossfades against. Null = permanent day (the beds still play).")]
        public LegaiaDayNight dayNight;

        [Header("2D beds")]
        [Tooltip("Wind / distant surf - always audible.")]
        public AudioSource baseBed;
        [Tooltip("Daytime bed (breeze, leaves, distant birds).")]
        public AudioSource dayBed;
        [Tooltip("Night bed (cricket chorus, cool breeze, frogs, a far owl).")]
        public AudioSource nightBed;
        [Tooltip("Rain bed - silent until the weather layer raises rainLevel.")]
        public AudioSource rainBed;
        [Tooltip("Storm gust bed - silent until the weather layer raises windLevel.")]
        public AudioSource windBed;

        [Header("Bed peak volumes")]
        public float baseVolume = 0.14f;
        public float dayVolume = 0.16f;
        public float nightVolume = 0.18f;
        public float rainVolume = 0.22f;
        public float windVolume = 0.2f;

        [Header("Spatial emitter groups")]
        [Tooltip("Day-only emitters (tree birds): faded out at night and ducked in rain.")]
        public AudioSource[] daySources;
        public float dayGroupVolume = 0.38f;

        [Tooltip("Night-only emitters (owls, frogs, crickets by the trees and the water).")]
        public AudioSource[] nightSources;
        public float nightGroupVolume = 0.34f;

        [Tooltip("All-hours emitters (shore waves, the windmill).")]
        public AudioSource[] anySources;
        public float anyGroupVolume = 0.52f;

        // --- Weather inputs (see the contract above) --------------------
        [Tooltip("0..1, written by the weather layer. Fades the rain bed in and ducks the birds.")]
        public float rainLevel;
        [Tooltip("0..1, written by the weather layer. Fades the storm-gust bed in.")]
        public float windLevel;

        [Tooltip("How fast the weather levels are allowed to move, per second.")]
        public float weatherSlew = 0.35f;

        private float rainNow;
        private float windNow;

        void Start()
        {
            // Spread every looping source across its clip so two emitters
            // sharing a clip are never in step (playback time is runtime
            // state - it cannot be authored in the scene file).
            Spread(daySources);
            Spread(nightSources);
            Spread(anySources);
            // The weather beds start silent whatever the authored volume is.
            if (rainBed != null)
                rainBed.volume = 0f;
            if (windBed != null)
                windBed.volume = 0f;
            rainNow = 0f;
            windNow = 0f;
        }

        void Spread(AudioSource[] group)
        {
            if (group == null)
                return;
            for (int i = 0; i < group.Length; i++)
            {
                AudioSource s = group[i];
                if (s == null || s.clip == null)
                    continue;
                float len = s.clip.length;
                if (len <= 0.05f)
                    continue;
                // Golden-ratio stride + a little jitter: an even spread for
                // any group size, still different per instance.
                float f = (i * 0.618034f) + Random.Range(0f, 0.13f);
                f = f - Mathf.Floor(f);
                s.time = Mathf.Clamp(f * len, 0f, len - 0.05f);
            }
        }

        void Update()
        {
            float day = 1f;
            if (dayNight != null)
                day = Mathf.Clamp01(dayNight.dayFactor);

            float dt = Time.deltaTime * (weatherSlew > 0f ? weatherSlew : 0.35f);
            rainNow = Mathf.MoveTowards(rainNow, Mathf.Clamp01(rainLevel), dt);
            windNow = Mathf.MoveTowards(windNow, Mathf.Clamp01(windLevel), dt);

            // Rain takes the top off everything else so it reads as the
            // dominant layer instead of piling on top of a full mix.
            float wet = 1f - 0.3f * rainNow;
            float birdDuck = 1f - 0.85f * rainNow;

            if (baseBed != null)
                baseBed.volume = baseVolume * wet * (1f + 0.35f * windNow);
            if (dayBed != null)
                dayBed.volume = dayVolume * day * wet;
            if (nightBed != null)
                nightBed.volume = nightVolume * (1f - day) * wet;
            if (rainBed != null)
                rainBed.volume = rainVolume * rainNow;
            if (windBed != null)
                windBed.volume = windVolume * windNow;

            SetGroup(daySources, dayGroupVolume * day * birdDuck);
            SetGroup(nightSources, nightGroupVolume * (1f - day) * wet);
            SetGroup(anySources, anyGroupVolume * wet);
        }

        void SetGroup(AudioSource[] group, float volume)
        {
            if (group == null)
                return;
            for (int i = 0; i < group.Length; i++)
            {
                AudioSource s = group[i];
                if (s != null)
                    s.volume = volume;
            }
        }
    }
}
