//! Per-art **attack-camera track** table (battle-action overlay, PROT 0898,
//! runtime VA `0x801F4E10`).
//!
//! The per-art attack camera `FUN_801D71B8` (dump:
//! `ghidra/scripts/funcs/overlay_battle_action_801d71b8.txt`) frames a party
//! member's Arts swing. Each of its per-character / per-art arms folds one or
//! two halfwords out of this table into the camera pose it hands the tween
//! builder, addressed by the battle context's **phase cursor** `ctx[+0x26D]`:
//!
//! ```text
//! 801d731c  sll   a0,t2,0x1        ; t2 = ctx[+0x26D], the phase cursor
//! 801d7324  lui   v0,0x801f
//! 801d7328  addiu v0,v0,0x4e10     ; table base
//! 801d732c  addu  a0,a0,v0         ; base + cursor * 2
//! 801d7338  lhu   v1,0x0(a0)       ; track 0 at this cursor
//! 801d734c  lhu   v0,0x4(a0)       ; track 1 at this cursor
//! ```
//!
//! So the table is a flat array of **two-halfword rows**: row `t` at
//! `base + t*4`, phase `c` at `+ c*2`. The cursor is binary in every arm that
//! reads it (`beq t2,zero,…` at `0x801D7398` and its siblings), so a row
//! holds one value for each of the swing's two phases.
//!
//! ## Extent
//!
//! [`ATTACK_CAMERA_ROWS`] rows, `0x50` bytes. Two independent measurements
//! agree exactly:
//!
//! * **Which rows the code reads.** Sweeping every `0x801F4E10` base
//!   computation in the dump and collecting the `lhu` displacements off the
//!   pointer it forms yields `0x00, 0x04, …, 0x4C` - twenty rows, dense, with
//!   nothing above `0x4C`.
//! * **Where the data stops.** The halfwords from `0x801F4E10` to
//!   `0x801F4E5F` read as camera offsets (`-256`, `2176`, `1536`, `-1024`,
//!   `3200`, …); at `0x801F4E60` the byte pattern changes character and no
//!   arm reaches it.
//!
//! The row *above* the table is the per-character height table
//! ([`crate::battle_camera_table`], `0x801F4D2C`) and its trailing pointer
//! list; the region below is the move-power table
//! ([`crate::move_power`], `0x801F4F5C`).
//!
//! ## What a row means
//!
//! Nothing uniform, and the disassembly says so: each arm picks its own rows
//! *and its own destination*. The first Gala arm adds row `0` to the pose's
//! pitch and row `1` to its yaw (`0x801D7338`/`0x801D734C`); another adds row
//! `2` to the yaw and row `3` to the eye-space Z (`0x801D7470`/`0x801D748C`).
//! The rows are per-arm constants, not a column-typed record, so this parser
//! exposes them as numbered tracks and leaves the meaning to the arm that
//! reads them ([`legaia_engine_vm::battle_attack_camera`] on the engine side).
//!
//! ## Provenance
//!
//! Static overlay data: VA `0x801F4E10` maps to PROT 0898 file offset
//! [`ATTACK_CAMERA_FILE_OFFSET`] under the same link base
//! ([`OVERLAY_LINK_BASE`]) that pins the per-character height table and the
//! move-power table, inside the overlay's RAM-verified byte-identical
//! `.text` + `.rodata` window.

pub use crate::battle_camera_table::{BATTLE_ACTION_OVERLAY_PROT_INDEX, OVERLAY_LINK_BASE};

/// Runtime VA of the per-art attack-camera track table.
pub const ATTACK_CAMERA_VA: u32 = 0x801F_4E10;

/// Raw PROT 0898 file offset of the table (= `VA − OVERLAY_LINK_BASE`).
pub const ATTACK_CAMERA_FILE_OFFSET: usize = 0x265F8;

/// Rows in the table. One per `lhu` displacement the arms use
/// (`0x00..=0x4C` step `4`).
pub const ATTACK_CAMERA_ROWS: usize = 20;

/// Phase-cursor values a row holds - `ctx[+0x26D]`, which every arm treats as
/// a boolean.
pub const ATTACK_CAMERA_PHASES: usize = 2;

/// Byte length of the table.
pub const ATTACK_CAMERA_LEN: usize = ATTACK_CAMERA_ROWS * ATTACK_CAMERA_PHASES * 2;

/// The parsed per-art attack-camera track table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackCameraTracks {
    rows: [[i16; ATTACK_CAMERA_PHASES]; ATTACK_CAMERA_ROWS],
}

impl AttackCameraTracks {
    /// Parse the table out of the raw PROT 0898 entry bytes. `None` when the
    /// buffer is too short to be that overlay.
    pub fn parse(prot_0898: &[u8]) -> Option<AttackCameraTracks> {
        if prot_0898.len() < ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN {
            return None;
        }
        let mut rows = [[0i16; ATTACK_CAMERA_PHASES]; ATTACK_CAMERA_ROWS];
        for (r, row) in rows.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                let o = ATTACK_CAMERA_FILE_OFFSET + r * 4 + c * 2;
                *v = i16::from_le_bytes([prot_0898[o], prot_0898[o + 1]]);
            }
        }
        Some(AttackCameraTracks { rows })
    }

    /// One track value: `row` is the arm's `lhu` displacement divided by four
    /// (`0x801F4E10 + row*4`), `phase` is `ctx[+0x26D]`.
    ///
    /// Retail forms `base + cursor*2` and indexes it with a **fixed**
    /// displacement, so an out-of-range cursor would read into the next row
    /// rather than fail. The port refuses instead - a cursor above
    /// [`ATTACK_CAMERA_PHASES`] never occurs in the arms that read it.
    pub fn track(&self, row: usize, phase: usize) -> Option<i16> {
        self.rows.get(row)?.get(phase).copied()
    }

    /// A whole row - both phases of one arm's track.
    pub fn row(&self, row: usize) -> Option<[i16; ATTACK_CAMERA_PHASES]> {
        self.rows.get(row).copied()
    }

    /// Every row in table order.
    pub fn rows(&self) -> &[[i16; ATTACK_CAMERA_PHASES]; ATTACK_CAMERA_ROWS] {
        &self.rows
    }

    /// The byte offset of `row` from the table base - retail's own `lhu`
    /// displacement, which is what an arm's dump line shows.
    pub const fn row_byte_offset(row: usize) -> usize {
        row * 4
    }
}

/// Parse helper mirroring the other format modules.
pub fn parse(prot_0898: &[u8]) -> Option<AttackCameraTracks> {
    AttackCameraTracks::parse(prot_0898)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN];
        for r in 0..ATTACK_CAMERA_ROWS {
            for c in 0..ATTACK_CAMERA_PHASES {
                let v = (r as i16 * 16 - c as i16).to_le_bytes();
                let o = ATTACK_CAMERA_FILE_OFFSET + r * 4 + c * 2;
                buf[o] = v[0];
                buf[o + 1] = v[1];
            }
        }
        buf
    }

    #[test]
    fn file_offset_matches_the_link_base() {
        assert_eq!(
            ATTACK_CAMERA_VA - OVERLAY_LINK_BASE,
            ATTACK_CAMERA_FILE_OFFSET as u32
        );
    }

    /// The table sits between the two neighbours that pin its extent: the
    /// per-character height table above and the move-power table below.
    #[test]
    fn table_fits_between_its_neighbours() {
        let height = crate::battle_camera_table::CAMERA_HEIGHT_VA as usize;
        let power = crate::move_power::MOVE_POWER_TABLE_VA as usize;
        let start = ATTACK_CAMERA_VA as usize;
        assert!(height < start, "{height:#x} .. {start:#x}");
        assert!(start + ATTACK_CAMERA_LEN <= power, "overlaps move-power");
        assert_eq!(ATTACK_CAMERA_LEN, 0x50);
    }

    /// Row `t` at `base + t*4`, phase `c` at `+ c*2` - the addressing the
    /// arms use.
    #[test]
    fn rows_are_four_bytes_apart_and_hold_two_phases() {
        let t = AttackCameraTracks::parse(&synthetic()).expect("parses");
        for r in 0..ATTACK_CAMERA_ROWS {
            assert_eq!(t.track(r, 0), Some(r as i16 * 16));
            assert_eq!(t.track(r, 1), Some(r as i16 * 16 - 1));
            assert_eq!(AttackCameraTracks::row_byte_offset(r), r * 4);
        }
        assert_eq!(t.track(ATTACK_CAMERA_ROWS, 0), None, "past the table");
        assert_eq!(t.track(0, ATTACK_CAMERA_PHASES), None, "past the cursor");
    }

    /// Values are **signed** - the real table's first row is `(-256, -128)`,
    /// and a camera offset that only ever read positive would be wrong.
    #[test]
    fn tracks_are_signed_halfwords() {
        let mut buf = vec![0u8; ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN];
        buf[ATTACK_CAMERA_FILE_OFFSET] = 0x00;
        buf[ATTACK_CAMERA_FILE_OFFSET + 1] = 0xFF;
        let t = AttackCameraTracks::parse(&buf).expect("parses");
        assert_eq!(t.track(0, 0), Some(-256));
    }

    #[test]
    fn short_buffer_is_rejected() {
        assert!(AttackCameraTracks::parse(&[0u8; 16]).is_none());
        let short = vec![0u8; ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN - 1];
        assert!(AttackCameraTracks::parse(&short).is_none());
    }
}
