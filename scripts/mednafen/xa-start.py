#!/usr/bin/env python3
"""Time a cast's CD-XA voice: the starter's frame and the clip's first sample, on one clock.

A `*_summon_mid_cast` library state plays on into its module's head cue with no input.
This records that run with the sequences silenced, subtracts the state's SPU resumed by the
engine, and places the clip by correlating what is left against the decoded channel
(`extracted/XA_WAV/`). It then steps the same run frame by frame (mednafen's advance-frame and
save-state keys through xdotool), reading the drive-state word `0x8007BC20` out of each
saved state, until the clip starter has run. Both times count from the state as sample `0`.
Numbers only: it changes nothing and gates nothing. See `docs/subsystems/audio.md`,
"A clip's start".

    scripts/mednafen/xa-start.py XA7 6 theeder_summon_mid_cast     # file, channel, state
"""

import argparse
import gzip
import importlib.util
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import wave
from pathlib import Path

import numpy as np
from numpy.fft import irfft, rfft

REPO = Path(__file__).resolve().parents[2]
VSYNC_SAMPLES = 737.1365
RAM_NAME = b"MainRAM.data8"
VCOUNT = 0x7A894  # libetc's VSync(-1) counter, main-RAM offset
DRIVE_STATE = 0x7BC20  # _DAT_8007BC20, raised by the clip starter
SECONDS = 14

spec = importlib.util.spec_from_file_location("audio_pcm", REPO / "scripts" / "ci" / "audio-pcm.py")
audio_pcm = importlib.util.module_from_spec(spec)
sys.modules["audio_pcm"] = audio_pcm
spec.loader.exec_module(audio_pcm)


def read_pcm(path: Path) -> np.ndarray:
    b = path.read_bytes()
    at = b.find(b"data") + 8
    end = at + (len(b) - at) // 4 * 4
    return np.frombuffer(b[at:end], dtype="<i2").astype(float).reshape(-1, 2)


def clip_start(res: np.ndarray, channel_wav: Path) -> tuple[int, float]:
    """Where the channel's sample 0 lands in `res`, and the correlation there."""
    with wave.open(str(channel_wav)) as w:
        rate = w.getframerate()
        d = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(float)
        d = (d.reshape(-1, 2).mean(axis=1) if w.getnchannels() == 2 else d)[: rate * 3]
    x = np.interp(np.arange(len(d) * 44100 // rate) * rate / 44100, np.arange(len(d)), d)
    n = len(res) + len(x)
    cc = irfft(rfft(res, n) * np.conj(rfft(x, n)), n)
    at = int(np.argmax(np.abs(cc[: len(res) - len(x)])))
    return at, abs(cc[at]) / np.sqrt((x**2).sum() * (res[at : at + len(x)] ** 2).sum() + 1)


def main_ram(path: Path) -> bytes | None:
    raw = path.read_bytes()
    try:
        b = gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw
    except (EOFError, OSError):
        return None
    i = b.find(bytes([len(RAM_NAME)]) + RAM_NAME)
    ram = b[i + 1 + len(RAM_NAME) + 4 :][: 0x200000]
    return ram if i >= 0 and len(ram) == 0x200000 else None


def step_to_starter(state: Path, disc: Path, tmp: Path, display: int, frames: int) -> int | None:
    """The VSync count of the first saved frame with the drive-state word changed."""
    stage, home = tmp / "disc", tmp / "home"
    (home / "mcs").mkdir(parents=True)
    (home / "firmware").mkdir()
    stage.mkdir()
    (stage / disc.name).symlink_to(disc)
    shutil.copy(disc.with_suffix(".cue"), stage / disc.with_suffix(".cue").name)
    firmware = Path.home() / ".mednafen" / "firmware" / "scph5501.bin"
    shutil.copy(firmware, home / "firmware" / firmware.name)
    shutil.copy(state, home / "mcs" / "state.mca")
    env = dict(os.environ, MEDNAFEN_HOME=str(home), SDL_AUDIODRIVER="dummy",
               MEDNAFEN_ALLOWMULTI="1", DISPLAY=f":{display}")
    server = subprocess.Popen(["Xvfb", f":{display}", "-screen", "0", "800x600x24"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    emu = subprocess.Popen(
        ["mednafen", "-autosave", "1", "-filesys.fname_state", "state.%X", "-sound.driver", "sdl",
         "-video.driver", "softfb", "-video.fs", "0", "-psx.input.port1", "gamepad",
         str(stage / disc.with_suffix(".cue").name)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.6)
    key = lambda k: subprocess.run(["xdotool", "key", k], env=env, capture_output=True)
    key("Pause")
    time.sleep(0.2)
    saved, seen, first = home / "mcs" / "state.mc0", None, None
    try:
        for _ in range(frames):
            key("alt+a")
            time.sleep(0.03)
            key("F5")
            for _ in range(100):
                time.sleep(0.03)
                if saved.exists() and saved.stat().st_mtime_ns != seen:
                    time.sleep(0.03)
                    ram = main_ram(saved)
                    if ram:
                        seen = saved.stat().st_mtime_ns
                        word = struct.unpack_from("<I", ram, DRIVE_STATE)[0]
                        if first is None:
                            first = word
                        elif word != first:
                            return struct.unpack_from("<I", ram, VCOUNT)[0]
                        break
    finally:
        emu.terminate()
        time.sleep(0.5)
        emu.kill()
        server.terminate()
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("file", help="XA file of the clip, e.g. XA7")
    ap.add_argument("channel", type=int)
    ap.add_argument("state", help="scenarios.toml label of a mednafen state ahead of its head cue")
    ap.add_argument("--display", type=int, default=277)
    ap.add_argument("--exe", default=str(REPO / "target" / "release-test" / "legaia-engine"))
    a = ap.parse_args()
    disc = Path(os.environ["LEGAIA_DISC_BIN"])
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    source = audio_pcm.state_file(a.state, REPO / "saves" / "library", manifest)
    with tempfile.TemporaryDirectory(prefix="xa-start-") as t:
        tmp = Path(t)
        silent, recorded, resumed = tmp / "silent.mcr", tmp / "silent.wav", tmp / "resumed.wav"
        audio_pcm.silenced_copy(source, silent)
        audio_pcm.record(silent, recorded, SECONDS, disc)
        retail = read_pcm(recorded) / 0.75  # the recorder's gain, `MEDNAFEN_RECORD_GAIN`
        subprocess.run([a.exe, "pcm-retail", "--resume-state", str(silent), "--samples",
                        str(len(retail)), "--engine-wav", str(resumed)], capture_output=True,
                       env=dict(os.environ, RUST_LOG="error"))
        engine = read_pcm(resumed)
        n = min(len(retail), len(engine))
        at, corr = clip_start((retail[:n] - engine[:n]).mean(axis=1),
                              next((REPO / "extracted" / "XA_WAV").glob(f"{a.file}_file*_ch{a.channel}.wav")))
        ram = main_ram(silent)
        start = struct.unpack_from("<I", ram, VCOUNT)[0]
        stepped = tmp / "step"
        stepped.mkdir()
        call = step_to_starter(silent, disc, stepped, a.display, int(at / VSYNC_SAMPLES) + 60)
    if call is None:
        print(f"{a.state}: clip at {at} ({corr:.3f}); the starter did not run in the frames stepped")
        return 1
    v = at / VSYNC_SAMPLES - (call - start)
    print(f"{a.state}: clip at {at} ({corr:.3f}), starter on vsync +{call - start}: "
          f"{v * 1000 / 59.8261:.0f}..{(v + 1) * 1000 / 59.8261:.0f} ms")
    return 0


if __name__ == "__main__":
    sys.exit(main())
