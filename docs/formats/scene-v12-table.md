# scene_v12_table - the per-scene `.PCH` walk-on trigger sidecar

The scene's **walk-on tile-trigger patch file**: dev filename
`DATA\FIELD\<scene>.PCH` (suffix pool `0x8007B3BC/.MAP`, `0x8007B3C4/.PCH`,
`0x8007B3CC/.LZS` in `SCUS_942.54`). The first `0x800` bytes are a
four-kind sub-table directory + the kind-1 trigger records - the same
header shape as the `.MAP` file's `+0x10000` trigger block
([`field-locomotion.md` § trigger block](../subsystems/field-locomotion.md#trigger-block-0x10000---four-kind-sub-tables)) -
followed by a full
[scene event-scripts](scene-bundles.md#scene_event_scripts---prescript-only)
prescript at a sector-aligned offset.

Implementation: [`crates/asset/src/scene_v12_table.rs`](../../crates/asset/src/scene_v12_table.rs).
CLI: `asset scene-v12 <PROT-entry>` (single), `asset scene-v12-scan <dir>` (bulk).
97 PROT entries match. Position law: a scene with CDNAME `#define <scene> n`
carries its `.PCH` at **raw TOC index `n + 1`** = extraction entry `n − 1`
(defines are raw indices - see [`cdname.md` § numbering space](cdname.md#numbering-space);
disc-gated `scene_v12_position_law_and_lzs_sibling` in
`crates/asset/tests/scene_v12_corpus.rs`).
Scenes without one (`opurud`, `opkorout`, `edson` - op-`0x44`-driven cutscene
scenes with no trigger tiles) hit the loader's zero-fill branch below.

**Naming caveat.** The extraction *filename* labels apply defines as
extraction indices and are shifted +2, so per-file attributions inherited
from those labels are off by one block: `0093_map01.BIN` is **garmel**'s
table (raw `95 = 94 + 1`), Drake `map01`'s is `0084_suimon.BIN` (raw
`86 = 85 + 1`, and it alone carries `b2 = 0x26` = the `P2[38]` fly-in
record [`cutscene.md`](../subsystems/cutscene.md#record-spawn-mechanisms-live-probe-pinned)
pins to `map01`), and `town01`'s is `0002_gameover_data.BIN` (raw `4`,
carrying the opening trigger record `(0x1D, 0x5B, 0x03)` = P2[3] at the
documented arrival tile).

## On-disc layout

```text
+0x000   u16  N + 4              ; directory end-of-table offset
+0x002   u16  0x0012             ; kind-0 sub-table offset (empty in retail)
+0x004   u16  0x0000             ; kind-0 count
+0x006   u16  0x0014             ; kind-1 sub-table offset (= the records)
+0x008   u16  param              ; kind-1 count (0..=192 in retail)
+0x00A   u16  N                  ; kind-2 sub-table offset (empty)
+0x00C   u16  0x0000             ; kind-2 count
+0x00E   u16  N + 2              ; kind-3 sub-table offset (empty)
+0x010   u32  0                  ; kind-3 count + pad to 0x14
+0x014   param × 4 bytes         ; kind-1 trigger records
+end_records (= 0x14 + 4*param)  ; empty kind-2/3 sub-table bodies at
                                 ; +N (= +end_records+2), +N+2, +N+4.
                                 ; Zero on disc (see Open questions on
                                 ; the old "runtime fixup" reading).
+end_records .. 0x800            ; zero padding
+0x800   u16  script_count       ; scene event-scripts prescript
+0x802   script_count × u16      ;   offset table (relative to +0x800)
+0x800 + offsets[i]              ;   per-record word-aligned command bytes
                                 ;   (records typically open with the
                                 ;   `0xFFFF 0x0000` header sentinel; NOT
                                 ;   field-VM bytecode - see below).
```

## Confidence

**Confirmed** - header algebra, inline-record shape, and prescript-at-0x800
layout verified across all 97 corpus entries by the disc-gated
`scene_v12_corpus` test.

The inline-record semantics is also **confirmed**: the records are kind-1
walk-on tile triggers `[tile_x][tile_z][p2_record][gate]`, read by the same
consumers as the `.MAP` `+0x10000` trigger block (see
[Runtime staging](#runtime-staging---the-pch-sidecar) below). The earlier
"`b0` = scene-local resource index" reading is superseded.

## Header algebra

The three u16 fields at `u16[0]`, `u16[5]`, `u16[7]` are not random - they
sit in the tightest algebraic family the corpus exhibits:

| Field      | Value                  |
|------------|------------------------|
| `u16[0]`   | `N + 4`                |
| `u16[5]`   | `N`                    |
| `u16[7]`   | `N + 2`                |
| `N`        | `4 * param + 22`       |

`N` is the byte distance from the start of the file to the **first runtime
fixup slot**, which sits immediately past the inline records:
`N = (0x14 + 4*param) + 2 = 4*param + 22`.

**The header is a four-kind sub-table directory** - the same shape the
per-tile lookup helper reads for the `.MAP` `+0x10000` trigger block: for
kind `k`, the sub-table body offset is the `s16` at `+4k+2` and its record
count the `s16` at `+4k+4` (overlay 0897 `FUN_801D5AE0`, called by the
two-window wrapper `FUN_801D5630`). Under that reading the algebra
dissolves:

| Kind | Offset field | Count field | Retail `.PCH` value |
|---|---|---|---|
| 0 | `u16[1] = 0x0012` | `u16[2] = 0` | empty (teleports live in the `.MAP` block) |
| 1 | `u16[3] = 0x0014` | `u16[4] = param` | **the inline trigger records** |
| 2 | `u16[5] = N` | `u16[6] = 0` | empty (elevation overrides) |
| 3 | `u16[7] = N + 2` | `u16[8] = 0` | empty (region AABBs) |

`u16[0] = N + 4` is the directory's end-of-table offset. Every retail
`.PCH` populates **kind 1 only**; the empty kinds' offsets pack
consecutively past the records, which is exactly the `N = 4*param + 22`
tie the detector checks. The zero "fixup slots" at `+N`, `+N+2`, `+N+4`
are the empty kind-2/3 sub-table bodies.

## Inline records at `+0x14` - kind-1 walk-on tile triggers

`param` records, each 4 bytes, in the trigger-block kind-1 form:

| Byte | Field | Notes |
|------|-------|-------|
| `+0` | `tile_x` (`b0`) | Trigger tile column (128-unit field tiles). |
| `+1` | `tile_z` (`b1`) | Trigger tile row. |
| `+2` | `p2_record` (`b2`) | MAN **partition-2 record index** spawned on step-on. |
| `+3` | `gate` (`flag`) | Always `0x01` across all 97 entries = gate-1 "spawn on walk-on" (the `.MAP` block's gate-0 object-bind class never appears in a `.PCH`). |

Records sharing a `b2` are **multi-tile strips of one trigger** - adjacent
tiles that all fire the same partition-2 record (a gate several tiles wide
gets one record per tile). The record's own C1/C2 story-flag gates still
apply at spawn time (`FUN_8003BDE0` vs `DAT_80085758` - see
[`cutscene.md`](../subsystems/cutscene.md#record-spawn-mechanisms-live-probe-pinned)).

Concrete shape for `0093_map01.BIN` (**garmel**'s table under the position
law - the filename label is the +2-shifted naive attribution, see the
naming caveat above; `param=12`):

```
[0] x=15 z=08 p2=02  ┐
[1] x=14 z=08 p2=02  │ one trigger spanning 3 adjacent tiles
[2] x=13 z=08 p2=02  ┘
[3] x=17 z=2A p2=0C
[4] x=17 z=68 p2=0B  ┐
[5] x=17 z=69 p2=0B  │ 3-tile strip
[6] x=17 z=6A p2=0B  ┘
[7] x=14 z=09 p2=0A
[8] x=06 z=5F p2=09
[9] x=14 z=5E p2=08
[10] x=77 z=12 p2=01
[11] x=72 z=3E p2=00
```

The earlier "maps to actor placements only on world-map kingdom scenes"
reading is superseded: `(b0, b1)` are tile coordinates on every scene
class; on kingdom overworlds the referenced P2 records happen to be the
town/dungeon-entrance and story-beat records, which produced the
placement correlation.

## Event-script prescript at `+0x800`

Identical shape to the standalone [scene_event_scripts](scene-bundles.md#scene_event_scripts---prescript-only)
format: a `[u16 count][u16 offsets[count]]` table indexing **move-VM
(`FUN_80023070`) records in the summon-stager format** (`[i16 model_sel][u16 flags][move-VM bytecode]`)
- **not** field-VM (`FUN_801DE840`) bytecode (it disassembles as field-VM with a
65–88 % error rate). The per-record `0xFFFF 0x0000` lead is `model_sel = -1`
(a transform/pivot node) + `flags = 0`, and the `0x0008` terminator is move-VM
opcode `0x08` (Halt). The field VM installs a record by id via `FUN_800252EC`
(→ part-stager `FUN_80021B04` → move VM); see the
[scene_event_scripts](scene-bundles.md#scene_event_scripts---prescript-only)
section for the full chain. The genuine per-scene field-VM *scripts* live in the
scene MAN sub-asset (see [`subsystems/script-vm.md`](../subsystems/script-vm.md));
this prescript is the move-VM *stager* table they spawn from.

Across the 97 v12 entries:

| Metric | Value |
|--------|-------|
| Valid prescript at `+0x800` | **97 / 97** |
| `script_count` range | 2 .. 71 |
| Frame-opener rate ≥ 50 % | 75 / 97 |
| Max records per entry | 71 (`0119_keikoku.BIN`, `0154_retock.BIN`) |

The 22 entries with frame-opener rate below 50 % carry "init"-style first
records that open differently, then transition into the standard
header-sentinel stream. Those entries carry the same word-aligned command
structure; the first record is just shaped differently (record 0 on the town
scenes is a fixed 768-byte master ambient stager - the record the entry
effect-actor installs; see
[scene-bundles](scene-bundles.md)).

## Runtime staging - the `.PCH` sidecar

The field-asset loader `FUN_8001F7C0` (called per scene entry from the
mode-2 initializer `FUN_801D6704`) stages the file **statically** - no
capture needed (`see ghidra/scripts/funcs/8001f7c0.txt`). Retail branch,
in order:

1. `DATA\FIELD\<scene>.MAP` → the per-scene buffer `*(0x1F8003EC)`
   (collision grid `+0x4000`, event-cell grid `+0x8000`, trigger block
   `+0x10000`).
2. `DATA\FIELD\<scene>.PCH` → **`*(0x1F8003EC) + 0x12000`**. If the open
   (`FUN_800608F0`) misses, the loader **zero-fills `0x800` bytes** there
   instead - the empty-directory fallback for trigger-less scenes.
3. `h:\PROT\FIELD\<scene>\efect.dat` → `*(0x1F8003EC) + 0x12800`
   (`= _DAT_8007B8D0`), which **overwrites every `.PCH` byte past the
   first `0x800`** - so the runtime-live window is exactly the directory +
   kind-1 records; the on-disc prescript at `+0x800` is not reachable
   through this path.

This closes the format's long-open "where does the loader stage the file"
question and matches the live capture that found the table at heap
`0x8014B530` (= the `town01` scene buffer `0x80139530 + 0x12000`).

Consumers of the staged window:

- **Scene init** - `FUN_8003AEB0` (the field/town map-init, body
  `0x8003AFA8..0x8003B018`) walks the kind-1 records (`count` at
  `+0x12008`, cursor from `+0x12006`) and ORs the footprint bit `0x400`
  into the u16 event-cell word at
  `*(0x1F8003EC) + 0x8000 + (tile_z << 8) + (tile_x << 1)`.
- **Per step** - the tile lookup `FUN_801D5630` (overlay 0897) scans the
  `.MAP`'s `+0x10000` block first and **falls back to the `+0x12000`
  `.PCH` window** (helper `FUN_801D5AE0`, same directory shape); a kind-1
  hit reaches `FUN_8003BDE0(x, z, rec[2], rec[3])` and spawns the
  partition-2 record. Full runtime contract:
  [`field-locomotion.md` § trigger block](../subsystems/field-locomotion.md#trigger-block-0x10000---four-kind-sub-tables).

So the `.PCH` is a **patch/extension layer over the `.MAP` trigger block**:
same directory, same record forms, second lookup window.

## The `~0x800219xx` lead resolved - `FUN_80021934` stages the `.LZS`, not the `.PCH`

The formerly un-analyzed `_DAT_8007B85C` reader near `~0x800219xx` is
**`FUN_80021934`** (real entry 3 instructions before the `0x80021940`
prologue; `see ghidra/scripts/funcs/80021940.txt`): the **scene-transition
streaming actor**, a 5-state SM (state at `actor+0x1A`, jump table
`0x80010760`) that pre-streams the *next* scene's
[`scene_asset_table`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
bundle during the transition fade:

- It is **not** a game-mode handler: its only corpus reference is the
  handler word of the 24-byte spawn descriptor at `0x80070734` (the
  system-actor descriptor family at `0x800705FC..0x80070763`, just below
  the mode table at `0x8007078C` - phase-misaligned with it, layout
  `[+4 0xFFFF0000][+8 handler][+0xC flags]`). `FUN_8001FD44` (the named
  scene-change packet) spawns it via the pool spawner `FUN_80020DE0`
  (`actor+0x0C` = handler, `actor+0x1A` zeroed;
  `see ghidra/scripts/funcs/8001fd44.txt`, `80020de0.txt`); the five
  `FUN_8001FD44` call sites all live in the field overlay 0897 (field-VM
  op `0x3F` at `0x801DEB14` plus four controller sites).
- **Case 0** seeds a 70-frame countdown (`gp+0x710 = 0x46`); cases 1/3
  poll stream progress (`FUN_8003DE7C(1)`).
- **Case 2** (`_DAT_8007B8C2` set - the retail path, since retail boots the flag
  at `1`): streams **raw TOC entry
  `DAT_8007B768 + 3`** - the destination scene's block base + 3, the
  `.LZS`/`scene_asset_table` slot - into `_DAT_8007B85C` by index
  (`FUN_8001EEF0` → `FUN_8003EB98`; `see ghidra/scripts/funcs/8001eef0.txt`,
  `8003eb98.txt`).
- **Case 4** (retail): builds the literal path `DATA_FIELD\<scene>.LZS`
  (suffix `0x8007B3CC`) and streams it into `_DAT_8007B85C` by name, then
  hands off with `_DAT_8007B83C = 2` (mode 2 MAIN INIT, whose
  `FUN_801D6704` → `FUN_8001F7C0` chain then stages `.MAP`/`.PCH`/efect
  as above).

So the raw scene block layout is `n+0` `.MAP` (`0x12000` footprint),
`n+1` `.PCH` (this format), `n+2` event-scripts sister, `n+3` `.LZS`
bundle head, `n+6` BGM base - and the transition actor touches only
`n+3`. The v12 record-table staging + consumer chain is the `.PCH` path
above; `_DAT_8007B85C` never holds this file.

## Detection

The strict gate combines six checks:

1. `buf.len() >= 16`.
2. `u16[1] == 0x0012`, `u16[2] == 0`, `u16[3] == 0x0014`, `u16[6] == 0`.
3. `u16[0] == u16[5] + 4` (= `N + 4`).
4. `u16[7] == u16[5] + 2` (= `N + 2`).
5. `0 <= param <= 1024` (corpus tops out at `param = 192`; `0724_noaru.BIN`
   is the `param = 0` edge case).
6. `N == 4 * param + 22` (= the runtime-fixup slot algebra).

The algebraic tie at step 6 is the tightest constraint: across the entire
1233-entry PROT corpus it matches **97** entries with zero false positives.
Steps 1–5 alone would already match the same set, but the explicit `N/param`
check is a strong contract for code that consumes the parser output and
relies on `end_records = N - 2`.

## Sister formats

Every scene block carries a sister `scene_event_scripts` entry (prescript
at offset 0, no directory header) at raw `n + 2` - **directly after** the
`.PCH` at raw `n + 1`:

```
raw n+1  (extraction n−1)  scene_v12_table / .PCH (this format)  ┐ same
raw n+2  (extraction n)    scene_event_scripts (no header)       ┘ scene
```

For Drake `map01` (`n = 85`) that is extraction `0084` + `0085`; the
historical pairing of `0085` with `0093` crossed a block boundary
(`0093` is garmel's `.PCH` - see the naming caveat above).

The two prescript tables likely serve different scopes (scene-enter
triggers vs. per-actor / per-region triggers), or they're "early-load"
and "late-load" splits of one logical script set. The exact runtime split
isn't pinned down; note the `.PCH`-resident copy at `+0x800` is dead via
the retail staging path (efect.dat overwrites it - see
[Runtime staging](#runtime-staging---the-pch-sidecar)).

## Reading the parsed structure

```rust
use legaia_asset::scene_v12_table;

let buf = std::fs::read("extracted/PROT/0093_map01.BIN")?;
let t = scene_v12_table::detect(&buf).expect("v12 header valid");

println!("N={}, param={}", t.n, t.param);
for (i, rec) in t.records.iter().enumerate() {
    println!("rec[{i}]: b0={:02x} b1={:02x} b2={:02x}",
             rec.b0, rec.b1, rec.b2);
}
for (i, s) in t.scripts.iter().enumerate() {
    let bytecode = t.script_payload(&buf, i).unwrap();
    println!("script[{i}] @{:#x} len={} opener={}",
             s.start, s.len(), s.frame_opener);
}
```

## The "embedded MAN at `0x1000`" is an extended-footprint over-read

Extraction entries `0076` and `0164` show a canonical 7-asset
[`scene_asset_table`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
at file offset `0x1000`, which was read as the v12 "embedding" its scene's
bundle. Byte comparison falsifies the embedding: `0076 + 0x1000` onward is
**byte-identical to extraction `0078`** (suimon's ordinary base+3 bundle)
and `0164 + 0x1000` to extraction `0166` (geremi's) - the extraction slice
of the `.PCH` slot simply **over-reads into the following TOC entries**
(the same extended-window trap as the historical "16 MB container at
0865"). Under the position law those two windows are suimon's and
geremi's `.PCH` files, not dolk2's / rikuroa's.

What stays true: `dolk2` and `rikuroa` are the two scenes whose **own**
base+3 bundle is the MAN-less `count=4` form (types `[1, 2, 6, 0x14]`;
the type-`0x14` slot is a small LZS filler, not a MAN carrier). Where
retail sources their partition scripts is now **closed**: each block
ships a standalone `data_field_streaming` entry whose type-3 chunk is a
plain MAN (`dolk2` extraction 70, partitions `[29, 73, 17]`; `rikuroa`
extraction 157, `[13, 29, 64]`), and the live script heap at the Mt.
Rikuroa Caruban beat byte-matches the `0157` chunk - the streaming
carrier IS the resident MAN (see
[script-vm.md](../subsystems/script-vm.md#a-second-script-byte-carrier-the-streaming-variant-man)).
The engine resolves it via the `field_man_payload` streaming fallback
(`legaia_engine_core::scene_bundle::streaming_man_payloads`, disc-gated
`crates/engine-core/tests/v12_bundle_man_disc.rs`); the earlier
`V12Embedded { table_offset: 0x1000 }` fallback resolved the over-read
windows (suimon's / geremi's bundles) under the CDNAME-shifted scene
windows and is superseded for these two scenes.

## Open questions

The two long-standing opens are **closed statically**: the loader stages
the file at `*(0x1F8003EC) + 0x12000` (`FUN_8001F7C0` `.PCH` load, zero-fill
when absent), and `b0`/`b1` are trigger tile coordinates consumed by
`FUN_8003AEB0` (footprint-bit `0x400` marking) and the `FUN_801D5630`
fallback lookup - see [Runtime staging](#runtime-staging---the-pch-sidecar).
The `~0x800219xx` / `_DAT_8007B85C` lead resolved to the *sibling* `.LZS`
transition streamer `FUN_80021934`, not this file. Earlier falsified leads,
kept so they aren't re-walked: the `_DAT_8007B8D0` relocation via
`FUN_800252EC` is the `efect.dat` prescript stager; the `FUN_8001F05C`
dispatch hypothesis fails because the `.PCH` is a standalone top-level PROT
entry, never a `type << 24` chunk; `FUN_8002541C` is a generic 3-mode
streaming driver.

Still open:

- **The `+N` "fixup slot" writes.** Under the directory reading the zero
  words at `+N`, `+N+2`, `+N+4` are the empty kind-2/3 sub-table bodies;
  whether any runtime writer fills them (the old "loader writes computed
  pointers" observation) needs a targeted re-capture of the `+0x12000`
  window.
- **Empty kinds 0/2/3.** No retail `.PCH` populates teleports, elevation
  overrides, or region AABBs (those live only in the `.MAP` `+0x10000`
  block). Whether the engine-side patch mechanism was ever used for them
  is a dev-history question, not a runtime one.
- ~~dolk2 / rikuroa MAN source~~ - closed: the standalone
  `data_field_streaming` sibling's type-3 chunk is the scene's MAN
  (live byte-match at the Caruban beat; see the over-read section
  above for what the `.PCH` windows are instead).
- **Two prescript tables per scene** - the sister offset-0
  `scene_event_scripts` entry (raw `n + 2`) and this file's offset-`0x800`
  copy carry the same move-VM stager records, both consumed via
  `FUN_800252EC` → `FUN_80021B04`
  (see [scene_event_scripts](scene-bundles.md#scene_event_scripts---prescript-only)).
  The runtime split is unpinned; the `.PCH` copy is unreachable via the
  retail staging path (efect.dat overwrites `+0x800..`), pointing at the
  sister entry / the `.LZS` scripted-table prefix as the live copies.

## Related

- [Scene bundles overview](scene-bundles.md) - the four other scene-prefixed
  asset layouts.
- [Field/event script VM](../subsystems/script-vm.md) - `FUN_801DE840`,
  the runtime that executes the spawned partition-2 records.
- [Field locomotion](../subsystems/field-locomotion.md#trigger-block-0x10000---four-kind-sub-tables) -
  the `.MAP` `+0x10000` trigger block this file extends, and the engine's
  `field_regions` port of the lookup.
- [Cutscene routing](../subsystems/cutscene.md#record-spawn-mechanisms-live-probe-pinned) -
  the walk-on trigger → `FUN_8003BDE0` record-spawn chain (the `map01` /
  `town01` opening legs fire from these records).
