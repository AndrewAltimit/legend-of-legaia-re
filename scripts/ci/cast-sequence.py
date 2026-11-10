#!/usr/bin/env python3
"""Measure one cast frame by frame against retail (the cast-sequence oracle).

Three stages, each cached under `--out`:

1. retail  - `scripts/pcsx-redux/autorun_cast_sequence.lua` from a library
             PCSX-Redux battle state: inject the action id into the acting
             seat's queue, dump the displayed frame every `--every` vsyncs
             plus per-vsync metadata (`<out>/retail/`). Slow (an emulator
             run); reused until `--recapture`.
2. engine  - the retail-compare image child for the same state
             (`LEGAIA_RC_CHILD_LOG=1`, kept under `<out>/rc/`), re-run with
             the cast seeded as `LEGAIA_BATTLE_INFLIGHT` (party caster) or
             `LEGAIA_BATTLE_MONSTER_CAST` (monster caster) and the
             `LEGAIA_DIAG_CAST_SEQ` recorder (`<out>/engine/`). Seconds.
3. score   - `legaia-engine cast-sequence-score`: pair the frames on the
             action-SM clock, score each pair with the retail-compare block
             metric, match the two sides' sound cues (`sfx.csv`), write
             `report.md` / `report.json` / `strip.png`.

    scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu
    scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu --engine-only

The outputs hold retail pixels and RAM: they live under the gitignored
`captures/` (default) or a scratch directory, never in the repo. See
`docs/tooling/cast-sequence-oracle.md`.
"""

import argparse
import csv
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PROFILE = "release-test"
# Ticks `gate_film_skip` may drop: a frame step of 4 plus an overrun.
MAX_GATE_FILM_SKIP = 8
# `LEGAIA_CAST_SEQ_EXE` scores another build (a before / after pair) without
# rebuilding over the one in the tree.
EXE = Path(os.environ.get("LEGAIA_CAST_SEQ_EXE") or REPO / "target" / PROFILE / "legaia-engine")
PROBE = "scripts/pcsx-redux/autorun_cast_sequence.lua"
# Draws in one frame that mark a melee hit (`retail_hit_words`).
HIT_FRAME_DRAWS = 20
# Env the retail-compare child carries that would steer the fight somewhere
# else than the injected cast: its own capture gate, pad drive and replay.
DROP_ENV = {
    "LEGAIA_BATTLE_DRIVE", "LEGAIA_CAPTURE_GATE", "LEGAIA_CAPTURE_DISPLAY_LAG",
    "LEGAIA_BATTLE_INFLIGHT", "LEGAIA_BATTLE_MONSTER_CAST", "LEGAIA_DIAG_CAPTURE_TICK",
    "LEGAIA_BATTLE_CAM_ALIGN", "LEGAIA_BATTLE_FRAME_STEP", "LEGAIA_BATTLE_FRAME_STEP_AFTER_ACCUM",
    "LEGAIA_SEAT_HUD_GLIDES",
    # One capture instant's cycler state: seated for a whole run it holds the
    # arena's palette still (the Cort fight's flesh). The scroller rects stay,
    # held only until the cast's first action pass
    # (`LEGAIA_SEAT_VRAM_RECTS_UNTIL_ACTION`).
    "LEGAIA_SEAT_CLUT_FX",
    # A strike capture's re-seat of its own round: left in, the cast's action
    # seed recentres a fight the re-seat stood elsewhere (a caster recorded on
    # (-978, 339) cast from (1977, 2207)).
    "LEGAIA_BATTLE_RESEAT", "LEGAIA_BATTLE_GROUND_SNAP", "LEGAIA_BATTLE_APPROACH_YAW",
    "LEGAIA_BATTLE_DISPLAYED_SEAT", "LEGAIA_SEAT_DISPLAYED_CLIPS",
    # The still's pin to the default step, for a replay whose gate counts
    # passes from a prompt the capture's history did not run on: a film plays
    # the whole cast from there, and retail's ring ran the party's step
    # (`chitei2_cort_pre_mystic_circle` opens on `3`, the pin held `2`).
    "LEGAIA_BATTLE_PARTY_STEP",
}

# What a `--physical` recording keeps of those: it plays the state's own round,
# so its drive and the re-seat that stands that round on the capture's ground.
PHYSICAL_KEEP_ENV = {
    "LEGAIA_BATTLE_DRIVE", "LEGAIA_BATTLE_RESEAT", "LEGAIA_BATTLE_GROUND_SNAP",
    "LEGAIA_BATTLE_APPROACH_YAW", "LEGAIA_BATTLE_DISPLAYED_SEAT",
}


def main_checkout() -> Path:
    try:
        common = subprocess.run(
            ["git", "-C", str(REPO), "rev-parse", "--git-common-dir"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        return (REPO / common).resolve().parent
    except (subprocess.CalledProcessError, FileNotFoundError):
        return REPO


def num(s: str) -> int:
    return int(s, 0)


def read_kv(path: Path) -> dict:
    out = {}
    if path.exists():
        for line in path.read_text().splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                out[k.strip()] = v.strip()
    return out


def engine_ground(retail: str) -> str:
    """Re-key the recorded ground list from retail pool slots to engine slots.

    The probe writes one `x:z:facing` (or `-`) per retail pool slot: the party at
    0..2, the monsters at 3 + m whatever the party size. The engine packs the
    monsters behind the party, so monster m is engine slot party_count + m
    (the re-key `RetailBattle::engine_ground` does for the corpus seeds).
    """
    if not retail:
        return ""
    toks = retail.split(",")
    party = [t for t in toks[:3] if t not in ("", "-")]
    monsters = toks[3:]
    return ",".join(party + monsters)


def run_retail(a, out: Path) -> bool:
    rdir = out / "retail"
    done = read_kv(rdir / "done.txt")
    if done and not a.recapture and int(done.get("done", "-1")) >= 0:
        print(f"[retail] cached: {rdir} (cast at vsync {done['cast']}, {done['shots']} frames)")
        return True
    if rdir.exists():
        shutil.rmtree(rdir)
    rdir.mkdir(parents=True)
    disc = os.environ.get("LEGAIA_DISC_BIN")
    if not disc:
        print("[skip] LEGAIA_DISC_BIN unset: no retail capture")
        return False
    env = dict(os.environ)
    env.update({
        "LEGAIA_SPELL": str(a.spell or 0),
        "LEGAIA_CASTER_SEAT": str(a.caster),
        "LEGAIA_TARGET_SEAT": str(a.target),
        "LEGAIA_SHOT_EVERY": str(a.every),
        "LEGAIA_TAIL": str(a.tail),
        # A private emulator profile and temp dir: the runner stages the disc
        # in a fresh directory with no sibling .ppf (the auto-apply trap).
        "LEGAIA_PCSX_PROFILE_DIR": str(out / ".pcsx-profile"),
        "TMPDIR": str(out / ".tmp"),
    })
    (out / ".tmp").mkdir(exist_ok=True)
    if a.learned:
        env["LEGAIA_LEARN_LEVEL"] = str(a.learned)
    if a.monster_hp:
        env["LEGAIA_MONSTER_HP"] = str(a.monster_hp)
    if a.formation:
        env["LEGAIA_FORCE_ROW"] = a.formation
    if a.party_spd:
        env["LEGAIA_PARTY_SPD"] = str(a.party_spd)
    if a.monster_pick:
        env["LEGAIA_MONSTER_PICK"] = "1"
        if a.cast_queue:
            env["LEGAIA_CAST_QUEUE"] = a.cast_queue
    if a.physical:
        env["LEGAIA_PHYSICAL"] = "1"
    if a.save_inject:
        env["LEGAIA_SAVE_INJECT"] = "1"
    if a.retail_parts:
        env["LEGAIA_CAST_PARTS"] = "1"
    if a.prim_calls:
        env["LEGAIA_PRIM_CALLS"] = "1"
    if a.sfx_callers:
        env["LEGAIA_SFX_CALLERS"] = "1"
    if a.pose_objects:
        env["LEGAIA_POSE_OBJECTS"] = "1"
    if a.watch:
        env["LEGAIA_WATCH_W"] = a.watch
    if a.range_trace:
        env["LEGAIA_RANGE_TRACE"] = "1"
    if a.hit_trace:
        env["LEGAIA_HIT_TRACE"] = "1"
    cmd = ["timeout", "--kill-after=10", str(a.retail_timeout), "bash",
           "scripts/pcsx-redux/run_probe.sh", "--lua", PROBE, "--scenario", a.state,
           "--iso", disc, "--out-dir", str(rdir), "--frames", str(a.frames)]
    if not a.window:
        cmd.append("--headless")
    # A forced formation row and the monster-pick rewrite need the breakpoint
    # core; everything else runs on the recompiler (vsync hooks only).
    if not (a.formation or a.monster_pick or a.sfx_callers or a.watch or a.prim_calls or a.pose_objects
            or a.range_trace or a.hit_trace):
        cmd.insert(5, "--fast")
    print(f"[retail] capturing {a.state} {'strike' if a.physical else f'spell 0x{a.spell:02X}'} (an emulator run, minutes)")
    with open(rdir / "stdout.txt", "w") as log:
        rc = subprocess.run(cmd, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT).returncode
    done = read_kv(rdir / "done.txt")
    if int(done.get("cast", "-1")) < 0:
        print(f"[retail] FAILED (exit {rc}): no cast begin in {rdir}/pcsx.log")
        return False
    if int(done.get("done", "-1")) < 0:
        st = int(done.get("parked", "-1"))
        where = f"parked on action state 0x{st:02X}" if st >= 0 else "capture cap hit"
        print(f"[retail] FAILED: the cast never reached its Done band ({where}); "
              "the caster may have no clip or record for this action")
        return False
    print(f"[retail] cast begin at vsync {done['cast']}, done band at {done['done']}, {done['shots']} frames")
    return True


def child_cmd(a, out: Path) -> Path | None:
    """The retail-compare image child's command for the state."""
    kept = out / "rc" / "engine" / f"{a.state}.cmd.sh"
    if kept.exists() and not a.reseed:
        return kept
    library = REPO / "saves" / "library"
    if not library.is_dir():
        library = main_checkout() / "saves" / "library"
    extracted = REPO / "extracted"
    if not (extracted / "PROT.DAT").exists():
        extracted = main_checkout() / "extracted"
    env = dict(os.environ, LEGAIA_RC_CHILD_LOG="1", RUST_LOG="error")
    cmd = [str(EXE), "retail-compare", "--out", str(out / "rc"), "--library", str(library),
           "--extracted-root", str(extracted), "--manifest", str(REPO / "scripts" / "scenarios.toml"),
           "--images", "--filter", f"={a.state}"]
    print(f"[engine] seeding {a.state} through retail-compare")
    subprocess.run(cmd, cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if not kept.exists():
        print(f"[engine] FAILED: retail-compare kept no child command for {a.state} "
              f"(see {out / 'rc' / 'report.md'})")
        return None
    return kept


def state_camera_pose(out: Path) -> str | None:
    """The state's own live camera, as the retail-compare seed read it from
    the save's RAM (`rc/report.json`'s camera detail): pitch, yaw, eye and
    focus. A `--physical` film's gate tick is the state's instant, so this is
    the pose the engine stands on there. The recording's first row is not:
    the probe writes it after the first vsync has run, which on a capture-band
    state can be the pass that builds a module's shot (Lip Kid's Kiss of Death:
    the state holds TR z `4729` ahead of arm 0, the first row `3943` one step
    into arm 0's twelve-frame shot), and seated on the gate the engine built
    that shot again from the later pose, on a smaller increment and a pass
    longer."""
    try:
        with open(out / "rc" / "report.json") as f:
            states = json.load(f)["states"]
    except (OSError, ValueError, KeyError):
        return None
    detail = next((s.get("detail", {}).get("camera", "") for s in states), "")
    m = re.match(
        r"retail pitch/yaw/H=(-?\d+)/(-?\d+)/\d+ eye=\[(-?\d+), (-?\d+), (-?\d+)\] "
        r"focus=\[(-?\d+), (-?\d+)\]", detail)
    if not m:
        return None
    pitch, yaw, ex, ey, ez, fx, fz = (int(v) for v in m.groups())
    pitch = pitch - 4096 if pitch >= 2048 else pitch
    return ",".join(str(v) for v in (pitch, yaw, ex, ey, ez, fx, 0, fz))


def retail_opening_pose(out: Path, first_row: bool = False) -> str | None:
    """The camera pose on retail's cast begin (`0x28`, or a heal's `0x3C`), as
    `LEGAIA_DIAG_CAST_SEQ_SEAT_POSE`: `cam.csv`'s pitch, yaw and eye with
    `body.csv`'s focus. A harness seat: the child drops the capture's drive and
    runs no earlier actor, so its camera would open elsewhere and the band's
    opening passes would score that, not the framing law. A `--physical` film
    (`first_row`) opens on the state's own instant, mid-glide wherever the
    state sat, so it takes the recording's first row; the child seats it on
    the gate's tick."""
    if first_row and (pose := state_camera_pose(out)):
        return pose
    begin = (0x28, 0x3C)
    keep = (lambda r: True) if first_row else (lambda r: int(r["st7"]) in begin)
    try:
        with open(out / "retail" / "cam.csv") as f:
            cam = next((r for r in csv.DictReader(f) if keep(r)), None)
        with open(out / "retail" / "body.csv") as f:
            body = next((r for r in csv.DictReader(f) if keep(r)), None)
    except OSError:
        return None
    if cam is None or body is None:
        return None
    pitch = int(cam["pitch"])
    pitch = pitch - 4096 if pitch >= 2048 else pitch
    return ",".join(str(v) for v in (
        pitch, cam["yaw"], cam["trx"], cam["try"], cam["trz"], body["fx"], 0, body["fz"]))


def state_guard_seats(sstate: str) -> str | None:
    """The party seats a PCSX-Redux state holds in the Spirit stance
    (`+0x1DE == 4` on actor `(&DAT_801C9370)[seat]`), as a comma list, for a
    recording older than the probe's `guard=` field. `None` when the state
    cannot be read."""
    exe = REPO / "target" / PROFILE / "pcsxr-state"
    if not exe.exists() or not sstate:
        return None
    if not Path(sstate).exists():
        # A recording made from another worktree names that worktree's
        # library; the states are fingerprint-named, so look up the same file.
        sstate = str(REPO / "saves" / "library" / "pcsx-redux" / Path(sstate).name)
        if not Path(sstate).exists():
            return None
    base, top = 0x80000000, 0x80200000
    tmp = REPO / "target" / "cast-sequence-guard-ram.bin"
    r = subprocess.run([str(exe), "extract", sstate, "--start", hex(base), "--end", hex(top),
                        "--out", str(tmp)], capture_output=True)
    if r.returncode != 0 or not tmp.exists():
        return None
    ram = tmp.read_bytes()
    tmp.unlink()

    def u32(va: int) -> int:
        o = va - base
        return int.from_bytes(ram[o:o + 4], "little") if 0 <= o <= len(ram) - 4 else 0

    seats = []
    for s in range(3):
        a = u32(0x801C9370 + 4 * s)
        if not base <= a < top - 0x200:
            continue
        o = a - base
        max_hp = int.from_bytes(ram[o + 0x14E:o + 0x150], "little")
        if max_hp > 0 and ram[o + 0x1DE] == 4:
            seats.append(str(s))
    return ",".join(seats)


def engine_guard(a, out: Path, done: dict) -> str:
    """The party seats the engine child stands in the Spirit stance.

    Both per-move wrappers double the defender roll on `+0x1DE == 4` and the
    finisher halves the hit, so a capture whose victims guarded takes about
    half the engine's unguarded damage. `--guard` wins (`-` = none); else the
    probe's `guard=` (the stance at cast begin); else, for a recording older
    than that field, the injected state's own stance - unless the run picked
    the monster's turn rounds on from the state, whose stance it no longer
    holds."""
    if a.guard:
        return "" if a.guard == "-" else a.guard
    if "guard" in done:
        return done["guard"]
    manifest = read_kv(out / "retail" / "manifest.txt")
    if a.physical or int(manifest.get("monster_pick", "0") or 0):
        return ""
    seats = state_guard_seats(manifest.get("sstate", ""))
    if seats:
        print(f"[engine] guard {seats} from the state's +0x1DE (recording predates guard=)")
    return seats or ""


def retail_opening_cursor(out: Path) -> str | None:
    """`clip,cursor` of the caster on retail's cast begin, for
    `LEGAIA_DIAG_CAST_SEQ_SEAT_CURSOR`. A harness seat, like the pose."""
    begin = (0x28, 0x3C)
    try:
        with open(out / "retail" / "body.csv") as f:
            body = next((r for r in csv.DictReader(f) if int(r["st7"]) in begin), None)
    except OSError:
        return None
    return None if body is None else f"{body['clip']},{body['cursor']}"


def retail_rand_seed(out: Path, key: str) -> str | None:
    """`st7:phase:seed` for `LEGAIA_CAST_RAND_SEED`: the BIOS rand() seed
    word (`rand` in the probe's meta.csv) on retail's last vsync under the
    cast key `st7:phase` (each a number, `0x` allowed). A recording made
    before the probe logged the word has no `rand` column."""
    st7, phase = (int(v, 0) for v in key.split(":"))
    seed = None
    with open(out / "retail" / "meta.csv") as f:
        for r in csv.DictReader(f):
            if r.get("rand") and int(r["st7"]) == st7 and int(r["phase"]) == phase:
                seed = int(r["rand"])
    return None if seed is None else f"{st7}:{phase}:{seed}"


def record_key(exe: str, env: dict, args: list[str]) -> str:
    """What an engine recording is a function of: the binary's bytes, the
    child's `LEGAIA_*` environment and arguments, the contents of every file
    an argument names, and the disc's identity. The child is tick-locked, so a
    matching key means the recording it already made."""
    h = hashlib.sha256()
    with open(exe, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    for k, v in sorted(env.items()):
        if k.startswith(("LEGAIA_", "WGPU_")):
            h.update(f"{k}={v}\0".encode())
    for t in args:
        h.update(f"{t}\0".encode())
        p = Path(t)
        if p.is_file():
            h.update(p.read_bytes())
        elif (p / "PROT.DAT").is_file():
            st = (p / "PROT.DAT").stat()
            h.update(f"{st.st_size} {st.st_mtime_ns}\0".encode())
    return h.hexdigest()


def retail_step_track(out: Path) -> str:
    """`state phase step` per retail frame, in film order, for
    `LEGAIA_CAST_STEP_TRACK`: a frame's row is its last, and the phase counts
    only inside the module bands, as the pairing's key does."""
    lines, last = [], None
    with open(out / "retail" / "meta.csv") as f:
        for r in csv.DictReader(f):
            if last is not None and r["frame"] != last["frame"]:
                lines.append(_step_row(last))
            last = r
    if last is not None:
        lines.append(_step_row(last))
    return "".join(lines)


def retail_rand_track(out: Path) -> str:
    """`state phase word` per retail frame inside the module bands, for
    `LEGAIA_CAST_RAND_TRACK`: the BIOS rand() seed word on the frame's last
    row, which the next frame's pass draws from, keyed as the step track keys
    a frame. The key the recording opens on is left out: the capture's
    instant falls inside it, so its first frames are not the band's."""
    lines, last, first_key = [], None, None
    with open(out / "retail" / "meta.csv") as f:
        rows = list(csv.DictReader(f))
    if not rows or not rows[0].get("rand"):
        return ""
    for r in rows + [None]:
        if last is not None and (r is None or r["frame"] != last["frame"]):
            st7 = int(last["st7"])
            if st7 in (0x35, 0x36, 0x70):
                key = (st7, int(last["phase"]))
                if first_key is None:
                    first_key = key
                if key != first_key:
                    lines.append(f"{key[0]} {key[1]} {int(last['rand'])}\n")
        if r is not None and first_key is None and int(r["st7"]) not in (0x35, 0x36, 0x70):
            first_key = (-1, -1)
        last = r
    return "".join(lines)


def _step_row(r: dict) -> str:
    st7 = int(r["st7"])
    phase = int(r["phase"]) if st7 in (0x35, 0x36, 0x70) else 0
    return f"{st7} {phase} {r['step']}\n"


def traced_hit_words(out: Path) -> list[str] | None:
    """The seed word each melee hit of the first action drew from, off a
    `--hit-trace` recording's `hits.csv` (`word@vsync`), or `None` without
    one. The probe reads the word at the head of the strike kernel's hit body
    (`0x801EC5A8`), ahead of the block roll, so a blocked or missed swing has
    its word as well as a landed one. Rows stop where the meta's first HP-bar
    settle (`0x51`) begins, as the burst reading does."""
    path = out / "retail" / "hits.csv"
    if not path.exists():
        return None
    settle = None
    with open(out / "retail" / "meta.csv") as f:
        for r in csv.DictReader(f):
            if int(r["st7"]) == 0x51:
                settle = int(r["vsync"])
                break
    words = []
    with open(path) as f:
        for r in csv.DictReader(f):
            v = int(r["vsync"])
            if settle is not None and v >= settle:
                break
            words.append(f"{int(r['seed']) & 0xFFFFFFFF}@{v}")
    return words


def retail_hit_words(out: Path) -> list[str]:
    """The seed word retail held going into each melee hit of a `--physical`
    recording's first action, as `word@vsync`. A hit's spark burst makes its
    frame draw tens of times where any other frame draws once or twice, so the
    frame is read off the distance between two rows' words on the BIOS
    generator. The word steps on once, for the frame driver's discard
    (`0x80046D2C`) ahead of the hit. A blocked or missed swing draws no burst
    and leaves no word, so each word carries its vsync and the engine hands it
    to the hit nearest that time."""
    traced = traced_hit_words(out)
    if traced is not None:
        return traced
    step = lambda w: (w * 0x41C64E6D + 0x3039) & 0xFFFFFFFF
    words, prev, settled = [], None, False
    with open(out / "retail" / "meta.csv") as f:
        for r in csv.DictReader(f):
            st7 = int(r["st7"])
            if settled and st7 != 0x51:
                break
            settled = settled or st7 == 0x51
            w = int(r["rand"])
            if prev is not None and w != prev:
                s, n = prev, 0
                while s != w and n < 256:
                    s, n = step(s), n + 1
                if HIT_FRAME_DRAWS <= n < 256 and not settled:
                    words.append(f"{step(prev)}@{r['vsync']}")
            prev = w
    return words


def run_engine(a, out: Path) -> bool:
    kept = child_cmd(a, out)
    if kept is None:
        return False
    toks = shlex.split(kept.read_text())
    assert toks[0] == "cd" and toks[2] == "&&", f"unexpected child command shape in {kept}"
    work = Path(toks[1])
    rest = toks[3:]
    env = dict(os.environ)
    i = 0
    child_inflight = None
    while i < len(rest) and "=" in rest[i] and not rest[i].startswith("/"):
        k, v = rest[i].split("=", 1)
        if k == "LEGAIA_BATTLE_INFLIGHT":
            child_inflight = v
        # A physical recording walks the state's own pad path to the strike.
        if k not in DROP_ENV or (a.physical and k in PHYSICAL_KEEP_ENV):
            env[k] = v
        i += 1
    args = rest[i + 1:]
    if "LEGAIA_SEAT_VRAM_RECTS" in env:
        env["LEGAIA_SEAT_VRAM_RECTS_UNTIL_ACTION"] = "1"
    # The sound channel reads the audio director's command record, so the
    # child keeps its director, routed to no device (`LEGAIA_AUDIO_DETACHED`).
    if not a.no_sound:
        args = [t for t in args if t != "--no-audio"]
        env["LEGAIA_AUDIO_DETACHED"] = "1"
    exe = str(EXE)
    for j, t in enumerate(args):
        if t == "--screenshot-tick":
            args[j + 1] = "40000"
        if t == "--screenshot":
            args[j + 1] = str(work / "cast-sequence-unused.png")
    done = read_kv(out / "retail" / "done.txt")
    caster = int(done.get("caster", a.caster)) if done else a.caster
    if not done and child_inflight and a.caster == 0:
        # No retail recording yet (an engine-only look at a corpus mid-cast
        # state): keep the caster the state's own replay seeds.
        caster = int(child_inflight.split(",")[0])
    if a.physical:
        env["LEGAIA_DIAG_CAST_SEQ_BEGIN"] = "gate"
        # The child takes its frame on the drive's gate and leaves: push the
        # frame past the recording, which exits on its own tail.
        env["LEGAIA_DIAG_CAPTURE_TICK"] = "40000"
    elif caster < 3:
        ground = engine_ground(done.get("ground", "")) if not a.no_ground else ""
        env["LEGAIA_BATTLE_INFLIGHT"] = f"{caster},{a.spell},{a.target}" + (f";{ground}" if ground else "")
    else:
        # The seam takes the engine's actor index, and the engine packs the
        # party ahead of the monsters: retail seat 3 + m is engine slot
        # party_count + m. The party count is the seated party slots in
        # retail's ground list.
        pc = max(1, sum(1 for g in done.get("ground", "").split(",")[:3] if g not in ("", "-")))
        # The ground retail's earlier actions left (a party member's swing
        # walks it in, a knockback pushes the monster back): a walk-in body
        # walks from there, so its length is the capture's.
        ground = engine_ground(done.get("ground", "")) if not a.no_ground else ""
        # The target the monster's own pick (or the probe) left at cast begin:
        # a single-target cast lands on the same seat on both sides, and the
        # victim's voice cue and the framing follow it. A recording older than
        # that field takes --target.
        # `t` takes an engine slot too: a monster seat (a self-cast such as
        # Ultra Charge, whose pick stores the caster's own seat) re-keys as the
        # caster does, a group code (8 / 9) passes through.
        target = int(done.get("cast_target", "-1"))
        target = target if 0 <= target < 8 else a.target
        if 3 <= target < 8:
            target = pc + target - 3
        env["LEGAIA_BATTLE_MONSTER_CAST"] = f"{pc + caster - 3},{a.spell},p,t{target}" + (f";{ground}" if ground else "")
    if (pose := retail_opening_pose(out, first_row=a.physical)):
        env["LEGAIA_DIAG_CAST_SEQ_SEAT_POSE"] = pose
    if not a.physical and (cursor := retail_opening_cursor(out)):
        env["LEGAIA_DIAG_CAST_SEQ_SEAT_CURSOR"] = cursor
    edir = out / "engine"
    env.update({
        "LEGAIA_DIAG_CAST_SEQ": str(edir),
        "LEGAIA_DIAG_CAST_SEQ_SEAT": str(caster),
        "LEGAIA_DIAG_CAST_SEQ_TAIL": str(a.tail),
        # Past the retail side's own span (1500 grabs, every 2 vsyncs by
        # default): the recorder's 2000-frame default stops a long summon
        # (Mule) short of its last arms.
        "LEGAIA_DIAG_CAST_SEQ_MAX": "4000",
    })
    if guard := engine_guard(a, out, done):
        env["LEGAIA_BATTLE_GUARD"] = guard
    if a.learned and caster < 3:
        env["LEGAIA_CAST_LEARN"] = f"{caster},{a.spell},{a.learned}"
    if a.monster_hp and env.get("LEGAIA_BATTLE_BARS"):
        # `slot:hp:mp[:..]`, engine slots: the party's first, `129+` empty seats.
        pc = max(1, sum(1 for g in done.get("ground", "").split(",")[:3] if g not in ("", "-")))
        def raised(e: str) -> str:
            f = e.split(":")
            if pc <= int(f[0]) < 129 and int(f[1]) > 0:
                f[1] = str(a.monster_hp)
            return ":".join(f)
        env["LEGAIA_BATTLE_BARS"] = ",".join(raised(e) for e in env["LEGAIA_BATTLE_BARS"].split(","))
    if a.rand_at:
        seed = retail_rand_seed(out, a.rand_at)
        if seed is None:
            print(f"[engine] --rand-at {a.rand_at}: the retail recording has no rand seed under that key")
            return False
        env["LEGAIA_CAST_RAND_SEED"] = seed
    if a.rand_track:
        track = out / "retail-rand.txt"
        text = retail_rand_track(out)
        if not text:
            print("[engine] --rand-track: the retail recording has no rand words inside a module band")
            return False
        track.write_text(text)
        env["LEGAIA_CAST_RAND_TRACK"] = str(track)
    if a.retail_steps:
        track = out / "retail-steps.txt"
        track.write_text(retail_step_track(out))
        env["LEGAIA_CAST_STEP_TRACK"] = str(track)
    if a.strike_deal:
        env["LEGAIA_STRIKE_DEAL"] = ",".join(retail_hit_words(out))
    env.update(dict(kv.split("=", 1) for kv in a.engine_env))
    (out / "engine-cmd.txt").write_text(
        " ".join(f"{k}={shlex.quote(v)}" for k, v in sorted(env.items()) if k.startswith("LEGAIA_"))
        + " " + shlex.join([exe] + args) + "\n")
    key = record_key(exe, env, args)
    key_file = out / "engine-key.txt"
    if (not a.rerecord and (edir / "meta.csv").exists() and key_file.exists()
            and key_file.read_text() == key):
        print(f"[engine] cached: {edir} (same binary, command and inputs; --rerecord to run it again)")
        return True
    key_file.unlink(missing_ok=True)
    if edir.exists():
        shutil.rmtree(edir)
    seed = "pad drive" if a.physical else "inflight" if caster < 3 else "monster cast"
    print(f"[engine] recording the action ({seed} seed, seat {caster})")
    r = subprocess.run([exe] + args, cwd=work, env=env, capture_output=True, text=True,
                       timeout=a.engine_timeout)
    (out / "engine.log").write_text(r.stdout + r.stderr)
    if not (edir / "meta.csv").exists():
        print(f"[engine] FAILED: the action never began (status {r.returncode}); see {out / 'engine.log'}")
        return False
    if r.returncode == 0:
        key_file.write_text(key)
    print(f"[engine] {sum(1 for _ in edir.glob('e_*.png'))} frames")
    return True


def score(a, out: Path) -> int:
    cmd = [str(EXE), "cast-sequence-score", "--retail", str(out / "retail"),
           "--engine", str(out / "engine"), "--out", str(out),
           "--label", out.name, "--strip", str(a.strip)]
    if a.retail_lag is not None:
        cmd += ["--retail-lag", str(a.retail_lag)]
    if a.physical and (skip := gate_film_skip(out)):
        cmd += ["--engine-skip", str(skip)]
    if a.sound_tolerance is not None:
        cmd += ["--sound-tolerance", str(a.sound_tolerance)]
    if a.strict_drive:
        cmd += ["--strict-drive"]
    if a.camera_only:
        cmd += ["--camera-only"]
    r = subprocess.run(cmd, cwd=REPO)
    if r.returncode == 0 and not a.camera_only:
        rep = json.loads((out / "report.json").read_text())
        print(f"[score] {out.name}: mean {rep['mean_within']:.3f} over {rep['valid_pairs']} pairs "
              f"(min {rep['min_within']:.3f}; white {rep['white_pairs']}, dark {rep['dark_pairs']}, "
              f"unpaired {rep['unpaired_retail']}) -> {out / 'report.md'}")
        if w := rep.get("drive_wait"):
            # The module's CD-seek wait and its window: what the pairing absorbed.
            verdict = (f"absorbed, live shift {w['live_shift']:+}, {rep.get('drive_hold_pairs', 0)} hold "
                       f"frames, strict mean {rep['strict_mean_within']:.3f}") if w["accepted"] else "paired strictly"
            print(f"[score] {out.name}: CD-seek wait (arms {w['seek_arm']}/{w['poll_arm']}): engine "
                  f"{w['engine_frozen']} held + {w['engine_poll_hold']} poll, retail {w['retail_wait']}, "
                  f"window {w['window'][0]}..{w['window'][1]}: {verdict}")
        snd = rep.get("sound")
        if snd:
            print(f"[score] {out.name}: sound cue score {snd['score']:.3f} (id {snd['id_score']:.3f}): "
                  f"retail {snd['retail_cues']} engine {snd['engine_cues']} matched {len(snd['matched'])} "
                  f"timing {len(snd['timing'])} missing {len(snd['missing'])} extra {len(snd['extra'])}")
        else:
            print(f"[score] {out.name}: no sound channel (a side has no sfx.csv)")
    return r.returncode


def resident_patch_of(label: str) -> str | None:
    """The scenario's `resident_patch` in scripts/scenarios.toml, if any."""
    import tomllib
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    for sc in manifest.get("scenarios", []):
        if sc.get("label") == label:
            return sc.get("resident_patch")
    return None


def gate_film_skip(out: Path) -> int:
    """Engine ticks a gate film spends on the state before the pass retail's first
    row already holds.

    The two sides' first rows do not fall in the same place of a frame step: the
    retail recording starts at the save, a vsync into the state's frame or on its
    boundary, and the engine's gate tick is the seat. Counted on the pose, the
    leading rows on the seated pose (engine `cam.csv` row 0): a recording whose
    first row has already left it drops the engine's leading rows. A harness
    alignment over the camera channel, measured to help strike openings.
    """
    def lead(rows: list[dict], first: list[str]) -> int:
        n = 0
        for r in rows:
            if [r[k] for k in POSE_COLS] != first:
                break
            n += 1
        return n

    POSE_COLS = ("pitch", "yaw", "try", "trz")
    try:
        with open(out / "engine" / "cam.csv") as f:
            engine = list(csv.DictReader(f))
        with open(out / "retail" / "cam.csv") as f:
            retail = list(csv.DictReader(f))
    except FileNotFoundError:
        return 0
    if not engine:
        return 0
    first = [engine[0][k] for k in POSE_COLS]
    # A retail first row still on the state has not run its first pass, as the
    # engine's gate tick has not; only a recording whose first row has left the
    # state is a pass ahead.
    # One frame at most separates the two: a held camera (Jette's Shadow Break
    # stands still for its first arms) would otherwise count them all.
    return min(lead(engine, first), MAX_GATE_FILM_SKIP) if lead(retail, first) == 0 else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--spell", type=num, help="action id to inject (0x81..); not with --physical")
    ap.add_argument("--physical", action="store_true",
                    help="no inject: record the strike a mid-strike state (action state 0x1E) already "
                         "holds, on both sides, from the state's own instant to past the Done band")
    ap.add_argument("--state", required=True, help="scenarios.toml label of a PCSX-Redux battle state")
    ap.add_argument("--caster", type=num, default=0, help="seat whose action is rewritten (255 = any party seat)")
    ap.add_argument("--target", type=num, default=3, help="target seat")
    ap.add_argument("--formation", help="retail only: formation row bytes forced at FUN_801DA600 (comma list)")
    ap.add_argument("--cast-queue", metavar="HEX,HEX,..",
                    help="retail, with --monster-pick: bytes written behind the spell at +0x1E0 "
                         "(clip, shot, ff), as the state's own queue holds them")
    ap.add_argument("--monster-pick", action="store_true",
                    help="retail: a monster caster (--caster 3..7) gets the cast written where the AI "
                         "pick FUN_801E9FD4 returns (breakpoint core), so any battle state that reaches "
                         "its turn records any spell the monster owns a clip for")
    ap.add_argument("--allow-patched", action="store_true",
                    help="measure a state whose manifest entry carries resident_patch")
    ap.add_argument("--save-inject", action="store_true",
                    help="retail: keep the emulator state one vsync past the inject "
                         "(retail/state_inject.rawsstate), a pre-cast state for the save library")
    ap.add_argument("--party-spd", type=int, default=0, help="retail: write this SPD into the party seats")
    ap.add_argument("--every", type=int, default=2, help="retail vsyncs between frames")
    ap.add_argument("--tail", type=int, default=60, help="vsyncs / ticks past the Done band")
    ap.add_argument("--frames", type=int, default=6000, help="retail capture cap in vsyncs")
    ap.add_argument("--window", action="store_true",
                    help="retail: run the emulator in its window instead of headless (the same frames, "
                         "slower, and it takes a display)")
    ap.add_argument("--retail-timeout", type=int, default=900, help="emulator wall-clock cap, seconds")
    ap.add_argument("--engine-timeout", type=int, default=600, help="engine child wall-clock cap, seconds")
    ap.add_argument("--retail-lag", help="display lag in vsyncs, or 'auto' (2 * frame step, the default)")
    ap.add_argument("--strip", type=int, default=12, help="pairs in strip.png")
    ap.add_argument("--strict-drive", action="store_true",
                    help="pair a summon module's CD-seek wait strictly, with no window")
    ap.add_argument("--sound-tolerance", type=int,
                    help="sound channel: ticks a cue still matches at (default 6)")
    ap.add_argument("--watch", default="",
                    help="retail: log each store into these byte addresses (comma list) with its pc and "
                         "ra (retail/watch.csv; breakpoint core)")
    ap.add_argument("--range-trace", action="store_true",
                    help="retail: each range-law call FUN_8004E2F0 with the pairs it reads to retail/range.csv "
                         "(breakpoint core)")
    ap.add_argument("--hit-trace", action="store_true",
                    help="retail: each melee hit's rand() seed word to retail/hits.csv (breakpoint core), "
                         "which --strike-deal then deals in place of the spark-burst reading")
    ap.add_argument("--sfx-callers", action="store_true",
                    help="retail: log every call into the cue producers and routers with its ra "
                         "(retail/sfxcall.csv; breakpoint core)")
    ap.add_argument("--pose-objects", action="store_true",
                    help="retail: pose.csv, the summon creature's decoded object table at each burn emit "
                         "(breakpoint core; the engine half is LEGAIA_DIAG_CAST_PARTS's pose.csv)")
    ap.add_argument("--retail-parts", action="store_true",
                    help="retail: parts.csv, every live move-VM part per vsync (the engine half is LEGAIA_DIAG_CAST_PARTS)")
    ap.add_argument("--prim-calls", action="store_true",
                    help="retail: primcall.csv, each prim dispatcher entry and cued leaf (breakpoint core)")
    ap.add_argument("--no-sound", action="store_true",
                    help="engine: keep the child's --no-audio (no audio director, no sfx.csv)")
    ap.add_argument("--no-ground", action="store_true", help="do not seat the combatants on retail's ground pairs")
    ap.add_argument("--guard", default="", metavar="SEATS",
                    help="engine: party seats in the Spirit stance at the cast (comma list, `-` = none; "
                         "default: the recording's guard=, retail's +0x1DE == 4 at cast begin)")
    ap.add_argument("--rand-at", metavar="ST7:PHASE",
                    help="engine: hold the world rand() stream on retail's seed word under this cast key "
                         "(its last vsync there), so the arm that leaves it draws as retail's did")
    ap.add_argument("--rand-track", action="store_true",
                    help="engine: seat every module-band pass on the rand() word retail's frame before it "
                         "ended on (a harness seat: an arm that draws each pass, such as Earthquake's "
                         "rocks, plays retail's draws)")
    ap.add_argument("--retail-steps", action="store_true",
                    help="engine: run on the recording's own frame steps (a harness seat: it takes the "
                         "frame's cost out of what the film scores, so a law residual reads alone)")
    ap.add_argument("--strike-deal", action="store_true",
                    help="--physical: stand the engine's strike on the capture's queue, approach terms, "
                         "ATK and retail's word at each hit, so what differs is rules")
    ap.add_argument("--engine-env", action="append", default=[], metavar="K=V",
                    help="extra environment for the engine child (repeatable)")
    ap.add_argument("--out", help="output dir (default captures/cast-sequence/<state>_<spell>)")
    ap.add_argument("--recapture", action="store_true", help="re-run the retail capture")
    ap.add_argument("--rerecord", action="store_true",
                    help="run the engine recording even when its binary, command and inputs match the last one")
    ap.add_argument("--reseed", action="store_true", help="re-run retail-compare for the child command")
    ap.add_argument("--engine-only", action="store_true", help="skip the retail stage (needs a cached capture)")
    ap.add_argument("--retail-only", action="store_true")
    ap.add_argument("--score-only", action="store_true")
    ap.add_argument("--learned", type=int, default=0, metavar="LEVEL",
                    help="both sides: the spell sits on the caster's list at this level "
                         "(off the list, a summon's hits read a wrapped level and cap at 9999)")
    ap.add_argument("--monster-hp", type=int, default=0, metavar="HP",
                    help="both sides: every living monster's HP at the inject / first prompt, "
                         "so the cast's whole damage shows in the HP columns")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--camera-only", action="store_true",
                    help="score only the camera channel (camera.csv); no frame is read")
    a = ap.parse_args()
    if a.spell is None and not a.physical:
        ap.error("--spell is required without --physical")
    # The user's rule is retail states only. A state made on a patched disc
    # keeps the patched executable resident through every later load, and a
    # state saved from it (`--save-inject`) inherits it: the manifest's
    # `resident_patch` names those, and the driver refuses them unless told.
    patch = resident_patch_of(a.state)
    if patch and not a.allow_patched:
        print(f"[refuse] {a.state} holds a patched resident executable ({patch}); "
              "retail states only (--allow-patched to measure it anyway)")
        return 2

    out = Path(a.out) if a.out else REPO / "captures" / "cast-sequence" / f"{a.state}_{'strike' if a.physical else format(a.spell, '02x')}"
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not a.no_build and not a.retail_only:
        subprocess.run(["cargo", "build", "-p", "legaia-engine-shell", "--profile", PROFILE,
                        "--features", "oracles", "--bin", "legaia-engine"], cwd=REPO, check=True)
    if not a.score_only and not a.engine_only:
        if not run_retail(a, out):
            return 0 if not os.environ.get("LEGAIA_DISC_BIN") else 1
    if a.retail_only:
        return 0
    if not a.score_only and not run_engine(a, out):
        return 1
    if not (out / "retail" / "meta.csv").exists():
        print(f"[score] no retail recording under {out / 'retail'}: engine side only")
        return 0
    return score(a, out)


if __name__ == "__main__":
    sys.exit(main())
