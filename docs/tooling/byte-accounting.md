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
| `pad` | Declared slack inside a fixed-stride slot that the container's own size math covers. |
| `scan` | Found by a magic sweep over the residue, not by a structural walk. |

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
| `ascii_text` | At least 80 % printable ASCII or NUL. A string pool. |
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

A code image's non-code regions - string pools, jump tables, data segments - fall to the residue
classifier, which is the right answer for them. `ascii_text` and `pointer_dense` runs in an overlay
entry are the segments the dump corpus is not about.

### The slot-B module band

The 64 entries `0903..=0966` are selected on their **index**, not on a class and
not on the presence of a dump directory: they are code images, but their
structural regions come out of the image itself, so `asset account 0923` walks
them with or without `--funcs` (it delegates to the overlay-code walker when one
is given). Walker `slot_b_module`; parser
[`legaia_asset::slot_b_module`](../formats/slot-b-module-layout.md).

| Claim | Owner | What it is |
|---|---|---|
| head jump table | `toc` | the leading run of in-image VA words, bounded by the first frame-matched function |
| spawn record `i` | `record` | `[i16 model_sel][u16 flags][move-VM bytecode]`, bounded by the next record pointer or by the next function's prologue |

Both ends of a record claim are addresses the module's own code computes and
hands to `FUN_80021B04` / `FUN_80050ED4` in `$a2`, which is why these are
structural claims and not `scan` ones. The image's **highest** record has
nothing above it to bound it and stays residue; the walker's note says so per
entry, and names the offset.

What the band measures, and why it is the cleanest class on the disc: all 64
entries select this walker, over 616448 bytes, and every claim is structural -
`scan_bytes` is **zero** across the band, so nothing in the figure rests on a
magic guess. Residue is 80080 bytes (12.99%), and **none of it is slack**: the
band has no `zero_pad` at all, so the unaccounted share is the unbounded top
record in each image and nothing else. Per entry the accounted share runs 52.7%
to 99.1% with a median of 87.2%, and the spread is a property of where each
module's highest record sits rather than of the walker. The three classes the
entries carry (`overlay_ptr_table` 39, `mips_overlay` 20, `overlay_data_blob` 5)
are a statistic over the bytes and do **not** select the walker - the index does.

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
121006080 bytes. At the state below, 11.31% of that is residue, and 6.40 of those
11.31 points are slack (`zero_pad` / `alignment` / `repeated_fill`), leaving 4.91%
non-slack. Of the accounted 88.69%, 5.51 points came from the magic sweep rather
than from a walked layout, so the *structural* share of the disc is 83.81%.

| Class | entries | bytes | non-slack residue |
|---|---:|---:|---:|
| `lzs_container` | 18 | 4098048 | 4098048 |
| `pochi_filler` | 266 | 544768 | 544768 |
| `overlay_data_blob` | 25 | 17164288 | 486042 |
| `vab_multi_bank` | 1 | 6002688 | 210704 |
| `scene_vab_stream` | 218 | 22450176 | 199440 |
| `scene_tmd_stream` | 182 | 14632960 | 141832 |
| `init_pak` | 1 | 153600 | 80864 |
| `scene_asset_table` | 90 | 22577152 | 77420 |
| `overlay_ptr_table` | 42 | 407552 | 55312 |
| `mips_overlay` | 22 | 194560 | 27188 |
| `efect_pack` | 1 | 8192 | 8192 |
| `scene_event_scripts` | 101 | 329728 | 2048 |
| `bse_bank` | 2 | 6144 | 1716 |
| `data_field_streaming` | 49 | 9052160 | 1536 |
| `pack` | 7 | 1634304 | 948 |
| `summon_readef` | 2 | 12232704 | 20 |
| `battle_data_pack` | 4 | 1863680 | 0 |
| `scene_v12_table` | 97 | 198656 | 0 |
| `all_zeros` | 4 | 8192 | 0 |
| `field_map` | 101 | 7446528 | 0 |

| Class | What its unclaimed bytes are | Verdict |
|---|---|---|
| `lzs_container` | The whole entry, `high_entropy`, in one run: no walker binds to the class, so every one of these entries accounts to zero. The decoder exists (`legaia_lzs`) - what is missing is the binding, not the format. | Instrument gap, and the largest non-slack residue on the disc. |
| `overlay_data_blob` | Almost all `zero_pad`. What is left is the `other5` / `other6` pair, `0x28000` bytes each of raw `bgr555`, and entry `0896`, whose whole extent reads `plausible_mips`. | Two raw 16bpp pages with no walker; `0896` is code with no recovered link base. |
| `overlay_ptr_table`, `mips_overlay` | `low_entropy` runs with a `plausible_mips` minority - the tables beside code the dump corpus has not reached. | Dump worklist; agrees with [`disc-coverage.md`](disc-coverage.md)'s gap list. |
| `init_pak` (`0895`) | `ascii_text`: a string pool no walker claims. | Small, and a string pool is not a format. |
| `efect_pack` (`0873`) | One sector-sized entry, `low_entropy`, walker `generic`. | The [effect bundle](../formats/effect.md) has a parser; the account walker does not select it. |
| `scene_vab_stream`, `scene_tmd_stream`, `scene_asset_table`, `pack` | Short `mixed` / `low_entropy` runs at the tail of records the walker did reach, plus one `high_entropy` minority in `scene_asset_table`. | Walker tails, not unwalked format. |
| `data_field_streaming`, `battle_data_pack` | Almost entirely `zero_pad` now; `battle_data_pack`'s residue is slack outright and `data_field_streaming` keeps one `ascii_text` sector. | Closed but for that sector. |
| `bse_bank`, `scene_event_scripts` | Kilobyte-scale `low_entropy` / `ascii_text` tails behind a walker that reached the records. | Walker tails. |
| `pochi_filler`, `all_zeros`, `scene_v12_table`, `summon_readef` | `ascii_text` and `zero_pad` fill. | The disc's own slack. Not work. |
| `field_map` | Nothing: all 101 entries account fully. | Closed. |

### Two ways the headline number lies, both visible in the sweep

**A low percentage that is finished.** Every `scene_v12_table` entry accounts in the single digits,
and every unclaimed byte across all of them is `zero_pad`. The [walk-on trigger
sidecar](../formats/scene-v12-table.md) is one `0x800` sector whose records occupy a small head and
whose rest is padding the format declares. Nothing is owed there, and ranking a worklist by
accounted share alone would put all 97 of them near the top. Rank by *non-slack* residue instead,
which is what the rollup's own ordering does.

**A high percentage that walked nothing.** Entry `0891` (`vab_multi_bank`) accounts for almost all
of its bytes and structurally for none of them: every claim came from the magic sweep over the
residue. The `scan` tier exists so that reads as a warning rather than as a result, and this is its
largest instance on the disc - the bank's own layout is unwalked, and the figure beside it is
evidence that VAG bodies are *there*, nothing more. `0895` carries a smaller version of the same
split. Quote `structural`, or carry `scan_bytes` beside the accounted share.

### The pochi corroboration

`pochi_filler` is 266 entries and every one is exactly one 2048-byte sector, which is what
[`pochi.md`](../formats/pochi.md) establishes from the other direction. The sweep adds a shape:
the fill classifies as `ascii_text`, and `repeated_fill` is tested *first*, so the sector is not a
pattern of period 1, 2, 4, 8 or 16 - it is text-shaped. That is a second, independent reason none of
these slots carries a parseable asset.

## Tests

`crates/asset/tests/byte_account_entries.rs` accounts a fixed set of entries off `extracted/PROT`
and asserts a floor on each one's accounted share, plus the invariants that make the figure
meaningful (claims stay inside the buffer, residue plus accounted equals the size, `structural`
never exceeds `accounted`). It skips and passes without extracted data, like every other
disc-dependent test. The range algebra and the residue classifier are unit-tested on synthetic
buffers inside the module, including the two ordering traps above.

`crates/asset/tests/slot_b_module_layout_real.rs` covers the module band over all 64 entries: no
claimed record overlaps a framed function, every claimed record's start is an address some spawn
call in the image really hands over in `$a2` (re-derived from the raw words, not taken from the
parser), and the band is non-vacuous. Same skip-and-pass gating.
