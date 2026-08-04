# Port provenance: does the address name the right routine?

[`port-catalog.py`](port-catalog.md) answers two questions about a
`// PORT: FUN_<addr>` tag. Does the tag **exist**, and is the Rust symbol
carrying it **reachable** from a host root. Neither question is *does that
address name the routine this Rust code implements*.

Nothing gated that. A live, tested, shipped port could wear a wrong address
with every gate green - `check-port-tags.py`, the live audit and the
[disc-coverage](disc-coverage.md) ratchet all pass, and the ratchet then counts
the wrong retail bytes as covered. The gate that asks the third question is
`scripts/ci/check-port-provenance.py`.

## The defect this exists for

`engine-core::shop::shop_stock_row_ink` carried `PORT: FUN_801D5DE0` while live
on both hosts. That routine is the **casino prize list's** row renderer: it
indexes the prize table `0x801E4518` at `base + block*0x60 + row*8`, takes the
block byte from the entry-context pointer `_DAT_8007B450[1]`, and gates
affordability on `_DAT_800845A4`, the coin bank. The party gold purse
`_DAT_8008459C` appears nowhere in its 151 instructions.

The mis-attribution came from the dump filename `overlay_shop_save_801d5de0.txt`
- a prefix that names the **image the routine was dumped from**, an overlay that
carries menu, save, shop and casino code alike.
[`dump-corpus-integrity.md`](dump-corpus-integrity.md) already states that law
for load bases and [`phantom-print-index.md`](phantom-print-index.md) applies it
address by address; this page is the same law applied to **function identity**.

How far one filename travelled: the routine was described as the shop stock
list in `docs/subsystems/shop.md`, in `docs/reference/functions/menus.md` and in
two crates' module docs, **and** as the world-map tile cursor state machine in
`docs/subsystems/world-map.md`. Three subsystems, one routine, no page aware of
the others.

### Why nothing noticed

Every text agreed with the disassembly about the *operands*. `world-map.md`
correctly listed `DAT_801EF0D0`, `_DAT_8007BB98`, `FUN_8002B994`,
`_DAT_8007B454 = 7` and `DAT_801E4518`; `menus.md` correctly listed the 8-byte
stride and the `0x60`-byte block. Only the **subsystem label** was invented, and
with it the reading of `DAT_801E4518` as per-cell art ids rather than the casino
prize table it is everywhere else. A checker that verifies cited addresses
against the disassembly passes all three texts. The contradiction is only
visible between them.

## The signals

All four are cheap enough to run on the whole corpus in seconds. For every
PORT-tagged address the checker recovers, from the **disassembly only**, the
data addresses the routine forms and the `jal` targets it calls.

| Signal | Question it asks |
|---|---|
| `module-orphan` | Does this routine corroborate anything else its module claims? |
| `dual-label` | Is this routine given a defining description on two unrelated docs pages? |
| `absent-citation` | Does a `PORT:` line cite evidence its routine's dump does not carry? |
| `doc-citation` | Same, for `docs/reference/functions/` rows. |

`module-orphan` is the one that reproduces the shop defect. It flags a routine
that shares no distinctive data address, no distinctive callee and no call edge
with any of its module's siblings while those siblings corroborate each other,
and it lifts the row when the same rare table turns up under a module with an
unrelated name. For `FUN_801D5DE0` in `shop.rs` that second half reads
`0x801E4518 is also formed by FUN_801dc1cc, PORT-tagged in prize_exchange.rs`.

### Distinctiveness is the whole trick

Without a corpus denominator every routine looks related to every other one.
The text writer `0x80036888` is called from 82 dumps and the string blitter
`0x8002b994` from 39, so "these two share a callee" is true of almost any pair.
The checker therefore counts, across every dump, how many addresses form each
data address and call each target, and only features below a cut carry
information. The cuts are constants at the top of the script, and they are
precision dials, not fitted numbers.

## What was tried and deleted

A fifth signal, `split-table`, reported every pair of PORT-tagged routines that
shared a rare table across unrelated module names. It produced **328** findings
and none of the top ones were defects. The premise was wrong: sharing a
subsystem's own tables is the normal case, and a host-dispatcher module such as
`world/frame_tick.rs` pairs with every minigame it dispatches.

Requiring a *consensus* to contradict - two or more mutually-related modules
claiming the table and exactly one that matches none of them - cut it to 133 and
did not rescue it. The largest groups are fifteen battle modules where "the odd
one out by name" is arbitrary. What survived is its use as **corroboration** on
a `module-orphan` row, where the conjunction is rare enough to act on.

This is recorded rather than quietly dropped because the premise is the
attractive one, and the next person to build this instrument will reach for it
first.

## Precision, measured

The first run raised 7 `absent-citation` findings. Triaging them by hand against
the disassembly found **six checker defects and one real finding** - a precision
of 1/7. Each defect was in the reader, not the corpus, and each is now fixed:

- **A tag line claiming several routines.** `PORT: FUN_801ed308 (cases 6/7),
  ..., FUN_801ee90c (the 0x801EEA50 block), ...` was checked by testing every
  parenthetical against every address on the line, so four routines were
  reported as missing a block only the fifth ever claimed. Citations now bind to
  the nearest preceding address.
- **A dump's header `size` is not its extent.** It is the instruction count
  times four, which differs when Ghidra skipped undefined bytes inside the body:
  `overlay_0977_slotA_801cf870` reports 1748 bytes and prints across 2088. Every
  citation landing in such a gap read as absent. The extent now comes from the
  printed addresses.
- **The indexed jump-table idiom.** `lui at,0x801f` / `addu at,at,index` /
  `lw v0,0x6aa8(at)` forms the table base `0x801F6AA8`, but the `addu` clobbers
  the register the `lui` half lives in, so a tracker that drops a register on
  any write loses every jump-table base in the corpus and reports the tags
  citing one as unsupported.

A fourth defect came out of `module-orphan` triage: **calling a sibling was not
counted as corroboration**. The feature-set intersection could never see it,
because the callee is the sibling's own address rather than a shared third
party, and `save_select.rs` read as having an orphan that calls four of its own
siblings. Fixing it took `module-orphan` from 110 findings to 67.

The lesson generalises past this script: when a new measurement's first output
is a pile of findings, the first hypothesis to test is that the measurement is
wrong, and hand-triage of the top rows is how you test it.

## Reading a report

    python3 scripts/ci/check-port-provenance.py              # ranked report
    python3 scripts/ci/check-port-provenance.py --live-only  # live ports only
    python3 scripts/ci/check-port-provenance.py --addr 801d5de0
    python3 scripts/ci/check-port-provenance.py --signal dual-label
    python3 scripts/ci/check-port-provenance.py --emit-waivers

`--live-only` needs `target/port-catalog/catalog.csv`, which
`port-catalog.py --live-only` writes. Prefer it: a wrong tag on an inert port is
a doc bug, while a wrong tag on a live one also mis-credits disc coverage.

This is a **worklist, not a gate**. It is warn-only and deliberately not wired
into pre-commit - the surviving signals rank suspicion, they do not prove
anything, and a blocking check that a reader learns to skip is worse than no
check. `--strict` fails on any finding without a waiver, for a reader who has
worked the list down.

Reviewed findings go in `scripts/ci/port-provenance-waivers.toml`, keyed by the
finding key, each with a `reason` that says what was read and what it showed.
"Probably fine" is not a reason - it would be equally true of a real defect, and
an unreviewed row belongs in the report where it can still be seen.

## Known-good shapes, excluded by construction

- **One routine linked into two overlays.** `FUN_801D14B0` and `FUN_801D6710`
  are the same 24 instructions at two VAs. The signals are per-address, so
  identical bodies at different VAs never produce a finding.
- **VA aliasing.** Overlays are linked over `0x801C0000+`, several at the same
  base, so a VA at or above it identifies an object only together with its
  image. Corroboration across the overlay band requires a shared image tag.
- **Untrusted call targets.** `jal` targets decoded from the `overlay_0896`
  window below `0x801CE818` are a property of a wrong load base
  ([`call-target-integrity.md`](call-target-integrity.md)); those dumps
  contribute no callee signal.
- **Kernel reuse across subsystems.** The port reuses `FUN_801D5DE0`'s row-ink
  kernel for shop stock rows because the *shape* matches. That is not the
  defect - the defect was the claim that the routine was the shop's. A finding
  names both sides and a human decides which it is.

## An empty corpus is not a clean run

`ghidra/scripts/funcs/` is gitignored, so a fresh clone or a git worktree has no
dumps and every port looks unimpeachable. The checker says so explicitly rather
than printing a clean report, because this repo has already shipped the other
kind: `check-port-tags.py` once printed "0 warnings across 0 file(s)", which
reads as a pass and meant it had scanned nothing.
