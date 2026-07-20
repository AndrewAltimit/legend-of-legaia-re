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
| `MATCH` | 1783 | Printed VA equals the VA the bytes resolve to. | Everything. Addresses, provenance citations, port tags. |
| `SHIFTED` | 215 | Bytes resolve at a constant non-zero delta. The dump was produced at the wrong load base. | Instruction *text* and decoded `jal` targets only. Never its addresses, and never as provenance for a function identity. |
| `NOT_FOUND` | 1007 | Bytes are in no extracted image. | Unresolved - see below. Not known-bad. |
| `SHORT` | 619 | Fewer than 10 instructions; too short to sign. | No verdict either way. |

Those four numbers are what the committed script prints at its default
threshold, and they are the ones to quote.

Lowering the threshold trades coverage for certainty. At `--min-insns 4` the
`SHORT` class shrinks to 468 and the sweep returns 2333 `MATCH` / 336
`SHIFTED` / 487 `NOT_FOUND` - but a 4-instruction signature also matches
*ambiguously*, so part of that growth is the method resolving dumps it should
have declined. Treat a multi-hit resolution as weaker than a single-hit one.
The clusters below are quoted at both thresholds for exactly this reason: the
counts move, the conclusion does not.

### `NOT_FOUND` is unverifiable, not wrong

This is the class most likely to be over-discarded, so state it plainly: **a
`NOT_FOUND` dump is not a bad dump.** The sweep can only resolve bytes against
images that were extracted statically from `PROT.DAT`. Much of the corpus was
dumped from *live RAM captures* - mednafen and PCSX-Redux save states - of
overlays that have never been statically extracted, or of runtime-mutated
memory that no longer matches its on-disc form. Those dumps have no source
image to resolve against and land here by construction.

About 120 of them carry `base=0x801C0000` in their own header tag, which is the
same suspect base as the `+0xE818` cluster below - so some fraction of
`NOT_FOUND` is probably mis-based too. It cannot be shown statically either
way. Treat `NOT_FOUND` as "unproven", verify against a capture before relying
on its addresses, and do not delete it.

## The shift clusters

The `SHIFTED` dumps are not scattered one-offs. Two clusters account for the
overwhelming majority, and both point at one mistake. Counts are given as
`default / --min-insns 4`.

| Delta | Count | Program | Reading |
|---|---|---|---|
| `+0xE818` | 137 / 187 | field overlay (PROT 0897) | Imported at base `0x801C0000` instead of `0x801CE818`. `0x801CE818 - 0x801C0000 = 0xE818`. |
| `+0x5818` | 35 / 43 | `overlay_0896_*` | Same field-overlay bytes, reached at PROT 0896's over-read base. |
| `+0xD018` | 6 / 8 | `overlay_0971` | Uncertain - see below. |

**`+0xE818` is a single mis-based batch run.** Every member resolves
single-hit into `overlay_field_0897.bin`, with a median of 35 consecutive
exactly-matching instructions. A constant delta shared by well over a hundred
dumps is not coincidence; it is one import performed at the wrong base, and
every dump taken from that program inherited it. Of the default-threshold
members, 119 of 137 are untagged - the untagged class is where this
concentrates.

**`+0x5818` corroborates the PROT 0896 over-read.** These dumps are labelled
`overlay_0896_*` yet their bytes resolve into the *field* overlay. That is
independent confirmation of what
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) already
argues on other grounds and what
[call-target integrity](call-target-integrity.md) found from the resolve-rate
seam: PROT 0896's footprint runs into its neighbour, so dumps taken at its
widely-cited base are reading field-overlay code. `0x801CE818 - 0x5818 =
0x801C9000`, the over-read base. PROT 0896's own link base remains unrecovered.

**`+0xD018` is unresolved.** A handful of `overlay_0971` dumps share it. PROT 0971 has
never been statically extracted, so there is no image to resolve them against;
the delta is inferred from partial hits and is the weakest row here. What would
settle it: extract PROT 0971 (`asset overlay ...`, see
[static-overlay-pipeline.md](static-overlay-pipeline.md)) and re-run the sweep.
If they then resolve single-hit at a constant delta, it is a third
mis-based batch; if they resolve at zero, the delta was an artifact of matching
against the wrong image.

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

## Re-running the sweep

```bash
scripts/ghidra-analysis/check-dump-base-integrity.py
scripts/ghidra-analysis/check-dump-base-integrity.py --list-shifted
scripts/ghidra-analysis/check-dump-base-integrity.py --min-insns 4
```

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

- [`call-target-integrity.md`](call-target-integrity.md) - the sibling failure: what a decoded `jal` target does and does not prove.
- [`static-overlay-pipeline.md`](static-overlay-pipeline.md) - how an overlay's base is recovered and what makes a recovery load-bearing.
- [`ghidra.md`](ghidra.md) - the dump scripts, and the decompiler artifacts that have produced false claims.
