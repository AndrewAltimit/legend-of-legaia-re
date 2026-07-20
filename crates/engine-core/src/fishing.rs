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
//! # Wiring status
//!
//! The **rules** half is live: [`FishingSession`] and the kernels it drives
//! ([`CastPower`], [`Tension`], [`FishingRecord`]) are called from
//! `world`'s minigame dispatch, which is how the fishing minigame runs.
//!
//! The **presentation** half is not. Everything below producing a
//! [`HudDraw`], [`BarFrame`] or [`DigitCell`] - [`persistent_hud_draws`],
//! [`catch_hud_draws`], the four banner animators, [`strike_splash_draws`],
//! [`number_digit_cells`], [`bar_frame`], [`power_bar_frame`] - is
//! reachable only from this module's unit tests. Nothing renders a fishing
//! HUD yet: the host would need a fishing-mode draw pass in
//! `engine-render` / the play page, the sibling of `engine-ui`'s
//! `battle_hud_draws_for`, to consume these lists. The ports are kept
//! because they pin the retail layout constants; they are **not** evidence
//! that the HUD is drawn. Individual items carry a `NOT WIRED:` note.
//!
//! [`select_owned_rod`] is likewise unwired: it belongs to the rod/lure
//! selection screen, which has no host UI.

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

// --- HUD / banner-timer cluster ---------------------------------------------

/// The line-record base offset shared by the hook check (`FUN_801d4004`:
/// `record < gate + 300`) and the catch-HUD length readout (`FUN_801d1580`:
/// `record - 300`, clamped at zero).
pub const RECORD_STRIKE_BASE: i32 = 300;

/// Glyph / digit brightness of the persistent + catch HUD rows
/// (`FUN_801d13f0` / `FUN_801d1580`: the `0x80` brightness argument).
pub const HUD_BRIGHTNESS: i32 = 0x80;
/// Full brightness (`0xff`): the hooked-gauge block and the banner sprites.
pub const HUD_BRIGHTNESS_FULL: i32 = 0xff;

/// Frames a slide banner stays live (`FUN_801d75dc` / `FUN_801d78ec`:
/// active while `frame < 0xc8`).
pub const BANNER_FRAMES: i32 = 0xc8;
/// Frames the strike splash stays live (`FUN_801d71d4`: `frame < 0x98`).
pub const SPLASH_FRAMES: i32 = 0x98;

/// One primitive of the fishing HUD draw list. Each variant models a call
/// into one of the overlay's shared draw helpers; the coordinates, glyph ids,
/// and brightness values are the retail call-site constants. Rendering is the
/// host's job - this module only decides *what* is drawn where.
// REF: FUN_801d76e0 (digit blitter), FUN_801d63b0 (shared sprite-quad emitter)
// REF: FUN_801d26cc (the driver whose seed sites arm the banner timers)
// The variants above name their retail helper; the two gauge-bar helpers
// `FUN_801d1870` / `FUN_801d1a90` *are* ported, as `bar_frame` /
// `power_bar_frame` (see their own PORT tags) - a `HudDraw::Bar` or
// `PowerBar` is resolved through those. The digit blitter and the shared
// sprite-quad emitter remain unported: they are pure VRAM emitters with no
// decision content, so the variant carries their call-site arguments and
// the host does the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudDraw {
    /// A number via the digit blitter `FUN_801d76e0`.
    Number {
        x: i32,
        y: i32,
        value: i32,
        brightness: i32,
    },
    /// A single sprite-quad glyph via the shared emitter `FUN_801d63b0`,
    /// drawn at `0x1000` (1.0) scale. `layer` is the emitter's first argument
    /// (`1` on the HUD rows, `0` on the banner sprites; a draw-class selector,
    /// not further pinned).
    Glyph {
        layer: i32,
        id: u32,
        x: i32,
        y: i32,
        brightness: i32,
    },
    /// A fixed-width count via the shared number primitive (`0x80034b78`).
    Count {
        value: i32,
        digits: u32,
        x: i32,
        y: i32,
    },
    /// A caption via the shared string primitive (`0x80036888`). The text
    /// bytes live in the overlay rodata (not committed); the variant names
    /// the string symbolically.
    Caption { text: HudCaption, x: i32, y: i32 },
    /// A gauge bar via `FUN_801d1870` (`style` 0 = depth, 1 = tension at the
    /// retail call sites; `step` is its per-segment argument).
    Bar {
        style: i32,
        x: i32,
        y: i32,
        value: i32,
        step: i32,
    },
    /// The casting-power meter bar via `FUN_801d1a90`.
    PowerBar {
        x: i32,
        y: i32,
        power: i32,
        step: i32,
    },
}

impl HudDraw {
    /// Resolve a bar variant into the concrete frame + fill its retail
    /// helper would build, routing [`HudDraw::Bar`] through [`bar_frame`]
    /// and [`HudDraw::PowerBar`] through [`power_bar_frame`].
    ///
    /// Returns `None` for every non-bar variant - those name emitters that
    /// carry no decision content and are left to the host.
    pub fn resolve_bar(self) -> Option<BarFrame> {
        match self {
            HudDraw::Bar {
                style,
                x,
                y,
                value,
                step,
            } => Some(bar_frame(x, y, value, step, style)),
            HudDraw::PowerBar { x, y, power, step } => Some(power_bar_frame(x, y, power, step)),
            _ => None,
        }
    }
}

/// Which overlay-rodata caption a [`HudDraw::Caption`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudCaption {
    /// The selected rod/lure type label (three overlay strings, picked by the
    /// persistent index `_DAT_80084450` = 0/1/2; other values draw no label).
    RodName(u32),
    /// The "remaining" caption drawn before the lure count.
    LuresLeft,
    /// The trailing caption drawn after the lure count.
    LureCountSuffix,
}

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
// NOT WIRED: belongs to the rod/lure selection screen's cursor handler.
// That screen has no host UI, so nothing calls this outside tests.
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

/// The persistent fishing HUD (drawn every frame by the mode driver's shared
/// tail): the best-catch row (glyph `0x1a`), the capped point-total row
/// (glyph `0x1c`), the selected rod/lure label, and the lures-remaining
/// count. Retail reads `_DAT_80084458` / `_DAT_8008444c` / `_DAT_80084450`
/// and the live inventory count of [`lure_item_id`]; here they are caller
/// parameters.
// PORT: FUN_801d13f0 (persistent HUD: best-catch + capped point rows, rod label, lure count)
pub fn persistent_hud_draws(
    points: i32,
    best_points: i32,
    rod_index: u32,
    lure_count: i32,
) -> Vec<HudDraw> {
    let mut d = vec![
        HudDraw::Number {
            x: 0x32,
            y: 0x08,
            value: best_points,
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x1a,
            x: 0x10,
            y: 0x08,
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Number {
            x: 0x32,
            y: 0x16,
            value: points.min(FISH_POINTS_CAP),
            brightness: HUD_BRIGHTNESS,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x1c,
            x: 0x10,
            y: 0x16,
            brightness: HUD_BRIGHTNESS,
        },
    ];
    // Rod indices 0..=2 pick one of three overlay label strings; any other
    // index draws no label but still draws the count row.
    if rod_index <= 2 {
        d.push(HudDraw::Caption {
            text: HudCaption::RodName(rod_index),
            x: 0x98,
            y: 0x0c,
        });
    }
    d.push(HudDraw::Caption {
        text: HudCaption::LuresLeft,
        x: 0xf3,
        y: 0x0c,
    });
    d.push(HudDraw::Count {
        value: lure_count,
        digits: 4,
        x: 0x100,
        y: 0x0c,
    });
    d.push(HudDraw::Caption {
        text: HudCaption::LureCountSuffix,
        x: 0x12a,
        y: 0x0c,
    });
    d
}

/// The live values the catch HUD reads (retail globals -> parameters).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatchHudState {
    /// Line length / catch record value (`DAT_801d927c`).
    pub record: i32,
    /// The second length term (`DAT_801d9178`): displayed alone as the lower
    /// readout and added into the total length in the same `>>9` scale (the
    /// cast line-projection component of the readout).
    pub line_extent: i32,
    /// Casting-power meter (`DAT_801d9274`).
    pub cast_power: i32,
    /// Line depth / sink value (`DAT_801d9298`).
    pub depth: i32,
    /// Tension gauge (`DAT_801d9168`).
    pub tension: i32,
    /// `DAT_801d91b4` - set at the hook; gates the depth + tension gauge
    /// block of the catch HUD.
    pub gauges_visible: bool,
}

/// The `DAT_801d9178` display term: `((x >> 8) + (x >>> 31)) >> 1` (an
/// arithmetic `/512` with round-toward-zero), clamped at zero - the exact
/// `FUN_801d1580` sequence.
pub fn extent_display(line_extent: i32) -> i32 {
    (((line_extent >> 8) + ((line_extent as u32) >> 31) as i32) >> 1).max(0)
}

/// The catch HUD's total-length readout, in tenths of a display unit:
/// `max(record - 300, 0) * 100 >> 9` plus [`extent_display`]. The HUD splits
/// it as `value / 10` (whole part) and `value % 10` (tenths digit).
pub fn length_display(record: i32, line_extent: i32) -> i32 {
    let past = (record - RECORD_STRIKE_BASE).max(0);
    let mut scaled = past * 100;
    if scaled < 0 {
        // Retail's negative-rounding adjust before the >>9; unreachable for
        // the clamped input, kept for the exact arithmetic.
        scaled += 0x1ff;
    }
    (scaled >> 9) + extent_display(line_extent)
}

/// The cast-power percent readout: `power * 100 >> 12` (percent of the
/// `0x1000` meter ceiling, with retail's negative-rounding adjust).
pub fn cast_power_percent(power: i32) -> i32 {
    let mut scaled = power * 100;
    if scaled < 0 {
        scaled += 0xfff;
    }
    scaled >> 12
}

/// The catch HUD, drawn while a cast is out: the total-length readout (its
/// whole and tenths digits, glyphs `0xb`/`0x10`), the extent readout (glyph
/// `0xa`), the cast-power percent (glyph `0xe`) plus the power bar, and -
/// once hooked ([`CatchHudState::gauges_visible`]) - the depth and tension
/// gauge bars (glyphs `8`/`9`). Retail also emits a debug length line behind
/// the global print flag `_DAT_8007b9b0`; that log call is not modeled.
// PORT: FUN_801d1580 (catch HUD: length/extent/power readouts + hooked gauge block)
pub fn catch_hud_draws(s: &CatchHudState) -> Vec<HudDraw> {
    let len = length_display(s.record, s.line_extent);
    let ext = extent_display(s.line_extent);
    let pct = cast_power_percent(s.cast_power);
    let b = HUD_BRIGHTNESS;
    let mut d = vec![
        HudDraw::Number {
            x: 0xda,
            y: 0x30,
            value: len / 10,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe8,
            y: 0x30,
            value: len % 10,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xb,
            x: 0xd4,
            y: 0x30,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0x10,
            x: 0x114,
            y: 0x30,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xda,
            y: 0xc0,
            value: ext / 10,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe8,
            y: 0xc0,
            value: ext % 10,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xa,
            x: 0xd4,
            y: 0xc0,
            brightness: b,
        },
        HudDraw::Number {
            x: 0xe4,
            y: 0xb0,
            value: pct,
            brightness: b,
        },
        HudDraw::Glyph {
            layer: 1,
            id: 0xe,
            x: 0xd4,
            y: 0xb0,
            brightness: b,
        },
        HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: s.cast_power,
            step: 0xc,
        },
    ];
    if s.gauges_visible {
        d.extend([
            HudDraw::Glyph {
                layer: 1,
                id: 8,
                x: 0x10,
                y: 0x80,
                brightness: HUD_BRIGHTNESS_FULL,
            },
            HudDraw::Bar {
                style: 0,
                x: 0x10,
                y: 0x90,
                value: s.depth,
                step: 10,
            },
            HudDraw::Glyph {
                layer: 1,
                id: 9,
                x: 0x10,
                y: 0xa0,
                brightness: HUD_BRIGHTNESS_FULL,
            },
            HudDraw::Bar {
                style: 1,
                x: 0x10,
                y: 0xb0,
                value: s.tension,
                step: 10,
            },
        ]);
    }
    d
}

/// Shared slide ramp of the two banner animators: `frame * 8` up to the
/// `0xa0` hold (reached at frame `0x14`), held until frame `0x8c`, then
/// `frame * 8 - 0x3c0` sliding off (the ramps join continuously at `0xa0`).
/// Retail leaves the value undefined for a negative frame (the timers only
/// ever pass `>= 1`); this clamps to the frame-0 value.
fn banner_slide(frame: i32) -> i32 {
    let mut v = frame.max(0) * 8;
    if frame >= 0x14 {
        v = 0xa0;
    }
    if frame >= 0x8c {
        v = frame * 8 - 0x3c0;
    }
    v
}

/// One frame of the banner that slides in from the **left** (glyph `7` at
/// `y = 0x78`, x = the slide ramp). Its timer (`DAT_801d9160`) is seeded at
/// the moment the fish hooks (`FUN_801d26cc`, alongside the gauge-block
/// enable). Returns the draw while active, `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag).
// PORT: FUN_801d78ec (hook banner: left slide-in, hold, slide-off)
pub fn banner_from_left_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 7,
        x: banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the banner that slides in from the **right** (glyph `0xd` at
/// `y = 0x78`, x = `0x140 -` the slide ramp - the mirrored trajectory of
/// [`banner_from_left_draw`], holding at the same `x = 0xa0`). Its timer
/// (`DAT_801d915c`) is seeded on the reel-in-complete path of the hooked
/// fight (`FUN_801d26cc`: record below `0x136` while hooked); while it runs,
/// the driver tail forces the from-left timer back to zero.
// PORT: FUN_801d75dc (reel-in banner: right slide-in, hold, slide-off)
pub fn banner_from_right_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0xd,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the miss / retry banner (glyph `0x19` at `y = 0x78`), the
/// mirrored trajectory of [`banner_from_left_draw`] over the shared ramp. Its
/// timer (`DAT_801d9268`) is the retry countdown the driver runs in state
/// `0x2d` before returning to the cast state. `None` once the frame count
/// reaches [`BANNER_FRAMES`] (retail returns the active flag, which is what
/// keeps the state machine parked).
// PORT: FUN_801d6f10 (miss/retry banner: right slide-in, hold, slide-off)
// NOT WIRED: no host consumes the fishing HUD draw list - see the module
// header's "Wiring status".
pub fn banner_miss_draw(frame: i32) -> Option<HudDraw> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    Some(HudDraw::Glyph {
        layer: 0,
        id: 0x19,
        x: 0x140 - banner_slide(frame),
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    })
}

/// One frame of the auxiliary two-sided banner: the *same* glyph (`0xc`)
/// emitted twice, once at the ramp and once mirrored at `0x140 -` the ramp, so
/// the pair converges on the `0xa0` hold from both edges and parts again on
/// the way out. Its timer (`DAT_801d9164`) is what the driver's state `0x28`
/// waits on before returning to the main loop.
// PORT: FUN_801d7528 (auxiliary banner: mirrored converging glyph pair)
// NOT WIRED: same fishing-HUD draw list as the other banner animators.
pub fn banner_converge_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= BANNER_FRAMES {
        return None;
    }
    let x = banner_slide(frame);
    let glyph = |x: i32| HudDraw::Glyph {
        layer: 0,
        id: 0xc,
        x,
        y: 0x78,
        brightness: HUD_BRIGHTNESS_FULL,
    };
    Some([glyph(x), glyph(0x140 - x)])
}

/// The strike-splash brightness ramp: `frame * 8` up to the `0x80` hold
/// (reached at frame `0x10`), held until frame `0x88`, then
/// `0x80 - (frame - 0x88) * 8` fading out (zero exactly at the `0x98`
/// lifetime end).
pub fn splash_brightness(frame: i32) -> i32 {
    let mut a = frame.max(0) * 8;
    if frame >= 0x10 {
        a = 0x80;
    }
    if frame >= 0x88 {
        a = 0x80 - (frame - 0x88) * 8;
    }
    a
}

/// One frame of the strike splash: a two-glyph pair (`0x416` / `0x816`) at
/// `x = 0xa0` that rises one pixel every 32 frames from `y = 0x50` while
/// fading through [`splash_brightness`]. Its timer (`DAT_801d90f0`) is seeded
/// at the strike / hit event before the fish hooks (`FUN_801d26cc`, gated on
/// the gauge block not yet being up). `None` once the frame count reaches
/// [`SPLASH_FRAMES`].
// PORT: FUN_801d71d4 (strike splash: rising, fading two-glyph pair)
pub fn strike_splash_draws(frame: i32) -> Option<[HudDraw; 2]> {
    if frame >= SPLASH_FRAMES {
        return None;
    }
    let y = 0x50 - (frame.max(0) >> 5);
    let brightness = splash_brightness(frame);
    let glyph = |id: u32| HudDraw::Glyph {
        layer: 0,
        id,
        x: 0xa0,
        y,
        brightness,
    };
    Some([glyph(0x416), glyph(0x816)])
}

/// One digit cell of an expanded [`HudDraw::Number`]: the digit value and the
/// screen slot it occupies. Retail draws these through a digit primitive
/// (`FUN_801d7dd8` / `FUN_801d7d44`) that is separate from the glyph emitter,
/// so they are their own draw type rather than a [`HudDraw::Glyph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitCell {
    /// Screen x of this digit's slot.
    pub x: i32,
    /// Screen y (constant across the field).
    pub y: i32,
    /// The digit, `0..=9`.
    pub digit: i32,
}

/// Horizontal slot pitch of the two digit-field styles (`FUN_801d76e0`):
/// style `0` advances 8 px per slot, any other style 16 px.
pub const DIGIT_PITCH_NARROW: i32 = 8;
/// Wide digit-field pitch - see [`DIGIT_PITCH_NARROW`].
pub const DIGIT_PITCH_WIDE: i32 = 0x10;

/// The digit field is a fixed 8 slots wide; the value is right-aligned in it.
pub const DIGIT_FIELD_SLOTS: usize = 8;

/// Expand a number into its digit cells - the layout half of the digit blitter
/// behind [`HudDraw::Number`].
///
/// The field is a fixed [`DIGIT_FIELD_SLOTS`]-slot row: slot `i` holds
/// `value / 10^(7 - i)` and is emitted only once that quotient is non-zero, so
/// leading zeros are *blank slots*, not drawn zeros, and the number ends up
/// right-aligned. Retail seeds the last slot with `0` before the fill loop,
/// which is what makes a `value` of zero draw a single `0` instead of nothing.
/// `style` selects the slot pitch ([`DIGIT_PITCH_NARROW`] /
/// [`DIGIT_PITCH_WIDE`]).
///
/// Retail applies no negative guard; a negative `value` there yields negative
/// quotients. The port clamps at zero instead, since every call site passes a
/// count or a score.
// PORT: FUN_801d76e0 (8-slot right-aligned digit field: leading-zero blanking)
// NOT WIRED: would be driven by the HUD's Number/Count draws once a host
// renders them.
pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell> {
    let value = value.max(0);
    let pitch = if style == 0 {
        DIGIT_PITCH_NARROW
    } else {
        DIGIT_PITCH_WIDE
    };

    // Slot contents: `-1` = blank, else the quotient at that power of ten.
    let mut slots = [-1i32; DIGIT_FIELD_SLOTS];
    slots[DIGIT_FIELD_SLOTS - 1] = 0;
    let mut pow = 10_000_000i32;
    for slot in slots.iter_mut() {
        let q = value / pow;
        if q != 0 {
            *slot = q;
        }
        pow /= 10;
    }

    slots
        .iter()
        .enumerate()
        .filter_map(|(i, &q)| {
            (q >= 0).then_some(DigitCell {
                x: x + i as i32 * pitch,
                y,
                digit: q % 10,
            })
        })
        .collect()
}

/// The three-glyph frame of a gauge bar: a start cap, a body stretched over
/// the segment count, and an end cap. Both bar animators emit this triple
/// through the shared glyph emitter, then overlay the fill quad themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFrame {
    /// The start-cap / body / end-cap glyph ids, in emit order.
    pub glyphs: [u32; 3],
    /// Screen position of each of the three glyphs.
    pub positions: [(i32, i32); 3],
    /// The body glyph's stretch factor in 12.4 fixed point (`segments << 12`),
    /// applied along the bar's axis.
    pub body_scale: i32,
    /// Length in pixels of the filled portion of the bar,
    /// `segments * value * 8 / 0x1000`.
    pub fill_len: i32,
    /// The fill quad's brightness ramp, `value * 0xff / 0x1000` - the bar
    /// brightens as it fills. Note the *glyph* frame is emitted at a fixed
    /// `0x80`; only the fill tracks the value.
    pub fill_brightness: i32,
    /// The RGB written to all four fill-quad vertices, selected by the
    /// `style` argument. `None` when retail writes no colour at all
    /// (see [`bar_frame`]); the four vertices always share one triple.
    pub fill_rgb: Option<(u8, u8, u8)>,
}

/// Constant red channel of the style-0 fill ramp (`li v0, 0xbc`).
pub const BAR_FILL_STYLE0_RED: u8 = 0xbc;

/// Resolve `FUN_801d1870`'s `param_1` style selector into the fill-quad
/// vertex colour, given the already-scaled brightness byte.
///
/// Retail branches three ways, and only the first two write anything:
///
/// - `0` - `(0xbc, brightness, 0)`: a constant red against the ramp.
/// - `1` - `(brightness, !brightness, 0)`: the ramp against its own
///   bitwise complement, so the bar crossfades as it fills.
/// - anything else - the colour stores are jumped over entirely
///   (`j LAB_801d1974`), leaving whatever the primitive buffer held.
///
/// The retail call sites use `0` for the depth gauge and `1` for the
/// tension gauge; the third arm is unreachable from them.
fn bar_fill_rgb(style: i32, brightness: i32) -> Option<(u8, u8, u8)> {
    let b = brightness as u8;
    match style {
        0 => Some((BAR_FILL_STYLE0_RED, b, 0)),
        1 => Some((b, !b, 0)),
        _ => None,
    }
}

/// Emit brightness of the bar frame glyphs - fixed, unlike the fill.
pub const BAR_FRAME_BRIGHTNESS: i32 = 0x80;

/// Fixed-point unit of the bar `value` (`0x1000` = completely full).
pub const BAR_VALUE_ONE: i32 = 0x1000;

/// The **horizontal** gauge bar behind [`HudDraw::Bar`] (depth / tension at
/// the retail call sites): caps at `x` and `x + segments*8 + 8` with the body
/// stretched between them, filling left-to-right.
///
/// `style` (retail `param_1`) selects the fill quad's colour ramp only - see
/// [`bar_fill_rgb`] - and moves no geometry.
///
/// The retail `>> 12` carries a `+0xfff` negative bias, which is just C
/// division truncating toward zero; the port divides directly.
// PORT: FUN_801d1870 (horizontal gauge bar: cap/body/cap frame + fill extent
// PORT: + the param_1 style ramp)
// NOT WIRED: reached in-crate only through HudDraw::resolve_bar, and no
// host renders the fishing HUD draw list yet.
pub fn bar_frame(x: i32, y: i32, value: i32, segments: i32, style: i32) -> BarFrame {
    let fill_brightness = value * 0xff / BAR_VALUE_ONE;
    BarFrame {
        glyphs: [3, 4, 5],
        positions: [(x, y), (x + 8, y), (x + segments * 8 + 8, y)],
        body_scale: segments << 12,
        fill_len: segments * value * 8 / BAR_VALUE_ONE,
        fill_brightness,
        fill_rgb: bar_fill_rgb(style, fill_brightness),
    }
}

/// The **vertical** gauge bar behind [`HudDraw::PowerBar`] (the casting-power
/// meter): the same cap/body/cap frame rotated onto the y axis, with glyph ids
/// `0`/`1`/`2` and the body stretched vertically. It fills *upward* - the fill
/// quad grows from the bottom cap at `y + segments*8 + 8` back toward the top.
///
/// Unlike [`bar_frame`] this helper takes **no** style argument: retail's
/// `FUN_801d1a90` is a four-argument function that stores `0xbc` into the
/// red channel unconditionally, i.e. it is permanently the style-0 ramp.
// PORT: FUN_801d1a90 (vertical power bar: cap/body/cap frame + upward fill)
// NOT WIRED: as bar_frame - resolve_bar routes to it, nothing renders it.
pub fn power_bar_frame(x: i32, y: i32, value: i32, segments: i32) -> BarFrame {
    let end = y + segments * 8 + 8;
    let fill_brightness = value * 0xff / BAR_VALUE_ONE;
    BarFrame {
        glyphs: [0, 1, 2],
        positions: [(x, y), (x, y + 8), (x, end)],
        body_scale: segments << 12,
        fill_len: segments * value * 8 / BAR_VALUE_ONE,
        fill_brightness,
        fill_rgb: Some((BAR_FILL_STYLE0_RED, fill_brightness as u8, 0)),
    }
}

/// One of the driver tail's auxiliary one-shot animation timers
/// (`DAT_801d9160` / `DAT_801d915c` / `DAT_801d90f0`): zero = idle, seeded to
/// `1` to start, advanced by the frame step (`DAT_1f800393`) each frame its
/// animator reports active, and reset to zero when the animation expires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BannerTimer(pub i32);

impl BannerTimer {
    /// Seed the timer to `1` (the retail start value).
    pub fn start(&mut self) {
        self.0 = 1;
    }

    /// Force the timer idle (retail zeroes a timer to cancel its banner -
    /// e.g. the from-right banner cancels the from-left one while it runs).
    pub fn cancel(&mut self) {
        self.0 = 0;
    }

    /// `true` while the timer is running.
    pub fn is_active(&self) -> bool {
        self.0 != 0
    }

    /// Service one frame: while active, run `animator` on the current frame
    /// count; advance by `frame_step` if it drew, reset to idle if it
    /// expired. Returns the animator's draw output.
    // PORT: FUN_801cf3bc shared tail LAB_801d01a4 (banner-timer service loop)
    pub fn service<T>(
        &mut self,
        frame_step: i32,
        animator: impl FnOnce(i32) -> Option<T>,
    ) -> Option<T> {
        if self.0 == 0 {
            return None;
        }
        let out = animator(self.0);
        if out.is_some() {
            self.0 += frame_step.max(1);
        } else {
            self.0 = 0;
        }
        out
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
    fn banner_variants_share_the_slide_ramp_and_expire_together() {
        // All four banner animators ride `banner_slide`; the two mirrored ones
        // hold at the same 0xa0 centre as the from-left banner.
        let hold = 0x20;
        assert_eq!(banner_slide(hold), 0xa0);
        let miss = banner_miss_draw(hold).expect("active mid-hold");
        match miss {
            HudDraw::Glyph { id, x, .. } => {
                assert_eq!(id, 0x19);
                assert_eq!(x, 0x140 - 0xa0);
            }
            _ => panic!("miss banner is a glyph draw"),
        }
        let pair = banner_converge_draws(hold).expect("active mid-hold");
        match (pair[0], pair[1]) {
            (HudDraw::Glyph { x: a, id: ia, .. }, HudDraw::Glyph { x: b, id: ib, .. }) => {
                assert_eq!((ia, ib), (0xc, 0xc), "same glyph both sides");
                assert_eq!(a + b, 0x140, "mirrored about the screen centre");
            }
            _ => panic!("converge banner is a glyph pair"),
        }
        // Both expire exactly at the shared lifetime.
        assert!(banner_miss_draw(BANNER_FRAMES - 1).is_some());
        assert!(banner_miss_draw(BANNER_FRAMES).is_none());
        assert!(banner_converge_draws(BANNER_FRAMES).is_none());
    }

    #[test]
    fn digit_field_blanks_leading_zeros_and_right_aligns() {
        let cells = number_digit_cells(0, 100, 50, 42);
        let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
        assert_eq!(digits, vec![4, 2], "only significant digits are emitted");
        // Right-aligned in the 8-slot field: '4' lands in slot 6, '2' in 7.
        assert_eq!(cells[0].x, 100 + 6 * DIGIT_PITCH_NARROW);
        assert_eq!(cells[1].x, 100 + 7 * DIGIT_PITCH_NARROW);
        assert!(cells.iter().all(|c| c.y == 50));
    }

    #[test]
    fn digit_field_draws_a_lone_zero() {
        // The seeded last slot is what keeps a zero total visible.
        let cells = number_digit_cells(0, 0, 0, 0);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].digit, 0);
        assert_eq!(cells[0].x, 7 * DIGIT_PITCH_NARROW);
    }

    #[test]
    fn digit_field_style_selects_the_slot_pitch() {
        let wide = number_digit_cells(1, 0, 0, 7);
        assert_eq!(wide[0].x, 7 * DIGIT_PITCH_WIDE);
    }

    #[test]
    fn digit_field_fills_every_slot_at_eight_digits() {
        let cells = number_digit_cells(0, 0, 0, 12_345_678);
        let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
        assert_eq!(digits, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn bar_frames_span_their_segments_and_track_the_fill() {
        let segs = 8;
        let h = bar_frame(20, 40, BAR_VALUE_ONE, segs, 0);
        assert_eq!(h.glyphs, [3, 4, 5]);
        // Caps bracket the body along x; y is constant.
        assert_eq!(h.positions[0], (20, 40));
        assert_eq!(h.positions[2], (20 + segs * 8 + 8, 40));
        assert_eq!(h.fill_len, segs * 8, "full value fills every segment");
        assert_eq!(h.fill_brightness, 0xff);

        let v = power_bar_frame(20, 40, BAR_VALUE_ONE / 2, segs);
        assert_eq!(v.glyphs, [0, 1, 2]);
        // The vertical bar brackets along y instead, at a constant x.
        assert_eq!(v.positions[0], (20, 40));
        assert_eq!(v.positions[2], (20, 40 + segs * 8 + 8));
        assert_eq!(v.fill_len, segs * 8 / 2, "half value fills half the bar");
        assert_eq!(v.fill_brightness, 0x7f);
        assert_eq!(v.body_scale, segs << 12);

        // An empty bar still draws its frame, with nothing lit.
        let empty = bar_frame(0, 0, 0, segs, 0);
        assert_eq!((empty.fill_len, empty.fill_brightness), (0, 0));
    }

    #[test]
    fn bar_style_selects_the_fill_ramp_without_moving_geometry() {
        let segs = 8;
        let value = BAR_VALUE_ONE / 2; // brightness byte 0x7f
        let s0 = bar_frame(20, 40, value, segs, 0);
        let s1 = bar_frame(20, 40, value, segs, 1);

        // Style 0 holds the constant red against the ramp...
        assert_eq!(s0.fill_rgb, Some((BAR_FILL_STYLE0_RED, 0x7f, 0)));
        // ...style 1 runs the ramp against its own complement.
        assert_eq!(s1.fill_rgb, Some((0x7f, 0x80, 0)));
        // Blue is zero in both, and the geometry is identical.
        assert_eq!(s0.positions, s1.positions);
        assert_eq!(
            (s0.fill_len, s0.body_scale, s0.glyphs),
            (s1.fill_len, s1.body_scale, s1.glyphs)
        );

        // The complement tracks the ramp across its range.
        let full = bar_frame(0, 0, BAR_VALUE_ONE, segs, 1);
        assert_eq!(full.fill_rgb, Some((0xff, 0x00, 0)));
        let dark = bar_frame(0, 0, 0, segs, 1);
        assert_eq!(dark.fill_rgb, Some((0x00, 0xff, 0)));

        // Any other style jumps the colour stores entirely.
        assert_eq!(bar_frame(0, 0, value, segs, 2).fill_rgb, None);

        // The power bar is permanently the style-0 ramp.
        assert_eq!(
            power_bar_frame(0, 0, value, segs).fill_rgb,
            Some((BAR_FILL_STYLE0_RED, 0x7f, 0))
        );
    }

    #[test]
    fn hud_bar_variants_resolve_through_the_ported_helpers() {
        // The retail HUD uses style 0 for depth and style 1 for tension,
        // so the draw list exercises both ramps.
        let depth = HudDraw::Bar {
            style: 0,
            x: 0x10,
            y: 0x90,
            value: BAR_VALUE_ONE,
            step: 10,
        };
        let tension = HudDraw::Bar {
            style: 1,
            x: 0x10,
            y: 0xb0,
            value: BAR_VALUE_ONE,
            step: 10,
        };
        assert_eq!(
            depth.resolve_bar().unwrap(),
            bar_frame(0x10, 0x90, BAR_VALUE_ONE, 10, 0)
        );
        assert_eq!(
            tension.resolve_bar().unwrap().fill_rgb,
            Some((0xff, 0x00, 0))
        );

        let power = HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: 0,
            step: 0xc,
        };
        assert_eq!(power.resolve_bar().unwrap().glyphs, [0, 1, 2]);

        // Non-bar variants carry no frame.
        assert!(
            HudDraw::Caption {
                text: HudCaption::LuresLeft,
                x: 0,
                y: 0,
            }
            .resolve_bar()
            .is_none()
        );
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

    #[test]
    fn persistent_hud_caps_points_and_gates_the_rod_label() {
        let d = persistent_hud_draws(2_000_000, 1234, 2, 7);
        // Point total renders capped; the best-catch row is uncapped input.
        assert!(d.contains(&HudDraw::Number {
            x: 0x32,
            y: 0x16,
            value: FISH_POINTS_CAP,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0x32,
            y: 0x08,
            value: 1234,
            brightness: HUD_BRIGHTNESS
        }));
        // Rod index 2 draws its label; the count row shows the lure count.
        assert!(d.contains(&HudDraw::Caption {
            text: HudCaption::RodName(2),
            x: 0x98,
            y: 0x0c
        }));
        assert!(d.contains(&HudDraw::Count {
            value: 7,
            digits: 4,
            x: 0x100,
            y: 0x0c
        }));
        assert_eq!(lure_item_id(2), 0x9f);
        // An out-of-range rod index draws no label but keeps the count row.
        let d = persistent_hud_draws(0, 0, 3, 0);
        assert!(!d.iter().any(|x| matches!(
            x,
            HudDraw::Caption {
                text: HudCaption::RodName(_),
                ..
            }
        )));
        assert!(d.contains(&HudDraw::Caption {
            text: HudCaption::LuresLeft,
            x: 0xf3,
            y: 0x0c
        }));
    }

    #[test]
    fn catch_hud_length_and_percent_arithmetic() {
        // record 812 -> 512 past the strike base -> 512*100 >> 9 = 100 tenths;
        // extent 1024 -> (1024 >> 8) >> 1 = 2 tenths -> 102 = "10.2".
        assert_eq!(length_display(812, 1024), 102);
        assert_eq!(extent_display(1024), 2);
        // Below the strike base the record term clamps to zero.
        assert_eq!(length_display(0, 0), 0);
        assert_eq!(length_display(299, 0), 0);
        // Negative extent clamps to zero (with retail's toward-zero rounding).
        assert_eq!(extent_display(-1), 0);
        assert_eq!(extent_display(-1024), 0);
        // Cast power percent: percent of the 0x1000 meter ceiling.
        assert_eq!(cast_power_percent(0x1000), 100);
        assert_eq!(cast_power_percent(0x800), 50);
        assert_eq!(cast_power_percent(0x20), 0);
        // The HUD splits the length into whole + tenths digits.
        let s = CatchHudState {
            record: 812,
            line_extent: 1024,
            cast_power: 0x800,
            ..Default::default()
        };
        let d = catch_hud_draws(&s);
        assert!(d.contains(&HudDraw::Number {
            x: 0xda,
            y: 0x30,
            value: 10,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0xe8,
            y: 0x30,
            value: 2,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::Number {
            x: 0xe4,
            y: 0xb0,
            value: 50,
            brightness: HUD_BRIGHTNESS
        }));
        assert!(d.contains(&HudDraw::PowerBar {
            x: 0x120,
            y: 0x40,
            power: 0x800,
            step: 0xc
        }));
    }

    #[test]
    fn catch_hud_gauge_block_is_gated_on_hook() {
        let mut s = CatchHudState {
            depth: 0x300,
            tension: 0x700,
            ..Default::default()
        };
        // Not hooked: no gauge bars.
        assert!(
            !catch_hud_draws(&s)
                .iter()
                .any(|d| matches!(d, HudDraw::Bar { .. }))
        );
        // Hooked: the depth (style 0) + tension (style 1) bars appear.
        s.gauges_visible = true;
        let d = catch_hud_draws(&s);
        assert!(d.contains(&HudDraw::Bar {
            style: 0,
            x: 0x10,
            y: 0x90,
            value: 0x300,
            step: 10
        }));
        assert!(d.contains(&HudDraw::Bar {
            style: 1,
            x: 0x10,
            y: 0xb0,
            value: 0x700,
            step: 10
        }));
    }

    fn glyph_x(d: HudDraw) -> i32 {
        match d {
            HudDraw::Glyph { x, .. } => x,
            other => panic!("expected a glyph, got {other:?}"),
        }
    }

    #[test]
    fn banner_slide_ramp_and_lifetime() {
        // From the left: slide in at 8 px/frame, hold at 0xa0, slide off.
        assert_eq!(glyph_x(banner_from_left_draw(1).unwrap()), 8);
        assert_eq!(glyph_x(banner_from_left_draw(0x13).unwrap()), 0x98);
        assert_eq!(glyph_x(banner_from_left_draw(0x14).unwrap()), 0xa0);
        assert_eq!(glyph_x(banner_from_left_draw(0x8b).unwrap()), 0xa0);
        // The slide-off ramp joins continuously at the hold value.
        assert_eq!(glyph_x(banner_from_left_draw(0x8c).unwrap()), 0xa0);
        assert_eq!(glyph_x(banner_from_left_draw(0xc7).unwrap()), 0x278);
        assert!(banner_from_left_draw(0xc8).is_none());
        // From the right: the mirrored trajectory, holding at the same x.
        assert_eq!(glyph_x(banner_from_right_draw(1).unwrap()), 0x140 - 8);
        assert_eq!(glyph_x(banner_from_right_draw(0x20).unwrap()), 0xa0);
        assert!(banner_from_right_draw(0xc8).is_none());
    }

    #[test]
    fn strike_splash_rises_and_fades() {
        // Fade-in at 8/frame, hold at 0x80, fade-out from frame 0x88.
        assert_eq!(splash_brightness(1), 8);
        assert_eq!(splash_brightness(0x10), 0x80);
        assert_eq!(splash_brightness(0x87), 0x80);
        assert_eq!(splash_brightness(0x90), 0x40);
        // The pair rises one pixel every 32 frames from y = 0x50.
        let [a, b] = strike_splash_draws(0x40).unwrap();
        match (a, b) {
            (
                HudDraw::Glyph {
                    id: 0x416,
                    x: 0xa0,
                    y,
                    brightness,
                    ..
                },
                HudDraw::Glyph {
                    id: 0x816,
                    y: y2,
                    brightness: b2,
                    ..
                },
            ) => {
                assert_eq!(y, 0x50 - 2);
                assert_eq!((y, brightness), (y2, b2));
                assert_eq!(brightness, 0x80);
            }
            other => panic!("unexpected splash pair {other:?}"),
        }
        // Expires exactly at the lifetime end (brightness reaches 0 there).
        assert!(strike_splash_draws(SPLASH_FRAMES - 1).is_some());
        assert!(strike_splash_draws(SPLASH_FRAMES).is_none());
    }

    #[test]
    fn banner_timer_advances_while_active_and_resets() {
        let mut t = BannerTimer::default();
        assert!(!t.is_active());
        assert!(t.service(2, banner_from_left_draw).is_none());
        t.start();
        assert!(t.is_active());
        // Each serviced frame draws and advances by the frame step.
        assert!(t.service(2, banner_from_left_draw).is_some());
        assert_eq!(t.0, 3);
        // Run it out: the animator expires and the timer resets to idle.
        let mut frames = 0;
        while t.service(2, banner_from_left_draw).is_some() {
            frames += 1;
            assert!(frames < 1000, "timer failed to expire");
        }
        assert!(!t.is_active());
        // Cancel mirrors the tail's cross-banner zeroing.
        t.start();
        t.cancel();
        assert!(!t.is_active());
    }
}
