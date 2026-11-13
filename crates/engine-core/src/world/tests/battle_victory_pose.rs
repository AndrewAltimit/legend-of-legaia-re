//! Who poses at a win: retail's results sequencer reads the acting slot
//! `ctx[+0x13]` (`FUN_8004E568`, `0x8004E5A4`), which the action SM's victory
//! arm leaves on the member who finished the fight, re-picks when that member
//! is down, and forces to seat 2 / seat 1 for the Songi formations
//! (`FUN_801E295C`, `0x801E6688..0x801E6760`).

use super::*;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_action::{ActionState, BattleEndCause};
use legaia_engine_mode::mode::SceneMode;

/// Three party members (slots 0..=2) against one monster (slot 3), the
/// monster already down and `acting` holding the action at its end.
fn world_won_by(acting: u8) -> World {
    let mut world = World::new();
    world.party.party_count = 3;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..4 {
        let a = &mut world.actors[i];
        a.active = true;
        a.battle.liveness = 1;
        a.battle.hp = 100;
        a.battle.max_hp = 100;
    }
    world.actors.truncate(4);
    world.actors[3].battle.hp = 0;
    world.actors[3].battle.liveness = 0;
    world.battle_ctx.active_actor = acting;
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    world
}

fn pose_seat(world: &mut World) -> usize {
    for _ in 0..64 {
        world.live_battle_tick();
        if let Some(seq) = world.battle.victory {
            assert_eq!(seq.cause, BattleEndCause::MonsterWipe);
            return seq.pose_actor;
        }
    }
    panic!("the wiped monster side never raised the victory");
}

#[test]
fn the_member_who_finished_the_fight_poses() {
    let mut world = world_won_by(1);
    assert_eq!(pose_seat(&mut world), 1, "Noa's finishing blow, Noa's pose");
    let mut world = world_won_by(0);
    assert_eq!(pose_seat(&mut world), 0);
}

#[test]
fn a_downed_finisher_hands_the_pose_to_a_standing_member() {
    let mut world = world_won_by(2);
    world.actors[2].battle.hp = 0;
    world.actors[2].battle.liveness = 0;
    let seat = pose_seat(&mut world);
    assert!(
        seat < 2,
        "the re-pick lands on a standing member, got {seat}"
    );
}

#[test]
fn the_songi_formations_force_their_seat() {
    for (songi, seat) in [(0xB3u16, 2usize), (0xB4, 1)] {
        let mut world = world_won_by(0);
        world.battle.active_formation = Some(FormationDef::new(1, vec![FormationSlot::new(songi)]));
        assert_eq!(pose_seat(&mut world), seat, "formation {songi:#04x}");
    }
}

/// A party wipe stores no seat (`FUN_801E295C`'s wipe arm has no
/// `ctx[+0x13]` store), so the sequencer's shared tail frames whoever acted
/// last - here the monster that landed the wiping blow.
#[test]
fn a_party_wipe_frames_the_monster_that_landed_the_blow() {
    let mut world = world_won_by(3);
    world.actors[3].battle.hp = 100;
    world.actors[3].battle.liveness = 1;
    for i in 0..3 {
        world.actors[i].battle.hp = 0;
        world.actors[i].battle.liveness = 0;
    }
    for _ in 0..64 {
        world.live_battle_tick();
        if world.battle.victory.is_some() {
            break;
        }
    }
    let seq = world
        .battle
        .victory
        .expect("the wiped party raised the loss");
    assert_eq!(seq.cause, BattleEndCause::PartyWipe);
    assert_eq!(seq.pose_actor, 3, "the killing monster is framed");
    assert_eq!(seq.pose_id, None, "a wipe picks no win pose");
    assert_eq!(seq.voice_bank, None, "and streams no victory voice");
    assert_eq!(world.retail_acting_slot(), 3);
}

/// The win pose streams its voice over the formation's slot-7 bank: the
/// SCUS voice table's byte for (character, pose), less one.
#[test]
fn the_win_pose_stages_its_voice_bank_in_slot_7() {
    let mut world = world_won_by(0);
    world.tables.victory_pose_table = Some([[0x14; 6]; 4]);
    let mut voice = [[0u8; 8]; 4];
    for (r, row) in voice.iter_mut().enumerate() {
        for (c, b) in row.iter_mut().enumerate() {
            *b = 0xB0 + (r * 8 + c) as u8;
        }
    }
    world.tables.victory_voice_table = Some(voice);
    world.battle.active_formation = Some(FormationDef::new(1, vec![FormationSlot::new(0x10)]));
    pose_seat(&mut world);
    let seq = world.battle.victory.expect("armed");
    assert_eq!(seq.pose_id, Some(0x14));
    let char_id = world.party_roster_slot(0) as u8 + 1;
    let want = u16::from(voice[usize::from(char_id - 1)][3]) - 1;
    assert_eq!(seq.voice_bank, Some(want));
    let banks = world.battle_monster_sound_banks();
    assert_eq!(
        banks.iter().find(|(slot, _)| *slot == 7).map(|b| b.1),
        Some(want),
        "slot 7 answers the voice bank: {banks:?}"
    );
}

/// Songi's formations force win pose `0x17` (`0x8004EB90..0x8004EBAC`).
#[test]
fn a_songi_win_forces_pose_0x17() {
    let mut world = world_won_by(0);
    world.tables.victory_pose_table = Some([[0x14; 6]; 4]);
    world.battle.active_formation = Some(FormationDef::new(1, vec![FormationSlot::new(0xB3)]));
    pose_seat(&mut world);
    assert_eq!(world.battle.victory.and_then(|s| s.pose_id), Some(0x17));
}

/// The retail-compare seed's pose seat (`BattleState::forced_victory_seat`)
/// stands the pose on the capture's acting slot whoever closed the replay's
/// fight; a party wipe ignores it.
#[test]
fn a_seeded_victory_seat_poses_whoever_closed_the_fight() {
    let mut world = world_won_by(2);
    world.battle.forced_victory_seat = Some(0);
    assert_eq!(pose_seat(&mut world), 0, "the capture's seat, not Gala");
    assert_eq!(world.retail_acting_slot(), 0);
}
