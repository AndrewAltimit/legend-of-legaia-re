//! Shared-world roaming: a guest walking its own world moves its avatar on
//! the host's, interpolated from the snapshots.

use crate::harness::{Room, SCENE, lan};

#[test]
fn a_guest_walking_moves_its_avatar_on_the_host() {
    let mut room = Room::open(2, lan(), 7);
    let guest_id = room.peers[1].net.me.unwrap();
    let mut seen = Vec::new();
    for f in 0..240i16 {
        room.peers[1].world.actors[0].move_state.world_x = f * 8;
        room.peers[1].world.actors[0].move_state.render_26 = 1024;
        room.step();
        if let Some(a) = room.peers[0]
            .net
            .remote_avatars()
            .into_iter()
            .find(|a| a.player == guest_id)
        {
            assert_eq!(a.scene, SCENE);
            assert_eq!(a.facing, 1024);
            seen.push(a.pos[0]);
        }
    }
    assert!(seen.len() > 150, "the host sees the guest ({})", seen.len());
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "it only walks forward"
    );
    let last = *seen.last().unwrap();
    assert!(
        last > 1400 && last < 240 * 8,
        "it trails the guest a little ({last})"
    );
    // The guest sees the host too, standing at the origin.
    let host_id = room.peers[0].net.me.unwrap();
    assert!(
        room.peers[1]
            .net
            .remote_avatars()
            .iter()
            .any(|a| a.player == host_id && a.pos == [0, 0, 0])
    );
    // A player in another scene is not drawn.
    assert!(
        room.peers[1]
            .net
            .remote_avatars()
            .iter()
            .all(|a| a.player != guest_id)
    );
}

#[test]
fn chat_and_emotes_reach_the_room() {
    let mut room = Room::open(2, lan(), 8);
    room.peers[1].net.say("hello there");
    room.peers[0].net.emote(0);
    for _ in 0..20 {
        room.step();
    }
    for p in &room.peers {
        assert!(
            p.net.chat.iter().any(|l| l == "Guest1: hello there"),
            "{:?}",
            p.net.chat
        );
        assert!(
            p.net.chat.iter().any(|l| l == "* Host waves"),
            "{:?}",
            p.net.chat
        );
    }
    let strings = legaia_engine_netplay::screens::net_overlay_strings(
        &room.peers[0].net,
        &room.peers[0].world,
    );
    assert!(
        strings.iter().any(|l| l.starts_with("Room ")),
        "{strings:?}"
    );
}
