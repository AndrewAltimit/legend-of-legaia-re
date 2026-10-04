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

/// The pad a player presses in the sparring fight: acknowledge a box, take
/// the command each lesson teaches, confirm. Item for the Items lesson (the
/// ring's up arm, then the first item on the first target), Spirit (the down
/// arm) for Spirit, Attack for Attacks and Hyper Arts.
fn lesson_pad(w: &World) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    use legaia_engine_core::battle_tutorial::TutorialLesson;
    let cross = InputState::mask_of([PadButton::Cross]);
    if !w.battle.tutorial_boxes.is_empty() || w.battle.item_menu.is_some() {
        return cross;
    }
    let lesson = w.battle.tutorial.as_ref().map(|t| t.lesson());
    match w.battle.command.as_ref().map(|c| &c.phase) {
        Some(CommandPhase::Menu { .. }) => match lesson {
            Some(TutorialLesson::Items) => InputState::mask_of([PadButton::Up]),
            Some(TutorialLesson::Spirit) => InputState::mask_of([PadButton::Down]),
            _ => InputState::mask_of([PadButton::Left]),
        },
        Some(
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. },
        ) => InputState::mask_of([PadButton::Left]),
        Some(CommandPhase::Targeting { .. }) | None => cross,
        _ => 0,
    }
}

/// Play the whole sparring fight with the pad, lesson by lesson, until the
/// world leaves battle. Returns the lesson counter's values in the order the
/// fight reached them.
fn play_every_lesson(w: &mut World) -> Vec<u8> {
    walk_into_battle(w);
    assert!(
        w.battle.tutorial.is_some(),
        "the primed fight arms the machine"
    );
    // The stand-in formation's monster is no Tetsu: give it the spar's
    // staying power so the lessons, not a knockout, end the fight. The spar
    // seats Vahn alone, so the monster is the seat right after him - pick it
    // by its monster id, not by a fixed party width.
    for a in w.actors.iter_mut() {
        if a.battle_monster_id.is_some() && a.battle.max_hp > 0 {
            a.battle.max_hp = 9999;
            a.battle.set_hp_synced(9999);
        }
    }
    // A wounded lead, so the leaf has a target to benefit - and, like the
    // real spar's Vahn, one the stand-in's full-strength hits cannot fell
    // across four lessons. Every write goes through the synced setter so the
    // HP readout pair stays coherent (a bare `hp` write is the absorbing
    // `0x51` bar-drain park, `BattleActor::set_hp_synced`).
    w.actors[0].battle.max_hp = 999;
    w.actors[0].battle.set_hp_synced(499);
    let mut seen = vec![0u8];
    let mut prev = 0u16;
    for _ in 0..40_000u32 {
        let want = lesson_pad(w);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        w.set_pad(pad);
        let _ = w.tick();
        if let Some(t) = w.battle.tutorial.as_ref()
            && seen.last() != Some(&t.lesson)
        {
            seen.push(t.lesson);
        }
        if w.mode != SceneMode::Battle {
            return seen;
        }
    }
    panic!(
        "the sparring fight never ended by play: lesson counter walked {seen:?}, flow {:?}, item window {:?}, bag {:?}, lead hp {}/{}",
        w.battle.flow,
        w.battle
            .item_menu
            .as_ref()
            .map(|m| (&m.state, &m.filtered_items)),
        w.party.inventory.get(&0x77),
        w.actors[0].battle.hp,
        w.actors[0].battle.max_hp,
    );
}

/// A leaf to heal with: the item window offers only an item some target
/// benefits from, and [`play_every_lesson`] wounds the lead once the fight
/// has seated its stats.
fn stock_the_items_lesson(w: &mut World) {
    w.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    w.party.inventory.add(0x77, 3); // Healing Leaf
}

/// The whole spar, played: each lesson commits the category it teaches, so
/// the counter walks `0 -> 1 -> 2 -> 3` and on past the last lesson, and
/// the fight ends.
///
/// The Items lesson is the regression. Retail's commit validator (overlay
/// 967, flow state `110`, `0x801F7088..0x801F7190`) reads the committed
/// category off the active actor (`lbu v1,0x1de(v0)`), and an item commit
/// writes `1` there. The engine once validated only Attack (`3`) and Spirit
/// (`4`), so an item use never met the validator, the lesson was never
/// accepted, and the counter sat at `1` for the rest of the fight.
#[test]
fn the_sparring_fight_is_won_by_playing_all_four_lessons() {
    let mut w = primed_world(None);
    stock_the_items_lesson(&mut w);
    let seen = play_every_lesson(&mut w);
    println!("sparring fight played lesson by lesson: counter walked {seen:?}");
    assert_eq!(&seen[..4], &[0, 1, 2, 3], "each lesson taught in turn");
    assert!(
        seen.last().is_some_and(|&l| l >= 4),
        "the fourth lesson completed the drill"
    );
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.system_flag_test(1), "story flag 1 = the survived outcome");
    // Each seated member takes the Items lesson's command in its turn.
    assert!(
        w.party.inventory.get(&0x77).copied().unwrap_or(0) < 3,
        "the Items lesson used a leaf"
    );
}

/// The same played spar seeded from the retail in-fight flag bank ends with
/// the retail post-fight bank.
#[test]
fn a_played_spar_leaves_the_flag_bank_as_retail_does() {
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
    let pre: [u8; 4] = pre.try_into().unwrap();
    let mut w = primed_world(Some(pre));
    stock_the_items_lesson(&mut w);
    let seen = play_every_lesson(&mut w);
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
        "played spar (counter {seen:?}): retail flag bank {pre:02x?} -> {post:02x?}; port {got:02x?}"
    );
    assert_eq!(&seen[..4], &[0, 1, 2, 3]);
    assert_eq!(got, post, "flag bank 0x80085758..5B after the played fight");
}

/// Play the spar with a stand-in that outlasts the lessons and a lead it
/// fells, until the wipe's end sequence has run (the scene leaves battle, or
/// the game-over hold is raised).
fn play_to_a_knockout(w: &mut World) {
    walk_into_battle(w);
    for a in w.actors.iter_mut() {
        if a.battle_monster_id.is_some() && a.battle.max_hp > 0 {
            a.battle.max_hp = 9999;
            a.battle.set_hp_synced(9999);
        }
    }
    w.actors[0].battle.set_hp_synced(1);
    let mut prev = 0u16;
    for _ in 0..20_000u32 {
        let want = lesson_pad(w);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        w.set_pad(pad);
        let _ = w.tick();
        if w.mode != SceneMode::Battle || w.game_over_hold {
            return;
        }
    }
    panic!(
        "the lead was never knocked out (lead hp {}, action state {:#x})",
        w.actors[0].battle.hp, w.battle_ctx.action_state
    );
}

/// A knockout in the spar ends the fight as retail's annihilated arm does.
///
/// With the scripted-loss latch the town01 record raises (flag 0, seeded from
/// the retail in-fight bank) the wipe returns to the field with Vahn floored
/// at 1 HP (`sh 1,0x14c` at `0x8004FBA4`). With the latch clear it is a game
/// over: the scene is held for the hand-off and **no further battle frame
/// runs** - retail has already switched to CARD INIT. The regression: the
/// held world kept ticking the action SM, which started a fresh round on the
/// floored lead and parked at the `0x51` bar-drain gate on a readout pair
/// (`hp 1`, shown `0`) the floor had left absorbing.
#[test]
fn a_knockout_in_the_spar_ends_the_fight() {
    // Latch set: back to the field, standing.
    let mut w = primed_world(Some([0x81, 0x02, 0x80, 0x00]));
    play_to_a_knockout(&mut w);
    assert_eq!(
        w.mode,
        SceneMode::Field,
        "a scripted loss returns to the field"
    );
    assert!(!w.game_over);
    assert!(
        !w.system_flag_test(1),
        "story flag 1 clear = the lost outcome"
    );

    // Latch clear: the game-over hold, frozen.
    let mut w = primed_world(None);
    play_to_a_knockout(&mut w);
    assert!(
        w.game_over && w.game_over_hold,
        "an unscripted wipe is a game over"
    );
    let b = &w.actors[0].battle;
    assert_eq!(b.hp, 1, "the annihilated arm floors the lead at 1 HP");
    assert!(
        b.hp_display.is_none_or(|shown| shown == b.hp) && b.hp_bar_pending == 0,
        "the floored lead's readout pair is settled, not absorbing"
    );
    let state = w.battle_ctx.action_state;
    for frame in 0..600 {
        w.set_pad(0);
        let _ = w.tick();
        assert_eq!(
            w.battle_ctx.action_state, state,
            "the held battle ran its action SM again {frame} frames into the hold"
        );
    }
    w.resolve_game_over_hold();
    assert_eq!(w.mode, SceneMode::Field);
}
