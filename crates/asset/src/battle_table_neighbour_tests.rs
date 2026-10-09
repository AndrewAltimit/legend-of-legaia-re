//! The battle-action overlay's data tables sit in a fixed order; checked
//! here, where both the camera tables and the move-power table resolve.

use crate::battle_attack_camera_table::{ATTACK_CAMERA_LEN, ATTACK_CAMERA_VA};

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
