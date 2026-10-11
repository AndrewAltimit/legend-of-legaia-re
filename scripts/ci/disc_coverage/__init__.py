"""Disc-denominated coverage: how much of the game's own bytes we can explain.

`port-catalog.py` measures three status columns over the set of addresses this
project has *cited*. That is the right tool for tracking work, but its
denominator is our own documentation, so it can never say how much of the game
is left. A page can read "99.9% covered" while an entire un-dumped subsystem
sits outside the citation graph, because nothing cites it.

This script takes the denominator from the disc instead.

Two measurements, and they are NOT the same kind of number - the report says so
in its own text, because quoting them interchangeably is the obvious way to
misuse this page:

  CODE  - byte-exact. A Ghidra dump header states the body's entry and size, so
          the dumped functions are real byte intervals. Merge them, subtract
          from an image's extent, and every remaining byte is genuinely
          un-dumped. Gaps are then classified code-vs-data so the rodata an
          executable carries inside its text segment does not inflate the
          denominator. The header is parsed by `dump_header`, shared with the
          attribution sweep - the corpus spells every header field more than
          one way and a private regex silently under-counts.

  DATA  - format RECOGNITION, one level coarser. `asset categorize` says which
          format class each PROT entry is. Knowing an entry is a
          `scene_vab_stream` is not the same as accounting for every byte
          inside it, and no parser currently reports consumed-vs-unconsumed
          bytes. Treat the data figure as an upper bound.

Overlay images alias in VA space, so an address alone cannot say which of them a
dump belongs to. `scripts/ghidra-analysis/dump-extent-attribution.csv` carries
the per-extent verdict the *bytes* give, and this script applies it: an extent
the bytes place in another image (or in none) leaves the row rather than
inflating it. That file is committed and keyed by `(entry, bytes)`, so it does
not rot when a dump lands. It is optional - without it every overlay extent
stays ambiguous by address, which is the same honest upper bound, just looser.

Disc-gated, like the rest of the repo: with no `extracted/` tree and no dump
corpus this exits 0 and reports SKIPPED, so CI passes without disc data. Both
inputs are gitignored, so this only produces numbers on a developer's machine.

The DATA half reads a *cache* (`extracted/PROT/categorize.json`) that nothing
here regenerates. A tree whose `categorize` detectors have moved on will report
the old classification through a passing gate. Re-run `asset categorize
extracted/PROT` before trusting a data figure or taking a baseline.

No Sony bytes are emitted - the report carries addresses, byte counts and class
names, the same things the committed docs already carry.

Usage:
    python3 scripts/ci/disc-coverage.py                 # report to stdout + target/
    python3 scripts/ci/disc-coverage.py --md            # markdown to stdout
    python3 scripts/ci/disc-coverage.py --check         # ratchet against the baseline
    python3 scripts/ci/disc-coverage.py --update-baseline
"""

import os
import sys

from disc_coverage.paths import REPO

# The dump header has one parser, shared with the attribution sweep. Each
# instrument over this corpus used to carry its own regex, and because the
# corpus spells every header field more than one way, each silently rejected a
# different subset of real dumps as "no parseable header". See
# `scripts/ghidra-analysis/dump_header.py`.
sys.path.insert(0, os.path.join(REPO, "scripts", "ghidra-analysis"))
