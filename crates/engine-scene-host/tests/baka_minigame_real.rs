//! Disc-gated: drive the **real** parsed Baka Fighter roster + action tables
//! (PROT 0976) through the engine duel rules engine
//! ([`legaia_engine_minigames::baka_fighter`]).
//!
//! The table parsers are pinned by `legaia-minigame-formats`' `baka_opponents_real`;
//! this closes the engine end - the play-window load path
//! (`SceneHost::open_disc` -> `entry_bytes_extended(976)` ->
//! `static_overlay::as_loaded` -> `parse` / `parse_actions`) resolves the
//! real tables, and a `BakaFight` runs exchanges to a decided best-of-3
//! match whose coin prize lands in the casino coin bank. No Sony bytes are
//! asserted, only structural facts. Skips + passes when `LEGAIA_DISC_BIN`
//! is absent.

use legaia_engine_core::world::World;
use legaia_engine_minigames::baka_fighter::{BakaFight, HP_START, MatchPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_minigame_formats::baka_opponents;
use legaia_overlay_images::static_overlay;

fn real_tables() -> Option<(
    Vec<baka_opponents::BakaOpponent>,
    Vec<baka_opponents::BakaActionSet>,
)> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
        .expect("baka overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0976 (extended)");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let opponents = baka_opponents::parse(&loaded).expect("real roster parses");
    let actions = baka_opponents::parse_actions(&loaded).expect("real action tables parse");
    Some((opponents, actions))
}

#[test]
fn real_tables_drive_a_decided_match_and_bank_the_gold() {
    let Some((opponents, actions)) = real_tables() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // Fight ladder opponent 4 (the 100-gold record observed structurally to
    // pay; any paying opponent works - assert on the parsed value, not a
    // literal). Player plays roster 0 like the play-window B key.
    let opponent = opponents
        .iter()
        .find(|o| o.gold_reward > 0 && baka_opponents::is_valid_pattern(&o.ai_pattern))
        .expect("a paying opponent exists")
        .index;
    let prize = opponents[opponent].gold_reward;

    // Play to a decided match by throwing the three attacks in turn. The CPU
    // answers each throw blind, so a match is rock-paper-scissors on the stream:
    // take the first seed that wins.
    let mut won = None;
    for seed in 0..32 {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0xBAA5EED ^ seed)
            .expect("fight builds from real tables");
        assert_eq!(fight.gold_reward(), prize);
        world.enter_baka_fighter(fight);
        assert_eq!(world.mode, SceneMode::BakaFighter);
        let mut frames = 0;
        let mut throw = 0;
        let mut saw_player_damage = false;
        loop {
            frames += 1;
            assert!(frames < 100_000, "match terminates");
            let f = world
                .minigames
                .baka_fighter
                .as_ref()
                .expect("fight installed");
            if f.match_over() {
                break;
            }
            let pad = if f.can_choose(0) && frames % 2 == 0 {
                throw += 1;
                [
                    PadButton::Square.mask(),
                    PadButton::Circle.mask(),
                    PadButton::Cross.mask(),
                ][throw % 3]
            } else {
                0
            };
            world.set_pad(pad);
            let _ = world.tick();
            if let Some(r) = world
                .minigames
                .baka_fighter
                .as_ref()
                .and_then(|f| f.last_exchange())
                && r.winner == 0
                && !r.draw
                && r.damage > 0
            {
                saw_player_damage = true;
            }
        }
        assert!(saw_player_damage, "the match dealt real-table damage");
        if world.minigames.baka_fighter.as_ref().unwrap().winner() == Some(0) {
            won = Some(world);
            break;
        }
    }
    let mut world = won.expect("some seed wins the match");
    let coins0 = World::new().minigames.casino_coins;

    let f = world.minigames.baka_fighter.as_ref().unwrap();
    assert_eq!(f.winner(), Some(0));
    assert_eq!(f.round_wins(0), baka_opponents::ROUND_WIN_TARGET);
    assert!(f.hp(1) < HP_START || matches!(f.phase(), MatchPhase::MatchOver(0)));

    // Leave the decided match the cabinet's way: its tally runs into the
    // "NEXT GAME / PAY OUT" choice, PAY OUT (Right, then Cross) runs the exit
    // state, and the exit's fade end is the mode-24 return warp that banks
    // the parsed coin prize into the casino coin bank.
    for _ in 0..5_000 {
        let f = world
            .minigames
            .baka_fighter
            .as_ref()
            .expect("fight installed");
        if f.cabinet().choice_sheet().is_some() {
            break;
        }
        world.set_pad(0);
        let _ = world.tick();
    }
    for mask in [PadButton::Right.mask(), 0, PadButton::Cross.mask()] {
        world.set_pad(mask);
        let _ = world.tick();
    }
    for _ in 0..0x100 {
        if world.mode != SceneMode::BakaFighter {
            break;
        }
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(world.mode, SceneMode::Field, "return mode restored");
    assert!(
        world.minigames.baka_fighter.is_none(),
        "fight cleared on exit"
    );
    assert_eq!(
        world.minigames.casino_coins,
        coins0 + prize,
        "the opponent's parsed coin prize banked"
    );
}
