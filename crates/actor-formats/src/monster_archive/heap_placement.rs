//! Where the battle heap puts each monster block, and what a **four-slot**
//! event list reads once it is there.
//!
//! The battle heap starts empty for every fight and its allocations run in a
//! fixed order (`docs/subsystems/battle.md` § The battle heap budget), so the
//! address a monster block lands at is a function of three things only: how
//! many party members are seated, which monster ids the formation names, and
//! each block's heap cost `block[+0x08]`.
//!
//! - The loader `FUN_800542C8` walks monster ids **upward** (outer loop on
//!   `s5`, `0x800543EC..0x800549F0`) and, for each id, the four formation
//!   seats `0x8007BD0C[s3]` (`0x80054414..0x800549D8`). The first seat
//!   naming the id streams and allocates its block; a later seat with the
//!   same id copies the pointer (`0x80054454..0x800544F0`). So blocks are
//!   allocated in ascending id order, whatever order the formation lists
//!   them in.
//! - Each block allocates `block[+0x08]` bytes (`lw s0,8(s1)` at
//!   `0x80054668`, `jal 0x80017888` at `0x8005468C`), and the best-fit heap
//!   carves it from the **top** of the one free node, so block `n` ends
//!   `0x0C` bytes (a node header) below block `n - 1`'s payload.
//! - The first block ends at [`SOLO_TOP`] with one party member seated, and
//!   [`PARTY_NODE_BYTES`] lower for each further seated member (each one's
//!   render node is a heap node of `0x9C` payload plus header, allocated
//!   before the blocks).
//!
//! The four-slot read: `FUN_80050E00` returns `entry + 0x13` when
//! `+0x11..+0x13` are all non-zero, and the anim tick reads
//! `entry[0x10 + ((entry + 0x13) & 0xFF)]` (`0x80047E28..0x80047E38`,
//! and `0x80047918` on the event-path commit). Read in RAM, which differs
//! from the decoded block at the words the loader relocates: every entry's
//! `+0x88` becomes the absolute address of its own `+0x8C` stream
//! (`addiu v0,v1,0x8c; sw v0,0x88(v1)` at `0x8005473C..0x80054740`), so a
//! read landing on `+0x8B` sees the address's high byte, `0x80`.
//!
//! Measured: 143 seat placements over 85 retail battle save states (one and
//! three seated members) and all 21 four-slot reads in them agree with this
//! law byte for byte. The two-member offset is the same per-node step,
//! unmeasured.

use std::collections::BTreeMap;

use super::{EVENT_FRAME_LIST_OFFSET, MIN_RECORD_BYTES};

/// End address of the first monster block with one party member seated.
pub const SOLO_TOP: u32 = 0x800B_9A50;
/// How much lower the first block ends for each further seated member: one
/// render node (`0x9C`) plus its 12-byte heap header.
pub const PARTY_NODE_BYTES: u32 = 0xA8;
/// The heap's per-node header, between one block's payload and the end of
/// the next block down.
pub const HEAP_NODE_HEADER: u32 = 0x0C;
/// Entry offset of the self-pointer the loader relocates (`+0x88` = the
/// absolute address of `+0x8C`).
const ENTRY_STREAM_POINTER: usize = 0x88;
/// Offset the relocated `+0x88` word points at.
const ENTRY_STREAM_OFFSET: u32 = 0x8C;

/// A block's heap cost, `block[+0x08]`: the bytes the loader allocates for
/// it (the texture pool past it is staged elsewhere). `None` for a block too
/// short to carry the header.
pub fn block_heap_bytes(block: &[u8]) -> Option<u32> {
    legaia_bytes::u32_le(block, 0x08)
}

/// The payload address of every distinct monster block a formation loads.
///
/// `seated_party` is the number of party members seated in the fight
/// (`1..=3`), `ids` the formation's monster ids (zeros and repeats allowed),
/// and `heap_bytes(id)` each block's [`block_heap_bytes`]. An id whose cost
/// is unknown stops the walk: every block below it would be placed wrong.
pub fn monster_block_bases(
    seated_party: usize,
    ids: &[u16],
    mut heap_bytes: impl FnMut(u16) -> Option<u32>,
) -> BTreeMap<u16, u32> {
    let mut distinct: Vec<u16> = ids.iter().copied().filter(|&id| id != 0).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let extra = seated_party.max(1) as u32 - 1;
    let mut top = SOLO_TOP - PARTY_NODE_BYTES * extra;
    let mut out = BTreeMap::new();
    for id in distinct {
        let Some(size) = heap_bytes(id) else {
            break;
        };
        let base = top.wrapping_sub(size);
        out.insert(id, base);
        top = base.wrapping_sub(HEAP_NODE_HEADER);
    }
    out
}

/// Byte `off` of a block loaded at `base`, as RAM holds it after the
/// loader's entry relocation: inside any entry's `+0x88` word, the byte of
/// that entry's absolute `+0x8C` address; elsewhere the decoded byte.
fn relocated_byte(block: &[u8], base: u32, off: usize) -> Option<u8> {
    let word = off & !3;
    let count = usize::from(*block.get(0x4A)?);
    for i in 0..count {
        let Some(entry) = legaia_bytes::u32_le(block, 0x4C + i * 4) else {
            break;
        };
        if entry as usize + ENTRY_STREAM_POINTER == word {
            let addr = base.wrapping_add(entry).wrapping_add(ENTRY_STREAM_OFFSET);
            return Some(addr.to_le_bytes()[off & 3]);
        }
    }
    block.get(off).copied()
}

/// The beat a **four-slot** event list resolves to with its block loaded at
/// `base`: `entry[0x10 + ((base + entry_off + 0x13) & 0xFF)]`, read through
/// the loader's relocation. `None` when the entry's list is not a four-slot
/// one (its beat is plainly `entry[0x10]`) or the read falls off the block.
pub fn four_slot_beat(block: &[u8], entry_off: usize, base: u32) -> Option<u8> {
    let list =
        block.get(entry_off + EVENT_FRAME_LIST_OFFSET..entry_off + EVENT_FRAME_LIST_OFFSET + 4)?;
    if list[1..].contains(&0) {
        return None;
    }
    let addr = base.wrapping_add(entry_off as u32);
    let idx = (addr.wrapping_add(0x13) & 0xFF) as usize;
    relocated_byte(block, base, entry_off + EVENT_FRAME_LIST_OFFSET + idx)
}

/// [`four_slot_beat`] for every entry of a block's `+0x4C` action table,
/// index-aligned with
/// [`animations_by_entry`](super::animations_by_entry): `None` for an entry
/// whose list is not four-slot.
pub fn four_slot_beats(block: &[u8], base: u32) -> Vec<Option<u8>> {
    if block.len() < MIN_RECORD_BYTES {
        return Vec::new();
    }
    let count = usize::from(block[0x4A]);
    (0..count)
        .map(|i| {
            legaia_bytes::u32_le(block, 0x4C + i * 4)
                .and_then(|off| four_slot_beat(block, off as usize, base))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_stack_down_from_the_seated_top_in_ascending_id_order() {
        // Formation [114, 89]: id 89 loads first (ascending), whatever the
        // seat order - as `battle_korb2_mage_strike` holds it.
        let sizes = |id: u16| match id {
            89 => Some(0xC4A0),
            114 => Some(0xAA50),
            _ => None,
        };
        let b = monster_block_bases(3, &[114, 89, 0, 0], sizes);
        assert_eq!(b[&89], 0x800B_9900 - 0xC4A0);
        assert_eq!(b[&89], 0x800A_D460);
        assert_eq!(b[&114], 0x800A_D460 - HEAP_NODE_HEADER - 0xAA50);
        assert_eq!(b[&114], 0x800A_2A04);
        // One seated member: the top is two render nodes higher.
        let solo = monster_block_bases(1, &[4], |_| Some(0xC558));
        assert_eq!(solo[&4], 0x800A_D4F8);
    }

    #[test]
    fn repeated_ids_take_one_block() {
        let b = monster_block_bases(3, &[141, 141, 150, 0], |_| Some(0x100));
        assert_eq!(b.len(), 2);
        assert_eq!(b[&141], 0x800B_9900 - 0x100);
    }

    /// A block with one entry at `0x60` whose list is four-slot.
    fn one_entry_block() -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0x4A] = 1;
        b[0x4C..0x50].copy_from_slice(&0x60u32.to_le_bytes());
        b[0x70..0x74].copy_from_slice(&[16, 22, 28, 34]);
        b
    }

    #[test]
    fn the_read_index_is_the_low_byte_of_the_list_tail_address() {
        let mut b = one_entry_block();
        // base + 0x60 + 0x13 = ..0x8F -> entry[0x10 + 0x8F] = block[0xFF].
        b[0xFF] = 209;
        assert_eq!(
            four_slot_beat(&b, 0x60, 0x800B_4200 - 0x60 + 0x7C),
            Some(209)
        );
    }

    #[test]
    fn a_read_on_the_relocated_stream_pointer_sees_the_ram_high_byte() {
        let b = one_entry_block();
        // idx 0x7B lands on entry + 0x8B, the high byte of the relocated
        // `+0x88` word: (base + 0x60 + 0x13) & 0xFF == 0x7B.
        let base = 0x800B_7F68 - 0x60;
        assert_eq!(four_slot_beat(&b, 0x60, base), Some(0x80));
    }

    #[test]
    fn a_list_with_a_zero_is_not_four_slot() {
        let mut b = one_entry_block();
        b[0x73] = 0;
        assert_eq!(four_slot_beat(&b, 0x60, 0x800A_0000), None);
        assert_eq!(four_slot_beats(&b, 0x800A_0000), vec![None]);
    }
}
