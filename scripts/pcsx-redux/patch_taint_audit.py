#!/usr/bin/env python3
"""Audit save states, probe logs and .ppf files for patched-disc taint.

PCSX-Redux applies `<image stem>.ppf` from beside whatever image it is handed
(logged only as `[+ppf]`), and `legaia-patcher randomize` writes its `.ppf`
beside its INPUT disc. A save state then carries the patched executable in
RAM forever: `SCUS_942.54` is loaded once at boot and never re-read, so a
state made on a patched disc keeps the patch through every later reload of
that state, whatever disc it is loaded onto. This tool measures both halves:

  states   For every library save state (PCSX-Redux `.sstate` and mednafen
           `.mcr`), read the resident SCUS window out of main RAM and compare
           a fixed table of patcher sites against the retail executable. A
           site that differs names the patch family resident in that state.
  logs     For every `pcsx.log` under a captures tree, report whether the
           emulator applied a `.ppf` (`[+ppf]`), on which image, and whether
           the run booted cold or from a state.
  ppf      Decode a PPF 3.0 file into disc byte runs -> ISO file -> PROT entry
           (or SCUS virtual address), so a patch can be matched to the
           runs that read its bytes.

No disc bytes are printed: only addresses, lengths, entry indices and site
labels. Needs the release binaries `pcsxr-state`, `mednafen-state`,
`disc-extract` and `prot-extract` (pass `--bin-dir`, default
`target/release`) and the extracted `SCUS_942.54`.

Usage:
    patch_taint_audit.py states [--library saves/library] [--scenarios scripts/scenarios.toml]
    patch_taint_audit.py logs   [--captures captures]
    patch_taint_audit.py ppf    <file.ppf> --disc <retail.bin>

See docs/tooling/pcsx-redux-automation.md#patched-disc-taint.
"""
import argparse
import os
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

SCUS_VA = 0x80010000
SCUS_HDR = 0x800
SCUS_END_VA = 0x8007B800

# (va, len, label). Each is a site one patcher feature rewrites; a state whose
# resident bytes differ from the retail executable over the window carries
# that feature. The arenas are the verified-dead SCUS regions the hand-
# assembled features share (crates/patcher/src/shiny_seru/layout.rs), so an
# arena hit alone names the class, not the feature.
SITES = [
    (0x800321D4, 8, "shiny-seru (menu hook)"),
    (0x8004AD0C, 4, "shiny-seru (anim-commit hook)"),
    (0x80051990, 8, "enemy-ally charm (battle-loader hook)"),
    (0x80051A20, 8, "battle-loader setup hook (shiny-seru / oscillating-ap)"),
    (0x800344D8, 8, "oscillating-ap (arts-list read-out)"),
    (0x80034ADC, 16, "starting-bag / warp-preset seed"),
    (0x80034B04, 40, "starting-bag / warp-preset seed"),
    (0x80073B18, 0x100, "quick-travel names (location rename)"),
    (0x80012DD0, 45, "delilas party swap"),
    (0x80054008, 1, "delilas cast"),
    (0x80077728, 0x100, "SCUS gap 1 arena"),
    (0x80078A88, 0x44, "SCUS slot 6 arena"),
    (0x8007ACA0, 0x60, "flee-exp / enemy-ally arena"),
    (0x8007AE00, 0x100, "SCUS arena 1"),
]


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def resident_scus(bin_dir, path):
    tool = "mednafen-state" if path.suffix == ".mcr" else "pcsxr-state"
    with tempfile.NamedTemporaryFile(suffix=".bin") as tmp:
        subprocess.run(
            [str(bin_dir / tool), "extract", str(path), "--start", hex(SCUS_VA),
             "--end", hex(SCUS_END_VA), "--out", tmp.name],
            check=True, capture_output=True)
        return Path(tmp.name).read_bytes()


def site_hits(retail, ram):
    hits = []
    for va, ln, label in SITES:
        off = va - SCUS_VA
        diff = sum(1 for i in range(off, off + ln) if ram[i] != retail[i])
        if diff:
            hits.append((va, diff, label))
    return hits


def scenario_labels(manifest):
    labels = {}
    if not manifest.exists():
        return labels
    cur = None
    for line in manifest.read_text().splitlines():
        m = re.match(r'\s*label\s*=\s*"([^"]+)"', line)
        if m:
            cur = m.group(1)
        for fp in re.findall(r"[0-9a-f]{64}", line):
            if cur:
                labels.setdefault(fp, []).append(cur)
    return labels


def cmd_states(args):
    retail = Path(args.scus).read_bytes()[SCUS_HDR:]
    labels = scenario_labels(Path(args.scenarios))
    rows = 0
    tainted = 0
    for sub in ("pcsx-redux", "mednafen"):
        d = Path(args.library) / sub
        if not d.is_dir():
            continue
        for p in sorted(d.iterdir()):
            if p.suffix not in (".sstate", ".mcr"):
                continue
            try:
                ram = resident_scus(Path(args.bin_dir), p)
            except subprocess.CalledProcessError:
                print(f"{sub:10s} {p.stem[:12]}  UNREADABLE")
                continue
            rows += 1
            hits = site_hits(retail, ram)
            if hits:
                tainted += 1
            if hits or args.all:
                lab = ",".join(sorted(set(labels.get(p.stem, ["-"]))))
                fam = "; ".join(sorted({h[2] for h in hits})) or "retail"
                print(f"{sub:10s} {p.stem[:12]}  {lab:40s} {fam}")
    print(f"# {tainted} of {rows} library states carry a patcher site in resident SCUS")


def cmd_logs(args):
    root = Path(args.captures)
    total = tainted = 0
    for log in sorted(root.rglob("pcsx.log")):
        txt = log.read_text(errors="replace")[:20000]
        m = re.search(r"Loaded CD Image: (.*?)(\[\+cue\])?(\[\+ppf\])?\.\s*$", txt, re.M)
        if not m:
            continue
        total += 1
        if not m.group(3):
            continue
        tainted += 1
        cold = "cold boot" in txt[:3000]
        print(f"{'COLD ' if cold else 'STATE'} {log.parent.relative_to(root)}  image={Path(m.group(1)).name}")
    print(f"# {tainted} of {total} pcsx.log files show [+ppf]")


def cmd_ppf(args):
    ppf = Path(args.ppf).read_bytes()
    if ppf[:5] != b"PPF30":
        raise SystemExit("not a PPF 3.0 file")
    pos = 60 + (1024 if ppf[57] else 0)
    recs = []
    while pos + 9 <= len(ppf):
        off, = struct.unpack_from("<Q", ppf, pos)
        n = ppf[pos + 8]
        recs.append((off, n))
        pos += 9 + n
    bin_dir = Path(args.bin_dir)
    files = []
    for line in run([str(bin_dir / "disc-extract"), "list", args.disc]).splitlines():
        m = re.match(r"\s*(\d+)\s+(\d+)\s+(\S+)", line)
        if m:
            files.append((int(m[2]), int(m[1]), m[3]))
    prot_lba = next(lba for lba, _, p in files if p == "PROT.DAT")
    with tempfile.TemporaryDirectory() as td:
        # prot-extract list reads the archive itself; hand it the extracted copy.
        prot_path = Path(args.prot) if args.prot else None
        if prot_path is None:
            raise SystemExit("--prot extracted/PROT.DAT is required")
        prot = []
        for line in run([str(bin_dir / "prot-extract"), "list", str(prot_path)]).splitlines():
            m = re.match(r"\s*(\d+)\s+0x([0-9A-Fa-f]+)\s+(\d+)", line)
            if m:
                prot.append((int(m[1]), int(m[2], 16), int(m[3])))
        del td
    agg = {}
    for off, n in recs:
        for b in range(off, off + n):
            sec, ins = divmod(b, 2352)
            if not 24 <= ins < 2072:
                key = "EDC/ECC or header bytes"
            else:
                name = next((p for lba, sz, p in files if lba <= sec < lba + (sz + 2047) // 2048), "?")
                key = name
                if name == "PROT.DAT":
                    fo = (sec - prot_lba) * 2048 + ins - 24
                    key = next((f"PROT {i:04d}" for i, bo, sz in prot if bo <= fo < bo + sz), key)
                elif name.startswith("SCUS"):
                    lba = next(lba for lba, _, p in files if p == name)
                    va = SCUS_VA + (sec - lba) * 2048 + ins - 24 - SCUS_HDR
                    key = f"SCUS VA 0x{va & ~0xFF:08X}"
            agg[key] = agg.get(key, 0) + 1
    desc = ppf[6:56].decode("latin1").strip()
    print(f"# {desc!r}: {len(recs)} records, {sum(n for _, n in recs)} bytes")
    for k in sorted(agg):
        print(f"{agg[k]:6d}  {k}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin-dir", default="target/release")
    ap.add_argument("--scus", default="extracted/SCUS_942.54")
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("states")
    s.add_argument("--library", default="saves/library")
    s.add_argument("--scenarios", default="scripts/scenarios.toml")
    s.add_argument("--all", action="store_true", help="also list retail-clean states")
    s.set_defaults(fn=cmd_states)
    lg = sub.add_parser("logs")
    lg.add_argument("--captures", default="captures")
    lg.set_defaults(fn=cmd_logs)
    pp = sub.add_parser("ppf")
    pp.add_argument("ppf")
    pp.add_argument("--disc", required=True)
    pp.add_argument("--prot", default="extracted/PROT.DAT")
    pp.set_defaults(fn=cmd_ppf)
    args = ap.parse_args()
    args.fn(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
