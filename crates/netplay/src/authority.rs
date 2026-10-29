//! The host-authoritative model: one player (the host) owns the state, the
//! rest send intentions and follow.
//!
//! Two shapes share it:
//!
//! - **Turn-based events** (cards, poker). A client sends an
//!   [`Message::Action`]; the host validates it against its state, turns it
//!   into an event, applies it and broadcasts [`Message::Event`] with a
//!   sequence number and the post-event state hash. Every replica applies
//!   events in sequence order, so replicas stay identical without lockstep's
//!   per-frame traffic; a hash mismatch is a desync, and a simulation that
//!   can serialise its state asks the host for a [`Message::StateBlob`] to
//!   recover.
//! - **Shared-world roaming.** The host broadcasts every avatar's position
//!   and animation state as a [`Message::Snapshot`] at 10-20 Hz; each client
//!   draws the others through an [`AvatarInterpolator`] a little in the past,
//!   so the motion between snapshots is smooth.
//!
//! Like [`crate::lockstep`], nothing here does I/O.

use std::collections::{BTreeMap, VecDeque};

use crate::protocol::{AvatarState, MAX_PAYLOAD_LEN, Message, PlayerId};
use crate::transport::{NetError, Transport, recv_or_hold};

/// A simulation advanced by discrete events rather than per-frame pads.
pub trait EventSim {
    /// Applies one event (already validated by the host) for `seat`.
    fn apply_event(&mut self, seat: u8, event: &[u8]);
    /// A platform-stable hash of the whole state.
    fn state_hash(&self) -> u64;
    /// The whole state as bytes, for a replica that fell out of step.
    /// `None` = not supported (a desync is then only reported).
    fn save_state(&self) -> Option<Vec<u8>> {
        None
    }
    /// Replaces the state with one [`Self::save_state`] produced.
    fn load_state(&mut self, _bytes: &[u8]) -> bool {
        false
    }
}

/// The host's side: validates actions and broadcasts events.
#[derive(Debug)]
pub struct AuthorityHost<S> {
    sim: S,
    /// Seat index -> its player.
    seats: Vec<Option<PlayerId>>,
    next_seq: u32,
    outbox: Vec<Message>,
}

impl<S: EventSim> AuthorityHost<S> {
    pub fn new(sim: S, seats: Vec<Option<PlayerId>>) -> Self {
        Self {
            sim,
            seats,
            next_seq: 0,
            outbox: Vec::new(),
        }
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    /// Events applied so far.
    pub fn seq(&self) -> u32 {
        self.next_seq
    }

    pub fn seat_of(&self, player: PlayerId) -> Option<u8> {
        self.seats
            .iter()
            .position(|p| *p == Some(player))
            .map(|s| s as u8)
    }

    /// Applies `event` for `seat` and queues its broadcast. For the host's
    /// own seat, AI seats and timers, and for validated client actions.
    pub fn emit(&mut self, seat: u8, event: Vec<u8>) {
        if event.len() > MAX_PAYLOAD_LEN {
            return;
        }
        self.sim.apply_event(seat, &event);
        let seq = self.next_seq;
        self.next_seq += 1;
        self.outbox.push(Message::Event {
            seq,
            seat,
            payload: event,
            hash_after: self.sim.state_hash(),
        });
    }

    /// Handles a client's action: `validate` turns it into the event to
    /// apply, or refuses it (`None`). Actions from players without a seat
    /// are refused before `validate` runs. Returns whether an event was
    /// emitted.
    pub fn handle_action(
        &mut self,
        from: PlayerId,
        action: &[u8],
        validate: impl FnOnce(&S, u8, &[u8]) -> Option<Vec<u8>>,
    ) -> bool {
        let Some(seat) = self.seat_of(from) else {
            return false;
        };
        match validate(&self.sim, seat, action) {
            Some(event) => {
                self.emit(seat, event);
                true
            }
            None => false,
        }
    }

    /// Answers a replica's state request, if the simulation can save.
    pub fn handle_state_request(&mut self, from: PlayerId) {
        if let Some(bytes) = self.sim.save_state()
            && bytes.len() <= MAX_PAYLOAD_LEN
        {
            self.outbox.push(Message::StateBlob {
                to: from,
                seq: self.next_seq,
                bytes,
            });
        }
    }

    pub fn drain_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Sends the outbox, then routes received actions through `validate`
    /// and answers state requests. Returns everything else.
    pub fn pump<T: Transport + ?Sized>(
        &mut self,
        t: &mut T,
        mut validate: impl FnMut(&S, u8, &[u8]) -> Option<Vec<u8>>,
    ) -> Result<Vec<Message>, NetError> {
        let mut other = Vec::new();
        while let Some(msg) = recv_or_hold(t, other.is_empty())? {
            match msg {
                Message::Action { from, payload } => {
                    self.handle_action(from, &payload, &mut validate);
                }
                Message::StateRequest { from } => self.handle_state_request(from),
                _ => other.push(msg),
            }
        }
        for msg in self.drain_outbox() {
            t.send(&msg)?;
        }
        Ok(other)
    }
}

/// A replica fell out of step with the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventDesync {
    pub seq: u32,
    pub local_hash: u64,
    pub host_hash: u64,
}

/// Events held for out-of-order delivery.
const MAX_BUFFERED_EVENTS: usize = 1024;

/// A non-host player's side: applies the host's events in order.
#[derive(Debug)]
pub struct AuthorityReplica<S> {
    sim: S,
    me: PlayerId,
    next_seq: u32,
    buffered: BTreeMap<u32, (u8, Vec<u8>, u64)>,
    desync: Option<EventDesync>,
    awaiting_state: bool,
    outbox: Vec<Message>,
}

impl<S: EventSim> AuthorityReplica<S> {
    pub fn new(sim: S, me: PlayerId) -> Self {
        Self {
            sim,
            me,
            next_seq: 0,
            buffered: BTreeMap::new(),
            desync: None,
            awaiting_state: false,
            outbox: Vec::new(),
        }
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    /// Events applied so far.
    pub fn seq(&self) -> u32 {
        self.next_seq
    }

    /// The current desync, cleared once a state blob restores the replica.
    pub fn desync(&self) -> Option<EventDesync> {
        self.desync
    }

    /// Asks the host for the whole state and holds events until it
    /// arrives: for a replica joining late or one that knows it is stale.
    pub fn request_state(&mut self) {
        self.awaiting_state = true;
        self.outbox.push(Message::StateRequest { from: self.me });
    }

    /// Queues an action for the host.
    pub fn send_action(&mut self, payload: Vec<u8>) {
        self.outbox.push(Message::Action {
            from: self.me,
            payload,
        });
    }

    pub fn receive(&mut self, msg: &Message) {
        match msg {
            Message::Event {
                seq,
                seat,
                payload,
                hash_after,
            } => {
                if *seq >= self.next_seq && self.buffered.len() < MAX_BUFFERED_EVENTS {
                    self.buffered
                        .insert(*seq, (*seat, payload.clone(), *hash_after));
                }
                self.apply_ready();
            }
            Message::StateBlob { to, seq, bytes }
                if *to == self.me && self.sim.load_state(bytes) =>
            {
                self.next_seq = *seq;
                self.buffered = self.buffered.split_off(&self.next_seq);
                self.desync = None;
                self.awaiting_state = false;
                self.apply_ready();
            }
            _ => {}
        }
    }

    fn apply_ready(&mut self) {
        // A desynced replica that can be restored stops applying until the
        // blob arrives; one that cannot keeps following.
        if self.awaiting_state {
            return;
        }
        while let Some((seat, payload, host_hash)) = self.buffered.remove(&self.next_seq) {
            self.sim.apply_event(seat, &payload);
            let local = self.sim.state_hash();
            if local != host_hash {
                self.desync.get_or_insert(EventDesync {
                    seq: self.next_seq,
                    local_hash: local,
                    host_hash,
                });
                if self.sim.save_state().is_some() {
                    self.awaiting_state = true;
                    self.outbox.push(Message::StateRequest { from: self.me });
                    self.next_seq += 1;
                    return;
                }
            }
            self.next_seq += 1;
        }
    }

    pub fn drain_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Sends the outbox and applies received events. Returns everything
    /// that is not event traffic.
    pub fn pump<T: Transport + ?Sized>(&mut self, t: &mut T) -> Result<Vec<Message>, NetError> {
        for msg in self.drain_outbox() {
            t.send(&msg)?;
        }
        let mut other = Vec::new();
        while let Some(msg) = recv_or_hold(t, other.is_empty())? {
            match msg {
                Message::Event { .. } | Message::StateBlob { .. } => self.receive(&msg),
                _ => other.push(msg),
            }
        }
        for msg in self.drain_outbox() {
            t.send(&msg)?;
        }
        Ok(other)
    }
}

/// Frames (60 Hz) between snapshots at `hz`, at least one.
pub fn snapshot_interval_frames(hz: u8) -> u32 {
    (60 / u32::from(hz.clamp(1, 60))).max(1)
}

/// One PSX turn in angle units.
const TURN: i32 = 4096;

/// Draws remote avatars between the host's snapshots.
///
/// The interpolator keeps its own estimate of the host's frame clock
/// (advanced by [`Self::tick`] once per local frame, re-anchored when a
/// snapshot arrives far from it) and samples each avatar `delay` frames
/// behind it, between the two snapshots that bracket that moment. Past the
/// newest snapshot it holds the last pose rather than extrapolating, so a
/// stalled link freezes an avatar instead of sending it through a wall.
#[derive(Debug, Clone)]
pub struct AvatarInterpolator {
    delay: u32,
    clock: u32,
    anchored: bool,
    tracks: BTreeMap<PlayerId, VecDeque<(u32, AvatarState)>>,
}

/// Snapshots kept per avatar.
const KEPT_SNAPSHOTS: usize = 16;

impl AvatarInterpolator {
    /// `delay` frames of buffering; two snapshot intervals rides out one
    /// late snapshot.
    pub fn new(delay: u32) -> Self {
        Self {
            delay,
            clock: 0,
            anchored: false,
            tracks: BTreeMap::new(),
        }
    }

    /// For a snapshot rate: two intervals of delay.
    pub fn for_rate(hz: u8) -> Self {
        Self::new(2 * snapshot_interval_frames(hz))
    }

    /// Advances the clock estimate one local frame.
    pub fn tick(&mut self) {
        self.clock = self.clock.wrapping_add(1);
    }

    /// Takes a snapshot. Avatars missing from it are dropped (the snapshot
    /// is the whole set).
    pub fn push_snapshot(&mut self, tick: u32, avatars: &[AvatarState]) {
        if !self.anchored || self.clock.abs_diff(tick) > 4 * self.delay.max(1) {
            self.clock = tick;
            self.anchored = true;
        }
        self.tracks
            .retain(|p, _| avatars.iter().any(|a| a.player == *p));
        for a in avatars {
            let track = self.tracks.entry(a.player).or_default();
            if track.back().is_some_and(|(t, _)| *t >= tick) {
                continue; // late or duplicate snapshot
            }
            track.push_back((tick, *a));
            while track.len() > KEPT_SNAPSHOTS {
                track.pop_front();
            }
        }
    }

    /// Every avatar's pose at the render time (`clock - delay`).
    pub fn sample(&self) -> Vec<AvatarState> {
        let t = self.clock.saturating_sub(self.delay);
        self.tracks
            .values()
            .filter_map(|track| sample_track(track, t))
            .collect()
    }
}

fn sample_track(track: &VecDeque<(u32, AvatarState)>, t: u32) -> Option<AvatarState> {
    let (first_t, first) = *track.front()?;
    if t <= first_t {
        return Some(first);
    }
    for pair in track.iter().collect::<Vec<_>>().windows(2) {
        let (t0, a) = *pair[0];
        let (t1, b) = *pair[1];
        if t >= t0 && t < t1 {
            if a.scene != b.scene {
                return Some(b);
            }
            let num = i64::from(t - t0);
            let den = i64::from(t1 - t0);
            let lerp =
                |x: i32, y: i32| (i64::from(x) + (i64::from(y) - i64::from(x)) * num / den) as i32;
            let mut d = (i32::from(b.facing) - i32::from(a.facing)).rem_euclid(TURN);
            if d > TURN / 2 {
                d -= TURN;
            }
            return Some(AvatarState {
                pos: [
                    lerp(a.pos[0], b.pos[0]),
                    lerp(a.pos[1], b.pos[1]),
                    lerp(a.pos[2], b.pos[2]),
                ],
                facing: (i32::from(a.facing) + (i64::from(d) * num / den) as i32).rem_euclid(TURN)
                    as u16,
                ..a
            });
        }
    }
    track.back().map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(player: u32, x: i32, facing: u16) -> AvatarState {
        AvatarState {
            player: PlayerId(player),
            pos: [x, 0, 0],
            facing,
            ..Default::default()
        }
    }

    #[test]
    fn interpolates_between_bracketing_snapshots() {
        let mut ip = AvatarInterpolator::new(4);
        ip.push_snapshot(0, &[at(1, 0, 4000)]);
        ip.push_snapshot(4, &[at(1, 400, 96)]);
        for _ in 0..6 {
            ip.tick();
        }
        // clock 6, render time 2: halfway.
        let s = ip.sample();
        assert_eq!(s[0].pos[0], 200);
        // 4000 -> 96 the short way round passes 0.
        assert_eq!(s[0].facing, 0);
    }

    #[test]
    fn holds_the_newest_pose_past_the_last_snapshot() {
        let mut ip = AvatarInterpolator::new(2);
        ip.push_snapshot(0, &[at(1, 0, 0)]);
        ip.push_snapshot(3, &[at(1, 30, 0)]);
        for _ in 0..20 {
            ip.tick();
        }
        assert_eq!(ip.sample()[0].pos[0], 30);
    }

    #[test]
    fn avatars_missing_from_a_snapshot_are_dropped() {
        let mut ip = AvatarInterpolator::new(0);
        ip.push_snapshot(0, &[at(1, 0, 0), at(2, 0, 0)]);
        ip.push_snapshot(1, &[at(2, 5, 0)]);
        let s = ip.sample();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].player, PlayerId(2));
    }
}
