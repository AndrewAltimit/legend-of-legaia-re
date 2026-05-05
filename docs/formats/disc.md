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
`PROT.DAT` is the main asset archive — see [PROT.DAT TOC](prot.md).
`SCUS_942.54` is the executable. Reverse-engineering instructions: [`tooling/ghidra.md`](../tooling/ghidra.md).
