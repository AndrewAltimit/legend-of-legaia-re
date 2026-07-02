//! Clean-room **casino slot-machine** rules engine.
//!
//! A port of the confirmed numeric kernels of the slot-machine overlay (PROT
//! 0975, `data\OTHER4`) - the reel-strip permutation builder, the slot LCG,
//! the net-take-bracketed feature roll, the reel-landing search, and the
//! payline / payout / bonus-round evaluation - composed into an interactive
//! session. It consumes spin / stop input and produces reel outcomes and a
//! running coin balance the host commits back to the casino coin bank
//! (`_DAT_800845A4`) on cash-out.
//!
//! What is **Confirmed** (formula pinned in
//! [`docs/subsystems/minigame-slot-machine.md`](../../../docs/subsystems/minigame-slot-machine.md)):
//! - the slot LCG `x = x*5 + 1` with the 16-bit halves folded
//!   (`FUN_801d30cc`, [`SlotRng`]);
//! - the 20-slot reel-strip build: per slot draw RNG mod `0x14`, probe forward
//!   (`+0xd` / `+1`) to the first unused position, place symbol id `slot/2`
//!   (`FUN_801cf0d8` case 0, [`build_strip`]);
//! - the per-spin feature roll: jitter `rand%5`, normal-mode target
//!   `rand%6 + 2`, one optional widen roll (`rand%100 + 200` when
//!   `DAT_801d3790` is set), and `rand % N == 0` feature-entry rolls whose
//!   denominators are bracketed on the **net-take counter** `DAT_801d3d40` -
//!   `< 1000` → `700`/`500`, `1001..=1999` → `350`/`250`, `> 2000` →
//!   `175`/`125`, plus a flat `widen+600` mode-3 roll (`FUN_801d258c`,
//!   [`feature_roll`]);
//! - the spin charge: a flat [`SPIN_COST_NORMAL`] = 3 coins (the overlay's
//!   "insert 3 coins" help text), [`SPIN_COST_FEATURE`] = 1 coin in feature
//!   modes 4..=6, accruing `+6` / `+1` into the net-take counter;
//! - the net-take counter itself: `+6`/`+1` per spin, **minus** each
//!   bonus-round payout, never reset during a session - the machine gets
//!   *more* generous as its net take rises;
//! - the entry init (`FUN_801cec94`): balance seeded by assignment from the
//!   casino coin bank (default [`ENTRY_DEFAULT_BALANCE`] = 70 when the
//!   battle-return flag `_DAT_8007B8B8` is clear - a dev-launch fallback),
//!   slot LCG seeded with the literal [`ENTRY_LCG_SEED`];
//! - the per-reel stop plan: depth `rand%3 + 2` targeting the normal-mode
//!   symbol in mode 0, depth `(rand&3) + 6` targeting the jackpot symbols
//!   `9` / `8` in the reach modes 1 / 2 (`FUN_801d2114`, [`stop_plan`]);
//! - the landing search: walk up to `depth` rows ahead for the target symbol,
//!   else stop on the next natural row (`FUN_801d2440`, [`land_row`]);
//! - the win evaluation: three paylines checked all-equal on the display
//!   strip, highest-value line kept, normal payout =
//!   `payout_table[symbol]` ([`legaia_asset::slot_payout`]), jackpot symbols
//!   `9` / `8` kick off a bonus round of 3 / 1 free spins, and a bonus-round
//!   win pays the *product* of the three matched `(value - 0xf)` factors
//!   (`FUN_801d13e8`, [`SlotMachine::evaluate_spin`]);
//! - the coin economy: the playing balance is overlay-local, capped at
//!   `9_999_999` in the tally path, and *assigned* to the coin bank on
//!   cash-out (state `100`), not debited per spin.
//!
//! What is an **engine-side reconstruction** (marked at each site): the
//! spin-up velocity/timer magnitudes (visual pacing, not pinned), and the
//! BIOS-`rand` feature stream substituted with a second deterministic
//! [`BiosRand`] LCG so replays stay bit-identical.
//! Feature modes 3 (hot) and 5 (hold) are documented but folded to the
//! normal landing plan here - their bonus-strip value targeting is not
//! modeled (the engine models the normal strip only; the bonus product
//! payout uses `symbol + 1` in place of the bonus strip's `value - 0xf`).
//!
//! Chain: retail `FUN_801cf0d8` (reel SM) -> `FUN_801d258c` (feature roll) ->
//! `FUN_801d2114` / `FUN_801d2440` (stop) -> `FUN_801d13e8` (win eval).

use crate::levelup::BiosRand;
use legaia_asset::slot_payout::SlotPayoutTable;

/// Reels on the machine.
pub const REEL_COUNT: usize = 3;
/// Symbols per reel strip (`0x14`).
pub const STRIP_LEN: usize = 20;
/// Distinct symbol ids (`slot/2` over the 20-slot strip → `0..=9`).
pub const SYMBOL_COUNT: usize = 10;
/// Fixed-point reel wrap (`STRIP_LEN << 8`; positions wrap mod `0x1400`).
pub const REEL_WRAP: i32 = (STRIP_LEN as i32) << 8;
/// Balance cap applied in the payout tally path (`9999999`).
pub const BALANCE_CAP: i32 = 9_999_999;
/// The state-1 "not enough coins" gate: a spin needs at least 3 coins banked
/// (`DAT_801d4114 < 3` routes to the state-`0x5a` prompt) - applied in every
/// feature mode, even though a feature spin only costs 1.
pub const MIN_SPIN_BALANCE: i32 = 3;
/// Flat coin cost of a normal spin (feature modes 0..=3): `DAT_801d4114 -= 3`
/// in state `1`. All three paylines always play - there is no bet-line
/// selection ("insert 3 coins" is the whole bet).
pub const SPIN_COST_NORMAL: i32 = 3;
/// Coin cost of a spin during feature modes 4..=6 (`DAT_801d4114 -= 1`).
pub const SPIN_COST_FEATURE: i32 = 1;
/// Net-take accrual per normal spin (`DAT_801d3d40 += 6` - twice the coins
/// charged).
pub const NET_TAKE_NORMAL_SPIN: i32 = 6;
/// Net-take accrual per feature-mode spin (`DAT_801d3d40 += 1`).
pub const NET_TAKE_FEATURE_SPIN: i32 = 1;
/// Entry-init balance fallback (`FUN_801cec94`): when the battle-return flag
/// `_DAT_8007B8B8` is clear (the overlay launched outside the casino door
/// path), the balance defaults to `0x46` = 70 coins instead of the coin-bank
/// copy.
pub const ENTRY_DEFAULT_BALANCE: i32 = 70;
/// The literal seed `FUN_801cec94` writes into the slot LCG (`DAT_801d3c80`)
/// on every machine entry.
pub const ENTRY_LCG_SEED: u32 = 0x6C0A_2AF0;
/// Free spins granted when the jackpot symbol `9` matches (`FUN_801d13e8`).
pub const BONUS_SPINS_JACKPOT: i32 = 3;
/// Free spins granted when the bonus symbol `8` matches.
pub const BONUS_SPINS_BONUS: i32 = 1;
/// The probe step used for the primary strip array (`(pos + 0xd) % 0x14`).
pub const STRIP_PROBE_PRIMARY: usize = 0xd;
/// The probe step used for the secondary strip array (`(pos + 1) % 0x14`).
pub const STRIP_PROBE_SECONDARY: usize = 1;

/// The slot machine's own deterministic LCG over `DAT_801d3c80`:
/// `x = x*5 + 1`, then the 16-bit halves are folded
/// (`x = (x << 16) + (x >> 16)`). Reel outcomes are reproducible from the
/// seed state.
// PORT: FUN_801d30cc (slot LCG)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRng {
    state: u32,
}

impl SlotRng {
    /// Seed the generator (retail reseeds from BIOS `rand` at machine init).
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Advance and return the next state word.
    pub fn next_u32(&mut self) -> u32 {
        let x = self.state.wrapping_mul(5).wrapping_add(1);
        self.state = (x << 16).wrapping_add(x >> 16);
        self.state
    }
}

/// Build one 20-slot reel strip (`FUN_801cf0d8` case 0): for each of the 20
/// slots draw a fresh RNG value, reduce it mod `0x14`, and probe forward by
/// `probe_step` until an unused position is found; place symbol id `slot/2`
/// there. A collision-resolving permutation that scatters each symbol id
/// (two strip positions each) around the reel. `probe_step` is
/// [`STRIP_PROBE_PRIMARY`] for one retail array and
/// [`STRIP_PROBE_SECONDARY`] for the other; both are coprime with 20 so the
/// probe always terminates.
// PORT: FUN_801cf0d8 case 0 (reel-strip permutation build)
pub fn build_strip(rng: &mut SlotRng, probe_step: usize) -> [u8; STRIP_LEN] {
    let mut strip = [u8::MAX; STRIP_LEN];
    for slot in 0..STRIP_LEN {
        let mut pos = (rng.next_u32() as usize) % STRIP_LEN;
        while strip[pos] != u8::MAX {
            pos = (pos + probe_step) % STRIP_LEN;
        }
        strip[pos] = (slot / 2) as u8;
    }
    strip
}

/// The per-spin roll (`FUN_801d258c`): the landing jitter, the normal-mode
/// target symbol, and - when no feature is already active - whether a
/// feature mode was entered this spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinRoll {
    /// Per-spin landing jitter (`DAT_801d4134 = rand%5`).
    pub jitter: i32,
    /// Normal-mode target symbol (`DAT_801d3cb8 = rand%6 + 2`).
    pub normal_target: u8,
    /// Feature mode entered this spin (`1` reach-jackpot / `2` reach-bonus /
    /// `3` hot), or `None`.
    pub entered_mode: Option<u8>,
}

/// The net-take-bracketed feature-entry denominators (`FUN_801d258c`).
///
/// Bracketed on the net-take counter `DAT_801d3d40`, and the direction is
/// the opposite of a house-edge squeeze: a *low* net take gets the large
/// denominators (features rare), a *high* one gets the small (features
/// roughly 4x more likely once 2000+ has accrued). The machine pays back
/// what it has taken. The exact values `1000` and `2000` fall in no bracket
/// (`< 1000` / `1001..=1999` / `> 2000` in the dump) - those spins roll only
/// the mode-3 denominator.
fn feature_denominators(net_take: i32) -> Option<(u32, u32)> {
    if net_take < 1000 {
        Some((700, 500))
    } else if (1001..=1999).contains(&net_take) {
        Some((0x15e, 0xfa)) // 350, 250
    } else if net_take > 2000 {
        Some((0xaf, 0x7d)) // 175, 125
    } else {
        None
    }
}

/// Run the per-spin feature roll (`FUN_801d258c`): seed the landing jitter
/// (`rand%5`) and normal-mode target (`rand%6 + 2`), roll the widen amount
/// once (`rand%100 + 200`) when `richer_odds` (`DAT_801d3790`) is set, then -
/// only when no feature is active (`feature_mode == 0`) - roll the net-take
/// bracket's two `rand % (widen + N) == 0` probabilities (mode 1 then mode
/// 2) and finally the flat `rand % (widen + 600) == 0` mode-3 roll. Draw
/// order matches the dump exactly.
// PORT: FUN_801d258c (per-spin feature roll, net-take-bracketed odds)
pub fn feature_roll(
    rand: &mut BiosRand,
    net_take: i32,
    feature_mode: u8,
    richer_odds: bool,
) -> SpinRoll {
    let jitter = (rand.next_u15() % 5) as i32;
    let normal_target = (rand.next_u15() % 6 + 2) as u8;
    let widen: u32 = if richer_odds {
        (rand.next_u15() % 100 + 200) as u32
    } else {
        0
    };
    let mut entered_mode = None;
    if feature_mode == 0 {
        if let Some((d1, d2)) = feature_denominators(net_take) {
            if (rand.next_u15() as u32).is_multiple_of(widen + d1) {
                entered_mode = Some(1); // reach / jackpot tease (target symbol 9)
            }
            // The mode-2 roll draws even when mode 1 already hit.
            if (rand.next_u15() as u32).is_multiple_of(widen + d2) && entered_mode.is_none() {
                entered_mode = Some(2); // reach / bonus tease (target symbol 8)
            }
        }
        if (rand.next_u15() as u32).is_multiple_of(widen + 600) && entered_mode.is_none() {
            entered_mode = Some(3); // hot mode
        }
    }
    SpinRoll {
        jitter,
        normal_target,
        entered_mode,
    }
}

/// The per-reel stop plan (`FUN_801d2114`): how many rows ahead to search
/// (`depth`) and which symbol to bias toward (`target`), keyed on the active
/// feature mode.
///
/// Confirmed: mode `0` scans `rand%3 + 2` rows for the normal-mode target;
/// modes `1` / `2` scan `(rand&3) + 6` rows for the jackpot symbols `9` / `8`;
/// mode `4` (guaranteed-hit) drives the reel to a winning symbol. Modes `3`
/// (hot, bonus-strip value targeting) and `5` (hold) are folded to the
/// normal plan here (reconstruction - see the module docs); mode `6` (bonus
/// round) reuses the guaranteed plan so free spins land the multiplier line,
/// matching the bonus round paying every spin. Draws from the slot LCG -
/// retail uses it for reel-landing selection (the BIOS-`rand` stream feeds
/// only the feature/jitter rolls).
// PORT: FUN_801d2114 (per-reel stop: target symbol + search depth by feature mode)
pub fn stop_plan(
    rng: &mut SlotRng,
    feature_mode: u8,
    normal_target: u8,
    guarantee_target: Option<u8>,
) -> (usize, u8) {
    match feature_mode {
        1 => ((((rng.next_u32() & 3) + 6) as usize), 9),
        2 => ((((rng.next_u32() & 3) + 6) as usize), 8),
        4 | 6 => match guarantee_target {
            // Drive the reel all the way to the guaranteed symbol.
            Some(t) => (STRIP_LEN, t),
            None => (STRIP_LEN, normal_target),
        },
        // Modes 0, 3, 5 (and anything unmapped): the normal scan.
        _ => (((rng.next_u32() % 3 + 2) as usize), normal_target),
    }
}

/// The reel landing search (`FUN_801d2440`): starting from `from_row`, walk
/// up to `depth` rows forward looking for `target` on the display strip; if
/// found, return that row (the symbol lands on the payline), otherwise
/// return the next natural row - no forced result.
// PORT: FUN_801d2440 (landing search: find target within depth, else next row)
pub fn land_row(strip: &[u8; STRIP_LEN], from_row: usize, depth: usize, target: u8) -> usize {
    for d in 0..=depth {
        let row = (from_row + d) % STRIP_LEN;
        if strip[row] == target {
            return row;
        }
    }
    (from_row + 1) % STRIP_LEN
}

/// The outcome of one evaluated spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinResult {
    /// Winning payline index (`0` middle / `1` top / `2` bottom), or `None`.
    pub line: Option<usize>,
    /// Winning symbol id, or `None`.
    pub symbol: Option<u8>,
    /// Coins credited for this spin (post-eval, pre-tally).
    pub payout: i32,
    /// `true` when this spin's win kicked off the bonus round (symbols 8/9).
    pub bonus_triggered: bool,
    /// `true` when this spin was a bonus-round free spin (product payout).
    pub bonus_spin: bool,
}

/// Which phase the machine is in. Mirrors the `DAT_801d3c84` state word at
/// the granularity the host drives (init/attract/spin/stop/payout; the
/// cash-out submenu is host UI - [`SlotMachine::cash_out`] is the state-100
/// commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPhase {
    /// Attract / idle (state `1`): waiting for a bet.
    Idle,
    /// Spin-up (state `2`): reels accelerate until the spin timer expires.
    Spinning,
    /// Stopping (state `3`): reels stop one per stop input.
    Stopping,
    /// Payout tally (state `4`): a result is latched for collection.
    Payout,
    /// Cash-out committed (state `100`): the session is over.
    CashedOut,
}

/// A live slot-machine session: the reel strips, the two RNG streams, the
/// feature state, and the overlay-local coin balance. This is the
/// host-facing composition of the confirmed kernels (`FUN_801cf0d8` in
/// miniature).
#[derive(Debug, Clone)]
pub struct SlotMachine {
    payouts: SlotPayoutTable,
    /// Slot LCG (`DAT_801d3c80`): strips + (via [`stop_plan`]) landings.
    rng: SlotRng,
    /// Feature-roll stream. Retail uses BIOS `rand` here; the engine
    /// substitutes the same [`BiosRand`] LCG seeded alongside the slot LCG so
    /// replays stay deterministic (reconstruction - see the module docs).
    rand: BiosRand,
    /// Display reel strips (`DAT_801d3d50`): win eval + render read these.
    strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Live fixed-point reel positions (`DAT_801d3cc0..`), wrap mod `0x1400`.
    reel_pos: [i32; REEL_COUNT],
    /// Reel velocities during a spin (`DAT_801d3cd0..`). Magnitudes are
    /// visual pacing, not pinned - host-rate constants.
    reel_vel: [i32; REEL_COUNT],
    /// Landed payline row per reel (`None` while still spinning).
    stopped: [Option<usize>; REEL_COUNT],
    /// Spin-up timer (`DAT_801d3c90`), frames until stopping is allowed.
    spin_timer: i32,
    phase: SlotPhase,
    /// Feature mode (`DAT_801d3cac`): `0` normal … `6` bonus round.
    feature_mode: u8,
    /// Bonus free-spin / multiplier counter (`DAT_801d3cb0`).
    bonus_spins: i32,
    /// Net-take heat counter (`DAT_801d3d40`): `+6` per normal spin, `+1` per
    /// feature spin, minus each bonus-round payout. The feature-odds bracket
    /// input; never reset during a session.
    net_take: i32,
    /// Normal-mode target symbol for this spin (`DAT_801d3cb8`).
    normal_target: u8,
    /// Per-spin landing jitter (`DAT_801d4134`). Carried for fidelity of the
    /// roll stream; the engine's landing keeps rows exact (the retail `*0x10`
    /// nudge is sub-row presentation).
    jitter: i32,
    /// Overlay-local playing balance (`DAT_801d4114`).
    balance: i32,
    /// Richer-odds flag (`DAT_801d3790`).
    richer_odds: bool,
    /// The last evaluated spin, latched through [`SlotPhase::Payout`].
    last_result: Option<SpinResult>,
}

/// Spin-up frames before the reels may be stopped (visual pacing constant;
/// the retail `DAT_801d3c90` magnitude is not pinned).
pub const SPIN_UP_FRAMES: i32 = 30;
/// Per-reel spin velocities (visual pacing constants, staggered like the
/// retail ramp so the reels visibly desynchronize).
pub const SPIN_VELOCITY: [i32; REEL_COUNT] = [0x60, 0x70, 0x80];

impl SlotMachine {
    /// A fresh machine over the parsed payout table, seeded (retail reseeds
    /// from BIOS `rand` at init) and holding `balance` coins loaded from the
    /// casino coin bank.
    pub fn new(payouts: SlotPayoutTable, seed: u32, balance: i32) -> Self {
        let mut rng = SlotRng::new(seed);
        // Retail builds two parallel strip arrays (probe steps +0xd / +1)
        // and clones one into the display copy the win eval + renderer read.
        let strips = [
            build_strip(&mut rng, STRIP_PROBE_PRIMARY),
            build_strip(&mut rng, STRIP_PROBE_SECONDARY),
            build_strip(&mut rng, STRIP_PROBE_PRIMARY),
        ];
        Self {
            payouts,
            rng,
            rand: BiosRand::new(seed ^ 0x5A5A_5A5A),
            strips,
            reel_pos: [0; REEL_COUNT],
            reel_vel: [0; REEL_COUNT],
            stopped: [None; REEL_COUNT],
            spin_timer: 0,
            phase: SlotPhase::Idle,
            feature_mode: 0,
            bonus_spins: 0,
            net_take: 0,
            normal_target: 2,
            jitter: 0,
            balance: balance.clamp(0, BALANCE_CAP),
            richer_odds: false,
            last_result: None,
        }
    }

    /// Current phase.
    pub fn phase(&self) -> SlotPhase {
        self.phase
    }

    /// Overlay-local playing balance (`DAT_801d4114`).
    pub fn balance(&self) -> i32 {
        self.balance
    }

    /// Active feature mode (`DAT_801d3cac`; `0` normal, `6` bonus round).
    pub fn feature_mode(&self) -> u8 {
        self.feature_mode
    }

    /// Remaining bonus free spins (`DAT_801d3cb0`).
    pub fn bonus_spins(&self) -> i32 {
        self.bonus_spins
    }

    /// Net-take heat counter (`DAT_801d3d40`; the feature-odds bracket
    /// input).
    pub fn net_take(&self) -> i32 {
        self.net_take
    }

    /// The display strips (win eval + render source).
    pub fn strips(&self) -> &[[u8; STRIP_LEN]; REEL_COUNT] {
        &self.strips
    }

    /// The last evaluated spin (latched through [`SlotPhase::Payout`]).
    pub fn last_result(&self) -> Option<SpinResult> {
        self.last_result
    }

    /// The symbol currently on the payline of `reel` (`(pos >> 8) mod 0x14`
    /// on the display strip).
    pub fn payline_symbol(&self, reel: usize) -> u8 {
        let row = self.payline_row(reel);
        self.strips[reel][row]
    }

    /// The payline row index of `reel`.
    pub fn payline_row(&self, reel: usize) -> usize {
        ((self.reel_pos[reel] >> 8) as usize) % STRIP_LEN
    }

    /// How many reels are stopped this spin (`DAT_801d3d2c`).
    pub fn reels_stopped(&self) -> usize {
        self.stopped.iter().filter(|s| s.is_some()).count()
    }

    /// `true` when the spin timer has expired and stop inputs are accepted.
    pub fn can_stop(&self) -> bool {
        self.phase == SlotPhase::Stopping
    }

    /// The coin cost of the next spin: flat [`SPIN_COST_NORMAL`] in modes
    /// 0..=3, [`SPIN_COST_FEATURE`] in feature modes 4..=6 (there is no
    /// bet-line selection - all three paylines always play).
    pub fn spin_cost(&self) -> i32 {
        if (4..=6).contains(&self.feature_mode) {
            SPIN_COST_FEATURE
        } else {
            SPIN_COST_NORMAL
        }
    }

    /// `true` when a spin is accepted: idle and the balance clears the
    /// state-1 "not enough coins" gate (applied in every mode - retail
    /// checks `< 3` before looking at the feature mode, so even a 1-coin
    /// feature spin needs 3 banked).
    pub fn can_spin(&self) -> bool {
        self.phase == SlotPhase::Idle && self.balance >= MIN_SPIN_BALANCE
    }

    /// This spin's landing jitter (`DAT_801d4134`; sub-row presentation
    /// nudge - carried for roll-stream fidelity).
    pub fn jitter(&self) -> i32 {
        self.jitter
    }

    /// Charge the bet and start a spin (state `1` → `2`): subtract the flat
    /// spin cost (3 coins, or 1 during feature modes 4..=6 - a bonus "free
    /// spin" still costs 1), accrue the net take (`+6` / `+1`), run the
    /// per-spin feature roll, ramp the reels, and arm the spin timer.
    /// Returns `false` (no-op) when not idle or the balance is under the
    /// 3-coin gate.
    // PORT: FUN_801cf0d8 states 1-2 (bet charge + spin-up)
    pub fn spin(&mut self) -> bool {
        if !self.can_spin() {
            return false;
        }
        let feature_spin = (4..=6).contains(&self.feature_mode);
        self.balance = (self.balance - self.spin_cost()).max(0);
        self.net_take += if feature_spin {
            NET_TAKE_FEATURE_SPIN
        } else {
            NET_TAKE_NORMAL_SPIN
        };
        let roll = feature_roll(
            &mut self.rand,
            self.net_take,
            self.feature_mode,
            self.richer_odds,
        );
        self.jitter = roll.jitter;
        self.normal_target = roll.normal_target;
        if let Some(mode) = roll.entered_mode {
            self.feature_mode = mode;
        }
        self.stopped = [None; REEL_COUNT];
        self.reel_vel = SPIN_VELOCITY;
        self.spin_timer = SPIN_UP_FRAMES;
        self.last_result = None;
        self.phase = SlotPhase::Spinning;
        true
    }

    /// Advance one frame: reels advance by their velocities (wrapping mod
    /// `0x1400`, the tail of `FUN_801cf0d8`), and the spin timer counts down
    /// into the stopping state.
    // PORT: FUN_801cf0d8 tail (reel position advance) + state 2 (spin timer)
    pub fn tick(&mut self) {
        for reel in 0..REEL_COUNT {
            if self.stopped[reel].is_none() {
                self.reel_pos[reel] =
                    (self.reel_pos[reel] + self.reel_vel[reel]).rem_euclid(REEL_WRAP);
            }
        }
        if self.phase == SlotPhase::Spinning {
            self.spin_timer -= 1;
            if self.spin_timer <= 0 {
                self.phase = SlotPhase::Stopping;
            }
        }
    }

    /// Stop reel `reel` (a Stop input in state `3`): plan the stop for the
    /// active feature mode, run the landing search from the live row, and
    /// snap the reel. Once all three reels are stopped the spin is evaluated
    /// and the machine moves to [`SlotPhase::Payout`]. Returns `false` when
    /// stopping isn't allowed or the reel is already stopped.
    // PORT: FUN_801cf0d8 state 3 (per-reel stop + all-stopped -> win eval)
    pub fn stop_reel(&mut self, reel: usize) -> bool {
        if self.phase != SlotPhase::Stopping || reel >= REEL_COUNT || self.stopped[reel].is_some() {
            return false;
        }
        // Guaranteed / bonus modes drive later reels to the first reel's
        // landed symbol so the line connects.
        let guarantee = self
            .stopped
            .iter()
            .flatten()
            .next()
            .map(|&row| self.strips[0][row]);
        let (depth, target) = stop_plan(
            &mut self.rng,
            self.feature_mode,
            self.normal_target,
            guarantee,
        );
        let from_row = self.payline_row(reel);
        let row = land_row(&self.strips[reel], from_row, depth, target);
        self.reel_pos[reel] = (row as i32) << 8;
        self.reel_vel[reel] = 0;
        self.stopped[reel] = Some(row);
        if self.reels_stopped() == REEL_COUNT {
            let result = self.evaluate_spin();
            self.last_result = Some(result);
            self.phase = SlotPhase::Payout;
        }
        true
    }

    /// Stop the leftmost still-spinning reel (host convenience for a single
    /// stop button; retail maps three pad bits to the three reels).
    pub fn stop_next_reel(&mut self) -> bool {
        (0..REEL_COUNT).any(|r| self.stopped[r].is_none() && self.stop_reel(r))
    }

    /// Evaluate the stopped spin (`FUN_801d13e8`): outside a bonus round,
    /// check all three paylines all-three-equal on the display strips, keep
    /// the highest-value line, pay `payout_table[symbol]`, and trigger the
    /// bonus round on the jackpot symbols. During a bonus round every spin
    /// pays the *product* of the three payline factors unconditionally (no
    /// equality check - the mode-6 stop plan drives the line together) and
    /// the payout is subtracted from the net-take counter.
    // PORT: FUN_801d13e8 (win evaluation + payout lookup + bonus trigger)
    fn evaluate_spin(&mut self) -> SpinResult {
        let rows: [usize; REEL_COUNT] = core::array::from_fn(|r| self.stopped[r].unwrap_or(0));
        let bonus_spin = self.feature_mode == 6 && self.bonus_spins > 0;
        if bonus_spin {
            // Bonus round: the product of the three payline `(value - 0xf)`
            // factors, no equality gate. The bonus strip carries values
            // `0x10..=0x19`, i.e. symbol id + 0x10, so each factor is
            // `symbol + 1` (1..=10).
            let payout = (0..REEL_COUNT)
                .map(|r| self.strips[r][rows[r]] as i32 + 1)
                .product::<i32>();
            let all_equal =
                (1..REEL_COUNT).all(|r| self.strips[r][rows[r]] == self.strips[0][rows[0]]);
            self.net_take -= payout;
            self.bonus_spins -= 1;
            if self.bonus_spins <= 0 {
                self.feature_mode = 0;
            }
            return SpinResult {
                line: Some(0),
                symbol: all_equal.then(|| self.strips[0][rows[0]]),
                payout,
                bonus_triggered: false,
                bonus_spin: true,
            };
        }
        // Payline row offsets: 0 = middle (the payline row itself), 1 = top
        // (-1), 2 = bottom (+1) - the ±1 row reads in the dump. All three
        // lines always play.
        const LINE_OFFSETS: [isize; 3] = [0, -1, 1];
        let mut best: Option<(usize, u8, i32)> = None;
        for (line, &off) in LINE_OFFSETS.iter().enumerate() {
            let sym = |r: usize| {
                let row = (rows[r] as isize + off).rem_euclid(STRIP_LEN as isize) as usize;
                self.strips[r][row]
            };
            let (a, b, c) = (sym(0), sym(1), sym(2));
            if a == b && b == c {
                let value = self.payouts.payout(a).unwrap_or(0) as i32;
                if best.map(|(_, _, v)| value > v).unwrap_or(true) {
                    best = Some((line, a, value));
                }
            }
        }
        let mut result = SpinResult {
            line: best.map(|(l, _, _)| l),
            symbol: best.map(|(_, s, _)| s),
            payout: best.map(|(_, _, v)| v).unwrap_or(0),
            bonus_triggered: false,
            bonus_spin,
        };
        if let Some((_, sym, _)) = best {
            if legaia_asset::slot_payout::BONUS_SYMBOL_IDS.contains(&sym) {
                // Jackpot symbols kick off the bonus round: 3 free spins for
                // id 9, 1 for id 8.
                self.feature_mode = 6;
                self.bonus_spins = if sym == 9 {
                    BONUS_SPINS_JACKPOT
                } else {
                    BONUS_SPINS_BONUS
                };
                result.bonus_triggered = true;
            } else if self.feature_mode != 0 && self.feature_mode != 4 {
                // A resolved normal-mode win clears a tease/hot feature.
                self.feature_mode = 0;
            }
        }
        result
    }

    /// Collect the latched payout into the balance (state `4` tally, capped
    /// at [`BALANCE_CAP`]) and return to idle. Returns the credited amount.
    // PORT: FUN_801cf0d8 state 4 (payout tally into DAT_801d4114)
    pub fn collect(&mut self) -> i32 {
        if self.phase != SlotPhase::Payout {
            return 0;
        }
        let credit = self.last_result.map(|r| r.payout).unwrap_or(0);
        self.balance = (self.balance + credit).min(BALANCE_CAP);
        self.phase = SlotPhase::Idle;
        credit
    }

    /// Commit the cash-out (state `100`): the machine is done and the final
    /// balance is returned for assignment into the casino coin bank
    /// (`_DAT_800845A4 = DAT_801d4114` - an assignment, not a delta).
    // PORT: FUN_801cf0d8 state 100 (cash-out commit)
    pub fn cash_out(&mut self) -> i32 {
        self.phase = SlotPhase::CashedOut;
        self.balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payouts() -> SlotPayoutTable {
        // Synthetic table: symbol id i pays (i+1)*2 coins.
        let mut payouts = [0u8; SYMBOL_COUNT];
        for (i, p) in payouts.iter_mut().enumerate() {
            *p = ((i + 1) * 2) as u8;
        }
        SlotPayoutTable { payouts }
    }

    #[test]
    fn slot_lcg_folds_the_halves() {
        // seed 0: x = 0*5+1 = 1; folded = (1<<16) + 0 = 0x10000.
        let mut r = SlotRng::new(0);
        assert_eq!(r.next_u32(), 0x10000);
        // next: x = 0x10000*5+1 = 0x50001; folded = (0x50001<<16)+(0x50001>>16)
        //       = 0x00010000 + 5 = 0x10005.
        assert_eq!(r.next_u32(), 0x10005);
        // Deterministic per seed.
        let mut a = SlotRng::new(0xDEAD_BEEF);
        let mut b = SlotRng::new(0xDEAD_BEEF);
        assert_eq!(a.next_u32(), b.next_u32());
    }

    #[test]
    fn strip_is_a_two_of_each_permutation_for_both_probe_steps() {
        for probe in [STRIP_PROBE_PRIMARY, STRIP_PROBE_SECONDARY] {
            let mut rng = SlotRng::new(12345);
            let strip = build_strip(&mut rng, probe);
            let mut counts = [0usize; SYMBOL_COUNT];
            for &s in &strip {
                assert!((s as usize) < SYMBOL_COUNT, "symbol id in range");
                counts[s as usize] += 1;
            }
            assert_eq!(
                counts, [2; SYMBOL_COUNT],
                "each symbol twice (probe {probe})"
            );
        }
    }

    #[test]
    fn land_row_finds_target_within_depth_else_next_row() {
        let mut strip = [0u8; STRIP_LEN];
        strip[5] = 7;
        // Target within depth from row 2 -> lands on row 5.
        assert_eq!(land_row(&strip, 2, 4, 7), 5);
        // Depth too shallow -> next natural row.
        assert_eq!(land_row(&strip, 2, 2, 7), 3);
        // Wraps around the strip end.
        strip[1] = 9;
        assert_eq!(land_row(&strip, 18, 4, 9), 1);
    }

    #[test]
    fn feature_roll_shapes() {
        let mut rand = BiosRand::new(7);
        let roll = feature_roll(&mut rand, 500, 0, false);
        assert!(roll.jitter < 5);
        assert!((2..8).contains(&roll.normal_target));
        // With a feature already active the entry rolls are skipped.
        let mut rand = BiosRand::new(7);
        let roll = feature_roll(&mut rand, 500, 6, false);
        assert_eq!(roll.entered_mode, None);
    }

    #[test]
    fn stop_plan_by_mode() {
        let mut rng = SlotRng::new(3);
        let (d, t) = stop_plan(&mut rng, 0, 4, None);
        assert!((2..5).contains(&d), "mode 0 depth = rand%3 + 2");
        assert_eq!(t, 4);
        let (d, t) = stop_plan(&mut rng, 1, 4, None);
        assert!((6..10).contains(&d), "reach depth = (rand&3) + 6");
        assert_eq!(t, 9);
        let (_, t) = stop_plan(&mut rng, 2, 4, None);
        assert_eq!(t, 8);
        // Guaranteed mode drives to the already-landed symbol.
        let (d, t) = stop_plan(&mut rng, 4, 4, Some(6));
        assert_eq!((d, t), (STRIP_LEN, 6));
    }

    #[test]
    fn spin_charges_the_bet_and_sequences_phases() {
        let mut m = SlotMachine::new(payouts(), 42, 50);
        assert_eq!(m.phase(), SlotPhase::Idle);
        assert!(m.spin());
        assert_eq!(m.balance(), 50 - SPIN_COST_NORMAL);
        assert_eq!(m.net_take(), NET_TAKE_NORMAL_SPIN);
        assert_eq!(m.phase(), SlotPhase::Spinning);
        // Reels advance while spinning; timer runs down into Stopping.
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        assert_eq!(m.phase(), SlotPhase::Stopping);
        assert!(m.can_stop());
        // Stop all three reels; the spin evaluates into Payout.
        assert!(m.stop_next_reel());
        m.tick();
        assert!(m.stop_next_reel());
        m.tick();
        assert!(m.stop_next_reel());
        assert_eq!(m.phase(), SlotPhase::Payout);
        assert_eq!(m.reels_stopped(), REEL_COUNT);
        let result = m.last_result().expect("evaluated");
        // Collect returns to idle, crediting exactly the evaluated payout.
        let before = m.balance();
        let credited = m.collect();
        assert_eq!(credited, result.payout);
        assert_eq!(m.balance(), before + credited);
        assert_eq!(m.phase(), SlotPhase::Idle);
    }

    #[test]
    fn spin_gate_blocks_a_thin_balance() {
        let mut m = SlotMachine::new(payouts(), 42, MIN_SPIN_BALANCE - 1);
        assert!(!m.can_spin());
        assert!(!m.spin());
        assert_eq!(m.phase(), SlotPhase::Idle);
    }

    #[test]
    fn winning_line_pays_the_table_value() {
        let mut m = SlotMachine::new(payouts(), 9, 100);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        // Force a known middle-line win: overwrite the display strips so the
        // payline rows all read symbol 5, then stop with rigged positions.
        for reel in 0..REEL_COUNT {
            m.strips[reel] = [5; STRIP_LEN];
        }
        m.stop_reel(0);
        m.stop_reel(1);
        m.stop_reel(2);
        let r = m.last_result().expect("evaluated");
        assert_eq!(r.symbol, Some(5));
        assert_eq!(r.payout, (5 + 1) * 2);
        assert!(!r.bonus_triggered);
    }

    #[test]
    fn jackpot_symbols_trigger_the_bonus_round_and_product_payout() {
        let mut m = SlotMachine::new(payouts(), 9, 100);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            m.strips[reel] = [9; STRIP_LEN];
        }
        m.stop_reel(0);
        m.stop_reel(1);
        m.stop_reel(2);
        let r = m.last_result().expect("evaluated");
        assert!(r.bonus_triggered);
        assert_eq!(m.feature_mode(), 6);
        assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT);
        m.collect();
        // A bonus "free" spin still costs 1 coin (the mode-4..6 charge) and
        // pays the (sym+1)^3 product, which is subtracted from the net take.
        let before = m.balance();
        let take_before = m.net_take();
        assert!(m.spin());
        assert_eq!(
            m.balance(),
            before - SPIN_COST_FEATURE,
            "feature spin costs 1 coin"
        );
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        m.stop_reel(0);
        m.stop_reel(1);
        m.stop_reel(2);
        let r = m.last_result().expect("evaluated");
        assert!(r.bonus_spin);
        assert_eq!(r.payout, 10 * 10 * 10, "(9 + 1)^3 product payout");
        assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT - 1);
        assert_eq!(
            m.net_take(),
            take_before + NET_TAKE_FEATURE_SPIN - r.payout,
            "bonus payout is subtracted from the net take"
        );
    }

    #[test]
    fn balance_caps_in_the_tally_and_cash_out_returns_it() {
        let mut m = SlotMachine::new(payouts(), 9, BALANCE_CAP - 1);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            m.strips[reel] = [7; STRIP_LEN];
        }
        m.stop_reel(0);
        m.stop_reel(1);
        m.stop_reel(2);
        m.collect();
        assert!(m.balance() <= BALANCE_CAP);
        let committed = m.cash_out();
        assert_eq!(committed, m.balance());
        assert_eq!(m.phase(), SlotPhase::CashedOut);
    }

    #[test]
    fn all_three_paylines_always_play() {
        // There is no bet-line selection: a top-row-only match pays.
        let mut m = SlotMachine::new(payouts(), 9, 100);
        assert_eq!(m.spin_cost(), SPIN_COST_NORMAL);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            // Middle rows differ; only the top row (-1) matches.
            let mut s = [0u8; STRIP_LEN];
            s[STRIP_LEN - 1] = 6; // row -1 from row 0
            s[0] = reel as u8; // middle row: 0/1/2 - no match
            m.strips[reel] = s;
        }
        // Rig positions to row 0 and stop.
        for reel in 0..REEL_COUNT {
            m.reel_pos[reel] = 0;
            m.reel_vel[reel] = 0;
            m.stopped[reel] = Some(0);
        }
        let r = m.evaluate_spin();
        assert_eq!(r.symbol, Some(6), "top-line match pays");
        assert_eq!(r.payout, (6 + 1) * 2);
        assert_eq!(r.line, Some(1), "line 1 = the top payline");
    }

    #[test]
    fn feature_odds_bracket_on_the_net_take() {
        // Low net take -> large denominators (rare); high -> small
        // (frequent). The exact edges 1000 / 2000 fall in no bracket.
        assert_eq!(feature_denominators(0), Some((700, 500)));
        assert_eq!(feature_denominators(999), Some((700, 500)));
        assert_eq!(feature_denominators(1000), None);
        assert_eq!(feature_denominators(1001), Some((350, 250)));
        assert_eq!(feature_denominators(1999), Some((350, 250)));
        assert_eq!(feature_denominators(2000), None);
        assert_eq!(feature_denominators(2001), Some((175, 125)));
        // Empirically the high bracket enters features far more often than
        // the low one over the same stream length.
        let hits = |take: i32| -> usize {
            let mut rand = BiosRand::new(0x1234_5678);
            (0..4000)
                .filter(|_| {
                    feature_roll(&mut rand, take, 0, false)
                        .entered_mode
                        .is_some()
                })
                .count()
        };
        assert!(
            hits(2500) > hits(500) * 2,
            "high net take is far more generous"
        );
    }
}
