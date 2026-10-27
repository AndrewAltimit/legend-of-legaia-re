//! Retail species selection: cadence, band, strike, the band-4 gate, and the fish AI.

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
/// ([`legaia_minigame_formats::fishing_species::parse_spawn_tables`]). Returns `None`
/// for an out-of-range row/band or a species id past the 10-record table.
// PORT: FUN_801d26cc (species lookup: spawn_table[lure*8 + band])
pub fn spawn_species(table: &[[u32; 8]], lure: u32, band: u32) -> Option<usize> {
    let id = *table.get(lure as usize)?.get(band as usize)? as usize;
    (id < legaia_minigame_formats::fishing_species::SPECIES_COUNT).then_some(id)
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

/// The hooked fish's behaviour sub-state (`DAT_801d910c`), re-rolled when its
/// countdown (`DAT_801d9110`) expires. The discriminant is retail's state
/// number: the jump table at `0x801CF028` sends `0..=3` to the four arms of
/// `FUN_801D4004`. Its fifth target (`0x801D43C8`) is unreachable: no store
/// to `DAT_801d910c` in the image writes `4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishMove {
    /// State `0` (`0x801D4154`): the line sinks by the species sink factor,
    /// the fish swims off at `pull / 32`, and the reel load is `4 * pull/16`.
    Sink = 0,
    /// State `1` (`0x801D41FC`): the hard run - swim `pull / 4`, reel load
    /// `52 * pull/16`, and the fish tires (the depth ceiling falls `2` a
    /// frame). Drops back to [`FishMove::Sink`] once the line is reeled
    /// above depth `0x258`.
    Run = 1,
    /// State `2` (`0x801D42BC`): a side dart, angle offset `0xC40`.
    DartLeft = 2,
    /// State `3` (`0x801D4300`): the other side dart, angle offset `0x440`.
    DartRight = 3,
}

impl FishMove {
    fn from_state(state: i32) -> Self {
        match state & 3 {
            0 => FishMove::Sink,
            1 => FishMove::Run,
            2 => FishMove::DartLeft,
            _ => FishMove::DartRight,
        }
    }
}

/// The depth ceiling `DAT_801D929C` the hook arm seeds (`li v0,0x2000` /
/// `sw` at `0x801D37AC` / `0x801D37B0`); the line depth is held at half of it.
pub const FIGHT_DEPTH_CEILING: i32 = 0x2000;
/// The line depth the hook arm seeds (`li v0,0x1000` / `sw` at `0x801D37A0` /
/// `0x801D37A4`).
pub const FIGHT_DEPTH_SEED: i32 = 0x1000;
/// The per-frame depth rise at the head of `FUN_801D4004` (`addiu v0,v0,0x14`
/// at `0x801D4094`); not scaled by the frame step.
pub const FIGHT_DEPTH_RISE: i32 = 0x14;
/// The surge term `DAT_801D90F4`'s ceiling (`slti v0,v0,0x3C1` at `0x801D4450`).
pub const FIGHT_SURGE_MAX: i32 = 0x3C0;

/// One frame of fish output from [`FishAi::tick`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishFrame {
    /// This frame's pull `DAT_801D90FC`: `((rand & 0xff) + bias) * f08 / 150`.
    pub pull: i32,
    /// The reel load: `(k * (pull / 16) * depth + 0x200) >> 12`, the value
    /// the reel arms divide by `3` and the rod divisor
    /// ([`TensionGauge::apply_load`]).
    pub load: i32,
    /// The swim offset `FUN_801D7BB8` writes to `DAT_801D9100` / `DAT_801D9104`
    /// this frame - `(sin, cos) * radius * 3 * step >> 12` at the arm's angle,
    /// 24.8 - in the fish's own frame (its facing taken as `0`).
    pub swim: (i32, i32),
    /// The surge term `DAT_801D90F4`, `0..=0x3C0`: the lure tick caps the
    /// hooked reel speed at `0x1800 - 2 * surge` (`0x801D2B98..0x801D2BB0`).
    pub surge: i32,
}

/// `FUN_801d4004`, the hooked-fish tick: the pull, the behaviour sub-state
/// and its arm, the line depth, and the reel load the tension gauge takes.
///
/// Retail order per frame: depth `+= 0x14`; the pull off one `rand` and the
/// bias `DAT_801D9108`; the state arm (sink, ceiling decay, the run's exit,
/// the swim offset off a second `rand`, the phase); the surge; the load; the
/// held-reel depth nudges; the clamps; the countdown and the re-roll.
/// [`TensionGauge::apply_load`] is the reel arms' tension half and runs on
/// the load this returns; nothing it does draws `rand` or feeds the re-roll,
/// so the split leaves the stream in retail's order.
#[derive(Debug, Clone, Copy)]
pub struct FishAi {
    /// Current behaviour (`DAT_801d910c`).
    pub state: FishMove,
    /// Frames until the next behaviour re-roll (`DAT_801d9110`).
    pub(super) timer: i32,
    /// The surge phase `DAT_801D9068`, a 12-bit angle into the sine table.
    pub(super) phase: i32,
    /// The depth ceiling `DAT_801D929C`.
    pub(super) ceiling: i32,
}

impl Default for FishAi {
    fn default() -> Self {
        Self::hooked()
    }
}

/// `x / 0xC8000` the way `0x801D442C..0x801D4448` forms it: the reciprocal
/// `0x51EB851F`, `hi >> 18`, minus the sign.
fn div_c8000(x: i32) -> i32 {
    let hi = ((x as i64 * 0x51EB_851F_i64) >> 32) as i32;
    (hi >> 18) - (x >> 31)
}

impl FishAi {
    /// The fish as the hook leaves it: state `0` (the landing arm stores it,
    /// `0x801D295C`) and the seeded depth ceiling. The countdown is not
    /// re-seeded by retail; a fresh one expires on the first frame.
    pub fn hooked() -> Self {
        Self {
            state: FishMove::Sink,
            timer: 0,
            phase: 0,
            ceiling: FIGHT_DEPTH_CEILING,
        }
    }

    /// Advance one hooked frame.
    ///
    /// `bias` is `DAT_801D9108` (the hook's strength `+ 0x400`,
    /// [`PondSession::fish_scale`]), `depth` the line depth `DAT_801D9298`,
    /// `record` the line record `DAT_801D927C`, `reel_mask` the held pad word
    /// `_DAT_8007B850` (bits `0x40` / `0x80` each run their arm: with both
    /// held retail nudges the depth twice).
    // PORT: FUN_801d4004 (pull, behaviour arms, depth, reel load, re-roll)
    #[allow(clippy::too_many_arguments)] // the retail tick reads exactly these globals
    pub fn tick(
        &mut self,
        sp: &FishingSpecies,
        bias: i32,
        depth: &mut i32,
        record: i32,
        reel_mask: u32,
        rng: &mut BiosRand,
        frame_step: i32,
    ) -> FishFrame {
        let fs = frame_step.max(1);
        *depth = depth.wrapping_add(FIGHT_DEPTH_RISE);
        let pull = ((rng.next_u15() & 0xff) as i32 + bias).wrapping_mul(sp.pull_factor) / 150;
        let s2 = pull / 16;

        // The arm: (radius, angle offset, phase step, reel-load factor,
        // surge factor s4, held-reel depth nudge s5).
        let (radius, angle_k, phase_step, k, s4, s5) = match self.state {
            FishMove::Sink => {
                *depth = depth.wrapping_add(pull.wrapping_mul(sp.sink_factor) / 150);
                (pull / 32, 0x840, fs << 4, 4, 0xF0, 0)
            }
            FishMove::Run => {
                self.ceiling -= fs * 2;
                if *depth < 0x258 {
                    self.state = FishMove::Sink;
                }
                (pull / 4, 0x840, fs << 5, 52, 0x1E0, -0x4A)
            }
            FishMove::DartLeft | FishMove::DartRight => {
                self.ceiling -= fs;
                let angle_k = if self.state == FishMove::DartLeft {
                    0xC40
                } else {
                    0x440
                };
                let radius = (pull / 4 + 0x20).wrapping_mul(sp.dart_factor) / 100;
                (radius, angle_k, fs << 4, 32, 0x17C, -0x32)
            }
        };
        // `FUN_801D7BB8(facing - ((rand & 0x7f) - K), radius, .., step * 3)`.
        let angle = (angle_k - (rng.next_u15() & 0x7f) as i32) as u16;
        let scale = fs * 3;
        let polar = |t: i32| (t.wrapping_mul(radius).wrapping_mul(scale)) >> 12;
        let swim = (
            polar(legaia_render_kernels::gte::psx_sin(angle)),
            polar(legaia_render_kernels::gte::psx_cos(angle)),
        );
        self.phase = self.phase.wrapping_add(phase_step);

        let wave = legaia_render_kernels::gte::psx_sin((self.phase & 0xFFF) as u16) + 0x1000;
        let surge =
            div_c8000(wave.wrapping_mul(s4).wrapping_mul(sp.pull_factor)).min(FIGHT_SURGE_MAX);

        let v = (k * s2).wrapping_mul(*depth).wrapping_add(0x200);
        let load = if v >= 0 { v >> 12 } else { (v + 0xFFF) >> 12 };

        if reel_mask & REEL_A_PAD_BIT != 0 {
            *depth += s5;
        }
        if reel_mask & REEL_B_PAD_BIT != 0 {
            *depth += s5;
        }
        self.ceiling = self.ceiling.max(0);
        *depth = (*depth).max(0).min(self.ceiling / 2);

        self.timer -= fs;
        self.reroll(sp, *depth, record, rng);
        FishFrame {
            pull,
            load,
            swim,
            surge,
        }
    }

    /// The countdown's tail (`0x801D46A0..0x801D4920`): the forced run near
    /// the rod, then - on an expired countdown - the re-roll and its reload.
    fn reroll(&mut self, sp: &FishingSpecies, depth: i32, record: i32, rng: &mut BiosRand) {
        let mut forced = false;
        if record < sp.strike_gate + 300
            && depth > 0x400
            && ((rng.next_u15() & 0xFFF) as i32) < sp.roll_cutoff_c
        {
            self.timer = -1;
            forced = true;
        }
        if self.timer >= 0 {
            return;
        }
        let mut state = (rng.next_u15() & 3) as i32;
        let mut sink_pick = false;
        if sp.depth_gate >= depth && ((rng.next_u15() & 0xFFF) as i32) < sp.roll_cutoff_a {
            state = 0;
            sink_pick = true;
        }
        if !sink_pick {
            if ((rng.next_u15() & 0xFFF) as i32) < sp.roll_cutoff_b {
                state = 1;
            } else if (rng.next_u15() & 0x1FF) < 0x190 && record >= 0x579 {
                state = 2 + (rng.next_u15() & 1) as i32;
            }
        }
        if record >= 0x709 {
            state = 2 + (rng.next_u15() & 1) as i32;
        }
        if forced {
            state = 1;
        }
        self.state = FishMove::from_state(state);
        self.timer = match self.state {
            FishMove::Sink => (rng.next_u15() % 20) as i32 + 20,
            FishMove::Run => (rng.next_u15() % 100) as i32 + 120,
            FishMove::DartLeft | FishMove::DartRight => (rng.next_u15() % 60) as i32 + 100,
        };
    }
}
