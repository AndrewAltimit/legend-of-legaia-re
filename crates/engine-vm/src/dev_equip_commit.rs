//! Dev-menu **equip commit** - the write half of the debug equipment editor.
//!
//! `FUN_801E5A08` in the field overlay (PROT 0897, base `0x801CE818`) is the
//! routine the dev menu's `EQUIP <char>` rows call once a bag id has been
//! picked. Its sibling [`crate::world_map_overlay`] already carries the
//! *browse* half (row model, stat aggregation, slot resolution); this module
//! is the commit that mutates the character record.
//!
//! The body reads, in order:
//!
//! 1. `FUN_80042EE0(item_id & 0xFF)` - locate the id in the bag. The miss
//!    sentinel is `0x100`, and on a miss the routine returns `0` having
//!    changed nothing.
//! 2. `FUN_80043048(bag_index, 1)` - consume one of that stack.
//! 3. Destination slot. When the caller's `slot` argument is `>= 4` the slot
//!    is `slot + 1` verbatim; otherwise it is resolved from the item's
//!    equipment-table record byte `+7` exactly as
//!    [`crate::world_map_overlay::resolve_equip_slot`] does (the item table
//!    `0x80074368 + id*0xC` byte `+1` indexes the equipment table
//!    `0x80074F68 + n*8`).
//! 4. The prior occupant of that slot, read from the character record at
//!    `0x80084140 + char*0x414 + 0x75E + slot` (= record `+0x196 + slot`,
//!    the equipment-slot bytes). A non-zero occupant is refunded to the bag
//!    with `FUN_800421D4(old, 1)`.
//! 5. The new id is stored, SFX cue `0x24` is played through `FUN_80035BD0`,
//!    and the routine returns `1`.
//!
//! The refund is unconditional on "non-zero", not on "was equipment": id `0`
//! is the empty-slot marker, so slot `0` holding item id `0` refunds nothing.
//!
//! `see ghidra/scripts/funcs/801e5a08.txt`

use crate::world_map_overlay::resolve_equip_slot;

/// The bag-scan miss sentinel `FUN_80042EE0` returns when an id is absent.
pub const BAG_MISS: u16 = 0x100;

/// SFX cue the commit plays on success (`FUN_80035BD0(0x24)`).
pub const EQUIP_SFX_CUE: u8 = 0x24;

/// The equipment-slot byte window inside a `0x414`-byte character record.
/// Retail addresses it as `0x80084140 + char*0x414 + 0x75E`, which is
/// `record + 0x196`.
pub const EQUIP_SLOT_BASE: usize = 0x196;

/// Number of equipment-slot bytes in a record (`+0x196..+0x19E`).
pub const EQUIP_SLOT_COUNT: usize = 8;

/// What one commit did, for a host that wants to mirror the retail side
/// effects (bag mutation + SFX) rather than have this module perform them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipCommit {
    /// Slot index within the record's equipment window that was written.
    pub slot: usize,
    /// Item id written into the slot.
    pub equipped: u8,
    /// The id displaced out of the slot, if it was occupied. Retail hands
    /// this back to the bag as one unit.
    pub refunded: Option<u8>,
}

/// The host bindings the commit needs, matching the three SCUS calls the
/// retail body makes. Every method is the engine's own bag, not a RAM poke.
pub trait EquipCommitHost {
    /// `FUN_80042EE0` - bag index of `item_id`, or [`BAG_MISS`] on a miss.
    fn find_in_bag(&self, item_id: u8) -> u16;
    /// `FUN_80043048` - remove `qty` from the stack at `bag_index`.
    fn take_from_bag(&mut self, bag_index: u16, qty: u8);
    /// `FUN_800421D4` - hand `qty` of `item_id` back to the bag.
    fn give_to_bag(&mut self, item_id: u8, qty: u8);
    /// `FUN_80035BD0` - play an SFX cue.
    fn play_sfx(&mut self, cue: u8);
}

/// Commit an equip onto one character's equipment-slot bytes.
///
/// `equip_slot_bits` supplies the item's equipment-table `+7` byte (the
/// caller resolves the item-table `+1` chain); `weapon_slot_table` is the
/// per-character weapon-slot table at `0x8007B42C`. `slot_arg` is the
/// caller's third argument: `>= 4` bypasses the table and targets
/// `slot_arg + 1`.
///
/// Returns `None` when the bag scan misses - the retail `return 0` path,
/// which leaves the record, the bag and the audio untouched.
///
/// PORT: FUN_801e5a08
/// REF: FUN_801E5B4C (the slot resolution reused from `world_map_overlay`)
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession`'s `EQUIP` row
/// confirm calls this against the engine's own bag (`WorldEquipHost`).
pub fn commit_equip<H: EquipCommitHost>(
    host: &mut H,
    record: &mut [u8],
    item_id: u8,
    char_idx: usize,
    slot_arg: i32,
    equip_slot_bits: u8,
    weapon_slot_table: &[i16],
) -> Option<EquipCommit> {
    let bag_index = host.find_in_bag(item_id);
    if bag_index == BAG_MISS {
        return None;
    }
    host.take_from_bag(bag_index, 1);

    let slot = if slot_arg >= 4 {
        (slot_arg as usize) + 1
    } else {
        resolve_equip_slot(equip_slot_bits, char_idx, weapon_slot_table)
    };
    if slot >= EQUIP_SLOT_COUNT {
        // Retail indexes the record unchecked; the port refuses to write
        // outside the eight-byte window rather than corrupt the neighbour
        // fields. The bag take above already happened, matching the order of
        // the retail body.
        return None;
    }
    let off = EQUIP_SLOT_BASE + slot;
    let prev = *record.get(off)?;
    let refunded = if prev != 0 {
        host.give_to_bag(prev, 1);
        Some(prev)
    } else {
        None
    };
    record[off] = item_id;
    host.play_sfx(EQUIP_SFX_CUE);
    Some(EquipCommit {
        slot,
        equipped: item_id,
        refunded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost {
        bag: Vec<(u8, u8)>,
        sfx: Vec<u8>,
        taken: Vec<(u16, u8)>,
    }

    impl EquipCommitHost for TestHost {
        fn find_in_bag(&self, item_id: u8) -> u16 {
            match self.bag.iter().position(|(id, _)| *id == item_id) {
                Some(i) => i as u16,
                None => BAG_MISS,
            }
        }
        fn take_from_bag(&mut self, bag_index: u16, qty: u8) {
            self.taken.push((bag_index, qty));
        }
        fn give_to_bag(&mut self, item_id: u8, qty: u8) {
            self.bag.push((item_id, qty));
        }
        fn play_sfx(&mut self, cue: u8) {
            self.sfx.push(cue);
        }
    }

    fn record() -> Vec<u8> {
        vec![0u8; 0x414]
    }

    #[test]
    fn bag_miss_changes_nothing() {
        let mut host = TestHost::default();
        let mut rec = record();
        let out = commit_equip(&mut host, &mut rec, 0x30, 0, 0, 0x00, &[2, 2, 2]);
        assert_eq!(out, None);
        assert!(host.taken.is_empty());
        assert!(host.sfx.is_empty());
        assert_eq!(rec[EQUIP_SLOT_BASE], 0);
    }

    #[test]
    fn body_armour_lands_in_slot_zero() {
        let mut host = TestHost {
            bag: vec![(0x30, 1)],
            ..Default::default()
        };
        let mut rec = record();
        // +7 bits 0x00 -> Body -> slot 0.
        let out = commit_equip(&mut host, &mut rec, 0x30, 0, 0, 0x00, &[2, 2, 2]).unwrap();
        assert_eq!(out.slot, 0);
        assert_eq!(out.refunded, None);
        assert_eq!(rec[EQUIP_SLOT_BASE], 0x30);
        assert_eq!(host.sfx, vec![EQUIP_SFX_CUE]);
        assert_eq!(host.taken, vec![(0, 1)]);
    }

    #[test]
    fn weapon_uses_the_per_character_slot_table() {
        let mut host = TestHost {
            bag: vec![(0x11, 1)],
            ..Default::default()
        };
        let mut rec = record();
        // +7 bits 0x40 -> Weapon -> weapon_slot_table[char].
        let out = commit_equip(&mut host, &mut rec, 0x11, 2, 0, 0x40, &[2, 3, 5]).unwrap();
        assert_eq!(out.slot, 5);
        assert_eq!(rec[EQUIP_SLOT_BASE + 5], 0x11);
    }

    #[test]
    fn footwear_lands_in_slot_four() {
        let mut host = TestHost {
            bag: vec![(0x40, 1)],
            ..Default::default()
        };
        let mut rec = record();
        let out = commit_equip(&mut host, &mut rec, 0x40, 0, 0, 0x60, &[2, 2, 2]).unwrap();
        assert_eq!(out.slot, 4);
    }

    #[test]
    fn slot_arg_at_or_above_four_bypasses_the_table() {
        let mut host = TestHost {
            bag: vec![(0x50, 1)],
            ..Default::default()
        };
        let mut rec = record();
        // slot_arg 4 -> slot 5, regardless of the +7 bits.
        let out = commit_equip(&mut host, &mut rec, 0x50, 0, 4, 0x40, &[2, 2, 2]).unwrap();
        assert_eq!(out.slot, 5);
    }

    #[test]
    fn occupied_slot_refunds_the_prior_id() {
        let mut host = TestHost {
            bag: vec![(0x30, 1)],
            ..Default::default()
        };
        let mut rec = record();
        rec[EQUIP_SLOT_BASE] = 0x2F;
        let out = commit_equip(&mut host, &mut rec, 0x30, 0, 0, 0x00, &[2, 2, 2]).unwrap();
        assert_eq!(out.refunded, Some(0x2F));
        assert!(host.bag.contains(&(0x2F, 1)));
        assert_eq!(rec[EQUIP_SLOT_BASE], 0x30);
    }

    #[test]
    fn empty_slot_marker_is_not_refunded() {
        let mut host = TestHost {
            bag: vec![(0x30, 1)],
            ..Default::default()
        };
        let mut rec = record();
        rec[EQUIP_SLOT_BASE] = 0;
        let out = commit_equip(&mut host, &mut rec, 0x30, 0, 0, 0x00, &[2, 2, 2]).unwrap();
        assert_eq!(out.refunded, None);
        assert_eq!(host.bag.len(), 1);
    }
}
