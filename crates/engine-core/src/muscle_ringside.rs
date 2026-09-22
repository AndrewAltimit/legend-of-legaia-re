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
//! What the port does **not** model is the other half of the fork: on a
//! first visit (latch zero) the same emitter rebuilds the hub's live scene
//! as six tiled prims and a 320x240 fill. Neither host draws that backdrop.

use crate::muscle_dome::{
    HUB_BACKDROP_HALF, HUB_FADE_FULL, HUB_FADE_STEP_FAST, HUB_FADE_STEP_SLOW, HubScreen,
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

#[cfg(test)]
mod tests {
    use super::*;
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
