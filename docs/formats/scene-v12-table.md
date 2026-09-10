# scene_v12_table - the per-scene `.PCH` walk-on trigger sidecar

The scene's **walk-on tile-trigger patch file**: dev filename
`DATA\FIELD\<scene>.PCH` (suffix pool `0x8007B3BC/.MAP`, `0x8007B3C4/.PCH`,
`0x8007B3CC/.LZS` in `SCUS_942.54`). The entry is a four-kind sub-table
directory + the kind-1 trigger records - the same header shape as the `.MAP`
file's `+0x10000` trigger block
([`field-locomotion.md` § trigger block](../subsystems/field-locomotion.md#trigger-block-0x10000---four-kind-sub-tables)).

**The entry is exactly one `0x800`-byte sector, in all 97 cases.** The
[scene event-scripts](scene-bundles.md#scene_event_scripts---prescript-only)
prescript is the **next PROT entry**, not a field at `+0x800` of this one -
`0x800` *is* this entry's size. See
[Event-script prescript](#event-script-prescript---the-next-prot-entry).

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
+end_records .. 0x800            ; zero padding; the entry ENDS at 0x800

-- NEXT PROT ENTRY (extraction `define`) --------------------------------
+0x000   u16  script_count       ; scene event-scripts prescript
+0x002   script_count × u16      ;   offset table (relative to entry start)
+offsets[i]                      ;   per-record word-aligned command bytes
                                 ;   (records typically open with the
                                 ;   `0xFFFF 0x0000` header sentinel; NOT
                                 ;   field-VM bytecode - see below).
```

## Confidence

**Confirmed** - header algebra, inline-record shape, one-sector entry size,
and the prescript-is-the-next-entry structure verified across all 97 corpus
entries by the disc-gated `scene_v12_corpus` test.

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

## Event-script prescript - the next PROT entry

A v12 header entry is **2048 bytes - one sector - in all 97 cases**, so
`0x800` is one past its end, not an offset inside it. The prescript is the
next TOC row: extraction `define` (the header is `define - 1`, the `.LZS`
`scene_asset_table` bundle `define + 1`), which is the position law asserted
lower down this page.

The earlier "prescript at `+0x800` of the same entry" reading came from the
superseded over-reading PROT entry size, which appended following entries to
every buffer and put the neighbour at exactly `+0x800`. Retail's own loader
had already said otherwise and it was read as a puzzle instead of an answer -
see [Runtime staging](#runtime-staging---the-pch-sidecar): a missing `.PCH`
is **zero-filled `0x800` bytes**, and `efect.dat` stages at `+0x12800`,
`0x800` past the `.PCH`. Both are statements that the `.PCH` is one sector.
Parser: `legaia_asset::scene_v12_table::parse_prescript_entry`.

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
| Valid prescript in the next PROT entry | **97 / 97** |
| Header entry size | **0x800 (one sector), 97 / 97** |
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
capture needed (`see ghidra/scripts/funcs/8001f7c0.txt`). It forks on the
dev/retail selector `_DAT_8007B8C2` ([`cdname.md`](cdname.md#the-table-is-populated-on-retail-hardware)),
and the two arms reach the same window by different means.

**Retail arm** (`_DAT_8007B868 == 0`, `_DAT_8007B8C2 != 0`; the `bne` at
`0x8001F87C` jumps to `0x8001F9A4`) - **one** contiguous read, no per-file
opens at all:

1. `FUN_8003E8A8(record, 1)` resolves the scene's `.MAP` PROT entry.
2. `FUN_8003E800(dest, 0x28, 1)` reads **40 sectors = `0x14000` bytes** into
   the per-scene buffer `*(0x1F8003EC)`, and the loader returns that
   `0x14000` verbatim (`lui s1,0x1; ori s1,s1,0x4000` at `0x8001F9BC`).

The `.MAP` is `0x12000` bytes ([`field-map.md`](field-map.md)), i.e. 36
sectors, so the read runs **four sectors past it** - and PROT entries are
contiguous, so those four are the entries that follow. The `.PCH` is block
slot 1, one sector, and it lands at `+0x12000` because it is next on the
disc. `_DAT_8007B8D0` is pointed at `+0x12800` (`0x8001F864`, computed
before the fork) and is therefore the *next* entry after the `.PCH` - the
scene's event-script prescript, staged by the same read.

**Dev arm** (`_DAT_8007B8C2 == 0`, or `_DAT_8007B868 != 0`) - three separate
host-station opens, which is where the per-file names live:

1. `DATA\FIELD\<scene>.MAP` → `*(0x1F8003EC)` via `FUN_8003E6BC`.
2. `DATA\FIELD\<scene>.PCH` → `*(0x1F8003EC) + 0x12000`. If the open
   (`FUN_800608F0`, the `break 0x103` host trap) misses, this arm
   **zero-fills `0x800` bytes** there instead (`FUN_8001A6A4`).
3. `h:\PROT\FIELD\<scene>\efect.dat` → `*(0x1F8003EC) + 0x12800`.

An earlier revision of this page attributed the whole three-step list to the
**retail** branch. It is the dev branch: `FUN_800608F0` is the debug-station
file trap retail hardware cannot service, and step 3's path is literally
`h:\`. What survives the correction is the destination - the `.PCH` really
is at `+0x12000` and really is one sector - and the `+0x12800` pointer,
which is computed on both arms. What does **not** survive is the zero-fill:
on retail the `+0x12000` window is simply the next four sectors of the same
read, so a scene whose block has no `.PCH` at slot 1 gets whatever those
sectors hold rather than zeros.

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

### Who writes the staged window

The whole write surface is three routines wide, and a byte-level sweep of
`SCUS_942.54` plus all 83 mapped overlay images bounds it:

- **The `+0x12000` offset itself is materialised six times** in those 84
  images (`lui r,0x1; ori r,r,0x2000`): twice in the loader's dev arm
  (`0x8001F8F4` host read, `0x8001F920` zero-fill), once in `FUN_8003AEB0`
  (`0x8003AFA8`), and three times in the two-window lookup - `FUN_801D5630`
  at `0x801D568C` plus that routine's private copies inside the fishing
  (`0x801D617C`) and dance (`0x801D3F1C`) overlays. `FUN_8003AEB0` only
  reads the directory; its stores all land in the `.MAP`'s `+0x8000`
  event-cell grid.
- **`FUN_801D5AE0` is reached from nowhere but `FUN_801D5630`'s two arms**
  (`0x801D567C`, `0x801D56A0`), so the returned record pointer is the only
  other handle on the window.
- **`FUN_801D5630` has seven callers**, and exactly one of them writes
  through the pointer it returns.

### The one runtime writer - field-VM `0x4C 0x83`

`0x801E20A8` in the field overlay is a field-VM arm, reached as main opcode
`0x4C` (`MENU_CTRL`; main JT `0x801CECC0` indexed `opcode - 0x21`, slot at
`0x801CED6C`) → operand high nibble `8` (JT `0x801CEE60`) → low nibble `3`
(JT `0x801CEF48`). It walks a tile rectangle, `x` from `op[1]` to `op[3]`
and `z` from `op[2]` to `op[4]` inclusive, calls `FUN_801D5630(2, x, z)`
for every tile, and on a hit writes

```text
801e20f4  sb zero,0x3(v1)     ; quads  = 0
801e20f8  sb v0,0x2(v1)       ; coarse = op[5]
```

into the matched **kind-2 elevation-override** record, then advances the VM
PC by 7 (`0x801E2130`). A tile with no kind-2 record is skipped by the
`beq v1,zero` guard. Sweeping all 84 images for that store pair
(`sb zero,0x3(rX)` immediately followed by `sb ?,0x2(rX)`) returns four
hits and this is the only one on a lookup return.

That settles both standing questions on this page.

- **The `+N` / `+N+2` / `+N+4` words are never filled at runtime.** The
  writer above rewrites the *body* of a record the directory already
  counts; it cannot append one, because the record it patches is the one the
  lookup found. Nothing anywhere stores a count or an offset into the
  directory. The words stay whatever the file carries, which on the retail
  disc is zero in all 97 entries.
- **The empty kinds split two ways.** Kind 2 has an engine-side writer -
  the arm above, which is how a script re-floors a ramp or a bridge mid-scene
  - but on retail it can only ever land in the `.MAP` `+0x10000` block,
  because every retail `.PCH` ships `kind-2 count = 0` and an empty
  sub-table has nothing to match. Kinds 0 and 3 have no writer in any image.
  So the patch mechanism exists for exactly one kind and the `.PCH` never
  carries a record it could reach.

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

That sister entry is the scene's **only** prescript - there is no second copy
to split a runtime role with. The "copy at this file's `+0x800`" was the
over-reading entry size showing the sister entry through the `.PCH`'s buffer;
the `.PCH` ends at `0x800`. The record installer confirms it directly:
`FUN_800252EC` takes its `[count][offsets]` table from `_DAT_8007B8D0`
(`lw a3, -0x4730(a3)` at `0x800252F4`, then `lhu v0, 0x2(a0)` for
`offsets[id]` and `addu a2, a3, v1` for the record pointer), and the field
asset loader sets that global to `*(0x1F8003EC) + 0x12800` - the `efect.dat`
window, one sector *past* the `.PCH` - with `sw v0, -0x4730(at)` at
`0x8001F864`. Since the offsets are non-negative, the `.PCH` window at
`+0x12000` can never be the source. `_DAT_8007B8D0` has one other SCUS writer,
`0x8001FAC8`, which points it at the `0x1800`-byte `bse.dat` bank buffer
([`bse-dat.md`](bse-dat.md)) - the battle occupant of the same slot, not a
second field prescript.

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

Both remaining opens are **closed from the bytes**, in
[Who writes the staged window](#who-writes-the-staged-window):

- ~~**The `+N` "fixup slot" writes**~~ - closed: nothing writes them. The
  window's whole write surface is the loader's own read/zero-fill plus one
  field-VM arm, and that arm patches a record's body, never the directory.
  The old "loader writes computed pointers" observation has no instruction
  behind it.
- ~~**Empty kinds 0/2/3**~~ - closed: kind 2 *does* have an engine-side
  writer (field-VM `0x4C 0x83`, [above](#the-one-runtime-writer---field-vm-0x4c-0x83)),
  kinds 0 and 3 have none in any image, and none of the three can add a
  record to an empty sub-table - so on retail the `.PCH`'s empty kinds stay
  empty by construction rather than by convention.
- ~~dolk2 / rikuroa MAN source~~ - closed: the standalone
  `data_field_streaming` sibling's type-3 chunk is the scene's MAN
  (live byte-match at the Caruban beat; see the over-read section
  above for what the `.PCH` windows are instead).
- ~~Two prescript tables per scene~~ - closed: there is one. The "copy at this
  file's `+0x800`" was the over-read showing the sister `scene_event_scripts`
  entry (raw `n + 2`) through the `.PCH`'s buffer, and the installer picks that
  sister entry by construction - `FUN_800252EC` reads its table from
  `_DAT_8007B8D0` (`0x800252F4`), which the field asset loader points at the
  `efect.dat` window `*(0x1F8003EC) + 0x12800` at `0x8001F864`, one sector past
  the `.PCH`. See [Sister formats](#sister-formats) and
  [scene_event_scripts](scene-bundles.md#scene_event_scripts---prescript-only).

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
