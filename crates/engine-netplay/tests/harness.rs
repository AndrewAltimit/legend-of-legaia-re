//! Two (or more) in-process worlds, each with its own netplay controller,
//! joined through the relay core over loopback links on one virtual clock.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{ActiveMode, ModeEntry};
use legaia_engine_netplay::room::NetPlay;
use legaia_netplay::relay::RelayLimits;
use legaia_netplay::relay::local::LocalRelay;
use legaia_netplay::transport::loopback::{self, LinkConditions, Loopback, SimClock};

/// The scene id every test world stands in.
pub const SCENE: u16 = 412;

/// One player: a world and its controller.
pub struct Peer {
    pub world: World,
    pub net: NetPlay,
}

impl Peer {
    /// The world's networked session, while one is seated.
    pub fn mode(&self) -> Option<&ActiveMode> {
        self.world.expansion.mode_seat.as_ref()?.active()
    }
}

pub fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w
}

/// A link with a little latency, reliable and in order (the relay's
/// transports are streams).
pub fn lan() -> LinkConditions {
    LinkConditions {
        latency: 2,
        ..LinkConditions::default()
    }
}

pub struct Room {
    pub clock: SimClock,
    pub relay: LocalRelay<Loopback>,
    pub peers: Vec<Peer>,
}

impl Room {
    /// Peer 0 hosts; the rest join once its room exists.
    pub fn open(players: usize, conditions: LinkConditions, seed: u64) -> Self {
        let clock = SimClock::new();
        let mut relay = LocalRelay::new(RelayLimits::default(), seed);
        let (client, server) = loopback::link(&clock, conditions, conditions, seed);
        relay.add(server, 0);
        let host = Peer {
            world: field_world(),
            net: NetPlay::host(Box::new(client), "Host", None),
        };
        let mut room = Self {
            clock,
            relay,
            peers: vec![host],
        };
        assert!(
            room.run_until(200, |r| r.peers[0].net.room.is_some()),
            "no room"
        );
        let code = room.peers[0].net.room.unwrap();
        for i in 1..players {
            let (client, server) =
                loopback::link(&room.clock, conditions, conditions, seed + 31 * i as u64);
            let now = room.clock.now() * 16;
            room.relay.add(server, now);
            room.peers.push(Peer {
                world: field_world(),
                net: NetPlay::join(Box::new(client), &format!("Guest{i}"), code, None),
            });
        }
        assert!(
            room.run_until(400, |r| r.peers.iter().all(|p| p.net.in_lobby())
                && r.peers[0]
                    .net
                    .lobby
                    .as_ref()
                    .is_some_and(|l| l.players.len() == players)),
            "everyone joins"
        );
        room
    }

    /// One frame everywhere, every player holding `pads(peer, frame)`.
    pub fn step_with(&mut self, pads: &mut dyn FnMut(usize, &Peer) -> u16) {
        self.clock.advance(1);
        let now = self.clock.now() * 16;
        self.relay.pump(now);
        for i in 0..self.peers.len() {
            let pad = pads(i, &self.peers[i]);
            let p = &mut self.peers[i];
            let world_pad = p.net.frame(&mut p.world, SCENE, pad);
            p.world.input.set_pad(world_pad);
            p.world.tick();
        }
    }

    pub fn step(&mut self) {
        self.step_with(&mut |_, _| 0);
    }

    pub fn run_until(&mut self, limit: u32, mut done: impl FnMut(&Room) -> bool) -> bool {
        for _ in 0..limit {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    /// Ready every guest, then have the host start instance `id`.
    pub fn start(&mut self, id: &str) {
        // Let a session that just ended settle (its lobby broadcast clears
        // everyone's readiness) before readying.
        for _ in 0..30 {
            self.step();
        }
        for p in &mut self.peers[1..] {
            p.net.set_ready(true);
            p.net.screen_open = false;
        }
        self.peers[0].net.screen_open = false;
        let n = self.peers.len();
        assert!(
            self.run_until(200, |r| r.peers[0].net.lobby.as_ref().is_some_and(|l| {
                l.players.len() == n && l.players.iter().filter(|p| p.ready).count() == n - 1
            }))
        );
        let entry: ModeEntry = self.peers[0]
            .world
            .pack_mode_entries()
            .into_iter()
            .find(|e| e.id == id)
            .expect("a built-in instance");
        let host = &mut self.peers[0];
        host.net.request_start(&host.world, &entry);
        let entered = self.run_until(200, |r| r.peers.iter().all(|p| p.net.in_mode()));
        assert!(
            entered,
            "every peer enters {id}: {:?}",
            self.peers
                .iter()
                .map(|p| (&p.net.chat, &p.net.notice))
                .collect::<Vec<_>>()
        );
    }
}
