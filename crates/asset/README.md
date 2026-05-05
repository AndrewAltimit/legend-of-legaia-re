# legaia-asset

Legaia asset descriptor parser, dispatcher, and the structural detectors
that classify raw PROT entries.

The game's loader (`FUN_8001f05c` in `SCUS_942.54`) takes a buffer plus a
single `u32` packing `(type << 24) | (size & 0xFFFFFF)` and dispatches to a
type-specific handler. Each asset can be either LZS-compressed (the
common case — handled by `FUN_8001a55c` via [`legaia-lzs`]) or stored raw
(handled by `FUN_8001a8b0`, a sized memcpy).

## What it provides

### Core descriptor + decoder

- `AssetType` — the enum of known asset categories.
- `Descriptor` — `(type, size, data_offset)` parsed from the on-disc form.
- `decode` — apply a `Descriptor` + `DecodeMode` to a buffer.
- `parse_player_lzs` — header parser for `player.lzs`-style containers.

### Streaming + pack formats

- `pack` — used inside DATA_FIELD streaming chunks. Header is
  `u32 count` then `u32 word_offsets[count]`.
- `parse_streaming` — DATA_FIELD streaming-chunk walker
  (entry point: `FUN_8002541c`).

### Structural detectors (for `categorize`)

| Module | What it detects |
|---|---|
| `categorize` | Dispatcher — runs every detector and tags the entry's `Class`. |
| `mips_overlay` | RAM overlays loaded into the `0x801C0000+` window. |
| `overlay_ptr_table` | Sister format: pointer tables that index into overlays. |
| `effect_bundle` | `efect.dat` and friends — magic `0x02018B0C`. |
| `field_pack` | Field bundles — magic `0x01059B84`. |
| `stage_geom` | Stage geometry: 12-byte prefix + 8-byte u16 quad records. |
| `scene_tmd_stream` | `[u32 chunk0][bare TMD][streaming chunks]`. |
| `scene_vab_stream` | `[u32 chunk0][VABp ...]`. |
| `scene_asset_table` | Per-scene asset slot table (CDNAME block layout). |
| `scene_v12_table` | Variant of the per-scene table. |
| `tim_scan` / `tmd_scan` | Brute-force magic search inside an entry. |

Detector coverage and provenance are tracked in
[`docs/formats/scene-bundles.md`](../../docs/formats/scene-bundles.md).

## CLI

```bash
asset describe         <input>            # parse + print descriptor
asset decode           <input> <output>   # apply the dispatcher
asset categorize       <PROT.DAT> [--cdname <CDNAME.TXT>]
asset find-overlay     <PROT.DAT>         # MIPS-code candidate scan
asset tim-scan         <input>            # locate embedded TIMs
asset tmd-scan         <input>            # locate embedded TMDs
asset stage / stage-scan
asset field-pack / field-pack-scan
asset effect-bundle / effect-bundle-scan
asset extract <PROT.DAT> <out_dir>        # full per-entry extraction
asset validate                            # cross-check detector coverage
```

`asset --help` lists the rest. `categorize` is the one most other tools
key off — its JSON output drives the asset-viewer's "browseable PROT
entry list" and the cross-reference scripts under [`scripts/`](../../scripts/).

## See also

- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  — full asset-loader chain from disc to dispatch.
- [`docs/formats/`](../../docs/formats/overview.md) — per-format byte-level
  specs that this crate's detectors implement.
