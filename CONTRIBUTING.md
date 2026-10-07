# Contributing

Thanks for looking. This page covers the rules of engagement: what will never land, which gates your change has to pass, and the conventions that make a change reviewable.

[`CLAUDE.md`](CLAUDE.md) is the full repository map - which doc covers what, which crate owns what - and is worth skimming before you start.

## What this project is

Two tracks share one Cargo workspace:

1. **Asset preservation.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** A from-scratch Rust port, written from the format docs and decompiled-C reference - the ScummVM / OpenRCT2 model, not a static recompilation of `SCUS_942.54`.

The end-user model is: ship the engine, the user supplies their own disc image, the engine extracts and runs it.

Retail behaviour is the measured ground truth for game logic and simulation. That does not make this a strict 1:1 remake - the engine also carries an enhancement layer (enhanced lighting, volumetric fog, the camera-occlusion fade, precise movement, alternate cameras, VR), and the randomizer and translation toolchains are deliberate, shipped features. The rule enhancements follow: they may ship **enabled by default** where they are clearly the better experience, but each is a toggle, and with it off the faithful mode stays bit-identical - so retail is always one switch away and the parity oracles keep passing. [`docs/subsystems/engine.md`](docs/subsystems/engine.md#fidelity-and-enhancements) is the authority on where the port boundaries sit and what each knob defaults to.

## The one hard rule: no Sony bytes

**Nothing Sony owns gets committed to this repository.** Not the executable, not asset data, not decompressed output, not decompiled C that carries literal data or text strings.

In practice:

- These paths are gitignored and stay that way: `extracted/`, `captures/`, `ghidra/projects/`, `ghidra/scripts/funcs/`, `saves/library/`, and exported translation packs (`translations/`, `legaia_*.yaml`).
- Anything that needs real disc bytes goes behind the `LEGAIA_DISC_BIN` skip-gate (below). Tests must pass with the variable unset.
- Quoting a handful of bytes in a doc to explain a header layout is fine. Pasting a decompiled function that embeds a string table is not.

If a change would put Sony bytes in git history, it doesn't land, however useful it is. This constraint is what keeps the project legally in the same position as ScummVM and OpenRCT2.

## Getting set up

```bash
cargo build --release
scripts/ci/install-hooks.sh   # once per clone
```

The installer points `core.hooksPath` at [`scripts/git-hooks/`](scripts/git-hooks/), so the hooks stay in version control and updating them just means pulling.

## The gates

CI is strict - warnings are failures. Run these before you push:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace --profile release-test
```

`release-test` is CI's test profile: `release`'s optimisation level without LTO, so a relink after a small change stays fast. To run one test file, select it by module path inside the crate's single `integration` test binary: `cargo test -p legaia-engine-core --profile release-test --test integration foo::`.

**Run them yourself.** The full CI job triggers on push-to-main and manual dispatch, not on pull-request events, so opening a PR does not automatically prove your branch is green.

CI also builds the workspace and the WASM target, which is easy to break from native-only code:

```bash
cargo build --release --workspace
cargo build --release --target wasm32-unknown-unknown -p legaia-web-viewer
```

The port ships three hosts on one engine - the native `play-window`, the browser play page, and the minigames page - so a feature wired into one host is invisible in a diff of the others. [`docs/tooling/host-drift.md`](docs/tooling/host-drift.md) covers the gates that catch that.

### What the pre-commit hook actually runs

The hook is scoped to what you staged, so a docs-only commit doesn't pay for a clippy run. The broad shape (the [hook itself](scripts/git-hooks/pre-commit) is the authority, and documents why each gate exists):

| Staged | Gates |
|---|---|
| any docs | `check-doc-density.py --staged` (legibility linter), `check-md-links.py --staged` (intra-repo links + heading anchors) |
| `site/` | internal links + anchors, CSS cascade order, the JS syntax / case-fold / play-page state checks, generated-page freshness |
| `scripts/vrc-diorama/` | `codegen.py --check` (generated register drift) |
| every commit | shell observer traps, integration-test declarations, and the disc-coverage / byte-accounting / dump-integrity / port-catalog ratchets (each self-skips when its gitignored input is absent) |
| `Cargo.toml`, `Cargo.lock`, `crates/`, `rust-toolchain*`, `.cargo/` | `cargo fmt --check`, `cargo clippy -D warnings`, UI host drift, trait-override symmetry, `check-port-tags.py` (warn-only) |
| the wasm-sensitive crates | the wasm32 type-check (`scripts/ci/check-wasm-target.sh`) |

Two things to know: a `cargo fmt` failure is **auto-fixed** - the hook runs `cargo fmt --all` and re-stages the `.rs` files it had staged, then asks you to review. And the hook does **not** run the test suite; that's the manual `cargo test --workspace --profile release-test` above.

Set `LEGAIA_SKIP_PRECOMMIT=1` to bypass in an emergency.

Two gates cover the docs, and both scope to `docs/**/*.md`, each `crates/*/README.md`, and the top-level `*.md` (this file, `README.md`, `CLAUDE.md`).

[`check-doc-density.py`](scripts/ci/check-doc-density.py) caps lines at 800 characters and table cells at 150 words. These are backstops against unreadable walls of text, **not targets to write up to** - prose that lands at 799 characters was written for the linter rather than for a reader.

[`check-md-links.py`](scripts/ci/check-md-links.py) resolves every relative link and `#anchor` against the file it points at. It exists because a dead fragment renders as a jump to the top of the page instead of an error, so broken anchors survive review indefinitely. When a link and a heading disagree, **fix the link**: the heading is an anchor target that other pages - and the site's hand-mirrored HTML - already point at.

Audit the whole corpus with:

```bash
python3 scripts/ci/check-doc-density.py
python3 scripts/ci/check-md-links.py
```

## Disc-gated tests

Tests that touch a real disc read `LEGAIA_DISC_BIN`:

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace --profile release-test
```

With it unset, they **skip and pass**. That's deliberate - it's what lets CI run without disc data - so don't "fix" a skipping test by removing the gate. Find them with `grep -rl LEGAIA_DISC_BIN crates/*/tests`.

Every push to a pull request runs the disc-free repo gates plus `cargo fmt`, `cargo clippy` and a wasm32 type-check; the full test pass runs after merge. The disc-gated suite runs as a separate job, on manual dispatch or when a maintainer adds the `disc-test` label to a pull request. If your change touches disc-gated behaviour, say so in the PR so the label gets applied - and run the suite locally against your own image first.

If you're adding one, follow the shape of its neighbours: a disc round-trip oracle asserts invariants survive a patch/re-decode cycle, and a runtime oracle drives the actual engine kernel rather than a save-state RAM cache. Keep a non-disc baseline assertion so the test isn't vacuous when it does run.

## Reverse-engineering work

Read the relevant [`docs/formats/`](docs/formats/overview.md) page before writing a parser - don't infer the layout from the data alone. Several of these formats look like their standard PSX counterparts and aren't.

Port and document from the **disassembly**, not the decompiled C: the C is a rendering, and its artifacts (label-calls, dropped register arguments, reordered stores) have each put a false claim into these docs before. [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) lists them.

Findings need provenance. Cite the function dump (`see ghidra/scripts/funcs/<addr>.txt`) or the entry (`FUN_801XXXXXX in PROT entry NNNN_<name>`). If you pin something notable, add it to [`docs/reference/functions.md`](docs/reference/functions.md).

[`CLAUDE.md`](CLAUDE.md) has a "Cross-cutting facts that catch people out" section covering the traps that recur - the MIPS LUI+ADDIU pairs Ghidra won't resolve, the CDNAME +2 index shift, why "LZS decompresses without error" proves nothing, and the one pack format with two readers. Skim it before chasing a "why is X broken" thread. [`docs/reference/open-rev-eng-threads.md`](docs/reference/open-rev-eng-threads.md) tracks the open questions, [`re-settled-threads.md`](docs/reference/re-settled-threads.md) the answered ones with their evidence grade, and [`re-do-not-re-walk.md`](docs/reference/re-do-not-re-walk.md) the falsified hypotheses - check the last so you don't re-walk a dead end.

## Code conventions

- Crate naming: package `legaia-foo`, lib `legaia_foo`. Every dependency, internal or external, is declared once in the root `[workspace.dependencies]` and taken as `foo.workspace = true` - never a per-crate `path =` or version pin.
- A crate's command-line binary sits behind its default-on `cli` feature (`required-features = ["cli"]`, with `clap` optional), so library builds and the wasm bundle stay free of CLI dependencies.
- A crate's integration tests build as one binary: `tests/foo.rs` is a `mod foo;` of `tests/integration.rs`, and a file without that line is never compiled (`check-integration-tests.py` fails the commit).
- Prefer a new subcommand on an existing per-crate binary over a new binary, unless the tool genuinely spans crates. The pattern is `clap` derive plus a subcommand enum at the top of `bin/<name>.rs`.
- Tag ported functions with `// PORT: FUN_<addr>` and cross-references with `// REF: FUN_<addr>`. The [port catalog](docs/tooling/port-catalog.md) reads these.

## Writing docs

The docs under `docs/` are a **technical reference**, not a changelog. They describe what a format or subsystem *is*, not the history of figuring it out.

- Present tense. No dates, no session numbers, no "recently added" markers.
- No rot-prone counts of project state - test counts, crate counts, coverage percentages. Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine and encouraged.
- No progress trackers or status tables. Operational state lives in git log and PR descriptions.
- If you change a fact, fix every page that repeats it - including the index pages and the site. The site's page bodies under `site/_content/` are hand-mirrored from `docs/`, so the two copies rot independently if you only fix one.

## Licensing

Contributions are accepted under the repository's dual [Unlicense](LICENSE) / [MIT](LICENSE-MIT) terms. Apache-2.0 is intentionally not offered; see the root [`README.md`](README.md#status-and-license) for the reasoning.
