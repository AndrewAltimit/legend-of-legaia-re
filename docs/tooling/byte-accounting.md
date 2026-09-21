# Byte accounting inside a PROT entry

[`disc-coverage.md`](disc-coverage.md) measures **format recognition**: for every PROT entry, which
named format class the bytes fall into, weighted by size. That page says in its own output what the
number does not cover - no parser reports consumed-versus-unconsumed bytes, so the data figure is an
upper bound. Knowing entry `0867` is a monster archive and `0893` / `0894` are battle side-band
streaming files says nothing about which bytes *inside* them are understood.

Byte accounting is the sibling measurement, one entry at a time. It runs every parser this workspace
already has that applies to the entry's class, collects the byte ranges those parsers consume, and
reports the complement - classified by shape, largest run first. The result is a worklist: a large
run of bytes that no parser claims and that does not look like padding is a format nobody has walked.

Module `legaia_asset::byte_account`; CLI `asset account`.

## Running it

```bash
asset account extracted/PROT/0867_battle_data.BIN
asset account 893 --prot-dir extracted/PROT          # bare extraction index
asset account 898 --funcs ghidra/scripts/funcs       # an overlay code image
asset account 0086 --depth 2 --json > kingdom.json
```

| Flag | Effect |
|---|---|
| `--prot-dir` | Where a bare extraction index is resolved (default `extracted/PROT`). |
| `--prot-index` | Override the index the filename implies. Selects the two index-keyed walkers. |
| `--funcs` | Ghidra dump directory. Required for an overlay code image; ignored otherwise. |
| `--depth` | How many levels of decoded payload to account. `0` accounts only the outer buffer. |
| `--no-rescan` | Skip the magic sweep over the residue. |
| `--min-residue` | Shortest residue run to list individually (all runs are still counted by shape). |
| `--claims` | Keep the largest individual claims in the report. |
| `--json` | Emit the whole `Account` structure instead of the text report. |

## Method

1. Classify the buffer with `legaia_asset::categorize`, then pick a walker. Two selections are keyed
   on the extraction index instead of the class: the monster archive (whose head is a `dec_size`
   word, so no detector fires on it) and any entry with a row in
   [`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) plus a dump directory.
2. Run the walker. It emits **claims** - half-open `[start, end)` ranges, each tagged with an owner
   from the vocabulary below plus a free-form detail naming the instance.
3. Merge the claims (they overlap and arrive out of order) and take the complement. Each maximal
   uncovered run is one **residue**, classified by shape.
4. Where a claim covers a compressed span, the walker decodes it and accounts the payload in a
   nested pass. Both figures are reported: an LZS stream is 100 % accounted on the outer pass by
   construction, and the number worth reading is what the decoded side accounts to.

### What the number means

`accounted` is the share of the entry's bytes that some parser in this workspace consumes. It is not
the share whose meaning is understood. A fixed-region file such as a
[field map](../formats/field-map.md) accounts to 100 % the moment its four region constants are
written down, while the per-field semantics of three of those regions are only partly pinned. Read
the figure as an upper bound on understanding and a lower bound on structure.

Claims are tiered so the figure cannot be inflated by guessing. A claim whose owner is `scan` came
from a magic sweep over the residue rather than from following the container's own layout - evidence
that a sub-asset is *there*, not that anything walked to it. The report prints `structural`
(excluding `scan`) beside `accounted` (including it).

## Owner vocabulary

An owner says what kind of thing consumes the bytes, not which module claimed them; the per-claim
`detail` names the instance. `legaia_asset::byte_account::OWNERS` carries the same list in code.

| Owner | Bytes it names |
|---|---|
| `header` | A container's own fixed head words: magic, counts, totals, a chunk header. |
| `toc` | An offset / descriptor / index table the container's reader walks. |
| `lzs` | A compressed span. The decoded side is accounted in a nested pass. |
| `tim` | A PSX TIM: header, CLUT block, pixel block. |
| `tmd` | A Legaia TMD mesh. |
| `vab` | A Sony VAB instrument bank - header, program and tone tables, VAG bodies. |
| `seq` | A PsyQ SEQ sequence, header through the end-of-track meta event. |
| `anm` | An animation record or keyframe stream. |
| `record` | A fixed-stride data record: stat record, action entry, trigger row. |
| `grid` | A dense per-tile grid: collision, floor height, object index. |
| `script` | A bytecode body: field-VM, move-VM, event prescript. |
| `code` | MIPS instructions inside a dumped function's extent. |
| `texture` | A raw texture page - indices with no TIM header. |
| `clut` | A palette region. |
| `string` | A NUL-terminated string a parser resolves a pointer to. |
| `pad` | Declared slack inside a fixed-stride slot that the container's own size math covers, and dev fill a slot is entirely made of. |
| `inherited_tail` | Another mapped image's bytes, at the same file offset - the run from where this overlay stops being its own content. |
| `scan` | Found by a magic sweep over the residue, not by a structural walk. |

### The `lzs_container` class fits a count it never reads

The class name says descriptor bundle and the class membership does not mean
that. Its detector never consults the header's `count` word: it tries a fixed
list of descriptor counts and accepts the first that passes the per-descriptor
checks, so a buffer whose leading words merely *look* like descriptors joins
the class with a count nothing on the disc states.

Two consequences, both measured over the 18 retail members.

- **The reported count was wrong on eleven of them.** The fitted list's
  per-descriptor floor of 32 bytes rejects the 4-byte ANM slot that the
  count-4 bundles carry, so every one of them fitted at `n = 1` - validating
  one descriptor is vacuously easy. Reading the count word first, and
  validating it the way `FUN_80020224` walks it, reports 4 / 5 / 3 / 1 as the
  headers state. Nothing on the disc changes class either way.
- **Three members are not bundles at all.** `0485` is an offset pack behind a
  DATA_FIELD chunk header, `0872` is a bare offset pack, and `0981` is a code
  image that opens with a string pool. The first two are recovered from the
  offset-pack anchor `offsets[0] == 4 + 4 * count`, which is a layout law
  rather than a guess; the third has no structural walker and stays residue.

So read `lzs_container` as "nothing earlier claimed it and the first words fit
a descriptor shape". The walker behind it, `descriptor_bundle`, is the one that
transcribes the runtime walk - `count` off `+0x00`, descriptor `i` at
`+0x08 + 8i`, payload at `base + data_offset`, no count bound - and every
payload extent it claims is **measured** by what the LZS decoder consumed, not
inferred from the next descriptor's offset.

### Walking a bundle's sections

A [scene bundle](../formats/scene-bundles.md) is accounted twice: the outer pass
claims each descriptor's LZS span, and a nested pass accounts the decoded
payload with the walker its **type byte** selects. Three of those types had no
walker, so a town bundle's decoded MOVE / VDF / MES sections read 0.0 % and the
shortfall looked like unwalked format. It was not - all three are containers
this workspace already parses:

| Type | Section | Walker | What the claims are |
|---|---|---|---|
| `0x04` | MES | `mes` | The compact form's magic + fixed header region, then the dialog bytecode; the records form's per-record extents. Several retail bundles carry a 40-byte *empty* compact MES - the magic and 36 zero bytes. |
| `0x05` | MOVE | `clip_bank` | Per clip: the 8-byte header, `bone_count * frame_count` 8-byte transforms, and the 8-byte record trailer. Despite the dispatcher's "MOVE" label the content is an ANM clip bank ([`anm.md`](../formats/anm.md)), **not** a Tactical-Arts [move table](../formats/mdt.md). |
| `0x07` | VDF | `offset_pack` | `[u32 count][u32 byte_offset[count]]` with **absolute** byte offsets - the same container as the clip bank, without the `0x080C` record header. |

The clip-bank walker claims a clip's three parts separately rather than its
whole offset-table extent, so a record that does not satisfy
`size == 16 + 8 * bones * frames` leaves residue instead of being absorbed. The
`anm` walker falls back to `offset_pack` for the same reason: a kingdom bundle's
type-`0x06` section is that container with records `legaia_anm::parse` declines,
and the member extents are still real.

Do not read `offset_pack` as [`pack`](../formats/pack.md). The offsets differ in
*units*: `pack`'s are word indices from the pack's own base, `offset_pack`'s are
byte offsets. The anchor `offsets[0] == 4 + 4 * count` tells them apart - a
word-offset table would put member 0 four times further on.

A `TIM_LIST` or `TMD` section is likewise not always a single asset: several are
a [pack](../formats/pack.md) of them, and a pack's head word is a count carrying
no magic. Choosing the walker from the type byte alone therefore sent those
payloads to `generic`, where the magic sweep still found the members - so the
report read `accounted` near 100 % beside `structural` 0.0 %, which is exactly
the "found by guessing" split the tiering exists to expose. The section walker
now tests for the pack anchor first, and those payloads walk structurally.

## Residue shapes

Each uncovered run gets exactly one shape. The tests run in the order below and the order is
load-bearing.

| Shape | Test |
|---|---|
| `zero_pad` | Every byte is `0x00`. Sector or record padding. |
| `alignment` | Shorter than 16 bytes and not all zero. Inter-record alignment, not a finding. |
| `repeated_fill` | The run is one short pattern (period 1, 2, 4, 8 or 16) repeated. Dev fill. |
| `ascii_text` | At least 80 % printable ASCII **or NUL**. A string pool - or a mostly-empty region with strings in it; see the note below. |
| `pointer_dense` | At least 20 % of words land in `0x80000000..0x80200000`. A pointer or jump table. |
| `bgr555` | Nearly every halfword is below `0x8000`, over a wide high-entropy spread. PSX 15-bit colour with the STP bit clear: a CLUT block, a 16bpp page, raw VRAM. |
| `plausible_mips` | Plausible primary opcodes, spread over at least four of them, almost no pointers, and no SPECIAL landslide. Un-dumped code. |
| `low_entropy` | Below 4.0 bits/byte. Tabular data, sparse vectors, geometry. |
| `high_entropy` | At or above 7.2 bits/byte. Already compressed, or sample data. |
| `mixed` | None of the above. |

Three ordering traps are built into that sequence, and each has a one-sided failure mode that a
naive classifier walks straight into.

- **A pointer table decodes as code.** A word like `0x801C1234` has primary opcode `0x20`, a
  plausible `lb`, so a table of overlay pointers passes an opcode-plausibility test with a perfect
  score. `pointer_dense` is therefore tested first, the same order
  [`disc-coverage.py`](../../scripts/ci/disc-coverage.py) uses for code gaps.
- **A small-value table decodes as code.** A sparse 16-bit table has primary opcode `0` (SPECIAL) in
  every word, which is also plausible. Opcode plausibility alone therefore reports every such table
  as un-dumped code. `plausible_mips` additionally requires the run to spread across at least four
  distinct primary opcodes and to keep SPECIAL below 60 % of its words - real code mixes loads,
  stores, immediates and branches; a table does not.
- **A colour page decodes as code too.** A BGR555 halfword pair forms a word whose primary opcode is
  in the low, plausible range, and colour data spreads across enough of them to clear the
  distinct-opcode floor as well. `bgr555` is therefore tested first, and it is discriminating in the
  other direction: real MIPS puts a halfword at or above `0x8000` in every load, store and
  `lui`-pair word, so code never passes the STP-clear test.

A run's **boundaries** are drawn by the claims around it, not by its content, so
one run can be two findings glued together - and the vocabulary then has to name
both with one word. A kilobyte of unwalked content followed by a hundred
kilobytes of fill arrives as a single run and reads as `ascii_text`, because the
NUL asymmetry below takes it and `zero_pad` will not. A residue run is therefore
**cut** wherever a sector or more of fill sits inside it. Each piece still earns
its own shape and the residue total does not move; only the reporting changes,
and the fill stops being counted as work. Entry `0970` is the case: 3924 bytes
of unwalked content and a 131172-byte hole came out as one 135096-byte
`ascii_text` run at 0.23 bits/byte.

There is a fourth asymmetry, and it inflates `work_bytes` rather than misnaming
a run. `ascii_text` counts NUL as printable, because a string pool is mostly
short strings separated by terminators - but so is a mostly-empty region with a
handful of strings in it, and `zero_pad` will not take it because a single
non-zero byte disqualifies the run. The gap between the two tests is real
residue that is almost all padding: of the overlay entry `0899`'s `ascii_text`
residue, the runs that are at least 90 % NUL are the large majority by bytes,
and the one dense string pool is a small minority. So read an `ascii_text` run
by its zero fraction (`asset account --json` reports it per run) before treating
it as a format nobody walked - and do not "fix" it by widening `zero_pad`, which
would move bytes out of `work_bytes` by redefining the instrument rather than by
understanding them.

## Overlay code images

For an entry that is a runtime overlay, the "parser" is the Ghidra dump corpus: a dumped function's
`(entry, size)` header states an extent, and that extent maps to file offsets through the overlay's
load base in [`static-overlays.toml`](../../crates/asset/data/static-overlays.toml). The header
parse mirrors [`dump_header.py`](../../scripts/ghidra-analysis/dump_header.py) and is re-implemented
in Rust, so the instrument needs a checkout plus a dump directory and never invokes Ghidra.

The hard part is not the header. Every slot-A overlay loads at `0x801CE818`, so a printed VA cannot
say which image a dump belongs to - the ambiguity
[`call-target-integrity.md`](call-target-integrity.md) and
[`dump-corpus-integrity.md`](dump-corpus-integrity.md) both circle. This instrument resolves it from
the bytes: it re-encodes the dump's first printed instructions to their 32-bit words and compares
them against the image at the mapped offset.

| Verdict | Meaning | Effect |
|---|---|---|
| confirmed | A printed instruction re-encodes to the image's word at that offset. | Credited. |
| refuted | A printed instruction re-encodes to a *different* word. | Dropped - the bytes belong to an aliased sibling. |
| unverifiable | No printed instruction is in the encodable grammar. | Credited only if the dump's filename label names this entry; otherwise reported as ambiguous and not counted. |

The encodable grammar is small on purpose - `nop`, `jr ra`, `addiu`, `li`, `lui`, `move`, `j`,
`jal`, and the load/store forms - because a function's first instructions are nearly always drawn
from it. A mnemonic outside the grammar yields *unverifiable*, never a mismatch: the instrument's
silence must never read as a refutation.

#### A zero match is not a match

`nop` is in that grammar and it encodes to `0x00000000`, so a dump whose head is
`nop` **agrees with zero fill** - in any image, at any base. That is not a
property of the disc; it is the one encoding that carries no information.

The corpus contains such dumps, taken over an image's own zero region rather
than over a function. One of them, `FUN_801d84b4` at 20060 bytes, confirmed
against entry `0970` - whose content is a 12676-byte head, a **131172-byte**
all-zero hole, and a 7413-byte tail. The extent lands wholly inside that hole,
and it carried most of the entry's reported code share: `0970` read 20.5 %
accounted where its own non-zero bytes are 12555 of 149504, and reads 5.6 %
once zero agreement stops counting.

Two rules close it, and the entry's note reports the second: a confirmation
needs one printed instruction that re-encodes to a **non-zero** word the image
carries, and an extent whose bytes are entirely zero is never claimed as
`code` - not even when the dump's filename names this entry, because the
filename says where the dump was taken and not what is there. A *mismatch*
still refutes whatever the word: a difference is informative where an agreement
with fill is not.

The same agreement reaches the committed byte-attribution CSV
([`disc-coverage.md`](disc-coverage.md)), which places that extent in
`baka_fighter(0976)` on a 24-instruction window - an image whose own extent
ends before the run does.

A code image's non-code regions - string pools, jump tables, data segments - fall to the residue
classifier, which is the right answer for them. `ascii_text` and `pointer_dense` runs in an overlay
entry are the segments the dump corpus is not about.

### An entry can be an overlay *and* a container

The overlay selection above is an override: with a dump directory and a row in
the map, the code walker replaces whatever walker the entry's class would have
chosen. That is right for an image that is only code, and wrong for one that is
also a container - the class walker's claims simply vanish, and precisely when a
sweep runs, because a sweep is when `--funcs` is given.

`init.pak` (PROT `0895`) is the case on this disc. It is a boot overlay with a
map row *and* the four-TIM publisher-logo pack `legaia_asset::init_pak` already
parses at fixed offsets. Under the override its logo claims were dropped and
its TIM pixel data - over half the entry - read as unwalked `ascii_text`, which
is the shape indexed pixel data takes (see the NUL note above). The walker now
owns the entry and calls the code walker itself, the way the slot-B module
walker does, and the entry accounts nearly whole.

Two smaller corrections came out of the same read. The pack has **four** logo
TIMs, not five: the fifth offset that had been recorded is a `10 00 00 00` word
inside the fourth logo's pixel block, which is the magic-sweep false positive
this tiering exists to keep visible. And the residue that is left is the head
pointer table, the executable-name string, and the tail past the last logo.

### A pinned offset is not a magic sweep

The dump corpus says nothing about a code image's data segment, so a sub-asset
that lives there falls to the magic sweep - which finds it, tags the claim
`scan`, and thereby reports "found by guessing" for something a module in this
workspace already reads at a named constant.

The menu overlay (PROT `0899`) carried both of the disc's two largest remaining
sweep-found claims, and both are pinned: the save-menu UI atlas at
`legaia_asset::title_pak::OVERLAY_SAVE_MENU_TIM_OFFSET` and the
[save-slot icon sheet](../formats/save-icon.md) at
`legaia_asset::save_icon::PROT_ENTRY_OFFSET`. The overlay walker now claims
both from those constants, with the extent taken from each TIM's own header
rather than from a table here, so the entry's `scan` share goes to zero and its
structural share carries the two atlases.

The general rule this instrument wants: when the sweep finds something, check
whether a constant for it already exists. A `scan` claim beside a named offset
is a missing binding, not a discovery.

#### The same gap, without a magic to find it

A sub-asset at a pinned offset at least announces itself to the sweep. A
**table** does not: a stride of small integers carries no magic, so it never
becomes a `scan` claim and never reads as "found by guessing" - it reads as
residue, and ranks in the worklist beside a format nobody has opened. Six of
the disc's larger `low_entropy` / `mixed` runs were that: the move-power and
attack-camera tables, the element-affinity matrix, the menu window
descriptors, the Baka Fighter roster, the slot-machine and dance tables. Each
already had a parser with a `pub const` offset and a decoded record layout.

`byte_account::pinned_overlay_tables` is the binding, one row per table,
carrying the offset and the `count * stride` **from the owning module's own
constants** - so a row cannot be widened here without widening the parser that
reads it, and re-pinning a table in the parser moves the claim with it. The
rows are asserted against the disc: every row is in bounds, the rows are
disjoint from each other, and no row overlaps a dump extent the bytes confirm
in that image. That last one is the real guard, because an overlap is
invisible in the accounted total - the sink merges ranges before reporting,
and a merged range keeps no owner, so a table wrongly placed inside a function
would silently agree with the number.

The last instance on the disc was entry `1062`, and it was the same shape: a
single-chunk DATA_FIELD stream carrying a SEQ - one `(0x02 << 24) | len` header,
the sequence, a zero terminator, sector padding - classed `overlay_data_blob`
and walked as `generic`, so its sequence was found by magic. The `generic`
fallback now tests the shape instead: a buffer whose own bytes walk to a
stream terminator, every chunk payload in bounds and at least one chunk
consumed, goes to the stream walker. `1062` is the only entry that reaches that
arm - the rest of the `generic` population is all-zero filler plus the un-based
`0896` - and with it the disc's `scan` share is zero.

The chunk's type byte is `0x02`, which the asset dispatcher reads as TMD while
the payload's magic is `pQES`. The owner comes from the **magic** where the two
disagree: a type byte selects the runtime's handler, and an owner names what the
bytes are.

### A base is not a dump, and a dump is not a caller

PROT `0896` produced two wrong readings in a row here, one about its base and
one about what a base buys, and the second outlived the first.

"No load base makes the image self-consistent" was a reading of one metric - a
ratio over the image's `lui`+`addiu` pairs, which is blind to the call graph and
which ranks a refuted base first on this image
([`static-overlay-pipeline.md`](static-overlay-pipeline.md#a-resolution-ratio-is-not-a-base-test)).
The call-graph recovery lands on `0x801D4DF0`, and the entry has a map row.

What this page then said was that the residue does not move: a base answers
*where the bytes go* and says nothing about whether anything has read them, so
the whole entry would stay `plausible_mips` residue. The first half is true and
the conclusion was not. The parser for a code image's code is the **dump
corpus**, and a base is exactly what lets the image be imported and dumped -
which is a separate step from recovering the base, and the step nobody had
taken. Imported at `0x801D4DF0` the image's whole code region dumps, and the
entry's accounted share rises accordingly; what remains is its data segment and
byte-curve tail, not its code.

The part of the old reading that survives is about *callers*, not about
coverage: this image's code is linked against an executable this disc does not
carry - none of its SCUS-range calls lands on a `SCUS_942.54` function entry -
so no capture can show it resident and no port is owed for any of it. A dump
credits bytes; it does not make them reachable.

### A pointer table is not its pool

`pinned_overlay_tables` binds a table by `offset + count * stride`, and a table
of **pointers** then reads as fully accounted while everything it points at
stays residue. The battle overlay's effect-prototype table `0x801F6324` was that
case: its sixty-one `u32`s were claimed and the fifty-four unique records behind
them - three and a half kilobytes, packed, ending at the table itself - ranked as
one unbroken `low_entropy` run, which is what a completely decoded structure
looks like when only its index is claimed.
[`move_power::parse_effect_proto_records`](../formats/move-power.md#effect-prototype-records---the-spawn-path)
had been decoding them the whole time.

Two properties of the pool make the claim exact rather than a guess about
lengths: the records are packed, so each one ends where the next begins, and the
last is bounded by the table rather than by the end of the entry - which is the
one place the shared record walker's generic bound is too generous for a byte
claim.

The sibling shape is a **NUL-terminated string**, where the length is in the
bytes rather than in any table. The battle overlay's UI labels and the Muscle
Dome victory messages each have a pinned start and no stride at all, so the
walker NUL-scans from the pinned offset and claims nothing when there is no
terminator in the image. Same rule as the tables: the start is a `pub const` of
the module that reads it, and only the extent comes from the disc.

#### A filename is not corroboration on its own

Committing that row surfaced a second gap, in the walker rather than on the
disc. An extent whose printed instructions are outside the encodable grammar is
*unverifiable*, and the walker credits it when the dump's filename names this
entry. That is sound where other extents in the same image confirm by bytes -
the filename is then one claim among corroborated ones. Where **nothing** in an
image confirms, the filename is the whole of the evidence, and that is exactly
the case in which the dump program's base was wrong: the extents then land at
arbitrary offsets in a file that never held them. Three such extents landed
inside `0896` and were credited as code; re-disassembling the file at those
offsets shows different instructions.

So a label-credited extent needs at least one byte confirmation elsewhere in
the same image. The rule costs nothing where the corpus is real - the field,
battle, menu and minigame images all confirm hundreds of extents by bytes - and
it is reported rather than silently dropped: the entry's note says how many
label-matching extents were left uncredited and why.

### The multi-bank VAB

PROT `0891` is selected on its class, and every claim it makes comes out of a
length the container states: the `count + 1` sector bounds its reader indexes,
then per bank the two DATA_FIELD chunk headers, the header part, the VAG bodies,
the stream terminator and the sector slack. `scan_bytes` is zero across the
entry. The `pBAV` magic gates the class and is never a claim boundary - the
distinction matters here because the previous account of this entry *was* the
magic sweep, at 96 % accounted and 0 % structural. Walker `vab_multi_bank`;
parser [`legaia_asset::vab_multi_bank`](../formats/vab.md#the-multi-bank-archive-monstersnd).

The sector slack is claimed as `pad` rather than left as residue, and the reason
is the same one the filler slots use: the bank's extent is declared (by the next
index entry) and its content length is declared (by `fsize`), so nothing reads
between them. It is not zero fill - the builder left its previous sector
buffer's contents there, and two banks carry a third bank's bytes at the same
buffer offset.

### A fixed-stride streaming slot is transferred whole

Three entries are a flat array of fixed-size slots, and each one's residue used
to be the largest of its class: the monster archive (`0867`, `0x14000` per
slot - four fifths of the whole disc's residue on its own) and the two battle
side-band streaming files (`0893` / `0894`, `0x10800` per slot). In all three
the unclaimed bytes were zero, in one run per slot, ending exactly on a slot
boundary.

They are declared slack, and the evidence is the consumer's transfer length
rather than the shape of the bytes. Both loaders seek by a stride and then read
a **literal** span: `li a1,0x28` (40 sectors) at `0x80054608` in
`FUN_800542C8` for the archive, `0x10800` bytes at `0x801F1958`/`0x801F1970` in
`FUN_801F17F8` for the streaming files. Only afterwards does either hand the
slot's head to a reader that stops at the content's own end - an LZS terminator,
a declared texture width. So the tail is transferred and never interpreted, and
each file's extent is an exact multiple of its stride, which makes the slot
boundary a bound the container states.

The claims are therefore `pad`, with the boundary as their end. Two guard rails
keep that from being a way to buy percentage points, and both are asserted by
`crates/asset/tests/byte_account_entries.rs` against the raw file rather than
against the parser: a claim covers only the slot's maximal all-zero **suffix**,
so a walker that stopped early inside live content leaves that content as
residue; and every claim ends on a multiple of the stride. The same rule, one
sector wide, covers what is left of a PROT entry's last sector past a stream
terminator - past one sector it is a second region, not slack, and stays
residue.

Read the resulting figure with the tiering in mind, because the two rules move
different numbers and one of them moves the worklist.

The slot fill moves only the *structural* share: its bytes were already
`zero_pad`, which `work_bytes` never counted. The disc's largest remaining
unwalked region turns out to have been the disc's own padding, and the
instrument now says so instead of ranking it.

The last-sector rule does move `work_bytes`, and by a lot - the three streaming
classes' non-slack residue goes to zero. Those bytes were **not** zero: they are
the builder's sector buffer, the same thing the multi-bank VAB's per-bank slack
is, and they had been ranking as the largest `mixed` / `low_entropy` figure on
the disc under the verdict "walker tails". They were not walker tails; every one
of them sat past a terminator the walk reached. That is a claim about where they
sit, not about what shape they take, which is the distinction the `ascii_text`
note below insists on - widening a *shape* test to absorb residue redefines the
instrument, while bracketing a run between two declared bounds reads the
container. Anything a sector or more past the terminator stays residue, so the
rule cannot swallow a region.

### An overlay's uninitialised data region travels with its code

The same fixed-length argument settles the largest zero runs in the overlay
images, and it is the loader itself that makes it: `FUN_8003EBE4` asks
`FUN_8003E8A8` for the entry's sector count - `toc[i+3] - toc[i+2]`, the gap to
the next entry, which is `FUN_8003E68C`'s own expression - and hands it straight
to `FUN_8003E800`. The transfer length is therefore the whole PROT extent, and a
linked image's **uninitialised data region** rides into RAM with its code as
zero fill. Those bytes are not a format nobody has walked; they are the buffers
the image writes at runtime.

Shape cannot establish that, because zero fill looks the same whoever wrote it.
The claim rests on the image's own code instead, and the walker
(`claim_uninitialised_data`) applies two rules:

- the claim is exactly one maximal **all-zero** run of at least 256 bytes, so it
  can never grow into live content and a single non-zero byte splits it in two;
- the image's own code must address the run, and once is not enough. A lone
  `lui` pair landing somewhere in a multi-kilobyte window is a coincidence an
  image with thousands of pairs produces; two distinct addresses, or one address
  formed at four separate sites, is a structure. Each claim's `detail` carries
  both counts.

The second rule is what keeps a donor's zero tail and a zero hole inside a
sparse data segment out of the figure. Three runs in the worked entry below are
refused by it, and so is the menu overlay's largest data-segment hole - one
address, one site.

The pair scan walks **forward** from each `lui` for sixteen instructions and
abandons the window the moment something redefines the register. Forward matters:
the STR overlay hands its VLC unpacker the destination in a `jal` **delay slot**
(`801cf214 jal 0x801f1a00` / `801cf218 _addiu a0,a0,0xa00`), which a backward-only
scan from the second instruction never sees.

Like the slot fill above, this moves only the *structural* share. The bytes were
already `zero_pad`, which `work_bytes` never counted; what changes is that the
instrument stops ranking the disc's own `.bss` as the largest unwalked region on
it.

#### The STR overlay's hole, region by region

PROT `0970` carried the disc's largest single unclaimed run, 131172 bytes of
zeros between the overlay's initialised data and the unpacker at the top of the
image. Every boundary inside it is an address the overlay's own code forms, and
the six regions sum to the run exactly:

| VA | bytes | what addresses it |
|---|---:|---|
| `0x801D199C` | 4 | alignment below the descriptor |
| `0x801D19A0` | `0x50` | the play loop's decode context: `801cf10c addiu a0,v0,0x19a0` is the argument to the ring/rect init `FUN_801CF8B0`, and every play-loop helper takes the same pointer - this is the `ctx` whose fields [`cutscene.md`](../subsystems/cutscene.md#play-loop---fun_801cf098-overlay) tabulates |
| `0x801D19F0` | `0x7800` | slice staging buffer 0, stored to `ctx+0x0C` by `801cf904 addiu v0,v0,0x19f0` |
| `0x801D91F0` | `0x7800` | slice staging buffer 1, stored to `ctx+0x10` by `801cf910 addiu v0,v0,-0x6e10`; the pair ping-pongs on `ctx+0x14` (`801cf344 lw a0,0xc(a2)`) |
| `0x801E09F0` | `0x10` | four overlay globals, among them the demuxer's end-frame latch `DAT_801E09F8` and the decoder selector `DAT_801E09FC` |
| `0x801E0A00` | `0x11000` | the [STRv2 VLC lookup table](../subsystems/cutscene.md#strv2-vlc-lookup-table-fun_801f1a00) destination, ending flush against the unpacker `FUN_801F1A00` |

Two other zero runs in the same entry are **refused**: the 1827-byte tail past
the compressed blob (claimed instead as last-sector slack) and a 256-byte hole
inside the data segment. Neither carries a formed address, which is the answer
the rule is supposed to give.

### A compressed table whose extent nothing else states

The rest of `0970`'s residue was its second-largest run, 3597 `mixed` bytes at
file `0x232D0` - and `mixed` is what a compressed stream looks like to a shape
test. It is the source the VLC table above is unpacked from, which
[`legaia_mdec::strv2_table`](../../crates/mdec/src/strv2_table.rs) already
walks; the accounting measures the extent with that walk rather than
re-implementing it, so `unpack_lz_tracked` reports what the control-byte walk
**consumed** the way `decompress_tracked` does for an LZS span.

Measuring it is the only option here. The blob sits at the top of the image with
nothing after it but the entry's last-sector slack, so no next-offset bounds it,
and its own stream carries no length - only the `0xFF 0xFF` terminator. What the
bytes do state twice over is the *output*: the unpacker's `ori a2, zero, 0x87ff`
bound says `0x8800` halfwords, the retail blob decodes to exactly that many, and
the destination plus that length lands on the unpacker's own entry.

### A dev module's roster is one stride, not one string pool

PROT `0974`, the `OTHER3` dev module, read as 10904 bytes of `ascii_text` in two
runs - which invites the verdict "a text blob nobody claims" and is wrong about
the shape. Three quarters of the entry is a fixed-stride table, and the stride
is in the drawing loop's index arithmetic rather than in the bytes: `(i << 5) + i`
then `<< 2` is `i * 0x84`, the base comes from `801cee00 addiu s3,v0,-0x10c0`,
and the reciprocal divide at `801cee2c`..`801cee54` wraps the cursor `mod 81`,
which is the record count. Ten rows are drawn per page. The loop is inside
`FUN_801CED68`
(`see ghidra/scripts/funcs/overlay_other3_dev_0974_801ced68.txt`). Parser
[`legaia_asset::other3_roster`](../../crates/asset/src/other3_roster.rs);
`claim_other3_roster` claims each record at the stride, padding included, because
the stride is what the loop advances by.

The labels are Japanese, stored as little-endian `u16` Shift-JIS code units
(four lead bytes in use: `0x81` / `0x82` fullwidth, `0x83` katakana, `0x88` /
`0x8F` kanji), and that is **not** evidence of a foreign build: 34 of `0974`'s 37 distinct
SCUS-range `jal` targets land on a `SCUS_942.54` function head, where PROT
`0896` - the image that really is from another build - scores 0 of 42. The dev
modules were simply never localised.

### What the overlay residue that is left actually is

With the zero regions and the two data-segment structures above claimed, what
remains in the overlay entries is a short list, and none of it is an unopened
format. Each row is measured from the image's own bytes; none is claimed,
because a claim needs a parser or a table with a named constant behind it and
these have neither yet.

| Entry | Run | What it is | Why it is not claimed |
|---|---|---|---|
| `0897` | `0x2399C`, 4988 B | the field overlay's data segment; its first twelve rows are the collision probe table `FUN_801CFE4C` indexes at `0x801F21B4` | no parser binds the table, and the rest of the segment is unsorted globals |
| `0897` | `0x0`, 3900 B | head string pool plus the pointer table that reaches it | the pointers are not read by anything here |
| `0898` | `0x0`, 3512 B | the battle overlay's own head: UI strings (eleven of which `battle_ui_strings` already claims by pointer) and **nine** runs of in-image VA words - `switch` jump tables, not one ([below](#the-0898-head-is-nine-jump-tables)) | no named constant binds any of the nine, and the strings past the eleven have no pointer to follow |
| `0899` | `0x1EB28`, 3552 B | the unclaimed part of a 16 KB data-segment hole | one address formed at one site, which is below the uninitialised-data bar above |
| `0899` | `0x0`, 3512 B | the menu overlay's option-label string pool | same as `0898`: no pointer table this workspace reads |
| `0899` | `0x2050C`, 844 B | the save-screen message slots on a `0x80` stride, and the memory-card filename prefix behind them | the stride is measured off the slots, not off a consumer |
| `0899` | `0x15BEC`, 820 B | unidentified data-segment words; the leading rows read as `[u16][u16]` pairs but the shape does not hold across the run | nothing identified |
| `0967` | `0xCAC`, 1757 B | the tutorial module's own ASCII prompt pool - 1689 of its bytes are printable, in thirty runs | no pointer table or consumer traced to it yet |
| `0970` | `0x2534`, 3152 B | the STR overlay's initialised data segment: the one-shot init flag at `0x801D0D4C` and the MDEC hardware-register pointers `FUN_801CFFDC` loads from `0x801D0E60`..`0x801D0E98` | a pointer block, not a table with a stride |

`0967`'s run is smaller than it first measured, and the difference is two
instrument gaps rather than a finding about the image. Selecting the slot-B
walk on the link base ([above](#the-slot-b-images)) claims its 408-byte head
table, and cutting the inherited tail removes the 1143 bytes above file
`0x1389` that are PROT `0966`'s. What is left of the old "2992 bytes of
un-dumped code" is 92 bytes of code and a string pool: the 23 words at `0xC50`
are one leaf routine ending `jr ra` at `0xCA4`, and everything from `0xCAC` up
is ASCII. Reading the whole run as code was the shape classifier's verdict over
a window that was mostly another module's bytes; `disc-coverage.py`, which cuts
the tail, reports **no** un-dumped code run in this image at all.

### The `0898` head is nine jump tables

The row above used to read "a `switch` jump table" from file `0x9B4`. There are
nine runs of in-image VA words below `0xDF8`, and the singular was an artifact
of looking at one of them:

```text
file 0x0068..0x01E4   VA 0x801CE880    95 words
file 0x01E8..0x02B4   VA 0x801CEA00    51 words
file 0x02B8..0x034C   VA 0x801CEAD0    37 words
file 0x0350..0x0490   VA 0x801CEB68    80 words
file 0x0494..0x0500   VA 0x801CECAC    27 words
file 0x052C..0x09B0   VA 0x801CED44   289 words
file 0x09B4..0x0C80   VA 0x801CF1CC   179 words
file 0x0C84..0x0C98   VA 0x801CF49C     5 words
file 0x0C9C..0x0DF8   VA 0x801CF4B4    87 words
```

Two of them have their consumer, and both consumers form the table address the
way the [address-reference scan](address-reference-scan.md) says to look for -
a `lui`/`addiu` pair, which is why a literal-word sweep finds neither.

`0x801CF1CC` (179 words) is dispatched by the `jr` at `0x801EA9FC`. The index is
a byte, not a state word: `lbu` from `0x8007BD0C + $s7` (the per-seat byte the
same body writes back at `+0x1DE`), less 4, and the bound is the `sltiu $v0,
$v1, 0xB3` at `0x801EA9D8` - **179**, the table's own word count exactly, which
is what makes the extent a measurement rather than a scan. 37 distinct arms;
`0x801EBDAC` is the default and fills every unused slot.

`0x801CF49C` (5 words) is dispatched by the `jr` after `0x801EB558`, indexed by
the battle context's `+0x28A` phase byte under `sltiu $v0, $v1, 5` at
`0x801EB540`, with the same `0x801EBDAC` default on the out-of-range arm.

Neither is claimed. A [pinned table row](#a-pinned-offset-is-not-a-magic-sweep)
has to be a binding to a parser's own `pub const`, and no module here reads
either table; what is written down above is the extent, the index expression and
the bound, so a parser that wants them has the three facts it needs.

### A residue run that is another image's code

PROT `0975`'s 1760-byte `plausible_mips` run at file `0x5920` has no prologue, no
`jr ra` and no caller, and it runs to the last byte of the entry. None of the
three readings that invites - a jump-table body, data, or the interior of a
neighbouring function - is right: those bytes are **PROT `0972`'s**, byte-identical
at the same file offset, and `0972` is nearly twice as long. It is an
[inherited tail](disc-coverage.md#content_bytes-is-longer-than-the-images-own-code-the-inherited-tail) - the packer wrote
the shorter module into a buffer it did not clear, and the residue is whatever
the longer module left there. The run is a mid-function slice of the fishing
overlay, which is exactly why it has no entry and no exit.

Both instruments cut those bytes now, and they cut them by the same
measurement. `legaia_asset::inherited_tail` is the Rust side of
`scripts/ghidra-analysis/inherited_tail.py`: the same suffix comparison, the
same `0x40` minimum, the same gated equal-extent leg and the same
cut / own-content fixpoint, run over the images named by
`crates/asset/data/static-overlays.toml`. Every row there is `form = "raw"`
with `content_source = "prot_entry_extent"` and each row's `content_bytes`
equals its extracted entry's file length exactly, so the entry file **is** the
as-loaded image and the walk needs no `extracted/overlays/` tree - only
`--prot-dir`, which `asset account` already takes. Without it the report says
so in a note rather than silently counting a donor's code as this entry's
residue.

The two sides are held to each other cut for cut
(`crates/asset/tests/inherited_tail_real.rs`) and they agree on every mapped
image but PROT `0944`. What disagrees there is not either tail implementation
but the own-content figure they both call: this side bounds `0944`'s top
pointer-credited record and chains fourteen more above it to file `0x1EC8`, the
Python side leaves that record unbounded and stops at `0x1948`. The bytes say
the cut is `0x199C` with `0942` as donor - `0944` and `0942` are identical from
there to the end and differ immediately below it, and `0942`'s own content
reaches above it - so this side's cut is 1412 bytes too high, which
under-claims the tail rather than crediting a neighbour's bytes to a parser
here. Settling it is a slot-B record-walk question, not a tail one.

The claim is made before any walker runs, so a walker that reaches into the
tail loses nothing (claims merge) while the residue classifier no longer sees
the run. On the slot-B images it is also fed back into the walk:
`slot_b_module::parse_with_tail` drops every spawn record at or above the cut,
because a spawn call site up there belongs to the donor and so does the record
pointer it forms. Four band images report slightly *more* residue for that
reason - the record chain used to run past the cut and claim the donor's
bytes - and that is the instrument getting stricter, not worse.

### The residue run **count** was capped, and read as a measurement

Three entries reported "64 runs" in the sweep, which is not a coincidence: the
report keeps only the 64 longest runs, and the sweep was reading the length of
that kept list. The real counts are one run per slot (194 / 92 / 84), and the
figure that was never capped is `largest`, because the list is sorted longest
first before it is truncated. The report has always carried the true count in
its own `residue_runs` field; the sweep reads that field now. A capped list is
a reporting bound - never derive a statistic from its length.

### The slot-B images

An image is walked this way when its `static-overlays.toml` row is **linked at
the slot-B base** `0x801F69D8` - not when its index falls in a range, and not on
a class or on the presence of a dump directory. The three regions this walk
recovers are each resolved by comparing a word against that base, so the base is
what makes the walk apply; the structural regions come out of the image itself,
so `asset account 0923` walks them with or without `--funcs` (it delegates to
the overlay-code walker when one is given). Walker `slot_b_module`; parser
[`legaia_asset::slot_b_module`](../formats/slot-b-module-layout.md),
predicate `is_slot_b_image`.

Seventy mapped images sit at that base. Sixty-four are the cast / summon band
`0903..=0966` the three PROT 0898 entry tables reach
([`cast-module.md`](../subsystems/cast-module.md)) - which is what
`is_slot_b_module` still names, because "which images does the cast dispatcher
reach" is a different question from "which images does this layout describe".
The other six are the two render occupants (`0900` `summon_render`, `0901`
`world_map_render`), the battle tutorial and the two battle stage modules
(`0967` / `0968` / `0969`) and the staged texture loader (`0978`
`field_back_read`). Selecting on the index band left all six measured as if they
had no head table, which is what kept `0967`'s 408-byte table of in-window VAs
in the residue and `0968`'s seven spawn records with it.

| Claim | Owner | What it is |
|---|---|---|
| head jump table | `toc` | the leading run of in-image VA words, bounded by the first frame-matched function |
| spawn record `i` | `record` | `[i16 model_sel][u16 reserved][move-VM bytecode]`, bounded by the next record pointer or by the next function's prologue |
| spawn record `i`, *chained* | `record` | a record **above** the highest pointer-credited one, its start taken from the record below it and its end from its own program |

The first two rest on addresses the module's own code computes and hands to
`FUN_80021B04` / `FUN_80050ED4` in `$a2`, which is why they are structural claims
and not `scan` ones. The third rests on the move-VM width walk instead - the
reason line says so per claim, so the two kinds of evidence never blur. The
image's **highest** record is bounded by its own program's terminator, and on
retail every image that has one is bounded; where a walk does not terminate the
record stays residue and the walker's note names the offset.

What the set measures, and why it is the cleanest class on the disc: all 70
images select this walker, over 653312 bytes, and every claim is structural -
`scan_bytes` is **zero** across the set, so nothing in the figure rests on a
magic guess. Residue is 11216 bytes (1.72%), and 2618 of those bytes are
`zero_pad`. Per entry the accounted share runs 69.2% to 100.0% with a median of
99.1%; the floor is `0967`, whose own content above its code is a string pool no
consumer has been traced to yet. The classes the entries carry
(`overlay_ptr_table` 42, `mips_overlay` 22, `overlay_data_blob` 6) are a
statistic over the bytes and do **not** select the walker - the link base does.

## Interpreting a report

- A large `high_entropy` run that no parser claims is a compressed or sample-data region with no
  walker. That is the top of the worklist.
- A large `plausible_mips` run in a non-overlay entry means either an un-dumped routine or a
  container whose walker stopped early.
- A `low_entropy` or `mixed` run adjacent to a claimed table is usually the table's body: the walker
  found the header and stopped.
- `zero_pad` and `repeated_fill` are not work. Sector padding and dev fill
  ([`pochi.md`](../formats/pochi.md)) are the disc's own slack.
- A high `structural` with a low `accounted` gap means the magic sweep found nothing extra, which is
  the healthy case. The reverse - `accounted` far above `structural` - means most of what was found
  was found by guessing at magics, and the container's layout is still unwalked.

## Ranked residue across the whole TOC

One entry at a time is the right shape for a hunt and the wrong shape for a worklist: the entries
whose residue is worth walking are not the ones anybody thought to run `asset account` on.
`scripts/asset-investigation/byte-account-sweep.py` runs every PROT entry and rolls the result up by
entry class and by residue shape, so what lands at the top is a property of the disc rather than of
what somebody sampled.

```bash
scripts/asset-investigation/byte-account-sweep.py            # sweep + rollup
scripts/asset-investigation/byte-account-sweep.py --top 25
```

It writes a CSV of numbers only - the per-run `head` hex that `--json` prints is disc bytes and is
dropped - under `target/byte-account/` (gitignored). Nothing about that file is meant to be
committed: the numbers move with every parser that lands, so what belongs on this page is the
*shape* of the residue, which does not.

### What the rollup says

Read the classes in three groups; only the first is work.

The `entries` and `bytes` columns are the disc; the `non-slack residue` column is
a **snapshot of the instrument** and moves with every parser that binds - re-derive
it rather than quoting it. Its denominator is the whole TOC: 1233 entries,
121006080 bytes. At the state below, 0.55% of that is residue, and 0.31 of those
0.55 points are slack (`zero_pad` / `alignment` / `repeated_fill`), leaving 0.24%
non-slack. The magic sweep contributes nothing: `accounted` and `structural` are
the same figure, so no part of the accounted share rests on a guessed magic.

| Class | entries | bytes | non-slack residue |
|---|---:|---:|---:|
| `overlay_data_blob` | 25 | 17164288 | 127022 |
| `scene_asset_table` | 90 | 22577152 | 77420 |
| `overlay_ptr_table` | 42 | 407552 | 46428 |
| `mips_overlay` | 22 | 194560 | 21836 |
| `lzs_container` | 18 | 4098048 | 12889 |
| `init_pak` | 1 | 153600 | 2940 |
| `scene_event_scripts` | 101 | 329728 | 2048 |
| `bse_bank` | 2 | 6144 | 1716 |
| `pack` | 7 | 1634304 | 948 |
| `summon_readef` | 2 | 12232704 | 20 |
| `data_field_streaming` | 49 | 9052160 | 0 |
| `scene_vab_stream` | 218 | 22450176 | 0 |
| `scene_tmd_stream` | 182 | 14632960 | 0 |
| `battle_data_pack` | 4 | 1863680 | 0 |
| `vab_multi_bank` | 1 | 6002688 | 0 |
| `efect_pack` | 1 | 8192 | 0 |
| `scene_v12_table` | 97 | 198656 | 0 |
| `pochi_filler` | 266 | 544768 | 0 |
| `all_zeros` | 4 | 8192 | 0 |
| `field_map` | 101 | 7446528 | 0 |

| Class | What its unclaimed bytes are | Verdict |
|---|---|---|
| `vab_multi_bank` (`0891`) | Nothing: the bank index, each bank's two chunks and each bank's sector slack are claimed from lengths the container states. | Closed. Layout in [`vab.md`](../formats/vab.md#the-multi-bank-archive-monstersnd). |
| `overlay_data_blob` | The whole class's remaining work, and most of it is entry `0896`, whose extent reads `plausible_mips` although its head is a length-prefixed Shift-JIS label table. The rest is per-image data segments beside code the dump corpus reached. | `0896` links at `0x801D4DF0` and calls no function entry of this disc's executable - a foreign-build image, resident in no state here. |
| `overlay_ptr_table`, `mips_overlay` | `low_entropy` runs with a `plausible_mips` minority - the tables beside code the dump corpus has not reached. | Dump worklist; agrees with [`disc-coverage.md`](disc-coverage.md)'s gap list. |
| `init_pak` (`0895`) | The head pointer table, the SCUS-name string, and a tail past the last logo. | Closed but for those three; see the composition rule below. |
| `lzs_container` | Per-entry tails of a few hundred bytes past the last descriptor's stream, plus `0981` entire - the one class member that is a code image rather than a container. | Walker tails plus one mis-classed entry. |
| `scene_asset_table`, `pack` | Short `mixed` / `low_entropy` runs at the tail of records the walker did reach, plus one `high_entropy` minority in `scene_asset_table`. | Walker tails, not unwalked format. |
| `scene_vab_stream`, `scene_tmd_stream`, `data_field_streaming` | Nothing: the chunk walk reaches the terminator and what is left of the entry's last sector is claimed as slack. This class's residue used to be its single largest figure and read as "walker tails". | Closed; see the fixed-stride section above. |
| `battle_data_pack` | Nothing but inter-record alignment: all four entries account whole. | Closed; the table-to-data gap is declared slack. |
| `bse_bank`, `scene_event_scripts` | Kilobyte-scale `low_entropy` / `ascii_text` tails behind a walker that reached the records. | Walker tails. |
| `efect_pack` (`0873`) | Nothing: the header, the inline sprite atlas, and both packs' members account fully. | Closed. |
| `pochi_filler`, `all_zeros`, `scene_v12_table` | Nothing, or `zero_pad`. | The disc's own slack. Not work. |
| `summon_readef` | Tens of bytes of inter-record alignment. Each slot's fill is claimed out to the stride the stream SM transfers. | Closed. |
| `field_map` | Nothing: all 101 entries account fully. | Closed. |

### Three families that read as unwalked format and were not

Each of these headed the ranking at some point and none of them turned out to
be an unrecognised format. All three were bindings: the bytes were already
understood somewhere, and nothing connected that understanding to the walker.
That is the shape to expect at the top of this ranking, and it is the reason to
read the per-class verdict column before starting work on a row.

| Family | What the bytes are | What was missing |
|---|---|---|
| `lzs_container`, 18 entries | The descriptor bundle [`scene-bundles.md`](../formats/scene-bundles.md) specifies, at counts the *detector's* window excludes - retail bounds the count nowhere. | A walker bound to the class. |
| `pochi_filler`, 266 entries | One 1927-byte dev fill file plus 121 bytes of the mastering buffer's previous contents ([`pochi.md`](../formats/pochi.md)). | A shape rule. The fill is text-shaped and its line is 52 bytes long, so `repeated_fill`'s period-1/2/4/8/16 test never fires and 266 sectors of filler ranked as work. |
| `other5` / `other6`, 2 entries | Four `320x64` 16bpp band uploads to VRAM `(384, 0)` ([`ringside-still.md`](../formats/ringside-still.md)). | A walker. The rectangle was already recovered from the consumer's immediates. |
| `vab_multi_bank`, 1 entry | 206 VAB banks on a sector index the entry's own head carries, each a two-chunk stream ([`vab.md`](../formats/vab.md#the-multi-bank-archive-monstersnd)). | A walker. The detector already read the count and the sector table - it just never claimed anything with them. |
| `init_pak`, 1 entry | The boot overlay's four publisher-logo TIMs, at offsets `legaia_asset::init_pak` already knew. | Nothing about the format: a **dispatch** rule dropped the walker. See below. |

### Two ways the headline number lies, both visible in the sweep

**A low percentage that is finished.** Every `scene_v12_table` entry accounts in the single digits,
and every unclaimed byte across all of them is `zero_pad`. The [walk-on trigger
sidecar](../formats/scene-v12-table.md) is one `0x800` sector whose records occupy a small head and
whose rest is padding the format declares. Nothing is owed there, and ranking a worklist by
accounted share alone would put all 97 of them near the top. Rank by *non-slack* residue instead,
which is what the rollup's own ordering does.

**A high percentage that walked nothing.** Entry `0891` (`vab_multi_bank`) used to account for
almost all of its bytes and structurally for none of them: every claim came from the magic sweep over
the residue, which is evidence that VAG bodies are *there* and nothing more. That is the shape the
`scan` tier exists to expose, and it was the largest instance on the disc - 206 banks' worth of VAG
bodies found by magic while the container's own index table went unread. The entry is walked now
(the index is 206 sector spans; [`vab.md`](../formats/vab.md#the-multi-bank-archive-monstersnd)), so
the lesson has to be read off the tier rather than off that entry: quote `structural`, or carry
`scan_bytes` beside the accounted share, because the two numbers can differ by five points of the
whole disc without the headline saying so.

### The figure counts a parser's claims, not a host's reads

A third thing the headline cannot say, and it is the one that makes an
unchanged number easy to misread. The sweep runs `asset account` per entry, so
every claim comes from one binary asking the parsers what spans they consume.
Whether anything in the engine then *slices those bytes at the right origin* is
outside the question entirely.

The VAB carriers are the worked case. `legaia_vab::parse` claims the same spans
whichever offset its callers hand it, so correcting a call site - the boot
stager, the audio and PCM oracles, the browser runtime, the dialogue path, the
minigame SFX resolver, the patcher - moves nothing here. The figure was right
before the fix and is the same number after it.

So read an unchanged structural percentage across a wave of wiring work as the
expected result rather than as evidence the wiring did not land, and reach for a
different instrument when the question is about a consumer: a disc-gated oracle
that parses at the offset the host uses answers it, and this sweep cannot.

### The pochi corroboration

`pochi_filler` is 266 entries and every one is exactly one 2048-byte sector, which is what
[`pochi.md`](../formats/pochi.md) establishes from the other direction. The sweep adds a shape:
the fill classifies as `ascii_text`, and `repeated_fill` is tested *first*, so the sector is not a
pattern of period 1, 2, 4, 8 or 16 - it is text-shaped. That is a second, independent reason none of
these slots carries a parseable asset.

It is also why the class had to be claimed rather than left to the classifier.
A shape vocabulary tests the *statistics* of a run, and the two things a filler
slot is made of - a 52-byte-period fill file and a sector tail that is another
entry's bytes - are indistinguishable from content by any statistic. The
walker claims both as `pad` with a detail apiece, which is the difference
between "the instrument knows what these bytes are" and "the instrument's
shape tests do not object to them".

## Ratcheting the figure

A sweep that nobody compares is a number in a terminal. `scripts/ci/byte-account-coverage.py`
reads the sweep's CSV and ratchets it the way [`disc-coverage.py`](disc-coverage.md) ratchets the
code figure, against a committed baseline at `scripts/ci/byte-account-baseline.json`.

```bash
scripts/ci/byte-account-coverage.py                  # report
scripts/ci/byte-account-coverage.py --check          # ratchet (hook + CI)
scripts/ci/byte-account-coverage.py --update-baseline
```

It is the third denominator, and the reason it exists beside the other two is that they cannot ask
this question. `port-catalog.py` measures the addresses this project has cited. `disc-coverage.py`
measures the disc, but its DATA half is format **recognition** - "this entry is a
`scene_vab_stream`" - which an entry satisfies fully while most of its bytes have never been walked.
That gap is exactly what `asset account` reports, and the ratchet is what keeps it from sliding.

### What ratchets, in which direction

| figure | direction | why |
|---|---|---|
| `structural_pct` | up only | bytes a parser walked to, from a header or a table |
| `accounted_pct` | up only | structural plus magic-sweep hits - always the larger number, never the headline |
| `work_bytes` | down only | unconsumed runs whose shape is not slack |

Every figure is re-weighted by **bytes**, not averaged over entries: a mean over 1233 entries makes
one 15 MB archive weigh the same as one 2 KB filler sector, which is a statement about entries and
not about the disc.

Each is carried per class as well as whole-disc, for every class holding at least 1 MB. A per-class
row is what makes a regression attributable - one class losing its walker moves the whole-disc
figure by a rounding error, and "the total fell 0.3 points" does not say which parser to open.

Slack shapes stay out of `work_bytes` for the reason the rollup keeps them apart: `zero_pad`,
`alignment` and `repeated_fill` are the disc's own padding, most of the residue by bytes, and
counting them as work produces a worklist nobody can act on.

### A stale binary is the same failure, one layer down

The sweep's CSV is a cache and the page says so below. The `asset` binary the
sweep runs is a cache too, and that one fails more quietly, because nothing
about a built binary announces which tree it came from. A committed CSV and a
"fresh" per-entry run disagreed on two entries - `0970` at 5.6% against 20.5%,
`0895` at 98.1% against 46.5% - and the natural reading, that the CSV was
stale, was backwards. Both "fresh" figures are this page's own *historical*
numbers: 20.5% is `0970` before the zero-match rule stopped crediting an
all-zero extent, and 46.5% is `0895` before the `init_pak` walker took the
entry back from the code-walker override. A binary built before those two
fixes reproduces both exactly, against the current tree and the current disc.

Two habits follow. Build the binary from the tree you are measuring, in that
tree - a `target/` from another checkout is not a tool, it is a previous
answer. And when two figures disagree, check whether the lower one is a number
this page already explains, because a documented historical figure reappearing
is a stale *instrument*, not a finding about the disc.

### The input is a cache, and a stale one passes

The sweep needs the disc, the `asset` binary and the dump corpus, so the gate does not run it - it
consumes the CSV, and with no CSV it SKIPS and exits 0, which is what keeps CI green without disc
data. The failure mode that leaves is the one the categorize cache has: a CSV swept on an older
tree reports *that* tree's parsers through a passing gate. Re-run the sweep after a parser change
and before taking a baseline; `--max-age-days` is the guard for any automated use.

## Tests

`crates/asset/tests/byte_account_entries.rs` accounts a fixed set of entries off `extracted/PROT`
and asserts a floor on each one's accounted share, plus the invariants that make the figure
meaningful (claims stay inside the buffer, residue plus accounted equals the size, `structural`
never exceeds `accounted`). It skips and passes without extracted data, like every other
disc-dependent test. The range algebra and the residue classifier are unit-tested on synthetic
buffers inside the module, including the two ordering traps above.

Three of its cases are corpus-wide rather than per-entry, because the claim they guard is about a
family: every filler slot carries the same fill file and a tail that appears in some other entry at
the same offset, every count-4 / 5 bundle's `FLAG` slot decompresses to that fill file, and both
stills are covered entirely by their four band claims.

One of those replaced a test that **asserted the defect**: it required entries `1221` / `1222` to
report walker `generic`, which was a true statement about the instrument and a false one about the
disc, and it would have failed the moment a walker landed. Pin the absence of a binding only where
the absence is itself the finding.

`crates/asset/tests/slot_b_module_layout_real.rs` covers the module band over all 64 entries: no
claimed record overlaps a framed function, every claimed record's start is an address some spawn
call in the image really hands over in `$a2` (re-derived from the raw words, not taken from the
parser), and the band is non-vacuous. Same skip-and-pass gating.
