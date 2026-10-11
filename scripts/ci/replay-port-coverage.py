#!/usr/bin/env python3
"""Join a replay's runtime coverage against the port catalog.

This file is the entry point only. The report is the `replay_port_coverage`
package beside it:

    replay_port_coverage/__init__.py    what the join measures, and the
                                        export recipe
    replay_port_coverage/paths.py       where exports are read and the page
                                        is written
    replay_port_coverage/ladders.py     `CANONICAL_LADDERS`: every
                                        `(test, package)` the report unions
    replay_port_coverage/ladder_check.py  does each ladder name a test on disk?
    replay_port_coverage/inputs.py      which exports are on disk
    replay_port_coverage/catalog.py     the port catalog as this report reads it
    replay_port_coverage/coverage.py    llvm-cov exports and the anchor join
    replay_port_coverage/page_audit.py  `--page-audit`
    replay_port_coverage/selftest.py    `--selftest`
    replay_port_coverage/cli.py         command line and report
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from replay_port_coverage.cli import main  # noqa: E402  (path set just above)

if __name__ == "__main__":
    raise SystemExit(main())
