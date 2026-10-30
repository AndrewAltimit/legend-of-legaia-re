//! Party night end to end: whole worlds running the built-in night through
//! the relay core - lockstep rounds (golf, racing) and host-authoritative
//! ones (poker, the Grid Duel ladder) - ending on one scoreboard
//! everywhere, a dropped player, a desynced round voided, a late joiner
//! watching, and the prizes landing in each player's own world.

use legaia_engine_modes::active::{ModeKind, ModeView};
use legaia_engine_modes::fixed::angle_delta;
use legaia_engine_modes::party::PartyPhase;
use legaia_engine_netplay::room::NIGHT_ENTRY_PREFIX;
use legaia_engine_system::pad::PadButton;

use crate::harness::{Peer, Room, give_cards, lan};

const NIGHT: &str = "builtin-party";

/// A random walk of presses, one every six frames, Cross most often: it
/// drives poker's menu, places Grid Duel cards and swings golf clubs.
fn fuzz(i: usize, f: u32) -> u16 {
    if !f.is_multiple_of(6) {
        return 0;
    }
    let h = (f / 6).wrapping_mul(2_654_435_761) ^ (i as u32).wrapping_mul(40_503);
    let pick = [
        PadButton::Cross,
        PadButton::Cross,
        PadButton::Cross,
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
        PadButton::Right,
    ];
    pick[(h >> 7) as usize % pick.len()].mask()
}

/// A racer that holds the throttle and steers at its next checkpoint.
fn drive(p: &Peer) -> Option<u16> {
    let m = p.mode()?;
    let ModeView::Race(v) = m.view() else {
        return None;
    };
    let track = m.race_track()?;
    let r = v.racers.get(m.local_seat)?;
    let want = (track.points[r.next_cp] - r.pos).angle();
    let d = angle_delta(r.heading, want);
    let mut pad = PadButton::Cross.mask();
    if d > 30 {
        pad |= PadButton::Right.mask();
    } else if d < -30 {
        pad |= PadButton::Left.mask();
    }
    Some(pad)
}

fn pads(i: usize, p: &Peer, f: u32) -> u16 {
    // Leave a finished night's podium up (any press would close it).
    if p.net.party().is_some_and(|l| {
        matches!(
            l.night.phase,
            PartyPhase::Podium { .. } | PartyPhase::Over { .. }
        )
    }) {
        return 0;
    }
    if p.mode().is_some_and(|m| m.kind() == ModeKind::Race) {
        return drive(p).unwrap_or(0);
    }
    fuzz(i, f)
}

/// Ready everyone and start the built-in night from the host.
fn begin(room: &mut Room) {
    for p in &mut room.peers[1..] {
        p.net.set_ready(true);
    }
    let n = room.peers.len();
    assert!(room.run_until(200, |r| {
        r.peers[0]
            .net
            .lobby
            .as_ref()
            .is_some_and(|l| l.players.iter().filter(|p| p.ready).count() == n - 1)
    }));
    let host = &mut room.peers[0];
    let entry = host
        .net
        .game_entries(&host.world)
        .into_iter()
        .find(|e| e.id == format!("{NIGHT_ENTRY_PREFIX}{NIGHT}"))
        .expect("the built-in night is offered");
    host.net.request_start(&host.world, &entry);
    assert!(
        room.run_until(120, |r| r.peers.iter().all(|p| p.net.party().is_some())),
        "every peer holds the night: {:?}",
        room.peers.iter().map(|p| &p.net.chat).collect::<Vec<_>>()
    );
}

/// Play until every peer still in the room sees the night over (or its
/// podium), up to `limit` frames, with `each` run after every frame.
fn play(room: &mut Room, limit: u32, mut each: impl FnMut(&mut Room, u32)) -> bool {
    for f in 0..limit {
        let done = room.peers.iter().all(|p| {
            p.net.closed
                || p.net
                    .party()
                    .is_some_and(|l| l.night.completed() || l.night.is_over())
        });
        if done {
            return true;
        }
        room.step_with(&mut |i, p| pads(i, p, f));
        each(room, f);
    }
    false
}

fn hashes(room: &Room) -> Vec<u64> {
    room.peers
        .iter()
        .filter(|p| !p.net.closed)
        .map(|p| p.net.party().unwrap().night.scoreboard_hash())
        .collect()
}

#[test]
fn a_four_player_night_ends_on_one_scoreboard_everywhere() {
    let mut room = Room::open(4, lan(), 11);
    for p in &mut room.peers {
        give_cards(&mut p.world, &[]);
    }
    begin(&mut room);
    let mut kinds = Vec::new();
    let finished = play(&mut room, 60 * 60 * 30, |r, _| {
        if let Some(k) = r.peers[0].mode().map(|m| m.kind())
            && kinds.last() != Some(&k)
        {
            kinds.push(k);
        }
    });
    assert!(
        finished,
        "the night reaches its podium: {:?}",
        room.peers[0].net.chat
    );
    // Lockstep and host-authoritative rounds both ran.
    for k in [
        ModeKind::Golf,
        ModeKind::Poker,
        ModeKind::Race,
        ModeKind::CardDuel,
    ] {
        assert!(
            kinds.contains(&k),
            "{k:?} ran: {kinds:?} {:?} {:?}",
            room.peers[0].net.chat,
            room.peers[0]
                .net
                .party()
                .map(|p| (&p.night.phase, &p.night.history))
        );
    }
    let h = hashes(&room);
    assert!(h.windows(2).all(|w| w[0] == w[1]), "one scoreboard: {h:?}");
    let host = room.peers[0].net.party().unwrap();
    assert_eq!(host.night.history.len(), 4);
    assert!(host.night.history.iter().all(|r| !r.voided));
    for p in &room.peers[1..] {
        let l = p.net.party().unwrap();
        assert!(
            l.agreed >= 4,
            "each guest verified every round: {}",
            l.agreed
        );
        assert_eq!(l.resyncs, 0, "and never had to adopt the host's");
    }
    // Every machine granted its own player the prize for their place:
    // first place's booster opened into their collection, gold for the
    // podium.
    let standings = host.night.standings();
    for (i, p) in room.peers.iter().enumerate() {
        let me = p.net.me.unwrap();
        let place = standings.iter().find(|s| s.key == me.0).unwrap().place;
        let granted = p.net.party().unwrap().granted.clone().expect("granted");
        let gold = match place {
            1 => 300,
            2 => 150,
            3 => 50,
            _ => 0,
        };
        assert_eq!(p.world.party.money, gold, "peer {i} (place {place})");
        let boosters = p
            .world
            .expansion
            .quests
            .progress
            .cards
            .booster_count("builtin-booster");
        if place == 1 {
            assert!(granted.iter().any(|l| l.contains("Booster")), "{granted:?}");
            assert_eq!(boosters, 1, "the booster is in peer {i}'s collection");
        } else {
            assert_eq!(boosters, 0);
        }
    }
}

#[test]
fn a_player_who_drops_keeps_their_score_and_the_night_goes_on() {
    let mut room = Room::open(3, lan(), 12);
    begin(&mut room);
    let mut dropped = false;
    let finished = play(&mut room, 60 * 60 * 30, |r, _| {
        // Guest 2 drops out during the second round (poker).
        let second = r.peers[0]
            .net
            .party()
            .is_some_and(|l| l.night.history.len() == 1 && l.round_live);
        if !dropped && second && r.peers[2].mode().is_some() {
            dropped = true;
            let p = &mut r.peers[2];
            p.net.leave(&mut p.world);
        }
    });
    assert!(dropped);
    assert!(finished, "{:?}", room.peers[0].net.chat);
    let h = hashes(&room);
    assert_eq!(h.len(), 2);
    assert_eq!(h[0], h[1], "the two left agree");
    let night = &room.peers[0].net.party().unwrap().night;
    let gone = night.players.iter().find(|p| p.left).expect("marked gone");
    // Their first-round points stand; they scored nothing after.
    let first = night.history[0]
        .rows
        .iter()
        .find(|r| r.key == gone.key)
        .map_or(0, |r| r.points);
    assert_eq!(gone.score, first);
    assert!(night.history[1..].iter().all(|r| {
        r.rows
            .iter()
            .all(|row| row.key != gone.key || row.points == 0)
    }));
    assert_eq!(night.prize_for(gone.key), None);
}

#[test]
fn a_desynced_round_is_void_and_the_night_continues() {
    let mut room = Room::open(2, lan(), 13);
    begin(&mut room);
    let mut broke = false;
    let finished = play(&mut room, 60 * 60 * 30, |r, f| {
        // A few seconds into the opening golf round, break the guest's
        // copy behind the session's back.
        if broke || f < 400 {
            return;
        }
        let Some(seat) = r.peers[1].world.expansion.mode_seat.as_deref_mut() else {
            return;
        };
        let Some(mode) = seat.active_mut() else {
            return;
        };
        if mode.kind() != ModeKind::Golf {
            return;
        }
        for i in 0..60 {
            mode.tick(
                &[if i % 4 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                }; 4],
            );
        }
        broke = true;
    });
    assert!(broke);
    assert!(finished, "{:?}", room.peers[0].net.chat);
    let h = hashes(&room);
    assert_eq!(h[0], h[1]);
    let night = &room.peers[0].net.party().unwrap().night;
    assert!(night.history[0].voided, "the desynced golf round is void");
    assert!(night.history[1..].iter().all(|r| !r.voided));
    assert_eq!(night.history.len(), 4);
}

#[test]
fn a_late_joiner_watches_the_rest_of_the_night() {
    let mut room = Room::open(2, lan(), 14);
    begin(&mut room);
    // Join between the first and second rounds.
    let mut between = false;
    for f in 0..60 * 60 * 10 {
        between = room.peers[0]
            .net
            .party()
            .is_some_and(|l| l.night.history.len() == 1 && !l.round_live);
        if between {
            break;
        }
        room.step_with(&mut |i, p| pads(i, p, f));
    }
    assert!(between, "{:?}", room.peers[0].net.chat);
    let code = room.peers[0].net.room.unwrap();
    let (client, server) = legaia_netplay::transport::loopback::link(&room.clock, lan(), lan(), 99);
    let now = room.clock.now() * 16;
    room.relay.add(server, now);
    room.peers.push(Peer {
        world: crate::harness::field_world(),
        net: legaia_engine_netplay::room::NetPlay::join(Box::new(client), "Late", code, None),
    });
    assert!(
        room.run_until(300, |r| r.peers[2].net.party().is_some()),
        "the joiner gets the night"
    );
    let late = room.peers[2].net.me.unwrap();
    let finished = play(&mut room, 60 * 60 * 30, |_, _| {});
    assert!(finished, "{:?}", room.peers[0].net.chat);
    let h = hashes(&room);
    assert!(h.windows(2).all(|w| w[0] == w[1]), "{h:?}");
    let night = &room.peers[0].net.party().unwrap().night;
    assert!(night.player(late.0).is_none(), "a watcher, not a player");
    assert_eq!(night.players.len(), 2);
    let lobby = room.peers[0].net.lobby.as_ref().unwrap();
    let row = lobby.players.iter().find(|p| p.id == late).unwrap();
    assert_eq!(row.seat, None, "moved off the seats");
}
