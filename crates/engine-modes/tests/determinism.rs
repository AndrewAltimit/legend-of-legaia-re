//! Same config + same seed + same pad stream => the same `state_hash`
//! after every tick. A different seed must diverge, or the test would pass
//! on a hash that ignores state.

use legaia_engine_modes::cards::CardBook;
use legaia_engine_modes::cards::duel::{DuelConfig, DuelSession};
use legaia_engine_modes::golf::{GolfConfig, GolfSession};
use legaia_engine_modes::poker::{PokerConfig, PokerSession};
use legaia_engine_modes::prop_hunt::{PropHuntConfig, PropHuntSession};
use legaia_engine_modes::racing::{RaceConfig, RaceSession};
use legaia_engine_modes::session::{AiTier, ModeSession};

use crate::common::{PLAY_MASK, PadScript, rebirth_cards, rebirth_modes, run};

fn check<S: ModeSession>(build: impl Fn(u64) -> S, max_frames: usize) {
    let (a, sa) = run(build(7), &mut PadScript::new(1, PLAY_MASK), max_frames);
    let (b, sb) = run(build(7), &mut PadScript::new(1, PLAY_MASK), max_frames);
    assert_eq!(a, b, "same seed and inputs diverged");
    assert_eq!(sa.results(), sb.results());
    let (c, _) = run(build(8), &mut PadScript::new(1, PLAY_MASK), max_frames);
    assert_ne!(a, c, "a different seed produced the same hash sequence");
}

#[test]
fn poker_is_deterministic() {
    let modes = rebirth_modes();
    check(
        |seed| {
            let mut cfg = PokerConfig::from_table(&modes.poker[0], seed);
            cfg.max_hands = Some(12);
            cfg.ai_think_frames = 2;
            cfg.showdown_frames = 4;
            PokerSession::new(cfg)
        },
        400_000,
    );
}

#[test]
fn card_duel_is_deterministic() {
    let modes = rebirth_modes();
    let book = CardBook::new(rebirth_cards());
    let mine: Vec<String> = book.section().cards[..5]
        .iter()
        .map(|c| c.id.clone())
        .collect();
    check(
        |seed| {
            let mut cfg = DuelConfig::from_duel(&modes.card_duels[0], &book, &mine, seed).unwrap();
            cfg.ai_think_frames = 3;
            DuelSession::new(cfg)
        },
        200_000,
    );
}

#[test]
fn golf_is_deterministic() {
    let modes = rebirth_modes();
    check(
        |seed| {
            let mut cfg =
                GolfConfig::from_course(&modes.golf[0], &[AiTier::Regular], seed).unwrap();
            cfg.ai_think_frames = 2;
            GolfSession::new(cfg)
        },
        400_000,
    );
}

#[test]
fn racing_is_deterministic() {
    let modes = rebirth_modes();
    check(
        |seed| {
            let mut cfg = RaceConfig::from_def(&modes.races[0], seed).unwrap();
            cfg.finish_grace = 600;
            RaceSession::new(cfg)
        },
        200_000,
    );
}

#[test]
fn prop_hunt_is_deterministic() {
    let modes = rebirth_modes();
    check(
        |seed| {
            let mut cfg = PropHuntConfig::from_arena(&modes.prop_hunt[0], seed).unwrap();
            cfg.hide_frames = 120;
            cfg.round_frames = 1200;
            PropHuntSession::new(cfg)
        },
        200_000,
    );
}
