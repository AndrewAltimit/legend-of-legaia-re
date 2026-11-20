# legaia-release-version

The version every shipped program shows and reports: each command-line
tool's `--version`, `legaia-engine --version`, the launcher's header,
Explorer's `--version` and About box, and the version line of every copied
bug report.

`scripts/ci/release-build.sh` exports `LEGAIA_RELEASE_VERSION` (the release
tag's version) before it builds, and `VERSION` reads it at compile time. A
build made any other way, such as a local `cargo build`, names the
workspace's crate version instead, which is not a release number, and
`RELEASE` is `None`.

`scripts/ci/build-wasm.sh` sets the same variable for the site's wasm
bundles from the `v*` tag on the commit it builds
(`scripts/ci/release-version.sh`), so the netplay room's build check
(`legaia_engine_netplay::room::engine_build`, which keys on `RELEASE`)
admits a desktop release and the site's play page of that release alike.

## What decides that a module sits here

Nothing but the version string. The crate has no dependencies, so a format
crate's binary takes it (behind its `cli` feature) without pulling anything
else into its build.

## See also

- [`docs/tooling/releases.md`](../../docs/tooling/releases.md) - how a
  release is built, and the smoke check that holds every program to the
  release's version.
