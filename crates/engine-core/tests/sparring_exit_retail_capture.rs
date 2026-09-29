//! The Tetsu sparring fight **ends**, and it leaves the story-flag bank the
//! way retail's does.
//!
//! Retail's exit is the overlay-967 hook's `ctx[+0x6B4]` countdown: the
//! completion tail (`0x801F7380`) arms it through `FUN_801F7628`
//! (`jal` at `0x801F7460`), and its expiry with the lesson counter past `3`
//! (`0x801F72A8..0x801F7378`) bumps the side-band phase `ctx[+0x289]` to `3`,
//! raises the battle-end signal and the party-survived bit
//! `DAT_8007BD60 |= 0x80` (`0x801F735C`). The side-band's phase 3 then counts
//! `ctx[+0x6CE]` up to the frame driver's exit gate `0x43`
//! (`FUN_80046A20` `0x80046DAC`), and MAIN INIT's back-from-battle arm
//! (`FUN_8003AEB0` `0x8003B518..0x8003B60C`) turns the survived bit into
//! story flag 1.
//!
//! Two halves:
//!
//! * **disc-free** - a primed sparring fight entered through a real
//!   encounter, walked to its completion tail, must hand the world back to
//!   the field. Before the countdown was ported the machine disarmed at the
//!   tail and the fight never ended.
//! * **save-library-gated** - the four flag-bank bytes `0x80085758..5B` read
//!   out of the retail in-fight state (`v0_1_battle_start_tetsu`) are loaded
//!   into the world, and after the port's exit they must equal the retail
//!   post-fight state's (`v0_1_post_battle_tetsu_town`): flag 1 set, flag 0
//!   (the scripted-loss latch the town01 record raised) consumed, flag 14
//!   cleared.
//!
//! The library is found through `LEGAIA_SAVES_LIBRARY` (the
//! `saves/library` directory) before the repo-relative fallback.

use legaia_engine_core::battle_tutorial::{BattleTutorialScript, OVERLAY_967_BASE_VA, msg};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_mednafen::SaveState;
use std::path::PathBuf;

/// `v0_1_battle_start_tetsu` (backup fingerprint prefix) - in the fight.
const IN_FIGHT: &str = "a4673cb5";
/// `v0_1_post_battle_tetsu_town` - back in town01 after it.
const POST_FIGHT: &str = "1b1d645f";
const FLAG_BANK: u32 = 0x8008_5758;
const BATTLE_FLAGS: u32 = 0x8007_BD60;
const STAGE_ID: u32 = 0x8007_B64A;

fn synthetic_script() -> BattleTutorialScript {
    let base = OVERLAY_967_BASE_VA;
    let mut ids: Vec<u32> = BattleTutorialScript::MESSAGE_IDS.to_vec();
    ids.push(msg::ENTER_HIGH_LOW_HIGH);
    ids.push(msg::WRONG_COMMANDS);
    ids.push(msg::PRACTICE_OVER);
    let span = ids.iter().map(|v| v - base).max().unwrap() as usize + 16;
    let mut bytes = vec![0u8; span];
    for va in ids {
        let off = (va - base) as usize;
        let marker = format!("m{va:08X}");
        bytes[off..off + marker.len()].copy_from_slice(marker.as_bytes());
    }
    BattleTutorialScript::from_overlay(&bytes, base)
}

/// A field world one encounter away from a primed sparring fight, with the
/// flag bank's first four bytes optionally seeded.
fn primed_world(flag_bytes: Option<[u8; 4]>) -> World {
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
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("sparring_exit");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    if let Some(bytes) = flag_bytes {
        for (i, b) in bytes.iter().enumerate() {
            for bit in 0..8u16 {
                let idx = i as u16 * 8 + bit;
                if b & (0x80 >> bit) != 0 {
                    w.system_flag_set(idx);
                } else {
                    w.system_flag_clear(idx);
                }
            }
        }
    }

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w.prime_battle_tutorial(synthetic_script());
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

/// Enter the fight, bump the lesson counter to `4` - the action SM's
/// `case 0xFF` store (`World::advance_battle_mode`) after the hyper-arts
/// lesson - and tick with an idle pad until the world leaves battle. The
/// per-frame hook runs the completion tail on the next frame. Returns the
/// frames spent in battle after the tail ran.
fn run_to_exit(w: &mut World) -> u32 {
    walk_into_battle(w);
    w.battle
        .tutorial
        .as_mut()
        .expect("the primed fight arms the machine")
        .lesson = 4;
    let mut closed_at = None;
    for frame in 0..4000u32 {
        w.set_pad(0);
        let _ = w.tick();
        if closed_at.is_none() && w.battle.tutorial.as_ref().is_some_and(|t| t.finished) {
            closed_at = Some(frame);
        }
        if w.mode != SceneMode::Battle {
            let closed = closed_at.expect("the completion tail ran");
            return frame - closed;
        }
    }
    panic!(
        "the sparring fight never exited (tail ran: {}, side-band phase {})",
        closed_at.is_some(),
        w.battle.sideband.phase
    );
}

#[test]
fn the_sparring_fight_hands_back_to_the_field_after_its_last_lesson() {
    let mut w = primed_world(None);
    let frames = run_to_exit(&mut w);
    println!("sparring fight exited {frames} frames after its completion tail");
    assert_eq!(w.mode, SceneMode::Field);
    assert_eq!(w.battle.stage_id, 0);
    assert!(w.system_flag_test(1), "story flag 1 = the survived outcome");
    assert!(!w.game_over);
}

fn library() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LEGAIA_SAVES_LIBRARY") {
        let p = PathBuf::from(p).join("mednafen");
        if p.is_dir() {
            return Some(p);
        }
    }
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}saves/library/mednafen")))
        .find(|p| p.is_dir())
}

fn state_bytes(lib: &std::path::Path, prefix: &str, va: u32, n: usize) -> Option<Vec<u8>> {
    let path = std::fs::read_dir(lib)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with(prefix))
        })?;
    let state = SaveState::from_path(&path).ok()?;
    let ram = state.main_ram().ok()?;
    let o = (va & 0x001F_FFFF) as usize;
    Some(ram[o..o + n].to_vec())
}

#[test]
fn the_sparring_exit_leaves_the_flag_bank_as_retail_does() {
    let Some(lib) = library() else {
        eprintln!("[skip] saves/library/mednafen missing (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let (Some(pre), Some(post)) = (
        state_bytes(&lib, IN_FIGHT, FLAG_BANK, 4),
        state_bytes(&lib, POST_FIGHT, FLAG_BANK, 4),
    ) else {
        eprintln!("[skip] the two Tetsu anchors are not in {}", lib.display());
        return;
    };
    // The capture pair's own facts: the fight runs on stage 1, and the
    // survived bit is up once it has ended.
    let stage = state_bytes(&lib, IN_FIGHT, STAGE_ID, 1).unwrap()[0];
    let survived = state_bytes(&lib, POST_FIGHT, BATTLE_FLAGS, 1).unwrap()[0];
    assert_eq!(
        stage, 1,
        "the in-fight anchor is the stage-1 sparring fight"
    );
    assert_eq!(survived & 0x80, 0x80, "retail left the survived bit up");

    let pre: [u8; 4] = pre.try_into().unwrap();
    let mut w = primed_world(Some(pre));
    run_to_exit(&mut w);
    let got: Vec<u8> = (0..4u16)
        .map(|byte| {
            (0..8u16).fold(0u8, |acc, bit| {
                if w.system_flag_test(byte * 8 + bit) {
                    acc | (0x80 >> bit)
                } else {
                    acc
                }
            })
        })
        .collect();
    println!(
        "retail flag bank {:02x?} -> {:02x?}; port after the sparring exit {:02x?}",
        pre, post, got
    );
    assert_ne!(
        pre.to_vec(),
        post,
        "the capture pair differs across the fight"
    );
    assert_eq!(got, post, "flag bank 0x80085758..5B after the fight");
}
