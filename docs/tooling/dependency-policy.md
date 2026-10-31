# Dependency policy

Every input the repository does not author is pinned to an exact, reviewed
version, and a version is only taken once it has been public long enough for a
compromised release to have been noticed and yanked. One gate checks the Rust
dependency graph against the RustSec advisory database.

## The publish cooldown

A crate version, action release or image is not taken until it has been
published for **14 days**. A malicious release is typically found and yanked
within days; a cooldown means the tree never contains one during that window.

For a lockfile refresh that means: run `cargo update`, then compare every
version that changed against its publish date on crates.io
(`https://crates.io/api/v1/crates/<name>/versions`, field `created_at`). A
version inside the window is pinned back to the newest release that is old
enough and still semver-compatible:

```bash
cargo update -p <crate>@<too-new-version> --precise <older-version>
```

Repeat until nothing in the lockfile is inside the window - pinning one crate
back can pull a sibling of the same family (the `icu_*` / `zerovec` / `yoke`
set moves together) back with it, or refuse until that sibling moves first.

When every release that is old enough is **yanked**, the version already in
the lockfile stays. A yanked version keeps resolving from a committed
lockfile; the advisories gate reports it as a warning, not a failure (see
[`deny.toml`](../../deny.toml)).

## What is pinned, and where

| Input | Pin |
|---|---|
| Rust crates | `Cargo.lock` (committed). Requirements live once in the root `[workspace.dependencies]`. |
| GitHub Actions | Only GitHub's own `actions/*`; every `uses:` names a full commit SHA, the release tag as a trailing comment. Anything else runs through the runner's `gh` CLI (the release upload). |
| CI tools | `cargo install <tool> --version <x.y.z> --locked` - an exact crates.io version, built from the lockfile it ships with. No `curl \| sh` installers. |
| Ghidra container | `docker/ghidra.Dockerfile` builds `FROM blacktop/ghidra:<tag>@sha256:<digest>`. |
| Site scripts | The static site loads no third-party script; the only external request is the Google Fonts stylesheet. |

To resolve an action tag to its commit, dereference an annotated tag:

```bash
gh api repos/<owner>/<repo>/git/ref/tags/<tag> --jq '.object.type + " " + .object.sha'
# type "tag": one more hop
gh api repos/<owner>/<repo>/git/tags/<sha> --jq .object.sha
```

To move the Ghidra pin, read the new tag's index digest with
`docker buildx imagetools inspect blacktop/ghidra:<tag>` and change the tag and
the digest together.

## The advisories gate

```bash
cargo deny check advisories
```

reads `Cargo.lock` against the RustSec database and fails on any vulnerability,
unsoundness or unmaintained notice for a crate in the graph, on every target
platform. It runs in the `gates` job of
[`.github/workflows/main-ci.yml`](../../.github/workflows/main-ci.yml), which
installs `cargo-deny` at a pinned version. It is not in the pre-commit hook:
it fetches the advisory database over the network, and a new advisory is news
about the world, not about the commit being made.

[`deny.toml`](../../deny.toml) at the repository root configures it. An
advisory is fixed by updating the crate (directly, or by moving the parent that
pulls it in - `cargo tree -i <crate> --target all` names it). An ignore is the
last resort and is only acceptable for an **unmaintained** notice whose crate
reaches the tree through a third-party dependency with no maintained
alternative; each ignore carries a comment naming that parent, so it can be
dropped when the parent moves. A vulnerability or unsoundness advisory is never
ignored.

## Major-version bumps

A bump across a major version lands as its own commit, so it can be dropped
without the others. A bump that adds a second copy of a crate to the graph
(because another dependency still pins the old major) and fixes nothing is not
taken: the duplicate costs build time and binary size for no gain.
