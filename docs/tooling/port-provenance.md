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

### `dual-label` compares defining pages, and a pointer is not a claim

Page relatedness starts from filename tokens, and that alone over-fires by a
wide margin. The function directory is named for coarse topics (`menus`,
`script-vms`, `battle`) and the write-ups for fine ones (`save-screen`,
`field-locomotion`, `battle-action`), so an index entry and the page it indexes
always read as "two unrelated names". Two exclusions cut that class:

- **A site that links to the counterpart page is a pointer.** A directory row
  whose own text sends the reader to the page it is said to contradict is
  filing that page's claim rather than competing with it. Scoped to the pair -
  a row pointing at `save-screen.md` is still an independent claim against
  `world-map.md` - and scoped to *every* site the page has for that address, so
  a page that defines a routine twice and links once still counts as a rival.
- **Only `docs/subsystems/`, `docs/formats/` and `docs/reference/functions/`
  define a routine.** A thread ledger records which readings are falsified and
  a `docs/tooling/` page describes an instrument; naming an address there is
  not a second label for it. Two of the rows this drops are titled "X is *not*
  Y".

Neither reaches `FUN_801D5DE0`, whose carriers cited nobody. Neither is a
substitute for reading the two texts, either: once a human reconciles a pair,
the corrected pages agree and the fine page usually gains a link *because* of
the correction, so the row goes quiet through the pointer rule rather than
through any test of whether it was ever a conflict. That is what a waiver
records, and it is why the pointer rule is validated against the doc state the
conflicts were found in rather than the state after they were fixed.

### Distinctiveness is the whole trick

Without a corpus denominator every routine looks related to every other one.
The text writer `0x80036888` is called from 82 dumps and the string blitter
`0x8002b994` from 39, so "these two share a callee" is true of almost any pair.
The checker therefore counts, across every dump, how many addresses form each
data address and call each target, and only features below a cut carry
information.

The denominator is taken over **every** dump that printed instructions, not
only the image-tagged ones. How many routines form an address is a property of
the corpus; a dump with no `[image]` header is a worse witness to *which* body
sits at a VA and an equally good one to how common that address is. A third of
the corpus carries no image tag, so restricting the count to the tagged part
measured half the corpus and called the result distinctive.

### The cut has to follow the module

One fixed cut cannot read every module, and the modules it fails on are a kind,
not a sample. A window-descriptor **painter** has exactly one datum of its own -
its private overlay string pool - and calls only the corpus-wide text writer,
number writer and marker blitter. Its siblings share the choice-state word and
the panel-install callee, both far above any fixed cut. So every painter in a
painter module shares nothing distinctive with every other painter, and the
module reports as many orphans as it has members: fourteen from one file,
eleven more from an actor-hub module with the same shape one level up, and a
host-dispatcher module reports the per-mode state machines it dispatches, each
in its own overlay with its own tables.

So the cut climbs a ladder per module until the module reads as cohesive, and
only a member still unlinked *there* is saying anything. A painter module links
at 96 and has no orphan left; `world/field_movement.rs` is cohesive at the base
cut and its one wrong tag stays an orphan at every cut on the ladder. A module
that never reads as cohesive is one the signal cannot read, and it produces
silence rather than one finding per member.

Two guards keep the ladder honest. A module with too few tags does not get a
per-module cut at all - the "cut it needs" would be one pair's accident - and
under both paths a *majority* of the module's evidence-bearing tags must
corroborate before a member of the minority can be called foreign. Tags whose
routine forms nothing distinctive are outside that denominator: they cannot
corroborate anything, so counting them against the module measures the corpus
rather than the module.

Corroboration and orphanhood need different evidence, and the asymmetry decides
where a filter belongs. "These two touch the same table" is an existence claim a
fragmentary dump can settle, so a fragmentary sibling still corroborates. "This
one touches nothing any of them touch" is a claim about a whole body, and a dump
that printed a tenth of it - a jump-table dispatcher dumped as its head plus its
shared epilogue - cannot make it. Applying the coverage floor to corroboration
instead silenced a real defect whose module's dispatcher sibling is such a dump.

The thresholds are precision dials. `ADAPTIVE_TARGET` in particular was chosen
by sweeping it against the hand-reviewed verdicts under the constraint that
every known-real defect keeps firing, and the script says so where it is
defined.

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
siblings. Fixing it retired a large fraction of the class in one change.

A fifth came out of `doc-citation` triage, and it is the same mistake in a new
place. Both citation signals exempt a cited **function entry**, because prose
about a routine names its callers, its siblings and the dispatcher that reaches
it, and none of those is in its own bytes. That exemption was keyed on whether
the cited address had a *dump*, which inverts it: a row naming a caller we had
read passed, and the same row naming a caller we had not read as unsupported
evidence. Roughly two thirds of the signal's output was that shape. The test is
now the citation's **written form** - `FUN_8003D53C` is a function reference
whether or not it is in the corpus.

What survived that fix split three ways. Six rows named a data global the body
does not form and a sibling page had right, all of them a hex digit out
(`0x801D1CBC` for the SEQ voice count `0x801CE344`, `0x801DADD8` for the libcd
FS table `0x801CADD8`, `0x801D04B8` for the format string `0x801CF4B8`). Three
described a routine no dump of that VA carries. The rest cite an address that
belongs to a *different* routine - a caller's `jal` site, a table its dispatcher
reads, a load base named as provenance - and no written form separates "the
table I read" from "the table that indexes me", so those are waived rather than
signalled.

The lesson generalises past this script: when a new measurement's first output
is a pile of findings, the first hypothesis to test is that the measurement is
wrong, and hand-triage of the top rows is how you test it.

### The tracker synthesised addresses that exist in no dump

`FUN_801DB8B4`'s reported distinctive data included `0x801C94BC`, which appears
in none of its dumps. Its sixteen instructions form `0x801C9370` with a
`lui`/`addiu` pair, reload the register from a pointer table with `lw v0,0x0(a0)`
and then read `lhu v0,0x14c(v0)` - a field off a runtime pointer. The tracker
carried the `lui` provenance **through the load** and reported the sum as a
global the routine owns. Every `base + small offset` chain behind a pointer load
has that shape, so this was a class rather than a case: a load now ends its
destination register's provenance, as do the one-operand writers (`mflo`,
`mfhi`) that carry no comma and so slipped past the generic write rule.

Three corpus-shape filters landed with it, each a case the checker had been
reading as evidence:

- **A dump whose header `entry` is not the address asked for** is a window
  opening inside a *different* routine, and one such file was the sole
  disagreeing body for an address whose other seven dumps agree.
- **A dump that printed no instructions at all** - the "0 instructions,
  decompiled C only" shape [`ghidra.md`](ghidra.md) catalogues - was still
  counted as a body. One `dual-label` finding rested entirely on an address
  whose only tagged dump was of that kind.
- **Bodies that disagree at one VA.** Overlays link different code over one
  overlay address, so an address's dumps are not all the same routine and
  unioning their formed addresses attributes one routine's tables to another.
  Dumps are grouped by body fingerprint and only the plurality is read; a tie
  means the corpus cannot say which body the port implements, and the address
  contributes nothing rather than the wrong thing. `0x801F8E6C` is the worked
  case - six dumps agree at 47 instructions with a real prologue, and one
  48-instruction window opens on a bare `jal` mid-routine.

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

`module-orphan` is the signal that has been pushed hardest on precision, and it
is still not a gate on its own numbers. Against a hand-reviewed control set the
structural fixes above cut most of the class away while every known-real defect
in it keeps firing, which raises the share of rows worth acting on by roughly a
factor of four. What survives is a worklist a reader can finish; it is not a
check that may fail a commit. What can gate is the
*delta*: with the reviewed rows waived, `--strict` fires only on rows nobody has
read, and those arrive a handful at a time.

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
  image. Corroboration across the overlay band requires a shared image tag, and
  an address whose dumps disagree on the body is read from the plurality body
  only - or from none, when they tie.
- **Fragmentary dumps.** A jump-table dispatcher is routinely dumped as its
  head plus its shared epilogue, printing a tenth of its own address span.
  Such a dump can still corroborate a sibling, and can never make a routine an
  orphan.
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
