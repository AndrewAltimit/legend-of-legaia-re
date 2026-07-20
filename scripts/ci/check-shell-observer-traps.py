#!/usr/bin/env python3
"""Detect "the observer is inside the thing it observes" defects in shell scripts.

Three defect shapes, each of which has silently produced a false result in this
repo's own workflow at least once:

  PIPE_STATUS   `cmd | tail && echo OK` reports the LAST pipe stage's exit
                status, not `cmd`'s. A failing build behind a `| tail` reads as
                a pass. Needs `${PIPESTATUS[0]}`, `set -o pipefail`, or the
                redirect-to-file-then-inspect form.

  SELF_MATCH    `pkill -f <pattern>` / `pgrep -f <pattern>` match the CALLER's
                own command line, because the pattern is a literal substring of
                that command line. `until ! pgrep -f "cargo test"` can never
                exit; `pkill -f cargo` kills the shell that ran it. Needs a PID,
                a process group (`pgrep -g`), or `scripts/lib/proc.sh`.

  GREP_RC       `grep` exits 1 when it matches nothing. Under `set -e`, or when
                the status is read as "the check failed", a *clean* result is
                misread as a failure. Needs `|| true`, an `if`-guard, or the
                `grep -c`/count comparison form.

Usage:
    python3 scripts/ci/check-shell-observer-traps.py            # audit scripts/
    python3 scripts/ci/check-shell-observer-traps.py --selftest # positive control
    python3 scripts/ci/check-shell-observer-traps.py PATH...

A finding can be waived in-place with a trailing `# observer-trap-ok: <reason>`
comment on the offending line. The reason is mandatory -- a bare waiver is
itself reported, because "someone decided this was fine" without saying why is
how the simpler-looking wrong form gets restored later.

Exit status: 0 = clean, 1 = findings, 2 = self-test failed.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# Directories the audit walks by default.
DEFAULT_ROOTS = ["scripts"]

WAIVER_RE = re.compile(r"#\s*observer-trap-ok\s*:\s*(?P<reason>.*)$")

# ---------------------------------------------------------------------------
# Shape 1: pipe exit status
# ---------------------------------------------------------------------------

# Commands whose exit status is essentially always 0 and therefore destroys the
# status of whatever fed them. `grep` is deliberately NOT in this list: grep's
# status is meaningful, and a `cmd | grep -q x && ...` is usually intentional.
STATUS_SWALLOWING = r"(?:tail|head|tee|cat|sort|uniq|wc|awk|sed|tr|column|jq|less|fold|nl|xargs)"

# `cmd | tail ... && something` / `|| something` -- the && binds to the tail.
PIPE_THEN_LOGICAL_RE = re.compile(
    r"\|\s*" + STATUS_SWALLOWING + r"\b[^|&]*(?:&&|\|\|)"
)

# `if cmd | tail; then` / `if ! cmd | wc -l; then` -- the condition is the tail's.
PIPE_IN_CONDITION_RE = re.compile(
    r"\b(?:if|while|until)\s+!?\s*[^;|]*\|\s*" + STATUS_SWALLOWING + r"\b"
)

# ---------------------------------------------------------------------------
# Shape 2: self-matching process predicates
# ---------------------------------------------------------------------------

# pkill/pgrep with -f (full-command-line match). The pattern appears verbatim in
# the caller's own /proc/self/cmdline, so the predicate matches itself.
SELF_MATCH_RE = re.compile(r"\b(?:pkill|pgrep)\b[^\n]*?(?:^|\s)-\w*f")

# killall matches by process NAME, so `killall bash` from a bash script is the
# same defect in a different wrapper.
KILLALL_RE = re.compile(r"\bkillall\b")

# ---------------------------------------------------------------------------
# Shape 3: grep's empty-match exit code
# ---------------------------------------------------------------------------

# A bare `grep ...` as a whole statement, under `set -e`, aborts the script when
# it matches nothing. Only flagged in files that actually enable `set -e`.
BARE_GREP_RE = re.compile(r"^\s*(?:sudo\s+)?z?e?f?grep\b")

# Guarded forms: these already handle the empty-match case.
GREP_GUARDED_RE = re.compile(r"(\|\|\s*true|\|\|\s*:|\|\|\s*\w|^\s*(?:if|while|until|elif)\b|&&|\btest\b)")

SET_E_RE = re.compile(r"^\s*set\s+-\w*e|^\s*set\s+-o\s+errexit")
PIPEFAIL_RE = re.compile(r"^\s*set\s+-\w*o?\s*\w*pipefail|^\s*set\s+-o\s+pipefail|^\s*set\s+-\w*euo?\s+pipefail")


@dataclass
class Finding:
    path: str
    line_no: int
    shape: str
    line: str
    detail: str

    def render(self) -> str:
        return (
            f"{self.path}:{self.line_no}: [{self.shape}] {self.detail}\n"
            f"    {self.line.strip()}"
        )


def strip_comment(line: str) -> str:
    """Remove a trailing shell comment, respecting single/double quotes."""
    out = []
    quote = None
    prev = ""
    for ch in line:
        if quote:
            if ch == quote and prev != "\\":
                quote = None
            out.append(ch)
        elif ch in "'\"":
            quote = ch
            out.append(ch)
        elif ch == "#" and (not out or out[-1].isspace()):
            break
        else:
            out.append(ch)
        prev = ch
    return "".join(out)


def scan_text(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    lines = text.splitlines()

    has_set_e = any(SET_E_RE.search(ln) for ln in lines)
    has_pipefail = any("pipefail" in ln and ln.lstrip().startswith("set ") for ln in lines)

    for i, raw in enumerate(lines, start=1):
        waiver = WAIVER_RE.search(raw)
        if waiver:
            if not waiver.group("reason").strip():
                findings.append(
                    Finding(path, i, "BARE_WAIVER", raw,
                            "observer-trap-ok waiver with no reason given")
                )
            continue

        code = strip_comment(raw)
        if not code.strip():
            continue

        # -- shape 1 ------------------------------------------------------
        if not has_pipefail:
            if PIPE_THEN_LOGICAL_RE.search(code):
                findings.append(Finding(
                    path, i, "PIPE_STATUS", raw,
                    "&&/|| binds to the status-swallowing pipe stage, not the "
                    "producer; use ${PIPESTATUS[0]} or `set -o pipefail`",
                ))
            elif PIPE_IN_CONDITION_RE.search(code):
                findings.append(Finding(
                    path, i, "PIPE_STATUS", raw,
                    "condition tests the last pipe stage's status, not the "
                    "producer's; use ${PIPESTATUS[0]} or `set -o pipefail`",
                ))

        # -- shape 2 ------------------------------------------------------
        if SELF_MATCH_RE.search(code):
            findings.append(Finding(
                path, i, "SELF_MATCH", raw,
                "pkill/pgrep -f matches the caller's own command line; use a "
                "PID, `pgrep -g <pgid>`, or scripts/lib/proc.sh",
            ))
        if KILLALL_RE.search(code):
            findings.append(Finding(
                path, i, "SELF_MATCH", raw,
                "killall matches by process name and can match the caller; use "
                "a PID or scripts/lib/proc.sh",
            ))

        # -- shape 3 ------------------------------------------------------
        if has_set_e and BARE_GREP_RE.search(code) and not GREP_GUARDED_RE.search(code):
            findings.append(Finding(
                path, i, "GREP_RC", raw,
                "unguarded grep under `set -e`: exit 1 on NO match aborts the "
                "script, and a no-match is usually the clean result; add "
                "`|| true` or an if-guard",
            ))

    return findings


# ---------------------------------------------------------------------------
# Positive control
# ---------------------------------------------------------------------------

SELFTEST_CASES: list[tuple[str, str, str]] = [
    (
        "pipe-then-and",
        "#!/bin/bash\nset -eu\ncargo test | tail -20 && echo OK\n",
        "PIPE_STATUS",
    ),
    (
        "pipe-in-if",
        "#!/bin/bash\nset -eu\nif cargo build | tail -1; then echo fine; fi\n",
        "PIPE_STATUS",
    ),
    (
        "pkill-dash-f",
        "#!/bin/bash\nset -eu\npkill -f 'cargo test -p legaia'\n",
        "SELF_MATCH",
    ),
    (
        "pgrep-waiter",
        "#!/bin/bash\nset -eu\nuntil ! pgrep -f 'cargo test'; do sleep 1; done\n",
        "SELF_MATCH",
    ),
    (
        "killall",
        "#!/bin/bash\nset -eu\nkillall mednafen\n",
        "SELF_MATCH",
    ),
    (
        "bare-grep-under-set-e",
        "#!/bin/bash\nset -euo pipefail\ngrep FAILED build.log\necho done\n",
        "GREP_RC",
    ),
    (
        "bare-waiver",
        "#!/bin/bash\nset -eu\npkill -f cargo  # observer-trap-ok:\n",
        "BARE_WAIVER",
    ),
]

# Snippets that must NOT be flagged -- the negative half of the control. A
# detector that flags everything is as useless as one that flags nothing.
SELFTEST_CLEAN: list[tuple[str, str]] = [
    ("pipefail-set", "#!/bin/bash\nset -euo pipefail\ncargo test | tail -20 && echo OK\n"),
    ("pipestatus-checked",
     "#!/bin/bash\nset -eu\ncargo test > log.txt\nrc=${PIPESTATUS[0]}\n"),
    ("pgrep-by-group", "#!/bin/bash\nset -eu\npgrep -g \"$PGID\" >/dev/null\n"),
    ("kill-by-pid", "#!/bin/bash\nset -eu\nkill -TERM \"$PID\"\n"),
    ("grep-guarded", "#!/bin/bash\nset -eu\ngrep FAILED build.log || true\n"),
    ("grep-in-if", "#!/bin/bash\nset -eu\nif grep -q FAILED build.log; then exit 1; fi\n"),
    ("waiver-with-reason",
     "#!/bin/bash\nset -eu\npkill -f xyz  # observer-trap-ok: pattern cannot appear in our argv\n"),
    ("awk-and-inside-program",
     "#!/bin/bash\nset -euo pipefail\ncat f | awk 'NR>1 && $7 >= 3 { print }'\n"),
]


def run_selftest() -> int:
    failures = 0
    for name, snippet, expect_shape in SELFTEST_CASES:
        got = scan_text(f"<selftest:{name}>", snippet)
        shapes = {f.shape for f in got}
        if expect_shape in shapes:
            print(f"  ok    {name}: flagged {expect_shape}")
        else:
            print(f"  FAIL  {name}: expected {expect_shape}, got {sorted(shapes) or 'nothing'}")
            failures += 1

    for name, snippet in SELFTEST_CLEAN:
        got = scan_text(f"<selftest:{name}>", snippet)
        if got:
            print(f"  FAIL  {name}: expected clean, got {[f.shape for f in got]}")
            failures += 1
        else:
            print(f"  ok    {name}: clean")

    if failures:
        print(f"\nself-test: {failures} case(s) failed -- the detector is not "
              f"trustworthy, so a clean audit from it means nothing")
        return 2
    print(f"\nself-test: all {len(SELFTEST_CASES) + len(SELFTEST_CLEAN)} cases pass")
    return 0


def iter_shell_files(roots: list[Path]) -> list[Path]:
    out: list[Path] = []
    for root in roots:
        if root.is_file():
            out.append(root)
            continue
        for p in sorted(root.rglob("*")):
            if not p.is_file():
                continue
            if p.suffix in {".sh", ".bash"}:
                out.append(p)
            elif p.suffix == "" and p.is_file():
                try:
                    first = p.read_text(errors="replace").splitlines()[:1]
                except (OSError, IndexError):
                    continue
                if first and first[0].startswith("#!") and "sh" in first[0]:
                    out.append(p)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="*", help="files or dirs to scan (default: scripts/)")
    ap.add_argument("--selftest", action="store_true",
                    help="run the positive/negative control suite and exit")
    ap.add_argument("--quiet", action="store_true", help="only print findings")
    args = ap.parse_args()

    if args.selftest:
        print("check-shell-observer-traps self-test")
        return run_selftest()

    roots = [Path(p) for p in args.paths] if args.paths else [REPO_ROOT / r for r in DEFAULT_ROOTS]
    files = iter_shell_files(roots)

    # The audit is only meaningful if the detector demonstrably fires. Run the
    # control every time, not just under --selftest: a sweep that reports
    # "clean" from a probe that never matches is the exact failure this file
    # exists to prevent.
    for _name, snippet, expect in SELFTEST_CASES:
        if expect not in {f.shape for f in scan_text("<control>", snippet)}:
            print("ERROR: built-in positive control failed; audit result is not "
                  "trustworthy. Run --selftest.", file=sys.stderr)
            return 2

    findings: list[Finding] = []
    for f in files:
        try:
            findings.extend(scan_text(str(f.relative_to(REPO_ROOT)), f.read_text(errors="replace")))
        except ValueError:
            findings.extend(scan_text(str(f), f.read_text(errors="replace")))

    if not args.quiet:
        print(f"scanned {len(files)} shell file(s) (positive control: passed)")

    if findings:
        for fi in findings:
            print(fi.render())
        print(f"\n{len(findings)} finding(s)")
        return 1

    if not args.quiet:
        print("no observer-trap findings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
