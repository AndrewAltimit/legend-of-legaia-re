# DMY.DAT - developer fixtures

Sibling archive to `PROT.DAT` at the disc root. Despite the name suggesting "dummy data" or a parallel asset bank, DMY.DAT carries developer fixtures, not real game data.

## Contents

Three discernable patterns:

1. A memory-bus test pattern (alternating bit-walk values used to validate RAM during development).
2. Paired random blobs (used as test inputs for the audio / video pipelines).
3. A small offset table at the start.

No part of the file is referenced by retail gameplay code; the file is included on disc but never loaded.

## Treatment

Skipped by the categorize pipeline. Not interesting for either preservation or the engine port.

It is, however, the disc's **spare room**: 18,054 Form 1 sectors at the end of the disc (LBA 180228 on the USA image) that nothing loads. The patcher's equipment editor parks rebuilt player-file records there when they outgrow `PROT.DAT` - the PROT entry keeps its header and its descriptor offsets reach into `DMY.DAT` (see [battle-data-pack.md](battle-data-pack.md#parking-the-records-in-dmydat)). A bump-allocator marker (`LGAX`, version, sectors used) in the file's **last** sector records what an earlier patch already placed; the first sector (its own TOC) is left alone so the archive still parses.

## See also

- [PROT TOC](prot.md) - the sibling container with real game content.
- [Pochi-fill slots](pochi.md) - the other dev-placeholder pattern in the corpus.
