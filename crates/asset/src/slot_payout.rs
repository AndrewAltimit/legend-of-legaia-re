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
}
