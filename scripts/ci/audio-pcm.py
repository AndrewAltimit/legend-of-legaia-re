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
    xa     the same recording of a state with a CD-XA clip sounding, the resumed
           SPU subtracted: the CD input against the channel through the port's
           rate converter and cue gain. Rate, level and waveform; not the start.
    emitter  the track channel with the state's own cue voices resumed into it:
           a looping positional emitter holds its voices for the whole recording
    cast   the track channel with the engine's own cues mixed in: the fight
           seeded into a battle state by the retail comparison corpus's child,
           its mix recorded from the seed's gate on with the music bus silent
           (`LEGAIA_DIAG_AUDIO_WAV`), over the cast's own seconds
    cast_steps  the cast channel with the engine's cast run on the recording's own
           frame steps (`<label>.steps` beside the WAV, mednafen's, from
           `scripts/mednafen/drive-timeline.py --steps-out`): the battle frame
           step is a measure of the emulated machine's frame cost, so this is
           the cast's sound with the two emulators' costs taken out. Harness-only:
           `cast` stays the engine's own clock

    scripts/ci/audio-pcm.py --capture             # record what is missing, score
    scripts/ci/audio-pcm.py --capture --jobs 6    # the recordings side by side
    scripts/ci/audio-pcm.py --check               # fail below the baseline
    scripts/ci/audio-pcm.py --propose PATH        # candidate baseline (never the committed one)

Exit 0 and a `[skip]` line when the recordings or the disc are missing, matching
the repo's disc-gated convention.
"""

import argparse
import concurrent.futures
import gzip
import itertools
import shlex
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
    "noa_levelup_field_pre",
    "zeto_call_wave_mid_cast",
    "new_game_cutscene_intro_a",
    "ending_vignette_fullscreen",
]
CORE_STATES = ["mei_house_inside", "keikoku_chest_pre", "title_screen_new_game"]
# The title screen times out into the attract sequence past this.
SHORT_WINDOWS = {"title_screen_new_game": 10}
# A state with a CD-XA one-shot sounding: the decoded channel it reads, where in
# that channel the state stands, and the stereo samples before the clip's stop.
# The start is on the converter's own six-step phase: it repeats every 6 samples,
# and three off reads a correlation of `0.991` against `0.999` on the same audio.
XA_STATES = {"battle_melee_hit_spark": ("XA2_file1_ch9.wav", 35891, 11025)}
# A state a looping emitter sounds through: the track alone reads its energy as
# level and correlation.
EMITTER_STATES = ["rimelm_wall_press_left"]
# Mid-cast battle states for the `cast` channel. The engine's recording runs from
# the seed's gate to `CAST_DONE_TAIL` vsyncs past the action SM's Done band, where
# the child plays on into its own next turn; the score is over that span. A state
# not in STATES is recorded for `CAST_RECORD_SECONDS` and scored on this channel
# alone.
CAST_STATES = [
    "zeto_call_wave_mid_cast",
    "zeto_big_wave_mid_cast",
    "cort_guilty_cross_mid_cast",
    "cort_mystic_shield_mid_cast",
    "theeder_summon_mid_cast",
    "zenoir_summon_mid_cast",
    "nighto_summon_mid_cast",
    "orb_summon_mid_cast",
    "swordie_summon_mid_cast",
    "gilium_summon_mid_cast",
]
# Cast states whose recording carries its own frame steps (`<label>.steps`) for
# the `cast_steps` channel, and how many mednafen frames the stepping logs: a
# frame-advance key the emulator drops repeats a vsync, so this runs past the
# cast's own length.
CAST_STEP_STATES = {"cort_guilty_cross_mid_cast": 1350, "zenoir_summon_mid_cast": 1650}
CAST_DONE_TAIL = 20
CAST_RECORD_SECONDS = 40
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


# Each recording gets its own X server. `xvfb-run -a` moves on from a number in
# use, but two started together can pick the same one: start them apart.
_server_num = itertools.count(200)


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
            "xvfb-run", "-a", "-n", str(next(_server_num)), "-s", "-screen 0 800x600x24",
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
            for _ in range(seconds * 80):
                time.sleep(0.25)
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


def capture_missing(want, library: Path, manifest: dict, out: Path, disc: Path, jobs: int) -> None:
    """Record every missing WAV, `jobs` mednafens at a time.

    Each recording is a private home, disc stage and X server, and the run is
    deterministic in emulated time however slowly it goes, so running them side
    by side leaves the WAVs' bytes as they are.
    """
    def track(label: str, state: Path, wav: Path) -> None:
        record(state, wav, TRACK_SECONDS, disc)

    def core(label: str, state: Path, wav: Path) -> None:
        silent = out / f"{label}.core.mcr"
        silenced_copy(state, silent)
        record(silent, wav, CORE_SECONDS, disc)

    def cast(label: str, state: Path, wav: Path) -> None:
        record(state, wav, CAST_RECORD_SECONDS, disc)

    tasks = []
    for labels, suffix, fn in ((STATES, "", track), (CAST_STATES, "", cast),
                               (CORE_STATES, ".core", core), (XA_STATES, ".core", core)):
        for label in filter(want, labels):
            state = state_file(label, library, manifest)
            wav = out / f"{label}{suffix}.wav"
            if state is not None and not wav.exists() and not any(t[2] == wav for t in tasks):
                tasks.append((fn, label, wav, state))
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        for f in [pool.submit(fn, label, state, wav) for fn, label, wav, state in tasks]:
            f.result()
    # The frame steps step the emulator by hand, one at a time.
    for label, frames in CAST_STEP_STATES.items():
        steps = out / f"{label}.steps"
        if want(label) and not steps.exists() and state_file(label, library, manifest):
            with tempfile.TemporaryDirectory(prefix="audio-pcm-steps-") as t:
                subprocess.run([sys.executable, str(REPO / "scripts" / "mednafen" / "drive-timeline.py"),
                                label, "--frames", str(frames), "--settle", "0.15",
                                "--display", str(next(_server_num)), "--out", str(Path(t) / "t.csv"),
                                "--steps-out", str(steps)], check=True)


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


def engine_cast_wav(exe: Path, label: str, out: Path, library: Path, disc: Path,
                    steps: Path | None = None) -> Path | None:
    """The engine's mix of a battle state's cast, the music bus silent.

    The retail comparison corpus seeds the state (its image child, kept with
    `LEGAIA_RC_CHILD_LOG=1`); the child is run again with its director on a
    device-less output, reading the disc's CD-XA, and records from its gate on.
    With `steps`, the recorded run takes that frame-step track
    (`LEGAIA_CAST_STEP_TRACK`) and writes `<label>.steps.cast.wav`.
    """
    rc = out / f"{label}.rc"
    extracted = REPO / "extracted"
    env = dict(os.environ, LEGAIA_RC_CHILD_LOG="1", RUST_LOG="error", XMODIFIERS="@im=none")
    subprocess.run([str(exe), "retail-compare", "--out", str(rc), "--library", str(library),
                    "--extracted-root", str(extracted), "--manifest",
                    str(REPO / "scripts" / "scenarios.toml"), "--images", "--filter", f"={label}"],
                   cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    kept = rc / "engine" / f"{label}.cmd.sh"
    if not kept.exists():
        print(f"[cast] {label}: retail-compare kept no child command")
        return None
    toks = shlex.split(kept.read_text())
    work, rest = Path(toks[1]), toks[3:]
    i = 0
    while i < len(rest) and "=" in rest[i] and not rest[i].startswith("/"):
        k, v = rest[i].split("=", 1)
        env[k] = v
        i += 1
    args = [t for t in rest[i + 1:] if t != "--no-audio"]
    # The child exits when its frame tick passes with the frame untaken.
    if "--screenshot-tick" in args:
        args[args.index("--screenshot-tick") + 1] = "40000"
    if "--extracted-root" in args:
        j = args.index("--extracted-root")
        args[j : j + 2] = ["--disc", str(disc)]
    stem = f"{label}.steps.cast" if steps else f"{label}.cast"
    wav = (out / f"{stem}.wav").resolve()
    wav.unlink(missing_ok=True)
    if steps:
        env["LEGAIA_CAST_STEP_TRACK"] = str(steps.resolve())
    env.update({
        "LEGAIA_AUDIO_DETACHED": "1",
        # The frame is not taken: the child plays on past its gate.
        "LEGAIA_DIAG_CAPTURE_TICK": "40000",
        "LEGAIA_DIAG_AUDIO_WAV": str(wav),
        "LEGAIA_DIAG_AUDIO_WAV_SECONDS": str(CAST_RECORD_SECONDS),
        "LEGAIA_DIAG_AUDIO_WAV_DONE_TAIL": str(CAST_DONE_TAIL),
    })
    log = out / f"{stem}.log"
    with log.open("w") as f:
        subprocess.run([str(exe), *args], cwd=work, env=env, stdout=f, stderr=subprocess.STDOUT,
                       timeout=900)
    if not wav.exists():
        print(f"[cast] {label}: the child wrote no recording (see {log})")
        return None
    return wav


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--capture", action="store_true", help="record the WAVs that are missing")
    ap.add_argument("--check", action="store_true", help="assert the baseline")
    ap.add_argument("--propose", help="write a candidate baseline to this path")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several)")
    ap.add_argument("--jobs", type=int, default=4, help="--capture: recordings side by side")
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
    exe = Path(os.environ.get("LEGAIA_ENGINE_BIN") or REPO / "target" / PROFILE / "legaia-engine")
    want = lambda label: not a.filter or any(f in label for f in a.filter.split(","))
    if a.capture:
        capture_missing(want, library, manifest, out, disc, a.jobs)

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
        # A state can sit on a later pass of its score, with its own tick phase
        # against the vsync. `0` finds the pass whose wait matches the state's;
        # a state whose wait no pass reproduces takes whichever early pass fits.
        seconds = SHORT_WINDOWS.get(label, TRACK_SECONDS)
        s = max(
            (score(exe, ["--label", label, "--pass", p], wav, seconds * 44100, out / f"{label}.json")
             for p in ("0", "1", "2")),
            key=lambda r: r["corr_median"],
        )
        results[label] = {"track_corr": s["corr_median"], "track_level": s["level_db"]}
        if label in EMITTER_STATES:
            e = max(
                (score(exe, ["--label", label, "--pass", p, "--cue-voices"], wav, seconds * 44100,
                       out / f"{label}.emitter.json")
                 for p in ("0", "1", "2")),
                key=lambda r: r["corr_median"],
            )
            results[label] |= {"emitter_corr": e["corr_median"], "emitter_level": e["level_db"]}
    for label in filter(want, CAST_STATES):
        state = state_file(label, library, manifest)
        wav = out / f"{label}.wav"
        if state is None:
            continue
        if not wav.exists():
            if not a.capture:
                continue
            record(state, wav, CAST_RECORD_SECONDS, disc)
        cast = engine_cast_wav(exe, label, out, library, disc)
        if cast is None:
            continue
        samples = (cast.stat().st_size - 44) // 4
        c = max(
            (score(exe, ["--label", label, "--pass", p, "--cast-wav", str(cast)], wav, samples,
                   out / f"{label}.cast.json")
             for p in ("0", "1", "2")),
            key=lambda r: r["corr_median"],
        )
        if d := c.get("cast_drive"):
            # The voice past the cast's CD-seek wait is aligned on its own, within
            # the seek window (docs/subsystems/audio.md): reported, never silent.
            vs = 737.1365
            print(f"[cast] {label}: CD-seek wait {d['held_ticks']} ticks; voice "
                  f"{(d['voice_offset'] - d['offset']) / vs:+.1f} vsyncs past the cast's offset, "
                  f"window {d['window'][0] / vs:+.1f}..{d['window'][1] / vs:+.1f} vsyncs")
        results.setdefault(label, {})
        results[label] |= {"cast_corr": c["corr_median"], "cast_level": c["level_db"]}
        steps = out / f"{label}.steps"
        if steps.exists() and (sc := engine_cast_wav(exe, label, out, library, disc, steps)):
            samples = (sc.stat().st_size - 44) // 4
            c = max(
                (score(exe, ["--label", label, "--pass", p, "--cast-wav", str(sc)], wav, samples,
                       out / f"{label}.steps.cast.json")
                 for p in ("0", "1", "2")),
                key=lambda r: r["corr_median"],
            )
            results[label] |= {"cast_steps_corr": c["corr_median"],
                               "cast_steps_level": c["level_db"]}
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
    for label, (channel, start, samples) in XA_STATES.items():
        if not want(label):
            continue
        state = state_file(label, library, manifest)
        wav = out / f"{label}.core.wav"
        silent = out / f"{label}.core.mcr"
        decoded = REPO / "extracted" / "XA_WAV" / channel
        if state is None or not decoded.exists():
            continue
        if not wav.exists():
            if not a.capture:
                continue
            silenced_copy(state, silent)
            record(silent, wav, CORE_SECONDS, disc)
        if not silent.exists():
            silenced_copy(state, silent)
        s = score(exe, ["--resume-state", str(silent), "--xa-wav", str(decoded),
                        "--xa-start", str(start)], wav, samples, out / f"{label}.xa.json")
        results[label] = {"xa_corr": s["corr_median"], "xa_level": s["level_db"]}

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
