#!/usr/bin/env python3
"""How many `// PORT:` routines the retail kernel differential runs.

The figure is read from the `differential!` declarations in
`crates/retail-exec/tests/` (see `differential_map.py`), which the tests
themselves check against the routines they enter when the disc is present. So
the count needs no disc and no build, and gates on a runner.

    differential-coverage.py                  # report: covered / ported, per routine
    differential-coverage.py --uncovered      # PORT-tagged addresses no differential runs
    differential-coverage.py --check          # declarations well-formed + ratchet
    differential-coverage.py --update-baseline
    differential-coverage.py --record         # run the suite with the ledger on (disc)
    differential-coverage.py --propose        # ledger vs declarations, with fixes
    differential-coverage.py --propose --apply  # write the missing declarations

`--record` runs `cargo test -p psx-retail-exec --test integration` with
`PSX_RETAIL_EXEC_LEDGER` pointed at `target/differential-coverage/ledger.tsv`:
one line per (test, routine entered), with the first bytes at the entry. Those
bytes are Sony's - the file stays under `target/` and is never committed.
`--propose` resolves each entry's image off `extracted/` and prints the
`differential!` line each undeclared test should open with; `--apply` inserts
it. Exits 1 on `--check` failure.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import differential_map  # noqa: E402
from differential_map import REPO, TESTS_DIR  # noqa: E402
from port_catalog.citations import collect_ports  # noqa: E402

BASELINE = REPO / "scripts" / "ci" / "differential-coverage-baseline.json"
LEDGER = REPO / "target" / "differential-coverage" / "ledger.tsv"
EXTRACTED = REPO / "extracted"


def snapshot(scan: differential_map.Scan, ported: set[str]) -> dict:
    declared = scan.addrs()
    return {
        "ported_covered": len(declared & ported),
        "routines": len(declared),
        "tests_declared": sum(1 for t in scan.tests if t.routines),
    }


def report(scan: differential_map.Scan, ported: set[str], uncovered: bool) -> None:
    snap = snapshot(scan, ported)
    by_va = scan.by_va()
    declared = scan.addrs()
    if uncovered:
        for a in sorted(ported - declared):
            print(f"FUN_{a}")
        return
    n = len(ported)
    pct = 100.0 * snap["ported_covered"] / n if n else 0.0
    print(
        f"differential coverage: {snap['ported_covered']} / {n} PORT-tagged "
        f"addresses ({pct:.1f}%) run by a retail-exec differential"
    )
    print(
        f"  {snap['routines']} routine(s) declared by {snap['tests_declared']} test(s); "
        f"{sum(1 for t in scan.tests if t.none_reason)} test(s) marked not_a_differential"
    )
    untagged = sorted(declared - ported)
    if untagged:
        print(
            f"  {len(untagged)} declared routine(s) carry no PORT tag (a missing "
            "tag, or a helper the test runs beside the routine it compares):"
        )
        for a in untagged:
            tests = ", ".join(sorted({t.path for t, _ in by_va[int(a, 16)]}))
            print(f"    FUN_{a}  {tests}")
    print()
    for va in sorted(by_va):
        rows = by_va[va]
        imgs = sorted({"SCUS" if i is None else f"PROT {i:04}" for _, i in rows})
        tag = "" if f"{va:08x}" in ported else "  (no PORT tag)"
        print(f"FUN_{va:08x}  [{', '.join(imgs)}]{tag}")
        for t, _ in rows:
            print(f"    {t.path}")


def check(scan: differential_map.Scan, ported: set[str]) -> int:
    rc = 0
    for e in scan.errors:
        print(f"differential-coverage: {e}")
        rc = 1
    snap = snapshot(scan, ported)
    if not BASELINE.is_file():
        print(f"differential-coverage: no baseline at {BASELINE.relative_to(REPO)}; run --update-baseline")
        return 1
    base = json.loads(BASELINE.read_text())
    for key, now in snap.items():
        was = base.get(key)
        if was is None:
            print(f"differential-coverage: baseline lacks `{key}`; run --update-baseline")
            rc = 1
        elif now < was:
            print(
                f"differential-coverage: {key} fell {was} -> {now}. A differential "
                "stopped declaring a routine, or a PORT tag it covered was removed. "
                "If that is right, --update-baseline and record the step in "
                "scripts/ci/baseline-loosenings.toml."
            )
            rc = 1
        elif now > was:
            print(f"differential-coverage: {key} rose {was} -> {now} (run --update-baseline to keep it)")
    if rc == 0:
        print(
            f"differential-coverage: ok - {snap['ported_covered']} / {len(ported)} "
            f"PORT-tagged addresses covered, {snap['routines']} routines, "
            f"{snap['tests_declared']} tests"
        )
    return rc


# --- recording ---------------------------------------------------------------


def record() -> int:
    if not os.environ.get("LEGAIA_DISC_BIN"):
        print("differential-coverage --record: LEGAIA_DISC_BIN is unset; the suite would skip every test")
        return 2
    LEDGER.parent.mkdir(parents=True, exist_ok=True)
    LEDGER.unlink(missing_ok=True)
    env = dict(os.environ, PSX_RETAIL_EXEC_LEDGER=str(LEDGER))
    cmd = [
        "cargo", "test", "-p", "psx-retail-exec", "--profile", "release-test",
        "--test", "integration",
    ]
    print("+", " ".join(cmd), f"(PSX_RETAIL_EXEC_LEDGER={LEDGER.relative_to(REPO)})")
    rc = subprocess.run(cmd, cwd=REPO, env=env).returncode
    if rc != 0:
        print("differential-coverage --record: the suite failed; the ledger covers the tests that ran")
    return rc


def read_ledger(path: Path) -> dict[str, dict[int, bytes]]:
    out: dict[str, dict[int, bytes]] = {}
    for line in path.read_text().splitlines():
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        test, entry, body = parts
        out.setdefault(test, {})[int(entry, 16)] = bytes.fromhex(body)
    return out


class Images:
    """The extracted images, to name the one that holds an entry's bytes."""

    def __init__(self, root: Path):
        self.scus = (root / "SCUS_942.54").read_bytes() if (root / "SCUS_942.54").is_file() else b""
        self.prot: dict[int, Path] = {}
        for p in sorted((root / "PROT").glob("*")):
            m = re.match(r"(\d{4})_", p.name)
            if m:
                self.prot[int(m.group(1))] = p
        self._cache: dict[int, bytes] = {}

    def entry(self, n: int) -> bytes:
        if n not in self._cache:
            self._cache[n] = self.prot[n].read_bytes()
        return self._cache[n]

    def resolve(self, va: int, body: bytes, preferred: list[int]) -> tuple[int | None, str]:
        """`(prot entry or None for SCUS, note)`."""
        off = va - 0x8001_0000 + 0x800
        if self.scus and 0 <= off and self.scus[off : off + len(body)] == body:
            return None, ""
        for n in preferred:
            if n in self.prot and body in self.entry(n):
                return n, ""
        hits = [n for n in self.prot if body in self.entry(n)]
        if not hits:
            return None, "bytes found in no image - check by hand"
        note = "" if len(hits) == 1 else f"also in PROT {', '.join(f'{h:04}' for h in hits[1:6])}"
        return hits[0], note


def _literal(va: int, consts: dict[str, int]) -> str:
    for name, v in sorted(consts.items()):
        if v == va and v > 0xFFFF:
            return name
    h = f"{va:08X}"
    return f"0x{h[:4]}_{h[4:]}"


def propose(scan: differential_map.Scan, ledger: Path, apply: bool) -> int:
    if not ledger.is_file():
        print(f"differential-coverage --propose: no ledger at {ledger}; run --record first")
        return 2
    entered = read_ledger(ledger)
    images = Images(EXTRACTED)
    if not images.prot:
        print(f"differential-coverage --propose: no extracted images under {EXTRACTED}")
        return 2
    by_test = {t.path: t for t in scan.tests}
    edits: dict[Path, list[tuple[int, str]]] = {}
    rc = 0
    for path, t in sorted(by_test.items()):
        ran = entered.get(path, {})
        file = TESTS_DIR / f"{t.module}.rs"
        text = file.read_text()
        consts = {
            name: int(v.replace("_", ""), 0)
            for name, v in re.findall(r"^\s*(?:pub\s+)?const\s+([A-Z_][A-Z0-9_]*)\s*:\s*u32\s*=\s*(0x[0-9a-fA-F_]+)\s*;", text, re.M)
        }
        wanted = [int(x) for x in re.findall(r"read_entry\((\d+)\)", text)]
        if t.declared:
            missing = [va for _, va in t.routines if va not in ran]
            if missing and ran:
                rc = 1
                print(f"{path}: declares {', '.join(f'{v:08X}' for v in missing)} but entered none of them")
            extra = sorted(set(ran) - {va for _, va in t.routines})
            if extra:
                print(f"{path}: also enters {', '.join(f'{v:08X}' for v in extra)} (undeclared; fine for setup calls)")
            continue
        if not ran:
            line = 'not_a_differential!("<why this test runs no routine>");'
            print(f"{path}: enters no routine - open it with {line}")
            continue
        parts, notes = [], []
        for va, body in sorted(ran.items()):
            img, note = images.resolve(va, body, wanted)
            lit = _literal(va, consts)
            parts.append(f"scus({lit})" if img is None else f"prot({img}, {lit})")
            if note:
                notes.append(f"{va:08X}: {note}")
        line = f"differential!({', '.join(parts)});"
        print(f"{path}: {line}" + (f"   // {'; '.join(notes)}" if notes else ""))
        edits.setdefault(file, []).append((t.line, line))
    if apply:
        for file, rows in edits.items():
            lines = file.read_text().split("\n")
            for line_no, decl in sorted(rows, reverse=True):
                # The `fn` line is at or after the `#[test]` the scan reported;
                # insert after the first line that opens the body.
                i = line_no - 1
                while i < len(lines) and not lines[i].rstrip().endswith("{"):
                    i += 1
                indent = re.match(r"\s*", lines[i]).group(0) + "    "
                lines.insert(i + 1, indent + decl)
            file.write_text("\n".join(lines))
            print(f"wrote {file.relative_to(REPO)} ({len(rows)} declaration(s))")
    return rc


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--check", action="store_true", help="declarations well-formed, coverage not below the baseline")
    g.add_argument("--update-baseline", action="store_true")
    g.add_argument("--record", action="store_true", help="run the suite with the ledger on (needs the disc)")
    g.add_argument("--propose", action="store_true", help="ledger vs declarations")
    g.add_argument("--uncovered", action="store_true", help="list PORT-tagged addresses no differential runs")
    ap.add_argument("--apply", action="store_true", help="with --propose: write the missing declarations")
    ap.add_argument("--ledger", default=str(LEDGER), help="ledger file for --propose")
    args = ap.parse_args()

    if args.record:
        return record()
    scan = differential_map.scan()
    if args.propose:
        return propose(scan, Path(args.ledger), args.apply)
    ported = set(collect_ports())
    if args.check:
        return check(scan, ported)
    if args.update_baseline:
        if scan.errors:
            for e in scan.errors:
                print(f"differential-coverage: {e}")
            return 1
        BASELINE.write_text(json.dumps(snapshot(scan, ported), indent=2, sort_keys=True) + "\n")
        print(f"wrote {BASELINE.relative_to(REPO)}")
        return 0
    report(scan, ported, args.uncovered)
    for e in scan.errors:
        print(f"differential-coverage: {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
