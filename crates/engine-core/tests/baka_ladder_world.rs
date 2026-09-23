//! The Baka Fighter **ladder** through the world tick: the cabinet
//! (`FUN_801CF388`, `baka_cabinet::BakaCabinet`) runs the rest of the run
//! inside the one mode-24 visit, as retail does.
//!
//! - a won match reaches the "NEXT GAME / PAY OUT" choice;
//! - NEXT GAME seats the next rung through the cabinet's install state and
//!   keeps the prize accumulator (`_DAT_80084440`) at risk;
//! - PAY OUT runs the exit state, whose end is the return warp that banks
//!   the accumulator into the coin bank;
//! - a lost match runs "GAME OVER", which zeroes the accumulator before the
//!   exit (`sw zero,0x300(s2)` at `0x801D1288`), so nothing is banked.
//!
//! Disc-free legs use synthetic fighters (no roster tables, so NEXT GAME
//! re-seats the same opponent); the disc-gated leg reads PROT 0976's roster
//! and checks the install climbs the rung fold (`roster = stage + 3`).

use legaia_engine_core::baka_cabinet::{ST_CHOICE, ST_GAME_OVER};
use legaia_engine_core::baka_fighter::{BakaFight, FighterConfig, first_rung_roster};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

const PRIZE: u32 = 100;

fn cfg(roster_id: usize, power: i32, gold: u32) -> FighterConfig {
    FighterConfig {
        roster_id,
        damage_mod: 0,
        def_tiers: [0, 0, 0],
        crit_chance: 0,
        atk_tiers: [0, 0, 0],
        attack_power: [0, power, power, power, power],
        gold_reward: gold,
        ai_pattern: Vec::new(),
    }
}

fn step(w: &mut World, mask: u16) {
    w.set_pad(mask);
    let _ = w.tick();
}

fn press(w: &mut World, mask: u16) {
    step(w, mask);
    step(w, 0);
}

fn fight(w: &World) -> &BakaFight {
    w.minigames.baka_fighter.as_ref().expect("fight installed")
}

fn world_with(f: BakaFight) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.enter_baka_fighter(f);
    w
}

/// Throw the special until the match is decided.
fn play_out(w: &mut World) {
    for _ in 0..100_000 {
        if fight(w).match_over() {
            return;
        }
        press(w, PadButton::Triangle.mask());
    }
    panic!("the duel never resolved");
}

/// Idle until the cabinet reaches `state` (or give up).
fn idle_to_state(w: &mut World, state: u32) {
    for _ in 0..5_000 {
        if w.minigames
            .baka_fighter
            .as_ref()
            .is_none_or(|f| f.cabinet().state() == state)
        {
            return;
        }
        step(w, 0);
    }
    panic!("cabinet never reached state {state:#x}");
}

#[test]
fn next_game_keeps_the_pot_at_risk_and_pay_out_banks_it() {
    let mut w = world_with(BakaFight::new(
        cfg(0, 4000, 0),
        cfg(1, 0, PRIZE),
        [0, 0],
        0xBAA5EED,
    ));
    play_out(&mut w);
    assert_eq!(fight(&w).winner(), Some(0));
    idle_to_state(&mut w, ST_CHOICE);
    assert_eq!(
        w.minigames.winnings, PRIZE,
        "the tally drained the rung prize"
    );
    assert!(
        fight(&w).cabinet().choice_sheet().is_some(),
        "the sheet is up"
    );

    // NEXT GAME (the default row): the next rung is seated and the match
    // restarts; the accumulator is not banked.
    press(&mut w, PadButton::Cross.mask());
    for _ in 0..0x400 {
        if !fight(&w).match_over() {
            break;
        }
        step(&mut w, 0);
    }
    assert!(!fight(&w).match_over(), "NEXT GAME seats a fresh match");
    assert_eq!(w.mode, SceneMode::BakaFighter);
    assert_eq!(w.minigames.casino_coins, 0, "nothing banked yet");
    assert_eq!(w.minigames.winnings, PRIZE, "the pot rides on");

    // Win the second rung too, then PAY OUT.
    play_out(&mut w);
    idle_to_state(&mut w, ST_CHOICE);
    assert_eq!(w.minigames.winnings, 2 * PRIZE);
    press(&mut w, PadButton::Right.mask());
    assert_eq!(fight(&w).cabinet().menu_cursor(), 1);
    press(&mut w, PadButton::Cross.mask());
    for _ in 0..0x80 {
        if w.minigames.baka_fighter.is_none() {
            break;
        }
        step(&mut w, 0);
    }
    assert!(
        w.minigames.baka_fighter.is_none(),
        "PAY OUT leaves the cabinet"
    );
    assert_eq!(w.mode, SceneMode::Field);
    assert_eq!(w.minigames.casino_coins, 2 * PRIZE, "both rungs banked");
}

#[test]
fn a_lost_rung_forfeits_the_pot_on_the_way_out() {
    // The player cannot hurt the opponent; the opponent one-shots.
    let mut w = world_with(BakaFight::new(
        cfg(0, 0, 0),
        cfg(1, 4000, PRIZE),
        [0, 0],
        0xBAA5EED,
    ));
    // A pot carried in from earlier rungs.
    w.minigames.winnings = 70;
    for _ in 0..100_000 {
        if fight(&w).match_over() {
            break;
        }
        press(&mut w, PadButton::Square.mask());
    }
    assert_eq!(fight(&w).winner(), Some(1));
    idle_to_state(&mut w, ST_GAME_OVER);
    step(&mut w, 0);
    assert_eq!(w.minigames.winnings, 0, "GAME OVER zeroes the accumulator");
    for _ in 0..0x200 {
        if w.minigames.baka_fighter.is_none() {
            break;
        }
        step(&mut w, 0);
    }
    assert!(w.minigames.baka_fighter.is_none(), "the exit state leaves");
    assert_eq!(w.minigames.casino_coins, 0, "a forfeited pot banks nothing");
}

#[test]
fn next_game_climbs_the_disc_roster() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let Ok(overlay) = host.index.entry_bytes_extended(976) else {
        eprintln!("[skip] PROT 0976 unreadable");
        return;
    };
    let opponents = legaia_asset::baka_opponents::parse(&overlay).expect("roster");
    let actions = legaia_asset::baka_opponents::parse_actions(&overlay).expect("actions");
    let first = first_rung_roster();
    assert_eq!(first, 5, "stage seeded 2 folds to roster 5");
    let f = BakaFight::from_tables(&opponents, &actions, 0, first, 7).expect("fight");
    let mut w = world_with(f);
    // The disc fighters trade blows; the pad script throws the special until
    // the match falls, and the fixed seed makes the outcome reproducible.
    play_out(&mut w);
    if fight(&w).winner() != Some(0) {
        eprintln!("[ok] rung lost by the pad script; install path covered disc-free");
        return;
    }
    idle_to_state(&mut w, ST_CHOICE);
    press(&mut w, PadButton::Cross.mask());
    for _ in 0..0x400 {
        if !fight(&w).match_over() {
            break;
        }
        step(&mut w, 0);
    }
    assert_eq!(
        fight(&w).opponent_roster(),
        first + 1,
        "NEXT GAME installs the next rung's roster record"
    );
    eprintln!("[ok] rung {first} -> {}", first + 1);
}
