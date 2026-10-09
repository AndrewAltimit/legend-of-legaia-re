# legaia-disc-patch

The foundation every disc-patching feature writes through, split out of
[`legaia-patcher`](../patcher/README.md) so the feature crates above it build
against a small, stable base:

- `disc` - `DiscPatcher`: same-size in-place PROT-entry edits on a
  user-supplied Mode 2/2352 image, through `legaia_iso::write`'s sector
  write-back with the EDC/ECC re-encoded, plus the `DMY.DAT` annex that gives
  grown records room. `disc::synth` builds the tiny synthetic discs the
  disc-free tests here and in `legaia-patcher` run against.
- `ppf` - the PPF 3.0 patch writer / reader (the portable deliverable).
- `space_ledger` - the ledger of the SCUS and overlay regions the code mods
  and the translation importer claim, with the runtime buffers no claim may
  overlap.
- `man_compressed_budget` / `compress_within` - a scene MAN's stable
  compressed-stream budget and the greedy-then-optimal re-pack every MAN
  writer uses.

`legaia-patcher` re-exports `disc`, `ppf` and `space_ledger` at their old
paths, so `legaia_patcher::disc::DiscPatcher` keeps resolving.

No Sony bytes: like the patcher, this crate ships code only, and every test
that needs real game data is disc-gated.

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) - the
  patcher's feature and code-injection reference.
