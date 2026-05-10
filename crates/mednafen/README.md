# `legaia-mednafen`

Mednafen save-state parser + watchpoint-equivalent automation toolkit.

## Scope

- Parse gzipped `MDFNSVST` save states (`.mc{0..9}` files in
  `~/.mednafen/mcs/`).
- Index PSX-module sections (`MAIN`, `GPU`, `SPU`, `CDC`, …) and resolve
  `MAIN.MainRAM.data8` as 2 MiB of main RAM.
- Diff main RAM between two snapshots - coalesce per-byte changes into
  contiguous "regions" with PSX virtual addresses, suitable for handing to
  Ghidra to look up writers.
- Bisect a sequence of snapshots to find when a target address crossed a
  predicate boundary (zero → nonzero, etc.).
- A declarative scenario manifest (`scripts/mednafen/scenarios.toml`) maps
  each save slot to a labelled scenario with watchpoint regions; the CLI's
  `watch` subcommand runs all configured watchpoints against sister
  scenarios in one shot.

## CLI

```text
mednafen-state info SAVE              # section table + PSX register snapshot
mednafen-state extract SAVE [--start ADDR --end ADDR --out PATH]
mednafen-state diff LEFT RIGHT [--start ADDR --end ADDR --json PATH]
mednafen-state bisect --addr ADDR SAVE...
mednafen-state trace  --addr ADDR SAVE...
mednafen-state watch LABEL [--manifest PATH]
mednafen-state scenarios [--manifest PATH]
```

See `docs/tooling/mednafen-automation.md` for the full workflow.

## Why "watchpoint-equivalent"?

PCSX-Redux and mednafen both have interactive memory-watchpoint debuggers,
but neither exposes a scriptable interface. The pragmatic substitute is
to take save states at progressive points during a sequence (mc1 → mc2 →
mc3 during a scene load) and diff the RAM regions of interest. Anything
that changed was written by code that ran in the gap. The diff output
gives addresses that map directly back to Ghidra's "Find references to
this address" search.

This crate exists to make that workflow scriptable.

## Composition

- Library API for engine-side tools that want to read live RAM out of a
  save state (e.g. validating an in-engine VM trace against the retail
  result).
- CLI binary for the per-PR manual workflow.
- Disc-gated integration tests under `tests/real_saves.rs` skip cleanly
  when `LEGAIA_MEDNAFEN_DIR` is unset.

## Sony-IP boundaries

Save states capture the user's runtime memory, which contains Sony-owned
bytes. The crate ships with no fixtures; tests that read real saves are
behind `LEGAIA_MEDNAFEN_DIR` and skip-pass without it.
