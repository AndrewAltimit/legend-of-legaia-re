// Synthesized ambience clips + VRChat spatial-audio compliance, shared
// by the builder (fire crackle for the camp props) and the realism pass
// (day / night ambience beds). Everything here is generated from
// scratch - noise, sines and envelopes - no disc audio.
//
// VRChat deprecation note: the SDK flags every AudioSource that has no
// VRC_SpatialAudioSource sibling ("Found 2D audio source with no VRC
// Spatial Audio component, this is deprecated"). AddVrcSpatial is the
// builder-side version of the SDK's own Auto Fix: a disabled component
// for the flat 2D beds (music, ambience), a configured enabled one for
// genuinely spatial sources (torch / campfire crackle).

using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaAudioGen
    {
        const int SR = 44100;

        /// Write the wav (if missing), import it, and return the AudioClip.
        internal static AudioClip EnsureClip(string path, System.Func<float[]> gen)
        {
            if (AssetDatabase.LoadAssetAtPath<AudioClip>(path) == null)
            {
                WriteWav(path, gen());
                AssetDatabase.ImportAsset(path);
            }
            var clip = AssetDatabase.LoadAssetAtPath<AudioClip>(path);
            if (clip == null)
                Debug.LogWarning("[Legaia] generated clip failed to import: " + path);
            return clip;
        }

        /// Mono 16-bit RIFF writer, peak-normalized to 0.8.
        internal static void WriteWav(string path, float[] s)
        {
            int n = s.Length;
            float peak = 1e-4f;
            for (int i = 0; i < n; i++)
                peak = Mathf.Max(peak, Mathf.Abs(s[i]));
            float gain = 0.8f / peak;

            var bytes = new byte[44 + n * 2];
            void W32(int off, uint val)
            {
                bytes[off] = (byte)val;
                bytes[off + 1] = (byte)(val >> 8);
                bytes[off + 2] = (byte)(val >> 16);
                bytes[off + 3] = (byte)(val >> 24);
            }
            void WTag(int off, string tag)
            {
                for (int i = 0; i < 4; i++)
                    bytes[off + i] = (byte)tag[i];
            }
            WTag(0, "RIFF");
            W32(4, (uint)(36 + n * 2));
            WTag(8, "WAVE");
            WTag(12, "fmt ");
            W32(16, 16);
            bytes[20] = 1; // PCM
            bytes[22] = 1; // mono
            W32(24, SR);
            W32(28, SR * 2);
            bytes[32] = 2;  // block align
            bytes[34] = 16; // bits
            WTag(36, "data");
            W32(40, (uint)(n * 2));
            for (int i = 0; i < n; i++)
            {
                short q = (short)Mathf.RoundToInt(
                    Mathf.Clamp(s[i] * gain, -1f, 1f) * 32767f);
                bytes[44 + i * 2] = (byte)q;
                bytes[44 + i * 2 + 1] = (byte)((ushort)q >> 8);
            }
            File.WriteAllBytes(path, bytes);
        }

        /// Crossfade the trailing `fade` samples onto the head so a looping
        /// AudioSource plays through the seam without a click, then trim.
        static float[] LoopFade(float[] s, int keep, int fade)
        {
            for (int i = 0; i < fade; i++)
            {
                float w = (float)i / fade;
                s[i] = s[i] * w + s[keep + i] * (1f - w);
            }
            var trimmed = new float[keep];
            System.Array.Copy(s, trimmed, keep);
            return trimmed;
        }

        /// 8 s seamless fire-crackle loop: a low-passed rumble bed, an airy
        /// hiss, and a Poisson scatter of exponentially-decaying noise pops.
        internal static float[] FireCrackle()
        {
            const int seconds = 8;
            int n = SR * seconds;
            int fade = SR / 2;
            var s = new float[n + fade];
            var rng = new System.Random(4242);
            float lp = 0f, lpAir = 0f;
            float pop = 0f, popDecay = 0f;
            int nextPop = 0;
            for (int i = 0; i < s.Length; i++)
            {
                float white = (float)(rng.NextDouble() * 2.0 - 1.0);
                lp += 0.015f * (white - lp);        // deep rumble
                lpAir += 0.30f * (white - lpAir);   // airy body
                float hiss = (white - lpAir) * 0.05f;
                if (i >= nextPop)
                {
                    // ~9 pops/second, sizes and decays varied.
                    nextPop = i + SR / 20 + rng.Next(SR / 5);
                    pop = 0.35f + 0.65f * (float)rng.NextDouble();
                    popDecay = Mathf.Exp(-1f / (SR * (0.002f + 0.010f * (float)rng.NextDouble())));
                }
                pop *= popDecay;
                s[i] = lp * 1.6f + lpAir * 0.25f + hiss + white * pop * 0.9f;
            }
            return LoopFade(s, n, fade);
        }

        /// 16 s daytime bed: a light breeze with a slow swell, fluttering
        /// leaf rustle, and a few soft descending bird chirps.
        internal static float[] DayBed()
        {
            const int seconds = 16;
            int n = SR * seconds;
            int fade = SR;
            var s = new float[n + fade];
            var rng = new System.Random(1717);
            float lp = 0f, bpIn = 0f, bp = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                float white = (float)(rng.NextDouble() * 2.0 - 1.0);
                float time = (float)i / SR;
                lp += 0.03f * (white - lp);
                // Band-passed rustle, gated by a fast irregular flutter.
                bpIn += 0.25f * (white - bpIn);
                bp += 0.10f * (bpIn - bp);
                float flutter = Mathf.Max(0f,
                    Mathf.Sin(2f * Mathf.PI * 5f * time)
                    * Mathf.Sin(2f * Mathf.PI * 5f * 3f * time / seconds + 0.8f));
                float swell = 0.6f + 0.4f * Mathf.Sin(2f * Mathf.PI * 2f * time / seconds);
                s[i] = lp * 1.5f * swell + (bpIn - bp) * 0.5f * flutter;
            }
            // Bird chirps: soft frequency-swept blips, a handful per loop,
            // kept off the loop seam.
            var chirpRng = new System.Random(99);
            for (int c = 0; c < 5; c++)
            {
                float start = 1.2f + c * 2.8f + (float)chirpRng.NextDouble();
                int notes = 2 + chirpRng.Next(3);
                float f0 = 2600f + 700f * (float)chirpRng.NextDouble();
                for (int k = 0; k < notes; k++)
                {
                    int at = (int)((start + k * 0.16f) * SR);
                    int len = SR / 9;
                    if (at + len >= n)
                        break;
                    for (int i = 0; i < len; i++)
                    {
                        float t = (float)i / len;
                        float f = f0 * (1.12f - 0.24f * t); // downward sweep
                        float env = Mathf.Sin(Mathf.PI * t);
                        env *= env * 0.055f;
                        s[at + i] += Mathf.Sin(2f * Mathf.PI * f * i / SR) * env;
                    }
                }
            }
            return LoopFade(s, n, fade);
        }

        /// 16 s night bed: a faint cool breeze under two interleaved cricket
        /// voices (pulse-train chirps at different carriers and rhythms).
        internal static float[] NightBed()
        {
            const int seconds = 16;
            int n = SR * seconds;
            int fade = SR;
            var s = new float[n + fade];
            var rng = new System.Random(3131);
            float lp = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                float white = (float)(rng.NextDouble() * 2.0 - 1.0);
                float time = (float)i / SR;
                lp += 0.02f * (white - lp);
                float swell = 0.5f + 0.5f * Mathf.Sin(2f * Mathf.PI * 2f * time / seconds + 2.1f);
                s[i] = lp * 0.9f * swell;
            }
            // Cricket voice: within each `period`-long cycle, `pulses` short
            // AM bursts of the carrier, then silence to the next cycle.
            void Cricket(float carrier, float period, int pulses, float level, float phase)
            {
                for (int i = 0; i < n; i++)
                {
                    float time = (float)i / SR + phase;
                    float inCycle = time % period;
                    float pulseLen = 0.052f;
                    int pulseIdx = (int)(inCycle / pulseLen);
                    if (pulseIdx >= pulses)
                        continue;
                    float t = (inCycle - pulseIdx * pulseLen) / pulseLen;
                    float env = Mathf.Sin(Mathf.PI * t);
                    env *= env * env * level;
                    s[i] += Mathf.Sin(2f * Mathf.PI * carrier * i / SR) * env;
                }
            }
            Cricket(4200f, 0.62f, 3, 0.050f, 0.00f);
            Cricket(3650f, 0.94f, 4, 0.036f, 0.31f);
            return LoopFade(s, n, fade);
        }

        /// The SDK-required VRC_SpatialAudioSource sibling. `spatialize`
        /// false = the Auto-Fix shape for a flat 2D source (component added
        /// disabled); true = a configured spatial emitter.
        internal static void AddVrcSpatial(
            GameObject go, bool spatialize, float gain, float near, float far)
        {
            var t = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCSpatialAudioSource")
                ?? LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_SpatialAudioSource");
            if (t == null)
                return; // no VRChat SDK - nothing to comply with
            var comp = go.GetComponent(t);
            if (comp == null)
                comp = go.AddComponent(t);
            void Set(string field, object value)
            {
                var f = t.GetField(field);
                if (f != null)
                    f.SetValue(comp, value);
            }
            Set("EnableSpatialization", spatialize);
            if (spatialize)
            {
                Set("Gain", gain);
                Set("Near", near);
                Set("Far", far);
            }
            var beh = comp as Behaviour;
            if (beh != null)
                beh.enabled = spatialize;
            EditorUtility.SetDirty(go);
        }
    }
}
