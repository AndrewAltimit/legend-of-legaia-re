#!/usr/bin/env bash
# Export one `target/cov-<test>.json` per canonical ladder, for
# `replay-port-coverage.py` to join. This is the recipe from
# `replay_port_coverage/__init__.py` as a runnable file, so the recipe and the
# ladder list cannot drift apart.
#
#   scripts/ci/export-replay-coverage.sh              # every ladder
#   scripts/ci/export-replay-coverage.sh menu_replay  # only the named ladders
#
# Instrumented, never `--release` (a release export loses executed code). The
# two disc-heavy ladders run last so the short ones land first. A ladder that
# fails is logged in `target/cov-export.log` and skipped; the join names any
# export that is missing, so a partial run is visible rather than silent.
set -u

cd "$(dirname "$0")/../.."

LOG=target/cov-export.log
TAIL_LADDERS="full_game_ladder chapter1_frontier_ladder"

mkdir -p target
: >"$LOG"

ladders=$(python3 scripts/ci/replay-port-coverage.py --list-ladders)
if [ -z "$ladders" ]; then
    echo "no ladders listed" >&2
    exit 1
fi

is_tail() {
    case " $TAIL_LADDERS " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
    esac
}

wanted() {
    [ "$#" -eq 1 ] && return 0
    local t=$1
    shift
    case " $* " in
    *" $t "*) return 0 ;;
    *) return 1 ;;
    esac
}

export_one() {
    local t=$1 pkg=$2
    echo "== $t ($pkg) $(date -Is)" >>"$LOG"
    cargo llvm-cov clean --profraw-only >>"$LOG" 2>&1
    if ! cargo llvm-cov -p "$pkg" --test integration --no-report -- "${t}::" >>"$LOG" 2>&1; then
        echo "FAILED $t" >>"$LOG"
        return 1
    fi
    if ! cargo llvm-cov report --json --output-path "target/cov-$t.json" >>"$LOG" 2>&1; then
        echo "FAILED-REPORT $t" >>"$LOG"
        return 1
    fi
    echo "OK $t" >>"$LOG"
}

failed=0
for pass in head tail; do
    while read -r t pkg; do
        [ -n "$t" ] || continue
        wanted "$t" "$@" || continue
        if is_tail "$t"; then
            [ "$pass" = tail ] || continue
        else
            [ "$pass" = head ] || continue
        fi
        export_one "$t" "$pkg" || failed=$((failed + 1))
    done <<<"$ladders"
done

echo "done, $failed failed; log in $LOG"
[ "$failed" -eq 0 ]
