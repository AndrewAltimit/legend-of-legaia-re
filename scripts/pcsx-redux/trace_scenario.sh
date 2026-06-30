#!/usr/bin/env bash
#
# trace_scenario.sh - run the gap-set trace as a UNION of windowed passes
# against ONE catalogued checkpoint (--scenario label), then merge the per-
# window CSVs into a single hit table for that segment.
#
# Each pass arms <= ~120 exec breakpoints (the headless BP-count ceiling; see
# docs/tooling/playthrough-coverage.md) over one contiguous address window of
# the sorted gap-set, resumes the checkpoint, drives input, and records which
# gap-set functions executed. The windows tile the whole 762-entry gap-set
# (2 SCUS + 5 overlay). A resumed save aborts a few hundred vsyncs in on this
# build, but the harness flushes the CSV every ~60 vsyncs so the hits survive.
#
# Usage:
#   bash scripts/pcsx-redux/trace_scenario.sh <scenario-label> [mash-spec]
#     scenario-label : e.g. s1_newgame_field / s2_rimelm_town01
#     mash-spec      : LEGAIA_MASH value (default CROSS:40 = advance dialogue)
#
# Output: captures/trace/<label>/{win_*.csv, union.csv}
set -uo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

LABEL="${1:?usage: trace_scenario.sh <scenario-label> [mash-spec]}"
MASH="${2:-CROSS:40}"
FRAMES="${LEGAIA_FRAMES:-700}"
OUTDIR="captures/trace/$LABEL"
mkdir -p "$OUTDIR"

# Address windows tiling the sorted gap-set, each <= ~120 entries. HI is
# exclusive. The trailing 0x8020D05C singleton is folded into O5.
WINDOWS=(
  "scus1 0x80016E4C 0x8005B679"
  "scus2 0x8005BA38 0x80076580"
  "ov1   0x801C0164 0x801D0751"
  "ov2   0x801D079C 0x801D6575"
  "ov3   0x801D657C 0x801DD095"
  "ov4   0x801DD0BC 0x801EE095"
  "ov5   0x801EE5B0 0x8020D060"
)

run_window() {
  local tag="$1" lo="$2" hi="$3" csv="$OUTDIR/win_${tag}.csv"
  local try
  for try in 1 2 3; do
    rm -f "$csv"
    LEGAIA_BOOT_DELAY=2 \
    LEGAIA_ADDR_LO="$lo" LEGAIA_ADDR_HI="$hi" \
    LEGAIA_MASH="$MASH" \
    LEGAIA_LUA=scripts/pcsx-redux/autorun_trace_segment.lua \
    LEGAIA_OUT="$csv" \
    LEGAIA_FRAMES="$FRAMES" \
      xvfb-run -a timeout --kill-after=15s 210s \
      bash scripts/pcsx-redux/run_probe.sh --scenario "$LABEL" >/dev/null 2>&1
    # Success = CSV exists with at least one data row (armed + captured).
    if [[ -f "$csv" && $(wc -l <"$csv") -gt 1 ]]; then
      echo "  [$tag] $(($(wc -l <"$csv")-1)) hits (try $try)"
      return 0
    fi
    echo "  [$tag] empty (try $try) - retrying"
  done
  echo "  [$tag] FAILED after 3 tries"
  return 1
}

echo "=== trace_scenario $LABEL (mash=$MASH frames=$FRAMES) ==="
for w in "${WINDOWS[@]}"; do
  read -r tag lo hi <<<"$w"
  run_window "$tag" "$lo" "$hi"
done

# Union all window CSVs (each function appears in exactly one window, so a
# plain concatenation of data rows is the union; no dedup needed).
python3 - "$OUTDIR" <<'PY'
import sys, glob, os, csv
outdir = sys.argv[1]
rows = []
for f in sorted(glob.glob(os.path.join(outdir, "win_*.csv"))):
    with open(f) as fh:
        r = csv.reader(fh)
        next(r, None)
        for row in r:
            if row:
                rows.append(row)
rows.sort(key=lambda x: int(x[0], 16))
with open(os.path.join(outdir, "union.csv"), "w", newline="") as fh:
    w = csv.writer(fh)
    w.writerow(["addr", "hits", "first_frame", "first_mode", "first_ra", "stem"])
    w.writerows(rows)
print(f"union.csv: {len(rows)} gap-set functions hit across {outdir}")
PY
