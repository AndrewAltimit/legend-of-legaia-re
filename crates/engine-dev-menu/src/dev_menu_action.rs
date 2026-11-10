//! The developer menu's row-action dispatcher `FUN_801EA9B0`.
//!
//! It lives in the field overlay (PROT 0897, base `0x801CE818`) beside the
//! world-map panel window system (`legaia_engine_field::world_map_panel`),
//! whose list-cursor helper the dev menu's list picker mirrors, but its only
//! caller in the port is the dev-menu screen
//! `legaia_engine_menus::dev_menu_host`, whose cancel leg asks it for the park
//! phase and the draw-gate factor.
//!
//! ## Source
//!
//! - `ghidra/scripts/funcs/overlay_cutscene_dialogue_801ea9b0.txt`
//!
//! The same-VA dumps under the `overlay_0896_*` / `overlay_0897_*` prefixes
//! are **not** usable here: at this address they print a body that resolves
//! against no extracted image. The body ported here is the one that matches
//! PROT 0897 at the queried VA.

// ---------------------------------------------------------------------------
// FUN_801EA9B0 - dev-menu row-action dispatcher
// ---------------------------------------------------------------------------

/// Row count the dev-menu action dispatcher bounds against
/// (`sltiu v0,v1,0x18`).
pub const DEV_MENU_ACTION_ROWS: i16 = 0x18;

/// The phase the out-of-range arm parks the actor in (`ctx[+0x54] = 1`).
pub const DEV_MENU_ACTION_PARK_PHASE: i16 = 1;

/// The value `FUN_801EA9B0` returns. It is `1` on **every** path: `s1` is
/// loaded with `1` in the delay slot of the bound check and every arm exits
/// through `move v0,s1`, including the out-of-range arm. The routine has no
/// second return value.
pub const DEV_MENU_ACTION_RESULT: i32 = 1;

/// One frame of the dev-menu row-action dispatcher.
///
/// `row` is `ctx[+0x9E]`, the cursor row the list picker maintains. Rows
/// `0..DEV_MENU_ACTION_ROWS` select one of the 24 jump-table arms (each a
/// debug cheat: heal the party, cycle the encounter rate at `_DAT_8007B5F8`,
/// max every stat on the three `0x80084140 + n*0x414` records, grant the
/// whole item table, cycle the BGM index at `_DAT_801F2E90`, toggle the
/// `_DAT_8007B606` flag, ...). Out-of-range parks the caller's phase at
/// [`DEV_MENU_ACTION_PARK_PHASE`].
///
/// Returns `(park_phase, result)`: `park_phase` is `Some` for a row past the
/// table, the one arm with no work of its own, and `result` is always
/// [`DEV_MENU_ACTION_RESULT`].
///
/// Every in-range arm writes `ctx[+0x54]` too - [`dev_menu_action_phase`]
/// is that store, row by row - so `None` here does not mean the phase word
/// is left alone; it means the row ran its cheat and the phase it leaves is
/// the arm's own.
///
/// The arms' *effects* are not modelled - they poke globals and party
/// records that have no engine counterpart. What is ported is the contract
/// the **caller** depends on: the bound, the phase each path leaves, and the
/// unconditional `1`.
///
/// PORT: FUN_801ea9b0 (dispatch head + shared epilogue)
///
/// Wired: `legaia_engine_menus::dev_menu_host::DevMenuSession::tick` runs this
/// on its cancel leg and uses both halves of the result - the park phase and
/// the factor [`crate::dev_menu_leaves::list_body_draws`] multiplies.
pub fn dev_menu_action(row: i16) -> (Option<i16>, i32) {
    if (row as u16 as u32) >= DEV_MENU_ACTION_ROWS as u32 {
        return (
            Some(dev_menu_action_phase(row, false)),
            DEV_MENU_ACTION_RESULT,
        );
    }
    (None, DEV_MENU_ACTION_RESULT)
}

/// The phase each in-range row's arm stores to `ctx[+0x54]`, read off the
/// 24 arms behind the jump table at `0x801CF2E4`. Row 1 is the one arm that
/// picks: `2` while `_DAT_8007B868` is non-zero, else `4` (`0x801EAC40`).
const DEV_MENU_ROW_PHASES: [i16; DEV_MENU_ACTION_ROWS as usize] = [
    4, 2, 4, 1, 1, 4, 1, 2, 2, 2, 1, 2, 1, 1, 1, 0, 1, 1, 2, 2, 2, 2, 1, 2,
];

/// The phase `FUN_801EA9B0` leaves in `ctx[+0x54]` for `row`: the arm's own
/// store for a row of the table, [`DEV_MENU_ACTION_PARK_PHASE`] past it.
/// `row_one_gate` is `_DAT_8007B868 != 0`, read by row 1 alone.
///
/// PORT: FUN_801ea9b0 (the 24 arms' `ctx[+0x54]` stores)
pub fn dev_menu_action_phase(row: i16, row_one_gate: bool) -> i16 {
    match usize::try_from(row)
        .ok()
        .and_then(|r| DEV_MENU_ROW_PHASES.get(r))
    {
        Some(_) if row == 1 && !row_one_gate => 4,
        Some(&phase) => phase,
        None => DEV_MENU_ACTION_PARK_PHASE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_menu_action_always_returns_one() {
        for row in [-1i16, 0, 1, 0x17, 0x18, 0x400] {
            assert_eq!(dev_menu_action(row).1, DEV_MENU_ACTION_RESULT);
        }
    }

    #[test]
    fn dev_menu_action_parks_out_of_range_rows() {
        assert_eq!(dev_menu_action(0).0, None);
        assert_eq!(dev_menu_action(0x17).0, None);
        assert_eq!(dev_menu_action(0x18).0, Some(DEV_MENU_ACTION_PARK_PHASE));
        // The bound is `sltiu`, so a negative row folds to a huge unsigned.
        assert_eq!(dev_menu_action(-1).0, Some(DEV_MENU_ACTION_PARK_PHASE));
    }
}
