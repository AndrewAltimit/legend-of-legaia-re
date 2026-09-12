//! Battle **"No effect." banner pass** at `FUN_801F3C34` (PROT 0898, base
//! `0x801CE818`, file `0x2541C`) - the return-from-fade half of the
//! Seru-magic side-effect pair whose staging half is
//! [`crate::seru_side_effect`].
//!
//! A short pass over the acting battle actor's queued action byte
//! `actor[+0x1DF]`. When the cast spell is levelled enough to carry a side
//! effect (`>= 3`) and the stager left nothing pending, it installs the
//! **"No effect."** string (`0x801CFA20` in the overlay image) at the banner
//! pointer `0x800775B4`, fires the banner through `FUN_801D8DE8(0x66, 0)` and
//! mirrors the id into `ctx[+0x18]`. That is how a suppressed or
//! already-landed side effect tells the player it did not happen.
//!
//! ## Where it runs
//!
//! Its one caller is the battle-action SM itself: `FUN_801E295C` reaches
//! `jal 0x801f3c34` at `0x801E4CB8`, at the head of **state `0x36`**
//! (`Summon - return-from-fade`, ported as
//! [`crate::battle_action`]'s `SummonReturn`). The same SM body then runs the
//! 7-slot `+0x21C` / `+0x8` reset the port already carries, calls the
//! summon spell-XP check `FUN_801E70BC`, and clamps the side-effect hold
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
//!   if *(0x801F6960) != 0: return             // the stager left an effect pending
//!   *(0x800775B4) = 0x801CFA20                // "No effect." banner text
//!   FUN_801D8DE8(0x66, 0)
//!   ctx[+0x18] = 0x66
//! ```
//!
//! `0x80084140 + char*0x414 + 0x705` is character-record `+0x13D`, the
//! **spell-id array**; `+0x729` is record `+0x161`, its parallel level byte
//! (see `docs/formats/save-record.md` and `legaia_save::character::SpellList`).
//! So the scan is "find the queued action in this caster's learned-spell list
//! and read its level", and the message is emitted when that level is `>= 3`
//! and nothing was staged.
//!
//! The three action ids that early-out are exactly the Seru spells whose
//! summon modules never call the stager: `0x85` **Nighto** and `0x8E`
//! **Aluru** (byte-scan of PROT 0907 / 0916 for the `jal 0x801f3d3c` word
//! finds none, where the other nineteen modules carry one), and everything
//! from `0x96` up (the Ra-Seru spells, likewise stager-free). A cast that can
//! never have a side effect never prints "No effect.".
//!
//! Two things this body is **not**, both worth stating because the shape
//! invites the guess:
//!
//! * It is not a "move is unusable" reject - it changes no queue state and
//!   returns nothing. Its only effects are the message, the context byte and
//!   the installed banner pointer.
//! * The `>= 3` test is the *firing* condition, not a suppression: a level-1
//!   or level-2 spell takes the early return and prints nothing, because it
//!   never had an effect to miss.
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

/// Queued-action ids the pass skips outright: Nighto and Aluru, the two Seru
/// spells whose modules stage no side effect.
pub const SKIPPED_ACTIONS: [u8; 2] = [0x85, 0x8E];

/// Queued-action ids at or above this value are skipped (the Ra-Seru band).
pub const ACTION_CEILING: u8 = 0x96;

/// Number of spell-list entries the scan walks (retail caps at `0x20`, short
/// of the record's 36-entry array).
pub const SCAN_LIMIT: usize = 0x20;

/// Minimum spell level that lets the message fire - the same gate the stager
/// applies before staging anything.
pub const MIN_LEVEL: u8 = legaia_asset::seru_side_effect::MIN_LEVEL;

/// The banner id the pass prints and mirrors into `ctx[+0x18]`.
pub const MESSAGE_ID: u8 = 0x66;

/// Runtime VA of the "No effect." string the pass installs at `0x800775B4`.
pub const NO_EFFECT_TEXT_VA: u32 = 0x801C_FA20;

/// Index of the queued action inside the caster's spell-id array, or
/// [`SCAN_LIMIT`] when the scan runs off the end - retail keeps using the
/// out-of-range index to read the level array, which is long enough that the
/// read stays inside the record.
///
/// PORT: FUN_801f3c34 (`0x801F3C9C..0x801F3CDC`)
///
/// Live from the action SM's state `0x36` through [`queued_magic_message`];
/// the stager ([`crate::seru_side_effect::stage`]) shares the same scan.
pub fn spell_index_of(spell_ids: &[u8], action: u8) -> usize {
    for i in 0..SCAN_LIMIT {
        if spell_ids.get(i).copied() == Some(action) {
            return i;
        }
    }
    SCAN_LIMIT
}

/// Run the pass for one queued action.
///
/// `spell_ids` / `spell_levels` are the caster's record `+0x13D` and `+0x161`
/// arrays; `effect_pending` is the `*(0x801F6960) != 0` gate - the stager
/// left a side effect staged, so there is nothing to report as missed.
///
/// Returns the banner id to print, or `None` for any of the four early
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
/// The latch's **writer** is the stager ([`crate::seru_side_effect::stage`]),
/// which the live loop does not yet run (see that module's wiring note), so
/// in the port the latch is only ever read as clear: every levelled cast
/// prints "No effect." where retail would print the effect banner instead.
/// A one-branch difference, named here rather than papered over.
pub fn queued_magic_message(
    action: u8,
    spell_ids: &[u8],
    spell_levels: &[u8],
    effect_pending: bool,
) -> Option<u8> {
    if SKIPPED_ACTIONS.contains(&action) || action >= ACTION_CEILING {
        return None;
    }
    let idx = spell_index_of(spell_ids, action);
    let level = spell_levels.get(idx).copied().unwrap_or(0);
    if level < MIN_LEVEL {
        return None;
    }
    if effect_pending {
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
    fn a_pending_effect_suppresses_the_message() {
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
