//! Arms-command execution-time weapon fold: the per-command equipment read
//! inside the Arms execution resolver `FUN_801EC3E4` (battle overlay `0898`).
//!
//! This is the half of the equipment story that battle-load deliberately
//! leaves out. [`super::stat_init::equip_stat_bonuses`] records the trap: the
//! battle-load seeder `FUN_80053CB8` folds the equipment table's UDF (`+2`),
//! LDF (`+3`) and SPD (`+4`) bytes and folds **neither** the INT (`+0`) nor the
//! ATK (`+1`) byte, so a weapon's attack bonus never reaches the actor's ATK
//! base (`+0x15A`) at battle load. It reaches ATK *working* (`+0x158`) here
//! instead, at execution time, once per committed arms command - which is why
//! the seeder skipping it is correct rather than an omission.
//!
//! ## Where the numbers come from
//!
//! The resolver dispatches on the actor's current command byte (`+0x1D9`)
//! through the six-arm jump table at `PTR_801CF4B4`, bounds-checked with
//! `(command - 0x0C) < 6`. Each arm resolves one or more of the character
//! record's five equipment slots (`+0x196..+0x19B`) into an attack bonus by
//! the same two-hop lookup the menu aggregator uses - item property record
//! `DAT_80074368 + id*0xC` byte `+1` selects a row in the equipment stat table
//! `DAT_80074F68 + row*8`, whose byte `+1` is the attack bonus
//! (`legaia_asset::equip_stats`) - then folds it into ATK working:
//!
//! | command | equipment slots read | record offsets | fold |
//! |---|---|---|---|
//! | `0x0C` | 2 | `+0x198` | `atk[2] >> 1` |
//! | `0x0D` | 3 | `+0x199` | `atk[3] >> 1` |
//! | `0x0E` | 4 | `+0x19A` | `atk[4] >> 1` |
//! | `0x0F` | 4 | `+0x19A` | `atk[4] >> 1` |
//! | `0x10` | none | - | no fold |
//! | `0x11` | 0,1,2,3,4 | `+0x196..+0x19B` | `(sum of all five) >> 1` |
//!
//! Commands `0x0E` and `0x0F` share one jump-table arm (table slots `[2]` and
//! `[3]` hold the same target), and command `0x10`'s arm is the same address
//! the bounds check bails to, so it is a live table slot that folds nothing.
//!
//! Retail applies **no** empty-slot test and **no** `kind == 1` item-class
//! guard on this path, exactly as [`super::stat_init::equip_stat_bonuses`]
//! notes for battle-load: an id of `0` is looked up like any other. Callers
//! therefore pass the resolved bonus byte for every slot, not `Option`.
//!
//! The four single-slot arms shift with `srl` and the five-slot arm with
//! `sra`. Both operate on non-negative values here (a sum of five `u8`s), so
//! the results agree; the port uses one unsigned shift.

use super::stat_init::EQUIP_SLOTS;

/// Lowest command byte the resolver's jump table covers (`+0x1D9` values below
/// this fall out of the `(command - 0x0C) < 6` bounds check).
pub const ARMS_COMMAND_BASE: u8 = 0x0C;

/// Number of arms in the `PTR_801CF4B4` dispatch table.
pub const ARMS_COMMAND_ARMS: u8 = 6;

/// Highest command byte the **admission** gate at the resolver's head accepts.
///
/// The head gate and the dispatch bound are deliberately different widths and
/// read from different places: admission tests the caller's command-record
/// byte with `(cmd - 0x0C) < 0x14` (i.e. `0x0C..=0x1F`), while dispatch tests
/// the *actor's* `+0x1D9` byte with `(cmd - 0x0C) < 6` (`0x0C..=0x11`). A
/// command in `0x12..=0x1F` is admitted and then folds nothing.
pub const ARMS_ADMIT_SPAN: u8 = 0x14;

/// Action-state (`ctx[7]`) value that makes the resolver return immediately.
pub const ARMS_BLOCKED_ACTION_STATE: u8 = 0x5A;

/// Highest actor slot on the player dispatch path. Slots at or above this take
/// the enemy branch, which does not perform the equipment fold.
pub const ARMS_PLAYER_SLOT_LIMIT: u8 = 3;

/// Maximum value of the actor's input cursor (`+0x1F4`) the head gate accepts.
pub const ARMS_CURSOR_LIMIT: u8 = 4;

/// Which equipment slots the given arms command folds, or `None` when the
/// command is outside the six-arm dispatch table.
///
/// An empty slice is a live arm that folds nothing (command `0x10`).
///
/// PORT: FUN_801EC3E4 (the `PTR_801CF4B4` dispatch arms)
pub fn arms_command_equip_slots(command: u8) -> Option<&'static [u8]> {
    let arm = command.wrapping_sub(ARMS_COMMAND_BASE);
    if arm >= ARMS_COMMAND_ARMS {
        return None;
    }
    Some(match arm {
        0 => &[2],
        1 => &[3],
        2 | 3 => &[4],
        4 => &[],
        _ => &[0, 1, 2, 3, 4],
    })
}

/// The execution-time weapon fold: the ATK-working delta an arms command adds.
///
/// `atk_bonuses[i]` is the attack bonus byte (`DAT_80074F68 + row*8`, `+1`)
/// already resolved for equipment slot `i` - see the module docs for the
/// two-hop lookup that produces it. Returns `None` for a command outside the
/// dispatch table, and `Some(0)` for the live-but-empty arm `0x10`.
///
/// PORT: FUN_801EC3E4 (the ATK-working fold at `+0x158`)
pub fn arms_weapon_atk_fold(command: u8, atk_bonuses: &[u8; EQUIP_SLOTS]) -> Option<u16> {
    let slots = arms_command_equip_slots(command)?;
    let sum: u16 = slots
        .iter()
        .map(|&s| u16::from(atk_bonuses[usize::from(s)]))
        .sum();
    Some(sum >> 1)
}

/// The resolver's head guard chain: whether this call reaches the dispatch at
/// all.
///
/// Mirrors the tests in `FUN_801EC3E4`'s prologue, in order:
///
/// 1. `ctx[7] != 0x5A` - the action-state gate.
/// 2. `(record_command - 0x0C) < 0x14` - the admission band, read from the
///    caller's command record, **not** from the actor.
/// 3. `step_index + 1 >= step_count` - the caller's step must be the last one
///    the record declares (`record[cursor + 0x10]`).
/// 4. `step_count != 0`.
/// 5. `input_cursor < 4` - the actor's `+0x1F4` cursor.
/// 6. `slot < 3` - the player branch; higher slots take the enemy path, which
///    skips the equipment fold entirely.
///
/// PORT: FUN_801EC3E4 (head guard chain)
pub fn arms_resolver_admits(
    action_state: u8,
    record_command: u8,
    step_index: u8,
    step_count: u8,
    input_cursor: u8,
    slot: u8,
) -> bool {
    if action_state == ARMS_BLOCKED_ACTION_STATE {
        return false;
    }
    if record_command.wrapping_sub(ARMS_COMMAND_BASE) >= ARMS_ADMIT_SPAN {
        return false;
    }
    if u16::from(step_index) + 1 < u16::from(step_count) {
        return false;
    }
    if step_count == 0 {
        return false;
    }
    if input_cursor >= ARMS_CURSOR_LIMIT {
        return false;
    }
    slot < ARMS_PLAYER_SLOT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_table_matches_the_six_retail_arms() {
        // PTR_801CF4B4 = [801ecbc4, 801ecc0c, 801ecc54, 801ecc54, 801ecde4,
        // 801eccd0]: slots [2] and [3] share an arm, and [4] is the bail
        // target, so it folds nothing.
        assert_eq!(arms_command_equip_slots(0x0C), Some(&[2u8][..]));
        assert_eq!(arms_command_equip_slots(0x0D), Some(&[3u8][..]));
        assert_eq!(arms_command_equip_slots(0x0E), Some(&[4u8][..]));
        assert_eq!(arms_command_equip_slots(0x0F), Some(&[4u8][..]));
        assert_eq!(arms_command_equip_slots(0x10), Some(&[][..]));
        assert_eq!(arms_command_equip_slots(0x11), Some(&[0u8, 1, 2, 3, 4][..]));
    }

    #[test]
    fn commands_outside_the_table_do_not_dispatch() {
        for cmd in [0x00u8, 0x0B, 0x12, 0x1F, 0x20, 0xFF] {
            assert_eq!(arms_command_equip_slots(cmd), None, "command {cmd:#04x}");
            assert_eq!(arms_weapon_atk_fold(cmd, &[9; EQUIP_SLOTS]), None);
        }
    }

    #[test]
    fn single_slot_arms_fold_half_their_slot() {
        let atk = [10u8, 20, 30, 40, 50];
        assert_eq!(arms_weapon_atk_fold(0x0C, &atk), Some(15)); // slot 2
        assert_eq!(arms_weapon_atk_fold(0x0D, &atk), Some(20)); // slot 3
        assert_eq!(arms_weapon_atk_fold(0x0E, &atk), Some(25)); // slot 4
        assert_eq!(arms_weapon_atk_fold(0x0F, &atk), Some(25)); // slot 4
        assert_eq!(arms_weapon_atk_fold(0x10, &atk), Some(0)); // live, empty
    }

    #[test]
    fn arm_0x11_folds_half_the_sum_of_all_five_slots() {
        let atk = [10u8, 20, 30, 40, 50];
        assert_eq!(arms_weapon_atk_fold(0x11, &atk), Some(75));
        // The shift is applied to the sum, not per slot: five odd bonuses
        // keep their halves rather than each truncating away.
        assert_eq!(arms_weapon_atk_fold(0x11, &[1; EQUIP_SLOTS]), Some(2));
    }

    #[test]
    fn the_shift_truncates_like_retail() {
        // srl 1 on an odd byte drops the low bit.
        assert_eq!(arms_weapon_atk_fold(0x0C, &[0, 0, 7, 0, 0]), Some(3));
        // The widest byte cannot overflow the u16 accumulator.
        assert_eq!(arms_weapon_atk_fold(0x11, &[0xFF; EQUIP_SLOTS]), Some(637));
    }

    #[test]
    fn head_guards_admit_the_nominal_call() {
        assert!(arms_resolver_admits(0x00, 0x0C, 0, 1, 0, 0));
    }

    #[test]
    fn head_guards_reject_each_failing_condition() {
        // 1. action-state gate
        assert!(!arms_resolver_admits(0x5A, 0x0C, 0, 1, 0, 0));
        // 2. admission band is 0x0C..=0x1F on the record byte
        assert!(!arms_resolver_admits(0x00, 0x0B, 0, 1, 0, 0));
        assert!(!arms_resolver_admits(0x00, 0x20, 0, 1, 0, 0));
        assert!(arms_resolver_admits(0x00, 0x1F, 0, 1, 0, 0));
        // 3. step must be the record's last
        assert!(!arms_resolver_admits(0x00, 0x0C, 0, 3, 0, 0));
        assert!(arms_resolver_admits(0x00, 0x0C, 2, 3, 0, 0));
        // 4. zero-step records bail
        assert!(!arms_resolver_admits(0x00, 0x0C, 0, 0, 0, 0));
        // 5. cursor bound
        assert!(!arms_resolver_admits(0x00, 0x0C, 0, 1, 4, 0));
        assert!(arms_resolver_admits(0x00, 0x0C, 0, 1, 3, 0));
        // 6. player slots only
        assert!(!arms_resolver_admits(0x00, 0x0C, 0, 1, 0, 3));
        assert!(arms_resolver_admits(0x00, 0x0C, 0, 1, 0, 2));
    }

    #[test]
    fn admission_and_dispatch_bands_are_different_widths() {
        // A command in 0x12..=0x1F is admitted by the head gate but folds
        // nothing, because dispatch bounds at 6 arms rather than 0x14.
        for cmd in 0x12u8..=0x1F {
            assert!(arms_resolver_admits(0x00, cmd, 0, 1, 0, 0));
            assert_eq!(arms_command_equip_slots(cmd), None);
        }
    }
}
