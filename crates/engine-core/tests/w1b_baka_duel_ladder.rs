//! Baka Fighter ladder: a duel played **through its intro card to the
//! end-of-match tally**, by pad, through `World::tick`.
//!
//! ## Why this instrument exists
//!
//! `minigame_replay`'s baka rung stops at the first resolved exchange and
//! breaks the moment `match_over()` is true, so three things retail does after
//! that moment had no run behind them: the per-round score rows
//! (`FUN_801D2A28`), the tally screen that drains them (`FUN_801D239C`) and
//! the drain-rate kernel it steps with (`FUN_801D6710`, linked twice - the
//! arena's copy is `FUN_801D14B0`). The intro title card (`FUN_801D59D4`) had
//! no caller at all: the port armed it from a bare constructor no host used.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | a cabinet entered at boot animates the title card | the card runs off the cabinet's own clock, not a private timeline |
//! | 2 | rounds resolve and accumulate the two score rows | the disc's bonus tables reach `baka_round_score` |
//! | 3 | a player win installs the tally and it drains over frames | the prize arrives at the retail rate, not in one lump |
//! | 4 | the drain rate takes every band, boosted and not | `step_scale`'s four arms are all executed, not just the common one |
//!
//! Rung 2 is disc-gated (the bonus tables are overlay rodata); the rest is
//! disc-free, so a run without `LEGAIA_DISC_BIN` still scores 3 of 4.
//!
//! ## What this ladder does not claim
//!
//! Nothing here reaches the duel through the **door**. `minigame_replay` owns
//! that, and its entry (`scene/host/minigame_warp.rs`) neither hands the
//! score tables over nor enters the cabinet at boot - so on that path rungs 1
//! and 2 are still unreached in production. Both are one call each and both
//! are named in this module's report rather than papered over here.

use legaia_engine_core::baka_fighter::{
    BakaFight, BakaScoreTables, FighterConfig, HP_START, MatchPhase, TALLY_COUNTERS,
    TALLY_FADE_GATE, TALLY_GOLD_COUNTER, baka_round_score, tally_drain_sequence, tally_drain_step,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

/// PROT 0976 - the Baka Fighter overlay, which carries both bonus tables.
const BAKA_OVERLAY_PROT_INDEX: u32 = 976;

const PRIZE: u32 = 250;

fn overlay_0976() -> Option<Vec<u8>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    host.index
        .entry_bytes_extended(BAKA_OVERLAY_PROT_INDEX)
        .ok()
}

/// A fighter whose landed special ends a round outright.
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

fn fight() -> BakaFight {
    BakaFight::new(cfg(0, 4000, 0), cfg(1, 0, PRIZE), [0, 0], 0xBAA5EED)
}

fn world_with(fight: BakaFight) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.enter_baka_fighter(fight);
    w
}

fn step(w: &mut World, mask: u16) {
    w.input.set_pad(mask);
    let _ = w.tick();
}

fn press(w: &mut World, mask: u16) {
    step(w, mask);
    step(w, 0);
}

/// Throw the special until the match is decided. Down is the special in the
/// world's pad mapping, and a landed special is an unbeatable exchange.
fn play_to_player_win(w: &mut World) {
    for _ in 0..100_000 {
        if w.baka_fighter.as_ref().is_none_or(|f| f.match_over()) {
            return;
        }
        press(w, PadButton::Down.mask());
    }
    panic!("the duel never resolved");
}

// ---------------------------------------------------------------------------
// Rung 1 - the intro title card
// ---------------------------------------------------------------------------

/// A cabinet entered at **boot** walks its attract arms, and the intro title
/// card animates off the cabinet's own clock while it does. The card's three
/// segments are independent range tests on that counter, so all three have to
/// appear over one run of it.
#[test]
fn rung1_a_cabinet_entered_at_boot_animates_the_intro_title_card() {
    let mut w = world_with(fight().with_attract());

    // The logo widget, the subtitle widget and the sweep bar - one from each
    // of the card's three segments.
    let (mut logo, mut subtitle, mut sweep) = (false, false, false);
    let mut announcer = 0usize;
    for _ in 0..600 {
        step(&mut w, 0);
        let Some(f) = w.baka_fighter.as_ref() else {
            break;
        };
        let frame = f.chrome_frame();
        for d in &frame.draws {
            match d.widget {
                0x28 => logo = true,
                0x22 => subtitle = true,
                0x32 => sweep = true,
                _ => {}
            }
        }
        if frame.xa.is_some() {
            announcer += 1;
        }
    }
    assert!(logo, "the logo segment never drew");
    assert!(subtitle, "the subtitle segment never drew");
    assert!(sweep, "the assembled card's sweep bar never drew");
    assert_eq!(
        announcer, 2,
        "each announcer line fires exactly once (the latch is `DAT_801DBE8C`)"
    );
}

/// The card is the **cabinet's**, so a fight entered mid-duel - which is what
/// every shipped host does today - must not run it. Asserting the negative is
/// what keeps rung 1 from being a tautology about a flag this file sets.
#[test]
fn a_duel_entered_mid_cabinet_runs_no_title_card() {
    let mut w = world_with(fight());
    for _ in 0..200 {
        step(&mut w, 0);
        let Some(f) = w.baka_fighter.as_ref() else {
            break;
        };
        assert!(
            !f.chrome_frame().draws.iter().any(|d| d.widget == 0x28),
            "the duel host animated the attract card"
        );
    }
}

// ---------------------------------------------------------------------------
// Rung 2 - the per-round score rows
// ---------------------------------------------------------------------------

/// With the disc's two bonus tables installed, a finished round folds a combo
/// row and a health row into the score the tally later drains. Without them
/// the port has no score channel at all, which is the state every disc-free
/// oracle measures - so this rung is what tells the two apart.
#[test]
fn rung2_the_disc_bonus_tables_reach_the_per_round_score_rows() {
    let Some(overlay) = overlay_0976() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let tables = BakaScoreTables::from_overlay(&overlay).expect("both bonus tables decode");
    assert_eq!(
        tables.combo_bonus.len(),
        20,
        "the combo table's index space"
    );
    assert!(
        tables.combo_bonus.windows(2).all(|w| w[0] <= w[1]),
        "a bonus table that is not monotonic is not this table"
    );
    assert!(
        tables.health_bonus.windows(2).all(|w| w[0] <= w[1]),
        "same for the health rows"
    );

    // The kernel's own arms, against the disc rows: a full-HP round takes the
    // perfect bonus rather than a table row, and a clamped combo takes the
    // last row rather than running off the end.
    let perfect = baka_round_score(3, &tables.combo_bonus, HP_START, &tables.health_bonus);
    assert_eq!(perfect.combo_gain, tables.combo_bonus[3]);
    assert_eq!(
        perfect.bonus_gain,
        legaia_engine_core::baka_fighter::BAKA_PERFECT_BONUS
    );
    let clamped = baka_round_score(999, &tables.combo_bonus, 0, &tables.health_bonus);
    assert_eq!(
        clamped.combo_gain,
        *tables.combo_bonus.last().unwrap(),
        "a runaway combo pins to the last row"
    );

    // And through a live duel: the rows only move when the tables are there.
    let mut with_tables = world_with(fight().with_score_tables(tables));
    play_to_player_win(&mut with_tables);
    let scored = with_tables.baka_fighter.as_ref().unwrap().score_rows();

    let mut without = world_with(fight());
    play_to_player_win(&mut without);
    let unscored = without.baka_fighter.as_ref().unwrap().score_rows();

    assert_eq!(unscored, [0, 0, 0], "no tables, no score channel");
    assert!(
        scored.iter().any(|&r| r > 0),
        "the disc tables produced no score rows: {scored:?}"
    );
}

// ---------------------------------------------------------------------------
// Rung 3 - the tally drains over frames
// ---------------------------------------------------------------------------

/// A player win installs the four-counter tally, and it drains a step per
/// frame with each row gated behind its own fade. The prize reaches the
/// player's purse across many frames, never in one.
#[test]
fn rung3_a_won_match_drains_its_tally_over_frames() {
    let mut w = world_with(fight());
    play_to_player_win(&mut w);

    let f = w.baka_fighter.as_ref().expect("the fight is live");
    assert_eq!(f.winner(), Some(0), "the player took the match");
    assert!(matches!(f.phase(), MatchPhase::MatchOver(0)));
    let tally = f.tally().expect("a won match installs the tally");
    assert_eq!(
        tally.counters().len(),
        TALLY_COUNTERS,
        "four counters, the last of which is the prize"
    );
    assert_eq!(
        tally.counters()[TALLY_GOLD_COUNTER],
        PRIZE as i32,
        "the prize row carries the opponent's reward"
    );

    // The prize arrives *over frames*. Two independent readings of that: the
    // first coin does not land on the frame the tally opened (its row has a
    // fade to walk first), and the payout takes many separate increments.
    let mut frames_to_first_coin = 0usize;
    let mut increments = 0usize;
    let mut paid = 0i32;
    for i in 0..4_000 {
        step(&mut w, 0);
        let Some(f) = w.baka_fighter.as_ref() else {
            break;
        };
        let Some(t) = f.tally() else { break };
        if t.gold_drained() > paid {
            increments += 1;
            if frames_to_first_coin == 0 {
                frames_to_first_coin = i + 1;
            }
        }
        paid = t.gold_drained();
        if t.done() {
            break;
        }
    }
    assert!(
        frames_to_first_coin > 1,
        "the first coin landed on frame {frames_to_first_coin} - the row's fade never ran"
    );
    assert!(
        increments > 1,
        "the prize arrived in {increments} step(s), i.e. as a lump"
    );
    // The gate is the counter each row walks before it drains, so the first
    // coin cannot land before the tally has been up for that many of the
    // fight's own frame steps.
    assert!(
        frames_to_first_coin >= TALLY_FADE_GATE.max(1) as usize / 2,
        "the first coin landed on frame {frames_to_first_coin}, ahead of the \
         {TALLY_FADE_GATE}-step fade gate at the actor cadence"
    );
    assert_eq!(paid, PRIZE as i32, "the whole prize drained");
}

// ---------------------------------------------------------------------------
// Rung 4 - every drain band
// ---------------------------------------------------------------------------

/// The drain rate is proportional, and its four arms are what make the tally
/// slow down as it empties: a remainder above `5` moves a fifth, `3..=5` a
/// half, below `3` exactly one, and the fast-forward latch moves the lot.
///
/// A tally big enough to start in the top band walks all three unboosted arms
/// on its own, which is the property asserted - not a table of hand-picked
/// inputs.
#[test]
fn rung4_the_drain_rate_takes_every_band() {
    let steps = tally_drain_sequence(1_000);
    assert!(
        steps.iter().sum::<i32>() == 1_000,
        "the drain is lossless: the steps sum to the counter"
    );
    assert_eq!(
        steps.last(),
        Some(&1),
        "the last step is one, so the counter reaches zero exactly"
    );
    assert!(
        steps.len() > 20,
        "a proportional drain takes many frames, not a handful: {}",
        steps.len()
    );
    // Each band, named by the arm it exercises. The rate is **not** monotonic
    // across the band edge - `6 / 5 = 1` and `5 / 2 = 2` - and that is retail's
    // own arithmetic (truncating divides on either side of a `>= 6` test),
    // not a rounding choice of the port's.
    assert_eq!(tally_drain_step(1_000, false), 200, "the /5 band");
    assert_eq!(tally_drain_step(6, false), 1, "the /5 band's own floor");
    assert_eq!(tally_drain_step(5, false), 2, "the /2 band starts higher");
    assert_eq!(tally_drain_step(4, false), 2, "the /2 band");
    assert_eq!(tally_drain_step(2, false), 1, "the unit band");
    assert_eq!(
        tally_drain_step(1_000, true),
        1_000,
        "the fast-forward latch passes the whole remainder"
    );

    // And the latch through a live session: a face button while the tally is
    // up snaps it, so the prize is fully paid in far fewer frames.
    let mut slow = world_with(fight());
    play_to_player_win(&mut slow);
    let mut slow_frames = 0usize;
    for i in 0..8_000 {
        step(&mut slow, 0);
        slow_frames = i + 1;
        if slow
            .baka_fighter
            .as_ref()
            .and_then(|f| f.tally())
            .is_some_and(|t| t.done())
        {
            break;
        }
    }

    let mut fast = world_with(fight());
    play_to_player_win(&mut fast);
    let mut fast_frames = 0usize;
    for i in 0..8_000 {
        press(&mut fast, PadButton::Triangle.mask());
        fast_frames = i + 1;
        if fast
            .baka_fighter
            .as_ref()
            .and_then(|f| f.tally())
            .is_some_and(|t| t.done())
        {
            break;
        }
    }
    assert!(
        fast_frames < slow_frames,
        "the fast-forward latch did not shorten the tally ({fast_frames} vs {slow_frames})"
    );
}
