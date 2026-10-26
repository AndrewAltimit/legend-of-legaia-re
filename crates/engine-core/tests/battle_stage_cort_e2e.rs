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

use legaia_engine_battle::battle_stage_module::{
    ARRIVAL_CUE, ARRIVAL_DROP_HEIGHT, FORM_TRANSITION_ACTION_STATE, FORM_TRANSITION_CUE,
};
use legaia_engine_battle::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};
use legaia_scene_formats::encounter_record::BOSS_TRANSITION_MONSTER_ID;

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
    // A synthetic XA span table, so the stage cues' CD-XA arm has a span.
    w.audio.xa_cue_durations = Some(vec![10; 0x200]);
    w
}

/// The `(clip, channel)` the dispatcher `FUN_8004FCC8` starts for a cue id
/// past `0x100`: slot `(id - 0x100) >> 3`, channel `id & 7`.
fn xa_of(id: u16) -> (u32, u32) {
    (u32::from(id - 0x100) >> 3, u32::from(id & 7))
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
        cues.extend(
            w.drain_battle_xa_cues()
                .into_iter()
                .map(|c| (c.clip, c.channel)),
        );
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
    // `0x20A` is past `0x100`: the dispatcher's CD-XA arm, not an SFX row.
    assert!(
        cues.contains(&xa_of(ARRIVAL_CUE)),
        "arrival cue raised: {cues:?}"
    );
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
        legaia_engine_battle_vm::battle_action::ActionState::IdleHold.as_byte(),
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
        cues.extend(
            w.drain_battle_xa_cues()
                .into_iter()
                .map(|c| (c.clip, c.channel)),
        );
        if w.mode == SceneMode::Battle {
            saw_one_hp |= w.actors[boss].battle.hp == 1 && w.actors[boss].battle.liveness == 1;
            saw_parked |= w.battle_ctx.action_state == FORM_TRANSITION_ACTION_STATE;
            if let Some(c) = w.battle.stage_camera {
                shake.insert(c.tr[1]);
            }
        }
    }
    assert!(cues.contains(&xa_of(FORM_TRANSITION_CUE)), "{cues:?}");
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

/// Disc-gated: with PROT 0968's image installed, the arrival seats its
/// in-image spawn records on the boss seat; the drop's records draw as
/// draw-kind-4 parts while the module runs (the crystal columns of
/// `cort_evolved_card_first_menu`), phase 1's flush retires phase 0's three,
/// and every part retires itself by its own program - the drop's columns
/// cue toward black about a dozen frames past the hand-back and halt.
#[test]
fn the_arrival_seats_its_in_image_records_from_the_module_image() {
    use legaia_engine_battle::battle_stage_module::records;
    let Some(dir) = std::env::var_os("LEGAIA_DISC_BIN").and(
        ["extracted", "../../extracted"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.join("PROT.DAT").is_file()),
    ) else {
        eprintln!("[skip] arrival spawn records: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[legaia_engine_core::world::ARRIVAL_MODULE_PROT as usize].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0968");
    let mut w = world_before(u16::from(BOSS_TRANSITION_MONSTER_ID));
    w.install_stage_module_image(
        legaia_engine_core::world::ARRIVAL_MODULE_PROT,
        std::sync::Arc::from(bytes.as_slice()),
    );
    walk_into_battle(&mut w);
    let live = |w: &World| -> Vec<(u32, u16)> {
        w.casting
            .stage_fx
            .iter()
            .flat_map(|s| s.parts.iter())
            .filter(|p| !p.finished)
            .filter_map(|p| p.tag.map(|t| (t, p.state.field_78)))
            .collect()
    };
    let (mut saw_entry, mut saw_drop, mut kind4) = (false, false, 0);
    let mut frames = 0;
    let step = |w: &mut World| {
        let _ = w.tick();
        // The hosts' frame tail: the effect scene graphs' part tick.
        w.tick_effect_scene_graphs();
    };
    while w.battle_stage_id() == 2 {
        step(&mut w);
        frames += 1;
        assert!(frames < 4000, "the arrival never handed the battle back");
        let l = live(&w);
        saw_entry |= l.iter().any(|(t, _)| records::ARRIVAL_ENTRY.contains(t));
        if l.iter().any(|(t, _)| records::ARRIVAL_DROP.contains(t)) {
            saw_drop = true;
            assert!(
                !l.iter().any(|(t, _)| records::ARRIVAL_ENTRY.contains(t)),
                "phase 1's flush retires phase 0's records"
            );
        }
        kind4 = kind4.max(w.active_effect_kind4_draws().len());
    }
    assert!(saw_entry, "phase 0 seats its records");
    assert!(saw_drop, "phase 1 seats the drop's records");
    assert!(kind4 > 0, "the drop's records draw as draw-kind-4 parts");
    let at_hand_back = live(&w);
    eprintln!("[arrival] live at hand-back: {at_hand_back:x?}");
    let mut gone = None;
    for f in 0..600 {
        step(&mut w);
        if live(&w).iter().all(|(t, _)| !is_arrival(*t)) {
            gone = Some(f);
            break;
        }
    }
    eprintln!("[arrival] every arrival part retired {gone:?} frames past the hand-back");
    assert!(gone.is_some(), "the arrival's parts retire themselves");
}

fn is_arrival(t: u32) -> bool {
    use legaia_engine_battle::battle_stage_module::records;
    records::ARRIVAL_ENTRY.contains(&t)
        || records::ARRIVAL_DROP.contains(&t)
        || t == records::ARRIVAL_TRAIL
}
