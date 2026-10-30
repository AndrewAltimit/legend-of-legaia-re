# Tagged-release binary pipeline

Pushing a `v*` tag builds the workspace binaries and attaches them to that
tag's GitHub release. There is no crates.io publishing step: GitHub release
artifacts are the only distribution channel.

The release archives carry **compiled binaries and our own text files only**.
No game data is packaged, no step reads a disc image, and nothing in the
pipeline may change that - the same rule that governs the rest of the repo.

| Piece | Lives in | Role |
|---|---|---|
| Release workflow | [`.github/workflows/release.yml`](../../.github/workflows/release.yml) | Tag trigger, test gate, the two build jobs, publish |
| Toolchain provisioning | [`scripts/ci/setup-cross-toolchain.sh`](../../scripts/ci/setup-cross-toolchain.sh) | Per-target cross toolchain, idempotent |
| Build + package | [`scripts/ci/release-build.sh`](../../scripts/ci/release-build.sh) | Per-target build, staging, archive, checksum |

## Cutting a release

```bash
git tag v0.2.0
git push origin v0.2.0
```

That is the whole procedure. The workflow runs the test gate, builds each
target, and creates the release for the tag if it does not already exist,
attaching the archives to it.

The workflow has four jobs:

| Job | Runner | Does |
|---|---|---|
| `verify` | self-hosted | `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` |
| `build-cross` | self-hosted (arm64 Linux) | the two Linux targets and Windows; uploads the archives as a workflow artifact |
| `build-macos` | GitHub-hosted `macos-14` | the universal macOS archive; uploads it as a workflow artifact |
| `publish` | GitHub-hosted `ubuntu-latest` | `needs` all three; checks every archive's hash, writes `SHA256SUMS`, makes the one release edit |

The build jobs never touch the release. Only `publish` does, and it waits for
the test gate and for **both** build jobs, so a release never appears with a
platform missing and `SHA256SUMS` always covers every archive. `publish` also
fails if any of the four expected archive names is absent. `build-macos` runs
in parallel with `verify` rather than after it: the repo is public, so the
hosted macOS minutes are free, and nothing it builds is published until the
gate is green. A tag containing `-` (say `v0.2.0-rc1`) is
marked as a prerelease.

To rehearse without touching a release, use the `workflow_dispatch` trigger
(Actions tab, or `gh workflow run release.yml -f tag=<ref>`). It runs every job
- gate, all four builds, checksum aggregation - and leaves the archives as
workflow artifacts, but **does not publish** unless the `publish` input is
checked, so the dispatch path is a safe dry run. A dry run takes any ref, a
branch included (a `/` in it becomes `-` in the archive names); publishing
takes only an existing tag, and `publish` refuses anything else before the
release step, since the release action would otherwise mint a new tag.

## Target matrix

The self-hosted runner is **arm64 Linux**, which makes both other targets
cross-compiles - including the x86_64 Linux one.

| Target | Toolchain | Contents |
|---|---|---|
| `aarch64-unknown-linux-gnu` | Native | `Legend of Legaia`, `Legaia Explorer`, every command-line binary |
| `x86_64-pc-windows-gnu` | mingw-w64 cross | `Legend of Legaia.exe`, `Legaia Explorer.exe` (both GUI-subsystem), every command-line binary |
| `x86_64-unknown-linux-gnu` | `cargo-zigbuild` cross + amd64 ALSA / libudev sysroot, glibc pinned to 2.28 | As `aarch64-unknown-linux-gnu` |
| `universal-apple-darwin` | Native on a macOS runner: `aarch64-apple-darwin` + `x86_64-apple-darwin`, fused by `lipo` | `Legend of Legaia.app`, `Legaia Explorer.app`, every command-line binary |

The per-target binary list is declared explicitly in `release-build.sh`, and
the script fails if an expected binary is missing from the build output, so a
target can never quietly drop one.

## Archive layout

The top folder of an archive holds only what a user should open: the game and
Legaia Explorer, `README-PLAY.txt` and the license files. Every command-line
program sits in `command-line-tools/` below it, so a download makes plain
what is meant to be double-clicked:

```
legaia-tools-<version>-<target>/
    Legend of Legaia            (.exe on Windows, .app on macOS)
    Legaia Explorer             (.exe on Windows, .app on macOS)
    README-PLAY.txt
    LICENSE  LICENSE-MIT  THIRD-PARTY-LICENSES.txt
    packs/
        rebirth.lxpack          the Rebirth expansion pack
    command-line-tools/
        README.txt              every tool, one line each
        legaia-engine           the engine: launcher, play-window, every subcommand
        asset-viewer
        legaia-extract  legaia-patcher  asset  tim  tmd  vab  ...
```

`Legend of Legaia` on Linux and Windows is the `legaia-launch` stub
(`crates/engine-shell/src/bin/legaia-launch.rs`): it starts
`command-line-tools/legaia-engine` with the same arguments - on Linux by
`exec`, so a terminal sees the engine's own output and exit status; on
Windows as a GUI-subsystem child ([below](#windows-the-console-window)). The
engine stays in the tools folder because it is also the archive's largest
command-line program; one copy serves both uses. `Legaia Explorer` is the
`legaia-explorer` binary under a readable name. On macOS both are `.app`
bundles and no bare GUI binary sits at the top.

The stub and Explorer's *Play* button find the engine through
`legaia_app_theme::release_layout::find_engine`: in each of the four folders
above the program, the engine in that folder (a `cargo build` puts every
binary side by side), in its `command-line-tools/`, then inside a `Legend of
Legaia.app` there. Explorer inside `Legaia Explorer.app/Contents/MacOS/`
reaches the archive folder three levels up. The release smoke-runs the staged
`Legend of Legaia --version` on the native row, which exercises that lookup.

### The bundled expansion pack

`packs/rebirth.lxpack` is [`packs/rebirth/`](../../packs/rebirth/README.md)
bundled by `asset expansion bundle` while the archive is staged. The bundler
is the format's one deterministic writer, so every archive carries the same
bytes, and it refuses a pack that fails validation - the
[no-disc-bytes rule](../formats/expansion-pack.md#the-no-disc-bytes-rule)
among the checks - so what ships is disc coordinates plus the pack's own art
and audio. A cross target cannot run its own `asset`, so its row bundles with a
host build; the native and universal macOS rows use the staged binary.

The game finds the folder through
`legaia_app_theme::release_layout::find_bundled_packs`: the first `packs/`
folder in the four folders above the running program, the same walk as
`find_engine`, which reaches the archive's top level from
`command-line-tools/` and from inside a `.app`. A `cargo build` in
`target/<profile>/` reaches the checkout's `packs/` the same way. The
launcher's *Expansion pack* button steps through the packs it finds there
and in the per-user packs folder (an installed copy of a pack id hides the
bundled one), `legaia-engine packs` lists both folders, and
`play-window --expansion rebirth` resolves the id in the same order.

## Icons

The game and Legaia Explorer carry their own icons, original artwork in
[`crates/app-theme/icons/`](../../crates/app-theme/README.md#icons-and-where-the-release-puts-the-programs)
(no game asset is drawn from). `make_icons.py` there draws them and writes the
SVG, PNG, `.ico` and `.icns`; the outputs are committed, so a release build
never runs it.

| Where | How |
|---|---|
| Windows executables | `build.rs` in `engine-shell` and `explorer` calls `legaia_app_theme::win_icon::embed_in_bins`, which compiles a one-icon resource with `x86_64-w64-mingw32-windres` (part of mingw-w64, already a provisioning requirement) and links it into `Legend of Legaia.exe`, `legaia-engine.exe` and `Legaia Explorer.exe`. Other targets skip it; no crate dependency is added. |
| macOS bundles | `make_app` copies the `.icns` into `Contents/Resources/` and names it in `CFBundleIconFile`. |
| Window icons | The engine (`legaia_engine_render::window::set_app_icon`, from the PNG) and Explorer (`ViewportBuilder::with_icon`) set it at runtime: the title bar and task switcher on Windows and X11. macOS shows the bundle's icon and Wayland the desktop entry's, so neither takes a window icon. |

`release-build.sh` reads the `.rsrc` section of the three Windows executables
and fails unless each carries an icon group (`RT_GROUP_ICON`), so a build host
without a resource compiler, which only warns at build time, cannot ship
iconless programs.

### macOS

Apple's SDK cannot be redistributed onto a Linux runner, so macOS builds on a
GitHub-hosted macOS runner instead. `universal-apple-darwin` is not a rustc
triple; it is the release's name for "both macOS slices in one file".
`release-build.sh` builds the workspace for `aarch64-apple-darwin` and
`x86_64-apple-darwin`, then `lipo -create` fuses each binary into
`target/universal-apple-darwin/release/`, and the rest of the script stages it
like any other target. The deployment target is macOS 11, the arm64 floor.
cpal's macOS backend is CoreAudio, so there is no ALSA question here.

The archive's top folder carries `Legend of Legaia.app`, a minimal bundle
(an `Info.plist`, the icon and a copy of `legaia-engine`) so a Finder user
double-clicks an app. Started without arguments the engine runs its launcher,
which needs no terminal. `Legaia Explorer.app` wraps `legaia-explorer` the
same way; one `make_app` helper in the script builds, lints and signs both. `CFBundleVersion` takes the version with any prerelease suffix cut
(macOS wants dot-separated integers there); `CFBundleShortVersionString` keeps
the full string. The script lints the `Info.plist` with `plutil` and verifies
the seal with `codesign --verify --deep --strict` after signing, so a malformed
bundle fails on the runner. The bundle is **ad-hoc signed**
(`codesign --sign -`): arm64 macOS
refuses to run unsigned code at all, and an ad-hoc seal satisfies that. It is
not notarised - that needs a paid Apple Developer identity - so Gatekeeper
asks the user to confirm the first launch; `README-PLAY.txt` gives the steps.

The script stays inside bash 3.2 and BSD userland on this path: archives are
written with plain `tar -czf` when the host tar is not GNU tar (the GNU
normalising flags do not exist on BSD tar), and checksums fall back to
`shasum -a 256`, which writes the same `hash  name` lines as `sha256sum`.

### Windows: the console window

`legaia-engine.exe` is a **console** program, and a console program started
from Explorer gets a console window of its own. The Windows archive therefore
carries `Legend of Legaia.exe` at its top level - the `legaia-launch` binary
renamed at staging - a `windows`-subsystem stub that starts
`command-line-tools\legaia-engine.exe` with `CREATE_NO_WINDOW` and returns its
exit status. Being a GUI program, the
stub never gets a console, and the engine (and the game process its launcher
spawns, which inherits the engine's hidden console) never shows one. A failure
to start the engine, or a non-zero exit, is reported in a message box, since
the stub has nowhere to print. `README-PLAY.txt` points players at the stub.

The alternative - `#![windows_subsystem = "windows"]` on the engine itself,
plus `AttachConsole(ATTACH_PARENT_PROCESS)` to reach the terminal for CLI use
- was rejected because it breaks the terminal contract every subcommand
relies on: `cmd.exe` does not wait for a GUI-subsystem program, so output lands
after the next prompt and `%ERRORLEVEL%` is lost, and the engine's trace
subcommands are pipe-heavy. A second full GUI build of the engine would double
the archive's largest file. The stub costs a few hundred kilobytes.

A double-click on `legaia-engine.exe` itself is covered too: with no
subcommand, when the engine is its console's only process (Explorer made the
console for it), it calls `FreeConsole` and starts the game child with
`CREATE_NO_WINDOW`. That leaves a brief flash, which is why the stub exists;
`LEGAIA_KEEP_CONSOLE` keeps the console for debugging. Started from a
terminal, the console is shared and stays.

`release-build.sh` reads the PE optional header of the staged files and fails
unless the stub is subsystem 2 (Windows GUI) and the engine subsystem 3
(console), so a build change cannot quietly bring the console back or take the
terminal away. The same check holds `Legaia Explorer.exe` at subsystem 2 (see
below), and the [icon check](#icons) runs over the same three files.

### Legaia Explorer

`legaia-explorer` ([`crates/explorer`](../../crates/explorer/README.md)) is the
desktop asset browser and extractor, the archive's entry point for a user who
does not want a terminal. It ships on every target and differs from the other
GUI binaries in three ways the pipeline accounts for:

- **No console on Windows.** It has no command-line contract (one optional
  path to open, plus `--version` / `--help`), so unlike the engine it is a
  `windows`-subsystem program itself (`#![cfg_attr(windows,
  windows_subsystem = "windows")]`) and needs no launcher stub. Its
  `--version` prints only when a console is attached.
- **OpenGL, not wgpu.** It draws through `eframe` on the glow backend: it
  draws only egui's own meshes, so it links no wgpu at all. On Linux it opens
  libGL / libEGL and X11 / Wayland at runtime (dlopen), so the cross builds
  need nothing beyond the ALSA sysroot the engine already uses; on Windows it
  imports only system DLLs (`opengl32.dll` among them).
- **Its own bundle on macOS.** `Legaia Explorer.app` sits beside `Legend of
  Legaia.app`; Gatekeeper's first-launch confirmation applies to it the same
  way. On Linux and Windows it ships as `Legaia Explorer` / `Legaia
  Explorer.exe`, at the top level rather than in `command-line-tools/`.

`README-PLAY.txt` carries a "browse and extract" section that points at it,
and the release smoke-runs `legaia-explorer --version` next to the engine's.

### Why Windows is `-gnu` rather than `-msvc`

`x86_64-pc-windows-gnu` links through mingw-w64, which apt packages as an
arm64-hosted compiler emitting x86_64 PE objects. The whole workspace crosses
cleanly this way, GUI binaries included: wgpu, winit and cpal all build, and
cpal's Windows backend is WASAPI, so there is no ALSA dependency to satisfy.
The MSVC ABI would need `cargo-xwin` and a downloaded Microsoft SDK; the gnu
ABI produces working `.exe`s with a toolchain the runner already has.

### Why x86_64 Linux needs an ALSA sysroot

`legaia-engine` and `asset-viewer` reach cpal, whose Linux backend binds
`alsa-sys`. `alsa-sys` resolves `libasound` through `pkg-config` at build
time, so cross-compiling it to x86_64 needs an **x86_64** `libasound`. An
arm64 runner has only the arm64 one, and pkg-config correctly refuses to hand
a foreign-architecture library to an x86_64 link. `legaia-engine`'s
controller input (gilrs, behind its default `gamepad` feature) binds
`libudev-sys` the same way and needs an x86_64 `libudev`, so the sysroot
carries both. The rest of the workspace is ALSA- and udev-free and crosses
without any of this.

**apt cannot supply the x86_64 copy on an arm64 Ubuntu.** Arm64 Ubuntu serves
from `ports.ubuntu.com`, which publishes no amd64 packages at all, so the
usual multiarch incantation fails at fetch time:

```text
dpkg --add-architecture amd64 && apt update
  N: Skipping acquire of configured file 'main/binary-amd64/Packages' as
     repository '...ubuntu-ports... noble InRelease' doesn't support
     architecture 'amd64'
apt install libasound2-dev:amd64
  E: Unable to locate package libasound2-dev:amd64
```

The amd64 packages live on `archive.ubuntu.com` instead. Making multiarch work
would mean pinning the existing `ports` entries to `arm64` and adding a second
`archive.ubuntu.com` source restricted to `amd64` - a permanent, root-owned
rewrite of the runner's apt configuration.

`setup-cross-toolchain.sh` sidesteps that entirely: it fetches the four amd64
`.deb`s (`libasound2-dev`, `libasound2t64`, `libudev-dev`, `libudev1`) straight from `archive.ubuntu.com` and unpacks them with `dpkg -x`
into `$LEGAIA_RELEASE_CACHE/sysroot-amd64`. No root, no apt sources touched,
nothing installed system-wide. `release-build.sh` then points pkg-config at
that sysroot (`PKG_CONFIG_SYSROOT_DIR` + `PKG_CONFIG_LIBDIR`, with
`PKG_CONFIG_ALLOW_CROSS=1`).

Two details make it work:

- **Package versions are resolved from the `noble` suite index**, not
  hardcoded. Ubuntu drops superseded `.deb`s from the pool, so a literal
  filename would 404 the first time a security update landed. The suite is
  pinned to `noble` rather than tracking the host: linking against an older
  libasound is the safe direction, since a binary linked against a *newer* one
  can reference a symbol the user's runtime lacks.
- **`-Wl,--allow-shlib-undefined` is required.** libasound is built against a
  newer glibc than the 2.28 pin, so its own internal references
  (`pow@GLIBC_2.29`, `dlclose@GLIBC_2.34`, ...) cannot resolve in this link.
  They do not need to: libasound is a *shared* dependency, satisfied at
  runtime by the user's own copy and their glibc. The flag relaxes the check
  only for symbols undefined inside shared libraries - undefined references
  from our own objects still fail the link. The pin survives, and
  `release-build.sh` asserts it by reading the built symbol table
  (`objdump -T`) rather than trusting the target suffix.

The binaries record `NEEDED libasound.so.2` (and `legaia-engine` also
`libudev.so.1`) and bind them at runtime, so a user needs ALSA and udev present
- true of every mainstream desktop Linux. A sysroot built before libudev joined
it is topped up on the next run: the script keys on both `.pc` files.

Adding an x86_64 Linux runner would make all of this moot: the target stops
being a cross-compile and `setup-cross-toolchain.sh` skips both zig and the
sysroot as soon as it detects the host triple.

## Artifacts

Each target produces one archive plus an entry in a single aggregate
`SHA256SUMS`:

```
legaia-tools-<version>-aarch64-unknown-linux-gnu.tar.gz
legaia-tools-<version>-x86_64-pc-windows-gnu.zip
legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz
legaia-tools-<version>-universal-apple-darwin.tar.gz
SHA256SUMS
```

`<version>` is the tag with its leading `v` stripped, so `v0.2.0` yields
`legaia-tools-0.2.0-x86_64-pc-windows-gnu.zip`. Linux and macOS ship
`.tar.gz`, Windows ships `.zip`.

Every archive contains a single top-level
`legaia-tools-<version>-<target>/` directory - extracting one never scatters
files across the working directory. Inside it, laid out as
[above](#archive-layout): the programs, `LICENSE` and `LICENSE-MIT` (the
project is `MIT OR Unlicense`), and three generated text files:

- `README-PLAY.txt` (top level) - the file a player needs: which two programs
  to open and that `command-line-tools/` is not needed to play; start
  `Legend of Legaia`, pick your own Legend of Legaia (USA) disc image once in
  the launcher, set its display options, and press Play. It
  restates that no game data is included and where
  settings and saves live, and lists the keyboard controls (every pad
  button, the Escape-twice quit) and the display toggles. The launcher itself is described in
  [`playing-and-viewing.md`](../guides/playing-and-viewing.md#0-the-launcher).
  It also points asset users at Legaia Explorer and the extraction guide, and
  says how to play the bundled Rebirth pack.
- `command-line-tools/README.txt` - the target, every command-line program
  with a one-line role, and the same bring-your-own-disc rule.
- `THIRD-PARTY-LICENSES.txt` (top level) - the license texts and notices of every crate
  the binaries link (see below).

### Third-party notices

Most of the linked crates' licenses (MIT, Apache-2.0, BSL-1.0, Zlib,
Unicode-3.0, the OFL / Ubuntu font licenses of egui's bundled fonts, ...)
require their notice to travel with the binary.
[`scripts/ci/third-party-licenses.py`](../../scripts/ci/third-party-licenses.py)
writes the file at staging time from the crates themselves:

- **Which crates.** The closure of every workspace package that builds a
  binary over normal dependency edges, from `cargo tree --workspace -e normal
  --target <triple>` (both slices for the universal macOS row), so features
  resolve exactly as in the release build and build-only or dev-only crates
  are left out. `cargo metadata`'s own resolve is not used for the set: it
  lists optional dependencies no feature enables (eframe's unused wgpu
  backend).
- **Which texts.** The license files each crate ships (`LICENSE*`,
  `COPYING*`, `NOTICE*`, `COPYRIGHT*`, the manifest's `license-file`, and
  notices below the root such as a font's licence next to the font). An OR
  expression is satisfied by a branch the crate ships text for; when a crate
  ships none, the Apache-2.0 text (no copyright holder in its wording) or a
  canonical template (MIT, BSL-1.0, Zlib) is borrowed rather than another
  crate's copyright line. Public-domain dedications (CC0-1.0) need no text.
- **Shape.** Each distinct text once, with every crate that carries it listed
  above it, after a crate / version / license / repository table.

The script exits non-zero when a linked crate declares no license or an id
has no text anywhere, which fails the release. It needs only `cargo` and the
Python 3 standard library.

A build smoke-runs `legaia-engine --version`, the game entry's `--version`
(the stub on Linux, the bundle's binary on macOS) and Explorer's `--version`
from the staged archive whenever the build host can execute it (the native Linux row and the
universal macOS row), so a binary that cannot start fails the release rather
than a user.

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
| amd64 ALSA / libudev sysroot | `.deb`s unpacked into the cache, automatic | No |
| `libasound2-dev`, `libudev-dev` | Already present; the native build needs them | Yes - one-time |

`mingw-w64` is the only piece the pipeline cannot install for itself. Without
it the Windows target fails fast with a provisioning hint rather than a linker
error. `curl`, `gunzip` and `dpkg` are needed to build the ALSA sysroot and are
checked up front for the same reason.

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
`cargo test --workspace --profile release-test` gates that `main-ci.yml` runs, and the
`publish` job `needs` it. Disc-gated tests skip in `verify` exactly as they do in
CI, because `LEGAIA_DISC_BIN` is not set there.

## Local rehearsal

Both scripts run outside CI, which is the fastest way to check a matrix change
before tagging:

```bash
scripts/ci/setup-cross-toolchain.sh x86_64-pc-windows-gnu
scripts/ci/release-build.sh 0.0.0-test x86_64-pc-windows-gnu
```

On a Mac the same pair works for the macOS row:

```bash
scripts/ci/setup-cross-toolchain.sh universal-apple-darwin
scripts/ci/release-build.sh 0.0.0-test universal-apple-darwin
```

The archive and its `.sha256` land in `target/dist/`, which is inside the
already-gitignored `target/`, so a rehearsal leaves no untracked files behind.
Pass a third argument to choose a different output directory. The workflow's
only extra step is `publish` folding the per-archive `.sha256` files into the
aggregate `SHA256SUMS`.
