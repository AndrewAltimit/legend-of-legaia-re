#!/usr/bin/env python3
"""Port-catalog tracker: per-address status across decompilation and engine port.

This file is the entry point only. The catalog is the `port_catalog` package
beside it:

    port_catalog/__init__.py     what the status columns mean
    port_catalog/paths.py        the corpus, docs, crates and data-file paths
    port_catalog/citations.py    address grammars and the four collectors
    port_catalog/rust_source.py  one parsed Rust file, and the host roots
    port_catalog/rust_graph.py   the Rust call graph and reachability
    port_catalog/disclosure.py   the `NOT WIRED` / `WIRED:` / `REPLACED-BY:` grammar
    port_catalog/anchors.py      `// PORT:` tag -> the Rust item it sits on
    port_catalog/live.py         the `live` verdict per address
    port_catalog/features.py     feature views over `features.toml`
    port_catalog/rows.py         the catalog rows, the ignore list, row filters
    port_catalog/render/         one module per output page
    port_catalog/ratchet.py      `--check` / `--update-baseline`
    port_catalog/selftest.py     `--selftest`
    port_catalog/cli.py          command line

Data files: `features.toml`, `port-catalog-ignore.toml`,
`port-catalog-baseline.json`.

Other scripts load this file by path and read the catalog's functions off the
module, so every public name of the package is re-exported here. `paths` is
exposed as a module because `paths.FUNCS_DIR` is rebound at run time.

Usage:
    python3 scripts/ci/port-catalog.py                       # build global catalog
    python3 scripts/ci/port-catalog.py --missing-ports       # dumped+documented, not ported
    python3 scripts/ci/port-catalog.py --missing-dumps       # cited but not dumped
    python3 scripts/ci/port-catalog.py --md                  # markdown table to stdout
    python3 scripts/ci/port-catalog.py --top 30              # cap rows
    python3 scripts/ci/port-catalog.py --addr 801dd35c       # single-row drill-down
    python3 scripts/ci/port-catalog.py --dashboard           # open-work rollup (single page)

Output (default): target/port-catalog/catalog.csv + catalog.md
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from port_catalog import paths  # noqa: E402,F401  (path set just above)
from port_catalog.anchors import *  # noqa: E402,F401,F403
from port_catalog.citations import *  # noqa: E402,F401,F403
from port_catalog.cli import main  # noqa: E402
from port_catalog.disclosure import *  # noqa: E402,F401,F403
from port_catalog.features import *  # noqa: E402,F401,F403
from port_catalog.live import *  # noqa: E402,F401,F403
from port_catalog.paths import *  # noqa: E402,F401,F403
from port_catalog.ratchet import *  # noqa: E402,F401,F403
from port_catalog.render.catalog import *  # noqa: E402,F401,F403
from port_catalog.render.dashboard import *  # noqa: E402,F401,F403
from port_catalog.render.live_audit import *  # noqa: E402,F401,F403
from port_catalog.render.summary import *  # noqa: E402,F401,F403
from port_catalog.rows import *  # noqa: E402,F401,F403
from port_catalog.rust_graph import *  # noqa: E402,F401,F403
from port_catalog.rust_source import *  # noqa: E402,F401,F403
from port_catalog.selftest import *  # noqa: E402,F401,F403

if __name__ == "__main__":
    sys.exit(main())
