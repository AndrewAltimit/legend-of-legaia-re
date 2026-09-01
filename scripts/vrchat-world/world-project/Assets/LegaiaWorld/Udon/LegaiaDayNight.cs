// Optional Udon behaviour the builder wires when the realism foldout's
// "Day / night cycle" is on: sweeps the realism sun (this object's own
// transform - the directional light sits on the same GameObject) through
// a full day on a fixed cycle. Every client derives the same sun angle
// from the shared server clock, so the cycle is synced across players
// with no networking events or ownership.
//
// Night keeps the scene's ambient trilight (a dim, moonless look); only
// the sun's intensity and colour animate. Shorten cycleMinutes or raise
// dayShare if the dark stretch drags.
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

        private float azimuth;

        void Start()
        {
            // The builder aims the sun with a world-space rotation; keep its
            // compass heading and let this behaviour own only the elevation.
            azimuth = transform.eulerAngles.y;
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
        }
    }
}
