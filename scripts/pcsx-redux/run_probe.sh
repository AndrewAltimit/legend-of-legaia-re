#!/usr/bin/env bash
#
# Canonical PCSX-Redux probe runner. Used by every autorun_*.lua probe;
# replaces the older run_world_map_probe.sh / run_fast_probe.sh /
# run_dump_slot4.sh wrappers (their behaviour is folded into flags).
#
# Launches the locally-built PCSX-Redux with the Legaia disc + BIOS,
# runs an autorun Lua script that loads a save state / arms BPs /
# captures hits / writes outputs / quits, then surfaces the probe-hits
# summary from the log.
#
# Usage:
#   bash scripts/pcsx-redux/run_probe.sh                          # default lua
#   bash scripts/pcsx-redux/run_probe.sh --lua <path>             # pick the probe
#   bash scripts/pcsx-redux/run_probe.sh --fast --lua <path>      # recompiler mode
#   LEGAIA_LUA=... LEGAIA_OUT=... bash scripts/pcsx-redux/run_probe.sh
#
# Flags (each one also accepts the corresponding LEGAIA_* env var):
#   --lua PATH           autorun lua to -dofile (default world_map probe)
#   --spec PATH          declarative .probe.toml spec to run via
#                        probes/_runner.lua (sets LEGAIA_PROBE_SPEC and
#                        forces --lua to probes/_runner.lua)
#   --sstate PATH        save state path (default sstate1)
#   --scenario NAME      named scenario from scripts/scenarios.toml
#                        (resolved to a per-emulator save-state path;
#                        overrides --sstate)
#   --out PATH           probe CSV / output path
#   --frames N           post-load capture vsyncs (default 600)
#   --bios PATH          PSX BIOS (default ~/.mednafen/firmware/SCPH1001.BIN)
#   --iso PATH           disc image (default ~/Downloads/...)
#   --pcsx PATH          pcsx-redux binary (default ~/Tools/pcsx-redux/pcsx-redux)
#   --fast               drop -interpreter -debugger (recompiler ~10-50x
#                        faster; Lua BPs do NOT fire in this mode — use
#                        only for vsync-event-only probes)
#   --log PATH           emulator log path (default logs/pcsx_probe_<stem>.log)
#   --help               print this header and exit
#
# Why -interpreter -debugger by default:
#   psxinterpreter.cc:1652 — Lua BPs only fire when both are set. The
#   interpreter is required for the debug-process hook, and DebugSettings::Debug
#   gates the hook itself. --fast skips both for probes that don't arm BPs.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# ---------- defaults (overridable via env or flag) ----------
PCSX_REDUX="${PCSX_REDUX:-$HOME/Tools/pcsx-redux/pcsx-redux}"
LEGAIA_ISO="${LEGAIA_ISO:-$HOME/Downloads/Legend of Legaia (USA)/Legend of Legaia (USA).bin}"
LEGAIA_SSTATE="${LEGAIA_SSTATE:-$HOME/Tools/pcsx-redux/SCUS94254.sstate1}"
LEGAIA_BIOS="${LEGAIA_BIOS:-$HOME/.mednafen/firmware/SCPH1001.BIN}"
LEGAIA_FRAMES="${LEGAIA_FRAMES:-600}"
LEGAIA_OUT="${LEGAIA_OUT:-}"
LEGAIA_OUT_DIR="${LEGAIA_OUT_DIR:-}"
LEGAIA_LUA="${LEGAIA_LUA:-scripts/pcsx-redux/autorun_world_map_probe.lua}"
LEGAIA_SCENARIO="${LEGAIA_SCENARIO:-}"
LEGAIA_PROBE_SPEC="${LEGAIA_PROBE_SPEC:-}"
LOG_FILE=""
FAST=0

# ---------- flag parsing ----------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --lua)       LEGAIA_LUA="$2"; shift 2 ;;
        --spec)      LEGAIA_PROBE_SPEC="$2"; shift 2 ;;
        --sstate)    LEGAIA_SSTATE="$2"; shift 2 ;;
        --scenario)  LEGAIA_SCENARIO="$2"; shift 2 ;;
        --out)       LEGAIA_OUT="$2"; shift 2 ;;
        --out-dir)   LEGAIA_OUT_DIR="$2"; shift 2 ;;
        --frames)    LEGAIA_FRAMES="$2"; shift 2 ;;
        --bios)      LEGAIA_BIOS="$2"; shift 2 ;;
        --iso)       LEGAIA_ISO="$2"; shift 2 ;;
        --pcsx)      PCSX_REDUX="$2"; shift 2 ;;
        --log)       LOG_FILE="$2"; shift 2 ;;
        --fast)      FAST=1; shift ;;
        -h|--help)
            sed -n '2,38p' "$0"
            exit 0 ;;
        *)
            echo "ERROR: unknown flag: $1" >&2
            echo "Run 'bash $0 --help' for usage." >&2
            exit 64 ;;
    esac
done

# ---------- scenario resolution (LEGAIA_SCENARIO -> per-emulator path) ----------
# Probes that don't need a save state set LEGAIA_NO_SSTATE=1 (e.g. cold-boot
# captures). Probes can also pass --scenario / LEGAIA_SCENARIO to pick a
# named state from scripts/scenarios.toml. We shell out to python3 +
# stdlib tomllib instead of grepping/awking the TOML, because [[scenarios]]
# blocks have nested subtables (e.g. [scenarios.overlay_slice]) that
# string-pattern matching gets wrong.
if [[ -n "$LEGAIA_SCENARIO" ]]; then
    MANIFEST="$REPO_ROOT/scripts/scenarios.toml"
    if [[ ! -f "$MANIFEST" ]]; then
        echo "ERROR: --scenario passed but $MANIFEST does not exist" >&2
        exit 65
    fi
    if ! command -v python3 >/dev/null 2>&1; then
        echo "ERROR: --scenario requires python3 (for tomllib)" >&2
        exit 65
    fi
    # Resolution order: an immutable library backup (backup_fingerprint ->
    # saves/library/pcsx-redux/<fp>.*) is preferred over the wipe-prone live
    # pcsx_redux_sstate path, so probes don't break when a slot is overwritten.
    resolved="$(python3 - "$MANIFEST" "$LEGAIA_SCENARIO" "$REPO_ROOT" <<'PY'
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
                print(hits[0])
                sys.exit(0)
        v = s.get("pcsx_redux_sstate")
        if v:
            print(os.path.expanduser(os.path.expandvars(v)))
        sys.exit(0)
sys.exit(2)
PY
    )" || {
        echo "ERROR: scenario '$LEGAIA_SCENARIO' not found in $MANIFEST" >&2
        exit 65
    }
    if [[ -z "$resolved" ]]; then
        echo "ERROR: scenario '$LEGAIA_SCENARIO' has neither backup_fingerprint nor pcsx_redux_sstate in $MANIFEST" >&2
        exit 65
    fi
    LEGAIA_SSTATE="$resolved"
fi

# ---------- declarative spec dispatch (--spec / LEGAIA_PROBE_SPEC) ----------
# If a .probe.toml spec was passed, force the autorun to be probes/_runner.lua
# so it'll pick the spec up via $LEGAIA_PROBE_SPEC. The spec path is
# preserved verbatim; the runner does the TOML parse + dispatch into the
# probe lib.
if [[ -n "$LEGAIA_PROBE_SPEC" ]]; then
    if [[ ! -e "$LEGAIA_PROBE_SPEC" ]]; then
        echo "ERROR: --spec file not found: $LEGAIA_PROBE_SPEC" >&2
        exit 65
    fi
    LEGAIA_LUA="scripts/pcsx-redux/probes/_runner.lua"
fi

# ---------- preflight ----------
required=("$PCSX_REDUX" "$LEGAIA_ISO" "$LEGAIA_BIOS")
if [[ "${LEGAIA_NO_SSTATE:-0}" != "1" ]]; then
    required+=("$LEGAIA_SSTATE")
fi
for f in "${required[@]}"; do
    if [[ ! -e "$f" ]]; then
        echo "ERROR: required file not found: $f" >&2
        exit 1
    fi
done

if [[ ! -e "$LEGAIA_LUA" ]]; then
    echo "ERROR: lua probe not found: $LEGAIA_LUA" >&2
    exit 1
fi

# Derive stem from the lua probe basename for downstream defaults.
# When --spec is used, the lua is _runner.lua for every spec — derive the
# stem from the spec basename so each spec's captures land in its own
# captures/<spec-stem>/ subtree instead of captures/_runner/.
if [[ -n "$LEGAIA_PROBE_SPEC" ]]; then
    stem="$(basename "$LEGAIA_PROBE_SPEC" .probe.toml)"
else
    stem="$(basename "$LEGAIA_LUA" .lua)"
    stem="${stem#autorun_}"
fi

# Default LEGAIA_OUT_DIR to captures/<stem>/<iso-ts>/ if neither
# LEGAIA_OUT nor LEGAIA_OUT_DIR was supplied. This is what gives every
# probe a per-run subtree without each probe knowing the policy.
if [[ -z "$LEGAIA_OUT" && -z "$LEGAIA_OUT_DIR" ]]; then
    run_ts="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
    LEGAIA_OUT_DIR="$REPO_ROOT/captures/$stem/$run_ts"
fi
if [[ -n "$LEGAIA_OUT_DIR" ]]; then
    mkdir -p "$LEGAIA_OUT_DIR"
fi

# Derive a log filename from the same stem if none was given. Drop it
# alongside the captures (one log per run) instead of a flat logs/ dir.
if [[ -z "$LOG_FILE" ]]; then
    if [[ -n "$LEGAIA_OUT_DIR" ]]; then
        LOG_FILE="$LEGAIA_OUT_DIR/pcsx.log"
    else
        LOG_FILE="$REPO_ROOT/logs/pcsx_${stem}.log"
    fi
fi
mkdir -p "$(dirname "$LOG_FILE")"

cd "$REPO_ROOT"

# ---------- banner ----------
{
    echo "=== run_probe.sh ==="
    echo "  pcsx-redux : $PCSX_REDUX"
    echo "  bios       : $LEGAIA_BIOS"
    echo "  iso        : $LEGAIA_ISO"
    [[ "${LEGAIA_NO_SSTATE:-0}" == "1" ]] \
        && echo "  sstate     : (cold boot — LEGAIA_NO_SSTATE=1)" \
        || echo "  sstate     : $LEGAIA_SSTATE${LEGAIA_SCENARIO:+ (from --scenario $LEGAIA_SCENARIO)}"
    echo "  lua        : $LEGAIA_LUA"
    [[ -n "$LEGAIA_PROBE_SPEC" ]] && echo "  spec       : $LEGAIA_PROBE_SPEC"
    echo "  frames     : $LEGAIA_FRAMES"
    [[ -n "$LEGAIA_OUT" ]]     && echo "  out        : $LEGAIA_OUT"
    [[ -n "$LEGAIA_OUT_DIR" ]] && echo "  out_dir    : $LEGAIA_OUT_DIR"
    [[ $FAST -eq 1 ]] && echo "  mode       : fast (recompiler — no Lua BPs)" \
                      || echo "  mode       : interpreter+debugger (Lua BPs fire)"
    echo "  log        : $LOG_FILE"
    echo "===================="
} | tee "$LOG_FILE"

# Pass through all the LEGAIA_* knobs the autorun reads.
export LEGAIA_SSTATE LEGAIA_FRAMES LEGAIA_OUT LEGAIA_OUT_DIR LEGAIA_SCENARIO LEGAIA_PROBE_SPEC

# ---------- run ----------
# stdbuf forces line-buffered stdout/stderr so the log streams live rather
# than dumping in one chunk at exit. -stdout enables pcsx-redux's
# fputs-to-stdout path.
emu_flags=(-bios "$LEGAIA_BIOS" -iso "$LEGAIA_ISO" -run -stdout -dofile "$LEGAIA_LUA")
if [[ $FAST -eq 0 ]]; then
    # Both flags are required: -interpreter selects the non-recompiling CPU,
    # and DebugSettings::Debug (= -debugger) gates the debug-process hook.
    emu_flags=(-interpreter -debugger "${emu_flags[@]}")
fi

stdbuf -oL -eL "$PCSX_REDUX" "${emu_flags[@]}" >> "$LOG_FILE" 2>&1
EXIT=$?

echo "" | tee -a "$LOG_FILE"
echo "pcsx-redux exited with status $EXIT" | tee -a "$LOG_FILE"

# Surface CSV / probe-hits summary from the log if present.
if [[ -n "$LEGAIA_OUT" && -f "$LEGAIA_OUT" ]]; then
    rows=$(($(wc -l < "$LEGAIA_OUT") - 1))
    echo "CSV: $LEGAIA_OUT  ($rows data rows)"
fi
if grep -q "=== probe hits ===\|=== world-map probe hits ===\|=== prim-pool writer\|=== lzs+bundle probe\|=== fog probe hit\|=== writer-hunt summary ===" "$LOG_FILE"; then
    echo ""
    echo "=== probe summary ==="
    sed -n '/=== .* ===$/,/=== end ===\|^pcsx-redux exited/p' "$LOG_FILE" \
        | grep -vE "^pcsx-redux exited" | head -40
fi

exit "$EXIT"
