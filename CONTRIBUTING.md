# Contributing

Thanks for looking. This page covers the rules of engagement; [`CLAUDE.md`](CLAUDE.md) is the full repository map (which doc covers what, which crate owns what) and is worth skimming before you start.

## The one hard rule: no Sony bytes

**Nothing Sony owns gets committed to this repository.** Not the executable, not asset data, not decompressed output, not decompiled C that carries literal data or text strings. Everything here operates on a disc image the user supplies themselves.

In practice:

- `extracted/`, `ghidra/projects/`, `ghidra/scripts/funcs/`, and exported translation packs are gitignored. Keep it that way.
- Anything that needs real disc bytes goes behind the `LEGAIA_DISC_BIN` skip-gate (below). Tests must pass with the variable unset.
- Quoting a handful of bytes in a doc to explain a header layout is fine. Pasting a decompiled function that embeds a string table is not.

If a change would put Sony bytes in git history, it doesn't land, however useful it is. This constraint is what keeps the project legally in the same position as ScummVM and OpenRCT2.

## Getting set up

```bash
cargo build --release
scripts/ci/install-hooks.sh   # once per clone
```

The hook installer points `core.hooksPath` at the repo's hooks so the CI gates run before each commit. Set `LEGAIA_SKIP_PRECOMMIT=1` to bypass in an emergency.

## The gates

CI is strict - warnings are failures. Run these before pushing:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace --release
```

Docs have their own gate. `scripts/ci/check-doc-density.py` is a legibility linter over `docs/` and the crate READMEs: no lines over 800 characters, no markdown table cell over 150 words. It runs pre-commit on staged docs.

```bash
python3 scripts/ci/check-doc-density.py
```

## Disc-gated tests

Tests that touch a real disc read `LEGAIA_DISC_BIN`:

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace --release
```

With it unset, they **skip and pass**. That's deliberate - it's what lets CI run without disc data - so don't "fix" a skipping test by removing the gate. Find them with `grep -rl LEGAIA_DISC_BIN crates/*/tests`.

If you're adding one, follow the shape of its neighbours: a disc round-trip oracle asserts invariants survive a patch/re-decode cycle, and a runtime oracle drives the actual engine kernel rather than a save-state RAM cache. Keep a non-disc baseline assertion so the test isn't vacuous when it does run.

## Reverse-engineering work

Read the relevant [`docs/formats/`](docs/formats/overview.md) page before writing a parser - don't infer the layout from the data alone. Several of these formats look like their standard PSX counterparts and aren't.

Findings need provenance. Cite the function dump (`see ghidra/scripts/funcs/<addr>.txt`) or the entry (`FUN_801XXXXXX in PROT entry NNNN_<name>`). If you pin something notable, add it to [`docs/reference/functions.md`](docs/reference/functions.md).

[`CLAUDE.md`](CLAUDE.md) has a "Cross-cutting facts that catch people out" section covering the traps that recur - the MIPS LUI+ADDIU pairs Ghidra won't resolve, the CDNAME +2 index shift, why "LZS decompresses without error" proves nothing, and the three distinct pack formats. Skim it before chasing a "why is X broken" thread. [`docs/reference/open-rev-eng-threads.md`](docs/reference/open-rev-eng-threads.md) tracks open questions *and* falsified hypotheses - check it so you don't re-walk a dead end.

## Code conventions

- Crate naming: package `legaia-foo`, lib `legaia_foo`. Internal deps go through workspace path entries.
- Prefer a new subcommand on an existing per-crate binary over a new binary, unless the tool genuinely spans crates. The pattern is `clap` derive plus a subcommand enum at the top of `bin/<name>.rs`.
- Tag ported functions with `// PORT: FUN_<addr>` and cross-references with `// REF: FUN_<addr>`. The [port catalog](docs/tooling/port-catalog.md) reads these.

## Writing docs

The docs under `docs/` are a **technical reference**, not a changelog. They describe what a format or subsystem *is*, not the history of figuring it out.

- Present tense. No dates, no session numbers, no "recently added" markers.
- No rot-prone counts of project state - test counts, crate counts, coverage percentages. Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine and encouraged.
- No progress trackers or status tables. Operational state lives in git log and PR descriptions.
- If you change a fact, fix every page that repeats it - including the index pages and the site.

## Licensing

Contributions are accepted under the repository's dual [Unlicense](LICENSE) / [MIT](LICENSE-MIT) terms. Apache-2.0 is intentionally not offered; see the root [`README.md`](README.md#status-and-license) for the reasoning.
