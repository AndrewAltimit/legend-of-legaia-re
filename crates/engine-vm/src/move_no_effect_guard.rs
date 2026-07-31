//! Battle **queued-magic follow-up guard** at `FUN_801F3C34` (PROT 0898, base
//! `0x801CE818`, file `0x2541C`).
//!
//! A short pass over the acting battle actor's queued action byte
//! `actor[+0x1DF]`. It fires a fixed message id `0x66` through the battle
//! message printer `FUN_801D8DE8(0x66, 0)` and mirrors that id into the battle
//! context byte `ctx[+0x18]`, gated on the caster's own spell list.
//!
//! ## Where it runs
//!
//! Its one caller is the battle-action SM itself: `FUN_801E295C` reaches
//! `jal 0x801f3c34` at `0x801E4CB8`, at the head of **state `0x36`**
//! (`Summon - return-from-fade`, ported as
//! [`crate::battle_action`]'s `SummonReturn`). The same SM body then runs the
//! 7-slot `+0x21C` / `+0x8` reset the port already carries, calls the
//! summon spell-XP check `FUN_801E70BC`, and clamps the follow-up hold
//! `0x801F6964` to `1`. So this is a **post-cast** pass on the summon /
//! Seru-magic band, not a pre-resolution one.
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
//! ## Reading the dumps at this VA
//!
//! `0x801F3C34` is PROT 0898's own code (file `0x2541C`, the same base that
//! puts the move-power table at `0x801F4F5C` / file `0x26744`), and the
//! `jal` that proves it is in the battle overlay's SM dump. Every *dump file*
//! at this VA is nevertheless named for some other image
//! (`overlay_muscle_dome_801f3c34.txt`, `..._dance_...`, `..._fishing_...`,
//! and three more): those overlays are short, so the extracted `.bin` window
//! runs past their own content into the same physical bytes, and all six
//! disassemble identically. `overlay_0897_801f3c34.txt` is a different trap
//! again - it is a dump of `FUN_801F3894`, printed under the field overlay's
//! base for bytes that are not the field overlay's. Only the absolute
//! operands survive a wrong base, which is why every global and `jal` target
//! quoted here is safe to read off those dumps and the *entry address* is not.
//! See `docs/tooling/dump-corpus-integrity.md` and
//! `docs/tooling/call-target-integrity.md`.
//!
//! `see ghidra/scripts/funcs/overlay_muscle_dome_801f3c34.txt` (body) and
//! `overlay_battle_action_801e295c.txt` `0x801E4CB8` (the call site)

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
/// Shared by [`queued_magic_message`] - live from the action SM's state
/// `0x36` - and [`follow_up_hook_install`], which is still inert.
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
/// Live from the action SM's state `0x36` (`SummonReturn`, retail's
/// `jal 0x801f3c34` at `0x801E4CB8`), which is where all three of its inputs
/// now come from:
///
/// 1. the caster's record `+0x13D` / `+0x161` spell-id and spell-level arrays
///    reach it through
///    [`BattleActionHost::caster_spell_list`](crate::battle_action::BattleActionHost::caster_spell_list)
///    (`legaia_save::character::SpellList` on the engine side);
/// 2. the battle message channel `FUN_801D8DE8(id, mode)` is
///    [`BattleActionHost::ui_element`](crate::battle_action::BattleActionHost::ui_element),
///    the same printer the SM's other HUD calls already use, and the
///    `ctx[+0x18]` mirror is
///    [`BattleActionCtx::message_id`](crate::battle_action::BattleActionCtx::message_id);
/// 3. the pending latch `0x801F6960` is
///    [`BattleActionCtx::follow_up_pending`](crate::battle_action::BattleActionCtx::follow_up_pending).
///
/// The latch's **writer** is still missing: [`follow_up_hook_install`] is the
/// routine that sets it, and that half stays inert (see its own note), so in
/// the port the latch is only ever read as clear. Retail's guard is silent
/// whenever a follow-up is already pending, so the port errs toward printing
/// the message in a case retail might not - a one-branch difference, named
/// here rather than papered over.
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

/// Runtime VA of the `[element][level band]` follow-up record table
/// (`0x20` bytes per element = four 8-byte records; `sll v0,v0,0x5` at
/// `0x801F4438`).
pub const FOLLOW_UP_TABLE: u32 = 0x801F_6870;

/// Runtime VA of the table the suppression roll indexes: **the battle
/// element-affinity matrix**, 8 bytes per row (`sll v1,v1,0x3` at
/// `0x801F3E64`).
///
/// An earlier reading here called this "the `[class][class]` pass-chance
/// byte table". It is `0x801F53E8`, the same matrix
/// `docs/subsystems/battle-formulas.md` pins as
/// `affinity[attacker_element][defender_element]` and the engine already
/// parses off PROT 0898 as `World::element_affinity` - and the `+0x1D` bytes
/// it is indexed by are the two records' **element** bytes, not a class.
pub const ELEMENT_AFFINITY_TABLE: u32 = 0x801F_53E8;

/// Frames the installer seeds into `0x801F6964`.
pub const FOLLOW_UP_HOLD: i32 = 0xB4;

/// An affinity percent at or above this passes the suppression test, i.e.
/// the follow-up needs the defender to be **weak** to the attacker's element
/// (`0x65` = 101%). Below it, the roll suppresses - the sense is the opposite
/// of the "high value = more likely to be blocked" reading the byte invites.
pub const AFFINITY_WEAK_MIN: u8 = 0x65;

/// The element value that skips the roll outright.
pub const ELEMENT_SKIP: u8 = 5;

/// Element values below this index the seven-entry jump table at
/// `0x801CFA2C`; element `7` (non-elemental) falls through to the installer
/// tail.
pub const ELEMENT_JUMP_TABLE_LEN: u8 = 7;

/// Everything the installer reads that is not the caster's spell record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FollowUpInputs {
    /// `ctx[+0x287]` - when zero the suppression roll is skipped entirely.
    pub roll_enabled: u8,
    /// `(*(0x801C9358))[+0x1D]` - the acting side's **element** byte, off the
    /// readef-installed actor record (`docs/formats/summon-readef.md`).
    pub actor_element: u8,
    /// `(*(0x801C9348))[+0x1D]` - the opposing side's element byte.
    pub other_element: u8,
    /// The `[actor_element][other_element]` byte of
    /// [`ELEMENT_AFFINITY_TABLE`] - an affinity **percent**.
    pub affinity_pct: u8,
    /// `FUN_80056798()` - this frame's BIOS `rand()` draw.
    pub rand: i32,
}

/// What the installer decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowUpOutcome {
    /// The spell level scan came back below [`MIN_LEVEL`].
    LevelTooLow,
    /// The affinity roll suppressed the follow-up.
    Suppressed,
    /// The element byte (`< 7`) indexes the seven-entry jump table at
    /// `0x801CFA2C` - one arm per element. Those arms are separate bodies and
    /// are not dumped with this one.
    JumpTable(u8),
    /// The installer tail ran: the caller installs this hook.
    Installed { band: i32, hook: FollowUpHook },
}

/// The level band the tail folds a spell level into: `(level - 3) >> 1`, so
/// levels `3..=4` share band `0`, `5..=6` band `1`, and so on. It is a byte
/// stride of `8` into the element's `0x20`-byte row, which bounds the useful
/// range at four bands.
///
/// PORT: FUN_801f3d3c (`0x801F4420..0x801F4434`)
///
/// NOT WIRED: a helper of [`follow_up_hook_install`], which is itself inert -
/// same blocker.
pub fn follow_up_band(level: u8) -> i32 {
    (level as i32 - 3) >> 1
}

/// Whether the element-affinity roll lets the follow-up through.
///
/// The roll only runs when `ctx[+0x287]` is set. Inside it, two shapes pass
/// without consulting the matrix at all: an actor element of
/// [`ELEMENT_SKIP`], and a `rand()` divisible by five (the
/// `0x66666667` magic-multiply divide at `0x801F3E24`). Otherwise the
/// `affinity[actor][other]` percent decides, and a value **below**
/// [`AFFINITY_WEAK_MIN`] is what suppresses: the follow-up needs the opposing
/// side to be elementally weak to the caster.
///
/// PORT: FUN_801f3d3c (`0x801F3DEC..0x801F3E7C`)
///
/// NOT WIRED: a helper of [`follow_up_hook_install`], which is itself inert -
/// same blocker (the `[element][band]` record table at `0x801F6870` has no
/// parser, so there is no record to install). Its own input is *not* missing:
/// the affinity matrix is disc-parsed and live as `World::element_affinity`.
pub fn follow_up_roll_passes(inp: &FollowUpInputs) -> bool {
    if inp.roll_enabled == 0 {
        return true;
    }
    if inp.actor_element == ELEMENT_SKIP {
        return true;
    }
    if inp.rand % 5 == 0 {
        return true;
    }
    inp.affinity_pct >= AFFINITY_WEAK_MIN
}

/// The sibling of [`queued_magic_message`]: the routine that **installs** the
/// follow-up the guard then reads.
///
/// It opens on the identical preamble - resolve the acting actor, take its
/// queued action byte `+0x1DF`, find that action in the caster's spell-id
/// array and read the parallel level byte, bail below [`MIN_LEVEL`] - and then
/// runs the element-affinity roll before selecting a record out of
/// [`FOLLOW_UP_TABLE`] by `[actor_element][level band]`. The record's byte `0`
/// becomes the pending latch, its word `1` the routine pointer, and the hold
/// is always [`FOLLOW_UP_HOLD`]; the same message id [`MESSAGE_ID`] is printed
/// through `FUN_801D8DE8(0x66, 0)`.
///
/// PORT: FUN_801f3d3c
///
/// NOT WIRED, and the blocker is no longer the one [`queued_magic_message`]
/// had: the caster's spell record now reaches the SM
/// (`BattleActionHost::caster_spell_list`) and the latch is
/// `BattleActionCtx::follow_up_pending`. What remains missing is the
/// **record** this installs. [`FOLLOW_UP_TABLE`] (`0x801F6870`,
/// `[element][band]` 8-byte records) has no parser - `legaia_asset` extracts
/// the move-power and effect-aux regions of PROT 0898 but not this one - so
/// [`FollowUpHookRecord`] has no disc source and a caller could only install
/// a zeroed hook. The `0x801CFA2C` element jump-table arms this function
/// escapes into for elements `< 7` are separate bodies and are not dumped
/// either, so most elements would not reach the installer tail at all.
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
    if inp.actor_element < ELEMENT_JUMP_TABLE_LEN {
        return FollowUpOutcome::JumpTable(inp.actor_element);
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
/// overlay image at `[actor_element][band]`.
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
            actor_element: 9,
            other_element: 2,
            affinity_pct: 0x70,
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
    fn a_resistant_defender_suppresses_the_follow_up() {
        let mut inp = inputs();
        inp.affinity_pct = AFFINITY_WEAK_MIN - 1;
        assert!(!follow_up_roll_passes(&inp));
        inp.affinity_pct = AFFINITY_WEAK_MIN;
        assert!(follow_up_roll_passes(&inp));
    }

    #[test]
    fn the_roll_is_skipped_three_ways() {
        let mut inp = inputs();
        inp.affinity_pct = 0;
        // ctx[+0x287] clear.
        inp.roll_enabled = 0;
        assert!(follow_up_roll_passes(&inp));
        // The skip element.
        inp.roll_enabled = 1;
        inp.actor_element = ELEMENT_SKIP;
        assert!(follow_up_roll_passes(&inp));
        // One draw in five.
        inp.actor_element = 9;
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
    fn a_low_element_reaches_the_jump_table_instead_of_the_tail() {
        let (ids, levels) = lists(0x81, 5);
        let mut inp = inputs();
        inp.actor_element = 2;
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
