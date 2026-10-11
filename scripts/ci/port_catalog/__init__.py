"""Port-catalog tracker: per-address status across decompilation and engine port.

Generates a unified per-function table with three independent status columns:

  - dumped     : a Ghidra decompiler dump exists under `ghidra/scripts/funcs/`
  - documented : the address is cited from at least one file under `docs/`
  - ported     : the address appears in a `// PORT: FUN_<addr>` tag in a Rust
                 source file under `crates/`. The tag is the source of truth -
                 grep-by-mention is too noisy because addresses get cited as
                 "not yet ported" or "inspired by" in port-progress comments.

Reuses helpers from `function-coverage.py` where useful: same dump-file naming
convention, same address regex shape (SCUS 0x80010000-0x8006FFFF and overlays
0x801C0000-0x8020FFFF).

PORT tag format (Rust source files under crates/). The scanner accepts the
tag inside any of three comment forms:

    // PORT: FUN_801dd35c                  -- plain line comment
    /// PORT: FUN_801dd35c                  -- outer doc-comment (attaches to next item)
    //! PORT: FUN_801dd35c, FUN_801cf244    -- inner doc-comment (module-level)

The dominant convention in this codebase is the `//!` module-level form: each
crate file leads with a doc block that lists every dispatcher / top-level
function ported into that module on one (or several) `//! PORT:` line(s). The
catalog counts each distinct `FUN_<addr>` only once across all tag forms, so a
quick sanity check is:

    grep -rEi '//[/!]?\\s*PORT\\s*:' crates/ | grep -oEi 'FUN_8[0-9a-f]{7}' | sort -u | wc -l

A plain `grep -r "// PORT:"` under-counts dramatically - most tags live in
`//!` doc blocks that don't contain the literal substring `"// "`.

Trailing context after the address list is allowed:

    //! PORT: FUN_801dd35c (sub-mode jump table)

Addresses are matched strictly to the SCUS/overlay range (0x80010000-0x8006FFFF
and 0x801C0000-0x8020FFFF) so unrelated text in the tail of a tag won't
false-positive.

Usage:
    python3 scripts/ci/port-catalog.py                       # build global catalog
    python3 scripts/ci/port-catalog.py --missing-ports       # dumped+documented, not ported
    python3 scripts/ci/port-catalog.py --missing-dumps       # cited but not dumped
    python3 scripts/ci/port-catalog.py --md                  # markdown table to stdout
    python3 scripts/ci/port-catalog.py --top 30              # cap rows
    python3 scripts/ci/port-catalog.py --addr 801dd35c       # single-row drill-down
    python3 scripts/ci/port-catalog.py --dashboard           # open-work rollup (single page)

Output (default): target/port-catalog/catalog.csv + catalog.md
"""
