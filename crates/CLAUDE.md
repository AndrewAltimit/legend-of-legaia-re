# crates/ - working notes

Loaded when you work under `crates/`. The crate map (one row per crate, both tracks) is [`README.md`](README.md); each crate's own `README.md` is its scope page.

## Workspace rules

- Every dependency, internal or external, is declared once in the root `[workspace.dependencies]` and taken as `foo.workspace = true` - never a per-crate `path =` or version pin.
- A crate's CLI binary sits behind its default-on `cli` feature (`required-features = ["cli"]`); the workspace table lists those crates with `default-features = false`, keeping CLI deps out of library builds and the wasm bundle.
- No crate re-exports another crate's modules; each `Cargo.toml` lists exactly the workspace crates its sources name.
- Engine kernel-crate edges are a committed table, `scripts/ci/crate-layering.toml` (gate `check-crate-layering.py`): a new edge is a row with its reason ([`engine.md`](../docs/subsystems/engine.md#the-layering-gate)).
- Naming: `legaia-foo` / `legaia_foo` for game-specific crates, `psx-foo` / `psx_foo` for the generic PS1 ones; directories carry no prefix.

## Adding or changing a crate

- A new crate gets a row in [`README.md`](README.md) (the crate-map generator reads it), a place in `scripts/ci/crate-map-layout.toml`, then `python3 scripts/ci/gen-crate-map.py` to regenerate `site/crate-map.json` and the README diagrams; `--check` is a CI gate.
- A new public module also needs `gen-crate-map.py` re-run.
- Tests: a crate with several `tests/*.rs` builds them as one `integration` binary (`cargo test -p <crate> --test integration foo::`); engine-core, engine-scene(-host), engine-session, parity and web-play mount `tests/*.rs` into the unit-test target instead (`--lib integration::foo::`).
- Shared kernels have tests in the hosts too: a change to a kernel `web-play` / `web-viewer` / `engine-render` use should run their tests, not just the kernel crate's.
