//! The field overlay's **subsystem actor installer**: the enter half of the
//! actor that the field VM's op `0x49` and the field menu button both spawn.
//!
//! PORT: FUN_801F1278
//!
//! `FUN_801F1278(actor)` is not a party picker. It is the one installer every
//! field subsystem screen goes through: it suspends field input, refreshes the
//! player, seeds the submode context `*_DAT_801C6EA4`, and installs the actor's
//! `+0x50` handler id - the index the dispatcher `FUN_801F159C` (the per-frame
//! resume / close half) `jalr`s through the 52-entry table `0x801F33B4`.
//!
//! The handler it installs by default is id `7` ([`STATE_PICK_HANDLER`]), and
//! slot `7` of that table is the **state pick** `FUN_801F1F4C`
//! ([`crate::field_state_pick`]), not a cursor screen. The state pick runs once
//! on the actor's first dispatch and moves the actor on to `0x30`, the
//! pause-menu session `FUN_801ED308` (or the debug shortcut `0x13`). Only when
//! the op-`0x49` operand pointer `_DAT_8007B450` is live and its sub-op maps to
//! a slot through the byte table `0x801F33A4` does the installer overwrite that
//! `7` with the sub-op's own screen (name entry, tile board, the shop and
//! flag windows, ...):
//!
//! ```text
//! 801f13f4  sh   v0,0x2e(v1)       ; ctx[+0x2E] = -1
//! 801f13f8  lhu  v0,0x50(s4)
//! 801f1400  sh   v0,0x40(v1)       ; ctx[+0x40] = actor[+0x50] (outgoing)
//! 801f1404  li   v0,0x7
//! 801f140c  sh   v0,0x50(s4)       ; actor[+0x50] = 7, the state pick
//! 801f141c  sh   zero,0x54(s4)     ; (delay slot) actor[+0x54] = 0
//! 801f1468  lb   v0,0x0(v0)        ; slot = (i8)table_0x801F33A4[sub_op]
//! 801f1470  beq  v0,a0,0x801f14b0  ; -1 -> keep the 7
//! 801f14ac  sh   v0,0x50(s4)       ; else install the sub-op's screen
//! ```
//!
//! So the menu button and every op-`0x49` row whose table byte is `-1` reach
//! the pause menu through this routine - the save-point menu press is one of
//! them. Both are described in
//! [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md) and
//! [`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md).
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
//! address belongs to the resident field overlay (PROT 0897) rather than to the
//! minigame overlay that names the file. The sixth,
//! `overlay_overlay_0897_801f1278.txt`, reports `0 instructions` and carries
//! only decompiled C - one of the artifacts
//! [`docs/tooling/ghidra.md`](../../docs/tooling/ghidra.md) catalogues, and not
//! usable as evidence on its own.
//!
//! ## What the installer writes, in order
//!
//! 1. `FUN_801DE190()` (input suspend), player `*_DAT_8007C364` flag word
//!    `[+0x10] |= 0x80000`, pad latch
//!    `_DAT_1F800394 &= ~0x8000`.
//! 2. The player's `+0x8E` / `+0x8F` bytes are forced to `0xFF` around a
//!    `FUN_801D9E1C(player, 0)` refresh and restored afterwards
//!    ([`MaskedBytes`]).
//! 3. `_DAT_8007BDD8 = 2`, player `[+0x5C]`, player `[+0x10] |= 0x1000000`, pad latch
//!    re-set ([`enter_context`]).
//! 4. `submode[+0x3E] = 1`, `actor[+0x1A] = 1`, `_DAT_8007B374 = 0`, the
//!    current-member byte `_DAT_8007B469` re-resolved against the roster
//!    ([`resolve_current_member`]).
//! 5. The handler install above ([`installer_entry`]), the submode state
//!    `DAT_801F2734` rewind ([`rearm_state`]).
//! 6. The submode context's cursor home and its three roster cells
//!    ([`seed_member_cells`]).
//!
//! ## Roster cells
//!
//! The three cells at `submode[+0x36]`, `+0x38`, `+0x3A` are seeded `0, 1, 2`
//! by a countdown loop and then **overwritten by roster size**: a one-member
//! party goes into the *middle* cell and a two-member party takes the *outer*
//! two. This routine only writes them; which screen reads them is not traced
//! here.
//!
//! # NOT WIRED
//!
//! No engine caller for this module's kernels. The installer's load-bearing
//! decision - default handler `7`, then the `0x801F33A4` sub-op table read - is
//! ported a second time where the engine needs it, in `engine-core`'s
//! `field_submode_screen` (the `-1`-row arm that turns an op-`0x49` park into a
//! scripted menu press) and `World::field_menu_button_state` (which runs the
//! state pick with handler `7`). The context-flag, pad-latch, roster-cell and
//! cursor-home writes have no engine reader: the engine models neither the
//! submode context `*_DAT_801C6EA4` nor the field-context flag word.

/// Field-context flag bit the enter path raises (`ctx[+0x10] |= 0x80000`) -
/// the "a modal submode owns input" marker the close path clears.
pub const CTX_SUBMODE_BUSY: u32 = 0x0008_0000;
/// Second field-context flag bit raised on the way in (`|= 0x1000000`).
pub const CTX_SUBSYSTEM_ACTIVE: u32 = 0x0100_0000;
/// Pad-latch bit cleared and then re-set around the roster seed
/// (`_DAT_1F800394 & ~0x8000`, then `| 0x8000`).
pub const PAD_LATCH_BIT: u32 = 0x0000_8000;
/// Submode kind word the enter path writes (`_DAT_8007BDD8 = 2`).
pub const SUBMODE_KIND: u32 = 2;
/// Default handler id installed into the actor's `+0x50` slot: slot `7` of
/// the `0x801F33B4` table, the state pick `FUN_801F1F4C`
/// ([`crate::field_state_pick`]), which hands on to the pause-menu session.
pub const STATE_PICK_HANDLER: u16 = 7;
/// Cursor home position (`submode[+0x46]`, `submode[+0x48]`).
pub const CURSOR_HOME: (u16, u16) = (0xA0, 0x58);
/// Submode states the enter path rewinds to `1` when re-armed.
pub const REARM_STATES: [u32; 2] = [4, 7];
/// Sentinel in the op-`0x49` sub-op slot table (`0x801F33A4`) meaning "this
/// sub-op opens no screen of its own" - the default handler stands.
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
        flags: flags_before | CTX_SUBMODE_BUSY | CTX_SUBSYSTEM_ACTIVE,
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

/// The three roster cells at `submode[+0x36]`, `+0x38`, `+0x3A`.
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

/// What the enter path writes into the submode context `*_DAT_801C6EA4` and the calling actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallerEntry {
    /// `submode[+0x3E] = 1` - the completion gate `FUN_801F159C` polls; the
    /// screen the handler opens clears it to close.
    pub completion_gate: u16,
    /// `submode[+0x2E] = -1` - the selection sentinel.
    pub selection: i16,
    /// `submode[+0x40]` - the caller's previous `+0x50` handler, saved.
    pub saved_handler: u16,
    /// `actor[+0x50]` - the installed handler: [`STATE_PICK_HANDLER`] or the
    /// sub-op's slot.
    pub handler: u16,
    /// `actor[+0x54] = 0` - the handler's sub-state.
    pub sub_state: u16,
    /// `actor[+0x1A] = 1` - the actor's yield marker.
    pub yield_marker: u16,
    /// `submode[+0x46]`, `submode[+0x48]` - cursor home.
    pub cursor: (u16, u16),
    /// The three roster cells.
    pub cells: [u16; 3],
}

/// Build the enter-path writes (`0x801F1368` onward), including the optional
/// op-`0x49` sub-op slot lookup.
///
/// `pending` is `_DAT_8007B450`: `0` means no parked op `0x49`, `1` is consumed and
/// cleared on the way in, and any other value is a pointer whose first byte
/// indexes the remap table at `DAT_801F33A4`. When that lookup yields anything
/// but [`REMAP_NONE`], the installed handler becomes the remapped value instead
/// of [`STATE_PICK_HANDLER`] - and the saved handler is re-saved from the
/// already-overwritten `+0x50`, so a remap saves `7`, not the caller's original.
pub fn installer_entry(caller_handler: u16, roster: &[u8], remap: Option<i8>) -> InstallerEntry {
    let mut entry = InstallerEntry {
        completion_gate: 1,
        selection: -1,
        saved_handler: caller_handler,
        handler: STATE_PICK_HANDLER,
        sub_state: 0,
        yield_marker: 1,
        cursor: CURSOR_HOME,
        cells: seed_member_cells(roster),
    };
    if let Some(target) = remap.filter(|&t| t != REMAP_NONE) {
        // Retail re-runs the same three stores with `+0x50` already at 7.
        entry.saved_handler = STATE_PICK_HANDLER;
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

/// Is the op-`0x49` operand word the "consume and clear" sentinel?
pub const fn pending_is_consumed(pending: u32) -> bool {
    pending == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_sets_both_context_bits_and_leaves_the_latch_set() {
        let c = enter_context(0x0000_0001, 0, 0);
        assert_eq!(
            c.flags,
            0x0000_0001 | CTX_SUBMODE_BUSY | CTX_SUBSYSTEM_ACTIVE
        );
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
    fn entry_saves_the_callers_handler_and_installs_the_state_pick() {
        let e = installer_entry(0x12, &[1, 2, 3], None);
        assert_eq!(e.saved_handler, 0x12);
        assert_eq!(e.handler, STATE_PICK_HANDLER);
        assert_eq!(e.completion_gate, 1);
        assert_eq!(e.selection, -1);
        assert_eq!(e.sub_state, 0);
        assert_eq!(e.yield_marker, 1);
        assert_eq!(e.cursor, CURSOR_HOME);
        assert_eq!(e.cells, [1, 2, 3]);
    }

    #[test]
    fn a_remap_overwrites_the_handler_and_loses_the_callers_original() {
        let e = installer_entry(0x12, &[1], Some(9));
        assert_eq!(e.handler, 9);
        assert_eq!(
            e.saved_handler, STATE_PICK_HANDLER,
            "the second save reads the already-installed 7"
        );
    }

    #[test]
    fn the_remap_sentinel_leaves_the_handler_alone() {
        let e = installer_entry(0x12, &[1], Some(REMAP_NONE));
        assert_eq!(e.handler, STATE_PICK_HANDLER);
        assert_eq!(e.saved_handler, 0x12);
    }

    #[test]
    fn a_negative_remap_target_sign_extends_into_the_halfword() {
        // The table is read with `lb` and stored with `sh` after a
        // sign-extending shift pair, so -2 becomes 0xFFFE.
        let e = installer_entry(0, &[1], Some(-2));
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
