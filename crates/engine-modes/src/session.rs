//! The contract every mode session keeps.
//!
//! A mode is a [`ModeSession`]: a pure function of its config, its seed and
//! the per-seat pad words fed to [`ModeSession::tick`] once per frame. Two
//! machines that build the same session and feed it the same pad words in
//! the same order hold the same state after every tick, which is what makes
//! a session replayable from an input log and runnable in lockstep over a
//! network: peers exchange only pad words and compare
//! [`ModeSession::state_hash`] to catch a desync.
//!
//! What a session may not do, so that holds:
//!
//! - read a clock, a global RNG or any host state - randomness comes from
//!   the session's own [`ModeRng`], seeded by the config;
//! - keep floats in state - physics is 20.12 fixed point
//!   ([`crate::fixed`]), whose results are the same on every target;
//! - iterate a hash map - state uses `Vec` / `BTreeMap` only.
//!
//! A seat is either a human (its pad word drives it) or an AI
//! ([`SeatKind::Ai`]); an AI seat ignores its pad word and decides from
//! the session's RNG, so it is deterministic too. A host draws from
//! [`ModeSession::view`], a read-only snapshot; it never mutates the
//! session except through `tick`.

use legaia_engine_system::pad::PadButton;
use serde::{Deserialize, Serialize};

/// One seat's pad word for one frame, in the **raw** libpad layout
/// ([`PadButton`]).
pub type PadWord = u16;

/// The most seats any mode accepts.
pub const MAX_SEATS: usize = 8;

/// How hard an AI seat plays.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum AiTier {
    /// Loose and forgiving.
    Novice,
    /// Sound, readable play.
    #[default]
    Regular,
    /// Plays close to the best the kernel knows.
    Expert,
}

impl From<legaia_expansion_format::modes::AiTier> for AiTier {
    fn from(t: legaia_expansion_format::modes::AiTier) -> Self {
        use legaia_expansion_format::modes::AiTier as F;
        match t {
            F::Novice => Self::Novice,
            F::Regular => Self::Regular,
            F::Expert => Self::Expert,
        }
    }
}

/// Who drives a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SeatKind {
    Human,
    Ai(AiTier),
}

impl SeatKind {
    pub fn is_ai(self) -> bool {
        matches!(self, Self::Ai(_))
    }

    fn hash_code(self) -> u8 {
        match self {
            Self::Human => 0,
            Self::Ai(t) => 1 + t as u8,
        }
    }
}

/// One finishing position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub seat: usize,
    /// 1-based; tied seats share a place.
    pub place: u8,
    /// The mode's own score (chips, strokes, captures, points, finish
    /// frame).
    pub score: i64,
}

/// The currency a mode settles in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    /// Party gold.
    Gold,
    /// Casino coins.
    Coins,
}

impl From<legaia_expansion_format::modes::Currency> for Currency {
    fn from(c: legaia_expansion_format::modes::Currency) -> Self {
        match c {
            legaia_expansion_format::modes::Currency::Gold => Self::Gold,
            legaia_expansion_format::modes::Currency::Coins => Self::Coins,
        }
    }
}

/// Something a seat takes away from a finished session. The host grants
/// it to the owning save; the session only names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reward {
    /// A net change in a currency (negative = lost).
    Currency {
        seat: usize,
        currency: Currency,
        amount: i64,
    },
    /// One booster pack from a pack's card section.
    Booster { seat: usize, booster: String },
    /// One card by id.
    Card { seat: usize, card: String },
}

/// The outcome of a finished session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeResults {
    /// Every seat, best first.
    pub placements: Vec<Placement>,
    pub rewards: Vec<Reward>,
}

impl ModeResults {
    /// Rank `scores` (one per seat) into placements, ties sharing a place.
    /// `lower_is_better` for golf strokes and race times.
    pub fn rank(scores: &[i64], lower_is_better: bool) -> Vec<Placement> {
        let mut order: Vec<usize> = (0..scores.len()).collect();
        order.sort_by(|&a, &b| {
            let ord = scores[a].cmp(&scores[b]);
            let ord = if lower_is_better { ord } else { ord.reverse() };
            ord.then(a.cmp(&b))
        });
        let mut out: Vec<Placement> = Vec::with_capacity(order.len());
        for (i, &seat) in order.iter().enumerate() {
            let place = match out.last() {
                Some(prev) if prev.score == scores[seat] => prev.place,
                _ => (i + 1) as u8,
            };
            out.push(Placement {
                seat,
                place,
                score: scores[seat],
            });
        }
        out
    }
}

/// The shape every mode session shares.
pub trait ModeSession {
    /// What one tick reports to the host (sounds, banners, log lines).
    type Event;
    /// The read-only snapshot a host draws from.
    type View;

    /// Seats in the session, humans and AI.
    fn seats(&self) -> usize;
    /// Who drives `seat`.
    fn seat_kind(&self, seat: usize) -> SeatKind;
    /// Advance one frame. `pads[i]` is seat `i`'s pad word; a short slice
    /// reads as released pads, and AI seats ignore theirs.
    fn tick(&mut self, pads: &[PadWord]) -> Vec<Self::Event>;
    /// The host's draw snapshot.
    fn view(&self) -> Self::View;
    /// A digest of every field the rules read. Equal on two peers after the
    /// same inputs; a mismatch is a desync.
    fn state_hash(&self) -> u64;
    /// The session has ended; further ticks change nothing.
    fn is_finished(&self) -> bool;
    /// Placements and rewards once [`Self::is_finished`].
    fn results(&self) -> Option<ModeResults>;
}

/// Per-seat press-edge tracking over raw pad words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PadEdges {
    held: [PadWord; MAX_SEATS],
    pressed: [PadWord; MAX_SEATS],
}

impl PadEdges {
    /// Latch this frame's words. Seats past `pads.len()` read as released.
    pub fn update(&mut self, pads: &[PadWord]) {
        for seat in 0..MAX_SEATS {
            let now = pads.get(seat).copied().unwrap_or(0);
            self.pressed[seat] = now & !self.held[seat];
            self.held[seat] = now;
        }
    }

    /// `button` went down this frame.
    pub fn pressed(&self, seat: usize, button: PadButton) -> bool {
        seat < MAX_SEATS && self.pressed[seat] & button.mask() != 0
    }

    /// `button` is down this frame.
    pub fn held(&self, seat: usize, button: PadButton) -> bool {
        seat < MAX_SEATS && self.held[seat] & button.mask() != 0
    }

    pub fn hash_into(&self, h: &mut StateHasher) {
        for w in self.held {
            h.u16(w);
        }
    }
}

/// The session-owned deterministic generator: SplitMix64. Fast, integer
/// only, and the same sequence on every target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeRng {
    state: u64,
}

impl ModeRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `0..n` (`n > 0`), rejection-sampled so there is no
    /// modulo bias.
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        if n <= 1 {
            return 0;
        }
        let zone = u32::MAX - (u32::MAX % n);
        loop {
            let v = self.next_u32();
            if v < zone {
                return v % n;
            }
        }
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// `true` with probability `pct / 100`.
    pub fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Pick an index weighted by `weights`; `None` when they sum to zero.
    pub fn weighted(&mut self, weights: &[u32]) -> Option<usize> {
        let total: u64 = weights.iter().map(|&w| w as u64).sum();
        if total == 0 {
            return None;
        }
        let total = total.min(u32::MAX as u64) as u32;
        let mut roll = self.below(total);
        for (i, &w) in weights.iter().enumerate() {
            if roll < w {
                return Some(i);
            }
            roll -= w;
        }
        Some(weights.len() - 1)
    }

    pub fn hash_into(&self, h: &mut StateHasher) {
        h.u64(self.state);
    }
}

/// FNV-1a 64 over explicitly written fields. Stable across Rust versions
/// and targets, unlike `std`'s `DefaultHasher`.
#[derive(Debug, Clone)]
pub struct StateHasher(u64);

impl Default for StateHasher {
    fn default() -> Self {
        Self(0xCBF2_9CE4_8422_2325)
    }
}

impl StateHasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01B3);
        }
    }

    pub fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }

    pub fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }

    pub fn str(&mut self, s: &str) {
        self.usize(s.len());
        self.bytes(s.as_bytes());
    }

    pub fn seat(&mut self, k: SeatKind) {
        self.u8(k.hash_code());
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_reproducible_and_bounded() {
        let mut a = ModeRng::new(42);
        let mut b = ModeRng::new(42);
        for _ in 0..1000 {
            let v = a.below(7);
            assert_eq!(v, b.below(7));
            assert!(v < 7);
        }
        // Known first output pins the algorithm across refactors.
        assert_eq!(ModeRng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn weighted_respects_zero_weights() {
        let mut r = ModeRng::new(1);
        for _ in 0..500 {
            assert_ne!(r.weighted(&[3, 0, 5]), Some(1));
        }
        assert_eq!(r.weighted(&[0, 0]), None);
    }

    #[test]
    fn pad_edges_latch_presses_once() {
        let mut e = PadEdges::default();
        let x = PadButton::Cross.mask();
        e.update(&[x]);
        assert!(e.pressed(0, PadButton::Cross));
        e.update(&[x]);
        assert!(!e.pressed(0, PadButton::Cross));
        assert!(e.held(0, PadButton::Cross));
        e.update(&[]);
        assert!(!e.held(0, PadButton::Cross));
    }

    #[test]
    fn rank_shares_places_on_ties() {
        let p = ModeResults::rank(&[30, 28, 30, 35], true);
        let places: Vec<(usize, u8)> = p.iter().map(|p| (p.seat, p.place)).collect();
        assert_eq!(places, vec![(1, 1), (0, 2), (2, 2), (3, 4)]);
        let p = ModeResults::rank(&[5, 9], false);
        assert_eq!(p[0].seat, 1);
    }

    #[test]
    fn fnv_is_stable() {
        let mut h = StateHasher::new();
        h.bytes(b"a");
        assert_eq!(h.finish(), 0xAF63_DC4C_8601_EC8C);
    }
}
