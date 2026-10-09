//! Capture observation: STR-FMV overlay file/path-table residency windows.

/// Overlay residency window (inclusive lower, exclusive upper).
pub const OVERLAY_WINDOW: (u32, u32) = (0x801C0000, 0x80200000);

/// Historical "compact FMV file table" window - actually a phase-shifted
/// read of libcd's `CdSearchFile` directory cache (PsyQ `CdlFILE`
/// records starting at `0x801CAE08`; see `docs/formats/str-fmv-table.md`).
pub const COMPACT_TABLE_ADDR: u32 = 0x801CAE40;

/// ISO9660-shape directory record copies. 56 bytes per entry,
/// 6 entries. The publisher tag `"USA"` appears at +0x17 of each.
pub const ISO_DIRECTORY_TABLE_ADDR: u32 = 0x801CCA80;

/// Packed path string table. Nine null-padded paths covering MOV.STR,
/// MOV15.STR, MV1A.STR, plus MV6..MV1 in reverse order.
pub const PATH_TABLE_ADDR: u32 = 0x801CE810;

/// Packed post-FMV return-scene label table.
pub const MID_GAME_LABELS_ADDR: u32 = 0x801CE8AC;

/// CDNAME-shape post-FMV return-scene labels in table order. After a
/// mid-game FMV finishes, the master dispatch (`FUN_801CEA3C`) copies
/// the label for the just-played `fmv_id` into the next-scene name
/// global `0x80084548` (+ spawn/door word `0x80084540`) - these are
/// the scenes each FMV *returns* to, not the trigger-scene set.
pub const MID_GAME_LABELS: [&str; 7] = [
    "town0b", "map01", "chitei2", "map02", "jou", "uru2", "town0e",
];

/// Six MV file basenames in canonical disc order (matches both the
/// compact table and the ISO9660 directory copies).
pub const MV_BASENAMES: [&str; 6] = [
    "MV1.STR", "MV2.STR", "MV3.STR", "MV4.STR", "MV5.STR", "MV6.STR",
];

/// Detect whether the FMV overlay is residency-resident in `main_ram`.
/// The check looks for the compact table's first entry name (`MV1.STR`)
/// at the pinned address - if present, the overlay is loaded.
pub fn is_resident(main_ram: &[u8]) -> bool {
    let off = (COMPACT_TABLE_ADDR - 0x80000000) as usize;
    let head = match main_ram.get(off..off + 8) {
        Some(b) => b,
        None => return false,
    };
    head.starts_with(b"MV1.STR")
}
