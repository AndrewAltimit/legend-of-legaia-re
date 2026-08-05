# Lane E - `module-orphan` precision, the decoder bug, and the uncomputed ratchet

Three deliverables, in the order the brief set them. Every number below is
measured, and the measurement harness and its inputs are described so the next
reader can redo it rather than trust it.

## How precision was measured at all

Wave 1's four real defects were **already fixed on this lane's base branch**, so
the current tree cannot show whether a change keeps them firing. The measurement
therefore runs against a **fixture**: a copy of `crates/` with the four wave-1
tags reverted to their pre-fix `PORT:` form, taken from `cae7b0e8` (the commit
before `6c892624` / `2f5b1e67`, the two lane fix commits).

| Key | File | Wave-1 verdict |
|---|---|---|
| `80036d80` | `engine-core/src/world/field_movement.rs` | MISPLACED (lane A) |
| `801d688c` | `engine-core/src/save_select.rs` | MISPLACED (lane A) |
| `801db380` | `engine-ui/src/ui_menu_window_painters_large.rs` | WRONG-CODE (lane B) |
| `801dd9d4` | `engine-vm/src/field.rs` | WRONG-CODE (lane B) |

Labelled set: 66 reviewed rows (lane A's 26 keys, lane B's 40), of which 4 real
and 62 reviewed-correct. The fixture reproduces wave 1 exactly - **67 findings,
all four defects among them**, the 67th being `world/frame_tick.rs:801ef2b0`,
which neither lane reviewed.

Dumps live only in the main checkout, so the harness loads the worktree's
checker as a module and repoints `FUNCS_DIR` at
`/home/mikunpc/.../ghidra/scripts/funcs`. **No dump was copied into the
worktree.**

## 1. Precision, before and after

| | before | after |
|---|---|---|
| findings (fixture) | 67 | **15** |
| known-real defects firing | 4/4 | **4/4** |
| reviewed-correct still firing | 62/62 | **9/62** (53 silenced) |
| unreviewed rows | 1 | 2 |
| precision | 4/67 = **6.0%** | 4/15 = **26.7%** |

On the real tree (defects already fixed): **63 -> 11** findings.

**The four known-real defects all still fire.** This was the binding constraint,
and it is the reason two changes that looked good were rejected or moved:

- A module-cohesion floor of 0.75 measured over all tags silences
  `801db380` - its module has four tags, two corroborating.
- A dump-coverage floor applied inside `features()` silences `801dd9d4` -
  its module's dispatcher sibling `FUN_801DE840` is dumped as a fragment, so
  removing fragments from corroboration made the module read differently. The
  floor moved onto the orphan *candidate* instead, where it belongs.

`ADAPTIVE_TARGET = 0.85` was chosen by sweeping it against the labelled set.
0.95 is what the sweep would pick on raw precision and it absorbs three of the
four defects, so the constraint - not the sweep - fixes the ceiling. The script
says so where the constant is defined.

### What each change contributed

Cumulative on the fixture, in the order applied:

| step | findings | defects |
|---|---|---|
| baseline | 67 | 4/4 |
| decoder fixes (load invalidation, entry filter, empty dumps, body consensus, wider DF denominator) | 61 | 4/4 |
| adaptive per-module cut + require own formed data | 34 | 4/4 |
| co-call corroboration | 31 | 4/4 |
| `ADAPTIVE_TARGET` 0.75 -> 0.85, `ADAPTIVE_MIN_TAGS` 8 -> 6 | 20 | 4/4 |
| majority-of-informative-tags floor | 16 | 4/4 |
| dump-coverage floor on the orphan candidate | **15** | **4/4** |

The decoder step is net -6 but not monotone: it removed 18 rows and *added* 12,
because the phantom addresses it deletes were acting as corroboration. Those 12
were absorbed by the later steps.

### The two structural causes, closed

- **Painter modules.** `ui_menu_window_painters.rs`: 14 -> **0**. Its members
  link at cut 96; at 8 only two of nineteen corroborate.
- **Actor-hub modules.** `baka_hub_actors.rs`: 11 -> **1**. Links at 48-64.
- **Host-dispatcher modules.** `world/frame_tick.rs`: 3 -> **0**. Links at 24.
- Also closed by the same rule: `save_subscreen.rs` 3 -> 0, `shop.rs` 3 -> 1,
  `screen_fx.rs` 3 -> 0.

A fourth cause is now named in the script and the doc: a **fragmentary dump**.
`FUN_801EA9B0` prints 25 instructions over a 250-instruction span - it is a
jump-table dispatcher dumped as its head plus its shared epilogue - so "shares
nothing with its siblings" was true of the tenth that was read.

A fifth is visible in what remains and has no structural detector: a module
named for a **shape** rather than a subsystem. `engine-core/src/scus_leaf_kernels.rs`
declares itself a bag of unrelated SCUS leaf kernels; "shares nothing with its
siblings" is that module's design. That is what a waiver is for.

## 2. The decoder bug

**Root cause: the `lui`-provenance tracker survived a load that invalidated it.**

`FUN_801DB8B4`, all sixteen instructions (`overlay_battle_action_801db8b4.txt`):

    801db8b8  lui   v0,0x801d
    801db8bc  addiu v0,v0,-0x6c90     ; v0 = 0x801C9370
    801db8c0  addiu a0,v0,0xc         ; a0 = 0x801C937C
    801db8c4  lw    v0,0x0(a0)        ; v0 RELOADED from the pointer table
    801db8cc  lhu   v0,0x14c(v0)      ; a field off a runtime pointer

`MEM_RE` did not capture the destination register, so `full[v0] = 0x801C9370`
survived the `lw` and the next displacement was read as forming
`0x801C9370 + 0x14C = 0x801C94BC` - an address that exists in no dump of the
routine. Verified gone: `features()` for `801db8b4` returns `(none)` where it
returned `0x801c94bc`.

Every `base + small offset` chain behind a pointer load has this shape, so this
was a class rather than a case. `mflo` / `mfhi` had the same hole from the other
direction - they carry no comma and so never matched the generic write rule.

**The aliasing half.** Lane A's reading was that VA aliasing supplied the wrong
body. What the corpus shows is narrower and worth recording: `801db8b4` has five
dumps, one from `overlay_0897` (14 instructions, the body the port implements)
and four battle-family dumps that agree at 16. The 0897 one **carries no
`[image]` tag**, and the old `HEADER_RE` required one - so it was not in the
corpus at all, and the only bodies the checker could see were from the wrong
image. The general fix is `consensus()`: group an address's dumps by body
fingerprint, read the plurality, and read nothing when they tie. Worked case is
`FUN_801F8E6C` - six dumps agree at 47 instructions with a prologue, one 48-
instruction `overlay_0897` window opens on a bare `jal` mid-routine, and the
union reported that window's callee `0x801d5718` and string reads
`0x80077024/3c/54/6c` as the routine's own. All four are gone.

**Non-entry windows** (lane A's third ask) are excluded: a dump whose header
`entry` differs from the address asked for is a window inside another routine.
195 files corpus-wide. This is what removes `overlay_0897_801dd9d4.txt`
(`entry=801dd8f0`) from `FUN_801DD9D4`'s evidence.

**Dumps that printed no instructions** are excluded too - the "0 instructions,
decompiled C only" shape `ghidra.md` catalogues. One `dual-label` finding rested
entirely on such a dump (below).

**The DF denominator was measuring half the corpus.** 1305 of 4090 dump files
carry no `[image]` tag and were invisible; 898 addresses had *only* such dumps.
How common an address is corpus-wide is a property of the corpus, not of whether
we can name the image, so the denominator now counts every dump that printed
instructions (2037 addresses, not 1139) while the **evidence** map stays
restricted to image-tagged dumps. That split is deliberate: it leaves
`by_addr`'s membership - which is what `dual-label` and `doc-citation` iterate -
unchanged.

## 3. The ratchet

**Measured:** `port-catalog.py --check` = 2.7s. `--check --live` = 19.7s.

**`--live` cannot go in pre-commit unconditionally, and CI cannot help.** The CI
step self-skips: the catalog's `dumped` column reads the gitignored Ghidra
corpus, so `Port-catalog ratchet` has always reported SKIPPED on the runner. The
hook is the only place any of these figures is ever compared.

**Decision.** The hook spends the pass when the figure can move. `disclosure_gap`
is a property of the Rust call graph and the `NOT WIRED:` tags, both under
`crates/`, so a commit staging `crates/` gets `--check --live` and a docs-only
commit gets `--check --allow-uncompared`. The CI step is switched to `--live`
without `--allow-uncompared` - inert today, correct the day a runner has the
corpus.

**The gate itself.** `--check` now **exits 1** when a baselined figure went
uncompared. Printing `NOT COMPARED THIS RUN` and exiting 0 is exactly what let
`disclosure_gap` drift off a baseline of 0. A caller that cannot afford the slow
pass must say `--allow-uncompared`, which puts the decision at the call site.

**The `--update-baseline` footgun.** A snapshot carries only what its run
computed, so writing it verbatim deleted the whole `live` block on any run
without `--live`. Uncomputed figures are now carried forward from the existing
baseline and named in the output. Verified: `--update-baseline` without `--live`
leaves `live/disclosure_gap` intact and prints
`carried forward (not computed this run): live/disclosure_gap=0`.

All four paths exercised against the real corpus:

| invocation | result |
|---|---|
| `--check` | exit 1, `NOT RATCHETED: 1 baselined figure(s) went uncompared` |
| `--check --allow-uncompared` | exit 0, `OK - 5/6 figure(s) compared` |
| `--check --live` | exit 0, `OK - 6/6 figure(s) compared` (17.9s) |
| `--update-baseline` (no `--live`) | exit 0, `live` block preserved |

`scripts/ci/port-catalog-baseline.json` is **unchanged** - no figure moved.

## For Lane F - waiver keys that move

Do not read this as a request to delete anything blind; the counts are here so
the churn is visible before the merge.

**55 waiver keys go dead** (the finding no longer fires): 54 `module-orphan`
and one `dual-label`.

- `dual-label:801f69d8` - correct removal, not a loss of coverage. Its only
  image-tagged dump (`overlay_muscle_dome_801f69d8.txt`) reports
  `size=1 bytes, 0 instructions` and has an empty disassembly; the other two
  files at that VA have headers the checker has never matched. The row was two
  doc pages disagreeing about an address with no readable body anywhere.
- The 54 `module-orphan` keys are the reviewed-correct rows the structural fixes
  silence. Full list: run
  `python3 scripts/ci/check-port-provenance.py --emit-waivers` and diff against
  the current TOML, or take it from the harness in this lane's scratch.

**2 `module-orphan` rows arrive unwaived**, both new and both in the shape the
signal cannot read structurally:

- `crates/engine-core/src/scus_leaf_kernels.rs:800265e8` - **reads as correct.**
  The module header declares a bag of unrelated SCUS leaf kernels. The
  disassembly matches the tag exactly: `FUN_800265E8` seeds the twelve words at
  `0x800917B0..` and sets three enable halfwords at `0x80070520 / 0x80070580 /
  0x800705B0` off `0x8007051C`, which is what the Rust doc claims.
- `crates/engine-core/src/world/battle/loop_driver.rs:801ec3e4` - **needs
  review.** Host-role module with three tags, each a different battle-loop
  concern. The tag's cited interior addresses (`0x801EEB88`, `0x801EEBD8`) do
  occur in the routine's dump, so it is self-consistent; I checked consistency,
  not identity.

**`doc-citation` moved too** (Lane F's class, none of these were waived): 63 ->
61 keys.

- GONE `doc-citation:docs/reference/functions/renderer.md:801f69d8:80043390+801f725c`
  and `...script-vms.md:801e30e4:801d0a6c` - both rested on dumps now excluded.
- RE-KEYED `docs/reference/functions/battle.md:801f0450` from `:801f02d0` to
  `:801f0000+801f02d0`. The row still fires; the second address is new because
  the phantom that "supported" `0x801F0000` is gone. **It is a false positive of
  the `doc-citation` kind**: the row says the routine sits "in `0898`'s render
  tail `0x801F0000..8000`", which is a range endpoint, not a claim the routine
  forms that address. Worth a `doc-citation` rule about range endpoints if you
  are already in there.

`absent-citation` is unchanged (1 finding).

## Is `module-orphan` gateable? No, and here is what remains

One row in four is real, up from one in seventeen. That is a worklist a reader
can finish in an afternoon; it is not a check that may fail a commit, because
three of every four failures would be correct code.

What can gate is the **delta**. With the reviewed rows waived, `--strict` fires
only on rows nobody has read, and this pass produced two of those from a corpus
of 1409 tags. That is the shape to wire up if the class is to block anything -
and it needs the waiver file to be current, which is why the churn above matters
more than the precision figure.

What is left in the 15, by cause:

- **9 reviewed-correct rows.** Six sit in modules of three or four tags where
  the corroborating core is exactly two - the smallest module the signal will
  speak about at all. Raising that floor is the obvious next move and it is
  blocked by `801db380`, whose module is that shape and whose tag was genuinely
  wrong. Any rule that clears those six clears the defect too, on this corpus.
- **2 unreviewed rows**, both above.
- The remaining ceiling is not a threshold problem. "Shares nothing with its
  siblings" has four innocent causes and one guilty one, and three of the four
  are now handled. The fourth - a module whose members are unrelated **by
  design** - is a fact about the Rust file, not about the disassembly, and no
  amount of reading the dumps will find it.

## Reproducing

The harness is in this session's scratch, not committed (it hardcodes an
absolute path to the main checkout's dumps). To rebuild it: load
`scripts/ci/check-port-provenance.py` with `importlib`, set `FUNCS_DIR` to the
main checkout's `ghidra/scripts/funcs`, set `REPO` / `CRATES_DIR` to a fixture
tree with the four tags reverted, and call `find_module_orphans` directly.
Corpus parse is ~3s; the full four-signal run is 3.5s.
