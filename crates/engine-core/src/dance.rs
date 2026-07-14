//! Clean-room **Noa dance (rhythm) minigame** rules engine.
//!
//! A faithful port of the dance overlay's per-frame rhythm logic - the beat
//! clock, the timing-window hit judge, the triangle "groovy move" wildcard, the
//! score / groove-gauge award, and the **three-dancer floor** (the human plus
//! the two competitors, who score through the very same award routine off a
//! chart auto-feed). Driven by the already-parsed step chart + scoring tables
//! ([`legaia_asset::dance_chart`]). This is the *rules* layer: it consumes pad
//! presses and produces judged results + running scores, exactly as the retail
//! overlay does. The visible dance-floor / arrow rendering is a separate host
//! concern and is not covered here.
//!
//! ## The retail shape, in one paragraph
//!
//! Three dancers stand on the floor; slot 0 is the human. Every frame the
//! per-dancer actor handler (`FUN_801d1358`) calls the award routine
//! (`FUN_801d1af4`) with a **pad word**: for the human that is the real pad, for
//! the competitors it is *synthesised from the chart* (`FUN_801d4040` ->
//! `FUN_801d1820`). So the rivals are not on a scripted score curve - they play
//! the same chart through the same judge, and differ only by their **kind** row
//! in two overlay tables (their sequence-bonus values and the schedule on which
//! they spend their triangles). Directional presses (Square `0x80` / Circle
//! `0x20`) are matched against the chart cell by `FUN_801d1960`; they score
//! **only when they close the lane's direction chain** (a "sequence"), for the
//! kind+lane value in `DAT_801d41a4`. The Triangle button (`0x10`) is the
//! **wildcard**: three per song, usable on any beat, worth `(lane+1) * 3` off
//! the beat but `(lane+1) * 0x19` when spent on the 4-beat combo slot - and it
//! throws the dancer into a multi-turn spin during which nothing is judged.
//!
//! Every constant and formula below is read from the overlay dumps
//! (`overlay_dance_801cf470/801d1358/801d1820/801d1960/801d1af4.txt`); see
//! [`docs/subsystems/minigame-dance.md`](../../../docs/subsystems/minigame-dance.md).
//! The two **data tables** (sequence bonus + triangle schedule) are disc
//! resident and parsed from the user's own image - no Sony bytes are baked in.
//!
//! Chain: retail `FUN_801cf470` (beat clock, state 10) -> `FUN_801d1358`
//! (per-dancer handler: latch decay, chart auto-feed) -> `FUN_801d1820` (AI
//! chart lookup) -> `FUN_801d1960` (hit judge) -> `FUN_801d1af4` (score/award).

use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart, DanceScoreTables};

/// Beat period in phase units (`FUN_801d1960`'s `0x119` divisor): one beat slot
/// spans this many phase units. `phase % PERIOD` = intra-beat phase,
/// `phase / PERIOD` = beat index.
pub const BEAT_PERIOD: u32 = 0x119;

/// Acceptance-window width inside a beat slot (`0xd2`). An intra-beat phase past
/// this is the dead zone between beats - no note is active and a press misses.
pub const BEAT_WINDOW: u32 = 0xd2;

/// The beat phase counter wraps at this value (`FUN_801cf470` beat clock). It is
/// exactly [`BEATS_PER_ROW`] × [`BEAT_PERIOD`], so the beat index runs `0..=31`
/// and indexes a chart row directly.
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

/// Groove-gauge step per **landed triangle** (`FUN_801d1af4`: `+= 1000` on the
/// combo slot). `gauge / GAUGE_STEP` selects the chart row (difficulty lane), so
/// crossing a step promotes the dancer to a denser, higher-scoring row.
pub const GAUGE_STEP: u32 = 1000;
/// Groove-gauge clamp ceiling (`[0, 2999]`).
pub const GAUGE_MAX: u32 = 2999;
/// Groove-gauge step per completed direction sequence (`DAT_801d6088 = 0xfa`).
pub const SEQUENCE_GAUGE_STEP: u32 = 0xfa;

/// Score multiplier for a triangle spent **off** the combo slot
/// (`(lane + 1) * 3`).
pub const MULT_ORDINARY: u32 = 3;
/// Score multiplier for a triangle spent **on** the 4-beat combo slot, inside
/// the window (`(lane + 1) * 0x19`) - the wildcard's payoff.
pub const MULT_COMBO: u32 = 0x19;
/// The award routine's *other* combo multiplier (`(lane + 1) * 0x22`). Retail
/// selects it by `DAT_801d5334 - 0xb < 2`, i.e. **only in the post-song Finish /
/// result-wipe states** (11 / 12), where the pad is still read. The rules engine
/// ends the run at the song timer, so this tier is documented but unreachable
/// here.
pub const MULT_FINALE: u32 = 0x22;

/// Triangles ("groovy moves") each dancer gets per song (`FUN_801cf470` state 3
/// / `FUN_801d0750`: `DAT_801d534c[0..3] = 3`). Not replenished mid-run.
pub const TRIANGLE_STOCK: u32 = 3;

/// Feedback window armed when a triangle is spent (`DAT_801d5144 = 0x3c`,
/// counted down by the frame delta). Retail's tutorial reads it to caption the
/// spend ("pretty good!" when it landed on the combo slot, "your timing is off"
/// when it didn't).
pub const TRIANGLE_FEEDBACK_WINDOW: u32 = 0x3c;

/// Spin accumulator units per full turn of the groovy move (`FUN_801d1358`
/// wraps the dancer's yaw at `0x1000`).
pub const SPIN_TURN_UNITS: u32 = 0x1000;
/// Groovy-move spin rate at lane 0, in yaw units per frame-delta
/// (`FUN_801d1358`: `(lane * 0x20 + 0x80) * DAT_1f800393`).
pub const SPIN_RATE_BASE: u32 = 0x80;
/// Groovy-move spin-rate increment per difficulty lane.
pub const SPIN_RATE_PER_LANE: u32 = 0x20;

/// Hit-tier latch timer set on every judged press (`DAT_801d54cc = 0xf`),
/// decayed by `2 * frame_delta` each frame. While the latch is up the dancer's
/// presses are not re-judged.
pub const NOTE_LATCH_TIMER: i32 = 0xf;
/// Latch decay per frame delta (`FUN_801d1358`: `timer -= 2 * DAT_1f800393`).
pub const NOTE_LATCH_DECAY: i32 = 2;

/// The direction chain cursor (`DAT_801d550c`) is cleared every this many beats
/// (`FUN_801d1358`: `beat & 7 == 0`), so a sequence must be closed within one
/// 8-beat bar.
pub const CURSOR_RESET_BEATS: u32 = 8;

/// Dancers on the qualifier floor (`DAT_801d53cc[0..3]` - the human + two
/// competitors).
pub const DANCER_SLOTS: usize = 3;

/// Solo-style win threshold the results state compares the score against
/// (`0x12d`, retail mode 2). Modes 0/1 instead compare the human's score against
/// a rival's - see [`DanceGame::beating_rivals`].
pub const WIN_THRESHOLD_SOLO: u32 = 300;

/// The qualifier (yosenn) floor's dancer kinds: Noa in the centre flanked by the
/// dance hall's two competitor NPCs (`FUN_801d0190`'s mode-0 spawn table). Used
/// when no cast table is supplied; [`DanceGame::from_overlay`] reads the real one
/// off the disc.
pub const QUALIFIER_KINDS: [usize; DANCER_SLOTS] = [0, 2, 3];

/// One of the three dance buttons. The retail judge compares the chart symbol
/// against `(pressed & 0xf) + 1`, so direction index `d` matches chart symbol
/// `d + 1`; [`DanceChart`] stores symbols `1`/`2`/`3` and `FUN_801d4040` maps
/// them to the pad bits `0x80`/`0x20`/`0x10` = Square / Circle / **Triangle**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceDir {
    /// Chart symbol `1`, pad bit `0x80` (Square) - a judged direction.
    A = 0,
    /// Chart symbol `2`, pad bit `0x20` (Circle) - a judged direction.
    B = 1,
    /// Chart symbol `3`, pad bit `0x10` (Triangle) - **not** a direction: the
    /// three-per-song "groovy move" wildcard (see [`DanceGame::press`]).
    C = 2,
}

impl DanceDir {
    /// The chart symbol this button matches (`index + 1`).
    pub fn symbol(self) -> u8 {
        self as u8 + 1
    }

    /// The pad-mask bit for this button (`FUN_801d4040`).
    pub fn pad_bit(self) -> u16 {
        match self {
            DanceDir::A => 0x80,
            DanceDir::B => 0x20,
            DanceDir::C => 0x10,
        }
    }

    /// `true` for the Triangle wildcard (chart symbol `3`).
    pub fn is_triangle(self) -> bool {
        matches!(self, DanceDir::C)
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

/// The result of judging a press (`FUN_801d1960`'s three-way return, as folded
/// by `FUN_801d1af4`). Kept for the existing host wiring; [`DanceEvent`] is the
/// full-fidelity result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judge {
    /// Outside the window, wrong direction, out of triangles, or ignored because
    /// the dancer is mid-groovy-move.
    Miss,
    /// Correct direction inside the window - a matched note that has not yet
    /// closed the chain (retail scores nothing for it, it advances the cursor).
    /// `weight` is the `0..=0x1000` accuracy weight (peaks on the beat).
    Hit { weight: u32 },
    /// A scoring event: a closed direction chain, or a landed triangle.
    Sequence { weight: u32 },
}

/// The full result of a press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceEvent {
    /// The press did nothing: the dancer is inside the groovy-move window or the
    /// per-note latch (retail's actor handler simply does not call the award
    /// routine while a move clip plays). No score, no miss.
    Ignored,
    /// Outside the acceptance window, or the pressed direction is not this
    /// beat's chart cell.
    Miss,
    /// A matched direction that advanced the chain cursor without closing it -
    /// retail awards nothing for it (`FUN_801d1960` return 1).
    Hit { weight: u32 },
    /// A matched direction that **closed** the lane's chain (`FUN_801d1960`
    /// return 2): `points` from the kind's bonus row, weighted by accuracy for
    /// the human (`base/2 + (base * weight) >> 13`), flat for a CPU dancer.
    Sequence { weight: u32, points: u32 },
    /// A triangle wildcard was spent. `landed` = it hit the 4-beat combo slot
    /// inside the window (the big multiplier); `lock` = frames of groovy-move
    /// spin during which input is ignored; `left` = triangles still in stock.
    Groovy {
        landed: bool,
        points: u32,
        lock: u32,
        left: u32,
    },
    /// Triangle pressed with an empty stock (three per song, no refill).
    NoCharge,
}

impl DanceEvent {
    /// Fold to the legacy three-way [`Judge`] the host wiring matches on.
    pub fn judge(self) -> Judge {
        match self {
            DanceEvent::Hit { weight } => Judge::Hit { weight },
            DanceEvent::Sequence { weight, .. } => Judge::Sequence { weight },
            DanceEvent::Groovy { landed: true, .. } => Judge::Sequence {
                weight: ACCURACY_MAX,
            },
            DanceEvent::Groovy { landed: false, .. } => Judge::Hit { weight: 0 },
            DanceEvent::Miss | DanceEvent::Ignored | DanceEvent::NoCharge => Judge::Miss,
        }
    }
}

/// One dancer's live state (the per-player arrays of the retail overlay).
#[derive(Debug, Clone, Default)]
struct Dancer {
    /// Dancer kind (`DAT_801d540c`): the row both scoring tables are indexed by.
    /// `0` = Noa (the human).
    kind: usize,
    /// Score (`DAT_801d53cc`), clamped to [`SCORE_MAX`].
    score: u32,
    /// Groove gauge (`DAT_801d544c`), clamped to `[0, GAUGE_MAX]`. Retail never
    /// lowers it on a miss - the Disco King's own tutorial says the level "rises
    /// automatically".
    gauge: u32,
    /// Direction-chain cursor (`DAT_801d550c`); closing `lane + 1` matched notes
    /// is a sequence. Cleared every [`CURSOR_RESET_BEATS`] beats.
    cursor: u32,
    /// Triangles left (`DAT_801d534c`).
    triangles: u32,
    /// Triangle-schedule cursor (`DAT_801d574c`) - CPU dancers only.
    tri_cursor: usize,
    /// Combo slots banked since the last triangle (`DAT_801d578c`) - CPU only.
    tri_meter: i32,
    /// Hit-tier latch (`DAT_801d548c`): non-zero = this dancer's presses are not
    /// judged.
    latch: u32,
    /// Latch countdown (`DAT_801d54cc`).
    latch_timer: i32,
    /// Groovy-move spin turns left (`DAT_801d564c`).
    spin_turns: u32,
    /// Spin accumulator (the dancer's yaw, `actor+0x26`).
    spin_acc: u32,
    /// Miss counter (`DAT_801d568c`; drives the sad-face pose).
    misses: u32,
    /// The last triangle landed on the combo slot (`DAT_801d570c`).
    landed: bool,
    /// Beat index of the last judged press. Retail's actor handler stops calling
    /// the award routine while the reaction / move clip plays, which is always
    /// long enough to cover the rest of the beat's window; this is that gate in
    /// rules terms (one registered press per beat per dancer).
    last_beat: Option<u32>,
    /// Last beat whose combo slot was banked into `tri_meter` (retail's
    /// `DAT_801d57cc` edge flag).
    last_meter_beat: Option<u32>,
    /// Last beat on which the chain cursor was cleared.
    last_reset_beat: Option<u32>,
}

impl Dancer {
    fn new(kind: usize) -> Self {
        Self {
            kind,
            triangles: TRIANGLE_STOCK,
            ..Default::default()
        }
    }

    /// The dancer's difficulty lane (`gauge / 1000`), clamped to the chart.
    fn lane(&self, rows: usize) -> u32 {
        (self.gauge / GAUGE_STEP).min(rows.saturating_sub(1) as u32)
    }

    /// Nothing is judged for this dancer right now: mid-spin, latched, or a
    /// press already registered on this beat.
    fn locked(&self, beat: u32) -> bool {
        self.spin_turns > 0 || self.latch != 0 || self.last_beat == Some(beat)
    }
}

/// The dance floor: the beat clock, the chart, and the three dancers' runs.
#[derive(Debug, Clone)]
pub struct DanceGame {
    chart: DanceChart,
    tables: DanceScoreTables,
    /// Beat phase counter (`DAT_801d581c`); wraps at [`BEAT_PHASE_WRAP`].
    phase: u32,
    /// Total-song timer (`DAT_801d5820`).
    song_timer: u32,
    /// Song-length limit this run ends at.
    song_len: u32,
    /// The floor, slot 0 = the human.
    dancers: Vec<Dancer>,
    /// Triangle feedback window (`DAT_801d5144`), armed on the human's spend.
    feedback: u32,
}

impl DanceGame {
    /// Start a run on `chart` with no disc scoring tables (sequences award no
    /// points and the CPU dancers never spend a triangle). Prefer
    /// [`DanceGame::from_overlay`], which reads the real tables + cast.
    pub fn new(chart: DanceChart, long_song: bool) -> Self {
        Self::with_tables(
            chart,
            DanceScoreTables::default(),
            &QUALIFIER_KINDS,
            long_song,
        )
    }

    /// Start a run on `chart` + the overlay's scoring `tables`, with the floor
    /// cast given as dancer kinds (slot 0 = the human).
    pub fn with_tables(
        chart: DanceChart,
        tables: DanceScoreTables,
        kinds: &[usize],
        long_song: bool,
    ) -> Self {
        Self {
            chart,
            tables,
            phase: 0,
            song_timer: 0,
            song_len: if long_song {
                SONG_LEN_LONG
            } else {
                SONG_LEN_SHORT
            },
            dancers: kinds.iter().map(|&k| Dancer::new(k)).collect(),
            feedback: 0,
        }
    }

    /// Parse the baked step chart + scoring tables + qualifier cast out of the
    /// dance overlay image (PROT 0980) and start a run. `None` when the chart
    /// doesn't decode (see [`legaia_asset::dance_chart::parse`]).
    pub fn from_overlay(overlay: &[u8], long_song: bool) -> Option<Self> {
        let chart = legaia_asset::dance_chart::parse(overlay)?;
        let tables = legaia_asset::dance_chart::parse_tables(overlay).unwrap_or_default();
        // The qualifier floor's cast (`FUN_801d0190` mode-0 spawn table): Noa
        // plus the two competitor NPCs, in floor order.
        let kinds: Vec<usize> = legaia_asset::dance_cast::parse(overlay)
            .map(|c| c.qualifier.iter().map(|s| s.kind as usize).collect())
            .filter(|k: &Vec<usize>| !k.is_empty())
            .unwrap_or_else(|| QUALIFIER_KINDS.to_vec());
        Some(Self::with_tables(chart, tables, &kinds, long_song))
    }

    // ---------------------------------------------------------------- clock

    /// Intra-beat phase (`phase % BEAT_PERIOD`).
    pub fn intra_beat_phase(&self) -> u32 {
        self.phase % BEAT_PERIOD
    }

    /// Beat index (`phase / BEAT_PERIOD`), `0..=31`.
    pub fn beat_index(&self) -> u32 {
        self.phase / BEAT_PERIOD
    }

    /// `true` when the intra-beat phase is in the dead zone (past the window) -
    /// no note is active, presses miss.
    pub fn in_dead_zone(&self) -> bool {
        self.intra_beat_phase() > BEAT_WINDOW
    }

    /// `true` on a 4-beat combo slot - the beat a triangle should be spent on
    /// (`FUN_801d1af4`: `(beat & 3) == 3 && phase < 0xd2`).
    pub fn on_combo_slot(&self) -> bool {
        self.beat_index() & 3 == 3 && !self.in_dead_zone()
    }

    /// The accuracy weight for the current phase (`FUN_801d1960`:
    /// `0x1000 - phase * 0x1000 / 0xd2`), `0` in the dead zone.
    pub fn accuracy_weight(&self) -> u32 {
        let p = self.intra_beat_phase();
        if p > BEAT_WINDOW {
            return 0;
        }
        ACCURACY_MAX - (p * ACCURACY_MAX) / BEAT_WINDOW
    }

    /// Song-timer position (`DAT_801d5820`), saturating at the song length.
    pub fn song_timer(&self) -> u32 {
        self.song_timer
    }

    /// This run's song-length limit ([`SONG_LEN_SHORT`] / [`SONG_LEN_LONG`]).
    pub fn song_len(&self) -> u32 {
        self.song_len
    }

    /// `true` once the song timer has reached this run's length limit.
    pub fn song_over(&self) -> bool {
        self.song_timer >= self.song_len
    }

    // ---------------------------------------------------------------- state

    /// The human's running score.
    pub fn score(&self) -> u32 {
        self.dancers[0].score
    }

    /// The human's groove gauge.
    pub fn gauge(&self) -> u32 {
        self.dancers[0].gauge
    }

    /// The human's difficulty lane (`gauge / GAUGE_STEP`).
    pub fn lane(&self) -> usize {
        self.dancers[0].lane(self.chart.rows.len()) as usize
    }

    /// Triangles the human has left this song.
    pub fn triangles(&self) -> u32 {
        self.dancers[0].triangles
    }

    /// Frames of groovy-move spin still to run on the human - input is ignored
    /// while this is non-zero.
    pub fn groovy_lock(&self) -> u32 {
        self.spin_frames_left(0)
    }

    /// `true` while the human is inside the groovy-move window.
    pub fn in_groovy_move(&self) -> bool {
        self.dancers[0].spin_turns > 0
    }

    /// The triangle feedback window (`DAT_801d5144`) still running, and whether
    /// the spend that armed it landed on the combo slot.
    pub fn triangle_feedback(&self) -> Option<bool> {
        (self.feedback > 0).then(|| self.dancers[0].landed)
    }

    /// Dancers on the floor (slot 0 = the human).
    pub fn dancer_count(&self) -> usize {
        self.dancers.len()
    }

    /// Dancer `i`'s score (`DAT_801d53cc[i]`).
    pub fn dancer_score(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.score).unwrap_or(0)
    }

    /// Dancer `i`'s groove gauge.
    pub fn dancer_gauge(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.gauge).unwrap_or(0)
    }

    /// Dancer `i`'s difficulty lane.
    pub fn dancer_lane(&self, i: usize) -> usize {
        self.dancers
            .get(i)
            .map(|d| d.lane(self.chart.rows.len()) as usize)
            .unwrap_or(0)
    }

    /// Dancer `i`'s remaining triangles.
    pub fn dancer_triangles(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.triangles).unwrap_or(0)
    }

    /// Dancer `i`'s kind (the row both scoring tables are indexed by).
    pub fn dancer_kind(&self, i: usize) -> usize {
        self.dancers.get(i).map(|d| d.kind).unwrap_or(0)
    }

    /// Final solo-style grade (retail mode 2): `true` when the score meets
    /// [`WIN_THRESHOLD_SOLO`].
    pub fn passed(&self) -> bool {
        self.score() >= WIN_THRESHOLD_SOLO
    }

    /// The versus grade (retail modes 0/1): the human out-scores every rival on
    /// the floor. Ties go to the human - retail clears the win flag only when
    /// `human < rival`.
    pub fn beating_rivals(&self) -> bool {
        let me = self.score();
        self.dancers.iter().skip(1).all(|d| me >= d.score)
    }

    // ---------------------------------------------------------------- chart

    /// The chart symbol the **hit judge** (`FUN_801d1960`) matches a press
    /// against for the human's lane + beat: `None` in the dead zone, `Some(0)`
    /// when the beat carries no note, else the direction symbol.
    // PORT: FUN_801d1960 (the judged chart cell)
    pub fn judged_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        Some(self.cell(self.lane(), self.beat_index()))
    }

    /// The symbol the **CPU auto-feed** would press for the human's lane
    /// (`FUN_801d1820` - the display half, which substitutes the triangle symbol
    /// `3` on the combo slot once the dancer's schedule is due). Kept for hosts
    /// that draw the retail "displayed" note; only [`Self::judged_symbol`]
    /// scores a direction.
    // PORT: FUN_801d1820 (chart lookup - the auto-feed / display half)
    pub fn required_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let beat = self.beat_index();
        if beat & 3 == 3 {
            return Some(3);
        }
        Some(self.cell(self.lane(), beat))
    }

    /// The chart row `lane`, for a host drawing the note highway.
    pub fn chart_row(&self, lane: usize) -> Option<&[u8; BEATS_PER_ROW]> {
        self.chart.rows.get(lane)
    }

    fn cell(&self, lane: usize, beat: u32) -> u8 {
        self.chart
            .symbol(lane, (beat as usize) % BEATS_PER_ROW)
            .unwrap_or(0)
    }

    // ---------------------------------------------------------------- frame

    /// Advance one frame (`FUN_801cf470` state 10 + `FUN_801d1358` per dancer):
    /// step the beat clock, decay each dancer's latches / groovy spin, bank the
    /// combo slot, and run the **CPU dancers' auto-fed presses** through the same
    /// judge + award the human's presses go through.
    // PORT: FUN_801cf470 (beat clock + song-end test, states 10..12)
    // PORT: FUN_801d1358 (per-dancer handler: latch decay, spin, chart auto-feed)
    pub fn advance(&mut self, frame_delta: u32) {
        let step = frame_delta * PHASE_PER_DELTA;
        self.phase = (self.phase + step) % BEAT_PHASE_WRAP;
        // The song timer saturates at the length limit (the retail clock keeps
        // counting but the run ends; clamping keeps `song_over` monotone).
        self.song_timer = self.song_timer.saturating_add(step).min(self.song_len);
        self.feedback = self.feedback.saturating_sub(frame_delta);

        let beat = self.beat_index();
        let rows = self.chart.rows.len();
        for d in &mut self.dancers {
            // Latch decay (`timer -= 2 * delta`; at 0 the latch clears).
            if d.latch_timer > 0 {
                d.latch_timer -= NOTE_LATCH_DECAY * frame_delta as i32;
                if d.latch_timer < 1 {
                    d.latch_timer = 0;
                    d.latch = 0;
                }
            }
            // Groovy-move spin: the dancer turns once per SPIN_TURN_UNITS of
            // accumulated yaw, `lane + 1` turns in all.
            if d.spin_turns > 0 {
                let rate = SPIN_RATE_BASE + d.lane(rows) * SPIN_RATE_PER_LANE;
                d.spin_acc += rate * frame_delta;
                while d.spin_acc >= SPIN_TURN_UNITS && d.spin_turns > 0 {
                    d.spin_acc -= SPIN_TURN_UNITS;
                    d.spin_turns -= 1;
                }
                if d.spin_turns == 0 {
                    d.spin_acc = 0;
                }
            }
            // Chain cursor clears once per 8-beat bar.
            if beat.is_multiple_of(CURSOR_RESET_BEATS) && d.last_reset_beat != Some(beat) {
                d.last_reset_beat = Some(beat);
                d.cursor = 0;
            }
            // Bank one combo slot per 4-beat boundary (the CPU triangle clock).
            if beat & 3 == 3 && d.last_meter_beat != Some(beat) {
                d.last_meter_beat = Some(beat);
                d.tri_meter += 1;
            }
        }

        // The competitors' pad word is synthesised from the chart every frame.
        for i in 1..self.dancers.len() {
            if let Some(sym) = self.auto_feed(i) {
                match sym {
                    1 => {
                        self.award(i, DanceDir::A);
                    }
                    2 => {
                        self.award(i, DanceDir::B);
                    }
                    3 => {
                        self.award(i, DanceDir::C);
                    }
                    _ => {}
                }
            }
        }
    }

    /// The CPU dancer's synthetic pad symbol for this frame (`FUN_801d1820`):
    /// nothing in the dead zone; on a combo slot the triangle once the kind's
    /// schedule (`DAT_801d41e4`) has banked enough slots; otherwise its own
    /// lane's chart cell.
    // PORT: FUN_801d1820 (the CPU auto-feed)
    fn auto_feed(&mut self, i: usize) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let beat = self.beat_index();
        let rows = self.chart.rows.len();
        let (lane, due) = {
            let d = &self.dancers[i];
            let due = beat & 3 == 3
                && d.triangles > 0
                && self.tables.schedule(d.kind, d.tri_cursor) <= d.tri_meter;
            (d.lane(rows) as usize, due)
        };
        if due {
            let d = &mut self.dancers[i];
            d.tri_cursor += 1;
            d.tri_meter = 0;
            return Some(3);
        }
        Some(self.cell(lane, beat))
    }

    // ---------------------------------------------------------------- press

    /// Judge a human press. Square / Circle are judged against the chart cell;
    /// **Triangle spends a groovy-move wildcard** (three per song, any beat,
    /// worth the big multiplier only on the 4-beat combo slot, and locking input
    /// out for the length of the spin it throws the dancer into).
    // PORT: FUN_801d1af4 (score / groove-gauge award; pad-word branches)
    pub fn press(&mut self, dir: DanceDir) -> DanceEvent {
        self.award(0, dir)
    }

    /// Legacy three-way wrapper over [`Self::press`] for hosts matching on
    /// [`Judge`]. An ignored press (mid-groovy-move) folds to [`Judge::Miss`],
    /// but applies no penalty.
    pub fn judge_press(&mut self, dir: DanceDir) -> Judge {
        self.press(dir).judge()
    }

    /// The award routine (`FUN_801d1af4`), for any dancer: the human's presses
    /// and the CPU dancers' auto-fed ones run through exactly this path.
    fn award(&mut self, i: usize, dir: DanceDir) -> DanceEvent {
        let beat = self.beat_index();
        if self.dancers[i].locked(beat) {
            return DanceEvent::Ignored;
        }
        if dir.is_triangle() {
            self.spend_triangle(i, beat)
        } else {
            self.judge_direction(i, dir, beat)
        }
    }

    /// The `0x80` / `0x20` branches: judge the press against the chart cell
    /// (`FUN_801d1960`), advance the chain cursor, and award the kind's bonus
    /// when the chain closes. A plain matched note scores **nothing** in retail -
    /// only the closing note does.
    // PORT: FUN_801d1960 (hit judge: dead-zone + accuracy weight + direction match)
    fn judge_direction(&mut self, i: usize, dir: DanceDir, beat: u32) -> DanceEvent {
        let rows = self.chart.rows.len();
        let weight = self.accuracy_weight();
        let dead = self.in_dead_zone();
        let lane = self.dancers[i].lane(rows);
        let want = self.cell(lane as usize, beat);
        let bonus = self
            .tables
            .bonus(self.dancers[i].kind, lane as usize)
            .max(0) as u32;

        let d = &mut self.dancers[i];
        // Every judged press latches the dancer (retail binds a reaction / move
        // clip and stops re-judging until it ends).
        d.latch = dir.symbol() as u32;
        d.latch_timer = NOTE_LATCH_TIMER;
        d.last_beat = Some(beat);

        if dead || want == 0 || want != dir.symbol() {
            d.misses += 1;
            return DanceEvent::Miss;
        }
        d.cursor += 1;
        if d.cursor <= lane {
            return DanceEvent::Hit { weight };
        }
        // Chain closed (`cursor + 1 == lane + 1`).
        d.cursor = 0;
        // The human's award is accuracy-weighted (`base/2 + (base * w) >> 13`);
        // a CPU dancer takes the flat table value.
        let points = if i == 0 {
            bonus / 2 + ((bonus * weight) >> 13)
        } else {
            bonus
        };
        d.gauge = (d.gauge + SEQUENCE_GAUGE_STEP).min(GAUGE_MAX);
        d.score = (d.score + points).min(SCORE_MAX);
        d.misses = d.misses.saturating_sub(1);
        DanceEvent::Sequence { weight, points }
    }

    /// The `0x10` branch: **spend a triangle**. Retail gates it on the stock
    /// counter only (no chart match - it is a wildcard on any beat), scores
    /// `(lane+1) * 0x19` when it lands on the 4-beat combo slot inside the window
    /// (plus a full `+1000` gauge step, which promotes the lane) and only
    /// `(lane+1) * 3` when it does not, and throws the dancer into a `lane + 1`
    /// turn spin during which no press is judged.
    // PORT: FUN_801d1af4 (the pad-0x10 groovy-move branch)
    fn spend_triangle(&mut self, i: usize, beat: u32) -> DanceEvent {
        let rows = self.chart.rows.len();
        let landed = self.on_combo_slot();
        let d = &mut self.dancers[i];
        if d.triangles == 0 {
            return DanceEvent::NoCharge;
        }
        d.triangles -= 1;
        d.latch = 3;
        d.latch_timer = NOTE_LATCH_TIMER;
        d.last_beat = Some(beat);
        let lane = d.lane(rows);
        d.landed = landed;
        let points = if landed {
            d.gauge = (d.gauge + GAUGE_STEP).min(GAUGE_MAX);
            (lane + 1) * MULT_COMBO
        } else {
            (lane + 1) * MULT_ORDINARY
        };
        d.score = (d.score + points).min(SCORE_MAX);
        // The groovy move: `lane + 1` full turns of the dancer's yaw, spun at
        // `0x80 + lane * 0x20` units per frame - up to 64 frames of locked-out
        // input, the whole time retail is playing the move clip.
        d.spin_turns = lane + 1;
        d.spin_acc = 0;
        let left = d.triangles;
        if i == 0 {
            self.feedback = TRIANGLE_FEEDBACK_WINDOW;
        }
        DanceEvent::Groovy {
            landed,
            points,
            lock: self.spin_frames_left(i),
            left,
        }
    }

    /// Frames of groovy-move spin still to run on dancer `i` - the window its
    /// input is disrupted for. The spin rate is read from the dancer's *current*
    /// lane each frame (`FUN_801d1358`), so a landed triangle's own gauge step
    /// speeds up the move it started.
    fn spin_frames_left(&self, i: usize) -> u32 {
        let Some(d) = self.dancers.get(i) else {
            return 0;
        };
        if d.spin_turns == 0 {
            return 0;
        }
        let rate = SPIN_RATE_BASE + d.lane(self.chart.rows.len()) * SPIN_RATE_PER_LANE;
        (d.spin_turns * SPIN_TURN_UNITS - d.spin_acc).div_ceil(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::dance_chart::{DANCE_BONUS_LANES, DANCE_SCHEDULE_SLOTS, DANCE_SKILL_ROWS};

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

    /// Retail-shaped scoring tables: `k, 2k, 3k` bonus rows and a triangle
    /// schedule that fires the CPU dancers' first groovy move early.
    fn tables() -> DanceScoreTables {
        let mut bonus = Vec::new();
        let mut schedule = Vec::new();
        for k in 0..DANCE_SKILL_ROWS {
            let base = (17 - 3 * k) as i32;
            let mut row = [0i32; DANCE_BONUS_LANES];
            for (lane, cell) in row.iter_mut().enumerate().take(3) {
                *cell = base * (lane as i32 + 1);
            }
            bonus.push(row);
            let mut s = [1000i32; DANCE_SCHEDULE_SLOTS];
            if k > 0 {
                s[0] = 1; // spend the first triangle after one banked combo slot
                s[1] = 2;
            }
            schedule.push(s);
        }
        DanceScoreTables { bonus, schedule }
    }

    fn game() -> DanceGame {
        DanceGame::with_tables(chart(), tables(), &QUALIFIER_KINDS, false)
    }

    #[test]
    fn constants_match_the_re() {
        assert_eq!(BEAT_PERIOD, 0x119);
        assert_eq!(BEAT_WINDOW, 0xd2);
        assert_eq!(BEAT_PHASE_WRAP, 0x2320);
        // The phase wrap is exactly one chart row of beats.
        assert_eq!(BEAT_PHASE_WRAP, BEAT_PERIOD * BEATS_PER_ROW as u32);
        assert_eq!((MULT_ORDINARY, MULT_COMBO, MULT_FINALE), (3, 25, 34));
        assert_eq!((SCORE_MAX, GAUGE_MAX, GAUGE_STEP), (999, 2999, 1000));
        assert_eq!((SEQUENCE_GAUGE_STEP, TRIANGLE_STOCK), (250, 3));
        assert_eq!(TRIANGLE_FEEDBACK_WINDOW, 0x3c);
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
        assert!(DanceDir::C.is_triangle());
        assert!(!DanceDir::A.is_triangle());
    }

    #[test]
    fn accuracy_weight_peaks_on_beat_and_decays_to_edge() {
        let mut g = game();
        assert_eq!(g.accuracy_weight(), ACCURACY_MAX);
        g.phase = BEAT_WINDOW;
        assert_eq!(g.accuracy_weight(), 0);
        assert!(!g.in_dead_zone());
        g.phase = BEAT_WINDOW + 1;
        assert!(g.in_dead_zone());
        assert_eq!(g.accuracy_weight(), 0);
    }

    #[test]
    fn beat_clock_wraps_and_ends_song() {
        let mut g = game();
        g.advance(1);
        assert_eq!(g.phase, PHASE_PER_DELTA);
        assert_eq!(g.beat_index(), 0);
        for _ in 0..2000 {
            g.advance(1);
        }
        assert!(g.song_over());
        assert!(g.phase < BEAT_PHASE_WRAP);
    }

    #[test]
    fn dead_zone_press_misses_but_never_lowers_the_gauge() {
        let mut g = game();
        g.dancers[0].gauge = 1500;
        g.phase = BEAT_WINDOW + 5; // dead zone
        assert_eq!(g.press(DanceDir::A), DanceEvent::Miss);
        // Retail's award routine has no gauge-drop path: a miss only bumps the
        // miss counter (and the sad-face pose).
        assert_eq!(g.gauge(), 1500);
        assert_eq!(g.dancers[0].misses, 1);
    }

    #[test]
    fn a_closed_chain_scores_the_kinds_bonus_a_bare_hit_does_not() {
        // Lane 0: a single matched note closes the chain (cursor + 1 == 1).
        let mut g = game();
        assert_eq!(g.judged_symbol(), Some(1));
        // Kind 0's lane-0 bonus is 17; the human's award is accuracy-weighted
        // (`base/2 + (base * w) >> 13`), so a dead-on press banks 8 + 8 = 16.
        assert!(matches!(
            g.press(DanceDir::A),
            DanceEvent::Sequence { points, .. } if points == 16
        ));
        assert_eq!(g.score(), 16);
        assert_eq!(g.gauge(), SEQUENCE_GAUGE_STEP);

        // Lane 1 needs two matched notes: the first is a bare Hit worth nothing.
        let mut g = game();
        g.dancers[0].gauge = 1000; // lane 1
        assert!(matches!(g.press(DanceDir::A), DanceEvent::Hit { .. }));
        assert_eq!(g.score(), 0);
        // Advance to beat 1 (lane 1 wants symbol 1 again) and close the chain.
        g.phase = BEAT_PERIOD;
        g.dancers[0].latch = 0;
        g.dancers[0].latch_timer = 0;
        assert!(matches!(
            g.press(DanceDir::A),
            DanceEvent::Sequence { points, .. } if points == 34 // 17 * lane(1)+1
        ));
        assert_eq!(g.score(), 34);
    }

    #[test]
    fn wrong_direction_misses_and_the_press_is_latched() {
        let mut g = game();
        assert_eq!(g.press(DanceDir::B), DanceEvent::Miss);
        assert_eq!(g.score(), 0);
        // A judged press latches the dancer: an immediate re-press is ignored
        // (retail is playing the miss-reaction clip).
        assert_eq!(g.press(DanceDir::A), DanceEvent::Ignored);
    }

    // ---------------------------------------------------------- triangles

    #[test]
    fn triangle_stock_is_three_and_runs_out() {
        let mut g = game();
        assert_eq!(g.triangles(), 3);
        for n in 0..3 {
            // Free the dancer from the previous spend's spin + latch.
            g.dancers[0].spin_turns = 0;
            g.dancers[0].latch = 0;
            g.dancers[0].last_beat = None;
            assert!(matches!(
                g.press(DanceDir::C),
                DanceEvent::Groovy { left, .. } if left == 2 - n
            ));
        }
        assert_eq!(g.triangles(), 0);
        g.dancers[0].spin_turns = 0;
        g.dancers[0].latch = 0;
        g.dancers[0].last_beat = None;
        assert_eq!(g.press(DanceDir::C), DanceEvent::NoCharge);
        assert_eq!(g.triangles(), 0);
    }

    #[test]
    fn triangle_on_the_combo_slot_multiplies_and_promotes_the_lane() {
        // Off the combo slot: the wildcard is worth only (lane + 1) * 3.
        let mut g = game();
        assert!(!g.on_combo_slot());
        assert!(matches!(
            g.press(DanceDir::C),
            DanceEvent::Groovy { landed: false, points, .. } if points == MULT_ORDINARY
        ));
        assert_eq!(g.gauge(), 0, "an off-beat spend does not fill the gauge");

        // On the 4-beat combo slot: (lane + 1) * 25, plus a full gauge step.
        let mut g = game();
        g.phase = 3 * BEAT_PERIOD;
        assert!(g.on_combo_slot());
        assert!(matches!(
            g.press(DanceDir::C),
            DanceEvent::Groovy { landed: true, points, .. } if points == MULT_COMBO
        ));
        assert_eq!(g.score(), MULT_COMBO);
        assert_eq!(g.gauge(), GAUGE_STEP);
        assert_eq!(g.lane(), 1, "the landed triangle promoted the lane");

        // Spent at the end of a long combo (lane 2) it is worth 3 x 25 = 75.
        let mut g = game();
        g.dancers[0].gauge = 2000; // lane 2 - the combo the player built
        g.phase = 3 * BEAT_PERIOD;
        assert!(matches!(
            g.press(DanceDir::C),
            DanceEvent::Groovy { landed: true, points, .. } if points == 3 * MULT_COMBO
        ));
    }

    #[test]
    fn a_spent_triangle_locks_input_out_for_the_groovy_move() {
        // Spent at the end of a long combo (lane 2): 3 turns at 0xC0 units per
        // frame = 64 frames - the retail groovy-move window.
        let mut g = game();
        g.dancers[0].gauge = 2000;
        g.phase = 3 * BEAT_PERIOD;
        let DanceEvent::Groovy { lock, .. } = g.press(DanceDir::C) else {
            panic!("triangle spent");
        };
        assert_eq!(
            lock,
            3 * SPIN_TURN_UNITS / (SPIN_RATE_BASE + 2 * SPIN_RATE_PER_LANE)
        );
        assert_eq!(lock, 64);
        assert!(g.in_groovy_move());
        assert_eq!(g.groovy_lock(), lock);
        // Every press inside the window is ignored - no score, no miss.
        let before = g.score();
        for f in 0..lock {
            assert_eq!(
                g.press(DanceDir::A),
                DanceEvent::Ignored,
                "input is disrupted for the whole groovy move (frame {f})"
            );
            assert_eq!(g.score(), before);
            g.advance(1);
        }
        // ...and it ends: the dancer is judged again.
        assert!(!g.in_groovy_move());
        assert_eq!(g.groovy_lock(), 0);
        assert_eq!(g.dancers[0].misses, 0, "ignored presses are not misses");
    }

    #[test]
    fn triangle_arms_the_feedback_window() {
        let mut g = game();
        assert_eq!(g.triangle_feedback(), None);
        g.phase = 3 * BEAT_PERIOD;
        let _ = g.press(DanceDir::C);
        assert_eq!(g.triangle_feedback(), Some(true), "it landed on the slot");
        for _ in 0..TRIANGLE_FEEDBACK_WINDOW {
            g.advance(1);
        }
        assert_eq!(g.triangle_feedback(), None);
    }

    // ------------------------------------------------------------- rivals

    #[test]
    fn rival_scores_advance_over_the_song() {
        let mut g = game();
        assert_eq!(g.dancer_count(), 3);
        assert_eq!(
            (g.dancer_kind(0), g.dancer_kind(1), g.dancer_kind(2)),
            (0, 2, 3)
        );
        assert_eq!((g.dancer_score(1), g.dancer_score(2)), (0, 0));
        let mut last = [0u32; 2];
        let mut climbs = 0;
        for _ in 0..1500 {
            g.advance(1);
            let now = [g.dancer_score(1), g.dancer_score(2)];
            if now[0] > last[0] && now[1] > last[1] {
                climbs += 1;
            }
            assert!(now[0] >= last[0] && now[1] >= last[1], "scores never fall");
            last = now;
        }
        assert!(g.dancer_score(1) > 0, "rival 1 scored off the auto-feed");
        assert!(g.dancer_score(2) > 0, "rival 2 scored off the auto-feed");
        assert!(
            climbs > 1,
            "the rival scores advance repeatedly over the song"
        );
        // The human never touched the pad, so the rivals are ahead.
        assert_eq!(g.score(), 0);
        assert!(!g.beating_rivals());
        // A rival's kind picks its bonus row: kind 2 out-scores kind 3.
        assert!(
            g.dancer_score(1) >= g.dancer_score(2),
            "the stronger kind's bonus row scores at least as fast"
        );
    }

    #[test]
    fn rivals_spend_their_triangles_on_the_disc_schedule() {
        let mut g = game();
        // The fixture schedule fires kind 2/3's first triangle after one banked
        // combo slot, so both rivals spend one within the first bars.
        for _ in 0..600 {
            g.advance(1);
        }
        assert!(g.dancer_triangles(1) < TRIANGLE_STOCK);
        assert!(g.dancer_triangles(2) < TRIANGLE_STOCK);
        // Never more than the stock, ever.
        for _ in 0..2000 {
            g.advance(1);
        }
        assert!(g.dancer_triangles(1) <= TRIANGLE_STOCK);
    }

    #[test]
    fn gauge_promotes_lane_and_score_clamps() {
        let mut g = game();
        g.dancers[0].gauge = 1500;
        assert_eq!(g.lane(), 1);
        g.dancers[0].gauge = GAUGE_MAX;
        assert_eq!(g.lane(), 2);
        g.dancers[0].score = SCORE_MAX - 1;
        g.dancers[0].gauge = 0;
        let _ = g.press(DanceDir::A);
        assert_eq!(g.score(), SCORE_MAX);
    }

    #[test]
    fn required_symbol_holds_the_triangle_on_the_fourth_beat() {
        let mut g = game();
        g.phase = 3 * BEAT_PERIOD;
        assert_eq!(g.required_symbol(), Some(3));
        g.phase = 3 * BEAT_PERIOD + BEAT_WINDOW + 1;
        assert_eq!(g.required_symbol(), None);
    }

    #[test]
    fn pass_threshold_and_versus_grade() {
        let mut g = game();
        assert!(!g.passed());
        g.dancers[0].score = WIN_THRESHOLD_SOLO;
        assert!(g.passed());
        g.dancers[1].score = WIN_THRESHOLD_SOLO;
        assert!(g.beating_rivals(), "a tie goes to the human");
        g.dancers[2].score = WIN_THRESHOLD_SOLO + 1;
        assert!(!g.beating_rivals());
    }

    #[test]
    fn legacy_judge_wrapper_folds_the_events() {
        let mut g = game();
        assert!(matches!(g.judge_press(DanceDir::A), Judge::Sequence { .. }));
        let mut g = game();
        assert_eq!(g.judge_press(DanceDir::B), Judge::Miss);
        let mut g = game();
        g.phase = 3 * BEAT_PERIOD;
        assert!(matches!(g.judge_press(DanceDir::C), Judge::Sequence { .. }));
        // Mid-groovy-move presses fold to Miss but apply no penalty.
        assert_eq!(g.judge_press(DanceDir::A), Judge::Miss);
        assert_eq!(g.dancers[0].misses, 0);
    }
}
