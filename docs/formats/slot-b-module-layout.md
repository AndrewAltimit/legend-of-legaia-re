# Slot-B module image layout

The 64 PROT entries `0903..=0966` are one file format wearing 64 different
choreographies: the per-spell summon stagers and capture-class cast modules
that timeshare the second overlay window at link base `0x801F69D8`.
[`cast-module.md`](../subsystems/cast-module.md) is the *behaviour* page - the
entry tables in PROT 0898, the `ctx+0x279` phase machine, the damage shape.
This page is the *file* page: which bytes of an image are what, and how each
region is recovered from the image alone.

Parser `legaia_asset::slot_b_module`; the spawn records it names are the same
records [`cast_effect_pool`](../subsystems/cast-module.md#what-the-port-runs)
stages, at a granularity the pool does not carry.

## The regions

| Region | Contents | Recovered by |
|---|---|---|
| head table | 0 to 256 words of in-image VAs - the jump table of **one** of the module's two switches | leading run of words inside `[base, base + len)`, stopping at the first framed function |
| code | the tick's phase machine, the `0x801F6734` spawn stager, the capture-class trampolines | frame matching (below) |
| spawn-record band | `[i16 model_sel][u16 reserved][move-VM bytecode]` records | the consumer's own pointer-forming instruction (below) |
| inherited tail | a byte-identical, same-file-offset copy of another extracted image's bytes | out of scope here - see [cast-module.md](../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes) |

Which switch owns the head table varies across the band, and the arm count
settles it: read the `sltiu` immediate, per
[cast-module.md](../subsystems/cast-module.md#image-anatomy-recovered-from-the-bytes).
The parser bounds the table without deciding its owner - the two questions are
independent, and only the extent is a byte-accounting claim.

### The regions are not laid out head-to-tail

The obvious reading - head table, then code, then everything above the last
function is data - is wrong for at least two images. PROT 0943 (`cast_curse`)
and PROT 0961 (`cast_dead_end_crisis`) both carry **framed code above their
record band**: 0943's frame-matched partition is four bodies ending at file
`+0xD98`, then records, then three more bodies at `+0x135C..+0x17E0`; 0961's is
three bodies to `+0x10DC`, records, then two more at `+0x1C60..+0x1D90`. A rule
that reads the band as "past the last function" swallows those five bodies.

They are real routines, but they are **not this image's**. All five sit above
the image's own-content cut - 0943's own bytes end at file `+0x1037`
(VA `0x801F7A0F`), 0961's at `+0x1918` (VA `0x801F82F0`) - and are
byte-identical to PROT 0942 and PROT 0960 at the same file offsets. They are
inherited residue, and a Ghidra dump prints at each of them under *both*
images' names, so a dump is not evidence of ownership here. Read them at the
donor: [`cast-module.md`](../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes)
and `scripts/ghidra-analysis/inherited_tail.py`.

So a record claim is cut at the next framed function's prologue, never run to
the image end. The cut is still load-bearing - a claim that ran past it would
cover code, whichever image the code belongs to - but its justification is the
residue, not a second code region of the image's own.

## Frame matching

A body opens at `addiu sp, sp, -F` and closes at the first `jr ra` whose delay
slot is `addiu sp, sp, +F` for the same `F`. This is the partition
`ghidra/scripts/dump_static_overlay.py` drives its dumps from, and
`legaia_asset::slot_b_module::framed_functions` reproduces its committed
`RANGES` rows. It survives the three shapes that break a count-and-interleave
rule: a frameless leaf, an early `jr ra` inside a body, and a `jr ra` word that
is data in the image's tail.

### It does not find a frameless arm, and the head table does

Frame matching answers "where does this body end", not "where do bodies
start": a **frameless leaf** has no prologue, so nothing in the scan names it.
The head jump table does, and a table word is the stronger evidence of the two -
a routine reached only through a table is still a routine.

PROT 0949 is the worked case, and it is also a worked case for reading the
**consumer's** pointer-forming instruction rather than counting table words.
Its stager `FUN_801F75BC` dispatches `sltiu v1, a1, 0x8` through a table based at
`0x801F69F0` - six words into the image's leading VA run, not at the image head -
so the table it owns is eight arms, `0x801F69F0..0x801F6A10`. Arm 0 is the
fall-through body at `0x801F761C`, inside the stager's own extent; arms 1..7 are
seven 20-byte frameless leaves at `0x801F7630 + (i-1)*0x14`.

The eight arms are one eight-step ramp on the victim actor: `actor[+0x0C]` (the
tint blend) `0x200` .. `0x1000` against `actor[+0x21D]` (the anim rate) `7` .. `0`.
The frame scan sees none of the seven, so the 140-byte run they occupy read as
"the module's own data region" and was filed under a settled thread saying the
band's un-dumped runs are not code. It is code: see
`ghidra/scripts/funcs/overlay_cast_water_crystals_0949_801f7630.txt` and its six
siblings. When a band image's residue is being classified, read the dispatcher's
`lui`/`addiu` table base and its `sltiu` bound before reading the frame
partition.

## The spawn-record band

A record is named by an **instruction**, never by a statistic. The module
materialises the record's address with a `lui` / `addiu` pair and passes it in
`$a2` to a `jal` into one of two spawn helpers:

| Helper | What it is |
|---|---|
| `FUN_80021B04` | the SCUS actor-spawn helper - seats the record as a move-VM actor (`actor[+0x48] = record`, `actor[+0x70] = 2`) |
| `FUN_80050ED4` | its pool-tracked wrapper - takes the first free slot of the `DAT_801C90F0` pool and forwards `(world_pos, src_pos, record, scale)` unchanged |

The record format is the summon part record
([move-power.md](move-power.md#effect-prototype-records---the-spawn-path)):
`[i16 model_sel][u16 reserved][move-VM bytecode]`, `model_sel = -1` for a
transform node, a small library index for a modelled part, `0x4000` / `0x4001`
for the two render-mode nodes `FUN_80021B04` special-cases.

**One record's end is the next record's start**, so both ends of a claim are
addresses the module's own code computes. That is the whole of the evidence,
and it is why the band needs no length field, no terminator scan and no
entropy test.

### `+0x02` is reserved, not a flags word

A record header of the shape `[i16 model_sel][u16 ?]` reads as a flags word by
default, and that default is not a measurement. Nothing reads `+0x02`:
`FUN_80021B04` loads only `($a2)` (`lh` at `0x80021B2C`, `lhu` at
`0x80021B30`, and again at `0x80021BD0` / `0x80021C98` / `0x80021CB4`), and the
move VM starts the program at halfword index 2 (`actor[+0x70] = 2`, `sll v0,1`
at `0x800230B8`), so `+0x02` is stepped over rather than fetched. It is zero in
every band record and in every PROT 0898 effect-prototype record. Confidence on
the halfword is therefore **Unknown**, and it is named `reserved` wherever the
docs quote this record shape. Both legs of that evidence - `FUN_80021B04`
loading only `($a2)`, and the move VM starting at halfword index 2 - are
properties of the *record shape*, not of the band, so the rename covers every
carrier of it: the summon stagers, the cast band, the PROT 0898 effect
prototypes, the per-scene prescript stagers and the battle-overlay burst
records. The four Rust structs that carry the halfword
(`legaia_asset::summon_overlay::SummonPart`,
`legaia_engine_core::summon::SummonPartRuntime`,
`legaia_engine_core::world::ambient::AmbientPart`,
`legaia_engine_vm::battle_burst::BurstRecord`) name it `reserved` too.

### The filters, and which of them retail exercises

| Filter | What it excludes | Pointers it drops across the band |
|---|---|---|
| the resolved address must land in the image with room for a header | a neighbour's record, reached through the shared link base | 97 |
| the **call site** must lie inside a framed body of this image | an inherited fragment of a sibling's routine, whose pointer names the sibling's records | 5 records, 484 claimed bytes |
| an intervening `jal` between the `lui`/`addiu` pair and the consuming call voids the value - `$a2` is caller-saved | a pointer formed for something else earlier in the window | 9 |
| a target inside a framed function | a stale register the 22-instruction window mis-read as a record pointer | 0 |
| a `model_sel` outside the set `FUN_80021B04` dispatches | an address that lands on something the spawn path would never seat | 0 |

The last two never fire on retail, and saying so is the point of the column:
they are guards against a resolution failure this disc does not contain, not
findings about it. Quote the first three if the question is what the band's
extent rests on.

The call-site filter is the one the **inherited tail** makes necessary. Every
band image ends in a byte-identical, same-file-offset run of another extracted
image, so a module's buffer still holds a fragment of a sibling's body - and
that fragment's spawn calls are real `jal` words with real `lui`/`addiu` pairs
in front of them. PROT 0909 (`summon_viguro`) is the measured case: three of
its resolvable spawn calls sit at file `+0x199C` / `+0x19F4` / `+0x1A10`, which
are PROT 0908's calls at the same offsets, and they name three of 0908's
records. In 0909 those addresses hold nothing of 0909's, and without the filter
484 bytes of the band's 118 KB of claims are a sibling's records seen through
the build buffer. The filter is corpus-free: an inherited fragment's calls fall
outside this image's own framed partition.

What it does **not** rest on is residue never frame-matching. It often does:
PROT 0943's residue contains three complete bodies and PROT 0961's two, and six
band images (0908, 0910, 0920, 0943, 0945, 0961) carry a spawn call inside a
frame-matched body that is another module's bytes. In 0909's shape the fragment
starts mid-body, so its prologue is missing and the calls fall outside the
partition; in those six the frame is complete and the call passes. The filter
is a partition test, not a truncation test, and where the residue frames
cleanly it lets the donor's call through.

The cut that closes this is the image's own **content end**: a call site at or
above it is the donor's, whatever it frames as. `slot_b_module::parse_with_tail`
takes that offset and drops every call site and every record target from there
up. Over the band it removes a credited pointer on five images - 0908, 0910,
0920, 0943, 0961 - at offsets `0x26D8`, `0x26D8`, `0x1EF4`, `0x17E0`, `0x1DAC`.
PROT 0945 is the sixth image whose donor call site passes the frame test, and it
loses nothing, because its pointer never resolved to a record in the first
place. The content end comes from `inherited_tail.py` when the other images are
in hand, and from `slot_b_module::content_end` when only this one is.

### Bounding the highest record

The image's highest record has no next pointer above it, so the *band* cannot
bound it - but its **program** can. A record's payload is a move-VM program,
and a program ends where nothing above it can execute. Three words do that,
and the third is a fallback the first two outrank:

| terminator | why nothing above it runs |
|---|---|
| `0x08` HALT | sets `flags \| 8` and drops out of the tick loop |
| an armed `0x19` / `0x1B` | its paired `0x18` / `0x1A` loaded a counter carrying bit `0x4000`, which makes the branch back to the saved PC unconditional |
| `0x09` WAIT with operand `0x0FFF` | 4095 frames is over a minute, which no cast or summon part is on screen for - the actor is gone before the counter runs out |

### The maximal WAIT is a fallback, not a terminator

The third row is weaker than the other two and is treated as such. `WAIT`
retires like any other instruction once its counter expires, so nothing in the
VM stops there; the evidence is the band's layout, and the band emits the
maximal operand mid-program as well as at the end. Ending the walk at the
*first* one costs 48 of the extents the HALT rule reproduces exactly.

So the walk remembers the last maximal `WAIT` it stepped over and returns it
only where it would otherwise have no bound at all - where it runs off the
buffer or meets a halfword that is no opcode. That ordering cannot lose a HALT
or an armed loop, because those return immediately.

What it does is settle the four images the band used to leave open. Their
highest records are byte-identical in two cases (PROT 0927 and 0966 carry the
same record), all four open `model_sel = -1`, and in all four the last
cleanly-decoded instruction is `0x09 0x0FFF`; above it sits either a word that
is no opcode or a zero run that does not tile at any opcode width. All four sit
**below** the image's measured inherited tail, so they are the image's own
bytes and not a donor's.

Four details decide whether this reproduces the measured extents or misses
them, and all four were needed:

1. **Round the end up to 4.** The records are word-aligned, so a program whose
   last halfword lands mid-word is followed by one halfword of padding. Without
   this step the walk misses by exactly 4 bytes on a large minority of records -
   which is the "within four bytes for about three quarters" figure an earlier
   reading of this page recorded as evidence that no static bound exists. The
   miss was the alignment rule, not the absence of one.
2. **HALT outranks the idle loop.** Most records that idle-loop still emit the
   record's `HALT` in the very next halfword, and the record ends after the
   `HALT`. So an armed loop ends the program only when a `HALT` does not follow
   it immediately.
3. **Chain, don't assume one record per pointer.** Above the highest credited
   record the bytes often keep reading as `[header][program]`, and those are
   records the consumer reaches by some other route. The parser keeps them in
   `chained_records`, apart from the pointer-credited ones.
4. **Fall back to the last maximal WAIT.** The section above: where the walk
   dies with no terminator, the last `0x09 0x0FFF` it stepped over bounds the
   record.

Under those four, chaining `[header][program]` from every record start
reproduces 1021 of the band's 1027 bounded record extents exactly (the band's
own highest records are excluded from that count - their ends come from this
very walk), no chain overruns a measured end, and every one of the 62 images
that has a highest record bounds it. The six that miss stall below the measured
end and are a stated residue, not a rounding tolerance.

The residue has exactly one direction, and the direction is a property of the
**claim**, not of the walk's program counter - the two are easy to confuse.
What holds without exception is that no miss ever *terminates* above the
measured end, so the rule never claims bytes the band does not bound; it
declines to claim bytes the band does.

The walk's PC is a different matter, and it does run past the end. Re-measured
over the band's pointer-credited extents, the misses split three ways: the PC
lands **exactly on** the measured end on the largest group - the record whose
last instruction is non-terminating, which is the shape the rule was written
for - stops **below** it on a handful, and steps **past** it on fourteen,
before dying on a halfword that is no opcode. On those fourteen the widths
mis-step somewhere inside the record, so by the time the walk fails it is
reading the next record's bytes as operands. A page that says "every miss
stalls below the end" is describing the first two groups only.

That asymmetry is what makes the figure usable by the two consumers below. An
over-claim would retire real code from the dump worklist; a stall only leaves
an extent for the pointer bound that already exists.

## What this is for

Two instruments consume the claims.

- `asset account <entry>` credits the head table as `toc` and each record as
  `record`, so a band entry's residue report shows what is left rather than
  reporting the module's whole data half as unwalked
  ([byte-accounting.md](../tooling/byte-accounting.md#the-slot-b-images)).
- `scripts/ci/disc-coverage.py` carries the records as the `spawn_record_band`
  shape and takes them out of the **code** denominator
  ([disc-coverage.md](../tooling/disc-coverage.md#spawn_record_band-a-shape-a-parser-names)).

The second one matters because the band's residue read as un-dumped code, and
the reason it did is worth stating: `in_data_segment`'s second leg rejects a run
holding a `lui $rt, 0x80xx`, and a record's move-VM bytecode is arbitrary bytes,
so a long enough record band contains that word by accident. The statistic then
scores the band as code, and several images' whole record bands were ranked as
dump work. Eleven of the band's ranked dump-worklist rows were the record band
in whole or in part, and four of those were nothing else.
