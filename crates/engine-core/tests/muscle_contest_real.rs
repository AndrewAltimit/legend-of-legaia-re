//! Disc-gated: run a whole Muscle Dome **contest** off the real arena
//! roster/init overlay (PROT 0977) and check what it pays.
//!
//! The single leg was already covered (`muscle_dome_minigame_real`); what
//! this closes is the layer above it - the ladder run that decides which
//! `(course, round)` is staged, banks a cleared leg's score cell, hands the
//! recovery lanes back as HP, and settles into casino coins. All of it is
//! disc data: the course lengths and the score rows both come off PROT 0977
//! and nothing here is a constant this test invented.
//!
//! It also cross-validates the curated `casino.toml` course rewards against
//! the disc, which is the join that corrected the Master course's figure: a
//! course's `reward_coins` **is** its score row summed, and the walkthrough's
//! Master number did not match.
//!
//! No Sony bytes are asserted, only structural facts and arithmetic. Skips +
//! passes when `LEGAIA_DISC_BIN` is absent.

use legaia_engine_core::muscle_dome as md;
use legaia_engine_core::scene::SceneHost;

/// Every gate open, so a course runs its declared length.
fn open_flags() -> md::ContestFlags {
    md::ContestFlags {
        course_unlock: [false; 3],
        master_gates: [true; 3],
        prize_awarded: false,
    }
}

fn arena_overlay() -> Option<Vec<u8>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    match host
        .index
        .entry_bytes_extended(md::ARENA_OVERLAY_PROT_INDEX as u32)
    {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("[skip] PROT 0977 read failed: {e:#}");
            None
        }
    }
}

/// Clear `course` end to end, stepping the between-leg hub as the arena
/// does, and return the settled payout.
fn run_course(raw: &[u8], course: usize, flags: &md::ContestFlags) -> md::ContestSettlement {
    let mut unlock = *flags;
    // The unlock seed is what picks the course, so ask for exactly this one.
    unlock.course_unlock = [false; 3];
    for slot in unlock.course_unlock.iter_mut().take(course + 1) {
        *slot = true;
    }
    let mut c = md::DomeContest::from_overlay(raw, &unlock).expect("PROT 0977 decodes");
    assert_eq!(c.course(), course, "the unlock seed staged this course");
    assert_eq!(c.round(), 0, "a fresh contest opens on its first leg");
    for _ in 0..md::MAX_ROUNDS_PER_COURSE {
        c.finish_leg(
            md::LegReport {
                survived: true,
                outcome: 0,
                turns_taken: 3,
            },
            500,
            &unlock,
        );
        if c.over() {
            break;
        }
        // The hub's three between-leg states: rows in, rows drained, HP back.
        while !matches!(c.state(), md::ContestState::Fight) {
            c.advance();
        }
    }
    assert!(c.over(), "a run-out course settles");
    assert!(c.continue_latch(), "cleared and still standing");
    c.settle(&unlock)
}

#[test]
fn a_cleared_course_pays_its_whole_score_row_into_coins() {
    let Some(raw) = arena_overlay() else { return };
    let ladder = md::parse_course_ladder(&raw).expect("course ladder decodes");
    let score = md::parse_score_table(&raw).expect("score table decodes");
    let flags = open_flags();

    // The disc declares three courses; their lengths are the ladder's own.
    assert_eq!(ladder.len(), md::COURSE_COUNT);
    let mut coins = 0u32;
    for (course, decl) in ladder.iter().enumerate() {
        let len = decl.rounds.len();
        assert!(len > 0 && len <= md::MAX_ROUNDS_PER_COURSE);
        let out = run_course(&raw, course, &flags);
        // Every cell of the row banked exactly once: the cleared legs through
        // the tally screen, the final one at settlement.
        let row_sum: i32 = score[course][..len].iter().sum();
        assert_eq!(
            out.score, row_sum,
            "course {course} pays its score row summed"
        );
        assert!(out.set_continue_flag);
        assert!(!out.set_gave_up_flag);
        // And it lands in the coin bank.
        let before = coins;
        coins = md::credit_casino_coins(coins, out.score);
        assert_eq!(coins as i32 - before as i32, row_sum);
    }
    assert!(coins > 0, "the run banked something");
}

#[test]
fn the_curated_course_rewards_are_the_disc_score_rows() {
    let Some(raw) = arena_overlay() else { return };
    let ladder = md::parse_course_ladder(&raw).expect("course ladder decodes");
    let score = md::parse_score_table(&raw).expect("score table decodes");
    let db = legaia_gamedata::Database::load();
    let curated = db.muscle_dome();
    assert_eq!(
        curated.len(),
        md::COURSE_COUNT,
        "curated table has one row per arena course"
    );
    for (course, row) in curated.iter().enumerate() {
        let len = ladder[course].rounds.len();
        let row_sum: i32 = score[course][..len].iter().sum();
        assert_eq!(
            row.reward_coins as i32, row_sum,
            "curated `{}` reward is the disc score row summed",
            row.name
        );
        assert_eq!(
            row.enemies.len(),
            len,
            "curated `{}` line-up is as long as the disc course",
            row.name
        );
        // The fee has no writer anywhere in the arena overlay, so the curated
        // table must not claim it is confirmed.
        assert!(
            !row.entry_fee_verified,
            "`{}` entry fee is not disc-confirmed",
            row.name
        );
    }
}

#[test]
fn losing_a_course_halves_the_tally_and_running_voids_it() {
    let Some(raw) = arena_overlay() else { return };
    let score = md::parse_score_table(&raw).expect("score table decodes");
    let flags = open_flags();

    // Clear two legs of course 0, then lose the third.
    let mut c = md::DomeContest::from_overlay(&raw, &flags).expect("decodes");
    for leg in 0..3 {
        c.finish_leg(
            md::LegReport {
                survived: leg < 2,
                outcome: 0,
                turns_taken: 2,
            },
            500,
            &flags,
        );
        if c.over() {
            break;
        }
        while !matches!(c.state(), md::ContestState::Fight) {
            c.advance();
        }
    }
    assert!(c.over());
    assert!(!c.continue_latch(), "a loss drops the latch");
    let banked: i32 = score[0][..2].iter().sum();
    assert_eq!(c.settle(&flags).score, banked / 2, "a lost run pays half");

    // The same two legs, then run: nothing at all.
    let mut c = md::DomeContest::from_overlay(&raw, &flags).expect("decodes");
    for _ in 0..2 {
        c.finish_leg(
            md::LegReport {
                survived: true,
                outcome: 0,
                turns_taken: 2,
            },
            500,
            &flags,
        );
        while !matches!(c.state(), md::ContestState::Fight) {
            c.advance();
        }
    }
    assert_eq!(c.tally(), banked, "two legs banked before the give-up");
    c.finish_leg(
        md::LegReport {
            survived: true,
            outcome: md::LEG_OUTCOME_RAN,
            turns_taken: 1,
        },
        500,
        &flags,
    );
    assert!(c.gave_up());
    let out = c.settle(&flags);
    assert_eq!(out.score, 0, "a give-up pays nothing");
    assert!(out.set_gave_up_flag);
}

#[test]
fn the_master_course_is_story_gated_and_pays_the_one_shot_prize() {
    let Some(raw) = arena_overlay() else { return };
    let ladder = md::parse_course_ladder(&raw).expect("course ladder decodes");
    let master = ladder[md::MASTER_COURSE].rounds.len() as u32;
    assert!(
        master > 8,
        "the Master course is the one long enough to be clamped"
    );

    // With none of the three gate flags set the run stops at round 8 - short
    // of the prize - even though the descriptor declares more.
    let closed = md::ContestFlags {
        course_unlock: [true, true, true],
        master_gates: [false; 3],
        prize_awarded: false,
    };
    let mut c = md::DomeContest::from_overlay(&raw, &closed).expect("decodes");
    assert_eq!(c.course(), md::MASTER_COURSE);
    for _ in 0..md::MAX_ROUNDS_PER_COURSE {
        c.finish_leg(
            md::LegReport {
                survived: true,
                outcome: 0,
                turns_taken: 2,
            },
            500,
            &closed,
        );
        if c.over() {
            break;
        }
        while !matches!(c.state(), md::ContestState::Fight) {
            c.advance();
        }
    }
    assert_eq!(c.round(), 8, "clamped by the missing first gate flag");
    assert!(!c.settle(&closed).award_prize);

    // Every gate open: the full run, and the War God Icon exactly once.
    let open = md::ContestFlags {
        course_unlock: [true, true, true],
        master_gates: [true; 3],
        prize_awarded: false,
    };
    let out = run_course(&raw, md::MASTER_COURSE, &open);
    assert_eq!(c_round_of(&raw, &open), master);
    assert!(out.award_prize, "the full Master run awards the prize");
    // Re-running with the one-shot flag already latched awards nothing.
    let latched = md::ContestFlags {
        prize_awarded: true,
        ..open
    };
    assert!(!run_course(&raw, md::MASTER_COURSE, &latched).award_prize);
}

/// The round a fully-open Master run settles on, so the prize assertion above
/// is anchored to the disc's own course length rather than to a literal.
fn c_round_of(raw: &[u8], flags: &md::ContestFlags) -> u32 {
    let mut c = md::DomeContest::from_overlay(raw, flags).expect("decodes");
    for _ in 0..md::MAX_ROUNDS_PER_COURSE {
        c.finish_leg(
            md::LegReport {
                survived: true,
                outcome: 0,
                turns_taken: 2,
            },
            500,
            flags,
        );
        if c.over() {
            break;
        }
        while !matches!(c.state(), md::ContestState::Fight) {
            c.advance();
        }
    }
    c.round()
}
