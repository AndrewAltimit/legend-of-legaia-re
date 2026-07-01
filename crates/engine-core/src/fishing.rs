//! Clean-room **fishing minigame** rules engine.
//!
//! A port of the confirmed numeric kernels of the fishing overlay (PROT 0972,
//! `data\OTHER1`) - the casting-power oscillator, the tension-gauge tug-of-war,
//! and the catch-scoring / persistent-record model - composed into an
//! interactive fight session. It consumes reel input and a per-frame fish pull
//! and produces a running fight + a scored catch, driven by the already-parsed
//! per-species table ([`legaia_asset::fishing_species`]).
//!
//! What is **Confirmed** (byte / formula pinned in
//! [`docs/subsystems/minigame-fishing.md`](../../../docs/subsystems/minigame-fishing.md)):
//! - the casting-power bounds `0x20..=0x1000` and its `0x40` seed (states `0x14`
//!   / `0xa`);
//! - the tension-gauge update: reel-held divisors `rod*9 + 0x23` (button `0x40`)
//!   / `rod*6 + 0x19` (button `0x80`), reel-released decrement
//!   `(rod*0x40 + 0x4a) * frame_step`, and the `[0, 0x1000]` clamp
//!   (`FUN_801d4004` tail);
//! - the catch award `value * (strength + 0x9c0) / 0x32000`, the `999999`
//!   persistent-point cap, and the best-catch (value + fish id) update
//!   (`FUN_801d5298`); the award itself is [`FishingSpecies::score_for`].
//!
//! What is an **engine-side reconstruction** (the retail win/lose conditions are
//! in this module's [Open](../../../docs/subsystems/minigame-fishing.md#open)
//! list - the exact reel-button bit assignment and the land/snap thresholds are
//! not pinned from the dumps): the [`FishingSession`] flow ties the
//! confirmed kernels together with a line-snaps-at-max-tension loss and a
//! reel-progress land, so the minigame is playable. Those glue rules are marked
//! at their call sites; every numeric kernel above is the confirmed one. No Sony
//! bytes are baked in - the species values decode from the user's disc.
//!
//! Chain: retail `FUN_801cf3bc` (mode SM) -> `FUN_801d4004` (fish-AI + tension)
//! -> `FUN_801d5298` (catch scoring).

use legaia_asset::fishing_species::FishingSpecies;

/// Tension-gauge ceiling (`FUN_801d4004`: clamp high at `0x1000`).
pub const TENSION_MAX: i32 = 0x1000;
/// Tension-gauge floor (`FUN_801d4004`: clamp low at `0`).
pub const TENSION_MIN: i32 = 0;

/// Casting-power oscillator low bound (`FUN_801cf3bc` state `0x14`).
pub const CAST_POWER_MIN: i32 = 0x20;
/// Casting-power oscillator high bound (`FUN_801cf3bc` state `0x14`).
pub const CAST_POWER_MAX: i32 = 0x1000;
/// Casting-power seed at run-loop init (`FUN_801cf3bc` state `0xa`).
pub const CAST_POWER_SEED: i32 = 0x40;

/// Persistent fishing-point cap (`FUN_801d5298`: `_DAT_8008444c` clamped to
/// `999999`).
pub const FISH_POINTS_CAP: i32 = 999_999;

/// Reel-held tension divisor for the `0x40` reel button: `rod*9 + 0x23`.
pub const REEL_A_DIV_MUL: i32 = 9;
/// Additive term of the `0x40`-button reel divisor.
pub const REEL_A_DIV_ADD: i32 = 0x23;
/// Reel-held tension divisor for the `0x80` reel button: `rod*6 + 0x19`.
pub const REEL_B_DIV_MUL: i32 = 6;
/// Additive term of the `0x80`-button reel divisor.
pub const REEL_B_DIV_ADD: i32 = 0x19;
/// Reel-released tension decrement multiplier: `(rod*0x40 + 0x4a) * frame_step`.
pub const REEL_RELEASE_MUL: i32 = 0x40;
/// Additive term of the reel-released decrement.
pub const REEL_RELEASE_ADD: i32 = 0x4a;

/// The reel-input state this frame. The retail held mask is `_DAT_8007b850`
/// bits `0x40` / `0x80`; which physical buttons those are is not pinned (see the
/// minigame doc's Open list), so this stays at the two-reel-button level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReelInput {
    /// Neither reel button held - tension bleeds off.
    Idle,
    /// The `0x40` reel button (the `rod*9 + 0x23`-divisor path).
    ReelA,
    /// The `0x80` reel button (the `rod*6 + 0x19`-divisor path).
    ReelB,
}

/// The casting-power oscillator (`FUN_801cf3bc` state `0x14`): a value that
/// bounces between [`CAST_POWER_MIN`] and [`CAST_POWER_MAX`] until the player
/// locks it, setting the cast distance. The per-frame `step` magnitude is not
/// byte-pinned in the dumps, so it is a caller parameter (the retail meter
/// visibly sweeps the full range in well under a second).
#[derive(Debug, Clone, Copy)]
pub struct CastPower {
    power: i32,
    /// Oscillation direction (`DAT_801d9278`, `+1` / `-1`).
    dir: i32,
    locked: bool,
}

impl Default for CastPower {
    fn default() -> Self {
        Self::new()
    }
}

impl CastPower {
    /// A fresh oscillator seeded at [`CAST_POWER_SEED`], sweeping upward.
    pub fn new() -> Self {
        Self {
            power: CAST_POWER_SEED,
            dir: 1,
            locked: false,
        }
    }

    /// Current meter value.
    pub fn value(&self) -> i32 {
        self.power
    }

    /// `true` once [`Self::lock`] has fixed the meter.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Advance the meter by `step`, bouncing off the `[0x20, 0x1000]` bounds and
    /// flipping direction. No-op once locked.
    // PORT: FUN_801cf3bc state 0x14 (casting-power oscillator + direction flip)
    pub fn advance(&mut self, step: i32) {
        if self.locked {
            return;
        }
        let step = step.max(1);
        let mut p = self.power + self.dir * step;
        if p >= CAST_POWER_MAX {
            p = CAST_POWER_MAX;
            self.dir = -1;
        } else if p <= CAST_POWER_MIN {
            p = CAST_POWER_MIN;
            self.dir = 1;
        }
        self.power = p;
    }

    /// Lock the meter at its current value and return it (the cast distance).
    pub fn lock(&mut self) -> i32 {
        self.locked = true;
        self.power
    }
}

/// The tension gauge (`DAT_801d9168`): a `[0, 0x1000]` tug-of-war raised by
/// reeling and bled off when the reel is released. `rod_stat` is the persistent
/// rod / upgrade stat (`_DAT_80084454`); a higher value softens both the
/// reel-in spike and the bleed-off.
#[derive(Debug, Clone, Copy)]
pub struct TensionGauge {
    tension: i32,
    rod_stat: i32,
}

impl TensionGauge {
    /// A slack gauge for a rod of the given persistent stat.
    pub fn new(rod_stat: i32) -> Self {
        Self {
            tension: 0,
            rod_stat: rod_stat.max(0),
        }
    }

    /// Current tension, `0..=0x1000`.
    pub fn tension(&self) -> i32 {
        self.tension
    }

    /// `true` when tension is pinned at [`TENSION_MAX`] (the line-snap edge).
    pub fn at_max(&self) -> bool {
        self.tension >= TENSION_MAX
    }

    /// Apply one frame of reel input against a fish pulling with `base_pull`,
    /// scaled by the frame step `frame_step` (`DAT_1f800393`), then clamp.
    ///
    /// Confirmed (`FUN_801d4004` tail): the reel-held divisors
    /// (`rod*9 + 0x23` / `rod*6 + 0x19`) and the reel-released decrement
    /// `(rod*0x40 + 0x4a) * frame_step`, and the `[0, 0x1000]` clamp. The
    /// held-path grouping `base_pull * frame_step / divisor` is the natural
    /// integer reading (a stronger fish pull spikes tension faster); the exact
    /// MIPS operand order of the held term is not separately pinned.
    // PORT: FUN_801d4004 (tension-gauge integration, reel held / released)
    pub fn apply_reel(&mut self, input: ReelInput, base_pull: i32, frame_step: i32) {
        let fs = frame_step.max(1);
        let delta = match input {
            ReelInput::ReelA => {
                let div = (self.rod_stat * REEL_A_DIV_MUL + REEL_A_DIV_ADD).max(1);
                base_pull.max(0) * fs / div
            }
            ReelInput::ReelB => {
                let div = (self.rod_stat * REEL_B_DIV_MUL + REEL_B_DIV_ADD).max(1);
                base_pull.max(0) * fs / div
            }
            ReelInput::Idle => -((self.rod_stat * REEL_RELEASE_MUL + REEL_RELEASE_ADD) * fs),
        };
        self.tension = (self.tension + delta).clamp(TENSION_MIN, TENSION_MAX);
    }
}

/// The persistent fishing record (`_DAT_8008444c` / `_DAT_80084458` /
/// `_DAT_8008445c`): the running point total and the best single catch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingRecord {
    /// Accumulated fishing points, capped at [`FISH_POINTS_CAP`].
    pub points: i32,
    /// Best single-catch point value seen.
    pub best_points: i32,
    /// Fish id of the best catch.
    pub best_fish: usize,
}

impl FishingRecord {
    /// Credit a landed catch worth `award` points from species `fish_id`
    /// (`FUN_801d5298`): add to the capped point total and, if it beats the
    /// current best, update the best value + fish id. Returns the awarded
    /// points (post-cap contribution is not clamped away from the return - the
    /// caller sees the raw award).
    // PORT: FUN_801d5298 (persistent point credit + best-catch update)
    pub fn credit(&mut self, fish_id: usize, award: i32) {
        let award = award.max(0);
        self.points = (self.points + award).min(FISH_POINTS_CAP);
        if award > self.best_points {
            self.best_points = award;
            self.best_fish = fish_id;
        }
    }
}

/// The outcome of a fishing fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FightOutcome {
    /// The fish is still on the line.
    Fighting,
    /// The fish was landed for `points` (already credited to the record).
    Landed { points: i32 },
    /// The line snapped (tension hit max) - no catch.
    Snapped,
}

/// A live fishing fight against one hooked species. Composes the confirmed
/// [`TensionGauge`] + catch scoring with an engine-side land/snap loop so the
/// minigame is playable.
///
/// The land/snap rules are the module's reconstruction (see the module docs):
/// the line **snaps** the frame tension reaches [`TENSION_MAX`], and the fish is
/// **landed** once accumulated reel progress reaches the fish's strike gate
/// (`+0x24`, `record < f + 300` in `FUN_801d4004`) - reusing a confirmed
/// per-species field as the fight length. The scored `strength` is the
/// confirmed `DAT_801d91b8` accumulator that feeds `FUN_801d5298`.
#[derive(Debug, Clone)]
pub struct FishingFight {
    species: FishingSpecies,
    gauge: TensionGauge,
    /// Accumulated fight strength (`DAT_801d91b8`) - grows as the fish is worked;
    /// feeds the score award.
    strength: i32,
    /// Accumulated reel progress toward landing.
    progress: i32,
    outcome: FightOutcome,
}

impl FishingFight {
    /// Begin a fight against `species` with a rod of persistent stat `rod_stat`.
    pub fn new(species: FishingSpecies, rod_stat: i32) -> Self {
        Self {
            species,
            gauge: TensionGauge::new(rod_stat),
            strength: 0,
            progress: 0,
            outcome: FightOutcome::Fighting,
        }
    }

    /// Live tension, `0..=0x1000`.
    pub fn tension(&self) -> i32 {
        self.gauge.tension()
    }

    /// Accumulated fight strength (the value that feeds the score award).
    pub fn strength(&self) -> i32 {
        self.strength
    }

    /// The hooked species.
    pub fn species(&self) -> &FishingSpecies {
        &self.species
    }

    /// The current fight outcome.
    pub fn outcome(&self) -> FightOutcome {
        self.outcome
    }

    /// The strike-gate target that reel progress must reach to land the fish
    /// (`+0x24 + 300`, the confirmed `record < f + 300` hook check).
    pub fn land_target(&self) -> i32 {
        self.species.strike_gate + 300
    }

    /// Advance one fight frame: the fish pulls with `base_pull` (raising fight
    /// strength), the player reels (or not), and the tension + progress update.
    /// Returns the (possibly terminal) outcome.
    ///
    /// - Confirmed: the tension update ([`TensionGauge::apply_reel`]) and the
    ///   score award on landing ([`FishingSpecies::score_for`], credited via
    ///   [`FishingRecord::credit`]).
    /// - Reconstruction: reeling adds to `progress` and to `strength`; the line
    ///   snaps at max tension; the fish lands when `progress >= land_target()`.
    pub fn tick(
        &mut self,
        input: ReelInput,
        base_pull: i32,
        frame_step: i32,
        record: &mut FishingRecord,
    ) -> FightOutcome {
        if self.outcome != FightOutcome::Fighting {
            return self.outcome;
        }
        self.gauge.apply_reel(input, base_pull, frame_step);
        // Working the fish (reeling) accrues fight strength + landing progress;
        // a stronger pull banks more strength (a better score) but risks tension.
        if input != ReelInput::Idle {
            self.strength = self.strength.saturating_add(base_pull.max(0));
            self.progress = self.progress.saturating_add(frame_step.max(1));
        }
        // Line snap: tension pinned at the ceiling loses the fish.
        if self.gauge.at_max() {
            self.outcome = FightOutcome::Snapped;
            return self.outcome;
        }
        // Land: reel progress met the strike gate.
        if self.progress >= self.land_target() {
            let points = self.species.score_for(self.strength);
            record.credit(self.species.index, points);
            self.outcome = FightOutcome::Landed { points };
        }
        self.outcome
    }
}

/// Which phase of a fishing session is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishingPhase {
    /// Casting: the power meter oscillates until the player locks it.
    Casting,
    /// Fighting a hooked fish with the reel.
    Fighting,
    /// The last fight resolved (landed or snapped); the player can recast.
    Done,
}

/// A full fishing session: the cast-power meter, the current fight, and the
/// persistent record, sequenced cast -> fight -> done -> recast. This is the
/// host-facing composition (`FUN_801cf3bc` mode SM in miniature); it holds the
/// parsed per-species table and drives the confirmed kernels.
///
/// Two glue rules are the module's reconstruction (documented at their sites):
/// the locked cast power selects which species hooks (a longer cast reaches
/// rarer fish), and the hooked fish exerts a steady per-frame `base_pull`
/// derived from its `pull_factor` (retail rolls it against `rand`; this keeps
/// the wired minigame deterministic - see the doc's Open list).
#[derive(Debug, Clone)]
pub struct FishingSession {
    species: Vec<FishingSpecies>,
    rod_stat: i32,
    record: FishingRecord,
    cast: CastPower,
    fight: Option<FishingFight>,
    phase: FishingPhase,
    last_outcome: Option<FightOutcome>,
}

impl FishingSession {
    /// Start a session over the parsed species table with a rod of persistent
    /// stat `rod_stat` and an existing point `record`. Begins in [`Casting`].
    ///
    /// [`Casting`]: FishingPhase::Casting
    pub fn new(species: Vec<FishingSpecies>, rod_stat: i32, record: FishingRecord) -> Self {
        Self {
            species,
            rod_stat: rod_stat.max(0),
            record,
            cast: CastPower::new(),
            fight: None,
            phase: FishingPhase::Casting,
            last_outcome: None,
        }
    }

    /// The current phase.
    pub fn phase(&self) -> FishingPhase {
        self.phase
    }

    /// The persistent record (points + best catch).
    pub fn record(&self) -> FishingRecord {
        self.record
    }

    /// The live cast-power meter value.
    pub fn cast_power(&self) -> i32 {
        self.cast.value()
    }

    /// The live fight, if one is in progress.
    pub fn fight(&self) -> Option<&FishingFight> {
        self.fight.as_ref()
    }

    /// The most recent resolved fight outcome (set on entering [`Done`]).
    ///
    /// [`Done`]: FishingPhase::Done
    pub fn last_outcome(&self) -> Option<FightOutcome> {
        self.last_outcome
    }

    /// Advance the cast-power oscillator by `step`. No-op outside the casting
    /// phase.
    pub fn advance_cast(&mut self, step: i32) {
        if self.phase == FishingPhase::Casting {
            self.cast.advance(step);
        }
    }

    /// Lock the cast and hook a fish, entering the fight. The locked power picks
    /// the species: a longer cast reaches a rarer (higher-index) fish
    /// (reconstruction). No-op outside casting or with an empty table.
    pub fn lock_cast(&mut self) {
        if self.phase != FishingPhase::Casting || self.species.is_empty() {
            return;
        }
        let power = self.cast.lock();
        let span = (CAST_POWER_MAX - CAST_POWER_MIN).max(1);
        let idx = (((power - CAST_POWER_MIN).max(0) as i64 * self.species.len() as i64)
            / span as i64) as usize;
        let idx = idx.min(self.species.len() - 1);
        self.fight = Some(FishingFight::new(self.species[idx], self.rod_stat));
        self.phase = FishingPhase::Fighting;
    }

    /// The steady per-frame pull the hooked fish exerts (`pull_factor` scaled
    /// down; reconstruction - retail rolls it against `rand`). `0` when not
    /// fighting.
    pub fn fish_pull(&self) -> i32 {
        self.fight
            .as_ref()
            .map(|f| (f.species().pull_factor / 8).max(1))
            .unwrap_or(0)
    }

    /// Apply one fight frame with the given reel input. On a terminal outcome
    /// the session moves to [`Done`] and records [`Self::last_outcome`]. No-op
    /// outside the fighting phase.
    ///
    /// [`Done`]: FishingPhase::Done
    pub fn reel(&mut self, input: ReelInput, frame_step: i32) {
        if self.phase != FishingPhase::Fighting {
            return;
        }
        let base_pull = self.fish_pull();
        let mut record = self.record;
        let outcome = match self.fight.as_mut() {
            Some(f) => f.tick(input, base_pull, frame_step, &mut record),
            None => return,
        };
        self.record = record;
        if outcome != FightOutcome::Fighting {
            self.last_outcome = Some(outcome);
            self.phase = FishingPhase::Done;
        }
    }

    /// Recast after a resolved fight: reset the cast meter and clear the fight.
    /// No-op unless in [`Done`].
    ///
    /// [`Done`]: FishingPhase::Done
    pub fn recast(&mut self) {
        if self.phase == FishingPhase::Done {
            self.cast = CastPower::new();
            self.fight = None;
            self.phase = FishingPhase::Casting;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn species(index: usize, score_value: i32, strike_gate: i32) -> FishingSpecies {
        FishingSpecies {
            index,
            name_ptr_va: 0,
            score_value,
            pull_factor: 250,
            dart_factor: 60,
            sink_factor: 4,
            depth_gate: 1024,
            roll_cutoff_a: 200,
            roll_cutoff_b: 512,
            roll_cutoff_c: 90,
            strike_gate,
        }
    }

    #[test]
    fn cast_power_oscillates_within_bounds_and_locks() {
        let mut c = CastPower::new();
        assert_eq!(c.value(), CAST_POWER_SEED);
        // Sweep up to the ceiling and confirm it bounces back down.
        for _ in 0..200 {
            c.advance(0x40);
        }
        assert!(c.value() >= CAST_POWER_MIN && c.value() <= CAST_POWER_MAX);
        let locked = c.lock();
        assert!(c.is_locked());
        assert_eq!(locked, c.value());
        // Locked meter no longer moves.
        c.advance(0x40);
        assert_eq!(c.value(), locked);
    }

    #[test]
    fn cast_power_bounces_at_ceiling() {
        let mut c = CastPower::new();
        // Big step jumps straight to the ceiling and flips direction.
        c.advance(CAST_POWER_MAX);
        assert_eq!(c.value(), CAST_POWER_MAX);
        c.advance(0x40);
        assert!(
            c.value() < CAST_POWER_MAX,
            "direction flipped downward at ceiling"
        );
    }

    #[test]
    fn tension_rises_on_reel_and_bleeds_when_idle() {
        let mut g = TensionGauge::new(0);
        // Reel button A with a base pull raises tension (rod_stat 0 -> div 0x23).
        g.apply_reel(ReelInput::ReelA, 0x1000, 1);
        let after_reel = g.tension();
        assert!(after_reel > 0, "reeling raised tension");
        // Idle bleeds it off by (0*0x40 + 0x4a) * 1 = 0x4a per frame.
        g.apply_reel(ReelInput::Idle, 0, 1);
        assert_eq!(g.tension(), (after_reel - REEL_RELEASE_ADD).max(0));
    }

    #[test]
    fn tension_clamps_at_bounds() {
        let mut g = TensionGauge::new(0);
        // Huge reel spike pins at the ceiling.
        g.apply_reel(ReelInput::ReelA, i32::MAX / 2, 1);
        assert_eq!(g.tension(), TENSION_MAX);
        assert!(g.at_max());
        // Idle can't drive below zero.
        for _ in 0..1000 {
            g.apply_reel(ReelInput::Idle, 0, 1);
        }
        assert_eq!(g.tension(), TENSION_MIN);
    }

    #[test]
    fn rod_stat_softens_the_reel_spike() {
        let mut weak = TensionGauge::new(0);
        let mut strong = TensionGauge::new(10);
        weak.apply_reel(ReelInput::ReelA, 0x1000, 1);
        strong.apply_reel(ReelInput::ReelA, 0x1000, 1);
        assert!(
            strong.tension() < weak.tension(),
            "a higher rod stat divides the tension spike down"
        );
    }

    #[test]
    fn record_credit_caps_and_tracks_best() {
        let mut r = FishingRecord::default();
        r.credit(3, 100);
        assert_eq!(r.points, 100);
        assert_eq!((r.best_points, r.best_fish), (100, 3));
        // A smaller catch adds points but doesn't beat the best.
        r.credit(1, 40);
        assert_eq!(r.points, 140);
        assert_eq!((r.best_points, r.best_fish), (100, 3));
        // A bigger catch takes the best.
        r.credit(7, 250);
        assert_eq!((r.best_points, r.best_fish), (250, 7));
        // Points cap at 999999.
        r.credit(0, FISH_POINTS_CAP);
        assert_eq!(r.points, FISH_POINTS_CAP);
    }

    #[test]
    fn fight_lands_a_fish_and_scores_it() {
        let mut record = FishingRecord::default();
        // Small strike gate so a few gentle reels land it without snapping.
        let mut fight = FishingFight::new(species(2, 10_000, 10), 8);
        let target = fight.land_target();
        assert_eq!(target, 10 + 300);
        // Reel with a modest pull (rod stat 8 softens tension) until landed.
        let mut outcome = FightOutcome::Fighting;
        for _ in 0..1000 {
            outcome = fight.tick(ReelInput::ReelA, 4, 4, &mut record);
            if outcome != FightOutcome::Fighting {
                break;
            }
        }
        match outcome {
            FightOutcome::Landed { points } => {
                assert!(points > 0);
                assert_eq!(record.points, points);
                assert_eq!(record.best_fish, 2);
            }
            other => panic!("expected a landed catch, got {other:?}"),
        }
    }

    #[test]
    fn session_sequences_cast_fight_and_recast() {
        let table = vec![
            species(0, 8_000, 8),
            species(1, 12_000, 8),
            species(2, 20_000, 8),
        ];
        let mut s = FishingSession::new(table, 8, FishingRecord::default());
        assert_eq!(s.phase(), FishingPhase::Casting);
        // Oscillate the meter, then lock -> a fish hooks and the fight starts.
        for _ in 0..5 {
            s.advance_cast(0x40);
        }
        s.lock_cast();
        assert_eq!(s.phase(), FishingPhase::Fighting);
        assert!(s.fight().is_some());
        assert!(s.fish_pull() > 0);
        // Reel until the fight resolves.
        for _ in 0..2000 {
            if s.phase() != FishingPhase::Fighting {
                break;
            }
            s.reel(ReelInput::ReelA, 4);
        }
        assert_eq!(s.phase(), FishingPhase::Done);
        assert!(s.last_outcome().is_some());
        // Recast returns to a fresh casting meter.
        s.recast();
        assert_eq!(s.phase(), FishingPhase::Casting);
        assert_eq!(s.cast_power(), CAST_POWER_SEED);
    }

    #[test]
    fn locked_cast_power_selects_a_species() {
        let table = vec![
            species(0, 8_000, 8),
            species(1, 12_000, 8),
            species(2, 20_000, 8),
        ];
        // A max-power cast reaches the rarest (last) fish.
        let mut s = FishingSession::new(table.clone(), 8, FishingRecord::default());
        s.advance_cast(CAST_POWER_MAX); // jump to the ceiling
        s.lock_cast();
        assert_eq!(s.fight().unwrap().species().index, table.len() - 1);
    }

    #[test]
    fn fight_snaps_the_line_at_max_tension() {
        let mut record = FishingRecord::default();
        // Huge pull + weak rod -> tension pins immediately -> snap.
        let mut fight = FishingFight::new(species(5, 20_000, 10_000), 0);
        let outcome = fight.tick(ReelInput::ReelA, i32::MAX / 2, 1, &mut record);
        assert_eq!(outcome, FightOutcome::Snapped);
        // A snap scores nothing.
        assert_eq!(record.points, 0);
        // The fight is terminal - further ticks stay snapped.
        assert_eq!(
            fight.tick(ReelInput::ReelA, 4, 4, &mut record),
            FightOutcome::Snapped
        );
    }
}
