//! Disc-free: the Muscle Dome's **intermission cadence** - the INTERVAL +
//! score-tally hub screen is a between-FIGHTS beat, never a between-turns one.
//!
//! Retail splits this across two state machines. A turn ends inside the battle:
//! the battle-action SM writes `ctx[6] = 0x14` and bumps the turn counter
//! `ctx+0x28a` (`FUN_801E295C`, `0x801E67E8..0x801E6810`), and the round driver
//! re-enters its own command cluster - the arena hub is not running, so no hub
//! screen exists to raise. A *leg* ends when the `0x5A` end-of-action scan
//! raises the battle-end signal (`0x801E65D8` party wipe / `0x801E6674` monster
//! wipe), the exit selector routes back to arena mode `0x18`, and only then
//! does `FUN_801CF870`'s hub pick between the tally screen `0x0A` and
//! settlement `0x32`.
//!
//! This locks the rule both hosts read
//! (`md::leg_boundary_raises_interval`), because a host that re-derives it can
//! get it wrong the way the browser dome page did - raising the screen on every
//! `MusclePhase::TurnOver`, i.e. one intermission per turn instead of per
//! fight.

use legaia_engine_core::muscle_dome as md;
use legaia_engine_core::muscle_dome::{
    ContestState, HAND_SLOTS, MuscleCard, MuscleDomeSession, MusclePhase,
};

/// Four identical direction slots at 30 AP each - the retail deal shape.
fn hand() -> [MuscleCard; HAND_SLOTS] {
    let mut h = [MuscleCard {
        command_id: 0x0C,
        cost: 30,
    }; HAND_SLOTS];
    for (i, c) in h.iter_mut().enumerate() {
        c.command_id = 0x0C + i as u8;
    }
    h
}

/// A leg that takes many turns: 900 HP against 30 damage a swing.
fn long_leg() -> MuscleDomeSession {
    MuscleDomeSession::new(hand(), hand(), [30, 30], [900, 900], 0)
}

/// A three-round ladder with readable cells.
fn contest() -> md::DomeContest {
    let mut score = [[0i32; md::MAX_ROUNDS_PER_COURSE]; md::COURSE_COUNT];
    score[0][..3].copy_from_slice(&[10, 20, 40]);
    md::DomeContest::enter(&md::ContestFlags::default(), [3, 3, 3], score)
}

/// Drain the hub's `0x0A`..`0x0C` sequence the way both hosts do inside their
/// leg report, and hand back the state the host can still observe.
fn report(run: &mut md::DomeContest, survived: bool, turns: u32) -> Option<ContestState> {
    let flags = md::ContestFlags::default();
    run.finish_leg(
        md::LegReport {
            survived,
            outcome: 0,
            turns_taken: turns,
        },
        500,
        &flags,
    );
    while matches!(
        run.state(),
        ContestState::LegScore | ContestState::Tally | ContestState::Restore
    ) {
        run.advance();
    }
    Some(run.state())
}

/// Play one turn out: both sides commit one direction, resolve at `dmg`.
fn play_turn(s: &mut MuscleDomeSession, dmg: i32) {
    s.commit_card(0, 0);
    s.ai_commit_all(1);
    s.end_selection();
    s.resolve_turn(|_, _| dmg);
}

#[test]
fn turns_inside_one_fight_raise_no_intermission() {
    let mut s = long_leg();
    let run = contest();
    let round_before = run.round();

    let mut turn_boundaries = 0;
    let mut leg_boundaries = 0;
    for _ in 0..8 {
        play_turn(&mut s, 30);
        let phase = s.phase();
        if phase.ends_leg() {
            leg_boundaries += 1;
            break;
        }
        assert!(
            phase.ends_turn(),
            "a resolved non-terminal turn parks at TurnOver, got {phase:?}"
        );
        // The turn boundary is not a leg boundary, so nothing here ever
        // reaches the arena hub - the ladder does not move and no leg is
        // reported.
        assert!(!phase.ends_leg(), "a turn boundary is not a leg boundary");
        assert_eq!(
            run.round(),
            round_before,
            "the ladder must not advance on a turn"
        );
        turn_boundaries += 1;
        s.next_turn();
    }

    assert!(
        turn_boundaries >= 4,
        "the fixture must actually run several turns, ran {turn_boundaries}"
    );
    assert_eq!(
        leg_boundaries, 0,
        "8 turns at 30 damage cannot end a 900 HP leg"
    );
    // The whole point: not one of those turn boundaries raised the screen,
    // because none of them is a leg boundary at all.
    assert_eq!(run.state(), ContestState::Fight);
    assert_eq!(run.tally(), 0, "a turn banks nothing");
}

#[test]
fn a_cleared_fight_raises_exactly_one_intermission() {
    let mut run = contest();

    // Leg 1 cleared, course not exhausted -> hub state 0x0A ran.
    let after = report(&mut run, true, 4);
    assert!(
        md::leg_boundary_raises_interval(after),
        "a survived leg with the course unexhausted shows the tally screen"
    );
    assert_eq!(run.round(), 1, "the ladder advanced exactly one leg");
    assert_eq!(run.tally(), 10, "the leg's own score cell banked");

    // Leg 2 cleared: one more, and only one more.
    let after = report(&mut run, true, 2);
    assert!(md::leg_boundary_raises_interval(after));
    assert_eq!(run.round(), 2);
}

#[test]
fn a_lost_or_final_fight_settles_instead_of_showing_the_screen() {
    // Lost leg: retail routes to 0x32, not 0x0A.
    let mut run = contest();
    let after = report(&mut run, false, 3);
    assert!(
        !md::leg_boundary_raises_interval(after),
        "a lost leg settles; it does not show the tally screen"
    );
    assert!(run.over());

    // Final leg of the course: also settlement, not the tally screen.
    let mut run = contest();
    for _ in 0..2 {
        assert!(md::leg_boundary_raises_interval(report(&mut run, true, 1)));
    }
    let after = report(&mut run, true, 1);
    assert!(
        !md::leg_boundary_raises_interval(after),
        "the course-exhausting leg settles"
    );
    assert!(run.over());

    // And a run-from leg, the third settlement path.
    let mut run = contest();
    let flags = md::ContestFlags::default();
    run.finish_leg(
        md::LegReport {
            survived: true,
            outcome: md::LEG_OUTCOME_RAN,
            turns_taken: 1,
        },
        500,
        &flags,
    );
    assert!(!md::leg_boundary_raises_interval(Some(run.state())));
    assert!(run.gave_up());
}

#[test]
fn no_contest_and_a_settled_contest_show_nothing() {
    assert!(!md::leg_boundary_raises_interval(None));
    assert!(!md::leg_boundary_raises_interval(Some(
        ContestState::Settle
    )));
    assert!(!md::leg_boundary_raises_interval(Some(
        ContestState::Settled
    )));
}

#[test]
fn the_phase_predicates_split_turn_from_leg() {
    assert!(MusclePhase::TurnOver.ends_turn());
    assert!(!MusclePhase::TurnOver.ends_leg());
    for p in [MusclePhase::Won, MusclePhase::Lost] {
        assert!(p.ends_leg());
        assert!(!p.ends_turn(), "{p:?} ends the leg, not merely a turn");
    }
    for p in [MusclePhase::Select, MusclePhase::Resolve] {
        assert!(!p.ends_turn());
        assert!(!p.ends_leg());
    }
}
