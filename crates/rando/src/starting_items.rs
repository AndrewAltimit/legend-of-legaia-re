//! Starting-inventory randomization: replace the new game's fixed Healing Leaf
//! with up to five random consumables.
//!
//! A vanilla New Game seeds exactly one inventory slot — Healing Leaf (item id
//! `0x77`) ×5 — written in code by `FUN_80034A6C`
//! (see [`legaia_asset::new_game::StartingInventory`] and
//! `docs/formats/new-game-table.md`). There is no static starting-inventory
//! table to edit, so this randomizer rewrites the **seed code** itself: the
//! 40-byte reclaimable region at [`legaia_asset::new_game::STARTING_INV_SEED_VA`]
//! (the original `li`/`sb` seed plus the redundant inline zero-loop the callers
//! already cover with their `SC`-block `memset`).
//!
//! Each item is written with a single packed **halfword store** — an inventory
//! slot is two contiguous bytes `[id][count]`, so `sh $v0` writes both at once
//! after `addiu $v0, $zero, (count << 8) | id`. That is two instructions per
//! item, and the reclaimable region is ten instructions, so the seed holds at
//! most [`MAX_STARTING_ITEMS`] slots. The patch is the same size as the
//! original code (no executable growth or relocation) and is applied through
//! [`crate::disc::DiscPatcher::patch_named_file`] like the steal table.
//!
//! Because the write lands **directly** in the consumable inventory page
//! (bypassing the engine's id-routing add primitive), the random pool is
//! restricted to the contiguous block of genuine consumables
//! ([`STARTING_ITEM_POOL`], Healing Leaf .. Wonder Elixir) so a starting item
//! is always something that belongs on that page.

use legaia_asset::new_game::{INVENTORY_SC_OFFSET, STARTING_INV_SEED_LEN};

use crate::rng::SplitMix64;

/// Most starting-item slots the reclaimable seed region can hold: ten
/// instructions / two per item (`addiu` + `sh`).
pub const MAX_STARTING_ITEMS: usize = STARTING_INV_SEED_LEN / 4 / 2;

/// Default `(min, max)` random count for each seeded item (inclusive). Modest,
/// so a random start is helpful without trivializing the early game; vanilla
/// seeds five Healing Leaves.
pub const DEFAULT_COUNT_RANGE: (u8, u8) = (1, 5);

/// The consumable item-id pool starting items are drawn from: the contiguous
/// block from Healing Leaf (`0x77`) through Wonder Elixir (`0x8e`) in the
/// retail item-name table — restoratives, cures, Phoenix, Waters, Doors,
/// Incense, and Elixirs. Everything below `0x77` is equipment and everything
/// above `0x8e` is magic books / quest / key items, none of which belong on the
/// consumable inventory page this seed writes to directly.
pub const STARTING_ITEM_POOL: &[u8] = &[
    0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86,
    0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
];

// MIPS encodings for the three instructions the seed uses; see
// `docs/formats/new-game-table.md`. `$v0` = rt 2, `$s0` (= SC base) = base 16.
const NOP: u32 = 0x0000_0000;
/// `addiu $v0, $zero, imm16`.
fn addiu_v0(imm: u16) -> u32 {
    0x2402_0000 | imm as u32
}
/// `sh $v0, off($s0)`.
fn sh_v0_s0(off: u16) -> u32 {
    0xA602_0000 | off as u32
}

/// Plan `n` random starting items from `seed`: `n` distinct ids drawn from
/// [`STARTING_ITEM_POOL`], each with a random count in [`DEFAULT_COUNT_RANGE`].
/// `n` is clamped to `0..=MAX_STARTING_ITEMS` and to the pool size. Deterministic
/// in `(seed, n)`.
pub fn plan_starting_items(seed: u64, n: usize) -> Vec<(u8, u8)> {
    let n = n.min(MAX_STARTING_ITEMS).min(STARTING_ITEM_POOL.len());
    let mut rng = SplitMix64::new(seed ^ 0x5247_4E49_5453_5452); // "RGNITSTR"-ish salt
    let mut pool = STARTING_ITEM_POOL.to_vec();
    rng.shuffle(&mut pool);
    let (lo, hi) = DEFAULT_COUNT_RANGE;
    let span = (hi - lo) as usize + 1;
    pool.into_iter()
        .take(n)
        .map(|id| (id, lo + rng.below(span) as u8))
        .collect()
}

/// Encode a list of `(id, count)` starting items into the 40-byte seed patch.
///
/// Emits one `addiu $v0, $zero, (count << 8) | id` + `sh $v0, (0x1818 + 2k)($s0)`
/// pair per item, then pads to [`STARTING_INV_SEED_LEN`] with `nop`. Panics if
/// more than [`MAX_STARTING_ITEMS`] items are passed (callers clamp first). The
/// inventory base offset comes from [`INVENTORY_SC_OFFSET`].
pub fn build_seed_patch(items: &[(u8, u8)]) -> [u8; STARTING_INV_SEED_LEN] {
    assert!(
        items.len() <= MAX_STARTING_ITEMS,
        "at most {MAX_STARTING_ITEMS} starting items fit the seed region"
    );
    let mut words: Vec<u32> = Vec::with_capacity(STARTING_INV_SEED_LEN / 4);
    for (slot, &(id, count)) in items.iter().enumerate() {
        let off = (INVENTORY_SC_OFFSET as usize + slot * 2) as u16;
        words.push(addiu_v0(((count as u16) << 8) | id as u16));
        words.push(sh_v0_s0(off));
    }
    while words.len() < STARTING_INV_SEED_LEN / 4 {
        words.push(NOP);
    }
    let mut out = [0u8; STARTING_INV_SEED_LEN];
    for (i, w) in words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::new_game::StartingInventory;

    #[test]
    fn pool_is_the_consumable_block() {
        assert_eq!(STARTING_ITEM_POOL.first(), Some(&0x77)); // Healing Leaf
        assert_eq!(STARTING_ITEM_POOL.last(), Some(&0x8e)); // Wonder Elixir
        assert_eq!(STARTING_ITEM_POOL.len(), 0x8e - 0x77 + 1);
        // Contiguous, ascending, no equipment / key items.
        for w in STARTING_ITEM_POOL.windows(2) {
            assert_eq!(w[1], w[0] + 1);
        }
    }

    #[test]
    fn max_items_is_five() {
        assert_eq!(MAX_STARTING_ITEMS, 5);
    }

    #[test]
    fn plan_gives_distinct_ids_in_pool_with_valid_counts() {
        let items = plan_starting_items(0xC0FFEE, 5);
        assert_eq!(items.len(), 5);
        let mut ids: Vec<u8> = items.iter().map(|i| i.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5, "ids are distinct");
        for (id, count) in items {
            assert!(STARTING_ITEM_POOL.contains(&id));
            assert!((DEFAULT_COUNT_RANGE.0..=DEFAULT_COUNT_RANGE.1).contains(&count));
        }
    }

    #[test]
    fn plan_clamps_to_max_and_is_deterministic() {
        let a = plan_starting_items(42, 99);
        let b = plan_starting_items(42, 99);
        assert_eq!(a, b, "deterministic for a fixed seed");
        assert_eq!(
            a.len(),
            MAX_STARTING_ITEMS,
            "clamped to the region capacity"
        );
        assert!(plan_starting_items(42, 0).is_empty());
    }

    #[test]
    fn build_patch_round_trips_through_the_decoder() {
        // The encoder + the asset-side decoder must agree: build a patch, decode
        // it back, get the same slots. This is the contract the disc round-trip
        // and the runtime oracle both rely on.
        let items = vec![(0x80u8, 2u8), (0x7eu8, 1u8), (0x8au8, 4u8)];
        let patch = build_seed_patch(&items);
        let decoded = StartingInventory::decode_region(&patch);
        assert_eq!(decoded.items(), items.as_slice());
    }

    #[test]
    fn empty_patch_is_all_nops_and_decodes_to_nothing() {
        let patch = build_seed_patch(&[]);
        assert!(patch.iter().all(|&b| b == 0));
        assert!(StartingInventory::decode_region(&patch).is_empty());
    }

    #[test]
    fn five_random_items_round_trip() {
        let items = plan_starting_items(7, 5);
        let patch = build_seed_patch(&items);
        assert_eq!(StartingInventory::decode_region(&patch).items(), &items[..]);
    }
}
