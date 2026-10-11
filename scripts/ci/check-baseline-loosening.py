#!/usr/bin/env python3
"""A ratchet baseline may not loosen without a ledger entry that says why.

Every ratchet in this repository compares a measurement against a committed
baseline file, and every one of them accepts a new baseline in the same commit
as the code that moved it. Most of those ratchets read inputs CI does not have
(the disc, the dump corpus, the save library), so on a runner they skip. What
is left is the baseline file itself, which is plain text and needs nothing: a
value in it that got worse is visible in the diff alone.

This gate reads each baseline at a base revision and in the working tree,
walks the numeric leaves, and fails on any that moved the wrong way - a score
that fell, a worklist that grew, an entry added to a known-bad set, a
baselined key that disappeared - unless `scripts/ci/baseline-loosenings.toml`
records that exact transition with a reason. The ledger is the record of
every time a ratchet was let out, which a squash-merged history otherwise
buries in a wave commit.

    check-baseline-loosening.py                 # working tree vs merge-base with main
    check-baseline-loosening.py --base REV      # working tree vs REV
    check-baseline-loosening.py --propose       # print ledger entries for what loosened
    check-baseline-loosening.py --self-test

Needs no disc and no build. Exits 1 on an unrecorded loosening.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
LEDGER = Path(__file__).resolve().parent / "baseline-loosenings.toml"

# Which way each baselined leaf is allowed to move. First matching pattern
# wins; a numeric leaf no pattern covers is a finding, so a new baseline key
# cannot arrive unclassified.
#
#   up    higher is better: a fall is a loosening
#   down  lower is better: a rise is a loosening
#   pin   a setting of the measurement: any change is a loosening
#   set   a list of known-bad entries: an addition is a loosening
#   skip  not a ratcheted value
RULES: dict[str, list[tuple[str, str]]] = {
    "scripts/ci/port-catalog-baseline.json": [
        ("totals/*", "up"),
        ("worklist/*", "down"),
        ("live/*", "down"),
    ],
    "scripts/ci/disc-coverage-baseline.json": [("*", "up")],
    "scripts/ci/byte-account-baseline.json": [
        ("*work_bytes*", "down"),
        ("*", "up"),
    ],
    "scripts/ci/retail-compare-baseline.json": [
        ("settle_ticks", "pin"),
        ("classes/*", "skip"),
        ("states/*", "up"),
    ],
    "scripts/ghidra-analysis/dump-base-baseline.json": [("shifted", "set")],
    "scripts/ghidra-analysis/jal-target-baseline.json": [("unresolved", "set")],
    "scripts/replays/boot_logos_baseline.toml": [("*", "up")],
    "scripts/replays/chapter1_frontier_baseline.toml": [("*", "up")],
    "scripts/replays/critical_path_baseline.toml": [("*", "up")],
    "scripts/replays/full_game_baseline.toml": [("*", "up")],
    "scripts/replays/menu_replay_baseline.toml": [("*", "up")],
    "scripts/replays/minigame_replay_baseline.toml": [("*", "up")],
    "scripts/replays/play_compose_baseline.toml": [("*", "up")],
}

ABSENT = "absent"


def parse(path: str, text: str) -> dict:
    return tomllib.loads(text) if path.endswith(".toml") else json.loads(text)


def direction(rules: list[tuple[str, str]], key: str) -> str | None:
    for pattern, way in rules:
        if fnmatch.fnmatchcase(key, pattern):
            return way
    return None


def leaves(node, rules: list[tuple[str, str]], prefix: str = "") -> dict[str, object]:
    """Flatten a baseline to `{key: value}` over its ratcheted leaves. A list
    under a `set` rule contributes one `key/<entry>` leaf per entry."""
    out: dict[str, object] = {}
    if isinstance(node, dict):
        for name, child in node.items():
            out.update(leaves(child, rules, f"{prefix}/{name}" if prefix else str(name)))
    elif isinstance(node, list):
        if direction(rules, prefix) == "set":
            for entry in node:
                out[f"{prefix}/{entry}"] = "listed"
    elif isinstance(node, bool):
        pass
    elif isinstance(node, (int, float)):
        out[prefix] = node
    return out


def loosenings(path: str, old: dict, new: dict) -> tuple[list[tuple[str, object, object]], list[str]]:
    """`(key, from, to)` for every leaf that moved the wrong way between two
    parses of one baseline, plus the keys no rule classifies."""
    rules = RULES[path]
    before, after = leaves(old, rules), leaves(new, rules)
    found: list[tuple[str, object, object]] = []
    unclassified: list[str] = []
    for key in sorted(before.keys() | after.keys()):
        was, now = before.get(key, ABSENT), after.get(key, ABSENT)
        # A set entry is classified by its list, not its own name.
        way = "set" if "listed" in (was, now) else direction(rules, key)
        if way is None:
            unclassified.append(key)
            continue
        if way == "skip" or was == now:
            continue
        if way == "set":
            if was == ABSENT:
                found.append((key, was, now))
            continue
        if was == ABSENT:
            continue  # a newly baselined figure tightens the ratchet
        if now == ABSENT:
            found.append((key, was, now))
        elif way == "pin" or (way == "up" and now < was) or (way == "down" and now > was):
            found.append((key, was, now))
    return found, unclassified


def load_ledger(path: Path = LEDGER) -> tuple[set[tuple[str, str, str, str]], list[str]]:
    """The recorded transitions as `(file, key, from, to)` strings, and the
    ledger's own defects."""
    recorded: set[tuple[str, str, str, str]] = set()
    problems: list[str] = []
    if not path.is_file():
        return recorded, problems
    for i, entry in enumerate(tomllib.loads(path.read_text()).get("loosening", [])):
        where = f"{path.name} entry {i + 1}"
        file = entry.get("file", "")
        if file not in RULES:
            problems.append(f"{where}: `file` is not a baseline this gate reads: {file!r}")
            continue
        if len(str(entry.get("reason", "")).split()) < 4:
            problems.append(f"{where}: `reason` must say why the ratchet was let out")
            continue
        for change in entry.get("changes", []):
            if len(change) != 3:
                problems.append(f"{where}: a change is [key, from, to]: {change!r}")
                continue
            key, was, now = change
            recorded.add((file, str(key), str(was), str(now)))
    return recorded, problems


def git(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=REPO, capture_output=True, text=True)


def resolve_base(requested: str | None) -> str:
    if requested:
        return requested
    head = git("rev-parse", "HEAD").stdout.strip()
    for main in ("origin/main", "main"):
        found = git("merge-base", "HEAD", main)
        if found.returncode == 0 and found.stdout.strip():
            base = found.stdout.strip()
            # On main itself the merge-base is HEAD: compare the tree to it.
            return base if base != head else "HEAD"
    return "HEAD"


def toml_value(value: object) -> str:
    return json.dumps(value)


def propose(found: dict[str, list[tuple[str, object, object]]]) -> str:
    out = []
    for file, changes in found.items():
        out.append("[[loosening]]")
        out.append(f'file = "{file}"')
        out.append('reason = ""')
        out.append("changes = [")
        for key, was, now in changes:
            out.append(f"  [{toml_value(key)}, {toml_value(was)}, {toml_value(now)}],")
        out.append("]")
        out.append("")
    return "\n".join(out)


def self_test() -> int:
    path = "scripts/ci/port-catalog-baseline.json"
    old = {"totals": {"ported": 10}, "worklist": {"port": 0}, "live": {"gap": 0}}
    cases = [
        ({"totals": {"ported": 11}, "worklist": {"port": 0}, "live": {"gap": 0}}, []),
        ({"totals": {"ported": 9}, "worklist": {"port": 0}, "live": {"gap": 0}}, ["totals/ported"]),
        ({"totals": {"ported": 10}, "worklist": {"port": 2}, "live": {"gap": 0}}, ["worklist/port"]),
        ({"totals": {"ported": 10}, "worklist": {"port": 0}}, ["live/gap"]),
    ]
    failures = 0
    for new, want in cases:
        got = [k for k, _, _ in loosenings(path, old, new)[0]]
        if got != want:
            print(f"self-test: {new} -> {got}, want {want}")
            failures += 1
    sets = "scripts/ghidra-analysis/jal-target-baseline.json"
    got = [k for k, _, _ in loosenings(sets, {"unresolved": ["a"]}, {"unresolved": ["b"]})[0]]
    if got != ["unresolved/b"]:
        print(f"self-test: set growth -> {got}")
        failures += 1
    pinned = "scripts/ci/retail-compare-baseline.json"
    got = loosenings(
        pinned,
        {"settle_ticks": 60, "states": {"s": {"image": 0.9}}, "classes": {"s": "battle"}},
        {"settle_ticks": 30, "states": {"s": {"image": 0.95}}, "classes": {}},
    )[0]
    if [k for k, _, _ in got] != ["settle_ticks"]:
        print(f"self-test: pin -> {got}")
        failures += 1
    unclassified = loosenings(path, old, {**old, "extra": 1})[1]
    if unclassified != ["extra"]:
        print(f"self-test: unclassified -> {unclassified}")
        failures += 1
    print("self-test: FAILED" if failures else "self-test: ok")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", help="revision to compare against (default: merge-base with main)")
    ap.add_argument("--propose", action="store_true", help="print ledger entries for the loosenings found")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    base = resolve_base(args.base)
    if git("rev-parse", "--verify", "--quiet", f"{base}^{{commit}}").returncode != 0:
        print(f"baseline loosening: base revision {base!r} is not in this clone", file=sys.stderr)
        return 1

    recorded, problems = load_ledger()
    found: dict[str, list[tuple[str, object, object]]] = {}
    compared = 0
    for path in RULES:
        shown = git("show", f"{base}:{path}")
        if shown.returncode != 0:
            continue  # not baselined at the base: nothing to loosen
        old = parse(path, shown.stdout)
        here = REPO / path
        if not here.is_file():
            if (path, path, "present", "removed") not in recorded:
                found.setdefault(path, []).append((path, "present", "removed"))
            continue
        compared += 1
        moved, unclassified = loosenings(path, old, parse(path, here.read_text()))
        for key in unclassified:
            problems.append(f"{path}: `{key}` matches no rule in RULES - say which way it ratchets")
        for key, was, now in moved:
            if (path, key, str(was), str(now)) not in recorded:
                found.setdefault(path, []).append((key, was, now))

    if args.propose:
        print(propose(found), end="")
        return 0
    for problem in problems:
        print(f"baseline loosening: {problem}", file=sys.stderr)
    for path, changes in found.items():
        for key, was, now in changes:
            print(f"baseline loosening: {path}: {key}: {was} -> {now}", file=sys.stderr)
    total = sum(len(c) for c in found.values())
    if total or problems:
        if total:
            print(
                f"baseline loosening: {total} value(s) loosened against {base} with no ledger entry.\n"
                f"  If the code regressed, fix it. If the measurement legitimately moved, record it:\n"
                f"  scripts/ci/check-baseline-loosening.py --propose >> scripts/ci/baseline-loosenings.toml\n"
                f"  and fill in each `reason`.",
                file=sys.stderr,
            )
        return 1
    print(f"baseline loosening: OK ({compared} baselines against {base})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
