"""Provenance checker for `// PORT: FUN_<addr>` tags.

`port-catalog.py` answers two questions about a port tag: does it **exist**,
and is the symbol carrying it **reachable** from a host root. Neither question
is "does that address name the routine this Rust code actually implements".
Nothing gated that, so a live, tested, shipped port could wear a wrong address
with every gate green - and `disc-coverage.py` would then count the wrong
retail bytes as covered.

The worked example, and the reason this file exists:
`engine-menus::shop::shop_stock_row_ink` carried `PORT: FUN_801D5DE0` while live
on both hosts. That routine is the **casino prize list's** row renderer - it
forms the prize table `0x801E4518` and gates affordability on the coin bank
`0x800845A4`. The mis-attribution came from the dump filename
`overlay_shop_save_801d5de0.txt`, whose prefix names the *image the routine was
dumped from*, not what the routine does.
[`docs/tooling/dump-corpus-integrity.md`](../../docs/tooling/dump-corpus-integrity.md)
already states that law for load bases; this checker applies it to function
identity.

## What it measures

Four signals. The two disassembly-side ones read the **disassembly** and never
the decompiled C (see
`docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims`):
for every PORT-tagged address the checker recovers the data addresses the
routine forms (`lui`+`addiu`/`ori`, load/store displacements against a register
whose provenance is a `lui`, and the `lui`/`addu`/`lw` indexed jump-table
idiom) plus the `jal` targets it calls.

`module-orphan`
    A module PORT-tags several routines, and one of them shares **no**
    distinctive data address, **no** distinctive callee and no call edge with
    any of its siblings, while the siblings do corroborate each other. Ranked,
    not accused - a module may legitimately port an unrelated kernel. What
    lifts a row is the corroboration line: the same rare table turning up under
    a module with an unrelated name. That conjunction is the shop shape, and
    `shop.rs` / `FUN_801D5DE0` reproduces it.

    "Distinctive" is measured at a cut that follows the module, because a fixed
    one is blind to a whole kind of module rather than to a random sample of
    them: a painter's only private datum is its own string pool and a host
    dispatcher's members are unrelated by design, so both report one orphan per
    member. See `DF_LADDER`.

`dual-label`
    One routine given a defining description - a `###` section, or a row in the
    function directory - on two docs pages whose names share nothing. At most
    one can be right. `FUN_801D5DE0` was simultaneously the world-map tile
    cursor SM, the shop stock list and the casino prize list.

`absent-citation`
    A `PORT:` line's own parenthetical cites a concrete retail address - an
    interior range, a `DAT_` - that appears **nowhere** in the routine's dump.
    Evidence the port offers for itself that the disassembly does not carry.

`doc-citation`
    The same test applied to `docs/reference/functions/` rows.

## What it deliberately does not do

There is no zero-false-positive oracle here and this does not try to be one.
Precision beats recall: a noisy gate gets ignored, which is worse than no gate.
Known-good shapes are excluded by construction rather than left to the reader:

- **One routine linked into two overlays** is not a defect. `FUN_801D14B0` and
  `FUN_801D6710` are the same 24 instructions at two VAs. Identical bodies at
  different VAs never produce a finding, because the signals are per-address.
- **Deliberate kernel reuse across subsystems** is not a defect either. The
  defect is the *claim*. A finding names both sides; a human decides which
  label is wrong, or waives it as reuse.
- **Sharing a subsystem's tables is the normal case.** A fifth signal that
  reported every pair of PORT-tagged routines sharing a rare table across
  unrelated module names was built, measured at 328 findings, and deleted. See
  `elsewhere_claims` for what survived of it and why the consensus variant did
  not rescue it either.
- `jal` targets decoded from the `overlay_0896` window **below** `0x801CE818`
  are untrustworthy (`docs/tooling/call-target-integrity.md`), so those dumps
  contribute no callee signal.

The report is a **worklist, not a gate**. It is warn-only and is not wired into
pre-commit; `--strict` exists for a reader who has waived what they have read.

## Usage

    python3 scripts/ci/check-port-provenance.py             # ranked report
    python3 scripts/ci/check-port-provenance.py --live-only  # live ports first
    python3 scripts/ci/check-port-provenance.py --addr 801d5de0
    python3 scripts/ci/check-port-provenance.py --strict     # exit 1 on unwaived
    python3 scripts/ci/check-port-provenance.py --emit-waivers

Reviewed false positives go in `scripts/ci/port-provenance-waivers.toml`, one
entry per finding key, each with a `reason`. `--strict` is the ratchet: it
fails only on findings that carry no waiver.

The two corpora are gitignored, so a git worktree has neither. Point the
checker at another checkout's copies rather than reading a pass that examined
nothing:

    python3 scripts/ci/check-port-provenance.py \
        --funcs /path/to/main/ghidra/scripts/funcs \
        --extracted /path/to/main/extracted

Without a funcs corpus the run exits 2 (`--allow-vacuous` downgrades that to
the old exit 0), because a corpus-blind run reports every tag as unimpeachable.
"""
