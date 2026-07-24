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
///
/// NOT WIRED: shared by [`queued_magic_message`] and
/// [`follow_up_hook_install`], both of which are themselves inert - the
/// engine's battle round resolves a queued action through
/// [`crate::battle_action`] with no pre-pass, so nothing reaches this scan.
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

// ---------------------------------------------------------------------------
// The installer half: FUN_801F3D3C
// ---------------------------------------------------------------------------

/// The two globals `FUN_801F3D3C` writes and `FUN_801F3C34` reads. They are
/// one latch: while `pending` is non-zero the guard above stays silent,
/// because a follow-up is already queued.
///
/// * `0x800775B4` - the follow-up routine pointer, taken from word `1` of the
///   selected [`FOLLOW_UP_TABLE`] record.
/// * `0x801F6960` - the follow-up id, byte `0` of that record.
/// * `0x801F6964` - the follow-up countdown, always seeded [`FOLLOW_UP_HOLD`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowUpHook {
    /// `*(0x800775B4)`.
    pub routine: u32,
    /// `*(0x801F6960)` - the pending latch the guard reads.
    pub pending: u8,
    /// `*(0x801F6964)`.
    pub hold: i32,
}

/// Runtime VA of the `[class][level band]` follow-up record table
/// (`0x20` bytes per class = four 8-byte records).
pub const FOLLOW_UP_TABLE: u32 = 0x801F_6870;
/// Runtime VA of the `[class][class]` pass-chance byte table, `8` per row.
pub const CLASS_PAIR_TABLE: u32 = 0x801F_53E8;
/// Frames the installer seeds into `0x801F6964`.
pub const FOLLOW_UP_HOLD: i32 = 0xB4;
/// A class-pair byte at or above this passes the suppression test.
pub const CLASS_PAIR_PASS: u8 = 0x65;
/// The class value that skips the roll outright.
pub const CLASS_SKIP: u8 = 5;
/// Class values below this index the seven-entry jump table at `0x801CFA2C`;
/// anything else falls through to the installer tail.
pub const CLASS_JUMP_TABLE_LEN: u8 = 7;

/// Everything the installer reads that is not the caster's spell record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FollowUpInputs {
    /// `ctx[+0x287]` - when zero the suppression roll is skipped entirely.
    pub roll_enabled: u8,
    /// `(*(0x801C9358))[+0x1D]` - the acting side's class byte.
    pub actor_class: u8,
    /// `(*(0x801C9348))[+0x1D]` - the opposing side's class byte.
    pub other_class: u8,
    /// The `[actor_class][other_class]` byte of [`CLASS_PAIR_TABLE`].
    pub class_pair_byte: u8,
    /// `FUN_80056798()` - this frame's BIOS `rand()` draw.
    pub rand: i32,
}

/// What the installer decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowUpOutcome {
    /// The spell level scan came back below [`MIN_LEVEL`].
    LevelTooLow,
    /// The class-pair roll suppressed the follow-up.
    Suppressed,
    /// The class byte indexes the seven-entry jump table at `0x801CFA2C`;
    /// those arms are separate bodies and are not dumped with this one.
    JumpTable(u8),
    /// The installer tail ran: the caller installs this hook.
    Installed { band: i32, hook: FollowUpHook },
}

/// The level band the tail folds a spell level into: `(level - 3) >> 1`, so
/// levels `3..=4` share band `0`, `5..=6` band `1`, and so on. It is a byte
/// stride of `8` into the class's `0x20`-byte row, which bounds the useful
/// range at four bands.
///
/// PORT: FUN_801f3d3c (`0x801F4420..0x801F4434`)
///
/// NOT WIRED: a helper of [`follow_up_hook_install`], which is itself inert -
/// same blocker.
pub fn follow_up_band(level: u8) -> i32 {
    (level as i32 - 3) >> 1
}

/// Whether the class-pair roll lets the follow-up through.
///
/// The roll only runs when `ctx[+0x287]` is set. Inside it, two shapes pass
/// without consulting the table at all: an actor class of [`CLASS_SKIP`], and
/// a `rand()` divisible by five. Otherwise the `[actor][other]` byte decides,
/// and a byte **below** [`CLASS_PAIR_PASS`] is what suppresses - the sense is
/// the opposite of the "high value = more likely" reading the table shape
/// invites.
///
/// PORT: FUN_801f3d3c (`0x801F3DEC..0x801F3E7C`)
///
/// NOT WIRED: a helper of [`follow_up_hook_install`], which is itself inert -
/// same blocker.
pub fn follow_up_roll_passes(inp: &FollowUpInputs) -> bool {
    if inp.roll_enabled == 0 {
        return true;
    }
    if inp.actor_class == CLASS_SKIP {
        return true;
    }
    if inp.rand % 5 == 0 {
        return true;
    }
    inp.class_pair_byte >= CLASS_PAIR_PASS
}

/// The sibling of [`queued_magic_message`]: the routine that **installs** the
/// follow-up the guard then reads.
///
/// It opens on the identical preamble - resolve the acting actor, take its
/// queued action byte `+0x1DF`, find that action in the caster's spell-id
/// array and read the parallel level byte, bail below [`MIN_LEVEL`] - and then
/// runs the class-pair roll before selecting a record out of
/// [`FOLLOW_UP_TABLE`] by `[actor_class][level band]`. The record's byte `0`
/// becomes the pending latch, its word `1` the routine pointer, and the hold
/// is always [`FOLLOW_UP_HOLD`]; the same message id [`MESSAGE_ID`] is printed
/// through `FUN_801D8DE8(0x66, 0)`.
///
/// PORT: FUN_801f3d3c
///
/// NOT WIRED: same blocker as [`queued_magic_message`] - nothing in the port's
/// battle round runs a pre-resolution pass, and the follow-up slot
/// `0x800775B4` is not modelled, so there is no caller and nowhere to install
/// the returned hook.
pub fn follow_up_hook_install(
    action: u8,
    spell_ids: &[u8],
    spell_levels: &[u8],
    inp: &FollowUpInputs,
    record: FollowUpHookRecord,
) -> FollowUpOutcome {
    let idx = spell_index_of(spell_ids, action);
    let level = spell_levels.get(idx).copied().unwrap_or(0);
    if level < MIN_LEVEL {
        return FollowUpOutcome::LevelTooLow;
    }
    if !follow_up_roll_passes(inp) {
        return FollowUpOutcome::Suppressed;
    }
    if inp.actor_class < CLASS_JUMP_TABLE_LEN {
        return FollowUpOutcome::JumpTable(inp.actor_class);
    }
    FollowUpOutcome::Installed {
        band: follow_up_band(level),
        hook: FollowUpHook {
            routine: record.routine,
            pending: record.id,
            hold: FOLLOW_UP_HOLD,
        },
    }
}

/// One 8-byte [`FOLLOW_UP_TABLE`] record, as the caller reads it out of the
/// overlay image at `[actor_class][band]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FollowUpHookRecord {
    /// Byte `0` - the pending id.
    pub id: u8,
    /// Word `1` - the routine pointer.
    pub routine: u32,
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

    fn inputs() -> FollowUpInputs {
        FollowUpInputs {
            roll_enabled: 1,
            actor_class: 9,
            other_class: 2,
            class_pair_byte: 0x70,
            rand: 3,
        }
    }

    #[test]
    fn follow_up_bands_pair_levels() {
        assert_eq!(follow_up_band(3), 0);
        assert_eq!(follow_up_band(4), 0);
        assert_eq!(follow_up_band(5), 1);
        assert_eq!(follow_up_band(6), 1);
        assert_eq!(follow_up_band(9), 3);
    }

    #[test]
    fn a_low_class_pair_byte_suppresses_the_follow_up() {
        let mut inp = inputs();
        inp.class_pair_byte = CLASS_PAIR_PASS - 1;
        assert!(!follow_up_roll_passes(&inp));
        inp.class_pair_byte = CLASS_PAIR_PASS;
        assert!(follow_up_roll_passes(&inp));
    }

    #[test]
    fn the_roll_is_skipped_three_ways() {
        let mut inp = inputs();
        inp.class_pair_byte = 0;
        // ctx[+0x287] clear.
        inp.roll_enabled = 0;
        assert!(follow_up_roll_passes(&inp));
        // The skip class.
        inp.roll_enabled = 1;
        inp.actor_class = CLASS_SKIP;
        assert!(follow_up_roll_passes(&inp));
        // One draw in five.
        inp.actor_class = 9;
        inp.rand = 10;
        assert!(follow_up_roll_passes(&inp));
        inp.rand = 11;
        assert!(!follow_up_roll_passes(&inp));
    }

    #[test]
    fn the_installer_shares_the_guards_level_gate() {
        let (ids, levels) = lists(0x81, 2);
        assert_eq!(
            follow_up_hook_install(
                0x81,
                &ids,
                &levels,
                &inputs(),
                FollowUpHookRecord::default()
            ),
            FollowUpOutcome::LevelTooLow
        );
    }

    #[test]
    fn a_low_class_reaches_the_jump_table_instead_of_the_tail() {
        let (ids, levels) = lists(0x81, 5);
        let mut inp = inputs();
        inp.actor_class = 2;
        assert_eq!(
            follow_up_hook_install(0x81, &ids, &levels, &inp, FollowUpHookRecord::default()),
            FollowUpOutcome::JumpTable(2)
        );
    }

    #[test]
    fn the_tail_installs_the_record_with_a_fixed_hold() {
        let (ids, levels) = lists(0x81, 5);
        let rec = FollowUpHookRecord {
            id: 0x2A,
            routine: 0x801C_FA20,
        };
        assert_eq!(
            follow_up_hook_install(0x81, &ids, &levels, &inputs(), rec),
            FollowUpOutcome::Installed {
                band: 1,
                hook: FollowUpHook {
                    routine: 0x801C_FA20,
                    pending: 0x2A,
                    hold: FOLLOW_UP_HOLD,
                },
            }
        );
    }
}
