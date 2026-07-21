# legaia-prot

`PROT.DAT` / `DMY.DAT` table-of-contents reader, `CDNAME.TXT` symbol map,
and the standalone TIM-pack subformat.

`PROT.DAT` is the master archive that holds most of the disc's game data:
characters, monsters, fields, dialog, sound. `DMY.DAT` shares its TOC
shape but contains developer fixtures (memory-bus test patterns and
random blobs).

## What it provides

- `archive` - TOC math and per-entry slicing. Two key invariants traced
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

- `cdname` - parser for `CDNAME.TXT`, the human-readable symbol map.
  `#define name N` lines mark **block starts**: every entry from index
  `N` up to (but not including) the next `#define` inherits the same
  name.

  **The define numbers are raw in-RAM TOC indices, not extraction
  indices.** The boot loader copies `PROT.DAT` verbatim (8-byte header
  included) into `0x801C70F0`, so `raw index = extraction index + 2`, and
  the content `#define name N` names lives at extraction entry `N − 2`.
  `block_for_extraction_index` resolves the retail-space name for an
  extraction index. This shift is what the old "CDNAME labels lie"
  reports (`vab_01` seemingly without VAB headers, `move_program_no` not
  matching the move-table layout) actually were - the labels are sound;
  the two index spaces were being crossed. Say which space you mean, and
  still confirm an attribution with the loader-call constant or the
  file's magic bytes.

- `runtime_toc` - queries against the in-RAM TOC copy the boot loader
  installs at `0x801C70F0`. `entry_sector_span` is the port of
  `FUN_8003E68C` (`TABLE[i+3] - TABLE[i+2]`), the entry's on-disc sector
  footprint - the same `next_start - start_lba` quantity `archive`
  computes when extending an entry over a trailing gap, and *not* the
  TOC-indexed payload size. The module also pins the `+2` word skew
  between the RAM word array and `Archive::toc`.

- `timpack` - the standalone TIM-pack subformat used by some PROT
  entries (notably `tim.dat`). Header is
  `(magic_lo, magic_hi, count<16, marker=0x01)` followed by word offsets;
  `byte_offset = word_index * 4 + 4`. Distinct from `legaia-asset::pack`
  (DATA_FIELD streaming) and from the field-pack / effect-bundle
  formats handled in `legaia-asset`.

## CLI

```bash
prot-extract list    <PROT.DAT> [--cdname <CDNAME.TXT>]
prot-extract locate  <PROT.DAT> <offset> [--in-entry N] [--cdname <CDNAME.TXT>]
prot-extract extract <PROT.DAT> <out_dir> [--cdname <CDNAME.TXT>] [--clamp-footprint]
```

Names from `CDNAME.TXT` propagate to the extracted filenames
(`0865_battle_data.BIN`, etc.) so downstream tools stay self-describing.

`list` shows each entry's declared size next to its true `footprint` (the
sector span to the next entry) and flags an `OVR` when the extracted `.BIN`
over-reads that footprint. `locate` is the reverse lookup: given a PROT.DAT
byte offset (or an in-`.BIN` offset via `--in-entry`), it names the entry
whose footprint actually owns those bytes and warns when the offset lands in
an over-read tail. The footprint/owner logic is `legaia_prot::locate`
(unit-tested); it shares the `next_start - start` span arithmetic with
`runtime_toc`.

## See also

- [`docs/formats/prot.md`](../../docs/formats/prot.md) - TOC math, CDNAME
  inheritance rules, the three pack-format gotchas.
- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  - runtime path: boot loader → in-RAM TOC → `FUN_8003e8a8` LBA resolver.
