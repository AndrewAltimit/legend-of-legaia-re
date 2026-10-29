//! Shared test pieces: a toy deterministic simulation and a room of
//! simulated peers on loopback links through a [`LocalRelay`].

use legaia_netplay::lockstep::Simulation;
use legaia_netplay::protocol::*;
use legaia_netplay::relay::RelayLimits;
use legaia_netplay::relay::local::LocalRelay;
use legaia_netplay::transport::Transport;
use legaia_netplay::transport::loopback::{self, LinkConditions, Loopback, SimClock};

pub const BUILD: &str = "test-build";

/// A deterministic toy: folds every seat's pad into a 64-bit state. AI seats
/// play a word derived from the state, so an AI handover that lands on
/// different frames on two peers diverges them.
#[derive(Debug, Clone, Default)]
pub struct Toy {
    pub state: u64,
    pub frame: u32,
    pub ai: [bool; MAX_SEATS],
    /// Corrupt the state after this frame (a deliberately broken peer).
    pub diverge_after: Option<u32>,
}

impl Simulation for Toy {
    fn tick(&mut self, inputs: &[u16]) {
        for (seat, &pad) in inputs.iter().enumerate() {
            let word = if self.ai[seat] {
                (self.state >> (seat * 7)) as u16 & 0x0FFF
            } else {
                pad
            };
            self.state = (self.state ^ (u64::from(word) << (seat * 8)))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .rotate_left(17);
        }
        self.frame += 1;
        if self.diverge_after == Some(self.frame) {
            self.state ^= 1;
        }
    }

    fn state_hash(&self) -> u64 {
        self.state ^ u64::from(self.frame)
    }

    fn set_seat_controller(&mut self, seat: usize, controller: Controller) {
        self.ai[seat] = controller == Controller::Ai;
    }
}

/// xorshift64.
pub fn rng(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// One client: its end of the link and what it has learned.
pub struct Client {
    pub t: Loopback,
    pub me: Option<PlayerId>,
    pub room: Option<RoomCode>,
    pub lobby: Option<LobbyState>,
    pub start: Option<StartGame>,
    pub rejects: Vec<RejectReason>,
    pub inbox: Vec<Message>,
    pub closed: bool,
}

impl Client {
    /// Drains the link into the client's state; returns what arrived.
    pub fn poll(&mut self) -> Vec<Message> {
        let mut got = Vec::new();
        loop {
            match self.t.try_recv() {
                Ok(Some(m)) => {
                    self.observe(&m);
                    got.push(m);
                }
                Ok(None) => break,
                Err(_) => {
                    self.closed = true;
                    break;
                }
            }
        }
        got
    }

    pub fn observe(&mut self, m: &Message) {
        match m {
            Message::Welcome { room, you } => {
                self.me = Some(*you);
                self.room = Some(*room);
            }
            Message::Lobby(l) => self.lobby = Some(l.clone()),
            Message::Start(s) => self.start = Some(s.clone()),
            Message::Reject(r) => self.rejects.push(*r),
            _ => {}
        }
        self.inbox.push(m.clone());
    }

    pub fn send(&mut self, m: Message) {
        self.t.send(&m).expect("client send");
    }
}

pub fn hello(name: &str, room: RoomRequest) -> Message {
    Message::Hello(Hello {
        protocol: PROTOCOL_VERSION,
        engine_build: BUILD.into(),
        player_name: name.into(),
        pack: Some(PackIdentity {
            id: "rebirth".into(),
            content_hash: [7; 32],
        }),
        room,
    })
}

/// A relay and its clients on one virtual clock. One tick = 16 ms of relay
/// time (the rate limiter's clock).
pub struct Net {
    pub clock: SimClock,
    pub relay: LocalRelay<Loopback>,
    pub clients: Vec<Client>,
}

impl Net {
    pub fn new(peers: usize, conditions: LinkConditions, seed: u64) -> Self {
        let clock = SimClock::new();
        let mut relay = LocalRelay::new(RelayLimits::default(), seed);
        let clients = (0..peers)
            .map(|i| {
                let (client, server) =
                    loopback::link(&clock, conditions, conditions, seed + i as u64 * 101);
                relay.add(server, 0);
                Client {
                    t: client,
                    me: None,
                    room: None,
                    lobby: None,
                    start: None,
                    rejects: Vec::new(),
                    inbox: Vec::new(),
                    closed: false,
                }
            })
            .collect();
        Self {
            clock,
            relay,
            clients,
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now() * 16
    }

    /// One tick: time moves, the relay runs, every client polls.
    pub fn step(&mut self) {
        self.clock.advance(1);
        let now = self.now_ms();
        self.relay.pump(now);
        for c in &mut self.clients {
            c.poll();
        }
    }

    /// Steps until `done` or `limit` ticks; returns whether `done` held.
    pub fn step_until(&mut self, limit: u32, mut done: impl FnMut(&Net) -> bool) -> bool {
        for _ in 0..limit {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    /// Client 0 creates a `seats`-seat room, the rest join it, everyone
    /// readies, client 0 starts with `model`. Returns the start order.
    pub fn open_room(&mut self, seats: u8, model: SimModel) -> StartGame {
        self.clients[0].send(hello("host", RoomRequest::Create { seats }));
        assert!(
            self.step_until(200, |n| n.clients[0].room.is_some()),
            "no welcome"
        );
        let code = self.clients[0].room.unwrap();
        for i in 1..self.clients.len() {
            self.clients[i].send(hello(&format!("peer{i}"), RoomRequest::Join(code)));
        }
        let n = self.clients.len();
        assert!(self.step_until(400, |net| net.clients.iter().all(|c| c.me.is_some())));
        for i in 1..n {
            self.clients[i].send(Message::SetReady(true));
        }
        assert!(self.step_until(400, |net| {
            net.clients[0]
                .lobby
                .as_ref()
                .is_some_and(|l| l.players.len() == n && l.players.iter().skip(1).all(|p| p.ready))
        }));
        self.clients[0].send(Message::Start(StartGame {
            seed: 42,
            mode: "toy".into(),
            config: Vec::new(),
            model,
            seats: Vec::new(),
        }));
        assert!(self.step_until(400, |net| net.clients.iter().all(|c| c.start.is_some())));
        self.clients[0].start.clone().unwrap()
    }
}
