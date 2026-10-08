//! From-scratch **casino slot-machine** rules engine.
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
//!   to the first unused position, and place the slot's value there
//!   (`FUN_801cf0d8` case 0, [`build_reel`]). Retail builds **two** strips per
//!   reel in one interleaved pass - the symbol strip `DAT_801d3e90` (`slot/2`,
//!   probe `+0xd`) and the bonus-numeral strip `DAT_801d3fd0`
//!   (`slot/2 + 0x10`, probe `+1`);
//! - the **display strip** `DAT_801d3d50` the win eval and the renderer read, and
//!   the one-row-per-frame copy that refills it from whichever source strip the
//!   feature mode selects - the mechanism by which a bonus round "swaps the reels
//!   to numbers" ([`SlotMachine::tick`], `FUN_801cf0d8` render tail);
//! - the per-reel **claimed value** `DAT_801d3d20` - the payline value + 1,
//!   latched the frame a reel locks and cleared at spin start (`FUN_801d0554`) -
//!   which is what the marquee tally prints ([`SlotMachine::tally`]);
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
//! - the win evaluation: five paylines - three horizontal and two diagonal
//!   ([`legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS`]) - checked
//!   all-equal on the display strip, highest-value line kept, normal payout =
//!   `payout_table[symbol]` ([`legaia_asset::slot_payout`]), jackpot symbols
//!   `9` / `8` kick off a bonus round of 3 / 1 free spins, and a **bonus round
//!   pays the product of the three payline `(value - 0xf)` factors** - no
//!   equality gate, and the winning line is forced to the centre
//!   (`FUN_801d13e8`, [`SlotMachine::evaluate_spin`]);
//! - the bonus round's own stop plan: depth `0`, target `-1` - i.e. the reel
//!   simply lands on the next row, so the three numbers are the player's timing
//!   and nothing else (`FUN_801d2114` case 6 -> `FUN_801d2440`);
//! - the coin economy: the playing balance is overlay-local, capped at
//!   `9_999_999` in the tally path, and *assigned* to the coin bank on
//!   cash-out (state `100`), not debited per spin.
//!
//! What is an **engine-side reconstruction** (marked at each site): the
//! spin-up velocity/timer magnitudes (visual pacing, not pinned), and the
//! BIOS-`rand` feature stream substituted with a second deterministic
//! [`BiosRand`] LCG so replays stay bit-identical.
//! Feature modes 3 (hot) and 5 (hold) are documented but folded to the
//! normal landing plan here - their bonus-strip value targeting is not modeled.
//!
//! Chain: retail `FUN_801cf0d8` (reel SM) -> `FUN_801d258c` (feature roll) ->
//! `FUN_801d2114` / `FUN_801d2440` (stop) -> `FUN_801d0554` (snap + claim) ->
//! `FUN_801d13e8` (win eval).

use legaia_asset::minigame_slot_scene::{
    LANDING_LINE_BY_JITTER, MarqueeFrame, MarqueePlacement, PAYLINE_CENTRE_ROW_BIAS,
    PAYLINE_ROW_OFFSETS, compose_marquee_frame,
};
use legaia_asset::slot_payout::{self, SlotPayoutTable};
use legaia_engine_vm::bios_rand::BiosRand;

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
/// in state `1`. All five paylines always play - there is no bet-line
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
/// Bonus rounds granted when the jackpot symbol `9` (the red "punch") matches
/// (`FUN_801d13e8`).
pub const BONUS_SPINS_JACKPOT: i32 = slot_payout::PUNCH_BONUS_ROUNDS as i32;
/// Bonus rounds granted when the bonus symbol `8` (the blue "kick") matches.
pub const BONUS_SPINS_BONUS: i32 = slot_payout::KICK_BONUS_ROUNDS as i32;
/// The probe step used for the **symbol** strip array `DAT_801d3e90`
/// (`(pos + 0xd) % 0x14`).
pub const STRIP_PROBE_PRIMARY: usize = 0xd;
/// The probe step used for the **bonus** strip array `DAT_801d3fd0`
/// (`(pos + 1) % 0x14`).
pub const STRIP_PROBE_SECONDARY: usize = 1;
/// The feature mode a bonus round runs in (`DAT_801d3cac == 6`).
pub const FEATURE_MODE_BONUS: u8 = 6;
/// How far **ahead of the payline row** the display strip is refilled, in strip
/// rows.
///
/// Retail refills exactly one row of the display strip per reel per frame, at
/// row `(pos >> 8) + 0x19`, while the payline it pays on is row
/// `(pos >> 8) + 0x10` - so the row being rewritten is always this many rows
/// ahead of the payline (`0x19 - 0x10`, both mod `0x14`). That gap is the whole
/// bonus-round strip swap: rows are converted to the other strip *before* they
/// reach the payline, and the mode-6 spin timer ([`BONUS_SPIN_UP_FRAMES`]) is
/// sized to guarantee the reel travels far enough for the conversion to arrive.
pub const DISPLAY_REFRESH_LEAD: usize = 9;

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

/// Build one reel's **two** 20-slot strips (`FUN_801cf0d8` case 0).
///
/// For each of the 20 slots: draw a fresh RNG value, reduce it mod `0x14`, and
/// probe forward until an unused position is found; place `slot/2` (plus the
/// strip's value base) there. A collision-resolving permutation that scatters
/// each value - two strip positions each - around the reel. The probe step is
/// [`STRIP_PROBE_PRIMARY`] for the symbol strip and [`STRIP_PROBE_SECONDARY`]
/// for the bonus one; both are coprime with 20, so the probe always terminates.
/// The value base is `0` for the symbol strip (ids `0..=9`) and
/// [`slot_payout::BONUS_VALUE_BASE`] for the bonus one (values `0x10..=0x19`).
///
/// The two strips are built in retail's **interleaved** draw order: slot `i`
/// is placed in the symbol strip and then slot `i` in the bonus strip, from the
/// same RNG stream, before moving on to slot `i + 1`. The order matters - it is
/// what the strips are, and building either strip alone from a fresh stream
/// would produce a different permutation.
///
/// Wired: [`SlotMachine::new`] builds all three reels through this at session
/// start, and seeds the display strip from the symbol half.
// PORT: FUN_801cf0d8 case 0 (reel-strip permutation build)
pub fn build_reel(rng: &mut SlotRng) -> ([u8; STRIP_LEN], [u8; STRIP_LEN]) {
    let (mut symbols, mut bonus) = ([u8::MAX; STRIP_LEN], [u8::MAX; STRIP_LEN]);
    for slot in 0..STRIP_LEN {
        let mut pos = (rng.next_u32() as usize) % STRIP_LEN;
        while symbols[pos] != u8::MAX {
            pos = (pos + STRIP_PROBE_PRIMARY) % STRIP_LEN;
        }
        symbols[pos] = (slot / 2) as u8;

        let mut pos = (rng.next_u32() as usize) % STRIP_LEN;
        while bonus[pos] != u8::MAX {
            pos = (pos + STRIP_PROBE_SECONDARY) % STRIP_LEN;
        }
        bonus[pos] = (slot / 2) as u8 + slot_payout::BONUS_VALUE_BASE;
    }
    (symbols, bonus)
}

/// The per-spin roll (`FUN_801d258c`): the landing jitter, the normal-mode
/// target symbol, and - when no feature is already active - whether a
/// feature mode was entered this spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinRoll {
    /// Per-spin landing-line row (`DAT_801d4134 = rand%5`).
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
/// once (`rand%100 + 200`) when `spin_up_pressed` (`DAT_801d3790`) is set, then -
/// only when no feature is active (`feature_mode == 0`) - roll the net-take
/// bracket's two `rand % (widen + N) == 0` probabilities (mode 1 then mode
/// 2) and finally the flat `rand % (widen + 600) == 0` mode-3 roll. Draw
/// order matches the dump exactly.
// PORT: FUN_801d258c (per-spin feature roll, net-take-bracketed odds)
pub fn feature_roll(
    rand: &mut BiosRand,
    net_take: i32,
    feature_mode: u8,
    spin_up_pressed: bool,
) -> SpinRoll {
    let jitter = (rand.next_u15() % 5) as i32;
    let normal_target = (rand.next_u15() % 6 + 2) as u8;
    let widen: u32 = if spin_up_pressed {
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
/// mode `4` (guaranteed-hit) drives the reel to a winning symbol; and mode `6`,
/// the **bonus round**, passes depth `0` with target `-1`, which searches
/// nothing and lands the reel on the next row. That is the bonus round's whole
/// character: the machine does not steer it, so the three numbers you multiply
/// are your own timing. Modes `3` (hot, bonus-strip value targeting) and `5`
/// (hold) are folded to the normal plan here (reconstruction - see the module
/// docs). Draws from the slot LCG - retail uses it for reel-landing selection
/// (the BIOS-`rand` stream feeds only the feature/jitter rolls).
// PORT: FUN_801d2114 (per-reel stop: target symbol + search depth by feature mode)
pub fn stop_plan(
    rng: &mut SlotRng,
    feature_mode: u8,
    normal_target: u8,
    guarantee_target: Option<u8>,
) -> (usize, Option<u8>) {
    match feature_mode {
        1 => ((((rng.next_u32() & 3) + 6) as usize), Some(9)),
        2 => ((((rng.next_u32() & 3) + 6) as usize), Some(8)),
        // The bonus round: no search, no target - the reel stops where you
        // stopped it (`FUN_801d2114` case 6 passes depth 0 / target -1).
        FEATURE_MODE_BONUS => (0, None),
        4 => match guarantee_target {
            // Drive the reel all the way to the guaranteed symbol.
            Some(t) => (STRIP_LEN, Some(t)),
            None => (STRIP_LEN, Some(normal_target)),
        },
        // Modes 0, 3, 5 (and anything unmapped): the normal scan.
        _ => (((rng.next_u32() % 3 + 2) as usize), Some(normal_target)),
    }
}

/// The reel landing search (`FUN_801d2440`), in the engine's payline-row frame
/// (retail's raw reel row plus [`PAYLINE_CENTRE_ROW_BIAS`]).
///
/// Retail walks `depth` raw rows `cur + 1 ..= cur + depth` (`0x801D2494..
/// 0x801D2528`) - five to `4 + depth` rows past the current payline row - for
/// `target`. On a hit at row `R` it stops the reel at raw `R + 3 + word`, where
/// `word` is this spin's landing-line table entry for the reel, which puts the
/// target on the payline `line_offset` names rather than always on the middle
/// row ([`legaia_asset::minigame_slot_scene::LANDING_LINE_BY_JITTER`]). With no
/// hit it takes the next row. A `None` target (or a `depth` of 0, its retail
/// companion: the `blez` at `0x801D2468`) searches nothing - the bonus round's
/// free stop.
// PORT: FUN_801d2440 (landing search: find target within depth, land it on the jitter's payline, else next row)
pub fn land_row(
    strip: &[u8; STRIP_LEN],
    from_row: usize,
    depth: usize,
    target: Option<u8>,
    line_offset: i32,
) -> usize {
    if let Some(target) = target {
        // Raw `cur + 1 + t1` is payline row `from_row + 1 + t1 - 0x10`.
        let ahead = STRIP_LEN + 1 - PAYLINE_CENTRE_ROW_BIAS as usize % STRIP_LEN;
        for t1 in 0..depth.min(STRIP_LEN) {
            let row = (from_row + ahead + t1) % STRIP_LEN;
            if strip[row] == target {
                return (row as i32 - line_offset).rem_euclid(STRIP_LEN as i32) as usize;
            }
        }
    }
    (from_row + 1) % STRIP_LEN
}

/// The outcome of one evaluated spin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinResult {
    /// Winning payline index (`0` top / `1` middle / `2` bottom / `3`, `4` the
    /// two diagonals), or `None`. The index is also the medallion / lamp the
    /// machine lights - see [`legaia_asset::minigame_slot_scene`].
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
/// the granularity the host drives: init/attract/spin/stop/payout, the
/// cash-out submenu and its two instruction pages (states `0x32..=0x39`), the
/// not-enough-coins prompt (`0x5A`) and the state-100 leave fade
/// ([`SlotMachine::retail_state`] carries the exact word).
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
    /// The cash-out submenu and its instruction pages (states `0x32..=0x39`,
    /// [`SlotMachine::cash_out_input`]).
    Menu,
    /// The not-enough-coins prompt (state `0x5A`): any face button leaves.
    NoCoins,
    /// The leave fade (state `100`): the screen fades to black, then the
    /// balance is committed ([`SlotPhase::CashedOut`]).
    Leaving,
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
    /// Display reel strips (`DAT_801d3d50`): win eval + render read **these**,
    /// and only these. A row holds a symbol id (`0..=9`) or, once a bonus round
    /// has rotated it in, a bonus numeral value (`0x10..=0x19`).
    strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Source strip: the ten reel **symbols** (`DAT_801d3e90`).
    symbol_strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Source strip: the ten bonus **numerals** as values `0x10..=0x19`
    /// (`DAT_801d3fd0`). A bonus round does not relabel the symbols - it feeds
    /// the display strip from here instead.
    bonus_strips: [[u8; STRIP_LEN]; REEL_COUNT],
    /// Per-reel **claimed value** (`DAT_801d3d20`): the payline value + 1,
    /// latched the frame the reel locks; `0` until the reel's stop is taken.
    /// The marquee's bonus tally is this array (see [`SlotMachine::tally`]), and
    /// the payout multiplies the very same rows - so they cannot disagree.
    claimed: [i32; REEL_COUNT],
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
    /// Per-spin landing-line row (`DAT_801d4134`, `rand % 5`): the row of the
    /// `0x801d3630` table that picks which payline a forced stop lands its
    /// target on ([`LANDING_LINE_BY_JITTER`]). Its `* 0x10` in
    /// `FUN_801d2440` is the table's row stride, not a sub-row nudge.
    jitter: i32,
    /// Overlay-local playing balance (`DAT_801d4114`).
    balance: i32,
    /// The spin-up press latch (`DAT_801d3790`): raised by any face-button
    /// edge during the spin-up ([`Self::latch_spin_up`]), read by the next
    /// spin's feature roll and cleared straight after it. It **widens** every
    /// feature-entry denominator, so a press makes the next spin's features
    /// *rarer* - the word was once read as a "richer odds" flag, which has
    /// the effect backwards.
    spin_up_pressed: bool,
    /// "The bonus round just ended" latch (`DAT_801d3798`): the next spin runs
    /// the long spin-up, so the display strip has time to rotate back to the
    /// symbols before it reaches the payline.
    bonus_just_ended: bool,
    /// The last evaluated spin, latched through [`SlotPhase::Payout`].
    last_result: Option<SpinResult>,
    /// `DAT_801d3d3c`: the coin figure the marquee's payout caption prints.
    /// Latched the frame the third reel locks; cleared on collect.
    caption_payout: i32,
    /// `DAT_801d3c94`: frames since the caption came up. The composer slides the
    /// caption in over the first [`PAYOUT_SLIDE_ROWS`] frames, so this has to
    /// advance for the caption to finish arriving.
    caption_frame: i32,
    /// The five payline segments' model-space geometry (`DAT_801d3680`), when
    /// the host staged it ([`Self::with_paylines`]).
    paylines: Vec<legaia_asset::minigame_slot_scene::PayLine>,
    /// The bonus-anticipation latch (`DAT_801d3ca4`): `1` while two landed
    /// reels show a punch (`9`) pair on any payline, `2` for a kick (`8`)
    /// pair, `0` otherwise - see [`Self::anticipation`].
    anticipation: i32,
    /// The scanner's once-per-spin guard (`DAT_801d3ca8`) over its sting and
    /// its loop swap.
    anticipation_sounded: bool,
    /// `DAT_801d3d38`: the coins the payout state still has to tally into the
    /// balance.
    payout_left: i32,
    /// `DAT_801d3c8c`: the winning-line word the lamps and the payline pass
    /// compare against; `-1` lights nothing.
    win_line: i32,
    /// The overlay's free-running frame counter whose parity paces the tally
    /// (`DAT_801d3c9c`, advanced by the reel renderer's tail).
    frame: u32,
    /// The sounds this machine raised since the host last took them.
    sounds: SlotSounds,
    /// The retail state word while the machine is in [`SlotPhase::Menu`]
    /// (`0x32..=0x39`); unused otherwise.
    menu_state: u8,
    /// The submenu cursor `DAT_801d4110`, kept as retail stores it - an
    /// unsigned word reduced `% 3` (so Up on row 0 stays on row 0).
    menu_cursor: u32,
    /// The screen-fade level `DAT_801d3c98` (`0..=0xFF`) the instruction
    /// pages and the leave fade ramp by `0x10` a frame.
    fade: i32,
}

/// One frame's worth of the machine's sound writes, as retail issues them:
/// stores straight into a cue-ring slot (`DAT_8007B6D8[slot] = id` - the
/// overlay never goes through the cursor producer), and the reel-motor voice
/// it keys and releases itself through `FUN_80065034` / `FUN_800653C8`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotSounds {
    /// `(ring slot, cue id)` stores, in order.
    pub ring: Vec<(u8, i16)>,
    /// Voices keyed on (`FUN_80065034`).
    pub voice_on: Vec<crate::other_game_overlay::VoiceAttrCue>,
    /// Voices released (`FUN_800653C8`).
    pub voice_off: Vec<u8>,
}

/// The SPU voice the reel motor loop keys (`FUN_80065034`'s `a0 = 0x13`).
pub const SPIN_VOICE: u8 = 0x13;
/// Reel stop click - one store into ring slot 0 per Stop press taken
/// (`0x801CF74C` / `0x801CF794` / `0x801CF7DC`).
pub const CUE_REEL_STOP: i16 = 0x20A;
/// Payout tally tick - one store into ring slot 0 per transfer
/// (`0x801CF900..0x801CF90C`).
pub const CUE_PAYOUT_TICK: i16 = 0x209;
/// The reach sting the scanner fires once per spin (`0x801D2050`).
pub const CUE_REACH: i16 = 0x200;
/// The spin-start stings of feature modes `1` / `2`, into ring slot 2
/// (`0x801CF5C0` / `0x801CF5DC`).
pub const CUE_FEATURE_1: i16 = 0x201;
pub const CUE_FEATURE_2: i16 = 0x202;
/// Caption timer the payout state starts at on a losing spin
/// (`li v0,0x6b` at `0x801CF85C`), so the state lasts
/// [`PAYOUT_HOLD_FRAMES`]` - 0x6B` frames.
pub const LOSING_PAYOUT_TIMER: i32 = 0x6B;
/// Caption timer at which a fully tallied payout state returns to idle
/// (`slti v0,v0,0x79` at `0x801CF918`).
pub const PAYOUT_HOLD_FRAMES: i32 = 0x79;

/// The reel motor's voice-attr call: `FUN_80065034(0x13, 2, 1, tone, 0x3C,
/// 0x40, 0x28, 0x28)` - class-2 VAB, program 1, tone `0` for the spin loop
/// (`0x801CF558`) and tone `1` for the reach loop the scanner swaps in
/// (`FUN_801D1AF4`, `0x801D2070`).
fn spin_voice(tone: i32) -> crate::other_game_overlay::VoiceAttrCue {
    crate::other_game_overlay::VoiceAttrCue {
        voice: u32::from(SPIN_VOICE),
        vab_program_tone: (2, 1, tone),
        note_and_fine: (0x3C, 0x40),
        volume: (0x28, 0x28),
    }
}

/// Spin-up frames before the reels may be stopped (visual pacing constant;
/// the retail `DAT_801d3c90` magnitude is not pinned).
pub const SPIN_UP_FRAMES: i32 = 30;
/// Extra spin-up frames on a bonus spin, and on the first spin after a bonus
/// round ends (`FUN_801cf0d8` state 1: `DAT_801d3c90 = 0x18` when
/// `DAT_801d3cac == 6` or `DAT_801d3798`).
///
/// This is not decoration. The display strip is refilled one row per frame,
/// [`DISPLAY_REFRESH_LEAD`] rows ahead of the payline, so a reel has to *travel*
/// before the strip it swapped to reaches the row it pays on. The long spin-up
/// is what buys that travel - on both edges of the bonus round.
pub const BONUS_SPIN_UP_FRAMES: i32 = 0x18;
/// Per-reel spin velocities (visual pacing constants, staggered like the
/// retail ramp so the reels visibly desynchronize).
pub const SPIN_VELOCITY: [i32; REEL_COUNT] = [0x60, 0x70, 0x80];

impl SlotMachine {
    /// A fresh machine over the parsed payout table, seeded (retail reseeds
    /// from BIOS `rand` at init) and holding `balance` coins loaded from the
    /// casino coin bank.
    pub fn new(payouts: SlotPayoutTable, seed: u32, balance: i32) -> Self {
        let mut rng = SlotRng::new(seed);
        // Retail builds BOTH strips for each reel in one interleaved pass - the
        // symbol strip and the bonus-numeral strip, off the same RNG stream -
        // and then clones the symbol strip into the display copy the win eval
        // and the renderer read (`FUN_801cf0d8` case 0).
        let mut symbol_strips = [[0u8; STRIP_LEN]; REEL_COUNT];
        let mut bonus_strips = [[0u8; STRIP_LEN]; REEL_COUNT];
        for reel in 0..REEL_COUNT {
            let (symbols, bonus) = build_reel(&mut rng);
            symbol_strips[reel] = symbols;
            bonus_strips[reel] = bonus;
        }
        Self {
            payouts,
            rng,
            rand: BiosRand::new(seed ^ 0x5A5A_5A5A),
            strips: symbol_strips,
            symbol_strips,
            bonus_strips,
            claimed: [0; REEL_COUNT],
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
            spin_up_pressed: false,
            bonus_just_ended: false,
            last_result: None,
            caption_payout: 0,
            caption_frame: 0,
            paylines: Vec::new(),
            anticipation: 0,
            anticipation_sounded: false,
            payout_left: 0,
            win_line: -1,
            frame: 0,
            sounds: SlotSounds::default(),
            menu_state: 0,
            menu_cursor: 0,
            fade: 0,
        }
    }

    /// Stage the overlay's payline geometry
    /// ([`legaia_asset::minigame_slot_scene::parse_paylines`]) so every host
    /// draws the paylines off the machine itself ([`Self::payline_segments`]).
    pub fn with_paylines(
        mut self,
        paylines: Vec<legaia_asset::minigame_slot_scene::PayLine>,
    ) -> Self {
        self.paylines = paylines;
        self
    }

    /// The winning-line word the payline pass compares against
    /// (`DAT_801d3c8c`): the last spin's line, or `-1` - which lights none.
    pub fn winning_line_word(&self) -> i32 {
        self.win_line
    }

    /// Take the sounds raised since the last call ([`SlotSounds`]).
    pub fn take_sounds(&mut self) -> SlotSounds {
        std::mem::take(&mut self.sounds)
    }

    /// Coins the payout state still has to tally (`DAT_801d3d38`).
    pub fn payout_left(&self) -> i32 {
        self.payout_left
    }

    /// This frame's paylines, built by the ported pass and projected onto the
    /// 640x240 slot framebuffer. Empty when no geometry was staged.
    pub fn payline_segments(&self) -> Vec<ProjectedPayline> {
        projected_paylines(&self.paylines, self.winning_line_word())
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

    /// The display-strip **value** currently on the payline of `reel`
    /// (`(pos >> 8) mod 0x14`). A symbol id `0..=9` in the normal game; a bonus
    /// numeral value `0x10..=0x19` once a bonus round has rotated the numbers in.
    pub fn payline_symbol(&self, reel: usize) -> u8 {
        let row = self.payline_row(reel);
        self.strips[reel][row]
    }

    /// `true` when a bonus round is running (`DAT_801d3cac == 6`): the reels
    /// carry numbers, every spin pays, and the payout is a product.
    pub fn in_bonus_round(&self) -> bool {
        self.feature_mode == FEATURE_MODE_BONUS
    }

    /// `true` when a bonus round has *just* ended (`DAT_801d3798`), so the next
    /// spin still runs the long spin-up - the symbols have to rotate back onto
    /// the payline before it can pay on them.
    pub fn bonus_just_ended(&self) -> bool {
        self.bonus_just_ended
    }

    /// Frames of spin-up the next spin will arm: the long one during a bonus
    /// round and on the spin straight after one, the short one otherwise.
    pub fn next_spin_up_frames(&self) -> i32 {
        SPIN_UP_FRAMES
            + if self.in_bonus_round() || self.bonus_just_ended {
                BONUS_SPIN_UP_FRAMES
            } else {
                0
            }
    }

    /// The raw claimed value of `reel` (`DAT_801d3d20[reel]`): the payline value
    /// **+ 1**, latched the frame the reel's stop was taken; `0` while the reel
    /// is still spinning.
    pub fn claimed(&self, reel: usize) -> i32 {
        self.claimed[reel]
    }

    /// The **bonus tally** - what the machine's marquee prints across the top of
    /// a bonus round, one column per reel.
    ///
    /// A column reads `0` until that reel's stop is claimed, and its landed
    /// number `1..=10` after: `0 x 0 x 0` at the start of a round, `9 x 5 x 0`
    /// with two reels down. The arithmetic is retail's, verbatim
    /// (`FUN_801cfff0`): print the message at `claimed - 0x10` when the claimed
    /// value clears `0xF`, else the `"0"` glyph.
    ///
    /// This is *the same latch* the payout multiplies - not a parallel display
    /// copy - so once all three columns are in, their product **is** the coins
    /// the round pays ([`SlotMachine::tally_product`]).
    // PORT: FUN_801cfff0 (the marquee's bonus tally row)
    pub fn tally(&self) -> [u32; REEL_COUNT] {
        core::array::from_fn(|r| {
            let claimed = self.claimed[r];
            if claimed > slot_payout::BONUS_VALUE_BIAS as i32 {
                (claimed - slot_payout::BONUS_VALUE_BASE as i32).max(0) as u32
            } else {
                0
            }
        })
    }

    /// `true` once every reel's stop has been claimed - i.e. the tally is
    /// complete and its product is the round's payout.
    pub fn tally_complete(&self) -> bool {
        self.claimed
            .iter()
            .all(|&c| c > slot_payout::BONUS_VALUE_BIAS as i32)
    }

    /// The product of the [`SlotMachine::tally`]'s three numbers, or `0` until
    /// all three columns are claimed. For a bonus round this is exactly the
    /// coins the spin pays.
    pub fn tally_product(&self) -> u32 {
        if !self.tally_complete() {
            return 0;
        }
        self.tally().iter().product()
    }

    /// The payline row index of `reel`.
    pub fn payline_row(&self, reel: usize) -> usize {
        ((self.reel_pos[reel] >> 8) as usize) % STRIP_LEN
    }

    /// The raw reel position of `reel` - a fixed-point angle whose high byte is
    /// the strip row and whose low byte is the sub-symbol fraction
    /// (`DAT_801d3cc0`). The renderer needs the fraction: retail's reel is a 3D
    /// cylinder and the fraction is what rotates it between symbols.
    pub fn reel_pos(&self, reel: usize) -> i32 {
        self.reel_pos[reel]
    }

    /// Whether `reel`'s stop is still open - the per-reel flag
    /// `DAT_801d3d00[reel]`, which the bet charge sets and the reel's Stop
    /// press clears (`FUN_801d2114`). The furniture pass `FUN_801d08e4` reads
    /// it to light that reel's pedestal and, ORed over the three, to brighten
    /// the medallions and marquee while any stop is still to be taken.
    // REF: FUN_801d08e4 (the pedestal pass that reads the flag)
    pub fn reel_stop_open(&self, reel: usize) -> bool {
        matches!(self.phase, SlotPhase::Spinning | SlotPhase::Stopping)
            && self.stopped.get(reel).is_some_and(|s| s.is_none())
    }

    /// The bonus-anticipation latch (`DAT_801d3ca4`): `1` while two landed
    /// reels show a punch pair, `2` for a kick pair, `0` otherwise. Read by
    /// the marquee's legend tail, which scrolls message `4` / `5` for it
    /// ([`legaia_asset::minigame_slot_scene::attract_legend`]).
    pub fn anticipation(&self) -> i32 {
        self.anticipation
    }

    /// The bonus-symbol scanner (`FUN_801d1af4`), run while exactly two stops
    /// are in: on each of the five paylines it compares the three reel pairs
    /// `(0,1)`, `(1,2)`, `(0,2)`, each only when both reels have landed
    /// (`DAT_801d3d10[r]`, ANDed per pair), and notes an equal pair of `9`s
    /// (punch) or `8`s (kick). A punch pair anywhere wins: the latch reads
    /// `1`, else `2` for a kick pair, else nothing is written (`0`, the state
    /// frame's clear). The scanner's rows are retail's `+0x11 / +0x10 / +0x0F`
    /// and the two diagonals - [`PAYLINE_ROW_OFFSETS`] around the payline row.
    ///
    /// The scanner also raises SFX cue `0x200` and calls `FUN_80065034`
    /// once per spin behind the guard `DAT_801d3ca8`; that half rides
    /// [`Self::tick`], which raises the sting and the reach loop.
    ///
    /// [`PAYLINE_ROW_OFFSETS`]: legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS
    // PORT: FUN_801d1af4 (PROT 0975)
    fn anticipation_scan(&self) -> i32 {
        use legaia_asset::slot_payout::{KICK_SYMBOL_ID, PUNCH_SYMBOL_ID};
        let landed: [bool; REEL_COUNT] = core::array::from_fn(|r| self.stopped[r].is_some());
        let (mut punch, mut kick) = (false, false);
        for offs in legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS.iter() {
            let v = |r: usize| {
                let row = (self.payline_row(r) as isize + offs[r] as isize)
                    .rem_euclid(STRIP_LEN as isize) as usize;
                self.strips[r][row]
            };
            for (a, b) in [(0, 1), (1, 2), (0, 2)] {
                if landed[a] && landed[b] && v(a) == v(b) {
                    punch |= v(a) == PUNCH_SYMBOL_ID;
                    kick |= v(a) == KICK_SYMBOL_ID;
                }
            }
        }
        if punch {
            1
        } else if kick {
            2
        } else {
            0
        }
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
    /// bet-line selection - all five paylines always play).
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

    /// This spin's landing-line row (`DAT_801d4134`): which payline a forced
    /// stop lands its target on.
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
            self.spin_up_pressed,
        );
        // State 1 clears the latch the moment the roll has read it
        // (`sw zero,0x3790(v0)` at `0x801CF56C`, right after
        // `jal 0x801D258C`): one spin-up press widens exactly one roll.
        self.spin_up_pressed = false;
        self.jitter = roll.jitter;
        self.normal_target = roll.normal_target;
        if let Some(mode) = roll.entered_mode {
            self.feature_mode = mode;
        }
        self.stopped = [None; REEL_COUNT];
        // The claimed values are cleared with the reel flags at the bet charge,
        // which is what resets the marquee tally to `0 x 0 x 0`.
        self.claimed = [0; REEL_COUNT];
        self.reel_vel = SPIN_VELOCITY;
        // A bonus spin (and the first spin after one) runs long, so the display
        // strip has room to rotate onto the other source strip before the
        // payline row comes round - see BONUS_SPIN_UP_FRAMES.
        self.spin_timer = self.next_spin_up_frames();
        self.bonus_just_ended = false;
        self.last_result = None;
        // State 2 clears the latch and its one-shot guard on entry
        // (`0x801CF600` / `0x801CF608`).
        self.anticipation = 0;
        self.anticipation_sounded = false;
        self.win_line = -1;
        self.payout_left = 0;
        self.phase = SlotPhase::Spinning;
        // The bet charge keys the reel motor and, in the two reach modes the
        // roll just entered, stores their sting into ring slot 2.
        self.sounds.voice_on.push(spin_voice(0));
        match self.feature_mode {
            1 => self.sounds.ring.push((2, CUE_FEATURE_1)),
            2 => self.sounds.ring.push((2, CUE_FEATURE_2)),
            _ => {}
        }
        true
    }

    /// The spin-up's input latch: a face-button edge (`_DAT_8007B874 & 0xF0`)
    /// on a spin-up frame whose timer has not run out raises the latch
    /// `DAT_801D3790`, which widens every feature-entry denominator of the
    /// **next** spin's roll (`FUN_801D258C`) by `rand % 100 + 200` - so a
    /// player who mashes buttons while the reels spin up makes the next
    /// spin's reach / hot modes rarer (`1/700` becomes `1/900..=1/999`).
    ///
    /// Call once per frame after [`Self::tick`] with this frame's edge; a no-op
    /// outside [`SlotPhase::Spinning`] - retail's test lives in state `2`
    /// alone. One frame differs: retail tests after the decrement that ends
    /// the spin-up, so an edge on that last frame still latches; the port's
    /// tick has already left `Spinning` by then.
    ///
    /// PORT: FUN_801cf0d8 (`0x801CF6D4`..`0x801CF704`, state 2's latch)
    pub fn latch_spin_up(&mut self, face_edge: bool) {
        if face_edge && self.phase == SlotPhase::Spinning && self.spin_timer != 0 {
            self.spin_up_pressed = true;
        }
    }

    /// Whether the spin-up press latch (`DAT_801D3790`) is up for the next
    /// spin.
    pub fn spin_up_pressed(&self) -> bool {
        self.spin_up_pressed
    }

    /// The source strip the display strip is currently being refilled from: the
    /// bonus numerals during a bonus round, the reel symbols otherwise
    /// (`FUN_801cf0d8` render tail: `DAT_801d3cac == 6 ? DAT_801d3fd0 :
    /// DAT_801d3e90`).
    fn active_source(&self) -> &[[u8; STRIP_LEN]; REEL_COUNT] {
        if self.in_bonus_round() {
            &self.bonus_strips
        } else {
            &self.symbol_strips
        }
    }

    /// Advance one frame: reels advance by their velocities (wrapping mod
    /// `0x1400`), the spin timer counts down into the stopping state, and each
    /// reel copies **one row** of the display strip from the active source.
    ///
    /// That one-row copy is the whole reel-swap mechanism. Retail never rewrites
    /// a strip wholesale: the render tail of `FUN_801cf0d8` refills the row
    /// [`DISPLAY_REFRESH_LEAD`] ahead of the payline, every frame, from whichever
    /// source strip the feature mode names - so when a bonus round opens, the
    /// numbers *rotate into* the reels from off-screen as they turn, and rotate
    /// back out again when it ends. Nothing is swapped in one go, and a stopped
    /// reel keeps its row (the refill runs ahead of the payline, never on it).
    // PORT: FUN_801cf0d8 tail (reel advance + display-strip row refill) + state 2
    pub fn tick(&mut self) {
        // The caption's slide-in clock. It only runs while a caption is up, and
        // the composer reads `min(frame - PAYOUT_SLIDE_ROWS, 0)` off it, so
        // without this advance the caption would sit one row short forever.
        if self.phase == SlotPhase::Payout {
            self.tick_payout();
        }
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
        // State 3 zeroes the latch every frame (the store sits in the delay
        // slot of the Stop-0 test at `0x801CF71C`, so it runs whatever the
        // pad says), then re-runs the scanner while exactly two stops are in
        // (the `DAT_801d3d2c == 2` test at `0x801CF7EC`).
        if self.phase == SlotPhase::Stopping {
            self.anticipation = if self.reels_stopped() == 2 {
                self.anticipation_scan()
            } else {
                0
            };
            // The first sighting per spin stings (ring slot 2) and swaps the
            // motor voice onto the reach loop, behind `DAT_801d3ca8`.
            if self.anticipation != 0 && !self.anticipation_sounded {
                self.anticipation_sounded = true;
                self.sounds.ring.push((2, CUE_REACH));
                self.sounds.voice_on.push(spin_voice(1));
            }
        }
        // The display-strip refill, after the advance - as in retail's tail.
        let source = *self.active_source();
        let rows: [usize; REEL_COUNT] =
            core::array::from_fn(|r| (self.payline_row(r) + DISPLAY_REFRESH_LEAD) % STRIP_LEN);
        for ((display, src), &row) in self.strips.iter_mut().zip(source.iter()).zip(rows.iter()) {
            display[row] = src[row];
        }
        self.frame = self.frame.wrapping_add(1);
    }

    /// State 4, one frame (`0x801CF888..0x801CF940`): advance the caption
    /// timer; while coins are owed, transfer a chunk into the balance on
    /// every odd frame - `11` while more than `20` remain, else `1` - with
    /// one tally tick into ring slot 0 per transfer; once nothing is owed
    /// and the timer reaches [`PAYOUT_HOLD_FRAMES`], drop back to idle with
    /// the winning line cleared.
    // PORT: FUN_801cf0d8 state 4 (the timed tally)
    fn tick_payout(&mut self) {
        self.caption_frame += 1;
        if self.payout_left > 0 {
            if self.frame & 1 == 0 {
                return;
            }
            let chunk = if self.payout_left > 20 { 11 } else { 1 };
            self.payout_left -= chunk;
            self.balance = (self.balance + chunk).min(BALANCE_CAP);
            self.sounds.ring.push((0, CUE_PAYOUT_TICK));
        } else if self.caption_frame >= PAYOUT_HOLD_FRAMES {
            self.phase = SlotPhase::Idle;
            self.win_line = -1;
            self.caption_frame = 0;
        }
    }

    /// Stop reel `reel` (a Stop input in state `3`): plan the stop for the
    /// active feature mode, run the landing search from the live row, snap the
    /// reel, and **claim** it - latch the landed payline value + 1 into
    /// [`SlotMachine::claimed`], which is what fills that column of the marquee
    /// tally. Once all three reels are stopped the spin is evaluated and the
    /// machine moves to [`SlotPhase::Payout`]. Returns `false` when stopping
    /// isn't allowed or the reel is already stopped.
    // PORT: FUN_801cf0d8 state 3 (per-reel stop) + FUN_801d0554 (snap + claim)
    pub fn stop_reel(&mut self, reel: usize) -> bool {
        if self.phase != SlotPhase::Stopping || reel >= REEL_COUNT || self.stopped[reel].is_some() {
            return false;
        }
        // The guaranteed-hit mode drives later reels to the first reel's landed
        // symbol so the line connects. (The bonus round does NOT: its stop plan
        // has no target at all - the reel lands where you stopped it.)
        // This spin's landing line (`DAT_801d4134` row of the `0x801d3630`
        // table): a forced target lands on that payline, not on the middle row.
        let line = LANDING_LINE_BY_JITTER[self.jitter.rem_euclid(5) as usize];
        let line_offset = |r: usize| PAYLINE_ROW_OFFSETS[line][r];
        let guarantee = self.stopped.iter().enumerate().find_map(|(r, s)| {
            s.map(|row| {
                let at = (row as i32 + line_offset(r)).rem_euclid(STRIP_LEN as i32);
                self.strips[r][at as usize]
            })
        });
        let (depth, target) = stop_plan(
            &mut self.rng,
            self.feature_mode,
            self.normal_target,
            guarantee,
        );
        let from_row = self.payline_row(reel);
        let row = land_row(
            &self.strips[reel],
            from_row,
            depth,
            target,
            line_offset(reel),
        );
        self.reel_pos[reel] = (row as i32) << 8;
        self.reel_vel[reel] = 0;
        self.stopped[reel] = Some(row);
        // `DAT_801d3d20[reel] = display[reel][payline_row] + 1`, the frame the
        // reel locks. The +1 is retail's, and it is what makes the tally's
        // "unclaimed" state (`0`) distinguishable from a landed value of `0`.
        self.claimed[reel] = self.strips[reel][row] as i32 + 1;
        self.sounds.ring.push((0, CUE_REEL_STOP));
        if self.reels_stopped() == REEL_COUNT {
            // The third stop leaves the scanner's two-stop window; retail's
            // next state-3 frame zeroes the latch before the payout state.
            self.anticipation = 0;
            let result = self.evaluate_spin();
            self.last_result = Some(result);
            self.phase = SlotPhase::Payout;
            // Raise the marquee's payout caption on a paying spin. Retail gates
            // the caption on the figure being non-zero, so a losing spin leaves
            // the matrix to the tally / attract strip.
            self.caption_payout = result.payout;
            // The payout state's timer starts at 0 on a win and at
            // `0x6B` on a loss (`0x801CF850..0x801CF868`); the motor voice
            // is released (`FUN_800653C8(0x13)` at `0x801CF878`).
            self.caption_frame = if result.payout != 0 {
                0
            } else {
                LOSING_PAYOUT_TIMER
            };
            self.payout_left = result.payout;
            self.win_line = result.line.map_or(-1, |l| l as i32);
            self.sounds.voice_off.push(SPIN_VOICE);
        }
        true
    }

    /// Stop the leftmost still-spinning reel (host convenience for a single
    /// stop button; retail maps three pad bits to the three reels).
    pub fn stop_next_reel(&mut self) -> bool {
        (0..REEL_COUNT).any(|r| self.stopped[r].is_none() && self.stop_reel(r))
    }

    /// Evaluate the stopped spin (`FUN_801d13e8`): outside a bonus round,
    /// check all five paylines all-three-equal on the display strips, keep
    /// the highest-value line, pay `payout_table[symbol]`, and trigger the
    /// bonus round on the jackpot symbols. During a bonus round every spin
    /// pays the **product of the three payline numbers**, unconditionally - no
    /// equality check, no payout table - and the payout is subtracted from the
    /// net-take counter.
    // PORT: FUN_801d13e8 (win evaluation + payout lookup + bonus trigger)
    fn evaluate_spin(&mut self) -> SpinResult {
        let rows: [usize; REEL_COUNT] = core::array::from_fn(|r| self.stopped[r].unwrap_or(0));
        let bonus_spin = self.in_bonus_round() && self.bonus_spins > 0;
        if bonus_spin {
            // The bonus round's arithmetic, whole:
            //
            //   payout = (v0 - 0xf) * (v1 - 0xf) * (v2 - 0xf)
            //
            // over the three payline values of the display strip - which, in a
            // bonus round, are bonus-strip values `0x10..=0x19`, so each factor
            // is the numeral 1..=10 drawn on that reel. 1 (1x1x1) to 1000
            // (10x10x10) coins. There is no all-equal gate and no payout-table
            // lookup: every bonus spin pays, and it pays what it shows.
            //
            // The claimed values the tally prints are `value + 1` off the SAME
            // rows, so `tally_product() == payout` by construction, not by
            // agreement between two copies.
            let numbers: [u32; REEL_COUNT] = core::array::from_fn(|r| {
                slot_payout::bonus_number_for_value(self.strips[r][rows[r]])
            });
            let payout = numbers.iter().product::<u32>() as i32;
            let all_equal =
                (1..REEL_COUNT).all(|r| self.strips[r][rows[r]] == self.strips[0][rows[0]]);
            self.net_take -= payout;
            self.bonus_spins -= 1;
            if self.bonus_spins <= 0 {
                self.feature_mode = 0;
                // Latch the "just ended" flag so the next spin runs long enough
                // for the symbols to rotate back onto the payline.
                self.bonus_just_ended = true;
            }
            return SpinResult {
                // Retail forces the winning line to the centre (`DAT_801d3c8c = 1`)
                // - the bonus round pays the middle row and lights its lamp.
                line: Some(1),
                symbol: all_equal.then(|| self.strips[0][rows[0]]),
                payout,
                bonus_triggered: false,
                bonus_spin: true,
            };
        }
        // Five paylines - three horizontal and two diagonal. The per-reel row
        // offsets are `legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS`,
        // read off the retail evaluator's absolute row reads. All five always
        // play; the winning line index is also the medallion the machine lights.
        let mut best: Option<(usize, u8, i32)> = None;
        for (line, offs) in legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS
            .iter()
            .enumerate()
        {
            let sym = |r: usize| {
                let row =
                    (rows[r] as isize + offs[r] as isize).rem_euclid(STRIP_LEN as isize) as usize;
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
            if let Some(rounds) = slot_payout::bonus_rounds_for(sym) {
                // The jackpot symbols kick off the bonus round: 3 rounds for the
                // red "punch" (id 9), 1 for the blue "kick" (id 8). From the next
                // spin the display strip starts rotating onto the numerals.
                self.feature_mode = FEATURE_MODE_BONUS;
                self.bonus_spins = rounds as i32;
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
        // The rest of the tally at once; what the timed tally already moved
        // is in the balance.
        let credit = self.payout_left;
        self.balance = (self.balance + credit).min(BALANCE_CAP);
        self.payout_left = 0;
        self.phase = SlotPhase::Idle;
        self.win_line = -1;
        // The caption comes down with the tally it was captioning.
        self.caption_payout = 0;
        self.caption_frame = 0;
        credit
    }

    /// The marquee's per-frame inputs, read off this machine's live state.
    ///
    /// Every field but the caption pair is a global the machine already keeps:
    /// the feature mode, the reel state word, the bonus-round counter and the
    /// per-reel claimed values are the same storage the payout arithmetic uses,
    /// so the marquee cannot disagree with what the machine actually paid.
    pub fn marquee(&self) -> MarqueeFrame {
        MarqueeFrame {
            payout: self.caption_payout,
            payout_frame: self.caption_frame,
            feature_mode: self.feature_mode,
            reel_state: match self.phase {
                SlotPhase::Idle => 1,
                SlotPhase::Spinning => 2,
                SlotPhase::Stopping => 3,
                SlotPhase::Payout => 4,
                SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving => self.retail_state(),
                SlotPhase::CashedOut => 100,
            },
            bonus_rounds: self.bonus_spins,
            claimed: self.claimed,
        }
    }

    /// What the dot matrix shows this frame, as blit placements. Pair with
    /// [`legaia_asset::minigame_slot_scene::render_marquee`] and a parsed message
    /// bank to rasterise it.
    pub fn marquee_placements(&self) -> Vec<MarqueePlacement> {
        compose_marquee_frame(&self.marquee())
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

// --- The cash-out submenu ----------------------------------------------------

/// Retail packed pad bits (`_DAT_8007B874`, `legaia_engine_core::retail_pad`) the
/// cash-out flow tests. The face/shoulder byte is the low byte, the
/// d-pad/system byte the high one.
pub mod menu_pad {
    /// State 1's submenu edge: Triangle (`0x10`) or Select (`0x100`)
    /// (`andi v0,v1,0x110` at `0x801CF40C`).
    pub const OPEN: u32 = 0x0110;
    /// Cursor up (`0x801CF950`).
    pub const UP: u32 = 0x1000;
    /// Cursor down (`0x801CF974`).
    pub const DOWN: u32 = 0x4000;
    /// Back out: Circle or L2 (`andi v0,a0,0x21` at `0x801CFA2C`).
    pub const CANCEL: u32 = 0x0021;
    /// Take the row / turn the page: Cross or L1 (`andi v0,a0,0x44`).
    pub const CONFIRM: u32 = 0x0044;
    /// Any face button - the not-enough-coins prompt's leave edge (`0xF0`).
    pub const FACE: u32 = 0x00F0;
}

/// Submenu confirm - and the not-enough-coins prompt's leave - into ring slot
/// 0 (`0x801CF418`, `0x801CFA58`, `0x801CFD8C`). A static-table id (`< 0x200`),
/// so it resolves through the class-0 bank, not the machine's own.
pub const CUE_MENU_CONFIRM: i16 = 0x20;
/// Submenu cursor move, and the first instruction page's turn (`0x801CF964`,
/// `0x801CF988`, `0x801CFB3C`).
pub const CUE_MENU_CURSOR: i16 = 0x21;
/// Submenu cancel, and the second instruction page's close (`0x801CFA38`,
/// `0x801CFBA8`).
pub const CUE_MENU_CANCEL: i16 = 0x37;

/// The submenu's rows, in cursor order. The rows' words are a 4bpp image on the
/// art pack (page `(832, 256)`, `uv (0, 160)`, 80x48 - `FUN_801D317C`), so the
/// order is read off the arms state `0x32` branches to, not off any string.
pub const MENU_ROW_PLAY: u32 = 0;
/// Row 1 commits the balance and leaves (state `100`).
pub const MENU_ROW_QUIT: u32 = 1;
/// Row 2 opens the two instruction pages (state `0x33`).
pub const MENU_ROW_RULES: u32 = 2;

/// The fade step the instruction pages and the leave fade ramp by, per frame.
pub const MENU_FADE_STEP: i32 = 0x10;

pub use legaia_asset::minigame_slot_scene::SlotScreen;

impl SlotMachine {
    /// The retail state word (`DAT_801d3c84`) this machine stands for.
    pub fn retail_state(&self) -> i32 {
        match self.phase {
            SlotPhase::Idle => 1,
            SlotPhase::Spinning => 2,
            SlotPhase::Stopping => 3,
            SlotPhase::Payout => 4,
            SlotPhase::Menu => i32::from(self.menu_state),
            SlotPhase::NoCoins => 0x5A,
            SlotPhase::Leaving | SlotPhase::CashedOut => 100,
        }
    }

    /// What to draw ([`SlotScreen`]).
    pub fn screen(&self) -> SlotScreen {
        match (self.phase, self.menu_state) {
            (SlotPhase::Menu, 0x32) => SlotScreen::Picker {
                row: self.menu_cursor,
            },
            (SlotPhase::Menu, 0x35) => SlotScreen::Instructions { page: 0 },
            (SlotPhase::Menu, 0x36 | 0x38) => SlotScreen::Instructions { page: 1 },
            (SlotPhase::NoCoins, _) => SlotScreen::NoCoins,
            _ => SlotScreen::Machine,
        }
    }

    /// The screen-fade level (`DAT_801d3c98`, `0..=0xFF`): how far the frame
    /// is faded to black (`FUN_80024EE4(0, 2, level * 0x10101)`). Non-zero only
    /// across the instruction pages' transitions and the leave fade.
    pub fn fade_level(&self) -> i32 {
        match self.phase {
            SlotPhase::Menu | SlotPhase::Leaving | SlotPhase::CashedOut => self.fade.clamp(0, 0xFF),
            _ => 0,
        }
    }

    /// `true` while the cash-out flow owns the pad: the submenu, its pages,
    /// the not-enough-coins prompt and the leave fade. A host must not feed
    /// spin / stop / collect input on such a frame.
    pub fn in_cash_out_flow(&self) -> bool {
        matches!(
            self.phase,
            SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving
        )
    }

    /// One frame of the cash-out flow on the retail packed pad-edge word
    /// `pressed` ([`menu_pad`]). Call it every frame, **before** the spin /
    /// stop input; it returns `true` when the flow owns this frame (the
    /// host then skips the rest of its slot input).
    ///
    /// In state 1 the submenu edge is tested first - so the submenu opens
    /// even on an empty balance - and only then the `< 3` coin gate, which
    /// raises the not-enough-coins prompt rather than refusing a spin.
    ///
    /// - `0x32`, the picker: Up / Down move the cursor (`0x21` each), kept as
    ///   an unsigned word `% 3`, so Up on row 0 stays put while Down on row 2
    ///   wraps; Circle / L2 cancel back to play (`0x37`); Cross / L1 take the
    ///   row (`0x20`) - play, quit (state `100`), or the rules pages
    ///   (state `0x33`).
    /// - `0x33..=0x39`, the rules: fade to black (`0x34`), page 0 fades in and
    ///   waits for Cross / L1 (`0x35`, `0x21`), page 1 waits for Cross / L1
    ///   (`0x36`, `0x37`), fade out (`0x38`), fade back in on the machine
    ///   (`0x39`), and back to state 1.
    /// - `0x5A`, the prompt: any face button leaves (`0x20`, state `100`).
    /// - `100`: fade out; at full black the balance is committed
    ///   ([`SlotPhase::CashedOut`]).
    ///
    /// Retail runs the confirm test after the cancel test in the same frame,
    /// relative to the state the cancel just wrote; a frame pressing both on
    /// the rules row therefore lands in state 2 with no bet charged. That
    /// collision is not reproduced: the rules row opens the rules.
    // PORT: FUN_801cf0d8 state 1 (submenu edge + coin gate), states 0x32..0x39, 0x5a, 100
    pub fn cash_out_input(&mut self, pressed: u32) -> bool {
        use menu_pad::*;
        match self.phase {
            SlotPhase::Idle => {
                if pressed & OPEN != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    self.menu_cursor = 0;
                    self.menu_state = 0x32;
                    self.phase = SlotPhase::Menu;
                    return true;
                }
                if self.balance < MIN_SPIN_BALANCE {
                    self.phase = SlotPhase::NoCoins;
                    return true;
                }
                false
            }
            SlotPhase::Menu => {
                self.menu_frame(pressed);
                true
            }
            SlotPhase::NoCoins => {
                if pressed & FACE != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    self.phase = SlotPhase::Leaving;
                }
                true
            }
            SlotPhase::Leaving => {
                self.fade += MENU_FADE_STEP;
                if self.fade > 0xFF {
                    self.fade = 0xFF;
                    self.phase = SlotPhase::CashedOut;
                }
                true
            }
            _ => false,
        }
    }

    fn menu_frame(&mut self, pressed: u32) {
        use menu_pad::*;
        let confirm = pressed & CONFIRM != 0;
        match self.menu_state {
            0x32 => {
                if pressed & UP != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CURSOR));
                    self.menu_cursor = self.menu_cursor.wrapping_sub(1);
                }
                if pressed & DOWN != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CURSOR));
                    self.menu_cursor = self.menu_cursor.wrapping_add(1);
                }
                // `multu` by `0xAAAAAAAB`: an unsigned remainder.
                self.menu_cursor %= 3;
                if pressed & CANCEL != 0 {
                    self.sounds.ring.push((0, CUE_MENU_CANCEL));
                    self.phase = SlotPhase::Idle;
                }
                if confirm {
                    self.sounds.ring.push((0, CUE_MENU_CONFIRM));
                    match self.menu_cursor {
                        MENU_ROW_PLAY => self.phase = SlotPhase::Idle,
                        MENU_ROW_QUIT => {
                            // The quit arm stores the confirm cue a second
                            // time (`0x801CFD8C`) - the same slot, one sound.
                            self.fade = 0;
                            self.phase = SlotPhase::Leaving;
                        }
                        _ => {
                            self.phase = SlotPhase::Menu;
                            self.menu_state = 0x33;
                        }
                    }
                }
            }
            0x33 => {
                self.fade = 0;
                self.menu_state = 0x34;
            }
            0x34 | 0x38 => {
                self.fade += MENU_FADE_STEP;
                if self.fade > 0xFF {
                    self.fade = 0xFF;
                    self.menu_state += 1;
                }
            }
            0x35 => {
                self.fade -= MENU_FADE_STEP;
                if self.fade < 0 {
                    self.fade = 0;
                    if confirm {
                        self.sounds.ring.push((0, CUE_MENU_CURSOR));
                        self.menu_state = 0x36;
                    }
                }
            }
            0x36 => {
                if confirm {
                    self.sounds.ring.push((0, CUE_MENU_CANCEL));
                    self.menu_state = 0x38;
                }
            }
            _ => {
                // 0x39: fade back in on the machine.
                self.fade -= MENU_FADE_STEP;
                if self.fade < 0 {
                    self.fade = 0;
                    self.phase = SlotPhase::Idle;
                }
            }
        }
    }
}

/// What one [`SlotMachine::frame`] did that the host owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFrameOutcome {
    /// An ordinary frame.
    Stepped,
    /// The cash-out flow committed this frame (state `100`'s tail): the host
    /// banks the balance and leaves the machine.
    CashedOut,
    /// The machine had already committed its cash-out before this frame:
    /// the host restores whatever the cabinet interrupted.
    Committed,
}

impl SlotMachine {
    /// One frame of the cabinet on this frame's **packed retail edge word**
    /// ([`packed_edges`]) - the whole per-frame step every host runs, so the
    /// play hosts' world tick and the standalone minigames page cannot answer
    /// the same press differently.
    ///
    /// Order is retail's: the reels tick, then the cash-out flow (submenu,
    /// rules pages, the not-enough-coins prompt, the leave fade) takes the
    /// edge when it owns the frame, otherwise any face-button edge latches the
    /// spin-up rarity and the phase reads its own button - Cross spins at
    /// idle, Square / Cross / Circle stop reels 0 / 1 / 2 while they are
    /// stoppable, and Cross on a resolved spin finishes its timed tally at
    /// once ([`Self::collect`]); left alone, state 4 counts the win in by
    /// itself and drops back to idle.
    // PORT: FUN_801cf0d8 (the per-frame state dispatch: state 1 spin, state 3
    //       stops, state 4 collect)
    pub fn frame(&mut self, packed: u32) -> SlotFrameOutcome {
        use legaia_engine_vm::pad::{PACK_CIRCLE, PACK_CROSS, PACK_SQUARE, PACK_TRIANGLE};
        let on = |m: u16| packed & u32::from(m) != 0;
        let phase = self.phase();
        self.tick();
        if self.cash_out_input(packed) {
            return if self.phase() == SlotPhase::CashedOut {
                SlotFrameOutcome::CashedOut
            } else {
                SlotFrameOutcome::Stepped
            };
        }
        self.latch_spin_up(
            on(PACK_TRIANGLE) || on(PACK_CIRCLE) || on(PACK_CROSS) || on(PACK_SQUARE),
        );
        match phase {
            SlotPhase::Idle => {
                if on(PACK_CROSS) {
                    self.spin();
                }
            }
            SlotPhase::Stopping => {
                for (reel, m) in [PACK_SQUARE, PACK_CROSS, PACK_CIRCLE]
                    .into_iter()
                    .enumerate()
                {
                    if on(m) {
                        self.stop_reel(reel);
                    }
                }
            }
            SlotPhase::Payout => {
                if on(PACK_CROSS) {
                    self.collect();
                }
            }
            SlotPhase::CashedOut => return SlotFrameOutcome::Committed,
            SlotPhase::Spinning | SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving => {}
        }
        SlotFrameOutcome::Stepped
    }
}

/// The literal LCG seed the slot overlay's init writes to `DAT_801d3c80` -
/// the seed every host racks a cabinet with, so the same presses land the
/// same reels on the play hosts and the standalone minigames page.
pub const SLOT_RNG_SEED: u32 = 0x6C0A_2AF0;

/// The packed retail edge word for this frame off the engine's raw pad words
/// (`InputState::pad` / `pad_prev`, [`legaia_engine_vm::pad::PadButton`] layout).
pub fn packed_edges(pad: u16, pad_prev: u16) -> u32 {
    u32::from(legaia_engine_vm::pad::retail_packed(pad & !pad_prev))
}

// --- Coin exchange counter --------------------------------------------------

/// Gold price of one casino coin at the exchange counter (`Total Cost` is the
/// requested coin count times this).
pub const COIN_PRICE_GOLD: i32 = 100;

/// Digit slots in the counter's "Coins to Buy" entry field.
pub const COIN_ENTRY_DIGITS: usize = 8;

/// A quote from the casino's coin-exchange counter: what the entered coin
/// count costs and whether the purchase is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinQuote {
    /// Coin count decoded from the entry field.
    pub coins: i32,
    /// `coins * COIN_PRICE_GOLD`.
    pub cost: i32,
    /// The party can pay: `gold >= cost`.
    pub affordable: bool,
    /// The counter can serve it: `stock >= coins`.
    pub in_stock: bool,
}

impl CoinQuote {
    /// Whether the counter will accept this purchase - retail draws the total
    /// in the normal ink only when both gates pass, and in the alert ink when
    /// either fails.
    pub fn is_valid(&self) -> bool {
        self.affordable && self.in_stock
    }
}

/// Decode the counter's per-digit entry field into a coin count.
///
/// The field is [`COIN_ENTRY_DIGITS`] single-digit cells stored
/// **least-significant first** (the accumulator starts at 1 and multiplies by
/// ten each cell), so `digits[0]` is the units place. The cells are signed
/// bytes: retail loads them with `lb` (`0x801E6FBC`).
// PORT: FUN_801e6f70 (`0x801E6FB8..0x801E6FE4`, the entry field's accumulator)
pub fn coin_entry_value(digits: &[i8]) -> i32 {
    let mut place = 1i32;
    let mut total = 0i32;
    for &d in digits.iter().take(COIN_ENTRY_DIGITS) {
        total = total.wrapping_add(place.wrapping_mul(i32::from(d)));
        place = place.wrapping_mul(10);
    }
    total
}

/// Quote the coin-exchange counter for the entered `digits`, against the
/// party's `gold` and the counter's buyable ceiling `stock`.
///
/// Coins cost a flat [`COIN_PRICE_GOLD`] each. Retail gates the sale twice -
/// on the party's gold (`_DAT_8008459C`) against the total, and on the
/// ceiling the counter's state machine publishes every frame
/// (`_DAT_8007BB90`, `min(gold / 100, bank headroom)`) against the coin
/// count - and recolours the total to the alert ink when *either* fails. The
/// bank word this feeds (`_DAT_800845A4`) is the same one
/// [`SlotMachine::cash_out`] assigns back.
///
/// This is the quote half of the counter's entry panel
/// ([`coin_entry_panel`]); the sale itself commits in the state machine
/// (`legaia_engine_vm::baka_hub_actors::coin_exchange`, state 3).
// PORT: FUN_801e6f70 (`0x801E7138..0x801E7174`, the cost and its two gates)
pub fn coin_exchange_quote(digits: &[i8], gold: i32, stock: i32) -> CoinQuote {
    let coins = coin_entry_value(digits);
    let cost = coins.wrapping_mul(COIN_PRICE_GOLD);
    CoinQuote {
        coins,
        cost,
        affordable: gold >= cost,
        in_stock: stock >= coins,
    }
}

/// Record of the field overlay's panel-window table (`0x801F2B98`) whose
/// painter is [`coin_entry_panel`]: the coin counter's idle descriptor
/// (`0x801F3340`) opens it.
pub const COIN_ENTRY_WINDOW: usize = 10;

/// Rodata VAs of the entry panel's four labels, in draw order (bank, entry,
/// gold, total). The bytes are the user's disc's; hosts read them off the
/// field overlay (PROT 0897).
pub const COIN_ENTRY_LABEL_VAS: [u32; 4] = [0x801C_F0D4, 0x801C_F0E0, 0x801C_F0F0, 0x801C_F0FC];

/// System-UI sprite cell the panel draws under the edited digit
/// (`FUN_8002C488(x, y, 0x67)`).
pub const COIN_ENTRY_CARET_CELL: i32 = 0x67;

/// Text pens (`_DAT_8007B454`) the panel stages: the default white, the
/// accent the entry row takes, the normal total and the refused total.
pub const COIN_PEN_DEFAULT: u8 = 7;
pub const COIN_PEN_ENTRY: u8 = 6;
pub const COIN_PEN_TOTAL: u8 = 5;
pub const COIN_PEN_REFUSED: u8 = 9;

/// One draw of the coin counter's entry panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinPanelDraw {
    /// `FUN_8003CD00(label, x, y)` in pen `pen`.
    Label { va: u32, x: i16, y: i16, pen: u8 },
    /// `FUN_80034B78(value, digits, x, y)` - a blank-padded, right-aligned
    /// decimal field of 8-pixel cells, in pen `pen`.
    Number {
        value: i32,
        digits: u8,
        x: i16,
        y: i16,
        pen: u8,
    },
    /// `FUN_8002C488(x, y, cell)` - the caret under the edited digit.
    Cell { x: i16, y: i16, cell: i32 },
}

/// What the entry panel reads: its window's origin and the counter's live
/// globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinPanelInput<'a> {
    /// The window actor's `+0x0A` / `+0x0C` - where the descriptor's open
    /// placed it ([`COIN_ENTRY_ORIGIN`] for the retail record).
    pub origin: (i16, i16),
    /// `DAT_801F35F0..+7`, least significant first.
    pub digits: &'a [i8],
    /// `_DAT_8007BB9C` - the edited cell.
    pub cursor: i32,
    /// `_DAT_800845A4` - the coin bank.
    pub bank: i32,
    /// `_DAT_8008459C` - party gold.
    pub gold: i32,
    /// `_DAT_8007BB90` - the buyable ceiling.
    pub stock: i32,
    /// `_DAT_80084570` - the play clock; bits `0xC` blink the caret.
    pub clock: u32,
}

/// Where record [`COIN_ENTRY_WINDOW`]'s open places its window: the record's
/// geometry words at `+8` / `+0xA` (`(0x40, 0x26)`), which the installer's
/// open sub-op hands to `FUN_800357FC` as the window actor's target.
pub const COIN_ENTRY_ORIGIN: (i16, i16) = (0x40, 0x26);

/// The coin counter's **entry panel**: the painter of panel-window record
/// [`COIN_ENTRY_WINDOW`], drawn every frame the counter's idle descriptor
/// has the window open - the digit entry and the confirm both.
///
/// Four rows from the window origin `(x, y)`:
///
/// | Row | Label | Value |
/// |---|---|---|
/// | `y + 2` | label 0, pen 7 | the coin bank, 8 digits at `x + 0x78` |
/// | `y + 0x12` | label 1, pen 6 | cells `5..=0`, one digit each from `x + 0x88`, 8 px apart |
/// | `y + 0x30` | label 2, pen 7 | party gold, 8 digits at `x + 0x78` |
/// | `y + 0x40` | label 3, pen 7 | the total cost, 8 digits at `x + 0x78`, pen 5 or 9 |
///
/// The caret cell sits at `(x + 0xB0 - 8 * cursor, y + 0x1D)` - under the
/// edited digit, the units cell rightmost - and draws only while the cursor
/// is below ten and the play clock's `0xC` bits are non-zero, a blink three
/// frames in four. The total takes the refused pen whenever
/// [`coin_exchange_quote`] fails either gate. Only cells `0..=5` are drawn;
/// the two top cells are summed into the total but never shown.
///
/// Read from the field overlay's bytes (PROT 0897, base `0x801CE818`,
/// `see ghidra/scripts/funcs/801e6f70.txt`). The epilogue restores pen 7.
// PORT: FUN_801e6f70
pub fn coin_entry_panel(input: &CoinPanelInput<'_>) -> Vec<CoinPanelDraw> {
    let (x, y) = input.origin;
    let at = |dx: i32, dy: i32| ((i32::from(x) + dx) as i16, (i32::from(y) + dy) as i16);
    let quote = coin_exchange_quote(input.digits, input.gold, input.stock);
    let mut out = Vec::new();
    let label = |va: u32, (x, y): (i16, i16), pen: u8| CoinPanelDraw::Label { va, x, y, pen };
    let number = |value: i32, digits: u8, (x, y): (i16, i16), pen: u8| CoinPanelDraw::Number {
        value,
        digits,
        x,
        y,
        pen,
    };

    out.push(label(COIN_ENTRY_LABEL_VAS[0], at(0, 2), COIN_PEN_DEFAULT));
    out.push(number(input.bank, 8, at(0x78, 2), COIN_PEN_DEFAULT));

    out.push(label(COIN_ENTRY_LABEL_VAS[1], at(0, 0x12), COIN_PEN_ENTRY));
    for (col, cell) in (0..=5usize).rev().enumerate() {
        let d = input.digits.get(cell).copied().unwrap_or(0);
        out.push(number(
            i32::from(d),
            1,
            at(0x88 + 8 * col as i32, 0x12),
            COIN_PEN_ENTRY,
        ));
    }
    // `slti v0,a0,0xa` is a signed compare, and the blink tests the clock.
    if input.cursor < 10 && input.clock & 0xC != 0 {
        let (cx, cy) = at(0xB0 - 8 * input.cursor, 0x1D);
        out.push(CoinPanelDraw::Cell {
            x: cx,
            y: cy,
            cell: COIN_ENTRY_CARET_CELL,
        });
    }

    out.push(label(
        COIN_ENTRY_LABEL_VAS[2],
        at(0, 0x30),
        COIN_PEN_DEFAULT,
    ));
    out.push(number(input.gold, 8, at(0x78, 0x30), COIN_PEN_DEFAULT));

    out.push(label(
        COIN_ENTRY_LABEL_VAS[3],
        at(0, 0x40),
        COIN_PEN_DEFAULT,
    ));
    let pen = if quote.is_valid() {
        COIN_PEN_TOTAL
    } else {
        COIN_PEN_REFUSED
    };
    out.push(number(quote.cost, 8, at(0x78, 0x40), pen));
    out
}

// --- payline draw list -----------------------------------------------------

/// GP0 command byte of a payline segment: `0x43` - a flat (non-gouraud),
/// **semi-transparent** two-point line.
pub const PAYLINE_GP0_CODE: u8 = 0x43;

/// Colour every unlit payline draws in - a neutral half-grey.
pub const PAYLINE_COLOR_IDLE: (u8, u8, u8) = (0x80, 0x80, 0x80);

/// Colour the lit payline draws in. Retail overwrites only the three
/// colour bytes of the already-assembled command word, so the `0x43` code
/// byte survives and the line stays semi-transparent.
pub const PAYLINE_COLOR_LIT: (u8, u8, u8) = (0xFF, 0xFF, 0x80);

/// One payline segment ready to draw: the two model-space endpoints the
/// caller projects, plus the resolved GPU packet fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaylinePrim {
    /// Payline index `0..5`. Doubles as the medallion / lamp index.
    pub index: usize,
    /// The segment's two endpoints, straight out of the geometry table.
    pub a: legaia_asset::minigame_slot_scene::Pos3,
    pub b: legaia_asset::minigame_slot_scene::Pos3,
    /// 24-bit modulation colour.
    pub color: (u8, u8, u8),
    /// Always [`PAYLINE_GP0_CODE`].
    pub code: u8,
    /// True for the line matching the winning-line index.
    pub lit: bool,
}

/// Build the five payline line-prims for one frame.
///
/// `winning_line` is retail's `DAT_801d3c8c`, compared for **equality**
/// against each line index - so a frame where that word still holds `0`
/// lights line 0, and only a value outside `0..5` leaves every line unlit.
/// The caller supplies the geometry table
/// ([`legaia_asset::minigame_slot_scene::SlotScene::paylines`], disc data
/// at `DAT_801d3680`); nothing here is hard-coded geometry.
///
/// Projection and ordering-table linkage stay caller-side, as they do for
/// the rest of the machine's 3D furniture: retail `RTPS`-projects each
/// endpoint on its own through `FUN_8003d368` and links the packet at
/// [`payline_ot_depth`] of the **second** endpoint's returned depth.
// REF: FUN_8003d368 (the SCUS RTPS wrapper each endpoint goes through; the
// browser pages run its equivalent caller-side, over the same projection)
// PORT: FUN_801d3380 (payline 3D line segments)
//
// Wired on all three hosts through one projection, [`projected_paylines`]:
// the minigames page (`slot_payline_prims_json`) and the play page
// (`play_mg_slot_payline_prims_json`) hand their page each prim with its
// projected endpoints, which the page strokes as a two-point line in the
// prim's colour, half-blended when the `0x43` code carries the
// semi-transparency bit; the native window draws the same segments as
// one-pixel flat quads (`legaia_engine_ui::ui_slot_paylines`) off
// [`SlotMachine::payline_segments`]. The native window still has no model draw
// of the cabinet mesh (PROT 1200 descriptor 1) around them - see
// docs/subsystems/minigame-slot-machine.md, "The cabinet is a mesh".
pub fn payline_prims(
    paylines: &[legaia_asset::minigame_slot_scene::PayLine],
    winning_line: i32,
) -> Vec<PaylinePrim> {
    paylines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let lit = index as i32 == winning_line;
            PaylinePrim {
                index,
                a: line.a,
                b: line.b,
                color: if lit {
                    PAYLINE_COLOR_LIT
                } else {
                    PAYLINE_COLOR_IDLE
                },
                code: PAYLINE_GP0_CODE,
                lit,
            }
        })
        .collect()
}

/// One payline prim with both endpoints projected onto the retail 640x240
/// slot framebuffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPayline {
    pub prim: PaylinePrim,
    /// Screen position of each endpoint.
    pub a: (f32, f32),
    pub b: (f32, f32),
}

/// The projection pass [`payline_prims`] leaves caller-side, run once for
/// every host: each endpoint goes through
/// [`legaia_asset::minigame_slot_scene::project`] - the machine's captured
/// projection (the stand-in for `FUN_8003D368`'s `RTPS` under the camera the
/// overlay installs) that the rest of the cabinet is drawn with.
///
/// REF: FUN_801d3380 (the two `FUN_8003D368` calls per segment)
pub fn projected_paylines(
    paylines: &[legaia_asset::minigame_slot_scene::PayLine],
    winning_line: i32,
) -> Vec<ProjectedPayline> {
    use legaia_asset::minigame_slot_scene::project;
    payline_prims(paylines, winning_line)
        .into_iter()
        .map(|prim| ProjectedPayline {
            prim,
            a: project(prim.a.x as i32, prim.a.y as i32, prim.a.z as i32),
            b: project(prim.b.x as i32, prim.b.y as i32, prim.b.z as i32),
        })
        .collect()
}

/// Ordering-table bucket for a payline packet: `(depth >> 2) >> ot_shift`,
/// with retail's round-toward-zero fixup (`depth + 3` before the shift when
/// negative) and `ot_shift` the frame context's `+0x90` byte.
pub fn payline_ot_depth(projected_depth: i32, ot_shift: u32) -> i32 {
    let biased = if projected_depth < 0 {
        projected_depth + 3
    } else {
        projected_depth
    };
    (biased >> 2) >> ot_shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payline_prims_light_exactly_the_winning_line() {
        use legaia_asset::minigame_slot_scene::{PayLine, Pos3};
        let p = |y: i16| PayLine {
            a: Pos3 {
                x: -640,
                y,
                z: -768,
            },
            b: Pos3 { x: 640, y, z: -768 },
        };
        let table = [p(-192), p(0), p(192), p(320), p(-320)];

        let prims = payline_prims(&table, 2);
        assert_eq!(prims.len(), 5);
        assert_eq!(
            prims.iter().map(|q| q.lit).collect::<Vec<_>>(),
            [false, false, true, false, false]
        );
        assert_eq!(prims[2].color, PAYLINE_COLOR_LIT);
        assert_eq!(prims[0].color, PAYLINE_COLOR_IDLE);
        // The code byte is the same on both - retail patches only the
        // colour bytes, so the line stays semi-transparent when lit.
        assert!(prims.iter().all(|q| q.code == PAYLINE_GP0_CODE));
        // Geometry passes through untouched.
        assert_eq!(prims[3].a, table[3].a);
        assert_eq!(prims[3].b, table[3].b);
    }

    #[test]
    fn payline_index_zero_lights_when_the_winner_word_is_zero() {
        use legaia_asset::minigame_slot_scene::{PayLine, Pos3};
        let z = Pos3 { x: 0, y: 0, z: 0 };
        let table = [PayLine { a: z, b: z }; 5];
        assert!(payline_prims(&table, 0)[0].lit);
        // Anything outside 0..5 leaves the whole rack dark.
        assert!(payline_prims(&table, -1).iter().all(|q| !q.lit));
        assert!(payline_prims(&table, 9).iter().all(|q| !q.lit));
    }

    #[test]
    fn payline_ot_depth_rounds_toward_zero_then_shifts() {
        assert_eq!(payline_ot_depth(16, 0), 4);
        assert_eq!(payline_ot_depth(16, 2), 1);
        // Negative depths take the +3 bias so the >>2 truncates toward zero.
        assert_eq!(payline_ot_depth(-1, 0), 0);
        assert_eq!(payline_ot_depth(-4, 0), -1);
        assert_eq!(payline_ot_depth(-5, 0), -1);
    }

    #[test]
    fn coin_entry_field_is_least_significant_first() {
        // Units in slot 0: 1234 = 4,3,2,1 then blanks.
        assert_eq!(coin_entry_value(&[4, 3, 2, 1, 0, 0, 0, 0]), 1234);
        assert_eq!(coin_entry_value(&[0; 8]), 0);
        // Every slot filled with 9 = the widest enterable count.
        assert_eq!(coin_entry_value(&[9; 8]), 99_999_999);
    }

    #[test]
    fn coin_exchange_charges_a_hundred_gold_each() {
        let q = coin_exchange_quote(&[5, 0, 0, 0, 0, 0, 0, 0], 1000, 100);
        assert_eq!(q.coins, 5);
        assert_eq!(q.cost, 500);
        assert!(q.is_valid());
    }

    #[test]
    fn coin_exchange_gates_on_gold_and_on_stock_independently() {
        // Affordable but the counter is short: stock gate alone fails.
        let q = coin_exchange_quote(&[9, 0, 0, 0, 0, 0, 0, 0], 100_000, 5);
        assert!(q.affordable, "gold covers 900");
        assert!(!q.in_stock, "counter only holds 5");
        assert!(!q.is_valid());

        // In stock but the party is short: gold gate alone fails.
        let q = coin_exchange_quote(&[9, 0, 0, 0, 0, 0, 0, 0], 100, 100);
        assert!(!q.affordable);
        assert!(q.in_stock);
        assert!(!q.is_valid());
    }

    fn panel(digits: &[i8], cursor: i32, gold: i32, stock: i32, clock: u32) -> Vec<CoinPanelDraw> {
        coin_entry_panel(&CoinPanelInput {
            origin: COIN_ENTRY_ORIGIN,
            digits,
            cursor,
            bank: 42,
            gold,
            stock,
            clock,
        })
    }

    #[test]
    fn the_entry_panel_lays_out_four_rows_off_the_window_origin() {
        let (x, y) = COIN_ENTRY_ORIGIN;
        let d = panel(&[4, 3, 2, 1, 0, 0, 0, 0], 0, 5_000, 50, 4);
        let labels: Vec<_> = d
            .iter()
            .filter_map(|e| match *e {
                CoinPanelDraw::Label { va, x, y, pen } => Some((va, x, y, pen)),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                (COIN_ENTRY_LABEL_VAS[0], x, y + 2, 7),
                (COIN_ENTRY_LABEL_VAS[1], x, y + 0x12, 6),
                (COIN_ENTRY_LABEL_VAS[2], x, y + 0x30, 7),
                (COIN_ENTRY_LABEL_VAS[3], x, y + 0x40, 7),
            ]
        );
        let numbers: Vec<_> = d
            .iter()
            .filter_map(|e| match *e {
                CoinPanelDraw::Number {
                    value,
                    digits,
                    x,
                    y,
                    pen,
                } => Some((value, digits, x, y, pen)),
                _ => None,
            })
            .collect();
        // Bank, six single cells (cell 5 leftmost), gold, total.
        assert_eq!(numbers[0], (42, 8, x + 0x78, y + 2, 7));
        let cells: Vec<_> = numbers[1..7].iter().map(|n| (n.0, n.2)).collect();
        assert_eq!(
            cells,
            vec![
                (0, x + 0x88),
                (0, x + 0x90),
                (1, x + 0x98),
                (2, x + 0xA0),
                (3, x + 0xA8),
                (4, x + 0xB0),
            ]
        );
        assert_eq!(numbers[7], (5_000, 8, x + 0x78, y + 0x30, 7));
        assert_eq!(
            numbers[8],
            (123_400, 8, x + 0x78, y + 0x40, COIN_PEN_REFUSED)
        );
    }

    #[test]
    fn the_caret_sits_under_the_edited_cell_and_blinks_on_the_clock() {
        let (x, y) = COIN_ENTRY_ORIGIN;
        let caret = |d: &[CoinPanelDraw]| {
            d.iter().find_map(|e| match *e {
                CoinPanelDraw::Cell { x, y, cell } => Some((x, y, cell)),
                _ => None,
            })
        };
        let d = panel(&[0; 8], 2, 0, 0, 4);
        assert_eq!(caret(&d), Some((x + 0xA0, y + 0x1D, COIN_ENTRY_CARET_CELL)));
        // The clock's 0xC bits clear: the caret is off this frame.
        assert_eq!(caret(&panel(&[0; 8], 2, 0, 0, 0x10)), None);
        assert_eq!(caret(&panel(&[0; 8], 2, 0, 0, 3)), None);
    }

    #[test]
    fn the_total_takes_the_normal_pen_only_when_both_gates_pass() {
        let pen_of = |d: Vec<CoinPanelDraw>| match d.last() {
            Some(CoinPanelDraw::Number { pen, value, .. }) => (*pen, *value),
            _ => panic!("the total is the last draw"),
        };
        assert_eq!(
            pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 500, 5, 4)),
            (COIN_PEN_TOTAL, 500)
        );
        assert_eq!(
            pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 499, 5, 4)).0,
            COIN_PEN_REFUSED
        );
        assert_eq!(
            pen_of(panel(&[5, 0, 0, 0, 0, 0, 0, 0], 0, 500, 4, 4)).0,
            COIN_PEN_REFUSED
        );
    }

    #[test]
    fn coin_exchange_allows_exactly_affordable_and_exact_stock() {
        // Both gates are `>=`, so an exact match still sells.
        let q = coin_exchange_quote(&[3, 0, 0, 0, 0, 0, 0, 0], 300, 3);
        assert_eq!(q.cost, 300);
        assert!(q.is_valid());
    }

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
        let mut rng = SlotRng::new(12345);
        let (symbols, bonus) = build_reel(&mut rng);
        // The symbol half probes by STRIP_PROBE_PRIMARY over base 0; the bonus
        // half probes by STRIP_PROBE_SECONDARY over BONUS_VALUE_BASE. Both are
        // two-of-each permutations of the ten values.
        for (strip, base, probe) in [
            (symbols, 0u8, STRIP_PROBE_PRIMARY),
            (bonus, slot_payout::BONUS_VALUE_BASE, STRIP_PROBE_SECONDARY),
        ] {
            let mut counts = [0usize; SYMBOL_COUNT];
            for &s in &strip {
                let id = s.wrapping_sub(base) as usize;
                assert!(id < SYMBOL_COUNT, "symbol id in range");
                counts[id] += 1;
            }
            assert_eq!(
                counts, [2; SYMBOL_COUNT],
                "each symbol twice (probe {probe})"
            );
        }
    }

    /// The bonus strip is the same permutation over a rebased value space: the
    /// numerals `1..=10` as values `0x10..=0x19`, two rows each.
    #[test]
    fn the_bonus_strip_carries_the_numerals_as_values_0x10_to_0x19() {
        let mut rng = SlotRng::new(0xBEEF);
        let (symbols, bonus) = build_reel(&mut rng);
        let mut counts = [0usize; SYMBOL_COUNT];
        for &v in &bonus {
            let n = slot_payout::bonus_number_for_value(v);
            assert!(
                (slot_payout::BONUS_VALUE_BASE..=0x19).contains(&v),
                "bonus row {v:#x} is a bonus-strip value"
            );
            assert!((1..=10).contains(&n), "and shows a numeral 1..=10");
            counts[(n - 1) as usize] += 1;
        }
        assert_eq!(counts, [2; SYMBOL_COUNT], "each numeral twice");
        // The two strips are independent permutations - not the same order.
        assert!(
            symbols
                .iter()
                .zip(bonus.iter())
                .any(|(&s, &b)| s + slot_payout::BONUS_VALUE_BASE != b),
            "the two strips are shuffled independently"
        );
    }

    #[test]
    fn land_row_finds_target_within_depth_else_next_row() {
        let mut strip = [0u8; STRIP_LEN];
        strip[9] = 7;
        // From payline row 2 retail searches rows 7.. (raw cur+1..): depth 4
        // reaches row 9, and the middle line puts the target on the payline.
        assert_eq!(land_row(&strip, 2, 4, Some(7), 0), 9);
        // The top line stops one row short, so the target shows one row up
        // (`centre + 1`), which is what line 0's `+1` offset reads.
        assert_eq!(land_row(&strip, 2, 4, Some(7), 1), 8);
        // The bottom line stops one row past it.
        assert_eq!(land_row(&strip, 2, 4, Some(7), -1), 10);
        // Depth too shallow (rows 7, 8) -> next natural row.
        assert_eq!(land_row(&strip, 2, 2, Some(7), 0), 3);
        // The current payline row is never a candidate.
        strip[2] = 7;
        assert_eq!(land_row(&strip, 2, 2, Some(7), 0), 3);
        // Wraps around the strip end: from 14 the window is 19, 0, 1.
        strip[1] = 9;
        assert_eq!(land_row(&strip, 14, 3, Some(9), -1), 2);
        // The bonus round's plan (depth 0 / no target) never searches: the reel
        // lands on the next row, wherever that is.
        assert_eq!(land_row(&strip, 5, 0, None, 0), 6);
        assert_eq!(land_row(&strip, 19, 0, None, 0), 0);
    }

    /// The landing line follows the spin's jitter: with the same strip and the
    /// same search, each `rand % 5` row lands the target on its own payline,
    /// and the win the evaluator finds is on that line.
    #[test]
    fn landing_line_follows_the_jitter_row() {
        for (jitter, &line) in LANDING_LINE_BY_JITTER.iter().enumerate() {
            let mut strip = [0u8; STRIP_LEN];
            strip[9] = 7;
            let rows: Vec<usize> = (0..REEL_COUNT)
                .map(|r| land_row(&strip, 2, 4, Some(7), PAYLINE_ROW_OFFSETS[line][r]))
                .collect();
            for (r, &row) in rows.iter().enumerate() {
                let shown =
                    (row as i32 + PAYLINE_ROW_OFFSETS[line][r]).rem_euclid(STRIP_LEN as i32);
                assert_eq!(strip[shown as usize], 7, "jitter {jitter} reel {r}");
            }
        }
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
        assert_eq!(t, Some(4));
        let (d, t) = stop_plan(&mut rng, 1, 4, None);
        assert!((6..10).contains(&d), "reach depth = (rand&3) + 6");
        assert_eq!(t, Some(9));
        let (_, t) = stop_plan(&mut rng, 2, 4, None);
        assert_eq!(t, Some(8));
        // Guaranteed mode drives to the already-landed symbol.
        let (d, t) = stop_plan(&mut rng, 4, 4, Some(6));
        assert_eq!((d, t), (STRIP_LEN, Some(6)));
        // The bonus round steers nothing: no target, no search.
        assert_eq!(
            stop_plan(&mut rng, FEATURE_MODE_BONUS, 4, Some(6)),
            (0, None)
        );
    }

    /// `FUN_801d1af4`: two landed reels showing a punch pair on a payline
    /// raise the latch to `1`, a kick pair to `2`; one landed reel, or a pair
    /// on a reel still spinning, raises nothing; the third stop clears it.
    #[test]
    fn two_landed_bonus_symbols_raise_the_anticipation_latch() {
        let mut m = SlotMachine::new(payouts(), 42, 50);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        assert_eq!(m.phase(), SlotPhase::Stopping);
        let plant = |m: &mut SlotMachine, sym: u8| {
            for r in 0..REEL_COUNT {
                m.reel_pos[r] = 0;
                m.strips[r] = [r as u8; STRIP_LEN];
            }
            // Top row (line 0, `+1`) of reels 0 and 2 - the `(0,2)` pair.
            m.strips[0][1] = sym;
            m.strips[2][1] = sym;
        };
        plant(&mut m, legaia_asset::slot_payout::PUNCH_SYMBOL_ID);
        m.stopped = [Some(0), None, None];
        m.tick();
        assert_eq!(m.anticipation(), 0, "one landed reel is not a pair");
        m.stopped = [Some(0), None, Some(0)];
        m.tick();
        assert_eq!(m.anticipation(), 1, "a punch pair");
        plant(&mut m, legaia_asset::slot_payout::KICK_SYMBOL_ID);
        m.tick();
        assert_eq!(m.anticipation(), 2, "a kick pair");
        assert!(m.stop_reel(1));
        assert_eq!(m.anticipation(), 0, "the third stop clears it");
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
    fn a_spin_up_press_latches_for_exactly_one_roll() {
        let mut m = SlotMachine::new(payouts(), 42, 200);
        // Idle: the latch is state 2's alone.
        m.latch_spin_up(true);
        assert!(!m.spin_up_pressed());
        assert!(m.spin());
        m.tick();
        assert_eq!(m.phase(), SlotPhase::Spinning);
        m.latch_spin_up(false);
        assert!(!m.spin_up_pressed(), "no edge, no latch");
        m.latch_spin_up(true);
        assert!(m.spin_up_pressed(), "a face edge mid spin-up latches");
        // Run the spin out and start the next: its roll reads the latch, and
        // state 1 clears it straight after.
        while m.phase() == SlotPhase::Spinning {
            m.tick();
        }
        for r in 0..REEL_COUNT {
            m.stop_reel(r);
            for _ in 0..0x40 {
                m.tick();
            }
        }
        if m.phase() == SlotPhase::Payout {
            m.collect();
        }
        assert!(m.spin_up_pressed(), "survives until the next roll");
        assert!(m.spin());
        assert!(!m.spin_up_pressed(), "the next spin's roll consumed it");
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

    /// Rig a matching line of `sym` on the middle row and stop out of it, so the
    /// spin resolves as a win on that symbol.
    fn win_on(m: &mut SlotMachine, sym: u8) {
        assert!(m.spin());
        while m.phase() != SlotPhase::Stopping {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            m.strips[reel] = [sym; STRIP_LEN];
        }
        m.stop_reel(0);
        m.stop_reel(1);
        m.stop_reel(2);
    }

    /// Play one whole bonus round: spin, run the (long) spin-up out, stop the
    /// three reels.
    fn play_bonus_spin(m: &mut SlotMachine) {
        assert!(m.spin());
        while m.phase() != SlotPhase::Stopping {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            // A few frames between stops, as a player's fingers would.
            m.tick();
            assert!(m.stop_reel(reel));
        }
    }

    #[test]
    fn jackpot_symbols_trigger_the_bonus_round_and_product_payout() {
        let mut m = SlotMachine::new(payouts(), 9, 100);
        win_on(&mut m, 9);
        let r = m.last_result().expect("evaluated");
        assert!(r.bonus_triggered);
        assert!(m.in_bonus_round());
        assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT);
        m.collect();

        // A bonus "free" spin still costs 1 coin (the mode-4..6 charge).
        let before = m.balance();
        let take_before = m.net_take();
        play_bonus_spin(&mut m);
        assert_eq!(
            m.balance(),
            before - SPIN_COST_FEATURE,
            "feature spin costs 1 coin"
        );
        let r = m.last_result().expect("evaluated");
        assert!(r.bonus_spin);
        assert_eq!(r.line, Some(1), "a bonus round pays the centre line");

        // The reels carry numbers now, and the payout is their product.
        let numbers = m.tally();
        assert!(
            numbers.iter().all(|&n| (1..=10).contains(&n)),
            "three numerals 1..=10, got {numbers:?}"
        );
        assert_eq!(
            r.payout,
            numbers.iter().product::<u32>() as i32,
            "the payout is the product of the three numbers the tally shows"
        );
        assert!((1..=1000).contains(&r.payout), "bounded 1..=1000");

        assert_eq!(m.bonus_spins(), BONUS_SPINS_JACKPOT - 1);
        assert_eq!(
            m.net_take(),
            take_before + NET_TAKE_FEATURE_SPIN - r.payout,
            "bonus payout is subtracted from the net take"
        );
    }

    /// The reels really do swap: after a bonus round opens, the value under
    /// every payline is a bonus-strip value, so the renderer draws the numeral
    /// art - and after the round ends they swap back to symbols.
    #[test]
    fn the_bonus_round_rotates_the_numbers_onto_the_reels_and_back_off() {
        let mut m = SlotMachine::new(payouts(), 0x51075, 200);
        // One kick = exactly one bonus round, so the round boundary is crisp.
        win_on(&mut m, slot_payout::KICK_SYMBOL_ID);
        assert!(m.in_bonus_round());
        assert_eq!(m.bonus_spins(), BONUS_SPINS_BONUS);
        m.collect();

        play_bonus_spin(&mut m);
        for r in 0..REEL_COUNT {
            let v = m.payline_symbol(r);
            assert!(
                v >= slot_payout::BONUS_VALUE_BASE,
                "reel {r} pays on a bonus value, got {v:#x}"
            );
        }
        // The single round is spent: the machine is back in the normal game.
        assert!(!m.in_bonus_round());
        assert_eq!(m.bonus_spins(), 0);
        m.collect();

        // ...and the next spin's payline is a symbol again - the strip rotated
        // back on its own, which is what the long post-bonus spin-up buys.
        assert!(m.spin());
        while m.phase() != SlotPhase::Stopping {
            m.tick();
        }
        for r in 0..REEL_COUNT {
            m.tick();
            m.stop_reel(r);
            let v = m.payline_symbol(r);
            assert!(
                v < slot_payout::BONUS_VALUE_BASE,
                "reel {r} is back on a symbol id, got {v:#x}"
            );
        }
    }

    /// The falsifiable half of the tally: it is not a display copy that could
    /// drift from the result. Each column fills only as its reel is claimed, and
    /// the finished tally's product **is** the payout the evaluator computed.
    #[test]
    fn the_tally_fills_per_claimed_reel_and_its_product_is_the_payout() {
        let mut m = SlotMachine::new(payouts(), 7, 200);
        win_on(&mut m, slot_payout::PUNCH_SYMBOL_ID);
        assert!(m.in_bonus_round());
        m.collect();

        assert!(m.spin());
        // The bet charge clears the tally: `0 x 0 x 0`.
        assert_eq!(m.tally(), [0, 0, 0]);
        assert!(!m.tally_complete());
        assert_eq!(m.tally_product(), 0);
        while m.phase() != SlotPhase::Stopping {
            m.tick();
        }
        for reel in 0..REEL_COUNT {
            m.tick();
            assert!(m.stop_reel(reel));
            let tally = m.tally();
            // Exactly the claimed columns are filled; the rest still read 0.
            for (r, &n) in tally.iter().enumerate() {
                if r <= reel {
                    assert!((1..=10).contains(&n), "claimed column {r} shows its number");
                    assert_eq!(
                        n,
                        slot_payout::bonus_number_for_value(m.payline_symbol(r)),
                        "and it is the number that reel actually stopped on"
                    );
                } else {
                    assert_eq!(n, 0, "unclaimed column {r} still reads 0");
                }
            }
        }
        let r = m.last_result().expect("evaluated");
        assert!(m.tally_complete());
        assert_eq!(
            m.tally_product() as i32,
            r.payout,
            "the tally's product is the coins the round pays"
        );
        // And the balance takes exactly that.
        let before = m.balance();
        assert_eq!(m.collect(), r.payout);
        assert_eq!(m.balance(), before + r.payout);
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

    /// Rig a machine whose three reels are stopped on row 0 with the given
    /// per-reel row offsets carrying symbol `sym`, and every other cell distinct.
    fn rigged(sym: u8, offsets: [i32; REEL_COUNT]) -> SpinResult {
        let mut m = SlotMachine::new(payouts(), 9, 100);
        assert!(m.spin());
        for _ in 0..SPIN_UP_FRAMES {
            m.tick();
        }
        for (reel, &off) in offsets.iter().enumerate() {
            // Fill with symbols that can never line up across all three reels.
            let mut s = [0u8; STRIP_LEN];
            for (row, cell) in s.iter_mut().enumerate() {
                *cell = ((reel * 3 + row) % 5) as u8;
            }
            s[off.rem_euclid(STRIP_LEN as i32) as usize] = sym;
            m.strips[reel] = s;
            m.reel_pos[reel] = 0;
            m.reel_vel[reel] = 0;
            m.stopped[reel] = Some(0);
        }
        m.evaluate_spin()
    }

    #[test]
    fn all_five_paylines_always_play() {
        // There is no bet-line selection: a match on ANY of the five lines pays,
        // and the line index is the medallion the machine lights.
        //
        // Retail's per-reel row offsets (relative to the payline row):
        //   0 = top   (+1 +1 +1)      3 = diagonal (-1  0 +1)
        //   1 = middle ( 0  0  0)     4 = diagonal (+1  0 -1)
        //   2 = bottom (-1 -1 -1)
        for (line, offsets) in legaia_asset::minigame_slot_scene::PAYLINE_ROW_OFFSETS
            .iter()
            .enumerate()
        {
            let r = rigged(6, *offsets);
            assert_eq!(r.symbol, Some(6), "line {line} pays");
            assert_eq!(r.payout, (6 + 1) * 2);
            assert_eq!(r.line, Some(line), "the winning line index is {line}");
        }
    }

    #[test]
    fn the_two_diagonals_are_real_lines_and_not_the_straights() {
        // The falsifiable half: a diagonal match must NOT be reported as a
        // straight line, and a straight must not be reported as a diagonal.
        let diag = rigged(6, [-1, 0, 1]);
        assert_eq!(diag.line, Some(3), "bottom-left to top-right is line 3");
        let straight = rigged(6, [0, 0, 0]);
        assert_eq!(straight.line, Some(1), "the middle row is line 1");
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

    /// The marquee is driven by the machine, not decoration beside it: a paying
    /// spin has to put that spin's own figure on the dot matrix.
    ///
    /// Symbol 6 pays `(6+1)*2 = 14` in the synthetic table, so the caption is a
    /// two-digit figure - which pins the leading-zero suppression too: the
    /// thousands and hundreds places must be absent, and the tens and units
    /// present at their own columns.
    #[test]
    fn a_paying_spin_puts_its_own_figure_on_the_marquee() {
        use legaia_asset::minigame_slot_scene as scene;

        let mut m = SlotMachine::new(payouts(), 42, 200);
        // Nothing is captioned before a spin resolves.
        assert!(
            m.marquee_placements().is_empty(),
            "an idle machine in the normal game captions nothing"
        );

        win_on(&mut m, 6);
        let payout = m.last_result().unwrap().payout;
        assert_eq!(payout, 14, "synthetic table pays (6+1)*2");
        // The payout state starts its caption timer at 0 and advances it on
        // its first frame (`0x801CF850..0x801CF868`, then `0x801CF898`).
        assert!(
            m.marquee_placements().is_empty(),
            "no caption on the eval frame"
        );
        m.tick();

        let p = m.marquee_placements();
        let at = |col: usize| {
            p.iter()
                .find(|q| q.col == col as i32)
                .map(|q| q.msg)
                .unwrap_or_else(|| panic!("nothing placed at dot column {col}: {p:?}"))
        };
        // "14 coin": tens then units then the word, at the retail columns.
        assert_eq!(at(scene::PAYOUT_DIGIT_COLS[2]), scene::MSG_NUMBER_BASE + 1);
        assert_eq!(at(scene::PAYOUT_DIGIT_COLS[3]), scene::MSG_NUMBER_BASE + 4);
        assert_eq!(at(scene::PAYOUT_COINS_COL), scene::MSG_COINS);
        // Leading zeros are suppressed, not drawn as "0".
        for place in [0usize, 1] {
            assert!(
                !p.iter()
                    .any(|q| q.col == scene::PAYOUT_DIGIT_COLS[place] as i32),
                "place {place} is above the figure and must not draw"
            );
        }

        // The caption comes down with the tally it captioned.
        m.collect();
        assert!(
            m.marquee_placements().is_empty(),
            "collecting the payout clears the caption"
        );
    }

    /// The machine's sound writes, in retail's order: the bet keys the reel
    /// motor, each stop stores the click into ring slot 0, the evaluation
    /// releases the motor, and the timed tally ticks once per transfer into
    /// slot 0 - `11` coins while more than `20` are owed, else `1`, on odd
    /// frames - before the state drops back to idle on its own.
    #[test]
    fn a_paying_spin_sounds_and_tallies_like_the_overlay() {
        let mut m = SlotMachine::new(payouts(), 42, 200);
        win_on(&mut m, 6);
        let s = m.take_sounds();
        assert_eq!(
            s.voice_on.first().map(|v| v.voice),
            Some(u32::from(SPIN_VOICE))
        );
        assert_eq!(s.voice_on[0].vab_program_tone, (2, 1, 0));
        assert_eq!(
            s.ring.iter().filter(|r| **r == (0, CUE_REEL_STOP)).count(),
            3,
            "one click per stop: {:?}",
            s.ring
        );
        assert_eq!(s.voice_off, vec![SPIN_VOICE]);
        let before = m.balance();
        let owed = m.payout_left();
        assert_eq!(owed, 14);
        let mut ticks = 0;
        for _ in 0..PAYOUT_HOLD_FRAMES + 40 {
            m.tick();
            ticks += m
                .take_sounds()
                .ring
                .iter()
                .filter(|r| **r == (0, CUE_PAYOUT_TICK))
                .count();
            if m.phase() == SlotPhase::Idle {
                break;
            }
        }
        assert_eq!(m.balance(), before + owed, "the tally pays the whole win");
        assert_eq!(ticks, 14, "14 coins under 20 owed: one-coin transfers");
        assert_eq!(m.phase(), SlotPhase::Idle, "the payout state ends itself");
        assert_eq!(m.winning_line_word(), -1, "and clears the lit line");
    }

    /// The caption slides in over its first frames, and the clock that moves it
    /// is the machine's own tick. A caption that never advances is the failure
    /// this pins: every row would stay clipped above the matrix.
    #[test]
    fn the_payout_caption_slides_down_one_row_per_tick() {
        use legaia_asset::minigame_slot_scene as scene;

        let mut m = SlotMachine::new(payouts(), 42, 200);
        win_on(&mut m, 6);
        m.tick();

        // Frame 1 of the caption: 12 rows above the matrix.
        let first = m.marquee_placements()[0].row;
        assert_eq!(first, 1 - scene::PAYOUT_SLIDE_ROWS);
        assert!(first < 0, "the caption starts above the matrix");

        // It descends exactly one row per tick...
        for expected in (first + 1)..=0 {
            m.tick();
            assert_eq!(
                m.marquee_placements()[0].row,
                expected,
                "the caption advances one row per tick"
            );
        }
        // ...and then holds at row 0 rather than running off the bottom.
        for _ in 0..5 {
            m.tick();
            assert_eq!(m.marquee_placements()[0].row, 0, "the caption holds");
        }
    }

    /// The bonus tally's marquee columns come off the same `claimed` array the
    /// payout multiplies, so the strip cannot show a different spin than it paid.
    #[test]
    fn the_bonus_tally_marquee_reads_the_claimed_reels() {
        use legaia_asset::minigame_slot_scene as scene;

        let mut m = SlotMachine::new(payouts(), 0x51075, 200);
        win_on(&mut m, slot_payout::KICK_SYMBOL_ID);
        m.collect();

        // Mid-bonus-spin, with the reels stopping: the tally strip is up.
        assert!(m.spin());
        while m.phase() != SlotPhase::Stopping {
            m.tick();
        }
        m.stop_reel(0);
        let p = m.marquee_placements();
        // Three numerals and two multiplication signs, at the retail columns.
        for &col in scene::TALLY_TIMES_COLS.iter() {
            assert!(
                p.iter()
                    .any(|q| q.col == col as i32 && q.msg == scene::MSG_TIMES),
                "a multiplication sign belongs at column {col}"
            );
        }
        // Reel 0 is claimed, so its column shows that reel's landed number;
        // the unclaimed reels read "0".
        let claimed = m.claimed(0);
        let want = scene::MSG_NUMBER_BASE + (claimed - 0x10).max(0) as usize;
        let got = p
            .iter()
            .find(|q| q.col == scene::TALLY_NUMBER_COLS[0] as i32)
            .unwrap()
            .msg;
        assert_eq!(got, want, "the tally's first column is reel 0's claim");
        for reel in 1..REEL_COUNT {
            let got = p
                .iter()
                .find(|q| q.col == scene::TALLY_NUMBER_COLS[reel] as i32)
                .unwrap()
                .msg;
            assert_eq!(
                got,
                scene::MSG_NUMBER_BASE,
                "reel {reel} is unclaimed and reads 0"
            );
        }
    }

    fn ring(m: &mut SlotMachine) -> Vec<i16> {
        m.take_sounds().ring.into_iter().map(|(_, id)| id).collect()
    }

    /// Triangle opens the picker (`0x20`); Up on row 0 stays on row 0 (the
    /// unsigned `% 3`), Down walks and wraps; Circle backs out (`0x37`).
    #[test]
    fn the_cash_out_picker_moves_and_cancels_like_retail() {
        let mut m = SlotMachine::new(payouts(), 1, 50);
        assert!(!m.cash_out_input(0), "no edge: the machine keeps the pad");
        assert!(m.cash_out_input(menu_pad::OPEN & 0x10));
        assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
        assert_eq!(ring(&mut m), vec![CUE_MENU_CONFIRM]);
        m.cash_out_input(menu_pad::UP);
        assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
        assert_eq!(ring(&mut m), vec![CUE_MENU_CURSOR]);
        m.cash_out_input(menu_pad::DOWN);
        m.cash_out_input(menu_pad::DOWN);
        assert_eq!(m.screen(), SlotScreen::Picker { row: 2 });
        m.cash_out_input(menu_pad::DOWN);
        assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
        ring(&mut m);
        m.cash_out_input(0x20);
        assert_eq!(m.phase(), SlotPhase::Idle);
        assert_eq!(ring(&mut m), vec![CUE_MENU_CANCEL]);
        assert_eq!(m.balance(), 50, "the picker charges nothing");
    }

    /// The quit row fades out over sixteen frames and only then commits.
    #[test]
    fn the_quit_row_fades_then_commits() {
        let mut m = SlotMachine::new(payouts(), 1, 50);
        m.cash_out_input(0x100);
        m.cash_out_input(menu_pad::DOWN);
        m.cash_out_input(0x40);
        assert_eq!(m.phase(), SlotPhase::Leaving);
        let mut frames = 0;
        while m.phase() == SlotPhase::Leaving {
            m.cash_out_input(0);
            frames += 1;
        }
        assert_eq!(frames, 16);
        assert_eq!(m.phase(), SlotPhase::CashedOut);
        assert_eq!(m.fade_level(), 0xFF);
    }

    /// The rules row: fade to black, page 0 (taken once faded in), page 1,
    /// fade out, fade back in on the machine, idle.
    #[test]
    fn the_rules_row_walks_both_pages_and_returns() {
        let mut m = SlotMachine::new(payouts(), 1, 50);
        m.cash_out_input(0x10);
        m.cash_out_input(menu_pad::DOWN);
        m.cash_out_input(menu_pad::DOWN);
        m.cash_out_input(0x04);
        assert_eq!(m.retail_state(), 0x33);
        for _ in 0..40 {
            m.cash_out_input(0);
        }
        assert_eq!(m.screen(), SlotScreen::Instructions { page: 0 });
        ring(&mut m);
        m.cash_out_input(0x40);
        assert_eq!(m.screen(), SlotScreen::Instructions { page: 1 });
        assert_eq!(ring(&mut m), vec![CUE_MENU_CURSOR]);
        m.cash_out_input(0x40);
        assert_eq!(ring(&mut m), vec![CUE_MENU_CANCEL]);
        let mut frames = 0;
        while m.phase() != SlotPhase::Idle {
            m.cash_out_input(0);
            frames += 1;
            assert!(frames < 64);
        }
        assert_eq!(m.fade_level(), 0);
    }

    /// Under three coins the idle state raises the prompt; any face button
    /// leaves through the fade.
    #[test]
    fn an_empty_machine_raises_the_prompt_and_leaves() {
        let mut m = SlotMachine::new(payouts(), 1, 2);
        assert!(m.cash_out_input(0));
        assert_eq!(m.screen(), SlotScreen::NoCoins);
        m.cash_out_input(0x80);
        assert_eq!(ring(&mut m), vec![CUE_MENU_CONFIRM]);
        assert_eq!(m.phase(), SlotPhase::Leaving);
        // The submenu edge still wins over the gate on an empty machine.
        let mut m = SlotMachine::new(payouts(), 1, 0);
        m.cash_out_input(0x10);
        assert_eq!(m.screen(), SlotScreen::Picker { row: 0 });
    }

    #[test]
    fn packed_edges_swap_the_raw_layout() {
        use legaia_engine_vm::pad::PadButton;
        let raw = PadButton::Triangle.mask() | PadButton::Up.mask();
        assert_eq!(packed_edges(raw, 0), 0x1010);
        assert_eq!(packed_edges(raw, raw), 0);
    }
}
