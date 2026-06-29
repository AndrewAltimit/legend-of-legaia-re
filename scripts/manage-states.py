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

# Immutable, fingerprint-named backups of ephemeral emulator save states.
# Gitignored (Sony game RAM). The committed catalogue is the manifest's
# per-scenario `backup_fingerprint` field; the library is the bytes.
LIBRARY_DIR = REPO_ROOT / "saves" / "library"

# Default file extension per emulator (used when --ext is omitted and the
# source path has no informative suffix).
EMULATOR_EXT = {
    "pcsx-redux": "sstate",
    "mednafen": "mcr",
    "duckstation": "sav",
}


def file_sha256(path: Path) -> str:
    """Full lowercase hex sha256 of a file's bytes."""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def library_path(emulator: str, fingerprint: str, ext: str) -> Path:
    """Resolve the immutable library path for a backed-up save state."""
    return LIBRARY_DIR / emulator / f"{fingerprint}.{ext}"


def find_library_file(fingerprint: str) -> Path | None:
    """Locate a backed-up save by (full or prefix) fingerprint, across all
    emulator subdirs. Returns the path or None."""
    if not LIBRARY_DIR.is_dir():
        return None
    for emu_dir in sorted(LIBRARY_DIR.iterdir()):
        if not emu_dir.is_dir():
            continue
        for f in sorted(emu_dir.iterdir()):
            if f.is_file() and f.stem.startswith(fingerprint):
                return f
    return None


# --------------------------------------------------------------------
# Manifest loading + per-emulator path resolution

def load_manifest(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def expand_path(s: str) -> Path:
    """Expand ~ and $VARS in a manifest path field."""
    return Path(os.path.expandvars(os.path.expanduser(s)))


def backup_resolved_path(emulator: str, fingerprint: str | None) -> Path | None:
    """If `fingerprint` names a file in the immutable library for this
    emulator, return that path. The library is the preferred source: live
    emulator slots get overwritten, so a scenario with a `backup_fingerprint`
    resolves to the stable copy first."""
    if not fingerprint:
        return None
    emu_dir = LIBRARY_DIR / emulator
    if not emu_dir.is_dir():
        return None
    for f in sorted(emu_dir.iterdir()):
        if f.is_file() and f.stem.startswith(fingerprint):
            return f
    return None


def mednafen_path(manifest: dict, scenario: dict) -> Path | None:
    """Resolve the mednafen .mc{N} path for a scenario. Prefers an immutable
    library backup (via `backup_fingerprint`) over the wipe-prone live slot."""
    bp = backup_resolved_path("mednafen", scenario.get("backup_fingerprint"))
    if bp:
        return bp
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
    bp = backup_resolved_path("pcsx-redux", scenario.get("backup_fingerprint"))
    if bp:
        return bp
    v = scenario.get("pcsx_redux_sstate")
    return expand_path(v) if v else None


def duckstation_path(scenario: dict) -> Path | None:
    bp = backup_resolved_path("duckstation", scenario.get("backup_fingerprint"))
    if bp:
        return bp
    v = scenario.get("duckstation_sav")
    return expand_path(v) if v else None


# --------------------------------------------------------------------
# Fingerprinting - per emulator

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
        "PCSX-Redux fingerprinting not wired yet - defer to follow-up "
        "task. Use mednafen-side fingerprint as the source of truth "
        "for now (the same scenario across emulators should produce "
        "the same first-64-KiB RAM contents modulo emulator BSS init "
        "differences)."
    )


# --------------------------------------------------------------------
# Manifest writeback - line-edit one field per scenario

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
    n = len(lines)

    # --- Phase 1: locate the matched [[scenarios]] block's line span,
    # its `label` line, and any pre-existing `{field} =` line within it.
    #
    # A [[scenarios]] block runs until the next top-level [[table]] (a
    # nested [scenarios.subtable] does NOT end it). We must scan the
    # whole block before deciding insert-vs-replace: a single-pass
    # "insert right after label" would duplicate a field that already
    # exists later in the same block.
    block_start: int | None = None  # index of the matched `[[scenarios]]` line
    label_idx: int | None = None  # index of the matched `label = ...` line
    field_idx: int | None = None  # index of an existing `{field} =` line in-block

    cur_block: int | None = None
    cur_matched = False
    i = 0
    while i < n:
        stripped = lines[i].strip()
        if stripped == "[[scenarios]]":
            cur_block = i
            cur_matched = False
            i += 1
            continue
        # Any other top-level [[table]] ends the current block.
        if stripped.startswith("[[") and stripped != "[[scenarios]]":
            if cur_matched:
                break  # matched block fully scanned
            cur_block = None
        if cur_block is not None:
            m = _SCENARIO_LABEL_RE.match(lines[i])
            if m and m.group(1) == label and block_start is None:
                block_start = cur_block
                label_idx = i
                cur_matched = True
            elif cur_matched and lines[i].lstrip().startswith(f"{field} ="):
                field_idx = i
                break  # existing field found; nothing left to scan
        i += 1

    if block_start is None or label_idx is None:
        return False

    # --- Phase 2: replace in place if the field exists, else insert it
    # on the line after `label`.
    if field_idx is not None:
        indent = lines[field_idx][: len(lines[field_idx]) - len(lines[field_idx].lstrip())]
        lines[field_idx] = f'{indent}{field} = "{value}"\n'
    else:
        lines.insert(label_idx + 1, f'{field} = "{value}"\n')

    manifest_path.write_text("".join(lines))
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


def cmd_backup(args: argparse.Namespace) -> int:
    """Copy an ephemeral save state into the immutable, fingerprint-named
    library, and (optionally) record its fingerprint on a manifest scenario."""
    src = expand_path(args.path)
    if not src.is_file():
        print(f"ERROR: source save not found: {src}", file=sys.stderr)
        return 1
    emulator = args.emulator
    # Prefer the emulator's canonical extension (the source suffix is
    # misleading for PCSX-Redux, where ".sstate6" carries a slot digit).
    ext = args.ext or EMULATOR_EXT.get(emulator) \
        or (src.suffix.lstrip(".") if src.suffix else "bin")
    fingerprint = file_sha256(src)
    dest = library_path(emulator, fingerprint, ext)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        print(f"EXISTS  {dest.relative_to(REPO_ROOT)}  (already backed up; immutable)")
    else:
        shutil.copy2(src, dest)
        print(f"BACKED  {src}\n     -> {dest.relative_to(REPO_ROOT)}")
    print(f"        fingerprint = {fingerprint}")
    print(f"        emulator    = {emulator}   ext = {ext}   size = {dest.stat().st_size} bytes")

    if args.label:
        ok = set_scenario_field(
            args.manifest, args.label, "backup_fingerprint", fingerprint
        )
        if ok:
            print(f"        manifest scenario '{args.label}' -> backup_fingerprint set")
        else:
            print(
                f"WARN  scenario '{args.label}' not found in {args.manifest.name}; "
                f"add a [[scenarios]] block with backup_fingerprint = \"{fingerprint}\"",
                file=sys.stderr,
            )
    else:
        print("        (no --label given; add backup_fingerprint to a "
              "[[scenarios]] block manually to catalogue it)")
    return 0


def library_emulators_for(fingerprint: str) -> list[str]:
    """Emulator subdirs in which a (full or prefix) fingerprint has a file.

    Distinct from `find_library_file`, which stops at the first hit: the audit
    needs the full emulator set because PCSX-Redux probes can only load a
    `pcsx-redux` `.sstate`, never a `mednafen` `.mcr` backed up under the same
    fingerprint."""
    if not LIBRARY_DIR.is_dir():
        return []
    emus: list[str] = []
    for emu_dir in sorted(p for p in LIBRARY_DIR.iterdir() if p.is_dir()):
        if any(f.is_file() and f.stem.startswith(fingerprint)
               for f in emu_dir.iterdir()):
            emus.append(emu_dir.name)
    return emus


def cmd_library_audit(args: argparse.Namespace) -> int:
    """Scenario-centric catalogue audit. For every manifest scenario, report
    whether it has an immutable library backup, for which emulator(s), and
    whether it is usable for a PCSX-Redux breakpoint probe (which needs a
    `pcsx-redux` `.sstate` - a mednafen-only backup does not qualify, even
    though it IS catalogued). Also flags orphan backups (library files no
    scenario references) and BACKUP-MISSING pointers (fingerprint recorded
    but the file is gone)."""
    manifest = load_manifest(args.manifest)
    scenarios = manifest.get("scenarios", [])

    rows = []  # (label, phase, cls, pcsx_ok, fp, has_pcsx_path, has_real_slot)
    referenced: set[str] = set()
    for s in scenarios:
        label = s.get("label", "<no-label>")
        phase = s.get("phase", "-")
        fp = s.get("backup_fingerprint")
        if fp:
            referenced.add(fp)
        pcsx_path = bool(s.get("pcsx_redux_sstate"))
        slot = s.get("slot")
        # 255 is the manifest's "no live mc slot" sentinel, not a real pointer.
        real_slot = slot is not None and slot != 255
        live_ptr = pcsx_path or real_slot or bool(s.get("duckstation_sav"))
        emus = library_emulators_for(fp) if fp else []

        if fp and emus:
            cls = "CATALOGED(" + "+".join(emus) + ")"
        elif fp and not emus:
            cls = "BACKUP-MISSING"
        elif live_ptr:
            cls = "EPHEMERAL-ONLY"
        else:
            cls = "NO-SAVE"
        pcsx_ok = ("pcsx-redux" in emus) or pcsx_path
        rows.append((label, phase, cls, pcsx_ok, fp, pcsx_path, real_slot))

    # Summary counts.
    from collections import Counter
    cnt = Counter(r[2] for r in rows)
    print(f"# scenario catalogue audit ({args.manifest.name}) - {len(rows)} scenarios")
    for k in sorted(cnt):
        print(f"    {k:22} {cnt[k]}")
    print(f"    {'PCSX-probe-usable':22} {sum(1 for r in rows if r[3])}")
    print(f"    {'NOT PCSX-usable':22} {sum(1 for r in rows if not r[3])}")

    # Orphan backups: on-disk library files no scenario fingerprint references.
    # Only the fingerprint-named emulator dirs are in scope; the `cards/`
    # subdir holds named full-playthrough memory cards (a swap-in capture
    # library), not fingerprint scenario backups, so it is never "orphaned".
    if LIBRARY_DIR.is_dir():
        for emu_dir in sorted(p for p in LIBRARY_DIR.iterdir()
                              if p.is_dir() and p.name in EMULATOR_EXT):
            files = [f for f in emu_dir.iterdir() if f.is_file()]
            orphans = [f.name for f in files
                       if not any(f.stem.startswith(r) for r in referenced)]
            if orphans:
                print(f"\n# ORPHAN backups in {emu_dir.name} "
                      f"({len(orphans)} of {len(files)} unreferenced):")
                for o in sorted(orphans):
                    print(f"    {o}")

    def section(title, pred):
        sub = [r for r in rows if pred(r)]
        if not sub:
            return
        print(f"\n# {title} ({len(sub)})")
        for label, phase, cls, pcsx_ok, fp, _pp, _rs in sub:
            tag = "PCSX-OK" if pcsx_ok else "no-pcsx"
            fps = fp[:12] if fp else "-"
            print(f"    [{phase:9}] {label:42} {cls:20} {tag:8} fp={fps}")

    section("BACKUP-MISSING (fingerprint recorded but file gone -> re-capture or drop)",
            lambda r: r[2] == "BACKUP-MISSING")
    section("EPHEMERAL-ONLY (live-slot pointer, never backed up -> back up or toss)",
            lambda r: r[2] == "EPHEMERAL-ONLY")
    section("NO-SAVE (pure markers / slot=255 sentinel; nothing to back up)",
            lambda r: r[2] == "NO-SAVE")
    section("CATALOGED but NOT PCSX-probe-usable (mednafen-only backup)",
            lambda r: r[2].startswith("CATALOGED") and not r[3])
    return 0


def cmd_library(args: argparse.Namespace) -> int:
    """List the immutable save-state library and cross-reference which
    manifest scenario (if any) points at each backed-up file."""
    if getattr(args, "audit", False):
        return cmd_library_audit(args)
    manifest = load_manifest(args.manifest)
    # fingerprint -> scenario label
    by_fp: dict[str, str] = {}
    for s in manifest.get("scenarios", []):
        fp = s.get("backup_fingerprint")
        if fp:
            by_fp[fp] = s.get("label", "?")

    if not LIBRARY_DIR.is_dir():
        print(f"# no library yet at {LIBRARY_DIR.relative_to(REPO_ROOT)}")
        print("# back up an ephemeral save with: manage-states.py backup <emulator> <path>")
        return 0

    print(f"# {LIBRARY_DIR.relative_to(REPO_ROOT)}  (immutable, gitignored)")
    header = f"  {'fingerprint':<20} {'emulator':<12} {'ext':<7} {'MiB':<6} {'scenario':<28} status"
    print(header)
    print("  " + "-" * (len(header) - 2))
    n = 0
    for emu_dir in sorted(LIBRARY_DIR.iterdir()):
        if not emu_dir.is_dir():
            continue
        for f in sorted(emu_dir.iterdir()):
            if not f.is_file():
                continue
            n += 1
            fp = f.stem
            mib = f.stat().st_size / (1024 * 1024)
            label = ""
            status = "uncatalogued"
            for cat_fp, cat_label in by_fp.items():
                if fp.startswith(cat_fp) or cat_fp.startswith(fp):
                    label, status = cat_label, "catalogued"
                    break
            print(f"  {fp[:18] + '..':<20} {emu_dir.name:<12} {f.suffix.lstrip('.'):<7} "
                  f"{mib:<6.2f} {label:<28} {status}")
    # Catalogued-but-missing: manifest references a fingerprint with no file.
    present = set()
    for emu_dir in (p for p in LIBRARY_DIR.iterdir() if p.is_dir()):
        for f in emu_dir.iterdir():
            if f.is_file():
                present.add(f.stem)
    for fp, label in by_fp.items():
        if not any(p.startswith(fp) or fp.startswith(p) for p in present):
            print(f"  {fp[:18] + '..':<20} {'?':<12} {'?':<7} {'--':<6} {label:<28} MISSING (re-capture)")
    print(f"\n  {n} backed-up save(s); {len(by_fp)} catalogued in {args.manifest.name}")
    return 0


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

    pb = sub.add_parser(
        "backup",
        help="Copy an ephemeral save into the immutable fingerprint-named library",
    )
    pb.add_argument("emulator", choices=sorted(EMULATOR_EXT.keys()),
                    help="which emulator produced the save")
    pb.add_argument("path", help="path to the ephemeral save state to back up")
    pb.add_argument("--label", help="manifest scenario to record backup_fingerprint on")
    pb.add_argument("--ext", help="override the library file extension")
    pb.set_defaults(func=cmd_backup)

    pl = sub.add_parser(
        "library", help="List the immutable save-state library + catalogue status"
    )
    pl.add_argument(
        "--audit", action="store_true",
        help="Scenario-centric audit: emulator-aware catalogue status, "
             "PCSX-probe-usability, orphan backups + missing/ephemeral gaps",
    )
    pl.set_defaults(func=cmd_library)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
