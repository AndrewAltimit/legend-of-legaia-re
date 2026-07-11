#!/usr/bin/env bash
#
# One-command regression self-test for the community poll rig.
#
#   bash scripts/pcsx-redux/run_state_poll_selftest.sh
#
# Resolves a field scenario (phase A) + a battle scenario (phase B) from
# scripts/scenarios.toml, launches autorun_state_poll_selftest.lua (which
# dofile()s the REAL autorun_state_poll.lua and pokes every watched cell),
# waits for the wrapper's completion marker in the emulator log, kills the
# session, then runs check_state_poll_selftest.py over the run dir.
#
# Exit code: the checker's (0 = every stream + autosnap fired).
#
# Knobs:
#   LEGAIA_SELFTEST_FIELD_SCENARIO   default s3_rimelm_freeroam
#   LEGAIA_SELFTEST_BATTLE_SCENARIO  default party_basic_attack_vs_gobu_gobu
#                                    (must resolve to a PCSX-native .sstate)
#   LEGAIA_SELFTEST_TIMEOUT          hard cap in seconds (default 360)
#
# Run this after ANY edit to autorun_state_poll.lua / lib/probe/* before
# handing the rig to volunteers - a silently broken stream costs a whole
# volunteer playthrough.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

FIELD_SCENARIO="${LEGAIA_SELFTEST_FIELD_SCENARIO:-s3_rimelm_freeroam}"
BATTLE_SCENARIO="${LEGAIA_SELFTEST_BATTLE_SCENARIO:-party_basic_attack_vs_gobu_gobu}"
HARD_TIMEOUT="${LEGAIA_SELFTEST_TIMEOUT:-360}"
MARKER="STATE_POLL_SELFTEST COMPLETE"

resolve_scenario() {
    python3 - "$REPO_ROOT/scripts/scenarios.toml" "$1" "$REPO_ROOT" <<'PY'
import os, sys, glob, tomllib
manifest_path, label, repo_root = sys.argv[1], sys.argv[2], sys.argv[3]
with open(manifest_path, "rb") as f:
    data = tomllib.load(f)
for s in data.get("scenarios", []):
    if s.get("label") == label:
        fp = s.get("backup_fingerprint")
        if fp:
            hits = sorted(glob.glob(os.path.join(
                repo_root, "saves", "library", "pcsx-redux", fp + "*")))
            if hits:
                print(hits[0]); sys.exit(0)
        v = s.get("pcsx_redux_sstate")
        if v:
            print(os.path.expanduser(os.path.expandvars(v))); sys.exit(0)
sys.exit(2)
PY
}

BATTLE_SSTATE="$(resolve_scenario "$BATTLE_SCENARIO")" || {
    echo "ERROR: battle scenario '$BATTLE_SCENARIO' not resolvable" >&2
    exit 65
}
echo "field scenario  : $FIELD_SCENARIO"
echo "battle scenario : $BATTLE_SCENARIO -> $BATTLE_SSTATE"

# keep the target-flag snap test off real gates
export LEGAIA_SELFTEST_BATTLE_SSTATE="$BATTLE_SSTATE"
export LEGAIA_SNAP_FLAGS=4001

PGID=""
cleanup() {
    [[ -n "$PGID" ]] || return 0
    kill -TERM -- -"$PGID" 2>/dev/null || true
    sleep 2
    kill -KILL -- -"$PGID" 2>/dev/null || true
    PGID=""
}
trap cleanup EXIT

# One attempt = launch (own process group, so the emulator tree can be torn
# down once the marker appears - the wrapper never self-quits, per the
# poll-probe family convention) + poll the log for the completion marker.
# Returns 0 iff the marker was seen.
run_once() {
    local out_dir="$1" log="$1/pcsx.log"
    setsid bash scripts/pcsx-redux/run_probe.sh --fast \
        --scenario "$FIELD_SCENARIO" \
        --lua scripts/pcsx-redux/autorun_state_poll_selftest.lua \
        --out-dir "$out_dir" \
        --log "$log" &
    PGID=$!

    echo "waiting for '$MARKER' in $log (cap ${HARD_TIMEOUT}s)..."
    local elapsed=0 dead_checks=0 seen=1
    while [[ $elapsed -lt $HARD_TIMEOUT ]]; do
        if [[ -f "$log" ]] && grep -q "$MARKER" "$log"; then
            echo "marker seen after ${elapsed}s - tearing down emulator"
            seen=0
            break
        fi
        # Liveness by group membership. Grace period covers boot; two
        # consecutive empty checks so a transient can't tear the run down.
        if [[ $elapsed -ge 20 ]] && ! pgrep -g "$PGID" >/dev/null 2>&1; then
            dead_checks=$((dead_checks + 1))
            if [[ $dead_checks -ge 2 ]]; then
                echo "WARNING: emulator group $PGID empty before the marker (crash?)" >&2
                tail -5 "$log" >&2 || true
                break
            fi
        else
            dead_checks=0
        fi
        sleep 5
        elapsed=$((elapsed + 5))
    done
    [[ $elapsed -ge $HARD_TIMEOUT ]] \
        && echo "ERROR: hit ${HARD_TIMEOUT}s cap without the completion marker" >&2
    cleanup
    return $seen
}

# The emulator has a KNOWN sporadic cold-boot crash; one retry absorbs it so
# a flaky boot doesn't read as a broken probe.
OUT_DIR=""
for attempt in 1 2; do
    RUN_TS="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
    OUT_DIR="$REPO_ROOT/captures/state_poll_selftest/$RUN_TS"
    mkdir -p "$OUT_DIR"
    if run_once "$OUT_DIR"; then
        break
    fi
    echo "attempt $attempt did not complete$( [[ $attempt -lt 2 ]] && echo ' - retrying (cold-boot crash is a known sporadic)' )" >&2
done
trap - EXIT

echo ""
python3 scripts/pcsx-redux/check_state_poll_selftest.py "$OUT_DIR"
