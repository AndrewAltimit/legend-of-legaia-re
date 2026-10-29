//! Real sockets on 127.0.0.1: the threaded relay on an ephemeral port, one
//! raw-TCP client and one WebSocket client playing a short lockstep
//! session. Every wait has a deadline, and the relay is shut down (and its
//! port checked closed) before the test returns.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use legaia_netplay::lockstep::{LockstepConfig, LockstepSession, Simulation};
use legaia_netplay::protocol::*;
use legaia_netplay::relay::RelayLimits;
use legaia_netplay::relay::server::{RelayServer, ServerConfig};
use legaia_netplay::transport::stream::StreamTransport;
use legaia_netplay::transport::{NetError, Transport};

use crate::harness::{Toy, hello, rng};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Polls `t` until a message matches `want`, failing after `TIMEOUT`.
fn wait_for(t: &mut impl Transport, mut want: impl FnMut(&Message) -> bool) -> Message {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match t.try_recv() {
            Ok(Some(m)) if want(&m) => return m,
            Ok(Some(_)) => {}
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => panic!("transport failed while waiting: {e}"),
        }
    }
    panic!("timed out waiting for a message");
}

fn ws(addr: SocketAddr) -> StreamTransport {
    StreamTransport::connect_ws(addr, &addr.to_string(), "/legaia", TIMEOUT).expect("ws connect")
}

#[test]
fn tcp_and_websocket_clients_play_lockstep_through_the_relay() {
    // The loop below runs frames as fast as the sockets allow, far above
    // 60 Hz, so the per-connection message rate is lifted for it.
    let config = ServerConfig {
        limits: RelayLimits {
            messages_per_sec: 1_000_000,
            burst: 1_000_000,
            ..RelayLimits::default()
        },
        ..ServerConfig::default()
    };
    let relay = RelayServer::spawn_with("127.0.0.1:0", config).expect("bind");
    let addr = relay.local_addr();

    let mut a = StreamTransport::connect_tcp(addr, TIMEOUT).expect("tcp connect");
    let mut b = ws(addr);

    a.send(&hello("tcp", RoomRequest::Create { seats: 2 }))
        .unwrap();
    let Message::Welcome { room, you: a_id } =
        wait_for(&mut a, |m| matches!(m, Message::Welcome { .. }))
    else {
        unreachable!()
    };
    b.send(&hello("ws", RoomRequest::Join(room))).unwrap();
    let Message::Welcome { you: b_id, .. } =
        wait_for(&mut b, |m| matches!(m, Message::Welcome { .. }))
    else {
        unreachable!()
    };
    b.send(&Message::SetReady(true)).unwrap();
    wait_for(
        &mut a,
        |m| matches!(m, Message::Lobby(l) if l.players.len() == 2 && l.players[1].ready),
    );

    // Ping is answered over both framings.
    b.send(&Message::Ping { nonce: 7 }).unwrap();
    wait_for(&mut b, |m| *m == Message::Pong { nonce: 7 });

    a.send(&Message::Start(StartGame {
        seed: 9,
        mode: "toy".into(),
        config: vec![],
        model: SimModel::Lockstep {
            input_delay: 3,
            checkpoint_interval: 20,
        },
        seats: vec![],
    }))
    .unwrap();
    let starts: Vec<StartGame> = [&mut a, &mut b]
        .into_iter()
        .map(|t| match wait_for(t, |m| matches!(m, Message::Start(_))) {
            Message::Start(s) => s,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(starts[0], starts[1]);

    let mut sessions = [
        LockstepSession::new(
            Toy::default(),
            LockstepConfig::from_start(&starts[0], a_id).unwrap(),
        ),
        LockstepSession::new(
            Toy::default(),
            LockstepConfig::from_start(&starts[1], b_id).unwrap(),
        ),
    ];
    let mut transports = [a, b];
    let mut pads = [1u64, 2u64];
    const FRAMES: u32 = 600;
    let deadline = Instant::now() + Duration::from_secs(20);
    while sessions.iter().any(|s| s.frame() < FRAMES) {
        assert!(Instant::now() < deadline, "lockstep over sockets stalled");
        for i in 0..2 {
            let s = &mut sessions[i];
            if s.frame() < FRAMES {
                if s.wants_local_input() {
                    s.push_local_input(rng(&mut pads[i]) as u16);
                }
                s.advance();
            }
            s.pump(&mut transports[i]).expect("pump");
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    assert_eq!(
        sessions[0].sim().state_hash(),
        sessions[1].sim().state_hash()
    );
    assert!(
        sessions
            .iter()
            .all(|s| s.desync().is_none() && s.verified_checkpoints() >= 25)
    );

    relay.shutdown();
    // Clients learn the room is gone, then the socket closes.
    for t in &mut transports {
        let deadline = Instant::now() + TIMEOUT;
        let mut saw_close = false;
        loop {
            assert!(Instant::now() < deadline, "client never saw the shutdown");
            match t.try_recv() {
                Ok(Some(Message::RoomClosed(CloseReason::ServerShutdown))) => saw_close = true,
                Ok(_) => std::thread::sleep(Duration::from_millis(1)),
                Err(NetError::Closed) | Err(NetError::Io(_)) => break,
                Err(e) => panic!("{e}"),
            }
        }
        assert!(saw_close);
    }
    assert!(
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err(),
        "listener left behind"
    );
}

#[test]
fn non_upgrade_http_and_garbage_are_turned_away() {
    let relay = RelayServer::spawn("127.0.0.1:0").expect("bind");
    let addr = relay.local_addr();

    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT).unwrap();
    s.set_read_timeout(Some(TIMEOUT)).unwrap();
    s.write_all(b"GET /etc/passwd HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    assert!(
        resp.starts_with(b"HTTP/1.1 400"),
        "{:?}",
        String::from_utf8_lossy(&resp)
    );

    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT).unwrap();
    s.set_read_timeout(Some(TIMEOUT)).unwrap();
    s.write_all(b"\x00\x01\x02\x03hello").unwrap();
    let mut resp = Vec::new();
    let n = s.read_to_end(&mut resp).unwrap_or(0);
    assert_eq!(n, 0, "garbage preamble got a reply");

    // A message with no meaning from a client is refused; the connection
    // stays.
    let mut t = StreamTransport::connect_tcp(addr, TIMEOUT).unwrap();
    t.send(&Message::Reject(RejectReason::Unexpected)).unwrap();
    wait_for(&mut t, |m| *m == Message::Reject(RejectReason::Unexpected));
    t.send(&Message::Ping { nonce: 1 }).unwrap();
    wait_for(&mut t, |m| *m == Message::Pong { nonce: 1 });

    // A frame that does not decode is rejected and the connection dropped.
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT).unwrap();
    s.set_read_timeout(Some(TIMEOUT)).unwrap();
    s.write_all(b"LGNP\x01\x00\x00\x00\xEE").unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let reject = Message::Reject(RejectReason::Malformed).encode();
    assert_eq!(&resp[4..], &reject[..], "{resp:?}");

    relay.shutdown();
}

#[test]
fn dropping_the_handle_stops_the_relay() {
    let addr = {
        let relay = RelayServer::spawn("127.0.0.1:0").expect("bind");
        let _client = StreamTransport::connect_tcp(relay.local_addr(), TIMEOUT).unwrap();
        relay.local_addr()
    };
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}
