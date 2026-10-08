//! In-RAM STR FMV file lookup table - the in-memory descriptor used by the
//! cutscene / MDEC overlay to resolve a movie file by index without having
//! to walk the ISO9660 path table on every play.
//!
//! ## Where this lives
//!
//! The cutscene overlay loads this table into RAM around `0x801CAE40` once
//! the FMV system is initialised. A second copy of the same data, formatted
//! as full ISO9660 directory records, lives ~7 KB later (around `0x801CCA80`).
//! Both copies cover the same six `\MOV\MV1.STR;1` .. `\MOV\MV6.STR;1` files.
//!
//! The compact form parsed here is the "fast lookup" representation - 24
//! bytes per entry, just enough for the cutscene player to seek the disc
//! head:
//!
//! ```text
//! offset  size  field
//! 0x00    12    name      "MV1.STR;1\0..." (null-padded, libcd-shaped)
//! 0x0C     4    field_c   reserved (always zero in the captured corpus)
//! 0x10     4    bcd_msf   libcd BCD MSF (Minute|Second|Frame, lo->hi bytes)
//! 0x14     4    size      file size in bytes (LE u32)
//! ```
//!
//! `bcd_msf` packs three BCD bytes plus one zero byte: byte 0 is the BCD
//! minute, byte 1 is the BCD second, byte 2 is the BCD frame, byte 3 is
//! always zero. This matches the libcd `CdlLOC` representation that the
//! retail loader passes to `CdControl(CdlSetloc, ...)`.
//!
//! ## What this gives us
//!
//! - A pure-Rust parser for the captured table so engines can compare a
//!   captured snapshot against `legaia_iso`'s ISO9660 walk and surface
//!   any drift between the two representations.
//! - The MSF↔LBA conversion needed to look up the same files via the disc
//!   reader without going back through the directory.
//!
//! ## Provenance
//!
//! Captured from a save state with the FMV cutscene overlay loaded.
//! The 6-entry table sits at `0x801CAE40` with stride `0x18`. The
//! matching ISO9660-shaped directory records start at `0x801CCA80`
//! with stride `0x38` and carry the publisher tag "USA" + LBA in
//! big-endian-then-little-endian ISO9660 fashion.

use serde::Serialize;

/// One entry in the in-RAM compact STR FMV table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrFmvEntry {
    /// File name as carried in RAM (e.g. `"MV1.STR;1"`). Trailing nulls
    /// are stripped.
    pub name: String,
    /// Reserved word at offset `+0xC`. Always zero in the captured corpus.
    pub field_c: u32,
    /// BCD minute (0..=99), decoded from byte 0 of `bcd_msf`.
    pub minute: u8,
    /// BCD second (0..=59), decoded from byte 1 of `bcd_msf`.
    pub second: u8,
    /// BCD frame (0..=74), decoded from byte 2 of `bcd_msf`.
    pub frame: u8,
    /// File size in bytes (LE u32 from `+0x14`).
    pub size: u32,
}

impl StrFmvEntry {
    /// Compute the absolute LBA from the BCD MSF triple. Uses the standard
    /// CD addressing identity `LBA = ((M * 60) + S) * 75 + F - 150`.
    pub fn lba(&self) -> u32 {
        let m = u32::from(self.minute);
        let s = u32::from(self.second);
        let f = u32::from(self.frame);
        ((m * 60) + s) * 75 + f - 150
    }
}

/// Stride of one entry in the in-RAM compact table.
pub const ENTRY_STRIDE: usize = 0x18;

/// Maximum number of entries we expect in the compact table. The retail
/// engine ships six FMV files (`MV1.STR;1` .. `MV6.STR;1`).
pub const EXPECTED_ENTRY_COUNT: usize = 6;

/// Parse `count` consecutive entries from `bytes`. Returns `None` if the
/// slice is too short. Entries with all-zero name bytes are dropped (the
/// cutscene overlay zero-fills any unused slot).
pub fn parse_entries(bytes: &[u8], count: usize) -> Option<Vec<StrFmvEntry>> {
    if bytes.len() < count * ENTRY_STRIDE {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = i * ENTRY_STRIDE;
        let raw = &bytes[off..off + ENTRY_STRIDE];
        let name_bytes = &raw[..12];
        if name_bytes.iter().all(|&b| b == 0) {
            continue;
        }
        let nul = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..nul]).into_owned();
        let field_c = u32::from_le_bytes(raw[0xC..0x10].try_into().ok()?);
        let bcd_msf = raw[0x10..0x14].try_into().ok()?;
        let bcd_msf: [u8; 4] = bcd_msf;
        let size = u32::from_le_bytes(raw[0x14..0x18].try_into().ok()?);
        let minute = bcd_to_decimal(bcd_msf[0])?;
        let second = bcd_to_decimal(bcd_msf[1])?;
        let frame = bcd_to_decimal(bcd_msf[2])?;
        out.push(StrFmvEntry {
            name,
            field_c,
            minute,
            second,
            frame,
            size,
        });
    }
    Some(out)
}

/// Decode a single BCD byte (`0xMN` where `M` and `N` are decimal digits).
/// Returns `None` for invalid BCD.
pub fn bcd_to_decimal(b: u8) -> Option<u8> {
    let hi = b >> 4;
    let lo = b & 0x0F;
    if hi > 9 || lo > 9 {
        return None;
    }
    Some(hi * 10 + lo)
}

/// Detect whether `bytes` looks like the head of an STR FMV table by
/// checking that the first entry's name starts with `"MV"` followed by a
/// digit and `".STR"`. Cheap structural check used by the categorize pass.
pub fn looks_like_str_fmv_table(bytes: &[u8]) -> bool {
    if bytes.len() < ENTRY_STRIDE {
        return false;
    }
    let head = &bytes[..12];
    let nul = head.iter().position(|&b| b == 0).unwrap_or(head.len());
    let name = match std::str::from_utf8(&head[..nul]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    name.starts_with("MV")
        && name.len() >= 7
        && name.chars().nth(2).is_some_and(|c| c.is_ascii_digit())
        && name.contains(".STR")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic 6-entry table that mirrors the layout
    /// captured from a real FMV-overlay-resident save state.
    fn synthetic_table() -> Vec<u8> {
        let mut buf = Vec::with_capacity(6 * ENTRY_STRIDE);
        // (name, bcd_minute, bcd_second, bcd_frame, size)
        let entries = [
            (b"MV1.STR;1\0\0\0", 0x53, 0x51, 0x33, 0x004D_D000),
            (b"MV2.STR;1\0\0\0", 0x68, 0x24, 0x34, 0x0114_4000),
            (b"MV3.STR;1\0\0\0", 0x58, 0x22, 0x36, 0x006B_8000),
            (b"MV4.STR;1\0\0\0", 0x48, 0x08, 0x37, 0x00CC_6000),
            (b"MV5.STR;1\0\0\0", 0x63, 0x35, 0x38, 0x00D1_1000),
            (b"MV6.STR;1\0\0\0", 0x41, 0x14, 0x19, 0x00E2_0000),
        ];
        for (name, m, s, f, size) in entries {
            buf.extend_from_slice(name);
            buf.extend_from_slice(&0u32.to_le_bytes()); // field_c
            buf.extend_from_slice(&[m, s, f, 0]);
            buf.extend_from_slice(&(size as u32).to_le_bytes());
        }
        buf
    }

    #[test]
    fn parses_six_synthetic_entries() {
        let buf = synthetic_table();
        let parsed = parse_entries(&buf, EXPECTED_ENTRY_COUNT).expect("parse");
        assert_eq!(parsed.len(), 6);
        assert_eq!(parsed[0].name, "MV1.STR;1");
        assert_eq!(parsed[0].size, 0x004D_D000);
        assert_eq!(parsed[5].name, "MV6.STR;1");
    }

    #[test]
    fn bcd_round_trip_pinned_values() {
        // Pinned from a real capture: MV1 at minute=53, second=51,
        // frame=33 (decimal).
        assert_eq!(bcd_to_decimal(0x53), Some(53));
        assert_eq!(bcd_to_decimal(0x51), Some(51));
        assert_eq!(bcd_to_decimal(0x33), Some(33));
        // Invalid BCD (high nibble > 9) should reject.
        assert_eq!(bcd_to_decimal(0xA0), None);
        assert_eq!(bcd_to_decimal(0x0F), None);
    }

    #[test]
    fn lba_matches_msf_identity_for_pinned_entries() {
        let buf = synthetic_table();
        let parsed = parse_entries(&buf, EXPECTED_ENTRY_COUNT).unwrap();
        // MV1: 53:51.33 -> ((53*60+51)*75+33)-150 = (3231)*75 + 33 - 150 = 242208
        assert_eq!(parsed[0].lba(), 242208);
        // MV6: 41:14.19 -> ((41*60+14)*75+19)-150 = (2474)*75 + 19 - 150 = 185419
        assert_eq!(parsed[5].lba(), 185419);
    }

    #[test]
    fn detector_accepts_mv_prefix() {
        let buf = synthetic_table();
        assert!(looks_like_str_fmv_table(&buf));
    }

    #[test]
    fn detector_rejects_unrelated_data() {
        // ISO9660 directory record - starts with record-length byte, not "MV".
        let bytes = b"\x24\x00\x00\x00\x01\x00\x00\x01\x09MV1.STR";
        assert!(!looks_like_str_fmv_table(bytes));
    }

    #[test]
    fn parser_skips_zero_filled_trailing_slots() {
        let mut buf = synthetic_table();
        // Append one zero-filled slot - parser should drop it silently.
        buf.extend_from_slice(&[0u8; ENTRY_STRIDE]);
        let parsed = parse_entries(&buf, EXPECTED_ENTRY_COUNT + 1).unwrap();
        assert_eq!(parsed.len(), 6, "trailing zero slot should be skipped");
    }
}
