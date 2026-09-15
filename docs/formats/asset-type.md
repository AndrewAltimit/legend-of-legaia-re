# Asset type dispatcher

`FUN_8001F05C` is the central asset-type dispatcher - every per-asset-format branch (TIM, TMD, MES, ANM, …) is reached through it. Implementation: `crates/asset/src/lib.rs::AssetType`. Source: `ghidra/scripts/funcs/8001f05c.txt`.

## Calling convention

```c
result = FUN_8001f05c(byte *src_data, u32 type_and_size, int param3, int copy_only);
```

- `type_and_size` packs **type** in the high 8 bits, **size** in the low 24 bits.
- `copy_only != 0` → asset is uncompressed; the dispatcher calls `FUN_8001A8B0` (memcpy).
- `copy_only == 0` → asset is LZS-compressed; the dispatcher calls `FUN_8001A55C` (the [LZS decoder](lzs.md)).

## Type table

| Type byte | Name (from malloc-error string) | Notes |
|---|---|---|
| `0x00` | `TIM` | Single PSX texture, 0x11800-byte buffer |
| `0x01` | `TIM_LIST` (`Tim_Malloc_Err`) | Pack of multiple TIMs, 0x70800-byte buffer |
| `0x02` | `TMD` | Pack of meshes; calls `FUN_80026B4C` per submesh |
| `0x03` | `MAN` | Raw load |
| `0x04` | `MES` | Dialog text |
| `0x05` | `MOVE` | Raw load. Also carries the **per-scene player ANM bundle** despite the "MOVE" label - see [`anm.md` § Disc source](anm.md#disc-source---per-scene-anm-bundle); content is a canonical ANM container with `marker_1 = 0x080C` records. The dispatcher's "ANM malloc-err" string at `FUN_8001F05C` case 6 indexes this type, not type `0x06`. |
| `0x06` | `ANM` | Reserved by the asset-type enum but **not the player ANM source** on disc - those are stored under type `0x05` (above). The type-`0x06` slot carries the **CLUT-walk `MoveImage` walker table** (case 6 installs it at `DAT_8007B7C8`; `FUN_8001ADA4` case 0xB steps it): populated in 12 bundles - the 3 kingdoms plus 9 water/waterfall field scenes - and a 4-byte `count = 0` placeholder everywhere else. See [`subsystems/field-ambient-fx.md`](../subsystems/field-ambient-fx.md); parser `legaia_asset::clut_walk::from_scene_bundle`. |
| `0x07` | `VDF` | Vertex-deformation (morph-delta) pack, installed at `DAT_8007B7DC` and post-processed via `FUN_8001FBCC` per sub-entry (pointer table `0x80083E58`). Populated in 61 scene bundles; parser `legaia_asset::scene_vdf`. |
| `0x08` | `SIN` | Raw load |
| `0x09` | `TMD2` | Single bare TMD blob (no pack header). Hands directly to `FUN_80026B4C`; same on-disc format as a single member of the TMD-pack used by case 2. Parse with `crates/tmd::parse` directly. |
| `0x0B` | `MOVE2` | Raw load with cleanup of prior buffer |
| `0x0A` | `FLAG` (implicit) | Returns sentinel `0xA00` - no malloc, no decompress, no register |
| `0x0F` | `FLAG` | Returns sentinel `0xF00` |
| `0x14` | `FLAG` | Returns sentinel `0x1400` |

Names come from the dev's malloc-error strings literally embedded in the binary (`s_Tim_Malloc_Err_800104E8`, `s_tmd_malloc_err_80010504`, etc).

## Return-value bitfield

Every return is a small bitfield:

| Type | Return |
|---|---|
| 0x00 TIM | 0x01 |
| 0x01 TIM_LIST | 0x01 |
| 0x02 TMD | 0x02 |
| 0x03 MAN | 0x04 |
| 0x04 MES | 0x08 |
| 0x05 MOVE | 0x10 |
| 0x06 ANM | 0x20 |
| 0x07 VDF | 0x40 |
| 0x08 SIN | 0x80 |
| 0x09 TMD2 | 0 (success) or asset-error bit |
| 0x0B MOVE2 | 0x10 (same bit as MOVE) |
| 0x0A | `0xA00` (`type << 8`) - pure flag |
| 0x0F | `0xF00` (`type << 8`) - pure flag |
| 0x14 | `0x1400` (`type << 8`) - pure flag |

The two SCUS call sites (`FUN_8002541C` streaming-walker and `FUN_80020224` descriptor walker) **OR all returns into one accumulator** and return that union, so the FLAG sentinels become high bits in the streaming-walker's "what was in this stream" summary value:

```
return_value & 0x00FF  = bit per data-bearing asset type seen
return_value & 0xFF00  = bit per FLAG-type marker seen
```

## Why FLAG types exist

The FLAG cases let the dispatcher accept chunks whose `type_byte` falls outside the data-bearing range without aborting the streaming walk. The data bytes are still skipped by the walker (`advance = 4 + (size & ~3)`), but the dispatcher never reads them. So FLAG chunks act as **stream-level out-of-band markers** - any code that calls `FUN_8002541C` and looks at the returned bitfield can detect that "this stream contained a `0x14`-typed marker chunk" without that marker carrying a parsed asset.

Where the markers are consumed is open; possibly by code that reads past the streaming terminator (the [DATA_FIELD trailer](data-field.md)).

## `AssetType` Rust enum

```rust
pub enum AssetType {
    Tim,        // 0
    TimList,    // 1
    Tmd,        // 2
    Man,        // 3
    Mes,        // 4
    Move,       // 5
    Anm,        // 6
    Vdf,        // 7
    Sin,        // 8
    Tmd2,       // 9
    Move2,      // 0xB
    Flag(u8),   // 0xA, 0xF, 0x14
    Unknown(u8),
}
```

## What an `Unknown` type byte does

`AssetType::Unknown(u8)` covers **242 of the 256** values: the seven in-range
gaps `0x0C`, `0x0D`, `0x0E`, `0x10`, `0x11`, `0x12`, `0x13`, and everything
from `0x15` upward. Both routes end at the same address:

```text
8001F0B4  sltiu $v0, $v1, 0x15      ; type < 0x15 ?
8001F0B8  beqz  $v0, 0x8001f660     ; no  -> 0x8001F660
8001F0C0  addiu $v0, $v0, 0x638     ; jump table @ 0x80010638, 21 entries
8001F0D4  jr    $v0
```

and the jump table's entries for the seven in-range gaps are all
`0x8001F660` as well.

**`0x8001F660` is not a default handler - it is the function's return tail.**
`$s4` is zeroed at `0x8001F084` and nothing on an unknown path writes it, so an
unknown type byte **returns 0** having done nothing: no allocation, no
`jal 0x8001A55C` (LZS), no `jal 0x8001A8B0` (copy), no registration. It also
raises no error, unlike the allocation-failure paths, which set the error word
and print. It is a silent no-op. (The three FLAG arms differ only in writing
`$s4` before falling into the same tail - `0xA00` at `0x8001F574`, `0xF00` at
`0x8001F60C`, `0x1400` at `0x8001F658` - which is why their return values are
`type << 8`.)

The table is never patched: a reference sweep over `[0x80010638, 0x8001068C)`
across 84 images returns one hit, the `lui` at `0x8001F0BC` that indexes it.
There is no store into it and no second index site, so no handler is installed
into the empty slots at runtime.

## Where the dispatcher actually gets called

**Three** `jal 0x8001F05C` sites exist on the disc (1234 images, 121.4 MB).
Two are in SCUS: `FUN_8002541C` at `0x80025564`, reached only from its `0x14`
(DATA_FIELD) branch, and `FUN_80020224` at `0x800202B4`, the descriptor-pair
walker with zero static xrefs in SCUS, called at runtime from the town/field
overlay (`FUN_801D6704` → `0x801D6B0C` with `a0 = 0`); see
[asset descriptor](asset-descriptor.md).

The third is **overlay-resident**: `0x801D4DA8` in PROT 0976
(`baka_fighter`), an in-overlay re-implementation of the same DATA_FIELD walk
with the same `advance = 4 + (size & ~3)` step. It passes `copy_only = 1` and
**discards** the return value rather than folding it into an accumulator, so
the FLAG bitfield above is a two-caller property, not a three-caller one. The
`jal` itself is a byte fact (the target is encoded absolutely) at file offset
`0x6590`, inside 0976's own `content_bytes`; the printed VA depends on that
image's `static`-graded base.

## See also

- [Asset descriptor](asset-descriptor.md) - the descriptor-pair layout the dispatcher consumes.
- [DATA_FIELD streaming](data-field.md) - the streaming container whose `0x14` branch reaches the dispatcher.
- [asset::pack](pack.md) - the in-DATA_FIELD pack the type bytes resolve to.
- [LZS compression](lzs.md) - the decode path taken when `copy_only` is zero.
