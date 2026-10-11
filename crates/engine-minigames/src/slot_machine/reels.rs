//! The slot LCG, the reel-strip builder, the per-spin feature roll and the stop / landing plan.

use super::*;
use legaia_engine_system::rand::BiosRand;
use legaia_minigame_formats::minigame_slot_scene::PAYLINE_CENTRE_ROW_BIAS;
use legaia_minigame_formats::slot_payout;

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
pub(super) fn feature_denominators(net_take: i32) -> Option<(u32, u32)> {
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
/// row ([`legaia_minigame_formats::minigame_slot_scene::LANDING_LINE_BY_JITTER`]). With no
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
