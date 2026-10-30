use super::*;
use crate::active::{ActiveMode, BuildOptions, builtin_sections};
use legaia_expansion_format::modes::PartyNight as Def;

fn players(n: u32) -> Vec<(PlayerKey, String, bool)> {
    (0..n).map(|k| (k * 10, format!("P{k}"), true)).collect()
}

fn builtin_def() -> Def {
    builtin_sections()
        .0
        .party_night("builtin-party")
        .expect("the built-in night")
        .clone()
}

fn night(n: u32) -> PartyNight {
    PartyNight::new(&builtin_def(), &builtin_sections().0, &players(n), 9).unwrap()
}

/// Run the clock until the night cues something.
fn run_to_cue(night: &mut PartyNight) -> PartyCue {
    for _ in 0..100_000 {
        let c = night.tick();
        if c != PartyCue::None || matches!(night.phase, PartyPhase::Pick { .. }) {
            return c;
        }
    }
    panic!("no cue");
}

#[test]
fn the_builtin_night_resolves_and_every_round_builds() {
    let n = night(3);
    assert_eq!(n.total_rounds(), 4);
    assert_eq!(n.infos[0].kind, ModeKind::Golf);
    assert_eq!(n.infos[0].detail, "1 hole");
    assert_eq!(n.infos[1].detail, "2 hands, 200 chips");
    assert_eq!(n.infos[1].wager, 25);
    assert_eq!(n.infos[2].detail, "1 lap");
    assert_eq!(n.infos[3].detail, "King of the hill");
    let (base, cards) = builtin_sections();
    for r in &n.rounds {
        let section = round_section(base, r, 4).unwrap();
        let m = ActiveMode::build(&section, cards, &r.mode, 1, &BuildOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", r.mode));
        assert!(!m.is_finished());
    }
    let golf = round_section(base, &n.rounds[0], 4).unwrap();
    assert_eq!(golf.golf[0].holes.len(), 1);
    let poker = round_section(base, &n.rounds[1], 4).unwrap();
    assert_eq!(poker.poker[0].seats, 4);
    assert_eq!(poker.poker[0].buy_in, 200);
    assert_eq!(poker.poker[0].max_hands, Some(2));
    let race = round_section(base, &n.rounds[2], 4).unwrap();
    assert_eq!((race.races[0].laps, race.races[0].racers), (1, 4));
}

#[test]
fn points_by_place_with_ties_double_final_and_a_wager() {
    let mut n = night(3);
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 0 });
    // Golf: P1 wins, P0 and P2 tie for second.
    n.record(&[(10, 1), (0, 2), (20, 2)]).unwrap();
    let scores: Vec<i64> = n.players.iter().map(|p| p.score).collect();
    assert_eq!(scores, vec![6, 10, 6]);
    // Poker, a 25% wager: stakes 1, 2, 1 -> pot 4 to the winner P2.
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 1 });
    let rec = n.record(&[(20, 1), (0, 2), (10, 3)]).unwrap().clone();
    assert_eq!(rec.pot, 4);
    let scores: Vec<i64> = n.players.iter().map(|p| p.score).collect();
    assert_eq!(scores, vec![6 - 1 + 6, 10 - 2 + 4, 6 - 1 + 10 + 4]);
    // Race, then the duel ladder: the final round scores double.
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 2 });
    n.record(&[(0, 1), (10, 2), (20, 3)]).unwrap();
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 3 });
    assert_eq!(n.multiplier(3), 2);
    // Standings now: P0 21, P2 23, P1 18 -> ladder P1 (king) vs P0, then
    // the winner vs P2.
    assert_eq!(n.current_match(), Some((10, 0)));
    assert!(n.record_duel(Some(0)).is_none());
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 3 });
    assert_eq!(n.current_match(), Some((0, 20)));
    let rec = n.record_duel(None).unwrap().clone();
    // A draw keeps the king: P0 first, P2 second, P1 third, doubled.
    let places: Vec<(PlayerKey, Option<u8>, i64)> = rec
        .rows
        .iter()
        .map(|r| (r.key, r.place, r.points))
        .collect();
    assert_eq!(
        places,
        vec![(0, Some(1), 20), (20, Some(2), 12), (10, Some(3), 8)]
    );
    assert_eq!(run_to_cue(&mut n), PartyCue::Podium);
    let s = n.standings();
    assert_eq!((s[0].key, s[0].score, s[0].place), (0, 41, 1));
    assert_eq!(n.prize_for(0).map(|p| p.gold), Some(300));
    assert_eq!(n.prize_for(20).map(|p| p.gold), Some(150));
    assert_eq!(run_to_cue(&mut n), PartyCue::Over);
    assert!(n.is_over() && n.completed());
}

#[test]
fn a_voided_round_scores_nothing_and_the_night_goes_on() {
    let mut n = night(2);
    run_to_cue(&mut n);
    let before = n.scoreboard_hash();
    let rec = n.void_round().unwrap();
    assert!(rec.voided && rec.rows.is_empty());
    assert_ne!(n.scoreboard_hash(), before);
    assert!(n.players.iter().all(|p| p.score == 0));
    assert_eq!(run_to_cue(&mut n), PartyCue::Start { slot: 1 });
    assert_eq!(n.round_number(), 2);
}

#[test]
fn a_player_who_leaves_keeps_their_total_but_scores_and_wins_nothing() {
    let mut n = night(3);
    run_to_cue(&mut n);
    n.record(&[(20, 1), (0, 2), (10, 3)]).unwrap();
    n.mark_left(20);
    run_to_cue(&mut n);
    n.record(&[(20, 1), (0, 2), (10, 3)]).unwrap();
    let p2 = n.player(20).unwrap();
    assert_eq!(p2.score, 10, "the leaver's total stands");
    for _ in 0..2 {
        run_to_cue(&mut n);
        if n.hill.is_some() {
            while n.current_match().is_some() {
                let (_, c) = n.current_match().unwrap();
                n.record_duel(Some(c));
                if n.current_match().is_some() {
                    run_to_cue(&mut n);
                }
            }
        } else {
            n.record(&[(0, 1), (10, 2)]);
        }
    }
    assert_eq!(run_to_cue(&mut n), PartyCue::Podium);
    assert_eq!(n.prize_for(20), None);
}

#[test]
fn last_place_picks_the_next_round() {
    let mut def = builtin_def();
    def.last_place_picks = true;
    let mut n = PartyNight::new(&def, &builtin_sections().0, &players(3), 1).unwrap();
    run_to_cue(&mut n);
    n.record(&[(0, 1), (10, 2), (20, 3)]).unwrap();
    run_to_cue(&mut n);
    let PartyPhase::Pick { picker, .. } = n.phase else {
        panic!("{:?}", n.phase)
    };
    assert_eq!(picker, 20);
    assert!(!n.pick(0, 1, true), "only the picker picks");
    n.pick(20, 2, false);
    assert_eq!(n.next_slot(), Some(3));
    n.pick(20, 0, true);
    assert_eq!(n.current_slot(), Some(3));
    // An AI rival in last place picks at once.
    let mut players = players(3);
    players[2].2 = false;
    let mut n = PartyNight::new(&def, &builtin_sections().0, &players, 1).unwrap();
    run_to_cue(&mut n);
    n.record(&[(0, 1), (10, 2), (20, 3)]).unwrap();
    for _ in 0..BOARD_FRAMES {
        n.tick();
    }
    assert!(matches!(n.phase, PartyPhase::Intro { .. }));
}

#[test]
fn a_shuffled_playlist_is_a_function_of_the_seed() {
    let mut def = builtin_def();
    def.order = legaia_expansion_format::modes::PartyOrder::Shuffle;
    let base = &builtin_sections().0;
    let a = PartyNight::new(&def, base, &players(2), 77).unwrap();
    let b = PartyNight::new(&def, base, &players(2), 77).unwrap();
    assert_eq!(a.queue, b.queue);
    assert_eq!(a.scoreboard_hash(), b.scoreboard_hash());
    let mut sorted = a.queue.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2, 3]);
    let differs =
        (0..20u64).any(|s| PartyNight::new(&def, base, &players(2), s).unwrap().queue != a.queue);
    assert!(differs, "the seed shuffles");
}

#[test]
fn a_night_round_trips_through_serde_and_the_hash_ignores_timers() {
    let mut n = night(2);
    let h = n.scoreboard_hash();
    n.tick();
    assert_eq!(n.scoreboard_hash(), h, "timers are presentation");
    let json = serde_json::to_string(&n).unwrap();
    let back: PartyNight = serde_json::from_str(&json).unwrap();
    assert_eq!(back, n);
    assert!(
        json.len() < 4096,
        "a snapshot fits an event: {}",
        json.len()
    );
}

#[test]
fn bad_nights_are_refused() {
    let base = &builtin_sections().0;
    let mut def = builtin_def();
    assert_eq!(
        PartyNight::new(&def, base, &players(1), 0).unwrap_err(),
        PartyError::TooFewPlayers
    );
    def.rounds[0].holes = vec![9];
    assert!(matches!(
        PartyNight::new(&def, base, &players(2), 0),
        Err(PartyError::BadHole { .. })
    ));
    def.rounds[0].mode = "nope".into();
    assert!(matches!(
        PartyNight::new(&def, base, &players(2), 0),
        Err(PartyError::UnknownInstance(_))
    ));
}

#[test]
fn ending_early_grants_nothing() {
    let mut n = night(2);
    run_to_cue(&mut n);
    n.record(&[(0, 1), (10, 2)]);
    n.end_early();
    assert!(n.is_over() && !n.completed());
    assert_eq!(n.prize_for(0), None);
}
