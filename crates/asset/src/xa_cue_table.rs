//! The static `SCUS_942.54` **XA cue duration table** at `DAT_800788B8` -
//! one `u16` per voice-cue index `n = id - 0x100`, read by both CD-XA cue
//! dispatchers before they call the clip starter `FUN_8003D53C`:
//!
//! - the menu / jingle dispatcher `FUN_8004FCC8` (`0x8004FD4C..0x8004FD78`),
//! - the battle sound funnel `FUN_8004FE5C`'s party voice leg.
//!
//! Both convert the raw value the same way, `dur = (raw * 60 + 99) / 100`
//! (`legaia_engine_shell::xa_clip::voice_clip_duration_sectors`), and hand
//! `dur` to the starter, which stops the drive `(dur * 150 + 149) / 60`
//! physical sectors past the clip's start. `dur` is therefore denominated in
//! **vsyncs**: `dur * 2.5` sectors at the 150-sector/s XA read rate is
//! `dur / 60` seconds.
//!
//! Every capture-witnessed cue's `dur` reproduces this table
//! (`crates/art/src/hyper_fanfare.rs`); the melee kernel's `0x10C` cue reads
//! entry `0x0C`.

/// `DAT_800788B8`.
pub const XA_CUE_DURATION_TABLE_VA: u32 = 0x8007_88B8;

/// Entries read: cue ids `0x100..=0x13F` cover every clip slot the two
/// dispatchers can name (`(id - 0x100) >> 3` reaches slot 7 at `0x13F`),
/// and the table's trailing entries past `0x37` are zero in retail.
pub const XA_CUE_DURATION_ENTRIES: usize = 0x40;

/// Parse the raw `u16` entries out of a `SCUS_942.54` image. `None` when the
/// image is not a PS-X EXE or the table falls outside its text segment.
pub fn xa_cue_durations_from_scus(scus: &[u8]) -> Option<Vec<u16>> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
    let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
    let end = t_addr.checked_add(t_size)?;
    let len = (XA_CUE_DURATION_ENTRIES * 2) as u32;
    if XA_CUE_DURATION_TABLE_VA < t_addr || XA_CUE_DURATION_TABLE_VA + len > end {
        return None;
    }
    let off = (XA_CUE_DURATION_TABLE_VA - t_addr) as usize + 0x800;
    let bytes = scus.get(off..off + len as usize)?;
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_entries_at_the_table_va() {
        let t_addr = 0x8001_0000u32;
        let t_size = 0x0008_0000u32;
        let mut exe = vec![0u8; 0x800 + t_size as usize];
        exe[0..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        exe[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let off = (XA_CUE_DURATION_TABLE_VA - t_addr) as usize + 0x800;
        exe[off + 0x18..off + 0x1A].copy_from_slice(&373u16.to_le_bytes());
        let t = xa_cue_durations_from_scus(&exe).unwrap();
        assert_eq!(t.len(), XA_CUE_DURATION_ENTRIES);
        assert_eq!(t[0x0C], 373);
        assert_eq!(xa_cue_durations_from_scus(&[0u8; 0x1000]), None);
    }
}
