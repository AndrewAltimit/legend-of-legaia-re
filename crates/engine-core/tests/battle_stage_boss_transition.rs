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
//! * mid-battle transition (`FUN_801FD150` epilogue,
//!   `0x801FD4D4..0x801FD548`, the `sb` at `0x801FD514`): the same formation
//!   id **and** the first monster seat's `+0x14C == 0` → stage `3` (entry
//!   969), plus the loader-B call (`jal 0x8003EC70`, `a0 = 0x4A = 3 + 0x47`)
//!   issued in the arm itself.
//!
//! The guard separating the arms is the seat's liveness: arm 2 is a property
//! of the formation alone, arm 3 is the phase transition taken once that seat
//! has died. Both are static disassembly facts, so both are pinned here
//! disc-free.

use legaia_engine_core::encounter_record::BOSS_TRANSITION_MONSTER_ID;
use legaia_engine_core::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_core::overlay_loader::{
    battle_init_stage_override, battle_stage_overlay_entry, boss_transition_stage_id,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

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

/// A battle against the `0xB5` formation: one party member, one monster.
fn boss_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 2 {
        w.actors.push(Actor::default());
    }
    w.party_count = 1;
    w.enter_battle(1, 1);
    for i in 0..2 {
        w.actors[i].battle.hp = 400;
        w.actors[i].battle.max_hp = 400;
        w.actors[i].battle.liveness = 1;
    }
    w.mode = SceneMode::Battle;
    w.active_formation = Some(FormationDef::new(
        0x0B5,
        vec![FormationSlot::new(BOSS_TRANSITION_MONSTER_ID.into())],
    ));
    w
}

#[test]
fn the_world_resolver_walks_stage_2_then_3_as_the_phase_1_seat_dies() {
    let mut w = boss_world();
    assert_eq!(
        w.battle_stage_id(),
        2,
        "the 0xB5 formation reads the init override while phase 1 lives"
    );

    // Phase 1 dies: the transition arm's guard is now true.
    w.actors[1].battle.liveness = 0;
    assert_eq!(
        w.battle_stage_id(),
        3,
        "a dead first monster seat flips the id to the phase-2 stage"
    );
}

#[test]
fn an_ordinary_formation_reads_stage_zero_alive_or_dead() {
    let mut w = boss_world();
    w.active_formation = Some(FormationDef::new(1, vec![FormationSlot::new(0x04)]));
    assert_eq!(w.battle_stage_id(), 0);
    w.actors[1].battle.liveness = 0;
    assert_eq!(
        w.battle_stage_id(),
        0,
        "the transition arm is keyed on the formation id, not on any death"
    );
}
