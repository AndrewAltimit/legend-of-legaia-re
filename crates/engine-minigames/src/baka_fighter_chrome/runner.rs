//! The runner that drives the round timelines and the result banners.

use super::*;

// ---------------------------------------------------------------------------
// The runner that drives the round timelines and the result banners
// ---------------------------------------------------------------------------

/// Frame at which the intro title card has finished assembling and the
/// duel host stops ticking it. The card's last independent range test opens
/// at `140` and its ramps run 64 frames.
pub const INTRO_END: i32 = 204;

/// Frame at which the round banner's fly-out brightness has clamped to `0`
/// and the banner is done: `0xC8 - (t - 90) * 127 / 30 <= 0` first holds at
/// `t - 90 == 48`.
pub const BANNER_END: i32 = BANNER_SLIDE_OUT + 48;

/// One frame's worth of match state the runner needs from the duel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTick {
    /// The global frame step (`DAT_1F800393`).
    pub frame_step: i32,
    /// `DAT_801DBF78`, the argument the result sprites' draw reads.
    pub match_phase: i32,
}

impl Default for ChromeTick {
    fn default() -> Self {
        ChromeTick {
            frame_step: 1,
            match_phase: 0,
        }
    }
}

/// The Baka Fighter round chrome as one advancing object.
///
/// Retail runs both timelines off the overlay's own per-frame tick, in
/// parallel with the resolution state machine, and spawns the round-result
/// banners as sprite actors that the `_DAT_8007BA2C` draw hook services. The
/// round banner is not one of them: `FUN_801D5C7C` draws its quads on the
/// frames the cabinet calls it and spawns nothing, so when the banner arms
/// end, the word is gone.
/// [`BakaChrome`] is that arrangement: the duel
/// ([`crate::baka_fighter::BakaFight`]) steps it once per frame and reads the
/// [`ChromeFrame`] it produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakaChrome {
    intro: IntroTitle,
    intro_t: Option<i32>,
    /// The cabinet-driven card clock for the next step
    /// ([`Self::set_intro_clock`]), consumed by it.
    intro_live: Option<i32>,
    /// The round banner clock the cabinet drew this frame
    /// ([`Self::round_banner_step`]); cleared at the top of every
    /// [`Self::step`].
    banner_live: Option<i32>,
    /// The live round-result banner sprites ([`Self::raise_result`]).
    results: Vec<ChromeActor>,
    /// The result's announcer line, waiting for a frame whose XA slot is free.
    result_xa: Option<XaCue>,
    /// Frames the result banners still stand ([`RESULT_HOLD_FRAMES`]).
    result_hold: i32,
    /// Whether the banner's two sprite-actor visibility flags are up.
    banner_flags: bool,
    /// The last round whose banner line a prestage list named
    /// ([`Self::take_xa_prestage`]); `None` before the first list.
    xa_prestaged_through: Option<i32>,
}

impl BakaChrome {
    /// The announcer lines to stage ahead of use at round `round`: the whole
    /// [`announcer_xa_prestage`] list on the first call, then the next
    /// round's banner line each time `round` advances, and nothing in
    /// between. A host that reads a clip's span synchronously (the native
    /// window) drains and drops it.
    pub fn take_xa_prestage(&mut self, round: i32) -> Vec<XaCue> {
        match self.xa_prestaged_through {
            None => {
                self.xa_prestaged_through = Some(round + 1);
                announcer_xa_prestage(round)
            }
            Some(through) if round + 1 > through => {
                self.xa_prestaged_through = Some(round + 1);
                (through + 1..=round + 1).map(round_banner_xa).collect()
            }
            Some(_) => Vec::new(),
        }
    }

    /// A runner with the intro title card armed (the cabinet's attract
    /// sequence) rather than starting mid-duel.
    pub fn with_intro() -> Self {
        BakaChrome {
            intro_t: Some(0),
            ..BakaChrome::default()
        }
    }

    /// Drive the intro title card off an **externally owned clock** - the
    /// cabinet's `DAT_801DBE94`.
    ///
    /// The card is not the duel's: its two `jal` sites in PROT 0976
    /// (`0x801CF784`, `0x801CF840`) are inside the cabinet state machine's
    /// attract arms, and those are the arms that advance `DAT_801DBE94`. So
    /// the honest wiring runs the card while the cabinet occupies an attract
    /// state, with the cabinet's own counter as `t`, rather than arming a
    /// private timeline at duel start.
    ///
    /// `Some(t)` draws the card at `t` on the next [`Self::step`]; `None`
    /// (the cabinet called no attract arm) draws none. There is no upper
    /// bound: past [`INTRO_END`] the card's last segment keeps drawing the
    /// assembled card, which is what a parked attract screen shows (the
    /// `minigame_baka_fighter` state sits at `DAT_801DBE94 = 373` with the
    /// whole card up).
    pub fn set_intro_clock(&mut self, t: Option<i32>) {
        self.intro_t = None;
        self.intro_live = t;
    }

    /// One frame of the round banner (`FUN_801D5C7C`), on the cabinet's
    /// clock: the four banner arms `0x33..0x36` call it with `DAT_801DC128`
    /// (zeroed by the round setup `0x32`, stepped after each call) and the
    /// round index `DAT_801DBF8C` the setup has already advanced
    /// (`0x801D0640`), so the line `round_banner_xa(round)` starts on the
    /// first banner frame of the round it names. Clock `0` also takes the
    /// result banners down.
    pub fn round_banner_step(&mut self, t: i32, round: i32) -> ChromeFrame {
        if t == 0 {
            self.results.clear();
            self.result_hold = 0;
        }
        self.banner_live = Some(t);
        self.banner_flags = round_banner_flags(t);
        round_banner_frame(t, round)
    }

    /// Raise the round-result banners for `r`: the sprites
    /// [`result_banner_spawns`] places and the announcer line
    /// [`result_xa`] starts. Retail raises them once per round off the
    /// banner sub-state `DAT_801DBF84`; the duel holds that latch.
    pub fn raise_result(&mut self, r: RoundResult) {
        self.results = result_banner_spawns(r)
            .into_iter()
            .map(|(id, x, y)| ChromeActor {
                x,
                y,
                id,
                size: 0x1000,
                // The neutral level (`0x800` -> brightness `0x80`).
                fade: 0x800,
            })
            .collect();
        self.result_xa = Some(result_xa(r));
        self.result_hold = RESULT_HOLD_FRAMES;
    }

    /// Take the round-result banners down.
    pub fn clear_result(&mut self) {
        self.results.clear();
        self.result_xa = None;
        self.result_hold = 0;
    }

    /// The live round-result banner sprites.
    pub fn result_sprites(&self) -> &[ChromeActor] {
        &self.results
    }

    /// `true` while any timeline is still running.
    pub fn busy(&self) -> bool {
        self.intro_t.is_some() || self.intro_live.is_some() || self.banner_live.is_some()
    }

    /// Advance every armed timeline one frame and draw the result banners.
    pub fn step(&mut self, tick: &ChromeTick) -> ChromeFrame {
        let mut out = ChromeFrame::default();
        self.banner_live = None;

        if let Some(t) = self.intro_live.take() {
            let f = self.intro.frame(t);
            merge_frame(&mut out, f);
        } else if let Some(t) = self.intro_t {
            let f = self.intro.frame(t);
            merge_frame(&mut out, f);
            self.intro_t = if t + 1 < INTRO_END { Some(t + 1) } else { None };
        }

        for a in &self.results {
            if let Some(draw) = chrome_actor_draw(a, 0, tick.match_phase).draw {
                out.draws.push(draw);
            }
        }
        if out.xa.is_none() {
            out.xa = self.result_xa.take();
        }
        if !self.results.is_empty() {
            self.result_hold -= tick.frame_step;
            if self.result_hold <= 0 {
                self.results.clear();
                self.result_hold = 0;
            }
        }
        out
    }
}

/// Fold one timeline's frame into the accumulating frame: draws append, and
/// the three single-slot channels take the first value produced this frame -
/// retail's later writes land on the same globals, and the earlier timeline
/// is the one that owns them while it runs.
fn merge_frame(out: &mut ChromeFrame, f: ChromeFrame) {
    out.draws.extend(f.draws);
    out.xa = out.xa.or(f.xa);
    out.tint = out.tint.or(f.tint);
    out.banner_clut = out.banner_clut.or(f.banner_clut);
}
