//! A game mode over the network: which simulation model each kind runs
//! under, building one instance identically on every peer, the lockstep
//! adapter, and the host-authoritative view stream.
//!
//! **Lockstep** (golf, racing, prop hunt): every peer builds the same
//! [`ActiveMode`] from the start order's instance id and seed and ticks it
//! from every seat's pad word ([`ModeSim`] is its
//! [`Simulation`](legaia_netplay::lockstep::Simulation)). Nothing but pad
//! words and checkpoint hashes travels, and a mismatched hash is a desync.
//!
//! **Host-authoritative view stream** (poker, Grid Duel): only the host
//! runs the session. A guest sends its pad word when it changes
//! ([`encode_pad`]) and the host feeds it to that seat; after every tick
//! the host sends each guest a [`StreamFrame`] - the table as that guest
//! may see it plus the tick's log lines worded for it - whenever the frame
//! changed. A poker guest therefore never holds another seat's hole cards
//! before the showdown ([`redact`]), which lockstep cannot promise: under
//! lockstep every peer holds the whole deal, and a modified client could
//! read it. The price is a round trip of input latency, which a turn-based
//! table does not notice.

use legaia_engine_modes::active::{ActiveMode, BuildError, BuildOptions, ModeKind, ModeView};
use legaia_engine_modes::cards::duel::DuelView;
use legaia_engine_modes::poker::{Phase, PokerView};
use legaia_engine_modes::session::{AiTier, ModeResults, PadWord, SeatKind};
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::modes::ModeSection;
use legaia_netplay::lockstep::{LockstepConfig, Simulation};
use legaia_netplay::protocol::{Controller, PlayerId, SimModel, StartGame};
use serde::{Deserialize, Serialize};

/// Frames between sampling a pad and simulating it under lockstep.
pub const INPUT_DELAY: u8 = 3;
/// Frames between lockstep checkpoints.
pub const CHECKPOINT_INTERVAL: u16 = 60;
/// The roaming snapshot rate a host-authoritative start order names.
pub const SNAPSHOT_HZ: u8 = 15;

/// The simulation model a mode kind runs under.
pub fn model_for(kind: ModeKind) -> SimModel {
    match kind {
        ModeKind::Poker | ModeKind::CardDuel => SimModel::HostAuthoritative {
            snapshot_hz: SNAPSHOT_HZ,
        },
        ModeKind::Golf | ModeKind::Race | ModeKind::PropHunt => SimModel::Lockstep {
            input_delay: INPUT_DELAY,
            checkpoint_interval: CHECKPOINT_INTERVAL,
        },
    }
}

/// Why a start order could not be turned into a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetModeError {
    Build(BuildError),
    /// The start order's model is not the one this build runs the kind
    /// under (a peer of another build would simulate differently).
    WrongModel,
}

impl std::fmt::Display for NetModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build(e) => e.fmt(f),
            Self::WrongModel => f.write_str("the host runs this mode under another model"),
        }
    }
}

/// One instance built from a start order, as `me` sees it.
#[derive(Debug, Clone)]
pub struct NetMode {
    pub mode: ActiveMode,
    /// Mode seat -> its player (`None` = the AI).
    pub seats: Vec<Option<PlayerId>>,
    /// `me`'s seat; `None` = spectating.
    pub local_seat: Option<usize>,
    pub model: SimModel,
}

impl NetMode {
    /// Build `start`'s instance from `modes` / `cards`. The first room seats
    /// fill the mode's seats in order; a mode seat with no player is the
    /// AI, and a player whose room seat the mode does not have spectates.
    /// Golf, which seats any number, seats the whole room.
    ///
    /// Under lockstep every peer gets the same session; only
    /// [`ActiveMode::local_seat`] differs. Under the view stream only `host`
    /// holds the real session: a guest's copy is built from another seed and
    /// shows a blank table until the host's first frame, so it never holds
    /// the real deal.
    pub fn build(
        modes: &ModeSection,
        cards: &CardSection,
        start: &StartGame,
        me: PlayerId,
        host: PlayerId,
    ) -> Result<Self, NetModeError> {
        let room_seats = start.seats.len().max(1);
        let opts = BuildOptions {
            player_deck: None,
            golf_ai: vec![AiTier::Regular; room_seats - 1],
        };
        let guest_copy = matches!(start.model, SimModel::HostAuthoritative { .. }) && me != host;
        let seed = if guest_copy { !start.seed } else { start.seed };
        let mut mode = ActiveMode::build(modes, cards, &start.mode, seed, &opts)
            .map_err(NetModeError::Build)?;
        if model_for(mode.kind()) != start.model {
            return Err(NetModeError::WrongModel);
        }
        if guest_copy && let Some(v) = StreamedView::from_view(mode.view()) {
            mode.set_remote_view(Some(redact(v, usize::MAX).into_view()), None);
        }
        let seats: Vec<Option<PlayerId>> = (0..mode.seats())
            .map(|s| start.seats.get(s).copied().flatten())
            .collect();
        for (s, p) in seats.iter().enumerate() {
            match p {
                Some(_) => mode.set_seat_kind(s, SeatKind::Human),
                // A built human seat (seat 0) with nobody in it plays
                // itself; built AI seats keep the instance's tier.
                None if !mode.seat_kind(s).is_ai() => {
                    mode.set_seat_kind(s, SeatKind::Ai(AiTier::Regular))
                }
                None => {}
            }
        }
        let local_seat = seats.iter().position(|p| *p == Some(me));
        mode.local_seat = local_seat.unwrap_or(0);
        Ok(Self {
            mode,
            seats,
            local_seat,
            model: start.model,
        })
    }

    /// The lockstep configuration for this instance, or `None` when it runs
    /// host-authoritative.
    pub fn lockstep_config(&self) -> Option<LockstepConfig> {
        let SimModel::Lockstep {
            input_delay,
            checkpoint_interval,
        } = self.model
        else {
            return None;
        };
        Some(LockstepConfig {
            input_delay: u32::from(input_delay),
            checkpoint_interval: u32::from(checkpoint_interval),
            ai_seats: (0..self.seats.len() as u8)
                .filter(|&s| self.seats[usize::from(s)].is_none())
                .collect(),
            ..LockstepConfig::new(self.seats.len(), self.local_seat.map(|s| s as u8))
        })
    }
}

/// An [`ActiveMode`] as a lockstep [`Simulation`]. The log lines each tick
/// produces collect in [`Self::log`] for the host to show.
#[derive(Debug, Clone)]
pub struct ModeSim {
    pub mode: Box<ActiveMode>,
    pub log: Vec<String>,
}

impl ModeSim {
    pub fn new(mode: ActiveMode) -> Self {
        Self {
            mode: Box::new(mode),
            log: Vec::new(),
        }
    }
}

impl Simulation for ModeSim {
    fn tick(&mut self, inputs: &[u16]) {
        let lines = self.mode.tick(inputs);
        self.log.extend(lines);
    }

    fn state_hash(&self) -> u64 {
        self.mode.state_hash()
    }

    fn set_seat_controller(&mut self, seat: usize, controller: Controller) {
        let kind = match controller {
            Controller::Human(_) => SeatKind::Human,
            Controller::Ai => SeatKind::Ai(AiTier::Regular),
        };
        self.mode.set_seat_kind(seat, kind);
    }
}

/// The table as a guest draws it. Built once per frame and serialised, so
/// the duel's larger variant is not boxed (as [`ModeView`]).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamedView {
    Poker(PokerView),
    Duel(DuelView),
}

impl StreamedView {
    /// The streamable part of `view` (`None` for a lockstep kind).
    pub fn from_view(view: ModeView) -> Option<Self> {
        match view {
            ModeView::Poker(v) => Some(Self::Poker(v)),
            ModeView::Duel(v) => Some(Self::Duel(v)),
            _ => None,
        }
    }

    pub fn into_view(self) -> ModeView {
        match self {
            Self::Poker(v) => ModeView::Poker(v),
            Self::Duel(v) => ModeView::Duel(v),
        }
    }
}

/// What a host sends one guest after a tick that changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFrame {
    pub view: StreamedView,
    /// The tick's log lines, worded for the guest's seat.
    pub log: Vec<String>,
    /// The session's results once it has ended.
    pub results: Option<ModeResults>,
}

impl StreamFrame {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("stream frames serialise")
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// `view` as the player in `seat` may see it: a poker table hides every
/// other seat's hole cards until the hand reaches its showdown, and then
/// shows only the hands still live (the ones a real table turns over).
/// Grid Duel is played with open hands and passes through.
pub fn redact(view: StreamedView, seat: usize) -> StreamedView {
    match view {
        StreamedView::Poker(mut v) => {
            let showdown = matches!(v.phase, Phase::Showdown { .. } | Phase::Finished);
            for (i, s) in v.seats.iter_mut().enumerate() {
                let shown = i == seat || (showdown && s.in_hand && !s.folded);
                if !shown {
                    s.hole = None;
                }
            }
            StreamedView::Poker(v)
        }
        v @ StreamedView::Duel(_) => v,
    }
}

/// Action payload tags: one byte ahead of the body, so the lobby's roaming
/// reports and a mode's pad words share the `Action` message.
pub mod action_tag {
    /// A guest's own avatar (lobby roaming); body = [`crate::roam::encode_avatar`].
    pub const AVATAR: u8 = 0x01;
    /// A guest's pad word (host-authoritative mode); body = `u16 LE`.
    pub const PAD: u8 = 0x02;
}

/// A pad-word action payload.
pub fn encode_pad(pad: PadWord) -> Vec<u8> {
    let [lo, hi] = pad.to_le_bytes();
    vec![action_tag::PAD, lo, hi]
}

/// The pad word in a pad action payload.
pub fn decode_pad(payload: &[u8]) -> Option<PadWord> {
    match payload {
        [action_tag::PAD, lo, hi] => Some(u16::from_le_bytes([*lo, *hi])),
        _ => None,
    }
}
