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
    w.minigames.muscle_contest = Some(md::DomeContest::enter(&flags, [3, 3, 3], score()));
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
    assert_eq!(w.minigames.muscle_contest.as_ref().unwrap().round(), 0);

    let state = w.report_muscle_leg(cleared()).expect("a contest is open");
    // The hub lands on the restore state, then the next leg is stageable.
    assert!(matches!(
        state,
        md::ContestState::Restore | md::ContestState::Fight
    ));
    let run = w.minigames.muscle_contest.as_ref().unwrap();
    assert_eq!(run.round(), 1, "the ladder advanced one leg");
    assert_eq!(run.tally(), 10, "cell 0 banked");
    assert!(!run.over());
}

#[test]
fn the_between_leg_restore_lands_on_the_fighters_record() {
    let mut w = world_with_contest();
    let Some(rec) = w.party.roster.members.first_mut() else {
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
    let after = w.party.roster.members[0].hp_mp_sp();
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
    let seru_rows_before = w.seru.log.iter_rows().count();
    // Three legs is the whole course.
    for _ in 0..3 {
        w.report_muscle_leg(cleared());
    }
    let out = w.settle_muscle_contest().expect("the run finished");
    assert_eq!(out.score, 70, "10 + 20 + 40, the whole row");
    assert_eq!(w.minigames.casino_coins, 70, "paid into the coin bank");
    assert!(w.minigames.muscle_contest.is_none(), "the contest closed");
    assert_eq!(
        w.minigames.muscle_settlement,
        Some(out),
        "kept for the host to show"
    );
    // Continuing latches its flag.
    assert!(w.system_flag_test(md::CONTEST_CONTINUE_FLAG));
    assert!(!w.system_flag_test(md::CONTEST_GAVE_UP_FLAG));
    // And nothing captured a Seru: the victory caption names a spell, it does
    // not award one.
    assert_eq!(
        w.seru.log.iter_rows().count(),
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
    assert_eq!(w.minigames.casino_coins, 0, "a give-up pays nothing");
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
    assert!(w.minigames.muscle_contest.is_some(), "and stays open");
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
    w.minigames.muscle_contest = Some(md::DomeContest::enter(&flags, [8, 8, 13], s));
    assert_eq!(
        w.minigames.muscle_contest.as_ref().unwrap().course(),
        md::MASTER_COURSE
    );
    for _ in 0..13 {
        w.report_muscle_leg(cleared());
    }
    let out = w.settle_muscle_contest().expect("the run finished");
    assert!(out.award_prize);
    assert_eq!(
        w.party.inventory.get(&md::CONTEST_PRIZE_ITEM_ID).copied(),
        Some(1),
        "the War God Icon is in the bag"
    );
    assert!(
        w.system_flag_test(md::CONTEST_PRIZE_FLAG),
        "and the one-shot flag latched"
    );
}

/// The ringside still is picked off the **lead record** after the leg, the
/// way the back-read loader `FUN_801F6B24` reads it: the battle end writes
/// the fighter's HP into `+0x106`, and the pick is `+0x106 < +0x11C / 2`.
#[test]
fn the_ringside_pick_reads_the_lead_record_after_the_leg() {
    let mut w = World::default();
    w.load_party(legaia_save::Party::zeroed(1));
    let mut party = w.party.roster.clone();
    let rec = &mut party.members[0];
    let mut hms = rec.hp_mp_sp();
    hms.hp_cur = 300;
    hms.hp_max = 300;
    rec.set_hp_mp_sp(hms);
    // The base maximum `+0x11C` the pick halves.
    let mut stats = rec.record_stats();
    stats.hp_max = 300;
    rec.set_record_stats(stats);
    w.load_party(party);
    let card = md::MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    let fought_to = |w: &mut World, hp: i32| {
        w.enter_muscle_dome(md::MuscleDomeSession::new(
            [card; md::HAND_SLOTS],
            [card; md::HAND_SLOTS],
            [120, 120],
            [hp, 400],
            1,
        ));
        w.exit_muscle_dome();
        (
            w.minigames.muscle_ringside_still,
            w.party.roster.members[0].hp_mp_sp().hp_cur,
        )
    };
    assert_eq!(
        fought_to(&mut w, 100),
        (Some(legaia_asset::ringside_still::PROT_INDEX_LOW_HP), 100),
        "below half: the second still, and the HP lands on the record"
    );
    assert_eq!(
        fought_to(&mut w, 200),
        (Some(legaia_asset::ringside_still::PROT_INDEX_DEFAULT), 200)
    );
}

/// Leaving the arena mid-leg through the escape both hosts share (`Start`,
/// `World::poll_minigame_escape`) is the run / give-up path: the leg is
/// reported as ran and the contest ends with nothing paid - the same thing
/// the native `M` hotkey did alone before the escape reported the leg.
#[test]
fn the_shared_escape_reports_a_left_leg_and_ends_the_contest() {
    use legaia_engine_core::input::PadButton;
    let mut w = world_with_contest();
    let card = md::MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    w.enter_muscle_dome(md::MuscleDomeSession::new(
        [card; md::HAND_SLOTS],
        [card; md::HAND_SLOTS],
        [120, 120],
        [400, 400],
        1,
    ));
    assert!(w.minigames.muscle_contest.is_some());
    w.set_pad(0);
    let _ = w.tick();
    w.set_pad(PadButton::Start.mask());
    let _ = w.tick();
    assert!(w.minigames.muscle_dome.is_none(), "Start left the arena");
    assert!(
        w.minigames.muscle_contest.is_none(),
        "the left leg ended the contest and it settled"
    );
    assert!(w.system_flag_test(md::CONTEST_GAVE_UP_FLAG));
    assert_eq!(w.minigames.casino_coins, 0, "a give-up pays nothing");
}

/// A won leg with the course not exhausted keeps the frame in the arena:
/// retail re-enters the hub (state `0x0A`), plays the INTERVAL tally and the
/// ROUND card and starts the next fight itself. The leg used to hand the
/// field back on its confirm, so the next round never opened.
#[test]
fn a_won_leg_mid_ladder_stays_in_the_arena_and_stages_the_next_fight() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::minigame_entry::MinigameSubId;
    use legaia_engine_core::world::SceneMode;
    let mut w = world_with_contest();
    w.mode = SceneMode::Field;
    let card = md::MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    let won_leg = |w: &mut World| {
        let mut s = md::MuscleDomeSession::new(
            [card; md::HAND_SLOTS],
            [card; md::HAND_SLOTS],
            [120, 120],
            [400, 1],
            1,
        );
        assert!(s.commit_card(0, 0));
        s.end_selection();
        s.resolve_turn(|attacker, _| if attacker == 0 { 5 } else { 0 });
        assert_eq!(s.phase(), md::MusclePhase::Won);
        w.enter_muscle_dome(s);
    };
    won_leg(&mut w);
    // No text over the KO.
    assert!(legaia_engine_core::minigame_status::muscle_status_rows(&w).is_empty());
    w.set_pad(0);
    let _ = w.tick();
    w.set_pad(PadButton::Cross.mask());
    let _ = w.tick();
    assert!(w.minigames.muscle_dome.is_none(), "the leg closed");
    assert_eq!(w.mode, SceneMode::MuscleDome, "the arena keeps the frame");
    assert!(w.muscle_hub_between_legs());
    assert_eq!(w.minigames.muscle_contest.as_ref().unwrap().round(), 1);
    // Ticks with no host hub hold the arena; nothing leaks back to the field.
    for _ in 0..30 {
        w.set_pad(0);
        let _ = w.tick();
    }
    assert_eq!(w.mode, SceneMode::MuscleDome);
    assert_eq!(w.minigames.pending_warp, None);
    // The hub's hand-off stages the next fight through the mode-24 drain.
    w.begin_next_muscle_leg();
    assert_eq!(
        w.minigames.pending_warp,
        Some(MinigameSubId::MuscleDome.sub_id())
    );
    assert_eq!(w.mode, SceneMode::MuscleDome);
    won_leg(&mut w);
    assert!(!w.muscle_hub_between_legs(), "the new leg owns the arena");
}

/// The hub kernel both play hosts drive plays the INTERVAL screen and the
/// re-entered hub's ROUND card over the still, and only then asks for the
/// next fight - once.
#[test]
fn the_hub_timers_hand_the_next_leg_off_after_the_round_card() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::muscle_ringside::HubTimers;
    use legaia_engine_core::world::SceneMode;
    let mut w = world_with_contest();
    w.mode = SceneMode::Field;
    let card = md::MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    let mut s = md::MuscleDomeSession::new(
        [card; md::HAND_SLOTS],
        [card; md::HAND_SLOTS],
        [120, 120],
        [400, 1],
        1,
    );
    assert!(s.commit_card(0, 0));
    s.end_selection();
    s.resolve_turn(|attacker, _| if attacker == 0 { 5 } else { 0 });
    w.enter_muscle_dome(s);
    let mut timers = HubTimers::default();
    let mut saw_interval = false;
    let mut saw_card = false;
    let mut handed_off = 0;
    for frame in 0..4000 {
        w.set_pad(if frame == 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = w.tick();
        let out = timers.tick(&w, 0, 0);
        saw_interval |= timers.interval.is_some();
        saw_card |= timers
            .backdrop
            .is_some_and(|b| b.card_brightness().is_some());
        if out.next_leg {
            assert!(saw_interval && saw_card, "the hub played out first");
            handed_off += 1;
            w.begin_next_muscle_leg();
        }
    }
    assert_eq!(w.mode, SceneMode::MuscleDome);
    assert!(handed_off >= 1, "the hub handed the next fight off");
    assert!(w.minigames.pending_warp.is_some());
}

/// The last leg of a course settles and hands the field back; Start between
/// legs gives the contest up.
#[test]
fn a_course_ending_leg_and_a_between_legs_escape_hand_the_field_back() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::world::SceneMode;
    let card = md::MuscleCard {
        command_id: 0x0C,
        cost: 0x1E,
    };
    let decide = |w: &mut World, won: bool| {
        let mut s = md::MuscleDomeSession::new(
            [card; md::HAND_SLOTS],
            [card; md::HAND_SLOTS],
            [120, 120],
            [400, 400],
            1,
        );
        assert!(s.commit_card(0, 0));
        assert!(s.commit_card(1, 0));
        s.end_selection();
        s.resolve_turn(|attacker, _| if (attacker == 0) == won { 999 } else { 0 });
        w.enter_muscle_dome(s);
        w.set_pad(0);
        let _ = w.tick();
        w.set_pad(PadButton::Cross.mask());
        let _ = w.tick();
    };
    // A lost leg settles at once.
    let mut w = world_with_contest();
    w.mode = SceneMode::Field;
    decide(&mut w, false);
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.minigames.muscle_contest.is_none(), "settled");
    // Three won legs run a three-round course out: hub, hub, then the field.
    let mut w = world_with_contest();
    w.mode = SceneMode::Field;
    decide(&mut w, true);
    assert!(w.muscle_hub_between_legs());
    decide(&mut w, true);
    assert!(w.muscle_hub_between_legs());
    decide(&mut w, true);
    assert_eq!(w.mode, SceneMode::Field, "the exhausted course settles");
    assert!(w.minigames.muscle_contest.is_none());
    // Start between legs is the give-up arm.
    let mut w = world_with_contest();
    w.mode = SceneMode::Field;
    decide(&mut w, true);
    assert!(w.muscle_hub_between_legs());
    w.set_pad(0);
    let _ = w.tick();
    w.set_pad(PadButton::Start.mask());
    let _ = w.tick();
    assert_eq!(w.mode, SceneMode::Field);
    assert!(w.minigames.muscle_contest.is_none());
    assert!(w.system_flag_test(md::CONTEST_GAVE_UP_FLAG));
}
