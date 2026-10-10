# PSX Mode2/2352 disc geometry

The Legend of Legaia disc is a standard PlayStation CD-ROM XA disc. Every sector on it is 2352 raw bytes, and a `.bin` image is those sectors back to back. Game files (the executable, `PROT.DAT`, `CDNAME.TXT`) sit in **Form 1** sectors, where 2048 bytes are file data and the rest is addressing and error correction. Streamed audio and video (`XA/`, `MOV/`) use **Form 2** sectors, which trade the error-correction bytes for a larger payload. Above the sectors sits an ordinary ISO9660 filesystem, so every file is found by name.

Implementation: [`crates/iso`](../../crates/iso/README.md) - `raw.rs` (sector reads), `iso9660.rs` (filesystem walk), `write.rs` (EDC/ECC re-encode), `relayout.rs` (whole-disc relayout).

## Sector layout

A sector's logical block address (LBA) is its index in the image: sector `lba` starts at image byte `lba * 2352`.

```
Mode 2 Form 1 (data files)                         2352 bytes
+------+--------+-----------+----------------------+-----+---------------+
| sync | header | subheader |      user data       | EDC |  ECC (P + Q)  |
|  12  |   4    |     8     |        2048          |  4  |      276      |
+------+--------+-----------+----------------------+-----+---------------+
0      12       16          24                     2072  2076        2352

Mode 2 Form 2 (XA audio, STR video)
+------+--------+-----------+--------------------------------------+-----+
| sync | header | subheader |              payload                 | EDC |
|  12  |   4    |     8     |               2324                   |  4  |
+------+--------+-----------+--------------------------------------+-----+
0      12       16          24                                     2348
```

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `0x000` | 12 | sync | `00 FF FF FF FF FF FF FF FF FF FF 00` | Confirmed |
| `0x00C` | 4 | header | Minute:second:frame address in BCD (`lba + 150` frames), then the mode byte (`2`) | Confirmed |
| `0x010` | 8 | subheader | `file, channel, submode, coding`, stored twice. Submode bit `0x20` marks Form 2 | Confirmed |
| `0x018` | 2048 | user data (Form 1) | The bytes ISO9660 and every file parser see | Confirmed |
| `0x818` | 4 | EDC (Form 1) | Checksum over `0x010..0x818` | Confirmed |
| `0x81C` | 276 | ECC (Form 1) | P parity at `0x81C`, Q parity at `0x8C8`; computed with the header treated as zero | Confirmed |
| `0x018` | 2324 | payload (Form 2) | XA sound groups or STR video; no ECC | Confirmed |
| `0x92C` | 4 | EDC (Form 2) | Checksum over `0x010..0x92C` | Confirmed |

`RawDisc::read_sector(lba)` returns the 2048-byte user-data slice, `read_user_data(lba, count, buf)` reads a contiguous run, and `read_raw_sector(lba)` returns all 2352 bytes (what the XA demuxer needs).

```rust
const SECTOR_SIZE: usize = 2352;
const USER_DATA_OFFSET: usize = 24;
const USER_DATA_SIZE: usize = 2048;
```

## ISO9660 walk

The filesystem is standard ISO9660:

- Primary Volume Descriptor (PVD) at LBA 16.
- Root directory record at PVD offset 156.
- Each directory record begins with a length byte; records pad to even lengths. The extent LBA is at record offset `+2` and the byte size at `+10`.

The walker is iterative (not recursive) and yields stable-sorted file paths. The USA disc holds 45 files:

```
CDNAME.TXT  DMY.DAT  PROT.DAT  SCUS_942.54  SYSTEM.CNF
MOV/MV1.STR ... MV6.STR
XA/XA1.XA  ... XA34.XA
```

| File | What it is | Page |
|---|---|---|
| `SCUS_942.54` | The executable | [`tooling/ghidra.md`](../tooling/ghidra.md) |
| `PROT.DAT` | The main asset archive | [`prot.md`](prot.md) |
| `DMY.DAT` | Developer fixtures in the same container shape | [`dmy.md`](dmy.md) |
| `CDNAME.TXT` | Name map for `PROT.DAT` indices | [`cdname.md`](cdname.md) |
| `MOV/MV*.STR` | MDEC video in the **Iki** bitstream, not STRv2 | [`crates/mdec`](../../crates/mdec/README.md) |
| `XA/XA*.XA` | XA-ADPCM audio in standard Form 2 sectors, demuxed per `(file_no, ch_no)` | [`xa.md`](xa.md) |

The XA files need the raw 2352-byte sectors: an extraction that keeps only the 2048-byte Form 1 slice truncates every sound sector ([`xa.md`](xa.md#non-standard-interleave---what-it-is-and-isnt)).

## Full-ISO relayout

Growing a file (specifically `PROT.DAT`) by whole sectors - what the official PAL
discs do to fit longer localized dialog - requires
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
| PROT internal TOC | `PROT.DAT` byte `8+(j+2)*4` | entries after a grown one `+= cumulative G` (PROT-relative); every non-zero word past the last entry's start (its end bound + the monotone tail) `+= total G`, since the reader sizes an entry by the gap to the next word and an unshifted bound drops the last entry |

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
