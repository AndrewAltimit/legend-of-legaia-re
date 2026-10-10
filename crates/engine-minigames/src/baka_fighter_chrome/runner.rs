//! The runner that drives the three timelines and the banner actor pool.

use super::*;

// ---------------------------------------------------------------------------
// The runner that drives the three timelines and the banner actor pool
// ---------------------------------------------------------------------------

/// Frame at which the intro title card has finished assembling and the
/// duel host stops ticking it. The card's last independent range test opens
/// at `140` and its ramps run 64 frames.
pub const INTRO_END: i32 = 204;

/// Frame at which the round banner's fly-out brightness has clamped to `0`
/// and the banner is done: `0xC8 - (t - 90) * 127 / 30 <= 0` first holds at
/// `t - 90 == 48`.
pub const BANNER_END: i32 = BANNER_SLIDE_OUT + 48;

/// Widget id the round banner spawns as a sprite actor
/// (`FUN_801D6E04`'s argument on the round-result path).
pub const ROUND_BANNER_SPRITE: u16 = 3;

/// One frame's worth of match state the runner needs from the duel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTick {
    /// The global frame step (`DAT_1F800393`).
    pub frame_step: i32,
    /// `_DAT_8007BC20` - the scene-load flag that freezes the countdown walk.
    pub loading: bool,
    /// `DAT_801DBF78` - `0` while the match is torn down, non-zero while it
    /// runs. The actor bind retires every pooled banner once it is set.
    pub match_phase: i32,
    /// `DAT_801DBF8C` - the 0-based round index the banner announces.
    pub round: i32,
    /// `DAT_801DC110` - the round counter the countdown tests for its final
    /// announcer line.
    pub round_counter: i32,
    /// `DAT_801DBF70` / `DAT_801DBF74` - the two focused fighter ids the bind
    /// compares each actor's owner against.
    pub focus: (i16, i16),
}

impl Default for ChromeTick {
    fn default() -> Self {
        ChromeTick {
            frame_step: 1,
            loading: false,
            match_phase: 0,
            round: 0,
            round_counter: 0,
            focus: (0, 1),
        }
    }
}

/// One live banner sprite actor in the runner's pool, plus the mode its draw
/// callback runs under and the owner id the bind compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeSprite {
    pub actor: ChromeActor,
    /// The `mode` argument `FUN_801D67F0` is installed with.
    pub mode: i32,
    /// `+0x5A` - the fighter this banner belongs to.
    pub owner: i16,
    /// `+0x5C` - the animation id the bind resolves.
    pub anim_id: i16,
    /// `+0x10` flag word.
    pub flags: u32,
}

/// The Baka Fighter round chrome as one advancing object.
///
/// Retail runs all three timelines off the overlay's own per-frame tick, in
/// parallel with the resolution state machine, and spawns the round-result
/// banners as sprite actors that the `_DAT_8007BA2C` draw hook services.
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
    banner_t: Option<i32>,
    countdown: Countdown,
    /// `DAT_801DBEB4` / `DAT_801DBEB0` - the two banner brightness globals
    /// the countdown gates on and draws with.
    banner_level: i32,
    title_level: i32,
    sprites: Vec<ChromeSprite>,
    /// The live round-result banner sprites ([`Self::raise_result`]).
    results: Vec<ChromeActor>,
    /// The result's announcer line, waiting for a frame whose XA slot is free.
    result_xa: Option<XaCue>,
    /// Frames the result banners still stand ([`RESULT_HOLD_FRAMES`]).
    result_hold: i32,
    /// A round banner asked for while the result banners stand: retail draws
    /// it from the next round's setup, after the hold, so it waits.
    pending_round_banner: Option<u16>,
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

    /// Start the round banner timeline and spawn its sprite actor at the
    /// screen centre through the shared spawn wrapper.
    pub fn start_round_banner(&mut self, sprite_id: u16) {
        if !self.results.is_empty() {
            self.pending_round_banner = Some(sprite_id);
            return;
        }
        self.banner_t = Some(0);
        self.countdown = Countdown::default();
        let spec = crate::baka_fighter::center_effect_spawn(sprite_id);
        self.sprites.push(ChromeSprite {
            actor: ChromeActor {
                x: spec.x,
                y: spec.y,
                id: spec.sprite_id,
                size: spec.scale as u16,
                fade: 0x800,
            },
            mode: 1,
            owner: -1,
            anim_id: spec.sprite_id as i16,
            flags: chrome_actor_hold(0),
        });
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
                // No owner matches either focused fighter, so the bind
                // seats the neutral level (`0x800` -> brightness `0x80`).
                fade: 0x800,
            })
            .collect();
        self.result_xa = Some(result_xa(r));
        self.result_hold = RESULT_HOLD_FRAMES;
        // A round banner already started for this round end (the duel books
        // the round before the HP the banner tail reads settles) waits too:
        // retail draws it from the next round's setup, after the hold.
        if self.banner_t.take().is_some() {
            self.countdown = Countdown::default();
            self.pending_round_banner = Some(ROUND_BANNER_SPRITE);
        }
        // The previous round's ROUND sprite actor leaves with it: the result
        // takes the screen centre the round banner stood at.
        self.sprites.retain(|s| s.actor.id != ROUND_BANNER_SPRITE);
    }

    /// Take the round-result banners down.
    pub fn clear_result(&mut self) {
        self.results.clear();
        self.result_xa = None;
        self.result_hold = 0;
        self.pending_round_banner = None;
    }

    /// The live round-result banner sprites.
    pub fn result_sprites(&self) -> &[ChromeActor] {
        &self.results
    }

    /// `true` while any timeline is still running.
    pub fn busy(&self) -> bool {
        self.intro_t.is_some() || self.intro_live.is_some() || self.banner_t.is_some()
    }

    /// The live banner sprite pool.
    pub fn sprites(&self) -> &[ChromeSprite] {
        &self.sprites
    }

    /// The countdown's own state, for a host that wants to show it.
    pub fn countdown(&self) -> Countdown {
        self.countdown
    }

    /// Advance every armed timeline one frame and service the sprite pool.
    ///
    /// `banks` are the two runtime clip banks the bind resolves frame
    /// counts out of (`_DAT_8007B888` below [`ANIM_BANK_SPLIT`],
    /// `_DAT_8007B840` at or above). A host with neither staged passes empty
    /// slices, in which case the resolved frame count is `None` and every
    /// other bind output still applies.
    pub fn step(&mut self, tick: &ChromeTick, banks: (&[u8], &[u8])) -> ChromeFrame {
        let mut out = ChromeFrame::default();

        if let Some(t) = self.intro_live.take() {
            let f = self.intro.frame(t);
            merge_frame(&mut out, f);
        } else if let Some(t) = self.intro_t {
            let f = self.intro.frame(t);
            merge_frame(&mut out, f);
            self.intro_t = if t + 1 < INTRO_END { Some(t + 1) } else { None };
        }

        if let Some(t) = self.banner_t {
            let f = round_banner_frame(t, tick.round);
            self.banner_flags = round_banner_flags(t);
            merge_frame(&mut out, f);
            // The banner brightness the countdown gates on is the same ramp
            // the banner draws with, so it crosses COUNTDOWN_FADE_GATE a
            // couple of frames into the slide-in.
            self.banner_level = t.min(0xFF);
            self.title_level = self.banner_level;
            let cd = countdown_frame(
                &mut self.countdown,
                tick.frame_step,
                tick.loading,
                self.banner_level,
                self.title_level,
                tick.round_counter,
            );
            merge_frame(&mut out, cd);
            self.banner_t = if t + 1 < BANNER_END {
                Some(t + 1)
            } else {
                None
            };
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
                // The hold is over: the round setup's banner takes the screen.
                self.results.clear();
                self.result_hold = 0;
                if let Some(id) = self.pending_round_banner.take() {
                    self.start_round_banner(id);
                }
            }
        }

        let mut retired = Vec::new();
        for (i, s) in self.sprites.iter_mut().enumerate() {
            let bind = chrome_actor_bind(
                s.flags,
                tick.match_phase,
                s.owner,
                tick.focus,
                s.anim_id,
                banks.0,
                banks.1,
            );
            s.flags = bind.flags;
            s.actor.fade = bind.fade;
            if bind.flags & ACTOR_FLAG_RETIRE != 0 {
                retired.push(i);
                continue;
            }
            let d = chrome_actor_draw(&s.actor, s.mode, tick.match_phase);
            if let Some(draw) = d.draw {
                out.draws.push(draw);
            }
            if d.retire {
                s.flags |= ACTOR_FLAG_RETIRE;
                retired.push(i);
            }
        }
        for i in retired.into_iter().rev() {
            self.sprites.remove(i);
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
