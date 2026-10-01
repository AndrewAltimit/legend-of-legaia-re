//! Baka Fighter's **front end** through `World::tick`: the attract card, the
//! player select, and the hand-off into the first rung - the screens every
//! host now enters the cabinet on (`scene/host/minigame_warp.rs` boots the
//! fight with `with_attract`).
//!
//! Disc-free: the roster and action tables are synthetic, shaped like the
//! parsed overlay tables (`legaia_asset::baka_opponents`), with a distinct
//! stand-off per record so the seated pick is observable.

use legaia_asset::baka_opponents::{BakaActionSet, BakaOpponent, OPPONENT_COUNT};
use legaia_engine_core::baka_cabinet::{
    ST_ATTRACT, ST_ATTRACT_OUT, WIDGET_PLAYER_SELECT, WIDGET_PRESS_START,
};
use legaia_engine_core::baka_fighter::{BakaFight, HP_START, first_rung_roster};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

fn opp(i: usize) -> BakaOpponent {
    BakaOpponent {
        index: i,
        gold_reward: 10 * i as u32,
        damage_mod: 100,
        def_tiers: [0; 3],
        crit_chance: 0,
        atk_tiers: [0; 3],
        stand_off: (i * 4) as i16,
        ai_pattern: vec![],
    }
}

fn act(i: usize) -> BakaActionSet {
    BakaActionSet {
        index: i,
        power: [0, 10, 10, 10, 0, 0, 0, 0, 0],
        keyframes: [0, 1, 1, 1, 2, 0, 0, 0, 0],
        speed: [16; 9],
        sub_keyframes: Default::default(),
    }
}

fn world() -> World {
    let opponents: Vec<_> = (0..OPPONENT_COUNT).map(opp).collect();
    let actions: Vec<_> = (0..OPPONENT_COUNT).map(act).collect();
    let fight = BakaFight::from_tables(&opponents, &actions, 0, first_rung_roster(), 0xBA4A)
        .expect("fight")
        .with_attract();
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.enter_baka_fighter(fight);
    w
}

fn step(w: &mut World, mask: u16) {
    w.input.set_pad(mask);
    let _ = w.tick();
}

fn press(w: &mut World, b: PadButton) {
    step(w, b.mask());
    step(w, 0);
}

fn fight(w: &World) -> &BakaFight {
    w.minigames.baka_fighter.as_ref().expect("fight live")
}

fn has_cell(w: &World, widget: u8) -> bool {
    fight(w).cabinet_cells().iter().any(|c| c.widget == widget)
}

#[test]
fn the_attract_card_holds_with_its_prompt_and_runs_no_fight() {
    let mut w = world();
    let mut card = false;
    for _ in 0..400 {
        step(&mut w, 0);
        card |= fight(&w)
            .chrome_frame()
            .draws
            .iter()
            .any(|d| d.widget == 0x28);
    }
    let f = fight(&w);
    assert_eq!(f.cabinet().state(), ST_ATTRACT, "waits for start");
    assert!(card, "the title card drew off the cabinet clock");
    assert!(
        f.chrome_frame().draws.iter().any(|d| d.widget == 0x28),
        "the card still shows long after its ramps (the parked state sits at t = 373)"
    );
    assert!(has_cell(&w, WIDGET_PRESS_START));
    assert!(f.last_exchange().is_none(), "no exchange before the pick");
    assert_eq!([f.hp(0), f.hp(1)], [HP_START, HP_START]);
}

#[test]
fn the_pick_seats_the_cursor_fighter_and_starts_the_first_rung() {
    let mut w = world();
    for _ in 0..10 {
        step(&mut w, 0);
    }
    step(&mut w, PadButton::Start.mask());
    step(&mut w, 0);
    assert_eq!(fight(&w).cabinet().state(), ST_ATTRACT_OUT);
    for _ in 0..0x40 {
        step(&mut w, 0);
    }
    assert_eq!(fight(&w).select_lineup().map(|l| l.0), Some(0));
    assert!(has_cell(&w, WIDGET_PLAYER_SELECT));

    // Right steps Vahn -> Noa; Left twice wraps Noa -> Vahn -> Gala.
    press(&mut w, PadButton::Right);
    assert_eq!(fight(&w).select_lineup().map(|l| l.0), Some(1));
    press(&mut w, PadButton::Left);
    press(&mut w, PadButton::Left);
    assert_eq!(fight(&w).select_lineup().map(|l| l.0), Some(2));
    press(&mut w, PadButton::Right);
    press(&mut w, PadButton::Right);
    assert_eq!(fight(&w).select_lineup().map(|l| l.0), Some(1));

    press(&mut w, PadButton::Cross);
    for _ in 0..0x40 {
        if !fight(&w).cabinet().front_end() {
            break;
        }
        step(&mut w, 0);
    }
    let f = fight(&w);
    assert!(
        !f.cabinet().front_end(),
        "the select hands off to the ladder"
    );
    assert!(f.select_lineup().is_none());
    assert_eq!(f.player_roster(), 1, "the pick is the roster record");
    assert_eq!(f.opponent_roster(), first_rung_roster());
    // Record 1's own stand-off places the player.
    assert_eq!(f.fighter_position(0)[0], -(4.0 + 200.0));
}
