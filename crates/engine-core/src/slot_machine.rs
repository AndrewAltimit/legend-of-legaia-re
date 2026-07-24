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

use crate::levelup::BiosRand;
use legaia_asset::minigame_slot_scene::{MarqueeFrame, MarqueePlacement, compose_marquee_frame};
use legaia_asset::slot_payout::{self, SlotPayoutTable};

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

/// The reel landing search (`FUN_801d2440`): starting from `from_row`, walk
/// up to `depth` rows forward looking for `target` on the display strip; if
/// found, return that row (the symbol lands on the payline), otherwise
/// return the next natural row - no forced result. A `None` target (or a
/// `depth` of 0, its retail companion) never matches, so the reel takes the
/// next row: the bonus round's free stop.
// PORT: FUN_801d2440 (landing search: find target within depth, else next row)
pub fn land_row(
    strip: &[u8; STRIP_LEN],
    from_row: usize,
    depth: usize,
    target: Option<u8>,
) -> usize {
    // Retail guards the search with `0 < depth`, so a zero depth searches
    // nothing at all - which is how the bonus round's free stop is expressed.
    if let (Some(target), true) = (target, depth > 0) {
        for d in 0..=depth.min(STRIP_LEN) {
            let row = (from_row + d) % STRIP_LEN;
            if strip[row] == target {
                return row;
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
    /// Per-spin landing jitter (`DAT_801d4134`). Carried for fidelity of the
    /// roll stream; the engine's landing keeps rows exact (the retail `*0x10`
    /// nudge is sub-row presentation).
    jitter: i32,
    /// Overlay-local playing balance (`DAT_801d4114`).
    balance: i32,
    /// Richer-odds flag (`DAT_801d3790`).
    richer_odds: bool,
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
            richer_odds: false,
            bonus_just_ended: false,
            last_result: None,
            caption_payout: 0,
            caption_frame: 0,
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
        self.phase = SlotPhase::Spinning;
        true
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
        if self.caption_frame != 0 {
            self.caption_frame += 1;
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
        // The display-strip refill, after the advance - as in retail's tail.
        let source = *self.active_source();
        let rows: [usize; REEL_COUNT] =
            core::array::from_fn(|r| (self.payline_row(r) + DISPLAY_REFRESH_LEAD) % STRIP_LEN);
        for ((display, src), &row) in self.strips.iter_mut().zip(source.iter()).zip(rows.iter()) {
            display[row] = src[row];
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
        // `DAT_801d3d20[reel] = display[reel][payline_row] + 1`, the frame the
        // reel locks. The +1 is retail's, and it is what makes the tally's
        // "unclaimed" state (`0`) distinguishable from a landed value of `0`.
        self.claimed[reel] = self.strips[reel][row] as i32 + 1;
        if self.reels_stopped() == REEL_COUNT {
            let result = self.evaluate_spin();
            self.last_result = Some(result);
            self.phase = SlotPhase::Payout;
            // Raise the marquee's payout caption on a paying spin. Retail gates
            // the caption on the figure being non-zero, so a losing spin leaves
            // the matrix to the tally / attract strip.
            self.caption_payout = result.payout;
            self.caption_frame = i32::from(result.payout != 0);
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
        let credit = self.last_result.map(|r| r.payout).unwrap_or(0);
        self.balance = (self.balance + credit).min(BALANCE_CAP);
        self.phase = SlotPhase::Idle;
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
/// ten each cell), so `digits[0]` is the units place.
/// Wired through [`coin_exchange_quote`], which the play-window casino entry
/// point calls to buy coins before seating the player at a machine.
// PORT: FUN_801e6f70 entry-field half (digit accumulation). The gate half of
// the same function is `coin_exchange_quote`; the two together cover it.
pub fn coin_entry_value(digits: &[u8]) -> i32 {
    let mut place = 1i32;
    let mut total = 0i32;
    for &d in digits.iter().take(COIN_ENTRY_DIGITS) {
        total += place * i32::from(d);
        place *= 10;
    }
    total
}

/// Quote the coin-exchange counter for the entered `digits`, against the
/// party's `gold` and the counter's remaining coin `stock`.
///
/// Coins cost a flat [`COIN_PRICE_GOLD`] each. Retail gates the sale twice -
/// on the party's gold (`_DAT_8008459C`) against the total, and on the
/// counter's stock (`_DAT_8007BB90`) against the coin count - and recolours
/// the total to the alert ink when *either* fails. The bank word this feeds
/// (`_DAT_800845A4`) is the same one [`SlotMachine::cash_out`] assigns back,
/// so buying coins here and cashing out of a machine write the same global.
///
/// This function is the quote/validation half only; retail commits the sale on
/// the counter's confirm path, not in the screen routine.
/// Wired: the play-window casino entry point runs a coin purchase through
/// this quote before seating the player at a machine, committing the gold
/// debit and the coin credit only when both gates pass. What is still
/// unreached is the *screen* around it - retail's per-frame quote refresh
/// with the digit-entry cursor and the alert-ink recolour on a failed gate.
// PORT: FUN_801e6f70 (coin-exchange counter: total cost + gold/stock gates)
pub fn coin_exchange_quote(digits: &[u8], gold: i32, stock: i32) -> CoinQuote {
    let coins = coin_entry_value(digits);
    let cost = coins * COIN_PRICE_GOLD;
    CoinQuote {
        coins,
        cost,
        affordable: gold >= cost,
        in_stock: stock >= coins,
    }
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
// NOT WIRED: the remaining blocker is the sink, not the source. The source
// half is now covered - `legaia_asset::minigame_slot_scene::parse_paylines`
// takes the five segments straight off the raw overlay, with no decoded
// page-3 art plane. What is still missing is a consumer: paylines are
// GTE-projected 3D line prims, and the native window draws the machine as a
// text HUD with no projection or ordering-table pass to link them into, while
// the browser play page draws its cabinet from JS geometry of its own. Wiring
// it needs a 3D slot-cabinet render pass on either host.
// PORT: FUN_801d3380 (payline 3D line segments)
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
        strip[5] = 7;
        // Target within depth from row 2 -> lands on row 5.
        assert_eq!(land_row(&strip, 2, 4, Some(7)), 5);
        // Depth too shallow -> next natural row.
        assert_eq!(land_row(&strip, 2, 2, Some(7)), 3);
        // Wraps around the strip end.
        strip[1] = 9;
        assert_eq!(land_row(&strip, 18, 4, Some(9)), 1);
        // The bonus round's plan (depth 0 / no target) never searches: the reel
        // lands on the next row, wherever that is.
        assert_eq!(land_row(&strip, 5, 0, None), 6);
        assert_eq!(land_row(&strip, 19, 0, None), 0);
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

    /// The caption slides in over its first frames, and the clock that moves it
    /// is the machine's own tick. A caption that never advances is the failure
    /// this pins: every row would stay clipped above the matrix.
    #[test]
    fn the_payout_caption_slides_down_one_row_per_tick() {
        use legaia_asset::minigame_slot_scene as scene;

        let mut m = SlotMachine::new(payouts(), 42, 200);
        win_on(&mut m, 6);

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
}
