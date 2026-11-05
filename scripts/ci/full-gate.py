#!/usr/bin/env python3
"""The end-of-work gate as one command: everything a change owes before it merges.

    scripts/ci/full-gate.py                  # the whole gate, every failure reported
    scripts/ci/full-gate.py --list           # what would run, and how
    scripts/ci/full-gate.py --only repo,fmt  # a subset: gate names, globs or groups
    scripts/ci/full-gate.py --skip 'ladder-seed-*'
    scripts/ci/full-gate.py --resume         # rerun what did not pass on this tree
    scripts/ci/full-gate.py --selftest       # the parser and the scheduler, on fakes

What it runs, in groups:

    repo     every `run:` step of main-ci.yml's Repo Gates job, in a clean
             `git worktree add HEAD` with no disc in the environment and no
             gitignored data, as the runner sees them
    tree     fmt, clippy and the wasm type-check (read from the PR Lint job),
             the same job's ratchet steps again where the data is, the baseline
             loosening gate against --base, tracked-but-ignored site files,
             revert-scan
    tests    the workspace tests with the disc (read from the Disc-Gated Tests job)
    ladder   part B, part C default, and every seed `[continuous_seeds]` names
    retail   retail-compare --images --check, audio sweep, PCM check, capture coverage
    wasm     the bundle build, its freshness, headless page checks, the cost ratchet

Run lines come out of the workflow file each time, so a step added there is a
gate here with no edit. A step's name becomes its gate's name: `ci-<slug>`.

It reports, it does not decide: no gate blesses, proposes or pushes, a command
carrying such a flag is refused before anything runs, and the `tree-unchanged`
gate fails if a tracked file moved during the run. Ratchet modes always measure
fresh (`--check` reads no result cache).

A skip is not a pass. A gate whose driver printed `[skip]` is reported as SKIP
and the verdict line says so, except in the `repo` group, where a ratchet that
skips for want of the disc is the runner's own behaviour.

Output lands under --out (default target/full-gate): one log per gate in `logs/`,
`timing.md`, and `state.json` for --resume. Exit 0 only if nothing failed.

Parallelism: --max-cpu is a budget (default 12 of the box's cores; it is shared)
that each gate is weighed against, --jobs is what retail-compare, the audio sweep
and libtest get, and --cargo-jobs is CARGO_BUILD_JOBS. --memory-max caps the
engine-looping gates through `systemd-run --user --scope`. See
docs/tooling/build-and-test-speed.md#the-full-gate.
"""

from __future__ import annotations

import argparse
import dataclasses
import fnmatch
import hashlib
import json
import os
import re
import resource
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Callable

REPO = Path(__file__).resolve().parents[2]
WORKFLOW = Path(".github/workflows/main-ci.yml")
SPINE_BASELINE = Path("scripts/replays/full_game_baseline.toml")
PROFILE = "release-test"

# A gate here never writes a baseline and never publishes anything.
FORBIDDEN = re.compile(
    r"--bless\b|--update-baseline\b|--write-baseline\b|--propose\b|\bgit\s+push\b"
    r"|\bgit\s+commit\b|\bgh\s+(pr|release)\b"
)
# The data a tree needs for its gates to measure anything.
DATA_PATHS = ("extracted/PROT.DAT", "saves/library", "captures", "ghidra/scripts/funcs")
# The line of a failed gate's log worth putting in the table.
FAILURE_LINE = re.compile(r"\b(?:FAILED|problem\(s\)|error(?:\[|:)|panicked|failure|regress\w*|drops?|Diff in)\b", re.I)
ANSI = re.compile(r"\x1b(?:\[[0-9;]*[A-Za-z]|\([A-Z0-9])")
SKIP_LINE = re.compile(r"^\s*\[skip\]|\bSKIPPED\b|^\[no baseline\]", re.M)

# The order gates are offered to the budget, ahead of plan order. The wall clock is
# set by the chain the display lock serialises (retail-compare, the smokes, the page
# gates), and two six-core gates side by side fill the default budget, so that chain
# and the cargo-rt chain under the tests each hold a slot from the start. Lints come
# early because a lint failure is worth hearing about in the first minutes. The seeds
# need a slot and block nothing, so they take what is left.
START_FIRST = (
    "engine-bin", "ladder-bin", "tests-build", "tests-disc",
    "lint-clippy", "lint-clippy-engine-shell-oracles", "lint-wasm32-type-check",
    "retail-compare", "smoke-clean", "smoke-enhanced",
    "wasm-build", "wasm-freshness", "pages", "quality-ladder", "cost-ratchet",
)

PASS, FAIL, WARN, SKIP, BLOCKED, UNRUN = "PASS", "FAIL", "WARN", "SKIP", "BLOCKED", "NOT RUN"
OK_STATUSES = (PASS, WARN, SKIP)


# -- The workflow file --------------------------------------------------------


@dataclasses.dataclass
class Step:
    name: str
    run: str
    env: dict[str, str]


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _scalar(text: str) -> str:
    text = text.strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "\"'":
        return text[1:-1]
    return re.sub(r"\s+#.*$", "", text)


def workflow_env(text: str) -> dict[str, str]:
    """The top-level `env:` block."""
    out: dict[str, str] = {}
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.rstrip() == "env:":
            for sub in lines[i + 1 :]:
                if not sub.strip() or sub.lstrip().startswith("#"):
                    continue
                if _indent(sub) == 0:
                    break
                key, _, value = sub.strip().partition(":")
                out[key] = _scalar(value)
            break
    return out


def workflow_steps(text: str, job: str) -> list[Step]:
    """A job's steps with a `run:`, read by indentation.

    GitHub's own subset of YAML, the shape this one file is written in: `jobs:` at
    column 0, a job at 2, `steps:` at 4, `- ` at 6, a step's keys at 8. Standard
    library only, as every gate here is; `--selftest` holds it against PyYAML where
    that is installed.
    """
    lines = text.splitlines()
    i = lines.index("jobs:") + 1
    while i < len(lines) and lines[i].rstrip() != f"  {job}:":
        i += 1
    if i == len(lines):
        raise SystemExit(f"{WORKFLOW}: no job {job!r}")
    i += 1
    while i < len(lines) and lines[i].rstrip() != "    steps:":
        if lines[i].strip() and _indent(lines[i]) <= 2:
            raise SystemExit(f"{WORKFLOW}: job {job!r} has no steps")
        i += 1
    i += 1
    steps: list[Step] = []
    cur: dict | None = None

    def flush() -> None:
        if cur and cur.get("run") is not None:
            steps.append(Step(cur.get("name") or cur["run"].split("\n")[0], cur["run"], cur.get("env", {})))

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            i += 1
            continue
        ind = _indent(line)
        if ind <= 4:
            break
        if ind == 6 and stripped.startswith("- "):
            flush()
            cur = {}
            line = " " * 8 + stripped[2:]
            stripped = line.strip()
            ind = 8
        if ind == 8 and cur is not None:
            key, _, value = stripped.partition(":")
            value = value.strip()
            if key == "run" and value[:1] in ("|", ">"):
                i += 1
                body = []
                while i < len(lines) and (not lines[i].strip() or _indent(lines[i]) > 8):
                    body.append(lines[i][10:] if lines[i].strip() else "")
                    i += 1
                cur["run"] = "\n".join(body).strip("\n") + "\n"
                continue
            if key == "run":
                cur["run"] = _scalar(value)
            elif key == "name":
                cur["name"] = _scalar(value)
            elif key == "env":
                env = {}
                i += 1
                while i < len(lines) and (not lines[i].strip() or _indent(lines[i]) > 8):
                    if lines[i].strip() and not lines[i].lstrip().startswith("#"):
                        k, _, v = lines[i].strip().partition(":")
                        env[k] = _scalar(v)
                    i += 1
                cur["env"] = env
                continue
        i += 1
    flush()
    return steps


def slug(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


# -- Gates --------------------------------------------------------------------


@dataclasses.dataclass
class Gate:
    name: str
    group: str
    # A shell string, run by `bash -e` as Actions runs a step, or a Python function
    # taking the open log and returning an exit code.
    cmd: str | Callable | None
    cwd: Path | None = None
    env: dict[str, str] = dataclasses.field(default_factory=dict)
    unset: tuple[str, ...] = ()
    cpu: int = 1
    locks: tuple[str, ...] = ()
    needs: tuple[str, ...] = ()
    scope: bool = False
    default: bool = True
    timeout: int = 4 * 3600
    # A `[skip]` line in the log means SKIP rather than PASS.
    honest_skip: bool = True
    # log text -> (status override or None, detail for the table)
    read_log: Callable[[str], tuple[str | None, str]] | None = None
    # Answers without running (status, detail), or None to run.
    reuse: Callable[[], tuple[str, str] | None] | None = None
    # Always run again on --resume: the step leaves nothing --resume can rely on.
    always: bool = False
    # Called with the log path after a successful run.
    after: Callable[[Path], None] | None = None


class Plan:
    def __init__(self, a: argparse.Namespace, root: Path):
        self.a = a
        self.root = root
        self.out = a.out
        self.logs = self.out / "logs"
        self.bin = self.out / "bin"
        self.clean = self.out / "clean-tree"
        self.ci_root = self.out / "ci-target" / "legaia-ci-target"
        self.ci_env: dict[str, str] = {}
        self.gates: dict[str, Gate] = {}

    def add(self, gate: Gate) -> None:
        if gate.name in self.gates:
            raise SystemExit(f"two gates named {gate.name}")
        if isinstance(gate.cmd, str) and FORBIDDEN.search(gate.cmd):
            raise SystemExit(f"refusing {gate.name}: a gate may not bless, propose or push:\n{gate.cmd}")
        self.gates[gate.name] = gate


def git(root: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=True).stdout


def tree_key(root: Path) -> str:
    """What `--resume` and `tree-unchanged` compare: HEAD plus any local edit."""
    h = hashlib.sha256()
    h.update(git(root, "rev-parse", "HEAD").encode())
    h.update(git(root, "status", "--porcelain").encode())
    h.update(git(root, "diff", "HEAD").encode())
    return h.hexdigest()


def reach_detail(text: str) -> tuple[str | None, str]:
    found = re.findall(r"reaches (\d+/\d+) milestones[^;\n]*?(\d+) reload", text)
    if found:
        return None, f"reaches {found[-1][0]}, {found[-1][1]} reload(s)"
    found = re.findall(r"reaches (\d+/\d+)", text)
    return None, f"reaches {found[-1]}" if found else ""


def libtest_detail(text: str) -> tuple[str | None, str]:
    tally = re.findall(r"^test result: \w+\. (\d+) passed; (\d+) failed", text, re.M)
    if not tally:
        return None, ""
    passed = sum(int(p) for p, _ in tally)
    failed = sum(int(f) for _, f in tally)
    names = re.findall(r"^test (\S+) .*\.\.\. FAILED$", text, re.M)
    shown = ", ".join(sorted(set(names))[:6])
    return None, f"{passed} passed, {failed} failed" + (f": {shown}" if shown else "")


def regressions(text: str) -> tuple[str | None, str]:
    """Retail-compare's own list of what dropped, by label and channel."""
    rows = re.findall(r"^\[regression\] (\S+): ([\d.]+) < baseline ([\d.]+)", text, re.M)
    return None, ", ".join(f"{label} {now}<{base}" for label, now, base in rows)


def tail_detail(pattern: str) -> Callable[[str], tuple[str | None, str]]:
    def read(text: str) -> tuple[str | None, str]:
        found = re.findall(pattern, text, re.M)
        return None, found[-1].strip()[:110] if found else ""

    return read


def build_plan(a: argparse.Namespace, root: Path) -> Plan:
    p = Plan(a, root)
    text = (root / WORKFLOW).read_text()
    top_env = workflow_env(text)

    # Whatever the workflow's env says, the runner's own cache is not this script's.
    top_env.pop("CARGO_TARGET_DIR", None)
    no_data = ("LEGAIA_DISC_BIN", "LEGAIA_SAVES_LIBRARY", "LEGAIA_EXTRACTED_DIR", "LEGAIA_SCUS")

    def step_env(step: Step) -> dict[str, str]:
        # An Actions expression has no value here. The one in use is BASE_SHA, whose
        # step then takes its self-test branch; `loosening-base` below does the rest.
        return {**top_env, **{k: "" if "${{" in v else v for k, v in step.env.items()}}

    # -- repo: the Repo Gates job, on a clean checkout of HEAD ----------------
    p.add(Gate("clean-tree", "repo", lambda log: make_clean_tree(p, log), always=True))
    after_select: list[str] = []
    ratchets: list[Step] = []
    # What the clean tree holds: HEAD's job, not an uncommitted edit to it.
    head_text = subprocess.run(
        ["git", "show", f"HEAD:{WORKFLOW}"], cwd=root, capture_output=True, text=True
    ).stdout or text
    for step in workflow_steps(head_text, "gates"):
        name = "ci-" + slug(step.name)
        compiles = "cargo " in step.run or "check-expansion-packs" in step.run
        p.add(
            Gate(
                name,
                "repo",
                step.run,
                cwd=p.clean,
                env={**step_env(step), "CARGO_TARGET_DIR": str(p.ci_root)},
                unset=no_data,
                needs=("clean-tree", *after_select),
                locks=("cargo-clean",) if compiles else (),
                cpu=a.cargo_jobs if "check-expansion-packs" in step.run else 1,
                honest_skip=False,
                always="select-ci-target-dir" in step.run,
            )
        )
        if "select-ci-target-dir" in step.run:
            after_select = [name]
        if "ratchet" in step.name.lower():
            ratchets.append(step)

    p.add(Gate("site-tracked-not-ignored", "repo", lambda log: tracked_not_ignored(p, log), needs=("clean-tree",)))

    # -- tree: the PR Lint job's own lines, then what needs the data ---------
    p.add(Gate("preflight", "tree", lambda log: preflight(p, log), always=True))
    test_step = None
    for step in workflow_steps(text, "pr-lint"):
        if "select-ci-target-dir" in step.run:
            continue
        if step.run.startswith("cargo test"):
            test_step = step
            continue
        is_wasm = "wasm" in step.run
        p.add(
            Gate(
                "lint-" + slug(step.name),
                "tree",
                step.run,
                env=step_env(step),
                cpu=a.cargo_jobs if "cargo fmt" not in step.run else 1,
                locks=("cargo-wasm",) if is_wasm else ("cargo-dev",) if "cargo fmt" not in step.run else (),
                scope="cargo fmt" not in step.run,
            )
        )
    # The byte-accounting ratchet reads a sweep nothing else produces.
    p.add(
        Gate(
            "byte-sweep",
            "tree",
            f"cargo build -p legaia-asset --profile {PROFILE} --bin asset\n"
            f"python3 scripts/asset-investigation/byte-account-sweep.py --asset target/{PROFILE}/asset --jobs {a.jobs}\n",
            cpu=a.jobs,
            locks=("cargo-rt",),
            scope=True,
            needs=("preflight",),
        )
    )
    for step in ratchets:
        byte = "byte-account" in step.run
        p.add(
            Gate(
                "data-" + slug(step.name),
                "tree",
                step.run,
                env=step_env(step),
                needs=("preflight", "byte-sweep") if byte else ("preflight",),
            )
        )
    p.add(
        Gate(
            "loosening-base",
            "tree",
            f"python3 scripts/ci/check-baseline-loosening.py --base {a.base}",
        )
    )
    p.add(
        Gate(
            "revert-scan",
            "tree",
            f"python3 scripts/ci/revert-scan.py --base {a.base}",
            read_log=lambda t: (
                (WARN, f"{t.count(chr(10) + '    ')} line runs to read")
                if "revert-scan: nothing" not in t
                else (None, "")
            ),
        )
    )

    # -- what the rest shares: one build of each binary ----------------------
    build = (
        f"cargo build -p legaia-engine-shell --profile {PROFILE} --features oracles --bin legaia-engine\n"
        f'mkdir -p "{p.bin}"\ncp -f target/{PROFILE}/legaia-engine "{p.bin}/legaia-engine"\n'
    )
    p.add(
        Gate(
            "engine-bin",
            "build",
            build,
            cpu=a.cargo_jobs,
            locks=("cargo-rt",),
            scope=True,
            needs=("preflight",),
            always=True,
        )
    )
    p.add(
        Gate(
            "ladder-bin",
            "build",
            build_ladder_cmd(p),
            cpu=a.cargo_jobs,
            locks=("cargo-rt",),
            scope=True,
            needs=("preflight",),
            always=True,
        )
    )

    # -- tests ---------------------------------------------------------------
    disc_log = p.out / "disc-test.log"
    p.add(
        Gate(
            "tests-build",
            "tests",
            f"cargo test --workspace --profile {PROFILE} --no-fail-fast --no-run",
            cpu=a.cargo_jobs,
            locks=("cargo-rt",),
            scope=True,
            needs=("preflight",),
            always=True,
        )
    )
    disc_step = [s for s in workflow_steps(text, "disc-test") if "run-disc-tests" in s.run]
    if len(disc_step) != 1:
        raise SystemExit(f"{WORKFLOW}: disc-test job should have one run-disc-tests step")
    p.add(
        Gate(
            "tests-disc",
            "tests",
            disc_step[0].run,
            env={"LEGAIA_DISC_TEST_LOG": str(disc_log), "RUST_TEST_THREADS": str(a.jobs)},
            cpu=a.jobs,
            locks=("cargo-rt",),
            scope=True,
            needs=("tests-build",),
            read_log=lambda _: libtest_detail(disc_log.read_text(errors="replace")) if disc_log.exists() else (None, ""),
        )
    )
    if test_step is not None:
        p.add(
            Gate(
                "tests-no-disc",
                "tests",
                test_step.run,
                cwd=p.clean,
                env={"CARGO_TARGET_DIR": str(p.ci_root)},
                unset=no_data,
                cpu=a.cargo_jobs,
                scope=True,
                default=False,
                needs=tuple(after_select) or ("clean-tree",),
                read_log=libtest_detail,
                honest_skip=False,
            )
        )

    # -- ladder --------------------------------------------------------------
    ladder = p.bin / "ladder"
    shell_dir = root / "crates" / "engine-shell"
    cases = (
        ("ladder-b", "full_game_ladder:: --skip full_game_ladder::continuous::", "part_b_full_game_ladder"),
        ("ladder-c", "full_game_ladder::continuous::", "part_c_continuous_run"),
    )
    for name, args, test in cases:
        p.add(
            Gate(
                name,
                "ladder",
                f'"{ladder}" {args} --nocapture --test-threads 1',
                cwd=shell_dir,
                needs=("ladder-bin",),
                scope=True,
                read_log=reach_detail,
                # `cargo test --workspace` already ran this test; one run is the check.
                reuse=lambda test=test: reuse_from_disc_log(disc_log, test),
            )
        )
    try:
        seeds = tomllib.loads((root / SPINE_BASELINE).read_text()).get("continuous_seeds", {})
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise SystemExit(f"{SPINE_BASELINE}: {e}")
    for key in seeds:
        n = key.removeprefix("seed_")
        p.add(
            Gate(
                f"ladder-seed-{n}",
                "ladder",
                f'"{ladder}" full_game_ladder::continuous::part_c_continuous_run --exact --nocapture --test-threads 1',
                cwd=shell_dir,
                env={"LEGAIA_FGL_RNG_SEED": n},
                needs=("ladder-bin",),
                scope=True,
                read_log=reach_detail,
            )
        )

    # -- retail --------------------------------------------------------------
    engine_env = {"LEGAIA_ENGINE_BIN": str(p.bin / "legaia-engine")}
    drop = lambda name: p.out / "reports" / name  # noqa: E731
    p.add(
        Gate(
            "retail-compare",
            "retail",
            "python3 scripts/ci/retail-compare.py --images --check --no-build "
            f"--jobs {a.jobs} --out \"{drop('retail-compare')}\"",
            env=engine_env,
            cpu=a.jobs,
            locks=("display",),
            scope=True,
            needs=("engine-bin",),
            read_log=regressions,
        )
    )
    for mode in ("clean", "enhanced"):
        p.add(
            Gate(
                f"smoke-{mode}",
                "retail",
                f"python3 scripts/ci/retail-compare.py --smoke {mode} --no-build --jobs {a.jobs} "
                f"--smoke-reference \"{drop('retail-compare')}\" --out \"{drop(f'smoke-{mode}')}\"",
                env=engine_env,
                cpu=a.jobs,
                locks=("display",),
                scope=True,
                needs=("retail-compare",),
            )
        )
    p.add(
        Gate(
            "audio-sweep",
            "retail",
            f"python3 scripts/ci/audio-sweep.py --check --no-build --jobs {a.jobs} "
            f"--out \"{drop('audio-sweep')}/report.json\"",
            env=engine_env,
            cpu=a.jobs,
            scope=True,
            needs=("engine-bin",),
        )
    )
    p.add(
        Gate(
            "audio-pcm",
            "retail",
            audio_pcm_cmd(p),
            env=engine_env,
            needs=("engine-bin",),
            scope=True,
            read_log=tail_detail(r"^\[(?:skip|no baseline)\].*$"),
        )
    )
    p.add(
        Gate(
            "capture-coverage",
            "retail",
            f"python3 scripts/ci/capture-coverage.py --no-build --summary --jobs {a.jobs} --out \"{drop('capture-coverage')}\"",
            env=engine_env,
            cpu=a.jobs,
            scope=True,
            needs=("engine-bin",),
        )
    )

    # -- wasm ----------------------------------------------------------------
    p.add(
        Gate(
            "wasm-build",
            "wasm",
            "scripts/ci/build-wasm.sh",
            cpu=a.cargo_jobs,
            locks=("cargo-wasm",),
            scope=True,
            reuse=lambda: wasm_fresh(root),
        )
    )
    p.add(
        Gate(
            "wasm-freshness",
            "wasm",
            "python3 scripts/ci/check-wasm-freshness.py --strict",
            needs=("wasm-build",),
            read_log=lambda _: (None, bundle_size(root)),
        )
    )
    p.add(
        Gate(
            "play-disc",
            "wasm",
            "node scripts/ci/play-disc-check.mjs --disc",
            cpu=1,
        )
    )
    p.add(
        Gate(
            "pages",
            "wasm",
            f'node scripts/ci/check-pages.mjs --out "{drop("pages")}"',
            needs=("wasm-freshness",),
            locks=("display",),
            cpu=2,
            scope=True,
            read_log=tail_detail(r"^\[check-pages\] .*$"),
        )
    )
    p.add(
        Gate(
            "quality-ladder",
            "wasm",
            "node scripts/ci/quality-ladder-web.mjs",
            needs=("wasm-freshness",),
            locks=("display",),
            cpu=2,
            scope=True,
            read_log=tail_detail(r"^quality ladder: .*$"),
        )
    )
    p.add(
        Gate(
            "cost-ratchet",
            "wasm",
            "python3 scripts/ci/cost-ratchet.py --check",
            needs=("wasm-freshness",),
            # Its native half builds a bench-alloc engine and drives the window.
            locks=("display", "cargo-rt"),
            cpu=a.cargo_jobs,
            scope=True,
            read_log=tail_detail(r"^\[cost-ratchet\] .*$"),
        )
    )

    # -- last: the run moved nothing tracked ----------------------------------
    p.add(Gate("tree-unchanged", "tree", None, always=True))
    return p


def make_clean_tree(p: Plan, log) -> int:
    remove_clean_tree(p)
    p.clean.parent.mkdir(parents=True, exist_ok=True)
    return subprocess.run(
        ["git", "worktree", "add", "--detach", "--force", str(p.clean), "HEAD"],
        cwd=p.root,
        stdout=log,
        stderr=subprocess.STDOUT,
    ).returncode


def remove_clean_tree(p: Plan) -> None:
    subprocess.run(
        ["git", "worktree", "remove", "--force", str(p.clean)],
        cwd=p.root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if p.clean.exists():
        shutil.rmtree(p.clean, ignore_errors=True)
    subprocess.run(["git", "worktree", "prune"], cwd=p.root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def tracked_not_ignored(p: Plan, log) -> int:
    """A tracked site file a .gitignore rule covers would vanish on the next `git add`."""
    tracked = subprocess.run(["git", "ls-files", "site"], cwd=p.clean, capture_output=True, text=True, check=True).stdout
    r = subprocess.run(
        ["git", "check-ignore", "--no-index", "--stdin"], cwd=p.clean, input=tracked, capture_output=True, text=True
    )
    # check-ignore exits 1 for "nothing matched", which is the pass; 128 is an error.
    if r.returncode not in (0, 1):
        log.write(f"git check-ignore exited {r.returncode}: {r.stderr}\n".encode())
        return r.returncode
    if r.stdout.strip():
        log.write(f"tracked under site/ but ignored:\n{r.stdout}".encode())
        return 1
    log.write(f"{len(tracked.splitlines())} tracked site files, none ignored\n".encode())
    return 0


def preflight(p: Plan, log) -> int:
    """A missing input makes these gates skip and pass, so it is a failure here."""
    bad = [f"{rel}: missing under {p.root}" for rel in DATA_PATHS if not (p.root / rel).exists()]
    disc = os.environ.get("LEGAIA_DISC_BIN")
    if not disc or not Path(disc).is_file():
        bad.append(f"LEGAIA_DISC_BIN: {disc!r} is not a file")
    if not os.environ.get("DISPLAY"):
        bad.append("DISPLAY is unset: retail-compare --images would keep baselined image scores")
    for line in bad:
        log.write((line + "\n").encode())
    if bad:
        log.write(b"A worktree links the gitignored data from the main checkout: ln -s <main>/extracted extracted, and\n")
        log.write(b"likewise saves, captures and ghidra/scripts/funcs.\n")
    return 1 if bad else 0


def build_ladder_cmd(p: Plan) -> str:
    """Build the engine-shell test binary once, so the seeds skip cargo's lock. A
    copy, since a later cargo build relinks the original under a running seed."""
    return f"""\
mkdir -p "{p.bin}"
cargo test -p legaia-engine-shell --profile {PROFILE} --test integration --no-run \\
    --message-format=json-render-diagnostics > "{p.bin}/ladder-build.json"
exe=$(python3 -c '
import json, sys
for line in open(sys.argv[1]):
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("executable") and m.get("target", {{}}).get("name") == "integration":
        print(m["executable"])
' "{p.bin}/ladder-build.json")
test -n "$exe"
cp -f "$exe" "{p.bin}/ladder.tmp"
mv -f "{p.bin}/ladder.tmp" "{p.bin}/ladder"
echo "$exe -> {p.bin}/ladder"
"""


def reuse_from_disc_log(disc_log: Path, test: str) -> tuple[str, str] | None:
    if not disc_log.exists():
        return None
    text = disc_log.read_text(errors="replace")
    verdict = re.search(rf"^test \S*::{test} \.\.\. (\w+)$", text, re.M)
    if not verdict or verdict.group(1) != "ok":
        return None  # Not run there, or failed: measure it on its own.
    _, detail = reach_detail(text)
    return PASS, f"in tests-disc: {detail if test == 'part_c_continuous_run' else 'ok'}"


def audio_pcm_cmd(p: Plan) -> str:
    """The scorer writes beside the recordings, and `captures/` is shared between
    checkouts: score from a directory of links to them."""
    work = p.out / "reports" / "audio-pcm"
    return f"""\
rm -rf "{work}"
mkdir -p "{work}"
if [ -d captures/audio-pcm ]; then
    find "$(realpath captures/audio-pcm)" -maxdepth 1 \\( -name '*.wav' -o -name '*.mcr' \\) -exec ln -s -t "{work}" {{}} +
fi
python3 scripts/ci/audio-pcm.py --check --no-build --out "{work}"
"""


def wasm_fresh(root: Path) -> tuple[str, str] | None:
    """The stamp is content-addressed: a bundle it vouches for is this tree's build."""
    r = subprocess.run(
        ["python3", "scripts/ci/check-wasm-freshness.py", "--strict"], cwd=root, capture_output=True, text=True
    )
    return (PASS, "bundle already built from this tree") if r.returncode == 0 else None


def bundle_size(root: Path) -> str:
    import gzip

    parts = []
    for key, f in (
        ("play", root / "site" / "wasm" / "play" / "legaia_web_play_bundle_bg.wasm"),
        ("tool", root / "site" / "wasm" / "legaia_web_viewer_bg.wasm"),
    ):
        if f.exists():
            raw = f.read_bytes()
            parts.append(f"{key} {len(raw)} bytes, {len(gzip.compress(raw, 6))} gzip")
    return "; ".join(parts)


# -- Running ------------------------------------------------------------------


def wrap(p: Plan, argv: list[str], scope: bool) -> list[str]:
    if scope and p.a.memory_max != "off" and shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "-q", "-p", f"MemoryMax={p.a.memory_max}", *argv]
    return argv


def gate_env(p: Plan, g: Gate) -> dict[str, str]:
    env = dict(os.environ)
    # The drivers read `target/<profile>/`: another target dir would have them run
    # an older binary with a straight face.
    env.pop("CARGO_TARGET_DIR", None)
    env["CARGO_BUILD_JOBS"] = str(p.a.cargo_jobs)
    # What .cargo/config.toml sets for what cargo spawns; the binaries here are run by hand.
    env.setdefault("GLIBC_TUNABLES", "glibc.malloc.hugetlb=1")
    for k in g.unset:
        env.pop(k, None)
    if g.cwd == p.clean:
        env.update(p.ci_env)
    env.update(g.env)
    # The selector takes the cache root and names the subdirectory; a resumed
    # run's saved choice must not be handed back to it as the root.
    if g.cwd == p.clean and "CARGO_TARGET_DIR" in p.ci_env and "select-ci-target-dir" not in g.cmd:
        env["CARGO_TARGET_DIR"] = p.ci_env["CARGO_TARGET_DIR"]
    return env


class Running:
    def __init__(self, gate: Gate, proc, log, log_path: Path, gh_env: Path, started: float):
        self.gate, self.proc, self.log, self.log_path, self.gh_env, self.started = (
            gate, proc, log, log_path, gh_env, started,
        )


def start(p: Plan, g: Gate) -> Running:
    log_path = p.logs / f"{g.name}.log"
    log = open(log_path, "wb")
    gh_env = p.logs / f"{g.name}.github_env"
    gh_env.write_text("")
    env = gate_env(p, g)
    env["GITHUB_ENV"] = str(gh_env)
    env["GITHUB_STEP_SUMMARY"] = str(p.logs / f"{g.name}.summary")
    cwd = g.cwd or p.root
    script = p.logs / f"{g.name}.sh"
    script.write_text(g.cmd)
    argv = wrap(p, ["bash", "-e", str(script)], g.scope)
    log.write(f"$ (cd {cwd}) {g.cmd}\n".encode())
    log.flush()
    proc = subprocess.Popen(
        argv, cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True
    )
    return Running(g, proc, log, log_path, gh_env, time.time())


def finish(p: Plan, g: Gate, code: int, log_path: Path, wall: float, gh_env: Path | None) -> dict:
    if gh_env and gh_env.exists():
        for line in gh_env.read_text().splitlines():
            k, _, v = line.partition("=")
            if k:
                p.ci_env[k] = v
    text = log_path.read_text(errors="replace")
    status = PASS if code == 0 else FAIL
    detail = ""
    if code == 0 and g.honest_skip and SKIP_LINE.search(text):
        status = SKIP
        m = SKIP_LINE.search(text)
        detail = text[m.start() : text.find("\n", m.start())].strip()[:110]
    if g.read_log:
        try:
            over, d = g.read_log(text)
        except OSError as e:
            over, d = None, f"read_log: {e}"
        detail = d or detail
        if over and status == PASS:
            status = over
    if code != 0 and not detail:
        lines = [x for x in ANSI.sub("", text).splitlines()[1:] if x.strip()]
        said = [x for x in lines if FAILURE_LINE.search(x)]
        pick = said[-1] if said else lines[-1] if lines else ""
        detail = f"exit {code}: {pick.strip()[:110]}"
    return {"status": status, "code": code, "wall": round(wall, 1), "detail": detail, "log": str(log_path)}


def select(p: Plan) -> tuple[list[str], set[str]]:
    """The gates to run, in plan order; `--only` pulls in what they need."""
    a = p.a

    def hits(patterns: list[str]) -> set[str]:
        out: set[str] = set()
        for pat in patterns:
            found = {
                n for n, g in p.gates.items() if fnmatch.fnmatchcase(n, pat) or g.group == pat
            }
            if not found:
                raise SystemExit(f"no gate or group matches {pat!r}; --list shows them")
            out |= found
        return out

    chosen = hits(a.only) if a.only else {n for n, g in p.gates.items() if g.default}
    skipped = hits(a.skip) if a.skip else set()
    chosen -= skipped
    queue = list(chosen)
    while queue:
        for need in p.gates[queue.pop()].needs:
            if need not in chosen and need not in skipped:
                chosen.add(need)
                queue.append(need)
    if "tree-unchanged" in p.gates and "tree-unchanged" not in skipped:
        chosen.add("tree-unchanged")
    return [n for n in p.gates if n in chosen], skipped


def start_order(order: list[str]) -> list[str]:
    """`order` as the scheduler offers it: START_FIRST, the rest, then the seeds."""
    seed = lambda n: n.startswith("ladder-seed-")  # noqa: E731
    first = [n for n in START_FIRST if n in order]
    rest = [n for n in order if n not in first and not seed(n)]
    return first + rest + [n for n in order if seed(n)]


def children_cpu() -> float:
    """User + system seconds of every child reaped so far, descendants included."""
    use = resource.getrusage(resource.RUSAGE_CHILDREN)
    return use.ru_utime + use.ru_stime


def run_all(p: Plan, order: list[str], skipped: set[str], prior: dict) -> dict:
    a = p.a
    results: dict[str, dict] = {}
    pending = start_order(order)
    running: list[Running] = []
    held: set[str] = set()
    cpu = 0
    p.logs.mkdir(parents=True, exist_ok=True)
    key = tree_key(p.root)
    state_path = p.out / "state.json"
    t0 = time.time()

    def save() -> None:
        merged = {**prior, **results}
        state_path.write_text(json.dumps({"tree": key, "gates": merged, "ci_env": p.ci_env}, indent=1))

    def say(line: str) -> None:
        print(f"[{(time.time() - t0) / 60:6.1f}m] {line}", flush=True)

    def stop_all(*_):
        for r in running:
            try:
                os.killpg(r.proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        remove_clean_tree(p)
        save()
        raise SystemExit(130)

    signal.signal(signal.SIGINT, stop_all)
    signal.signal(signal.SIGTERM, stop_all)

    if a.resume:
        for n in list(pending):
            r = prior.get(n)
            if r and r["status"] in OK_STATUSES and not p.gates[n].always:
                pending.remove(n)
                results[n] = {**r, "resumed": True}
                results[n].pop("start", None)  # seconds into an earlier run
                say(f"keep   {n}: {r['status']} from the run being resumed")

    # `cargo test --workspace` runs the ladder's two default parts, so they wait for
    # the suite and read its log; on their own only if it did not get that far.
    def ready(n: str) -> bool:
        g = p.gates[n]
        for need in g.needs:
            if need in skipped or need not in order:
                continue
            st = results.get(need)
            if st is None or st["status"] in (FAIL, BLOCKED):
                return False
        if n in ("ladder-b", "ladder-c") and "tests-disc" in order and "tests-disc" not in results:
            return False
        if n == "tree-unchanged":
            return not running and len(pending) == 1
        return True

    def blocked(n: str) -> str | None:
        for need in p.gates[n].needs:
            st = results.get(need)
            if st and st["status"] in (FAIL, BLOCKED):
                return need
        return None

    try:
        while pending or running:
            # Anything whose prerequisite failed is reported now, not lost.
            for n in list(pending):
                need = blocked(n)
                if need:
                    pending.remove(n)
                    results[n] = {"status": BLOCKED, "code": None, "wall": 0, "detail": f"needs {need}", "log": ""}
                    say(f"BLOCK  {n}: needs {need}")
            for n in list(pending):
                g = p.gates[n]
                if set(g.locks) & held or not ready(n):
                    continue
                if running and cpu + g.cpu > a.max_cpu:
                    continue
                pending.remove(n)
                if g.reuse:
                    answer = g.reuse()
                    if answer:
                        results[n] = {"status": answer[0], "code": 0, "wall": 0, "detail": answer[1], "log": "", "reused": True}
                        say(f"reuse  {n}: {answer[1]}")
                        save()
                        continue
                if g.cmd is None:  # tree-unchanged
                    now = tree_key(p.root)
                    moved = git(p.root, "status", "--porcelain") if now != key else ""
                    results[n] = {
                        "status": PASS if now == key else FAIL,
                        "code": 0 if now == key else 1,
                        "wall": 0,
                        "detail": "" if now == key else "tracked files moved during the run: " + moved[:200],
                        "log": "",
                    }
                    say(f"{results[n]['status']:<6} {n}")
                    save()
                    continue
                if callable(g.cmd):
                    log_path = p.logs / f"{g.name}.log"
                    t = time.time()
                    say(f"start  {n}")
                    with open(log_path, "wb") as log:
                        code = g.cmd(log)
                    results[n] = finish(p, g, code, log_path, time.time() - t, None)
                    results[n]["start"] = round(t - t0, 1)
                    say(f"{results[n]['status']:<6} {n} {results[n]['wall']:.0f}s {results[n]['detail']}")
                    save()
                    continue
                say(f"start  {n}")
                running.append(start(p, g))
                cpu += g.cpu
                held |= set(g.locks)
            for r in list(running):
                # What a reap adds to the children's total is that gate's CPU, so read
                # it each time round: two gates ending together still come out apart.
                seen_cpu = children_cpu()
                code = r.proc.poll()
                if code is None and time.time() - r.started > r.gate.timeout:
                    try:
                        os.killpg(r.proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    r.log.write(f"\n[full-gate] killed after {r.gate.timeout}s\n".encode())
                    code = r.proc.wait()
                if code is None:
                    continue
                r.log.close()
                running.remove(r)
                cpu -= r.gate.cpu
                held -= set(r.gate.locks)
                results[r.gate.name] = finish(p, r.gate, code, r.log_path, time.time() - r.started, r.gh_env)
                results[r.gate.name]["start"] = round(r.started - t0, 1)
                results[r.gate.name]["cpu"] = round(children_cpu() - seen_cpu, 1)
                res = results[r.gate.name]
                say(f"{res['status']:<6} {r.gate.name} {res['wall']:.0f}s {res['detail']}")
                save()
            if running or pending:
                time.sleep(0.2)
    finally:
        if not a.keep_clean_tree:
            remove_clean_tree(p)
    results["__wall__"] = {"wall": time.time() - t0}
    save()
    return results


def report(p: Plan, order: list[str], skipped: set[str], results: dict) -> int:
    wall = results.pop("__wall__", {}).get("wall", 0)
    rows = [(n, results.get(n)) for n in order]
    bad = [(n, r) for n, r in rows if r and r["status"] in (FAIL, BLOCKED)]
    skips = [(n, r) for n, r in rows if r and r["status"] == SKIP]
    warns = [(n, r) for n, r in rows if r and r["status"] == WARN]
    serial = sum((r or {}).get("wall", 0) for _, r in rows)
    cpu_total = sum((r or {}).get("cpu", 0) for _, r in rows)

    def table(rows2) -> list[str]:
        lines = ["| Gate | Group | Status | Start | Seconds | CPU s | Detail |", "|---|---|---|---:|---:|---:|---|"]
        for n, r in rows2:
            r = r or {"status": UNRUN, "wall": 0, "detail": ""}
            detail = str(r.get("detail", "")).replace("|", "\\|")
            # Seconds into this run; a gate this run did not start (blocked, reused) has none.
            start = f"{r['start']:.0f}" if "start" in r else ""
            cpu = f"{r['cpu']:.0f}" if "cpu" in r else ""
            lines.append(f"| {n} | {p.gates[n].group} | {r['status']} | {start} | {r.get('wall', 0):.0f} | {cpu} | {detail} |")
        return lines

    head = git(p.root, "rev-parse", "--short=12", "HEAD").strip()
    verdict = "FAIL" if bad else ("PARTIAL" if skips or p.a.only or skipped else "PASS")
    out = [
        f"# Full gate at {head}: {verdict}",
        "",
        f"Wall clock {wall / 60:.1f} min; the same gates one after another {serial / 60:.1f} min; "
        f"{cpu_total / 60:.1f} CPU min.",
        "",
        *table(rows),
    ]
    if skipped:
        out += ["", "Left out by --only/--skip: " + ", ".join(sorted(skipped))]
    (p.out / "timing.md").write_text("\n".join(out) + "\n")
    print()
    print("\n".join(out))
    print()
    for label, group in (("FAILED", bad), ("SKIPPED (a skip is not a pass)", skips), ("TO REVIEW", warns)):
        if group:
            print(f"{label}:")
            for n, r in group:
                print(f"  {n}: {r['detail']}" + (f"\n    log: {r['log']}" if r.get("log") else ""))
    print(f"\ntiming table: {p.out / 'timing.md'}")
    return 1 if bad else 0


def list_gates(p: Plan, order: list[str]) -> int:
    for n in p.gates:
        g = p.gates[n]
        cmd = (
            "(in-process)"
            if not isinstance(g.cmd, str)
            else "; ".join(x for x in g.cmd.strip().splitlines() if x.strip())[:110]
        )
        flags = "" if n in order else " [not selected]"
        where = "clean" if g.cwd == p.clean else "tree"
        print(f"{n:<44} {g.group:<7} {where:<5} cpu={g.cpu:<2}{flags}  {cmd}")
    return 0


# -- Self-test ----------------------------------------------------------------

FIXTURE = """\
name: t
env:
  CARGO_TARGET_DIR: /somewhere
  OTHER: x   # note
jobs:
  gates:
    runs-on: self-hosted
    steps:
      - name: Checkout
        uses: actions/checkout@abc
      # a comment
      - name: One
        run: python3 one.py

      - name: Two lines (advisory)
        env:
          BASE: ${{ github.sha }}
          # a comment
          PLAIN: "v"
        run: |
          a

          # kept
          b || true
      - run: tail
  other:
    steps:
      - name: Not this
        run: nope
"""


def selftest() -> int:
    errors: list[str] = []

    def check(cond, what: str) -> None:
        if not cond:
            errors.append(what)

    steps = workflow_steps(FIXTURE, "gates")
    check([s.name for s in steps] == ["One", "Two lines (advisory)", "tail"], f"step names: {[s.name for s in steps]}")
    check(steps[0].run == "python3 one.py", "one-line run")
    check(steps[1].run == "a\n\n# kept\nb || true\n", f"block run: {steps[1].run!r}")
    check(steps[1].env == {"BASE": "${{ github.sha }}", "PLAIN": "v"}, f"step env: {steps[1].env}")
    check(workflow_env(FIXTURE) == {"CARGO_TARGET_DIR": "/somewhere", "OTHER": "x"}, "top env")
    check(slug("Two lines (advisory)") == "two-lines-advisory", "slug")

    # The real file: this parser against a real YAML reader where there is one.
    real = (REPO / WORKFLOW).read_text()
    gates = workflow_steps(real, "gates")
    check(len(gates) >= 20, f"Repo Gates parsed to {len(gates)} steps")
    check(len({slug(s.name) for s in gates}) == len(gates), "two Repo Gates steps share a slug")
    check(any(s.run.startswith("cargo clippy") for s in workflow_steps(real, "pr-lint")), "no clippy in PR Lint")
    for s in gates:
        check(not FORBIDDEN.search(s.run), f"Repo Gates step {s.name!r} carries a forbidden command")
    try:
        import yaml  # type: ignore
    except ImportError:
        yaml = None
    if yaml:
        doc = yaml.safe_load(real)
        for job in ("gates", "pr-lint", "disc-test"):
            want = [x for x in doc["jobs"][job]["steps"] if "run" in x]
            got = workflow_steps(real, job)
            check([x["name"] for x in want] == [s.name for s in got], f"{job}: names differ from PyYAML")
            for x, s in zip(want, got):
                check(x["run"].strip() == s.run.strip(), f"{job}/{s.name}: run differs from PyYAML")

    # The real plan builds: every name unique, nothing forbidden, the seeds found.
    with tempfile.TemporaryDirectory() as td:
        a = argparse.Namespace(
            out=Path(td), max_cpu=12, jobs=8, cargo_jobs=8, memory_max="off", only=None, skip=None,
            resume=False, keep_clean_tree=False, base="main",
        )
        plan = build_plan(a, REPO)
        for must in ("tests-disc", "ladder-b", "ladder-c", "ladder-seed-3", "retail-compare", "pages", "lint-clippy"):
            check(must in plan.gates, f"the plan has no {must}")

    # Start order: the same gates, the chains first, the seeds last.
    names = ["lint-clippy", "ladder-seed-3", "other", "retail-compare", "engine-bin"]
    got = start_order(names)
    check(sorted(got) == sorted(names), f"start order lost a gate: {got}")
    check(got == ["engine-bin", "lint-clippy", "retail-compare", "other", "ladder-seed-3"], f"start order {got}")

    # The scheduler on fakes: every failure reported, nothing after a failed need,
    # a skip marked, a forbidden command refused, locks and budget respected.
    with tempfile.TemporaryDirectory() as td:
        root = Path(td) / "r"
        root.mkdir()
        for cmd in (["init", "-q"], ["config", "user.email", "t@t"], ["config", "user.name", "t"]):
            subprocess.run(["git", *cmd], cwd=root, check=True)
        (root / "f").write_text("x\n")
        subprocess.run(["git", "add", "f"], cwd=root, check=True)
        subprocess.run(["git", "commit", "-qm", "f"], cwd=root, check=True)
        a = argparse.Namespace(
            out=Path(td) / "o", max_cpu=2, jobs=1, cargo_jobs=1, memory_max="off", only=None, skip=None,
            resume=False, keep_clean_tree=False, base="main",
        )
        p = Plan(a, root)
        stamp = Path(td) / "stamp"
        p.add(Gate("ok", "g", "echo fine"))
        p.add(Gate("bad-one", "g", "echo oops; exit 3"))
        p.add(Gate("bad-two", "g", "false; echo unreachable"))
        p.add(Gate("after-bad", "g", "echo no", needs=("bad-one",)))
        p.add(Gate("skips", "g", "echo '[skip] no data'"))
        p.add(Gate("skip-ok", "g", "echo '[skip] no data'", honest_skip=False))
        p.add(Gate("warns", "g", "echo x", read_log=lambda t: (WARN, "look")))
        p.add(Gate("lock-a", "g", f"test ! -e {stamp}; touch {stamp}; sleep 0.3; rm {stamp}", locks=("l",)))
        p.add(Gate("lock-b", "g", f"test ! -e {stamp}; touch {stamp}; sleep 0.3; rm {stamp}", locks=("l",)))
        p.add(Gate("reused", "g", "exit 9", reuse=lambda: (PASS, "already")))
        p.add(Gate("dirties", "g", "echo y >> f"))
        p.add(Gate("tree-unchanged", "g", None, always=True))
        order, skipped = select(p)
        real_stdout = sys.stdout
        try:
            sys.stdout = open(os.devnull, "w")
            res = run_all(p, order, skipped, {})
            code = report(p, order, skipped, dict(res))
        finally:
            sys.stdout.close()
            sys.stdout = real_stdout
        got = {n: res[n]["status"] for n in order}
        want = {
            "ok": PASS, "bad-one": FAIL, "bad-two": FAIL, "after-bad": BLOCKED, "skips": SKIP, "skip-ok": PASS,
            "warns": WARN, "lock-a": PASS, "lock-b": PASS, "reused": PASS, "dirties": PASS, "tree-unchanged": FAIL,
        }
        check(got == want, f"scheduler outcome {got}")
        check(code == 1, "a failed gate must exit 1")
        table = (a.out / "timing.md").read_text()
        check("bad-one" in table and "bad-two" in table and "after-bad" in table, "timing table lost a failure")
        check(re.search(r"^\| bad-one \| g \| FAIL \| \d+ \| \d+ \| \d+ \|", table, re.M), "timing table lost a gate's start")
        check(re.search(r"^\| reused \| g \| PASS \|  \| 0 \|  \|", table, re.M), "a reused gate has no start in this run")
        for line in ("--bless", "python3 x.py --update-baseline", "git push origin", "x --propose p"):
            try:
                p.add(Gate("nope-" + line, "g", line))
                errors.append(f"{line!r} was not refused")
            except SystemExit:
                pass

    for e in errors:
        print(f"full-gate selftest: {e}", file=sys.stderr)
    print("full-gate selftest:", "FAILED" if errors else "ok")
    return 1 if errors else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--list", action="store_true", help="print the gates and exit")
    ap.add_argument("--selftest", action="store_true", help="the parser and scheduler on fakes, plus the real workflow file")
    ap.add_argument("--only", type=lambda s: s.split(","), help="comma list: gate names, globs or groups (their needs come along)")
    ap.add_argument("--skip", type=lambda s: s.split(","), help="comma list: gate names, globs or groups")
    ap.add_argument("--resume", action="store_true", help="keep what passed in --out's last run, if the tree is the same")
    ap.add_argument("--out", type=Path, default=REPO / "target" / "full-gate")
    ap.add_argument("--base", default="main", help="the revision baselines and revert-scan compare against (default %(default)s)")
    ap.add_argument("--max-cpu", type=int, default=12, help="the budget gates are weighed against (default %(default)s)")
    ap.add_argument("--jobs", type=int, default=6, help="retail-compare, audio sweep, libtest threads (default %(default)s)")
    ap.add_argument("--cargo-jobs", type=int, default=6, help="CARGO_BUILD_JOBS (default %(default)s)")
    ap.add_argument("--memory-max", default="24G", help="systemd scope cap per engine-looping gate, or 'off' (default %(default)s)")
    ap.add_argument("--keep-clean-tree", action="store_true", help="leave the clean worktree to inspect a repo gate")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    a.out = a.out.resolve()
    p = build_plan(a, REPO)
    order, skipped = select(p)
    if a.list:
        return list_gates(p, order)
    prior: dict = {}
    if a.resume:
        state = p.out / "state.json"
        if not state.exists():
            raise SystemExit(f"--resume: nothing to resume at {state}")
        saved = json.loads(state.read_text())
        if saved.get("tree") != tree_key(REPO):
            raise SystemExit("--resume: the tree changed since that run; start a new one")
        prior = {k: v for k, v in saved["gates"].items() if k != "__wall__"}
        p.ci_env.update(saved.get("ci_env", {}))
    p.out.mkdir(parents=True, exist_ok=True)
    if git(REPO, "status", "--porcelain").strip():
        print("note: uncommitted changes. The repo group measures HEAD; the rest measures the tree.", flush=True)
    results = run_all(p, order, skipped, prior)
    return report(p, order, skipped, results)


if __name__ == "__main__":
    sys.exit(main())
