//! Slice PSX virtual-address windows out of mednafen save-state main RAM.
//!
//! `main_ram_via_anchor` is the legacy-compatible path that mirrors
//! `scripts/ghidra-analysis/extract-mednafen-overlay.py` - used as a fallback when the
//! structured section walker can't find the `MainRAM.data8` entry.

use anyhow::{Context, Result, anyhow, bail};

pub const PSX_RAM_KSEG0: u32 = 0x8000_0000;
pub const PSX_RAM_SIZE: usize = 2 * 1024 * 1024;
pub const SCUS_LOAD_ADDR: u32 = 0x8001_0000;
pub const PSX_EXE_HEADER: usize = 0x800;

/// Anchor strings known to live in SCUS_942.54's loaded region. The first
/// anchor present in BOTH the SCUS binary and the save state determines the
/// file→RAM offset.
pub const ANCHORS: &[&[u8]] = &[
    b"---- FIELD PROGRAM -----%d",
    b"PSX TEST PROGRAM",
    b"enter main loop",
    b"main free mem%d",
    b"h:\\prot\\cdname.dat",
];

/// Locate main RAM in a decompressed save-state payload by anchor search.
///
/// Returns a 2 MiB slice; index `0` corresponds to PSX virtual address
/// `0x80000000`. Caller must supply the SCUS_942.54 binary so the anchor
/// strings can be matched on both sides.
pub fn main_ram_via_anchor_with_scus<'a>(payload: &'a [u8], scus: &[u8]) -> Result<&'a [u8]> {
    for anchor in ANCHORS {
        let scus_off = match find(scus, anchor) {
            Some(o) if o >= PSX_EXE_HEADER => o,
            _ => continue,
        };
        let state_off = match find(payload, anchor) {
            Some(o) => o,
            None => continue,
        };
        let ram_addr = SCUS_LOAD_ADDR as usize + (scus_off - PSX_EXE_HEADER);
        let phys = ram_addr - PSX_RAM_KSEG0 as usize;
        let ram_offset_in_state = state_off
            .checked_sub(phys)
            .ok_or_else(|| anyhow!("anchor implies negative RAM offset"))?;
        if ram_offset_in_state + PSX_RAM_SIZE > payload.len() {
            continue;
        }
        return Ok(&payload[ram_offset_in_state..ram_offset_in_state + PSX_RAM_SIZE]);
    }
    bail!("no anchor found; can't locate main RAM")
}

/// Convenience wrapper that loads `extracted/SCUS_942.54` from `cwd` (or the
/// path in `LEGAIA_SCUS`) on demand. Tests and CLI binaries call this; the
/// library `SaveState::main_ram` calls `main_ram_via_anchor` directly.
pub fn main_ram_via_anchor(payload: &[u8]) -> Result<&[u8]> {
    let scus_path =
        std::env::var("LEGAIA_SCUS").unwrap_or_else(|_| "extracted/SCUS_942.54".to_owned());
    let scus = std::fs::read(&scus_path)
        .with_context(|| format!("reading SCUS at {scus_path} (set LEGAIA_SCUS to override)"))?;
    main_ram_via_anchor_with_scus(payload, &scus)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Slice a `[start..end)` PSX virtual-address window out of main RAM.
/// Returns `Err` if the window straddles the 2 MiB main-RAM boundary.
pub fn ram_slice(ram: &[u8], start: u32, end: u32) -> Result<&[u8]> {
    if start < PSX_RAM_KSEG0 || end > PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32 {
        bail!(
            "slice [0x{start:08X}..0x{end:08X}) outside main RAM \
             [0x{PSX_RAM_KSEG0:08X}..0x{:08X})",
            PSX_RAM_KSEG0 + PSX_RAM_SIZE as u32
        );
    }
    if start > end {
        bail!("slice start > end (0x{start:08X} > 0x{end:08X})");
    }
    let lo = (start - PSX_RAM_KSEG0) as usize;
    let hi = (end - PSX_RAM_KSEG0) as usize;
    Ok(&ram[lo..hi])
}

/// MIPS little-endian word at PSX virtual address `addr`.
pub fn read_u32_le(ram: &[u8], addr: u32) -> Result<u32> {
    let bytes = ram_slice(ram, addr, addr + 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_slice_rejects_out_of_range() {
        let ram = vec![0u8; PSX_RAM_SIZE];
        assert!(ram_slice(&ram, 0x70000000, 0x80000010).is_err());
        assert!(ram_slice(&ram, 0x801F0000, 0x80300000).is_err());
        assert!(ram_slice(&ram, 0x80000010, 0x80000000).is_err());
    }

    #[test]
    fn ram_slice_returns_correct_window() {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        ram[0x10000] = 0xAB;
        ram[0x10003] = 0xCD;
        let slice = ram_slice(&ram, 0x80010000, 0x80010004).unwrap();
        assert_eq!(slice, &[0xAB, 0x00, 0x00, 0xCD]);
    }

    #[test]
    fn read_u32_le_decodes() {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        ram[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        assert_eq!(read_u32_le(&ram, 0x80000000).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn anchor_search_locates_synthetic_ram() {
        // Synthesize a save state where main RAM is anchored by a known
        // SCUS string at a known offset.
        let anchor = b"h:\\prot\\cdname.dat";
        let mut scus = vec![0u8; PSX_EXE_HEADER + 0x10000];
        let scus_pos = PSX_EXE_HEADER + 0x100;
        scus[scus_pos..scus_pos + anchor.len()].copy_from_slice(anchor);

        let mut payload = vec![0u8; 0x40_0000];
        let ram_offset_in_payload = 0x10_000;
        // Compute where the anchor MUST land in payload, given the SCUS layout.
        let ram_addr = SCUS_LOAD_ADDR as usize + (scus_pos - PSX_EXE_HEADER);
        let phys = ram_addr - PSX_RAM_KSEG0 as usize;
        let payload_anchor_pos = ram_offset_in_payload + phys;
        payload[payload_anchor_pos..payload_anchor_pos + anchor.len()].copy_from_slice(anchor);

        let ram = main_ram_via_anchor_with_scus(&payload, &scus).unwrap();
        assert_eq!(ram.len(), PSX_RAM_SIZE);
        // Verify the anchor lands at the expected RAM offset.
        let anchor_in_ram = ram.windows(anchor.len()).position(|w| w == anchor).unwrap();
        assert_eq!(anchor_in_ram, phys);
    }
}
