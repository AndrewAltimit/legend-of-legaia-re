//! The host-authoritative view stream's two ends (see
//! [`crate::mode_net`]): [`ViewHost`] ticks the real session from its own
//! pad and its guests' reported pads; [`ViewGuest`] reports its pad and
//! draws the host's frames.
//!
//! Neither owns the session: both act on the [`ActiveMode`] in the world's
//! mode seat, which is what the screens draw.

use std::collections::VecDeque;

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::session::{AiTier, PadWord, SeatKind};
use legaia_netplay::protocol::{Message, PlayerId};

use crate::mode_net::{StreamFrame, StreamedView, decode_pad, encode_pad, redact};

/// Queued pad words per guest a host holds before dropping the oldest.
const PAD_QUEUE: usize = 32;

/// Frames a turn-based table (poker, Grid Duel) waits on one seat with no
/// press before the AI moves for it: an away player cannot stall the
/// table. Thirty seconds.
pub const TURN_LIMIT_FRAMES: u32 = 30 * 60;
/// Frames before the limit the table warns the seat.
pub const TURN_WARN_FRAMES: u32 = 10 * 60;

/// The host's turn clock over the seat a table waits on.
#[derive(Debug, Clone, Default)]
struct TurnClock {
    /// The seat being waited on, and for how many frames without a press.
    seat: Option<usize>,
    frames: u32,
    last_pad: PadWord,
    /// A seat the AI took over on its timeout, handed back once the turn
    /// moves on.
    auto: Option<usize>,
}

/// One guest seat on the host.
#[derive(Debug, Clone)]
struct GuestSeat {
    seat: usize,
    player: PlayerId,
    /// Changes not yet fed; one is taken per tick so a press and its
    /// release arriving together still give the session a pressed frame.
    queue: VecDeque<PadWord>,
    held: PadWord,
    last_sent: Option<Vec<u8>>,
    pending_log: Vec<String>,
    /// Holds no seat: watches the table (every hidden card stays hidden).
    watcher: bool,
}

/// The hosting end.
#[derive(Debug, Clone)]
pub struct ViewHost {
    seats: usize,
    host_seat: Option<usize>,
    guests: Vec<GuestSeat>,
    outbox: Vec<Message>,
    /// Frames sent, numbering each so a guest can drop a stale one.
    sent: u32,
    /// Frames a waited-on seat may sit idle before the AI moves for it
    /// ([`TURN_LIMIT_FRAMES`]; `0` = no limit).
    pub turn_limit: u32,
    clock: TurnClock,
}

impl ViewHost {
    /// `seats[i]` is mode seat `i`'s player; `me` is the host.
    pub fn new(seats: &[Option<PlayerId>], me: PlayerId) -> Self {
        let guests = seats
            .iter()
            .enumerate()
            .filter_map(|(seat, p)| match p {
                Some(p) if *p != me => Some(GuestSeat {
                    seat,
                    player: *p,
                    queue: VecDeque::new(),
                    held: 0,
                    last_sent: None,
                    pending_log: Vec::new(),
                    watcher: false,
                }),
                _ => None,
            })
            .collect();
        Self {
            seats: seats.len(),
            host_seat: seats.iter().position(|p| *p == Some(me)),
            guests,
            outbox: Vec::new(),
            sent: 0,
            turn_limit: TURN_LIMIT_FRAMES,
            clock: TurnClock::default(),
        }
    }

    /// Stream the table to `player`, who holds no seat (a party night's
    /// players outside a Grid Duel match, a late joiner). A watcher's view
    /// is redacted as nobody's seat, and its pad reaches nothing.
    pub fn watch(&mut self, player: PlayerId) {
        if self.guests.iter().any(|g| g.player == player) {
            return;
        }
        self.guests.push(GuestSeat {
            seat: usize::MAX,
            player,
            queue: VecDeque::new(),
            held: 0,
            last_sent: None,
            pending_log: Vec::new(),
            watcher: true,
        });
    }

    /// A guest's action. Returns `true` when it was a pad report.
    pub fn receive_action(&mut self, from: PlayerId, payload: &[u8]) -> bool {
        let Some(pad) = decode_pad(payload) else {
            return false;
        };
        if let Some(g) = self.guests.iter_mut().find(|g| g.player == from) {
            g.queue.push_back(pad);
            while g.queue.len() > PAD_QUEUE {
                g.queue.pop_front();
            }
        }
        true
    }

    /// A guest left: its seat stops reading pads (the session's seat goes
    /// to the AI through `ActiveMode::set_seat_kind`, which the caller does).
    pub fn drop_player(&mut self, player: PlayerId) -> Option<usize> {
        let i = self.guests.iter().position(|g| g.player == player)?;
        let g = self.guests.remove(i);
        (!g.watcher).then_some(g.seat)
    }

    /// One frame: tick `mode` from the host's pad and each guest's, then
    /// queue a frame for every guest whose view changed. Returns the host's
    /// own log lines.
    pub fn tick(&mut self, mode: &mut ActiveMode, local_pad: PadWord) -> Vec<String> {
        self.tick_with_positions(mode, local_pad, &[])
    }

    /// [`Self::tick`] with host-authored seat positions (a field prop hunt:
    /// `ActiveMode::tick_for_positions`).
    pub fn tick_with_positions(
        &mut self,
        mode: &mut ActiveMode,
        local_pad: PadWord,
        positions: &[Option<legaia_engine_modes::fixed::Vec2>],
    ) -> Vec<String> {
        let mut pads = vec![0; self.seats];
        if let Some(s) = self.host_seat {
            pads[s] = local_pad;
        }
        for g in &mut self.guests {
            if let Some(p) = g.queue.pop_front() {
                g.held = p;
            }
            if let Some(slot) = pads.get_mut(g.seat) {
                *slot = g.held;
            }
        }
        let mut viewers: Vec<usize> = vec![mode.viewer_seat()];
        viewers.extend(self.guests.iter().map(|g| g.seat));
        let timer = self.turn_clock(mode, &pads);
        let mut logs = if positions.is_empty() {
            mode.tick_for(&pads, &viewers)
        } else {
            mode.tick_for_positions(&pads, positions, &viewers)
        }
        .into_iter();
        let mut own = logs.next().unwrap_or_default();
        let mut logs: Vec<Vec<String>> = logs.collect();
        if let Some((seat, line)) = timer {
            let names = mode.seat_names.clone();
            let word = |me: usize| {
                let n = legaia_engine_modes::active::Namer { me, names: &names };
                line(&n.name(seat), seat == me)
            };
            own.insert(0, word(viewers[0]));
            for (log, &v) in logs.iter_mut().zip(&viewers[1..]) {
                log.insert(0, word(v));
            }
        }
        let logs = logs.into_iter();
        let results = mode.results();
        let view = StreamedView::from_view(mode.view());
        for (g, log) in self.guests.iter_mut().zip(logs) {
            g.pending_log.extend(log);
            let Some(v) = view.clone() else { continue };
            let frame = StreamFrame {
                view: redact(v, g.seat),
                log: Vec::new(),
                results: results.clone(),
            };
            let key = frame.encode();
            if g.last_sent.as_ref() == Some(&key) && g.pending_log.is_empty() {
                continue;
            }
            let frame = StreamFrame {
                log: std::mem::take(&mut g.pending_log),
                ..frame
            };
            self.sent += 1;
            self.outbox.push(Message::StateBlob {
                to: g.player,
                seq: self.sent,
                bytes: frame.encode(),
            });
            g.last_sent = Some(key);
        }
        own
    }

    pub fn drain_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether `seat` is a player's (the host's or a seated guest's).
    fn player_seat(&self, seat: usize) -> bool {
        self.host_seat == Some(seat) || self.guests.iter().any(|g| !g.watcher && g.seat == seat)
    }

    /// Run the turn clock before a tick: hand a timed-out seat back once
    /// its turn has moved on, count the frames the waited-on player seat
    /// sits without a press, warn it, and at the limit give its move to the
    /// AI. Returns the seat and the line to word for each viewer (`name`,
    /// whether the viewer is that seat).
    #[allow(clippy::type_complexity)]
    fn turn_clock(
        &mut self,
        mode: &mut ActiveMode,
        pads: &[PadWord],
    ) -> Option<(usize, fn(&str, bool) -> String)> {
        let waiting = mode.waiting_on();
        if let Some(s) = self.clock.auto
            && waiting != Some(s)
        {
            self.clock.auto = None;
            if self.player_seat(s) {
                mode.set_seat_kind(s, SeatKind::Human);
            }
        }
        let Some(s) = waiting
            .filter(|&s| self.turn_limit > 0 && self.player_seat(s) && !mode.seat_kind(s).is_ai())
        else {
            self.clock.seat = None;
            self.clock.frames = 0;
            return None;
        };
        let pad = pads.get(s).copied().unwrap_or(0);
        let pressed = pad & !self.clock.last_pad != 0;
        self.clock.last_pad = pad;
        if self.clock.seat != Some(s) || pressed {
            self.clock.seat = Some(s);
            self.clock.frames = 0;
            return None;
        }
        self.clock.frames += 1;
        let warn_at = self.turn_limit.saturating_sub(TURN_WARN_FRAMES);
        if self.clock.frames == warn_at && warn_at > 0 {
            return Some((s, |who, you| {
                if you {
                    "10 seconds left to move!".to_string()
                } else {
                    format!("{who}: 10 seconds left")
                }
            }));
        }
        if self.clock.frames >= self.turn_limit {
            mode.set_seat_kind(s, SeatKind::Ai(AiTier::Regular));
            self.clock.auto = Some(s);
            self.clock.seat = None;
            self.clock.frames = 0;
            return Some((s, |who, you| {
                if you {
                    "Out of time - the AI moves for you".to_string()
                } else {
                    format!("{who} ran out of time")
                }
            }));
        }
        None
    }
}

/// The guest end.
#[derive(Debug, Clone, Default)]
pub struct ViewGuest {
    /// Holds no seat: draws the table, reports no pad.
    pub watching: bool,
    last_pad: Option<PadWord>,
    /// The newest frame number drawn.
    seq: u32,
    outbox: Vec<Message>,
    /// Frames received.
    pub frames: u32,
}

impl ViewGuest {
    pub fn new() -> Self {
        Self::default()
    }

    /// A guest that watches the table from no seat.
    pub fn watcher() -> Self {
        Self {
            watching: true,
            ..Self::default()
        }
    }

    /// Report the local pad (only when it changed).
    pub fn push_pad(&mut self, pad: PadWord) {
        if !self.watching && self.last_pad != Some(pad) {
            self.last_pad = Some(pad);
            self.outbox.push(Message::Action {
                from: PlayerId(0),
                payload: encode_pad(pad),
            });
        }
    }

    /// Host frame `seq`, decoded, for a guest that draws it itself (a field
    /// prop hunt keeps no session of its own). `None` for a stale or
    /// malformed frame.
    pub fn receive_frame(&mut self, seq: u32, bytes: &[u8]) -> Option<StreamFrame> {
        if seq <= self.seq {
            return None;
        }
        let frame = StreamFrame::decode(bytes)?;
        self.seq = seq;
        self.frames += 1;
        Some(frame)
    }

    /// Host frame `seq`: draw it on `mode`. Returns its log lines; a frame
    /// older than one already drawn is dropped.
    pub fn receive(
        &mut self,
        mode: &mut ActiveMode,
        seq: u32,
        bytes: &[u8],
    ) -> Option<Vec<String>> {
        if seq <= self.seq {
            return None;
        }
        let frame = StreamFrame::decode(bytes)?;
        self.seq = seq;
        self.frames += 1;
        mode.set_remote_view(Some(frame.view.into_view()), frame.results);
        Some(frame.log)
    }

    pub fn drain_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }
}
