# Ratchet integrity

A ratchet compares a measurement against a committed baseline and fails when
the measurement gets worse. Two things undo it without any ratchet noticing:
the baseline itself is edited, or the measurement never runs. Each has a gate,
and so does the committed build input the site renders, which goes stale the
same invisible way.

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

The retail-compare baseline is one ratchet filed in two lists: `states`, the
retail-disc states, and `patched_states`, the mod states the manifest tags
with a `resident_patch` ([`retail-compare.md`](retail-compare.md#the-ratchet)).
The gate reads a `patched_states/<state>/<channel>` leaf under its
`states/...` key (`FOLD` in the script), so moving a row between the lists is
no loosening and a fall on either list needs its ledger entry.

## A build input may not go stale

The site builds without disc data, so the landing page renders figures
committed beside the code: `scripts/ci/progress-metrics.json` (the tiles) and
`scripts/ci/image-port-status.json` (the per-image port table). They are not
ratchets - a number may move either way - but they describe one tree, and a
stale one is invisible: well-formed JSON with plausible numbers.

`python3 scripts/ci/update-progress-metrics.py --check` fails when a refresh
would change either file. What needs no corpus is always compared: the
verification block, the port track's counts against the `// PORT:` tags and
`port-catalog-baseline.json`, and the per-image table's input digests. With
the dump corpus and the extracted disc present it also recomputes the corpus
tracks and, without `--skip-catalog`, the catalog tracks and the per-image
table. The pre-commit hook and the `gates` job run `--check --skip-catalog`.
Clearing a failure means refreshing on a machine with the disc
(`update-progress-metrics.py`, no flags) and committing both files; the check
never rewrites them. Details:
[`disc-coverage.md`](disc-coverage.md#refreshing-the-landing-page-tiles).

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
