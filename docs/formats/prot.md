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

The TOC is a flat array of `u32` words (`toc[]` below - word 0 is the first u32 after the 8-byte header). `p` is the 0-based **entry index** (the extraction index); each entry contributes one start-LBA word at `toc[p+2]`, so the formulas below reach into neighbouring entries' words (`toc[p+3]` is both entry `p`'s footprint end and entry `p+1`'s start). For entry index `p`:

```
start_lba             = toc[p + 2]                       // absolute LBA into PROT.DAT
indexed_size_sectors  = toc[p + 5] - toc[p + 3] + 4      // TOC-declared payload size
footprint_sectors     = toc[p + 3] - toc[p + 2]          // on-disc span to next entry
size_sectors          = max(indexed_size_sectors, footprint_sectors)
byte_offset           = start_lba * 0x800
size_bytes            = size_sectors * 0x800
```

`toc[p+5]` is the absolute LBA of entry `p+3` (an end-marker that aliases the next-entry's start), so `toc[p+5] - toc[p+3] + 4` recovers the indexed size in sectors.

**The TOC LBAs are `PROT.DAT`-relative, not absolute disc LBAs.** `byte_offset = start_lba * 0x800` is an offset *into* `PROT.DAT`, and the in-RAM TOC is raw `PROT.DAT` bytes, so the values are position-independent w.r.t. where `PROT.DAT` sits on the disc. Verified by diffing USA against the PAL discs: entry-0's TOC start LBA is identical on every disc despite `PROT.DAT` living at a different disc LBA per region. This is what makes a whole-sector entry-growth **relayout** tractable - growing an interior entry needs only an internal-TOC shift of the later entries' start-LBA words (at `PROT.DAT` byte `8 + (j+2)*4`), not a disc-wide cascade. See [disc.md § Full-ISO relayout](disc.md#full-iso-relayout).

### Trailing-overlay sectors (`indexed_size` vs `size`)

For ~24% of entries the on-disc contiguous range to the next entry's start LBA is **larger** than the indexed payload - the trailing sectors carry overlay content the SCUS boot loader reads via a multi-sector `ReadN` past the TOC-claimed end. PROT entry 899 is the canonical example: indexed payload is 14 sectors (28 KiB, the options menu), but the on-disc footprint is 74 sectors - the trailing 60 sectors are the title-screen overlay code (see [`subsystems/boot.md`](../subsystems/boot.md#title-overlay-source-on-disc)).

[`legaia_prot::archive::Archive`](../../crates/prot/src/archive.rs) exposes both views:

- `Entry::size_sectors` / `size_bytes` - full on-disc footprint (default).
- `Entry::indexed_size_sectors` / `indexed_size_bytes` - TOC-indexed payload only.
- `Archive::read_entry` reads the footprint; `Archive::read_entry_indexed` reads only the indexed sub-region.

Scene-side parsers were designed for the indexed view and use `read_entry_indexed` via [`ProtIndex::entry_bytes`](../../crates/engine-core/src/scene.rs). Asset-viewer / disc-browser consumers use the full footprint so trailing-overlay content is visible.

> **Historical note.**
>
> - An earlier Python proof-of-concept used `start_lba = toc[p+5] - toc[p+2]`. That subtraction actually computes the SIZE in sectors and was misinterpreted as the start LBA - under that math `start_lba` collapsed to a small relative offset within "block 0" of the file, and ~80% of PROT entries ended up reading the SAME few low-LBA byte ranges.
> - Anything written using that formula's outputs is artefacted; trust only post-`toc[p+2]` extractions.
> - The `size_sectors = max(indexed, footprint)` extension is a later correction (the indexed formula alone misses trailing-overlay sectors for entries like 899).

## In-RAM TOC

`SCUS_942.54` keeps a transformed copy of the TOC at RAM address `0x801C70F0`. Used at `FUN_8003E8A8` (the LBA resolver):

```c
start_lba    = TABLE[(idx + 2) * 4 + 0x801C70F0]
end_lba      = TABLE[(idx + 3) * 4 + 0x801C70F0]
size_sectors = end_lba - start_lba
```

The in-RAM copy is **raw `PROT.DAT` from byte 0** - `FUN_8003E4E8` reads the first three sectors of `PROT.DAT` into `0x801C70F0` at boot, header words included (byte-verified against a live save state's RAM). There is no transformation; but the **index space differs by 2** from the extraction's:
the extraction (`crates/prot`, and the `NNNN` in `extracted/PROT/NNNN_*.BIN`) builds its
`toc[]` array *after* the two file-header words, so extraction entry `p`'s `start_lba`
sits at file word `p + 4`, while the resolver's `TABLE[(idx + 2)]` is file word `idx + 2`.
Hence `resolver idx = extraction index + 2` - any PROT index recovered from a
`FUN_8003E8A8` argument must subtract 2 to land in extraction space (byte-verified for
the battle side-band files: TOC indices `0x37F`/`0x380` resolve to extraction entries
893/894, see [`summon-readef.md`](summon-readef.md)). Raw-TOC entries 0 and 1 cover the
pre-`init_data` boot-UI region (LBA 3..120) that extraction indexing leaves unindexed:
two TIM-packs holding the boot-resident **system-UI bundle** (menu-glyph atlas, sprite
sheets, cursor parts; uploaded once at boot by `FUN_800198E0` with flat-strip CLUT
semantics, parser `legaia_asset::system_ui_bundle`) - see
[`tim-pack.md` § boot-resident system-UI instance](tim-pack.md#boot-resident-system-ui-instance-raw-toc-entries-0-and-1).
[`CDNAME.TXT`](cdname.md)'s `#define` numbers are authored in this raw-TOC space - the
extractor's filename labels are shifted +2 relative to the content the defines name; see
[`cdname.md` § numbering space](cdname.md#numbering-space) for the evidence and the
consequential relabelings.

## Resolving entries by name vs by index

Two entry points:

- `FUN_8003E8A8` - index-based (consumed directly by the streaming loader and the dev-build sound branch).
- `FUN_8003E6BC` - path-based; resolves dev paths like `data\battle\efect.dat` or `h:\PROT\FIELD\<scene>\…` into an index via the CDNAME-driven name map, then delegates to the LBA resolver. Most retail-build code paths land here.

Names come from [`CDNAME.TXT`](cdname.md), which lives at the top level of the disc.

## Overlay loaders (parallel slots)

Two paired wrappers on top of `FUN_8003E8A8` + `FUN_8003E800` (async LBA-based loader) manage two **independently swappable** overlay slots. Both call `FUN_8003E8A8(param + 0x381)` - which, per the index-space note above, is **extraction entry `param + 0x37F`** (e.g. param 2 → 0897 field, 3 → 0898 battle, 4 → 0899 menu):

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
