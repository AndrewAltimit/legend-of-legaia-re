//! Locate main RAM in a save-state payload and slice PSX virtual-address
//! windows out of it.
//!
//! The container readers find RAM structurally (mednafen's
//! `MAIN.MainRAM.data8` entry, PCSX-Redux's memory submessage). The anchor
//! search here is the fallback for a payload they cannot walk: it matches a
//! string from the game's executable in both files. It is game-agnostic in
//! [`main_ram_via_exe_anchors`] (the caller passes the strings) and
//! Legaia-specific in [`main_ram_via_anchor`] / [`main_ram_via_anchor_with_scus`]
//! (the legacy path mirroring
//! `scripts/ghidra-analysis/extract-mednafen-overlay.py`).

use anyhow::{Context, Result, anyhow, bail};

use crate::profile::GameProfile;

pub const PSX_RAM_KSEG0: u32 = 0x8000_0000;
pub const PSX_RAM_SIZE: usize = 2 * 1024 * 1024;
pub const SCUS_LOAD_ADDR: u32 = 0x8001_0000;
pub const PSX_EXE_HEADER: usize = 0x800;

/// Legaia's anchor strings, kept under their original name. The list lives
/// with the rest of the game's knowledge in
/// [`crate::game_anchors::EXE_ANCHORS`].
pub const ANCHORS: &[&[u8]] = crate::game_anchors::EXE_ANCHORS;

/// PSX-EXE magic at offset 0 of a console executable.
const PSX_EXE_MAGIC: &[u8] = b"PS-X EXE";

/// Load address of an executable's text: the header's `t_addr` word
/// (`+0x18`) when the file starts with the `PS-X EXE` magic, else
/// [`SCUS_LOAD_ADDR`] (`0x80010000`, which is what almost every title links
/// at and what a headerless test image is taken to mean).
pub fn exe_load_addr(exe: &[u8]) -> u32 {
    if exe.starts_with(PSX_EXE_MAGIC)
        && let Some(b) = exe.get(0x18..0x1C)
    {
        let t = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if (t & 0xFFE0_0000) == PSX_RAM_KSEG0 {
            return t;
        }
    }
    SCUS_LOAD_ADDR
}

/// Locate main RAM in a decompressed save-state payload by anchor search,
/// for any game.
///
/// `exe` is the game's PSX executable (header included) and `anchors` the
/// strings to try, in order; each must sit in the executable's loaded image.
/// The first anchor found in both fixes the payload offset of RAM: the
/// anchor's file offset past the `0x800`-byte header, plus the load address
/// ([`exe_load_addr`]), gives its RAM address. Returns the 2 MiB slice whose
/// index `0` is `0x80000000`.
///
/// This is the fallback for a container no structural walker understands; a
/// string that also occurs in the payload outside RAM (an earlier copy in a
/// different blob) yields a wrong slice, so prefer anchors unique to the
/// executable.
pub fn main_ram_via_exe_anchors<'a>(
    payload: &'a [u8],
    exe: &[u8],
    anchors: &[&[u8]],
) -> Result<&'a [u8]> {
    if anchors.is_empty() {
        bail!("no anchor strings given; can't locate main RAM by anchor search");
    }
    let load = exe_load_addr(exe) as usize;
    for anchor in anchors {
        let exe_off = match find(exe, anchor) {
            Some(o) if o >= PSX_EXE_HEADER => o,
            _ => continue,
        };
        let state_off = match find(payload, anchor) {
            Some(o) => o,
            None => continue,
        };
        let ram_addr = load + (exe_off - PSX_EXE_HEADER);
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

/// [`main_ram_via_exe_anchors`] with a [`GameProfile`]'s anchor strings.
pub fn main_ram_via_profile<'a>(
    payload: &'a [u8],
    exe: &[u8],
    profile: &dyn GameProfile,
) -> Result<&'a [u8]> {
    main_ram_via_exe_anchors(payload, exe, profile.exe_anchor_strings())
}

/// Locate main RAM in a decompressed save-state payload by Legaia's anchor
/// strings ([`ANCHORS`]), matched in the supplied `SCUS_942.54` binary.
///
/// Returns a 2 MiB slice; index `0` corresponds to PSX virtual address
/// `0x80000000`.
pub fn main_ram_via_anchor_with_scus<'a>(payload: &'a [u8], scus: &[u8]) -> Result<&'a [u8]> {
    main_ram_via_exe_anchors(payload, scus, ANCHORS)
}

/// Convenience wrapper that loads `extracted/SCUS_942.54` from `cwd` (or the
/// path in `LEGAIA_SCUS`) on demand and runs the Legaia anchor search.
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

    /// A non-Legaia executable with its own header-declared load address and
    /// its own anchor string: the generic search uses both.
    #[test]
    fn generic_anchor_search_reads_the_exe_load_address() {
        let anchor: &[u8] = b"MY GAME BUILD 1.0";
        let load = 0x8002_0000u32;
        let mut exe = vec![0u8; PSX_EXE_HEADER + 0x4000];
        exe[..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1C].copy_from_slice(&load.to_le_bytes());
        let exe_pos = PSX_EXE_HEADER + 0x200;
        exe[exe_pos..exe_pos + anchor.len()].copy_from_slice(anchor);
        assert_eq!(exe_load_addr(&exe), load);

        let ram_at = 0x40;
        let mut payload = vec![0u8; ram_at + PSX_RAM_SIZE + 0x10];
        let phys = (load - PSX_RAM_KSEG0) as usize + 0x200;
        payload[ram_at + phys..ram_at + phys + anchor.len()].copy_from_slice(anchor);

        let ram = main_ram_via_exe_anchors(&payload, &exe, &[anchor]).unwrap();
        assert_eq!(ram.as_ptr(), payload[ram_at..].as_ptr());
        // Legaia's strings are not in this game.
        assert!(main_ram_via_anchor_with_scus(&payload, &exe).is_err());
        assert!(main_ram_via_profile(&payload, &exe, &crate::profile::GenericGame).is_err());
    }
}
