#!/usr/bin/env python3
"""One machine's numbers, base against tip, for a box other than the usual one.

    python scripts/ci/machine-bench.py --disc game.bin --extracted extracted \
        --exe new/legaia-engine.exe --base-exe old/legaia-engine.exe \
        --site new-site --base-site old-site

Runs frame-bench.py (frame table and entry, paired by itself) and frame-bench-web.mjs
(each row twice, the two sides alternating) one after another, then zips the output for
sending back: logs, JSON and the machine's description. Times and counts only, with no
screenshots and no disc bytes. A step that fails is recorded and the rest still run, so a
partial run still reports something. Exit 1 if any step failed.

Runs on Linux and Windows. It wants the machine otherwise idle, and takes about 45 min;
--quick does one round of the page rows at the throttles that matter, about 15 min. See
docs/tooling/frame-bench.md#another-machine.
"""

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
# name, driver flags. The rows are the wave's paired table: a display's pace, then CPU proxies.
WEB_ROWS = [
    ("vsync", ["--vsync"]),
    ("uncapped", []),
    ("t4", ["--cpu-throttle", "4", "--vsync"]),
    ("t6", ["--cpu-throttle", "6", "--vsync"]),
]


def describe():
    info = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor(),
        "cpu_count": os.cpu_count(),
        "python": platform.python_version(),
    }
    probes = {"node": ["node", "--version"]}
    if sys.platform == "win32":
        ps = "powershell -NoProfile -Command".split()
        probes["cpu"] = [*ps, "(Get-CimInstance Win32_Processor).Name"]
        probes["gpu"] = [*ps, "(Get-CimInstance Win32_VideoController).Name"]
        probes["memory_bytes"] = [*ps, "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"]
    for key, cmd in probes.items():
        try:
            info[key] = subprocess.run(cmd, capture_output=True, text=True, timeout=30).stdout.strip()
        except OSError as e:
            info[key] = f"unavailable: {e}"
    return info


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disc", default=os.environ.get("LEGAIA_DISC_BIN"), help="the disc image (default: $LEGAIA_DISC_BIN)")
    ap.add_argument("--extracted", required=True, help="the extracted disc, holding PROT.DAT")
    ap.add_argument("--exe", required=True, help="the build under test")
    ap.add_argument("--base-exe", required=True, help="the build to pair it against")
    ap.add_argument("--site", required=True, help="the site under test, with its bundle")
    ap.add_argument("--base-site", required=True, help="the site to pair it against")
    ap.add_argument("--out", default=f"machine-bench-{platform.node() or 'box'}")
    ap.add_argument("--quick", action="store_true", help="one round of the vsync and t6 rows, native as usual")
    a = ap.parse_args()

    for label, value in (("--disc", a.disc), ("--exe", a.exe), ("--base-exe", a.base_exe), ("--site", a.site),
                         ("--base-site", a.base_site), ("--extracted", a.extracted)):
        if not value or not Path(value).exists():
            print(f"{label}: not found: {value}", file=sys.stderr)
            return 2
    if not (Path(a.extracted) / "PROT.DAT").exists():
        print(f"--extracted: no PROT.DAT in {a.extracted}", file=sys.stderr)
        return 2

    out = Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, LEGAIA_DISC_BIN=str(Path(a.disc).resolve()))
    (out / "machine.json").write_text(json.dumps(describe(), indent=1))
    extracted = str(Path(a.extracted).resolve())
    exe, base = str(Path(a.exe).resolve()), str(Path(a.base_exe).resolve())

    steps = [
        ("native", [sys.executable, str(HERE / "frame-bench.py"), "--exe", exe, "--base-exe", base,
                    "--extracted", extracted, "--repeat", "3", "--json", str(out / "native.json")]),
        ("entry", [sys.executable, str(HERE / "frame-bench.py"), "--entry", "--exe", exe, "--base-exe", base,
                   "--extracted", extracted, "--repeat", "5", "--json", str(out / "entry.json")]),
    ]
    rows = [r for r in WEB_ROWS if r[0] in ("vsync", "t6")] if a.quick else WEB_ROWS
    for name, flags in rows:
        for round_no in range(1, 2 if a.quick else 3):
            for side, site in (("base", a.base_site), ("tip", a.site)):
                tag = f"{name}-{side}-{round_no}"
                steps.append((tag, ["node", str(HERE / "frame-bench-web.mjs"), "--site", str(Path(site).resolve()),
                                    *flags, "--json", str(out / f"{tag}.json")]))

    failed = []
    with (out / "steps.log").open("w") as summary:
        for i, (tag, cmd) in enumerate(steps, 1):
            print(f"[{i}/{len(steps)}] {tag} ...", flush=True)
            start = time.monotonic()
            with (out / f"{tag}.log").open("wb") as log:
                try:
                    code = subprocess.run(cmd, cwd=HERE.parents[1], env=env, stdout=log, stderr=subprocess.STDOUT,
                                          timeout=3600).returncode
                except (OSError, subprocess.TimeoutExpired) as e:
                    log.write(f"\nmachine-bench: {e}\n".encode())
                    code = -1
            # A driver with nothing to run exits 0 and says [skip]: that is not a result.
            if code == 0 and b"[skip]" in (out / f"{tag}.log").read_bytes():
                code = -2
            line = f"{tag}: exit {code}, {time.monotonic() - start:.0f} s"
            print("  " + line, flush=True)
            summary.write(line + "\n")
            if code != 0:
                failed.append(tag)

    archive = shutil.make_archive(str(out), "zip", out.parent, out.name)
    print(f"\nSend this file back: {archive}")
    if failed:
        print("Failed, see their logs: " + ", ".join(failed))
        return 1
    print("All steps ran.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
