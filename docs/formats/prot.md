# PROT.DAT / DMY.DAT TOC

`PROT.DAT` is the main asset archive - 1232 numbered entries containing every TIM, TMD, VAB, MES, ANM, MDT, DATA_FIELD streaming buffer, scene asset table, and runtime overlay. `DMY.DAT` is a sibling archive that turns out to be developer fixtures (memory-bus test pattern + paired random blobs); see [DMY.DAT](dmy.md).

Implementation: `crates/prot/src/archive.rs`.

## Header (8 bytes at offset 0x000 OR 0x800)

```
u32 file_count_minus_1
u32 header_sectors      // size of TOC in 0x800-byte sectors
```

The detector tries offset 0x000 first, then 0x800, accepting whichever yields plausible values. PROT.DAT uses 0x000.

## TOC (immediately after header)

The TOC is a sequence of `u32` words. Each on-disc entry occupies multiple TOC slots. For entry index `p`:

```
start_lba             = toc[p + 2]                       // absolute LBA into PROT.DAT
indexed_size_sectors  = toc[p + 5] - toc[p + 3] + 4      // TOC-declared payload size
footprint_sectors     = toc[p + 3] - toc[p + 2]          // on-disc span to next entry
size_sectors          = max(indexed_size_sectors, footprint_sectors)
byte_offset           = start_lba * 0x800
size_bytes            = size_sectors * 0x800
```

`toc[p+5]` is the absolute LBA of entry `p+3` (an end-marker that aliases the next-entry's start), so `toc[p+5] - toc[p+3] + 4` recovers the indexed size in sectors.

### Trailing-overlay sectors (`indexed_size` vs `size`)

For ~24% of entries the on-disc contiguous range to the next entry's start LBA is **larger** than the indexed payload — the trailing sectors carry overlay content the SCUS boot loader reads via a multi-sector `ReadN` past the TOC-claimed end. PROT entry 899 is the canonical example: indexed payload is 14 sectors (28 KiB, the options menu), but the on-disc footprint is 74 sectors — the trailing 60 sectors are the title-screen overlay code (see [`subsystems/boot.md`](../subsystems/boot.md#title-overlay-source-on-disc)).

[`legaia_prot::archive::Archive`](../../crates/prot/src/archive.rs) exposes both views:

- `Entry::size_sectors` / `size_bytes` — full on-disc footprint (default).
- `Entry::indexed_size_sectors` / `indexed_size_bytes` — TOC-indexed payload only.
- `Archive::read_entry` reads the footprint; `Archive::read_entry_indexed` reads only the indexed sub-region.

Scene-side parsers were designed for the indexed view and use `read_entry_indexed` via [`ProtIndex::entry_bytes`](../../crates/engine-core/src/scene.rs). Asset-viewer / disc-browser consumers use the full footprint so trailing-overlay content is visible.

> **Historical note.** An earlier Python proof-of-concept used `start_lba = toc[p+5] - toc[p+2]`. That subtraction actually computes the SIZE in sectors and was misinterpreted as the start LBA — under that math `start_lba` collapsed to a small relative offset within "block 0" of the file, and ~80% of PROT entries ended up reading the SAME few low-LBA byte ranges. Anything written using that formula's outputs is artefacted; trust only post-`toc[p+2]` extractions. The `size_sectors = max(indexed, footprint)` extension is a later correction (the indexed formula alone misses trailing-overlay sectors for entries like 899).

## In-RAM TOC

`SCUS_942.54` keeps a transformed copy of the TOC at RAM address `0x801C70F0`. Used at `FUN_8003E8A8` (the LBA resolver):

```c
start_lba    = TABLE[(idx + 2) * 4 + 0x801C70F0]
end_lba      = TABLE[(idx + 3) * 4 + 0x801C70F0]
size_sectors = end_lba - start_lba
```

Different stride from the on-disc TOC. The on-disc-to-in-RAM transformation runs once at boot (`FUN_8003E4E8` reads the first three sectors of `PROT.DAT` into `0x801C70F0`).

## Resolving entries by name vs by index

Two entry points:

- `FUN_8003E8A8` - index-based (consumed directly by the streaming loader and the dev-build sound branch).
- `FUN_8003E6BC` - path-based; resolves dev paths like `data\battle\efect.dat` or `h:\PROT\FIELD\<scene>\…` into an index via the CDNAME-driven name map, then delegates to the LBA resolver. Most retail-build code paths land here.

Names come from [`CDNAME.TXT`](cdname.md), which lives at the top level of the disc.

## Overlay loaders (parallel slots)

Two paired wrappers on top of `FUN_8003E8A8` + `FUN_8003E800` (async LBA-based loader) manage two **independently swappable** overlay slots. Both compute `prot_index = param + 0x381`:

| Loader | Destination buffer ptr | Current-id tracker |
|---|---|---|
| `FUN_8003EBE4` | `*DAT_8001038C` | `gp+0x924` |
| `FUN_8003EC70` | `*DAT_80010390` | `gp+0x934` |

This means two overlays can be RAM-resident at the same time (e.g., a title-overlay code blob in slot A and a sister asset blob in slot B). Mode-init handlers use one or the other depending on what they're loading. The full CD-read API stack that backs these is documented in [`subsystems/boot.md` § CD-read API stack](../subsystems/boot.md#cd-read-api-stack).

`FUN_8003E360` shows a **dual-mode loader pattern**: in retail (`_DAT_8007B8C2 == 0`) it loads via the ISO9660 file system (`FUN_800608F0` / `FUN_80060944`); in debug (`_DAT_8007B8C2 != 0`) it loads via the PROT TOC index. Both branches reach the same data through different on-disc locations.

## See also

- [Disc layout](disc.md) - the Mode2/2352 geometry that holds PROT.DAT.
- [CDNAME map](cdname.md) - the name labels for PROT indices.
- [LZS compression](lzs.md) - the decompression most entries need.
- [Asset-type dispatch](asset-type.md) - the per-entry type-byte handler.
- [`tooling/extraction.md`](../tooling/extraction.md) - the extraction pipeline that walks the TOC.
