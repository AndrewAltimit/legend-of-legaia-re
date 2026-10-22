# Ratchet integrity

A ratchet compares a measurement against a committed baseline and fails when
the measurement gets worse. Two things undo it without any ratchet noticing:
the baseline itself is edited, or the measurement never runs. Each has a gate.

## A baseline may not loosen silently

Every ratchet accepts a new baseline in the same commit as the code that moved
it (`--update-baseline`, `--bless`). Most of them read inputs a runner does not
have - the disc, the dump corpus, the save library - so in CI they report
`SKIPPED` and exit 0. What is left on a runner is the baseline file, which is
plain text.

[`scripts/ci/check-baseline-loosening.py`](../../scripts/ci/check-baseline-loosening.py)
reads each baseline at a base revision and in the working tree and walks the
numeric leaves. A leaf that moved the wrong way is a **loosening**:

| Rule | Loosening |
|---|---|
| `up` | the value fell (a score, a coverage percentage, a ladder tier) |
| `down` | the value rose (a worklist, a count of unparsed bytes) |
| `pin` | the value changed at all (a setting of the measurement) |
| `set` | an entry was added to a list of known-bad items |
| any | a baselined key, or the whole file, disappeared |

The `RULES` table in the script says which rule each key takes. A numeric key
that no rule covers is itself a finding, so a new baselined figure cannot
arrive unclassified.

A loosening fails unless
[`scripts/ci/baseline-loosenings.toml`](../../scripts/ci/baseline-loosenings.toml)
records that exact `from -> to` step with a reason:

```bash
scripts/ci/check-baseline-loosening.py --propose >> scripts/ci/baseline-loosenings.toml
# then write each `reason`
```

An entry matches one transition only, so it cannot excuse a later fall, and it
is never removed. The file is the history of what the ratchets gave back -
which a squash-merged log otherwise buries inside a wave commit.

The pre-commit hook compares the working tree against the merge-base with
`main`. The `gates` job compares a pull request's merge commit against the PR
base, and a push against the commit it replaced. It needs no disc and no build.

## A skipped test is not a passed test

A disc-gated test that finds its input missing prints a skip line and returns,
and libtest reports `ok`. On a machine with no disc the whole gated suite is
green; on a machine with the disc, a test whose second input is gone
(`extracted/`, the save library, a memory-card file) is green too.

[`scripts/ci/run-disc-tests.sh`](../../scripts/ci/run-disc-tests.sh) runs the
workspace tests with every test's captured output kept, and
[`scripts/ci/disc-test-tally.py`](../../scripts/ci/disc-test-tally.py) reads
the log:

- a test that skipped **for want of the disc** fails the run - with the image
  present, such a line means a gate that is reading the wrong thing;
- a test that skipped **on another input** is counted, grouped by the line it
  printed, and written to the job summary. `LEGAIA_MAX_OTHER_SKIPS` turns the
  count into a bound.

The tally depends on the skip line. A test that returns early without printing
one is invisible to it, so a gated test prints `[skip] <what was missing>`
before it returns, and names *every* input it lacked: a test that needs
`extracted/PROT.DAT` says so, or a fresh runner checkout (which has the disc but
no extraction) reads as a disc skip. Only `LEGAIA_DISC_BIN` itself is the disc;
another disc's variable (`LEGAIA_DISC_BIN_JP`, `LEGAIA_FANPATCH_BASE_BIN`) is
another input.

The suite runs through this script in two places: the `disc-test` job of
`main-ci.yml` (manual dispatch, or a `disc-test` label on a pull request) and
`disc-nightly.yml`, which runs it on a schedule and skips a night when `main`
has not moved since the last run that passed. The log holds output derived
from the disc and stays on the runner.
