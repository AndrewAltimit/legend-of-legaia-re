//! Contest-hub presentation: sprite glides, the time meter and the hub screen timing.
//! Split out of `muscle_dome.rs`.

/// One animated-sprite glide record (`ctx + 0x11B4 + i*0xC`, up to 0x28
/// handles): a sprite easing from `start` to `target` over `total` frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpriteGlide {
    /// `+0x00` total frame count; `0` = slot inactive.
    pub total: u8,
    /// `+0x01` elapsed frames.
    pub elapsed: u8,
    /// `+0x04`/`+0x06` target screen position.
    pub target: (i16, i16),
    /// `+0x08`/`+0x0A` start screen position.
    pub start: (i16, i16),
}

/// One step's outcome for a glide handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlideStep {
    /// Slot inactive - nothing written.
    Idle,
    /// The step reached the target: the sprite snaps to `target` and the
    /// record deactivates (`total = 0`).
    Arrived { pos: (i16, i16) },
    /// Still in flight: linear interpolation `start + (target - start) *
    /// elapsed / total` (signed division), plus the remaining-frames count
    /// retail folds into its return (`total - elapsed + 1`).
    Moving { pos: (i16, i16), remaining: u32 },
}

impl SpriteGlide {
    /// PORT: FUN_801d9bbc (one handle's step; retail loops all 0x28 handles
    /// per frame with the frame delta from scratchpad `0x1F800393`).
    ///
    /// Arrival test is `dt >= total - elapsed` **before** accumulating;
    /// otherwise `elapsed += dt` first and the eased position uses the new
    /// elapsed count.
    pub fn step(&mut self, dt: u8) -> GlideStep {
        if self.total == 0 {
            return GlideStep::Idle;
        }
        if dt as i32 >= self.total as i32 - self.elapsed as i32 {
            self.total = 0;
            return GlideStep::Arrived { pos: self.target };
        }
        self.elapsed += dt;
        let lerp = |s: i16, t: i16| {
            let d = (t as i32 - s as i32) * self.elapsed as i32 / self.total as i32;
            (s as i32 + d) as i16
        };
        GlideStep::Moving {
            pos: (
                lerp(self.start.0, self.target.0),
                lerp(self.start.1, self.target.1),
            ),
            remaining: (self.total - self.elapsed) as u32 + 1,
        }
    }
}

/// The round time meter's counter ceiling (`0xC` ticks = a full bar).
pub const TIME_METER_MAX: u8 = 0xC;

/// PORT: FUN_801d3444 (PROT 0898; core ramp + bar mapping) - the round **time meter**:
/// while the phase tag is `'P'` (0x50, the selection phase) and the ramp
/// flag is up, the 0..=0xC counter climbs by the frame delta (clamped at
/// [`TIME_METER_MAX`]); otherwise it drains by the delta (floored at 0).
/// The bar sprite's Y offset is `counter * 160 / 12 - 0x92` (the
/// `0x2AAAAAAB` reciprocal-multiply divide) - `-0x92` empty, `+0xE` full.
/// Returns `(new_counter, bar_y)`.
///
/// Wired: `legaia_engine_core::muscle_dome::MuscleDomeSession::tick_time_meter`, which the host calls once a
/// frame while a contest is up.
pub fn time_meter_step(counter: u8, dt: u8, in_select_phase: bool, ramp_up: bool) -> (u8, i16) {
    let new = if ramp_up && in_select_phase {
        (counter as u32 + dt as u32).min(TIME_METER_MAX as u32) as u8
    } else {
        counter.saturating_sub(dt)
    };
    let bar_y = (new as i32 * 160 / 12 - 0x92) as i16;
    (new, bar_y)
}

// ------------------------------------------------------- hub screen timing

/// The value every hub fade counter clamps at (`slti v0,v0,0x81` at
/// `0x801CF944` / `0x801CFA14` / `0x801CFB34` / `0x801CFD08` / `0x801CFF90`).
///
/// It is also the emitter's **neutral** brightness: the sprite emitter scales
/// each stored channel by `c * brightness / 256` (`mult`/`sra 8` inside
/// `FUN_801D050C`), so `0x80` reproduces the record's own colour as a PSX
/// textured primitive's neutral modulation. A host that draws a hub screen at
/// `0x100` draws it at twice retail's brightness.
pub const HUB_FADE_FULL: i32 = 0x80;

/// The fast fade rate: `counter += dt * 4` (`sll v1,v1,0x2`, `0x801CF938`).
/// 32 ticks from black to [`HUB_FADE_FULL`] at `dt == 1`.
pub const HUB_FADE_STEP_FAST: i32 = 4;

/// The slow fade rate: `counter += dt * 2` (`0x801CFA78`, `0x801CFB28`,
/// `0x801CFF64`). 64 ticks at `dt == 1`.
pub const HUB_FADE_STEP_SLOW: i32 = 2;

/// The half-brightness floor the INTERVAL arm dims its backdrop to while the
/// score tally rolls (`slti v0,v0,0x40` at `0x801CFDAC`).
pub const HUB_BACKDROP_HALF: i32 = 0x40;

/// Ticks the "Welcome to the Muscle Dome!" strip holds at full brightness -
/// the phase-1 arm counts `DAT_801D1A84` up by `dt` and leaves at
/// `slti v0,v0,0x7b` (`0x801CF9A0`).
pub const HUB_INTRO_HOLD_TICKS: i32 = 0x7B;

/// Ticks the ROUND banner holds - the phase-4 arm seeds `DAT_801D1A70` with
/// `0xB4` (`li v1,0xb4` at `0x801CFB68`) and phase 5 counts it down by `dt`,
/// leaving on `bgez` (`0x801CFBEC`).
pub const HUB_ROUND_BANNER_HOLD_TICKS: i32 = 0xB4;

/// Ticks the opponent / ROUND-n card holds - `DAT_801D1A8C` counts up by `dt`
/// and leaves at `slti v0,v0,0x3d` (`0x801CFFB8`).
pub const HUB_OPPONENT_CARD_HOLD_TICKS: i32 = 0x3D;

/// Pad mask a hold arm tests to let the player skip the rest of it
/// (`andi v0,v0,0xf4` at `0x801CFBE0` / `0x801CFFE4`, over the edge snapshot
/// `DAT_801D1A9C = _DAT_8007B874 | _DAT_8007B938`).
///
/// Only the two card holds are skippable; the intro strip's hold is not.
pub const HUB_SKIP_PAD_MASK: u16 = 0xF4;

/// Lead-in ticks before the score tally's four roll-up lanes start stepping
/// (`slti 0x11` at `0x801CF0CC` / `0x801CF144` / `0x801CF1BC` / `0x801CF234`,
/// each reseeding to `0x10`).
pub const HUB_TALLY_ROLL_LEAD_TICKS: i32 = 0x11;

/// Value each roll-up lane's tick counter reseeds to after a step.
pub const HUB_TALLY_ROLL_RESEED: i32 = 0x10;

/// Per-lane vsync delay of the tally's four "ka-ching" cues - the INTERVAL
/// arm writes the cue ring `DAT_8007B6D8 = [0x202, 0x202, 0x202, 0x203]` and
/// this parallel countdown array `DAT_8007C338` at
/// `0x801CFCAC..0x801CFCEC`.
pub const HUB_TALLY_CUE_STAGGER: [u8; 4] = [0, 0x1E, 0x3C, 0x5A];

/// The four ring ids the INTERVAL arm writes beside [`HUB_TALLY_CUE_STAGGER`]
/// (`0x801CFCAC..0x801CFCC8`), one per ring slot. Both are runtime-bank ids:
/// they resolve against the arena's own descriptor bundle
/// (`legaia_asset::minigame_sfx::ARENA_SFX_BUNDLE_PROT_INDEX`), whose rows
/// `0x202` / `0x203` key tones 3 and 4..5 of the arena's slot-3 side bank.
pub const HUB_TALLY_CUES: [i16; 4] = [0x202, 0x202, 0x202, 0x203];

/// Which stage of its envelope a hub screen is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubScreenStage {
    /// The fade counter is climbing toward [`HUB_FADE_FULL`].
    FadeIn,
    /// The counter is parked at full and the hold counter is running.
    Hold,
    /// The fade counter is draining back toward the screen's floor.
    FadeOut,
    /// The screen is finished; the hub arm has advanced past it.
    Done,
}

/// One hub screen's retail fade / hold envelope: the counter arms of
/// `FUN_801CF870` reduced to the three stages every screen shares.
///
/// Retail does not hold a screen for one fixed frame count - the count the
/// two host timelines used to invent. Each screen is a fade-in at its own
/// rate, a hold at full, and a fade-out, and two of the four holds end early
/// on a pad press. This carries the measured literals so neither host has to
/// pick a number.
///
/// PORT: FUN_801cf870 (arms `0`..`6`, `0x0A`..`0x0C`, `0x14`..`0x16` - the
/// `DAT_801D1A70` / `1A7C` / `1A80` / `1A84` / `1A8C` counter family)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubScreen {
    pub(super) stage: HubScreenStage,
    pub(super) level: i32,
    pub(super) hold: i32,
    pub(super) fade_in_step: i32,
    pub(super) fade_out_step: i32,
    pub(super) hold_ticks: i32,
    /// Level the fade-out drains to - `0` for a screen that leaves, or
    /// [`HUB_BACKDROP_HALF`] for the INTERVAL backdrop, which dims to half
    /// and stays there while the tally rolls.
    pub(super) floor: i32,
    pub(super) skippable: bool,
}

impl HubScreen {
    pub(super) const fn new(
        fade_in_step: i32,
        hold_ticks: i32,
        fade_out_step: i32,
        floor: i32,
        skippable: bool,
    ) -> Self {
        Self {
            stage: HubScreenStage::FadeIn,
            level: 0,
            hold: 0,
            fade_in_step,
            fade_out_step,
            hold_ticks,
            floor,
            skippable,
        }
    }

    /// The "Welcome to the Muscle Dome!" strip: arm `0` fades it in at the
    /// fast rate, arm `1` holds it [`HUB_INTRO_HOLD_TICKS`] ticks (no skip),
    /// arm `2` cross-fades it out at the fast rate.
    pub const fn intro_card() -> Self {
        Self::new(
            HUB_FADE_STEP_FAST,
            HUB_INTRO_HOLD_TICKS,
            HUB_FADE_STEP_FAST,
            0,
            false,
        )
    }

    /// The leg-open banner's envelope: arm `4`'s slow fade-in, arm `5`'s
    /// [`HUB_ROUND_BANNER_HOLD_TICKS`]-tick pad-skippable hold, arm `6`'s
    /// fast fade-out.
    ///
    /// Those arms are not the ROUND banner's on the disc. Arm `4` fades the
    /// **course card** `FUN_801D042C` in over the title art, arm `5` holds
    /// both and clears the card on exit (`sw zero,0x1a84` at `0x801CFC00`),
    /// and arm `6`'s `4 dt` drain is the first-visit backdrop level
    /// (`0x801CFC54`), with nothing else drawn. The ROUND banner
    /// (`FUN_801D02F0`) is arm `0x15`'s, under [`Self::opponent_card`]. The
    /// play hosts run those arms through
    /// `legaia_engine_core::muscle_ringside::FirstVisitHub` and raise any other leg-open
    /// ROUND card on [`Self::opponent_card`]; this envelope stays for the
    /// standalone page's sampled screen `1`.
    pub const fn round_banner() -> Self {
        Self::new(
            HUB_FADE_STEP_SLOW,
            HUB_ROUND_BANNER_HOLD_TICKS,
            HUB_FADE_STEP_FAST,
            0,
            true,
        )
    }

    /// The opponent / ROUND-n card: arm `0x15` fades in at the slow rate and
    /// holds [`HUB_OPPONENT_CARD_HOLD_TICKS`] ticks (pad-skippable); arm
    /// `0x16` fades out at the slow rate.
    pub const fn opponent_card() -> Self {
        Self::new(
            HUB_FADE_STEP_SLOW,
            HUB_OPPONENT_CARD_HOLD_TICKS,
            HUB_FADE_STEP_SLOW,
            0,
            true,
        )
    }

    /// The between-legs INTERVAL + score tally: arm `0x0A` fades it in at the
    /// fast rate, arm `0x0B` dims the backdrop toward
    /// [`HUB_BACKDROP_HALF`] at the slow rate while the tally rolls, and arm
    /// `0x0C` drains the rest. Those last two are one fade-out here, at the
    /// slow rate, since nothing between them changes what is drawn.
    ///
    /// Its "hold" is the tally roll, which is data-dependent (the four lanes
    /// step one row per tick after a [`HUB_TALLY_ROLL_LEAD_TICKS`] lead-in),
    /// so the hold length is the caller's: pass the roll length in ticks.
    pub const fn interval(roll_ticks: i32) -> Self {
        Self::new(HUB_FADE_STEP_FAST, roll_ticks, HUB_FADE_STEP_SLOW, 0, false)
    }

    /// Advance one hub tick. `dt` is the adaptive frame-skip factor
    /// `_DAT_1F800393` every counter step is scaled by (`1` at the normal
    /// cadence); `pad` is the arm's edge snapshot `DAT_801D1A9C`, which ends
    /// a skippable hold when it carries any [`HUB_SKIP_PAD_MASK`] bit.
    pub fn tick(&mut self, dt: u8, pad: u16) {
        let dt = dt.max(1) as i32;
        match self.stage {
            HubScreenStage::FadeIn => {
                self.level += dt * self.fade_in_step;
                if self.level >= HUB_FADE_FULL {
                    self.level = HUB_FADE_FULL;
                    self.stage = HubScreenStage::Hold;
                }
            }
            HubScreenStage::Hold => {
                self.hold += dt;
                let skipped = self.skippable && pad & HUB_SKIP_PAD_MASK != 0;
                if skipped || self.hold >= self.hold_ticks {
                    self.stage = HubScreenStage::FadeOut;
                }
            }
            HubScreenStage::FadeOut => {
                self.level -= dt * self.fade_out_step;
                if self.level <= self.floor {
                    self.level = self.floor;
                    self.stage = HubScreenStage::Done;
                }
            }
            HubScreenStage::Done => {}
        }
    }

    /// The brightness argument to pass the hub sprite emitters
    /// (`0 ..= `[`HUB_FADE_FULL`]).
    pub fn brightness(&self) -> i32 {
        self.level
    }

    /// Which stage the envelope is in.
    pub fn stage(&self) -> HubScreenStage {
        self.stage
    }

    /// Whether the screen still draws - anything but [`HubScreenStage::Done`]
    /// at a zero floor.
    pub fn visible(&self) -> bool {
        self.stage != HubScreenStage::Done || self.floor > 0
    }

    /// Whether the hub arm has advanced past this screen.
    pub fn done(&self) -> bool {
        self.stage == HubScreenStage::Done
    }

    /// Total ticks this screen runs for at `dt == 1` with no skip - what a
    /// host that wants a single number should ask for instead of inventing
    /// one. Fade-in + hold + fade-out.
    pub fn total_ticks(&self) -> i32 {
        let ceil_div = |n: i32, d: i32| (n + d - 1) / d.max(1);
        let up = ceil_div(HUB_FADE_FULL, self.fade_in_step);
        let down = ceil_div(HUB_FADE_FULL - self.floor, self.fade_out_step);
        up + self.hold_ticks + down
    }
}
