# PSX Mode2/2352 disc geometry

The Legend of Legaia disc is a standard PlayStation Mode2/Form1 CD. Sectors are 2352 bytes; only the 2048-byte user-data slice is meaningful for ISO9660 / file content.

Implementation: `crates/iso/src/raw.rs`.

## Sector layout

| Bytes | Meaning |
|---|---|
| 0..12 | Sync pattern `00 FF FF FF FF FF FF FF FF FF FF 00` |
| 12..16 | Header (M:S:F address + mode byte) |
| 16..24 | Subheader (file/channel/submode/coding) |
| 24..2072 | **User data** (2048 bytes) |
| 2072..2352 | EDC + ECC error correction |

The `iso` crate's `RawDisc::read_sector(lba)` returns just the 2048-byte user data slice. `read_user_data(lba, count, buf)` reads a contiguous run.

```rust
const SECTOR_SIZE: usize = 2352;
const USER_DATA_OFFSET: usize = 24;
const USER_DATA_SIZE: usize = 2048;
```

# ISO9660 walk

Implementation: `crates/iso/src/iso9660.rs`.

Standard ISO9660:
- Primary Volume Descriptor at LBA 16.
- Root directory record at PVD offset 156.
- Each directory record begins with a length byte; records pad to even lengths.

The walker is iterative (not recursive) and yields stable-sorted file paths. The Legend of Legaia (USA) disc produces 45 files:

```
CDNAME.TXT  DMY.DAT  PROT.DAT  SCUS_942.54  SYSTEM.CNF
MOV/MV1.STR ... MV6.STR
XA/XA1.XA  ... XA21.XA
```

`MOV/MV*.STR` are PSX MDEC video streams (delegate to public decoders like jPSXdec).
`XA/XA*.XA` are XA-ADPCM audio; `crates/xa` decodes the format spec but the on-disc files use a non-standard interleave.
`PROT.DAT` is the main asset archive - see [PROT.DAT TOC](prot.md).
`SCUS_942.54` is the executable. Reverse-engineering instructions: [`tooling/ghidra.md`](../tooling/ghidra.md).

## Full-ISO relayout

Growing a file (specifically `PROT.DAT`) by whole sectors - the operation the
official PAL discs did at mastering to fit longer localized dialog - requires
shifting every file after it and rewriting each on-disc LBA reference. Implemented
in [`legaia_iso::relayout`](../../crates/iso/src/relayout.rs) (generic ISO9660 +
ECMA-130; embeds no game bytes).

### The LBA reference graph

Two facts make the relayout safe (proven by diffing USA against all three PAL
discs, which are themselves a per-entry +1-sector relayout of the same structure):

1. **`PROT.DAT`'s internal TOC is PROT.DAT-relative, not absolute disc LBAs.**
   Entry-0's TOC start LBA is identical on every disc even though `PROT.DAT` sits
   at a different disc LBA per region. So growing an interior entry needs only an
   internal-TOC start-LBA shift (see [prot.md](prot.md)), not a disc-wide cascade.
2. **No file is located by a hardcoded absolute LBA in the executable.** Every file
   is found by ISO9660 name/directory lookup (`PROT.DAT` via `FUN_8003E4E8`,
   STR/XA via the path opener) - no post-`PROT.DAT` file's disc LBA appears as a
   little-endian literal in any USA or PAL executable.

When `PROT.DAT` grows by `G` sectors, the full cascade reduces to one rule:
**every ISO9660 LBA value `> prot_lba` gains `+G`; `PROT.DAT`'s directory-record
size gains `+G*2048`; the PVD volume-space size gains `+G`.** The structures:

| Structure | Location | Edit |
|---|---|---|
| PVD volume space | LBA 16, off 80 (LE) + 84 (BE) | `+= G` |
| Path table (LE @18 + BE @20, incl. optional copies @19/@21) | dir extents | extents `> prot_lba` `+= G` |
| Directory records (root + every subdirectory extent) | rec off `+2` LBA / `+10` size | LBA `> prot_lba` `+= G`; `PROT.DAT` size `+= G*2048` |
| PROT internal TOC | `PROT.DAT` byte `8+(j+2)*4` | entries after a grown one `+= cumulative G` (PROT-relative) |

Subdirectory extents that live **after** `PROT.DAT` (on the retail disc `MOV` and
`XA`) relocate too, so their self `.` record and file records are patched at the
extent's new position.

### Per-sector mechanics

Every sector after `PROT.DAT` is relocated `+G`: its 12-byte sync + 4-byte header
are rewritten so the stored MSF address is `BCD(lba+150)` for the new position.
Form 1 EDC/ECC are computed with the header treated as zero (see
[`write`](../../crates/iso/src/write.rs)), so a **pure relocation needs no EDC/ECC
recompute** - only the MSF header changes. Form 2 (XA) sectors carry no ECC.
EDC/ECC are recomputed only for sectors whose 2048-byte user data changes (the
rebuilt `PROT.DAT` payload, the PVD, path tables, and the directory extents).

The disc-level operation preserves the PROT entry index space (no entries
added/removed), so index-keyed same-size edits still resolve after a relayout.
Consumer: `DiscPatcher::grow_prot_entries` + the `translate import --allow-relayout`
localization path (see [pal-localizations.md](../tooling/pal-localizations.md)).

## See also

- [PROT.DAT TOC](prot.md) - the in-disc container index.
- [`tooling/extraction.md`](../tooling/extraction.md) - the extraction pipeline that walks this geometry.
