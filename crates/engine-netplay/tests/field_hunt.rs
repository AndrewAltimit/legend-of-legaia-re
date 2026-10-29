//! Prop hunt in the field: two worlds standing in one scene, the host's
//! session fed from both players' real field positions, tags by Cross next
//! to a prop, a decoy's penalty, disguises, and the match ending back in
//! the lobby.

use legaia_engine_modes::prop_hunt::{HuntPhase, Role};
use legaia_engine_netplay::field_hunt::{FIELD_HUNT_ID, FieldProp, to_session};
use legaia_engine_system::pad::PadButton;
use legaia_netplay::protocol::PlayerId;

use crate::harness::{Room, lan};

const CROSS: u16 = PadButton::Cross as u16;

/// A row of barrels (slot 5) and two carts (slot 9) across a town square.
fn props() -> Vec<FieldProp> {
    let mut out: Vec<FieldProp> = (0..6)
        .map(|i| FieldProp {
            slot: 5,
            pos: [i * 400, 0, 0],
            radius: 48,
        })
        .collect();
    out.push(FieldProp {
        slot: 9,
        pos: [0, 0, 1600],
        radius: 120,
    });
    out.push(FieldProp {
        slot: 9,
        pos: [1600, 0, 1600],
        radius: 120,
    });
    out
}

fn place(room: &mut Room, peer: usize, x: i32, z: i32) {
    let m = &mut room.peers[peer].world.actors[0].move_state;
    m.world_x = x as i16;
    m.world_z = z as i16;
}

/// The peer seated in session seat `seat`.
fn peer_of(room: &Room, seats: &[PlayerId], seat: usize) -> usize {
    room.peers
        .iter()
        .position(|p| p.net.me == Some(seats[seat]))
        .unwrap()
}

#[test]
fn a_field_prop_hunt_plays_on_real_positions() {
    let mut room = Room::open(2, lan(), 31);
    room.peers[0].net.set_scene_props(props());
    room.start(FIELD_HUNT_ID);
    // The world stays in the field: nothing took the mode seat.
    for p in &room.peers {
        assert!(p.world.expansion.mode_seat.is_none());
    }
    assert!(
        room.run_until(60, |r| r.peers[1].net.hunt_hud().is_some()),
        "the guest draws the hunt"
    );

    // Hiders stand off to the side; everyone takes their first prop.
    let mut frame = 0u32;
    let mut decoy_hit = false;
    let mut found = 0;
    let mut disguised_seen = false;
    while frame < 60 * 60 * 8 {
        let (view, seats) = {
            let (v, s) = room.peers[0]
                .net
                .hunt_view()
                .expect("the host runs the hunt");
            (v.clone(), s.to_vec())
        };
        if view.phase == HuntPhase::Finished {
            break;
        }
        let hunter = view
            .avatars
            .iter()
            .position(|a| a.role == Role::Hunter)
            .unwrap();
        let hider = view
            .avatars
            .iter()
            .position(|a| a.role == Role::Hider)
            .unwrap();
        let (hp, dp) = (
            peer_of(&room, &seats, hunter),
            peer_of(&room, &seats, hider),
        );
        place(&mut room, dp, 800, 800);
        let hunting = matches!(view.phase, HuntPhase::Hunt { .. });
        // The hunter first tags a barrel (a decoy), then walks onto the hider.
        if hunting && !decoy_hit {
            place(&mut room, hp, 0, 0);
        } else if hunting {
            place(&mut room, hp, 800, 800);
        } else {
            place(&mut room, hp, -800, -800);
        }
        if matches!(view.phase, HuntPhase::Hide { .. }) {
            disguised_seen |= room.peers[hp].net.disguise_of(seats[hider]).is_some()
                && room.peers[dp].net.self_disguise().is_some();
        }
        let tap = frame.is_multiple_of(12);
        room.step_with(&mut |i, _| {
            let choosing = matches!(view.phase, HuntPhase::Choose { .. });
            if tap && (choosing || (hunting && i == hp)) {
                CROSS
            } else {
                0
            }
        });
        frame += 1;
        let (after, _) = room.peers[0]
            .net
            .hunt_view()
            .map(|(v, s)| (v.clone(), s))
            .unwrap();
        let h = &after.avatars[hunter];
        if hunting && !decoy_hit && h.health < view.avatars[hunter].health {
            decoy_hit = true;
        }
        if after.avatars[hider].out && !view.avatars[hider].out {
            found += 1;
        }
        // The session reads the hider where its own world put it.
        if matches!(after.phase, HuntPhase::Hunt { .. }) && !after.avatars[hider].out {
            assert_eq!(after.avatars[hider].pos, to_session([800, 0, 800]));
        }
    }
    assert!(decoy_hit, "tagging a decoy costs the hunter health");
    assert!(found >= 1, "the hunter found the hider");
    assert!(disguised_seen, "both worlds draw the hider as its prop");
    // The match ends and the room returns to its lobby.
    assert!(room.run_until(60 * 10, |r| r.peers.iter().all(|p| p.net.in_lobby())));
    assert!(
        room.peers[1]
            .net
            .chat
            .iter()
            .any(|l| l.contains("Prop hunt"))
    );
}

#[test]
fn the_world_sees_only_movement_during_a_hunt() {
    let mut room = Room::open(2, lan(), 32);
    room.peers[0].net.set_scene_props(props());
    room.start(FIELD_HUNT_ID);
    assert!(room.run_until(60, |r| r.peers.iter().all(|p| p.net.hunt_hud().is_some())));
    let up = PadButton::Up as u16;
    for p in &room.peers {
        let f = p.net.world_pad_filter();
        assert_eq!(f & CROSS, 0, "Cross tags; it never talks to a villager");
        // Everyone picks a prop standing still.
        assert_eq!(f & up, 0);
    }
}
