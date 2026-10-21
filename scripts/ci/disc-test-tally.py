#!/usr/bin/env python3
"""Count what a disc-gated test run actually ran.

A disc-gated test that finds its input missing prints a skip line and
returns, and libtest reports it `ok`. So a green run says nothing about how
much of the suite executed: every gated test passes on a machine with no disc,
and a test whose second input is gone (`extracted/`, the save library, a
memory-card file) passes on a machine that has the disc.

This reads the output of `cargo test ... -- --show-output`, finds each test
whose captured output carries a skip line, and reports

- how many tests ran, failed and skipped,
- the skips that blame `LEGAIA_DISC_BIN` alone, which `--require-disc` turns
  into a failure: with the disc present such a line means a broken gate,
- the skips that blame another input, grouped by the line they printed, which
  `--max-other-skips N` bounds.

    cargo test --workspace -- --show-output > run.log 2>&1
    disc-test-tally.py run.log --require-disc
    disc-test-tally.py run.log --summary "$GITHUB_STEP_SUMMARY"
    disc-test-tally.py --self-test

A test that returns early without printing anything is invisible here; the
skip line is the convention this depends on.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

BLOCK = re.compile(r"^---- (\S+) (?:stdout|stderr) ----$")
# The shapes a skip line takes in this workspace: a `[skip]` prefix (nearly all
# of them), a leading `skip:` / `skipping`, a trailing `- skipping`, and a
# closing `(skip)`. The engine's own `... load skipped: <error>` log lines are
# not test skips and match none of these.
SKIP = re.compile(
    r"^\s*\[skip(-case)?\]|^\s*skip(ping|ped)?[: ]|[-;,:] ?skipping\b|\(skip\)\s*$",
    re.IGNORECASE,
)
RESULT = re.compile(
    r"^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out"
)
# The disc variable itself, not a longer name it prefixes (`LEGAIA_DISC_BIN_JP`).
DISC_VAR = re.compile(r"LEGAIA_DISC_BIN(?!\w)")
# Inputs a skip line can name besides the disc - another disc's variable
# (`LEGAIA_DISC_BIN_JP`, `LEGAIA_FANPATCH_BASE_BIN`) among them.
OTHER_INPUT = re.compile(
    r"extracted|PROT|save|\.mc|mednafen|sstate|library|card|corpus|dump|funcs|pcsx|recomp|PAL|lang|LEGAIA_\w+",
    re.IGNORECASE,
)


def tally(lines: list[str]) -> dict:
    passed = failed = ignored = 0
    skips: dict[str, str] = {}
    current: str | None = None
    for line in lines:
        line = line.rstrip("\n")
        head = BLOCK.match(line)
        if head:
            current = head.group(1)
            continue
        result = RESULT.match(line)
        if result:
            passed += int(result.group(1))
            failed += int(result.group(2))
            ignored += int(result.group(3))
            current = None
            continue
        if line.startswith(("successes:", "failures:", "running ")) and not line.startswith("    "):
            current = None
            continue
        if current and current not in skips and SKIP.search(line):
            skips[current] = line.strip()
    disc_only = {
        name: why
        for name, why in skips.items()
        if DISC_VAR.search(why) and not OTHER_INPUT.search(DISC_VAR.sub("", why))
    }
    other = {name: why for name, why in skips.items() if name not in disc_only}
    return {
        "passed": passed,
        "failed": failed,
        "ignored": ignored,
        "disc_only": disc_only,
        "other": other,
    }


def report(t: dict) -> str:
    skipped = len(t["disc_only"]) + len(t["other"])
    out = [
        f"tests passed {t['passed']}, failed {t['failed']}, ignored {t['ignored']}",
        f"of the passes, {skipped} printed a skip line "
        f"({len(t['disc_only'])} blaming the disc alone, {len(t['other'])} another input)",
    ]
    reasons = Counter(re.sub(r"\s+", " ", why) for why in t["other"].values())
    for why, n in reasons.most_common(15):
        out.append(f"  {n:5d}  {why[:140]}")
    if len(reasons) > 15:
        out.append(f"  ... {len(reasons) - 15} more reasons")
    for name, why in sorted(t["disc_only"].items())[:20]:
        out.append(f"  disc-only skip: {name}: {why[:120]}")
    return "\n".join(out)


def self_test() -> int:
    log = """running 5 tests
test a::ran ... ok
test a::no_disc ... ok
test a::no_extract ... ok
test a::no_jp ... ok
test a::quiet ... ok

successes:

---- a::no_disc stdout ----
[skip] LEGAIA_DISC_BIN unset

---- a::no_extract stdout ----
[skip] extracted/PROT.DAT or LEGAIA_DISC_BIN missing

---- a::no_jp stdout ----
[skip] LEGAIA_DISC_BIN_JP unset

---- a::ran stdout ----
checked 12 entries, none skipped a beat

successes:
    a::ran

test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s
""".splitlines()
    t = tally(log)
    want = (5, 0, 1, ["a::no_disc"], ["a::no_extract", "a::no_jp"])
    got = (t["passed"], t["failed"], t["ignored"], sorted(t["disc_only"]), sorted(t["other"]))
    if got != want:
        print(f"self-test: FAILED\n  got  {got}\n  want {want}")
        return 1
    print("self-test: ok")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("log", nargs="?", help="output of `cargo test -- --show-output` (default: stdin)")
    ap.add_argument("--require-disc", action="store_true", help="fail on a skip that blames the disc alone")
    ap.add_argument("--max-other-skips", type=int, help="fail when more tests than this skip on another input")
    ap.add_argument("--summary", help="append the report to this file as a fenced block")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    text = Path(args.log).read_text(errors="replace") if args.log else sys.stdin.read()
    t = tally(text.splitlines())
    body = report(t)
    print(body)
    if args.summary:
        with open(args.summary, "a") as f:
            f.write(f"### Disc-gated test tally\n\n```\n{body}\n```\n")

    status = 0
    if t["passed"] + t["failed"] == 0:
        print("disc-test tally: no `test result:` line in the log - nothing ran", file=sys.stderr)
        status = 1
    if args.require_disc and t["disc_only"]:
        print(
            f"disc-test tally: {len(t['disc_only'])} test(s) skipped for want of the disc on a run that requires it",
            file=sys.stderr,
        )
        status = 1
    if args.max_other_skips is not None and len(t["other"]) > args.max_other_skips:
        print(
            f"disc-test tally: {len(t['other'])} test(s) skipped on another input, over the bound of {args.max_other_skips}",
            file=sys.stderr,
        )
        status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
