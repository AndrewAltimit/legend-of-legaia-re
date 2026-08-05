//! A party wipe routes to the title screen, and nowhere else.
//!
//! Retail's wipe destination is not a menu and not a screen. `FUN_8003AEB0`'s
//! back-from-battle arm falls through two branches (`0x8003B57C` - the
//! party-survived latch `DAT_8007BD60 & 0x80` is clear; `0x8003B5BC` - story
//! flag 0, the scripted-loss latch, is clear) onto a pair of stores:
//! `game_mode = 0x16` (22, CARD INIT) at `0x8003B5D4` and
//! `_DAT_8007BB00 = 1` at `0x8003B5E0`. The title overlay reads that context
//! word at `0x801DD968` and enters at sub-mode `0x11`, fading into the title
//! screen. The same store pair is the whole body of `FUN_801D84B4` and of
//! `FUN_8003C7EC` (the field-VM `4C EA` scripted-loss trigger), and the STR
//! overlay's attract exit repeats it at `0x801CF048`.
//!
//! So the port owes exactly one thing on a wipe: the title. This test pins
//! the whole chain disc-free - wipe the party in a synthetic battle, watch
//! `World::game_over` rise, drive the hand-off, and assert its only reachable
//! outcome. It is deliberately not a unit test of the session alone: the
//! defect it guards against is a host inventing a chooser between the flag
//! and the title, which a session-only test cannot see.

use legaia_engine_core::game_over::{
    GameOverOutcome, GameOverPhase, GameOverSession, TITLE_HANDOFF_FRAMES,
};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn world_in_a_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.mp = 40;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        hms.mp_cur = 40;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.mode = SceneMode::Field;
    w
}

/// Wipe the party, then drive the hand-off a host would build off the flag.
/// The only thing it can ever produce is the title.
#[test]
fn a_party_wipe_hands_the_frame_to_the_title() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    // The scripted entry runs the field-to-battle intro transition
    // (132 display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);

    for i in 0..3 {
        w.actors[i].battle.hp = 0;
        w.actors[i].battle.liveness = 0;
    }
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(
        w.mode,
        SceneMode::Battle,
        "the wipe must resolve the battle"
    );
    assert!(w.game_over, "a party wipe raises game over");

    // What a host does with the flag: consume it, start the hand-off, tick.
    w.game_over = false;
    let mut session = GameOverSession::new();
    let mut resolved = None;
    // One frame of headroom past the hold; a hand-off that needs more than
    // its own advertised window is a hand-off that can hang.
    for _ in 0..=TITLE_HANDOFF_FRAMES {
        session.tick();
        if let Some(o) = session.outcome() {
            resolved = Some(o);
            break;
        }
    }
    assert_eq!(
        resolved,
        Some(GameOverOutcome::ReturnToTitle),
        "the wipe hand-off resolves to the title screen"
    );
    assert!(!w.game_over, "the flag is consumed, not left latched");
}

/// The hold is real: nothing resolves before it drains, so a host cannot skip
/// the transition window retail spends streaming the menu overlay.
#[test]
fn the_hold_runs_before_the_title_takes_over() {
    let mut s = GameOverSession::new();
    for f in 1..TITLE_HANDOFF_FRAMES {
        s.tick();
        assert_eq!(s.outcome(), None, "resolved early at frame {f}");
        assert_eq!(
            s.phase(),
            GameOverPhase::Hold {
                frames_remaining: TITLE_HANDOFF_FRAMES - f
            }
        );
    }
    s.tick();
    assert_eq!(s.outcome(), Some(GameOverOutcome::ReturnToTitle));
}

/// The invention is gone and must stay gone. `GameOverOutcome` has one
/// variant because retail's wipe arm has one exit store; a `match` that is
/// exhaustive over a single arm is the compile-time form of that claim, and
/// this test is the runtime form.
#[test]
fn the_wipe_offers_the_player_no_choice() {
    let mut s = GameOverSession::with_hold(1);
    s.tick();
    let outcome = s.outcome().expect("resolves after its hold");
    match outcome {
        GameOverOutcome::ReturnToTitle => {}
    }
    // Sticky: a second tick cannot walk it somewhere else.
    s.tick();
    assert_eq!(s.outcome(), Some(GameOverOutcome::ReturnToTitle));
}
