// The ambience layer's mixer: one behaviour that owns every ambient
// volume in the world, so day/night, weather and distance never fight
// each other over the same AudioSource.
//
// It sits on the realism pass's `ambience` container (a child of the
// built root) and drives:
//
//   - the 2D beds: a base wind/surf bed that always plays, a day bed and
//     a night bed crossfaded against LegaiaDayNight's published
//     `dayFactor`, and a wind-gust bed that idles at volume 0 until the
//     weather layer asks for it;
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
// and writes exactly one float, every frame or whenever it changes,
// either typed (a LegaiaAmbienceMixer reference) or through the backing
// UdonBehaviour:
//
//     mixer.SetProgramVariable("windLevel", 0f..1f);
//
//   windLevel  0 = calm, 1 = a blustery spell. Fades the gust bed in and
//              lifts the base bed slightly.
//
// It is clamped and slewed here (weatherSlew per second), so the writer
// may step it instantly without a click. No other field is part of the
// contract; nothing else writes it.
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
        [Tooltip("Wind gust bed - silent until the weather layer raises windLevel.")]
        public AudioSource windBed;

        [Header("Bed peak volumes")]
        public float baseVolume = 0.14f;
        public float dayVolume = 0.16f;
        public float nightVolume = 0.18f;
        public float windVolume = 0.2f;

        [Header("Spatial emitter groups")]
        [Tooltip("Day-only emitters (tree birds): faded out at night.")]
        public AudioSource[] daySources;
        public float dayGroupVolume = 0.38f;

        [Tooltip("Night-only emitters (owls, frogs, crickets by the trees and the water).")]
        public AudioSource[] nightSources;
        public float nightGroupVolume = 0.34f;

        [Tooltip("All-hours emitters (shore waves, the windmill).")]
        public AudioSource[] anySources;
        public float anyGroupVolume = 0.52f;

        // --- Weather input (see the contract above) ---------------------
        [Tooltip("0..1, written by the weather layer. Fades the wind-gust bed in.")]
        public float windLevel;

        [Tooltip("How fast the weather levels are allowed to move, per second.")]
        public float weatherSlew = 0.35f;

        private float windNow;

        void Start()
        {
            // Spread every looping source across its clip so two emitters
            // sharing a clip are never in step (playback time is runtime
            // state - it cannot be authored in the scene file).
            Spread(daySources);
            Spread(nightSources);
            Spread(anySources);
            // The gust bed starts silent whatever the authored volume is.
            if (windBed != null)
                windBed.volume = 0f;
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
            windNow = Mathf.MoveTowards(windNow, Mathf.Clamp01(windLevel), dt);

            if (baseBed != null)
                baseBed.volume = baseVolume * (1f + 0.35f * windNow);
            if (dayBed != null)
                dayBed.volume = dayVolume * day;
            if (nightBed != null)
                nightBed.volume = nightVolume * (1f - day);
            if (windBed != null)
                windBed.volume = windVolume * windNow;

            SetGroup(daySources, dayGroupVolume * day);
            SetGroup(nightSources, nightGroupVolume * (1f - day));
            SetGroup(anySources, anyGroupVolume);
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
