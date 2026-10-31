#!/usr/bin/env python3
"""Validate and bundle every expansion pack under packs/.

A release archive ships each pack as an `.lxpack` built by
`asset expansion bundle` (scripts/ci/release-build.sh), and the play page
bundles the same tree in the browser. A pack that stops validating would
fail only at release time; this gate fails on the commit that broke it.

For each `packs/<dir>/pack.toml`:

  1. `asset expansion validate <dir>` - errors fail (warnings are printed);
  2. `asset expansion bundle <dir> --out <tmp>` twice - the bundle is refused
     unless the pack validates, and the two runs must be byte-identical
     (the release bundle is deterministic);
  3. `asset expansion validate <tmp>.lxpack` - the bundle reads back clean.

Disc-free: a pack names disc content by coordinate only.

    python3 scripts/ci/check-expansion-packs.py            # builds `asset`
    python3 scripts/ci/check-expansion-packs.py --asset target/release/asset
    python3 scripts/ci/check-expansion-packs.py --selftest

The self-test runs first on every invocation: a deliberately broken pack must
fail validation and must be refused by the bundler, or the gate refuses to
report a pass (a detector that cannot fail measures nothing).
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PACKS = REPO / "packs"


def build_asset() -> Path:
    """Build the `asset` CLI (dev profile: the gate only parses TOML)."""
    cmd = ["cargo", "build", "--quiet", "-p", "legaia-asset", "--bin", "asset"]
    print("[expansion-packs] " + " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=REPO, check=True)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    exe = target / "debug" / ("asset.exe" if os.name == "nt" else "asset")
    if not exe.is_file():
        sys.exit(f"[expansion-packs] built asset not found at {exe}")
    return exe


def run(asset: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(asset), "expansion", *args],
        cwd=REPO,
        capture_output=True,
        text=True,
    )


def show(proc: subprocess.CompletedProcess) -> None:
    for stream in (proc.stdout, proc.stderr):
        for line in stream.splitlines():
            print(f"    {line}")


def check_pack(asset: Path, pack: Path, tmp: Path) -> list[str]:
    """Every failure for one pack, as one line each."""
    name = pack.name
    failures: list[str] = []
    v = run(asset, "validate", str(pack))
    show(v)
    if v.returncode != 0:
        return [f"{name}: does not validate"]
    out_a, out_b = tmp / f"{name}.a.lxpack", tmp / f"{name}.b.lxpack"
    for out in (out_a, out_b):
        b = run(asset, "bundle", str(pack), "--out", str(out))
        if b.returncode != 0:
            show(b)
            return [f"{name}: bundle refused"]
    if out_a.read_bytes() != out_b.read_bytes():
        failures.append(f"{name}: two bundles of the same tree differ")
    rb = run(asset, "validate", str(out_a))
    if rb.returncode != 0:
        show(rb)
        failures.append(f"{name}: the bundle does not validate")
    else:
        print(f"    bundle: {out_a.stat().st_size} bytes, reads back clean")
    return failures


def selftest(asset: Path, tmp: Path) -> bool:
    """A broken pack must fail both validation and bundling."""
    bad = tmp / "selftest-bad"
    bad.mkdir()
    (bad / "pack.toml").write_text('id = "Bad Id"\nname = ""\nversion = "1.0"\n')
    v = run(asset, "validate", str(bad))
    b = run(asset, "bundle", str(bad), "--out", str(tmp / "bad.lxpack"))
    ok = v.returncode != 0 and b.returncode != 0
    print(f"[expansion-packs] self-test: broken pack refused: {'yes' if ok else 'NO'}")
    return ok


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--asset", type=Path, help="a built `asset` binary (else cargo builds one)")
    ap.add_argument("--selftest", action="store_true", help="run only the self-test")
    args = ap.parse_args()

    asset = args.asset.resolve() if args.asset else build_asset()
    with tempfile.TemporaryDirectory(prefix="legaia-packs-") as t:
        tmp = Path(t)
        if not selftest(asset, tmp):
            print("[expansion-packs] FAIL: the self-test's broken pack was accepted")
            return 1
        if args.selftest:
            return 0
        packs = sorted(p.parent for p in PACKS.glob("*/pack.toml"))
        if not packs:
            print(f"[expansion-packs] FAIL: no pack under {PACKS.relative_to(REPO)}/")
            return 1
        failures: list[str] = []
        for pack in packs:
            print(f"[expansion-packs] {pack.relative_to(REPO)}", flush=True)
            failures += check_pack(asset, pack, tmp)
    if failures:
        for f in failures:
            print(f"[expansion-packs] FAIL: {f}")
        return 1
    print(f"[expansion-packs] OK: {len(packs)} pack(s) validate and bundle deterministically")
    return 0


if __name__ == "__main__":
    sys.exit(main())
