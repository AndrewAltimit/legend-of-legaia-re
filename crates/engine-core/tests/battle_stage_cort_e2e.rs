//! The Cort fight (formation monster `0xB5`) end to end through the battle
//! side-band (`FUN_80056208`) and its two stage modules: PROT 0968, the
//! arrival (stage id `2`), and PROT 0969, the form transition (stage id `3`).
//!
//! The fight is entered the way a player enters it - a field encounter rolls
//! the formation, `World::tick` flips into battle - and then only `World::tick`
//! runs. Nothing here pokes the stage id or calls a module: the battle-init
//! override writes `2`, the arrival hands the round back, the party's own auto
//! attack fells the first form, the Final Heal sweep's tail writes `3` in
//! cleanup state `0x50`, and the form transition takes the battle back to the
//! field.
//!
//! Disc-free: the monster is a synthetic one-HP record under the real id, so
//! this runs in CI.

use legaia_engine_core::battle_stage_module::{
    ARRIVAL_CUE, ARRIVAL_DROP_HEIGHT, FORM_TRANSITION_ACTION_STATE, FORM_TRANSITION_CUE,
};
use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::encounter_record::BOSS_TRANSITION_MONSTER_ID;
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

const BOSS_NAME: &str = "Boss";

/// A field world one encounter away from `formation_monster`.
fn world_before(formation_monster: u16) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(
        1,
        vec![FormationSlot::new(formation_monster)],
    ));
    let mut catalog = MonsterCatalog::new();
    // One HP: the party's first swing fells it.
    catalog.insert(MonsterDef::new(formation_monster, BOSS_NAME, 1, 1));
    w.set_formation_table(table, catalog);

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;
    let mut enc = EncounterTable::new("cort_e2e");
    enc.set_trigger_rate(0xFF);
    enc.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(enc));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));
    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w
}

fn walk_into_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            w.set_pad(0);
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

#[test]
fn the_cort_fight_walks_the_arrival_then_the_form_transition_back_to_the_field() {
    let mut w = world_before(u16::from(BOSS_TRANSITION_MONSTER_ID));
    walk_into_battle(&mut w);
    let boss = usize::from(w.party.party_count);

    // Battle init's override: stage 2, the arrival paged in, the round held.
    assert_eq!(w.battle_stage_id(), 2);
    assert!(
        w.battle.command.is_none(),
        "no command prompt under the arrival"
    );

    // ---- Stage 2: the arrival module owns the fight until it hands back ----
    let mut cues = Vec::new();
    let mut max_drop = 0i16;
    let mut banner = None;
    let mut owned_camera_frames = 0;
    let mut frames = 0;
    while w.battle_stage_id() == 2 {
        let _ = w.tick();
        frames += 1;
        assert!(frames < 4000, "the arrival never handed the battle back");
        assert_eq!(w.mode, SceneMode::Battle);
        cues.extend(w.audio.battle_sfx_cues.drain(..).map(|c| c.kind));
        max_drop = max_drop.max(w.actors[boss].move_state.world_y);
        if let Some(b) = w.battle.stage_banner.clone() {
            banner = Some(b);
        }
        if let Some(cam) = w.battle.stage_camera {
            owned_camera_frames += 1;
            // Both hosts read the module's camera through this one accessor.
            let pose = w.battle_cam_pose();
            assert_eq!(pose.tr[1], cam.tr[1] as f32);
        }
        if w.battle_stage_id() == 2 {
            assert!(
                w.battle.command.is_none(),
                "round one must not open before the hand-back (frame {frames})"
            );
        }
    }
    assert!(cues.contains(&ARRIVAL_CUE), "arrival cue raised: {cues:?}");
    assert_eq!(
        max_drop, ARRIVAL_DROP_HEIGHT,
        "the boss drops in from above"
    );
    assert_eq!(
        w.actors[boss].move_state.world_y, 0,
        "and lands on the floor"
    );
    assert_eq!(
        banner.as_deref(),
        Some(BOSS_NAME),
        "the name banner went up"
    );
    assert!(owned_camera_frames > 0x100);
    // The hand-back: stage cleared, camera returned, the banner swept.
    assert_eq!(w.battle_stage_id(), 0);
    assert!(w.battle.stage_camera.is_none());
    assert!(w.battle.stage_banner.is_none());
    assert_eq!(w.battle.sideband.phase, 0);
    assert_eq!(w.actors[boss].battle.anim_rate.get(), 8);

    // ---- The first form falls: the Final Heal tail writes stage 3 ----
    let mut frames = 0;
    while w.battle_stage_id() != 3 {
        let _ = w.tick();
        frames += 1;
        assert!(frames < 20000, "the first form never fell");
        assert!(
            w.battle.victory.is_none(),
            "the gate must not run the results sequence on a stage-2 fight"
        );
        cues.clear();
    }
    assert_eq!(w.actors[boss].battle.liveness, 0);
    assert_eq!(
        w.battle_ctx.action_state,
        legaia_engine_vm::battle_action::ActionState::IdleHold.as_byte(),
        "the tail parks the action SM"
    );

    // ---- Stage 3: the form transition takes the battle back to the field ----
    let mut saw_one_hp = false;
    let mut saw_parked = false;
    let mut shake = std::collections::BTreeSet::new();
    let mut frames = 0;
    while w.mode == SceneMode::Battle {
        let _ = w.tick();
        frames += 1;
        assert!(frames < 4000, "the form transition never exited");
        assert!(w.battle.victory.is_none(), "no results sequence");
        cues.extend(w.audio.battle_sfx_cues.drain(..).map(|c| c.kind));
        if w.mode == SceneMode::Battle {
            saw_one_hp |= w.actors[boss].battle.hp == 1 && w.actors[boss].battle.liveness == 1;
            saw_parked |= w.battle_ctx.action_state == FORM_TRANSITION_ACTION_STATE;
            if let Some(c) = w.battle.stage_camera {
                shake.insert(c.tr[1]);
            }
        }
    }
    assert!(cues.contains(&FORM_TRANSITION_CUE), "{cues:?}");
    assert!(saw_one_hp, "the killing blow leaves the first form at 1 HP");
    assert!(saw_parked, "the SM is parked at 0xFC");
    assert!(
        shake.contains(&0x780) && shake.contains(&0x800),
        "{shake:?}"
    );
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.system_flag_test(1), "the won bit reaches story flag 1");
    assert!(
        w.battle.last_rewards.is_none(),
        "the form transition credits no spoils"
    );
}

#[test]
fn an_ordinary_formation_never_leaves_stage_zero() {
    let mut w = world_before(1);
    walk_into_battle(&mut w);
    assert_eq!(w.battle_stage_id(), 0);
    for _ in 0..600 {
        let _ = w.tick();
        assert_eq!(w.battle_stage_id(), 0);
        assert!(w.battle.stage_camera.is_none());
        if w.mode != SceneMode::Battle {
            break;
        }
    }
}
