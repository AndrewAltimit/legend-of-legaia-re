# Dump-corpus integrity

**An `overlay_0897_` filename prefix is not evidence of base correctness. Only
the `[overlay_0897 base=0x801CE818]` header tag is - and even a tagged dump may
have gaps.**

That sentence is the whole page. Everything below bounds the damage and shows
how to re-measure it.

A Ghidra dump in `ghidra/scripts/funcs/` prints instruction addresses derived
from the load base Ghidra was given when the program was imported. Get that
base wrong and every address in the dump is wrong by a constant, while the
instruction text stays perfectly plausible. Nothing in the dump looks broken.
It cites a function at a VA where that function does not exist, and it reads as
authoritative while doing so.

This is the dump-level sibling of
[call-target integrity](call-target-integrity.md). That page's subject is a
decoded `jal` target, which is a property of the bytes and survives a wrong
base. This page's subject is the *printed address*, which is a property of the
base and does not survive it at all.

## Why the filename cannot be trusted

Dump filenames are assigned by the operator running the dump script, from the
program they believe they imported. The `[label base=0xVA]` header tag is
emitted by the script from the program's actual load base. When the two
disagree, the header is right.

Three dumps in the corpus carry an `overlay_0897_` prefix and disassemble to
bytes that are not at that VA in the field overlay. All three lack the header
tag. Their prefix records an intention; the tag records a fact.

## Classification

Measured by [`check-dump-base-integrity.py`](#re-running-the-sweep), which
ignores printed addresses entirely and asks the bytes where they live: it
canonicalises each dump's opening instructions into a base-independent token
sequence and looks that up in an index built the same way over every extracted
image.

Default pass, 10-instruction signature, 3624 dumps:

| Class | Count | What it means | Usable for |
|---|---|---|---|
| `MATCH` | 2606 | Printed VA equals the VA the bytes resolve to. | Everything. Addresses, provenance citations, port tags. |
| `SHIFTED` | 292 | Bytes resolve at a constant non-zero delta. The dump was produced at the wrong load base. | Instruction *text* and decoded `jal` targets only. Never its addresses, and never as provenance for a function identity. |
| `NOT_FOUND` | 107 | Bytes are in no extracted image. | Unresolved - see below. Not known-bad. |
| `SHORT` | 619 | Fewer than 10 instructions; too short to sign. | No verdict either way. |

Those four numbers are what the committed script prints at its default
threshold, and they are the ones to quote.

### `canon()` must fold register spellings, not just mnemonics

The sweep compares Ghidra's rendering against capstone's, so every spelling the
two disassemblers disagree on has to be folded or the comparison fails on
identical machine code. Mnemonics are the obvious case. **Registers are the one
that bites**: the two name r30 differently - Ghidra `s8`, capstone `fp` - and
every function that saves a frame pointer touches r30.

Left unfolded, such a dump can never match any image, and it lands in
`NOT_FOUND`. That is the dangerous direction, because `NOT_FOUND` reads as
"this dump is of an overlay we never extracted" - a fact about the game -
when it is really a fact about the comparison. A quieter sibling rides along:
register names carry digits (`s7`, `a1`), so an immediate extractor run over
the raw operand string picks those digits up as operand values, and a register
spelled two ways then perturbs the immediate list as well as the register list.
Both are handled - `s8`/`s9`/`r30` fold to one name, and register tokens are
stripped before immediates are read - and together they account for roughly a
quarter of the corpus.

A third folding gap survives in the *positive* direction and is worth naming
because it produces a near-miss rather than a miss: the two disassemblers render
`break`'s code field differently (Ghidra prints the 10-bit code, capstone the
full 20-bit immediate, e.g. `break 6` against `break 0x1800`). A window whose
only disagreement is a `break` operand is a match; anything that compares
canonicalised tokens should treat a lone `break`-immediate mismatch as noise
rather than evidence of different code.

The generalisable point: **a resolver's negative class is where its own bugs
accumulate**, because a false negative there looks like missing data rather
than a broken comparison. Validate any change to `canon()` against a dump known
to be correctly based - a tagged one whose bytes you can confirm by hand -
before trusting the counts. A sweep that cannot resolve a dump it should is
indistinguishable, from the outside, from a corpus that genuinely lacks the
image.

Lowering the threshold trades coverage for certainty. At `--min-insns 4` the
`SHORT` class shrinks to 468 and the sweep returns 2698 `MATCH` / 372
`SHIFTED` / 86 `NOT_FOUND` - but a 4-instruction signature also matches
*ambiguously*, so part of that growth is the method resolving dumps it should
have declined. Treat a multi-hit resolution as weaker than a single-hit one.
The clusters below are quoted at both thresholds for exactly this reason: the
counts move, the conclusion does not.

### A capture with no static image is not automatically unattributable

`overlay_magic_capture.bin` is a save-state RAM slice, so nothing on the disc
reproduces it and the obvious reading is that its dumps can never be attributed
by bytes. That reading is wrong, and the way it is wrong generalises.

Its dumps are `ADDRLESS_DISASM`, so the attribution sweep - which reads printed
addresses - resolves none of them. But every one of them shares its
`(entry, size)` extent with an `overlay_magic_level_up_*` dump taken from the
sibling capture, and those *are* addressed and *do* resolve, single-hit, to
`battle_action(898)`. Comparing the two streams instruction by instruction, all
of the pairs carry identical code, and in all but one the addressed sibling's
printed addresses run `entry + 4*i` with no gap.

So the capture's slot A is PROT 0898 - established by extent identity across
the whole set, not inferred from the sibling's label. **A dump that cannot be
resolved directly can still be resolved through a dump that can**, whenever the
two share an extent. The extent is the join key the attribution artifact is
already keyed on, which is what makes the cross-check cheap.

The residual caution is the one worth keeping: a capture's slot B is whatever
the emulator held at that instant, so this settles slot A and says nothing
about the rest of the image.

### `NOT_FOUND` is unverifiable, not wrong

This is the class most likely to be over-discarded, so state it plainly: **a
`NOT_FOUND` dump is not a bad dump.** The sweep can only resolve bytes against
images that were extracted statically from `PROT.DAT`. Much of the corpus was
dumped from *live RAM captures* - mednafen and PCSX-Redux save states - of
overlays that have never been statically extracted, or of runtime-mutated
memory that no longer matches its on-disc form. Those dumps have no source
image to resolve against and land here by construction.

Some of them carry `base=0x801C0000` in their own header tag, which is the same
suspect base as the `+0xE818` cluster below - so a fraction of `NOT_FOUND` is
probably mis-based too. It cannot be shown statically either way. Treat
`NOT_FOUND` as "unproven", verify against a capture before relying on its
addresses, and do not delete it.

The class is now small enough to enumerate, which is itself the useful check:
when it was large, it was hiding a resolver bug rather than describing the
corpus.

## The shift clusters

The `SHIFTED` dumps are not scattered one-offs. Two clusters account for the
overwhelming majority, and both point at one mistake. Counts are given as
`default / --min-insns 4`.

| Delta | Count | Program | Reading |
|---|---|---|---|
| `+0xE818` | 208 / 221 | field overlay (PROT 0897) | Imported at base `0x801C0000` instead of `0x801CE818`. `0x801CE818 - 0x801C0000 = 0xE818`. |
| `+0x5818` | 50 / 55 | `overlay_0896_*` | Same field-overlay bytes, reached at PROT 0896's over-read base. |
| `+0xD018` | 8 / 8 | `overlay_0971` | The same mistake again, read through an over-read tail - see below. |
| `+0x9818` / `+0xD818` | small | `overlay_0978_*` | One `0x801C0000` import of 0978's over-read footprint; the delta names the stratum - `+0xD818` = field_battle_intro (0979) bytes, `+0x9818` = **dance** (0980) bytes. |
| `+0xE818` / `+0xA018` | small | `overlay_0977_*` | Same shape for 0977's footprint: `+0xE818` = 0977's own code, `+0xA018` = the 0979 stratum. Per-dump resolution: [re-settled-threads.md](../reference/re-settled-threads.md#prot-0977--0978-extraction--the-dump-re-key). |

The `+0xE818` mistake is not confined to the field overlay. `overlay_0899_xxx_dat_*`
dumps take the same delta into the *menu* overlay, so the base error travels with
the operator rather than with the program. The per-program deltas measured across
the whole `0x801C…` / `0x801D…` printed band, and every affected address, are
tabulated in [phantom-print-index.md](phantom-print-index.md).

**`+0xE818` is a single mis-based batch run.** Every member resolves
single-hit into `overlay_field_0897.bin`, with a median of 35 consecutive
exactly-matching instructions. A constant delta shared by well over a hundred
dumps is not coincidence; it is one import performed at the wrong base, and
every dump taken from that program inherited it. Most members are untagged -
the untagged class is where this concentrates.

**`+0x5818` corroborates the PROT 0896 over-read.** These dumps are labelled
`overlay_0896_*` yet their bytes resolve into the *field* overlay. That is
independent confirmation of what
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) already
argues on other grounds and what
[call-target integrity](call-target-integrity.md) found from the resolve-rate
seam: PROT 0896's footprint runs into its neighbour, so dumps taken at its
widely-cited base are reading field-overlay code. `0x801CE818 - 0x5818 =
0x801C9000`, the over-read base. PROT 0896's own link base remains unrecovered.

The seam is measurable rather than inferred, and it is two hops deep. Against
the extracted images, `0896_bat_back_dat.BIN[0x9000:]` equals
`0897_xxx_dat.BIN[0:]` byte for byte over its whole `0x46800`-byte remainder,
and `0897_xxx_dat.BIN[0x25000:]` equals `0898_xxx_dat.BIN[0:]` over its whole
`0x29800` bytes. So PROT 0896's own content is exactly its first `0x9000`
bytes, and re-keying an `overlay_0896_*` printed VA runs:

| `printed - 0x801C0000` | Owner | True VA |
|---|---|---|
| `< 0x9000` | PROT 0896 itself | unrecoverable - 0896's link base is still unknown |
| `0x9000 ..< 0x2E000` | field (PROT 0897) | `printed + 0x5818` |
| `>= 0x2E000` | battle_action (PROT 0898) | `printed - 0x1F7E8` |

Read against the `+0xE818` row above, that is the trap worth naming: the two
mis-based batches take **different** deltas. An `overlay_0896_*` VA re-keyed
with the 0897 batch's `+0xE818`, or with the `0x167E8` the 0897-into-0898
over-read uses, lands `0x9000` off - close enough to disassemble into plausible
code, which is exactly how a wrong re-key survives review.

The table is a **per-program** law, not a per-prefix one. The byte-level sweep
([`overlay-va-aliases.md`](../reference/overlay-va-aliases.md#prot-0896-two-programs-one-law-each))
splits the `overlay_0896_*` family into two imports: the untagged batch above
at `0x801C0000`, and a batch tagged `base=0x801C5818` (the phantom
jal-recovered base) whose prints are 0896's **own** bytes at
`printed - 0x801C5818` - including prints above `0x801C9000`, exactly where
the untagged law would mis-read them as `+0x5818` field code. Pick the program
from the header tag or the bytes before applying any row.

**`+0xD018` is a third mis-based batch, seen through an over-read tail.** It was
settled the way this page proposed: extract PROT 0971 (now mapped as
`debug_menu` at `0x801CE818`, see
[static-overlay-pipeline.md](static-overlay-pipeline.md)) and re-run the sweep.

The whole `overlay_0971` program was imported at `0x801C0000`, so its true delta
is the same `+0xE818` as the field batch. Only two of its dumps report that,
because PROT 0971's own content is `0x1800` bytes and the rest of the entry's
footprint is PROT **0972** (fishing). Dumps landing in that tail resolve into
`overlay_fishing_0972.bin`, whose base is `0x1800` lower, so the reported delta
comes out `0xE818 - 0x1800 = 0xD018`. One import error, two deltas, because two
images legitimately hold the bytes at bases that differ by the over-read offset.

The generalisable form: **a reported delta is relative to whichever image the
resolver matched.** Where entry footprints overlap, the same mis-based batch
splits across histogram rows, and the rows are not independent findings. Read a
delta together with the image named beside it.

## Two false positives of the method

Recording these is what makes the rest of the count credible - both are the
sweep being wrong, not the dumps.

**`+0x2800` (8 dumps, `overlay_world_map_top_ext`).** PROT 0901 resolves
through its documented PROT 0900 sibling alias. The bytes genuinely appear in
both images at a `0x2800` offset; the sweep picks the wrong one. Not a base
error.

**`+0x4000` (10-12 dumps, `overlay_slot_machine`).** A **stale local artifact**,
now understood: `extracted/overlays/overlay_slot_machine_0973.bin` contains
PROT **0973** (`move_program_no`), not the slot-machine overlay. Its filename
embeds a `prot_index` that the overlay map has since corrected - the map's
entry reads `prot_index = 975` (`other_game`) and its recorded fingerprint
matches PROT 0975, not the local file. The extractor derives the filename from
the map (`bin_filename()` = `overlay_<label>_<prot_index:04>.bin`), so the
committed code and the map are both correct; the local `.bin` simply predates
the correction and was never regenerated.

Nothing to fix in the repository, therefore - the fix is to **re-extract**, and
the generalisable trap is worth more than the instance: `extracted/` is
gitignored, so a stale image from an older map revision survives indefinitely
on one machine and silently mis-attributes every dump taken from it. **Delete
and regenerate `extracted/overlays/` after any change to
`static-overlays.toml`.** A filename that disagrees with the map is the tell.

### Measured: what a stale extraction directory actually looked like

The trap above is not hypothetical. A regeneration of a working checkout - 15
images on disk against 25 map rows - produced this:

- **10 images byte-identical** to the fresh extraction. The bytes on disk were
  never the problem.
- **15 images absent entirely**, including `overlay_world_map_render_0901.bin`
  and `overlay_battle_tutorial_0967.bin` - both needed by live analysis, both
  re-extracted by hand at the time rather than being noticed as missing.
- **5 images carrying the wrong identity**: `overlay_dance_dark_eclipse_0927`
  held summon Juggernaut, `overlay_dance_hells_music_0907` held summon Nighto,
  `overlay_dance_ultimate_rave_0924` held the stager, `overlay_summon_gimard_0905`
  held `summon_stager_x83` (gimard is 0903), and `overlay_slot_machine_0973` held
  0975.

The mis-identified five are the dangerous class, and they fail in the same shape
as a mis-based dump: **plausible bytes under a wrong label**. Anyone porting the
dance minigame would have opened `overlay_dance_dark_eclipse_0927.bin`, found
valid MIPS, and ported summon code into the dance module. No gate in this
repository can catch that - not `fmt`, not `clippy`, not the doc gates, not the
tests, because the resulting code is internally consistent and merely wrong about
what game system it implements.

`asset overlay verify <PROT.DAT>` is the cheap check: it re-extracts from the
disc and asserts every committed fingerprint reproduces. If it passes while the
local directory disagrees, the map and the disc are fine and the *directory* is
stale. Run it before any work that reads `extracted/overlays/` in bulk.

## The five hand-verified dumps

Confirmed instruction-by-instruction against `overlay_field_0897.bin` at base
`0x801CE818`, independently of the sweep.

| Dump | Header instruction | At that VA in 0897 | Real VA | Diagnosis |
|---|---|---|---|---|
| `overlay_0897_801e0b1c.txt` | `lw v1,-0x4bb0(s1)` | `addiu v0,v0,-5` | `0x801EF334` | `+0xE818`. Interior label of `FUN_801ef2b0`, not a function. |
| `overlay_0897_801e1c64.txt` | `sh s0,0x54(s4)` | `lbu v0,0x3(s6)` | `0x801F047C` | `+0xE818`. |
| `overlay_0897_801e1d98.txt` | `li v0,0x74` | mid-stream | `0x801F05B0` | `+0xE818`. Also a delay-slot-misaligned carve-out of the previous dump's body. |
| `801dba20.txt` | - | - | - | Not a dump of `FUN_801DBA20` at all; its own header reads `entry=801db7f4`. |
| `overlay_0897_801dbec4.txt` | `lw a0,-0x3c9c(v0)` | `addiu v0,v0,-1` | - | Prefix disagrees with the bytes. |

`FUN_801e0b1c` is the instructive one. It was cited in committed docs and in a
port tag as the tile-board procedural fill. There is no function at that
address; there is not even an instruction boundary worth naming. The citation
survived because the dump looked complete and its filename looked specific.

## Printed VAs resolved against the extracted images

A second hand-verified batch, resolved the same way: take the dump's opening
instruction stream, find those exact words in an extracted overlay image, and
report the VA the bytes actually occupy. Every row below is a **printed** VA
that had a dump but no real function entry behind it - the reason each one sat
in the corpus looking like unported work.

The pattern generalises: a mis-based print and a genuine interior fragment are
indistinguishable from the dump alone, and both are common enough that "there
is a dump at this address" carries almost no information about whether a
function lives there.

| Printed VA | Dump | Bytes really live at | Reading |
|---|---|---|---|
| `0x801DCAA0` | `overlay_0897_xxx_dat_801dcaa0` | field (0897) `0x801EB2B8` | `+0xE818`. Interior of the world-map debug-menu renderer `FUN_801EAD98`. |
| `0x801DF510` | `801df510` | field (0897) `0x801EDD28` | `+0xE818`. Interior of the battle-records screen `FUN_801ED710`; its first printed instruction is a delay slot and its back-branch leaves the window. |
| `0x801DFEF4` | `overlay_0897_xxx_dat_801dfef4` | field (0897) `0x801EE70C` | `+0xE818`. Frameless slice of `FUN_801EE5D4`. At the correct base the VA is a lone `j 0x801E212C` inside the field VM `FUN_801DE840`. |
| `0x801E0BE8` | `overlay_0896_bat_back_dat_801e0be8` | field (0897) `0x801E6400` | `+0x5818`. A real entry, the world-map numeric-field draw `FUN_801E6400`, printed at a VA no runtime image uses. |
| `0x801E205C` | `overlay_0896_801e205c` | field (0897) `0x801E7874` | `+0x5818`. Interior of the world-map controller `FUN_801E76D4`. |
| `0x801E249C` | `overlay_0897_xxx_dat_801e249c` | - | The dump's stream starts at `0x801DAAAC`, a disjoint region. At the correct base the VA is a lone `j 0x801E3628` inside the field VM `FUN_801DE840`. |
| `0x801E5520` | `overlay_0897_801e5520` | battle-action (0898) `0x801CED38` | Two words of ASCII string data decoded as code, word-exact at 0898 file `+0x520` (`- 0x167E8`); the field image holds a real instruction at the print-correct offset, which Ghidra would have decoded rather than printing `SPECIAL2` garbage. Separately, the *VA* `0x801E5520` in the field image is an intra-function `j` label of `FUN_801E5338`, reached from `0x801E537C` / `0x801E538C` / `0x801E54D0` / `0x801E54D8` - a fact about the image, not about this dump's bytes. |
| `0x801E9D8C` | `801e9d8c` | battle-action (0898) `0x801D35A4` | `+0xE818`. Interior of `FUN_801D344C`. |
| `0x801E9F48` | `overlay_0896_801e9f48` | field (0897) `0x801EF760` | `+0x5818`. Interior of the tile-board walk SM `FUN_801EF2B0`. |
| `0x801F04B0` | `overlay_0896_801f04b0` | battle-action (0898) `0x801D0CC8` | `+0x5818` lands in 0897's over-read tail, i.e. 0898's own image. Interior of the battle dispatcher `FUN_801D0748`; the fragment exits `j 0x801D3290`, that function's epilogue hop. |
| `0x801F7E4C` | `overlay_muscle_dome_801f7e4c` | PROT 0900 `0x801F7E4C` | Base-correct but interior: inside the sprite-widget handler `FUN_801F7A9C`. |
| `0x801F8080` | `overlay_muscle_dome_801f8080` | PROT 0900 `0x801F8080` | Base-correct but interior: inside the sprite-widget spawner `FUN_801F8004`. Opens in a delay slot. |
| `0x801F8190` | `overlay_muscle_dome_801f8190` | PROT 0900 `0x801F8190` | Base-correct but interior: inside the screen-mask widget handler `FUN_801F811C`. |
| `0x801F92A4` | `overlay_muscle_dome_801f92a4` | PROT 0900 `0x801F92A4` | Base-correct but interior: inside `FUN_801F91D8`. |
| `0x801E1538` | `overlay_0897_801e1538` | field (0897) `0x801EFD50` | `+0xE818`. Opens with a load whose base register is never set in the window - a frameless slice, not an entry. |
| `0x801E158C` | `overlay_0897_801e158c` | field (0897) `0x801EFDA4` | `+0xE818`. Opens in a delay slot (`_nop`) and exits `j 0x801EFEA0`, a VA outside its own printed window. |
| `0x801E175C` | `overlay_0897_801e175c` | field (0897) `0x801EFF74` | `+0xE818`. |
| `0x801E22C4` | `overlay_0897_801e22c4` | field (0897) `0x801F0ADC` | `+0xE818`. A real entry with a prologue - a five-case state machine on `s16 arg[+0x54]` through the jump table at `0x801CF734` - printed at a VA no runtime image uses. |
| `0x801E5134` | `overlay_0897_xxx_dat_801e5134` | battle-action (0898) `0x801CE94C` | `- 0x167E8`. An earlier `+0xE818` reading ("field `0x801F394C`") resolved against the pre-correction over-read field image; 0897's own content ends at `0x801F3818`, so the bytes are 0898 file `+0x134`. The *other* dump at this printed VA, `overlay_0897_801e5134`, is print-correct field code - two programs, one VA, two owners. |
| `0x801EC370` | `overlay_0897_801ec370` | field (0897) `0x801FAB88` | `+0xE818`. The dump's own body jumps from `0x801EC394` straight to `0x801ED920`, i.e. it splices two disjoint regions - a second reason not to read its addresses. |
| `0x801E6A7C` | `overlay_0896_801e6a7c` (cite of `FUN_801E66D8`) | field (0897) via `+0x5818` | The enclosing dump `overlay_0896_801e66d8` is itself `SHIFTED +0x5818`, so the cited interior VA is phantom twice over. |
| `0x801E8B34` | `overlay_0896_801e8b34` (cite of `FUN_801E8B10`) | field (0897) via `+0x5818` | Same shape; enclosing dump resolves to `0x801EE328`. |
| `0x801EA074` / `0x801EA348` | `overlay_0896_801ea074` / `_801ea348` (cite of `FUN_801E9FD4`) | field (0897) via `+0x5818` | Same shape; enclosing dump resolves to `0x801EF7EC`. **Not** the enemy AGL action picker - that `FUN_801E9FD4` is the *battle-action* image's function at the same VA, a different dump. |
| `0x801EC228` | `overlay_0896_801ec228` (cite of `FUN_801EC204`) | field (0897) via `+0x5818` | Same shape; enclosing dump resolves to `0x801F1A1C`. |
| `0x801EF648` / `0x801EF6E0` / `0x801EF7B4` | `overlay_0896_801ef6e0` and its two cites | field (0897) via `+0x5818` | Same shape; the enclosing dump resolves to `0x801F4C78`. |
| `0x801E65F8` | `overlay_0896_bat_back_dat_801e65f8` | field (0897) `0x801EAFD8`, low confidence | Reported `+0x49E0` on an 11-hit signature, so the resolution is weak. Independent of that, the dump is a frameless fragment - it opens mid-flow with a `div` and a `break 0x1C00` divide guard - so no function starts at the printed VA either way. |
| `0x801FFBA4` | `overlay_0896_bat_back_dat_801fa38c`, `overlay_0897_xxx_dat_801f138c` | battle-action (0898) `0x801DABA4` | Three-way confirmed: both mis-based dumps and the base-correct `overlay_battle_action_801daba4` are 1408 bytes / 352 instructions with an identical opening. `0x801FFBA4` sits in 0897's over-read tail, so the field-overlay resolution has to be re-keyed into 0898 by the table above. Cite `FUN_801DABA4`. |

Read the `overlay_0896_*` rows together: the whole group is one mis-based batch
seen through a cite-pointer, and a **cite of a shifted dump inherits the shift**.
The corpus stores mid-function citations as their own files, so a phantom entry
address can spawn several more phantom interior addresses, each of which looks
like an independent unported function in a worklist.

## Region-window dumps are not addresses

A second shape that reads as an address but is not one. `dump_levelup_data_section.py`
emits **fixed 4 KB hex windows** over the level-up overlay's data segment, one
file per window, named `overlay_magic_level_up_data_0x<base>.txt`. The header
line says `DATA REGION 0x801F1000..0x801F1FFF`, not `FUN_`; the body is a
`C`/`D`-annotated hexdump, not a disassembly.

Any tool that recovers an address from a dump filename therefore mints entries
at `0x801C8F00`, `0x801F0000`, `0x801F1000`, `0x801F2000`, `0x801F3000`,
`0x801F4000`, `0x801F5000`, `0x801F6000`, `0x801F7000` and `0x801FA000` - the
window bases, spaced on round 4 KB boundaries. **The roundness is the tell.**
Nothing in the retail link lands ten function entries on exact 4 KB multiples.
None of these is a function and none is a port site.

## PROT 0900's head window (`0x801F69D8..0x801F6A84`)

The third shape, and the one that produces the largest single cluster of false
entries: a run of eighteen `FUN_` pseudo-entries at 4- and 8-byte spacing in
`overlay_muscle_dome.bin`, covering `0x801F69D8`, `0x801F69E8`, `0x801F69EC`,
`0x801F69F0`, `0x801F69F4`, `0x801F69F8`, `0x801F69FC`, `0x801F6A00`,
`0x801F6A08`, `0x801F6A10`, `0x801F6A18`, `0x801F6A30`, `0x801F6A34`,
`0x801F6A3C`, `0x801F6A40`, `0x801F6A58`, `0x801F6A74` and `0x801F6A84`.

Read the dumps and the cluster falls apart on its own terms. Every member is at
most 8 bytes. Ten report `size=1 bytes, 0 instructions` with Ghidra's
"bad instruction data" warning - it could not decode even one instruction. The
rest decode to a single nonsense word each: a `beq` into the middle of the run,
a `jal 0x8C3C0004`, and one that Ghidra named `thunk_EXT_FUN_8C000000` because
the word looks like a jump into the KSEG1 hardware window. **Four-byte spacing
between `jal` targets is not a function layout.** It is a table.

And `0x801F69D8` is a known address: it is PROT 0900's slot-B link base, and
the overlay-resident dispatcher `FUN_801F2D68` indexes a jump table there with
`jr *(0x801F69D8 + sub*4)` (see
[move-vm.md § screen-effect widget family](../subsystems/move-vm.md#screen-effect-widget-family-prot-0900)).
`crates/asset/data/static-overlays.toml` records the same head being referenced
at `+0x00`, `+0x20` and `+0x84` from PROT 0977's code. So the window holds the
module's head pointer/string data, the surrounding slot-A code words decode as
`jal` into it, and Ghidra dutifully minted an entry per target.

The capture provenance closes it: `overlay_muscle_dome.bin` is a Duckstation
save-state RAM slice, so slot A and slot B are whatever the emulator held at
that instant and need not be the pair the slot-A code was linked against.
Treat all eighteen as data. None is a port site, and the surrounding real
functions of that band belong to PROT 0900, not to the Muscle Dome.

The four `overlay_muscle_dome_*` rows are the instructive ones, because their
base is *right* and the label is wrong. PROT 0977 (Muscle Dome) is a slot-A
overlay; a dome capture's slot B holds whatever render library is resident, and
here that is PROT 0900. Every one of those four VAs disassembles byte-identically
out of `0900_xxx_dat.BIN` at base `0x801F69D8`, inside the
[screen-effect widget family](../subsystems/move-vm.md#screen-effect-widget-family-prot-0900).
None of them is dome logic. A `overlay_<minigame>_` prefix names the *capture*,
not the code.

`FUN_801F91D8` is the one enclosing body in that band with no separate write-up:
a PROT 0900 scene-draw setup routine that seeds the render scratchpad window
(`0x1F8002A8` / `0x1F8002CC` / `0x1F8002EC`) from the camera globals `0x8007BF10`
and `0x8007B790`, snapshots the scratchpad view bytes `0x1F800384/385` and
`0x1F8003E8..3EB` into overlay-local slots from `0x801F8EE0`, and then runs the
draw through `FUN_80026988`.

## Three cheap tells that a dump is not a function

The sweep needs ten signable instructions, so the short dumps - the largest
class in the corpus - get no verdict from it. These three checks cost one look
at the disassembly section and settle most of them without any tooling.

**The first printed line is a delay slot.** Ghidra prints a delay-slot
instruction with a leading underscore (`_li v0,0x8`). A function cannot begin
with one, so the dump is a slice of a body whose branch is above the window.
`0x801E0F40` is the minimal case: three instructions, opening `_li v0,0x8`,
closing `j 0x801EFEA0`. `0x801E0F24` is the same shape inside a body that *is*
identified - the dump's own header names the enclosing function
`FUN_801DE840`, the field/event VM, and script-vm.md catalogues the VA as the
`switchD_801e0f24::caseD_4` label.

**No prologue and an unconditional `j` for an exit.** A real leaf ends `jr ra`.
`0x801E015C`, `0x801E08C4`, `0x801E0DF0` and `0x801E2640` all open mid-flow
with no `addiu sp,sp,-N` and leave through `j` to a shared epilogue
(`0x801EED24`, `0x801EF228`, `0x801EFEA0`). They are basic blocks of larger
overlay routines.

**The disassembly contains instructions the R3000A does not have.** This is the
strongest tell available, because it needs no context at all. `0x801E5E84`
decodes as `andi zero,...` followed by `tge` - a MIPS-II trap instruction.
`0x801E60A8` decodes as `jalx` and `daddi` - MIPS-16 and 64-bit opcodes. The
PSX CPU implements neither. Any window that disassembles to them is data being
rendered as code, and the surrounding "function" is fiction. `0x801E45AC`
(four `nop`s - alignment padding) and `0x801E565C` (`size=1 bytes, 0
instructions`) are the degenerate cases of the same thing.

`0x801ECC00` is worth naming separately: three independent images
(`overlay_battle_action`, `overlay_battle_action_0898`,
`overlay_0896_bat_back_dat`) all dump it as `NOFUNC - no analyzed function at
or containing this address`. Three misses agreeing is about as clear as the
corpus gets.

## Tagged is necessary, not sufficient

The obvious remedy - "trust tagged dumps, discard untagged ones" - does not
hold, and this is the strongest reason to read the bytes rather than any
metadata.

`overlay_0897_801de840.txt` is correctly tagged
`[overlay_0897 base=0x801CE818]`, resolves `MATCH`, and is the field VM's
authoritative dump. It also has **silent gaps**: no ellipsis, no marker, just
addresses that stop being consecutive.

| Gap | Consequence |
|---|---|
| `801df8d8` → `801df8e4` | Hides `801df8dc`, the epilogue hop the nibble-7 no-mask paints return through. |
| `801e1d94` → `801e1e20` | Hides the whole sub-2 arm of the collision-grid wall paint. |
| ends before `0x801e3624` | Hides the function epilogue itself. |

Those are precisely the addresses two separate audited claims turned on, and
reading the dump alone produced a wrong mechanism for both: a "shared continue
label" that is in fact the function epilogue, and a flat 7-byte operand width
for an op that is 6 bytes in two of its four arms. Both were settled only by
disassembling the image directly.

So a tag proves the *base*. It does not prove *completeness*.

## Attributing an extent to an image

Several overlays load at one base, so a dump extent can fall inside more than
one image's mapped span. Address arithmetic cannot separate those cases and it
is not a close call: the two spans
[`disc-coverage.py`](disc-coverage.md) measures are **nested**, one wholly
inside the other, so every extent in the overlap belongs to both by
construction and the inner image is 100% ambiguous however good the corpus is.

[`attribute-dump-extents.py`](../../scripts/ghidra-analysis/attribute-dump-extents.py)
asks the bytes instead: it canonicalises each dump's opening window and reports
which image's **own content** reproduces it at that VA. The committed result is
[`dump-extent-attribution.csv`](../../scripts/ghidra-analysis/dump-extent-attribution.csv),
keyed by `(entry, bytes)` - the extent, not the dump filename, so adding a dump
does not invalidate a row and the artifact does not rot the way a per-dump table
would.

| Class | Meaning | What a consumer should do |
|---|---|---|
| `unique` | One image's own content reproduces the window here. | Credit that image only. |
| `identical` | Several images hold byte-identical code here. | Credit each; the dump documents all of them. |
| `divergent` | Dumps at this extent resolve to different images. | Genuinely several routines; leave ambiguous. |
| `misbased` | No image holds these bytes here; they live elsewhere. | Credit no image - the extent is fiction. |
| `unresolved` | The bytes are in no extracted image at any VA. | Leave ambiguous. |
| `short` / `data` / `gapped` / `no_disassembly` | The window cannot sign. | Leave ambiguous; `data` and `gapped` credit nobody. |

### Own content, not the extracted file

The cut matters more than the comparison. An extraction is the entry's
`read_entry` footprint and runs into its neighbours' sectors, so a raw file
answers for VAs its overlay never loads - with a neighbour's code. Two cuts are
available and they are not equally good: a cited `clean_copy_bytes` length, and
the sector-aligned offset at which another image's head appears. Where both
exist they agree, which is what licenses the second where the first is absent -
and most rows have only the second, so the artifact records which one each image
used.

### The residue is the finding

Roughly three fifths of ambiguous extents attribute to exactly one image, and a
further fifth attribute to *no* image because the print is mis-based. What is
left does not yield to more effort of the same kind, and the largest share of it
is not a corpus gap but a **dump** gap: windows too short to sign, dumps that
carry decompiled C and no instruction stream at all, and gapped streams. Those
are repaired by re-dumping, not by extracting another overlay.

Lowering the signature floor does not help, and that is measured rather than
assumed: going from eight instructions to five moves a handful of extents and
changes the per-image ambiguity by well under a point, while making every
verdict rest on less evidence. The floor is not the binding constraint.

One image's row can therefore become meaningful while the other's does not, and
that is a legitimate result rather than a half-finished one. The inner of two
nested spans starts at total ambiguity and keeps a larger share of the residue,
because most of what it loses is loss to *other* images rather than to itself.

## A complete dump can still read as no evidence at all

Three of the shape classes are not about what a dump omitted. They are about
what the *reader* could not see, and each traces to one line in the script that
wrote the file. The dumps themselves are whole and correct.

**The address column.** One dumper writes `ins` where the others write
`ins.getAddress(), ins`, so its disassembly section carries the instructions and
nothing to key them by. Every instrument over this corpus is keyed on the
printed address, so such a file resolves to zero rows, is graded as carrying no
disassembly, and is counted as a dump that needs re-taking. A 2781-instruction
body reads as an empty one. The class is `ADDRLESS_DISASM`, and the addresses
are recoverable without Ghidra - a body is contiguous, so row `i` is
`entry + 4*i`.

**The section marker.** The same dumper emits `--- DECOMPILED C ---` rather
than `--- DECOMPILED ---`. A reader looking for the standard marker never
leaves the disassembly section, so the whole C rendering is parsed as more
instruction rows. That is why the defect compounds: the file looks like it has
*more* stream than it does, in a format that matches nothing.

**The filename.** Covered under the dumper defect below, and the largest of the
three.

The generalisable point is that a dump has two audiences - a human reading the
instructions, and a tool joining on the addresses - and only the first of them
notices when a field is missing rather than wrong. A defect that a human reader
would never spot is exactly the one that propagates furthest, because nothing
about the file looks incomplete.

`check-dump-base-integrity.py --audit-dumpers` reports which
`ghidra/scripts/*.py` still carry each of the three. Repairing dumps without
repairing the script that wrote them regenerates the defect on the next run.

## Not every file in `funcs/` is a dump

The corpus stores answers as well as dumps, and three kinds of answer are not
defective dumps however a shape sweep grades them:

| Shape | What it is |
|---|---|
| `POINTER_STUB` | `== citation pointer 0x<addr> ==` / `== <addr> (cite of FUN_<addr>) ==`. A mid-function citation recorded as a file naming the enclosing dump. |
| `NOFUNC_RECORD` | `== NOFUNC <addr> ==`, or a `--- PSEUDO-DISASSEMBLY WINDOW` section. A recorded negative, and a window explicitly not a function body. |
| `NOT_A_DUMP` | An analysis script's output whose filename happens to end `_<addr>.txt`. |

A pointer stub is the corpus doing the *right* thing with an interior address:
the alternative is a dump file whose name asserts an entry point that does not
exist. Counting it as a defect therefore penalises the one handling that avoids
the defect it is being counted as. Nearly a fifth of what a cited-only shape
sweep once called defective was this - the instrument scoring the corpus's own
good work against it.

The same lesson applies to the header regex that finds those files: it accepted
one spelling of `(entry=…)` while three exist in the corpus, so real dumps fell
into the headerless classes. **A parser's strictness is a claim about the
corpus, and an over-strict one manufactures a gap.**

## Every instrument in this chain has had a defect that made a number look better

Worth stating as a standing caution rather than a grievance, because the pattern
repeats and it has one shape. The measurement layer over this corpus has
produced more defects than the code it measures, and none of them announced
itself: each returned a plausible number, in a plausible format, with no error.

The catalogued ones span every stage. A dumper names its output after the
address *requested* while Ghidra resolves the one *containing* it, so a file
asserts an entry point that does not exist - the signature is that the resolved
entry is always **below** the requested address, which nothing else produces.
A function walker stops at the first unconditional `j` and reports a 259-
instruction body as 85. Its replacement, unbounded, merges a run of routines
into one body and mints a phantom interior for every entry it swallows. A
canonicaliser leaves one operand spelling unfolded and the mismatch lands in the
class that reads as missing data. A shape sweep counts pointer stubs and
recorded negatives as defective dumps, so the corpus's own correct handling of
an interior address scores against it. A status string grows a parenthetical
while its caller still compares it with `!=`. An audit applies a test that can
only refute one kind of claim to a list containing two kinds, and re-raises
every row of the other kind forever. An entry test reads a prologue as a
boundary when a routine can begin a few instructions before its frame.

Two of those were introduced *by* a repair pass, which is the part worth
sitting with: fixing a measurement defect is itself a measurement change, and it
lands in the same blind spot as the defect it fixes. A repair pass therefore
needs its own undo - `repair_truncated_dumps.py` grew a `+addr` restore mode for
exactly this, because the rebuild that over-reached had already deleted the
function entries that proved it wrong.

### A repair pass cannot be its own control

The entry/interior verdict a repair pass reports is read out of the same Ghidra
database the pass has been rewriting, so the two are not independent and no
amount of care makes them so. What *is* available is the sign of the bias: a
rebuild merges bodies and therefore manufactures interiors, a restore splits
them and therefore manufactures entries. Count both, and the direction of the
error follows - a pass dominated by restores under-reports interiors, and its
interior count is a floor rather than an estimate.

State the direction alongside the number. "126 of 314, and the bias runs
downward" is a usable claim; "126 of 314" from a mutated database is not, and
re-deriving it on a fresh import is the only way to remove the qualifier
rather than merely to restate it.

The common factor is not carelessness, it is that **a measurement instrument has
no oracle**. Code that is wrong eventually crashes or renders the wrong pixel;
a counter that is wrong just prints. So the defences are structural: cross-check
one instrument against another built on different evidence, prefer the class an
error would land in being *loud* over it being small, and treat any negative or
"unverifiable" class as the place to look first, because that is where a broken
comparison and a genuine absence are indistinguishable from the outside.

## The remedy

Disassemble from the extracted image, not from the dump:

```
image:       extracted/overlays/overlay_field_0897.bin
base:        0x801CE818
file offset: va - 0x801CE818
```

For other overlays take the base from
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml); for the
always-resident executable use `extracted/SCUS_942.54`, text base `0x80010000`,
file offset `0x800 + va - 0x80010000`.
[`disasm-overlay-fn.py`](../../scripts/ghidra-analysis/disasm-overlay-fn.py)
does this directly. Validate any new base by disassembling one known anchor and
comparing against a `MATCH` dump before trusting the rest.

Its walk ends the body at a `jr ra` or an outbound `j` only once nothing already
walked branches **past** it. Neither half of that rule is safe alone, and each
fails silently in the opposite direction: stopping at the first `j` truncates
any routine that jumps forward to a shared epilogue, and stopping at the first
`jr ra` truncates any routine with an early-exit arm. A walk that ends any other
way - the instruction cap, the end of the input, an explicit `--max-size` -
prints an `INCOMPLETE BODY` marker, because an instruction count that is really
a lower bound is indistinguishable from a whole body once it is quoted
somewhere else.

### The frontier rule needs an upper bound, or it fails the other way

The rule above is stated as a fix for two truncating rules, and read that way it
invites an unbounded frontier. Unbounded, it is worse than what it replaces. One
forward branch whose target lies beyond the routine drags the frontier past
every `jr ra` in between, and the walk swallows a run of functions into a single
body. Measured against
[`repair_truncated_dumps.py`](../../ghidra/scripts/repair_truncated_dumps.py):
a 68-byte routine became a 20060-byte one, and a 4-byte jump-table slot became a
32256-byte one.

That failure is loud in *size* and silent in *correctness*, and it does more
damage than the truncation it fixes, for two reasons. A rebuild deletes the
function entries inside its span, so real entries disappear from the project.
And every address inside the merged body then reports as an interior of it -
including addresses that are documented function entries - so the fiction
manufactures phantom-interior verdicts at exactly the rate it swallows
functions.

Two bounds close it, and both are needed:

- **A prologue after a return is a boundary.** A `jr ra` whose delay slot is
  followed by `addiu sp,sp,-N` ends the body whatever the frontier says: a
  function cannot push a frame twice without popping, so that frame belongs to
  the next routine. Used as an *end* test right after a return, this does not
  hit the trap that a routine may begin a few instructions before its frame -
  that trap is about the *entry*.
- **A budget on crossed returns.** A return not followed by a prologue is either
  a frameless leaf's end or a genuine early exit, and locally the two are
  indistinguishable. So the walk counts them and refuses past a small budget,
  because a body whose every return but the last is an "early exit" is far more
  likely to be several routines. A refusal is cheaper than a merged body.

The status string carries the crossed-return count into the verdict, so a body
that used its budget is visible rather than merely plausible. And the callers
test it with a prefix match, not equality - an earlier version compared
`status != "complete"` against a status that had grown a parenthetical, and
reported five *successful* walks as incomplete. **A status string that both
carries detail and is compared exactly is a bug waiting for the first detail.**

## Re-running the sweep

```bash
scripts/ghidra-analysis/check-dump-base-integrity.py
scripts/ghidra-analysis/check-dump-base-integrity.py --list-shifted
scripts/ghidra-analysis/check-dump-base-integrity.py --min-insns 4
scripts/ghidra-analysis/check-dump-base-integrity.py --emit-base-csv /tmp/b.csv
scripts/ghidra-analysis/check-dump-base-integrity.py --audit-dumpers
```

`--emit-base-csv` is the form a re-dump pass needs: per dump, the printed VA,
the VA the bytes resolve to, the delta, and the image. **A re-dump has to be
told which program to run against, and the filename is not evidence of that -
this is.** Feeding a phantom printed VA back into Ghidra dumps whatever
unrelated routine sits there, which is how a mis-based citation acquires a
second, freshly-generated dump backing it up.

Exit status is non-zero when any dump is `SHIFTED`. It needs `extracted/`
populated ([extraction.md](extraction.md)) and `capstone`; it reads only
gitignored, disc-derived inputs and prints no game data beyond instruction
mnemonics.

The per-dump list is deliberately not reproduced here. It is operational state
over a gitignored corpus that changes whenever anyone adds a dump, so a table
committed today would rot into a second source of exactly the wrong claims this
page exists to prevent. `--list-shifted` regenerates it in about a minute.

Run it after importing any program at a base recovered from call targets rather
than a documented anchor, and after changing `static-overlays.toml` - the two
cases where a base can be self-consistently wrong.

## See also

- [`phantom-print-index.md`](phantom-print-index.md) - this page's findings applied address-by-address to the `0x801C…` / `0x801D…` printed band.
- [`call-target-integrity.md`](call-target-integrity.md) - the sibling failure: what a decoded `jal` target does and does not prove.
- [`static-overlay-pipeline.md`](static-overlay-pipeline.md) - how an overlay's base is recovered and what makes a recovery load-bearing.
- [`ghidra.md`](ghidra.md) - the dump scripts, and the decompiler artifacts that have produced false claims.
