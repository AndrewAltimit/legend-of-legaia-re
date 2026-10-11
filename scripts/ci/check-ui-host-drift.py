#!/usr/bin/env python3
"""UI host-drift checker: does every shared screen reach BOTH hosts?

This file is the entry point only. The gate is the `ui_host_drift` package
beside it:

    ui_host_drift/__init__.py   what each tier claims and what it cannot see
                                (the text `--help` prints)
    ui_host_drift/roots.py      the repository, crate and host source roots
    ui_host_drift/rows/         the declared rows, one module per tier;
                                `host_files.py` holds every host source path
    ui_host_drift/tiers/        the analysis, one module per tier
    ui_host_drift/path_check.py do the paths the rows name exist?
    ui_host_drift/rust_text.py  the source-text helpers the tiers share
    ui_host_drift/controls.py   the detector controls
    ui_host_drift/selftest.py   `--selftest`
    ui_host_drift/report.py     command line and report

Waivers live in `ui-host-drift-waivers.toml`.

Usage:

    python3 scripts/ci/check-ui-host-drift.py            # check, exit 1 on drift
    python3 scripts/ci/check-ui-host-drift.py --quiet    # findings only
    python3 scripts/ci/check-ui-host-drift.py --list     # full surface table
    python3 scripts/ci/check-ui-host-drift.py --selftest # detector control suite

Exit status: 0 = clean, 1 = drift / stale waiver / constant mismatch /
sim-pair mismatch, 2 = self-test failed.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ui_host_drift.report import main  # noqa: E402  (path set just above)

if __name__ == "__main__":
    sys.exit(main())
