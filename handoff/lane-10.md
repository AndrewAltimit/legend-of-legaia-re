# Lane 10 handoff - the unexplained share of disc DATA coverage

## Result

`python3 scripts/ci/disc-coverage.py` (data half), same dump corpus both runs:

| | before | after |
|---|---:|---:|
| parsed to a named format | 265208299 (87.8%) | 278038528 (92.3%) |
| documented placeholder / padding | 28555264 (9.4%) | 23318528 (7.7%) |
| **unexplained** | **8419328 (2.79%)** | **0 (0.000%)** |

**Every statistical residual bucket is now empty** - `unknown_other`,
`unknown_low_entropy`, `unknown_high_entropy`, `mostly_zeros`, `constant_byte`.
No PROT entry falls through to a class named after its byte histogram rather
than its format. All five are pinned at 0 in `validation_suite` so a detector
regression fails loudly instead of quietly re-filing content under a histogram.

Nothing else moved: a per-file diff of the baseline vs final `categorize.json`
shows exactly 108 reclassifications and zero collateral.

**Read the totals with the caveat below.** The denominator counts some disc
bytes more than once. Under the correct (footprint) extents the same class map
reads **parsed 99.5% / placeholder 0.5% / unexplained 0.0%** of a 121 MB
archive; the table above is the same map against the gate's current 302 MB
denominator. Both are honest; they answer different questions. See
[The denominator defect](#the-denominator-defect-largest-finding-of-this-lane).

## Per-entry verdicts - all 6 `unknown_other`

| Entry | Verdict | Grounding |
|---|---|---|
| `0083_suimon.BIN` | per-scene field map (`map01` block slot 0) | disassembly |
| `0242_tunnela.BIN` | per-scene field map (`map02` block slot 0) | disassembly |
| `0389_deene.BIN` | per-scene field map (`map03` block slot 0) | disassembly |
| `0873_befect_data.BIN` | runtime `efect.dat` 2-pack (extraction 873) | doc + byte-pattern |
| `0894_card_data.BIN` | `readef.DAT` battle side-band file (extraction 894) | doc + byte-pattern |
| `0895_bat_back_dat.BIN` | boot `init.pak` (extraction 895) | doc + byte-pattern |

The last three were **already documented formats with no detector**:
[`effect.md`](../docs/formats/effect.md) (runtime 2-pack),
[`summon-readef.md`](../docs/formats/summon-readef.md),
`boot.md` + `crates/asset/src/init_pak.rs`. Each now has one.

## The finding: 101 entries were one undetected format

Every scene's CDNAME block opens with an entry of **exactly `0x12000` bytes** -
101 of them, all at block slot 0 (extraction `define - 2`). That is
`DATA\FIELD\<scene>.MAP`, the per-scene field map, and `0x12000` is the exact sum
of the four regions the runtime addresses off `*(0x1F8003EC)`:

```text
+0x00000  0x4000  object / actor descriptors   0x20 stride, 512 slots
+0x04000  0x4000  collision + floor grid       1 byte/tile, 0x80 rows
+0x08000  0x8000  per-tile object-index map    u16/tile,   0x100 rows
+0x10000  0x2000  per-tile trigger block       header + 4 sub-tables
```

The per-field semantics were already documented in
[`field-locomotion.md`](../docs/subsystems/field-locomotion.md); what was missing
was a container page and a detector. New page:
[`docs/formats/field-map.md`](../docs/formats/field-map.md). New module:
`crates/asset/src/field_map.rs`.

**Detector**: `size == 0x12000` plus a trigger-block header that chains -
`offset[0] == 0x12`, each following `offset[k]` equal to the previous body's end
plus a 2-byte gap, and the `u16` at `+0x00` equal to the last body's end + 2.
100 of the 101 satisfy it; **zero** entries outside the size class do, at any
size. The 101st (extraction 126, the `dream` block's map) ships the header
zeroed, and its object table and both grids are zero too - a cutscene-only scene
with no walkable field. Accepted with an absent trigger block.

## What the 70 `mostly_zeros` were

**69 of the 70 were field maps.** The class is a statistical bucket
(`zero_fraction >= 0.75`) that `disc-coverage.py` then counts under "documented
placeholder / padding" - so the file carrying every scene's collision grid, floor
heights, object placements and door triggers sat in the *explained-as-empty*
column, split across three buckets by nothing but how crowded each scene is
(69 `mostly_zeros`, 29 `unknown_low_entropy`, 3 `unknown_other`).

That caveat is now written up as
[`disc-coverage.md` § A statistical class is not a verdict](../docs/tooling/disc-coverage.md#a-statistical-class-is-not-a-verdict),
with the practical lead that found it: **group the unclaimed entries by exact
size and by slot position within the CDNAME block**. A size repeating across a
hundred entries is a fixed-layout format whatever its byte statistics say.

The surviving `mostly_zeros` entry is **extraction 0970** - also not a
placeholder. It is the STR/FMV + MDEC overlay's data image: the nine FMV path
strings that match [`str-fmv-table.md`](../docs/formats/str-fmv-table.md)'s
dispatch table, the per-FMV return-scene names, two 11-word runs of
`0x801Cxxxx` pointers at `+0xC8`/`+0xF8`, and the PsyQ MDEC library's debug
strings. At 91.6% zeros with sparse small data runs it is an overlay *data*
segment, not code - so neither `mips_overlay` (offset-0 prologue) nor
`overlay_ptr_table` (offset-0 pointer run) reaches it. Left in `mostly_zeros`
rather than invent a class from one sample; see "Proposals" below.

## Negative finding for `re-do-not-re-walk.md` (Lane 2 owns that file)

**`monster_sound_bank` never matched `monster.snd`. It matched `summon.dat`, and
it is the +2 CDNAME label shift with a byte coincidence on top.**

- The class tests `[u32 format == 2][u16 spu_addrs[256], all >= 0x8000]` and
  cited `h:\mpack\monster.snd` loaded by `FUN_8003E104`.
- Its one match was extraction **893**, which
  [`summon-readef.md`](../docs/formats/summon-readef.md) confirms is
  `summon.dat`. That file opens `[u32 mode = 2]` followed by a 256-entry CLUT in
  which every colour carries the STP bit - so all 256 `u16`s read `>= 0x8000`,
  byte-for-byte the "256 active SPU slots" shape. The give-away the test cannot
  see: the values repeat (`0x8000 0x8000 0x8000 0x8000 0x8001 0x8001 ...`), and
  SPU sample addresses are strictly increasing.
- `monster.snd` is extraction **891**: `FUN_8003E104` does `li v0,0x37d` at
  `0x8003E174` (raw TOC `0x37D` = 893 = extraction 891), immediately alongside
  the `h:\mpack\monster_snd` path string.
  `see ghidra/scripts/funcs/8003e104.txt`.
- Extraction 891 classifies as `vab_multi_bank` - a 206-bank VAB archive. So
  `monster.snd` is a multi-bank VAB, and `vab_multi_bank`'s docstring calling it
  "the level_up cluster's" archive was the same +2 shift read off the extraction
  filename. Both docstrings are corrected in `categorize.rs`.

With `summon_readef` ordered ahead of it, `monster_sound_bank` matches **no**
PROT entry. The class is kept (so the shape stays named and cannot be
re-derived by accident) and pinned at 0 in `validation_suite.rs`.

## CLAUDE.md rows to add (coordinator owns that file)

Two new format pages. In the **Formats** table, directly after the
`scene-v12-table.md` row:

```
| [`field-map.md`](docs/formats/field-map.md) | Per-scene `DATA\FIELD\<scene>.MAP` - the fixed `0x12000`-byte slot 0 of every scene block (101 entries). Four regions whose sizes sum to the footprint exactly: object descriptors, the collision + floor grid, the per-tile object-index map, the trigger block. Detected on the trigger block's sub-table chain; per-field semantics live in [`field-locomotion.md`](docs/subsystems/field-locomotion.md). |
```

and in the **Auxiliary** group, next to `sound-driver.md`:

```
| [`bse-dat.md`](docs/formats/bse-dat.md) | `bse.dat` master sound bank - the file `FUN_8001FA88` loads once at sound-init into `_DAT_8007B8D0`. `[u16 tag][u16 body_offset][8-byte records]`; the `+2` word is a byte offset, not a count. Extraction 888 (the loader's raw TOC `0x37A`) plus an uncalled sibling at 1195. Record columns are shape, not semantics. |
```

Also, the existing `prot.md` row quotes the size formula that this lane
falsified. Suggested replacement text for that cell:

```
| [`prot.md`](docs/formats/prot.md) | PROT.DAT TOC (`start_lba = toc[p+2]`; entry size is the **footprint** `toc[p+3] - toc[p+2]` - the page carries why the older `toc[p+5] - toc[p+3] + 4` formula is not entry `p`'s size). |
```

## Out-of-scope edits I had to make

`crates/extract/tests/validation_suite.rs` pins the per-class census
(`EXPECTED_CLASS_COUNTS`), so adding classes turns it red. I updated that one
constant (plus its comments) across both rounds and re-ran it green. **No logic
touched.** If another lane is editing that file, this is the merge point.

`docs/guides/extracting-assets.md`, `docs/subsystems/shop.md` and
`site/_content/*` were granted explicitly for the `shop-stock` request.

## Extraction 1195 - resolved. It is `bse.dat`-shaped, and it is 2048 bytes

Following `FUN_8001FA88`'s arithmetic closed this row and found the denominator
defect below on the way.

**Extraction 888 is `bse.dat`, the master sound bank** - loader-grounded, not
shape-guessed. `FUN_8001FA88` allocates one `0x1800`-byte buffer into
`_DAT_8007B8D0` and fills it down either the dev branch (path opener on the
`"bse.dat"` string at `0x8007B3AC`) or the retail branch
(`byindex_sync_loader(0x37A, …)`, `li a0,0x37a` at `0x8001FAD0`). Both branches
write the **same destination**, so the dev file name and the retail TOC index
name one asset: raw `0x37A` = extraction 888. The function's tail computes
`gp[0x678] = base + ((s16)u16@+2 / 2) * 2`, so the `+2` header word is a byte
offset to a table of 8-byte records. `see ghidra/scripts/funcs/8001fa88.txt`.

**Extraction 1195 is the same format, 7 records, and nothing calls it.** Its raw
TOC index `0x4AD` appears as a load literal in no dumped function, while its
block neighbours `0x4B0` / `0x4B1` do (slot-machine assets) - so the absence is
not merely that nothing in that block has been dumped. The dump corpus is
incomplete, so this is evidence of an unused sibling, not proof.

Record column semantics are **explicitly not claimed**. The
`(program, tone, unity key)` reading is labelled a hypothesis in both the module
and [`bse-dat.md`](../docs/formats/bse-dat.md), because no consumer of
`gp[0x678]` has been traced. What is pinned is the loader, the destination, the
header word's use as a byte offset, and the stride.

**Extraction 0970 also settled - and it is not a singleton.** It is the STR/FMV
+ MDEC overlay's *data* image. Scanning for its structure (leading
NUL-terminated ASCII pool + a `>= 8`-word run in the overlay load window) finds
**12** entries; 11 were already `overlay_data_blob` / `overlay_ptr_table`, so
0970 is an ordinary member of a family, not a new format. It landed in
`mostly_zeros` only because the printable-ASCII test that recognises its
siblings is a whole-buffer ratio and 91.6% zeros dilutes it under threshold.
The structural test now runs ahead of the zero-fraction gate; exactly one entry
moves. No new class was invented for it.

## The denominator defect (largest finding of this lane)

**`indexed_size_sectors` is not an entry's size, and the data half's totals are
~2.5x the archive because of it.** `toc[p+3]` is entry `p+1`'s start LBA and
`toc[p+5]` is entry `p+3`'s, so `toc[p+5] - toc[p+3] + 4` measures a span of
*neighbouring* entries. The extractor takes `max(indexed, footprint)`, so for
the 931 entries where the wrong number is larger the extracted `.BIN` runs past
the entry into its neighbours, and those bytes are weighed again under every
entry that overlaps them.

Proof, four independent parts:

1. **The footprints tile `PROT.DAT` exactly** - monotonic starts, entry 0 at LBA
   121, entry 1233's end marker at the archive's last sector, and the sum equal
   to the contiguous span with no gaps and no overlaps. A partition with that
   property *is* the entry layout. The `max()` totals sum to 2.49x the 121 MB
   archive, which no partition of it can. (The "entry `p`'s tail equals entry
   `p+1`'s head" check is a **tautology** - footprint is defined as the gap to
   the next entry - so it is not evidence and is not cited as such.)
2. **The runtime uses the footprint**: `FUN_8003E8A8` returns
   `TABLE[idx+3] - TABLE[idx+2]`, and `FUN_8003EB98` passes it straight to the
   sector read.
3. **Known-length files agree with it and not with the other formula**:
   `readef.DAT` = exactly 78 x `0x10800`, `summon.dat` = exactly 103 x
   `0x10800`, every field map = exactly `0x12000` (its four regions' sum),
   `bse.dat` = 2 sectors, which is what lets it fit the `0x1800` buffer its
   loader allocates. The `+4` formula gives a non-multiple, a truncation that
   stops inside the object table, and a 43x buffer overrun respectively.
4. **PROT 899's documented "trailing overlay" case is the footprint being
   right** and the `+4` formula being short - never a counter-example.

`prot.md` now carries the correction with the proof, and `disc-coverage.md` has
the consequence for reading its own table. **The extractor is deliberately not
changed**: switching `size_sectors` would rewrite every extracted file and every
pinned census in the repo, which is not a parallel-wave edit.

Recommended, in this order: (a) fix `crates/prot`'s `size_sectors` to the
footprint on a quiet branch and re-pin the censuses; (b) once that lands, the
data half's totals become meaningful and the 99.5% figure is the one to quote.

## Proposals (did not do)

1. **`disc-coverage.py`**: moving `mostly_zeros` from `PLACEHOLDER_CLASSES` to
   `UNEXPLAINED_CLASSES` is still the right policy - a statistics bucket with no
   format page should not count as explained - but it is now a **no-op on this
   disc**, because the bucket is empty. Take it for the invariant, not for a
   number change.
2. **`engine-core::field_regions`** carries duplicate copies of the field-map
   region offsets (`MAP_REGION_BLOCK_OFFSET`, `MAP_OBJECT_INDEX_OFFSET`,
   `MAP_OBJECT_DESCRIPTOR_STRIDE`, `MAP_TRIGGER_FALLBACK_OFFSET`). They agree
   with `legaia_asset::field_map`'s constants; engine-core depends on asset, so
   they could be re-exports instead of a second source of truth.
3. **`docs/formats/overlay-ptr-table.md`**: worth a line that the pointer-table
   run is at offset 0 only for overlay *code* images - a data image puts its
   string pool first. The detector for that now lives in `categorize.rs`.

## Left open

- **What `bse.dat`'s record columns mean.** Needs a consumer of `gp[0x678]`.
  The shape is pinned; the semantics are a labelled hypothesis.
- **Whether extraction 1195 is truly unreferenced.** Requires either a complete
  overlay dump corpus or a runtime probe; "no dumped caller" is not "no caller",
  and this repo has a standing rule about exactly that inference.
- **No parser reports consumed-vs-unconsumed bytes**, so 92.3% (or 99.5%) is
  still format *recognition*, not byte accounting. `field_map` is the cheapest
  place to start closing that - its four regions are fixed-size, so a
  consumed-extent report for it is exact rather than estimated.

## `asset shop-stock` (separate user request, delivered)

`asset shop-stock --prot PROT.DAT --scus SCUS_942.54 [--cdname CDNAME.TXT]
[--scene NAME | --entry N] [--json]` - wiring over the existing
`shop_stock` + `item_names` libraries; the scanner is untouched.

Verified against the disc: **34 shops**. "Market" decodes 10 / sells 7 (the
module docstring's worked example), and Biron Monastery's **Corey** is found -
the confirm-picker vendor a linear opcode walk misses, so its presence is the
standing evidence that the site scan has not been "improved" into a walk.

Two design points worth keeping if anyone refactors it:

- The scan is gated on a **name** mask and sellability is computed separately
  with a **price** predicate. A price-gated scan would reject or silently trim
  the unsellable tail, which is the very thing the output exists to expose.
- Records carry their MAN offset, because a scene can hold the same shop more
  than once (one record per script path that opens it).

Docs: a full section in `docs/guides/extracting-assets.md`, a "read it off your
own disc" section on `site/_content/shops.html` (prose only - JSON pipeline and
browser markup untouched), and a three-trap recipe table in
`docs/subsystems/shop.md`.

**Trap for whoever edits the guides next:** `site/_content/guides/*.html` is a
**hand-authored parallel copy** of `docs/guides/*.md`, not a conversion of it.
A section added to the markdown does not appear on the site until it is added
there too - `_gen.py`'s "mirrored from docs/guides/" comment reads as if it
converts, and it does not. `check-site-links.py` is what catches the omission,
and only if something links to the new anchor.

## Files touched

New: `crates/asset/src/field_map.rs`, `crates/asset/src/efect_pack.rs`,
`crates/asset/src/bse_bank.rs`, `crates/asset/src/bin/asset/shops.rs`,
`crates/asset/tests/field_map_real.rs`,
`crates/asset/tests/shop_stock_cli_real.rs`, `docs/formats/field-map.md`,
`docs/formats/bse-dat.md`, `handoff/lane-10.md`.

Modified: `crates/asset/src/categorize.rs` (5 classes, detector order, the
overlay-data-image test, three corrected docstrings), `crates/asset/src/lib.rs`,
`crates/asset/src/summon_readef.rs` (`detect`),
`crates/asset/src/bin/asset.rs` (the `shop-stock` subcommand),
`crates/asset/README.md`, `docs/formats/overview.md`, `docs/formats/prot.md`,
`docs/tooling/disc-coverage.md`, `docs/guides/extracting-assets.md`,
`docs/subsystems/shop.md`, `site/_content/shops.html`,
`site/_content/guides/extracting-assets.html`,
`crates/extract/tests/validation_suite.rs` (census pin - see above).

## Verification

- `cargo fmt` + `cargo clippy -p legaia-asset --all-targets -- -D warnings`
  clean; `cargo test -p legaia-asset --release` all pass;
  `cargo test -p legaia-extract --release --test validation_suite` pass.
- `check-doc-density.py`, `check-md-links.py`, and (after `site/_gen.py`)
  `check-site-links.py` all clean - 126 pages, 2586 internal links, 0
  violations.
- Disc gating proven by **contrast**, not pass count. `field_map_real`: 3 x
  `[skip]` in 0.04s unset vs 0 skips in 6.32s set. `shop_stock_cli_real`: 3 x
  `[skip]` in 0.00s unset vs 0 skips in 0.27s set.
- `disc-coverage.py` was run against a scratch `extracted/` tree so the shared
  `extracted/PROT/categorize.json` in the main checkout was **not** overwritten
  mid-wave. Regenerate it with `asset categorize extracted/PROT` after merge;
  the coverage numbers above come from the regenerated file.

## Commits

| SHA | What |
|---|---|
| `d6371eaa` | `field_map` + `efect_pack` / `summon_readef` / `init_pak`; unexplained 2.79% -> 0.115% |
| `09b622b1` | `bse_bank` + overlay data images; every residual bucket empty; the denominator defect |
| `8489c244` | `asset shop-stock` + guide / site / subsystem docs |
