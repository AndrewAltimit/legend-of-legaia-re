//! A whole party under Rage (the Evil Medallion's passive), through the live
//! player-driven loop: what a write-watched retail round shows with every
//! party seat poked to Rage.
//!
//!   1. The command ring has nobody to serve: every seat carries the `0x380`
//!      group the anim tick stamps, so the round prompt's Begin goes straight
//!      to the commit confirm (`FUN_801DBA04` returns `ctx[0]`, flow `0x6E`).
//!   2. The confirm's Begin starts the round, whose state-`0x00` pass fills
//!      each seat's stream with learned arts (`FUN_801F0450`).
//!   3. Each seat's turn plays its art bytes as staged clips - the latched
//!      clip `+0x1DB` takes the stream's action constants, never the
//!      `0x0C..=0x0F` swings of a plain strike.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};

const LEARNED: [u8; 4] = [7, 11, 10, 6];

fn rage_party_world() -> World {
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
        let m = &mut w.party.roster.members[i];
        let mut bits = m.ability_bits();
        bits[4..8].copy_from_slice(&0x2000u32.to_le_bytes());
        m.set_ability_bits(bits);
        let mut list = m.displayed_skills();
        list.count = LEARNED.len() as u8;
        list.ids[..LEARNED.len()].copy_from_slice(&LEARNED);
        m.set_displayed_skills(list);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("rage_party_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w
}

#[test]
fn a_whole_rage_party_skips_the_ring_and_plays_learned_arts() {
    let mut w = rage_party_world();
    let up = InputState::mask_of([PadButton::Up]);
    let cross = InputState::mask_of([PadButton::Cross]);

    let mut entered = false;
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(entered, "walking triggers the encounter");

    use legaia_engine_battle_menus::battle_input::CommandPhase;
    let mut ring_seen = false;
    let mut art_clips = 0usize;
    let mut swing_clips = 0usize;
    let mut last = [0u8; 3];
    let mut release = false;
    for _ in 0..4000 {
        if matches!(
            w.battle.command.as_ref().map(|s| &s.phase),
            Some(CommandPhase::Menu { .. })
        ) {
            ring_seen = true;
        }
        w.set_pad(if release { 0 } else { cross });
        release = !release;
        w.tick();
        for (seat, prev) in last.iter_mut().enumerate() {
            let clip = w.actors[seat].battle.latched_anim;
            if clip != *prev {
                if (0x1B..0x40).contains(&clip) {
                    assert!(
                        LEARNED.contains(&(clip - 0x1B)),
                        "seat {seat} latched {clip:#04X}, not a learned art"
                    );
                    art_clips += 1;
                } else if (0x0C..=0x0F).contains(&clip) {
                    swing_clips += 1;
                }
                *prev = clip;
            }
        }
        if w.mode == SceneMode::Field {
            break;
        }
    }
    assert!(!ring_seen, "no Rage seat is handed the command ring");
    assert!(art_clips > 0, "a Rage turn stages its learned arts");
    assert_eq!(
        swing_clips, 0,
        "no Rage turn falls back to the basic swings"
    );
}
