# Build and test speed

What a build and a test run spend their time on, and what to run so an edit does not wait for them. Everything here changes how fast a result arrives, never the result: the optimiser's choices do not move a simulation that runs on integers, and the ladder's trace comes out byte-identical under every profile below.

## Contents

- [Which profile](#which-profile)
- [What an edit costs](#what-an-edit-costs)
- [The linker](#the-linker)
- [Where a test run goes](#where-a-test-run-goes)
- [Profiling without perf](#profiling-without-perf)

## Which profile

| Profile | Use it for | What it is |
|---|---|---|
| `dev-fast` | The edit-build-test loop: one crate's tests, one oracle, a short replay. | `release-test`'s optimisation level with rustc's incremental cache over the workspace crates. Dependencies are built as `release-test` builds them. |
| `release-test` | The full-game ladder, the soak, `retail-compare`, anything long; and the last run before a commit touching simulation. It is what CI runs. | `release`'s optimisation level, no LTO, 16 codegen units. |
| `release` | Shipped binaries only. | Thin LTO, one codegen unit. |

```bash
cargo test -p legaia-engine-shell --profile dev-fast --test integration foo::
```

`dev-fast` binaries lose the inlining across codegen units, so they run about a third slower: a recompile saved is worth more than that on anything short, and less on a half-hour ladder. Its output lands in `target/dev-fast/`, beside `target/release-test/`, and the first build under it costs a build of the dependencies (sccache holds those, when it is configured).

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

The suite's wall clock is set by the slowest binary, since cargo runs binaries one at a time and tests within one in parallel. With a disc, that is `engine-shell`'s `integration`, whose length is part C of the [full-game ladder](full-game-ladder.md#what-the-time-goes-to). `patcher`'s `integration` is the most CPU, nearly all of it in the tests that apply a whole mod to a whole disc. Disc-gated tests that open a session read `PROT.DAT` out of the image each time, which is a real but small share next to those.

## Profiling without perf

Where `perf` is refused (`kernel.perf_event_paranoid` above 2, no root), [`scripts/profile/sampler.c`](../../scripts/profile/sampler.c) gets the same answer with no privilege and no rebuild: a preload library that takes a stack every 10 ms of CPU time, and [`scripts/profile/report.py`](../../scripts/profile/report.py) to read it.

```bash
gcc -O2 -shared -fPIC -o /tmp/sampler.so scripts/profile/sampler.c
cp target/release-test/deps/integration-<hash> /tmp/bin   # a later build relinks it in place
(cd crates/engine-shell && LD_PRELOAD=/tmp/sampler.so PROF_OUT=/tmp/run /tmp/bin full_game_ladder::continuous:: --test-threads 1)
scripts/profile/report.py /tmp/bin /tmp/run.<pid> --under 'plan_search$'
```

On a shared machine compare CPU seconds, not wall clock, and compare two binaries by running them side by side: a loaded box inflates even CPU time, through cache and memory contention.

A speedup is proved by the output, not the timing. For the ladder, the trace with its wall-clock fields stripped: `LEGAIA_FGL_TRACE=1` before and after must match line for line. For a patcher feature, the `.ppf` from a fixed seed must hash the same.
