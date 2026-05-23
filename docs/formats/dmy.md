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

## See also

- [PROT TOC](prot.md) - the sibling container with real game content.
- [Pochi-fill slots](pochi.md) - the other dev-placeholder pattern in the corpus.
