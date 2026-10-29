//! The relay core's lobby rules: admission checks, seats, readiness, host
//! migration, anti-spoofing and the rate limits.

use legaia_netplay::protocol::*;
use legaia_netplay::relay::{ConnId, RelayCore, RelayLimits, RelayOutput};
use legaia_netplay::transport::loopback::LinkConditions;

use crate::harness::{BUILD, Net, hello};

fn instant() -> LinkConditions {
    LinkConditions::default()
}

fn create(net: &mut Net, seats: u8) -> RoomCode {
    net.clients[0].send(hello("host", RoomRequest::Create { seats }));
    net.step();
    net.clients[0].room.expect("room created")
}

#[test]
fn joiners_take_free_seats_then_spectate() {
    let mut net = Net::new(3, instant(), 1);
    let code = create(&mut net, 2);
    net.clients[1].send(hello("a", RoomRequest::Join(code)));
    net.clients[2].send(hello("b", RoomRequest::Join(code)));
    net.step();
    let lobby = net.clients[0].lobby.clone().unwrap();
    let seats: Vec<Option<u8>> = lobby.players.iter().map(|p| p.seat).collect();
    assert_eq!(seats, vec![Some(0), Some(1), None]);
    assert_eq!(lobby.host, net.clients[0].me.unwrap());
}

#[test]
fn mismatched_build_pack_and_code_are_refused() {
    let mut net = Net::new(2, instant(), 2);
    let code = create(&mut net, 2);
    let mut other_pack = hello("x", RoomRequest::Join(code));
    if let Message::Hello(h) = &mut other_pack {
        h.pack.as_mut().unwrap().content_hash[0] ^= 1;
    }
    net.clients[1].send(other_pack);
    net.step();
    let mut other_build = hello("x", RoomRequest::Join(code));
    if let Message::Hello(h) = &mut other_build {
        h.engine_build = format!("{BUILD}-other");
    }
    net.clients[1].send(other_build);
    net.step();
    net.clients[1].send(hello(
        "x",
        RoomRequest::Join(RoomCode::parse("ZZZZZZ").unwrap()),
    ));
    net.step();
    assert_eq!(
        net.clients[1].rejects,
        vec![
            RejectReason::PackMismatch,
            RejectReason::BuildMismatch,
            RejectReason::NoSuchRoom
        ]
    );
    // None of those are fatal: the same connection still gets in.
    net.clients[1].send(hello("x", RoomRequest::Join(code)));
    net.step();
    assert!(net.clients[1].me.is_some());
    assert!(!net.clients[1].closed);
}

#[test]
fn guessing_room_codes_gets_the_connection_dropped() {
    let mut net = Net::new(1, instant(), 3);
    for i in 0..RelayLimits::default().max_bad_codes {
        let code = RoomCode::from_bits(u64::from(i) + 1000);
        net.clients[0].send(hello("guesser", RoomRequest::Join(code)));
        net.step();
    }
    assert_eq!(
        net.clients[0].rejects.last(),
        Some(&RejectReason::RateLimited)
    );
    net.step();
    assert!(net.clients[0].closed);
}

#[test]
fn wrong_protocol_version_is_fatal() {
    let mut net = Net::new(1, instant(), 4);
    let mut h = hello("old", RoomRequest::Create { seats: 2 });
    if let Message::Hello(h) = &mut h {
        h.protocol = PROTOCOL_VERSION + 1;
    }
    net.clients[0].send(h);
    net.step();
    net.step();
    assert_eq!(
        net.clients[0].rejects,
        vec![RejectReason::ProtocolMismatch {
            server: PROTOCOL_VERSION
        }]
    );
    assert!(net.clients[0].closed);
}

#[test]
fn start_needs_the_host_and_every_seated_player_ready() {
    let mut net = Net::new(3, instant(), 5);
    let code = create(&mut net, 2);
    net.clients[1].send(hello("a", RoomRequest::Join(code)));
    net.step();
    let start = Message::Start(StartGame {
        seed: 1,
        mode: "m".into(),
        config: vec![],
        model: SimModel::HostAuthoritative { snapshot_hz: 15 },
        seats: vec![],
    });
    net.clients[1].send(start.clone());
    net.clients[0].send(start.clone());
    net.step();
    assert_eq!(net.clients[1].rejects, vec![RejectReason::NotHost]);
    assert_eq!(net.clients[0].rejects, vec![RejectReason::NotReady]);
    net.clients[1].send(Message::SetReady(true));
    net.step();
    net.clients[0].send(start);
    net.step();
    let s = net.clients[1].start.clone().expect("started");
    assert_eq!(s.seats, vec![net.clients[0].me, net.clients[1].me]);
    // Once started, the room admits no one.
    net.clients[2].send(hello("late", RoomRequest::Join(code)));
    net.step();
    assert_eq!(net.clients[2].rejects, vec![RejectReason::InGame]);
}

#[test]
fn host_migrates_in_the_lobby() {
    let mut net = Net::new(3, instant(), 7);
    let code = create(&mut net, 3);
    for i in 1..3 {
        net.clients[i].send(hello("p", RoomRequest::Join(code)));
    }
    net.step();
    net.clients[0].send(Message::Leave);
    net.step();
    net.step();
    let next = net.clients[1].me.unwrap();
    assert!(
        net.clients[1]
            .inbox
            .contains(&Message::HostChanged { host: next })
    );
    assert_eq!(net.clients[2].lobby.as_ref().unwrap().host, next);
    assert!(net.clients[0].closed);
    // The new host may now reseat people.
    let third = net.clients[2].me.unwrap();
    net.clients[1].send(Message::AssignSeat {
        player: third,
        seat: None,
    });
    net.step();
    let lobby = net.clients[2].lobby.clone().unwrap();
    assert_eq!(
        lobby.players.iter().find(|p| p.id == third).unwrap().seat,
        None
    );
}

#[test]
fn chat_is_stamped_with_the_real_sender() {
    let mut net = Net::new(2, instant(), 8);
    let code = create(&mut net, 2);
    net.clients[1].send(hello("a", RoomRequest::Join(code)));
    net.step();
    let host = net.clients[0].me.unwrap();
    net.clients[1].send(Message::Chat {
        from: host, // spoof attempt
        text: "hi\u{7}".into(),
    });
    net.step();
    let got = net.clients[0]
        .inbox
        .iter()
        .find_map(|m| match m {
            Message::Chat { from, text } => Some((*from, text.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(got, (net.clients[1].me.unwrap(), "hi".into()));
}

#[test]
fn input_seats_are_stamped_by_the_relay() {
    let mut net = Net::new(2, instant(), 9);
    net.open_room(
        2,
        SimModel::Lockstep {
            input_delay: 2,
            checkpoint_interval: 0,
        },
    );
    net.clients[1].send(Message::Input(InputPacket {
        seat: 0, // claims the host's seat
        first_frame: 2,
        pads: vec![1],
    }));
    net.step();
    let seat = net.clients[0]
        .inbox
        .iter()
        .find_map(|m| match m {
            Message::Input(p) => Some(p.seat),
            _ => None,
        })
        .unwrap();
    assert_eq!(seat, 1);
}

#[test]
fn a_flooding_connection_is_cut_off() {
    let limits = RelayLimits {
        messages_per_sec: 10,
        burst: 20,
        ..RelayLimits::default()
    };
    let mut core = RelayCore::new(limits, 1);
    let conn = ConnId(1);
    core.connect(conn, 0);
    for nonce in 0..21 {
        core.handle(conn, Message::Ping { nonce }, 0);
    }
    let out = core.drain();
    assert!(out.contains(&RelayOutput::Send(
        conn,
        Message::Reject(RejectReason::RateLimited)
    )));
    assert_eq!(out.last(), Some(&RelayOutput::Close(conn)));
    assert_eq!(core.connection_count(), 0);

    // The sustained rate refills: 10 a second is fine forever.
    let mut core = RelayCore::new(
        RelayLimits {
            messages_per_sec: 10,
            burst: 20,
            ..RelayLimits::default()
        },
        1,
    );
    core.connect(conn, 0);
    for i in 0..1_000u64 {
        core.handle(conn, Message::Ping { nonce: i as u32 }, i * 100);
    }
    assert_eq!(core.connection_count(), 1);
}

#[test]
fn connection_and_room_caps_hold() {
    let limits = RelayLimits {
        max_connections: 2,
        max_rooms: 1,
        ..RelayLimits::default()
    };
    let mut core = RelayCore::new(limits, 1);
    assert!(core.connect(ConnId(1), 0));
    assert!(core.connect(ConnId(2), 0));
    assert!(!core.connect(ConnId(3), 0));
    for c in [1, 2] {
        core.handle(ConnId(c), hello("h", RoomRequest::Create { seats: 2 }), 0);
    }
    assert_eq!(core.room_count(), 1);
    let out = core.drain();
    assert!(out.contains(&RelayOutput::Send(
        ConnId(2),
        Message::Reject(RejectReason::ServerFull)
    )));
}

#[test]
fn an_empty_room_is_freed() {
    let mut core = RelayCore::new(RelayLimits::default(), 1);
    core.connect(ConnId(1), 0);
    core.handle(ConnId(1), hello("h", RoomRequest::Create { seats: 2 }), 0);
    assert_eq!(core.room_count(), 1);
    core.disconnect(ConnId(1));
    assert_eq!(core.room_count(), 0);
    assert_eq!(core.connection_count(), 0);
}

#[test]
fn an_ended_session_returns_the_room_to_its_lobby() {
    let mut net = Net::new(3, instant(), 11);
    let start = net.open_room(
        2,
        SimModel::Lockstep {
            input_delay: 2,
            checkpoint_interval: 30,
        },
    );
    assert_eq!(start.seats.len(), 2);
    // Anyone at the table may end it; the relay stamps the sender.
    net.clients[1].send(Message::EndSession { by: PlayerId(99) });
    net.step();
    let me1 = net.clients[1].me.unwrap();
    assert!(
        net.clients[0]
            .inbox
            .contains(&Message::EndSession { by: me1 })
    );
    let lobby = net.clients[0].lobby.clone().unwrap();
    assert!(
        lobby.players.iter().all(|p| !p.ready),
        "readiness is cleared"
    );
    // Back in the lobby: guests' actions reach the host (roaming reports),
    // the host's snapshots reach the room, and the room starts again.
    net.clients[1].send(Message::Action {
        from: PlayerId(0),
        payload: vec![1, 2, 3],
    });
    net.clients[0].send(Message::Snapshot {
        tick: 5,
        avatars: vec![AvatarState::default()],
    });
    net.step();
    assert!(net.clients[0].inbox.contains(&Message::Action {
        from: me1,
        payload: vec![1, 2, 3]
    }));
    assert!(
        net.clients[2]
            .inbox
            .iter()
            .any(|m| matches!(m, Message::Snapshot { tick: 5, .. }))
    );
    // The host's events reach the room in the lobby as well (trades).
    net.clients[0].send(Message::Event {
        seq: 1,
        seat: 0,
        payload: vec![9],
        hash_after: 0,
    });
    net.step();
    assert!(
        net.clients[1]
            .inbox
            .iter()
            .any(|m| matches!(m, Message::Event { seq: 1, .. }))
    );
    net.clients[1].send(Message::SetReady(true));
    net.clients[2].send(Message::SetReady(true));
    net.step();
    net.clients[0].start = None;
    net.clients[0].send(Message::Start(StartGame {
        seed: 9,
        mode: "again".into(),
        config: vec![],
        model: SimModel::HostAuthoritative { snapshot_hz: 15 },
        seats: vec![],
    }));
    net.step();
    assert_eq!(net.clients[0].start.as_ref().map(|s| s.seed), Some(9));
}
