//! Casino slot-machine **per-symbol payout table** (overlay VA `0x801D3598`).
//!
//! The win-evaluator [`FUN_801d13e8`] credits a winning line as
//! `DAT_801d3d38 = (byte)(&DAT_801d3598)[DAT_801d3d34]`, where `DAT_801d3d34` is
//! the winning symbol id (`0..=9`; the reel display strip stores `slot/2`, so
//! symbol ids run `0..9` with two strip positions each - see
//! `docs/subsystems/minigame-slot-machine.md`). The HUD payout preview
//! `FUN_801d2aa4` reads the same table. So the table is a flat array of
//! **per-symbol line payouts**, one byte per symbol id.
//!
//! ## Extent - 10 symbols
//!
//! The winning id is special-cased only for `8` and `9` (the bonus/jackpot
//! symbols) and the strip carries ids `0..=9`, so the table is
//! [`SLOT_SYMBOL_COUNT`] = 10 bytes. Past entry 9 the region is zero padding,
//! then an unrelated overlay string - the table never indexes beyond 9.
//!
//! During an active bonus round the payout is instead a *product* of the three
//! matched symbols' `(value - 0xf)` factors (the bonus reel strip carries values
//! `0x10..=0x19`), computed inline in `FUN_801d13e8` - it does **not** read this
//! table. This module covers the normal-line payout only.
//!
//! ## Provenance - baked overlay data, pinned on disc
//!
//! The table is static `.rodata` in the slot-machine overlay (PROT entry
//! **0975**, base [`SLOT_OVERLAY_BASE_VA`]) at file offset
//! [`SLOT_PAYOUT_FILE_OFFSET`]; reproducible from the user's `PROT.DAT`
//! (disc-gated `slot_payout_real`). No Sony bytes are committed - the payout
//! values decode from the user's disc.

/// CDNAME / PROT index of the slot-machine overlay (`data\OTHER4`).
pub const SLOT_OVERLAY_PROT_INDEX: usize = 975;

/// Load base of the slot-machine overlay (the shared slot-A minigame base).
pub const SLOT_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Runtime VA of the payout-byte table (`DAT_801d3598`).
pub const SLOT_PAYOUT_TABLE_VA: u32 = 0x801D_3598;

/// File offset of the payout table within the as-loaded overlay image.
pub const SLOT_PAYOUT_FILE_OFFSET: usize = (SLOT_PAYOUT_TABLE_VA - SLOT_OVERLAY_BASE_VA) as usize;

/// Number of reel symbols (`slot/2` over the 20-slot strip → ids `0..=9`).
pub const SLOT_SYMBOL_COUNT: usize = 10;

/// The two bonus / jackpot symbol ids (`FUN_801d13e8` special-cases these to
/// kick off the bonus round: id 9 → 3 free spins, id 8 → 1).
pub const BONUS_SYMBOL_IDS: [u8; 2] = [8, 9];

// --- Bonus game (the two jackpot symbols, named by their reel artwork) ---
//
// `FUN_801d13e8` special-cases exactly two reel symbols to open the bonus
// round, and their free-spin counts are pinned in the disassembly: a matching
// line of id **8** grants **1** bonus round, id **9** grants **3**. Decoding
// the PROT 1200 reel art off the disc pins the artwork: symbol 8 is the **blue
// "kick"** cell, symbol 9 the **red "punch"** cell (per-symbol CLUT
// `0x7A80 + sym`; the average opaque hue of the two cells is blue-dominant for
// 8 and red-dominant for 9). So the player-facing rule - *3 blue kicks earn 1
// bonus round, 3 red punches earn 3* - is exactly what the disc encodes.

/// Reel symbol id of the **blue "kick"** cell - one matching line earns
/// [`KICK_BONUS_ROUNDS`] bonus round (`FUN_801d13e8` id 8 → 1 free spin).
pub const KICK_SYMBOL_ID: u8 = 8;
/// Bonus rounds a line of [`KICK_SYMBOL_ID`] earns.
pub const KICK_BONUS_ROUNDS: u32 = 1;
/// Reel symbol id of the **red "punch"** cell - one matching line earns
/// [`PUNCH_BONUS_ROUNDS`] bonus rounds (`FUN_801d13e8` id 9 → 3 free spins).
pub const PUNCH_SYMBOL_ID: u8 = 9;
/// Bonus rounds a line of [`PUNCH_SYMBOL_ID`] earns.
pub const PUNCH_BONUS_ROUNDS: u32 = 3;

/// Lowest number a bonus reel can stop on (the bonus strip shows `1..=10`).
pub const BONUS_NUMBER_MIN: u32 = 1;
/// Highest number a bonus reel can stop on.
pub const BONUS_NUMBER_MAX: u32 = 10;
/// Minimum coins a bonus round can pay (`1 × 1 × 1`).
pub const BONUS_PAYOUT_MIN: u32 = BONUS_NUMBER_MIN.pow(3);
/// Maximum coins a bonus round can pay (`10 × 10 × 10`).
pub const BONUS_PAYOUT_MAX: u32 = BONUS_NUMBER_MAX.pow(3);

// --- The bonus strip's own value space ---
//
// A bonus round does not re-label the symbol strip: it swaps the reels onto a
// **second strip array** the init builds beside the symbol one (`FUN_801cf0d8`
// case 0 fills `DAT_801d3fd0` with `slot/2 + 0x10`), so a bonus reel row carries
// a value in `0x10..=0x19` - not a symbol id in `0..=9`. Three consumers read
// that value, and the `>= 0x10` test is what each one switches on:
//
// * the **reel renderer** (`FUN_801d0fa8`) takes `value >= 0x10` as the signal to
//   switch texpage (`0x0C` -> `0x0D`) and CLUT base (`0x7A80` -> `0x7AC0`), so the
//   numerals come off their own art page - see [`crate::minigame_art`];
// * the **payout** (`FUN_801d13e8`, bonus branch) multiplies the three payline
//   `value - 0xf` factors - [`bonus_number_for_value`], i.e. `1..=10`;
// * the **marquee tally** (`FUN_801d0554` latches `value + 1` into
//   `DAT_801d3d20`; `FUN_801cfff0` prints message `(claimed - 0x10) + 6`) - the
//   same `value - 0xf` number, which is why the strip and the payout can never
//   disagree.

/// First value of the bonus reel strip (`DAT_801d3fd0` = `slot/2 + 0x10`).
pub const BONUS_VALUE_BASE: u8 = 0x10;
/// The bias the payout evaluator subtracts from a bonus strip value to get its
/// multiplier factor (`FUN_801d13e8`: `(value - 0xf) * ...`).
pub const BONUS_VALUE_BIAS: u8 = 0x0F;

/// The bonus number `1..=10` a bonus **strip value** (`0x10..=0x19`) shows -
/// `value - 0xf`, the exact factor `FUN_801d13e8` multiplies.
///
/// This one byte is the reel's on-screen numeral, the tally column's digit and
/// the payout factor at once: all three read it through the same bias.
pub fn bonus_number_for_value(value: u8) -> u32 {
    (value.saturating_sub(BONUS_VALUE_BIAS) as u32).clamp(BONUS_NUMBER_MIN, BONUS_NUMBER_MAX)
}

/// The bonus strip value that carries `number` (`1..=10` -> `0x10..=0x19`).
pub fn bonus_value_for_number(number: u32) -> u8 {
    (number.clamp(BONUS_NUMBER_MIN, BONUS_NUMBER_MAX) as u8) + BONUS_VALUE_BIAS
}

/// The number of bonus rounds a winning line of `symbol` earns, or `None` when
/// the symbol is not a jackpot symbol (`FUN_801d13e8`: id 8 → 1, id 9 → 3).
pub fn bonus_rounds_for(symbol: u8) -> Option<u32> {
    match symbol {
        KICK_SYMBOL_ID => Some(KICK_BONUS_ROUNDS),
        PUNCH_SYMBOL_ID => Some(PUNCH_BONUS_ROUNDS),
        _ => None,
    }
}

/// The bonus number a **symbol id** (`0..=9`) maps to - `symbol + 1`, the same
/// numeral the strip value `symbol + 0x10` carries. The two strips are built
/// slot-for-slot from the same `slot / 2` id, so a symbol id and its bonus value
/// name the same numeral; [`bonus_number_for_value`] is the one to use on a live
/// reel row, since that row holds the *value*.
pub fn bonus_number_for_symbol(symbol: u8) -> u32 {
    (symbol as u32 + 1).clamp(BONUS_NUMBER_MIN, BONUS_NUMBER_MAX)
}

/// A bonus round's coin payout: the **product of the three stopped numbers**,
/// each clamped to `1..=10`, so the result is always `1..=1000`
/// (`FUN_801d13e8`: during a bonus round the credit is the product of the three
/// payline factors, no equality gate).
pub fn bonus_round_payout(numbers: [u32; 3]) -> u32 {
    numbers
        .iter()
        .map(|n| (*n).clamp(BONUS_NUMBER_MIN, BONUS_NUMBER_MAX))
        .product()
}

/// The decoded payout table: one line-payout byte per symbol id `0..=9`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPayoutTable {
    /// `payouts[symbol_id]` = coins credited for a matching line of that symbol.
    pub payouts: [u8; SLOT_SYMBOL_COUNT],
}

impl SlotPayoutTable {
    /// The line payout for a symbol id, or `None` if out of range.
    pub fn payout(&self, symbol_id: u8) -> Option<u8> {
        self.payouts.get(symbol_id as usize).copied()
    }

    /// Whether a symbol id is a bonus / jackpot symbol.
    pub fn is_bonus_symbol(&self, symbol_id: u8) -> bool {
        BONUS_SYMBOL_IDS.contains(&symbol_id)
    }
}

/// Parse the slot payout table out of the as-loaded slot-machine overlay image
/// (PROT entry [`SLOT_OVERLAY_PROT_INDEX`]). Returns `None` if the buffer is too
/// short.
pub fn parse(overlay: &[u8]) -> Option<SlotPayoutTable> {
    parse_at(overlay, SLOT_PAYOUT_FILE_OFFSET)
}

/// Parse [`SLOT_SYMBOL_COUNT`] payout bytes starting at file offset `off`.
pub fn parse_at(overlay: &[u8], off: usize) -> Option<SlotPayoutTable> {
    let bytes = overlay.get(off..off + SLOT_SYMBOL_COUNT)?;
    let mut payouts = [0u8; SLOT_SYMBOL_COUNT];
    payouts.copy_from_slice(bytes);
    Some(SlotPayoutTable { payouts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_and_count() {
        assert_eq!(SLOT_PAYOUT_FILE_OFFSET, 0x4D80);
        assert_eq!(SLOT_SYMBOL_COUNT, 10);
    }

    #[test]
    fn parse_and_lookup() {
        let off = 0x10;
        let mut buf = vec![0u8; off + SLOT_SYMBOL_COUNT];
        // synthetic payouts 1..=10 for symbols 0..=9.
        for (i, b) in buf[off..off + SLOT_SYMBOL_COUNT].iter_mut().enumerate() {
            *b = (i + 1) as u8;
        }
        let t = parse_at(&buf, off).expect("parses");
        assert_eq!(t.payout(0), Some(1));
        assert_eq!(t.payout(9), Some(10));
        assert_eq!(t.payout(10), None);
        assert!(t.is_bonus_symbol(8));
        assert!(t.is_bonus_symbol(9));
        assert!(!t.is_bonus_symbol(0));
    }

    #[test]
    fn too_short_is_none() {
        assert!(parse_at(&[0u8; 4], 0).is_none());
    }

    #[test]
    fn three_blue_kicks_earn_one_round_three_red_punches_earn_three() {
        // The disc pins it (`FUN_801d13e8`): a line of symbol 8 (blue kick)
        // opens 1 bonus round, a line of symbol 9 (red punch) opens 3.
        assert_eq!(bonus_rounds_for(KICK_SYMBOL_ID), Some(1));
        assert_eq!(bonus_rounds_for(PUNCH_SYMBOL_ID), Some(3));
        assert_eq!(KICK_SYMBOL_ID, 8);
        assert_eq!(PUNCH_SYMBOL_ID, 9);
        // Every jackpot symbol id agrees with the round-count map.
        for &s in &BONUS_SYMBOL_IDS {
            assert!(bonus_rounds_for(s).is_some());
        }
        // Any non-jackpot symbol earns no bonus round.
        for s in 0..8u8 {
            assert_eq!(bonus_rounds_for(s), None, "symbol {s} is not a jackpot");
        }
    }

    #[test]
    fn bonus_numbers_are_symbol_plus_one() {
        // The bonus reels show 1..=10; number = reel symbol id + 1.
        assert_eq!(bonus_number_for_symbol(0), 1);
        assert_eq!(bonus_number_for_symbol(9), 10);
        for s in 0..10u8 {
            let n = bonus_number_for_symbol(s);
            assert!((BONUS_NUMBER_MIN..=BONUS_NUMBER_MAX).contains(&n));
        }
    }

    #[test]
    fn bonus_payout_is_the_product_bounded_one_to_a_thousand() {
        // Payout for a bonus round = product of the three stopped numbers.
        assert_eq!(bonus_round_payout([1, 1, 1]), BONUS_PAYOUT_MIN);
        assert_eq!(bonus_round_payout([1, 1, 1]), 1);
        assert_eq!(bonus_round_payout([10, 10, 10]), BONUS_PAYOUT_MAX);
        assert_eq!(bonus_round_payout([10, 10, 10]), 1000);
        assert_eq!(bonus_round_payout([2, 5, 7]), 70);
        assert_eq!(bonus_round_payout([3, 4, 6]), 72);
        // Every number combination stays inside 1..=1000, and out-of-range
        // inputs are clamped into the reel's own 1..=10 range first.
        for a in 1..=10 {
            for b in 1..=10 {
                for c in 1..=10 {
                    let p = bonus_round_payout([a, b, c]);
                    assert!((BONUS_PAYOUT_MIN..=BONUS_PAYOUT_MAX).contains(&p));
                }
            }
        }
        assert_eq!(bonus_round_payout([0, 0, 0]), 1, "clamped up to 1");
        assert_eq!(bonus_round_payout([99, 99, 99]), 1000, "clamped down to 10");
    }
}
