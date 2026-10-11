//! The turn timeline: the beats one resolved turn plays back and their tick budget.

use super::*;
use legaia_engine_minigames::muscle_dome::DomePlay;

/// What one stretch of a turn's playback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatKind {
    /// The leg's first acting play walking its attacker in.
    Approach,
    /// One play's swing.
    Swing,
    /// The done band closing an attacker's string.
    Done,
}

/// One stretch of [`turn_timeline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    pub kind: BeatKind,
    /// The play it belongs to (the first play of the string for
    /// [`BeatKind::Approach`], the last for [`BeatKind::Done`]).
    pub play: usize,
    /// The acting fighter slot.
    pub attacker: usize,
    /// Tick the beat starts at, from the start of the playback.
    pub start: u32,
    pub len: u32,
}

/// A resolved turn's playback schedule: an approach ahead of the first play
/// when `closes_in` ([`MuscleDomeSession::last_turn_closes_in`]), one swing
/// per play, and a done tail after each attacker's last play.
pub fn turn_timeline(plays: &[DomePlay], closes_in: bool) -> Vec<Beat> {
    let mut out = Vec::new();
    let mut t = 0;
    let mut push = |out: &mut Vec<Beat>, kind, play, attacker, len| {
        out.push(Beat {
            kind,
            play,
            attacker,
            start: t,
            len,
        });
        t += len;
    };
    if closes_in && let Some(first) = plays.first() {
        push(
            &mut out,
            BeatKind::Approach,
            0,
            first.attacker.min(1),
            APPROACH_TICKS,
        );
    }
    for (i, p) in plays.iter().enumerate() {
        let a = p.attacker.min(1);
        push(&mut out, BeatKind::Swing, i, a, PLAY_CADENCE_TICKS);
        let string_ends = plays.get(i + 1).is_none_or(|n| n.attacker.min(1) != a);
        if string_ends {
            push(&mut out, BeatKind::Done, i, a, DONE_TAIL_TICKS);
        }
    }
    out
}

/// How long a resolved turn plays out ([`turn_timeline`]'s span). A turn with
/// no plays (an empty queue on both sides) has nothing to show and holds for
/// nothing.
pub fn turn_playback_ticks(plays: &[DomePlay], closes_in: bool) -> u32 {
    turn_timeline(plays, closes_in)
        .last()
        .map_or(0, |b| b.start + b.len)
}

/// The beat `elapsed` ticks into the playback, if any.
pub fn beat_at(plays: &[DomePlay], closes_in: bool, elapsed: u32) -> Option<Beat> {
    turn_timeline(plays, closes_in)
        .into_iter()
        .find(|b| (b.start..b.start + b.len).contains(&elapsed))
}
