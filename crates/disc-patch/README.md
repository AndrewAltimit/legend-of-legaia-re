# legaia-disc-patch

The write foundation every disc-patching feature goes through. It sits at the
bottom of the patcher family: the feature crates and
[`legaia-patcher`](../patcher/README.md) build on it, and it depends on none
of them.

- `disc` - `DiscPatcher`: same-size in-place PROT-entry edits on a
  user-supplied Mode 2/2352 image, through `legaia_iso::write`'s sector
  write-back with the EDC/ECC re-encoded, plus the `DMY.DAT` annex that gives
  grown records room, the XA sector readers / writers, and `boot_exe_name`
  (which executable `SYSTEM.CNF` boots). `disc::synth` builds the tiny
  synthetic discs the disc-free tests here and in `legaia-patcher` run
  against.
- `ppf` - the PPF 3.0 patch writer / reader (the portable deliverable).
- `space_ledger` - the ledger of the SCUS and overlay regions the code mods
  and the translation importer claim, with the runtime buffers no claim may
  overlap.
- `rng` - the version-stable `SplitMix64` generator every seeded pass draws
  from, so a published seed reproduces a run on any build.
- `man_compressed_budget` / `compress_within` - a scene MAN's stable
  compressed-stream budget and the greedy-then-optimal re-pack every MAN
  writer uses.
- `monster` - the monster-archive slot re-pack (`[u32 size][LZS]`, padded
  back to the fixed slot) every monster-record edit goes through, beside
  `DiscPatcher`'s slot reader and writer.
- `field_script` - the two field-VM system-flag op encoders (set, and
  test-and-skip) that a mod splicing new script into a scene MAN guards it
  with.

## What decides that a module sits here

It is a way of *writing to the disc* that more than one feature needs, and it
knows nothing about any feature. No MIPS (that is
[`legaia-code-hooks`](../code-hooks/README.md)), no audio or image codec, no
randomizer logic.

It depends on the disc and container crates (`legaia-iso`, `legaia-prot`,
`legaia-lzs`) and on `legaia-actor-formats` and `legaia-scene-formats` for
three format items: the monster
archive's slot stride, the player-file annex chain and the scene asset table.

No Sony bytes: this crate ships code only, and every test that needs real
game data is disc-gated.

## See also

- [`docs/tooling/randomizer-internals.md`](../../docs/tooling/randomizer-internals.md) -
  the write path (LZS re-pack, in-place PROT edit, EDC/ECC re-encode, PPF).
