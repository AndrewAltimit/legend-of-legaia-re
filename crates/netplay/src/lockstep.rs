//! Delay-based lockstep: every peer runs the same deterministic simulation
//! from every seat's pad word, frame by frame.
//!
//! A peer samples its pad for frame `f + input_delay` while simulating frame
//! `f`, and sends it to the others. Frame `f` is simulated only once every
//! human seat's word for `f` is known; until then [`LockstepSession::advance`]
//! stalls. Frames `0..input_delay` run on zero pads everywhere, so nobody
//! waits at the start. The delay hides that much one-way latency; beyond it
//! the slowest link sets the pace, which is the price of needing no
//! snapshot.
//!
//! Every `checkpoint_interval` frames each peer hashes its state
//! ([`Simulation::state_hash`]) and sends the hash; a mismatch against any
//! peer's hash for the same frame is a desync and is reported with the
//! frame, both hashes and the peer's seat.
//!
//! The session does no I/O. It queues outgoing messages
//! ([`LockstepSession::drain_outbox`]) and takes incoming ones
//! ([`LockstepSession::receive`]); [`LockstepSession::pump`] joins it to a
//! [`Transport`].
//!
//! ## Rollback (not implemented)
//!
//! Rollback would predict missing inputs (repeat the last word), simulate
//! ahead, and on a misprediction restore a snapshot and re-simulate. It
//! needs `fn snapshot(&self) -> S::Snapshot` and `fn restore(&mut self, &S::Snapshot)`
//! on [`Simulation`], cheap enough to take every frame. The game-mode
//! sessions could provide that; a whole `World` cannot (it is large and
//! holds scene resources), which is why the core is delay-based. A rollback
//! session would keep this module's wire format unchanged - input packets
//! already carry frame numbers - and add a `RollbackSimulation` trait beside
//! [`Simulation`] rather than widening it.

use std::collections::{BTreeMap, VecDeque};

use crate::protocol::{
    Controller, InputPacket, MAX_REDUNDANCY, MAX_SEATS, Message, PlayerId, SimModel, StartGame,
};
use crate::transport::{NetError, Transport, recv_or_hold};

/// A deterministic simulation a lockstep session can drive: a game-mode
/// session or a whole `World`.
pub trait Simulation {
    /// Advances one frame. `inputs[seat]` is that seat's pad word; an
    /// AI-controlled seat's word is `0` and should be ignored.
    fn tick(&mut self, inputs: &[u16]);
    /// A hash of the whole simulated state. Equal states must hash equal on
    /// every platform (no pointer or `HashMap`-order dependence).
    fn state_hash(&self) -> u64;
    /// `seat` changes hands from the next tick on. Called at the same frame
    /// on every peer.
    fn set_seat_controller(&mut self, _seat: usize, _controller: Controller) {}
}

/// Frames ahead of the simulation the session buffers inputs for. A peer
/// cannot legitimately send further ahead than twice the input delay.
const INPUT_WINDOW: u32 = 1024;
/// Local checkpoint hashes kept for comparison against late peers.
const KEPT_CHECKPOINTS: usize = 64;
/// Remote checkpoint hashes held for frames not simulated yet.
const MAX_PENDING_REMOTE: usize = 1024;

/// How a session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockstepConfig {
    /// Seats in the session (pads per tick).
    pub seats: usize,
    /// This peer's seat; `None` = spectator (sends nothing, still verifies).
    pub local_seat: Option<u8>,
    /// Seats the AI drives from frame 0.
    pub ai_seats: Vec<u8>,
    /// Frames between sampling a pad and simulating it.
    pub input_delay: u32,
    /// Past frames repeated in every input packet (`1..=MAX_REDUNDANCY`).
    pub redundancy: usize,
    /// Frames between checkpoints; `0` disables them.
    pub checkpoint_interval: u32,
    /// Consecutive stalled [`LockstepSession::advance`] calls before the
    /// latest input packet is sent again (covers a lost packet when the
    /// stall stops new ones being sent).
    pub resend_after_stalls: u32,
    /// Stop advancing once a desync is seen.
    pub halt_on_desync: bool,
}

impl LockstepConfig {
    pub fn new(seats: usize, local_seat: Option<u8>) -> Self {
        Self {
            seats,
            local_seat,
            ai_seats: Vec::new(),
            input_delay: 3,
            redundancy: 8,
            checkpoint_interval: 60,
            resend_after_stalls: 4,
            halt_on_desync: true,
        }
    }

    /// The configuration a [`StartGame`] describes, from `me`'s side.
    /// `None` when the start order is not a lockstep one.
    pub fn from_start(start: &StartGame, me: PlayerId) -> Option<Self> {
        let SimModel::Lockstep {
            input_delay,
            checkpoint_interval,
        } = start.model
        else {
            return None;
        };
        let local_seat = start
            .seats
            .iter()
            .position(|p| *p == Some(me))
            .map(|s| s as u8);
        Some(Self {
            input_delay: u32::from(input_delay),
            checkpoint_interval: u32::from(checkpoint_interval),
            ai_seats: (0..start.seats.len() as u8)
                .filter(|&s| start.seats[usize::from(s)].is_none())
                .collect(),
            ..Self::new(start.seats.len(), local_seat)
        })
    }
}

/// Two peers' states differ at a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesyncReport {
    /// The frame whose post-tick state was hashed.
    pub frame: u32,
    pub local_hash: u64,
    pub remote_seat: u8,
    pub remote_hash: u64,
}

/// A peer broke the protocol (the message is ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    /// An input for a seat outside the session.
    BadSeat(u8),
    /// An input for a frame beyond the buffering window.
    TooFarAhead { seat: u8, frame: u32 },
    /// Two different words for the same seat and frame.
    ConflictingInput { seat: u8, frame: u32 },
    /// A controller change for a frame already simulated.
    LateControllerChange { seat: u8, frame: u32 },
}

/// What one [`LockstepSession::advance`] call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Simulated `frame`.
    Advanced {
        frame: u32,
    },
    /// Waiting on these seats' inputs for the next frame.
    Stalled {
        waiting_on: Vec<u8>,
    },
    Paused,
    /// Halted on a desync ([`LockstepConfig::halt_on_desync`]).
    Desynced(DesyncReport),
}

/// A lockstep session over simulation `S`.
#[derive(Debug)]
pub struct LockstepSession<S> {
    sim: S,
    config: LockstepConfig,
    /// The next frame to simulate.
    frame: u32,
    /// Per seat, the words for frames `frame..`; `None` = not yet known.
    inputs: Vec<VecDeque<Option<u16>>>,
    ai: Vec<bool>,
    /// Controller changes not reached yet: `(from_frame, seat, controller)`.
    pending_controllers: Vec<(u32, u8, Controller)>,
    /// The frame the next local pad is for.
    local_input_frame: u32,
    /// The last `redundancy` local words, ending at `local_input_frame - 1`.
    sent: VecDeque<u16>,
    local_hashes: VecDeque<(u32, u64)>,
    remote_hashes: BTreeMap<u32, Vec<(u8, u64)>>,
    desync: Option<DesyncReport>,
    verified: u32,
    paused: bool,
    stalled_calls: u32,
    outbox: Vec<Message>,
    violations: Vec<Violation>,
}

impl<S: Simulation> LockstepSession<S> {
    pub fn new(sim: S, mut config: LockstepConfig) -> Self {
        config.seats = config.seats.clamp(1, MAX_SEATS);
        config.redundancy = config.redundancy.clamp(1, MAX_REDUNDANCY);
        let delay = config.input_delay;
        let inputs = (0..config.seats)
            .map(|_| (0..delay).map(|_| Some(0)).collect())
            .collect();
        let mut ai = vec![false; config.seats];
        let mut sim = sim;
        for &s in &config.ai_seats {
            if let Some(slot) = ai.get_mut(usize::from(s)) {
                *slot = true;
                sim.set_seat_controller(usize::from(s), Controller::Ai);
            }
        }
        Self {
            sim,
            frame: 0,
            inputs,
            ai,
            pending_controllers: Vec::new(),
            local_input_frame: delay,
            sent: VecDeque::new(),
            local_hashes: VecDeque::new(),
            remote_hashes: BTreeMap::new(),
            desync: None,
            verified: 0,
            paused: false,
            stalled_calls: 0,
            outbox: Vec::new(),
            violations: Vec::new(),
            config,
        }
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    pub fn into_sim(self) -> S {
        self.sim
    }

    pub fn config(&self) -> &LockstepConfig {
        &self.config
    }

    /// The next frame to simulate (= frames simulated so far).
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// The first desync seen, if any.
    pub fn desync(&self) -> Option<DesyncReport> {
        self.desync
    }

    /// Remote checkpoints that matched the local hash.
    pub fn verified_checkpoints(&self) -> u32 {
        self.verified
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Protocol violations seen so far (the offending data was ignored).
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    /// Whether the local pad should be sampled this frame: there is a local
    /// seat, the session is not paused, and the local pad for
    /// `frame + input_delay` is not in yet.
    pub fn wants_local_input(&self) -> bool {
        self.config.local_seat.is_some()
            && !self.paused
            && self.local_input_frame <= self.frame + self.config.input_delay
    }

    /// Records the local pad for the next input frame and queues the input
    /// packet. Returns `false` (and records nothing) when
    /// [`Self::wants_local_input`] is false.
    pub fn push_local_input(&mut self, pad: u16) -> bool {
        if !self.wants_local_input() {
            return false;
        }
        let seat = self
            .config
            .local_seat
            .expect("checked by wants_local_input");
        let f = self.local_input_frame;
        self.store(seat, f, pad);
        self.local_input_frame += 1;
        self.sent.push_back(pad);
        while self.sent.len() > self.config.redundancy {
            self.sent.pop_front();
        }
        self.outbox.push(self.latest_packet(seat));
        true
    }

    fn latest_packet(&self, seat: u8) -> Message {
        Message::Input(InputPacket {
            seat,
            first_frame: self.local_input_frame - self.sent.len() as u32,
            pads: self.sent.iter().copied().collect(),
        })
    }

    fn store(&mut self, seat: u8, f: u32, pad: u16) {
        let slot = (f - self.frame) as usize;
        let q = &mut self.inputs[usize::from(seat)];
        if q.len() <= slot {
            q.resize(slot + 1, None);
        }
        match q[slot] {
            Some(old) if old != pad => self
                .violations
                .push(Violation::ConflictingInput { seat, frame: f }),
            _ => q[slot] = Some(pad),
        }
    }

    /// Takes one incoming message. Messages that are not lockstep traffic
    /// are left to the caller (see [`Self::pump`]).
    pub fn receive(&mut self, msg: &Message) {
        match msg {
            Message::Input(p) => self.receive_input(p),
            Message::Checkpoint { seat, frame, hash } => {
                self.receive_checkpoint(*seat, *frame, *hash)
            }
            Message::SeatController {
                seat,
                controller,
                from_frame,
            } => {
                if usize::from(*seat) >= self.config.seats {
                    self.violations.push(Violation::BadSeat(*seat));
                } else if *from_frame < self.frame {
                    self.violations.push(Violation::LateControllerChange {
                        seat: *seat,
                        frame: *from_frame,
                    });
                } else {
                    self.pending_controllers
                        .push((*from_frame, *seat, *controller));
                }
            }
            Message::Pause { .. } => self.paused = true,
            Message::Resume { .. } => self.paused = false,
            _ => {}
        }
    }

    fn receive_input(&mut self, p: &InputPacket) {
        if usize::from(p.seat) >= self.config.seats {
            self.violations.push(Violation::BadSeat(p.seat));
            return;
        }
        if Some(p.seat) == self.config.local_seat {
            return;
        }
        for (i, &pad) in p.pads.iter().enumerate() {
            let f = p.first_frame + i as u32;
            if f < self.frame {
                continue;
            }
            if f >= self.frame + INPUT_WINDOW {
                self.violations.push(Violation::TooFarAhead {
                    seat: p.seat,
                    frame: f,
                });
                return;
            }
            self.store(p.seat, f, pad);
        }
    }

    fn receive_checkpoint(&mut self, seat: u8, frame: u32, hash: u64) {
        if let Some(&(_, local)) = self.local_hashes.iter().find(|(f, _)| *f == frame) {
            self.compare(frame, local, seat, hash);
        } else if frame >= self.frame {
            let pending: usize = self.remote_hashes.values().map(Vec::len).sum();
            if pending < MAX_PENDING_REMOTE {
                self.remote_hashes
                    .entry(frame)
                    .or_default()
                    .push((seat, hash));
            }
        }
    }

    fn compare(&mut self, frame: u32, local: u64, seat: u8, remote: u64) {
        if local == remote {
            self.verified += 1;
        } else if self.desync.is_none() {
            self.desync = Some(DesyncReport {
                frame,
                local_hash: local,
                remote_seat: seat,
                remote_hash: remote,
            });
        }
    }

    /// Pauses locally and tells the room.
    pub fn pause(&mut self, me: PlayerId) {
        self.paused = true;
        self.outbox.push(Message::Pause { by: me });
    }

    pub fn resume(&mut self, me: PlayerId) {
        self.paused = false;
        self.outbox.push(Message::Resume { by: me });
    }

    /// Simulates the next frame if every input for it is known.
    pub fn advance(&mut self) -> Step {
        if let (Some(d), true) = (self.desync, self.config.halt_on_desync) {
            return Step::Desynced(d);
        }
        if self.paused {
            return Step::Paused;
        }
        let f = self.frame;
        // Controller changes reached this frame take effect before the
        // readiness check: an AI seat is never waited on.
        let mut i = 0;
        while i < self.pending_controllers.len() {
            let (from, seat, ctrl) = self.pending_controllers[i];
            if from <= f {
                self.pending_controllers.swap_remove(i);
                self.ai[usize::from(seat)] = ctrl == Controller::Ai;
                self.sim.set_seat_controller(usize::from(seat), ctrl);
            } else {
                i += 1;
            }
        }

        let waiting_on: Vec<u8> = (0..self.config.seats)
            .filter(|&s| !self.ai[s] && self.inputs[s].front().copied().flatten().is_none())
            .map(|s| s as u8)
            .collect();
        if !waiting_on.is_empty() {
            self.stalled_calls += 1;
            if let Some(seat) = self.config.local_seat
                && !self.sent.is_empty()
                && self.config.resend_after_stalls > 0
                && self
                    .stalled_calls
                    .is_multiple_of(self.config.resend_after_stalls)
            {
                self.outbox.push(self.latest_packet(seat));
            }
            return Step::Stalled { waiting_on };
        }
        self.stalled_calls = 0;

        let pads: Vec<u16> = (0..self.config.seats)
            .map(|s| {
                let word = self.inputs[s].pop_front().flatten().unwrap_or(0);
                if self.ai[s] { 0 } else { word }
            })
            .collect();
        self.sim.tick(&pads);
        self.frame += 1;

        let interval = self.config.checkpoint_interval;
        if interval > 0 && (f + 1).is_multiple_of(interval) {
            let hash = self.sim.state_hash();
            self.local_hashes.push_back((f, hash));
            while self.local_hashes.len() > KEPT_CHECKPOINTS {
                self.local_hashes.pop_front();
            }
            if let Some(seat) = self.config.local_seat {
                self.outbox.push(Message::Checkpoint {
                    seat,
                    frame: f,
                    hash,
                });
            }
            if let Some(remote) = self.remote_hashes.remove(&f) {
                for (seat, h) in remote {
                    self.compare(f, hash, seat, h);
                }
            }
        }
        // Remote hashes for frames we passed without a checkpoint of our
        // own (a mismatched interval) can never be compared.
        while let Some((&first, _)) = self.remote_hashes.first_key_value() {
            if first >= self.frame {
                break;
            }
            self.remote_hashes.pop_first();
        }
        Step::Advanced { frame: f }
    }

    /// Outgoing messages queued since the last drain.
    pub fn drain_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Sends the outbox over `t` and feeds everything `t` has received into
    /// the session. Returns the received messages that are not lockstep
    /// traffic (lobby, chat, room events) for the host to handle.
    pub fn pump<T: Transport + ?Sized>(&mut self, t: &mut T) -> Result<Vec<Message>, NetError> {
        for msg in self.drain_outbox() {
            t.send(&msg)?;
        }
        let mut other = Vec::new();
        while let Some(msg) = recv_or_hold(t, other.is_empty())? {
            match msg {
                Message::Input(_)
                | Message::Checkpoint { .. }
                | Message::SeatController { .. }
                | Message::Pause { .. }
                | Message::Resume { .. } => {
                    self.receive(&msg);
                    if matches!(
                        msg,
                        Message::SeatController { .. }
                            | Message::Pause { .. }
                            | Message::Resume { .. }
                    ) {
                        other.push(msg);
                    }
                }
                _ => other.push(msg),
            }
        }
        Ok(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct Sum(u64);

    impl Simulation for Sum {
        fn tick(&mut self, inputs: &[u16]) {
            for (i, &p) in inputs.iter().enumerate() {
                self.0 = self.0.wrapping_mul(31).wrapping_add(u64::from(p) << i);
            }
        }
        fn state_hash(&self) -> u64 {
            self.0
        }
    }

    #[test]
    fn delay_frames_run_without_inputs_then_stall() {
        let mut s = LockstepSession::new(Sum::default(), LockstepConfig::new(2, Some(0)));
        // Frames 0..3 need no input; the local pads pushed meanwhile are
        // for frames 3..6.
        for f in 0..3 {
            assert!(s.push_local_input(f as u16));
            assert!(
                !s.push_local_input(9),
                "one local input per simulated frame"
            );
            assert_eq!(s.advance(), Step::Advanced { frame: f });
        }
        assert!(s.push_local_input(3));
        assert_eq!(
            s.advance(),
            Step::Stalled {
                waiting_on: vec![1]
            }
        );
        assert!(!s.push_local_input(9), "stalled: no input runs ahead");
        s.receive(&Message::Input(InputPacket {
            seat: 1,
            first_frame: 3,
            pads: vec![9],
        }));
        assert_eq!(s.advance(), Step::Advanced { frame: 3 });
    }

    #[test]
    fn ai_seat_is_never_waited_on() {
        let mut cfg = LockstepConfig::new(2, Some(0));
        cfg.input_delay = 0;
        let mut s = LockstepSession::new(Sum::default(), cfg);
        s.receive(&Message::SeatController {
            seat: 1,
            controller: Controller::Ai,
            from_frame: 0,
        });
        assert!(s.push_local_input(1));
        assert_eq!(s.advance(), Step::Advanced { frame: 0 });
    }

    #[test]
    fn conflicting_input_is_a_violation() {
        let mut s = LockstepSession::new(Sum::default(), LockstepConfig::new(2, Some(0)));
        for pads in [vec![1, 2], vec![1, 3]] {
            s.receive(&Message::Input(InputPacket {
                seat: 1,
                first_frame: 3,
                pads,
            }));
        }
        assert_eq!(
            s.violations(),
            &[Violation::ConflictingInput { seat: 1, frame: 4 }]
        );
    }
}
