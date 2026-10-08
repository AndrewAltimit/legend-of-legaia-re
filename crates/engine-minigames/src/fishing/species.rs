//! Retail species selection: cadence, band, strike, the band-4 gate, and the fish AI.
//! Split out of `fishing.rs`.

use super::*;

// --- Retail species selection: cadence, band, strike, band-4 gate -----------
//
// The retail pond does NOT pick the hooked species from the cast power: the
// pre-hook half of `FUN_801d26cc` assigns
// `species = spawn_table[lure*8 + band]`, where `lure` is the equipped-lure
// row (`_DAT_80084450`) and `band` (`DAT_801d90e8`) comes from a per-frame
// roll that a matched reel-cadence template overrides. See
// `docs/subsystems/minigame-fishing.md` "Species selection and the band-4
// gate". The kernels below port that path; [`PondSession`] composes them
// with the confirmed cast/tension/score kernels above into the full
// venue-faithful loop (the browser minigame's engine).

/// Band-roll cutoffs (`FUN_801d26cc`): `r = rand & 0xfff`; `r <= 0xc00` band 3
/// (~75.0%), `<= 0xe70` band 2 (~15.2%), `<= 0xf38` band 1 (~4.9%), else
/// band 0 (~4.9%). No roll outcome maps to band 4.
pub const BAND_ROLL_CUTOFF_3: i32 = 0xc00;
/// See [`BAND_ROLL_CUTOFF_3`].
pub const BAND_ROLL_CUTOFF_2: i32 = 0xe70;
/// See [`BAND_ROLL_CUTOFF_3`].
pub const BAND_ROLL_CUTOFF_1: i32 = 0xf38;

/// Frames a cadence-matched band holds (the `DAT_801d90ec` countdown arm).
pub const BAND_HOLD_FRAMES: i32 = 0x40;

/// Strike-credit base: `credit = countdown + 2` (+1 per fresh input edge).
pub const STRIKE_CREDIT_BASE: i32 = 2;

/// A length readout (`DAT_801d9280`) under this cannot strike at all.
///
/// This is not a test retail runs. It is the *consequence* of the strike
/// ladder: below it the credit base is replaced with
/// [`crate::fishing_actors::BITE_FAR_CREDIT`] (`-100`) and the modulus jumps
/// to `2000`, so `rand % 2000 < credit` can never hold however much the
/// water-class bonus and the pad nudges add. [`BandCheck::tick`] reproduces
/// the ladder rather than the shortcut; the constant stays as the documented
/// bound.
pub const STRIKE_MIN_READOUT: i32 = 200;

/// The credit is zeroed while the length readout is under this.
pub const STRIKE_CREDIT_ZERO_READOUT: i32 = 100;

/// The band-roll body only runs while the line record exceeds this.
pub const BAND_CHECK_MIN_RECORD: i32 = 500;

/// Reel-in-complete threshold: the hooked fight lands once the line record
/// (`DAT_801d927c`) drops below this (`FUN_801d26cc` seeds the reel-in
/// banner on `record < 0x136` while hooked).
pub const LAND_RECORD: i32 = 0x136;

/// Roll a cast band from `r = rand & 0xfff` against the three fixed cutoffs.
///
/// One kernel, one implementation: the arm is
/// [`crate::fishing_actors::roll_hit_type`], which spells the same three
/// cutoffs as retail's overwrite ladder (seed `3`, then `2` / `1` / `0` as
/// the draw passes each bound) instead of as an `else` chain. This wrapper
/// exists only for the `u32` band type the spawn table is indexed by.
// PORT: FUN_801d26cc (band roll: 0xc00 / 0xe70 / 0xf38 cutoffs)
pub fn band_roll(r: i32) -> u32 {
    crate::fishing_actors::roll_hit_type(r as u32) as u32
}

/// The strike-time band-4 gate: whether an active band 0 upgrades to the
/// venue's rare band. Every condition is venue-hardwired: the third rod
/// (`rod == 2`), the cast counter even, the venue's own lure row (Normal at
/// venue 0 / Buma, Heavy at venue 1 / Vidna), band 0 active, and then a
/// `rand` mask (`1/16` at Buma - which additionally needs more than 50
/// lifetime casts - `1/4` at Vidna). `rng` is only advanced when the
/// preconditions hold, matching the retail short-circuit.
// PORT: FUN_801d26cc (band-4 gate: cast-counter / lure / rod / band-0 arm)
pub fn band4_gate(
    venue: usize,
    lure: u32,
    rod: i32,
    band: u32,
    casts: i32,
    rng: &mut BiosRand,
) -> bool {
    if band != 0 || rod != 2 || (casts & 1) != 0 {
        return false;
    }
    match venue {
        0 => lure == 1 && casts > 0x32 && (rng.next_u15() & 0xf) == 0,
        _ => lure == 2 && (rng.next_u15() & 3) == 0,
    }
}

/// The species-spawn lookup: `spawn_table[lure * 8 + band]`, where the table
/// is a venue page of `8 x 8` u32 species ids
/// ([`legaia_asset::fishing_species::parse_spawn_tables`]). Returns `None`
/// for an out-of-range row/band or a species id past the 10-record table.
// PORT: FUN_801d26cc (species lookup: spawn_table[lure*8 + band])
pub fn spawn_species(table: &[[u32; 8]], lure: u32, band: u32) -> Option<usize> {
    let id = *table.get(lure as usize)?.get(band as usize)? as usize;
    (id < legaia_asset::fishing_species::SPECIES_COUNT).then_some(id)
}

/// The reel-cadence recogniser: a 16-slot `{button, held-frames}` ring buffer
/// (`DAT_801d91e4`, write index `DAT_801d91dc`) fed the decoded reel button
/// each frame and walked backwards against the overlay's four gesture
/// templates with a +-10 frame-step tolerance. On a full match the buffer is
/// reset (`FUN_801d746c`) and the matched template id is reported - the
/// consumer stores it **as the cast band**.
///
/// The `history_window` word of each template bounds the total duration the
/// backwards walk may span; reading it as an inclusive bound (+ tolerance) is
/// this port's interpretation - the per-step button/duration match and the
/// reset are the pinned parts.
// PORT: FUN_801d3db4 (reel-cadence recogniser: ring accumulate + template walk)
// PORT: FUN_801d746c (ring reset: index + all 16 slots cleared)
#[derive(Debug, Clone)]
pub struct ReelCadence {
    pub(super) templates: Vec<CadenceTemplate>,
    pub(super) ring: [(u8, i32); 16],
    pub(super) idx: usize,
    /// Last decoded button (`DAT_801d9064`) - not cleared by the reset.
    pub(super) last: u8,
}

impl ReelCadence {
    /// A recogniser over the disc's parsed gesture templates.
    pub fn new(templates: Vec<CadenceTemplate>) -> Self {
        Self {
            templates,
            ring: [(0, 0); 16],
            idx: 0,
            last: 0,
        }
    }

    /// Reset the ring (index + every slot zeroed; the last-button latch is
    /// retail's `DAT_801d9064`, which the reset does not touch).
    ///
    /// The body is [`crate::fishing_chrome::clear_slot_ring`] - the same
    /// `FUN_801D746C` this type's `PORT` tag names, kept as one
    /// implementation rather than two readings of the same 16 x 2-word table.
    pub fn reset(&mut self) {
        crate::fishing_chrome::clear_slot_ring(&mut self.idx, &mut self.ring);
    }

    /// Feed this frame's decoded reel button (`0` idle / `1` reel A / `2`
    /// reel B) and frame step; returns the matched template id (= the band)
    /// if a gesture completed this frame, resetting the ring.
    pub fn feed(&mut self, button: u8, frame_step: i32) -> Option<usize> {
        if button != self.last {
            self.last = button;
            self.idx = (self.idx + 1) % self.ring.len();
            self.ring[self.idx] = (button, 0);
        }
        self.ring[self.idx].1 += frame_step.max(1);

        'template: for (t, tpl) in self.templates.iter().enumerate() {
            let n = tpl.steps.len();
            if n == 0 || n > self.ring.len() {
                continue;
            }
            let mut span = 0i32;
            for k in 0..n {
                let slot = self.ring[(self.idx + self.ring.len() - k) % self.ring.len()];
                let step = tpl.steps[n - 1 - k];
                if slot.0 != step.button || (slot.1 - step.duration).abs() > CADENCE_TOLERANCE {
                    continue 'template;
                }
                span += slot.1;
            }
            if span > tpl.history_window + CADENCE_TOLERANCE {
                continue;
            }
            self.reset();
            return Some(t);
        }
        None
    }
}

/// The pre-hook band + strike check (`FUN_801d26cc`, run per frame while no
/// fish is hooked and the lure is in the water).
#[derive(Debug, Clone, Copy)]
pub struct BandCheck {
    /// The live cast band (`DAT_801d90e8`).
    pub band: u32,
    /// Band-hold countdown (`DAT_801d90ec`); doubles as the strike credit.
    pub countdown: i32,
    /// A cadence matched this frame - the "Good!" splash seed
    /// (`DAT_801d90f0`), fired for *any* matched template.
    pub splash: bool,
}

impl Default for BandCheck {
    fn default() -> Self {
        Self {
            band: 3,
            countdown: 0,
            splash: false,
        }
    }
}

impl BandCheck {
    /// Run one waiting-phase frame.
    ///
    /// `record` is the line record (`DAT_801d927c`), `readout` the HUD length
    /// term (`DAT_801d9280` = `max(record - 300, 0)`), `cadence` the
    /// recogniser's match this frame, `edge_bonus` the pad nudge
    /// ([`crate::fishing_actors::bite_pad_nudge`]), `water_bonus` the
    /// water-class addend the tile under the lure contributes
    /// ([`crate::fishing_actors::water_tile_class`]; `0` off water), and
    /// `reel_held` whether a reel button is held (`_DAT_8007b850 & 0xc0`).
    /// Returns `true` when a strike lands this frame.
    ///
    /// `edge_bonus` and `water_bonus` stay separate arguments because they
    /// are separate retail addends onto one register - `addu s1,s1,s2` at
    /// `0x801D3434` for the water class, then one `addiu s1,s1,1` per newly-pressed
    /// pad mask from `0x801D3450` - and not two readings of one quantity.
    ///
    /// Pinned: the every-frame re-entry (countdown clamped at 0), the
    /// cadence-match band store + `0x40` hold + splash, the roll cutoffs, the
    /// `credit = countdown + 2 (+ edges)` strike credit and its `0x40`
    /// cadence-match override, the modulus ladder
    /// ([`crate::fishing_actors::bite_interval`]) and the far band's credit
    /// replacement ([`crate::fishing_actors::bite_credit_override`]), the
    /// credit zeroing under a `100` readout, and the reel-held requirement.
    ///
    /// The credit base is sampled **before** the countdown decays, which is
    /// retail's order (`addiu s1, v0, 2` runs off the loaded value, then the
    /// store writes the decremented one) - a frame's credit is the countdown
    /// it entered with.
    // PORT: FUN_801d26cc (pre-hook band check + strike roll)
    #[allow(clippy::too_many_arguments)] // the retail check reads exactly these globals
    pub fn tick(
        &mut self,
        rng: &mut BiosRand,
        cadence: Option<usize>,
        record: i32,
        readout: i32,
        edge_bonus: i32,
        water_bonus: i32,
        reel_held: bool,
        frame_step: i32,
    ) -> bool {
        self.splash = false;
        let mut credit_base = self.countdown + STRIKE_CREDIT_BASE;
        if self.countdown > 0 {
            // Matched band holds for the countdown; clamped to 0 on underflow
            // so the steady state re-enters every tick.
            self.countdown = (self.countdown - frame_step.max(1)).max(0);
        } else if record > BAND_CHECK_MIN_RECORD {
            match cadence {
                Some(t) => {
                    self.band = t as u32;
                    self.countdown = BAND_HOLD_FRAMES;
                    self.splash = true;
                    // Retail overwrites the credit register with the hold
                    // length itself on the match frame, so the base is `0x40`
                    // and not `0x40 + 2`.
                    credit_base = BAND_HOLD_FRAMES;
                }
                None => {
                    self.band = band_roll(rng.next_u15() as i32);
                }
            }
        }

        if !reel_held {
            return false;
        }
        // The readout picks both halves of the roll: the modulus off the
        // interval ladder, and - in the far band - a credit base that replaces
        // the countdown term outright.
        let interval = crate::fishing_actors::bite_interval(readout, false);
        let mut credit = crate::fishing_actors::bite_credit_override(readout)
            .unwrap_or(credit_base)
            + water_bonus.max(0)
            + edge_bonus.max(0);
        if readout < STRIKE_CREDIT_ZERO_READOUT {
            credit = 0;
        }
        (rng.next_u15() as i32 % interval.max(1)) < credit
    }
}

/// The hooked fish's behaviour sub-state (`DAT_801d910c`): run / dart left /
/// dart right / dive, re-rolled when its countdown (`DAT_801d9110`) expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishMove {
    /// Steady run: full pull, line sinks by the species sink factor.
    Run,
    /// Lateral dart (left).
    DartLeft,
    /// Lateral dart (right).
    DartRight,
    /// Dive: picked when the species depth gate is under the line depth.
    Dive,
}

/// One frame of fish output from [`FishAi::tick`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishFrame {
    /// This frame's pull (`((rand & 0xff) + bias) * pull_factor / 150`).
    pub pull: i32,
    /// Lateral dart push (signed; `((step >> 2) + 0x20) * dart_factor / 100`).
    pub lateral: i32,
    /// Line-depth sink this frame (`pull * sink_factor / 150` in the run
    /// state).
    pub sink: i32,
}

/// The fish-AI half of `FUN_801d4004`: the behaviour sub-state machine and
/// the per-frame pull / dart / sink terms, driven by the hooked species'
/// per-record factors.
///
/// The per-field formulas (pull, dart push, sink, the `rand & 0xfff`
/// cutoff comparisons, the depth-gate dive pick) are the documented ones;
/// the *composition* - which cutoff feeds which state and the re-roll
/// interval - is an engine-side reading of the same function (the doc's
/// per-field table stops short of the branch order).
// PORT: FUN_801d4004 (fish behaviour sub-state + pull/dart/sink terms)
#[derive(Debug, Clone, Copy)]
pub struct FishAi {
    /// Current behaviour (`DAT_801d910c`).
    pub state: FishMove,
    /// Frames until the next behaviour re-roll (`DAT_801d9110`).
    pub(super) timer: i32,
}

impl Default for FishAi {
    fn default() -> Self {
        Self {
            state: FishMove::Run,
            timer: 0,
        }
    }
}

impl FishAi {
    /// Per-frame pull bias. The doc pins the `((rand & 0xff) + bias) *
    /// factor / 150` shape but not the bias literal; `0x40` keeps the pull
    /// centred near `factor` (rand averages `0x80`).
    pub const PULL_BIAS: i32 = 0x40;

    pub(super) fn reroll(&mut self, sp: &FishingSpecies, depth: i32, rng: &mut BiosRand) {
        // Dive is the depth-gated pick (`+0x14`: behaviour pick when
        // `f < line-depth`); otherwise roll the cutoffs.
        if sp.depth_gate < depth {
            self.state = FishMove::Dive;
        } else {
            let r = (rng.next_u15() & 0xfff) as i32;
            self.state = if sp.roll_cutoff_a <= r {
                FishMove::Run
            } else if r < sp.roll_cutoff_c {
                if rng.next_u15() & 1 == 0 {
                    FishMove::DartLeft
                } else {
                    FishMove::DartRight
                }
            } else if r < sp.roll_cutoff_b {
                FishMove::Run
            } else if rng.next_u15() & 1 == 0 {
                FishMove::DartLeft
            } else {
                FishMove::DartRight
            };
        }
        // Re-roll interval: not byte-pinned; a fraction of a second keeps the
        // fight lively without thrashing.
        self.timer = 0x18 + (rng.next_u15() & 0x1f) as i32;
    }

    /// Advance one frame: countdown, re-roll on expiry, and produce this
    /// frame's pull / lateral / sink terms from the species factors.
    pub fn tick(
        &mut self,
        sp: &FishingSpecies,
        depth: i32,
        rng: &mut BiosRand,
        frame_step: i32,
    ) -> FishFrame {
        let fs = frame_step.max(1);
        self.timer -= fs;
        if self.timer <= 0 {
            self.reroll(sp, depth, rng);
        }
        let pull = (((rng.next_u15() & 0xff) as i32 + Self::PULL_BIAS) * sp.pull_factor) / 150;
        let mut out = FishFrame {
            pull,
            lateral: 0,
            sink: 0,
        };
        match self.state {
            FishMove::Run => out.sink = (pull * sp.sink_factor) / SINK_DIVISOR,
            FishMove::Dive => out.sink = (pull * sp.sink_factor) / 75,
            FishMove::DartLeft | FishMove::DartRight => {
                let push = (((fs) >> 2) + 0x20) * sp.dart_factor / 100;
                out.lateral = if self.state == FishMove::DartLeft {
                    -push
                } else {
                    push
                };
            }
        }
        out
    }
}
