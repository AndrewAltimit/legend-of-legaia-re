// Synthesized ambience clips + VRChat spatial-audio compliance, shared
// by the builder (fire crackle for the camp props) and the realism pass
// (the ambience beds and spatial emitters). Everything here is generated
// from scratch - noise, sines and envelopes - no disc audio.
//
// Design rules the beds follow, because the first generation of these
// clips read as "annoying, too short, fast looping":
//
// - LONG. The beds are 60-100 s, not 12-16 s. A listener standing in the
//   square hears one pass of the loop per couple of minutes.
// - NO AUDIBLE PERIOD. Nothing is driven by a sine LFO at an audible or
//   countable rate and nothing sits on a fixed grid. The slow motion in
//   every bed comes from LoopNoise: multi-octave value noise whose
//   control points wrap circularly over the clip, so the envelope is
//   exactly periodic at the LOOP length (seamless) and has no shorter
//   period at all. Discrete sounds (birds, waves, crickets) are
//   Poisson-scheduled from a seeded RNG with per-event parameter jitter.
// - SEAMLESS. Continuous layers are rendered `fade` samples past the end
//   and crossfaded onto the head (LoopFade); events are splatted with
//   wraparound (Mix), so an owl hoot that starts 0.4 s before the seam
//   finishes on the other side instead of being cut or avoided. There is
//   no event-density dip at the loop point.
// - QUIET. Every clip is normalized to a target RMS with a peak ceiling
//   (WriteWav): the 2D beds land near -18 dBFS so they never mask the
//   BGM, and the sparse spatial clips end up peak-limited and lower.
//
// The beds render at 22050 Hz (BED_SR): the content tops out around
// 5 kHz, and half the samples is half the world's audio memory. The fire
// crackle keeps the original 44100 Hz path so its committed asset is
// unchanged.
//
// User-supplied audio: the realism pass prefers an AudioClip named in
// `Settings/<scene>.settings.json` -> "ambience" over any of these, so a
// better field recording replaces a synthesized role without code.
//
// VRChat deprecation note: the SDK flags every AudioSource that has no
// VRC_SpatialAudioSource sibling ("Found 2D audio source with no VRC
// Spatial Audio component, this is deprecated"). AddVrcSpatial is the
// builder-side version of the SDK's own Auto Fix: a disabled component
// for the flat 2D beds (music, ambience), a configured enabled one for
// genuinely spatial sources (waves, birds, torches).
//
// The generators below are plain float math over System.Random and
// System.IO, deliberately free of UnityEditor and of AudioClip: the
// repo compiles this same file into a small console harness to measure
// duration / RMS / seam continuity / periodicity of every clip outside
// Unity. Keep the #if UNITY_EDITOR guards when editing.

using System.IO;
#if UNITY_EDITOR
using UnityEditor;
#endif
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaAudioGen
    {
        const int SR = 44100;

        /// Sample rate of every ambience bed (the fire crackle stays at SR).
        internal const int BED_SR = 22050;

        // Loop lengths, in seconds. Public so the realism pass and the
        // headless self-test agree on what each asset must import as.
        internal const int BASE_SECONDS = 75;
        internal const int DAY_SECONDS = 100;
        internal const int NIGHT_SECONDS = 100;
        internal const int GUST_SECONDS = 32;
        internal const int WAVES_SECONDS = 90;
        internal const int BIRDS_SECONDS = 52;
        internal const int WILDLIFE_SECONDS = 64;
        internal const int WINDMILL_SECONDS = 12;

        // Level targets. Beds are dense noise (crest factor ~4), so the
        // RMS target is what actually lands; the sparse event clips hit
        // the peak ceiling first and come out quieter still.
        internal const float BED_RMS_DBFS = -18f;
        internal const float EVENT_RMS_DBFS = -20f;
        internal const float PEAK_CEILING = 0.89f;

#if UNITY_EDITOR
        /// Write the wav (if missing), import it, and return the AudioClip.
        internal static AudioClip EnsureClip(string path, System.Func<float[]> gen)
        {
            return EnsureClip(path, gen, SR, 1f, 0.8f);
        }

        /// Level-targeted variant: `rmsDbfs` < 0 normalizes to that RMS with
        /// PEAK_CEILING as the ceiling, `rmsDbfs` >= 0 peak-normalizes to
        /// `peak` (the legacy behaviour). A clip longer than 30 s is also
        /// re-imported as a streaming Vorbis asset - a 100 s bed decompressed
        /// on load would sit in memory as several megabytes for nothing.
        internal static AudioClip EnsureClip(
            string path, System.Func<float[]> gen, int sampleRate,
            float rmsDbfs, float peak)
        {
            if (AssetDatabase.LoadAssetAtPath<AudioClip>(path) == null)
            {
                var samples = gen();
                WriteWav(path, samples, sampleRate, rmsDbfs, peak);
                AssetDatabase.ImportAsset(path);
                ConfigureImport(path, samples.Length > sampleRate * 30);
            }
            var clip = AssetDatabase.LoadAssetAtPath<AudioClip>(path);
            if (clip == null)
                Debug.LogWarning("[Legaia] generated clip failed to import: " + path);
            return clip;
        }

        /// Vorbis + (for the long beds) streaming, loaded in the background:
        /// the ambience layer must not add a multi-megabyte resident cost or
        /// a world-load stall. No-op when the importer already agrees.
        static void ConfigureImport(string path, bool streaming)
        {
            var imp = AssetImporter.GetAtPath(path) as AudioImporter;
            if (imp == null)
                return;
            var s = imp.defaultSampleSettings;
            var wantLoad = streaming
                ? AudioClipLoadType.Streaming
                : AudioClipLoadType.CompressedInMemory;
            if (s.loadType == wantLoad &&
                s.compressionFormat == AudioCompressionFormat.Vorbis &&
                imp.loadInBackground)
                return;
            s.loadType = wantLoad;
            s.compressionFormat = AudioCompressionFormat.Vorbis;
            s.quality = 0.55f;
            // preloadAudioData moved onto the per-platform sample settings
            // in 2022; the AudioImporter property is obsolete (a hard error
            // under this project's warning settings).
            s.preloadAudioData = !streaming;
            imp.defaultSampleSettings = s;
            imp.loadInBackground = true;
            imp.SaveAndReimport();
        }
#endif

        /// Mono 16-bit RIFF writer, peak-normalized to 0.8 at 44100 Hz.
        internal static void WriteWav(string path, float[] s)
        {
            WriteWav(path, s, SR, 1f, 0.8f);
        }

        /// Mono 16-bit RIFF writer. `rmsDbfs` < 0 scales the clip so its RMS
        /// lands there, then backs the gain off if that would push the peak
        /// past `peak` (so a sparse clip of loud events stays peak-limited
        /// and simply ends up quieter). `rmsDbfs` >= 0 peak-normalizes to
        /// `peak` instead.
        internal static void WriteWav(
            string path, float[] s, int sampleRate, float rmsDbfs, float peak)
        {
            int n = s.Length;
            float pk = 1e-6f;
            double sum = 0.0;
            for (int i = 0; i < n; i++)
            {
                float a = s[i];
                if (a < 0f)
                    a = -a;
                if (a > pk)
                    pk = a;
                sum += (double)s[i] * s[i];
            }
            float gain;
            if (rmsDbfs < 0f)
            {
                float rms = (float)System.Math.Sqrt(sum / (n > 0 ? n : 1));
                if (rms < 1e-7f)
                    rms = 1e-7f;
                gain = Mathf.Pow(10f, rmsDbfs / 20f) / rms;
                if (pk * gain > peak)
                    gain = peak / pk;
            }
            else
            {
                gain = peak / pk;
            }

            var bytes = new byte[44 + n * 2];
            void W32(int off, int val)
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
            W32(4, 36 + n * 2);
            WTag(8, "WAVE");
            WTag(12, "fmt ");
            W32(16, 16);
            bytes[20] = 1; // PCM
            bytes[22] = 1; // mono
            W32(24, sampleRate);
            W32(28, sampleRate * 2);
            bytes[32] = 2;  // block align
            bytes[34] = 16; // bits
            WTag(36, "data");
            W32(40, n * 2);
            for (int i = 0; i < n; i++)
            {
                int q = Mathf.RoundToInt(Mathf.Clamp(s[i] * gain, -1f, 1f) * 32767f);
                bytes[44 + i * 2] = (byte)q;
                bytes[44 + i * 2 + 1] = (byte)(q >> 8);
            }
            var dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            File.WriteAllBytes(path, bytes);
        }

        // --- Synthesis primitives -------------------------------------------

        static float Rand(System.Random r)
        {
            return (float)r.NextDouble();
        }

        static float Rand(System.Random r, float lo, float hi)
        {
            return lo + (hi - lo) * (float)r.NextDouble();
        }

        /// White noise in -1..1.
        static float White(System.Random r)
        {
            return (float)(r.NextDouble() * 2.0 - 1.0);
        }

        /// One-pole lowpass coefficient for a cutoff in Hz.
        static float Coeff(float hz, int sr)
        {
            float a = 1f - Mathf.Exp(-2f * Mathf.PI * hz / sr);
            return Mathf.Clamp(a, 1e-5f, 1f);
        }

        /// Loop-periodic multi-octave value noise over `n` samples, in 0..1.
        /// Each octave holds `cells` random control points read circularly
        /// and smoothstep-interpolated, with the cell count derived from the
        /// octave's rate in Hz - so the curve is exactly periodic at the LOOP
        /// length (the seam is continuous) and carries no shorter period for
        /// an ear to latch onto. This is the only slow modulator the beds
        /// use; there is deliberately no sine LFO anywhere.
        static float[] LoopNoise(int n, int sr, System.Random rng,
            float baseHz, int octaves, float persistence)
        {
            var outp = new float[n];
            float amp = 1f, norm = 0f;
            for (int o = 0; o < octaves; o++)
            {
                int cells = Mathf.Max(2,
                    Mathf.RoundToInt(baseHz * (1 << o) * n / sr));
                var v = new float[cells];
                for (int c = 0; c < cells; c++)
                    v[c] = Rand(rng);
                float step = (float)cells / n;
                for (int i = 0; i < n; i++)
                {
                    float x = i * step;
                    int c0 = (int)x;
                    float f = x - c0;
                    f = f * f * (3f - 2f * f);
                    int i0 = c0 % cells;
                    int i1 = (c0 + 1) % cells;
                    outp[i] += amp * (v[i0] + (v[i1] - v[i0]) * f);
                }
                norm += amp;
                amp *= persistence;
            }
            for (int i = 0; i < n; i++)
                outp[i] /= norm;
            return outp;
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

        /// Circular one-pole highpass: two passes over the loop, the second
        /// starting from the first's final state, so the filter is warm at
        /// sample 0 and the seam stays continuous. Clears the sub-audio
        /// rumble a heavily low-passed noise bed accumulates - inaudible
        /// energy that would otherwise eat the headroom.
        static void HighPass(float[] s, float hz, int sr)
        {
            float a = Coeff(hz, sr);
            float y = 0f;
            for (int pass = 0; pass < 2; pass++)
                for (int i = 0; i < s.Length; i++)
                {
                    y += a * (s[i] - y);
                    if (pass == 1)
                        s[i] -= y;
                }
        }

        /// Add a rendered event into the loop at `at`, wrapping past the end
        /// onto the head - the reason events never have to dodge the seam.
        static void Mix(float[] dst, int at, float[] src, float gain)
        {
            int n = dst.Length;
            if (n == 0)
                return;
            int j = at % n;
            if (j < 0)
                j += n;
            for (int i = 0; i < src.Length; i++)
            {
                dst[j] += src[i] * gain;
                if (++j >= n)
                    j = 0;
            }
        }

        /// Raised-cosine attack/decay window over a 0..1 position.
        static float Window(float t, float attack, float release)
        {
            if (t <= 0f || t >= 1f)
                return 0f;
            if (t < attack)
                return 0.5f - 0.5f * Mathf.Cos(Mathf.PI * t / attack);
            if (t > 1f - release)
                return 0.5f - 0.5f * Mathf.Cos(Mathf.PI * (1f - t) / release);
            return 1f;
        }

        /// Poisson-ish gaps: uniform in [lo, hi] seconds, in samples.
        static int Gap(System.Random r, int sr, float lo, float hi)
        {
            int g = Mathf.RoundToInt(Rand(r, lo, hi) * sr);
            return g < 1 ? 1 : g;
        }

        // --- Camp fire ------------------------------------------------------

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
                float white = White(rng);
                lp += 0.015f * (white - lp);        // deep rumble
                lpAir += 0.30f * (white - lpAir);   // airy body
                float hiss = (white - lpAir) * 0.05f;
                if (i >= nextPop)
                {
                    // ~9 pops/second, sizes and decays varied.
                    nextPop = i + SR / 20 + rng.Next(SR / 5);
                    pop = 0.35f + 0.65f * Rand(rng);
                    popDecay = Mathf.Exp(-1f / (SR * (0.002f + 0.010f * Rand(rng))));
                }
                pop *= popDecay;
                s[i] = lp * 1.6f + lpAir * 0.25f + hiss + white * pop * 0.9f;
            }
            return LoopFade(s, n, fade);
        }

        // --- Base bed: wind over distant surf --------------------------------

        /// 75 s wind + distant-surf bed, the layer that always plays. Three
        /// noise bands (deep pressure, a body whose cutoff opens with the
        /// gust, an airy top) under a four-octave gust envelope running from
        /// 0.03 Hz to 0.24 Hz, plus a slower swell driving a surf wash. Every
        /// modulator is LoopNoise, so nothing pulses on a countable beat.
        internal static float[] BaseBed()
        {
            int sr = BED_SR;
            int n = sr * BASE_SECONDS;
            int fade = sr * 4;
            var rng = new System.Random(20260907);
            var gustN = LoopNoise(n, sr, rng, 0.03f, 4, 0.55f);
            var swellN = LoopNoise(n, sr, rng, 0.02f, 3, 0.6f);
            var s = new float[n + fade];

            float deep = 0f, b1 = 0f, b2 = 0f, air = 0f, surf1 = 0f, surf2 = 0f;
            float aDeep = Coeff(28f, sr), aAir = Coeff(2400f, sr);
            float aBody = Coeff(400f, sr), aSurf = Coeff(360f, sr);
            float gust = 0f, swell = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                {
                    // Control-rate: the gust shapes both level and colour.
                    gust = gustN[e];
                    gust = gust * gust * (3f - 2f * gust); // ease the extremes
                    swell = swellN[e];
                    aBody = Coeff(170f + 780f * gust, sr);
                }
                float w = White(rng);
                deep += aDeep * (w - deep);
                b1 += aBody * (w - b1);
                b2 += aBody * (b1 - b2);
                air += aAir * (w - air);
                float hiss = w - air;

                float w2 = White(rng);
                surf1 += aSurf * (w2 - surf1);
                surf2 += aSurf * (surf1 - surf2);

                s[i] = deep * 1.25f
                     + b2 * (0.95f + 1.9f * gust)
                     + hiss * 0.05f * gust * gust
                     + surf2 * (0.55f + 0.9f * swell);
            }
            var bed = LoopFade(s, n, fade);
            HighPass(bed, 38f, sr);
            return bed;
        }

        // --- Day bed ---------------------------------------------------------

        /// 100 s daytime bed: a light breeze whose brightness follows a
        /// four-octave gust, leaf rustle gated by a faster (still noise-
        /// driven) flutter, and a sparse scatter of bird calls drawn from
        /// the same five-species voice bank the spatial tree emitters use,
        /// rendered distant (low-passed, quiet). Roughly 25 calls a minute,
        /// none of them the same shape twice.
        internal static float[] DayBed()
        {
            int sr = BED_SR;
            int n = sr * DAY_SECONDS;
            int fade = sr * 4;
            var rng = new System.Random(1717);
            var gustN = LoopNoise(n, sr, rng, 0.035f, 4, 0.55f);
            var flutN = LoopNoise(n, sr, rng, 0.45f, 3, 0.5f);
            var s = new float[n + fade];

            float b1 = 0f, b2 = 0f, air = 0f, leaf = 0f, leaf2 = 0f, leafHp = 0f;
            float aBody = Coeff(500f, sr), aAir = Coeff(2600f, sr);
            float aLeaf = Coeff(2400f, sr), aLeafHp = Coeff(700f, sr);
            float gust = 0f, flut = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                {
                    gust = gustN[e];
                    flut = flutN[e];
                    flut = flut * flut;
                    aBody = Coeff(260f + 900f * gust, sr);
                }
                float w = White(rng);
                b1 += aBody * (w - b1);
                b2 += aBody * (b1 - b2);
                air += aAir * (w - air);

                float w2 = White(rng);
                leaf += aLeaf * (w2 - leaf);
                leaf2 += aLeaf * (leaf - leaf2);
                leafHp += aLeafHp * (leaf2 - leafHp);
                float rustle = leaf2 - leafHp; // 0.7-2.4 kHz band

                s[i] = b2 * (1.1f + 1.7f * gust)
                     + (w - air) * 0.012f * gust
                     + rustle * (0.45f + 2.1f * flut) * (0.35f + 0.9f * gust);
            }
            var bed = LoopFade(s, n, fade);

            // Distant birds over the whole loop, gaps 1.4-4.2 s.
            var birdRng = new System.Random(770317);
            for (int at = birdRng.Next(n); at < n; at += Gap(birdRng, sr, 1.4f, 4.2f))
                Mix(bed, at, BirdCall(birdRng, sr, 0.55f), 0.55f);
            return bed;
        }

        // --- Night bed -------------------------------------------------------

        /// 100 s night bed: a cool low breeze under a cricket chorus whose
        /// DENSITY drifts on a 0.015 Hz noise envelope (4 to 12 chirps a
        /// second) instead of two fixed voices repeating, plus occasional
        /// frogs and a rare distant owl. Every chirp jitters its carrier,
        /// pulse count and rhythm, so the chorus never lands in step.
        /// The chorus is deliberately thin and low in the mix: the breeze
        /// is the bed, the crickets are what is happening in it.
        internal static float[] NightBed()
        {
            int sr = BED_SR;
            int n = sr * NIGHT_SECONDS;
            int fade = sr * 4;
            var rng = new System.Random(3131);
            var breezeN = LoopNoise(n, sr, rng, 0.025f, 3, 0.6f);
            var s = new float[n + fade];

            float deep = 0f, b1 = 0f, b2 = 0f;
            float aDeep = Coeff(40f, sr), aBody = Coeff(320f, sr);
            float breeze = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                {
                    breeze = breezeN[e];
                    aBody = Coeff(180f + 380f * breeze, sr);
                }
                float w = White(rng);
                deep += aDeep * (w - deep);
                b1 += aBody * (w - b1);
                b2 += aBody * (b1 - b2);
                s[i] = deep * 0.62f + b2 * (0.45f + 0.95f * breeze);
            }
            var bed = LoopFade(s, n, fade);
            HighPass(bed, 38f, sr);

            // Cricket chorus with a drifting density.
            var densN = LoopNoise(n, sr, new System.Random(5150), 0.015f, 3, 0.6f);
            var cr = new System.Random(918273);
            int pos = cr.Next(n);
            while (pos < n)
            {
                float dens = densN[pos];
                float rate = 4f + 8f * dens * dens;   // chirps per second
                Mix(bed, pos, Cricket(cr, sr), 0.75f);
                pos += Mathf.Max(1, Mathf.RoundToInt(
                    sr * (0.4f + 1.2f * Rand(cr)) / rate));
            }
            // Frogs and a rare owl, both wrapping across the seam.
            var fr = new System.Random(6161);
            for (int at = fr.Next(n); at < n; at += Gap(fr, sr, 5f, 13f))
                Mix(bed, at, FrogCroak(fr, sr), 0.7f);
            var ow = new System.Random(3030);
            for (int at = ow.Next(n); at < n; at += Gap(ow, sr, 18f, 34f))
                Mix(bed, at, OwlPhrase(ow, sr), 0.6f);
            return bed;
        }

        // --- Wind gust (windy weather) ----------------------------------------

        /// 32 s of stronger gusts for the weather layer: a broadband body
        /// whose cutoff and level track a fast (0.12-0.5 Hz) four-octave gust
        /// envelope, plus two resonant band-passed howls whose centres slide
        /// with the gust and beat against each other, plus a buffeting low
        /// rumble. Louder and busier than the base bed by design - the mixer
        /// fades it in on windLevel.
        internal static float[] WindGust()
        {
            int sr = BED_SR;
            int n = sr * GUST_SECONDS;
            int fade = sr * 3;
            var rng = new System.Random(60613);
            var gN = LoopNoise(n, sr, rng, 0.12f, 4, 0.55f);
            var hN = LoopNoise(n, sr, rng, 0.07f, 3, 0.6f);
            var s = new float[n + fade];

            float b1 = 0f, b2 = 0f, low = 0f;
            float aBody = Coeff(800f, sr), aLow = Coeff(60f, sr);
            // Two state-variable bandpasses = the howl.
            float lo1 = 0f, ba1 = 0f, lo2 = 0f, ba2 = 0f;
            float f1 = 0.1f, f2 = 0.1f;
            const float q = 0.16f;
            float g = 0f, h = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 31) == 0)
                {
                    g = gN[e];
                    g = g * g;
                    h = hN[e];
                    aBody = Coeff(320f + 2600f * g, sr);
                    f1 = 2f * Mathf.Sin(Mathf.PI * (430f + 620f * g) / sr);
                    f2 = 2f * Mathf.Sin(Mathf.PI * (760f + 900f * h) / sr);
                }
                float w = White(rng);
                b1 += aBody * (w - b1);
                b2 += aBody * (b1 - b2);
                low += aLow * (w - low);

                float w2 = White(rng) * 0.5f;
                lo1 += f1 * ba1;
                ba1 += f1 * (w2 - lo1 - q * ba1);
                lo2 += f2 * ba2;
                ba2 += f2 * (w2 - lo2 - q * ba2);

                s[i] = b2 * (0.8f + 2.2f * g)
                     + low * 2.2f * (0.5f + g)
                     + (ba1 * 0.30f + ba2 * 0.20f) * g * g;
            }
            return LoopFade(s, n, fade);
        }

        // --- Shore waves (spatial) --------------------------------------------

        /// 90 s of individual wave washes for the water-edge emitters: a low
        /// sea rumble under wave cycles spaced 6-12 s apart, each one a swell
        /// rise, a bright break with a mid thump, and a granular retreat that
        /// sweeps its lowpass down as it fizzles. Wave size, timing and the
        /// three phase lengths all jitter per event, and events wrap the
        /// seam, so no two passes of the loop line up.
        internal static float[] ShoreWaves()
        {
            int sr = BED_SR;
            int n = sr * WAVES_SECONDS;
            int fade = sr * 3;
            var rng = new System.Random(9021);
            var seaN = LoopNoise(n, sr, rng, 0.02f, 3, 0.6f);
            var s = new float[n + fade];
            float d1 = 0f, d2 = 0f;
            float aSea = Coeff(150f, sr);
            float sea = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                    sea = 0.45f + 0.55f * seaN[e];
                float w = White(rng);
                d1 += aSea * (w - d1);
                d2 += aSea * (d1 - d2);
                s[i] = d2 * 0.55f * sea;
            }
            var bed = LoopFade(s, n, fade);
            HighPass(bed, 34f, sr);

            var wr = new System.Random(31337);
            for (int at = wr.Next(n); at < n; at += Gap(wr, sr, 6f, 12f))
                Mix(bed, at, Wave(wr, sr), 1f);
            return bed;
        }

        /// One wave: rise, break, retreat.
        static float[] Wave(System.Random r, int sr)
        {
            float rise = Rand(r, 1.4f, 2.6f);
            float brk = Rand(r, 1.1f, 2.0f);
            float ret = Rand(r, 2.0f, 3.6f);
            float size = Rand(r, 0.5f, 1f);
            int nR = Mathf.RoundToInt(sr * rise);
            int nB = Mathf.RoundToInt(sr * brk);
            int nT = Mathf.RoundToInt(sr * ret);
            var e = new float[nR + nB + nT];

            float lo = 0f, lo2 = 0f, hp = 0f, gr = 0f;
            float aLo = Coeff(420f, sr), aGr = Coeff(9f, sr);
            float aHp = Coeff(1500f, sr);
            // Rise: a swelling low-mid wash.
            for (int i = 0; i < nR; i++)
            {
                float t = (float)i / nR;
                float w = White(r);
                lo += aLo * (w - lo);
                lo2 += aLo * (lo - lo2);
                e[i] += lo2 * 1.1f * t * t * size;
            }
            // Break: bright hiss with a fast attack over a mid thump.
            float thump = 0f, aTh = Coeff(110f, sr);
            for (int i = 0; i < nB; i++)
            {
                float t = (float)i / nB;
                float env = t < 0.12f
                    ? 0.5f - 0.5f * Mathf.Cos(Mathf.PI * t / 0.12f)
                    : Mathf.Exp(-2.6f * (t - 0.12f));
                float w = White(r);
                hp += aHp * (w - hp);
                float top = w - hp;
                thump += aTh * (w - thump);
                e[nR + i] += (top * 0.95f + thump * 0.55f) * env * size;
            }
            // Retreat: granular fizz with a closing lowpass.
            float rl = 0f, rl2 = 0f;
            for (int i = 0; i < nT; i++)
            {
                float t = (float)i / nT;
                float aR = Coeff(3400f - 2600f * t, sr);
                float w = White(r);
                rl += aR * (w - rl);
                rl2 += aR * (rl - rl2);
                gr += aGr * (Mathf.Abs(White(r)) - gr);
                float env = Mathf.Exp(-2.2f * t) * (1f - t);
                e[nR + nB + i] += (rl - rl2) * (0.6f + 2.2f * gr) * env * 1.7f * size;
            }
            // Soft knee: a single break's transient would otherwise set the
            // whole loop's peak and drag every other wave down with it.
            for (int i = 0; i < e.Length; i++)
            {
                float v = e[i];
                e[i] = v / (1f + 0.9f * (v < 0f ? -v : v));
            }
            return e;
        }

        // --- Birds (spatial, daytime) -----------------------------------------

        /// 52 s of daytime tree-bird activity for the canopy emitters: a very
        /// quiet leaf bed under Poisson calls from five voices - a swept
        /// multi-note chirp, a trill (fast AM of a carrier), a two-note
        /// whistle, a soft peep, and a rare distant crow caw. Pitch, note
        /// count, rhythm and distance jitter per call, so nothing is one
        /// chirp on repeat.
        internal static float[] TreeBirds()
        {
            int sr = BED_SR;
            int n = sr * BIRDS_SECONDS;
            int fade = sr * 2;
            var rng = new System.Random(4400);
            var leafN = LoopNoise(n, sr, rng, 0.06f, 3, 0.55f);
            var s = new float[n + fade];
            float l1 = 0f, l2 = 0f;
            float aL = Coeff(3000f, sr), aH = Coeff(800f, sr);
            float lf = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                    lf = leafN[e];
                float w = White(rng);
                l1 += aL * (w - l1);
                l2 += aH * (l1 - l2);
                s[i] = (l1 - l2) * 0.22f * (0.3f + 1.1f * lf);
            }
            var bed = LoopFade(s, n, fade);

            var br = new System.Random(51515);
            for (int at = br.Next(n); at < n; at += Gap(br, sr, 0.9f, 4.0f))
                Mix(bed, at, BirdCall(br, sr, 1f), 1f);
            return bed;
        }

        /// One bird call, species chosen at random. `near` 1 = close and
        /// bright, lower = distant (quieter, low-passed).
        static float[] BirdCall(System.Random r, int sr, float near)
        {
            float pick = Rand(r);
            float[] e;
            if (pick < 0.34f)
                e = BirdChirp(r, sr);
            else if (pick < 0.56f)
                e = BirdTrill(r, sr);
            else if (pick < 0.76f)
                e = BirdWhistle(r, sr);
            else if (pick < 0.93f)
                e = BirdPeep(r, sr);
            else
                e = CrowCaw(r, sr);
            float dist = near * Rand(r, 0.45f, 1f);
            if (dist < 0.98f)
            {
                // Distance: quieter and duller (one-pole, cutoff falls off).
                float a = Coeff(1200f + 6000f * dist, sr);
                float y = 0f;
                for (int i = 0; i < e.Length; i++)
                {
                    y += a * (e[i] - y);
                    e[i] = y * dist;
                }
            }
            return e;
        }

        /// Multi-note swept chirp: 2-5 notes, each a short frequency sweep
        /// (down, up or arched), with the whole phrase transposed per call.
        static float[] BirdChirp(System.Random r, int sr)
        {
            int notes = 2 + r.Next(4);
            float f0 = Rand(r, 2300f, 4600f);
            float gapS = Rand(r, 0.075f, 0.19f);
            float noteS = Rand(r, 0.045f, 0.10f);
            int gapN = Mathf.RoundToInt(sr * gapS);
            int noteN = Mathf.RoundToInt(sr * noteS);
            var e = new float[gapN * (notes - 1) + noteN + 8];
            int shape = r.Next(3);
            for (int k = 0; k < notes; k++)
            {
                float fk = f0 * Mathf.Pow(Rand(r, 0.94f, 1.07f), 1f) *
                           (1f - 0.035f * k);
                float span = Rand(r, 0.16f, 0.34f);
                float ph = 0f;
                for (int i = 0; i < noteN; i++)
                {
                    float t = (float)i / noteN;
                    float bend = shape == 0 ? (1f + span * 0.5f - span * t)
                        : shape == 1 ? (1f - span * 0.5f + span * t)
                        : (1f + span * Mathf.Sin(Mathf.PI * t) * 0.7f);
                    ph += 2f * Mathf.PI * fk * bend / sr;
                    float env = Window(t, 0.25f, 0.45f);
                    e[k * gapN + i] += (Mathf.Sin(ph)
                        + 0.16f * Mathf.Sin(ph * 2f)) * env * 0.42f;
                }
            }
            return e;
        }

        /// Trill: a carrier under fast amplitude modulation (the AM rate is
        /// per-call, 20-46 Hz - a rate, not a musical pulse).
        static float[] BirdTrill(System.Random r, int sr)
        {
            float dur = Rand(r, 0.35f, 0.95f);
            int len = Mathf.RoundToInt(sr * dur);
            float f = Rand(r, 2700f, 4400f);
            float am = Rand(r, 20f, 46f);
            float drift = Rand(r, -0.12f, 0.14f);
            var e = new float[len];
            float ph = 0f, aph = 0f;
            for (int i = 0; i < len; i++)
            {
                float t = (float)i / len;
                ph += 2f * Mathf.PI * f * (1f + drift * t) / sr;
                aph += 2f * Mathf.PI * am / sr;
                float mod = 0.5f + 0.5f * Mathf.Sin(aph);
                mod *= mod;
                e[i] = (Mathf.Sin(ph) + 0.2f * Mathf.Sin(ph * 2.01f))
                       * mod * Window(t, 0.12f, 0.3f) * 0.34f;
            }
            return e;
        }

        /// Two-note whistle with a light vibrato on each note.
        static float[] BirdWhistle(System.Random r, int sr)
        {
            float d1 = Rand(r, 0.16f, 0.30f);
            float d2 = Rand(r, 0.16f, 0.34f);
            float gap = Rand(r, 0.05f, 0.16f);
            float f1 = Rand(r, 1900f, 3100f);
            float f2 = f1 * Rand(r, 0.72f, 1.38f);
            int n1 = Mathf.RoundToInt(sr * d1);
            int n2 = Mathf.RoundToInt(sr * d2);
            int ng = Mathf.RoundToInt(sr * gap);
            var e = new float[n1 + ng + n2];
            float vib = Rand(r, 4.5f, 7.5f), depth = Rand(r, 0.006f, 0.02f);
            void Note(int at, int len, float f)
            {
                float ph = 0f;
                for (int i = 0; i < len; i++)
                {
                    float t = (float)i / len;
                    float m = 1f + depth * Mathf.Sin(2f * Mathf.PI * vib * i / sr);
                    ph += 2f * Mathf.PI * f * m / sr;
                    e[at + i] += (Mathf.Sin(ph) + 0.12f * Mathf.Sin(ph * 3f))
                                 * Window(t, 0.2f, 0.35f) * 0.4f;
                }
            }
            Note(0, n1, f1);
            Note(n1 + ng, n2, f2);
            return e;
        }

        /// A single short high peep.
        static float[] BirdPeep(System.Random r, int sr)
        {
            int reps = 1 + r.Next(3);
            float f = Rand(r, 3600f, 5400f);
            int len = Mathf.RoundToInt(sr * Rand(r, 0.035f, 0.07f));
            int step = Mathf.RoundToInt(sr * Rand(r, 0.10f, 0.24f));
            var e = new float[step * (reps - 1) + len + 4];
            for (int k = 0; k < reps; k++)
            {
                float ph = 0f;
                float fk = f * Rand(r, 0.97f, 1.03f);
                for (int i = 0; i < len; i++)
                {
                    float t = (float)i / len;
                    ph += 2f * Mathf.PI * fk * (1.06f - 0.12f * t) / sr;
                    e[k * step + i] += Mathf.Sin(ph) * Window(t, 0.3f, 0.5f) * 0.3f;
                }
            }
            return e;
        }

        /// Distant crow: a low buzzy burst (harmonic stack with a rasp), 2-3
        /// caws. Deliberately rare in the draw.
        static float[] CrowCaw(System.Random r, int sr)
        {
            int caws = 2 + r.Next(2);
            float f = Rand(r, 360f, 520f);
            int len = Mathf.RoundToInt(sr * Rand(r, 0.22f, 0.36f));
            int step = Mathf.RoundToInt(sr * Rand(r, 0.32f, 0.52f));
            var e = new float[step * (caws - 1) + len + 4];
            for (int k = 0; k < caws; k++)
            {
                float fk = f * Rand(r, 0.94f, 1.07f);
                float ph = 0f, rasp = 0f;
                float aR = Coeff(2000f, sr);
                for (int i = 0; i < len; i++)
                {
                    float t = (float)i / len;
                    ph += 2f * Mathf.PI * fk * (1.05f - 0.14f * t) / sr;
                    // Band-limited saw-ish stack (6 partials, 1/h taper).
                    float v = 0f;
                    for (int h = 1; h <= 6; h++)
                        v += Mathf.Sin(ph * h) / h;
                    rasp += aR * (White(r) - rasp);
                    float env = Window(t, 0.08f, 0.55f);
                    e[k * step + i] += (v * 0.34f + rasp * 0.22f) * env * 0.32f;
                }
            }
            return e;
        }

        // --- Night wildlife (spatial) -----------------------------------------

        /// 64 s of night wildlife for the tree/water emitters: owl hoots
        /// (low sine with a breathy noise skirt), frog croaks (pulse trains
        /// at a per-croak rate), and crickets whose density drifts on a slow
        /// LoopNoise - a different, sparser mix than the 2D night bed so the
        /// two do not read as the same clip twice.
        internal static float[] NightWildlife()
        {
            int sr = BED_SR;
            int n = sr * WILDLIFE_SECONDS;
            int fade = sr * 2;
            var rng = new System.Random(7788);
            var airN = LoopNoise(n, sr, rng, 0.03f, 3, 0.6f);
            var s = new float[n + fade];
            float a1 = 0f, a2 = 0f;
            float aA = Coeff(240f, sr);
            float air = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                int e = i < n ? i : i - n;
                if ((i & 63) == 0)
                    air = airN[e];
                float w = White(rng);
                a1 += aA * (w - a1);
                a2 += aA * (a1 - a2);
                s[i] = a2 * 0.30f * (0.3f + 0.9f * air);
            }
            var bed = LoopFade(s, n, fade);

            var densN = LoopNoise(n, sr, new System.Random(2244), 0.02f, 3, 0.6f);
            var cr = new System.Random(135791);
            int pos = cr.Next(n);
            while (pos < n)
            {
                float rate = 1.0f + 2.6f * densN[pos];
                Mix(bed, pos, Cricket(cr, sr), 0.6f);
                pos += Mathf.Max(1, Mathf.RoundToInt(
                    sr * (0.4f + 1.2f * Rand(cr)) / rate));
            }
            var ow = new System.Random(2468);
            for (int at = ow.Next(n); at < n; at += Gap(ow, sr, 9f, 19f))
                Mix(bed, at, OwlPhrase(ow, sr), 0.75f);
            var fr = new System.Random(1379);
            for (int at = fr.Next(n); at < n; at += Gap(fr, sr, 4f, 11f))
                Mix(bed, at, FrogCroak(fr, sr), 0.40f);
            return bed;
        }

        /// One cricket chirp: 3-5 AM pulses of a jittered carrier.
        ///
        /// Tuned DOWN and SOFT on purpose. The first version sat at
        /// 3.5-4.9 kHz with a second harmonic a quarter as loud, which
        /// puts its energy - and a sizzling octave above it - straight
        /// through the ear's most sensitive band; over a whole night that
        /// reads as harsh rather than as summer. It now sings around
        /// 2.3-3.2 kHz (the low end of a real field cricket), the
        /// harmonic is a hint rather than a layer, the per-pulse envelope
        /// is squared rather than cubed so each pulse swells instead of
        /// snapping, and the level is roughly half. Quiet enough to be
        /// atmosphere; a night should be something you can talk over.
        static float[] Cricket(System.Random r, int sr)
        {
            int pulses = 3 + r.Next(3);
            float f = Rand(r, 2300f, 3200f);
            float pl = Rand(r, 0.034f, 0.060f);
            float gap = pl + Rand(r, 0.010f, 0.026f);
            int lenP = Mathf.RoundToInt(sr * pl);
            int stepP = Mathf.RoundToInt(sr * gap);
            var e = new float[stepP * (pulses - 1) + lenP + 4];
            float level = Rand(r, 0.05f, 0.15f);
            for (int k = 0; k < pulses; k++)
            {
                float fk = f * Rand(r, 0.985f, 1.015f);
                float ph = 0f;
                for (int i = 0; i < lenP; i++)
                {
                    float t = (float)i / lenP;
                    ph += 2f * Mathf.PI * fk / sr;
                    float env = Mathf.Sin(Mathf.PI * t);
                    env *= env;
                    e[k * stepP + i] += (Mathf.Sin(ph) + 0.07f * Mathf.Sin(ph * 2f))
                                        * env * level;
                }
            }
            return e;
        }

        /// Owl: 2-4 hoots, each a low sine with a second harmonic, a breathy
        /// noise skirt and a small downward drift at the tail.
        static float[] OwlPhrase(System.Random r, int sr)
        {
            int hoots = 2 + r.Next(3);
            float f = Rand(r, 290f, 430f);
            float dur = Rand(r, 0.30f, 0.48f);
            int len = Mathf.RoundToInt(sr * dur);
            int step = Mathf.RoundToInt(sr * (dur + Rand(r, 0.28f, 0.65f)));
            var e = new float[step * (hoots - 1) + len + 4];
            for (int k = 0; k < hoots; k++)
            {
                float fk = f * Rand(r, 0.97f, 1.04f) * (1f - 0.02f * k);
                float ph = 0f, br = 0f;
                float aB = Coeff(700f, sr);
                for (int i = 0; i < len; i++)
                {
                    float t = (float)i / len;
                    ph += 2f * Mathf.PI * fk * (1.015f - 0.05f * t * t) / sr;
                    br += aB * (White(r) - br);
                    float env = Window(t, 0.22f, 0.45f);
                    e[k * step + i] += (Mathf.Sin(ph) * 0.8f
                        + 0.13f * Mathf.Sin(ph * 2f) + br * 0.22f) * env * 0.30f;
                }
            }
            return e;
        }

        /// Frog: a pulse train (6-16 pulses at a per-croak 16-32 Hz rate) of
        /// a short harmonic burst, under a swelling phrase envelope.
        static float[] FrogCroak(System.Random r, int sr)
        {
            int pulses = 6 + r.Next(11);
            float rate = Rand(r, 16f, 32f);
            float f = Rand(r, 130f, 260f);
            int step = Mathf.RoundToInt(sr / rate);
            int len = Mathf.Max(4, Mathf.RoundToInt(step * Rand(r, 0.45f, 0.75f)));
            var e = new float[step * (pulses - 1) + len + 4];
            float level = Rand(r, 0.16f, 0.34f);
            for (int k = 0; k < pulses; k++)
            {
                float phrase = Mathf.Sin(Mathf.PI * (k + 0.5f) / pulses);
                float fk = f * (1f + 0.06f * phrase);
                float ph = 0f;
                for (int i = 0; i < len; i++)
                {
                    float t = (float)i / len;
                    ph += 2f * Mathf.PI * fk / sr;
                    float v = Mathf.Sin(ph) + 0.5f * Mathf.Sin(ph * 2f)
                              + 0.28f * Mathf.Sin(ph * 3f);
                    e[k * step + i] += v * Mathf.Exp(-5.5f * t) * phrase * level * 0.5f;
                }
            }
            return e;
        }

        // --- Windmill (spatial, on the prop) -----------------------------------

        /// 12 s windmill loop = three 4 s rotations, four blade passes each.
        /// Periodic on purpose (it is a machine), but the pass level, the
        /// whoosh colour and the per-rotation timber creak all vary across
        /// the twelve passes, so the ear hears a turning mill rather than one
        /// second of audio on repeat.
        internal static float[] WindmillLoop()
        {
            int sr = BED_SR;
            int n = sr * WINDMILL_SECONDS;
            int fade = sr / 2;
            var rng = new System.Random(1360);
            var s = new float[n + fade];
            float ax1 = 0f, ax2 = 0f;
            float aAx = Coeff(90f, sr);
            for (int i = 0; i < s.Length; i++)
            {
                float w = White(rng);
                ax1 += aAx * (w - ax1);
                ax2 += aAx * (ax1 - ax2);
                s[i] = ax2 * 1.1f; // axle rumble
            }
            var loop = LoopFade(s, n, fade);

            // Blade passes: one per second (4 s rotation, 4 blades).
            var pr = new System.Random(97531);
            for (int k = 0; k < WINDMILL_SECONDS * 4; k++)
            {
                int at = Mathf.RoundToInt(sr * (k * 0.25f * 4f / 4f))
                         + Mathf.RoundToInt(sr * Rand(pr, -0.012f, 0.012f));
                Mix(loop, at, Whoosh(pr, sr), 1f);
            }
            // One timber creak per rotation, at a different phase each time.
            var cr = new System.Random(24680);
            for (int k = 0; k < WINDMILL_SECONDS / 4; k++)
                Mix(loop, Mathf.RoundToInt(sr * (k * 4f + Rand(cr, 0.4f, 3.4f))),
                    Creak(cr, sr), 1f);
            return loop;
        }

        /// One blade pass: a band-passed noise swell.
        static float[] Whoosh(System.Random r, int sr)
        {
            int len = Mathf.RoundToInt(sr * Rand(r, 0.42f, 0.60f));
            var e = new float[len];
            float lo = 0f, hi = 0f, level = Rand(r, 0.7f, 1f);
            float aLo = Coeff(Rand(r, 380f, 620f), sr), aHi = Coeff(90f, sr);
            for (int i = 0; i < len; i++)
            {
                float t = (float)i / len;
                float w = White(r);
                lo += aLo * (w - lo);
                hi += aHi * (lo - hi);
                float env = Mathf.Sin(Mathf.PI * t);
                e[i] = (lo - hi) * env * env * 2.6f * level;
            }
            return e;
        }

        /// Wooden creak: a low harmonic stack with a wobbling pitch and a
        /// stick-slip amplitude ripple.
        static float[] Creak(System.Random r, int sr)
        {
            int len = Mathf.RoundToInt(sr * Rand(r, 0.28f, 0.55f));
            var e = new float[len];
            float f = Rand(r, 150f, 280f);
            float wob = Rand(r, 7f, 16f);
            float ph = 0f;
            for (int i = 0; i < len; i++)
            {
                float t = (float)i / len;
                float m = 1f + 0.10f * Mathf.Sin(2f * Mathf.PI * wob * i / sr)
                          + 0.25f * t;
                ph += 2f * Mathf.PI * f * m / sr;
                float rip = 0.55f + 0.45f * Mathf.Sin(2f * Mathf.PI * wob * 2.3f * i / sr);
                float v = Mathf.Sin(ph) + 0.4f * Mathf.Sin(ph * 2f)
                          + 0.22f * Mathf.Sin(ph * 3.02f);
                e[i] = v * rip * Window(t, 0.15f, 0.5f) * 0.16f;
            }
            return e;
        }

#if UNITY_EDITOR
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
#endif
    }
}
