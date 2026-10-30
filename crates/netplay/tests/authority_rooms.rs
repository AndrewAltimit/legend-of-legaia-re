//! The host-authoritative model through the relay: turn-based events stay
//! identical on every replica, a corrupted replica recovers from a state
//! blob, avatar snapshots reach the others, and the room closes when its
//! host leaves.

use legaia_netplay::authority::{AuthorityHost, AuthorityReplica, AvatarInterpolator, EventSim};
use legaia_netplay::protocol::*;
use legaia_netplay::transport::Transport;
use legaia_netplay::transport::loopback::LinkConditions;

use crate::harness::Net;

/// A turn-taking toy: seats bet in order; an event is `[amount]`.
#[derive(Debug, Clone, Default, PartialEq)]
struct Pot {
    turn: u8,
    seats: u8,
    pot: u32,
    log: Vec<(u8, u8)>,
}

impl EventSim for Pot {
    fn apply_event(&mut self, seat: u8, event: &[u8]) {
        self.pot += u32::from(event[0]);
        self.log.push((seat, event[0]));
        self.turn = (self.turn + 1) % self.seats;
    }

    fn state_hash(&self) -> u64 {
        self.log.iter().fold(u64::from(self.pot), |h, &(s, a)| {
            h.wrapping_mul(1_000_003) ^ (u64::from(s) << 8 | u64::from(a))
        })
    }

    fn save_state(&self) -> Option<Vec<u8>> {
        let mut out = vec![self.turn, self.seats];
        out.extend_from_slice(&self.pot.to_le_bytes());
        for &(s, a) in &self.log {
            out.extend_from_slice(&[s, a]);
        }
        Some(out)
    }

    fn load_state(&mut self, b: &[u8]) -> bool {
        if b.len() < 6 {
            return false;
        }
        self.turn = b[0];
        self.seats = b[1];
        self.pot = u32::from_le_bytes(b[2..6].try_into().unwrap());
        self.log = b[6..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| (c[0], c[1]))
            .collect();
        true
    }
}

/// The host accepts a bet only on the sender's turn and caps it at 10.
fn validate(sim: &Pot, seat: u8, action: &[u8]) -> Option<Vec<u8>> {
    (seat == sim.turn && action.len() == 1).then(|| vec![action[0].min(10)])
}

const MODEL: SimModel = SimModel::HostAuthoritative { snapshot_hz: 15 };

fn jittery() -> LinkConditions {
    LinkConditions {
        latency: 2,
        jitter: 5,
        input_drop_per_mille: 0,
    }
}

struct Table {
    net: Net,
    host: AuthorityHost<Pot>,
    replicas: Vec<AuthorityReplica<Pot>>,
}

impl Table {
    fn new() -> Self {
        let mut net = Net::new(3, jittery(), 21);
        let start = net.open_room(3, MODEL);
        let pot = Pot {
            seats: 3,
            ..Pot::default()
        };
        let host = AuthorityHost::new(pot.clone(), start.seats.clone());
        let replicas = (1..3)
            .map(|i| AuthorityReplica::new(pot.clone(), net.clients[i].me.unwrap()))
            .collect();
        Self {
            net,
            host,
            replicas,
        }
    }

    fn step(&mut self) {
        self.net.clock.advance(1);
        let now = self.net.now_ms();
        self.net.relay.pump(now);
        let host = &mut self.net.clients[0];
        match self.host.pump(&mut host.t, validate) {
            Ok(other) => other.iter().for_each(|m| host.observe(m)),
            Err(_) => host.closed = true,
        }
        for (i, r) in self.replicas.iter_mut().enumerate() {
            let client = &mut self.net.clients[i + 1];
            match r.pump(&mut client.t) {
                Ok(other) => other.iter().for_each(|m| client.observe(m)),
                Err(_) => client.closed = true,
            }
        }
    }

    fn settle(&mut self) {
        for _ in 0..40 {
            self.step();
        }
    }
}

#[test]
fn replicas_follow_the_host_through_a_hundred_rounds() {
    let mut t = Table::new();
    for round in 0..100u32 {
        // Host bets on its own turn directly.
        t.host.emit(0, vec![(round % 7) as u8]);
        t.settle();
        // Seat 2 bets out of turn (seat 1 is up): refused.
        t.replicas[1].send_action(vec![99]);
        t.settle();
        for i in 0..2 {
            t.replicas[i].send_action(vec![(round % 13) as u8]);
            t.settle();
        }
    }
    let want = t.host.sim().clone();
    assert_eq!(want.log.len(), 300, "out-of-turn bets must be refused");
    assert!(want.log.iter().all(|&(_, a)| a <= 10), "host caps bets");
    for r in &t.replicas {
        assert_eq!(r.sim(), &want);
        assert_eq!(r.desync(), None);
        assert_eq!(r.seq(), 300);
    }
}

#[test]
fn a_corrupted_replica_recovers_from_a_state_blob() {
    let mut t = Table::new();
    t.host.emit(0, vec![5]);
    t.settle();
    // Replica 0 goes wrong (a rejoin with a stale state) and asks the
    // host for the real one.
    let me = t.net.clients[1].me.unwrap();
    t.replicas[0] = AuthorityReplica::new(
        Pot {
            seats: 3,
            pot: 1_000,
            ..Pot::default()
        },
        me,
    );
    t.replicas[0].request_state();
    t.host.emit(1, vec![3]);
    t.settle();
    let r = &t.replicas[0];
    assert_eq!(r.sim(), t.host.sim(), "replica not restored");
    assert_eq!(r.desync(), None);
    assert_eq!(r.seq(), t.host.seq());
}

#[test]
fn avatar_snapshots_reach_the_others_and_interpolate() {
    let mut t = Table::new();
    let host_id = t.net.clients[0].me.unwrap();
    let mut ip = AvatarInterpolator::for_rate(15);
    for tick in (0..=40u32).step_by(4) {
        t.net.clients[0].send(Message::Snapshot {
            tick,
            avatars: vec![AvatarState {
                player: host_id,
                pos: [tick as i32 * 10, 0, 0],
                ..Default::default()
            }],
        });
    }
    t.settle();
    for m in &t.net.clients[1].inbox {
        if let Message::Snapshot { tick, avatars } = m {
            ip.push_snapshot(*tick, avatars);
        }
    }
    for _ in 0..30 {
        ip.tick();
    }
    let s = ip.sample();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].player, host_id);
    // Render time = clock - 8; position is a straight line through it.
    assert_eq!(s[0].pos[0] % 10, 0);
    assert!(s[0].pos[0] > 0 && s[0].pos[0] <= 400);
    // Only the host may send snapshots.
    t.net.clients[1].send(Message::Snapshot {
        tick: 0,
        avatars: vec![],
    });
    t.settle();
    assert!(t.net.clients[1].rejects.contains(&RejectReason::Unexpected));
}

#[test]
fn the_room_closes_when_the_host_leaves() {
    let mut t = Table::new();
    t.net.clients[0].t.close();
    t.settle();
    for c in &t.net.clients[1..] {
        assert!(
            c.inbox
                .contains(&Message::RoomClosed(CloseReason::HostLeft))
        );
        assert!(c.closed);
    }
    assert_eq!(t.net.relay.core().room_count(), 0);
}
