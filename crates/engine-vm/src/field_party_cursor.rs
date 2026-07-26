//! The field VM's party-member cursor: entering the submode.
//!
//! PORT: FUN_801F1278
//!
//! The "pick a party member" overlay the field VM raises for op `0x49` - the
//! same submode the tile-board and Seru-trade flows reuse. This is the **enter**
//! half; the per-frame resume / close half is `FUN_801F159C`. Both are described
//! in [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md).
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_baka_fighter_801f1278.txt` (201 instructions).
//!
//! ## Which dump is authoritative
//!
//! The corpus holds this VA in six programs. Five of them - the `baka_fighter`,
//! `dance`, `debug_menu`, `fishing` and `slot_machine` images - carry the
//! **byte-identical** 201-instruction body (same instruction stream modulo the
//! printed addresses), because all five are RAM-derived captures in which this
//! address belongs to resident library code rather than to the minigame overlay
//! that names the file. The sixth, `overlay_overlay_0897_801f1278.txt`, reports
//! `0 instructions` and carries only decompiled C - one of the artifacts
//! [`docs/tooling/ghidra.md`](../../docs/tooling/ghidra.md) catalogues, and not
//! usable as evidence on its own. The port therefore reads a
//! disassembly-bearing dump and the C only as a cross-check; the two agree.
//!
//! ## Roster centring
//!
//! The one behaviour worth calling out, because the C's `if`-chain hides it:
//! the three portrait cells are seeded `0, 1, 2` and then **overwritten by
//! roster size**, and a one-member party goes into the *middle* cell while a
//! two-member party takes the *outer* two. So the picker is centre-weighted, not
//! left-packed. [`seed_member_cells`] is that table.
//!
//! # NOT WIRED
//!
//! No engine caller. The submode's own driver is the field VM's op-`0x49`
//! `MENU_CTRL` arm, and the engine's field VM routes that opcode to the
//! tile-board installer instead (`FieldHost::op4c_*` and the board path in
//! `engine-core`), with no party-picker submode behind it. Wiring means a
//! picker submode on the engine's field side - a `engine-core` menu runtime
//! change plus the portrait cells reaching `engine-ui`, both other crates.

/// Field-context flag bit the enter path raises (`ctx[+0x10] |= 0x80000`) -
/// the "a modal submode owns input" marker the close path clears.
pub const CTX_SUBMODE_BUSY: u32 = 0x0008_0000;
/// Second field-context flag bit raised on the way in (`|= 0x1000000`).
pub const CTX_PICKER_ACTIVE: u32 = 0x0100_0000;
/// Pad-latch bit cleared and then re-set around the roster seed
/// (`_DAT_1F800394 & ~0x8000`, then `| 0x8000`).
pub const PAD_LATCH_BIT: u32 = 0x0000_8000;
/// Submode kind word the enter path writes (`_DAT_8007BDD8 = 2`).
pub const SUBMODE_KIND: u32 = 2;
/// Handler id installed into the caller's `+0x50` slot.
pub const PICKER_HANDLER: u16 = 7;
/// Cursor home position (`cursor[+0x46]`, `cursor[+0x48]`).
pub const CURSOR_HOME: (u16, u16) = (0xA0, 0x58);
/// Submode states the enter path rewinds to `1` when re-armed.
pub const REARM_STATES: [u32; 2] = [4, 7];
/// Sentinel in the pending-pick remap table meaning "no remap".
pub const REMAP_NONE: i8 = -1;

/// The field-context writes the enter path makes, in one value so a caller can
/// apply them without re-deriving the bit names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterContext {
    /// `ctx[+0x10]` after both flag bits are OR'd in.
    pub flags: u32,
    /// `_DAT_1F800394` after the clear-then-set round trip.
    pub pad_latch: u32,
    /// `ctx[+0x5C]` - a scroll or timing value, `_DAT_8007B8F8 * 7` plus the
    /// submode kind word the same code just set to `2`.
    pub scroll: i16,
}

/// Apply the enter path's context writes.
///
/// The `scroll` term is the one place the disassembly and a casual reading of
/// the C part ways: the `+ 2` addend is not a literal, it is a **re-read of
/// `_DAT_8007BDD8`** three instructions after that word was stored as `2`
/// (`sw a2, -0x4228(a1)` then `lhu v0, -0x4228(a1)`). The value is `2`, but the
/// dependency is on the submode-kind word, not on a constant.
pub fn enter_context(flags_before: u32, pad_before: u32, scroll_base: u16) -> EnterContext {
    EnterContext {
        flags: flags_before | CTX_SUBMODE_BUSY | CTX_PICKER_ACTIVE,
        // Cleared first, then re-set - the net effect on this bit is "set", and
        // the clear matters only to code that runs in between (the roster seed
        // call `FUN_801D9E1C`).
        pad_latch: (pad_before & !PAD_LATCH_BIT) | PAD_LATCH_BIT,
        scroll: (scroll_base.wrapping_mul(7)).wrapping_add(SUBMODE_KIND as u16) as i16,
    }
}

/// The `0x8E`/`0x8F` byte pair saved, forced to `0xFF`, and restored around the
/// roster-seed call `FUN_801D9E1C`.
///
/// Two independent bytes, each stashed in its own register and put back after
/// the call - so whatever `FUN_801D9E1C` does with them, the enter path is
/// transparent to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskedBytes {
    /// The saved originals.
    pub saved: (u8, u8),
}

impl MaskedBytes {
    /// Save the pair and report the masked values to write before the call.
    pub const fn mask(before: (u8, u8)) -> (Self, (u8, u8)) {
        (Self { saved: before }, (0xFF, 0xFF))
    }

    /// The values to write back after the call.
    pub const fn restore(self) -> (u8, u8) {
        self.saved
    }
}

/// Resolve the "current member" byte `DAT_8007B469` against the live roster.
///
/// Retail scans the roster ids at `DAT_80084598..` for a match; if the scan runs
/// off the end (including the `count == 0` case, where the loop is skipped and
/// the index is already zero) the current member is reset to the roster's first
/// entry. Returns the byte to store.
pub fn resolve_current_member(current: u8, roster: &[u8]) -> u8 {
    if roster.contains(&current) {
        current
    } else {
        roster.first().copied().unwrap_or(current)
    }
}

/// The three portrait cells at `cursor[+0x36]`, `+0x38`, `+0x3A`.
///
/// Seeded `0, 1, 2` by a countdown loop and then overwritten per roster size:
///
/// | members | `+0x36` | `+0x38` | `+0x3A` |
/// |---|---|---|---|
/// | 0 | `0` | `1` | `2` |
/// | 1 | `0` | roster[0] | `2` |
/// | 2 | roster[0] | `1` | roster[1] |
/// | 3 | roster[0] | roster[1] | roster[2] |
/// | 4+ | `0` | `1` | `2` |
///
/// The seeds survive wherever the size arm does not write, which is what leaves
/// a one-member party centred and a two-member party split to the outsides.
pub fn seed_member_cells(roster: &[u8]) -> [u16; 3] {
    let mut cells: [u16; 3] = [0, 1, 2];
    match roster.len() {
        1 => cells[1] = roster[0] as u16,
        2 => {
            cells[0] = roster[0] as u16;
            cells[2] = roster[1] as u16;
        }
        3 => {
            cells[0] = roster[0] as u16;
            cells[1] = roster[1] as u16;
            cells[2] = roster[2] as u16;
        }
        _ => {}
    }
    cells
}

/// What the enter path writes into the cursor context and the calling actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickerEntry {
    /// `cursor[+0x3E] = 1` - "a pick is in progress".
    pub picking: u16,
    /// `cursor[+0x2E] = -1` - the selection sentinel.
    pub selection: i16,
    /// `cursor[+0x40]` - the caller's previous `+0x50` handler, saved.
    pub saved_handler: u16,
    /// `actor[+0x50]` - the handler installed for the picker.
    pub handler: u16,
    /// `actor[+0x54] = 0` - the handler's sub-state.
    pub sub_state: u16,
    /// `actor[+0x1A] = 1` - the actor's yield marker.
    pub yield_marker: u16,
    /// `cursor[+0x46]`, `cursor[+0x48]` - cursor home.
    pub cursor: (u16, u16),
    /// The three portrait cells.
    pub cells: [u16; 3],
}

/// Build the enter-path writes (`0x801F1368` onward), including the optional
/// pending-pick remap.
///
/// `pending` is `_DAT_8007B450`: `0` means no pending pick, `1` is consumed and
/// cleared on the way in, and any other value is a pointer whose first byte
/// indexes the remap table at `DAT_801F33A4`. When that lookup yields anything
/// but [`REMAP_NONE`], the installed handler becomes the remapped value instead
/// of [`PICKER_HANDLER`] - and the saved handler is re-saved from the
/// already-overwritten `+0x50`, so a remap saves `7`, not the caller's original.
pub fn picker_entry(caller_handler: u16, roster: &[u8], remap: Option<i8>) -> PickerEntry {
    let mut entry = PickerEntry {
        picking: 1,
        selection: -1,
        saved_handler: caller_handler,
        handler: PICKER_HANDLER,
        sub_state: 0,
        yield_marker: 1,
        cursor: CURSOR_HOME,
        cells: seed_member_cells(roster),
    };
    if let Some(target) = remap.filter(|&t| t != REMAP_NONE) {
        // Retail re-runs the same three stores with `+0x50` already at 7.
        entry.saved_handler = PICKER_HANDLER;
        entry.handler = target as i16 as u16;
        entry.sub_state = 0;
    }
    entry
}

/// Should the submode state be rewound to `1` on this entry?
///
/// Retail rewinds `DAT_801F2734` from `4` or `7` only - any other state is left
/// alone, which is what lets a re-arm resume mid-flow.
pub fn rearm_state(state: u32) -> u32 {
    if REARM_STATES.contains(&state) {
        1
    } else {
        state
    }
}

/// Is the pending-pick word the "consume and clear" sentinel?
pub const fn pending_is_consumed(pending: u32) -> bool {
    pending == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_sets_both_context_bits_and_leaves_the_latch_set() {
        let c = enter_context(0x0000_0001, 0, 0);
        assert_eq!(c.flags, 0x0000_0001 | CTX_SUBMODE_BUSY | CTX_PICKER_ACTIVE);
        assert_eq!(c.pad_latch, PAD_LATCH_BIT);
        // An already-set latch stays set.
        let c = enter_context(0, PAD_LATCH_BIT | 0x0F, 0);
        assert_eq!(c.pad_latch, PAD_LATCH_BIT | 0x0F);
    }

    #[test]
    fn scroll_is_seven_times_the_base_plus_the_submode_kind() {
        assert_eq!(enter_context(0, 0, 0).scroll, 2);
        assert_eq!(enter_context(0, 0, 10).scroll, 72);
        // Built as `(x << 3) - x`, so it wraps at the halfword like retail.
        let big = enter_context(0, 0, 0x2000).scroll;
        assert_eq!(big, (0x2000u16.wrapping_mul(7).wrapping_add(2)) as i16);
    }

    #[test]
    fn masked_bytes_round_trip() {
        let (saved, masked) = MaskedBytes::mask((0x12, 0x34));
        assert_eq!(masked, (0xFF, 0xFF));
        assert_eq!(saved.restore(), (0x12, 0x34));
    }

    #[test]
    fn current_member_survives_when_it_is_on_the_roster() {
        assert_eq!(resolve_current_member(2, &[1, 2, 3]), 2);
        assert_eq!(resolve_current_member(3, &[1, 2, 3]), 3);
    }

    #[test]
    fn current_member_falls_back_to_the_first_roster_entry() {
        assert_eq!(resolve_current_member(9, &[1, 2, 3]), 1);
        // An empty roster skips the scan entirely and the index is already the
        // count, so retail still takes the fallback store.
        assert_eq!(resolve_current_member(9, &[]), 9);
    }

    #[test]
    fn one_member_lands_in_the_middle_cell() {
        assert_eq!(seed_member_cells(&[5]), [0, 5, 2]);
    }

    #[test]
    fn two_members_take_the_outer_cells() {
        assert_eq!(seed_member_cells(&[5, 6]), [5, 1, 6]);
    }

    #[test]
    fn three_members_fill_every_cell_in_order() {
        assert_eq!(seed_member_cells(&[5, 6, 7]), [5, 6, 7]);
    }

    #[test]
    fn zero_and_oversize_rosters_keep_the_countdown_seeds() {
        assert_eq!(seed_member_cells(&[]), [0, 1, 2]);
        assert_eq!(seed_member_cells(&[5, 6, 7, 8]), [0, 1, 2]);
    }

    #[test]
    fn entry_saves_the_callers_handler_and_installs_the_picker() {
        let e = picker_entry(0x12, &[1, 2, 3], None);
        assert_eq!(e.saved_handler, 0x12);
        assert_eq!(e.handler, PICKER_HANDLER);
        assert_eq!(e.picking, 1);
        assert_eq!(e.selection, -1);
        assert_eq!(e.sub_state, 0);
        assert_eq!(e.yield_marker, 1);
        assert_eq!(e.cursor, CURSOR_HOME);
        assert_eq!(e.cells, [1, 2, 3]);
    }

    #[test]
    fn a_remap_overwrites_the_handler_and_loses_the_callers_original() {
        let e = picker_entry(0x12, &[1], Some(9));
        assert_eq!(e.handler, 9);
        assert_eq!(
            e.saved_handler, PICKER_HANDLER,
            "the second save reads the already-installed 7"
        );
    }

    #[test]
    fn the_remap_sentinel_leaves_the_handler_alone() {
        let e = picker_entry(0x12, &[1], Some(REMAP_NONE));
        assert_eq!(e.handler, PICKER_HANDLER);
        assert_eq!(e.saved_handler, 0x12);
    }

    #[test]
    fn a_negative_remap_target_sign_extends_into_the_halfword() {
        // The table is read with `lb` and stored with `sh` after a
        // sign-extending shift pair, so -2 becomes 0xFFFE.
        let e = picker_entry(0, &[1], Some(-2));
        assert_eq!(e.handler, 0xFFFE);
    }

    #[test]
    fn only_states_four_and_seven_rewind() {
        assert_eq!(rearm_state(4), 1);
        assert_eq!(rearm_state(7), 1);
        for s in [0u32, 1, 2, 3, 5, 6, 8] {
            assert_eq!(rearm_state(s), s, "state {s}");
        }
    }

    #[test]
    fn pending_one_is_the_consumed_sentinel() {
        assert!(pending_is_consumed(1));
        assert!(!pending_is_consumed(0));
        assert!(!pending_is_consumed(0x8008_0000));
    }
}
