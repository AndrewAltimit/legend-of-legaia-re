//! Networked modes end to end: poker over the host-authoritative view
//! stream, golf and prop hunt in lockstep, a desync caught and surfaced, a
//! dropped player's seat handed to the AI.

use std::collections::BTreeMap;

use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_modes::active::ModeView;
use legaia_engine_modes::poker::Phase;
use legaia_engine_system::pad::PadButton;

use crate::harness::{Room, lan};

const CROSS: u16 = PadButton::Cross as u16;

/// Cross tapped every eighth frame.
fn tap(frame: u32) -> u16 {
    if frame.is_multiple_of(8) { CROSS } else { 0 }
}

fn poker_view(p: &crate::harness::Peer) -> Option<legaia_engine_modes::poker::PokerView> {
    match p.mode()?.view() {
        ModeView::Poker(v) => Some(v),
        _ => None,
    }
}

#[test]
fn a_poker_hand_plays_over_the_view_stream_without_leaking_hole_cards() {
    let mut room = Room::open(2, lan(), 1);
    room.start("builtin-poker");
    let host_seat = room.peers[0].mode().unwrap().local_seat;
    let guest_seat = room.peers[1].mode().unwrap().local_seat;
    assert_eq!((host_seat, guest_seat), (0, 1));
    let mut frame = 0u32;
    let mut showdowns = 0;
    let mut was_showdown = false;
    while showdowns < 2 && frame < 60 * 60 * 5 {
        room.step_with(&mut |_, _| tap(frame));
        frame += 1;
        let guest = poker_view(&room.peers[1]).expect("the guest draws a table");
        let showdown = matches!(guest.phase, Phase::Showdown { .. } | Phase::Finished);
        // Before a showdown the guest never holds the host's hole cards.
        if !showdown {
            assert_eq!(guest.seats[host_seat].hole, None, "frame {frame}");
        }
        if showdown && !was_showdown {
            showdowns += 1;
        }
        was_showdown = showdown;
    }
    assert!(showdowns >= 2, "two hands reach their showdown");
    // Let the last frames arrive, then the guest's table is the host's as
    // the guest may see it.
    for _ in 0..8 {
        room.step();
    }
    let host = poker_view(&room.peers[0]).unwrap();
    let guest = poker_view(&room.peers[1]).unwrap();
    assert_eq!(guest.hand, host.hand);
    assert_eq!(guest.pot, host.pot);
    assert_eq!(guest.board, host.board);
    let chips = |v: &legaia_engine_modes::poker::PokerView| -> Vec<u32> {
        v.seats.iter().map(|s| s.chips).collect()
    };
    assert_eq!(chips(&guest), chips(&host));
    assert_eq!(guest.seats[guest_seat].hole, host.seats[guest_seat].hole);
    // The guest's log is worded for the guest.
    let log = &room.peers[1]
        .world
        .expansion
        .mode_seat
        .as_ref()
        .unwrap()
        .log;
    assert!(!log.is_empty());
}

/// Run a lockstep mode to its end, recording every peer's hash per frame.
fn play_lockstep(
    room: &mut Room,
    pads: impl Fn(usize, u32) -> u16,
    limit: u32,
) -> Vec<BTreeMap<u32, u64>> {
    let mut hashes = vec![BTreeMap::new(); room.peers.len()];
    let mut frame = 0u32;
    while frame < limit {
        room.step_with(&mut |i, _| pads(i, frame));
        frame += 1;
        for (i, p) in room.peers.iter().enumerate() {
            if let (Some(f), Some(m)) = (p.net.mode_frame(), p.mode()) {
                hashes[i].insert(f, m.state_hash());
            }
        }
        let done = room.peers.iter().all(|p| {
            matches!(
                p.world.expansion.mode_seat.as_ref().map(|s| &s.stage),
                Some(ModeStage::Results { .. })
            )
        });
        if done {
            break;
        }
    }
    hashes
}

fn assert_agree(hashes: &[BTreeMap<u32, u64>]) -> usize {
    let mut common = 0;
    for (f, h) in &hashes[0] {
        for other in &hashes[1..] {
            if let Some(o) = other.get(f) {
                assert_eq!(h, o, "peers differ at frame {f}");
                common += 1;
            }
        }
    }
    common
}

#[test]
fn a_lockstep_golf_round_ends_identically_on_both_peers() {
    let mut room = Room::open(2, lan(), 2);
    room.start("builtin-golf");
    assert_eq!(room.peers[1].mode().unwrap().local_seat, 1);
    // Both players strike on their own cadence; the AI seats play too.
    let hashes = play_lockstep(&mut room, |i, f| tap(f + 3 * i as u32), 60 * 60 * 30);
    assert!(assert_agree(&hashes) > 600, "the peers shared most frames");
    let rows = |i: usize| match &room.peers[i]
        .world
        .expansion
        .mode_seat
        .as_ref()
        .unwrap()
        .stage
    {
        ModeStage::Results {
            rows, local_seat, ..
        } => (rows.clone(), *local_seat),
        _ => panic!("peer {i} did not reach the results"),
    };
    let (host_rows, host_seat) = rows(0);
    let (guest_rows, guest_seat) = rows(1);
    assert_eq!(host_rows, guest_rows, "one result for the table");
    assert_eq!((host_seat, guest_seat), (0, 1));
    assert!(room.peers[0].net.verified_checkpoints() > 5);
}

#[test]
fn a_lockstep_prop_hunt_match_ends_identically() {
    let mut room = Room::open(2, lan(), 3);
    room.start("builtin-prop-hunt");
    let walk = [
        PadButton::Right as u16,
        PadButton::Down as u16,
        PadButton::Left as u16,
        PadButton::Up as u16,
    ];
    let hashes = play_lockstep(
        &mut room,
        |i, f| walk[((f / 90) as usize + i) % 4] | tap(f + i as u32),
        60 * 60 * 6,
    );
    assert!(assert_agree(&hashes) > 600);
    assert!(room.peers.iter().all(|p| matches!(
        p.world.expansion.mode_seat.as_ref().map(|s| &s.stage),
        Some(ModeStage::Results { .. })
    )));
}

#[test]
fn a_desync_is_caught_and_sends_everyone_back_to_the_lobby() {
    let mut room = Room::open(2, lan(), 4);
    room.start("builtin-golf");
    for f in 0..90 {
        room.step_with(&mut |_, _| tap(f));
    }
    // Break the guest's copy behind the session's back: a second of
    // frames, with strokes, that only it simulates.
    let seat = room.peers[1]
        .world
        .expansion
        .mode_seat
        .as_deref_mut()
        .unwrap();
    let mode = seat.active_mut().unwrap();
    let before = mode.state_hash();
    for i in 0..60 {
        mode.tick(&[tap(i * 4); 4]);
    }
    assert_ne!(mode.state_hash(), before);
    assert!(
        room.run_until(400, |r| r.peers.iter().all(|p| p.net.in_lobby())),
        "both peers return to the lobby"
    );
    for p in &room.peers {
        assert!(p.world.expansion.mode_seat.is_none(), "the seat is closed");
    }
    let noted = |i: usize| room.peers[i].net.chat.iter().any(|l| l.contains("Desync"));
    assert!(noted(0) || noted(1), "the desync is reported");
    let n = room.peers[0].net.notice.clone().unwrap_or_default()
        + &room.peers[1].net.notice.clone().unwrap_or_default();
    assert!(n.contains("Desync") || n.contains("ended the game"), "{n}");
    // The room can start again.
    room.start("builtin-golf");
}

#[test]
fn leaving_a_lockstep_game_hands_the_seat_to_the_ai() {
    let mut room = Room::open(3, lan(), 5);
    room.start("builtin-race");
    for f in 0..120 {
        room.step_with(&mut |_, _| tap(f) | PadButton::Up as u16);
    }
    // Guest 2 drops out of the room entirely.
    let p = &mut room.peers[2];
    p.net.leave(&mut p.world);
    let mut moved = 0;
    for f in 0..600 {
        let before = room.peers[0].net.mode_frame();
        room.step_with(&mut |i, _| {
            if i == 2 {
                0
            } else {
                tap(f) | PadButton::Up as u16
            }
        });
        if room.peers[0].net.mode_frame() != before {
            moved += 1;
        }
    }
    // The race runs on (to its finish, which ends the counting).
    assert!(moved > 200, "the remaining peers keep simulating ({moved})");
    assert!(
        room.peers[0]
            .net
            .chat
            .iter()
            .any(|l| l.contains("The AI takes seat 3"))
    );
    let a = room.peers[0].net.mode_frame();
    let b = room.peers[1].net.mode_frame();
    assert!(a.is_some() && b.is_some());
}

#[test]
fn readiness_reads_cleared_the_moment_a_session_ends() {
    let mut room = Room::open(2, lan(), 6);
    room.start("builtin-golf");
    // The host leaves through the quit prompt (Start, then Cross) and its
    // results card (Cross). On the frame it is back in the lobby - before
    // the relay's lobby broadcast can reach it - its copy of the lobby
    // already reads nobody ready, so a Start pressed then is refused by the
    // screen, not by the relay.
    let mut checked = false;
    for pad in [PadButton::Start as u16, 0, CROSS, 0, CROSS, 0, 0, 0] {
        room.step_with(&mut |p, _| if p == 0 { pad } else { 0 });
        let host = &room.peers[0].net;
        if host.in_lobby() && !checked {
            checked = true;
            let l = host.lobby.as_ref().unwrap();
            assert!(l.players.iter().all(|x| !x.ready), "{:?}", l.players);
        }
    }
    assert!(checked, "the host left the game");
    assert!(room.run_until(60, |r| r.peers.iter().all(|p| p.net.in_lobby())));
    for p in &room.peers {
        let l = p.net.lobby.as_ref().unwrap();
        assert!(l.players.iter().all(|x| !x.ready), "{:?}", l.players);
        assert!(!p.net.is_ready());
    }
}

/// A host whose own world is not on a free field (here, in a battle) cannot
/// start a game: the guests would get the table while the host's screen
/// kept the other. Back on the field the same start goes through.
#[test]
fn a_host_off_the_free_field_cannot_start_a_game() {
    use legaia_engine_mode::mode::SceneMode;
    let mut room = Room::open(2, lan(), 1);
    room.peers[0].world.mode = SceneMode::Battle;
    let entry = room.peers[0]
        .net
        .game_entries(&room.peers[0].world)
        .into_iter()
        .find(|e| e.id == "builtin-poker")
        .expect("a built-in instance");
    let host = &mut room.peers[0];
    host.net.request_start(&host.world, &entry);
    for _ in 0..60 {
        room.step();
    }
    assert!(room.peers[1].mode().is_none(), "no table for the guest");
    assert!(room.peers[0].net.notice.is_some(), "the host is told why");
    room.peers[0].world.mode = SceneMode::Field;
    room.start("builtin-poker");
    assert!(room.peers[1].mode().is_some());
}

#[test]
fn an_idle_seat_runs_out_of_time_and_the_ai_moves_for_it() {
    use legaia_engine_netplay::view_stream::TURN_LIMIT_FRAMES;
    let mut room = Room::open(2, lan(), 3);
    room.start("builtin-poker");
    let guest_seat = room.peers[1].mode().unwrap().local_seat;
    // The host plays; the guest never touches the pad.
    let mut frame = 0u32;
    let mut guest_acted = false;
    let log_has = |room: &Room, i: usize, needle: &str| {
        room.peers[i]
            .world
            .expansion
            .mode_seat
            .as_ref()
            .is_some_and(|s| s.log.iter().any(|l| l.contains(needle)))
    };
    while frame < TURN_LIMIT_FRAMES * 3 {
        room.step_with(&mut |i, _| if i == 0 { tap(frame) } else { 0 });
        frame += 1;
        if log_has(&room, 0, "Guest1 ran out of time") {
            guest_acted = true;
            // The guest's own log says so in its words.
            for _ in 0..8 {
                room.step();
            }
            assert!(log_has(&room, 1, "Out of time"), "the guest is told");
            break;
        }
    }
    assert!(guest_acted, "the table moved past an idle seat");
    // The seat is the guest's again once the turn moves on.
    for _ in 0..600 {
        room.step_with(&mut |i, _| if i == 0 { tap(frame) } else { 0 });
        frame += 1;
    }
    let seat = room.peers[0].world.expansion.mode_seat.as_ref().unwrap();
    let ModeStage::Playing { mode, .. } = &seat.stage else {
        return;
    };
    if mode.waiting_on() != Some(guest_seat) {
        assert!(
            !mode.seat_kind(guest_seat).is_ai(),
            "the seat is handed back"
        );
    }
}

#[test]
fn seats_read_as_the_players_in_them() {
    let mut room = Room::open(2, lan(), 4);
    room.start("builtin-poker");
    let host = room.peers[0].mode().unwrap();
    assert_eq!(host.seat_name(0), "You");
    assert_eq!(host.seat_name(1), "Guest1");
    let guest = room.peers[1].mode().unwrap();
    assert_eq!(guest.seat_name(0), "Host");
    assert_eq!(guest.seat_name(1), "You");
    assert!(!guest.watching);
}
