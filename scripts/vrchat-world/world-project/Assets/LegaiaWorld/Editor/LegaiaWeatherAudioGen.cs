// Synthesized weather audio: the thunder clap LegaiaWeather fires after
// a lightning flash. Generated from scratch (noise + filters + envelopes)
// like every other clip the kit ships - no disc audio, nothing sampled.
//
// Deliberately separate from LegaiaAudioGen.cs (the ambience generator):
// this file only ADDS a generator and calls that file's WriteWav /
// EnsureClip writers, so the two evolve independently.
//
// Shape of a real clap, in three overlapping parts:
//   1. the crack - a few tens of milliseconds of bright noise with a
//      near-instant attack and a fast decay (the leader stroke);
//   2. the body - broadband noise whose low-pass cutoff falls as the
//      sound arrives from further along the channel, so it darkens as it
//      decays;
//   3. the rumble tail - deep, slowly decaying noise with a lazy
//      amplitude wobble (the echoes off cloud layers and terrain), a few
//      seconds long.
// Peak normalization happens in WriteWav, so only the relative shape of
// these three matters here.

using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaWeatherAudioGen
    {
        const int SR = 44100;

        /// Write the thunder wav (if missing) and return the clip.
        internal static AudioClip EnsureThunder(string genDir)
        {
            return LegaiaAudioGen.EnsureClip(genDir + "/thunder.wav", Thunder);
        }

        /// ~5.5 s clap: crack, darkening body, slow rumble tail.
        internal static float[] Thunder()
        {
            const float seconds = 5.5f;
            int n = (int)(SR * seconds);
            var s = new float[n];
            var rng = new System.Random(90210);

            // Filter state: three one-pole low passes (cascaded for a
            // steeper skirt on the rumble) and one high pass for the crack.
            float lp1 = 0f, lp2 = 0f, lp3 = 0f, hp = 0f, prev = 0f;
            // Slow amplitude wobble of the tail - two detuned sines, so the
            // rumble breathes instead of decaying like a bell.
            float wobblePhase = 0f;

            for (int i = 0; i < n; i++)
            {
                float t = (float)i / SR;
                float white = (float)(rng.NextDouble() * 2.0 - 1.0);

                // 1. Crack: high-passed noise, 25 ms attackless burst with
                // a 90 ms decay, plus one weaker restrike at 140 ms.
                hp = 0.86f * (hp + white - prev);
                prev = white;
                float crackEnv = Mathf.Exp(-t / 0.09f);
                if (t > 0.14f)
                    crackEnv += 0.45f * Mathf.Exp(-(t - 0.14f) / 0.06f);
                float crack = hp * crackEnv * 0.9f;

                // 2. Body: the cutoff falls from ~1.5 kHz to ~120 Hz over
                // the first second and a half, so the clap darkens as it
                // rolls away.
                float k = Mathf.Lerp(0.10f, 0.010f, Mathf.Clamp01(t / 1.5f));
                lp1 += k * (white - lp1);
                lp2 += k * (lp1 - lp2);
                float bodyEnv = Mathf.Exp(-t / 0.55f) * (1f - Mathf.Exp(-t / 0.02f));
                float body = lp2 * bodyEnv * 2.6f;

                // 3. Rumble tail: a third pole for the deep end, a long
                // decay and the wobble.
                lp3 += 0.004f * (lp2 - lp3);
                wobblePhase += 1f / SR;
                float wobble = 0.72f
                    + 0.28f * Mathf.Sin(wobblePhase * 2f * Mathf.PI * 0.7f)
                    * Mathf.Sin(wobblePhase * 2f * Mathf.PI * 0.23f + 1.1f);
                float tailEnv = Mathf.Exp(-t / 1.9f) * (1f - Mathf.Exp(-t / 0.12f));
                float rumble = lp3 * tailEnv * wobble * 9f;

                s[i] = crack + body + rumble;
            }

            // Fade the last 300 ms so the clip ends in silence (a truncated
            // rumble clicks).
            int fade = SR * 3 / 10;
            for (int i = 0; i < fade; i++)
            {
                float w = (float)i / fade;
                s[n - 1 - i] *= w;
            }
            return s;
        }
    }
}
