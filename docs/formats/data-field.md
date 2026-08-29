# DATA_FIELD streaming format

A stream of typed chunks consumed by `FUN_8002541C` on its `0x14` (DATA_FIELD) branch. The walker passes every chunk through the [asset-type dispatcher](asset-type.md) with `copy_only=1`, so chunks are always uncompressed.

Implementation: `crates/asset/src/lib.rs::parse_streaming`.

> **Scope note.** This doc covers the *typed-chunk streaming* shape that `FUN_8002541C` consumes - not the wider question of "what does the per-scene CDNAME block actually carry?" Per-scene field bundles use multiple shapes; the typed map below identifies which shapes show up where.

## Layout

```
[u32 type_size] [size_bytes of raw data]
[u32 type_size] [size_bytes of raw data]
...
[u32 terminator]    // type_size with low 24 bits all zero
```

Where:
- `type_size = (type_byte << 24) | (size_bytes & 0x00FFFFFF)`.
- `type_byte` matches the [asset type table](asset-type.md).
- The next chunk header starts at `current_pos + 4 + (size & ~3)` - i.e., header + size truncated to a 4-byte boundary. Sizes are always 4-aligned in practice.
- Terminator: any header `u32` whose low 24 bits are zero.

## What's in the wild

`asset scan-stream` strict-validates 34 PROT entries (class `data_field_streaming`), in two families.

**Character / effect streams** - the `other5` run, entries 1204..1219. The common shape is three chunks:

```
chunk[0]: TIM   (single, magic 0x10)        - sprite atlas / texture
chunk[1]: TMD2  (single, magic 0x80000002)  - single Legaia TMD
chunk[2]: MOVE2 (single, magic 0x08)        - animation data
terminator
```

The two heads of the run are homogeneous instead: `1204` is five `TMD2` chunks (the battle-form party meshes) and `1205` is eight `TIM` chunks of `0x8220` bytes each (their atlases) - which is what makes the atlas stride `0x8224`, chunk header included.

**Scene bundles** - `MAN / MES / MOVE / VDF`, four chunks (`dolk2`, `rikuroa`, `rikuroa2`, `rayman`, `station`, `taiku`, `taiku2`, `doman`, `nilboa2`, `edbalden`, `eddoman`). Shorter variants drop the trailing `VDF` (`balden2`, `ropeway2`) or lead with a bare `TMD` where a `MAN` would sit (`balden`, `bubu1`, `edbubu`). Two entries are neither family: `init_data` is `TIM_LIST + TMD`, and `0890_sound_data2` is two `TIM`s.

The chunk layouts are **single assets** in the `other5` family (one TIM, one TMD2, one MOVE2), not packs. Other clusters elsewhere in the corpus do use pack-shaped TIM_LIST / TMD chunks; the [pack format](pack.md) handles that case.

## Trailer data

Some entries contain bytes past the streaming terminator. `asset extract` preserves these as `_trailer.bin` next to the extracted chunks. The function that consumes the trailer hasn't been located; tracing the caller of `FUN_8002541C` in the field/town overlay is the next move if a specific entry's trailer looks structured.

## Related shapes

- The [scene-TMD-prefixed streaming](scene-bundles.md) shape is structurally similar but the leading chunk has no `[u32 type_size]` header and the inner content is a bare TMD instead of a TIM.
- The [scene-VAB-prefixed streaming](scene-bundles.md) shape uses the same chunk0-header trick but with VAB content.
- [Pack format](pack.md) lives *inside* TIM_LIST / TMD chunks when the chunk's data is a pack rather than a single asset.
- One entry (`0892_level_up`) carries the streaming layout but its **final** chunk's declared `size` walks past the entry's end without a terminator on disc - three clean leading chunks, then an over-large fourth header. Detector: `crates/asset/src/data_field_truncated.rs` (class `data_field_truncated`). How the runtime consumes that chunk is not pinned.
- Three scene entries commonly cited in this class - `0157_rikuroa`, `0228_station`, `0373_taiku` - are **not** truncated. Each is one of the four-chunk `MAN / MES / MOVE / VDF` bundles above, and each is a case where the superseded `toc[p+5] - toc[p+3] + 4` span fell *short* of the real entry (`0157` declares 163840 bytes against a real 186368; the other two are short by 69632 and 8192), so the last chunk overran a buffer that ended early. Against their own sectors all three terminate cleanly. See [`prot.md`](prot.md#tocp5---tocp3--4-is-not-an-entrys-size).

## Per-scene field bundles - what's still open

The CDNAME block for a typical field/town scene (e.g. `town01`, `bubu1`) carries 8–12 PROT entries. Categorize identifies several known shapes per block:

| Common slot | Class | Typical content |
|---|---|---|
| 0 / 1 | `SceneTmdStream` or `TmdSizePrefix` | scene mesh (room geometry) |
| 1 / 2 | `Pack` (TIM-pack) | scene textures / sprite atlas |
| 2 / 3 | `SceneEventScripts` or `SceneScriptedAssetTable` | per-event field-VM bytecode |
| 3 / 4 | `MesContainer` | dialog text |
| 4 / 5 | `Pack` (ANM-pack) | per-actor animation sets |
| 5..7 | `PochiFiller` | reserved-but-unused dev fillers |
| 6..8 | `SceneVabStream` (rare; only on scenes with custom audio) | per-scene VAB + SEQ |

What's **NOT** modelled yet:
- Cross-entry pointers (NPC references that point into other PROT entries - the asset chain in [asset-loader.md](../subsystems/asset-loader.md) is best-effort).
- The runtime-reconstructed slot-to-asset mapping inside `field-pack` containers (magic `0x01059B84`, 124 entries) - known shape, unknown slot semantics.
- The retail engine's per-scene "asset table" indirection (`SceneAssetTable` and `SceneScriptedAssetTable` detectors fire on a small fraction of entries; full reverse needs an overlay capture of `FUN_8001f7c0` at scene-load time).

The categorize sweep covers the bulk of bytes - every PROT entry classifies to *something*, and ~95% of bytes fall into known classes. Refining the residual classes is the work tracked under "Reverse-engineer DATA_FIELD per-scene layout" in [`docs/subsystems/engine.md`](../subsystems/engine.md).

## See also

- [asset::pack](pack.md) - the pack format carried inside DATA_FIELD chunks.
- [Asset-type dispatch](asset-type.md) - the type byte that opens each chunk.
- [Asset descriptor](asset-descriptor.md) - the descriptor layout the chunks reference.
