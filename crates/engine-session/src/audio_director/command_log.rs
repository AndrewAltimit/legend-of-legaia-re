//! The director's command record: every command a host's routing hands the
//! director, as issued, folded into a running digest.
//!
//! What sounds depends on the output (a voice keys only while one is free, a
//! CD-XA clip only once its span is staged), so two hosts with different
//! outputs can sound different things from the same commands. What the host
//! **asks** for depends on the host's routing alone. This is the ask: the BGM
//! commands, the SFX cues enqueued and those that came due (keyed or not), the
//! retail-ring cues that came due, the immediate blips, the CD-XA clip and
//! arts-shout requests. Two hosts fed one pad script must issue the same
//! sequence; `host_trace` compares the digest frame by frame.

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// One command the director received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCommand {
    /// A BGM start (op `0x35`), by id, before the director's re-emit dedupe.
    BgmStart(u16),
    /// A BGM start that stages its own VAB (a global-pool track).
    BgmStartOwned(u16),
    BgmPause,
    BgmResume,
    BgmStop,
    BgmUnhaltPause,
    /// A cue queued on the delay scheduler: `(id, frames)`.
    SfxEnqueue(u16, u16),
    /// A queued cue that came due this frame, keyed or not.
    SfxDue(u16),
    /// A retail-ring cue that came due this frame, keyed or not.
    RingDue(i16),
    /// A cue fired at once, off the scheduler (a UI blip).
    SfxNow(u16),
    /// The sustained-cue teardown (`FUN_80017910`).
    SfxReleaseSustained,
    /// A CD-XA clip request: `(clip slot, channel, duration sectors)`.
    XaClip(u32, u32, u32),
    /// An arts-voice shout request: `(character slot, action)`.
    Shout(u8, u8),
    /// The CD input's reverb send turned on or off (`SPUCNT` bit 2), logged
    /// on the edge.
    CdReverb(bool),
}

/// How many recent commands [`AudioCommandLog::recent`] keeps for a report.
const RECENT: usize = 32;

/// The running record (see the module docs).
#[derive(Debug, Clone, Default)]
pub struct AudioCommandLog {
    digest: u64,
    count: u64,
    recent: VecDeque<AudioCommand>,
}

impl AudioCommandLog {
    /// Fold one command in.
    pub fn push(&mut self, cmd: AudioCommand) {
        let mut h = DefaultHasher::new();
        self.digest.hash(&mut h);
        cmd.hash(&mut h);
        self.digest = h.finish();
        self.count += 1;
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(cmd);
    }

    /// The digest of every command so far, in order.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// How many commands have been folded in.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The last few commands, oldest first.
    pub fn recent(&self) -> impl Iterator<Item = &AudioCommand> {
        self.recent.iter()
    }
}
