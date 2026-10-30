//! Lockstep across 2-4 simulated peers through the relay core, over links
//! with latency, jitter (so reordering) and input-packet loss: every peer
//! reaches the same state, desyncs are caught, and a dropped player's seat
//! goes to the AI on the same frame everywhere.

use legaia_netplay::lockstep::{LockstepConfig, LockstepSession, Simulation, Step};
use legaia_netplay::protocol::*;
use legaia_netplay::transport::Transport;
use legaia_netplay::transport::loopback::LinkConditions;

use crate::harness::{Net, Toy, rng};

const LOCKSTEP: SimModel = SimModel::Lockstep {
    input_delay: 4,
    checkpoint_interval: 30,
};

struct Run {
    net: Net,
    sessions: Vec<Option<LockstepSession<Toy>>>,
    pad_rng: Vec<u64>,
}

impl Run {
    fn new(peers: usize, conditions: LinkConditions, seed: u64, toys: Vec<Toy>) -> Self {
        let mut net = Net::new(peers, conditions, seed);
        let start = net.open_room(peers as u8, LOCKSTEP);
        assert_eq!(start.seats.len(), peers);
        let sessions = net
            .clients
            .iter()
            .zip(toys)
            .map(|(c, toy)| {
                let cfg = LockstepConfig::from_start(&start, c.me.unwrap()).unwrap();
                assert_eq!(cfg.input_delay, 4);
                Some(LockstepSession::new(toy, cfg))
            })
            .collect();
        Self {
            net,
            sessions,
            pad_rng: (0..peers as u64).map(|i| 0x1234 + i * 77).collect(),
        }
    }

    /// One tick for everyone; a session stops at `target` frames.
    fn step(&mut self, target: u32) {
        self.net.clock.advance(1);
        let now = self.net.now_ms();
        self.net.relay.pump(now);
        for (i, slot) in self.sessions.iter_mut().enumerate() {
            let Some(s) = slot else { continue };
            if s.frame() < target {
                if s.wants_local_input() {
                    let pad = rng(&mut self.pad_rng[i]) as u16;
                    s.push_local_input(pad);
                }
                s.advance();
            }
            let client = &mut self.net.clients[i];
            match s.pump(&mut client.t) {
                Ok(other) => {
                    for m in &other {
                        client.observe(m);
                    }
                }
                Err(_) => client.closed = true,
            }
        }
    }

    fn run_to(&mut self, target: u32, tick_limit: u32) {
        for _ in 0..tick_limit {
            if self
                .sessions
                .iter()
                .flatten()
                .all(|s| s.frame() >= target || s.desync().is_some())
            {
                return;
            }
            self.step(target);
        }
        let frames: Vec<u32> = self.sessions.iter().flatten().map(|s| s.frame()).collect();
        panic!("stalled: frames {frames:?} after {tick_limit} ticks");
    }

    fn assert_converged(&self, frames: u32) {
        let live: Vec<&LockstepSession<Toy>> = self.sessions.iter().flatten().collect();
        let hash = live[0].sim().state_hash();
        for s in &live {
            assert_eq!(s.frame(), frames);
            assert_eq!(s.desync(), None);
            assert!(s.violations().is_empty(), "{:?}", s.violations());
            assert_eq!(s.sim().state_hash(), hash, "peers diverged");
            // Non-vacuous: checkpoints were actually compared.
            assert!(
                s.verified_checkpoints() >= frames / 30 - 2,
                "{}",
                s.verified_checkpoints()
            );
        }
    }
}

fn rough() -> LinkConditions {
    LinkConditions {
        latency: 3,
        jitter: 6,
        input_drop_per_mille: 50,
    }
}

#[test]
fn two_to_four_peers_agree_over_ten_thousand_frames() {
    for peers in 2..=4 {
        let mut run = Run::new(
            peers,
            rough(),
            11 + peers as u64,
            vec![Toy::default(); peers],
        );
        run.run_to(10_000, 60_000);
        run.assert_converged(10_000);
    }
}

#[test]
fn clean_link_runs_at_one_frame_per_tick() {
    // Latency under the input delay: nobody ever waits after the start.
    let c = LinkConditions {
        latency: 1,
        ..Default::default()
    };
    let mut run = Run::new(2, c, 3, vec![Toy::default(); 2]);
    let t0 = run.net.clock.now();
    run.run_to(2_000, 10_000);
    let ticks = run.net.clock.now() - t0;
    assert!(ticks <= 2_010, "took {ticks} ticks for 2000 frames");
    run.assert_converged(2_000);
}

#[test]
fn a_diverging_peer_is_detected_by_everyone() {
    let mut toys = vec![Toy::default(); 3];
    toys[2].diverge_after = Some(5_000);
    let mut run = Run::new(3, rough(), 99, toys);
    run.run_to(10_000, 60_000);
    for (i, slot) in run.sessions.iter_mut().enumerate() {
        let s = slot.as_mut().unwrap();
        let d = s
            .desync()
            .unwrap_or_else(|| panic!("peer {i} saw no desync"));
        // The first checkpoint after the corruption: frames 29, 59, ...
        // (the post-tick state of frame f is frame f + 1's input).
        assert_eq!(d.frame, 5_009, "peer {i}");
        assert_ne!(d.local_hash, d.remote_hash);
        if i != 2 {
            assert_eq!(d.remote_seat, 2, "peer {i} blamed the wrong seat");
        }
        assert!(
            matches!(s.advance(), Step::Desynced(_)),
            "peer {i} kept running"
        );
    }
}

#[test]
fn a_dropped_player_becomes_ai_on_the_same_frame_everywhere() {
    // Reliable but jittery, as TCP is.
    let c = LinkConditions {
        input_drop_per_mille: 0,
        ..rough()
    };
    let mut run = Run::new(3, c, 5, vec![Toy::default(); 3]);
    run.run_to(3_000, 30_000);
    // Peer 2 vanishes mid-session. (Its seat is whichever its join won.)
    let gone = run.sessions[2]
        .as_ref()
        .unwrap()
        .config()
        .local_seat
        .unwrap();
    run.net.clients[2].t.close();
    run.sessions[2] = None;
    run.run_to(6_000, 60_000);
    run.assert_converged(6_000);
    for c in &run.net.clients[..2] {
        let handover = c.inbox.iter().find_map(|m| match m {
            Message::SeatController {
                seat,
                controller,
                from_frame,
            } => Some((*seat, *controller, *from_frame)),
            _ => None,
        });
        let (seat, controller, from) = handover.expect("no seat handover");
        assert_eq!((seat, controller), (gone, Controller::Ai));
        assert!((3_000..=3_000 + 4).contains(&from), "handover at {from}");
    }
    let ais: Vec<bool> = run
        .sessions
        .iter()
        .flatten()
        .map(|s| s.sim().ai[usize::from(gone)])
        .collect();
    assert_eq!(ais, vec![true, true]);
}

#[test]
fn pause_stops_everyone_and_resume_continues() {
    let c = LinkConditions {
        latency: 2,
        ..Default::default()
    };
    let mut run = Run::new(2, c, 8, vec![Toy::default(); 2]);
    run.run_to(500, 5_000);
    let me = run.net.clients[0].me.unwrap();
    run.sessions[0].as_mut().unwrap().pause(me);
    for _ in 0..200 {
        run.step(u32::MAX);
    }
    let frames: Vec<u32> = run.sessions.iter().flatten().map(|s| s.frame()).collect();
    // Paused peer 0 stopped at once; peer 1 ran out of peer 0's buffered
    // inputs within the input delay.
    assert!(frames[0] == 500 && frames[1] <= 500 + 4, "{frames:?}");
    assert!(run.sessions[1].as_ref().unwrap().is_paused());
    run.sessions[0].as_mut().unwrap().resume(me);
    run.run_to(1_500, 10_000);
    run.assert_converged(1_500);
}
