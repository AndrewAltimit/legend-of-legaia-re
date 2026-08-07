//! Regression: a monster killed BEFORE its staged action fires must not
//! execute that action.
//!
//! The measured defect (wave instrumentation on the first living battle
//! runs): a monster whose HP was zeroed at battle entry still executed its
//! staged multi-strike action - three strikes retired against the party in
//! the first `AttackChain -> AttackRecovery` tick, because the action SM
//! never re-validated the acting actor's liveness between arming and
//! dispatch.
//!
//! Retail cannot reach the state: every battle-order recompute
//! (`FUN_801DABA4`) zeroes a dead actor's unspent initiative key
//! (`0x801DABD8`/`0x801DABE8` guards), bumps the round-skip count
//! `ctx[+0x25]`, and clears a dead slot's staged Item action - so a dead
//! actor is never picked into `ctx[+0x274]`, the only writer of the acting
//! slot the seed state copies. The port's hosts CAN kill an actor between
//! arming and seed (an external HP write, a harness force-kill), so the
//! seed state itself now gates on the acting actor's liveness and routes a
//! dead actor where retail's cleared-action category-0 arm routes: the Done
//! band (state `0x50`), spending the turn without an attack.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;

/// A live-loop world seated for a battle against the vanilla Goblin
/// formation (id 1). The monster is given an overwhelming initiative key
/// source (SPD) so it is the first combatant armed, and the party's attack
/// is left at zero so nothing else can kill it first.
fn world_entering_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
    }
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w
}

#[test]
fn a_monster_killed_before_its_staged_action_fires_never_attacks() {
    let mut w = world_entering_battle();
    assert!(
        w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1),
        "the vanilla formation table should register a scripted row"
    );
    for _ in 0..300 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle, "the battle must open");

    let monster_slot = 3usize;
    assert_eq!(
        w.actors[monster_slot].battle.liveness, 1,
        "battle entry seats a living monster"
    );
    // Make the monster the first pick and the party harmless to it, so the
    // only thing that can end this battle is the monster's own death below.
    w.battle_speed[monster_slot] = 20000;
    for i in 0..3 {
        w.set_battle_attack(i as u8, 0);
    }
    w.set_battle_attack(monster_slot as u8, 50);

    // Tick until the monster's turn is ARMED but not yet dispatched: the SM
    // holds its staged action in the pre-seed band with the monster as the
    // acting actor. That is the arming-to-dispatch window retail cannot be
    // killed in and the port can.
    let pre_seed = [
        ActionState::Begin.as_byte(),
        ActionState::PreActionWait.as_byte(),
        ActionState::QueuedFromMenu.as_byte(),
        ActionState::ActionSeed.as_byte(),
    ];
    let mut armed = false;
    for _ in 0..2_000 {
        if w.battle_ctx.active_actor as usize == monster_slot
            && pre_seed.contains(&w.battle_ctx.action_state)
        {
            armed = true;
            break;
        }
        w.tick();
        assert_eq!(
            w.mode,
            SceneMode::Battle,
            "the battle may not resolve before the monster's turn comes up"
        );
    }
    assert!(armed, "the monster's staged action must get armed");

    // Kill it in the window.
    {
        let a = &mut w.actors[monster_slot].battle;
        a.hp = 0;
        a.liveness = 0;
    }
    let party_hp_at_kill: Vec<u16> = (0..3).map(|i| w.actors[i].battle.hp).collect();

    // Drive to resolution: with its one monster dead the battle must end in
    // a monster wipe, and the corpse's staged strikes must never retire.
    let mut resolved = false;
    for _ in 0..5_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            resolved = true;
            break;
        }
    }
    assert!(resolved, "the battle must resolve after the monster dies");
    let party_hp_after: Vec<u16> = (0..3).map(|i| w.actors[i].battle.hp).collect();
    assert_eq!(
        party_hp_at_kill, party_hp_after,
        "a dead monster's staged action executed - party HP moved after the kill"
    );
}
