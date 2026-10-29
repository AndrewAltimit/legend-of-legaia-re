//! The threaded relay on an ephemeral 127.0.0.1 port: a host and a guest
//! over real TCP sockets play poker frames; every wait has a deadline and
//! the port is closed after shutdown.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use legaia_engine_netplay::room::NetPlay;
use legaia_netplay::relay::server::RelayServer;
use legaia_netplay::transport::stream::StreamTransport;

use crate::harness::{Peer, SCENE, field_world};

fn frame(p: &mut Peer, pad: u16) {
    let w = p.net.frame(&mut p.world, SCENE, pad);
    p.world.input.set_pad(w);
    p.world.tick();
}

fn run_until(peers: &mut [Peer], secs: u64, mut done: impl FnMut(&[Peer]) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if done(peers) {
            return true;
        }
        for p in peers.iter_mut() {
            frame(p, 0);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    done(peers)
}

#[test]
fn host_and_guest_play_over_real_sockets() {
    let relay = RelayServer::spawn("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = relay.local_addr();
    let t = Duration::from_secs(3);
    let host_link = StreamTransport::connect_tcp(addr, t).expect("host connects");
    let mut peers = vec![Peer {
        world: field_world(),
        net: NetPlay::host(Box::new(host_link), "Host", None),
    }];
    assert!(
        run_until(&mut peers, 5, |p| p[0].net.room.is_some()),
        "a room"
    );
    let code = peers[0].net.room.unwrap();
    let guest_link = StreamTransport::connect_tcp(addr, t).expect("guest connects");
    peers.push(Peer {
        world: field_world(),
        net: NetPlay::join(Box::new(guest_link), "Guest", code, None),
    });
    assert!(
        run_until(&mut peers, 5, |p| p[1].net.in_lobby()),
        "the guest joins"
    );
    peers[1].net.set_ready(true);
    assert!(run_until(&mut peers, 5, |p| p[0]
        .net
        .lobby
        .as_ref()
        .is_some_and(|l| l.players.iter().any(|x| x.ready))));
    let entry = peers[0]
        .world
        .pack_mode_entries()
        .into_iter()
        .find(|e| e.id == "builtin-poker")
        .unwrap();
    let host = &mut peers[0];
    host.net.request_start(&host.world, &entry);
    assert!(
        run_until(&mut peers, 5, |p| p.iter().all(|x| x.net.in_mode())),
        "both enter the table"
    );
    assert!(
        run_until(&mut peers, 5, |p| p[1].mode().is_some_and(|m| {
            match m.view() {
                legaia_engine_modes::active::ModeView::Poker(v) => {
                    v.seats[m.local_seat].hole.is_some()
                }
                _ => false,
            }
        })),
        "the guest draws the host's frames (its own hole cards dealt)"
    );
    for p in &mut peers {
        p.net.leave(&mut p.world);
    }
    drop(peers);
    relay.shutdown();
    assert!(
        TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err(),
        "nothing is left listening on {addr}"
    );
}
