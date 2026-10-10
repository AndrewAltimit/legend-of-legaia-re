# DMY.DAT - developer fixtures

`DMY.DAT` is the second archive at the disc root, beside `PROT.DAT`. It carries developer fixtures, not game content, and no retail code loads it. Its practical value is as **spare room**: it is a large run of sectors at the end of the disc that the patcher can write into without moving anything.

## Layout

The container is the [`PROT.DAT` TOC format](prot.md): the same header and the same start-LBA table, read by the same `legaia_prot::Archive`.

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | unused | Zero | Confirmed |
| `+0x04` | u32 | `lba_row_count - 1` | 15: 16 TOC rows, resolving to 13 entries | Confirmed |
| `+0x08` | u32 x 16 | `toc[]` | Start LBAs relative to `DMY.DAT`; an entry's size is the gap to the next row | Confirmed |

## Contents

1. A memory-bus test pattern (alternating bit-walk values of the kind used to validate RAM).
2. Paired random blobs (Inferred: test inputs for the audio / video pipelines).

No part of the file is referenced by retail gameplay code. The categorize pipeline skips it.

## Use as spare room

It is the disc's **spare room**: 18,054 Form 1 sectors at the end of the disc (LBA 180228 on the USA image) that nothing loads. The patcher's equipment editor parks rebuilt player-file records there when they outgrow `PROT.DAT` - the PROT entry keeps its header and its descriptor offsets reach into `DMY.DAT` (see [battle-data-pack.md](battle-data-pack.md#parking-the-records-in-dmydat)). A bump-allocator marker (`LGAX`, version, sectors used) in the file's **last** sector records what an earlier patch already placed; the first sector (its own TOC) is left alone so the archive still parses.

## See also

- [PROT TOC](prot.md) - the sibling container with real game content.
- [Pochi-fill slots](pochi.md) - the other dev-placeholder pattern in the corpus.
