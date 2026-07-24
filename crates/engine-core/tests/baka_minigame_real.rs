//! Disc-gated: drive the **real** parsed Baka Fighter roster + action tables
//! (PROT 0976) through the engine duel rules engine
//! ([`legaia_engine_core::baka_fighter`]).
//!
//! The table parsers are pinned by `legaia-asset`'s `baka_opponents_real`;
//! this closes the engine end - the play-window load path
//! (`SceneHost::open_disc` -> `entry_bytes_extended(976)` ->
//! `static_overlay::as_loaded` -> `parse` / `parse_actions`) resolves the
//! real tables, and a `BakaFight` runs exchanges to a decided best-of-3
//! match whose coin prize lands in the casino coin bank. No Sony bytes are
//! asserted, only structural facts. Skips + passes when `LEGAIA_DISC_BIN`
//! is absent.

use legaia_asset::baka_opponents;
use legaia_asset::static_overlay;
use legaia_engine_core::baka_fighter::{BakaAttack, BakaFight, HP_START, MatchPhase};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

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

/// The counter to an attack under the pinned beats relation
/// (2 beats 1, 3 beats 2, 1 beats 3).
fn counter_of(t: BakaAttack) -> BakaAttack {
    match t {
        BakaAttack::A => BakaAttack::B,
        BakaAttack::B => BakaAttack::C,
        BakaAttack::C => BakaAttack::A,
        BakaAttack::Special => BakaAttack::A, // unreachable for the CPU picker
    }
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

    let mut world = World::new();
    world.mode = SceneMode::Field;
    let coins0 = world.casino_coins;
    let fight = BakaFight::from_tables(&opponents, &actions, 0, opponent, 0xBAA5EED)
        .expect("fight builds from real tables");
    assert_eq!(fight.gold_reward(), prize);
    world.enter_baka_fighter(fight);
    assert_eq!(world.mode, SceneMode::BakaFighter);

    // Play to a decided match by countering the CPU's committed attack each
    // frame (readable off the fight state - the scripted patterns are real
    // disc data). The counter strategy must eventually take 2 rounds.
    let mut frames = 0;
    let mut saw_player_damage = false;
    loop {
        frames += 1;
        assert!(frames < 100_000, "match terminates");
        let f = world.baka_fighter.as_ref().expect("fight installed");
        if f.match_over() {
            break;
        }
        let pad = match (f.chosen(1), f.can_choose(0)) {
            (Some(cpu), true) => match counter_of(cpu) {
                BakaAttack::A => PadButton::Left.mask(),
                BakaAttack::B => PadButton::Right.mask(),
                BakaAttack::C => PadButton::Up.mask(),
                BakaAttack::Special => 0,
            },
            _ => 0,
        };
        world.set_pad(pad);
        let _ = world.tick();
        if let Some(r) = world.baka_fighter.as_ref().and_then(|f| f.last_exchange())
            && r.winner == 0
            && !r.draw
            && r.damage > 0
        {
            saw_player_damage = true;
        }
    }
    assert!(saw_player_damage, "counter play dealt real-table damage");

    let f = world.baka_fighter.as_ref().unwrap();
    assert_eq!(f.winner(), Some(0), "the counter strategy wins the match");
    assert_eq!(f.round_wins(0), baka_opponents::ROUND_WIN_TARGET);
    assert!(f.hp(1) < HP_START || matches!(f.phase(), MatchPhase::MatchOver(0)));

    // Cross leaves the decided match through the world tick path, banking the
    // parsed coin prize into the casino coin bank via the mode-24 return warp
    // (play-window's B key exit goes through the same `exit_baka_fighter`).
    world.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field, "return mode restored");
    assert!(world.baka_fighter.is_none(), "fight cleared on exit");
    assert_eq!(
        world.casino_coins,
        coins0 + prize,
        "the opponent's parsed coin prize banked"
    );
}
