//! The Muscle Dome hub's **ringside still** - which of the two stills a
//! finished leg leaves resident, and the backdrop level `*(0x801D1A7C)` a
//! re-entered hub draws it at.
//!
//! ## Which still
//!
//! Every special battle ends through the `field_back_read` loader
//! (`FUN_801F6B24`, PROT `0978`), which picks raw TOC `0x4C7 + s0` with
//! `s0 = hp_curr_live < hp_max_record / 2` over party slot 0's record, and
//! uploads that still to VRAM `(384, 0)`. Raw `0x4C7` / `0x4C8` are
//! extraction `1221` / `1222` (`int.tim`, `int2.tim`), so the below-half-HP
//! still is the second one. [`still_prot_index`] is that pick over the
//! engine's typed values; `World::exit_muscle_dome` - the port's battle end
//! for a dome leg, reached by both hosts' dome pad path - runs it and leaves
//! the answer on `MinigameState::muscle_ringside_still`.
//!
//! ## When it is drawn
//!
//! The contest hub `FUN_801CF870` (PROT `0977`) calls the backdrop emitter
//! `FUN_801D00F8` with `*(0x801D1A7C)` on every frame that word is non-zero,
//! and the emitter draws the still only when the re-entry latch
//! `_DAT_801D1AE0` is set - which the arena init `FUN_801CEA6C` does exactly
//! when the arena word is already non-zero, i.e. when a finished leg returns
//! to a hub that has run once. On that path the init seeds the hub's arm to
//! `0x0A` (`0x801CEE2C`) and zeroes the level (`0x801CECD0`), and the level
//! moves through six arms:
//!
//! | arm | level `*(0x801D1A7C)` | ends when |
//! |---|---|---|
//! | `0x0A` | `+= 4 dt`, clamp `0x80` (`0x801CFCF8`) | the INTERVAL heading reaches full |
//! | `0x0B` | `-= 2 dt`, floor `0x40`, only while tally lane 0 is at its clamp (`0x801CFD84..0x801CFDB8`) | the tally stops rolling **and** the level is `0x40` |
//! | `0x0C` | held | the heading has drained |
//! | `0x14` | `+= 4 dt`, clamp `0x80` (`0x801CFEF4..0x801CFF34`) | the level is full |
//! | `0x15` | held | the ROUND card's fade-in + hold (skippable) |
//! | `0x16` | `-= 2 dt`, clamp `0` (`0x801D002C..0x801D0040`) | the card has drained |
//!
//! [`HubBackdrop`] carries that envelope. The INTERVAL arms (`0x0A..0x0C`)
//! are the host's [`HubScreen::interval`], so the backdrop takes that
//! screen's stage as an input rather than running a second clock; the
//! return and card arms (`0x14..0x16`) are its own, with the card's envelope
//! [`HubScreen::opponent_card`] - the same arms `0x15` / `0x16` that kernel
//! was measured from, whose draw is `FUN_801D02F0`, the ROUND banner.
//!
//! The other half of the fork is the first visit (latch zero), where the same
//! emitter tiles a brick wall under the intro, title and course cards;
//! [`FirstVisitHub`] runs that visit's arms, the wall's level among them.

use crate::muscle_dome::{
    HUB_BACKDROP_HALF, HUB_FADE_FULL, HUB_FADE_STEP_FAST, HUB_FADE_STEP_SLOW, HUB_INTRO_HOLD_TICKS,
    HUB_OPPONENT_CARD_HOLD_TICKS, HUB_ROUND_BANNER_HOLD_TICKS, HUB_SKIP_PAD_MASK, HubScreen,
    HubScreenStage,
};
use legaia_engine_vm::panel_backread_loader::backread_texture_variant;

/// Which still a battle end leaves resident, as an extraction PROT index:
/// `1221` (`int.tim`) at or above half HP, `1222` (`int2.tim`) below.
///
/// `hp_cur_live` / `hp_max_record` are party slot 0's record `+0x106` /
/// `+0x11C`; the comparison is the loader's own unsigned `sltu` against a
/// logical halving.
///
/// REF: FUN_801f6b24 (`0x801F6B8C..0x801F6BAC` + the delay-slot `addiu
/// a0,s0,0x4c7` at `0x801F6C3C`; the compare is ported at
/// `backread_texture_variant`)
pub fn still_prot_index(hp_cur_live: u16, hp_max_record: u16) -> u32 {
    legaia_asset::ringside_still::PROT_INDEX_DEFAULT
        + backread_texture_variant(hp_cur_live, hp_max_record)
}

/// Where a [`HubBackdrop`] is in the re-entered hub's arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropStage {
    /// Arms `0x0A..0x0C` - riding the host's INTERVAL screen.
    Interval,
    /// Arm `0x14` - climbing back to full once the heading has drained.
    Return,
    /// Arms `0x15` / `0x16` - the ROUND card over the still.
    Card,
    /// Past arm `0x16`: the hub hands the next leg its fight.
    Done,
}

/// The re-entered hub's backdrop: the still it shows and the level
/// `*(0x801D1A7C)` it shows it at.
///
/// PORT: FUN_801cf870 (the `0x801D1A7C` writes of arms `0x0A`, `0x0B`,
/// `0x14` and `0x16` - `0x801CFCF8`, `0x801CFD9C`, `0x801CFF00`,
/// `0x801D002C`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubBackdrop {
    still: u32,
    level: i32,
    stage: BackdropStage,
    card: HubScreen,
}

impl HubBackdrop {
    /// A hub re-entered after a leg, showing extraction entry `still`
    /// ([`still_prot_index`]). The level starts at zero - the init's
    /// `sw zero,0x1a7c` at `0x801CECD0`.
    pub fn reentry(still: u32) -> Self {
        Self {
            still,
            level: 0,
            stage: BackdropStage::Interval,
            card: HubScreen::opponent_card(),
        }
    }

    /// Advance one hub tick.
    ///
    /// `interval` is the host's INTERVAL screen's stage, `None` once the host
    /// has dropped it; `lane0_full` is whether the score tally's first lane
    /// has reached its fade clamp (`DAT_801D1ABC == 0x10`, the arm-`0x0B`
    /// gate). `dt` / `pad` are the hub's frame-skip factor and edge word, as
    /// [`HubScreen::tick`] takes them.
    ///
    /// The port's INTERVAL screen ends its hold on a roll length rather than
    /// on retail's "tally stopped and level at half" test, so the dim keeps
    /// running through the screen's fade-out: either way the heading drains
    /// over a still held at [`HUB_BACKDROP_HALF`].
    pub fn tick(&mut self, dt: u8, pad: u16, interval: Option<HubScreenStage>, lane0_full: bool) {
        let step = dt.max(1) as i32;
        match self.stage {
            BackdropStage::Interval => match interval {
                Some(HubScreenStage::FadeIn) => {
                    self.level = (self.level + step * HUB_FADE_STEP_FAST).min(HUB_FADE_FULL);
                }
                Some(HubScreenStage::Hold) if lane0_full => self.dim_to_half(step),
                Some(HubScreenStage::Hold) => {}
                Some(HubScreenStage::FadeOut) => self.dim_to_half(step),
                Some(HubScreenStage::Done) | None => self.stage = BackdropStage::Return,
            },
            BackdropStage::Return => {
                self.level += step * HUB_FADE_STEP_FAST;
                if self.level >= HUB_FADE_FULL {
                    self.level = HUB_FADE_FULL;
                    self.stage = BackdropStage::Card;
                }
            }
            BackdropStage::Card => {
                let draining = self.card.stage() == HubScreenStage::FadeOut;
                self.card.tick(dt, pad);
                if draining {
                    self.level = (self.level - step * HUB_FADE_STEP_SLOW).max(0);
                }
                if self.card.done() {
                    self.level = 0;
                    self.stage = BackdropStage::Done;
                }
            }
            BackdropStage::Done => {}
        }
    }

    fn dim_to_half(&mut self, step: i32) {
        self.level = (self.level - step * HUB_FADE_STEP_SLOW).max(HUB_BACKDROP_HALF);
    }

    /// Extraction PROT index of the still this hub shows.
    pub fn still(&self) -> u32 {
        self.still
    }

    /// `0` (`int.tim`) or `1` (`int2.tim`) - the loader's `s0`.
    pub fn variant(&self) -> u32 {
        self.still
            .saturating_sub(legaia_asset::ringside_still::PROT_INDEX_DEFAULT)
    }

    /// The backdrop level `*(0x801D1A7C)`, the emitter's fade argument.
    pub fn level(&self) -> i32 {
        self.level
    }

    /// Which arm the backdrop is in.
    pub fn stage(&self) -> BackdropStage {
        self.stage
    }

    /// Whether the emitter runs this frame - retail skips the call at a zero
    /// level (`beqz a0` at `0x801D00A4`).
    pub fn visible(&self) -> bool {
        self.stage != BackdropStage::Done && self.level > 0
    }

    /// The ROUND card's brightness while arms `0x15` / `0x16` draw it over
    /// the still (`FUN_801D02F0` at `*(0x801D1A84)`), else `None`.
    pub fn card_brightness(&self) -> Option<i32> {
        (self.stage == BackdropStage::Card).then(|| self.card.brightness())
    }

    /// Whether the hub has handed the next leg its fight.
    pub fn done(&self) -> bool {
        self.stage == BackdropStage::Done
    }
}

/// Title-art zoom step per tick: `dt << 7` (`sll v1,v1,7` at `0x801CFAA0`).
pub const TITLE_ZOOM_STEP: i32 = 0x80;

/// Title-art scale the first visit's arm `2` seeds (`li v0,0x1640` at
/// `0x801CFA58`) and arm `3` zooms down from.
pub const TITLE_ZOOM_START: i32 = 0x1640;

/// Title-art scale the zoom clamps to - `1.0` in 12.12.
pub const TITLE_ZOOM_END: i32 = 0x1000;

/// Which arm of `FUN_801CF870` a [`FirstVisitHub`] is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstVisitArm {
    /// Arm `0`: the intro strip fades in.
    IntroIn,
    /// Arm `1`: the intro strip holds `0x7B` ticks.
    IntroHold,
    /// Arm `2`: the intro strip fades out while the wall rises.
    IntroOut,
    /// Arm `3`: the course-title art zooms in.
    TitleZoom,
    /// Arm `4`: the course card fades in over the title art.
    CardIn,
    /// Arm `5`: the course card holds (pad-skippable).
    CardHold,
    /// Arm `6`: the wall drains.
    Drain,
    /// Arm `0x14`: on a first visit, a single pass-through tick.
    Return,
    /// Arm `0x15`: the ROUND card fades in and holds.
    RoundIn,
    /// Arm `0x16`: the ROUND card fades out; at zero the fight starts.
    RoundOut,
    /// Past arm `0x16` (`FUN_801D1510`, the fight).
    Done,
}

/// What a [`FirstVisitHub`] draws this frame: each screen's level, or
/// `None` for a screen its arm does not draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FirstVisitFrame {
    /// `*(0x801D1A7C)` - the brick wall's level; the backdrop emitter runs
    /// only while it is non-zero (`beqz a0` at `0x801D00A4`).
    pub backdrop: i32,
    /// The intro strip (record 3) at `*(0x801D1A80)`.
    pub intro: Option<i32>,
    /// The course-title art's face scale `*(0x801D1A88)` (drawn at the fixed
    /// level `0x80`; its drop shadow is always at scale `0x1000`).
    pub title_scale: Option<i32>,
    /// The course card `FUN_801D042C` at `*(0x801D1A84)`.
    pub course_card: Option<i32>,
    /// The ROUND card `FUN_801D02F0` at `*(0x801D1A84)`.
    pub round_card: Option<i32>,
}

/// The contest hub's **first visit**: arms `0..6` and `0x14..0x16` of
/// `FUN_801CF870` with the re-entry latch `_DAT_801D1AE0` zero - the intro
/// strip, the wall rising under it, the title zoom, the course card, the
/// wall draining, and the ROUND card over black before the first fight.
///
/// Two gates are modelled as always open: the load-busy word
/// `_DAT_8007BC20` (arms `3`, `4` and `0x16` wait on it) and arm `6`'s
/// wait for `_DAT_8007B648 == 0x80` - the port has no asynchronous load to
/// wait for. The two sound cues (`FUN_8003D53C` at arm `0`'s and arm
/// `0x15`'s first tick) are not the hub's to play here.
///
/// PORT: FUN_801cf870 (arms `0`..`6` and `0x14`..`0x16` on the latch-`0`
/// path: `0x801CF90C..0x801CFC94`, `0x801CFEE0..0x801D0084`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstVisitHub {
    arm: FirstVisitArm,
    /// The arm the last tick ran - the one whose emitter calls this frame's
    /// draw is (retail draws inside the arm, then the arm byte moves on).
    drawn: FirstVisitArm,
    /// The card level the last tick drew at: arm `5` draws before its exit
    /// test zeroes `0x801D1A84`.
    drawn_card: i32,
    /// `0x801D1A80`.
    intro: i32,
    /// `0x801D1A7C`.
    backdrop: i32,
    /// `0x801D1A84` - the intro hold counter in arm `1`, the card level after.
    card: i32,
    /// `0x801D1A88`.
    scale: i32,
    /// `0x801D1A70`.
    hold: i32,
    /// `0x801D1A8C`.
    card_hold: i32,
}

impl Default for FirstVisitHub {
    fn default() -> Self {
        Self::new()
    }
}

impl FirstVisitHub {
    /// A fresh first visit at arm `0`, every counter zero.
    pub const fn new() -> Self {
        Self {
            arm: FirstVisitArm::IntroIn,
            drawn: FirstVisitArm::IntroIn,
            drawn_card: 0,
            intro: 0,
            backdrop: 0,
            card: 0,
            scale: 0,
            hold: 0,
            card_hold: 0,
        }
    }

    /// Advance one hub tick. `dt` is the frame-skip factor
    /// (`_DAT_1F800393`, read as `lbu 0x7f(0x1F800314)` too), `pad` the edge
    /// snapshot `DAT_801D1A9C`.
    pub fn tick(&mut self, dt: u8, pad: u16) {
        let dt = i32::from(dt.max(1));
        let skip = pad & HUB_SKIP_PAD_MASK != 0;
        self.drawn = self.arm;
        match self.arm {
            FirstVisitArm::IntroIn => {
                self.intro += dt * HUB_FADE_STEP_FAST;
                if self.intro > HUB_FADE_FULL {
                    self.intro = HUB_FADE_FULL;
                    self.card = 0;
                    self.arm = FirstVisitArm::IntroHold;
                }
            }
            FirstVisitArm::IntroHold => {
                self.card += dt;
                if self.card >= HUB_INTRO_HOLD_TICKS {
                    self.card = 0;
                    self.arm = FirstVisitArm::IntroOut;
                }
            }
            FirstVisitArm::IntroOut => {
                self.backdrop += dt * HUB_FADE_STEP_FAST;
                self.intro = (self.intro - dt * HUB_FADE_STEP_FAST).max(0);
                if self.backdrop > HUB_FADE_FULL {
                    self.backdrop = HUB_FADE_FULL;
                    self.arm = FirstVisitArm::TitleZoom;
                }
                self.scale = TITLE_ZOOM_START;
            }
            FirstVisitArm::TitleZoom => {
                self.card = (self.card + dt * HUB_FADE_STEP_SLOW).min(HUB_FADE_FULL);
                self.scale -= dt * TITLE_ZOOM_STEP;
                if self.scale < TITLE_ZOOM_END {
                    self.scale = TITLE_ZOOM_END;
                    self.arm = FirstVisitArm::CardIn;
                }
            }
            FirstVisitArm::CardIn => {
                self.card += dt * HUB_FADE_STEP_SLOW;
                if self.card > HUB_FADE_FULL {
                    self.card = HUB_FADE_FULL;
                    self.hold = HUB_ROUND_BANNER_HOLD_TICKS;
                    self.arm = FirstVisitArm::CardHold;
                }
            }
            FirstVisitArm::CardHold => {
                // `jal 0x801D042C` at `0x801CFBBC` runs before the test.
                self.drawn_card = self.card;
                self.hold -= dt;
                if skip || self.hold < 0 {
                    self.card = 0;
                    self.arm = FirstVisitArm::Drain;
                }
            }
            FirstVisitArm::Drain => {
                self.backdrop -= dt * HUB_FADE_STEP_FAST;
                if self.backdrop < 0 {
                    self.backdrop = 0;
                    self.arm = FirstVisitArm::Return;
                }
            }
            FirstVisitArm::Return => self.arm = FirstVisitArm::RoundIn,
            FirstVisitArm::RoundIn => {
                self.card += dt * HUB_FADE_STEP_SLOW;
                if self.card > HUB_FADE_FULL {
                    self.card = HUB_FADE_FULL;
                    self.card_hold += dt;
                    if self.card_hold >= HUB_OPPONENT_CARD_HOLD_TICKS || skip {
                        self.arm = FirstVisitArm::RoundOut;
                    }
                }
            }
            FirstVisitArm::RoundOut => {
                self.card -= dt * HUB_FADE_STEP_SLOW;
                self.backdrop = (self.backdrop - dt * HUB_FADE_STEP_SLOW).max(0);
                if self.card < 0 {
                    self.card = 0;
                    self.arm = FirstVisitArm::Done;
                }
            }
            FirstVisitArm::Done => {}
        }
    }

    /// Which arm the hub is in.
    pub fn arm(&self) -> FirstVisitArm {
        self.arm
    }

    /// Whether the first fight has been handed its start.
    pub fn done(&self) -> bool {
        self.arm == FirstVisitArm::Done
    }

    /// The screens the last tick drew, per that arm's own emitter calls.
    pub fn frame(&self) -> FirstVisitFrame {
        use FirstVisitArm as A;
        let arm = self.drawn;
        let intro = matches!(arm, A::IntroIn | A::IntroHold | A::IntroOut).then_some(self.intro);
        let title_scale =
            matches!(arm, A::TitleZoom | A::CardIn | A::CardHold).then_some(self.scale);
        let course_card = match arm {
            A::CardIn => Some(self.card),
            A::CardHold => Some(self.drawn_card),
            _ => None,
        };
        let round_card = matches!(arm, A::RoundIn | A::RoundOut).then_some(self.card);
        FirstVisitFrame {
            backdrop: self.backdrop,
            intro,
            title_scale,
            course_card,
            round_card,
        }
    }
}

/// Whether a leg that is opening should raise the ROUND card itself.
///
/// Retail shows the card once per leg, in the hub, before the fight: arms
/// `0x15` / `0x16` of `FUN_801CF870` (`FUN_801D02F0` over the backdrop),
/// entered from `0x14` on either visit - the first visit reaches `0x14` from
/// arm `6`'s battle-load kick (`0x801CFC88..0x801CFC94`), a re-entered hub
/// from the INTERVAL arms. The port's re-entered hub plays those arms
/// ([`HubBackdrop`]), but the leg itself only opens later, when the player
/// walks back through the dome door - so a host that raised the card again
/// at that edge showed it twice. `card_shown_round` is the round a
/// [`HubBackdrop`] last drew its card for (`None` when no re-entered hub ran
/// since the last leg); the leg-open card is owed only when that is not the
/// round now opening.
///
/// REF: FUN_801cf870 (arms `0x15` / `0x16`)
pub fn leg_open_raises_round_card(card_shown_round: Option<i32>, round: i32) -> bool {
    card_shown_round != Some(round)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a first visit to the end, recording each frame.
    fn run_first_visit(pad_at_hold: bool) -> Vec<(FirstVisitArm, FirstVisitFrame)> {
        let mut hub = FirstVisitHub::new();
        let mut out = Vec::new();
        for _ in 0..2000 {
            let pad = if pad_at_hold && hub.arm() == FirstVisitArm::CardHold {
                0x40
            } else {
                0
            };
            hub.tick(1, pad);
            out.push((hub.arm(), hub.frame()));
            if hub.done() {
                break;
            }
        }
        out
    }

    #[test]
    fn the_first_visit_walks_every_arm_in_order() {
        let frames = run_first_visit(false);
        let mut arms: Vec<FirstVisitArm> = frames.iter().map(|(a, _)| *a).collect();
        arms.dedup();
        use FirstVisitArm as A;
        assert_eq!(
            arms,
            vec![
                A::IntroIn,
                A::IntroHold,
                A::IntroOut,
                A::TitleZoom,
                A::CardIn,
                A::CardHold,
                A::Drain,
                A::Return,
                A::RoundIn,
                A::RoundOut,
                A::Done
            ]
        );
        // The wall peaks at full under the title + course card and is gone
        // before the ROUND card: a first visit's arm 0x14 does not raise it.
        let peak = frames.iter().map(|(_, f)| f.backdrop).max().unwrap();
        assert_eq!(peak, HUB_FADE_FULL);
        for (_, f) in &frames {
            if f.round_card.is_some() {
                assert_eq!(f.backdrop, 0, "no wall under the first ROUND card");
            }
            if f.title_scale.is_some() {
                assert_eq!(
                    f.backdrop, HUB_FADE_FULL,
                    "the wall is full under the title"
                );
            }
        }
    }

    #[test]
    fn the_title_zooms_from_the_seed_to_unit_scale() {
        let frames = run_first_visit(false);
        let scales: Vec<i32> = frames.iter().filter_map(|(_, f)| f.title_scale).collect();
        assert_eq!(scales[0], TITLE_ZOOM_START - TITLE_ZOOM_STEP);
        assert_eq!(*scales.last().unwrap(), TITLE_ZOOM_END);
        assert!(scales.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn the_course_card_carries_the_zoom_ramp_and_a_skippable_hold() {
        let full = run_first_visit(false);
        let skipped = run_first_visit(true);
        let held = |f: &[(FirstVisitArm, FirstVisitFrame)]| {
            f.iter()
                .filter(|(a, _)| *a == FirstVisitArm::CardHold)
                .count()
        };
        assert_eq!(held(&full) as i32, HUB_ROUND_BANNER_HOLD_TICKS + 1);
        assert_eq!(
            held(&skipped),
            1,
            "a pad edge leaves on the first hold tick"
        );
        // The card level started climbing under the zoom (arm 3 ramps
        // `0x801D1A84` too), so it enters arm 4 above zero.
        let first_card = full.iter().find_map(|(_, f)| f.course_card).unwrap();
        assert!(first_card > 0);
    }

    #[test]
    fn the_leg_after_a_hub_card_does_not_replay_it() {
        assert!(
            leg_open_raises_round_card(None, 1),
            "first leg: no hub card ran"
        );
        assert!(
            !leg_open_raises_round_card(Some(2), 2),
            "the hub showed round 2"
        );
        assert!(leg_open_raises_round_card(Some(2), 3));
    }
    use legaia_asset::ringside_still::{PROT_INDEX_DEFAULT, PROT_INDEX_LOW_HP};

    #[test]
    fn below_half_hp_picks_the_second_still() {
        assert_eq!(still_prot_index(500, 500), PROT_INDEX_DEFAULT);
        assert_eq!(still_prot_index(250, 500), PROT_INDEX_DEFAULT);
        assert_eq!(still_prot_index(249, 500), PROT_INDEX_LOW_HP);
        assert_eq!(still_prot_index(0, 0), PROT_INDEX_DEFAULT);
    }

    /// Drive the envelope through a whole re-entry with an INTERVAL screen
    /// the way a host holds one.
    #[test]
    fn a_reentry_rises_dims_returns_and_drains() {
        let mut screen = Some(HubScreen::interval(40));
        let mut b = HubBackdrop::reentry(PROT_INDEX_LOW_HP);
        assert_eq!(b.variant(), 1);
        assert!(!b.visible(), "the init zeroes the level");
        let mut peak = 0;
        let mut floor_seen = false;
        let mut card_seen = false;
        for _ in 0..2000 {
            let stage = screen.as_ref().map(|s| s.stage());
            b.tick(1, 0, stage, true);
            if let Some(s) = screen.as_mut() {
                s.tick(1, 0);
                if s.done() {
                    screen = None;
                }
            }
            peak = peak.max(b.level());
            floor_seen |= b.stage() == BackdropStage::Interval && b.level() == HUB_BACKDROP_HALF;
            card_seen |= b.card_brightness().is_some_and(|c| c > 0);
            if b.done() {
                break;
            }
        }
        assert_eq!(peak, HUB_FADE_FULL);
        assert!(floor_seen, "the tally dims the still to half");
        assert!(card_seen, "the ROUND card draws over it");
        assert!(b.done() && !b.visible());
    }

    #[test]
    fn the_interval_rise_is_the_fast_rate() {
        let mut b = HubBackdrop::reentry(PROT_INDEX_DEFAULT);
        for _ in 0..HUB_FADE_FULL / HUB_FADE_STEP_FAST {
            b.tick(1, 0, Some(HubScreenStage::FadeIn), false);
        }
        assert_eq!(b.level(), HUB_FADE_FULL);
        // Held while the tally's first lane has not reached its clamp.
        b.tick(1, 0, Some(HubScreenStage::Hold), false);
        assert_eq!(b.level(), HUB_FADE_FULL);
        b.tick(1, 0, Some(HubScreenStage::Hold), true);
        assert_eq!(b.level(), HUB_FADE_FULL - HUB_FADE_STEP_SLOW);
    }
}
