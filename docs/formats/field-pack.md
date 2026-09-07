# Field-pack - a format that does not exist

**"Field-pack" is not a format.** The entries this page used to describe are ordinary
[DATA_FIELD streaming](data-field.md) files whose single chunk is a
[`asset::pack`](pack.md) of PSX TIMs or Legaia TMDs. The word `0x01059B84` that named
the format is not a magic: it is that chunk's `(type << 24) | size` header, with
`type = 0x01` (`TIM_LIST`) and `size = 0x059B84`. The "97-entry strict schema" behind it
is the pack's own `[u32 count][u32 word_offsets[count]]` table, and the "≈ 91 KB
byte-identical global constant block" is two Rim Elm scenes sharing their first three
texture members.

Detector + CLI: `crates/asset/src/field_pack.rs` (kept as the classifier that owns these
entries; its corrected reader is [`scene_pack`](#reading-a-carrier)).

## Contents

- [Where the carriers sit](#where-the-carriers-sit)
- [Layout](#layout)
- [What each old claim really was](#what-each-old-claim-really-was)
- [Runtime consumers](#runtime-consumers)
- [Reading a carrier](#reading-a-carrier)
- [Per-scene runtime RAM base](#per-scene-runtime-ram-base)
- [Loader order-of-operations](#loader-order-of-operations)
- [Mednafen-state diff observations](#mednafen-state-diff-observations)
- [Tooling](#tooling)
- [See also](#see-also)

## Where the carriers sit

Every scene's [CDNAME](cdname.md) block seats the same slots at the same offsets from
its `#define`, in **raw TOC** space (extraction index = raw − 2):

| Block offset | Content |
|---|---|
| `+0` | `DATA\FIELD\<scene>.MAP` - the [field map](field-map.md), `0x12000` bytes |
| `+1` | `DATA\FIELD\<scene>.PCH` - the [walk-on trigger sidecar](scene-v12-table.md), `0x800` bytes |
| `+2` | `efect.dat` - the [move-VM stager prescript](scene-bundles.md#scene_event_scripts---prescript-only) |
| `+3` | the [scene asset table](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle) |
| `+4` | **this page** - the scene's streamed TIM / TMD pack, or a [pochi filler](pochi.md) where the scene has none |
| `+5…` | the battle-stage [`scene_tmd_stream`](scene-bundles.md#scene_tmd_stream---bare-tmd-prefix) backdrops |

The `+0`, `+1` and `+2` names come from the loader's own path literals (`FUN_8001F7C0`,
`see ghidra/scripts/funcs/8001f7c0.txt`); the `+4` position is what `FUN_800255B8`'s
by-index branch computes directly, `FUN_8003EB98(scene_index + 4, …)` with
`scene_index = *(0x80084540)` = the block's `#define` value (town01 = 3, town0c = 0x15 -
both matching `CDNAME.TXT`).

Sweeping the `+4` slot of every `#define` finds **23 carriers**. The rest of the slot is
one-sector pochi fill: a scene with no streamed pack still reserves the slot.

## Layout

Two forms, both a pack:

```text
; chunk-headered (18 carriers)
+0x00   u32  chunk_header       ; (type << 24) | size, the DATA_FIELD packing
+0x04   u32  count              ; pack member count
+0x08   u32  word_offset[count] ; byte offset = word_offset[i] * 4, relative to +0x04
+0x04 + 4 + 4*count             ; members, packed back-to-back
...     zero pad to the entry's sector-aligned end

; bare (5 carriers) - identical minus the chunk header
+0x00   u32  count
+0x04   u32  word_offset[count]
```

`type` is `0x01` (`TIM_LIST`) in 8 carriers and `0x02` (`TMD`) in 10, and in **every**
case it agrees with the members' own magic - all-TIM (`0x00000010`) under type `0x01`,
all-Legaia-TMD (`0x80000002`) under type `0x02`. `size` is the payload length: `4 + size`
lands inside the entry, before its sector padding, in all 18.

`word_offset[0] * 4` equals the header end (`4 + 4*count`) in all 23, which is the anchor
the reader gates on.

The chunk-headered form is the same trick the [scene bundles](scene-bundles.md) open with
- a `(type << 24) | size` word at offset 0 - and the walk that consumes it terminates on
the zero pad after the single chunk, so these files are one-chunk DATA_FIELD streams.

## What each old claim really was

| Old claim | What it is |
|---|---|
| Magic `0x01059B84` | town01's chunk header: `(TIM_LIST << 24) \| 0x059B84`. `0x059B84` = 367,492 = the pack's byte length. Every other carrier's word differs because its payload length differs, which is why a corpus scan found the "magic" exactly once. |
| "97-entry strict schema, byte-identical everywhere" | `[u32 count = 96][u32 word_offset[96]]`. `CANONICAL_SCHEMA[0] = 0x60` is the **count**, not an offset; `CANONICAL_SCHEMA[96] = 0x16651` is the last member's word offset. town0b's table is 98 words (count 97) and starts `0x62`, so it is not identical to town01's. |
| "≈ 91 KB schema-indexed region, a global constant" | town01 (`0005_town01`) and town0c (`0023_town0c`) share the first three members of their texture packs byte-for-byte - the same Rim Elm atlases. The compared span (91,633 bytes) ends inside member 2; their offset tables diverge at the fifth word (`0x6609` vs `0x6a09`), which is member 3's end, and their last members sit at `0x16651` vs `0x16a31`. |
| Slot-size clusters (`0x218` ×21, `0x110` ×17, `0x90` ×16, `0x2088` ×5) named "NPC record / event trigger / collision box / TIM page" | Member sizes in **words**. ×4 gives bytes: 2144, 1088, 576 and `0x8220` - and every one is a TIM. The `0x8220` five are the standard 64×256 4bpp atlas this repo meets everywhere else; the 2144s are 16×64 4bpp sprites with a 16-colour CLUT. There are no NPC, trigger or collision records here. |
| "The per-scene payload is the preamble" (234 KB / 233 KB / 227 KB …) | The over-reading entry size ([`prot.md`](prot.md)). Those bytes are the block's **earlier entries** - the prescript (`0003_town01`, 6144 B) and the scene asset table (`0004_town01`, 227,328 B). On its own sectors `0005_town01` has no preamble: the chunk header is at offset 0. |
| "The magic has zero runtime references" | True, and now expected: there is no magic to reference. |
| "8 carriers, four per block" | Four entries of one block all "carrying" the same block-final pack is the over-read signature. Each block has one carrier: town01 = `0005`, town0b = `0014`, town0c = `0023`. |

## Runtime consumers

`FUN_800255B8` (`see ghidra/scripts/funcs/800255b8.txt`) loads one streamed file into the
scene asset buffer `*(0x8007B85C)` and returns its sector count. It builds the path from
a mode argument:

| Mode | Path |
|---|---|
| `0x0A` | `h:\PROT\FIELD\<scene>\tim.dat` |
| `0x0F` | `h:\PROT\FIELD\<scene>\move.mdt` |
| `0x14` | `DATA\FIELD\<scene>.pac` |

The scene name comes from the name table at `0x80084548`. When the build flag at
`0x8007B8C2` is set the whole path build is skipped for
`FUN_8003EB98(*(0x80084540) + 4, *(0x8007B85C), 1)` - the by-index route named above.

`FUN_8002541C` (`see ghidra/scripts/funcs/8002541c.txt`) calls that loader and then
dispatches on the same mode:

- **`0x0A`** - treats the buffer as a bare pack: `count = base[0]`, then for `i` in
  `0..count` reads `base[1 + i]`, shifts left 2 and calls `FUN_800198E0` (`LoadImage`) at
  `base + word_offset[i] * 4`. This is the reader the **five bare carriers** fit; the
  loop would run 17 million times on a chunk-headered one.
- **`0x14`** - walks DATA_FIELD chunks: `size = *base & 0xFFFFFF`, dispatch
  `FUN_8001F05C(base + 4, *base, 0, 1)`, advance `base += (size & ~3) + 4`, stop when the
  size field is zero. This is the reader the **18 chunk-headered** carriers fit, and the
  type byte it hands the [asset-type dispatcher](asset-type.md) is exactly the `0x01` /
  `0x02` that matches their members.

Both paths therefore end in an already-documented reader; neither needs a field-pack
parser.

### The bundle's `Flag` descriptor is the mode argument

Nothing else picks the mode: it comes out of the scene's own asset table. The
[asset-type dispatcher](asset-type.md) `FUN_8001F05C` handles type bytes `0x0A` / `0x0F`
/ `0x14` by returning `(descriptor_low_byte) + (case << 8)` and nothing else
(`8001f574`, `8001f60c`, `8001f658`). `FUN_80020224` ORs every descriptor's return into
its status word, and the field init shifts that right by 8 and hands it to
`FUN_8002541C` (`801d6bf8`: `sra s1, s4, 0x8`, then `jal 0x8002541c`). So a `Flag(0x14)`
descriptor in the bundle *is* "stream `<scene>.pac`", and `Flag(0x0A)` is "stream
`tim.dat`".

The corpus agrees block by block. Over the 100 scene blocks whose `+3` entry parses as a
descriptor table:

| Bundle carries | Block `+4` holds | Blocks |
|---|---|---|
| `Flag(0x14)` | a chunk-headered single-chunk pack | 16 |
| `Flag(0x14)` | a multi-chunk DATA_FIELD stream (`MAN`/`MES`/`MOVE`/`VDF`) | 12 |
| `Flag(0x0A)` | a bare pack | 4 |
| no `Flag` | a one-sector pochi filler | 64 |

Two blocks break the pattern in the harmless direction - `opurud` and `other7` carry a
chunk-headered pack with no `Flag` descriptor to reach it. `Flag(0x0F)` (`move.mdt`)
appears in no retail bundle, which is consistent with the move table being descriptor
type `0x05` inside the bundle rather than a streamed file ([`mdt.md`](mdt.md)).

## Reading a carrier

```rust
use legaia_asset::field_pack;

// Handles both forms: `chunk_header` is None for the bare carriers.
if let Some(p) = field_pack::scene_pack(&entry_bytes) {
    println!("{} members, type {:?}", p.members.len(), p.asset_type());
    for r in &p.members {
        let member = &entry_bytes[r.clone()];   // a PSX TIM or a Legaia TMD
        let _ = member;
    }
}
```

`scene_pack` gates on the pack anchor (`word_offset[0] * 4 == 4 + 4*count`), monotonic
offsets, and - when a chunk header is present - a legal type byte whose declared size
fits the buffer. Disc-gated coverage: `crates/asset/tests/field_pack_real.rs` pins the
carrier positions, the chunk-header arithmetic, the type-byte / member-magic agreement,
and the two-Rim-Elm shared prefix.

`detect` / `FieldPack` / `CANONICAL_SCHEMA` stay for the classifier and the
`asset field-pack` CLI; their doc comments carry the corrected reading, and
`CANONICAL_SCHEMA` is now what it always was - a verbatim copy of town01's pack table,
count word included.

## Per-scene runtime RAM base

`_DAT_8007B8D0` is the `efect.dat` base, and `FUN_8001F7C0` sets it to
`*(0x1F8003EC) + 0x12800`. So `_DAT_8007B8D0 − 0x12800` recovers the **field-file scratch
base**, the buffer holding `<scene>.MAP` at `+0` and `<scene>.PCH` at `+0x12000` - it is
not a field-pack base, and nothing at `base + 0x60` is a schema slot. The constants and a
`recover_base()` helper live in
[`crates/engine-core/src/capture_observations.rs`](../../crates/engine-core/src/capture_observations.rs)
under `field_pack_load` (the module name predates this correction).

| Save | CDNAME | scene `0x80084540` | `_DAT_8007B8D0` | Field-file scratch base |
|---|---|---|---|---|
| `mc2` | `town01` | `0x03` | `0x8014BD30` | `0x80139530` |
| `mc0` | `town0c` | `0x15` | `0x800B4DF0` | `0x800A25F0` |

Reading `base + 0x60` in the `mc2` save yields GP0 GPU primitive packets, which is what
that buffer holds - the scene's primitive scratch. The earlier reading of that as "the
runtime layout differs from the on-disc schema" was comparing a scratch buffer against a
schema that does not exist.

The scene's own asset buffer is the separate `0x62C00`-byte allocation at
`*(0x8007B85C)`, allocated once by `FUN_8001E1B4` (`8001e28c`:
`FUN_80017888(0, 0x62C00)` then `sw v0, -0x47a4(at)`); the streamed pack lands there, at
offset 0.

## Loader order-of-operations

A save captured mid-transition between `town01` and `town0c` pins the sequencing:

- The scene-bundle pool at `0x80084540` already carries the **destination** scene name.
- `_DAT_8007B8D0` still reads the **previous** scene's value.
- The destination scene's scratch region is partially populated; the previous scene's is
  zeroed.
- The `0x8015CBD0` table is bit-identical between the pre- and mid-transition snapshots.

So: **(1)** write the new scene name into the bundle pool, **(2)** zero the previous
scratch region, **(3)** populate the destination region, **(4)** flip `_DAT_8007B8D0`
last. Mid-transition, a scene swap is detectable by the pool slot's CDNAME label
disagreeing with the base implied by `_DAT_8007B8D0`.

Detector + constants: `legaia_engine_core::capture_observations::field_pack_intra_transition`.

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

A diff over `0x801C0000..0x80200000` lights up a 9 KB region at `0x801F69D8..0x801F8F02`
that toggles between two MIPS-code overlays - different scenes load different per-area
code into the same slot. Its first 16 bytes match the standard PSX function prologue,
confirming an overlay rather than a data buffer.

### Town01 vs town0c diff (mc2 ↔ mc0, full main RAM)

| Region | Bytes changed | Interpretation |
|---|---:|---|
| `0x800C505C..0x80139527` | ~402 KB | Shared scene-asset pool; ends just before mc2's field-file scratch base |
| `0x801853F5..0x801B93D0` | ~205 KB | Heap-resident sibling region |
| `0x8015CBD0..0x80184C89` | ~152 KB | Asset descriptor table contents |
| `0x80098900..0x800BE5FC` | ~132 KB | Other heap-resident scene buffers |
| `0x80084140..0x80084398` | 526 B | Scene-bundle metadata |
| `0x801F3488..0x801F69D8` | 7.6 KB | Post-overlay scratch |

The 9 KB overlay slot does **not** change between mc2 and mc0 - both are town-resident
saves sharing a town overlay there.

Disc-gated coverage: `town01_field_pack_save_documents_active_scene_and_ram_base` and
`town01_vs_town0c_diff_lights_up_field_pack_pool` in
[`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs).

## Tooling

```bash
asset field-pack <PATH>                # legacy view: chunk header + pack table
asset field-pack <PATH> --all-slots    # every member offset/size
asset field-pack <PATH> --groups       # cluster members by size
asset field-pack-scan <DIR>            # find the chunk-headered carriers in a PROT dir
```

## See also

- [asset::pack](pack.md) - the pack these carriers hold. This *is* that format.
- [DATA_FIELD streaming](data-field.md) - the chunk header the 18 chunk-headered carriers
  open with.
- [Asset-type dispatch](asset-type.md) - what the `0x01` / `0x02` type byte selects.
- [prot.md](prot.md) - the entry-size correction that dissolved the "preamble".
- [PSX TIM](tim.md) / [Legaia TMD](tmd.md) - the member sub-assets.
