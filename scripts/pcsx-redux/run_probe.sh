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
#                        faster; Lua BPs do NOT fire in this mode - use
#                        only for vsync-event-only probes)
#   --timing             interpreter CPU with the debugger hook OFF: the
#                        interpreter's guest-visible timing (cycle
#                        accounting differs from the recompiler's) at much
#                        better host speed than the default
#                        interpreter+debugger core. Lua BPs do NOT fire
#                        (the debug-process hook is what runs them) - use
#                        only for poll-only probes, specifically ones
#                        testing whether a repro is timing-sensitive
#                        across CPU cores. Mutually exclusive with --fast.
#   --isolate-config     run the emulator against a throwaway persistent dir
#                        (a curated fast profile) instead of the user's real
#                        ~/.config/pcsx-redux. ON BY DEFAULT under --fast so a
#                        volunteer's persisted PCSX-Redux config (debugger-on
#                        -> interpreter, a broken hardware-GPU pick, an odd
#                        frame limit) can't degrade the capture.
#   --no-isolate-config  force the real persistent dir even under --fast
#                        (env: LEGAIA_NO_ISOLATE=1). Use when you WANT your own
#                        saved layout / memcards / settings.
#   --log PATH           emulator log path (default logs/pcsx_probe_<stem>.log)
#   --help               print this header and exit
#
# Why -interpreter -debugger by default:
#   psxinterpreter.cc:1652 - Lua BPs only fire when both are set. The
#   interpreter is required for the debug-process hook, and DebugSettings::Debug
#   gates the hook itself. --fast skips both for probes that don't arm BPs.
#
# Config isolation (--isolate-config, default-on under --fast):
#   PCSX-Redux reads pcsx.json + memcards + imgui layout from getPersistentDir()
#   (src/core/system.cc): $HOME/.config/pcsx-redux on Linux, OR the -portable
#   PATH argument when given. It does NOT honour XDG_CONFIG_HOME. So a volunteer's
#   persisted settings (Debug=true forces the interpreter; HardwareRenderer /
#   Scaler / frame limits ride the saved config) leak into the community capture
#   and tank speed. Isolation writes a minimal fast profile (Dynarec on, Debug
#   off, ship-default renderer) into LEGAIA_PCSX_PROFILE_DIR and launches with
#   -portable, so the capture is config-independent. Memory cards are pointed at
#   the real ~/.config/pcsx-redux via ABSOLUTE Mcd paths (memorycard.cc only
#   prepends the persistent dir to RELATIVE names), so card saves still work.
#   Knobs: LEGAIA_PCSX_PROFILE_DIR (profile dir), LEGAIA_PCSX_REAL_CONFIG (real
#   config dir for memcards), LEGAIA_PCSX_HARDWARE_GPU=1 (pin the OpenGL/hardware
#   renderer instead of the ship-default software one).

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
TIMING=0
# Config isolation (see "Config isolation" block below). "" = auto (on under
# --fast), 1 = force on, 0 = force off. Env LEGAIA_NO_ISOLATE=1 forces off.
ISOLATE_CONFIG="${LEGAIA_ISOLATE_CONFIG:-}"
# Managed throwaway persistent dir for the isolated profile.
LEGAIA_PCSX_PROFILE_DIR="${LEGAIA_PCSX_PROFILE_DIR:-$REPO_ROOT/captures/.pcsx-profile}"
# Where the volunteer's real memory cards live, so isolation keeps card saves
# visible instead of stranding them behind a fresh persistent dir.
LEGAIA_PCSX_REAL_CONFIG="${LEGAIA_PCSX_REAL_CONFIG:-$HOME/.config/pcsx-redux}"

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
        --timing)    TIMING=1; shift ;;
        --isolate-config)    ISOLATE_CONFIG=1; shift ;;
        --no-isolate-config) ISOLATE_CONFIG=0; shift ;;
        -h|--help)
            sed -n '2,65p' "$0"
            exit 0 ;;
        *)
            echo "ERROR: unknown flag: $1" >&2
            echo "Run 'bash $0 --help' for usage." >&2
            exit 64 ;;
    esac
done

if [[ $FAST -eq 1 && $TIMING -eq 1 ]]; then
    echo "ERROR: --fast and --timing are mutually exclusive (pick one CPU core)" >&2
    exit 64
fi

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
# When --spec is used, the lua is _runner.lua for every spec - derive the
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

# ---------- config isolation resolution + fast-profile write ----------
# Resolve the effective isolation setting: explicit flag/env wins, else auto
# (on under --fast, off otherwise). See the "Config isolation" header block.
if [[ -z "$ISOLATE_CONFIG" ]]; then
    if [[ "${LEGAIA_NO_ISOLATE:-0}" == "1" ]]; then
        ISOLATE_CONFIG=0
    elif [[ $FAST -eq 1 ]]; then
        ISOLATE_CONFIG=1
    else
        ISOLATE_CONFIG=0
    fi
elif [[ "${LEGAIA_NO_ISOLATE:-0}" == "1" ]]; then
    # An explicit LEGAIA_NO_ISOLATE=1 always disables, even vs LEGAIA_ISOLATE_CONFIG.
    ISOLATE_CONFIG=0
fi

PROFILE_HW_GPU="ship-default (software)"
if [[ "$ISOLATE_CONFIG" == "1" ]]; then
    if [[ "${LEGAIA_PCSX_HARDWARE_GPU:-0}" == "1" ]]; then
        _hw_gpu=true; PROFILE_HW_GPU="hardware (OpenGL)"
    else
        _hw_gpu=false
    fi
    # CPU/Debug mirror the run mode so isolation composes with any core: fast
    # -> recompiler + debugger off; timing -> interpreter + debugger off;
    # slow (firehose) -> interpreter + debugger on (the CLI flags below
    # re-assert this per-run too, but pinning it in the profile keeps the
    # two in agreement).
    if [[ $FAST -eq 1 ]]; then _dynarec=true; _debug=false
    elif [[ $TIMING -eq 1 ]]; then _dynarec=false; _debug=false
    else _dynarec=false; _debug=true; fi
    mkdir -p "$LEGAIA_PCSX_PROFILE_DIR" "$LEGAIA_PCSX_REAL_CONFIG"
    # Minimal, deterministic fast profile. Only the keys we care about are
    # pinned; every other setting falls back to the emulator's compile-time
    # ship default (src/core/psxemulator.h), so this can't drift from a
    # volunteer's oddities. Rewritten fresh every run (PCSX rewrites it fully
    # on exit, so we re-pin each launch). Mcd paths are ABSOLUTE (into the real
    # config dir) so card saves survive isolation (memorycard.cc only prepends
    # the persistent dir to relative names).
    cat > "$LEGAIA_PCSX_PROFILE_DIR/pcsx.json" <<JSON
{
  "emulator": {
    "Dynarec": $_dynarec,
    "HardwareRenderer": $_hw_gpu,
    "Xa": true,
    "SpuIrq": false,
    "FastBoot": true,
    "Scaler": 100,
    "AutoUpdate": false,
    "Mcd1": "$LEGAIA_PCSX_REAL_CONFIG/memcard1.mcd",
    "Mcd2": "$LEGAIA_PCSX_REAL_CONFIG/memcard2.mcd",
    "Mcd1Inserted": true,
    "Mcd2Inserted": true,
    "Debug": { "Debug": $_debug, "GdbServer": false, "WebServer": false }
  }
}
JSON
fi

# ---------- banner ----------
{
    echo "=== run_probe.sh ==="
    echo "  pcsx-redux : $PCSX_REDUX"
    echo "  bios       : $LEGAIA_BIOS"
    echo "  iso        : $LEGAIA_ISO"
    [[ "${LEGAIA_NO_SSTATE:-0}" == "1" ]] \
        && echo "  sstate     : (cold boot - LEGAIA_NO_SSTATE=1)" \
        || echo "  sstate     : $LEGAIA_SSTATE${LEGAIA_SCENARIO:+ (from --scenario $LEGAIA_SCENARIO)}"
    echo "  lua        : $LEGAIA_LUA"
    [[ -n "$LEGAIA_PROBE_SPEC" ]] && echo "  spec       : $LEGAIA_PROBE_SPEC"
    echo "  frames     : $LEGAIA_FRAMES"
    [[ -n "$LEGAIA_OUT" ]]     && echo "  out        : $LEGAIA_OUT"
    [[ -n "$LEGAIA_OUT_DIR" ]] && echo "  out_dir    : $LEGAIA_OUT_DIR"
    if [[ $FAST -eq 1 ]]; then
        echo "  mode       : fast (-dynarec forced; no Lua BPs; verify top bar = CPU: Dynarec)"
    elif [[ $TIMING -eq 1 ]]; then
        echo "  mode       : timing (interpreter, debugger OFF; no Lua BPs; verify top bar = CPU: Interpreted)"
    else
        echo "  mode       : interpreter+debugger (Lua BPs fire)"
    fi
    [[ "$ISOLATE_CONFIG" == "1" ]] \
        && echo "  config     : ISOLATED profile ($LEGAIA_PCSX_PROFILE_DIR; renderer=$PROFILE_HW_GPU; cards=$LEGAIA_PCSX_REAL_CONFIG)" \
        || echo "  config     : real persistent dir ($LEGAIA_PCSX_REAL_CONFIG)"
    echo "  log        : $LOG_FILE"
    echo "===================="
} | tee "$LOG_FILE"

# Pass through all the LEGAIA_* knobs the autorun reads.
export LEGAIA_SSTATE LEGAIA_FRAMES LEGAIA_OUT LEGAIA_OUT_DIR LEGAIA_SCENARIO LEGAIA_PROBE_SPEC

# Tell the Lua side which CPU core this launch selected, so a breakpoint
# probe can HARD-REFUSE a --fast launch (Lua BPs silently never fire under
# the recompiler - hours of play, empty capture) and a poll probe can warn
# it was launched on the slow core. Probes launched outside this runner
# see no LEGAIA_CORE and fall back to their runtime canaries.
if [[ $FAST -eq 1 ]]; then LEGAIA_CORE=dynarec
elif [[ $TIMING -eq 1 ]]; then LEGAIA_CORE=interpreter-nodebug
else LEGAIA_CORE=interpreter; fi
export LEGAIA_CORE

# ---------- run ----------
# stdbuf forces line-buffered stdout/stderr so the log streams live rather
# than dumping in one chunk at exit. -stdout enables pcsx-redux's
# fputs-to-stdout path.
emu_flags=(-bios "$LEGAIA_BIOS" -iso "$LEGAIA_ISO" -run -stdout -dofile "$LEGAIA_LUA")
if [[ "$ISOLATE_CONFIG" == "1" ]]; then
    # -portable PATH points getPersistentDir() at our throwaway profile dir
    # (src/core/arguments.cc: the flag's value sets m_portablePath AND flips
    # m_portable true). The flags parser takes the token after -portable as its
    # value, so keep the path immediately after the flag.
    emu_flags=(-portable "$LEGAIA_PCSX_PROFILE_DIR" "${emu_flags[@]}")
fi
if [[ $TIMING -eq 1 ]]; then
    # Interpreter core WITHOUT the debug-process hook: guest timing matches
    # the default slow core, host speed is much better. Lua BPs never fire
    # here (the hook is what runs them) - poll-only probes only.
    # -no-debugger is required, not just omitting -debugger: with no flag,
    # main.cc falls back to the PERSISTED DebugSettings::Debug in pcsx.json
    # (usually true on a debugging workstation), which re-enables the hook.
    emu_flags=(-interpreter -no-debugger "${emu_flags[@]}")
elif [[ $FAST -eq 0 ]]; then
    # Both flags are required: -interpreter selects the non-recompiling CPU,
    # and DebugSettings::Debug (= -debugger) gates the debug-process hook.
    emu_flags=(-interpreter -debugger "${emu_flags[@]}")
else
    # EXPLICITLY force the recompiler. Merely OMITTING -interpreter is not
    # enough: with no CPU flag, PCSX-Redux falls back to the PERSISTED
    # settings in pcsx.json ("Dynarec": false, "Debug": true), which pin it to
    # the slow interpreter+debugger core regardless of the CLI - the top bar
    # reads "CPU: Interpreted" and fps tanks. -dynarec overrides that per-run
    # without editing the user's config (the slow firehose still needs the
    # persisted debugger). The debugger WINDOW may still open from the saved
    # imgui layout - harmless here (no BPs arm); the CPU runs the recompiler.
    # Verify: the top bar should read "CPU: Dynarec".
    emu_flags=(-dynarec "${emu_flags[@]}")
fi

# LEGAIA_GDB=1 wraps the emulator in gdb so a segfault (sporadic on
# cold-boot / scene transitions with the debugger enabled) yields a
# backtrace in the log instead of just "(core dumped)". Runs at full
# speed until a signal fires; no interaction needed.
if [[ "${LEGAIA_GDB:-0}" == "1" ]]; then
    gdb -batch -q \
        -ex "set exec-wrapper stdbuf -oL -eL" \
        -ex run \
        -ex "thread apply all bt" \
        --args "$PCSX_REDUX" "${emu_flags[@]}" >> "$LOG_FILE" 2>&1
    EXIT=$?
else
    stdbuf -oL -eL "$PCSX_REDUX" "${emu_flags[@]}" >> "$LOG_FILE" 2>&1
    EXIT=$?
fi

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
