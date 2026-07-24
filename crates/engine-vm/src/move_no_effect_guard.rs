//! Battle **queued-magic guard** at `FUN_801F3C34` (PROT 0898, base
//! `0x801CE818`).
//!
//! A short pre-resolution pass over the acting battle actor's queued action
//! byte `actor[+0x1DF]`. It fires a fixed message id `0x66` through the
//! battle message printer `FUN_801D8DE8(0x66, 0)` and mirrors that id into
//! the battle context byte `ctx[+0x18]`, gated on the caster's own spell
//! list.
//!
//! ## Body, read from the PROT 0898 image
//!
//! ```text
//!   a3     = ctx[+0x13]                       // acting-actor index
//!   actor  = *(0x801C9370 + a3*4)             // battle actor pointer table
//!   action = actor[+0x1DF]                    // queued action id
//!   if action == 0x85 || action == 0x8E || action >= 0x96: return
//!   char   = *(u8)(0x8007BD10 + a3) - 1       // party slot of that actor
//!   base   = 0x80084140 + char*0x414          // = record - 0x5C8
//!   i = first index in 0..0x20 with base[0x705 + i] == action   (else 0x20)
//!   if base[0x729 + i] < 3: return
//!   if *(0x801F6960) != 0: return
//!   *(0x800775B4) = 0x801CFA20                // install the follow-up hook
//!   FUN_801D8DE8(0x66, 0)
//!   ctx[+0x18] = 0x66
//! ```
//!
//! `0x80084140 + char*0x414 + 0x705` is character-record `+0x13D`, the
//! **spell-id array**; `+0x729` is record `+0x161`, its parallel level byte
//! (see `docs/formats/save-record.md` and `legaia_save::character::SpellList`).
//! So the scan is "find the queued action in this caster's learned-spell list
//! and read its level", and the message is emitted when that level is `>= 3`.
//!
//! Two things this body is **not**, both worth stating because the shape
//! invites the guess:
//!
//! * It is not a "move is unusable" reject - it changes no queue state and
//!   returns nothing. Its only effects are the message, the context byte and
//!   the installed hook pointer.
//! * The `>= 3` test is the *firing* condition, not a suppression: a level-1
//!   or level-2 spell takes the early return and prints nothing.
//!
//! The three action ids that early-out (`0x85`, `0x8E`, and everything from
//! `0x96` up) are excluded before the party slot is even read, so a
//! non-magic queued action never reaches the record.
//!
//! `see ghidra/scripts/funcs/overlay_muscle_dome_801f3c34.txt`

/// Queued-action ids the guard skips outright.
pub const SKIPPED_ACTIONS: [u8; 2] = [0x85, 0x8E];

/// Queued-action ids at or above this value are skipped.
pub const ACTION_CEILING: u8 = 0x96;

/// Number of spell-list entries the scan walks (retail caps at `0x20`, short
/// of the record's 36-entry array).
pub const SCAN_LIMIT: usize = 0x20;

/// Minimum spell level that lets the message fire.
pub const MIN_LEVEL: u8 = 3;

/// The message id the guard prints and mirrors into `ctx[+0x18]`.
pub const MESSAGE_ID: u8 = 0x66;

/// Index of the queued action inside the caster's spell-id array, or
/// [`SCAN_LIMIT`] when the scan runs off the end - retail keeps using the
/// out-of-range index to read the level array, which is long enough that the
/// read stays inside the record.
///
/// PORT: FUN_801f3c34 (`0x801F3C9C..0x801F3CDC`)
pub fn spell_index_of(spell_ids: &[u8], action: u8) -> usize {
    for i in 0..SCAN_LIMIT {
        if spell_ids.get(i).copied() == Some(action) {
            return i;
        }
    }
    SCAN_LIMIT
}

/// Run the guard for one queued action.
///
/// `spell_ids` / `spell_levels` are the caster's record `+0x13D` and `+0x161`
/// arrays; `hook_installed` is the `*(0x801F6960) != 0` gate, which suppresses
/// the message when a follow-up is already pending.
///
/// Returns the message id to print, or `None` for any of the four early
/// returns.
///
/// PORT: FUN_801f3c34
///
/// NOT WIRED: the engine's battle round resolves a queued action through
/// `crate::battle_action` without this pre-pass; there is no call site until
/// the message-hook slot `0x800775B4` is modelled.
pub fn queued_magic_message(
    action: u8,
    spell_ids: &[u8],
    spell_levels: &[u8],
    hook_installed: bool,
) -> Option<u8> {
    if SKIPPED_ACTIONS.contains(&action) || action >= ACTION_CEILING {
        return None;
    }
    let idx = spell_index_of(spell_ids, action);
    let level = spell_levels.get(idx).copied().unwrap_or(0);
    if level < MIN_LEVEL {
        return None;
    }
    if hook_installed {
        return None;
    }
    Some(MESSAGE_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(action: u8, level: u8) -> ([u8; 36], [u8; 36]) {
        let mut ids = [0u8; 36];
        let mut levels = [0u8; 36];
        ids[4] = action;
        levels[4] = level;
        (ids, levels)
    }

    #[test]
    fn skipped_action_ids_return_early() {
        let (ids, levels) = lists(0x85, 9);
        assert_eq!(queued_magic_message(0x85, &ids, &levels, false), None);
        let (ids, levels) = lists(0x8E, 9);
        assert_eq!(queued_magic_message(0x8E, &ids, &levels, false), None);
    }

    #[test]
    fn actions_at_or_above_the_ceiling_return_early() {
        let (ids, levels) = lists(0x96, 9);
        assert_eq!(queued_magic_message(0x96, &ids, &levels, false), None);
        let (ids, levels) = lists(0xFF, 9);
        assert_eq!(queued_magic_message(0xFF, &ids, &levels, false), None);
    }

    #[test]
    fn level_three_fires_the_message() {
        let (ids, levels) = lists(0x81, 3);
        assert_eq!(queued_magic_message(0x81, &ids, &levels, false), Some(0x66));
    }

    #[test]
    fn level_below_three_is_silent() {
        for lvl in 0..MIN_LEVEL {
            let (ids, levels) = lists(0x81, lvl);
            assert_eq!(queued_magic_message(0x81, &ids, &levels, false), None);
        }
    }

    #[test]
    fn a_pending_hook_suppresses_the_message() {
        let (ids, levels) = lists(0x81, 5);
        assert_eq!(queued_magic_message(0x81, &ids, &levels, true), None);
    }

    #[test]
    fn unlearned_action_reads_the_out_of_range_slot() {
        // The action is not in the list, so the scan returns SCAN_LIMIT and
        // the level read lands at index 0x20 - inside the 36-byte array.
        let mut ids = [0u8; 36];
        let mut levels = [0u8; 36];
        ids[0] = 0x70;
        levels[SCAN_LIMIT] = 7;
        assert_eq!(spell_index_of(&ids, 0x81), SCAN_LIMIT);
        assert_eq!(queued_magic_message(0x81, &ids, &levels, false), Some(0x66));
        levels[SCAN_LIMIT] = 1;
        assert_eq!(queued_magic_message(0x81, &ids, &levels, false), None);
    }

    #[test]
    fn scan_stops_at_the_retail_limit() {
        let mut ids = [0u8; 36];
        ids[SCAN_LIMIT + 1] = 0x81;
        assert_eq!(spell_index_of(&ids, 0x81), SCAN_LIMIT);
    }
}
