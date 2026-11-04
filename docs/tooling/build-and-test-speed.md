# Build and test speed

What a build and a test run spend their time on, and what to run so an edit does not wait for them. Everything here changes how fast a result arrives, never the result: the optimiser's choices do not move a simulation that runs on integers, and the ladder's trace comes out byte-identical under every profile below.

## Contents

- [Which profile](#which-profile)
- [What an edit costs](#what-an-edit-costs)
- [The wasm bundle](#the-wasm-bundle)
- [The linker](#the-linker)
- [Where a test run goes](#where-a-test-run-goes)
- [Page faults](#page-faults)
- [The commit-time gates](#the-commit-time-gates)
- [The full gate](#the-full-gate)
- [Profiling without perf](#profiling-without-perf)

## Which profile

| Profile | Use it for | What it is |
|---|---|---|
| `dev-fast` | The edit-build-test loop: one crate's tests, an oracle, a replay, a ladder run after an edit. | `release-test`'s optimisation level with rustc's incremental cache over the workspace crates. Dependencies are built as `release-test` builds them. |
| `release-test` | What CI runs, so the last run before a commit; and what the `scripts/ci/*.py` drivers build. | `release`'s optimisation level, no LTO, 16 codegen units. |
| `release` | Shipped binaries only. | Thin LTO, one codegen unit. |

```bash
cargo test -p legaia-engine-shell --profile dev-fast --test integration foo::
```

`dev-fast` binaries lose some inlining across codegen units, and run a few percent slower for it on the ladder, with the whole suite's outcomes the same, test for test. Its output lands in `target/dev-fast/`, beside `target/release-test/`, and the first build under it costs a build of the dependencies (sccache holds those, when it is configured).

CI stays on `release-test`, and a change is not verified until it has run there. `dev-fast` is a local choice and no script or workflow names it.

The `scripts/ci/*.py` drivers build and run `legaia-engine` under `release-test` themselves, so `retail-compare.py` after an edit pays the non-incremental rebuild. Their long runs are what that profile is for.

## What an edit costs

A crate is compiled whole when it or anything under it changes, because cargo has no early cut-off on an unchanged interface. So the cost of an edit is the crates above it: an edit in `engine-core` recompiles `engine-scene-host`, `engine-session`, `engine-netplay`, `web-play`, `parity`, `engine-shell` and the `integration` test binary, each from scratch under `release-test`. `dev-fast` recompiles the same crates, and each reuses the codegen units the edit did not reach; that is the whole difference, and it is an order of magnitude on a mid-graph edit.

Under a non-incremental profile, by CPU:

- LLVM dominates. Type-checking and borrow-checking are a small part of nearly every crate.
- Of everything in the graph, `legaia-expansion-format` costs the most. Its TOML sections carry derived `Deserialize` and `Serialize` for hundreds of types, instantiated against each of `toml`'s deserializers, and that is most of its IR. It is low in the graph but changes rarely, so it weighs on a clean build, not on an edit.
- `engine-shell`'s `integration` binary is the largest single unit an edit reaches, and it is recompiled for an edit in anything it links. An edit under `crates/engine-shell/tests/` rebuilds that unit alone.

Two things that look like costs and are not worth chasing: the test harness of each `[[bin]]` and the examples `cargo test` builds are a few percent together, and `cargo tree -d` finds only a handful of duplicated dependency versions, all under the windowing stack.

`cargo test -p <crate>` and `cargo test --workspace` resolve different feature sets, so switching between them rebuilds the shared dependencies the first time, not every time: cargo keeps both. `--test integration` is a third selection with its own set under `engine-shell`.

sccache misses on workspace crates between checkouts by design: the checkout path is in `-C metadata` and `CARGO_MANIFEST_DIR`. What it serves is dependencies. It passes incremental crates through uncached, so under `dev-fast` it covers the dependencies and rustc's own cache covers the workspace.

## The wasm bundle

The shipped bundles are `scripts/ci/build-wasm.sh`: `release` (one codegen unit) plus `wasm-opt` stripping. Both root crates are `cdylib` + `rlib`, and cargo runs LTO only when every crate type allows it, so the profile's `lto` does not reach them. Measured over the tool bundle with `CARGO_BUILD_JOBS=4` on the shared box, wall clock, sccache warm:

| What | Shipped | `--fast` |
|---|---|---|
| Clean target directory | about 6 min | about 5 min |
| Nothing changed | 35 s | 15 s |
| One line, `engine-core` | 4.5 min | 30 s |
| One line, `web-play` | 2 min | 30 s |
| One line, `xa` (low in the graph) | 4.5 min | 35 s |
| A function body, `engine-core` | - | 55 s |
| Tool `.wasm`, raw / gzip | 21.4 / 6.9 MB | 33.4 / 8.3 MB |
| Play `.wasm`, raw / gzip | 14.5 / 4.8 MB | - |

A one-line edit in `engine-core` costs three quarters of a clean build, and no one step is why. It is the chain above the edit, each crate compiled whole: `engine-core` itself about 85 s, `web-play` 55 s, the final link 35 s, the rest of the stack 70 s, and `wasm-bindgen` plus the strip pass 33 s. The strip is fast only by comparison, about 25 s across a dozen threads, because Binaryen parses and rewrites the module to do it; a full `-O` pass is what takes twenty minutes.

The second bundle costs one more final link, `wasm-bindgen` and strip pass, not a second chain: the two builds share every rlib. That rests on two choices in its manifests. `web-play` lists the tool bundle's `web-sys` features, because each bundle is a cargo build of its own and features unify per build, and a shorter list compiles `web-sys` and everything above it twice. The play crate keeps the `rlib` type, because a lone `cdylib` turns LTO on for that bundle alone, which recompiles the whole graph with bitcode and came out larger, 5.18 against 4.77 MB gzip.

The obvious knob stays where it is. `codegen-units = 16` takes the `engine-core` edit to under 3 min and the bundle to 7.13 MB gzipped against 6.86, which every player downloads. A target directory reused across worktrees gains nothing either: workspace crates hash their path, and sccache already serves the dependencies, which is why a clean build is minutes.

So the iteration path is a separate build:

```bash
scripts/ci/build-wasm.sh --fast   # both bundles from the wasm-fast profile, no wasm-opt
scripts/ci/build-wasm.sh --fast --only play   # one bundle; --strict refuses the mix
scripts/ci/check-pages.mjs        # the same page checks run on it
```

`wasm-fast` is `release-test` with the incremental cache over the workspace crates, the `dev-fast` recipe for the browser target. `opt-level = 1` is no faster to rebuild and runs slower, so it keeps `release-test`'s level. They land in `site/wasm/` where a page expects them, and the freshness stamp records the profile: `check-wasm-freshness.py --strict` refuses it, so neither a claim that a fix is live nor the full gate can rest on them. Run `build-wasm.sh` once before either.

`scripts/ci/check-wasm-target.sh` stays the cheapest question, whether the browser host compiles: about 90 s from clean, 30 to 60 s after an edit.

## The linker

The `aarch64-unknown-linux-gnu` toolchain links with the system `cc`, which runs GNU `ld`. `rust-lld`, which rustup ships, takes well under half the time on the test binaries. x86_64 Linux stable already defaults to it.

```bash
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-Clink-arg=-fuse-ld=lld -Clink-arg=-B$(rustc --print sysroot)/lib/rustlib/aarch64-unknown-linux-gnu/bin/gcc-ld"
```

Set it per shell and not in `~/.cargo/config.toml`: on a machine that is also the CI runner, that file applies to the release builds as well, and `release-build.sh` is the only thing that should decide how a shipped binary links. The flags enter cargo's fingerprint, so the first build in a shell with them rebuilds the tree.

## Where a test run goes

libtest's per-test timing works on a stable compiler when the flag is passed to the built binary with `RUSTC_BOOTSTRAP` set, without touching the build:

```bash
cargo test -p legaia-patcher --profile release-test --test integration --no-run
cd crates/patcher
RUSTC_BOOTSTRAP=1 ../../target/release-test/deps/integration-<hash> -Zunstable-options --report-time
```

Run from the crate directory, as cargo does: tests read paths relative to it.

The suite's wall clock is set by the slowest binary, since cargo runs binaries one at a time and tests within one in parallel. With a disc, that is `engine-shell`'s `integration`, whose length is part C of the [full-game ladder](full-game-ladder.md#what-the-time-goes-to). `patcher`'s `integration` is the most CPU, nearly all of it in the tests that apply a whole mod to a whole disc. Disc-gated tests that open a session read `PROT.DAT` out of the image each time, which is a real but small share next to those, and small enough that sharing one index across a test binary is not worth tests no longer starting from a fresh one.

## Page faults

A test that holds the disc image in a `Vec` pays for it twice: the copy, and a page fault for every 4 KiB of the allocation, in the kernel and contended between threads. On the patcher suite those faults are more CPU than the tests' own code, and it shows as system time rather than in a profile of the program.

The repo's [`.cargo/config.toml`](../../.cargo/config.toml) sets `GLIBC_TUNABLES=glibc.malloc.hugetlb=1` for everything cargo spawns. glibc then asks for transparent huge pages behind its large allocations (`madvise`, so it works where the system setting is `madvise`, as well as `always`), and one fault covers 2 MiB. It is an allocation hint: it moves nothing a program computes, peak memory stays about where it was, a clean build under it costs what one costs without it, and other libcs ignore the variable. An inherited `GLIBC_TUNABLES` wins, so `GLIBC_TUNABLES= cargo test ...` measures without it.

It reaches what cargo runs. A binary started by hand, as in the profiling recipe below or by a `scripts/ci/*.py` driver, runs without it unless the shell sets it.

`DiscPatcher::open` reads the `PROT.DAT` TOC in place out of the image rather than through a flat copy of the archive, for the same reason. `crates/web-patcher`'s byte pin runs its cases side by side, each over its own copy of the image. Neither moved a pinned digest.

## The commit-time gates

[`scripts/git-hooks/pre-commit`](../../scripts/git-hooks/pre-commit) runs the same checks in the same tree whatever the order, so the order is chosen for how fast a failure reports:

1. The sub-second gates, one after another.
2. `cargo fmt`, with its auto-fix, when Rust is staged: whatever reads source below reads what the commit will hold.
3. The gates that take seconds, side by side (`start_gate` / `finish_gates`): the dump-corpus ratchets, the port catalog, and on a Rust commit the host-drift gates. Each logs to its own file, the logs print in the hook's order, and every failure is reported, not only the first.
4. `cargo clippy` and the `wasm32` check, which are the longest and take cargo's build lock.

A gate added to step 3 has to read the tree only, and not read another gate's output. The port catalog owns `target/port-catalog/`.

In the Repo Gates job the one step that compiles is last for the same reason, and it builds into the per-toolchain target directory the other jobs use. Built into the cache root, `select-ci-target-dir.sh` in the next job pruned it as stale, and every run compiled `asset` from nothing.

A gate that is slow is usually repeating itself. `check-css-cascade-order.py` keeps its parse of the shared stylesheet across pages, and the catalog's call graph resolves a name once per calling file. For a gate, the proof is its output: `port-catalog.py --live-audit` must write `target/port-catalog/` byte for byte as before.

## The full gate

[`scripts/ci/full-gate.py`](../../scripts/ci/full-gate.py) is the end-of-work gate as one command. It reports and never decides: nothing it runs blesses, proposes or pushes, and a command that carries such a flag is refused before the run starts.

```bash
scripts/ci/full-gate.py                       # everything
scripts/ci/full-gate.py --list                # the gates, their groups and commands
scripts/ci/full-gate.py --only repo,lint-*    # gate names, globs or groups; needs come along
scripts/ci/full-gate.py --skip 'ladder-seed-*'
scripts/ci/full-gate.py --resume              # rerun what did not pass, on the same tree
```

| Group | What runs |
|---|---|
| `repo` | Every `run:` step of `main-ci.yml`'s Repo Gates job, in a clean `git worktree add HEAD`, with no disc in the environment and none of the gitignored data; and `git ls-files site \| git check-ignore --no-index --stdin`. |
| `tree` | `fmt`, both clippy passes and the wasm type-check, read from the PR Lint job; Repo Gates' ratchet steps again where the data is; `byte-sweep`, which writes the sweep the byte-accounting ratchet reads; `check-baseline-loosening.py --base`; [`revert-scan.py`](../../scripts/ci/revert-scan.py). |
| `build` | One build of `legaia-engine` and one of the ladder's test binary, each copied aside so the gates below it read one binary whatever cargo relinks later. |
| `tests` | The Disc-Gated Tests job's `run-disc-tests.sh`. `tests-no-disc`, PR Lint's disc-free suite in the clean tree, runs only when named: it is a second whole build. |
| `ladder` | Part B, part C on the default stream, and each seed `[continuous_seeds]` lists. |
| `retail` | `retail-compare.py --images --check`, the audio sweep, the PCM check, capture coverage. |
| `wasm` | `build-wasm.sh`, `check-wasm-freshness.py --strict`, [`check-pages.mjs`](../../scripts/ci/check-pages.mjs), the [cost ratchet](frame-bench.md#cost-ratchet). |

The workflow's steps are read from the file on every run, so a step added there is a gate here with no edit; the Repo Gates job runs `full-gate.py --selftest`, which fails on a step the reader cannot read or one carrying a forbidden command. An Actions expression has no value outside Actions, so a step's `${{ ... }}` variable is empty: `BASE_SHA`'s step takes its self-test branch, and `loosening-base` does the comparison, without the step's `git fetch --depth=1`, which would turn the shared repository shallow. The runner's `CARGO_TARGET_DIR` is replaced by a directory under `--out`, because `select-ci-target-dir.sh` prunes around whatever it is given.

Every failure is reported with its log, not the first: a gate whose prerequisite failed shows as `BLOCKED`, and the rest still run. A driver's `[skip]` line reads as `SKIP` and the verdict as `PARTIAL`, since a skip is not a pass; only the `repo` group is exempt, where a ratchet skipping for want of the disc is what the runner does. `preflight` fails when the extracted disc, the save library, the captures, the dump corpus, the disc image or a display is missing, because each of those otherwise makes a gate pass without measuring. `tree-unchanged` runs last and fails if a tracked file moved during the run.

Output goes under `--out` (default `target/full-gate`): `logs/<gate>.log`, `timing.md` and `state.json`. `timing.md` gives each gate's start, length and CPU seconds, which are what a critical path and a gate's real share of the budget are read from. `--resume` refuses a tree that has changed since the run it would resume.

Three flags set the load on a shared box: `--max-cpu` is the budget each gate is weighed against, `--jobs` is what `retail-compare`, the audio sweep and libtest get, and `--cargo-jobs` is `CARGO_BUILD_JOBS`. `--memory-max` is the `systemd-run --user --scope` cap on each engine-looping gate. A gate over the budget starts when the budget has room, or alone.

The budget and the jobs flags fit together: at the default twelve and six, two heavy gates run side by side, where a job count above six makes each heavy gate run alone. What sets the wall clock is the chain the `display` lock serialises: `retail-compare`, the two smokes, `pages`, `quality-ladder`, `cost-ratchet`. `tests-build` and `tests-disc` hold `cargo-rt` beside it, and that is the second chain. So the scheduler offers those two chains the budget first, with the lints among them because a lint failure is worth hearing about early, and the seeds last: they block nothing and each takes one core. The order is `START_FIRST` in the script; `timing.md` keeps plan order.

What saves time beyond running side by side: `cargo test --workspace` already runs parts B and C, so `ladder-b` and `ladder-c` read their verdict out of the suite's log, and run on their own only when the suite did not reach them; the seeds share one binary and skip cargo's lock; `wasm-build` is answered by a freshness stamp that vouches for a shipped-profile bundle from this tree; and the drivers read `LEGAIA_ENGINE_BIN`, so the suite's relink of `target/release-test/legaia-engine` with other features cannot pull the binary out from under them. Ratchet modes measure fresh whatever the flags: `--check` reads no result cache.

## Profiling without perf

Where `perf` is refused (`kernel.perf_event_paranoid` above 2, no root), [`scripts/profile/sampler.c`](../../scripts/profile/sampler.c) gets the same answer with no privilege and no rebuild: a preload library that takes a stack every 10 ms of CPU time, and [`scripts/profile/report.py`](../../scripts/profile/report.py) to read it.

```bash
gcc -O2 -shared -fPIC -o /tmp/sampler.so scripts/profile/sampler.c
cp target/release-test/deps/integration-<hash> /tmp/bin   # a later build relinks it in place
(cd crates/engine-shell && LD_PRELOAD=/tmp/sampler.so PROF_OUT=/tmp/run /tmp/bin full_game_ladder::continuous:: --test-threads 1)
scripts/profile/report.py /tmp/bin /tmp/run.<pid> --under 'plan_search$'
```

On a shared machine compare CPU seconds, not wall clock, and compare two binaries by running them side by side: a loaded box inflates even CPU time, through cache and memory contention.

A speedup is proved by the output, not the timing. For part C of the ladder, the trace with its wall-clock fields stripped: `LEGAIA_FGL_TRACE=1` before and after must match line for line, on any stream; the wall-clock fields are the `[run N]` and `[seg N]` durations and the test harness's own `finished in`. The trace repeats on one binary, so a difference is the change's ([what keeps it so](full-game-ladder.md#what-the-time-goes-to)). For a patcher feature, the `.ppf` from a fixed seed must hash the same.
