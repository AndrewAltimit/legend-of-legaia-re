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

The wider docs quote this record as `[i16 model_sel][u16 flags][bytecode]`, and
the `flags` half of that name is not a measurement. Nothing reads `+0x02`:
`FUN_80021B04` loads only `($a2)` (`lh` at `0x80021B2C`, `lhu` at
`0x80021B30`, and again at `0x80021BD0` / `0x80021C98` / `0x80021CB4`), and the
move VM starts the program at halfword index 2 (`actor[+0x70] = 2`, `sll v0,1`
at `0x800230B8`), so `+0x02` is stepped over rather than fetched. It is zero in
every band record and in every PROT 0898 effect-prototype record. Confidence on
the halfword is therefore **Unknown**, and this page names it `reserved`; the
other pages still carry the older `flags` label.

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

No claimed byte comes from one today, and the reason is the *other* rule: in
five of the six (0908, 0910, 0920, 0943, 0961) the donor's pointer is the
image's **highest** offset, so the unbounded-record rule below drops it, and in
PROT 0945 the pointer does not resolve at all. `asset account` reports the
dropped offset for each - `0x26D8`, `0x26D8`, `0x1EF4`, `0x17E0`, `0x1DAC`. So
the band's record bytes are sound while the call-site filter is narrower than
its own description; a donor whose residue framed cleanly *and* whose record
sat below one of this image's own would be credited here.

### The one span the band cannot bound

The image's **highest** record has no next pointer above it. Nothing in the
module computes an address past it, the record carries no length, and the
move-VM program's own terminator (`0x08` HALT) is not a reliable static bound -
walking the documented opcode widths from a known record start lands on the
next record's start, within four bytes, for about three quarters of the band's
records and misses on the rest. So the parser stops at the highest record and
reports it rather than claiming it. Everything below it is bounded on both
sides.

## What this is for

Two instruments consume the claims.

- `asset account <entry>` credits the head table as `toc` and each record as
  `record`, so a band entry's residue report shows what is left rather than
  reporting the module's whole data half as unwalked
  ([byte-accounting.md](../tooling/byte-accounting.md#the-slot-b-module-band)).
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
