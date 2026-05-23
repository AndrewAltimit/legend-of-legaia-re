# CDNAME.TXT - entry name map

A plain-text file at the disc root that maps PROT.DAT entry indices to human-readable names. One C-style `#define` per line:

```
#define init_data 0
#define gameover_data 1
#define town01 3
#define town0b 12
#define town0c 21
...
#define vab_01 1072
```

Implementation: `crates/prot/src/cdname.rs`.

## Block-start semantics

Each `#define name N` marks **the start of a block** of N entries. Subsequent PROT entries inherit the name of the most recent block:

- entry 3 → block `town01`
- entry 11 → block `town01` (since `town0b` starts at 12)
- entry 12 → block `town0b`

`prot-extract` uses these names to produce filenames like `0148_retock.BIN`.

## Block names can be misleading

A block name describes the developer's organisation, not necessarily the runtime semantics of every entry inside the block. Several caveats are worth remembering:

- The `vab_01` cluster (1072..1194) really does carry VAB headers - 119 of 123 entries match the [scene-VAB-prefixed streaming](scene-bundles.md) shape, the standard distributed-bank layout.
- The `0972/0973_move_program_no` entries are flat 128-byte record arrays that **don't** match the `move.mdt` runtime buffer layout the consumer expects (see [MDT](mdt.md)) - same name, different structure.
- The `xxx_dat` block (901..969) holds runtime overlay code blobs (see [MIPS overlay](mips-overlay.md) and [overlay pointer-table](overlay-ptr-table.md)).
- The `bat_back_dat 895` slot is the boot-time `init.pak` bundle, not "battle background data" — it carries the four publisher-logo TIMs (PROKION / Contrail / SCEA Presents / WARNING) plus the boot-time debug-string pool. See [`boot.md` § Boot init.pak](../subsystems/boot.md#boot-initpak-prot-0895).

When the block name conflicts with what the bytes actually look like, trust the bytes. Re-derive structure from the leading magic + the loader-call constant in SCUS, not from the CDNAME label.

## Per-scene asset reservations

Most scene blocks reserve 6–8 PROT slots for asset variants. Unused slots get filled with the dev placeholder pattern documented in [pochi-filler](pochi.md). The `edstati3` block (likely "ending station 3", possibly cut content) is almost entirely pochi-filled.

## See also

- [PROT TOC](prot.md) - the index this name map labels.
- [Disc layout](disc.md) - the on-disc geometry that holds both files.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader that resolves CDNAME labels to PROT entries.
- [`subsystems/boot.md`](../subsystems/boot.md) - the boot sequence that loads the TOC.
