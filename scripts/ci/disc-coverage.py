#!/usr/bin/env python3
"""Disc-denominated coverage: how much of the game's own bytes we can explain.

This file is the entry point only. The gate is the `disc_coverage` package
beside it:

    disc_coverage/__init__.py     what the figures mean
    disc_coverage/paths.py        the corpus, disc, map, baseline and output paths
    disc_coverage/thresholds.py   classification thresholds and PROT class sets
    disc_coverage/dumps.py        the dump corpus as extents
    disc_coverage/attribution.py  which image a dump's bytes belong to
    disc_coverage/gaps.py         what an undumped run of an image is
    disc_coverage/images.py       coverage per code image, PROT recognition
    disc_coverage/worklist.py     the dump worklist
    disc_coverage/render.py       the markdown report
    disc_coverage/ratchet.py      `--check` against the baseline
    disc_coverage/cli.py          command line

Baseline: `disc-coverage-baseline.json`.

`update-progress-metrics.py` loads this file by path and reads the coverage
functions off the module, so every public name of the package is re-exported
here.

Usage:
    python3 scripts/ci/disc-coverage.py                 # report to stdout + target/
    python3 scripts/ci/disc-coverage.py --md            # markdown to stdout
    python3 scripts/ci/disc-coverage.py --check         # ratchet against the baseline
    python3 scripts/ci/disc-coverage.py --update-baseline
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from disc_coverage.attribution import *  # noqa: E402,F401,F403  (path set just above)
from disc_coverage.cli import main  # noqa: E402
from disc_coverage.dumps import *  # noqa: E402,F401,F403
from disc_coverage.gaps import *  # noqa: E402,F401,F403
from disc_coverage.images import *  # noqa: E402,F401,F403
from disc_coverage.paths import *  # noqa: E402,F401,F403
from disc_coverage.ratchet import *  # noqa: E402,F401,F403
from disc_coverage.render import *  # noqa: E402,F401,F403
from disc_coverage.thresholds import *  # noqa: E402,F401,F403
from disc_coverage.worklist import *  # noqa: E402,F401,F403

if __name__ == "__main__":
    sys.exit(main())
