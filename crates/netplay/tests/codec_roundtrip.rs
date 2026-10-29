//! Every message variant survives encode -> decode, every strict prefix of
//! an encoding is refused, and arbitrary bytes never panic the decoder.

use legaia_netplay::protocol::*;

pub fn every_message() -> Vec<Message> {
    let code = RoomCode::parse("ab23cd").unwrap();
    vec![
        Message::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            engine_build: "legaia-engine 0.1.0+abc".into(),
            player_name: "Vahn".into(),
            pack: Some(PackIdentity {
                id: "rebirth".into(),
                content_hash: [0xA5; 32],
            }),
            room: RoomRequest::Create { seats: 4 },
        }),
        Message::Hello(Hello {
            protocol: 9,
            engine_build: String::new(),
            player_name: "Noa".into(),
            pack: None,
            room: RoomRequest::Join(code),
        }),
        Message::Welcome {
            room: code,
            you: PlayerId(300),
        },
        Message::Reject(RejectReason::ProtocolMismatch { server: 7 }),
        Message::Reject(RejectReason::Unexpected),
        Message::Ping { nonce: u32::MAX },
        Message::Pong { nonce: 0 },
        Message::Lobby(LobbyState {
            host: PlayerId(1),
            seats: 4,
            players: vec![
                LobbyPlayer {
                    id: PlayerId(1),
                    name: "Gala".into(),
                    seat: Some(0),
                    ready: true,
                },
                LobbyPlayer {
                    id: PlayerId(2),
                    name: "Spectator".into(),
                    seat: None,
                    ready: false,
                },
            ],
        }),
        Message::SetReady(true),
        Message::AssignSeat {
            player: PlayerId(2),
            seat: None,
        },
        Message::Chat {
            from: PlayerId(2),
            text: "gl hf - ünïcødé".into(),
        },
        Message::Leave,
        Message::PlayerLeft {
            player: PlayerId(3),
            reason: LeaveReason::Disconnected,
        },
        Message::HostChanged { host: PlayerId(2) },
        Message::RoomClosed(CloseReason::HostLeft),
        Message::Start(StartGame {
            seed: 0xDEAD_BEEF_0123_4567,
            mode: "poker".into(),
            config: vec![1, 2, 3],
            model: SimModel::Lockstep {
                input_delay: 4,
                checkpoint_interval: 600,
            },
            seats: vec![Some(PlayerId(1)), None, Some(PlayerId(5))],
        }),
        Message::Start(StartGame {
            seed: 1,
            mode: "roam".into(),
            config: vec![],
            model: SimModel::HostAuthoritative { snapshot_hz: 15 },
            seats: vec![],
        }),
        Message::Input(InputPacket {
            seat: 3,
            first_frame: 1_000_000,
            pads: (0..MAX_REDUNDANCY as u16).map(|i| i * 0x0101).collect(),
        }),
        Message::Checkpoint {
            seat: 1,
            frame: 59,
            hash: u64::MAX,
        },
        Message::SeatController {
            seat: 2,
            controller: Controller::Ai,
            from_frame: 77,
        },
        Message::SeatController {
            seat: 0,
            controller: Controller::Human(PlayerId(9)),
            from_frame: 0,
        },
        Message::Pause { by: PlayerId(1) },
        Message::Resume { by: PlayerId(1) },
        Message::Action {
            from: PlayerId(4),
            payload: vec![0xFF; 100],
        },
        Message::Event {
            seq: 12,
            seat: 1,
            payload: vec![7],
            hash_after: 99,
        },
        Message::StateRequest { from: PlayerId(4) },
        Message::StateBlob {
            to: PlayerId(4),
            seq: 12,
            bytes: vec![0; MAX_PAYLOAD_LEN],
        },
        Message::Snapshot {
            tick: 600,
            avatars: vec![AvatarState {
                player: PlayerId(2),
                scene: 17,
                pos: [-1000, 0, i32::MAX],
                facing: 4095,
                anim: 3,
                anim_frame: 11,
            }],
        },
    ]
}

#[test]
fn every_variant_round_trips() {
    for msg in every_message() {
        let bytes = msg.encode();
        assert!(bytes.len() <= MAX_MESSAGE_LEN, "{msg:?} over the cap");
        assert_eq!(Message::decode(&bytes).unwrap(), msg);
    }
}

#[test]
fn every_strict_prefix_is_refused() {
    for msg in every_message() {
        let bytes = msg.encode();
        for len in 0..bytes.len() {
            assert!(
                Message::decode(&bytes[..len]).is_err(),
                "{len}-byte prefix of {msg:?} decoded"
            );
        }
        let mut long = bytes.clone();
        long.push(0);
        assert!(Message::decode(&long).is_err(), "trailing byte accepted");
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut x = 0x1234_5678_9ABC_DEF1u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..20_000 {
        let len = (next() % 64) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Bias toward real tags so the field decoders get exercised.
        if let Some(b) = bytes.first_mut() {
            *b = [0x01, 0x10, 0x20, 0x21, 0x31, 0x34][(*b % 6) as usize];
        }
        let _ = Message::decode(&bytes);
    }
}

#[test]
fn room_codes_parse_case_insensitively_and_refuse_lookalikes() {
    assert_eq!(RoomCode::parse(" ab23cd ").unwrap().as_str(), "AB23CD");
    assert!(
        RoomCode::parse("AB23C0").is_none(),
        "0 is not in the alphabet"
    );
    assert!(
        RoomCode::parse("AB23CI").is_none(),
        "I is not in the alphabet"
    );
    assert!(RoomCode::parse("AB23C").is_none());
    let code = RoomCode::from_bits(0x3FFF_FFFF);
    assert_eq!(RoomCode::parse(code.as_str()), Some(code));
}
