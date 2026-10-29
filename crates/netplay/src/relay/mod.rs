//! The relay: rooms with join codes, the lobby, and message routing between
//! a room's members.
//!
//! [`RelayCore`] is the whole relay as a state machine with no I/O: feed it
//! connection events and messages, drain the sends and closes it decides
//! on. Two drivers run it - [`server::RelayServer`] (threads and sockets,
//! the `legaia-relay` binary and the in-process "host a world" server) and
//! [`local::LocalRelay`] (any [`crate::transport::Transport`]s, for tests
//! and same-machine sessions).
//!
//! The relay never simulates. In a lockstep room it forwards each seat's
//! inputs and checkpoints to everyone else, stamped with the sender's real
//! seat; in a host-authoritative room it carries actions to the host and
//! the host's events and snapshots to everyone. It remembers one number per
//! seat (the end of the last input frame it forwarded) so that when a
//! player drops it can hand their seat to the AI at a frame every peer
//! agrees on.

use std::collections::HashMap;

use crate::protocol::{
    CloseReason, Controller, Hello, InputPacket, LeaveReason, LobbyPlayer, LobbyState,
    MAX_NAME_LEN, MAX_ROOM_MEMBERS, MAX_SEATS, Message, PROTOCOL_VERSION, PackIdentity, PlayerId,
    RejectReason, RoomCode, RoomRequest, SimModel, StartGame,
};

pub mod local;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

/// A connection, as the driver numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnId(pub u64);

/// What the core asks its driver to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayOutput {
    Send(ConnId, Message),
    /// Close the connection (after the sends queued before this).
    Close(ConnId),
}

/// The relay's resource bounds. Everything a client can make the relay
/// hold is bounded by one of these: rooms, members per room, the lobby
/// rows, the per-seat frame counter. No per-frame history is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayLimits {
    pub max_connections: usize,
    pub max_rooms: usize,
    /// Members (seated and spectating) per room, at most [`MAX_ROOM_MEMBERS`].
    pub max_members: usize,
    /// Sustained messages per second per connection.
    pub messages_per_sec: u32,
    /// Message burst a connection may send above the sustained rate.
    pub burst: u32,
    /// Unknown room codes a connection may try before it is dropped.
    pub max_bad_codes: u8,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            max_connections: 256,
            max_rooms: 64,
            max_members: MAX_ROOM_MEMBERS,
            // 60 input packets + checkpoints + stall resends, with headroom.
            messages_per_sec: 240,
            burst: 480,
            max_bad_codes: 5,
        }
    }
}

#[derive(Debug)]
struct ConnState {
    member: Option<(RoomCode, PlayerId)>,
    /// Token bucket, in thousandths of a message.
    tokens: u64,
    last_ms: u64,
    bad_codes: u8,
}

#[derive(Debug)]
struct Member {
    id: PlayerId,
    conn: ConnId,
    name: String,
    seat: Option<u8>,
    ready: bool,
}

#[derive(Debug)]
enum Phase {
    Lobby,
    Lockstep {
        input_delay: u32,
        /// Per seat, one past the last input frame forwarded.
        input_end: Vec<Option<u32>>,
        /// Per seat, the last input packet forwarded: re-sent when the
        /// seat's player drops, so a peer that lost it is not left waiting
        /// on a sender that is gone.
        last_packet: Vec<Option<InputPacket>>,
    },
    Authoritative,
}

#[derive(Debug)]
struct Room {
    host: PlayerId,
    seats: u8,
    engine_build: String,
    pack: Option<PackIdentity>,
    /// In join order (the order host migration follows).
    members: Vec<Member>,
    next_id: u32,
    phase: Phase,
}

impl Room {
    fn member(&self, id: PlayerId) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    fn lobby(&self) -> LobbyState {
        LobbyState {
            host: self.host,
            seats: self.seats,
            players: self
                .members
                .iter()
                .map(|m| LobbyPlayer {
                    id: m.id,
                    name: m.name.clone(),
                    seat: m.seat,
                    ready: m.ready,
                })
                .collect(),
        }
    }

    fn free_seat(&self) -> Option<u8> {
        (0..self.seats).find(|s| self.members.iter().all(|m| m.seat != Some(*s)))
    }
}

/// The relay state machine.
#[derive(Debug)]
pub struct RelayCore {
    limits: RelayLimits,
    conns: HashMap<ConnId, ConnState>,
    rooms: HashMap<RoomCode, Room>,
    code_rng: u64,
    out: Vec<RelayOutput>,
}

impl RelayCore {
    /// `seed` drives room-code generation; give it real entropy on a
    /// public-facing relay.
    pub fn new(limits: RelayLimits, seed: u64) -> Self {
        Self {
            limits,
            conns: HashMap::new(),
            rooms: HashMap::new(),
            code_rng: seed | 1,
            out: Vec::new(),
        }
    }

    pub fn connection_count(&self) -> usize {
        self.conns.len()
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// The sends and closes decided since the last drain, in order.
    pub fn drain(&mut self) -> Vec<RelayOutput> {
        std::mem::take(&mut self.out)
    }

    /// A new connection. Refused (and closed) over the connection cap.
    pub fn connect(&mut self, conn: ConnId, now_ms: u64) -> bool {
        if self.conns.len() >= self.limits.max_connections {
            self.send(conn, Message::Reject(RejectReason::ServerFull));
            self.out.push(RelayOutput::Close(conn));
            return false;
        }
        self.conns.insert(
            conn,
            ConnState {
                member: None,
                tokens: u64::from(self.limits.burst) * 1000,
                last_ms: now_ms,
                bad_codes: 0,
            },
        );
        true
    }

    /// The connection's bytes did not decode.
    pub fn malformed(&mut self, conn: ConnId) {
        self.reject(conn, RejectReason::Malformed);
    }

    /// The connection is gone (socket closed or errored).
    pub fn disconnect(&mut self, conn: ConnId) {
        if let Some(state) = self.conns.remove(&conn)
            && let Some((code, id)) = state.member
        {
            self.remove_member(code, id, LeaveReason::Disconnected);
        }
    }

    /// Closes every room and connection.
    pub fn shutdown(&mut self) {
        let conns: Vec<ConnId> = self.conns.keys().copied().collect();
        for conn in conns {
            if self.conns[&conn].member.is_some() {
                self.send(conn, Message::RoomClosed(CloseReason::ServerShutdown));
            }
            self.out.push(RelayOutput::Close(conn));
        }
        self.conns.clear();
        self.rooms.clear();
    }

    fn send(&mut self, conn: ConnId, msg: Message) {
        self.out.push(RelayOutput::Send(conn, msg));
    }

    /// Sends `reason`; a fatal one also closes and forgets the connection.
    fn reject(&mut self, conn: ConnId, reason: RejectReason) {
        self.send(conn, Message::Reject(reason));
        if reason.is_fatal() {
            self.out.push(RelayOutput::Close(conn));
            self.disconnect(conn);
        }
    }

    fn broadcast(&mut self, code: RoomCode, except: Option<PlayerId>, msg: &Message) {
        let Some(room) = self.rooms.get(&code) else {
            return;
        };
        let targets: Vec<ConnId> = room
            .members
            .iter()
            .filter(|m| Some(m.id) != except)
            .map(|m| m.conn)
            .collect();
        for conn in targets {
            self.send(conn, msg.clone());
        }
    }

    fn broadcast_lobby(&mut self, code: RoomCode) {
        if let Some(room) = self.rooms.get(&code) {
            let msg = Message::Lobby(room.lobby());
            self.broadcast(code, None, &msg);
        }
    }

    fn take_token(&mut self, conn: ConnId, now_ms: u64) -> bool {
        let rate = u64::from(self.limits.messages_per_sec);
        let cap = u64::from(self.limits.burst) * 1000;
        let Some(state) = self.conns.get_mut(&conn) else {
            return false;
        };
        let elapsed = now_ms.saturating_sub(state.last_ms);
        state.last_ms = state.last_ms.max(now_ms);
        state.tokens = (state.tokens + elapsed * rate).min(cap);
        if state.tokens < 1000 {
            return false;
        }
        state.tokens -= 1000;
        true
    }

    fn new_code(&mut self) -> RoomCode {
        loop {
            let mut x = self.code_rng;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.code_rng = x;
            let code = RoomCode::from_bits(x);
            if !self.rooms.contains_key(&code) {
                return code;
            }
        }
    }

    /// One message from `conn`.
    pub fn handle(&mut self, conn: ConnId, msg: Message, now_ms: u64) {
        if !self.conns.contains_key(&conn) {
            return;
        }
        if !self.take_token(conn, now_ms) {
            self.reject(conn, RejectReason::RateLimited);
            return;
        }
        if let Message::Ping { nonce } = msg {
            self.send(conn, Message::Pong { nonce });
            return;
        }
        if matches!(msg, Message::Pong { .. }) {
            return;
        }
        match self.conns[&conn].member {
            None => match msg {
                Message::Hello(hello) => self.hello(conn, hello),
                _ => self.reject(conn, RejectReason::Unexpected),
            },
            Some((code, id)) => self.member_message(conn, code, id, msg),
        }
    }

    fn hello(&mut self, conn: ConnId, hello: Hello) {
        if hello.protocol != PROTOCOL_VERSION {
            self.reject(
                conn,
                RejectReason::ProtocolMismatch {
                    server: PROTOCOL_VERSION,
                },
            );
            return;
        }
        let name = hello.player_name.trim().to_string();
        if name.is_empty() || name.len() > MAX_NAME_LEN || name.chars().any(char::is_control) {
            self.reject(conn, RejectReason::BadRequest);
            return;
        }
        let (code, id) = match hello.room {
            RoomRequest::Create { seats } => {
                if seats == 0 || usize::from(seats) > MAX_SEATS {
                    self.reject(conn, RejectReason::BadRequest);
                    return;
                }
                if self.rooms.len() >= self.limits.max_rooms {
                    self.reject(conn, RejectReason::ServerFull);
                    return;
                }
                let code = self.new_code();
                let id = PlayerId(1);
                self.rooms.insert(
                    code,
                    Room {
                        host: id,
                        seats,
                        engine_build: hello.engine_build,
                        pack: hello.pack,
                        members: vec![Member {
                            id,
                            conn,
                            name,
                            seat: Some(0),
                            ready: false,
                        }],
                        next_id: 2,
                        phase: Phase::Lobby,
                    },
                );
                (code, id)
            }
            RoomRequest::Join(code) => {
                let max_members = self.limits.max_members.min(MAX_ROOM_MEMBERS);
                let refusal = match self.rooms.get(&code) {
                    None => Some(RejectReason::NoSuchRoom),
                    Some(room) if !matches!(room.phase, Phase::Lobby) => Some(RejectReason::InGame),
                    Some(room) if room.members.len() >= max_members => Some(RejectReason::RoomFull),
                    Some(room) if room.engine_build != hello.engine_build => {
                        Some(RejectReason::BuildMismatch)
                    }
                    Some(room) if room.pack != hello.pack => Some(RejectReason::PackMismatch),
                    Some(_) => None,
                };
                if let Some(reason) = refusal {
                    if reason == RejectReason::NoSuchRoom {
                        let state = self.conns.get_mut(&conn).expect("checked in handle");
                        state.bad_codes += 1;
                        if state.bad_codes >= self.limits.max_bad_codes {
                            self.reject(conn, RejectReason::RateLimited);
                            return;
                        }
                    }
                    self.reject(conn, reason);
                    return;
                }
                let room = self.rooms.get_mut(&code).expect("checked above");
                let id = PlayerId(room.next_id);
                room.next_id += 1;
                let seat = room.free_seat();
                room.members.push(Member {
                    id,
                    conn,
                    name,
                    seat,
                    ready: false,
                });
                (code, id)
            }
        };
        self.conns.get_mut(&conn).expect("checked in handle").member = Some((code, id));
        self.send(
            conn,
            Message::Welcome {
                room: code,
                you: id,
            },
        );
        self.broadcast_lobby(code);
    }

    fn member_message(&mut self, conn: ConnId, code: RoomCode, id: PlayerId, msg: Message) {
        let Some(room) = self.rooms.get_mut(&code) else {
            return;
        };
        let is_host = room.host == id;
        let seat = room.member(id).and_then(|m| m.seat);
        let in_lobby = matches!(room.phase, Phase::Lobby);
        match msg {
            Message::SetReady(ready) if in_lobby => {
                if let Some(m) = room.members.iter_mut().find(|m| m.id == id) {
                    m.ready = ready;
                }
                self.broadcast_lobby(code);
            }
            Message::AssignSeat { player, seat } if in_lobby => {
                if !is_host {
                    return self.reject(conn, RejectReason::NotHost);
                }
                let taken = seat.is_some_and(|s| {
                    s >= room.seats
                        || room
                            .members
                            .iter()
                            .any(|m| m.seat == Some(s) && m.id != player)
                });
                let Some(target) = room.members.iter_mut().find(|m| m.id == player) else {
                    return self.reject(conn, RejectReason::BadRequest);
                };
                if taken {
                    return self.reject(conn, RejectReason::BadRequest);
                }
                target.seat = seat;
                target.ready = false;
                self.broadcast_lobby(code);
            }
            Message::Chat { text, .. } => {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                self.broadcast(code, None, &Message::Chat { from: id, text });
            }
            Message::Leave => {
                self.conns.remove(&conn);
                self.out.push(RelayOutput::Close(conn));
                self.remove_member(code, id, LeaveReason::Left);
            }
            Message::Start(start) if in_lobby => {
                if !is_host {
                    return self.reject(conn, RejectReason::NotHost);
                }
                let unready = room
                    .members
                    .iter()
                    .any(|m| m.seat.is_some() && m.id != room.host && !m.ready);
                if unready {
                    return self.reject(conn, RejectReason::NotReady);
                }
                let seats: Vec<Option<PlayerId>> = (0..room.seats)
                    .map(|s| {
                        room.members
                            .iter()
                            .find(|m| m.seat == Some(s))
                            .map(|m| m.id)
                    })
                    .collect();
                room.phase = match start.model {
                    SimModel::Lockstep { input_delay, .. } => Phase::Lockstep {
                        input_delay: u32::from(input_delay),
                        input_end: vec![None; seats.len()],
                        last_packet: vec![None; seats.len()],
                    },
                    SimModel::HostAuthoritative { .. } => Phase::Authoritative,
                };
                let start = Message::Start(StartGame { seats, ..start });
                self.broadcast(code, None, &start);
            }
            Message::Input(mut packet) => {
                let (
                    Phase::Lockstep {
                        input_end,
                        last_packet,
                        ..
                    },
                    Some(seat),
                ) = (&mut room.phase, seat)
                else {
                    return self.reject(conn, RejectReason::Unexpected);
                };
                packet.seat = seat;
                let end = &mut input_end[usize::from(seat)];
                if end.is_none_or(|e| packet.end_frame() >= e) {
                    *end = Some(packet.end_frame());
                    last_packet[usize::from(seat)] = Some(packet.clone());
                }
                self.broadcast(code, Some(id), &Message::Input(packet));
            }
            Message::Checkpoint { frame, hash, .. } => {
                let (Phase::Lockstep { .. }, Some(seat)) = (&room.phase, seat) else {
                    return self.reject(conn, RejectReason::Unexpected);
                };
                self.broadcast(code, Some(id), &Message::Checkpoint { seat, frame, hash });
            }
            Message::Pause { .. } | Message::Resume { .. } if !in_lobby => {
                let msg = match msg {
                    Message::Pause { .. } => Message::Pause { by: id },
                    _ => Message::Resume { by: id },
                };
                self.broadcast(code, Some(id), &msg);
            }
            Message::EndSession { .. } if !in_lobby => {
                room.phase = Phase::Lobby;
                for m in &mut room.members {
                    m.ready = false;
                }
                self.broadcast(code, None, &Message::EndSession { by: id });
                self.broadcast_lobby(code);
            }
            // Actions reach the host in the lobby too: shared-world roaming
            // runs there, each guest reporting its own avatar.
            Message::Action { payload, .. }
                if in_lobby || matches!(room.phase, Phase::Authoritative) =>
            {
                if let Some(host) = room.member(room.host).map(|m| m.conn)
                    && !is_host
                {
                    self.send(host, Message::Action { from: id, payload });
                }
            }
            Message::StateRequest { .. } if matches!(room.phase, Phase::Authoritative) => {
                if let Some(host) = room.member(room.host).map(|m| m.conn)
                    && !is_host
                {
                    self.send(host, Message::StateRequest { from: id });
                }
            }
            Message::Event { .. } if is_host && matches!(room.phase, Phase::Authoritative) => {
                self.broadcast(code, Some(id), &msg);
            }
            Message::Snapshot { .. }
                if is_host && (in_lobby || matches!(room.phase, Phase::Authoritative)) =>
            {
                self.broadcast(code, Some(id), &msg);
            }
            Message::StateBlob { to, .. }
                if is_host && matches!(room.phase, Phase::Authoritative) =>
            {
                if let Some(target) = room.member(to).map(|m| m.conn) {
                    self.send(target, msg);
                }
            }
            _ => self.reject(conn, RejectReason::Unexpected),
        }
    }

    fn remove_member(&mut self, code: RoomCode, id: PlayerId, reason: LeaveReason) {
        let Some(room) = self.rooms.get_mut(&code) else {
            return;
        };
        let Some(index) = room.members.iter().position(|m| m.id == id) else {
            return;
        };
        let member = room.members.remove(index);
        if room.members.is_empty() {
            self.rooms.remove(&code);
            return;
        }
        let was_host = room.host == id;

        // A host-authoritative session's state lives on the host: without
        // it the room cannot continue.
        if was_host && matches!(room.phase, Phase::Authoritative) {
            let conns: Vec<ConnId> = room.members.iter().map(|m| m.conn).collect();
            self.rooms.remove(&code);
            for conn in conns {
                self.send(conn, Message::RoomClosed(CloseReason::HostLeft));
                self.out.push(RelayOutput::Close(conn));
                self.conns.remove(&conn);
            }
            return;
        }

        let (handover, last_packet) = match (&mut room.phase, member.seat) {
            (
                Phase::Lockstep {
                    input_delay,
                    input_end,
                    last_packet,
                },
                Some(seat),
            ) => (
                Some((seat, input_end[usize::from(seat)].unwrap_or(*input_delay))),
                last_packet[usize::from(seat)].take(),
            ),
            (Phase::Authoritative, Some(seat)) => (Some((seat, 0)), None),
            _ => (None, None),
        };
        if was_host {
            room.host = room.members[0].id;
        }
        let new_host = room.host;
        let in_lobby = matches!(room.phase, Phase::Lobby);

        self.broadcast(code, None, &Message::PlayerLeft { player: id, reason });
        if let Some(packet) = last_packet {
            self.broadcast(code, None, &Message::Input(packet));
        }
        if let Some((seat, from_frame)) = handover {
            self.broadcast(
                code,
                None,
                &Message::SeatController {
                    seat,
                    controller: Controller::Ai,
                    from_frame,
                },
            );
        }
        if was_host {
            self.broadcast(code, None, &Message::HostChanged { host: new_host });
        }
        if in_lobby {
            self.broadcast_lobby(code);
        }
    }
}
