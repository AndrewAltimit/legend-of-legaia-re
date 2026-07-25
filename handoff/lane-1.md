# Lane 1 handoff

Two tasks: the disc-denominated SCUS dump worklist, then a corpus-integrity
sweep over the dumps. The sweep section is first because its findings change how
much of the rest of this repo's evidence base can be trusted.

---

# Part A - corpus-integrity sweep

`scripts/ghidra-analysis/check-dump-base-integrity.py` gained a second axis,
`--shape`, alongside its existing base check. The base axis asks *are these
addresses real*; the shape axis asks *is this file the whole routine*. Shape
reads only dump text, so it needs no `extracted/` tree and runs anywhere.

## A1. Six defect classes, not three

The brief named three. The sweep found three more, and two of them are worse
than anything on the original list because they are **internally consistent** -
the file's header agrees with its own stream, every printed address is right,
every instruction is real, and it is still wrong.

| Class | What it is | Visible to |
|---|---|---|
| `HEADERLESS_C_ONLY` | No header, no disassembly - only a C rendering, which this repo's own rules say is not evidence. | disc-coverage (excludes it) |
| `HEADERLESS_WITH_DISASM` | Complete disassembly, header absent. **Sound evidence the counters discard.** | nothing |
| `SHIFTED` (base axis) | Right bytes, wrong load base. | the base check |
| `NO_RETURN` | Header agrees with the stream, but the stream stops mid-body: Ghidra computed a short function body. | **nothing** |
| `INTERIOR_SLICE` | The dump's entry lies inside another dump's body - a slice of a larger function, not an entry point. | **nothing** |
| name mismatch | The filename's address is not where the content starts. | **nothing** |

The last three are new. `NO_RETURN` is what blocked Lane 2 on `801d56e4`.

## A2. The name-mismatch class is the big one - 142 cited dumps

**Every dumper in `ghidra/scripts/` resolves a target with
`getFunctionContaining(addr)` but names the output file after the *requested*
address.** Ask for an interior address and you get a file named
`<interior>.txt` holding the *enclosing* function. Every citation of that
filename then asserts a function entry that does not exist.

142 dumps in the cited set are name-mismatched, and **every single delta is
negative** - content always starts at or before the filename address, which is
exactly the signature of that resolution rule. Examples:

```
8001b47c.txt   named 8001b47c   starts 8001ada4   (-1752)
8002cdd0.txt   named 8002cdd0   starts 8002c69c   (-1844)
8004f0e8.txt   named 8004f0e8   starts 8004e568   (-2944)
80056b18.txt   named 80056b18   starts 800567b8   (-864)
```

The docs already caught a handful of these by hand - `functions/runtime-libs.md`
carries rows saying `0x8003F000` and `0x8003F0F4` are interiors and "the bare
address is a citation stub, not a second function". That was the right call, made
one address at a time. There are 142.

**This is not a re-dump job.** The bytes are fine. What is wrong is the filename
and any citation that treats it as an entry point. Two fixes worth considering,
both outside Lane 1's scope:

1. Change the dumpers to name output after `func.getEntryPoint()`, not the
   requested address, and emit a `requested=` line when they differ. One-line
   change in each dumper; `dump_scus_gaps.py` and `repair_truncated_dumps.py`
   already name by entry point.
2. Run the sweep's mismatch list against the citation graph and re-point each
   citation at the real entry.

## A3. What was repaired

**`801d56e4` (fishing, PROT 972) - Lane 2's blocker, fixed.** Ghidra's body was
524 B / 131 instructions and stopped mid-load at `0x801D58EC`. Two interior
`FUN_` entries - `801d58f0` and `801d5a24` - were cutting it. Dropping both and
rebuilding over the `jr ra` walk gives **1352 B / 338 instructions**, ending
properly at `801d5c24 jr ra`. Lane 2 is unblocked.

Two consequences for Lane 2:

- `overlay_fishing_801d58f0.txt` and `overlay_fishing_801d5a24.txt` are now
  provably **interior slices** of `801d56e4`. Retire both dumps and any citation
  of them; do **not** re-dump those addresses.
- `overlay_debug_menu_801d56e4.txt` holds the same 524 B. Per
  `static-overlays.toml`, PROT 0971's own content is only `0x1800` bytes and
  everything past that is over-read of 0972 - and `0x801D56E4` is `0x6ECC` into
  the overlay. So that file is fishing-overlay bytes wearing a debug-menu label.

**Bulk SCUS pass - 58 targets.** Every defective dump in the cited set with an
unambiguous SCUS base was re-dumped worst-first: 38 `HEADERLESS_C_ONLY`, 8
`HEADERLESS_WITH_DISASM`, 8 `TAIL_J`, 4 `NO_RETURN` (forced rebuild). Three were
structural repairs, not just re-dumps:

- `8003d388` - body was **4 bytes**; rebuilt to 60. `functions/runtime-libs.md`
  already said "Ghidra's auto-analysis splits this entry as `8003d388`/`8003d38c`;
  the body is one function at `0x8003D388`". That is now true in the project DB.
- `8003d178` - interior entry `8003d190` dropped, rebuilt to 44 B.
- `80021940` - body was **0 bytes**; rebuilt to 452 B.

`800480d8` (Lane 6's request) re-dumped: 568 B / 142 instructions, complete.
Lane 6's calibration point holds - it was `HEADERLESS_WITH_DISASM`, sound
evidence the header-driven counter could not see, not a thin dump.

## A4. The headerless number the brief asked for

**46 headerless files converted into real disassembly** in this pass (38
C-only + 8 with-disasm), all SCUS.

The more useful number is what the class is made of. Corpus-wide the sweep sees
**324 headerless dumps**, and only **32 of them carried a complete disassembly**
- roughly 19,900 instructions (~78 KB) of perfectly good evidence that every
header-driven counter was silently discarding. So the coordinator's hypothesis is
**half right**: the headerless population is mostly genuinely empty (291 C-only),
but the ~10% that is complete does mean the SCUS figure was understated. That is
part of why re-dumping moved it from 94.4% to 95.4% - some of what looked like
un-dumped gap was dumped all along.

## A5. Cited-set scoreboard

Restricted to dumps the committed docs cite as evidence (3363 files):

| Class | Before | After |
|---|---:|---:|
| `SOUND` | 2868 | 2911 |
| `HEADERLESS_C_ONLY` | 232 | 194 |
| `NO_RETURN` | 141 | 135 |
| `TAIL_J` | 92 | 86 |
| `HEADERLESS_WITH_DISASM` | 29 | 21 |
| `INTERIOR_SLICE` | (undetected) | 15 |
| name-mismatched (orthogonal) | (undetected) | 142 |

**Defect rate in the cited set: 14.7% -> 13.4%.** That is the honest measure of
how much of this repo's evidence base is not checkable as it stands.

## A6. Residue - what Lane 1 did NOT reach

**452 defective cited dumps remain, and all but 14 are overlay dumps.** They were
left deliberately, not for lack of time: an overlay re-dump has to be run against
the *right* program, and the sweep does not yet establish which program each
overlay dump belongs to. Re-dumping them against a guessed program is precisely
the failure `dump-corpus-integrity.md` exists to warn about, so doing it blind
would manufacture the defect it is meant to remove.

```
overlay_0897            161      overlay_muscle_dome      15
overlay_magic_capture    77      SCUS-bare                14
overlay_battle_action    64      overlay_baka_fighter      9
overlay_0897_xxx_dat     20      overlay_debug_menu        9
overlay_0896             16      overlay_dance             7
```

`overlay_magic_capture` (77) is RAM-capture-derived and has no static image to
re-dump from at all. The prerequisite for the rest is byte-level program
attribution - the same thing the base axis's `NOT_FOUND` class needs.

The 14 remaining SCUS-bare rows are the ones the bulk pass could not resolve; the
sweep names them with `--shape --cited-only --list-defects`.

## A7. For Lane 2 - `docs/tooling/dump-corpus-integrity.md`

Lane 2 owns that page. It currently documents the base axis only. The shape axis
belongs on it: the six-class table in A1, the `NO_RETURN` mechanism (interior
`FUN_` entries cutting a body), the `INTERIOR_SLICE` and name-mismatch classes,
and the repair tool. Suggested framing for the page's thesis, which currently
reads "a dump's printed addresses are a property of its load base":

> A dump has two independent ways to lie. Its **addresses** are a property of
> the load base it was imported at. Its **extent** is a property of the function
> body Ghidra computed - and a body that stops short produces a dump that is
> internally consistent, correctly addressed, and still not the routine. The
> first is caught by resolving bytes to an image; the second only by asking
> whether the stream reaches a return.

Commands:

```
scripts/ghidra-analysis/check-dump-base-integrity.py --shape
scripts/ghidra-analysis/check-dump-base-integrity.py --shape --cited-only --list-defects
scripts/ghidra-analysis/check-dump-base-integrity.py --shape --emit-csv out.csv
```

## A8. Scope note

The brief scoped Lane 1 to `ghidra/scripts/**`, the five `functions/*.md` pages,
`handoff/lane-1.md` and `docs/tooling/ghidra.md`. The sweep itself lives in
`scripts/ghidra-analysis/check-dump-base-integrity.py`, which is outside that
list - taken on the coordinator's explicit instruction to *extend the existing
tool rather than write a second one*. Flagging it in case a sibling also holds
that file.

---

# Part B - disc-denominated SCUS dump worklist

Lane 1 closed the `SCUS_942.54` code-gap worklist that
`scripts/ci/disc-coverage.py` emits. Coverage moved **84.0% → 94.4%**
(303518 → 341010 of 361126 code bytes), and the Part A repairs took it to
**95.4%** (346708/363608 - the denominator moves too, because newly-dumped
regions reclassify neighbouring gaps from data to code). All eight runs the
report opened with are closed, plus the eight it promoted afterwards.

Everything below is a finding that belongs in a file Lane 1 does not own.

## 1. `docs/subsystems/audio.md` line 204 - a false claim, correct conclusion

The "Correction (label != role)" paragraph reads:

> `FUN_8006352C` / `FUN_8006320C` were tagged elsewhere as "fixed-point div"
> pitch kernels - they carry **no division** and are per-channel note/expression
> handlers.

**"they carry no division" is wrong.** `FUN_8006320C` carries a `div` / `mfhi`
pair at `0x8006329C..0x800632C4`, and `FUN_8006352C` carries the matching one at
`0x800635BC..0x800635E4`. Both are a **modulo** of the slide's remaining-tick
counter (`+0xA0`) by its signed per-tick step (`+0x4C`) - the sub-tick divider
that lets a slide move one volume unit every `N` ticks instead of `N` units every
tick.

The paragraph's *conclusion* survives: neither is a pitch kernel, and the
note→pitch math really is confined to `FUN_80066E50` / `FUN_8006C6E4`. Only the
reason needs replacing. Suggested wording: *"neither is a pitch kernel - the one
`div` each carries is a modulo of the slide tick counter, not a fixed-point
pitch divide"*.

While there, the same page's per-frame call graph (line 202) can be sharpened.
It currently says `FUN_80062F98` (per-slot fan-out) → `FUN_8006320C` /
`FUN_8006352C` → … with "track-end / vab-release in `FUN_80063AA8`". The
dispatch is by flag bit, and the full map is now in
`docs/reference/functions/audio.md` § "SsAPI per-frame calc tier". Two
corrections worth folding back:

- `FUN_8006320C` / `FUN_8006352C` are the **volume-slide ticks** (ascending /
  descending), not note/expression handlers. `FUN_8006206C`
  (`_SsSetSlideVolume`) is their installer and writes exactly their field set
  (`+0x48` / `+0x4A` / `+0x4C` / `+0x9C` / `+0xA0`) - that pairing is what
  identifies them.
- `FUN_80063AA8` is the **track-end / loop-repeat** handler, and the "vab
  release" reading is not what the disassembly does: on the last repeat it
  chains to another `(slot, channel)` named by `+0x22` / `+0x23` through
  `FUN_80064090` (unless `+0x22 == 0xFF`), and kills notes via `FUN_800684CC`.

## 2. `docs/reference/re-settled-threads.md` - two rows can be re-graded

Line 1598 lists `0x8002149C` and `0x80059E10` among addresses whose evidence
grade rests on something weaker than disassembly. Both now have dumps carrying a
full disassembly section:

- `0x8002149C` - 688 bytes / 172 instructions, force-disassembled (Ghidra had
  never analysed it). It is a **leaf** (no `addiu sp,sp,-N`), sits between
  `FUN_80021248` and `FUN_8002174C`, and reads the frame-delta scratch pair
  `0x1F800393` / `0x1F80037D` plus the camera-angle triple `DAT_8007B790` - the
  same inputs as `FUN_80021248`'s camera normalisation. Role not pinned; the
  bytes are now readable. Row added to `functions/game-modes.md`.
- `0x80059E10` - 644 bytes / 161 instructions. A libgpu **VRAM-transfer entry**:
  it clamps the caller's `RECT` extents at `+0x4` / `+0x6` against the
  framebuffer limits `0x80078D58` / `0x80078D5A` (the same globals the rectangle
  validator `FUN_80058170` bounds-checks) and calls `FUN_8005AA30` first. Row
  added to `functions/runtime-libs.md`.

Same line's `0x8004DC68` is **not** covered by this lane - it is still an
un-dumped gap edge.

## 3. `docs/reference/functions/battle.md` (sibling-owned) - one row to add

`FUN_800508DC` (732 bytes / 183 instructions) was in the worklist and is now
dumped with full disassembly. It is cited in
`docs/tooling/playthrough-coverage.md:695` as *"voice/anim-cue select keyed to
the 0x414"* but has no row in `functions/battle.md`. Lane 1 did not read it -
flagging only that the dump now exists at `ghidra/scripts/funcs/800508dc.txt`
where it previously carried no `size=` header.

The other battle-band addresses in the worklist (`8004AD80`, `8004C140`,
`8004C650`, `8004C7B4`) were already cited in `subsystems/battle-action.md` /
`subsystems/battle.md` / `formats/battle-data-pack.md`, so they were already in
the tracked set; only their **dumps** were missing. No new rows needed.

`8003D764` (already in `functions/script-vms.md`) and `8001B964` / `8001BE80`
(already in `functions/renderer.md`) are the same case.

## 4. `docs/subsystems/save-screen.md` - the missing entry path

That page documents the save-slot select + write flow `FUN_801DC6B4` but not how
a field session reaches it. It is `FUN_80024190` (new row + detail section in
`functions/game-modes.md`): an 11-state actor SM, spawn descriptor `0x800706BC`,
spawned by the field/world fade SM `FUN_801EE5D4`. It pages the menu overlay in
(`FUN_8003EBE4(4, 0)`), runs **either** `FUN_801DC6B4` (save) or `FUN_801DD35C`
(load) selected by `actor[+0x5C]`, then pages the field overlay (`slot 2`) and
the slot-B pair (`FUN_80025BA0`) back and sets mode 3.

Note the near-miss this corrects: `functions/game-modes.md` already says the
mode-23 CARD actor is `_DAT_8007B8E0`, spawned from descriptor `0x800706D4`.
`FUN_80024190` also writes `_DAT_8007B8E0`, which makes it look like that
actor's `+0x0C` handler. It is not - descriptor `0x800706D4 + 8` holds
`0x801E36A0` (read from `extracted/SCUS_942.54` at file offset `0x60EDC`), and
`0x80024190` appears exactly once in the image's data, at `0x800706C4`, i.e.
descriptor `0x800706BC`.

## 5. Remaining worklist (for the next lane)

After this lane the largest SCUS code gaps are all under ~630 bytes and sit
almost entirely in the statically-linked PsyQ band:

```
0x8006DB54..0x8006DDC8   628 B    0x8004DA00..0x8004DC68   616 B
0x800198E0..0x80019B28   584 B    0x8006DE30..0x8006E06C   572 B
0x800480D8..0x80048310   568 B    0x80059068..0x80059280   536 B
0x80020C14..0x80020DE0   460 B    0x800697E0..0x800699AC   460 B
```

`ghidra/scripts/dump_scus_gaps.py` takes them directly: append to `RANGES` and
run it against `SCUS_942.54`. Un-analysed sub-runs inside a range are reported
as "un-attributed" and go in `FORCE_RANGES`, which disassembles them and creates
one function per `jr ra` + delay-slot unit. **Check the bytes are MIPS first** -
forcing data produces convincing garbage.

## 6. `docs/tooling/ghidra.md` - script-catalogue row to add

`ghidra/scripts/dump_scus_gaps.py` is new and outside Lane 1's doc scope, so it
is not in the catalogue yet. Suggested row for the **Per-function dumps** table:

> | `dump_scus_gaps.py` | Dumper for the disc-denominated **code gap** worklist
> `scripts/ci/disc-coverage.py` emits. Takes address `RANGES` rather than entry
> points: walks the listing per range, dumps every function entry inside, and
> reports the bytes no function covers. Those go in `FORCE_RANGES`, which
> force-disassembles the run and creates one function per `jr ra` + delay-slot
> unit - the shape a run of separately-emitted library leaves has, where
> `force_disasm_dump.py`'s one-entry-per-address model needs the entries known in
> advance. `in_program()`-guarded, so it is safe to run against any program. |

## 7. Baseline

`scripts/ci/disc-coverage-baseline.json` is **not** touched by this lane, per
the wave brief. It needs re-ratcheting at integration; the current measured
value is `SCUS_942.54` code **95.4%** (346708/363608), after the Part A
corpus repairs.

## 8. Lane 6's `audio.md` correction - checked, no change needed

Lane 6 reports that `FUN_8006320C` does **not** call `FUN_80066308`, and asked
whether `docs/reference/functions/audio.md` restates that edge. It does not -
the string `80066308` does not appear anywhere on that page. The bad edge is in
`docs/subsystems/audio.md`'s per-frame call graph, which Lane 6 owns and has
already re-routed. Lane 1's section states only the edges it read: `FUN_80062F98`
calls `FUN_80065BAC` and the flag-bit handlers, and `8006320C` / `8006352C`
call `FUN_80067E9C` and `FUN_800683D8`. That agrees with Lane 6.
