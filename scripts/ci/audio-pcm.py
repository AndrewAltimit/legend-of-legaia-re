#!/usr/bin/env python3
"""Score the engine's mix against real retail PCM, and ratchet it.

mednafen plays on from a library state while `-soundrecord` writes its output
to a WAV (`--capture`, needs mednafen and xvfb; the WAVs are Sony-derived and
stay under the gitignored `captures/audio-pcm/`). `legaia-engine pcm-retail`
then renders the same bars and scores them. See `docs/subsystems/audio.md`,
"Real retail PCM".

Two channels per state:

    track  the state's track from its libsnd read cursor on: sequencer + SPU
    core   the state's SPU resumed sample for sample, with the state's open
           sequence bitmap zeroed in a scratch copy so nothing writes a voice:
           the SPU model alone

    scripts/ci/audio-pcm.py --capture             # record what is missing, score
    scripts/ci/audio-pcm.py --check               # fail below the baseline
    scripts/ci/audio-pcm.py --propose PATH        # candidate baseline (never the committed one)

Exit 0 and a `[skip]` line when the recordings or the disc are missing, matching
the repo's disc-gated convention.
"""

import argparse
import gzip
import json
import os
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "audio-pcm-baseline.json"
PROFILE = "release-test"
TRACK_SECONDS = 45
CORE_SECONDS = 6
# Field, town, world map, battle, menu, title, minigames: a state's own cues
# stay in the recording, so a state without any scores the music cleanest.
STATES = [
    "title_screen_new_game",
    "mei_house_inside",
    "rimelm_wall_press_left",
    "keikoku_chest_pre",
    "sebucus_overworld_resident",
    "party_battle_gobu_gobu",
    "terra_party_battle",
    "menu_status_town",
    "minigame_slot_machine",
    "minigame_fishing",
    "minigame_baka_fighter",
]
CORE_STATES = ["mei_house_inside", "keikoku_chest_pre", "title_screen_new_game"]
# The title screen times out into the attract sequence past this.
SHORT_WINDOWS = {"title_screen_new_game": 10}
OPEN_SEQUENCE_BITMAP = 0x1C_D2B8  # _DAT_801CD2B8, main-RAM offset
CORR_SLACK = 0.002
LEVEL_SLACK = 0.05


def state_file(label: str, library: Path, manifest: dict) -> Path | None:
    for s in manifest["scenarios"]:
        if s["label"] == label and s.get("backup_fingerprint"):
            p = library / "mednafen" / f"{s['backup_fingerprint']}.mcr"
            return p if p.exists() else None
    return None


def main_ram_offset(b: bytes | bytearray) -> int:
    """File offset of the `MainRAM.data8` value in a decompressed state."""
    name = b"MainRAM.data8"
    i = b.find(bytes([len(name)]) + name)
    if i < 0:
        raise RuntimeError("no MainRAM.data8 entry")
    return i + 1 + len(name) + 4


def silenced_copy(state: Path, out: Path) -> None:
    """A copy of a state whose game code writes no more sequence voices."""
    raw = state.read_bytes()
    b = bytearray(gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw)
    at = main_ram_offset(b) + OPEN_SEQUENCE_BITMAP
    b[at : at + 2] = struct.pack("<H", 0)
    out.write_bytes(gzip.compress(bytes(b), 1))


def record(state: Path, wav: Path, seconds: int, disc: Path) -> None:
    """Run mednafen from `state` until `wav` holds `seconds` of output."""
    wav.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="audio-pcm-") as t:
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
        # `-autosave` loads the state before the first frame runs, so the
        # recording's sample 0 is the state.
        shutil.copy(state, home / "mcs" / "state.mca")
        partial = wav.with_suffix(".partial.wav")
        partial.unlink(missing_ok=True)
        cmd = [
            "xvfb-run", "-a", "-s", "-screen 0 800x600x24",
            "mednafen", "-autosave", "1", "-filesys.fname_state", "state.%X",
            "-sound.rate", "44100", "-sound.driver", "sdl", "-video.driver", "softfb",
            "-video.fs", "0", "-psx.input.port1", "gamepad",
            "-soundrecord", str(partial), str(stage / disc.with_suffix(".cue").name),
        ]
        env = dict(os.environ, MEDNAFEN_HOME=str(home), SDL_AUDIODRIVER="dummy",
                   MEDNAFEN_ALLOWMULTI="1")
        want = seconds * 44100 * 4 + 44
        p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True)
        try:
            for _ in range(seconds * 20):
                time.sleep(1)
                if partial.exists() and partial.stat().st_size >= want:
                    break
                if p.poll() is not None:
                    break
        finally:
            if p.poll() is None:
                os.killpg(p.pid, signal.SIGTERM)
            p.wait()
        if not partial.exists() or partial.stat().st_size < want:
            raise RuntimeError(f"mednafen wrote too little for {state.name}")
        partial.replace(wav)


def score(exe: Path, args: list[str], wav: Path, samples: int, out: Path) -> dict:
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "error")
    cmd = [str(exe), "pcm-retail", *args, "--retail-wav", str(wav), "--samples", str(samples),
           "--json", str(out)]
    out.unlink(missing_ok=True)
    # A pass the score never gets back to (a position before its last Loop
    # Start) is no error: the caller takes the pass that exists.
    if subprocess.run(cmd, cwd=REPO, env=env, stderr=subprocess.DEVNULL).returncode != 0:
        return {"corr_median": -1.0, "level_db": 0.0}
    return json.loads(out.read_text())


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--capture", action="store_true", help="record the WAVs that are missing")
    ap.add_argument("--check", action="store_true", help="assert the baseline")
    ap.add_argument("--propose", help="write a candidate baseline to this path")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several)")
    ap.add_argument("--out", default=str(REPO / "captures" / "audio-pcm"))
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args()
    if a.propose and Path(a.propose).resolve() == BASELINE.resolve():
        ap.error("--propose never writes the committed baseline")

    disc = os.environ.get("LEGAIA_DISC_BIN")
    library = Path(os.environ.get("LEGAIA_SAVES_LIBRARY", REPO / "saves" / "library"))
    if not disc or not Path(disc).exists() or not library.is_dir():
        print("[skip] disc image or save library not found")
        return 0
    disc = Path(disc)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())

    if not a.no_build:
        subprocess.run(
            ["cargo", "build", "-p", "legaia-engine-shell", "--profile", PROFILE,
             "--features", "oracles", "--bin", "legaia-engine"],
            cwd=REPO, check=True,
        )
    exe = REPO / "target" / PROFILE / "legaia-engine"
    want = lambda label: not a.filter or any(f in label for f in a.filter.split(","))

    results: dict[str, dict] = {}
    for label in filter(want, STATES):
        state = state_file(label, library, manifest)
        wav = out / f"{label}.wav"
        if state is None:
            continue
        if not wav.exists():
            if not a.capture:
                continue
            record(state, wav, TRACK_SECONDS, disc)
        # A state can sit on a later pass of its score (a channel's state
        # carries across the loop), so take the pass that fits.
        seconds = SHORT_WINDOWS.get(label, TRACK_SECONDS)
        s = max(
            (score(exe, ["--label", label, "--pass", p], wav, seconds * 44100, out / f"{label}.json")
             for p in ("1", "2")),
            key=lambda r: r["corr_median"],
        )
        results[label] = {"track_corr": s["corr_median"], "track_level": s["level_db"]}
    for label in filter(want, CORE_STATES):
        state = state_file(label, library, manifest)
        wav = out / f"{label}.core.wav"
        silent = out / f"{label}.core.mcr"
        if state is None:
            continue
        if not wav.exists():
            if not a.capture:
                continue
            silenced_copy(state, silent)
            record(silent, wav, CORE_SECONDS, disc)
        if not silent.exists():
            silenced_copy(state, silent)
        s = score(exe, ["--resume-state", str(silent)], wav, CORE_SECONDS * 44100,
                  out / f"{label}.core.json")
        results.setdefault(label, {})
        results[label].update(core_corr=s["corr_median"], core_level=s["level_db"])

    if not results:
        print("[skip] no recordings (run with --capture)")
        return 0
    rounded = {k: {c: round(v, 4) for c, v in r.items()} for k, r in results.items()}
    if a.propose:
        Path(a.propose).write_text(json.dumps({"states": rounded}, indent=2, sort_keys=True) + "\n")
        print(f"[propose] wrote {a.propose}")
    if a.check:
        if not BASELINE.exists():
            print(f"[no baseline] {BASELINE.relative_to(REPO)} is not committed yet")
            return 0
        base = json.loads(BASELINE.read_text())["states"]
        drops = []
        for label, chans in rounded.items():
            for c, v in chans.items():
                was = base.get(label, {}).get(c)
                if was is None:
                    print(f"[new] {label}.{c}={v}")
                elif c.endswith("corr") and v < was - CORR_SLACK:
                    drops.append(f"{label}.{c}: {was} -> {v}")
                elif c.endswith("level") and abs(v) > abs(was) + LEVEL_SLACK:
                    drops.append(f"{label}.{c}: {was} -> {v}")
        for d in drops:
            print(f"[drop] {d}")
        print(f"audio-pcm check: {len(drops)} drop(s)")
        return 1 if drops else 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
