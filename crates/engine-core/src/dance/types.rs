//! Floor modes, step directions, judgements, events and the per-dancer run state.
//! Split out of `dance.rs`.

use super::*;

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

/// Story flag state 1 maps to [`DanceMode::HowTo`] (`li a0,0x133` at
/// `0x801CF8F8`).
pub const MODE_FLAG_HOW_TO: u16 = 0x133;
/// Story flag state 1 maps to [`DanceMode::Qualifier`] (`0x801CF910`).
pub const MODE_FLAG_QUALIFIER: u16 = 0x134;
/// Story flag state 1 maps to [`DanceMode::Finals`] (`0x801CF924`).
pub const MODE_FLAG_FINALS: u16 = 0x135;
/// Story flag state 1 maps to [`DanceMode::FreePlay`] (`0x801CF93C`). Unlike
/// the other three it is **not** cleared on entry: it is the standing "the
/// hall is yours to play" state, not a one-shot request.
pub const MODE_FLAG_FREE_PLAY: u16 = 0x428;
/// The run's pass flag: set on entry (`0x801CF968`), cleared by the results
/// state on a loss (`0x801CFF10`), read by the hall's field script.
pub const WIN_FLAG: u16 = 0x50A;

/// Which floor the dance overlay's state 1 opens, off the story flags the
/// calling field script raised.
///
/// State 1 tests the four flags in a fixed order and each hit overwrites the
/// mode global (`0x801CF8F4..0x801CF94C`): `0x133` -> how-to, `0x134` ->
/// qualifier, `0x135` -> finals, `0x428` -> free play - so a later flag in
/// that order wins. With none set the global keeps the `0` the overlay's
/// entry `FUN_801CEF54` stored at `0x801CF398`: the qualifier.
// PORT: FUN_801cf470 (state 1's flag -> mode map)
pub fn dance_mode_from_flags(test: impl Fn(u16) -> bool) -> DanceMode {
    let mut mode = DanceMode::Qualifier;
    for (flag, m) in [
        (MODE_FLAG_HOW_TO, DanceMode::HowTo),
        (MODE_FLAG_QUALIFIER, DanceMode::Qualifier),
        (MODE_FLAG_FINALS, DanceMode::Finals),
        (MODE_FLAG_FREE_PLAY, DanceMode::FreePlay),
    ] {
        if test(flag) {
            mode = m;
        }
    }
    mode
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
pub(super) struct Dancer {
    /// Dancer kind (`DAT_801d540c`): the row both scoring tables are indexed by.
    /// `0` = Noa (the human).
    pub(super) kind: usize,
    /// The dancer actor's spawn position, straight out of the mode's spawn
    /// table (`FUN_801d0190` stores the record's three words into the actor's
    /// `+0x14` / `+0x16` / `+0x18`). Zero on a chart-only run.
    pub(super) home: [i16; 3],
    /// The actor's bound clip id (`+0x5C`), masked to `0x1FF` exactly as
    /// `FUN_801d0190` and `FUN_801d1358` write it. `0` = nothing bound.
    pub(super) clip: i16,
    /// The bound clip's cursor step (`+0x6A`), the kind descriptor's rate word.
    pub(super) clip_rate: u16,
    /// The body's display track ([`super::DanceBodyClip`]): the standing loop
    /// the dancer returns to ...
    pub(super) show_loop: super::DanceBodyClip,
    /// ... the judge-returned move playing over it, if one was bound ...
    pub(super) show_move: Option<super::DanceBodyClip>,
    /// ... and the ticks since the last restart of that track (the clip
    /// driver's cursor `+0x68` before the per-record step is applied).
    pub(super) show_ticks: u32,
    /// Ticks the bound judge move still plays before the clip driver raises
    /// its end flag (`0` = the standing loop is bound). Only counted when the
    /// run has its clip lengths ([`super::DanceGame::attach_clip_bank`]).
    pub(super) move_left: u32,
    /// The actor flag word (`+0x10`). Only the bits the dance overlay writes
    /// are modelled: [`crate::minigame_actor::FLAG_TRANSLUCENT`] (the anim
    /// word's `0x200`) and [`crate::minigame_actor::FLAG_DRIVE_CLIP`].
    pub(super) flags: u32,
    /// Score (`DAT_801d53cc`), clamped to [`SCORE_MAX`].
    pub(super) score: u32,
    /// Groove gauge (`DAT_801d544c`), clamped to `[0, GAUGE_MAX]`. Retail never
    /// lowers it on a miss - the Disco King's own tutorial says the level "rises
    /// automatically".
    pub(super) gauge: u32,
    /// Direction-chain cursor (`DAT_801d550c`); closing `lane + 1` matched notes
    /// is a sequence. Cleared every [`CURSOR_RESET_BEATS`] beats.
    pub(super) cursor: u32,
    /// Triangles left (`DAT_801d534c`).
    pub(super) triangles: u32,
    /// Triangle-schedule cursor (`DAT_801d574c`) - CPU dancers only.
    pub(super) tri_cursor: usize,
    /// Combo slots banked since the last triangle (`DAT_801d578c`) - CPU only.
    pub(super) tri_meter: i32,
    /// Hit-tier latch (`DAT_801d548c`): non-zero = this dancer's presses are not
    /// judged.
    pub(super) latch: u32,
    /// Latch countdown (`DAT_801d54cc`).
    pub(super) latch_timer: i32,
    /// Groovy-move spin turns left (`DAT_801d564c`).
    pub(super) spin_turns: u32,
    /// Spin accumulator (the dancer's yaw, `actor+0x26`).
    pub(super) spin_acc: u32,
    /// Miss counter (`DAT_801d568c`; drives the sad-face pose).
    pub(super) misses: u32,
    /// The last triangle landed on the combo slot (`DAT_801d570c`).
    pub(super) landed: bool,
    /// Beat index of the last judged press. Retail's actor handler stops calling
    /// the award routine while the reaction / move clip plays, which is always
    /// long enough to cover the rest of the beat's window; this is that gate in
    /// rules terms (one registered press per beat per dancer).
    pub(super) last_beat: Option<u32>,
    /// Last beat whose combo slot was banked into `tri_meter` (retail's
    /// `DAT_801d57cc` edge flag).
    pub(super) last_meter_beat: Option<u32>,
    /// Last beat on which the chain cursor was cleared.
    pub(super) last_reset_beat: Option<u32>,
}

impl Dancer {
    pub(super) fn new(kind: usize) -> Self {
        Self {
            kind,
            triangles: TRIANGLE_STOCK,
            ..Default::default()
        }
    }

    /// Bind a clip into the actor's `+0x5C` / `+0x6A` pair, folding the anim
    /// word's `0x200` bit into the flag word the way `FUN_801d1358` does.
    pub(super) fn bind_clip(&mut self, clip: &legaia_asset::dance_cast::DanceClip) {
        let id = (clip.anim_id & 0x1FF) as i16;
        let show = super::DanceBodyClip::of(clip);
        // A loop that changes while no move plays restarts; the rebind the
        // rules run every frame does not.
        if self.show_move.is_none() && show.id != self.show_loop.id {
            self.show_ticks = 0;
        }
        self.show_loop = show;
        self.clip = id;
        self.clip_rate = clip.rate;
        if clip.translucent {
            self.flags |= crate::minigame_actor::FLAG_TRANSLUCENT;
        } else {
            self.flags &= !crate::minigame_actor::FLAG_TRANSLUCENT;
        }
    }

    /// Bind a judge-returned move clip: always from its first frame, even when
    /// the same move is still playing (a fresh judge event restarts it).
    pub(super) fn bind_move(&mut self, clip: &legaia_asset::dance_cast::DanceClip) {
        let standing = self.show_loop;
        self.bind_clip(clip);
        self.show_loop = standing;
        self.show_move = Some(super::DanceBodyClip::of(clip));
        self.show_ticks = 0;
    }

    /// The dancer's difficulty lane (`gauge / 1000`), clamped to the chart.
    pub(super) fn lane(&self, rows: usize) -> u32 {
        (self.gauge / GAUGE_STEP).min(rows.saturating_sub(1) as u32)
    }

    /// Nothing is judged for this dancer right now: a judge move still
    /// playing (retail calls the award routine only while the bound clip is a
    /// standing loop), mid-spin, latched, or a press already registered on
    /// this beat.
    pub(super) fn locked(&self, beat: u32) -> bool {
        self.move_left > 0 || self.spin_turns > 0 || self.latch != 0 || self.last_beat == Some(beat)
    }
}
