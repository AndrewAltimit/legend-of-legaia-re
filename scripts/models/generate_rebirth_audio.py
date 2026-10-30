#!/usr/bin/env python3
"""Generate the Rebirth pack's own audio: a short tower drone that loops
seamlessly, a torch crackle loop, a water loop and a telegraph chime.

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
    ]:
        wav(os.path.join(out, name), rate, samples, loop)


if __name__ == "__main__":
    main()
