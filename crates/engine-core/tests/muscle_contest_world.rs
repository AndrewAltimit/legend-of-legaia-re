//! Disc-free: the **world** end of the Muscle Dome contest - that a finished
//! leg reaches the ladder, that the between-leg recovery lands on the
//! fighter's record, and that a settled run pays coins rather than a Seru.
//!
//! The rules themselves are unit-tested in `muscle_dome`, and the disc join
//! lives in `muscle_contest_real`. What this covers is the wiring the native
//! play-window drives: `World::report_muscle_leg` and
//! `World::settle_muscle_contest`, plus the reward misattribution that used
//! to sit in `World::exit_muscle_dome`.

use legaia_engine_core::muscle_dome as md;
use legaia_engine_core::world::World;

/// A two-course ladder with hand-picked rows, so the arithmetic below is
/// readable without a disc in the loop.
fn score() -> [md::ScoreRow; md::COURSE_COUNT] {
    let mut s = [[0i32; md::MAX_ROUNDS_PER_COURSE]; md::COURSE_COUNT];
    s[0][..3].copy_from_slice(&[10, 20, 40]);
    s
}

fn world_with_contest() -> World {
    let mut w = World::default();
    let flags = w.muscle_contest_flags();
    w.muscle_contest = Some(md::DomeContest::enter(&flags, [3, 3, 3], score()));
    w
}

fn cleared() -> md::LegReport {
    md::LegReport {
        survived: true,
        outcome: 0,
        turns_taken: 4,
    }
}

#[test]
fn a_reported_leg_advances_the_ladder_and_banks_its_cell() {
    let mut w = world_with_contest();
    assert_eq!(w.muscle_contest.as_ref().unwrap().round(), 0);

    let state = w.report_muscle_leg(cleared()).expect("a contest is open");
    // The hub lands on the restore state, then the next leg is stageable.
    assert!(matches!(
        state,
        md::ContestState::Restore | md::ContestState::Fight
    ));
    let run = w.muscle_contest.as_ref().unwrap();
    assert_eq!(run.round(), 1, "the ladder advanced one leg");
    assert_eq!(run.tally(), 10, "cell 0 banked");
    assert!(!run.over());
}

#[test]
fn the_between_leg_restore_lands_on_the_fighters_record() {
    let mut w = world_with_contest();
    let Some(rec) = w.roster.members.first_mut() else {
        // A default world may carry no party; the restore has nothing to do
        // and the ladder must still advance.
        assert!(w.report_muscle_leg(cleared()).is_some());
        return;
    };
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = 500;
    hms.hp_cur = 100;
    rec.set_hp_mp_sp(hms);

    w.report_muscle_leg(cleared()).expect("a contest is open");
    let after = w.roster.members[0].hp_mp_sp();
    assert!(
        after.hp_cur > 100,
        "the recovery lanes healed the fighter (was 100, now {})",
        after.hp_cur
    );
    assert!(after.hp_cur <= after.hp_max, "and are capped at max HP");
}

#[test]
fn a_finished_run_pays_coins_and_leaves_the_seru_log_alone() {
    let mut w = world_with_contest();
    let seru_rows_before = w.seru_log.iter_rows().count();
    // Three legs is the whole course.
    for _ in 0..3 {
        w.report_muscle_leg(cleared());
    }
    let out = w.settle_muscle_contest().expect("the run finished");
    assert_eq!(out.score, 70, "10 + 20 + 40, the whole row");
    assert_eq!(w.casino_coins, 70, "paid into the coin bank");
    assert!(w.muscle_contest.is_none(), "the contest closed");
    assert_eq!(w.muscle_settlement, Some(out), "kept for the host to show");
    // Continuing latches its flag.
    assert!(w.system_flag_test(md::CONTEST_CONTINUE_FLAG));
    assert!(!w.system_flag_test(md::CONTEST_GAVE_UP_FLAG));
    // And nothing captured a Seru: the victory caption names a spell, it does
    // not award one.
    assert_eq!(
        w.seru_log.iter_rows().count(),
        seru_rows_before,
        "a dome win credits no Seru capture"
    );
}

#[test]
fn running_from_the_first_fight_voids_the_run_and_latches_the_course_flag() {
    let mut w = world_with_contest();
    w.report_muscle_leg(md::LegReport {
        survived: true,
        outcome: md::LEG_OUTCOME_RAN,
        turns_taken: 1,
    });
    let out = w.settle_muscle_contest().expect("the run ended");
    assert_eq!(out.score, 0);
    assert_eq!(w.casino_coins, 0, "a give-up pays nothing");
    assert!(w.system_flag_test(md::CONTEST_GAVE_UP_FLAG));
    // Round 1 = the course's first fight, so its own flag latches - the
    // Muscle Paradise trigger's course-0 third.
    assert!(w.system_flag_test(md::COURSE_RAN_FIRST_FLAG_BASE));
}

#[test]
fn settling_needs_a_finished_run() {
    let mut w = world_with_contest();
    assert!(
        w.settle_muscle_contest().is_none(),
        "a fresh contest settles nothing"
    );
    w.report_muscle_leg(cleared());
    assert!(
        w.settle_muscle_contest().is_none(),
        "a contest mid-ladder settles nothing"
    );
    assert!(w.muscle_contest.is_some(), "and stays open");
}

#[test]
fn the_master_prize_lands_in_the_bag_once() {
    let mut w = World::default();
    let mut s = [[0i32; md::MAX_ROUNDS_PER_COURSE]; md::COURSE_COUNT];
    // A full-length Master course, so the run reaches the prize round.
    for (r, cell) in s[md::MASTER_COURSE].iter_mut().enumerate().take(13) {
        *cell = r as i32 + 1;
    }
    // Unlock the Master course + open every length gate.
    w.system_flag_set(md::COURSE_UNLOCK_FLAGS[2].0);
    for &(_, id) in &md::MASTER_LENGTH_GATES {
        w.system_flag_set(id);
    }
    let flags = w.muscle_contest_flags();
    w.muscle_contest = Some(md::DomeContest::enter(&flags, [8, 8, 13], s));
    assert_eq!(
        w.muscle_contest.as_ref().unwrap().course(),
        md::MASTER_COURSE
    );
    for _ in 0..13 {
        w.report_muscle_leg(cleared());
    }
    let out = w.settle_muscle_contest().expect("the run finished");
    assert!(out.award_prize);
    assert_eq!(
        w.inventory.get(&md::CONTEST_PRIZE_ITEM_ID).copied(),
        Some(1),
        "the War God Icon is in the bag"
    );
    assert!(
        w.system_flag_test(md::CONTEST_PRIZE_FLAG),
        "and the one-shot flag latched"
    );
}
