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
             metric, write `report.md` / `report.json` / `strip.png`.

    scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu
    scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu --engine-only

The outputs hold retail pixels and RAM: they live under the gitignored
`captures/` (default) or a scratch directory, never in the repo. See
`docs/tooling/cast-sequence-oracle.md`.
"""

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PROFILE = "release-test"
EXE = REPO / "target" / PROFILE / "legaia-engine"
PROBE = "scripts/pcsx-redux/autorun_cast_sequence.lua"
# Env the retail-compare child carries that would steer the fight somewhere
# else than the injected cast: its own capture gate, pad drive and replay.
DROP_ENV = {
    "LEGAIA_BATTLE_DRIVE", "LEGAIA_CAPTURE_GATE", "LEGAIA_CAPTURE_DISPLAY_LAG",
    "LEGAIA_BATTLE_INFLIGHT", "LEGAIA_BATTLE_MONSTER_CAST", "LEGAIA_DIAG_CAPTURE_TICK",
    "LEGAIA_BATTLE_CAM_ALIGN", "LEGAIA_BATTLE_FRAME_STEP", "LEGAIA_SEAT_HUD_GLIDES",
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

    The probe writes one `x:z` (or `-`) per retail pool slot: the party at
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
        "LEGAIA_SPELL": str(a.spell),
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
    if a.formation:
        env["LEGAIA_FORCE_ROW"] = a.formation
    if a.party_spd:
        env["LEGAIA_PARTY_SPD"] = str(a.party_spd)
    cmd = ["timeout", "--kill-after=10", str(a.retail_timeout), "bash",
           "scripts/pcsx-redux/run_probe.sh", "--lua", PROBE, "--scenario", a.state,
           "--iso", disc, "--out-dir", str(rdir), "--frames", str(a.frames)]
    # A forced formation row needs the breakpoint core; everything else runs
    # on the recompiler (vsync hooks only).
    if not a.formation:
        cmd.insert(5, "--fast")
    print(f"[retail] capturing {a.state} spell 0x{a.spell:02X} (an emulator run, minutes)")
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
           "--images", "--filter", a.state]
    print(f"[engine] seeding {a.state} through retail-compare")
    subprocess.run(cmd, cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if not kept.exists():
        print(f"[engine] FAILED: retail-compare kept no child command for {a.state} "
              f"(see {out / 'rc' / 'report.md'})")
        return None
    return kept


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
        if k not in DROP_ENV:
            env[k] = v
        i += 1
    args = rest[i + 1:]
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
    if caster < 3:
        ground = engine_ground(done.get("ground", "")) if not a.no_ground else ""
        env["LEGAIA_BATTLE_INFLIGHT"] = f"{caster},{a.spell},{a.target}" + (f";{ground}" if ground else "")
    else:
        # The seam takes the engine's actor index, and the engine packs the
        # party ahead of the monsters: retail seat 3 + m is engine slot
        # party_count + m. The party count is the seated party slots in
        # retail's ground list.
        pc = max(1, sum(1 for g in done.get("ground", "").split(",")[:3] if g not in ("", "-")))
        env["LEGAIA_BATTLE_MONSTER_CAST"] = f"{pc + caster - 3},{a.spell}"
    edir = out / "engine"
    if edir.exists():
        shutil.rmtree(edir)
    env.update({
        "LEGAIA_DIAG_CAST_SEQ": str(edir),
        "LEGAIA_DIAG_CAST_SEQ_SEAT": str(caster),
        "LEGAIA_DIAG_CAST_SEQ_TAIL": str(a.tail),
        # Past the retail side's own span (1500 grabs, every 2 vsyncs by
        # default): the recorder's 2000-frame default stops a long summon
        # (Mule) short of its last arms.
        "LEGAIA_DIAG_CAST_SEQ_MAX": "4000",
    })
    env.update(dict(kv.split("=", 1) for kv in a.engine_env))
    (out / "engine-cmd.txt").write_text(
        " ".join(f"{k}={shlex.quote(v)}" for k, v in sorted(env.items()) if k.startswith("LEGAIA_"))
        + " " + shlex.join([exe] + args) + "\n")
    print(f"[engine] recording the cast ({'inflight' if caster < 3 else 'monster cast'} seed, caster seat {caster})")
    r = subprocess.run([exe] + args, cwd=work, env=env, capture_output=True, text=True,
                       timeout=a.engine_timeout)
    (out / "engine.log").write_text(r.stdout + r.stderr)
    if not (edir / "meta.csv").exists():
        print(f"[engine] FAILED: the cast never began (status {r.returncode}); see {out / 'engine.log'}")
        return False
    print(f"[engine] {sum(1 for _ in edir.glob('e_*.png'))} frames")
    return True


def score(a, out: Path) -> int:
    cmd = [str(EXE), "cast-sequence-score", "--retail", str(out / "retail"),
           "--engine", str(out / "engine"), "--out", str(out),
           "--label", out.name, "--strip", str(a.strip)]
    if a.retail_lag is not None:
        cmd += ["--retail-lag", str(a.retail_lag)]
    r = subprocess.run(cmd, cwd=REPO)
    if r.returncode == 0:
        rep = json.loads((out / "report.json").read_text())
        print(f"[score] {out.name}: mean {rep['mean_within']:.3f} over {rep['valid_pairs']} pairs "
              f"(min {rep['min_within']:.3f}; white {rep['white_pairs']}, dark {rep['dark_pairs']}, "
              f"unpaired {rep['unpaired_retail']}) -> {out / 'report.md'}")
    return r.returncode


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--spell", type=num, required=True, help="action id to inject (0x81..)")
    ap.add_argument("--state", required=True, help="scenarios.toml label of a PCSX-Redux battle state")
    ap.add_argument("--caster", type=num, default=0, help="seat whose action is rewritten (255 = any party seat)")
    ap.add_argument("--target", type=num, default=3, help="target seat")
    ap.add_argument("--formation", help="retail only: formation row bytes forced at FUN_801DA600 (comma list)")
    ap.add_argument("--party-spd", type=int, default=0, help="retail: write this SPD into the party seats")
    ap.add_argument("--every", type=int, default=2, help="retail vsyncs between frames")
    ap.add_argument("--tail", type=int, default=60, help="vsyncs / ticks past the Done band")
    ap.add_argument("--frames", type=int, default=6000, help="retail capture cap in vsyncs")
    ap.add_argument("--retail-timeout", type=int, default=900, help="emulator wall-clock cap, seconds")
    ap.add_argument("--engine-timeout", type=int, default=600, help="engine child wall-clock cap, seconds")
    ap.add_argument("--retail-lag", help="display lag in vsyncs, or 'auto' (2 * frame step, the default)")
    ap.add_argument("--strip", type=int, default=12, help="pairs in strip.png")
    ap.add_argument("--no-ground", action="store_true", help="do not seat the combatants on retail's ground pairs")
    ap.add_argument("--engine-env", action="append", default=[], metavar="K=V",
                    help="extra environment for the engine child (repeatable)")
    ap.add_argument("--out", help="output dir (default captures/cast-sequence/<state>_<spell>)")
    ap.add_argument("--recapture", action="store_true", help="re-run the retail capture")
    ap.add_argument("--reseed", action="store_true", help="re-run retail-compare for the child command")
    ap.add_argument("--engine-only", action="store_true", help="skip the retail stage (needs a cached capture)")
    ap.add_argument("--retail-only", action="store_true")
    ap.add_argument("--score-only", action="store_true")
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args()

    out = Path(a.out) if a.out else REPO / "captures" / "cast-sequence" / f"{a.state}_{a.spell:02x}"
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not a.no_build and not a.retail_only:
        subprocess.run(["cargo", "build", "-p", "legaia-engine-shell", "--profile", PROFILE,
                        "--bin", "legaia-engine"], cwd=REPO, check=True)
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
