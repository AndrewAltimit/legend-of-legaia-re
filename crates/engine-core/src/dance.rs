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
/// counted down by the frame delta). The retail tutorial reads it to caption the
/// spend - praise when it landed on the combo slot, a timing scold when it did
/// not (`FUN_801d0750` case `0xd`, gated on `DAT_801d570c`).
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

/// Which floor a run is on - the mode global `DAT_801d514c`, `0..=3`.
///
/// The mode is normally chosen by the *caller*: a field script sets one of the
/// story flags `0x134` / `0x135` / `0x133` / `0x428` before entering, and the
/// overlay's state 1 maps it here and clears it. The on-screen cursor menu in
/// state 0 is the debug selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceMode {
    /// `0` yosenn - the qualifier. Graded against score slot 2.
    Qualifier,
    /// `1` hosenn - the finals. Graded against score slot 1.
    Finals,
    /// `2` setumei - the how-to demo: one dancer, short song, graded on
    /// [`WIN_THRESHOLD_SOLO`].
    HowTo,
    /// `3` asobi - free play: six dancers, and no win/lose flag at all.
    FreePlay,
}

impl DanceMode {
    /// The mode global's value.
    pub fn value(self) -> u32 {
        match self {
            DanceMode::Qualifier => 0,
            DanceMode::Finals => 1,
            DanceMode::HowTo => 2,
            DanceMode::FreePlay => 3,
        }
    }

    /// How many dancers this mode spawns (`FUN_801d0190`'s per-mode count -
    /// the `$s3` the spawn loop counts down).
    pub fn cast_size(self) -> usize {
        match self {
            DanceMode::Qualifier | DanceMode::Finals => 3,
            DanceMode::HowTo => 1,
            DanceMode::FreePlay => 6,
        }
    }
}

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

    /// The chart symbol -> pad-mask bit map: symbol `1` (`DanceDir::A`) ->
    /// `0x80`, `2` -> `0x20`, `3` -> `0x10`. Retail takes the raw chart byte and
    /// returns `0` for anything else; the chart decoder converts symbols to
    /// [`DanceDir`] before this point, so the whole retail domain is covered by
    /// the three variants and the `0` arm has no reachable input.
    ///
    /// Wired: `World::tick_dance` packs this frame's pad edges into the retail
    /// layout (`_DAT_8007B874`) and picks the pressed direction by matching
    /// this bit, the way `FUN_801d1af4` tests `0x10` / `0x80` / `0x20`.
    ///
    /// Retail's own call site is the *other* consumer of the same map: an NPC
    /// dancer (`FUN_801d1af4` with a non-zero dancer index) has no pad, so the
    /// judge substitutes `FUN_801d4040(dancer)` - that dancer's current chart
    /// symbol translated into the pad bit space - for the player's pad word.
    /// The port models only the player, so that substitution has no caller.
    // PORT: FUN_801d4040 (chart symbol -> pad-mask bit)
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
    /// The dancer actor's spawn position, straight out of the mode's spawn
    /// table (`FUN_801d0190` stores the record's three words into the actor's
    /// `+0x14` / `+0x16` / `+0x18`). Zero on a chart-only run.
    home: [i16; 3],
    /// The actor's bound clip id (`+0x5C`), masked to `0x1FF` exactly as
    /// `FUN_801d0190` and `FUN_801d1358` write it. `0` = nothing bound.
    clip: i16,
    /// The bound clip's cursor step (`+0x6A`), the kind descriptor's rate word.
    clip_rate: u16,
    /// The actor flag word (`+0x10`). Only the bits the dance overlay writes
    /// are modelled: [`crate::minigame_actor::FLAG_TRANSLUCENT`] (the anim
    /// word's `0x200`) and [`crate::minigame_actor::FLAG_DRIVE_CLIP`].
    flags: u32,
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

    /// Bind a clip into the actor's `+0x5C` / `+0x6A` pair, folding the anim
    /// word's `0x200` bit into the flag word the way `FUN_801d1358` does.
    fn bind_clip(&mut self, clip: &legaia_asset::dance_cast::DanceClip) {
        self.clip = (clip.anim_id & 0x1FF) as i16;
        self.clip_rate = clip.rate;
        if clip.translucent {
            self.flags |= crate::minigame_actor::FLAG_TRANSLUCENT;
        } else {
            self.flags &= !crate::minigame_actor::FLAG_TRANSLUCENT;
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
    /// The mode global (`DAT_801d514c`), which the HUD driver's score-box
    /// permutation and its solo arm both key off.
    mode: DanceMode,
    /// The overlay's HUD widget table with each record's `+0x13` ABR byte
    /// alongside it, when the run was started from a real overlay image.
    widgets: Vec<(legaia_asset::dance_art::DanceWidget, u8)>,
    /// The five kind descriptors, when the run was started from a real overlay
    /// image. This is what supplies the clip ids the dancer actors bind into
    /// their `+0x5C`; without it a dancer actor carries no clip and the clip
    /// driver gate reports `false` for it (which is what retail would do too -
    /// an actor with nothing bound is not handed to `FUN_800204F8`).
    kinds: Vec<legaia_asset::dance_cast::DanceKind>,
    /// The dancer actor pool - one [`crate::minigame_actor::MinigameActor`]
    /// per floor slot, rebuilt from the dancers every [`DanceGame::advance`].
    actors: crate::minigame_actor::MinigameActorPool,
    /// The **sprite-part** pool: what `FUN_801d3fd0` spawns and
    /// [`sprite_part_emit`] draws. A separate pool from the dancers because it
    /// is a separate actor family - the dancer is a 3D body the clip driver
    /// animates, the part is a 2D sprite in the `<< 3` screen space.
    parts: crate::minigame_actor::MinigameActorPool,
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
        let mut game = Self {
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
            mode: DanceMode::Qualifier,
            widgets: Vec::new(),
            kinds: Vec::new(),
            actors: crate::minigame_actor::MinigameActorPool::new(),
            parts: crate::minigame_actor::MinigameActorPool::new(),
        };
        // A chart-only run still spawns its floor - the actors just stand at
        // the origin and bind no clip, because both of those come off the
        // overlay's spawn + kind tables.
        let spawns: Vec<(usize, [i16; 3])> = kinds.iter().map(|&k| (k, [0i16; 3])).collect();
        game.spawn_dancer_actors(&spawns);
        game
    }

    /// Parse the baked step chart + scoring tables + qualifier cast out of the
    /// dance overlay image (PROT 0980) and start a run. `None` when the chart
    /// doesn't decode (see [`legaia_asset::dance_chart::parse`]).
    ///
    /// Starts the **qualifier** floor; [`DanceGame::from_overlay_for_mode`] is
    /// the per-mode entry point.
    pub fn from_overlay(overlay: &[u8], long_song: bool) -> Option<Self> {
        Self::from_overlay_for_mode(overlay, DanceMode::Qualifier, long_song)
    }

    /// Start a run on `mode`'s floor.
    ///
    /// The mode picks the cast **and its size**, which is the part that is easy
    /// to miss: the three spawn tables are not three arrangements of one roster.
    /// Free play puts **six** dancers on the floor and the how-to demo puts a
    /// single one, so a host that always spawns the qualifier's three is wrong
    /// in two of the four modes.
    ///
    /// `long_song` stays a parameter because the caller owns the song choice;
    /// the how-to demo is the one mode whose length retail fixes, and it forces
    /// [`SONG_LEN_SHORT`] regardless.
    // PORT: FUN_801d0190 (per-mode spawn-table + cast-size selection)
    pub fn from_overlay_for_mode(overlay: &[u8], mode: DanceMode, long_song: bool) -> Option<Self> {
        let chart = legaia_asset::dance_chart::parse(overlay)?;
        let tables = legaia_asset::dance_chart::parse_tables(overlay).unwrap_or_default();
        let cast = legaia_asset::dance_cast::parse(overlay);
        // The spawn record carries the dancer's kind **and** its floor
        // position; the position is what `FUN_801d0190` stores into the
        // spawned actor's `+0x14` / `+0x16` / `+0x18`, so it is kept here
        // rather than dropped with the rest of the record.
        let spawns: Vec<(usize, [i16; 3])> = cast
            .as_ref()
            .map(|c| {
                let table = match mode {
                    // The how-to demo reads the qualifier table but spawns only
                    // its first record - one dancer, not three.
                    DanceMode::Qualifier | DanceMode::HowTo => &c.qualifier,
                    DanceMode::Finals => &c.finals,
                    DanceMode::FreePlay => &c.free_play,
                };
                table
                    .iter()
                    .take(mode.cast_size())
                    .map(|s| (s.kind as usize, [s.x, s.y, s.z]))
                    .collect()
            })
            .filter(|k: &Vec<(usize, [i16; 3])>| !k.is_empty())
            .unwrap_or_else(|| {
                QUALIFIER_KINDS[..mode.cast_size().min(DANCER_SLOTS)]
                    .iter()
                    .map(|&k| (k, [0i16; 3]))
                    .collect()
            });
        let kinds: Vec<usize> = spawns.iter().map(|&(k, _)| k).collect();
        let long = long_song && mode != DanceMode::HowTo;
        let mut game = Self::with_tables(chart, tables, &kinds, long);
        game.mode = mode;
        game.widgets = dance_widgets_with_abr(overlay);
        game.kinds = cast.map(|c| c.kinds).unwrap_or_default();
        game.spawn_dancer_actors(&spawns);
        Some(game)
    }

    /// Seed the dancer actor pool from the mode's spawn table, mirroring
    /// `FUN_801d0190`'s per-record stores: position into `+0x14`/`+0x16`/`+0x18`,
    /// the floor slot into `+0x5A`, the kind descriptor's idle clip (masked to
    /// `0x1FF`) into `+0x5C` and its rate word into `+0x6A`.
    // PORT: FUN_801d0190 (the per-dancer actor spawn stores)
    fn spawn_dancer_actors(&mut self, spawns: &[(usize, [i16; 3])]) {
        self.actors.clear();
        for (slot, &(kind, home)) in spawns.iter().enumerate() {
            if let Some(d) = self.dancers.get_mut(slot) {
                d.home = home;
                if let Some(k) = self.kinds.get(kind) {
                    d.bind_clip(&k.idle);
                }
            }
            let mut a = crate::minigame_actor::MinigameActor::at(home, 0);
            a.live_mask = slot as u16;
            self.actors.push(a);
        }
        self.parts.clear();
        self.sync_dancer_actors();
    }

    /// Spawn one sprite part, the shape `FUN_801d3fd0` builds: the spec's
    /// already-shifted screen pair into `+0x14`/`+0x16`, the sprite id into
    /// `+0x50`, the fixed `0x1000` scale.
    ///
    /// Both [`step_mark_effect_spawn`] and [`good_banner_spawn`] produce these
    /// specs, and the run spawns the banner set itself on a scoring judge, so
    /// the pool fills from gameplay rather than from a host.
    // PORT: FUN_801d3fd0 (the spawn's actor-field stores)
    pub fn spawn_sprite_part(&mut self, spec: &crate::baka_fighter::EffectSpawnSpec) -> usize {
        let mut a = crate::minigame_actor::MinigameActor::at([spec.x, spec.y, 0], PART_DRAW_MODE);
        a.sprite = spec.sprite_id;
        a.scale = spec.scale;
        // A fresh part starts at the top of the fade ramp - see
        // [`DanceGame::advance`]'s aging pass for why the port drives `+0x78`
        // downward.
        a.beat = crate::minigame_actor::BEAT_FADE_CEILING;
        self.parts.push(a)
    }

    /// Rebuild each dancer actor's live fields off its dancer's state. Runs
    /// once per [`DanceGame::advance`], which is what keeps the record a
    /// production datum rather than a test fixture.
    fn sync_dancer_actors(&mut self) {
        for (slot, d) in self.dancers.iter().enumerate() {
            let Some(a) = self.actors.get_mut(slot) else {
                continue;
            };
            a.pos = d.home;
            // `+0x26` is the yaw the groovy-move spin drives (retail wraps it
            // at 0x1000 per turn; `spin_acc` is that same accumulator).
            a.yaw = (d.spin_acc % SPIN_TURN_UNITS) as i16;
            a.field_5c = d.clip;
            a.cursor = d.clip_rate as i16;
            a.flags = d.flags;
            // The `0x1000` arm of the clip gate: retail raises it for an actor
            // whose clip must keep running even with nothing bound, which for
            // the dance floor is the groovy-move spin.
            a.set_drives_clip(d.spin_turns > 0);
        }
    }

    /// Age the sprite parts one frame and retire the faded ones.
    ///
    /// `+0x78` is the slot [`sprite_part_fade_weight`] reads, and its *writer*
    /// is not in the dump corpus - no caller of `FUN_801d387c` exists there
    /// either, because the address sits as an actor-prototype callback word.
    /// The port therefore drives it **down** the prologue's own ramp: a part
    /// spawns at [`crate::minigame_actor::BEAT_FADE_CEILING`] (weight `0xFF`
    /// after the prologue's `>> 4` and clamp) and decays to zero, so it fades
    /// out over its life the way every other banner in the port does. That is
    /// a port decision, disclosed, not a reading of a store - and it is the
    /// choice, not the ramp, that is unpinned: the arithmetic on either side of
    /// `+0x78` is the disassembly's.
    fn age_sprite_parts(&mut self, frame_delta: u32) {
        let step = (frame_delta * PART_AGE_STEP).min(u32::from(u16::MAX)) as u16;
        for p in self.parts.actors_mut() {
            p.beat = p.beat.saturating_sub(step);
            if p.beat == 0 {
                p.flags |= crate::minigame_actor::FLAG_KILLED;
            }
        }
        self.parts.retire_dead();
    }

    /// This run's mode (`DAT_801d514c`).
    pub fn mode(&self) -> DanceMode {
        self.mode
    }

    /// Wired: the play window's dance block (`window/hud.rs`) lays the HUD out
    /// from this list in retail 320x240 framebuffer coordinates each frame,
    /// upscaled through the same stage transform the menu chrome uses. The
    /// `rival_hud` gate stands in for `_DAT_8007B6D0`: the host raises it in
    /// the two versus modes ([`DanceMode::Qualifier`] / [`DanceMode::Finals`]),
    /// which is when retail's dance-hall script sets the flag.
    ///
    /// PORT: FUN_801d231c - one frame of the HUD driver, laid out off this
    /// run's own live state.
    ///
    /// `rival_hud` is the `_DAT_8007B6D0` gate: with it clear the two rival
    /// gauges and beat tracks are not drawn at all, even in the versus modes.
    ///
    /// The output is pinned non-vacuous against a real overlay by
    /// `engine-core/tests/dance_minigame_real.rs`.
    pub fn hud_draws(&self, rival_hud: bool) -> Vec<DanceHudDraw> {
        dance_hud_draws(
            self.mode.value(),
            [
                self.dancer_score(0),
                self.dancer_score(1),
                self.dancer_score(2),
            ],
            [
                self.dancer_gauge(0),
                self.dancer_gauge(1),
                self.dancer_gauge(2),
            ],
            rival_hud,
        )
    }

    /// The score-box frame quads for [`DanceGame::hud_draws`], resolved through
    /// the overlay's widget table. Empty when the run was not started from a
    /// real overlay image (the table is disc data).
    pub fn hud_quads(&self, rival_hud: bool) -> Vec<DanceHudQuad> {
        let Some((widget, abr)) = self
            .widgets
            .get(DANCE_SCORE_BOX_WIDGET as usize)
            .map(|(w, a)| (w, *a))
        else {
            return Vec::new();
        };
        self.hud_draws(rival_hud)
            .into_iter()
            .filter_map(|d| match d {
                DanceHudDraw::ScoreBox { x, y } => Some(dance_hud_widget_quad(
                    widget,
                    abr,
                    x,
                    y,
                    DANCE_SCORE_BOX_WIDGET,
                    DANCE_HUD_BRIGHTNESS,
                    0x1000,
                )),
                _ => None,
            })
            .collect()
    }

    /// One number readout as widget quads, mirroring the multi-digit number
    /// renderer's emit loop (`FUN_801d32f8`): per **drawn** slot of
    /// [`dance_number_digits`] the digit's glyph-U is patched into the style's
    /// widget record and the widget is emitted at the style's fixed x step.
    ///
    /// Style A (`style_b == false`) is widget `1` - 16-texel glyphs at a 16-px
    /// step, glyph-U from [`dance_score_digit_u`]. Style B is widget `0x21`,
    /// the narrow counter - 8-texel glyphs at an 8-px step, glyph-U from
    /// [`dance_level_digit_u`]. Empty when the run has no widget table (a
    /// chart-only run not started from a real overlay image).
    pub fn number_quads(&self, style_b: bool, value: u32, x: i16, y: i16) -> Vec<DanceHudQuad> {
        let (widget_id, step, glyph_u): (usize, i16, fn(u8) -> u8) = if style_b {
            (0x21, 8, dance_level_digit_u)
        } else {
            (1, 16, dance_score_digit_u)
        };
        let Some((widget, abr)) = self.widgets.get(widget_id).map(|(w, a)| (*w, *a)) else {
            return Vec::new();
        };
        dance_number_digits(value)
            .iter()
            .enumerate()
            .filter_map(|(i, d)| d.map(|d| (i, d)))
            .map(|(i, d)| {
                let mut w = widget;
                w.u = glyph_u(d);
                dance_hud_widget_quad(
                    &w,
                    abr,
                    x + step * i as i16,
                    y,
                    widget_id as u32,
                    DANCE_HUD_BRIGHTNESS,
                    0x1000,
                )
            })
            .collect()
    }

    /// The `Lv.` gauge readout as widget quads (`FUN_801d3e28`'s emit pair):
    /// the label widget `6` at `(x, y)` and the digit widget `7` eight pixels
    /// on, its glyph-U patched through [`score_thousands_glyph_u`] from the
    /// dancer's raw `gauge` (the `value / 1000` level digit), both at
    /// [`DANCE_HUD_BRIGHTNESS`] and scale `0x1000`. Empty without a widget
    /// table.
    pub fn gauge_readout_quads(&self, gauge: u32, x: i16, y: i16) -> Vec<DanceHudQuad> {
        let mut out = Vec::new();
        if let Some((w, a)) = self.widgets.get(6) {
            out.push(dance_hud_widget_quad(
                w,
                *a,
                x,
                y,
                6,
                DANCE_HUD_BRIGHTNESS,
                0x1000,
            ));
        }
        if let Some((w, a)) = self.widgets.get(7).map(|(w, a)| (*w, *a)) {
            let mut w2 = w;
            w2.u = score_thousands_glyph_u(gauge as i32) as u8;
            out.push(dance_hud_widget_quad(
                &w2,
                a,
                x + 8,
                y,
                7,
                DANCE_HUD_BRIGHTNESS,
                0x1000,
            ));
        }
        out
    }

    /// The full per-frame HUD **quad** list: the score-box frames
    /// ([`DanceGame::hud_quads`]) plus, per [`DanceGame::hud_draws`] element,
    /// the style-A digit run for each score readout and the `Lv.` label +
    /// digit pair (with the style-B narrow gauge counter beside it) for each
    /// gauge readout. This is the textured-quad half of the HUD driver frame;
    /// which glyph each quad samples is carried in its patched `uv`, so a host
    /// without the dance sprite page resident still receives the retail
    /// geometry + gouraud colours.
    pub fn hud_draw_quads(&self, rival_hud: bool) -> Vec<DanceHudQuad> {
        let mut out = self.hud_quads(rival_hud);
        for d in self.hud_draws(rival_hud) {
            match d {
                DanceHudDraw::Score { x, y, value, .. } => {
                    out.extend(self.number_quads(false, value, x, y));
                }
                DanceHudDraw::Gauge { x, y, value, .. } => {
                    out.extend(self.gauge_readout_quads(value, x, y));
                    out.extend(self.number_quads(true, value, x + 0x18, y));
                }
                _ => {}
            }
        }
        out
    }

    /// The triangle feedback window's remaining frames (`DAT_801d5144`) - the
    /// raw counter behind [`DanceGame::triangle_feedback`], which the tutorial
    /// actor's practice step reads directly.
    pub fn feedback_frames(&self) -> u32 {
        self.feedback
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

    // --------------------------------------------------------------- actors

    /// The dancer actor pool - one record per floor slot, live every frame.
    ///
    /// This is the port's equivalent of the actors `FUN_801d0190` spawns:
    /// position, flag word, bound clip id and beat field, in the retail slots
    /// the dance overlay's draw kernels read.
    pub fn dancer_actors(&self) -> &[crate::minigame_actor::MinigameActor] {
        self.actors.actors()
    }

    /// One frame of per-dancer clip work: [`dance_clip_driver_gate`] applied to
    /// each floor slot's actor record.
    ///
    /// `clip_driver` says whether the shared clip driver runs for that dancer
    /// this frame - the whole of `FUN_801d4098`.
    pub fn dancer_clip_frames(&self) -> Vec<DancerClipFrame> {
        self.actors
            .actors()
            .iter()
            .enumerate()
            .map(|(slot, a)| DancerClipFrame {
                slot,
                clip_id: a.field_5c,
                clip_rate: a.cursor as u16,
                clip_driver: dance_clip_driver_gate(a.field_5c, a.flags),
                translucent: a.flags & crate::minigame_actor::FLAG_TRANSLUCENT != 0,
            })
            .collect()
    }

    /// The live sprite parts.
    pub fn sprite_parts(&self) -> &[crate::minigame_actor::MinigameActor] {
        self.parts.actors()
    }

    /// One frame of sprite-part draw work: [`sprite_part_emit`] and
    /// [`sprite_part_fade_weight`] applied to every live part.
    ///
    /// This is what makes those two kernels live. A host draws the emitted
    /// quads (the shadowed arm is two of them per part) at
    /// [`SpritePartEmit`]'s screen pair, modulated by `fade`.
    pub fn sprite_part_emits(&self) -> Vec<SpritePartFrame> {
        self.parts
            .actors()
            .iter()
            .enumerate()
            .map(|(index, a)| SpritePartFrame {
                index,
                emit: sprite_part_emit(a.draw_mode, a.pos[0], a.pos[1], a.sprite),
                fade: sprite_part_fade_weight(a.beat),
                sprite: a.sprite,
            })
            .collect()
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

        // `FUN_801d1358` rebinds a dancer's loop clip once its judge-triggered
        // reaction / move clip has run out - the port's stand-in for the clip's
        // own playback is the note latch, which is what the judge arms and
        // what this frame's decay above clears.
        for i in 0..self.dancers.len() {
            if self.dancers[i].latch_timer == 0 {
                self.bind_loop_clip(i);
            }
        }
        self.sync_dancer_actors();
        self.age_sprite_parts(frame_delta);
    }

    /// Bind dancer `i`'s standing clip: the kind descriptor's dance-groove loop
    /// during a run, its idle before the beat clock has moved.
    fn bind_loop_clip(&mut self, i: usize) {
        let Some(kind) = self.dancers.get(i).map(|d| d.kind) else {
            return;
        };
        let Some(k) = self.kinds.get(kind).cloned() else {
            return;
        };
        let clip = if self.song_timer > 0 { k.dance } else { k.idle };
        if let Some(d) = self.dancers.get_mut(i) {
            d.bind_clip(&clip);
        }
    }

    /// Bind the judge-returned move-pair clip on dancer `i`
    /// (`FUN_801d1af4` returns the pair index, `FUN_801d1358` applies it).
    fn bind_move_clip(&mut self, i: usize, pair: usize) {
        let Some(kind) = self.dancers.get(i).map(|d| d.kind) else {
            return;
        };
        let Some(clip) = self
            .kinds
            .get(kind)
            .and_then(|k| k.moves.get(pair))
            .copied()
        else {
            return;
        };
        if let Some(d) = self.dancers.get_mut(i) {
            d.bind_clip(&clip);
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
        let lane = self.dancers[i].lane(self.chart.rows.len()) as usize;
        let ev = if dir.is_triangle() {
            self.spend_triangle(i, beat)
        } else {
            self.judge_direction(i, dir, beat)
        };
        // `FUN_801d1af4`'s return value is a **move-pair index** into the
        // dancer's kind descriptor, which `FUN_801d1358` binds into the
        // actor's `+0x5C` / `+0x6A`. The mapping is the one
        // `legaia_asset::dance_cast` documents: pair 0/1 = the Square/Circle
        // miss reaction, pair `lane*2 + 2 (+1)` = the closed-chain move, pair
        // `8 + lane` = the on-beat / timing-button step. A plain matched note
        // that does not close the chain returns nothing and binds nothing.
        use legaia_asset::dance_cast as dc;
        let circle = dir.symbol() == 2;
        match ev {
            DanceEvent::Miss => {
                let pair = if circle {
                    dc::MOVE_MISS_CIRCLE
                } else {
                    dc::MOVE_MISS_SQUARE
                };
                self.bind_move_clip(i, pair);
            }
            DanceEvent::Sequence { weight, .. } => {
                self.bind_move_clip(i, dc::move_sequence_pair(lane, circle));
                // The human's closed chain fires the sequence-clear banner,
                // which is three `FUN_801d3fd0` spawns into the part pool.
                if i == 0 {
                    let b = good_banner_spawn(weight.min(0xFFFF) as u16);
                    self.spawn_sprite_part(&b.banner);
                    for s in &b.stars {
                        self.spawn_sprite_part(s);
                    }
                }
            }
            DanceEvent::Groovy { .. } => {
                self.bind_move_clip(i, dc::move_beat_pair(lane));
            }
            DanceEvent::Hit { .. } | DanceEvent::Ignored | DanceEvent::NoCharge => {}
        }
        self.sync_dancer_actors();
        ev
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

// Wired: [`good_banner_spawn`] composes three of these records, and the play
// window's minigame effect pool (`window/minigame_fx.rs`) hosts the spawns -
// the sequence-clear banner + stars are spawned into it on the human's scoring
// judge and aged / drawn per frame. The overlay's own sprite page is still not
// uploaded, so the pool draws each part through its placeholder glyph rather
// than the `sprite_id` cell.
/// PORT: FUN_801d3fd0 - the dance overlay's cell-placed effect spawn (the
/// step-mark flash): retail zero-fills a spawn record, spawns through the
/// shared part-spawn API `FUN_80021B04` at scale `0x1000`, stamps
/// `sprite_id` into the part's `+0x50` and places it at the dance-grid cell
/// (`x << 3`, `y << 3` into `+0x14`/`+0x16`). The Baka Fighter's
/// screen-centre twin is [`crate::baka_fighter::center_effect_spawn`]
/// (`FUN_801d6e04`).
pub fn step_mark_effect_spawn(
    cell_x: i16,
    cell_y: i16,
    sprite_id: u16,
) -> crate::baka_fighter::EffectSpawnSpec {
    crate::baka_fighter::EffectSpawnSpec {
        x: cell_x << 3,
        y: cell_y << 3,
        scale: 0x1000,
        sprite_id,
    }
}

// Wired: [`DanceGame::gauge_readout_quads`] patches this into widget `7`'s
// texture-U before emitting the pair, on the live HUD path
// ([`DanceGame::hud_draw_quads`] -> the play window's dance block).
/// PORT: FUN_801d3e28 - the score-banner thousands-digit glyph selector:
/// retail stores `(score / 1000) * 8 - 0x30` into the banner widget's
/// texture-U byte (`DAT_801d4760`) - each digit glyph is 8 texels wide and
/// the cell base sits `0x30` texels left of digit `0` - then draws widget
/// ids 6 and 7 (the two banner halves, 8 px apart) through the hub sprite
/// emitter `FUN_801d2f38` at brightness `0x80`, scale `0x1000`.
pub fn score_thousands_glyph_u(score: i32) -> i8 {
    ((score / 1000) * 8 - 0x30) as i8
}

// ------------------------------------------------------------ HUD kernels
//
// The overlay draws its whole HUD through the shared textured-quad emitter
// (`FUN_801d2f38`, an engine-ui concern). These functions carry the *arithmetic*
// those draws are parameterised by - the value a widget's texture-U / screen-x /
// CLUT is patched to - which is disc-derived game logic, not rendering. Each is
// the computational content of a HUD render routine; the quad emit stays a host
// job, exactly as [`step_mark_effect_spawn`] / [`score_thousands_glyph_u`]
// already split `FUN_801d3fd0` / `FUN_801d3e28`.

/// PORT: FUN_801d32f8 - the multi-digit number renderer's decimal split.
///
/// Retail walks eight decimal places most-significant first, storing the running
/// quotient `value / 10^(7-i)` only when it is non-zero (leading slots stay at
/// its `-1` sentinel and draw nothing), then per drawn slot rewrites one HUD
/// widget's texture-U (via [`dance_score_digit_u`] / [`dance_level_digit_u`]) and
/// x before emitting it. This is the split: `Some(digit)` per drawn slot,
/// `None` for a suppressed leading zero. `0` renders as no digits at all, matching
/// the retail sentinel (the drawn-slot test is "quotient non-zero").
///
/// Wired: the dance HUD's score readout in the play window runs through this,
/// so a blank slot really does draw nothing there.
pub fn dance_number_digits(value: u32) -> [Option<u8>; 8] {
    let mut out = [None; 8];
    let mut place = 10_000_000u32; // 10^7 - the leading of eight digit slots
    for slot in out.iter_mut() {
        let quotient = value / place;
        if quotient != 0 {
            *slot = Some((quotient % 10) as u8);
        }
        place /= 10;
    }
    out
}

// Wired: [`DanceGame::number_quads`] (style A) patches this into widget `1`'s
// texture-U per drawn digit slot, on the live HUD path
// ([`DanceGame::hud_draw_quads`] -> the play window's dance block).
/// PORT: FUN_801d32f8 style-A digit glyph-U (the score boxes, widget `1`):
/// `digit * 0x10` - each score glyph is 16 texels wide, drawn at a 16-px x step.
pub fn dance_score_digit_u(digit: u8) -> u8 {
    digit * 0x10
}

// Wired: [`DanceGame::number_quads`] (style B) patches this into widget
// `0x21`'s texture-U per drawn digit slot, on the same live HUD path as
// [`dance_score_digit_u`].
/// PORT: FUN_801d32f8 style-B digit glyph-U (widget `0x21`, the narrow counter):
/// `digit * 8 + 0x40` - 8-texel glyphs offset `0x40` into the page, 8-px x step.
pub fn dance_level_digit_u(digit: u8) -> u8 {
    digit * 8 + 0x40
}

/// Beat-track CLUT ids (`FUN_801d2524`): the caps + body idle palette.
pub const BEAT_TRACK_CLUT_IDLE: u16 = 0x7d08;
/// Beat-track combo-window flash palette (caps + body, on the combo slot).
pub const BEAT_TRACK_CLUT_COMBO: u16 = 0x7d0d;
/// Beat-track scrolling-note palette.
pub const BEAT_TRACK_CLUT_NOTE: u16 = 0x7d0e;
/// Intra-beat phase below which the combo slot flashes (`FUN_801d2524`: `< 0x46`).
pub const COMBO_FLASH_WINDOW: u32 = 0x46;

/// Wired: [`dance_combo_window_bright`], which the play window's beat-track row
/// reads every frame.
///
/// PORT: FUN_801d2524 - the beat-track's combo-slot mask. The beat index is
/// masked to 8 once the dancer has promoted a level (`gauge / 1000 > 0`), else 4,
/// so the flash + note read-out cadence widens on the higher rows.
pub fn dance_beat_level_mask(level: u32) -> u32 {
    if level > 0 { 7 } else { 3 }
}

/// This is the **displayed** combo slot, and it is not the judged one:
/// [`DanceGame::on_combo_slot`] masks the beat by `3` and accepts the whole
/// window, while the track's flash widens its mask with the dancer's level and
/// uses the much narrower [`COMBO_FLASH_WINDOW`]. Keep the two apart - the cell
/// the track lights is not the cell the judge scores.
///
/// Wired: the play window's dance HUD lights its beat-track row from this.
///
/// PORT: FUN_801d2524 - the combo-window flash test: the beat track lights its
/// caps + body ([`BEAT_TRACK_CLUT_COMBO`]) on the masked combo slot inside the
/// flash window, else it stays [`BEAT_TRACK_CLUT_IDLE`].
pub fn dance_combo_window_bright(beat: u32, level: u32, frac: u32) -> bool {
    beat & dance_beat_level_mask(level) == 3 && frac < COMBO_FLASH_WINDOW
}

/// Wired: the play window's dance HUD places its upcoming-note row at these
/// offsets (from its own pen, not the overlay's screen constant).
///
/// PORT: FUN_801d2524 - the scrolling note's screen-x. Note `i` sits at
/// `base_x + i*16`, scrolled left by the intra-beat fraction
/// (`(frac * 16) / BEAT_PERIOD + 5`) and a fixed 4-texel inset, so the row of
/// notes slides one 16-px cell per beat toward the judge line.
pub fn dance_beat_track_note_x(base_x: i32, i: u32, frac: u32) -> i32 {
    base_x + (i as i32) * 16 - ((frac * 16 / BEAT_PERIOD) as i32 + 5) - 4
}

// REF: FUN_80065034 (the voice-attr primitive both key-ons go through)
/// Channel mixer level both sting voices are keyed at (`li a1,0x2`).
pub const STING_LEVEL: i8 = 2;

/// VAB **program** both sting voices come from (`li a2,0x1`) - the argument
/// the port used to drop, and the one that says which tone bank the
/// `2r` / `2r + 1` tone indices are inside.
pub const STING_PROGRAM: u8 = 1;

/// Sting variants the tier-2 award site draws between: `r = rand() % 3`, the
/// `0x55555556` magic-multiply divide at `FUN_801d1af4 + 0x64C`.
pub const STING_RANDOM_VARIANTS: u16 = 3;

/// The **fixed** sting the three groovy-move tiers key instead, and it is
/// outside the random space: all three of the tier-3 / 4 / 5 arms of
/// `FUN_801d1af4` reach `FUN_801d3d78` with a literal `5` (two `li a0,0x5`,
/// and a `move a0,v0` off the `li v0,0x5` the tier compare just loaded). So a
/// groovy move is not "cue only" - it fires cue `0x202` / `0x203` / `0x205`
/// *and* this sting, at tones `0xA` / `0xB` and note `0x41`.
pub const STING_TIER_VARIANT: u16 = 5;

/// One of the two voices a good-step sting keys (`FUN_801d3d78`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceStingVoice {
    /// Voice id handed to the SPU key-on primitive (`0x12` / `0x13`).
    pub voice: u16,
    /// Channel mixer level ([`STING_LEVEL`]).
    pub level: i8,
    /// VAB program ([`STING_PROGRAM`]).
    pub program: u8,
    /// Tone within [`STING_PROGRAM`] (`2r` / `2r + 1`).
    pub tone: i16,
    /// Note the voice is keyed at (`0x3c + r`).
    pub note: i16,
}

// Wired: the browser dance page's `dance_sting`
// (`web-viewer::minigames_dance`) takes its `(program, tone, note)` triple
// from here rather than recomputing it, and decodes the named tone out of the
// overlay's own VAB - which is what makes the bank index a read of
// [`STING_PROGRAM`] instead of a literal `1` that happened to agree. The
// native window is a separate case and does still lack a key-on API:
// `AudioBgmDirector::enqueue_sfx` only schedules cue ids.
/// PORT: FUN_801d3d78 - the on-beat "good step" sting. A judged direction fires
/// **no** ring cue; it keys two voices together through the SPU voice-attr
/// primitive (`FUN_80065034`, whose eight arguments are `(voice, level,
/// program, tone, note, 0x40, vol_l, vol_r)`): voice `0x12` at tone `2r` and
/// voice `0x13` at tone `2r + 1`, both in program [`STING_PROGRAM`] at level
/// [`STING_LEVEL`] and note `0x3c + r`. Both volume slots are the voice-volume
/// config `_DAT_80084580` halved, the same value
/// [`crate::other_game_overlay::cue_volume`] decodes. Returns the two voice
/// descriptors; the key-on itself is the audio host's.
///
/// `r` is not always a random pick. `FUN_801d1af4` reaches this from **four**
/// sites: the tier-2 chain-closed award passes `rand() % 3`
/// ([`STING_RANDOM_VARIANTS`]), and each of the three groovy-move tiers passes
/// the literal [`STING_TIER_VARIANT`]. Anything that enumerates "the stings"
/// over `0..3` is missing the one the higher tiers play.
pub fn dance_hit_sting_voices(r: u16) -> [DanceStingVoice; 2] {
    let note = 0x3c + r as i16;
    let voice = |voice: u16, tone: i16| DanceStingVoice {
        voice,
        level: STING_LEVEL,
        program: STING_PROGRAM,
        tone,
        note,
    };
    [voice(0x12, (2 * r) as i16), voice(0x13, (2 * r + 1) as i16)]
}

/// The sequence-clear ("Good!") banner and its two flanking star sparkles
/// (`FUN_801d40dc`). The two stars carry the press's accuracy weight - retail
/// stores it into each star actor's `+0x72`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoodBannerSpawns {
    /// The "Good!" banner sprite (id `0xb`) at screen centre.
    pub banner: crate::baka_fighter::EffectSpawnSpec,
    /// The two star sparkles (sprite id `0x16`) flanking the banner.
    pub stars: [crate::baka_fighter::EffectSpawnSpec; 2],
    /// The accuracy weight stamped into each star's `+0x72`.
    pub weight: u16,
}

// Wired: the play window spawns this on the human's scoring judge
// (`Judge::Sequence`) into its minigame effect pool
// (`window/minigame_fx.rs`), which ages and draws the three parts. The dance
// overlay's sprite ids `0xb` / `0x16` are still not resident, so the pool's
// placeholder glyphs stand in for the banner art.
/// PORT: FUN_801d40dc - spawn the sequence-clear banner + two stars. Retail
/// issues three `FUN_801d3fd0` spawns - `(0xa0, 0x90, sprite 0xb)` for the banner
/// and `(0x68, 0x90, sprite 0x16)` / `(0xd8, 0x90, sprite 0x16)` for the stars
/// (the banner centred, the stars `0x38` to either side) - then stamps the
/// accuracy `weight` into each star's `+0x72`. The spawn records go through the
/// same primitive as [`step_mark_effect_spawn`].
pub fn good_banner_spawn(weight: u16) -> GoodBannerSpawns {
    GoodBannerSpawns {
        banner: step_mark_effect_spawn(0xa0, 0x90, 0xb),
        stars: [
            step_mark_effect_spawn(0x68, 0x90, 0x16),
            step_mark_effect_spawn(0xd8, 0x90, 0x16),
        ],
        weight,
    }
}

/// The intro-cue id the count-in banner fires once, when it crosses into its
/// hold segment (`FUN_801d2d98`, into the runtime SFX bank; see `sfx-table.md`).
pub const COUNTIN_INTRO_CUE: u16 = 0x200;

/// The count-in banner's slide / hold / fade envelope for one frame
/// (`FUN_801d2d98`). The two banner halves emit through the hub sprite emitter
/// [`FUN_801d2f38`]; this is the arithmetic that emit is fed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountInBanner {
    /// Horizontal offset of the sliding halves from screen centre (`0xa0`); `0`
    /// during the hold. Retail places the right half at `0xa0 + x_offset`, the
    /// left at `0xa0 - x_offset`.
    pub x_offset: i32,
    /// Brightness passed to the emit, clamped `0..=0xff`. Halved for the two
    /// sliding halves; full for the single held banner.
    pub brightness: i32,
    /// `true` = the single centred banner (widget `0x78`, opaque); `false` = the
    /// two half-brightness sliding halves (widget `0x77`).
    pub hold: bool,
}

// Wired: the play window runs a pre-song count-in phase (the shell holds the
// parsed [`DanceGame`] pending while the banner's own frame counter runs,
// entering the dance only when the envelope finishes - the `FUN_801cf470`
// below-10 states as a host phase). The host owns the counter and the
// once-only [`COUNTIN_INTRO_CUE`] latch; this returns the envelope.
/// PORT: FUN_801d2d98 - the count-in banner animator (`1 2 3 READY... GO!`).
///
/// Three segments keyed on the banner's own frame counter `frame`:
/// **slide-in** (`frame < 0x1e`): the two halves fly in from `0xb4 - 6*frame`
/// to centre at flat brightness `0x80`; **hold** (`0x1e..0x5a`): a single
/// centred banner whose brightness ramps `((frame-0x1e)*0x7f)/0x1e + 0x80`
/// (clamped `0xff`) - and the once-only intro cue [`COUNTIN_INTRO_CUE`] fires
/// on entry; **slide-out** (`>= 0x5a`): the halves fly back out `6*(frame-0x5a)`
/// as brightness fades `200 - ((frame-0x5a)*0x7f)/0x1e`. The emit itself and the
/// cue-fire latch (`DAT_801d5134`) are the host's; this returns the envelope.
pub fn dance_countin_banner_envelope(frame: i32) -> CountInBanner {
    let (mut x_offset, mut brightness, hold);
    if frame < 0x1e {
        x_offset = 0xb4 - 6 * frame;
        brightness = 0x80;
        hold = false;
    } else {
        x_offset = 0;
        brightness = ((frame - 0x1e) * 0x7f) / 0x1e + 0x80;
        hold = true;
    }
    if frame > 0x59 {
        x_offset = 6 * (frame - 0x5a);
        brightness = 200 - ((frame - 0x5a) * 0x7f) / 0x1e;
        // The slide-out overrides the hold path back to two sliding halves.
        return CountInBanner {
            x_offset,
            brightness: brightness.clamp(0, 0xff) / 2,
            hold: false,
        };
    }
    brightness = brightness.clamp(0, 0xff);
    if !hold {
        brightness /= 2;
    }
    CountInBanner {
        x_offset,
        brightness,
        hold,
    }
}

// REF: FUN_800204f8 (the shared clip driver this gate decides to call; the
// driver itself is the move-VM consumer ported in `legaia-engine-vm`)
// Wired: [`DanceGame::dancer_clip_frames`] runs this per floor slot every frame
// off the dancer actor pool, and the pool is populated by
// [`DanceGame::from_overlay_for_mode`] from the disc's own spawn + kind tables.
//
// CORRECTION to the reason this row previously carried. The old tag read
// `+0x5C` as "the groovy-move turns left" and concluded the spin arm was
// already satisfied. It is not that slot. `FUN_801d0190` stores
// `kind_desc[0x10] & 0x1FF` - the **idle clip's anim id** - into `+0x5C` at
// spawn, and `FUN_801d1358` rewrites it with each judge-returned move clip
// (`sh v0,0x5c(s0)` at `801d1544` / `801d1584` / `801d16d4`, each preceded by
// `andi ...,0x1ff`); the groovy-move turn counter is the overlay global
// `DAT_801d564c[i]`, decremented at `801d1454` and not an actor field at all.
// So the first arm is "this actor has a clip bound", and satisfying it meant
// binding clips - which is what the dancer record now does.
/// PORT: FUN_801d4098 - the per-dancer actor clip-driver gate. Retail hands the
/// dancer to the shared clip driver `FUN_800204f8` only when its bound clip id
/// (`+0x5C`) is positive **or** its flag word (`+0x10`) carries bit `0x1000`.
/// That predicate is the whole function; the driver call is the animation
/// host's. `clip_id` is the signed `+0x5C` halfword, `flags` the actor flag
/// word.
pub fn dance_clip_driver_gate(clip_id: i16, flags: u32) -> bool {
    clip_id > 0 || (flags & crate::minigame_actor::FLAG_DRIVE_CLIP) != 0
}

// NOT WIRED, AND REDUNDANT RATHER THAN MISSING - read the second paragraph
// before treating this as wirable work.
//
// No host needs the *selector*, because every host that stamps a face already
// holds a rig id and never holds a slot index. The browser dance page resolves
// its rigs from the disc **cast table** - `castRigs()` in
// `site/js/minigame-dance.js` maps the mode's spawn records to their
// `dance_cast` kinds and passes those straight to `drawFace`, which is
// `LegaiaMinigames::dance_face_rgba(rig, pose)` - and on the qualifier floor
// those kinds are already `0/2/3`, the exact output of the overlay's hard-coded
// slot -> rig remap. The two arrive at the same rig from different data, so a
// caller appears only if a host ever drives the floor by slot index instead of
// by cast kind. Nothing does today, and nothing needs to.
//
// (The blockers an *earlier* reason named here - "no face pages resident, no
// blit pass" - are indeed long gone: `legaia_asset::dance_art::FACE_RIGS`
// carries the four rigs' strips and frame tables and `dance_art::face_window_rgba`
// performs the two `MoveImage` blits, per frame on the browser page. That
// correction is history, not an invitation: it is the paragraph above that
// says why the row is still open, and a wire placed on the strength of the
// correction alone would be a call site with nothing behind it.)
/// PORT: FUN_801d03c4 - the dancer face-stamp's rig selector. The face blit picks
/// a per-dancer VRAM strip + eye/mouth frame table by rig index; in the qualifier
/// (mode 0) the overlay remaps dancer `2 -> 3` and `1 -> 2`, so the rig id equals
/// the dancer's kind (the qualifier cast is kinds `0/2/3`). Dancers past `3` are
/// not stamped (retail early-returns); a rig `>= 5` has no jump-table case.
/// Returns the rig index; the pose-unchanged latch (`DAT_801d56cc`) and the two
/// `MoveImage` blits are the render host's.
pub fn dance_face_rig(mode: DanceMode, dancer: usize) -> Option<usize> {
    if dancer >= 4 {
        return None;
    }
    let rig = if mode == DanceMode::Qualifier {
        match dancer {
            2 => 3,
            1 => 2,
            d => d,
        }
    } else {
        dancer
    };
    (rig < 5).then_some(rig)
}

/// The scene name the dance hall stages when the minigame starts or tears
/// down (`s_other1_801D518C`) - the same `other1` bundle the fishing venue
/// uses, which is why both minigames live in the slot-A overlay band.
pub const DANCE_SCENE_NAME: &str = "other1";

/// What the dance scene stager writes, in the order retail stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceSceneStage {
    /// Scene name copied into the scene-name buffer at `0x80084548`.
    pub scene: &'static str,
    /// `_DAT_8007B880` is zeroed - the pad latch the field subsystem reads,
    /// so the frame the dance enters or leaves on cannot carry a stale press
    /// into the next mode.
    pub clear_pad_latch: bool,
    /// The word copied from the overlay's `DAT_801D5180` into
    /// `_DAT_80084540`, the scene-name buffer's preceding word (the scene
    /// **kind** the loader dispatches on).
    pub scene_kind_from_overlay: bool,
    /// `_DAT_8007BA9C` is armed to `-1` after the scene-setup helper returns.
    pub arm_value: i32,
}

// PARTIALLY WIRED: `World::enter_dance` / `World::exit_dance` apply the record's
// `clear_pad_latch` through `InputState::clear_edges`, which is the half of the
// stager the port has an equivalent for. The other three fields still have no
// consumer: the port enters the dance by suspending the current scene mode
// rather than staging the `other1` bundle, so there is no scene-name buffer to
// write, no scene-kind word to dispatch on and no `_DAT_8007BA9C` to arm.
// Those wait on the dance becoming a real scene load.
/// PORT: FUN_801d414c - the dance scene-name stager / teardown.
///
/// Copies [`DANCE_SCENE_NAME`] into the scene-name buffer at `0x80084548`
/// through the string copy `FUN_80056758`, clears the pad latch
/// `_DAT_8007B880`, stores the overlay word `DAT_801D5180` into
/// `_DAT_80084540`, calls the scene-setup helper `FUN_80026018`, and only
/// **then** arms `_DAT_8007BA9C = -1`. The ordering matters: the arm is after
/// the setup call, so a setup that re-enters cannot see the armed value.
///
/// Called once from the dance tick `FUN_801CF470`.
pub const fn dance_scene_stage() -> DanceSceneStage {
    DanceSceneStage {
        scene: DANCE_SCENE_NAME,
        clear_pad_latch: true,
        scene_kind_from_overlay: true,
        arm_value: -1,
    }
}

/// Fade weight the sprite emit derives from the part's `+0x78` halfword,
/// clamped to `0 ..= 0xFF`.
///
/// Retail reads the field as a **halfword** and compares it against `0x4000`
/// as a *signed 32-bit* value, so the "above the window" test can never see a
/// negative: a value past `0x4000` collapses the weight to zero outright
/// rather than saturating it.
// PORT: FUN_801d387c (the fade-weight prologue)
// Wired: [`DanceGame::sprite_part_emits`] applies it to every live sprite
// part every frame, over the pool [`DanceGame::advance`] ages.
//
// What is still not retail-pinned is the *producer* of `+0x78`: no caller of
// `FUN_801d387c` exists in the dump corpus (its address sits as a callback
// word, the same shape as the duel's mirrored sprite pass), so nothing shows
// which quantity the dance overlay parks there. The port drives it as the
// part's age on the prologue's own
// [`crate::minigame_actor::BEAT_FADE_CEILING`] ramp - a port decision, stated
// as one, not a reading of a store.
pub fn sprite_part_fade_weight(beat: u16) -> u8 {
    if beat as i32 > 0x4000 {
        return 0;
    }
    ((beat as i32) >> 4).min(0xFF) as u8
}

/// The emit dispatch's draw mode for a spawned sprite part.
///
/// **Not pinned to retail.** `FUN_801d387c` takes its mode from its caller and
/// no caller of that address is in the dump corpus - the address sits as an
/// actor-prototype callback word, the same shape as the duel's mirrored sprite
/// pass. Mode `2` is the shadowed arm, the two-emit draw that applies the
/// `>> 3` inverse of the spawn's `<< 3`; the port uses it and says so rather
/// than implying a disassembly reading.
pub const PART_DRAW_MODE: u32 = 2;

/// `+0x78` units a sprite part sheds per frame.
///
/// A **port decision**, not a retail constant: nothing in the dump corpus
/// writes `+0x78` for this actor family, so the engine spawns a part at
/// [`crate::minigame_actor::BEAT_FADE_CEILING`] and decays it down the fade
/// prologue's own ramp. At this step a part reaches zero after 64 frames, the
/// same order as the play window's placeholder part lifetime.
pub const PART_AGE_STEP: u32 = 0x100;

/// One dancer's resolved per-frame clip work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DancerClipFrame {
    /// Floor slot (`0` = the human).
    pub slot: usize,
    /// The actor's bound clip id (`+0x5C`), for a host that wants to play it.
    pub clip_id: i16,
    /// The bound clip's cursor step (`+0x6A`).
    pub clip_rate: u16,
    /// What [`dance_clip_driver_gate`] resolved from `+0x5C` / `+0x10`: the
    /// shared clip driver runs for this actor this frame.
    pub clip_driver: bool,
    /// The bound clip asked for a translucent draw (anim word bit `0x200`).
    pub translucent: bool,
}

/// One sprite part's resolved per-frame draw work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpritePartFrame {
    /// Index into [`DanceGame::sprite_parts`].
    pub index: usize,
    /// What [`sprite_part_emit`] resolved for the part's draw mode.
    pub emit: SpritePartEmit,
    /// What [`sprite_part_fade_weight`] resolved from the part's `+0x78`.
    pub fade: u8,
    /// The part's `+0x50` sprite id (the spawn's third argument).
    pub sprite: u16,
}

/// Which emit the dancer sprite dispatch performs for a draw mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpritePartEmit {
    /// Mode `0` - no emit at all: copy the transform template's `+0x90` trio
    /// into the dancer and zero its `+0x96` / `+0x98` / `+0x9A`.
    CopyTemplate,
    /// Mode `1` - store the caller's third argument into the dancer's
    /// `+0x94`; still no emit.
    SetTemplateZ,
    /// Mode `2` - the shadowed draw: **two** emits at the dancer's screen
    /// position (its `+0x14` / `+0x16` pair rounded toward zero and shifted
    /// `>> 3`), the first with semi-transparency flag `0x400` and the second
    /// with `0x800`.
    Shadowed { x: i16, y: i16, flags: [u16; 2] },
    /// Mode `3` - one plain emit at the **unrounded** `+0x14` / `+0x16` pair.
    /// Note this mode does not divide by eight at all.
    Plain { x: i16, y: i16, flags: u16 },
    /// Mode `4` - the marker draw: one emit with flags forced to `1` and the
    /// scale word forced to `0x1000`, after stamping `sprite << 4` into the
    /// overlay byte `DAT_801D46E8`.
    Marker { x: i16, y: i16, clut_byte: u8 },
    /// A mode past the five-entry jump table - nothing is drawn.
    None,
}

// Wired: [`DanceGame::sprite_part_emits`] calls this once per live sprite part
// every frame, over the pool [`DanceGame::spawn_sprite_part`] fills and
// [`DanceGame::advance`] ages. The prerequisite the old tag named - "a
// minigame actor record" carrying the `+0x14/+0x16` pair, the `+0x50` sprite
// word and the `+0x78` field - is
// [`crate::minigame_actor::MinigameActor`].
//
// CORRECTION, and it is what decides where the wire belongs. The old tag (and
// this function's old name, `dancer_emit`) read this as the **dancer's** draw.
// It is not: the actor family it reads is the one `FUN_801d3fd0` spawns.
// That spawner stamps `+0x50 = sprite_id` and stores `x << 3` / `y << 3` into
// `+0x14` / `+0x16` (`801d401c`..`801d4024`), and this dispatch reads exactly
// those three slots and shifts the pair back down by three - an exact inverse,
// on a spawner whose only two callers in the port are
// [`step_mark_effect_spawn`] and [`good_banner_spawn`]. The dancer bodies
// `FUN_801d0190` spawns carry a *world* triple in `+0x14`..`+0x18` and no
// `+0x50` at all, so a `>> 3` of a dancer's position lands hundreds of pixels
// off-screen.
//
// The old tag's second prerequisite, "a quad sink", was **wrong about the
// engine**: `legaia_engine_ui::screen_prim` has carried a PSX screen-space
// quad (`ScreenQuad` / `FlatQuad`, per-vertex gouraud, CLUT + texpage, ABR
// mode, ordering-table bucket) with both hosts consuming its `build_geometry`
// output since the battle-intro work. What genuinely has no sink is the
// *texel source*: no dance sprite page is resident in engine VRAM, which is
// why both hosts still degrade the emitted quads to a placeholder rather than
// sampling the overlay's page.
/// PORT: FUN_801d387c - the sprite-part / shadow emit dispatch.
///
/// `mode` selects one of five arms through a jump table; `x` / `y` are the
/// part's `+0x14` / `+0x16` pair and `sprite` its `+0x50` word. The two
/// emitting arms round toward zero **before** the `>> 3` (retail's
/// `bgez / addiu 7 / sra 3`), so a part at `-1` maps to screen `0` and not
/// `-1`.
pub fn sprite_part_emit(mode: u32, x: i16, y: i16, sprite: u16) -> SpritePartEmit {
    let scaled = |v: i16| -> i16 {
        let v = v as i32;
        ((if v < 0 { v + 7 } else { v }) >> 3) as i16
    };
    match mode {
        0 => SpritePartEmit::CopyTemplate,
        1 => SpritePartEmit::SetTemplateZ,
        2 => SpritePartEmit::Shadowed {
            x: scaled(x),
            y: scaled(y),
            flags: [sprite | 0x400, sprite | 0x800],
        },
        3 => SpritePartEmit::Plain {
            x,
            y,
            flags: sprite,
        },
        4 => SpritePartEmit::Marker {
            x,
            y,
            clut_byte: (sprite << 4) as u8,
        },
        _ => SpritePartEmit::None,
    }
}

// --------------------------------------------- HUD widget quad + HUD driver

/// One resolved dance HUD quad - the renderer-agnostic form of the 12-word
/// `POLY_GT4` packet the overlay's emitter builds.
///
/// Deliberately **not** [`crate::baka_fighter::HudWidgetQuad`]: the two
/// emitters differ on their edges. Baka's spans `u ..= u + w - 1` inclusive;
/// the dance emitter writes `u + w` and `x + hw` straight out, so its rects
/// are half-open. Sharing one struct would silently pick one convention for
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceHudQuad {
    /// GP0 polygon code (`(semi << 1) | 0x3C`).
    pub poly_code: u8,
    /// Quad corners, **half-open**: `x0 .. x1` by `y0 .. y1`.
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Per-corner texture coordinates in vertex order (TL, TR, BL, BR),
    /// half-open the same way.
    pub uv: [(u8, u8); 4],
    /// Brightness-scaled gouraud colours: verts 0/1 take `rgb_top`, verts 2/3
    /// take `rgb_bottom`.
    pub rgb_top: [u8; 3],
    pub rgb_bottom: [u8; 3],
    /// CLUT id, after the mode-2 override.
    pub clut: u16,
    /// Texpage attribute after the ABR fold (`tpage + abr * 0x20`).
    pub tpage_attr: u16,
}

/// Parse the HUD widget table out of an overlay image, pairing each record
/// with its `+0x13` ABR byte.
///
/// `legaia_asset::dance_art::parse_widgets` decodes every field the emitter
/// reads **except** `+0x13`, the semi-transparency rate that folds into the
/// texpage attribute. Until that parser carries it the byte is lifted here off
/// the same committed offsets the parser uses, so there is one source for the
/// table's geometry.
pub fn dance_widgets_with_abr(overlay: &[u8]) -> Vec<(legaia_asset::dance_art::DanceWidget, u8)> {
    use legaia_asset::dance_art::{DANCE_OVERLAY_BASE_VA, WIDGET_STRIDE, WIDGET_TABLE_VA};
    let Ok(widgets) = legaia_asset::dance_art::parse_widgets(overlay) else {
        return Vec::new();
    };
    let base = (WIDGET_TABLE_VA - DANCE_OVERLAY_BASE_VA) as usize;
    widgets
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let abr = overlay
                .get(base + i * WIDGET_STRIDE + 0x13)
                .copied()
                .unwrap_or(0);
            (w, abr)
        })
        .collect()
}

/// Widget id bits the emitter takes as the widget **index** (`id & 0x3FF`).
pub const DANCE_WIDGET_ID_MASK: u32 = 0x3FF;
/// Blend mode the emitter takes from the id's upper bits (`id >> 10`).
pub const DANCE_WIDGET_MODE_SHIFT: u32 = 10;
/// CLUT the emitter substitutes when the id's mode field is `2`: palette
/// `0x0F` of the row-500 strip, i.e. VRAM `(240, 500)`.
pub const DANCE_MODE2_CLUT: u16 = 0x7D0F;

/// The MIPS `mult` / `sra` scale idiom: signed multiply, then shift with the
/// `bgez` bias so the round is toward zero.
fn mips_scale(value: i32, factor: i32, shift: u32) -> i32 {
    let p = value.wrapping_mul(factor);
    let p = if p < 0 { p + ((1 << shift) - 1) } else { p };
    p >> shift
}

// Wired: [`DanceGame::hud_quads`] / [`DanceGame::number_quads`] /
// [`DanceGame::gauge_readout_quads`] all emit through this, and the play
// window's dance block builds the full quad list per frame
// ([`DanceGame::hud_draw_quads`]). The dance overlay's 4bpp page at `(512, 0)`
// is still not uploaded (its art is staged by the entry path, PROT 1230), so
// the host's quad sink materialises the rects only against a solid atlas
// source - the geometry, gouraud colours and patched `uv` are live every
// frame regardless.
//
// A second, narrower gap is already closed here: the record's `+0x13` ABR byte
// is not decoded by `legaia_asset::dance_art::parse_widgets`, so
// [`dance_widgets_with_abr`] lifts it and the caller passes it in.
/// PORT: FUN_801d2f38 - the dance overlay's textured-quad emitter, the
/// sibling of Baka Fighter's `FUN_801d5ed0`.
///
/// `FUN_801d2f38(x, y, id, brightness, size)` draws one record of the
/// 34-record widget table `DAT_801D46CC`
/// ([`legaia_asset::dance_art::parse_widgets`]) as a quad **centred** on
/// `(x, y)`:
///
/// - the id is two fields. `id & 0x3FF` is the widget index; `id >> 10`
///   (rounded toward zero) is a **blend mode** that overrides the record:
///   mode `0` takes the record's own semi-transparency bit and ABR rate, and
///   any other mode forces semi-transparency on and uses the mode value
///   *itself* as the ABR rate. Mode `2` additionally replaces the CLUT with
///   the fixed [`DANCE_MODE2_CLUT`];
/// - half-extent per axis = `((cell * scale) >> 13) * size >> 12`, both shifts
///   rounding toward zero, so `scale = size = 0x1000` is exactly `cell / 2`;
/// - every colour channel is `channel * brightness >> 8`; verts 0/1 carry
///   `rgb_top`, verts 2/3 `rgb_bottom` - a vertical gradient;
/// - the texpage attribute folds the ABR rate in as `tpage + abr * 0x20`.
///
/// Retail then links the packet into the OT bucket `DAT_801D5154` and forces
/// that slot to `3`, so every draw after the first shares one bucket. That
/// scheduling is host-side; the port returns the quad.
///
/// `abr` is the record's `+0x13` byte. [`legaia_asset::dance_art::DanceWidget`]
/// does not decode it yet, so the caller supplies it.
pub fn dance_hud_widget_quad(
    widget: &legaia_asset::dance_art::DanceWidget,
    abr: u8,
    x: i16,
    y: i16,
    id: u32,
    brightness: i32,
    size: i32,
) -> DanceHudQuad {
    let signed = id as i32;
    let mode = (if signed < 0 { signed + 0x3FF } else { signed }) >> DANCE_WIDGET_MODE_SHIFT;
    let (semi, abr) = if mode == 0 {
        (widget.semi, abr)
    } else {
        (1, mode as u8)
    };
    let scale8 = |c: u8| mips_scale(c as i32, brightness, 8).clamp(0, 0xFF) as u8;
    let half = |cell: u8| mips_scale(size, mips_scale(cell as i32, widget.scale, 13), 12) as i16;
    let hw = half(widget.w);
    let hh = half(widget.h);
    let (u0, v0) = (widget.u, widget.v);
    let (u1, v1) = (
        widget.u.wrapping_add(widget.w),
        widget.v.wrapping_add(widget.h),
    );
    DanceHudQuad {
        poly_code: (semi << 1) | 0x3C,
        x0: x - hw,
        y0: y - hh,
        x1: x + hw,
        y1: y + hh,
        uv: [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        rgb_top: widget.rgb_top.map(scale8),
        rgb_bottom: widget.rgb_bottom.map(scale8),
        clut: if mode == 2 {
            DANCE_MODE2_CLUT
        } else {
            widget.clut
        },
        tpage_attr: widget.tpage + abr as u16 * 0x20,
    }
}

/// Which score slot each of the three on-screen score boxes shows, for a mode.
///
/// Wired: [`dance_hud_draws`] permutes its three score readouts through this,
/// and the play window's dance block draws all three boxes (the rivals'
/// scores included) from that list.
/// PORT: FUN_801d231c (`0x801D2320`..`0x801D23AC`) - the HUD driver's slot
/// permutation. The screen's three boxes are fixed; which dancer's score each
/// carries is chosen per mode so the **human** dancer always lands in the
/// centre one. Returns `(centre, left, right)` as indices into
/// `DAT_801D53CC`.
///
/// Retail's default arm (any mode outside `0..=3`) reads its third index out
/// of a register it never writes - an uninitialised read, visible in the
/// disassembly and rendered `unaff_s1` by Ghidra. The mode global is always
/// `0..=3`, so the arm is unreachable; the port returns `None` rather than
/// inventing a value for it.
pub fn dance_score_box_slots(mode: u32) -> Option<(usize, usize, usize)> {
    match mode {
        0 => Some((0, 1, 2)),
        1 => Some((1, 2, 0)),
        2 | 3 => Some((0, 2, 1)),
        _ => None,
    }
}

/// Screen x of the three score boxes' widget-8 frames (`FUN_801d231c`).
pub const DANCE_SCORE_BOX_X: [i16; 3] = [0xA0, 0x40, 0x100];
/// Digit-run origin x paired with each of [`DANCE_SCORE_BOX_X`].
pub const DANCE_SCORE_DIGIT_X: [i16; 3] = [0x40, -0x20, 0xA0];
/// Scanline the whole score strip sits on.
pub const DANCE_SCORE_Y: i16 = 0x14;
/// Widget id of a score-box frame.
pub const DANCE_SCORE_BOX_WIDGET: u32 = 8;
/// Brightness every element of the score strip draws at.
pub const DANCE_HUD_BRIGHTNESS: i32 = 0x80;
/// Groove-gauge readout position for the human dancer, `(x, y)`.
pub const DANCE_GAUGE_XY: (i16, i16) = (0x58, 0xC0);
/// Beat-track anchor for the human dancer, `(x, y)`.
pub const DANCE_TRACK_XY: (i16, i16) = (0x78, 0xC0);
/// Rival gauge / track positions, `(gauge_xy, track_xy)` per rival.
pub const DANCE_RIVAL_XY: [((i16, i16), (i16, i16)); 2] =
    [((0xDC, 0x40), (0xDC, 0xD4)), ((0x50, 0x40), (0x18, 0xD4))];

/// One element of the dance HUD's per-frame draw list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceHudDraw {
    /// A dancer's score, drawn as a run of digits from `x` (the emitter's
    /// leading-zero suppression is [`dance_number_digits`]).
    Score {
        slot: usize,
        x: i16,
        y: i16,
        value: u32,
    },
    /// A score-box frame (widget [`DANCE_SCORE_BOX_WIDGET`]).
    ScoreBox { x: i16, y: i16 },
    /// A groove-gauge `Lv.` readout for `slot`.
    Gauge {
        slot: usize,
        x: i16,
        y: i16,
        value: u32,
    },
    /// A beat track for `slot`.
    BeatTrack { slot: usize, x: i16, y: i16 },
}

// Wired: the free-function half of [`DanceGame::hud_draws`], reached through
// it from the play window's dance block every frame (see the note there for
// how the host stands in for `_DAT_8007B6D0`).
/// PORT: FUN_801d231c - the dance HUD render driver.
///
/// Per frame it draws the three score readouts and their box frames, then the
/// human dancer's groove gauge and beat track, then - **only while the rival
/// HUD flag `_DAT_8007B6D0` is set** - the two rivals' gauges and tracks. Mode
/// `3` (free play) is the single-dancer mode: it draws just the centre box and
/// its digits, skipping both side boxes.
///
/// `scores` and `gauges` are `DAT_801D53CC` / `DAT_801D544C`; `mode` is
/// `DAT_801D514C`.
pub fn dance_hud_draws(
    mode: u32,
    scores: [u32; 3],
    gauges: [u32; 3],
    rival_hud: bool,
) -> Vec<DanceHudDraw> {
    let mut out = Vec::with_capacity(12);
    let Some((centre, left, right)) = dance_score_box_slots(mode) else {
        return out;
    };
    let solo = mode == 3;
    let boxes: &[(usize, usize)] = if solo {
        &[(centre, 0)]
    } else {
        &[(centre, 0), (left, 1), (right, 2)]
    };
    for &(slot, pos) in boxes {
        out.push(DanceHudDraw::Score {
            slot,
            x: DANCE_SCORE_DIGIT_X[pos],
            y: DANCE_SCORE_Y,
            value: scores[slot],
        });
    }
    for &(_, pos) in boxes {
        out.push(DanceHudDraw::ScoreBox {
            x: DANCE_SCORE_BOX_X[pos],
            y: DANCE_SCORE_Y,
        });
    }
    out.push(DanceHudDraw::Gauge {
        slot: 0,
        x: DANCE_GAUGE_XY.0,
        y: DANCE_GAUGE_XY.1,
        value: gauges[0],
    });
    out.push(DanceHudDraw::BeatTrack {
        slot: 0,
        x: DANCE_TRACK_XY.0,
        y: DANCE_TRACK_XY.1,
    });
    if rival_hud {
        for (i, &(gauge_xy, track_xy)) in DANCE_RIVAL_XY.iter().enumerate() {
            out.push(DanceHudDraw::Gauge {
                slot: i + 1,
                x: gauge_xy.0,
                y: gauge_xy.1,
                value: gauges[i + 1],
            });
            out.push(DanceHudDraw::BeatTrack {
                slot: i + 1,
                x: track_xy.0,
                y: track_xy.1,
            });
        }
    }
    out
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
        assert_eq!(DanceDir::A.pad_bit(), 0x80);
        assert_eq!(DanceDir::B.pad_bit(), 0x20);
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

    #[test]
    fn step_mark_spawns_at_the_grid_cell() {
        let s = step_mark_effect_spawn(20, 15, 7);
        assert_eq!((s.x, s.y), (160, 120));
        assert_eq!((s.scale, s.sprite_id), (0x1000, 7));
    }

    #[test]
    fn score_glyph_u_steps_eight_texels_per_thousand() {
        assert_eq!(score_thousands_glyph_u(0), -0x30);
        assert_eq!(score_thousands_glyph_u(999), -0x30);
        assert_eq!(score_thousands_glyph_u(1000), -0x28);
        assert_eq!(score_thousands_glyph_u(6000), 0);
        assert_eq!(score_thousands_glyph_u(9999), 0x18);
    }

    // ---------------------------------------------------------- HUD kernels

    #[test]
    fn number_split_suppresses_leading_zeros_and_zero_draws_nothing() {
        // Right-aligned, leading blanks are None; the drawn digits are the value.
        assert_eq!(
            dance_number_digits(1234),
            [None, None, None, None, Some(1), Some(2), Some(3), Some(4)]
        );
        assert_eq!(
            dance_number_digits(50),
            [None, None, None, None, None, None, Some(5), Some(0)]
        );
        // The retail sentinel means a zero value draws no digit at all.
        assert_eq!(dance_number_digits(0), [None; 8]);
        // 10^7 - 1 fills the low seven slots (the eighth place is still zero).
        assert_eq!(
            dance_number_digits(9_999_999),
            [
                None,
                Some(9),
                Some(9),
                Some(9),
                Some(9),
                Some(9),
                Some(9),
                Some(9)
            ]
        );
    }

    #[test]
    fn digit_glyph_u_steps_match_the_two_widget_styles() {
        assert_eq!(dance_score_digit_u(0), 0x00);
        assert_eq!(dance_score_digit_u(9), 0x90);
        assert_eq!(dance_level_digit_u(0), 0x40);
        assert_eq!(dance_level_digit_u(9), 0x88);
    }

    #[test]
    fn beat_track_mask_widens_with_the_level() {
        assert_eq!(dance_beat_level_mask(0), 3);
        assert_eq!(dance_beat_level_mask(1), 7);
        assert_eq!(dance_beat_level_mask(2), 7);
    }

    #[test]
    fn combo_window_flashes_on_the_masked_slot_inside_the_window() {
        // Level 0 masks to 4: beat 3 on the beat flashes, past 0x46 does not.
        assert!(dance_combo_window_bright(3, 0, 0));
        assert!(dance_combo_window_bright(3, 0, 0x45));
        assert!(!dance_combo_window_bright(3, 0, 0x46));
        assert!(!dance_combo_window_bright(2, 0, 0));
        // Level 1 masks to 8: `beat & 7 == 3` flashes every 8th beat (3, 11, ...),
        // so beat 3 is a combo slot but beat 7 - a level-0 slot - no longer is.
        assert!(dance_combo_window_bright(3, 1, 0));
        assert!(dance_combo_window_bright(11, 1, 0));
        assert!(!dance_combo_window_bright(7, 1, 0));
        assert!(!dance_combo_window_bright(5, 1, 0));
    }

    #[test]
    fn beat_track_note_scrolls_one_cell_per_beat() {
        // On the beat (frac 0): note i sits at base + i*16 - 9.
        assert_eq!(dance_beat_track_note_x(120, 0, 0), 120 - 9);
        assert_eq!(dance_beat_track_note_x(120, 1, 0), 120 + 16 - 9);
        // Across a full beat the fraction subtracts a further ~16 texels: note 1
        // has scrolled almost onto note 0's on-beat slot.
        let edge = dance_beat_track_note_x(120, 1, BEAT_PERIOD - 1);
        assert!(edge < dance_beat_track_note_x(120, 1, 0));
        assert!(edge <= dance_beat_track_note_x(120, 0, 0) + 1);
    }

    #[test]
    fn hit_sting_keys_two_voices_per_random_pick() {
        for r in 0..STING_RANDOM_VARIANTS {
            let [a, b] = dance_hit_sting_voices(r);
            assert_eq!((a.voice, b.voice), (0x12, 0x13));
            assert_eq!((a.tone, b.tone), ((2 * r) as i16, (2 * r + 1) as i16));
            assert_eq!((a.note, b.note), (0x3c + r as i16, 0x3c + r as i16));
            // Both voices carry the two arguments the earlier port dropped:
            // `li a1,0x2` (level) and `li a2,0x1` (program). The program is
            // what makes the browser page's `tones[1]` bank lookup the right
            // one rather than a guess.
            assert_eq!((a.level, b.level), (STING_LEVEL, STING_LEVEL));
            assert_eq!((a.program, b.program), (STING_PROGRAM, STING_PROGRAM));
        }
    }

    /// The groovy-move tiers key a sting the random space never reaches, so
    /// the kernel has to answer for it too: `FUN_801d1af4` reaches
    /// `FUN_801d3d78` from four sites and three of them pass a literal `5`.
    #[test]
    fn the_tier_sting_is_outside_the_random_space() {
        let [a, b] = dance_hit_sting_voices(STING_TIER_VARIANT);
        assert_eq!((a.tone, b.tone), (0xa, 0xb));
        assert_eq!((a.note, b.note), (0x41, 0x41));
        assert_eq!((a.voice, b.voice), (0x12, 0x13));
        // Same primitive, same two dropped-then-restored arguments.
        assert_eq!((a.level, b.level), (STING_LEVEL, STING_LEVEL));
        assert_eq!((a.program, b.program), (STING_PROGRAM, STING_PROGRAM));
        // No random pick can produce it, which is why a `0..3` enumeration is
        // short one sting rather than merely unlucky.
        for r in 0..STING_RANDOM_VARIANTS {
            assert_ne!(dance_hit_sting_voices(r)[0].tone, a.tone);
        }
    }

    #[test]
    fn good_banner_places_banner_centre_and_stars_symmetric() {
        let s = good_banner_spawn(0x0abc);
        assert_eq!(s.weight, 0x0abc);
        assert_eq!(s.banner.sprite_id, 0xb);
        assert_eq!((s.stars[0].sprite_id, s.stars[1].sprite_id), (0x16, 0x16));
        // The two stars flank the banner symmetrically (0x38 either side, then
        // the shared <<3 spawn convention).
        let (bx, lx, rx) = (s.banner.x, s.stars[0].x, s.stars[1].x);
        assert_eq!(rx - bx, bx - lx);
        assert_eq!(bx, 0xa0 << 3);
    }

    #[test]
    fn face_rig_remaps_only_the_qualifier_cast() {
        // Qualifier: dancer -> kind (2 -> 3, 1 -> 2), so rig id == dancer kind.
        assert_eq!(dance_face_rig(DanceMode::Qualifier, 0), Some(0));
        assert_eq!(dance_face_rig(DanceMode::Qualifier, 1), Some(2));
        assert_eq!(dance_face_rig(DanceMode::Qualifier, 2), Some(3));
        // Other modes stamp the dancer index straight through.
        assert_eq!(dance_face_rig(DanceMode::Finals, 1), Some(1));
        assert_eq!(dance_face_rig(DanceMode::HowTo, 3), Some(3));
        // Dancers past the fourth are not stamped.
        assert_eq!(dance_face_rig(DanceMode::FreePlay, 4), None);
    }

    #[test]
    fn fade_weight_collapses_past_the_window_instead_of_saturating() {
        assert_eq!(sprite_part_fade_weight(0), 0);
        assert_eq!(sprite_part_fade_weight(0x10), 1);
        // 0x4000 >> 4 = 0x400, clamped to 0xff.
        assert_eq!(sprite_part_fade_weight(0x4000), 0xFF);
        // One past the window is zero, not 0xff.
        assert_eq!(sprite_part_fade_weight(0x4001), 0);
        assert_eq!(sprite_part_fade_weight(0xFFFF), 0);
    }

    #[test]
    fn dancer_emit_modes_differ_in_rounding_and_flags() {
        assert_eq!(sprite_part_emit(0, 0, 0, 0), SpritePartEmit::CopyTemplate);
        assert_eq!(sprite_part_emit(1, 0, 0, 0), SpritePartEmit::SetTemplateZ);
        assert_eq!(
            sprite_part_emit(2, 16, -1, 0x20),
            SpritePartEmit::Shadowed {
                x: 2,
                // -1 rounds toward zero before the shift.
                y: 0,
                flags: [0x420, 0x820],
            }
        );
        // Mode 3 does not scale at all.
        assert_eq!(
            sprite_part_emit(3, 16, -1, 0x20),
            SpritePartEmit::Plain {
                x: 16,
                y: -1,
                flags: 0x20
            }
        );
        assert_eq!(
            sprite_part_emit(4, 16, -1, 0x0A),
            SpritePartEmit::Marker {
                x: 16,
                y: -1,
                clut_byte: 0xA0
            }
        );
        assert_eq!(sprite_part_emit(5, 0, 0, 0), SpritePartEmit::None);
    }

    #[test]
    fn scene_stage_arms_after_the_setup_call() {
        let s = dance_scene_stage();
        assert_eq!(s.scene, "other1");
        assert!(s.clear_pad_latch);
        assert_eq!(s.arm_value, -1);
    }

    #[test]
    fn countin_banner_slides_holds_then_fades() {
        // Slide-in: two half-bright halves flying in from 0xb4 toward centre.
        let s0 = dance_countin_banner_envelope(0);
        assert!(!s0.hold);
        assert_eq!(s0.x_offset, 0xb4);
        assert_eq!(s0.brightness, 0x80 / 2);
        let s29 = dance_countin_banner_envelope(29);
        assert_eq!(s29.x_offset, 0xb4 - 6 * 29);
        assert!(!s29.hold);

        // Hold: single opaque centred banner, brightness ramps from 0x80 and
        // clamps at 0xff; the intro cue fires on entry (frame 0x1e).
        let h = dance_countin_banner_envelope(0x1e);
        assert!(h.hold);
        assert_eq!(h.x_offset, 0);
        assert_eq!(h.brightness, 0x80);
        assert_eq!(COUNTIN_INTRO_CUE, 0x200);
        // Deep into the hold the ramp saturates at full brightness.
        assert_eq!(dance_countin_banner_envelope(0x59).brightness, 0xff);

        // Slide-out: two halves again, flying back out as brightness fades.
        let o = dance_countin_banner_envelope(0x5a);
        assert!(!o.hold);
        assert_eq!(o.x_offset, 0);
        assert_eq!(o.brightness, 200 / 2);
        let o_late = dance_countin_banner_envelope(0x5a + 30);
        assert!(o_late.x_offset > o.x_offset);
        assert!(o_late.brightness < o.brightness);
    }

    #[test]
    fn clip_driver_gate_fires_on_spin_or_flag() {
        // A spinning dancer (groovy-move turns left) drives its clip.
        assert!(dance_clip_driver_gate(1, 0));
        // The 0x1000 flag bit alone drives it too.
        assert!(dance_clip_driver_gate(0, 0x1000));
        assert!(dance_clip_driver_gate(0, 0x1234));
        // Neither: idle, no clip drive.
        assert!(!dance_clip_driver_gate(0, 0));
        assert!(!dance_clip_driver_gate(-1, 0x2000));
    }

    fn probe_widget() -> legaia_asset::dance_art::DanceWidget {
        legaia_asset::dance_art::DanceWidget {
            scale: 0x1000,
            tpage: 0x0008,
            clut: 0x7D08,
            u: 0x10,
            v: 0x20,
            w: 0x20,
            h: 0x10,
            rgb_top: [0x80, 0x40, 0x20],
            rgb_bottom: [0x40, 0x20, 0x10],
            semi: 0,
        }
    }

    #[test]
    fn widget_quad_is_centred_and_half_open() {
        let w = probe_widget();
        let q = dance_hud_widget_quad(&w, 0, 100, 50, 0, 0x100, 0x1000);
        // 1:1 scale + 1:1 size means half-extent is exactly cell/2.
        assert_eq!((q.x0, q.x1), (100 - 0x10, 100 + 0x10));
        assert_eq!((q.y0, q.y1), (50 - 8, 50 + 8));
        // Half-open UVs: `u + w`, not `u + w - 1` like the Baka emitter's.
        assert_eq!(q.uv[0], (0x10, 0x20));
        assert_eq!(q.uv[3], (0x30, 0x30));
        assert_eq!(q.poly_code, 0x3C);
        assert_eq!(q.tpage_attr, 0x0008);
        // Brightness 0x100 passes the tints through unchanged.
        assert_eq!(q.rgb_top, w.rgb_top);
        assert_eq!(q.rgb_bottom, w.rgb_bottom);
        // Half brightness halves every channel.
        let dim = dance_hud_widget_quad(&w, 0, 0, 0, 0, 0x80, 0x1000);
        assert_eq!(dim.rgb_top, [0x40, 0x20, 0x10]);
    }

    #[test]
    fn the_widget_ids_upper_bits_override_the_records_blend() {
        let w = probe_widget();
        // Mode 0 takes the record's own semi bit and the caller's abr byte.
        let m0 = dance_hud_widget_quad(&w, 1, 0, 0, 8, 0x100, 0x1000);
        assert_eq!(m0.poly_code, 0x3C);
        assert_eq!(m0.tpage_attr, 0x0008 + 0x20);
        // Any other mode forces semi-transparency on and *is* the abr rate.
        let m1 = dance_hud_widget_quad(&w, 0, 0, 0, (1 << 10) | 8, 0x100, 0x1000);
        assert_eq!(m1.poly_code, 0x3E);
        assert_eq!(m1.tpage_attr, 0x0008 + 0x20);
        assert_eq!(m1.clut, w.clut);
        // Mode 2 additionally replaces the CLUT with the fixed override.
        let m2 = dance_hud_widget_quad(&w, 0, 0, 0, (2 << 10) | 8, 0x100, 0x1000);
        assert_eq!(m2.clut, DANCE_MODE2_CLUT);
        assert_eq!(m2.tpage_attr, 0x0008 + 0x40);
        // The index field is masked, so the mode bits never leak into it.
        assert_eq!(DANCE_WIDGET_ID_MASK & ((2 << 10) | 8), 8);
    }

    #[test]
    fn the_human_always_lands_in_the_centre_score_box() {
        // Whichever mode, slot 0 (the human) is in the centre box except in
        // the finals, where the mode global rotates the trio.
        assert_eq!(dance_score_box_slots(0), Some((0, 1, 2)));
        assert_eq!(dance_score_box_slots(1), Some((1, 2, 0)));
        assert_eq!(dance_score_box_slots(2), Some((0, 2, 1)));
        assert_eq!(dance_score_box_slots(3), Some((0, 2, 1)));
        // The unreachable default arm reads an unwritten register in retail;
        // the port refuses rather than inventing a slot.
        assert_eq!(dance_score_box_slots(4), None);
    }

    #[test]
    fn hud_driver_skips_the_side_boxes_in_free_play() {
        let scores = [111, 222, 333];
        let gauges = [1500, 500, 2500];
        let versus = dance_hud_draws(0, scores, gauges, false);
        assert_eq!(
            versus
                .iter()
                .filter(|d| matches!(d, DanceHudDraw::ScoreBox { .. }))
                .count(),
            3
        );
        let solo = dance_hud_draws(3, scores, gauges, false);
        assert_eq!(
            solo.iter()
                .filter(|d| matches!(d, DanceHudDraw::ScoreBox { .. }))
                .count(),
            1
        );
        // Only the centre box, and it carries slot 0's score.
        assert_eq!(
            solo[0],
            DanceHudDraw::Score {
                slot: 0,
                x: DANCE_SCORE_DIGIT_X[0],
                y: DANCE_SCORE_Y,
                value: 111
            }
        );
    }

    #[test]
    fn the_rival_hud_rows_are_gated_off_by_default() {
        let off = dance_hud_draws(0, [0; 3], [0; 3], false);
        let on = dance_hud_draws(0, [0; 3], [0; 3], true);
        let tracks = |v: &[DanceHudDraw]| {
            v.iter()
                .filter(|d| matches!(d, DanceHudDraw::BeatTrack { .. }))
                .count()
        };
        assert_eq!(tracks(&off), 1, "only the human's track without the flag");
        assert_eq!(tracks(&on), 3);
        // The rivals' rows sit at the traced off-centre positions.
        assert!(on.contains(&DanceHudDraw::BeatTrack {
            slot: 1,
            x: 0xDC,
            y: 0xD4
        }));
        assert!(on.contains(&DanceHudDraw::BeatTrack {
            slot: 2,
            x: 0x18,
            y: 0xD4
        }));
    }

    #[test]
    fn a_running_game_lays_its_own_hud_out() {
        let mut g = DanceGame::new(chart(), false);
        assert_eq!(g.mode(), DanceMode::Qualifier);
        let draws = g.hud_draws(false);
        assert!(draws.contains(&DanceHudDraw::Gauge {
            slot: 0,
            x: DANCE_GAUGE_XY.0,
            y: DANCE_GAUGE_XY.1,
            value: 0
        }));
        // With no overlay image behind it there is no widget table to resolve.
        assert!(g.hud_quads(false).is_empty());
        // The score readout tracks the run: land a step, then re-read the HUD.
        g.press(DanceDir::A);
        let scored = g.hud_draws(false);
        assert_eq!(
            scored[0],
            DanceHudDraw::Score {
                slot: 0,
                x: DANCE_SCORE_DIGIT_X[0],
                y: DANCE_SCORE_Y,
                value: g.score()
            }
        );
    }

    // ------------------------------------------------- dancer actor records

    #[test]
    fn a_run_spawns_one_actor_per_floor_slot() {
        let g = game();
        assert_eq!(g.dancer_actors().len(), g.dancer_count());
        // The pool is populated by the constructor, not by a test: every slot
        // already carries the retail spawn scale.
        for a in g.dancer_actors() {
            assert_eq!(a.scale, crate::minigame_actor::SPAWN_SCALE);
        }
        assert_eq!(g.dancer_clip_frames().len(), g.dancer_count());
        // Nothing enters the sprite-part pool until something scores.
        assert!(g.sprite_parts().is_empty());
    }

    #[test]
    fn the_groovy_spin_raises_the_clip_drive_flag() {
        let mut g = game();
        // No clip is bound on a chart-only run (the ids are overlay data), so
        // the gate is false until the flag arm fires.
        assert!(g.dancer_clip_frames().iter().all(|f| !f.clip_driver));
        // A triangle throws the human into the groovy move, which is the
        // `0x1000` arm: `FUN_801d4098` runs the clip driver regardless.
        let ev = g.press(DanceDir::C);
        assert!(matches!(ev, DanceEvent::Groovy { .. }), "{ev:?}");
        assert!(g.in_groovy_move());
        let f = g.dancer_clip_frames();
        assert!(f[0].clip_driver, "the spinning dancer must drive its clip");
        assert_eq!(
            g.dancer_actors()[0].flags & crate::minigame_actor::FLAG_DRIVE_CLIP,
            crate::minigame_actor::FLAG_DRIVE_CLIP
        );
        // The rivals are not spinning, so their gate stays down.
        assert!(f[1..].iter().all(|s| !s.clip_driver));
    }

    // ---------------------------------------------------------- sprite parts

    #[test]
    fn a_closed_chain_spawns_the_banner_parts_and_they_emit() {
        let mut g = game();
        assert!(g.sprite_parts().is_empty());
        // Lane 0 closes its chain on the first matched note.
        let ev = g.press(DanceDir::A);
        assert!(matches!(ev, DanceEvent::Sequence { .. }), "{ev:?}");
        // The banner + its two stars, spawned by the rules engine itself.
        assert_eq!(g.sprite_parts().len(), 3);
        let frames = g.sprite_part_emits();
        assert_eq!(frames.len(), 3);
        // The banner sits at screen centre: `0xa0 << 3` spawned, `>> 3` back
        // out again by the emit dispatch.
        match frames[0].emit {
            SpritePartEmit::Shadowed { x, y, flags } => {
                assert_eq!((x, y), (0xa0, 0x90));
                assert_eq!(flags, [0xb | 0x400, 0xb | 0x800]);
            }
            other => panic!("a part takes the shadowed arm, got {other:?}"),
        }
        // A fresh part spawns at the top of the ramp, so the prologue's `>> 4`
        // + clamp puts it at full weight; `advance` decays it from there. The
        // clamp is why the hold runs long and the fade is the tail: the weight
        // only leaves 0xFF once `+0x78` drops below `0xFF << 4`.
        assert!(frames.iter().all(|f| f.fade == 0xFF), "{frames:?}");
        let hold = (u32::from(crate::minigame_actor::BEAT_FADE_CEILING) - 0xFF0) / PART_AGE_STEP;
        g.advance(hold);
        assert!(g.sprite_part_emits().iter().all(|f| f.fade == 0xFF));
        g.advance(4);
        let mid = g.sprite_part_emits();
        assert!(
            mid.iter().all(|f| f.fade > 0 && f.fade < 0xFF),
            "the fade weight must track the part's age, got {mid:?}"
        );
    }

    #[test]
    fn sprite_parts_retire_when_the_fade_runs_out() {
        let mut g = game();
        g.press(DanceDir::A);
        assert_eq!(g.sprite_parts().len(), 3);
        let frames_to_zero = u32::from(crate::minigame_actor::BEAT_FADE_CEILING) / PART_AGE_STEP;
        for _ in 0..frames_to_zero {
            g.advance(1);
        }
        assert!(g.sprite_parts().is_empty(), "{:?}", g.sprite_parts());
    }

    #[test]
    fn the_fade_prologue_collapses_above_the_ceiling() {
        // The one arm the port's own driving never reaches, kept covered on
        // the kernel: retail compares `+0x78` against 0x4000 as a signed 32-bit
        // value, so a value past it goes to zero outright, not to a saturated
        // 0xFF.
        assert_eq!(sprite_part_fade_weight(0x4000), 0xFF);
        assert_eq!(sprite_part_fade_weight(0x4001), 0);
        assert_eq!(sprite_part_fade_weight(0xFFFF), 0);
        assert_eq!(sprite_part_fade_weight(0x100), 0x10);
        assert_eq!(sprite_part_fade_weight(0), 0);
    }

    #[test]
    fn the_emit_dispatch_rounds_a_negative_pair_toward_zero() {
        let mut g = game();
        g.press(DanceDir::A);
        // Park a part on a negative component so the round-toward-zero shift
        // is exercised through the live record.
        g.parts.actors_mut()[0].pos = [-1, 0x40, 0];
        match g.sprite_part_emits()[0].emit {
            SpritePartEmit::Shadowed { x, y, .. } => assert_eq!((x, y), (0, 8)),
            other => panic!("a part takes the shadowed arm, got {other:?}"),
        }
    }
}
