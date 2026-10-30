#!/usr/bin/env python3
"""Generate the Rebirth pack's own audio: a short tower drone that loops
seamlessly, torch, water, wind, crowd, engine and drip loops, a telegraph
chime, and the boss stings (an entrance hit, a phase-change shimmer and a
falling knell).

Everything is synthesised here from sine partials and seeded noise, so the
files are the pack's own work and the same script always writes the same
bytes. The drone carries its loop as a WAV `smpl` chunk; the engine reads it
(docs/formats/expansion-pack-audio.md).

Usage: generate_rebirth_audio.py OUT_DIR
"""

import math
import os
import random
import struct
import sys


def wav(path, rate, samples, loop=None):
    """Write mono 16-bit PCM, with a `smpl` loop [start, end) when given."""
    data = b"".join(struct.pack("<h", max(-32767, min(32767, int(round(s * 32767))))) for s in samples)
    fmt = struct.pack("<HHIIHH", 1, 1, rate, rate * 2, 2, 16)
    chunks = b"fmt " + struct.pack("<I", len(fmt)) + fmt
    if loop:
        s, e = loop
        smpl = struct.pack("<9I", 0, 0, 1_000_000_000 // rate, 60, 0, 0, 0, 1, 0)
        smpl += struct.pack("<6I", 0, 0, s, e - 1, 0, 0)
        chunks += b"smpl" + struct.pack("<I", len(smpl)) + smpl
    chunks += b"data" + struct.pack("<I", len(data)) + data
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 4 + len(chunks)) + b"WAVE" + chunks)


def normalise(samples, peak):
    top = max(abs(s) for s in samples) or 1.0
    return [s * peak / top for s in samples]


def drone(rate=22050, seconds=6.0):
    """A minor-ninth pad over a low fifth, every partial a whole number of
    cycles over the loop so the end meets the start without a seam."""
    n = int(rate * seconds)
    # (cycles over the loop, level): A1 / E2 under an A minor add-9 voicing,
    # snapped to whole cycles, plus slow beating pairs for movement.
    partials = []
    for hz, level in [(55.0, 0.5), (82.4, 0.32), (110.0, 0.22), (130.8, 0.16), (164.8, 0.14), (246.9, 0.07)]:
        k = round(hz * seconds)
        partials.append((k, level))
        partials.append((k + 1, level * 0.45))  # a beating twin, ~0.17 Hz apart
    out = []
    for i in range(n):
        t = i / n
        swell = 0.75 + 0.25 * math.sin(2 * math.pi * 2 * t)  # two swells a loop
        v = sum(level * math.sin(2 * math.pi * k * t) for k, level in partials)
        out.append(v * swell)
    return normalise(out, 0.55), (0, n)


def loop_noise(rng, n, fade):
    """White noise whose tail is cross-faded into its head, so it loops."""
    raw = [rng.uniform(-1.0, 1.0) for _ in range(n + fade)]
    out = raw[:n]
    for i in range(fade):
        a = i / fade
        out[i] = raw[i] * a + raw[n + i] * (1 - a)
    return out


def torch(rate=16000, seconds=2.0, seed=7):
    """Low rumble plus sparse sharp crackles."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    noise = loop_noise(rng, n, rate // 10)
    out, lp = [], 0.0
    for s in noise:
        lp += (s - lp) * 0.04  # a soft low-passed roar
        out.append(lp * 2.5)
    for _ in range(int(seconds * 18)):
        at = rng.randrange(n)
        amp = rng.uniform(0.25, 0.8)
        length = rng.randrange(40, 220)
        for j in range(length):
            k = (at + j) % n
            out[k] += amp * rng.uniform(-1, 1) * (1 - j / length) ** 3
    return normalise(out, 0.6), None


def water(rate=16000, seconds=3.0, seed=11):
    """Band-limited noise with a slow babble of loudness."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    noise = loop_noise(rng, n, rate // 5)
    out, a, b = [], 0.0, 0.0
    for i, s in enumerate(noise):
        a += (s - a) * 0.18
        b += (a - b) * 0.05
        band = a - b  # a band-pass between the two one-pole filters
        t = i / n
        babble = 0.6 + 0.25 * math.sin(2 * math.pi * 3 * t) + 0.15 * math.sin(2 * math.pi * 7 * t + 1.3)
        out.append(band * babble)
    return normalise(out, 0.6), None


def chime(rate=22050, seconds=1.2):
    """A bell-like strike: inharmonic partials under a fast decay."""
    n = int(rate * seconds)
    partials = [(880.0, 1.0, 3.0), (1756.0, 0.5, 4.5), (2390.0, 0.3, 6.0), (3520.0, 0.15, 8.0)]
    out = []
    for i in range(n):
        t = i / rate
        attack = min(1.0, t / 0.004)
        v = sum(level * math.exp(-decay * t) * math.sin(2 * math.pi * hz * t) for hz, level, decay in partials)
        out.append(v * attack)
    return normalise(out, 0.7), None


def wind(rate=16000, seconds=4.0, seed=23):
    """Low noise through a slowly sweeping band, in gusts: the open top of
    the tower."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    noise = loop_noise(rng, n, rate // 4)
    out, a, b = [], 0.0, 0.0
    for i, s in enumerate(noise):
        t = i / n
        # The band's corner sweeps twice a loop; the gusts swell three times.
        k = 0.02 + 0.03 * (0.5 + 0.5 * math.sin(2 * math.pi * 2 * t))
        a += (s - a) * k
        b += (a - b) * 0.004
        gust = 0.45 + 0.35 * math.sin(2 * math.pi * 3 * t + 0.7) ** 2 + 0.2 * math.sin(2 * math.pi * 5 * t)
        out.append((a - b) * gust)
    return normalise(out, 0.6), None


def crowd(rate=16000, seconds=4.0, seed=29):
    """A room of murmuring voices: band-passed noise per voice, each with its
    own syllable rhythm, and the odd clink of a glass."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    out = [0.0] * n
    for v in range(9):
        noise = loop_noise(rng, n, rate // 8)
        a = b = 0.0
        ka = rng.uniform(0.10, 0.22)
        rate_hz = rng.choice([2, 3, 4, 5])  # whole syllables a loop: seamless
        phase = rng.uniform(0, 2 * math.pi)
        level = rng.uniform(0.5, 1.0)
        for i, s in enumerate(noise):
            a += (s - a) * ka
            b += (a - b) * 0.03
            t = i / n
            env = max(0.0, math.sin(2 * math.pi * rate_hz * seconds / 4 * t * 4 + phase)) ** 1.5
            out[i] += (a - b) * env * level
    for _ in range(5):
        at = rng.randrange(n)
        hz = rng.uniform(2400, 3400)
        for j in range(int(rate * 0.3)):
            k = (at + j) % n
            tt = j / rate
            out[k] += 0.18 * math.exp(-tt * 18) * math.sin(2 * math.pi * hz * tt)
    return normalise(out, 0.55), None


def engine(rate=16000, seconds=2.0, seed=31):
    """The Engine Deep: a low hum on whole cycles and a clank twice a loop."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    out = []
    for i in range(n):
        t = i / n
        v = 0.5 * math.sin(2 * math.pi * round(48 * seconds) * t)
        v += 0.25 * math.sin(2 * math.pi * round(96 * seconds) * t)
        v += 0.12 * math.sin(2 * math.pi * round(145 * seconds) * t)
        out.append(v * (0.8 + 0.2 * math.sin(2 * math.pi * 4 * t)))
    for at in (0, n // 2):
        for j in range(int(rate * 0.25)):
            k = (at + j) % n
            tt = j / rate
            out[k] += 0.6 * math.exp(-tt * 22) * (rng.uniform(-1, 1) * 0.6 + math.sin(2 * math.pi * 310 * tt))
    return normalise(out, 0.6), None


def drips(rate=16000, seconds=3.0, seed=37):
    """A cave's drips: short pitched plinks, sparse, over a hush."""
    rng = random.Random(seed)
    n = int(rate * seconds)
    noise = loop_noise(rng, n, rate // 5)
    out, a = [], 0.0
    for s in noise:
        a += (s - a) * 0.02
        out.append(a * 0.5)
    for _ in range(7):
        at = rng.randrange(n)
        hz = rng.uniform(900, 1700)
        for j in range(int(rate * 0.2)):
            k = (at + j) % n
            tt = j / rate
            glide = hz * (1 + 0.6 * math.exp(-tt * 40))
            out[k] += 0.7 * math.exp(-tt * 30) * math.sin(2 * math.pi * glide * tt)
    return normalise(out, 0.6), None


def sting_boss(rate=22050, seconds=2.6):
    """A boss's entrance: a low drum hit and a minor chord of buzzing reeds
    that swells and cuts off."""
    rng = random.Random(41)
    n = int(rate * seconds)
    out = []
    chord = [55.0, 65.4, 82.4, 110.0, 130.8]
    for i in range(n):
        t = i / rate
        hit = math.exp(-t * 7) * (math.sin(2 * math.pi * (60 - 25 * t) * t) + 0.3 * rng.uniform(-1, 1) * math.exp(-t * 30))
        swell = min(1.0, t / 0.6) * (1.0 if t < 2.0 else max(0.0, 1 - (t - 2.0) / 0.6))
        reed = 0.0
        for k, hz in enumerate(chord):
            # A few odd harmonics: a reedy, brass-like edge.
            for h in (1, 3, 5):
                reed += math.sin(2 * math.pi * hz * h * t * (1 + 0.002 * k)) / (h * 1.6)
        out.append(hit * 1.1 + reed * 0.16 * swell)
    return normalise(out, 0.75), None


def sting_phase(rate=22050, seconds=1.6):
    """A phase change: a struck cluster over a rising shimmer."""
    n = int(rate * seconds)
    out = []
    for i in range(n):
        t = i / rate
        rise = 440 * 2 ** (t * 1.2)
        shimmer = sum(math.sin(2 * math.pi * rise * m * t) / m for m in (1, 1.5, 2.01)) * min(1.0, t / 0.2) * math.exp(-t * 1.6)
        strike = math.exp(-t * 5) * sum(math.sin(2 * math.pi * hz * t) for hz in (220.0, 233.1, 329.6))
        out.append(shimmer * 0.4 + strike * 0.5)
    return normalise(out, 0.7), None


def sting_fall(rate=22050, seconds=3.5):
    """A boss falls: a deep knell and a falling three-note line."""
    n = int(rate * seconds)
    out = []
    notes = [(0.0, 392.0), (0.45, 329.6), (0.9, 261.6)]
    for i in range(n):
        t = i / rate
        knell = math.exp(-t * 1.1) * (math.sin(2 * math.pi * 98 * t) + 0.5 * math.sin(2 * math.pi * 247 * t) + 0.25 * math.sin(2 * math.pi * 413 * t))
        line = 0.0
        for at, hz in notes:
            if t >= at:
                tt = t - at
                line += math.exp(-tt * 2.2) * (math.sin(2 * math.pi * hz * tt) + 0.3 * math.sin(2 * math.pi * hz * 2 * tt))
        out.append(knell * 0.6 + line * 0.4)
    return normalise(out, 0.7), None


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, (samples, loop), rate in [
        ("tower_drone.wav", drone(), 22050),
        ("torch.wav", torch(), 16000),
        ("water.wav", water(), 16000),
        ("chime.wav", chime(), 22050),
        ("wind.wav", wind(), 16000),
        ("crowd.wav", crowd(), 16000),
        ("engine.wav", engine(), 16000),
        ("drips.wav", drips(), 16000),
        ("sting_boss.wav", sting_boss(), 22050),
        ("sting_phase.wav", sting_phase(), 22050),
        ("sting_fall.wav", sting_fall(), 22050),
    ]:
        wav(os.path.join(out, name), rate, samples, loop)


if __name__ == "__main__":
    main()
