//! A battle the host leaves before its exit must not own the next one.
//!
//! Retail leaves a battle only through its exit, and battle init clears the
//! end-of-battle marker `DAT_8007BD71` (`FUN_80055B6C`, `sb zero` at
//! `0x80056014`) - the byte whose `0xFE` hands every battle frame to the
//! results sequencer. The port's hosts can leave a fight early (a scene
//! picker, a debug seat, a test running several fights on one runtime): a
//! results sequence left mid-way used to survive into the next fight and own
//! it from its first frame - no round prompt, no command session, and the
//! fight ended when the stale sequence did.
//!
//! Disc-free: synthetic party + the vanilla monster / formation tables.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_core::world::{Actor, InflightCastSeed, World};
use legaia_engine_mode::mode::SceneMode;

fn field_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut h = rec.hp_mp_sp();
        h.hp_cur = 100;
        h.hp_max = 100;
        rec.set_hp_mp_sp(h);
    }
    w.load_party(party);
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.tables.victory_pose_table = Some([[0x14; 6]; 4]);
    w.mode = SceneMode::Field;
    w
}

fn enter_scripted_battle(w: &mut World) {
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            return;
        }
        w.tick();
    }
    panic!("the scripted fight never reached SceneMode::Battle");
}

/// Fight a battle that executes its actions until the results sequencer is
/// running, and leave it there.
fn world_left_mid_results() -> World {
    let mut w = field_world();
    enter_scripted_battle(&mut w);
    for _ in 0..20_000 {
        if w.battle.victory.is_some() {
            return w;
        }
        w.tick();
        assert_eq!(
            w.mode,
            SceneMode::Battle,
            "the fight ended before its results"
        );
    }
    panic!("the fight never reached its results sequence");
}

/// Tick until the seeded cast has been dispatched; return whether the tick
/// that took it began on the first command prompt, which is the only state
/// the seam dispatches from, and closed it.
fn seed_dispatches(w: &mut World) -> bool {
    w.battle.inflight_seed = Some(InflightCastSeed {
        caster: 0,
        spell_id: 0x82,
        target: 9,
        ground: Default::default(),
        idle: Default::default(),
    });
    for _ in 0..4_000 {
        let on_prompt = w.battle.command.is_some() && w.battle.flow == BattleFlowState::TurnPrompt;
        w.set_pad(0);
        w.tick();
        assert!(
            w.battle.victory.is_none(),
            "a results sequence owns the new fight"
        );
        if w.battle.inflight_seed.is_none() {
            return on_prompt && w.battle.command.is_none();
        }
    }
    false
}

/// A battle entered over an unfinished one starts with the marker clear.
#[test]
fn battle_entry_clears_a_stale_results_sequence() {
    let mut w = world_left_mid_results();
    w.battle.player_driven = true;
    w.enter_battle(3, 2);
    assert!(w.battle.victory.is_none());
    assert!(
        seed_dispatches(&mut w),
        "the fresh fight never reached its first command prompt"
    );
}

/// Leaving a live battle for somewhere else (a scene load, a New Game) tears
/// it down with no outcome, and the next fight reaches its command prompt.
#[test]
fn an_abandoned_battle_leaves_the_world_field_shaped() {
    let mut w = world_left_mid_results();
    let flags_before = w.flags.story_flags;
    w.abandon_live_battle();
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.battle.victory.is_none());
    assert!(w.battle.command.is_none());
    assert_eq!(
        w.flags.story_flags, flags_before,
        "an abandoned fight stores no outcome flag"
    );
    // Idempotent once nothing is live.
    w.abandon_live_battle();
    assert_eq!(w.mode, SceneMode::Field);

    w.battle.player_driven = true;
    enter_scripted_battle(&mut w);
    assert!(
        seed_dispatches(&mut w),
        "the next fight never reached its first command prompt"
    );
}
