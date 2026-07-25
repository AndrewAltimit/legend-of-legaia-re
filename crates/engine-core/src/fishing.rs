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
//!
//! # Scope
//!
//! This module is the **rules** half only: [`FishingSession`] and the kernels
//! it drives ([`CastPower`], [`TensionGauge`], [`FishingRecord`],
//! [`PrizeExchange`]) are called from `world`'s minigame dispatch, which is
//! how the fishing minigame runs.
//!
//! The **presentation** half - the persistent / catch HUD layout, the gauge
//! bars, the digit field and the banner animators - lives in
//! `legaia_engine_ui::ui_fishing`, next to the consumer that renders it, in
//! line with the project's split between simulation (this crate) and
//! renderer-agnostic draw-list builders (`engine-ui`).

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
/// `999999`). The HUD row clamps to the same literal, one copy per crate -
/// `legaia_engine_ui::ui_fishing::HUD_POINT_CAP`.
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

/// Packed-pad bit of the reel-A button (Cross) in the retail held word
/// `_DAT_8007b850` - the mask [`ReelInput::from_pad_mask`] decodes.
pub const REEL_A_PAD_BIT: u32 = 0x40;
/// Packed-pad bit of the reel-B button (Square) in the retail held word.
/// **Not** Circle - `0x20` is the cast/hook input.
pub const REEL_B_PAD_BIT: u32 = 0x80;

/// The reel-input state this frame. The retail held mask is `_DAT_8007b850`
/// bits `0x40` / `0x80`, which are now pinned to physical buttons via the pad
/// packer `FUN_8001822C`: `0x40` = Cross, `0x80` = Square (reel B is Square,
/// NOT Circle; Circle `0x20` is the cast/hook input). See the fishing doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReelInput {
    /// Neither reel button held - tension bleeds off.
    Idle,
    /// The `0x40` reel button (Cross; the `rod*9 + 0x23`-divisor path).
    ReelA,
    /// The `0x80` reel button (Square; the `rod*6 + 0x19`-divisor path).
    ReelB,
}

impl ReelInput {
    /// Decode the pad held-mask (`_DAT_8007b850`) into a reel input.
    ///
    /// Cross (`0x40`) takes priority and selects reel A; Square (`0x80`)
    /// without Cross selects reel B; anything else is idle. This mirrors the
    /// retail decoder's three-way branch exactly - holding both reel buttons
    /// resolves to reel A, not a blend - so `0x40 -> ReelA`, `0x80 -> ReelB`,
    /// else `Idle`. The retail body is `if (m & 0x40) return 1; else return
    /// (m >> 6) & 2;`, whose `1` / `2` / `0` results map onto these variants.
    ///
    /// Wired: `World::tick_fishing` assembles the two reel bits out of this
    /// frame's held pad and decodes them here, so the priority rule is the
    /// ported one rather than a host `if` chain.
    // PORT: FUN_801d7450 (reel-button decoder)
    pub fn from_pad_mask(mask: u32) -> Self {
        if mask & 0x40 != 0 {
            ReelInput::ReelA
        } else if (mask >> 6) & 2 != 0 {
            ReelInput::ReelB
        } else {
            ReelInput::Idle
        }
    }
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

    /// Accumulated reel progress - the engine's analogue of the retail line
    /// record `DAT_801d927c`, and the value [`Self::land_target`] is the
    /// `record < f + 300` gate on. The catch HUD's length readout reads it.
    pub fn progress(&self) -> i32 {
        self.progress
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
        self.species.strike_gate + RECORD_STRIKE_BASE
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

    /// Overwrite the record's point total. The point exchange spends from the
    /// shared pool while a session is live (retail deducts `_DAT_8008444C`
    /// directly), so the host syncs the on-screen total after a purchase.
    pub fn set_points(&mut self, points: i32) {
        self.record.points = points;
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

// --- Point exchange (prize shop) -------------------------------------------

/// One prize row of the point-exchange screen, decoded from the overlay's
/// per-venue table ([`legaia_asset::fishing_exchange`]) and optionally named
/// from the SCUS item table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrizeRow {
    /// Row index within the venue page (0..6).
    pub row: usize,
    /// Max obtainable count (1 = one-time prize, 99 = repeatable).
    pub limit: u32,
    /// Price in fishing points per unit.
    pub price: u32,
    /// Granted item id (SCUS item-name-table id space).
    pub item_id: u8,
    /// Display name (from the SCUS item table when available).
    pub name: Option<String>,
}

impl PrizeRow {
    /// Whether this is a one-time prize row (latched in the purchased mask).
    pub fn is_one_time(&self) -> bool {
        self.limit == 1
    }
}

/// A purchase committed by [`PrizeExchange::buy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizePurchase {
    /// Granted item id.
    pub item_id: u8,
    /// Units granted.
    pub qty: u32,
    /// Points spent (`price * qty`).
    pub cost: u32,
    /// One-time bit latched into the purchased mask, if any
    /// (`row + venue * 8`).
    pub latched_bit: Option<u32>,
}

/// The fishing point-exchange session: a venue's 6 prize rows plus a cursor,
/// with the retail gating semantics of the exchange sub-screens
/// (`FUN_801d0c3c` list / `FUN_801d092c` quantity / `FUN_801d06c8` confirm /
/// `FUN_801d6f90` availability - see [`legaia_asset::fishing_exchange`]).
///
/// The kernel is pure over the caller's state: the point pool, the persistent
/// purchased bitmask, and the owned count come in per call (the engine keeps
/// them on `World`, mirroring retail's `_DAT_8008444C` / `_DAT_8008446C` /
/// inventory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrizeExchange {
    /// Venue page (0 = Buma, 1 = Vidna); selects the one-time bit block.
    pub venue: usize,
    /// The 6 prize rows.
    pub rows: Vec<PrizeRow>,
    /// List cursor (row index).
    pub cursor: usize,
}

impl PrizeExchange {
    /// Build from the parsed per-venue asset rows, naming each row from the
    /// SCUS item table when one is supplied.
    pub fn from_asset(
        venue: usize,
        rows: &[legaia_asset::fishing_exchange::ExchangeRow],
        names: Option<&legaia_asset::item_names::ItemNameTable>,
    ) -> Self {
        let rows = rows
            .iter()
            .map(|r| PrizeRow {
                row: r.row,
                limit: r.limit,
                price: r.price,
                item_id: r.item_id as u8,
                name: names
                    .and_then(|t| t.name(r.item_id as u8))
                    .map(str::to_owned),
            })
            .collect();
        Self {
            venue: venue.min(1),
            rows,
            cursor: 0,
        }
    }

    /// The one-time bit index for `row` (`row + venue * 8`).
    pub fn purchase_bit(&self, row: usize) -> u32 {
        legaia_asset::fishing_exchange::FishingExchange::purchase_bit(self.venue, row)
    }

    /// The first *visible* row for the current point total: row 0 is hidden
    /// until strictly affordable (`FUN_801d0c3c`'s `(price0 < points) ^ 1`
    /// cursor floor).
    pub fn first_visible(&self, points: i32) -> usize {
        // PORT: FUN_801d0c3c (prize-list cursor floor - row 0 hides until affordable)
        match self.rows.first() {
            Some(r0) if (r0.price as i64) < points as i64 => 0,
            Some(_) => 1,
            None => 0,
        }
    }

    /// Row availability (drawn white vs grey; `FUN_801d6f90`): affordable,
    /// the owned count is not at [`legaia_asset::fishing_exchange::OWNED_CAP`],
    /// and a one-time row is not already latched in `purchased_mask`.
    pub fn is_available(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> bool {
        // PORT: FUN_801d6f90 (row availability: afford + owned-cap + one-time latch)
        let Some(r) = self.rows.get(row) else {
            return false;
        };
        (r.price as i64) <= points as i64
            && owned != legaia_asset::fishing_exchange::OWNED_CAP
            && (purchased_mask >> self.purchase_bit(row)) & 1 == 0
    }

    /// Max purchasable quantity for `row` (`FUN_801d092c`):
    /// `min(points / price, limit - owned)`, where a not-yet-latched one-time
    /// row treats `owned` as 0. Zero when unaffordable or at the limit.
    pub fn max_qty(&self, row: usize, points: i32, owned: u32, purchased_mask: u32) -> u32 {
        // PORT: FUN_801d092c (quantity picker cap: min(points/price, limit - owned))
        let Some(r) = self.rows.get(row) else {
            return 0;
        };
        if r.price == 0 {
            return 0;
        }
        let owned = if (purchased_mask >> self.purchase_bit(row)) & 1 == 0 && r.limit == 1 {
            0
        } else {
            owned
        };
        let by_points = (points.max(0) as u32) / r.price;
        by_points.min(r.limit.saturating_sub(owned))
    }

    /// Commit a purchase of `qty` units of `row` (`FUN_801d06c8`'s Yes arm):
    /// returns the grant + cost + the one-time bit to latch, or `None` when
    /// the row is unavailable or `qty` exceeds [`Self::max_qty`]. The caller
    /// applies the returned deltas (deduct points, OR the latched bit, grant
    /// the item).
    pub fn buy(
        &self,
        row: usize,
        qty: u32,
        points: i32,
        owned: u32,
        purchased_mask: u32,
    ) -> Option<PrizePurchase> {
        // PORT: FUN_801d06c8 (confirm Yes arm: grant + deduct + one-time latch)
        if qty == 0 || !self.is_available(row, points, owned, purchased_mask) {
            return None;
        }
        if qty > self.max_qty(row, points, owned, purchased_mask) {
            return None;
        }
        let r = &self.rows[row];
        Some(PrizePurchase {
            item_id: r.item_id,
            qty,
            cost: r.price * qty,
            latched_bit: r.is_one_time().then(|| self.purchase_bit(row)),
        })
    }
}

// --- rod / lure selection ----------------------------------------------------

/// The line-record base offset shared by the hook check (`FUN_801d4004`:
/// `record < gate + 300`) and the catch-HUD length readout (`FUN_801d1580`:
/// `record - 300`, clamped at zero). The HUD-side copy of the same literal
/// is `legaia_engine_ui::ui_fishing::RECORD_STRIKE_BASE`.
pub const RECORD_STRIKE_BASE: i32 = 300;

/// The inventory item id whose count the persistent HUD shows for the
/// selected rod index (`FUN_801d13f0`: `_DAT_80084450 + 0x9d` - the lure
/// consumable paired with the rod).
pub fn lure_item_id(rod_index: u32) -> u32 {
    0x9d + rod_index
}

/// How many rod / lure kinds the selector cycles through
/// (items `0x9d..=0x9f`, i.e. [`lure_item_id`] over `0..ROD_KINDS`).
pub const ROD_KINDS: u32 = 3;

/// The rod-ownership gate the driver runs before letting a cast start: `false`
/// parks it in the "no rod" state, `true` lets it into the main loop.
///
/// Retail sums the inventory counts of all three lure items and bails when the
/// total is zero; otherwise it *advances the persistent rod index*
/// (`_DAT_80084450`, wrapping at [`ROD_KINDS`]) until it lands on a kind the
/// player actually holds. So the gate is not read-only - selling the selected
/// lure silently re-points the selection at the next owned one, which is why
/// the HUD's rod label can change without the player touching the menu.
///
/// `count_of` supplies the live inventory count for an item id. The sum
/// guarantees termination in retail; the port bounds the scan at
/// [`ROD_KINDS`] anyway so a caller with an out-of-range index cannot hang it.
// PORT: FUN_801d712c (rod-ownership gate + persistent rod-index re-point)
// PARTLY WIRED: the play window calls this to resolve the rod index its
// persistent HUD rows display. Its other retail role - the rod/lure
// selection screen's cursor handler, which is what lets the player *change*
// the selection - has no host UI, so that path is still unreached.
pub fn select_owned_rod(rod_index: &mut u32, mut count_of: impl FnMut(u32) -> i32) -> bool {
    let owned: i32 = (0..ROD_KINDS).map(|k| count_of(lure_item_id(k))).sum();
    if owned == 0 {
        return false;
    }
    for _ in 0..ROD_KINDS {
        if count_of(lure_item_id(*rod_index)) != 0 {
            return true;
        }
        *rod_index += 1;
        if *rod_index >= ROD_KINDS {
            *rod_index = 0;
        }
    }
    // Unreachable while `owned != 0` and the index is in range; a stale
    // out-of-range index lands here instead of spinning.
    false
}

/// One text line of the fishing help panel: which overlay string-table
/// row to draw, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpPanelLine {
    /// Index into the active page's string-pointer table
    /// (page 0 table at overlay VA `0x801D8130`, page 1 at `0x801D8168`).
    pub string_index: u8,
    /// Screen X (the panel's `x` argument, passed through per line).
    pub x: i16,
    /// Screen Y (`y + 13 * index` - the 13 px line pitch).
    pub y: i16,
}

/// Renderer-agnostic layout of the fishing **help panel** - the
/// two-page line-list screen the fishing overlay draws at `0x801D72A0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpPanelLayout {
    pub lines: Vec<HelpPanelLine>,
    /// Footer line position (retail constants `x = 0xE0`, `y = 0xCA`;
    /// the footer string differs per page: overlay VA `0x801CF048` /
    /// `0x801CF050`).
    pub footer: (i16, i16),
    /// The widget-frame emit that closes the draw
    /// (`FUN_8002C69C(x, y, 0x119, 0xC3)`).
    pub frame: (i16, i16, i16, i16),
}

// NOT WIRED: every line of the panel is a row of the overlay's own
// string-pointer tables (`0x801D8130` / `0x801D8168`), which are Sony text the
// port does not read - there is no fishing string-table reader, and no
// help-screen state on the fishing session for a host to open. A draw wired
// today would place 14 or 15 empty rows. Wiring it needs the overlay string
// tables decoded (or a translation-pack source for them) plus a help-page
// toggle on the session.
/// PORT: overlay_fishing_801d72a0
///
/// Fishing help-panel layout - the static-extract resolution of the VA
/// `0x801D72A0` open case (see `docs/subsystems/minigame-fishing.md`).
/// The fishing overlay's own bytes at that VA (PROT 0972 file `0x8A88`,
/// base `0x801CE818`) are a clean `(x, y, page)` panel renderer:
///
/// - page 0: 14 lines from the string-pointer table at `0x801D8130`;
/// - page != 0: 15 lines from the sibling table at `0x801D8168`;
/// - both: 13 px line pitch, a per-page footer at `(0xE0, 0xCA)`, a
///   widget-frame emit `FUN_8002C69C(x, y, 0x119, 0xC3)`, and the
///   field-subsystem mode byte `DAT_80073F20 = 0x10` stored on entry.
///
/// The line **strings** are overlay bytes (Sony text) and are not
/// modeled; hosts resolve `string_index` against the user's disc.
pub fn help_panel_layout(x: i16, y: i16, second_page: bool) -> HelpPanelLayout {
    let count = if second_page { 15 } else { 14 };
    let lines = (0..count)
        .map(|i| HelpPanelLine {
            string_index: i,
            x,
            y: y + 13 * i as i16,
        })
        .collect();
    HelpPanelLayout {
        lines,
        footer: (0xE0, 0xCA),
        frame: (x, y, 0x119, 0xC3),
    }
}

/// Outcome of one [`FishingMenu::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingMenuTick {
    /// SFX request this frame (`sh id, 0x8007B6D8`): `0x37` cancel,
    /// `0x21` cursor move, `0x20` confirm. `None` when no pad edge hit.
    pub sfx: Option<u16>,
    /// New fishing-SM state (`0x801D926C`) when a transition fired:
    /// cancel -> `0x0A`; confirm row 0..4 -> `0x0A` / `0x65` / `0x6E` /
    /// `0x78` / `0xC8`.
    pub next_state: Option<u32>,
    /// Rows 2 / 3 snapshot the fishing-points bank (`_DAT_80084450`)
    /// into the overlay session global `0x801D90DC` on confirm.
    pub snapshot_points: bool,
    /// Row 4 (leave) clears the scene-load flag `_DAT_8007BC20` and sets
    /// the overlay exit latch `0x801D90CC = 1`.
    pub leave_venue: bool,
}

/// PORT: overlay_fishing_801d0474
///
/// Fishing **main-menu picker** - static extract from PROT 0972 (file
/// `0x1C5C`, base `0x801CE818`). One call per frame:
///
/// - `interactive` (retail `a0 != 0`) gates both the pad handling and
///   the cursor icon; a zero call draws the row text only.
/// - Pad edges (pressed global `0x801D90D8`): `& 0x21` cancel (state
///   `0x0A`, SFX `0x37`); `& 0x1000` up / `& 0x4000` down move the
///   cursor (`0x801D912C`) with SFX `0x21`.
/// - The cursor clamps by **snapping**: `< 0` -> 4, `>= 5` -> 0 (with
///   the ±1 steps that is a 5-row wrap).
/// - Draw: 5 row strings at `x = 0x6C`, `y = 0x58 + 0x10 * row`; the
///   cursor icon (`FUN_8002C488`) at `(0x5B, 0x58 + 0x10 * cursor)`;
///   panel frame via `FUN_801D74B0(0xA0, 0x50, 0x68, 0x50)`.
/// - Confirm (`& 0x44`, SFX `0x20`): jump table over the cursor row ->
///   next SM state (see [`FishingMenuTick::next_state`]); rows 2/3 also
///   snapshot the points bank, row 4 arms the venue exit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingMenu {
    /// Cursor row (`0x801D912C`).
    pub cursor: i32,
}

/// Row text x / first-row y / row pitch, from the draw calls.
pub const FISHING_MENU_ROW_X: i16 = 0x6C;
pub const FISHING_MENU_ROW_Y0: i16 = 0x58;
pub const FISHING_MENU_ROW_PITCH: i16 = 0x10;
/// Confirm-row -> next-state map (jump table at overlay VA `0x801CEF58`).
pub const FISHING_MENU_ROW_STATES: [u32; 5] = [0x0A, 0x65, 0x6E, 0x78, 0xC8];

impl FishingMenu {
    pub fn tick(&mut self, pad_pressed: u16, interactive: bool) -> FishingMenuTick {
        let mut out = FishingMenuTick {
            sfx: None,
            next_state: None,
            snapshot_points: false,
            leave_venue: false,
        };
        if interactive {
            if pad_pressed & 0x21 != 0 {
                out.next_state = Some(0x0A);
                out.sfx = Some(0x37);
            }
            if pad_pressed & 0x1000 != 0 {
                out.sfx = Some(0x21);
                self.cursor -= 1;
            }
            if pad_pressed & 0x4000 != 0 {
                out.sfx = Some(0x21);
                self.cursor += 1;
            }
        }
        // Snap clamp (retail: bgez / slti 5 pair - not a modulo).
        if self.cursor < 0 {
            self.cursor = 4;
        }
        if self.cursor >= 5 {
            self.cursor = 0;
        }
        if interactive && pad_pressed & 0x44 != 0 {
            out.sfx = Some(0x20);
            let row = self.cursor as usize;
            if row < 5 {
                out.next_state = Some(FISHING_MENU_ROW_STATES[row]);
                out.snapshot_points = row == 2 || row == 3;
                out.leave_venue = row == 4;
            }
        }
        out
    }

    /// The cursor icon position for this frame (interactive draws only).
    pub fn cursor_pos(&self) -> (i16, i16) {
        (
            0x5B,
            FISHING_MENU_ROW_Y0 + FISHING_MENU_ROW_PITCH * self.cursor as i16,
        )
    }
}

/// Outcome of one [`RodLureSelect::tick`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodLureSelectTick {
    /// SFX request this frame (`sh id, _DAT_8007B6D8`): `0x20` confirm /
    /// equip, `0x22` cannot equip (selected lure not owned), `0x37` cancel,
    /// `0x21` cursor move. `None` when no pad edge acted.
    pub sfx: Option<u16>,
    /// A lure was equipped: the new persistent lure/label index
    /// (`_DAT_80084450 = cursor`, `cursor < 3`). Its consumable is
    /// [`lure_item_id`]`(index)`.
    pub equip_lure: Option<u32>,
    /// A rod was equipped: the new persistent rod-upgrade stat
    /// (`_DAT_80084454 = slot`, the value that scales the tension change).
    pub equip_rod: Option<i32>,
    /// Cancel / confirm-out: the caller jumps the fishing SM to `100`
    /// (`DAT_801D926C = 100`).
    pub leave: bool,
}

/// PORT: FUN_801d0f5c (rod / lure select screen - the input + equip half)
///
/// The fishing overlay's rod/lure select screen (`overlay_fishing_801d0f5c.txt`,
/// PROT 0972). The retail body is input, equip, **and** the row render; this
/// port models the input/equip kernel only (the row list + owned-count highlight
/// is a host draw concern, like the [`FishingMenu`] split).
///
/// Per frame it first counts the **owned rods** among item ids `0xA0..=0xA2`
/// (`owned_rods`), which bounds the cursor. When `interactive` (retail
/// `param_1 != 0`):
///
/// - **accept** (`pad_edge & 0x44` = Cross `0x40` / L1 `0x04`): a lure row
///   (`cursor < 3`) equips its lure - if the lure item `0x9D + cursor` is owned
///   it writes the persistent lure index `_DAT_80084450 = cursor` (SFX `0x20`),
///   otherwise it refuses with SFX `0x22`. A rod row (`cursor >= 3`) walks the
///   three rod slots and equips the `(cursor - 3)`-th **owned** one, writing the
///   persistent rod stat `_DAT_80084454 = slot` (SFX `0x20`) - so unowned slots
///   are skipped, and the visible rod rows are exactly the owned rods.
/// - **cancel** (`pad_edge & 0x21` = Circle `0x20` / L2 `0x01`): SFX `0x37`,
///   [`leave`](RodLureSelectTick::leave).
/// - **move** (`pad_move & 0x1000` up / `& 0x4000` down): step the cursor -/+1,
///   SFX `0x21`. `pad_move` is the retail `DAT_801D90D8` mask, distinct from the
///   `pad_edge` (`_DAT_8007B874`) accept/cancel mask.
///
/// The cursor **snap-wrap** runs every frame regardless of `interactive`
/// (retail): a cursor past `owned_rods + 2` snaps to `0`, a negative cursor snaps
/// to `owned_rods + 2` - the `3` lure rows plus the owned-rod rows.
///
/// Not wired: the play window has no rod/lure select screen, so this closes the
/// documented gap noted on [`select_owned_rod`] (the read-only rod re-point) with
/// the interactive selection half.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodLureSelect {
    /// Cursor row (`DAT_801D90DC`); `0..3` = the three lure rows, `3..` = the
    /// owned rods.
    pub cursor: i32,
}

impl RodLureSelect {
    pub fn tick(
        &mut self,
        pad_edge: u32,
        pad_move: u32,
        interactive: bool,
        mut count_of: impl FnMut(u32) -> i32,
    ) -> RodLureSelectTick {
        let owned_rods: i32 = (0..ROD_KINDS).filter(|k| count_of(0xa0 + k) != 0).count() as i32;
        let mut out = RodLureSelectTick::default();
        if interactive {
            if pad_edge & 0x44 != 0 {
                if self.cursor < 3 {
                    // Lure row: equip only if the paired lure item is owned.
                    if count_of(0x9d + self.cursor as u32) != 0 {
                        out.sfx = Some(0x20);
                        out.equip_lure = Some(self.cursor as u32);
                    } else {
                        out.sfx = Some(0x22);
                    }
                } else {
                    // Rod row: the (cursor - 3)-th owned rod among slots 0..3.
                    let mut remaining = self.cursor - 3;
                    let mut slot = 0i32;
                    while slot < 3 {
                        if count_of(0xa0 + slot as u32) != 0 {
                            if remaining < 1 {
                                out.sfx = Some(0x20);
                                out.equip_rod = Some(slot);
                                break;
                            }
                            remaining -= 1;
                        }
                        slot += 1;
                    }
                }
            }
            if pad_edge & 0x21 != 0 {
                out.sfx = Some(0x37);
                out.leave = true;
            }
            if pad_move & 0x1000 != 0 {
                out.sfx = Some(0x21);
                self.cursor -= 1;
            }
            if pad_move & 0x4000 != 0 {
                out.sfx = Some(0x21);
                self.cursor += 1;
            }
        }
        // Snap-wrap (retail: the bgez / slt pair, run every frame).
        if owned_rods + 2 < self.cursor {
            self.cursor = 0;
        }
        if self.cursor < 0 {
            self.cursor = owned_rods + 2;
        }
        out
    }
}

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

use legaia_asset::fishing_species::{CADENCE_TOLERANCE, CadenceTemplate};

use crate::levelup::BiosRand;

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
// PORT: FUN_801d26cc (band roll: 0xc00 / 0xe70 / 0xf38 cutoffs)
pub fn band_roll(r: i32) -> u32 {
    let r = r & 0xfff;
    if r <= BAND_ROLL_CUTOFF_3 {
        3
    } else if r <= BAND_ROLL_CUTOFF_2 {
        2
    } else if r <= BAND_ROLL_CUTOFF_1 {
        1
    } else {
        0
    }
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
    templates: Vec<CadenceTemplate>,
    ring: [(u8, i32); 16],
    idx: usize,
    /// Last decoded button (`DAT_801d9064`) - not cleared by the reset.
    last: u8,
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
    pub fn reset(&mut self) {
        self.ring = [(0, 0); 16];
        self.idx = 0;
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
    /// recogniser's match this frame, `edge_bonus` the count of fresh input
    /// edges (D-pad left/right, either reel button), and `reel_held` whether
    /// a reel button is held (`_DAT_8007b850 & 0xc0`). Returns `true` when a
    /// strike lands this frame.
    ///
    /// Pinned: the every-frame re-entry (countdown clamped at 0), the
    /// cadence-match band store + `0x40` hold + splash, the roll cutoffs, the
    /// `credit = countdown + 2 (+ edges)` strike credit, its zeroing under a
    /// `100` readout, the no-strike floor under a `200` readout, and the
    /// reel-held requirement. Approximated: the exact `denom` ladder the
    /// readout steps (`~1000` for a deep cast) - this port uses the readout
    /// itself as the denominator.
    // PORT: FUN_801d26cc (pre-hook band check + strike roll)
    #[allow(clippy::too_many_arguments)] // the retail check reads exactly these globals
    pub fn tick(
        &mut self,
        rng: &mut BiosRand,
        cadence: Option<usize>,
        record: i32,
        readout: i32,
        edge_bonus: i32,
        reel_held: bool,
        frame_step: i32,
    ) -> bool {
        self.splash = false;
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
                }
                None => {
                    self.band = band_roll(rng.next_u15() as i32);
                }
            }
        }

        if !reel_held || readout < STRIKE_MIN_READOUT {
            return false;
        }
        let mut credit = self.countdown + STRIKE_CREDIT_BASE + edge_bonus.max(0);
        if readout < STRIKE_CREDIT_ZERO_READOUT {
            credit = 0;
        }
        let denom = readout.max(1);
        (rng.next_u15() as i32 % denom) < credit
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
    timer: i32,
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

    fn reroll(&mut self, sp: &FishingSpecies, depth: i32, rng: &mut BiosRand) {
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
            FishMove::Run => out.sink = (pull * sp.sink_factor) / 150,
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

/// Which phase of a [`PondSession`] is live, mirroring the retail mode-SM
/// states (`FUN_801cf3bc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PondPhase {
    /// State `0xc`: idle at the shore, waiting for the cast press.
    Idle,
    /// State `0xd`: cast wind-up (~12 frames of camera pan).
    WindUp,
    /// State `0x14`: the casting-power oscillator, until the lock press.
    Power,
    /// States `0x1e`..`0x22`: the lure flies out and settles (the landing is
    /// the cast-counter increment).
    Flight,
    /// The pre-hook loop: band roll / cadence / strike checks per frame.
    Waiting,
    /// A fish is hooked: the reel tug-of-war.
    Hooked,
    /// The fight resolved with a landed catch.
    Landed,
    /// The fight resolved with a snapped line.
    Snapped,
}

/// One frame of player input to [`PondSession::tick`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PondInput {
    /// Held pad mask bits `0x40` (Cross / reel A) and `0x80` (Square /
    /// reel B) - the `_DAT_8007b850` bits the reel decoder reads.
    pub reel_mask: u32,
    /// The cast / confirm edge (Circle `0x20` in retail; Space on the page).
    pub cast_edge: bool,
    /// Count of fresh input edges this frame (D-pad left/right, either reel
    /// button) - each adds one to the strike credit.
    pub edge_bonus: i32,
}

/// A per-frame event the presentation layer reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PondEvent {
    /// A reel cadence matched: the "Good!" strike splash.
    Splash,
    /// A fish struck and hooked; the payload is the species id.
    Hooked(usize),
    /// The fight landed the fish for this many points.
    Landed(i32),
    /// The line snapped.
    Snapped,
}

/// The venue-faithful fishing session: the retail cast -> wait -> strike ->
/// fight -> score loop over the disc's species / spawn / cadence tables and
/// the save block's persistent lure, rod, cast-counter and point record.
///
/// Composes the pinned kernels ([`CastPower`], [`ReelCadence`], [`BandCheck`],
/// [`band4_gate`], [`spawn_species`], [`TensionGauge`] via the fight,
/// [`FishingRecord`]) with the reconstruction glue each doc-comment marks
/// (flight timing, the record reel-down rate, the snap-at-max-tension loss).
#[derive(Debug, Clone)]
pub struct PondSession {
    /// The 10-record species table (disc rodata).
    pub species: Vec<FishingSpecies>,
    /// This venue's `8 x 8` spawn page (disc rodata).
    pub spawn: Vec<[u32; 8]>,
    /// Venue: `0` Buma pond, `1` Vidna pond (`DAT_801d90d0`).
    pub venue: usize,
    /// Persistent equipped-lure row (`_DAT_80084450`, 0..=2).
    pub lure: u32,
    /// Persistent rod stat (`_DAT_80084454`, 0..=2).
    pub rod: i32,
    /// Persistent lifetime cast counter (`_DAT_80084460`).
    pub casts: i32,
    /// Persistent point record (`_DAT_8008444C` / `58` / `5C`).
    pub record: FishingRecord,
    /// Persistent one-time prize bitmask (`_DAT_8008446C`).
    pub purchased_mask: u32,

    phase: PondPhase,
    cast: CastPower,
    cadence: ReelCadence,
    band: BandCheck,
    rng: BiosRand,
    /// Phase-local frame counter (wind-up / flight).
    timer: i32,
    /// Line record (`DAT_801d927c`); seeded from the locked cast power.
    line_record: i32,
    /// Line depth (`DAT_801d9298`).
    depth: i32,
    /// Fish lateral offset during the fight (dart push accumulator).
    lateral: i32,
    fight_species: Option<usize>,
    fish: FishAi,
    gauge: TensionGauge,
    /// Accumulated fight strength (`DAT_801d91b8`).
    strength: i32,
    /// Points awarded by the last landed catch.
    last_award: i32,
    events: Vec<PondEvent>,
}

/// Wind-up frames before the power meter opens (state `0xd`: ~12 frames).
pub const WINDUP_FRAMES: i32 = 12;

/// Flight frames before the lure settles (state `0x1e` waits for the line
/// animation counter to reach `0x14`).
pub const FLIGHT_FRAMES: i32 = 0x14;

impl PondSession {
    /// Open a session at `venue` over the disc tables, with the persistent
    /// save-block state (`lure` / `rod` / `casts` / `record` /
    /// `purchased_mask`) supplied by the host.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        species: Vec<FishingSpecies>,
        spawn: Vec<[u32; 8]>,
        cadence_templates: Vec<CadenceTemplate>,
        venue: usize,
        lure: u32,
        rod: i32,
        casts: i32,
        record: FishingRecord,
        purchased_mask: u32,
        seed: u32,
    ) -> Self {
        Self {
            species,
            spawn,
            venue,
            lure: lure.min(2),
            rod: rod.clamp(0, 2),
            casts,
            record,
            purchased_mask,
            phase: PondPhase::Idle,
            cast: CastPower::new(),
            cadence: ReelCadence::new(cadence_templates),
            band: BandCheck::default(),
            rng: BiosRand::new(seed),
            timer: 0,
            line_record: 0,
            depth: 0,
            lateral: 0,
            fight_species: None,
            fish: FishAi::default(),
            gauge: TensionGauge::new(0),
            strength: 0,
            last_award: 0,
            events: Vec::new(),
        }
    }

    /// The live phase.
    pub fn phase(&self) -> PondPhase {
        self.phase
    }

    /// The live cast-power meter value.
    pub fn cast_power(&self) -> i32 {
        self.cast.value()
    }

    /// Line record (`DAT_801d927c`); `0` before a cast.
    pub fn line_record(&self) -> i32 {
        self.line_record
    }

    /// The HUD length readout term (`DAT_801d9280`).
    pub fn readout(&self) -> i32 {
        (self.line_record - RECORD_STRIKE_BASE).max(0)
    }

    /// Line depth (`DAT_801d9298`).
    pub fn depth(&self) -> i32 {
        self.depth
    }

    /// Fish lateral offset (dart accumulator), for the presentation layer.
    pub fn lateral(&self) -> i32 {
        self.lateral
    }

    /// Live tension, `0..=0x1000`.
    pub fn tension(&self) -> i32 {
        self.gauge.tension()
    }

    /// Accumulated fight strength (`DAT_801d91b8`).
    pub fn strength(&self) -> i32 {
        self.strength
    }

    /// The hooked species record, while fighting (and through the resolved
    /// phases, for the result banner).
    pub fn hooked(&self) -> Option<&FishingSpecies> {
        self.fight_species.and_then(|i| self.species.get(i))
    }

    /// The fish's current behaviour state, while hooked.
    pub fn fish_move(&self) -> Option<FishMove> {
        (self.phase == PondPhase::Hooked).then_some(self.fish.state)
    }

    /// Points awarded by the last landed catch.
    pub fn last_award(&self) -> i32 {
        self.last_award
    }

    /// The current band (hidden state; surfaced for tests + debug overlays).
    pub fn band(&self) -> u32 {
        self.band.band
    }

    /// Drain the events raised since the last call.
    pub fn take_events(&mut self) -> Vec<PondEvent> {
        std::mem::take(&mut self.events)
    }

    /// Line-record seed for a locked cast power: the deep-cast readout is
    /// ~1000 (`denom` context in the doc), so full power maps to
    /// `300 + 1000` and the floor stays above the `500` band-check gate.
    /// (Approximation - the retail line-projection vector math is unpinned.)
    fn record_for_power(power: i32) -> i32 {
        RECORD_STRIKE_BASE + 260 + power * 1000 / CAST_POWER_MAX
    }

    /// Advance one frame. `frame_step` is the retail `DAT_1f800393` (1 at
    /// 60 fps); `cast_step` is the casting-power meter step per frame (the
    /// native driver uses `0x80`).
    pub fn tick(&mut self, input: PondInput, frame_step: i32, cast_step: i32) {
        let fs = frame_step.max(1);
        match self.phase {
            PondPhase::Idle => {
                if input.cast_edge {
                    self.phase = PondPhase::WindUp;
                    self.timer = 0;
                }
            }
            PondPhase::WindUp => {
                self.timer += fs;
                if self.timer >= WINDUP_FRAMES {
                    self.cast = CastPower::new();
                    self.phase = PondPhase::Power;
                }
            }
            PondPhase::Power => {
                self.cast.advance(cast_step * fs);
                if input.cast_edge {
                    let power = self.cast.lock();
                    self.line_record = Self::record_for_power(power);
                    self.depth = 0;
                    self.timer = 0;
                    self.phase = PondPhase::Flight;
                }
            }
            PondPhase::Flight => {
                self.timer += fs;
                if self.timer >= FLIGHT_FRAMES {
                    // The lure lands: the persistent cast counter increments
                    // here (the same event that advances the retail SM to
                    // state 0x19).
                    self.casts += 1;
                    self.band = BandCheck::default();
                    self.cadence.reset();
                    self.phase = PondPhase::Waiting;
                }
            }
            PondPhase::Waiting => {
                let button = match ReelInput::from_pad_mask(input.reel_mask) {
                    ReelInput::ReelA => 1,
                    ReelInput::ReelB => 2,
                    ReelInput::Idle => 0,
                };
                let matched = self.cadence.feed(button, fs);
                if matched.is_some() {
                    self.events.push(PondEvent::Splash);
                }
                let reel_held = input.reel_mask & 0xc0 != 0;
                let readout = self.readout();
                let struck = self.band.tick(
                    &mut self.rng,
                    matched,
                    self.line_record,
                    readout,
                    input.edge_bonus,
                    reel_held,
                    fs,
                );
                if struck {
                    let mut band = self.band.band;
                    if band4_gate(
                        self.venue,
                        self.lure,
                        self.rod,
                        band,
                        self.casts,
                        &mut self.rng,
                    ) {
                        band = 4;
                    }
                    if let Some(id) = spawn_species(&self.spawn, self.lure, band) {
                        self.fight_species = Some(id);
                        self.fish = FishAi::default();
                        self.gauge = TensionGauge::new(self.rod);
                        self.strength = 0;
                        self.lateral = 0;
                        self.events.push(PondEvent::Hooked(id));
                        self.phase = PondPhase::Hooked;
                    }
                }
                // Reeling the empty line back in shortens it; fully reeled in
                // returns to the idle shore (an engine convenience - retail
                // parks in the cast loop until the leave confirm).
                if reel_held {
                    self.line_record -= 4 * fs;
                    if self.line_record <= RECORD_STRIKE_BASE {
                        self.line_record = 0;
                        self.phase = PondPhase::Idle;
                    }
                }
            }
            PondPhase::Hooked => {
                let Some(sp) = self
                    .fight_species
                    .and_then(|i| self.species.get(i))
                    .copied()
                else {
                    self.phase = PondPhase::Idle;
                    return;
                };
                let reel = ReelInput::from_pad_mask(input.reel_mask);
                let frame = self.fish.tick(&sp, self.depth, &mut self.rng, fs);
                // The per-frame pull accumulates into the fight strength
                // (`DAT_801d91b8`, "the accumulated pull / strength for the
                // fight") - the value the landed score is computed over.
                self.strength = self.strength.saturating_add(frame.pull);
                self.lateral = (self.lateral + frame.lateral).clamp(-0x400, 0x400);
                // Tension: the confirmed tug-of-war.
                self.gauge.apply_reel(reel, frame.pull, fs);
                // Line record: reeling brings the fish in, the fish's run
                // pays line back out (rates are engine-side glue - the doc's
                // Open list).
                match reel {
                    ReelInput::ReelA => {
                        self.line_record -= 3 * fs;
                        self.depth -= 2 * fs;
                    }
                    ReelInput::ReelB => {
                        self.line_record -= 2 * fs;
                        self.depth -= fs;
                    }
                    ReelInput::Idle => self.line_record += frame.pull >> 6,
                }
                self.depth = (self.depth + frame.sink).clamp(0, 0x1000);
                if self.gauge.at_max() {
                    // Reconstruction: tension pinned at the ceiling snaps the
                    // line (doc Open list).
                    self.events.push(PondEvent::Snapped);
                    self.phase = PondPhase::Snapped;
                } else if self.line_record < LAND_RECORD {
                    // Reel-in complete (`record < 0x136`): score the catch.
                    let award = sp.score_for(self.strength);
                    self.record.credit(sp.index, award);
                    self.last_award = award;
                    self.events.push(PondEvent::Landed(award));
                    self.phase = PondPhase::Landed;
                }
            }
            PondPhase::Landed | PondPhase::Snapped => {
                if input.cast_edge {
                    self.fight_species = None;
                    self.line_record = 0;
                    self.depth = 0;
                    self.phase = PondPhase::Idle;
                }
            }
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
    fn reel_decoder_matches_retail_three_way_branch() {
        // Cross (0x40) -> reel A.
        assert_eq!(ReelInput::from_pad_mask(0x40), ReelInput::ReelA);
        // Square (0x80) without Cross -> reel B.
        assert_eq!(ReelInput::from_pad_mask(0x80), ReelInput::ReelB);
        // Both reel buttons held: Cross takes priority (not a blend).
        assert_eq!(ReelInput::from_pad_mask(0xC0), ReelInput::ReelA);
        // Neither reel button -> idle, even with other buttons down.
        assert_eq!(ReelInput::from_pad_mask(0), ReelInput::Idle);
        assert_eq!(ReelInput::from_pad_mask(0x20), ReelInput::Idle); // Circle = cast
        assert_eq!(ReelInput::from_pad_mask(0x100), ReelInput::Idle);
        // Exhaustive cross-check of the low byte against the retail formula
        // `(m & 0x40) ? 1 : ((m >> 6) & 2)`.
        for m in 0u32..0x1_0000 {
            let want = if m & 0x40 != 0 {
                ReelInput::ReelA
            } else if (m >> 6) & 2 != 0 {
                ReelInput::ReelB
            } else {
                ReelInput::Idle
            };
            assert_eq!(ReelInput::from_pad_mask(m), want, "mask {m:#x}");
        }
    }

    #[test]
    fn rod_gate_rejects_an_empty_tacklebox() {
        let mut idx = 0;
        assert!(!select_owned_rod(&mut idx, |_| 0));
        assert_eq!(idx, 0, "index untouched when nothing is owned");
    }

    #[test]
    fn rod_gate_repoints_the_index_at_the_next_owned_lure() {
        // Only the third lure (0x9f) is held; a selection sitting on the first
        // must walk forward to it.
        let mut idx = 0;
        assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9f)));
        assert_eq!(idx, 2);
        // Already on an owned kind: no movement.
        let mut idx = 2;
        assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9f)));
        assert_eq!(idx, 2);
    }

    #[test]
    fn rod_gate_wraps_past_the_last_kind() {
        // Only the first lure is held, selection parked on the last -> wraps.
        let mut idx = 2;
        assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9d)));
        assert_eq!(idx, 0);
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

    fn exchange() -> PrizeExchange {
        // Shaped like a venue page: a one-time top prize + repeatables.
        let rows = [
            (1u32, 20_000u32, 0x6Fu32),
            (1, 6_500, 0xE5),
            (99, 200, 0x98),
        ];
        let rows: Vec<_> = rows
            .iter()
            .enumerate()
            .map(
                |(row, &(limit, price, item_id))| legaia_asset::fishing_exchange::ExchangeRow {
                    row,
                    limit,
                    price,
                    item_id,
                },
            )
            .collect();
        PrizeExchange::from_asset(1, &rows, None)
    }

    #[test]
    fn exchange_row0_hidden_until_strictly_affordable() {
        let ex = exchange();
        assert_eq!(ex.first_visible(19_999), 1);
        assert_eq!(ex.first_visible(20_000), 1); // strict less-than
        assert_eq!(ex.first_visible(20_001), 0);
    }

    #[test]
    fn exchange_availability_gates() {
        let ex = exchange();
        // Affordable + unowned + unlatched = available.
        assert!(ex.is_available(1, 6_500, 0, 0));
        // Unaffordable.
        assert!(!ex.is_available(1, 6_499, 0, 0));
        // Inventory pinned at the 99 cap.
        assert!(!ex.is_available(2, 1_000, 99, 0));
        // One-time bit latched (venue 1 -> bits 8..).
        let latched = 1 << ex.purchase_bit(1);
        assert!(!ex.is_available(1, 6_500, 0, latched));
        assert_eq!(ex.purchase_bit(1), 9);
    }

    #[test]
    fn exchange_max_qty_and_buy() {
        let ex = exchange();
        // Repeatable row: min(points/price, limit - owned).
        assert_eq!(ex.max_qty(2, 1_000, 0, 0), 5);
        assert_eq!(ex.max_qty(2, 1_000_000, 90, 0), 9);
        // One-time row not yet latched treats owned as 0.
        assert_eq!(ex.max_qty(1, 6_500, 1, 0), 1);
        let p = ex.buy(2, 3, 1_000, 0, 0).expect("buys");
        assert_eq!(
            (p.item_id, p.qty, p.cost, p.latched_bit),
            (0x98, 3, 600, None)
        );
        // One-time buy latches its venue-offset bit.
        let p = ex.buy(1, 1, 6_500, 0, 0).expect("buys");
        assert_eq!(p.latched_bit, Some(9));
        // Over-quantity and unavailable rows refuse.
        assert!(ex.buy(2, 6, 1_000, 0, 0).is_none());
        assert!(ex.buy(1, 1, 6_500, 0, 1 << 9).is_none());
    }

    // -- help_panel_layout (overlay_fishing 0x801D72A0) ----------------

    #[test]
    fn help_panel_page0_has_14_lines_at_13px_pitch() {
        let l = help_panel_layout(0x20, 0x18, false);
        assert_eq!(l.lines.len(), 14);
        assert_eq!(
            l.lines[0],
            HelpPanelLine {
                string_index: 0,
                x: 0x20,
                y: 0x18
            }
        );
        assert_eq!(l.lines[13].y, 0x18 + 13 * 13);
        assert_eq!(l.footer, (0xE0, 0xCA));
        assert_eq!(l.frame, (0x20, 0x18, 0x119, 0xC3));
    }

    #[test]
    fn help_panel_page1_has_15_lines() {
        let l = help_panel_layout(0, 0, true);
        assert_eq!(l.lines.len(), 15);
        assert_eq!(l.lines[14].y, 13 * 14);
    }

    // -- FishingMenu (overlay_fishing 0x801D0474) ----------------------

    #[test]
    fn fishing_menu_cursor_wraps_by_snapping() {
        let mut m = FishingMenu::default();
        // Up from row 0: cursor goes -1, snap to 4.
        let t = m.tick(0x1000, true);
        assert_eq!(m.cursor, 4);
        assert_eq!(t.sfx, Some(0x21));
        // Down from row 4: cursor goes 5, snap to 0.
        m.tick(0x4000, true);
        assert_eq!(m.cursor, 0);
        assert_eq!(m.cursor_pos(), (0x5B, 0x58));
    }

    #[test]
    fn fishing_menu_confirm_maps_rows_to_states() {
        for (row, want) in FISHING_MENU_ROW_STATES.iter().enumerate() {
            let mut m = FishingMenu { cursor: row as i32 };
            let t = m.tick(0x40, true);
            assert_eq!(t.next_state, Some(*want), "row {row}");
            assert_eq!(t.sfx, Some(0x20));
            assert_eq!(t.snapshot_points, row == 2 || row == 3, "row {row}");
            assert_eq!(t.leave_venue, row == 4, "row {row}");
        }
    }

    #[test]
    fn fishing_menu_cancel_and_non_interactive() {
        let mut m = FishingMenu { cursor: 2 };
        let t = m.tick(0x20, true);
        assert_eq!(t.next_state, Some(0x0A));
        assert_eq!(t.sfx, Some(0x37));
        // Non-interactive: pad ignored entirely.
        let mut m = FishingMenu { cursor: 2 };
        let t = m.tick(0xFFFF, false);
        assert_eq!(t.next_state, None);
        assert_eq!(t.sfx, None);
        assert_eq!(m.cursor, 2);
    }

    // Inventory helper for the rod/lure select tests: `owned` lists the item
    // ids the player holds (count 1 each).
    fn inv(owned: &[u32]) -> impl FnMut(u32) -> i32 + '_ {
        move |id| owned.contains(&id) as i32
    }

    #[test]
    fn rod_lure_select_equips_owned_lure() {
        // Cursor on lure row 1; the paired lure item 0x9e is owned. Accept
        // (Cross 0x40) equips lure index 1 with the confirm SFX.
        let mut s = RodLureSelect { cursor: 1 };
        let t = s.tick(0x40, 0, true, inv(&[0x9e, 0xa0]));
        assert_eq!(t.equip_lure, Some(1));
        assert_eq!(t.equip_rod, None);
        assert_eq!(t.sfx, Some(0x20));
    }

    #[test]
    fn rod_lure_select_refuses_unowned_lure() {
        // Lure row 2 whose item 0x9f is not owned: accept refuses (SFX 0x22),
        // nothing equipped.
        let mut s = RodLureSelect { cursor: 2 };
        let t = s.tick(0x40, 0, true, inv(&[0x9d, 0xa0]));
        assert_eq!(t.equip_lure, None);
        assert_eq!(t.sfx, Some(0x22));
    }

    #[test]
    fn rod_lure_select_walks_owned_rods() {
        // Player owns rod slots 0 and 2 (item 0xa0, 0xa2), so two rod rows show
        // at cursor 3 and 4. Rod row 4 (cursor-3 = 1) is the *second* owned rod
        // = slot 2, skipping the unowned slot 1.
        let mut s = RodLureSelect { cursor: 4 };
        let t = s.tick(0x40, 0, true, inv(&[0xa0, 0xa2]));
        assert_eq!(t.equip_rod, Some(2));
        assert_eq!(t.equip_lure, None);
        assert_eq!(t.sfx, Some(0x20));
        // Rod row 3 (cursor-3 = 0) is the first owned rod = slot 0.
        let mut s = RodLureSelect { cursor: 3 };
        let t = s.tick(0x40, 0, true, inv(&[0xa0, 0xa2]));
        assert_eq!(t.equip_rod, Some(0));
    }

    #[test]
    fn rod_lure_select_cursor_wraps_against_owned_rods() {
        // Two owned rods -> max cursor = owned_rods + 2 = 4. Moving down past it
        // snaps to 0; moving up below 0 snaps to 4.
        let mut s = RodLureSelect { cursor: 4 };
        let t = s.tick(0, 0x4000, true, inv(&[0xa0, 0xa2])); // down
        assert_eq!(t.sfx, Some(0x21));
        assert_eq!(s.cursor, 0);
        let mut s = RodLureSelect { cursor: 0 };
        s.tick(0, 0x1000, true, inv(&[0xa0, 0xa2])); // up
        assert_eq!(s.cursor, 4);
    }

    #[test]
    fn rod_lure_select_cancel_and_non_interactive() {
        // Cancel (Circle 0x20) leaves with the cancel SFX.
        let mut s = RodLureSelect { cursor: 1 };
        let t = s.tick(0x20, 0, true, inv(&[0x9d]));
        assert!(t.leave);
        assert_eq!(t.sfx, Some(0x37));
        // Non-interactive: no pad acted, but the snap-wrap still runs. An
        // out-of-range cursor with one owned rod (max 3) snaps to 0.
        let mut s = RodLureSelect { cursor: 9 };
        let t = s.tick(0xFFFF, 0xFFFF, false, inv(&[0xa0]));
        assert_eq!(t.sfx, None);
        assert!(!t.leave);
        assert_eq!(s.cursor, 0);
    }

    // --- retail species selection ------------------------------------------

    use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate};

    #[test]
    fn band_roll_matches_the_cutoff_table() {
        assert_eq!(band_roll(0), 3);
        assert_eq!(band_roll(0xc00), 3);
        assert_eq!(band_roll(0xc01), 2);
        assert_eq!(band_roll(0xe70), 2);
        assert_eq!(band_roll(0xe71), 1);
        assert_eq!(band_roll(0xf38), 1);
        assert_eq!(band_roll(0xf39), 0);
        assert_eq!(band_roll(0xfff), 0);
    }

    #[test]
    fn band4_gate_conditions() {
        // Buma: > 50 casts, even, Normal Lure, third rod, band 0, then 1/16.
        let mut hits = 0;
        for seed in 0..64u32 {
            let mut rng = BiosRand::new(seed);
            if band4_gate(0, 1, 2, 0, 52, &mut rng) {
                hits += 1;
            }
        }
        assert!(hits > 0, "1/16 roll never fired over 64 seeds");
        // Any failed precondition short-circuits without advancing the rng.
        let mut rng = BiosRand::new(7);
        let before = rng;
        assert!(!band4_gate(0, 1, 2, 0, 51, &mut rng)); // odd counter
        assert!(!band4_gate(0, 1, 2, 0, 40, &mut rng)); // even but under the 0x32 threshold
        assert!(!band4_gate(0, 0, 2, 0, 52, &mut rng)); // wrong lure
        assert!(!band4_gate(0, 1, 1, 0, 52, &mut rng)); // wrong rod
        assert!(!band4_gate(0, 1, 2, 1, 52, &mut rng)); // wrong band
        assert_eq!(rng, before, "short-circuit must not advance the rng");
        // Vidna: Heavy Lure, no cast-count threshold, 1/4.
        let mut hits = 0;
        for seed in 0..16u32 {
            let mut rng = BiosRand::new(seed);
            if band4_gate(1, 2, 2, 0, 0, &mut rng) {
                hits += 1;
            }
        }
        assert!(hits > 0, "1/4 roll never fired over 16 seeds");
    }

    fn templates() -> Vec<CadenceTemplate> {
        // The disc's four shapes (doc table; durations as on the USA disc).
        let t = |steps: &[(i32, u8)], window: i32| CadenceTemplate {
            history_window: window,
            steps: steps
                .iter()
                .map(|&(duration, button)| CadenceStep { duration, button })
                .collect(),
        };
        vec![
            t(&[(40, 0), (25, 1), (40, 0), (15, 2)], 0x8c),
            t(&[(15, 2), (25, 1), (0, 0)], 0x8c),
            t(&[(15, 2), (40, 0), (15, 2)], 0x82),
            t(&[(25, 1), (40, 0), (25, 1)], 0x96),
        ]
    }

    /// Drive the recogniser through `seq` = [(button, frames)] and return the
    /// first match.
    fn drive(c: &mut ReelCadence, seq: &[(u8, i32)]) -> Option<usize> {
        for &(b, frames) in seq {
            for _ in 0..frames {
                if let Some(t) = c.feed(b, 1) {
                    return Some(t);
                }
            }
        }
        None
    }

    #[test]
    fn cadence_recogniser_matches_template_3() {
        // Cross 25, idle 40, Cross 25 - the natural pump rhythm.
        let mut c = ReelCadence::new(templates());
        let got = drive(&mut c, &[(0, 30), (1, 25), (0, 40), (1, 25)]);
        assert_eq!(got, Some(3));
    }

    #[test]
    fn cadence_recogniser_matches_template_0_with_tolerance() {
        // idle 40, Cross 25, idle 40, Square 15 - +-10 slop on each step.
        let mut c = ReelCadence::new(templates());
        let got = drive(&mut c, &[(0, 45), (1, 20), (0, 35), (2, 18)]);
        assert_eq!(got, Some(0));
    }

    #[test]
    fn cadence_recogniser_rejects_out_of_tolerance_holds() {
        let mut c = ReelCadence::new(templates());
        // Cross held far too long between the idles: no template fits.
        let got = drive(&mut c, &[(0, 40), (1, 60), (0, 40)]);
        assert_eq!(got, None);
    }

    #[test]
    fn cadence_match_resets_the_ring() {
        let mut c = ReelCadence::new(templates());
        assert_eq!(
            drive(&mut c, &[(0, 30), (1, 25), (0, 40), (1, 25)]),
            Some(3)
        );
        // Immediately after the reset the same tail can't re-match.
        assert_eq!(c.feed(1, 1), None);
    }

    #[test]
    fn band_check_holds_a_matched_band_and_boosts_credit() {
        let mut b = BandCheck::default();
        let mut rng = BiosRand::new(1);
        // A cadence match stores the template id as the band and arms the
        // countdown + splash.
        b.tick(&mut rng, Some(0), 1000, 700, 0, false, 1);
        assert_eq!(b.band, 0);
        assert!(b.splash);
        assert_eq!(b.countdown, BAND_HOLD_FRAMES);
        // While held, unmatched frames keep the band (countdown decays).
        b.tick(&mut rng, None, 1000, 700, 0, false, 1);
        assert_eq!(b.band, 0);
        assert!(!b.splash);
        assert_eq!(b.countdown, BAND_HOLD_FRAMES - 1);
    }

    #[test]
    fn band_check_strike_requires_reel_and_readout() {
        let mut rng = BiosRand::new(2);
        let mut b = BandCheck::default();
        // Readout below the floor: no strike regardless of credit.
        for _ in 0..200 {
            assert!(!b.tick(&mut rng, Some(0), 1000, 150, 5, true, 1));
        }
        // Reel not held: no strike.
        let mut b = BandCheck::default();
        for _ in 0..200 {
            assert!(!b.tick(&mut rng, Some(0), 1000, 700, 5, false, 1));
        }
    }

    #[test]
    fn spawn_lookup_uses_lure_row_and_band_column() {
        let mut table = vec![[0u32; 8]; 8];
        table[1] = [5, 5, 3, 5, 9, 0, 0, 0];
        assert_eq!(spawn_species(&table, 1, 0), Some(5));
        assert_eq!(spawn_species(&table, 1, 4), Some(9));
        assert_eq!(spawn_species(&table, 9, 0), None);
        table[2][0] = 99; // out-of-table species id
        assert_eq!(spawn_species(&table, 2, 0), None);
    }

    fn pond() -> PondSession {
        let species: Vec<FishingSpecies> = (0..10)
            .map(|i| species(i, 1000 * (i as i32 + 1), 400))
            .collect();
        let mut spawn = vec![[0u32; 8]; 8];
        spawn[0] = [3, 3, 5, 5, 0, 0, 0, 0];
        spawn[1] = [5, 5, 3, 5, 9, 0, 0, 0];
        spawn[2] = [7, 1, 4, 2, 0, 0, 0, 0];
        PondSession::new(
            species,
            spawn,
            templates(),
            0,
            1,
            2,
            60,
            FishingRecord::default(),
            0,
            0xC0FFEE,
        )
    }

    #[test]
    fn pond_session_full_loop_hooks_fights_and_lands() {
        let mut p = pond();
        assert_eq!(p.phase(), PondPhase::Idle);
        // Cast press -> wind-up -> power.
        p.tick(
            PondInput {
                cast_edge: true,
                ..Default::default()
            },
            1,
            0x80,
        );
        for _ in 0..WINDUP_FRAMES {
            p.tick(PondInput::default(), 1, 0x80);
        }
        assert_eq!(p.phase(), PondPhase::Power);
        // Sweep to a deep cast, then lock.
        for _ in 0..24 {
            p.tick(PondInput::default(), 1, 0x80);
        }
        p.tick(
            PondInput {
                cast_edge: true,
                ..Default::default()
            },
            1,
            0x80,
        );
        assert_eq!(p.phase(), PondPhase::Flight);
        let casts_before = p.casts;
        for _ in 0..FLIGHT_FRAMES {
            p.tick(PondInput::default(), 1, 0x80);
        }
        assert_eq!(p.phase(), PondPhase::Waiting);
        assert_eq!(p.casts, casts_before + 1, "landing increments the counter");
        assert!(p.line_record() > BAND_CHECK_MIN_RECORD);

        // Hold reel A until a strike hooks a fish (bounded).
        let mut hooked = false;
        for _ in 0..2000 {
            p.tick(
                PondInput {
                    reel_mask: 0x40,
                    ..Default::default()
                },
                1,
                0x80,
            );
            if p.phase() == PondPhase::Hooked {
                hooked = true;
                break;
            }
            if p.phase() == PondPhase::Idle {
                // Fully reeled in without a strike: cast again.
                p.tick(
                    PondInput {
                        cast_edge: true,
                        ..Default::default()
                    },
                    1,
                    0x80,
                );
                for _ in 0..WINDUP_FRAMES + 40 {
                    p.tick(PondInput::default(), 1, 0x80);
                }
                p.tick(
                    PondInput {
                        cast_edge: true,
                        ..Default::default()
                    },
                    1,
                    0x80,
                );
                for _ in 0..FLIGHT_FRAMES {
                    p.tick(PondInput::default(), 1, 0x80);
                }
            }
        }
        assert!(hooked, "no strike over 2000 held-reel frames");
        let events = p.take_events();
        assert!(
            events.iter().any(|e| matches!(e, PondEvent::Hooked(_))),
            "{events:?}"
        );
        let id = p.hooked().expect("species").index;
        // The hooked species came from the lure row of the spawn table.
        assert!([5usize, 3, 9].contains(&id), "id {id} not in lure-1 row");

        // Fight: alternate reeling and resting so tension never pins, until
        // the fish lands.
        let mut landed = false;
        for i in 0..20000 {
            let reel = if p.tension() < 0x800 { 0x40 } else { 0 };
            p.tick(
                PondInput {
                    reel_mask: reel,
                    ..Default::default()
                },
                1,
                0x80,
            );
            match p.phase() {
                PondPhase::Landed => {
                    landed = true;
                    break;
                }
                PondPhase::Snapped => panic!("line snapped under the safe reel policy at {i}"),
                _ => {}
            }
        }
        assert!(landed, "fight never resolved");
        assert!(p.record.points > 0);
        assert!(p.last_award() > 0);
        let events = p.take_events();
        assert!(events.iter().any(|e| matches!(e, PondEvent::Landed(_))));
        // Recast returns to the shore.
        p.tick(
            PondInput {
                cast_edge: true,
                ..Default::default()
            },
            1,
            0x80,
        );
        assert_eq!(p.phase(), PondPhase::Idle);
    }

    #[test]
    fn pond_session_is_deterministic_for_a_seed() {
        let run = || {
            let mut p = pond();
            let mut log = Vec::new();
            for i in 0..4000u32 {
                let input = PondInput {
                    reel_mask: if i % 90 < 45 { 0x40 } else { 0 },
                    cast_edge: i % 200 == 0,
                    ..Default::default()
                };
                p.tick(input, 1, 0x80);
                log.push((p.phase() as u8 as u32, p.tension(), p.line_record()));
            }
            (log, p.record.points, p.casts)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn pond_snaps_when_tension_pins() {
        let mut p = pond();
        // Cast deep.
        p.tick(
            PondInput {
                cast_edge: true,
                ..Default::default()
            },
            1,
            0x80,
        );
        for _ in 0..WINDUP_FRAMES + 30 {
            p.tick(PondInput::default(), 1, 0x80);
        }
        p.tick(
            PondInput {
                cast_edge: true,
                ..Default::default()
            },
            1,
            0x80,
        );
        for _ in 0..FLIGHT_FRAMES {
            p.tick(PondInput::default(), 1, 0x80);
        }
        // Hold reel forever: the session must terminate (a weak fish lands
        // before tension pins; a strong pull snaps the line; an empty reel-in
        // returns to Idle) - it must never wedge in the fight.
        let mut resolved = None;
        for _ in 0..30000 {
            p.tick(
                PondInput {
                    reel_mask: 0x40,
                    ..Default::default()
                },
                1,
                0x80,
            );
            if let ph @ (PondPhase::Snapped | PondPhase::Landed | PondPhase::Idle) = p.phase() {
                resolved = Some(ph);
                break;
            }
        }
        assert!(resolved.is_some(), "held-reel session never resolved");
        // And the snap edge itself is exercised directly by the gauge: a
        // strong pull with the reel held pins the ceiling.
        let mut g = TensionGauge::new(2);
        for _ in 0..0x1000 {
            g.apply_reel(ReelInput::ReelA, 4000, 1);
        }
        assert!(g.at_max());
    }
}
