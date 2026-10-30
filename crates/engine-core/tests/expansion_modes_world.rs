//! The expansion game modes on the `World`: each built-in instance entered
//! from the field, driven to its end by scripted pad input through
//! `World::tick`, its rewards granted and the field restored.
//!
//! Synthetic world, no disc gate, no Sony bytes. No pack is loaded, so the
//! seat offers the built-in practice set.

use legaia_engine_core::world::World;
use legaia_engine_expansion::modes::{ModeStage, PendingCardReward};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::ModeKind;
use legaia_engine_system::pad::PadButton;

const CROSS: u16 = PadButton::Cross as u16;

fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.minigames.casino_coins = 1000;
    w.party.money = 500;
    w
}

/// One frame with `pad` held.
fn frame(w: &mut World, pad: u16) {
    w.input.set_pad(pad);
    w.tick();
}

/// Press and release `pad` (two frames).
fn press(w: &mut World, pad: u16) {
    frame(w, pad);
    frame(w, 0);
}

fn stage(w: &World) -> &ModeStage {
    &w.expansion
        .mode_seat
        .as_ref()
        .expect("the seat is open")
        .stage
}

fn in_results(w: &World) -> bool {
    matches!(
        w.expansion.mode_seat.as_deref().map(|s| &s.stage),
        Some(ModeStage::Results { .. })
    )
}

/// Drive the seat with `script(frame)` until it reaches its results, then
/// leave; returns the results stage's (rows, granted).
fn play_out(
    w: &mut World,
    cap: u32,
    mut script: impl FnMut(u32) -> u16,
) -> (Vec<legaia_engine_modes::session::Placement>, Vec<String>) {
    let mut n = 0;
    while !in_results(w) {
        assert!(n < cap, "the session did not end within {cap} frames");
        frame(w, script(n));
        n += 1;
    }
    let ModeStage::Results {
        rows,
        granted,
        completed,
        ..
    } = stage(w).clone()
    else {
        unreachable!()
    };
    assert!(completed);
    frame(w, 0);
    press(w, CROSS);
    assert_eq!(w.mode, SceneMode::Field, "leaving restores the field");
    assert!(w.expansion.mode_seat.is_none());
    (rows, granted)
}

/// A small deterministic generator for the scripted pads.
fn lcg(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *state >> 16
}

#[test]
fn the_picker_lists_the_practice_set_and_enters_a_mode() {
    let mut w = field_world();
    w.open_pack_mode_picker();
    assert_eq!(w.mode, SceneMode::Expansion);
    let ModeStage::Picker { entries, .. } = stage(&w).clone() else {
        panic!("picker")
    };
    assert_eq!(entries.len(), 5);
    assert_eq!(w.pack_mode_entries(), entries);
    // Down twice to golf, Cross to enter.
    press(&mut w, PadButton::Down.mask());
    press(&mut w, PadButton::Down.mask());
    press(&mut w, CROSS);
    let seat = w.expansion.mode_seat.as_ref().unwrap();
    assert_eq!(seat.active().map(|m| m.kind()), Some(ModeKind::Golf));
    // Circle on the picker would have left; inside a mode it does not.
    press(&mut w, PadButton::Circle.mask());
    assert_eq!(w.mode, SceneMode::Expansion);
}

#[test]
fn quitting_forfeits_and_restores_the_field() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-poker").unwrap();
    for _ in 0..200 {
        frame(&mut w, 0);
    }
    press(&mut w, PadButton::Start.mask());
    assert!(matches!(
        stage(&w),
        ModeStage::Playing {
            quit_prompt: true,
            ..
        }
    ));
    // Circle resumes, Start again re-asks, Cross leaves.
    press(&mut w, PadButton::Circle.mask());
    press(&mut w, PadButton::Start.mask());
    press(&mut w, CROSS);
    let ModeStage::Results {
        completed, granted, ..
    } = stage(&w).clone()
    else {
        panic!("results")
    };
    assert!(!completed && granted.is_empty());
    press(&mut w, CROSS);
    assert_eq!(w.mode, SceneMode::Field);
    assert_eq!(w.minigames.casino_coins, 1000, "a forfeit settles nothing");
    assert!(w.expansion.pending_card_rewards.is_empty());
}

#[test]
fn poker_settles_the_net_stack_into_the_coin_bank() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-poker").unwrap();
    // Fold every hand (the menu opens on Fold) and skip each showdown: the
    // table closes at its hand cap with the player's stack bled by blinds.
    let (rows, granted) = play_out(&mut w, 200_000, |n| if n % 2 == 0 { CROSS } else { 0 });
    let me = rows.iter().find(|p| p.seat == 0).unwrap();
    let net = me.score - 500;
    assert!(net < 0, "folding every hand loses chips");
    assert_eq!(i64::from(w.minigames.casino_coins), 1000 + net);
    assert_eq!(granted, vec![format!("{net:+} coins")]);
}

#[test]
fn golf_plays_every_hole_and_pays_a_win() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-golf").unwrap();
    // Start the meter, strike at 90 (it climbs 2 a frame), every second.
    let (rows, granted) = play_out(&mut w, 200_000, |n| match n % 60 {
        0 | 45 => CROSS,
        _ => 0,
    });
    assert_eq!(rows.len(), 2);
    let won = rows.iter().any(|p| p.seat == 0 && p.place == 1);
    let booster = PendingCardReward::Booster {
        id: "builtin-booster".into(),
        from_mode: "builtin-golf".into(),
    };
    assert_eq!(won, w.expansion.pending_card_rewards.contains(&booster));
    assert_eq!(won, granted.iter().any(|g| g == "Booster: builtin-booster"));
}

#[test]
fn a_duel_fills_the_grid_and_records_the_booster() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-duel").unwrap();
    let mut s = 7u32;
    let pads = [
        CROSS,
        PadButton::Left.mask(),
        PadButton::Right.mask(),
        PadButton::Up.mask(),
        PadButton::Down.mask(),
    ];
    let (rows, _) = play_out(&mut w, 100_000, |n| {
        if n % 2 == 1 {
            0
        } else {
            pads[(lcg(&mut s) % pads.len() as u32) as usize]
        }
    });
    assert_eq!(rows.len(), 2);
    let won = rows
        .iter()
        .all(|p| (p.seat == 0) == (p.place == 1) && p.place <= 2);
    assert_eq!(won, !w.expansion.pending_card_rewards.is_empty());
}

#[test]
fn race_and_prop_hunt_run_to_their_end() {
    let mut w = field_world();
    w.enter_pack_mode("builtin-race").unwrap();
    let (rows, _) = play_out(&mut w, 60_000, |_| CROSS);
    assert_eq!(rows.len(), 3);
    w.enter_pack_mode("builtin-prop-hunt").unwrap();
    let mut s = 3u32;
    let (rows, _) = play_out(&mut w, 60_000, |_| match lcg(&mut s) % 6 {
        0 => CROSS,
        1 => PadButton::Left.mask(),
        2 => PadButton::Right.mask(),
        3 => PadButton::Up.mask(),
        4 => PadButton::Down.mask(),
        _ => 0,
    });
    assert_eq!(rows.len(), 3);
    // Neither pays currency: the purses are untouched.
    assert_eq!((w.minigames.casino_coins, w.party.money), (1000, 500));
}

#[test]
fn an_unknown_instance_leaves_the_world_alone() {
    let mut w = field_world();
    assert!(w.enter_pack_mode("no-such-mode").is_err());
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.expansion.mode_seat.is_none());
}
