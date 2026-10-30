//! Shared fixtures: the Rebirth pack's mode and card sections, a scripted
//! pad generator, and a runner that records a session's hash sequence.

use std::path::PathBuf;

use legaia_engine_modes::session::{ModeSession, PadWord};
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::modes::ModeSection;

pub fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth")
}

pub fn rebirth_modes() -> ModeSection {
    let text = std::fs::read_to_string(pack_dir().join("modes.toml")).unwrap();
    toml::from_str(&text).unwrap()
}

pub fn rebirth_cards() -> CardSection {
    let text = std::fs::read_to_string(pack_dir().join("cards.toml")).unwrap();
    toml::from_str(&text).unwrap()
}

/// A deterministic pad stream independent of any session: each frame,
/// each seat holds a pseudo-random subset of `mask`.
pub struct PadScript {
    state: u64,
    mask: PadWord,
}

impl PadScript {
    pub fn new(seed: u64, mask: PadWord) -> Self {
        Self {
            state: seed ^ 0x5DEE_CE66_D1CE_4E5B,
            mask,
        }
    }

    pub fn next(&mut self, seats: usize) -> Vec<PadWord> {
        (0..seats)
            .map(|_| {
                self.state = self
                    .state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((self.state >> 33) as PadWord) & self.mask
            })
            .collect()
    }
}

/// Every button but Start / Select.
pub const PLAY_MASK: PadWord = 0xFFF0;

/// Tick `s` until it finishes or `max_frames`, recording the state hash
/// after every tick. Panics if the session does not finish in time.
pub fn run<S: ModeSession>(mut s: S, script: &mut PadScript, max_frames: usize) -> (Vec<u64>, S) {
    let mut hashes = Vec::new();
    for _ in 0..max_frames {
        let pads = script.next(s.seats());
        s.tick(&pads);
        hashes.push(s.state_hash());
        if s.is_finished() {
            // A finished session stays finished.
            s.tick(&script.next(s.seats()));
            assert!(s.is_finished());
            assert!(s.results().is_some());
            return (hashes, s);
        }
    }
    panic!("session did not finish within {max_frames} frames");
}
