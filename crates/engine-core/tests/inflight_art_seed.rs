//! The art viewer's seam: an entered arrow string dispatched at the battle's
//! first command prompt runs through the live art arm.
//!
//! Disc-free: synthetic party + the vanilla monster / formation tables. A
//! synthetic character has no art catalog, so the arrows arm as plain swings,
//! which is the builder's own answer for an unmatched string.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_battle_loop::battle_state::InflightArtSeed;
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;

fn battle_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 1;
    w.actors[0].active = true;
    w.actors[0].battle.hp = 100;
    w.actors[0].battle.max_hp = 100;
    w.actors[0].battle.liveness = 1;
    w.battle.set_battle_attack(0, 60);
    w.load_party(legaia_save::Party::zeroed(1));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.tables.victory_pose_table = Some([[0x14; 6]; 4]);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle, "the fight never opened");
    w.battle.player_driven = true;
    w.run_out_battle_open();
    w
}

#[test]
fn the_seed_arms_the_art_at_the_first_prompt() {
    let mut w = battle_world();
    w.battle.inflight_art_seed = Some(InflightArtSeed {
        caster: 0,
        sequence: vec![4, 3, 4],
        target: 3,
    });
    let mut dispatched = false;
    for _ in 0..4_000 {
        w.set_pad(0);
        w.tick();
        if w.battle.inflight_art_seed.is_none() {
            dispatched = true;
            break;
        }
    }
    assert!(dispatched, "the seed was never consumed");
    assert!(w.battle.art_seed_ra_seru, "the Ra-Seru marker stands up");
    assert!(
        w.battle.command.is_none(),
        "the prompt closed behind the seed"
    );
    assert_eq!(w.actors[0].battle.action_category, 3, "armed as an attack");
    // The seed gives the fight a monster that outlasts the string.
    assert!(
        w.actors
            .iter()
            .skip(1)
            .any(|a| a.battle.liveness != 0 && a.battle.hp >= 60_000),
        "no monster holds the art seed's HP floor"
    );
}
