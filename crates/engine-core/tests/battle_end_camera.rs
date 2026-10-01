//! The battle camera through the end-of-battle sequence.
//!
//! Retail's results sequencer `FUN_8004E568` frames its pose actor on every
//! frame it runs: case 8 with `ctx[+0xD] = 1` through the load window - every
//! monster down and its node gone, so the stand-off arm - and case 6 from the
//! results frame on, which with the battle-end signal up is the battle-over
//! arm's close-up behind the posing character. The port used to leave the
//! camera on the far framing with the idle orbit for the whole sequence.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_core::battle_cam_inputs::battle_cam_inputs;
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, VictoryPhase, World};
use legaia_engine_vm::battle_cam_script::{BattleCamPhase, PARTY_BODY_RADIUS, prescale_tr_z};

fn world_in_a_battle() -> World {
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
    // A win-pose table in the SCUS shape: every id in the `0x11..=0x18` band.
    w.tables.victory_pose_table = Some([[0x14; 6]; 4]);
    w.mode = SceneMode::Field;
    w
}

#[test]
fn the_victory_frames_the_pose_actor_not_the_far_orbit() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    let (mut saw_load, mut saw_results) = (false, false);
    let mut results_yaw = None;
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        let Some(seq) = w.battle.victory else {
            continue;
        };
        let phase = battle_cam_inputs(&w).phase;
        let pose = w.battle_cam_pose();
        let leader = w.battle_display_trio(seq.pose_actor).unwrap();
        match seq.phase {
            VictoryPhase::Loading { frames_left } => {
                assert_eq!(phase, BattleCamPhase::ActionEnd, "case 8 in the load");
                if frames_left < 40 {
                    // Settled on the stand-off arm: level, `0x400` up,
                    // `radius * 5 / 2` back, on the leader.
                    assert_eq!(pose.pitch, 0.0);
                    assert_eq!(pose.tr[1], 1024.0);
                    assert_eq!(pose.tr[2], prescale_tr_z(PARTY_BODY_RADIUS * 5 / 2));
                    assert_eq!(pose.focus, [leader[0], 0.0, leader[2]]);
                    saw_load = true;
                }
            }
            VictoryPhase::Results { hold } => {
                assert_eq!(phase, BattleCamPhase::Action, "case 6 from the results");
                if hold > 40 {
                    // The battle-over close-up: 0x500 back at most, on the
                    // leader, and the idle orbit no longer turns it.
                    assert!(pose.tr[2] <= prescale_tr_z(0x500), "{pose:?}");
                    assert_eq!(pose.focus, [leader[0], 0.0, leader[2]]);
                    let yaw = *results_yaw.get_or_insert(pose.yaw);
                    assert_eq!(pose.yaw, yaw, "no orbit through the hold");
                    saw_results = true;
                }
            }
            VictoryPhase::Exit { .. } => {
                assert_eq!(phase, BattleCamPhase::Action, "case 6 through the fade");
            }
        }
    }
    assert!(saw_load && saw_results, "the sequence ran both framings");
}
