//! The parts of a retail SC block that retail's composer derives rather than
//! copies, and the live-state window a block carries whole.
//!
//! `FUN_801E1934` (menu overlay, `overlay_menu_801e1934.txt`) composes a save
//! in three steps: it writes a **save summary** into the live state at
//! `0x80084364..`, copies the whole `0x1A18`-byte window from `0x80084140`
//! into its block buffer, and stamps the checksum. The summary is what the
//! load screen's per-slot preview reads, so a block composed without it
//! lists on retail's load screen with no names, levels or HP. Everything else
//! in the window is whatever live RAM held - see
//! [`RETAIL_LIVE_WINDOW_OFFSET`].

use anyhow::{Result, bail};

use super::{
    RETAIL_CHAR_RECORD_HEADER_SIZE, RETAIL_CHAR_RECORD_STRIDE, RETAIL_GAME_DATA_OFFSET,
    RETAIL_LIVE_STATE_SIZE, RETAIL_PARTY_COUNT_OFFSET, RETAIL_PARTY_MEMBERS_MAX,
    RETAIL_PARTY_MEMBERS_OFFSET, RETAIL_PARTY_NAME_STRIDE, RETAIL_PARTY_NAME_TABLE_OFFSET,
    RETAIL_PLAY_COUNTER_OFFSET, RETAIL_SCENE_LABEL_LEN, RETAIL_SCENE_LABEL_OFFSET,
    restamp_sc_block_checksum,
};

/// Start of the live-state window inside an SC block: the block from here to
/// [`RETAIL_LIVE_STATE_SIZE`] is RAM `0x80084340..0x80085B58` verbatim. The
/// bytes before it are the block identity (header, title, icon) that the
/// composer stamps fresh each save.
pub const RETAIL_LIVE_WINDOW_OFFSET: usize = RETAIL_GAME_DATA_OFFSET;
/// Length of the live-state window ([`RETAIL_LIVE_WINDOW_OFFSET`] to the end
/// of the `0x1A18`-byte copy).
pub const RETAIL_LIVE_WINDOW_LEN: usize = RETAIL_LIVE_STATE_SIZE - RETAIL_LIVE_WINDOW_OFFSET;

/// The previous scene's CDNAME label, `0x10` bytes NUL-padded (RAM
/// `0x80084558`). Every name-based scene change backs the active label up
/// into it before installing the new one (`FUN_80056758(0x80084558,
/// 0x80084548)`, `FUN_8001FD44` / the transition actor `FUN_80021934`).
pub const RETAIL_PREVIOUS_SCENE_LABEL_OFFSET: usize =
    RETAIL_SCENE_LABEL_OFFSET + RETAIL_SCENE_LABEL_LEN;

/// The current scene's raw CDNAME `#define` index, a `u32` (RAM
/// `0x80084540`): the resolved-index word a scene change rewrites
/// (`FUN_8001D7F8`, restored with `sw s3,0x4540(v0)` at `0x8001FE40` in
/// `FUN_8001FD44`). Every retail block on the library cards holds the define
/// of the scene label at [`RETAIL_SCENE_LABEL_OFFSET`] here.
pub const RETAIL_SCENE_INDEX_OFFSET: usize = 0x400;

/// Write the current scene's CDNAME define into
/// [`RETAIL_SCENE_INDEX_OFFSET`], restamping the checksum.
pub fn write_retail_scene_index(sc_block: &mut [u8], define: u32) -> Result<()> {
    let at = RETAIL_SCENE_INDEX_OFFSET;
    if sc_block.len() < at + 4 {
        bail!("sc_block too small for the scene index");
    }
    sc_block[at..at + 4].copy_from_slice(&define.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Summary: the play clock copied from [`RETAIL_PLAY_COUNTER_OFFSET`]
/// (`sw v0,0x24(s6)` at `0x801E1A48`).
pub const RETAIL_SUMMARY_CLOCK_OFFSET: usize = 0x224;
/// Summary: the present-party count copied from
/// [`RETAIL_PARTY_COUNT_OFFSET`] (`sb v1,0x28(s6)` at `0x801E1A50`).
pub const RETAIL_SUMMARY_COUNT_OFFSET: usize = 0x228;
/// Summary: a constant `1` (`sb s0,0x29(s6)` at `0x801E1A40`).
pub const RETAIL_SUMMARY_FLAG_OFFSET: usize = 0x229;
/// Summary: the save number, the save-select list position `_DAT_801F0210`
/// (`sb v1,0x2a(s6)` at `0x801E19AC`).
pub const RETAIL_SUMMARY_SLOT_OFFSET: usize = 0x22A;
/// Summary: per present member `k`, its roster id (`sb v0,0x2c(s1)`).
pub const RETAIL_SUMMARY_IDS_OFFSET: usize = 0x22C;
/// Summary: per member `k`, record byte `+0x130`, the displayed level
/// (`lbu v0,0x6f8(s0)` / `sb v0,0x30(s1)`).
pub const RETAIL_SUMMARY_LEVELS_OFFSET: usize = 0x230;

/// The summary's four per-member halfword rows: `(summary offset, record
/// offset)` - each row holds one halfword per member `k` at `+2k`, copied
/// from that member's record (`0x801E1AA8..0x801E1AD4`). Record `+0x104..`
/// is the live HP / MP block.
pub const RETAIL_SUMMARY_STAT_ROWS: [(usize, usize); 4] = [
    (RETAIL_SUMMARY_HP_MAX_ROW, 0x104),
    (RETAIL_SUMMARY_MP_MAX_ROW, 0x108),
    (RETAIL_SUMMARY_HP_CUR_ROW, 0x106),
    (RETAIL_SUMMARY_MP_CUR_ROW, 0x10A),
];
/// Summary row: per member `k` at `+2k`, maximum HP. The preview halves and
/// quarters it against [`RETAIL_SUMMARY_HP_CUR_ROW`] for the readout tier
/// (`lh v1,0x4c(s1)` against `lhu v1,0x3c(s1)` at `0x801E0CF0..0x801E0D50`).
pub const RETAIL_SUMMARY_HP_MAX_ROW: usize = 0x23C;
/// Summary row: maximum MP.
pub const RETAIL_SUMMARY_MP_MAX_ROW: usize = 0x234;
/// Summary row: current HP, the left-hand number of the preview's HP line.
pub const RETAIL_SUMMARY_HP_CUR_ROW: usize = 0x24C;
/// Summary row: current MP.
pub const RETAIL_SUMMARY_MP_CUR_ROW: usize = 0x244;

/// Record `+0x2A7`, the display name the summary copies `0xC` bytes of into
/// [`RETAIL_PARTY_NAME_TABLE_OFFSET`] (`addiu a1,s5,0x86f`).
const RECORD_NAME_OFFSET: usize = 0x2A7;
/// Record `+0x130`, the displayed level.
const RECORD_LEVEL_OFFSET: usize = 0x130;
/// Roster records a block holds.
const RECORDS: usize = 4;

fn record_base(id: usize) -> usize {
    RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE + id * RETAIL_CHAR_RECORD_STRIDE
}

/// Write the save summary `FUN_801E1934` composes, derived from the block's
/// own fields: the clock word, the party count and the constant `1`, then
/// per present member its id, level, four stat halfwords and its name into
/// the party name table. Slots past the count are left as found, as retail
/// leaves them. The save number is the block identity's
/// ([`super::write_retail_block_identity`]). Restamps the checksum.
pub fn write_retail_save_summary(sc_block: &mut [u8]) -> Result<()> {
    if sc_block.len() < RETAIL_LIVE_STATE_SIZE {
        bail!("sc_block too small for the save summary (need {RETAIL_LIVE_STATE_SIZE} bytes)");
    }
    let clock: [u8; 4] = sc_block[RETAIL_PLAY_COUNTER_OFFSET..RETAIL_PLAY_COUNTER_OFFSET + 4]
        .try_into()
        .unwrap();
    sc_block[RETAIL_SUMMARY_CLOCK_OFFSET..RETAIL_SUMMARY_CLOCK_OFFSET + 4].copy_from_slice(&clock);
    let count = sc_block[RETAIL_PARTY_COUNT_OFFSET];
    sc_block[RETAIL_SUMMARY_COUNT_OFFSET] = count;
    sc_block[RETAIL_SUMMARY_FLAG_OFFSET] = 1;
    for k in 0..usize::from(count).min(RETAIL_PARTY_MEMBERS_MAX) {
        let id = sc_block[RETAIL_PARTY_MEMBERS_OFFSET + k];
        sc_block[RETAIL_SUMMARY_IDS_OFFSET + k] = id;
        let id = usize::from(id);
        if id >= RECORDS {
            continue;
        }
        let rec = record_base(id);
        let name = rec + RECORD_NAME_OFFSET;
        sc_block.copy_within(
            name..name + RETAIL_PARTY_NAME_STRIDE,
            RETAIL_PARTY_NAME_TABLE_OFFSET + k * RETAIL_PARTY_NAME_STRIDE,
        );
        sc_block[RETAIL_SUMMARY_LEVELS_OFFSET + k] = sc_block[rec + RECORD_LEVEL_OFFSET];
        for (row, field) in RETAIL_SUMMARY_STAT_ROWS {
            sc_block.copy_within(rec + field..rec + field + 2, row + 2 * k);
        }
    }
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Summary: the location string the preview prints, the block's first
/// `0x24` bytes of the live window (the scene banner name, NUL-padded).
pub const RETAIL_SUMMARY_LOCATION_OFFSET: usize = RETAIL_GAME_DATA_OFFSET;
/// Bytes of [`RETAIL_SUMMARY_LOCATION_OFFSET`] the preview copies before it
/// measures the string (`FUN_8001A8B0(sp+0x18, buf, 0x24)` at `0x801E0A24`).
pub const RETAIL_SUMMARY_LOCATION_LEN: usize = 0x24;
/// Members the summary has room for: each per-member row is four entries
/// wide before the next row starts.
pub const RETAIL_SUMMARY_MEMBERS_MAX: usize = 4;

/// One party member as the save summary carries it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetailSummaryMember {
    /// Roster id (`0` Vahn, `1` Noa, `2` Gala): the save-icon tile the
    /// preview draws as this member's face.
    pub id: u8,
    /// The record's `+0x130`, the displayed level.
    pub level: u8,
    /// Current / maximum HP, the record's `+0x106` / `+0x104` as stored.
    pub hp: (i16, i16),
    /// Current / maximum MP, the record's `+0x10A` / `+0x108` as stored.
    pub mp: (i16, i16),
    /// The display name, up to its NUL inside the `0xC`-byte field.
    pub name: Vec<u8>,
}

/// The save summary as the load screen's preview reads it.
///
/// `FUN_801E08D8` draws a block's info panel from the per-slot copy of the
/// block's `0x200..0x300` (`0x801EF1B8 + slot * 0x100`, byte-identical to the
/// card on every library save), never from the character records: what the
/// panel shows is what [`write_retail_save_summary`] stored at save time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetailSaveSummary {
    /// The location bytes up to their NUL (at most
    /// [`RETAIL_SUMMARY_LOCATION_LEN`]).
    pub location: Vec<u8>,
    /// Retail's own test on the location string (`0x801E0A2C..0x801E0A78`):
    /// shorter than 33 bytes and every byte in `0x20..0xB0`. A block that
    /// fails it prints its badge and nothing else - no location, no time, no
    /// party rows.
    pub location_valid: bool,
    /// The play clock in 60 Hz frames ([`RETAIL_SUMMARY_CLOCK_OFFSET`]).
    pub clock_frames: u32,
    /// The stored party count ([`RETAIL_SUMMARY_COUNT_OFFSET`]).
    pub count: u8,
    /// The constant-`1` byte the composer writes beside the count. A block
    /// composed without a summary holds `0` here.
    pub flag: u8,
    /// The members, `min(count, RETAIL_SUMMARY_MEMBERS_MAX)` of them.
    pub members: Vec<RetailSummaryMember>,
}

/// Read the save summary off an SC block. `None` for a short block.
pub fn read_retail_save_summary(sc_block: &[u8]) -> Option<RetailSaveSummary> {
    if sc_block.len() < RETAIL_PARTY_NAME_TABLE_OFFSET + RETAIL_SUMMARY_MEMBERS_MAX * 0xC {
        return None;
    }
    let loc_field = &sc_block[RETAIL_SUMMARY_LOCATION_OFFSET
        ..RETAIL_SUMMARY_LOCATION_OFFSET + RETAIL_SUMMARY_LOCATION_LEN];
    let len = loc_field
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(RETAIL_SUMMARY_LOCATION_LEN);
    let location = loc_field[..len].to_vec();
    let location_valid = len < 0x21 && location.iter().all(|&b| b.wrapping_sub(0x20) < 0x90);
    let u16_at = |o: usize| i16::from_le_bytes([sc_block[o], sc_block[o + 1]]);
    let count = sc_block[RETAIL_SUMMARY_COUNT_OFFSET];
    let members = (0..usize::from(count).min(RETAIL_SUMMARY_MEMBERS_MAX))
        .map(|k| {
            let name_at = RETAIL_PARTY_NAME_TABLE_OFFSET + k * RETAIL_PARTY_NAME_STRIDE;
            let field = &sc_block[name_at..name_at + RETAIL_PARTY_NAME_STRIDE];
            let n = field.iter().position(|&b| b == 0).unwrap_or(field.len());
            RetailSummaryMember {
                id: sc_block[RETAIL_SUMMARY_IDS_OFFSET + k],
                level: sc_block[RETAIL_SUMMARY_LEVELS_OFFSET + k],
                hp: (
                    u16_at(RETAIL_SUMMARY_HP_CUR_ROW + 2 * k),
                    u16_at(RETAIL_SUMMARY_HP_MAX_ROW + 2 * k),
                ),
                mp: (
                    u16_at(RETAIL_SUMMARY_MP_CUR_ROW + 2 * k),
                    u16_at(RETAIL_SUMMARY_MP_MAX_ROW + 2 * k),
                ),
                name: field[..n].to_vec(),
            }
        })
        .collect();
    Some(RetailSaveSummary {
        location,
        location_valid,
        clock_frames: u32::from_le_bytes(
            sc_block[RETAIL_SUMMARY_CLOCK_OFFSET..RETAIL_SUMMARY_CLOCK_OFFSET + 4]
                .try_into()
                .unwrap(),
        ),
        count,
        flag: sc_block[RETAIL_SUMMARY_FLAG_OFFSET],
        members,
    })
}

/// The live-state window ([`RETAIL_LIVE_WINDOW_OFFSET`], [`RETAIL_LIVE_WINDOW_LEN`]
/// bytes), or `None` if the block is too short.
pub fn read_retail_live_window(sc_block: &[u8]) -> Option<&[u8]> {
    sc_block.get(RETAIL_LIVE_WINDOW_OFFSET..RETAIL_LIVE_STATE_SIZE)
}

/// Lay a whole live-state window over the block, restamping the checksum.
/// `Err` on a window of the wrong length or a short block.
pub fn write_retail_live_window(sc_block: &mut [u8], window: &[u8]) -> Result<()> {
    if window.len() != RETAIL_LIVE_WINDOW_LEN {
        bail!(
            "live-state window is {} bytes, expected {RETAIL_LIVE_WINDOW_LEN}",
            window.len()
        );
    }
    let Some(dst) = sc_block.get_mut(RETAIL_LIVE_WINDOW_OFFSET..RETAIL_LIVE_STATE_SIZE) else {
        bail!("sc_block too small for the live-state window");
    };
    dst.copy_from_slice(window);
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// The previous scene's label, or `None` for a short block or an empty field.
pub fn read_retail_previous_scene(sc_block: &[u8]) -> Option<String> {
    let field = sc_block.get(
        RETAIL_PREVIOUS_SCENE_LABEL_OFFSET
            ..RETAIL_PREVIOUS_SCENE_LABEL_OFFSET + RETAIL_SCENE_LABEL_LEN,
    )?;
    let s: String = field
        .iter()
        .take_while(|&&b| (0x20..=0x7E).contains(&b))
        .map(|&b| b as char)
        .collect();
    (!s.is_empty()).then_some(s)
}

/// Write the previous scene's label, NUL-padded, and restamp the checksum.
/// A field that already holds the same label is left as it is, residue
/// after the NUL included.
pub fn write_retail_previous_scene(sc_block: &mut [u8], scene: &str) -> Result<()> {
    let at = RETAIL_PREVIOUS_SCENE_LABEL_OFFSET;
    if sc_block.len() < at + RETAIL_SCENE_LABEL_LEN {
        bail!("sc_block too small for the previous-scene label");
    }
    if read_retail_previous_scene(sc_block).as_deref() == Some(scene) {
        return Ok(());
    }
    let field = &mut sc_block[at..at + RETAIL_SCENE_LABEL_LEN];
    field.fill(0);
    for (dst, c) in field
        .iter_mut()
        .zip(scene.chars())
        .take(RETAIL_SCENE_LABEL_LEN - 1)
    {
        *dst = if c.is_ascii() && !c.is_ascii_control() {
            c as u8
        } else {
            b'?'
        };
    }
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::BLOCK_SIZE;

    /// The summary is retail's derivation: count and clock copied, then per
    /// present member (in party order, not roster order) its id, level, the
    /// four stat halfwords and its name.
    #[test]
    fn the_summary_is_derived_from_the_block_like_retail_composes_it() {
        let mut b = vec![0u8; BLOCK_SIZE];
        b[RETAIL_PLAY_COUNTER_OFFSET..RETAIL_PLAY_COUNTER_OFFSET + 4]
            .copy_from_slice(&1234u32.to_le_bytes());
        b[RETAIL_PARTY_COUNT_OFFSET] = 2;
        b[RETAIL_PARTY_MEMBERS_OFFSET] = 2;
        b[RETAIL_PARTY_MEMBERS_OFFSET + 1] = 0;
        for (id, name) in [(0usize, b"Vahn"), (2usize, b"Gala")] {
            let rec = record_base(id);
            b[rec + RECORD_NAME_OFFSET..rec + RECORD_NAME_OFFSET + 4].copy_from_slice(name);
            b[rec + RECORD_LEVEL_OFFSET] = 10 + id as u8;
            for (i, (_, field)) in RETAIL_SUMMARY_STAT_ROWS.iter().enumerate() {
                b[rec + field] = (id * 10 + i) as u8;
            }
        }
        write_retail_save_summary(&mut b).unwrap();
        assert_eq!(&b[0x224..0x228], &1234u32.to_le_bytes());
        assert_eq!(&b[0x228..0x22A], &[2, 1]);
        assert_eq!(&b[0x22C..0x22E], &[2, 0]);
        assert_eq!(&b[0x230..0x232], &[12, 10]);
        assert_eq!(&b[0x254..0x258], b"Gala");
        assert_eq!(&b[0x260..0x264], b"Vahn");
        // Member 0 is Gala (id 2), member 1 Vahn (id 0).
        for (i, (row, _)) in RETAIL_SUMMARY_STAT_ROWS.iter().enumerate() {
            assert_eq!(b[*row], (20 + i) as u8, "row {row:#x} member 0");
            assert_eq!(b[*row + 2], i as u8, "row {row:#x} member 1");
        }
        assert!(crate::card::sc_block_checksum_valid(&b));
        // And the preview's reader gets the same rows back, current before
        // maximum.
        let sum = read_retail_save_summary(&b).unwrap();
        assert_eq!((sum.count, sum.flag, sum.clock_frames), (2, 1, 1234));
        assert_eq!(sum.members.len(), 2);
        assert_eq!(sum.members[0].id, 2);
        assert_eq!(sum.members[0].name, b"Gala");
        assert_eq!(sum.members[0].level, 12);
        // Record +0x104 = 20 is HP max, +0x106 = 22 HP cur, +0x108 = 21 MP
        // max, +0x10A = 23 MP cur.
        assert_eq!(sum.members[0].hp, (22, 20));
        assert_eq!(sum.members[0].mp, (23, 21));
    }

    #[test]
    fn the_location_test_is_retails() {
        let mut b = vec![0u8; BLOCK_SIZE];
        b[0x200..0x20B].copy_from_slice(b"Mt. Rikuroa");
        let s = read_retail_save_summary(&b).unwrap();
        assert_eq!(s.location, b"Mt. Rikuroa");
        assert!(s.location_valid);
        b[0x203] = 0x1F;
        assert!(!read_retail_save_summary(&b).unwrap().location_valid);
        b[0x203] = 0xAF;
        assert!(read_retail_save_summary(&b).unwrap().location_valid);
        b[0x200..0x224].fill(b'a');
        assert!(!read_retail_save_summary(&b).unwrap().location_valid);
    }

    #[test]
    fn the_previous_scene_round_trips_and_keeps_a_matching_field() {
        let mut b = vec![0u8; BLOCK_SIZE];
        assert_eq!(read_retail_previous_scene(&b), None);
        write_retail_previous_scene(&mut b, "town01").unwrap();
        assert_eq!(read_retail_previous_scene(&b).as_deref(), Some("town01"));
        b[RETAIL_PREVIOUS_SCENE_LABEL_OFFSET + 0xE] = 0x55;
        write_retail_previous_scene(&mut b, "town01").unwrap();
        assert_eq!(b[RETAIL_PREVIOUS_SCENE_LABEL_OFFSET + 0xE], 0x55);
    }
}
