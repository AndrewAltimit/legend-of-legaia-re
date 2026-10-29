//! The host-authoritative view stream's two ends (see
//! [`crate::mode_net`]): [`ViewHost`] ticks the real session from its own
//! pad and its guests' reported pads; [`ViewGuest`] reports its pad and
//! draws the host's frames.
//!
//! Neither owns the session: both act on the [`ActiveMode`] in the world's
//! mode seat, which is what the screens draw.

use std::collections::VecDeque;

use legaia_engine_modes::active::ActiveMode;
use legaia_engine_modes::session::PadWord;
use legaia_netplay::protocol::{Message, PlayerId};

use crate::mode_net::{StreamFrame, StreamedView, decode_pad, encode_pad, redact};

/// Queued pad words per guest a host holds before dropping the oldest.
const PAD_QUEUE: usize = 32;

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
        }
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
        Some(self.guests.remove(i).seat)
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
            pads[g.seat] = g.held;
        }
        let mut viewers: Vec<usize> = vec![mode.local_seat];
        viewers.extend(self.guests.iter().map(|g| g.seat));
        let mut logs = if positions.is_empty() {
            mode.tick_for(&pads, &viewers)
        } else {
            mode.tick_for_positions(&pads, positions, &viewers)
        }
        .into_iter();
        let own = logs.next().unwrap_or_default();
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
}

/// The guest end.
#[derive(Debug, Clone, Default)]
pub struct ViewGuest {
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

    /// Report the local pad (only when it changed).
    pub fn push_pad(&mut self, pad: PadWord) {
        if self.last_pad != Some(pad) {
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
