# In-RAM STR FMV file table

The cutscene / MDEC overlay's compact lookup table for STR FMV files. The retail engine loads this 6-entry table into RAM during overlay residency (around `0x801CAE40`); each entry is the minimum a libcd-driven STR player needs - filename, BCD MSF for `CdControl(CdlSetloc, ...)`, and file size for the chunk-read budget.

A second copy of the same six files appears nearby in full ISO9660 directory-record form (`0x801CCA80`); the compact form is the fast lookup, the directory copies are presumably retained for `CdReadDir`-style validation. Only the compact form is parsed here.

## Confidence

**Inferred — structural reading from a single live capture.** Pinned from the `mc1` save state during FMV playback. The 24-byte stride + libcd MSF shape is consistent across all six entries; the compact table is the structure the FMV overlay reads when seeking the disc head.

## Layout

```text
+0x00  char[12]  filename     "MV1.STR;1\0..." (null-padded; libcd path shape)
+0x0C  u32       reserved     zero across all observed entries
+0x10  u32       bcd_msf      byte 0 = BCD minute, 1 = BCD second,
                              2 = BCD frame, 3 = zero
+0x14  u32       size         file size in bytes (LE)
```

`bcd_msf` is the standard libcd `CdlLOC` representation: each byte is two BCD digits (high nibble = tens, low nibble = ones). The byte order packs into the u32 such that reading `bcd_msf` as 4 LE bytes recovers `[M, S, F, 0]` directly.

Convert to absolute LBA with the standard CD identity:

```text
LBA = ((M * 60) + S) * 75 + F - 150
```

The `-150` accounts for the 2-second pre-gap.

## What's in the captured table

| Idx | Name        | M:S.F (decimal) | LBA      | Size (bytes) |
|----:|-------------|-----------------|---------:|-------------:|
| 0   | `MV1.STR;1` | 53:51.33        | 242,208  |    5,099,520 |
| 1   | `MV2.STR;1` | 68:24.34        | 307,759  |   18,104,320 |
| 2   | `MV3.STR;1` | 58:22.36        | 262,711  |    7,045,120 |
| 3   | `MV4.STR;1` | 48:08.37        | 216,612  |   13,393,920 |
| 4   | `MV5.STR;1` | 63:35.38        | 286,063  |   13,701,120 |
| 5   | `MV6.STR;1` | 41:14.19        | 185,419  |   14,811,136 |

## What this gives us

- An in-RAM cross-check for the disc-side ISO9660 walk (`legaia_iso`) - any drift between the two representations is a corpus issue.
- The MSF↔LBA conversion needed to look up the same files via the disc reader without going back through the directory.
- A residency signature: the compact table's first entry name (`MV1.STR;1`) at `0x801CAE40` is the cheap "is the FMV overlay loaded" check.

## What this doesn't tell us

- Which **scene** triggers which MV. The scene-to-MV mapping is encoded in the field-VM script for each FMV-bearing scene (see [`subsystems/cutscene.md`](../subsystems/cutscene.md#mid-game-fmv-bearing-field-scenes)), not in this table.
- The runtime XA channel selector for multi-channel STR containers (`\DATA\MOV.STR;1`, which is referenced separately in the overlay's path table).

## Rust API

```rust
use legaia_asset::str_fmv_table;

// Slice the compact table out of a captured main-RAM image.
let off = (0x801CAE40 - 0x80000000) as usize;
let bytes = &main_ram[off..off + 6 * str_fmv_table::ENTRY_STRIDE];

// Parse 6 entries; zero-filled trailing slots are dropped silently.
let entries = str_fmv_table::parse_entries(bytes, 6).expect("table parses");
for entry in &entries {
    println!(
        "{} at LBA {} ({} bytes)",
        entry.name,
        entry.lba(),
        entry.size,
    );
}

// Cheap signature check.
assert!(str_fmv_table::looks_like_str_fmv_table(bytes));
```

## Provenance

| Subject | Source |
|---|---|
| Compact-table layout | `mc1` capture; `legaia_asset::str_fmv_table` |
| BCD MSF semantics | PSX-SPX libcd `CdlLOC` definition |
| ISO9660 directory copy | `mc1` capture at `0x801CCA80` |
| Residency signature | `legaia_engine_core::capture_observations::str_fmv_overlay::is_resident` |
