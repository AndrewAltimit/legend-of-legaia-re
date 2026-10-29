//! The wire protocol: every message a client, the relay and a hosting
//! player exchange, and its binary codec.
//!
//! One enum carries both directions. A message is a one-byte tag followed by
//! its fields (see [`crate::codec`]); the transport frames it (a length
//! prefix on TCP, one binary frame on WebSocket). The protocol version is
//! negotiated once, in [`Hello`]; a relay rejects a client whose version is
//! not its own, so no later message carries it.

use std::fmt;

use crate::codec::{CodecError, Reader, Writer};

/// The protocol revision this build speaks. Bumped on any change to a
/// message's layout or meaning.
pub const PROTOCOL_VERSION: u16 = 2;
/// Largest encoded message, tag included. Transports refuse a frame over it.
pub const MAX_MESSAGE_LEN: usize = 16 * 1024;
/// Player display name, in bytes.
pub const MAX_NAME_LEN: usize = 24;
/// Chat line, in bytes.
pub const MAX_CHAT_LEN: usize = 256;
/// Engine build id, pack id and mode id, in bytes.
pub const MAX_ID_LEN: usize = 64;
/// Opaque mode configuration in [`StartGame`].
pub const MAX_CONFIG_LEN: usize = 4096;
/// Opaque action / event / state payloads in the host-authoritative model.
pub const MAX_PAYLOAD_LEN: usize = 8192;
/// Seats a session can have (players with a pad; spectators have none).
pub const MAX_SEATS: usize = 8;
/// Everyone in a room: seated players and spectators.
pub const MAX_ROOM_MEMBERS: usize = 16;
/// Pad words in one [`InputPacket`] - the redundancy window cap.
pub const MAX_REDUNDANCY: usize = 32;
/// Avatars in one [`Snapshot`].
pub const MAX_AVATARS: usize = MAX_ROOM_MEMBERS;
/// Characters in a room code.
pub const ROOM_CODE_LEN: usize = 6;
/// The room-code alphabet: upper-case letters and digits without the
/// look-alike pairs `0`/`O` and `1`/`I`, so a code read aloud survives.
pub const ROOM_CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A room member, unique within its room for the room's lifetime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// A room's join code: [`ROOM_CODE_LEN`] characters of [`ROOM_CODE_ALPHABET`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoomCode([u8; ROOM_CODE_LEN]);

impl RoomCode {
    /// Parses a code typed by a player: case-insensitive, surrounding
    /// whitespace ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.len() != ROOM_CODE_LEN {
            return None;
        }
        let mut out = [0u8; ROOM_CODE_LEN];
        for (dst, c) in out.iter_mut().zip(text.bytes()) {
            let c = c.to_ascii_uppercase();
            if !ROOM_CODE_ALPHABET.contains(&c) {
                return None;
            }
            *dst = c;
        }
        Some(Self(out))
    }

    /// Builds a code from [`ROOM_CODE_LEN`] five-bit symbols of `bits`.
    pub fn from_bits(mut bits: u64) -> Self {
        let mut out = [0u8; ROOM_CODE_LEN];
        for c in &mut out {
            *c = ROOM_CODE_ALPHABET[(bits & 31) as usize];
            bits >>= 5;
        }
        Self(out)
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("room codes are ASCII")
    }
}

impl fmt::Display for RoomCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which expansion pack a player runs. Everyone in a room must match the
/// host's, byte for byte, or lockstep diverges on the first pack rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackIdentity {
    /// The pack's manifest id (`rebirth`).
    pub id: String,
    /// SHA-256 over the pack's canonical `.lxpack` bundle bytes.
    pub content_hash: [u8; 32],
}

/// Create a room or join one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomRequest {
    /// A new room with `seats` pad seats; the sender becomes its host.
    Create { seats: u8 },
    /// An existing room by code.
    Join(RoomCode),
}

/// The first message on every connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub protocol: u16,
    /// The engine's build id; a room admits only the host's.
    pub engine_build: String,
    pub player_name: String,
    /// `None` = plain retail content, no pack loaded.
    pub pack: Option<PackIdentity>,
    pub room: RoomRequest,
}

/// Why the relay refused a request. Some close the connection (see
/// [`RejectReason::is_fatal`]); the rest only refuse that one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The client speaks another protocol revision; `server` is the relay's.
    ProtocolMismatch {
        server: u16,
    },
    /// Not the host's engine build.
    BuildMismatch,
    /// Not the host's expansion pack (id or content hash).
    PackMismatch,
    NoSuchRoom,
    RoomFull,
    /// The relay is at its room or connection cap.
    ServerFull,
    /// The room's session has started; it admits no one new.
    InGame,
    /// Empty or over-long name, or a bad seat count.
    BadRequest,
    /// Over the message rate, or too many bad room codes.
    RateLimited,
    /// A frame that did not decode.
    Malformed,
    /// A host-only request from a non-host.
    NotHost,
    /// Start requested while a seated player is not ready.
    NotReady,
    /// A message that has no meaning in the connection's current state.
    Unexpected,
}

impl RejectReason {
    /// Whether the relay closes the connection after sending this. The
    /// rest leave the connection open so the client can retry (another
    /// room code, another room).
    pub fn is_fatal(self) -> bool {
        matches!(
            self,
            Self::ProtocolMismatch { .. } | Self::ServerFull | Self::RateLimited | Self::Malformed
        )
    }
}

/// One room member as the lobby shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyPlayer {
    pub id: PlayerId,
    pub name: String,
    /// `None` = spectator.
    pub seat: Option<u8>,
    pub ready: bool,
}

/// The whole lobby, re-sent after every change (it is at most
/// [`MAX_ROOM_MEMBERS`] rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyState {
    pub host: PlayerId,
    pub seats: u8,
    pub players: Vec<LobbyPlayer>,
}

/// How a session is simulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimModel {
    /// Every peer runs the simulation from everyone's pad words
    /// ([`crate::lockstep`]).
    Lockstep {
        /// Frames between sampling a pad and simulating it.
        input_delay: u8,
        /// Frames between state-hash checkpoints.
        checkpoint_interval: u16,
    },
    /// The host simulates; the others send actions and receive events and
    /// avatar snapshots ([`crate::authority`]).
    HostAuthoritative {
        /// Avatar snapshot rate.
        snapshot_hz: u8,
    },
}

/// The host's start order, relayed to everyone with `seats` filled in by
/// the relay from the lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartGame {
    /// Seed for every deterministic RNG in the session.
    pub seed: u64,
    /// The mode to run (an expansion mode id, or a built-in one).
    pub mode: String,
    /// Mode-defined configuration bytes.
    pub config: Vec<u8>,
    pub model: SimModel,
    /// Seat index -> its player; `None` = AI from the first frame.
    pub seats: Vec<Option<PlayerId>>,
}

/// One seat's pad words for frames `first_frame..first_frame + pads.len()`.
/// A sender repeats its last K frames in every packet, so a lost or late
/// packet is covered by the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPacket {
    pub seat: u8,
    pub first_frame: u32,
    pub pads: Vec<u16>,
}

impl InputPacket {
    /// One past the last frame this packet carries.
    pub fn end_frame(&self) -> u32 {
        self.first_frame + self.pads.len() as u32
    }
}

/// Who drives a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    Human(PlayerId),
    /// The mode's own AI (the seat's pad words are ignored).
    Ai,
}

/// Why a member left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    Left,
    Disconnected,
}

/// Why a room ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The host of a host-authoritative session left; nobody else holds
    /// the state.
    HostLeft,
    ServerShutdown,
}

/// One avatar in a shared-world snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AvatarState {
    pub player: PlayerId,
    /// The scene the avatar stands in (a scene index the hosts agree on).
    pub scene: u16,
    /// World position, engine units.
    pub pos: [i32; 3],
    /// Facing, PSX angle units (4096 = one turn).
    pub facing: u16,
    pub anim: u16,
    pub anim_frame: u16,
}

/// Every message of the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    // --- Connection ---
    /// Client -> relay, first message.
    Hello(Hello),
    /// Relay -> client: admitted to `room` as `you`.
    Welcome {
        room: RoomCode,
        you: PlayerId,
    },
    /// Relay -> client.
    Reject(RejectReason),
    /// Either direction; the relay answers a client's with [`Message::Pong`].
    Ping {
        nonce: u32,
    },
    Pong {
        nonce: u32,
    },

    // --- Lobby ---
    /// Relay -> room.
    Lobby(LobbyState),
    /// Client -> relay.
    SetReady(bool),
    /// Host -> relay: move a member to a seat, or to spectating (`None`).
    AssignSeat {
        player: PlayerId,
        seat: Option<u8>,
    },
    /// Both directions; the relay overwrites `from` with the sender.
    Chat {
        from: PlayerId,
        text: String,
    },
    /// Client -> relay: leave the room (then the connection closes).
    Leave,
    /// Relay -> room.
    PlayerLeft {
        player: PlayerId,
        reason: LeaveReason,
    },
    /// Relay -> room: host migration.
    HostChanged {
        host: PlayerId,
    },
    /// Relay -> room: the room is gone.
    RoomClosed(CloseReason),

    // --- Session ---
    /// Host -> relay -> room.
    Start(StartGame),
    /// Peer -> relay -> other peers (lockstep).
    Input(InputPacket),
    /// Peer -> relay -> other peers: the sender's state hash after
    /// simulating `frame`.
    Checkpoint {
        seat: u8,
        frame: u32,
        hash: u64,
    },
    /// Relay -> room: `seat` changes hands from `from_frame` on (a dropped
    /// player's seat goes to the AI).
    SeatController {
        seat: u8,
        controller: Controller,
        from_frame: u32,
    },
    /// Both directions; the relay overwrites `by` with the sender.
    Pause {
        by: PlayerId,
    },
    Resume {
        by: PlayerId,
    },
    /// Peer -> relay -> room: the running session is over and the room
    /// returns to its lobby (readiness cleared, joins admitted again). The
    /// relay overwrites `by` with the sender.
    EndSession {
        by: PlayerId,
    },

    // --- Host-authoritative ---
    /// Client -> relay -> host; the relay overwrites `from`.
    Action {
        from: PlayerId,
        payload: Vec<u8>,
    },
    /// Host -> relay -> room: the result of an action, in host order.
    Event {
        seq: u32,
        seat: u8,
        payload: Vec<u8>,
        hash_after: u64,
    },
    /// Client -> relay -> host: send me the full state.
    StateRequest {
        from: PlayerId,
    },
    /// Host -> relay -> `to`: the state after the first `seq` events.
    StateBlob {
        to: PlayerId,
        seq: u32,
        bytes: Vec<u8>,
    },
    /// Host -> relay -> room: avatar positions for shared-world roaming.
    Snapshot {
        tick: u32,
        avatars: Vec<AvatarState>,
    },
}

mod tag {
    pub const HELLO: u8 = 0x01;
    pub const WELCOME: u8 = 0x02;
    pub const REJECT: u8 = 0x03;
    pub const PING: u8 = 0x04;
    pub const PONG: u8 = 0x05;
    pub const LOBBY: u8 = 0x10;
    pub const SET_READY: u8 = 0x11;
    pub const ASSIGN_SEAT: u8 = 0x12;
    pub const CHAT: u8 = 0x13;
    pub const LEAVE: u8 = 0x14;
    pub const PLAYER_LEFT: u8 = 0x15;
    pub const HOST_CHANGED: u8 = 0x16;
    pub const ROOM_CLOSED: u8 = 0x17;
    pub const START: u8 = 0x20;
    pub const INPUT: u8 = 0x21;
    pub const CHECKPOINT: u8 = 0x22;
    pub const SEAT_CONTROLLER: u8 = 0x23;
    pub const PAUSE: u8 = 0x24;
    pub const RESUME: u8 = 0x25;
    pub const END_SESSION: u8 = 0x26;
    pub const ACTION: u8 = 0x30;
    pub const EVENT: u8 = 0x31;
    pub const STATE_REQUEST: u8 = 0x32;
    pub const STATE_BLOB: u8 = 0x33;
    pub const SNAPSHOT: u8 = 0x34;
}

fn put_player(w: &mut Writer, p: PlayerId) {
    w.varint(u64::from(p.0));
}

fn get_player(r: &mut Reader<'_>) -> Result<PlayerId, CodecError> {
    r.varint_u32().map(PlayerId)
}

fn put_opt_seat(w: &mut Writer, seat: Option<u8>) {
    w.u8(seat.unwrap_or(0xFF));
}

fn get_opt_seat(r: &mut Reader<'_>) -> Result<Option<u8>, CodecError> {
    match r.u8()? {
        0xFF => Ok(None),
        s if usize::from(s) < MAX_SEATS => Ok(Some(s)),
        _ => Err(CodecError::BadValue("seat")),
    }
}

fn get_seat(r: &mut Reader<'_>) -> Result<u8, CodecError> {
    match r.u8()? {
        s if usize::from(s) < MAX_SEATS => Ok(s),
        _ => Err(CodecError::BadValue("seat")),
    }
}

fn reject_code(reason: RejectReason) -> u8 {
    match reason {
        RejectReason::ProtocolMismatch { .. } => 0,
        RejectReason::BuildMismatch => 1,
        RejectReason::PackMismatch => 2,
        RejectReason::NoSuchRoom => 3,
        RejectReason::RoomFull => 4,
        RejectReason::ServerFull => 5,
        RejectReason::InGame => 6,
        RejectReason::BadRequest => 7,
        RejectReason::RateLimited => 8,
        RejectReason::Malformed => 9,
        RejectReason::NotHost => 10,
        RejectReason::NotReady => 11,
        RejectReason::Unexpected => 12,
    }
}

impl Message {
    /// Encodes to the bytes a transport frames.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Self::Hello(h) => {
                w.u8(tag::HELLO);
                w.u16(h.protocol);
                w.str(&h.engine_build);
                w.str(&h.player_name);
                match &h.pack {
                    None => w.u8(0),
                    Some(p) => {
                        w.u8(1);
                        w.str(&p.id);
                        w.raw(&p.content_hash);
                    }
                }
                match h.room {
                    RoomRequest::Create { seats } => {
                        w.u8(0);
                        w.u8(seats);
                    }
                    RoomRequest::Join(code) => {
                        w.u8(1);
                        w.raw(&code.0);
                    }
                }
            }
            Self::Welcome { room, you } => {
                w.u8(tag::WELCOME);
                w.raw(&room.0);
                put_player(&mut w, *you);
            }
            Self::Reject(reason) => {
                w.u8(tag::REJECT);
                w.u8(reject_code(*reason));
                if let RejectReason::ProtocolMismatch { server } = reason {
                    w.u16(*server);
                }
            }
            Self::Ping { nonce } => {
                w.u8(tag::PING);
                w.varint(u64::from(*nonce));
            }
            Self::Pong { nonce } => {
                w.u8(tag::PONG);
                w.varint(u64::from(*nonce));
            }
            Self::Lobby(l) => {
                w.u8(tag::LOBBY);
                put_player(&mut w, l.host);
                w.u8(l.seats);
                w.varint(l.players.len() as u64);
                for p in &l.players {
                    put_player(&mut w, p.id);
                    w.str(&p.name);
                    put_opt_seat(&mut w, p.seat);
                    w.bool(p.ready);
                }
            }
            Self::SetReady(ready) => {
                w.u8(tag::SET_READY);
                w.bool(*ready);
            }
            Self::AssignSeat { player, seat } => {
                w.u8(tag::ASSIGN_SEAT);
                put_player(&mut w, *player);
                put_opt_seat(&mut w, *seat);
            }
            Self::Chat { from, text } => {
                w.u8(tag::CHAT);
                put_player(&mut w, *from);
                w.str(text);
            }
            Self::Leave => w.u8(tag::LEAVE),
            Self::PlayerLeft { player, reason } => {
                w.u8(tag::PLAYER_LEFT);
                put_player(&mut w, *player);
                w.u8(match reason {
                    LeaveReason::Left => 0,
                    LeaveReason::Disconnected => 1,
                });
            }
            Self::HostChanged { host } => {
                w.u8(tag::HOST_CHANGED);
                put_player(&mut w, *host);
            }
            Self::RoomClosed(reason) => {
                w.u8(tag::ROOM_CLOSED);
                w.u8(match reason {
                    CloseReason::HostLeft => 0,
                    CloseReason::ServerShutdown => 1,
                });
            }
            Self::Start(s) => {
                w.u8(tag::START);
                w.u64_fixed(s.seed);
                w.str(&s.mode);
                w.bytes(&s.config);
                match s.model {
                    SimModel::Lockstep {
                        input_delay,
                        checkpoint_interval,
                    } => {
                        w.u8(0);
                        w.u8(input_delay);
                        w.u16(checkpoint_interval);
                    }
                    SimModel::HostAuthoritative { snapshot_hz } => {
                        w.u8(1);
                        w.u8(snapshot_hz);
                    }
                }
                w.varint(s.seats.len() as u64);
                for seat in &s.seats {
                    match seat {
                        None => w.u8(0),
                        Some(p) => {
                            w.u8(1);
                            put_player(&mut w, *p);
                        }
                    }
                }
            }
            Self::Input(p) => {
                w.u8(tag::INPUT);
                w.u8(p.seat);
                w.varint(u64::from(p.first_frame));
                w.varint(p.pads.len() as u64);
                for pad in &p.pads {
                    w.u16(*pad);
                }
            }
            Self::Checkpoint { seat, frame, hash } => {
                w.u8(tag::CHECKPOINT);
                w.u8(*seat);
                w.varint(u64::from(*frame));
                w.u64_fixed(*hash);
            }
            Self::SeatController {
                seat,
                controller,
                from_frame,
            } => {
                w.u8(tag::SEAT_CONTROLLER);
                w.u8(*seat);
                match controller {
                    Controller::Ai => w.u8(0),
                    Controller::Human(p) => {
                        w.u8(1);
                        put_player(&mut w, *p);
                    }
                }
                w.varint(u64::from(*from_frame));
            }
            Self::Pause { by } => {
                w.u8(tag::PAUSE);
                put_player(&mut w, *by);
            }
            Self::EndSession { by } => {
                w.u8(tag::END_SESSION);
                put_player(&mut w, *by);
            }
            Self::Resume { by } => {
                w.u8(tag::RESUME);
                put_player(&mut w, *by);
            }
            Self::Action { from, payload } => {
                w.u8(tag::ACTION);
                put_player(&mut w, *from);
                w.bytes(payload);
            }
            Self::Event {
                seq,
                seat,
                payload,
                hash_after,
            } => {
                w.u8(tag::EVENT);
                w.varint(u64::from(*seq));
                w.u8(*seat);
                w.bytes(payload);
                w.u64_fixed(*hash_after);
            }
            Self::StateRequest { from } => {
                w.u8(tag::STATE_REQUEST);
                put_player(&mut w, *from);
            }
            Self::StateBlob { to, seq, bytes } => {
                w.u8(tag::STATE_BLOB);
                put_player(&mut w, *to);
                w.varint(u64::from(*seq));
                w.bytes(bytes);
            }
            Self::Snapshot { tick, avatars } => {
                w.u8(tag::SNAPSHOT);
                w.varint(u64::from(*tick));
                w.varint(avatars.len() as u64);
                for a in avatars {
                    put_player(&mut w, a.player);
                    w.u16(a.scene);
                    for c in a.pos {
                        w.i32(c);
                    }
                    w.u16(a.facing);
                    w.u16(a.anim);
                    w.u16(a.anim_frame);
                }
            }
        }
        w.into_bytes()
    }

    /// Decodes one whole message; trailing bytes are an error.
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_MESSAGE_LEN {
            return Err(CodecError::TooLong {
                field: "message",
                len: bytes.len(),
                max: MAX_MESSAGE_LEN,
            });
        }
        let mut r = Reader::new(bytes);
        let msg = Self::decode_fields(&mut r)?;
        r.finish()?;
        Ok(msg)
    }

    fn decode_fields(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        let t = r.u8()?;
        Ok(match t {
            tag::HELLO => {
                let protocol = r.u16()?;
                let engine_build = r.string("engine_build", MAX_ID_LEN)?;
                let player_name = r.string("player_name", MAX_NAME_LEN)?;
                let pack = match r.u8()? {
                    0 => None,
                    1 => {
                        let id = r.string("pack id", MAX_ID_LEN)?;
                        let content_hash = r.raw(32)?.try_into().expect("32 bytes");
                        Some(PackIdentity { id, content_hash })
                    }
                    _ => return Err(CodecError::BadValue("pack flag")),
                };
                let room = match r.u8()? {
                    0 => RoomRequest::Create { seats: r.u8()? },
                    1 => RoomRequest::Join(get_code(r)?),
                    _ => return Err(CodecError::BadValue("room request")),
                };
                Self::Hello(Hello {
                    protocol,
                    engine_build,
                    player_name,
                    pack,
                    room,
                })
            }
            tag::WELCOME => Self::Welcome {
                room: get_code(r)?,
                you: get_player(r)?,
            },
            tag::REJECT => Self::Reject(match r.u8()? {
                0 => RejectReason::ProtocolMismatch { server: r.u16()? },
                1 => RejectReason::BuildMismatch,
                2 => RejectReason::PackMismatch,
                3 => RejectReason::NoSuchRoom,
                4 => RejectReason::RoomFull,
                5 => RejectReason::ServerFull,
                6 => RejectReason::InGame,
                7 => RejectReason::BadRequest,
                8 => RejectReason::RateLimited,
                9 => RejectReason::Malformed,
                10 => RejectReason::NotHost,
                11 => RejectReason::NotReady,
                12 => RejectReason::Unexpected,
                _ => return Err(CodecError::BadValue("reject reason")),
            }),
            tag::PING => Self::Ping {
                nonce: r.varint_u32()?,
            },
            tag::PONG => Self::Pong {
                nonce: r.varint_u32()?,
            },
            tag::LOBBY => {
                let host = get_player(r)?;
                let seats = r.u8()?;
                let n = r.len("lobby players", MAX_ROOM_MEMBERS)?;
                let mut players = Vec::with_capacity(n);
                for _ in 0..n {
                    players.push(LobbyPlayer {
                        id: get_player(r)?,
                        name: r.string("player_name", MAX_NAME_LEN)?,
                        seat: get_opt_seat(r)?,
                        ready: r.bool()?,
                    });
                }
                Self::Lobby(LobbyState {
                    host,
                    seats,
                    players,
                })
            }
            tag::SET_READY => Self::SetReady(r.bool()?),
            tag::ASSIGN_SEAT => Self::AssignSeat {
                player: get_player(r)?,
                seat: get_opt_seat(r)?,
            },
            tag::CHAT => Self::Chat {
                from: get_player(r)?,
                text: r.string("chat", MAX_CHAT_LEN)?,
            },
            tag::LEAVE => Self::Leave,
            tag::PLAYER_LEFT => Self::PlayerLeft {
                player: get_player(r)?,
                reason: match r.u8()? {
                    0 => LeaveReason::Left,
                    1 => LeaveReason::Disconnected,
                    _ => return Err(CodecError::BadValue("leave reason")),
                },
            },
            tag::HOST_CHANGED => Self::HostChanged {
                host: get_player(r)?,
            },
            tag::ROOM_CLOSED => Self::RoomClosed(match r.u8()? {
                0 => CloseReason::HostLeft,
                1 => CloseReason::ServerShutdown,
                _ => return Err(CodecError::BadValue("close reason")),
            }),
            tag::START => {
                let seed = r.u64_fixed()?;
                let mode = r.string("mode", MAX_ID_LEN)?;
                let config = r.bytes("config", MAX_CONFIG_LEN)?;
                let model = match r.u8()? {
                    0 => SimModel::Lockstep {
                        input_delay: r.u8()?,
                        checkpoint_interval: r.u16()?,
                    },
                    1 => SimModel::HostAuthoritative {
                        snapshot_hz: r.u8()?,
                    },
                    _ => return Err(CodecError::BadValue("sim model")),
                };
                let n = r.len("seats", MAX_SEATS)?;
                let mut seats = Vec::with_capacity(n);
                for _ in 0..n {
                    seats.push(match r.u8()? {
                        0 => None,
                        1 => Some(get_player(r)?),
                        _ => return Err(CodecError::BadValue("seat flag")),
                    });
                }
                Self::Start(StartGame {
                    seed,
                    mode,
                    config,
                    model,
                    seats,
                })
            }
            tag::INPUT => {
                let seat = get_seat(r)?;
                let first_frame = r.varint_u32()?;
                let n = r.len("pads", MAX_REDUNDANCY)?;
                let mut pads = Vec::with_capacity(n);
                for _ in 0..n {
                    pads.push(r.u16()?);
                }
                first_frame
                    .checked_add(n as u32)
                    .ok_or(CodecError::BadValue("frame"))?;
                Self::Input(InputPacket {
                    seat,
                    first_frame,
                    pads,
                })
            }
            tag::CHECKPOINT => Self::Checkpoint {
                seat: get_seat(r)?,
                frame: r.varint_u32()?,
                hash: r.u64_fixed()?,
            },
            tag::SEAT_CONTROLLER => Self::SeatController {
                seat: get_seat(r)?,
                controller: match r.u8()? {
                    0 => Controller::Ai,
                    1 => Controller::Human(get_player(r)?),
                    _ => return Err(CodecError::BadValue("controller")),
                },
                from_frame: r.varint_u32()?,
            },
            tag::PAUSE => Self::Pause { by: get_player(r)? },
            tag::RESUME => Self::Resume { by: get_player(r)? },
            tag::END_SESSION => Self::EndSession { by: get_player(r)? },
            tag::ACTION => Self::Action {
                from: get_player(r)?,
                payload: r.bytes("action", MAX_PAYLOAD_LEN)?,
            },
            tag::EVENT => Self::Event {
                seq: r.varint_u32()?,
                seat: r.u8()?,
                payload: r.bytes("event", MAX_PAYLOAD_LEN)?,
                hash_after: r.u64_fixed()?,
            },
            tag::STATE_REQUEST => Self::StateRequest {
                from: get_player(r)?,
            },
            tag::STATE_BLOB => Self::StateBlob {
                to: get_player(r)?,
                seq: r.varint_u32()?,
                bytes: r.bytes("state", MAX_PAYLOAD_LEN)?,
            },
            tag::SNAPSHOT => {
                let tick = r.varint_u32()?;
                let n = r.len("avatars", MAX_AVATARS)?;
                let mut avatars = Vec::with_capacity(n);
                for _ in 0..n {
                    avatars.push(AvatarState {
                        player: get_player(r)?,
                        scene: r.u16()?,
                        pos: [r.i32()?, r.i32()?, r.i32()?],
                        facing: r.u16()?,
                        anim: r.u16()?,
                        anim_frame: r.u16()?,
                    });
                }
                Self::Snapshot { tick, avatars }
            }
            other => return Err(CodecError::UnknownTag(other)),
        })
    }
}

fn get_code(r: &mut Reader<'_>) -> Result<RoomCode, CodecError> {
    let raw = r.raw(ROOM_CODE_LEN)?;
    if !raw.iter().all(|c| ROOM_CODE_ALPHABET.contains(c)) {
        return Err(CodecError::BadValue("room code"));
    }
    Ok(RoomCode(raw.try_into().expect("ROOM_CODE_LEN bytes")))
}
