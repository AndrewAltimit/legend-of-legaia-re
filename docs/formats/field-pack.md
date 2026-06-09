# Field-pack format

Magic `0x01059B84` followed by a 97-entry strict schema and a **byte-identical, globally-constant ≈ 91 KB block**. Detector + dispatch: `crates/asset/src/field_pack.rs`.

> **Corrected from disc (raw PROT scan).** The earlier reading — "124 PROT
> entries share the schema, the preamble fills the slots per-scene" — does not
> survive a byte scan of the corpus. The magic appears **raw in exactly four
> PROT entries** (`0002_gameover_data`, `0003`/`0004`/`0005_town01`); the 97-entry
> schema *signature* appears in **eight** (the other four — `0020_town0b`,
> `0021`/`0022`/`0023_town0c` — carry it **without** the magic prefix). And the
> ≈ 91 KB region the schema indexes is a **global constant**: byte-identical
> (FNV/SHA `c85d6a44d742…`) across town01 **and** town0c. So the slots are **not**
> filled per-scene — they are a shared template. The per-scene payload is the
> **preamble** that precedes the block. See [Corrected structure](#corrected-structure).

## Layout

```
[preamble - per-scene payload (count + u16 offset table + records; scene-structure shaped)]
[u32 LE = 0x01059B84]                  <- MAGIC (present in only 4 of the 8 carriers)
[97 × u32 LE - schema table, 388 bytes - byte-identical everywhere]
[≈ 91 KB schema-indexed region - byte-identical GLOBAL CONSTANT block]
[asset region - packed TIMs / TMDs, in some files]
```

The schema slot offsets cover `[0x60..0x16651]` (≈ 91 KB of logical layout). They are anchored on `slots[0] == 0x60` and `slots[96] == 0x16651` and are byte-identical across every carrier (MD5 `edcfdf1575889d63d2077c396089d7f3`). The schema is a STATIC abstract layout, and the region it indexes is likewise a fixed shared blob — not per-file metadata.

## Corrected structure

The four magic-bearing entries and their preamble/region/asset split:

| PROT | magic? | preamble | schema | region (≈91 KB) | asset region | TIMs / TMDs |
|---|---|---|---|---|---|---|
| `0002_gameover_data` | yes | 234 KB | 388 B | **5.7 KB (truncated)** | — | 2 TIMs + TMDs |
| `0003_town01` | yes | 233 KB | 388 B | full, `c85d6a44…` | 250 KB | 5 TIMs + 2 TMDs |
| `0004_town01` | yes | 227 KB | 388 B | full, `c85d6a44…` | 226 KB | 5 TIMs + 1 TMD |
| `0005_town01` | yes | 0 B | 388 B | full, `c85d6a44…` | trailing | none |
| `0020_town0b` | **no** | 222 KB | 388 B | 5.8 KB (truncated) | — | — |
| `0021_town0c` | **no** | 222 KB | 388 B | full, `c85d6a44…` | … | — |
| `0022_town0c` | **no** | 213 KB | 388 B | full, `c85d6a44…` | … | — |
| `0023_town0c` | **no** | 0 B | 388 B | full, `c85d6a44…` | trailing | — |

Two facts fall out:

- **The ≈ 91 KB region is a global constant.** Every full-length carrier (town01 + town0c) hashes identically. A block that is byte-identical across unrelated scenes is a **shared template / default asset**, not the scene's own field data. (`0002` and `0020` carry only a ~5.7 KB head of it.)
- **The magic is decorative.** The identical block ships with the magic in town01 and **without** it in town0c. Combined with [the magic having zero runtime references](#why-the-magic-isnt-load-bearing), the `0x01059B84` word is a build-tool stamp, not a parser anchor — `0005`/`0023` are the same "template-only" entry (region at file offset 0), one stamped and one not.

`0005_town01` / `0023_town0c` are the template-only carriers: the block sits at offset 0 with no preamble. Disc-gated coverage: `crates/asset/tests/field_pack_real.rs`.

### The per-scene payload is the preamble

What actually varies per scene is the **preamble** before the block. In `0003_town01` it begins with a record count (`0x3F` = 63) and an ascending `u16` offset table (`0x80, 0x380, 0x3c2, 0x42a, …`) followed by variable-length records — this is exactly the [`scene_event_scripts`](scene-bundles.md#scene_event_scripts---prescript-only) prescript shape (`[u16 count][u16 offsets[count]][records]`). Record 0 is a fixed 768-byte dispatch table; records `1..` are word-aligned (16-bit) actor/event command records (`0xFFFF 0x0000` header sentinel, `0x0008` terminator) — **not** field-VM bytecode. The town0b / town0c field files that carry **no** field-pack block at all (`0012_town0b`, …) **open with the same prescript**, confirming the preamble — not the constant block — is the scene's field data. So the long-open "preamble → schema-slot mapping" question (backlog D-FP) rests on a false premise: the slots are a fixed template; there is nothing per-scene to project into them, and the per-scene structure is the already-parsed `scene_event_scripts` prescript.

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

The scene transition itself is initiated by `FUN_8001FD44(scene_name, sub_index)` - a static SCUS function that:

- strcpy's the new scene name into the scene-name table at `0x80084548`;
- copies the previous scene name into `0x80084558`;
- OR-flips the `0x40` bit in `_DAT_1F800394` (pending-transition story flag).

Dialog-overlay handlers like `FUN_801D1344` call this directly when a story event needs to warp - e.g. the `town01` warp requires `_DAT_1F800394 & 0x04000000 != 0` plus a couple of menu-state flags.

`buffer_ptr` is read from scratchpad cell `0x1F8003EC` (the heap-resident scene asset buffer pointer). Per-scene values vary because the loader allocates from a pool. The asset descriptor table at `_DAT_8007B85C = 0x8015CBD0` is **statically allocated** and identical across captured saves; its entries point into the per-scene field-pack region above.

## Per-scene runtime RAM base

The active field-pack RAM base is recoverable from any save by reading `_DAT_8007B8D0` and subtracting `0x12800`. The constants and a `recover_base()` helper live in [`crates/engine-core/src/capture_observations.rs`](../../crates/engine-core/src/capture_observations.rs) under `field_pack_load`.

| Save | CDNAME | scene `0x80084540` | `_DAT_8007B8D0` | Field-pack RAM base |
|---|---|---|---|---|
| `mc2` | `town01` | `0x03` | `0x8014BD30` | `0x80139530` |
| `mc0` | `town0c` | `0x15` | `0x800B4DF0` | `0x800A25F0` |

The 75 KB region between the field-pack base and `_DAT_8007B8D0` (`base..base + 0x12800`) holds the loaded field asset; the slot-96 trailing zone of the schema falls inside it, and `efect.dat` lands immediately after.

## Runtime layout differs from on-disc schema

> **Re-check needed (see [Corrected structure](#corrected-structure)).** This
> section concluded that the loader *transforms* per-scene preamble bytes into
> the runtime slots. That premise is now doubtful: the schema-indexed region is
> a **disc-side global constant** (identical across town01 / town0c), so there
> is no per-scene content to project into it. Also, `base` here is
> `_DAT_8007B8D0 − 0x12800` = the **field-file buffer base**, but the loader
> places the field-asset region at `buffer + 0x12000` (efect.dat at `+0x12800`),
> so `base + 0x60` is the field file's `+0x0000` object/primitive region, **not**
> field-pack schema slot 0 (which would be near `buffer + 0x12000 + 0x60`). The
> GP0 packets observed below are therefore most likely the scene's primitive
> scratch, not a transformed slot 0. Treat the runtime-projection claim as open.

Reading the `mc2` save at `base + 0x60` (where on-disc slot 0 was *assumed* to sit) yields **post-processed GP0 GPU primitive packets**, not the raw NPC / event-trigger / collision records the disc bytes encode. The 91 KB schema describes a fixed **on-disc** logical layout; the observed runtime structure mixes:

- GP0-shaped primitive packets (visible at `base + 0x60`)
- The 400 KB shared scene-asset pool at `0x800C505C..0x80139527` (mc2 vs mc0 diff) the loader fills before / alongside the field-pack region - sibling buffers for TIM atlases, primitive scratch, descriptor-driven data
- The static asset descriptor table at `0x8015CBD0` whose entries point into the per-scene region

A direct preamble-byte → runtime-RAM-cell mapping requires capturing the loader **during** a scene transition (a frame between "scene change requested" and "field-pack region populated"). The current single-save snapshot is post-load, so only the FINAL runtime layout is observable, not the disc-byte-to-RAM-cell projection.

## Loader order-of-operations

A save captured mid-transition between `town01` (intro Rim Elm) and `town0c` (Rim Elm normal entry) pins the loader's order-of-operations. The mid-transition snapshot has these properties simultaneously:

- The scene-bundle pool at `0x80084540` already carries the **destination** scene name (`town0c`) - both pool slots `+0x08` and `+0x18` flip together.
- `_DAT_8007B8D0` still reads the **previous** scene's value (`0x8014BD30`, town01's `efect.dat` base).
- The destination scene's field-pack region at the canonical town0c base (`0x800A25F0..0x800B4DF0`) is partially populated.
- The previous scene's field-pack region at `0x80139530` is zeroed.
- The static asset descriptor table at `0x8015CBD0` is bit-identical between the pre- and mid-transition snapshots (4 KB SHA-256 match).

That sequencing pins the loader as: **(1)** write new scene name into the bundle pool, **(2)** zero the previous field-pack region, **(3)** populate the destination region at its canonical base, **(4)** flip `_DAT_8007B8D0` last. Mid-transition, the engine can detect a scene swap is in flight by checking that the pool slot's CDNAME label disagrees with the field-pack base implied by `_DAT_8007B8D0`.

The detector and constants live in `legaia_engine_core::capture_observations::field_pack_intra_transition`:

```rust
use legaia_engine_core::capture_observations::field_pack_intra_transition;

if let Some((label, stale_base)) =
    field_pack_intra_transition::detect_mid_transition(main_ram)
{
    eprintln!(
        "scene transition in flight: pool says {label}, base still reads 0x{stale_base:08X}"
    );
}
```

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

## See also

- [asset::pack](pack.md) - the in-DATA_FIELD pack this format is often confused with.
- [PSX TIM](tim.md) - the texture sub-asset bundled in field-pack slots.
- [Legaia TMD](tmd.md) - the mesh sub-asset bundled in field-pack slots.
