//! The two battle stage-id writers for the `0xB5` boss formation, pinned as
//! one resolver.
//!
//! `_DAT_8007B64A` has **four** retail writers, and the SCUS-only census sees
//! only three of them (two clears + the init override) - the fourth lives in
//! the battle band's overlay code:
//!
//! * init override (`FUN_80055B6C`, `0x80055D2C..0x80055D44`): formation cell
//!   `_DAT_8007BD0C == 0xB5` → stage `2` (extraction entry 968), written at
//!   battle setup while the phase-1 monster is alive;
//! * mid-battle transition (the tail arm of the Final Heal sweep
//!   `FUN_801E6968`, `0x801E6CE4..0x801E6D64`, the `sb` at `0x801E6D2C`):
//!   the same formation id **and** the first monster seat's `+0x14C == 0` →
//!   stage `3` (entry 969, Cort's form-transition module), plus the loader-B
//!   call (`jal 0x8003EC70` at `0x801E6D14`, `a0 = 0x4A = 3 + 0x47`) issued
//!   in the arm itself.
//!
//! The guard separating the arms is the seat's liveness: arm 2 is a property
//! of the formation alone, arm 3 is the phase transition taken once that seat
//! has died. Both are static disassembly facts, so both are pinned here
//! disc-free.

use legaia_engine_core::battle_stage_module::{
    battle_init_stage_override, boss_transition_stage_id,
};
use legaia_engine_core::encounter_record::BOSS_TRANSITION_MONSTER_ID;
use legaia_engine_core::overlay_loader::battle_stage_overlay_entry;

#[test]
fn the_two_stage_arms_fire_on_their_own_guards_and_map_to_968_and_969() {
    // Arm 2: the formation id alone.
    assert_eq!(
        battle_init_stage_override(BOSS_TRANSITION_MONSTER_ID),
        Some(2)
    );
    assert_eq!(battle_init_stage_override(0x04), None);

    // Arm 3: the formation id AND a dead first monster seat.
    assert_eq!(
        boss_transition_stage_id(BOSS_TRANSITION_MONSTER_ID, 0),
        Some(3)
    );
    assert_eq!(
        boss_transition_stage_id(BOSS_TRANSITION_MONSTER_ID, 1),
        None,
        "phase 1 alive - the transition arm's HP guard holds it off"
    );
    assert_eq!(boss_transition_stage_id(0x04, 0), None);

    // The ids select the two sibling PROT entries.
    assert_eq!(battle_stage_overlay_entry(2), Some(968));
    assert_eq!(battle_stage_overlay_entry(3), Some(969));
}

// The world-level walk - the init override writing `2`, the arrival's
// hand-back clearing it, the Final Heal tail writing `3` - is driven end to
// end by `battle_stage_cort_e2e.rs`.
