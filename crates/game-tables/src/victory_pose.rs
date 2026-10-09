//! The static `SCUS_942.54` **victory-pose table** the battle results
//! sequencer `FUN_8004E568` indexes - which win-pose action id a party
//! member strikes when a battle is won.
//!
//! Layout (`0x8004E870..0x8004EAEC`, `ghidra/scripts/funcs/8004e568.txt`):
//! one 6-byte row per playable character at [`VICTORY_POSE_TABLE_VA`],
//! indexed `(char_id - 1) * 6` off the seat's 1-based character id
//! (`DAT_8007BD10[seat]`). Columns pair up by pose tier:
//!
//! | columns | pair |
//! |---|---|
//! | `0..=1` | healthy |
//! | `2..=3` | alternate |
//! | `4..=5` | weak (the near-static breathing streams retail loops) |
//!
//! Every entry is an action id in `0x11..=0x18`: `id - 0x11` is the entry
//! index in the character's base "ME" win-pose archive (readef slot
//! `3*char + 2`; `docs/subsystems/audio.md` § "Hero victory voices"), and in
//! the engine that same id stages through the ordinary art-bank ladder
//! (`legaia_engine_vm::anim_vm::resolve_staged_anim`, records `1..=8` =
//! the eight base-archive records). The tier arithmetic that picks a column
//! lives with its consumer, `legaia_engine_core::world::battle::victory`.

/// `0x800788A0` - four 6-byte rows (Vahn / Noa / Gala / fourth slot).
pub const VICTORY_POSE_TABLE_VA: u32 = 0x8007_88A0;

/// Rows in the table (one per character slot the seat table can name).
pub const VICTORY_POSE_ROWS: usize = 4;

/// Columns per row (three pose pairs).
pub const VICTORY_POSE_COLS: usize = 6;

/// Lowest / highest action id a row may carry - the win-pose band.
pub const VICTORY_POSE_ID_MIN: u8 = 0x11;
pub const VICTORY_POSE_ID_MAX: u8 = 0x18;

/// The parsed table: `table[char_id - 1][column]`.
pub type VictoryPoseTable = [[u8; VICTORY_POSE_COLS]; VICTORY_POSE_ROWS];

/// Parse the table out of a `SCUS_942.54` image. `None` when the image is
/// not a PS-X EXE, the address falls outside its text segment, or any byte
/// is outside the win-pose band (a wrong executable, not a table).
pub fn victory_pose_table_from_scus(scus: &[u8]) -> Option<VictoryPoseTable> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
    let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
    let end = t_addr.checked_add(t_size)?;
    let len = (VICTORY_POSE_ROWS * VICTORY_POSE_COLS) as u32;
    if VICTORY_POSE_TABLE_VA < t_addr || VICTORY_POSE_TABLE_VA + len > end {
        return None;
    }
    let off = (VICTORY_POSE_TABLE_VA - t_addr) as usize + 0x800;
    let bytes = scus.get(off..off + len as usize)?;
    let mut table = [[0u8; VICTORY_POSE_COLS]; VICTORY_POSE_ROWS];
    for (r, row) in table.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            let b = bytes[r * VICTORY_POSE_COLS + c];
            if !(VICTORY_POSE_ID_MIN..=VICTORY_POSE_ID_MAX).contains(&b) {
                return None;
            }
            *cell = b;
        }
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_scus(rows: &[[u8; 6]; 4]) -> Vec<u8> {
        let t_addr = 0x8001_0000u32;
        let t_size = 0x0008_0000u32;
        let mut exe = vec![0u8; 0x800 + t_size as usize];
        exe[0..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        exe[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let off = (VICTORY_POSE_TABLE_VA - t_addr) as usize + 0x800;
        for (r, row) in rows.iter().enumerate() {
            exe[off + r * 6..off + r * 6 + 6].copy_from_slice(row);
        }
        exe
    }

    #[test]
    fn parses_rows_in_the_win_pose_band() {
        let rows = [
            [0x13, 0x14, 0x11, 0x12, 0x15, 0x16],
            [0x11, 0x13, 0x12, 0x14, 0x15, 0x16],
            [0x13, 0x14, 0x11, 0x12, 0x15, 0x16],
            [0x11, 0x12, 0x13, 0x14, 0x15, 0x16],
        ];
        let exe = synthetic_scus(&rows);
        assert_eq!(victory_pose_table_from_scus(&exe), Some(rows));
    }

    #[test]
    fn rejects_out_of_band_bytes_and_non_exe() {
        let mut rows = [[0x11u8; 6]; 4];
        rows[2][4] = 0x19;
        assert_eq!(victory_pose_table_from_scus(&synthetic_scus(&rows)), None);
        assert_eq!(victory_pose_table_from_scus(&[0u8; 0x1000]), None);
    }
}
