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

use legaia_asset::new_game::{
    DOOR_OF_WIND_ITEM, INVENTORY_SC_OFFSET, STARTING_INV_SEED_LEN, WARP_ALL_FLAGS_HI,
    WARP_ALL_FLAGS_LO, WARP_FLAGS_SC_OFFSET,
};

use crate::rng::SplitMix64;

/// Number of MIPS instructions the reclaimable seed region holds (40 bytes / 4).
const SEED_INSTRS: usize = STARTING_INV_SEED_LEN / 4;

/// Instructions one inventory slot costs: one `addiu $v0,(count<<8)|id` + one
/// `sh $v0,off($s0)`.
const INSTRS_PER_ITEM: usize = 2;

/// Instructions the all-warps preset costs: two `addiu`/`sh` pairs (the low and
/// high halfwords of the visited-towns bitmask at [`WARP_FLAGS_SC_OFFSET`]).
const WARP_FLAG_INSTRS: usize = 4;

/// Most starting-item slots the reclaimable seed region can hold: ten
/// instructions / two per item (`addiu` + `sh`).
pub const MAX_STARTING_ITEMS: usize = SEED_INSTRS / INSTRS_PER_ITEM;

/// Item id of Door of Wind (the warp consumable), re-exported for callers.
pub const DOOR_OF_WIND_ID: u8 = DOOR_OF_WIND_ITEM;

/// Default Door of Wind stack seeded when the toggle is enabled without an
/// explicit count. Door of Wind is consumed per warp, so a small stack keeps it
/// useful for a while without the GameShark "99 in one slot" overkill; the user
/// can override it.
pub const DOOR_OF_WIND_COUNT: u8 = 10;

/// Maximum stack the game holds in one inventory slot. A seeded Door-of-Wind
/// count is clamped to this (the count byte is written raw, so a larger value
/// would either overflow the display or be capped by the bag anyway).
pub const MAX_ITEM_STACK: u8 = 99;

/// The vanilla New Game starting item: Healing Leaf (`0x77`) ×5. Preserved as
/// the base slot when a convenience toggle is on but no random reroll was
/// requested, so the toggles read as *additional* to a normal new game.
pub const VANILLA_STARTING_ITEM: (u8, u8) = (0x77, 5);

/// Default `(min, max)` random count for each seeded item (inclusive). Modest,
/// so a random start is helpful without trivializing the early game; vanilla
/// seeds five Healing Leaves.
pub const DEFAULT_COUNT_RANGE: (u8, u8) = (1, 5);

/// Most inventory slots the seed region can hold once the all-warps preset (if
/// enabled) has claimed its share of the instruction budget. Without warps the
/// full [`MAX_STARTING_ITEMS`]; with warps, fewer (the 4 warp instructions
/// leave 6 → 3 slots).
pub fn max_items_with_warps(all_warps: bool) -> usize {
    let warp = if all_warps { WARP_FLAG_INSTRS } else { 0 };
    ((SEED_INSTRS - warp) / INSTRS_PER_ITEM).min(MAX_STARTING_ITEMS)
}

/// What the New Game starting seed should set, beyond the vanilla Healing Leaf.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartingSeedOptions {
    /// Number of random consumables to reroll the starting bag to. `0` keeps
    /// the vanilla Healing Leaf base (so the convenience toggles stay additive).
    pub random_items: usize,
    /// How many Door of Wind to seed into the starting bag. `0` = off; any
    /// positive count seeds one slot of Door of Wind, clamped to
    /// [`MAX_ITEM_STACK`]. (The CLI / web default when the toggle is enabled is
    /// [`DOOR_OF_WIND_COUNT`].)
    pub door_of_wind: u8,
    /// Preset the all-towns visited-towns bitmask so Door of Wind can warp to
    /// every destination from the start.
    pub all_warps: bool,
}

impl StartingSeedOptions {
    /// `true` when the seed should be rewritten at all (any toggle is set).
    pub fn is_active(&self) -> bool {
        self.random_items > 0 || self.door_of_wind > 0 || self.all_warps
    }
}

/// A resolved starting-seed plan: the warp preset plus the concrete inventory
/// slots to write, already clamped to the region's capacity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedPlan {
    /// Whether to write the all-warps bitmask preset.
    pub all_warps: bool,
    /// The `(id, count)` inventory slots in slot order.
    pub items: Vec<(u8, u8)>,
}

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
    plan_random_items(seed, n.min(MAX_STARTING_ITEMS), &[])
}

/// Plan `n` random starting items, excluding any id in `exclude` from the draw
/// (so a forced item like Door of Wind isn't also dealt a duplicate slot). `n`
/// is clamped to the (filtered) pool size. Deterministic in `(seed, n, exclude)`.
fn plan_random_items(seed: u64, n: usize, exclude: &[u8]) -> Vec<(u8, u8)> {
    let mut rng = SplitMix64::new(seed ^ 0x5247_4E49_5453_5452); // "RGNITSTR"-ish salt
    let mut pool: Vec<u8> = STARTING_ITEM_POOL
        .iter()
        .copied()
        .filter(|id| !exclude.contains(id))
        .collect();
    rng.shuffle(&mut pool);
    let (lo, hi) = DEFAULT_COUNT_RANGE;
    let span = (hi - lo) as usize + 1;
    pool.into_iter()
        .take(n.min(MAX_STARTING_ITEMS))
        .map(|id| (id, lo + rng.below(span) as u8))
        .collect()
}

/// Resolve [`StartingSeedOptions`] into a concrete [`SeedPlan`] for `seed`.
///
/// Composition, in slot order:
/// 1. **Door of Wind** (`door_of_wind`× , clamped to [`MAX_ITEM_STACK`]) if the
///    count is non-zero, written first so it always survives the capacity clamp.
/// 2. **Base / reroll**: with `random_items == 0` the vanilla Healing Leaf base
///    is kept (the convenience toggles stay additive to a normal new game);
///    with `random_items > 0` the bag is rerolled to that many random
///    consumables instead (the existing `--starting-items` behaviour), drawn
///    excluding Door of Wind so it isn't duplicated.
///
/// The whole list is clamped to [`max_items_with_warps`] so the warp preset
/// (if enabled) always has room. Deterministic in `(seed, opts)`.
pub fn plan_seed(seed: u64, opts: &StartingSeedOptions) -> SeedPlan {
    let cap = max_items_with_warps(opts.all_warps);
    let mut items: Vec<(u8, u8)> = Vec::new();

    let door_count = opts.door_of_wind.min(MAX_ITEM_STACK);
    if door_count > 0 {
        items.push((DOOR_OF_WIND_ID, door_count));
    }

    if opts.random_items > 0 {
        let room = cap.saturating_sub(items.len());
        let n = opts.random_items.min(room);
        // Exclude the forced item from the reroll so it isn't dealt twice; when
        // Door of Wind isn't forced it stays an eligible random consumable.
        let exclude: &[u8] = if door_count > 0 {
            &[DOOR_OF_WIND_ID]
        } else {
            &[]
        };
        items.extend(plan_random_items(seed, n, exclude));
    } else if items.len() < cap {
        // No reroll requested: keep the vanilla Healing Leaf base so the
        // toggles are purely additive.
        items.push(VANILLA_STARTING_ITEM);
    }

    items.truncate(cap);
    SeedPlan {
        all_warps: opts.all_warps,
        items,
    }
}

/// Encode a list of `(id, count)` starting items into the 40-byte seed patch
/// (no warp preset). Convenience wrapper over [`build_seed_patch_for`].
pub fn build_seed_patch(items: &[(u8, u8)]) -> [u8; STARTING_INV_SEED_LEN] {
    build_seed_patch_for(&SeedPlan {
        all_warps: false,
        items: items.to_vec(),
    })
}

/// Encode a [`SeedPlan`] into the 40-byte seed patch.
///
/// When `all_warps` is set, emits the two `addiu`/`sh` pairs that preset the
/// visited-towns bitmask ([`WARP_FLAGS_SC_OFFSET`]) first. Then one
/// `addiu $v0, $zero, (count << 8) | id` + `sh $v0, (0x1818 + 2k)($s0)` pair per
/// inventory slot, padded to [`STARTING_INV_SEED_LEN`] with `nop`. Panics if the
/// plan exceeds the [`SEED_INSTRS`]-instruction budget (callers clamp first via
/// [`plan_seed`]). The inventory base offset comes from [`INVENTORY_SC_OFFSET`].
pub fn build_seed_patch_for(plan: &SeedPlan) -> [u8; STARTING_INV_SEED_LEN] {
    let mut words: Vec<u32> = Vec::with_capacity(SEED_INSTRS);
    if plan.all_warps {
        // addiu sign-extends the immediate, but `sh` stores only the low 16
        // bits, so the high 0xFFFF.. fill is harmless.
        words.push(addiu_v0(WARP_ALL_FLAGS_LO));
        words.push(sh_v0_s0(WARP_FLAGS_SC_OFFSET as u16));
        words.push(addiu_v0(WARP_ALL_FLAGS_HI));
        words.push(sh_v0_s0(WARP_FLAGS_SC_OFFSET as u16 + 2));
    }
    for (slot, &(id, count)) in plan.items.iter().enumerate() {
        let off = (INVENTORY_SC_OFFSET as usize + slot * 2) as u16;
        words.push(addiu_v0(((count as u16) << 8) | id as u16));
        words.push(sh_v0_s0(off));
    }
    assert!(
        words.len() <= SEED_INSTRS,
        "seed plan needs {} instructions but only {SEED_INSTRS} fit the region",
        words.len()
    );
    while words.len() < SEED_INSTRS {
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

    #[test]
    fn warps_cut_the_item_capacity_to_three() {
        assert_eq!(max_items_with_warps(false), MAX_STARTING_ITEMS);
        assert_eq!(max_items_with_warps(false), 5);
        assert_eq!(max_items_with_warps(true), 3);
    }

    #[test]
    fn door_of_wind_only_is_additive_to_the_vanilla_base() {
        // No reroll, no warps: keep Healing Leaf AND add Door of Wind x10.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: DOOR_OF_WIND_COUNT,
                all_warps: false,
            },
        );
        assert!(!plan.all_warps);
        assert_eq!(
            plan.items,
            vec![(DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT), VANILLA_STARTING_ITEM]
        );
        let patch = build_seed_patch_for(&plan);
        assert!(!legaia_asset::new_game::region_unlocks_all_warps(&patch));
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &[(DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT), VANILLA_STARTING_ITEM]
        );
    }

    #[test]
    fn door_of_wind_count_is_user_settable_and_clamped() {
        // An explicit count is seeded verbatim.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 25,
                all_warps: false,
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, 25));
        // A count above the stack cap clamps to MAX_ITEM_STACK.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 250,
                all_warps: false,
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, MAX_ITEM_STACK));
        // Count 0 means off: no Door of Wind, just the vanilla base.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 0,
                all_warps: false,
            },
        );
        assert_eq!(plan.items, vec![VANILLA_STARTING_ITEM]);
        assert!(!plan.items.iter().any(|(id, _)| *id == DOOR_OF_WIND_ID));
    }

    #[test]
    fn all_warps_emits_the_bitmask_and_keeps_the_base() {
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 0,
                all_warps: true,
            },
        );
        assert!(plan.all_warps);
        assert_eq!(plan.items, vec![VANILLA_STARTING_ITEM]);
        let patch = build_seed_patch_for(&plan);
        assert!(legaia_asset::new_game::region_unlocks_all_warps(&patch));
        // The inventory base still decodes alongside the warp preset.
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &[VANILLA_STARTING_ITEM]
        );
    }

    #[test]
    fn door_plus_warps_plus_reroll_clamps_to_three_slots() {
        // Request 5 random + door of wind + warps: cap is 3, door takes one,
        // leaving room for 2 random items.
        let opts = StartingSeedOptions {
            random_items: 5,
            door_of_wind: DOOR_OF_WIND_COUNT,
            all_warps: true,
        };
        let plan = plan_seed(99, &opts);
        assert!(plan.all_warps);
        assert_eq!(plan.items.len(), 3, "clamped to the 3-slot warp budget");
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT));
        // The two random fills are distinct consumables, never Door of Wind.
        for (id, _) in &plan.items[1..] {
            assert!(STARTING_ITEM_POOL.contains(id));
            assert_ne!(*id, DOOR_OF_WIND_ID, "reroll excludes the forced item");
        }
        let patch = build_seed_patch_for(&plan);
        assert!(legaia_asset::new_game::region_unlocks_all_warps(&patch));
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &plan.items[..]
        );
    }

    #[test]
    fn reroll_replaces_the_vanilla_base() {
        // With a reroll requested, the Healing Leaf base is dropped.
        let plan = plan_seed(
            3,
            &StartingSeedOptions {
                random_items: 3,
                door_of_wind: 0,
                all_warps: false,
            },
        );
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items, plan_starting_items(3, 3));
    }

    #[test]
    fn plan_seed_is_deterministic() {
        let opts = StartingSeedOptions {
            random_items: 4,
            door_of_wind: DOOR_OF_WIND_COUNT,
            all_warps: true,
        };
        assert_eq!(plan_seed(0xABCD, &opts), plan_seed(0xABCD, &opts));
    }

    #[test]
    fn inactive_options_are_detected() {
        assert!(!StartingSeedOptions::default().is_active());
        assert!(
            StartingSeedOptions {
                all_warps: true,
                ..Default::default()
            }
            .is_active()
        );
    }
}
