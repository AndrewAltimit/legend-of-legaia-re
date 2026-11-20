//! The room's build check keys on the release: two peers of one release
//! share a room, two releases are refused with both named, and development
//! builds (no release baked in) join each other but no release build.

use legaia_engine_netplay::room::{NetPlay, build_label, engine_build, engine_build_for};
use legaia_netplay::relay::RelayLimits;
use legaia_netplay::relay::local::LocalRelay;
use legaia_netplay::transport::loopback::{self, SimClock};

use crate::harness::{Peer, SCENE, field_world, lan};

/// A host speaking `host`, then a guest speaking `guest` joining its room;
/// steps until the guest is in the lobby or refused. The guest's peer comes
/// back.
fn host_then_join(host: &str, guest: &str) -> Peer {
    let clock = SimClock::new();
    let mut relay = LocalRelay::new(RelayLimits::default(), 7);
    let mut peers: Vec<Peer> = Vec::new();
    let (client, server) = loopback::link(&clock, lan(), lan(), 7);
    relay.add(server, 0);
    peers.push(Peer {
        world: field_world(),
        net: NetPlay::host(Box::new(client), "Host", None).with_engine_build(host.into()),
    });
    let step = |peers: &mut Vec<Peer>, relay: &mut LocalRelay<_>| {
        clock.advance(1);
        relay.pump(clock.now() * 16);
        for p in peers.iter_mut() {
            let pad = p.net.frame(&mut p.world, SCENE, 0);
            p.world.input.set_pad(pad);
            p.world.tick();
        }
    };
    for _ in 0..200 {
        if peers[0].net.room.is_some() {
            break;
        }
        step(&mut peers, &mut relay);
    }
    let code = peers[0].net.room.expect("the host's room opens");
    let (client, server) = loopback::link(&clock, lan(), lan(), 38);
    relay.add(server, clock.now() * 16);
    peers.push(Peer {
        world: field_world(),
        net: NetPlay::join(Box::new(client), "Guest", code, None).with_engine_build(guest.into()),
    });
    for _ in 0..400 {
        if peers[1].net.in_lobby() || peers[1].net.notice.is_some() {
            break;
        }
        step(&mut peers, &mut relay);
    }
    peers.pop().unwrap()
}

#[test]
fn one_release_shares_a_room() {
    let v = engine_build_for(Some("1.30.0"));
    let guest = host_then_join(&v, &v);
    assert!(guest.net.in_lobby(), "refused: {:?}", guest.net.notice);
}

#[test]
fn another_release_is_refused_with_both_named() {
    let guest = host_then_join(
        &engine_build_for(Some("1.29.0")),
        &engine_build_for(Some("1.30.0")),
    );
    assert!(!guest.net.in_lobby());
    let notice = guest.net.notice.expect("a refusal notice");
    assert!(notice.contains("release 1.29.0"), "{notice}");
    assert!(notice.contains("release 1.30.0"), "{notice}");
}

#[test]
fn development_builds_join_each_other_and_no_release() {
    let dev = engine_build_for(None);
    assert!(dev.ends_with("-dev"), "{dev}");
    let guest = host_then_join(&dev, &dev);
    assert!(guest.net.in_lobby(), "refused: {:?}", guest.net.notice);

    let guest = host_then_join(&engine_build_for(Some("1.30.0")), &dev);
    assert!(!guest.net.in_lobby());
    let notice = guest.net.notice.expect("a refusal notice");
    assert!(notice.contains("release 1.30.0"), "{notice}");
    assert!(notice.contains("a development build"), "{notice}");
}

#[test]
fn this_build_is_a_release_or_a_development_build() {
    match legaia_release_version::RELEASE {
        Some(v) => assert_eq!(build_label(&engine_build()), format!("release {v}")),
        None => assert_eq!(engine_build(), engine_build_for(None)),
    }
}
