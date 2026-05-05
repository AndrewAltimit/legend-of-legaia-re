# PROT.DAT / DMY.DAT TOC

`PROT.DAT` is the main asset archive — 1232 numbered entries containing every TIM, TMD, VAB, MES, ANM, MDT, DATA_FIELD streaming buffer, scene asset table, and runtime overlay. `DMY.DAT` is a sibling archive that turns out to be developer fixtures (memory-bus test pattern + paired random blobs); see [DMY.DAT](dmy.md).

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
start_lba    = toc[p + 2]                       // absolute LBA into PROT.DAT
size_sectors = toc[p + 5] - toc[p + 3] + 4
byte_offset  = start_lba * 0x800
size_bytes   = size_sectors * 0x800
```

`toc[p+5]` is the absolute LBA of entry `p+3` (an end-marker that aliases the next-entry's start), so `toc[p+5] - toc[p+3] + 4` recovers the size in sectors.

> **Historical note.** An earlier Python proof-of-concept used `start_lba = toc[p+5] - toc[p+2]`. That subtraction actually computes the SIZE in sectors and was misinterpreted as the start LBA — under that math `start_lba` collapsed to a small relative offset within "block 0" of the file, and ~80% of PROT entries ended up reading the SAME few low-LBA byte ranges. Anything written using that formula's outputs is artefacted; trust only post-`toc[p+2]` extractions.

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

- `FUN_8003E8A8` — index-based (consumed directly by the streaming loader and the dev-build sound branch).
- `FUN_8003E6BC` — path-based; resolves dev paths like `data\battle\efect.dat` or `h:\PROT\FIELD\<scene>\…` into an index via the CDNAME-driven name map, then delegates to the LBA resolver. Most retail-build code paths land here.

Names come from [`CDNAME.TXT`](cdname.md), which lives at the top level of the disc.
