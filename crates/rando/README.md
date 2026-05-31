# legaia-rando

Randomizer / disc patcher for a user-supplied Legend of Legaia disc.

Edits gameplay data on the user's own `.bin` and writes it back: monster item
drops today, random encounters and treasure contents as that work lands. It is
Track-1-adjacent tooling — it does **not** touch the clean-room engine — and it
ships only code: no game bytes are embedded or committed, and every test that
needs real data is disc-gated.

See [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) for the
full design.

## Modules

| Module | Role |
|---|---|
| `rng` | Version-stable `SplitMix64` PRNG. A published seed always reproduces a run, independent of any external generator's algorithm (the first output for seed 0 is pinned by a test). |
| `items` | The valid item-id pool from the SCUS item-name table (`legaia_asset::item_names`), so a randomized drop is always an item the game has a name and handler for. |
| `drops` | The drop-table planner. `plan_drops` reassigns the monsters that currently drop something, in `Shuffle` mode (redistribute the existing drops — preserves the economy) or `Random` mode (draw from the item pool). Deterministic in `(current drops, pool, seed, mode)`. |
| `monster` | Re-pack a monster slot in the `battle_data` archive (PROT 867). `repack_slot` decompresses the `0x14000`-byte slot, hands the decoded record to an in-place mutator, recompresses with `legaia_lzs::compress`, and zero-pads back to the original slot size so no offset moves. `set_drop` is the drop-id + chance wrapper. |
| `disc` | `DiscPatcher`: own a mutable disc image, locate PROT.DAT + read its TOC, and apply same-size PROT-entry edits via the Mode 2/2352 sector write-back in `legaia_iso::write`. `patch_monster_slot` / `monster_slot` are the `battle_data` helpers; `patch_prot_entry` is the generic form. |

## How an edit reaches the disc

A PROT-entry-relative offset maps to the PROT.DAT-logical offset
`start_lba[entry] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. Every edit is same-size and in place — no LBA, TOC, or
directory record moves.

## Tests

- Synthetic (CI): planner determinism + shuffle-preserves-the-multiset, RNG
  stability, surgical `set_drop`, and a patch round-trip through a hand-built
  Mode 2 disc with a real-format PROT TOC.
- Disc-gated: edit real monster drops and a real monster slot on a scratch copy
  of the disc, asserting the edit is surgical (drop applied; everything else
  byte-identical) and the patched sectors stay EDC/ECC-valid.

```bash
cargo test -p legaia-rando                                   # synthetic only
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" \
    cargo test -p legaia-rando                               # + disc-gated
```

The patched image is never written to disk by the tests; it lives only in
memory. A patched `.bin` contains Sony data and must never be committed.

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) — design + the patch chain.
- [`crates/lzs`](../lzs/README.md) — the LZS encoder (`compress`) re-packing relies on.
- [`crates/iso`](../iso/README.md) — Mode 2/2352 sector write-back (`write` module).
- [`crates/asset`](../asset/README.md) — the monster-archive + item-name parsers it edits.
