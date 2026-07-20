# Shell observer traps

Three shell defects share one root cause: **the observer is inside the thing it
observes**. Each has produced a false result in this project's own workflow more
than once, and in every recorded case the author had already read a written
warning naming that exact trap. Warnings are not a working control here, so the
countermeasures are structural: a hard commit gate
(`scripts/ci/check-shell-observer-traps.py`) and a helper library whose API has
no argument you can get wrong (`scripts/lib/proc.sh`).

## The three shapes

### 1. Pipe exit status (`PIPE_STATUS`)

A pipeline's exit status is its **last** stage's. `tail`, `head`, `tee`, `wc`
and friends essentially always succeed, so anything they consume has its status
erased.

```bash
cargo test | tail -20 && echo OK     # prints OK even when cargo test fails
```

The failure is silent and reads as a pass, which is the dangerous direction. The
fixes, in order of preference:

```bash
set -o pipefail                      # whole-script; the pipeline takes the failure
cargo test | tail -20; rc=${PIPESTATUS[0]}
run_capture build.log cargo test     # scripts/lib/proc.sh
```

`${PIPESTATUS[0]}` must be read on the line **immediately** after the pipeline —
any intervening command overwrites it.

### 2. Self-matching process predicates (`SELF_MATCH`)

`pkill -f <pattern>` and `pgrep -f <pattern>` match against full command lines,
and the pattern is a literal substring of the *caller's own* command line. The
predicate therefore matches itself.

```bash
until ! pgrep -f "cargo test"; do sleep 5; done   # never exits
pkill -f "cargo test -p legaia"                   # kills the shell that ran it
```

This is not theoretical: one such call terminated its own test run, and the
retry revealed that the surviving matches belonged to a **concurrent worktree** —
a slightly broader pattern would have destroyed unrelated work. A polling waiter
of this shape orphaned 28 shells in one session.

`killall` is the same defect keyed on process name instead of command line.

The fixes are all "name the process by identity, not by text":

```bash
kill -0 "$PID"                       # liveness by PID
wait "$PID"                          # the correct wait primitive
pgrep -g "$PGID"                     # liveness by process group
proc_kill_tree "$PID"                # scripts/lib/proc.sh; walks the real tree
```

`scripts/pcsx-redux/run_state_poll_selftest.sh` is the worked example: it
launches the emulator under `setsid`, so the child owns a process group the
caller is not a member of, and `pgrep -g "$PGID"` becomes a liveness test that
*cannot* match the caller.

### 3. `grep`'s empty-match exit code (`GREP_RC`)

`grep` exits 1 when it matches nothing. For a failure-scan, matching nothing is
the **clean** result — so exit 1 means "all good", and reading it as failure
sends you chasing a green suite. Under `set -e` an unguarded failure-scan also
aborts the script at exactly the moment everything was fine.

```bash
grep FAILED build.log                # set -e: aborts BECAUSE there were no failures
```

Fixes:

```bash
grep FAILED build.log || true
if grep -q FAILED build.log; then exit 1; fi
n=$(grep_count FAILED build.log)     # scripts/lib/proc.sh; always returns 0
```

The same convention applies to `git check-ignore` and `git diff --quiet`: exit 1
is an *answer*, exit >= 2 is a *failure*, and conflating them turns a broken
command into a clean report.

## The vacuous-pass corollary

A checker that resolves its own input by running a command must verify that
command succeeded. If `git diff --cached` fails, `.stdout` is empty, the scoped
file list is empty, and the gate scans nothing and exits 0 — the observer's
failure is indistinguishable from "the corpus is clean". `check-doc-density.py`,
`check-md-links.py` and `check-port-tags.py` each check the return code for this
reason; do not simplify those checks away.

## The gate

`scripts/ci/check-shell-observer-traps.py` scans every shell file under
`scripts/` (including the `pre-commit` hook itself) and fails on any of the three
shapes. The pre-commit hook runs it as a hard gate.

```bash
python3 scripts/ci/check-shell-observer-traps.py            # audit scripts/
python3 scripts/ci/check-shell-observer-traps.py --selftest # controls only
python3 scripts/ci/check-shell-observer-traps.py PATH...    # specific files
```

The checker runs its own **positive control** on every invocation, not only
under `--selftest`, and refuses to report a result if a known-bad snippet fails
to trip its detectors. This is deliberate. A previous sweep of this repo
reported a clean result from an `awk` range whose header never matched, and an
earlier version of this very audit reported "no `set -e` anywhere" from a probe
that only read the first three lines of each file. **A negative from an
unvalidated detector is not a finding.**

A line may be waived with a trailing `# observer-trap-ok: <reason>` comment. The
reason is mandatory — a bare waiver is itself reported, because an unexplained
"someone decided this was fine" is how the simpler-looking wrong form gets
restored later.

## The helpers

`scripts/lib/proc.sh` is sourced, not executed:

```bash
source "$(git rev-parse --show-toplevel)/scripts/lib/proc.sh"
```

| Helper | Replaces |
|---|---|
| `proc_kill_tree <pid> [sig]` | `pkill -f`. Walks the real process tree by PID; refuses to kill the calling shell or any ancestor. |
| `proc_spawn_group <log> <cmd>` | Bare `&`. Launches under `setsid` and echoes the new PGID. |
| `proc_group_alive <pgid>` | `pgrep -f`. Liveness by process group, which the caller is not in. |
| `proc_kill_group <pgid>` | `pkill -f`. TERM then KILL the whole group. |
| `proc_wait_pid <pid> [timeout]` | `until ! pgrep -f ...`. Waits on an identity, so it terminates. |
| `run_capture <log> <cmd>` | `cmd \| tail && echo OK`. Returns the command's status, tails the log. |
| `grep_count` / `grep_found` | Unguarded `grep` under `set -e`. |

## Scope limit

The gate covers **committed** scripts. It does nothing about ad-hoc shell typed
directly into a terminal or a tool call, which is where every recorded incident
actually happened. Preventing that class needs enforcement at the point of
execution — a command-blocking hook — not a library the author has to remember
to reach for.
