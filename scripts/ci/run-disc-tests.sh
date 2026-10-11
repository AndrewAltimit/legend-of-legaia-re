#!/usr/bin/env bash
# Run the workspace tests against a real disc and count what ran.
#
# A disc-gated test skips and passes when its input is missing, so "the suite
# is green" and "the suite ran" are different claims. This runs the suite with
# every test's captured output kept, then hands the log to disc-test-tally.py,
# which fails the run when a test skipped for want of the disc and reports how
# many skipped on another input (extracted/, the save library, a card file).
#
# Usage (LEGAIA_DISC_BIN must name the image):
#     scripts/ci/run-disc-tests.sh [extra `cargo test` arguments]
#
# LEGAIA_DISC_TEST_LOG       where the full log goes (default: under the
#                            target directory). It holds test output derived
#                            from the disc, so it is never uploaded.
# LEGAIA_MAX_OTHER_SKIPS     fail when more tests than this skip on an input
#                            other than the disc. Unset: reported, not bounded.

set -euo pipefail

if [[ -z "${LEGAIA_DISC_BIN:-}" ]]; then
    echo "run-disc-tests: LEGAIA_DISC_BIN is not set" >&2
    exit 1
fi
if [[ ! -f "${LEGAIA_DISC_BIN}" ]]; then
    echo "run-disc-tests: no disc image at '${LEGAIA_DISC_BIN}'" >&2
    exit 1
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

LOG="${LEGAIA_DISC_TEST_LOG:-${CARGO_TARGET_DIR:-target}/disc-test.log}"
mkdir -p "$(dirname "${LOG}")"

# The test run's status is read from the command itself, never from a pipe.
status=0
cargo test --workspace --profile release-test --no-fail-fast "$@" \
    -- --show-output >"${LOG}" 2>&1 || status=$?

# The log carries every passing test's output; show what a reader needs.
grep -E '^(test result:|test .* \.\.\. FAILED|error(\[|:)|failures:$|    [a-zA-Z0-9_:]+$)' "${LOG}" || true

tally_args=("${LOG}" --require-disc)
if [[ -n "${LEGAIA_MAX_OTHER_SKIPS:-}" ]]; then
    tally_args+=(--max-other-skips "${LEGAIA_MAX_OTHER_SKIPS}")
fi
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    tally_args+=(--summary "${GITHUB_STEP_SUMMARY}")
fi
tally=0
python3 scripts/ci/disc-test-tally.py "${tally_args[@]}" || tally=$?

if [[ "${status}" -ne 0 ]]; then
    echo "run-disc-tests: cargo test exited ${status}; full log at ${LOG}" >&2
    exit "${status}"
fi
exit "${tally}"
