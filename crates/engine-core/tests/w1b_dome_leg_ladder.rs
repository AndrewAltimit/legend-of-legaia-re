//! Muscle Dome ladder: a leg **played to its between-leg tally**, off the
//! disc's own course ladder, through `World::tick`.
//!
//! ## Why this instrument exists
//!
//! `minigame_replay`'s dome rung enters a `MuscleDomeSession` through the
//! door and stops at the first advanced turn, and
//! `muscle_dome_minigame_real.rs` plays a whole contest - but neither opens a
//! `DomeContest`, the ladder wrapper that owns the between-leg screen. The
//! door warp (`scene/host/minigame_warp.rs`) does not install one either, so
//! the arena's own ladder, its four tally rows and the recovery they feed had
//! no pad-driven run behind them: `parse_course_ladder` (`FUN_801D1510`),
//! `leg_score_rows` (`FUN_801D1184`) and `LegScoreRows::hp_restore`
//! (`FUN_801CF074`) were all unentered.
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | the disc's course ladder opens a contest | the arena's `(course, round)` table is the one the run uses |
//! | 2 | a leg played by pad reaches a decision | the leg is a fight, not a turn counter |
//! | 3 | the decided leg reports into the ladder and rolls the tally | the four rows are computed from the leg that just happened |
//! | 4 | the recovery lanes hand HP back to the fighter | a dome leg costs no permanent HP |
//!
//! **The intermission is a fight boundary, never a turn boundary** - the
//! rungs are ordered so a tally can only be observed after a leg *decided*,
//! which is what keeps that reading from silently regressing.
//!
//! Disc-gated: the course ladder and the damage model both come off the
//! disc. Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::muscle_dome as asset_md;
use legaia_asset::static_overlay;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::muscle_dome::{
    self as md, ContestState, DomeCombatant, DomeDamageModel, GlideStep, MuscleCard,
    MuscleDomeSession, MusclePhase, SpriteGlide,
};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

/// The arena overlay - the entry that carries the course descriptor table and
/// the score rows.
const ARENA_PROT_INDEX: u32 = 977;

const PLAYER_HP: i32 = 500;
const OPPONENT_HP: i32 = 400;

struct DiscTables {
    arena: Vec<u8>,
    battle: Vec<u8>,
    commands: [u8; 4],
}

fn disc_tables() -> Option<DiscTables> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    let arena = host.index.entry_bytes_extended(ARENA_PROT_INDEX).ok()?;
    let rec =
        static_overlay::overlay_map().by_prot_index(asset_md::MUSCLE_OVERLAY_PROT_INDEX as u32)?;
    let battle = host.index.entry_bytes_extended(rec.prot_index).ok()?;
    let loaded = static_overlay::as_loaded(&battle, rec).ok()?;
    let commands = asset_md::hand_command_ids(&loaded)?;
    Some(DiscTables {
        arena,
        battle,
        commands,
    })
}

fn session_from(t: &DiscTables) -> MuscleDomeSession {
    let card = |cmd: u8| MuscleCard {
        command_id: cmd,
        cost: 0x1E,
    };
    let hand: [MuscleCard; md::HAND_SLOTS] = std::array::from_fn(|i| card(t.commands[i]));
    let mut session = MuscleDomeSession::new(hand, hand, [120, 120], [PLAYER_HP, OPPONENT_HP], 1);
    let profile = |hp: i32| DomeCombatant {
        hp_max: hp as u16,
        int: 60,
        udf: 20,
        ldf: 20,
        element: 0,
    };
    if let Some(model) = DomeDamageModel::from_battle_overlay(
        &t.battle,
        [profile(PLAYER_HP), profile(OPPONENT_HP)],
        [PLAYER_HP, OPPONENT_HP],
        0x4D55_5343,
    ) {
        session.install_damage_model(model);
    }
    session
}

/// Play the entered leg by pad until the session decides it.
fn play_leg(world: &mut World) -> md::LegReport {
    let directions = [
        PadButton::Left.mask(),
        PadButton::Right.mask(),
        PadButton::Up.mask(),
        PadButton::Down.mask(),
    ];
    let mut frames = 0u32;
    loop {
        frames += 1;
        assert!(frames < 20_000, "the leg never decided");
        let s = world.muscle_dome.as_ref().expect("session installed");
        if s.decided() {
            break;
        }
        let pad = if frames.is_multiple_of(2) {
            0
        } else {
            match s.phase() {
                MusclePhase::Select => {
                    let pick = (0..md::HAND_SLOTS)
                        .filter(|&c| s.can_commit(0, c))
                        .min_by_key(|&c| s.hand(0)[c].cost);
                    match pick {
                        Some(c) => directions[c],
                        None => PadButton::Cross.mask(),
                    }
                }
                MusclePhase::Resolve => 0,
                MusclePhase::TurnOver | MusclePhase::Won | MusclePhase::Lost => {
                    PadButton::Cross.mask()
                }
            }
        };
        world.input.set_pad(pad);
        let _ = world.tick();
    }
    let s = world.muscle_dome.as_ref().expect("session installed");
    md::LegReport {
        survived: matches!(s.phase(), MusclePhase::Won),
        outcome: 0,
        turns_taken: s.turn(),
    }
}

// ---------------------------------------------------------------------------
// Rung 1 - the disc's course ladder
// ---------------------------------------------------------------------------

#[test]
fn rung1_the_disc_course_ladder_opens_a_contest() {
    let Some(t) = disc_tables() else { return };
    let ladder = md::parse_course_ladder(&t.arena).expect("PROT 0977 course descriptor decodes");
    assert_eq!(ladder.len(), md::COURSE_COUNT, "three courses");
    for (i, course) in ladder.iter().enumerate() {
        assert!(!course.rounds.is_empty(), "course {i} has rounds");
        assert!(
            course.rounds.len() <= md::MAX_ROUNDS_PER_COURSE,
            "course {i} fits the score table"
        );
        for (r, round) in course.rounds.iter().enumerate() {
            assert!(round.monster_id > 0, "course {i} round {r} names a monster");
        }
    }

    let mut w = World::new();
    let flags = w.muscle_contest_flags();
    w.muscle_contest =
        Some(md::DomeContest::from_overlay(&t.arena, &flags).expect("the contest opens"));
    let run = w.muscle_contest.as_ref().unwrap();
    assert_eq!(run.round(), 0, "a fresh contest starts before leg one");
    assert_eq!(
        run.state(),
        ContestState::Fight,
        "the first leg is stageable"
    );
    assert!(
        run.course() < md::COURSE_COUNT,
        "the unlock flags pick a real course"
    );
}

// ---------------------------------------------------------------------------
// Rungs 2-4 - a leg played, reported and paid
// ---------------------------------------------------------------------------

/// The whole chain in one run, because the rungs are only meaningful in
/// order: a tally observed without a decided leg behind it would be the
/// "intermission is a turn boundary" reading this file exists to keep out.
#[test]
fn rungs2to4_a_played_leg_reports_into_the_ladder_and_pays_its_rows() {
    let Some(t) = disc_tables() else { return };

    let mut w = World::new();
    w.mode = SceneMode::Field;
    let flags = w.muscle_contest_flags();
    w.muscle_contest =
        Some(md::DomeContest::from_overlay(&t.arena, &flags).expect("the contest opens"));
    let course = w.muscle_contest.as_ref().unwrap().course();
    w.enter_muscle_dome(session_from(&t));
    assert_eq!(w.mode, SceneMode::MuscleDome);

    // Rung 2: the leg is a fight, decided by knockout.
    let report = play_leg(&mut w);
    assert!(
        report.turns_taken > 0,
        "a decided leg took at least one turn"
    );

    // Rung 3: reporting it advances the ladder and rolls the four rows.
    let before_hp = w
        .roster
        .members
        .first()
        .map(|r| r.hp_mp_sp().hp_cur)
        .unwrap_or(0);
    let state = w.report_muscle_leg(report).expect("a contest is open");
    let run = w.muscle_contest.as_ref().expect("the contest survives");
    assert_eq!(run.round(), 1, "the ladder advanced one leg");
    let rows = run.rows();
    assert_eq!(
        rows.score_cell,
        md::course_score_cell(&t.arena, course, 1).unwrap_or(0),
        "the score row is the disc's own `(course, round)` cell"
    );
    // The three recovery lanes are HP, not money - the property the port had
    // to get right to stop the tally screen paying twice.
    assert_eq!(
        rows.hp_restore(),
        rows.round_lane + rows.turns_lane + rows.outcome_lane,
        "the recovery total is the three lanes and nothing else"
    );
    assert!(
        rows.hp_restore() >= 0,
        "a recovery lane never takes HP away"
    );

    // Rung 4: the recovery reaches the fighter's record, and the contest is
    // left ready for the next leg (or settling).
    if report_survived(state) && !w.roster.members.is_empty() {
        let after_hp = w
            .roster
            .members
            .first()
            .map(|r| r.hp_mp_sp().hp_cur)
            .unwrap_or(0);
        assert!(
            after_hp >= before_hp,
            "the between-leg restore lowered HP ({before_hp} -> {after_hp})"
        );
    }
    assert!(
        matches!(
            w.muscle_contest.as_ref().unwrap().state(),
            ContestState::Fight | ContestState::Restore | ContestState::Settle
        ),
        "the hub landed somewhere the next leg can start from"
    );
}

fn report_survived(state: ContestState) -> bool {
    !matches!(state, ContestState::Settle | ContestState::Settled)
}

// ---------------------------------------------------------------------------
// The glide table, which has no producer
// ---------------------------------------------------------------------------

/// `FUN_801D9BBC` steps one of the arena's `0x28` sprite-glide handles. The
/// kernel is ported and correct, and **nothing in the engine writes a glide
/// record** - there is no producer for the `(start, target, total)` triple, so
/// no session reaches it. That is the same shape as `camera_rel_glide`'s
/// declined row, and it is stated here rather than dressed up as a wire: what
/// this asserts is the kernel's own contract, not a reached path.
#[test]
fn the_sprite_glide_kernel_eases_and_deactivates_but_has_no_producer() {
    // Inactive slot: nothing written.
    let mut idle = SpriteGlide::default();
    assert_eq!(idle.step(1), GlideStep::Idle);

    // In flight: linear interpolation, and the remaining count retail folds
    // into its return.
    let mut g = SpriteGlide {
        total: 10,
        elapsed: 0,
        start: (0, 100),
        target: (100, 0),
    };
    let GlideStep::Moving { pos, remaining } = g.step(2) else {
        panic!("a live handle with 8 frames left is still moving");
    };
    assert_eq!(pos, (20, 80), "start + (target - start) * elapsed / total");
    assert_eq!(remaining, 9, "total - elapsed + 1");

    // Arrival is tested *before* the accumulate, so a step that would overrun
    // snaps to the target and deactivates the slot.
    let mut g = SpriteGlide {
        total: 10,
        elapsed: 8,
        start: (0, 0),
        target: (50, 50),
    };
    assert_eq!(g.step(4), GlideStep::Arrived { pos: (50, 50) });
    assert_eq!(g.total, 0, "an arrived handle deactivates");
    assert_eq!(g.step(1), GlideStep::Idle);
}
