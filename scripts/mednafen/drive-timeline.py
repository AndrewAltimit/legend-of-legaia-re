#!/usr/bin/env python3
"""Step a mednafen library state frame by frame and log the CD drive beside a cast's phase.

One CSV row per emulated frame: libetc's VSync counter, the battle context's module phase
`ctx[+0x279]` and side-band stage `ctx[+0x276]`, the XA drive-state word `0x8007BC20`, the
action state `ctx[+0x07]`, the audio level `_DAT_8007B910`, and
mednafen's own CD controller state (`DriveStatus`, `CurSector`, `SeekTarget`, `PSRCounter`,
`PendingCommand`), the frame step and the cue ring's ids and countdowns. It is how long a summon module's seek-then-wait arms hold in retail, and
where the head stood before the seek. Numbers only: it changes nothing and gates nothing.
Steps the emulator with mednafen's advance-frame and save-state keys through xdotool, as
`scripts/mednafen/xa-start.py` does. See `docs/subsystems/audio.md`, "The cast channel".

    scripts/mednafen/drive-timeline.py orb_summon_mid_cast --frames 120 --out orb.csv
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
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
VCOUNT = 0x7A894  # libetc's VSync(-1) counter, main-RAM offset
DRIVE_STATE = 0x7BC20  # _DAT_8007BC20
CTX_PTR = 0x7BD24  # gp+0xA0C, the battle context pointer
DUCK_CELL = 0x7B910  # _DAT_8007B910, the audio level the action SM steps
RING_IDS = 0x7B6D8  # DAT_8007B6D8[4], the cue ring's ids (i16)
RING_TIMERS = 0x7C338  # DAT_8007C338[4], their countdowns (u32)
FRAME_STEP = 0x393  # 0x1F800393 in the scratchpad, the frame step
CDC_VARS = ["DriveStatus", "CurSector", "SeekTarget", "PSRCounter", "PendingCommand",
            "PendingCommandCounter", "StatusAfterSeek"]

spec = importlib.util.spec_from_file_location("audio_pcm", REPO / "scripts" / "ci" / "audio-pcm.py")
audio_pcm = importlib.util.module_from_spec(spec)
sys.modules["audio_pcm"] = audio_pcm
spec.loader.exec_module(audio_pcm)


def var(b: bytes, name: str, after: int = 0) -> int | None:
    """A mednafen state variable's value: `[len][name][u32 size][bytes]`, little-endian."""
    key = bytes([len(name)]) + name.encode()
    i = b.find(key, after)
    if i < 0:
        return None
    at = i + len(key)
    size = struct.unpack_from("<I", b, at)[0]
    data = b[at + 4 : at + 4 + min(size, 4)]
    return int.from_bytes(data, "little", signed=size == 4 and name in ("PSRCounter", "PendingCommandCounter"))


_last_ram = [None]


def row(path: Path) -> list | None:
    raw = path.read_bytes()
    try:
        b = gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw
    except (EOFError, OSError):
        return None
    name = b"MainRAM.data8"
    i = b.find(bytes([len(name)]) + name)
    if i < 0:
        return None
    ram = b[i + 1 + len(name) + 4 :][: 0x200000]
    if len(ram) != 0x200000:
        return None
    ctx = struct.unpack_from("<I", ram, CTX_PTR)[0] & 0x1FFFFF
    cdc = b.find(b"CDC")
    # Bytes of main RAM that moved since the last frame: a game loop held inside a
    # blocking CD call moves only what the interrupt handlers write.
    prev, _last_ram[0] = _last_ram[0], ram
    moved = -1 if prev is None else sum(1 for x, y in zip(prev[0x10000:0x1F0000:4], ram[0x10000:0x1F0000:4]) if x != y)
    ring = [struct.unpack_from("<h", ram, RING_IDS + 2 * n)[0] for n in range(4)] + \
        [struct.unpack_from("<I", ram, RING_TIMERS + 4 * n)[0] for n in range(4)]
    sp = b.find(b"ScratchRAM.data8")
    step = b[sp + 16 + 4 + FRAME_STEP] if sp >= 0 else -1
    return [struct.unpack_from("<I", ram, VCOUNT)[0], ram[ctx + 0x279], ram[ctx + 0x276],
            struct.unpack_from("<I", ram, DRIVE_STATE)[0], ram[ctx + 7],
            struct.unpack_from("<i", ram, DUCK_CELL)[0]] + [var(b, v, max(cdc, 0)) for v in CDC_VARS] + \
        [moved, step] + ring


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("state", help="scenarios.toml label of a mednafen library state")
    ap.add_argument("--frames", type=int, default=120)
    ap.add_argument("--out", required=True)
    ap.add_argument("--display", type=int, default=278)
    ap.add_argument("--settle", type=float, default=0.6,
                    help="seconds between launch and pausing; the game runs meanwhile, so a "
                         "shorter one logs the frames just after the state")
    a = ap.parse_args()
    disc = Path(os.environ["LEGAIA_DISC_BIN"])
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    state = audio_pcm.state_file(a.state, REPO / "saves" / "library", manifest)
    if state is None:
        print(f"no mednafen library state {a.state}")
        return 1
    rows = [["vsync", "phase", "sideband", "drive_word", "action_state", "duck"] + CDC_VARS + ["ram_words_moved", "step"] + \
        [f"id{n}" for n in range(4)] + [f"timer{n}" for n in range(4)]]
    with tempfile.TemporaryDirectory(prefix="drive-timeline-") as t:
        tmp = Path(t)
        stage, home = tmp / "disc", tmp / "home"
        (home / "mcs").mkdir(parents=True)
        (home / "firmware").mkdir()
        stage.mkdir()
        # A private stage: nothing beside the image can patch it.
        (stage / disc.name).symlink_to(disc)
        shutil.copy(disc.with_suffix(".cue"), stage / disc.with_suffix(".cue").name)
        firmware = Path.home() / ".mednafen" / "firmware" / "scph5501.bin"
        shutil.copy(firmware, home / "firmware" / firmware.name)
        shutil.copy(state, home / "mcs" / "state.mca")
        rows.append(row(state))
        env = dict(os.environ, MEDNAFEN_HOME=str(home), SDL_AUDIODRIVER="dummy",
                   MEDNAFEN_ALLOWMULTI="1", DISPLAY=f":{a.display}")
        server = subprocess.Popen(["Xvfb", f":{a.display}", "-screen", "0", "800x600x24"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
        emu = subprocess.Popen(
            ["mednafen", "-autosave", "1", "-filesys.fname_state", "state.%X", "-sound.driver",
             "sdl", "-video.driver", "softfb", "-video.fs", "0", "-psx.input.port1", "gamepad",
             str(stage / disc.with_suffix(".cue").name)],
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(a.settle)
        key = lambda k: subprocess.run(["xdotool", "key", k], env=env, capture_output=True)
        key("Pause")
        time.sleep(0.2)
        saved, seen = home / "mcs" / "state.mc0", None
        try:
            for _ in range(a.frames):
                key("alt+a")
                time.sleep(0.03)
                key("F5")
                for _ in range(100):
                    time.sleep(0.03)
                    if saved.exists() and saved.stat().st_mtime_ns != seen:
                        time.sleep(0.03)
                        r = row(saved)
                        if r:
                            seen = saved.stat().st_mtime_ns
                            rows.append(r)
                            break
        finally:
            emu.terminate()
            time.sleep(0.5)
            emu.kill()
            server.terminate()
    Path(a.out).write_text("\n".join(",".join(str(v) for v in r) for r in rows) + "\n")
    print(f"{a.state}: {len(rows) - 1} frames -> {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
