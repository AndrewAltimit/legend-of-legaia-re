#!/usr/bin/env python3
"""Give a real-retail-PCM residual its cause, note by note.

`legaia-engine pcm-retail --stems PREFIX` writes the engine's render of a state's
bars as one stem per voice, the wet return beside them, plus the key-ons and how many
voices each flush staged. This splits the stems at the key-ons and fits every note's
own time offset and gain against mednafen's recording by coordinate descent. A whole
mix's correlation cannot say whose timing is off; these offsets can:

    against pitch   a fixed lead in source samples reads `a + b / pitch`
    against a flush's staged voices   the cost of the flush before its KON write
    a whole flush a vsync off   the tick stream's phase, not the SPU

Numbers only: it changes nothing and gates nothing. Needs numpy, the disc and a
recording from `scripts/ci/audio-pcm.py --capture`. See `docs/subsystems/audio.md`,
"Real retail PCM".

    scripts/ci/audio-note-fit.py menu_status_town --wav captures/audio-pcm/menu_status_town.wav
"""

import argparse
import json
import os
import subprocess
import tempfile
import wave
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[2]
RECORD_GAIN = 0.75
VSYNC = 737.1365
PAD = 2000


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path)) as w:
        raw = w.readframes(w.getnframes())
    return np.frombuffer(raw, dtype="<i2").reshape(-1, 2).astype(np.float32)


def add(mix: np.ndarray, seg: dict, offset: int, sign: int) -> None:
    a = seg["start"] + offset
    x = seg["x"]
    b = a + len(x)
    if a < 0:
        x = x[-a:]
        a = 0
    if b > len(mix):
        x = x[: len(mix) - a]
        b = len(mix)
    mix[a:b] += sign * x


def score(retail: np.ndarray, mix: np.ndarray, n: int, label: str) -> None:
    window = 22050
    cs = []
    for s in range(window, n - window, window):
        a, b = retail[s : s + window].mean(1), mix[s : s + window].mean(1)
        cs.append(float((a * b).sum() / np.sqrt((a * a).sum() * (b * b).sum() + 1e-9)))
    print(f"{label}: median {np.median(cs):.4f}  p10 {np.percentile(cs, 10):.4f}")


def fit(args: argparse.Namespace, prefix: Path) -> None:
    retail_wav = read_wav(args.wav) / RECORD_GAIN
    engine = read_wav(prefix.with_suffix(".wav"))
    count = len(engine)
    stems = np.memmap(prefix.with_suffix(".stems"), dtype="<i2", mode="r").reshape(count, 25, 2)
    n = min(int(args.seconds * 44100), count - PAD)
    notes = [json.loads(line) for line in prefix.with_suffix(".notes").read_text().splitlines()[1:]]
    ons = [e for e in notes if e["ev"] == "on"]
    segs = []
    for voice in range(24):
        keyed = [e for e in ons if e["v"] == voice]
        held = {"sample": 0, "pitch": 0, "adsr1": 0}
        for key in keyed + [None]:
            a, b = held["sample"], key["sample"] if key else n + PAD
            if a < n and b > a:
                x = np.array(stems[a : min(b, n + PAD), voice], dtype=np.float32)
                if (x * x).sum() >= 1:
                    segs.append({"start": a, "x": x, "note": held, "off": 0, "gain": 0.0})
            if key:
                held = key
    wet = np.array(stems[: n + PAD, 24], dtype=np.float32)
    mix = wet.copy()
    for seg in segs:
        add(mix, seg, 0, 1)
    # Fix the recording's overall place by the render's whole lag, then each note's.
    lag = args.lag
    retail = np.zeros_like(mix)
    usable = min(len(retail_wav) + lag, len(retail))
    retail[:usable] = retail_wav[-lag : -lag + usable]
    score(retail, mix, n, "as rendered")

    segs.sort(key=lambda s: -float((s["x"] * s["x"]).sum()))
    for _ in range(3):
        for seg in segs:
            add(mix, seg, seg["off"], -1)
            length = min(len(seg["x"]), args.support)
            lo = max(-seg["start"], -args.reach)
            hi = min(args.reach, n + PAD - seg["start"] - length)
            if hi > lo:
                target = (retail - mix)[seg["start"] + lo : seg["start"] + hi + length]
                head = seg["x"][:length]
                size = len(target) + length
                dot = sum(
                    np.fft.irfft(np.fft.rfft(target[:, c], size) * np.conj(np.fft.rfft(head[:, c], size)),
                                 size)[: hi - lo + 1]
                    for c in (0, 1)
                )
                seg["off"] = int(np.argmax(dot)) + lo
                seg["gain"] = float(dot.max() / max((head * head).sum(), 1.0))
            add(mix, seg, seg["off"], 1)
    score(retail, mix, n, "every note at its own offset")

    fitted = [s for s in segs if s["start"] > 0 and 0.7 < s["gain"] < 1.3]
    near = [s for s in fitted if abs(s["off"]) < 25]
    if near:
        x = np.array([4096 / s["note"]["pitch"] for s in near])
        y = np.array([s["off"] for s in near], dtype=float)
        slope, icept = np.polyfit(x, y, 1)
        print(f"against pitch: offset = {icept:+.2f} {slope:+.3f} / ratio, over {len(near)} notes")
    staged = {}
    flush_log = prefix.with_suffix(".flush")
    for line in flush_log.read_text().splitlines():
        at, count_staged = line.split()
        staged[int(at)] = int(count_staged)
    rows = []
    for start in sorted({s["start"] for s in fitted}):
        offs = [s["off"] for s in fitted if s["start"] == start]
        rows.append((start, staged.get(start, 0), float(np.median(offs))))
    in_grid = [r for r in rows if abs(r[2]) < 25]
    if len(in_grid) > 2:
        xs = np.array([r[1] for r in in_grid], dtype=float)
        ys = np.array([r[2] for r in in_grid])
        slope, icept = np.polyfit(xs, ys, 1)
        scatter = np.std(ys - np.polyval((slope, icept), xs))
        print(f"per flush: offset = {icept:+.2f} {slope:+.3f} * staged voices, scatter {scatter:.2f}")
    off_grid = [r for r in rows if abs(round(r[2] / VSYNC)) >= 1]
    print(f"flushes a vsync off: {len(off_grid)} of {len(rows)}")
    for start, voices, off in off_grid[:20]:
        print(f"  {start / 44100:7.3f} s  {voices:2} staged  {off:+6.0f} samples")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("label")
    ap.add_argument("--wav", type=Path, required=True, help="mednafen's recording of the state")
    ap.add_argument("--pass", dest="pass_no", default="0", help="pcm-retail --pass")
    ap.add_argument("--seconds", type=float, default=28)
    ap.add_argument("--lag", type=int, default=-780, help="render against recording, samples")
    ap.add_argument("--reach", type=int, default=800, help="how far a note may move")
    ap.add_argument("--support", type=int, default=6000, help="samples of a note the fit reads")
    a = ap.parse_args()
    exe = REPO / "target" / "release-test" / "legaia-engine"
    with tempfile.TemporaryDirectory(prefix="note-fit-") as tmp:
        prefix = Path(tmp) / a.label
        subprocess.run(
            [str(exe), "pcm-retail", "--label", a.label, "--pass", a.pass_no,
             "--samples", str(int((a.seconds + 2) * 44100)), "--vsync-samples", str(VSYNC),
             "--stems", str(prefix), "--engine-wav", str(prefix.with_suffix(".wav"))],
            cwd=REPO, check=True, env=dict(os.environ, RUST_LOG="error"),
        )
        fit(a, prefix)


if __name__ == "__main__":
    main()
