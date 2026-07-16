# Tagged-release binary pipeline

Pushing a `v*` tag builds the workspace binaries and attaches them to that
tag's GitHub release. There is no crates.io publishing step: GitHub release
artifacts are the only distribution channel.

The release archives carry **compiled binaries and our own text files only**.
No game data is packaged, no step reads a disc image, and nothing in the
pipeline may change that - the same rule that governs the rest of the repo.

| Piece | Lives in | Role |
|---|---|---|
| Release workflow | [`.github/workflows/release.yml`](../../.github/workflows/release.yml) | Tag trigger, test gate, publish |
| Toolchain provisioning | [`scripts/ci/setup-cross-toolchain.sh`](../../scripts/ci/setup-cross-toolchain.sh) | Per-target cross toolchain, idempotent |
| Build + package | [`scripts/ci/release-build.sh`](../../scripts/ci/release-build.sh) | Per-target build, staging, archive, checksum |

## Cutting a release

```bash
git tag v0.2.0
git push origin v0.2.0
```

That is the whole procedure. The workflow runs the test gate, builds each
target, and creates the release for the tag if it does not already exist,
attaching the archives to it. A tag containing `-` (say `v0.2.0-rc1`) is
marked as a prerelease.

To rehearse without touching a release, use the `workflow_dispatch` trigger
with an existing tag. It builds and packages but **does not publish** unless
the `publish` input is checked, so the dispatch path is a safe dry run.

## Target matrix

The self-hosted runner is **arm64 Linux**, which makes both other targets
cross-compiles - including the x86_64 Linux one.

| Target | Toolchain | Contents |
|---|---|---|
| `aarch64-unknown-linux-gnu` | Native | Every workspace binary |
| `x86_64-pc-windows-gnu` | mingw-w64 cross | Every workspace binary |
| `x86_64-unknown-linux-gnu` | `cargo-zigbuild` cross, glibc pinned to 2.28 | Every workspace binary **except** `legaia-engine` and `asset-viewer` |

The per-target binary lists are declared explicitly in `release-build.sh`, and
the script fails if an expected binary is missing from the build output. The
x86_64 Linux exclusion is a deliberate matrix entry, not a build that quietly
drops binaries.

### Why Windows is `-gnu` rather than `-msvc`

`x86_64-pc-windows-gnu` links through mingw-w64, which apt packages as an
arm64-hosted compiler emitting x86_64 PE objects. The whole workspace crosses
cleanly this way, GUI binaries included: wgpu, winit and cpal all build, and
cpal's Windows backend is WASAPI, so there is no ALSA dependency to satisfy.
The MSVC ABI would need `cargo-xwin` and a downloaded Microsoft SDK; the gnu
ABI produces working `.exe`s with a toolchain the runner already has.

### Why x86_64 Linux omits the GUI binaries

`legaia-engine` and `asset-viewer` reach cpal, whose Linux backend binds
`alsa-sys`. `alsa-sys` resolves `libasound` through `pkg-config` at build
time, so cross-compiling it to x86_64 needs an **x86_64** `libasound`. An
arm64 runner has the arm64 one, and pkg-config correctly refuses to hand a
foreign-architecture library to an x86_64 link. Everything else in the
workspace is ALSA-free and crosses without complaint.

Two ways to lift the restriction, in order of preference:

1. **Add an x86_64 Linux runner.** The target stops being a cross-compile,
   `setup-cross-toolchain.sh` detects it as the host and skips zig entirely,
   and the GUI binaries build like any native target. Move
   `x86_64-unknown-linux-gnu` to the `workspace` build mode in
   `release-build.sh` and add `GUI_BINS` to its matrix entry.
2. **Provision amd64 multiarch on the arm64 runner**, giving the cross link an
   x86_64 `libasound` to bind:

   ```bash
   sudo dpkg --add-architecture amd64
   sudo apt update
   sudo apt install libasound2-dev:amd64 gcc-x86-64-linux-gnu
   ```

   This also removes the need for zig on that target, at the cost of carrying
   a multiarch apt configuration on the runner.

## Artifacts

Each target produces one archive plus an entry in a single aggregate
`SHA256SUMS`:

```
legaia-tools-<version>-aarch64-unknown-linux-gnu.tar.gz
legaia-tools-<version>-x86_64-pc-windows-gnu.zip
legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz
SHA256SUMS
```

`<version>` is the tag with its leading `v` stripped, so `v0.2.0` yields
`legaia-tools-0.2.0-x86_64-pc-windows-gnu.zip`. Linux targets ship `.tar.gz`,
Windows ships `.zip`.

Every archive contains a single top-level
`legaia-tools-<version>-<target>/` directory - extracting one never scatters
files across the working directory. Inside it: the binaries, `LICENSE` and
`LICENSE-MIT` (the project is `MIT OR Unlicense`), and a generated
`README.txt` naming the target, listing the binaries actually present, and
restating that the user supplies their own disc image.

Verify a download against the published manifest:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

## Runner provisioning

Most of what the pipeline needs installs without root and is handled
automatically by `setup-cross-toolchain.sh`, which is idempotent - it is a
no-op once the runner is warm.

| Requirement | How it arrives | Root? |
|---|---|---|
| `mingw-w64` | `sudo apt install mingw-w64` | Yes - one-time |
| Rust target std | `rustup target add`, automatic | No |
| `zig` | pip wheel into a cache venv, automatic | No |
| `cargo-zigbuild` | `cargo install`, automatic | No |
| `libasound2-dev` | Already present; the native build needs it | Yes - one-time |

`mingw-w64` is the only piece the pipeline cannot install for itself. Without
it the Windows target fails fast with a provisioning hint rather than a linker
error.

Root-free tooling is cached under `$LEGAIA_RELEASE_CACHE`, defaulting to
`~/.cache/legaia-release`. Deleting that directory forces a clean reinstall on
the next run.

zig arrives through the `ziglang` pip wheel rather than apt, installed into a
venv inside the cache. The venv is what sidesteps PEP 668's
"externally managed environment" refusal on Debian and Ubuntu without
resorting to `--break-system-packages`. zig is used purely as a cross-linker
with bundled glibc stubs; no Zig code is involved.

## Interaction with `main-ci.yml`

`main-ci.yml` does **not** trigger on tags - the `v*` tag event belongs to
`release.yml` alone. Two workflows firing on one tag would contend for the
single self-hosted runner and double the wall-clock cost of a release for no
added signal.

The release stays test-gated regardless: `release.yml`'s `verify` job
reproduces the same `cargo fmt --check`, `cargo clippy -D warnings` and
`cargo test --workspace --release` gates that `main-ci.yml` runs, and the
build job `needs` it. Disc-gated tests skip in `verify` exactly as they do in
CI, because `LEGAIA_DISC_BIN` is not set there.

## Local rehearsal

Both scripts run outside CI, which is the fastest way to check a matrix change
before tagging:

```bash
scripts/ci/setup-cross-toolchain.sh x86_64-pc-windows-gnu
scripts/ci/release-build.sh 0.0.0-test x86_64-pc-windows-gnu
```

The archive and its `.sha256` land in `target/dist/`, which is inside the
already-gitignored `target/`, so a rehearsal leaves no untracked files behind.
Pass a third argument to choose a different output directory. The workflow's
only extra step is folding the per-archive `.sha256` files into the aggregate
`SHA256SUMS`.
