//! Baka Fighter **end-of-match score tally** through the world tick.
//!
//! The retail result screen (`FUN_801d239c`) does not hand the player the
//! prize in one lump: it drains four counters, each gated behind its own
//! fade-in, at the proportional rate `FUN_801d6710` returns, and adds every
//! step straight into party gold. This asserts the engine does the same -
//! that the prize arrives *over frames* through `World::tick`, that a face
//! button snaps it, and that the total paid still equals the prize however
//! the duel is left.
//!
//! Disc-free: the fighters are synthetic configs, so nothing here is gated
//! on `LEGAIA_DISC_BIN` and no Sony bytes are touched.

use legaia_engine_core::baka_fighter::{BakaFight, FighterConfig, MatchPhase, TALLY_FADE_GATE};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

const PRIZE: u32 = 100;

/// A fighter that hits hard enough to end a round in one exchange.
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

/// One world frame with `mask` held. The world's pad is edge-triggered, so
/// callers that want a repeated press must release in between - [`press`].
fn step(world: &mut World, mask: u16) {
    world.set_pad(mask);
    let _ = world.tick();
}

/// A press-then-release pair: two frames, one edge.
fn press(world: &mut World, mask: u16) {
    step(world, mask);
    step(world, 0);
}

/// Drive a match to a player win: the player throws the special, which is an
/// unbeatable exchange win, until two rounds are taken.
fn play_to_player_win(world: &mut World) {
    for _ in 0..100_000 {
        let f = world.baka_fighter.as_ref().expect("fight installed");
        if f.match_over() {
            return;
        }
        // Down = the special (type 4) in the world's pad mapping.
        press(world, PadButton::Down.mask());
    }
    panic!("match did not terminate");
}

fn start(world: &mut World) {
    world.mode = SceneMode::Field;
    // Player power large enough to KO in one landed special; opponent inert.
    let fight = BakaFight::new(cfg(0, 4000, 0), cfg(1, 0, PRIZE), [0, 0], 0xBAA5EED);
    world.enter_baka_fighter(fight);
}

#[test]
fn the_prize_arrives_over_frames_not_in_one_step() {
    let mut world = World::new();
    let money0 = world.money;
    start(&mut world);
    play_to_player_win(&mut world);

    let f = world.baka_fighter.as_ref().expect("fight installed");
    assert_eq!(f.winner(), Some(0), "player takes the match");
    assert!(matches!(f.phase(), MatchPhase::MatchOver(0)));
    assert!(f.tally().is_some(), "a won match installs the tally screen");

    // THE WIRING ASSERTION. Retail pays the prize a step at a time while the
    // result screen is up. If the world tick did not run the tally, money
    // would still be untouched here and only move on exit.
    assert_eq!(world.money, money0, "nothing paid before the tally runs");
    // The row must fade in before its first step, so an early frame pays 0.
    step(&mut world, 0);
    assert_eq!(world.money, money0, "the row stalls while it fades in");

    for _ in 0..TALLY_FADE_GATE {
        step(&mut world, 0);
    }
    let partway = world.money;
    assert!(
        partway > money0,
        "the tally has started paying into gold: {partway} > {money0}"
    );
    assert!(
        partway < money0 + PRIZE as i32,
        "but has not finished in one step: {partway}"
    );

    // Run it out; the full prize lands and no more.
    for _ in 0..5_000 {
        step(&mut world, 0);
    }
    assert_eq!(
        world.money,
        money0 + PRIZE as i32,
        "the whole prize reaches party gold"
    );
    assert_eq!(
        world.baka_fighter.as_ref().unwrap().tally_gold_remaining(),
        0,
        "nothing left owed"
    );

    // Leaving after a finished tally must not double-pay.
    step(&mut world, PadButton::Cross.mask());
    assert!(world.baka_fighter.is_none(), "Cross leaves the duel");
    assert_eq!(
        world.money,
        money0 + PRIZE as i32,
        "exit does not pay the prize twice"
    );
}

#[test]
fn a_face_button_snaps_the_tally_to_its_end_state() {
    let mut world = World::new();
    let money0 = world.money;
    start(&mut world);
    play_to_player_win(&mut world);

    for _ in 0..=TALLY_FADE_GATE {
        step(&mut world, 0);
    }
    assert!(world.money < money0 + PRIZE as i32, "tally still running");

    // The retail fast-forward latch (`_DAT_8007b874 & 0xf0` → `DAT_801dbf00`)
    // moves the whole remainder in one step.
    step(&mut world, PadButton::Square.mask());
    assert_eq!(
        world.money,
        money0 + PRIZE as i32,
        "a face button snaps the whole remainder into gold"
    );
}

#[test]
fn leaving_mid_tally_still_banks_exactly_the_prize() {
    let mut world = World::new();
    let money0 = world.money;
    start(&mut world);
    play_to_player_win(&mut world);

    // Pay part of it out, then walk away with the tally unfinished.
    for _ in 0..(TALLY_FADE_GATE + 3) {
        step(&mut world, 0);
    }
    let partway = world.money;
    assert!(
        partway > money0 && partway < money0 + PRIZE as i32,
        "mid-tally"
    );

    step(&mut world, PadButton::Cross.mask());
    assert!(world.baka_fighter.is_none(), "duel left");
    assert_eq!(
        world.money,
        money0 + PRIZE as i32,
        "the undrained remainder is banked on exit - total is the prize"
    );
}

#[test]
fn a_lost_match_installs_no_tally_and_pays_nothing() {
    let mut world = World::new();
    let money0 = world.money;
    world.mode = SceneMode::Field;
    // The player chips (power 1 nets 0 damage on a fresh streak); the CPU
    // one-shots. The CPU takes both rounds long before the chip damage adds
    // up, so this is a loss - deterministic under the fixed seed.
    let fight = BakaFight::new(cfg(0, 1, 0), cfg(1, 4000, PRIZE), [0, 0], 0xBAA5EED);
    world.enter_baka_fighter(fight);
    for _ in 0..100_000 {
        if world.baka_fighter.as_ref().expect("installed").match_over() {
            break;
        }
        press(&mut world, PadButton::Left.mask());
    }
    let f = world.baka_fighter.as_ref().expect("installed");
    assert_eq!(f.winner(), Some(1), "the CPU takes the match");
    assert!(f.tally().is_none(), "a beaten player gets no tally");

    for _ in 0..500 {
        step(&mut world, 0);
    }
    assert_eq!(world.money, money0, "a loss pays nothing while on screen");
    step(&mut world, PadButton::Cross.mask());
    assert_eq!(world.money, money0, "and nothing on exit");
}
