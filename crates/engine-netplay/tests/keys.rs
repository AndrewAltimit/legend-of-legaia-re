//! A key press in a room: one table, `NetPlay::room_key`, that both play hosts
//! ask.

use legaia_engine_netplay::room::RoomKey;

use crate::harness::{Room, lan};

#[test]
fn a_room_reads_a_key_the_same_on_every_host() {
    let mut room = Room::open(2, lan(), 1);
    for _ in 0..30 {
        room.step();
    }
    let net = &mut room.peers[1].net;
    net.screen_open = false;

    // An unbound number key is a reaction; one a pad button holds is the pad's.
    assert_eq!(net.room_key("Digit3", false), Some(RoomKey::Reaction(2)));
    assert_eq!(net.room_key("Digit1", true), None);

    // Tab and backquote flip the room screen.
    assert_eq!(net.room_key("Tab", false), Some(RoomKey::ToggleScreen));
    assert_eq!(
        net.room_key("Backquote", false),
        Some(RoomKey::ToggleScreen)
    );

    // `T` is the camera key until the room screen is up, then the chat line's.
    assert_eq!(net.room_key("KeyT", false), None);
    net.screen_open = true;
    assert_eq!(net.room_key("KeyT", false), Some(RoomKey::BeginChat));

    // A line being typed keeps its letters and digits.
    net.begin_chat();
    assert_eq!(net.room_key("KeyT", false), None);
    assert_eq!(net.room_key("Digit3", false), None);

    // No other key is the room's.
    assert_eq!(net.room_key("KeyZ", false), None);
}
