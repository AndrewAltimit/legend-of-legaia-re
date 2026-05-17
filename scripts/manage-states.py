#!/usr/bin/env python3
"""Manage the unified save-state catalogue at scripts/scenarios.toml.

The manifest names each labelled save state and (optionally) records a
sha256 fingerprint of the first 64 KiB of main RAM after the load
settles. Subcommands here exercise that catalogue cross-emulator:

    list                       Print the manifest + on-disk presence
                               flags for each emulator.
    fingerprint [<label>...]   Compute ram_fingerprint_sha256 for one
                               or all scenarios and update the manifest
                               in place. Mednafen is supported today;
                               PCSX-Redux + Duckstation print TODO.
    validate [<label>...]      Re-fingerprint and compare against the
                               committed value. Exits non-zero on
                               drift. Skips scenarios that don't have
                               a fingerprint yet.

The manifest path can be overridden with --manifest <path>; default is
scripts/scenarios.toml.

Why not just use the mednafen-state CLI directly?
    `mednafen-state` knows how to extract main RAM from a single mednafen
    save state, but it doesn't know about cross-emulator scenarios.
    manage-states.py is the cross-emulator front door: it consults the
    unified manifest, resolves whichever save state(s) you have on disk,
    and delegates per-emulator work to the right extractor.

See docs/tooling/mednafen-automation.md for the diff-sweep workflow and
docs/tooling/pcsx-redux-automation.md for the probe workflow.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Iterable

# --------------------------------------------------------------------
# Repo + manifest layout

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "scripts" / "scenarios.toml"
MEDNAFEN_BIN = REPO_ROOT / "target" / "release" / "mednafen-state"


# --------------------------------------------------------------------
# Manifest loading + per-emulator path resolution

def load_manifest(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def expand_path(s: str) -> Path:
    """Expand ~ and $VARS in a manifest path field."""
    return Path(os.path.expandvars(os.path.expanduser(s)))


def mednafen_path(manifest: dict, scenario: dict) -> Path | None:
    """Resolve the mednafen .mc{N} path for a scenario via defaults."""
    slot = scenario.get("slot")
    if slot is None:
        return None
    defaults = manifest.get("defaults", {})
    pattern = defaults.get(
        "filename_pattern",
        "Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.mc{slot}",
    )
    mcs_dir = defaults.get("mcs_dir") or os.environ.get(
        "LEGAIA_MEDNAFEN_DIR"
    ) or os.path.expanduser("~/.mednafen/mcs")
    return Path(mcs_dir) / pattern.replace("{slot}", str(slot))


def pcsx_redux_path(scenario: dict) -> Path | None:
    v = scenario.get("pcsx_redux_sstate")
    return expand_path(v) if v else None


def duckstation_path(scenario: dict) -> Path | None:
    v = scenario.get("duckstation_sav")
    return expand_path(v) if v else None


# --------------------------------------------------------------------
# Fingerprinting — per emulator

def fingerprint_mednafen(save_path: Path) -> str:
    """Extract first 64 KiB of main RAM via `mednafen-state extract` and
    sha256 it. Returns the lowercase hex digest."""
    if not MEDNAFEN_BIN.is_file():
        # Try a debug build before bailing.
        debug_bin = REPO_ROOT / "target" / "debug" / "mednafen-state"
        if debug_bin.is_file():
            tool = debug_bin
        else:
            raise RuntimeError(
                f"mednafen-state not built: {MEDNAFEN_BIN} missing. "
                f"Run: cargo build --release -p legaia-mednafen"
            )
    else:
        tool = MEDNAFEN_BIN

    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as tf:
        tmp_path = Path(tf.name)
    try:
        cmd = [
            str(tool), "extract", str(save_path),
            "--start", "0x80000000",
            "--end",   "0x80010000",
            "--out",   str(tmp_path),
        ]
        subprocess.run(cmd, check=True, capture_output=True)
        data = tmp_path.read_bytes()
    finally:
        tmp_path.unlink(missing_ok=True)
    return hashlib.sha256(data).hexdigest()


def fingerprint_pcsx_redux(save_path: Path) -> str:
    """Currently unimplemented. Wiring this up requires running PCSX-Redux
    headlessly via run_probe.sh with a one-shot dump probe; deferred."""
    raise NotImplementedError(
        "PCSX-Redux fingerprinting not wired yet — defer to follow-up "
        "task. Use mednafen-side fingerprint as the source of truth "
        "for now (the same scenario across emulators should produce "
        "the same first-64-KiB RAM contents modulo emulator BSS init "
        "differences)."
    )


# --------------------------------------------------------------------
# Manifest writeback — line-edit one field per scenario

_SCENARIO_LABEL_RE = re.compile(r'^\s*label\s*=\s*"([^"]+)"\s*$')


def set_scenario_field(
    manifest_path: Path,
    label: str,
    field: str,
    value: str,
) -> bool:
    """In-place line-based update: find the [[scenarios]] block whose
    label matches <label>, then set <field> = "<value>".

    If <field> already exists in the block, replace its value.
    If it doesn't, insert it on the line after `label = "..."` to keep
    the block visually grouped.

    Returns True on success, False if the label wasn't found.
    """
    lines = manifest_path.read_text().splitlines(keepends=True)
    out: list[str] = []
    in_block = False
    matched_block = False
    label_line_idx_in_block: int | None = None
    pending_insert = False
    replaced = False
    last_block_start = -1

    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # A new [[scenarios]] block.
        if stripped == "[[scenarios]]":
            in_block = True
            matched_block = False
            label_line_idx_in_block = None
            last_block_start = len(out)
            out.append(line)
            i += 1
            continue

        # A different top-level table ends the current scenario block.
        if in_block and stripped.startswith("[[") and stripped != "[[scenarios]]":
            in_block = False
        # Nested subtable (e.g. [scenarios.overlay_slice]) does NOT end
        # the block — those belong to the outer [[scenarios]] entry.

        # If we matched and haven't inserted yet, do it before the next
        # line that isn't part of our block.
        if pending_insert and matched_block:
            out.append(f'{field} = "{value}"\n')
            pending_insert = False
            replaced = True

        if in_block:
            m = _SCENARIO_LABEL_RE.match(line)
            if m and m.group(1) == label:
                matched_block = True
                out.append(line)
                # We'll insert <field> on the next line if it doesn't exist.
                pending_insert = True
                i += 1
                continue

            if matched_block and line.lstrip().startswith(f"{field} ="):
                # Replace existing line.
                indent = line[:len(line) - len(line.lstrip())]
                out.append(f'{indent}{field} = "{value}"\n')
                pending_insert = False
                replaced = True
                i += 1
                continue

        out.append(line)
        i += 1

    # Trailing-EOF pending insert.
    if pending_insert and matched_block:
        out.append(f'{field} = "{value}"\n')
        replaced = True

    if not replaced:
        return False

    manifest_path.write_text("".join(out))
    return True


# --------------------------------------------------------------------
# Subcommands

def cmd_list(args: argparse.Namespace) -> int:
    manifest = load_manifest(args.manifest)
    scenarios = manifest.get("scenarios", [])
    print(f"# {args.manifest.relative_to(REPO_ROOT)}  ({len(scenarios)} scenarios)")
    print(f"# legend: M=mednafen P=PCSX-Redux D=Duckstation  (• = on disk, · = manifest-defined, blank = no record)")
    print()
    header = (
        f"  {'label':<32} {'phase':<10} {'M':<2} {'P':<2} {'D':<2} "
        f"{'sha256_prefix':<10} description"
    )
    print(header)
    print("  " + "-" * (len(header) - 2))
    for s in scenarios:
        label = s.get("label", "?")
        phase = s.get("phase", "")
        m_path = mednafen_path(manifest, s)
        p_path = pcsx_redux_path(s)
        d_path = duckstation_path(s)
        m = "•" if m_path and m_path.is_file() else ("·" if m_path else " ")
        p = "•" if p_path and p_path.is_file() else ("·" if p_path else " ")
        d = "•" if d_path and d_path.is_file() else ("·" if d_path else " ")
        sha = s.get("ram_fingerprint_sha256", "")
        sha_pfx = sha[:8] if sha else ""
        desc = s.get("description", "")
        if len(desc) > 60:
            desc = desc[:57] + "..."
        print(f"  {label:<32} {phase:<10} {m:<2} {p:<2} {d:<2} {sha_pfx:<10} {desc}")
    return 0


def fingerprint_for(manifest: dict, s: dict) -> tuple[str | None, str | None]:
    """Return (digest, source-emulator-name) or (None, error_msg)."""
    m_path = mednafen_path(manifest, s)
    if m_path and m_path.is_file():
        try:
            return fingerprint_mednafen(m_path), "mednafen"
        except subprocess.CalledProcessError as e:
            return None, f"mednafen-state failed: {e.stderr.decode(errors='replace')[:200]}"
        except Exception as e:
            return None, f"mednafen fingerprint error: {e}"
    # Other emulators: deferred.
    return None, "no mednafen save on disk and other emulators not wired yet"


def cmd_fingerprint(args: argparse.Namespace) -> int:
    manifest = load_manifest(args.manifest)
    scenarios = manifest.get("scenarios", [])
    want: set[str] = set(args.labels)
    rc = 0
    for s in scenarios:
        label = s.get("label", "?")
        if want and label not in want:
            continue
        digest, source = fingerprint_for(manifest, s)
        if digest is None:
            print(f"SKIP  {label}: {source}", file=sys.stderr)
            continue
        existing = s.get("ram_fingerprint_sha256")
        if existing == digest:
            print(f"OK    {label}  sha256={digest[:16]}…  (via {source}; unchanged)")
            continue
        if args.dry_run:
            print(f"DRY   {label}  sha256={digest[:16]}…  (via {source}; would update)")
            continue
        ok = set_scenario_field(
            args.manifest, label, "ram_fingerprint_sha256", digest
        )
        if not ok:
            print(f"FAIL  {label}: cannot locate block in manifest", file=sys.stderr)
            rc = 1
            continue
        if existing:
            print(f"UPD   {label}  sha256={digest[:16]}…  (was {existing[:16]}…)")
        else:
            print(f"SET   {label}  sha256={digest[:16]}…  (new)")
    return rc


def cmd_validate(args: argparse.Namespace) -> int:
    manifest = load_manifest(args.manifest)
    scenarios = manifest.get("scenarios", [])
    want: set[str] = set(args.labels)
    rc = 0
    n_total = 0
    n_drift = 0
    n_skip  = 0
    for s in scenarios:
        label = s.get("label", "?")
        if want and label not in want:
            continue
        expected = s.get("ram_fingerprint_sha256")
        if not expected:
            print(f"SKIP  {label}: no committed fingerprint")
            n_skip += 1
            continue
        digest, source = fingerprint_for(manifest, s)
        if digest is None:
            print(f"SKIP  {label}: {source}", file=sys.stderr)
            n_skip += 1
            continue
        n_total += 1
        if digest == expected:
            print(f"OK    {label}  {digest[:16]}…")
        else:
            print(
                f"DRIFT {label}\n"
                f"      expected: {expected}\n"
                f"      observed: {digest}\n"
                f"      source:   {source}",
                file=sys.stderr,
            )
            n_drift += 1
            rc = 1
    print(
        f"\n{n_total} checked, {n_drift} drift, {n_skip} skipped",
        file=sys.stderr,
    )
    return rc


# --------------------------------------------------------------------
# Argparse driver

def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="manage-states.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--manifest", type=Path, default=DEFAULT_MANIFEST,
        help=f"path to scenarios manifest (default: {DEFAULT_MANIFEST.relative_to(REPO_ROOT)})",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("list", help="Show manifest + on-disk presence").set_defaults(
        func=cmd_list
    )
    pf = sub.add_parser("fingerprint", help="Compute + persist RAM fingerprint")
    pf.add_argument(
        "labels", nargs="*",
        help="scenarios to fingerprint (default: all)",
    )
    pf.add_argument(
        "--dry-run", action="store_true",
        help="print what would change without writing the manifest",
    )
    pf.set_defaults(func=cmd_fingerprint)

    pv = sub.add_parser("validate", help="Compare on-disk fingerprint vs manifest")
    pv.add_argument(
        "labels", nargs="*",
        help="scenarios to validate (default: all)",
    )
    pv.set_defaults(func=cmd_validate)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
