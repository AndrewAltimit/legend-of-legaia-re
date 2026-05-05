# legaia-prot

`PROT.DAT` / `DMY.DAT` table-of-contents reader, `CDNAME.TXT` symbol map,
and the standalone TIM-pack subformat.

`PROT.DAT` is the master archive that holds most of the disc's game data:
characters, monsters, fields, dialog, sound. `DMY.DAT` shares its TOC
shape but contains developer fixtures (memory-bus test patterns and
random blobs).

## What it provides

- `archive` — TOC math and per-entry slicing. Two key invariants traced
  back to `FUN_8003e4e8` (the boot-time loader that reads the first three
  sectors of `PROT.DAT` into RAM at `0x801C70F0`):

  ```text
  start_lba = toc[p + 2]
  size      = toc[p + 5] - toc[p + 3] + 4
  ```

  This was the fix for an early off-by-one in Sam Ste's Python unpacker
  that read `size` as `start_lba`; see
  [`docs/formats/prot.md`](../../docs/formats/prot.md) for the full
  derivation.

- `cdname` — parser for `CDNAME.TXT`, the human-readable symbol map.
  `#define name N` lines mark **block starts**: every entry from index
  `N` up to (but not including) the next `#define` inherits the same
  name. Don't trust labels alone — `vab_01` doesn't actually contain
  VAB headers, and `move_program_no` doesn't match the consumer-derived
  move-table layout. The label is a hint; verify with the loader-call
  constant or the file's magic bytes.

- `timpack` — the standalone TIM-pack subformat used by some PROT
  entries (notably `tim.dat`). Header is
  `(magic_lo, magic_hi, count<16, marker=0x01)` followed by word offsets;
  `byte_offset = word_index * 4 + 4`. Distinct from `legaia-asset::pack`
  (DATA_FIELD streaming) and from the field-pack / effect-bundle
  formats handled in `legaia-asset`.

## CLI

```bash
prot-extract list    <PROT.DAT> [--cdname <CDNAME.TXT>]
prot-extract extract <PROT.DAT> <out_dir> [--cdname <CDNAME.TXT>]
```

Names from `CDNAME.TXT` propagate to the extracted filenames
(`0865_battle_data.BIN`, etc.) so downstream tools stay self-describing.

## See also

- [`docs/formats/prot.md`](../../docs/formats/prot.md) — TOC math, CDNAME
  inheritance rules, the three pack-format gotchas.
- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  — runtime path: boot loader → in-RAM TOC → `FUN_8003e8a8` LBA resolver.
