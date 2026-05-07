# Field-pack format

Magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. Four PROT entries match the signature today. Detector + dispatch: `crates/asset/src/field_pack.rs`.

## Layout

```
[preamble — variable size, content shape unknown]
[u32 LE = 0x01059B84]                  <- MAGIC
[97 × u32 LE — schema table, 388 bytes — byte-identical across all field-packs]
[asset region — packed TIMs / TMDs, in some files]
```

The schema slot offsets cover `[0x60..0x16651]` (≈ 91 KB of logical layout). They are anchored on `slots[0] == 0x60` and `slots[96] == 0x16651` and are byte-identical across every field-pack file (MD5 `edcfdf1575889d63d2077c396089d7f3`). The schema is therefore a STATIC abstract layout, not per-file metadata.

## What the four entries look like

| PROT | preamble | schema | asset region | TIMs / TMDs in tim_scan / tmd_scan |
|---|---|---|---|---|
| `0002_gameover_data` | 234 KB | 388 B | 5.7 KB | 2 TIMs + several TMDs |
| `0003_town01` | 233 KB | 388 B | 362 KB | 5 TIMs + 2 TMDs |
| `0004_town01` | 227 KB | 388 B | 226 KB | 5 TIMs + 1 TMD |
| `0005_town01` | 0 B | 388 B | 166 KB | none — schema-indexed data only |

`0005_town01` is the odd one out: the magic sits at offset 0 and there are no packed TIMs/TMDs after the schema. The simplest explanation is that this entry holds the canonical schema-indexed data block on its own — likely a default template that scene-specific entries override piecewise.

## Slot-size clusters

Because the schema is byte-identical across every instance, slots that share the same `slot[i+1] - slot[i]` are the **same kind of record**. Run `asset field-pack <PATH> --groups` to surface the clusters; the bucket structure on the canonical schema is:

| Bucket size (bytes) | Count | Likely interpretation |
|---:|---:|---|
| `0x2088` (8328) | 5 | Large blobs (TIM-page-like) at slots 1, 2, 3, 30, 41 — matches the 5-TIM count in `0003_town01` / `0004_town01` |
| `0x1010` (4112) | 2 | Medium records at slots 42, 43 |
| `0x810` (2064) | 1 | One large record at slot 94 |
| `0x610` (1552) | 1 | Slot 57 |
| `0x510` (1296) | 1 | Slot 91 |
| `0x490` (1168) | 1 | Slot 83 |
| `0x410` (1040) | 6 | Medium-records cluster at slots 4, 32, 44, 45, 61, 66 |
| `0x340` (832) | 1 | Slot 26 |
| `0x310` (784) | 2 | Slots 35, 72 |
| `0x218` (536) | 21 | NPC-record cluster at slots 5..25 (uniform stride; strong signal of a tabular array) |
| `0x210` (528) | 12 | Smaller record cluster |
| `0x190` (400) | 1 | Slot 70 |
| `0x150` (336) | 1 | Slot 80 |
| `0x130` (304) | 2 | Slots 54, 82 |
| `0x110` (272) | 17 | Dialog-trigger / event-region cluster |
| `0x100` (256) | 3 | Slots 56, 65, 67 |
| `0xD0` (208) | 2 | Slots 29, 89 |
| `0x90` (144) | 16 | Collision-box-sized cluster |
| `0x1` | 1 | Slot 0 — likely a single-byte flag/type marker |

The three big clusters (21 × 0x218, 17 × 0x110, 16 × 0x90) are arrays of fixed-size records — exactly the shape a field scene uses for NPC slots, event triggers, and hit regions. Five 0x2088 blobs match the empirical TIM count in the two town variants.

## Why the magic isn't load-bearing

A scan of `SCUS_942.54` and every captured overlay (dialog, town, battle action, menu, the 0896 / 0897 / battle-action clusters) for either the `LUI`+`ADDIU/ORI` immediate pair that synthesises `0x01059B84` or the byte sequence `84 9B 05 01` returns zero hits. The runtime never compares against this magic.

That rules out a magic-checked format loader. The most likely interpretation is that field-pack is a build-time layout artefact — the schema describes the in-RAM shape that per-scene code reads at hard-coded slot offsets, and the magic is a sanity marker the disc mastering left behind (or the dev tooling stamped) rather than a runtime parser anchor.

Per-slot interpretation therefore depends on locating the consumer — per-scene code in a field/town overlay that reads from the slot offsets. See [`ghidra/scripts/find_field_pack_magic.py`](../../ghidra/scripts/find_field_pack_magic.py) for the scan that established the magic isn't referenced, and [`ghidra/scripts/find_field_pack_consumers.py`](../../ghidra/scripts/find_field_pack_consumers.py) for the consumer-search complement.

## Tooling

```bash
asset field-pack <PATH>                # show schema + slot sizes
asset field-pack <PATH> --all-slots    # all 97 slot offsets/sizes
asset field-pack <PATH> --groups       # cluster slots by size (semantic index)
asset field-pack-scan <DIR>            # find every field-pack in a PROT dir
```

### Rust API

`legaia_asset::field_pack::FieldPack` exposes typed accessors:

```rust
// Classify a slot by index.
let kind: Option<SlotKind> = fp.slot_kind(i);

// Iterate all 97 slots with structural classification + bytes.
// Bytes are non-empty only when magic_offset == 0 (entry 0005_town01).
for (kind, bytes) in fp.iter_slots(buf) {
    // kind: SlotKind::{TypeFlag, TimPage, NpcRecord, EventTrigger,
    //                   CollisionBox, CompactRecord, MediumRecord,
    //                   SingleRecord, LastSlot}
}
```

`SlotKind` is derived from the slot's byte size and covers the major structural clusters identified in the size table above.  Per-slot interpretation beyond size-clustering is deferred pending a consumer trace (see "Why the magic isn't load-bearing" above).

The detector is reliable for classification today; per-slot interpretation is bracketed by the cluster table above and pending a per-scene consumer trace for the final mapping.
