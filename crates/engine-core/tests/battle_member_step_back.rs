//! The ring's cancel steps the command cursor **back**: retail's `0x28`
//! cancel arm (`FUN_801D0748` `0x801D11B4`) and the command window's case
//! `0x10`, whose tail is `FUN_801D32BC(1)`.
//!
//! A three-member player-driven round: member 0 commits Spirit, member 1's
//! ring opens, and a cancel press there must reopen member 0's ring with its
//! commit (and the guard stance it raised) dropped. A second cancel, now on
//! the round's first member, goes back to `Begin | Run`.
//!
//! Disc-free.

use legaia_engine_core::battle_input::CommandPhase;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn world_in_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 200;
        hms.hp_max = 200;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 10);
        w.set_battle_defense(i as u8, 20);
    }
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Post", 5000, 5);
    def.udf = 40;
    def.ldf = 40;
    cat.insert(def);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(7)]));
    w.set_formation_table(table, cat);
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle && w.battle.command.is_some() {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    w
}

/// Press `button` for one frame, release for one.
fn press(w: &mut World, button: PadButton) {
    w.set_pad(InputState::mask_of([button]));
    w.tick();
    w.set_pad(0);
    w.tick();
}

fn session(w: &World) -> (u8, &CommandPhase) {
    let s = w
        .battle
        .command
        .as_ref()
        .expect("a command session is open");
    (s.actor, &s.phase)
}

#[test]
fn a_ring_cancel_steps_back_a_member_then_to_the_round_prompt() {
    let mut w = world_in_battle();
    // Settle onto the round prompt, then `Begin` (the Left arm).
    for _ in 0..10 {
        if matches!(session(&w).1, CommandPhase::RoundPrompt { .. }) {
            break;
        }
        w.tick();
    }
    assert!(matches!(session(&w).1, CommandPhase::RoundPrompt { .. }));
    press(&mut w, PadButton::Left);
    assert_eq!(session(&w).0, 0);
    assert!(matches!(session(&w).1, CommandPhase::Menu { .. }));

    // Member 0 commits Spirit (the Down arm); member 1's ring opens.
    press(&mut w, PadButton::Down);
    assert!(w.battle.round_flow.committed(0));
    assert!(
        w.battle.guarding[0],
        "Spirit raises the stance at the commit"
    );
    assert_eq!(session(&w).0, 1);
    assert!(matches!(session(&w).1, CommandPhase::Menu { .. }));

    // Cancel on member 1: back to member 0's ring, its commit dropped.
    press(&mut w, PadButton::Circle);
    assert_eq!(session(&w).0, 0, "the cursor stepped back a member");
    assert!(matches!(session(&w).1, CommandPhase::Menu { .. }));
    assert!(!w.battle.round_flow.committed(0), "the commit was dropped");
    assert!(!w.battle.guarding[0], "and the stance with it");

    // Cancel on the round's first member: back to `Begin | Run`.
    press(&mut w, PadButton::Circle);
    assert_eq!(session(&w).0, 0);
    assert!(
        matches!(session(&w).1, CommandPhase::RoundPrompt { .. }),
        "the first member's cancel reopens the round prompt"
    );
}

#[test]
fn a_step_back_skips_a_member_that_cannot_act() {
    let mut w = world_in_battle();
    for _ in 0..10 {
        if matches!(session(&w).1, CommandPhase::RoundPrompt { .. }) {
            break;
        }
        w.tick();
    }
    press(&mut w, PadButton::Left);
    press(&mut w, PadButton::Down); // member 0: Spirit
    press(&mut w, PadButton::Down); // member 1: Spirit
    assert_eq!(session(&w).0, 2);
    // Member 1 falls between its commit and member 2's cancel: the backward
    // scan (`+0x14C != 0`) passes over it to member 0.
    w.actors[1].battle.hp = 0;
    press(&mut w, PadButton::Circle);
    assert_eq!(session(&w).0, 0, "the scan skipped the fallen member");
    assert!(!w.battle.round_flow.committed(0));
}
