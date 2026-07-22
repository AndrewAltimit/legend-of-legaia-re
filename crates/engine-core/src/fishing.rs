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
}
