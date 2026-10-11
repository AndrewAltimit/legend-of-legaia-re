#!/usr/bin/env python3
"""Provenance checker for `// PORT: FUN_<addr>` tags.

This file is the entry point only. The sweep is the `port_provenance` package
beside it:

    port_provenance/__init__.py          what each signal claims
    port_provenance/paths.py             crates, the two corpora, the waiver file
    port_provenance/rows.py              `SPLIT_MODULES`, `DEFINING_DOC_ROOTS`
                                         and the accepted-exception lists
    port_provenance/path_check.py        do the paths rows.py names exist?
    port_provenance/thresholds.py        the numbers each signal is tuned by
    port_provenance/tags.py              `// PORT:` tags and their module
    port_provenance/dumps.py             the dump corpus
    port_provenance/dump_features.py     what a dump is evidence of
    port_provenance/call_sites.py        the extracted images, call-site tests
    port_provenance/citation_support.py  cited addresses no dump carries
    port_provenance/findings.py          the finding record
    port_provenance/signals/             one module per signal
    port_provenance/waivers.py           the waiver file and the live set
    port_provenance/cli.py               command line and report

Reviewed false positives go in `port-provenance-waivers.toml`.

Usage:

    python3 scripts/ci/check-port-provenance.py              # ranked report
    python3 scripts/ci/check-port-provenance.py --live-only  # live ports only
    python3 scripts/ci/check-port-provenance.py --addr 801d5de0
    python3 scripts/ci/check-port-provenance.py --signal dual-label
    python3 scripts/ci/check-port-provenance.py --strict     # exit 1 on unwaived
    python3 scripts/ci/check-port-provenance.py --emit-waivers
    python3 scripts/ci/check-port-provenance.py --funcs DIR --extracted DIR
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from port_provenance.cli import main  # noqa: E402  (path set just above)

if __name__ == "__main__":
    sys.exit(main())
