# scene_v12_table - scene header + event-script bundle

A per-scene container that bundles a small "runtime fixup" header with a full
[scene event-scripts](scene-bundles.md#scene_event_scripts---prescript-only)
prescript at a sector-aligned offset.

Implementation: [`crates/asset/src/scene_v12_table.rs`](../../crates/asset/src/scene_v12_table.rs).
CLI: `asset scene-v12 <PROT-entry>` (single), `asset scene-v12-scan <dir>` (bulk).
97 PROT entries match - one per game scene.

## On-disc layout

```text
+0x000   u16  N + 4              ; runtime fixup-slot offset; header field
+0x002   u16  0x0012             ; constant magic
+0x004   u16  0x0000             ; constant
+0x006   u16  0x0014             ; constant magic (= byte offset of records)
+0x008   u16  param              ; count of inline records (0..=192 in retail)
+0x00A   u16  N                  ; runtime fixup-slot offset; header field
+0x00C   u16  0x0000             ; constant
+0x00E   u16  N + 2              ; runtime fixup-slot offset; header field
+0x010   u32  0                  ; padding to 0x14
+0x014   param × 4 bytes         ; inline record table
+end_records (= 0x14 + 4*param)  ; runtime fills three fixup pointers
                                 ; immediately past here, at offsets
                                 ; +N (= +end_records+2), +N+2, +N+4.
                                 ; These bytes are zero on disc.
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

The semantics of the inline records (`b0`, `b1`, `b2` bytes) is **inferred**
- grouping by `b2` correlates with scene region kind, but the exact lookup
the runtime performs hasn't been pinned to a specific function.

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
`N = (0x14 + 4*param) + 2 = 4*param + 22`. The slots at `+N`, `+N+2`,
`+N+4` are zero on disc; the loader writes computed pointers into them at
scene init. The three u16 fields at the header front therefore double as
slot-offset hints for the loader (`"write the first pointer at +N+4"`,
etc.) and as a strict validation signature.

The constants `0x0012` at `u16[1]` and `0x0014` at `u16[3]` are stable
across the whole corpus; `u16[3]` also happens to equal the byte offset
of the inline records table (`+0x14`), which is unlikely to be a
coincidence - the loader probably re-reads it as the records pointer.

## Inline records at `+0x14`

`param` records, each 4 bytes:

| Byte | Field | Notes |
|------|-------|-------|
| `+0` | `b0`  | Scene-local identifier (sub-index / region-id). |
| `+1` | `b1`  | Scene-local identifier (region-id / target-id). |
| `+2` | `b2`  | Categorises records into 1..N groups within the scene. |
| `+3` | `flag` | Always `0x01` across all 97 entries - probably "live" bit. |

`b2` partitions records into per-scene groups. Drake (`map01`) has 8 distinct
`b2` values across 12 records (one group of 3, one of 3, then singletons);
Karisto (`map03`) groups 12 of its 23 records under a single
`(b1=0x2F, b2=0x05)` triple, plus several smaller groups. This "many records
share a `b2`, a few singletons" pattern matches a scene-region
transition table: rooms / sub-areas of the scene each get a `b2` group, and
sub-records inside each group correspond to interactive objects, NPCs, or
exits.

Concrete shape for `0093_map01.BIN` (Drake's kingdom map, `param=12`):

```
[0] b0=15 b1=08 b2=02  ┐
[1] b0=14 b1=08 b2=02  │ group b2=0x02, 3 records
[2] b0=13 b1=08 b2=02  ┘
[3] b0=17 b1=2A b2=0C
[4] b0=17 b1=68 b2=0B  ┐
[5] b0=17 b1=69 b2=0B  │ group b2=0x0B, 3 records
[6] b0=17 b1=6A b2=0B  ┘
[7] b0=14 b1=09 b2=0A
[8] b0=06 b1=5F b2=09
[9] b0=14 b1=5E b2=08
[10] b0=77 b1=12 b2=01
[11] b0=72 b1=3E b2=00
```

The full semantic decoding of the `(b0, b1)` pair depends on the consumer.
It maps to scene-actor placements **only on world-map kingdom scenes**;
on towns and dungeons the pair selects different runtime resources.

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
1234-entry PROT corpus it matches **97** entries with zero false positives.
Steps 1–5 alone would already match the same set, but the explicit `N/param`
check is a strong contract for code that consumes the parser output and
relies on `end_records = N - 2`.

## Sister formats

The v12 file is the **second** scene-event-scripts table in each scene
block. Every scene block also has a sister `scene_event_scripts` entry
(prescript at offset 0, no v12 header):

```
PROT 0085_map01  scene_event_scripts (no v12 header)   ┐ Drake
PROT 0093_map01  scene_v12_table (this format)         ┘
PROT 0244_map02  scene_event_scripts                   ┐ Sebucus
PROT 0253_map02  scene_v12_table                       ┘
…
```

The two scripts likely serve different scopes (scene-enter triggers vs.
per-actor / per-region triggers), or they're "early-load" and "late-load"
splits of a single logical script set. The exact runtime split isn't
pinned down yet; both are walked by the same field VM.

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

## Open questions

- **Where does the loader stage the file?** The `FUN_8001F7C0` /
  `FUN_800255B8` field-loader chain (see
  [`subsystems/asset-loader.md`](../subsystems/asset-loader.md)) is the
  canonical scene-asset path, but the static call graph for v12 entries
  hasn't been pinned. Earlier overlay captures put the file at RAM
  `0x8014B530` for one scene; that address is heap-allocated and varies
  per load.
- **What does `b0` index into?** For Drake the `b0` values fit inside the
  scene's TMD pack count (40 slots), but for other scenes they exceed it,
  ruling out "global TMD-slot index". They probably index a scene-local
  resource table the loader builds from the v12 header.
- **Two prescript tables per scene** - the sister offset-0 `scene_event_scripts`
  entry and this offset-0x800 table - carry the same move-VM stager records. Both
  are consumed by the move VM via `FUN_800252EC` → `FUN_80021B04`
  (see [scene_event_scripts](scene-bundles.md#scene_event_scripts---prescript-only)).
  The exact per-record decode follows the move VM's control flow (a linear
  disassembly desyncs at its jump ops `0x18/0x19/0x1A/0x1B`).

## Related

- [Scene bundles overview](scene-bundles.md) - the four other scene-prefixed
  asset layouts.
- [Field/event script VM](../subsystems/script-vm.md) - `FUN_801DE840`,
  the runtime that walks the prescript records.
- [World-map subsystem](../subsystems/world-map.md) - the kingdom-map
  renderer that consumes parts of the v12 inline-record table for actor
  placements.
