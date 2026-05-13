//! World-map quick-travel menu data parsed out of `SCUS_942.54`.
//!
//! Two static tables drive the in-game world-map landmark menu:
//!
//! - `DAT_80073A98` - 6-byte placement records, walked by `FUN_80030628`
//!   case `0x19` and resolved by `FUN_8002FF8C` for the `0x8XXX` string-id
//!   range. Terminator: a record whose byte\[0] is `0xFF`. The walker
//!   dedupes consecutive records that share the same name index, and a
//!   record is only visible when the system-flag at index
//!   `(byte[1] + 0x20)` is set in the fourth flag bank
//!   (`FUN_8003ce64`, `DAT_80086D70`). Schema:
//!
//!   ```text
//!   offset  size  field
//!   0x00    u8    name_idx        index into the name table (32-byte stride
//!                                 from `DAT_80073B18`); `0xFF` = terminator
//!   0x01    u8    discovery_flag  bit index (offset +0x20) in the
//!                                 system-flag bank queried by FUN_8003ce64
//!   0x02    u16   scene_id        destination scene id (LE)
//!   0x04    u8    menu_x          x position on the world-map menu screen
//!   0x05    u8    menu_y          y position on the world-map menu screen
//!   ```
//!
//! - `DAT_80073B18` - 32-byte stride NUL-terminated ASCII landmark names
//!   ("Rim Elm", "Drake Castle", ... 16 entries through "Soren Camp").
//!   The 16th name (index `0x0E`, "Conkram") has no placement record - it
//!   only appears in cutscenes.
//!
//! See [`ghidra/scripts/funcs/8002ff8c.txt`](https://github.com/.../ghidra/scripts/funcs/8002ff8c.txt)
//! and [`8002c69c.txt`](.../8002c69c.txt) for the consumer.

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

/// PS-X EXE header size (skipped past in the SCUS file to reach loaded code).
pub const PSX_EXE_HEADER: usize = 0x800;
/// SCUS_942.54 loads at this PSX virtual address.
pub const SCUS_LOAD_ADDR: u32 = 0x8001_0000;

/// RAM address of the 6-byte placement-record table base.
pub const PLACEMENT_TABLE_ADDR: u32 = 0x8007_3A98;
/// RAM address of the 32-byte-stride name table base.
pub const NAME_TABLE_ADDR: u32 = 0x8007_3B18;
/// Number of name entries in the table (16; the 16th = "Conkram" has no
/// placement record and only appears in cutscenes).
pub const NAME_COUNT: usize = 16;
/// Stride of the name table.
pub const NAME_STRIDE: usize = 0x20;
/// Stride of the placement-record table.
pub const PLACEMENT_STRIDE: usize = 6;
/// Hard cap on placement walk (matches the walker's `& 0x3FF` clamp; real
/// data is ~20 records terminated by `0xFF`).
pub const PLACEMENT_MAX: usize = 64;

/// One placement record from `DAT_80073A98`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacementRecord {
    /// Zero-based record index in the table (before terminator).
    pub index: u32,
    /// Index into the landmark-name table.
    pub name_idx: u8,
    /// Bit index (offset +0x20) in the fourth flag bank; the in-game menu
    /// only shows this record once the flag is set.
    pub discovery_flag: u8,
    /// Destination scene id (LE u16) loaded when the player picks this entry.
    pub scene_id: u16,
    /// On-screen X coordinate of the marker in the menu world-map view.
    pub menu_x: u8,
    /// On-screen Y coordinate of the marker.
    pub menu_y: u8,
}

/// Parsed world-map menu data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorldmapMenu {
    /// Landmark names in order; entries with no placement record (e.g.
    /// "Conkram" at index 0x0E) are still returned here so the index is
    /// stable.
    pub names: Vec<String>,
    /// Placement records walked from the table base up to the `0xFF`
    /// terminator. Records that exceed `name_idx >= names.len()` are
    /// dropped.
    pub placements: Vec<PlacementRecord>,
}

/// Slice `len` bytes out of `scus` starting at PSX virtual address `ram_addr`.
/// Uses the standard PS-X EXE `t_addr` at offset `0x18` to compute the
/// file-offset.
pub fn read_scus_at(scus: &[u8], ram_addr: u32, len: usize) -> Result<&[u8]> {
    if scus.len() < PSX_EXE_HEADER || &scus[0..8] != b"PS-X EXE" {
        bail!("SCUS bytes don't start with `PS-X EXE` magic");
    }
    let t_addr = u32::from_le_bytes(
        scus[0x18..0x1C]
            .try_into()
            .map_err(|_| anyhow!("SCUS too short for t_addr"))?,
    );
    let ram_off = ram_addr
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow!("RAM 0x{ram_addr:08X} below t_addr 0x{t_addr:08X}"))?;
    let file_off = PSX_EXE_HEADER
        .checked_add(ram_off as usize)
        .ok_or_else(|| anyhow!("file offset overflow"))?;
    let end = file_off
        .checked_add(len)
        .ok_or_else(|| anyhow!("read end overflow"))?;
    if end > scus.len() {
        bail!(
            "RAM 0x{ram_addr:08X} -> file 0x{file_off:X}+{len} past SCUS end 0x{:X}",
            scus.len()
        );
    }
    Ok(&scus[file_off..end])
}

/// Read a NUL-terminated ASCII string from a buffer at the given offset, up
/// to `max_len` bytes. Non-ASCII or control bytes (< 0x20 except NUL) are
/// dropped; the returned string is the prefix up to the first NUL.
fn read_cstr(buf: &[u8], offset: usize, max_len: usize) -> String {
    let end = (offset + max_len).min(buf.len());
    let slice = &buf[offset..end];
    let mut out = String::new();
    for &b in slice {
        if b == 0 {
            break;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
    }
    out
}

/// Parse the placement table + name table out of a loaded SCUS_942.54
/// binary.
pub fn parse_scus(scus: &[u8]) -> Result<WorldmapMenu> {
    // Name table: 16 entries x 32 bytes.
    let name_bytes = read_scus_at(scus, NAME_TABLE_ADDR, NAME_COUNT * NAME_STRIDE)?;
    let mut names = Vec::with_capacity(NAME_COUNT);
    for i in 0..NAME_COUNT {
        let off = i * NAME_STRIDE;
        names.push(read_cstr(name_bytes, off, NAME_STRIDE));
    }

    // Placement table: walk 6-byte records until byte[0] == 0xFF or cap.
    let max_bytes = PLACEMENT_MAX * PLACEMENT_STRIDE;
    let raw = read_scus_at(scus, PLACEMENT_TABLE_ADDR, max_bytes)?;
    let mut placements = Vec::new();
    for i in 0..PLACEMENT_MAX {
        let off = i * PLACEMENT_STRIDE;
        if off + PLACEMENT_STRIDE > raw.len() {
            break;
        }
        let name_idx = raw[off];
        if name_idx == 0xFF {
            break;
        }
        if (name_idx as usize) >= names.len() {
            // Record points outside the name table - skip silently rather
            // than fail; this matches the walker's tolerance.
            continue;
        }
        let scene_id = u16::from_le_bytes([raw[off + 2], raw[off + 3]]);
        placements.push(PlacementRecord {
            index: i as u32,
            name_idx,
            discovery_flag: raw[off + 1],
            scene_id,
            menu_x: raw[off + 4],
            menu_y: raw[off + 5],
        });
    }

    Ok(WorldmapMenu { names, placements })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-crafted SCUS-shaped buffer that places synthetic placement + name
    // tables at the canonical addresses. Doesn't ship real Sony bytes; the
    // bytes are arbitrary fixtures chosen to round-trip the parser.
    fn synth_scus() -> Vec<u8> {
        // Total span: header (0x800) + (0x80074200 - 0x80010000) so the name
        // table fits comfortably.
        let span =
            (NAME_TABLE_ADDR + (NAME_COUNT as u32 * NAME_STRIDE as u32)) - SCUS_LOAD_ADDR + 0x100;
        let mut buf = vec![0u8; PSX_EXE_HEADER + span as usize];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        // t_addr at +0x18.
        buf[0x18..0x1C].copy_from_slice(&SCUS_LOAD_ADDR.to_le_bytes());

        // 3 placement records + terminator.
        let p_off = PSX_EXE_HEADER + (PLACEMENT_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        buf[p_off..p_off + 6].copy_from_slice(&[0x00, 0x00, 0x55, 0x00, 0x60, 0x19]);
        buf[p_off + 6..p_off + 12].copy_from_slice(&[0x01, 0x01, 0x55, 0x00, 0x36, 0x3E]);
        buf[p_off + 12..p_off + 18].copy_from_slice(&[0x0F, 0x14, 0x62, 0x01, 0x16, 0x3E]);
        buf[p_off + 18] = 0xFF;

        // Names at +0x00, +0x20, +0x40. Index 0xE (Conkram analogue) at +0x1C0.
        let n_off = PSX_EXE_HEADER + (NAME_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        buf[n_off..n_off + 7].copy_from_slice(b"Rim Elm");
        buf[n_off + 0x20..n_off + 0x20 + 12].copy_from_slice(b"Drake Castle");
        // Index 0x0F = "Soren Camp" (matches the real layout: 0x0E is skipped
        // from placements but still occupies its slot).
        buf[n_off + 0x1E0..n_off + 0x1E0 + 10].copy_from_slice(b"Soren Camp");

        buf
    }

    #[test]
    fn parses_synthetic_fixture() {
        let scus = synth_scus();
        let menu = parse_scus(&scus).unwrap();
        assert_eq!(menu.names.len(), 16);
        assert_eq!(menu.names[0], "Rim Elm");
        assert_eq!(menu.names[1], "Drake Castle");
        assert_eq!(menu.names[15], "Soren Camp");
        assert_eq!(menu.placements.len(), 3);
        let r0 = &menu.placements[0];
        assert_eq!(r0.name_idx, 0);
        assert_eq!(r0.discovery_flag, 0);
        assert_eq!(r0.scene_id, 0x0055);
        assert_eq!(r0.menu_x, 0x60);
        assert_eq!(r0.menu_y, 0x19);
        let r2 = &menu.placements[2];
        assert_eq!(r2.name_idx, 0x0F);
        assert_eq!(r2.discovery_flag, 0x14);
        assert_eq!(r2.scene_id, 0x0162);
    }

    #[test]
    fn terminator_stops_walk() {
        let mut scus = synth_scus();
        let p_off = PSX_EXE_HEADER + (PLACEMENT_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        // Move terminator to record 1 - we expect only 1 record back.
        scus[p_off + 6] = 0xFF;
        let menu = parse_scus(&scus).unwrap();
        assert_eq!(menu.placements.len(), 1);
    }

    #[test]
    fn rejects_non_psx_exe() {
        let mut scus = synth_scus();
        scus[0] = b'X';
        assert!(parse_scus(&scus).is_err());
    }

    #[test]
    fn drops_out_of_range_name_idx() {
        let mut scus = synth_scus();
        let p_off = PSX_EXE_HEADER + (PLACEMENT_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        // Record 1: name_idx = 0x40 (way past NAME_COUNT=16). The walker
        // tolerates this, so the parser does too - record is dropped.
        scus[p_off + 6] = 0x40;
        let menu = parse_scus(&scus).unwrap();
        // Records: 0 (kept), 1 (dropped), 2 (kept).
        assert_eq!(menu.placements.len(), 2);
        assert_eq!(menu.placements[0].name_idx, 0x00);
        assert_eq!(menu.placements[1].name_idx, 0x0F);
    }
}
