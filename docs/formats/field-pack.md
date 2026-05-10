# Field-pack format

Magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. Four PROT entries match the signature today. Detector + dispatch: `crates/asset/src/field_pack.rs`.

## Layout

```
[preamble - variable size, content shape unknown]
[u32 LE = 0x01059B84]                  <- MAGIC
[97 × u32 LE - schema table, 388 bytes - byte-identical across all field-packs]
[asset region - packed TIMs / TMDs, in some files]
```

The schema slot offsets cover `[0x60..0x16651]` (≈ 91 KB of logical layout). They are anchored on `slots[0] == 0x60` and `slots[96] == 0x16651` and are byte-identical across every field-pack file (MD5 `edcfdf1575889d63d2077c396089d7f3`). The schema is therefore a STATIC abstract layout, not per-file metadata.

## What the four entries look like

| PROT | preamble | schema | asset region | TIMs / TMDs in tim_scan / tmd_scan |
|---|---|---|---|---|
| `0002_gameover_data` | 234 KB | 388 B | 5.7 KB | 2 TIMs + several TMDs |
| `0003_town01` | 233 KB | 388 B | 362 KB | 5 TIMs + 2 TMDs |
| `0004_town01` | 227 KB | 388 B | 226 KB | 5 TIMs + 1 TMD |
| `0005_town01` | 0 B | 388 B | 166 KB | none - schema-indexed data only |

`0005_town01` is the odd one out: the magic sits at offset 0 and there are no packed TIMs/TMDs after the schema. The simplest explanation is that this entry holds the canonical schema-indexed data block on its own - likely a default template that scene-specific entries override piecewise.

## Slot-size clusters

Because the schema is byte-identical across every instance, slots that share the same `slot[i+1] - slot[i]` are the **same kind of record**. Run `asset field-pack <PATH> --groups` to surface the clusters; the bucket structure on the canonical schema is:

| Bucket size (bytes) | Count | Likely interpretation |
|---:|---:|---|
| `0x2088` (8328) | 5 | Large blobs (TIM-page-like) at slots 1, 2, 3, 30, 41 - matches the 5-TIM count in `0003_town01` / `0004_town01` |
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
| `0x1` | 1 | Slot 0 - likely a single-byte flag/type marker |

The three big clusters (21 × 0x218, 17 × 0x110, 16 × 0x90) are arrays of fixed-size records - exactly the shape a field scene uses for NPC slots, event triggers, and hit regions. Five 0x2088 blobs match the empirical TIM count in the two town variants.

## Why the magic isn't load-bearing

A scan of `SCUS_942.54` and every captured overlay (dialog, town, battle action, menu, the 0896 / 0897 / battle-action clusters) for either the `LUI`+`ADDIU/ORI` immediate pair that synthesises `0x01059B84` or the byte sequence `84 9B 05 01` returns zero hits. The runtime never compares against this magic.

That rules out a magic-checked format loader. The most likely interpretation is that field-pack is a build-time layout artefact - the schema describes the in-RAM shape that per-scene code reads at hard-coded slot offsets, and the magic is a sanity marker the disc mastering left behind (or the dev tooling stamped) rather than a runtime parser anchor.

Per-slot interpretation therefore depends on locating the consumer - per-scene code in a field/town overlay that reads from the slot offsets. See [`ghidra/scripts/find_field_pack_magic.py`](../../ghidra/scripts/find_field_pack_magic.py) for the scan that established the magic isn't referenced, and [`ghidra/scripts/find_field_pack_consumers.py`](../../ghidra/scripts/find_field_pack_consumers.py) for the consumer-search complement.

## Scene-transition consumer

The confirmed scene-transition caller of `FUN_8001f7c0` (scene asset loader)
is `FUN_801D6704` (overlay 0897, `801d6ae8`):

```
; a0 = _DAT_1f8003ec (DMA read buffer)
; a1 = 0x80084548   (scene name table)
; a2 = _DAT_80084540 (current scene pointer, from s4-8)
; a3 = 0
jal   0x8001f7c0
_clear a3
```

After the load, `FUN_801D6704` at `801d6b0c` calls `FUN_80020224`
(descriptor-pair walker), which iterates the asset descriptor table at
`_DAT_8007B85C` and dispatches each entry through `FUN_8001F05C` (asset
type dispatcher).

The 97-slot field-pack data is consumed at **static offsets** - there is no
slot-iteration loop in the captured code. The byte-identical schema confirms
it: the consumer treats the buffer as a fixed in-RAM layout template and
reads NPC/event/collision slots by hard-coded index, not by walking the
offset table. Per-NPC and per-event slot handlers are called indirectly
through the descriptor table; their specific entry points require capturing
a full scene-init execution trace (not yet available in the overlay dumps).

## Loader chain

Tracing the `town01` save `mc2` (CDNAME `town01`, scene `0x03`) through the captured overlays + `SCUS_942.54` pins the runtime path that brings a field-pack file into RAM:

```
FUN_801D6704  (overlay 0897, scene-transition orchestrator)
  └── FUN_8001F7C0(buffer_ptr, scene_name_table=0x80084548,
                   scene_index=0x80084540, 0)         ; scene asset loader (SCUS)
        ├── builds path  DATA\FIELD\<scene>           ; e.g. DATA\FIELD\town01
        ├── loads it via FUN_8003E6BC(path, buffer_ptr)
        ├── builds path  h:\PROT\FIELD\<scene>\efect.dat
        └── loads efect.dat at  buffer_ptr + 0x12800
              and writes  buffer_ptr + 0x12800  to  _DAT_8007B8D0
  └── FUN_80020224  (descriptor-pair walker)
        └── FUN_8001F05C(descriptor)                  ; per-asset-type dispatcher
              … iterates table at _DAT_8007B85C
```

The scene transition itself is initiated by `FUN_8001FD44(scene_name, sub_index)` - a static SCUS function that strcpy's the new scene name into the scene-name table at `0x80084548`, copies the previous scene name into `0x80084558`, and OR-flips the `0x40` bit in `_DAT_1F800394` (pending-transition story flag). Dialog-overlay handlers like `FUN_801D1344` call this directly when a story event needs to warp - e.g. the `town01` warp requires `_DAT_1F800394 & 0x04000000 != 0` plus a couple of menu-state flags.

`buffer_ptr` is read from scratchpad cell `0x1F8003EC` (the heap-resident scene asset buffer pointer). Per-scene values vary because the loader allocates from a pool. The asset descriptor table at `_DAT_8007B85C = 0x8015CBD0` is **statically allocated** and identical across captured saves; its entries point into the per-scene field-pack region above.

## Per-scene runtime RAM base

The active field-pack RAM base is recoverable from any save by reading `_DAT_8007B8D0` and subtracting `0x12800`. The constants and a `recover_base()` helper live in [`crates/engine-core/src/capture_observations.rs`](../../crates/engine-core/src/capture_observations.rs) under `field_pack_load`.

| Save | CDNAME | scene `0x80084540` | `_DAT_8007B8D0` | Field-pack RAM base |
|---|---|---|---|---|
| `mc2` | `town01` | `0x03` | `0x8014BD30` | `0x80139530` |
| `mc0` | `town0c` | `0x15` | `0x800B4DF0` | `0x800A25F0` |

The 75 KB region between the field-pack base and `_DAT_8007B8D0` (`base..base + 0x12800`) holds the loaded field asset; the slot-96 trailing zone of the schema falls inside it, and `efect.dat` lands immediately after.

## Runtime layout differs from on-disc schema

Reading the `mc2` save at `base + 0x60` (where on-disc slot 0 sits) yields **post-processed GP0 GPU primitive packets**, not the raw NPC / event-trigger / collision records the disc bytes encode. The 91 KB schema describes the **on-disc** logical layout; a loader transforms the on-disc preamble into a runtime structure that mixes:

- GP0-shaped primitive packets (visible at `base + 0x60`)
- The 400 KB shared scene-asset pool at `0x800C505C..0x80139527` (mc2 vs mc0 diff) the loader fills before / alongside the field-pack region - sibling buffers for TIM atlases, primitive scratch, descriptor-driven data
- The static asset descriptor table at `0x8015CBD0` whose entries point into the per-scene region

A direct preamble-byte → runtime-RAM-cell mapping requires capturing the loader **during** a scene transition (a frame between "scene change requested" and "field-pack region populated"). The current single-save snapshot is post-load, so only the FINAL runtime layout is observable, not the disc-byte-to-RAM-cell projection.

## Mednafen-state diff observations

A prior diff over the engine RAM range `0x801C0000..0x80200000` lit up a 9 KB region at `0x801F69D8..0x801F8F02` that toggled between two different MIPS-code overlays - different scenes load different per-area code into the same slot. The first 16 bytes match the standard PSX function-prologue shape (`addiu sp,sp,-N`, `sw s1,N(sp)`, `lui s1,0x801F`, `ori s1,s1,...`), confirming the slot is an MIPS overlay rather than a data buffer.

### Town01 vs town0c diff (mc2 ↔ mc0, full main RAM)

| Region | Bytes changed | Interpretation |
|---|---:|---|
| `0x800C505C..0x80139527` | ~402 KB | Shared scene-asset pool; ends just before mc2's field-pack base |
| `0x801853F5..0x801B93D0` | ~205 KB | Heap-resident sibling region (`0x80185000..0x801B9000`) |
| `0x8015CBD0..0x80184C89` | ~152 KB | Asset descriptor table contents (base = `0x8015CBD0`) |
| `0x80098900..0x800BE5FC` | ~132 KB | Other heap-resident scene buffers |
| `0x80084140..0x80084398` | 526 B | Scene-bundle metadata (pre-`SCENE_NAME_TABLE`) |
| `0x801F3488..0x801F69D8` | 7.6 KB | Just-before the 9 KB MIPS-overlay slot - post-overlay scratch |

The pinned residency window (9 KB MIPS overlay at `0x801F69D8..0x801F8F02`) does NOT change between mc2 and mc0 - both are town-resident saves and share a town overlay there. Engine-relevant residency differences live in the ~933 KB of heap-pool deltas above.

The disc-gated tests `town01_field_pack_save_documents_active_scene_and_ram_base` and `town01_vs_town0c_diff_lights_up_field_pack_pool` (in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs)) exercise both the static-scene-label assertion and the empirical heap-pool diff against the user's actual saves.

## Tooling

```bash
asset field-pack <PATH>                # show schema + slot sizes
asset field-pack <PATH> --all-slots    # all 97 slot offsets/sizes
asset field-pack <PATH> --groups       # cluster slots by size (semantic index)
asset field-pack-scan <DIR>            # find every field-pack in a PROT dir
```

### Rust API

`legaia_asset::field_pack::FieldPack` exposes typed accessors over a parsed file:

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

For static schema enumeration without holding a file:

```rust
use legaia_asset::field_pack::{CANONICAL_SCHEMA, canonical_slot, iter_canonical_slots};

// 97-element static array of u32 LE schema offsets.
assert_eq!(CANONICAL_SCHEMA[0], 0x60);

// Per-slot accessor: returns (offset, size) where size is None for slot 96.
let (off, size) = canonical_slot(5).unwrap();

// Iterate (index, kind, offset, size) over the canonical schema.
for (i, kind, off, size) in iter_canonical_slots() { /* ... */ }
```

`SlotKind` is derived from the slot's byte size and covers the major structural clusters identified in the size table above.

`legaia_engine_core::capture_observations::field_pack_load` exposes the runtime constants:

```rust
use legaia_engine_core::capture_observations::field_pack_load;

// Pin the heap-allocated RAM base from a saved main-RAM image.
let base = field_pack_load::recover_base(main_ram).expect("scene loaded");

// Walk the schema in RAM at this base.
for (i, _kind, off, _size) in legaia_asset::field_pack::iter_canonical_slots() {
    let abs = base + off;
    /* ... */
}
```

The detector is reliable for classification today; per-slot interpretation beyond size-clustering is bracketed by the cluster table above and pending a per-scene loader trace for the final on-disc → RAM projection.
