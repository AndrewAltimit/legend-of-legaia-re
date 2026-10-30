//! Party night over the network: one [`PartyNight`] per peer, directed by
//! the room's host and verified by everyone else.
//!
//! The night runs in the room's lobby. Between rounds every player is back
//! in their own field, under the party card (round intro, scoreboard, pick,
//! podium); a round is an ordinary networked mode start whose instance id
//! carries [`PARTY_PREFIX`], which every peer builds from its own copy of
//! the night ([`round_section`]) - so a round runs under the same models a
//! picked game does (lockstep for golf, racing and prop hunt, the
//! host-authoritative view stream for poker and Grid Duel).
//!
//! The host's copy moves on ([`PartyNight::tick`]); a guest's only runs its
//! timers down ([`PartyNight::tick_display`]). Every change the host makes
//! travels as a [`PartyEvent`]: the operation (a round's placements, a void,
//! a player leaving, a pick) and the night after it, with the scoreboard
//! hash in the event's `hash_after`. A guest applies the operation to its
//! own copy and compares hashes - the scoreboard every player sees is one
//! they computed and agreed on - and adopts the host's night when they
//! differ (a late joiner, a promoted host's first move).
//!
//! What a round's end is: the host's seat reaching its results (the host
//! holds them [`RESULT_HOLD`] frames, then ends the session and records the
//! placements); anything else that ends a live round - a desync, a player
//! quitting it - voids it. A player who drops mid-night has their seat
//! played by the AI and scores nothing more; their total stands. When the
//! host of a lockstep night leaves, the relay migrates the room and the
//! new host directs from its own copy; a host-authoritative round cannot
//! survive its host, so the room closes and the night ends early on the
//! standings so far, with no prizes.

use legaia_engine_modes::party::{PartyNight, PlayerKey};
use legaia_netplay::protocol::PlayerId;
use serde::{Deserialize, Serialize};

/// The instance-id prefix of a party round's start order:
/// `party:<night>:<round>:<match>`.
pub const PARTY_PREFIX: &str = "party:";
/// Frames the host leaves a finished round's results up before it ends
/// the session and records the placements.
pub const RESULT_HOLD: u32 = 3 * 60;
/// Frames the host waits for every seated player to ready up for a round
/// before it voids the round.
pub const START_TIMEOUT: u32 = 20 * 60;

/// The tag byte of a party payload in an `Action` or an `Event`.
pub const TAG: u8 = crate::mode_net::action_tag::PARTY;

/// A player id as the night keys it.
pub fn key(p: PlayerId) -> PlayerKey {
    p.0
}

/// What changed the night.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyOp {
    /// The night begins (and its first intro card).
    Begin,
    /// The clock moved the night to its next phase.
    Phase,
    /// A copy for a player who has none (a late joiner).
    Sync,
    /// The round in play finished with these placements.
    Recorded(Vec<(PlayerKey, u8)>),
    /// A Grid Duel match finished (`None`: a draw).
    Duel(Option<PlayerKey>),
    /// The round in play was voided.
    Void,
    /// A player left the night.
    Left(PlayerKey),
    /// The last-place player's pick input.
    Pick {
        key: PlayerKey,
        step: i32,
        confirm: bool,
    },
    /// The host ended the night early.
    End,
}

/// One host-to-room party message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyEvent {
    pub op: PartyOp,
    pub night: PartyNight,
}

impl PartyEvent {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![TAG];
        out.extend(serde_json::to_vec(self).expect("party events serialise"));
        out
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        match payload.split_first() {
            Some((&TAG, body)) => serde_json::from_slice(body).ok(),
            _ => None,
        }
    }
}

/// One guest-to-host party message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartyAction {
    /// Pick input (only the picker's counts).
    Pick { step: i32, confirm: bool },
}

impl PartyAction {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![TAG];
        out.extend(serde_json::to_vec(self).expect("party actions serialise"));
        out
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        match payload.split_first() {
            Some((&TAG, body)) => serde_json::from_slice(body).ok(),
            _ => None,
        }
    }
}

/// Apply `op` to `night` as the host did. `false` for an operation that
/// carries nothing to verify (the copy is adopted instead).
pub fn apply(night: &mut PartyNight, op: &PartyOp) -> bool {
    match op {
        PartyOp::Begin | PartyOp::Phase | PartyOp::Sync => false,
        PartyOp::Recorded(p) => night.record(p).is_some(),
        PartyOp::Duel(w) => {
            let before = night.history.len();
            let was_playing = night.current_slot().is_some();
            night.record_duel(*w);
            was_playing || night.history.len() != before
        }
        PartyOp::Void => night.void_round().is_some(),
        PartyOp::Left(k) => {
            night.mark_left(*k);
            true
        }
        PartyOp::Pick { key, step, confirm } => {
            night.pick(*key, *step, *confirm);
            true
        }
        PartyOp::End => {
            night.end_early();
            true
        }
    }
}

/// A round's start-order id.
pub fn round_id(night: &PartyNight) -> String {
    let played = night.hill.as_ref().map_or(0, |h| h.played);
    format!(
        "{PARTY_PREFIX}{}:{}:{played}",
        night.id,
        night.history.len()
    )
}

/// One peer's party night.
#[derive(Debug, Clone)]
pub struct PartyLink {
    pub night: PartyNight,
    /// A round of the night is in play on this machine.
    pub round_live: bool,
    /// The round in play's mode seat -> player.
    pub round_seats: Vec<Option<PlayerId>>,
    /// Host: frames the finished round has stood.
    pub finished_for: u32,
    /// Host: frames since the night cued a round it has not yet started.
    pub start_pending: Option<u32>,
    /// Host: the `start_pending` frame the start order last went out on.
    pub start_sent: Option<u32>,
    /// Guest: the phase it last readied for.
    pub readied_for: Option<(u8, usize)>,
    /// This player's prize lines, once granted (empty: no prize).
    pub granted: Option<Vec<String>>,
    /// Events whose scoreboard this copy computed and agreed with.
    pub agreed: u32,
    /// Events whose scoreboard this copy did not reach, and so adopted.
    pub resyncs: u32,
    /// Host: players that have a copy (late joiners get a sync).
    pub synced: Vec<PlayerId>,
    /// The phase last broadcast (host) or received.
    pub last_phase: (u8, usize),
    /// The pad last frame, for the pick's edges.
    pub last_pad: u16,
}

impl PartyLink {
    pub fn new(night: PartyNight) -> Self {
        let last_phase = night.phase_key();
        Self {
            night,
            round_live: false,
            round_seats: Vec::new(),
            finished_for: 0,
            start_pending: None,
            start_sent: None,
            readied_for: None,
            granted: None,
            agreed: 0,
            resyncs: 0,
            synced: Vec::new(),
            last_phase,
            last_pad: 0,
        }
    }

    /// A host event: apply its operation to this copy, check the hash, and
    /// take the host's copy (its timers, and its state should they differ).
    /// Returns whether this copy agreed.
    pub fn receive(&mut self, ev: PartyEvent, hash: u64) -> bool {
        let mut mine = self.night.clone();
        let verifiable = apply(&mut mine, &ev.op);
        let agreed = verifiable && mine.scoreboard_hash() == hash;
        if verifiable {
            if agreed {
                self.agreed += 1;
            } else {
                self.resyncs += 1;
            }
        }
        self.night = ev.night;
        self.last_phase = self.night.phase_key();
        agreed
    }
}
