//! Starting-bag expansion via field-VM `GIVE_ITEM` injection.
//!
//! The direct new-game inventory seed (rewriting `FUN_80034A6C`'s reclaimable
//! code, see [`crate::starting_items`]) is hard-capped at 7 `(id, count)` slots —
//! the executable has no safe code cave and can't grow within the same-size patch
//! model. To seed an arbitrarily large starting bag (all the convenience items
//! **plus** the full requested random count), this module instead grants items the
//! way a treasure chest does: a run of silent `GIVE_ITEM` field-VM ops (`0x39`,
//! `[0x39, id]`, the same op the [chest randomizer](crate::chest) rewrites — the
//! "found X!" text is a *separate* `0xC2` token, so a bare `0x39` is a silent add).
//! A 10× consumable is just ten `0x39 <id>` ops; the bag stacks by id.
//!
//! Because the ops live in a scene's event script (which re-runs every time the
//! scene loads), they are wrapped in a **once-only guard** keyed on a persistent
//! story flag: test the flag, skip the whole block if it is already set, otherwise
//! grant the bag and set the flag. The flag must live in the **saved** SC
//! story-flag bitfield at `0x80085758` (where `--all-warps` writes), reached by the
//! extended flag ops `0x50` (SET) / `0x70` (TEST) — *not* the cheap `0x2E`/`0x30`
//! ops, which target the per-scene-reloaded scratchpad word `_DAT_1F800394` and so
//! would not persist across a save/reload (the bag would re-grant). See
//! `docs/subsystems/script-vm.md` (flag banks) and `docs/formats/new-game-table.md`.
//!
//! This module only **emits** the guarded grant block (the bytecode). Inserting it
//! into the opening scene's MAN (with the partition-table + jump-delta fixups a
//! variable-length insert needs) and recompressing is the injector's job; the block
//! is position-independent (its one internal jump is a relative skip), so it can be
//! spliced at any instruction boundary.

/// Field-VM `GIVE_ITEM` opcode (`[0x39, item_id]`, silent single-item add).
pub const GIVE_ITEM_OP: u8 = 0x39;

/// Base opcode of the persistent system-flag **SET** op (`0x5x`). The flag index
/// is split across the opcode's low bits and the operand byte:
/// `idx = ((op & 0x8F) << 8) | operand` (see `field_disasm` `0x50..=0x77`).
pub const SYSFLAG_SET_BASE: u8 = 0x50;

/// Base opcode of the persistent system-flag **TEST** op (`0x7x`). When the bit is
/// set the VM jumps by the trailing `u16` delta (`[0x70, operand, dlo, dhi]`,
/// target `= pc + 2 + delta`); when clear it falls through.
pub const SYSFLAG_TEST_BASE: u8 = 0x70;

/// Default persistent story-flag bit for the grant guard. Chosen from the high end
/// of the saved SC bitfield (`0x80085758`): bit `0xD70` lands at SC `+0x17C6`, which
/// is inside the new-game-zeroed / card-saved SC block yet reads zero across every
/// near-complete retail save (so it is very likely an unused flag). **It is not
/// proven unused at runtime** — boot-validate before trusting it; exposed as a knob
/// so it can be moved if it collides.
pub const DEFAULT_GUARD_BIT: u16 = 0xD70;

/// Largest flag index the `0x50`/`0x70` encoding addresses with a single
/// low-nibble opcode (`idx = ((op & 0x0F) << 8) | operand`, opcodes `0x50..=0x5F` /
/// `0x70..=0x7F`). Higher indices would need the `0x80` opcode bit; the guard stays
/// within this range.
pub const MAX_GUARD_BIT: u16 = 0x0FFF;

/// Encode the system-flag **SET** op for `bit` (`[op, operand]`, 2 bytes).
pub fn sysflag_set(bit: u16) -> [u8; 2] {
    let op = SYSFLAG_SET_BASE | ((bit >> 8) as u8 & 0x0F);
    [op, (bit & 0xFF) as u8]
}

/// Encode the system-flag **TEST**-and-skip op for `bit` (`[op, operand, dlo, dhi]`,
/// 4 bytes). `delta` is the relative jump applied when the bit is set; the VM
/// computes the target as `pc + 2 + delta` (`pc + header_size(1) + 1 + delta`).
pub fn sysflag_test(bit: u16, delta: u16) -> [u8; 4] {
    let op = SYSFLAG_TEST_BASE | ((bit >> 8) as u8 & 0x0F);
    let d = delta.to_le_bytes();
    [op, (bit & 0xFF) as u8, d[0], d[1]]
}

/// Total byte length of the `GIVE_ITEM` run for `items`: two bytes per granted
/// unit (`count` × `[0x39, id]`), summing the counts. Items with `count == 0` are
/// skipped.
pub fn gives_len(items: &[(u8, u8)]) -> usize {
    items.iter().map(|&(_, c)| c as usize * 2).sum()
}

/// Emit the guarded grant block for `items`, keyed on persistent story-flag
/// `guard_bit`:
///
/// ```text
/// 70 <bit> <delta>   ; if guard already set, skip to end of block
/// 39 <id>            ; one per granted unit (count x per item)
/// ...
/// 50 <bit>           ; mark the bag granted
/// ```
///
/// The block is position-independent: the test op's `delta` is computed so the
/// skip lands exactly on the byte *after* the block, i.e. on whatever instruction
/// the block was spliced in front of. Items are emitted in slice order; a
/// `count == 0` item contributes nothing.
///
/// Panics if `guard_bit > MAX_GUARD_BIT` (callers pass a constant) or if the gives
/// run does not fit a `u16` skip delta (a bag of ~16k units — far past any real
/// use; the injector caps the bag well below this).
pub fn guarded_grant_block(items: &[(u8, u8)], guard_bit: u16) -> Vec<u8> {
    assert!(
        guard_bit <= MAX_GUARD_BIT,
        "guard bit {guard_bit:#x} exceeds the single-opcode index range {MAX_GUARD_BIT:#x}"
    );
    let gives = gives_len(items);
    // Skip target = end of block (after the gives + the 2-byte SET). The VM resolves
    // the target as pc + 2 + delta from the TEST op, so delta = block_len - 2 - 0
    // measured from the TEST op at the block start: 4 (test) + gives + 2 (set) - 2.
    let delta = u16::try_from(4 + gives).expect("grant block too large for a u16 skip delta");

    let mut out = Vec::with_capacity(4 + gives + 2);
    out.extend_from_slice(&sysflag_test(guard_bit, delta));
    for &(id, count) in items {
        for _ in 0..count {
            out.push(GIVE_ITEM_OP);
            out.push(id);
        }
    }
    out.extend_from_slice(&sysflag_set(guard_bit));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::field_disasm::{self, FlagKind, InsnInfo};

    #[test]
    fn flag_ops_encode_the_index_split() {
        // idx = ((op & 0x0F) << 8) | operand for the 0x5x / 0x7x banks.
        assert_eq!(sysflag_set(0xD70), [0x5D, 0x70]);
        assert_eq!(sysflag_test(0xD70, 0x1234), [0x7D, 0x70, 0x34, 0x12]);
        assert_eq!(sysflag_set(0x005), [0x50, 0x05]);
        assert_eq!(
            sysflag_test(0x105, 0).to_vec(),
            vec![0x71, 0x05, 0x00, 0x00]
        );
    }

    /// The whole block must round-trip through the real field-VM disassembler:
    /// the guard TEST decodes with the right index and a skip target landing
    /// exactly at the end of the block, then `n` GIVE_ITEM ops, then the SET.
    #[test]
    fn block_round_trips_through_the_disassembler() {
        let items = [(0x89u8, 10u8), (0x8au8, 10u8), (0xd1u8, 1u8), (0x77u8, 5u8)];
        let bit = DEFAULT_GUARD_BIT;
        let block = guarded_grant_block(&items, bit);
        let total_units: usize = items.iter().map(|&(_, c)| c as usize).sum();
        assert_eq!(block.len(), 4 + total_units * 2 + 2);

        // 1) guard TEST at pc 0 → idx == bit, target == end of block (skip past).
        let test = field_disasm::decode(&block, 0).expect("decode test");
        match test.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(target),
                ..
            } => {
                assert_eq!(idx, bit, "guard tests the requested bit");
                assert_eq!(target, block.len(), "skip lands exactly after the block");
            }
            other => panic!("expected SystemFlag Test, got {other:?}"),
        }
        assert_eq!(test.size, 4);

        // 2) the gives, in order, one GiveItem per unit.
        let mut pc = test.size;
        let mut decoded_units = Vec::new();
        for &(id, count) in &items {
            for _ in 0..count {
                let insn = field_disasm::decode(&block, pc).expect("decode give");
                match insn.info {
                    InsnInfo::GiveItem { item_id } => decoded_units.push(item_id),
                    other => panic!("expected GiveItem at {pc:#x}, got {other:?}"),
                }
                assert_eq!(insn.size, 2);
                pc += insn.size;
            }
            let _ = id;
        }
        let expected: Vec<u8> = items
            .iter()
            .flat_map(|&(id, count)| std::iter::repeat_n(id, count as usize))
            .collect();
        assert_eq!(decoded_units, expected);

        // 3) the closing SET at the block tail.
        let set = field_disasm::decode(&block, pc).expect("decode set");
        match set.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                delta: None,
                ..
            } => assert_eq!(idx, bit, "guard set marks the same bit"),
            other => panic!("expected SystemFlag Set, got {other:?}"),
        }
        assert_eq!(pc + set.size, block.len(), "SET is the last instruction");
    }

    #[test]
    fn empty_bag_is_just_the_guard() {
        // No items: test (skip 4) then set, an inert once-only marker.
        let block = guarded_grant_block(&[], 0x10);
        assert_eq!(block.len(), 6);
        let test = field_disasm::decode(&block, 0).expect("decode");
        match test.info {
            InsnInfo::SystemFlag {
                target: Some(t), ..
            } => assert_eq!(t, block.len()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn zero_count_items_emit_no_gives() {
        let block = guarded_grant_block(&[(0x80, 0), (0x81, 2)], 0x10);
        // Only the 2-unit item contributes: 4 (test) + 2*2 + 2 (set).
        assert_eq!(block.len(), 4 + 4 + 2);
    }
}
