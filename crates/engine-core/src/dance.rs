//! Clean-room **Noa dance (rhythm) minigame** rules engine.
//!
//! A faithful port of the dance overlay's per-frame rhythm logic - the beat
//! clock, the timing-window hit judge, and the scoring / groove-gauge model -
//! driven by the already-parsed step chart ([`legaia_asset::dance_chart`]). This
//! is the *rules* layer: it consumes pad-direction presses and produces judged
//! results + a running score, exactly as the retail overlay does. The visible
//! dance-floor / arrow rendering (which reuses the field scene infrastructure)
//! is a separate host concern and is not covered here.
//!
//! Every constant and formula below is the **Confirmed** reading from
//! [`docs/subsystems/minigame-dance.md`](../../../docs/subsystems/minigame-dance.md);
//! see the citations on each item. The one piece deliberately *not* reproduced
//! is the exact sequence-bonus magnitude (retail scales the per-lane value table
//! `DAT_801d41a4` by the accuracy weight): those table values are disc-resident
//! and unmapped, so [`Judge::Sequence`] surfaces the accuracy weight and the
//! confirmed tier increment, and a caller that wants the exact bonus supplies
//! the table. No Sony bytes are baked in.
//!
//! Chain: retail `FUN_801cf470` (beat clock, state 10) → `FUN_801d1820` (chart
//! lookup) → `FUN_801d1960` (hit judge) → `FUN_801d1af4` (score / award).

use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart};

/// Beat period in phase units (`FUN_801d1960`'s `0x119` divisor): one beat slot
/// spans this many phase units. `phase % PERIOD` = intra-beat phase,
/// `phase / PERIOD` = beat index.
pub const BEAT_PERIOD: u32 = 0x119;

/// Acceptance-window width inside a beat slot (`0xd2`). An intra-beat phase past
/// this is the dead zone between beats - no note is active and a press misses.
pub const BEAT_WINDOW: u32 = 0xd2;

/// The beat phase counter wraps at this value (`FUN_801cf470` beat clock).
pub const BEAT_PHASE_WRAP: u32 = 0x2320;

/// Per-frame phase advance = `frame_delta * PHASE_PER_DELTA` (`DAT_1f800393 * 10`
/// in the retail beat clock, framerate-compensated).
pub const PHASE_PER_DELTA: u32 = 10;

/// Peak accuracy weight (dead-on the beat). The weight ramps `0..=0x1000`,
/// maximal at phase 0 and decaying to 0 at the window edge.
pub const ACCURACY_MAX: u32 = 0x1000;

/// Song-length limit for the short mode (`FUN_801cf470` song-end test).
pub const SONG_LEN_SHORT: u32 = 0x41dc;
/// Song-length limit for the long mode.
pub const SONG_LEN_LONG: u32 = 0x64fc;

/// Per-player score clamp (`0x3e7`).
pub const SCORE_MAX: u32 = 999;

/// Groove-gauge step per success and its clamp ceiling. `gauge / GAUGE_STEP`
/// selects the chart row (difficulty lane), so crossing a step promotes the
/// dancer to a denser, higher-scoring row.
pub const GAUGE_STEP: u32 = 1000;
/// Groove-gauge clamp ceiling (`[0, 2999]`).
pub const GAUGE_MAX: u32 = 2999;

/// Score multiplier for an ordinary on-beat hit (`(lane + 1) * 3`).
pub const MULT_ORDINARY: u32 = 3;
/// Score multiplier for a combo hit on a 4-beat boundary with streak `>= 2`
/// (`(lane + 1) * 0x19`).
pub const MULT_COMBO: u32 = 0x19;
/// Score multiplier for a Perfect combo (streak `< 2` on the boundary)
/// (`(lane + 1) * 0x22`); also raises the retail Perfect banner flag.
pub const MULT_PERFECT: u32 = 0x22;

/// Solo-style win threshold the results state compares the score against
/// (`0x12d`).
pub const WIN_THRESHOLD_SOLO: u32 = 300;

/// One of the three judged pad directions. The retail judge compares the chart
/// symbol against `(pressed & 0xf) + 1`, so direction index `d` matches chart
/// symbol `d + 1`; [`DanceChart`] stores symbols `1`/`2`/`3` (`FUN_801d4040`
/// maps them to pad bits `0x80`/`0x20`/`0x10`). Which physical d-pad direction
/// each bit is is Inferred in the RE, so this stays at the bit level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceDir {
    /// Chart symbol `1` (pad bit `0x80`).
    A = 0,
    /// Chart symbol `2` (pad bit `0x20`).
    B = 1,
    /// Chart symbol `3` (pad bit `0x10`).
    C = 2,
}

impl DanceDir {
    /// The chart symbol this direction matches (`index + 1`).
    pub fn symbol(self) -> u8 {
        self as u8 + 1
    }

    /// The pad-mask bit for this direction (`FUN_801d4040`).
    pub fn pad_bit(self) -> u16 {
        match self {
            DanceDir::A => 0x80,
            DanceDir::B => 0x20,
            DanceDir::C => 0x10,
        }
    }
}

/// The chart symbol → pad-mask bit map (`FUN_801d4040`): `1 → 0x80`, `2 → 0x20`,
/// `3 → 0x10`, anything else `0`.
// PORT: FUN_801d4040 (chart symbol -> pad-mask bit)
pub fn symbol_pad_bit(symbol: u8) -> u16 {
    match symbol {
        1 => 0x80,
        2 => 0x20,
        3 => 0x10,
        _ => 0,
    }
}

/// The result of judging a press (`FUN_801d1960`'s three-way return).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judge {
    /// Outside the window or wrong direction (return 0).
    Miss,
    /// Correct direction inside the window - a single matched note (return 1).
    /// `weight` is the `0..=0x1000` accuracy weight (peaks on the beat).
    Hit { weight: u32 },
    /// A hit that also completed the lane's chart cursor (return 2). Carries the
    /// accuracy weight; the exact per-lane bonus magnitude is disc-resident (see
    /// the module docs) and left to the caller.
    Sequence { weight: u32 },
}

/// A single dancer's live rhythm state (the human player, port slot 0).
#[derive(Debug, Clone)]
pub struct DanceGame {
    chart: DanceChart,
    /// Beat phase counter (`DAT_801d581c`); wraps at [`BEAT_PHASE_WRAP`].
    phase: u32,
    /// Total-song timer (`DAT_801d5820`).
    song_timer: u32,
    /// Song-length limit this run ends at.
    song_len: u32,
    /// Running score (`DAT_801d53cc[0]`), clamped to [`SCORE_MAX`].
    score: u32,
    /// Groove gauge (`DAT_801d544c[0]`), clamped to `[0, GAUGE_MAX]`.
    gauge: u32,
    /// Per-player chart cursor (`DAT_801d550c[0]`), advanced on each matched
    /// note; completing `lane + 1` matches is a sequence.
    cursor: u32,
    /// Consecutive-success streak (drives the combo vs Perfect tier split).
    streak: u32,
}

impl DanceGame {
    /// Start a run on `chart`; `long_song` selects the long song-length limit.
    pub fn new(chart: DanceChart, long_song: bool) -> Self {
        Self {
            chart,
            phase: 0,
            song_timer: 0,
            song_len: if long_song {
                SONG_LEN_LONG
            } else {
                SONG_LEN_SHORT
            },
            score: 0,
            gauge: 0,
            cursor: 0,
            streak: 0,
        }
    }

    /// Parse the baked step chart out of the dance overlay image (PROT 0980) and
    /// start a run. `None` when the chart doesn't decode (see
    /// [`legaia_asset::dance_chart::parse`]).
    pub fn from_overlay(overlay: &[u8], long_song: bool) -> Option<Self> {
        Some(Self::new(
            legaia_asset::dance_chart::parse(overlay)?,
            long_song,
        ))
    }

    /// The current difficulty lane (`gauge / GAUGE_STEP`), clamped to the chart's
    /// row count. The groove gauge selects which (denser) chart row is active.
    pub fn lane(&self) -> usize {
        let rows = self.chart.rows.len().max(1);
        ((self.gauge / GAUGE_STEP) as usize).min(rows - 1)
    }

    /// Intra-beat phase (`phase % BEAT_PERIOD`).
    pub fn intra_beat_phase(&self) -> u32 {
        self.phase % BEAT_PERIOD
    }

    /// Beat index (`phase / BEAT_PERIOD`).
    pub fn beat_index(&self) -> u32 {
        self.phase / BEAT_PERIOD
    }

    /// `true` when the intra-beat phase is in the dead zone (past the window) -
    /// no note is active, presses miss.
    pub fn in_dead_zone(&self) -> bool {
        self.intra_beat_phase() > BEAT_WINDOW
    }

    /// Running score.
    pub fn score(&self) -> u32 {
        self.score
    }

    /// Groove gauge.
    pub fn gauge(&self) -> u32 {
        self.gauge
    }

    /// `true` once the song timer has reached this run's length limit.
    pub fn song_over(&self) -> bool {
        self.song_timer >= self.song_len
    }

    /// Final solo-style grade: `true` (pass) when the score meets
    /// [`WIN_THRESHOLD_SOLO`].
    pub fn passed(&self) -> bool {
        self.score >= WIN_THRESHOLD_SOLO
    }

    /// Advance the beat clock by `frame_delta` frames (`FUN_801cf470` state 10):
    /// steps the beat phase (wrapping) and the song timer.
    // PORT: FUN_801cf470 (beat clock + song-end test, states 10..12)
    pub fn advance(&mut self, frame_delta: u32) {
        let step = frame_delta * PHASE_PER_DELTA;
        self.phase = (self.phase + step) % BEAT_PHASE_WRAP;
        // The song timer saturates at the length limit (the retail clock keeps
        // counting but the run ends; clamping keeps `song_over` monotone).
        self.song_timer = self.song_timer.saturating_add(step).min(self.song_len);
    }

    /// The accuracy weight for the current phase (`FUN_801d1960`:
    /// `0x1000 - phase * 0x1000 / 0xd2`), `0` in the dead zone. Peaks at the beat
    /// and decays to `0` at the window edge.
    pub fn accuracy_weight(&self) -> u32 {
        let p = self.intra_beat_phase();
        if p > BEAT_WINDOW {
            return 0;
        }
        ACCURACY_MAX - (p * ACCURACY_MAX) / BEAT_WINDOW
    }

    /// The chart symbol that should be pressed on the current beat
    /// (`FUN_801d1820`): `None` in the dead zone; symbol `3` on every 4th beat
    /// (the held-sequence slot); otherwise the chart byte for the active lane +
    /// beat. `0` means "no step on this beat".
    // PORT: FUN_801d1820 (chart lookup - the display/auto-feed half incl. held-sequence)
    pub fn required_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let beat = self.beat_index();
        if beat & 3 == 3 {
            return Some(3);
        }
        let col = (beat as usize) % BEATS_PER_ROW;
        self.chart.symbol(self.lane(), col)
    }

    /// The chart symbol the **hit judge** (`FUN_801d1960`) matches a press
    /// against for the current lane + beat: `None` in the dead zone, `Some(0)`
    /// when the beat carries no note, else the direction symbol. This is the
    /// judge's source and can differ from [`Self::required_symbol`] (the display
    /// / auto-feed source, `FUN_801d1820`, which substitutes the held-sequence
    /// symbol on every 4th beat) - the retail split between "what to press" and
    /// "what is judged".
    // PORT: FUN_801d1820 (chart lookup - the judged-cell half)
    pub fn judged_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let col = (self.beat_index() as usize) % BEATS_PER_ROW;
        Some(self.chart.symbol(self.lane(), col).unwrap_or(0))
    }

    /// Judge a directional press (`FUN_801d1960` + the `FUN_801d1af4` award).
    /// Returns the three-way [`Judge`] and applies the score / gauge / streak
    /// side effects. A miss floors the gauge and breaks the streak.
    // PORT: FUN_801d1960 (hit judge: dead-zone + accuracy weight + direction match)
    pub fn judge_press(&mut self, dir: DanceDir) -> Judge {
        // Dead zone: outside the acceptance window is always a miss.
        if self.in_dead_zone() {
            self.on_miss();
            return Judge::Miss;
        }
        let weight = self.accuracy_weight();
        let beat = self.beat_index();
        let col = (beat as usize) % BEATS_PER_ROW;
        let want = self.chart.symbol(self.lane(), col).unwrap_or(0);
        // Wrong direction (or no note this beat) -> miss.
        if want == 0 || want != dir.symbol() {
            self.on_miss();
            return Judge::Miss;
        }
        // Matched: advance the cursor; completing `lane + 1` matches is a
        // sequence (the retail `cursor + 1 == lane + 1` test).
        self.cursor += 1;
        let lane = self.lane() as u32;
        // Retail completes the lane's chart when `cursor + 1 == lane + 1`; after
        // the increment above that is exactly `cursor > lane`.
        let sequence = self.cursor > lane;
        if sequence {
            self.cursor = 0;
        }
        self.award_hit(beat, lane);
        if sequence {
            Judge::Sequence { weight }
        } else {
            Judge::Hit { weight }
        }
    }

    /// Apply a successful hit's score + gauge (`FUN_801d1af4`): the tier
    /// multiplier is picked by the 4-beat-boundary + streak rule, scaled by
    /// `(lane + 1)`, added to the `999`-clamped score; the gauge steps `+1000`
    /// (clamped) and the streak grows.
    // PORT: FUN_801d1af4 (score / groove-gauge award)
    fn award_hit(&mut self, beat: u32, lane: u32) {
        let on_boundary = beat & 3 == 3;
        let mult = if on_boundary {
            if self.streak >= 2 {
                MULT_COMBO
            } else {
                MULT_PERFECT
            }
        } else {
            MULT_ORDINARY
        };
        let gain = (lane + 1) * mult;
        self.score = (self.score + gain).min(SCORE_MAX);
        self.gauge = (self.gauge + GAUGE_STEP).min(GAUGE_MAX);
        self.streak += 1;
    }

    /// A miss floors the gauge and breaks the streak (`FUN_801d1af4` miss path).
    fn on_miss(&mut self) {
        self.gauge = 0;
        self.streak = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::dance_chart::BEATS_PER_ROW;

    /// A 3-row chart with a known step layout for judging.
    fn chart() -> DanceChart {
        let mut rows = Vec::new();
        for lane in 0..3u8 {
            let mut row = [0u8; BEATS_PER_ROW];
            // Beat 0 of every lane wants symbol 1 (DanceDir::A).
            row[0] = 1;
            // Beat 1 wants symbol 2 in lane 0, symbol 1 elsewhere.
            row[1] = if lane == 0 { 2 } else { 1 };
            rows.push(row);
        }
        DanceChart { rows }
    }

    #[test]
    fn constants_match_the_re() {
        assert_eq!(BEAT_PERIOD, 0x119);
        assert_eq!(BEAT_WINDOW, 0xd2);
        assert_eq!(BEAT_PHASE_WRAP, 0x2320);
        assert_eq!((MULT_ORDINARY, MULT_COMBO, MULT_PERFECT), (3, 25, 34));
        assert_eq!((SCORE_MAX, GAUGE_MAX, GAUGE_STEP), (999, 2999, 1000));
        assert_eq!(WIN_THRESHOLD_SOLO, 300);
    }

    #[test]
    fn symbol_pad_bit_map() {
        assert_eq!(symbol_pad_bit(1), 0x80);
        assert_eq!(symbol_pad_bit(2), 0x20);
        assert_eq!(symbol_pad_bit(3), 0x10);
        assert_eq!(symbol_pad_bit(0), 0);
        assert_eq!(DanceDir::A.symbol(), 1);
        assert_eq!(DanceDir::C.pad_bit(), 0x10);
    }

    #[test]
    fn accuracy_weight_peaks_on_beat_and_decays_to_edge() {
        let mut g = DanceGame::new(chart(), false);
        // Dead on the beat (phase 0) -> peak weight.
        assert_eq!(g.accuracy_weight(), ACCURACY_MAX);
        // At the window edge (phase == 0xd2) -> zero weight, still not dead zone.
        g.phase = BEAT_WINDOW;
        assert_eq!(g.accuracy_weight(), 0);
        assert!(!g.in_dead_zone());
        // Past the edge -> dead zone, zero weight.
        g.phase = BEAT_WINDOW + 1;
        assert!(g.in_dead_zone());
        assert_eq!(g.accuracy_weight(), 0);
    }

    #[test]
    fn beat_clock_wraps_and_ends_song() {
        let mut g = DanceGame::new(chart(), false);
        g.advance(1);
        assert_eq!(g.phase, PHASE_PER_DELTA);
        assert_eq!(g.beat_index(), 0);
        // Enough frames to end the short song.
        for _ in 0..2000 {
            g.advance(1);
        }
        assert!(g.song_over());
        // Phase stays within the wrap.
        assert!(g.phase < BEAT_PHASE_WRAP);
    }

    #[test]
    fn dead_zone_press_misses_and_floors_gauge() {
        let mut g = DanceGame::new(chart(), false);
        g.gauge = 1500;
        g.streak = 3;
        g.phase = BEAT_WINDOW + 5; // dead zone
        assert_eq!(g.judge_press(DanceDir::A), Judge::Miss);
        assert_eq!(g.gauge, 0);
        assert_eq!(g.streak, 0);
    }

    #[test]
    fn correct_direction_scores_wrong_direction_misses() {
        // On beat 0, lane 0 wants symbol 1 = DanceDir::A.
        let mut g = DanceGame::new(chart(), false);
        assert_eq!(g.required_symbol(), Some(1));
        // Wrong direction -> miss.
        assert_eq!(g.judge_press(DanceDir::B), Judge::Miss);
        // Correct direction: lane 0 needs cursor+1 == 1 -> immediate sequence.
        assert!(matches!(g.judge_press(DanceDir::A), Judge::Sequence { .. }));
        // Ordinary hit (beat 0 is not a 4-beat boundary): (lane 0 + 1) * 3.
        assert_eq!(g.score(), MULT_ORDINARY);
        // Success stepped the gauge.
        assert_eq!(g.gauge(), GAUGE_STEP);
    }

    #[test]
    fn combo_boundary_tiers_split_on_streak() {
        // Put the clock on a 4-beat boundary (beat index 3) at phase 0.
        let mut g = DanceGame::new(chart(), false);
        g.phase = 3 * BEAT_PERIOD;
        assert_eq!(g.beat_index(), 3);
        // On a boundary the required symbol is the held-sequence 3 (DanceDir::C),
        // but the judge matches against the raw chart cell, so set the chart cell
        // at beat 3 to symbol 1 for a deterministic match.
        g.chart.rows[0][3] = 1;
        // Streak < 2 -> Perfect tier (lane 0 -> *34).
        assert!(matches!(g.judge_press(DanceDir::A), Judge::Sequence { .. }));
        assert_eq!(g.score(), MULT_PERFECT);
        // Build the streak up and hit another boundary -> Combo tier (*25).
        g.score = 0;
        g.streak = 2;
        g.gauge = 0; // lane 0
        g.phase = 3 * BEAT_PERIOD;
        assert!(matches!(g.judge_press(DanceDir::A), Judge::Sequence { .. }));
        assert_eq!(g.score(), MULT_COMBO);
    }

    #[test]
    fn gauge_promotes_lane_and_score_clamps() {
        let mut g = DanceGame::new(chart(), false);
        // Gauge in the second step selects lane 1.
        g.gauge = 1500;
        assert_eq!(g.lane(), 1);
        // Max gauge selects the top row (lane 2), never past the row count.
        g.gauge = GAUGE_MAX;
        assert_eq!(g.lane(), 2);
        // Score saturates at 999.
        g.score = SCORE_MAX - 1;
        g.phase = 0;
        g.gauge = 0;
        let _ = g.judge_press(DanceDir::A);
        assert_eq!(g.score(), SCORE_MAX);
    }

    #[test]
    fn required_symbol_holds_sequence_on_fourth_beat() {
        let mut g = DanceGame::new(chart(), false);
        // Every 4th beat surfaces the held-sequence symbol 3 regardless of chart.
        g.phase = 3 * BEAT_PERIOD;
        assert_eq!(g.required_symbol(), Some(3));
        // Dead zone -> no active note.
        g.phase = 3 * BEAT_PERIOD + BEAT_WINDOW + 1;
        assert_eq!(g.required_symbol(), None);
    }

    #[test]
    fn pass_threshold() {
        let mut g = DanceGame::new(chart(), false);
        assert!(!g.passed());
        g.score = WIN_THRESHOLD_SOLO;
        assert!(g.passed());
    }
}
