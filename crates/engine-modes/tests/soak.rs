//! Random pads over many seeds: no panic, chips / cards conserved where
//! the rules say so, and every session ends within a bound.

use legaia_engine_modes::cards::CardBook;
use legaia_engine_modes::cards::duel::{DuelConfig, DuelSession};
use legaia_engine_modes::golf::{GolfConfig, GolfSession};
use legaia_engine_modes::poker::{PokerConfig, PokerSession};
use legaia_engine_modes::prop_hunt::{PropHuntConfig, PropHuntSession};
use legaia_engine_modes::racing::{RaceConfig, RaceSession};
use legaia_engine_modes::session::{AiTier, ModeSession, SeatKind};

use crate::common::{PLAY_MASK, PadScript, rebirth_cards, rebirth_modes, run};

const SEEDS: u64 = 12;

#[test]
fn poker_soak_all_human_seats() {
    let modes = rebirth_modes();
    for seed in 0..SEEDS {
        let mut cfg = PokerConfig::from_table(&modes.poker[0], seed);
        // Every seat on random pads: humans fold, raise and shove at will.
        cfg.seats = vec![SeatKind::Human; 2 + (seed as usize % 5)];
        cfg.max_hands = Some(25);
        cfg.showdown_frames = 3;
        let total = cfg.buy_in as u64 * cfg.seats.len() as u64;
        let (_, s) = run(
            PokerSession::new(cfg),
            &mut PadScript::new(seed, PLAY_MASK),
            2_000_000,
        );
        let v = s.view();
        let chips: u64 = v.seats.iter().map(|s| s.chips as u64).sum();
        assert_eq!(chips, total, "seed {seed}: chips not conserved");
    }
}

#[test]
fn card_duel_soak() {
    let modes = rebirth_modes();
    let book = CardBook::new(rebirth_cards());
    let ids: Vec<String> = book.section().cards.iter().map(|c| c.id.clone()).collect();
    for seed in 0..SEEDS {
        let start = (seed as usize * 3) % (ids.len() - 5);
        let mine = &ids[start..start + 5];
        let mut duel = modes.card_duels[0].clone();
        duel.chain = seed % 2 == 0;
        duel.stakes = seed % 3 == 0;
        let mut cfg = DuelConfig::from_duel(&duel, &book, mine, seed).unwrap();
        if seed % 4 == 0 {
            cfg.seats = [SeatKind::Human, SeatKind::Human];
        }
        let (_, s) = run(
            DuelSession::new(cfg),
            &mut PadScript::new(seed, PLAY_MASK),
            500_000,
        );
        let sc = s.view().scores;
        assert_eq!(sc[0] + sc[1], 10, "seed {seed}");
    }
}

#[test]
fn golf_soak() {
    let modes = rebirth_modes();
    for seed in 0..SEEDS {
        let mut cfg =
            GolfConfig::from_course(&modes.golf[0], &[AiTier::Novice, AiTier::Expert], seed)
                .unwrap();
        cfg.ai_think_frames = 1;
        if seed % 2 == 0 {
            cfg.seats.push(SeatKind::Human);
        }
        let (_, s) = run(
            GolfSession::new(cfg),
            &mut PadScript::new(seed, PLAY_MASK),
            3_000_000,
        );
        for (seat, row) in s.view().scorecard.iter().enumerate() {
            for (hole, strokes) in row.iter().enumerate() {
                let st = strokes.unwrap_or_else(|| panic!("seed {seed} seat {seat} hole {hole}"));
                // The Rebirth holes are par 2 / 3, capped at par + 5.
                assert!((1..=8).contains(&st), "seed {seed}: {st} strokes");
            }
        }
    }
}

#[test]
fn racing_soak() {
    let modes = rebirth_modes();
    for seed in 0..SEEDS {
        let mut cfg = RaceConfig::from_def(&modes.races[0], seed).unwrap();
        cfg.seats[1] = SeatKind::Human;
        cfg.time_limit = 60 * 60 * 6;
        let (_, s) = run(
            RaceSession::new(cfg),
            &mut PadScript::new(seed, PLAY_MASK),
            1_000_000,
        );
        let r = s.results().unwrap();
        assert_eq!(r.placements.len(), 4);
    }
}

#[test]
fn prop_hunt_soak() {
    let modes = rebirth_modes();
    for seed in 0..SEEDS {
        let mut cfg = PropHuntConfig::from_arena(&modes.prop_hunt[0], seed).unwrap();
        cfg.hide_frames = 200;
        cfg.round_frames = 3000;
        if seed % 2 == 0 {
            cfg.seats[2] = SeatKind::Human;
        }
        let arena = cfg.arena.clone();
        let (_, s) = run(
            PropHuntSession::new(cfg),
            &mut PadScript::new(seed, PLAY_MASK),
            1_000_000,
        );
        for a in s.view().avatars {
            assert!(arena.open(a.pos), "seed {seed}: avatar inside a wall");
        }
    }
}
