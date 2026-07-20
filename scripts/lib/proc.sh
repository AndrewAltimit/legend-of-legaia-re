# shellcheck shell=bash
#
# proc.sh - process control that structurally cannot match the caller, plus a
# run-and-capture that reports the real exit code.
#
#   source "$(git rev-parse --show-toplevel)/scripts/lib/proc.sh"
#
# WHY THIS FILE EXISTS
#
# Three shell defects have each produced a false result in this project's own
# workflow, repeatedly, and always in the same way: the observer was inside the
# thing it observed.
#
#   * `pkill -f cargo` matches the shell that ran it, because the pattern is a
#     literal substring of that shell's own /proc/self/cmdline. One such call
#     killed its own test run; on the retry it turned out the surviving
#     processes belonged to a *concurrent* worktree, so a slightly different
#     pattern would have destroyed unrelated work.
#   * `until ! pgrep -f "cargo test"` can never exit, for the same reason. This
#     orphaned 28 shells in a single wave.
#   * `cmd | tail && echo OK` reports tail's status. A green suite was reported
#     over a real failure.
#
# Every one of those happened AFTER the author had read an explicit written
# warning naming that exact trap. Warnings demonstrably do not work here, so
# these helpers are built so the wrong thing is not expressible: there is no
# pattern argument to get wrong.
#
# The committed-script half of the problem is covered by
# `scripts/ci/check-shell-observer-traps.py`, which fails a commit that
# reintroduces any of the three shapes.

# --- process control -------------------------------------------------------

# proc_kill_tree <pid> [signal]
#
# Kill a process and its descendants BY PID, walking the actual process tree.
# There is no pattern, so there is nothing that can match this shell. Safe to
# call on a dead or bogus PID (returns 0).
#
# Refuses to touch the calling shell or any of its ancestors -- passing $$ by
# accident is an error, not a suicide.
proc_kill_tree() {
    local pid="${1:?proc_kill_tree: need a PID}"
    local sig="${2:-TERM}"

    [[ "$pid" =~ ^[0-9]+$ ]] || { echo "proc_kill_tree: '$pid' is not a PID" >&2; return 2; }

    # Self-protection: refuse $$, $PPID, and anything above us in the tree.
    local probe=$$
    while [[ -n "$probe" && "$probe" != "0" && "$probe" != "1" ]]; do
        if [[ "$probe" == "$pid" ]]; then
            echo "proc_kill_tree: refusing to kill $pid -- it is this shell or an ancestor" >&2
            return 2
        fi
        probe="$(ps -o ppid= -p "$probe" 2>/dev/null | tr -d ' ')"
    done

    # Depth-first: children before the parent, so nothing gets reparented to
    # init and survives.
    local child
    while read -r child; do
        [[ -n "$child" ]] && proc_kill_tree "$child" "$sig"
    done < <(ps -o pid= --ppid "$pid" 2>/dev/null | tr -d ' ')

    kill -"$sig" "$pid" 2>/dev/null || true
    return 0
}

# proc_spawn_group <logfile> <cmd> [args...]
#
# Launch a command in its OWN process group and echo that group's id. Because
# the caller is in a different group, `pgrep -g <pgid>` is a liveness test that
# cannot match the caller -- the structural version of `pgrep -f`.
#
# Pair with proc_group_alive / proc_kill_group below.
proc_spawn_group() {
    local log="${1:?proc_spawn_group: need a logfile}"; shift
    setsid "$@" >"$log" 2>&1 &
    # In a non-interactive shell (no job control) the background child is not a
    # process-group leader, so setsid execs in place rather than forking, and
    # $! is the new group leader's PID == its PGID. If you enable `set -m`, this
    # stops being true -- setsid would fork and $! would be the wrong process.
    echo $!
}

# proc_group_alive <pgid> -- true while any process in the group lives.
proc_group_alive() {
    local pgid="${1:?proc_group_alive: need a PGID}"
    pgrep -g "$pgid" >/dev/null 2>&1
}

# proc_kill_group <pgid> -- TERM the group, then KILL what is left.
proc_kill_group() {
    local pgid="${1:?proc_kill_group: need a PGID}"
    [[ "$pgid" =~ ^[0-9]+$ ]] || return 2
    kill -TERM -- -"$pgid" 2>/dev/null || true
    sleep 2
    kill -KILL -- -"$pgid" 2>/dev/null || true
}

# proc_wait_pid <pid> [timeout_secs]
#
# Block until a PID exits. This is the correct shape for "wait for the build to
# finish" -- `until ! pgrep -f "cargo test"` is the incorrect one, and it never
# terminates. Returns 0 if the process exited, 1 on timeout.
proc_wait_pid() {
    local pid="${1:?proc_wait_pid: need a PID}"
    local timeout="${2:-0}"
    local waited=0
    while kill -0 "$pid" 2>/dev/null; do
        if (( timeout > 0 && waited >= timeout )); then
            return 1
        fi
        sleep 1
        waited=$(( waited + 1 ))
    done
    return 0
}

# --- run and capture -------------------------------------------------------

# run_capture <logfile> <cmd> [args...]
#
# Run a command with all output to <logfile>, print a short tail, and return the
# COMMAND's exit status -- not the tail's.
#
# This exists because `cmd | tail -20 && echo OK` reports tail's status, and
# tail essentially always succeeds. Here the command's status is captured before
# anything else runs, so no later stage can overwrite it.
#
#   run_capture build.log cargo test -p legaia-asset || echo "FAILED (rc=$?)"
run_capture() {
    local log="${1:?run_capture: need a logfile}"; shift
    local rc=0
    "$@" >"$log" 2>&1 || rc=$?
    local tail_lines="${RUN_CAPTURE_TAIL:-20}"
    if (( tail_lines > 0 )); then
        tail -n "$tail_lines" "$log" || true
    fi
    if (( rc == 0 )); then
        echo "[run_capture] OK (rc=0)  full log: $log"
    else
        echo "[run_capture] FAILED rc=$rc  full log: $log" >&2
    fi
    return $rc
}

# grep_count <pattern> <file...>
#
# Number of matching lines, on stdout, ALWAYS returning 0. Use when a no-match
# is a legitimate (usually the *good*) result, so that `set -e` does not abort
# on grep's exit-1-means-nothing-matched.
#
#   n=$(grep_count 'FAILED' build.log)
#   (( n == 0 )) || { echo "$n failures"; exit 1; }
grep_count() {
    local pattern="${1:?grep_count: need a pattern}"; shift
    grep -c -- "$pattern" "$@" 2>/dev/null || true
}

# grep_found <pattern> <file...> -- true iff there is a match. Never aborts
# under `set -e`, and reads as a boolean at the call site.
grep_found() {
    local pattern="${1:?grep_found: need a pattern}"; shift
    grep -q -- "$pattern" "$@" 2>/dev/null
}
