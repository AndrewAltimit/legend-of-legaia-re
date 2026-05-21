//! Live gameplay-loop integration test: the Field <-> Battle round trip
//! driven entirely through [`World::tick`].
//!
//! Unlike `end_to_end_gameplay_loop.rs` - which composes the encounter /
//! battle / loot pieces with test-side glue (manual `on_field_step`,
//! `enter_battle`, a hand-rolled `drive_battle_to_victory` damage loop) -
//! this test sets [`World::live_gameplay_loop`] and then does nothing but
//! hold a d-pad direction and call [`World::tick`]. The engine itself must:
//!
//!   1. Walk the player on the held pad (field locomotion).
//!   2. Roll a step-driven encounter and flip `Field -> Battle`.
//!   3. Resolve the battle (auto physical attacks) to a monster wipe.
//!   4. Apply XP / gold / drops and return to `Field`.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables. Runs
//! in CI unconditionally.

use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// Build a field-ready world: a 3-member party with battle stats seeded,
/// the vanilla tables installed, a player field actor placed on open
/// ground, and a guaranteed-trigger encounter session for the single
/// Goblin formation (id 1).
fn build_field_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;

    // Party battle stats. `spawn_actor` (called inside the transition) does
    // not reset these, so they carry into battle. Attack 60 vs a Goblin's
    // defense 5 / HP 30 means a one-shot kill.
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    // Roster records back apply_battle_xp's per-member crediting.
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    // Player field actor (party slot 0). Placed at positive coords so the
    // off-grid (< 0) wall clamp never trips; an empty collision grid reads
    // as all-walkable. `field_72` is the per-frame speed multiplier (12.4
    // fixed point); 4096 = 1.0, giving 8 units/frame.
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    // Encounter session: rate 0xFF (every step rolls a battle), one row =
    // formation 1. Short transition / grace timers keep the test brisk.
    let mut table = EncounterTable::new("live_loop_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w
}

#[test]
fn walking_triggers_battle_and_returns_to_field_with_loot() {
    let mut w = build_field_world();
    let start_z = w.actors[0].move_state.world_z;
    let pre_money = w.money;
    let up = InputState::mask_of([PadButton::Up]);

    let mut entered_battle = false;
    let mut moved = false;
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered_battle = true;
        }
        if !entered_battle && w.actors[0].move_state.world_z != start_z {
            moved = true;
        }
        // Stop once we've fought and come back to the field with rewards.
        if entered_battle && w.mode == SceneMode::Field && w.last_battle_rewards.is_some() {
            break;
        }
    }

    assert!(
        moved,
        "player should have walked on the held d-pad in the field"
    );
    assert!(
        entered_battle,
        "walking should have triggered a Field -> Battle transition"
    );
    assert_eq!(
        w.mode,
        SceneMode::Field,
        "should return to the field after a monster wipe"
    );

    let rewards = w
        .last_battle_rewards
        .as_ref()
        .expect("victory should record rewards");
    assert!(rewards.gold > 0, "Goblin formation drops gold: {rewards:?}");
    assert!(rewards.xp > 0, "Goblin formation grants XP: {rewards:?}");
    assert_eq!(
        w.money,
        pre_money + rewards.gold as i32,
        "gold added to money"
    );
    assert!(
        w.player_actor_slot.is_some(),
        "field player slot restored after battle"
    );
    assert!(!w.game_over, "a monster wipe is not a party wipe");
    assert!(
        w.active_formation.is_none(),
        "formation cleared after battle"
    );
}

#[test]
fn live_loop_off_never_transitions_on_its_own() {
    // Same setup but with the live loop disabled: walking must not roll an
    // encounter or enter battle. Guards the opt-in contract that keeps
    // existing Field-mode tick callers unaffected.
    let mut w = build_field_world();
    w.live_gameplay_loop = false;
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..2000 {
        w.set_pad(up);
        w.tick();
        assert_eq!(
            w.mode,
            SceneMode::Field,
            "must stay in field when loop is off"
        );
    }
    assert!(w.last_battle_rewards.is_none());
}
