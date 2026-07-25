# Lane 10 handoff - the unexplained share of disc DATA coverage

## Result

`python3 scripts/ci/disc-coverage.py` (data half), same dump corpus both runs:

| | before | after |
|---|---:|---:|
| parsed to a named format | 265208299 (87.8%) | 278366700 (92.1%) |
| documented placeholder / padding | 28555264 (9.4%) | 23468032 (7.8%) |
| **unexplained** | **8419328 (2.79%)** | **348160 (0.115%)** |

`unknown_other` and `unknown_low_entropy` are now **empty**. `mostly_zeros` went
70 entries -> 1. One unexplained entry remains (extraction 1195), triaged below.

Nothing else moved: a per-file diff of the old vs new `categorize.json` shows
exactly 105 reclassifications and zero collateral.

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

## CLAUDE.md row to add (coordinator owns that file)

In the **Formats** table, under **Streaming + scene containers** neighbours -
place directly after the `scene-v12-table.md` row:

```
| [`field-map.md`](docs/formats/field-map.md) | Per-scene `DATA\FIELD\<scene>.MAP` - the fixed `0x12000`-byte slot 0 of every scene block (101 entries). Four regions whose sizes sum to the footprint exactly: object descriptors, the collision + floor grid, the per-tile object-index map, the trigger block. Detected on the trigger block's sub-table chain; per-field semantics live in [`field-locomotion.md`](docs/subsystems/field-locomotion.md). |
```

## Out-of-scope edit I had to make

`crates/extract/tests/validation_suite.rs` pins the per-class census
(`EXPECTED_CLASS_COUNTS`), so adding four classes turns it red. I updated that
one constant (plus its comments) and re-ran it green. **No logic touched.** If
another lane is editing that file, this is the merge point.

## Proposals (did not do)

1. **`disc-coverage.py`** (measurement instrument, not mine to edit): move
   `mostly_zeros` out of `PLACEHOLDER_CLASSES` and into `UNEXPLAINED_CLASSES`.
   It is a statistics bucket, not a documented placeholder - `pochi_filler` and
   `zero_sector_high_entropy` have format pages, `mostly_zeros` does not.
   Counting it as explained is what hid 5 MB of field maps. This *raises* the
   unexplained figure by 149504 bytes today, which is the honest number.
2. **`docs/formats/overlay-ptr-table.md`** (existing page, outside my scope):
   note that the pointer-table run is at offset 0 only for overlay *code*
   images; an overlay's *data* image can put a NUL-terminated string pool ahead
   of it (extraction 0970). A detector allowing a leading ASCII pool would claim
   0970 and empty `mostly_zeros` entirely - worth doing if a second example
   turns up, not worth a bespoke rule for one.
3. **`engine-core::field_regions`** already carries duplicate copies of the
   region offsets (`MAP_REGION_BLOCK_OFFSET`, `MAP_OBJECT_INDEX_OFFSET`,
   `MAP_OBJECT_DESCRIPTOR_STRIDE`, `MAP_TRIGGER_FALLBACK_OFFSET`). They agree
   with `legaia_asset::field_map`'s constants; engine-core depends on asset, so
   they could be re-exports instead of a second source of truth.

## Left open

**Extraction 1195** (348160 B, `unknown_high_entropy`) - the last unexplained
entry, 0.115% of the disc. Narrowed, not solved:

- Exactly **two** PROT entries share its head, `01 00 04 00 00 00 3C 01`: 1195
  and extraction **0888** (180224 B, currently `overlay_data_blob`). A
  two-member format family, not a one-off.
- Head shape is 8-byte records. The 7th byte walks `3C 3D 3E 3F 40 41 40` in
  1195 and pins at `3C`/`3D` in 0888 while byte 4 counts up - `0x3C` = 60 is
  the MIDI/SPU unity key, so "program / bank / centre-note" is the obvious
  reading. **Not asserted**: no loader is pinned, and
  [`sound-driver.md`](../docs/formats/sound-driver.md) explicitly leaves the
  `.spk` / `.MAP` / `.PCH` byte layouts TBD. This is that territory.
- Ruled out by measurement: **not** SPU-ADPCM (only 4.3% / 6.3% of 16-byte
  blocks have legal shift-filter + flag bytes), **not** the type-2-terminated
  streaming container (`FUN_8001FE70` shape - first chunk's declared size
  overruns), **not** `bse.dat` (which loads into a `0x1800`-byte buffer).
- Next step: find the loader. `FUN_8001FA88`'s retail branch loads raw TOC
  `0x37A` plus `param_1 + 5` for per-scene variants; walking that arithmetic to
  see whether it can reach extraction 888 / 1195 is the cheapest lead, and the
  `sound_data2` block's slot layout is the other.

Also open, deliberately: **no parser reports consumed-vs-unconsumed bytes**, so
92.1% is still format *recognition*, not byte accounting. `field_map` is the
cheapest place to start closing that - its four regions are fixed-size, so a
consumed-extent report for it is exact rather than estimated.

## Files touched

New: `crates/asset/src/field_map.rs`, `crates/asset/src/efect_pack.rs`,
`crates/asset/tests/field_map_real.rs`, `docs/formats/field-map.md`,
`handoff/lane-10.md`.

Modified: `crates/asset/src/categorize.rs` (4 classes + detector order + two
corrected docstrings), `crates/asset/src/lib.rs`,
`crates/asset/src/summon_readef.rs` (`detect`), `crates/asset/README.md`,
`docs/formats/overview.md`, `docs/tooling/disc-coverage.md`,
`crates/extract/tests/validation_suite.rs` (census pin - see above).

## Verification

- `cargo fmt -p legaia-asset` / `-p legaia-extract`;
  `cargo clippy -p legaia-asset --all-targets -- -D warnings` clean.
- `cargo test -p legaia-asset --release` - all pass, including 17 new unit tests.
- `cargo test -p legaia-extract --release --test validation_suite` - pass.
- `check-doc-density.py` and `check-md-links.py` both OK.
- Disc gating proven by **contrast**, not pass count:
  `cargo test -p legaia-asset --release --test field_map_real -- --nocapture`
  prints 3 x `[skip]` in 0.04s with `LEGAIA_DISC_BIN` unset, and 0 skips in
  6.32s with it set.
- `disc-coverage.py` was run against a scratch `extracted/` tree so the shared
  `extracted/PROT/categorize.json` in the main checkout was **not** overwritten
  mid-wave. Regenerate it with
  `asset categorize extracted/PROT` after merge; the coverage numbers above come
  from the regenerated file.
